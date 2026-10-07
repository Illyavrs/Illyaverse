//! Bounded, restart-safe conversion of historical score rows. This worker never
//! changes raw scores, PP, review decisions, ownership, or replay files.
use crate::{ApiError, ApiResult, AppState, scores};
use sqlx::Row;
use std::path::Path;

pub(crate) fn start(state: AppState) {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(30));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            timer.tick().await;
            if let Err(error) = recover_batch(&state).await {
                tracing::warn!(code = error.1, "score conversion recovery deferred");
            }
        }
    });
}

pub(crate) async fn recover_batch(state: &AppState) -> ApiResult<u32> {
    let Ok(_permit) = state.upload_permit() else {
        return Ok(0);
    };
    let mut tx = state.db.begin().await?;
    let rows = sqlx::query(
        "SELECT s.*,b.osu_path,COALESCE(s.native_submission,t.submission) recovered_native_submission
         FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id
         LEFT JOIN LATERAL(SELECT submission FROM lazer_score_tokens WHERE score_id=s.id ORDER BY id DESC LIMIT 1)t ON true
         WHERE s.score_scale_status='unavailable' AND s.score_scale_version IS NULL
         ORDER BY s.id LIMIT 16 FOR UPDATE OF s SKIP LOCKED",
    ).fetch_all(&mut *tx).await?;
    let processed = rows.len() as u32;
    for row in rows {
        let id: i64 = row.get("id");
        let client: String = row.get("client");
        let path: String = row.get("osu_path");
        let calculated = async {
            if !matches!(client.as_str(), "stable" | "lazer") {
                return Err(ApiError::bad("unsupported_score_source"));
            }
            if Path::new(&path).is_absolute() || path.contains("..") {
                return Err(ApiError::bad("unsafe_stored_map_path"));
            }
            let bytes = tokio::fs::read(state.maps_root.join(&path)).await?;
            let input = scores::from_stored_row(&row)?;
            let lazer = client == "lazer";
            tokio::task::spawn_blocking(move || scores::calculate_score(&bytes, &input, lazer))
                .await
                .map_err(ApiError::internal)?
        }
        .await;
        match calculated {
            Ok(result) => {
                let scale = result.scales;
                sqlx::query("UPDATE scores SET native_submission=COALESCE($2,native_submission),standardised_score=$3,standardised_without_mods=$4,classic_score=$5,native_basic_maximum=$6,score_scale_status=$7,score_scale_version=$8,score_scale_reason=$9,legacy_attributes=$10 WHERE id=$1")
                    .bind(id).bind(scale.native_submission).bind(scale.standardised_score).bind(scale.standardised_without_mods).bind(scale.classic_score).bind(scale.native_basic_maximum).bind(scale.status).bind(scale.version.unwrap_or_else(||"unavailable.v1".into())).bind(scale.reason).bind(scale.legacy_attributes).execute(&mut *tx).await?;
            }
            Err(error) => {
                // A bounded attempt leaves an explicit unsupported marker. An
                // operator may clear this marker after restoring map/context.
                sqlx::query("UPDATE scores SET score_scale_version='recovery_unavailable.v1',score_scale_reason=$2 WHERE id=$1")
                    .bind(id).bind(if client=="api" {"unsupported_score_source"} else {"stored_map_or_context_invalid"}).execute(&mut *tx).await?;
                tracing::warn!(
                    score_id = id,
                    code = error.1,
                    "historical score conversion unavailable"
                );
            }
        }
    }
    tx.commit().await?;
    Ok(processed)
}
