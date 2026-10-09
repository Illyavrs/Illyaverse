//! Room results are a separate authorization surface from global score lookup.
//! Every peer result is joined to the immutable round roster before formatting.

use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::{
    PgConnection, Postgres, Row, Transaction, postgres::PgArguments, query::Query as SqlQuery,
};
use std::collections::HashMap;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/rooms/{room}/playlist/{item}/scores",
            get(index_scores),
        )
        .route(
            "/api/v2/rooms/{room}/playlist/{item}/scores/{score}",
            get(show_score),
        )
        .route(
            "/api/v2/rooms/{room}/playlist/{item}/scores/users/{user}",
            get(show_user_score),
        )
        .route("/api/v2/rooms/{room}/leaderboard", get(leaderboard))
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "room_score_not_found")
}

fn overflow() -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "room_score_overflow")
}

fn result_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|error| error.code().as_deref() == Some("22003"))
    {
        overflow()
    } else {
        error.into()
    }
}

// The complete native arrays need a single MVCC snapshot across their bounded
// SQL chunks: concurrent best replacement/rejection must not move a user across
// the internal cursor and make a one-shot leaderboard omit or repeat that user.
async fn results_snapshot(state: &AppState) -> Result<Transaction<'static, Postgres>, ApiError> {
    let mut tx = state.pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

fn native_count(value: i64) -> Result<i32, ApiError> {
    i32::try_from(value).map_err(|_| overflow())
}

fn check_ids(room: i64, item: Option<i64>) -> Result<(), ApiError> {
    if room <= 0 || item.is_some_and(|item| item <= 0) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_room_score_identity",
        ));
    }
    Ok(())
}

/// A live member may request an empty snapshot. Historical rows still require
/// their own shared frozen round, so joining later never grants earlier results.
pub(crate) async fn room_history_authorized(
    state: &AppState,
    viewer: &Account,
    room: i64,
) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lazer_rooms r JOIN users v ON v.id=$2 AND NOT v.is_restricted WHERE r.id=$1 AND ($3 OR r.host_id=$2 OR EXISTS(SELECT 1 FROM lazer_room_members m WHERE m.room_id=r.id AND m.user_id=$2) OR EXISTS(SELECT 1 FROM lazer_room_rounds rr JOIN lazer_room_round_participants p ON p.round_id=rr.id WHERE rr.room_id=r.id AND p.user_id=$2)))",
    )
    .bind(room)
    .bind(viewer.id)
    .bind(viewer.can("scores.moderate"))
    .fetch_one(&state.pool)
    .await?)
}

pub(crate) async fn item_history_authorized(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
) -> Result<bool, ApiError> {
    if !room_history_authorized(state, viewer, room).await? {
        return Ok(false);
    }
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lazer_room_playlist_versions WHERE room_id=$1 AND playlist_item_id=$2)",
    )
    .bind(room)
    .bind(item)
    .fetch_one(&state.pool)
    .await?)
}

/// Used for committed event delivery. The room/item/round triple must be exact;
/// current membership is deliberately insufficient for historical events.
pub(crate) async fn round_history_authorized(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
    round: i64,
) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lazer_room_rounds r JOIN users v ON v.id=$4 AND NOT v.is_restricted WHERE r.id=$3 AND r.room_id=$1 AND r.playlist_item_id=$2 AND ($5 OR EXISTS(SELECT 1 FROM lazer_room_round_participants p WHERE p.round_id=r.id AND p.user_id=$4)))",
    )
    .bind(room)
    .bind(item)
    .bind(round)
    .bind(viewer.id)
    .bind(viewer.can("scores.moderate"))
    .fetch_one(&state.pool)
    .await?)
}

#[derive(Clone, Debug)]
struct Score {
    id: i64,
    total: i64,
    position: i32,
}

