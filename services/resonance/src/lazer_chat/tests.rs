use super::*;
use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message as ClientMessage, client::IntoClientRequest},
};

#[test]
fn native_commands_have_no_arbitrary_payload_or_unbounded_frames() {
    assert_eq!(
        command(r#"{"event":"chat.start","data":null,"error":null}"#),
        Ok(true)
    );
    assert_eq!(command(r#"{"event":"chat.end"}"#), Ok(false));
    assert!(command(r#"{"event":"chat.start","data":{"user_id":9}}"#).is_err());
    assert!(command(r#"{"event":"chat.start","error":"spoof"}"#).is_err());
    assert!(command(r#"{"event":"chat.send"}"#).is_err());
    assert!(command(&"x".repeat(2049)).is_err());
}
#[test]
fn native_form_and_message_echo_bounds() {
    let input: ChatInput =
        serde_urlencoded::from_str("message=hello+Illya&is_action=true&uuid=native-client-1")
            .unwrap();
    assert_eq!(
        input.validate().unwrap(),
        ("hello Illya", Some("native-client-1"))
    );
    assert!(input.is_action);
    assert!(
        ChatInput {
            message: "\0".into(),
            is_action: false,
            uuid: None
        }
        .validate()
        .is_err()
    );
    assert!(
        ChatInput {
            message: "valid".into(),
            is_action: false,
            uuid: Some("x".repeat(129))
        }
        .validate()
        .is_err()
    );
    assert!(
        History {
            since: Some(-1),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        History {
            limit: Some(51),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    let pm: NewPm =
        serde_urlencoded::from_str("target_id=2&message=hello&is_action=false&uuid=pm-1").unwrap();
    assert_eq!(pm.target_id, 2);
    assert_eq!(pm.message, "hello");
}

const FORM_BOUNDARY: &str = "IllyaNativeChatFixture";
fn multipart_body(fields: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!(
            "--{FORM_BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{FORM_BOUNDARY}--\r\n"));
    body
}
fn multipart_request(fields: &[(&str, &str)]) -> Request {
    Request::builder()
        .header(
            "content-type",
            format!("multipart/form-data; boundary={FORM_BOUNDARY}"),
        )
        .body(axum::body::Body::from(multipart_body(fields)))
        .unwrap()
}
#[tokio::test]
async fn native_chat_scalar_multipart_and_existing_form_contracts() {
    // Pinned native PostMessageRequest sends the boolean in lowercase.
    let input: ChatInput = crate::lazer::native_form(multipart_request(&[
        ("message", "原創 Illya\nmessage"),
        ("is_action", "true"),
        ("uuid", "native-multipart-1"),
    ]))
    .await
    .unwrap();
    assert!(input.is_action);
    assert_eq!(input.validate().unwrap().0, "原創 Illya\nmessage");
    let pm: NewPm = crate::lazer::native_form(multipart_request(&[
        ("target_id", "2"),
        ("message", "private"),
        ("is_action", "false"),
    ]))
    .await
    .unwrap();
    assert_eq!(pm.target_id, 2);
    assert!(!pm.is_action);
    let channel: Create =
        crate::lazer::native_form(multipart_request(&[("type", "PM"), ("target_id", "2")]))
            .await
            .unwrap();
    assert_eq!(channel.kind, "PM");
    let ack: Keepalive = crate::lazer::native_form(multipart_request(&[
        ("since", "42"),
        ("history_since", "7"),
    ]))
    .await
    .unwrap();
    assert_eq!(ack.since, Some(42));
    assert_eq!(ack.history_since, Some(7));
    let ack: Keepalive = crate::lazer::native_form(multipart_request(&[]))
        .await
        .unwrap();
    assert_eq!(ack.since, None);
    let ack = parse_keepalive(Request::new(axum::body::Body::empty()))
        .await
        .unwrap();
    assert_eq!(ack.history_since, None);
    assert_eq!(
        parse_keepalive(Request::new(axum::body::Body::from("since=42")))
            .await
            .err()
            .unwrap()
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert!(
        crate::lazer::native_form::<Keepalive>(multipart_request(&[("channel", "42")]))
            .await
            .is_err()
    );
    assert!(
        crate::lazer::native_form::<ChatInput>(multipart_request(&[
            ("message", "valid"),
            ("sender_id", "42"),
        ]))
        .await
        .is_err()
    );
    for (content_type, body) in [
        (
            "application/x-www-form-urlencoded",
            "message=compatibility&is_action=false",
        ),
        (
            "application/json",
            r#"{"message":"compatibility","is_action":false}"#,
        ),
    ] {
        let request = Request::builder()
            .header("content-type", content_type)
            .body(axum::body::Body::from(body))
            .unwrap();
        let input: ChatInput = crate::lazer::native_form(request).await.unwrap();
        assert_eq!(input.message, "compatibility");
    }
}
#[test]
fn notification_endpoint_cannot_embed_credentials() {
    for invalid in [
        "https://osu.example/notification-server",
        "wss://token@osu.example/notification-server",
        "wss://osu.example/notification-server?access_token=secret",
        "wss://osu.example/notification-server#secret",
    ] {
        assert!(validate_endpoint(invalid).is_err());
    }
    assert!(validate_endpoint("wss://osu.example/notification-server").is_ok());
    assert!(validate_endpoint("ws://127.0.0.1:8081/notification-server").is_ok());
}
#[test]
fn socket_capacity_is_released_on_disconnect() {
    let user = i64::MAX - 1;
    let mut permits = Vec::new();
    for _ in 0..4 {
        permits.push(SocketPermit::take(user).unwrap());
    }
    assert!(SocketPermit::take(user).is_err());
    permits.pop();
    assert!(SocketPermit::take(user).is_ok());
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
async fn open(base: &str, token: &str) -> Socket {
    let mut request = format!(
        "{}/notification-server",
        base.replacen("http://", "ws://", 1)
    )
    .into_client_request()
    .unwrap();
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
        .send(ClientMessage::Text(
            r#"{"event":"chat.start","data":null,"error":null}"#.into(),
        ))
        .await
        .unwrap();
    socket
}
async fn event(socket: &mut Socket, name: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = socket.next().await.expect("socket closed").unwrap();
            if let ClientMessage::Text(text) = frame {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["event"] == name {
                    return value;
                }
            }
        }
    })
    .await
    .expect("event timed out")
}
async fn next_message(socket: &mut Socket) -> Value {
    event(socket, "chat.message.new").await["data"].clone()
}
async fn quiet_messages(socket: &mut Socket) {
    let result = tokio::time::timeout(Duration::from_millis(1100), async {
        loop {
            match socket.next().await {
                Some(Ok(ClientMessage::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(
                        value["event"], "chat.message.new",
                        "unexpected private/blocked message: {value}"
                    );
                }
                Some(Ok(ClientMessage::Ping(_))) => {}
                Some(Ok(_)) => {}
                None => break,
                Some(Err(_)) => break,
            }
        }
    })
    .await;
    assert!(result.is_err(), "socket unexpectedly closed");
}
async fn submit(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    channel: i64,
    text: &str,
    uuid: &str,
) -> reqwest::Response {
    client
        .post(format!("{base}/api/v2/chat/channels/{channel}/messages"))
        .bearer_auth(token)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={FORM_BOUNDARY}"),
        )
        .body(multipart_body(&[
            ("message", text),
            ("is_action", "false"),
            ("uuid", uuid),
        ]))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
async fn native_chat_http_and_websocket() {
    let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("ILLYA_TEST_DATABASE_URL");
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert!(
        matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
        "local disposable DB required"
    );
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("native_chat_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search = format!("SET search_path TO {schema}");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(18)
        .after_connect(move |connection, _| {
            let query = search.clone();
            Box::pin(async move {
                sqlx::query(&query).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = AppState {
        pool: pool.clone(),
        storage: Arc::new(std::env::temp_dir()),
        sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
        service_key: Arc::new(String::new()),
        http: reqwest::Client::new(),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        signalr: Arc::new(crate::signalr::Runtime::new()),
        lazer_chat: Arc::new(Runtime::with_endpoint(format!(
            "{}/notification-server",
            base.replacen("http://", "ws://", 1)
        ))),
    };
    state.lazer_chat.start(pool.clone()).await.unwrap();
    let app = router()
        .route("/", post(crate::stable::bancho))
        .merge(crate::stable::router())
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let result=std::panic::AssertUnwindSafe(async{
        let mut ids=Vec::new();let mut tokens=Vec::new();for n in 0..4{
            let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id").bind(format!("ChatPlayer{n}")).bind(format!("chat{n}@example.test")).fetch_one(&pool).await.unwrap();
            sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name=$2").bind(id).bind(if n==2{"administrator"}else{"player"}).execute(&pool).await.unwrap();
            let token=common::secret();sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')").bind(uuid::Uuid::new_v4()).bind(id).bind(common::token_hash(&token)).bind(common::secret()).execute(&pool).await.unwrap();ids.push(id);tokens.push(token);
        }
        let client=reqwest::Client::new();assert_eq!(client.get(format!("{base}/api/v2/notifications")).send().await.unwrap().status(),StatusCode::UNAUTHORIZED);
        let bundle:Value=client.get(format!("{base}/api/v2/notifications")).bearer_auth(&tokens[0]).send().await.unwrap().json().await.unwrap();assert_eq!(bundle["notification_endpoint"],base.replacen("http://","ws://",1)+"/notification-server");assert_eq!(bundle["notifications"],json!([]));
        let presence:Value=client.get(format!("{base}/api/v2/chat/updates?since=0&includes[]=presence&includes[]=silences")).bearer_auth(&tokens[0]).send().await.unwrap().json().await.unwrap();assert!(presence["presence"].as_array().unwrap().len()>=3);assert_eq!(presence["silences"],json!([]));assert!(presence.get("messages").is_none());
        let general:i64=sqlx::query_scalar("SELECT id FROM channels WHERE name='general'").fetch_one(&pool).await.unwrap();
        // The complete multipart body is bounded before parsing and writes.
        for (path, fields) in [
            (format!("chat/channels/{general}/messages"), vec![("message", "a"), ("message", "b")]),
            ("chat/channels".into(), vec![("type", "PM"), ("target_id", "1"), ("target_id", "2")]),
            ("chat/new".into(), vec![("target_id", "1"), ("message", "a"), ("message", "b")]),
            ("chat/ack".into(), vec![("since", "1"), ("since", "2")]),
        ] {
            assert_eq!(client.post(format!("{base}/api/v2/{path}")).bearer_auth(&tokens[0]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&fields)).send().await.unwrap().status(),StatusCode::BAD_REQUEST);
        }
        let file_body=format!("--{FORM_BOUNDARY}\r\nContent-Disposition: form-data; name=\"message\"; filename=\"chat.txt\"\r\n\r\nforbidden attachment\r\n--{FORM_BOUNDARY}--\r\n");
        assert_eq!(client.post(format!("{base}/api/v2/chat/channels/{general}/messages")).bearer_auth(&tokens[0]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(file_body).send().await.unwrap().status(),StatusCode::BAD_REQUEST);
        assert_eq!(client.post(format!("{base}/api/v2/chat/channels/{general}/messages")).bearer_auth(&tokens[0]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("message",&"x".repeat(8193))])).send().await.unwrap().status(),StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM chat_messages").fetch_one(&pool).await.unwrap(),0);
        assert!(sqlx::query("INSERT INTO channels(name,kind,dm_user_low,dm_user_high) VALUES('invalid-private','PM',$1,$2)").bind(ids[0]).bind(ids[1]).execute(&pool).await.is_err());
        let mut receiver=open(&base,&tokens[0]).await;event(&mut receiver,"chat.channel.join").await;
        let mut sender=open(&base,&tokens[1]).await;event(&mut sender,"chat.channel.join").await;
        let mut observer=open(&base,&tokens[2]).await;event(&mut observer,"chat.channel.join").await;
        let wrong_origin=client.get(format!("{base}/notification-server")).bearer_auth(&tokens[0]).header("Origin","https://untrusted.example").header("Connection","Upgrade").header("Upgrade","websocket").header("Sec-WebSocket-Key","dGhlIHNhbXBsZSBub25jZQ==").header("Sec-WebSocket-Version","13").send().await.unwrap();assert_eq!(wrong_origin.status(),StatusCode::FORBIDDEN);
        let sent:Value=submit(&client,&base,&tokens[1],general,"native public","public-1").await.json().await.unwrap();let message_id=sent["message_id"].as_i64().unwrap();
        let received=next_message(&mut receiver).await;assert_eq!(received["messages"][0],sent);assert_eq!(received["users"][0]["id"],ids[1]);assert_eq!(received["messages"][0]["uuid"],"public-1");next_message(&mut sender).await;next_message(&mut observer).await;
        let (first,second)=tokio::join!(submit(&client,&base,&tokens[1],general,"native public","public-1"),submit(&client,&base,&tokens[1],general,"native public","public-1"));assert_eq!(first.json::<Value>().await.unwrap()["message_id"],message_id);assert_eq!(second.json::<Value>().await.unwrap()["message_id"],message_id);
        assert_eq!(submit(&client,&base,&tokens[1],general,"changed retry","public-1").await.status(),StatusCode::CONFLICT);
        assert_eq!(client.put(format!("{base}/api/v2/chat/channels/{general}/mark-as-read/{message_id}")).bearer_auth(&tokens[0]).send().await.unwrap().status(),StatusCode::NO_CONTENT);
        assert_eq!(client.put(format!("{base}/api/v2/chat/channels/{general}/mark-as-read/0")).bearer_auth(&tokens[0]).send().await.unwrap().status(),StatusCode::BAD_REQUEST);

        // External website-style writes and delayed transactions use the same commit stream.
        let mut delayed=pool.begin().await.unwrap();let late:i64=sqlx::query_scalar("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'late transaction') RETURNING id").bind(general).bind(ids[3]).fetch_one(&mut *delayed).await.unwrap();
        sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'website writer')").bind(general).bind(ids[3]).execute(&pool).await.unwrap();assert_eq!(next_message(&mut receiver).await["messages"][0]["content"],"website writer");
        delayed.commit().await.unwrap();assert_eq!(next_message(&mut receiver).await["messages"][0]["message_id"],late);next_message(&mut sender).await;next_message(&mut sender).await;next_message(&mut observer).await;next_message(&mut observer).await;

        sqlx::query("UPDATE users SET preferences='{\"pm_friends_only\":true}' WHERE id=$1").bind(ids[0]).execute(&pool).await.unwrap();
        assert_eq!(client.post(format!("{base}/api/v2/chat/channels")).bearer_auth(&tokens[1]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("type","PM"),("target_id",&ids[0].to_string())])).send().await.unwrap().status(),StatusCode::FORBIDDEN);
        common::change_relationship(&state,ids[0],ids[1],"friend",false).await.unwrap();
        let created:Value=client.post(format!("{base}/api/v2/chat/channels")).bearer_auth(&tokens[1]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("type","PM"),("target_id",&ids[0].to_string())])).send().await.unwrap().json().await.unwrap();let pm=created["channel_id"].as_i64().unwrap();assert_eq!(created["type"],"PM");assert_eq!(created["recent_messages"],json!([]));
        sqlx::query("UPDATE users SET preferences='{}' WHERE id=$1").bind(ids[0]).execute(&pool).await.unwrap();
        assert_eq!(client.get(format!("{base}/api/v2/chat/channels/{pm}")).bearer_auth(&tokens[2]).send().await.unwrap().status(),StatusCode::NOT_FOUND);
        let pm_sent:Value=submit(&client,&base,&tokens[1],pm,"native private","private-1").await.json().await.unwrap();assert_eq!(next_message(&mut receiver).await["messages"][0],pm_sent);next_message(&mut sender).await;quiet_messages(&mut observer).await;
        let rejoined:Value=client.post(format!("{base}/api/v2/chat/channels")).bearer_auth(&tokens[1]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("type","PM"),("target_id",&ids[0].to_string())])).send().await.unwrap().json().await.unwrap();assert_eq!(rejoined["recent_messages"],json!([pm_sent.clone()]));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM protocol_private_messages WHERE content='native private'").fetch_one(&pool).await.unwrap(),1);
        let (left,right)=tokio::join!(submit(&client,&base,&tokens[0],pm,"opposite left","left-1"),submit(&client,&base,&tokens[1],pm,"opposite right","right-1"));assert_eq!(left.status(),StatusCode::OK);assert_eq!(right.status(),StatusCode::OK);next_message(&mut receiver).await;next_message(&mut receiver).await;next_message(&mut sender).await;next_message(&mut sender).await;
        let history:Value=client.get(format!("{base}/api/v2/chat/channels/{pm}/messages?until={}&limit=1",pm_sent["message_id"].as_i64().unwrap()+1)).bearer_auth(&tokens[0]).send().await.unwrap().json().await.unwrap();assert_eq!(history[0]["message_id"],pm_sent["message_id"]);
        common::change_relationship(&state,ids[0],ids[1],"block",false).await.unwrap();assert_eq!(client.get(format!("{base}/api/v2/chat/channels/{pm}/messages")).bearer_auth(&tokens[0]).send().await.unwrap().status(),StatusCode::NOT_FOUND);
        sqlx::query("INSERT INTO protocol_private_messages(sender_id,recipient_id,content) VALUES($1,$2,'blocked injected')").bind(ids[1]).bind(ids[0]).execute(&pool).await.unwrap();quiet_messages(&mut receiver).await;
        common::change_relationship(&state,ids[0],ids[1],"block",true).await.unwrap();

        // PM remains active without public keepalive; public notifications pause.
        sqlx::query("UPDATE lazer_chat_activity SET active_until=now()-interval '1 second' WHERE user_id=$1").bind(ids[0]).execute(&pool).await.unwrap();
        submit(&client,&base,&tokens[1],general,"expired public","expired-1").await;quiet_messages(&mut receiver).await;next_message(&mut sender).await;next_message(&mut observer).await;
        submit(&client,&base,&tokens[1],pm,"private while idle","private-idle").await;assert_eq!(next_message(&mut receiver).await["messages"][0]["content"],"private while idle");next_message(&mut sender).await;
        assert_eq!(client.post(format!("{base}/api/v2/chat/ack")).bearer_auth(&tokens[0]).form(&[("since","-1")]).send().await.unwrap().status(),StatusCode::BAD_REQUEST);
        assert_eq!(client.post(format!("{base}/api/v2/chat/ack")).bearer_auth(&tokens[0]).send().await.unwrap().status(),StatusCode::OK);
        assert_eq!(client.post(format!("{base}/api/v2/chat/ack")).bearer_auth(&tokens[0]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("since",&message_id.to_string()),("history_since","0")])).send().await.unwrap().status(),StatusCode::OK);

        let room_channel:i64=sqlx::query_scalar("INSERT INTO channels(name,required_permission) VALUES('private-room','lazer.room') RETURNING id").fetch_one(&pool).await.unwrap();let room:i64=sqlx::query_scalar("INSERT INTO lazer_rooms(host_id,channel_id,settings) VALUES($1,$2,'{}') RETURNING id").bind(ids[0]).bind(room_channel).fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO lazer_room_members(room_id,user_id) VALUES($1,$2)").bind(room).bind(ids[0]).execute(&pool).await.unwrap();sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2)").bind(ids[0]).bind(room_channel).execute(&pool).await.unwrap();
        assert_eq!(client.get(format!("{base}/api/v2/chat/channels/{room_channel}")).bearer_auth(&tokens[2]).send().await.unwrap().status(),StatusCode::NOT_FOUND);
        sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'private room')").bind(room_channel).bind(ids[0]).execute(&pool).await.unwrap();assert_eq!(next_message(&mut receiver).await["messages"][0]["content"],"private room");quiet_messages(&mut observer).await;
        sqlx::query("DELETE FROM lazer_room_members WHERE user_id=$1").bind(ids[0]).execute(&pool).await.unwrap();sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'after room revoke')").bind(room_channel).bind(ids[3]).execute(&pool).await.unwrap();quiet_messages(&mut receiver).await;
        assert_eq!(client.delete(format!("{base}/api/v2/chat/channels/{general}/users/{}",ids[0])).bearer_auth(&tokens[0]).send().await.unwrap().status(),StatusCode::NO_CONTENT);
        sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,'after membership deletion')").bind(general).bind(ids[3]).execute(&pool).await.unwrap();quiet_messages(&mut receiver).await;next_message(&mut sender).await;next_message(&mut observer).await;

        // Native PMs flow to stable; stable packets flow back to the native socket.
        let stable_token=common::secret();sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at,channels,last_dm_id) VALUES($1,$2,$3,now()+interval '3 minutes',$4,(SELECT COALESCE(max(id),0) FROM protocol_private_messages))").bind(uuid::Uuid::new_v4()).bind(common::token_hash(&stable_token)).bind(ids[3]).bind(vec![general]).execute(&pool).await.unwrap();
        let stable_pm:Value=client.post(format!("{base}/api/v2/chat/new")).bearer_auth(&tokens[1]).header("content-type",format!("multipart/form-data; boundary={FORM_BOUNDARY}")).body(multipart_body(&[("target_id",ids[3].to_string().as_str()),("message","waves"),("is_action","true"),("uuid","action-pm")])).send().await.unwrap().json().await.unwrap();assert!(stable_pm["message"]["is_action"].as_bool().unwrap());next_message(&mut sender).await;
        let reply=client.post(format!("{base}/")).header("osu-token",&stable_token).body(resonance_protocol::stable::encode_packet(resonance_protocol::stable::client::PING,&[]).unwrap()).send().await.unwrap().bytes().await.unwrap();let packets=resonance_protocol::stable::decode_packets(&reply).unwrap();assert!(packets.iter().filter(|packet|packet.id==resonance_protocol::stable::server::SEND_MESSAGE).any(|packet|resonance_protocol::stable::ChatMessage::decode(packet.payload).unwrap().text=="\u{1}ACTION waves\u{1}"));
        let text=resonance_protocol::stable::ChatMessage{sender:"spoof".into(),text:"stable to lazer".into(),recipient:"ChatPlayer0".into(),sender_id:0};
        let reply=client.post(format!("{base}/")).header("osu-token",&stable_token).body(resonance_protocol::stable::encode_packet(resonance_protocol::stable::client::SEND_PRIVATE_MESSAGE,&text.encode().unwrap()).unwrap()).send().await.unwrap();assert_eq!(reply.status(),StatusCode::OK);let received=next_message(&mut receiver).await;assert_eq!(received["messages"][0]["content"],"stable to lazer");assert_eq!(received["users"][0]["id"],ids[3]);
        receiver.send(ClientMessage::Text(r#"{"event":"chat.start"}"#.into())).await.unwrap();receiver.send(ClientMessage::Text(r#"{"event":"chat.start"}"#.into())).await.unwrap();quiet_messages(&mut receiver).await;
        receiver.send(ClientMessage::Text(r#"{"event":"chat.end"}"#.into())).await.unwrap();tokio::time::sleep(Duration::from_millis(50)).await;
        submit(&client,&base,&tokens[1],pm,"paused notification","paused-1").await;quiet_messages(&mut receiver).await;next_message(&mut sender).await;
        receiver.send(ClientMessage::Text(r#"{"event":"chat.start"}"#.into())).await.unwrap();tokio::time::sleep(Duration::from_millis(50)).await;
        submit(&client,&base,&tokens[1],pm,"resumed notification","resume-1").await;assert_eq!(next_message(&mut receiver).await["messages"][0]["content"],"resumed notification");next_message(&mut sender).await;
        sqlx::query("DELETE FROM user_roles WHERE user_id=$1").bind(ids[1]).execute(&pool).await.unwrap();assert_eq!(submit(&client,&base,&tokens[1],pm,"revoked send","revoked-1").await.status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE lazer_sessions SET access_expires_at=now()-interval '1 second' WHERE access_hash=$1").bind(common::token_hash(&tokens[0])).execute(&pool).await.unwrap();event(&mut receiver,"logout").await;
        sqlx::query("DELETE FROM lazer_sessions WHERE access_hash=$1").bind(common::token_hash(&tokens[2])).execute(&pool).await.unwrap();event(&mut observer,"logout").await;
        sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(ids[1]).execute(&pool).await.unwrap();event(&mut sender,"logout").await;
        let _=sender.close(None).await;let _=receiver.close(None).await;let _=observer.close(None).await;
    }).catch_unwind().await;
    server.abort();
    let _ = server.await;
    if let Some(task) = state.lazer_chat.listener.lock().unwrap().take() {
        task.abort();
    }
    drop(state);
    tokio::time::timeout(Duration::from_secs(10), pool.close())
        .await
        .expect("test connections did not release");
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
