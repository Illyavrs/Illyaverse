use crate::common::{self, ApiError, AppState};
use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use resonance_protocol::{
    stable as wire,
    submission::{StableSubmission, decrypt_score_field, validate_compressed_frames},
};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;

pub async fn submit(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let mut fields = HashMap::new();
    let mut envelope = None;
    let mut replay = None;
    let mut total = 0usize;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_multipart"))?
    {
        let name = field.name().unwrap_or("").to_owned();
        let is_file = field.file_name().is_some();
        let mut bytes = Vec::new();
        let limit = if name == "score" && is_file {
            resonance_protocol::replay::MAX_REPLAY_BYTES
        } else {
            32_768
        };
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_multipart"))?
        {
            total = total.saturating_add(chunk.len());
            if bytes.len() + chunk.len() > limit || total > 20_971_520 {
                return Err(ApiError(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "score_upload_too_large",
                ));
            }
            bytes.extend(chunk);
        }
        if name == "score" && is_file {
            if replay.replace(bytes).is_some() {
                return Err(ApiError(StatusCode::BAD_REQUEST, "duplicate_replay_field"));
            }
        } else if name == "score" {
            if envelope
                .replace(
                    String::from_utf8(bytes)
                        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_text"))?,
                )
                .is_some()
            {
                return Err(ApiError(
                    StatusCode::BAD_REQUEST,
                    "duplicate_score_envelope",
                ));
            }
        } else if !is_file
            && fields
                .insert(
                    name,
                    String::from_utf8(bytes)
                        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_text"))?,
                )
                .is_some()
        {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "duplicate_score_parameter",
            ));
        }
    }
    let required = |name: &str| {
        fields
            .get(name)
            .map(String::as_str)
            .ok_or(ApiError(StatusCode::BAD_REQUEST, "missing_score_parameter"))
    };
    let version = required("osuver")?;
    let iv = required("iv")?;
    let password = required("pass")?;
    if !wire::is_md5(password) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    let decoded = decrypt_score_field(
        envelope
            .as_deref()
            .ok_or(ApiError(StatusCode::BAD_REQUEST, "missing_score_envelope"))?,
        iv,
        version,
    )?;
    let submission = StableSubmission::parse(&decoded)?;
    let client_hash = decrypt_score_field(required("s")?, iv, version)?;
    let player = common::authenticate(
        &state,
        &submission.username,
        &password.to_ascii_lowercase(),
        true,
    )
    .await?;
    common::rate_limit(&state, format!("stable-score:{}", player.id), 60).await?;
    if submission.username != player.username {
        return Err(ApiError(StatusCode::BAD_REQUEST, "score_username_mismatch"));
    }
    let Some(session)=sqlx::query("SELECT client_version,client_hash_hash FROM bancho_sessions WHERE user_id=$1 AND expires_at>now()").bind(player.id).fetch_optional(&state.pool).await?else{return Ok("".into_response());};
    let login_version: String = session.get("client_version");
    if !login_version
        .strip_prefix('b')
        .unwrap_or(&login_version)
        .starts_with(version)
        || !submission
            .client_version
            .strip_prefix('b')
            .unwrap_or(&submission.client_version)
            .starts_with(version)
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "score_version_mismatch"));
    }
    if common::token_hash(&client_hash) != session.get::<String, _>("client_hash_hash") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "score_client_hash_mismatch",
        ));
    }
    if !submission
        .beatmap_md5
        .eq_ignore_ascii_case(required("bmk")?)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "score_beatmap_hash_mismatch",
        ));
    }
    let storyboard = fields.get("sbk").map(String::as_str).unwrap_or("");
    if !storyboard.is_empty() && !wire::is_md5(storyboard) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_storyboard_hash"));
    }
    submission.validate_checksum(version, &client_hash, storyboard)?;
    if submission.mods & (1 << 23) != 0 {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "target_practice_submission_unavailable",
        ));
    }
    let map = sqlx::query("SELECT id,mode,official_set_id FROM beatmaps WHERE md5=$1")
        .bind(&submission.beatmap_md5)
        .fetch_optional(&state.pool)
        .await?;
    let Some(map) = map else {
        return Ok("error: beatmap".into_response());
    };
    let map_id: i64 = map.get("id");
    if state.service_key.len() < 32 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "internal_ingestion_not_configured",
        ));
    }
    let replay = replay.unwrap_or_default();
    let frame_count = if submission.passed {
        let compressed = replay.clone();
        tokio::task::spawn_blocking(move || validate_compressed_frames(&compressed))
            .await
            .map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "replay_validation_failed",
                )
            })??
    } else {
        0
    };
    let [n300, n100, n50, geki, katu, misses] = submission.hit_counts;
    let response=state.http.post(format!("{}/api/v1/internal/scores",state.sanctuary_url.trim_end_matches('/'))).header("x-illya-service-key",state.service_key.as_str()).json(&json!({"user_id":player.id,"beatmap_id":map_id,"mode":submission.mode,"score":submission.total_score,"accuracy":0,"max_combo":submission.max_combo,"mods":submission.mods,"passed":submission.passed,"hit_counts":{"n300":n300,"n100":n100,"n50":n50,"geki":geki,"katu":katu,"misses":misses},"client":"stable","ingestion_key":format!("stable:{}:{}",player.id,submission.checksum)})).send().await.map_err(|_|ApiError(StatusCode::SERVICE_UNAVAILABLE,"score_ingestion_unavailable"))?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "score_rejected_by_ingestion",
        ));
    }
    let result: Value = response
        .json()
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_ingestion_response"))?;
    let score_id = result.get("id").and_then(Value::as_i64).ok_or(ApiError(
        StatusCode::BAD_GATEWAY,
        "invalid_ingestion_response",
    ))?;
    if submission.passed {
        let mut tx = state.pool.begin().await?;
        let row =
            sqlx::query("SELECT replay_path FROM scores WHERE id=$1 AND user_id=$2 FOR UPDATE")
                .bind(score_id)
                .bind(player.id)
                .fetch_one(&mut *tx)
                .await?;
        if let Some(path) = row.get::<Option<String>, _>("replay_path") {
            let existing = common::stored_file(&state, &path, "replays", 16_777_216).await?;
            if resonance_protocol::replay::LegacyReplay::parse(&existing)?.compressed_frames
                != replay
            {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "duplicate_score_replay_mismatch",
                ));
            }
        } else {
            let container = submission.replay_container(
                version
                    .parse()
                    .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_version"))?,
                &replay,
                score_id,
            )?;
            // Verify our exported container before publishing the stored path.
            resonance_protocol::replay::LegacyReplay::parse(&container)?;
            let filename = format!("stable-{score_id}-{}.osr", uuid::Uuid::new_v4());
            let path = state.storage.join("replays").join(&filename);
            tokio::fs::write(&path, &container).await?;
            if let Err(error) =
                sqlx::query("UPDATE scores SET replay_path=$2,verification='pending' WHERE id=$1")
                    .bind(score_id)
                    .bind(&filename)
                    .execute(&mut *tx)
                    .await
            {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(error.into());
            }
            sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.stable_ingest','score',$2,$3)").bind(player.id).bind(score_id.to_string()).bind(json!({"checksum_matches":true,"client_matches":true,"frames":frame_count,"gameplay_verified":false,"replay_bytes":replay.len(),"rank":submission.rank,"ended_at":submission.played_at.and_utc()})).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    let notification = wire::string_packet(
        wire::server::NOTIFICATION,
        &format!(
            "Score #{score_id} saved ({:.2}pp preview). It will enter rankings after moderator review.",
            result.get("pp").and_then(Value::as_f64).unwrap_or(0.0)
        ),
    )?;
    sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) VALUES($1,$2)")
        .bind(player.id)
        .bind(notification)
        .execute(&state.pool)
        .await?;
    if !submission.passed {
        return Ok("error: no".into_response());
    }
    let set_id = map
        .get::<Option<i64>, _>("official_set_id")
        .unwrap_or(map_id);
    // Pending scores do not update ranked totals or placement. This chart still
    // gives stable the persistent online score ID for its local score/replay.
    let body = format!(
        "beatmapId:{map_id}|beatmapSetId:{set_id}|beatmapPlaycount:0|beatmapPasscount:0|approvedDate:{}|\n|chartId:beatmap|chartUrl:/beatmaps/{map_id}|chartName:Awaiting review|rankBefore:0|rankAfter:0|rankedScoreBefore:0|rankedScoreAfter:0|totalScoreBefore:0|totalScoreAfter:0|maxComboBefore:0|maxComboAfter:0|accuracyBefore:0|accuracyAfter:0|ppBefore:0|ppAfter:0|onlineScoreId:{score_id}|\n|chartId:overall|chartUrl:/players/{}|chartName:Awaiting review|rankBefore:0|rankAfter:0|rankedScoreBefore:0|rankedScoreAfter:0|totalScoreBefore:0|totalScoreAfter:0|maxComboBefore:0|maxComboAfter:0|accuracyBefore:0|accuracyAfter:0|ppBefore:0|ppAfter:0|achievements-new:",
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        player.id
    );
    Ok(body.into_response())
}
