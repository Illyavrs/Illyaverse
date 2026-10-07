//! Run only against an owned disposable PostgreSQL 17 database.
//! ILLYA_AUTH_MUTATION_TEST_DATABASE_URL=... cargo test -p sanctuary
//! auth_mutation_guards_and_queued_writes -- --ignored
//! Every run creates and removes its own schema and synthetic media directory.
use super::*;
use crate::{account_recovery, mail_delivery, maps, router};
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration as StdDuration};
use tokio::sync::Mutex;
use tower::ServiceExt;

fn fixture_state(db: PgPool, root: PathBuf) -> AppState {
    AppState {
        db,
        secret: Arc::new("auth-mutation-fixture-session-secret-over-thirty-two-bytes".into()),
        internal_key: Arc::new(String::new()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: root.join("media"),
        maps_root: root.join("maps"),
        replay_root: root.join("replays"),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    }
}

async fn user(db: &PgPool, name: &str, role_ids: &[i64]) -> i64 {
    let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'synthetic','synthetic') RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).fetch_one(db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,unnest($2::bigint[])")
        .bind(id)
        .bind(role_ids)
        .execute(db)
        .await
        .unwrap();
    id
}

async fn session(state: &AppState, id: i64) -> (Uuid, String) {
    let sid = Uuid::new_v4();
    let expiry = Utc::now() + Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    let token = encode(
        &Header::default(),
        &Claims {
            sub: id,
            sid,
            exp: expiry.timestamp() as usize,
        },
        &EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (sid, token)
}

fn snapshot(id: i64, session: Uuid, permissions: &[&str]) -> User {
    User {
        id,
        session,
        permissions: permissions.iter().map(|value| (*value).into()).collect(),
    }
}

async fn request(
    app: &axum::Router,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn wait_for_lock(db: &PgPool, application: &str, fragment: &str, count: i64) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND wait_event_type='Lock' AND query LIKE $2")
                .bind(application).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if waiting >= count { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("queued mutation must reach its expected database lock");
}

async fn wait_until_expired(db: &PgPool, expires_at: DateTime<Utc>) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1::timestamptz")
                .bind(expires_at)
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
    .expect("the fixture session deadline must pass");
}

fn unauthorized<T>(result: ApiResult<T>) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("queued stale mutation must be denied"),
    };
    assert_eq!(error.0, StatusCode::UNAUTHORIZED);
    assert_eq!(error.1, "unauthorized");
}

async fn audits(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(db)
        .await
        .unwrap()
}

async fn user_data(db: &PgPool, id: i64) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('bio',bio,'preferences',preferences,'password_hash',password_hash,'password_md5_hash',password_md5_hash,'avatar_url',avatar_url) FROM users WHERE id=$1")
        .bind(id).fetch_one(db).await.unwrap()
}

async fn recovery_data(db: &PgPool, id: i64, recipient: &[u8]) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('limit',(SELECT to_jsonb(l) FROM account_recovery_limits l WHERE recipient_hash=$2),'tokens',(SELECT COALESCE(jsonb_agg(to_jsonb(t) ORDER BY id),'[]'::jsonb) FROM account_recovery_tokens t WHERE user_id=$1),'jobs',(SELECT COALESCE(jsonb_agg(to_jsonb(j) ORDER BY id),'[]'::jsonb) FROM mail_jobs j WHERE user_id=$1))")
        .bind(id).bind(recipient).fetch_one(db).await.unwrap()
}

async fn upload_avatar(app: &axum::Router, token: &str) -> (StatusCode, Value) {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(16, 16)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let boundary = "auth-mutation-synthetic-image";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\navatar\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"synthetic.png\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
    body.extend_from_slice(&png.into_inner());
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/me/media")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn directory_entries(path: &std::path::Path) -> usize {
    let mut directory = tokio::fs::read_dir(path).await.unwrap();
    let mut count = 0;
    while directory.next_entry().await.unwrap().is_some() {
        count += 1;
    }
    count
}

async fn media_get(app: &axum::Router, path: &str) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, content_type, bytes)
}

