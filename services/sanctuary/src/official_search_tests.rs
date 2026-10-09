use super::*;
use crate::maps::map_mutation_tests::{MutationFixture, actor, file_count};
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
        };
        let app = Router::new()
            .route(
                "/oauth/token",
                post(|| async {
                    Json(json!({"access_token":"owned-fixture-public","expires_in":3600}))
                }),
            )
            .route("/api/v2/beatmapsets/search", get(upstream_search))
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
    assert_eq!(
        headers[header::AUTHORIZATION],
        "Bearer owned-fixture-public"
    );
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.queries.lock().await.push(query.unwrap_or_default());
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
    let mut builder = Request::builder().uri(format!("/api/v1/beatmaps/official/search?{query}"));
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
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
