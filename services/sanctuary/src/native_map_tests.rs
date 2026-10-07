//! Resolver checks never contact osu!. Database checks use an isolated PG17 schema.
//! Run with ILLYA_NATIVE_MAP_TEST_DATABASE_URL=<owned disposable database>
//! cargo test -p sanctuary native_map_resolution_imports_cache_and_integrity -- --ignored
use super::*;
use axum::{
    body::{Body, to_bytes},
    extract::{Path, Query},
    http::{HeaderMap, Request},
};
use sqlx::{Row, postgres::PgConnectOptions};
use std::{
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

const SERVICE_KEY: &str = "native-map-fixture-service-key-over-thirty-two-bytes";
const RESOLVE_PATH: &str = "/api/v1/internal/beatmaps/resolve";

fn fixture_state(db: PgPool, root: PathBuf, origin: String, configured: bool) -> AppState {
    AppState {
        db,
        secret: Arc::new("native-map-fixture-session-key-over-thirty-two-bytes".into()),
        internal_key: Arc::new(SERVICE_KEY.into()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: root.join("media"),
        maps_root: root.join("maps"),
        replay_root: root.join("replays"),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        official: Arc::new(Mutex::new(maps::OfficialState::fixture(origin, configured))),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    }
}

async fn raw_request(app: &Router, key: Option<&str>, body: String) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(RESOLVE_PATH)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        builder = builder.header("x-illya-service-key", key);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn resolve(app: &Router, body: Value) -> (StatusCode, Value) {
    raw_request(app, Some(SERVICE_KEY), body.to_string()).await
}

async fn manual_import(app: &Router, token: Option<&str>, official_id: i64) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/beatmaps/import")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(Body::from(json!({"official_id":official_id}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn bearer_session(state: &AppState, user_id: i64) -> String {
    let sid = Uuid::new_v4();
    let expiry = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(user_id)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":user_id,"sid":sid,"exp":expiry.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap()
}

#[tokio::test]
async fn native_map_resolution_request_guards() {
    // The lazy pool points at a closed port. A DB access would exceed this timeout.
    let state = fixture_state(
        PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy("postgres://fixture@127.0.0.1:1/fixture")
            .unwrap(),
        std::env::temp_dir(),
        "http://127.0.0.1:1".into(),
        false,
    );
    let app = router(state.clone());
    let valid = json!({"user_id":1,"official_id":100});
    let wrong_length_matched_key = "x".repeat(SERVICE_KEY.len());
    for key in [
        None,
        Some(""),
        Some("incorrect"),
        Some(wrong_length_matched_key.as_str()),
    ] {
        let (status, value) = tokio::time::timeout(
            Duration::from_secs(1),
            raw_request(&app, key, valid.to_string()),
        )
        .await
        .expect("missing and incorrect service keys must not query the database");
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(value["error"]["code"], "unauthorized");
    }
    let too_short_key = "s".repeat(31);
    for expected in ["", "short-fixture-key", too_short_key.as_str()] {
        let mut short = state.clone();
        short.internal_key = Arc::new(expected.into());
        let (status, value) = tokio::time::timeout(
            Duration::from_secs(1),
            raw_request(&router(short), Some(expected), valid.to_string()),
        )
        .await
        .expect("an unconfigured or short service key must not query the database");
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
    }
    let minimum_key = "s".repeat(32);
    let mut minimum = state.clone();
    minimum.internal_key = Arc::new(minimum_key.clone());
    let (status, value) = tokio::time::timeout(
        Duration::from_secs(1),
        raw_request(
            &router(minimum),
            Some(&minimum_key),
            json!({"user_id":0,"official_id":100}).to_string(),
        ),
    )
    .await
    .expect("a 32-byte matching service key must pass to input validation without DB access");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");

    // Shape, selectors, values, and duplicate fields are rejected before DB access.
    for body in [
        json!({}),
        json!({"user_id":1}),
        json!({"user_id":0,"official_id":100}),
        json!({"user_id":-1,"official_id":100}),
        json!({"user_id":1,"official_id":0}),
        json!({"user_id":1,"official_id":-1}),
        json!({"user_id":1,"checksum":""}),
        json!({"user_id":1,"checksum":"a".repeat(31)}),
        json!({"user_id":1,"checksum":"g".repeat(32)}),
        json!({"user_id":1,"checksum":" aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
        json!({"user_id":1,"checksum":"a".repeat(32),"official_id":100}),
        json!({"user_id":1,"official_id":100,"url":"http://127.0.0.1/"}),
        json!({"user_id":"1","official_id":100}),
        json!({"user_id":1,"official_id":"100"}),
        json!({"user_id":1,"checksum":null}),
        json!({"user_id":1,"official_id":null}),
        json!({"user_id":1,"checksum":null,"official_id":100}),
        json!({"user_id":1,"official_id":null,"checksum":"a".repeat(32)}),
        json!({"user_id":1.5,"official_id":100}),
        json!({"user_id":1,"official_id":100.5}),
        json!({"user_id":1,"filename":null}),
        json!({"user_id":1,"filename":42}),
        json!({"user_id":1,"filename":""}),
        json!({"user_id":1,"filename":".osu"}),
        json!({"user_id":1,"filename":"../map.osu"}),
        json!({"user_id":1,"filename":"map\\name.osu"}),
        json!({"user_id":1,"filename":"C:map.osu"}),
        json!({"user_id":1,"filename":"map\u{0}.osu"}),
        json!({"user_id":1,"filename":"map.osu "}),
        json!({"user_id":1,"filename":format!("{}.osu","x".repeat(1021))}),
        json!({"user_id":1,"filename":"map.osu","checksum":"a".repeat(32)}),
        json!({"user_id":1,"filename":"map.osu","official_id":100}),
        json!({"user_id":1,"filename":"map.osu","checksum":null}),
        json!({"user_id":1,"filename":"map.osu","official_id":null}),
    ] {
        let (status, value) =
            tokio::time::timeout(Duration::from_secs(1), resolve(&app, body.clone()))
                .await
                .expect("invalid resolver input must not query the database");
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "{body}: {status} {value}"
        );
    }
    for body in [
        r#"{"user_id":1,"official_id":100,"official_id":101}"#,
        r#"{"user_id":1,"filename":"a.osu","filename":"b.osu"}"#,
    ] {
        let (status, value) = tokio::time::timeout(
            Duration::from_secs(1),
            raw_request(&app, Some(SERVICE_KEY), body.into()),
        )
        .await
        .expect("duplicate selectors must not query the database");
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{value}");
    }
    let mut boundary_body = json!({"user_id":0,"filename":"a.osu"}).to_string();
    boundary_body.extend(std::iter::repeat_n(' ', 4 * 1024 - boundary_body.len()));
    let (status, value) = tokio::time::timeout(
        Duration::from_secs(1),
        raw_request(&app, Some(SERVICE_KEY), boundary_body.clone()),
    )
    .await
    .expect("a 4 KiB body must pass to input validation without DB access");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{value}");
    boundary_body.push(' ');
    let (status, _) = raw_request(&app, Some(SERVICE_KEY), boundary_body).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

fn osu_bytes(id: i64, set_id: i64, mode: i16, title: &str) -> Vec<u8> {
    format!("osu file format v14\n[General]\nMode:{mode}\n[Metadata]\nTitle:{title}\nArtist:Illyaverse\nCreator:Resolver fixture\nVersion:Easy\nBeatmapID:{id}\nBeatmapSetID:{set_id}\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n").into_bytes()
}

#[derive(Clone)]
pub(crate) struct OfficialMap {
    metadata: Value,
    bytes: Vec<u8>,
}
impl OfficialMap {
    pub(crate) fn new(id: i64, status: &str) -> Self {
        let bytes = osu_bytes(id, id + 1000, 0, &format!("Fixture {id}"));
        Self {
            metadata: json!({"id":id,"beatmapset_id":id+1000,"checksum":format!("{:x}",md5::compute(&bytes)),"mode_int":0,"status":status}),
            bytes,
        }
    }
    pub(crate) fn checksum(&self) -> String {
        self.metadata["checksum"].as_str().unwrap().into()
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(crate) fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = metadata;
        self
    }
    pub(crate) fn with_bytes(mut self, bytes: Vec<u8>) -> Self {
        self.bytes = bytes;
        self
    }
}

#[derive(Default)]
struct Counters {
    oauth: AtomicUsize,
    lookup: AtomicUsize,
    detail: AtomicUsize,
    osu: AtomicUsize,
    set: AtomicUsize,
}
impl Counters {
    fn snapshot(&self) -> [usize; 4] {
        [&self.oauth, &self.lookup, &self.detail, &self.osu].map(|v| v.load(Ordering::SeqCst))
    }
}
struct FixtureData {
    maps: HashMap<i64, OfficialMap>,
    lookup_routes: HashMap<String, i64>,
    filename_routes: HashMap<String, i64>,
    lookup_filenames: Vec<String>,
    oauth_status: Option<StatusCode>,
    lookup_status: Option<StatusCode>,
    lookup_rate_limits: usize,
    lookup_retry_after: String,
    lookup_pause: Option<Arc<LookupPause>>,
    sets: HashMap<i64, Value>,
    set_queries: Vec<i64>,
    set_rate_limits: usize,
    set_retry_after: String,
    set_pause: Option<Arc<LookupPause>>,
}
impl Default for FixtureData {
    fn default() -> Self {
        Self {
            maps: Default::default(),
            lookup_routes: Default::default(),
            filename_routes: Default::default(),
            lookup_filenames: Default::default(),
            oauth_status: None,
            lookup_status: None,
            lookup_rate_limits: 0,
            lookup_retry_after: "1".into(),
            lookup_pause: None,
            sets: Default::default(),
            set_queries: Default::default(),
            set_rate_limits: 0,
            set_retry_after: "1".into(),
            set_pause: None,
        }
    }
}
#[derive(Default)]
pub(crate) struct LookupPause {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl LookupPause {
    pub(crate) async fn wait_entered(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.notified())
            .await
            .expect("upstream filename lookup must reach the fixture pause");
    }
    pub(crate) fn release(&self) {
        self.release.notify_one();
    }
}
#[derive(Clone, Default)]
struct FixtureState {
    counters: Arc<Counters>,
    data: Arc<Mutex<FixtureData>>,
}
pub(crate) struct OfficialServer {
    pub(crate) origin: String,
    state: FixtureState,
    task: tokio::task::JoinHandle<()>,
}
impl OfficialServer {
    pub(crate) async fn start() -> Self {
        let state = FixtureState::default();
        let app = Router::new()
            .route("/oauth/token", post(fixture_oauth))
            .route("/api/v2/beatmaps/lookup", get(fixture_lookup))
            .route("/api/v2/beatmaps/{id}", get(fixture_detail))
            .route("/api/v2/beatmapsets/{id}", get(fixture_set))
            .route("/osu/{id}", get(fixture_osu))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            state,
            task,
        }
    }
    pub(crate) async fn add(&self, map: OfficialMap) {
        let mut data = self.state.data.lock().await;
        let id = map.metadata["id"].as_i64().unwrap();
        data.lookup_routes.insert(map.checksum(), id);
        data.maps.insert(id, map);
    }
    pub(crate) async fn add_filename(&self, filename: &str, map: OfficialMap) {
        let mut data = self.state.data.lock().await;
        let id = map.metadata["id"].as_i64().unwrap();
        data.filename_routes.insert(filename.to_owned(), id);
        data.lookup_routes.insert(map.checksum(), id);
        data.maps.insert(id, map);
    }
    pub(crate) async fn lookup_filenames(&self) -> Vec<String> {
        self.state.data.lock().await.lookup_filenames.clone()
    }
    pub(crate) async fn pause_next_lookup(&self) -> Arc<LookupPause> {
        let pause = Arc::new(LookupPause::default());
        self.state.data.lock().await.lookup_pause = Some(pause.clone());
        pause
    }
    pub(crate) async fn add_set(&self, set_id: i64, metadata: Value) {
        self.state.data.lock().await.sets.insert(set_id, metadata);
    }
    pub(crate) async fn set_queries(&self) -> Vec<i64> {
        self.state.data.lock().await.set_queries.clone()
    }
    pub(crate) async fn set_rate_limits(&self, count: usize, retry_after: &str) {
        let mut data = self.state.data.lock().await;
        data.set_rate_limits = count;
        data.set_retry_after = retry_after.into();
    }
    pub(crate) async fn pause_next_set(&self) -> Arc<LookupPause> {
        let pause = Arc::new(LookupPause::default());
        self.state.data.lock().await.set_pause = Some(pause.clone());
        pause
    }
    pub(crate) fn counters(&self) -> [usize; 4] {
        self.state.counters.snapshot()
    }
}
impl Drop for OfficialServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture_oauth(State(state): State<FixtureState>, Json(body): Json<Value>) -> Response {
    state.counters.oauth.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["grant_type"], "client_credentials");
    assert_eq!(body["scope"], "public");
    assert!(body["client_id"].is_string() || body["client_id"].is_number());
    assert!(
        body["client_secret"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    if let Some(status) = state.data.lock().await.oauth_status {
        return status.into_response();
    }
    Json(json!({"access_token":"native-map-fixture-token","token_type":"Bearer","expires_in":3600}))
        .into_response()
}
fn check_bearer(headers: &HeaderMap) {
    assert_eq!(
        headers[header::AUTHORIZATION],
        "Bearer native-map-fixture-token"
    );
}
async fn fixture_lookup(
    State(state): State<FixtureState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    state.counters.lookup.fetch_add(1, Ordering::SeqCst);
    check_bearer(&headers);
    assert_eq!(query.len(), 1);
    let checksum = query.get("checksum");
    let filename = query.get("filename");
    assert_ne!(
        checksum.is_some(),
        filename.is_some(),
        "lookup requires exactly one selector"
    );
    if let Some(checksum) = checksum {
        assert_eq!(checksum, &checksum.to_ascii_lowercase());
    }
    let pause = {
        let mut data = state.data.lock().await;
        if let Some(filename) = filename {
            data.lookup_filenames.push(filename.clone());
        }
        data.lookup_pause.take()
    };
    if let Some(pause) = pause {
        pause.entered.notify_one();
        pause.release.notified().await;
    }
    let mut data = state.data.lock().await;
    if data.lookup_rate_limits > 0 {
        data.lookup_rate_limits -= 1;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, data.lookup_retry_after.clone())],
        )
            .into_response();
    }
    if let Some(status) = data.lookup_status {
        return status.into_response();
    }
    let map_id = match (checksum, filename) {
        (Some(checksum), None) => data.lookup_routes.get(checksum),
        (None, Some(filename)) => data.filename_routes.get(filename),
        _ => unreachable!("selector shape is checked above"),
    };
    match map_id.and_then(|id| data.maps.get(id)) {
        Some(map) => Json(map.metadata.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn fixture_detail(
    State(state): State<FixtureState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    state.counters.detail.fetch_add(1, Ordering::SeqCst);
    check_bearer(&headers);
    match state.data.lock().await.maps.get(&id) {
        Some(map) => Json(map.metadata.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn fixture_set(
    State(state): State<FixtureState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    state.counters.set.fetch_add(1, Ordering::SeqCst);
    check_bearer(&headers);
    let pause = {
        let mut data = state.data.lock().await;
        data.set_queries.push(id);
        data.set_pause.take()
    };
    if let Some(pause) = pause {
        pause.entered.notify_one();
        pause.release.notified().await;
    }
    let mut data = state.data.lock().await;
    if data.set_rate_limits > 0 {
        data.set_rate_limits -= 1;
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, data.set_retry_after.clone())],
        )
            .into_response();
    }
    match data.sets.get(&id) {
        Some(set) => Json(set.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
async fn fixture_osu(State(state): State<FixtureState>, Path(id): Path<i64>) -> Response {
    state.counters.osu.fetch_add(1, Ordering::SeqCst);
    match state.data.lock().await.maps.get(&id) {
        Some(map) => ([(header::CONTENT_TYPE, "text/plain")], map.bytes.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn player(db: &PgPool, name: &str, restricted: bool) -> i64 {
    let id = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash,is_restricted) VALUES($1,$2,'fixture','fixture',$3) RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).bind(restricted).fetch_one(db).await.unwrap();
    sqlx::query(
        "INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'",
    )
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn local_map(db: &PgPool, user: i64, checksum: &str, official_id: Option<i64>) -> i64 {
    sqlx::query_scalar("INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,status,source,stars,md5,osu_path,official_id,official_set_id) VALUES($1,'Private fixture','Fixture','Fixture','Easy',0,'pending','local',0,$2,'private-fixture.osu',$3,$4) RETURNING id")
        .bind(user).bind(checksum).bind(official_id).bind(official_id.map(|id| id+1000)).fetch_one(db).await.unwrap()
}

async fn beatmap_count(db: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM beatmaps")
        .fetch_one(db)
        .await
        .unwrap()
}

async fn reset_official(state: &AppState, server: &OfficialServer, configured: bool) -> Router {
    *state.official.lock().await = maps::OfficialState::fixture(server.origin.clone(), configured);
    router(state.clone())
}

async fn wait_for_queued_resolution(db: &PgPool, application: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let queued: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND state='idle' AND query LIKE '%jsonb_build_object(''id'',id%')")
                .bind(application).fetch_one(db).await.unwrap();
            if queued {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queued resolver must finish its initial active-user and stored-map reads");
}

async fn assert_resolved(
    db: &PgPool,
    value: &Value,
    checksum: &str,
    official_id: Option<i64>,
    status: &str,
    source: &str,
) {
    let keys = value
        .as_object()
        .expect("resolver success must be an object");
    assert_eq!(
        keys.len(),
        9,
        "resolver must only return native beatmap identity"
    );
    for key in [
        "id",
        "native_id",
        "native_set_id",
        "official_id",
        "official_set_id",
        "md5",
        "status",
        "source",
        "mode",
    ] {
        assert!(keys.contains_key(key), "missing {key}: {value}");
    }
    assert!(value["id"].as_i64().is_some_and(|id| id > 0));
    assert_eq!(value["official_id"], json!(official_id));
    for (native_key, official_key) in [
        ("native_id", "official_id"),
        ("native_set_id", "official_set_id"),
    ] {
        let native = value[native_key]
            .as_i64()
            .unwrap_or_else(|| panic!("{native_key} must be a signed integer: {value}"));
        assert!((1..=i64::from(i32::MAX)).contains(&native));
        if let Some(official) = value[official_key].as_i64() {
            assert!((1..=1_999_999_999).contains(&official));
            assert_eq!(
                native, official,
                "canonical {native_key} must use its official ID"
            );
        } else {
            assert!((2_000_000_000..=i64::from(i32::MAX)).contains(&native));
        }
    }
    let canonical: (i64, i64) =
        sqlx::query_as("SELECT native_id,native_set_id FROM beatmaps WHERE id=$1")
            .bind(value["id"].as_i64().unwrap())
            .fetch_one(db)
            .await
            .unwrap();
    assert_eq!(value["native_id"], canonical.0);
    assert_eq!(value["native_set_id"], canonical.1);
    for private_key in [
        "osu_path",
        "archive_path",
        "uploaded_by",
        "local_package_id",
    ] {
        assert!(
            !keys.contains_key(private_key),
            "private field {private_key}: {value}"
        );
    }
    assert_eq!(value["md5"], checksum);
    assert_eq!(value["status"], status);
    assert_eq!(value["source"], source);
    assert_eq!(value["mode"], 0);
}

#[tokio::test]
#[ignore = "requires ILLYA_NATIVE_MAP_TEST_DATABASE_URL pointing to an owned disposable PostgreSQL 17"]
async fn native_map_resolution_imports_cache_and_integrity() {
    let url = env::var("ILLYA_NATIVE_MAP_TEST_DATABASE_URL")
        .expect("Set the disposable native-map fixture URL");
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
        "native-map fixture must run PostgreSQL 17"
    );
    let suffix = Uuid::new_v4().simple().to_string();
    let schema = format!("illya_native_maps_{suffix}");
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
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let queued_application = format!("{schema}_queued");
    let queued_db = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.application_name(&queued_application))
        .await
        .unwrap();
    let root = std::env::temp_dir().join(format!("illyaverse-native-maps-{suffix}"));
    assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
    let pool = db.clone();
    let queued_pool = queued_db.clone();
    let test_root = root.clone();
    let outcome = tokio::spawn(async move {
        let server = OfficialServer::start().await;
        let state = fixture_state(pool.clone(), test_root, server.origin.clone(), true);
        tokio::fs::create_dir_all(&state.maps_root).await.unwrap();
        let app = router(state.clone());
        let user = player(&pool, "NativePlayer", false).await;
        let restricted = player(&pool, "NativeRestricted", true).await;
        let before = server.counters();
        for rejected in [restricted, i64::MAX] {
            let (status, value) =
                resolve(&app, json!({"user_id":rejected,"official_id":100})).await;
            assert!(
                matches!(
                    status,
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
                ),
                "{value}"
            );
        }
        assert_eq!(
            server.counters(),
            before,
            "invalid users must not contact upstream"
        );
        assert_eq!(beatmap_count(&pool).await, 0);

        // Manual import still requires a valid session and the import permission.
        let player_token = bearer_session(&state, user).await;
        let (status, value) = manual_import(&app, None, 100).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(value["error"]["code"], "unauthorized");
        let (status, value) = manual_import(&app, Some(&player_token), 100).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{value}");
        assert_eq!(value["error"]["code"], "forbidden");
        assert_eq!(server.counters(), before, "denied manual imports must not contact upstream");

        // A regular player can import by uppercase checksum, using actual file bytes and API status.
        let map = OfficialMap::new(100, "approved");
        let checksum = map.checksum();
        let bytes = map.bytes.clone();
        server.add(map).await;
        let (status, imported) = resolve(
            &app,
            json!({"user_id":user,"checksum":checksum.to_ascii_uppercase()}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{imported}");
        assert_resolved(&pool, &imported, &checksum, Some(100), "ranked", "official").await;
        assert_eq!(imported["official_set_id"], 1100);
        let row = sqlx::query("SELECT uploaded_by,osu_path FROM beatmaps WHERE id=$1")
            .bind(imported["id"].as_i64().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<Option<i64>, _>("uploaded_by"), Some(user));
        let path: String = row.get("osu_path");
        assert!(!PathBuf::from(&path).is_absolute() && !path.contains(".."));
        assert_eq!(
            tokio::fs::read(state.maps_root.join(&path)).await.unwrap(),
            bytes
        );
        assert_eq!(server.counters(), [1, 1, 0, 1]);
        let before = server.counters();
        for selector in [json!({"checksum":checksum}), json!({"official_id":100})] {
            let mut body = selector;
            body["user_id"] = json!(user);
            assert_eq!(
                resolve(&app, body).await,
                (StatusCode::OK, imported.clone())
            );
        }
        assert_eq!(
            server.counters(),
            before,
            "stored identities must not reimport"
        );

        // A private local row, including its status and source, is returned unchanged.
        let private_md5 = "1".repeat(32);
        let private_id = local_map(&pool, user, &private_md5, Some(101)).await;
        for body in [
            json!({"user_id":user,"checksum":private_md5}),
            json!({"user_id":user,"official_id":101}),
        ] {
            let (status, value) = resolve(&app, body).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_resolved(&pool, &value, &private_md5, Some(101), "pending", "local").await;
            assert_eq!(value["id"], private_id);
        }
        assert_eq!(server.counters(), before);

        // Same checksum callers share one fetch; ID/hash callers converge on one row.
        let concurrent = OfficialMap::new(102, "loved");
        let concurrent_md5 = concurrent.checksum();
        server.add(concurrent).await;
        let before = server.counters();
        let mut requests = Vec::new();
        for _ in 0..8 {
            let app = app.clone();
            let body = json!({"user_id":user,"checksum":concurrent_md5});
            requests.push(tokio::spawn(async move { resolve(&app, body).await }));
        }
        let mut concurrent_id = None;
        for request in requests {
            let (status, value) = request.await.unwrap();
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_resolved(&pool, &value, &concurrent_md5, Some(102), "loved", "official").await;
            assert_eq!(
                *concurrent_id.get_or_insert(value["id"].clone()),
                value["id"]
            );
        }
        let after = server.counters();
        assert_eq!(after[1] - before[1], 1);
        assert_eq!(after[3] - before[3], 1);
        let mixed = OfficialMap::new(103, "ranked");
        let mixed_md5 = mixed.checksum();
        server.add(mixed).await;
        let (by_id, by_hash) = tokio::join!(
            resolve(&app, json!({"user_id":user,"official_id":103})),
            resolve(&app, json!({"user_id":user,"checksum":mixed_md5})),
        );
        assert_eq!(by_id.0, StatusCode::OK, "{}", by_id.1);
        assert_eq!(by_hash.0, StatusCode::OK, "{}", by_hash.1);
        assert_eq!(by_id.1, by_hash.1);
        let rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM beatmaps WHERE official_id=103 OR md5=$1")
                .bind(&mixed_md5)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows, 1);

        // A caller restricted after its initial reads must be denied after another import finishes.
        // The paused lookup and the queued pool's last SQL statement establish the order.
        let queued_user = player(&pool, "NativeQueued", false).await;
        let queued_map = OfficialMap::new(109, "ranked");
        let queued_md5 = queued_map.checksum();
        server.add(queued_map).await;
        let pause = Arc::new(LookupPause::default());
        server.state.data.lock().await.lookup_pause = Some(pause.clone());
        let importer_app = app.clone();
        let importer_body = json!({"user_id":user,"checksum":queued_md5});
        let importer = tokio::spawn(async move { resolve(&importer_app, importer_body).await });
        tokio::time::timeout(Duration::from_secs(5), pause.entered.notified())
            .await
            .expect("the first resolver must reach the paused upstream lookup");
        let mut queued_state = state.clone();
        queued_state.db = queued_pool.clone();
        let queued_app = router(queued_state);
        let queued_body = json!({"user_id":queued_user,"checksum":queued_md5});
        let queued = tokio::spawn(async move { resolve(&queued_app, queued_body).await });
        wait_for_queued_resolution(&pool, &queued_application).await;
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(queued_user)
            .execute(&pool)
            .await
            .unwrap();
        server.state.data.lock().await.lookup_pause = None;
        pause.release.notify_one();
        let (status, value) = tokio::time::timeout(Duration::from_secs(5), importer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{value}");
        let (status, value) = tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{value}");
        assert_eq!(value["error"]["code"], "unauthorized");

        // A cached upstream 404 is overridden by a subsequently inserted local row.
        let missing_md5 = "2".repeat(32);
        let before = server.counters();
        for _ in 0..2 {
            let (status, value) =
                resolve(&app, json!({"user_id":user,"checksum":missing_md5})).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{value}");
            assert_eq!(value["error"]["code"], "beatmap_not_found");
        }
        assert_eq!(server.counters()[1] - before[1], 1);
        let missing_local_id = local_map(&pool, user, &missing_md5, None).await;
        let before = server.counters();
        let (status, value) = resolve(&app, json!({"user_id":user,"checksum":missing_md5})).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_resolved(&pool, &value, &missing_md5, None, "pending", "local").await;
        assert_eq!(value["id"], missing_local_id);
        assert_eq!(server.counters(), before);

        // Missing configuration and transient failures never become not-found results.
        let app = reset_official(&state, &server, false).await;
        let before = server.counters();
        let (status, value) = resolve(&app, json!({"user_id":user,"official_id":999})).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
        assert_eq!(value["error"]["code"], "official_api_not_configured");
        assert_eq!(server.counters(), before);

        // Invalid OAuth credentials produce an operational failure shared across selectors.
        let app = reset_official(&state, &server, true).await;
        server.state.data.lock().await.oauth_status = Some(StatusCode::UNAUTHORIZED);
        let before = server.counters();
        for body in [
            json!({"user_id":user,"checksum":"6".repeat(32)}),
            json!({"user_id":user,"official_id":998}),
        ] {
            let (status, value) = resolve(&app, body).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
            assert_eq!(value["error"]["code"], "official_auth_failed");
        }
        assert_eq!(
            server.counters(),
            [before[0] + 1, before[1], before[2], before[3]],
            "different selectors must reuse the global OAuth failure cache"
        );
        server.state.data.lock().await.oauth_status = None;

        let app = reset_official(&state, &server, true).await;
        server.state.data.lock().await.lookup_status = Some(StatusCode::INTERNAL_SERVER_ERROR);
        let before = server.counters();
        for _ in 0..2 {
            let (status, value) =
                resolve(&app, json!({"user_id":user,"checksum":"3".repeat(32)})).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
            assert_eq!(value["error"]["code"], "official_api_failed");
        }
        assert_eq!(
            server.counters()[1] - before[1],
            1,
            "transient failure must be cached"
        );
        server.state.data.lock().await.lookup_status = None;

        // A long Retry-After returns promptly and blocks same-key and different-key requests.
        // This tests the real 60-second deadline without waiting for it to expire.
        let app = reset_official(&state, &server, true).await;
        {
            let mut data = server.state.data.lock().await;
            data.lookup_rate_limits = 1;
            data.lookup_retry_after = "60".into();
        }
        let before = server.counters();
        for (attempt, body) in [
            json!({"user_id":user,"checksum":"5".repeat(32)}),
            json!({"user_id":user,"checksum":"5".repeat(32)}),
            json!({"user_id":user,"official_id":995}),
        ].into_iter().enumerate() {
            let (status, value) = tokio::time::timeout(
                Duration::from_secs(1),
                resolve(&app, body),
            )
            .await
            .expect("a 60-second upstream Retry-After must return 429 promptly");
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
            assert_eq!(value["error"]["code"], "official_rate_limited");
            if attempt == 0 {
                assert!(
                    state.official.lock().await.backoff_remaining().unwrap()
                        >= Duration::from_secs(58),
                    "the prompt response must retain the full real 60-second backoff deadline"
                );
            }
        }
        assert_eq!(
            server.counters(),
            [before[0] + 1, before[1] + 1, before[2], before[3]],
            "one actual 429 must cache the cooldown across selectors without extra upstream calls"
        );

        // A new independent scenario uses a real one-second cooldown, then a caller retries.
        let app = reset_official(&state, &server, true).await;
        let retry = OfficialMap::new(104, "ranked");
        let retry_md5 = retry.checksum();
        server.add(retry).await;
        {
            let mut data = server.state.data.lock().await;
            data.lookup_rate_limits = 1;
            data.lookup_retry_after = "1".into();
        }
        let before = server.counters();
        for body in [
            json!({"user_id":user,"checksum":retry_md5}),
            json!({"user_id":user,"checksum":retry_md5}),
            json!({"user_id":user,"official_id":996}),
        ] {
            let (status, value) = tokio::time::timeout(
                Duration::from_secs(1),
                resolve(&app, body),
            ).await.expect("an upstream 429 must return immediately instead of retrying internally");
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
            assert_eq!(value["error"]["code"], "official_rate_limited");
        }
        assert_eq!(
            server.counters(),
            [before[0] + 1, before[1] + 1, before[2], before[3]],
            "requests during the real one-second cooldown must not call upstream"
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let (status, value) = tokio::time::timeout(
            Duration::from_secs(5),
            resolve(&app, json!({"user_id":user,"checksum":retry_md5})),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_resolved(&pool, &value, &retry_md5, Some(104), "ranked", "official").await;
        assert_eq!(
            server.counters(),
            [before[0] + 1, before[1] + 2, before[2], before[3] + 1],
            "a caller retry after Retry-After expires must import using the cached OAuth token"
        );

        // API/file checksum, mode, and identities must agree before persistence.
        for (id, defect, expected_code) in [
            (105, "checksum", "checksum_mismatch"),
            (106, "mode", "official_identity_mismatch"),
            (107, "file_id", "official_identity_mismatch"),
            (108, "file_set", "official_identity_mismatch"),
        ] {
            let app = reset_official(&state, &server, true).await;
            let mut map = OfficialMap::new(id, "ranked");
            match defect {
                "checksum" => map.metadata["checksum"] = json!("4".repeat(32)),
                "mode" => map.metadata["mode_int"] = json!(1),
                "file_id" => {
                    map.bytes = osu_bytes(id + 99, id + 1000, 0, "Wrong file ID");
                    map.metadata["checksum"] = json!(format!("{:x}", md5::compute(&map.bytes)));
                }
                "file_set" => {
                    map.bytes = osu_bytes(id, id + 1099, 0, "Wrong file set");
                    map.metadata["checksum"] = json!(format!("{:x}", md5::compute(&map.bytes)));
                }
                _ => unreachable!(),
            }
            server.add(map).await;
            let before = beatmap_count(&pool).await;
            let (status, value) = resolve(&app, json!({"user_id":user,"official_id":id})).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{defect}: {value}");
            assert_eq!(value["error"]["code"], expected_code, "{defect}: {value}");
            assert_eq!(
                beatmap_count(&pool).await,
                before,
                "{defect} must not insert"
            );
        }

        // A newer file with the same official ID cannot replace a stored score identity.
        let app = reset_official(&state, &server, true).await;
        let mut updated = OfficialMap::new(100, "loved");
        updated.bytes = osu_bytes(100, 1100, 0, "Updated official file");
        updated.metadata["checksum"] = json!(format!("{:x}", md5::compute(&updated.bytes)));
        let updated_md5 = updated.checksum();
        server.add(updated).await;
        let before = beatmap_count(&pool).await;
        let (status, value) = resolve(&app, json!({"user_id":user,"checksum":updated_md5})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(value["error"]["code"], "official_version_conflict");
        assert_eq!(beatmap_count(&pool).await, before);
        let (status, preserved) = resolve(&app, json!({"user_id":user,"official_id":100})).await;
        assert_eq!(status, StatusCode::OK, "{preserved}");
        assert_eq!(preserved, imported);
        assert_eq!(
            tokio::fs::read(state.maps_root.join(&path)).await.unwrap(),
            bytes,
            "concurrent aliases and version conflicts must preserve the original bytes"
        );

        // Manual imports intentionally reuse local rows and retain existing moderator status.
        let moderator = player(&pool, "NativeModerator", false).await;
        sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='moderator'")
            .bind(moderator).execute(&pool).await.unwrap();
        let moderator_token = bearer_session(&state, moderator).await;
        let local_before: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(private_id).fetch_one(&pool).await.unwrap();
        let before = server.counters();
        let (status, value) = manual_import(&app, Some(&moderator_token), 101).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["id"], private_id);
        assert_eq!(value["md5"], private_md5);
        assert_eq!(value["source"], "local");
        assert_eq!(value["status"], "pending");
        let local_after: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(private_id).fetch_one(&pool).await.unwrap();
        assert_eq!(local_after, local_before, "manual import must preserve all local metadata");

        sqlx::query("UPDATE beatmaps SET status='unranked' WHERE id=$1")
            .bind(imported["id"].as_i64().unwrap()).execute(&pool).await.unwrap();
        let official_before: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(imported["id"].as_i64().unwrap()).fetch_one(&pool).await.unwrap();
        let (status, value) = manual_import(&app, Some(&moderator_token), 100).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["id"], imported["id"]);
        assert_eq!(value["md5"], checksum);
        assert_eq!(value["source"], "official");
        assert_eq!(value["status"], "unranked");
        let official_after: Value = sqlx::query_scalar("SELECT to_jsonb(b) FROM beatmaps b WHERE id=$1")
            .bind(imported["id"].as_i64().unwrap()).fetch_one(&pool).await.unwrap();
        assert_eq!(official_after, official_before, "manual import must preserve moderated metadata");
        assert_eq!(server.counters(), before, "known manual imports must not refresh upstream metadata");

        let official_rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM beatmaps WHERE source='official'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let mut directory = tokio::fs::read_dir(&state.maps_root).await.unwrap();
        let mut official_files = 0_i64;
        while let Some(entry) = directory.next_entry().await.unwrap() {
            let file_path = entry.path();
            assert_ne!(
                file_path.extension().and_then(|value| value.to_str()),
                Some("part"),
                "temporary official map files must be cleaned up"
            );
            if entry.file_name().to_string_lossy().starts_with("official-")
                && file_path.extension().and_then(|value| value.to_str()) == Some("osu")
            {
                assert!(entry.file_type().await.unwrap().is_file());
                official_files += 1;
            }
        }
        assert_eq!(
            official_files, official_rows,
            "only successfully stored official rows may retain an official .osu file"
        );
    })
    .await;
    queued_db.close().await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    if root.exists() {
        // Only this random fixture directory is removed, after verifying its parent.
        assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
        tokio::fs::remove_dir_all(&root).await.unwrap();
    }
    outcome.unwrap();
}
