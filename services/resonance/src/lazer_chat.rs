//! Original native chat contracts. Database commit notifications join all writers.
use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, RawQuery, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post, put},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{
    PgPool, Row,
    postgres::{PgListener, PgRow},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast};

const MAX_CHANNELS: i64 = 4096;
const MAX_WIRE: usize = 65_536;

pub struct Runtime {
    endpoint: String,
    events: broadcast::Sender<Value>,
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl Runtime {
    pub fn new() -> Self {
        let endpoint = std::env::var("LAZER_NOTIFICATION_ENDPOINT").unwrap_or_else(|_| {
            format!(
                "wss://osu.{}/notification-server",
                std::env::var("ILLYA_DOMAIN").unwrap_or_else(|_| "localhost".into())
            )
        });
        Self::with_endpoint(endpoint)
    }
    pub fn with_endpoint(endpoint: String) -> Self {
        Self {
            endpoint,
            events: broadcast::channel(256).0,
            listener: Mutex::new(None),
        }
    }
    pub async fn start(&self, pool: PgPool) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(&pool).await?;
        listener.listen("illya_chat").await?;
        let schema: String = sqlx::query_scalar("SELECT current_schema()")
            .fetch_one(&pool)
            .await?;
        let sender = self.events.clone();
        let mut task = self.listener.lock().expect("chat listener mutex");
        if task.is_some() {
            return Ok(());
        }
        *task = Some(tokio::spawn(async move {
            loop {
                match listener.try_recv().await {
                    Ok(Some(event)) => {
                        if let Ok(value) = serde_json::from_str::<Value>(event.payload())
                            && value["schema"].as_str() == Some(&schema)
                        {
                            let _ = sender.send(value);
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(json!({"kind":"resync"}));
                    }
                    Err(error) => {
                        tracing::warn!(%error,"native chat listener reconnecting");
                        let _ = sender.send(json!({"kind":"resync"}));
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }));
        Ok(())
    }
}
impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Ok(task) = self.listener.get_mut()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

pub fn validate_endpoint(endpoint: &str) -> Result<(), &'static str> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "invalid_notification_endpoint")?;
    if !matches!(url.scheme(), "ws" | "wss")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_notification_endpoint");
    }
    Ok(())
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/notification-server", get(upgrade))
        .route("/api/v2/notifications", get(notifications))
        .route("/api/v2/chat/updates", get(updates))
        .route("/api/v2/chat/presence", get(presence))
        .route("/api/v2/chat/ack", post(ack))
        .route("/api/v2/chat/new", post(new_conversation))
        .route("/api/v2/chat/channels", get(channels).post(create_channel))
        .route("/api/v2/chat/channels/{id}", get(channel))
        .route(
            "/api/v2/chat/channels/{id}/messages",
            get(messages).post(send_message),
        )
        .route(
            "/api/v2/chat/channels/{id}/users/{user}",
            put(join_channel).delete(leave_channel),
        )
        .route(
            "/api/v2/chat/channels/{id}/mark-as-read/{message_id}",
            put(mark_read),
        )
        .layer(DefaultBodyLimit::max(8192))
}

async fn initialise(state: &AppState, user: i64) -> Result<(), ApiError> {
    let mut tx = state.pool.begin().await?;
    let inserted =
        sqlx::query("INSERT INTO lazer_chat_activity(user_id) VALUES($1) ON CONFLICT DO NOTHING")
            .bind(user)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
    if inserted {
        sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) SELECT $1,id FROM channels WHERE kind='PUBLIC' AND required_permission IS NULL AND name IN('general','mapping','help') ON CONFLICT DO NOTHING")
            .bind(user).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    initialise(&state, user.id).await?;
    validate_endpoint(&state.lazer_chat.endpoint)
        .map_err(|error| ApiError(StatusCode::SERVICE_UNAVAILABLE, error))?;
    Ok(Json(
        json!({"notifications":[],"stacks":[],"unread_count":0,"has_more":false,"notification_endpoint":state.lazer_chat.endpoint}),
    ))
}
async fn blocked(state: &AppState, first: i64, second: i64) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='block')")
        .bind(first).bind(second).fetch_one(&state.pool).await?)
}
async fn allowed(state: &AppState, user: &Account, row: &PgRow) -> Result<bool, ApiError> {
    if row.get::<String, _>("kind") == "PM" {
        let low: i64 = row.get("dm_user_low");
        let high: i64 = row.get("dm_user_high");
        if user.id != low && user.id != high {
            return Ok(false);
        }
        let other = if user.id == low { high } else { low };
        return Ok(
            common::account(state, other).await.is_ok() && !blocked(state, user.id, other).await?
        );
    }
    match row.get::<Option<String>,_>("required_permission").as_deref() {
        Some("lazer.room") => Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lazer_room_members m JOIN lazer_rooms r ON r.id=m.room_id WHERE m.user_id=$1 AND r.channel_id=$2)").bind(user.id).bind(row.get::<i64,_>("id")).fetch_one(&state.pool).await?),
        Some("stable.room" | "chat.pm") => Ok(false),
        Some(permission) => Ok(user.can(permission)), None => Ok(true),
    }
}
async fn access(state: &AppState, user: &Account, id: i64) -> Result<PgRow, ApiError> {
    let row = sqlx::query("SELECT * FROM channels WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
    if !allowed(state, user, &row).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "channel_not_found"));
    }
    Ok(row)
}
async fn pm_can_send(state: &AppState, sender: i64, recipient: i64) -> Result<(), ApiError> {
    if sender == recipient {
        return Err(ApiError(StatusCode::BAD_REQUEST, "cannot_message_self"));
    }
    common::account(state, recipient)
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "user_not_found"))?;
    if blocked(state, sender, recipient).await? {
        return Err(ApiError(StatusCode::FORBIDDEN, "user_blocked"));
    }
    let friends_only: bool = sqlx::query_scalar("SELECT COALESCE(preferences->>'pm_friends_only','false')='true' OR EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$1 AND expires_at>now() AND block_non_friend_dms) FROM users WHERE id=$1")
        .bind(recipient).fetch_one(&state.pool).await?;
    let friend: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE user_id=$1 AND target_id=$2 AND relation_type='friend')")
        .bind(recipient).bind(sender).fetch_one(&state.pool).await?;
    if friends_only && !friend {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "recipient_accepts_friends_only",
        ));
    }
    Ok(())
}
async fn channel_json(state: &AppState, user: &Account, row: &PgRow) -> Result<Value, ApiError> {
    let id: i64 = row.get("id");
    let private = row.get::<String, _>("kind") == "PM";
    let mut name = format!("#{}", row.get::<String, _>("name"));
    let mut users = Vec::new();
    let mut icon: Option<String> = None;
    if private {
        let low: i64 = row.get("dm_user_low");
        let high: i64 = row.get("dm_user_high");
        let other = common::account(state, if user.id == low { high } else { low }).await?;
        name = other.username;
        icon = other.avatar_url;
        users = vec![low, high];
    }
    let last: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(id),0) FROM chat_messages WHERE channel_id=$1")
            .bind(id)
            .fetch_one(&state.pool)
            .await?;
    let read: Option<i64> = sqlx::query_scalar(
        "SELECT last_read_id FROM lazer_channel_members WHERE channel_id=$1 AND user_id=$2",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;
    let can_message = user.can("chat.send")
        && (!private
            || pm_can_send(
                state,
                user.id,
                users.iter().copied().find(|id| *id != user.id).unwrap(),
            )
            .await
            .is_ok());
    let kind = if private {
        "PM"
    } else if row
        .get::<Option<String>, _>("required_permission")
        .as_deref()
        == Some("lazer.room")
    {
        "MULTIPLAYER"
    } else {
        "PUBLIC"
    };
    Ok(
        json!({"channel_id":id,"name":name,"description":row.get::<String,_>("description"),"type":kind,"icon":icon,"moderated":false,"message_length_limit":1000,"last_message_id":if last>0{Some(last)}else{None},"last_read_id":read,"current_user_attributes":{"can_message":can_message,"can_message_error":if can_message{None}else{Some("chat_permission_required")},"last_read_id":read},"users":users}),
    )
}
async fn joined(state: &AppState, user: &Account) -> Result<HashMap<i64, Value>, ApiError> {
    let rows = sqlx::query("SELECT c.* FROM channels c JOIN lazer_channel_members m ON m.channel_id=c.id WHERE m.user_id=$1 ORDER BY c.id LIMIT 4097")
        .bind(user.id).fetch_all(&state.pool).await?;
    if rows.len() > MAX_CHANNELS as usize {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "channel_limit_reached",
        ));
    }
    let mut channels = HashMap::new();
    for row in rows {
        if allowed(state, user, &row).await? {
            channels.insert(row.get("id"), channel_json(state, user, &row).await?);
        }
    }
    Ok(channels)
}
async fn channels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let rows=sqlx::query("SELECT * FROM channels WHERE kind='PUBLIC' AND required_permission IS DISTINCT FROM 'lazer.room' ORDER BY id LIMIT 1000").fetch_all(&state.pool).await?;
    let mut items = Vec::new();
    for row in rows {
        if allowed(&state, &user, &row).await? {
            items.push(channel_json(&state, &user, &row).await?);
        }
    }
    Ok(Json(json!(items)))
}
async fn channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let row = access(&state, &user, id).await?;
    let value = channel_json(&state, &user, &row).await?;
    let mut users = Vec::new();
    if row.get::<String, _>("kind") == "PM" {
        for id in value["users"].as_array().unwrap() {
            users.push(crate::lazer::user_json(
                &common::account(&state, id.as_i64().unwrap()).await?,
            ));
        }
    }
    Ok(Json(json!({"channel":value,"users":users})))
}
async fn join_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, owner)): Path<(i64, i64)>,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    if owner != user.id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "cannot_join_as_another_user",
        ));
    }
    let row = access(&state, &user, id).await?;
    common::rate_limit(&state, format!("chat-membership:{}", user.id), 60).await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM lazer_channel_members WHERE user_id=$1 AND channel_id<>$2",
    )
    .bind(user.id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if count >= MAX_CHANNELS {
        return Err(ApiError(StatusCode::BAD_REQUEST, "channel_limit_reached"));
    }
    sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(user.id).bind(id).execute(&state.pool).await?;
    Ok(Json(channel_json(&state, &user, &row).await?))
}
async fn leave_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, owner)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    if owner != user.id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "cannot_leave_as_another_user",
        ));
    }
    common::rate_limit(&state, format!("chat-membership:{}", user.id), 60).await?;
    sqlx::query("DELETE FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2")
        .bind(user.id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize, Default)]
