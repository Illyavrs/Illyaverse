//! Original loopback discovery/ingestion stubs and disposable PostgreSQL schemas.
use crate::{common, native_maps, stable};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use futures_util::FutureExt;
use resonance_protocol::submission::{StableSubmission, decrypt_score_field};
use serde_json::{Value, json};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU16, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

const HASH: &str = "0123456789abcdef0123456789abcdef";
const USER: &str = "NativeMapFixture";
const PASSWORD: &str = "original-map-fixture-password";
const CLIENT: &str = "original-map-fixture-client";
const SERVICE_KEY: &str = "original-map-fixture-service-key-32-bytes";
const OFFICIAL_ID: i64 = 700001;
const SET_ID: i64 = 800001;
const UNKNOWN: &str = "-1|false|0|0|0|0|\n0\nUnknown beatmap\n0\n\n";
// Generated independently with Windows .NET Framework RijndaelManaged, 256-bit
// blocks/key, CBC/PKCS7 and IV bytes 0..31. No reference server code is used.
const ENVELOPE: &str = "W/e0n+xIjbOV/5+FZx6k4O0i4OnLIKpNqh/p1B37gg2c/rGmmE24SeizXZnGztd63nNMJb8dulf/rXwbwfufVeSW1aVv9xQRZBJFEPsdncaU50HMcvZvSguZjc/gmlah50viEsE+2WfQHHcPG2WULY7WtpDXXwp9n7bLy20BDs8tWkSnUCrrJ3D04tOA/jUvKcV6ehOyGCUVm0a04d4Rsw==";
const ENCRYPTED_CLIENT: &str = "NKrV9nP+VMd7l6FwV1C7IfGW0MyuaqjDVfvZ4hkYWwo=";
const IV: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
// Python's standard-library LZMA encoder, FORMAT_ALONE, three original frames.
const FRAMES: &[u8] = &[
    93, 0, 0, 128, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 24, 140, 48, 15, 131, 97, 187, 4,
    147, 222, 218, 150, 237, 232, 154, 181, 230, 127, 105, 39, 21, 108, 38, 89, 217, 166, 59, 158,
    63, 254, 78, 28, 179, 241, 0, 34, 216, 255, 244, 58, 112, 0,
];

#[derive(Clone)]
struct Backend {
    pool: PgPool,
    status: Arc<AtomicU16>,
    invalid: Arc<AtomicU8>,
    calls: Arc<Mutex<Vec<Value>>>,
    ingested: Arc<Mutex<Vec<Value>>>,
}

async fn resolve(
    State(state): State<Backend>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(headers["x-illya-service-key"], SERVICE_KEY);
    assert_eq!(input.as_object().unwrap().len(), 2);
    assert!(input["user_id"].as_i64().unwrap() > 0);
    assert!(input.get("official_id").is_none());
    state.calls.lock().await.push(input.clone());
    let status = StatusCode::from_u16(state.status.load(Ordering::SeqCst)).unwrap();
    if status != StatusCode::OK {
        let code = if status == StatusCode::NOT_FOUND && state.invalid.load(Ordering::SeqCst) != 4 {
            "beatmap_not_found"
        } else {
            "original_fixture_error"
        };
        return (status, Json(json!({"error":{"code":code}})));
    }
    let hash = input["checksum"].as_str().unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,max_combo,md5,osu_path) VALUES($1,$2,'Native map','Fixture','Fixture','Discovery',0,'ranked','official',1,2,$3,'fixture.osu') ON CONFLICT(md5) DO UPDATE SET md5=beatmaps.md5 RETURNING id")
        .bind(OFFICIAL_ID).bind(SET_ID).bind(hash).fetch_one(&state.pool).await.unwrap();
    let mut body = json!({"id":id,"official_id":OFFICIAL_ID,"official_set_id":SET_ID,"md5":hash,"status":"ranked","source":"official","mode":0});
    match state.invalid.load(Ordering::SeqCst) {
        1 => body["md5"] = json!("ffffffffffffffffffffffffffffffff"),
        2 => body["id"] = json!(id + 123),
        3 => body["official_id"] = json!(OFFICIAL_ID + 1),
        _ => {}
    }
    (StatusCode::OK, Json(body))
}