// SQL aliases are local to this module. Besides the FK triple, validate the
// token/participant and score identities so a corrupt link cannot be rendered.
// All derived queries share this exact viewer-specific frozen-round predicate.
// Attempts (including quit/revoked/rejected attempts) are counted, while only
// structurally valid, processed, non-rejected scores can become highs/stats.
// PostgreSQL evaluates the complete history; callers retrieve only scalars,
// aggregates or explicitly limited result windows, never all attempt rows.
const RESULTS_CTE: &str = "WITH attempts AS (SELECT t.user_id,t.playlist_item_id,s.id score_id,s.score total_score,s.accuracy/100.0 accuracy,s.passed \
    FROM lazer_score_tokens t \
    JOIN lazer_room_rounds r ON r.id=t.round_id AND r.room_id=t.room_id AND r.playlist_item_id=t.playlist_item_id \
    JOIN lazer_room_round_participants p ON p.round_id=r.id AND p.user_id=t.user_id AND p.beatmap_id=t.beatmap_id AND p.ruleset_id=t.ruleset_id \
    JOIN users u ON u.id=t.user_id AND NOT u.is_restricted \
    JOIN users v ON v.id=$3 AND NOT v.is_restricted \
    LEFT JOIN scores s ON s.id=t.score_id AND s.user_id=t.user_id AND s.beatmap_id=p.beatmap_id AND s.mode=p.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text AND s.verification<>'rejected' AND t.processing_status='processed' AND t.revoked_at IS NULL AND s.score>=0 AND s.accuracy BETWEEN 0 AND 100 \
    WHERE t.scope='room' AND t.room_id=$1 AND ($2::bigint IS NULL OR t.playlist_item_id=$2) \
    AND ($4 OR t.user_id=$3 OR EXISTS(SELECT 1 FROM lazer_room_round_participants vp WHERE vp.round_id=r.id AND vp.user_id=$3)) \
    ), highs AS (SELECT DISTINCT ON(user_id,playlist_item_id) user_id,playlist_item_id,score_id,total_score,accuracy,passed \
    FROM attempts WHERE score_id IS NOT NULL AND ($5 OR passed) \
    ORDER BY user_id,playlist_item_id,total_score DESC,score_id), \
    ranked AS (SELECT *,row_number() OVER(ORDER BY total_score DESC,score_id) position FROM highs)";

fn results_query<'a>(
    sql: &'a str,
    viewer: &Account,
    room: i64,
    item: Option<i64>,
    head_to_head: bool,
) -> SqlQuery<'a, Postgres, PgArguments> {
    sqlx::query(sql)
        .bind(room)
        .bind(item)
        .bind(viewer.id)
        .bind(viewer.can("scores.moderate"))
        .bind(head_to_head)
}

fn score_from_row(row: sqlx::postgres::PgRow) -> Result<Score, ApiError> {
    Ok(Score {
        id: row.try_get("score_id")?,
        total: row.try_get("total_score")?,
        position: native_count(row.try_get("position")?)?,
    })
}

async fn head_to_head(state: &AppState, room: i64) -> Result<bool, ApiError> {
    let settings: Value = sqlx::query_scalar("SELECT settings FROM lazer_rooms WHERE id=$1")
        .bind(room)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(not_found)?;
    // Creation currently accepts only MatchType.HeadToHead (1). Supporting the
    // playlist selection rule here does not add playlist-room creation support.
    Ok(settings["matchType"].as_i64() == Some(1)
        || settings["matchType"].as_str() == Some("head_to_head"))
}

// A response may contain every user/item because the native contracts are
// unpaged arrays. SQL fetches stay bounded even for those complete responses.
const AGGREGATE_BATCH: i64 = 100;
const AGGREGATES_CTE: &str = ", attempt_counts AS (SELECT user_id,count(*)::bigint attempts FROM attempts GROUP BY user_id), \
    totals AS (SELECT h.user_id,c.attempts,count(*)::bigint completed,sum(h.total_score)::bigint total_score,avg(h.accuracy)::double precision accuracy,max(h.score_id) last_high \
    FROM highs h JOIN attempt_counts c ON c.user_id=h.user_id GROUP BY h.user_id,c.attempts), \
    aggregates AS (SELECT *,row_number() OVER(ORDER BY total_score DESC,last_high) position FROM totals)";

#[derive(Debug)]
struct Aggregate {
    user: i64,
    attempts: i32,
    completed: i32,
    total: i64,
    accuracy: f64,
    last_high: i64,
    position: i32,
}

fn aggregate_from_row(row: sqlx::postgres::PgRow) -> Result<Aggregate, ApiError> {
    let accuracy: f64 = row.try_get("accuracy")?;
    if !accuracy.is_finite() || !(0.0..=1.0).contains(&accuracy) {
        return Err(overflow());
    }
    Ok(Aggregate {
        user: row.try_get("user_id")?,
        attempts: native_count(row.try_get("attempts")?)?,
        completed: native_count(row.try_get("completed")?)?,
        total: row.try_get("total_score")?,
        accuracy,
        last_high: row.try_get("last_high")?,
        position: native_count(row.try_get("position")?)?,
    })
}