struct History {
    since: Option<i64>,
    until: Option<i64>,
    limit: Option<u32>,
}
impl History {
    fn validate(&self) -> Result<(), ApiError> {
        if self.since.is_some_and(|id| id < 0)
            || self.until.is_some_and(|id| id < 0)
            || self.limit.is_some_and(|n| n == 0 || n > 50)
        {
            return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_history_bounds"));
        }
        Ok(())
    }
}
fn message_json(row: &PgRow) -> Value {
    json!({"message_id":row.get::<i64,_>("id"),"sender_id":row.get::<i64,_>("user_id"),"channel_id":row.get::<i64,_>("channel_id"),"content":row.get::<String,_>("content"),"is_action":row.get::<bool,_>("is_action"),"timestamp":row.get::<chrono::DateTime<Utc>,_>("created_at"),"uuid":row.get::<Option<String>,_>("client_uuid").unwrap_or_default()})
}
async fn history(
    state: &AppState,
    user: &Account,
    id: i64,
    query: History,
) -> Result<Vec<Value>, ApiError> {
    query.validate()?;
    access(state, user, id).await?;
    let rows=sqlx::query("SELECT m.* FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND ($2::bigint IS NULL OR m.id>$2) AND ($3::bigint IS NULL OR m.id<$3) AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$4 AND r.target_id=m.user_id) OR (r.target_id=$4 AND r.user_id=m.user_id))) ORDER BY m.id DESC LIMIT $5")
        .bind(id).bind(query.since).bind(query.until).bind(user.id).bind(i64::from(query.limit.unwrap_or(50))).fetch_all(&state.pool).await?;
    Ok(rows.iter().rev().map(message_json).collect())
}
async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<History>,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    Ok(Json(json!(history(&state, &user, id, query).await?)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatInput {
    message: String,
    #[serde(default)]
    is_action: bool,
    #[serde(default)]
    uuid: Option<String>,
}
impl ChatInput {
    fn validate(&self) -> Result<(&str, Option<&str>), ApiError> {
        let content = self.message.trim();
        if content.is_empty()
            || content.chars().count() > 1000
            || content
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_message_length"));
        }
        let uuid = self.uuid.as_deref().filter(|v| !v.is_empty());
        if uuid.is_some_and(|v| {
            v.len() > 128
                || !v
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':'))
        }) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_message_uuid"));
        }
        Ok((content, uuid))
    }
}
async fn store_message(
    state: &AppState,
    user: &Account,
    id: i64,
    input: &ChatInput,
) -> Result<Value, ApiError> {
    let (content, uuid) = input.validate()?;
    if !user.can("chat.send") {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_permission_required"));
    }
    common::rate_limit(state, format!("chat:{}", user.id), 30).await?;
    let row = access(state, user, id).await?;
    let private = row.get::<String, _>("kind") == "PM";
    let mut ids = vec![user.id];
    if private {
        ids = vec![
            row.get::<i64, _>("dm_user_low"),
            row.get::<i64, _>("dm_user_high"),
        ];
    }
    let mut tx = state.pool.begin().await?;
    // All PM peers are locked in ID order, including concurrent opposite sends.
    sqlx::query("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR UPDATE")
        .bind(ids)
        .execute(&mut *tx)
        .await?;
    if !common::account(state, user.id).await?.can("chat.send") {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_permission_required"));
    }
    access(state, user, id).await?;
    if let Some(uuid) = uuid
        && let Some(existing) =
            sqlx::query("SELECT * FROM chat_messages WHERE user_id=$1 AND client_uuid=$2")
                .bind(user.id)
                .bind(uuid)
                .fetch_optional(&mut *tx)
                .await?
    {
        if existing.get::<i64, _>("channel_id") != id
            || existing.get::<String, _>("content") != content
            || existing.get::<bool, _>("is_action") != input.is_action
        {
            return Err(ApiError(StatusCode::CONFLICT, "message_uuid_conflict"));
        }
        return Ok(message_json(&existing));
    }
    let stored = if private {
        let low: i64 = row.get("dm_user_low");
        let high: i64 = row.get("dm_user_high");
        let other = if low == user.id { high } else { low };
        pm_can_send(state, user.id, other).await?;
        let private_id:i64=sqlx::query_scalar("INSERT INTO protocol_private_messages(sender_id,recipient_id,content,is_action,client_uuid) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(user.id).bind(other).bind(content).bind(input.is_action).bind(uuid).fetch_one(&mut *tx).await?;
        sqlx::query("SELECT * FROM chat_messages WHERE private_message_id=$1")
            .bind(private_id)
            .fetch_one(&mut *tx)
            .await?
    } else {
        let membership=sqlx::query("SELECT user_id FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2 FOR SHARE").bind(user.id).bind(id).fetch_optional(&mut *tx).await?;
        if membership.is_none() {
            return Err(ApiError(StatusCode::FORBIDDEN, "join_channel_first"));
        }
        sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content,is_action,client_uuid) VALUES($1,$2,$3,$4,$5) RETURNING *").bind(id).bind(user.id).bind(content).bind(input.is_action).bind(uuid).fetch_one(&mut *tx).await?
    };
    tx.commit().await?;
    Ok(message_json(&stored))
}
async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let input: ChatInput = crate::lazer::native_form(request).await?;
    Ok(Json(store_message(&state, &user, id, &input).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    #[serde(rename = "type")]
    kind: String,
    target_id: i64,
}
async fn pm_channel(state: &AppState, user: &Account, target: i64) -> Result<i64, ApiError> {
    if !user.can("chat.send") {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_permission_required"));
    }
    pm_can_send(state, user.id, target).await?;
    common::rate_limit(state, format!("chat-create:{}", user.id), 30).await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR UPDATE")
        .bind(vec![user.id, target])
        .execute(&mut *tx)
        .await?;
    pm_can_send(state, user.id, target).await?;
    let id: i64 = sqlx::query_scalar("SELECT illya_pm_channel($1,$2)")
        .bind(user.id)
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(user.id).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(id)
}
async fn create_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let input: Create = crate::lazer::native_form(request).await?;
    if input.kind != "PM" {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "channel_type_not_supported",
        ));
    }
    let id = pm_channel(&state, &user, input.target_id).await?;
    let row = access(&state, &user, id).await?;
    let mut channel = channel_json(&state, &user, &row).await?;
    // Native ChannelManager consumes this non-null APIChatChannel collection.
    channel["recent_messages"] = json!(history(&state, &user, id, History::default()).await?);
    Ok(Json(channel))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPm {
    target_id: i64,
    message: String,
    #[serde(default)]
    is_action: bool,
    #[serde(default)]
    uuid: Option<String>,
}
async fn new_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let input: NewPm = crate::lazer::native_form(request).await?;
    let id = pm_channel(&state, &user, input.target_id).await?;
    let input = ChatInput {
        message: input.message,
        is_action: input.is_action,
        uuid: input.uuid,
    };
    let message = store_message(&state, &user, id, &input).await?;
    let row = access(&state, &user, id).await?;
    Ok(Json(
        json!({"channel":channel_json(&state,&user,&row).await?,"message":message,"new_channel_id":id}),
    ))
}
async fn mark_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, message_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    access(&state, &user, id).await?;
    if message_id < 0 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_read_marker"));
    }
    let updated=sqlx::query("UPDATE lazer_channel_members SET last_read_id=$3 WHERE user_id=$1 AND channel_id=$2 AND last_read_id<=$3 AND EXISTS(SELECT 1 FROM chat_messages WHERE id=$3 AND channel_id=$2)")
        .bind(user.id).bind(id).bind(message_id).execute(&state.pool).await?.rows_affected();
    if updated == 0 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_read_marker"));
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Keepalive {
    since: Option<i64>,
    history_since: Option<i64>,
}
async fn parse_keepalive(request: Request) -> Result<Keepalive, ApiError> {
    if request.headers().get("content-type").is_none() {
        // Native ChatAckRequest has no parameters before receiving a message;
        // osu-framework then sends an empty POST without a Content-Type header.
        let body = axum::body::to_bytes(request.into_body(), 8192)
            .await
            .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "native_request_too_large"))?;
        if !body.is_empty() {
            return Err(ApiError(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_native_form_type",
            ));
        }
        return Ok(Keepalive::default());
    }
    crate::lazer::native_form(request).await
}
async fn ack(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    let input = parse_keepalive(request).await?;
    if input.since.is_some_and(|n| n < 0) || input.history_since.is_some_and(|n| n < 0) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_keepalive_cursor",
        ));
    }
    initialise(&state, user.id).await?;
    common::rate_limit(&state, format!("chat-ack:{}", user.id), 120).await?;
    sqlx::query(
        "UPDATE lazer_chat_activity SET active_until=now()+interval '10 minutes' WHERE user_id=$1",
    )
    .bind(user.id)
    .execute(&state.pool)
    .await?;
    Ok(Json(json!({"silences":[]})))
}
async fn presence(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    initialise(&state, user.id).await?;
    Ok(Json(json!(
        joined(&state, &user)
            .await?
            .into_values()
            .collect::<Vec<_>>()
    )))
}
async fn updates(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let raw = raw.unwrap_or_default();
    if raw.len() > 2048 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "updates_query_too_large"));
    }
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(&raw)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_updates_query"))?;
    let query: HashMap<_, _> = pairs.iter().cloned().collect();
    let since = query
        .get("since")
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "missing_updates_cursor"))?
        .parse::<i64>()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_updates_cursor"))?;
    if since < 0 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_updates_cursor"));
    }
    let user = common::bearer(&state, &headers).await?;
    initialise(&state, user.id).await?;
    let channels = joined(&state, &user).await?;
    let mut response =
        json!({"presence":channels.into_values().collect::<Vec<_>>(),"messages":[],"silences":[]});
    // Native lazer requests presence only; older clients use all fields by default.
    let includes: HashSet<&str> = pairs
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "includes[]" | "includes"))
        .map(|(_, value)| value.as_str())
        .collect();
    if !includes.is_empty() {
        for key in ["presence", "messages", "silences"] {
            if !includes.contains(key) {
                response.as_object_mut().unwrap().remove(key);
            }
        }
    }
    Ok(Json(response))
}

