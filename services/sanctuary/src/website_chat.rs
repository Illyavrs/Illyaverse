//! Website conversations use the original native PM bridge and commit events.
#[cfg(test)]
#[path = "website_chat_tests.rs"]
mod integration_tests;
use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use illya_domain::has_permission;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::time::{Duration, Instant};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    before_id: Option<i64>,
    after_id: Option<i64>,
}
impl HistoryQuery {
    fn bounds(&self) -> ApiResult<(i64, i64, i64)> {
        if self.page == Some(0)
            || self.page.is_some_and(|p| p > 1_000_000)
            || self.page_size.is_some_and(|s| s == 0 || s > 100)
            || self.before_id.is_some_and(|id| id <= 0)
            || self.after_id.is_some_and(|id| id < 0)
            || (self.before_id.is_some() && self.after_id.is_some())
        {
            return Err(ApiError::bad("Invalid chat pagination or cursor."));
        }
        let page = i64::from(self.page.unwrap_or(1));
        let size = i64::from(self.page_size.unwrap_or(50));
        Ok((page, size, (page - 1) * size))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateConversation {
    target_username: Option<String>,
    target_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewMessage {
    pub content: String,
    #[serde(default)]
    pub is_action: bool,
    #[serde(default)]
    pub client_uuid: Option<String>,
}
impl NewMessage {
    fn validate(&self, private: bool) -> ApiResult<(&str, Option<&str>)> {
        let content = self.content.trim();
        if content.is_empty()
            || content.chars().count() > 1000
            || content
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(ApiError::bad("Message must contain 1–1,000 characters."));
        }
        let uuid = self.client_uuid.as_deref().filter(|v| !v.is_empty());
        if (private && uuid.is_none())
            || uuid.is_some_and(|v| {
                v.len() > 128
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
            })
        {
            return Err(ApiError::bad(
                "A valid client message identifier is required.",
            ));
        }
        Ok((content, uuid))
    }
}

pub(crate) async fn actor_limit(state: &AppState, user: i64, scope: &str) -> ApiResult<()> {
    let mut limits = state.limits.lock().await;
    if limits.len() >= 10_000 {
        limits.retain(|_, (time, _)| time.elapsed() < Duration::from_secs(600));
    }
    let key = format!("website-{scope}:user:{user}");
    if limits.len() >= 10_000 && !limits.contains_key(&key) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Please wait before trying again.".into(),
        ));
    }
    let entry = limits.entry(key).or_insert((Instant::now(), 0));
    if entry.0.elapsed() >= Duration::from_secs(60) {
        *entry = (Instant::now(), 0);
    }
    if entry.1 >= 30 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Please wait before trying again.".into(),
        ));
    }
    entry.1 += 1;
    Ok(())
}