async fn aggregate_json(
    db: &mut PgConnection,
    room: i64,
    aggregate: &Aggregate,
) -> Result<Value, ApiError> {
    let owner = common::account_on_connection(db, aggregate.user).await?;
    Ok(json!({
        "room_id": room, "user_id": aggregate.user, "user": crate::lazer::user_json(&owner),
        "attempts": aggregate.attempts, "completed": aggregate.completed,
        "total_score": aggregate.total, "accuracy": aggregate.accuracy,
        "pp": null, "position": aggregate.position,
    }))
}

async fn own_aggregate(
    db: &mut PgConnection,
    viewer: &Account,
    room: i64,
    head_to_head: bool,
) -> Result<Option<Aggregate>, ApiError> {
    let sql =
        format!("{RESULTS_CTE}{AGGREGATES_CTE} SELECT * FROM aggregates WHERE user_id=$3 LIMIT 1");
    results_query(&sql, viewer, room, None, head_to_head)
        .fetch_optional(db)
        .await
        .map_err(result_error)?
        .map(aggregate_from_row)
        .transpose()
}

/// The native Room.current_user_score contract is PlaylistAggregateScore, whose
/// playlist_item_attempts entries use int IDs/counts and a passing-score flag.
pub(crate) async fn room_summary(
    state: &AppState,
    viewer: &Account,
    room: i64,
) -> Result<Value, ApiError> {
    if !room_history_authorized(state, viewer, room).await? {
        return Ok(Value::Null);
    }
    let head_to_head = head_to_head(state, room).await?;
    let mut tx = results_snapshot(state).await?;
    let sql =
        format!("{RESULTS_CTE} SELECT count(*)::bigint attempts FROM attempts WHERE user_id=$3");
    let row = results_query(&sql, viewer, room, None, head_to_head)
        .fetch_one(&mut *tx)
        .await?;
    let attempt_count = native_count(row.try_get("attempts")?)?;
    if attempt_count == 0 {
        return Ok(Value::Null);
    }
    let mut items = Vec::new();
    let mut after = 0_i64;
    let sql = format!(
        "{RESULTS_CTE} SELECT playlist_item_id,count(*)::bigint attempts,coalesce(bool_or(passed),false) passed FROM attempts WHERE user_id=$3 AND playlist_item_id>$6 GROUP BY playlist_item_id ORDER BY playlist_item_id LIMIT $7"
    );
    loop {
        let rows = results_query(&sql, viewer, room, None, head_to_head)
            .bind(after)
            .bind(AGGREGATE_BATCH)
            .fetch_all(&mut *tx)
            .await?;
        let complete = rows.len() < AGGREGATE_BATCH as usize;
        for row in rows {
            let item: i64 = row.try_get("playlist_item_id")?;
            after = item;
            items.push(json!({"id":native_count(item)?,"attempts":native_count(row.try_get("attempts")?)?,"passed":row.try_get::<bool,_>("passed")?}));
        }
        if complete {
            break;
        }
    }
    let aggregate = own_aggregate(&mut tx, viewer, room, head_to_head).await?;
    let mut result = if let Some(aggregate) = aggregate {
        aggregate_json(&mut tx, room, &aggregate).await?
    } else {
        json!({"room_id":room,"user_id":viewer.id,"attempts":attempt_count,"completed":0,"total_score":0,"accuracy":0.0,"pp":null,"position":null})
    };
    tx.commit().await?;
    result["playlist_item_attempts"] = json!(items);
    Ok(result)
}

pub(crate) async fn item_attempts(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
) -> Result<Value, ApiError> {
    if !item_history_authorized(state, viewer, room, item).await? {
        return Ok(json!({"attempts":0,"completed":false,"passed":false}));
    }
    let sql = format!(
        "{RESULTS_CTE} SELECT count(*)::bigint attempts,coalesce(bool_or(passed),false) passed FROM attempts WHERE user_id=$3"
    );
    let row = results_query(&sql, viewer, room, Some(item), false)
        .fetch_one(&state.pool)
        .await?;
    let attempts = native_count(row.try_get("attempts")?)?;
    let passed: bool = row.try_get("passed")?;
    Ok(json!({"attempts":attempts,"completed":passed,"passed":passed}))
}