#[derive(Deserialize)]
struct Command {
    event: String,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}
fn command(text: &str) -> Result<bool, &'static str> {
    if text.len() > 2048 {
        return Err("command_too_large");
    }
    let value: Command = serde_json::from_str(text).map_err(|_| "invalid_chat_command")?;
    if value.data.is_some() || value.error.is_some() {
        return Err("unexpected_command_payload");
    }
    match value.event.as_str() {
        "chat.start" => Ok(true),
        "chat.end" => Ok(false),
        _ => Err("unsupported_chat_command"),
    }
}
static SOCKETS: OnceLock<Arc<Semaphore>> = OnceLock::new();
static USER_SOCKETS: OnceLock<Mutex<HashMap<i64, usize>>> = OnceLock::new();
struct SocketPermit {
    user: i64,
    _total: OwnedSemaphorePermit,
}
impl SocketPermit {
    fn take(user: i64) -> Result<Self, ApiError> {
        let total = SOCKETS
            .get_or_init(|| Arc::new(Semaphore::new(512)))
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                ApiError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "notification_capacity_reached",
                )
            })?;
        let mut users = USER_SOCKETS
            .get_or_init(Default::default)
            .lock()
            .expect("chat sockets mutex");
        let count = users.entry(user).or_default();
        if *count >= 4 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_notification_connections",
            ));
        }
        *count += 1;
        Ok(Self {
            user,
            _total: total,
        })
    }
}
impl Drop for SocketPermit {
    fn drop(&mut self) {
        if let Some(users) = USER_SOCKETS.get()
            && let Ok(mut users) = users.lock()
            && let Some(count) = users.get_mut(&self.user)
        {
            *count -= 1;
            if *count == 0 {
                users.remove(&self.user);
            }
        }
    }
}
async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let user = common::bearer(&state, &headers).await?;
    if let Some(origin) = headers.get("origin") {
        let trusted = std::env::var("PUBLIC_ORIGIN").unwrap_or_default();
        if origin.to_str().ok() != Some(trusted.as_str()) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "untrusted_websocket_origin",
            ));
        }
    }
    common::rate_limit(&state, format!("chat-socket:{}", user.id), 30).await?;
    initialise(&state, user.id).await?;
    let permit = SocketPermit::take(user.id)?;
    let events = state.lazer_chat.events.subscribe();
    Ok(ws
        .max_frame_size(4096)
        .max_message_size(2048)
        .write_buffer_size(4096)
        .max_write_buffer_size(131_072)
        .on_upgrade(move |socket| connection(socket, state, headers, events, permit)))
}
async fn emit(socket: &mut WebSocket, event: &str, data: Value) -> Result<(), ()> {
    let text = json!({"event":event,"data":data}).to_string();
    if text.len() > MAX_WIRE {
        return Err(());
    }
    send(socket, Message::Text(text.into())).await
}
async fn send(socket: &mut WebSocket, message: Message) -> Result<(), ()> {
    tokio::time::timeout(Duration::from_secs(2), socket.send(message))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
}
async fn deliver(state: &AppState, user: &Account, id: i64) -> Result<Option<Value>, ApiError> {
    let row=sqlx::query("SELECT m.* FROM chat_messages m JOIN channels c ON c.id=m.channel_id JOIN users u ON u.id=m.user_id WHERE m.id=$1 AND NOT u.is_restricted AND ((c.kind='PM' AND $2 IN(c.dm_user_low,c.dm_user_high)) OR EXISTS(SELECT 1 FROM lazer_channel_members WHERE user_id=$2 AND channel_id=c.id))")
        .bind(id).bind(user.id).fetch_optional(&state.pool).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let sender: i64 = row.get("user_id");
    let channel = match access(state, user, row.get("channel_id")).await {
        Ok(row) => row,
        Err(_) => return Ok(None),
    };
    if blocked(state, user.id, sender).await? {
        return Ok(None);
    }
    if channel.get::<String, _>("kind") == "PUBLIC"
        && channel
            .get::<Option<String>, _>("required_permission")
            .as_deref()
            != Some("lazer.room")
    {
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lazer_chat_activity WHERE user_id=$1 AND active_until>now())").bind(user.id).fetch_one(&state.pool).await?;
        if !active {
            return Ok(None);
        }
    }
    let sender = common::account(state, sender).await?;
    Ok(Some(
        json!({"messages":[message_json(&row)],"users":[crate::lazer::user_json(&sender)]}),
    ))
}
async fn connection(
    mut socket: WebSocket,
    state: AppState,
    headers: HeaderMap,
    mut events: broadcast::Receiver<Value>,
    _permit: SocketPermit,
) {
    let mut active = false;
    let mut known = HashMap::<i64, Value>::new();
    let mut seen = HashSet::<i64>::new();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut command_count = 0;
    let mut command_window = std::time::Instant::now();
    loop {
        tokio::select! {
            incoming=socket.recv()=>match incoming{
                Some(Ok(Message::Text(text)))=>{
                    if command_window.elapsed()>Duration::from_secs(60){command_count=0;command_window=std::time::Instant::now();}command_count+=1;if command_count>60{break;}
                    if common::bearer(&state,&headers).await.is_err(){break;}
                match command(&text){Ok(enabled)=>{active=enabled;},Err(error)=>{let text=json!({"event":"error","error":error}).to_string();if send(&mut socket,Message::Text(text.into())).await.is_err(){break;}}}
            },Some(Ok(Message::Ping(payload)))=>{if send(&mut socket,Message::Pong(payload)).await.is_err(){break;}},Some(Ok(Message::Pong(_)))=>{},_=>break,
            },
            event=events.recv()=>{
            let Ok(event)=event else{break;};if event["kind"]=="resync"{break;}
            if !active{continue;}
                let Ok(user)=common::bearer(&state,&headers).await else{break;};
                if event["kind"]=="message"{
                let Some(id)=event["id"].as_i64()else{break;};if seen.contains(&id){continue;}
                if seen.len()>=1024{seen.clear();}seen.insert(id);
                    match deliver(&state,&user,id).await{Ok(Some(data))=>{if emit(&mut socket,"chat.message.new",data).await.is_err(){break;}},Ok(None)=>{},Err(_)=>break}
                }else if event["user"].as_i64()==Some(user.id){
                    let Some(id)=event["channel"].as_i64()else{break;};
                let member=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2)").bind(user.id).bind(id).fetch_one(&state.pool).await;
                let Ok(member)=member else{break;};
                    if event["kind"]=="part"{
                    if member{continue;}
                        if let Some(channel)=known.remove(&id)&&emit(&mut socket,"chat.channel.part",channel).await.is_err(){break;}
                }else if member && let Ok(row)=access(&state,&user,id).await{
                        match channel_json(&state,&user,&row).await{Ok(channel)=>{known.insert(id,channel.clone());if emit(&mut socket,"chat.channel.join",channel).await.is_err(){break;}},Err(_)=>break}
                    }
                }
            },
            _=interval.tick()=>{
                let Ok(user)=common::bearer(&state,&headers).await else{let _=emit(&mut socket,"logout",json!({})).await;break;};
                if !active{continue;}let Ok(current)=joined(&state,&user).await else{break;};
                let mut failed=false;
                for (id,channel) in &known{if !current.contains_key(id)&&emit(&mut socket,"chat.channel.part",channel.clone()).await.is_err(){failed=true;break;}}
                if failed{break;}
                for (id,channel) in &current{if !known.contains_key(id)&&emit(&mut socket,"chat.channel.join",channel.clone()).await.is_err(){failed=true;break;}}
                if failed{break;}known=current;
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Close(None))).await;
}

#[cfg(test)]
mod tests;
