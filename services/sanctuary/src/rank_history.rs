//! Latest observed rankings per UTC day. All rankings reuse the public source.
use crate::{ApiError, ApiResult, AppState, scores};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{DateTime, NaiveDate, Utc};
use illya_domain::ScoreVariant;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;

pub(crate) const COLLECTION_INTERVAL_SECONDS: u64 = 1800;
const POLL_INTERVAL_SECONDS: u64 = 60;
// A separate two-integer namespace from beatmap admission and moderation locks.
pub(crate) const CAPTURE_LOCK: (i32, i32) = (0x494c5248, 1);

#[derive(Debug, PartialEq)]
pub(crate) enum CaptureOutcome {
    Captured {
        date: NaiveDate,
        captured_at: DateTime<Utc>,
        rows: u64,
    },
    SkippedBusy,
    SkippedCooldown,
}

#[derive(Deserialize)]
pub(crate) struct HistoryQuery {
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
    days: Option<u16>,
}

fn query_bounds(query: &HistoryQuery) -> ApiResult<(i16, ScoreVariant, u16)> {
    let mode = query.mode.unwrap_or(0);
    let variant = query.variant.unwrap_or_default();
    scores::validate_partition(mode, variant)?;
    let days = query.days.unwrap_or(90);
    if !(1..=180).contains(&days) {
        return Err(ApiError::bad("Rank history must cover 1–180 days."));
    }
    Ok((mode, variant, days))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult {
    let (mode, variant, days) = query_bounds(&query)?;
    // The current restriction and all stored observations are read in the same
    // statement snapshot. Staff have the same public visibility as everyone.
    let value = sqlx::query_scalar::<_, Value>(
        "WITH utc_day AS (SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date end_date),
         bounds AS (SELECT end_date,end_date-($4::integer-1) start_date FROM utc_day)
         SELECT jsonb_build_object(
             'mode',$2::smallint,'variant',$3::text,'days',$4::integer,
             'start_date',b.start_date,'end_date',b.end_date,'time_zone','UTC',
             'collection_interval_seconds',$5::bigint,
             'last_captured_at',(SELECT max(captured_at) FROM rank_history_batches
                 WHERE date BETWEEN b.start_date AND b.end_date),
             'points',COALESCE((SELECT jsonb_agg(jsonb_build_object(
                 'date',h.date,'captured_at',h.captured_at,'pp',h.pp,'rank',h.rank,
                 'country_rank',h.country_rank,'country',h.country) ORDER BY h.date)
                 FROM rank_history_daily h WHERE h.user_id=u.id AND h.mode=$2 AND h.variant=$3
                 AND h.date BETWEEN b.start_date AND b.end_date),'[]'::jsonb))
         FROM users u CROSS JOIN bounds b WHERE u.id=$1 AND NOT u.is_restricted",
    )
    .bind(id)
    .bind(mode)
    .bind(variant.as_str())
    .bind(i32::from(days))
    .bind(COLLECTION_INTERVAL_SECONDS as i64)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    Ok(Json(value))
}

pub(crate) fn start(db: PgPool) {
    tokio::spawn(async move {
        // Polling faster than collection prevents tick/capture timestamp jitter
        // or a replica restart from stretching a nominal 30-minute cadence.
        let mut timer = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECONDS));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            // Tokio intervals deliver the first tick immediately on startup.
            timer.tick().await;
            match capture_current_day(&db).await {
                Ok(CaptureOutcome::Captured { rows, .. }) => {
                    tracing::info!(
                        code = "rank_history_captured",
                        rows,
                        "rank history observation recorded"
                    );
                }
                Ok(CaptureOutcome::SkippedBusy) => {
                    tracing::debug!(
                        code = "rank_history_capture_busy",
                        "rank history observation deferred"
                    );
                }
                Ok(CaptureOutcome::SkippedCooldown) => {
                    tracing::debug!(
                        code = "rank_history_capture_cooldown",
                        "rank history observation deferred"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        code = error_category(&error),
                        "rank history observation deferred"
                    );
                }
            }
        }
    });
}

