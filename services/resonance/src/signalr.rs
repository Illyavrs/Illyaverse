//! Lazer hub endpoints implemented from published client interfaces.
//! Transport negotiation and authority checks are independent of client claims.
use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        DefaultBodyLimit, OriginalUri, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use resonance_protocol::signalr::{Codec, Incoming, Protocol, RECORD_SEPARATOR};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hub {
    Multiplayer,
    Spectator,
    Metadata,
}
impl Hub {
    fn from_path(path: &str) -> Result<Self, ApiError> {
        match path.trim_end_matches("/negotiate").rsplit('/').next() {
            Some("multiplayer") => Ok(Self::Multiplayer),
            Some("spectator") => Ok(Self::Spectator),
            Some("metadata") => Ok(Self::Metadata),
            _ => Err(ApiError(StatusCode::NOT_FOUND, "unknown_hub")),
        }
    }
}
#[derive(Clone)]
enum Outbound {
    Raw(Vec<u8>),
    Invocation(&'static str, Vec<Value>),
    Completion(String, Result<Option<Value>, &'static str>),
    Close(&'static str),
}
struct TransportState {
    codec: Option<Codec>,
    handshake: Vec<u8>,
}
struct Connection {
    key: String,
    user: Account,
    hub: Hub,
    headers: HeaderMap,
    created: Instant,
    active: AtomicBool,
    state: Mutex<TransportState>,
    sender: mpsc::Sender<Outbound>,
    receiver: Mutex<Option<mpsc::Receiver<Outbound>>>,
    presence: AtomicBool,
    room_watches: Mutex<HashSet<i64>>,
    spectator_watches: Mutex<HashSet<i64>>,
}
#[derive(Clone)]
struct Playing {
    state: Value,
    token: Option<i64>,
    connection_key: String,
}
#[derive(Clone)]
struct EndedPlay {
    user_id: i64,
    connection_key: String,
    registered: Instant,
}
pub struct Runtime {
    connections: Mutex<HashMap<String, Arc<Connection>>>,
    playing: Mutex<HashMap<i64, Playing>>,
    presence: Mutex<HashMap<i64, Value>>,
    ended_plays: Mutex<HashMap<i64, EndedPlay>>,
    score_event_dispatch: Mutex<()>,
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            playing: Mutex::new(HashMap::new()),
            presence: Mutex::new(HashMap::new()),
            ended_plays: Mutex::new(HashMap::new()),
            score_event_dispatch: Mutex::new(()),
        }
    }
    /// An interrupted process cannot honestly resume a live match or spectator stream.
    /// Preserve the room history while releasing memberships and closing those rooms.
    pub async fn recover(pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        // Process restart aborts unfinished rounds, while completed histories
        // retain their submission windows and immutable participant identities.
        sqlx::query("UPDATE lazer_room_rounds SET status='aborted',finished_at=COALESCE(finished_at,now()) WHERE status IN('loading','playing')").execute(&mut *tx).await?;
        sqlx::query("UPDATE lazer_room_round_participants p SET aborted_at=COALESCE(p.aborted_at,now()) FROM lazer_room_rounds r WHERE r.id=p.round_id AND r.status='aborted'").execute(&mut *tx).await?;
        sqlx::query("UPDATE lazer_score_tokens t SET revoked_at=COALESCE(t.revoked_at,now()),revoke_reason=COALESCE(t.revoke_reason,'server_restart'),processing_status='revoked' FROM lazer_room_rounds r WHERE t.scope='room' AND r.id=t.round_id AND r.status='aborted' AND t.processing_status<>'processed'").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM lazer_channel_members m USING channels c WHERE m.channel_id=c.id AND c.required_permission='lazer.room'").execute(&mut *tx).await?;
        sqlx::query("DELETE FROM lazer_room_members")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE lazer_rooms SET state=3,revision=revision+1,updated_at=now() WHERE state<>3",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }
    async fn create(
        &self,
        user: Account,
        hub: Hub,
        headers: HeaderMap,
    ) -> Result<(String, Arc<Connection>), ApiError> {
        let mut connections = self.connections.lock().await;
        connections.retain(|_, c| {
            c.active.load(Ordering::Acquire) || c.created.elapsed() < Duration::from_secs(30)
        });
        if connections.len() >= 1024 {
            return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "hub_capacity"));
        }
        if connections
            .values()
            .filter(|c| c.user.id == user.id)
            .count()
            >= 12
        {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_connections",
            ));
        }
        let token = common::secret();
        let key = common::token_hash(&token);
        let (sender, receiver) = mpsc::channel(64);
        let connection = Arc::new(Connection {
            key: key.clone(),
            user,
            hub,
            headers,
            created: Instant::now(),
            active: AtomicBool::new(false),
            state: Mutex::new(TransportState {
                codec: None,
                handshake: Vec::new(),
            }),
            sender,
            receiver: Mutex::new(Some(receiver)),
            presence: AtomicBool::new(false),
            room_watches: Mutex::new(HashSet::new()),
            spectator_watches: Mutex::new(HashSet::new()),
        });
        connections.insert(key, connection.clone());
        Ok((token, connection))
    }
    async fn targets(&self, hub: Hub, users: &[i64]) -> Vec<Arc<Connection>> {
        self.connections
            .lock()
            .await
            .values()
            .filter(|c| {
                c.hub == hub && c.active.load(Ordering::Acquire) && users.contains(&c.user.id)
            })
            .cloned()
            .collect()
    }
    async fn emit(&self, hub: Hub, users: &[i64], method: &'static str, args: Vec<Value>) {
        for c in self.targets(hub, users).await {
            send(&c, Outbound::Invocation(method, args.clone()));
        }
    }
}

pub fn router() -> Router<AppState> {
    let mut router = Router::new();
    for prefix in ["", "/signalr"] {
        for hub in ["multiplayer", "spectator", "metadata"] {
            router = router
                .route(&format!("{prefix}/{hub}/negotiate"), post(negotiate))
                .route(
                    &format!("{prefix}/{hub}"),
                    get(connect).post(send_http).delete(close_http),
                );
        }
    }
    router
        .route("/api/v2/rooms", get(rooms))
        .route("/api/v2/rooms/{id}", get(room_detail))
        .layer(DefaultBodyLimit::max(262_144))
}
#[derive(Default, Deserialize)]
struct TransportQuery {
    id: Option<String>,
    access_token: Option<String>,
    #[serde(rename = "negotiateVersion")]
    negotiate_version: Option<u8>,
}
async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
    query: &TransportQuery,
) -> Result<(Account, HeaderMap), ApiError> {
    let mut headers = headers.clone();
    if !headers.contains_key("authorization") {
        let token = query
            .access_token
            .as_ref()
            .filter(|v| !v.is_empty() && v.len() <= 4096)
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing_access_token"))?;
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid_access_token"))?,
        );
    }
    // Browser origins are exact, including scheme/port. Native clients send no Origin.
    if let Some(origin) = headers.get("origin") {
        let expected =
            std::env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
        if origin.to_str().ok() != Some(expected.as_str()) {
            return Err(ApiError(StatusCode::FORBIDDEN, "origin_not_allowed"));
        }
    }
    let user = common::bearer(state, &headers).await?;
    if !user.can("chat.send") {
        return Err(ApiError(StatusCode::FORBIDDEN, "hub_permission_required"));
    }
    Ok((user, headers))
}
async fn authorized(state: &AppState, connection: &Connection) -> bool {
    common::bearer(state, &connection.headers)
        .await
        .is_ok_and(|user| user.can("chat.send"))
}

