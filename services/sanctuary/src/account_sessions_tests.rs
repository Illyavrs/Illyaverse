//! Own disposable PostgreSQL 17 fixture only; never uses deployment databases.
//! ILLYA_ACCOUNT_SESSIONS_TEST_DATABASE_URL=... cargo test -p sanctuary
//! account_session_visibility_and_revocation -- --ignored --nocapture
use super::*;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use chrono::Duration;
use jsonwebtoken::{EncodingKey, Header, encode};
use serde_json::Value;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, sync::Arc, time::Duration as StdDuration};
use tokio::sync::Mutex;
use tower::ServiceExt;

#[derive(Serialize)]
struct Claims {
    sub: i64,
    sid: Uuid,
    exp: usize,
}

fn state(db: PgPool) -> AppState {
    let root = std::env::temp_dir().join("illyaverse-account-sessions-synthetic-unused");
    AppState {
        db,
        secret: Arc::new("account-session-fixture-secret-over-thirty-two-bytes".into()),
        internal_key: Arc::new(String::new()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(crate::mail_delivery::MailConfig::disabled()),
        secure_cookie: true,
        media_root: root.join("media"),
        maps_root: root.join("maps"),
        replay_root: root.join("replays"),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(crate::maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        chat_runtime: Default::default(),
    }
}

async fn user(db: &PgPool, name: &str) -> i64 {
    let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'synthetic-password-hash','synthetic-legacy-hash') RETURNING id")
        .bind(name).bind(format!("{name}@example.invalid")).fetch_one(db).await.unwrap();
    sqlx::query(
        "INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'",
    )
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn web(state: &AppState, user: i64) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let expiry = Utc::now() + Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(id)
        .bind(user)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    let token = encode(
        &Header::default(),
        &Claims {
            sub: user,
            sid: id,
            exp: expiry.timestamp() as usize,
        },
        &EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (id, token)
}

async fn native(db: &PgPool, user: i64) -> (Uuid, Uuid) {
    let stable = Uuid::new_v4();
    let lazer = Uuid::new_v4();
    sqlx::query("INSERT INTO bancho_sessions(id,user_id,token_hash,expires_at) VALUES($1,$2,$3,clock_timestamp()+interval '1 hour')")
        .bind(stable).bind(user).bind(format!("stable-private-{stable}")).execute(db).await.unwrap();
    // An expired access token remains a valid login while refresh is active.
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at,last_seen_at) VALUES($1,$2,$3,$4,clock_timestamp()-interval '1 minute',clock_timestamp()+interval '1 day','2026-10-01T01:02:03Z')")
        .bind(lazer).bind(user).bind(format!("access-private-{lazer}")).bind(format!("refresh-private-{lazer}")).execute(db).await.unwrap();
    (stable, lazer)
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: &str,
) -> (StatusCode, HeaderMap, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

async fn count(db: &PgPool, table: &str, user: i64) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE user_id=$1"))
        .bind(user)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn audits(db: &PgPool, user: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE actor_id=$1")
        .bind(user)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn wait_for_lock(db: &PgPool, app_name: &str, fragment: &str) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(app_name).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("request must reach its expected authorization/resource lock");
}

async fn wait_until_expired(db: &PgPool, deadline: DateTime<Utc>) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1::timestamptz")
                .bind(deadline)
                .fetch_one(db)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(StdDuration::from_millis(10)).await;
        }
    })
    .await
    .expect("synthetic session deadline must pass");
}