pub(crate) async fn playlist_stats(
    state: &AppState,
    viewer: &Account,
    room: i64,
) -> Result<Value, ApiError> {
    if !room_history_authorized(state, viewer, room).await? {
        return Err(not_found());
    }
    let mut tx = results_snapshot(state).await?;
    // The bins are SQL aggregates of every eligible passing attempt, rather
    // than selected highs. Invalid/rejected/failed attempts cannot enter bins.
    let sql = format!(
        "{RESULTS_CTE}, items AS (SELECT DISTINCT playlist_item_id FROM lazer_room_playlist_versions WHERE room_id=$1 AND playlist_item_id>$6 ORDER BY playlist_item_id LIMIT $7), \
        stats AS (SELECT a.playlist_item_id,coalesce(sum(a.total_score) FILTER(WHERE a.passed),0)::bigint cumulative_score,max(a.score_id) last_processed, \
        array[{}] bins FROM attempts a JOIN items i ON i.playlist_item_id=a.playlist_item_id WHERE a.score_id IS NOT NULL GROUP BY a.playlist_item_id) \
        SELECT i.playlist_item_id,coalesce(s.cumulative_score,0) cumulative_score,coalesce(s.last_processed,0) last_processed,coalesce(s.bins,array_fill(0::bigint,ARRAY[13])) bins \
        FROM items i LEFT JOIN stats s ON s.playlist_item_id=i.playlist_item_id ORDER BY i.playlist_item_id",
        (0..13)
            .map(|bin| format!(
                "count(*) FILTER(WHERE a.passed AND least(a.total_score/100000,12)={bin})"
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut stats = Vec::new();
    let mut after = 0_i64;
    loop {
        let rows = results_query(&sql, viewer, room, None, false)
            .bind(after)
            .bind(AGGREGATE_BATCH)
            .fetch_all(&mut *tx)
            .await
            .map_err(result_error)?;
        let complete = rows.len() < AGGREGATE_BATCH as usize;
        for row in rows {
            let item: i64 = row.try_get("playlist_item_id")?;
            after = item;
            stats.push(json!({"playlistItemID":item,"totalScoreDistribution":row.try_get::<Vec<i64>,_>("bins")?,"cumulativeScore":row.try_get::<i64,_>("cumulative_score")?,"lastProcessedScoreID":row.try_get::<i64,_>("last_processed")?}));
        }
        if complete {
            break;
        }
    }
    tx.commit().await?;
    Ok(json!(stats))
}
const SCORE_SQL: &str = "SELECT s.*,b.native_id native_map_id,b.status map_status,t.id room_token_id \
    FROM lazer_score_tokens t \
    JOIN lazer_room_rounds r ON r.id=t.round_id AND r.room_id=t.room_id AND r.playlist_item_id=t.playlist_item_id \
    JOIN lazer_room_round_participants p ON p.round_id=r.id AND p.user_id=t.user_id AND p.beatmap_id=t.beatmap_id AND p.ruleset_id=t.ruleset_id \
    JOIN scores s ON s.id=t.score_id AND s.user_id=t.user_id AND s.beatmap_id=p.beatmap_id AND s.mode=p.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text \
    JOIN beatmaps b ON b.id=s.beatmap_id \
    JOIN users u ON u.id=s.user_id AND NOT u.is_restricted \
    JOIN users v ON v.id=$4 AND NOT v.is_restricted \
    WHERE t.scope='room' AND t.processing_status='processed' AND t.revoked_at IS NULL AND t.room_id=$1 AND t.playlist_item_id=$2 \
    AND (($6 AND t.id=$3 AND t.user_id=$4) OR (NOT $6 AND s.id=$3)) \
    AND ($5 OR s.user_id=$4 OR EXISTS(SELECT 1 FROM lazer_room_round_participants vp WHERE vp.round_id=r.id AND vp.user_id=$4)) \
    AND (s.verification<>'rejected' OR s.user_id=$4 OR $5)";

async fn render_score(
    db: &mut PgConnection,
    viewer: &Account,
    room: i64,
    item: i64,
    id: i64,
    by_token: bool,
    position: Option<i32>,
) -> Result<Value, ApiError> {
    let row = sqlx::query(SCORE_SQL)
        .bind(room)
        .bind(item)
        .bind(id)
        .bind(viewer.id)
        .bind(viewer.can("scores.moderate"))
        .bind(by_token)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(not_found)?;
    // Preserve the native gameplay total even when common-score caches or PP
    // context are unavailable (including per-participant Relax choices).
    let total: i64 = row.try_get("score")?;
    let native: Option<Value> = row.try_get("native_submission")?;
    let mut value = crate::lazer::score_json_from_row_on_connection(db, row).await?;
    value["total_score"] = json!(total);
    value["position"] = json!(position);
    value["processed"] = json!(true);
    value["ranked"] = json!(false);
    if let Some(native) = native
        && let Some(ended_at) = native.get("ended_at")
    {
        value["ended_at"] = ended_at.clone();
    }
    Ok(value)
}

pub(crate) async fn score_for_token(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
    token: i64,
) -> Result<Value, ApiError> {
    let value = {
        let mut db = state.pool.acquire().await?;
        render_score(&mut db, viewer, room, item, token, true, None).await?
    };
    let id = value["id"].as_i64().ok_or_else(not_found)?;
    let position = event_rank(state, viewer, room, item, id).await?;
    let mut value = value;
    value["position"] = json!(position);
    Ok(value)
}

/// Event ranks use exactly the recipient's visible frozen-round history. The
/// durable global rank is useful for accounting but may contain hidden rounds.
pub(crate) async fn event_rank(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
    score: i64,
) -> Result<Option<i32>, ApiError> {
    let head_to_head = head_to_head(state, room).await?;
    let mut tx = results_snapshot(state).await?;
    let high = high_score(&mut tx, viewer, room, item, Some(score), None, head_to_head).await?;
    tx.commit().await?;
    Ok(high.map(|high| high.position))
}

async fn high_score(
    db: &mut PgConnection,
    viewer: &Account,
    room: i64,
    item: i64,
    score: Option<i64>,
    user: Option<i64>,
    head_to_head: bool,
) -> Result<Option<Score>, ApiError> {
    let sql = format!(
        "{RESULTS_CTE} SELECT * FROM ranked WHERE ($6::bigint IS NULL OR score_id=$6) AND ($7::bigint IS NULL OR user_id=$7) LIMIT 1"
    );
    results_query(&sql, viewer, room, Some(item), head_to_head)
        .bind(score)
        .bind(user)
        .fetch_optional(db)
        .await?
        .map(score_from_row)
        .transpose()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sort {
    ScoreDesc,
    ScoreAsc,
    IdDesc,
    IdAsc,
}

impl Sort {
    fn name(self) -> &'static str {
        match self {
            Self::ScoreDesc => "score_desc",
            Self::ScoreAsc => "score_asc",
            Self::IdDesc => "id_desc",
            Self::IdAsc => "id_asc",
        }
    }

    fn sql_order(self) -> &'static str {
        match self {
            Self::ScoreDesc => "total_score DESC,score_id ASC",
            Self::ScoreAsc => "total_score ASC,score_id DESC",
            Self::IdDesc => "score_id DESC",
            Self::IdAsc => "score_id ASC",
        }
    }

    fn sql_after(self) -> &'static str {
        match self {
            Self::ScoreDesc => "(total_score<$6 OR (total_score=$6 AND score_id>$7))",
            Self::ScoreAsc => "(total_score>$6 OR (total_score=$6 AND score_id<$7))",
            Self::IdDesc => "score_id<$7",
            Self::IdAsc => "score_id>$7",
        }
    }

    fn reverse(self) -> Self {
        match self {
            Self::ScoreDesc => Self::ScoreAsc,
            Self::ScoreAsc => Self::ScoreDesc,
            Self::IdDesc => Self::IdAsc,
            Self::IdAsc => Self::IdDesc,
        }
    }
}

struct IndexParams {
    sort: Sort,
    limit: usize,
    cursor: Option<(i64, i64)>,
    around: bool,
}

impl IndexParams {
    fn parse(query: &HashMap<String, String>) -> Result<Self, ApiError> {
        let invalid = || ApiError(StatusCode::BAD_REQUEST, "invalid_room_score_pagination");
        let sort = match query
            .get("sort")
            .map(String::as_str)
            .unwrap_or("score_desc")
        {
            "score_desc" => Sort::ScoreDesc,
            "score_asc" => Sort::ScoreAsc,
            "id_desc" => Sort::IdDesc,
            "id_asc" => Sort::IdAsc,
            _ => return Err(invalid()),
        };
        let limit = query
            .get("limit")
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| invalid())?
            .unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(invalid());
        }
        let total = query
            .get("cursor[total_score]")
            .or_else(|| query.get("cursor[score]"));
        let id = query
            .get("cursor[id]")
            .or_else(|| query.get("cursor[score_id]"));
        let cursor = match (total, id) {
            (None, None) => None,
            (total, Some(id)) => {
                let id = id.parse::<i64>().map_err(|_| invalid())?;
                let total = total
                    .map(|total| total.parse::<i64>())
                    .transpose()
                    .map_err(|_| invalid())?;
                if id <= 0
                    || total.is_some_and(|total| total < 0)
                    || (matches!(sort, Sort::ScoreDesc | Sort::ScoreAsc) && total.is_none())
                {
                    return Err(invalid());
                }
                Some((total.unwrap_or(0), id))
            }
            _ => return Err(invalid()),
        };
        let around = match query
            .get("scores_around")
            .map(String::as_str)
            .unwrap_or("0")
        {
            "0" | "false" => false,
            "1" | "true" => true,
            _ => return Err(invalid()),
        };
        Ok(Self {
            sort,
            limit,
            cursor,
            around,
        })
    }

    fn json(&self) -> Value {
        json!({"sort":self.sort.name(),"limit":self.limit})
    }
}

