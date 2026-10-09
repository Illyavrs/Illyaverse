use super::*;
use crate::maps::map_mutation_tests::{MutationFixture, actor, file_count, request, wait_for_lock};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    routing::{get, post},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

fn set(set_id: i64) -> Value {
    json!({"id":set_id,"title":"Title 星", "artist":"Artist", "creator":"Mapper", "status":"ranked", "covers":{"card":"https://example.invalid/private"}, "user_id":99,
        "availability":{"download_disabled":false}, "beatmaps":[{"id":set_id*10,"beatmapset_id":set_id,"version":"Hard", "mode_int":0,"mode":"osu","difficulty_rating":5.5,"total_length":120}]})
}
fn response() -> Value {
    json!({"beatmapsets":[set(42)], "total":1, "cursor_string":null, "search":{"sort":"ranked_desc"}})
}
fn query(parameters: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("https://fixture.invalid/").unwrap();
    url.query_pairs_mut()
        .extend_pairs(parameters.iter().copied());
    url.query().unwrap_or_default().into()
}
#[test]
fn official_search_query_bounds_and_opaque_encoding() {
    let defaults = parse(None).unwrap();
    assert_eq!(defaults.status, "ranked");
    assert_eq!(defaults.mode, "all");
    for raw in [
        "mode=4",
        "status=mine",
        "sort=url",
        "url=https://example.invalid",
        "q=a&q=b",
        "mode=all&mode=0",
        "cursor_string=",
        "q=%00",
        "q=%",
        "q=%0",
        "q=%GG",
        "q=%FF",
        "q=%E6%98",
    ] {
        assert_eq!(
            parse(Some(raw.into())).unwrap_err().0,
            StatusCode::BAD_REQUEST,
            "{raw}"
        );
    }
    for (name, value) in [
        ("q", "星".repeat(201)),
        ("q", "𐀀".repeat(201)),
        ("cursor_string", "a".repeat(2049)),
        ("cursor_string", "a\nb".into()),
    ] {
        assert!(parse(Some(query(&[(name, &value)]))).is_err());
    }
    let cursor = r#"abc+/= &?cursor=hidden%20{"id":42}"#;
    let key = parse(Some(query(&[
        ("q", "歌 & 星"),
        ("cursor_string", cursor),
        ("sort", "date_desc"),
        ("mode", "3"),
    ])))
    .unwrap();
    let url = reqwest::Url::parse(&format!("https://osu.ppy.sh{}", key.path())).unwrap();
    assert_eq!(url.host_str(), Some("osu.ppy.sh"));
    let pairs = url.query_pairs().collect::<HashMap<_, _>>();
    assert_eq!(pairs["q"], "歌 & 星");
    assert_eq!(pairs["cursor_string"], cursor);
    assert_eq!(pairs["sort"], "updated_desc");
    assert_eq!(pairs["m"], "3");
    assert!(
        parse(Some(query(&[
            ("q", &"𐀀".repeat(200)),
            ("cursor_string", &"a".repeat(2048))
        ])))
        .is_ok()
    );
}
#[test]
fn official_search_sanitizes_and_excludes_deleted_converted_members() {
    let mut value = response();
    value["beatmapsets"][0]["title"] = json!("  Title\n星  ");
    value["beatmapsets"][0]["beatmaps"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"convert":true, "id":999999999999i64}),
            json!({"deleted_at":"2026-01-01", "id":0}),
        ]);
    value["beatmapsets"]
        .as_array_mut()
        .unwrap()
        .push(json!({"deleted_at":"2026-01-01","id":0}));
    let result = transform(&value).unwrap();
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].difficulties.len(), 1);
    assert_eq!(result.items[0].title, "Title 星");
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(
        result["items"][0]["cover_url"],
        "https://assets.ppy.sh/beatmaps/42/covers/card.jpg"
    );
    assert_eq!(
        result["items"][0]["official_url"],
        "https://osu.ppy.sh/beatmapsets/42"
    );
    assert!(result["items"][0].get("user_id").is_none());
    assert!(result["items"][0].get("covers").is_none());
    assert!(
        result["items"][0]["difficulties"][0]
            .get("checksum")
            .is_none()
    );
}
#[test]
fn official_search_rejects_invalid_identity_and_response_bounds() {
    for (field, bad) in [
        ("id", json!(2_000_000_000)),
        ("title", json!("")),
        ("creator", json!("x".repeat(1025))),
        ("status", json!("unknown")),
    ] {
        let mut value = response();
        value["beatmapsets"][0][field] = bad;
        assert!(transform(&value).is_err(), "{field}");
    }
    for (field, bad) in [
        ("id", json!(0)),
        ("beatmapset_id", json!(43)),
        ("mode_int", json!(4)),
        ("mode", json!("mania")),
        ("difficulty_rating", json!(-1)),
        ("total_length", json!(-1)),
        ("convert", json!("false")),
        ("checksum", json!("not-a-checksum")),
        ("bpm", json!(-1)),
        ("ar", json!(101)),
        ("cs", json!("4")),
        ("accuracy", json!(101)),
        ("drain", json!(-1)),
        ("max_combo", json!(2147483648i64)),
    ] {
        let mut value = response();
        value["beatmapsets"][0]["beatmaps"][0][field] = bad;
        assert!(transform(&value).is_err(), "{field}");
    }
    let mut duplicate = response();
    duplicate["beatmapsets"]
        .as_array_mut()
        .unwrap()
        .push(set(42));
    assert!(transform(&duplicate).is_err());
    let mut many = response();
    many["beatmapsets"] = json!((1..=51).map(set).collect::<Vec<_>>());
    assert!(transform(&many).is_err());
    let mut members = response();
    members["beatmapsets"][0]["beatmaps"] = json!((1..=513).map(|n| json!({"id":n,"beatmapset_id":42,"version":"Hard","mode_int":0,"difficulty_rating":1,"total_length":1})).collect::<Vec<_>>());
    assert!(transform(&members).is_err());
    for value in [
        json!({}),
        json!({"beatmapsets":[],"total":-1}),
        json!({"beatmapsets":[],"total":0,"cursor_string":"a".repeat(2049)}),
    ] {
        assert!(transform(&value).is_err());
    }
}
#[test]
fn official_search_cache_is_bounded_and_expires() {
    let mut state = OfficialState::default();
    for index in 0..32 {
        let key = parse(Some(format!("q={index}"))).unwrap();
        remember(&mut state, key, &Ok(transform(&response()).unwrap()));
    }
    assert_eq!(state.search_cache.len(), MAX_ENTRIES);
    assert!(
        state
            .search_cache
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            <= MAX_CACHE_BYTES
    );
    for entry in state.search_cache.values_mut() {
        entry.until = Instant::now();
    }
    assert!(cached(&mut state, &parse(None).unwrap()).is_none());
    assert!(state.search_cache.is_empty());
    let key = parse(None).unwrap();
    state.failure = Some(CachedFailure::new(
        &ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "official_rate_limited",
            "fixture".into(),
        ),
        Duration::from_secs(120),
    ));
    remember(
        &mut state,
        key.clone(),
        &Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "official_rate_limited",
            "fixture".into(),
        )),
    );
    assert!(state.search_cache[&key].until > Instant::now() + Duration::from_secs(115));
}

