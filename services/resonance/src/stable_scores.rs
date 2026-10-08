use crate::common::{self, ApiError, AppState};
use axum::{
    extract::{Multipart, State, multipart::MultipartError},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use resonance_protocol::{
    ProtocolError, stable as wire,
    submission::{StableSubmission, decrypt_score_field, validate_compressed_frames},
};
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
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
    // A failed or unavailable resolver must not look like valid absent parameters.
    resolved_name_present: Option<bool>,
    resolved_filename_present: Option<bool>,
    parser_name_matches_resolved: Option<bool>,
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
        resolved_name_present = metadata.resolved_name_present,
        resolved_filename_present = metadata.resolved_filename_present,
        parser_name_matches_resolved = metadata.parser_name_matches_resolved,
        score_name_case_variant = metadata.score_name_case_variant,
        parser_name_present = metadata.name_present,
        parser_filename_present = metadata.filename_present,
        // Retain the existing parser-presence aliases for deployed log readers.
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

fn score_multipart_error(error: MultipartError) -> ApiError {
    // Preserve Axum's trusted classification without serializing parser errors,
    // which may contain untrusted field names, headers, or payload details.
    let status = error.status();
    ApiError(
        status,
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            "score_upload_too_large"
        } else {
            "invalid_score_multipart"
        },
    )
}

