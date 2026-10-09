//! Own-account session visibility and revocation. Session IDs are opaque row
//! identifiers, never bearer tokens; only the authenticated owner's rows leave
//! this module. Native sessions use the same authoritative tables as clients.
use crate::{ApiError, ApiResult, AppState, auth};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

const LIST_LIMIT: usize = 100;

#[derive(Serialize)]
struct Session {
    id: Uuid,
    client: String,
    current: bool,
    created_at: DateTime<Utc>,
    /// Lazer's refresh expiry: an expired access token can still be refreshed.
    expires_at: DateTime<Utc>,
    /// Only lazer records this. Other clients return null, rather than an
    /// invented last-used time or device information that we do not collect.
    last_seen_at: Option<DateTime<Utc>>,
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let user = auth::require(&state, &headers).await?;
    let rows = sqlx::query(
        "WITH owned AS (
            SELECT id,'web'::text AS client,id=$2 AS current,created_at,expires_at,NULL::timestamptz AS last_seen_at
            FROM sessions WHERE user_id=$1 AND expires_at>clock_timestamp()
            UNION ALL
            SELECT id,'stable',false,created_at,expires_at,NULL::timestamptz
            FROM bancho_sessions WHERE user_id=$1 AND expires_at>clock_timestamp()
            UNION ALL
            SELECT id,'lazer',false,created_at,refresh_expires_at,last_seen_at
            FROM lazer_sessions WHERE user_id=$1 AND refresh_expires_at>clock_timestamp()
        )
        SELECT id,client,current,created_at,expires_at,last_seen_at FROM owned
        WHERE EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id
            WHERE s.id=$2 AND s.user_id=$1 AND s.expires_at>clock_timestamp() AND NOT u.is_restricted)
        ORDER BY current DESC,created_at DESC,client,id LIMIT $3",
    )
    .bind(user.id)
    .bind(user.session)
    .bind((LIST_LIMIT + 1) as i64)
    .fetch_all(&state.db)
    .await?;
    // The current web session must be present if its authority still exists.
    // This also catches revocation between the initial auth read and listing.
    if rows.is_empty() {
        return Err(ApiError::unauthorized());
    }
    let has_more = rows.len() > LIST_LIMIT;
    let sessions: Vec<Session> = rows
        .into_iter()
        .take(LIST_LIMIT)
        .map(|row| Session {
            id: row.get("id"),
            client: row.get("client"),
            current: row.get("current"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            last_seen_at: row.get("last_seen_at"),
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(json!({"sessions":sessions,"has_more":has_more})),
    )
        .into_response())
}

pub async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((client, id)): Path<(String, Uuid)>,
) -> ApiResult<Response> {
    let user = auth::require(&state, &headers).await?;
    let delete = match client.as_str() {
        "web" => "DELETE FROM sessions WHERE user_id=$1 AND id=$2 AND expires_at>clock_timestamp()",
        "stable" => {
            "DELETE FROM bancho_sessions WHERE user_id=$1 AND id=$2 AND expires_at>clock_timestamp()"
        }
        "lazer" => {
            "DELETE FROM lazer_sessions WHERE user_id=$1 AND id=$2 AND refresh_expires_at>clock_timestamp()"
        }
        _ => return Err(ApiError::missing()),
    };
    let mut tx = state.db.begin().await?;
    let user = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Session).await?;
    let changed = sqlx::query(delete)
        .bind(user.id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    if changed.rows_affected() != 1 {
        return Err(ApiError::missing());
    }
    let current_revoked = client == "web" && id == user.session;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.sessions.revoke_own','session',$2,$3)")
        .bind(user.id)
        .bind(id.to_string())
        .bind(json!({"client":client,"current_revoked":current_revoked}))
        .execute(&mut *tx)
        .await?;
    // The original authority deadline is retained even for deliberate logout.
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    // Website streams recheck this same table before delivery and on their
    // auth heartbeat. Native poll, access and refresh use their deleted row.
    // An already authenticated request may finish; this does not cancel work
    // that the client protocol accepted before revocation.
    let body = Json(json!({"ok":true,"current_revoked":current_revoked}));
    if current_revoked {
        let cookie = format!(
            "illya_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            if state.secure_cookie { "; Secure" } else { "" }
        );
        Ok(([(header::SET_COOKIE, cookie)], body).into_response())
    } else {
        Ok(body.into_response())
    }
}

pub async fn revoke_others(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    let mut tx = state.db.begin().await?;
    let user = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Session).await?;
    auth::revoke_login_sessions(&mut tx, user.id, Some(user.session)).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.sessions.revoke_others','user',$2,'{\"current_web_session_retained\":true}')")
        .bind(user.id)
        .bind(user.id.to_string())
        .execute(&mut *tx)
        .await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[cfg(test)]
#[path = "account_sessions_tests.rs"]
mod tests;
