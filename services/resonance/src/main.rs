mod assets;
mod beatmaps;
mod common;
#[cfg(test)]
mod credential_race_tests;
mod lazer;
mod lazer_chat;
#[cfg(test)]
mod lazer_map_tests;
mod multiplayer;
#[cfg(test)]
mod native_map_tests;
mod native_maps;
mod native_score_publication;
mod native_set_metadata;
#[cfg(test)]
mod native_set_metadata_tests;
mod registration;
#[cfg(test)]
mod relax_tests;
mod replay_capture;
mod room_results;
mod room_rounds;
#[cfg(test)]
mod room_score_tests;
mod room_scores;
mod score_aggregates;
mod signalr;
#[cfg(test)]
mod social_session_tests;
mod stable;
mod stable_scores;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::get,
};
use common::AppState;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::{env, path::PathBuf, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("resonance=info,tower_http=info")),
        )
        .init();
    let database_url = env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("../../migrations").run(&pool).await?;
    signalr::Runtime::recover(&pool).await?;
    let storage = PathBuf::from(env::var("STORAGE_PATH").unwrap_or_else(|_| "./data".into()));
    tokio::fs::create_dir_all(&storage).await?;
    tokio::fs::create_dir_all(storage.join("maps")).await?;
    tokio::fs::create_dir_all(storage.join("replays")).await?;
    let notification_endpoint = notification_endpoint()?;
    let state = AppState {
        pool: pool.clone(),
        storage: Arc::new(tokio::fs::canonicalize(storage).await?),
        sanctuary_url: Arc::new(
            env::var("SANCTUARY_INTERNAL_URL").unwrap_or_else(|_| "http://sanctuary:8080".into()),
        ),
        service_key: Arc::new(env::var("INTERNAL_SERVICE_KEY").unwrap_or_default()),
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?,
        limits: Default::default(),
        signalr: Arc::new(signalr::Runtime::new()),
        lazer_chat: Arc::new(lazer_chat::Runtime::with_endpoint(notification_endpoint)),
        trusted_proxies: Arc::new(
            env::var("TRUSTED_PROXY_CIDRS")
                .unwrap_or_default()
                .split(',')
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().parse::<ipnet::IpNet>())
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    state.lazer_chat.start(pool).await?;
    if let Err(error) = room_scores::recover_pending(&state).await {
        tracing::warn!(
            code = error.1,
            "room score recovery will retry in background"
        );
    }
    if let Err(error) = signalr::publish_room_score_events(&state).await {
        tracing::warn!(code = error.1, "room score events will retry in background");
    }
    if let Err(error) = replay_capture::recover_pending(&state).await {
        tracing::warn!(code = error.1, "replay recovery will retry in background");
    }
    let recovery_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = room_scores::recover_pending(&recovery_state).await {
                tracing::warn!(code = error.1, "room score recovery batch failed");
            }
            if let Err(error) = replay_capture::recover_pending(&recovery_state).await {
                tracing::warn!(code = error.1, "replay recovery batch failed");
            }
            if let Err(error) = signalr::publish_room_score_events(&recovery_state).await {
                tracing::warn!(code = error.1, "room score event batch failed");
            }
        }
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(index).post(stable::bancho))
        .merge(stable::router())
        .merge(registration::router(&env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into()))?)
        .merge(lazer::router())
        .merge(room_scores::router())
        .merge(room_results::router())
        .merge(lazer_chat::router())
        .merge(signalr::router())
        .merge(beatmaps::router())
        .merge(assets::router())
        .layer(DefaultBodyLimit::max(4_194_304))
        .layer(TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| tracing::info_span!("http_request", method=%request.method(), path=%request.uri().path())))
        .with_state(state);
    let address = env::var("RESONANCE_BIND").unwrap_or_else(|_| "0.0.0.0:8081".into());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "Resonance protocol gateway listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    Ok(())
}

async fn index() -> Json<serde_json::Value> {
    Json(
        json!({"service":"Resonance", "project":"Illyaverse", "stable":"login, presence, chat, friends, encrypted score submission, map/replay downloads, multiplayer and spectating", "lazer":"OAuth, map lookup/download, structured-mod solo scores, durable replay exports, native notification chat, friends, HeadToHead room scores and SignalR hub subsets", "unsupported":["unsupported lazer mod settings and customized Flashlight parameters", "deterministic replay gameplay verification", "announcement and group chat channels", "non-chat notification types", "cross-client multiplayer rooms", "playlist room creation", "stable target-practice submission", "tournament interfaces"], "compatibility":"experimental; live protocol tests do not replace actual native client validation", "score_policy":"validated native scores publish automatically; passed lazer scores require a complete replay capture; API submissions and explicit moderator decisions retain their review state; RX has separate ruleset-specific PP, ranks and statistics", "score_scales":"supported vanilla stable/lazer contexts use versioned common scales; RX and unavailable contexts are excluded from common-score comparison"}),
    )
}
async fn health(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, common::ApiError> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({"status":"ok", "service":"resonance"})))
}
fn notification_endpoint() -> Result<String, Box<dyn std::error::Error>> {
    let explicit = env::var("LAZER_NOTIFICATION_ENDPOINT").unwrap_or_default();
    let mut url = if explicit.is_empty() {
        let origin = env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
        let mut url = reqwest::Url::parse(&origin)?;
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            _ => return Err("PUBLIC_ORIGIN requires HTTP or HTTPS".into()),
        };
        url.set_scheme(scheme)
            .map_err(|_| "Invalid notification scheme")?;
        url.set_path("/notification-server");
        url
    } else {
        reqwest::Url::parse(&explicit)?
    };
    if !matches!(url.scheme(), "ws" | "wss")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("LAZER_NOTIFICATION_ENDPOINT requires a websocket URL without credentials, query or fragment".into());
    }
    if url.path().is_empty() {
        url.set_path("/notification-server");
    }
    Ok(url.into())
}
async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