#[derive(Default, Deserialize)]
struct RoomQuery {
    search: Option<String>,
    limit: Option<i64>,
}
async fn rooms(
    State(state): State<AppState>,
    Query(query): Query<RoomQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let search = query.search.unwrap_or_default();
    if search.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search_too_long"));
    }
    let rows=sqlx::query("SELECT r.* FROM lazer_rooms r JOIN users u ON u.id=r.host_id WHERE r.state<>3 AND NOT u.is_restricted AND r.settings->>'name' ILIKE $2 AND NOT EXISTS(SELECT 1 FROM user_relationships b WHERE b.relation_type='block' AND ((b.user_id=$1 AND b.target_id=r.host_id)OR(b.target_id=$1 AND b.user_id=r.host_id))) ORDER BY r.updated_at DESC LIMIT $3")
        .bind(user.id).bind(format!("%{search}%")).bind(query.limit.unwrap_or(50).clamp(1,100)).fetch_all(&state.pool).await?;
    let mut result = Vec::new();
    for row in rows {
        result.push(rest_room(&state, &user, &row).await?);
    }
    Ok(Json(json!(result)))
}
async fn room_detail(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let row = sqlx::query("SELECT * FROM lazer_rooms WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "room_not_found"))?;
    if row.get::<i16, _>("state") == 3
        && !crate::room_results::room_history_authorized(&state, &user, id).await?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "room_not_found"));
    }
    if blocked(&state, user.id, row.get("host_id"))
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database_error"))?
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "room_not_found"));
    }
    Ok(Json(rest_room(&state, &user, &row).await?))
}
async fn rest_room(
    state: &AppState,
    viewer: &Account,
    row: &sqlx::postgres::PgRow,
) -> Result<Value, ApiError> {
    let room: i64 = row.get("id");
    let settings: Value = row.get("settings");
    let playlist: Value = row.get("playlist");
    let host = common::account(state, row.get("host_id")).await?;
    let members: Vec<i64> = sqlx::query_scalar(
        "SELECT user_id FROM lazer_room_members WHERE room_id=$1 ORDER BY joined_at LIMIT 16",
    )
    .bind(room)
    .fetch_all(&state.pool)
    .await?;
    let mut recent = Vec::new();
    for member in &members {
        if let Ok(account) = common::account(state, *member).await {
            recent.push(crate::lazer::user_json(&account));
        }
    }
    let mut items = Vec::new();
    let mut stars = Vec::<f64>::new();
    let mut rulesets = HashSet::new();
    for item in playlist.as_array().into_iter().flatten() {
        let map = sqlx::query(
            "SELECT * FROM beatmaps WHERE id=$1 OR official_id=$1 ORDER BY (official_id=$1) DESC LIMIT 1",
        )
        .bind(item["beatmapID"].as_i64().unwrap_or(0))
        .fetch_optional(&state.pool)
        .await?;
        let Some(map) = map else {
            continue;
        };
        let id = item["beatmapID"].clone();
        let mode: usize = map.get::<i16, _>("mode") as usize;
        let rating = map.get::<f64, _>("stars");
        stars.push(rating);
        rulesets.insert(item["rulesetID"].as_i64().unwrap_or(0));
        let beatmap = json!({"id":id,"beatmapset_id":map.get::<Option<i64>,_>("official_set_id").unwrap_or(map.get("id")),"checksum":map.get::<String,_>("md5"),"version":map.get::<String,_>("version"),"mode":(["osu","taiko","fruits","mania"][mode]),"mode_int":mode,"difficulty_rating":rating,"bpm":map.get::<f64,_>("bpm"),"total_length":map.get::<i32,_>("length_seconds"),"hit_length":map.get::<i32,_>("length_seconds"),"max_combo":map.get::<i32,_>("max_combo"),"status":map.get::<String,_>("status"),"beatmapset":{"id":map.get::<Option<i64>,_>("official_set_id").unwrap_or(map.get("id")),"title":map.get::<String,_>("title"),"artist":map.get::<String,_>("artist"),"creator":map.get::<String,_>("creator"),"covers":{},"status":map.get::<String,_>("status")}});
        let attempts = if let Some(id) = item["id"].as_i64() {
            if crate::room_results::item_history_authorized(state, viewer, room, id).await? {
                crate::room_results::item_attempts(state, viewer, room, id).await?
            } else {
                json!({"attempts":0,"completed":false,"passed":false})
            }
        } else {
            json!({"attempts":0,"completed":false,"passed":false})
        };
        items.push(json!({"id":item["id"],"owner_id":item["ownerID"],"ruleset_id":item["rulesetID"],"expired":item["expired"],"playlist_order":item["playlistOrder"],"played_at":item["playedAt"],"allowed_mods":item["allowedMods"],"required_mods":item["requiredMods"],"freestyle":item["freestyle"],"beatmap":beatmap,"attempts":attempts["attempts"],"completed":attempts["completed"],"passed":attempts["passed"]}));
    }
    let current = items
        .iter()
        .find(|i| i["id"] == settings["playlistItemId"])
        .cloned();
    let active = items.iter().filter(|i| i["expired"] == false).count();
    let current_user_score =
        if crate::room_results::room_history_authorized(state, viewer, room).await? {
            crate::room_results::room_summary(state, viewer, room).await?
        } else {
            Value::Null
        };
    Ok(
        json!({"id":room,"name":settings["name"],"description":"","has_password":settings["password"].as_str().is_some_and(|v|!v.is_empty()),"password":null,"host":crate::lazer::user_json(&host),"category":"normal","type":"head_to_head","queue_mode":(["host_only","all_players","all_players_round_robin"][settings["queueMode"].as_u64().unwrap_or(0).min(2)as usize]),"auto_start_duration":0,"auto_skip":settings["autoSkip"],"max_participants":settings["maxParticipants"],"participant_count":members.len(),"recent_participants":recent,"starts_at":row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"ends_at":null,"playlist":items,"playlist_item_stats":{"count_active":active,"count_total":items.len(),"ruleset_ids":rulesets},"difficulty_range":{"min":stars.iter().cloned().reduce(f64::min),"max":stars.iter().cloned().reduce(f64::max)},"current_playlist_item":current,"channel_id":row.get::<Option<i64>,_>("channel_id").unwrap_or(0),"status":if row.get::<i16,_>("state")==2{"playing"}else{"idle"},"pinned":false,"current_user_score":current_user_score}),
    )
}
async fn negotiate(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<TransportQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let hub = Hub::from_path(uri.path())?;
    let (user, headers) = authenticate(&state, &headers, &query).await?;
    common::rate_limit(&state, format!("hub-negotiate:{}", user.id), 30).await?;
    let (token, _) = state.signalr.create(user, hub, headers).await?;
    Ok(Json(
        json!({"negotiateVersion":query.negotiate_version.unwrap_or(1).min(1),"connectionId":token,"connectionToken":token,"availableTransports":[{"transport":"WebSockets","transferFormats":["Text","Binary"]},{"transport":"ServerSentEvents","transferFormats":["Text"]}]}),
    ))
}
async fn lookup(
    state: &AppState,
    hub: Hub,
    query: &TransportQuery,
    user: &Account,
) -> Result<Arc<Connection>, ApiError> {
    let id = query
        .id
        .as_ref()
        .filter(|v| v.len() <= 128)
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "missing_connection_id"))?;
    let connection = state
        .signalr
        .connections
        .lock()
        .await
        .get(&common::token_hash(id))
        .cloned()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "connection_expired"))?;
    if connection.user.id != user.id || connection.hub != hub {
        return Err(ApiError(StatusCode::FORBIDDEN, "connection_owner_mismatch"));
    }
    if !connection.active.load(Ordering::Acquire)
        && connection.created.elapsed() > Duration::from_secs(30)
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "connection_expired"));
    }
    Ok(connection)
}
async fn connect(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<TransportQuery>,
    headers: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let ws = ws.ok();
    let hub = Hub::from_path(uri.path())?;
    let (user, auth) = authenticate(&state, &headers, &query).await?;
    let connection = if query.id.is_some() {
        lookup(&state, hub, &query, &user).await?
    } else if ws.is_some() {
        state.signalr.create(user, hub, auth).await?.1
    } else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "sse_requires_negotiate"));
    };
    if connection.active.swap(true, Ordering::AcqRel) {
        return Err(ApiError(StatusCode::CONFLICT, "connection_already_active"));
    }
    let receiver = connection
        .receiver
        .lock()
        .await
        .take()
        .ok_or(ApiError(StatusCode::CONFLICT, "connection_already_active"))?;
    if let Some(ws) = ws {
        return Ok(ws
            .max_message_size(262_144)
            .max_frame_size(262_144)
            .on_upgrade(move |socket| websocket(state, connection, receiver, socket))
            .into_response());
    }
    if !headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"))
    {
        connection.active.store(false, Ordering::Release);
        return Err(ApiError(StatusCode::BAD_REQUEST, "unsupported_transport"));
    }
    let lease = Lease {
        state: state.clone(),
        connection: connection.clone(),
    };
    let stream = futures_util::stream::unfold(
        (
            receiver,
            lease,
            tokio::time::interval(Duration::from_secs(10)),
        ),
        |(mut receiver, lease, mut timer)| async move {
            loop {
                let outbound = tokio::select! { value = receiver.recv() => value?, _ = timer.tick() => {
                    if !lease.connection.active.load(Ordering::Acquire) || !authorized(&lease.state,&lease.connection).await { return None; }
                    let stream = lease.connection.state.lock().await;
                    if stream.codec.is_none() { if lease.connection.created.elapsed() > Duration::from_secs(10) { return None; } continue; }
                    Outbound::Raw(stream.codec.as_ref()?.ping())
                }};
                let bytes = encoded(&lease.connection, outbound).await.ok()?;
                let data = String::from_utf8(bytes).ok()?;
                return Some((
                    Ok::<_, Infallible>(Event::default().data(data)),
                    (receiver, lease, timer),
                ));
            }
        },
    );
    let stream = futures_util::stream::once(async {
        Ok::<_, Infallible>(Event::default().comment("connected"))
    })
    .chain(stream);
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}
struct Lease {
    state: AppState,
    connection: Arc<Connection>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        let state = self.state.clone();
        let connection = self.connection.clone();
        tokio::spawn(async move {
            disconnect(&state, &connection).await;
        });
    }
}
async fn websocket(
    state: AppState,
    connection: Arc<Connection>,
    mut receiver: mpsc::Receiver<Outbound>,
    socket: WebSocket,
) {
    let _lease = Lease {
        state: state.clone(),
        connection: connection.clone(),
    };
    let (mut output, mut input) = socket.split();
    let mut timer = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            incoming=input.next()=>match incoming {
                Some(Ok(Message::Text(text)))=>if process(&state,&connection,text.as_bytes(),false).await.is_err(){break;},
                Some(Ok(Message::Binary(bytes)))=>if process(&state,&connection,&bytes,false).await.is_err(){break;},
                Some(Ok(Message::Ping(bytes)))=>if !tokio::time::timeout(Duration::from_secs(10),output.send(Message::Pong(bytes))).await.is_ok_and(|sent|sent.is_ok()){break;},
                Some(Ok(Message::Pong(_)))=>{},_=>break,
            },
            outbound=receiver.recv()=>{
                let Some(outbound)=outbound else {break;}; let closing=matches!(outbound,Outbound::Close(_));let handshake=matches!(&outbound,Outbound::Raw(bytes) if bytes==b"{}\x1e");
                let Ok(bytes)=encoded(&connection,outbound).await else {break;};
                let binary=connection.state.lock().await.codec.as_ref().is_some_and(|c|c.protocol()==Protocol::MessagePack) && !handshake;
                let message=if binary {Message::Binary(bytes.into())} else {match String::from_utf8(bytes){Ok(text)=>Message::Text(text.into()),Err(_)=>break}};
                if !tokio::time::timeout(Duration::from_secs(10),output.send(message)).await.is_ok_and(|sent|sent.is_ok())||closing{break;}
            },
            _=timer.tick()=>{
                if !connection.active.load(Ordering::Acquire)||!authorized(&state,&connection).await{break;}
                let transport=connection.state.lock().await;
                if let Some(codec)=&transport.codec {send(&connection,Outbound::Raw(codec.ping()));}
                else if connection.created.elapsed()>Duration::from_secs(10){break;}
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), output.send(Message::Close(None))).await;
}
async fn send_http(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<TransportQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let hub = Hub::from_path(uri.path())?;
    let (user, _) = authenticate(&state, &headers, &query).await?;
    let connection = lookup(&state, hub, &query, &user).await?;
    if !connection.active.load(Ordering::Acquire) {
        return Err(ApiError(StatusCode::CONFLICT, "connection_not_active"));
    }
    process(&state, &connection, &body, true)
        .await
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_hub_payload"))?;
    Ok(StatusCode::OK)
}
async fn close_http(
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
    Query(query): Query<TransportQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let hub = Hub::from_path(uri.path())?;
    let (user, _) = authenticate(&state, &headers, &query).await?;
    let connection = lookup(&state, hub, &query, &user).await?;
    send(&connection, Outbound::Close("client_closed"));
    disconnect(&state, &connection).await;
    Ok(StatusCode::ACCEPTED)
}
fn send(connection: &Connection, outbound: Outbound) {
    if connection.sender.try_send(outbound).is_err() {
        connection.active.store(false, Ordering::Release);
    }
}
async fn encoded(connection: &Connection, outbound: Outbound) -> Result<Vec<u8>, ()> {
    if let Outbound::Raw(bytes) = outbound {
        return Ok(bytes);
    }
    let transport = connection.state.lock().await;
    let codec = transport.codec.as_ref().ok_or(())?;
    let binary = codec.protocol() == Protocol::MessagePack;
    match outbound {
        Outbound::Invocation(target, args) => codec
            .invocation(target, wire_arguments(target, args, binary))
            .map_err(|_| ()),
        Outbound::Completion(id, result) => codec
            .completion(
                &id,
                result.map(|result| result.map(|v| wire_result(v, binary))),
            )
            .map_err(|_| ()),
        Outbound::Close(reason) => Ok(codec.close(reason)),
        Outbound::Raw(_) => unreachable!(),
    }
}
async fn process(
    state: &AppState,
    connection: &Arc<Connection>,
    bytes: &[u8],
    text_only: bool,
) -> Result<(), ()> {
    let records = {
        let mut transport = connection.state.lock().await;
        let mut remainder = bytes;
        if transport.codec.is_none() {
            transport.handshake.extend_from_slice(bytes);
            if transport.handshake.len() > 263_168 {
                return Err(());
            }
            let Some(end) = transport
                .handshake
                .iter()
                .position(|b| *b == RECORD_SEPARATOR)
            else {
                if transport.handshake.len() > 1024 {
                    return Err(());
                }
                return Ok(());
            };
            let protocol = resonance_protocol::signalr::handshake(&transport.handshake[..end])
                .map_err(|_| ())?;
            if text_only && protocol != Protocol::Json {
                send(
                    connection,
                    Outbound::Raw(b"{\"error\":\"SSE requires the JSON protocol\"}\x1e".to_vec()),
                );
                return Err(());
            }
            let leftover = transport.handshake[end + 1..].to_vec();
            transport.handshake.clear();
            transport.codec = Some(Codec::new(protocol));
            send(connection, Outbound::Raw(b"{}\x1e".to_vec()));
            let records = transport
                .codec
                .as_mut()
                .unwrap()
                .feed(&leftover)
                .map_err(|_| ())?;
            remainder = &[];
            if !records.is_empty() {
                drop(transport);
                return dispatch_records(state, connection, records).await;
            }
        }
        transport
            .codec
            .as_mut()
            .unwrap()
            .feed(remainder)
            .map_err(|_| ())?
    };
    dispatch_records(state, connection, records).await
}
async fn dispatch_records(
    state: &AppState,
    connection: &Arc<Connection>,
    records: Vec<Incoming>,
) -> Result<(), ()> {
    for record in records {
        match record {
            Incoming::Ping => {}
            Incoming::Close => return Err(()),
            Incoming::Invocation {
                id,
                target,
                arguments,
            } => {
                let result =
                    if common::rate_limit(state, format!("hub-call:{}", connection.user.id), 1200)
                        .await
                        .is_err()
                    {
                        Err("rate_limited")
                    } else if !authorized(state, connection).await {
                        Err("session_revoked")
                    } else {
                        dispatch(state, connection, &target, &arguments).await
                    };
                if let Some(id) = id {
                    send(connection, Outbound::Completion(id, result));
                } else if let Err(error) = result {
                    send(connection, Outbound::Close(error));
                    return Err(());
                }
            }
        }
    }
    Ok(())
}
async fn disconnect(state: &AppState, connection: &Arc<Connection>) {
    connection.active.store(false, Ordering::Release);
    state
        .signalr
        .connections
        .lock()
        .await
        .remove(&connection.key);
    state
        .signalr
        .ended_plays
        .lock()
        .await
        .retain(|_, ended| ended.connection_key != connection.key);
    let another = state.signalr.connections.lock().await.values().any(|c| {
        c.hub == connection.hub
            && c.user.id == connection.user.id
            && c.active.load(Ordering::Acquire)
    });
    if another {
        return;
    }
    match connection.hub {
        Hub::Multiplayer => {
            let _ = leave_room(
                state,
                connection.user.id,
                None,
                RoomWriteAuth::TrustedDisconnect,
            )
            .await;
        }
        Hub::Spectator => {
            let targets = connection.spectator_watches.lock().await.clone();
            for target in targets {
                state
                    .signalr
                    .emit(
                        Hub::Spectator,
                        &[target],
                        "UserEndedWatching",
                        vec![json!(connection.user.id)],
                    )
                    .await;
            }
            if let Some(mut playing) = state
                .signalr
                .playing
                .lock()
                .await
                .remove(&connection.user.id)
            {
                playing.state["state"] = json!(5);
                broadcast_spectator(
                    state,
                    connection.user.id,
                    "UserFinishedPlaying",
                    playing.state,
                )
                .await;
            }
        }
        Hub::Metadata => {
            state
                .signalr
                .presence
                .lock()
                .await
                .remove(&connection.user.id);
            broadcast_presence(state, connection.user.id, Value::Null).await;
        }
    }
}

type HubResult = Result<Option<Value>, &'static str>;
async fn dispatch(
    state: &AppState,
    c: &Arc<Connection>,
    target: &str,
    args: &[Value],
) -> HubResult {
    match c.hub {
        Hub::Multiplayer => multiplayer(state, c, target, args).await,
        Hub::Spectator => spectator(state, c, target, args).await,
        Hub::Metadata => metadata(state, c, target, args).await,
    }
}
fn field<'a>(value: &'a Value, name: &str, index: usize) -> &'a Value {
    value
        .get(name)
        .or_else(|| value.get(index))
        .unwrap_or(&Value::Null)
}
fn integer(value: &Value) -> Result<i64, &'static str> {
    value.as_i64().ok_or("invalid_integer")
}
fn argument(args: &[Value], index: usize) -> Result<&Value, &'static str> {
    args.get(index).ok_or("missing_argument")
}
fn db(_: sqlx::Error) -> &'static str {
    "database_error"
}
fn text(value: &Value, max: usize) -> Result<String, &'static str> {
    value
        .as_str()
        .filter(|v| v.len() <= max && !v.contains('\0'))
        .map(str::to_owned)
        .ok_or("invalid_text")
}
fn mods(value: &Value) -> Result<Value, &'static str> {
    let values = value
        .as_array()
        .filter(|v| v.len() <= 16)
        .ok_or("invalid_mods")?;
    let mut result = Vec::new();
    for value in values {
        let acronym = text(field(value, "acronym", 0), 8)?;
        if acronym.is_empty()
            || !acronym
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Err("invalid_mods");
        }
        let settings = if field(value, "settings", 1).is_null() {
            json!({})
        } else {
            field(value, "settings", 1).clone()
        };
        if !settings.is_object()
            || settings.as_object().unwrap().len() > 16
            || settings.as_object().unwrap().values().any(|v| {
                !(v.is_number() || v.is_boolean() || v.as_str().is_some_and(|v| v.len() <= 256))
            })
        {
            return Err("invalid_mod_settings");
        }
        result.push(json!({"acronym":acronym,"settings":settings}));
    }
    Ok(json!(result))
}
fn room_settings(value: &Value) -> Result<Value, &'static str> {
    let name = text(field(value, "name", 0), 100)?;
    if name.trim().is_empty() {
        return Err("invalid_room_name");
    }
    let password = text(field(value, "password", 2), 64)?;
    let match_type = integer(field(value, "matchType", 3))?;
    if match_type != 1 {
        return Err("unsupported_match_type");
    }
    let queue_mode = integer(field(value, "queueMode", 4))?;
    if !(0..=2).contains(&queue_mode) {
        return Err("invalid_queue_mode");
    }
    let max = field(value, "maxParticipants", 7);
    if !max.is_null() && !max.as_u64().is_some_and(|n| (2..=16).contains(&n)) {
        return Err("invalid_max_participants");
    }
    let duration = field(value, "autoStartDuration", 5);
    if !duration.is_null() && duration != &json!(0) && duration != &json!("00:00:00") {
        return Err("auto_start_not_supported");
    }
    Ok(
        json!({"name":name,"playlistItemId":field(value,"playlistItemId",1).as_i64().unwrap_or(0),"password":password,"matchType":1,"queueMode":queue_mode,"autoStartDuration":0,"autoSkip":field(value,"autoSkip",6).as_bool().unwrap_or(false),"maxParticipants":max}),
    )
}
async fn playlist_item(
    tx: &mut Transaction<'_, Postgres>,
    value: &Value,
    id: i64,
    owner: i64,
) -> Result<Value, &'static str> {
    let beatmap_id = integer(field(value, "beatmapID", 2))?;
    let ruleset = integer(field(value, "rulesetID", 4))?;
    if !(0..=3).contains(&ruleset) {
        return Err("invalid_ruleset");
    }
    let checksum = text(field(value, "beatmapChecksum", 3), 32)?;
    let row = sqlx::query("SELECT md5,mode,stars FROM beatmaps WHERE id=$1 OR official_id=$1 ORDER BY (official_id=$1) DESC LIMIT 1")
        .bind(beatmap_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db)?.ok_or("beatmap_not_found")?;
    let mapmode: i16 = row.get("mode");
    if mapmode != 0 && i64::from(mapmode) != ruleset {
        return Err("invalid_ruleset_conversion");
    }
    let actual: String = row.get("md5");
    if checksum != actual {
        return Err("beatmap_checksum_mismatch");
    }
    let required = mods(field(value, "requiredMods", 5))?;
    crate::room_rounds::canonical_mods(ruleset as u8, &required).map_err(|e| e.1)?;
    let allowed = mods(field(value, "allowedMods", 6))?;
    let mut seen = HashSet::new();
    for modifier in allowed.as_array().unwrap() {
        if !seen.insert(modifier["acronym"].as_str().unwrap()) {
            return Err("invalid_mods");
        }
        crate::room_rounds::canonical_mods(ruleset as u8, &json!([modifier])).map_err(|e| e.1)?;
    }
    let rating = row.get::<f64, _>("stars");
    if !rating.is_finite() {
        return Err("invalid_star_rating");
    }
    Ok(
        json!({"id":id,"ownerID":owner,"beatmapID":beatmap_id,"beatmapChecksum":actual,"rulesetID":ruleset,"requiredMods":required,"allowedMods":allowed,"expired":false,"playlistOrder":id.min(65535),"playedAt":null,"starRating":rating,"freestyle":field(value,"freestyle",11).as_bool().unwrap_or(false)}),
    )
}