fn cursor(score: &Score, sort: Sort) -> Value {
    match sort {
        Sort::ScoreDesc | Sort::ScoreAsc => json!({"total_score":score.total,"id":score.id}),
        Sort::IdDesc | Sort::IdAsc => json!({"id":score.id}),
    }
}

async fn render_list(
    db: &mut PgConnection,
    viewer: &Account,
    room: i64,
    item: i64,
    scores: &[Score],
) -> Result<Vec<Value>, ApiError> {
    let mut output = Vec::with_capacity(scores.len());
    for score in scores {
        output.push(
            render_score(
                db,
                viewer,
                room,
                item,
                score.id,
                false,
                Some(score.position),
            )
            .await?,
        );
    }
    Ok(output)
}

struct ScorePage {
    scores: Vec<Score>,
    has_more: bool,
}

/// Window ranks/counts consider every eligible high before keyset filtering.
/// The result transfer is bounded by the validated limit plus one sentinel.
async fn high_page(
    db: &mut PgConnection,
    viewer: &Account,
    room: i64,
    item: i64,
    params: &IndexParams,
    head_to_head: bool,
) -> Result<ScorePage, ApiError> {
    let sql = format!(
        "{RESULTS_CTE} SELECT * FROM ranked WHERE ($6::bigint IS NULL OR {}) ORDER BY {} LIMIT $8",
        params.sort.sql_after(),
        params.sort.sql_order()
    );
    let rows = results_query(&sql, viewer, room, Some(item), head_to_head)
        .bind(params.cursor.map(|(total, _)| total))
        .bind(params.cursor.map(|(_, id)| id))
        .bind(i64::try_from(params.limit + 1).map_err(|_| overflow())?)
        .fetch_all(db)
        .await?;
    let has_more = rows.len() > params.limit;
    let scores = rows
        .into_iter()
        .take(params.limit)
        .map(score_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ScorePage { scores, has_more })
}

