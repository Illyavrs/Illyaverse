//! Original loopback metadata service fixtures and disposable PostgreSQL schemas.
use crate::{beatmaps, common, lazer};
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use futures_util::FutureExt;
use illya_domain::official_set::{MAX_METADATA_RESPONSE_BYTES, OfficialSetMetadata};
use serde_json::{Value, json};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, LazyLock, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify},
    task::JoinHandle,
};

const SERVICE_KEY: &str = "original-native-set-service-key-32-bytes";
const ACCESS: &str = "original-native-set-access-token";
const SECRET_MARKER: &str = "PRIVATE_UPSTREAM_METADATA_MARKER";
const SET: i64 = 820001;
const MAP: i64 = 720001;
const HASHES: [&str; 4] = [
    "0123456789abcdef0123456789abcdef",
    "11111111111111111111111111111111",
    "22222222222222222222222222222222",
    "33333333333333333333333333333333",
];
const ARCHIVE_B_HASHES: [&str; 2] = [
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    "cccccccccccccccccccccccccccccccc",
];
const ARCHIVE_B_BYTES: &[u8] =
    b"original archive B transport fixture with a new complete two-member manifest";

#[derive(Default)]
struct SnapshotBarrier {
    entered: Notify,
    release: Notify,
    cancelled: AtomicBool,
}

static SNAPSHOT_BARRIERS: LazyLock<StdMutex<HashMap<PathBuf, Arc<SnapshotBarrier>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Test-only seam, keyed by unique owned storage rather than a global pause.
/// Other fixtures and every production build execute without this barrier.
pub(crate) async fn after_stored_snapshot(state: &common::AppState) {
    let barrier = SNAPSHOT_BARRIERS
        .lock()
        .unwrap()
        .get(state.storage.as_ref())
        .cloned();
    if let Some(barrier) = barrier {
        if barrier.cancelled.load(Ordering::SeqCst) {
            return;
        }
        barrier.entered.notify_one();
        tokio::time::timeout(Duration::from_secs(3), barrier.release.notified())
            .await
            .expect("owned fixture must release its stored-row snapshot barrier");
    }
}

struct SnapshotRegistration {
    storage: PathBuf,
    barrier: Arc<SnapshotBarrier>,
}

impl SnapshotRegistration {
    fn new(storage: &std::path::Path) -> Self {
        let barrier = Arc::new(SnapshotBarrier::default());
        let previous = SNAPSHOT_BARRIERS
            .lock()
            .unwrap()
            .insert(storage.to_owned(), barrier.clone());
        assert!(
            previous.is_none(),
            "each owned fixture has at most one barrier"
        );
        Self {
            storage: storage.to_owned(),
            barrier,
        }
    }
}

impl Drop for SnapshotRegistration {
    fn drop(&mut self) {
        SNAPSHOT_BARRIERS.lock().unwrap().remove(&self.storage);
        self.barrier.cancelled.store(true, Ordering::SeqCst);
        self.barrier.release.notify_one();
    }
}

fn metadata(id: i64) -> Value {
    let beatmaps: Vec<Value> = HASHES
        .iter()
        .enumerate()
        .map(|(index, checksum)| {
            json!({
                "id":MAP+index as i64,"beatmapset_id":id,"checksum":checksum,
                "version":format!("Original difficulty {index}"),"mode_int":index,
                "status":"ranked","last_updated":"2020-02-02T03:04:05Z",
                "difficulty_rating":5.25+index as f64,"bpm":180.5,"total_length":150,
                "hit_length":135,"max_combo":432,"ar":9.5,"cs":4.0,"od":8.5,"hp":6.0,
                "count_circles":100,"count_sliders":140,"count_spinners":2
            })
        })
        .collect();
    let value = json!({"id":id,"artist":"Original artist","artist_unicode":"歌手",
        "title":"Original set metadata","title_unicode":"原創圖譜組","creator":"Original mapper",
        "status":"ranked","submitted_date":"2020-01-02T03:04:05Z",
        "bpm":180.5,"source":"Original fixture source","tags":"original fixture","nsfw":false,
        "last_updated":"2020-02-02T03:04:05Z","ranked_date":null,
        "genre":{"id":5,"name":"Video Game"},"language":{"id":3,"name":"Japanese"},
        "beatmaps":beatmaps});
    serde_json::from_value::<OfficialSetMetadata>(value.clone())
        .unwrap()
        .validate(id)
        .unwrap();
    value
}

#[derive(Clone)]
enum Reply {
    Json(StatusCode, Value),
    Malformed,
    HtmlNotFound,
    Oversized,
    DelayedHeaders,
    DelayedChunk,
}

