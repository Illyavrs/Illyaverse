use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub storage: Arc<PathBuf>,
    pub sanctuary_url: Arc<String>,
    pub service_key: Arc<String>,
    pub http: reqwest::Client,
    pub limits: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    pub trusted_proxies: Arc<Vec<ipnet::IpNet>>,
    pub signalr: Arc<crate::signalr::Runtime>,
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1,"message":self.1}))).into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "protocol database operation failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
    }
}
impl From<resonance_protocol::ProtocolError> for ApiError {
    fn from(_: resonance_protocol::ProtocolError) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_protocol_payload")
    }
}
impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        tracing::error!(%error, "protocol storage operation failed");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "storage_error")
    }
}

#[derive(Clone)]
pub struct Account {
    pub id: i64,
    pub username: String,
    pub country: String,
    pub avatar_url: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub permissions: Vec<String>,
    pub is_online: bool,
}
impl Account {
    pub fn can(&self, permission: &str) -> bool {
        illya_domain::has_permission(&self.permissions, permission)
    }
}

pub async fn account(state: &AppState, id: i64) -> Result<Account, ApiError> {
    let row = sqlx::query("SELECT id,username,country,avatar_url,joined_at,(EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=users.id AND expires_at>now()) OR EXISTS(SELECT 1 FROM lazer_sessions WHERE user_id=users.id AND access_expires_at>now() AND last_seen_at>now()-interval '3 minutes')) AS is_online FROM users WHERE id=$1 AND NOT is_restricted").bind(id).fetch_optional(&state.pool).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_account"))?;
    let permissions = sqlx::query_scalar::<_,String>("SELECT DISTINCT unnest(r.permissions) FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1").bind(id).fetch_all(&state.pool).await?;
    Ok(Account {
        id,
        username: row.get("username"),
        country: row.get("country"),
        avatar_url: row.get("avatar_url"),
        joined_at: row.get("joined_at"),
        permissions,
        is_online: row.get("is_online"),
    })
}

pub async fn authenticate(
    state: &AppState,
    username: &str,
    password: &str,
    digest: bool,
) -> Result<Account, ApiError> {
    if username.len() > 64 || password.len() > 1024 {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    let field = if digest {
        "password_md5_hash"
    } else {
        "password_hash"
    };
    let query = format!(
        "SELECT id,{field} AS credential FROM users WHERE lower(username)=lower($1) AND NOT is_restricted"
    );
    let row = sqlx::query(&query)
        .bind(username)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"))?;
    let stored: String = row.get("credential");
    let password = password.to_owned();
    let verified = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&stored).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !verified {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    account(state, row.get("id")).await
}

pub fn secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
pub fn token_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub async fn rate_limit(state: &AppState, key: String, max: u32) -> Result<(), ApiError> {
    let mut limits = state.limits.lock().await;
    let now = Instant::now();
    limits.retain(|_, (at, _)| now.duration_since(*at) < Duration::from_secs(60));
    if limits.len() > 10_000 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    let counter = limits.entry(key).or_insert((now, 0));
    counter.1 += 1;
    if counter.1 > max {
        Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited"))
    } else {
        Ok(())
    }
}

/// The socket peer is authoritative; forwarding headers are never trusted here.
/// Put the deployment's public auth rate limit on its trusted reverse proxy too.
pub fn client_key(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> String {
    let trusted = |address: std::net::IpAddr| {
        state
            .trusted_proxies
            .iter()
            .any(|network| network.contains(&address))
    };
    if !trusted(peer.ip()) {
        return peer.ip().to_string();
    }
    let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) else {
        return peer.ip().to_string();
    };
    if forwarded.len() > 1024 {
        return peer.ip().to_string();
    }
    let addresses: Result<Vec<std::net::IpAddr>, _> =
        forwarded.split(',').map(|v| v.trim().parse()).collect();
    let Ok(addresses) = addresses else {
        return peer.ip().to_string();
    };
    if addresses.len() > 8 {
        return peer.ip().to_string();
    }
    addresses
        .into_iter()
        .rev()
        .find(|address| !trusted(*address))
        .map_or_else(|| peer.ip().to_string(), |address| address.to_string())
}

pub async fn bearer(state: &AppState, headers: &HeaderMap) -> Result<Account, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing_access_token"))?;
    let id = sqlx::query_scalar::<_,i64>("UPDATE lazer_sessions SET last_seen_at=now() WHERE access_hash=$1 AND access_expires_at>now() RETURNING user_id").bind(token_hash(token)).fetch_optional(&state.pool).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_access_token"))?;
    account(state, id).await
}

