//! Original HTTP fixtures for native difficulty identity and solo-token bindings.
use crate::{beatmaps, common, lazer};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use futures_util::FutureExt;
use reqwest::{Method, Response};
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
        atomic::{AtomicBool, AtomicI16, AtomicU8, AtomicU16, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify},
    task::JoinHandle,
};

const SERVICE_KEY: &str = "original-lazer-map-fixture-service-key-32-bytes";
const ACCESS: [&str; 2] = [
    "original-lazer-map-access",
    "original-lazer-map-other-access",
];
const OFFICIAL_ID: i64 = 710001;
const SET_ID: i64 = 810001;
const FILENAME: &str = "音樂 & Fixture + 100% (Mapper) [Original].osu";
const HASHES: [&str; 8] = [
    "0123456789abcdef0123456789abcdef",
    "11111111111111111111111111111111",
    "22222222222222222222222222222222",
    "33333333333333333333333333333333",
    "44444444444444444444444444444444",
    "55555555555555555555555555555555",
    "66666666666666666666666666666666",
    "77777777777777777777777777777777",
];

#[derive(Clone)]
struct Backend {
    pool: PgPool,
    status: Arc<AtomicU16>,
    invalid: Arc<AtomicU8>,
    mode: Arc<AtomicI16>,
    calls: Arc<Mutex<Vec<Value>>>,
    ingested: Arc<Mutex<Vec<Value>>>,
    pause: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

async fn resolve(
    State(state): State<Backend>,
    headers: HeaderMap,
    Json(input): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(headers["x-illya-service-key"], SERVICE_KEY);
    let fields = input.as_object().unwrap();
    assert_eq!(fields.len(), 2);
    assert!(input["user_id"].as_i64().unwrap() > 0);
    assert_eq!(
        ["checksum", "official_id", "filename"]
            .iter()
            .filter(|key| fields.contains_key(**key))
            .count(),
        1
    );
    state.calls.lock().await.push(input.clone());
    if state.pause.load(Ordering::SeqCst) {
        state.entered.notify_one();
        tokio::time::timeout(Duration::from_secs(3), state.release.notified())
            .await
            .expect("test must release its paused discovery");
    }
    let status = StatusCode::from_u16(state.status.load(Ordering::SeqCst)).unwrap();
    if status != StatusCode::OK {
        let code = if status == StatusCode::NOT_FOUND && state.invalid.load(Ordering::SeqCst) != 4 {
            "beatmap_not_found"
        } else {
            match state.invalid.load(Ordering::SeqCst) {
                7 => "beatmap_filename_ambiguous",
                8 => "beatmap_filename_conflict",
                9 => "official_set_version_conflict",
                _ => "original_fixture_error",
            }
        };
        return (status, Json(json!({"error":{"code":code}})));
    }
    let index = if let Some(hash) = input.get("checksum") {
        let hash = hash.as_str().unwrap();
        assert_eq!(hash, hash.to_ascii_lowercase());
        HASHES.iter().position(|value| *value == hash).unwrap()
    } else if let Some(filename) = input.get("filename") {
        assert_eq!(filename, FILENAME);
        0
    } else {
        let id = input["official_id"].as_i64().unwrap();
        assert!((OFFICIAL_ID..OFFICIAL_ID + HASHES.len() as i64).contains(&id));
        (id - OFFICIAL_ID) as usize
    };
    let official_id = OFFICIAL_ID + index as i64;
    let set_id = SET_ID + index as i64;
    let mode = state.mode.load(Ordering::SeqCst);
    let id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,max_combo,md5,osu_path) VALUES($1,$2,'Native discovery','Fixture','Fixture','Original', $3,'ranked','official',1,2,$4,'fixture.osu') ON CONFLICT(md5) DO UPDATE SET md5=beatmaps.md5 RETURNING id")
        .bind(official_id).bind(set_id).bind(mode).bind(HASHES[index])
        .fetch_one(&state.pool).await.unwrap();
    if let Some(filename) = input.get("filename")
        && state.invalid.load(Ordering::SeqCst) != 5
    {
        sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
            .bind(filename.as_str().unwrap()).bind(id).execute(&state.pool).await.unwrap();
    }
    let mut body = json!({"id":id,"official_id":official_id,"official_set_id":set_id,"md5":HASHES[index],"status":"ranked","source":"official","mode":mode});
    match state.invalid.load(Ordering::SeqCst) {
        1 => body["md5"] = json!("ffffffffffffffffffffffffffffffff"),
        2 => body["id"] = json!(id + 123),
        3 => body["official_id"] = json!(official_id + 1),
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
    assert_eq!(input["client"], "lazer");
    state.ingested.lock().await.push(input.clone());
    let id: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,native_submission,api_mods) VALUES($1,$2,$3,$4,$5,$6,$7,1,$8,$9,'lazer',$10,$11,$12) RETURNING id")
        .bind(input["user_id"].as_i64().unwrap())
        .bind(input["beatmap_id"].as_i64().unwrap())
        .bind(input["mode"].as_i64().unwrap() as i16)
        .bind(input["score"].as_i64().unwrap())
        .bind(input["accuracy"].as_f64().unwrap())
        .bind(input["max_combo"].as_i64().unwrap() as i32)
        .bind(input["mods"].as_i64().unwrap())
        .bind(input["passed"].as_bool().unwrap())
        .bind(&input["hit_counts"])
        .bind(input["ingestion_key"].as_str().unwrap())
        .bind(&input["native_submission"])
        .bind(&input["api_mods"])
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
    users: [i64; 2],
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
        let schema = format!("lazer_maps_{}", uuid::Uuid::new_v4().simple());
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
        for (name, bytes) in [
            ("fixture.osu", b"original map content".as_slice()),
            ("local.osz", b"original local archive".as_slice()),
            ("official.osz", b"original official archive".as_slice()),
        ] {
            tokio::fs::write(storage.join("maps").join(name), bytes)
                .await
                .unwrap();
        }
        let storage = tokio::fs::canonicalize(storage).await.unwrap();
        let mut users = [0; 2];
        for (index, user) in users.iter_mut().enumerate() {
            *user = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id")
                .bind(format!("NativeLazerFixture{index}"))
                .bind(format!("native-lazer-{index}@invalid.test"))
                .fetch_one(&pool).await.unwrap();
            Self::session(&pool, *user, index).await;
        }
        let backend = Backend {
            pool: pool.clone(),
            status: Arc::new(AtomicU16::new(200)),
            invalid: Arc::new(AtomicU8::new(0)),
            mode: Arc::new(AtomicI16::new(0)),
            calls: Default::default(),
            ingested: Default::default(),
            pause: Default::default(),
            entered: Default::default(),
            release: Default::default(),
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
            users,
        }
    }

    async fn session(pool: &PgPool, user: i64, index: usize) {
        sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')")
            .bind(uuid::Uuid::new_v4()).bind(user).bind(common::token_hash(ACCESS[index]))
            .bind(common::token_hash(&format!("original-refresh-{index}")))
            .execute(pool).await.unwrap();
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        user: Option<usize>,
        body: Option<Value>,
    ) -> Response {
        let mut request = self
            .state
            .http
            .request(method, format!("{}{path}", self.base));
        if let Some(user) = user {
            request = request.bearer_auth(ACCESS[user]);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.unwrap()
    }

    async fn get(&self, path: &str) -> Response {
        self.request(Method::GET, path, Some(0), None).await
    }

    async fn token(&self, map: i64, hash: &str, ruleset: u8) -> Response {
        self.request(
            Method::POST,
            &format!("/api/v2/beatmaps/{map}/solo/scores"),
            Some(0),
            Some(
                json!({"beatmap_hash":hash,"version_hash":"original-version","ruleset_id":ruleset}),
            ),
        )
        .await
    }

    async fn submit(&self, map: i64, token: i64, user: usize, body: Value) -> Response {
        self.request(
            Method::PUT,
            &format!("/api/v2/beatmaps/{map}/solo/scores/{token}"),
            Some(user),
            Some(body),
        )
        .await
    }

    async fn local_map(
        &self,
        hash: &str,
        official: Option<i64>,
        set: Option<i64>,
        archive: Option<&str>,
    ) -> i64 {
        sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,max_combo,md5,osu_path,archive_path) VALUES($1,$2,'Identity fixture','Fixture','Fixture',$3,0,'loved',$4,1,2,$5,'fixture.osu',$6) RETURNING id")
            .bind(official).bind(set).bind(if official.is_some() {"Imported"} else {"Local"})
            .bind(if official.is_some() {"official"} else {"local"})
            .bind(hash).bind(archive).fetch_one(&self.state.pool).await.unwrap()
    }

    async fn public_ids(&self, internal: i64) -> (i64, i64) {
        let row = sqlx::query("SELECT native_id,native_set_id FROM beatmaps WHERE id=$1")
            .bind(internal)
            .fetch_one(&self.state.pool)
            .await
            .unwrap();
        (row.get("native_id"), row.get("native_set_id"))
    }

    async fn package_maps(&self, hashes: &[String]) -> Vec<i64> {
        let mut manifest = hashes.to_vec();
        manifest.sort();
        let package: uuid::Uuid = sqlx::query_scalar("INSERT INTO beatmap_packages(kind,member_md5,archive_path,legacy) VALUES('osz',$1,'local.osz',true) RETURNING id").bind(manifest).fetch_one(&self.state.pool).await.unwrap();
        let mut maps = Vec::new();
        for (index, hash) in hashes.iter().enumerate() {
            maps.push(sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,status,stars,md5,osu_path,archive_path) VALUES($1,'Package grouping','Fixture','Fixture',$2,0,'loved',1,$3,'fixture.osu','local.osz') RETURNING id").bind(package).bind(format!("Difficulty {index}")).bind(hash).fetch_one(&self.state.pool).await.unwrap());
        }
        maps
    }

    async fn room_token(&self, map: i64) -> i64 {
        let room: i64 = sqlx::query_scalar(
            "INSERT INTO lazer_rooms(host_id,settings) VALUES($1,'{}') RETURNING id",
        )
        .bind(self.users[0])
        .fetch_one(&self.state.pool)
        .await
        .unwrap();
        let version: i64 = sqlx::query_scalar("INSERT INTO lazer_room_playlist_versions(room_id,playlist_item_id,snapshot) VALUES($1,1,'{}') RETURNING id")
            .bind(room).fetch_one(&self.state.pool).await.unwrap();
        let round: i64 = sqlx::query_scalar("INSERT INTO lazer_room_rounds(room_id,playlist_item_id,playlist_version_id,status) VALUES($1,1,$2,'playing') RETURNING id")
            .bind(room).bind(version).fetch_one(&self.state.pool).await.unwrap();
        sqlx::query("INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods) VALUES($1,$2,$3,$4,0,'[]')")
            .bind(round).bind(self.users[0]).bind(map).bind(HASHES[0])
            .execute(&self.state.pool).await.unwrap();
        sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope,room_id,playlist_item_id,round_id) VALUES($1,$2,0,'original-room-version',now()+interval '1 hour','room',$3,1,$4) RETURNING id")
            .bind(self.users[0]).bind(map).bind(room).bind(round)
            .fetch_one(&self.state.pool).await.unwrap()
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
        // Only remove the unique canonical directory created by this fixture.
        assert_eq!(self.storage.file_name().unwrap(), self.schema.as_str());
        assert!(
            self.storage
                .starts_with(tokio::fs::canonicalize(std::env::temp_dir()).await.unwrap())
        );
        tokio::fs::remove_dir_all(self.storage).await.unwrap();
    }
}