async fn room_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    room_id: i64,
) -> Result<Value, &'static str> {
    let row = sqlx::query("SELECT * FROM lazer_rooms WHERE id=$1")
        .bind(room_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db)?;
    let members =
        sqlx::query("SELECT * FROM lazer_room_members WHERE room_id=$1 ORDER BY joined_at,user_id")
            .bind(room_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db)?;
    let users: Vec<Value> = members.iter().map(member_json).collect();
    let host = row.get::<i64, _>("host_id");
    let mut settings: Value = row.get("settings");
    let ids: Vec<i64> = users.iter().filter_map(|v| v["userID"].as_i64()).collect();
    let slots = slots_for(&settings, &ids);
    settings.as_object_mut().unwrap().remove("_slots");
    if row.get::<i16, _>("state") != 3 {
        settings["playlistItemId"] = settings.get("playlistItemId").cloned().unwrap_or(json!(0));
    }
    Ok(
        json!({"roomID":room_id,"state":row.get::<i16,_>("state"),"settings":settings,"users":users,"host":users.iter().find(|u|u["userID"]==host).cloned(),"matchState":{"locked":row.get::<bool,_>("locked"),"slots":slots},"playlist":row.get::<Value,_>("playlist"),"activeCountdowns":[],"channelID":row.get::<Option<i64>,_>("channel_id").unwrap_or(0)}),
    )
}
fn member_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"userID":row.get::<i64,_>("user_id"),"state":row.get::<i16,_>("state"),"beatmapAvailability":row.get::<Value,_>("beatmap_availability"),"mods":row.get::<Value,_>("mods"),"matchState":null,"rulesetId":row.get::<Option<i16>,_>("ruleset_id"),"beatmapId":row.get::<Option<i64>,_>("beatmap_id"),"votedToSkipIntro":row.get::<bool,_>("voted_to_skip"),"role":0})
}
fn slots_for(settings: &Value, users: &[i64]) -> Value {
    let Some(max) = settings["maxParticipants"].as_u64() else {
        return Value::Null;
    };
    let max = max.min(16) as usize;
    let mut slots = vec![Value::Null; max];
    let mut placed = HashSet::new();
    if let Some(previous) = settings.get("_slots").and_then(Value::as_array) {
        for (index, value) in previous.iter().take(max).enumerate() {
            if let Some(id) = value.as_i64()
                && users.contains(&id)
                && placed.insert(id)
            {
                slots[index] = json!(id);
            }
        }
    }
    for id in users {
        if !placed.contains(id)
            && let Some(slot) = slots.iter_mut().find(|slot| slot.is_null())
        {
            *slot = json!(id);
            placed.insert(*id);
        }
    }
    json!(slots)
}
async fn room_members(
    tx: &mut Transaction<'_, Postgres>,
    room_id: i64,
) -> Result<Vec<i64>, &'static str> {
    sqlx::query_scalar("SELECT user_id FROM lazer_room_members WHERE room_id=$1 ORDER BY joined_at")
        .bind(room_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(db)
}
#[derive(Clone, Copy)]
enum RoomWriteAuth<'a> {
    Bearer { actor: i64, headers: &'a HeaderMap },
    TrustedDisconnect,
}

/// A room mutation may reference any current participant through an FK or the
/// frozen roster. Acquire users in the same ascending order as moderation and
/// social writes before revalidating the initiating bearer session.
async fn guard_room_actor(
    tx: &mut Transaction<'_, Postgres>,
    room: i64,
    actor: i64,
    headers: &HeaderMap,
) -> Result<(), &'static str> {
    sqlx::query("SELECT id FROM users WHERE id=$2 OR id IN(SELECT user_id FROM lazer_room_members WHERE room_id=$1) ORDER BY id FOR UPDATE")
        .bind(room).bind(actor).fetch_all(&mut **tx).await.map_err(db)?;
    let account = common::lazer_account_in_tx(tx, headers, actor)
        .await
        .map_err(|error| {
            if matches!(error.0, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                "session_revoked"
            } else {
                error.1
            }
        })?;
    if !account.can("chat.send") {
        return Err("session_revoked");
    }
    Ok(())
}

async fn locked_room<'state>(
    state: &'state AppState,
    user_id: i64,
    authorization: RoomWriteAuth<'_>,
) -> Result<(Transaction<'state, Postgres>, sqlx::postgres::PgRow), &'static str> {
    let mut tx = state.pool.begin().await.map_err(db)?;
    let row=sqlx::query("SELECT r.* FROM lazer_rooms r JOIN lazer_room_members m ON m.room_id=r.id WHERE m.user_id=$1 FOR UPDATE OF r").bind(user_id).fetch_optional(&mut *tx).await.map_err(db)?.ok_or("not_in_room")?;
    if let RoomWriteAuth::Bearer { actor, headers } = authorization {
        guard_room_actor(&mut tx, row.get("id"), actor, headers).await?;
    }
    Ok((tx, row))
}
fn host(row: &sqlx::postgres::PgRow, user_id: i64) -> Result<(), &'static str> {
    if row.get::<i64, _>("host_id") == user_id {
        Ok(())
    } else {
        Err("host_required")
    }
}
async fn bump(tx: &mut Transaction<'_, Postgres>, room_id: i64) -> Result<(), &'static str> {
    sqlx::query("UPDATE lazer_rooms SET revision=revision+1,updated_at=now() WHERE id=$1")
        .bind(room_id)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    Ok(())
}

async fn multiplayer(
    state: &AppState,
    c: &Arc<Connection>,
    target: &str,
    args: &[Value],
) -> HubResult {
    let uid = c.user.id;
    let authorization = RoomWriteAuth::Bearer {
        actor: uid,
        headers: &c.headers,
    };
    match target {
        "CreateRoom" => {
            common::rate_limit(state, format!("room-create:{uid}"), 10)
                .await
                .map_err(|_| "rate_limited")?;
            let input = argument(args, 0)?;
            let mut settings = room_settings(field(input, "settings", 2))?;
            let source = field(input, "playlist", 6)
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 100)
                .ok_or("playlist_required")?;
            let mut tx = state.pool.begin().await.map_err(db)?;
            let account = common::lazer_account_in_tx(&mut tx, &c.headers, uid)
                .await
                .map_err(|error| error.1)?;
            if !account.can("chat.send") {
                return Err("session_revoked");
            }
            let mut playlist = Vec::new();
            for (i, item) in source.iter().enumerate() {
                playlist.push(playlist_item(&mut tx, item, (i + 1) as i64, uid).await?);
            }
            settings["playlistItemId"] = json!(1);
            if sqlx::query_scalar::<_, i64>(
                "SELECT room_id FROM lazer_room_members WHERE user_id=$1",
            )
            .bind(uid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .is_some()
            {
                return Err("already_in_room");
            }
            let room: i64 = sqlx::query_scalar(
                "INSERT INTO lazer_rooms(host_id,settings,playlist,next_playlist_item_id) VALUES($1,$2,$3,$4) RETURNING id",
            )
            .bind(uid)
            .bind(&settings)
            .bind(json!(playlist))
            .bind(playlist.len() as i64 + 1)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            for item in &playlist {
                crate::room_rounds::record_playlist(&mut tx, room, item)
                    .await
                    .map_err(|e| e.1)?;
            }
            let channel:i64=sqlx::query_scalar("INSERT INTO channels(name,description,required_permission) VALUES($1,$2,'lazer.room') RETURNING id").bind(format!("multiplayer-{room}")).bind("Private lazer multiplayer room").fetch_one(&mut *tx).await.map_err(db)?;
            sqlx::query("UPDATE lazer_rooms SET channel_id=$2 WHERE id=$1")
                .bind(room)
                .bind(channel)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            sqlx::query("INSERT INTO lazer_room_members(room_id,user_id) VALUES($1,$2)")
                .bind(room)
                .bind(uid)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(uid).bind(channel).execute(&mut *tx).await.map_err(db)?;
            let result = room_snapshot(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            Ok(Some(result))
        }
        "JoinRoom" | "JoinRoomWithPassword" => {
            let room = integer(argument(args, 0)?)?;
            let password = if target == "JoinRoomWithPassword" {
                text(argument(args, 1)?, 64)?
            } else {
                String::new()
            };
            let mut tx = state.pool.begin().await.map_err(db)?;
            let row = sqlx::query("SELECT * FROM lazer_rooms WHERE id=$1 FOR UPDATE")
                .bind(room)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db)?
                .ok_or("room_not_found")?;
            guard_room_actor(&mut tx, room, uid, &c.headers).await?;
            if row.get::<i16, _>("state") != 0 {
                return Err("room_not_open");
            }
            let settings: Value = row.get("settings");
            if settings["password"].as_str() != Some(password.as_str()) {
                return Err("invalid_room_password");
            }
            let users = room_members(&mut tx, room).await?;
            if users.len() >= settings["maxParticipants"].as_u64().unwrap_or(16) as usize {
                return Err("room_full");
            }
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE relation_type='block' AND ((user_id=$1 AND target_id=ANY($2)) OR (target_id=$1 AND user_id=ANY($2))))").bind(uid).bind(&users).fetch_one(&mut *tx).await.map_err(db)?;
            if blocked {
                return Err("room_member_blocked");
            }
            if sqlx::query_scalar::<_, i64>(
                "SELECT room_id FROM lazer_room_members WHERE user_id=$1",
            )
            .bind(uid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .is_some()
            {
                return Err("already_in_room");
            }
            sqlx::query("INSERT INTO lazer_room_members(room_id,user_id) VALUES($1,$2)")
                .bind(room)
                .bind(uid)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            if let Some(channel) = row.get::<Option<i64>, _>("channel_id") {
                sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(uid).bind(channel).execute(&mut *tx).await.map_err(db)?;
            }
            bump(&mut tx, room).await?;
            let result = room_snapshot(&mut tx, room).await?;
            let new = result["users"]
                .as_array()
                .unwrap()
                .iter()
                .find(|u| u["userID"] == uid)
                .cloned()
                .unwrap();
            tx.commit().await.map_err(db)?;
            state
                .signalr
                .emit(Hub::Multiplayer, &users, "UserJoined", vec![new])
                .await;
            let mut everyone = users;
            everyone.push(uid);
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &everyone,
                    "MatchRoomStateChanged",
                    vec![result["matchState"].clone()],
                )
                .await;
            Ok(Some(result))
        }
        "LeaveRoom" => {
            leave_room(state, uid, None, authorization).await?;
            Ok(None)
        }
        "KickUser" => {
            let target = integer(argument(args, 0)?)?;
            let (tx, row) = locked_room(state, uid, authorization).await?;
            host(&row, uid)?;
            let room = row.get("id");
            drop(tx);
            leave_room(state, target, Some((room, uid)), authorization).await?;
            Ok(None)
        }
        "TransferHost" => {
            let next = integer(argument(args, 0)?)?;
            let (mut tx, row) = locked_room(state, uid, authorization).await?;
            host(&row, uid)?;
            let room = row.get("id");
            let users = room_members(&mut tx, room).await?;
            if !users.contains(&next) {
                return Err("user_not_in_room");
            }
            sqlx::query("UPDATE lazer_rooms SET host_id=$2 WHERE id=$1")
                .bind(room)
                .bind(next)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            bump(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            state
                .signalr
                .emit(Hub::Multiplayer, &users, "HostChanged", vec![json!(next)])
                .await;
            Ok(None)
        }
        "ChangeSettings" => {
            let mut settings = room_settings(argument(args, 0)?)?;
            let (mut tx, row) = locked_room(state, uid, authorization).await?;
            host(&row, uid)?;
            if row.get::<i16, _>("state") != 0 {
                return Err("room_busy");
            }
            let room = row.get("id");
            let playlist: Value = row.get("playlist");
            if !playlist.as_array().unwrap().iter().any(|item| {
                item["id"] == settings["playlistItemId"]
                    && !item["expired"].as_bool().unwrap_or(false)
            }) {
                return Err("playlist_item_not_found");
            }
            let users = room_members(&mut tx, room).await?;
            if settings["maxParticipants"]
                .as_u64()
                .is_some_and(|max| max < users.len() as u64)
            {
                return Err("max_below_member_count");
            }
            settings["_slots"] = row.get::<Value, _>("settings")["_slots"].clone();
            let slots = slots_for(&settings, &users);
            settings["_slots"] = slots.clone();
            let reset: Vec<i64> = sqlx::query_scalar(
                "SELECT user_id FROM lazer_room_members WHERE room_id=$1 AND state=1",
            )
            .bind(room)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
            sqlx::query("UPDATE lazer_rooms SET settings=$2 WHERE id=$1")
                .bind(room)
                .bind(&settings)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            sqlx::query("UPDATE lazer_room_members SET state=0 WHERE room_id=$1 AND state=1")
                .bind(room)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            bump(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            settings.as_object_mut().unwrap().remove("_slots");
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &users,
                    "MatchRoomStateChanged",
                    vec![json!({"locked":row.get::<bool,_>("locked"),"slots":slots})],
                )
                .await;
            state
                .signalr
                .emit(Hub::Multiplayer, &users, "SettingsChanged", vec![settings])
                .await;
            for user in &reset {
                state
                    .signalr
                    .emit(
                        Hub::Multiplayer,
                        &users,
                        "UserStateChanged",
                        vec![json!(user), json!(0)],
                    )
                    .await;
            }
            Ok(None)
        }
        "ChangeState" => change_state(state, uid, &c.headers, integer(argument(args, 0)?)?).await,
        "ChangeBeatmapAvailability" => {
            let input = argument(args, 0)?;
            let availability = integer(field(input, "state", 0))?;
            if !(0..=4).contains(&availability) {
                return Err("invalid_beatmap_availability");
            }
            let progress = field(input, "downloadProgress", 1);
            if !progress.is_null() && !progress.as_f64().is_some_and(|n| (0.0..=1.0).contains(&n)) {
                return Err("invalid_download_progress");
            }
            let value = json!({"state":availability,"downloadProgress":progress});
            let (mut tx, row) = locked_room(state, uid, authorization).await?;
            let room = row.get("id");
            let users = room_members(&mut tx, room).await?;
            sqlx::query("UPDATE lazer_room_members SET beatmap_availability=$2 WHERE user_id=$1")
                .bind(uid)
                .bind(&value)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            bump(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &users,
                    "UserBeatmapAvailabilityChanged",
                    vec![json!(uid), value],
                )
                .await;
            Ok(None)
        }
        "ChangeUserMods" => {
            let value = mods(argument(args, 0)?)?;
            let (mut tx, row) = locked_room(state, uid, authorization).await?;
            if row.get::<i16, _>("state") != 0 {
                return Err("room_busy");
            }
            let room = row.get("id");
            let users = room_members(&mut tx, room).await?;
            sqlx::query("UPDATE lazer_room_members SET mods=$2,state=CASE WHEN state=1 THEN 0 ELSE state END WHERE user_id=$1").bind(uid).bind(&value).execute(&mut *tx).await.map_err(db)?;
            bump(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &users,
                    "UserModsChanged",
                    vec![json!(uid), value],
                )
                .await;
            Ok(None)
        }
        "ChangeUserStyle" => {
            let beatmap = argument(args, 0)?.as_i64();
            let ruleset = argument(args, 1)?.as_i64();
            if ruleset.is_some_and(|r| !(0..=3).contains(&r)) {
                return Err("invalid_ruleset");
            }
            let (mut tx, row) = locked_room(state, uid, authorization).await?;
            if row.get::<i16, _>("state") != 0 {
                return Err("room_busy");
            }
            let room = row.get("id");
            let users = room_members(&mut tx, room).await?;
            sqlx::query("UPDATE lazer_room_members SET beatmap_id=$2,ruleset_id=$3,state=CASE WHEN state=1 THEN 0 ELSE state END WHERE user_id=$1").bind(uid).bind(beatmap).bind(ruleset.map(|r|r as i16)).execute(&mut *tx).await.map_err(db)?;
            bump(&mut tx, room).await?;
            tx.commit().await.map_err(db)?;
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &users,
                    "UserStyleChanged",
                    vec![json!(uid), json!(beatmap), json!(ruleset)],
                )
                .await;
            Ok(None)
        }
        "StartMatch" => start_match(state, uid, &c.headers).await,
        "AbortMatch" | "AbortGameplay" => {
            abort_match(state, uid, &c.headers, target == "AbortMatch").await
        }
        "AddPlaylistItem" | "EditPlaylistItem" | "RemovePlaylistItem" => {
            edit_playlist(state, uid, &c.headers, target, argument(args, 0)?).await
        }
        "InvitePlayer" => {
            let invited = integer(argument(args, 0)?)?;
            if invited == uid {
                return Err("cannot_invite_self");
            }
            common::account(state, invited)
                .await
                .map_err(|_| "user_not_found")?;
            if blocked(state, uid, invited).await? {
                return Err("user_blocked");
            }
            common::rate_limit(state, format!("room-invite:{uid}"), 20)
                .await
                .map_err(|_| "rate_limited")?;
            let (tx, row) = locked_room(state, uid, authorization).await?;
            let settings: Value = row.get("settings");
            let room: i64 = row.get("id");
            drop(tx);
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &[invited],
                    "Invited",
                    vec![json!(uid), json!(room), settings["password"].clone()],
                )
                .await;
            Ok(None)
        }
        "VoteToSkipIntro" => vote_skip(state, uid, &c.headers).await,
        "SendMatchRequest" => match_request(state, uid, &c.headers, argument(args, 0)?).await,
        _ => Err("unsupported_hub_method"),
    }
}

