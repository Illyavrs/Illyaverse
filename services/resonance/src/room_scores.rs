//! Durable native room-score intents. Network ingestion never borrows a transaction.
use crate::common::{self, ApiError, AppState};
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode},
    routing::{post, put},
};
use chrono::{DateTime, Utc};
use resonance_protocol::lazer::{ApiMod, ScoreTokenRequest, SoloScoreSubmission};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};

const BATCH: i64 = 16;
const LEASE_SECONDS: i64 = 45;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v2/rooms/{room}/playlist/{item}/scores", post(create))
        .route(
            "/api/v2/rooms/{room}/playlist/{item}/scores/{score}",
            put(submit),
        )
}

fn invalid(code: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, code)
}
fn unavailable(code: &'static str) -> ApiError {
    ApiError(StatusCode::SERVICE_UNAVAILABLE, code)
}
fn digest(value: &Value) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("invalid_room_submission"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Native mods are sets; omit/default settings and order do not alter context.
pub(crate) fn canonical_mods(value: &Value) -> Result<Value, ApiError> {
    let mut mods: Vec<ApiMod> =
        serde_json::from_value(value.clone()).map_err(|_| invalid("invalid_score_mods"))?;
    mods.sort_by(|left, right| left.acronym.cmp(&right.acronym));
    Ok(json!(
        mods.into_iter()
            .map(|modifier| { json!({"acronym":modifier.acronym,"settings":modifier.settings}) })
            .collect::<Vec<_>>()
    ))
}

#[derive(Deserialize)]
struct CreateToken {
    beatmap_id: i64,
    beatmap_hash: String,
    version_hash: String,
    ruleset_id: u8,
}

async fn lock_room(tx: &mut Transaction<'_, Postgres>, room: i64) -> Result<PgRow, ApiError> {
    sqlx::query("SELECT id,state,current_round_id FROM lazer_rooms WHERE id=$1 FOR UPDATE")
        .bind(room)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "room_not_found"))
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((room, item)): Path<(i64, i64)>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let input: CreateToken = crate::lazer::native_form(request).await?;
    ScoreTokenRequest {
        beatmap_hash: input.beatmap_hash.clone(),
        version_hash: input.version_hash.clone(),
        ruleset_id: input.ruleset_id,
    }
    .validate()?;
    if input.beatmap_id <= 0 || room <= 0 || item <= 0 {
        return Err(invalid("invalid_room_score_context"));
    }
    common::rate_limit(&state, format!("room-score-token:{}", player.id), 60).await?;
    let mut tx = state.pool.begin().await?;
    let current = lock_room(&mut tx, room).await?;
    common::lazer_account_in_tx(&mut tx, &headers, player.id).await?;
    let round = current
        .get::<Option<i64>, _>("current_round_id")
        .ok_or(invalid("room_round_required"))?;
    let participant = sqlx::query("SELECT p.*,b.official_id,b.md5,r.expires_at FROM lazer_room_round_participants p JOIN lazer_room_rounds r ON r.id=p.round_id JOIN beatmaps b ON b.id=p.beatmap_id JOIN lazer_room_members m ON m.room_id=r.room_id AND m.user_id=p.user_id WHERE p.round_id=$1 AND p.user_id=$2 AND r.room_id=$3 AND r.playlist_item_id=$4 AND r.status='loading' AND r.expires_at>now() AND p.aborted_at IS NULL AND m.state=2")
        .bind(round).bind(player.id).bind(room).bind(item).fetch_optional(&mut *tx).await?
        .ok_or(invalid("room_loading_participant_required"))?;
    if current.get::<i16, _>("state") != 1 {
        return Err(invalid("room_loading_participant_required"));
    }
    let map: i64 = participant.get("beatmap_id");
    if ![Some(map), participant.get::<Option<i64>, _>("official_id")]
        .contains(&Some(input.beatmap_id))
        || participant.get::<i16, _>("ruleset_id") != i16::from(input.ruleset_id)
        || !participant
            .get::<String, _>("beatmap_checksum")
            .eq_ignore_ascii_case(&input.beatmap_hash)
        || !participant
            .get::<String, _>("md5")
            .eq_ignore_ascii_case(&input.beatmap_hash)
    {
        return Err(invalid("room_score_context_mismatch"));
    }
    let token = if let Some(previous) = sqlx::query("SELECT * FROM lazer_score_tokens WHERE scope='room' AND round_id=$1 AND user_id=$2 FOR UPDATE")
        .bind(round).bind(player.id).fetch_optional(&mut *tx).await? {
        if previous.get::<i64, _>("room_id") != room
            || previous.get::<i64, _>("playlist_item_id") != item
            || previous.get::<i64, _>("beatmap_id") != map
            || previous.get::<i16, _>("ruleset_id") != i16::from(input.ruleset_id)
            || previous.get::<String, _>("version_hash") != input.version_hash
            || previous.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some()
        {
            return Err(ApiError(StatusCode::CONFLICT, "room_score_token_already_issued"));
        }
        previous.get::<i64, _>("id")
    } else {
        sqlx::query_scalar::<_, i64>("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope,room_id,playlist_item_id,round_id) VALUES($1,$2,$3,$4,$5,'room',$6,$7,$8) RETURNING id")
            .bind(player.id).bind(map).bind(i16::from(input.ruleset_id)).bind(input.version_hash)
            .bind(participant.get::<DateTime<Utc>, _>("expires_at")).bind(room).bind(item).bind(round)
            .fetch_one(&mut *tx).await?
    };
    tx.commit().await?;
    common::bearer(&state, &headers).await?;
    Ok(Json(json!({"id":token})))
}