#[derive(Clone)]
struct Backend {
    pool: PgPool,
    owner: i64,
    reply: Arc<Mutex<Reply>>,
    calls: Arc<Mutex<Vec<Value>>>,
    resolve_calls: Arc<Mutex<Vec<Value>>>,
    pause: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

async fn service_metadata(
    State(state): State<Backend>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Response {
    assert_eq!(headers["x-illya-service-key"], SERVICE_KEY);
    assert_eq!(input.as_object().unwrap().len(), 2);
    assert_eq!(input["user_id"], state.owner);
    assert!(
        input["official_set_id"]
            .as_i64()
            .is_some_and(|id| (1..2_000_000_000).contains(&id))
    );
    state.calls.lock().await.push(input);
    if state.pause.load(Ordering::SeqCst) {
        state.entered.notify_one();
        tokio::time::timeout(Duration::from_secs(4), state.release.notified())
            .await
            .expect("fixture must release its own paused metadata response");
    }
    let reply = state.reply.lock().await.clone();
    match reply {
        Reply::Json(status, value) => (status, Json(value)).into_response(),
        Reply::Malformed => (
            StatusCode::OK,
            [("content-type", "application/json")],
            format!("{{ malformed {SECRET_MARKER}"),
        )
            .into_response(),
        Reply::HtmlNotFound => (
            StatusCode::NOT_FOUND,
            [("content-type", "text/html")],
            format!("<!doctype html><html>{SECRET_MARKER}</html>"),
        )
            .into_response(),
        Reply::Oversized => {
            // A streaming body deliberately omits Content-Length. The gateway
            // must count bytes while reading, not trust response headers.
            let chunks = vec![
                Ok::<_, std::io::Error>(vec![b' '; MAX_METADATA_RESPONSE_BYTES]),
                Ok(vec![b'x']),
            ];
            Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from_stream(futures_util::stream::iter(chunks)))
                .unwrap()
        }
        Reply::DelayedHeaders => {
            tokio::time::sleep(Duration::from_secs(9)).await;
            Json(metadata(SET)).into_response()
        }
        Reply::DelayedChunk => {
            let stream = futures_util::stream::unfold(0, |index| async move {
                match index {
                    0 => Some((Ok::<_, std::io::Error>(vec![b'{']), 1)),
                    1 => {
                        tokio::time::sleep(Duration::from_secs(9)).await;
                        Some((Ok(vec![b'}']), 2))
                    }
                    _ => None,
                }
            });
            Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from_stream(stream))
                .unwrap()
        }
    }
}

