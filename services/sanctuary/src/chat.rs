use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{Path, Query, State, WebSocketUpgrade},
    http::HeaderMap,
    response::Response,
};
use illya_domain::{Pagination, has_permission};
use serde::Deserialize;
use serde_json::Value;

#[cfg(test)]
#[path = "community_mutation_tests.rs"]
mod mutation_tests;

pub(crate) async fn can_read(state: &AppState, headers: &HeaderMap, id: i64) -> ApiResult<()> {
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
async fn fetch(state: &AppState, id: i64, viewer: Option<i64>, query: MessageQuery) -> ApiResult {
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id)))")
        .bind(id)
        .bind(viewer)
        .fetch_one(&state.db)
        .await?;
    let mut items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND ($2::bigint IS NULL OR m.id>$2) AND ($3::bigint IS NULL OR m.id<$3) AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$4 AND r.target_id=m.user_id) OR (r.target_id=$4 AND r.user_id=m.user_id))) ORDER BY m.id DESC LIMIT $5 OFFSET $6").bind(id).bind(query.after_id).bind(query.before_id).bind(viewer).bind(size).bind(offset).fetch_all(&state.db).await?;
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
    let viewer = auth::optional(&state, &headers).await?.map(|user| user.id);
    fetch(&state, id, viewer, query).await
}
pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<crate::website_chat::NewMessage>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    Ok(Json(
        crate::website_chat::store_message(&state, &user, id, &input, false).await?,
    ))
}
pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    crate::website_chat_runtime::channel_stream(State(state), headers, Path(id), ws).await
}
