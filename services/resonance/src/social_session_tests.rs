//! Real Bancho requests against disposable PostgreSQL schemas and loopback HTTP.
use crate::{common, stable};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{Router, routing::post};
use futures_util::FutureExt;
use resonance_protocol::stable as wire;
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{collections::HashSet, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

struct Database {
    control: PgPool,
    pool: PgPool,
    schema: String,
}

impl Database {
    async fn new(connections: u32) -> Self {
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
        let schema = format!("social_session_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&control)
            .await
            .unwrap();
        let options = PgConnectOptions::from_str(&url)
            .unwrap()
            .application_name(&schema)
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
        Self {
            control,
            pool,
            schema,
        }
    }

    async fn server(&self) -> Server {
        let state = common::AppState {
            pool: self.pool.clone(),
            storage: Arc::new(std::env::temp_dir()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(crate::signalr::Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let router = Router::new()
            .route("/", post(stable::bancho))
            .merge(stable::router())
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        Server {
            url,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            stop,
            task,
        }
    }

    // The barrier observes PostgreSQL's real row-lock wait, rather than hoping
    // a request has reached the critical section after an arbitrary delay.
    async fn wait_for_user_locks(&self, count: i64) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE '%FROM users%FOR UPDATE%'")
                    .bind(&self.schema).fetch_one(&self.control).await.unwrap();
                if waiting >= count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Bancho requests must reach their user-row lock");
    }

    async fn cleanup(self) {
        self.pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.control)
            .await
            .unwrap();
        self.control.close().await;
    }
}

struct Server {
    url: String,
    http: reqwest::Client,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Server {
    async fn login(&self, name: &str, password: &str) -> String {
        self.inspect_login(name, password).await.token
    }

    async fn inspect_login(&self, name: &str, password: &str) -> InitialLogin {
        let response = self
            .http
            .post(&self.url)
            .body(format!(
                "{name}\n{:x}\nb20261007|8|0|fixture:hashes:|1\n",
                md5::compute(password)
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["cho-protocol"], "19");
        assert_eq!(
            response.headers()["content-type"],
            "application/octet-stream"
        );
        let token = response.headers()["cho-token"].to_str().unwrap().to_owned();
        assert_ne!(token, "no", "real stable login must succeed");
        assert!(!token.is_empty() && token.len() <= 128);
        let bytes = response.bytes().await.unwrap();
        let mut initial = inspect_initial_login(&bytes, name);
        initial.token = token;
        initial
    }

    async fn poll(&self, token: &str, body: Vec<u8>) -> reqwest::Response {
        poll(&self.http, &self.url, token, body).await
    }

    fn spawn_poll(&self, token: String, body: Vec<u8>) -> JoinHandle<reqwest::Response> {
        let http = self.http.clone();
        let url = self.url.clone();
        tokio::spawn(async move { poll(&http, &url, &token, body).await })
    }

    async fn cleanup(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("ephemeral HTTP server must shut down")
            .unwrap();
    }
}

struct InitialLogin {
    token: String,
    user_id: i32,
    presences: Vec<wire::Presence>,
    stats: wire::UserStats,
    friends: Vec<i32>,
}

/// This test reader and packet scanner deliberately do not use the production
/// decoders. Packet lengths, primitive widths and complete field consumption
/// are checked independently against the public initial-login wire contract.
struct InitialReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> InitialReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn take(&mut self, length: usize) -> &'a [u8] {
        let end = self
            .position
            .checked_add(length)
            .expect("initial field size overflow");
        let bytes = self
            .bytes
            .get(self.position..end)
            .expect("complete initial packet field");
        self.position = end;
        bytes
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take(2).try_into().unwrap())
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take(8).try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn string(&mut self) -> String {
        match self.u8() {
            0 => return String::new(),
            11 => {}
            _ => panic!("initial string marker must be empty or UTF-8"),
        }
        let mut length = 0u32;
        for shift in [0, 7, 14, 21, 28] {
            let part = self.u8();
            if shift == 28 {
                assert!(part <= 15, "initial string length fits u32");
            }
            length |= u32::from(part & 127) << shift;
            if part < 128 {
                assert!(length <= 16_384, "bounded initial string");
                return std::str::from_utf8(self.take(length as usize))
                    .expect("initial UTF-8")
                    .to_owned();
            }
        }
        panic!("initial string length must terminate")
    }
    fn finish(self) {
        assert_eq!(
            self.position,
            self.bytes.len(),
            "initial packet has no unread/trailing fields"
        );
    }
}

fn initial_presence(payload: &[u8]) -> wire::Presence {
    let mut reader = InitialReader::new(payload);
    let user_id = reader.i32();
    let username = reader.string();
    let utc = reader.u8();
    let country_code = reader.u8();
    let privileges_and_mode = reader.u8();
    let longitude = reader.f32();
    let latitude = reader.f32();
    let rank = reader.i32();
    reader.finish();
    assert!(user_id > 0 && !username.is_empty());
    assert!(utc <= 48, "encoded UTC offset remains in -24..24");
    assert!(
        (1..=252).contains(&country_code),
        "country is a legacy client table entry"
    );
    let mode = privileges_and_mode >> 5;
    assert!(mode <= 3);
    assert_eq!(privileges_and_mode & 31, 1);
    assert!(longitude.is_finite() && longitude.abs() <= 180.0);
    assert!(latitude.is_finite() && latitude.abs() <= 90.0);
    assert!(rank >= 0);
    wire::Presence {
        user_id,
        username,
        utc_offset: (i16::from(utc) - 24) as i8,
        country_code,
        client_privileges: privileges_and_mode & 31,
        mode,
        longitude,
        latitude,
        rank,
    }
}

fn inspect_initial_login(bytes: &[u8], expected_name: &str) -> InitialLogin {
    assert!(
        bytes.len() <= 4_194_304,
        "bounded complete initial response"
    );
    let mut packets = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let header = bytes
            .get(offset..offset + 7)
            .expect("complete initial packet header");
        let id = u16::from_le_bytes(header[..2].try_into().unwrap());
        assert_eq!(header[2], 0, "initial packet reserved byte");
        let length = u32::from_le_bytes(header[3..].try_into().unwrap()) as usize;
        assert!(length <= 1_048_576);
        offset += 7;
        let end = offset
            .checked_add(length)
            .expect("initial packet length overflow");
        packets.push((
            id,
            bytes
                .get(offset..end)
                .expect("complete initial packet body"),
        ));
        offset = end;
    }
    assert_eq!(offset, bytes.len());
    assert!(packets.len() <= 4096 && packets.len() >= 11);
    let single_integer = |index: usize, id: u16| {
        assert_eq!(packets[index].0, id);
        let mut reader = InitialReader::new(packets[index].1);
        let value = reader.i32();
        reader.finish();
        value
    };
    let user_id = single_integer(0, 5);
    assert!(user_id > 0, "successful initial user identity");
    assert_eq!(single_integer(1, 75), 19);
    assert_eq!(single_integer(2, 71), 5);
    assert_eq!(single_integer(3, 92), 0);
    assert_eq!(packets[4].0, 72);
    let mut reader = InitialReader::new(packets[4].1);
    let count = reader.u16();
    assert!(count <= 4096);
    let friends: Vec<_> = (0..count).map(|_| reader.i32()).collect();
    reader.finish();
    assert!(friends.iter().all(|id| *id > 0 && *id != user_id));
    assert_eq!(
        friends.iter().copied().collect::<HashSet<_>>().len(),
        friends.len()
    );

    let mut index = 5;
    let mut channel_names = HashSet::new();
    while packets[index].0 == 65 {
        let channel = |packet: (u16, &[u8])| {
            let mut reader = InitialReader::new(packet.1);
            let name = reader.string();
            let description = reader.string();
            let members = reader.u16();
            reader.finish();
            assert!(name.starts_with('#') && name.len() > 1);
            (name, description, members)
        };
        let info = channel(packets[index]);
        assert!(
            channel_names.insert(info.0.clone()),
            "each initial channel appears once"
        );
        index += 1;
        assert_eq!(
            packets[index].0, 67,
            "each advertised channel has its complete autojoin"
        );
        assert_eq!(channel(packets[index]), info);
        index += 1;
    }
    assert!(
        !channel_names.is_empty(),
        "initial channel directory is present"
    );
    assert_eq!(packets[index].0, 89);
    InitialReader::new(packets[index].1).finish();
    index += 1;
    assert_eq!(
        packets[index].0, 83,
        "self presence follows channel-directory end"
    );
    let self_presence = initial_presence(packets[index].1);
    assert_eq!(self_presence.user_id, user_id);
    assert_eq!(self_presence.username, expected_name);
    assert_eq!(self_presence.utc_offset, 8);
    assert_eq!(self_presence.mode, 0);
    assert_eq!(
        (self_presence.longitude, self_presence.latitude),
        (0.0, 0.0)
    );
    index += 1;
    assert_eq!(packets[index].0, 11, "self stats follows self presence");
    let mut reader = InitialReader::new(packets[index].1);
    let stats = wire::UserStats {
        user_id: reader.i32(),
        action: wire::UserAction {
            action: reader.u8(),
            info_text: reader.string(),
            beatmap_md5: reader.string(),
            mods: reader.u32(),
            mode: reader.u8(),
            beatmap_id: reader.i32(),
        },
        ranked_score: reader.i64(),
        accuracy: reader.f32(),
        play_count: reader.i32(),
        total_score: reader.i64(),
        rank: reader.i32(),
        pp: reader.u16(),
    };
    reader.finish();
    assert_eq!(stats.user_id, user_id);
    assert_eq!(stats.action, wire::UserAction::default());
    assert!(
        stats.ranked_score >= 0
            && stats.play_count >= 0
            && stats.total_score >= 0
            && stats.rank >= 0
    );
    assert!(stats.accuracy.is_finite() && (0.0..=1.0).contains(&stats.accuracy));
    index += 1;
    let mut presences = vec![self_presence];
    let mut present_ids = HashSet::from([user_id]);
    while packets[index].0 == 83 {
        let presence = initial_presence(packets[index].1);
        assert!(
            present_ids.insert(presence.user_id),
            "initial peer presence identity is unique"
        );
        presences.push(presence);
        index += 1;
    }
    assert_eq!(
        packets[index].0, 24,
        "welcome notification terminates the initial response"
    );
    let mut reader = InitialReader::new(packets[index].1);
    assert!(!reader.string().is_empty());
    reader.finish();
    assert_eq!(index + 1, packets.len(), "entire initial response consumed");
    InitialLogin {
        token: String::new(),
        user_id,
        presences,
        stats,
        friends,
    }
}

async fn poll(http: &reqwest::Client, url: &str, token: &str, body: Vec<u8>) -> reqwest::Response {
    tokio::time::timeout(
        Duration::from_secs(5),
        http.post(url).header("osu-token", token).body(body).send(),
    )
    .await
    .expect("friend packet request must complete without nested-lock deadlock")
    .unwrap()
}

fn hashes(password: &str) -> (String, String) {
    let salt = SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).unwrap();
    let hash = |value: &str| {
        Argon2::default()
            .hash_password(value.as_bytes(), &salt)
            .unwrap()
            .to_string()
    };
    (
        hash(password),
        hash(&format!("{:x}", md5::compute(password))),
    )
}

async fn user(pool: &PgPool, name: &str, password: &str) -> i64 {
    let (plain, digest) = hashes(password);
    let id = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,$3,$4) RETURNING id")
        .bind(name).bind(format!("{name}@invalid.test")).bind(plain).bind(digest)
        .fetch_one(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO user_roles(user_id,role_id) SELECT $1,id FROM roles WHERE name='player'",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn friends(operations: &[(u16, i64)]) -> Vec<u8> {
    operations
        .iter()
        .flat_map(|(packet, target)| {
            wire::int_packet(*packet, i32::try_from(*target).unwrap()).unwrap()
        })
        .collect()
}

async fn friend_lists(response: reqwest::Response) -> Vec<Vec<i32>> {
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.bytes().await.unwrap();
    wire::decode_packets(&body)
        .unwrap()
        .into_iter()
        .filter(|packet| packet.id == wire::server::FRIENDS_LIST)
        .map(|packet| {
            let mut reader = wire::Reader::new(packet.payload);
            let ids = reader.i32_list().unwrap();
            reader.finish().unwrap();
            ids
        })
        .collect()
}

async fn relationship(pool: &PgPool, user: i64, target: i64) -> Option<String> {
    sqlx::query_scalar(
        "SELECT relation_type FROM user_relationships WHERE user_id=$1 AND target_id=$2",
    )
    .bind(user)
    .bind(target)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn queue_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM bancho_packet_queue")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn action_packet(action: &wire::UserAction) -> Vec<u8> {
    let mut writer = wire::Writer::new();
    action.write_to(&mut writer).unwrap();
    wire::encode_packet(wire::client::CHANGE_ACTION, &writer.into_bytes()).unwrap()
}

fn users_packet(packet: u16, ids: &[i64]) -> Vec<u8> {
    let mut writer = wire::Writer::new();
    writer
        .i32_list(
            &ids.iter()
                .map(|id| i32::try_from(*id).unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    wire::encode_packet(packet, &writer.into_bytes()).unwrap()
}

async fn response_packets(response: reqwest::Response) -> Vec<(u16, Vec<u8>)> {
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.bytes().await.unwrap();
    wire::decode_packets(&body)
        .unwrap()
        .into_iter()
        .map(|packet| (packet.id, packet.payload.to_vec()))
        .collect()
}

fn decode_stats(payload: &[u8]) -> wire::UserStats {
    let mut reader = wire::Reader::new(payload);
    let user_id = reader.i32().unwrap();
    let action = wire::UserAction {
        action: reader.u8().unwrap(),
        info_text: reader.string().unwrap().to_owned(),
        beatmap_md5: reader.string().unwrap().to_owned(),
        mods: reader.u32().unwrap(),
        mode: reader.u8().unwrap(),
        beatmap_id: reader.i32().unwrap(),
    };
    let result = wire::UserStats {
        user_id,
        action,
        ranked_score: reader.i64().unwrap(),
        accuracy: reader.f32().unwrap(),
        play_count: reader.i32().unwrap(),
        total_score: reader.i64().unwrap(),
        rank: reader.i32().unwrap(),
        pp: reader.u16().unwrap(),
    };
    reader.finish().unwrap();
    result
}

fn assert_stats(packets: &[(u16, Vec<u8>)], id: i64, mode: u8, mods: u32, total: i64, pp: u16) {
    let stats = packets
        .iter()
        .filter(|(packet, _)| *packet == wire::server::USER_STATS)
        .map(|(_, payload)| decode_stats(payload))
        .find(|stats| i64::from(stats.user_id) == id)
        .expect("requested player stats must be returned");
    assert_eq!(stats.action.mode, mode);
    assert_eq!(stats.action.mods, mods);
    assert_eq!(stats.total_score, total);
    assert_eq!(stats.ranked_score, if mods & 128 == 0 { total } else { 0 });
    assert_eq!(stats.play_count, 1);
    assert_eq!(stats.pp, pp);
    assert!((stats.accuracy - 0.98).abs() < 0.0001);
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_initial_login_packets_are_complete_and_preserve_legacy_country_bytes() {
    let database = Database::new(1).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let password = "Initial-Login-Fixture-Password";
        let seasonal = server.http.get(format!("{}web/osu-getseasonal.php?fixture=ignored", server.url))
            .send().await.unwrap();
        assert_eq!(seasonal.status(), reqwest::StatusCode::OK);
        assert_eq!(seasonal.json::<serde_json::Value>().await.unwrap(), serde_json::json!([]));
        let probe = server.http.get(format!("{}web/bancho_connect.php?fixture=ignored", server.url))
            .send().await.unwrap();
        assert_eq!(probe.status(), reqwest::StatusCode::OK);
        assert!(probe.bytes().await.unwrap().is_empty());
        let taiwan = user(&database.pool, "InitialTaiwan", password).await;
        let unknown = user(&database.pool, "InitialUnknown", password).await;
        // Existing account fixtures inherit the schema's TW default. Only this
        // separately owned synthetic peer gets an unsupported country value.
        let country: String = sqlx::query_scalar("SELECT country FROM users WHERE id=$1")
            .bind(taiwan).fetch_one(&database.pool).await.unwrap();
        assert_eq!(country, "TW");
        sqlx::query("UPDATE users SET country='ZZ' WHERE id=$1")
            .bind(unknown).execute(&database.pool).await.unwrap();
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend')")
            .bind(taiwan).bind(unknown).execute(&database.pool).await.unwrap();

        let first = server.inspect_login("InitialTaiwan", password).await;
        assert_eq!(i64::from(first.user_id), taiwan);
        assert_eq!(first.presences.len(), 1);
        assert_eq!(first.presences[0].country_code, 220);
        assert_eq!(first.friends, vec![i32::try_from(unknown).unwrap()]);
        assert_eq!((first.stats.ranked_score, first.stats.total_score, first.stats.play_count, first.stats.pp), (0, 0, 0, 0));
        assert_eq!(first.stats.accuracy, 0.0);

        for _ in 0..3 {
            let idle = server.poll(&first.token, vec![4, 0, 0, 0, 0, 0, 0]).await;
            assert_eq!(idle.status(), reqwest::StatusCode::OK);
            assert!(idle.bytes().await.unwrap().is_empty(), "idle heartbeat must not generate unsolicited packets");
        }
        let queued = [24, 0, 0, 8, 0, 0, 0, 11, 6, b'q', b'u', b'e', b'u', b'e', b'd'];
        sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) VALUES($1,$2)")
            .bind(taiwan).bind(queued.as_slice()).execute(&database.pool).await.unwrap();
        let delivery = server.poll(&first.token, vec![4, 0, 0, 0, 0, 0, 0]).await;
        assert_eq!(delivery.status(), reqwest::StatusCode::OK);
        assert_eq!(delivery.bytes().await.unwrap().as_ref(), queued);
        assert!(server.poll(&first.token, vec![4, 0, 0, 0, 0, 0, 0]).await.bytes().await.unwrap().is_empty());
        assert_eq!(server.poll(&first.token, vec![4, 0, 0, 1, 0, 0, 0, 42]).await.status(), reqwest::StatusCode::BAD_REQUEST);

        let second = server.inspect_login("InitialUnknown", password).await;
        assert_eq!(i64::from(second.user_id), unknown);
        assert!(second.friends.is_empty(), "friendship does not become reciprocal at login");
        assert_eq!(second.presences.len(), 2);
        for (id, expected_country) in [(taiwan, 220), (unknown, 244)] {
            let presence = second.presences.iter().find(|presence| i64::from(presence.user_id) == id)
                .expect("both active synthetic accounts have complete initial presences");
            assert_eq!(presence.country_code, expected_country);
        }
        assert_eq!(second.stats.total_score, 0);
        // The explicit unknown region and an unsupported value share the same
        // wire assignment. Relogin validates the whole response again.
        sqlx::query("UPDATE users SET country='XX' WHERE id=$1")
            .bind(unknown).execute(&database.pool).await.unwrap();
        let relogin = server.inspect_login("InitialUnknown", password).await;
        assert_eq!(relogin.presences[0].country_code, 244);
        assert_eq!(relogin.presences.len(), 2);
    }).catch_unwind().await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_action_status_and_presence_reuse_one_connection_and_preserve_rx_partitions() {
    let database = Database::new(1).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let password = "Status-Social-Password";
        let actor = user(&database.pool, "StatusActor", password).await;
        let target = user(&database.pool, "StatusTarget", password).await;
        for (id, mode, mods, score, pp) in [
            (actor, 0_i16, 0_i64, 1234_i64, 11.0_f64),
            (actor, 0, 128, 4321, 99.0),
            (target, 2, 128, 6789, 65.5),
        ] {
            let map: i64 = sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,status,stars,md5,osu_path) VALUES('Status fixture','Illyaverse','Fixture','Partition',$1,'ranked',1,$2,'fixture.osu') RETURNING id")
                .bind(mode).bind(format!("status-{id}-{mode}-{mods}")).fetch_one(&database.pool).await.unwrap();
            let vanilla = mods & 128 == 0;
            sqlx::query("INSERT INTO scores(user_id,beatmap_id,mode,mods,score,pp,accuracy,max_combo,client,verification,passed,pp_eligible,score_variant,standardised_score,classic_score,score_scale_status) VALUES($1,$2,$3,$4,$5,$6,98,2,'stable','verified',true,true,$7,$8,$9,$10)")
                .bind(id).bind(map).bind(mode).bind(mods).bind(score).bind(pp)
                .bind(if vanilla { "vanilla" } else { "relax" })
                .bind(vanilla.then_some(score * 100)).bind(vanilla.then_some(score))
                .bind(if vanilla { "legacy_estimated" } else { "unavailable" })
                .execute(&database.pool).await.unwrap();
        }
        let token = server.login("StatusActor", password).await;
        let target_token = server.login("StatusTarget", password).await;
        for (mods, total, pp) in [(0_u32, 1234_i64, 11_u16), (128, 4321, 99)] {
            let action = wire::UserAction {
                action: 2,
                info_text: "Status fixture".into(),
                mods,
                ..Default::default()
            };
            let packets = response_packets(server.poll(&token, action_packet(&action)).await).await;
            assert_stats(&packets, actor, 0, mods, total, pp);
            let packets = response_packets(server.poll(&token, wire::encode_packet(wire::client::REQUEST_STATUS_UPDATE, &[]).unwrap()).await).await;
            assert_stats(&packets, actor, 0, mods, total, pp);
            let stored: serde_json::Value = sqlx::query_scalar("SELECT action FROM bancho_sessions WHERE user_id=$1")
                .bind(actor).fetch_one(&database.pool).await.unwrap();
            assert_eq!(stored["mods"], mods);
        }
        let target_action = wire::UserAction { action: 2, mode: 2, mods: 128, ..Default::default() };
        let packets = response_packets(server.poll(&target_token, action_packet(&target_action)).await).await;
        assert_stats(&packets, target, 2, 128, 6789, 66);

        let packets = response_packets(server.poll(&token, users_packet(wire::client::USER_STATS_REQUEST, &[actor, target])).await).await;
        assert_eq!(packets.iter().filter(|(id, _)| *id == wire::server::USER_STATS).count(), 2);
        assert_stats(&packets, actor, 0, 128, 4321, 99);
        assert_stats(&packets, target, 2, 128, 6789, 66);

        let packets = response_packets(server.poll(&token, users_packet(wire::client::USER_PRESENCE_REQUEST, &[actor, target])).await).await;
        let mut present = Vec::new();
        for (id, payload) in packets {
            if id != wire::server::USER_PRESENCE {
                continue;
            }
            let mut reader = wire::Reader::new(&payload);
            let id = i64::from(reader.i32().unwrap());
            let name = reader.string().unwrap().to_owned();
            assert_eq!(reader.u8().unwrap(), 32); // Login UTC+8 plus the wire offset.
            reader.u8().unwrap();
            let mode = reader.u8().unwrap() >> 5;
            reader.f32().unwrap();
            reader.f32().unwrap();
            reader.i32().unwrap();
            reader.finish().unwrap();
            present.push((id, name, mode));
        }
        assert_eq!(present, vec![(actor, "StatusActor".into(), 0), (target, "StatusTarget".into(), 2)]);

        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(target).execute(&database.pool).await.unwrap();
        let packets = response_packets(server.poll(&token, users_packet(wire::client::USER_PRESENCE_REQUEST, &[actor, target])).await).await;
        let present: Vec<_> = packets.iter().filter(|(id, _)| *id == wire::server::USER_PRESENCE).map(|(_, payload)| {
            wire::Reader::new(payload).i32().unwrap()
        }).collect();
        assert_eq!(present, vec![i32::try_from(actor).unwrap()]);
    })
    .catch_unwind()
    .await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_spectator_and_room_invites_reuse_one_connection_and_keep_boundaries() {
    let database = Database::new(1).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let password = "Boundary-Social-Password";
        let actor = user(&database.pool, "BoundaryActor", password).await;
        let target = user(&database.pool, "BoundaryTarget", password).await;
        let token = server.login("BoundaryActor", password).await;
        let target_token = server.login("BoundaryTarget", password).await;

        let packets = response_packets(
            server
                .poll(
                    &target_token,
                    wire::int_packet(
                        wire::client::START_SPECTATING,
                        i32::try_from(actor).unwrap(),
                    )
                    .unwrap(),
                )
                .await,
        )
        .await;
        assert!(
            packets
                .iter()
                .any(|(id, _)| *id == wire::server::CHANNEL_JOIN_SUCCESS)
        );
        let host: Option<i64> =
            sqlx::query_scalar("SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1")
                .bind(target)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(host, Some(actor));
        let packets = response_packets(server.poll(&token, vec![]).await).await;
        assert!(packets.iter().any(|(id, payload)| *id == 13
            && wire::Reader::new(payload).i32().unwrap() == i32::try_from(target).unwrap()));
        response_packets(
            server
                .poll(
                    &target_token,
                    wire::encode_packet(wire::client::STOP_SPECTATING, &[]).unwrap(),
                )
                .await,
        )
        .await;
        let host: Option<i64> =
            sqlx::query_scalar("SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1")
                .bind(target)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(host, None);

        sqlx::query(
            "INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block')",
        )
        .bind(target)
        .bind(actor)
        .execute(&database.pool)
        .await
        .unwrap();
        let response = server
            .poll(
                &target_token,
                wire::int_packet(
                    wire::client::START_SPECTATING,
                    i32::try_from(actor).unwrap(),
                )
                .unwrap(),
            )
            .await;
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        let host: Option<i64> =
            sqlx::query_scalar("SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1")
                .bind(target)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(host, None);
        sqlx::query("DELETE FROM user_relationships WHERE user_id=$1 AND target_id=$2")
            .bind(target)
            .bind(actor)
            .execute(&database.pool)
            .await
            .unwrap();

        let room = resonance_protocol::multiplayer::Match {
            id: 0,
            in_progress: false,
            mods: 0,
            name: "Connection boundary fixture".into(),
            password: String::new(),
            map_name: String::new(),
            map_id: 0,
            map_md5: String::new(),
            statuses: [resonance_protocol::multiplayer::OPEN; 16],
            teams: [0; 16],
            users: [None; 16],
            host_id: i32::try_from(actor).unwrap(),
            mode: 0,
            win_condition: 0,
            team_type: 0,
            freemods: false,
            slot_mods: [0; 16],
            seed: 0,
        };
        let packets = response_packets(
            server
                .poll(
                    &token,
                    wire::encode_packet(wire::client::CREATE_MATCH, &room.encode(false).unwrap())
                        .unwrap(),
                )
                .await,
        )
        .await;
        assert!(packets.iter().any(|(id, _)| *id == 36)); // MATCH_JOIN_SUCCESS.
        let room_id: i16 =
            sqlx::query_scalar("SELECT room_id FROM stable_room_members WHERE user_id=$1")
                .bind(actor)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        // Packet 87 is MATCH_INVITE; callers must already belong to a room.
        let response = server
            .poll(
                &target_token,
                wire::int_packet(87, i32::try_from(actor).unwrap()).unwrap(),
            )
            .await;
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"],
            "room_membership_required"
        );
        response_packets(
            server
                .poll(
                    &token,
                    wire::int_packet(87, i32::try_from(target).unwrap()).unwrap(),
                )
                .await,
        )
        .await;
        let packets = response_packets(server.poll(&target_token, vec![]).await).await;
        let invite = packets
            .iter()
            .find(|(id, _)| *id == 88)
            .expect("MATCH_INVITE must reach the target");
        let invite = wire::ChatMessage::decode(&invite.1).unwrap();
        assert_eq!(invite.sender_id, i32::try_from(actor).unwrap());
        assert_eq!(invite.recipient, "BoundaryTarget");
        assert!(invite.text.contains(&format!("osump://{room_id}/")));

        sqlx::query(
            "INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block')",
        )
        .bind(target)
        .bind(actor)
        .execute(&database.pool)
        .await
        .unwrap();
        let queued_before = queue_count(&database.pool).await;
        let response = server
            .poll(
                &token,
                wire::int_packet(87, i32::try_from(target).unwrap()).unwrap(),
            )
            .await;
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"],
            "relationship_blocked"
        );
        assert_eq!(queue_count(&database.pool).await, queued_before);
    })
    .catch_unwind()
    .await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_friend_packets_are_directed_atomic_and_complete_with_one_connection() {
    let database = Database::new(1).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let password = "Social-Fixture-Password";
        let actor = user(&database.pool, "SocialActor", password).await;
        let target = user(&database.pool, "SocialTarget", password).await;
        let token = server.login("SocialActor", password).await;
        let target_token = server.login("SocialTarget", password).await;
        let adds = friends(&[(wire::client::FRIEND_ADD, target); 2]);
        assert_eq!(
            friend_lists(server.poll(&token, adds).await).await,
            vec![vec![target as i32], vec![target as i32]]
        );
        assert_eq!(relationship(&database.pool, actor, target).await.as_deref(), Some("friend"));
        assert_eq!(relationship(&database.pool, target, actor).await, None);
        assert_eq!(friend_lists(server.poll(&target_token, vec![]).await).await, vec![Vec::<i32>::new(), Vec::new()]);

        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend')")
            .bind(target).bind(actor).execute(&database.pool).await.unwrap();
        assert_eq!(
            friend_lists(server.poll(&token, friends(&[(wire::client::FRIEND_REMOVE, target); 2])).await).await,
            vec![Vec::<i32>::new(), Vec::new()]
        );
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(relationship(&database.pool, target, actor).await.as_deref(), Some("friend"));
        assert_eq!(friend_lists(server.poll(&target_token, vec![]).await).await, vec![vec![actor as i32], vec![actor as i32]]);

        // A later invalid packet rolls the earlier mutation and its queue writes
        // back together. The reciprocal friendship remains untouched.
        let response = server.poll(&token, friends(&[(wire::client::FRIEND_ADD, target), (wire::client::FRIEND_ADD, actor)])).await;
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);

        let mut malformed = i32::try_from(target).unwrap().to_le_bytes().to_vec();
        malformed.push(0);
        let response = server.poll(&token, wire::encode_packet(wire::client::FRIEND_ADD, &malformed).unwrap()).await;
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);
        let response = server.poll(&token, friends(&[(wire::client::FRIEND_ADD, target); 61])).await;
        assert_eq!(response.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);

        sqlx::query("UPDATE user_relationships SET relation_type='block' WHERE user_id=$1 AND target_id=$2")
            .bind(target).bind(actor).execute(&database.pool).await.unwrap();
        let response = server.poll(&token, friends(&[(wire::client::FRIEND_ADD, target)])).await;
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(relationship(&database.pool, target, actor).await.as_deref(), Some("block"));
        assert_eq!(queue_count(&database.pool).await, 0);

        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1")
            .bind(target).execute(&database.pool).await.unwrap();
        let response = server.poll(&token, friends(&[(wire::client::FRIEND_REMOVE, target)])).await;
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        assert_eq!(queue_count(&database.pool).await, 0);
    })
    .catch_unwind()
    .await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn stable_friend_packets_recheck_reset_and_permission_revocation_after_user_lock() {
    let database = Database::new(4).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let old = "Social-Reset-Old-Password";
        let new = "Social-Reset-New-Password";
        let actor = user(&database.pool, "ResetSocialActor", old).await;
        let target = user(&database.pool, "ResetSocialTarget", old).await;
        let token = server.login("ResetSocialActor", old).await;
        let (plain, digest) = hashes(new);
        let mut reset = database.pool.begin().await.unwrap();
        sqlx::query("UPDATE users SET password_hash=$2,password_md5_hash=$3 WHERE id=$1")
            .bind(actor)
            .bind(plain)
            .bind(digest)
            .execute(&mut *reset)
            .await
            .unwrap();
        let task = server.spawn_poll(token, friends(&[(wire::client::FRIEND_ADD, target)]));
        database.wait_for_user_locks(1).await;
        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1")
                .bind(actor)
                .execute(&mut *reset),
        )
        .await
        .expect("reset must delete the session while friend poll waits on the user")
        .unwrap();
        reset.commit().await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.bytes().await.unwrap();
        assert!(
            wire::decode_packets(&body)
                .unwrap()
                .iter()
                .any(|packet| packet.id == wire::server::RESTART)
        );
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);
        let sessions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM bancho_sessions WHERE user_id=$1")
                .bind(actor)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(sessions, 0);

        let token = server.login("ResetSocialActor", new).await;
        // The statement snapshot does not lock a target created after its
        // initial scan. Such a target must wait for a subsequent poll rather
        // than acquiring a new user/FK lock while holding the session lock.
        let late_target: i64 = sqlx::query_scalar("SELECT nextval('users_id_seq')")
            .fetch_one(&database.pool).await.unwrap();
        let mut barrier = database.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(actor).fetch_one(&mut *barrier).await.unwrap();
        let task = server.spawn_poll(token.clone(), friends(&[(wire::client::FRIEND_ADD, late_target)]));
        database.wait_for_user_locks(1).await;
        sqlx::query("INSERT INTO users(id,username,email,password_hash,password_md5_hash) VALUES($1,'LateSocialTarget','late-social@invalid.test','unused','unused')")
            .bind(late_target).execute(&database.pool).await.unwrap();
        barrier.commit().await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        assert_eq!(relationship(&database.pool, actor, late_target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);

        let mut revocation = database.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(actor)
            .fetch_one(&mut *revocation)
            .await
            .unwrap();
        sqlx::query("DELETE FROM user_roles WHERE user_id=$1")
            .bind(actor)
            .execute(&mut *revocation)
            .await
            .unwrap();
        let task = server.spawn_poll(token, friends(&[(wire::client::FRIEND_ADD, target)]));
        database.wait_for_user_locks(1).await;
        revocation.commit().await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"],
            "social_permission_required"
        );
        assert_eq!(relationship(&database.pool, actor, target).await, None);
        assert_eq!(queue_count(&database.pool).await, 0);
    })
    .catch_unwind()
    .await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL; creates and removes an isolated schema"]