async fn bound_token(
    tx: &mut Transaction<'_, Postgres>,
    room: i64,
    item: i64,
    token: i64,
    uid: i64,
) -> Result<PgRow, ApiError> {
    sqlx::query("SELECT t.*,p.beatmap_id frozen_map,p.ruleset_id frozen_ruleset,p.beatmap_checksum,p.mods frozen_mods,p.aborted_at participant_aborted,r.status round_status,r.expires_at round_expires,b.md5,b.official_id FROM lazer_score_tokens t JOIN lazer_room_rounds r ON r.id=t.round_id AND r.room_id=t.room_id AND r.playlist_item_id=t.playlist_item_id JOIN lazer_room_round_participants p ON p.round_id=t.round_id AND p.user_id=t.user_id JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1 AND t.user_id=$2 AND t.scope='room' AND t.room_id=$3 AND t.playlist_item_id=$4 FOR UPDATE OF t")
        .bind(token).bind(uid).bind(room).bind(item).fetch_optional(&mut **tx).await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "invalid_room_score_token"))
}

fn validate_binding(
    row: &PgRow,
    payload: &Value,
    input: &SoloScoreSubmission,
) -> Result<(), ApiError> {
    if row.get::<i64, _>("beatmap_id") != row.get::<i64, _>("frozen_map")
        || row.get::<i16, _>("ruleset_id") != row.get::<i16, _>("frozen_ruleset")
        || row.get::<i16, _>("ruleset_id") != i16::from(input.ruleset_id)
        || !row
            .get::<String, _>("md5")
            .eq_ignore_ascii_case(&row.get::<String, _>("beatmap_checksum"))
        || canonical_mods(&json!(input.mods))?
            != canonical_mods(&row.get::<Value, _>("frozen_mods"))?
    {
        return Err(invalid("room_score_context_mismatch"));
    }
    for (key, expected) in [
        ("user_id", row.get::<i64, _>("user_id")),
        ("room_id", row.get("room_id")),
        ("playlist_item_id", row.get("playlist_item_id")),
        ("round_id", row.get("round_id")),
        ("id", row.get("id")),
    ] {
        if payload
            .get(key)
            .is_some_and(|value| !value.is_null() && value.as_i64() != Some(expected))
        {
            return Err(invalid("room_score_context_mismatch"));
        }
    }
    if payload.get("beatmap_id").is_some_and(|value| {
        !value.is_null()
            && ![
                Some(row.get::<i64, _>("beatmap_id")),
                row.get::<Option<i64>, _>("official_id"),
            ]
            .contains(&value.as_i64())
    }) {
        return Err(invalid("room_score_context_mismatch"));
    }
    for key in ["beatmap_hash", "beatmap_checksum"] {
        if payload.get(key).is_some_and(|value| {
            !value.is_null()
                && !value.as_str().is_some_and(|hash| {
                    hash.eq_ignore_ascii_case(&row.get::<String, _>("beatmap_checksum"))
                })
        }) {
            return Err(invalid("room_score_context_mismatch"));
        }
    }
    if payload.get("version_hash").is_some_and(|value| {
        !value.is_null() && value.as_str() != Some(row.get::<String, _>("version_hash").as_str())
    }) {
        return Err(invalid("room_score_context_mismatch"));
    }
    Ok(())
}