async fn service_resolve(
    State(state): State<Backend>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> Json<Value> {
    assert_eq!(headers["x-illya-service-key"], SERVICE_KEY);
    assert_eq!(input.as_object().unwrap().len(), 2);
    assert_eq!(input["user_id"], state.owner);
    let official = input["official_id"].as_i64().unwrap();
    let index = usize::try_from(official - MAP).unwrap();
    assert!(index < HASHES.len());
    state.resolve_calls.lock().await.push(input);
    let internal: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,'Stored authoritative title','Stored artist','Stored mapper','Stored difficulty',$4,'loved','official',9.25,123,321,77,$5,'fixture.osu') ON CONFLICT(md5) DO UPDATE SET md5=beatmaps.md5 RETURNING id")
        .bind(official).bind(SET).bind(state.owner).bind(index as i16).bind(HASHES[index])
        .fetch_one(&state.pool).await.unwrap();
    Json(
        json!({"id":internal,"official_id":official,"official_set_id":SET,
        "md5":HASHES[index],"status":"loved","source":"official","mode":index}),
    )
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
        let version: i32 =
            sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
                .fetch_one(&control)
                .await
                .unwrap();
        assert!(
            (170_000..180_000).contains(&version),
            "PostgreSQL 17 fixture required"
        );
        let schema = format!("native_set_metadata_{}", uuid::Uuid::new_v4().simple());
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
            b"original metadata fixture difficulty bytes",
        )
        .await
        .unwrap();
        tokio::fs::write(
            storage.join("maps/local.osz"),
            b"original metadata fixture local archive",
        )
        .await
        .unwrap();
        let storage = tokio::fs::canonicalize(storage).await.unwrap();
        let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES('NativeSetMetadataFixture','native-set@invalid.test','unused','unused') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        Self::session(&pool, user).await;
        let backend = Backend {
            pool: pool.clone(),
            owner: user,
            reply: Arc::new(Mutex::new(Reply::Json(StatusCode::OK, metadata(SET)))),
            calls: Default::default(),
            resolve_calls: Default::default(),
            pause: Default::default(),
            entered: Default::default(),
            release: Default::default(),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/api/v1/internal/beatmapsets/metadata",
                post(service_metadata),
            )
            .route("/api/v1/internal/beatmaps/resolve", post(service_resolve))
            .with_state(backend.clone());
        let backend_task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let state = common::AppState {
            pool,
            storage: Arc::new(storage.clone()),
            sanctuary_url: Arc::new(backend_url),
            service_key: Arc::new(SERVICE_KEY.into()),
            // Longer than the metadata operation's own eight-second deadline.
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .build()
                .unwrap(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = lazer::router()
            .merge(beatmaps::router())
            .with_state(state.clone());
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

    async fn session(pool: &PgPool, user: i64) {
        sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')")
            .bind(uuid::Uuid::new_v4()).bind(user).bind(common::token_hash(ACCESS))
            .bind(common::token_hash("original-native-set-refresh" )).execute(pool).await.unwrap();
    }

    async fn request(&self, path: &str, authenticated: bool) -> reqwest::Response {
        let request = self.state.http.get(format!("{}{path}", self.base));
        let request = if authenticated {
            request.bearer_auth(ACCESS)
        } else {
            request
        };
        request.send().await.unwrap()
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.request(path, true).await
    }

    async fn stored(&self, index: usize, set: i64, hash: &str, mode: i16) -> i64 {
        sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,'Stored authoritative title','Stored artist','Stored mapper','Stored difficulty',$4,'loved','official',9.25,123,321,77,$5,'fixture.osu') RETURNING id")
            .bind(MAP+index as i64).bind(set).bind(self.user).bind(mode).bind(hash).fetch_one(&self.state.pool).await.unwrap()
    }

    async fn snapshot(&self) -> Value {
        let maps: Value = sqlx::query_scalar(
            "SELECT coalesce(jsonb_agg(to_jsonb(b) ORDER BY b.id),'[]'::jsonb) FROM beatmaps b",
        )
        .fetch_one(&self.state.pool)
        .await
        .unwrap();
        let aliases: Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.filename,a.beatmap_id),'[]'::jsonb) FROM beatmap_filename_aliases a").fetch_one(&self.state.pool).await.unwrap();
        let mut files = Vec::new();
        let mut directories = vec![self.storage.clone()];
        // Inspect only this fixture's owned root, including nested paths. A
        // newly created directory must count as a write even if it is empty.
        while let Some(directory) = directories.pop() {
            let mut entries = tokio::fs::read_dir(directory).await.unwrap();
            while let Some(entry) = entries.next_entry().await.unwrap() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(&self.storage)
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                let kind = entry.file_type().await.unwrap();
                let fingerprint = if kind.is_dir() {
                    directories.push(path);
                    "directory".to_owned()
                } else if kind.is_file() {
                    assert!(
                        entry.metadata().await.unwrap().len() <= 1024 * 1024,
                        "fixture content remains bounded"
                    );
                    format!("{:x}", md5::compute(tokio::fs::read(path).await.unwrap()))
                } else {
                    // Do not follow links or read any file outside our root.
                    "other".to_owned()
                };
                files.push((relative, fingerprint));
                assert!(files.len() <= 1024, "fixture entries remain bounded");
            }
        }
        files.sort();
        json!({"maps":maps,"aliases":aliases,"files":files})
    }

    async fn publish_archive_b(&self) {
        let ids = [
            self.stored(0, SET, ARCHIVE_B_HASHES[0], 0).await,
            self.stored(1, SET, ARCHIVE_B_HASHES[1], 1).await,
        ];
        let archive = uuid::Uuid::new_v4();
        let directory = format!("official-package-{archive}");
        let path = format!("{directory}/package.osz");
        tokio::fs::create_dir(self.storage.join("maps").join(&directory))
            .await
            .unwrap();
        tokio::fs::write(self.storage.join("maps").join(&path), ARCHIVE_B_BYTES)
            .await
            .unwrap();
        let mut tx = self.state.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(SET).bind(archive).bind(&path).bind("b".repeat(64)).bind(ARCHIVE_B_BYTES.len() as i64)
            .bind(ARCHIVE_B_HASHES.to_vec()).bind(self.user).execute(&mut *tx).await.unwrap();
        for id in ids {
            sqlx::query("INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id) VALUES($1,$2)")
            .bind(archive).bind(id).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
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
        // Only this fixture's newly created canonical temporary directory.
        assert_eq!(self.storage.file_name().unwrap(), self.schema.as_str());
        assert!(
            self.storage
                .starts_with(tokio::fs::canonicalize(std::env::temp_dir()).await.unwrap())
        );
        tokio::fs::remove_dir_all(self.storage).await.unwrap();
    }
}

async fn body(response: reqwest::Response, expected: StatusCode) -> Value {
    let status = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(status, expected, "{text}");
    for secret in [ACCESS, SERVICE_KEY, SECRET_MARKER] {
        assert!(
            !text.contains(secret),
            "upstream secrets must not reach native responses"
        );
    }
    serde_json::from_str(&text).unwrap()
}

macro_rules! fixture_test {
    ($name:ident, $f:ident, $body:block) => {
        #[tokio::test]
        #[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
        async fn $name() {
            let $f = Fixture::new().await;
            let result = std::panic::AssertUnwindSafe(async $body).catch_unwind().await;
            $f.cleanup().await;
            if let Err(panic) = result { std::panic::resume_unwind(panic); }
        }
    };
}