async fn scores_around(
    db: &mut PgConnection,
    viewer: &Account,
    binding: (i64, i64),
    score: i64,
    params: &IndexParams,
    head_to_head: bool,
) -> Result<Value, ApiError> {
    let (room, item) = binding;
    let Some(pivot) = high_score(db, viewer, room, item, Some(score), None, head_to_head).await?
    else {
        return Ok(Value::Null);
    };
    // The native result screen steps away from the pivot: preceding rows are
    // returned in reversed sort order, beginning with the immediate neighbor.
    let higher_params = IndexParams {
        sort: params.sort.reverse(),
        limit: params.limit,
        cursor: Some((pivot.total, pivot.id)),
        around: false,
    };
    let lower_params = IndexParams {
        sort: params.sort,
        limit: params.limit,
        cursor: Some((pivot.total, pivot.id)),
        around: false,
    };
    let higher = high_page(db, viewer, room, item, &higher_params, head_to_head).await?;
    let lower = high_page(db, viewer, room, item, &lower_params, head_to_head).await?;
    let higher_scores = render_list(db, viewer, room, item, &higher.scores).await?;
    let lower_scores = render_list(db, viewer, room, item, &lower.scores).await?;
    Ok(json!({
        "higher":{"scores":higher_scores,"params":higher_params.json(),"cursor":if higher.has_more {higher.scores.last().map(|score|cursor(score,higher_params.sort))}else{None}},
        "lower":{"scores":lower_scores,"params":params.json(),"cursor":if lower.has_more {lower.scores.last().map(|score|cursor(score,params.sort))}else{None}},
    }))
}

