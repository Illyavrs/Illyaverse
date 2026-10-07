//! True TCP HTTP/WebSocket integration against a fresh PostgreSQL schema.
use super::*;
use crate::{common, lazer, signalr};
use axum::{Json, Router, extract::State, routing::post};
use futures_util::{FutureExt, SinkExt, StreamExt};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

async fn ingest(State(pool): State<sqlx::PgPool>, Json(input): Json<Value>) -> Json<Value> {
    let id:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,native_submission) VALUES($1,$2,$3,$4,$5,$6,$7,0,$8,$9,'lazer',$10,$11,$12) ON CONFLICT(ingestion_key) DO UPDATE SET ingestion_key=excluded.ingestion_key RETURNING id").bind(input["user_id"].as_i64().unwrap()).bind(input["beatmap_id"].as_i64().unwrap()).bind(input["mode"].as_i64().unwrap() as i16).bind(input["score"].as_i64().unwrap()).bind(input["accuracy"].as_f64().unwrap()).bind(input["max_combo"].as_i64().unwrap() as i32).bind(input["mods"].as_i64().unwrap()).bind(input["passed"].as_bool().unwrap()).bind(&input["hit_counts"]).bind(input["ingestion_key"].as_str().unwrap()).bind(&input["api_mods"]).bind(&input["native_submission"]).fetch_one(&pool).await.unwrap();
    Json(json!({"id":id}))
}
type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn websocket(base: &str, secret: &str) -> Ws {
    let (mut socket, _) = connect_async(format!(
        "{}/spectator?access_token={secret}",
        base.replacen("http", "ws", 1)
    ))
    .await
    .unwrap();
    socket
        .send(Message::Text(
            "{\"protocol\":\"json\",\"version\":1}\u{1e}".into(),
        ))
        .await
        .unwrap();
    let ack = socket.next().await.unwrap().unwrap();
    assert_eq!(ack.into_text().unwrap(), "{}\u{1e}");
    socket
}
async fn call(socket: &mut Ws, target: &str, args: Vec<Value>) -> Value {
    socket
        .send(Message::Text(
            format!(
                "{}\u{1e}",
                json!({"type":1,"invocationId":"test","target":target,"arguments":args})
            )
            .into(),
        ))
        .await
        .unwrap();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = frame {
            for line in text.split('\u{1e}').filter(|s| !s.is_empty()) {
                let value: Value = serde_json::from_str(line).unwrap();
                if value["type"] == 3 {
                    return value;
                }
            }
        }
    }
}
fn ok(value: &Value) {
    assert!(value.get("error").is_none(), "{value}");
}
fn initial(map: i64) -> Value {
    json!({"beatmapID":map,"rulesetID":0,"mods":[{"acronym":"DT","settings":{"speed_change":1.2}}],"state":1,"maximumStatistics":{"5":2}})
}
fn bundle(time: f64) -> Value {
    json!({"header":{"totalScore":1234,"accuracy":1.0,"combo":2,"maxCombo":2,"statistics":{"5":2},"scoreProcessorStatistics":[600.0,600.0,2,0.5,0.0],"receivedTime":null,"mods":[{"acronym":"DT","settings":{"speed_change":1.2}}],"totalScoreWithoutMods":1234,"pauses":[]},"frames":[{"time":time,"mouseX":256.0,"mouseY":192.0,"buttonState":1}]})
}
fn submission() -> Value {
    json!({"ruleset_id":0,"passed":true,"total_score":1234,"total_score_without_mods":1234,"accuracy":1.0,"max_combo":2,"rank":"X","mods":[{"acronym":"DT","settings":{"speed_change":1.2}}],"statistics":{"great":2},"maximum_statistics":{"great":2},"pauses":[]})
}

