use crate::{common, lazer, stable};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Extension, Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::post,
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use sqlx::{
    PgPool, Postgres, Transaction,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tower::ServiceExt;

struct Database {
    control: PgPool,
    pool: PgPool,
    schema: String,
}

impl Database {
    async fn new() -> Self {
        Self::with_connections(8).await
    }

    async fn with_connections(connections: u32) -> Self {
        let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "local disposable database required"
        );
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("credential_race_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema)
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        Self {
            control,
            pool,
            schema,
        }
    }

    fn state(&self) -> common::AppState {
        common::AppState {
            pool: self.pool.clone(),
            storage: Arc::new(std::env::temp_dir()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        }
    }

    fn router(&self) -> Router {
        Router::new()
            .route("/", post(stable::bancho))
            .merge(lazer::router())
            .with_state(self.state())
            .layer(Extension(ConnectInfo(
                "127.0.0.1:19001".parse::<SocketAddr>().unwrap(),
            )))
    }

    // Observing the actual PostgreSQL lock wait is the barrier: the route has
    // verified/read its old credential but cannot yet create or rotate a session.
    async fn wait_for_user_lock(&self) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%FROM users%FOR UPDATE%')")
                    .bind(&self.schema).fetch_one(&self.control).await.unwrap();
                if waiting {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session creator must wait on the user row");
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
    }
}

fn hashes(password: &str) -> (String, String) {
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).unwrap();
    let hash = |value: &str| {
        Argon2::default()
            .hash_password(value.as_bytes(), &salt)
            .unwrap()
            .to_string()
    };
    (
        hash(password),
        hash(&format!("{:x}", md5::compute(password))),
    )
}

async fn user(pool: &PgPool, name: &str, password: &str) -> i64 {
    let (plain, digest) = hashes(password);
    sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,$3,$4) RETURNING id")
        .bind(name).bind(format!("{name}@invalid.test")).bind(plain).bind(digest)
        .fetch_one(pool).await.unwrap()
}

async fn seed_sessions(pool: &PgPool, user: i64) -> String {
    sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at) VALUES($1,$2,$3,now()+interval '3 minutes')")
        .bind(uuid::Uuid::new_v4()).bind(common::secret()).bind(user).execute(pool).await.unwrap();
    let refresh = common::secret();
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')")
        .bind(uuid::Uuid::new_v4()).bind(user).bind(common::secret()).bind(common::token_hash(&refresh))
        .execute(pool).await.unwrap();
    refresh
}

async fn revoke<'a>(pool: &'a PgPool, user: i64, password: &str) -> Transaction<'a, Postgres> {
    let (plain, digest) = hashes(password);
    let mut tx = pool.begin().await.unwrap();
    // Same user-first ordering as Sanctuary's password-change/reset transaction.
    sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
        .bind(user)
        .bind(plain)
        .bind(digest)
        .execute(&mut *tx)
        .await
        .unwrap();
    for table in ["bancho_sessions", "lazer_sessions"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id=$1"))
            .bind(user)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx
}

async fn session_count(pool: &PgPool, user: i64) -> i64 {
    sqlx::query_scalar("SELECT (SELECT count(*) FROM bancho_sessions WHERE user_id=$1)+(SELECT count(*) FROM lazer_sessions WHERE user_id=$1)")
        .bind(user).fetch_one(pool).await.unwrap()
}

fn password_request(stable: bool, name: &str, password: &str) -> Request<Body> {
    if stable {
        Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::from(format!(
                "{name}\n{:x}\nb20261007|8|0|fixture:hashes:|1\n",
                md5::compute(password)
            )))
            .unwrap()
    } else {
        Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"grant_type":"password","username":name,"password":password}).to_string(),
            ))
            .unwrap()
    }
}

fn refresh_request(token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/oauth/token")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"grant_type":"refresh_token","refresh_token":token}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_password_logins_cannot_mint_sessions_after_concurrent_reset() {
    let database = Database::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        for stable in [true, false] {
            let name = if stable { "StableRace" } else { "LazerRace" };
            let old = "Race-Fixture-Old-Password";
            let new = "Race-Fixture-New-Password";
            let user = user(&database.pool, name, old).await;
            seed_sessions(&database.pool, user).await;
            let reset = revoke(&database.pool, user, new).await;
            let router = database.router();
            let task = tokio::spawn(async move {
                router
                    .oneshot(password_request(stable, name, old))
                    .await
                    .unwrap()
            });
            database.wait_for_user_lock().await;
            reset.commit().await.unwrap();
            let response = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            if stable {
                assert_eq!(response.headers().get("cho-token").unwrap(), "no");
            } else {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }
            assert_eq!(session_count(&database.pool, user).await, 0);

            // Fresh credentials still work through both real protocol handlers.
            let response = database
                .router()
                .oneshot(password_request(stable, name, new))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            if stable {
                assert_ne!(response.headers().get("cho-token").unwrap(), "no");
            } else {
                let json: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap())
                        .unwrap();
                assert!(
                    json["access_token"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
                );
            }
            assert_eq!(session_count(&database.pool, user).await, 1);
        }
    })
    .catch_unwind()
    .await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_refresh_rotation_serializes_with_reset_and_preserves_single_use() {
    let database = Database::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let user = user(&database.pool, "RefreshRace", "Refresh-Race-Old-Password").await;
        let refresh = seed_sessions(&database.pool, user).await;
        let reset = revoke(&database.pool, user, "Refresh-Race-New-Password").await;
        let router = database.router();
        let token = refresh.clone();
        let task =
            tokio::spawn(async move { router.oneshot(refresh_request(&token)).await.unwrap() });
        database.wait_for_user_lock().await;
        reset.commit().await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(session_count(&database.pool, user).await, 0);

        // A rotation which commits before reset is revoked by that reset too.
        let refresh = seed_sessions(&database.pool, user).await;
        let response = database
            .router()
            .oneshot(refresh_request(&refresh))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let replacement: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        let replacement = replacement["refresh_token"].as_str().unwrap();
        let replay = database
            .router()
            .oneshot(refresh_request(&refresh))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
        let reset = revoke(&database.pool, user, "Refresh-Race-Third-Password").await;
        reset.commit().await.unwrap();
        let response = database
            .router()
            .oneshot(refresh_request(replacement))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(session_count(&database.pool, user).await, 0);
    })
    .catch_unwind()
    .await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_authentication_succeeds_with_one_pool_connection() {
    let database = Database::with_connections(1).await;
    let result = std::panic::AssertUnwindSafe(async {
        let name = "SingleConnection";
        let password = "Single-Connection-Fixture-Password";
        user(&database.pool, name, password).await;
        for stable in [true, false] {
            let response = tokio::time::timeout(
                Duration::from_secs(5),
                database
                    .router()
                    .oneshot(password_request(stable, name, password)),
            )
            .await
            .expect(
                "authentication must not acquire a second connection while holding its transaction",
            )
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            if stable {
                assert_ne!(response.headers().get("cho-token").unwrap(), "no");
            } else {
                let json: Value =
                    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap())
                        .unwrap();
                let response = tokio::time::timeout(
                    Duration::from_secs(5),
                    database
                        .router()
                        .oneshot(refresh_request(json["refresh_token"].as_str().unwrap())),
                )
                .await
                .expect("refresh must reuse its transaction connection")
                .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
            }
        }
    })
    .catch_unwind()
    .await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
