//! Read-only official set fixtures use isolated PostgreSQL 17 and loopback HTTP.
use crate::{
    maps::map_mutation_tests::{MutationFixture, actor, file_count},
    native_map_tests::OfficialServer,
    *,
};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use illya_domain::official_set::OfficialSetMetadata;
use tower::ServiceExt;

const PATH: &str = "/api/v1/internal/beatmapsets/metadata";

fn metadata(set_id: i64) -> Value {
    json!({"id":set_id,"artist":"Artist","artist_unicode":"歌手","title":"Title","title_unicode":"標題","creator":"Creator","status":"approved","bpm":180.5,"source":"","tags":"metadata fixture","nsfw":false,"submitted_date":"2020-01-02T03:04:05Z","last_updated":"2020-02-02T03:04:05Z","ranked_date":null,"genre":{"id":5,"name":"Video Game"},"language":{"id":3,"name":"Japanese"},"beatmaps":[{"id":set_id*10,"beatmapset_id":set_id,"checksum":format!("{:032x}",set_id),"version":"Hard","mode_int":0,"mode":"osu","status":"ranked","last_updated":"2020-02-02T03:04:05Z","difficulty_rating":5.25,"bpm":180.5,"total_length":150,"hit_length":135,"max_combo":432,"ar":9.5,"cs":4.0,"accuracy":8.5,"drain":6.0,"count_circles":100,"count_sliders":140,"count_spinners":2}],"covers":{"cover":"https://example.invalid/private"},"user_id":999,"converts":[{"id":9999999999_i64}]})
}

async fn raw(app: &Router, key: Option<&str>, body: String) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(PATH)
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
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn get(app: &Router, state: &AppState, user: i64, set_id: i64) -> (StatusCode, Value) {
    raw(
        app,
        Some(state.internal_key.as_str()),
        json!({"user_id":user,"official_set_id":set_id}).to_string(),
    )
    .await
}
async fn no_catalog_writes(state: &AppState) {
    for table in [
        "beatmaps",
        "beatmap_filename_aliases",
        "official_beatmapset_archives",
        "official_beatmapset_archive_members",
        "audit_events",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(count, 0, "{table} must remain empty");
    }
    assert_eq!(file_count(&state.maps_root).await, 0);
}