async fn leave_room(
    state: &AppState,
    uid: i64,
    kicked: Option<(i64, i64)>,
    authorization: RoomWriteAuth<'_>,
) -> Result<(), &'static str> {
    let (mut tx, row) = match locked_room(state, uid, authorization).await {
        Ok(value) => value,
        Err("not_in_room") if kicked.is_none() => return Ok(()),
        Err(e) => return Err(e),
    };
    let room: i64 = row.get("id");
    if let Some((expected, actor)) = kicked {
        if room != expected {
            return Err("user_not_in_room");
        }
        host(&row, actor)?;
        if actor == uid {
            return Err("cannot_kick_host");
        }
    }
    let before = room_members(&mut tx, room).await?;
    let departing = sqlx::query("SELECT * FROM lazer_room_members WHERE user_id=$1")
        .bind(uid)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let member = member_json(&departing);
    if row.get::<i16, _>("state") != 0
        && let Some(round) = row.get::<Option<i64>, _>("current_round_id")
    {
        crate::room_rounds::abort_round(&mut tx, round, "member_left")
            .await
            .map_err(|e| e.1)?;
    }
    sqlx::query("DELETE FROM lazer_room_members WHERE user_id=$1")
        .bind(uid)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(channel) = row.get::<Option<i64>, _>("channel_id") {
        sqlx::query("DELETE FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2")
            .bind(uid)
            .bind(channel)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    let remaining = room_members(&mut tx, room).await?;
    let matchstate = json!({"locked":row.get::<bool,_>("locked"),"slots":slots_for(&row.get::<Value,_>("settings"),&remaining)});
    let changed_host = row.get::<i64, _>("host_id") == uid && !remaining.is_empty();
    let aborted = row.get::<i16, _>("state") != 0;
    if remaining.is_empty() {
        sqlx::query("UPDATE lazer_rooms SET state=3 WHERE id=$1")
            .bind(room)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    } else {
        if changed_host {
            sqlx::query("UPDATE lazer_rooms SET host_id=$2 WHERE id=$1")
                .bind(room)
                .bind(remaining[0])
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        if aborted {
            sqlx::query("UPDATE lazer_rooms SET state=0 WHERE id=$1")
                .bind(room)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
            sqlx::query("UPDATE lazer_room_members SET state=0,voted_to_skip=false WHERE room_id=$1 AND state<>8").bind(room).execute(&mut *tx).await.map_err(db)?;
        }
    }
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(
            Hub::Multiplayer,
            &before,
            if kicked.is_some() {
                "UserKicked"
            } else {
                "UserLeft"
            },
            vec![member],
        )
        .await;
    if changed_host {
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &remaining,
                "HostChanged",
                vec![json!(remaining[0])],
            )
            .await;
    }
    if !remaining.is_empty() {
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &remaining,
                "MatchRoomStateChanged",
                vec![matchstate],
            )
            .await;
    }
    if aborted && !remaining.is_empty() {
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &remaining,
                "GameplayAborted",
                vec![json!(1)],
            )
            .await;
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &remaining,
                "RoomStateChanged",
                vec![json!(0)],
            )
            .await;
        for user in &remaining {
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &remaining,
                    "UserStateChanged",
                    vec![json!(user), json!(0)],
                )
                .await;
        }
    }
    Ok(())
}
fn valid_transition(room: i16, current: i16, next: i64) -> bool {
    if i64::from(current) == next {
        return true;
    }
    matches!(
        (room, current, next),
        (0, 0 | 1 | 7 | 8, 0 | 1 | 8) | (1, 2, 3) | (1, 3, 4) | (2, 5, 6)
    )
}
async fn change_state(state: &AppState, uid: i64, headers: &HeaderMap, next: i64) -> HubResult {
    if !(0..=8).contains(&next) {
        return Err("invalid_user_state");
    }
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    let room: i64 = row.get("id");
    let roomstate: i16 = row.get("state");
    let member = sqlx::query("SELECT * FROM lazer_room_members WHERE user_id=$1")
        .bind(uid)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let current: i16 = member.get("state");
    if !valid_transition(roomstate, current, next) {
        return Err("invalid_state_transition");
    }
    if next == 1 {
        let availability: Value = member.get("beatmap_availability");
        if availability["state"] != 4 {
            return Err("beatmap_not_available");
        }
    }
    sqlx::query("UPDATE lazer_room_members SET state=$2 WHERE user_id=$1")
        .bind(uid)
        .bind(next as i16)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let users = room_members(&mut tx, room).await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lazer_room_members WHERE room_id=$1 AND state IN(2,3)",
    )
    .bind(room)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM lazer_room_members WHERE room_id=$1 AND state=5")
            .bind(room)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    let start = roomstate == 1 && next == 4 && pending == 0;
    let complete = roomstate == 2 && next == 6 && active == 0;
    let mut playlist_change = None;
    let mut settings_change = None;
    if start {
        let round = row
            .get::<Option<i64>, _>("current_round_id")
            .ok_or("active_round_required")?;
        crate::room_rounds::mark_playing(&mut tx, round)
            .await
            .map_err(|e| e.1)?;
        sqlx::query("UPDATE lazer_rooms SET state=2 WHERE id=$1")
            .bind(room)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("UPDATE lazer_room_members SET state=5 WHERE room_id=$1 AND state=4")
            .bind(room)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    if complete {
        let round = row
            .get::<Option<i64>, _>("current_round_id")
            .ok_or("active_round_required")?;
        crate::room_rounds::mark_finished(&mut tx, round)
            .await
            .map_err(|e| e.1)?;
        let mut playlist: Value = row.get("playlist");
        let mut settings: Value = row.get("settings");
        let current = settings["playlistItemId"].as_i64().unwrap_or(0);
        if let Some(items) = playlist.as_array_mut() {
            if let Some(item) = items.iter_mut().find(|i| i["id"] == current) {
                item["expired"] = json!(true);
                playlist_change = Some(item.clone());
                crate::room_rounds::record_playlist(&mut tx, room, item)
                    .await
                    .map_err(|e| e.1)?;
            }
            if let Some(next) = items.iter().find(|i| i["expired"] == false) {
                settings["playlistItemId"] = next["id"].clone();
                settings_change = Some(settings.clone());
            }
        }
        sqlx::query("UPDATE lazer_rooms SET state=0,playlist=$2,settings=$3 WHERE id=$1")
            .bind(room)
            .bind(&playlist)
            .bind(&settings)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("UPDATE lazer_room_members SET state=7,voted_to_skip=false WHERE room_id=$1 AND state=6").bind(room).execute(&mut *tx).await.map_err(db)?;
    }
    bump(&mut tx, room).await?;
    let states: Vec<(i64, i16)> = sqlx::query_as(
        "SELECT user_id,state FROM lazer_room_members WHERE room_id=$1 ORDER BY joined_at",
    )
    .bind(room)
    .fetch_all(&mut *tx)
    .await
    .map_err(db)?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(
            Hub::Multiplayer,
            &users,
            "UserStateChanged",
            vec![json!(uid), json!(next)],
        )
        .await;
    if start {
        for (user, status) in &states {
            if *status == 5 {
                state
                    .signalr
                    .emit(
                        Hub::Multiplayer,
                        &users,
                        "UserStateChanged",
                        vec![json!(user), json!(5)],
                    )
                    .await;
            }
        }
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "RoomStateChanged", vec![json!(2)])
            .await;
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "GameplayStarted", vec![])
            .await;
    }
    if complete {
        for (user, status) in &states {
            if *status == 7 {
                state
                    .signalr
                    .emit(
                        Hub::Multiplayer,
                        &users,
                        "UserStateChanged",
                        vec![json!(user), json!(7)],
                    )
                    .await;
            }
        }
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "RoomStateChanged", vec![json!(0)])
            .await;
        if let Some(item) = playlist_change {
            state
                .signalr
                .emit(Hub::Multiplayer, &users, "PlaylistItemChanged", vec![item])
                .await;
        }
        if let Some(settings) = settings_change {
            state
                .signalr
                .emit(Hub::Multiplayer, &users, "SettingsChanged", vec![settings])
                .await;
        }
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "ResultsReady", vec![])
            .await;
    }
    Ok(None)
}
async fn start_match(state: &AppState, uid: i64, headers: &HeaderMap) -> HubResult {
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    host(&row, uid)?;
    if row.get::<i16, _>("state") != 0 {
        return Err("room_busy");
    }
    let room: i64 = row.get("id");
    let settings: Value = row.get("settings");
    let mut playlist: Value = row.get("playlist");
    let selected = playlist
        .as_array_mut()
        .and_then(|items| {
            items
                .iter_mut()
                .find(|i| i["id"] == settings["playlistItemId"] && i["expired"] == false)
        })
        .ok_or("playlist_item_not_found")?;
    selected["playedAt"] = json!(chrono::Utc::now());
    let selected = selected.clone();
    let ready:Vec<i64>=sqlx::query_scalar("SELECT user_id FROM lazer_room_members WHERE room_id=$1 AND state=1 AND beatmap_availability->>'state'='4'").bind(room).fetch_all(&mut *tx).await.map_err(db)?;
    if ready.is_empty() {
        return Err("no_ready_players");
    }
    let unready: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lazer_room_members WHERE room_id=$1 AND (state NOT IN(1,8) OR (state=1 AND beatmap_availability->>'state' IS DISTINCT FROM '4'))",
    )
    .bind(room)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if unready > 0 {
        return Err("players_not_ready");
    }
    if let Some(previous) = row.get::<Option<i64>, _>("current_round_id") {
        crate::room_rounds::abort_round(&mut tx, previous, "round_replaced")
            .await
            .map_err(|e| e.1)?;
    }
    crate::room_rounds::freeze_round(&mut tx, room, &selected, &ready)
        .await
        .map_err(|e| e.1)?;
    sqlx::query("UPDATE lazer_rooms SET state=1,playlist=$2 WHERE id=$1")
        .bind(room)
        .bind(&playlist)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    sqlx::query(
        "UPDATE lazer_room_members SET state=2,voted_to_skip=false WHERE room_id=$1 AND user_id=ANY($2)",
    )
    .bind(room)
    .bind(&ready)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    let users = room_members(&mut tx, room).await?;
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    for user in &ready {
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &users,
                "UserStateChanged",
                vec![json!(user), json!(2)],
            )
            .await;
    }
    state
        .signalr
        .emit(Hub::Multiplayer, &users, "RoomStateChanged", vec![json!(1)])
        .await;
    state
        .signalr
        .emit(Hub::Multiplayer, &ready, "LoadRequested", vec![])
        .await;
    Ok(None)
}
async fn abort_match(state: &AppState, uid: i64, headers: &HeaderMap, whole: bool) -> HubResult {
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    if whole {
        host(&row, uid)?;
    }
    let room: i64 = row.get("id");
    if row.get::<i16, _>("state") == 0 {
        return Err("no_active_gameplay");
    }
    let users = room_members(&mut tx, room).await?;
    if whole {
        let round = row
            .get::<Option<i64>, _>("current_round_id")
            .ok_or("active_round_required")?;
        crate::room_rounds::abort_round(&mut tx, round, "match_aborted")
            .await
            .map_err(|e| e.1)?;
        sqlx::query("UPDATE lazer_rooms SET state=0 WHERE id=$1")
            .bind(room)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        sqlx::query("UPDATE lazer_room_members SET state=0,voted_to_skip=false WHERE room_id=$1 AND state<>8").bind(room).execute(&mut *tx).await.map_err(db)?;
    } else {
        let round = row
            .get::<Option<i64>, _>("current_round_id")
            .ok_or("active_round_required")?;
        crate::room_rounds::abort_participant(&mut tx, round, uid, "gameplay_aborted")
            .await
            .map_err(|e| e.1)?;
        sqlx::query("UPDATE lazer_room_members SET state=8 WHERE user_id=$1")
            .bind(uid)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM lazer_room_members WHERE room_id=$1 AND state IN(2,3,4,5)",
        )
        .bind(room)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if remaining == 0 {
            let finished: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM lazer_room_members WHERE room_id=$1 AND state=6)",
            )
            .bind(room)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if finished {
                crate::room_rounds::mark_finished(&mut tx, round)
                    .await
                    .map_err(|e| e.1)?;
            } else {
                crate::room_rounds::abort_round(&mut tx, round, "all_players_aborted")
                    .await
                    .map_err(|e| e.1)?;
            }
            sqlx::query("UPDATE lazer_rooms SET state=0 WHERE id=$1")
                .bind(room)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
    }
    let open = sqlx::query_scalar::<_, i16>("SELECT state FROM lazer_rooms WHERE id=$1")
        .bind(room)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?
        == 0;
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(
            Hub::Multiplayer,
            if whole {
                &users
            } else {
                std::slice::from_ref(&uid)
            },
            "GameplayAborted",
            vec![json!(1)],
        )
        .await;
    if whole {
        for user in &users {
            state
                .signalr
                .emit(
                    Hub::Multiplayer,
                    &users,
                    "UserStateChanged",
                    vec![json!(user), json!(0)],
                )
                .await;
        }
    } else {
        state
            .signalr
            .emit(
                Hub::Multiplayer,
                &users,
                "UserStateChanged",
                vec![json!(uid), json!(8)],
            )
            .await;
    }
    if open {
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "RoomStateChanged", vec![json!(0)])
            .await;
    }
    Ok(None)
}
async fn edit_playlist(
    state: &AppState,
    uid: i64,
    headers: &HeaderMap,
    target: &str,
    value: &Value,
) -> HubResult {
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    let room: i64 = row.get("id");
    let mut settings: Value = row.get("settings");
    if row.get::<i16, _>("state") != 0 {
        return Err("room_busy");
    }
    if target != "AddPlaylistItem" || settings["queueMode"] == 0 {
        host(&row, uid)?;
    }
    let mut playlist: Vec<Value> = row
        .get::<Value, _>("playlist")
        .as_array()
        .cloned()
        .unwrap_or_default();
    let users = room_members(&mut tx, room).await?;
    let (callback, argument) = if target == "AddPlaylistItem" {
        if playlist.len() >= 100 {
            return Err("playlist_full");
        }
        let id: i64 = row.get("next_playlist_item_id");
        let next = id.checked_add(1).ok_or("playlist_id_exhausted")?;
        let item = playlist_item(&mut tx, value, id, uid).await?;
        crate::room_rounds::record_playlist(&mut tx, room, &item)
            .await
            .map_err(|e| e.1)?;
        sqlx::query("UPDATE lazer_rooms SET next_playlist_item_id=$2 WHERE id=$1")
            .bind(room)
            .bind(next)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        playlist.push(item.clone());
        if !playlist
            .iter()
            .any(|i| i["id"] == settings["playlistItemId"] && i["expired"] == false)
        {
            settings["playlistItemId"] = json!(id);
        }
        ("PlaylistItemAdded", item)
    } else {
        let id = if target == "RemovePlaylistItem" {
            integer(value)?
        } else {
            integer(field(value, "id", 0))?
        };
        let position = playlist
            .iter()
            .position(|v| v["id"] == id)
            .ok_or("playlist_item_not_found")?;
        if target == "RemovePlaylistItem" {
            if playlist.len() == 1 {
                return Err("playlist_cannot_be_empty");
            }
            playlist.remove(position);
            if settings["playlistItemId"] == id {
                settings["playlistItemId"] = playlist
                    .iter()
                    .find(|i| i["expired"] == false)
                    .unwrap_or(&playlist[0])["id"]
                    .clone();
            }
            ("PlaylistItemRemoved", json!(id))
        } else {
            let owner = playlist[position]["ownerID"].as_i64().unwrap_or(uid);
            let item = playlist_item(&mut tx, value, id, owner).await?;
            crate::room_rounds::record_playlist(&mut tx, room, &item)
                .await
                .map_err(|e| e.1)?;
            playlist[position] = item.clone();
            ("PlaylistItemChanged", item)
        }
    };
    sqlx::query("UPDATE lazer_rooms SET playlist=$2,settings=$3 WHERE id=$1")
        .bind(room)
        .bind(json!(playlist))
        .bind(&settings)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(Hub::Multiplayer, &users, callback, vec![argument])
        .await;
    state
        .signalr
        .emit(Hub::Multiplayer, &users, "SettingsChanged", vec![settings])
        .await;
    Ok(None)
}
async fn vote_skip(state: &AppState, uid: i64, headers: &HeaderMap) -> HubResult {
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    if row.get::<i16, _>("state") != 2 {
        return Err("gameplay_not_started");
    }
    let room: i64 = row.get("id");
    let memberstate: i16 =
        sqlx::query_scalar("SELECT state FROM lazer_room_members WHERE user_id=$1")
            .bind(uid)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
    if memberstate != 5 {
        return Err("not_playing");
    }
    sqlx::query("UPDATE lazer_room_members SET voted_to_skip=true WHERE user_id=$1")
        .bind(uid)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    let outstanding:i64=sqlx::query_scalar("SELECT count(*) FROM lazer_room_members WHERE room_id=$1 AND state=5 AND NOT voted_to_skip").bind(room).fetch_one(&mut *tx).await.map_err(db)?;
    let users = room_members(&mut tx, room).await?;
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(
            Hub::Multiplayer,
            &users,
            "UserVotedToSkipIntro",
            vec![json!(uid), json!(true)],
        )
        .await;
    if outstanding == 0 {
        state
            .signalr
            .emit(Hub::Multiplayer, &users, "VoteToSkipIntroPassed", vec![])
            .await;
    }
    Ok(None)
}
async fn match_request(
    state: &AppState,
    uid: i64,
    headers: &HeaderMap,
    input: &Value,
) -> HubResult {
    let array = input.as_array().ok_or("unsupported_match_request")?;
    let tag = array
        .first()
        .and_then(Value::as_i64)
        .ok_or("invalid_match_request")?;
    let payload = array.get(1).ok_or("invalid_match_request")?;
    if tag != 5 && tag != 7 {
        return Err("unsupported_match_request");
    }
    let (mut tx, row) = locked_room(
        state,
        uid,
        RoomWriteAuth::Bearer {
            actor: uid,
            headers,
        },
    )
    .await?;
    let room: i64 = row.get("id");
    let users = room_members(&mut tx, room).await?;
    let mut settings: Value = row.get("settings");
    let mut slots = slots_for(&settings, &users);
    let mut locked = row.get::<bool, _>("locked");
    if tag == 5 {
        host(&row, uid)?;
        locked = field(payload, "locked", 0)
            .as_bool()
            .ok_or("invalid_match_request")?;
        sqlx::query("UPDATE lazer_rooms SET locked=$2 WHERE id=$1")
            .bind(room)
            .bind(locked)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    } else {
        if locked {
            return Err("room_slots_locked");
        }
        if row.get::<i16, _>("state") != 0 {
            return Err("room_busy");
        }
        let slot = field(payload, "slotID", 0).as_u64().ok_or("invalid_slot")? as usize;
        let values = slots.as_array_mut().ok_or("room_slots_disabled")?;
        if slot >= values.len() {
            return Err("invalid_slot");
        }
        if !values[slot].is_null() && values[slot] != uid {
            return Err("slot_occupied");
        }
        for value in values.iter_mut() {
            if *value == uid {
                *value = Value::Null;
            }
        }
        values[slot] = json!(uid);
        settings["_slots"] = slots.clone();
        sqlx::query("UPDATE lazer_rooms SET settings=$2 WHERE id=$1")
            .bind(room)
            .bind(&settings)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    bump(&mut tx, room).await?;
    tx.commit().await.map_err(db)?;
    state
        .signalr
        .emit(
            Hub::Multiplayer,
            &users,
            "MatchRoomStateChanged",
            vec![json!({"locked":locked,"slots":slots})],
        )
        .await;
    Ok(None)
}

async fn blocked(state: &AppState, first: i64, second: i64) -> Result<bool, &'static str> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE relation_type='block' AND ((user_id=$1 AND target_id=$2) OR(user_id=$2 AND target_id=$1)))").bind(first).bind(second).fetch_one(&state.pool).await.map_err(db)
}
fn spectator_state(value: &Value) -> Result<Value, &'static str> {
    let beatmap = field(value, "beatmapID", 0);
    let ruleset = field(value, "rulesetID", 1);
    let status = integer(field(value, "state", 3))?;
    if !(0..=5).contains(&status)
        || (!beatmap.is_null() && beatmap.as_i64().is_none())
        || (!ruleset.is_null() && !ruleset.as_i64().is_some_and(|r| (0..=3).contains(&r)))
    {
        return Err("invalid_spectator_state");
    }
    let statistics = field(value, "maximumStatistics", 4);
    if !statistics.is_object()
        || statistics.as_object().unwrap().len() > 40
        || statistics
            .as_object()
            .unwrap()
            .values()
            .any(|v| v.as_u64().is_none_or(|n| n > 1_000_000))
    {
        return Err("invalid_statistics");
    }
    Ok(
        json!({"beatmapID":beatmap,"rulesetID":ruleset,"mods":mods(field(value,"mods",2))?,"state":status,"maximumStatistics":statistics}),
    )
}
fn frame_bundle(value: &Value) -> Result<Value, &'static str> {
    let header = field(value, "header", 0);
    let frames = field(value, "frames", 1)
        .as_array()
        .filter(|f| !f.is_empty() && f.len() <= 512)
        .ok_or("invalid_frames")?;
    let accuracy = field(header, "accuracy", 1)
        .as_f64()
        .filter(|n| (0.0..=1.0).contains(n))
        .ok_or("invalid_accuracy")?;
    let total = field(header, "totalScore", 0)
        .as_u64()
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or("invalid_score")?;
    let combo = field(header, "combo", 2)
        .as_u64()
        .filter(|v| *v <= 1_000_000)
        .ok_or("invalid_combo")?;
    let max = field(header, "maxCombo", 3)
        .as_u64()
        .filter(|v| *v >= combo && *v <= 1_000_000)
        .ok_or("invalid_combo")?;
    let statistics = field(header, "statistics", 4);
    if !statistics.is_object()
        || statistics.as_object().unwrap().len() > 40
        || statistics
            .as_object()
            .unwrap()
            .values()
            .any(|v| v.as_u64().is_none_or(|n| n > 1_000_000))
    {
        return Err("invalid_statistics");
    }
    // Replay frames remain data for viewers, never evidence that a score is verified.
    let mut normalized = Vec::new();
    for frame in frames {
        let time = field(frame, "time", 0)
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 86_400_000.0)
            .ok_or("invalid_frame_time")?;
        let x = field(frame, "mouseX", 1);
        let y = field(frame, "mouseY", 2);
        if [x, y].iter().any(|v| {
            !v.is_null()
                && v.as_f64()
                    .is_none_or(|n| !n.is_finite() || n.abs() > 1_000_000.0)
        }) {
            return Err("invalid_frame_position");
        }
        let buttons = field(frame, "buttonState", 3)
            .as_u64()
            .filter(|n| *n <= 31)
            .ok_or("invalid_frame_buttons")?;
        normalized.push(json!({"time":time,"mouseX":x,"mouseY":y,"buttonState":buttons}));
    }
    let processor = field(header, "scoreProcessorStatistics", 5);
    let keys = [
        "baseScore",
        "maximumBaseScore",
        "accuracyJudgementCount",
        "comboPortion",
        "bonusPortion",
    ];
    let mut normalized_processor = serde_json::Map::new();
    for (index, key) in keys.iter().enumerate() {
        let value = field(processor, key, index);
        if !value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=1e12).contains(&n))
        {
            return Err("invalid_score_processor_statistics");
        }
        normalized_processor.insert((*key).into(), value.clone());
    }
    let bundle = json!({"header":{"totalScore":total,"accuracy":accuracy,"combo":combo,"maxCombo":max,"statistics":statistics,"scoreProcessorStatistics":normalized_processor,"receivedTime":chrono::Utc::now().to_rfc3339(),"mods":mods(field(header,"mods",7))?,"totalScoreWithoutMods":field(header,"totalScoreWithoutMods",8),"pauses":field(header,"pauses",9)},"frames":normalized});
    if serde_json::to_vec(&bundle)
        .map_err(|_| "invalid_frames")?
        .len()
        > 32768
    {
        return Err("frame_bundle_too_large");
    }
    Ok(bundle)
}
async fn token_owned(
    state: &AppState,
    uid: i64,
    token: Option<i64>,
    spectator: &Value,
) -> Result<(), &'static str> {
    let Some(token) = token else {
        return Ok(());
    };
    let row=sqlx::query("SELECT t.user_id,t.ruleset_id,b.id,b.official_id FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1 AND t.expires_at>now() AND t.revoked_at IS NULL").bind(token).fetch_optional(&state.pool).await.map_err(db)?.ok_or("invalid_score_token")?;
    if row.get::<i64, _>("user_id") != uid
        || spectator["rulesetID"] != i64::from(row.get::<i16, _>("ruleset_id"))
    {
        return Err("score_token_owner_mismatch");
    }
    let beatmap = spectator["beatmapID"].as_i64();
    if beatmap != Some(row.get("id")) && beatmap != row.get::<Option<i64>, _>("official_id") {
        return Err("score_token_beatmap_mismatch");
    }
    Ok(())
}
async fn broadcast_spectator(state: &AppState, uid: i64, target: &'static str, value: Value) {
    let connections: Vec<_> = state
        .signalr
        .connections
        .lock()
        .await
        .values()
        .filter(|c| c.hub == Hub::Spectator && c.active.load(Ordering::Acquire))
        .cloned()
        .collect();
    for c in connections {
        if c.spectator_watches.lock().await.contains(&uid)
            && !blocked(state, uid, c.user.id).await.unwrap_or(true)
        {
            send(
                &c,
                Outbound::Invocation(target, vec![json!(uid), value.clone()]),
            );
        }
    }
}
async fn spectator(
    state: &AppState,
    c: &Arc<Connection>,
    target: &str,
    args: &[Value],
) -> HubResult {
    let uid = c.user.id;
    match target {
        "BeginPlaySession" | "BeginPlaySessionV2" => {
            let token = argument(args, 0)?.as_i64();
            let spectator = spectator_state(argument(args, 1)?)?;
            if spectator["state"] != 1 {
                return Err("session_must_be_playing");
            }
            token_owned(state, uid, token, &spectator).await?;
            if let Some(token) = token {
                crate::replay_capture::begin(state, uid, token, &c.key, &spectator)
                    .await
                    .map_err(|e| e.1)?;
            }
            state.signalr.playing.lock().await.insert(
                uid,
                Playing {
                    state: spectator.clone(),
                    token,
                    connection_key: c.key.clone(),
                },
            );
            broadcast_spectator(state, uid, "UserBeganPlaying", spectator).await;
            Ok(None)
        }
        "SendFrameData" | "SendFrameDataV2" => {
            common::rate_limit(state, format!("spectator-frames:{uid}"), 600)
                .await
                .map_err(|_| "rate_limited")?;
            let playing = state
                .signalr
                .playing
                .lock()
                .await
                .get(&uid)
                .cloned()
                .ok_or("play_session_required")?;
            if playing.connection_key != c.key {
                return Err("play_connection_replaced");
            }
            let bundle = if target == "SendFrameDataV2" {
                if argument(args, 0)?.as_i64() != playing.token {
                    return Err("score_token_mismatch");
                }
                argument(args, 1)?
            } else {
                argument(args, 0)?
            };
            let bundle = frame_bundle(bundle)?;
            if let Some(token) = playing.token {
                crate::replay_capture::frames(state, uid, token, &c.key, &bundle)
                    .await
                    .map_err(|e| e.1)?;
            }
            broadcast_spectator(state, uid, "UserSentFrames", bundle).await;
            Ok(None)
        }
        "EndPlaySession" | "EndPlaySessionV2" => {
            let mut playing = state
                .signalr
                .playing
                .lock()
                .await
                .get(&uid)
                .cloned()
                .ok_or("play_session_required")?;
            if playing.connection_key != c.key {
                return Err("play_connection_replaced");
            }
            if target == "EndPlaySessionV2" {
                if argument(args, 0)?.as_i64() != playing.token {
                    return Err("score_token_mismatch");
                }
                let finalstate = integer(argument(args, 1)?)?;
                if !(3..=5).contains(&finalstate) {
                    return Err("invalid_final_state");
                }
                playing.state["state"] = json!(finalstate);
            } else {
                let finalstate = spectator_state(argument(args, 0)?)?;
                for key in ["beatmapID", "rulesetID", "mods", "maximumStatistics"] {
                    if finalstate[key] != playing.state[key] {
                        return Err("play_session_state_mismatch");
                    }
                }
                playing.state = finalstate;
                if !playing.state["state"]
                    .as_i64()
                    .is_some_and(|n| (3..=5).contains(&n))
                {
                    return Err("invalid_final_state");
                }
            }
            let mut validated_token = None;
            if let Some(token) = playing.token {
                // Fanout is preserved even if the recorded metadata cannot be bound.
                match crate::replay_capture::end(
                    state,
                    uid,
                    token,
                    &c.key,
                    playing.state["state"].as_i64().unwrap() as i16,
                )
                .await
                {
                    Ok(_) => validated_token = Some(token),
                    Err(error) => tracing::warn!(
                        user_id = uid,
                        code = error.1,
                        "spectator replay not published"
                    ),
                }
                // Replay encoding can fail after end identity was committed.
                // Register that validated end independently of replay publication.
                if validated_token.is_none() {
                    let ended: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lazer_replay_captures WHERE token_id=$1 AND user_id=$2 AND connection_key=$3 AND final_state=$4 AND ended_at IS NOT NULL)")
                        .bind(token).bind(uid).bind(&c.key).bind(playing.state["state"].as_i64().unwrap() as i16)
                        .fetch_one(&state.pool).await.map_err(db)?;
                    if ended {
                        validated_token = Some(token);
                    }
                }
            }
            let mut records = state.signalr.playing.lock().await;
            if records
                .get(&uid)
                .is_some_and(|current| current.connection_key == c.key)
            {
                records.remove(&uid);
            }
            drop(records);
            broadcast_spectator(state, uid, "UserFinishedPlaying", playing.state).await;
            if let Some(token) = validated_token {
                {
                    let mut ended = state.signalr.ended_plays.lock().await;
                    ended.retain(|_, play| {
                        play.registered.elapsed() < Duration::from_secs(6 * 60 * 60)
                    });
                    if ended.len() >= 4096
                        && !ended.contains_key(&token)
                        && let Some(oldest) = ended
                            .iter()
                            .min_by_key(|(_, play)| play.registered)
                            .map(|(token, _)| *token)
                    {
                        ended.remove(&oldest);
                    }
                    ended.insert(
                        token,
                        EndedPlay {
                            user_id: uid,
                            connection_key: c.key.clone(),
                            registered: Instant::now(),
                        },
                    );
                }
                let _dispatch = state.signalr.score_event_dispatch.lock().await;
                notify_ended_score(state, token).await.map_err(|e| e.1)?;
            }
            Ok(None)
        }
        "StartWatchingUser" => {
            let watched = integer(argument(args, 0)?)?;
            if watched == uid {
                return Err("cannot_spectate_self");
            }
            common::account(state, watched)
                .await
                .map_err(|_| "user_not_found")?;
            if blocked(state, uid, watched).await? {
                return Err("user_blocked");
            }
            let mut watches = c.spectator_watches.lock().await;
            if watches.len() >= 8 && !watches.contains(&watched) {
                return Err("too_many_watched_users");
            }
            watches.insert(watched);
            drop(watches);
            state
                .signalr
                .emit(
                    Hub::Spectator,
                    &[watched],
                    "UserStartedWatching",
                    vec![json!([{"onlineID":uid,"username":c.user.username}])],
                )
                .await;
            if let Some(playing) = state.signalr.playing.lock().await.get(&watched).cloned() {
                send(
                    c,
                    Outbound::Invocation("UserBeganPlaying", vec![json!(watched), playing.state]),
                );
            }
            Ok(None)
        }
        "EndWatchingUser" => {
            let watched = integer(argument(args, 0)?)?;
            c.spectator_watches.lock().await.remove(&watched);
            state
                .signalr
                .emit(
                    Hub::Spectator,
                    &[watched],
                    "UserEndedWatching",
                    vec![json!(uid)],
                )
                .await;
            Ok(None)
        }
        _ => Err("unsupported_hub_method"),
    }
}

