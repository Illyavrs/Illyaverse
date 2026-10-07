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
use sqlx::{Postgres, Row, Transaction};
use std::collections::{BTreeMap, HashMap};

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

fn native_count(value: usize) -> Result<i32, ApiError> {
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
    user: i64,
    item: i64,
    total: i64,
    accuracy: f64,
    passed: bool,
}

#[derive(Debug)]
struct Attempt {
    user: i64,
    item: i64,
    score: Option<Score>,
}

// SQL aliases are local to this module. Besides the FK triple, validate the
// token/participant and score identities so a corrupt link cannot be rendered.
const ATTEMPTS_SQL: &str = "SELECT t.user_id,t.playlist_item_id,s.id score_id,s.score total_score,s.accuracy,s.passed \
    FROM lazer_score_tokens t \
    JOIN lazer_room_rounds r ON r.id=t.round_id AND r.room_id=t.room_id AND r.playlist_item_id=t.playlist_item_id \
    JOIN lazer_room_round_participants p ON p.round_id=r.id AND p.user_id=t.user_id AND p.beatmap_id=t.beatmap_id AND p.ruleset_id=t.ruleset_id \
    JOIN users u ON u.id=t.user_id AND NOT u.is_restricted \
    JOIN users v ON v.id=$3 AND NOT v.is_restricted \
    LEFT JOIN scores s ON s.id=t.score_id AND s.user_id=t.user_id AND s.beatmap_id=p.beatmap_id AND s.mode=p.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text AND s.verification<>'rejected' AND t.processing_status='processed' AND t.revoked_at IS NULL \
    WHERE t.scope='room' AND t.room_id=$1 AND ($2::bigint IS NULL OR t.playlist_item_id=$2) \
    AND ($4 OR t.user_id=$3 OR EXISTS(SELECT 1 FROM lazer_room_round_participants vp WHERE vp.round_id=r.id AND vp.user_id=$3)) \
    ORDER BY t.id";

async fn load_attempts(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: Option<i64>,
) -> Result<Vec<Attempt>, ApiError> {
    let rows = sqlx::query(ATTEMPTS_SQL)
        .bind(room)
        .bind(item)
        .bind(viewer.id)
        .bind(viewer.can("scores.moderate"))
        .fetch_all(&state.pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            let user = row.try_get("user_id")?;
            let item = row.try_get("playlist_item_id")?;
            let score = row
                .try_get::<Option<i64>, _>("score_id")?
                .map(|id| -> Result<Score, ApiError> {
                    let total = row.try_get("total_score")?;
                    let accuracy: f64 = row.try_get("accuracy")?;
                    if total < 0 || !accuracy.is_finite() || !(0.0..=100.0).contains(&accuracy) {
                        return Err(ApiError(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "invalid_room_score",
                        ));
                    }
                    Ok(Score {
                        id,
                        user,
                        item,
                        total,
                        accuracy: accuracy / 100.0,
                        passed: row.try_get("passed")?,
                    })
                })
                .transpose()?;
            Ok(Attempt { user, item, score })
        })
        .collect()
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

fn selected_highs(attempts: &[Attempt], head_to_head: bool) -> Vec<Score> {
    let mut selected: HashMap<(i64, i64), &Score> = HashMap::new();
    for score in attempts.iter().filter_map(|attempt| attempt.score.as_ref()) {
        if !head_to_head && !score.passed {
            continue;
        }
        let key = (score.user, score.item);
        if selected.get(&key).is_none_or(|old| {
            score.total > old.total || (score.total == old.total && score.id < old.id)
        }) {
            selected.insert(key, score);
        }
    }
    let mut scores: Vec<_> = selected.into_values().cloned().collect();
    scores.sort_by(|a, b| b.total.cmp(&a.total).then(a.id.cmp(&b.id)));
    scores
}

#[derive(Debug)]
struct Aggregate {
    user: i64,
    attempts: i32,
    completed: i32,
    total: i64,
    accuracy: f64,
    last_high: i64,
}

