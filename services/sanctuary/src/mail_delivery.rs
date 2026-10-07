//! Durable account mail delivery. SMTP acceptance is at least once: a crash after
//! DATA acceptance and before the database update may cause a retry. Message-ID
//! remains tied to the job UUID. Neither transport errors nor payloads are logged.
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart},
    transport::smtp::{authentication::Credentials, client::Tls},
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::{net::IpAddr, sync::Arc, time::Duration};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const PAYLOAD_LIMIT: usize = 1024;
const CIPHER_LIMIT: usize = PAYLOAD_LIMIT + 41;
const MAX_ATTEMPTS: i32 = 8;
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailKind {
    ResetPassword,
    VerifyEmail,
}
impl MailKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResetPassword => "reset",
            Self::VerifyEmail => "verify",
        }
    }
    fn parse(value: &str) -> Result<Self, MailError> {
        match value {
            "reset" => Ok(Self::ResetPassword),
            "verify" => Ok(Self::VerifyEmail),
            _ => Err(MailError("mail_payload_invalid")),
        }
    }
    fn path(self) -> &'static str {
        match self {
            Self::ResetPassword => "/reset-password",
            Self::VerifyEmail => "/verify-email",
        }
    }
}

/// Display and Debug deliberately contain only a safe, static operator code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailError(pub &'static str);
impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for MailError {}

pub struct MailConfig {
    enabled: Option<EnabledMail>,
}
struct EnabledMail {
    origin: String,
    message_domain: String,
    from: Address,
    cipher: XChaCha20Poly1305,
    transport: AsyncSmtpTransport<Tokio1Executor>,
}
impl std::fmt::Debug for MailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MailConfig")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