fixture_test!(
    native_set_metadata_requires_auth_and_valid_ids_before_service_access,
    f,
    {
        for path in [
            format!("/api/v2/beatmapsets/{SET}"),
            "/api/v2/beatmapsets/0".into(),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={MAP}"),
        ] {
            body(f.request(&path, false).await, StatusCode::UNAUTHORIZED).await;
        }
        for path in [
            "/api/v2/beatmapsets/0",
            "/api/v2/beatmapsets/-1",
            "/api/v2/beatmapsets/2147483648",
            "/api/v2/beatmapsets/lookup?beatmap_id=0",
            "/api/v2/beatmapsets/lookup?beatmap_id=-1",
            "/api/v2/beatmapsets/lookup?beatmap_id=2147483648",
            "/api/v2/beatmapsets/lookup",
        ] {
            body(f.get(path).await, StatusCode::BAD_REQUEST).await;
        }
        for path in [
            "/api/v2/beatmapsets/2000000000",
            "/api/v2/beatmapsets/2147483647",
        ] {
            body(f.get(path).await, StatusCode::NOT_FOUND).await;
        }
        assert!(f.backend.calls.lock().await.is_empty());
        assert!(f.backend.resolve_calls.lock().await.is_empty());
        for id in [1, 1_999_999_999] {
            *f.backend.reply.lock().await = Reply::Json(StatusCode::OK, metadata(id));
            let value = body(
                f.get(&format!("/api/v2/beatmapsets/{id}")).await,
                StatusCode::OK,
            )
            .await;
            assert_eq!(value["id"], id);
        }
        assert_eq!(f.backend.calls.lock().await.len(), 2);
    }
);

fixture_test!(
    native_set_metadata_returns_all_original_modes_without_publishing_content,
    f,
    {
        let before = f.snapshot().await;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(value["id"], SET);
        assert_eq!(value["submitted_date"], "2020-01-02T03:04:05Z");
        assert_eq!(value["last_updated"], "2020-02-02T03:04:05Z");
        assert_eq!(value["genre"], json!({"id":5,"name":"Video Game"}));
        assert_eq!(value["language"], json!({"id":3,"name":"Japanese"}));
        assert_eq!(value["user_id"], 0);
        assert_eq!(value["download_disabled"], true);
        assert_eq!(value["availability"]["download_disabled"], true);
        assert_eq!(value["preview_url"], "");
        assert!(
            value["covers"]
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == "")
        );
        let maps = value["beatmaps"].as_array().unwrap();
        assert_eq!(maps.len(), 4);
        for (index, map) in maps.iter().enumerate() {
            assert_eq!(map["id"], MAP + index as i64);
            assert_eq!(map["beatmapset_id"], SET);
            assert_eq!(map["checksum"], HASHES[index]);
            assert_eq!(map["mode_int"], index);
            assert_eq!(map["mode"], ["osu", "taiko", "fruits", "mania"][index]);
            assert_eq!(map["difficulty_rating"], 5.25 + index as f64);
            assert_eq!(map["ar"], 9.5);
            assert_eq!(map["cs"], 4.0);
            assert_eq!(map["accuracy"], 8.5);
            assert_eq!(map["drain"], 6.0);
            assert_eq!(map["last_updated"], "2020-02-02T03:04:05Z");
            assert_eq!(map["user_id"], 0);
            assert!(map.get("beatmapset").is_none());
            assert!(
                map["url"]
                    .as_str()
                    .is_none_or(|url| !url.contains("osu.ppy.sh"))
            );
        }
        assert_eq!(f.snapshot().await, before);
        assert_eq!(
            *f.backend.calls.lock().await,
            vec![json!({"user_id":f.user,"official_set_id":SET})]
        );
        assert!(f.backend.resolve_calls.lock().await.is_empty());
        body(
            f.get(&format!("/api/v2/beatmapsets/{SET}/download")).await,
            StatusCode::NOT_FOUND,
        )
        .await;
    }
);

fixture_test!(
    native_set_metadata_preserves_stored_moderation_identity_and_pp_numbers,
    f,
    {
        let internal = f.stored(0, SET, HASHES[0], 0).await;
        sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES('Stored original.osu',$1)").bind(internal).execute(&f.state.pool).await.unwrap();
        let before = f.snapshot().await;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        let maps = value["beatmaps"].as_array().unwrap();
        assert_eq!(maps.len(), 4);
        let map = maps.iter().find(|map| map["id"] == MAP).unwrap();
        assert_eq!(map["id"], MAP);
        assert_ne!(map["id"], internal);
        assert_eq!(map["checksum"], HASHES[0]);
        assert_eq!(map["status"], "loved");
        assert_eq!(map["ranked"], 4);
        assert_eq!(map["version"], "Stored difficulty");
        assert_eq!(map["difficulty_rating"], 9.25);
        assert_eq!(map["bpm"], 123.0);
        assert_eq!(map["total_length"], 321);
        assert_eq!(map["max_combo"], 77);
        assert_eq!(map["user_id"], 0);
        assert_eq!(f.snapshot().await, before);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
    }
);

