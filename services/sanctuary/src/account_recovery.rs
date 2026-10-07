use crate::{ApiError, ApiResult, AppState, auth, mail_delivery::MailKind};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, Utc};
use illya_domain::validate_password;
use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const INVALID_LINK: &str = "This recovery link is invalid, expired or already used.";
const REQUEST_MESSAGE: &str =
    "If this account can receive recovery email, a link will arrive shortly.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Reset,
    Verify,
}
impl Purpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reset => "reset",
            Self::Verify => "verify",
        }
    }
    fn lifetime(self) -> Duration {
        match self {
            Self::Reset => Duration::minutes(15),
            Self::Verify => Duration::hours(24),
        }
    }
    fn mail_kind(self) -> MailKind {
        match self {
            Self::Reset => MailKind::ResetPassword,
            Self::Verify => MailKind::VerifyEmail,
        }
    }
}

fn invalid_token() -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        "invalid_recovery_token",
        INVALID_LINK.into(),
    )
}

fn require_enabled(state: &AppState) -> ApiResult<()> {
    if state.mail.is_enabled() {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "account_recovery_unavailable",
            "Email account recovery is not configured on this server.".into(),
        ))
    }
}

pub(crate) fn normalize_email(value: &str) -> ApiResult<String> {
    let email = value.trim().to_ascii_lowercase();
    if email.len() > 254
        || email.parse::<lettre::Address>().is_err()
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ApiError::bad("Enter a valid email address."));
    }
    Ok(email)
}

/// The wire token is the lowercase hexadecimal encoding of 32 random bytes.
/// Accept a single canonical representation so links cannot acquire aliases.
pub(crate) fn recovery_token_shape(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_token(value: &str) -> ApiResult<[u8; 32]> {
    if !recovery_token_shape(value) {
        return Err(invalid_token());
    }
    fn nibble(byte: u8) -> u8 {
        if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte - b'a' + 10
        }
    }
    let mut output = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Ok(output)
}

fn token_hash(raw: &[u8; 32]) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(b"illyaverse-recovery-token\0");
    hash.update(raw);
    hash.finalize().to_vec()
}

/// Mail delivery can bind its decrypted token to the linked database row without
/// exposing a token or an error containing input data to SQL or tracing.
pub(crate) fn canonical_token_hash(value: &str) -> Option<Vec<u8>> {
    let decoded = Zeroizing::new(decode_token(value).ok()?);
    Some(token_hash(&decoded))
}

fn recipient_hash(email: &str) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(b"illyaverse-recovery-recipient\0");
    hash.update(email.as_bytes());
    hash.finalize().to_vec()
}

fn new_token() -> ApiResult<(String, Vec<u8>)> {
    let mut bytes = [0; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ApiError::internal("Recovery randomness is unavailable."))?;
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let digest = token_hash(&bytes);
    bytes.zeroize();
    Ok((encoded, digest))
}

struct RecipientRate {
    window_started_at: DateTime<Utc>,
    requests: i32,
    last_requested_at: Option<DateTime<Utc>>,
}
fn allowed_rate(rate: &RecipientRate, now: DateTime<Utc>) -> Option<(DateTime<Utc>, i32)> {
    let (window, requests) = if now >= rate.window_started_at + Duration::hours(1) {
        (now, 0)
    } else {
        (rate.window_started_at, rate.requests)
    };
    if requests >= 5
        || rate
            .last_requested_at
            .is_some_and(|last| now < last + Duration::seconds(60))
    {
        None
    } else {
        Some((window, requests + 1))
    }
}

