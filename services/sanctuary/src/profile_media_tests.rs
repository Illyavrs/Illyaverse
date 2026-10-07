//! Profile media checks use a fresh schema in explicitly disposable PostgreSQL 17.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

async fn raw_request(
    app: &Router,
    method: Method,
    cookie: Option<&str>,
    origin: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri("/api/v1/me/media")
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, content_type);
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

async fn remove(app: &Router, cookie: Option<&str>, kind: &str) -> (StatusCode, Value) {
    raw_request(
        app,
        Method::DELETE,
        cookie,
        "http://localhost:3000",
        "application/json",
        json!({"kind":kind}).to_string().into_bytes(),
    )
    .await
}

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (StatusCode, Value) {
    let mut builder = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn fixture_state(db: PgPool, media_root: PathBuf) -> AppState {
    AppState {
        db,
        secret: Arc::new("profile-media-fixture-secret-at-least-thirty-two-bytes".into()),
        internal_key: Arc::new(String::new()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: media_root.clone(),
        maps_root: media_root.clone(),
        replay_root: media_root,
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    }
}

#[tokio::test]
async fn profile_media_removal_request_guards() {
    let state = fixture_state(
        PgPoolOptions::new()
            .connect_lazy("postgres://fixture@127.0.0.1:1/fixture")
            .unwrap(),
        std::env::temp_dir(),
    );
    let app = router(state.clone());
    assert_eq!(
        remove(&app, None, "avatar").await.0,
        StatusCode::UNAUTHORIZED
    );
    for (body, expected) in [
        (json!({}), StatusCode::UNPROCESSABLE_ENTITY),
        (
            json!({"kind":"background"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (json!({"kind":"Avatar"}), StatusCode::UNPROCESSABLE_ENTITY),
        (json!({"kind":null}), StatusCode::BAD_REQUEST),
        (
            json!({"kind":"avatar","user_id":1}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        state.limits.lock().await.clear();
        let (code, value) = raw_request(
            &app,
            Method::DELETE,
            None,
            "http://localhost:3000",
            "application/json",
            body.to_string().into_bytes(),
        )
        .await;
        assert_eq!(code, expected, "{body}: {value}");
    }
    assert_eq!(
        raw_request(
            &app,
            Method::DELETE,
            None,
            "https://foreign.invalid",
            "application/json",
            json!({"kind":"avatar"}).to_string().into_bytes(),
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
    );
    assert_eq!(
        raw_request(
            &app,
            Method::DELETE,
            None,
            "http://localhost:3000",
            "application/json",
            [b"{\"kind\":\"avatar\"}".as_slice(), &vec![b' '; 1024]].concat(),
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE,
    );
}

async fn player(db: &PgPool, name: &str) -> i64 {
    let id = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture','fixture') RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).fetch_one(db).await.unwrap();
    sqlx::query(
        "INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'",
    )
    .bind(id)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn session(state: &AppState, id: i64) -> (Uuid, String) {
    let sid = Uuid::new_v4();
    let expiry = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expiry)
        .execute(&state.db)
        .await
        .unwrap();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":id,"sid":sid,"exp":expiry.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (sid, format!("illya_session={token}"))
}

async fn upload(app: &Router, cookie: &str, kind: &str) -> Value {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(16, 16)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let boundary = "profile-media-fixture";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\n{kind}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"padding\"\r\n\r\n{}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fixture.png\"\r\nContent-Type: image/png\r\n\r\n", "x".repeat(2048)).into_bytes();
    body.extend_from_slice(&png.into_inner());
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let (code, value) = raw_request(
        app,
        Method::POST,
        Some(cookie),
        "http://localhost:3000",
        &format!("multipart/form-data; boundary={boundary}"),
        body,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{value}");
    assert!(value["url"].as_str().unwrap().ends_with(".webp"));
    value
}

async fn wait_for_lock(db: &PgPool, application: &str, fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(application).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("media removal must reach its authorization lock");
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to an owned disposable PostgreSQL 17"]
async fn profile_media_removal_persistence_and_queued_authorization() {
    let url =
        env::var("ILLYA_TEST_DATABASE_URL").expect("Set the disposable profile media fixture URL");
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
        "Profile media fixture must run PostgreSQL 17"
    );
    let schema = format!("illya_profile_media_{}", Uuid::new_v4().simple());
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
    let pool = db.clone();
    let application = schema.clone();
    let outcome = tokio::spawn(async move {
        let media_root = std::env::temp_dir().join(&application);
        let state = fixture_state(pool.clone(), media_root.clone());
        let app = router(state.clone());
        let owner = player(&pool, "MediaOwner").await;
        let peer = player(&pool, "MediaPeer").await;
        let (_, cookie) = session(&state, owner).await;
        let avatar = upload(&app, &cookie, "avatar").await["url"].as_str().unwrap().to_string();
        let banner = upload(&app, &cookie, "banner").await["url"].as_str().unwrap().to_string();
        sqlx::query("UPDATE users SET avatar_url=$2,banner_url=$3,bio='peer biography' WHERE id=$1")
            .bind(peer).bind(&avatar).bind(&banner).execute(&pool).await.unwrap();
        let public_path = format!("/api/v1/users/{owner}");
        let (_, initial) = get(&app, &public_path, None).await;
        assert_eq!(initial["avatar_url"], avatar);
        assert_eq!(initial["banner_url"], banner);
        assert!(initial.get("email").is_none());
        assert!(initial.get("preferences").is_none());
        assert_eq!(remove(&app, None, "avatar").await.0, StatusCode::UNAUTHORIZED);
        for (body, origin, expected) in [
            (
                json!({"kind":"avatar","user_id":peer}).to_string().into_bytes(),
                "http://localhost:3000",
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                json!({"kind":"avatar"}).to_string().into_bytes(),
                "https://foreign.invalid",
                StatusCode::FORBIDDEN,
            ),
            (
                [b"{\"kind\":\"avatar\"}".as_slice(), &vec![b' '; 1024]].concat(),
                "http://localhost:3000",
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
        ] {
            state.limits.lock().await.clear();
            assert_eq!(raw_request(&app, Method::DELETE, Some(&cookie), origin,
                "application/json", body).await.0, expected);
            let (_, unchanged) = get(&app, &public_path, None).await;
            assert_eq!(unchanged["avatar_url"], avatar);
            assert_eq!(unchanged["banner_url"], banner);
        }
        assert_eq!(remove(&app, Some(&cookie), "avatar").await,
            (StatusCode::OK, json!({"kind":"avatar","url":null})));
        for (path, auth) in [(public_path.as_str(), None), ("/api/v1/me", Some(cookie.as_str()))] {
            let (code, value) = get(&app, path, auth).await;
            assert_eq!(code, StatusCode::OK);
            assert!(value["avatar_url"].is_null());
            assert_eq!(value["banner_url"], banner);
        }
        assert_eq!(remove(&app, Some(&cookie), "avatar").await.0, StatusCode::OK);
        state.limits.lock().await.clear();
        assert_eq!(remove(&app, Some(&cookie), "banner").await,
            (StatusCode::OK, json!({"kind":"banner","url":null})));
        let (_, removed) = get(&app, &public_path, None).await;
        assert!(removed["banner_url"].is_null());
        let (_, untouched) = get(&app, &format!("/api/v1/users/{peer}"), None).await;
        assert_eq!(untouched["avatar_url"], avatar);
        assert_eq!(untouched["banner_url"], banner);
        assert_eq!(untouched["bio"], "peer biography");
        for url in [&avatar, &banner] {
            assert!(tokio::fs::try_exists(media_root.join(url.strip_prefix("/media/").unwrap())).await.unwrap(),
                "clearing a profile reference must not delete another profile's shared file");
        }

        for invalidation in ["deleted", "expired", "restricted"] {
            state.limits.lock().await.clear();
            sqlx::query("UPDATE users SET is_restricted=false,avatar_url=$2,banner_url=$3 WHERE id=$1")
                .bind(owner).bind(&avatar).bind(&banner).execute(&pool).await.unwrap();
            let (sid, cookie) = session(&state, owner).await;
            let mut blocking = pool.begin().await.unwrap();
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(owner).fetch_one(&mut *blocking).await.unwrap();
            let queued_app = app.clone();
            let queued = tokio::spawn(async move { remove(&queued_app, Some(&cookie), "avatar").await });
            wait_for_lock(&pool, &application, "SELECT id FROM users WHERE id=$1 FOR UPDATE").await;
            match invalidation {
                "deleted" => { sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap(); }
                "expired" => { sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap(); }
                "restricted" => { sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(owner).execute(&mut *blocking).await.unwrap(); }
                _ => unreachable!(),
            }
            blocking.commit().await.unwrap();
            let (code, value) = tokio::time::timeout(Duration::from_secs(5), queued).await.unwrap().unwrap();
            assert_eq!(code, StatusCode::UNAUTHORIZED, "queued removal after {invalidation}: {value}");
            let remaining: Option<String> = sqlx::query_scalar("SELECT avatar_url FROM users WHERE id=$1")
                .bind(owner).fetch_one(&pool).await.unwrap();
            assert_eq!(remaining.as_deref(), Some(avatar.as_str()));
        }

        // Expiry must also be checked after waiting on the session row itself.
        state.limits.lock().await.clear();
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(owner).execute(&pool).await.unwrap();
        let (sid, cookie) = session(&state, owner).await;
        sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1")
            .bind(sid).execute(&pool).await.unwrap();
        let mut blocking = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE").bind(sid).fetch_one(&mut *blocking).await.unwrap();
        let queued_app = app.clone();
        let queued = tokio::spawn(async move { remove(&queued_app, Some(&cookie), "banner").await });
        wait_for_lock(&pool, &application, "SELECT id FROM sessions WHERE id=$1 AND user_id=$2 FOR UPDATE").await;
        tokio::time::sleep(Duration::from_millis(1100)).await;
        blocking.commit().await.unwrap();
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), queued).await.unwrap().unwrap().0,
            StatusCode::UNAUTHORIZED);
        let remaining: Option<String> = sqlx::query_scalar("SELECT banner_url FROM users WHERE id=$1")
            .bind(owner).fetch_one(&pool).await.unwrap();
        assert_eq!(remaining.as_deref(), Some(banner.as_str()));
    }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
