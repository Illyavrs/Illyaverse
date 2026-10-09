//! Synthetic sessions only; each test owns and removes a fresh PostgreSQL schema.
use crate::{common, lazer};
use axum::{
    Extension, Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::Response,
};
use futures_util::FutureExt;
use serde_json::{Value, json};
use sqlx::{
    PgPool, Postgres, Row, Transaction,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;

struct Fixture {
    control: PgPool,
    pool: PgPool,
    schema: String,
    router: Router,
}

struct Session {
    id: Uuid,
    user: i64,
    access: String,
    refresh: String,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("ILLYA_REFRESH_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("ILLYA_TEST_DATABASE_URL"))
            .expect("owned disposable PostgreSQL URL");
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
        let schema = format!("lazer_refresh_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema)
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let state = common::AppState {
            pool: pool.clone(),
            storage: Arc::new(std::env::temp_dir()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let router = lazer::router()
            .with_state(state)
            .layer(Extension(ConnectInfo(
                "127.0.0.1:19101".parse::<SocketAddr>().unwrap(),
            )));
        Self {
            control,
            pool,
            schema,
            router,
        }
    }

    async fn seed(&self, name: &str) -> Session {
        let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id")
            .bind(name).bind(format!("{name}@invalid.test")).fetch_one(&self.pool).await.unwrap();
        let session = Session {
            id: Uuid::new_v4(),
            user,
            access: common::secret(),
            refresh: common::secret(),
        };
        sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,clock_timestamp()+interval '1 hour',clock_timestamp()+interval '1 day')")
            .bind(session.id).bind(user).bind(common::token_hash(&session.access)).bind(common::token_hash(&session.refresh))
            .execute(&self.pool).await.unwrap();
        session
    }

    async fn refresh(&self, token: &str) -> Response {
        self.router
            .clone()
            .oneshot(refresh_request(token))
            .await
            .unwrap()
    }

    async fn me(&self, token: &str) -> Response {
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v2/me")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn lock_user(&self, user: i64) -> Transaction<'_, Postgres> {
        let mut tx = self.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx
    }

    // This observes the real database lock wait rather than scheduling a sleep
    // and assuming the HTTP route has discovered the old refresh credential.
    async fn wait_for_user_locks(&self, count: i64) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%FROM users%FOR UPDATE%'")
                    .bind(&self.schema).fetch_one(&self.control).await.unwrap();
                if waiting >= count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("refresh requests must reach the user-row lock");
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

async fn replacement(response: Response) -> (String, String) {
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert_eq!(value["token_type"], "Bearer");
    assert_eq!(value["expires_in"], 3600);
    (
        value["access_token"].as_str().unwrap().to_owned(),
        value["refresh_token"].as_str().unwrap().to_owned(),
    )
}

macro_rules! fixture_test {
    ($name:ident, $fixture:ident, $body:block) => {
        #[tokio::test]
        #[ignore = "requires owned ILLYA_REFRESH_TEST_DATABASE_URL or ILLYA_TEST_DATABASE_URL; creates/removes isolated schema"]
        async fn $name() {
            let $fixture = Fixture::new().await;
            let outcome = std::panic::AssertUnwindSafe(async $body)
                .catch_unwind()
                .await;
            $fixture.cleanup().await;
            if let Err(error) = outcome {
                std::panic::resume_unwind(error);
            }
        }
    };
}

fixture_test!(
    listed_session_uuid_survives_rotations_and_revokes_both_credentials,
    f,
    {
        let session = f.seed("RefreshIdentity").await;
        let created_at: chrono::DateTime<chrono::Utc> =
            sqlx::query_scalar("SELECT created_at FROM lazer_sessions WHERE id=$1")
                .bind(session.id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        let mut old_access = session.access;
        let mut old_refresh = session.refresh;
        for _ in 0..2 {
            let (access, refresh) = replacement(f.refresh(&old_refresh).await).await;
            assert_eq!(f.me(&old_access).await.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                f.refresh(&old_refresh).await.status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(f.me(&access).await.status(), StatusCode::OK);
            let rows = sqlx::query("SELECT id,created_at,access_hash,refresh_hash FROM lazer_sessions WHERE user_id=$1")
            .bind(session.user).fetch_all(&f.pool).await.unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get::<Uuid, _>("id"), session.id);
            assert_eq!(
                rows[0].get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                created_at
            );
            assert!(rows[0].get::<String, _>("access_hash") == common::token_hash(&access));
            assert!(rows[0].get::<String, _>("refresh_hash") == common::token_hash(&refresh));
            old_access = access;
            old_refresh = refresh;
        }

        // Mirrors Sanctuary's authorized revoke transaction after its web-actor
        // fence: users precede the target native session, and the UUID is the one
        // listed before either credential rotation. No token is returned to the UI.
        let mut tx = f.lock_user(session.user).await;
        let deleted = sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1 AND id=$2 AND refresh_expires_at>clock_timestamp()")
        .bind(session.user).bind(session.id).execute(&mut *tx).await.unwrap();
        assert_eq!(deleted.rows_affected(), 1);
        tx.commit().await.unwrap();
        assert_eq!(f.me(&old_access).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            f.refresh(&old_refresh).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_sessions WHERE user_id=$1")
            .bind(session.user)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
);

fixture_test!(
    two_queued_refreshes_have_one_winner_and_keep_the_same_uuid,
    f,
    {
        let session = f.seed("RefreshDoubleUse").await;
        let blocker = f.lock_user(session.user).await;
        let router_a = f.router.clone();
        let router_b = f.router.clone();
        let request_a = refresh_request(&session.refresh);
        let request_b = refresh_request(&session.refresh);
        let first = tokio::spawn(async move { router_a.oneshot(request_a).await.unwrap() });
        let second = tokio::spawn(async move { router_b.oneshot(request_b).await.unwrap() });
        f.wait_for_user_locks(2).await;
        blocker.commit().await.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(5), first)
            .await
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(5), second)
            .await
            .unwrap()
            .unwrap();
        let (winner, loser) = if first.status() == StatusCode::OK {
            (first, second)
        } else {
            (second, first)
        };
        assert_eq!(loser.status(), StatusCode::UNAUTHORIZED);
        let (access, refresh) = replacement(winner).await;
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM lazer_sessions WHERE user_id=$1")
            .bind(session.user)
            .fetch_all(&f.pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![session.id]);
        assert_eq!(f.me(&access).await.status(), StatusCode::OK);
        assert_eq!(
            f.me(&session.access).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            f.refresh(&session.refresh).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let (_, next_refresh) = replacement(f.refresh(&refresh).await).await;
        assert!(!next_refresh.is_empty());
    }
);

fixture_test!(refresh_expiry_is_rechecked_after_the_user_lock_wait, f, {
    let session = f.seed("RefreshQueuedExpiry").await;
    sqlx::query("UPDATE lazer_sessions SET refresh_expires_at=clock_timestamp()+interval '2 seconds' WHERE id=$1")
        .bind(session.id).execute(&f.pool).await.unwrap();
    let blocker = f.lock_user(session.user).await;
    let router = f.router.clone();
    let request = refresh_request(&session.refresh);
    let task = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
    f.wait_for_user_locks(1).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar(
                "SELECT refresh_expires_at<=clock_timestamp() FROM lazer_sessions WHERE id=$1",
            )
            .bind(session.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    blocker.commit().await.unwrap();
    let response = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let row =
        sqlx::query("SELECT id,access_hash,refresh_hash FROM lazer_sessions WHERE user_id=$1")
            .bind(session.user)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(row.get::<Uuid, _>("id"), session.id);
    assert!(row.get::<String, _>("access_hash") == common::token_hash(&session.access));
    assert!(row.get::<String, _>("refresh_hash") == common::token_hash(&session.refresh));
});

fixture_test!(
    queued_refresh_cannot_restore_a_revoked_or_restricted_session,
    f,
    {
        for restricted in [false, true] {
            let session = f
                .seed(if restricted {
                    "RefreshRestricted"
                } else {
                    "RefreshRevoked"
                })
                .await;
            let mut blocker = f.lock_user(session.user).await;
            let router = f.router.clone();
            let request = refresh_request(&session.refresh);
            let task = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
            f.wait_for_user_locks(1).await;
            if restricted {
                sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
                    .bind(session.user)
                    .execute(&mut *blocker)
                    .await
                    .unwrap();
            } else {
                sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1 AND id=$2")
                    .bind(session.user)
                    .bind(session.id)
                    .execute(&mut *blocker)
                    .await
                    .unwrap();
            }
            blocker.commit().await.unwrap();
            let response = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                f.me(&session.access).await.status(),
                StatusCode::UNAUTHORIZED
            );
            let rows = sqlx::query("SELECT id,refresh_hash FROM lazer_sessions WHERE user_id=$1")
                .bind(session.user)
                .fetch_all(&f.pool)
                .await
                .unwrap();
            if restricted {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].get::<Uuid, _>("id"), session.id);
                assert!(
                    rows[0].get::<String, _>("refresh_hash")
                        == common::token_hash(&session.refresh)
                );
            } else {
                assert!(rows.is_empty());
            }
        }
    }
);

fixture_test!(password_logins_create_independent_session_uuids, f, {
    use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
    let session = f.seed("RefreshNewLogins").await;
    let password = "Refresh-Password-Login-Fixture";
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes()).unwrap();
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string();
    sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1")
        .bind(session.user)
        .bind(hash)
        .execute(&f.pool)
        .await
        .unwrap();
    let mut logins = Vec::new();
    for _ in 0..2 {
        let response = f.router.clone().oneshot(Request::builder()
            .method("POST").uri("/oauth/token").header("content-type","application/json")
            .body(Body::from(json!({"grant_type":"password","username":"RefreshNewLogins","password":password}).to_string()))
            .unwrap()).await.unwrap();
        let (access, refresh) = replacement(response).await;
        let id: Uuid =
            sqlx::query_scalar("SELECT id FROM lazer_sessions WHERE user_id=$1 AND access_hash=$2")
                .bind(session.user)
                .bind(common::token_hash(&access))
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_ne!(id, session.id);
        logins.push((id, access, refresh));
    }
    assert_ne!(logins[0].0, logins[1].0);
    let (access, _) = replacement(f.refresh(&logins[0].2).await).await;
    let rotated_id: Uuid = sqlx::query_scalar("SELECT id FROM lazer_sessions WHERE access_hash=$1")
        .bind(common::token_hash(&access))
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(rotated_id, logins[0].0);
    assert_eq!(f.me(&logins[1].1).await.status(), StatusCode::OK);
    assert_eq!(f.me(&session.access).await.status(), StatusCode::OK);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_sessions WHERE user_id=$1")
        .bind(session.user)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(count, 3);
});
