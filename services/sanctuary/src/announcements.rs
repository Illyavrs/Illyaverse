use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use illya_domain::Pagination;
use serde::Deserialize;
use serde_json::{Value, json};

// Expose only the creator's public identity, never the complete users row.
const RECORD: &str = "jsonb_build_object(\
    'id',a.id,'title',a.title,'body',a.body,'status',a.status,'revision',a.revision,\
    'created_at',a.created_at,'updated_at',a.updated_at,'published_at',a.published_at,\
    'author',jsonb_build_object('id',u.id,'username',u.username,'avatar_url',u.avatar_url))";

#[derive(Deserialize)]
pub struct ListQuery {
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize)]
pub struct AdminListQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    title: String,
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateInput {
    revision: i64,
    title: String,
    body: String,
    status: String,
}

fn validate_text(title: &str, body: &str) -> ApiResult<(String, String)> {
    // CRLF is the sole accepted carriage-return encoding for the plain text body.
    let body = body.replace("\r\n", "\n");
    if title.chars().any(char::is_control)
        || body
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
    {
        return Err(ApiError::bad(
            "Announcement text contains unsupported control characters.",
        ));
    }
    let title = title.trim();
    let body = body.trim();
    if !(1..=160).contains(&title.chars().count()) {
        return Err(ApiError::bad(
            "Announcement title must be 1–160 characters.",
        ));
    }
    if !(1..=20000).contains(&body.chars().count()) || body.len() > 64 * 1024 {
        return Err(ApiError::bad(
            "Announcement body must be 1–20,000 characters and at most 64 KiB.",
        ));
    }
    Ok((title.into(), body.into()))
}

fn validate_status(status: &str) -> ApiResult<()> {
    if matches!(status, "draft" | "published" | "archived") {
        Ok(())
    } else {
        Err(ApiError::bad("Unknown announcement status."))
    }
}

pub async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> ApiResult {
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM announcements WHERE status='published'")
            .fetch_one(&state.db)
            .await?;
    let items = sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {RECORD} FROM announcements a JOIN users u ON u.id=a.created_by \
         WHERE a.status='published' ORDER BY a.published_at DESC,a.id DESC LIMIT $1 OFFSET $2"
    ))
    .bind(size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(page(items, total, p, size))
}

pub async fn get(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let item = sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {RECORD} FROM announcements a JOIN users u ON u.id=a.created_by \
         WHERE a.id=$1 AND a.status='published'"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    Ok(Json(item))
}

pub async fn admin_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminListQuery>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("announcements.manage")?;
    let status = query.status.as_deref().unwrap_or("all");
    if status != "all" {
        validate_status(status)?;
    }
    let filter = (status != "all").then_some(status);
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM announcements WHERE ($1::text IS NULL OR status=$1)",
    )
    .bind(filter)
    .fetch_one(&state.db)
    .await?;
    let items = sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {RECORD} FROM announcements a JOIN users u ON u.id=a.created_by \
         WHERE ($1::text IS NULL OR a.status=$1) ORDER BY a.updated_at DESC,a.id DESC \
         LIMIT $2 OFFSET $3"
    ))
    .bind(filter)
    .bind(size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(page(items, total, p, size))
}

