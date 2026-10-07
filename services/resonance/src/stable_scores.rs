use crate::common::{self, ApiError, AppState};
use axum::{
    extract::{Multipart, State},
    http::{HeaderMap, StatusCode, header},
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

#[cfg(test)]
#[path = "stable_upload_tests.rs"]
mod upload_tests;

const MAX_SCORE_PARAMETER_BYTES: usize = 32_768;
const MAX_SCORE_UPLOAD_BYTES: usize = 20_971_520;
const MAX_SCORE_DISPOSITION_BYTES: usize = 8_192;
const MAX_SCORE_FIELD_NAME_BYTES: usize = 128;

struct ScoreUpload {
    fields: HashMap<String, String>,
    envelope: Option<String>,
    replay: Option<Vec<u8>>,
}

// Only literal protocol labels are eligible for diagnostics. Form field names,
// filenames, MIME strings and payloads are untrusted and may contain credentials.
fn upload_field_category(name: &str) -> &'static str {
    match name {
        "score" => "score",
        "osuver" => "osuver",
        "iv" => "iv",
        "pass" => "pass",
        "s" => "s",
        "bmk" => "bmk",
        "sbk" => "sbk",
        "x" => "x",
        "ft" => "ft",
        "fs" => "fs",
        "c1" => "c1",
        "st" => "st",
        "i" => "i",
        _ => "unknown",
    }
}

struct UploadPartMetadata {
    category: &'static str,
    parser_category: &'static str,
    resolved_name_length: usize,
    score_name_case_variant: bool,
    name_present: bool,
    name_length: usize,
    filename_present: bool,
    binary_mime: bool,
    content_type_category: &'static str,
    disposition: UploadDispositionMetadata,
    index: usize,
    limit: usize,
}

struct UploadDispositionMetadata {
    kind: &'static str,
    name_ci: bool,
    filename_ci: bool,
    name_lowercase: bool,
    filename_lowercase: bool,
}

fn upload_disposition_metadata(headers: &HeaderMap) -> UploadDispositionMetadata {
    let mut metadata = UploadDispositionMetadata {
        kind: "missing",
        name_ci: false,
        filename_ci: false,
        name_lowercase: false,
        filename_lowercase: false,
    };
    let Some(header) = headers.get(header::CONTENT_DISPOSITION) else {
        return metadata;
    };
    metadata.kind = "other";
    let Ok(value) = header.to_str() else {
        return metadata;
    };
    let kind = value.split(';').next().unwrap_or("").trim();
    metadata.kind = if kind.eq_ignore_ascii_case("form-data") {
        "form-data"
    } else if kind.eq_ignore_ascii_case("attachment") {
        "attachment"
    } else if kind.eq_ignore_ascii_case("file") {
        "file"
    } else {
        "other"
    };
    let mut quoted = false;
    let mut escaped = false;
    let mut parameter_start = None;
    for (index, byte) in value.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if !quoted && byte == b';' {
            if let Some(start) = parameter_start {
                record_upload_parameter(&mut metadata, &value[start..index]);
            }
            parameter_start = Some(index + 1);
        }
    }
    if let Some(start) = parameter_start {
        record_upload_parameter(&mut metadata, &value[start..]);
    }
    metadata
}

fn record_upload_parameter(metadata: &mut UploadDispositionMetadata, parameter: &str) {
    let Some((name, _)) = parameter.trim().split_once('=') else {
        return;
    };
    let name = name.trim();
    metadata.name_ci |= name.eq_ignore_ascii_case("name");
    metadata.filename_ci |= name.eq_ignore_ascii_case("filename");
    metadata.name_lowercase |= name == "name";
    metadata.filename_lowercase |= name == "filename";
}

struct UploadDisposition {
    name: Option<String>,
    filename_present: bool,
}

fn invalid_upload_disposition() -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, "invalid_score_disposition")
}

fn resolve_upload_disposition(headers: &HeaderMap) -> Result<UploadDisposition, ApiError> {
    let mut values = headers.get_all(header::CONTENT_DISPOSITION).iter();
    let Some(header) = values.next() else {
        return Ok(UploadDisposition {
            name: None,
            filename_present: false,
        });
    };
    if values.next().is_some() || header.as_bytes().len() > MAX_SCORE_DISPOSITION_BYTES {
        return Err(invalid_upload_disposition());
    }
    let value = std::str::from_utf8(header.as_bytes()).map_err(|_| invalid_upload_disposition())?;
    let mut segments = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if quoted && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if !quoted && byte == b';' {
            segments.push(&value[start..index]);
            start = index + 1;
        }
    }
    if quoted || escaped {
        return Err(invalid_upload_disposition());
    }
    segments.push(&value[start..]);
    if segments[0].trim().is_empty() {
        return Err(invalid_upload_disposition());
    }
    let mut disposition = UploadDisposition {
        name: None,
        filename_present: false,
    };
    for parameter in segments.into_iter().skip(1) {
        let Some((name, value)) = parameter.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.eq_ignore_ascii_case("name") {
            if disposition.name.is_some() {
                return Err(invalid_upload_disposition());
            }
            disposition.name = Some(decode_upload_parameter(value, MAX_SCORE_FIELD_NAME_BYTES)?);
        } else if name.eq_ignore_ascii_case("filename") {
            if disposition.filename_present {
                return Err(invalid_upload_disposition());
            }
            // Validate the parameter syntax, then discard the supplied filename.
            // Replay paths are always generated by the server after ingestion.
            decode_upload_parameter(value, MAX_SCORE_DISPOSITION_BYTES)?;
            disposition.filename_present = true;
        }
    }
    Ok(disposition)
}

