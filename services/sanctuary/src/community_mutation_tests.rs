//! Queued website writes use real PG17 locks and an independently owned schema.
use crate::{AppState, mail_delivery, maps, router};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(app: Router, path: String, cookie: String, body: Value) -> (StatusCode, Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn actor(state: &AppState, label: &str) -> (i64, i64, Uuid, String) {
    let role: i64 = sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES($1,ARRAY['chat.send','social.manage']) RETURNING id")
        .bind(format!("role_{label}")).fetch_one(&state.db).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'synthetic','synthetic') RETURNING id")
        .bind(label).bind(format!("{label}@example.test")).fetch_one(&state.db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
        .bind(id)
        .bind(role)
        .execute(&state.db)
        .await
        .unwrap();
    let sid = Uuid::new_v4();
    let expires = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expires)
        .execute(&state.db)
        .await
        .unwrap();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":id,"sid":sid,"exp":expires.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (id, role, sid, format!("illya_session={token}"))
}

async fn waiting(db: &PgPool, application: &str, fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(application).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if found { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("request must reach its known resource/authorization lock");
}

async fn expire_after_guard(db: &PgPool, sid: Uuid) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar(
                "SELECT expires_at<=clock_timestamp() FROM sessions WHERE id=$1",
            )
            .bind(sid)
            .fetch_one(db)
            .await
            .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture session must naturally expire while waiting");
}

async fn counts(db: &PgPool) -> (i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM chat_messages),(SELECT count(*) FROM user_relationships),(SELECT count(*) FROM bancho_packet_queue)")
        .fetch_one(db).await.unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_AUTH_MUTATION_TEST_DATABASE_URL pointing to owned disposable PostgreSQL 17"]
