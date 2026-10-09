//! Disposable PostgreSQL and TCP integration coverage for native room scores.
//! The controlled ingress is a crash/idempotency fixture, not a PP calculator.
use crate::{common, room_results, room_rounds, room_scores, signalr};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use futures_util::{FutureExt, SinkExt, StreamExt};
use resonance_protocol::{
    lazer::SoloScoreSubmission,
    signalr::{Codec, Protocol},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    PgPool, Row,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle};

const HASH: &str = "0123456789abcdef0123456789abcdef";
const VERSION: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone)]
struct Ingress {
    pool: PgPool,
    mode: Arc<AtomicU8>,
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

async fn ingest(
    State(state): State<Ingress>,
    Json(input): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(input["client"], "lazer");
    assert!(input.get("room_id").is_none());
    assert!(input.get("round_id").is_none());
    let mode = state.mode.load(Ordering::SeqCst);
    if mode == 3 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"fixture_disconnect_before_publication"})),
        );
    }
    if mode == 2 {
        state.entered.notify_one();
        state.release.notified().await;
    }
    let key = input["ingestion_key"].as_str().unwrap();
    let native: SoloScoreSubmission =
        serde_json::from_value(input["native_submission"].clone()).unwrap();
    native.validate().unwrap();
    let mut tx = state.pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(key)
        .execute(&mut *tx)
        .await
        .unwrap();
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM scores WHERE ingestion_key=$1")
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
    let id = if let Some(id) = existing {
        id
    } else {
        let active: bool = sqlx::query_scalar(
            "SELECT illya_beatmapset_active(native_set_id) FROM beatmaps WHERE id=$1",
        )
        .bind(input["beatmap_id"].as_i64().unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        if !active {
            return (
                StatusCode::GONE,
                Json(json!({"error":{"code":"beatmap_withdrawn"}})),
            );
        }
        let mods = input["mods"].as_i64().unwrap();
        sqlx::query_scalar::<_,i64>("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,native_submission,score_variant) VALUES($1,$2,$3,$4,$5,$6,$7,0,$8,$9,'lazer',$10,$11,$12,$13) RETURNING id")
            .bind(input["user_id"].as_i64().unwrap()).bind(input["beatmap_id"].as_i64().unwrap())
            .bind(input["mode"].as_i64().unwrap() as i16).bind(input["score"].as_i64().unwrap())
            .bind(input["accuracy"].as_f64().unwrap()).bind(input["max_combo"].as_i64().unwrap() as i32)
            .bind(mods).bind(input["passed"].as_bool().unwrap()).bind(&input["hit_counts"])
            .bind(key).bind(&input["api_mods"]).bind(&input["native_submission"])
            .bind(if mods & 128 != 0 { "relax" } else { "vanilla" }).fetch_one(&mut *tx).await.unwrap()
    };
    tx.commit().await.unwrap();
    if mode == 1 {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"fixture_commit_then_disconnect"})),
        )
    } else {
        (StatusCode::OK, Json(json!({"id":id})))
    }
}

struct Fixture {
    control: PgPool,
    state: common::AppState,
    schema: String,
    storage: PathBuf,
    ingress: Ingress,
    tasks: Vec<JoinHandle<()>>,
    base: String,
    http: reqwest::Client,
    users: Vec<i64>,
    tokens: Vec<String>,
    map: i64,
    native_map: i64,
    sibling: i64,
    native_sibling: i64,
}