fn validate_window(row: &PgRow) -> Result<(), ApiError> {
    if row.get::<Option<DateTime<Utc>>, _>("revoked_at").is_some()
        || row
            .get::<Option<DateTime<Utc>>, _>("participant_aborted")
            .is_some()
        || row.get::<String, _>("round_status") == "aborted"
        || row.get::<String, _>("processing_status") == "revoked"
    {
        return Err(invalid("room_score_token_revoked"));
    }
    if row.get::<DateTime<Utc>, _>("expires_at") <= Utc::now()
        || row.get::<DateTime<Utc>, _>("round_expires") <= Utc::now()
    {
        return Err(invalid("room_score_token_expired"));
    }
    Ok(())
}

enum Preflight {
    Done,
    Claimed,
    Busy,
}

#[derive(Clone, Copy)]
struct Attempt {
    uid: i64,
    room: i64,
    item: i64,
    token: i64,
}

async fn originating_session_valid(
    tx: &mut Transaction<'_, Postgres>,
    row: &PgRow,
) -> Result<bool, ApiError> {
    let Some(session) = row.get::<Option<uuid::Uuid>, _>("received_session_id") else {
        return Ok(false);
    };
    Ok(sqlx::query("SELECT id FROM lazer_sessions WHERE id=$1 AND user_id=$2 AND access_expires_at>clock_timestamp() FOR UPDATE")
        .bind(session).bind(row.get::<i64, _>("user_id")).fetch_optional(&mut **tx).await?.is_some())
}

async fn preflight(
    state: &AppState,
    headers: &HeaderMap,
    attempt: Attempt,
    payload: &Value,
    input: &SoloScoreSubmission,
    sha: &str,
) -> Result<Preflight, ApiError> {
    let Attempt {
        uid,
        room,
        item,
        token,
    } = attempt;
    let mut tx = state.pool.begin().await?;
    lock_room(&mut tx, room).await?;
    common::lazer_account_in_tx(&mut tx, headers, uid).await?;
    let row = bound_token(&mut tx, room, item, token, uid).await?;
    validate_binding(&row, payload, input)?;
    if row
        .get::<Option<Value>, _>("submission")
        .as_ref()
        .is_some_and(|saved| saved != payload)
        || row
            .get::<Option<String>, _>("submission_sha256")
            .as_deref()
            .is_some_and(|saved| saved != sha)
    {
        return Err(ApiError(StatusCode::CONFLICT, "score_token_already_used"));
    }
    if row.get::<String, _>("processing_status") == "processed"
        && row.get::<Option<i64>, _>("score_id").is_some()
    {
        tx.commit().await?;
        return Ok(Preflight::Done);
    }
    validate_window(&row)?;
    if row.get::<String, _>("processing_status") == "issued" {
        let session: uuid::Uuid = sqlx::query_scalar("SELECT id FROM lazer_sessions WHERE user_id=$1 AND access_hash=$2 AND access_expires_at>clock_timestamp()")
            .bind(uid).bind(common::bearer_hash(headers)?).fetch_optional(&mut *tx).await?
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_token"))?;
        sqlx::query("UPDATE lazer_score_tokens SET submission=$2,submission_sha256=$3,received_at=now(),received_session_id=$4,processing_status='received',retry_at=now() WHERE id=$1")
            .bind(token).bind(payload).bind(sha).bind(session).execute(&mut *tx).await?;
    } else if !originating_session_valid(&mut tx, &row).await? {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_originating_session",
        ));
    }
    let claimed = claim(&mut tx, token, true).await?;
    tx.commit().await?; // Canonical input is durable before the HTTP request.
    Ok(if claimed {
        Preflight::Claimed
    } else {
        Preflight::Busy
    })
}