// Participant identity, peer restriction and both block directions are resolved
// together. An administrator or an erroneous membership cannot grant access.
const PM_ACCESS: &str = "SELECT c.id,CASE WHEN c.dm_user_low=$2 THEN c.dm_user_high ELSE c.dm_user_low END AS peer_id FROM channels c JOIN users peer ON peer.id=CASE WHEN c.dm_user_low=$2 THEN c.dm_user_high ELSE c.dm_user_low END WHERE c.id=$1 AND c.kind='PM' AND $2 IN(c.dm_user_low,c.dm_user_high) AND NOT peer.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=peer.id) OR (r.target_id=$2 AND r.user_id=peer.id)))";
async fn peer(db: &PgPool, user: i64, id: i64) -> ApiResult<i64> {
    Ok(sqlx::query(PM_ACCESS)
        .bind(id)
        .bind(user)
        .fetch_optional(db)
        .await?
        .ok_or_else(ApiError::missing)?
        .get("peer_id"))
}
async fn locked_peer(tx: &mut Transaction<'_, Postgres>, user: i64, id: i64) -> ApiResult<i64> {
    Ok(sqlx::query(&format!("{PM_ACCESS} FOR SHARE OF c"))
        .bind(id)
        .bind(user)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(ApiError::missing)?
        .get("peer_id"))
}
async fn send_policy(
    tx: &mut Transaction<'_, Postgres>,
    sender: i64,
    recipient: i64,
) -> ApiResult<()> {
    let allowed: bool = sqlx::query_scalar("SELECT NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$1 AND r.target_id=$2) OR (r.user_id=$2 AND r.target_id=$1))) FROM users u WHERE u.id=$2")
        .bind(sender).bind(recipient).fetch_optional(&mut **tx).await?.unwrap_or(false);
    if !allowed {
        return Err(ApiError::missing());
    }
    if friends_only(tx, sender, recipient).await? {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "recipient_accepts_friends_only",
            "This player accepts private messages from friends only.".into(),
        ));
    }
    Ok(())
}
async fn friends_only(
    tx: &mut Transaction<'_, Postgres>,
    sender: i64,
    recipient: i64,
) -> ApiResult<bool> {
    Ok(sqlx::query_scalar("SELECT (COALESCE(preferences->>'pm_friends_only','false')='true' OR EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$2 AND expires_at>clock_timestamp() AND block_non_friend_dms)) AND NOT EXISTS(SELECT 1 FROM user_relationships WHERE user_id=$2 AND target_id=$1 AND relation_type='friend') FROM users WHERE id=$2")
        .bind(sender).bind(recipient).fetch_one(&mut **tx).await?)
}

// Reuse this predicate for stream delivery and cached-message reconciliation.
// It deliberately never admits either native private room sentinel.
const VISIBLE_MESSAGES: &str = "SELECT to_jsonb(m)||jsonb_build_object('username',sender.username,'avatar_url',sender.avatar_url) FROM chat_messages m JOIN users sender ON sender.id=m.user_id JOIN channels c ON c.id=m.channel_id WHERE m.id=ANY($1) AND NOT sender.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id))) AND ((c.kind='PUBLIC' AND (c.required_permission IS NULL OR (c.required_permission NOT IN('chat.pm','stable.room','lazer.room') AND (c.required_permission=ANY($3) OR '*'=ANY($3))))) OR (c.kind='PM' AND $2 IN(c.dm_user_low,c.dm_user_high) AND EXISTS(SELECT 1 FROM users peer WHERE peer.id=CASE WHEN c.dm_user_low=$2 THEN c.dm_user_high ELSE c.dm_user_low END AND NOT peer.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=peer.id) OR (r.target_id=$2 AND r.user_id=peer.id)))))) ORDER BY m.id";
pub(crate) async fn visible_message(
    state: &AppState,
    user: &auth::User,
    id: i64,
) -> ApiResult<Option<Value>> {
    Ok(sqlx::query_scalar(VISIBLE_MESSAGES)
        .bind(vec![id])
        .bind(user.id)
        .bind(&user.permissions)
        .fetch_optional(&state.db)
        .await?)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visibility {
    message_ids: Vec<i64>,
}
pub async fn visibility(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Visibility>,
) -> ApiResult {
    // The existing optional helper deliberately treats invalid credentials as
    // guests on public pages. Reconciliation must surface session loss so a
    // signed-in cache cannot silently keep private data under guest policy.
    let credentials = headers.contains_key(axum::http::header::AUTHORIZATION)
        || headers
            .get(axum::http::header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .any(|part| part.trim().starts_with("illya_session="))
            });
    let user = if credentials {
        Some(auth::require(&state, &headers).await?)
    } else {
        None
    };
    if input.message_ids.len() > 256 || input.message_ids.iter().any(|id| *id <= 0) {
        return Err(ApiError::bad(
            "Supply at most 256 positive message identifiers.",
        ));
    }
    let viewer = user.as_ref().map(|user| user.id);
    let permissions = user.map(|user| user.permissions).unwrap_or_default();
    let items = sqlx::query_scalar::<_, Value>(VISIBLE_MESSAGES)
        .bind(input.message_ids)
        .bind(viewer)
        .bind(&permissions)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(
        json!({"message_ids":items.iter().filter_map(|m|m["id"].as_i64()).collect::<Vec<_>>()}),
    ))
}