fn aggregate_scores(attempts: &[Attempt], highs: &[Score]) -> Result<Vec<Aggregate>, ApiError> {
    let mut attempts_by_user: HashMap<i64, i32> = HashMap::new();
    for attempt in attempts {
        let count = attempts_by_user.entry(attempt.user).or_default();
        *count = count.checked_add(1).ok_or_else(overflow)?;
    }
    let mut aggregates: HashMap<i64, Aggregate> = HashMap::new();
    for high in highs {
        let aggregate = aggregates.entry(high.user).or_insert(Aggregate {
            user: high.user,
            attempts: attempts_by_user.get(&high.user).copied().unwrap_or(0),
            completed: 0,
            total: 0,
            accuracy: 0.0,
            last_high: 0,
        });
        aggregate.total = aggregate
            .total
            .checked_add(high.total)
            .ok_or_else(overflow)?;
        aggregate.completed = aggregate.completed.checked_add(1).ok_or_else(overflow)?;
        aggregate.accuracy += high.accuracy;
        aggregate.last_high = aggregate.last_high.max(high.id);
    }
    let mut aggregates: Vec<_> = aggregates.into_values().collect();
    for aggregate in &mut aggregates {
        aggregate.accuracy /= f64::from(aggregate.completed);
        if !aggregate.accuracy.is_finite() {
            return Err(overflow());
        }
    }
    aggregates.sort_by(|a, b| b.total.cmp(&a.total).then(a.last_high.cmp(&b.last_high)));
    Ok(aggregates)
}

async fn aggregate_json(
    state: &AppState,
    room: i64,
    aggregate: &Aggregate,
    position: i32,
) -> Result<Value, ApiError> {
    let owner = common::account(state, aggregate.user).await?;
    Ok(json!({
        "room_id": room, "user_id": aggregate.user, "user": crate::lazer::user_json(&owner),
        "attempts": aggregate.attempts, "completed": aggregate.completed,
        "total_score": aggregate.total, "accuracy": aggregate.accuracy,
        "pp": null, "position": position,
    }))
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
    let attempts = load_attempts(state, viewer, room, None).await?;
    let own: Vec<_> = attempts
        .iter()
        .filter(|attempt| attempt.user == viewer.id)
        .collect();
    if own.is_empty() {
        return Ok(Value::Null);
    }
    let mut by_item: BTreeMap<i64, (i32, bool)> = BTreeMap::new();
    for attempt in own {
        let value = by_item.entry(attempt.item).or_default();
        value.0 = value.0.checked_add(1).ok_or_else(overflow)?;
        value.1 |= attempt.score.as_ref().is_some_and(|score| score.passed);
    }
    let items: Result<Vec<_>, ApiError> = by_item
        .into_iter()
        .map(|(id, (attempts, passed))| {
            let id = i32::try_from(id).map_err(|_| overflow())?;
            Ok(json!({"id": id, "attempts": attempts, "passed": passed}))
        })
        .collect();
    let highs = selected_highs(&attempts, head_to_head(state, room).await?);
    let aggregates = aggregate_scores(&attempts, &highs)?;
    let own_aggregate = aggregates
        .iter()
        .enumerate()
        .find(|(_, aggregate)| aggregate.user == viewer.id);
    let mut result = if let Some((index, aggregate)) = own_aggregate {
        aggregate_json(state, room, aggregate, native_count(index + 1)?).await?
    } else {
        json!({"room_id": room, "user_id": viewer.id, "attempts": by_item_count(&attempts, viewer.id)?, "completed":0, "total_score":0, "accuracy":0.0, "pp":null, "position":null})
    };
    result["playlist_item_attempts"] = json!(items?);
    Ok(result)
}

fn by_item_count(attempts: &[Attempt], user: i64) -> Result<i32, ApiError> {
    native_count(
        attempts
            .iter()
            .filter(|attempt| attempt.user == user)
            .count(),
    )
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
    let attempts = load_attempts(state, viewer, room, Some(item)).await?;
    let passed = attempts.iter().any(|attempt| {
        attempt.user == viewer.id && attempt.score.as_ref().is_some_and(|score| score.passed)
    });
    Ok(json!({"attempts":by_item_count(&attempts, viewer.id)?,"completed":passed,"passed":passed}))
}

#[derive(Default)]
struct ItemStats {
    bins: [i64; 13],
    cumulative: i64,
    last_processed: i64,
}

impl ItemStats {
    fn add(&mut self, score: &Score) -> Result<(), ApiError> {
        self.last_processed = self.last_processed.max(score.id);
        if score.passed {
            let bin = usize::try_from((score.total / 100_000).min(12)).map_err(|_| overflow())?;
            self.bins[bin] = self.bins[bin].checked_add(1).ok_or_else(overflow)?;
            self.cumulative = self
                .cumulative
                .checked_add(score.total)
                .ok_or_else(overflow)?;
        }
        Ok(())
    }
}