#[tokio::test]
#[ignore = "requires ILLYA_TEST_DATABASE_URL, every run creates/drops its own schema"]
async fn native_replay_http_websocket_persistence() {
    let url = std::env::var("ILLYA_TEST_DATABASE_URL").unwrap();
    let control = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let schema = format!("replay_test_{}", uuid::Uuid::new_v4().simple());
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
    let storage = std::env::temp_dir().join(&schema);
    tokio::fs::create_dir_all(&storage).await.unwrap();
    let ingest_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ingest_url = format!("http://{}", ingest_listener.local_addr().unwrap());
    let ingest_app = Router::new()
        .route("/api/v1/internal/scores", post(ingest))
        .with_state(pool.clone());
    let ingest_server =
        tokio::spawn(async move { axum::serve(ingest_listener, ingest_app).await.unwrap() });
    let state = AppState {
        pool: pool.clone(),
        storage: Arc::new(storage.clone()),
        sanctuary_url: Arc::new(ingest_url),
        service_key: Arc::new("test-service-key-with-at-least-thirty-two-bytes".into()),
        http: reqwest::Client::new(),
        limits: Default::default(),
        trusted_proxies: Default::default(),
        signalr: Arc::new(signalr::Runtime::new()),
        lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .merge(lazer::router())
        .merge(signalr::router())
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result=std::panic::AssertUnwindSafe(async {
        let mut users=vec![];let mut secrets=vec![];for i in 0..2 {let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id").bind(format!("ReplayPlayer{i}")).bind(format!("replay{i}@example.invalid")).fetch_one(&pool).await.unwrap();sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='player'").bind(id).execute(&pool).await.unwrap();let secret=common::secret();sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')").bind(uuid::Uuid::new_v4()).bind(id).bind(common::token_hash(&secret)).bind(common::secret()).execute(&pool).await.unwrap();users.push(id);secrets.push(secret);}
        let map:i64=sqlx::query_scalar("INSERT INTO beatmaps(title,artist,creator,version,mode,stars,max_combo,md5,osu_path) VALUES('Fixture','Illyaverse','Tests','Easy',0,1,2,'0123456789abcdef0123456789abcdef','fixture.osu') RETURNING id").fetch_one(&pool).await.unwrap();let native_map:i64=sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1").bind(map).fetch_one(&pool).await.unwrap();assert_ne!(map,native_map);let client=reqwest::Client::new();
        let mut ws=websocket(&base,&secrets[0]).await;let mut guest=websocket(&base,&secrets[1]).await;
        for score_before_end in [false,true] {
            let token:Value=client.post(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores")).bearer_auth(&secrets[0]).form(&[("beatmap_hash","0123456789abcdef0123456789abcdef"),("version_hash","fixture-version"),("ruleset_id","0")]).send().await.unwrap().json().await.unwrap();let token=token["id"].as_i64().unwrap();
            let denial=call(&mut guest,"BeginPlaySessionV2",vec![json!(token),initial(native_map)]).await;assert_eq!(denial["error"],"score_token_owner_mismatch");ok(&call(&mut ws,"BeginPlaySessionV2",vec![json!(token),initial(native_map)]).await);ok(&call(&mut ws,"SendFrameDataV2",vec![json!(token),bundle(100.0)]).await);ok(&call(&mut ws,"SendFrameDataV2",vec![json!(token),bundle(100.0)]).await);
            if !score_before_end {ok(&call(&mut ws,"EndPlaySessionV2",vec![json!(token),json!(3)]).await);}
            let response=client.put(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores/{token}")).bearer_auth(&secrets[0]).json(&submission()).send().await.unwrap();assert!(response.status().is_success());let score:Value=response.json().await.unwrap();let id=score["id"].as_i64().unwrap();
            if score_before_end {assert_eq!(score["has_replay"],false);ok(&call(&mut ws,"EndPlaySessionV2",vec![json!(token),json!(3)]).await);}
            let retry:Value=client.put(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores/{token}")).bearer_auth(&secrets[0]).json(&submission()).send().await.unwrap().json().await.unwrap();assert_eq!(retry["id"],id);assert_eq!(retry["has_replay"],true);
            let response=client.get(format!("{base}/api/v2/scores/{id}/download")).bearer_auth(&secrets[0]).send().await.unwrap();assert_eq!(response.status(),StatusCode::OK);let bytes=response.bytes().await.unwrap();let replay=lazer_replay::ExtendedReplay::parse(&bytes).unwrap();assert_eq!(replay.metadata.online_id,id);assert_eq!(replay.metadata.user_id,users[0]);assert_eq!(replay.frame_count,1);
            assert_eq!(client.get(format!("{base}/api/v2/scores/{id}/download")).bearer_auth(&secrets[1]).send().await.unwrap().status(),StatusCode::NOT_FOUND);
            let capture=sqlx::query("SELECT completed_at,(SELECT count(*) FROM lazer_replay_chunks WHERE token_id=$1) chunks,frame_count FROM lazer_replay_captures WHERE token_id=$1").bind(token).fetch_one(&pool).await.unwrap();assert!(capture.get::<Option<chrono::DateTime<chrono::Utc>>,_>("completed_at").is_some());assert_eq!(capture.get::<i64,_>("chunks"),0);assert_eq!(capture.get::<i32,_>("frame_count"),1);assert_eq!(retry["replay_complete"],true);
            sqlx::query("UPDATE scores SET verification='verified' WHERE id=$1").bind(id).execute(&pool).await.unwrap();assert_eq!(client.get(format!("{base}/api/v2/scores/{id}/download")).bearer_auth(&secrets[1]).send().await.unwrap().status(),StatusCode::OK);
            sqlx::query("UPDATE users SET is_restricted=true WHERE id=$1").bind(users[0]).execute(&pool).await.unwrap();assert_eq!(client.get(format!("{base}/api/v2/scores/{id}/download")).bearer_auth(&secrets[1]).send().await.unwrap().status(),StatusCode::NOT_FOUND);sqlx::query("UPDATE users SET is_restricted=false WHERE id=$1").bind(users[0]).execute(&pool).await.unwrap();
        }
        // Current native V1 methods, with score submission racing EndPlaySession.
        let native_token:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,0,'fixture',now()+interval '1 hour') RETURNING id").bind(users[0]).bind(map).fetch_one(&pool).await.unwrap();ok(&call(&mut ws,"BeginPlaySession",vec![json!(native_token),initial(native_map)]).await);ok(&call(&mut ws,"SendFrameData",vec![bundle(100.0)]).await);let mut finalstate=initial(native_map);finalstate["state"]=json!(3);let mut wrongstate=finalstate.clone();wrongstate["beatmapID"]=json!(native_map+1);assert_eq!(call(&mut ws,"EndPlaySession",vec![wrongstate]).await["error"],"play_session_state_mismatch");
        let request=client.put(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores/{native_token}")).bearer_auth(&secrets[0]).json(&submission());let (ended,submitted)=tokio::join!(call(&mut ws,"EndPlaySession",vec![finalstate]),request.send());ok(&ended);assert!(submitted.unwrap().status().is_success());let ready:Value=client.put(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores/{native_token}")).bearer_auth(&secrets[0]).json(&submission()).send().await.unwrap().json().await.unwrap();assert_eq!(ready["has_replay"],true);assert_eq!(ready["replay_complete"],true);
        // Reconnect preserves bytes and invalidates the old connection's writer.
        let token:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,0,'fixture',now()+interval '1 hour') RETURNING id").bind(users[0]).bind(map).fetch_one(&pool).await.unwrap();begin(&state,users[0],token,"connection-a",&initial(native_map)).await.unwrap();frames(&state,users[0],token,"connection-a",&bundle(100.0)).await.unwrap();begin(&state,users[0],token,"connection-b",&initial(native_map)).await.unwrap();assert_eq!(frames(&state,users[0],token,"connection-a",&bundle(200.0)).await.unwrap_err().1,"replay_connection_replaced");frames(&state,users[0],token,"connection-b",&bundle(200.0)).await.unwrap();assert_eq!(frames(&state,users[0],token,"connection-b",&bundle(150.0)).await.unwrap_err().1,"replay_frame_out_of_order");assert!(sqlx::query_scalar::<_,bool>("SELECT interrupted FROM lazer_replay_captures WHERE token_id=$1").bind(token).fetch_one(&pool).await.unwrap());
        end(&state,users[0],token,"connection-b",3).await.unwrap();
        let score:Value=client.put(format!("{base}/api/v2/beatmaps/{native_map}/solo/scores/{token}")).bearer_auth(&secrets[0]).json(&submission()).send().await.unwrap().json().await.unwrap();let id=score["id"].as_i64().unwrap();assert_eq!(score["has_replay"],true);assert_eq!(score["replay_complete"],false);let bytes=client.get(format!("{base}/api/v2/scores/{id}/download")).bearer_auth(&secrets[0]).send().await.unwrap().bytes().await.unwrap();let replay=lazer_replay::ExtendedReplay::parse(&bytes).unwrap();assert_eq!(replay.frame_count,2);assert_eq!(replay.metadata.mods[0].settings["speed_change"],1.2);
        let (first,second)=tokio::join!(finalize(&state,users[0],token),finalize(&state,users[0],token));assert_eq!(first.unwrap(),Some(id));assert_eq!(second.unwrap(),Some(id));assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events WHERE action='scores.lazer_replay_capture' AND target_id=$1").bind(id.to_string()).fetch_one(&pool).await.unwrap(),1);assert_eq!(begin(&state,users[0],token,"connection-c",&initial(native_map)).await.unwrap_err().1,"replay_token_already_used");
        let expired:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,0,'fixture',now()-interval '1 second') RETURNING id").bind(users[0]).bind(map).fetch_one(&pool).await.unwrap();assert_eq!(begin(&state,users[0],expired,"connection-c",&initial(native_map)).await.unwrap_err().1,"invalid_score_token");
        let quit:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,0,'fixture',now()+interval '1 hour') RETURNING id").bind(users[0]).bind(map).fetch_one(&pool).await.unwrap();begin(&state,users[0],quit,"connection-q",&initial(native_map)).await.unwrap();frames(&state,users[0],quit,"connection-q",&bundle(100.0)).await.unwrap();end(&state,users[0],quit,"connection-q",5).await.unwrap();assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM lazer_replay_chunks WHERE token_id=$1").bind(quit).fetch_one(&pool).await.unwrap(),0);
        // Simulate a process stopping after committing token.score_id but before
        // calling finalize; persisted state alone must publish after recovery.
        let recovery:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,0,'fixture',now()+interval '1 hour') RETURNING id").bind(users[0]).bind(map).fetch_one(&pool).await.unwrap();begin(&state,users[0],recovery,"connection-recovery",&initial(native_map)).await.unwrap();frames(&state,users[0],recovery,"connection-recovery",&bundle(100.0)).await.unwrap();end(&state,users[0],recovery,"connection-recovery",3).await.unwrap();let recovery_score:i64=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,api_mods) VALUES($1,$2,0,1234,100,2,64,0,true,'{\"n300\":2}','lazer',$3) RETURNING id").bind(users[0]).bind(map).bind(submission()["mods"].clone()).fetch_one(&pool).await.unwrap();sqlx::query("UPDATE lazer_score_tokens SET score_id=$2,submission=$3,consumed_at=now() WHERE id=$1").bind(recovery).bind(recovery_score).bind(submission()).execute(&pool).await.unwrap();assert_eq!(recover_pending(&state).await.unwrap(),1);assert_eq!(recover_pending(&state).await.unwrap(),0);assert!(sqlx::query_scalar::<_,Option<String>>("SELECT replay_path FROM scores WHERE id=$1").bind(recovery_score).fetch_one(&pool).await.unwrap().is_some());
        ws.close(None).await.unwrap();guest.close(None).await.unwrap();
    }).catch_unwind().await;
    server.abort();
    ingest_server.abort();
    let _ = server.await;
    let _ = ingest_server.await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&control)
        .await
        .unwrap();
    control.close().await;
    assert!(storage.parent() == Some(std::env::temp_dir().as_path()));
    tokio::fs::remove_dir_all(storage).await.unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic)
    }
}