async fn community_mutations_recheck_authority_policy_and_expiry() {
    let url = std::env::var("ILLYA_AUTH_MUTATION_TEST_DATABASE_URL")
        .expect("Set disposable community fixture URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&control)
        .await
        .unwrap();
    assert!((170000..180000).contains(&version));
    let schema = format!("illya_community_{}", Uuid::new_v4().simple());
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
        let state = AppState {
            db: pool.clone(), secret: Arc::new("community-fixture-secret-at-least-thirty-two-bytes".into()),
            internal_key: Arc::new(String::new()), origin: Arc::new("http://localhost:3000".into()),
            mail: Arc::new(mail_delivery::MailConfig::disabled()), secure_cookie: false,
            media_root: std::env::temp_dir(), maps_root: std::env::temp_dir(), replay_root: std::env::temp_dir(),
            client: reqwest::Client::new(), official: Arc::new(Mutex::new(maps::OfficialState::default())),
            limits: Default::default(), trusted_proxies: Default::default(), upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        };
        let app = router(state.clone());
        let channel: i64 = sqlx::query_scalar("INSERT INTO channels(name) VALUES('#community-fixture') RETURNING id").fetch_one(&pool).await.unwrap();
        let (peer, _, _, peer_cookie) = actor(&state, "CommunityPeer").await;
        sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at) VALUES($1,$2,$3,clock_timestamp()+interval '1 hour')")
            .bind(Uuid::new_v4()).bind(Uuid::new_v4().to_string()).bind(peer).execute(&pool).await.unwrap();

        // Both endpoints must discard their initial permission/session snapshot.
        for social in [false, true] {
            for failure in ["revoked", "restricted", "permission"] {
                let (id, role, sid, cookie) = actor(&state, &format!("C_{}_{}", social, failure)).await;
                let before = counts(&pool).await;
                let mut blocker = pool.begin().await.unwrap();
                let fragment = if failure == "permission" {
                    sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
                    "LOCK TABLE roles,user_roles IN SHARE MODE"
                } else {
                    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *blocker).await.unwrap();
                    "SELECT id FROM users WHERE id=ANY"
                };
                let path = if social { format!("/api/v1/me/relationships/{peer}") } else { format!("/api/v1/channels/{channel}/messages") };
                let body = if social { json!({"relation_type":"friend"}) } else { json!({"content":"queued synthetic message"}) };
                let work = tokio::spawn(request(app.clone(), path, cookie, body));
                waiting(&pool, &application, fragment).await;
                match failure {
                    "revoked" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    "restricted" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(id).execute(&mut *blocker).await.unwrap(); }
                    _ => { sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&mut *blocker).await.unwrap(); }
                }
                blocker.commit().await.unwrap();
                let result = tokio::time::timeout(Duration::from_secs(5), work).await.unwrap().unwrap();
                assert_eq!(result.0, if failure == "permission" { StatusCode::FORBIDDEN } else { StatusCode::UNAUTHORIZED });
                assert_eq!(counts(&pool).await, before, "denial must not publish messages, relationships or native packets");
            }
        }

        // Channel policy changes are checked after the channel lock is acquired.
        for policy in ["staff.read", "chat.pm", "deleted"] {
            let (_, _, _, cookie) = actor(&state, &format!("Policy_{}", policy.replace('.', "_"))).await;
            let policy_channel: i64 = sqlx::query_scalar("INSERT INTO channels(name) VALUES($1) RETURNING id").bind(format!("#{policy}")).fetch_one(&pool).await.unwrap();
            let before = counts(&pool).await;
            let mut blocker = pool.begin().await.unwrap();
            sqlx::query("SELECT id FROM channels WHERE id=$1 FOR UPDATE").bind(policy_channel).fetch_one(&mut *blocker).await.unwrap();
            let work = tokio::spawn(request(app.clone(), format!("/api/v1/channels/{policy_channel}/messages"), cookie, json!({"content":"policy fixture"})));
            waiting(&pool, &application, "SELECT required_permission FROM channels").await;
            if policy == "deleted" {
                sqlx::query("DELETE FROM channels WHERE id=$1").bind(policy_channel).execute(&mut *blocker).await.unwrap();
            } else {
                sqlx::query("UPDATE channels SET required_permission=$2 WHERE id=$1").bind(policy_channel).bind(policy).execute(&mut *blocker).await.unwrap();
            }
            blocker.commit().await.unwrap();
            assert_eq!(work.await.unwrap().0, if policy == "staff.read" { StatusCode::FORBIDDEN } else { StatusCode::NOT_FOUND });
            assert_eq!(counts(&pool).await, before);
        }

        // Natural expiry after the initial guard rolls back publication and packets.
        for social in [false, true] {
            let (id, _, sid, cookie) = actor(&state, &format!("Expiry_{social}")).await;
            sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1").bind(sid).execute(&pool).await.unwrap();
            let mut blocker = pool.begin().await.unwrap();
            let fragment = if social {
                sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend')").bind(id).bind(peer).execute(&pool).await.unwrap();
                sqlx::query("SELECT user_id FROM user_relationships WHERE user_id=$1 AND target_id=$2 FOR UPDATE").bind(id).bind(peer).fetch_one(&mut *blocker).await.unwrap();
                "DELETE FROM user_relationships"
            } else {
                sqlx::query("SELECT id FROM channels WHERE id=$1 FOR UPDATE").bind(channel).fetch_one(&mut *blocker).await.unwrap();
                "SELECT required_permission FROM channels"
            };
            let before = counts(&pool).await;
            let path = if social { format!("/api/v1/me/relationships/{peer}") } else { format!("/api/v1/channels/{channel}/messages") };
            let body = if social { json!({"relation_type":"block"}) } else { json!({"content":"expired fixture"}) };
            let work = tokio::spawn(request(app.clone(), path, cookie, body));
            waiting(&pool, &application, fragment).await;
            expire_after_guard(&pool, sid).await;
            blocker.commit().await.unwrap();
            assert_eq!(work.await.unwrap().0, StatusCode::UNAUTHORIZED);
            assert_eq!(counts(&pool).await, before);
            if social {
                let relation: String = sqlx::query_scalar("SELECT relation_type FROM user_relationships WHERE user_id=$1 AND target_id=$2").bind(id).bind(peer).fetch_one(&pool).await.unwrap();
                assert_eq!(relation, "friend", "expired update must preserve the old relationship");
            }
        }

        let (valid, _, _, cookie) = actor(&state, "CommunityValid").await;
        let message = request(app.clone(), format!("/api/v1/channels/{channel}/messages"), cookie.clone(), json!({"content":"  hello  "})).await;
        assert_eq!(message.0, StatusCode::OK);
        assert_eq!(message.1["user_id"], valid);
        assert_eq!(message.1["content"], "hello");
        let before = counts(&pool).await;
        let forward = request(app.clone(), format!("/api/v1/me/relationships/{peer}"), cookie, json!({"relation_type":"friend"}));
        let reverse = request(app.clone(), format!("/api/v1/me/relationships/{valid}"), peer_cookie, json!({"relation_type":"friend"}));
        let pair = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(forward, reverse) }).await.expect("opposing relationships must retain sorted lock order");
        assert_eq!(pair.0.0, StatusCode::OK);
        assert_eq!(pair.1.0, StatusCode::OK);
        let after = counts(&pool).await;
        assert_eq!(after.1, before.1 + 2);
        assert_eq!(after.2, before.2 + 2);
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