fn decode_upload_parameter(value: &str, limit: usize) -> Result<String, ApiError> {
    let value = value.trim();
    let mut decoded = String::with_capacity(value.len().min(limit));
    if let Some(value) = value.strip_prefix('"') {
        let mut characters = value.chars();
        let mut closed = false;
        while let Some(character) = characters.next() {
            let character = if character == '\\' {
                characters.next().ok_or_else(invalid_upload_disposition)?
            } else if character == '"' {
                if !characters.as_str().trim().is_empty() {
                    return Err(invalid_upload_disposition());
                }
                closed = true;
                break;
            } else {
                character
            };
            if character.is_control() || decoded.len() + character.len_utf8() > limit {
                return Err(invalid_upload_disposition());
            }
            decoded.push(character);
        }
        if !closed {
            return Err(invalid_upload_disposition());
        }
    } else {
        if value.is_empty()
            || value.len() > limit
            || value.chars().any(|character| {
                character.is_control() || character.is_whitespace() || character == '"'
            })
        {
            return Err(invalid_upload_disposition());
        }
        decoded.push_str(value);
    }
    Ok(decoded)
}

fn reject_score_upload(
    metadata: &UploadPartMetadata,
    part_bytes: usize,
    total_bytes: usize,
    stage: &'static str,
    error: ApiError,
) -> ApiError {
    tracing::warn!(
        field_category = metadata.category,
        parser_field_category = metadata.parser_category,
        resolved_name_length = metadata.resolved_name_length,
        score_name_case_variant = metadata.score_name_case_variant,
        name_present = metadata.name_present,
        name_length = metadata.name_length,
        filename_present = metadata.filename_present,
        binary_mime = metadata.binary_mime,
        content_type_category = metadata.content_type_category,
        disposition_type = metadata.disposition.kind,
        disposition_name_parameter_ci = metadata.disposition.name_ci,
        disposition_filename_parameter_ci = metadata.disposition.filename_ci,
        disposition_name_parameter_lowercase = metadata.disposition.name_lowercase,
        disposition_filename_parameter_lowercase = metadata.disposition.filename_lowercase,
        part_index = metadata.index,
        part_bytes,
        total_bytes,
        part_limit = metadata.limit,
        total_limit = MAX_SCORE_UPLOAD_BYTES,
        rejection_stage = stage,
        "stable score multipart rejected"
    );
    error
}