fn error_category(error: &sqlx::Error) -> &'static str {
    if error
        .as_database_error()
        .is_some_and(|error| error.code().as_deref() == Some("40001"))
    {
        "rank_history_capture_serialization"
    } else if matches!(error, sqlx::Error::Database(_)) {
        "rank_history_capture_database"
    } else {
        "rank_history_capture_unavailable"
    }
}

pub(crate) async fn capture_current_day(db: &PgPool) -> Result<CaptureOutcome, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await?;
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1,$2)")
        .bind(CAPTURE_LOCK.0)
        .bind(CAPTURE_LOCK.1)
        .fetch_one(&mut *tx)
        .await?;
    if !acquired {
        tx.rollback().await?;
        return Ok(CaptureOutcome::SkippedBusy);
    }
    // Time and UTC day belong to the acquired observation, not process startup.
    let (date, captured_at): (NaiveDate, DateTime<Utc>) = sqlx::query_as(
        "SELECT (observed_at AT TIME ZONE 'UTC')::date,observed_at
         FROM (SELECT clock_timestamp() observed_at) observation",
    )
    .fetch_one(&mut *tx)
    .await?;
    let cooldown: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM rank_history_batches WHERE date=$1
         AND captured_at>$2::timestamptz-($3::bigint * interval '1 second'))",
    )
    .bind(date)
    .bind(captured_at)
    .bind(COLLECTION_INTERVAL_SECONDS as i64)
    .fetch_one(&mut *tx)
    .await?;
    if cooldown {
        tx.rollback().await?;
        return Ok(CaptureOutcome::SkippedCooldown);
    }
    // Updating the batch fence also makes a narrowly stale RR snapshot fail
    // serialization if another replica committed just before the lock returned.
    sqlx::query(
        "INSERT INTO rank_history_batches(date,captured_at,row_count) VALUES($1,$2,0)
         ON CONFLICT(date) DO UPDATE SET captured_at=EXCLUDED.captured_at,row_count=0",
    )
    .bind(date)
    .bind(captured_at)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM rank_history_daily WHERE date=$1")
        .bind(date)
        .execute(&mut *tx)
        .await?;
    let sql = format!(
        "{} INSERT INTO rank_history_daily(user_id,date,mode,variant,captured_at,country,pp,rank,country_rank)
         SELECT id,$3,$1,$2,$4,country,pp,CASE WHEN pp>0 THEN rank END,
         CASE WHEN pp>0 THEN country_rank END FROM ranked",
        scores::RANKING_CTE
    );
    let mut rows = 0;
    for (mode, variant) in [
        (0_i16, ScoreVariant::Vanilla),
        (1, ScoreVariant::Vanilla),
        (2, ScoreVariant::Vanilla),
        (3, ScoreVariant::Vanilla),
        (0, ScoreVariant::Relax),
        (1, ScoreVariant::Relax),
        (2, ScoreVariant::Relax),
    ] {
        rows += sqlx::query(&sql)
            .bind(mode)
            .bind(variant.as_str())
            .bind(date)
            .bind(captured_at)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    sqlx::query("UPDATE rank_history_batches SET row_count=$2 WHERE date=$1")
        .bind(date)
        .bind(rows as i64)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(CaptureOutcome::Captured {
        date,
        captured_at,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_history_query_bounds_match_supported_partitions_and_windows() {
        let default = HistoryQuery {
            mode: None,
            variant: None,
            days: None,
        };
        assert_eq!(
            query_bounds(&default).unwrap(),
            (0, ScoreVariant::Vanilla, 90)
        );
        for variant in [ScoreVariant::Vanilla, ScoreVariant::Relax] {
            for mode in 0..=3 {
                for days in [1, 180] {
                    let query = HistoryQuery {
                        mode: Some(mode),
                        variant: Some(variant),
                        days: Some(days),
                    };
                    assert_eq!(
                        query_bounds(&query).is_ok(),
                        variant == ScoreVariant::Vanilla || mode != 3
                    );
                }
            }
        }
        for (mode, days) in [(-1, 90), (4, 90), (0, 0), (0, 181)] {
            assert!(
                query_bounds(&HistoryQuery {
                    mode: Some(mode),
                    variant: None,
                    days: Some(days)
                })
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<HistoryQuery>(serde_json::json!({"variant":"rx"})).is_err()
        );
    }
}