async fn index_scores(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((room, item)): Path<(i64, i64)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    check_ids(room, Some(item))?;
    let viewer = common::bearer(&state, &headers).await?;
    let params = IndexParams::parse(&query)?;
    if !item_history_authorized(&state, &viewer, room, item).await? {
        return Err(not_found());
    }
    let head_to_head = head_to_head(&state, room).await?;
    let mut tx = results_snapshot(&state).await?;
    let page = high_page(&mut tx, &viewer, room, item, &params, head_to_head).await?;
    let next = if page.has_more {
        page.scores.last().map(|score| cursor(score, params.sort))
    } else {
        None
    };
    let sql = format!("{RESULTS_CTE} SELECT count(*)::bigint total FROM highs");
    let total: i64 = results_query(&sql, &viewer, room, Some(item), head_to_head)
        .fetch_one(&mut *tx)
        .await?
        .try_get("total")?;
    let scores = render_list(&mut tx, &viewer, room, item, &page.scores).await?;
    let mut user_score = if let Some(score) = high_score(
        &mut tx,
        &viewer,
        room,
        item,
        None,
        Some(viewer.id),
        head_to_head,
    )
    .await?
    {
        render_score(
            &mut tx,
            &viewer,
            room,
            item,
            score.id,
            false,
            Some(score.position),
        )
        .await?
    } else {
        Value::Null
    };
    if params.around && !user_score.is_null() {
        user_score["scores_around"] = scores_around(
            &mut tx,
            &viewer,
            (room, item),
            user_score["id"].as_i64().ok_or_else(not_found)?,
            &params,
            head_to_head,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        json!({"scores":scores,"params":params.json(),"cursor":next,"total":total,"user_score":user_score}),
    ))
}

async fn show_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((room, item, score)): Path<(i64, i64, i64)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    check_ids(room, Some(item))?;
    let viewer = common::bearer(&state, &headers).await?;
    let mut params = IndexParams::parse(&query)?;
    // Native Show requests implicitly request adjacent results; their request
    // class has no flag. API callers can explicitly suppress this work.
    if !query.contains_key("scores_around") {
        params.around = true;
    }
    let head_to_head = head_to_head(&state, room).await?;
    let mut tx = results_snapshot(&state).await?;
    let position = high_score(
        &mut tx,
        &viewer,
        room,
        item,
        Some(score),
        None,
        head_to_head,
    )
    .await?
    .map(|score| score.position);
    let mut value = render_score(&mut tx, &viewer, room, item, score, false, position).await?;
    if params.around {
        value["scores_around"] =
            scores_around(&mut tx, &viewer, (room, item), score, &params, head_to_head).await?;
    }
    tx.commit().await?;
    Ok(Json(value))
}

async fn show_user_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((room, item, user)): Path<(i64, i64, i64)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    check_ids(room, Some(item))?;
    let viewer = common::bearer(&state, &headers).await?;
    let mut params = IndexParams::parse(&query)?;
    if !query.contains_key("scores_around") {
        params.around = true;
    }
    if !item_history_authorized(&state, &viewer, room, item).await? {
        return Err(not_found());
    }
    let head_to_head = head_to_head(&state, room).await?;
    let mut tx = results_snapshot(&state).await?;
    let score = high_score(&mut tx, &viewer, room, item, None, Some(user), head_to_head)
        .await?
        .ok_or_else(not_found)?;
    let mut value = render_score(
        &mut tx,
        &viewer,
        room,
        item,
        score.id,
        false,
        Some(score.position),
    )
    .await?;
    if params.around {
        value["scores_around"] = scores_around(
            &mut tx,
            &viewer,
            (room, item),
            score.id,
            &params,
            head_to_head,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(value))
}

async fn leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    check_ids(room, None)?;
    let viewer = common::bearer(&state, &headers).await?;
    Ok(Json(room_leaderboard(&state, &viewer, room).await?))
}

