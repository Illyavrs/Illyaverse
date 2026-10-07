//! Security checks use a fresh random schema in an explicitly disposable PG17.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::COOKIE, cookie);
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
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn user(db: &PgPool, name: &str, roles: &[i64]) -> i64 {
    let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture','fixture') RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).fetch_one(db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,unnest($2::bigint[])")
        .bind(id)
        .bind(roles)
        .execute(db)
        .await
        .unwrap();
    id
}

async fn role(db: &PgPool, name: &str, permissions: &[&str]) -> i64 {
    sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES($1,$2) RETURNING id")
        .bind(name)
        .bind(permissions)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn session(state: &AppState, id: i64) -> (Uuid, String) {
    let sid = Uuid::new_v4();
    let expiry = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":id,"sid":sid,"exp":expiry.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (sid, format!("illya_session={token}"))
}

async fn wait_for_lock(db: &PgPool, application: &str, query_fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(application).bind(format!("%{query_fragment}%")).fetch_one(db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("RBAC mutation must reach the serialization lock before changing its authorization");
}

async fn audit_count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_RBAC_TEST_DATABASE_URL pointing to an owned disposable PostgreSQL 17"]
async fn scoped_rbac_privacy_deletion_revocation_and_queued_authorization() {
    let url =
        env::var("ILLYA_RBAC_TEST_DATABASE_URL").expect("Set the disposable RBAC fixture URL");
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
        "RBAC fixture must run PostgreSQL 17"
    );
    let schema = format!("illya_rbac_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())])
        .application_name(&schema);
    let db = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let pool = db.clone();
    let application = schema.clone();
    let outcome = tokio::spawn(async move {
        let state = AppState {
            db: pool.clone(),
            secret: Arc::new("rbac-fixture-session-secret-at-least-thirty-two-bytes".into()),
            internal_key: Arc::new(String::new()),
            origin: Arc::new("http://localhost:3000".into()),
            mail: Arc::new(mail_delivery::MailConfig::disabled()),
            secure_cookie: false,
            media_root: std::env::temp_dir(), maps_root: std::env::temp_dir(), replay_root: std::env::temp_dir(),
            client: reqwest::Client::new(),
            official: Arc::new(Mutex::new(maps::OfficialState::default())),
            limits: Default::default(), trusted_proxies: Default::default(),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        };
        let app = router(state.clone());
        let admin = user(&pool, "RbacAdmin", &[3]).await;
        let (_, admin_cookie) = session(&state, admin).await;
        let manager_role = role(&pool, "limited_manager", &["roles.manage", "users.sessions.revoke", "chat.send", "social.manage", "beatmaps.upload", "beatmaps.moderate", "scores.moderate"]).await;
        let manager = user(&pool, "RbacManager", &[manager_role]).await;
        let (_, manager_cookie) = session(&state, manager).await;
        let target = user(&pool, "RbacTarget", &[1]).await;
        let (_, target_cookie) = session(&state, target).await;
        let strong_role = role(&pool, "strong_operator", &["roles.manage", "users.moderate"]).await;
        let strong = user(&pool, "RbacStrong", &[strong_role]).await;
        let revoker_role = role(&pool, "session_revoker", &["users.sessions.revoke"]).await;
        let revoker = user(&pool, "RbacRevoker", &[revoker_role]).await;
        let (_, revoker_cookie) = session(&state, revoker).await;
        let moderator = user(&pool, "RbacModerator", &[2]).await;
        let (_, moderator_cookie) = session(&state, moderator).await;

        // Directory is paginated and does not expose email or credentials to role/revoke managers.
        for cookie in [&manager_cookie, &revoker_cookie] {
            let (code, directory) = request(&app, Method::GET, "/api/v1/admin/users?page_size=2&page=2", cookie, None).await;
            assert_eq!(code, StatusCode::OK, "{directory}");
            assert_eq!(directory["total"], 6);
            assert_eq!(directory["page"], 2);
            assert_eq!(directory["page_size"], 2);
            assert_eq!(directory["items"].as_array().unwrap().len(), 2);
            for item in directory["items"].as_array().unwrap() {
                assert!(item.get("email").is_none());
                assert!(item.get("password_hash").is_none());
                assert!(item.get("permissions").is_none());
                assert!(item["roles"].is_array());
            }
        }
        let (code, directory) = request(&app, Method::GET, "/api/v1/admin/users", &moderator_cookie, None).await;
        assert_eq!(code, StatusCode::OK);
        assert!(directory["items"][0]["email"].is_string());
        assert_eq!(request(&app, Method::GET, "/api/v1/admin/users", &target_cookie, None).await.0, StatusCode::FORBIDDEN);
        let (code, roles) = request(&app, Method::GET, "/api/v1/admin/roles", &manager_cookie, None).await;
        assert_eq!(code, StatusCode::OK);
        assert!(roles["items"][0]["is_builtin"].as_bool().unwrap());
        assert_eq!(roles["items"][0]["membership_count"], 1);

        for (search, expected) in [("rbactarget".to_string(), target), (strong.to_string(), strong)] {
            let (code, directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={search}"), &manager_cookie, None).await;
            assert_eq!(code, StatusCode::OK);
            assert_eq!(directory["total"], 1);
            assert_eq!(directory["items"][0]["id"], expected);
            assert_eq!(directory["items"][0]["can_manage_roles"], expected == target);
            assert_eq!(directory["items"][0]["can_revoke_sessions"], expected == target);
            assert_eq!(directory["items"][0]["can_moderate"], false);
        }
        assert_eq!(request(&app, Method::GET, "/api/v1/admin/users?search=%25", &manager_cookie, None).await.1["total"], 0, "search treats SQL wildcard characters literally");
        let (code, invalid) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={}", "x".repeat(65)), &manager_cookie, None).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(invalid["error"]["code"], "invalid_search");
        let (code, directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={}", "%E9%9B%AA".repeat(64)), &manager_cookie, None).await;
        assert_eq!(code, StatusCode::OK, "search length counts characters, not UTF-8 bytes");
        assert_eq!(directory["total"], 0);
        assert_eq!(request(&app, Method::GET, &format!("/api/v1/admin/users?search={}", "%E9%9B%AA".repeat(65)), &manager_cookie, None).await.0, StatusCode::BAD_REQUEST);
        let (_, own_directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={moderator}"), &moderator_cookie, None).await;
        assert_eq!(own_directory["items"][0]["can_moderate"], false);
        let (_, admin_directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={admin}"), &moderator_cookie, None).await;
        assert_eq!(admin_directory["items"][0]["can_moderate"], false);
        let (_, target_directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={target}"), &moderator_cookie, None).await;
        assert_eq!(target_directory["items"][0]["can_moderate"], true);

        // A limited manager may neither remove existing stronger grants nor add them.
        for (id, assigned) in [(strong, vec![]), (target, vec![strong_role]), (admin, vec![1])] {
            let before = audit_count(&pool).await;
            let (code, denied) = request(&app, Method::PATCH, &format!("/api/v1/admin/users/{id}"), &manager_cookie, Some(json!({"roles":assigned}))).await;
            assert_eq!(code, StatusCode::FORBIDDEN);
            assert_eq!(denied["error"]["code"], "stronger_role");
            assert_eq!(audit_count(&pool).await, before);
        }
        let (code, _) = request(&app, Method::PATCH, &format!("/api/v1/admin/roles/{strong_role}"), &manager_cookie, Some(json!({"name":"strong_operator","description":"","permissions":[]}))).await;
        assert_eq!(code, StatusCode::FORBIDDEN);
        for id in [admin, strong] {
            assert_eq!(request(&app, Method::POST, &format!("/api/v1/admin/users/{id}/sessions/revoke"), &manager_cookie, None).await.0, StatusCode::FORBIDDEN);
        }
        assert_eq!(request(&app, Method::POST, &format!("/api/v1/admin/users/{target}/sessions/revoke"), &moderator_cookie, None).await.0, StatusCode::FORBIDDEN);

        // A custom-role create/edit/assign/unassign/delete cycle is available to a scoped manager.
        let (code, created) = request(&app, Method::POST, "/api/v1/admin/roles", &manager_cookie, Some(json!({"name":"managed_role","description":"Managed fixture","permissions":["chat.send"]}))).await;
        assert_eq!(code, StatusCode::OK, "{created}");
        assert_eq!(created["membership_count"], 0);
        assert_eq!(created["is_builtin"], false);
        let managed = created["id"].as_i64().unwrap();
        let (code, edited) = request(&app, Method::PATCH, &format!("/api/v1/admin/roles/{managed}"), &manager_cookie, Some(json!({"name":"managed_role","description":"Updated fixture","permissions":["chat.send","social.manage"]}))).await;
        assert_eq!(code, StatusCode::OK, "{edited}");
        assert_eq!(edited["description"], "Updated fixture");
        assert_eq!(request(&app, Method::PATCH, &format!("/api/v1/admin/users/{target}"), &manager_cookie, Some(json!({"roles":[1,managed]}))).await.0, StatusCode::OK);
        let (_, assigned_roles) = request(&app, Method::GET, "/api/v1/admin/roles", &manager_cookie, None).await;
        assert_eq!(assigned_roles["items"].as_array().unwrap().iter().find(|r| r["id"] == managed).unwrap()["membership_count"], 1);
        assert_eq!(request(&app, Method::DELETE, &format!("/api/v1/admin/roles/{managed}"), &manager_cookie, None).await.0, StatusCode::CONFLICT);
        assert_eq!(request(&app, Method::PATCH, &format!("/api/v1/admin/users/{target}"), &manager_cookie, Some(json!({"roles":[1]}))).await.0, StatusCode::OK);
        assert_eq!(request(&app, Method::DELETE, &format!("/api/v1/admin/roles/{managed}"), &manager_cookie, None).await.0, StatusCode::OK);

        // Builtin roles cannot be deleted or renamed. Assigned custom roles require explicit unassignment.
        for id in 1..=3 {
            let (code, denied) = request(&app, Method::DELETE, &format!("/api/v1/admin/roles/{id}"), &admin_cookie, None).await;
            assert_eq!(code, StatusCode::BAD_REQUEST);
            assert_eq!(denied["error"]["code"], "builtin_role");
            assert_eq!(request(&app, Method::PATCH, &format!("/api/v1/admin/roles/{id}"), &admin_cookie, Some(json!({"name":format!("renamed_{id}"),"description":"","permissions":["*"]}))).await.0, StatusCode::BAD_REQUEST);
        }
        let (code, value) = request(&app, Method::DELETE, &format!("/api/v1/admin/roles/{manager_role}"), &admin_cookie, None).await;
        assert_eq!(code, StatusCode::CONFLICT);
        assert_eq!(value["error"]["code"], "role_assigned");
        assert!(value["error"]["message"].as_str().unwrap().contains("Unassign"));
        let unused = role(&pool, "unused_role", &["chat.send"]).await;
        let before = audit_count(&pool).await;
        assert_eq!(request(&app, Method::DELETE, &format!("/api/v1/admin/roles/{unused}"), &manager_cookie, None).await.0, StatusCode::OK);
        assert_eq!(audit_count(&pool).await, before + 1);
        let deletion: Value = sqlx::query_scalar("SELECT details FROM audit_events WHERE action='roles.delete' AND target_id=$1").bind(unused.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(deletion["name"], "unused_role");

        // Last unrestricted wildcard holder cannot remove their own final grant.
        let (code, denied) = request(&app, Method::PATCH, &format!("/api/v1/admin/users/{admin}"), &admin_cookie, Some(json!({"roles":[1]}))).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert_eq!(denied["error"]["code"], "last_administrator");
        let wildcard = role(&pool, "custom_wildcard", &["*"]).await;
        sqlx::query("UPDATE user_roles SET role_id=$2 WHERE user_id=$1").bind(admin).bind(wildcard).execute(&pool).await.unwrap();
        assert_eq!(request(&app, Method::PATCH, &format!("/api/v1/admin/roles/{wildcard}"), &admin_cookie, Some(json!({"name":"custom_wildcard","description":"","permissions":[]}))).await.0, StatusCode::BAD_REQUEST);
        sqlx::query("UPDATE user_roles SET role_id=3 WHERE user_id=$1").bind(admin).execute(&pool).await.unwrap();

        // Independent session revocation removes all web, stable and lazer auth records without restricting the account.
        session(&state, target).await;
        let stable_token = Uuid::new_v4().to_string();
        let access_token = Uuid::new_v4().to_string();
        let refresh_token = Uuid::new_v4().to_string();
        let stable_hash = format!("{:x}", Sha256::digest(stable_token.as_bytes()));
        let access_hash = format!("{:x}", Sha256::digest(access_token.as_bytes()));
        let refresh_hash = format!("{:x}", Sha256::digest(refresh_token.as_bytes()));
        sqlx::query("INSERT INTO bancho_sessions(id,user_id,token_hash,expires_at) VALUES($1,$2,$3,now()+interval '1 hour')")
            .bind(Uuid::new_v4()).bind(target).bind(&stable_hash).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')")
            .bind(Uuid::new_v4()).bind(target).bind(&access_hash).bind(&refresh_hash).execute(&pool).await.unwrap();
        let valid_before: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bancho_sessions WHERE token_hash=$1 AND expires_at>now()) AND EXISTS(SELECT 1 FROM lazer_sessions WHERE access_hash=$2 AND access_expires_at>now() AND refresh_hash=$3 AND refresh_expires_at>now())")
            .bind(&stable_hash).bind(&access_hash).bind(&refresh_hash).fetch_one(&pool).await.unwrap();
        assert!(valid_before, "stable and lazer token hashes must identify live sessions before revocation");
        let before = audit_count(&pool).await;
        let (code, value) = request(&app, Method::POST, &format!("/api/v1/admin/users/{target}/sessions/revoke"), &manager_cookie, None).await;
        assert_eq!(code, StatusCode::OK, "{value}");
        assert_eq!(value["revoked"], json!({"web":2,"stable":1,"lazer":1}));
        assert_eq!(audit_count(&pool).await, before + 1);
        assert_eq!(request(&app, Method::GET, "/api/v1/me", &target_cookie, None).await.0, StatusCode::UNAUTHORIZED);
        let restricted: bool = sqlx::query_scalar("SELECT is_restricted FROM users WHERE id=$1").bind(target).fetch_one(&pool).await.unwrap();
        assert!(!restricted);
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bancho_sessions WHERE token_hash=$1) OR EXISTS(SELECT 1 FROM lazer_sessions WHERE access_hash=$2 OR refresh_hash=$3)")
            .bind(stable_hash).bind(access_hash).bind(refresh_hash).fetch_one(&pool).await.unwrap();
        assert!(!valid, "neither stable nor lazer bearer/refresh token remains valid");
        let (_, replacement_cookie) = session(&state, target).await;
        assert_eq!(request(&app, Method::GET, "/api/v1/me", &replacement_cookie, None).await.0, StatusCode::OK);
        let empty_target = user(&pool, "UnprivilegedTarget", &[]).await;
        let (_, empty_cookie) = session(&state, empty_target).await;
        let (_, directory) = request(&app, Method::GET, &format!("/api/v1/admin/users?search={empty_target}"), &revoker_cookie, None).await;
        assert_eq!(directory["items"][0]["can_revoke_sessions"], true);
        assert_eq!(directory["items"][0]["can_manage_roles"], false);
        assert_eq!(request(&app, Method::POST, &format!("/api/v1/admin/users/{empty_target}/sessions/revoke"), &revoker_cookie, None).await.0, StatusCode::OK);
        assert_eq!(request(&app, Method::GET, "/api/v1/me", &empty_cookie, None).await.0, StatusCode::UNAUTHORIZED);

        // RBAC, moderation and bounded RX recalculation must revalidate authorization
        // after waiting for serialization, before looking up or changing target data.
        let queued_role = role(&pool, "queued_role", &["chat.send"]).await;
        let mutations = [
            (Method::POST, "/api/v1/admin/roles".to_string(), Some(json!({"name":"queued_create","description":"","permissions":["chat.send"]}))),
            (Method::PATCH, format!("/api/v1/admin/roles/{queued_role}"), Some(json!({"name":"queued_role","description":"changed","permissions":["chat.send"]}))),
            (Method::DELETE, format!("/api/v1/admin/roles/{queued_role}"), None),
            (Method::PATCH, format!("/api/v1/admin/users/{target}"), Some(json!({"roles":[]}))),
            (Method::POST, format!("/api/v1/admin/users/{target}/sessions/revoke"), None),
            (Method::PATCH, "/api/v1/admin/beatmaps/1".to_string(), Some(json!({"status":"ranked"}))),
            (Method::PATCH, "/api/v1/admin/scores/1".to_string(), Some(json!({"verification":"verified"}))),
            (Method::POST, "/api/v1/admin/scores/recalculate-relax".to_string(), Some(json!({"score_ids":[1]}))),
        ];
        for (method, path, body) in mutations {
            for change in ["permission", "session", "restriction"] {
                sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(manager).bind(manager_role).execute(&pool).await.unwrap();
                sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(manager).execute(&pool).await.unwrap();
                let (sid, cookie) = session(&state, manager).await;
                let before = audit_count(&pool).await;
                let mut blocking = pool.begin().await.unwrap();
                sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocking).await.unwrap();
                let queued_app = app.clone();
                let queued_method = method.clone();
                let queued_path = path.clone();
                let queued_body = body.clone();
                let queued = tokio::spawn(async move { request(&queued_app, queued_method, &queued_path, &cookie, queued_body).await });
                wait_for_lock(&pool, &application, "LOCK TABLE roles,user_roles").await;
                match change {
                    "permission" => { sqlx::query("DELETE FROM user_roles WHERE user_id=$1").bind(manager).execute(&mut *blocking).await.unwrap(); }
                    "session" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap(); }
                    "restriction" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(manager).execute(&mut *blocking).await.unwrap(); }
                    _ => unreachable!(),
                }
                blocking.commit().await.unwrap();
                let (code, value) = tokio::time::timeout(Duration::from_secs(5), queued).await.unwrap().unwrap();
                assert_eq!(code, if change == "permission" { StatusCode::FORBIDDEN } else { StatusCode::UNAUTHORIZED }, "{method} {path} after {change}: {value}");
                assert_eq!(audit_count(&pool).await, before, "denied queued mutation must not audit or mutate");
                let unchanged: String = sqlx::query_scalar("SELECT description FROM roles WHERE id=$1").bind(queued_role).fetch_one(&pool).await.unwrap();
                assert_eq!(unchanged, "");
            }
        }

        // Recovery holding the actor user row can update credentials and revoke the session while RBAC waits, without a table-lock cycle.
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(manager).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(manager).bind(manager_role).execute(&pool).await.unwrap();
        let (_, cookie) = session(&state, manager).await;
        let mut recovery = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(manager).fetch_one(&mut *recovery).await.unwrap();
        let queued_app = app.clone();
        let queued = tokio::spawn(async move { request(&queued_app, Method::DELETE, &format!("/api/v1/admin/roles/{queued_role}"), &cookie, None).await });
        wait_for_lock(&pool, &application, "ORDER BY id FOR UPDATE").await;
        tokio::time::timeout(Duration::from_secs(5), async {
            sqlx::query("UPDATE users SET password_hash='replacement' WHERE id=$1").bind(manager).execute(&mut *recovery).await.unwrap();
            auth::revoke_sessions(&mut recovery, manager, None).await.unwrap();
            recovery.commit().await.unwrap();
        }).await.expect("recovery and RBAC must use a compatible lock order");
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), queued).await.unwrap().unwrap().0, StatusCode::UNAUTHORIZED);
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