#[tokio::test]
async fn official_set_metadata_request_guards() {
    let state = AppState {
        db: PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy("postgres://fixture@127.0.0.1:1/fixture")
            .unwrap(),
        secret: Arc::new("metadata-fixture-session-secret-at-least-thirty-two-bytes".into()),
        internal_key: Arc::new("metadata-fixture-service-key-at-least-thirty-two-bytes".into()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: std::env::temp_dir(),
        maps_root: std::env::temp_dir(),
        replay_root: std::env::temp_dir(),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        chat_runtime: Default::default(),
    };
    let app = router(state.clone());
    for key in [None, Some("wrong")] {
        let (status, _) =
            tokio::time::timeout(Duration::from_secs(1), raw(&app, key, "not JSON".into()))
                .await
                .unwrap();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    for value in [
        json!({}),
        json!({"user_id":0,"official_set_id":42}),
        json!({"user_id":1,"official_set_id":0}),
        json!({"user_id":1,"official_set_id":null}),
        json!({"user_id":1,"official_set_id":"42"}),
        json!({"user_id":1,"official_set_id":42,"url":"http://127.0.0.1/"}),
        json!({"user_id":1,"official_set_id":2_000_000_000}),
    ] {
        let (status, _) = tokio::time::timeout(
            Duration::from_secs(1),
            raw(&app, Some(&state.internal_key), value.to_string()),
        )
        .await
        .unwrap();
        assert!(matches!(
            status,
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT
        ));
    }
    let (status, _) = raw(
        &app,
        Some(&state.internal_key),
        r#"{"user_id":1,"official_set_id":42,"official_set_id":43}"#.into(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = raw(&app, Some(&state.internal_key), " ".repeat(4097)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_cache_auth_and_read_only() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(42, metadata(42)).await;
    let fixture = MutationFixture::new("set_metadata_cache", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataCache", &[]).await;
        assert_eq!(
            get(&app, &state, user + 9999, 42).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert!(upstream.set_queries().await.is_empty());
        for _ in 0..2 {
            let (status, value) = get(&app, &state, user, 42).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            let dto: OfficialSetMetadata = serde_json::from_value(value.clone()).unwrap();
            dto.validate(42).unwrap();
            assert_eq!(dto.beatmaps[0].od, 8.5);
            assert!(value.get("user_id").is_none());
            assert!(value.get("covers").is_none());
        }
        assert_eq!(upstream.set_queries().await, vec![42]);
        assert_eq!(upstream.counters(), [1, 0, 0, 0]);
        let (keys, bytes, ttl) = state.official.lock().await.metadata_cache_shape();
        assert_eq!(keys, 1);
        assert!(bytes > 0);
        assert!(ttl.unwrap() > Duration::from_secs(295));
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(user)
            .execute(&state.db)
            .await
            .unwrap();
        assert_eq!(
            get(&app, &state, user, 42).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(upstream.set_queries().await, vec![42]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_singleflight_rechecks_both_actors() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(43, metadata(43)).await;
    let fixture = MutationFixture::new("set_metadata_flight", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (first, _, _, _) = actor(&state, "MetadataFirst", &[]).await;
        let (second, _, _, _) = actor(&state, "MetadataSecond", &[]).await;
        let pause = upstream.pause_next_set().await;
        let first_state = state.clone();
        let first_app = app.clone();
        let one = tokio::spawn(async move { get(&first_app, &first_state, first, 43).await });
        pause.wait_entered().await;
        let second_state = state.clone();
        let second_app = app.clone();
        let two = tokio::spawn(async move { get(&second_app, &second_state, second, 43).await });
        // The shared upstream mutex is already held by the first fetch. The
        // second request must reauthorize even if restriction happens while it waits.
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=ANY($1)")
            .bind(vec![first, second])
            .execute(&state.db)
            .await
            .unwrap();
        pause.release();
        assert_eq!(one.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert_eq!(two.await.unwrap().0, StatusCode::UNAUTHORIZED);
        assert_eq!(upstream.set_queries().await, vec![43]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_negative_cache_and_rate_backoff() {
    let upstream = OfficialServer::start().await;
    let fixture = MutationFixture::new("set_metadata_negative", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataNegative", &[]).await;
        for _ in 0..2 {
            assert_eq!(get(&app, &state, user, 44).await.0, StatusCode::NOT_FOUND);
        }
        assert_eq!(upstream.set_queries().await, vec![44]);
        state.official.lock().await.expire_set_failure(44);
        upstream.add_set(44, metadata(44)).await;
        assert_eq!(get(&app, &state, user, 44).await.0, StatusCode::OK);
        upstream.set_rate_limits(1, "120").await;
        let started = Instant::now();
        let (status, value) = get(&app, &state, user, 45).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            state.official.lock().await.backoff_remaining().unwrap() > Duration::from_secs(115)
        );
        assert_eq!(
            get(&app, &state, user, 45).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            get(&app, &state, user, 46).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(upstream.set_queries().await, vec![44, 44, 45]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_invalid_upstream_and_positive_expiry() {
    let upstream = OfficialServer::start().await;
    let fixture = MutationFixture::new("set_metadata_invalid", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataInvalid", &[]).await;
        for (index, value) in [
            json!({"id":47}),
            {
                let mut value = metadata(48);
                value["beatmaps"][0]["convert"] = json!(true);
                value
            },
            {
                let mut value = metadata(49);
                value["beatmaps"][0]["beatmapset_id"] = json!(99);
                value
            },
        ]
        .into_iter()
        .enumerate()
        {
            let id = 47 + index as i64;
            upstream.add_set(id, value).await;
            for _ in 0..2 {
                let (status, value) = get(&app, &state, user, id).await;
                assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
                assert_eq!(value["error"]["code"], "official_identity_mismatch");
            }
        }
        assert_eq!(upstream.set_queries().await, vec![47, 48, 49]);
        upstream.add_set(50, metadata(50)).await;
        assert_eq!(get(&app, &state, user, 50).await.0, StatusCode::OK);
        state.official.lock().await.expire_set_metadata(50);
        let mut changed = metadata(50);
        changed["title"] = json!("Updated title");
        upstream.add_set(50, changed).await;
        assert_eq!(
            get(&app, &state, user, 50).await.1["title"],
            "Updated title"
        );
        assert_eq!(upstream.set_queries().await, vec![47, 48, 49, 50, 50]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_cache_key_and_byte_budgets() {
    let upstream = OfficialServer::start().await;
    let fixture = MutationFixture::new("set_metadata_budget", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataBudget", &[]).await;
        for id in 100..134 {
            upstream.add_set(id, metadata(id)).await;
            assert_eq!(get(&app, &state, user, id).await.0, StatusCode::OK);
        }
        let (keys, bytes, _) = state.official.lock().await.metadata_cache_shape();
        assert_eq!(keys, 32);
        assert!(bytes <= 16 * 1024 * 1024);
        upstream.add_set(100, metadata(100)).await;
        assert_eq!(get(&app, &state, user, 100).await.0, StatusCode::OK);
        assert_eq!(
            upstream
                .set_queries()
                .await
                .iter()
                .filter(|id| **id == 100)
                .count(),
            2
        );
        // Each normalized large set is still below its 2 MiB boundary. A
        // smaller number of large entries must reach the 16 MiB aggregate cap
        // before the 32-key cap, without growing an unbounded metadata cache.
        for set_id in 200..224 {
            let mut large = metadata(set_id);
            let member = large["beatmaps"][0].clone();
            large["beatmaps"] = json!(
                (0..512)
                    .map(|offset| {
                        let mut map = member.clone();
                        map["id"] = json!(set_id * 1000 + offset);
                        map["checksum"] = json!(format!("{:032x}", set_id * 1000 + offset));
                        map["version"] = json!("v".repeat(1024));
                        map
                    })
                    .collect::<Vec<_>>()
            );
            upstream.add_set(set_id, large).await;
            let (status, value) = get(&app, &state, user, set_id).await;
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["beatmaps"].as_array().unwrap().len(), 512);
            let (keys, bytes, _) = state.official.lock().await.metadata_cache_shape();
            assert!(keys <= 32);
            assert!(bytes <= 16 * 1024 * 1024);
        }
        let (keys, bytes, _) = state.official.lock().await.metadata_cache_shape();
        assert!(
            keys < 32,
            "serialized byte budget must evict before key budget"
        );
        assert!(bytes > 15 * 1024 * 1024 && bytes <= 16 * 1024 * 1024);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_upstream_size_and_deadline() {
    let upstream = OfficialServer::start().await;
    let fixture = MutationFixture::new("set_metadata_deadline", upstream.origin.clone()).await;
    let mut state = fixture.state.clone();
    state.client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let app = router(state.clone());
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataDeadline", &[]).await;
        let mut large = metadata(60);
        large["discarded_untrusted_field"] = json!("x".repeat(4 * 1024 * 1024));
        upstream.add_set(60, large).await;
        for _ in 0..2 {
            let (status, value) = get(&app, &state, user, 60).await;
            assert_eq!(status, StatusCode::BAD_GATEWAY, "{value}");
            assert_eq!(value["error"]["code"], "official_api_failed");
        }
        assert_eq!(upstream.set_queries().await, vec![60]);
        upstream.add_set(61, metadata(61)).await;
        let pause = upstream.pause_next_set().await;
        let request_state = state.clone();
        let request_app = app.clone();
        let started = Instant::now();
        let request =
            tokio::spawn(async move { get(&request_app, &request_state, user, 61).await });
        pause.wait_entered().await;
        let (status, value) = request.await.unwrap();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
        assert_eq!(value["error"]["code"], "official_set_metadata_timeout");
        assert!(
            started.elapsed() >= Duration::from_secs(6)
                && started.elapsed() < Duration::from_secs(8)
        );
        assert_eq!(
            get(&app, &state, user, 61).await.1["error"]["code"],
            "official_set_metadata_timeout"
        );
        pause.release();
        assert_eq!(upstream.set_queries().await, vec![60, 61]);
        // A different official operation may own the shared rate mutex. The
        // metadata deadline must still finish without blocking to cache its
        // timeout behind that operation.
        upstream.add_set(62, metadata(62)).await;
        let guard = state.official.lock().await;
        let started = Instant::now();
        let (status, value) = get(&app, &state, user, 62).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{value}");
        assert_eq!(value["error"]["code"], "official_set_metadata_timeout");
        assert!(
            started.elapsed() >= Duration::from_secs(6)
                && started.elapsed() < Duration::from_secs(8)
        );
        drop(guard);
        assert_eq!(get(&app, &state, user, 62).await.0, StatusCode::OK);
        assert_eq!(upstream.set_queries().await, vec![60, 61, 62]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_concurrent_discovery_and_shared_slots() {
    let upstream = OfficialServer::start().await;
    upstream.add_set(80, metadata(80)).await;
    let fixture = MutationFixture::new("set_metadata_concurrent", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result = tokio::spawn(async move {
        let (user, _, _, _) = actor(&state, "MetadataConcurrent", &[]).await;
        let pause = upstream.pause_next_set().await;
        let first_state = state.clone();
        let first_app = app.clone();
        let first = tokio::spawn(async move { get(&first_app, &first_state, user, 80).await });
        pause.wait_entered().await;
        let second_state = state.clone();
        let second_app = app.clone();
        let second = tokio::spawn(async move { get(&second_app, &second_state, user, 80).await });
        let third_state = state.clone();
        let third_app = app.clone();
        let third = tokio::spawn(async move { get(&third_app, &third_state, user, 80).await });
        pause.release();
        for task in [first, second, third] {
            let (status, value) = task.await.unwrap();
            assert_eq!(status, StatusCode::OK, "{value}");
            assert_eq!(value["id"], 80);
        }
        assert_eq!(upstream.set_queries().await, vec![80]);
        let slots = state.official.lock().await.resolution_slots();
        let mut permits = Vec::new();
        for _ in 0..4 {
            permits.push(slots.clone().try_acquire_owned().unwrap());
        }
        assert_eq!(slots.available_permits(), 0);
        let (status, value) = get(&app, &state, user, 81).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{value}");
        assert_eq!(value["error"]["code"], "official_resolution_busy");
        assert_eq!(upstream.set_queries().await, vec![80]);
        drop(permits);
        upstream.add_set(81, metadata(81)).await;
        assert_eq!(get(&app, &state, user, 81).await.0, StatusCode::OK);
        assert_eq!(upstream.set_queries().await, vec![80, 81]);
        no_catalog_writes(&state).await;
    })
    .await;
    fixture.finish(result).await;
}

#[tokio::test]
#[ignore = "requires an owned disposable PostgreSQL 17 database"]
async fn official_set_metadata_failures_do_not_poison_archive_identity_lookup() {
    let upstream = OfficialServer::start().await;
    let fixture = MutationFixture::new("set_metadata_archive", upstream.origin.clone()).await;
    let state = fixture.state.clone();
    let app = fixture.app.clone();
    let result=tokio::spawn(async move {
        let (user,_,_,_)=actor(&state,"MetadataArchive",&[]).await;
        // This minimal response is sufficient to verify an archive manifest,
        // but it cannot populate the richer native metadata DTO.
        upstream.add_set(70,json!({"id":70,"beatmaps":[{"id":700,"beatmapset_id":70,"checksum":"a".repeat(32),"mode_int":0,"status":"ranked"}]})).await;
        let (status,value)=get(&app,&state,user,70).await;
        assert_eq!(status,StatusCode::BAD_GATEWAY,"{value}");
        assert_eq!(value["error"]["code"],"official_identity_mismatch");
        assert_eq!(maps::archive_metadata_member_count(&state,70).await.unwrap(),1);
        // The native metadata failure remains cached in its separate key.
        assert_eq!(get(&app,&state,user,70).await.0,StatusCode::BAD_GATEWAY);
        assert_eq!(upstream.set_queries().await,vec![70,70]);
        no_catalog_writes(&state).await;
    }).await;
    fixture.finish(result).await;
}
