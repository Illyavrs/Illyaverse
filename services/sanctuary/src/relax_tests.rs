//! RX partitions and operator recalculation use isolated PostgreSQL schemas.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request},
};
use futures_util::FutureExt;
use sqlx::Row;
use std::{panic::AssertUnwindSafe, str::FromStr};
use tower::ServiceExt;

const MAP: &str = "osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:Relax partition fixture\nArtist:Illyaverse\nCreator:Test\nVersion:Two circles\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n";

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let body = if let Some(value) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(value.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[allow(clippy::too_many_arguments)]
async fn score(
    db: &PgPool,
    user: i64,
    map: i64,
    mode: i16,
    mods: i64,
    pp: f64,
    accuracy: f64,
    passed: bool,
    review: &str,
    eligible: bool,
    standardised: Option<i64>,
    grade: &str,
) -> i64 {
    let variant = if mods & 128 != 0 { "relax" } else { "vanilla" };
    let id:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,mods,score,accuracy,max_combo,pp,passed,verification,pp_eligible,client,hit_counts,score_variant,standardised_score,classic_score,score_scale_status) VALUES($1,$2,$3,$4,CASE WHEN $4=0 THEN 1000 ELSE 0 END,$5,2,$6,$7,$8,$9,'stable','{\"n300\":2}',$10,$11,$11,CASE WHEN $11::bigint IS NULL THEN 'unavailable' ELSE 'legacy_estimated' END) RETURNING id")
        .bind(user).bind(map).bind(mode).bind(mods).bind(accuracy).bind(pp).bind(passed).bind(review).bind(eligible).bind(variant).bind(standardised).fetch_one(db).await.unwrap();
    sqlx::query("INSERT INTO audit_events(action,target_type,target_id,details) VALUES('scores.stable_ingest','score',$1,$2)")
        .bind(id.to_string()).bind(json!({"rank":grade})).execute(db).await.unwrap();
    id
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn relax_scores_statistics_and_recalculation_are_partitioned() {
    let url = env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let schema = format!("illya_relax_{suffix}");
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
    let test_root = std::env::temp_dir().join(format!("illyaverse-relax-{suffix}"));
    assert!(test_root.is_absolute() && test_root.parent() == Some(std::env::temp_dir().as_path()));
    let outcome=AssertUnwindSafe(async {
        sqlx::migrate!("../../migrations").run(&db).await.unwrap();
        let state=AppState{
            db:db.clone(),secret:Arc::new("relax-test-session-secret-at-least-32-bytes".into()),internal_key:Arc::new("relax-test-service-secret-at-least-32-bytes".into()),origin:Arc::new("http://localhost:3000".into()),mail:Arc::new(mail_delivery::MailConfig::disabled()),secure_cookie:false,media_root:test_root.join("media"),maps_root:test_root.join("maps"),replay_root:test_root.join("replays"),client:reqwest::Client::new(),official:Arc::new(Mutex::new(maps::OfficialState::default())),limits:Arc::new(Mutex::new(HashMap::new())),trusted_proxies:Arc::new(Vec::new()),upload_slots:Arc::new(tokio::sync::Semaphore::new(2)),chat_runtime:Default::default(),
        };
        tokio::fs::create_dir_all(&state.maps_root).await.unwrap();
        tokio::fs::write(state.maps_root.join("fixture.osu"),MAP).await.unwrap();
        let mut users=Vec::new();
        for (name,country,restricted) in [("RelaxAlice","TW",false),("RelaxBob","JP",false),("RelaxHidden","TW",true)] {
            let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash,country,is_restricted) VALUES($1,$2,'fixture','fixture',$3,$4) RETURNING id")
                .bind(name).bind(format!("{name}@example.invalid")).bind(country).bind(restricted).fetch_one(&db).await.unwrap();
            sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'").bind(id).execute(&db).await.unwrap();
            users.push(id);
        }
        let alice=users[0];let bob=users[1];let hidden=users[2];
        let mut maps=Vec::new();
        for (name,mode,status) in [("First",0_i16,"ranked"),("Second",0,"ranked"),("Taiko",1,"ranked"),("Mania",3,"ranked"),("Unranked",0,"unranked")] {
            let id:i64=sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES($1,'Fixture','Illyaverse','Two circles',$2,$3,1,$4,'fixture.osu') RETURNING id")
                .bind(name).bind(mode).bind(status).bind(format!("{:x}",md5::compute(name))).fetch_one(&db).await.unwrap();maps.push(id);
        }
        let first=maps[0];let second=maps[1];
        let nm_a=score(&db,alice,first,0,0,100.0,90.0,true,"verified",true,Some(500000),"A").await;
        let nm_a_scorebest=score(&db,alice,first,0,0,50.0,80.0,true,"verified",true,Some(900000),"B").await;
        score(&db,alice,second,0,0,50.0,95.0,true,"verified",true,Some(700000),"S").await;
        let nm_b=score(&db,bob,first,0,0,200.0,99.0,true,"verified",true,Some(800000),"S").await;
        let rx_a=score(&db,alice,first,0,128,300.0,98.0,true,"verified",true,None,"S").await;
        score(&db,alice,second,0,128,100.0,97.0,true,"verified",true,None,"A").await;
        let rx_b=score(&db,bob,first,0,128,50.0,88.0,true,"verified",true,None,"A").await;
        score(&db,bob,maps[2],1,128,1000.0,100.0,true,"verified",true,None,"X").await;
        let pending=score(&db,alice,first,0,128,9999.0,100.0,true,"pending",true,None,"X").await;
        let rejected=score(&db,alice,first,0,128,8888.0,100.0,true,"rejected",true,None,"X").await;
        score(&db,alice,first,0,128,7777.0,25.0,false,"verified",true,None,"F").await;
        let excluded=score(&db,alice,first,0,128|8192,20000.0,10.0,true,"verified",false,None,"S").await;
        score(&db,alice,maps[4],0,128,5000.0,50.0,true,"verified",true,None,"B").await;
        score(&db,hidden,first,0,128,100000.0,100.0,true,"verified",true,None,"X").await;
        let app=router(state.clone());
        let (status,_,vanilla)=request(&app,Method::GET,"/api/v1/leaderboard",None,None).await;
        assert_eq!(status,StatusCode::OK,"{vanilla}");assert_eq!(vanilla["mode"],0);assert_eq!(vanilla["score_variant"],"vanilla");
        assert_eq!(vanilla["items"][0]["id"],bob);assert_eq!(vanilla["items"][1]["id"],alice);
        assert!((vanilla["items"][1]["pp"].as_f64().unwrap()-147.5).abs()<1e-9);
        let (_,_,relax)=request(&app,Method::GET,"/api/v1/leaderboard?mode=0&variant=relax",None,None).await;
        assert_eq!(relax["items"][0]["id"],alice);assert_eq!(relax["items"][0]["pp"],395.0);assert_eq!(relax["items"][1]["id"],bob);
        assert_eq!(relax["total"],2);assert!(relax["items"].as_array().unwrap().iter().all(|row|row["score_variant"]=="relax"));
        let (_,_,country)=request(&app,Method::GET,"/api/v1/leaderboard?mode=0&variant=relax&country=JP",None,None).await;
        assert_eq!(country["items"][0]["id"],bob);assert_eq!(country["items"][0]["rank"],2);assert_eq!(country["items"][0]["country_rank"],1);assert_eq!(country["total"],1);
        let (_,_,profile)=request(&app,Method::GET,&format!("/api/v1/users/{alice}?mode=0&variant=relax"),None,None).await;
        assert_eq!(profile["pp"],395.0);assert_eq!(profile["play_count"],5);assert_eq!(profile["accuracy"],56.0);assert_eq!(profile["statistics"]["mode"],0);assert_eq!(profile["statistics"]["score_variant"],"relax");
        assert_eq!(profile["statistics"]["global_rank"],1);assert_eq!(profile["statistics"]["grade_counts"]["s"],2);assert_eq!(profile["statistics"]["grade_counts"]["a"],1);assert_eq!(profile["statistics"]["grade_counts"]["f"],1);assert!(profile["statistics"]["total_score"].is_null());assert_eq!(profile["statistics"]["score_totals_complete"],false);assert!(profile.get("email").is_none());
        let (_,_,default_profile)=request(&app,Method::GET,&format!("/api/v1/users/{alice}"),None,None).await;
        assert_eq!(default_profile["pp"],147.5);assert_eq!(default_profile["play_count"],3);assert_eq!(default_profile["statistics"]["grade_counts"]["s"],1);
        let (_,_,taiko)=request(&app,Method::GET,&format!("/api/v1/users/{bob}?mode=1&variant=relax"),None,None).await;
        assert_eq!(taiko["pp"],1000.0);assert_eq!(taiko["play_count"],1);assert_eq!(taiko["statistics"]["grade_counts"]["ss"],1);
        for path in ["/api/v1/leaderboard?mode=3&variant=relax", "/api/v1/leaderboard?mode=4", "/api/v1/leaderboard?variant=autopilot", "/api/v1/leaderboard?country=tw"] {
            assert_eq!(request(&app,Method::GET,path,None,None).await.0,StatusCode::BAD_REQUEST);
        }
        let (_,_,nm_board)=request(&app,Method::GET,&format!("/api/v1/beatmaps/{first}/scores"),None,None).await;
        assert_eq!(nm_board["items"][0]["id"],nm_a_scorebest);assert_eq!(nm_board["items"][1]["id"],nm_b);assert_eq!(nm_board["ordered_by"],"standardised_score");assert_eq!(nm_board["total"],2);assert!(nm_board["items"].as_array().unwrap().iter().all(|row|row["id"]!=nm_a));
        let (_,_,rx_board)=request(&app,Method::GET,&format!("/api/v1/beatmaps/{first}/scores?mode=0&variant=relax&page_size=1&page=2"),None,None).await;
        assert_eq!(rx_board["items"][0]["id"],rx_b);assert_eq!(rx_board["items"][0]["position"],2);assert_eq!(rx_board["total"],2);assert_eq!(rx_board["ordered_by"],"pp");
        // These are real SQL results, independently checking the browser table contract.
        for (row,variant,user,username,raw,pp) in [
            (&nm_board["items"][0],"vanilla",alice,"RelaxAlice",1000,50.0),
            (&rx_board["items"][0],"relax",bob,"RelaxBob",0,50.0),
        ] {
            assert_eq!(row["title"],"First");assert_eq!(row["version"],"Two circles");
            assert_eq!(row["beatmap_id"],first);assert_eq!(row["user_id"],user);assert_eq!(row["username"],username);
            assert_eq!(row["mode"],0);assert_eq!(row["score_variant"],variant);assert_eq!(row["score"],raw);assert_eq!(row["pp"],pp);
            assert_eq!(row["verification"],"verified");assert_eq!(row["has_replay"],false);assert!(row["created_at"].is_string());
            assert!(row.get("replay_path").is_none());assert!(row.get("ingestion_key").is_none());
            let (status,_,detail)=request(&app,Method::GET,&format!("/api/v1/scores/{}",row["id"]),None,None).await;
            assert_eq!(status,StatusCode::OK);assert_eq!(detail["title"],row["title"]);assert_eq!(detail["version"],row["version"]);
            assert_eq!(detail["beatmap"]["id"],first);assert_eq!(detail["user"]["id"],user);assert!(detail["replay_url"].is_null());
        }
        let (_,_,scores)=request(&app,Method::GET,&format!("/api/v1/scores?user_id={alice}"),None,None).await;
        assert_eq!(scores["total"],3);assert!(scores["items"].as_array().unwrap().iter().all(|row|row["score_variant"]=="vanilla"&&row["mode"]==0));
        let (_,_,recent)=request(&app,Method::GET,"/api/v1/scores",None,None).await;
        assert!(recent["mode"].is_null()&&recent["score_variant"].is_null());assert!(recent["items"].as_array().unwrap().iter().any(|row|row["id"]==rx_a));assert!(recent["items"].as_array().unwrap().iter().any(|row|row["id"]==nm_b));assert!(recent["items"].as_array().unwrap().iter().all(|row|row["id"]!=pending&&row["id"]!=rejected&&row["user_id"]!=hidden));

        // Mint test-only web credentials for privacy and operator permission
        // checks; this never uses a real user's session or deployed database.
        let mut cookies=Vec::new();
        for user in [alice,bob] {
            let sid=uuid::Uuid::new_v4();sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,now()+interval '1 day')").bind(sid).bind(user).execute(&db).await.unwrap();
            let claims=json!({"sub":user,"sid":sid,"exp":(chrono::Utc::now()+chrono::Duration::days(1)).timestamp()});
            let jwt=jsonwebtoken::encode(&jsonwebtoken::Header::default(),&claims,&jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes())).unwrap();cookies.push(format!("illya_session={jwt}"));
        }
        let (_,_,own)=request(&app,Method::GET,&format!("/api/v1/scores?user_id={alice}&mode=0&variant=relax"),Some(&cookies[0]),None).await;
        assert!(own["items"].as_array().unwrap().iter().any(|row|row["id"]==pending));
        assert_eq!(request(&app,Method::GET,&format!("/api/v1/scores/{pending}"),Some(&cookies[1]),None).await.0,StatusCode::NOT_FOUND);
        assert_eq!(request(&app,Method::GET,&format!("/api/v1/scores/{pending}"),Some(&cookies[0]),None).await.0,StatusCode::OK);
        let submission=json!({"user_id":alice,"beatmap_id":first,"mode":0,"score":0,"accuracy":100.0,"max_combo":2,"mods":128,"hit_counts":{"n300":2},"client":"stable","ingestion_key":"relax-fresh"});
        let mut internal_headers=HeaderMap::new();internal_headers.insert("x-illya-service-key",state.internal_key.parse().unwrap());
        let fresh=scores::internal_submit(axum::extract::State(state.clone()),internal_headers.clone(),Json(serde_json::from_value(submission.clone()).unwrap())).await.unwrap().0;
        assert_eq!(fresh["score_variant"],"relax");assert_eq!(fresh["pp_eligible"],true);assert_eq!(fresh["verification"],"pending");assert_eq!(fresh["score_scale_status"],"unavailable");assert!(fresh["pp"].as_f64().unwrap().is_finite());assert!(fresh["pp"].as_f64().unwrap()>0.0);
        for mode in 0_u8..=2 {
            let target=if mode==1{maps[2]}else{first};
            if mode!=0 {
                let mut stable=submission.clone();stable["mode"]=json!(mode);stable["beatmap_id"]=json!(target);stable["ingestion_key"]=json!(format!("relax-stable-mode-{mode}"));
                let result=scores::internal_submit(axum::extract::State(state.clone()),internal_headers.clone(),Json(serde_json::from_value(stable).unwrap())).await.unwrap().0;
                assert_eq!(result["mode"],mode);assert_eq!(result["score_variant"],"relax");assert_eq!(result["pp_eligible"],true);assert!(result["pp"].as_f64().unwrap().is_finite());
            }
            let native_context=json!({"ruleset_id":mode,"passed":true,"total_score":0,"total_score_without_mods":1000000,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[{"acronym":"RX"}],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]});
            let native=json!({"user_id":alice,"beatmap_id":target,"mode":mode,"score":0,"accuracy":100.0,"max_combo":2,"mods":128,"api_mods":[{"acronym":"RX"}],"native_submission":native_context,"hit_counts":{"n300":2},"client":"lazer","ingestion_key":format!("relax-native-mode-{mode}")});
            let result=scores::internal_submit(axum::extract::State(state.clone()),internal_headers.clone(),Json(serde_json::from_value(native).unwrap())).await.unwrap().0;
            assert_eq!(result["mode"],mode);assert_eq!(result["score_variant"],"relax");assert_eq!(result["pp_eligible"],true);assert_eq!(result["score_scale_status"],"unavailable");assert!(result["native_submission"].is_object());assert!(result["pp"].as_f64().unwrap().is_finite());
        }
        let mut forged=submission.clone();forged["score_variant"]=json!("vanilla");assert!(serde_json::from_value::<scores::Submission>(forged).is_err());
        let mut mania=submission.clone();mania["mode"]=json!(3);mania["beatmap_id"]=json!(maps[3]);mania["ingestion_key"]=json!("invalid-mania-rx");
        assert_eq!(scores::internal_submit(axum::extract::State(state.clone()),internal_headers.clone(),Json(serde_json::from_value(mania).unwrap())).await.unwrap_err().0,StatusCode::BAD_REQUEST);
        let mut reuse=submission.clone();reuse["mods"]=json!(0);
        assert_eq!(scores::internal_submit(axum::extract::State(state.clone()),internal_headers,Json(serde_json::from_value(reuse).unwrap())).await.unwrap_err().0,StatusCode::CONFLICT);

        // Old RX exclusion stays excluded until this explicit operator action.
        let historic=score(&db,alice,first,0,128,123.0,100.0,true,"verified",false,None,"X").await;
        let endpoint="/api/v1/admin/scores/recalculate-relax";
        assert_eq!(request(&app,Method::POST,endpoint,Some(&cookies[0]),Some(json!({"score_ids":[historic]}))).await.0,StatusCode::FORBIDDEN);
        sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='moderator'").bind(bob).execute(&db).await.unwrap();
        assert_eq!(request(&app,Method::POST,endpoint,Some(&cookies[1]),Some(json!({"score_ids":[historic,excluded]}))).await.0,StatusCode::BAD_REQUEST);
        assert!(!sqlx::query_scalar::<_,bool>("SELECT pp_eligible FROM scores WHERE id=$1").bind(historic).fetch_one(&db).await.unwrap());
        let (status,_,recalculated)=request(&app,Method::POST,endpoint,Some(&cookies[1]),Some(json!({"score_ids":[historic]}))).await;
        assert_eq!(status,StatusCode::OK,"{recalculated}");assert_eq!(recalculated["items"][0]["pp_eligible"],true);assert_eq!(recalculated["items"][0]["verification"],"verified");
        let row=sqlx::query("SELECT score,mods,user_id,verification,pp_eligible,pp_calculation_version FROM scores WHERE id=$1").bind(historic).fetch_one(&db).await.unwrap();
        assert_eq!(row.get::<i64,_>("score"),0);assert_eq!(row.get::<i64,_>("mods"),128);assert_eq!(row.get::<i64,_>("user_id"),alice);assert_eq!(row.get::<String,_>("verification"),"verified");assert!(row.get::<bool,_>("pp_eligible"));assert_eq!(row.get::<String,_>("pp_calculation_version"),"rosu_pp4.relax.v1");
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events WHERE action='scores.relax_recalculate' AND actor_id=$1 AND target_id=$2").bind(bob).bind(historic.to_string()).fetch_one(&db).await.unwrap(),1);
        for ids in [json!([]),json!([historic,historic]),json!((1..=17).collect::<Vec<i64>>())] {
            assert_eq!(request(&app,Method::POST,endpoint,Some(&cookies[1]),Some(json!({"score_ids":ids}))).await.0,StatusCode::BAD_REQUEST);
        }
    }).catch_unwind().await;
    db.close().await;
    let drop_schema = sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await;
    control.close().await;
    if test_root.exists() {
        tokio::fs::remove_dir_all(&test_root).await.unwrap();
    }
    drop_schema.unwrap();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
