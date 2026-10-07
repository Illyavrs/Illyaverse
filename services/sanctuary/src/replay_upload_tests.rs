use crate::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::str::FromStr;
use tower::ServiceExt;
async fn register(app: &Router, name: &str) -> (i64, String) {
    let response=app.clone().oneshot(Request::builder().method("POST").uri("/api/v1/auth/register").header(header::ORIGIN,"http://localhost:3000").header(header::CONTENT_TYPE,"application/json").body(Body::from(json!({"username":name,"email":format!("{name}@example.invalid"),"password":"fixture-password-1234"}).to_string())).unwrap()).await.unwrap();
    assert!(response.status().is_success());
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    (value["user"]["id"].as_i64().unwrap(), cookie)
}
async fn upload(app: &Router, id: i64, cookie: Option<&str>, bytes: &[u8]) -> (StatusCode, Value) {
    let mut body=b"--replay-test\r\nContent-Disposition: form-data; name=\"file\"; filename=\"fixture.osr\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    body.extend(bytes);
    body.extend(b"\r\n--replay-test--\r\n");
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/scores/{id}/replay"))
        .header(header::ORIGIN, "http://localhost:3000")
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=replay-test",
        );
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap();
    (status, value)
}
#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; fresh PostgreSQL schema per run"]
async fn extended_replay_upload_permissions_and_idempotency() {
    let url = env::var("ILLYA_TEST_DATABASE_URL").unwrap();
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("replay_upload_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let directory = std::env::temp_dir().join(&schema);
    tokio::fs::create_dir_all(directory.join("replays"))
        .await
        .unwrap();
    let state = AppState {
        db: db.clone(),
        secret: Arc::new("test-session-secret-with-at-least-thirty-two-bytes".into()),
        internal_key: Arc::new("test-internal-secret-with-at-least-thirty-two-bytes".into()),
        origin: Arc::new("http://localhost:3000".into()),
        mail: Arc::new(mail_delivery::MailConfig::disabled()),
        secure_cookie: false,
        media_root: directory.join("media"),
        maps_root: directory.join("maps"),
        replay_root: directory.join("replays"),
        client: reqwest::Client::new(),
        official: Arc::new(Mutex::new(maps::OfficialState::default())),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        upload_slots: Arc::new(tokio::sync::Semaphore::new(2)),
    };
    let app = router(state);
    // A task wrapper ensures schema and files are removed even if an assertion fails.
    let app_clone = app.clone();
    let pool = db.clone();
    let result=tokio::spawn(async move {
        let (owner,cookie)=register(&app_clone,"ReplayOwner").await;let (_,stranger)=register(&app_clone,"ReplayStranger").await;let map:i64=sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,stars,max_combo,md5,osu_path) VALUES('Fixture','Illyaverse','Tests','Easy',0,1,2,'0123456789abcdef0123456789abcdef','fixture.osu') RETURNING id").fetch_one(&pool).await.unwrap();
        let submission:resonance_protocol::lazer::SoloScoreSubmission=serde_json::from_value(json!({"ruleset_id":0,"passed":true,"total_score":1234,"total_score_without_mods":1234,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]})).unwrap();
        let id:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,api_mods) VALUES($1,$2,0,1234,100,2,0,0,true,'{\"n300\":2}','lazer','[]') RETURNING id").bind(owner).bind(map).fetch_one(&pool).await.unwrap();sqlx::query("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,score_id,submission) VALUES($1,$2,0,'fixture',now()+interval '1 hour',$3,$4)").bind(owner).bind(map).bind(id).bind(json!(submission)).execute(&pool).await.unwrap();
        let mut binding=resonance_protocol::lazer_replay::ReplayScore {online_id:id,user_id:owner,username:"ReplayOwner",beatmap_md5:"0123456789abcdef0123456789abcdef",client_version:"fixture",mods_bits:0,timestamp_ticks:639269732961234567,submission:&submission};let bytes=resonance_protocol::lazer_replay::encode(&binding,b"0|256|192|0,100|256|192|1,-12345|0|0|0").unwrap();
        assert_eq!(upload(&app_clone,id,None,&bytes).await.0,StatusCode::UNAUTHORIZED);assert_eq!(upload(&app_clone,id,Some(&stranger),&bytes).await.0,StatusCode::FORBIDDEN);binding.online_id=id+1;let wrong=resonance_protocol::lazer_replay::encode(&binding,b"0|256|192|0,100|256|192|1").unwrap();assert_eq!(upload(&app_clone,id,Some(&cookie),&wrong).await.0,StatusCode::BAD_REQUEST);binding.online_id=id;
        let (status,value)=upload(&app_clone,id,Some(&cookie),&bytes).await;assert_eq!(status,StatusCode::OK,"{value}");assert_eq!(value["verification"],"pending");let (status,value)=upload(&app_clone,id,Some(&cookie),&bytes).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["idempotent"],true);
        binding.timestamp_ticks+=1;let different=resonance_protocol::lazer_replay::encode(&binding,b"0|256|192|0,100|256|192|1,-12345|0|0|0").unwrap();assert_eq!(upload(&app_clone,id,Some(&cookie),&different).await.0,StatusCode::CONFLICT);
        let request=|cookie:Option<&str>| {let mut request=Request::builder().uri(format!("/api/v1/scores/{id}/replay"));if let Some(cookie)=cookie{request=request.header(header::COOKIE,cookie);}request.body(Body::empty()).unwrap()};assert_eq!(app_clone.clone().oneshot(request(None)).await.unwrap().status(),StatusCode::UNAUTHORIZED);assert_eq!(app_clone.clone().oneshot(request(Some(&stranger))).await.unwrap().status(),StatusCode::FORBIDDEN);let response=app_clone.clone().oneshot(request(Some(&cookie))).await.unwrap();assert_eq!(response.status(),StatusCode::OK);assert_eq!(&to_bytes(response.into_body(),16_777_216).await.unwrap()[..],&bytes[..]);
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT replay_format FROM scores WHERE id=$1").bind(id).fetch_one(&pool).await.unwrap(),"lazer");assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events WHERE action='scores.replay_upload' AND target_id=$1").bind(id.to_string()).fetch_one(&pool).await.unwrap(),1);
    }).await;
    drop(app);
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
    tokio::fs::remove_dir_all(directory).await.unwrap();
    result.unwrap();
}