fn submission() -> Value {
    json!({"ruleset_id":0,"passed":true,"total_score":600,"total_score_without_mods":600,
        "accuracy":1.0,"max_combo":2,"rank":"X","mods":[],
        "statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]})
}

fn filename_lookup_path(filename: &str) -> String {
    format!(
        "/api/v2/beatmaps/lookup?{}",
        serde_urlencoded::to_string([("filename", filename)]).unwrap()
    )
}

async fn body(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    response.json().await.unwrap()
}

// Every test owns a fresh schema, servers, and storage; cleanup also runs after
// assertions panic so failures do not leave fixture accounts or maps behind.
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
    lazer_official_archive_uses_complete_set_manifest_and_preserves_native_identity,
    f,
    {
        let primary = f
            .local_map(
                HASHES[0],
                Some(OFFICIAL_ID),
                Some(SET_ID),
                Some("official.osz"),
            )
            .await;
        let sibling = f
            .local_map(HASHES[1], Some(OFFICIAL_ID + 1), Some(SET_ID), None)
            .await;
        let bytes = b"original transport fixture archive with both difficulties and asset bytes";
        let archive = uuid::Uuid::new_v4();
        let archive_directory = format!("official-package-{archive}");
        let archive_path = format!("{archive_directory}/package.osz");
        tokio::fs::create_dir(f.storage.join("maps").join(&archive_directory))
            .await
            .unwrap();
        tokio::fs::write(f.storage.join("maps").join(&archive_path), bytes)
            .await
            .unwrap();
        let mut manifest = vec![HASHES[0].to_string(), HASHES[1].to_string()];
        manifest.sort();
        let mut publication = f.state.pool.begin().await.unwrap();
        sqlx::query("INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(SET_ID).bind(archive).bind(&archive_path).bind("a".repeat(64)).bind(bytes.len() as i64).bind(manifest).bind(f.users[0])
        .execute(&mut *publication).await.unwrap();
        for internal in [primary, sibling] {
            sqlx::query("INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id) VALUES($1,$2)")
            .bind(archive).bind(internal).execute(&mut *publication).await.unwrap();
        }
        publication.commit().await.unwrap();
        let set = body(
            f.get(&format!("/api/v2/beatmapsets/{SET_ID}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(set["id"], SET_ID);
        assert_eq!(set["download_disabled"], false);
        let members = set["beatmaps"].as_array().unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0]["id"], OFFICIAL_ID);
        assert_eq!(members[1]["id"], OFFICIAL_ID + 1);
        let encoded = serde_json::to_string(&set).unwrap();
        for private in [
            archive_path.as_str(),
            "official.osz",
            "member_md5",
            "archive_path",
            "uploaded_by",
        ] {
            assert!(!encoded.contains(private));
        }
        let response = f
            .get(&format!("/api/v2/beatmapsets/{SET_ID}/download"))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/x-osu-beatmap-archive"
        );
        assert_eq!(response.bytes().await.unwrap().as_ref(), bytes);
        assert_eq!(
            f.request(
                Method::GET,
                &format!("/api/v2/beatmapsets/{SET_ID}/download"),
                None,
                None
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let lookup = body(
            f.get(&format!(
                "/api/v2/beatmapsets/lookup?beatmap_id={}",
                OFFICIAL_ID + 1
            ))
            .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(lookup["beatmaps"], set["beatmaps"]);
        let token = body(f.token(OFFICIAL_ID + 1, HASHES[1], 0).await, StatusCode::OK).await;
        let bound: i64 =
            sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1")
                .bind(token["id"].as_i64().unwrap())
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(bound, sibling);
        let error = sqlx::query(
            "DELETE FROM official_beatmapset_archive_members WHERE archive_id=$1 AND beatmap_id=$2",
        )
        .bind(archive)
        .bind(sibling)
        .execute(&f.state.pool)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        let error = sqlx::query("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,max_combo,md5,osu_path) VALUES($1,$2,'Unbound','Fixture','Fixture','Extra',0,'loved','official',1,2,$3,'fixture.osu')")
        .bind(OFFICIAL_ID+2).bind(SET_ID).bind(HASHES[2]).execute(&f.state.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(
            f.get(&format!("/api/v2/beatmapsets/{SET_ID}/download"))
                .await
                .bytes()
                .await
                .unwrap()
                .as_ref(),
            bytes
        );
        assert_eq!(
            tokio::fs::read(f.storage.join("maps").join(&archive_path))
                .await
                .unwrap(),
            bytes
        );
        assert!(f.backend.calls.lock().await.is_empty());
    }
);

fixture_test!(lazer_maps_discover_by_checksum_and_id_and_cache, f, {
    let lookup = body(
        f.get(&format!(
            "/api/v2/beatmaps/lookup?id={OFFICIAL_ID}&checksum={}&filename=Original.osu",
            HASHES[0].to_uppercase()
        ))
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(lookup["id"], OFFICIAL_ID);
    assert_eq!(lookup["beatmapset_id"], SET_ID);
    assert_eq!(lookup["checksum"], HASHES[0]);
    let direct = body(
        f.get(&format!("/api/v2/beatmaps/{}", OFFICIAL_ID + 1))
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(direct["id"], OFFICIAL_ID + 1);
    let set = body(
        f.get(&format!(
            "/api/v2/beatmapsets/lookup?beatmap_id={}",
            OFFICIAL_ID + 2
        ))
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(set["id"], SET_ID + 2);
    assert_eq!(set["beatmaps"][0]["id"], OFFICIAL_ID + 2);
    let scores = body(
        f.get(&format!("/api/v2/beatmaps/{}/scores", OFFICIAL_ID + 3))
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(scores["scores"], json!([]));
    assert_eq!(
        *f.backend.calls.lock().await,
        vec![
            json!({"user_id":f.users[0],"checksum":HASHES[0]}),
            json!({"user_id":f.users[0],"official_id":OFFICIAL_ID + 1}),
            json!({"user_id":f.users[0],"official_id":OFFICIAL_ID + 2}),
            json!({"user_id":f.users[0],"official_id":OFFICIAL_ID + 3}),
        ]
    );
    for path in [
        format!("/api/v2/beatmaps/lookup?checksum={}", HASHES[0]),
        format!("/api/v2/beatmaps/lookup?id={}", OFFICIAL_ID + 1),
        format!("/api/v2/beatmaps/{}", OFFICIAL_ID + 1),
        format!("/api/v2/beatmapsets/lookup?beatmap_id={}", OFFICIAL_ID + 2),
        format!("/api/v2/beatmaps/{}/scores", OFFICIAL_ID + 3),
    ] {
        assert_eq!(f.get(&path).await.status(), StatusCode::OK);
    }
    assert_eq!(f.backend.calls.lock().await.len(), 4);
    let mismatch = body(
        f.get(&format!(
            "/api/v2/beatmaps/lookup?id={}&checksum={}&filename=Original.osu",
            OFFICIAL_ID + 1,
            HASHES[0]
        ))
        .await,
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(mismatch["error"], "beatmap_id_mismatch");
    assert_eq!(f.backend.calls.lock().await.len(), 4);
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM beatmaps ORDER BY id")
        .fetch_all(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(ids.len(), 4);
    assert!(ids.iter().all(|id| *id < OFFICIAL_ID));
});

fixture_test!(
    lazer_filename_aliases_are_exact_and_ambiguity_is_not_discovery,
    f,
    {
        let local = f.local_map(HASHES[0], None, None, Some("local.osz")).await;
        let official = f
            .local_map(HASHES[1], Some(OFFICIAL_ID + 1), Some(SET_ID + 1), None)
            .await;
        let (native, set) = f.public_ids(local).await;
        for name in [FILENAME, "Café.osu"] {
            sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2)")
                .bind(name)
                .bind(local)
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        let value = body(f.get(&filename_lookup_path(FILENAME)).await, StatusCode::OK).await;
        assert_eq!(value["id"], native);
        assert_eq!(value["beatmapset_id"], set);
        assert_eq!(value["checksum"], HASHES[0]);
        assert_eq!(value["status"], "loved");
        assert!(value.get("filename").is_none());
        assert!(value.get("osu_path").is_none());
        assert!(f.backend.calls.lock().await.is_empty());

        // A weaker name cannot replace the immutable checksum/native-ID binding.
        for query in [
            format!("id={native}&checksum={}&filename=../ignored.osu", HASHES[0]),
            format!("id={native}&filename=../ignored.osu"),
        ] {
            assert_eq!(
                f.get(&format!("/api/v2/beatmaps/lookup?{query}"))
                    .await
                    .status(),
                StatusCode::OK
            );
        }
        for malformed in [
            "",
            ".osu",
            "bad.osz",
            "../map.osu",
            "sub/map.osu",
            "sub\\map.osu",
            "C:map.osu",
            "bad\n.osu",
            "bad\u{7f}.osu",
            &format!("{}.osu", "x".repeat(1021)),
        ] {
            let rejected = body(
                f.get(&filename_lookup_path(malformed)).await,
                StatusCode::BAD_REQUEST,
            )
            .await;
            assert_eq!(rejected["error"], "invalid_beatmap_filename");
        }
        assert!(f.backend.calls.lock().await.is_empty());

        f.backend.status.store(404, Ordering::SeqCst);
        for absent in [
            FILENAME.to_ascii_lowercase(),
            "Cafe\u{301}.osu".into(),
            "Other.osu".into(),
        ] {
            let missing = body(
                f.get(&filename_lookup_path(&absent)).await,
                StatusCode::NOT_FOUND,
            )
            .await;
            assert_eq!(missing["error"], "beatmap_not_found");
        }
        assert_eq!(f.backend.calls.lock().await.len(), 3);
        sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2)")
            .bind(FILENAME)
            .bind(official)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let ambiguous = body(
            f.get(&filename_lookup_path(FILENAME)).await,
            StatusCode::CONFLICT,
        )
        .await;
        assert_eq!(ambiguous["error"], "beatmap_filename_ambiguous");
        assert_eq!(
            f.backend.calls.lock().await.len(),
            3,
            "ambiguous local data must not contact discovery"
        );
        assert_eq!(
            f.get(&format!("/api/v2/beatmaps/lookup?checksum={}", HASHES[0]))
                .await
                .status(),
            StatusCode::OK
        );

        // Removing one member cascades its aliases; no official/local precedence
        // was used while both distinct maps still had the same exact name.
        sqlx::query("DELETE FROM beatmaps WHERE id=$1")
            .bind(local)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let remaining = body(f.get(&filename_lookup_path(FILENAME)).await, StatusCode::OK).await;
        assert_eq!(remaining["id"], OFFICIAL_ID + 1);
        assert_eq!(f.backend.calls.lock().await.len(), 3);
        assert_eq!(
            tokio::fs::read(f.storage.join("maps/fixture.osu"))
                .await
                .unwrap(),
            b"original map content"
        );
    }
);

fixture_test!(
    lazer_filename_discovery_validates_persisted_alias_and_error_boundaries,
    f,
    {
        let path = filename_lookup_path(FILENAME);
        assert_eq!(
            f.request(Method::GET, &path, None, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(f.backend.calls.lock().await.is_empty());
        for (status, invalid, expected) in [
            (404, 0, "beatmap_not_found"),
            (503, 0, "map_resolution_unavailable"),
            (429, 0, "map_resolution_rate_limited"),
            (502, 0, "map_resolution_failed"),
            (409, 7, "beatmap_filename_ambiguous"),
            (409, 8, "beatmap_filename_conflict"),
            (409, 9, "beatmap_set_version_conflict"),
        ] {
            f.backend.status.store(status, Ordering::SeqCst);
            f.backend.invalid.store(invalid, Ordering::SeqCst);
            let value = body(f.get(&path).await, StatusCode::from_u16(status).unwrap()).await;
            assert_eq!(value["error"], expected);
        }
        f.backend.status.store(200, Ordering::SeqCst);
        for invalid in [1, 2, 3, 5] {
            f.backend.invalid.store(invalid, Ordering::SeqCst);
            let value = body(f.get(&path).await, StatusCode::BAD_GATEWAY).await;
            assert_eq!(value["error"], "invalid_map_resolution_response");
            sqlx::query("DELETE FROM beatmaps")
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        f.backend.invalid.store(0, Ordering::SeqCst);
        let discovered = body(f.get(&path).await, StatusCode::OK).await;
        assert_eq!(discovered["id"], OFFICIAL_ID);
        assert_eq!(discovered["checksum"], HASHES[0]);
        let calls = f.backend.calls.lock().await.clone();
        assert!(
            calls
                .iter()
                .all(|input| *input == json!({"user_id":f.users[0],"filename":FILENAME}))
        );
        assert_eq!(f.get(&path).await.status(), StatusCode::OK);
        assert_eq!(f.backend.calls.lock().await.len(), calls.len());
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(f.users[0])
            .execute(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(f.get(&path).await.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(f.backend.calls.lock().await.len(), calls.len());
    }
);

fixture_test!(lazer_maps_validate_auth_and_inputs_before_discovery, f, {
    for path in [
        format!("/api/v2/beatmaps/{OFFICIAL_ID}"),
        format!("/api/v2/beatmaps/lookup?checksum={}", HASHES[0]),
        "/api/v2/beatmaps/lookup?filename=Original.osu".to_owned(),
        format!("/api/v2/beatmapsets/lookup?beatmap_id={OFFICIAL_ID}"),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores"),
    ] {
        assert_eq!(
            f.request(Method::GET, &path, None, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for path in [
        "/api/v2/beatmaps/lookup?checksum=invalid".to_owned(),
        "/api/v2/beatmaps/lookup?filename=../Original.osu".to_owned(),
        format!("/api/v2/beatmaps/lookup?id=-1&checksum={}", HASHES[0]),
        "/api/v2/beatmaps/0".to_owned(),
        "/api/v2/beatmapsets/lookup?beatmap_id=-1".to_owned(),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores?mode=invalid"),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores?type=invalid"),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores?mods%5B%5D=INVALID"),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores?variant=invalid"),
        format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores?mode=osu&mode=taiko"),
    ] {
        assert_eq!(
            f.get(&path).await.status(),
            StatusCode::BAD_REQUEST,
            "{path}"
        );
    }
    for (map, hash, ruleset, version) in [
        (OFFICIAL_ID, "invalid", 0, "original-version"),
        (OFFICIAL_ID, HASHES[0], 4, "original-version"),
        (OFFICIAL_ID, HASHES[0], 0, ""),
        (0, HASHES[0], 0, "original-version"),
    ] {
        let response = f
            .request(
                Method::POST,
                &format!("/api/v2/beatmaps/{map}/solo/scores"),
                Some(0),
                Some(json!({"beatmap_hash":hash,"version_hash":version,"ruleset_id":ruleset})),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        f.request(
            Method::POST,
            &format!("/api/v2/beatmaps/{OFFICIAL_ID}/solo/scores"),
            None,
            Some(
                json!({"beatmap_hash":HASHES[0],"version_hash":"original-version","ruleset_id":0})
            )
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
        .bind(f.users[0])
        .execute(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(
        f.token(OFFICIAL_ID, HASHES[0], 0).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(f.backend.calls.lock().await.is_empty());
    assert!(f.backend.ingested.lock().await.is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_score_tokens")
        .fetch_one(&f.state.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
});

fixture_test!(
    lazer_maps_propagate_discovery_failures_and_validate_responses,
    f,
    {
        f.backend.status.store(404, Ordering::SeqCst);
        for path in [
            format!("/api/v2/beatmaps/lookup?checksum={}", HASHES[0]),
            format!("/api/v2/beatmaps/{OFFICIAL_ID}"),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={OFFICIAL_ID}"),
            format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores"),
        ] {
            let value = body(f.get(&path).await, StatusCode::NOT_FOUND).await;
            assert_eq!(value["error"], "beatmap_not_found");
        }
        assert_eq!(
            f.token(OFFICIAL_ID, HASHES[0], 0).await.status(),
            StatusCode::NOT_FOUND
        );
        f.backend.invalid.store(4, Ordering::SeqCst);
        assert_eq!(
            f.get(&format!("/api/v2/beatmaps/{OFFICIAL_ID}"))
                .await
                .status(),
            StatusCode::BAD_GATEWAY
        );
        f.backend.invalid.store(0, Ordering::SeqCst);
        for status in [503, 502, 429, 409] {
            f.backend.status.store(status, Ordering::SeqCst);
            assert_eq!(
                f.get(&format!("/api/v2/beatmaps/{OFFICIAL_ID}"))
                    .await
                    .status()
                    .as_u16(),
                status
            );
            assert_eq!(
                f.token(OFFICIAL_ID, HASHES[0], 0).await.status().as_u16(),
                status
            );
        }
        f.backend.status.store(200, Ordering::SeqCst);
        for invalid in [1, 2, 3] {
            f.backend.invalid.store(invalid, Ordering::SeqCst);
            assert_eq!(
                f.get(&format!("/api/v2/beatmaps/lookup?checksum={}", HASHES[0]))
                    .await
                    .status(),
                StatusCode::BAD_GATEWAY
            );
            sqlx::query("DELETE FROM beatmaps WHERE md5=$1")
                .bind(HASHES[0])
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        assert!(f.backend.ingested.lock().await.is_empty());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_score_tokens")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
);

fixture_test!(
    lazer_maps_prefer_official_difficulty_and_set_namespaces,
    f,
    {
        let collision = f.local_map(HASHES[4], None, None, Some("local.osz")).await;
        let imported = f
            .local_map(HASHES[0], Some(collision), Some(collision), None)
            .await;
        let fallback = f.local_map(HASHES[5], None, None, Some("local.osz")).await;
        f.local_map(HASHES[6], Some(OFFICIAL_ID + 6), Some(SET_ID + 6), None)
            .await;
        assert_ne!(imported, collision);
        let (local_native, local_set) = f.public_ids(collision).await;
        let (fallback_native, fallback_set) = f.public_ids(fallback).await;
        assert_ne!(local_native, collision);
        f.backend.status.store(404, Ordering::SeqCst);
        let local_search = body(
            f.get("/api/v2/beatmapsets/search?q=Local").await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(local_search["total"], 2);
        assert_eq!(local_search["cursor_string"], Value::Null);
        let local_sets = local_search["beatmapsets"].as_array().unwrap();
        assert_eq!(local_sets.len(), 2);
        assert_eq!(local_sets[0]["id"], fallback_set);
        assert_eq!(local_sets[0]["beatmaps"][0]["id"], fallback_native);
        assert_eq!(local_sets[0]["beatmaps"][0]["checksum"], HASHES[5]);
        assert_eq!(local_sets[1]["id"], local_set);
        assert_eq!(local_sets[1]["beatmaps"][0]["checksum"], HASHES[4]);
        let imported_search = body(
            f.get("/api/v2/beatmapsets/search?q=Imported").await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(imported_search["total"], 2);
        let imported_sets = imported_search["beatmapsets"].as_array().unwrap();
        assert_eq!(imported_sets.len(), 2);
        assert_eq!(imported_sets[0]["id"], SET_ID + 6);
        assert_eq!(imported_sets[0]["beatmaps"][0]["id"], OFFICIAL_ID + 6);
        assert_eq!(imported_sets[0]["beatmaps"][0]["checksum"], HASHES[6]);
        assert_eq!(imported_sets[1]["id"], collision);
        assert_eq!(imported_sets[1]["beatmaps"][0]["checksum"], HASHES[0]);
        assert!(
            imported_sets
                .iter()
                .all(|set| set["beatmaps"][0]["version"] == "Imported")
        );
        assert!(f.backend.calls.lock().await.is_empty());
        let direct = body(
            f.get(&format!("/api/v2/beatmaps/{collision}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(direct["checksum"], HASHES[0]);
        assert_eq!(direct["version"], "Imported");
        let lookup = body(
            f.get(&format!("/api/v2/beatmaps/lookup?id={collision}"))
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(lookup["checksum"], HASHES[0]);
        let local = body(
            f.get(&format!("/api/v2/beatmaps/{fallback_native}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(local["id"], fallback_native);
        assert_eq!(local["checksum"], HASHES[5]);
        let collision_local = body(
            f.get(&format!("/api/v2/beatmaps/{local_native}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(collision_local["checksum"], HASHES[4]);
        assert_eq!(collision_local["beatmapset_id"], local_set);
        assert_eq!(
            body(
                f.get(&format!("/api/v2/beatmapsets/{local_set}")).await,
                StatusCode::OK
            )
            .await["beatmaps"][0]["checksum"],
            HASHES[4]
        );
        for path in [
            format!("/api/v2/beatmapsets/lookup?beatmap_id={collision}"),
            format!("/api/v2/beatmapsets/{collision}"),
        ] {
            let set = body(f.get(&path).await, StatusCode::OK).await;
            assert_eq!(set["beatmaps"].as_array().unwrap().len(), 1);
            assert_eq!(set["beatmaps"][0]["checksum"], HASHES[0]);
        }
        assert_eq!(
            f.get(&format!("/api/v2/beatmapsets/{collision}/download"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        sqlx::query("UPDATE beatmaps SET archive_path='official.osz' WHERE id=$1")
            .bind(imported)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let archive = f
            .get(&format!("/api/v2/beatmapsets/{collision}/download"))
            .await;
        assert_eq!(archive.status(), StatusCode::OK);
        assert_eq!(archive.text().await.unwrap(), "original official archive");
        assert!(f.backend.calls.lock().await.is_empty());
        for path in [
            format!("/api/v2/beatmaps/{imported}"),
            format!("/api/v2/beatmaps/lookup?id={imported}"),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={imported}"),
            format!("/api/v2/beatmaps/{imported}/scores"),
        ] {
            assert_eq!(f.get(&path).await.status(), StatusCode::NOT_FOUND);
        }
        assert_eq!(
            *f.backend.calls.lock().await,
            vec![json!({"user_id":f.users[0],"official_id":imported}); 4]
        );
        // Set-only routes are catalogue reads and never treat a set ID as a difficulty.
        assert_eq!(
            f.get(&format!("/api/v2/beatmapsets/{SET_ID}"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.get(&format!("/api/v2/beatmapsets/{SET_ID}/download"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(f.backend.calls.lock().await.len(), 4);
        assert_eq!(
            f.token(imported, HASHES[0], 0).await.status(),
            StatusCode::BAD_REQUEST
        );
        let token = body(f.token(collision, HASHES[0], 0).await, StatusCode::OK).await["id"]
            .as_i64()
            .unwrap();
        let map: i64 = sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1")
            .bind(token)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(map, imported);
        assert_eq!(f.backend.calls.lock().await.len(), 4);
    }
);

fixture_test!(
    lazer_maps_bind_tokens_to_checksum_canonical_id_and_ruleset,
    f,
    {
        let token = body(
            f.token(OFFICIAL_ID, &HASHES[0].to_uppercase(), 0).await,
            StatusCode::OK,
        )
        .await["id"]
            .as_i64()
            .unwrap();
        let row = sqlx::query("SELECT t.*,b.official_id,b.md5 FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1")
        .bind(token).fetch_one(&f.state.pool).await.unwrap();
        let internal: i64 = row.get("beatmap_id");
        assert_ne!(internal, OFFICIAL_ID);
        assert_eq!(row.get::<i64, _>("user_id"), f.users[0]);
        assert_eq!(row.get::<Option<i64>, _>("official_id"), Some(OFFICIAL_ID));
        assert_eq!(row.get::<String, _>("md5"), HASHES[0]);
        assert_eq!(row.get::<String, _>("version_hash"), "original-version");
        assert_eq!(
            *f.backend.calls.lock().await,
            vec![json!({"user_id":f.users[0],"checksum":HASHES[0]})]
        );
        assert_eq!(
            f.token(internal, HASHES[0], 0).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.token(OFFICIAL_ID + 7, HASHES[1], 0).await.status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            f.backend.calls.lock().await[1],
            json!({"user_id":f.users[0],"checksum":HASHES[1]})
        );
        // Osu difficulties support conversion; a native non-osu map cannot change mode.
        assert_eq!(
            f.token(OFFICIAL_ID, HASHES[0], 1).await.status(),
            StatusCode::OK
        );
        f.backend.mode.store(1, Ordering::SeqCst);
        let mismatch = body(
            f.token(OFFICIAL_ID + 2, HASHES[2], 2).await,
            StatusCode::BAD_REQUEST,
        )
        .await;
        assert_eq!(mismatch["error"], "unsupported_ruleset_conversion");
        assert_eq!(f.backend.calls.lock().await.len(), 3);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_score_tokens")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert!(f.backend.ingested.lock().await.is_empty());
    }
);

fixture_test!(lazer_maps_recheck_bearer_after_discovery_wait, f, {
    f.backend.pause.store(true, Ordering::SeqCst);
    for restricted in [false, true] {
        let index = usize::from(restricted);
        let request = f.state.http.post(format!("{}/api/v2/beatmaps/{}/solo/scores", f.base, OFFICIAL_ID + index as i64))
            .bearer_auth(ACCESS[0])
            .json(&json!({"beatmap_hash":HASHES[index],"version_hash":"original-version","ruleset_id":0}));
        let pending = tokio::spawn(async move { request.send().await.unwrap() });
        tokio::time::timeout(Duration::from_secs(2), f.backend.entered.notified())
            .await
            .expect("discovery must reach its original loopback stub");
        if restricted {
            sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
                .bind(f.users[0])
                .execute(&f.state.pool)
                .await
                .unwrap();
        } else {
            sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
                .bind(f.users[0])
                .execute(&f.state.pool)
                .await
                .unwrap();
        }
        f.backend.release.notify_one();
        assert_eq!(pending.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM lazer_score_tokens")
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        if !restricted {
            Fixture::session(&f.state.pool, f.users[0], 0).await;
        }
    }
    assert_eq!(
        *f.backend.calls.lock().await,
        vec![
            json!({"user_id":f.users[0],"checksum":HASHES[0]}),
            json!({"user_id":f.users[0],"checksum":HASHES[1]}),
        ]
    );
    assert!(f.backend.ingested.lock().await.is_empty());
});

fixture_test!(
    lazer_maps_submit_uses_token_storage_and_canonical_wire_identity,
    f,
    {
        sqlx::query("SELECT setval('lazer_score_tokens_id_seq',2147483648,false)")
            .execute(&f.state.pool)
            .await
            .unwrap();
        let token = body(f.token(OFFICIAL_ID, HASHES[0], 0).await, StatusCode::OK).await["id"]
            .as_i64()
            .unwrap();
        assert!(token > i64::from(i32::MAX));
        let internal: i64 =
            sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1")
                .bind(token)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        let room_token = f.room_token(internal).await;
        f.backend.status.store(503, Ordering::SeqCst);
        for (map, attempt, user, status) in [
            (OFFICIAL_ID + 1, token, 0, StatusCode::NOT_FOUND),
            (internal, token, 0, StatusCode::NOT_FOUND),
            (OFFICIAL_ID, token, 1, StatusCode::NOT_FOUND),
            (OFFICIAL_ID, token + 100, 0, StatusCode::NOT_FOUND),
            (OFFICIAL_ID, room_token, 0, StatusCode::NOT_FOUND),
            (0, token, 0, StatusCode::BAD_REQUEST),
            (OFFICIAL_ID, 0, 0, StatusCode::BAD_REQUEST),
        ] {
            assert_eq!(
                f.submit(map, attempt, user, submission()).await.status(),
                status
            );
        }
        let mut wrong_ruleset = submission();
        wrong_ruleset["ruleset_id"] = json!(1);
        assert_eq!(
            f.submit(OFFICIAL_ID, token, 0, wrong_ruleset)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert!(f.backend.ingested.lock().await.is_empty());
        assert_eq!(f.backend.calls.lock().await.len(), 1);
        let mut forged = submission();
        forged["beatmap_id"] = json!(OFFICIAL_ID + 1);
        forged["user_id"] = json!(f.users[1]);
        let submitted = body(
            f.submit(OFFICIAL_ID, token, 0, forged.clone()).await,
            StatusCode::OK,
        )
        .await;
        let score = submitted["id"].as_i64().unwrap();
        assert_eq!(submitted["beatmap_id"], OFFICIAL_ID);
        assert_eq!(submitted["user_id"], f.users[0]);
        assert_eq!(submitted["verification"], "pending");
        let ingested = f.backend.ingested.lock().await;
        assert_eq!(ingested.len(), 1);
        assert_eq!(ingested[0]["beatmap_id"], internal);
        assert_eq!(ingested[0]["user_id"], f.users[0]);
        assert_eq!(ingested[0]["ingestion_key"], format!("lazer:{token}"));
        assert!(ingested[0]["native_submission"].get("beatmap_id").is_none());
        assert!(ingested[0]["native_submission"].get("user_id").is_none());
        drop(ingested);
        let read = body(
            f.get(&format!("/api/v2/scores/{score}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(read["beatmap_id"], OFFICIAL_ID);
        let stored: i64 = sqlx::query_scalar("SELECT beatmap_id FROM scores WHERE id=$1")
            .bind(score)
            .fetch_one(&f.state.pool)
            .await
            .unwrap();
        assert_eq!(stored, internal);
        let empty = body(
            f.get(&format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores"))
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(empty["scores"], json!([]));
        sqlx::query("UPDATE scores SET verification='verified',standardised_score=600,classic_score=600,score_scale_status='native_reported' WHERE id=$1")
        .bind(score).execute(&f.state.pool).await.unwrap();
        let board = body(
            f.get(&format!("/api/v2/beatmaps/{OFFICIAL_ID}/scores"))
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(board["scores"][0]["id"], score);
        assert_eq!(board["scores"][0]["beatmap_id"], OFFICIAL_ID);
        assert_eq!(board["user_score"]["score"]["id"], score);
        assert_eq!(
            f.submit(OFFICIAL_ID, token, 0, forged.clone())
                .await
                .status(),
            StatusCode::OK
        );
        let mut changed = forged.clone();
        changed["total_score"] = json!(601);
        assert_eq!(
            f.submit(OFFICIAL_ID, token, 0, changed).await.status(),
            StatusCode::CONFLICT
        );
        let expired = body(f.token(OFFICIAL_ID, HASHES[0], 0).await, StatusCode::OK).await["id"]
            .as_i64()
            .unwrap();
        sqlx::query(
            "UPDATE lazer_score_tokens SET expires_at=now()-interval '1 hour' WHERE id=ANY($1)",
        )
        .bind(vec![token, expired])
        .execute(&f.state.pool)
        .await
        .unwrap();
        assert_eq!(
            f.submit(OFFICIAL_ID, expired, 0, submission())
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            f.submit(OFFICIAL_ID, token, 0, forged).await.status(),
            StatusCode::OK
        );
        assert_eq!(f.backend.ingested.lock().await.len(), 1);
        assert_eq!(f.backend.calls.lock().await.len(), 1);
    }
);

fixture_test!(
    lazer_local_package_groups_all_difficulties_and_uses_canonical_tokens,
    f,
    {
        // Exceed the former 256-row response limit without exceeding upload's 512
        // entries, so a complete immutable package cannot be silently truncated.
        let hashes: Vec<String> = (0..300).map(|n| format!("a{n:031x}")).collect();
        let maps = f.package_maps(&hashes).await;
        let (native, set) = f.public_ids(maps[0]).await;
        let (sibling, sibling_set) = f.public_ids(maps[299]).await;
        assert_ne!(native, maps[0]);
        assert_ne!(native, sibling);
        assert_eq!(set, sibling_set);
        let details = body(
            f.get(&format!("/api/v2/beatmapsets/{set}")).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(details["beatmaps"].as_array().unwrap().len(), 300);
        assert_eq!(details["beatmaps"][299]["id"], sibling);
        assert_eq!(details["beatmaps"][299]["checksum"], hashes[299]);
        let lookup = body(
            f.get(&format!("/api/v2/beatmapsets/lookup?beatmap_id={sibling}"))
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(lookup["id"], set);
        assert_eq!(lookup["beatmaps"].as_array().unwrap().len(), 300);
        let search = body(
            f.get("/api/v2/beatmapsets/search?q=Package%20grouping")
                .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(search["total"], 1);
        assert_eq!(search["beatmapsets"].as_array().unwrap().len(), 1);
        assert_eq!(search["beatmapsets"][0]["id"], set);
        let archive = f.get(&format!("/api/v2/beatmapsets/{set}/download")).await;
        assert_eq!(archive.status(), StatusCode::OK);
        assert_eq!(
            archive.bytes().await.unwrap().as_ref(),
            b"original local archive"
        );
        let token = body(f.token(native, &hashes[0], 0).await, StatusCode::OK).await["id"]
            .as_i64()
            .unwrap();
        let bound: i64 =
            sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1")
                .bind(token)
                .fetch_one(&f.state.pool)
                .await
                .unwrap();
        assert_eq!(bound, maps[0]);
        assert_eq!(
            f.submit(maps[0], token, 0, submission()).await.status(),
            StatusCode::NOT_FOUND
        );
        let score = body(
            f.submit(native, token, 0, submission()).await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(score["beatmap_id"], native);
        assert_eq!(f.backend.ingested.lock().await[0]["beatmap_id"], maps[0]);
        assert!(f.backend.calls.lock().await.is_empty());
    }
);

fixture_test!(
    lazer_reserved_native_misses_and_overflow_never_discover,
    f,
    {
        let reserved = i64::from(i32::MAX);
        for path in [
            format!("/api/v2/beatmaps/{reserved}"),
            format!(
                "/api/v2/beatmaps/lookup?id={reserved}&checksum={}",
                HASHES[0]
            ),
            format!("/api/v2/beatmapsets/lookup?beatmap_id={reserved}"),
            format!("/api/v2/beatmaps/{reserved}/scores"),
        ] {
            assert_eq!(f.get(&path).await.status(), StatusCode::NOT_FOUND);
        }
        assert_eq!(
            f.token(reserved, HASHES[0], 0).await.status(),
            StatusCode::NOT_FOUND
        );
        for invalid in [i64::from(i32::MAX) + 1, i64::MAX] {
            for path in [
                format!("/api/v2/beatmaps/{invalid}"),
                format!("/api/v2/beatmaps/lookup?id={invalid}"),
                format!("/api/v2/beatmapsets/{invalid}"),
                format!("/api/v2/beatmapsets/{invalid}/download"),
            ] {
                assert_eq!(f.get(&path).await.status(), StatusCode::BAD_REQUEST);
            }
            assert_eq!(
                f.token(invalid, HASHES[0], 0).await.status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert!(f.backend.calls.lock().await.is_empty());
        assert!(f.backend.ingested.lock().await.is_empty());
    }
);
