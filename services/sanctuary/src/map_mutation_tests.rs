//! Commit authorization fixtures use only an owned PG17 schema and loopback HTTP.
use crate::{AppState, Router, mail_delivery, router};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{env, path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

pub(crate) const SERVICE_KEY: &str = "mutation-fixture-service-key-at-least-thirty-two-bytes";

#[test]
fn commit_cleanup_requires_confirmed_postgres_rollback() {
    use sqlx::postgres::PgSeverity;
    for code in [
        "23000", "23001", "23502", "23503", "23505", "23514", "23P01", "25P02", "40000", "40001",
        "40002", "40P01", "P0001",
    ] {
        assert!(
            super::commit_error_confirms_rollback(PgSeverity::Error, code),
            "{code}"
        );
        for severity in [
            PgSeverity::Fatal,
            PgSeverity::Panic,
            PgSeverity::Warning,
            PgSeverity::Notice,
        ] {
            assert!(
                !super::commit_error_confirms_rollback(severity, code),
                "{severity:?}/{code}"
            );
        }
    }
    for code in [
        "40003", "08007", "08006", "08000", "57P01", "57P02", "57014", "XX000", "XX001", "58000",
        "00000", "P0002", "", "unknown",
    ] {
        assert!(
            !super::commit_error_confirms_rollback(PgSeverity::Error, code),
            "{code}"
        );
    }
    assert!(!super::commit_definitely_rolled_back(&sqlx::Error::Io(
        std::io::Error::new(std::io::ErrorKind::ConnectionReset, "owned fixture reset")
    )));
    assert!(!super::commit_definitely_rolled_back(
        &sqlx::Error::Protocol("owned fixture incomplete COMMIT response".into())
    ));
    assert!(!super::commit_definitely_rolled_back(
        &sqlx::Error::PoolClosed
    ));
}

pub(crate) struct MutationFixture {
    control: PgPool,
    pub(crate) db: PgPool,
    pub(crate) state: AppState,
    pub(crate) app: Router,
    pub(crate) schema: String,
    pub(crate) root: PathBuf,
}
impl MutationFixture {
    pub(crate) async fn new(label: &str, official_origin: String) -> Self {
        let url = env::var("ILLYA_TEST_DATABASE_URL").expect("owned disposable PG17 fixture URL");
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
            "fixture requires PostgreSQL 17"
        );
        let schema = format!("mutation_{label}_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let db = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(
                PgConnectOptions::from_str(&url)
                    .unwrap()
                    .options([("search_path", schema.as_str())])
                    .application_name(&schema),
            )
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&db).await.unwrap();
        let root = env::temp_dir().join(&schema);
        for kind in ["maps", "replays", "media"] {
            tokio::fs::create_dir_all(root.join(kind)).await.unwrap();
        }
        let state = AppState {
            db: db.clone(),
            secret: Arc::new("mutation-fixture-session-key-at-least-thirty-two-bytes".into()),
            internal_key: Arc::new(SERVICE_KEY.into()),
            origin: Arc::new("http://localhost:3000".into()),
            mail: Arc::new(mail_delivery::MailConfig::disabled()),
            secure_cookie: false,
            media_root: root.join("media"),
            maps_root: root.join("maps"),
            replay_root: root.join("replays"),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            official: Arc::new(Mutex::new(super::OfficialState::fixture(
                official_origin,
                true,
            ))),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            upload_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            chat_runtime: Default::default(),
        };
        let app = router(state.clone());
        Self {
            control,
            db,
            state,
            app,
            schema,
            root,
        }
    }

    pub(crate) async fn finish(self, result: Result<(), tokio::task::JoinError>) {
        drop(self.app);
        drop(self.state);
        self.db.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
        assert_eq!(self.root.parent(), Some(env::temp_dir().as_path()));
        tokio::fs::remove_dir_all(&self.root).await.unwrap();
        result.unwrap();
    }
}