async fn assert_private_staging(app: &axum::Router, name: &str, private_bytes: &[u8]) {
    let mut paths = vec![
        format!("/media/.illya-media-staging/{name}"),
        format!("/media/%2eillya-media-staging/{name}"),
        format!("/media/.illya-media-staging%2f{name}"),
        format!("/media%2f.illya-media-staging/{name}"),
    ];
    for kind in ["avatar", "banner", "background"] {
        paths.extend([
            format!("/media/{kind}/../.illya-media-staging/{name}"),
            format!("/media/{kind}/%2e%2e/.illya-media-staging/{name}"),
            format!("/media/{kind}/%2E%2E%2F.illya-media-staging/{name}"),
        ]);
    }
    for path in paths {
        let (status, _, bytes) = media_get(app, &path).await;
        assert!(
            matches!(status, StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST),
            "private staging route {path} returned {status}"
        );
        assert_ne!(
            bytes, private_bytes,
            "private staging content leaked at {path}"
        );
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_AUTH_MUTATION_TEST_DATABASE_URL pointing to an owned disposable PostgreSQL 17"]
async fn auth_mutation_guards_and_queued_writes() {
    let url = std::env::var("ILLYA_AUTH_MUTATION_TEST_DATABASE_URL")
        .expect("Set the disposable auth-mutation fixture URL");
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
        "auth-mutation fixture must run PostgreSQL 17"
    );
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("illya_auth_mutation_{suffix}");
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
    sqlx::query("CREATE TABLE auth_mutation_resources(id bigint PRIMARY KEY,value text NOT NULL)")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO auth_mutation_resources(id,value) VALUES(1,'original')")
        .execute(&db)
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("illyaverse-auth-mutation-{suffix}"));
    assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
    let pool = db.clone();
    let application = schema.clone();
    let test_root = root.clone();
    let outcome = tokio::spawn(async move {
        let state = fixture_state(pool.clone(), test_root);
        tokio::fs::create_dir_all(&state.media_root).await.unwrap();
        let actor_id = user(&pool, "MutationActor", &[1]).await;
        let role_id: i64 = sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES('mutation_manager',ARRAY['users.sessions.revoke']) RETURNING id")
            .fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
            .bind(actor_id).bind(role_id).execute(&pool).await.unwrap();

        // A Session guard neither loads cached permissions nor waits for role-table fences.
        let (sid, _) = session(&state, actor_id).await;
        let mut fence = pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *fence).await.unwrap();
        let worker_pool = pool.clone();
        let worker = tokio::spawn(async move {
            let mut tx = worker_pool.begin().await?;
            let lease = mutation_actor(&mut tx, &snapshot(actor_id, sid, &["*"]), &[], MutationScope::Session).await?;
            assert!(lease.permissions.is_empty());
            ensure_mutation_actor(&mut tx, &lease).await?;
            tx.commit().await?;
            Ok::<_, ApiError>(())
        });
        tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap().unwrap();
        fence.rollback().await.unwrap();

        // Permission scopes reload authority and keep the intended table-lock strength.
        for (scope, expected_lock) in [(MutationScope::Permissions, "ShareLock"), (MutationScope::RoleChanges, "ShareRowExclusiveLock")] {
            let (sid, _) = session(&state, actor_id).await;
            let mut tx = pool.begin().await.unwrap();
            let lease = mutation_actor(&mut tx, &snapshot(actor_id, sid, &["stale.permission"]), &[], scope).await.unwrap();
            assert!(lease.permissions.iter().any(|permission| permission == "users.sessions.revoke"));
            assert!(!lease.permissions.iter().any(|permission| permission == "stale.permission"));
            let modes: Vec<String> = sqlx::query_scalar("SELECT mode FROM pg_locks WHERE pid=pg_backend_pid() AND relation='roles'::regclass AND granted")
                .fetch_all(&mut *tx).await.unwrap();
            assert!(modes.iter().any(|mode| mode == expected_lock));
            ensure_mutation_actor(&mut tx, &lease).await.unwrap();
            tx.rollback().await.unwrap();
        }

        // A stale snapshot queued at the permission fence cannot authorize later writes.
        for change in ["permission", "session", "restriction"] {
            sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(actor_id).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
                .bind(actor_id).bind(role_id).execute(&pool).await.unwrap();
            let (sid, _) = session(&state, actor_id).await;
            let before = user_data(&pool, actor_id).await;
            let mut blocking = pool.begin().await.unwrap();
            sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocking).await.unwrap();
            let worker_pool = pool.clone();
            let worker = tokio::spawn(async move {
                let mut tx = worker_pool.begin().await?;
                let lease = mutation_actor(&mut tx, &snapshot(actor_id, sid, &["users.sessions.revoke"]), &[], MutationScope::Permissions).await?;
                lease.require("users.sessions.revoke")?;
                sqlx::query("UPDATE users SET bio='unauthorized queued change' WHERE id=$1").bind(actor_id).execute(&mut *tx).await?;
                ensure_mutation_actor(&mut tx, &lease).await?;
                tx.commit().await?;
                Ok::<_, ApiError>(())
            });
            wait_for_lock(&pool, &application, "LOCK TABLE roles,user_roles IN SHARE MODE", 1).await;
            match change {
                "permission" => { sqlx::query("DELETE FROM user_roles WHERE user_id=$1 AND role_id=$2").bind(actor_id).bind(role_id).execute(&mut *blocking).await.unwrap(); }
                "session" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap(); }
                "restriction" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(actor_id).execute(&mut *blocking).await.unwrap(); }
                _ => unreachable!(),
            }
            blocking.commit().await.unwrap();
            let result = tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap();
            let error = result.expect_err("queued authorization must be denied");
            assert_eq!(error.0, if change == "permission" { StatusCode::FORBIDDEN } else { StatusCode::UNAUTHORIZED });
            assert_eq!(user_data(&pool, actor_id).await, before);
        }
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(actor_id).execute(&pool).await.unwrap();

        // Initial liveness must be checked after acquiring an unchanged session row.
        let (sid, _) = session(&state, actor_id).await;
        let expires: DateTime<Utc> = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1 RETURNING expires_at")
            .bind(sid).fetch_one(&pool).await.unwrap();
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE").bind(sid).fetch_one(&mut *blocking).await.unwrap();
        let worker_pool = pool.clone();
        let worker = tokio::spawn(async move {
            let mut tx = worker_pool.begin().await?;
            mutation_actor(&mut tx, &snapshot(actor_id, sid, &[]), &[], MutationScope::Session).await?;
            tx.commit().await?;
            Ok::<_, ApiError>(())
        });
        wait_for_lock(&pool, &application, "SELECT expires_at FROM sessions WHERE id=$1 AND user_id=$2 FOR UPDATE", 1).await;
        wait_until_expired(&pool, expires).await;
        blocking.commit().await.unwrap();
        unauthorized(tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap());

        // Resource waits cannot extend the original authorization deadline.
        let (sid, _) = session(&state, actor_id).await;
        sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1")
            .bind(sid).execute(&pool).await.unwrap();
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM auth_mutation_resources WHERE id=1 FOR UPDATE").fetch_one(&mut *blocking).await.unwrap();
        let worker_pool = pool.clone();
        let (captured, deadline) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            let mut tx = worker_pool.begin().await?;
            let lease = mutation_actor(&mut tx, &snapshot(actor_id, sid, &[]), &[], MutationScope::Session).await?;
            assert!(captured.send(lease.expires_at).is_ok());
            sqlx::query("SELECT id FROM auth_mutation_resources WHERE id=1 FOR UPDATE").fetch_one(&mut *tx).await?;
            sqlx::query("UPDATE auth_mutation_resources SET value='expired change' WHERE id=1").execute(&mut *tx).await?;
            // Even a write to the session row cannot replace the captured deadline.
            sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(sid).execute(&mut *tx).await?;
            ensure_mutation_actor(&mut tx, &lease).await?;
            tx.commit().await?;
            Ok::<_, ApiError>(())
        });
        let expires = tokio::time::timeout(StdDuration::from_secs(5), deadline).await.unwrap().unwrap();
        wait_for_lock(&pool, &application, "SELECT id FROM auth_mutation_resources WHERE id=1 FOR UPDATE", 1).await;
        wait_until_expired(&pool, expires).await;
        blocking.commit().await.unwrap();
        unauthorized(tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap());
        let resource: String = sqlx::query_scalar("SELECT value FROM auth_mutation_resources WHERE id=1").fetch_one(&pool).await.unwrap();
        assert_eq!(resource, "original");

        // The real moderation handler must roll back both its resource write and
        // audit event when the actor expires while waiting for that resource.
        let moderator = user(&pool, "ExpiredMapModerator", &[2]).await;
        let (sid, token) = session(&state, moderator).await;
        let map_id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,stars,md5,osu_path) VALUES('Synthetic mutation map','Synthetic artist','Synthetic creator','Fixture',0,1,'e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1','unused-synthetic-path') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let before_audits = audits(&pool).await;
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM beatmaps WHERE id=$1 FOR UPDATE").bind(map_id).fetch_one(&mut *blocking).await.unwrap();
        let expires: DateTime<Utc> = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1 RETURNING expires_at")
            .bind(sid).fetch_one(&pool).await.unwrap();
        let admin_app = router(state.clone());
        let queued = tokio::spawn(async move {
            request(&admin_app, Method::PATCH, &format!("/api/v1/admin/beatmaps/{map_id}"), &token, Some(json!({"status":"ranked"}))).await
        });
        wait_for_lock(&pool, &application, "UPDATE beatmaps SET status=$2 WHERE id=$1", 1).await;
        wait_until_expired(&pool, expires).await;
        blocking.commit().await.unwrap();
        let (status, value) = tokio::time::timeout(StdDuration::from_secs(5), queued).await.unwrap().unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "expired moderation: {value}");
        let status: String = sqlx::query_scalar("SELECT status FROM beatmaps WHERE id=$1").bind(map_id).fetch_one(&pool).await.unwrap();
        assert_eq!(status, "pending");
        assert_eq!(audits(&pool).await, before_audits);

        // An operation may deliberately revoke/demote/restrict its own actor.
        let self_id = user(&pool, "SelfMutation", &[1]).await;
        let (sid, _) = session(&state, self_id).await;
        let mut tx = pool.begin().await.unwrap();
        let lease = mutation_actor(&mut tx, &snapshot(self_id, sid, &["*"]), &[], MutationScope::RoleChanges).await.unwrap();
        revoke_sessions(&mut tx, self_id, None).await.unwrap();
        sqlx::query("DELETE FROM user_roles WHERE user_id=$1").bind(self_id).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(self_id).execute(&mut *tx).await.unwrap();
        ensure_mutation_actor(&mut tx, &lease).await.unwrap();
        tx.commit().await.unwrap();
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE user_id=$1) OR EXISTS(SELECT 1 FROM user_roles WHERE user_id=$1)")
            .bind(self_id).fetch_one(&pool).await.unwrap();
        assert!(!valid);

        // Sorted actor/target locks leave higher IDs free until the lowest ID is acquired.
        let low = user(&pool, "MutationLow", &[1]).await;
        let high = user(&pool, "MutationHigh", &[1]).await;
        assert!(low < high);
        let (low_sid, _) = session(&state, low).await;
        let (high_sid, _) = session(&state, high).await;
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(low).fetch_one(&mut *blocking).await.unwrap();
        let mut workers = Vec::new();
        for (id, sid, extra) in [(high, high_sid, low), (low, low_sid, high)] {
            let worker_pool = pool.clone();
            workers.push(tokio::spawn(async move {
                let mut tx = worker_pool.begin().await?;
                let lease = mutation_actor(&mut tx, &snapshot(id, sid, &[]), &[extra, extra, id], MutationScope::Session).await?;
                ensure_mutation_actor(&mut tx, &lease).await?;
                tx.commit().await?;
                Ok::<_, ApiError>(())
            }));
        }
        wait_for_lock(&pool, &application, "ORDER BY id FOR UPDATE", 2).await;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE NOWAIT").bind(high).fetch_one(&mut *blocking).await.unwrap();
        blocking.commit().await.unwrap();
        for worker in workers { tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap().unwrap(); }

        // The real verification path must recheck its actor after recipient waits,
        // including a rate denial that would otherwise appear harmless to commit.
        let verification_user = user(&pool, "VerificationMutation", &[1]).await;
        let mut mail_state = state.clone();
        mail_state.mail = Arc::new(mail_delivery::MailConfig::local_for_test(
            "http://localhost:3000", "127.0.0.1:1".parse().unwrap(), [29; 32]));
        let (sid, _) = session(&mail_state, verification_user).await;
        assert!(account_recovery::enqueue_verification(&mail_state, &snapshot(verification_user, sid, &[])).await.unwrap());
        let recipient: Vec<u8> = sqlx::query_scalar("SELECT recipient_hash FROM account_recovery_limits").fetch_one(&pool).await.unwrap();
        let mail_app = router(mail_state.clone());
        for (endpoint, rate_denied) in [(true, false), (true, true), (false, true)] {
            mail_state.limits.lock().await.clear();
            sqlx::query("UPDATE account_recovery_limits SET requests=$2,last_requested_at=CASE WHEN $2=5 THEN clock_timestamp() ELSE NULL END WHERE recipient_hash=$1")
                .bind(&recipient).bind(if rate_denied {5_i32} else {0_i32}).execute(&pool).await.unwrap();
            let before = recovery_data(&pool, verification_user, &recipient).await;
            let (sid, token) = session(&mail_state, verification_user).await;
            let mut blocking = pool.begin().await.unwrap();
            sqlx::query("SELECT recipient_hash FROM account_recovery_limits WHERE recipient_hash=$1 FOR UPDATE").bind(&recipient).fetch_one(&mut *blocking).await.unwrap();
            let queued_state = mail_state.clone();
            let queued_app = mail_app.clone();
            let queued = tokio::spawn(async move {
                if endpoint {
                    let (status, value) = request(&queued_app, Method::POST, "/api/v1/auth/email/request", &token, Some(json!({}))).await;
                    assert_eq!(status, StatusCode::UNAUTHORIZED, "verification after revocation: {value}");
                } else {
                    unauthorized(account_recovery::enqueue_verification(&queued_state, &snapshot(verification_user, sid, &[])).await);
                }
            });
            wait_for_lock(&pool, &application, "SELECT window_started_at,requests,last_requested_at FROM account_recovery_limits", 1).await;
            sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap();
            blocking.commit().await.unwrap();
            tokio::time::timeout(StdDuration::from_secs(5), queued).await.unwrap().unwrap();
            assert_eq!(recovery_data(&pool, verification_user, &recipient).await, before);
        }

        // A recipient-first verification queued behind a credential change cannot
        // make that change wait for its own limiter row or publish stale mail.
        sqlx::query("UPDATE account_recovery_limits SET requests=0,last_requested_at=NULL WHERE recipient_hash=$1")
            .bind(&recipient).execute(&pool).await.unwrap();
        let before = recovery_data(&pool, verification_user, &recipient).await;
        let (sid, _) = session(&mail_state, verification_user).await;
        let mut changing = pool.begin().await.unwrap();
        let lease = mutation_actor(&mut changing, &snapshot(verification_user, sid, &[]), &[], MutationScope::Session).await.unwrap();
        let queued_state = mail_state.clone();
        let worker = tokio::spawn(async move {
            account_recovery::enqueue_verification(&queued_state, &snapshot(verification_user, sid, &[])).await
        });
        wait_for_lock(&pool, &application, "ORDER BY id FOR UPDATE", 1).await;
        tokio::time::timeout(StdDuration::from_secs(5), async {
            revoke_sessions(&mut changing, verification_user, None).await.unwrap();
            ensure_mutation_actor(&mut changing, &lease).await.unwrap();
            changing.commit().await.unwrap();
        }).await.expect("credential revocation must not wait on the recipient-first guard");
        let expected = recovery_data(&pool, verification_user, &recipient).await;
        assert_eq!(expected["limit"], before["limit"]);
        unauthorized(tokio::time::timeout(StdDuration::from_secs(5), worker).await.unwrap().unwrap());
        let after = recovery_data(&pool, verification_user, &recipient).await;
        assert_eq!(after["limit"], before["limit"]);
        assert_eq!(after, expected);

        // Real self-service endpoints must not publish queued writes after invalidation.
        let original_password = "synthetic-auth-mutation-original-password";
        let new_password = "synthetic-auth-mutation-new-password";
        let (hash, legacy) = hash_password(original_password.into()).await.unwrap();
        sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
            .bind(actor_id).bind(&hash).bind(&legacy).execute(&pool).await.unwrap();
        let app = router(state.clone());
        for (method, path, body) in [
            (Method::PATCH, "/api/v1/me/profile", json!({"bio":"queued biography"})),
            (Method::PATCH, "/api/v1/me/preferences", json!({"locale":"ja"})),
            (Method::POST, "/api/v1/me/password", json!({"current_password":original_password,"new_password":new_password})),
        ] {
            for invalidation in ["session", "restriction"] {
                sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(actor_id).execute(&pool).await.unwrap();
                let (sid, token) = session(&state, actor_id).await;
                let before = user_data(&pool, actor_id).await;
                let before_audits = audits(&pool).await;
                let mut blocking = pool.begin().await.unwrap();
                sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(actor_id).fetch_one(&mut *blocking).await.unwrap();
                let queued_app = app.clone();
                let queued_method = method.clone();
                let queued_body = body.clone();
                let queued = tokio::spawn(async move { request(&queued_app, queued_method, path, &token, Some(queued_body)).await });
                wait_for_lock(&pool, &application, "ORDER BY id FOR UPDATE", 1).await;
                if invalidation == "session" {
                    sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap();
                } else {
                    sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(actor_id).execute(&mut *blocking).await.unwrap();
                }
                blocking.commit().await.unwrap();
                let (status, value) = tokio::time::timeout(StdDuration::from_secs(5), queued).await.unwrap().unwrap();
                assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} after {invalidation}: {value}");
                assert_eq!(user_data(&pool, actor_id).await, before);
                assert_eq!(audits(&pool).await, before_audits);
            }
        }
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(actor_id).execute(&pool).await.unwrap();

        // A denied upload cleans its private staged file without publishing it.
        let (sid, token) = session(&state, actor_id).await;
        let before = user_data(&pool, actor_id).await;
        let before_audits = audits(&pool).await;
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(actor_id).fetch_one(&mut *blocking).await.unwrap();
        let queued_app = app.clone();
        let queued = tokio::spawn(async move { upload_avatar(&queued_app, &token).await });
        wait_for_lock(&pool, &application, "ORDER BY id FOR UPDATE", 1).await;
        let staging = state.media_root.join(".illya-media-staging");
        assert_eq!(directory_entries(&staging).await, 1);
        assert_eq!(directory_entries(&state.media_root.join("avatar")).await, 0);
        let staged = tokio::fs::read_dir(&staging).await.unwrap().next_entry().await.unwrap().unwrap();
        let staged_name = staged.file_name().into_string().unwrap();
        let private_bytes = tokio::fs::read(staged.path()).await.unwrap();
        assert_private_staging(&app, &staged_name, &private_bytes).await;
        sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap();
        blocking.commit().await.unwrap();
        let (status, value) = tokio::time::timeout(StdDuration::from_secs(5), queued).await.unwrap().unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "denied upload: {value}");
        assert_eq!(user_data(&pool, actor_id).await, before);
        assert_eq!(audits(&pool).await, before_audits);
        assert_eq!(directory_entries(&staging).await, 0);
        assert_eq!(directory_entries(&state.media_root.join("avatar")).await, 0);

        // Same-volume publication keeps the existing image URL usable through
        // the explicitly mounted public kind while private staging stays empty.
        let (_, token) = session(&state, actor_id).await;
        let (status, value) = upload_avatar(&app, &token).await;
        assert_eq!(status, StatusCode::OK, "published upload: {value}");
        let url = value["url"].as_str().unwrap();
        assert!(url.starts_with("/media/avatar/") && url.ends_with(".webp"));
        let expected = tokio::fs::read(state.media_root.join(url.strip_prefix("/media/").unwrap())).await.unwrap();
        let (status, content_type, bytes) = media_get(&app, url).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("image/webp"));
        assert_eq!(bytes, expected);
        assert!(image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).is_ok());
        assert_eq!(directory_entries(&staging).await, 0);

        // Successful password changes retain the initiating session and revoke its peers.
        let (current_sid, token) = session(&state, actor_id).await;
        let (other_sid, _) = session(&state, actor_id).await;
        let (status, value) = request(&app, Method::POST, "/api/v1/me/password", &token,
            Some(json!({"current_password":original_password,"new_password":new_password}))).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let sessions: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1").bind(actor_id).fetch_all(&pool).await.unwrap();
        assert!(sessions.contains(&current_sid));
        assert!(!sessions.contains(&other_sid));
        let current_hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1").bind(actor_id).fetch_one(&pool).await.unwrap();
        assert!(verify(new_password.into(), current_hash).await);

        // Endpoint self-revocation and demotion must not invalidate their own final guard.
        let self_revoker = user(&pool, "EndpointSelfRevoker", &[role_id]).await;
        let (_, token) = session(&state, self_revoker).await;
        let (status, value) = request(&app, Method::POST, &format!("/api/v1/admin/users/{self_revoker}/sessions/revoke"), &token, None).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1").bind(self_revoker).fetch_one(&pool).await.unwrap();
        assert_eq!(remaining, 0);
        let administrator = user(&pool, "EndpointSelfAdministrator", &[3]).await;
        user(&pool, "BackupAdministrator", &[3]).await;
        let (_, token) = session(&state, administrator).await;
        let (status, value) = request(&app, Method::PATCH, &format!("/api/v1/admin/users/{administrator}"), &token,
            Some(json!({"roles":[1]}))).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        let role_ids: Vec<i64> = sqlx::query_scalar("SELECT role_id FROM user_roles WHERE user_id=$1 ORDER BY role_id").bind(administrator).fetch_all(&pool).await.unwrap();
        assert_eq!(role_ids, vec![1]);
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    if root.exists() {
        assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
        tokio::fs::remove_dir_all(&root).await.unwrap();
    }
    outcome.unwrap();
}