async fn read_score_upload(mut multipart: Multipart) -> Result<ScoreUpload, ApiError> {
    let mut fields = HashMap::new();
    let mut envelope = None;
    let mut replay = None;
    let mut total = 0usize;
    let mut part_index = 0usize;
    while let Some(mut field) = multipart.next_field().await.map_err(|error| {
        reject_score_upload(
            &UploadPartMetadata {
                category: "unavailable",
                parser_category: "unavailable",
                resolved_name_length: 0,
                resolved_name_present: None,
                resolved_filename_present: None,
                parser_name_matches_resolved: None,
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
            score_multipart_error(error),
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
            resolved_name_present: None,
            resolved_filename_present: None,
            parser_name_matches_resolved: None,
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
        metadata.resolved_name_present = Some(disposition.name.is_some());
        metadata.resolved_filename_present = Some(disposition.filename_present);
        metadata.parser_name_matches_resolved = Some(field.name() == disposition.name.as_deref());
        let name = disposition.name.unwrap_or_default();
        let is_file = disposition.filename_present;
        // Extensions are not score inputs. Drain only ordinary, named form parts
        // we do not use, without retaining their contents or guessing a replay.
        // Case variants of known names stay on the strict parameter path.
        let discard_extension = !name.is_empty()
            && !is_file
            && metadata.disposition.kind == "form-data"
            && upload_field_category(&name.to_ascii_lowercase()) == "unknown";
        let limit = if discard_extension {
            MAX_SCORE_UPLOAD_BYTES
        } else if name == "score" && is_file {
            resonance_protocol::replay::MAX_REPLAY_BYTES
        } else {
            MAX_SCORE_PARAMETER_BYTES
        };
        metadata.category = upload_field_category(&name);
        metadata.resolved_name_length = name.len();
        metadata.score_name_case_variant = name != "score" && name.eq_ignore_ascii_case("score");
        metadata.limit = limit;
        let mut part_bytes = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|error| {
            reject_score_upload(
                &metadata,
                part_bytes,
                total,
                "multipart_chunk",
                score_multipart_error(error),
            )
        })? {
            total = total.saturating_add(chunk.len());
            part_bytes = part_bytes.saturating_add(chunk.len());
            if (!discard_extension && part_bytes > limit) || total > MAX_SCORE_UPLOAD_BYTES {
                return Err(reject_score_upload(
                    &metadata,
                    part_bytes,
                    total,
                    if !discard_extension && part_bytes > limit {
                        "part_limit"
                    } else {
                        "total_limit"
                    },
                    ApiError(StatusCode::PAYLOAD_TOO_LARGE, "score_upload_too_large"),
                ));
            }
            if !discard_extension {
                bytes.extend(chunk);
            }
        }
        if discard_extension {
            if part_bytes > MAX_SCORE_PARAMETER_BYTES {
                tracing::info!(
                    field_category = "unknown",
                    part_index,
                    part_bytes,
                    total_bytes = total,
                    envelope_present = envelope.is_some(),
                    replay_present = replay.is_some(),
                    "stable score unused form part discarded"
                );
            }
            continue;
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

fn duplicate_payload() -> ApiError {
    ApiError(StatusCode::CONFLICT, "duplicate_score_payload_mismatch")
}

async fn saved_score(state: &AppState, ingestion_key: &str) -> Result<Option<PgRow>, ApiError> {
    Ok(sqlx::query("SELECT s.*,b.md5 AS beatmap_md5 FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.ingestion_key=$1")
        .bind(ingestion_key).fetch_optional(&state.pool).await?)
}

fn judgement_accuracy(submission: &StableSubmission) -> Result<f64, ApiError> {
    let [a, b, c, g, k, m] = submission.hit_counts.map(f64::from);
    let (numerator, denominator) = match submission.mode {
        0 => (6.0 * a + 2.0 * b + c, 6.0 * (a + b + c + m)),
        1 => (2.0 * a + b, 2.0 * (a + b + m)),
        2 => (a + b + c, a + b + c + k + m),
        3 => (
            6.0 * (g + a) + 4.0 * k + 2.0 * b + c,
            6.0 * (g + a + k + b + c + m),
        ),
        _ => return Err(duplicate_payload()),
    };
    if denominator == 0.0 {
        return Err(duplicate_payload());
    }
    Ok(100.0 * numerator / denominator)
}

fn require_saved_payload(
    row: &PgRow,
    submission: &StableSubmission,
    user_id: i64,
    map_id: Option<i64>,
    ingestion_key: &str,
    variant: illya_domain::ScoreVariant,
) -> Result<(), ApiError> {
    // Online checksum only binds n300+n100, and concatenates other counts.
    // Compare every stored judgement independently before treating it as a
    // retry. Old JSON without the optional tick counters normalizes to zero.
    let h: illya_domain::HitCounts =
        serde_json::from_value(row.get("hit_counts")).map_err(|_| duplicate_payload())?;
    let accuracy = row.get::<f64, _>("accuracy");
    let expected_accuracy = judgement_accuracy(submission)?;
    if row.get::<i64, _>("user_id") != user_id
        || map_id.is_some_and(|map| row.get::<i64, _>("beatmap_id") != map)
        || !row.get::<String, _>("beatmap_md5").eq_ignore_ascii_case(&submission.beatmap_md5)
        || row.get::<Option<String>, _>("ingestion_key").as_deref() != Some(ingestion_key)
        || row.get::<String, _>("client") != "stable"
        || row.get::<i16, _>("mode") != i16::from(submission.mode)
        || row.get::<String, _>("score_variant") != variant.as_str()
        || row.get::<i64, _>("score") != i64::from(submission.total_score)
        || row.get::<i32, _>("max_combo") != i32::from(submission.max_combo)
        || row.get::<i64, _>("mods") != i64::from(submission.mods)
        || row.get::<bool, _>("passed") != submission.passed
        || [h.n300, h.n100, h.n50, h.geki, h.katu, h.misses] != submission.hit_counts.map(u32::from)
        || h.large_tick_hits != 0 || h.small_tick_hits != 0 || h.slider_end_hits != 0
        || !accuracy.is_finite()
        // Float8/JSON round trips can differ at the last few binary digits.
        // This is far below one possible u16 judgement's accuracy contribution.
        || (accuracy - expected_accuracy).abs() > 1e-7
    {
        return Err(duplicate_payload());
    }
    // For missing-replay legacy rows, the validated checksum/key is the only
    // persisted time/version/perfect/rank identity. Existing replay metadata
    // supplies an independent identity check below; review/PP are never inputs.
    Ok(())
}

fn require_replay_identity(
    existing: &resonance_protocol::replay::LegacyReplay<'_>,
    expected: &resonance_protocol::replay::LegacyReplay<'_>,
) -> Result<(), ApiError> {
    if existing.mode != expected.mode
        || existing.client_version != expected.client_version
        || !existing
            .beatmap_md5
            .eq_ignore_ascii_case(expected.beatmap_md5)
        || existing.player_name != expected.player_name
        || (!existing.replay_md5.is_empty()
            && !existing
                .replay_md5
                .eq_ignore_ascii_case(expected.replay_md5))
        || existing.hit_counts != expected.hit_counts
        || existing.total_score != expected.total_score
        || existing.max_combo != expected.max_combo
        || existing.perfect != expected.perfect
        || existing.mods != expected.mods
        || existing.timestamp_ticks != expected.timestamp_ticks
        || existing
            .online_score_id
            .is_some_and(|id| Some(id) != expected.online_score_id)
    {
        return Err(duplicate_payload());
    }
    // Older containers may omit checksum/online ID or include a life-bar string;
    // neither absence changes the independently stored immutable score context.
    Ok(())
}

fn parse_score_submission(decoded: &str) -> Result<StableSubmission, ApiError> {
    StableSubmission::parse(decoded).map_err(|error| {
        // Report only fixed categories and lengths. The decrypted envelope may
        // contain account names and arbitrary client-controlled text.
        let parse_reason = match error {
            ProtocolError::Limit("score envelope") => "envelope_limit",
            ProtocolError::Invalid("score field count") => "field_count",
            ProtocolError::Invalid("score digest") => "digest",
            ProtocolError::Invalid("score username") => "username",
            ProtocolError::Invalid("score hit counts") => "hit_counts",
            ProtocolError::Invalid("score total") => "total",
            ProtocolError::Invalid("score combo") => "combo",
            ProtocolError::Invalid("score boolean") => "boolean",
            ProtocolError::Invalid("score grade") => "grade",
            ProtocolError::Invalid("score mods") => "mods",
            ProtocolError::Invalid("score mode") => "mode",
            ProtocolError::Invalid("score timestamp") => "timestamp",
            ProtocolError::Invalid("score client version") => "client_version",
            _ => "other",
        };
        let envelope_field_count = decoded.split(':').count();
        let client_version = if envelope_field_count >= 18 {
            decoded.split(':').nth(17)
        } else {
            None
        };
        tracing::warn!(
            parse_reason,
            envelope_field_count,
            extension_field_count = envelope_field_count.saturating_sub(18),
            envelope_bytes = decoded.len(),
            client_version_raw_bytes = client_version.map(str::len),
            client_version_normalized_bytes = client_version.map(|value| value.trim().len()),
            client_flag_spaces =
                client_version.map(|value| value.len() - value.trim_end_matches(' ').len()),
            "stable score envelope rejected"
        );
        ApiError(StatusCode::BAD_REQUEST, "invalid_score_envelope")
    })
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
    let submission = parse_score_submission(&decoded)?;
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
    let variant = illya_domain::ScoreVariant::from_legacy(submission.mode, submission.mods)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_variant"))?;
    let ingestion_key = format!("stable:{}:{}", player.id, submission.checksum);
    // A persisted score owns its immutable identity even when the set is later
    // withdrawn. Only exact existing receipts bypass active discovery.
    let saved = saved_score(&state, &ingestion_key).await?;
    let resolved = if let Some(saved) = saved {
        require_saved_payload(
            &saved,
            &submission,
            player.id,
            None,
            &ingestion_key,
            variant,
        )?;
        Some(saved.get::<i64, _>("beatmap_id"))
    } else {
        match crate::native_maps::resolve_checksum(&state, player.id, &submission.beatmap_md5).await
        {
            Ok(resolved) => resolved,
            Err(error) if error.0 == StatusCode::GONE => {
                // A publisher can win between the initial lookup and an active
                // discovery fence. The winner must satisfy the same payload
                // guard before it can establish the retained retry exception.
                if let Some(saved) = saved_score(&state, &ingestion_key).await? {
                    require_saved_payload(
                        &saved,
                        &submission,
                        player.id,
                        None,
                        &ingestion_key,
                        variant,
                    )?;
                    Some(saved.get::<i64, _>("beatmap_id"))
                } else {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    };
    let Some(map_id) = resolved else {
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
        if response.status() == StatusCode::GONE {
            return Err(ApiError(StatusCode::GONE, "beatmap_withdrawn"));
        }
        if response.status() == StatusCode::CONFLICT {
            return Err(duplicate_payload());
        }
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
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query("SELECT s.*,b.md5 AS beatmap_md5 FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.id=$1 FOR UPDATE OF s")
        .bind(score_id).fetch_optional(&mut *tx).await?
        .ok_or(ApiError(StatusCode::BAD_GATEWAY, "invalid_ingestion_response"))?;
    // This second guard covers a winner committed during the ingestion HTTP
    // request/shared-set wait and protects an orphaned score before attachment.
    require_saved_payload(
        &row,
        &submission,
        player.id,
        Some(map_id),
        &ingestion_key,
        variant,
    )?;
    if submission.passed {
        let container = submission.replay_container(
            version
                .parse()
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_version"))?,
            &replay,
            score_id,
        )?;
        let expected = resonance_protocol::replay::LegacyReplay::parse(&container)?;
        if let Some(path) = row.get::<Option<String>, _>("replay_path") {
            let existing = common::stored_file(&state, &path, "replays", 16_777_216).await?;
            let existing = resonance_protocol::replay::LegacyReplay::parse(&existing)?;
            if existing.compressed_frames != replay {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "duplicate_score_replay_mismatch",
                ));
            }
            require_replay_identity(&existing, &expected)?;
        } else {
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
    }
    tx.commit().await?;
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