pub(crate) async fn actor(
    state: &AppState,
    name: &str,
    permissions: &[&str],
) -> (i64, i64, Uuid, String) {
    let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture','fixture') RETURNING id")
        .bind(name).bind(format!("{name}@example.invalid")).fetch_one(&state.db).await.unwrap();
    let role: i64 =
        sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES($1,$2) RETURNING id")
            .bind(format!("fixture-{name}"))
            .bind(permissions)
            .fetch_one(&state.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
        .bind(user)
        .bind(role)
        .execute(&state.db)
        .await
        .unwrap();
    let sid = Uuid::new_v4();
    let expiry = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(user)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":user,"sid":sid,"exp":expiry.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (user, role, sid, format!("illya_session={token}"))
}

pub(crate) async fn request(
    app: &Router,
    path: &str,
    cookie: Option<&str>,
    service: bool,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000")
        .header(header::CONTENT_TYPE, content_type);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    if service {
        builder = builder.header("x-illya-service-key", SERVICE_KEY);
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

pub(crate) fn multipart(filename: &str, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!("--mutation-fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--mutation-fixture--\r\n");
    body
}
pub(crate) const MULTIPART: &str = "multipart/form-data; boundary=mutation-fixture";

pub(crate) async fn wait_for_lock(db: &PgPool, application: &str, fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(application).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("request must reach its intended transaction lock");
}

pub(crate) async fn file_count(root: &std::path::Path) -> usize {
    let mut count = 0;
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let mut entries = tokio::fs::read_dir(directory).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if entry.file_type().await.unwrap().is_dir() {
                directories.push(entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

pub(crate) async fn wait_until_expired(db: &PgPool, expires_at: chrono::DateTime<chrono::Utc>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1::timestamptz")
                .bind(expires_at)
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
    .expect("fixture session must reach its actual database deadline");
}

pub(crate) async fn reject_audits_at_commit(db: &PgPool) {
    sqlx::query("CREATE FUNCTION fixture_reject_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned fixture rejects audit'; END $$")
        .execute(db).await.unwrap();
    sqlx::query("CREATE CONSTRAINT TRIGGER fixture_reject_audit AFTER INSERT ON audit_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fixture_reject_audit()")
        .execute(db).await.unwrap();
}

pub(crate) fn osu_bytes(title: &str) -> Vec<u8> {
    format!("osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:{title}\nArtist:Illyaverse\nCreator:Mutation fixture\nVersion:Easy\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n").into_bytes()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn map_writes_recheck_queued_authority_and_cleanup() {
    let server = crate::native_map_tests::OfficialServer::start().await;
    let fixture = MutationFixture::new("maps", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        sqlx::query("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,stars,md5,osu_path,source) VALUES(7001,8001,'Known','Fixture','Fixture','Easy',0,1,'0123456789abcdef0123456789abcdef','known.osu','official')")
            .execute(&state.db).await.unwrap();
        let mut retry_id = None;
        for operation in ["upload", "known", "cold"] {
            for change in ["revoke", "restrict", "permission", "expire"] {
                let name = format!("Map{operation}{change}");
                let (user, role, sid, cookie) = actor(&state, &name, &["beatmaps.upload", "beatmaps.import"]).await;
                let cold_id = 9000 + user;
                if operation == "cold" {
                    server.add(crate::native_map_tests::OfficialMap::new(cold_id, "ranked")).await;
                    if retry_id.is_none() { retry_id = Some(cold_id); }
                }
                let maps_before: i64 = sqlx::query_scalar("SELECT count(*) FROM beatmaps").fetch_one(&state.db).await.unwrap();
                let audits_before: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap();
                let mut blocker = state.db.begin().await.unwrap();
                sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
                let queued_app = app.clone();
                let body = if operation == "upload" { multipart("fixture.osu", &osu_bytes(&name)) } else { json!({"official_id":if operation == "known" {7001} else {cold_id}}).to_string().into_bytes() };
                let path = if operation == "upload" { "/api/v1/beatmaps/upload" } else { "/api/v1/beatmaps/import" };
                let content_type = if operation == "upload" { MULTIPART } else { "application/json" };
                let pending = tokio::spawn(async move { request(&queued_app, path, Some(&cookie), false, content_type, body).await });
                wait_for_lock(&state.db, &application, "LOCK TABLE roles,user_roles IN SHARE MODE").await;
                if operation != "known" { assert!(file_count(&state.maps_root).await > 0, "cold bytes must be prepared before the guard"); }
                match change {
                    "revoke" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    "restrict" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(user).execute(&mut *blocker).await.unwrap(); }
                    "permission" => { sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&mut *blocker).await.unwrap(); }
                    "expire" => { sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap(); }
                    _ => unreachable!(),
                }
                blocker.commit().await.unwrap();
                let (status, value) = pending.await.unwrap();
                assert_eq!(status, if change == "permission" {StatusCode::FORBIDDEN} else {StatusCode::UNAUTHORIZED}, "{operation}/{change}: {value}");
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM beatmaps").fetch_one(&state.db).await.unwrap(), maps_before);
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap(), audits_before);
                assert_eq!(file_count(&state.maps_root).await, 0, "definite rejection must remove owned files");
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM bancho_packet_queue").fetch_one(&state.db).await.unwrap(), 0);
            }
        }
        // A rejected web guard must not poison the shared resolver's cache.
        let (_, _, _, cookie) = actor(&state, "MapAllowed", &["beatmaps.import"]).await;
        let (status, value) = request(&app, "/api/v1/beatmaps/import", Some(&cookie), false, "application/json", json!({"official_id":retry_id.unwrap()}).to_string().into_bytes()).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(file_count(&state.maps_root).await, 1);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events WHERE action='beatmaps.import'").fetch_one(&state.db).await.unwrap(), 1);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn known_map_import_rechecks_deadline_after_resource_wait() {
    let fixture = MutationFixture::new("map_deadline", "http://127.0.0.1:1".into()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let application = fixture.schema.clone();
    let result = tokio::spawn(async move {
        let map: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,title,artist,creator,version,mode,stars,md5,osu_path) VALUES(7101,'Known','Fixture','Fixture','Easy',0,1,'0123456789abcdef0123456789abcdef','known.osu') RETURNING id").fetch_one(&state.db).await.unwrap();
        let (_, _, sid, cookie) = actor(&state, "MapDeadline", &["beatmaps.import"]).await;
        let expires_at = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE id=$1 RETURNING expires_at").bind(sid).fetch_one(&state.db).await.unwrap();
        let mut blocker = state.db.begin().await.unwrap();
        sqlx::query("SELECT id FROM beatmaps WHERE id=$1 FOR UPDATE").bind(map).fetch_one(&mut *blocker).await.unwrap();
        let pending = tokio::spawn(async move { request(&app, "/api/v1/beatmaps/import", Some(&cookie), false, "application/json", json!({"official_id":7101}).to_string().into_bytes()).await });
        wait_for_lock(&state.db, &application, "SELECT id FROM beatmaps WHERE id=$1 FOR SHARE").await;
        wait_until_expired(&state.db, expires_at).await;
        blocker.commit().await.unwrap();
        let (status, value) = pending.await.unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events").fetch_one(&state.db).await.unwrap(), 0);
        assert_eq!(file_count(&state.maps_root).await, 0);
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; owned disposable PostgreSQL 17"]
async fn map_files_are_cleaned_after_definite_commit_rejection() {
    let server = crate::native_map_tests::OfficialServer::start().await;
    server
        .add(crate::native_map_tests::OfficialMap::new(7201, "ranked"))
        .await;
    let fixture = MutationFixture::new("map_commit", server.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, cookie) =
            actor(&state, "MapCommit", &["beatmaps.upload", "beatmaps.import"]).await;
        reject_audits_at_commit(&state.db).await;
        for (path, content_type, body) in [
            (
                "/api/v1/beatmaps/upload",
                MULTIPART,
                multipart("fixture.osu", &osu_bytes("Commit failure")),
            ),
            (
                "/api/v1/beatmaps/import",
                "application/json",
                json!({"official_id":7201}).to_string().into_bytes(),
            ),
        ] {
            let (status, value) =
                request(&app, path, Some(&cookie), false, content_type, body).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{path}: {value}");
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM beatmaps")
                    .fetch_one(&state.db)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events")
                    .fetch_one(&state.db)
                    .await
                    .unwrap(),
                0
            );
            assert_eq!(file_count(&state.maps_root).await, 0);
        }
    })
    .await;
    fixture.finish(result).await;
    drop(server);
}