async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    token: i64,
    manual_retry: bool,
) -> Result<bool, ApiError> {
    Ok(sqlx::query("UPDATE lazer_score_tokens SET retry_at=now()+make_interval(secs=>$2::double precision),processing_attempts=processing_attempts+1,processing_error=NULL WHERE id=$1 AND scope='room' AND processing_status='received' AND (retry_at<=now() OR ($3 AND processing_error IS NOT NULL))")
        .bind(token).bind(LEASE_SECONDS as f64).bind(manual_retry).execute(&mut **tx).await?.rows_affected() == 1)
}

async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((room, item, token)): Path<(i64, i64, i64)>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let input: SoloScoreSubmission =
        serde_json::from_value(payload.clone()).map_err(|_| invalid("invalid_room_submission"))?;
    input.validate()?;
    crate::lazer::legacy_mods(input.ruleset_id, &input.mods)?;
    let sha = digest(&payload)?;
    let attempt = Attempt {
        uid: player.id,
        room,
        item,
        token,
    };
    let mut ready = false;
    // Concurrent identical retries wait without a transaction for the claimant.
    for _ in 0..100 {
        match preflight(&state, &headers, attempt, &payload, &input, &sha).await? {
            Preflight::Done => {
                ready = true;
                break;
            }
            Preflight::Claimed => {
                if let Err(error) = process(
                    &state,
                    room,
                    item,
                    token,
                    player.id,
                    &payload,
                    Some(&headers),
                )
                .await
                {
                    defer(&state, token, error.1).await?;
                    return Err(error);
                }
                ready = true;
                break;
            }
            Preflight::Busy => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
    if !ready {
        return Err(unavailable("room_score_processing_pending"));
    }
    after_processing(&state, player.id, token).await;
    let current = common::bearer(&state, &headers).await?;
    if current.id != player.id {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_token"));
    }
    Ok(Json(
        crate::room_results::score_for_token(&state, &current, room, item, token).await?,
    ))
}

async fn process(
    state: &AppState,
    room: i64,
    item: i64,
    token: i64,
    uid: i64,
    payload: &Value,
    headers: Option<&HeaderMap>,
) -> Result<(), ApiError> {
    if state.service_key.len() < 32 {
        return Err(unavailable("internal_ingestion_not_configured"));
    }
    let input: SoloScoreSubmission = serde_json::from_value(payload.clone())
        .map_err(|_| invalid("invalid_stored_submission"))?;
    input.validate()?;
    let mods = crate::lazer::legacy_mods(input.ruleset_id, &input.mods)?;
    // Read identity only; preflight persisted and claimed this immutable intent.
    let map: i64 = sqlx::query_scalar("SELECT beatmap_id FROM lazer_score_tokens WHERE id=$1 AND user_id=$2 AND scope='room' AND room_id=$3 AND playlist_item_id=$4 AND submission=$5")
        .bind(token).bind(uid).bind(room).bind(item).bind(payload).fetch_optional(&state.pool).await?
        .ok_or(invalid("invalid_room_score_token"))?;
    let response = state.http.post(format!("{}/api/v1/internal/scores", state.sanctuary_url.trim_end_matches('/')))
        .header("x-illya-service-key", state.service_key.as_str())
        .json(&json!({"user_id":uid,"beatmap_id":map,"mode":input.ruleset_id,"score":input.total_score,
            "accuracy":input.accuracy*100.0,"max_combo":input.max_combo,"mods":mods,"api_mods":input.mods,
            "passed":input.passed,"hit_counts":input.legacy_hit_counts(),"native_submission":input,
            "client":"lazer","ingestion_key":format!("lazer:{token}")}))
        .timeout(std::time::Duration::from_secs(30)).send().await
        .map_err(|_| unavailable("score_ingestion_unavailable"))?;
    if !response.status().is_success() {
        return Err(
            if response.status().is_server_error()
                || matches!(response.status().as_u16(), 401 | 403 | 404)
            {
                unavailable("score_ingestion_unavailable")
            } else {
                invalid("score_rejected_by_ingestion")
            },
        );
    }
    let result: Value = response
        .json()
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_ingestion_response"))?;
    let score_id = result["id"].as_i64().filter(|id| *id > 0).ok_or(ApiError(
        StatusCode::BAD_GATEWAY,
        "invalid_ingestion_response",
    ))?;
    let mut tx = state.pool.begin().await?;
    lock_room(&mut tx, room).await?;
    if let Some(headers) = headers {
        common::lazer_account_in_tx(&mut tx, headers, uid).await?;
    } else {
        sqlx::query("SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR UPDATE")
            .bind(uid)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_account"))?;
    }
    let row = bound_token(&mut tx, room, item, token, uid).await?;
    validate_binding(&row, payload, &input)?;
    if row.get::<Option<Value>, _>("submission").as_ref() != Some(payload)
        || row.get::<Option<String>, _>("submission_sha256").as_deref()
            != Some(digest(payload)?.as_str())
    {
        return Err(ApiError(StatusCode::CONFLICT, "score_token_already_used"));
    }
    if row.get::<String, _>("processing_status") == "processed" {
        if row.get::<Option<i64>, _>("score_id") != Some(score_id) {
            return Err(invalid("room_ingestion_link_mismatch"));
        }
        tx.commit().await?;
        return Ok(());
    }
    validate_window(&row)?;
    if !originating_session_valid(&mut tx, &row).await? {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_originating_session",
        ));
    }
    // Sanctuary's old idempotency guard does not compare the entire input. This
    // extra check refuses a foreign/mutated orphan instead of attaching it.
    let score = sqlx::query("SELECT user_id,beatmap_id,mode,score_variant,client,ingestion_key,native_submission,score,accuracy,max_combo,mods,passed,hit_counts,api_mods FROM scores WHERE id=$1 FOR SHARE")
        .bind(score_id).fetch_optional(&mut *tx).await?.ok_or(invalid("room_ingestion_link_mismatch"))?;
    let native_matches = score
        .get::<Option<Value>, _>("native_submission")
        .is_some_and(|native| {
            serde_json::from_value::<SoloScoreSubmission>(native)
                .ok()
                .is_some_and(|saved| json!(saved) == json!(input))
        });
    if score.get::<i64, _>("user_id") != uid
        || score.get::<i64, _>("beatmap_id") != map
        || score.get::<i16, _>("mode") != i16::from(input.ruleset_id)
        || score.get::<String, _>("score_variant")
            != illya_domain::ScoreVariant::from_legacy(input.ruleset_id, mods)
                .map_err(|_| invalid("invalid_score_mods"))?
                .as_str()
        || score.get::<String, _>("client") != "lazer"
        || score.get::<Option<String>, _>("ingestion_key").as_deref()
            != Some(format!("lazer:{token}").as_str())
        || !native_matches
        || score.get::<i64, _>("score") != input.total_score as i64
        || (score.get::<f64, _>("accuracy") - input.accuracy * 100.0).abs() > 1e-6
        || score.get::<i32, _>("max_combo") != input.max_combo as i32
        || score.get::<i64, _>("mods") != i64::from(mods)
        || score.get::<bool, _>("passed") != input.passed
        || score.get::<Value, _>("hit_counts") != input.legacy_hit_counts()
        || canonical_mods(&score.get::<Value, _>("api_mods"))?
            != canonical_mods(&json!(input.mods))?
    {
        return Err(invalid("room_ingestion_link_mismatch"));
    }
    sqlx::query("UPDATE lazer_score_tokens SET consumed_at=now(),score_id=$2,processing_status='processed',processing_error=NULL WHERE id=$1")
        .bind(token).bind(score_id).execute(&mut *tx).await?;
    let rank = crate::room_results::new_best_rank(&mut tx, token).await?;
    sqlx::query("INSERT INTO lazer_room_score_events(token_id,room_id,item_id,score_id,user_id,total_score,new_rank) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(token_id) DO NOTHING")
        .bind(token).bind(room).bind(item).bind(score_id).bind(uid).bind(input.total_score as i64).bind(rank)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn defer(state: &AppState, token: i64, code: &'static str) -> Result<(), ApiError> {
    sqlx::query("UPDATE lazer_score_tokens SET processing_error=$2,retry_at=now()+make_interval(secs=>LEAST(300,5*power(2,LEAST(processing_attempts,6)))::double precision) WHERE id=$1 AND scope='room' AND processing_status='received'")
        .bind(token).bind(code).execute(&state.pool).await?;
    Ok(())
}

