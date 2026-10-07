use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use illya_domain::{PERMISSIONS, Pagination, has_permission};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct UserDirectoryQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    search: Option<String>,
}

pub async fn users(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UserDirectoryQuery>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    if !["roles.manage", "users.moderate", "users.sessions.revoke"]
        .iter()
        .any(|permission| has_permission(&user.permissions, permission))
    {
        return Err(ApiError::forbidden());
    }
    let include_email = has_permission(&user.permissions, "users.moderate");
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if search.is_some_and(|s| s.chars().count() > 64 || s.chars().any(char::is_control)) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_search",
            "Search must be at most 64 characters without control characters.".into(),
        ));
    }
    let search_id = search
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|id| *id > 0);
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users u WHERE ($1::text IS NULL OR ($2::bigint IS NOT NULL AND u.id=$2) OR ($2::bigint IS NULL AND strpos(lower(u.username),lower($1))>0))",
    )
        .bind(search)
        .bind(search_id)
        .fetch_one(&state.db)
        .await?;
    let items = sqlx::query_scalar::<_, Value>(r#"
        SELECT jsonb_build_object(
            'id',u.id,'username',u.username,'country',u.country,'avatar_url',u.avatar_url,
            'is_restricted',u.is_restricted,'joined_at',u.joined_at,
            'roles',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',r.id,'name',r.name) ORDER BY r.id)
                FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=u.id),'[]'),
            'can_manage_roles',$6 AND authority.covered,
            'can_moderate',$7 AND u.id<>$9 AND (NOT authority.wildcard OR '*'=ANY($10)),
            'can_revoke_sessions',$8 AND authority.covered
        ) || CASE WHEN $3 THEN jsonb_build_object('email',u.email) ELSE '{}'::jsonb END
        FROM users u CROSS JOIN LATERAL (
            SELECT NOT EXISTS(
                SELECT 1 FROM roles r JOIN user_roles ur ON ur.role_id=r.id
                CROSS JOIN LATERAL unnest(r.permissions) permission
                WHERE ur.user_id=u.id AND NOT ('*'=ANY($10)) AND NOT (permission=ANY($10))
            ) covered, EXISTS(
                SELECT 1 FROM roles r JOIN user_roles ur ON ur.role_id=r.id
                WHERE ur.user_id=u.id AND '*'=ANY(r.permissions)
            ) wildcard
        ) authority
        WHERE ($4::text IS NULL OR ($5::bigint IS NOT NULL AND u.id=$5)
            OR ($5::bigint IS NULL AND strpos(lower(u.username),lower($4))>0))
        ORDER BY u.id LIMIT $1 OFFSET $2
        "#)
        .bind(size)
        .bind(offset)
        .bind(include_email)
        .bind(search)
        .bind(search_id)
        .bind(has_permission(&user.permissions, "roles.manage"))
        .bind(has_permission(&user.permissions, "users.moderate"))
        .bind(has_permission(&user.permissions, "users.sessions.revoke"))
        .bind(user.id)
        .bind(&user.permissions)
        .fetch_all(&state.db)
        .await?;
    Ok(page(items, total, p, size))
}

/// Serialize permission and membership changes, then lock users before sessions.
/// A users table lock can deadlock with recovery's user-row -> credential update.
pub(crate) async fn lock_actor(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &auth::User,
    target: Option<i64>,
) -> ApiResult<auth::MutationActor> {
    let targets = target.into_iter().collect::<Vec<_>>();
    auth::mutation_actor(tx, actor, &targets, auth::MutationScope::RoleChanges).await
}

async fn target_permissions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> ApiResult<Vec<String>> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT unnest(r.permissions) FROM roles r JOIN user_roles ur ON ur.role_id=r.id WHERE ur.user_id=$1",
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?)
}

