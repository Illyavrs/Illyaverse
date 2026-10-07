//! Original profile checks follow the existing disposable-schema, router,
//! synthetic-session and observed-lock fixtures. No media files are required.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request},
};
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tower::ServiceExt;
use uuid::Uuid;

fn fixture_state(db: PgPool, root: PathBuf) -> AppState {
    AppState {
        db,
        secret: Arc::new("profile-presentation-fixture-secret-over-thirty-two-bytes".into()),
        internal_key: Arc::new(String::new()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: root.clone(),
        maps_root: root.clone(),
        replay_root: root,
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        chat_runtime: Default::default(),
    }
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let code = response.status();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn patch(app: &Router, cookie: Option<&str>, body: Value) -> (StatusCode, Value) {
    request(app, Method::PATCH, "/api/v1/me/profile", cookie, Some(body)).await
}

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> Value {
    let (code, body) = request(app, Method::GET, path, cookie, None).await;
    assert_eq!(code, StatusCode::OK, "{path}: {body}");
    body
}

fn invalid_patches() -> Vec<(Value, StatusCode)> {
    let mut patches = Vec::new();
    for field in ["banner_position_x", "banner_position_y"] {
        for value in [json!(-1), json!(101), json!(i32::MIN), json!(i32::MAX)] {
            patches.push((
                json!({field:value,"bio":"must not persist"}),
                StatusCode::BAD_REQUEST,
            ));
        }
        for value in [
            json!(12.5),
            json!(50.0),
            json!("50"),
            json!(true),
            json!([]),
            json!({}),
            json!(i64::from(i32::MAX) + 1),
            json!(i64::from(i32::MIN) - 1),
        ] {
            patches.push((
                json!({field:value,"bio":"must not persist"}),
                StatusCode::UNPROCESSABLE_ENTITY,
            ));
        }
    }
    for field in [
        "user_id",
        "preferences",
        "locale",
        "background_url",
        "avatar_url",
        "banner_url",
    ] {
        patches.push((
            json!({field:1,"banner_position_x":22}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ));
    }
    patches
}

#[tokio::test]
async fn profile_presentation_request_types() {
    let state = fixture_state(
        PgPoolOptions::new()
            .connect_lazy("postgres://fixture@127.0.0.1:1/fixture")
            .unwrap(),
        std::env::temp_dir(),
    );
    let app = router(state.clone());
    for (body, expected) in invalid_patches()
        .into_iter()
        .filter(|(_, status)| *status == StatusCode::UNPROCESSABLE_ENTITY)
    {
        state.limits.lock().await.clear();
        let (code, value) = patch(&app, None, body.clone()).await;
        assert_eq!(code, expected, "{body}: {value}");
    }
    state.limits.lock().await.clear();
    assert_eq!(
        patch(&app, None, json!({"banner_position_x":50})).await.0,
        StatusCode::UNAUTHORIZED,
    );
}

async fn player(db: &PgPool, name: &str) -> i64 {
    let id = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'synthetic','synthetic') RETURNING id")
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

async fn presentation(db: &PgPool, id: i64) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('bio',bio,'country',country,'accent_color',accent_color,'banner_position_x',banner_position_x,'banner_position_y',banner_position_y,'avatar_url',avatar_url,'banner_url',banner_url,'preferences',preferences) FROM users WHERE id=$1")
        .bind(id).fetch_one(db).await.unwrap()
}

fn assert_public(profile: &Value, accent: &str, x: i32, y: i32) {
    assert_eq!(profile["accent_color"], accent);
    assert_eq!(profile["banner_position_x"], x);
    assert_eq!(profile["banner_position_y"], y);
    for private in [
        "email",
        "email_verified",
        "permissions",
        "preferences",
        "locale",
        "background_url",
        "background_dim",
        "background_blur",
        "panel_blur",
        "panel_opacity",
        "border_radius",
        "mode",
    ] {
        assert!(
            profile.get(private).is_none(),
            "public profile leaked {private}"
        );
    }
}

async fn wait_for_lock(db: &PgPool, application: &str, fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)")
                .bind(application).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("profile patch must reach its authorization lock");
}

async fn fixture_checks(db: PgPool, application: String) {
    // Exercise the forward upgrade with an existing customized player before
    // testing the new-user defaults, rather than only creating post-upgrade rows.
    let migrator = sqlx::migrate!("../../migrations");
    for migration in migrator.iter().filter(|migration| migration.version < 19) {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    let owner = player(&db, "PresentationOwner").await;
    let preferences = json!({"locale":"ja","mode":"light","accent_color":"#abcdef","background_url":"/media/background/private.webp","background_dim":0.5,"background_blur":12,"panel_blur":8,"panel_opacity":0.625,"border_radius":18,"pm_friends_only":true});
    sqlx::query("UPDATE users SET bio='Original biography',country='AU',accent_color='#123456',avatar_url='/media/avatar/retained.webp',banner_url='/media/banner/retained.webp',email_verified_at=now(),preferences=$2 WHERE id=$1")
        .bind(owner).bind(&preferences).execute(&db).await.unwrap();
    let upgrade = migrator
        .iter()
        .find(|migration| migration.version == 19)
        .unwrap();
    sqlx::raw_sql(&upgrade.sql).execute(&db).await.unwrap();
    let upgraded = presentation(&db, owner).await;
    assert_eq!(upgraded["banner_position_x"], 50);
    assert_eq!(upgraded["banner_position_y"], 50);
    assert_eq!(upgraded["preferences"], preferences);
    assert_eq!(upgraded["accent_color"], "#123456");
    // Preserve the explicit presentation upgrade check above, then bring the
    // fixture to the schema used by the current profile/statistics handlers.
    for migration in migrator.iter().filter(|migration| migration.version > 19) {
        sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
    }
    assert_eq!(presentation(&db, owner).await, upgraded);
    let peer = player(&db, "PresentationPeer").await;
    let peer_initial = presentation(&db, peer).await;
    assert_eq!(peer_initial["banner_position_x"], 50);
    assert_eq!(peer_initial["banner_position_y"], 50);

    // The storage boundary also protects writes outside the HTTP handler.
    for column in ["banner_position_x", "banner_position_y"] {
        for (value, expected) in [("-1", "23514"), ("101", "23514"), ("NULL", "23502")] {
            let error = sqlx::query(&format!("UPDATE users SET {column}={value} WHERE id=$1"))
                .bind(owner)
                .execute(&db)
                .await
                .unwrap_err();
            assert_eq!(
                error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(presentation(&db, owner).await, upgraded);
        }
    }

    let state = fixture_state(db.clone(), std::env::temp_dir().join(&application));
    let app = router(state.clone());
    let (_, owner_cookie) = session(&state, owner).await;
    let (_, peer_cookie) = session(&state, peer).await;
    let public_path = format!("/api/v1/users/{owner}");
    for cookie in [
        None,
        Some(peer_cookie.as_str()),
        Some(owner_cookie.as_str()),
    ] {
        assert_public(&get(&app, &public_path, cookie).await, "#123456", 50, 50);
    }
    let private = get(&app, "/api/v1/me", Some(&owner_cookie)).await;
    assert_eq!(private["preferences"], preferences);
    assert_eq!(private["locale"], "ja");
    assert_eq!(private["email"], "PresentationOwner@example.test");
    assert_eq!(private["email_verified"], true);
    assert!(!private["permissions"].as_array().unwrap().is_empty());
    assert_eq!(private["banner_position_x"], 50);
    assert_eq!(private["banner_position_y"], 50);
    assert_eq!(private["accent_color"], "#123456");

    let (code, saved) = patch(&app, Some(&owner_cookie), json!({"bio":"保存された biography","country":"JP","accent_color":"#13579B","banner_position_x":0,"banner_position_y":100})).await;
    assert_eq!(code, StatusCode::OK, "{saved}");
    assert_eq!(saved["bio"], "保存された biography");
    assert_eq!(saved["country"], "JP");
    assert_eq!(saved["accent_color"], "#13579B");
    assert_eq!(saved["banner_position_x"], 0);
    assert_eq!(saved["banner_position_y"], 100);
    assert_eq!(saved["preferences"], preferences);
    assert_eq!(saved["avatar_url"], upgraded["avatar_url"]);
    assert_eq!(saved["banner_url"], upgraded["banner_url"]);
    assert_eq!(presentation(&db, peer).await, peer_initial);

    state.limits.lock().await.clear();
    assert_eq!(
        patch(&app, Some(&owner_cookie), json!({"banner_position_x":17}))
            .await
            .0,
        StatusCode::OK
    );
    let mut expected = presentation(&db, owner).await;
    assert_eq!(expected["banner_position_x"], 17);
    assert_eq!(expected["banner_position_y"], 100);
    assert_eq!(expected["country"], "JP");
    assert_eq!(expected["accent_color"], "#13579B");
    assert_eq!(expected["preferences"], preferences);
    assert_public(&get(&app, &public_path, None).await, "#13579B", 17, 100);
    state.limits.lock().await.clear();
    assert_eq!(
        patch(&app, Some(&owner_cookie), json!({"banner_position_y":25}))
            .await
            .0,
        StatusCode::OK
    );
    expected["banner_position_y"] = json!(25);
    assert_eq!(presentation(&db, owner).await, expected);
    for no_op in [
        json!({}),
        json!({"banner_position_x":null,"banner_position_y":null}),
    ] {
        state.limits.lock().await.clear();
        assert_eq!(
            patch(&app, Some(&owner_cookie), no_op).await.0,
            StatusCode::OK
        );
        assert_eq!(presentation(&db, owner).await, expected);
    }

    for (body, code) in invalid_patches().into_iter().chain([
        (
            json!({"bio":"a".repeat(4001),"banner_position_y":20}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"country":"jp","banner_position_y":20}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"accent_color":"red","banner_position_y":20}),
            StatusCode::BAD_REQUEST,
        ),
    ]) {
        state.limits.lock().await.clear();
        let (actual, error) = patch(&app, Some(&owner_cookie), body.clone()).await;
        assert_eq!(actual, code, "{body}: {error}");
        assert_eq!(
            presentation(&db, owner).await,
            expected,
            "invalid patch changed the owner: {body}"
        );
        assert_eq!(presentation(&db, peer).await, peer_initial);
    }
    state.limits.lock().await.clear();
    assert_eq!(
        patch(&app, None, json!({"banner_position_y":12})).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(presentation(&db, owner).await, expected);

    // Private theme accent edits do not overwrite the public profile accent or
    // crop. The private reply and public projection retain separate values.
    state.limits.lock().await.clear();
    let (code, theme) = request(
        &app,
        Method::PATCH,
        "/api/v1/me/preferences",
        Some(&owner_cookie),
        Some(json!({"accent_color":"#2468ac"})),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{theme}");
    assert_eq!(theme["accent_color"], "#2468ac");
    expected["preferences"]["accent_color"] = json!("#2468ac");
    assert_eq!(presentation(&db, owner).await, expected);
    assert_public(
        &get(&app, &public_path, Some(&peer_cookie)).await,
        "#13579B",
        17,
        25,
    );
    let private = get(&app, "/api/v1/me", Some(&owner_cookie)).await;
    assert_eq!(private["accent_color"], "#13579B");
    assert_eq!(private["preferences"]["accent_color"], "#2468ac");

    // A player without role permissions can edit their own presentation. The
    // target is always the authenticated session, and the owner is untouched.
    sqlx::query("DELETE FROM user_roles WHERE user_id=$1")
        .bind(peer)
        .execute(&db)
        .await
        .unwrap();
    state.limits.lock().await.clear();
    let (code, peer_saved) = patch(
        &app,
        Some(&peer_cookie),
        json!({"accent_color":"#987654","banner_position_x":100,"banner_position_y":0}),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{peer_saved}");
    assert_eq!(peer_saved["id"], peer);
    assert_eq!(peer_saved["banner_position_x"], 100);
    assert_eq!(peer_saved["banner_position_y"], 0);
    assert_eq!(presentation(&db, owner).await, expected);
    let peer_saved = presentation(&db, peer).await;

    for invalidation in ["revoked", "expired", "restricted"] {
        state.limits.lock().await.clear();
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1")
            .bind(owner)
            .execute(&db)
            .await
            .unwrap();
        let (sid, cookie) = session(&state, owner).await;
        let mut blocking = db.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(owner)
            .fetch_one(&mut *blocking)
            .await
            .unwrap();
        let queued_app = app.clone();
        let queued = tokio::spawn(async move {
            patch(
                &queued_app,
                Some(&cookie),
                json!({"accent_color":"#ffffff","banner_position_x":99,"banner_position_y":1}),
            )
            .await
        });
        wait_for_lock(
            &db,
            &application,
            "SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR UPDATE",
        )
        .await;
        match invalidation {
            "revoked" => {
                sqlx::query("DELETE FROM sessions WHERE id=$1")
                    .bind(sid)
                    .execute(&mut *blocking)
                    .await
                    .unwrap();
            }
            "expired" => {
                sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(sid).execute(&mut *blocking).await.unwrap();
            }
            "restricted" => {
                sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
                    .bind(owner)
                    .execute(&mut *blocking)
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        blocking.commit().await.unwrap();
        let (code, body) = tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            code,
            StatusCode::UNAUTHORIZED,
            "queued presentation after {invalidation}: {body}"
        );
        assert_eq!(presentation(&db, owner).await, expected);
        assert_eq!(presentation(&db, peer).await, peer_saved);
    }
    assert_eq!(
        request(&app, Method::GET, &public_path, None, None).await.0,
        StatusCode::NOT_FOUND
    );

    // Natural expiry while an unchanged session row is locked must be checked
    // after the wait, even though the earlier HTTP authentication succeeded.
    state.limits.lock().await.clear();
    sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1")
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
    let (sid, cookie) = session(&state, owner).await;
    let expires_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("UPDATE sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE id=$1 RETURNING expires_at")
        .bind(sid).fetch_one(&db).await.unwrap();
    let mut blocking = db.begin().await.unwrap();
    sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
        .bind(sid)
        .fetch_one(&mut *blocking)
        .await
        .unwrap();
    let queued_app = app.clone();
    let queued = tokio::spawn(async move {
        patch(&queued_app, Some(&cookie), json!({"banner_position_x":75})).await
    });
    wait_for_lock(
        &db,
        &application,
        "SELECT expires_at FROM sessions WHERE id=$1 AND user_id=$2 FOR UPDATE",
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=$1::timestamptz")
                .bind(expires_at)
                .fetch_one(&db)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    blocking.commit().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap()
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(presentation(&db, owner).await, expected);
    assert_eq!(presentation(&db, peer).await, peer_saved);
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to an owned disposable PostgreSQL 17"]
async fn profile_presentation_persistence_validation_and_queued_authorization() {
    let url = env::var("ILLYA_TEST_DATABASE_URL")
        .expect("Set the disposable profile presentation fixture URL");
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
        "Profile presentation fixture must run PostgreSQL 17"
    );
    let schema = format!("illya_profile_presentation_{}", Uuid::new_v4().simple());
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
    let pool = db.clone();
    let application = schema.clone();
    let outcome = tokio::spawn(async move { fixture_checks(pool, application).await }).await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
