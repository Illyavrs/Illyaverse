//! Real HTTP/WebSocket checks use an isolated schema in disposable PostgreSQL 17.
use crate::{AppState, mail_delivery, maps, router};
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use uuid::Uuid;

async fn user(db: &PgPool, name: &str, role: i64) -> i64 {
    let id = sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture','fixture') RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).fetch_one(db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
        .bind(id)
        .bind(role)
        .execute(db)
        .await
        .unwrap();
    id
}

async fn session(state: &AppState, id: i64) -> (Uuid, String) {
    let sid = Uuid::new_v4();
    let expires = chrono::Utc::now() + chrono::Duration::hours(1);
    sqlx::query("INSERT INTO sessions(id,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(sid)
        .bind(id)
        .bind(expires)
        .execute(&state.db)
        .await
        .unwrap();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &json!({"sub":id,"sid":sid,"exp":expires.timestamp()}),
        &jsonwebtoken::EncodingKey::from_secret(state.secret.as_bytes()),
    )
    .unwrap();
    (sid, format!("illya_session={token}"))
}

async fn request(
    client: &reqwest::Client,
    base: &str,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = client
        .request(method, format!("{base}{path}"))
        .header("Origin", "http://localhost:3000");
    if let Some(cookie) = cookie {
        request = request.header("Cookie", cookie);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}

fn ids(value: &Value) -> Vec<i64> {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["id"].as_i64().unwrap())
        .collect()
}

async fn publish(db: &PgPool, channel: i64, entries: &[(i64, &str)]) -> Vec<i64> {
    // Commit each batch together so a polling tick observes all its fixtures.
    let mut tx = db.begin().await.unwrap();
    let mut ids = Vec::new();
    for (sender, content) in entries {
        ids.push(sqlx::query_scalar("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,$3) RETURNING id")
            .bind(channel).bind(sender).bind(content).fetch_one(&mut *tx).await.unwrap());
    }
    tx.commit().await.unwrap();
    ids
}

struct Socket(BufReader<TcpStream>);

async fn upgrade(address: SocketAddr, path: &str, cookie: Option<&str>) -> (u16, Socket) {
    let mut socket = BufReader::new(TcpStream::connect(address).await.unwrap());
    let cookie = cookie
        .map(|value| format!("Cookie: {value}\r\n"))
        .unwrap_or_default();
    socket.get_mut().write_all(format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://localhost:3000\r\n{cookie}Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    ).as_bytes()).await.unwrap();
    let mut line = String::new();
    socket.read_line(&mut line).await.unwrap();
    let status = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    loop {
        line.clear();
        assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
    }
    (status, Socket(socket))
}

impl Socket {
    async fn frame(&mut self) -> (u8, Vec<u8>) {
        tokio::time::timeout(Duration::from_secs(6), async {
            let mut header = [0; 2];
            self.0.read_exact(&mut header).await.unwrap();
            assert_ne!(
                header[0] & 0x80,
                0,
                "fixture expects complete server frames"
            );
            assert_eq!(header[1] & 0x80, 0, "server frames must be unmasked");
            let length = match header[1] & 0x7f {
                126 => self.0.read_u16().await.unwrap() as u64,
                127 => self.0.read_u64().await.unwrap(),
                length => u64::from(length),
            };
            assert!(length <= 1024 * 1024);
            let mut body = vec![0; length as usize];
            self.0.read_exact(&mut body).await.unwrap();
            (header[0] & 0x0f, body)
        })
        .await
        .expect("website WebSocket must produce its expected batch or close frame")
    }

    async fn ready(&mut self) {
        // The Pong proves connection() initialized its cursor before publication.
        // This client Ping is masked, as required for WebSocket client frames.
        self.0
            .get_mut()
            .write_all(b"\x89\x85\0\0\0\0ready")
            .await
            .unwrap();
        let (opcode, body) = self.frame().await;
        assert_eq!(opcode, 10);
        assert_eq!(body, b"ready");
    }

    async fn messages(&mut self) -> Value {
        let (opcode, body) = self.frame().await;
        assert_eq!(opcode, 1);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["type"], "messages");
        value
    }

    async fn messages_count(&mut self, count: usize) -> Value {
        let mut items = Vec::new();
        while items.len() < count {
            let value = self.messages().await;
            items.extend(value["items"].as_array().unwrap().iter().cloned());
        }
        json!({"items":items})
    }

    async fn closed(&mut self) {
        assert_eq!(
            self.frame().await.0,
            8,
            "current authorization must close the stream"
        );
    }
}