async fn broadcast_presence(state: &AppState, uid: i64, presence: Value) {
    let connections: Vec<_> = state
        .signalr
        .connections
        .lock()
        .await
        .values()
        .filter(|c| c.hub == Hub::Metadata && c.active.load(Ordering::Acquire))
        .cloned()
        .collect();
    for c in connections {
        if blocked(state, uid, c.user.id).await.unwrap_or(true) {
            continue;
        }
        if c.presence.load(Ordering::Acquire) {
            send(
                &c,
                Outbound::Invocation("UserPresenceUpdated", vec![json!(uid), presence.clone()]),
            );
        }
        let friend=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE user_id=$1 AND target_id=$2 AND relation_type='friend')").bind(c.user.id).bind(uid).fetch_one(&state.pool).await.unwrap_or(false);
        if friend {
            send(
                &c,
                Outbound::Invocation("FriendPresenceUpdated", vec![json!(uid), presence.clone()]),
            );
        }
    }
}
async fn metadata(
    state: &AppState,
    c: &Arc<Connection>,
    target: &str,
    args: &[Value],
) -> HubResult {
    let uid = c.user.id;
    match target {
        "GetChangesSince" => {
            let cursor = integer(argument(args, 0)?)?;
            if !(0..=i64::from(i32::MAX)).contains(&cursor) {
                return Err("invalid_update_cursor");
            }
            let rows:Vec<(i32,i64)>=sqlx::query_as("SELECT id,beatmap_set_id FROM lazer_beatmap_updates WHERE id>$1 ORDER BY id LIMIT 1000").bind(cursor as i32).fetch_all(&state.pool).await.map_err(db)?;
            let last = rows.last().map_or(cursor, |r| i64::from(r.0));
            let sets: HashSet<i64> = rows.iter().map(|r| r.1).collect();
            Ok(Some(
                json!({"beatmapSetIDs":sets,"lastProcessedQueueID":last}),
            ))
        }
        "UpdateActivity" | "UpdateStatus" => {
            let value = argument(args, 0)?;
            if target == "UpdateStatus"
                && !value.is_null()
                && !value.as_i64().is_some_and(|n| (0..=2).contains(&n))
            {
                return Err("invalid_user_status");
            }
            if target == "UpdateActivity" {
                validate_activity(value)?;
            }
            let presence = {
                let mut records = state.signalr.presence.lock().await;
                let presence = records
                    .entry(uid)
                    .or_insert_with(|| json!({"activity":null,"status":2}));
                presence[if target == "UpdateActivity" {
                    "activity"
                } else {
                    "status"
                }] = value.clone();
                presence.clone()
            };
            broadcast_presence(state, uid, presence).await;
            Ok(None)
        }
        "BeginWatchingUserPresence" => {
            c.presence.store(true, Ordering::Release);
            let snapshot = state.signalr.presence.lock().await.clone();
            for (user, presence) in snapshot {
                if !blocked(state, uid, user).await? {
                    send(
                        c,
                        Outbound::Invocation("UserPresenceUpdated", vec![json!(user), presence]),
                    );
                }
            }
            Ok(None)
        }
        "EndWatchingUserPresence" => {
            c.presence.store(false, Ordering::Release);
            Ok(None)
        }
        "RefreshFriends" => {
            let friends = common::friend_ids(state, uid)
                .await
                .map_err(|_| "database_error")?;
            let snapshot = state.signalr.presence.lock().await.clone();
            for user in friends {
                let user = i64::from(user);
                if !blocked(state, uid, user).await? {
                    send(
                        c,
                        Outbound::Invocation(
                            "FriendPresenceUpdated",
                            vec![
                                json!(user),
                                snapshot.get(&user).cloned().unwrap_or(Value::Null),
                            ],
                        ),
                    );
                }
            }
            Ok(None)
        }
        "BeginWatchingMultiplayerRoom" => {
            let room = integer(argument(args, 0)?)?;
            let viewer = common::bearer(state, &c.headers).await.map_err(|e| e.1)?;
            if !crate::room_results::room_history_authorized(state, &viewer, room)
                .await
                .map_err(|e| e.1)?
            {
                return Err("room_membership_required");
            }
            let mut watches = c.room_watches.lock().await;
            if watches.len() >= 4 && !watches.contains(&room) {
                return Err("too_many_room_watches");
            }
            watches.insert(room);
            drop(watches);
            Ok(Some(
                crate::room_results::playlist_stats(state, &viewer, room)
                    .await
                    .map_err(|e| e.1)?,
            ))
        }
        "EndWatchingMultiplayerRoom" => {
            c.room_watches
                .lock()
                .await
                .remove(&integer(argument(args, 0)?)?);
            Ok(None)
        }
        _ => Err("unsupported_hub_method"),
    }
}

