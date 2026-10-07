//! Real website HTTP and WebSocket checks in an independently owned PG17 schema.
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

fn fixture_state(db: PgPool) -> AppState {
    AppState {
        db,
        secret: Arc::new("website-chat-fixture-secret-at-least-thirty-two-bytes".into()),
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
    }
}

#[tokio::test]
async fn chat_rate_limit_recovers_at_exact_expired_capacity() {
    use crate::website_chat::actor_limit;
    use tower::ServiceExt;
    let state = fixture_state(
        PgPoolOptions::new()
            .connect_lazy("postgres://localhost/unused")
            .unwrap(),
    );
    let old = std::time::Instant::now() - Duration::from_secs(601);
    let fill = || {
        (0..10_000)
            .map(|i| (format!("expired:{i}"), (old, 30)))
            .collect()
    };
    *state.limits.lock().await = fill();
    actor_limit(&state, 1, "chat-send").await.unwrap();
    assert_eq!(state.limits.lock().await.len(), 1);
    for _ in 1..30 {
        actor_limit(&state, 1, "chat-send").await.unwrap();
    }
    assert_eq!(
        actor_limit(&state, 1, "chat-send").await.unwrap_err().0,
        StatusCode::TOO_MANY_REQUESTS
    );
    actor_limit(&state, 2, "chat-send").await.unwrap();
    *state.limits.lock().await = fill();
    let response = router(state.clone())
        .oneshot(
            axum::http::Request::builder()
                .method(Method::POST)
                .uri("/api/v1/auth/login")
                .header("Origin", "http://localhost:3000")
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "expired capacity must reach the malformed request parser"
    );
    assert_eq!(state.limits.lock().await.len(), 1);
}