impl MailConfig {
    pub fn disabled() -> Self {
        Self { enabled: None }
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled.is_some()
    }
    /// Read environment once during AppState construction. Partial or unsafe
    /// configuration is a startup error; an entirely unset mail setup is off.
    pub fn from_env(origin: &str) -> Result<Self, MailError> {
        Self::from_values(origin, |key| std::env::var(key).unwrap_or_default())
    }
    fn from_values(origin: &str, get: impl Fn(&str) -> String) -> Result<Self, MailError> {
        let host = get("SMTP_HOST");
        let from = get("SMTP_FROM");
        let username = get("SMTP_USERNAME");
        let password = Zeroizing::new(get("SMTP_PASSWORD"));
        let key_text = Zeroizing::new(get("MAIL_ENCRYPTION_KEY"));
        let port_text = get("SMTP_PORT");
        let tls_mode = get("SMTP_TLS");
        let allow_local = match get("MAIL_ALLOW_LOCAL_DEV").as_str() {
            "" | "false" => false,
            "true" => true,
            _ => return Err(MailError("mail_local_dev_flag_invalid")),
        };
        if [&host, &from, &username, &password, &port_text]
            .iter()
            .all(|v| v.is_empty())
        {
            return Ok(Self::disabled());
        }
        if host.is_empty() || from.is_empty() || key_text.is_empty() {
            return Err(MailError("mail_configuration_incomplete"));
        }
        if host.len() > 253
            || host.chars().any(|c| c.is_whitespace() || c.is_control())
            || host.contains(['/', '@', '?', '#'])
            || (host.contains(':') && host.parse::<IpAddr>().is_err())
        {
            return Err(MailError("mail_smtp_host_invalid"));
        }
        if username.is_empty() != password.is_empty()
            || username.len() > 254
            || password.len() > 4096
            || username.chars().any(char::is_control)
        {
            return Err(MailError("mail_smtp_credentials_invalid"));
        }
        let origin = validated_origin(origin, allow_local)?;
        let from = checked_address(&from).map_err(|_| MailError("mail_from_invalid"))?;
        let key = parse_key(&key_text)?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| MailError("mail_encryption_key_invalid"))?;
        let tls_mode = if tls_mode.is_empty() {
            "starttls"
        } else {
            &tls_mode
        };
        let (builder, default_port) = match tls_mode {
            "starttls" => (
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
                    .map_err(|_| MailError("mail_smtp_tls_configuration_invalid"))?,
                587,
            ),
            "implicit" => (
                AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
                    .map_err(|_| MailError("mail_smtp_tls_configuration_invalid"))?,
                465,
            ),
            "none" if allow_local && is_loopback_host(&host) => (
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&host).tls(Tls::None),
                25,
            ),
            "none" => return Err(MailError("mail_plaintext_requires_local_dev")),
            _ => return Err(MailError("mail_smtp_tls_mode_invalid")),
        };
        let port = if port_text.is_empty() {
            default_port
        } else {
            port_text
                .parse::<u16>()
                .ok()
                .filter(|v| *v != 0)
                .ok_or(MailError("mail_smtp_port_invalid"))?
        };
        let builder = builder.port(port).timeout(Some(Duration::from_secs(10)));
        let builder = if username.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(username, password.to_string()))
        };
        let website =
            reqwest::Url::parse(&origin).map_err(|_| MailError("mail_public_origin_invalid"))?;
        // An IP literal is not a useful Message-ID domain; it still binds AAD.
        let message_domain = match website.host_str() {
            Some(host) if host.parse::<IpAddr>().is_err() && !host.starts_with('[') => {
                host.to_owned()
            }
            _ => "illyaverse.local".to_owned(),
        };
        Ok(Self {
            enabled: Some(EnabledMail {
                origin,
                message_domain,
                from,
                cipher,
                transport: builder.build(),
            }),
        })
    }
    #[cfg(test)]
    pub(crate) fn local_for_test(
        origin: &str,
        address: std::net::SocketAddr,
        key: [u8; 32],
    ) -> Self {
        assert!(
            address.ip().is_loopback(),
            "test mail server must be loopback"
        );
        let key = key
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self::from_values(origin, |name| match name {
            "SMTP_HOST" => address.ip().to_string(),
            "SMTP_PORT" => address.port().to_string(),
            "SMTP_FROM" => "noreply@illyaverse.test".into(),
            "SMTP_TLS" => "none".into(),
            "MAIL_ALLOW_LOCAL_DEV" => "true".into(),
            "MAIL_ENCRYPTION_KEY" => key.clone(),
            _ => String::new(),
        })
        .expect("valid local SMTP fixture configuration")
    }
    /// The transaction creating the token must insert this ciphertext into its
    /// mail job. Changing id, user, purpose, origin, or key invalidates decryption.
    pub fn encrypt_job(
        &self,
        id: Uuid,
        user_id: i64,
        kind: MailKind,
        email: &str,
        locale: &str,
        token: &str,
    ) -> Result<Vec<u8>, MailError> {
        let config = self.enabled.as_ref().ok_or(MailError("mail_disabled"))?;
        if user_id <= 0 {
            return Err(MailError("mail_payload_invalid"));
        }
        checked_address(email)?;
        if !(32..=128).contains(&token.len())
            || !token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            || locale.len() > 16
        {
            return Err(MailError("mail_payload_invalid"));
        }
        let payload = MailPayload {
            email: email.into(),
            locale: match locale {
                "zh-TW" | "ja" => locale.into(),
                _ => "en".into(),
            },
            token: token.into(),
        };
        let plain = Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|_| MailError("mail_payload_invalid"))?,
        );
        if plain.len() > PAYLOAD_LIMIT {
            return Err(MailError("mail_payload_too_large"));
        }
        let mut nonce = [0u8; 24];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| MailError("mail_randomness_unavailable"))?;
        let encrypted = config
            .cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &plain,
                    aad: &associated_data(&config.origin, id, user_id, kind),
                },
            )
            .map_err(|_| MailError("mail_encrypt_failed"))?;
        let mut result = Vec::with_capacity(25 + encrypted.len());
        result.push(1);
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&encrypted);
        Ok(result)
    }
}

