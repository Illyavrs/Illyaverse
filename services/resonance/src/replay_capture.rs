//! Durable, bounded native spectator replay recording. No gameplay verification.
use crate::common::{ApiError, AppState};
use axum::http::StatusCode;
use resonance_protocol::{
    lazer::SoloScoreSubmission,
    lazer_replay::{self, ReplayScore},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::io::AsyncWriteExt;

#[cfg(test)]
#[path = "replay_capture_tests.rs"]
mod tests;

fn error(code: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, code)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
static ENCODERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Bounded recovery after a crash between token commit and replay publication.
/// Call on startup and periodically; row locks make overlapping calls harmless.
pub async fn recover_pending(state: &AppState) -> Result<usize, ApiError> {
    let rows:Vec<(i64,i64)>=sqlx::query_as("SELECT c.user_id,c.token_id FROM lazer_replay_captures c JOIN lazer_score_tokens t ON t.id=c.token_id JOIN users u ON u.id=c.user_id WHERE c.ended_at IS NOT NULL AND c.completed_at IS NULL AND c.abandoned_at IS NULL AND t.score_id IS NOT NULL AND NOT u.is_restricted ORDER BY c.updated_at,c.token_id LIMIT 32").fetch_all(&state.pool).await?;
    let mut published = 0;
    for (uid, token) in rows {
        match finalize(state, uid, token).await {
            Ok(Some(_)) => published += 1,
            Ok(None) => {}
            Err(error) => {
                // Move failures behind untouched captures so invalid input cannot
                // starve another user's pending replay at the head of the batch.
                sqlx::query("UPDATE lazer_replay_captures SET updated_at=now() WHERE token_id=$1 AND completed_at IS NULL").bind(token).execute(&state.pool).await?;
                tracing::warn!(
                    token_id = token,
                    code = error.1,
                    "pending replay recovery deferred"
                );
            }
        }
    }
    Ok(published)
}
pub async fn begin(
    state: &AppState,
    uid: i64,
    token: i64,
    connection: &str,
    initial: &Value,
) -> Result<(), ApiError> {
    // Only abandoned, unconsumed token captures are expired here. Published scores
    // and their replays are never removed by this temporary-data cleanup.
    sqlx::query("DELETE FROM lazer_replay_captures c USING lazer_score_tokens t WHERE c.token_id=t.id AND t.expires_at<now()-interval '1 day' AND c.completed_at IS NULL AND t.score_id IS NULL").execute(&state.pool).await?;
    let mut tx = state.pool.begin().await?;
    // Serialize capacity without blocking the score INSERT's user FK check.
    sqlx::query("SELECT pg_advisory_xact_lock(1196575833,($1::bigint & 2147483647)::integer)")
        .bind(uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 AND NOT is_restricted")
        .bind(uid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(error("user_restricted"))?;
    let row=sqlx::query("SELECT t.*,b.official_id FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1 AND t.user_id=$2 AND t.expires_at>now() FOR UPDATE OF t").bind(token).bind(uid).fetch_optional(&mut *tx).await?.ok_or(error("invalid_score_token"))?;
    if initial["rulesetID"].as_i64() != Some(i64::from(row.get::<i16, _>("ruleset_id")))
        || ![
            Some(row.get::<i64, _>("beatmap_id")),
            row.get::<Option<i64>, _>("official_id"),
        ]
        .contains(&initial["beatmapID"].as_i64())
    {
        return Err(error("score_token_beatmap_mismatch"));
    }
    let invalid_room:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lazer_room_members m JOIN lazer_rooms r ON r.id=m.room_id WHERE m.user_id=$1 AND (m.state NOT IN(4,5) OR r.state NOT IN(1,2)))").bind(uid).fetch_one(&mut *tx).await?;
    if invalid_room {
        return Err(error("room_gameplay_required"));
    }
    if let Some(previous)=sqlx::query("SELECT initial_state,ended_at,completed_at,abandoned_at FROM lazer_replay_captures WHERE token_id=$1 FOR UPDATE").bind(token).fetch_optional(&mut *tx).await? {
        if previous.get::<Option<chrono::DateTime<chrono::Utc>>,_>("abandoned_at").is_some() { return Err(error("replay_token_already_used")); }
        if previous.get::<Option<chrono::DateTime<chrono::Utc>>,_>("ended_at").is_some() || previous.get::<Option<chrono::DateTime<chrono::Utc>>,_>("completed_at").is_some() { return Err(error("replay_token_already_used")); }
        if previous.get::<Value,_>("initial_state")!=*initial { return Err(error("replay_session_state_mismatch")); }
        sqlx::query("UPDATE lazer_replay_captures SET interrupted=interrupted OR connection_key<>$2,connection_key=$2,updated_at=now() WHERE token_id=$1").bind(token).bind(connection).execute(&mut *tx).await?;
    } else {
        let pending:i64=sqlx::query_scalar("SELECT count(*) FROM lazer_replay_captures WHERE user_id=$1 AND completed_at IS NULL AND abandoned_at IS NULL").bind(uid).fetch_one(&mut *tx).await?;
        if pending>=4 { return Err(ApiError(StatusCode::TOO_MANY_REQUESTS,"too_many_replay_captures")); }
        if row.get::<Option<i64>,_>("score_id").is_some() { return Err(error("score_token_already_used")); }
        sqlx::query("INSERT INTO lazer_replay_captures(token_id,user_id,connection_key,initial_state) VALUES($1,$2,$3,$4)").bind(token).bind(uid).bind(connection).bind(initial).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn frames(
    state: &AppState,
    uid: i64,
    token: i64,
    connection: &str,
    bundle: &Value,
) -> Result<(), ApiError> {
    let header = &bundle["header"];
    let frames = bundle["frames"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .ok_or(error("invalid_frames"))?;
    // Ignore server receipt timestamps when detecting an exact transport retry.
    let mut canonical = bundle.clone();
    canonical["header"]["receivedTime"] = Value::Null;
    let hash = digest(&serde_json::to_vec(&canonical).map_err(|_| error("invalid_frames"))?);
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query(
        "SELECT * FROM lazer_replay_captures WHERE token_id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(token)
    .bind(uid)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(error("replay_session_required"))?;
    if row.get::<String, _>("connection_key") != connection {
        return Err(error("replay_connection_replaced"));
    }
    if row
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("ended_at")
        .is_some()
    {
        return Err(error("replay_session_ended"));
    }
    if row.get::<Option<String>, _>("last_bundle_hash").as_deref() == Some(hash.as_str()) {
        return Ok(());
    }
    let previous = row.get::<i32, _>("last_time");
    let mut last = previous;
    let mut text = String::new();
    for (index, frame) in frames.iter().enumerate() {
        let time = frame["time"]
            .as_f64()
            .filter(|v| v.is_finite() && v.abs() <= 86_400_000.0)
            .ok_or(error("invalid_frame_time"))?
            .round_ties_even() as i32;
        // Two synthetic stable preface frames may run backwards. After those,
        // reject ordering ambiguity instead of presenting a complete capture.
        if time < last && row.get::<i32, _>("frame_count") + index as i32 >= 2 {
            return Err(error("replay_frame_out_of_order"));
        }
        let x = frame["mouseX"].as_f64().unwrap_or(0.0);
        let y = frame["mouseY"].as_f64().unwrap_or(0.0);
        if !x.is_finite() || !y.is_finite() || x.abs() > 1_048_575.0 || y.abs() > 1_048_575.0 {
            return Err(error("invalid_frame_position"));
        }
        let buttons = frame["buttonState"]
            .as_u64()
            .filter(|v| *v <= 31)
            .ok_or(error("invalid_frame_buttons"))?;
        use std::fmt::Write;
        write!(&mut text, "{}|{x}|{y}|{buttons},", time - last)
            .map_err(|_| error("invalid_frames"))?;
        last = time;
    }
    let count = row.get::<i32, _>("frame_count") + frames.len() as i32;
    let length = row.get::<i32, _>("frame_bytes") as usize + text.len();
    if count > lazer_replay::MAX_FRAMES as i32 || length > lazer_replay::MAX_FRAME_TEXT {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "replay_capture_too_large",
        ));
    }
    sqlx::query("INSERT INTO lazer_replay_chunks(token_id,sequence,frame_text) VALUES($1,$2,$3)")
        .bind(token)
        .bind(row.get::<i32, _>("frame_count"))
        .bind(text.as_bytes())
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE lazer_replay_captures SET frame_bytes=$2,frame_count=$3,last_time=$4,last_bundle_hash=$5,final_header=$6,updated_at=now() WHERE token_id=$1").bind(token).bind(length as i32).bind(count).bind(last).bind(hash).bind(header).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn end(
    state: &AppState,
    uid: i64,
    token: i64,
    connection: &str,
    final_state: i16,
) -> Result<Option<i64>, ApiError> {
    if !(3..=5).contains(&final_state) {
        return Err(error("invalid_final_state"));
    }
    let mut tx = state.pool.begin().await?;
    let result=sqlx::query("UPDATE lazer_replay_captures SET final_state=$4,ended_at=COALESCE(ended_at,now()),abandoned_at=CASE WHEN $4=5 THEN now() ELSE abandoned_at END,updated_at=now() WHERE token_id=$1 AND user_id=$2 AND connection_key=$3 AND (final_state IS NULL OR final_state=$4)").bind(token).bind(uid).bind(connection).bind(final_state).execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(error("replay_connection_replaced"));
    }
    if final_state == 5 {
        sqlx::query("DELETE FROM lazer_replay_chunks WHERE token_id=$1")
            .bind(token)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    finalize(state, uid, token).await
}
pub async fn finalize(state: &AppState, uid: i64, token: i64) -> Result<Option<i64>, ApiError> {
    let mut tx = state.pool.begin().await?;
    let Some(capture) = sqlx::query(
        "SELECT * FROM lazer_replay_captures WHERE token_id=$1 AND user_id=$2 FOR UPDATE",
    )
    .bind(token)
    .bind(uid)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    if capture
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("ended_at")
        .is_none()
    {
        return Ok(None);
    }
    if capture
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("abandoned_at")
        .is_some()
    {
        return Ok(None);
    }
    let Some(row)=sqlx::query("SELECT COALESCE(s.native_submission,t.submission) submission,t.version_hash,s.id,s.mods,s.replay_path,s.created_at,b.md5,u.username FROM lazer_score_tokens t JOIN scores s ON s.id=t.score_id JOIN beatmaps b ON b.id=t.beatmap_id JOIN users u ON u.id=t.user_id WHERE t.id=$1 AND t.user_id=$2 AND s.user_id=$2 AND s.beatmap_id=t.beatmap_id AND NOT u.is_restricted FOR UPDATE OF s").bind(token).bind(uid).fetch_optional(&mut *tx).await? else { return Ok(None); };
    let score_id: i64 = row.get("id");
    if capture
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("completed_at")
        .is_some()
    {
        return Ok(Some(score_id));
    }
    if row.get::<Option<String>, _>("replay_path").is_some() {
        sqlx::query("DELETE FROM lazer_replay_chunks WHERE token_id=$1")
            .bind(token)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE lazer_replay_captures SET completed_at=now() WHERE token_id=$1")
            .bind(token)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(Some(score_id));
    }
    let input: SoloScoreSubmission = serde_json::from_value(row.get("submission"))
        .map_err(|_| error("invalid_stored_submission"))?;
    let header: Value = capture
        .get::<Option<Value>, _>("final_header")
        .ok_or(error("replay_frames_missing"))?;
    let initial: Value = capture.get("initial_state");
    let statistics = lazer_replay::statistics_from_signalr(&header["statistics"])?;
    let maximum = lazer_replay::statistics_from_signalr(&initial["maximumStatistics"])?;
    let mods: Vec<resonance_protocol::lazer::ApiMod> =
        serde_json::from_value(header["mods"].clone()).map_err(|_| error("invalid_replay_mods"))?;
    if header["totalScore"].as_u64() != Some(input.total_score)
        || header["maxCombo"].as_u64() != Some(u64::from(input.max_combo))
        || header["accuracy"]
            .as_f64()
            .is_none_or(|a| (a - input.accuracy).abs() > 1e-6)
        || statistics != lazer_replay::canonical_statistics(&input.statistics)
        || maximum != lazer_replay::canonical_statistics(&input.maximum_statistics)
        || serde_json::to_value(&mods).ok() != serde_json::to_value(&input.mods).ok()
        || (capture.get::<i16, _>("final_state") == 3) != input.passed
    {
        return Err(error("replay_metadata_mismatch"));
    }
    for (key, expected) in [
        (
            "totalScoreWithoutMods",
            json!(input.total_score_without_mods),
        ),
        ("pauses", json!(input.pauses)),
    ] {
        if !header[key].is_null() && header[key] != expected {
            return Err(error("replay_metadata_mismatch"));
        }
    }
    if initial["mods"] != header["mods"] {
        return Err(error("replay_metadata_mismatch"));
    }
    let username: String = row.get("username");
    let md5: String = row.get("md5");
    let date: chrono::DateTime<chrono::Utc> = row.get("created_at");
    let bits = row.get::<i64, _>("mods") as u32;
    let chunks: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT frame_text FROM lazer_replay_chunks WHERE token_id=$1 ORDER BY sequence",
    )
    .bind(token)
    .fetch_all(&mut *tx)
    .await?;
    let frames = chunks.into_iter().flatten().collect::<Vec<u8>>();
    if frames.len() != capture.get::<i32, _>("frame_bytes") as usize {
        return Err(error("replay_chunks_incomplete"));
    }
    let _permit = ENCODERS
        .try_acquire()
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "replay_encoding_busy"))?;
    let bytes = tokio::task::spawn_blocking(move || {
        let score = ReplayScore {
            online_id: score_id,
            user_id: uid,
            username: &username,
            beatmap_md5: &md5,
            client_version: "",
            mods_bits: bits,
            timestamp_ticks: date.timestamp() * 10_000_000
                + i64::from(date.timestamp_subsec_nanos() / 100)
                + 621_355_968_000_000_000,
            submission: &input,
        };
        lazer_replay::encode(&score, &frames)
    })
    .await
    .map_err(|_| error("replay_encoding_failed"))??;
    let sha = digest(&bytes);
    let name = format!("lazer-{score_id}-{}.osr", uuid::Uuid::new_v4());
    let directory = state.storage.join("replays");
    tokio::fs::create_dir_all(&directory).await?;
    let temporary = directory.join(format!("{name}.tmp"));
    let final_path = directory.join(&name);
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, &final_path).await?;
    let result:Result<(),sqlx::Error>=async {sqlx::query("UPDATE scores SET replay_path=$2,replay_sha256=$3,replay_format='lazer',replay_complete=$4,verification='pending' WHERE id=$1").bind(score_id).bind(&name).bind(&sha).bind(!capture.get::<bool,_>("interrupted")).execute(&mut *tx).await?;sqlx::query("UPDATE lazer_replay_captures SET completed_at=now(),updated_at=now() WHERE token_id=$1").bind(token).execute(&mut *tx).await?;sqlx::query("DELETE FROM lazer_replay_chunks WHERE token_id=$1").bind(token).execute(&mut *tx).await?;sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.lazer_replay_capture','score',$2,$3)").bind(uid).bind(score_id.to_string()).bind(json!({"frame_count":capture.get::<i32,_>("frame_count"),"interrupted":capture.get::<bool,_>("interrupted"),"gameplay_verified":false,"sha256":sha})).execute(&mut *tx).await?;tx.commit().await?;Ok(())}.await;
    if let Err(error) = result {
        // A lost COMMIT acknowledgement can still have persisted the reference.
        // Retain complete bytes rather than delete a possibly published replay.
        return Err(error.into());
    }
    Ok(Some(score_id))
}
