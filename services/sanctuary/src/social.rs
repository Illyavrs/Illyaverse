use crate::{ApiError, ApiResult, AppState, auth};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Friend,
    Block,
}
impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Friend => "friend",
            Self::Block => "block",
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    relation_type: Kind,
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    let records = sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('target_id',u.id,'relation_type',r.relation_type,'mutual',EXISTS(SELECT 1 FROM user_relationships inverse WHERE inverse.user_id=u.id AND inverse.target_id=$1 AND inverse.relation_type='friend'),'target',jsonb_build_object('id',u.id,'username',u.username,'avatar_url',u.avatar_url,'country',u.country)) FROM user_relationships r JOIN users u ON u.id=r.target_id WHERE r.user_id=$1 AND NOT u.is_restricted ORDER BY lower(u.username),u.id")
        .bind(user.id).fetch_all(&state.db).await?;
    let (friends, blocks): (Vec<_>, Vec<_>) = records
        .into_iter()
        .partition(|r| r["relation_type"] == "friend");
    Ok(Json(json!({"friends":friends,"blocks":blocks})))
}
pub async fn change(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target): Path<i64>,
    Json(body): Json<Change>,
) -> ApiResult {
    alter(&state, &headers, target, body.relation_type, false).await?;
    Ok(Json(json!({"updated":true})))
}
pub async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(target): Path<i64>,
    Query(body): Query<Change>,
) -> ApiResult<StatusCode> {
    alter(&state, &headers, target, body.relation_type, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn alter(
    state: &AppState,
    headers: &HeaderMap,
    target: i64,
    kind: Kind,
    remove: bool,
) -> ApiResult<()> {
    let user = auth::require(state, headers).await?;
    user.require("social.manage")?;
    if target <= 0 || target == user.id {
        return Err(ApiError::bad("Choose another player."));
    }
    let mut tx = state.db.begin().await?;
    // All transports take this lock order, preventing races between block and friend requests.
    let locked = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE id=ANY($1) AND NOT is_restricted ORDER BY id FOR UPDATE",
    )
    .bind(vec![user.id, target])
    .fetch_all(&mut *tx)
    .await?;
    if locked.len() != 2 {
        return Err(ApiError::missing());
    }
    if remove {
        sqlx::query(
            "DELETE FROM user_relationships WHERE user_id=$1 AND target_id=$2 AND relation_type=$3",
        )
        .bind(user.id)
        .bind(target)
        .bind(kind.as_str())
        .execute(&mut *tx)
        .await?;
    } else {
        if matches!(kind, Kind::Friend) {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='block')")
                .bind(user.id).bind(target).fetch_one(&mut *tx).await?;
            if blocked {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "relationship_blocked",
                    "This player relationship is blocked.".into(),
                ));
            }
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_relationships WHERE user_id=$1 AND relation_type=$2 AND target_id<>$3")
            .bind(user.id).bind(kind.as_str()).bind(target).fetch_one(&mut *tx).await?;
        if count >= 4096 {
            return Err(ApiError::bad("The relationship limit has been reached."));
        }
        if matches!(kind, Kind::Block) {
            sqlx::query("DELETE FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='friend'")
                .bind(user.id).bind(target).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO user_relationships(user_id,target_id,relation_type) VALUES($1,$2,$3) ON CONFLICT(user_id,target_id) DO UPDATE SET relation_type=EXCLUDED.relation_type")
            .bind(user.id).bind(target).bind(kind.as_str()).execute(&mut *tx).await?;
    }
    for player in [user.id, target] {
        let ids = sqlx::query_scalar::<_, i64>("SELECT r.target_id FROM user_relationships r JOIN users u ON u.id=r.target_id WHERE r.user_id=$1 AND r.relation_type='friend' AND NOT u.is_restricted ORDER BY r.target_id LIMIT 4096")
            .bind(player).fetch_all(&mut *tx).await?;
        let ids: Vec<i32> = ids
            .into_iter()
            .map(i32::try_from)
            .collect::<Result<_, _>>()
            .map_err(ApiError::internal)?;
        let packet = resonance_protocol::stable::friends_list(&ids).map_err(ApiError::internal)?;
        sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$1 AND expires_at>now())")
            .bind(player).bind(packet).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