async fn permit_recipient(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    now: DateTime<Utc>,
) -> ApiResult<bool> {
    let hash = recipient_hash(email);
    sqlx::query("INSERT INTO account_recovery_limits(recipient_hash,window_started_at) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(&hash).bind(now).execute(&mut **tx).await?;
    let row = sqlx::query("SELECT window_started_at,requests,last_requested_at FROM account_recovery_limits WHERE recipient_hash=$1 FOR UPDATE")
        .bind(&hash).fetch_one(&mut **tx).await?;
    let rate = RecipientRate {
        window_started_at: row.get("window_started_at"),
        requests: row.get("requests"),
        last_requested_at: row.get("last_requested_at"),
    };
    let Some((window, requests)) = allowed_rate(&rate, now) else {
        return Ok(false);
    };
    sqlx::query("UPDATE account_recovery_limits SET window_started_at=$2,requests=$3,last_requested_at=$4 WHERE recipient_hash=$1")
        .bind(hash).bind(window).bind(requests).bind(now).execute(&mut **tx).await?;
    Ok(true)
}

async fn cancel_purpose(
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
    purpose: Purpose,
) -> ApiResult<()> {
    sqlx::query("UPDATE account_recovery_tokens SET consumed_at=COALESCE(consumed_at,clock_timestamp()) WHERE user_id=$1 AND purpose=$2")
        .bind(user_id).bind(purpose.as_str()).execute(&mut **tx).await?;
    sqlx::query("UPDATE mail_jobs SET status='canceled',encrypted_payload=NULL,lease_until=NULL,lease_id=NULL,last_error_code=NULL WHERE user_id=$1 AND kind=$2 AND status IN ('pending','sending','failed')")
        .bind(user_id).bind(purpose.as_str()).execute(&mut **tx).await?;
    Ok(())
}

/// The recipient lock precedes the user lock in every issuance path. Confirmation
/// and authenticated password changes lock the user before touching their tokens.
async fn issue(
    state: &AppState,
    email: &str,
    user_id: Option<i64>,
    purpose: Purpose,
    requested_locale: Option<&str>,
) -> ApiResult<bool> {
    issue_with_actor(state, email, user_id, purpose, requested_locale, None).await
}

async fn issue_with_actor(
    state: &AppState,
    email: &str,
    user_id: Option<i64>,
    purpose: Purpose,
    requested_locale: Option<&str>,
    actor: Option<&auth::User>,
) -> ApiResult<bool> {
    let mut tx = state.db.begin().await?;
    let queued = issue_in_tx(
        state,
        &mut tx,
        email,
        user_id,
        purpose,
        requested_locale,
        actor,
    )
    .await?;
    tx.commit().await?;
    Ok(queued)
}

async fn issue_in_tx(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    user_id: Option<i64>,
    purpose: Purpose,
    requested_locale: Option<&str>,
    actor: Option<&auth::User>,
) -> ApiResult<bool> {
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut **tx)
        .await?;
    let permitted = permit_recipient(tx, email, now).await?;
    // Public issuance locks recipient before user. Keep that order here, even
    // for rate denial, so no stale session commits limiter/token/job writes.
    let actor = if let Some(actor) = actor {
        Some(auth::mutation_actor(tx, actor, &[], auth::MutationScope::Session).await?)
    } else {
        None
    };
    let queued = if permitted {
        queue_in_tx(state, tx, email, user_id, purpose, requested_locale, now).await?
    } else {
        false
    };
    if let Some(actor) = actor {
        auth::ensure_mutation_actor(tx, &actor).await?;
    }
    Ok(queued)
}