pub async fn stored_file(
    state: &AppState,
    stored: &str,
    directory: &str,
    max_size: u64,
) -> Result<Vec<u8>, ApiError> {
    let path = Path::new(stored);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        state.storage.join(directory).join(path)
    };
    let resolved = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "file_not_found"))?;
    let directory_root = tokio::fs::canonicalize(state.storage.join(directory)).await?;
    if !resolved.starts_with(&directory_root) {
        return Err(ApiError(StatusCode::FORBIDDEN, "invalid_storage_path"));
    }
    let metadata = tokio::fs::metadata(&resolved).await?;
    if metadata.len() > max_size {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "file_too_large"));
    }
    Ok(tokio::fs::read(resolved).await?)
}

pub async fn friend_ids(state: &AppState, user_id: i64) -> Result<Vec<i32>, ApiError> {
    let ids = sqlx::query_scalar::<_, i64>("SELECT r.target_id FROM user_relationships r JOIN users u ON u.id=r.target_id WHERE r.user_id=$1 AND r.relation_type='friend' AND NOT u.is_restricted ORDER BY r.target_id LIMIT 4096")
        .bind(user_id).fetch_all(&state.pool).await?;
    ids.into_iter()
        .map(|id| {
            i32::try_from(id)
                .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stable_user_id_overflow"))
        })
        .collect()
}

/// Mutations use the authenticated owner. Blocking removes friendship in both
/// directions, while unblocking never silently restores it.
pub async fn change_relationship(
    state: &AppState,
    user_id: i64,
    target_id: i64,
    kind: &str,
    remove: bool,
) -> Result<(), ApiError> {
    if !account(state, user_id).await?.can("social.manage") {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "social_permission_required",
        ));
    }
    if user_id == target_id || !matches!(kind, "friend" | "block") {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_relationship"));
    }
    rate_limit(state, format!("relationships:{user_id}"), 60).await?;
    let mut tx = state.pool.begin().await?;
    let users = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE id=ANY($1) AND NOT is_restricted ORDER BY id FOR UPDATE",
    )
    .bind(vec![user_id, target_id])
    .fetch_all(&mut *tx)
    .await?;
    if users.len() != 2 {
        return Err(ApiError(StatusCode::NOT_FOUND, "user_not_found"));
    }
    if remove {
        sqlx::query(
            "DELETE FROM user_relationships WHERE user_id=$1 AND target_id=$2 AND relation_type=$3",
        )
        .bind(user_id)
        .bind(target_id)
        .bind(kind)
        .execute(&mut *tx)
        .await?;
    } else {
        if kind == "friend" {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='block')")
                .bind(user_id).bind(target_id).fetch_one(&mut *tx).await?;
            if blocked {
                return Err(ApiError(StatusCode::FORBIDDEN, "relationship_blocked"));
            }
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_relationships WHERE user_id=$1 AND target_id<>$2 AND relation_type='friend'")
                .bind(user_id).bind(target_id).fetch_one(&mut *tx).await?;
            if count >= 4096 {
                return Err(ApiError(StatusCode::BAD_REQUEST, "friend_limit_reached"));
            }
        } else {
            sqlx::query("DELETE FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='friend'")
                .bind(user_id).bind(target_id).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,$3) ON CONFLICT(user_id,target_id) DO UPDATE SET relation_type=EXCLUDED.relation_type")
            .bind(user_id).bind(target_id).bind(kind).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    for id in [user_id, target_id] {
        let packet = resonance_protocol::stable::friends_list(&friend_ids(state, id).await?)?;
        sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$1 AND expires_at>now())")
            .bind(id).bind(packet).execute(&state.pool).await?;
    }
    Ok(())
}
