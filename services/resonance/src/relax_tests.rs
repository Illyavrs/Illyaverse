//! RX is a separate statistics/PP partition with the original native ruleset ID.
use crate::{beatmaps, common, lazer, score_aggregates, stable};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::post,
};
use futures_util::FutureExt;
use illya_domain::ScoreVariant;
use resonance_protocol::stable::{self as wire, Reader, UserAction, Writer};
use serde_json::{Value, json};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{path::PathBuf, str::FromStr, sync::Arc};
use tower::ServiceExt;

struct Database {
    control: PgPool,
    pool: PgPool,
    schema: String,
    storage: PathBuf,
}
impl Database {
    async fn new() -> Self {
        let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable PostgreSQL URL");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "local disposable database required"
        );
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("relax_gateway_{}", uuid::Uuid::new_v4().simple());
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
        let storage = std::env::temp_dir().join(format!(
            "illya-relax-gateway-{}",
            uuid::Uuid::new_v4().simple()
        ));
        tokio::fs::create_dir(&storage).await.unwrap();
        tokio::fs::create_dir(storage.join("replays"))
            .await
            .unwrap();
        Self {
            control,
            pool,
            schema,
            storage,
        }
    }
    fn router(&self) -> Router {
        let state = common::AppState {
            pool: self.pool.clone(),
            storage: Arc::new(self.storage.clone()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        Router::new()
            .route("/", post(stable::bancho))
            .merge(stable::router())
            .merge(lazer::router())
            .merge(beatmaps::router())
            .with_state(state)
            .layer(axum::Extension(axum::extract::ConnectInfo(
                "127.0.0.1:19014".parse::<std::net::SocketAddr>().unwrap(),
            )))
    }
    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
        let directory = tokio::fs::canonicalize(&self.storage).await.unwrap();
        let temporary = tokio::fs::canonicalize(std::env::temp_dir()).await.unwrap();
        assert_eq!(directory.parent(), Some(temporary.as_path()));
        assert!(
            directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("illya-relax-gateway-")
        );
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}

async fn user(pool: &PgPool, name: &str, country: &str, restricted: bool) -> i64 {
    let salt = SaltString::encode_b64(b"relax-fixture-salt").unwrap();
    let legacy = format!("{:x}", md5::compute("Relax-Fixture-Password"));
    let hash = Argon2::default()
        .hash_password(legacy.as_bytes(), &salt)
        .unwrap()
        .to_string();
    sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash,country,is_restricted) VALUES($1,$2,'fixture-not-web-password',$3,$4,$5) RETURNING id")
        .bind(name).bind(format!("{name}@invalid.test")).bind(hash).bind(country).bind(restricted).fetch_one(pool).await.unwrap()
}
async fn map(pool: &PgPool, mode: i16, md5: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('RX fixture','Illyaverse','Fixture','Partition',$1,'ranked',1,$2,'fixture.osu') RETURNING id")
        .bind(mode).bind(md5).fetch_one(pool).await.unwrap()
}
struct Play<'a> {
    user: i64,
    map: i64,
    mode: i16,
    mods: i64,
    score: i64,
    pp: f64,
    accuracy: f64,
    client: &'a str,
    verification: &'a str,
    passed: bool,
    eligible: bool,
    grade: &'a str,
}
async fn play(pool: &PgPool, p: Play<'_>) -> i64 {
    let variant = if p.mods & 128 != 0 {
        "relax"
    } else {
        "vanilla"
    };
    let normalized = variant == "vanilla";
    let id:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,mods,score,pp,accuracy,max_combo,client,verification,passed,pp_eligible,hit_counts,score_variant,standardised_score,classic_score,score_scale_status,native_submission) VALUES($1,$2,$3,$4,$5,$6,$7,2,$8,$9,$10,$11,'{\"n300\":2}',$12,$13,$14,$15,$16) RETURNING id")
        .bind(p.user).bind(p.map).bind(p.mode).bind(p.mods).bind(p.score).bind(p.pp).bind(p.accuracy).bind(p.client).bind(p.verification).bind(p.passed).bind(p.eligible).bind(variant)
        .bind(normalized.then_some(p.score*100)).bind(normalized.then_some(p.score))
        .bind(if !normalized {"unavailable"}else if p.client=="stable" {"legacy_estimated"}else{"native_reported"})
        .bind((p.client=="lazer").then(||json!({"rank":p.grade,"mods":[{"acronym":"RX"}]})))
        .fetch_one(pool).await.unwrap();
    if p.client == "stable" {
        sqlx::query("INSERT INTO audit_events(action,target_type,target_id,details) VALUES('scores.stable_ingest','score',$1,$2)")
        .bind(id.to_string()).bind(json!({"rank":p.grade})).execute(pool).await.unwrap();
    }
    id
}
async fn access(pool: &PgPool, user: i64) -> String {
    let token = common::secret();
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')")
        .bind(uuid::Uuid::new_v4()).bind(user).bind(common::token_hash(&token)).bind(common::secret()).execute(pool).await.unwrap();
    token
}
async fn get(router: &Router, path: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, bytes)
}
async fn json_get(router: &Router, path: &str, token: &str) -> Value {
    let (status, bytes) = get(router, path, token).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{path}: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn native_variant_defaults_are_explicit_and_do_not_invent_extra_rulesets() {
    for mode in 0..=2 {
        assert_eq!(
            lazer::requested_variant(mode, None).unwrap(),
            ScoreVariant::Vanilla
        );
        assert_eq!(
            lazer::requested_variant(mode, Some("relax")).unwrap(),
            ScoreVariant::Relax
        );
    }
    assert!(lazer::requested_variant(3, Some("relax")).is_err());
    assert!(lazer::requested_variant(4, None).is_err());
    assert!(lazer::requested_variant(0, Some("rx")).is_err());
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn relax_partitions_native_profiles_boards_rankings_and_replays() {
    let database = Database::new().await;
    let outcome=std::panic::AssertUnwindSafe(async {
        let pool=&database.pool;
        let a=user(pool,"RelaxAlpha","TW",false).await;
        let b=user(pool,"RelaxBeta","JP",false).await;
        let restricted=user(pool,"RelaxRestricted","TW",true).await;
        let map0=map(pool,0,"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").await;
        let map1=map(pool,1,"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").await;
        let native0:i64=sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1").bind(map0).fetch_one(pool).await.unwrap();
        assert_ne!(map0,native0);
        let mk=|user,map,mode,mods,score,pp,accuracy,client,verification,passed,eligible,grade|Play{user,map,mode,mods,score,pp,accuracy,client,verification,passed,eligible,grade};
        let av=play(pool,mk(a,map0,0,0,10000,200.0,100.0,"stable","verified",true,true,"X")).await;
        let ar=play(pool,mk(a,map0,0,128,20000,20.0,70.0,"stable","verified",true,true,"A")).await;
        play(pool,mk(a,map0,0,128,30000,10.0,60.0,"stable","verified",true,true,"A")).await;
        play(pool,mk(b,map0,0,0,9000,50.0,80.0,"stable","verified",true,true,"A")).await;
        let br=play(pool,mk(b,map0,0,128,5000,150.0,99.0,"stable","verified",true,true,"S")).await;
        let bn=play(pool,mk(b,map0,0,128,3000,140.0,95.0,"lazer","verified",true,true,"S")).await;
        for (review,passed,eligible,accuracy) in [("pending",true,true,100.0),("rejected",true,true,100.0),("verified",false,true,50.0),("verified",true,false,40.0)] {
            play(pool,mk(a,map0,0,128,100000,900.0,accuracy,"stable",review,passed,eligible,"X")).await;
        }
        let restricted_score=play(pool,mk(restricted,map0,0,128,999999,9999.0,100.0,"stable","verified",true,true,"X")).await;
        play(pool,mk(a,map1,1,0,7000,777.0,100.0,"stable","verified",true,true,"X")).await;
        play(pool,mk(a,map1,1,128,8888,333.0,98.0,"stable","verified",true,true,"S")).await;
        let token=access(pool,a).await;
        let router=database.router();
        let vanilla=json_get(&router,"/api/v2/me/osu",&token).await;
        assert_eq!(vanilla["score_variant"],"vanilla");assert_eq!(vanilla["statistics"]["pp"],200.0);
        assert_eq!(vanilla["statistics"]["play_count"],1);assert_eq!(vanilla["statistics"]["grade_counts"]["ss"],1);
        let relax=json_get(&router,"/api/v2/me/osu?variant=relax",&token).await;
        assert_eq!(relax["statistics"]["pp"],20.0);assert_eq!(relax["statistics"]["global_rank"],2);
        assert_eq!(relax["statistics"]["play_count"],4);assert_eq!(relax["statistics"]["hit_accuracy"],55.0);
        assert_eq!(relax["statistics"]["ranked_score"],0);assert_eq!(relax["statistics"]["grade_counts"]["a"],2);
        let taiko=json_get(&router,"/api/v2/me/taiko?variant=relax",&token).await;
        assert_eq!(taiko["statistics"]["pp"],333.0);assert_eq!(taiko["statistics"]["play_count"],1);
        assert_eq!(get(&router,"/api/v2/me/mania?variant=relax",&token).await.0,StatusCode::BAD_REQUEST);
        let rv=json_get(&router,"/api/v2/rankings/osu/performance",&token).await;
        let rr=json_get(&router,"/api/v2/rankings/osu/performance?variant=relax",&token).await;
        assert_eq!(rv["ranking"][0]["user"]["id"],a);assert_eq!(rr["ranking"][0]["user"]["id"],b);
        assert_eq!(rr["total"],2);assert_eq!(rr["score_variant"],"relax");assert_eq!(rr["ruleset_id"],0);
        let country=json_get(&router,"/api/v2/rankings/osu/performance?variant=relax&country=TW",&token).await;
        assert_eq!(country["total"],1);assert_eq!(country["ranking"][0]["user"]["id"],a);
        let friends=json_get(&router,"/api/v2/rankings/osu/performance?variant=relax&filter=friends",&token).await;
        assert_eq!(friends["total"],1);assert_eq!(friends["ranking"][0]["user"]["id"],a);
        // Window counts disappear after OFFSET runs past the last row. The
        // fallback must keep the same unrestricted country/friends population.
        for (path, total) in [
            ("/api/v2/rankings/osu/performance?page=2", 2),
            ("/api/v2/rankings/osu/performance?variant=relax&page=2", 2),
            ("/api/v2/rankings/osu/performance?variant=relax&country=tw&page=2", 1),
            ("/api/v2/rankings/osu/performance?variant=relax&filter=friends&page=2", 1),
            ("/api/v2/rankings/osu/performance?variant=relax&country=TW&filter=friends&page=2", 1),
            ("/api/v2/rankings/osu/performance?variant=relax&country=JP&filter=friends&page=2", 0),
            ("/api/v2/rankings/osu/performance?variant=relax&country=ZZ&page=2", 0),
        ] {
            let empty=json_get(&router,path,&token).await;
            assert_eq!(empty["ranking"],json!([]),"{path}");
            assert_eq!(empty["total"],total,"{path}");
        }
        let bv=json_get(&router,&format!("/api/v2/beatmaps/{native0}/scores"),&token).await;
        let rx=json_get(&router,&format!("/api/v2/beatmaps/{native0}/scores?variant=relax"),&token).await;
        assert_eq!(bv["scores"][0]["id"],av);assert_eq!(rx["scores"][0]["id"],br);assert_eq!(rx["scores"][1]["id"],ar);
        assert_eq!(rx["score_count"],2);assert_eq!(rx["scores"][0]["ruleset_id"],0);assert_eq!(rx["scores"][0]["score_variant"],"relax");
        assert_eq!(rx["scores"][0]["total_score"],0);assert_eq!(rx["scores"][0]["legacy_total_score"],5000);
        assert!(rx["scores"][0]["classic_total_score"].is_null());assert_eq!(rx["scores"][0]["score_scale_status"],"unavailable");
        let filtered=json_get(&router,&format!("/api/v2/beatmaps/{native0}/scores?mods%5B%5D=RX"),&token).await;
        assert_eq!(filtered["scores"][0]["id"],bn);assert_eq!(filtered["score_variant"],"relax");
        assert_eq!(get(&router,&format!("/api/v2/beatmaps/{native0}/scores?variant=vanilla&mods%5B%5D=RX"),&token).await.0,StatusCode::BAD_REQUEST);
        let best=json_get(&router,&format!("/api/v2/users/{b}/scores/best?mode=osu&variant=relax"),&token).await;
        assert_eq!(best.as_array().unwrap().len(),1);assert_eq!(best[0]["id"],br);
        let default=json_get(&router,&format!("/api/v2/users/{a}/scores/recent?mode=osu"),&token).await;
        assert_eq!(default.as_array().unwrap().len(),1);assert_eq!(default[0]["id"],av);
        // Structural .osr fixture binds the original ruleset/mods/score ID. The
        // opaque frame payload intentionally makes no gameplay-verification claim.
        let frames=b"opaque compressed RX frame fixture";
        let mut replay_header=Writer::new();
        replay_header.u8(0).i32(20261007).string("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
            .string("RelaxBeta").unwrap().string("").unwrap();
        for count in [2,0,0,0,0,0] {replay_header.u16(count);}
        replay_header.i32(5000).u16(2).boolean(false).u32(128).string("").unwrap().i64(0).i32(frames.len() as i32);
        let mut replay=replay_header.into_bytes();replay.extend_from_slice(frames);replay.extend(br.to_le_bytes());
        let container=resonance_protocol::replay::LegacyReplay::parse(&replay).unwrap();
        assert_eq!(container.mode,0);assert_eq!(container.mods,128);assert_eq!(container.online_score_id,Some(br));
        tokio::fs::write(database.storage.join("replays/rx.osr"),&replay).await.unwrap();
        sqlx::query("UPDATE scores SET replay_path='rx.osr',replay_complete=true WHERE id=$1").bind(br).execute(pool).await.unwrap();
        let detail=json_get(&router,&format!("/api/v2/scores/{br}"),&token).await;
        assert_eq!(detail["score_variant"],"relax");assert_eq!(detail["ruleset_id"],0);assert_eq!(detail["replay_complete"],true);
        let(status,bytes)=get(&router,&format!("/api/v2/scores/{br}/download"),&token).await;assert_eq!(status,StatusCode::OK);assert_eq!(bytes,replay);
        let legacy_hash=format!("{:x}",md5::compute("Relax-Fixture-Password"));
        let legacy_replay=|mode|format!("/web/osu-getreplay.php?u=RelaxAlpha&ha={legacy_hash}&c={br}&m={mode}");
        assert_eq!(get(&router,&legacy_replay(1),&token).await.0,StatusCode::NOT_FOUND);
        assert_eq!(get(&router,&legacy_replay(0),&token).await.1,frames);
        sqlx::query("UPDATE scores SET replay_path='rx.osr',replay_complete=true WHERE id=$1").bind(restricted_score).execute(pool).await.unwrap();
        assert_eq!(get(&router,&format!("/web/osu-getreplay.php?u=RelaxAlpha&ha={legacy_hash}&c={restricted_score}&m=0"),&token).await.0,StatusCode::NOT_FOUND);
        assert_eq!(get(&router,&format!("/api/v2/scores/{restricted_score}/download"),&token).await.0,StatusCode::NOT_FOUND);
        let stable_path=|mods|format!("/web/osu-osz2-getscores.php?u=RelaxAlpha&ha={legacy_hash}&c=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&m=0&mods={mods}&v=1");
        let(_,bytes)=get(&router,&stable_path(128),&token).await;
        let text=String::from_utf8(bytes).unwrap();let lines=text.lines().collect::<Vec<_>>();
        assert!(lines[5].starts_with(&format!("{br}|RelaxBeta|5000|")));assert!(lines[4].starts_with(&format!("{ar}|RelaxAlpha|20000|")));
        let bancho=common::secret();
        sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at) VALUES($1,$2,$3,now()+interval '3 minutes')")
            .bind(uuid::Uuid::new_v4()).bind(common::token_hash(&bancho)).bind(a).execute(pool).await.unwrap();
        for (mods,mode,pp,plays) in [(0,0,200,1),(128,0,20,4),(128,1,333,1)] {
            let action=UserAction{mods,mode,..UserAction::default()};let mut writer=Writer::new();action.write_to(&mut writer).unwrap();
            let packet=wire::encode_packet(wire::client::CHANGE_ACTION,&writer.into_bytes()).unwrap();
            let response=router.clone().oneshot(Request::builder().method("POST").uri("/").header("osu-token",&bancho).body(Body::from(packet)).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);let bytes=to_bytes(response.into_body(),1024*1024).await.unwrap();
            let packets=wire::decode_packets(&bytes).unwrap();let packet=packets.iter().find(|packet|packet.id==wire::server::USER_STATS).unwrap();
            let mut reader=Reader::new(packet.payload);assert_eq!(reader.i32().unwrap(),a as i32);
            reader.u8().unwrap();reader.string().unwrap();reader.string().unwrap();assert_eq!(reader.u32().unwrap(),mods);assert_eq!(reader.u8().unwrap(),mode);reader.i32().unwrap();
            reader.i64().unwrap();reader.f32().unwrap();assert_eq!(reader.i32().unwrap(),plays);reader.i64().unwrap();reader.i32().unwrap();assert_eq!(reader.u16().unwrap(),pp);reader.finish().unwrap();
        }
        assert_eq!(score_aggregates::ranking(pool,a,0,ScoreVariant::Vanilla).await.unwrap().get::<f64,_>("pp"),200.0);
    }).catch_unwind().await;
    database.cleanup().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