pub(crate) async fn playlist_stats(
    state: &AppState,
    viewer: &Account,
    room: i64,
) -> Result<Value, ApiError> {
    if !room_history_authorized(state, viewer, room).await? {
        return Err(not_found());
    }
    let items = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT playlist_item_id FROM lazer_room_playlist_versions WHERE room_id=$1 ORDER BY playlist_item_id",
    ).bind(room).fetch_all(&state.pool).await?;
    let mut stats: BTreeMap<i64, ItemStats> = items
        .into_iter()
        .map(|item| (item, ItemStats::default()))
        .collect();
    for attempt in load_attempts(state, viewer, room, None).await? {
        if let Some(score) = attempt.score {
            stats.entry(score.item).or_default().add(&score)?;
        }
    }
    Ok(json!(stats.into_iter().map(|(item, stats)| {
        json!({"playlistItemID":item,"totalScoreDistribution":stats.bins,"cumulativeScore":stats.cumulative,"lastProcessedScoreID":stats.last_processed})
    }).collect::<Vec<_>>()))
}

const SCORE_SQL: &str = "SELECT s.*,COALESCE(b.official_id,b.id) native_map_id,b.status map_status,t.id room_token_id \
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
    state: &AppState,
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
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(not_found)?;
    // Preserve the native gameplay total even when common-score caches or PP
    // context are unavailable (including per-participant Relax choices).
    let total: i64 = row.try_get("score")?;
    let native: Option<Value> = row.try_get("native_submission")?;
    let mut value = crate::lazer::score_json_from_row(state, row).await?;
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
    let value = render_score(state, viewer, room, item, token, true, None).await?;
    let id = value["id"].as_i64().ok_or_else(not_found)?;
    let attempts = load_attempts(state, viewer, room, Some(item)).await?;
    let highs = selected_highs(&attempts, head_to_head(state, room).await?);
    let position = highs
        .iter()
        .position(|score| score.id == id)
        .map(|index| native_count(index + 1))
        .transpose()?;
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
    let attempts = load_attempts(state, viewer, room, Some(item)).await?;
    let highs = selected_highs(&attempts, head_to_head(state, room).await?);
    highs
        .iter()
        .position(|high| high.id == score)
        .map(|index| native_count(index + 1))
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

    fn order(self, scores: &mut [Score]) {
        scores.sort_by(|a, b| match self {
            Self::ScoreDesc => b.total.cmp(&a.total).then(a.id.cmp(&b.id)),
            Self::ScoreAsc => a.total.cmp(&b.total).then(b.id.cmp(&a.id)),
            Self::IdDesc => b.id.cmp(&a.id),
            Self::IdAsc => a.id.cmp(&b.id),
        });
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

    fn after(&self, score: &Score) -> bool {
        let Some((total, id)) = self.cursor else {
            return true;
        };
        match self.sort {
            Sort::ScoreDesc => score.total < total || (score.total == total && score.id > id),
            Sort::ScoreAsc => score.total > total || (score.total == total && score.id < id),
            Sort::IdDesc => score.id < id,
            Sort::IdAsc => score.id > id,
        }
    }
}

fn cursor(score: &Score, sort: Sort) -> Value {
    match sort {
        Sort::ScoreDesc | Sort::ScoreAsc => json!({"total_score":score.total,"id":score.id}),
        Sort::IdDesc | Sort::IdAsc => json!({"id":score.id}),
    }
}

async fn render_list(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
    scores: &[Score],
    positions: &HashMap<i64, i32>,
) -> Result<Vec<Value>, ApiError> {
    let mut output = Vec::with_capacity(scores.len());
    for score in scores {
        output.push(
            render_score(
                state,
                viewer,
                room,
                item,
                score.id,
                false,
                positions.get(&score.id).copied(),
            )
            .await?,
        );
    }
    Ok(output)
}

fn positions(highs: &[Score]) -> Result<HashMap<i64, i32>, ApiError> {
    highs
        .iter()
        .enumerate()
        .map(|(index, score)| Ok((score.id, native_count(index + 1)?)))
        .collect()
}