/// Called after an end registration or committed room outbox event. The caller
/// holds the dispatch mutex, never a database transaction. Only the exact ended
/// submitting connection receives this two-scalar callback.
async fn notify_ended_score(state: &AppState, token: i64) -> Result<(), ApiError> {
    let ended = state.signalr.ended_plays.lock().await.get(&token).cloned();
    let Some(ended) = ended else {
        return Ok(());
    };
    let connection = state
        .signalr
        .connections
        .lock()
        .await
        .get(&ended.connection_key)
        .cloned();
    let Some(connection) = connection.filter(|c| {
        c.hub == Hub::Spectator && c.user.id == ended.user_id && c.active.load(Ordering::Acquire)
    }) else {
        state.signalr.ended_plays.lock().await.remove(&token);
        return Ok(());
    };
    if !authorized(state, &connection).await {
        state.signalr.ended_plays.lock().await.remove(&token);
        return Ok(());
    }
    let Some(score) = crate::room_scores::processed_score_id(state, ended.user_id, token).await?
    else {
        return Ok(());
    };
    let Ok(user) = i32::try_from(ended.user_id) else {
        state.signalr.ended_plays.lock().await.remove(&token);
        return Ok(());
    };
    send(
        &connection,
        Outbound::Invocation("UserScoreProcessed", vec![json!(user), json!(score)]),
    );
    state.signalr.ended_plays.lock().await.remove(&token);
    Ok(())
}

/// Best-effort realtime delivery follows committed accounting. An unavailable
/// transport never rolls back a score; a later room watch reads committed stats.
pub(crate) async fn publish_room_score_events(state: &AppState) -> Result<usize, ApiError> {
    let _dispatch = state.signalr.score_event_dispatch.lock().await;
    let events = sqlx::query("SELECT e.*,t.round_id,s.verification,u.is_restricted FROM lazer_room_score_events e JOIN lazer_score_tokens t ON t.id=e.token_id AND t.scope='room' AND t.processing_status='processed' AND t.revoked_at IS NULL AND t.score_id=e.score_id AND t.user_id=e.user_id AND t.room_id=e.room_id AND t.playlist_item_id=e.item_id JOIN scores s ON s.id=e.score_id AND s.user_id=t.user_id AND s.beatmap_id=t.beatmap_id AND s.mode=t.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text JOIN users u ON u.id=e.user_id WHERE e.delivered_at IS NULL ORDER BY e.id LIMIT 64")
        .fetch_all(&state.pool).await?;
    let mut delivered = 0;
    for event in events {
        let room: i64 = event.get("room_id");
        let item: i64 = event.get("item_id");
        let round: i64 = event.get("round_id");
        let score: i64 = event.get("score_id");
        let user: i64 = event.get("user_id");
        let restricted: bool = event.get("is_restricted");
        let rejected = event.get::<String, _>("verification") == "rejected";
        let targets: Vec<_> = state
            .signalr
            .connections
            .lock()
            .await
            .values()
            .filter(|c| c.hub == Hub::Metadata && c.active.load(Ordering::Acquire))
            .cloned()
            .collect();
        for connection in targets {
            if restricted || rejected || !connection.room_watches.lock().await.contains(&room) {
                continue;
            }
            let viewer = match common::bearer(state, &connection.headers).await {
                Ok(viewer) if viewer.can("chat.send") => viewer,
                _ => continue,
            };
            if !crate::room_results::round_history_authorized(state, &viewer, room, item, round)
                .await?
            {
                continue;
            }
            let Ok(user_id) = i32::try_from(user) else {
                continue;
            };
            let rank = crate::room_results::event_rank(state, &viewer, room, item, score).await?;
            let value = json!({"roomID":room,"playlistItemID":item,"scoreID":score,"userID":user_id,"totalScore":event.get::<i64,_>("total_score"),"newRank":rank});
            send(
                &connection,
                Outbound::Invocation("MultiplayerRoomScoreSet", vec![value]),
            );
        }
        notify_ended_score(state, event.get("token_id")).await?;
        sqlx::query("UPDATE lazer_room_score_events SET delivered_at=now() WHERE id=$1 AND delivered_at IS NULL")
            .bind(event.get::<i64,_>("id")).execute(&state.pool).await?;
        delivered += 1;
    }
    Ok(delivered)
}