async fn opposite_stable_friend_batches_use_the_same_user_lock_order() {
    let database = Database::new(4).await;
    let server = database.server().await;
    let result = std::panic::AssertUnwindSafe(async {
        let password = "Opposite-Social-Password";
        let actor = user(&database.pool, "OppositeActor", password).await;
        let target = user(&database.pool, "OppositeTarget", password).await;
        let token = server.login("OppositeActor", password).await;
        let target_token = server.login("OppositeTarget", password).await;
        let mut barrier = database.pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(actor)
            .fetch_one(&mut *barrier)
            .await
            .unwrap();
        let first = server.spawn_poll(token, friends(&[(wire::client::FRIEND_ADD, target); 2]));
        let second = server.spawn_poll(
            target_token,
            friends(&[(wire::client::FRIEND_ADD, actor); 2]),
        );
        database.wait_for_user_locks(2).await;
        barrier.commit().await.unwrap();
        for task in [first, second] {
            let response = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            response.bytes().await.unwrap();
        }
        assert_eq!(
            relationship(&database.pool, actor, target).await.as_deref(),
            Some("friend")
        );
        assert_eq!(
            relationship(&database.pool, target, actor).await.as_deref(),
            Some("friend")
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_relationships")
            .fetch_one(&database.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    })
    .catch_unwind()
    .await;
    server.cleanup().await;
    database.cleanup().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