async fn scores_around(
    state: &AppState,
    viewer: &Account,
    room: i64,
    item: i64,
    score: i64,
    highs: &[Score],
    params: &IndexParams,
) -> Result<Value, ApiError> {
    let positions = positions(highs)?;
    let mut ordered = highs.to_vec();
    params.sort.order(&mut ordered);
    let Some(index) = ordered.iter().position(|candidate| candidate.id == score) else {
        return Ok(Value::Null);
    };
    let higher_start = index.saturating_sub(params.limit);
    // The native result screen assigns positions by stepping away from the
    // pivot. Higher scores therefore start with the immediately preceding row.
    let higher: Vec<_> = ordered[higher_start..index].iter().rev().cloned().collect();
    let higher_params = IndexParams {
        sort: params.sort.reverse(),
        limit: params.limit,
        cursor: None,
        around: false,
    };
    let lower_end = index
        .saturating_add(1)
        .saturating_add(params.limit)
        .min(ordered.len());
    let lower = &ordered[index + 1..lower_end];
    let higher_scores = render_list(state, viewer, room, item, &higher, &positions).await?;
    let lower_scores = render_list(state, viewer, room, item, lower, &positions).await?;
    Ok(json!({
        "higher":{"scores":higher_scores,"params":higher_params.json(),"cursor":if higher_start>0{higher.last().map(|score|cursor(score,higher_params.sort))}else{None}},
        "lower":{"scores":lower_scores,"params":params.json(),"cursor":if lower_end<ordered.len(){lower.last().map(|score|cursor(score,params.sort))}else{None}},
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
    let attempts = load_attempts(&state, &viewer, room, Some(item)).await?;
    let highs = selected_highs(&attempts, head_to_head(&state, room).await?);
    let positions = positions(&highs)?;
    let total = i64::try_from(highs.len()).map_err(|_| overflow())?;
    let mut ordered = highs.clone();
    params.sort.order(&mut ordered);
    let mut page: Vec<_> = ordered
        .into_iter()
        .filter(|score| params.after(score))
        .take(params.limit + 1)
        .collect();
    let has_more = page.len() > params.limit;
    page.truncate(params.limit);
    let next = if has_more {
        page.last().map(|score| cursor(score, params.sort))
    } else {
        None
    };
    let scores = render_list(&state, &viewer, room, item, &page, &positions).await?;
    let mut user_score = if let Some(score) = highs.iter().find(|score| score.user == viewer.id) {
        render_score(
            &state,
            &viewer,
            room,
            item,
            score.id,
            false,
            positions.get(&score.id).copied(),
        )
        .await?
    } else {
        Value::Null
    };
    if params.around && !user_score.is_null() {
        user_score["scores_around"] = scores_around(
            &state,
            &viewer,
            room,
            item,
            user_score["id"].as_i64().ok_or_else(not_found)?,
            &highs,
            &params,
        )
        .await?;
    }
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
    let highs = selected_highs(
        &load_attempts(&state, &viewer, room, Some(item)).await?,
        head_to_head(&state, room).await?,
    );
    let positions = positions(&highs)?;
    let mut value = render_score(
        &state,
        &viewer,
        room,
        item,
        score,
        false,
        positions.get(&score).copied(),
    )
    .await?;
    if params.around {
        value["scores_around"] =
            scores_around(&state, &viewer, room, item, score, &highs, &params).await?;
    }
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
    let highs = selected_highs(
        &load_attempts(&state, &viewer, room, Some(item)).await?,
        head_to_head(&state, room).await?,
    );
    let positions = positions(&highs)?;
    let score = highs
        .iter()
        .find(|score| score.user == user)
        .ok_or_else(not_found)?;
    let mut value = render_score(
        &state,
        &viewer,
        room,
        item,
        score.id,
        false,
        positions.get(&score.id).copied(),
    )
    .await?;
    if params.around {
        value["scores_around"] =
            scores_around(&state, &viewer, room, item, score.id, &highs, &params).await?;
    }
    Ok(Json(value))
}

async fn leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    check_ids(room, None)?;
    let viewer = common::bearer(&state, &headers).await?;
    if !room_history_authorized(&state, &viewer, room).await? {
        return Err(not_found());
    }
    let attempts = load_attempts(&state, &viewer, room, None).await?;
    let highs = selected_highs(&attempts, head_to_head(&state, room).await?);
    let aggregates = aggregate_scores(&attempts, &highs)?;
    let mut leaderboard = Vec::with_capacity(aggregates.len());
    let mut user_score = Value::Null;
    for (index, aggregate) in aggregates.iter().enumerate() {
        let value = aggregate_json(&state, room, aggregate, native_count(index + 1)?).await?;
        if aggregate.user == viewer.id {
            user_score = value.clone();
        }
        leaderboard.push(value);
    }
    Ok(Json(
        json!({"leaderboard":leaderboard,"user_score":user_score}),
    ))
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

    fn attempt(id: i64, user: i64, item: i64, total: i64, passed: bool) -> Attempt {
        Attempt {
            user,
            item,
            score: Some(Score {
                id,
                user,
                item,
                total,
                accuracy: 0.9,
                passed,
            }),
        }
    }

    #[test]
    fn highs_keep_earlier_ties_and_head_to_head_failed_highs() {
        let attempts = vec![
            attempt(9, 1, 1, 900_000, true),
            attempt(8, 1, 1, 900_000, true),
            attempt(10, 1, 1, 950_000, false),
            attempt(11, 2, 1, 800_000, true),
        ];
        let hth = selected_highs(&attempts, true);
        assert_eq!(
            hth.iter().map(|score| score.id).collect::<Vec<_>>(),
            [10, 11]
        );
        let playlist = selected_highs(&attempts, false);
        assert_eq!(
            playlist.iter().map(|score| score.id).collect::<Vec<_>>(),
            [8, 11]
        );
    }

    #[test]
    fn aggregate_rank_uses_max_selected_id_and_counts_issued_attempts() {
        let mut attempts = vec![
            attempt(2, 1, 1, 500_000, true),
            attempt(7, 1, 2, 500_000, true),
            attempt(4, 2, 1, 1_000_000, true),
        ];
        attempts.push(Attempt {
            user: 1,
            item: 2,
            score: None,
        });
        let highs = selected_highs(&attempts, true);
        let aggregates = aggregate_scores(&attempts, &highs).unwrap();
        assert_eq!(aggregates[0].user, 2);
        assert_eq!(aggregates[1].user, 1);
        assert_eq!(aggregates[1].attempts, 3);
        assert_eq!(aggregates[1].completed, 2);
        assert_eq!(aggregates[1].accuracy, 0.9);
    }

    #[test]
    fn distribution_has_all_thirteen_bins_and_all_passing_attempts() {
        let mut stats = ItemStats::default();
        for bin in 0..13 {
            for total in [bin * 100_000, bin * 100_000 + 99_999] {
                stats
                    .add(&Score {
                        id: total + 1,
                        user: 1,
                        item: 1,
                        total,
                        accuracy: 1.0,
                        passed: true,
                    })
                    .unwrap();
            }
        }
        stats
            .add(&Score {
                id: 2_000_000,
                user: 1,
                item: 1,
                total: 3_000_000,
                accuracy: 0.5,
                passed: false,
            })
            .unwrap();
        assert_eq!(stats.bins, [2; 13]);
        assert_eq!(stats.cumulative, 16_899_987);
        assert_eq!(stats.last_processed, 2_000_000);
        assert!(
            stats
                .add(&Score {
                    id: 2_000_001,
                    user: 1,
                    item: 1,
                    total: i64::MAX,
                    accuracy: 1.0,
                    passed: true
                })
                .is_err()
        );
    }

    #[test]
    fn aggregate_overflow_is_an_error() {
        let attempts = vec![attempt(1, 1, 1, i64::MAX, true), attempt(2, 1, 2, 1, true)];
        assert!(aggregate_scores(&attempts, &selected_highs(&attempts, true)).is_err());
    }

    #[test]
    fn native_cursor_uses_stable_total_and_id_order() {
        let query = HashMap::from([
            ("cursor[total_score]".into(), "900000".into()),
            ("cursor[id]".into(), "10".into()),
            ("limit".into(), "2".into()),
        ]);
        let params = IndexParams::parse(&query).unwrap();
        assert!(params.after(&Score {
            id: 11,
            user: 2,
            item: 1,
            total: 900000,
            accuracy: 1.0,
            passed: true
        }));
        assert!(!params.after(&Score {
            id: 9,
            user: 2,
            item: 1,
            total: 900000,
            accuracy: 1.0,
            passed: true
        }));
        assert!(params.after(&Score {
            id: 8,
            user: 2,
            item: 1,
            total: 800000,
            accuracy: 1.0,
            passed: true
        }));
        assert!(IndexParams::parse(&HashMap::from([("cursor[id]".into(), "10".into())])).is_err());
        assert_eq!(params.json(), json!({"sort":"score_desc","limit":2}));
    }

    #[test]
    fn higher_pages_reverse_total_and_tie_order_without_repeating_pivot() {
        let attempts = vec![
            attempt(8, 1, 1, 900_000, true),
            attempt(9, 2, 1, 900_000, true),
            attempt(10, 3, 1, 800_000, true),
        ];
        let mut scores = selected_highs(&attempts, true);
        let canonical: Vec<_> = scores.iter().map(|score| score.id).collect();
        Sort::ScoreDesc.reverse().order(&mut scores);
        assert_eq!(canonical, [8, 9, 10]);
        assert_eq!(
            scores.iter().map(|score| score.id).collect::<Vec<_>>(),
            [10, 9, 8]
        );
        let params = IndexParams {
            sort: Sort::ScoreAsc,
            limit: 2,
            cursor: Some((900_000, 9)),
            around: false,
        };
        assert_eq!(
            scores
                .iter()
                .filter(|score| params.after(score))
                .map(|score| score.id)
                .collect::<Vec<_>>(),
            [8]
        );
    }
}
