//! Run with ILLYA_TEST_DATABASE_URL=<disposable PostgreSQL URL>
//! cargo test -p sanctuary persistence_and_permissions -- --ignored
//! Each run uses a new schema and never modifies public tables.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};
use std::str::FromStr;
use tower::ServiceExt;

const MAP: &str = "osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:Integration fixture\nArtist:Illyaverse\nCreator:Test\nVersion:Easy\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n";

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let request = if let Some(body) = body {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, value)
}
async fn multipart(
    app: &Router,
    path: &str,
    cookie: &str,
    name: &str,
    bytes: &[u8],
    kind: Option<&str>,
) -> (StatusCode, Value) {
    let boundary = "illya-test-boundary";
    let mut body = Vec::new();
    if let Some(kind) = kind {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\n{kind}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(header::ORIGIN, "http://localhost:3000")
                .header(header::COOKIE, cookie)
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn persistence_and_permissions() {
    let url = env::var("ILLYA_TEST_DATABASE_URL")
        .expect("Set ILLYA_TEST_DATABASE_URL to run database integration checks");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let schema = format!("illya_test_{suffix}");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let test_root = std::env::temp_dir().join(format!("illyaverse-sanctuary-test-{suffix}"));
    assert!(test_root.is_absolute() && test_root.parent() == Some(std::env::temp_dir().as_path()));
    let state = AppState {
        db: db.clone(),
        secret: Arc::new("test-only-session-secret-with-more-than-32-bytes".into()),
        internal_key: Arc::new("test-only-service-secret-with-more-than-32-bytes".into()),
        origin: Arc::new("http://localhost:3000".into()),
        secure_cookie: false,
        media_root: test_root.join("media"),
        maps_root: test_root.join("maps"),
        replay_root: test_root.join("replays"),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Arc::new(Mutex::new(HashMap::new())),
        trusted_proxies: Arc::new(Vec::new()),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    };
    for path in [&state.media_root, &state.maps_root, &state.replay_root] {
        tokio::fs::create_dir_all(path).await.unwrap();
    }
    let app = router(state.clone());
    let (s, _, oversized) = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username":"a".repeat(9*1024),"password":"small-password"})),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(oversized["error"]["code"], "payload_too_large");
    let (s, _, status) = request(&app, Method::GET, "/api/v1/status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(status["registered_players"], 0);
    let registration = json!({"username":"TestIllya","email":"illya@example.test","password":"integration-test-password","locale":"ja"});
    let (s, headers, result) = request(
        &app,
        Method::POST,
        "/api/v1/auth/register",
        None,
        Some(registration.clone()),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{result}");
    let id = result["user"]["id"].as_i64().unwrap();
    assert_eq!(result["user"]["roles"], json!(["player"]));
    assert_eq!(result["user"]["locale"], "ja");
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    assert!(
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("HttpOnly")
    );
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/register",
        None,
        Some(registration),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, error) = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username":"TestIllya","password":"wrong-password"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"]["code"], "unauthorized");
    let (s, _, user) = request(&app, Method::GET, "/api/v1/me", Some(&cookie), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(user["id"], id);
    let (s, _, _) = request(
        &app,
        Method::GET,
        "/api/v1/admin/users",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, prefs) = request(
        &app,
        Method::PATCH,
        "/api/v1/me/preferences",
        Some(&cookie),
        Some(json!({"locale":"en","background_dim":0.4})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(prefs["locale"], "en");
    // Website relationships must share the transport policy and authenticated owner.
    let (s, _, _) = request(&app, Method::GET, "/api/v1/me/relationships", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, peer_headers, peer) = request(&app, Method::POST, "/api/v1/auth/register", None,
        Some(json!({"username":"SocialPeer","email":"peer@example.test","password":"social-integration-password"}))).await;
    assert_eq!(s, StatusCode::OK);
    let peer_id = peer["user"]["id"].as_i64().unwrap();
    let peer_cookie = peer_headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let peer_path = format!("/api/v1/me/relationships/{peer_id}");
    let own_path = format!("/api/v1/me/relationships/{id}");
    let (s, _, _) = request(
        &app,
        Method::POST,
        &own_path,
        Some(&cookie),
        Some(json!({"relation_type":"friend"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = request(
        &app,
        Method::POST,
        &peer_path,
        Some(&cookie),
        Some(json!({"relation_type":"friend","user_id":peer_id})),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, _) = request(
        &app,
        Method::POST,
        &peer_path,
        Some(&cookie),
        Some(json!({"relation_type":"friend"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(
        &app,
        Method::POST,
        &own_path,
        Some(&peer_cookie),
        Some(json!({"relation_type":"friend"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, relations) = request(
        &app,
        Method::GET,
        "/api/v1/me/relationships",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(relations["friends"][0]["target_id"], peer_id);
    assert_eq!(relations["friends"][0]["mutual"], true);
    assert!(relations["friends"][0]["target"].get("email").is_none());
    let (s, _, _) = request(
        &app,
        Method::POST,
        &peer_path,
        Some(&cookie),
        Some(json!({"relation_type":"block"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, relations) = request(
        &app,
        Method::GET,
        "/api/v1/me/relationships",
        Some(&peer_cookie),
        None,
    )
    .await;
    assert_eq!(relations["friends"], json!([]));
    let (s, _, _) = request(
        &app,
        Method::POST,
        &own_path,
        Some(&peer_cookie),
        Some(json!({"relation_type":"friend"})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = request(
        &app,
        Method::DELETE,
        &format!("{own_path}?relation_type=block"),
        Some(&peer_cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, _, relations) = request(
        &app,
        Method::GET,
        "/api/v1/me/relationships",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(relations["blocks"][0]["target_id"], peer_id);
    let (s, _, _) = request(
        &app,
        Method::DELETE,
        &format!("{peer_path}?relation_type=block"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, _, relations) = request(
        &app,
        Method::GET,
        "/api/v1/me/relationships",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(relations, json!({"friends":[],"blocks":[]}));
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        "/api/v1/me/preferences",
        Some(&cookie),
        Some(json!({"background_url":"https://external.invalid/x"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri("/api/v1/me/preferences")
                .header(header::ORIGIN, "https://untrusted.invalid")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let (s, _, channels) = request(&app, Method::GET, "/api/v1/channels", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(channels["total"], 3);
    let (s, _, _) = request(
        &app,
        Method::GET,
        "/api/v1/channels/4/messages",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, message) = request(
        &app,
        Method::POST,
        "/api/v1/channels/1/messages",
        Some(&cookie),
        Some(json!({"content":"Persistent hello"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(message["username"], "TestIllya");
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(16, 16)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let (s, image) = multipart(
        &app,
        "/api/v1/me/media",
        &cookie,
        "avatar.png",
        &png.into_inner(),
        Some("avatar"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{image}");
    assert!(image["url"].as_str().unwrap().ends_with(".webp"));
    let permit_one = state.upload_permit().unwrap();
    let permit_two = state.upload_permit().unwrap();
    let (s, busy) = multipart(
        &app,
        "/api/v1/me/media",
        &cookie,
        "busy.png",
        b"not read",
        Some("avatar"),
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(busy["error"]["code"], "upload_busy");
    drop(permit_one);
    drop(permit_two);
    let (s, maps) = multipart(
        &app,
        "/api/v1/beatmaps/upload",
        &cookie,
        "test.osu",
        MAP.as_bytes(),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{maps}");
    let map_id = maps["items"][0]["id"].as_i64().unwrap();
    assert_eq!(maps["items"][0]["status"], "pending");
    let (s, _, public_map) = request(
        &app,
        Method::GET,
        &format!("/api/v1/beatmaps/{map_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(public_map.get("osu_path").is_none());
    let(s,_,_)=request(&app,Method::POST,"/api/v1/internal/scores",None,Some(json!({"user_id":id,"beatmap_id":map_id,"mode":0,"score":1000,"accuracy":100,"max_combo":2,"mods":0,"hit_counts":{"n300":2},"client":"stable"}))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let(s,_,score)=request(&app,Method::POST,"/api/v1/scores",Some(&cookie),Some(json!({"beatmap_id":map_id,"mode":0,"score":1000,"accuracy":1,"max_combo":2,"mods":0,"hit_counts":{"n300":2}}))).await;
    assert_eq!(s, StatusCode::OK, "{score}");
    let score_id = score["id"].as_i64().unwrap();
    assert_eq!(score["score_scale_status"], "unavailable");
    assert_eq!(score["score_scale_reason"], "unsupported_score_source");
    assert!(score["standardised_score"].is_null());
    assert_eq!(score["verification"], "pending");
    assert_eq!(score["accuracy"], 100);
    let (s, _, scores) = request(&app, Method::GET, "/api/v1/scores", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(scores["total"], 0);
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{score_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, score_detail) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{score_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(score_detail["hit_counts"]["n300"], 2);
    assert_eq!(score_detail["beatmap"]["id"], map_id);
    assert!(score_detail.get("replay_path").is_none());
    assert!(score_detail["beatmap"].get("osu_path").is_none());
    let mut replay = resonance_protocol::stable::Writer::new();
    replay
        .u8(0)
        .i32(20261007)
        .string(public_map["md5"].as_str().unwrap())
        .unwrap()
        .string("TestIllya")
        .unwrap()
        .string("")
        .unwrap();
    for count in [2, 0, 0, 0, 0, 0] {
        replay.u16(count);
    }
    replay
        .i32(1000)
        .u16(2)
        .boolean(true)
        .u32(0)
        .string("")
        .unwrap()
        .i64(638000000000000000)
        .i32(1);
    let mut replay = replay.into_bytes();
    replay.push(0);
    replay.extend_from_slice(&score_id.to_le_bytes());
    let (s, replay_result) = multipart(
        &app,
        &format!("/api/v1/scores/{score_id}/replay"),
        &cookie,
        "test.osr",
        &replay,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{replay_result}");
    assert_eq!(replay_result["verification"], "pending");
    let ingestion = json!({"user_id":id,"beatmap_id":map_id,"mode":0,"score":1000,"accuracy":100,"max_combo":2,"mods":0,"hit_counts":{"n300":2},"client":"stable","ingestion_key":"stable:integration-1"});
    let mut ingestion_id = None;
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/internal/scores")
                    .header("x-illya-service-key", state.internal_key.as_str())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(ingestion.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        if let Some(previous) = ingestion_id {
            assert_eq!(value["id"].as_i64(), Some(previous));
        } else {
            ingestion_id = value["id"].as_i64();
        }
    }
    let ingestion_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scores WHERE ingestion_key='stable:integration-1'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(ingestion_count, 1);
    let native = json!({"user_id":id,"beatmap_id":map_id,"mode":0,"score":1000,"accuracy":100,"max_combo":2,"mods":64,"api_mods":[{"acronym":"DT","settings":{"speed_change":1.2}}],"hit_counts":{"n300":2},"client":"lazer","ingestion_key":"lazer:integration-rate"});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/internal/scores")
                .header("x-illya-service-key", state.internal_key.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(native.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let stored: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(stored["api_mods"], native["api_mods"]);
    assert_eq!(stored["pp_eligible"], true);
    assert_eq!(stored["verification"], "pending");
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{}", stored["id"]),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let mut invalid = native.clone();
    invalid["ingestion_key"] = json!("lazer:integration-invalid");
    invalid["mods"] = json!(0);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/internal/scores")
                .header("x-illya-service-key", state.internal_key.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    invalid["api_mods"] = json!([{"acronym":"DA","settings":{"overall_difficulty":8.0}}]);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/internal/scores")
                .header("x-illya-service-key", state.internal_key.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let adjusted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(adjusted["pp_eligible"], false);
    assert_eq!(adjusted["api_mods"], invalid["api_mods"]);
    let (s, _, _) = request(&app, Method::POST, "/api/v1/scores", Some(&cookie), Some(json!({"beatmap_id":map_id,"mode":0,"score":1000,"accuracy":100,"max_combo":2,"mods":64,"api_mods":native["api_mods"],"hit_counts":{"n300":2}}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='administrator'").bind(id).execute(&db).await.unwrap();
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/beatmaps/{map_id}"),
        Some(&cookie),
        Some(json!({"status":"ranked"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/scores/{score_id}"),
        Some(&cookie),
        Some(json!({"verification":"verified"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, leaders) =
        request(&app, Method::GET, "/api/v1/leaderboard?mode=0", None, None).await;
    assert_eq!(s, StatusCode::OK, "{leaders}");
    assert_eq!(leaders["items"][0]["id"], id);
    assert!(leaders["items"][0]["pp"].as_f64().unwrap() > 0.0);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{id}"),
        Some(&cookie),
        Some(json!({"roles":[1]})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, audit) = request(
        &app,
        Method::GET,
        "/api/v1/admin/audit",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(audit["total"].as_i64().unwrap() >= 3);
    let(s,headers,manager)=request(&app,Method::POST,"/api/v1/auth/register",None,Some(json!({"username":"RoleManager","email":"manager@example.test","password":"manager-test-password"}))).await;
    assert_eq!(s, StatusCode::OK);
    let manager_id = manager["user"]["id"].as_i64().unwrap();
    let manager_cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{}", ingestion_id.unwrap()),
        Some(&manager_cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let manager_role:i64=sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES('limited_manager',ARRAY['roles.manage','users.moderate','admin.view']) RETURNING id").fetch_one(&db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
        .bind(manager_id)
        .bind(manager_role)
        .execute(&db)
        .await
        .unwrap();
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/admin/roles",
        Some(&manager_cookie),
        Some(json!({"name":"forged_admin","description":"must fail","permissions":["*"]})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{manager_id}"),
        Some(&manager_cookie),
        Some(json!({"roles":[3]})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let(s,_,_)=request(&app,Method::PATCH,"/api/v1/admin/roles/3",Some(&manager_cookie),Some(json!({"name":"administrator","description":"must fail","permissions":["roles.manage"]}))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{id}"),
        Some(&manager_cookie),
        Some(json!({"is_restricted":true})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let(s,_,manager_score)=request(&app,Method::POST,"/api/v1/scores",Some(&manager_cookie),Some(json!({"beatmap_id":map_id,"mode":0,"score":900,"accuracy":50,"max_combo":1,"mods":0,"hit_counts":{"n300":1,"misses":1}}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!(
            "/api/v1/admin/scores/{}",
            manager_score["id"].as_i64().unwrap()
        ),
        Some(&cookie),
        Some(json!({"verification":"verified"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(
        &app,
        Method::PATCH,
        &format!("/api/v1/admin/users/{manager_id}"),
        Some(&cookie),
        Some(json!({"is_restricted":true})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/users/{manager_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, hidden_scores) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores?user_id={manager_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(hidden_scores["total"], 0);
    let restricted_score_id = manager_score["id"].as_i64().unwrap();
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{restricted_score_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/scores/{restricted_score_id}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, headers, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username":"TestIllya","password":"integration-test-password"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let other_cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at) VALUES($1,'test-bancho-token',$2,now()+interval '3 minutes')").bind(uuid::Uuid::new_v4()).bind(id).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,'test-access','test-refresh',now()+interval '1 hour',now()+interval '1 day')").bind(uuid::Uuid::new_v4()).bind(id).execute(&db).await.unwrap();
    let (s, _, online) = request(&app, Method::GET, "/api/v1/status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(online["players_online"], 1);
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/me/password",
        Some(&cookie),
        Some(
            json!({"current_password":"wrong-password","new_password":"replacement-test-password"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let(s,_,_)=request(&app,Method::POST,"/api/v1/me/password",Some(&cookie),Some(json!({"current_password":"integration-test-password","new_password":"replacement-test-password"}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(&app, Method::GET, "/api/v1/me", Some(&other_cookie), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = request(&app, Method::GET, "/api/v1/me", Some(&cookie), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, online) = request(&app, Method::GET, "/api/v1/status", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(online["players_online"], 0);
    let legacy: String = sqlx::query_scalar("SELECT password_md5_hash FROM users WHERE id=$1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    use argon2::PasswordVerifier;
    assert!(
        argon2::Argon2::default()
            .verify_password(
                format!("{:x}", md5::compute("replacement-test-password")).as_bytes(),
                &argon2::PasswordHash::new(&legacy).unwrap()
            )
            .is_ok()
    );
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username":"TestIllya","password":"integration-test-password"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/login",
        None,
        Some(json!({"username":"TestIllya","password":"replacement-test-password"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let private_channel:i64 = sqlx::query_scalar("INSERT INTO channels(name,description,required_permission) VALUES('private-room-test','Private test','lazer.room') RETURNING id").fetch_one(&db).await.unwrap();
    let authenticated = auth::require(&state, &{
        let mut h = axum::http::HeaderMap::new();
        h.insert(header::COOKIE, cookie.parse().unwrap());
        h
    })
    .await
    .unwrap();
    authenticated.require("*").unwrap();
    let (_, _, public_channels) =
        request(&app, Method::GET, "/api/v1/channels", Some(&cookie), None).await;
    assert!(
        !public_channels["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == private_channel)
    );
    let (s, _, _) = request(
        &app,
        Method::GET,
        &format!("/api/v1/channels/{private_channel}/messages"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = request(
        &app,
        Method::POST,
        &format!("/api/v1/channels/{private_channel}/messages"),
        Some(&cookie),
        Some(json!({"content":"Must stay private"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = request(
        &app,
        Method::POST,
        "/api/v1/auth/logout",
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = request(&app, Method::GET, "/api/v1/me", Some(&cookie), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chat_messages WHERE content='Persistent hello'")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(count, 1);
    drop(app);
    drop(state);
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    tokio::fs::remove_dir_all(&test_root).await.unwrap();
}

async fn internal_score(app: &Router, state: &AppState, input: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/internal/scores")
                .header("x-illya-service-key", state.internal_key.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    (status, value)
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn score_scale_ingestion_recovery_and_privacy() {
    let url = env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let schema = format!("score_recovery_{suffix}");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let root = std::env::temp_dir().join(format!("illyaverse-score-recovery-{suffix}"));
    assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
    let state = AppState {
        db: db.clone(),
        secret: Arc::new("fixture-session-secret-with-more-than-32-bytes".into()),
        internal_key: Arc::new("fixture-service-secret-with-more-than-32-bytes".into()),
        origin: Arc::new("http://localhost:3000".into()),
        secure_cookie: false,
        media_root: root.join("media"),
        maps_root: root.join("maps"),
        replay_root: root.join("replays"),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Arc::new(Mutex::new(HashMap::new())),
        trusted_proxies: Arc::new(vec![]),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    };
    tokio::fs::create_dir_all(&state.maps_root).await.unwrap();
    tokio::fs::write(state.maps_root.join("fixture.osu"), MAP)
        .await
        .unwrap();
    let app = router(state.clone());
    let (status,headers,registration)=request(&app,Method::POST,"/api/v1/auth/register",None,
        Some(json!({"username":"ScoreRecovery","email":"recovery@invalid.test","password":"score-recovery-fixture-password"}))).await;
    assert_eq!(status, StatusCode::OK);
    let user = registration["user"]["id"].as_i64().unwrap();
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let map:i64=sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,stars,max_combo,md5,osu_path) VALUES('Fixture','Illyaverse','Test','Two circles',0,1,2,'recovery-fixture','fixture.osu') RETURNING id").fetch_one(&db).await.unwrap();
    let context = json!({"ruleset_id":0,"passed":true,"total_score":1000000,"total_score_without_mods":1000000,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]});
    let native = json!({"user_id":user,"beatmap_id":map,"mode":0,"score":1000000,"accuracy":100,"max_combo":2,"mods":0,"api_mods":[],"hit_counts":{"n300":2},"client":"lazer","native_submission":context,"ingestion_key":"lazer:scale-context"});
    let (status, stored) = internal_score(&app, &state, native.clone()).await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    let id = stored["id"].as_i64().unwrap();
    assert_eq!(stored["native_submission"], context);
    assert_eq!(stored["score"], 1000000);
    assert_eq!(stored["classic_score"], 100130);
    assert_eq!(stored["standardised_score"], 1000000);
    assert_eq!(stored["score_scale_status"], "native_reported");
    assert_eq!(stored["verification"], "pending");
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/scores/{id}"),
            None,
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/scores/{id}"),
            Some(cookie),
            None
        )
        .await
        .2["native_submission"],
        context
    );
    let mut forged = native.clone();
    forged["ingestion_key"] = json!("lazer:scale-forgery");
    forged["native_submission"]["maximum_statistics"] = json!({"great":1000});
    assert_eq!(
        internal_score(&app, &state, forged).await.0,
        StatusCode::BAD_REQUEST
    );
    let mut web = native.clone();
    web["client"] = json!("api");
    web.as_object_mut().unwrap().remove("ingestion_key");
    assert_eq!(
        request(
            &app,
            Method::POST,
            "/api/v1/scores",
            Some(cookie),
            Some(web)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Historical context exists only on a token. Recovery persists it before a
    // token can be removed, and deliberately preserves raw score/PP/review.
    let old:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,hit_counts,client,verification) VALUES($1,$2,0,1000000,100,2,666,'{\"n300\":2}','lazer','verified') RETURNING id").bind(user).bind(map).fetch_one(&db).await.unwrap();
    sqlx::query("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,score_id,submission) VALUES($1,$2,0,'fixture',now()+interval '1 hour',$3,$4)").bind(user).bind(map).bind(old).bind(&context).execute(&db).await.unwrap();
    let mut invalid = context.clone();
    invalid["maximum_statistics"] = json!({"great":1000});
    let bad:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,hit_counts,client,native_submission) VALUES($1,$2,0,1000000,100,2,222,'{\"n300\":2}','lazer',$3) RETURNING id").bind(user).bind(map).bind(invalid).fetch_one(&db).await.unwrap();
    for _ in 0..17 {
        sqlx::query("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,pp,hit_counts,client) VALUES($1,$2,0,10,100,2,0,'{\"n300\":2}','api')").bind(user).bind(map).execute(&db).await.unwrap();
    }
    assert_eq!(score_recovery::recover_batch(&state).await.unwrap(), 16);
    assert_eq!(score_recovery::recover_batch(&state).await.unwrap(), 3);
    assert_eq!(score_recovery::recover_batch(&state).await.unwrap(), 0);
    let restored: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM scores s WHERE id=$1")
        .bind(old)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(restored["native_submission"], context);
    assert_eq!(restored["classic_score"], 100130);
    assert_eq!(restored["pp"], 666.0);
    assert_eq!(restored["score"], 1000000);
    assert_eq!(restored["verification"], "verified");
    sqlx::query("DELETE FROM lazer_score_tokens WHERE score_id=$1")
        .bind(old)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::GET,
            &format!("/api/v1/scores/{old}"),
            None,
            None
        )
        .await
        .2["native_submission"],
        context
    );
    let unsupported: Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM scores s WHERE id=$1")
        .bind(bad)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(unsupported["score_scale_status"], "unavailable");
    assert!(unsupported["standardised_score"].is_null());
    assert_eq!(
        unsupported["score_scale_version"],
        "recovery_unavailable.v1"
    );
    assert_eq!(unsupported["verification"], "pending");
    assert_eq!(unsupported["pp"], 222.0);
    drop(app);
    drop(state);
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    tokio::fs::remove_dir_all(&root).await.unwrap();
}