impl Fixture {
    async fn new() -> Self {
        let url =
            std::env::var("ILLYA_TEST_DATABASE_URL").expect("disposable local PostgreSQL URL");
        let parsed = reqwest::Url::parse(&url).unwrap();
        assert!(
            matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "loopback fixture required"
        );
        let control = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("room_scores_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema)
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        let storage = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
            .join(format!(
                "data-room-score-fixtures-{}",
                uuid::Uuid::new_v4().simple()
            ));
        std::fs::create_dir(&storage).unwrap();
        std::fs::create_dir(storage.join("maps")).unwrap();
        std::fs::create_dir(storage.join("replays")).unwrap();
        let ingress = Ingress {
            pool: pool.clone(),
            mode: Arc::new(AtomicU8::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        let (ingress_url, ingress_task) = serve(
            Router::new()
                .route("/api/v1/internal/scores", post(ingest))
                .with_state(ingress.clone()),
        )
        .await;
        let state = common::AppState {
            pool: pool.clone(),
            storage: Arc::new(storage.canonicalize().unwrap()),
            sanctuary_url: Arc::new(ingress_url),
            service_key: Arc::new("original-room-fixture-service-key-32".into()),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let app = Router::new()
            .merge(room_scores::router())
            .merge(room_results::router())
            .merge(crate::lazer::router())
            .merge(signalr::router())
            .with_state(state.clone());
        let (base, gateway_task) = serve(app).await;
        let mut users = Vec::new();
        let mut tokens = Vec::new();
        for n in 0..4 {
            let id: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture-unused','fixture-unused') RETURNING id").bind(format!("RoomPlayer{n}")).bind(format!("room{n}@invalid.test")).fetch_one(&pool).await.unwrap();
            sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='player'")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
            let token = common::secret();
            session(&pool, id, &token).await;
            users.push(id);
            tokens.push(token);
        }
        let package: uuid::Uuid = sqlx::query_scalar("INSERT INTO beatmap_packages(kind,member_md5,archive_path,legacy) VALUES('osz',$1,'fixture.osz',true) RETURNING id").bind(vec![HASH,"1123456789abcdef0123456789abcdef"]).fetch_one(&pool).await.unwrap();
        let map: i64 = sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,stars,md5,osu_path,archive_path) VALUES($1,'Original fixture','Artist','Mapper','Round',0,1.5,$2,'unused.osu','fixture.osz') RETURNING id").bind(package).bind(HASH).fetch_one(&pool).await.unwrap();
        let sibling: i64 = sqlx::query_scalar("INSERT INTO beatmaps(local_package_id,title,artist,creator,version,mode,stars,md5,osu_path,archive_path) VALUES($1,'Original fixture','Artist','Mapper','Sibling',0,2.5,'1123456789abcdef0123456789abcdef','sibling.osu','fixture.osz') RETURNING id").bind(package).fetch_one(&pool).await.unwrap();
        let native_map = sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1")
            .bind(map)
            .fetch_one(&pool)
            .await
            .unwrap();
        let native_sibling = sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1")
            .bind(sibling)
            .fetch_one(&pool)
            .await
            .unwrap();
        Self {
            control,
            state,
            schema,
            storage,
            ingress,
            tasks: vec![ingress_task, gateway_task],
            base,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .build()
                .unwrap(),
            users,
            tokens,
            map,
            native_map,
            sibling,
            native_sibling,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn get(&self, user: usize, path: &str, expected: u16) -> Value {
        body(
            self.http
                .get(self.url(path))
                .bearer_auth(&self.tokens[user])
                .send()
                .await
                .unwrap(),
            expected,
        )
        .await
    }

    async fn token(&self, user: usize, room: i64, item: i64) -> i64 {
        let value = body(
            self.http
                .post(self.url(&format!("/api/v2/rooms/{room}/playlist/{item}/scores")))
                .bearer_auth(&self.tokens[user])
                .form(&[
                    ("beatmap_id", self.native_map.to_string()),
                    ("beatmap_hash", HASH.into()),
                    ("version_hash", VERSION.into()),
                    ("ruleset_id", "0".into()),
                ])
                .send()
                .await
                .unwrap(),
            200,
        )
        .await;
        value["id"].as_i64().unwrap()
    }

    async fn put(
        &self,
        user: usize,
        room: i64,
        item: i64,
        token: i64,
        input: &Value,
        expected: u16,
    ) -> Value {
        body(
            self.http
                .put(self.url(&format!(
                    "/api/v2/rooms/{room}/playlist/{item}/scores/{token}"
                )))
                .bearer_auth(&self.tokens[user])
                .json(input)
                .send()
                .await
                .unwrap(),
            expected,
        )
        .await
    }

    fn playlist(&self, item: i64) -> Value {
        json!({"id":item,"ownerID":self.users[0],"beatmapID":self.native_map,"beatmapChecksum":HASH,"rulesetID":0,"requiredMods":[],"allowedMods":[],"expired":false,"playlistOrder":item,"playedAt":null,"starRating":1.5,"freestyle":false})
    }

    async fn room(&self, members: &[usize]) -> i64 {
        let settings = json!({"name":"Room score fixture","password":"","playlistItemId":1,"matchType":1,"queueMode":0,"autoStartDuration":0,"autoSkip":false,"maxParticipants":16});
        let room = sqlx::query_scalar("INSERT INTO lazer_rooms(host_id,settings,playlist,next_playlist_item_id) VALUES($1,$2,$3,3) RETURNING id").bind(self.users[0]).bind(settings).bind(json!([self.playlist(1),self.playlist(2)])).fetch_one(&self.state.pool).await.unwrap();
        for member in members {
            sqlx::query("INSERT INTO lazer_room_members(room_id,user_id,state,beatmap_availability) VALUES($1,$2,1,'{\"state\":4,\"downloadProgress\":null}')").bind(room).bind(self.users[*member]).execute(&self.state.pool).await.unwrap();
        }
        let mut tx = self.state.pool.begin().await.unwrap();
        for item in 1..=2 {
            room_rounds::record_playlist(&mut tx, room, &self.playlist(item))
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        room
    }

    async fn round(&self, room: i64, item: i64, members: &[usize]) -> i64 {
        sqlx::query("UPDATE lazer_room_members SET state=1,beatmap_availability='{\"state\":4,\"downloadProgress\":null}' WHERE room_id=$1").bind(room).execute(&self.state.pool).await.unwrap();
        let mut tx = self.state.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM lazer_rooms WHERE id=$1 FOR UPDATE")
            .bind(room)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let ids: Vec<_> = members.iter().map(|i| self.users[*i]).collect();
        let round = room_rounds::freeze_round(&mut tx, room, &self.playlist(item), &ids)
            .await
            .unwrap();
        sqlx::query("UPDATE lazer_rooms SET state=1 WHERE id=$1")
            .bind(room)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE lazer_room_members SET state=2 WHERE room_id=$1")
            .bind(room)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        round
    }

    async fn finish(&self, round: i64) {
        let mut tx = self.state.pool.begin().await.unwrap();
        room_rounds::mark_finished(&mut tx, round).await.unwrap();
        tx.commit().await.unwrap();
    }

    async fn count(&self, query: &str) -> i64 {
        sqlx::query_scalar(query)
            .fetch_one(&self.state.pool)
            .await
            .unwrap()
    }

    async fn due(&self, token: i64) {
        sqlx::query("UPDATE lazer_score_tokens SET retry_at=now()-interval '1 second' WHERE id=$1")
            .bind(token)
            .execute(&self.state.pool)
            .await
            .unwrap();
    }

    async fn wait_lock(&self, pattern: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2")
                    .bind(&self.schema).bind(pattern).fetch_one(&self.control).await.unwrap();
                if count > 0 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("native operation must reach its actual PostgreSQL lock");
    }

    async fn seed_processed(
        &self,
        binding: (i64, i64, i64),
        user: usize,
        total: i64,
        passed: bool,
        accuracy: f64,
    ) -> (i64, i64) {
        let (room, item, round) = binding;
        let token: i64 = sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope,room_id,playlist_item_id,round_id) VALUES($1,$2,0,$3,now()+interval '1 hour','room',$4,$5,$6) RETURNING id").bind(self.users[user]).bind(self.map).bind(VERSION).bind(room).bind(item).bind(round).fetch_one(&self.state.pool).await.unwrap();
        let mut input = submission(total);
        input["passed"] = json!(passed);
        input["accuracy"] = json!(accuracy);
        let typed: SoloScoreSubmission = serde_json::from_value(input.clone()).unwrap();
        let score: i64 = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,native_submission) VALUES($1,$2,0,$3,$4,2,0,0,$5,$6,'lazer',$7,'[]',$8) RETURNING id").bind(self.users[user]).bind(self.map).bind(total).bind(accuracy*100.0).bind(passed).bind(typed.legacy_hit_counts()).bind(format!("lazer:{token}")).bind(json!(typed)).fetch_one(&self.state.pool).await.unwrap();
        let sha = format!("{:x}", Sha256::digest(serde_json::to_vec(&input).unwrap()));
        sqlx::query("UPDATE lazer_score_tokens SET score_id=$2,consumed_at=now(),processing_status='processed',submission=$3,submission_sha256=$4,received_at=now() WHERE id=$1").bind(token).bind(score).bind(input).bind(sha).execute(&self.state.pool).await.unwrap();
        (token, score)
    }

    // Bulk retained-history fixture only: production result queries must never
    // retrieve these raw attempts. Each token still has a separate frozen round.
    async fn seed_retained_history(&self, room: i64, attempts: i32, processed: usize) -> Vec<i64> {
        let rounds: Vec<i64> = sqlx::query_scalar("INSERT INTO lazer_room_rounds(room_id,playlist_item_id,playlist_version_id,status,finished_at) SELECT $1,1,(SELECT id FROM lazer_room_playlist_versions WHERE room_id=$1 AND playlist_item_id=1 ORDER BY id DESC LIMIT 1),'finished',now() FROM generate_series(1,$2) RETURNING id")
            .bind(room).bind(attempts).fetch_all(&self.state.pool).await.unwrap();
        sqlx::query("INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods) SELECT r,u,$3,$4,0,'[]' FROM unnest($1::bigint[]) r CROSS JOIN unnest($2::bigint[]) u")
            .bind(&rounds).bind(&self.users[..2]).bind(self.map).bind(HASH).execute(&self.state.pool).await.unwrap();
        let tokens: Vec<i64> = sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope,room_id,playlist_item_id,round_id) SELECT $1,$2,0,$3,now()+interval '1 hour','room',$4,1,r FROM unnest($5::bigint[]) r RETURNING id")
            .bind(self.users[0]).bind(self.map).bind(VERSION).bind(room).bind(&rounds).fetch_all(&self.state.pool).await.unwrap();
        let mut input = submission(350_000);
        input["accuracy"] = json!(0.95);
        let typed: SoloScoreSubmission = serde_json::from_value(input.clone()).unwrap();
        let scores = sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,native_submission) SELECT $1,$2,0,350000,95,2,0,0,true,$3,'lazer','lazer:'||t::text,'[]',$4 FROM unnest($5::bigint[]) t RETURNING id")
            .bind(self.users[0]).bind(self.map).bind(typed.legacy_hit_counts()).bind(json!(typed)).bind(&tokens[..processed]).fetch_all(&self.state.pool).await.unwrap();
        let sha = format!("{:x}", Sha256::digest(serde_json::to_vec(&input).unwrap()));
        sqlx::query("UPDATE lazer_score_tokens t SET score_id=s.id,consumed_at=now(),processing_status='processed',submission=$2,submission_sha256=$3,received_at=now() FROM scores s WHERE t.id=ANY($1) AND s.ingestion_key='lazer:'||t.id::text")
            .bind(&tokens[..processed]).bind(input).bind(sha).execute(&self.state.pool).await.unwrap();
        scores
    }

    async fn cleanup(self) {
        for task in self.tasks {
            task.abort();
            let _ = task.await;
        }
        self.state.pool.close().await;
        // Upgraded socket tasks may be unwinding a server-side transaction when
        // the listener ends. Terminate only this fixture's uniquely named peers
        // before dropping its schema, never another pool or player database.
        sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name=$1 AND pid<>pg_backend_pid()")
            .bind(&self.schema).execute(&self.control).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let actual = self.storage.canonicalize().unwrap();
        assert_eq!(actual.parent(), Some(root.as_path()));
        assert!(
            actual
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("data-room-score-fixtures-")
        );
        assert!(
            !std::fs::symlink_metadata(&self.storage)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::remove_dir_all(actual).unwrap();
    }
}

async fn session(pool: &PgPool, user: i64, token: &str) {
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')").bind(uuid::Uuid::new_v4()).bind(user).bind(common::token_hash(token)).bind(common::secret()).execute(pool).await.unwrap();
}

async fn serve(app: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (base, task)
}

async fn body(response: reqwest::Response, expected: u16) -> Value {
    let status = response.status().as_u16();
    let url = response.url().to_string();
    let text = response.text().await.unwrap();
    assert_eq!(status, expected, "{url}: {text}");
    if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).expect("native JSON response")
    }
}

fn submission(total: i64) -> Value {
    json!({"ruleset_id":0,"passed":true,"total_score":total,"total_score_without_mods":total,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]})
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_processed_legacy_receipt_retries_exact_payload_after_native_id_upgrade() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let room = f.room(&[0,1]).await;
        let round = f.round(room,1,&[0,1]).await;
        let (token, score) = f.seed_processed((room,1,round),0,600,true,1.0).await;
        f.finish(round).await;
        // Model a receipt accepted before0016: the durable FK still addresses
        // the same difficulty, but its saved public ID is the former local PK.
        assert_ne!(f.map,f.native_map);
        let mut original = submission(600);
        original["beatmap_id"] = json!(f.map);
        original["beatmap_hash"] = json!(HASH);
        original["version_hash"] = json!(VERSION);
        original["user_id"] = json!(f.users[0]);
        original["client_note"] = json!("original accepted legacy extension");
        let sha = format!("{:x}",Sha256::digest(serde_json::to_vec(&original).unwrap()));
        sqlx::query("UPDATE lazer_score_tokens SET submission=$2,submission_sha256=$3 WHERE id=$1")
            .bind(token).bind(&original).bind(&sha).execute(&f.state.pool).await.unwrap();
        let accepted = f.put(0,room,1,token,&original,200).await;
        assert_eq!(accepted["id"],score);
        assert_eq!(accepted["beatmap_id"],f.native_map);
        assert_eq!(f.put(0,room,1,token,&original,200).await["id"],score);
        let mut changed_id = original.clone();
        changed_id["beatmap_id"] = json!(f.native_map);
        assert_eq!(f.put(0,room,1,token,&changed_id,409).await["error"],"score_token_already_used");
        let mut changed_body = original.clone();
        changed_body["client_note"] = json!("changed legacy extension");
        assert_eq!(f.put(0,room,1,token,&changed_body,409).await["error"],"score_token_already_used");
        f.put(1,room,1,token,&original,404).await;
        // The receipt exception bypasses only the retired public ID. A broken
        // frozen checksum remains a rejection despite exact JSON and digest.
        sqlx::query("UPDATE lazer_room_round_participants SET beatmap_checksum=$3 WHERE round_id=$1 AND user_id=$2")
            .bind(round).bind(f.users[0]).bind("ffffffffffffffffffffffffffffffff").execute(&f.state.pool).await.unwrap();
        assert_eq!(f.put(0,room,1,token,&original,400).await["error"],"room_score_context_mismatch");
        sqlx::query("UPDATE lazer_room_round_participants SET beatmap_checksum=$3 WHERE round_id=$1 AND user_id=$2")
            .bind(round).bind(f.users[0]).bind(HASH).execute(&f.state.pool).await.unwrap();
        let receipt = sqlx::query("SELECT t.submission,t.submission_sha256,t.beatmap_id,t.score_id,p.beatmap_id frozen_map,s.beatmap_id score_map FROM lazer_score_tokens t JOIN lazer_room_round_participants p ON p.round_id=t.round_id AND p.user_id=t.user_id JOIN scores s ON s.id=t.score_id WHERE t.id=$1")
            .bind(token).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(receipt.get::<Value,_>("submission"),original);
        assert_eq!(receipt.get::<String,_>("submission_sha256"),sha);
        assert_eq!(receipt.get::<i64,_>("score_id"),score);
        for key in ["beatmap_id","frozen_map","score_map"] { assert_eq!(receipt.get::<i64,_>(key),f.map); }
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);
        assert_eq!(f.ingress.calls.load(Ordering::SeqCst),0);
        // An issued token still cannot accept the same retired public alias.
        let next = f.round(room,2,&[0,1]).await;
        let issued = f.token(0,room,2).await;
        f.finish(next).await;
        let mut new_submission = submission(600);
        new_submission["beatmap_id"] = json!(f.map);
        assert_eq!(f.put(0,room,2,issued,&new_submission,400).await["error"],"room_score_context_mismatch");
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_local_package_freestyle_and_tokens_bind_canonical_native_ids() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        assert_ne!(f.map, f.native_map);
        assert_ne!(f.sibling, f.native_sibling);
        let room = f.room(&[0, 1]).await;
        sqlx::query("UPDATE lazer_room_members SET beatmap_id=$2 WHERE room_id=$1 AND user_id=$3")
            .bind(room).bind(f.native_sibling).bind(f.users[1]).execute(&f.state.pool).await.unwrap();
        let mut snapshot = f.playlist(1);
        let mut tx = f.state.pool.begin().await.unwrap();
        let denied = room_rounds::freeze_round(&mut tx, room, &snapshot, &f.users[..2]).await.unwrap_err();
        assert_eq!(denied.1, "room_beatmap_not_allowed");
        tx.rollback().await.unwrap();
        snapshot["freestyle"] = json!(true);
        let mut tx = f.state.pool.begin().await.unwrap();
        let round = room_rounds::freeze_round(&mut tx, room, &snapshot, &f.users[..2]).await.unwrap();
        sqlx::query("UPDATE lazer_rooms SET state=1 WHERE id=$1").bind(room).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE lazer_room_members SET state=2 WHERE room_id=$1").bind(room).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let frozen: i64 = sqlx::query_scalar("SELECT beatmap_id FROM lazer_room_round_participants WHERE round_id=$1 AND user_id=$2")
            .bind(round).bind(f.users[1]).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(frozen, f.sibling);
        let context = json!({"beatmap_id":f.native_sibling,"beatmap_hash":"1123456789abcdef0123456789abcdef","version_hash":VERSION,"ruleset_id":0});
        let token = body(f.http.post(f.url(&format!("/api/v2/rooms/{room}/playlist/1/scores"))).bearer_auth(&f.tokens[1]).json(&context).send().await.unwrap(),200).await["id"].as_i64().unwrap();
        let internal: i64 = sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1").bind(token).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(internal, f.sibling);
        let mut old_alias = context;
        old_alias["beatmap_id"] = json!(f.sibling);
        let rejected = body(f.http.post(f.url(&format!("/api/v2/rooms/{room}/playlist/1/scores"))).bearer_auth(&f.tokens[1]).json(&old_alias).send().await.unwrap(),400).await;
        assert_eq!(rejected["error"], "room_score_context_mismatch");
        sqlx::query("UPDATE lazer_rooms SET state=2 WHERE id=$1").bind(room).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE lazer_room_members SET state=4 WHERE room_id=$1").bind(room).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE lazer_room_rounds SET status='playing' WHERE id=$1").bind(round).execute(&f.state.pool).await.unwrap();
        let mut score = submission(600);
        score["beatmap_id"] = json!(f.sibling);
        assert_eq!(f.put(1,room,1,token,&score,400).await["error"],"room_score_context_mismatch");
        score["beatmap_id"] = json!(f.native_sibling);
        let submitted = f.put(1,room,1,token,&score,200).await;
        assert_eq!(submitted["beatmap_id"], f.native_sibling);
        let stored: i64 = sqlx::query_scalar("SELECT beatmap_id FROM scores WHERE id=$1").bind(submitted["id"].as_i64().unwrap()).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(stored, f.sibling);
        let read = f.get(1,&format!("/api/v2/rooms/{room}/playlist/1/scores"),200).await;
        assert_eq!(read["scores"][0]["beatmap_id"],f.native_sibling);
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn frames(total: i64) -> Value {
    json!([[total,1.0,2,2,{"5":2},[600.0,600.0,2,0.5,0.0],null,[],null,null],[[100.0,256.0,192.0,1]]])
}

