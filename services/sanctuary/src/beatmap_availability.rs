//! Reversible set availability, independent of per-difficulty ranking status.
use crate::{ApiError, ApiResult, AppState, auth};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;

pub(crate) fn withdrawn() -> ApiError {
    ApiError(
        StatusCode::GONE,
        "beatmap_withdrawn",
        "This beatmap set has been withdrawn.".into(),
    )
}

pub(crate) async fn require_active_set(db: &PgPool, id: i64) -> ApiResult<()> {
    if sqlx::query_scalar::<_, bool>("SELECT illya_beatmapset_active($1)")
        .bind(id)
        .fetch_one(db)
        .await?
    {
        Ok(())
    } else {
        Err(withdrawn())
    }
}

pub(crate) async fn require_active_map(db: &PgPool, id: i64) -> ApiResult<i64> {
    let set: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmaps WHERE id=$1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(ApiError::missing)?;
    require_active_set(db, set).await?;
    Ok(set)
}

pub(crate) async fn lock_active_set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> ApiResult<()> {
    sqlx::query("SELECT illya_lock_active_beatmapset($1)")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn lock_exclusive_set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: i64,
) -> ApiResult<()> {
    let key =
        i32::try_from(id).map_err(|_| ApiError::bad("Supply a valid native beatmap set ID."))?;
    sqlx::query("SELECT pg_advisory_xact_lock($1,$2)")
        .bind(0x494c4f53_i32)
        .bind(key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// The absent row is the original active state. Public clients never receive
// the moderation reason or actor identity.
pub(crate) const PUBLIC_STATE: &str = "jsonb_build_object('withdrawn',COALESCE(av.withdrawn,false),'revision',COALESCE(av.revision,0),'changed_at',av.changed_at)";
pub(crate) const ADMIN_STATE: &str = "jsonb_build_object('withdrawn',COALESCE(av.withdrawn,false),'revision',COALESCE(av.revision,0),'reason',av.reason,'changed_at',av.changed_at)";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Patch {
    withdrawn: bool,
    expected_revision: i64,
    reason: String,
}

pub(crate) async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<Patch>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.moderate")?;
    if !(1..=i64::from(i32::MAX)).contains(&id) || input.expected_revision < 0 {
        return Err(ApiError::bad(
            "Supply a valid native beatmap set ID and revision.",
        ));
    }
    let reason = input.reason.trim();
    if reason.is_empty()
        || reason.chars().count() > 500
        || reason.len() > 2000
        || reason.contains('\0')
    {
        return Err(ApiError::bad(
            "Supply a reason between 1 and 500 characters.",
        ));
    }
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Permissions).await?;
    actor.require("beatmaps.moderate")?;
    // No affected-player, map, score, room or token locks are acquired here.
    // Readers/publishers take the same set namespace before their own rows.
    lock_exclusive_set(&mut tx, id).await?;
    if id >= 2_000_000_000
        && !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM beatmap_packages WHERE native_set_id=$1)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(ApiError::missing());
    }
    let before: Value = sqlx::query_scalar("SELECT jsonb_build_object('withdrawn',withdrawn,'revision',revision,'reason',reason,'changed_at',changed_at) FROM beatmapset_availability WHERE native_set_id=$1 FOR UPDATE")
        .bind(id).fetch_optional(&mut *tx).await?.unwrap_or_else(|| json!({"withdrawn":false,"revision":0,"reason":null,"changed_at":null}));
    if before["revision"].as_i64() != Some(input.expected_revision) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "availability_conflict",
            "Beatmap set availability changed. Reload before saving.".into(),
        ));
    }
    if before["withdrawn"].as_bool() == Some(input.withdrawn) {
        auth::ensure_mutation_actor(&mut tx, &actor).await?;
        tx.commit().await?;
        return Ok(Json(json!({"native_set_id":id,"availability":before})));
    }
    let statement = if input.expected_revision == 0 {
        "INSERT INTO beatmapset_availability(native_set_id,withdrawn,revision,reason,actor_id,changed_at) VALUES($1,$2,$3,$4,$5,clock_timestamp()) RETURNING jsonb_build_object('withdrawn',withdrawn,'revision',revision,'reason',reason,'changed_at',changed_at)"
    } else {
        "UPDATE beatmapset_availability SET withdrawn=$2,revision=$3,reason=$4,actor_id=$5,changed_at=clock_timestamp() WHERE native_set_id=$1 RETURNING jsonb_build_object('withdrawn',withdrawn,'revision',revision,'reason',reason,'changed_at',changed_at)"
    };
    let after: Value = sqlx::query_scalar(statement)
        .bind(id)
        .bind(input.withdrawn)
        .bind(
            input
                .expected_revision
                .checked_add(1)
                .ok_or_else(|| ApiError::bad("The revision is exhausted."))?,
        )
        .bind(reason)
        .bind(actor.id)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,$2,'beatmapset',$3,$4)")
        .bind(actor.id).bind(if input.withdrawn {"beatmaps.withdraw"} else {"beatmaps.restore"}).bind(id.to_string())
        .bind(json!({"before":before,"after":after,"reason":reason,"revision":after["revision"]})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(Json(json!({"native_set_id":id,"availability":after})))
}