async fn policy(
    db: &PgPool,
    viewer: i64,
    restricted: i64,
    outgoing: i64,
    incoming: i64,
    hide: bool,
) {
    let mut tx = db.begin().await.unwrap();
    sqlx::query("UPDATE users SET is_restricted=$2 WHERE id=$1")
        .bind(restricted)
        .bind(hide)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM user_relationships WHERE (user_id=$1 AND target_id=$2) OR (user_id=$3 AND target_id=$1)")
        .bind(viewer).bind(outgoing).bind(incoming).execute(&mut *tx).await.unwrap();
    if hide {
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block'),($3,$1,'block')")
            .bind(viewer).bind(outgoing).bind(incoming).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to disposable PostgreSQL 17"]
async fn public_chat_history_and_stream_apply_current_visibility() {
    let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("Set disposable chat fixture URL");
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::int")
        .fetch_one(&control)
        .await
        .unwrap();
    assert!((170000..180000).contains(&version));
    let schema = format!("illya_chat_visibility_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())])
        .application_name(&schema);
    let db = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let state = AppState {
        db: db.clone(),
        secret: Arc::new("chat-visibility-fixture-secret-at-least-thirty-two-bytes".into()),
        internal_key: Arc::new(String::new()),
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = router(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let pool = db.clone();
    let outcome = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let base = format!("http://{address}");
        let viewer = user(&pool, "ChatViewer", 1).await;
        let ordinary = user(&pool, "ChatOrdinary", 1).await;
        let restricted = user(&pool, "ChatRestricted", 1).await;
        let outgoing = user(&pool, "ChatOutgoingBlock", 1).await;
        let incoming = user(&pool, "ChatIncomingBlock", 1).await;
        let admin = user(&pool, "ChatWildcardAdmin", 3).await;
        let (_, cookie) = session(&state, viewer).await;
        let (_, admin_cookie) = session(&state, admin).await;
        let channel: i64 = sqlx::query_scalar("INSERT INTO channels(name) VALUES('chat-visibility-fixture') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let history_path = format!("/api/v1/channels/{channel}/messages");
        let stream_path = format!("/api/v1/channels/{channel}/stream");
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend'),($2,$1,'friend')")
            .bind(viewer).bind(ordinary).execute(&pool).await.unwrap();
        policy(&pool, viewer, restricted, outgoing, incoming, true).await;
        let history = publish(&pool, channel, &[
            (ordinary, "first ordinary"), (restricted, "restricted history"),
            (outgoing, "viewer blocks sender"), (viewer, "viewer history"),
            (incoming, "sender blocks viewer"), (ordinary, "last ordinary"),
        ]).await;
        let expected = vec![history[0], history[3], history[5]];
        let (code, value) = request(&client, &base, Method::GET, &history_path, Some(&cookie), None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(ids(&value), expected);
        assert_eq!(value["total"], 3, "pagination total must not expose hidden messages");
        assert_eq!(value["items"][0]["username"], "ChatOrdinary");
        let (_, latest) = request(&client, &base, Method::GET, &format!("{history_path}?page_size=2"), Some(&cookie), None).await;
        assert_eq!(ids(&latest), vec![history[3], history[5]]);
        let (_, previous) = request(&client, &base, Method::GET, &format!("{history_path}?page=2&page_size=2"), Some(&cookie), None).await;
        assert_eq!(ids(&previous), vec![history[0]]);
        assert_eq!(previous["page"], 2);
        let (_, bounded) = request(&client, &base, Method::GET, &format!("{history_path}?after_id={}&before_id={}", history[0], history[5]), Some(&cookie), None).await;
        assert_eq!(ids(&bounded), vec![history[3]]);
        assert_eq!(bounded["total"], 3, "cursor bounds retain the full visible total");
        for guest in [None, Some("illya_session=invalid")] {
            let (code, value) = request(&client, &base, Method::GET, &history_path, guest, None).await;
            assert_eq!(code, StatusCode::OK, "public history remains anonymously readable");
            assert_eq!(ids(&value), vec![history[0], history[2], history[3], history[4], history[5]]);
            assert_eq!(value["total"], 5);
            assert_eq!(upgrade(address, &stream_path, guest).await.0, 401);
        }

        let (status, mut socket) = upgrade(address, &stream_path, Some(&cookie)).await;
        assert_eq!(status, 101);
        socket.ready().await;
        let hidden = publish(&pool, channel, &[
            (restricted, "restricted stream"), (outgoing, "outgoing block stream"),
            (incoming, "incoming block stream"), (ordinary, "initial visible marker"),
        ]).await;
        assert_eq!(ids(&socket.messages().await), vec![hidden[3]]);

        // Unrestriction and both unblock directions take effect on this connection.
        policy(&pool, viewer, restricted, outgoing, incoming, false).await;
        let restored = publish(&pool, channel, &[
            (restricted, "restored sender"), (outgoing, "restored outgoing peer"),
            (incoming, "restored incoming peer"), (ordinary, "restored ordinary peer"),
        ]).await;
        assert_eq!(ids(&socket.messages_count(restored.len()).await), restored);
        let (_, restored_history) = request(&client, &base, Method::GET, &history_path, Some(&cookie), None).await;
        assert_eq!(restored_history["total"], 14);
        assert!(ids(&restored_history).contains(&history[1]));
        assert!(ids(&restored_history).contains(&history[2]));
        assert!(ids(&restored_history).contains(&history[4]));

        // Newly applied restrictions/blocks must not use the upgrade-time policy.
        policy(&pool, viewer, restricted, outgoing, incoming, true).await;
        let changed = publish(&pool, channel, &[
            (restricted, "new restriction"), (outgoing, "new outgoing block"),
            (incoming, "new incoming block"), (ordinary, "current visible marker"),
        ]).await;
        assert_eq!(ids(&socket.messages().await), vec![changed[3]]);
        let (_, changed_history) = request(&client, &base, Method::GET, &history_path, Some(&cookie), None).await;
        assert_eq!(changed_history["total"], 6);
        assert!(ids(&changed_history).iter().all(|id| ![history[1], history[2], history[4], hidden[0], hidden[1], hidden[2], restored[0], restored[1], restored[2], changed[0], changed[1], changed[2]].contains(id)));
        drop(socket);

        // Website channel policy remains authoritative, including wildcard admins.
        let permission_role: i64 = sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES('chat_fixture_reader',ARRAY['chat.fixture.read','chat.send']) RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let reader = user(&pool, "ChatPermissionReader", permission_role).await;
        let (_, reader_cookie) = session(&state, reader).await;
        let protected: i64 = sqlx::query_scalar("INSERT INTO channels(name,required_permission) VALUES('chat-protected-fixture','chat.fixture.read') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let protected_history = format!("/api/v1/channels/{protected}/messages");
        let protected_stream = format!("/api/v1/channels/{protected}/stream");
        for (credential, expected) in [(None, StatusCode::UNAUTHORIZED), (Some(cookie.as_str()), StatusCode::FORBIDDEN), (Some(reader_cookie.as_str()), StatusCode::OK), (Some(admin_cookie.as_str()), StatusCode::OK)] {
            assert_eq!(request(&client, &base, Method::GET, &protected_history, credential, None).await.0, expected);
            let (code, connection) = upgrade(address, &protected_stream, credential).await;
            assert_eq!(code, if expected == StatusCode::OK { 101 } else { expected.as_u16() });
            drop(connection);
        }
        for private_policy in ["lazer.room", "stable.room", "chat.pm"] {
            let private: i64 = if private_policy == "chat.pm" {
                sqlx::query_scalar("SELECT illya_pm_channel($1,$2)").bind(viewer).bind(ordinary).fetch_one(&pool).await.unwrap()
            } else {
                sqlx::query_scalar("INSERT INTO channels(name,required_permission) VALUES($1,$2) RETURNING id")
                    .bind(format!("chat-{private_policy}-fixture")).bind(private_policy).fetch_one(&pool).await.unwrap()
            };
            for credential in [None, Some(cookie.as_str()), Some(admin_cookie.as_str())] {
                let (_, channels) = request(&client, &base, Method::GET, "/api/v1/channels", credential, None).await;
                assert!(!ids(&channels).contains(&private));
                assert_eq!(request(&client, &base, Method::GET, &format!("/api/v1/channels/{private}/messages"), credential, None).await.0, StatusCode::NOT_FOUND);
                assert_eq!(upgrade(address, &format!("/api/v1/channels/{private}/stream"), credential).await.0, if credential.is_some() { 404 } else { 401 });
            }
            assert_eq!(request(&client, &base, Method::POST, &format!("/api/v1/channels/{private}/messages"), Some(&admin_cookie), Some(json!({"content":"private channel must remain inaccessible"}))).await.0, StatusCode::NOT_FOUND);
        }
        state.limits.lock().await.clear();

        // Existing streams still revalidate permission, channel policy and sessions.
        let (status, mut protected_socket) = upgrade(address, &protected_stream, Some(&reader_cookie)).await;
        assert_eq!(status, 101);
        protected_socket.ready().await;
        sqlx::query("UPDATE roles SET permissions=ARRAY['chat.send'] WHERE id=$1")
            .bind(permission_role).execute(&pool).await.unwrap();
        protected_socket.closed().await;
        let (status, mut policy_socket) = upgrade(address, &stream_path, Some(&admin_cookie)).await;
        assert_eq!(status, 101);
        policy_socket.ready().await;
        sqlx::query("UPDATE channels SET required_permission='chat.pm' WHERE id=$1")
            .bind(channel).execute(&pool).await.unwrap();
        policy_socket.closed().await;
        sqlx::query("UPDATE channels SET required_permission=NULL WHERE id=$1")
            .bind(channel).execute(&pool).await.unwrap();
        for restricted_viewer in [false, true] {
            let id = user(&pool, if restricted_viewer { "ChatRestrictedViewer" } else { "ChatRevokedViewer" }, 1).await;
            let (sid, credential) = session(&state, id).await;
            let (status, mut connection) = upgrade(address, &stream_path, Some(&credential)).await;
            assert_eq!(status, 101);
            connection.ready().await;
            if restricted_viewer {
                sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(id).execute(&pool).await.unwrap();
            } else {
                sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&pool).await.unwrap();
            }
            connection.closed().await;
        }
    }).await;
    server.abort();
    let _ = server.await;
    db.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    outcome.unwrap();
}
