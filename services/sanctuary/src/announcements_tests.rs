//! Router and transaction checks use the existing owned disposable PG17 fixture.
use crate::{
    Router,
    maps::map_mutation_tests::{MutationFixture, actor, reject_audits_at_commit, wait_for_lock},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN: &str = "/api/v1/admin/announcements";
const PUBLIC: &str = "/api/v1/announcements";

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    raw_request(
        app,
        method,
        path,
        cookie,
        if method == "GET" {
            Vec::new()
        } else {
            body.to_string().into_bytes()
        },
    )
    .await
}

async fn raw_request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(app: &Router, cookie: &str, title: &str) -> Value {
    let (status, item) = request(
        app,
        "POST",
        ADMIN,
        Some(cookie),
        json!({"title":title,"body":"  First\r\nSecond\tline  "}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{item}");
    assert_eq!(item["status"], "draft");
    assert_eq!(item["revision"], 1);
    assert!(item["published_at"].is_null());
    assert_eq!(item["body"], "First\nSecond\tline");
    item
}

fn patch_body(item: &Value, status: &str, title: &str) -> Value {
    json!({"revision":item["revision"],"title":title,"body":"Plain <b>text</b>","status":status})
}

async fn change(app: &Router, cookie: &str, item: &Value, status: &str, title: &str) -> Value {
    let (code, changed) = request(
        app,
        "PATCH",
        &format!("{ADMIN}/{}", item["id"]),
        Some(cookie),
        patch_body(item, status, title),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{changed}");
    assert_eq!(changed["status"], status);
    assert_eq!(
        changed["revision"].as_i64(),
        item["revision"].as_i64().map(|r| r + 1)
    );
    changed
}

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> Value {
    let (status, value) = request(app, "GET", path, cookie, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{path}: {value}");
    value
}

async fn snapshot(db: &PgPool) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('announcements',COALESCE((SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM announcements a),'[]'::jsonb),'audits',COALESCE((SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM audit_events a WHERE action LIKE 'announcements.%'),'[]'::jsonb))")
        .fetch_one(db).await.unwrap()
}

async fn expire(db: &PgPool, sid: Uuid) {
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
    .expect("fixture session must expire while the announcement write is queued");
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn announcement_http_permissions_lifecycle_and_pagination() {
    let fixture = MutationFixture::new("announcement_api", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let outcome = tokio::spawn(async move {
        let (creator, _, _, manager) = actor(&state, "AnnouncementManager", &["announcements.manage"]).await;
        let (editor, _, _, editor_cookie) = actor(&state, "AnnouncementEditor", &["announcements.manage"]).await;
        let (_, _, _, wildcard) = actor(&state, "AnnouncementWildcard", &["*"]).await;
        for permissions in [&["admin.view"][..], &["roles.manage"][..], &["users.moderate", "audit.read"][..]] {
            let (_, _, _, limited) = actor(&state, &format!("Limited{}", permissions[0].replace('.', "")), permissions).await;
            for (method, path, body) in [
                ("GET", ADMIN.to_string(), Value::Null),
                ("GET", format!("{ADMIN}/999999"), Value::Null),
                ("POST", ADMIN.to_string(), json!({"title":"Denied","body":"Denied"})),
                ("PATCH", format!("{ADMIN}/999999"), json!({"revision":1,"title":"Denied","body":"Denied","status":"published"})),
            ] {
                assert_eq!(request(&app, method, &path, Some(&limited), body.clone()).await.0, StatusCode::FORBIDDEN);
                assert_eq!(request(&app, method, &path, None, body).await.0, StatusCode::UNAUTHORIZED);
            }
        }
        assert_eq!(snapshot(&state.db).await["announcements"], json!([]));
        assert_eq!(snapshot(&state.db).await["audits"], json!([]));
        let draft = create(&app, &manager, "  Original  ").await;
        assert_eq!(draft["title"], "Original");
        assert_eq!(draft["author"], json!({"id":creator,"username":"AnnouncementManager","avatar_url":null}));
        assert_eq!(draft.as_object().unwrap().len(), 9);
        assert_eq!(snapshot(&state.db).await["audits"][0]["details"], json!({"id":draft["id"],"revision":1,"status":"draft"}));
        assert_eq!(get(&app, &format!("{ADMIN}/{}", draft["id"]), Some(&manager)).await, draft);
        for cookie in [None, Some(manager.as_str()), Some(wildcard.as_str())] {
            assert_eq!(request(&app, "GET", &format!("{PUBLIC}/{}", draft["id"]), cookie, Value::Null).await.0, StatusCode::NOT_FOUND);
            assert_eq!(get(&app, PUBLIC, cookie).await["items"], json!([]));
        }
        let published = change(&app, &manager, &draft, "published", "Published").await;
        assert!(published["published_at"].is_string());
        assert_eq!(get(&app, &format!("{PUBLIC}/{}", draft["id"]), None).await, published);
        let edited = change(&app, &editor_cookie, &published, "published", "Edited").await;
        assert_eq!(edited["published_at"], published["published_at"]);
        assert_eq!(edited["author"], draft["author"]);
        let actors: (i64, i64) = sqlx::query_as("SELECT created_by,updated_by FROM announcements WHERE id=$1").bind(draft["id"].as_i64().unwrap()).fetch_one(&state.db).await.unwrap();
        assert_eq!(actors, (creator, editor));
        let archived = change(&app, &manager, &edited, "archived", "Withdrawn").await;
        assert_eq!(archived["published_at"], published["published_at"]);
        assert_eq!(get(&app, &format!("{ADMIN}/{}", draft["id"]), Some(&wildcard)).await, archived);
        for cookie in [None, Some(manager.as_str()), Some(wildcard.as_str())] {
            assert_eq!(request(&app, "GET", &format!("{PUBLIC}/{}", draft["id"]), cookie, Value::Null).await.0, StatusCode::NOT_FOUND);
            assert_eq!(get(&app, PUBLIC, cookie).await["items"], json!([]));
        }
        let returned = change(&app, &manager, &archived, "draft", "Revised draft").await;
        assert_eq!(returned["published_at"], published["published_at"]);
        let restored = change(&app, &manager, &returned, "published", "Restored").await;
        assert!(restored["published_at"].as_str().unwrap() > published["published_at"].as_str().unwrap());
        let lifecycle = snapshot(&state.db).await;
        assert_eq!(lifecycle["audits"].as_array().unwrap().len(), 6);
        assert_eq!(lifecycle["audits"][2]["actor_id"], editor);
        assert_eq!(lifecycle["audits"][5]["details"], json!({"id":draft["id"],"revision":6,"status":"published","previous_revision":5,"previous_status":"draft"}));

        let draft_two = create(&app, &manager, "Second draft").await;
        let archive_two = create(&app, &manager, "Second archive").await;
        change(&app, &manager, &archive_two, "archived", "Second archive").await;
        let fourth = create(&app, &manager, "Fourth").await;
        change(&app, &manager, &fourth, "published", "Fourth").await;
        let fifth = create(&app, &wildcard, "Fifth").await;
        change(&app, &wildcard, &fifth, "published", "Fifth").await;
        sqlx::query("UPDATE announcements SET updated_at='2020-01-01',published_at=CASE WHEN status='published' AND id<>$1 THEN '2020-02-01'::timestamptz ELSE published_at END")
            .bind(draft["id"].as_i64().unwrap()).execute(&state.db).await.unwrap();
        sqlx::query("UPDATE announcements SET published_at='2020-01-01' WHERE id=$1").bind(draft["id"].as_i64().unwrap()).execute(&state.db).await.unwrap();
        let first_page = get(&app, &format!("{PUBLIC}?page=1&page_size=2"), None).await;
        assert_eq!(first_page["total"], 3);
        assert_eq!(first_page["page"], 1);
        assert_eq!(first_page["page_size"], 2);
        assert_eq!(first_page["items"][0]["id"], fifth["id"]);
        assert_eq!(first_page["items"][1]["id"], fourth["id"]);
        assert_eq!(get(&app, &format!("{PUBLIC}?page=2&page_size=2"), None).await["items"][0]["id"], draft["id"]);
        assert_eq!(get(&app, &format!("{PUBLIC}?page=999&page_size=2"), None).await["items"], json!([]));
        let clamped = get(&app, &format!("{PUBLIC}?page=0&page_size=0"), None).await;
        assert_eq!((clamped["page"].as_i64(), clamped["page_size"].as_i64()), (Some(1), Some(1)));
        assert_eq!(get(&app, &format!("{PUBLIC}?page_size=999"), None).await["page_size"], 100);
        let all = get(&app, &format!("{ADMIN}?page_size=2"), Some(&manager)).await;
        assert_eq!(all["total"], 5);
        assert_eq!(all["items"][0]["id"], fifth["id"]);
        assert_eq!(all["items"][1]["id"], fourth["id"]);
        let filtered = get(&app, &format!("{ADMIN}?status=draft"), Some(&manager)).await;
        assert_eq!(filtered["total"], 1);
        assert_eq!(filtered["items"][0]["id"], draft_two["id"]);
        assert_eq!(get(&app, &format!("{ADMIN}?status=archived"), Some(&manager)).await["items"][0]["id"], archive_two["id"]);
        assert_eq!(request(&app, "GET", &format!("{ADMIN}?status=private"), Some(&manager), Value::Null).await.0, StatusCode::BAD_REQUEST);
        assert_eq!(request(&app, "GET", &format!("{PUBLIC}/999999"), None, Value::Null).await.0, StatusCode::NOT_FOUND);
        assert_eq!(request(&app, "GET", &format!("{ADMIN}/999999"), Some(&manager), Value::Null).await.0, StatusCode::NOT_FOUND);

        let before = snapshot(&state.db).await;
        for body in [json!({"title":"","body":"body"}), json!({"title":"title\n","body":"body"}), json!({"title":"title","body":"a\rb"}), json!({"title":"title","body":"😀".repeat(16385)})] {
            assert_eq!(request(&app, "POST", ADMIN, Some(&manager), body).await.0, StatusCode::BAD_REQUEST);
        }
        assert_eq!(request(&app, "POST", ADMIN, Some(&manager), json!({"title":"title","body":"body","status":"published"})).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(request(&app, "PATCH", &format!("{ADMIN}/{}", draft["id"]), Some(&manager), json!({"revision":6,"title":"title","body":"body"})).await.0, StatusCode::UNPROCESSABLE_ENTITY);
        for body in [
            json!({"revision":0,"title":"title","body":"body","status":"draft"}),
            json!({"revision":6,"title":"title","body":"body","status":"private"}),
            json!({"revision":6,"title":"title","body":"","status":"published"}),
        ] {
            assert_eq!(request(&app, "PATCH", &format!("{ADMIN}/{}", draft["id"]), Some(&manager), body).await.0, StatusCode::BAD_REQUEST);
        }
        assert_eq!(request(&app, "DELETE", &format!("{ADMIN}/{}", draft["id"]), Some(&manager), Value::Null).await.0, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(snapshot(&state.db).await, before, "invalid writes must not change content or audit");
        let leaked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE action LIKE 'announcements.%' AND (details ? 'body' OR details ? 'title'))").fetch_one(&state.db).await.unwrap();
        assert!(!leaked, "audit metadata must exclude announcement content");
        // Valid JSON escapes can make the request much larger than the decoded
        // canonical body. This exercises the HTTP layer rather than just parsing.
        let escaped = format!(r#"{{"title":"Escaped CJK","body":"{}"}}"#, r"\u754c".repeat(20000));
        assert!(escaped.len() > 96 * 1024);
        let (status, escaped_item) = raw_request(&app, "POST", ADMIN, Some(&manager), escaped.into_bytes()).await;
        assert_eq!(status, StatusCode::CREATED, "{escaped_item}");
        assert_eq!(escaped_item["body"], "界".repeat(20000));
        let escaped = format!(r#"{{"title":"Escaped emoji","body":"{}"}}"#, r"\ud83d\ude00".repeat(16384));
        assert!(escaped.len() > 192 * 1024);
        let (status, escaped_item) = raw_request(&app, "POST", ADMIN, Some(&manager), escaped.into_bytes()).await;
        assert_eq!(status, StatusCode::CREATED, "{escaped_item}");
        assert_eq!(escaped_item["body"], "😀".repeat(16384));
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn announcement_mutations_recheck_current_authority() {
    let fixture = MutationFixture::new("announcement_auth", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let outcome = tokio::spawn(async move {
        let (_, _, _, owner) = actor(&state, "AnnouncementAuthOwner", &["announcements.manage"]).await;
        let item = create(&app, &owner, "Preserved").await;
        for update in [false, true] {
            for failure in ["permission", "revoked", "restricted", "expired"] {
                let (id, role, sid, cookie) = actor(&state, &format!("Announcement{update}{failure}"), &["announcements.manage"]).await;
                let before = snapshot(&state.db).await;
                if failure == "expired" {
                    sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1").bind(sid).execute(&state.db).await.unwrap();
                }
                let mut blocker = state.db.begin().await.unwrap();
                let fragment = if failure == "permission" {
                    sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
                    "LOCK TABLE roles,user_roles IN SHARE MODE"
                } else {
                    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *blocker).await.unwrap();
                    "SELECT id FROM users WHERE id=ANY"
                };
                let path = if update { format!("{ADMIN}/{}", item["id"]) } else { ADMIN.to_string() };
                let body = if update { patch_body(&item, "published", "Must not persist") } else { json!({"title":"Must not persist","body":"Denied"}) };
                let queued_app = app.clone();
                let queued = tokio::spawn(async move { request(&queued_app, if update { "PATCH" } else { "POST" }, &path, Some(&cookie), body).await });
                wait_for_lock(&state.db, &application, fragment).await;
                match failure {
                    "permission" => { sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&mut *blocker).await.unwrap(); }
                    "revoked" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    "restricted" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(id).execute(&mut *blocker).await.unwrap(); }
                    _ => { expire(&state.db, sid).await; }
                }
                blocker.commit().await.unwrap();
                let (status, _) = tokio::time::timeout(Duration::from_secs(5), queued).await.unwrap().unwrap();
                assert_eq!(status, if failure == "permission" { StatusCode::FORBIDDEN } else { StatusCode::UNAUTHORIZED });
                assert_eq!(snapshot(&state.db).await, before, "{update}/{failure} must not publish content or audit");
            }
        }
    }).await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn announcement_mutations_rollback_after_session_deadlines() {
    let fixture = MutationFixture::new("announcement_deadline", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let outcome = tokio::spawn(async move {
        let (_, _, _, owner) = actor(
            &state,
            "AnnouncementDeadlineOwner",
            &["announcements.manage"],
        )
        .await;
        let item = create(&app, &owner, "Preserved deadline draft").await;
        // The row wait proves authorization precedes the resource lock. The audit
        // wait proves a final check rolls back content already written in the tx.
        for wait in ["resource", "create_audit", "update_audit"] {
            let (_, _, sid, cookie) = actor(
                &state,
                &format!("AnnouncementDeadline{wait}"),
                &["announcements.manage"],
            )
            .await;
            let before = snapshot(&state.db).await;
            sqlx::query(
                "UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1",
            )
            .bind(sid)
            .execute(&state.db)
            .await
            .unwrap();
            let mut blocker = state.db.begin().await.unwrap();
            let fragment = if wait == "resource" {
                sqlx::query("SELECT id FROM announcements WHERE id=$1 FOR UPDATE")
                    .bind(item["id"].as_i64().unwrap())
                    .fetch_one(&mut *blocker)
                    .await
                    .unwrap();
                "SELECT revision,status FROM announcements"
            } else {
                sqlx::query("LOCK TABLE audit_events IN ACCESS EXCLUSIVE MODE")
                    .execute(&mut *blocker)
                    .await
                    .unwrap();
                "INSERT INTO audit_events"
            };
            let path = if wait == "create_audit" {
                ADMIN.to_string()
            } else {
                format!("{ADMIN}/{}", item["id"])
            };
            let body = if wait == "create_audit" {
                json!({"title":"Must roll back","body":"Expired draft"})
            } else {
                patch_body(&item, "published", "Must roll back")
            };
            let queued_app = app.clone();
            let queued = tokio::spawn(async move {
                request(
                    &queued_app,
                    if wait == "create_audit" {
                        "POST"
                    } else {
                        "PATCH"
                    },
                    &path,
                    Some(&cookie),
                    body,
                )
                .await
            });
            wait_for_lock(&state.db, &application, fragment).await;
            expire(&state.db, sid).await;
            blocker.commit().await.unwrap();
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), queued)
                    .await
                    .unwrap()
                    .unwrap()
                    .0,
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                snapshot(&state.db).await,
                before,
                "{wait} must preserve content and audit together"
            );
        }
    })
    .await;
    fixture.finish(outcome).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn announcement_revision_conflicts_and_audit_failures_are_atomic() {
    let fixture = MutationFixture::new("announcement_atomic", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let outcome = tokio::spawn(async move {
        let (_, _, _, cookie_one) = actor(&state, "AnnouncementRaceOne", &["announcements.manage"]).await;
        let (_, _, _, cookie_two) = actor(&state, "AnnouncementRaceTwo", &["announcements.manage"]).await;
        let item = create(&app, &cookie_one, "Race draft").await;
        let before = snapshot(&state.db).await;
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("SELECT id FROM announcements WHERE id=$1 FOR UPDATE").bind(item["id"].as_i64().unwrap()).fetch_one(&mut *blocker).await.unwrap();
        let path = format!("{ADMIN}/{}", item["id"]);
        let queued_app = app.clone();
        let queued_path = path.clone();
        let body = patch_body(&item, "published", "Winner or loser");
        let queued_body = body.clone();
        let queued_cookie = cookie_one.clone();
        let queued = tokio::spawn(async move { request(&queued_app, "PATCH", &queued_path, Some(&queued_cookie), queued_body).await });
        wait_for_lock(&state.db, &application, "SELECT revision,status FROM announcements").await;
        let app_two = app.clone();
        let path_two = path.clone();
        let cookie = cookie_two.clone();
        let second = tokio::spawn(async move { request(&app_two, "PATCH", &path_two, Some(&cookie), body).await });
        // Different actors can reach the same resource lock concurrently.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%SELECT revision,status FROM announcements%'")
                    .bind(&application).fetch_one(&state.db).await.unwrap();
                if count >= 2 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("both editors must wait for the announcement row");
        blocker.commit().await.unwrap();
        let (one, two) = tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(queued, second) }).await.unwrap();
        let one = one.unwrap();
        let two = two.unwrap();
        assert!(matches!((one.0, two.0), (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)));
        let conflict = if one.0 == StatusCode::CONFLICT { &one.1 } else { &two.1 };
        assert_eq!(conflict["error"]["code"], "announcement_conflict");
        let after = snapshot(&state.db).await;
        assert_eq!(after["announcements"][0]["revision"], 2);
        assert_eq!(after["audits"].as_array().unwrap().len(), before["audits"].as_array().unwrap().len() + 1);
        let winner = get(&app, &format!("{PUBLIC}/{}", item["id"]), None).await;
        let stale = request(&app, "PATCH", &path, Some(&cookie_one), patch_body(&item, "archived", "Stale")).await;
        assert_eq!(stale.0, StatusCode::CONFLICT);
        assert_eq!(stale.1["error"]["code"], "announcement_conflict");
        assert_eq!(snapshot(&state.db).await, after);
        assert_eq!(request(&app, "PATCH", &format!("{ADMIN}/999999"), Some(&cookie_one), patch_body(&item, "draft", "Missing")).await.0, StatusCode::NOT_FOUND);

        reject_audits_at_commit(&state.db).await;
        for (method, path, body) in [
            ("POST", ADMIN.to_string(), json!({"title":"Commit failure","body":"Must roll back"})),
            ("PATCH", path, patch_body(&winner, "archived", "Must roll back")),
        ] {
            let failed = request(&app, method, &path, Some(&cookie_one), body).await;
            assert_eq!(failed.0, StatusCode::INTERNAL_SERVER_ERROR, "{failed:?}");
            assert_eq!(snapshot(&state.db).await, after, "commit audit failure must roll back announcement and audit");
        }
    }).await;
    fixture.finish(outcome).await;
}
