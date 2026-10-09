mod account_recovery;
mod admin;
mod announcements;
#[cfg(test)]
mod announcements_tests;
mod auth;
mod beatmap_availability;
#[cfg(test)]
mod beatmap_availability_tests;
#[cfg(test)]
mod beatmap_filename_tests;
mod chat;
#[cfg(test)]
mod chat_visibility_tests;
#[cfg(test)]
mod integration_tests;
mod legacy_scoring;
mod mail_delivery;
#[cfg(test)]
mod mail_queue_tests;
mod map_catalogue;
#[cfg(test)]
mod map_catalogue_tests;
#[cfg(test)]
mod map_package_migration_tests;
#[cfg(test)]
mod map_search_tests;
mod maps;
#[cfg(test)]
mod native_map_tests;
#[cfg(test)]
mod official_archive_tests;
#[cfg(test)]
mod official_set_metadata_tests;
#[cfg(test)]
mod profile_best_tests;
#[cfg(test)]
mod profile_media_tests;
#[cfg(test)]
mod profile_presentation_tests;
mod rank_history;
#[cfg(test)]
mod rank_history_tests;
#[cfg(test)]
mod rbac_tests;
#[cfg(test)]
mod relax_tests;
mod score_recovery;
mod score_scales;
mod scores;
mod social;
mod website_chat;
mod website_chat_runtime;

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tower_http::{
    cors::CorsLayer, services::ServeDir, set_header::SetResponseHeaderLayer, trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub secret: Arc<String>,
    pub internal_key: Arc<String>,
    pub origin: Arc<String>,
    pub mail: Arc<mail_delivery::MailConfig>,
    pub secure_cookie: bool,
    pub media_root: PathBuf,
    pub maps_root: PathBuf,
    pub replay_root: PathBuf,
    pub client: reqwest::Client,
    pub official: Arc<Mutex<maps::OfficialState>>,
    pub limits: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    pub trusted_proxies: Arc<Vec<ipnet::IpNet>>,
    pub upload_slots: Arc<tokio::sync::Semaphore>,
    pub chat_runtime: Arc<website_chat_runtime::ChatRuntime>,
}
impl AppState {
    pub fn upload_permit(&self) -> ApiResult<tokio::sync::OwnedSemaphorePermit> {
        self.upload_slots.clone().try_acquire_owned().map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "upload_busy",
                "Upload processing is busy. Please try again shortly.".into(),
            )
        })
    }
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str, pub String);
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.1, self.2)
    }
}
impl std::error::Error for ApiError {}
impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message.into())
    }
    pub fn forbidden() -> Self {
        Self(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Your role does not permit this action.".into(),
        )
    }
    pub fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in to continue.".into(),
        )
    }
    pub fn missing() -> Self {
        Self(
            StatusCode::NOT_FOUND,
            "not_found",
            "The requested resource was not found.".into(),
        )
    }
    pub fn internal(message: impl std::fmt::Display) -> Self {
        tracing::error!(error=%message,"request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The request could not be completed.".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error":{"code":self.1,"message":self.2}})),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(value: sqlx::Error) -> Self {
        if value
            .as_database_error()
            .is_some_and(|e| e.code().as_deref() == Some("P2001"))
        {
            beatmap_availability::withdrawn()
        } else if value
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            Self(
                StatusCode::CONFLICT,
                "already_exists",
                "That username, email or content already exists.".into(),
            )
        } else {
            Self::internal(value)
        }
    }
}
impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        Self::internal(value)
    }
}
pub type ApiResult<T = Json<Value>> = Result<T, ApiError>;

pub fn page(items: Vec<Value>, total: i64, page: i64, page_size: i64) -> Json<Value> {
    Json(json!({"items":items,"total":total,"page":page,"page_size":page_size}))
}
pub async fn audit(
    state: &AppState,
    actor: i64,
    action: &str,
    target_type: &str,
    target_id: impl ToString,
    details: Value,
) -> ApiResult<()> {
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,$2,$3,$4,$5)")
        .bind(actor).bind(action).bind(target_type).bind(target_id.to_string()).bind(details).execute(&state.db).await?;
    Ok(())
}