async fn retain_administrator(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> ApiResult<()> {
    let admins:i64=sqlx::query_scalar("SELECT count(DISTINCT u.id) FROM users u JOIN user_roles ur ON ur.user_id=u.id JOIN roles r ON r.id=ur.role_id WHERE NOT u.is_restricted AND '*'=ANY(r.permissions)").fetch_one(&mut **tx).await?;
    if admins == 0 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "last_administrator",
            "At least one unrestricted administrator must remain.".into(),
        ));
    }
    Ok(())
}

fn builtin_role(id: i64, name: &str) -> bool {
    (1..=3).contains(&id) || matches!(name, "player" | "moderator" | "administrator")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPatch {
    roles: Option<Vec<i64>>,
    is_restricted: Option<bool>,
}
pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<UserPatch>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    if input.roles.is_some() {
        user.require("roles.manage")?;
    }
    if input.is_restricted.is_some() {
        user.require("users.moderate")?;
    }
    if input.roles.is_none() && input.is_restricted.is_none() {
        return Err(ApiError::bad("Choose a role or restriction change."));
    }
    if input.is_restricted == Some(true) && id == user.id {
        return Err(ApiError::bad("You cannot restrict your own account."));
    }
    let mut tx = state.db.begin().await?;
    let user = lock_actor(&mut tx, &user, Some(id)).await?;
    if input.roles.is_some() {
        user.require("roles.manage")?;
    }
    if input.is_restricted.is_some() {
        user.require("users.moderate")?;
    }
    let existing = target_permissions(&mut tx, id).await?;
    if existing.iter().any(|permission| permission == "*") {
        can_delegate(&user.permissions, &["*".into()])?;
    }
    if let Some(ref roles) = input.roles {
        // Removing a grant is privileged too: a limited manager must not be
        // able to strip a stronger operator's role by assigning weaker roles.
        can_delegate(&user.permissions, &existing)?;
        if roles.len() > 16 {
            return Err(ApiError::bad("Assign at most 16 roles."));
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM roles WHERE id=ANY($1)")
            .bind(roles)
            .fetch_one(&mut *tx)
            .await?;
        let distinct = roles.iter().collect::<std::collections::HashSet<_>>().len();
        if count as usize != distinct || distinct != roles.len() {
            return Err(ApiError::bad("Role IDs must exist and be unique."));
        }
        let delegated = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT unnest(permissions) FROM roles WHERE id=ANY($1)",
        )
        .bind(roles)
        .fetch_all(&mut *tx)
        .await?;
        can_delegate(&user.permissions, &delegated)?;
        sqlx::query("DELETE FROM user_roles WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO user_roles(user_id,role_id) SELECT $1,unnest($2::bigint[])")
            .bind(id)
            .bind(roles)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(restricted) = input.is_restricted {
        sqlx::query("UPDATE users SET is_restricted=$2 WHERE id=$1")
            .bind(id)
            .bind(restricted)
            .execute(&mut *tx)
            .await?;
        if restricted {
            sqlx::query("DELETE FROM sessions WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    retain_administrator(&mut tx).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.update','user',$2,$3)").bind(user.id).bind(id.to_string()).bind(json!({"roles":input.roles,"is_restricted":input.is_restricted})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn roles(State(state): State<AppState>, headers: HeaderMap) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    if !has_permission(&user.permissions, "admin.view")
        && !has_permission(&user.permissions, "roles.manage")
    {
        return Err(ApiError::forbidden());
    }
    let items = sqlx::query_scalar::<_, Value>("SELECT to_jsonb(r)||jsonb_build_object('membership_count',(SELECT count(*) FROM user_roles ur WHERE ur.role_id=r.id),'is_builtin',r.id BETWEEN 1 AND 3 OR r.name IN ('player','moderator','administrator')) FROM roles r ORDER BY id")
        .fetch_all(&state.db)
        .await?;
    let total = items.len() as i64;
    Ok(page(items, total, 1, total.max(1)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleInput {
    name: String,
    description: String,
    permissions: Vec<String>,
}
fn validate_role(input: &RoleInput) -> ApiResult<()> {
    if !(3..=32).contains(&input.name.len())
        || !input
            .name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        || input.description.len() > 500
    {
        return Err(ApiError::bad(
            "Role names must be 3–32 letters, numbers, underscores or hyphens.",
        ));
    }
    if input.permissions.len() > PERMISSIONS.len() + 1
        || input
            .permissions
            .iter()
            .any(|p| p != "*" && !PERMISSIONS.contains(&p.as_str()))
    {
        return Err(ApiError::bad("Role includes an unknown permission."));
    }
    Ok(())
}
fn can_delegate(effective: &[String], delegated: &[String]) -> ApiResult<()> {
    if delegated
        .iter()
        .any(|permission| !has_permission(effective, permission))
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "stronger_role",
            "Your permissions do not allow changes to this role or user.".into(),
        ));
    }
    Ok(())
}
pub async fn create_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RoleInput>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("roles.manage")?;
    validate_role(&input)?;
    can_delegate(&user.permissions, &input.permissions)?;
    let mut tx = state.db.begin().await?;
    let user = lock_actor(&mut tx, &user, None).await?;
    user.require("roles.manage")?;
    can_delegate(&user.permissions, &input.permissions)?;
    let value:Value=sqlx::query_scalar("INSERT INTO roles(name,description,permissions) VALUES($1,$2,$3) RETURNING to_jsonb(roles)||jsonb_build_object('membership_count',0,'is_builtin',id BETWEEN 1 AND 3 OR name IN ('player','moderator','administrator'))").bind(&input.name).bind(&input.description).bind(&input.permissions).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'roles.create','role',$2,$3)").bind(user.id).bind(value["id"].to_string()).bind(json!({"name":input.name,"permissions":input.permissions})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(value))
}
pub async fn update_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<RoleInput>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("roles.manage")?;
    validate_role(&input)?;
    can_delegate(&user.permissions, &input.permissions)?;
    let mut tx = state.db.begin().await?;
    let user = lock_actor(&mut tx, &user, None).await?;
    user.require("roles.manage")?;
    can_delegate(&user.permissions, &input.permissions)?;
    let name: String = sqlx::query_scalar("SELECT name FROM roles WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::missing)?;
    let existing: Vec<String> = sqlx::query_scalar("SELECT permissions FROM roles WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    can_delegate(&user.permissions, &existing)?;
    if builtin_role(id, &name) && input.name != name {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "builtin_role",
            "Built-in roles must retain their names.".into(),
        ));
    }
    if name == "administrator" && (input.name != "administrator" || input.permissions != ["*"]) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "builtin_role",
            "The administrator role must retain its name and full permission.".into(),
        ));
    }
    let value:Value=sqlx::query_scalar("UPDATE roles SET name=$2,description=$3,permissions=$4 WHERE id=$1 RETURNING to_jsonb(roles)||jsonb_build_object('membership_count',(SELECT count(*) FROM user_roles ur WHERE ur.role_id=roles.id),'is_builtin',id BETWEEN 1 AND 3 OR name IN ('player','moderator','administrator'))").bind(id).bind(&input.name).bind(&input.description).bind(&input.permissions).fetch_one(&mut *tx).await?;
    retain_administrator(&mut tx).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'roles.update','role',$2,$3)").bind(user.id).bind(id.to_string()).bind(json!({"name":input.name,"permissions":input.permissions})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(value))
}