fixture_test!(
    native_set_metadata_uses_native_pending_status_without_rewriting_unranked_moderation,
    f,
    {
        let mut upstream = metadata(SET);
        upstream["status"] = json!("unranked");
        upstream["beatmaps"][0]["status"] = json!("unranked");
        *f.backend.reply.lock().await = Reply::Json(StatusCode::OK, upstream);
        let before = f.snapshot().await;
        let cold = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(cold["status"], "pending");
        assert_eq!(cold["beatmaps"][0]["status"], "pending");
        assert_eq!(cold["beatmaps"][0]["ranked"], 0);
        assert_eq!(f.snapshot().await, before);
        let internal = f.stored(0, SET, HASHES[0], 0).await;
        sqlx::query("UPDATE beatmaps SET status='unranked' WHERE id=$1")
            .bind(internal)
            .execute(&f.state.pool)
            .await
            .unwrap();
        *f.backend.reply.lock().await = Reply::Json(StatusCode::OK, metadata(SET));
        let before = f.snapshot().await;
        let warm = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(warm["status"], "pending");
        let map = warm["beatmaps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|map| map["id"] == MAP)
            .unwrap();
        assert_eq!(map["status"], "pending");
        assert_eq!(map["ranked"], 0);
        assert_eq!(f.snapshot().await, before);
        let status: String = sqlx::query_scalar("SELECT status FROM beatmaps WHERE id=$1")
            .bind(internal)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(status, "unranked");
    }
);

fixture_test!(
    native_set_metadata_rejects_stored_identity_version_conflicts,
    f,
    {
        for (set, hash, mode) in [
            (SET + 1, HASHES[0], 0),
            (SET, HASHES[1], 0),
            (SET, HASHES[0], 1),
        ] {
            f.stored(0, set, hash, mode).await;
            let before = f.snapshot().await;
            let value = body(
                f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
                StatusCode::CONFLICT,
            )
            .await;
            assert_eq!(value["error"], "beatmap_set_version_conflict");
            assert_eq!(f.snapshot().await, before);
            sqlx::query("DELETE FROM beatmaps")
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        // A matching checksum cannot be silently attributed to another official ID.
        f.stored(1, SET, HASHES[0], 0).await;
        let before = f.snapshot().await;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::CONFLICT,
        )
        .await;
        assert_eq!(value["error"], "beatmap_set_version_conflict");
        assert_eq!(f.snapshot().await, before);
    }
);

fixture_test!(
    native_set_metadata_local_packages_use_only_canonical_local_members,
    f,
    {
        *f.backend.reply.lock().await = Reply::Json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":{"code":SECRET_MARKER}}),
        );
        let package:uuid::Uuid = sqlx::query_scalar("INSERT INTO beatmap_packages(kind,member_md5,archive_path,legacy) VALUES('osz',$1,'local.osz',true) RETURNING id")
        .bind(vec![HASHES[0],HASHES[1]]).fetch_one(&f.state.pool).await.unwrap();
        let mut identities = Vec::new();
        for (index, hash) in HASHES[..2].iter().enumerate() {
            let row = sqlx::query("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path) VALUES($1,'Local set fixture','Fixture','Fixture',$2,0,'loved',1,$3,'fixture.osu','local.osz') RETURNING id,native_id,native_set_id")
            .bind(package).bind(format!("Local difficulty {index}")).bind(hash).fetch_one(&f.state.pool).await.unwrap();
            identities.push((
                row.get::<i64, _>("id"),
                row.get::<i64, _>("native_id"),
                row.get::<i64, _>("native_set_id"),
            ));
        }
        let before = f.snapshot().await;
        let set = identities[0].2;
        assert!((2_000_000_000..=i32::MAX as i64).contains(&set));
        for path in [
            format!("/api/v2/beatmapsets/{set}"),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={}", identities[1].1),
        ] {
            let value = body(f.get(&path).await, StatusCode::OK).await;
            assert_eq!(value["id"], set);
            assert_eq!(value["availability"]["download_disabled"], false);
            assert_eq!(value["beatmaps"].as_array().unwrap().len(), 2);
            for (_, native, _) in &identities {
                assert!(
                    value["beatmaps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|map| map["id"] == *native)
                );
            }
        }
        assert_eq!(f.snapshot().await, before);
        assert!(f.backend.calls.lock().await.is_empty());
        assert!(f.backend.resolve_calls.lock().await.is_empty());
    }
);

fixture_test!(
    native_set_metadata_verified_official_archive_bypasses_discovery,
    f,
    {
        *f.backend.reply.lock().await = Reply::Json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":{"code":SECRET_MARKER}}),
        );
        let ids = [
            f.stored(0, SET, HASHES[0], 0).await,
            f.stored(1, SET, HASHES[1], 1).await,
        ];
        let bytes = b"original complete official archive transport fixture";
        let archive = uuid::Uuid::new_v4();
        let directory = format!("official-package-{archive}");
        let path = format!("{directory}/package.osz");
        tokio::fs::create_dir(f.storage.join("maps").join(&directory))
            .await
            .unwrap();
        tokio::fs::write(f.storage.join("maps").join(&path), bytes)
            .await
            .unwrap();
        let mut tx = f.state.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(SET).bind(archive).bind(&path).bind("b".repeat(64)).bind(bytes.len() as i64).bind(vec![HASHES[0],HASHES[1]]).bind(f.user).execute(&mut *tx).await.unwrap();
        for id in ids {
            sqlx::query("INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id) VALUES($1,$2)").bind(archive).bind(id).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        let before = f.snapshot().await;
        for route in [
            format!("/api/v2/beatmapsets/{SET}"),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={}", MAP + 1),
        ] {
            let value = body(f.get(&route).await, StatusCode::OK).await;
            assert_eq!(value["id"], SET);
            assert_eq!(value["beatmaps"].as_array().unwrap().len(), 2);
            assert_eq!(value["availability"]["download_disabled"], false);
            assert!(
                value["beatmaps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|map| map["status"] == "loved")
            );
        }
        let download = f.get(&format!("/api/v2/beatmapsets/{SET}/download")).await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(download.bytes().await.unwrap().as_ref(), bytes);
        assert_eq!(f.snapshot().await, before);
        assert!(f.backend.calls.lock().await.is_empty());
        assert!(f.backend.resolve_calls.lock().await.is_empty());
    }
);

fixture_test!(
    native_set_metadata_concurrent_verified_archive_returns_its_whole_manifest,
    f,
    {
        // Hold the old four-member metadata A response until a new complete
        // two-member archive B is committed under the same public set ID.
        f.backend.pause.store(true, Ordering::SeqCst);
        let request = f
            .state
            .http
            .get(format!("{}/api/v2/beatmapsets/{SET}", f.base))
            .bearer_auth(ACCESS);
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(3), f.backend.entered.notified())
            .await
            .expect("metadata A reached its fixture barrier");
        f.publish_archive_b().await;
        // An ordinary post-publication read is the canonical complete B DTO.
        // This also proves the already-published archive bypasses the paused
        // metadata service rather than requesting A a second time.
        let expected = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        let before = f.snapshot().await;
        f.backend.release.notify_one();
        let actual = body(pending.await.unwrap(), StatusCode::OK).await;
        assert_eq!(
            actual, expected,
            "late archive publication must replace the whole metadata response"
        );
        assert_eq!(actual["id"], SET);
        assert_eq!(actual["title"], "Stored authoritative title");
        assert_eq!(actual["download_disabled"], false);
        assert_eq!(actual["availability"]["download_disabled"], false);
        let maps = actual["beatmaps"].as_array().unwrap();
        assert_eq!(maps.len(), 2);
        for (index, hash) in ARCHIVE_B_HASHES.iter().enumerate() {
            let map = maps
                .iter()
                .find(|map| map["id"] == MAP + index as i64)
                .unwrap();
            assert_eq!(map["checksum"], *hash);
            assert_eq!(map["status"], "loved");
        }
        assert!(
            maps.iter()
                .all(|map| !HASHES.contains(&map["checksum"].as_str().unwrap())),
            "metadata A must not remain mixed into B"
        );
        let download = f.get(&format!("/api/v2/beatmapsets/{SET}/download")).await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(download.bytes().await.unwrap().as_ref(), ARCHIVE_B_BYTES);
        assert_eq!(f.snapshot().await, before);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
        assert!(f.backend.resolve_calls.lock().await.is_empty());
    }
);

fixture_test!(
    native_set_metadata_archive_after_empty_snapshot_replaces_entire_response,
    f,
    {
        let registration = SnapshotRegistration::new(f.storage.as_path());
        let request = f
            .state
            .http
            .get(format!("{}/api/v2/beatmapsets/{SET}", f.base))
            .bearer_auth(ACCESS);
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(
            Duration::from_secs(3),
            registration.barrier.entered.notified(),
        )
        .await
        .expect("metadata A completed its empty stored-row snapshot");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM beatmaps")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "snapshot barrier precedes archive B publication");
        f.publish_archive_b().await;
        let expected = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        let before = f.snapshot().await;
        registration.barrier.release.notify_one();
        let actual = body(pending.await.unwrap(), StatusCode::OK).await;
        assert_eq!(
            actual, expected,
            "a late archive replaces metadata A rather than changing only availability"
        );
        let maps = actual["beatmaps"].as_array().unwrap();
        assert_eq!(maps.len(), 2);
        for (index, hash) in ARCHIVE_B_HASHES.iter().enumerate() {
            let map = maps
                .iter()
                .find(|map| map["id"] == MAP + index as i64)
                .unwrap();
            assert_eq!(map["checksum"], *hash);
        }
        assert!(
            maps.iter()
                .all(|map| !HASHES.contains(&map["checksum"].as_str().unwrap()))
        );
        assert_eq!(actual["download_disabled"], false);
        assert_eq!(actual["availability"]["download_disabled"], false);
        let download = f.get(&format!("/api/v2/beatmapsets/{SET}/download")).await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(download.bytes().await.unwrap().as_ref(), ARCHIVE_B_BYTES);
        assert_eq!(f.snapshot().await, before);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
        assert!(f.backend.resolve_calls.lock().await.is_empty());
        drop(registration);
        assert!(!SNAPSHOT_BARRIERS.lock().unwrap().contains_key(&f.storage));
    }
);

fixture_test!(
    native_set_metadata_maps_only_allowlisted_service_errors_without_secrets,
    f,
    {
        let before = f.snapshot().await;
        for (status, code, expected_status, expected_code) in [
            (404, "beatmapset_not_found", 404, "beatmapset_not_found"),
            (404, SECRET_MARKER, 502, "set_metadata_failed"),
            (503, SECRET_MARKER, 503, "set_metadata_unavailable"),
            (429, SECRET_MARKER, 429, "set_metadata_rate_limited"),
            (502, SECRET_MARKER, 502, "set_metadata_failed"),
            (409, SECRET_MARKER, 502, "set_metadata_failed"),
        ] {
            *f.backend.reply.lock().await = Reply::Json(
                StatusCode::from_u16(status).unwrap(),
                json!({"error":{"code":code,"message":SECRET_MARKER},"private":ACCESS}),
            );
            let value = body(
                f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
                StatusCode::from_u16(expected_status).unwrap(),
            )
            .await;
            assert_eq!(value["error"], expected_code);
        }
        assert_eq!(f.snapshot().await, before);
        assert!(f.backend.resolve_calls.lock().await.is_empty());
        *f.backend.reply.lock().await = Reply::HtmlNotFound;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::BAD_GATEWAY,
        )
        .await;
        assert_eq!(value["error"], "invalid_set_metadata_response");
        assert_eq!(f.snapshot().await, before);
    }
);