pub async fn admin_get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("announcements.manage")?;
    let item = sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {RECORD} FROM announcements a JOIN users u ON u.id=a.created_by WHERE a.id=$1"
    ))
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    Ok(Json(item))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateInput>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let snapshot = auth::require(&state, &headers).await?;
    snapshot.require("announcements.manage")?;
    let (title, body) = validate_text(&input.title, &input.body)?;
    let mut tx = state.db.begin().await?;
    let actor =
        auth::mutation_actor(&mut tx, &snapshot, &[], auth::MutationScope::Permissions).await?;
    actor.require("announcements.manage")?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO announcements(title,body,created_by,updated_by) VALUES($1,$2,$3,$3) RETURNING id",
    )
    .bind(title)
    .bind(body)
    .bind(actor.id)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'announcements.create','announcement',$2,$3)")
        .bind(actor.id)
        .bind(id.to_string())
        .bind(json!({"id":id,"revision":1,"status":"draft"}))
        .execute(&mut *tx)
        .await?;
    let item = record(&mut tx, id).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(item)))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<UpdateInput>,
) -> ApiResult {
    let snapshot = auth::require(&state, &headers).await?;
    snapshot.require("announcements.manage")?;
    let (title, body) = validate_text(&input.title, &input.body)?;
    validate_status(&input.status)?;
    if input.revision < 1 {
        return Err(ApiError::bad("Announcement revision must be positive."));
    }
    let mut tx = state.db.begin().await?;
    let actor =
        auth::mutation_actor(&mut tx, &snapshot, &[], auth::MutationScope::Permissions).await?;
    actor.require("announcements.manage")?;
    // Actor/session/permission locks always precede the announcement row.
    let (revision, status): (i64, String) =
        sqlx::query_as("SELECT revision,status FROM announcements WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(ApiError::missing)?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    if revision != input.revision {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "announcement_conflict",
            "This announcement changed. Reload it before saving.".into(),
        ));
    }
    let next_revision = revision
        .checked_add(1)
        .ok_or_else(|| ApiError::bad("The announcement revision is exhausted."))?;
    sqlx::query(
        "UPDATE announcements SET title=$2,body=$3,status=$4,revision=$6,updated_by=$5,\
         updated_at=clock_timestamp(),published_at=CASE WHEN $4='published' AND status<>'published' \
         THEN clock_timestamp() ELSE published_at END WHERE id=$1",
    )
    .bind(id)
    .bind(title)
    .bind(body)
    .bind(&input.status)
    .bind(actor.id)
    .bind(next_revision)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'announcements.update','announcement',$2,$3)")
        .bind(actor.id)
        .bind(id.to_string())
        .bind(json!({"id":id,"revision":next_revision,"status":input.status,"previous_revision":revision,"previous_status":status}))
        .execute(&mut *tx)
        .await?;
    let item = record(&mut tx, id).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(Json(item))
}

async fn record(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: i64) -> ApiResult<Value> {
    Ok(sqlx::query_scalar::<_, Value>(&format!(
        "SELECT {RECORD} FROM announcements a JOIN users u ON u.id=a.created_by WHERE a.id=$1"
    ))
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_text_normalizes_and_enforces_character_and_byte_limits() {
        assert_eq!(
            validate_text("  Announcement  ", " \r\n First\tline\r\nSecond \n ").unwrap(),
            ("Announcement".into(), "First\tline\nSecond".into())
        );
        for title in [
            "".into(),
            "   ".into(),
            "a".repeat(161),
            "a\nb".into(),
            "a\tb".into(),
            "a\u{007f}b".into(),
        ] {
            assert!(validate_text(&title, "body").is_err(), "{title:?}");
        }
        for body in [
            "".into(),
            " \t\n".into(),
            "a".repeat(20001),
            "😀".repeat(16385),
            "a\rb".into(),
            "a\u{0000}b".into(),
            "a\u{0085}b".into(),
        ] {
            assert!(validate_text("title", &body).is_err());
        }
        assert!(validate_text(&"界".repeat(160), &"a".repeat(20000)).is_ok());
        assert!(validate_text("title", &"😀".repeat(16384)).is_ok());
        assert!(validate_text("<plain title>", "<script>plain text</script>").is_ok());
    }

    #[test]
    fn announcement_status_and_complete_inputs_are_validated() {
        for status in ["draft", "published", "archived"] {
            assert!(validate_status(status).is_ok());
        }
        for status in ["all", "", "publish", "Published", " published "] {
            assert!(validate_status(status).is_err());
        }
        for value in [
            json!({"title":"title","body":"body","status":"published"}),
            json!({"title":"title","body":"body","author_id":1}),
        ] {
            assert!(serde_json::from_value::<CreateInput>(value).is_err());
        }
        for value in [
            json!({"title":"title","body":"body","status":"draft"}),
            json!({"revision":1,"title":"title","status":"draft"}),
            json!({"revision":1,"title":"title","body":"body","status":"draft","author_id":1}),
        ] {
            assert!(serde_json::from_value::<UpdateInput>(value).is_err());
        }
    }
}