// MessagePack key order follows the public [Key] attributes on client models.
// JSON uses their camel-case names. No client implementation is reused here.
fn validate_activity(value: &Value) -> Result<(), &'static str> {
    if value.is_null() {
        return Ok(());
    }
    let union = value
        .as_array()
        .filter(|v| v.len() == 2)
        .ok_or("invalid_user_activity")?;
    let tag = union[0].as_i64().ok_or("invalid_user_activity")?;
    let data = union[1].as_array().ok_or("invalid_user_activity")?;
    match tag {
        11 | 21 | 51 if data.is_empty() => {}
        12 | 23 | 24 | 31 | 52 if data.len() == 4 => {
            integer(&data[0])?;
            text(&data[1], 256)?;
            if !data[2].as_i64().is_some_and(|v| (0..=3).contains(&v)) {
                return Err("invalid_activity_ruleset");
            }
            text(&data[3], 64)?;
        }
        13 | 14 if data.len() == 4 => {
            integer(&data[0])?;
            text(&data[1], 64)?;
            integer(&data[2])?;
            if !data[3].is_null() {
                text(&data[3], 256)?;
            }
        }
        22 if data.len() == 2 => {
            integer(&data[0])?;
            text(&data[1], 100)?;
        }
        _ => return Err("unsupported_user_activity"),
    }
    Ok(())
}
fn keyed(value: Value, keys: &[&str]) -> Value {
    if value.is_null() || value.is_array() {
        value
    } else {
        json!(
            keys.iter()
                .map(|key| value.get(*key).cloned().unwrap_or(Value::Null))
                .collect::<Vec<_>>()
        )
    }
}
fn wire_mods(value: Value) -> Value {
    json!(
        value
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|v| keyed(v, &["acronym", "settings"]))
            .collect::<Vec<_>>()
    )
}
fn wire_availability(value: Value) -> Value {
    keyed(value, &["state", "downloadProgress"])
}
fn wire_member(mut value: Value) -> Value {
    value["beatmapAvailability"] = wire_availability(value["beatmapAvailability"].take());
    value["mods"] = wire_mods(value["mods"].take());
    keyed(
        value,
        &[
            "userID",
            "state",
            "beatmapAvailability",
            "mods",
            "matchState",
            "rulesetId",
            "beatmapId",
            "votedToSkipIntro",
            "role",
        ],
    )
}
fn wire_settings(value: Value) -> Value {
    keyed(
        value,
        &[
            "name",
            "playlistItemId",
            "password",
            "matchType",
            "queueMode",
            "autoStartDuration",
            "autoSkip",
            "maxParticipants",
        ],
    )
}
fn wire_playlist(mut value: Value) -> Value {
    value["requiredMods"] = wire_mods(value["requiredMods"].take());
    value["allowedMods"] = wire_mods(value["allowedMods"].take());
    value["playedAt"] = wire_datetime(value["playedAt"].take());
    keyed(
        value,
        &[
            "id",
            "ownerID",
            "beatmapID",
            "beatmapChecksum",
            "rulesetID",
            "requiredMods",
            "allowedMods",
            "expired",
            "playlistOrder",
            "playedAt",
            "starRating",
            "freestyle",
        ],
    )
}
fn wire_match(value: Value) -> Value {
    if value.is_null() || value.is_array() {
        value
    } else {
        json!([3, [null, value["locked"], value["slots"]]])
    }
}
fn wire_room(mut value: Value) -> Value {
    value["settings"] = wire_settings(value["settings"].take());
    value["users"] = json!(
        value["users"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(wire_member)
            .collect::<Vec<_>>()
    );
    value["host"] = wire_member(value["host"].take());
    value["matchState"] = wire_match(value["matchState"].take());
    value["playlist"] = json!(
        value["playlist"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(wire_playlist)
            .collect::<Vec<_>>()
    );
    keyed(
        value,
        &[
            "roomID",
            "state",
            "settings",
            "users",
            "host",
            "matchState",
            "playlist",
            "activeCountdowns",
            "channelID",
        ],
    )
}
fn wire_spectator(mut value: Value) -> Value {
    value["mods"] = wire_mods(value["mods"].take());
    keyed(
        value,
        &[
            "beatmapID",
            "rulesetID",
            "mods",
            "state",
            "maximumStatistics",
        ],
    )
}
fn wire_frame(mut value: Value) -> Value {
    let mut header = value["header"].take();
    header["mods"] = wire_mods(header["mods"].take());
    header["scoreProcessorStatistics"] = keyed(
        header["scoreProcessorStatistics"].take(),
        &[
            "baseScore",
            "maximumBaseScore",
            "accuracyJudgementCount",
            "comboPortion",
            "bonusPortion",
        ],
    );
    header["receivedTime"] = wire_datetime(header["receivedTime"].take());
    value["header"] = keyed(
        header,
        &[
            "totalScore",
            "accuracy",
            "combo",
            "maxCombo",
            "statistics",
            "scoreProcessorStatistics",
            "receivedTime",
            "mods",
            "totalScoreWithoutMods",
            "pauses",
        ],
    );
    value["frames"] = json!(
        value["frames"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|v| keyed(v, &["time", "mouseX", "mouseY", "buttonState"]))
            .collect::<Vec<_>>()
    );
    keyed(value, &["header", "frames"])
}
fn wire_datetime(value: Value) -> Value {
    let Some(timestamp) = value
        .as_str()
        .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
    else {
        return value;
    };
    // Native DateTimeOffset carries an instant timestamp extension and minute offset.
    let seconds = timestamp.timestamp();
    let nanoseconds = timestamp.timestamp_subsec_nanos();
    let bytes = if (0..(1i64 << 34)).contains(&seconds) {
        ((u64::from(nanoseconds) << 34) | seconds as u64)
            .to_be_bytes()
            .to_vec()
    } else {
        let mut bytes = nanoseconds.to_be_bytes().to_vec();
        bytes.extend_from_slice(&seconds.to_be_bytes());
        bytes
    };
    json!([{"$msgpack_ext":[-1,bytes]},timestamp.offset().local_minus_utc()/60])
}
fn wire_arguments(target: &str, mut args: Vec<Value>, binary: bool) -> Vec<Value> {
    if !binary {
        // JSON TimeSpan uses the standard textual representation.
        if target == "SettingsChanged"
            && let Some(settings) = args.first_mut()
        {
            settings["autoStartDuration"] = json!("00:00:00");
        }
        return args;
    }
    match target {
        "MultiplayerRoomScoreSet" => {
            if let Some(value) = args.first_mut() {
                *value = keyed(
                    value.take(),
                    &[
                        "roomID",
                        "playlistItemID",
                        "scoreID",
                        "userID",
                        "totalScore",
                        "newRank",
                    ],
                );
            }
        }
        "UserJoined" | "UserLeft" | "UserKicked" => {
            if let Some(v) = args.first_mut() {
                *v = wire_member(v.take());
            }
        }
        "SettingsChanged" => {
            if let Some(v) = args.first_mut() {
                *v = wire_settings(v.take());
            }
        }
        "UserBeatmapAvailabilityChanged" => {
            if let Some(v) = args.get_mut(1) {
                *v = wire_availability(v.take());
            }
        }
        "UserModsChanged" => {
            if let Some(v) = args.get_mut(1) {
                *v = wire_mods(v.take());
            }
        }
        "PlaylistItemAdded" | "PlaylistItemChanged" => {
            if let Some(v) = args.first_mut() {
                *v = wire_playlist(v.take());
            }
        }
        "MatchRoomStateChanged" => {
            if let Some(v) = args.first_mut() {
                *v = wire_match(v.take());
            }
        }
        "UserBeganPlaying" | "UserFinishedPlaying" => {
            if let Some(v) = args.get_mut(1) {
                *v = wire_spectator(v.take());
            }
        }
        "UserSentFrames" => {
            if let Some(v) = args.get_mut(1) {
                *v = wire_frame(v.take());
            }
        }
        "UserStartedWatching" => {
            if let Some(v) = args.first_mut() {
                *v = json!(
                    v.as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|v| keyed(v, &["onlineID", "username"]))
                        .collect::<Vec<_>>()
                );
            }
        }
        "UserPresenceUpdated" | "FriendPresenceUpdated" => {
            if let Some(v) = args.get_mut(1) {
                *v = keyed(v.take(), &["activity", "status"]);
            }
        }
        _ => {}
    }
    args
}
fn wire_result(mut value: Value, binary: bool) -> Value {
    if binary {
        if value.get("roomID").is_some() {
            wire_room(value)
        } else if value.get("beatmapSetIDs").is_some() {
            keyed(value, &["beatmapSetIDs", "lastProcessedQueueID"])
        } else if value.as_array().is_some_and(|items| {
            items
                .iter()
                .all(|item| item.get("playlistItemID").is_some())
        }) {
            json!(
                value
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned()
                    .map(|item| keyed(
                        item,
                        &[
                            "playlistItemID",
                            "totalScoreDistribution",
                            "cumulativeScore",
                            "lastProcessedScoreID"
                        ]
                    ))
                    .collect::<Vec<_>>()
            )
        } else {
            value
        }
    } else {
        if value.get("roomID").is_some() {
            value["settings"]["autoStartDuration"] = json!("00:00:00");
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn room_score_native_event_and_stats_contracts() {
        let huge = i64::MAX - 1;
        let event = json!({"roomID":7,"playlistItemID":11,"scoreID":huge,"userID":3,"totalScore":huge,"newRank":null});
        let stats = json!([{"playlistItemID":11,"totalScoreDistribution":[],"cumulativeScore":huge,"lastProcessedScoreID":huge}]);
        for protocol in [Protocol::Json, Protocol::MessagePack] {
            let binary = protocol == Protocol::MessagePack;
            let codec = Codec::new(protocol);
            let args = wire_arguments("MultiplayerRoomScoreSet", vec![event.clone()], binary);
            let bytes = codec.invocation("MultiplayerRoomScoreSet", args).unwrap();
            let decoded = if binary {
                resonance_protocol::signalr::decode_messagepack_record(&bytes).unwrap()
            } else {
                serde_json::from_slice::<Value>(&bytes[..bytes.len() - 1]).unwrap()
            };
            if binary {
                assert_eq!(decoded[4], json!([[7, 11, huge, 3, huge, null]]));
            } else {
                assert_eq!(decoded["arguments"], json!([event]));
            }
            let bytes = codec
                .completion("stats", Ok(Some(wire_result(stats.clone(), binary))))
                .unwrap();
            let decoded = if binary {
                resonance_protocol::signalr::decode_messagepack_record(&bytes).unwrap()
            } else {
                serde_json::from_slice::<Value>(&bytes[..bytes.len() - 1]).unwrap()
            };
            if binary {
                assert_eq!(decoded[4], json!([[11, [], huge, huge]]));
            } else {
                assert_eq!(decoded["result"], stats);
            }
            let bytes = codec
                .invocation(
                    "UserScoreProcessed",
                    wire_arguments("UserScoreProcessed", vec![json!(3), json!(huge)], binary),
                )
                .unwrap();
            let decoded = if binary {
                resonance_protocol::signalr::decode_messagepack_record(&bytes).unwrap()
            } else {
                serde_json::from_slice::<Value>(&bytes[..bytes.len() - 1]).unwrap()
            };
            if binary {
                assert_eq!(decoded[4], json!([3, huge]));
            } else {
                assert_eq!(decoded["arguments"], json!([3, huge]));
            }
        }
        assert_eq!(wire_result(json!([]), true), json!([]));
        let ranked = json!({"roomID":7,"playlistItemID":11,"scoreID":huge,"userID":3,"totalScore":huge,"newRank":2});
        assert_eq!(
            wire_arguments("MultiplayerRoomScoreSet", vec![ranked], true),
            vec![json!([7, 11, huge, 3, huge, 2])]
        );
    }
    #[test]
    fn playlist_played_at_uses_native_nullable_datetimeoffset() {
        assert_eq!(wire_datetime(Value::Null), Value::Null);
        let instant = wire_datetime(json!("2026-10-07T12:00:00+08:00"));
        assert_eq!(instant[1], 480);
        assert_eq!(instant[0]["$msgpack_ext"][0], -1);
        assert_eq!(instant[0]["$msgpack_ext"][1].as_array().unwrap().len(), 8);
        let prior_epoch = wire_datetime(json!("1969-12-31T23:59:59Z"));
        assert_eq!(
            prior_epoch[0]["$msgpack_ext"][1].as_array().unwrap().len(),
            12
        );
    }
    #[test]
    fn authority_state_machine_rejects_client_shortcuts() {
        assert!(!valid_transition(0, 0, 5));
        assert!(!valid_transition(1, 2, 5));
        assert!(!valid_transition(2, 5, 7));
        assert!(valid_transition(0, 0, 1));
        assert!(valid_transition(1, 2, 3));
        assert!(valid_transition(1, 3, 4));
        assert!(valid_transition(2, 5, 6));
    }
    #[test]
    fn bounded_settings_and_mods() {
        let settings = json!(["Room", 1, "secret", 1, 0, 0, false, 16]);
        assert!(room_settings(&settings).is_ok());
        let mut settings = settings;
        settings[3] = json!(2);
        assert_eq!(room_settings(&settings), Err("unsupported_match_type"));
        assert!(mods(&json!([["DT",{"speed_change":1.5}]])).is_ok());
        assert!(mods(&json!([["invalid", {}]])).is_err());
    }
    #[test]
    fn model_keyed_wire_contract() {
        let value = json!({"roomID":7,"state":0,"settings":{"name":"Room","playlistItemId":1,"password":"","matchType":1,"queueMode":0,"autoStartDuration":0,"autoSkip":false,"maxParticipants":null},"users":[],"host":null,"matchState":{"locked":false,"slots":null},"playlist":[],"activeCountdowns":[],"channelID":10});
        let array = wire_result(value, true);
        assert_eq!(array[0], 7);
        assert_eq!(array[2][3], 1);
        assert_eq!(array[5], json!([3, [null, false, null]]));
        assert_eq!(array[8], 10);
        let args = wire_arguments(
            "UserBeganPlaying",
            vec![
                json!(3),
                json!({"beatmapID":8,"rulesetID":0,"mods":[],"state":1,"maximumStatistics":{"5":5}}),
            ],
            true,
        );
        assert_eq!(args[1], json!([8,0,[],1,{"5":5}]));
    }

    type Socket = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;
    struct TestClient {
        socket: Socket,
        binary: bool,
        events: Vec<Value>,
        counter: u32,
    }
    impl TestClient {
        async fn open(base: &str, token: &str, hub: &str, binary: bool) -> Self {
            let url = format!("{base}/{hub}?access_token={token}").replacen("http://", "ws://", 1);
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
            let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(message.into_text().unwrap(), "{}\u{1e}");
            Self {
                socket,
                binary,
                events: Vec::new(),
                counter: 0,
            }
        }
        async fn read(&mut self) -> Value {
            loop {
                let message = tokio::time::timeout(Duration::from_secs(3), self.socket.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let value = match message {
                    tokio_tungstenite::tungstenite::Message::Text(text) => {
                        serde_json::from_str(text.trim_end_matches('\u{1e}')).unwrap()
                    }
                    tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                        resonance_protocol::signalr::decode_messagepack_record(&bytes).unwrap()
                    }
                    tokio_tungstenite::tungstenite::Message::Ping(_)
                    | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
                    other => panic!("unexpected websocket record {other:?}"),
                };
                let kind = if self.binary {
                    value[0].as_i64()
                } else {
                    value["type"].as_i64()
                };
                if kind == Some(6) {
                    continue;
                }
                return value;
            }
        }
        async fn call(&mut self, target: &str, args: Vec<Value>) -> Value {
            self.counter += 1;
            let id = self.counter.to_string();
            let codec = Codec::new(if self.binary {
                Protocol::MessagePack
            } else {
                Protocol::Json
            });
            let bytes = codec.invocation_with_id(Some(&id), target, args).unwrap();
            let message = if self.binary {
                tokio_tungstenite::tungstenite::Message::Binary(bytes.into())
            } else {
                tokio_tungstenite::tungstenite::Message::Text(
                    String::from_utf8(bytes).unwrap().into(),
                )
            };
            self.socket.send(message).await.unwrap();
            loop {
                let record = self.read().await;
                let matches = if self.binary {
                    record[0] == 3 && record[2] == id
                } else {
                    record["type"] == 3 && record["invocationId"] == id
                };
                if matches {
                    return record;
                }
                self.events.push(record);
            }
        }
        async fn event(&mut self, target: &str) -> Value {
            if let Some(index) = self.events.iter().position(|v| {
                if self.binary {
                    v[3] == target
                } else {
                    v["target"] == target
                }
            }) {
                return self.events.remove(index);
            }
            loop {
                let record = self.read().await;
                let matches = if self.binary {
                    record[3] == target
                } else {
                    record["target"] == target
                };
                if matches {
                    return record;
                }
                self.events.push(record);
            }
        }
    }
    fn success(value: &Value) {
        assert!(value.get("error").is_none(), "{value}");
    }
    fn failure(value: &Value, error: &str) {
        assert_eq!(value["error"], error, "{value}");
    }
    async fn sse_until(response: &mut reqwest::Response, expected: &str) -> String {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut bytes = Vec::new();
            loop {
                bytes.extend(response.chunk().await.unwrap().expect("live SSE stream"));
                assert!(bytes.len() <= 65_536, "bounded SSE fixture response");
                let text = String::from_utf8_lossy(&bytes);
                if text.contains(expected) {
                    return text.into_owned();
                }
            }
        })
        .await
        .expect("expected SSE record across arbitrary HTTP chunks")
    }
    /// Uses an isolated database schema and a real ephemeral TCP listener.
    #[tokio::test]
    #[ignore = "requires ILLYA_TEST_DATABASE_URL pointing at disposable local PostgreSQL"]
    async fn signalr_http_websocket_and_sse() {
        use futures_util::FutureExt;
        let url = std::env::var("ILLYA_TEST_DATABASE_URL").expect("ILLYA_TEST_DATABASE_URL");
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let schema = format!("signalr_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let search = format!("SET search_path TO {schema}");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(12)
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
        let state = AppState {
            pool: pool.clone(),
            storage: Arc::new(std::env::temp_dir()),
            sanctuary_url: Arc::new("http://127.0.0.1:1".into()),
            service_key: Arc::new(String::new()),
            http: reqwest::Client::new(),
            limits: Default::default(),
            trusted_proxies: Default::default(),
            signalr: Arc::new(Runtime::new()),
            lazer_chat: Arc::new(crate::lazer_chat::Runtime::new()),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = router().with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result=std::panic::AssertUnwindSafe(async{
            let mut ids=Vec::new();let mut tokens=Vec::new();
            for n in 0..3{
                let id:i64=sqlx::query_scalar("INSERT INTO users(username,email,password_hash,password_md5_hash) VALUES($1,$2,'unused','unused') RETURNING id").bind(format!("Player{n}")).bind(format!("p{n}@example.test")).fetch_one(&pool).await.unwrap();
                sqlx::query("INSERT INTO user_roles SELECT $1,id FROM roles WHERE name='player'").bind(id).execute(&pool).await.unwrap();let token=common::secret();
                sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,now()+interval '1 hour',now()+interval '1 day')").bind(uuid::Uuid::new_v4()).bind(id).bind(common::token_hash(&token)).bind(common::secret()).execute(&pool).await.unwrap();ids.push(id);tokens.push(token);
            }
            let map:i64=sqlx::query_scalar("INSERT INTO beatmaps(official_set_id,title,artist,creator,version,mode,stars,md5,osu_path) VALUES(101,'Fixture','Artist','Mapper','Test',0,1.5,'0123456789abcdef0123456789abcdef','unused.osu') RETURNING id").fetch_one(&pool).await.unwrap();
            let client=reqwest::Client::new();
            assert_eq!(client.post(format!("{base}/multiplayer/negotiate")).send().await.unwrap().status(),StatusCode::UNAUTHORIZED);
            assert_eq!(client.post(format!("{base}/multiplayer/negotiate")).bearer_auth(&tokens[0]).header("Origin","https://untrusted.example").send().await.unwrap().status(),StatusCode::FORBIDDEN);
            let negotiation:Value=client.post(format!("{base}/signalr/multiplayer/negotiate?negotiateVersion=1")).bearer_auth(&tokens[0]).send().await.unwrap().json().await.unwrap();assert_eq!(negotiation["availableTransports"][0]["transport"],"WebSockets");
            let hijack=client.post(format!("{base}/signalr/multiplayer?id={}",negotiation["connectionToken"].as_str().unwrap())).bearer_auth(&tokens[1]).body("{}\u{1e}").send().await.unwrap();assert_eq!(hijack.status(),StatusCode::FORBIDDEN);
            let mut host=TestClient::open(&base,&tokens[0],"signalr/multiplayer",false).await;let mut guest=TestClient::open(&base,&tokens[1],"multiplayer",false).await;
            let settings=json!({"name":"Integration room","playlistItemId":0,"password":"private-secret","matchType":1,"queueMode":0,"autoStartDuration":0,"autoSkip":false,"maxParticipants":16});
            let playlist=json!({"id":0,"ownerID":ids[0],"beatmapID":map,"beatmapChecksum":"0123456789abcdef0123456789abcdef","rulesetID":0,"requiredMods":[],"allowedMods":[],"expired":false,"playlistOrder":0,"playedAt":null,"starRating":999,"freestyle":false});
            let created=host.call("CreateRoom",vec![json!({"settings":settings,"playlist":[playlist]})]).await;success(&created);let room=created["result"]["roomID"].as_i64().unwrap();let channel=created["result"]["channelID"].as_i64().unwrap();assert_eq!(created["result"]["playlist"][0]["starRating"],1.5);
            let listing:Value=client.get(format!("{base}/api/v2/rooms")).bearer_auth(&tokens[2]).send().await.unwrap().json().await.unwrap();assert_eq!(listing[0]["id"],room);assert_eq!(listing[0]["has_password"],true);assert!(!listing.to_string().contains("private-secret"));assert_eq!(listing[0]["playlist"][0]["beatmap"]["id"],map);
            failure(&guest.call("JoinRoomWithPassword",vec![json!(room),json!("wrong")]).await,"invalid_room_password");success(&guest.call("JoinRoomWithPassword",vec![json!(room),json!("private-secret")]).await);assert_eq!(host.event("UserJoined").await["arguments"][0]["userID"],ids[1]);
            failure(&guest.call("StartMatch",vec![]).await,"host_required");failure(&host.call("ChangeState",vec![json!(5)]).await,"invalid_state_transition");failure(&host.call("ChangeState",vec![json!(1)]).await,"beatmap_not_available");
            for socket in [&mut host,&mut guest]{success(&socket.call("ChangeBeatmapAvailability",vec![json!([4,null])]).await);success(&socket.call("ChangeState",vec![json!(1)]).await);}
            success(&host.call("StartMatch",vec![]).await);host.event("LoadRequested").await;guest.event("LoadRequested").await;
            for socket in [&mut host,&mut guest]{success(&socket.call("ChangeState",vec![json!(3)]).await);success(&socket.call("ChangeState",vec![json!(4)]).await);}
            host.event("GameplayStarted").await;guest.event("GameplayStarted").await;assert_eq!(sqlx::query_scalar::<_,i16>("SELECT state FROM lazer_rooms WHERE id=$1").bind(room).fetch_one(&pool).await.unwrap(),2);
            success(&host.call("ChangeState",vec![json!(6)]).await);assert_eq!(sqlx::query_scalar::<_,i16>("SELECT state FROM lazer_rooms WHERE id=$1").bind(room).fetch_one(&pool).await.unwrap(),2);
            success(&guest.call("ChangeState",vec![json!(6)]).await);host.event("ResultsReady").await;assert_eq!(sqlx::query_scalar::<_,i16>("SELECT state FROM lazer_rooms WHERE id=$1").bind(room).fetch_one(&pool).await.unwrap(),0);
            let mut metadata_host=TestClient::open(&base,&tokens[0],"metadata",true).await;let mut metadata_guest=TestClient::open(&base,&tokens[1],"metadata",true).await;
            let updates=metadata_host.call("GetChangesSince",vec![json!(0)]).await;assert_eq!(updates[0],3);assert_eq!(updates[3],3);assert_eq!(updates[4][0],json!([101]));
            assert_eq!(metadata_guest.call("BeginWatchingUserPresence",vec![]).await[3],2);assert_eq!(metadata_host.call("UpdateStatus",vec![json!(2)]).await[3],2);let presence=metadata_guest.event("UserPresenceUpdated").await;assert_eq!(presence[4],json!([ids[0],[null,2]]));
            let mut player=TestClient::open(&base,&tokens[0],"spectator",true).await;let mut watcher=TestClient::open(&base,&tokens[1],"spectator",false).await;success(&watcher.call("StartWatchingUser",vec![json!(ids[0])]).await);let notification=player.event("UserStartedWatching").await;assert_eq!(notification[4][0][0],json!([ids[1],"Player1"]));
            let mut native_watcher=TestClient::open(&base,&tokens[2],"spectator",true).await;assert_eq!(native_watcher.call("StartWatchingUser",vec![json!(ids[0])]).await[3],2);
            let spectator=json!([map,0,[["DT",{"speed_change":1.5}]],1,{"5":5}]);assert_eq!(player.call("BeginPlaySessionV2",vec![Value::Null,spectator]).await[3],2);assert_eq!(watcher.event("UserBeganPlaying").await["arguments"][1]["beatmapID"],map);let native_state=native_watcher.event("UserBeganPlaying").await;assert_eq!(native_state[4][1][2],json!([["DT",{"speed_change":1.5}]]));
            let frame=json!([[1234,1.0,2,2,{"5":2},[600.0,600.0,2,0.5,0.0],null,[["DT",{"speed_change":1.5}]],null,null],[[100.0,256.0,192.0,1]]]);assert_eq!(player.call("SendFrameDataV2",vec![Value::Null,frame]).await[3],2);let received=watcher.event("UserSentFrames").await;assert_eq!(received["arguments"][1]["frames"][0]["mouseX"],256.0);let native_frame=native_watcher.event("UserSentFrames").await;assert_eq!(native_frame[4][1][1][0],json!([100.0,256.0,192.0,1]));assert!(native_frame[4][1][0][6][0]["$msgpack_ext"].is_array());
            assert_eq!(player.call("EndPlaySessionV2",vec![Value::Null,json!(3)]).await[3],2);assert_eq!(watcher.event("UserFinishedPlaying").await["arguments"][1]["state"],3);
            sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,'block')").bind(ids[1]).bind(ids[0]).execute(&pool).await.unwrap();failure(&watcher.call("StartWatchingUser",vec![json!(ids[0])]).await,"user_blocked");
            let negotiation:Value=client.post(format!("{base}/metadata/negotiate")).bearer_auth(&tokens[2]).send().await.unwrap().json().await.unwrap();let id=negotiation["connectionToken"].as_str().unwrap();
            let mut sse=client.get(format!("{base}/metadata?id={id}")).bearer_auth(&tokens[2]).header("Accept","text/event-stream").send().await.unwrap();assert_eq!(sse.status(),StatusCode::OK);sse_until(&mut sse,"connected").await;
            assert_eq!(client.post(format!("{base}/metadata?id={id}")).bearer_auth(&tokens[2]).body("{\"protocol\":\"json\",\"version\":1}\u{1e}").send().await.unwrap().status(),StatusCode::OK);
            sse_until(&mut sse,"{}\u{1e}").await;
            assert_eq!(client.post(format!("{base}/metadata?id={id}")).bearer_auth(&tokens[2]).body("{\"type\":1,\"invocationId\":\"sse\",\"target\":\"GetChangesSince\",\"arguments\":[0]}\u{1e}").send().await.unwrap().status(),StatusCode::OK);
            let events=sse_until(&mut sse,"\"invocationId\":\"sse\"").await;assert!(events.contains("\"beatmapSetIDs\":[101]"));
            assert_eq!(client.delete(format!("{base}/metadata?id={id}")).bearer_auth(&tokens[2]).send().await.unwrap().status(),StatusCode::ACCEPTED);
            success(&host.call("TransferHost",vec![json!(ids[1])]).await);failure(&host.call("KickUser",vec![json!(ids[1])]).await,"host_required");success(&host.call("LeaveRoom",vec![]).await);success(&guest.call("LeaveRoom",vec![]).await);
            assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM lazer_channel_members WHERE channel_id=$1").bind(channel).fetch_one(&pool).await.unwrap(),0);
            let mut native_host=TestClient::open(&base,&tokens[0],"multiplayer",true).await;
            let native_room=json!([0,0,["Native model room",0,"",1,0,0,false,16],[],null,null,[[0,ids[0],map,"0123456789abcdef0123456789abcdef",0,[["HD",{}]],[],false,0,null,999,false]],[],0]);
            let native_created=native_host.call("CreateRoom",vec![native_room]).await;assert_eq!(native_created[3],3);assert!(native_created[4].is_array());assert_eq!(native_created[4][2][3],1);assert_eq!(native_created[4][3][0][0],ids[0]);assert_eq!(native_created[4][6][0][5],json!([["HD",{}]]));assert_eq!(native_created[4][5][1][2].as_array().unwrap().len(),16);assert_eq!(native_host.call("SendMatchRequest",vec![json!([7,[3]])]).await[3],2);let slot=native_host.event("MatchRoomStateChanged").await;assert_eq!(slot[4][0][1][2][3],ids[0]);assert_eq!(native_host.call("SendMatchRequest",vec![json!([5,[true]])]).await[3],2);let denied=native_host.call("SendMatchRequest",vec![json!([7,[2]])]).await;assert_eq!(denied[3],1);assert_eq!(denied[4],"room_slots_locked");assert_eq!(native_host.call("LeaveRoom",vec![]).await[3],2);
            sqlx::query("DELETE FROM lazer_sessions WHERE access_hash=$1").bind(common::token_hash(&tokens[1])).execute(&pool).await.unwrap();failure(&watcher.call("StartWatchingUser",vec![json!(ids[2])]).await,"session_revoked");
            for mut socket in [host,guest,metadata_host,metadata_guest,player,watcher,native_watcher,native_host]{let _=socket.socket.close(None).await;}
        }).catch_unwind().await;
        server.abort();
        let _ = server.await;
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}