async fn relax_migration_preserves_review_and_explicit_exclusions() {
    let url = env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("illya_relax_upgrade_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let db = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    let outcome=AssertUnwindSafe(async {
        let migrations=sqlx::migrate!("../../migrations");
        for migration in migrations.iter().filter(|migration|migration.version<14) {
            sqlx::raw_sql(&migration.sql).execute(&db).await.unwrap();
        }
        let user:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES('HistoricalRX','historicalrx@example.invalid','fixture','fixture') RETURNING id").fetch_one(&db).await.unwrap();
        let map:i64=sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('Historical RX','Fixture','Illyaverse','Migration',0,'ranked',1,$1,'fixture.osu') RETURNING id").bind(format!("{:x}",md5::compute("historical-relax-map"))).fetch_one(&db).await.unwrap();
        let mut expectations=Vec::new();
        for (index,(mode,mods,was_eligible,review,da)) in [(0_i16,128_i64,false,"pending",false),(1,128,true,"verified",false),(2,128,true,"verified",false),(3,128,true,"verified",false),(0,128|8192,true,"verified",false),(0,128|2048,true,"verified",false),(0,128,true,"verified",true),(0,0,true,"verified",false)].into_iter().enumerate() {
            let raw=10000+index as i64;let pp=100.0+index as f64;
            let id:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,mods,score,accuracy,max_combo,pp,verification,pp_eligible,client,api_mods,standardised_score,classic_score,score_scale_status,replay_path) VALUES($1,$2,$3,$4,$5,99,2,$6,$7,$8,$9,$10,1234,4321,$11,'preserved-fixture.osr') RETURNING id")
                .bind(user).bind(map).bind(mode).bind(mods).bind(raw).bind(pp).bind(review).bind(was_eligible)
                .bind(if da{"lazer"}else{"stable"}).bind(if da{Some(json!([{"acronym":"RX"},{"acronym":"DA","settings":{"overall_difficulty":7.0}}]))}else{None})
                .bind(if da{"native_reported"}else{"legacy_estimated"}).fetch_one(&db).await.unwrap();
            let remains_eligible=was_eligible&&mode!=3&&mods&10240==0&&!da;
            expectations.push((id,mode,mods,raw,pp,review,remains_eligible));
        }
        let upgrade=migrations.iter().find(|migration|migration.version==14).unwrap();
        sqlx::raw_sql(&upgrade.sql).execute(&db).await.unwrap();
        for (id,mode,mods,raw,pp,review,eligible) in expectations {
            let row=sqlx::query("SELECT * FROM scores WHERE id=$1").bind(id).fetch_one(&db).await.unwrap();
            assert_eq!(row.get::<i16,_>("mode"),mode);assert_eq!(row.get::<i64,_>("mods"),mods);assert_eq!(row.get::<i64,_>("score"),raw);assert_eq!(row.get::<f64,_>("pp"),pp);assert_eq!(row.get::<String,_>("verification"),review);assert_eq!(row.get::<bool,_>("pp_eligible"),eligible);
            assert_eq!(row.get::<String,_>("replay_path"),"preserved-fixture.osr");assert!(row.get::<Option<String>,_>("pp_calculation_version").is_none());
            assert_eq!(row.get::<String,_>("score_variant"),if mods&128!=0{"relax"}else{"vanilla"});
            if mods&128!=0 {
                assert!(row.get::<Option<i64>,_>("standardised_score").is_none());assert!(row.get::<Option<i64>,_>("classic_score").is_none());assert_eq!(row.get::<String,_>("score_scale_status"),"unavailable");
            }else {
                assert_eq!(row.get::<Option<i64>,_>("standardised_score"),Some(1234));assert_eq!(row.get::<Option<i64>,_>("classic_score"),Some(4321));
            }
        }
    }).catch_unwind().await;
    db.close().await;
    let cleanup = sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await;
    control.close().await;
    cleanup.unwrap();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