async fn conversation(state: &AppState, user: &auth::User, id: i64) -> ApiResult<Value> {
    let peer_id = peer(&state.db, user.id, id).await?;
    let peer: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'username',username,'avatar_url',avatar_url) FROM users WHERE id=$1 AND NOT is_restricted")
        .bind(peer_id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    let last_read: i64 = sqlx::query_scalar("SELECT COALESCE((SELECT last_read_id FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2),0)")
        .bind(user.id).bind(id).fetch_one(&state.db).await?;
    let preview: Option<Value> = sqlx::query_scalar("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id))) ORDER BY m.id DESC LIMIT 1")
        .bind(id).bind(user.id).fetch_optional(&state.db).await?;
    let unread: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND m.id>$3 AND m.user_id<>$2 AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id)))")
        .bind(id).bind(user.id).bind(last_read).fetch_one(&state.db).await?;
    let restriction = if !has_permission(&user.permissions, "chat.send") {
        Some("no_permission")
    } else {
        let mut tx = state.db.begin().await?;
        if friends_only(&mut tx, user.id, peer_id).await? {
            Some("friend_only")
        } else {
            None
        }
    };
    // Recheck the complete conversation policy after fetching its presentation.
    peer_check(state, user.id, id).await?;
    Ok(
        json!({"id":id,"peer":peer,"last_message":preview,"last_read_id":last_read,"unread_count":unread,"can_send":restriction.is_none(),"send_restriction":restriction}),
    )
}
async fn peer_check(state: &AppState, user: i64, id: i64) -> ApiResult<()> {
    peer(&state.db, user, id).await.map(|_| ())
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    let (p, size, offset) = query.bounds()?;
    if query.before_id.is_some() || query.after_id.is_some() {
        return Err(ApiError::bad("Conversation lists use page pagination."));
    }
    let source = "FROM channels c JOIN lazer_channel_members member ON member.channel_id=c.id AND member.user_id=$1 JOIN users peer ON peer.id=CASE WHEN c.dm_user_low=$1 THEN c.dm_user_high ELSE c.dm_user_low END WHERE c.kind='PM' AND $1 IN(c.dm_user_low,c.dm_user_high) AND NOT peer.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$1 AND r.target_id=peer.id) OR (r.target_id=$1 AND r.user_id=peer.id)))";
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) {source}"))
        .bind(user.id)
        .fetch_one(&state.db)
        .await?;
    let ids:Vec<i64>=sqlx::query_scalar(&format!("SELECT c.id {source} ORDER BY (SELECT max(m.id) FROM chat_messages m JOIN users sender ON sender.id=m.user_id WHERE m.channel_id=c.id AND NOT sender.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$1 AND r.target_id=m.user_id) OR (r.target_id=$1 AND r.user_id=m.user_id)))) DESC NULLS LAST,c.id DESC LIMIT $2 OFFSET $3"))
        .bind(user.id).bind(size).bind(offset).fetch_all(&state.db).await?;
    let mut items = Vec::new();
    for id in ids {
        match conversation(&state, &user, id).await {
            Ok(value) => items.push(value),
            Err(ApiError(StatusCode::NOT_FOUND, _, _)) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(page(items, total, p, size))
}
pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    Ok(Json(conversation(&state, &user, id).await?))
}
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateConversation>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("chat.send")?;
    let target = match (input.target_username, input.target_id) {
        (Some(name), None) if !name.trim().is_empty() && name.len() <= 24 => {
            sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE lower(username)=lower($1)")
                .bind(name.trim())
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(ApiError::missing)?
        }
        (None, Some(id)) if id > 0 => id,
        _ => {
            return Err(ApiError::bad(
                "Supply exactly one target username or identifier.",
            ));
        }
    };
    if user.id == target {
        return Err(ApiError::bad("You cannot message yourself."));
    }
    actor_limit(&state, user.id, "chat-create").await?;
    let mut tx = state.db.begin().await?;
    let actor =
        auth::mutation_actor(&mut tx, &user, &[target], auth::MutationScope::Permissions).await?;
    actor.require("chat.send")?;
    send_policy(&mut tx, actor.id, target).await?;
    let id: i64 = sqlx::query_scalar("SELECT illya_pm_channel($1,$2)")
        .bind(actor.id)
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(actor.id).bind(id).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    let user = auth::require(&state, &headers).await?;
    Ok(Json(conversation(&state, &user, id).await?))
}
pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    peer_check(&state, user.id, id).await?;
    let (p, size, offset) = query.bounds()?;
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id)))")
        .bind(id).bind(user.id).fetch_one(&state.db).await?;
    let mut items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.channel_id=$1 AND ($3::bigint IS NULL OR m.id<$3) AND ($4::bigint IS NULL OR m.id>$4) AND NOT u.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$2 AND r.target_id=m.user_id) OR (r.target_id=$2 AND r.user_id=m.user_id))) ORDER BY m.id DESC LIMIT $5 OFFSET $6")
        .bind(id).bind(user.id).bind(query.before_id).bind(query.after_id).bind(size).bind(offset).fetch_all(&state.db).await?;
    items.reverse();
    peer_check(&state, user.id, id).await?;
    Ok(page(items, total, p, size))
}
pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<NewMessage>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    Ok(Json(store_message(&state, &user, id, &input, true).await?))
}
pub(crate) async fn store_message(
    state: &AppState,
    user: &auth::User,
    id: i64,
    input: &NewMessage,
    private: bool,
) -> ApiResult<Value> {
    let (content, uuid) = input.validate(private)?;
    user.require("chat.send")?;
    let peer_id = if private {
        Some(peer(&state.db, user.id, id).await?)
    } else {
        None
    };
    actor_limit(state, user.id, "chat-send").await?;
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(
        &mut tx,
        user,
        &peer_id.into_iter().collect::<Vec<_>>(),
        auth::MutationScope::Permissions,
    )
    .await?;
    actor.require("chat.send")?;
    if private {
        if Some(locked_peer(&mut tx, actor.id, id).await?) != peer_id {
            return Err(ApiError::missing());
        }
    } else {
        let permission: Option<String> =
            sqlx::query_scalar("SELECT required_permission FROM channels WHERE id=$1 FOR SHARE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(ApiError::missing)?;
        if let Some(permission) = permission {
            if matches!(
                permission.as_str(),
                "chat.pm" | "stable.room" | "lazer.room"
            ) {
                return Err(ApiError::missing());
            }
            actor.require(&permission)?;
        }
    }
    // User locks serialize website/native retries. Restriction/block/channel
    // checks precede reuse; friend-only applies only to a new persisted send.
    if let Some(uuid) = uuid
        && let Some(existing) =
            sqlx::query("SELECT * FROM chat_messages WHERE user_id=$1 AND client_uuid=$2")
                .bind(actor.id)
                .bind(uuid)
                .fetch_optional(&mut *tx)
                .await?
    {
        if existing.get::<i64, _>("channel_id") != id
            || existing.get::<String, _>("content") != content
            || existing.get::<bool, _>("is_action") != input.is_action
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "message_uuid_conflict",
                "This message identifier was already used for a different message.".into(),
            ));
        }
        let value = message_value(&mut tx, existing.get("id")).await?;
        auth::ensure_mutation_actor(&mut tx, &actor).await?;
        tx.commit().await?;
        return Ok(value);
    }
    let message_id: i64 = if let Some(peer) = peer_id {
        send_policy(&mut tx, actor.id, peer).await?;
        let private_id:i64=sqlx::query_scalar("INSERT INTO protocol_private_messages(sender_id,recipient_id,content,is_action,client_uuid) VALUES($1,$2,$3,$4,$5) RETURNING id")
            .bind(actor.id).bind(peer).bind(content).bind(input.is_action).bind(uuid).fetch_one(&mut *tx).await?;
        sqlx::query_scalar("SELECT id FROM chat_messages WHERE private_message_id=$1")
            .bind(private_id)
            .fetch_one(&mut *tx)
            .await?
    } else {
        sqlx::query_scalar("INSERT INTO chat_messages(channel_id,user_id,content,is_action,client_uuid) VALUES($1,$2,$3,$4,$5) RETURNING id")
            .bind(id).bind(actor.id).bind(content).bind(input.is_action).bind(uuid).fetch_one(&mut *tx).await?
    };
    let value = message_value(&mut tx, message_id).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(value)
}
async fn message_value(tx: &mut Transaction<'_, Postgres>, id: i64) -> ApiResult<Value> {
    Ok(sqlx::query_scalar("SELECT to_jsonb(m)||jsonb_build_object('username',u.username,'avatar_url',u.avatar_url) FROM chat_messages m JOIN users u ON u.id=m.user_id WHERE m.id=$1")
        .bind(id).fetch_one(&mut **tx).await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadMarker {
    message_id: i64,
}
pub async fn mark_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<ReadMarker>,
) -> ApiResult<StatusCode> {
    let user = auth::require(&state, &headers).await?;
    if input.message_id <= 0 {
        return Err(ApiError::bad("Read markers must name a visible message."));
    }
    let other = peer(&state.db, user.id, id).await?;
    let mut tx = state.db.begin().await?;
    let actor =
        auth::mutation_actor(&mut tx, &user, &[other], auth::MutationScope::Permissions).await?;
    locked_peer(&mut tx, actor.id, id).await?;
    let updated=sqlx::query("UPDATE lazer_channel_members SET last_read_id=$3 WHERE user_id=$1 AND channel_id=$2 AND last_read_id<=$3 AND EXISTS(SELECT 1 FROM chat_messages m JOIN users sender ON sender.id=m.user_id WHERE m.id=$3 AND m.channel_id=$2 AND NOT sender.is_restricted AND NOT EXISTS(SELECT 1 FROM user_relationships r WHERE r.relation_type='block' AND ((r.user_id=$1 AND r.target_id=m.user_id) OR (r.target_id=$1 AND r.user_id=m.user_id))))")
        .bind(actor.id).bind(id).bind(input.message_id).execute(&mut *tx).await?.rows_affected();
    if updated != 1 {
        return Err(ApiError::bad(
            "Read markers cannot move backwards or name an unavailable message.",
        ));
    }
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn website_chat_inputs_are_strict_and_bounded() {
        assert!(
            serde_json::from_value::<NewMessage>(json!({"content":"hello","user_id":2})).is_err()
        );
        assert!(serde_json::from_value::<ReadMarker>(json!({"message_id":1,"user_id":2})).is_err());
        let input: NewMessage = serde_json::from_value(
            json!({"content":" hello ","client_uuid":"web:123","is_action":true}),
        )
        .unwrap();
        assert_eq!(input.validate(true).unwrap(), ("hello", Some("web:123")));
        let legacy: NewMessage = serde_json::from_value(json!({"content":"hello"})).unwrap();
        assert!(legacy.validate(false).is_ok());
        assert!(legacy.validate(true).is_err());
        for value in [
            json!({"page_size":101}),
            json!({"before_id":0}),
            json!({"after_id":-1}),
            json!({"before_id":5,"after_id":1}),
        ] {
            assert!(
                serde_json::from_value::<HistoryQuery>(value)
                    .unwrap()
                    .bounds()
                    .is_err()
            );
        }
    }
}
