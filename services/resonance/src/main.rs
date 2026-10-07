mod assets;
mod beatmaps;
mod common;
mod lazer;
mod multiplayer;
mod signalr;
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
    let state = AppState {
        pool,
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
        trusted_proxies: Arc::new(
            env::var("TRUSTED_PROXY_CIDRS")
                .unwrap_or_default()
                .split(',')
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().parse::<ipnet::IpNet>())
                .collect::<Result<Vec<_>, _>>()?,
        ),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/", get(index).post(stable::bancho))
        .merge(stable::router())
        .merge(lazer::router())
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
        json!({"service":"Resonance", "project":"Illyaverse", "stable":"login, presence, chat, friends, encrypted score submission, map/replay downloads, multiplayer and spectating", "lazer":"OAuth, map lookup/download, solo scores, REST chat, friends and SignalR hub subsets", "unsupported":["lazer extended replay uploads", "custom lazer mod settings", "native chat notification WebSocket", "cross-client multiplayer rooms", "stable target-practice submission", "tournament interfaces"], "compatibility":"experimental; live protocol tests do not replace actual native client validation", "score_policy":"submitted scores await moderator review before entering rankings"}),
    )
}
async fn health(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, common::ApiError> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(json!({"status":"ok", "service":"resonance"})))
}
async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}