#[derive(Clone)]
struct UpstreamState {
    reply: Arc<Mutex<(StatusCode, Vec<u8>, Duration)>>,
    calls: Arc<AtomicUsize>,
    queries: Arc<Mutex<Vec<String>>>,
    paths: Arc<Mutex<Vec<String>>>,
    oauth_calls: Arc<AtomicUsize>,
    oauth_delay: Arc<Mutex<Duration>>,
}
struct Upstream {
    origin: String,
    state: UpstreamState,
    task: tokio::task::JoinHandle<()>,
}
impl Upstream {
    async fn start() -> Self {
        let state = UpstreamState {
            reply: Arc::new(Mutex::new((
                StatusCode::OK,
                response().to_string().into_bytes(),
                Duration::ZERO,
            ))),
            calls: Arc::new(AtomicUsize::new(0)),
            queries: Default::default(),
            paths: Default::default(),
            oauth_calls: Default::default(),
            oauth_delay: Default::default(),
        };
        let app = Router::new()
            .route(
                "/oauth/token",
                post(
                    |State(state): State<UpstreamState>, Json(body): Json<Value>| async move {
                        assert_eq!(body["grant_type"], "client_credentials");
                        assert_eq!(body["scope"], "public");
                        state.oauth_calls.fetch_add(1, Ordering::SeqCst);
                        let delay = *state.oauth_delay.lock().await;
                        tokio::time::sleep(delay).await;
                        Json(json!({"access_token":"owned-fixture-public","expires_in":3600}))
                    },
                ),
            )
            .route("/api/v2/beatmapsets/search", get(upstream_search))
            .route("/api/v2/beatmapsets/{id}", get(upstream_detail))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            origin,
            state,
            task,
        }
    }
    async fn reply(&self, status: StatusCode, bytes: Vec<u8>, delay: Duration) {
        *self.state.reply.lock().await = (status, bytes, delay);
    }
    fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }
    async fn wait_calls(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.calls() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn wait_oauth_calls(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while self.state.oauth_calls.load(Ordering::SeqCst) < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn upstream_search(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    state.queries.lock().await.push(query.unwrap_or_default());
    upstream_reply(state, headers, "/api/v2/beatmapsets/search".into()).await
}
async fn upstream_detail(
    State(state): State<UpstreamState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    upstream_reply(state, headers, format!("/api/v2/beatmapsets/{id}")).await
}
async fn upstream_reply(state: UpstreamState, headers: HeaderMap, path: String) -> Response {
    assert_eq!(
        headers[header::AUTHORIZATION],
        "Bearer owned-fixture-public"
    );
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.paths.lock().await.push(path);
    let (status, bytes, delay) = state.reply.lock().await.clone();
    tokio::time::sleep(delay).await;
    let mut response = (status, bytes).into_response();
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "120".parse().unwrap());
    }
    response
}
async fn get_search(app: &Router, cookie: Option<&str>, query: &str) -> (StatusCode, Value) {
    get_endpoint(
        app,
        cookie,
        &format!("/api/v1/beatmaps/official/search?{query}"),
    )
    .await
}
async fn get_public_search(app: &Router, query: &str) -> (StatusCode, Value) {
    get_endpoint(
        app,
        None,
        &format!("/api/v1/official/beatmapsets/search?{query}"),
    )
    .await
}
async fn get_public_detail(app: &Router, id: i64) -> (StatusCode, Value) {
    get_endpoint(app, None, &format!("/api/v1/official/beatmapsets/{id}")).await
}
async fn get_endpoint(app: &Router, cookie: Option<&str>, uri: &str) -> (StatusCode, Value) {
    let mut builder = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if uri.starts_with("/api/v1/official/beatmapsets/") {
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
async fn no_writes(state: &AppState, maps: i64) {
    for (table, expected) in [
        ("beatmaps", maps),
        ("beatmap_filename_aliases", 0),
        ("official_beatmapset_archives", 0),
        ("official_beatmapset_archive_members", 0),
        ("audit_events", 0),
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, expected, "{table}");
    }
    assert_eq!(file_count(&state.maps_root).await, 0);
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_auth_query_cache_and_fresh_withdrawal() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("official_search_auth", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (staff_id,_,_,staff) = actor(&state,"SearchStaff",&["beatmaps.import"]).await;
        let (_,_,_,player) = actor(&state,"SearchPlayer",&["beatmaps.upload"]).await;
        assert_eq!(get_search(&app,None,"unknown=1").await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(get_search(&app,Some(&player),"unknown=1").await.0,StatusCode::FORBIDDEN);
        for query in ["unknown=1","mode=99","q=a&q=b"] { assert_eq!(get_search(&app,Some(&staff),query).await.0,StatusCode::BAD_REQUEST); }
        assert_eq!(upstream.calls(),0);
        let (status,value) = get_search(&app,Some(&staff),"").await; assert_eq!(status,StatusCode::OK,"{value}");
        assert_eq!(value["items"][0]["local"]["stored_count"],0);
        sqlx::query("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES(420,42,'Retained private title','A','C','V',0,'ranked','official',1,$1,'private.osu')").bind("a".repeat(32)).execute(&state.db).await.unwrap();
        let (_,value) = get_search(&app,Some(&staff),"").await; assert_eq!(value["items"][0]["local"]["stored_count"],1); assert_eq!(value["items"][0]["title"],"Title 星");
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(42,true,1,'Owned fixture',$1)").bind(staff_id).execute(&state.db).await.unwrap();
        let (_,value) = get_search(&app,Some(&staff),"").await; assert_eq!(value["items"][0]["local"],json!({"stored_count":0,"archive_available":false,"withdrawn":true}));
        assert_eq!(upstream.calls(),1);
        no_writes(&state,1).await;
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_coalesces_and_rechecks_revoked_sessions() {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            response().to_string().into_bytes(),
            Duration::from_millis(400),
        )
        .await;
    let fixture = MutationFixture::new("official_search_flight", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, first_session, first) = actor(&state, "SearchFirst", &["beatmaps.import"]).await;
        let (_, _, _, second) = actor(&state, "SearchSecond", &["beatmaps.import"]).await;
        let app_first = app.clone();
        let app_second = app.clone();
        let first_task =
            tokio::spawn(async move { get_search(&app_first, Some(&first), "q=same").await });
        upstream.wait_calls(1).await;
        let second_task =
            tokio::spawn(async move { get_search(&app_second, Some(&second), "q=same").await });
        sqlx::query("DELETE FROM sessions WHERE id=$1")
            .bind(first_session)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(first_task.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert_eq!(second_task.await.unwrap().0, StatusCode::OK);
        assert_eq!(upstream.calls(), 1);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_permission_revocation_precedes_cached_success_and_errors() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("official_search_permission", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, role, _, staff) = actor(&state, "SearchPermission", &["beatmaps.import"]).await;
        assert_eq!(get_search(&app, Some(&staff), "").await.0, StatusCode::OK);
        sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1")
            .bind(role)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(
            get_search(&app, Some(&staff), "").await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(upstream.calls(), 1);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_shares_global_backoff_and_encodes_opaque_cursor() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("official_search_backoff", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, staff) = actor(&state, "SearchBackoff", &["beatmaps.import"]).await;
        let cursor = r#"abc+/= &?cursor=hidden%20{"id":42}"#;
        let raw = query(&[
            ("cursor_string", cursor),
            ("q", "歌 & 星"),
            ("mode", "2"),
            ("status", "any"),
            ("sort", "date_desc"),
        ]);
        assert_eq!(get_search(&app, Some(&staff), &raw).await.0, StatusCode::OK);
        let seen = upstream.state.queries.lock().await.clone();
        let seen = reqwest::Url::parse(&format!("https://osu.ppy.sh/?{}", seen[0])).unwrap();
        let pairs = seen.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(pairs["cursor_string"], cursor);
        assert_eq!(pairs["q"], "歌 & 星");
        assert_eq!(pairs["sort"], "updated_desc");
        assert_eq!(pairs["m"], "2");
        upstream
            .reply(StatusCode::TOO_MANY_REQUESTS, vec![], Duration::ZERO)
            .await;
        for raw in ["q=limited", "q=other", "q=limited"] {
            let (status, value) = get_search(&app, Some(&staff), raw).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
            assert_eq!(value["error"]["code"], "official_rate_limited");
        }
        assert_eq!(upstream.calls(), 2);
        assert!(
            state.official.lock().await.backoff_remaining().unwrap() > Duration::from_secs(115)
        );
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_rejects_oversize_invalid_upstream_without_catalog_writes() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("official_search_invalid", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, staff) = actor(&state, "SearchInvalid", &["beatmaps.import"]).await;
        for (index, bytes) in [
            b"not JSON".to_vec(),
            json!({"beatmapsets":[],"total":-1})
                .to_string()
                .into_bytes(),
            vec![b' '; 4 * 1024 * 1024 + 1],
        ]
        .into_iter()
        .enumerate()
        {
            upstream.reply(StatusCode::OK, bytes, Duration::ZERO).await;
            let (status, value) = get_search(&app, Some(&staff), &format!("q={index}")).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
        }
        assert_eq!(upstream.calls(), 3);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_deadline_and_shared_slot_bound() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("official_search_deadline", upstream.origin.clone()).await;
    let mut state = fixture.state.clone();
    state.client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let app = crate::router(state.clone());
    let result = tokio::spawn(async move {
        let (_, _, _, staff) = actor(&state, "SearchDeadline", &["beatmaps.import"]).await;
        let permit = state
            .official
            .lock()
            .await
            .resolution_slots()
            .acquire_many_owned(4)
            .await
            .unwrap();
        let (status, value) = get_search(&app, Some(&staff), "q=busy").await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(value["error"]["code"], "official_resolution_busy");
        assert_eq!(upstream.calls(), 0);
        drop(permit);
        upstream
            .reply(
                StatusCode::OK,
                response().to_string().into_bytes(),
                Duration::from_secs(8),
            )
            .await;
        let started = Instant::now();
        let (status, value) = get_search(&app, Some(&staff), "q=slow").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "official_search_timeout");
        assert!(started.elapsed() < Duration::from_secs(7));
        assert_eq!(upstream.calls(), 1);
        let (status, value) = get_search(&app, Some(&staff), "q=slow").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(value["error"]["code"], "official_search_timeout");
        assert_eq!(upstream.calls(), 1);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

fn full_set() -> Value {
    let mut value = set(42);
    value["beatmaps"] = json!([
        {"id":422,"beatmapset_id":42,"version":"Oni","mode_int":1,"mode":"taiko","difficulty_rating":4.2,"total_length":123,"checksum":"c".repeat(32),"bpm":180.0,"ar":5.0,"cs":5.0,"accuracy":7.0,"drain":6.0,"max_combo":345},
        {"id":421,"beatmapset_id":42,"version":"Expert","mode_int":0,"mode":"osu","difficulty_rating":6.5,"total_length":124,"checksum":"b".repeat(32),"bpm":180.0,"ar":9.0,"cs":4.0,"accuracy":8.0,"drain":6.0,"max_combo":678},
        {"id":420,"beatmapset_id":42,"version":"Hard","mode_int":0,"mode":"osu","difficulty_rating":3.5,"total_length":125,"checksum":"A".repeat(32),"bpm":180.0,"ar":8.0,"cs":4.0,"accuracy":7.0,"drain":5.0,"max_combo":456}
    ]);
    for (field, data) in [
        ("artist_unicode", json!("歌手")),
        ("title_unicode", json!("標題")),
        ("bpm", json!(180.0)),
        ("source", json!("")),
        ("tags", json!("owned fixture")),
        ("nsfw", json!(false)),
        ("submitted_date", json!("2020-01-01T00:00:00Z")),
        ("last_updated", json!("2020-01-02T00:00:00Z")),
        ("genre", json!({"id":5,"name":"Video Game"})),
        ("language", json!({"id":3,"name":"Japanese"})),
    ] {
        value[field] = data;
    }
    for map in value["beatmaps"].as_array_mut().unwrap() {
        map["status"] = json!("ranked");
        map["last_updated"] = json!("2020-01-02T00:00:00Z");
        map["hit_length"] = json!(100);
        map["count_circles"] = json!(100);
        map["count_sliders"] = json!(100);
        map["count_spinners"] = json!(1);
    }
    value
}

#[test]
fn official_search_full_difficulties_preserve_stats_and_normalize_checksums() {
    let transformed = transform(&json!({"beatmapsets":[full_set()],"total":1})).unwrap();
    let maps = &transformed.items[0].difficulties;
    assert_eq!(maps.len(), 3);
    assert_eq!(
        maps.iter().map(|map| map.official_id).collect::<Vec<_>>(),
        [420, 421, 422]
    );
    assert_eq!(maps[0].checksum, Some("a".repeat(32)));
    assert_eq!(maps[0].bpm, Some(180.0));
    assert_eq!(maps[0].ar, Some(8.0));
    assert_eq!(maps[0].cs, Some(4.0));
    assert_eq!(maps[0].od, Some(7.0));
    assert_eq!(maps[0].hp, Some(5.0));
    assert_eq!(maps[0].max_combo, Some(456));
    assert!(maps.iter().all(|map| map.local_id.is_none()));
    assert!(parse(Some("q=a".repeat(3000))).is_err());
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_shared_cache_fresh_checksum_links_and_withdrawal() {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            json!({"beatmapsets":[full_set()],"total":1})
                .to_string()
                .into_bytes(),
            Duration::ZERO,
        )
        .await;
    let fixture = MutationFixture::new("public_search_links", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (staff_id, role, _, staff) = actor(&state, "PublicSearchStaff", &["beatmaps.import"]).await;
        for raw in ["unknown=1", "mode=9", "q=a&q=b", "q=%GG", "q=%FF"] {
            assert_eq!(get_public_search(&app, raw).await.0, StatusCode::BAD_REQUEST);
        }
        assert_eq!(upstream.calls(), 0);
        let (status, value) = get_public_search(&app, "").await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["items"][0]["difficulties"].as_array().unwrap().len(), 3);
        let local_id: i64 = sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES(420,42,'Private retained title','A','C','V',0,'ranked','official',1,$1,'private.osu') RETURNING id")
            .bind("a".repeat(32)).fetch_one(&state.db).await.unwrap();
        for (official_id, set_id, checksum) in [(Some(421_i64),42_i64,"d".repeat(32)), (Some(422),43,"c".repeat(32)), (None,42,"e".repeat(32))] {
            sqlx::query("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES($1,$2,'Private retained title','A','C','V',0,'ranked','official',1,$3,'private.osu')")
                .bind(official_id).bind(set_id).bind(checksum).execute(&state.db).await.unwrap();
        }
        let (status, value) = get_search(&app, Some(&staff), "").await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["items"][0]["title"], "Title 星");
        assert_eq!(value["items"][0]["difficulties"][0]["local_id"], local_id);
        assert_eq!(value["items"][0]["difficulties"][0]["archive_available"], false);
        assert!(value["items"][0]["difficulties"][1].get("local_id").is_none());
        assert!(value["items"][0]["difficulties"][2].get("local_id").is_none());
        assert_eq!(upstream.calls(), 1);
        assert_eq!(state.official.lock().await.public_discovery_requests.len(), 1);
        sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&state.db).await.unwrap();
        assert_eq!(get_search(&app, Some(&staff), "").await.0, StatusCode::FORBIDDEN);
        assert_eq!(get_public_search(&app, "").await.0, StatusCode::OK);
        sqlx::query("UPDATE roles SET permissions=ARRAY['beatmaps.import'] WHERE id=$1").bind(role).execute(&state.db).await.unwrap();
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(42,true,1,'Private moderation reason',$1)").bind(staff_id).execute(&state.db).await.unwrap();
        let (_, value) = get_public_search(&app, "").await;
        assert_eq!(value["items"], json!([]));
        let (status, value) = get_public_detail(&app, 42).await;
        assert_eq!(status, StatusCode::GONE, "{value}");
        assert!(!value.to_string().contains("Private moderation reason"));
        let (_, value) = get_search(&app, Some(&staff), "").await;
        assert_eq!(value["items"][0]["local"],json!({"stored_count":0,"archive_available":false,"withdrawn":true}));
        assert!(value["items"][0]["difficulties"].as_array().unwrap().iter().all(|map| map.get("local_id").is_none()));
        sqlx::query("UPDATE beatmapset_availability SET withdrawn=false,revision=2,changed_at=clock_timestamp() WHERE native_set_id=42").execute(&state.db).await.unwrap();
        let (_, value) = get_public_search(&app, "").await;
        assert_eq!(value["items"][0]["difficulties"][0]["local_id"], local_id);
        assert_eq!(upstream.calls(), 1);
        assert_eq!(upstream.state.oauth_calls.load(Ordering::SeqCst), 1);
        no_writes(&state, 4).await;
    }).await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_detail_validates_identity_complete_stats_and_negative_cache() {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            full_set().to_string().into_bytes(),
            Duration::ZERO,
        )
        .await;
    let fixture = MutationFixture::new("public_search_detail", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        for id in [0, -1, 2_000_000_000] {
            assert_eq!(get_public_detail(&app, id).await.0, StatusCode::BAD_REQUEST);
        }
        assert_eq!(upstream.calls(), 0);
        let (status, value) = get_public_detail(&app, 42).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["official_set_id"], 42);
        assert_eq!(value["difficulties"].as_array().unwrap().len(), 3);
        assert_eq!(value["difficulties"][0]["max_combo"], 456);
        assert_eq!(value["difficulties"][0]["length_seconds"], 125);
        assert_eq!(value["difficulties"][0]["od"], 7.0);
        assert!(value.get("user_id").is_none());
        assert!(value.get("covers").is_none());
        assert_eq!(get_public_detail(&app, 42).await.0, StatusCode::OK);
        assert_eq!(upstream.calls(), 1);
        assert_eq!(
            *upstream.state.paths.lock().await,
            vec!["/api/v2/beatmapsets/42"]
        );
        // An upstream mismatch never becomes a detail page under the requested ID.
        let (status, value) = get_public_detail(&app, 43).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
        assert_eq!(value["error"]["code"], "official_search_invalid");
        assert_eq!(get_public_detail(&app, 43).await.0, StatusCode::BAD_GATEWAY);
        assert_eq!(upstream.calls(), 2);
        upstream
            .reply(StatusCode::NOT_FOUND, vec![], Duration::ZERO)
            .await;
        assert_eq!(get_public_detail(&app, 44).await.0, StatusCode::NOT_FOUND);
        assert_eq!(get_public_detail(&app, 44).await.0, StatusCode::NOT_FOUND);
        assert_eq!(upstream.calls(), 3);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_quota_slots_and_cache_leave_native_capacity() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("public_search_budget", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, _, staff) = actor(&state, "PublicBudgetStaff", &["beatmaps.import"]).await;
        assert_eq!(get_public_search(&app, "").await.0, StatusCode::OK);
        let public_slots = state.official.lock().await.public_discovery_slots.clone();
        let held_public = public_slots.acquire_many_owned(2).await.unwrap();
        assert_eq!(get_public_search(&app, "").await.0, StatusCode::OK);
        assert_eq!(
            get_public_search(&app, "q=busy").await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            get_search(&app, Some(&staff), "q=staff-capacity").await.0,
            StatusCode::OK
        );
        assert_eq!(upstream.calls(), 2);
        drop(held_public);
        {
            let mut official = state.official.lock().await;
            official.public_discovery_requests = vec![Instant::now(); 30];
        }
        assert!(public_result(&state, parse(None).unwrap()).await.is_ok());
        assert_eq!(
            public_result(&state, parse(Some("q=quota".into())).unwrap())
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(upstream.calls(), 2);
        state.official.lock().await.public_discovery_requests[0] =
            Instant::now() - Duration::from_secs(61);
        assert!(
            public_result(&state, parse(Some("q=quota".into())).unwrap())
                .await
                .is_ok()
        );
        assert_eq!(
            state.official.lock().await.public_discovery_requests.len(),
            30
        );
        assert_eq!(upstream.calls(), 3);
        let slots = state.official.lock().await.slots.clone();
        let held_shared = slots.acquire_many_owned(4).await.unwrap();
        assert!(public_result(&state, parse(None).unwrap()).await.is_ok());
        assert_eq!(
            public_result(&state, parse(Some("q=shared-busy".into())).unwrap())
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(upstream.calls(), 3);
        drop(held_shared);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_and_staff_coalesce_without_bypassing_revoked_staff_auth() {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            response().to_string().into_bytes(),
            Duration::from_millis(400),
        )
        .await;
    let fixture = MutationFixture::new("public_search_flight", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (_, _, session, staff) = actor(&state, "PublicFlightStaff", &["beatmaps.import"]).await;
        let public_app = app.clone();
        let public = tokio::spawn(async move { get_public_search(&public_app, "q=same").await });
        upstream.wait_calls(1).await;
        let staff_app = app.clone();
        let staff_task =
            tokio::spawn(async move { get_search(&staff_app, Some(&staff), "q=same").await });
        sqlx::query("DELETE FROM sessions WHERE id=$1")
            .bind(session)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(public.await.unwrap().0, StatusCode::OK);
        assert_eq!(staff_task.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert_eq!(get_public_search(&app, "q=same").await.0, StatusCode::OK);
        assert_eq!(upstream.calls(), 1);
        assert_eq!(
            state.official.lock().await.public_discovery_requests.len(),
            1
        );
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_timeout_invalid_response_and_global_backoff_are_bounded() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("public_search_failures", upstream.origin.clone()).await;
    let mut state = fixture.state.clone();
    state.client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let app = crate::router(state.clone());
    let result = tokio::spawn(async move {
        for (index, bytes) in [
            b"not JSON".to_vec(),
            json!({"beatmapsets":[],"total":-1})
                .to_string()
                .into_bytes(),
            vec![b' '; 4 * 1024 * 1024 + 1],
        ]
        .into_iter()
        .enumerate()
        {
            upstream.reply(StatusCode::OK, bytes, Duration::ZERO).await;
            let raw = format!("q=invalid-{index}");
            assert_eq!(
                get_public_search(&app, &raw).await.0,
                StatusCode::BAD_GATEWAY
            );
            assert_eq!(
                get_public_search(&app, &raw).await.0,
                StatusCode::BAD_GATEWAY
            );
        }
        assert_eq!(upstream.calls(), 3);
        upstream
            .reply(
                StatusCode::OK,
                response().to_string().into_bytes(),
                Duration::from_secs(8),
            )
            .await;
        let started = Instant::now();
        let (status, value) = get_public_search(&app, "q=slow").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
        assert_eq!(value["error"]["code"], "official_search_timeout");
        assert!(started.elapsed() < Duration::from_secs(7));
        assert_eq!(
            get_public_search(&app, "q=slow").await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(upstream.calls(), 4);
        upstream
            .reply(StatusCode::TOO_MANY_REQUESTS, vec![], Duration::ZERO)
            .await;
        for raw in ["q=limited", "q=other", "q=limited"] {
            let (status, value) = get_public_search(&app, raw).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
            assert_eq!(value["error"]["code"], "official_rate_limited");
        }
        assert_eq!(upstream.calls(), 5);
        assert!(
            state.official.lock().await.backoff_remaining().unwrap() > Duration::from_secs(115)
        );
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_public_withdrawal_during_merge_and_sql_timeout_preserve_metadata() {
    let upstream = Upstream::start().await;
    let fixture = MutationFixture::new("public_search_sql", upstream.origin.clone()).await;
    let application = fixture.schema.clone();
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (staff_id,_,_,staff) = actor(&state,"PublicSqlStaff",&["beatmaps.import"]).await;
        assert_eq!(get_public_search(&app,"").await.0,StatusCode::OK);
        let mut lock = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE beatmaps IN ACCESS EXCLUSIVE MODE").execute(&mut *lock).await.unwrap();
        let waiting_app = app.clone();
        let waiting = tokio::spawn(async move {get_public_search(&waiting_app,"").await});
        wait_for_lock(&state.db,&application,"SELECT candidate.id").await;
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES(42,true,1,'Private reason during merge',$1)").bind(staff_id).execute(&state.db).await.unwrap();
        lock.commit().await.unwrap();
        let (status,value) = waiting.await.unwrap();
        assert_eq!(status,StatusCode::OK,"{value}");
        assert_eq!(value["items"],json!([]));
        sqlx::query("UPDATE beatmapset_availability SET withdrawn=false,revision=2,changed_at=clock_timestamp() WHERE native_set_id=42").execute(&state.db).await.unwrap();
        let mut lock = state.db.begin().await.unwrap();
        sqlx::query("LOCK TABLE beatmaps IN ACCESS EXCLUSIVE MODE").execute(&mut *lock).await.unwrap();
        let waiting_app = app.clone();
        let waiting = tokio::spawn(async move {get_public_search(&waiting_app,"").await});
        wait_for_lock(&state.db,&application,"SELECT candidate.id").await;
        let (status,value) = waiting.await.unwrap();
        assert_eq!(status,StatusCode::SERVICE_UNAVAILABLE,"{value}");
        lock.commit().await.unwrap();
        let (status,value) = get_search(&app,Some(&staff),"").await;
        assert_eq!(status,StatusCode::OK,"{value}");
        assert_eq!(get_public_search(&app,"").await.0,StatusCode::OK);
        assert_eq!(upstream.calls(),1,"local SQL timeout must retain healthy upstream metadata");
        no_writes(&state,0).await;
    }).await;
    fixture.finish(result).await;
}

async fn native_metadata(app: &Router, user: i64) -> (StatusCode, Value) {
    request(
        app,
        "/api/v1/internal/beatmapsets/metadata",
        None,
        true,
        "application/json",
        json!({"user_id":user,"official_set_id":42})
            .to_string()
            .into_bytes(),
    )
    .await
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_slow_public_request_keeps_native_metadata_cache_readable_and_releases_cancelled_gate()
 {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            full_set().to_string().into_bytes(),
            Duration::ZERO,
        )
        .await;
    let fixture = MutationFixture::new("public_search_native_cache", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "NativeCachePlayer", &[]).await;
        let (status, value) = native_metadata(&app, user).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(upstream.calls(), 1);
        upstream
            .reply(
                StatusCode::OK,
                response().to_string().into_bytes(),
                Duration::from_secs(2),
            )
            .await;
        let public_app = app.clone();
        let public =
            tokio::spawn(async move { get_public_search(&public_app, "q=slow-public").await });
        upstream.wait_calls(2).await;
        let (status, value) =
            tokio::time::timeout(Duration::from_millis(750), native_metadata(&app, user))
                .await
                .expect("cached native metadata must not wait for anonymous upstream HTTP");
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["id"], 42);
        assert!(!public.is_finished());
        public.abort();
        assert!(public.await.unwrap_err().is_cancelled());
        upstream
            .reply(
                StatusCode::OK,
                response().to_string().into_bytes(),
                Duration::ZERO,
            )
            .await;
        let (status, value) = get_public_search(&app, "q=after-cancel").await;
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(upstream.calls(), 3);
        assert_eq!(upstream.state.oauth_calls.load(Ordering::SeqCst), 1);
        let permitted = Instant::now() + Duration::from_secs(30);
        let gate = {
            let mut official = state.official.lock().await;
            official.next_request = Some(permitted);
            official.request_gate.clone()
        };
        let waiting_app = app.clone();
        let waiting =
            tokio::spawn(async move { get_public_search(&waiting_app, "q=cancel-spacing").await });
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if gate.try_lock().is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned request must reach the spacing gate");
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(native_metadata(&app, user).await.0, StatusCode::OK);
        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());
        assert_eq!(
            state.official.lock().await.next_request,
            Some(permitted),
            "cancelled long spacing cannot accumulate unused reservations"
        );
        assert_eq!(upstream.calls(), 3);
        state.official.lock().await.next_request = None;
        assert_eq!(
            get_public_search(&app, "q=after-spacing-cancel").await.0,
            StatusCode::OK
        );
        assert_eq!(upstream.calls(), 4);
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_oauth_cancellation_renewal_coalescing_and_queued_retry_after() {
    let upstream = Upstream::start().await;
    *upstream.state.oauth_delay.lock().await = Duration::from_secs(2);
    let fixture = MutationFixture::new("public_search_oauth_gate", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let cancelled_app = app.clone();
        let cancelled =
            tokio::spawn(async move { get_public_search(&cancelled_app, "q=cancel-oauth").await });
        upstream.wait_oauth_calls(1).await;
        cancelled.abort();
        assert!(cancelled.await.unwrap_err().is_cancelled());
        *upstream.state.oauth_delay.lock().await = Duration::from_millis(200);
        let first_app = app.clone();
        let second_app = app.clone();
        let first =
            tokio::spawn(async move { get_public_search(&first_app, "q=renew-first").await });
        let second =
            tokio::spawn(async move { get_public_search(&second_app, "q=renew-second").await });
        assert_eq!(first.await.unwrap().0, StatusCode::OK);
        assert_eq!(second.await.unwrap().0, StatusCode::OK);
        assert_eq!(
            upstream.state.oauth_calls.load(Ordering::SeqCst),
            2,
            "one cancelled OAuth plus one shared successful renewal"
        );
        assert_eq!(upstream.calls(), 2);
        state.official.lock().await.expires = Some(Instant::now());
        upstream
            .reply(
                StatusCode::TOO_MANY_REQUESTS,
                vec![],
                Duration::from_millis(200),
            )
            .await;
        let first_app = app.clone();
        let second_app = app.clone();
        let first =
            tokio::spawn(
                async move { get_public_search(&first_app, "q=queued-limited-first").await },
            );
        let second =
            tokio::spawn(
                async move { get_public_search(&second_app, "q=queued-limited-second").await },
            );
        for result in [first.await.unwrap(), second.await.unwrap()] {
            assert_eq!(result.0, StatusCode::TOO_MANY_REQUESTS, "{}", result.1);
            assert_eq!(result.1["error"]["code"], "official_rate_limited");
        }
        assert_eq!(
            upstream.calls(),
            3,
            "queued request must observe backoff without another HTTP request"
        );
        assert_eq!(upstream.state.oauth_calls.load(Ordering::SeqCst), 3);
        assert!(
            state.official.lock().await.backoff_remaining().unwrap() > Duration::from_secs(115)
        );
        no_writes(&state, 0).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_search_archive_anchor_requires_matching_member_and_current_checksum() {
    let upstream = Upstream::start().await;
    upstream
        .reply(
            StatusCode::OK,
            json!({"beatmapsets":[full_set()],"total":1})
                .to_string()
                .into_bytes(),
            Duration::ZERO,
        )
        .await;
    let fixture =
        MutationFixture::new("public_search_archive_anchor", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let (staff,_,_,_)=actor(&state,"ArchiveAnchorStaff",&[]).await;
        let map: i64=sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,title,artist,creator,version,mode,status,source,stars,md5,osu_path) VALUES(420,42,'Retained','A','C','V',0,'ranked','official',1,$1,'private.osu') RETURNING id").bind("a".repeat(32)).fetch_one(&state.db).await.unwrap();
        let (status,value)=get_public_search(&app,"").await;
        assert_eq!(status,StatusCode::OK,"{value}");
        assert_eq!(value["items"][0]["difficulties"][0]["local_id"],map);
        assert_eq!(value["items"][0]["difficulties"][0]["archive_available"],false);
        let archive=Uuid::new_v4();
        let mut tx=state.db.begin().await.unwrap();
        sqlx::query("INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by) VALUES(42,$1,$2,$3,1,$4,$5)").bind(archive).bind(format!("official-package-{archive}/package.osz")).bind("0".repeat(64)).bind(vec!["a".repeat(32)]).bind(staff).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id) VALUES($1,$2)").bind(archive).bind(map).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let (_,value)=get_public_search(&app,"").await;
        assert_eq!(value["items"][0]["local"]["archive_available"],true);
        assert_eq!(value["items"][0]["difficulties"][0]["archive_available"],true);
        assert!(value["items"][0]["difficulties"].as_array().unwrap().iter().skip(1).all(|difficulty| difficulty["archive_available"]==false));
        let mut changed=full_set();
        changed["beatmaps"][2]["checksum"]=json!("d".repeat(32));
        upstream.reply(StatusCode::OK,json!({"beatmapsets":[changed],"total":1}).to_string().into_bytes(),Duration::ZERO).await;
        let (_,value)=get_public_search(&app,"q=updated-checksum").await;
        assert_eq!(value["items"][0]["local"]["archive_available"],true);
        assert!(value["items"][0]["difficulties"][0].get("local_id").is_none());
        assert_eq!(value["items"][0]["difficulties"][0]["archive_available"],false);
        assert_eq!(upstream.calls(),2);
        for (table,expected) in [("beatmaps",1),("official_beatmapset_archives",1),("official_beatmapset_archive_members",1),("beatmap_filename_aliases",0),("audit_events",0)] {
            let count: i64=sqlx::query_scalar(&format!("SELECT count(*) FROM {table}")).fetch_one(&state.db).await.unwrap();
            assert_eq!(count,expected,"metadata browsing must not write {table}");
        }
        assert_eq!(file_count(&state.maps_root).await,0);
    }).await;
    fixture.finish(result).await;
}