fn parse_key(value: &str) -> Result<Zeroizing<[u8; 32]>, MailError> {
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(MailError("mail_encryption_key_invalid"));
    }
    let mut key = Zeroizing::new([0u8; 32]);
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| MailError("mail_encryption_key_invalid"))?;
    }
    if key.iter().all(|byte| *byte == 0) {
        return Err(MailError("mail_encryption_key_invalid"));
    }
    Ok(key)
}
fn checked_address(value: &str) -> Result<Address, MailError> {
    if value.is_empty()
        || value.len() > 254
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(MailError("mail_payload_invalid"));
    }
    value
        .parse::<Address>()
        .map_err(|_| MailError("mail_payload_invalid"))
}
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}
fn validated_origin(origin: &str, allow_local: bool) -> Result<String, MailError> {
    let url = reqwest::Url::parse(origin).map_err(|_| MailError("mail_public_origin_invalid"))?;
    let host = url
        .host_str()
        .ok_or(MailError("mail_public_origin_invalid"))?;
    let local = is_loopback_host(host) || host.ends_with(".test");
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.scheme() != "https" && !(url.scheme() == "http" && local && allow_local))
    {
        return Err(MailError("mail_public_origin_requires_https"));
    }
    Ok(url.origin().ascii_serialization())
}
fn associated_data(origin: &str, id: Uuid, user_id: i64, kind: MailKind) -> Vec<u8> {
    format!(
        "illyaverse.mail.v1\n{origin}\n{id}\n{user_id}\n{}",
        kind.as_str()
    )
    .into_bytes()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailPayload {
    email: String,
    locale: String,
    token: String,
}
impl Drop for MailPayload {
    fn drop(&mut self) {
        self.email.zeroize();
        self.token.zeroize();
    }
}
impl EnabledMail {
    fn decrypt_job(
        &self,
        id: Uuid,
        user_id: i64,
        kind: MailKind,
        encrypted: &[u8],
    ) -> Result<MailPayload, MailError> {
        if !(41..=CIPHER_LIMIT).contains(&encrypted.len()) || encrypted[0] != 1 {
            return Err(MailError("mail_payload_invalid"));
        }
        let nonce: [u8; 24] = encrypted[1..25]
            .try_into()
            .map_err(|_| MailError("mail_payload_invalid"))?;
        let plain = Zeroizing::new(
            self.cipher
                .decrypt(
                    &XNonce::from(nonce),
                    Payload {
                        msg: &encrypted[25..],
                        aad: &associated_data(&self.origin, id, user_id, kind),
                    },
                )
                .map_err(|_| MailError("mail_payload_authentication_failed"))?,
        );
        serde_json::from_slice(&plain).map_err(|_| MailError("mail_payload_invalid"))
    }
    fn message(
        &self,
        id: Uuid,
        kind: MailKind,
        payload: &MailPayload,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Message, MailError> {
        let recipient = checked_address(&payload.email)?;
        let link = Zeroizing::new(format!(
            "{}{}#token={}",
            self.origin,
            kind.path(),
            payload.token
        ));
        let (subject, prompt, button, safety) = template_copy(kind, &payload.locale);
        let link = link.as_str();
        let plain = format!(
            "{prompt}\n\n{link}\n\n{safety}\n\nIllyaverse\n{}",
            self.origin
        );
        let html = format!(
            "<!doctype html><html lang=\"{}\"><body style=\"margin:0;padding:24px;font-family:Arial,sans-serif;background:#000;color:#f5f5f5\"><main style=\"max-width:560px;margin:auto\"><p style=\"color:#a1a1aa\">Illyaverse</p><h1 style=\"font-size:24px\">{subject}</h1><p>{prompt}</p><p><a href=\"{link}\" style=\"display:inline-block;padding:12px 18px;border-radius:12px;background:#f5f5f5;color:#09090b;text-decoration:none\">{button}</a></p><p style=\"word-break:break-all\"><a href=\"{link}\" style=\"color:#d4d4d8\">{link}</a></p><p style=\"color:#a1a1aa\">{safety}</p></main></body></html>",
            payload.locale,
        );
        Message::builder()
            .from(Mailbox::new(Some("Illyaverse".into()), self.from.clone()))
            .to(Mailbox::new(None, recipient))
            .subject(subject)
            .message_id(Some(format!("<account-{id}@{}>", self.message_domain)))
            .date(created_at.into())
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(plain))
                    .singlepart(SinglePart::html(html)),
            )
            .map_err(|_| MailError("mail_message_invalid"))
    }
}

fn template_copy(
    kind: MailKind,
    locale: &str,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match (kind, locale) {
        (MailKind::ResetPassword, "zh-TW") => (
            "重設 Illyaverse 密碼",
            "你要求重設 Illyaverse 帳號密碼。請開啟下方連結，確認網站網址後設定新密碼。",
            "設定新密碼",
            "此連結只能使用一次並會到期。若你沒有提出這項要求，可以忽略這封郵件；你的密碼尚未變更。",
        ),
        (MailKind::VerifyEmail, "zh-TW") => (
            "驗證 Illyaverse 電子郵件",
            "請開啟下方連結，確認網站網址後驗證你的 Illyaverse 電子郵件地址。",
            "驗證電子郵件",
            "此連結只能使用一次並會到期。若你沒有提出這項要求，可以忽略這封郵件。",
        ),
        (MailKind::ResetPassword, "ja") => (
            "Illyaverse のパスワード再設定",
            "Illyaverse のパスワード再設定がリクエストされました。以下のリンクを開き、サイトの URL を確認して新しいパスワードを設定してください。",
            "新しいパスワードを設定",
            "このリンクは一度だけ使用でき、有効期限があります。心当たりがない場合は、このメールを無視してください。パスワードはまだ変更されていません。",
        ),
        (MailKind::VerifyEmail, "ja") => (
            "Illyaverse のメールアドレス確認",
            "以下のリンクを開き、サイトの URL を確認して Illyaverse のメールアドレスを確認してください。",
            "メールアドレスを確認",
            "このリンクは一度だけ使用でき、有効期限があります。心当たりがない場合は、このメールを無視してください。",
        ),
        (MailKind::ResetPassword, _) => (
            "Reset your Illyaverse password",
            "You requested a password reset for Illyaverse. Open the link below, check the website address, and choose a new password.",
            "Choose a new password",
            "This link expires and can only be used once. If you did not request this, ignore this email. Your password has not changed.",
        ),
        (MailKind::VerifyEmail, _) => (
            "Verify your Illyaverse email",
            "Open the link below and check the website address to verify your Illyaverse email address.",
            "Verify email",
            "This link expires and can only be used once. If you did not request this, ignore this email.",
        ),
    }
}