async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let mutating = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    );
    if mutating || request.headers().contains_key("sec-websocket-key") {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        let uses_cookie = request
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(';').any(|s| s.trim().starts_with("illya_session=")));
        if origin.is_some_and(|v| v != state.origin.as_str())
            || (uses_cookie
                && origin.is_none()
                && !request.headers().contains_key(header::AUTHORIZATION))
        {
            return ApiError(
                StatusCode::FORBIDDEN,
                "origin_mismatch",
                "This request must come from the configured website origin.".into(),
            )
            .into_response();
        }
    }
    let path = request.uri().path();
    let limit =
        if path.starts_with("/api/v1/auth/recovery/") || path.starts_with("/api/v1/auth/email/") {
            Some((5, 60, "recovery"))
        } else if mutating && path.starts_with("/api/v1/auth/") {
            Some((20, 600, "auth"))
        } else if path.ends_with("/stream") {
            Some((20, 60, "stream"))
        } else if mutating && path.starts_with("/api/v1/me/relationships/") {
            Some((60, 60, "relationships"))
        } else if mutating && path.ends_with("/messages") {
            Some((30, 60, "chat"))
        } else if mutating
            && (path.ends_with("/upload")
                || path.ends_with("/media")
                || path == "/api/v1/beatmaps/import/archive")
        {
            Some((10, 600, "upload"))
        } else {
            None
        };
    if let Some((max, seconds, scope)) = limit {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|v| v.0.ip());
        let forwarded =
            if peer.is_some_and(|ip| state.trusted_proxies.iter().any(|net| net.contains(&ip))) {
                request
                    .headers()
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| {
                        v.split(',')
                            .rev()
                            .filter_map(|part| part.trim().parse::<std::net::IpAddr>().ok())
                            .find(|ip| !state.trusted_proxies.iter().any(|net| net.contains(ip)))
                    })
            } else {
                None
            };
        let ip = forwarded
            .or(peer)
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".into());
        let mut limits = state.limits.lock().await;
        if limits.len() >= 10_000 {
            limits.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(600));
        }
        let key = format!("{scope}:{ip}");
        if limits.len() >= 10_000 && !limits.contains_key(&key) {
            return ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "The service is busy. Please try again shortly.".into(),
            )
            .into_response();
        }
        let entry = limits.entry(key).or_insert((Instant::now(), 0));
        if entry.0.elapsed() > Duration::from_secs(seconds) {
            *entry = (Instant::now(), 0);
        }
        if entry.1 >= max {
            return (StatusCode::TOO_MANY_REQUESTS,[(header::RETRY_AFTER,seconds.to_string())],Json(json!({"error":{"code":"rate_limited","message":"Please wait before trying again."}}))).into_response();
        }
        entry.1 += 1;
    }
    let response = next.run(request).await;
    if response.status().is_client_error()
        && !response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"))
    {
        let status = response.status();
        let code = if status == StatusCode::PAYLOAD_TOO_LARGE {
            "payload_too_large"
        } else if status == StatusCode::NOT_FOUND {
            "not_found"
        } else {
            "invalid_request"
        };
        return (status,Json(json!({"error":{"code":code,"message":"The request is malformed, unsupported, or exceeds its size limit."}}))).into_response();
    }
    response
}

fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(
            state
                .origin
                .parse::<axum::http::HeaderValue>()
                .expect("valid PUBLIC_ORIGIN"),
        )
        .allow_credentials(true)
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ]);
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok","service":"sanctuary"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/api/v1/status", get(status))
        .route("/api/v1/auth/capabilities", get(auth_capabilities))
        .route(
            "/api/v1/auth/recovery/request",
            post(account_recovery::request_reset).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/auth/recovery/reset",
            post(account_recovery::confirm_reset).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/auth/email/request",
            post(account_recovery::request_verification).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/auth/email/verify",
            post(account_recovery::confirm_verification).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/auth/register",
            post(auth::register).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/auth/login",
            post(auth::login).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me))
        .route("/api/v1/me/relationships", get(social::list))
        .route(
            "/api/v1/me/relationships/{id}",
            post(social::change)
                .delete(social::remove)
                .layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/v1/me/preferences",
            get(auth::preferences)
                .patch(auth::update_preferences)
                .layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route(
            "/api/v1/me/profile",
            patch(auth::update_profile).layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route(
            "/api/v1/me/password",
            post(auth::change_password).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/me/media",
            post(auth::upload_media)
                .layer(DefaultBodyLimit::max(14 * 1024 * 1024))
                .merge(delete(auth::remove_profile_media).layer(DefaultBodyLimit::max(1024))),
        )
        .route("/api/v1/users/{id}", get(auth::user_profile))
        .route("/api/v1/users/{id}/rank-history", get(rank_history::get))
        .route("/api/v1/announcements", get(announcements::list))
        .route("/api/v1/announcements/{id}", get(announcements::get))
        .route("/api/v1/users/{id}/scores/best", get(scores::best_for_user))
        .route("/api/v1/beatmaps", get(maps::list))
        .route("/api/v1/beatmapsets", get(map_catalogue::list))
        .route(
            "/api/v1/beatmaps/{id}/set/difficulties",
            get(map_catalogue::difficulties),
        )
        .route(
            "/api/v1/beatmaps/upload",
            post(maps::upload).layer(DefaultBodyLimit::max(66 * 1024 * 1024)),
        )
        .route(
            "/api/v1/beatmaps/import",
            post(maps::import).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/beatmaps/import/archive",
            post(maps::import_archive).layer(DefaultBodyLimit::max(68 * 1024 * 1024)),
        )
        .route("/api/v1/beatmaps/{id}", get(maps::detail))
        .route(
            "/api/v1/beatmaps/{id}/package/difficulties",
            get(maps::package_difficulties),
        )
        .route("/api/v1/beatmaps/{id}/download", get(maps::download))
        .route("/api/v1/beatmaps/{id}/archive", get(maps::archive_download))
        .route("/api/v1/beatmaps/{id}/osu", get(maps::osu_file))
        .route(
            "/api/v1/beatmaps/{id}/scores",
            get(scores::beatmap_leaderboard),
        )
        .route(
            "/api/v1/scores",
            get(scores::list)
                .post(scores::submit)
                .layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route(
            "/api/v1/scores/{id}/replay",
            get(scores::replay)
                .post(scores::upload_replay)
                .layer(DefaultBodyLimit::max(18 * 1024 * 1024)),
        )
        .route("/api/v1/scores/{id}", get(scores::detail))
        .route(
            "/api/v1/internal/scores",
            post(scores::internal_submit).layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route(
            "/api/v1/internal/beatmaps/resolve",
            post(maps::resolve).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route(
            "/api/v1/internal/beatmapsets/metadata",
            post(maps::metadata).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route("/api/v1/leaderboard", get(scores::leaderboard))
        .route("/api/v1/channels", get(chat::channels))
        .route(
            "/api/v1/channels/{id}/messages",
            get(chat::messages)
                .post(chat::send)
                .layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route("/api/v1/channels/{id}/stream", get(chat::stream))
        .route("/api/v1/chat/stream", get(website_chat_runtime::stream))
        .route(
            "/api/v1/chat/visibility",
            post(website_chat::visibility).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/conversations",
            get(website_chat::list)
                .post(website_chat::create)
                .layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route("/api/v1/conversations/{id}", get(website_chat::detail))
        .route(
            "/api/v1/conversations/{id}/messages",
            get(website_chat::history)
                .post(website_chat::send)
                .layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route(
            "/api/v1/conversations/{id}/read",
            axum::routing::put(website_chat::mark_read).layer(DefaultBodyLimit::max(8 * 1024)),
        )
        .route("/api/v1/admin/users", get(admin::users))
        .route(
            "/api/v1/admin/announcements",
            get(announcements::admin_list)
                .post(announcements::create)
                .layer(DefaultBodyLimit::max(256 * 1024)),
        )
        .route(
            "/api/v1/admin/announcements/{id}",
            get(announcements::admin_get)
                .patch(announcements::update)
                .layer(DefaultBodyLimit::max(256 * 1024)),
        )
        .route("/api/v1/admin/users/{id}", patch(admin::update_user))
        .route(
            "/api/v1/admin/users/{id}/sessions/revoke",
            post(admin::revoke_user_sessions),
        )
        .route(
            "/api/v1/admin/roles",
            get(admin::roles).post(admin::create_role),
        )
        .route(
            "/api/v1/admin/roles/{id}",
            patch(admin::update_role).delete(admin::delete_role),
        )
        .route("/api/v1/admin/beatmaps", get(maps::admin_list))
        .route(
            "/api/v1/admin/beatmap-packages/{id}/filename-aliases/backfill",
            post(maps::backfill_filename_aliases).layer(DefaultBodyLimit::max(1024)),
        )
        .route(
            "/api/v1/admin/beatmaps/{id}",
            get(maps::admin_detail).patch(admin::update_map),
        )
        .route(
            "/api/v1/admin/beatmapsets/{id}/availability",
            patch(beatmap_availability::update),
        )
        .route("/api/v1/admin/scores/{id}", patch(admin::update_score))
        .route(
            "/api/v1/admin/scores/recalculate-relax",
            post(scores::recalculate_relax).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/api/v1/admin/audit", get(admin::audit_events))
        // Mount only published image kinds. Private staging remains on the
        // same volume for atomic rename and has no static route of its own.
        .nest_service(
            "/media/avatar",
            ServeDir::new(state.media_root.join("avatar")),
        )
        .nest_service(
            "/media/banner",
            ServeDir::new(state.media_root.join("banner")),
        )
        .nest_service(
            "/media/background",
            ServeDir::new(state.media_root.join("background")),
        )
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .with_state(state)
}
async fn ready(State(state): State<AppState>) -> ApiResult {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(json!({"status":"ready"})))
}
async fn auth_capabilities(State(state): State<AppState>) -> Json<Value> {
    let enabled = state.mail.is_enabled();
    Json(json!({"password_reset":enabled,"email_verification":enabled}))
}
async fn status(State(state): State<AppState>) -> ApiResult {
    let value:Value=sqlx::query_scalar("SELECT jsonb_build_object('registered_players',(SELECT count(*) FROM users WHERE NOT is_restricted),'ranked_scores',(SELECT count(*) FROM scores s JOIN users u ON u.id=s.user_id JOIN active_beatmaps b ON b.id=s.beatmap_id WHERE s.verification='verified' AND s.passed AND b.status='ranked' AND NOT u.is_restricted AND s.pp_eligible),'beatmap_count',(SELECT count(*) FROM active_beatmaps))").fetch_one(&state.db).await?;
    let mut value = value;
    let online:i64=sqlx::query_scalar("SELECT count(DISTINCT active.user_id) FROM (SELECT user_id FROM bancho_sessions WHERE expires_at>now() UNION SELECT user_id FROM lazer_sessions WHERE access_expires_at>now() AND last_seen_at>now()-interval '3 minutes') active JOIN users u ON u.id=active.user_id WHERE NOT u.is_restricted").fetch_one(&state.db).await?;
    value["players_online"] = json!(online);
    value["compatibility"] = json!({"stable":{"status":"experimental","tested":false},"lazer":{"status":"experimental","tested":false}});
    Ok(Json(value))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sanctuary=info,tower_http=info".into()),
        )
        .init();
    let secret = env::var("SESSION_SECRET").map_err(|_| "SESSION_SECRET must be configured")?;
    if secret.len() < 32 {
        return Err("SESSION_SECRET must contain at least 32 bytes".into());
    }
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&env::var("DATABASE_URL")?)
        .await?;
    sqlx::migrate!("../../migrations").run(&db).await?;
    let media_root = PathBuf::from(env::var("MEDIA_ROOT").unwrap_or_else(|_| "data/media".into()));
    let maps_root = PathBuf::from(env::var("MAPS_ROOT").unwrap_or_else(|_| "data/maps".into()));
    let replay_root =
        PathBuf::from(env::var("REPLAY_ROOT").unwrap_or_else(|_| "data/replays".into()));
    for path in [&media_root, &maps_root, &replay_root] {
        tokio::fs::create_dir_all(path).await?;
    }
    let trusted_proxies = env::var("TRUSTED_PROXY_CIDRS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<ipnet::IpNet>())
        .collect::<Result<Vec<_>, _>>()?;
    let origin = env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
    let mail = Arc::new(mail_delivery::MailConfig::from_env(&origin)?);
    let state = AppState {
        db,
        secret: Arc::new(secret),
        internal_key: Arc::new(env::var("INTERNAL_SERVICE_KEY").unwrap_or_default()),
        origin: Arc::new(origin),
        mail,
        secure_cookie: env::var("COOKIE_SECURE").unwrap_or_else(|_| "true".into()) != "false",
        media_root,
        maps_root,
        replay_root,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Arc::new(Mutex::new(HashMap::new())),
        trusted_proxies: Arc::new(trusted_proxies),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        chat_runtime: Default::default(),
    };
    auth::bootstrap(&state).await?;
    score_recovery::start(state.clone());
    rank_history::start(state.db.clone());
    mail_delivery::start(state.db.clone(), state.mail.clone());
    sqlx::query("DELETE FROM sessions WHERE expires_at < now()")
        .execute(&state.db)
        .await?;
    let address = env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address,"Sanctuary listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