fn proof(name: &str, value: &Value) {
    if let Ok(path) = std::env::var("ILLYA_ROOM_DTO_OUTPUT") {
        let path = PathBuf::from(path).canonicalize().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        assert_eq!(path.parent(), Some(root.as_path()));
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("data-room-dto-proof-")
        );
        std::fs::write(path.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
struct Client {
    socket: Socket,
    binary: bool,
    counter: u32,
    events: Vec<Value>,
}
impl Client {
    async fn open(fixture: &Fixture, user: usize, hub: &str, binary: bool) -> Self {
        let url = format!(
            "{}/signalr/{hub}?access_token={}",
            fixture.base, fixture.tokens[user]
        )
        .replacen("http://", "ws://", 1);
        let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                format!(
                    "{{\"protocol\":\"{}\",\"version\":1}}\u{1e}",
                    if binary { "messagepack" } else { "json" }
                )
                .into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(4), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap(),
            "{}\u{1e}"
        );
        Self {
            socket,
            binary,
            counter: 0,
            events: Vec::new(),
        }
    }
    async fn read(&mut self) -> Value {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), self.socket.next())
                .await
                .expect("expected native event/completion")
                .unwrap()
                .unwrap();
            let value: Value = match msg {
                tokio_tungstenite::tungstenite::Message::Text(t) => {
                    serde_json::from_str(t.trim_end_matches('\u{1e}')).unwrap()
                }
                tokio_tungstenite::tungstenite::Message::Binary(b) => {
                    resonance_protocol::signalr::decode_messagepack_record(&b).unwrap()
                }
                tokio_tungstenite::tungstenite::Message::Ping(_)
                | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            };
            if (if self.binary {
                value[0].as_i64()
            } else {
                value["type"].as_i64()
            }) != Some(6)
            {
                return value;
            }
        }
    }
    async fn call(&mut self, target: &str, args: Vec<Value>) -> Value {
        self.counter += 1;
        let id = self.counter.to_string();
        let bytes = Codec::new(if self.binary {
            Protocol::MessagePack
        } else {
            Protocol::Json
        })
        .invocation_with_id(Some(&id), target, args)
        .unwrap();
        let msg = if self.binary {
            tokio_tungstenite::tungstenite::Message::Binary(bytes.into())
        } else {
            tokio_tungstenite::tungstenite::Message::Text(String::from_utf8(bytes).unwrap().into())
        };
        self.socket.send(msg).await.unwrap();
        loop {
            let record = self.read().await;
            if if self.binary {
                record[0] == 3 && record[2] == id
            } else {
                record["type"] == 3 && record["invocationId"] == id
            } {
                return record;
            }
            self.events.push(record);
        }
    }
    async fn ok(&mut self, target: &str, args: Vec<Value>) -> Value {
        let r = self.call(target, args).await;
        if self.binary {
            assert_ne!(r[3], 1, "{r}");
            if r[3] == 3 { r[4].clone() } else { Value::Null }
        } else {
            assert!(r.get("error").is_none(), "{r}");
            r["result"].clone()
        }
    }
    fn is_event(&self, r: &Value, target: &str) -> bool {
        if self.binary {
            r[3] == target
        } else {
            r["target"] == target
        }
    }
    async fn event(&mut self, target: &str) -> Value {
        let r = if let Some(i) = self.events.iter().position(|r| self.is_event(r, target)) {
            self.events.remove(i)
        } else {
            loop {
                let r = self.read().await;
                if self.is_event(&r, target) {
                    break r;
                }
                self.events.push(r);
            }
        };
        if self.binary {
            r[4].clone()
        } else {
            r["arguments"].clone()
        }
    }
    async fn no_event(&mut self, target: &str) {
        assert!(!self.events.iter().any(|r| self.is_event(r, target)));
        let result = tokio::time::timeout(Duration::from_millis(150), async {
            loop {
                let r = self.read().await;
                assert!(!self.is_event(&r, target), "unexpected event {r}");
                self.events.push(r);
            }
        })
        .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_round_lifecycle_tokens_history_and_http_results() {
    let f = Fixture::new().await;
    let result=std::panic::AssertUnwindSafe(async {
        let mut host=Client::open(&f,0,"multiplayer",false).await;
        let mut guest=Client::open(&f,1,"multiplayer",true).await;
        let settings=json!({"name":"Native lifecycle","password":"","playlistItemId":0,"matchType":1,"queueMode":0,"autoStartDuration":0,"autoSkip":false,"maxParticipants":16});
        let created=host.ok("CreateRoom",vec![json!({"settings":settings,"playlist":[f.playlist(0),f.playlist(0)]})]).await;
        let room=created["roomID"].as_i64().unwrap();
        guest.ok("JoinRoom",vec![json!(room)]).await;
        for c in [&mut host,&mut guest] {c.ok("ChangeBeatmapAvailability",vec![json!([4,null])]).await;c.ok("ChangeState",vec![json!(1)]).await;}
        host.ok("StartMatch",vec![]).await;
        host.event("LoadRequested").await; guest.event("LoadRequested").await;
        let round: i64=sqlx::query_scalar("SELECT current_round_id FROM lazer_rooms WHERE id=$1").bind(room).fetch_one(&f.state.pool).await.unwrap();
        let token=f.token(0,room,1).await;
        assert_eq!(f.token(0,room,1).await,token);
        let guest_token=f.token(1,room,1).await;
        assert_ne!(token,guest_token);
        f.get(2,&format!("/api/v2/rooms/{room}/playlist/1/scores"),404).await;
        let invalid=body(f.http.post(f.url(&format!("/api/v2/rooms/{room}/playlist/2/scores"))).bearer_auth(&f.tokens[0]).json(&json!({"beatmap_id":f.native_map,"beatmap_hash":HASH,"version_hash":VERSION,"ruleset_id":0})).send().await.unwrap(),400).await;
        assert_eq!(invalid["error"],"room_loading_participant_required");
        for c in [&mut host,&mut guest] {c.ok("ChangeState",vec![json!(3)]).await;c.ok("ChangeState",vec![json!(4)]).await;}
        host.event("GameplayStarted").await;guest.event("GameplayStarted").await;
        let mut metadata=Client::open(&f,1,"metadata",true).await;
        let stats=metadata.ok("BeginWatchingMultiplayerRoom",vec![json!(room)]).await;
        assert_eq!(stats[0].as_array().unwrap().len(),4);
        assert_eq!(stats[0][1],json!([0,0,0,0,0,0,0,0,0,0,0,0,0]));
        let mut player=Client::open(&f,0,"spectator",true).await;
        let mut same_user_other=Client::open(&f,0,"spectator",false).await;
        player.ok("BeginPlaySessionV2",vec![json!(token),json!([f.native_map,0,[],1,{"5":2}])]).await;
        player.ok("SendFrameDataV2",vec![json!(token),frames(1_000_000)]).await;
        let input=submission(1_000_000);
        let accepted=f.put(0,room,1,token,&input,200).await;
        proof("actual-score.json",&accepted);
        let score=accepted["id"].as_i64().unwrap();
        assert_eq!(accepted["verification"],"pending");assert_eq!(accepted["processed"],true);assert_eq!(accepted["ranked"],false);
        let event=metadata.event("MultiplayerRoomScoreSet").await;
        assert_eq!(event,json!([[room,1,score,f.users[0],1_000_000,1]]));
        proof("actual-event.json",&event[0]);
        proof("actual-stats.json",&stats[0]);
        player.no_event("UserScoreProcessed").await;
        player.ok("EndPlaySessionV2",vec![json!(token),json!(3)]).await;
        assert_eq!(player.event("UserScoreProcessed").await,json!([f.users[0],score]));
        assert!(f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores/{score}"),200).await["has_replay"].as_bool().unwrap());
        same_user_other.no_event("UserScoreProcessed").await;
        assert_eq!(f.put(0,room,1,token,&input,200).await["id"],score);
        let mut changed=input.clone();changed["total_score"]=json!(999_999);f.put(0,room,1,token,&changed,409).await;
        f.put(1,room,1,token,&input,404).await;
        body(f.http.put(f.url(&format!("/api/v2/beatmaps/{}/solo/scores/{token}",f.native_map))).bearer_auth(&f.tokens[0]).json(&input).send().await.unwrap(),404).await;
        let solo=body(f.http.post(f.url(&format!("/api/v2/beatmaps/{}/solo/scores",f.native_map))).bearer_auth(&f.tokens[0]).json(&json!({"beatmap_hash":HASH,"version_hash":VERSION,"ruleset_id":0})).send().await.unwrap(),200).await["id"].as_i64().unwrap();
        f.put(0,room,1,solo,&input,404).await;
        host.ok("ChangeState",vec![json!(6)]).await;guest.ok("ChangeState",vec![json!(6)]).await;
        host.event("ResultsReady").await;
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT status FROM lazer_room_rounds WHERE id=$1").bind(round).fetch_one(&f.state.pool).await.unwrap(),"finished");
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);
        let pending=f.get(1,&format!("/api/v2/rooms/{room}/playlist/1/scores/{score}"),200).await;
        assert_eq!(pending["id"],score);assert!(pending["scores_around"].is_object());
        f.get(1,&format!("/api/v2/scores/{score}"),404).await;
        f.get(1,&format!("/api/v2/rooms/{room}/playlist/2/scores/{score}"),404).await;
        let board=f.get(1,&format!("/api/v2/rooms/{room}/leaderboard"),200).await;
        proof("actual-aggregate.json",&board["leaderboard"][0]);
        assert_eq!(board["leaderboard"][0]["total_score"],1_000_000);
        let own=f.get(0,&format!("/api/v2/rooms/{room}"),200).await;
        proof("actual-summary.json",&own["current_user_score"]);
        proof("actual-index.json",&f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores"),200).await);
        assert_eq!(own["current_user_score"]["playlist_item_attempts"],json!([{"id":1,"attempts":1,"passed":true}]));
        host.ok("RemovePlaylistItem",vec![json!(1)]).await;
        host.ok("AddPlaylistItem",vec![f.playlist(0)]).await;
        let added=host.event("PlaylistItemAdded").await; assert_eq!(added[0]["id"],3);
        let mut edit=f.playlist(3);edit["requiredMods"]=json!([{"acronym":"HD"}]);host.ok("EditPlaylistItem",vec![edit]).await;
        let old: Value=sqlx::query_scalar("SELECT v.snapshot FROM lazer_room_rounds r JOIN lazer_room_playlist_versions v ON v.id=r.playlist_version_id WHERE r.id=$1").bind(round).fetch_one(&f.state.pool).await.unwrap(); assert_eq!(old["requiredMods"],json!([]));
        host.ok("LeaveRoom",vec![]).await;guest.ok("LeaveRoom",vec![]).await;
        signalr::Runtime::recover(&f.state.pool).await.unwrap();
        f.get(1,&format!("/api/v2/rooms/{room}/playlist/1/scores/{score}"),200).await;
        f.put(1,room,1,guest_token,&submission(800_000),200).await;
        assert_eq!(f.count("SELECT count(*) FROM scores").await,2);
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_score_receipts_recover_and_recheck_revoked_sessions() {
    let f = Fixture::new().await;
    let result=std::panic::AssertUnwindSafe(async {
        let room=f.room(&[0,1]).await;let round=f.round(room,1,&[0,1]).await;let token=f.token(0,room,1).await;f.finish(round).await;
        let mut input=submission(900_000); input["client_note"]=json!("immutable unknown native extension");
        f.ingress.mode.store(1,Ordering::SeqCst);f.put(0,room,1,token,&input,503).await;
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);assert_eq!(f.count("SELECT count(*) FROM lazer_room_score_events").await,0);
        let saved=sqlx::query("SELECT submission,submission_sha256,received_session_id,processing_status,score_id FROM lazer_score_tokens WHERE id=$1").bind(token).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(saved.get::<Value,_>("submission"),input); assert_eq!(saved.get::<String,_>("submission_sha256"),format!("{:x}",Sha256::digest(serde_json::to_vec(&input).unwrap())));assert!(saved.get::<Option<uuid::Uuid>,_>("received_session_id").is_some());assert!(saved.get::<Option<i64>,_>("score_id").is_none());
        let mut changed=input.clone(); changed["client_note"]=json!("mutated extension");f.put(0,room,1,token,&changed,409).await;
        assert_eq!(f.ingress.calls.load(Ordering::SeqCst),1);
        f.ingress.mode.store(0,Ordering::SeqCst); f.due(token).await;
        assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),1);
        assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),0);
        let first=f.put(0,room,1,token,&input,200).await;assert_eq!(first["total_score"],900_000);
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);assert_eq!(f.count("SELECT count(*) FROM lazer_room_score_events").await,1);
        sqlx::query("DELETE FROM lazer_sessions WHERE access_hash=$1").bind(common::token_hash(&f.tokens[0])).execute(&f.state.pool).await.unwrap();
        session(&f.state.pool,f.users[0],&f.tokens[0]).await;
        assert_eq!(f.put(0,room,1,token,&input,200).await["id"],first["id"]);
        let round2=f.round(room,2,&[0,1]).await;let revoked_token=f.token(0,room,2).await;f.finish(round2).await;
        f.ingress.mode.store(2,Ordering::SeqCst);
        let pending=f.http.put(f.url(&format!("/api/v2/rooms/{room}/playlist/2/scores/{revoked_token}"))).bearer_auth(&f.tokens[0]).json(&submission(700_000));
        let task=tokio::spawn(async move {pending.send().await.unwrap()});
        tokio::time::timeout(Duration::from_secs(5),f.ingress.entered.notified()).await.unwrap();
        let mut lock=f.state.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM lazer_rooms WHERE id=$1 FOR UPDATE NOWAIT").bind(room).fetch_one(&mut *lock).await.expect("HTTP ingestion must hold no room transaction");
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE NOWAIT").bind(f.users[0]).fetch_one(&mut *lock).await.expect("HTTP ingestion must hold no account transaction");
        sqlx::query("SELECT id FROM lazer_score_tokens WHERE id=$1 FOR UPDATE NOWAIT").bind(revoked_token).fetch_one(&mut *lock).await.expect("HTTP ingestion must hold no token transaction");
        sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1").bind(f.users[0]).execute(&mut *lock).await.unwrap();lock.commit().await.unwrap();
        f.ingress.mode.store(0,Ordering::SeqCst); f.ingress.release.notify_one();body(task.await.unwrap(),401).await;
        assert_eq!(f.count("SELECT count(*) FROM scores").await,2);
        f.due(revoked_token).await;assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),0);
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT processing_status FROM lazer_score_tokens WHERE id=$1").bind(revoked_token).fetch_one(&f.state.pool).await.unwrap(),"revoked");
        let board=f.get(1,&format!("/api/v2/rooms/{room}/leaderboard"),200).await;assert_eq!(board["leaderboard"][0]["total_score"],900_000);
        assert_eq!(f.count("SELECT count(*) FROM lazer_room_score_events").await,1);
        session(&f.state.pool,f.users[0],&f.tokens[0]).await;
        let round3=f.round(room,1,&[0,1]).await;let concurrent=f.token(0,room,1).await;f.finish(round3).await;
        let input=submission(1_050_000);
        let (a,b)=tokio::join!(f.put(0,room,1,concurrent,&input,200),f.put(0,room,1,concurrent,&input,200));assert_eq!(a["id"],b["id"]);
        assert_eq!(f.count("SELECT count(*) FROM scores").await,3);assert_eq!(f.count("SELECT count(*) FROM lazer_room_score_events").await,2);
        let round4=f.round(room,2,&[0,1]).await;let aborted=f.token(0,room,2).await;
        let mut tx=f.state.pool.begin().await.unwrap();room_rounds::abort_round(&mut tx,round4,"fixture_abort").await.unwrap();tx.commit().await.unwrap();f.put(0,room,2,aborted,&submission(10),400).await;
        let round5=f.round(room,2,&[0,1]).await;let expired=f.token(0,room,2).await;f.finish(round5).await;
        sqlx::query("UPDATE lazer_score_tokens SET expires_at=now()-interval '1 second' WHERE id=$1").bind(expired).execute(&f.state.pool).await.unwrap();f.put(0,room,2,expired,&submission(10),400).await;
        assert_eq!(f.count("SELECT count(*) FROM scores").await,3);
        // A moderator can reject the linked score before the replay's delayed
        // end arrives. Capture publication must leave that decision intact.
        let round6=f.round(room,2,&[0,1]).await;let rejected_token=f.token(0,room,2).await;
        sqlx::query("UPDATE lazer_room_members SET state=4 WHERE room_id=$1").bind(room).execute(&f.state.pool).await.unwrap();
        let mut player=Client::open(&f,0,"spectator",true).await;
        player.ok("BeginPlaySessionV2",vec![json!(rejected_token),json!([f.native_map,0,[],1,{"5":2}])]).await;
        player.ok("SendFrameDataV2",vec![json!(rejected_token),frames(600_000)]).await;
        let rejected=f.put(0,room,2,rejected_token,&submission(600_000),200).await["id"].as_i64().unwrap();
        sqlx::query("UPDATE scores SET verification='rejected' WHERE id=$1").bind(rejected).execute(&f.state.pool).await.unwrap();
        player.ok("EndPlaySessionV2",vec![json!(rejected_token),json!(3)]).await;player.no_event("UserScoreProcessed").await;
        f.finish(round6).await;
        assert!(crate::replay_capture::finalize(&f.state,f.users[0],rejected_token).await.unwrap().is_none());
        let row=sqlx::query("SELECT verification,replay_path FROM scores WHERE id=$1").bind(rejected).fetch_one(&f.state.pool).await.unwrap();assert_eq!(row.get::<String,_>("verification"),"rejected");assert!(row.get::<Option<String>,_>("replay_path").is_none());
        f.get(1,&format!("/api/v2/rooms/{room}/playlist/2/scores/{rejected}"),404).await;
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_score_aggregates_best_attempts_bins_and_cursors() {
    let f = Fixture::new().await;
    let result=std::panic::AssertUnwindSafe(async {
        let room=f.room(&[0,1,2]).await;let mut scores=Vec::new();
        for bin in 0..13 {
            let round=f.round(room,1,&[0,1,2]).await;
            let (_,score)=f.seed_processed((room,1,round),0,bin*100_000,true,0.95).await;scores.push(score);f.finish(round).await;
        }
        let round=f.round(room,1,&[0,1,2]).await;
        let (_,tie)=f.seed_processed((room,1,round),0,1_200_000,true,0.95).await;
        let (_,guest)=f.seed_processed((room,1,round),1,1_200_000,true,0.8).await;
        let (_,failed)=f.seed_processed((room,1,round),2,1_300_000,false,0.2).await;f.finish(round).await;
        let round=f.round(room,2,&[0,1,2]).await;
        f.seed_processed((room,2,round),0,200_000,true,0.7).await;
        f.seed_processed((room,2,round),1,400_000,true,1.0).await;
        f.token(2,room,2).await;f.finish(round).await;
        let board=f.get(0,&format!("/api/v2/rooms/{room}/leaderboard"),200).await;
        assert_eq!(board["leaderboard"].as_array().unwrap().len(),3);
        assert_eq!(board["leaderboard"][0]["user_id"],f.users[1]);assert_eq!(board["leaderboard"][0]["total_score"],1_600_000);assert_eq!(board["leaderboard"][0]["completed"],2);assert!((board["leaderboard"][0]["accuracy"].as_f64().unwrap()-0.9).abs()<1e-10);
        assert_eq!(board["leaderboard"][1]["user_id"],f.users[0]);assert_eq!(board["leaderboard"][1]["total_score"],1_400_000);assert_eq!(board["leaderboard"][1]["attempts"],15);
        assert_eq!(board["leaderboard"][2]["total_score"],1_300_000);assert_eq!(board["leaderboard"][2]["attempts"],2);
        let viewer=common::account(&f.state,f.users[0]).await.unwrap();
        let stats=room_results::playlist_stats(&f.state,&viewer,room).await.unwrap();
        assert_eq!(stats[0]["totalScoreDistribution"],json!([1,1,1,1,1,1,1,1,1,1,1,1,3]));assert_eq!(stats[0]["cumulativeScore"],10_200_000);
        assert_eq!(stats[0]["lastProcessedScoreID"],failed);
        let page=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1"),200).await;
        assert_eq!(page["scores"][0]["id"],failed);assert_eq!(page["total"],3);assert_eq!(page["user_score"]["id"],scores[12]);assert_ne!(page["user_score"]["id"],tie);
        let cursor=&page["cursor"];
        let next=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1&cursor[total_score]={}&cursor[id]={}",cursor["total_score"],cursor["id"]),200).await;
        assert_eq!(next["scores"][0]["id"],scores[12]);
        let third=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1&cursor[total_score]={}&cursor[id]={}",next["cursor"]["total_score"],next["cursor"]["id"]),200).await;
        assert_eq!(third["scores"][0]["id"],guest);assert!(third["cursor"].is_null());
        let pivot=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores/users/{}",f.users[0]),200).await;
        assert_eq!(pivot["scores_around"]["higher"]["scores"][0]["id"],failed);assert_eq!(pivot["scores_around"]["lower"]["scores"][0]["id"],guest);
        let own=f.get(0,&format!("/api/v2/rooms/{room}"),200).await;assert_eq!(own["playlist"][0]["attempts"],14);assert_eq!(own["current_user_score"]["playlist_item_attempts"][0]["attempts"],14);
        sqlx::query("INSERT INTO lazer_room_members(room_id,user_id) VALUES($1,$2)").bind(room).bind(f.users[3]).execute(&f.state.pool).await.unwrap();
        let stranger=f.get(3,&format!("/api/v2/rooms/{room}/playlist/1/scores"),200).await;assert_eq!(stranger["total"],0);assert!(stranger["scores"].as_array().unwrap().is_empty());
        f.get(3,&format!("/api/v2/rooms/{room}/playlist/1/scores/{guest}"),404).await;
        sqlx::query("UPDATE scores SET verification='rejected' WHERE id=$1").bind(guest).execute(&f.state.pool).await.unwrap();
        f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores/{guest}"),404).await;f.get(1,&format!("/api/v2/rooms/{room}/playlist/1/scores/{guest}"),200).await;
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(f.users[2]).execute(&f.state.pool).await.unwrap();
        let reduced=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores"),200).await;assert_eq!(reduced["total"],1);
        sqlx::query("UPDATE lazer_rooms SET settings=jsonb_set(settings,'{matchType}','0') WHERE id=$1").bind(room).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(f.users[2]).execute(&f.state.pool).await.unwrap();
        let playlist_policy=f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores"),200).await;assert_eq!(playlist_policy["total"],1);
        sqlx::query("UPDATE scores SET ingestion_key='corrupted-room-link' WHERE id=$1").bind(scores[12]).execute(&f.state.pool).await.unwrap();
        f.get(0,&format!("/api/v2/rooms/{room}/playlist/1/scores/{}",scores[12]),404).await;
        let bad_token:i64=sqlx::query_scalar("SELECT id FROM lazer_score_tokens WHERE score_id=$1").bind(scores[12]).fetch_one(&f.state.pool).await.unwrap();
        assert!(room_scores::processed_score_id(&f.state,f.users[0],bad_token).await.unwrap().is_none());
        // Two rounds overlap only through B. C must not infer A's private first
        // round from a globally stored new-best rank in C's second-round push.
        sqlx::query("DELETE FROM lazer_room_members").execute(&f.state.pool).await.unwrap();
        let private=f.room(&[0,1]).await;let first=f.round(private,1,&[0,1]).await;
        f.seed_processed((private,1,first),0,1_000_000,true,1.0).await;f.finish(first).await;
        sqlx::query("INSERT INTO lazer_room_members(room_id,user_id,state,beatmap_availability) VALUES($1,$2,1,'{\"state\":4}')").bind(private).bind(f.users[2]).execute(&f.state.pool).await.unwrap();
        let second=f.round(private,1,&[1,2]).await;
        let (t,cscore)=f.seed_processed((private,1,second),2,800_000,true,1.0).await;f.finish(second).await;
        let mut b=Client::open(&f,1,"metadata",false).await;let mut c=Client::open(&f,2,"metadata",true).await;let mut a=Client::open(&f,0,"metadata",false).await;
        for connection in [&mut a,&mut b,&mut c]{connection.ok("BeginWatchingMultiplayerRoom",vec![json!(private)]).await;}
        sqlx::query("INSERT INTO lazer_room_score_events(token_id,room_id,item_id,score_id,user_id,total_score,new_rank) VALUES($1,$2,1,$3,$4,800000,2)").bind(t).bind(private).bind(cscore).bind(f.users[2]).execute(&f.state.pool).await.unwrap();
        assert_eq!(signalr::publish_room_score_events(&f.state).await.unwrap(),1);
        assert_eq!(b.event("MultiplayerRoomScoreSet").await[0]["newRank"],2);
        assert_eq!(c.event("MultiplayerRoomScoreSet").await[0][5],1);
        a.no_event("MultiplayerRoomScoreSet").await;
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_lifecycle_rechecks_queued_credentials_and_locks_users_in_order() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let room = f.room(&[0, 1]).await;
        let mut host = Client::open(&f, 0, "multiplayer", false).await;
        let mut blocker = f.state.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM lazer_rooms WHERE id=$1 FOR UPDATE")
            .bind(room)
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            let result = host.call("StartMatch", vec![]).await;
            (host, result)
        });
        f.wait_lock("%FROM lazer_rooms%FOR UPDATE%").await;
        sqlx::query("DELETE FROM lazer_sessions WHERE access_hash=$1")
            .bind(common::token_hash(&f.tokens[0]))
            .execute(&f.state.pool)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
        let (mut host, denied) = task.await.unwrap();
        assert_eq!(denied["error"], "session_revoked");
        assert_eq!(f.count("SELECT count(*) FROM lazer_room_rounds").await, 0);
        session(&f.state.pool, f.users[0], &f.tokens[0]).await;
        host.ok("StartMatch", vec![]).await;
        let token = f.token(0, room, 1).await;
        let mut blocker = f.state.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM lazer_rooms WHERE id=$1 FOR UPDATE")
            .bind(room)
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            let result = host.call("AbortMatch", vec![]).await;
            (host, result)
        });
        f.wait_lock("%FROM lazer_rooms%FOR UPDATE%").await;
        sqlx::query("DELETE FROM lazer_sessions WHERE access_hash=$1")
            .bind(common::token_hash(&f.tokens[0]))
            .execute(&f.state.pool)
            .await
            .unwrap();
        blocker.commit().await.unwrap();
        let (mut host, denied) = task.await.unwrap();
        assert_eq!(denied["error"], "session_revoked");
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT processing_status FROM lazer_score_tokens WHERE id=$1"
            )
            .bind(token)
            .fetch_one(&f.state.pool)
            .await
            .unwrap(),
            "issued"
        );
        assert_eq!(
            f.count("SELECT count(*) FROM lazer_room_rounds WHERE status='loading'")
                .await,
            1
        );
        session(&f.state.pool, f.users[0], &f.tokens[0]).await;
        host.ok("AbortMatch", vec![]).await;
        // A high-ID host must wait on the lower account first. If its guard
        // held high before waiting low, this sorted two-user writer deadlocks.
        sqlx::query("UPDATE lazer_rooms SET host_id=$2 WHERE id=$1")
            .bind(room)
            .bind(f.users[1])
            .execute(&f.state.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE lazer_room_members SET state=1 WHERE room_id=$1")
            .bind(room)
            .execute(&f.state.pool)
            .await
            .unwrap();
        let mut high_host = Client::open(&f, 1, "multiplayer", true).await;
        let mut sorted_writer = f.state.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(f.users[0])
            .fetch_one(&mut *sorted_writer)
            .await
            .unwrap();
        let start = tokio::spawn(async move {
            let r = high_host.call("StartMatch", vec![]).await;
            (high_host, r)
        });
        f.wait_lock("%FROM users%FOR UPDATE%").await;
        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(f.users[1])
                .fetch_one(&mut *sorted_writer),
        )
        .await
        .expect("higher account must remain available until lower is locked")
        .unwrap();
        sorted_writer.commit().await.unwrap();
        let (_high_host, started) = start.await.unwrap();
        assert_eq!(started[3], 2, "{started}");
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_withdrawal_blocks_admission_defers_unsaved_intents_and_links_persisted_receipts() {
    let f = Fixture::new().await;
    let result=std::panic::AssertUnwindSafe(async {
        let room=f.room(&[0,1,2]).await; let round=f.round(room,1,&[0,1,2]).await;
        let saved=f.token(0,room,1).await; let unsaved=f.token(1,room,1).await; let issued=f.token(2,room,1).await;
        let payload=submission(123456);
        f.ingress.mode.store(1,Ordering::SeqCst);
        f.put(0,room,1,saved,&payload,503).await;
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);
        f.ingress.mode.store(3,Ordering::SeqCst); f.put(1,room,1,unsaved,&payload,503).await;
        let received_before:Value=sqlx::query_scalar("SELECT jsonb_build_object('submission',submission,'sha',submission_sha256,'attempts',processing_attempts,'status',processing_status) FROM lazer_score_tokens WHERE id=$1").bind(unsaved).fetch_one(&f.state.pool).await.unwrap();
        let calls_before=f.ingress.calls.load(Ordering::SeqCst);
        let set:i64=sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1").bind(f.map).fetch_one(&f.state.pool).await.unwrap();
        sqlx::query("INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id) VALUES($1,true,1,'Room fixture withdrawal',$2)").bind(set).bind(f.users[0]).execute(&f.state.pool).await.unwrap();
        for action in ["playlist","play","round"] {
            let mut tx=f.state.pool.begin().await.unwrap();
            let error=match action {
                "playlist"=>room_rounds::record_playlist(&mut tx,room,&f.playlist(3)).await.unwrap_err(),
                "play"=>room_rounds::mark_playing(&mut tx,round).await.unwrap_err(),
                _=>room_rounds::freeze_round(&mut tx,room,&f.playlist(1),&[f.users[0]]).await.unwrap_err(),
            };
            assert_eq!(error.0,StatusCode::GONE); assert_eq!(error.1,"beatmap_withdrawn");
            tx.rollback().await.unwrap();
        }
        let denied=body(f.http.post(f.url(&format!("/api/v2/rooms/{room}/playlist/1/scores"))).bearer_auth(&f.tokens[1]).json(&json!({"beatmap_id":f.native_map,"beatmap_hash":HASH,"version_hash":VERSION,"ruleset_id":0})).send().await.unwrap(),410).await;
        assert_eq!(denied["error"],"beatmap_withdrawn");
        assert_eq!(f.put(2,room,1,issued,&payload,410).await["error"],"beatmap_withdrawn");
        let issued_after=sqlx::query("SELECT submission,processing_status,processing_attempts FROM lazer_score_tokens WHERE id=$1").bind(issued).fetch_one(&f.state.pool).await.unwrap();
        assert!(issued_after.get::<Option<Value>,_>("submission").is_none()); assert_eq!(issued_after.get::<String,_>("processing_status"),"issued"); assert_eq!(issued_after.get::<i32,_>("processing_attempts"),0);
        assert_eq!(f.put(1,room,1,unsaved,&payload,410).await["error"],"beatmap_withdrawn");
        let received_after:Value=sqlx::query_scalar("SELECT jsonb_build_object('submission',submission,'sha',submission_sha256,'attempts',processing_attempts,'status',processing_status) FROM lazer_score_tokens WHERE id=$1").bind(unsaved).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(received_before,received_after); assert_eq!(f.ingress.calls.load(Ordering::SeqCst),calls_before);
        f.ingress.mode.store(0,Ordering::SeqCst);
        f.due(saved).await;
        assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),1);
        let accepted=f.put(0,room,1,saved,&payload,200).await;
        assert!(accepted["id"].as_i64().is_some());
        assert_eq!(f.put(1,room,1,unsaved,&payload,410).await["error"],"beatmap_withdrawn");
        // A long withdrawal retains the saved intent, without extending its
        // existing security window. Restoration applies the normal expiry rule.
        sqlx::query("UPDATE lazer_score_tokens SET expires_at=now()-interval '1 second',retry_at=now()-interval '1 second' WHERE id=$1").bind(unsaved).execute(&f.state.pool).await.unwrap();
        assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),0);
        let retained=sqlx::query("SELECT processing_status,processing_error,revoked_at,submission FROM lazer_score_tokens WHERE id=$1").bind(unsaved).fetch_one(&f.state.pool).await.unwrap();
        assert_eq!(retained.get::<String,_>("processing_status"),"received"); assert_eq!(retained.get::<Option<String>,_>("processing_error").as_deref(),Some("beatmap_withdrawn")); assert!(retained.get::<Option<chrono::DateTime<chrono::Utc>>,_>("revoked_at").is_none()); assert!(retained.get::<Option<Value>,_>("submission").is_some());
        let summary=f.get(0,&format!("/api/v2/rooms/{room}"),200).await;
        assert_eq!(summary["playlist"][0]["beatmap"]["is_scoreable"],false);
        assert_eq!(f.count("SELECT count(*) FROM scores").await,1);
        sqlx::query("UPDATE beatmapset_availability SET withdrawn=false,revision=revision+1,reason='Room fixture restoration' WHERE native_set_id=$1").bind(set).execute(&f.state.pool).await.unwrap();
        f.due(unsaved).await; assert_eq!(room_scores::recover_pending(&f.state).await.unwrap(),0);
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT processing_status FROM lazer_score_tokens WHERE id=$1").bind(unsaved).fetch_one(&f.state.pool).await.unwrap(),"revoked");
        assert_eq!(f.put(0,room,1,saved,&payload,200).await["id"],accepted["id"]);
        assert!(f.put(2,room,1,issued,&payload,200).await["id"].as_i64().is_some());
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_results_large_history_preserves_complete_aggregates_and_bounded_windows() {
    let mut f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let room = f.room(&[0, 1, 2]).await;
        let old = f.seed_retained_history(room, 950, 850).await;
        assert_eq!(old.len(), 850);
        let first = *old.iter().min().unwrap();
        let failed_round = f.round(room, 1, &[0, 1]).await;
        let (_, failed) = f.seed_processed((room, 1, failed_round), 1, 1_400_000, false, 0.5).await;
        f.finish(failed_round).await;
        let mut extra = Vec::new();
        for index in 0..122 {
            let user: i64 = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture-unused','fixture-unused') RETURNING id")
                .bind(format!("RetainedPlayer{index}")).bind(format!("retained{index}@invalid.test")).fetch_one(&f.state.pool).await.unwrap();
            sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='player'").bind(user).execute(&f.state.pool).await.unwrap();
            let credential = common::secret();
            session(&f.state.pool, user, &credential).await;
            f.users.push(user);
            f.tokens.push(credential);
            // Each distinct historical player shares a two-person frozen round
            // with A, never B. Current membership does not reveal these scores.
            let mut round = f.round(room, 1, &[0]).await;
            sqlx::query("INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods) VALUES($1,$2,$3,$4,0,'[]')")
                .bind(round).bind(user).bind(f.map).bind(HASH).execute(&f.state.pool).await.unwrap();
            if index == 0 {
                f.seed_processed((room, 1, round), index + 4, 600_000, false, 0.5).await;
                f.finish(round).await;
                round = f.round(room, 1, &[0]).await;
                sqlx::query("INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods) VALUES($1,$2,$3,$4,0,'[]')")
                    .bind(round).bind(user).bind(f.map).bind(HASH).execute(&f.state.pool).await.unwrap();
            }
            let (_, score) = f.seed_processed((room, 1, round), index + 4, 900_000, true, 0.8).await;
            extra.push(score);
            f.finish(round).await;
        }
        assert_eq!(f.count("SELECT count(*) FROM lazer_score_tokens WHERE scope='room'").await, 1074);
        assert_eq!(f.count("SELECT count(*) FROM scores").await, 974);
        let mut canonical = vec![failed];
        canonical.extend(extra.iter().copied());
        canonical.push(first);
        let positions: std::collections::HashMap<_, _> = canonical.iter().enumerate().map(|(n, id)| (*id, n + 1)).collect();
        for sort in ["score_desc", "score_asc", "id_desc", "id_asc"] {
            let mut expected = canonical.clone();
            match sort {
                "score_asc" => expected.reverse(),
                "id_desc" => expected.sort_unstable_by(|a, b| b.cmp(a)),
                "id_asc" => expected.sort_unstable(),
                _ => {}
            }
            let mut actual = Vec::new();
            let mut suffix = String::new();
            loop {
                let page = f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores?sort={sort}&limit=31{suffix}"), 200).await;
                assert_eq!(page["total"], 124);
                assert_eq!(page["user_score"]["id"], first);
                assert_eq!(page["user_score"]["position"], 124);
                let scores = page["scores"].as_array().unwrap();
                assert!(scores.len() <= 31);
                for score in scores {
                    let id = score["id"].as_i64().unwrap();
                    assert_eq!(score["position"], positions[&id]);
                    actual.push(id);
                }
                let cursor = &page["cursor"];
                if cursor.is_null() { break; }
                assert!(!scores.is_empty());
                suffix = format!("&cursor[id]={}", cursor["id"]);
                if sort.starts_with("score_") {
                    suffix.push_str(&format!("&cursor[total_score]={}", cursor["total_score"]));
                }
            }
            assert_eq!(actual, expected, "full native keyset ordering: {sort}");
            let pivot = extra[60];
            let index = expected.iter().position(|id| *id == pivot).unwrap();
            let value = f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores/{pivot}?sort={sort}&limit=2"), 200).await;
            let higher = value["scores_around"]["higher"]["scores"].as_array().unwrap();
            let lower = value["scores_around"]["lower"]["scores"].as_array().unwrap();
            assert_eq!(higher.iter().map(|row| row["id"].as_i64().unwrap()).collect::<Vec<_>>(), expected[index - 2..index].iter().rev().copied().collect::<Vec<_>>());
            assert_eq!(lower.iter().map(|row| row["id"].as_i64().unwrap()).collect::<Vec<_>>(), expected[index + 1..index + 3]);
            assert!(!value["scores_around"]["higher"]["cursor"].is_null());
            assert!(!value["scores_around"]["lower"]["cursor"].is_null());
        }
        let maximum = f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores?limit=100"), 200).await;
        assert_eq!(maximum["scores"].as_array().unwrap().len(), 100);
        assert!(!maximum["cursor"].is_null());
        let board = f.get(0, &format!("/api/v2/rooms/{room}/leaderboard"), 200).await;
        assert_eq!(board["leaderboard"].as_array().unwrap().len(), 124);
        assert_eq!(board["leaderboard"][0]["user_id"], f.users[1]);
        assert_eq!(board["leaderboard"][0]["total_score"], 1_400_000);
        assert_eq!(board["leaderboard"][123]["user_id"], f.users[0]);
        assert_eq!(board["user_score"]["attempts"], 950);
        assert_eq!(board["user_score"]["position"], 124);
        assert_eq!(board["user_score"]["total_score"], 350_000);
        let viewer = common::account(&f.state, f.users[0]).await.unwrap();
        let summary = room_results::room_summary(&f.state, &viewer, room).await.unwrap();
        assert_eq!(summary["attempts"], 950);
        assert_eq!(summary["playlist_item_attempts"], json!([{"id":1,"attempts":950,"passed":true}]));
        let attempts = room_results::item_attempts(&f.state, &viewer, room, 1).await.unwrap();
        assert_eq!(attempts, json!({"attempts":950,"completed":true,"passed":true}));
        let stats = room_results::playlist_stats(&f.state, &viewer, room).await.unwrap();
        assert_eq!(stats[0]["totalScoreDistribution"], json!([0,0,0,850,0,0,0,0,0,122,0,0,0]));
        assert_eq!(stats[0]["cumulativeScore"], 407_300_000);
        assert_eq!(stats[0]["lastProcessedScoreID"], *extra.last().unwrap());
        assert_eq!(stats[1]["totalScoreDistribution"], json!(vec![0_i64; 13]));
        let other = common::account(&f.state, f.users[1]).await.unwrap();
        let private = f.get(1, &format!("/api/v2/rooms/{room}/playlist/1/scores?limit=100"), 200).await;
        assert_eq!(private["total"], 2);
        assert_eq!(private["scores"].as_array().unwrap().len(), 2);
        f.get(1, &format!("/api/v2/rooms/{room}/playlist/1/scores/{}", extra[0]), 404).await;
        assert_eq!(room_results::event_rank(&f.state, &other, room, 1, extra[0]).await.unwrap(), None);
        assert_eq!(room_results::event_rank(&f.state, &viewer, room, 1, extra[0]).await.unwrap(), Some(2));
        let other_stats = room_results::playlist_stats(&f.state, &other, room).await.unwrap();
        assert_eq!(other_stats[0]["cumulativeScore"], 297_500_000);
        // Every reader must work with one pooled connection. Nesting an
        // acquisition during snapshot formatting would fail this positive gate.
        let options = PgConnectOptions::from_str(&std::env::var("ILLYA_TEST_DATABASE_URL").unwrap()).unwrap()
            .application_name(&f.schema).options([("search_path", f.schema.as_str())]);
        let single = PgPoolOptions::new().max_connections(1).acquire_timeout(Duration::from_secs(2)).connect_with(options).await.unwrap();
        let mut single_state = f.state.clone();
        single_state.pool = single.clone();
        let single_board = tokio::time::timeout(Duration::from_secs(10), room_results::room_leaderboard(&single_state, &viewer, room)).await.unwrap().unwrap();
        assert_eq!(single_board, board);
        assert_eq!(room_results::room_summary(&single_state, &viewer, room).await.unwrap(), summary);
        assert_eq!(room_results::playlist_stats(&single_state, &viewer, room).await.unwrap(), stats);
        let (single_base, single_task) = serve(room_results::router().with_state(single_state)).await;
        f.tasks.push(single_task);
        for path in [
            format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1&scores_around=1"),
            format!("/api/v2/rooms/{room}/playlist/1/scores/{first}?limit=1"),
            format!("/api/v2/rooms/{room}/playlist/1/scores/users/{}?limit=1", f.users[0]),
        ] {
            let response = tokio::time::timeout(Duration::from_secs(10), f.http.get(format!("{single_base}{path}")).bearer_auth(&f.tokens[0]).send()).await.unwrap().unwrap();
            let output = body(response, 200).await;
            if output.get("scores").is_some() { assert_eq!(output["total"], 124); }
            else { assert_eq!(output["id"], first); assert_eq!(output["position"], 124); }
        }
        single.close().await;
        // Pause the production leaderboard reader after its first SQL chunk.
        // The account formatter reads bancho_sessions; the result-query CTE
        // does not, so this lock reaches the true between-chunks boundary.
        let mut blocker = f.state.pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE bancho_sessions IN ACCESS EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
        let snapshot_state = f.state.clone();
        let snapshot_viewer = viewer.clone();
        let paused = tokio::spawn(async move { room_results::room_leaderboard(&snapshot_state, &snapshot_viewer, room).await.unwrap() });
        f.wait_lock("%FROM users WHERE id=%").await;
        // One emitted user falls behind the cursor by selecting its failed
        // backup. One not-yet-emitted user moves before it with a new best.
        sqlx::query("UPDATE scores SET verification='rejected' WHERE id=$1").bind(extra[0]).execute(&f.state.pool).await.unwrap();
        let replacement_round = f.round(room, 1, &[0]).await;
        let (replacement_token, _) = f.seed_processed((room, 1, replacement_round), 0, 2_000_000, true, 1.0).await;
        f.finish(replacement_round).await;
        blocker.commit().await.unwrap();
        let snapshot_board = paused.await.unwrap();
        assert_eq!(snapshot_board, board, "one complete response retains one history snapshot across concurrent highs/rejections");
        let updated = f.get(0, &format!("/api/v2/rooms/{room}/leaderboard"), 200).await;
        assert_eq!(updated["leaderboard"].as_array().unwrap().len(), 124);
        assert_eq!(updated["user_score"]["total_score"], 2_000_000);
        assert_eq!(updated["user_score"]["attempts"], 951);
        assert_eq!(updated["user_score"]["position"], 1);
        assert_eq!(updated["leaderboard"].as_array().unwrap().iter().filter(|row| row["user_id"] == f.users[4]).count(), 1);
        sqlx::query("UPDATE lazer_score_tokens SET revoked_at=now() WHERE id=$1").bind(replacement_token).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE scores SET verification='pending' WHERE id=$1").bind(extra[0]).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE scores SET verification='rejected' WHERE id=$1").bind(extra[0]).execute(&f.state.pool).await.unwrap();
        f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores/{}", extra[0]), 404).await;
        let rejected = f.get(4, &format!("/api/v2/rooms/{room}/playlist/1/scores/{}", extra[0]), 200).await;
        assert!(rejected["position"].is_null());
        assert!(rejected["scores_around"].is_null());
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(f.users[5]).execute(&f.state.pool).await.unwrap();
        let reduced = f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1"), 200).await;
        assert_eq!(reduced["total"], 123);
        let reduced_stats = room_results::playlist_stats(&f.state, &viewer, room).await.unwrap();
        assert_eq!(reduced_stats[0]["totalScoreDistribution"][9], 120);
        assert_eq!(reduced_stats[0]["cumulativeScore"], 405_500_000);
        // A later live member sees only empty snapshots, never private retained
        // histories, and the moderator exception remains server-authorized.
        sqlx::query("INSERT INTO lazer_room_members(room_id,user_id) VALUES($1,$2)").bind(room).bind(f.users[3]).execute(&f.state.pool).await.unwrap();
        let newcomer = f.get(3, &format!("/api/v2/rooms/{room}/playlist/1/scores"), 200).await;
        assert_eq!(newcomer["total"], 0);
        assert!(newcomer["user_score"].is_null());
        let empty_board = f.get(3, &format!("/api/v2/rooms/{room}/leaderboard"), 200).await;
        assert!(empty_board["leaderboard"].as_array().unwrap().is_empty());
        sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='moderator'").bind(f.users[3]).execute(&f.state.pool).await.unwrap();
        let moderator = f.get(3, &format!("/api/v2/rooms/{room}/playlist/1/scores?limit=100"), 200).await;
        assert_eq!(moderator["total"], 123);
        f.get(3, &format!("/api/v2/rooms/{room}/playlist/1/scores/{}?scores_around=0", extra[0]), 200).await;
        sqlx::query("UPDATE lazer_rooms SET settings=jsonb_set(settings,'{matchType}','0') WHERE id=$1").bind(room).execute(&f.state.pool).await.unwrap();
        let playlist = f.get(0, &format!("/api/v2/rooms/{room}/playlist/1/scores?limit=1"), 200).await;
        assert_eq!(playlist["total"], 121);
        assert_ne!(playlist["scores"][0]["id"], failed);
    }).catch_unwind().await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn room_results_keep_all_retained_item_aggregates_across_sql_batches() {
    let f = Fixture::new().await;
    let result = std::panic::AssertUnwindSafe(async {
        let room = f.room(&[0, 1, 2]).await;
        for item in 1..=123 {
            let round = f.round(room, item, &[0, 1]).await;
            f.seed_processed((room, item, round), 0, item * 1000, true, 0.9)
                .await;
            f.finish(round).await;
        }
        let viewer = common::account(&f.state, f.users[0]).await.unwrap();
        let summary = room_results::room_summary(&f.state, &viewer, room)
            .await
            .unwrap();
        assert_eq!(summary["attempts"], 123);
        assert_eq!(summary["completed"], 123);
        assert_eq!(summary["total_score"], 7_626_000);
        let items = summary["playlist_item_attempts"].as_array().unwrap();
        assert_eq!(items.len(), 123);
        assert_eq!(items[0], json!({"id":1,"attempts":1,"passed":true}));
        assert_eq!(items[122], json!({"id":123,"attempts":1,"passed":true}));
        let stats = room_results::playlist_stats(&f.state, &viewer, room)
            .await
            .unwrap();
        assert_eq!(stats.as_array().unwrap().len(), 123);
        for (index, item) in stats.as_array().unwrap().iter().enumerate() {
            assert_eq!(item["playlistItemID"], index + 1);
            assert_eq!(item["cumulativeScore"], (index + 1) * 1000);
            assert_eq!(
                item["totalScoreDistribution"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_i64().unwrap())
                    .sum::<i64>(),
                1
            );
        }
        // C joined before these rounds, but never shared their frozen roster.
        let stranger = common::account(&f.state, f.users[2]).await.unwrap();
        assert!(
            room_results::room_summary(&f.state, &stranger, room)
                .await
                .unwrap()
                .is_null()
        );
        let hidden = room_results::playlist_stats(&f.state, &stranger, room)
            .await
            .unwrap();
        assert_eq!(hidden.as_array().unwrap().len(), 123);
        assert!(
            hidden
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["cumulativeScore"] == 0 && item["lastProcessedScoreID"] == 0)
        );
        let first_score: i64 = sqlx::query_scalar("SELECT score_id FROM lazer_score_tokens WHERE room_id=$1 AND playlist_item_id=1 AND user_id=$2").bind(room).bind(f.users[0]).fetch_one(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE scores SET score=$2 WHERE id=$1").bind(first_score).bind(i64::MAX).execute(&f.state.pool).await.unwrap();
        let overflow = room_results::room_summary(&f.state, &viewer, room).await.unwrap_err();
        assert_eq!(overflow.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(overflow.1, "room_score_overflow");
        let overflow_round = f.round(room, 1, &[0, 1]).await;
        let (overflow_token, _) = f.seed_processed((room, 1, overflow_round), 0, 1, true, 1.0).await;
        f.finish(overflow_round).await;
        let overflow = room_results::playlist_stats(&f.state, &viewer, room).await.unwrap_err();
        assert_eq!(overflow.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(overflow.1, "room_score_overflow");
        sqlx::query("UPDATE scores SET score=1000 WHERE id=$1").bind(first_score).execute(&f.state.pool).await.unwrap();
        sqlx::query("UPDATE lazer_score_tokens SET revoked_at=now() WHERE id=$1").bind(overflow_token).execute(&f.state.pool).await.unwrap();
        sqlx::query("DELETE FROM lazer_room_members")
            .execute(&f.state.pool)
            .await
            .unwrap();
        let other_room = f.room(&[0, 1]).await;
        let round = f.round(other_room, 1, &[0, 1]).await;
        let (_, foreign) = f
            .seed_processed((other_room, 1, round), 0, 9_000_000, true, 1.0)
            .await;
        f.finish(round).await;
        f.get(
            0,
            &format!("/api/v2/rooms/{room}/playlist/1/scores/{foreign}"),
            404,
        )
        .await;
        assert_eq!(
            room_results::event_rank(&f.state, &viewer, room, 1, foreign)
                .await
                .unwrap(),
            None
        );
        // Leaving the room does not erase or widen its immutable history.
        assert_eq!(
            room_results::room_summary(&f.state, &viewer, room)
                .await
                .unwrap()["total_score"],
            7_626_000
        );
    })
    .catch_unwind()
    .await;
    f.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