async fn actor(state: &AppState, name: &str, permissions: &[&str]) -> (i64, i64, Uuid, String) {
    let role: i64 =
        sqlx::query_scalar("INSERT INTO roles(name,permissions) VALUES($1,$2) RETURNING id")
            .bind(format!("role_{name}"))
            .bind(permissions)
            .fetch_one(&state.db)
            .await
            .unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'fixture','fixture') RETURNING id")
        .bind(name).bind(format!("{name}@example.test")).fetch_one(&state.db).await.unwrap();
    sqlx::query("INSERT INTO user_roles(user_id,role_id) VALUES($1,$2)")
        .bind(id)
        .bind(role)
        .execute(&state.db)
        .await
        .unwrap();
    let (sid, cookie) = session(state, id).await;
    (id, role, sid, cookie)
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
    cookie: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = client
        .request(method, format!("{base}{path}"))
        .header("Origin", "http://localhost:3000")
        .header("Cookie", cookie);
    if let Some(value) = body {
        request = request.json(&value);
    }
    let response = request.send().await.unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn ids(value: &Value) -> Vec<i64> {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_i64().unwrap())
        .collect()
}
async fn private(db: &PgPool, sender: i64, recipient: i64, text: &str, uuid: Option<&str>) -> i64 {
    let pid:i64=sqlx::query_scalar("INSERT INTO protocol_private_messages(sender_id,recipient_id,content,client_uuid) VALUES($1,$2,$3,$4) RETURNING id")
        .bind(sender).bind(recipient).bind(text).bind(uuid).fetch_one(db).await.unwrap();
    sqlx::query_scalar("SELECT id FROM chat_messages WHERE private_message_id=$1")
        .bind(pid)
        .fetch_one(db)
        .await
        .unwrap()
}
async fn public(db: &PgPool, sender: i64, channel: i64, text: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,$3) RETURNING id",
    )
    .bind(channel)
    .bind(sender)
    .bind(text)
    .fetch_one(db)
    .await
    .unwrap()
}
struct Socket(BufReader<TcpStream>);
async fn upgrade(address: SocketAddr, path: &str, cookie: &str) -> Socket {
    let mut socket = BufReader::new(TcpStream::connect(address).await.unwrap());
    socket.get_mut().write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://localhost:3000\r\nCookie: {cookie}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes()).await.unwrap();
    let mut line = String::new();
    socket.read_line(&mut line).await.unwrap();
    assert!(line.contains("101"), "{line}");
    loop {
        line.clear();
        assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
    }
    Socket(socket)
}
impl Socket {
    async fn frame(&mut self) -> (u8, Vec<u8>) {
        tokio::time::timeout(Duration::from_secs(12), async {
            let mut head = [0; 2];
            self.0.read_exact(&mut head).await.unwrap();
            assert_eq!(head[1] & 0x80, 0);
            let length = match head[1] & 0x7f {
                126 => self.0.read_u16().await.unwrap() as u64,
                127 => self.0.read_u64().await.unwrap(),
                n => u64::from(n),
            };
            assert!(length <= 65_536);
            let mut bytes = vec![0; length as usize];
            self.0.read_exact(&mut bytes).await.unwrap();
            (head[0] & 15, bytes)
        })
        .await
        .expect("socket must deliver its expected committed event or close")
    }
    async fn json(&mut self) -> Value {
        let (opcode, bytes) = self.frame().await;
        assert_eq!(opcode, 1);
        serde_json::from_slice(&bytes).unwrap()
    }
    async fn ready(&mut self) {
        assert_eq!(self.json().await, json!({"type":"ready"}));
    }
    async fn message(&mut self) -> Value {
        loop {
            let value = self.json().await;
            if value["type"] == "refresh" {
                continue;
            }
            assert_eq!(value["type"], "messages");
            assert_eq!(value["items"].as_array().unwrap().len(), 1);
            return value["items"][0].clone();
        }
    }
    async fn ping_ready(&mut self) {
        self.0
            .get_mut()
            .write_all(b"\x89\x85\0\0\0\0ready")
            .await
            .unwrap();
        let (opcode, bytes) = self.frame().await;
        assert_eq!(opcode, 10);
        assert_eq!(bytes, b"ready");
    }
    async fn closed(&mut self) {
        loop {
            let (opcode, bytes) = self.frame().await;
            if opcode == 8 {
                break;
            }
            assert_eq!(opcode, 1);
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["type"], "refresh");
        }
    }
}
async fn wait_lock(db: &PgPool, app: &str, fragment: &str) {
    tokio::time::timeout(Duration::from_secs(5),async {loop {
        let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event_type='Lock' AND query LIKE $2)").bind(app).bind(format!("%{fragment}%")).fetch_one(db).await.unwrap();
        if waiting {break;}tokio::time::sleep(Duration::from_millis(10)).await;
    }}).await.expect("queued website request must reach its expected mutation lock");
}
async fn expire(db: &PgPool, sid: Uuid) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let expired: bool = sqlx::query_scalar(
                "SELECT expires_at<=clock_timestamp() FROM sessions WHERE id=$1",
            )
            .bind(sid)
            .fetch_one(db)
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
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing to disposable PostgreSQL 17"]
async fn website_conversations_commit_delivery_and_mutation_fences() {
    let url =
        std::env::var("ILLYA_TEST_DATABASE_URL").expect("Set disposable website chat fixture URL");
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
    let schema = format!("illya_website_chat_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&control)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&url)
        .unwrap()
        .options([("search_path", schema.as_str())])
        .application_name(&schema);
    let db = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&db).await.unwrap();
    let state = AppState {
        db: db.clone(),
        secret: Arc::new("website-chat-fixture-secret-at-least-thirty-two-bytes".into()),
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
    let app_name = schema.clone();
    let outcome=tokio::spawn(async move {
        let client=reqwest::Client::new();let base=format!("http://{address}");
        let (alice,_,alice_sid,alice_cookie)=actor(&state,"WebAlice",&["chat.send"]).await;
        let (bob,_,_,bob_cookie)=actor(&state,"WebBob",&["chat.send"]).await;
        let (stranger,_,_,stranger_cookie)=actor(&state,"WebStranger",&["chat.send"]).await;
        let (_,_,_,admin_cookie)=actor(&state,"WebAdmin",&["*"]).await;
        let (observer,_,_,observer_cookie)=actor(&state,"WebObserver",&[]).await;
        let (code,created)=request(&client,&base,Method::POST,"/api/v1/conversations",&alice_cookie,Some(json!({"target_username":"webbob"}))).await;
        assert_eq!(code,StatusCode::OK,"{created}");let channel=created["id"].as_i64().unwrap();assert_eq!(created["peer"]["id"],bob);assert_eq!(created["unread_count"],0);assert_eq!(created["last_message"],Value::Null);
        let conversation_path=format!("/api/v1/conversations/{channel}");let message_path=format!("{conversation_path}/messages");let read_path=format!("{conversation_path}/read");
        let (_,again)=request(&client,&base,Method::POST,"/api/v1/conversations",&alice_cookie,Some(json!({"target_id":bob}))).await;assert_eq!(again["id"],channel);
        for cookie in [&stranger_cookie,&admin_cookie] {
            assert_eq!(request(&client,&base,Method::GET,&conversation_path,cookie,None).await.0,StatusCode::NOT_FOUND);
            assert_eq!(request(&client,&base,Method::GET,&message_path,cookie,None).await.0,StatusCode::NOT_FOUND);
        }
        sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2)").bind(stranger).bind(channel).execute(&pool).await.unwrap();
        let (_,foreign_list)=request(&client,&base,Method::GET,"/api/v1/conversations",&stranger_cookie,None).await;assert!(ids(&foreign_list).is_empty());
        assert_eq!(request(&client,&base,Method::GET,&message_path,&stranger_cookie,None).await.0,StatusCode::NOT_FOUND);
        for sentinel in ["lazer.room","stable.room"] {
            let room:i64=sqlx::query_scalar("INSERT INTO channels(name,required_permission) VALUES($1,$1) RETURNING id").bind(sentinel).fetch_one(&pool).await.unwrap();
            assert_eq!(request(&client,&base,Method::GET,&format!("/api/v1/conversations/{room}"),&admin_cookie,None).await.0,StatusCode::NOT_FOUND);
        }
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/conversations",&observer_cookie,Some(json!({"target_id":bob}))).await.0,StatusCode::FORBIDDEN);
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/conversations",&alice_cookie,Some(json!({"target_id":alice}))).await.0,StatusCode::BAD_REQUEST);
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"spoof","user_id":bob,"client_uuid":"spoof"}))).await.0,StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"no uuid"}))).await.0,StatusCode::BAD_REQUEST);
        let payload=json!({"content":"  website action  ","is_action":true,"client_uuid":"website:action"});
        let (code,sent)=request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(payload.clone())).await;assert_eq!(code,StatusCode::OK,"{sent}");assert_eq!(sent["user_id"],alice);assert_eq!(sent["content"],"website action");assert_eq!(sent["is_action"],true);let sent_id=sent["id"].as_i64().unwrap();
        let mirrored:(i64,i64,String,bool,String)=sqlx::query_as("SELECT sender_id,recipient_id,content,is_action,client_uuid FROM protocol_private_messages WHERE id=$1").bind(sent["private_message_id"].as_i64().unwrap()).fetch_one(&pool).await.unwrap();assert_eq!(mirrored,(alice,bob,"website action".into(),true,"website:action".into()));
        let (_,retry)=request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(payload.clone())).await;assert_eq!(retry["id"],sent_id);
        let concurrent=json!({"content":"concurrent retry","client_uuid":"website:concurrent"});
        let (first,second)=tokio::join!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(concurrent.clone())),request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(concurrent)));
        assert_eq!(first.0,StatusCode::OK);assert_eq!(second.0,StatusCode::OK);assert_eq!(first.1["id"],second.1["id"]);
        let unique:i64=sqlx::query_scalar("SELECT count(*) FROM protocol_private_messages WHERE sender_id=$1 AND client_uuid='website:concurrent'").bind(alice).fetch_one(&pool).await.unwrap();assert_eq!(unique,1);
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"changed","client_uuid":"website:action","is_action":true}))).await.0,StatusCode::CONFLICT);
        let native_id=private(&pool,alice,bob,"native retry",Some("native:retry")).await;
        let (_,retry)=request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"native retry","client_uuid":"native:retry"}))).await;assert_eq!(retry["id"],native_id);
        let received=private(&pool,bob,alice,"stable native incoming",None).await;
        let (_,metadata)=request(&client,&base,Method::GET,&conversation_path,&alice_cookie,None).await;assert_eq!(metadata["unread_count"],1);assert_eq!(metadata["last_message"]["id"],received);
        assert_eq!(request(&client,&base,Method::PUT,&read_path,&alice_cookie,Some(json!({"message_id":received}))).await.0,StatusCode::NO_CONTENT);
        assert_eq!(request(&client,&base,Method::PUT,&read_path,&alice_cookie,Some(json!({"message_id":received}))).await.0,StatusCode::NO_CONTENT);
        assert_eq!(request(&client,&base,Method::PUT,&read_path,&alice_cookie,Some(json!({"message_id":sent_id}))).await.0,StatusCode::BAD_REQUEST);
        let native_read:i64=sqlx::query_scalar("SELECT last_read_id FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2").bind(alice).bind(channel).fetch_one(&pool).await.unwrap();assert_eq!(native_read,received);
        let other=private(&pool,bob,observer,"other conversation",None).await;
        assert_eq!(request(&client,&base,Method::PUT,&read_path,&alice_cookie,Some(json!({"message_id":other}))).await.0,StatusCode::BAD_REQUEST);
        let public_channel:i64=sqlx::query_scalar("INSERT INTO channels(name) VALUES('web-public') RETURNING id").fetch_one(&pool).await.unwrap();
        let guest_public=public(&pool,stranger,public_channel,"guest visible public").await;
        let (restricted_user,_,_,_)=actor(&state,"GuestRestricted",&["chat.send"]).await;
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(restricted_user).execute(&pool).await.unwrap();
        let guest_hidden=public(&pool,restricted_user,public_channel,"guest restricted sender").await;
        let guest_room:i64=sqlx::query_scalar("INSERT INTO channels(name,required_permission) VALUES('guest-private-room','lazer.room') RETURNING id").fetch_one(&pool).await.unwrap();
        let guest_private=public(&pool,alice,guest_room,"guest private room").await;
        let guest_visibility=request(&client,&base,Method::POST,"/api/v1/chat/visibility","",Some(json!({"message_ids":[guest_public,guest_hidden,guest_private,sent_id]}))).await;
        assert_eq!(guest_visibility.0,StatusCode::OK);assert_eq!(guest_visibility.1["message_ids"],json!([guest_public]));
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/chat/visibility","illya_session=invalid",Some(json!({"message_ids":[guest_public]}))).await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(request(&client,&base,Method::POST,&format!("/api/v1/channels/{public_channel}/messages"),&alice_cookie,Some(payload.clone())).await.0,StatusCode::CONFLICT);
        let (_,preferences)=request(&client,&base,Method::PATCH,"/api/v1/me/preferences",&bob_cookie,Some(json!({"locale":"ja","mode":"light","accent_color":"#123456","pm_friends_only":true}))).await;assert_eq!(preferences["pm_friends_only"],true);
        let (_,preferences)=request(&client,&base,Method::PATCH,"/api/v1/me/preferences",&bob_cookie,Some(json!({"panel_blur":9}))).await;assert_eq!(preferences["locale"],"ja");assert_eq!(preferences["accent_color"],"#123456");assert_eq!(preferences["pm_friends_only"],true);
        let (_,metadata)=request(&client,&base,Method::GET,&conversation_path,&alice_cookie,None).await;assert_eq!(metadata["send_restriction"],"friend_only");
        let deny=request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"new policy","client_uuid":"policy-new"}))).await;assert_eq!(deny.0,StatusCode::FORBIDDEN);assert_eq!(deny.1["error"]["code"],"recipient_accepts_friends_only");
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(payload.clone())).await.1["id"],sent_id,"matching retry remains valid after friend-only change");
        // Friend-only uses recipient -> sender, never sender -> recipient.
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend')").bind(alice).bind(bob).execute(&pool).await.unwrap();
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"wrong direction","client_uuid":"wrong-friend"}))).await.0,StatusCode::FORBIDDEN);
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'friend')").bind(bob).bind(alice).execute(&pool).await.unwrap();
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"recipient friend","client_uuid":"right-friend"}))).await.0,StatusCode::OK);
        // An active stable setting applies even when the typed preference is off.
        sqlx::query("DELETE FROM user_relationships WHERE user_id=$1 AND target_id=$2").bind(bob).bind(alice).execute(&pool).await.unwrap();
        request(&client,&base,Method::PATCH,"/api/v1/me/preferences",&bob_cookie,Some(json!({"pm_friends_only":false}))).await;
        sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at,block_non_friend_dms) VALUES($1,$2,$3,clock_timestamp()+interval '1 hour',true)").bind(Uuid::new_v4()).bind(Uuid::new_v4().to_string()).bind(bob).execute(&pool).await.unwrap();
        assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(json!({"content":"stable policy","client_uuid":"stable-policy"}))).await.0,StatusCode::FORBIDDEN);
        sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1").bind(bob).execute(&pool).await.unwrap();
        for (blocker,target) in [(alice,bob),(bob,alice)] {
            sqlx::query("DELETE FROM user_relationships WHERE (user_id=$1 AND target_id=$2) OR(user_id=$2 AND target_id=$1)").bind(alice).bind(bob).execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block')").bind(blocker).bind(target).execute(&pool).await.unwrap();
            assert_eq!(request(&client,&base,Method::GET,&message_path,&alice_cookie,None).await.0,StatusCode::NOT_FOUND);
            assert_eq!(request(&client,&base,Method::POST,&message_path,&alice_cookie,Some(payload.clone())).await.0,StatusCode::NOT_FOUND);
            assert!(ids(&request(&client,&base,Method::GET,"/api/v1/conversations",&alice_cookie,None).await.1).is_empty());
            let visible=request(&client,&base,Method::POST,"/api/v1/chat/visibility",&alice_cookie,Some(json!({"message_ids":[sent_id,received,other]}))).await;assert_eq!(visible.1["message_ids"],json!([]));
        }
        sqlx::query("DELETE FROM user_relationships WHERE (user_id=$1 AND target_id=$2) OR(user_id=$2 AND target_id=$1)").bind(alice).bind(bob).execute(&pool).await.unwrap();
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(bob).execute(&pool).await.unwrap();
        assert_eq!(request(&client,&base,Method::GET,&conversation_path,&alice_cookie,None).await.0,StatusCode::NOT_FOUND);
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/chat/visibility",&alice_cookie,Some(json!({"message_ids":[sent_id]}))).await.1["message_ids"],json!([]));
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(bob).execute(&pool).await.unwrap();
        // Reading is allowed without send permission and metadata explains it.
        let observer_channel:i64=sqlx::query_scalar("SELECT illya_pm_channel($1,$2)").bind(observer).bind(bob).fetch_one(&pool).await.unwrap();
        let (_,metadata)=request(&client,&base,Method::GET,&format!("/api/v1/conversations/{observer_channel}"),&observer_cookie,None).await;assert_eq!(metadata["send_restriction"],"no_permission");

        let mut unified=upgrade(address,"/api/v1/chat/stream",&alice_cookie).await;unified.ready().await;
        let mut channel_socket=upgrade(address,&format!("/api/v1/channels/{public_channel}/stream"),&alice_cookie).await;channel_socket.ping_ready().await;
        // A sequence ID is allocated before commit. The older ID commits last.
        let mut late=pool.begin().await.unwrap();let low:i64=sqlx::query_scalar("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'late lower commit') RETURNING id").bind(public_channel).bind(bob).fetch_one(&mut *late).await.unwrap();
        let high=public(&pool,bob,public_channel,"newer first commit").await;assert!(high>low);
        assert_eq!(unified.message().await["id"],high);assert_eq!(channel_socket.message().await["id"],high);
        late.commit().await.unwrap();assert_eq!(unified.message().await["id"],low);assert_eq!(channel_socket.message().await["id"],low);
        let dm=private(&pool,bob,alice,"native to website stream",None).await;assert_eq!(unified.message().await["id"],dm);
        let foreign=private(&pool,bob,stranger,"foreign private stream must hide",None).await;
        let sentinel=public(&pool,bob,public_channel,"visible after foreign PM").await;assert_eq!(unified.message().await["id"],sentinel);
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/chat/visibility",&alice_cookie,Some(json!({"message_ids":[foreign,sentinel,dm]}))).await.1["message_ids"],json!([dm,sentinel]));
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(bob).execute(&pool).await.unwrap();
        public(&pool,bob,public_channel,"hidden restricted live sender").await;
        let visible=public(&pool,stranger,public_channel,"visible live sender").await;assert_eq!(unified.message().await["id"],visible);
        assert_eq!(request(&client,&base,Method::POST,"/api/v1/chat/visibility",&alice_cookie,Some(json!({"message_ids":[dm,high,visible]}))).await.1["message_ids"],json!([visible]));
        // Observe the channel receiver while the sender is still restricted.
        assert_eq!(channel_socket.message().await["id"],sentinel);assert_eq!(channel_socket.message().await["id"],visible);
        sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(bob).execute(&pool).await.unwrap();
        // Refresh carries no hidden metadata and reconciles caches periodically.
        let value=unified.json().await;
        if value["type"]!="refresh" {panic!("unexpected event while waiting for policy refresh: {value}");}
        assert_eq!(value,json!({"type":"refresh"}));
        sqlx::query("UPDATE channels SET required_permission='staff.read' WHERE id=$1").bind(public_channel).execute(&pool).await.unwrap();channel_socket.closed().await;
        sqlx::query("DELETE FROM sessions WHERE id=$1").bind(alice_sid).execute(&pool).await.unwrap();unified.closed().await;
        let (sid,fresh_cookie)=session(&state,alice).await;let mut expiring=upgrade(address,"/api/v1/chat/stream",&fresh_cookie).await;expiring.ready().await;
        sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '100 milliseconds' WHERE id=$1").bind(sid).execute(&pool).await.unwrap();expiring.closed().await;

        // An interrupted LISTEN connection closes this generation. A new socket
        // subscribes to a fresh listener before ready and recovers through REST.
        let (recovery_sid,recovery_cookie)=session(&state,alice).await;
        let mut lost=upgrade(address,"/api/v1/chat/stream",&recovery_cookie).await;lost.ready().await;
        let terminated:Vec<bool>=sqlx::query_scalar("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name=$1 AND query LIKE 'LISTEN %'").bind(&app_name).fetch_all(&pool).await.unwrap();
        assert_eq!(terminated,vec![true],"only this fixture's one shared listener is terminated");lost.closed().await;
        let mut recovered=upgrade(address,"/api/v1/chat/stream",&recovery_cookie).await;recovered.ready().await;
        let recovery_channel:i64=sqlx::query_scalar("INSERT INTO channels(name) VALUES('web-recovery-public') RETURNING id").fetch_one(&pool).await.unwrap();
        let recovered_id=public(&pool,bob,recovery_channel,"listener recovery event").await;assert_eq!(recovered.message().await["id"],recovered_id);
        sqlx::query("DELETE FROM sessions WHERE id=$1").bind(recovery_sid).execute(&pool).await.unwrap();recovered.closed().await;

        // Every queued PM mutation rechecks authorization after its lock wait.
        // A denied write must leave both stores unchanged, including bridge rows.
        state.limits.lock().await.clear();
        for operation in ["create","send"] {
            for failure in ["revoked","restricted","permission"] {
                let (id,role,sid,cookie)=actor(&state,&format!("Queued_{operation}_{failure}"),&["chat.send"]).await;
                let queued_channel:i64=sqlx::query_scalar("SELECT illya_pm_channel($1,$2)").bind(id).bind(bob).fetch_one(&pool).await.unwrap();
                let before:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM protocol_private_messages),(SELECT count(*) FROM chat_messages)").fetch_one(&pool).await.unwrap();
                let mut blocker=pool.begin().await.unwrap();
                let fragment=if failure=="permission" {sqlx::query("LOCK TABLE roles,user_roles IN SHARE ROW EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();"LOCK TABLE roles,user_roles IN SHARE MODE"} else {sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(id).fetch_one(&mut *blocker).await.unwrap();"SELECT id FROM users WHERE id=ANY"};
                let (path,body)=if operation=="create" {("/api/v1/conversations".into(),json!({"target_id":bob}))} else {(format!("/api/v1/conversations/{queued_channel}/messages"),json!({"content":"must not publish","client_uuid":format!("queued-{id}")}))};
                let work=tokio::spawn({let c=client.clone();let b=base.clone();async move{request(&c,&b,Method::POST,&path,&cookie,Some(body)).await}});
                wait_lock(&pool,&app_name,fragment).await;
                match failure {"revoked"=>{sqlx::query("DELETE FROM sessions WHERE id=$1").bind(sid).execute(&mut *blocker).await.unwrap();},"restricted"=>{sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(id).execute(&mut *blocker).await.unwrap();},_=>{sqlx::query("UPDATE roles SET permissions='{}' WHERE id=$1").bind(role).execute(&mut *blocker).await.unwrap();}}
                blocker.commit().await.unwrap();let(code,value)=work.await.unwrap();assert_eq!(code,if failure=="permission" {StatusCode::FORBIDDEN}else{StatusCode::UNAUTHORIZED},"{operation} {failure}: {value}");
                let after:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM protocol_private_messages),(SELECT count(*) FROM chat_messages)").fetch_one(&pool).await.unwrap();assert_eq!(after,before);
            }
        }
        // Expiry after actor acquisition still rolls back the resource write.
        for operation in ["create","send","read"] {
            let (id,_,sid,cookie)=actor(&state,&format!("Expiry_{operation}"),&["chat.send"]).await;
            let queued_channel:i64=sqlx::query_scalar("SELECT illya_pm_channel($1,$2)").bind(id).bind(bob).fetch_one(&pool).await.unwrap();
            let marker=private(&pool,bob,id,"expiry marker",None).await;
            sqlx::query("UPDATE sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE id=$1").bind(sid).execute(&pool).await.unwrap();
            let mut blocker=pool.begin().await.unwrap();sqlx::query("SELECT id FROM channels WHERE id=$1 FOR UPDATE").bind(queued_channel).fetch_one(&mut *blocker).await.unwrap();
            let (method,path,body)=match operation {"create"=>(Method::POST,"/api/v1/conversations".into(),json!({"target_id":bob})),"read"=>(Method::PUT,format!("/api/v1/conversations/{queued_channel}/read"),json!({"message_id":marker})),_=>(Method::POST,format!("/api/v1/conversations/{queued_channel}/messages"),json!({"content":"expired","client_uuid":format!("expiry-{id}")}))};
            let work=tokio::spawn({let c=client.clone();let b=base.clone();async move{request(&c,&b,method,&path,&cookie,Some(body)).await}});
            wait_lock(&pool,&app_name,if operation=="create" {"illya_pm_channel"} else {"FOR SHARE OF c"}).await;expire(&pool,sid).await;blocker.commit().await.unwrap();let(code,value)=work.await.unwrap();assert_eq!(code,StatusCode::UNAUTHORIZED,"{operation}: {value}");
            let read:i64=sqlx::query_scalar("SELECT last_read_id FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2").bind(id).bind(queued_channel).fetch_one(&pool).await.unwrap();assert_eq!(read,0);
            let inserted:i64=sqlx::query_scalar("SELECT count(*) FROM chat_messages WHERE user_id=$1").bind(id).fetch_one(&pool).await.unwrap();assert_eq!(inserted,0);
        }
        // Policy changes queued on the peer lock are observed before insertion.
        for failure in ["block","friend_only","restricted"] {
            let (id,_,_,cookie)=actor(&state,&format!("Peer_{failure}"),&["chat.send"]).await;
            let cid:i64=sqlx::query_scalar("SELECT illya_pm_channel($1,$2)").bind(id).bind(bob).fetch_one(&pool).await.unwrap();
            let mut blocker=pool.begin().await.unwrap();sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE").bind(bob).fetch_one(&mut *blocker).await.unwrap();
            let work=tokio::spawn({let c=client.clone();let b=base.clone();async move{request(&c,&b,Method::POST,&format!("/api/v1/conversations/{cid}/messages"),&cookie,Some(json!({"content":"queued policy","client_uuid":format!("peer-{id}")}))).await}});
            wait_lock(&pool,&app_name,"SELECT id FROM users WHERE id=ANY").await;
            match failure {"block"=>{sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block')").bind(bob).bind(id).execute(&mut *blocker).await.unwrap();},"friend_only"=>{sqlx::query("UPDATE users SET preferences=preferences||'{\"pm_friends_only\":true}'::jsonb WHERE id=$1").bind(bob).execute(&mut *blocker).await.unwrap();},_=>{sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(bob).execute(&mut *blocker).await.unwrap();}}
            blocker.commit().await.unwrap();assert_eq!(work.await.unwrap().0,if failure=="friend_only" {StatusCode::FORBIDDEN}else{StatusCode::NOT_FOUND});
            sqlx::query("UPDATE users SET is_restricted=false,preferences=preferences||'{\"pm_friends_only\":false}'::jsonb WHERE id=$1").bind(bob).execute(&pool).await.unwrap();
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