pub(crate) async fn room_leaderboard(
    state: &AppState,
    viewer: &Account,
    room: i64,
) -> Result<Value, ApiError> {
    if !room_history_authorized(state, viewer, room).await? {
        return Err(not_found());
    }
    let head_to_head = head_to_head(state, room).await?;
    let mut tx = results_snapshot(state).await?;
    let sql = format!(
        "{RESULTS_CTE}{AGGREGATES_CTE} SELECT * FROM aggregates WHERE ($6::bigint IS NULL OR total_score<$6 OR (total_score=$6 AND last_high>$7)) ORDER BY total_score DESC,last_high LIMIT $8"
    );
    let mut leaderboard = Vec::new();
    let mut after = None;
    loop {
        let rows = results_query(&sql, viewer, room, None, head_to_head)
            .bind(after.map(|(total, _)| total))
            .bind(after.map(|(_, id)| id))
            .bind(AGGREGATE_BATCH)
            .fetch_all(&mut *tx)
            .await
            .map_err(result_error)?;
        let complete = rows.len() < AGGREGATE_BATCH as usize;
        for row in rows {
            let aggregate = aggregate_from_row(row)?;
            after = Some((aggregate.total, aggregate.last_high));
            leaderboard.push(aggregate_json(&mut tx, room, &aggregate).await?);
        }
        if complete {
            break;
        }
    }
    let aggregate = own_aggregate(&mut tx, viewer, room, head_to_head).await?;
    let user_score = if let Some(aggregate) = aggregate {
        aggregate_json(&mut tx, room, &aggregate).await?
    } else {
        Value::Null
    };
    tx.commit().await?;
    Ok(json!({"leaderboard":leaderboard,"user_score":user_score}))
}
/// Called after durable linking in the same transaction. Return a rank only for
/// the selected non-rejected item high; failed scores can qualify in HTH rooms.
pub(crate) async fn new_best_rank(
    tx: &mut Transaction<'_, Postgres>,
    token: i64,
) -> Result<Option<i32>, ApiError> {
    let rank = sqlx::query_scalar::<_, i64>(
        "WITH target AS (SELECT t.room_id,t.playlist_item_id,t.score_id,r.settings FROM lazer_score_tokens t JOIN lazer_rooms r ON r.id=t.room_id WHERE t.id=$1 AND t.scope='room' AND t.processing_status='processed' AND t.revoked_at IS NULL), \
        eligible AS (SELECT s.id,s.user_id,s.score FROM target x JOIN lazer_score_tokens t ON t.room_id=x.room_id AND t.playlist_item_id=x.playlist_item_id AND t.scope='room' AND t.processing_status='processed' AND t.revoked_at IS NULL JOIN lazer_room_rounds rr ON rr.id=t.round_id AND rr.room_id=t.room_id AND rr.playlist_item_id=t.playlist_item_id JOIN lazer_room_round_participants p ON p.round_id=rr.id AND p.user_id=t.user_id AND p.beatmap_id=t.beatmap_id AND p.ruleset_id=t.ruleset_id JOIN scores s ON s.id=t.score_id AND s.user_id=t.user_id AND s.beatmap_id=p.beatmap_id AND s.mode=p.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text JOIN users u ON u.id=s.user_id AND NOT u.is_restricted WHERE s.verification<>'rejected' AND (x.settings->>'matchType' IN ('1','head_to_head') OR s.passed)), \
        highs AS (SELECT DISTINCT ON(user_id) id,user_id,score FROM eligible ORDER BY user_id,score DESC,id), \
        ranked AS (SELECT id,row_number() OVER(ORDER BY score DESC,id) position FROM highs) \
        SELECT position FROM ranked WHERE id=(SELECT score_id FROM target)",
    ).bind(token).fetch_optional(&mut **tx).await?;
    rank.map(|rank| i32::try_from(rank).map_err(|_| overflow()))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cursor_requires_complete_nonnegative_total_and_positive_id() {
        let query = HashMap::from([
            ("cursor[total_score]".into(), "900000".into()),
            ("cursor[id]".into(), "10".into()),
            ("limit".into(), "2".into()),
        ]);
        let params = IndexParams::parse(&query).unwrap();
        assert_eq!(params.cursor, Some((900000, 10)));
        assert_eq!(params.json(), json!({"sort":"score_desc","limit":2}));
        for invalid in [
            HashMap::from([("cursor[id]".into(), "10".into())]),
            HashMap::from([("cursor[total_score]".into(), "10".into())]),
            HashMap::from([
                ("cursor[id]".into(), "0".into()),
                ("cursor[total_score]".into(), "1".into()),
            ]),
            HashMap::from([
                ("cursor[id]".into(), "1".into()),
                ("cursor[total_score]".into(), "-1".into()),
            ]),
            HashMap::from([("sort".into(), "score_desc;select 1".into())]),
            HashMap::from([("limit".into(), "101".into())]),
            HashMap::from([("limit".into(), "0".into())]),
        ] {
            assert!(IndexParams::parse(&invalid).is_err());
        }
        let ids = IndexParams::parse(&HashMap::from([
            ("sort".into(), "id_desc".into()),
            ("cursor[id]".into(), "10".into()),
        ]))
        .unwrap();
        assert_eq!(ids.cursor, Some((0, 10)));
    }

    #[test]
    fn higher_pages_reverse_both_total_and_tie_direction() {
        assert_eq!(Sort::ScoreDesc.reverse(), Sort::ScoreAsc);
        assert_eq!(Sort::ScoreAsc.reverse(), Sort::ScoreDesc);
        assert_eq!(Sort::IdDesc.reverse(), Sort::IdAsc);
        assert_eq!(Sort::IdAsc.reverse(), Sort::IdDesc);
        let score = Score {
            id: 10,
            total: 900000,
            position: 1,
        };
        assert_eq!(
            cursor(&score, Sort::ScoreDesc),
            json!({"total_score":900000,"id":10})
        );
        assert_eq!(cursor(&score, Sort::IdAsc), json!({"id":10}));
    }
}