pub async fn delete_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult {
    let actor = auth::require(&state, &headers).await?;
    actor.require("roles.manage")?;
    let mut tx = state.db.begin().await?;
    let actor = lock_actor(&mut tx, &actor, None).await?;
    actor.require("roles.manage")?;
    let value: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM roles r WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::missing)?;
    let name = value["name"].as_str().unwrap_or_default();
    if builtin_role(id, name) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "builtin_role",
            "Built-in roles cannot be deleted.".into(),
        ));
    }
    let existing: Vec<String> =
        serde_json::from_value(value["permissions"].clone()).map_err(ApiError::internal)?;
    can_delegate(&actor.permissions, &existing)?;
    let assigned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roles WHERE role_id=$1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if assigned {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "role_assigned",
            "Unassign this role from all users before deleting it.".into(),
        ));
    }
    sqlx::query("DELETE FROM roles WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    retain_administrator(&mut tx).await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'roles.delete','role',$2,$3)")
        .bind(actor.id).bind(id.to_string()).bind(value).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn revoke_user_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult {
    let actor = auth::require(&state, &headers).await?;
    actor.require("users.sessions.revoke")?;
    let mut tx = state.db.begin().await?;
    let actor = lock_actor(&mut tx, &actor, Some(id)).await?;
    actor.require("users.sessions.revoke")?;
    let existing = target_permissions(&mut tx, id).await?;
    can_delegate(&actor.permissions, &existing)?;
    let web = sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let stable = sqlx::query("DELETE FROM bancho_sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let lazer = sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let revoked = json!({"web":web,"stable":stable,"lazer":lazer});
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'users.sessions.revoke','user',$2,$3)")
        .bind(actor.id).bind(id.to_string()).bind(json!({"revoked":revoked})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true,"revoked":revoked})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn role_managers_cannot_delegate_permissions_they_do_not_hold() {
        let manager = vec!["roles.manage".into(), "admin.view".into()];
        assert!(can_delegate(&manager, &["roles.manage".into()]).is_ok());
        assert!(can_delegate(&manager, &["*".into()]).is_err());
        assert!(can_delegate(&manager, &["users.moderate".into()]).is_err());
        assert!(can_delegate(&["*".into()], &["users.moderate".into()]).is_ok());
    }
}
#[derive(Deserialize)]
pub struct MapPatch {
    status: String,
}
pub async fn update_map(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<MapPatch>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.moderate")?;
    if !matches!(
        input.status.as_str(),
        "pending" | "ranked" | "loved" | "unranked"
    ) {
        return Err(ApiError::bad("Unknown beatmap status."));
    }
    let mut tx = state.db.begin().await?;
    let user = lock_actor(&mut tx, &user, None).await?;
    user.require("beatmaps.moderate")?;
    let result = sqlx::query("UPDATE beatmaps SET status=$2 WHERE id=$1")
        .bind(id)
        .bind(&input.status)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.status','beatmap',$2,$3)").bind(user.id).bind(id.to_string()).bind(json!({"status":input.status})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(crate::maps::get(&state, id).await?))
}
#[derive(Deserialize)]
pub struct ScorePatch {
    verification: String,
}
pub async fn update_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<ScorePatch>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("scores.moderate")?;
    if !matches!(
        input.verification.as_str(),
        "pending" | "verified" | "rejected"
    ) {
        return Err(ApiError::bad("Unknown score verification status."));
    }
    let mut tx = state.db.begin().await?;
    let user = lock_actor(&mut tx, &user, None).await?;
    user.require("scores.moderate")?;
    let result = sqlx::query("UPDATE scores SET verification=$2 WHERE id=$1")
        .bind(id)
        .bind(&input.verification)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.verify','score',$2,$3)").bind(user.id).bind(id.to_string()).bind(json!({"verification":input.verification})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &user).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn audit_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(pagination): Query<Pagination>,
) -> ApiResult {
    auth::require(&state, &headers)
        .await?
        .require("audit.read")?;
    let (p, size, offset) = pagination.bounds();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events")
        .fetch_one(&state.db)
        .await?;
    let items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(a)||jsonb_build_object('actor_username',u.username) FROM audit_events a LEFT JOIN users u ON u.id=a.actor_id ORDER BY a.id DESC LIMIT $1 OFFSET $2").bind(size).bind(offset).fetch_all(&state.db).await?;
    Ok(page(items, total, p, size))
}