#[tokio::test]
#[ignore = "requires ILLYA_ACCOUNT_SESSIONS_TEST_DATABASE_URL for an owned disposable PostgreSQL 17"]
async fn account_session_visibility_and_revocation() {
    let url = std::env::var("ILLYA_ACCOUNT_SESSIONS_TEST_DATABASE_URL")
        .expect("owned disposable database URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&control)
        .await
        .unwrap();
    assert!(
        (170000..180000).contains(&version),
        "fixture must use PostgreSQL 17"
    );
    let schema = format!("illya_own_sessions_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())])
        .application_name(&schema);
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let pool = db.clone();
    let application = schema.clone();
    let outcome = tokio::spawn(async move {
        let state = state(pool.clone());
        let app = crate::router(state.clone());
        let alice = user(&pool, "SessionsAlice").await;
        let bob = user(&pool, "SessionsBob").await;
        let (current, token) = web(&state, alice).await;
        let (other, other_token) = web(&state, alice).await;
        let (bob_web, _) = web(&state, bob).await;
        let (stable, lazer) = native(&pool, alice).await;
        let (bob_stable, bob_lazer) = native(&pool, bob).await;
        let expired = Uuid::new_v4();
        sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,clock_timestamp()-interval '1 minute')")
            .bind(expired).bind(alice).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,'expired-access-private','expired-refresh-private',clock_timestamp()-interval '1 day',clock_timestamp()-interval '1 hour')")
            .bind(Uuid::new_v4()).bind(alice).execute(&pool).await.unwrap();

        let (status, headers, listed) = request(&app, Method::GET, "/api/v1/me/sessions", &token).await;
        assert_eq!(status, StatusCode::OK, "{listed}");
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(request(&app, Method::GET, "/api/v1/me/sessions", "invalid").await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(listed["has_more"], false);
        let entries = listed["sessions"].as_array().unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0]["id"], current.to_string());
        assert_eq!(entries[0]["current"], true);
        assert_eq!(entries.iter().filter(|row| row["current"] == true).count(), 1);
        for row in entries {
            let mut keys: Vec<_> = row.as_object().unwrap().keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, ["client", "created_at", "current", "expires_at", "id", "last_seen_at"]);
            assert_ne!(row["id"], bob_web.to_string());
            assert_ne!(row["id"], bob_stable.to_string());
            assert_ne!(row["id"], bob_lazer.to_string());
            assert_ne!(row["id"], expired.to_string());
            if row["client"] == "lazer" {
                assert_eq!(row["last_seen_at"], "2026-10-01T01:02:03Z");
            } else {
                assert!(row["last_seen_at"].is_null());
            }
        }
        let serialized = listed.to_string();
        for secret in ["private", "password", "token", "hash", "example.invalid", "access", "refresh"] {
            assert!(!serialized.contains(secret), "session DTO leaked {secret}");
        }

        // Missing and another owner's IDs produce the same response and audit nothing.
        let audit_before = audits(&pool, alice).await;
        for (client, foreign) in [("web", bob_web), ("stable", bob_stable), ("lazer", bob_lazer)] {
            let (status, _, foreign_body) = request(&app, Method::DELETE, &format!("/api/v1/me/sessions/{client}/{foreign}"), &token).await;
            let (missing_status, _, missing_body) = request(&app, Method::DELETE, &format!("/api/v1/me/sessions/{client}/{}", Uuid::new_v4()), &token).await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(missing_status, status);
            assert_eq!(missing_body, foreign_body);
        }
        assert_eq!(audits(&pool, alice).await, audit_before);
        assert_eq!(request(&app, Method::DELETE, &format!("/api/v1/me/sessions/unknown/{other}"), &token).await.0, StatusCode::NOT_FOUND);

        // Cookie mutations require the trusted origin and must leave all sessions intact.
        for (method, path) in [(Method::DELETE, format!("/api/v1/me/sessions/web/{other}")), (Method::POST, "/api/v1/me/sessions/revoke-others".into())] {
          for origin in [None, Some("http://untrusted.example")] {
            let mut builder = Request::builder().method(method.clone()).uri(&path)
                .header(header::COOKIE, format!("illya_session={token}"));
            if let Some(origin) = origin { builder = builder.header(header::ORIGIN, origin); }
            let response = app.clone().oneshot(builder.body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
          }
        }
        assert_eq!(count(&pool, "sessions", alice).await, 3);

        for (client, id) in [("web", other), ("stable", stable), ("lazer", lazer)] {
            let (status, headers, body) = request(&app, Method::DELETE, &format!("/api/v1/me/sessions/{client}/{id}"), &token).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(body, json!({"ok":true,"current_revoked":false}));
            assert!(!headers.contains_key(header::SET_COOKIE));
        }
        assert_eq!(request(&app, Method::GET, "/api/v1/me/sessions", &other_token).await.0, StatusCode::UNAUTHORIZED);
        assert!(!sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM bancho_sessions WHERE token_hash=$1)").bind(format!("stable-private-{stable}")).fetch_one(&pool).await.unwrap());
        assert!(!sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM lazer_sessions WHERE access_hash=$1 OR refresh_hash=$2)").bind(format!("access-private-{lazer}")).bind(format!("refresh-private-{lazer}")).fetch_one(&pool).await.unwrap());
        for table in ["sessions", "bancho_sessions", "lazer_sessions"] { assert_eq!(count(&pool, table, bob).await, 1); }

        // Bounded listing keeps the current session visible even among newer logins.
        for _ in 0..105 { web(&state, alice).await; }
        let (_, _, bounded) = request(&app, Method::GET, "/api/v1/me/sessions", &token).await;
        assert_eq!(bounded["sessions"].as_array().unwrap().len(), LIST_LIMIT);
        assert_eq!(bounded["sessions"][0]["id"], current.to_string());
        assert_eq!(bounded["has_more"], true);
        native(&pool, alice).await;
        let token_id = Uuid::new_v4();
        sqlx::query("INSERT INTO account_recovery_tokens(id,user_id,purpose,token_hash,email_snapshot,expires_at) VALUES($1,$2,'reset',decode(repeat('11',32),'hex'),'SessionsAlice@example.invalid',clock_timestamp()+interval '1 hour')")
            .bind(token_id).bind(alice).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO mail_jobs(id,token_id,user_id,kind,encrypted_payload,status) VALUES($1,$2,$3,'reset',decode(repeat('11',41),'hex'),'pending')")
            .bind(Uuid::new_v4()).bind(token_id).bind(alice).execute(&pool).await.unwrap();
        let recovery_before: Value = sqlx::query_scalar("SELECT jsonb_build_object('tokens',(SELECT jsonb_agg(to_jsonb(t)) FROM account_recovery_tokens t WHERE user_id=$1),'mail',(SELECT jsonb_agg(to_jsonb(m)) FROM mail_jobs m WHERE user_id=$1))")
            .bind(alice).fetch_one(&pool).await.unwrap();
        let (status, _, value) = request(&app, Method::POST, "/api/v1/me/sessions/revoke-others", &token).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let retained: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1").bind(alice).fetch_all(&pool).await.unwrap();
        assert_eq!(retained, vec![current]);
        assert_eq!(count(&pool, "bancho_sessions", alice).await, 0);
        assert_eq!(count(&pool, "lazer_sessions", alice).await, 0);
        let recovery_after: Value = sqlx::query_scalar("SELECT jsonb_build_object('tokens',(SELECT jsonb_agg(to_jsonb(t)) FROM account_recovery_tokens t WHERE user_id=$1),'mail',(SELECT jsonb_agg(to_jsonb(m)) FROM mail_jobs m WHERE user_id=$1))")
            .bind(alice).fetch_one(&pool).await.unwrap();
        assert_eq!(recovery_after, recovery_before, "logout must not consume recovery links or cancel mail");
        for table in ["sessions", "bancho_sessions", "lazer_sessions"] { assert_eq!(count(&pool, table, bob).await, 1); }

        // Authority revoked/restricted while a request queues cannot publish a logout.
        for (index, (change, bulk)) in [("session",false), ("restriction",false), ("session",true), ("restriction",true)].into_iter().enumerate() {
            let actor = user(&pool, &format!("QueuedSessions{index}")).await;
            let (sid, credential) = web(&state, actor).await;
            let (target, _) = web(&state, actor).await;
            native(&pool, actor).await;
            let before = audits(&pool, actor).await;
            let mut blocking = pool.begin().await.unwrap();
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(actor).fetch_one(&mut *blocking).await.unwrap();
            let queued_app = app.clone();
            let queued = tokio::spawn(async move {
                let (method,path) = if bulk { (Method::POST,"/api/v1/me/sessions/revoke-others".into()) } else { (Method::DELETE,format!("/api/v1/me/sessions/web/{target}")) };
                request(&queued_app, method, &path, &credential).await
            });
            wait_for_lock(&pool, &application, "SELECT id FROM users WHERE id=ANY").await;
            if change == "session" {
                sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap();
            } else {
                sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(actor).execute(&mut *blocking).await.unwrap();
            }
            blocking.commit().await.unwrap();
            assert_eq!(queued.await.unwrap().0, StatusCode::UNAUTHORIZED);
            assert!(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=$1)").bind(target).fetch_one(&pool).await.unwrap());
            assert_eq!(count(&pool,"bancho_sessions",actor).await,1);
            assert_eq!(count(&pool,"lazer_sessions",actor).await,1);
            assert_eq!(audits(&pool, actor).await, before);
        }

        // Natural expiry during a resource wait rolls back the delete and audit.
        let expiring_user = user(&pool, "ExpiringSessions").await;
        let (sid, credential) = web(&state, expiring_user).await;
        let (target, _) = web(&state, expiring_user).await;
        let deadline: DateTime<Utc> = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1 RETURNING expires_at")
            .bind(sid).fetch_one(&pool).await.unwrap();
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE").bind(target).fetch_one(&mut *blocking).await.unwrap();
        let queued_app = app.clone();
        let queued = tokio::spawn(async move {
            request(&queued_app, Method::DELETE, &format!("/api/v1/me/sessions/web/{target}"), &credential).await
        });
        wait_for_lock(&pool, &application, "DELETE FROM sessions WHERE user_id=$1 AND id=$2").await;
        wait_until_expired(&pool, deadline).await;
        blocking.commit().await.unwrap();
        assert_eq!(queued.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert!(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=$1)").bind(target).fetch_one(&pool).await.unwrap());
        assert_eq!(audits(&pool, expiring_user).await, 0);

        let audit_details: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events WHERE actor_id=$1").bind(alice).fetch_all(&pool).await.unwrap();
        for event in audit_details {
            let text = event.to_string();
            assert!(!text.contains("private") && !text.contains("hash") && !text.contains("password") && !text.contains(&token));
        }
        let (status, headers, body) = request(&app, Method::DELETE, &format!("/api/v1/me/sessions/web/{current}"), &token).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["current_revoked"], true);
        let cookie = headers[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("Max-Age=0") && cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax") && cookie.contains("; Secure"));
        assert_eq!(request(&app, Method::GET, "/api/v1/me/sessions", &token).await.0, StatusCode::UNAUTHORIZED);
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