async fn ingest(
    State(state): State<Backend>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Json<Value> {
    assert_eq!(headers["x-illya-service-key"], SERVICE_KEY);
    assert_eq!(input["client"], "stable");
    state.ingested.lock().await.push(input.clone());
    let id: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key) VALUES($1,$2,0,600,100,2,0,1,true,$3,'stable',$4) RETURNING id")
        .bind(input["user_id"].as_i64().unwrap()).bind(input["beatmap_id"].as_i64().unwrap())
        .bind(&input["hit_counts"]).bind(input["ingestion_key"].as_str().unwrap())
        .fetch_one(&state.pool).await.unwrap();
    Json(json!({"id":id,"pp":1}))
}

struct Fixture {
    control: PgPool,
    state: common::AppState,
    backend: Backend,
    schema: String,
    storage: PathBuf,
    base: String,
    tasks: Vec<JoinHandle<()>>,
    user: i64,
}

impl Fixture {
    async fn new() -> Self {
        let url =
            std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable local PostgreSQL URL");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "loopback fixture required"
        );
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("native_maps_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let storage = std::env::temp_dir().join(&schema);
        tokio::fs::create_dir_all(storage.join("maps"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(storage.join("replays"))
            .await
            .unwrap();
        tokio::fs::write(
            storage.join("maps/fixture.osu"),
            b"original fixture map content",
        )
        .await
        .unwrap();
        let storage = tokio::fs::canonicalize(storage).await.unwrap();
        let salt = SaltString::encode_b64(b"original-map-salt").unwrap();
        let credential = Argon2::default()
            .hash_password(format!("{:x}", md5::compute(PASSWORD)).as_bytes(), &salt)
            .unwrap()
            .to_string();
        let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,'native-map@invalid.test','unused',$2) RETURNING id")
            .bind(USER).bind(credential).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO bancho_sessions(id,user_id,token_hash,expires_at,client_version,client_hash_hash) VALUES($1,$2,$3,now()+interval '1 hour','b20261007',$4)")
            .bind(uuid::Uuid::new_v4()).bind(user).bind(common::token_hash("native-map-fixture-token"))
            .bind(common::token_hash(CLIENT)).execute(&pool).await.unwrap();
        let backend = Backend {
            pool: pool.clone(),
            status: Arc::new(AtomicU16::new(200)),
            invalid: Arc::new(AtomicU8::new(0)),
            calls: Default::default(),
            ingested: Default::default(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/api/v1/internal/beatmaps/resolve", post(resolve))
            .route("/api/v1/internal/scores", post(ingest))
            .with_state(backend.clone());
        let backend_task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let state = common::AppState {
            pool,
            storage: Arc::new(storage.clone()),
            sanctuary_url: Arc::new(backend_url),
            service_key: Arc::new(SERVICE_KEY.into()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = stable::router().with_state(state.clone());
        let gateway_task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            control,
            state,
            backend,
            schema,
            storage,
            base,
            tasks: vec![backend_task, gateway_task],
            user,
        }
    }

    fn auth(&self) -> Vec<(&'static str, String)> {
        vec![
            ("u", USER.into()),
            ("h", format!("{:x}", md5::compute(PASSWORD))),
        ]
    }

    async fn board(&self, hash: &str) -> reqwest::Response {
        let mut query = self.auth();
        query.extend([("c", hash.into()), ("m", "0".into())]);
        self.state
            .http
            .get(format!("{}/web/osu-osz2-getscores.php", self.base))
            .query(&query)
            .send()
            .await
            .unwrap()
    }

    async fn info(&self, input: Value) -> reqwest::Response {
        self.state
            .http
            .post(format!("{}/web/osu-getbeatmapinfo.php", self.base))
            .query(&self.auth())
            .json(&input)
            .send()
            .await
            .unwrap()
    }

    async fn submit(&self, envelope: &str) -> reqwest::Response {
        let boundary = "original-native-map-fixture-boundary";
        let password = format!("{:x}", md5::compute(PASSWORD));
        let mut body = Vec::new();
        for (name, value) in [
            ("score", envelope),
            ("iv", IV),
            ("osuver", "20261007"),
            ("pass", password.as_str()),
            ("bmk", HASH),
            ("sbk", ""),
            ("s", ENCRYPTED_CLIENT),
        ] {
            body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        }
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"score\"; filename=\"fixture.osr\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        body.extend(FRAMES);
        body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        self.state
            .http
            .post(format!("{}/web/osu-submit-modular-selector.php", self.base))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        for task in self.tasks {
            task.abort();
            let _ = task.await;
        }
        self.state.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
        // This unique canonical fixture directory was created and owned above.
        assert_eq!(self.storage.file_name().unwrap(), self.schema.as_str());
        assert!(
            self.storage
                .starts_with(tokio::fs::canonicalize(std::env::temp_dir()).await.unwrap())
        );
        tokio::fs::remove_dir_all(self.storage).await.unwrap();
    }
}

#[test]
fn native_map_encrypted_fixture_has_valid_checksum() {
    let decoded = decrypt_score_field(ENVELOPE, IV, "20261007").unwrap();
    let score = StableSubmission::parse(&decoded).unwrap();
    assert_eq!(score.beatmap_md5, HASH);
    assert_eq!(score.username, USER);
    assert_eq!(
        decrypt_score_field(ENCRYPTED_CLIENT, IV, "20261007").unwrap(),
        CLIENT
    );
    score.validate_checksum("20261007", CLIENT, "").unwrap();
    assert_eq!(
        resonance_protocol::submission::validate_compressed_frames(FRAMES).unwrap(),
        3
    );
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_maps_discover_authenticated_checksum_and_cache() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let board = f.board(&HASH.to_uppercase()).await;
        assert_eq!(board.status(), StatusCode::OK);
        assert!(
            board
                .text()
                .await
                .unwrap()
                .starts_with(&format!("2|false|{OFFICIAL_ID}|{SET_ID}|"))
        );
        assert_eq!(
            *f.backend.calls.lock().await,
            vec![json!({"user_id":f.user,"checksum":HASH})]
        );
        let local: i64 = sqlx::query_scalar("SELECT id FROM beatmaps WHERE md5=$1")
            .bind(HASH)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_ne!(local, OFFICIAL_ID);
        assert_eq!(f.board(HASH).await.status(), StatusCode::OK);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
        let info = f.info(json!({"Ids":[OFFICIAL_ID],"Filenames":[]})).await;
        assert_eq!(
            info.text().await.unwrap(),
            format!("0|{OFFICIAL_ID}|{SET_ID}|{HASH}|1|N|N|N|N")
        );
        let file = f
            .state
            .http
            .get(format!("{}/osu/{OFFICIAL_ID}", f.base))
            .send()
            .await
            .unwrap();
        assert_eq!(file.status(), StatusCode::OK);
        assert_eq!(file.text().await.unwrap(), "original fixture map content");
        let score = f.submit(ENVELOPE).await;
        assert_eq!(score.status(), StatusCode::OK);
        let chart = score.text().await.unwrap();
        assert!(chart.starts_with(&format!("beatmapId:{OFFICIAL_ID}|beatmapSetId:{SET_ID}|")));
        assert!(chart.contains("onlineScoreId:"));
        assert_eq!(f.backend.ingested.lock().await[0]["beatmap_id"], local);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_maps_discover_from_valid_score_and_keep_local_storage_identity() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let score = f.submit(ENVELOPE).await;
        assert_eq!(score.status(), StatusCode::OK);
        let chart = score.text().await.unwrap();
        assert!(chart.starts_with(&format!("beatmapId:{OFFICIAL_ID}|beatmapSetId:{SET_ID}|")));
        assert!(chart.contains("onlineScoreId:"));
        assert_eq!(
            *f.backend.calls.lock().await,
            vec![json!({"user_id":f.user,"checksum":HASH})]
        );
        let local: i64 = sqlx::query_scalar("SELECT id FROM beatmaps WHERE md5=$1")
            .bind(HASH)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_ne!(local, OFFICIAL_ID);
        assert_eq!(f.backend.ingested.lock().await[0]["beatmap_id"], local);
        let verification: String =
            sqlx::query_scalar("SELECT verification FROM scores WHERE beatmap_id=$1")
                .bind(local)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(verification, "pending");
        assert!(
            f.board(HASH)
                .await
                .text()
                .await
                .unwrap()
                .starts_with(&format!("2|false|{OFFICIAL_ID}|{SET_ID}|"))
        );
        assert_eq!(f.backend.calls.lock().await.len(), 1);
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_maps_only_genuine_not_found_is_unknown() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        f.backend.status.store(404, Ordering::SeqCst);
        let board = f.board(HASH).await;
        assert_eq!(board.status(), StatusCode::OK);
        let body = board.text().await.unwrap();
        assert_eq!(body, UNKNOWN);
        assert_eq!(body.len(), 39);
        let score = f.submit(ENVELOPE).await;
        assert_eq!(score.status(), StatusCode::OK);
        assert_eq!(score.text().await.unwrap(), "error: beatmap");
        f.backend.invalid.store(4, Ordering::SeqCst);
        assert_eq!(f.board(HASH).await.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(f.submit(ENVELOPE).await.status(), StatusCode::BAD_GATEWAY);
        f.backend.invalid.store(0, Ordering::SeqCst);
        for status in [503, 502, 429, 409] {
            f.backend.status.store(status, Ordering::SeqCst);
            let board = f.board(HASH).await;
            assert_eq!(board.status().as_u16(), status);
            assert_ne!(board.text().await.unwrap(), UNKNOWN);
            assert_eq!(f.submit(ENVELOPE).await.status().as_u16(), status);
        }
        assert!(f.backend.ingested.lock().await.is_empty());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM scores")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_maps_reject_invalid_discovery_and_auth_before_service() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let denied = f
            .state
            .http
            .get(format!("{}/web/osu-osz2-getscores.php", f.base))
            .query(&[
                ("u", USER),
                ("h", "00000000000000000000000000000000"),
                ("c", HASH),
                ("m", "0"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            f.board("invalid-hash").await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.submit("invalid-envelope").await.status(),
            StatusCode::BAD_REQUEST
        );
        assert!(f.backend.calls.lock().await.is_empty());
        let mut unconfigured = f.state.clone();
        unconfigured.service_key = Arc::new(String::new());
        assert_eq!(
            native_maps::resolve_checksum(&unconfigured, f.user, HASH)
                .await
                .unwrap_err()
                .0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(f.backend.calls.lock().await.is_empty());
        for invalid in [1, 2, 3] {
            f.backend.invalid.store(invalid, Ordering::SeqCst);
            assert_eq!(f.board(HASH).await.status(), StatusCode::BAD_GATEWAY);
            sqlx::query("DELETE FROM beatmaps WHERE md5=$1")
                .bind(HASH)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1")
            .bind(f.user)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let before = f.backend.calls.lock().await.len();
        let score = f.submit(ENVELOPE).await;
        assert_eq!(score.status(), StatusCode::OK);
        assert_eq!(score.text().await.unwrap(), "");
        assert_eq!(f.backend.calls.lock().await.len(), before);
        assert!(f.backend.ingested.lock().await.is_empty());
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn native_map_info_prefers_official_identity_and_does_not_discover_batches() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let local: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('Local','Fixture','Fixture','Local',0,'pending',1,'11111111111111111111111111111111','fixture.osu') RETURNING id").fetch_one(&f.state.pool).await.unwrap();
        let imported: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES($1,9,'Official','Fixture','Fixture','Official',0,'loved','official',1,$2,'fixture.osu') RETURNING id").bind(local).bind(HASH).fetch_one(&f.state.pool).await.unwrap();
        let fallback: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('Fallback','Fixture','Fixture','Fallback',0,'ranked',1,'22222222222222222222222222222222','fixture.osu') RETURNING id").fetch_one(&f.state.pool).await.unwrap();
        let row = sqlx::query("SELECT native_id,native_set_id FROM beatmaps WHERE id=$1").bind(fallback).fetch_one(&f.state.pool).await.unwrap();
        let fallback_native: i64 = row.get("native_id");
        let fallback_set: i64 = row.get("native_set_id");
        let local_native: i64 = sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1").bind(local).fetch_one(&f.state.pool).await.unwrap();
        let local_set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1").bind(local).fetch_one(&f.state.pool).await.unwrap();
        let info = f.info(json!({"Ids":[local,imported,fallback_native,OFFICIAL_ID,local_native],"Filenames":["Fixture - Local (Fixture) [Local].osu"]})).await;
        assert_eq!(info.status(),StatusCode::OK);
        assert_eq!(info.text().await.unwrap(),format!("0|{local}|9|{HASH}|4|N|N|N|N\n2|{fallback_native}|{fallback_set}|22222222222222222222222222222222|1|N|N|N|N\n4|{local_native}|{local_set}|11111111111111111111111111111111|0|N|N|N|N"));
        assert!(f.backend.calls.lock().await.is_empty());
        let board = f.board(HASH).await;
        assert!(board.text().await.unwrap().starts_with(&format!("5|false|{local}|9|")));
        let score = f.submit(ENVELOPE).await;
        assert_eq!(score.status(),StatusCode::OK);
        assert!(score.text().await.unwrap().starts_with(&format!("beatmapId:{local}|beatmapSetId:9|")));
        assert_eq!(f.backend.ingested.lock().await[0]["beatmap_id"],imported);
        assert!(f.backend.calls.lock().await.is_empty());
        let info = f.info(json!({"Ids":[],"Filenames":["Fixture - Local (Fixture) [Local].osu"]})).await;
        assert_eq!(info.text().await.unwrap(),"");
        let mut unconfigured = f.state.clone();
        unconfigured.service_key = Arc::new(String::new());
        assert_eq!(native_maps::resolve_checksum(&unconfigured,f.user,HASH).await.unwrap(),Some(imported));
        assert!(f.backend.calls.lock().await.is_empty());
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_local_package_direct_download_and_score_use_canonical_ids() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let sibling_hash = "11111111111111111111111111111111";
        let archive = b"complete original package bytes including assets";
        tokio::fs::write(f.storage.join("maps/package.osz"), archive).await.unwrap();
        let mut manifest = vec![HASH, sibling_hash];
        manifest.sort();
        let package: uuid::Uuid = sqlx::query_scalar("INSERT INTO beatmap_packages(kind,member_md5,archive_path,legacy) VALUES('osz',$1,'package.osz',true) RETURNING id")
            .bind(manifest).fetch_one(&f.state.pool).await.unwrap();
        let mut ids = Vec::new();
        for (hash, version) in [(HASH,"Primary"),(sibling_hash,"Sibling")] {
            let row = sqlx::query("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,status,stars,max_combo,md5,osu_path,archive_path) VALUES($1,'Complete package fixture','Fixture','Fixture',$2,0,'loved',1,2,$3,'fixture.osu','package.osz') RETURNING id,native_id,native_set_id")
                .bind(package).bind(version).bind(hash).fetch_one(&f.state.pool).await.unwrap();
            ids.push((row.get::<i64,_>("id"), row.get::<i64,_>("native_id"), row.get::<i64,_>("native_set_id")));
        }
        let (internal, native, set) = ids[0];
        assert_ne!(native, internal);
        assert_eq!(set, ids[1].2);
        let mut search = f.auth();
        search.push(("q", "Complete package fixture".into()));
        let listing = f.state.http.get(format!("{}/web/osu-search.php",f.base)).query(&search).send().await.unwrap();
        assert_eq!(listing.status(), StatusCode::OK);
        let text = listing.text().await.unwrap();
        assert_eq!(text.lines().count(),2);
        assert!(text.starts_with(&format!("1\n{set}.osz|")));
        assert!(text.contains("Primary@0") && text.contains("Sibling@0"));
        for (selector, value) in [("b",native),("b",ids[1].1),("s",set)] {
            let mut query = f.auth(); query.push((selector,value.to_string()));
            let row = f.state.http.get(format!("{}/web/osu-search-set.php",f.base)).query(&query).send().await.unwrap();
            assert_eq!(row.status(),StatusCode::OK);
            assert!(row.text().await.unwrap().starts_with(&format!("{set}.osz|")));
        }
        let download = f.state.http.get(format!("{}/d/{set}",f.base)).send().await.unwrap();
        assert_eq!(download.status(),StatusCode::OK);
        assert_eq!(download.bytes().await.unwrap().as_ref(),archive);
        let file = f.state.http.get(format!("{}/osu/{native}",f.base)).send().await.unwrap();
        assert_eq!(file.status(),StatusCode::OK);
        assert_eq!(file.bytes().await.unwrap().as_ref(),b"original fixture map content");
        let info = f.info(json!({"Ids":[native,ids[1].1]})).await.text().await.unwrap();
        assert!(info.starts_with(&format!("0|{native}|{set}|{HASH}|")));
        assert!(info.contains(&format!("1|{}|{set}|{sibling_hash}|",ids[1].1)));
        assert!(f.board(HASH).await.text().await.unwrap().starts_with(&format!("5|false|{native}|{set}|")));
        let submitted = f.submit(ENVELOPE).await;
        assert_eq!(submitted.status(),StatusCode::OK);
        assert!(submitted.text().await.unwrap().starts_with(&format!("beatmapId:{native}|beatmapSetId:{set}|")));
        assert_eq!(f.backend.ingested.lock().await[0]["beatmap_id"],internal);
        let old = f.state.http.get(format!("{}/osu/{internal}",f.base)).send().await.unwrap();
        assert_eq!(old.status(),StatusCode::NOT_FOUND);
        for path in ["/osu/2147483648","/d/2147483648"] {
            assert_eq!(f.state.http.get(format!("{}{path}",f.base)).send().await.unwrap().status(),StatusCode::BAD_REQUEST);
        }
        assert!(f.backend.calls.lock().await.is_empty());
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