async fn queue_in_tx(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    user_id: Option<i64>,
    purpose: Purpose,
    requested_locale: Option<&str>,
    now: DateTime<Utc>,
) -> ApiResult<bool> {
    let row = sqlx::query("SELECT id,email,preferences,is_restricted,email_verified_at FROM users WHERE lower(email)=$1 AND ($2::bigint IS NULL OR id=$2) FOR UPDATE")
        .bind(email).bind(user_id).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    if row.get::<bool, _>("is_restricted")
        || (purpose == Purpose::Verify
            && row
                .get::<Option<DateTime<Utc>>, _>("email_verified_at")
                .is_some())
    {
        return Ok(false);
    }
    let user_id = row.get::<i64, _>("id");
    let email = row.get::<String, _>("email");
    let preferences: Value = row.get("preferences");
    let locale = requested_locale.unwrap_or_else(|| {
        preferences
            .get("locale")
            .and_then(Value::as_str)
            .filter(|value| matches!(*value, "zh-TW" | "en" | "ja"))
            .unwrap_or("zh-TW")
    });
    let (token, hash) = new_token()?;
    let mut token = Zeroizing::new(token);
    let token_id = Uuid::new_v4();
    let job_id = Uuid::new_v4();
    let encrypted = state
        .mail
        .encrypt_job(job_id, user_id, purpose.mail_kind(), &email, locale, &token)
        .map_err(|_| ApiError::internal("Recovery mail encryption failed."))?;
    token.zeroize();
    cancel_purpose(tx, user_id, purpose).await?;
    sqlx::query("INSERT INTO account_recovery_tokens(id,user_id,purpose,email_snapshot,token_hash,expires_at) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(token_id).bind(user_id).bind(purpose.as_str()).bind(email).bind(hash)
        .bind(now+purpose.lifetime()).execute(&mut **tx).await?;
    sqlx::query(
        "INSERT INTO mail_jobs(id,token_id,user_id,kind,encrypted_payload) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(job_id)
    .bind(token_id)
    .bind(user_id)
    .bind(purpose.as_str())
    .bind(encrypted)
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

fn accepted() -> Response {
    (
        StatusCode::ACCEPTED,
        Json(json!({"ok":true,"message":REQUEST_MESSAGE})),
    )
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetRequest {
    email: String,
    locale: Option<String>,
}
pub async fn request_reset(
    State(state): State<AppState>,
    Json(input): Json<ResetRequest>,
) -> ApiResult<Response> {
    require_enabled(&state)?;
    let email = normalize_email(&input.email)?;
    if input
        .locale
        .as_deref()
        .is_some_and(|locale| !matches!(locale, "zh-TW" | "en" | "ja"))
    {
        return Err(ApiError::bad("Unsupported locale."));
    }
    let started = tokio::time::Instant::now();
    let mut random = [0; 1];
    OsRng
        .try_fill_bytes(&mut random)
        .map_err(|_| ApiError::internal("Recovery randomness is unavailable."))?;
    let delay = std::time::Duration::from_millis(300 + u64::from(random[0] % 101));
    let _ = issue(
        &state,
        &email,
        None,
        Purpose::Reset,
        input.locale.as_deref(),
    )
    .await?;
    // Mail is queued, never sent in this handler. The same modest response floor
    // covers the extra transaction work for existing addresses without hashing
    // attacker-selected passwords on this public issuance endpoint.
    tokio::time::sleep_until(started + delay).await;
    Ok(accepted())
}

/// Disabled delivery is a deliberate no-op; it never produces a recovery token
/// with nowhere to send it. Registration uses the transaction helper below.
pub(crate) async fn enqueue_verification(state: &AppState, actor: &auth::User) -> ApiResult<bool> {
    if !state.mail.is_enabled() {
        return Ok(false);
    }
    let email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(actor.id)
        .fetch_optional(&state.db)
        .await?;
    let email = email.ok_or_else(ApiError::unauthorized)?;
    issue_with_actor(
        state,
        &normalize_email(&email)?,
        Some(actor.id),
        Purpose::Verify,
        None,
        Some(actor),
    )
    .await
}

/// Account creation uses the caller's transaction so a queue encryption/database
/// error cannot leave an account durably created behind a failed registration.
pub(crate) async fn enqueue_verification_in_tx(
    state: &AppState,
    tx: &mut Transaction<'_, Postgres>,
    user_id: i64,
) -> ApiResult<bool> {
    if !state.mail.is_enabled() {
        return Ok(false);
    }
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
    issue_in_tx(
        state,
        tx,
        &normalize_email(&email)?,
        Some(user_id),
        Purpose::Verify,
        None,
        None,
    )
    .await
}

pub async fn request_verification(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    let user = auth::require(&state, &headers).await?;
    require_enabled(&state)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct EmptyRequest {}
    if !body.is_empty() {
        serde_json::from_slice::<EmptyRequest>(&body)
            .map_err(|_| ApiError::bad("Email verification requests do not accept any fields."))?;
    }
    let _ = enqueue_verification(&state, &user).await?;
    Ok(accepted())
}

struct TokenRecord {
    purpose: String,
    email_snapshot: String,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}
fn valid_binding(
    token: &TokenRecord,
    purpose: Purpose,
    current_email: &str,
    restricted: bool,
    now: DateTime<Utc>,
) -> bool {
    !restricted
        && token.purpose == purpose.as_str()
        && token.email_snapshot == current_email
        && token.consumed_at.is_none()
        && token.expires_at > now
}

/// Resolve the owner without a token lock, then take user/token locks in that
/// order. A concurrent reset or resend will be observed after those locks.
async fn lock_token(
    tx: &mut Transaction<'_, Postgres>,
    hash: &[u8],
    purpose: Purpose,
) -> ApiResult<(Uuid, i64)> {
    let owner: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM account_recovery_tokens WHERE token_hash=$1")
            .bind(hash)
            .fetch_optional(&mut **tx)
            .await?;
    let owner = owner.ok_or_else(invalid_token)?;
    let user = sqlx::query("SELECT email,is_restricted FROM users WHERE id=$1 FOR UPDATE")
        .bind(owner)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(invalid_token)?;
    let row = sqlx::query("SELECT id,purpose,email_snapshot,expires_at,consumed_at,clock_timestamp() AS checked_at FROM account_recovery_tokens WHERE token_hash=$1 AND user_id=$2 FOR UPDATE")
        .bind(hash).bind(owner).fetch_optional(&mut **tx).await?.ok_or_else(invalid_token)?;
    let token = TokenRecord {
        purpose: row.get("purpose"),
        email_snapshot: row.get("email_snapshot"),
        expires_at: row.get("expires_at"),
        consumed_at: row.get("consumed_at"),
    };
    if !valid_binding(
        &token,
        purpose,
        &user.get::<String, _>("email"),
        user.get("is_restricted"),
        row.get("checked_at"),
    ) {
        return Err(invalid_token());
    }
    Ok((row.get("id"), owner))
}

async fn consume_token(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> ApiResult<()> {
    let consumed = sqlx::query("UPDATE account_recovery_tokens SET consumed_at=clock_timestamp() WHERE id=$1 AND consumed_at IS NULL AND expires_at>clock_timestamp()")
        .bind(id).execute(&mut **tx).await?.rows_affected();
    if consumed == 1 {
        Ok(())
    } else {
        Err(invalid_token())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetConfirmation {
    token: String,
    password: String,
}
pub async fn confirm_reset(
    State(state): State<AppState>,
    Json(mut input): Json<ResetConfirmation>,
) -> ApiResult<Response> {
    require_enabled(&state)?;
    validate_password(&input.password).map_err(ApiError::bad)?;
    let decoded = Zeroizing::new(decode_token(&input.token)?);
    input.token.zeroize();
    let hash = token_hash(&decoded);
    // Avoid hashing attacker-selected passwords for unknown/invalid links, and
    // keep Argon2 outside user/token locks. The locked binding is checked again
    // below, so this preflight cannot authorize a consumed or changed token.
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM account_recovery_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=$1 AND t.purpose=$2 AND t.consumed_at IS NULL AND t.expires_at>clock_timestamp() AND t.email_snapshot=u.email AND NOT u.is_restricted)",
    )
    .bind(&hash)
    .bind(Purpose::Reset.as_str())
    .fetch_one(&state.db)
    .await?;
    if !valid {
        return Err(invalid_token());
    }
    let (password_hash, legacy_hash) = auth::hash_password(input.password).await?;
    let mut tx = state.db.begin().await?;
    let (token_id, user_id) = lock_token(&mut tx, &hash, Purpose::Reset).await?;
    consume_token(&mut tx, token_id).await?;
    sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
        .bind(user_id)
        .bind(password_hash)
        .bind(legacy_hash)
        .execute(&mut *tx)
        .await?;
    auth::revoke_sessions(&mut tx, user_id, None).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.password_reset','user',$2,'{\"all_sessions_revoked\":true}')")
        .bind(user_id).bind(user_id.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    let cookie = format!(
        "illya_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if state.secure_cookie { "; Secure" } else { "" }
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({"ok":true,"sessions_revoked":true})),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationConfirmation {
    token: String,
}
pub async fn confirm_verification(
    State(state): State<AppState>,
    Json(mut input): Json<VerificationConfirmation>,
) -> ApiResult<Response> {
    require_enabled(&state)?;
    let decoded = Zeroizing::new(decode_token(&input.token)?);
    input.token.zeroize();
    let hash = token_hash(&decoded);
    let mut tx = state.db.begin().await?;
    let (token_id, user_id) = lock_token(&mut tx, &hash, Purpose::Verify).await?;
    consume_token(&mut tx, token_id).await?;
    sqlx::query("UPDATE users SET email_verified_at=COALESCE(email_verified_at,clock_timestamp()) WHERE id=$1")
        .bind(user_id).execute(&mut *tx).await?;
    cancel_purpose(&mut tx, user_id, Purpose::Verify).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id) VALUES($1,'users.email_verify','user',$2)")
        .bind(user_id).bind(user_id.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"email_verified":true})).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    use axum::body::to_bytes;
    use futures_util::FutureExt;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::{collections::HashMap, panic::AssertUnwindSafe, str::FromStr, sync::Arc};
    use tokio::sync::Mutex;

    #[test]
    fn recovery_token_is_canonical_and_domain_separated() {
        let raw = [0xa5; 32];
        let encoded = "a5".repeat(32);
        assert_eq!(decode_token(&encoded).unwrap(), raw);
        assert!(!recovery_token_shape(&encoded.to_uppercase()));
        assert!(!recovery_token_shape(&format!(" {encoded}")));
        assert!(!recovery_token_shape(&"g0".repeat(32)));
        assert!(!recovery_token_shape(&"a5".repeat(31)));
        assert!(!recovery_token_shape(&"a5".repeat(33)));
        assert!(!recovery_token_shape(&"🪄".repeat(16)));
        assert_eq!(token_hash(&raw).len(), 32);
        assert_ne!(token_hash(&raw), raw);
        assert_ne!(token_hash(&raw), recipient_hash(&encoded));
        let mut modified = raw;
        modified[31] ^= 1;
        assert_ne!(token_hash(&raw), token_hash(&modified));
    }

    #[test]
    fn generated_tokens_have_independent_entropy() {
        let first = new_token().unwrap();
        let second = new_token().unwrap();
        assert_ne!(first.0, second.0);
        assert_ne!(first.1, second.1);
        assert!(recovery_token_shape(&first.0));
        assert_eq!(first.1, token_hash(&decode_token(&first.0).unwrap()));
    }

    #[test]
    fn token_cannot_cross_purpose_email_restriction_expiry_or_consumption() {
        let now = Utc::now();
        let mut token = TokenRecord {
            purpose: "reset".into(),
            email_snapshot: "player@example.invalid".into(),
            expires_at: now + Duration::minutes(15),
            consumed_at: None,
        };
        assert!(valid_binding(
            &token,
            Purpose::Reset,
            &token.email_snapshot,
            false,
            now
        ));
        assert!(!valid_binding(
            &token,
            Purpose::Verify,
            &token.email_snapshot,
            false,
            now
        ));
        assert!(!valid_binding(
            &token,
            Purpose::Reset,
            "changed@example.invalid",
            false,
            now
        ));
        assert!(!valid_binding(
            &token,
            Purpose::Reset,
            &token.email_snapshot,
            true,
            now
        ));
        assert!(!valid_binding(
            &token,
            Purpose::Reset,
            &token.email_snapshot,
            false,
            token.expires_at
        ));
        token.consumed_at = Some(now);
        assert!(!valid_binding(
            &token,
            Purpose::Reset,
            &token.email_snapshot,
            false,
            now
        ));
    }

    #[test]
    fn recipient_limit_combines_cooldown_and_hourly_window() {
        let now = Utc::now();
        let mut rate = RecipientRate {
            window_started_at: now,
            requests: 0,
            last_requested_at: None,
        };
        assert_eq!(allowed_rate(&rate, now), Some((now, 1)));
        rate.requests = 1;
        rate.last_requested_at = Some(now);
        assert!(allowed_rate(&rate, now + Duration::seconds(59)).is_none());
        assert_eq!(
            allowed_rate(&rate, now + Duration::seconds(60)),
            Some((now, 2))
        );
        rate.requests = 5;
        assert!(allowed_rate(&rate, now + Duration::minutes(59)).is_none());
        let next_hour = now + Duration::hours(1);
        assert_eq!(allowed_rate(&rate, next_hour), Some((next_hour, 1)));
        rate.last_requested_at = Some(next_hour - Duration::seconds(30));
        assert!(allowed_rate(&rate, next_hour).is_none());
    }

    #[test]
    fn bounded_email_and_payload_inputs_do_not_allow_extra_fields() {
        assert_eq!(
            normalize_email(" Player@Example.Invalid ").unwrap(),
            "player@example.invalid"
        );
        for invalid in [
            "missing-at",
            "a@@b",
            "x\n@example.invalid",
            "x y@example.invalid",
            &format!("{}@x", "a".repeat(254)),
        ] {
            assert!(normalize_email(invalid).is_err());
        }
        assert!(
            serde_json::from_value::<ResetRequest>(json!({"email":"x@x","token":"extra"})).is_err()
        );
        assert!(
            serde_json::from_value::<ResetConfirmation>(
                json!({"token":"a5".repeat(32),"password":"a long password","user_id":1})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<VerificationConfirmation>(
                json!({"token":"a5".repeat(32),"email":"other@x"})
            )
            .is_err()
        );
        assert_eq!(Purpose::Reset.lifetime(), Duration::minutes(15));
        assert_eq!(Purpose::Verify.lifetime(), Duration::hours(24));
    }

    async fn seed_token(state: &AppState, user_id: i64, purpose: Purpose) -> String {
        let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await
            .unwrap();
        let (token, digest) = new_token().unwrap();
        let id = Uuid::new_v4();
        let job_id = Uuid::new_v4();
        let encrypted = state
            .mail
            .encrypt_job(job_id, user_id, purpose.mail_kind(), &email, "ja", &token)
            .unwrap();
        assert!(
            !encrypted
                .windows(token.len())
                .any(|bytes| bytes == token.as_bytes())
        );
        assert!(
            !encrypted
                .windows(email.len())
                .any(|bytes| bytes == email.as_bytes())
        );
        let mut tx = state.db.begin().await.unwrap();
        sqlx::query("INSERT INTO account_recovery_tokens(id,user_id,purpose,email_snapshot,token_hash,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '15 minutes')")
            .bind(id).bind(user_id).bind(purpose.as_str()).bind(email).bind(digest)
            .execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO mail_jobs(id,token_id,user_id,kind,encrypted_payload) VALUES($1,$2,$3,$4,$5)")
            .bind(job_id).bind(id).bind(user_id).bind(purpose.as_str()).bind(encrypted)
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        token
    }

    async fn reset(state: &AppState, token: &str, password: &str) -> ApiResult<Response> {
        confirm_reset(
            State(state.clone()),
            Json(ResetConfirmation {
                token: token.into(),
                password: password.into(),
            }),
        )
        .await
    }

    async fn verify_link(state: &AppState, token: &str) -> ApiResult<Response> {
        confirm_verification(
            State(state.clone()),
            Json(VerificationConfirmation {
                token: token.into(),
            }),
        )
        .await
    }

    async fn assert_invalid_reset(state: &AppState, token: &str) {
        let error = reset(state, token, "valid replacement password")
            .await
            .unwrap_err();
        assert_eq!(error.0, StatusCode::BAD_REQUEST);
        assert_eq!(error.1, "invalid_recovery_token");
        assert_eq!(error.2, INVALID_LINK);
    }

    #[tokio::test]
    #[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
    async fn account_recovery_security() {
        let url =
            std::env::var("ILLYA_TEST_DATABASE_URL").expect("Set the disposable database URL");
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let suffix = Uuid::new_v4().simple().to_string();
        let schema = format!("illya_recovery_{suffix}");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let application_name = format!("illya-recovery-{suffix}");
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema.as_str())])
            .application_name(&application_name);
        let db = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        let outcome = AssertUnwindSafe(async {
            sqlx::migrate!("../../migrations").run(&db).await.unwrap();
            let state = AppState {
                db: db.clone(),
                secret: Arc::new("isolated-recovery-test-session-secret-with-32-bytes".into()),
                internal_key: Arc::new("isolated-recovery-test-internal-secret-with-32-bytes".into()),
                origin: Arc::new("http://localhost:3000".into()),
                mail: Arc::new(crate::mail_delivery::MailConfig::local_for_test(
                    "http://localhost:3000", "127.0.0.1:1".parse().unwrap(), [17; 32],
                )),
                secure_cookie: false,
                media_root: std::env::temp_dir().join(format!("illya-recovery-{suffix}-media")),
                maps_root: std::env::temp_dir().join(format!("illya-recovery-{suffix}-maps")),
                replay_root: std::env::temp_dir().join(format!("illya-recovery-{suffix}-replays")),
                client: reqwest::Client::new(),
                official: Arc::new(Mutex::new(crate::maps::OfficialState::default())),
                limits: Arc::new(Mutex::new(HashMap::new())),
                trusted_proxies: Arc::new(Vec::new()),
                upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            };
            // This configuration encrypts only. No worker or SMTP connection is
            // started by the test, and no media directories are created.
            let original_password = "original recovery test password";
            let (hash, legacy_hash) = auth::hash_password(original_password.into()).await.unwrap();
            let mut ids = Vec::new();
            for (name, restricted) in [("RecoverAlice", false), ("RecoverBlocked", true), ("RecoverLimit", false), ("RecoverConcurrent", false)] {
                let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash,is_restricted,preferences) VALUES($1,$2,$3,$4,$5,'{\"locale\":\"ja\"}') RETURNING id")
                    .bind(name).bind(format!("{}@example.invalid", name.to_ascii_lowercase()))
                    .bind(&hash).bind(&legacy_hash).bind(restricted).fetch_one(&db).await.unwrap();
                sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'").bind(id).execute(&db).await.unwrap();
                ids.push(id);
            }
            let alice = ids[0];
            let blocked = ids[1];
            let mut responses = Vec::new();
            for email in ["recoveralice@example.invalid", "missing@example.invalid", "recoverblocked@example.invalid", "recoveralice@example.invalid"] {
                let started = tokio::time::Instant::now();
                let response = request_reset(State(state.clone()), Json(ResetRequest { email: email.into(), locale: Some("en".into()) })).await.unwrap();
                assert_eq!(response.status(), StatusCode::ACCEPTED);
                assert!(started.elapsed() >= std::time::Duration::from_millis(300));
                responses.push(to_bytes(response.into_body(), 1024).await.unwrap());
            }
            assert!(responses.windows(2).all(|pair| pair[0] == pair[1]));
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1").bind(alice).fetch_one(&db).await.unwrap(), 1);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1").bind(blocked).fetch_one(&db).await.unwrap(), 0);
            let response_json: Value = serde_json::from_slice(&responses[0]).unwrap();
            assert!(response_json.get("token").is_none());
            assert!(response_json.get("email").is_none());
            assert!(sqlx::query_scalar::<_,bool>("SELECT bool_and(octet_length(token_hash)=32) FROM account_recovery_tokens").fetch_one(&db).await.unwrap());

            // The recipient row lock makes concurrent issuance admit only one
            // request, even when separate API transactions race.
            let (first, second) = tokio::join!(
                issue(&state, "recoverconcurrent@example.invalid", None, Purpose::Reset, None),
                issue(&state, "recoverconcurrent@example.invalid", None, Purpose::Reset, None),
            );
            assert_ne!(first.unwrap(), second.unwrap());
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mail_jobs WHERE user_id=$1").bind(ids[3]).fetch_one(&db).await.unwrap(), 1);
            for _ in 0..5 {
                assert!(issue(&state, "recoverlimit@example.invalid", None, Purpose::Reset, None).await.unwrap());
                sqlx::query("UPDATE account_recovery_limits SET last_requested_at=now()-interval '2 minutes' WHERE recipient_hash=$1")
                    .bind(recipient_hash("recoverlimit@example.invalid")).execute(&db).await.unwrap();
            }
            assert!(!issue(&state, "recoverlimit@example.invalid", None, Purpose::Verify, None).await.unwrap());
            assert_eq!(sqlx::query_scalar::<_,i32>("SELECT requests FROM account_recovery_limits WHERE recipient_hash=$1").bind(recipient_hash("recoverlimit@example.invalid")).fetch_one(&db).await.unwrap(), 5);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1 AND consumed_at IS NULL").bind(ids[2]).fetch_one(&db).await.unwrap(), 1);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mail_jobs WHERE user_id=$1 AND status='canceled' AND encrypted_payload IS NULL").bind(ids[2]).fetch_one(&db).await.unwrap(), 4);

            let wrong_purpose = seed_token(&state, alice, Purpose::Verify).await;
            assert_invalid_reset(&state, &wrong_purpose).await;
            let wrong_hash = token_hash(&decode_token(&wrong_purpose).unwrap());
            assert!(sqlx::query_scalar::<_,Option<DateTime<Utc>>>("SELECT consumed_at FROM account_recovery_tokens WHERE token_hash=$1").bind(wrong_hash).fetch_one(&db).await.unwrap().is_none());
            let expired = seed_token(&state, alice, Purpose::Reset).await;
            sqlx::query("UPDATE account_recovery_tokens SET created_at=now()-interval '1 day',expires_at=now()-interval '1 second' WHERE token_hash=$1")
                .bind(token_hash(&decode_token(&expired).unwrap())).execute(&db).await.unwrap();
            assert_invalid_reset(&state, &expired).await;
            let changed_email = seed_token(&state, alice, Purpose::Reset).await;
            sqlx::query("UPDATE account_recovery_tokens SET email_snapshot='old-address@example.invalid' WHERE token_hash=$1")
                .bind(token_hash(&decode_token(&changed_email).unwrap())).execute(&db).await.unwrap();
            assert_invalid_reset(&state, &changed_email).await;
            let restricted = seed_token(&state, blocked, Purpose::Reset).await;
            assert_invalid_reset(&state, &restricted).await;
            assert_invalid_reset(&state, &"01".repeat(32)).await;

            // All three authentication channels are populated before reset.
            for _ in 0..2 {
                sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,now()+interval '1 day')").bind(Uuid::new_v4()).bind(alice).execute(&db).await.unwrap();
            }
            sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at) VALUES($1,'reset-test-stable-token',$2,now()+interval '1 day')")
                .bind(Uuid::new_v4()).bind(alice).execute(&db).await.unwrap();
            sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,'reset-test-lazer-access','reset-test-lazer-refresh',now()+interval '1 hour',now()+interval '1 day')")
                .bind(Uuid::new_v4()).bind(alice).execute(&db).await.unwrap();
            let valid = seed_token(&state, alice, Purpose::Reset).await;
            let replacement_password = "new independent recovery password";
            let response = reset(&state, &valid, replacement_password).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(response.headers()[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
            for table in ["sessions", "bancho_sessions", "lazer_sessions"] {
                assert_eq!(sqlx::query_scalar::<_,i64>(&format!("SELECT count(*) FROM {table} WHERE user_id=$1")).bind(alice).fetch_one(&db).await.unwrap(), 0);
            }
            let credentials = sqlx::query("SELECT password_hash,password_md5_hash FROM users WHERE id=$1").bind(alice).fetch_one(&db).await.unwrap();
            let argon_hash: String = credentials.get("password_hash");
            let md5_hash: String = credentials.get("password_md5_hash");
            assert!(Argon2::default().verify_password(replacement_password.as_bytes(), &PasswordHash::new(&argon_hash).unwrap()).is_ok());
            assert!(Argon2::default().verify_password(format!("{:x}", md5::compute(replacement_password.as_bytes())).as_bytes(), &PasswordHash::new(&md5_hash).unwrap()).is_ok());
            assert!(Argon2::default().verify_password(original_password.as_bytes(), &PasswordHash::new(&argon_hash).unwrap()).is_err());
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1 AND consumed_at IS NULL").bind(alice).fetch_one(&db).await.unwrap(), 0);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mail_jobs WHERE user_id=$1 AND (encrypted_payload IS NOT NULL OR status<>'canceled')").bind(alice).fetch_one(&db).await.unwrap(), 0);
            assert_invalid_reset(&state, &valid).await;
            let audits: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events WHERE actor_id=$1").bind(alice).fetch_all(&db).await.unwrap();
            assert!(audits.iter().all(|event| !event.to_string().contains(&valid)));

            let concurrent_token = seed_token(&state, alice, Purpose::Reset).await;
            let final_password = "final recovery password fixture";
            let (first, second) = tokio::join!(reset(&state, &concurrent_token, final_password), reset(&state, &concurrent_token, final_password));
            assert_ne!(first.is_ok(), second.is_ok());
            let failed = if let Err(error) = first { error } else { second.unwrap_err() };
            assert_eq!(failed.1, "invalid_recovery_token");

            let verification = seed_token(&state, alice, Purpose::Verify).await;
            assert_eq!(verify_link(&state, &verification).await.unwrap().status(), StatusCode::OK);
            assert_eq!(verify_link(&state, &verification).await.unwrap_err().1, "invalid_recovery_token");
            let public = auth::profile(&state, alice, false).await.unwrap();
            let private = auth::profile(&state, alice, true).await.unwrap();
            assert!(public.get("email").is_none());
            assert!(public.get("email_verified").is_none());
            assert_eq!(private["email_verified"], true);
            assert_eq!(private["email"], "recoveralice@example.invalid");
            let login = auth::login(State(state.clone()), Json(serde_json::from_value::<auth::Login>(json!({"username":"RecoverAlice","password":final_password})).unwrap())).await.unwrap();
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, login.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().parse().unwrap());
            let count_before: i64 = sqlx::query_scalar("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1").bind(alice).fetch_one(&db).await.unwrap();
            assert_eq!(request_verification(State(state.clone()), headers.clone(), Bytes::new()).await.unwrap().status(), StatusCode::ACCEPTED);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM account_recovery_tokens WHERE user_id=$1").bind(alice).fetch_one(&db).await.unwrap(), count_before);
            assert_eq!(request_verification(State(state.clone()), headers, Bytes::from_static(b"{\"email\":\"other@example.invalid\"}")).await.unwrap_err().0, StatusCode::BAD_REQUEST);
            assert_eq!(request_verification(State(state.clone()), HeaderMap::new(), Bytes::new()).await.unwrap_err().0, StatusCode::UNAUTHORIZED);

            // A login may finish verifying an old credential while a reset is
            // uncommitted. Observe its actual row-lock wait before committing
            // the credential change, then prove it cannot create a new session.
            let (race_hash, race_legacy) = auth::hash_password("password after race test".into()).await.unwrap();
            let mut changing = db.begin().await.unwrap();
            sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
                .bind(alice).bind(race_hash).bind(race_legacy).execute(&mut *changing).await.unwrap();
            auth::revoke_sessions(&mut changing, alice, None).await.unwrap();
            let login_state = state.clone();
            let racing_login = tokio::spawn(async move {
                auth::login(State(login_state), Json(serde_json::from_value::<auth::Login>(json!({"username":"RecoverAlice","password":final_password})).unwrap())).await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND wait_event_type='Lock' AND query LIKE '%password_hash=$2%FOR UPDATE%')")
                        .bind(&application_name).fetch_one(&control).await.unwrap();
                    if waiting { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }).await.expect("old-credential login must reach its locked credential recheck");
            changing.commit().await.unwrap();
            assert_eq!(racing_login.await.unwrap().unwrap_err().0, StatusCode::UNAUTHORIZED);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM sessions WHERE user_id=$1").bind(alice).fetch_one(&db).await.unwrap(), 0);

            // A registration transaction owns its user row; queue insertion is
            // rolled back along with that new account if registration aborts.
            let mut tx = db.begin().await.unwrap();
            let staged: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES('RecoverStaged','staged@example.invalid','fixture','fixture') RETURNING id").fetch_one(&mut *tx).await.unwrap();
            assert!(enqueue_verification_in_tx(&state, &mut tx, staged).await.unwrap());
            tx.rollback().await.unwrap();
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM users WHERE id=$1").bind(staged).fetch_one(&db).await.unwrap(), 0);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mail_jobs WHERE user_id=$1").bind(staged).fetch_one(&db).await.unwrap(), 0);
            let mut disabled = state.clone();
            disabled.mail = Arc::new(crate::mail_delivery::MailConfig::disabled());
            assert_eq!(request_reset(State(disabled.clone()), Json(ResetRequest { email:"recoveralice@example.invalid".into(),locale:None })).await.unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
            assert!(!enqueue_verification(&disabled, &auth::User { id: alice, session: Uuid::new_v4(), permissions: Vec::new() }).await.unwrap());
        }).catch_unwind().await;
        db.close().await;
        let cleanup = sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&control)
            .await;
        control.close().await;
        cleanup.unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
}
