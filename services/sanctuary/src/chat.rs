use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::HeaderMap,
    response::Response,
};
use illya_domain::{Pagination, has_permission};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

#[cfg(test)]
#[path = "community_mutation_tests.rs"]
mod mutation_tests;

async fn can_read(state: &AppState, headers: &HeaderMap, id: i64) -> ApiResult<()> {
    let required: Option<String> =
        sqlx::query_scalar("SELECT required_permission FROM channels WHERE id=$1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(ApiError::missing)?;
    if let Some(permission) = required {
        if matches!(
            permission.as_str(),
            "lazer.room" | "stable.room" | "chat.pm"
        ) {
            return Err(ApiError::missing());
        }
        auth::require(state, headers).await?.require(&permission)?;
    }
    Ok(())
}
pub async fn channels(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = auth::optional(&state, &headers).await?;
    let permissions = user.map(|u| u.permissions).unwrap_or_default();
    let items = sqlx::query_scalar::<_, Value>("SELECT to_jsonb(c) FROM channels c WHERE required_permission IS NULL OR required_permission NOT IN ('lazer.room','stable.room','chat.pm') ORDER BY id")
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .filter(|v| {
            v["required_permission"]
                .as_str()
                .is_none_or(|p| has_permission(&permissions, p))
        })
        .collect::<Vec<_>>();
    let total = items.len() as i64;
    Ok(page(items, total, 1, total.max(1)))
}
#[derive(Deserialize)]
pub struct MessageQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    after_id: Option<i64>,
    before_id: Option<i64>,
}
async fn fetch(state: &AppState, id: i64, query: MessageQuery) -> ApiResult {
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_messages WHERE channel_id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let mut items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE channel_id=$1 AND ($2::bigint IS NULL OR m.id>$2) AND ($3::bigint IS NULL OR m.id<$3) ORDER BY m.id DESC LIMIT $4 OFFSET $5").bind(id).bind(query.after_id).bind(query.before_id).bind(size).bind(offset).fetch_all(&state.db).await?;
    items.reverse();
    Ok(page(items, total, p, size))
}
pub async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<MessageQuery>,
) -> ApiResult {
    can_read(&state, &headers, id).await?;
    fetch(&state, id, query).await
}
#[derive(Deserialize)]
pub struct NewMessage {
    content: String,
}
pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<NewMessage>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("chat.send")?;
    let content = input.content.trim();
    if content.is_empty()
        || content.chars().count() > 1000
        || content
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(ApiError::bad("Message must contain 1–1,000 characters."));
    }
    let mut tx = state.db.begin().await?;
    let user = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Permissions).await?;
    user.require("chat.send")?;
    // Hold the channel policy through publication. A queued permission change
    // must be observed before sending, rather than using an earlier read check.
    let required: Option<String> =
        sqlx::query_scalar("SELECT required_permission FROM channels WHERE id=$1 FOR SHARE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::missing)?;
    if let Some(permission) = required {
        if matches!(
            permission.as_str(),
            "lazer.room" | "stable.room" | "chat.pm"
        ) {
            return Err(ApiError::missing());
        }
        user.require(&permission)?;
    }
    let message_id: i64 = sqlx::query_scalar(
        "INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,$3) RETURNING id",
    )
    .bind(id)
    .bind(user.id)
    .bind(content)
    .fetch_one(&mut *tx)
    .await?;
    let value:Value=sqlx::query_scalar("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.id=$1").bind(message_id).fetch_one(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(value))
}
pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    auth::require(&state, &headers).await?;
    can_read(&state, &headers, id).await?;
    Ok(ws.on_upgrade(move |socket| connection(socket, state, headers, id)))
}
async fn connection(mut socket: WebSocket, state: AppState, headers: HeaderMap, id: i64) {
    let mut after_id = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(max(id),0) FROM chat_messages WHERE channel_id=$1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            incoming=socket.recv()=>{match incoming{Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,Some(Ok(Message::Ping(payload)))=>{if socket.send(Message::Pong(payload)).await.is_err(){break;}},_=>{}}},
            _=interval.tick()=>{
                if auth::require(&state,&headers).await.is_err()||can_read(&state,&headers,id).await.is_err(){let _=socket.send(Message::Close(None)).await;break;}
                let items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE channel_id=$1 AND m.id>$2 ORDER BY m.id LIMIT 100").bind(id).bind(after_id).fetch_all(&state.db).await;
                match items{Ok(items) if !items.is_empty()=>{after_id=items.last().and_then(|v|v["id"].as_i64()).unwrap_or(after_id);if socket.send(Message::Text(json!({"type":"messages","items":items}).to_string().into())).await.is_err(){break;}},Err(_)=>break,_=>{}}
            }
        }
    }
}