async fn read_score_upload(mut multipart: Multipart) -> Result<ScoreUpload, ApiError> {
    let mut fields = HashMap::new();
    let mut envelope = None;
    let mut replay = None;
    let mut total = 0usize;
    let mut part_index = 0usize;
    while let Some(mut field) = multipart.next_field().await.map_err(|_| {
        reject_score_upload(
            &UploadPartMetadata {
                category: "unavailable",
                parser_category: "unavailable",
                resolved_name_length: 0,
                score_name_case_variant: false,
                name_present: false,
                name_length: 0,
                filename_present: false,
                binary_mime: false,
                content_type_category: "missing",
                disposition: upload_disposition_metadata(&HeaderMap::new()),
                index: part_index.saturating_add(1),
                limit: MAX_SCORE_PARAMETER_BYTES,
            },
            0,
            total,
            "multipart_next_field",
            ApiError(StatusCode::BAD_REQUEST, "invalid_score_multipart"),
        )
    })? {
        part_index = part_index.saturating_add(1);
        let parsed_name = field.name().unwrap_or("");
        let parsed_filename_present = field.file_name().is_some();
        let mut bytes = Vec::new();
        let limit = if parsed_name == "score" && parsed_filename_present {
            resonance_protocol::replay::MAX_REPLAY_BYTES
        } else {
            MAX_SCORE_PARAMETER_BYTES
        };
        let binary_mime = field.content_type().is_some_and(|mime| {
            mime.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/octet-stream")
        });
        let mut metadata = UploadPartMetadata {
            category: upload_field_category(parsed_name),
            parser_category: upload_field_category(parsed_name),
            resolved_name_length: 0,
            score_name_case_variant: parsed_name != "score"
                && parsed_name.eq_ignore_ascii_case("score"),
            name_present: field.name().is_some(),
            name_length: parsed_name.len(),
            filename_present: parsed_filename_present,
            binary_mime,
            content_type_category: if binary_mime {
                "octet-stream"
            } else if field.content_type().is_some() {
                "other"
            } else {
                "missing"
            },
            disposition: upload_disposition_metadata(field.headers()),
            index: part_index,
            limit,
        };
        let disposition = resolve_upload_disposition(field.headers()).map_err(|error| {
            reject_score_upload(&metadata, 0, total, "content_disposition", error)
        })?;
        let name = disposition.name.unwrap_or_default();
        let is_file = disposition.filename_present;
        let limit = if name == "score" && is_file {
            resonance_protocol::replay::MAX_REPLAY_BYTES
        } else {
            MAX_SCORE_PARAMETER_BYTES
        };
        metadata.category = upload_field_category(&name);
        metadata.resolved_name_length = name.len();
        metadata.score_name_case_variant = name != "score" && name.eq_ignore_ascii_case("score");
        metadata.limit = limit;
        while let Some(chunk) = field.chunk().await.map_err(|_| {
            reject_score_upload(
                &metadata,
                bytes.len(),
                total,
                "multipart_chunk",
                ApiError(StatusCode::BAD_REQUEST, "invalid_score_multipart"),
            )
        })? {
            total = total.saturating_add(chunk.len());
            let part_bytes = bytes.len().saturating_add(chunk.len());
            if part_bytes > limit || total > MAX_SCORE_UPLOAD_BYTES {
                return Err(reject_score_upload(
                    &metadata,
                    part_bytes,
                    total,
                    if part_bytes > limit {
                        "part_limit"
                    } else {
                        "total_limit"
                    },
                    ApiError(StatusCode::PAYLOAD_TOO_LARGE, "score_upload_too_large"),
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
    Ok(ScoreUpload {
        fields,
        envelope,
        replay,
    })
}

pub async fn submit(
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let result = submit_inner(state, multipart).await;
    if let Err(error) = &result {
        tracing::warn!(
            error_code = error.1,
            http_status = error.0.as_u16(),
            "stable score submission rejected"
        );
    }
    result
}

async fn submit_inner(state: AppState, multipart: Multipart) -> Result<Response, ApiError> {
    let ScoreUpload {
        fields,
        envelope,
        replay,
    } = read_score_upload(multipart).await?;
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
    )
    .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_envelope_cipher"))?;
    let submission = StableSubmission::parse(&decoded)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_envelope"))?;
    let client_hash = decrypt_score_field(required("s")?, iv, version)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_client_hash_cipher"))?;
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
    submission
        .validate_checksum(version, &client_hash, storyboard)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "score_checksum_mismatch"))?;
    if submission.mods & (1 << 23) != 0 {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "target_practice_submission_unavailable",
        ));
    }
    let Some(map_id) =
        crate::native_maps::resolve_checksum(&state, player.id, &submission.beatmap_md5).await?
    else {
        return Ok("error: beatmap".into_response());
    };
    let map = sqlx::query("SELECT id,mode,native_id,native_set_id FROM beatmaps WHERE id=$1")
        .bind(map_id)
        .fetch_one(&state.pool)
        .await?;
    let (public_map_id, set_id) = crate::native_maps::row_ids(&map)?;
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
            })?
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_replay"))?
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
    // Pending scores do not update ranked totals or placement. This chart still
    // gives stable the persistent online score ID for its local score/replay.
    let body = format!(
        "beatmapId:{public_map_id}|beatmapSetId:{set_id}|beatmapPlaycount:0|beatmapPasscount:0|approvedDate:{}|\n|chartId:beatmap|chartUrl:/beatmaps/{map_id}|chartName:Awaiting review|rankBefore:0|rankAfter:0|rankedScoreBefore:0|rankedScoreAfter:0|totalScoreBefore:0|totalScoreAfter:0|maxComboBefore:0|maxComboAfter:0|accuracyBefore:0|accuracyAfter:0|ppBefore:0|ppAfter:0|onlineScoreId:{score_id}|\n|chartId:overall|chartUrl:/players/{}|chartName:Awaiting review|rankBefore:0|rankAfter:0|rankedScoreBefore:0|rankedScoreAfter:0|totalScoreBefore:0|totalScoreAfter:0|maxComboBefore:0|maxComboAfter:0|accuracyBefore:0|accuracyAfter:0|ppBefore:0|ppAfter:0|achievements-new:",
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        player.id
    );
    Ok(body.into_response())
}