fixture_test!(
    native_set_metadata_preserves_local_authority_when_metadata_is_unavailable,
    f,
    {
        f.stored(0, SET, HASHES[0], 0).await;
        let before = f.snapshot().await;
        for (status, code) in [
            (404, "beatmapset_not_found"),
            (404, SECRET_MARKER),
            (503, SECRET_MARKER),
            (429, SECRET_MARKER),
            (502, SECRET_MARKER),
        ] {
            *f.backend.reply.lock().await = Reply::Json(
                StatusCode::from_u16(status).unwrap(),
                json!({"error":{"code":code,"message":SECRET_MARKER}}),
            );
            let value = body(
                f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
                StatusCode::OK,
            )
            .await;
            assert_eq!(value["id"], SET);
            let maps = value["beatmaps"].as_array().unwrap();
            assert_eq!(
                maps.len(),
                1,
                "fallback advertises only actual retained content"
            );
            assert_eq!(maps[0]["id"], MAP);
            assert_eq!(maps[0]["status"], "loved");
            assert_eq!(value["availability"]["download_disabled"], true);
            assert_eq!(f.snapshot().await, before);
        }
        *f.backend.reply.lock().await = Reply::Malformed;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(value["beatmaps"].as_array().unwrap().len(), 1);
        *f.backend.reply.lock().await = Reply::HtmlNotFound;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(value["beatmaps"].as_array().unwrap().len(), 1);
        *f.backend.reply.lock().await = Reply::Json(
            StatusCode::UNAUTHORIZED,
            json!({"error":{"code":SECRET_MARKER}}),
        );
        body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::UNAUTHORIZED,
        )
        .await;
        assert_eq!(f.snapshot().await, before);
        assert!(f.backend.resolve_calls.lock().await.is_empty());
    }
);