pub fn start(db: PgPool, config: Arc<MailConfig>) {
    tokio::spawn(async move {
        loop {
            let pause = match process_one(&db, &config).await {
                Ok(true) => Duration::from_millis(250),
                Ok(false) => Duration::from_secs(5),
                Err(_) => {
                    tracing::warn!(code = "mail_database_unavailable", "mail delivery deferred");
                    Duration::from_secs(5)
                }
            };
            tokio::time::sleep(pause).await;
        }
    });
}

/// Claim at most one job with SKIP LOCKED. Leases recover after worker restarts.
/// The retry uses the same ciphertext, job UUID, and Message-ID. Do not log the
/// returned database error with its source chain or any row fields.
pub async fn process_one(db: &PgPool, config: &MailConfig) -> Result<bool, sqlx::Error> {
    // Retention and stale lease cleanup continue when SMTP has been disabled.
    sqlx::query("WITH stale AS (SELECT id FROM account_recovery_tokens WHERE COALESCE(consumed_at,expires_at)<now()-interval '7 days' ORDER BY COALESCE(consumed_at,expires_at) LIMIT 32 FOR UPDATE SKIP LOCKED) DELETE FROM account_recovery_tokens t USING stale s WHERE t.id=s.id")
        .execute(db).await?;
    sqlx::query("WITH stale AS (SELECT recipient_hash FROM account_recovery_limits WHERE window_started_at<now()-interval '24 hours' ORDER BY window_started_at LIMIT 32 FOR UPDATE SKIP LOCKED) DELETE FROM account_recovery_limits l USING stale s WHERE l.recipient_hash=s.recipient_hash")
        .execute(db).await?;
    // Bound expiration cleanup separately from the single delivery claim.
    sqlx::query("WITH expired AS (SELECT j.id FROM mail_jobs j JOIN account_recovery_tokens t ON t.id=j.token_id JOIN users u ON u.id=j.user_id WHERE j.status IN ('pending','sending') AND (t.consumed_at IS NOT NULL OR t.expires_at<=now() OR t.user_id<>j.user_id OR t.purpose<>j.kind OR t.email_snapshot<>u.email OR u.is_restricted) ORDER BY j.created_at LIMIT 32 FOR UPDATE OF j SKIP LOCKED) UPDATE mail_jobs j SET status='canceled',encrypted_payload=NULL,lease_id=NULL,lease_until=NULL,last_error_code='mail_token_inactive' FROM expired e WHERE j.id=e.id")
        .execute(db).await?;
    sqlx::query("WITH exhausted AS (SELECT id FROM mail_jobs WHERE attempts>=8 AND (status='pending' OR (status='sending' AND lease_until<=now())) ORDER BY created_at LIMIT 32 FOR UPDATE SKIP LOCKED) UPDATE mail_jobs j SET status='failed',encrypted_payload=NULL,lease_id=NULL,lease_until=NULL,last_error_code='mail_attempts_exhausted' FROM exhausted e WHERE j.id=e.id")
        .execute(db).await?;
    let Some(config) = config.enabled.as_ref() else {
        return Ok(false);
    };
    let lease = Uuid::new_v4();
    let row = sqlx::query("WITH candidate AS (SELECT j.id FROM mail_jobs j JOIN account_recovery_tokens t ON t.id=j.token_id JOIN users u ON u.id=j.user_id WHERE ((j.status='pending' AND j.available_at<=now()) OR (j.status='sending' AND j.lease_until<=now())) AND j.attempts<8 AND j.encrypted_payload IS NOT NULL AND t.user_id=j.user_id AND t.purpose=j.kind AND t.email_snapshot=u.email AND t.expires_at>now() AND t.consumed_at IS NULL AND NOT u.is_restricted ORDER BY j.available_at,j.created_at,j.id LIMIT 1 FOR UPDATE OF j SKIP LOCKED) UPDATE mail_jobs j SET status='sending',attempts=attempts+1,lease_id=$1,lease_until=now()+interval '90 seconds',last_error_code=NULL FROM candidate c WHERE j.id=c.id RETURNING j.id,j.user_id,j.kind,j.encrypted_payload,j.attempts,j.created_at")
        .bind(lease).fetch_optional(db).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let id: Uuid = row.get("id");
    let user_id: i64 = row.get("user_id");
    let attempts: i32 = row.get("attempts");
    let kind = MailKind::parse(row.get::<&str, _>("kind"));
    let payload = kind.and_then(|kind| {
        config.decrypt_job(
            id,
            user_id,
            kind,
            &row.get::<Vec<u8>, _>("encrypted_payload"),
        )
    });
    let (Ok(kind), Ok(payload)) = (kind, payload) else {
        finish_failure(db, id, lease, attempts, "mail_payload_invalid", true).await?;
        return Ok(true);
    };
    let Some(token_hash) = crate::account_recovery::canonical_token_hash(&payload.token) else {
        finish_failure(db, id, lease, attempts, "mail_payload_invalid", true).await?;
        return Ok(true);
    };
    // Revalidate after decryption and immediately before SMTP. A concurrent
    // consume after this check still invalidates the emailed one-time link.
    // Bind the decrypted secret to the referenced token row as well as its
    // authenticated job identity: substituting token_id cannot send a wrong link.
    let current_email: Option<String> = sqlx::query_scalar("SELECT u.email FROM mail_jobs j JOIN account_recovery_tokens t ON t.id=j.token_id JOIN users u ON u.id=j.user_id WHERE j.id=$1 AND j.status='sending' AND j.lease_id=$2 AND j.lease_until>now() AND t.token_hash=$3 AND t.user_id=j.user_id AND t.purpose=j.kind AND t.email_snapshot=u.email AND t.expires_at>now() AND t.consumed_at IS NULL AND NOT u.is_restricted")
        .bind(id).bind(lease).bind(token_hash).fetch_optional(db).await?;
    if current_email.as_deref() != Some(&payload.email) {
        sqlx::query("UPDATE mail_jobs SET status='canceled',encrypted_payload=NULL,lease_id=NULL,lease_until=NULL,last_error_code='mail_token_inactive' WHERE id=$1 AND status='sending' AND lease_id=$2")
            .bind(id).bind(lease).execute(db).await?;
        return Ok(true);
    }
    let message = config.message(id, kind, &payload, row.get("created_at"));
    let Ok(message) = message else {
        finish_failure(db, id, lease, attempts, "mail_message_invalid", true).await?;
        return Ok(true);
    };
    let result = tokio::time::timeout(DELIVERY_TIMEOUT, config.transport.send(message)).await;
    match result {
        Ok(Ok(_)) => {
            sqlx::query("UPDATE mail_jobs SET status='sent',encrypted_payload=NULL,sent_at=now(),lease_id=NULL,lease_until=NULL,last_error_code=NULL WHERE id=$1 AND status='sending' AND lease_id=$2")
                .bind(id).bind(lease).execute(db).await?;
        }
        Ok(Err(error)) => {
            let permanent = error.is_permanent();
            let code = if error.is_tls() {
                "mail_smtp_tls_failed"
            } else if permanent {
                "mail_smtp_rejected"
            } else if error.is_timeout() {
                "mail_smtp_timeout"
            } else {
                "mail_smtp_unavailable"
            };
            finish_failure(db, id, lease, attempts, code, permanent).await?;
        }
        Err(_) => {
            finish_failure(db, id, lease, attempts, "mail_smtp_timeout", false).await?;
        }
    }
    Ok(true)
}
fn retry_seconds(attempts: i32) -> i32 {
    30_i32
        .saturating_mul(1_i32 << (attempts - 1).clamp(0, 7))
        .min(3600)
}
async fn finish_failure(
    db: &PgPool,
    id: Uuid,
    lease: Uuid,
    attempts: i32,
    code: &'static str,
    permanent: bool,
) -> Result<(), sqlx::Error> {
    let terminal = permanent || attempts >= MAX_ATTEMPTS;
    sqlx::query("UPDATE mail_jobs SET status=CASE WHEN $3 THEN 'failed' ELSE 'pending' END,encrypted_payload=CASE WHEN $3 THEN NULL ELSE encrypted_payload END,available_at=now()+make_interval(secs=>$4),lease_id=NULL,lease_until=NULL,last_error_code=$5 WHERE id=$1 AND status='sending' AND lease_id=$2")
        .bind(id).bind(lease).bind(terminal).bind(retry_seconds(attempts)).bind(code).execute(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    fn values(overrides: &[(&str, &str)]) -> HashMap<String, String> {
        let mut result = HashMap::from([
            ("SMTP_HOST".into(), "127.0.0.1".into()),
            ("SMTP_FROM".into(), "noreply@illyaverse.test".into()),
            ("MAIL_ENCRYPTION_KEY".into(), "11".repeat(32)),
        ]);
        for (key, value) in overrides {
            result.insert((*key).into(), (*value).into());
        }
        result
    }
    fn config(origin: &str, values: &HashMap<String, String>) -> Result<MailConfig, MailError> {
        MailConfig::from_values(origin, |key| values.get(key).cloned().unwrap_or_default())
    }
    fn fixture_payload(locale: &str) -> MailPayload {
        MailPayload {
            email: "fixture@recipient.test".into(),
            locale: locale.into(),
            token: TOKEN.into(),
        }
    }

    #[test]
    fn mail_configuration_is_explicit_and_secret_safe() {
        let off = HashMap::from([("MAIL_ENCRYPTION_KEY".into(), "11".repeat(32))]);
        assert!(!config("http://localhost:3000", &off).unwrap().is_enabled());
        assert_eq!(
            format!("{:?}", config("", &off).unwrap()),
            "MailConfig { enabled: false }"
        );
        for missing in ["SMTP_HOST", "SMTP_FROM", "MAIL_ENCRYPTION_KEY"] {
            let mut settings = values(&[]);
            settings.remove(missing);
            assert_eq!(
                config("https://illya.test", &settings).unwrap_err().0,
                "mail_configuration_incomplete"
            );
        }
        let settings = values(&[("SMTP_USERNAME", "a-private-username")]);
        let error = config("https://illya.test", &settings).unwrap_err();
        assert!(!format!("{error:?} {error}").contains("a-private"));
        assert_eq!(error.0, "mail_smtp_credentials_invalid");
        for invalid in ["", "00", &"00".repeat(32), &"zz".repeat(32)] {
            let settings = values(&[("MAIL_ENCRYPTION_KEY", invalid)]);
            assert!(config("https://illya.test", &settings).is_err());
        }
        assert!(
            config("https://illya.test", &values(&[]))
                .unwrap()
                .is_enabled()
        );
        assert!(
            config("https://illya.test", &values(&[("SMTP_TLS", "implicit")]))
                .unwrap()
                .is_enabled()
        );
    }

    #[test]
    fn plaintext_and_http_require_local_development() {
        for origin in [
            "http://localhost:3000",
            "http://illya.test",
            "http://example.com",
        ] {
            assert_eq!(
                config(origin, &values(&[])).unwrap_err().0,
                "mail_public_origin_requires_https"
            );
        }
        for origin in [
            "https://user:secret@illya.test",
            "https://illya.test/path",
            "https://illya.test/?q=1",
            "https://illya.test/#token=secret",
        ] {
            assert!(config(origin, &values(&[])).is_err());
        }
        for host in [
            "smtp.example.com",
            "192.168.1.2",
            "0.0.0.0",
            "host.docker.internal",
        ] {
            let settings = values(&[
                ("SMTP_HOST", host),
                ("SMTP_TLS", "none"),
                ("MAIL_ALLOW_LOCAL_DEV", "true"),
            ]);
            assert_eq!(
                config("http://localhost:3000", &settings).unwrap_err().0,
                "mail_plaintext_requires_local_dev"
            );
        }
        for host in ["127.0.0.1", "::1", "localhost"] {
            let settings = values(&[
                ("SMTP_HOST", host),
                ("SMTP_TLS", "none"),
                ("MAIL_ALLOW_LOCAL_DEV", "true"),
            ]);
            assert!(config("http://illya.test", &settings).unwrap().is_enabled());
        }
        let settings = values(&[("SMTP_TLS", "none")]);
        assert_eq!(
            config("https://illya.test", &settings).unwrap_err().0,
            "mail_plaintext_requires_local_dev"
        );
    }

    #[test]
    fn job_encryption_authenticates_identity_key_and_origin() {
        let config = config("https://illya.test", &values(&[])).unwrap();
        let enabled = config.enabled.as_ref().unwrap();
        let id = Uuid::new_v4();
        let ciphertext = config
            .encrypt_job(
                id,
                12,
                MailKind::ResetPassword,
                "fixture@recipient.test",
                "ja",
                TOKEN,
            )
            .unwrap();
        let other = config
            .encrypt_job(
                id,
                12,
                MailKind::ResetPassword,
                "fixture@recipient.test",
                "ja",
                TOKEN,
            )
            .unwrap();
        assert_ne!(ciphertext, other, "nonces must differ between encryptions");
        assert!(
            !ciphertext
                .windows(TOKEN.len())
                .any(|slice| slice == TOKEN.as_bytes())
        );
        assert!(ciphertext.len() <= CIPHER_LIMIT);
        let plain = enabled
            .decrypt_job(id, 12, MailKind::ResetPassword, &ciphertext)
            .unwrap();
        assert_eq!(plain.token, TOKEN);
        assert_eq!(plain.locale, "ja");
        assert!(
            enabled
                .decrypt_job(Uuid::new_v4(), 12, MailKind::ResetPassword, &ciphertext)
                .is_err()
        );
        assert!(
            enabled
                .decrypt_job(id, 13, MailKind::ResetPassword, &ciphertext)
                .is_err()
        );
        assert!(
            enabled
                .decrypt_job(id, 12, MailKind::VerifyEmail, &ciphertext)
                .is_err()
        );
        let changed_origin = super::tests::config("https://another.test", &values(&[])).unwrap();
        assert!(
            changed_origin
                .enabled
                .as_ref()
                .unwrap()
                .decrypt_job(id, 12, MailKind::ResetPassword, &ciphertext)
                .is_err()
        );
        let changed_key = super::tests::config(
            "https://illya.test",
            &values(&[("MAIL_ENCRYPTION_KEY", &"22".repeat(32))]),
        )
        .unwrap();
        assert!(
            changed_key
                .enabled
                .as_ref()
                .unwrap()
                .decrypt_job(id, 12, MailKind::ResetPassword, &ciphertext)
                .is_err()
        );
        for index in [0, 1, 24, 25, ciphertext.len() - 1] {
            let mut changed = ciphertext.clone();
            changed[index] ^= 1;
            assert!(
                enabled
                    .decrypt_job(id, 12, MailKind::ResetPassword, &changed)
                    .is_err()
            );
        }
        assert!(
            enabled
                .decrypt_job(id, 12, MailKind::ResetPassword, &[0; CIPHER_LIMIT + 1])
                .is_err()
        );
    }

    #[test]
    fn payload_and_addresses_are_bounded_and_not_header_injectable() {
        let config = config("https://illya.test", &values(&[])).unwrap();
        let id = Uuid::new_v4();
        for email in [
            "recipient@test\r\nBcc: attacker@test",
            "a b@test",
            "",
            &"a".repeat(255),
        ] {
            assert!(
                config
                    .encrypt_job(id, 1, MailKind::VerifyEmail, email, "en", TOKEN)
                    .is_err()
            );
        }
        for token in [
            "short",
            "<script>alert()</script>",
            &"a".repeat(129),
            &"a".repeat(31),
        ] {
            assert!(
                config
                    .encrypt_job(
                        id,
                        1,
                        MailKind::VerifyEmail,
                        "fixture@recipient.test",
                        "en",
                        token
                    )
                    .is_err()
            );
        }
        assert!(
            config
                .encrypt_job(
                    id,
                    1,
                    MailKind::VerifyEmail,
                    "fixture@recipient.test",
                    &"j".repeat(17),
                    TOKEN
                )
                .is_err()
        );
        assert!(
            config
                .encrypt_job(
                    id,
                    0,
                    MailKind::VerifyEmail,
                    "fixture@recipient.test",
                    "en",
                    TOKEN
                )
                .is_err()
        );
        assert_eq!(retry_seconds(1), 30);
        assert_eq!(retry_seconds(2), 60);
        assert_eq!(retry_seconds(8), 3600);
    }

    #[derive(Default)]
    struct Wire {
        commands: Vec<String>,
        data: Vec<u8>,
    }
    async fn smtp_server(listener: TcpListener) -> Wire {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut result = Wire::default();
        stream
            .get_mut()
            .write_all(b"220 illyaverse-fixture ESMTP\r\n")
            .await
            .unwrap();
        loop {
            let mut line = String::new();
            if stream.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            assert!(line.len() <= 2048);
            result.commands.push(line.trim_end().to_string());
            let reply = if line.starts_with("EHLO") {
                b"250-illyaverse-fixture\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n".as_slice()
            } else if line.starts_with("DATA") {
                stream
                    .get_mut()
                    .write_all(b"354 send message\r\n")
                    .await
                    .unwrap();
                loop {
                    let mut data_line = Vec::new();
                    assert_ne!(stream.read_until(b'\n', &mut data_line).await.unwrap(), 0);
                    if data_line == b".\r\n" {
                        break;
                    }
                    result.data.extend(data_line);
                    assert!(result.data.len() < 16_384);
                }
                b"250 queued\r\n".as_slice()
            } else if line.starts_with("QUIT") {
                stream.get_mut().write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                b"250 accepted\r\n".as_slice()
            };
            stream.get_mut().write_all(reply).await.unwrap();
        }
        result
    }

    #[tokio::test]
    async fn smtp_wire_has_envelope_localized_alternatives_and_fragment_link() {
        for locale in ["zh-TW", "en", "ja"] {
            for kind in [MailKind::ResetPassword, MailKind::VerifyEmail] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let config = MailConfig::local_for_test(
                    "http://localhost:3000",
                    listener.local_addr().unwrap(),
                    [17; 32],
                );
                let handle = tokio::spawn(smtp_server(listener));
                let enabled = config.enabled.as_ref().unwrap();
                let id = Uuid::new_v4();
                let payload = fixture_payload(locale);
                let message = enabled
                    .message(id, kind, &payload, chrono::Utc::now())
                    .unwrap();
                enabled.transport.send(message).await.unwrap();
                let wire = tokio::time::timeout(Duration::from_secs(5), handle)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    wire.commands
                        .iter()
                        .any(|command| command.starts_with("MAIL FROM:<noreply@illyaverse.test>"))
                );
                assert!(
                    wire.commands
                        .iter()
                        .any(|command| command == "RCPT TO:<fixture@recipient.test>")
                );
                assert!(
                    !wire
                        .commands
                        .iter()
                        .any(|command| command.starts_with("AUTH"))
                );
                let body = String::from_utf8(wire.data).unwrap();
                assert!(body.contains("multipart/alternative"));
                assert!(body.contains("text/plain; charset=utf-8"));
                assert!(body.contains("text/html; charset=utf-8"));
                assert!(body.contains(&format!("Message-ID: <account-{id}@localhost>")));
                let parsed = mailparse::parse_mail(body.as_bytes()).unwrap();
                assert_eq!(parsed.subparts.len(), 2);
                let (subject, prompt, button, _) = template_copy(kind, locale);
                use mailparse::MailHeaderMap;
                assert_eq!(
                    parsed.headers.get_first_value("Subject").as_deref(),
                    Some(subject)
                );
                for part in &parsed.subparts {
                    let decoded = part.get_body().unwrap();
                    assert!(decoded.contains(&format!(
                        "http://localhost:3000{}#token={TOKEN}",
                        kind.path()
                    )));
                    assert!(decoded.contains(prompt));
                    assert!(!decoded.contains("?token="));
                    assert!(!decoded.contains("<img"));
                    if part.ctype.mimetype == "text/html" {
                        assert!(decoded.contains(button));
                        assert!(decoded.contains(&format!("lang=\"{locale}\"")));
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn required_starttls_refuses_credentials_and_envelope_on_downgrade() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let settings = values(&[
            ("SMTP_PORT", &port),
            ("SMTP_USERNAME", "private-fixture-name"),
            ("SMTP_PASSWORD", "private-fixture-password"),
        ]);
        let config = config("https://illya.test", &settings).unwrap();
        let handle = tokio::spawn(smtp_server(listener));
        let enabled = config.enabled.as_ref().unwrap();
        let message = enabled
            .message(
                Uuid::new_v4(),
                MailKind::VerifyEmail,
                &fixture_payload("en"),
                chrono::Utc::now(),
            )
            .unwrap();
        assert!(enabled.transport.send(message).await.is_err());
        let wire = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(wire.commands.iter().any(|line| line.starts_with("EHLO")));
        assert!(!wire.commands.iter().any(|line| line.starts_with("AUTH")
            || line.starts_with("MAIL")
            || line.starts_with("RCPT")
            || line.starts_with("DATA")));
        assert!(wire.data.is_empty());
        assert!(!wire.commands.join(" ").contains("private-fixture"));
    }
}