async fn after_processing(state: &AppState, uid: i64, token: i64) {
    if let Err(error) = crate::replay_capture::finalize(state, uid, token).await {
        tracing::warn!(
            token_id = token,
            code = error.1,
            "room score replay publication deferred"
        );
    }
    if let Err(error) = crate::signalr::publish_room_score_events(state).await {
        tracing::warn!(code = error.1, "room score event delivery deferred");
    }
}

/// Bounded recovery of saved intents, using the original shared ingestion key.
/// Invalid attempts become explicit revoked receipts; payloads are never edited.
pub(crate) async fn recover_pending(state: &AppState) -> Result<usize, ApiError> {
    let candidates: Vec<(i64,i64,i64,i64)> = sqlx::query_as("SELECT id,user_id,room_id,playlist_item_id FROM lazer_score_tokens WHERE scope='room' AND processing_status='received' AND retry_at<=now() ORDER BY retry_at,id LIMIT $1")
        .bind(BATCH).fetch_all(&state.pool).await?;
    let mut processed = 0;
    for (token, uid, room, item) in candidates {
        let mut tx = state.pool.begin().await?;
        lock_room(&mut tx, room).await?;
        let active =
            sqlx::query("SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR UPDATE")
                .bind(uid)
                .fetch_optional(&mut *tx)
                .await?
                .is_some();
        let row = bound_token(&mut tx, room, item, token, uid).await?;
        if row.get::<String, _>("processing_status") != "received" {
            continue;
        }
        let payload: Value = row
            .get::<Option<Value>, _>("submission")
            .ok_or(invalid("invalid_stored_submission"))?;
        let input = serde_json::from_value::<SoloScoreSubmission>(payload.clone());
        let session_valid = originating_session_valid(&mut tx, &row).await?;
        let valid = active
            && session_valid
            && validate_window(&row).is_ok()
            && input.as_ref().is_ok_and(|input| {
                input.validate().is_ok()
                    && crate::lazer::legacy_mods(input.ruleset_id, &input.mods).is_ok()
                    && validate_binding(&row, &payload, input).is_ok()
            })
            && row.get::<Option<String>, _>("submission_sha256").as_deref()
                == Some(digest(&payload)?.as_str());
        if !valid {
            sqlx::query("UPDATE lazer_score_tokens SET processing_status='revoked',revoked_at=COALESCE(revoked_at,now()),revoke_reason=COALESCE(revoke_reason,'invalid_saved_intent'),processing_error='invalid_saved_intent' WHERE id=$1 AND processing_status='received'")
                .bind(token).execute(&mut *tx).await?;
            tx.commit().await?;
            continue;
        }
        if !claim(&mut tx, token, false).await? {
            continue;
        }
        tx.commit().await?;
        match process(state, room, item, token, uid, &payload, None).await {
            Ok(()) => {
                processed += 1;
                after_processing(state, uid, token).await;
            }
            Err(error) => {
                defer(state, token, error.1).await?;
                tracing::warn!(
                    token_id = token,
                    code = error.1,
                    "room score recovery deferred"
                );
            }
        }
    }
    Ok(processed)
}