fixture_test!(
    native_set_metadata_strictly_validates_internal_dto_and_original_members,
    f,
    {
        let before = f.snapshot().await;
        let original = metadata(SET);
        let mut invalids = Vec::new();
        for (pointer, replacement) in [
            ("/id", json!(SET + 1)),
            ("/submitted_date", json!(SECRET_MARKER)),
            ("/beatmaps/0/id", json!(2_000_000_000)),
            ("/beatmaps/0/beatmapset_id", json!(SET + 1)),
            ("/beatmaps/0/checksum", json!("A".repeat(32))),
            ("/beatmaps/0/mode_int", json!(4)),
            ("/beatmaps/0/hit_length", json!(151)),
            ("/beatmaps/0/ar", json!(21)),
            ("/beatmaps/0/last_updated", Value::Null),
            ("/language/id", json!(0)),
        ] {
            let mut value = original.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            invalids.push(value);
        }
        let mut foreign = original.clone();
        foreign["user_id"] = json!(987654321);
        foreign["covers"] = json!({"cover":format!("https://example.invalid/{SECRET_MARKER}")});
        invalids.push(foreign);
        let mut foreign_map = original.clone();
        foreign_map["beatmaps"][0]["user_id"] = json!(987654321);
        invalids.push(foreign_map);
        let mut duplicate = original.clone();
        let map = duplicate["beatmaps"][0].clone();
        duplicate["beatmaps"].as_array_mut().unwrap().push(map);
        invalids.push(duplicate);
        let mut absent = original.clone();
        absent.as_object_mut().unwrap().remove("genre");
        invalids.push(absent);
        for invalid in invalids {
            *f.backend.reply.lock().await = Reply::Json(StatusCode::OK, invalid);
            let value = body(
                f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
                StatusCode::BAD_GATEWAY,
            )
            .await;
            assert_eq!(value["error"], "invalid_set_metadata_response");
            assert_eq!(f.snapshot().await, before);
        }
        *f.backend.reply.lock().await = Reply::Malformed;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::BAD_GATEWAY,
        )
        .await;
        assert_eq!(value["error"], "invalid_set_metadata_response");
        assert_eq!(f.snapshot().await, before);
    }
);

