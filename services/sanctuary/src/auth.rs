use crate::{ApiError, ApiResult, AppState, scores};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use illya_domain::{
    Preferences, ScoreVariant, has_permission, validate_color, validate_password, validate_username,
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: i64,
    sid: Uuid,
    exp: usize,
}
pub struct User {
    pub id: i64,
    pub session: Uuid,
    pub permissions: Vec<String>,
}
impl User {
    pub fn require(&self, permission: &str) -> ApiResult<()> {
        if has_permission(&self.permissions, permission) {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
}

pub async fn require(state: &AppState, headers: &HeaderMap) -> ApiResult<User> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.split(';')
                        .find_map(|part| part.trim().strip_prefix("illya_session="))
                })
        })
        .ok_or_else(ApiError::unauthorized)?;
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| ApiError::unauthorized())?
    .claims;
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND s.user_id=$2 AND s.expires_at>now() AND NOT u.is_restricted)")
        .bind(claims.sid).bind(claims.sub).fetch_one(&state.db).await?;
    if !valid {
        return Err(ApiError::unauthorized());
    }
    let permissions=sqlx::query_scalar::<_,String>("SELECT DISTINCT unnest(r.permissions) FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1")
        .bind(claims.sub).fetch_all(&state.db).await?;
    Ok(User {
        id: claims.sub,
        session: claims.sid,
        permissions,
    })
}
pub async fn optional(state: &AppState, headers: &HeaderMap) -> ApiResult<Option<User>> {
    if !headers.contains_key(header::AUTHORIZATION)
        && !headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("illya_session="))
    {
        return Ok(None);
    }
    match require(state, headers).await {
        Ok(user) => Ok(Some(user)),
        Err(e) if e.0 == StatusCode::UNAUTHORIZED => Ok(None),
        Err(e) => Err(e),
    }
}
pub async fn profile(state: &AppState, id: i64, private: bool) -> ApiResult<Value> {
    profile_in_variant(state, id, private, 0, ScoreVariant::Vanilla).await
}
pub(crate) async fn profile_in_variant(
    state: &AppState,
    id: i64,
    private: bool,
    mode: i16,
    variant: ScoreVariant,
) -> ApiResult<Value> {
    scores::validate_partition(mode, variant)?;
    let row=sqlx::query("SELECT id,username,email,email_verified_at,country,bio,avatar_url,banner_url,accent_color,preferences,joined_at,is_restricted FROM users WHERE id=$1 AND ($2 OR NOT is_restricted)")
        .bind(id).bind(private).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    let roles=sqlx::query_scalar::<_,String>("SELECT r.name FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1 ORDER BY r.id").bind(id).fetch_all(&state.db).await?;
    let permissions=sqlx::query_scalar::<_,String>("SELECT DISTINCT unnest(r.permissions) FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1").bind(id).fetch_all(&state.db).await?;
    let stats = scores::statistics(state, id, mode, variant).await?;
    let preferences: Preferences =
        serde_json::from_value(row.get::<Value, _>("preferences")).unwrap_or_default();
    let mut value = json!({"id":id,"username":row.get::<String,_>("username"),"country":row.get::<String,_>("country"),"bio":row.get::<String,_>("bio"),"avatar_url":row.get::<Option<String>,_>("avatar_url"),"banner_url":row.get::<Option<String>,_>("banner_url"),"accent_color":row.get::<String,_>("accent_color"),"roles":roles,"joined_at":row.get::<chrono::DateTime<Utc>,_>("joined_at"),"pp":stats["pp"],"play_count":stats["play_count"],"accuracy":stats["accuracy"],"statistics":stats});
    if private {
        value["email"] = json!(row.get::<String, _>("email"));
        value["email_verified"] = json!(
            row.get::<Option<chrono::DateTime<Utc>>, _>("email_verified_at")
                .is_some()
        );
        value["permissions"] = json!(permissions);
        value["preferences"] = json!(preferences);
        value["locale"] = json!(preferences.locale);
    }
    Ok(value)
}
pub(crate) async fn hash_password(password: String) -> ApiResult<(String, String)> {
    tokio::task::spawn_blocking(move || {
        let argon = Argon2::default();
        let md5 = format!("{:x}", md5::compute(password.as_bytes()));
        let hash = argon
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map_err(ApiError::internal)?
            .to_string();
        let legacy = argon
            .hash_password(md5.as_bytes(), &SaltString::generate(&mut OsRng))
            .map_err(ApiError::internal)?
            .to_string();
        Ok((hash, legacy))
    })
    .await
    .map_err(ApiError::internal)?
}
async fn verify(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}
async fn session(state: &AppState, id: i64, credential: &str) -> ApiResult<Response> {
    let mut tx = state.db.begin().await?;
    // Serialize issuance with credential changes, then recheck the verified
    // snapshot. An old-password verification cannot mint a post-reset session.
    let current = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE id=$1 AND password_hash=$2 AND NOT is_restricted FOR UPDATE",
    )
    .bind(id)
    .bind(credential)
    .fetch_optional(&mut *tx)
    .await?;
    if current.is_none() {
        return Err(ApiError::unauthorized());
    }
    let sid = Uuid::new_v4();
    let expiry = Utc::now() + Duration::days(7);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expiry)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let token = encode(
        &Header::default(),
        &Claims {
            sub: id,
            sid,
            exp: expiry.timestamp() as usize,
        },
        &EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .map_err(ApiError::internal)?;
    let cookie = format!(
        "illya_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800{}",
        if state.secure_cookie { "; Secure" } else { "" }
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({"user":profile(state,id,true).await?})),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    username: String,
    email: String,
    password: String,
    locale: Option<String>,
}
pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<Registration>,
) -> ApiResult<Response> {
    validate_username(&input.username).map_err(ApiError::bad)?;
    validate_password(&input.password).map_err(ApiError::bad)?;
    let email = crate::account_recovery::normalize_email(&input.email)?;
    let preferences = Preferences {
        locale: input.locale.unwrap_or_else(|| "zh-TW".into()),
        ..Preferences::default()
    };
    preferences.validate().map_err(ApiError::bad)?;
    let (hash, legacy) = hash_password(input.password).await?;
    let mut tx = state.db.begin().await?;
    let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash,preferences) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(input.username).bind(email).bind(&hash).bind(legacy).bind(json!(preferences)).fetch_one(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    crate::account_recovery::enqueue_verification_in_tx(&state, &mut tx, id).await?;
    tx.commit().await?;
    session(&state, id, &hash).await
}
#[derive(Deserialize)]
pub struct Login {
    username: String,
    password: String,
}
pub async fn login(State(state): State<AppState>, Json(input): Json<Login>) -> ApiResult<Response> {
    if input.username.len() > 254 || input.password.len() > 128 {
        return Err(ApiError::unauthorized());
    }
    let row=sqlx::query("SELECT id,password_hash,is_restricted FROM users WHERE lower(username)=lower($1) OR lower(email)=lower($1)").bind(input.username.trim()).fetch_optional(&state.db).await?;
    let Some(row) = row else {
        // Spend one password verification on unknown accounts too.
        let _=verify(input.password,"$argon2id$v=19$m=19456,t=2,p=1$MTIzNDU2Nzg5MGFiY2RlZg$HsHQAI/BTc+rAoLRycoM5qNHkmguCeOIW7+NXhszGsE".into()).await;
        return Err(ApiError::unauthorized());
    };
    let credential: String = row.get("password_hash");
    if !verify(input.password, credential.clone()).await || row.get::<bool, _>("is_restricted") {
        return Err(ApiError::unauthorized());
    }
    session(&state, row.get("id"), &credential).await
}
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Ok(user) = require(&state, &headers).await {
        sqlx::query("DELETE FROM sessions WHERE id=$1")
            .bind(user.session)
            .execute(&state.db)
            .await?;
    }
    let cookie = format!(
        "illya_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
        if state.secure_cookie { "; Secure" } else { "" }
    );
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response())
}
pub async fn me(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = require(&state, &headers).await?;
    Ok(Json(profile(&state, user.id, true).await?))
}
#[derive(Deserialize)]
pub struct StatisticsQuery {
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
}
pub async fn user_profile(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<StatisticsQuery>,
) -> ApiResult {
    Ok(Json(
        profile_in_variant(
            &state,
            id,
            false,
            query.mode.unwrap_or(0),
            query.variant.unwrap_or_default(),
        )
        .await?,
    ))
}
pub async fn preferences(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = require(&state, &headers).await?;
    let value: Value = sqlx::query_scalar("SELECT preferences FROM users WHERE id=$1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(json!(
        serde_json::from_value::<Preferences>(value).unwrap_or_default()
    )))
}
pub async fn update_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<Value>,
) -> ApiResult {
    let user = require(&state, &headers).await?;
    let mut tx = state.db.begin().await?;
    let original: Value =
        sqlx::query_scalar("SELECT preferences FROM users WHERE id=$1 FOR UPDATE")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    let mut value = json!(serde_json::from_value::<Preferences>(original).unwrap_or_default());
    let object = patch
        .as_object()
        .ok_or_else(|| ApiError::bad("Preferences must be an object."))?;
    for (key, v) in object {
        value[key] = v.clone();
    }
    let preferences: Preferences =
        serde_json::from_value(value).map_err(|e| ApiError::bad(e.to_string()))?;
    preferences.validate().map_err(ApiError::bad)?;
    sqlx::query("UPDATE users SET preferences=$2 WHERE id=$1")
        .bind(user.id)
        .bind(json!(preferences))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!(preferences)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilePatch {
    bio: Option<String>,
    country: Option<String>,
    accent_color: Option<String>,
}
pub async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProfilePatch>,
) -> ApiResult {
    let user = require(&state, &headers).await?;
    if input
        .bio
        .as_ref()
        .is_some_and(|v| v.chars().count() > 4000 || v.contains('\0'))
    {
        return Err(ApiError::bad("Biography must be at most 4,000 characters."));
    }
    if input
        .country
        .as_ref()
        .is_some_and(|v| v.len() != 2 || !v.bytes().all(|c| c.is_ascii_uppercase()))
    {
        return Err(ApiError::bad(
            "Country must be a two-letter uppercase country code.",
        ));
    }
    if input
        .accent_color
        .as_ref()
        .is_some_and(|v| !validate_color(v))
    {
        return Err(ApiError::bad("Enter a six-digit hex color."));
    }
    sqlx::query("UPDATE users SET bio=COALESCE($2,bio),country=COALESCE($3,country),accent_color=COALESCE($4,accent_color) WHERE id=$1").bind(user.id).bind(input.bio).bind(input.country).bind(input.accent_color).execute(&state.db).await?;
    Ok(Json(profile(&state, user.id, true).await?))
}
pub async fn upload_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult {
    let user = require(&state, &headers).await?;
    let _permit = state.upload_permit()?;
    let mut kind = None;
    let mut bytes = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
    {
        match field.name() {
            Some("kind") => {
                kind = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| ApiError::bad(e.to_string()))?,
                )
            }
            Some("file") => {
                let mut field = field;
                let mut buffer = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|e| ApiError::bad(e.to_string()))?
                {
                    if buffer.len() + chunk.len() > 12 * 1024 * 1024 {
                        return Err(ApiError::bad("Images must be at most 12 MiB."));
                    }
                    buffer.extend_from_slice(&chunk);
                }
                bytes = Some(buffer);
            }
            _ => {}
        }
    }
    let kind = kind.ok_or_else(|| ApiError::bad("Choose avatar, banner or background."))?;
    let max_width = match kind.as_str() {
        "avatar" => 512,
        "banner" => 1920,
        "background" => 2560,
        _ => return Err(ApiError::bad("Unknown image kind.")),
    };
    let bytes = bytes.ok_or_else(|| ApiError::bad("Attach an image in the file field."))?;
    let encoded = tokio::task::spawn_blocking(move || -> ApiResult<Vec<u8>> {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| ApiError::bad("Invalid image."))?;
        if !matches!(
            reader.format(),
            Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
        ) {
            return Err(ApiError::bad("Use a PNG, JPEG or WebP image."));
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|_| ApiError::bad("Image is invalid or too large."))?;
        let image = image.thumbnail(max_width, if max_width == 512 { 512 } else { 2560 });
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::WebP)
            .map_err(ApiError::internal)?;
        Ok(output.into_inner())
    })
    .await
    .map_err(ApiError::internal)??;
    let relative = format!("{kind}/{}.webp", Uuid::new_v4());
    tokio::fs::create_dir_all(state.media_root.join(&kind)).await?;
    tokio::fs::write(state.media_root.join(&relative), encoded).await?;
    let url = format!("/media/{relative}");
    match kind.as_str() {
        "avatar" => {
            sqlx::query("UPDATE users SET avatar_url=$2 WHERE id=$1")
                .bind(user.id)
                .bind(&url)
                .execute(&state.db)
                .await?;
        }
        "banner" => {
            sqlx::query("UPDATE users SET banner_url=$2 WHERE id=$1")
                .bind(user.id)
                .bind(&url)
                .execute(&state.db)
                .await?;
        }
        _ => {
            sqlx::query("UPDATE users SET preferences=jsonb_set(preferences,'{background_url}',to_jsonb($2::text)) WHERE id=$1").bind(user.id).bind(&url).execute(&state.db).await?;
        }
    }
    Ok(Json(json!({"url":url,"kind":kind})))
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileMediaKind {
    Avatar,
    Banner,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMediaRemoval {
    kind: ProfileMediaKind,
}

pub async fn remove_profile_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProfileMediaRemoval>,
) -> ApiResult {
    let user = require(&state, &headers).await?;
    let mut tx = state.db.begin().await?;
    // Recovery and administrative revocation also lock users before sessions.
    // Recheck authorization at the write after either lock has finished waiting.
    let owner = sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?;
    if owner.is_none() {
        return Err(ApiError::unauthorized());
    }
    let session = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM sessions WHERE id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(user.session)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    if session.is_none() {
        return Err(ApiError::unauthorized());
    }
    let update = match input.kind {
        ProfileMediaKind::Avatar => {
            "UPDATE users SET avatar_url=NULL WHERE id=$1 AND NOT is_restricted AND EXISTS(SELECT 1 FROM sessions WHERE id=$2 AND user_id=$1 AND expires_at>clock_timestamp())"
        }
        ProfileMediaKind::Banner => {
            "UPDATE users SET banner_url=NULL WHERE id=$1 AND NOT is_restricted AND EXISTS(SELECT 1 FROM sessions WHERE id=$2 AND user_id=$1 AND expires_at>clock_timestamp())"
        }
    };
    let changed = sqlx::query(update)
        .bind(user.id)
        .bind(user.session)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::unauthorized());
    }
    tx.commit().await?;
    Ok(Json(json!({"kind":input.kind,"url":null})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChange {
    current_password: String,
    new_password: String,
}
/// The caller locks the user row before changing credentials or revoking sessions.
/// Keeping one web session is only appropriate for an authenticated password change.
pub(crate) async fn revoke_sessions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i64,
    keep_web_session: Option<Uuid>,
) -> ApiResult<()> {
    sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND ($2::uuid IS NULL OR id<>$2)")
        .bind(user_id)
        .bind(keep_web_session)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE account_recovery_tokens SET consumed_at=COALESCE(consumed_at,now()) WHERE user_id=$1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE mail_jobs SET status='canceled',encrypted_payload=NULL,lease_until=NULL,lease_id=NULL,last_error_code=NULL WHERE user_id=$1 AND status IN ('pending','sending','failed')")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PasswordChange>,
) -> ApiResult {
    let user = require(&state, &headers).await?;
    validate_password(&input.new_password).map_err(ApiError::bad)?;
    if input.current_password.len() > 128 {
        return Err(ApiError::unauthorized());
    }
    let mut tx = state.db.begin().await?;
    let current: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 FOR UPDATE")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if !verify(input.current_password, current).await {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_password",
            "The current password is incorrect.".into(),
        ));
    }
    let (hash, legacy) = hash_password(input.new_password).await?;
    sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
        .bind(user.id)
        .bind(hash)
        .bind(legacy)
        .execute(&mut *tx)
        .await?;
    revoke_sessions(&mut tx, user.id, Some(user.session)).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.password_change','user',$2,'{\"other_sessions_revoked\":true}')").bind(user.id).bind(user.id.to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn bootstrap(state: &AppState) -> ApiResult<()> {
    let Ok(username) = std::env::var("BOOTSTRAP_ADMIN_USERNAME") else {
        return Ok(());
    };
    if username.trim().is_empty() {
        return Ok(());
    }
    let password = std::env::var("BOOTSTRAP_ADMIN_PASSWORD")
        .map_err(|_| ApiError::bad("Set BOOTSTRAP_ADMIN_PASSWORD."))?;
    let email = std::env::var("BOOTSTRAP_ADMIN_EMAIL")
        .map_err(|_| ApiError::bad("Set BOOTSTRAP_ADMIN_EMAIL."))?;
    let email = crate::account_recovery::normalize_email(&email)?;
    validate_username(&username).map_err(ApiError::bad)?;
    validate_password(&password).map_err(ApiError::bad)?;
    let existing =
        sqlx::query("SELECT id,password_hash FROM users WHERE lower(username)=lower($1)")
            .bind(&username)
            .fetch_optional(&state.db)
            .await?;
    let id = if let Some(row) = existing {
        if !verify(password, row.get("password_hash")).await {
            return Err(ApiError::bad(
                "Bootstrap administrator credentials do not match the existing account.",
            ));
        }
        row.get::<i64, _>("id")
    } else {
        let (hash, legacy) = hash_password(password).await?;
        sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,$3,$4) RETURNING id").bind(username).bind(email.to_lowercase()).bind(hash).bind(legacy).fetch_one(&state.db).await?
    };
    sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name IN ('administrator','player') ON CONFLICT DO NOTHING").bind(id).execute(&state.db).await?;
    Ok(())
}