/// Used after an ended spectator connection registers its exact token watcher.
pub(crate) async fn processed_score_id(
    state: &AppState,
    uid: i64,
    token: i64,
) -> Result<Option<i64>, ApiError> {
    Ok(sqlx::query_scalar("SELECT t.score_id FROM lazer_score_tokens t JOIN users u ON u.id=t.user_id JOIN lazer_room_rounds r ON r.id=t.round_id AND r.room_id=t.room_id AND r.playlist_item_id=t.playlist_item_id JOIN lazer_room_round_participants p ON p.round_id=t.round_id AND p.user_id=t.user_id JOIN beatmaps b ON b.id=t.beatmap_id JOIN scores s ON s.id=t.score_id AND s.user_id=t.user_id AND s.beatmap_id=t.beatmap_id AND s.mode=t.ruleset_id AND s.client='lazer' AND s.ingestion_key='lazer:'||t.id::text WHERE t.id=$1 AND t.user_id=$2 AND t.scope='room' AND t.processing_status='processed' AND t.revoked_at IS NULL AND p.aborted_at IS NULL AND r.status<>'aborted' AND NOT u.is_restricted AND t.beatmap_id=p.beatmap_id AND t.ruleset_id=p.ruleset_id AND lower(b.md5)=lower(p.beatmap_checksum) AND s.verification<>'rejected'")
        .bind(token).bind(uid).fetch_optional(&state.pool).await?.flatten())
}