fixture_test!(
    native_set_metadata_bounds_chunked_service_response_without_content_length,
    f,
    {
        *f.backend.reply.lock().await = Reply::Oversized;
        let before = f.snapshot().await;
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::BAD_GATEWAY,
        )
        .await;
        assert_eq!(value["error"], "invalid_set_metadata_response");
        assert_eq!(f.snapshot().await, before);
    }
);

fixture_test!(
    native_set_metadata_deadline_includes_headers_and_streamed_body,
    f,
    {
        for (reply, status, code) in [
            (
                Reply::DelayedHeaders,
                StatusCode::SERVICE_UNAVAILABLE,
                "set_metadata_unavailable",
            ),
            (
                Reply::DelayedChunk,
                StatusCode::SERVICE_UNAVAILABLE,
                "set_metadata_unavailable",
            ),
        ] {
            *f.backend.reply.lock().await = reply;
            let started = Instant::now();
            let value = body(f.get(&format!("/api/v2/beatmapsets/{SET}")).await, status).await;
            assert_eq!(value["error"], code);
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "metadata operation must enforce its own eight-second bound"
            );
            assert!(
                started.elapsed() >= Duration::from_secs(7),
                "fixture actually exercised the deadline"
            );
        }
    }
);

fixture_test!(
    native_set_metadata_rechecks_session_and_restriction_after_service_await,
    f,
    {
        f.backend.pause.store(true, Ordering::SeqCst);
        for change in ["expire", "restrict", "revoke"] {
            let request = f
                .state
                .http
                .get(format!("{}/api/v2/beatmapsets/{SET}", f.base))
                .bearer_auth(ACCESS);
            let task = tokio::spawn(async move { request.send().await.unwrap() });
            tokio::time::timeout(Duration::from_secs(3), f.backend.entered.notified())
                .await
                .expect("metadata response reached fixture barrier");
            let before = f.snapshot().await;
            match change {
                "expire" => {
                    sqlx::query("UPDATE lazer_sessions SET access_expires_at=now()-interval '1 second' WHERE user_id=$1").bind(f.user).execute(&f.state.pool).await.unwrap();
                }
                "restrict" => {
                    sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
                        .bind(f.user)
                        .execute(&f.state.pool)
                        .await
                        .unwrap();
                }
                "revoke" => {
                    sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
                        .bind(f.user)
                        .execute(&f.state.pool)
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            f.backend.release.notify_one();
            body(task.await.unwrap(), StatusCode::UNAUTHORIZED).await;
            assert_eq!(f.snapshot().await, before);
            sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1")
                .bind(f.user)
                .execute(&f.state.pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
                .bind(f.user)
                .execute(&f.state.pool)
                .await
                .unwrap();
            Fixture::session(&f.state.pool, f.user).await;
        }
        assert_eq!(f.backend.calls.lock().await.len(), 3);
    }
);

fixture_test!(
    native_set_metadata_lookup_returns_full_set_and_score_content_remains_lazy,
    f,
    {
        let before = f.snapshot().await;
        body(
            f.get(&format!("/api/v2/beatmapsets/{SET}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(f.snapshot().await, before);
        // Metadata alone creates no .osu row. Difficulty lookup then uses the
        // existing authenticated content resolver for just the selected member.
        let value = body(
            f.get(&format!("/api/v2/beatmapsets/lookup?beatmap_id={MAP}"))
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(value["beatmaps"].as_array().unwrap().len(), 4);
        assert_eq!(f.backend.resolve_calls.lock().await.len(), 1);
        assert_eq!(f.backend.calls.lock().await.len(), 2);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM beatmaps")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let selected=body(f.state.http.post(format!("{}/api/v2/beatmaps/{MAP}/solo/scores",f.base))
        .bearer_auth(ACCESS).json(&json!({"beatmap_hash":HASHES[0],"version_hash":"original-version","ruleset_id":0}))
        .send().await.unwrap(),StatusCode::OK).await;
        let token = selected["id"].as_i64().unwrap();
        let internal: i64 =
            sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1")
                .bind(token)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        let official: i64 = sqlx::query_scalar("SELECT official_id FROM beatmaps WHERE id=$1")
            .bind(internal)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(official, MAP);
        assert_ne!(internal, MAP);
        assert_eq!(f.backend.resolve_calls.lock().await.len(), 1);
        assert_eq!(f.backend.calls.lock().await.len(), 2);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM beatmaps")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
);
