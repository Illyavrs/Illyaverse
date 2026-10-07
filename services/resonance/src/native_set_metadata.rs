//! Complete official set metadata for native browsing. Difficulty content is
//! still resolved separately; metadata never implies an archive is available.
use crate::{
    beatmaps,
    common::{ApiError, AppState},
    native_maps,
};
use axum::http::StatusCode;
use illya_domain::official_set::{
    MAX_METADATA_RESPONSE_BYTES, OfficialSetDifficulty, OfficialSetMetadata,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::time::Duration;

async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ApiError> {
    let invalid = || ApiError(StatusCode::BAD_GATEWAY, "invalid_set_metadata_response");
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            ApiError(StatusCode::SERVICE_UNAVAILABLE, "set_metadata_unavailable")
        } else {
            invalid()
        }
    })? {
        if chunk.len() > limit - bytes.len() {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn discover(
    state: &AppState,
    user_id: i64,
    id: i64,
) -> Result<OfficialSetMetadata, ApiError> {
    if state.service_key.len() < 32 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "set_metadata_not_configured",
        ));
    }
    let response = state
        .http
        .post(format!(
            "{}/api/v1/internal/beatmapsets/metadata",
            state.sanctuary_url.trim_end_matches('/')
        ))
        .timeout(Duration::from_secs(8))
        .header("x-illya-service-key", state.service_key.as_str())
        .json(&json!({"user_id":user_id,"official_set_id":id}))
        .send()
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "set_metadata_unavailable"))?;
    let status = response.status();
    if status != StatusCode::OK {
        if status == StatusCode::NOT_FOUND {
            let body = bounded_body(response, 64 * 1024).await?;
            let body: Value = serde_json::from_slice(&body)
                .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_set_metadata_response"))?;
            if body["error"]["code"] == "beatmapset_not_found" {
                return Err(ApiError(StatusCode::NOT_FOUND, "beatmapset_not_found"));
            }
            return Err(ApiError(StatusCode::BAD_GATEWAY, "set_metadata_failed"));
        }
        return Err(match status {
            StatusCode::UNAUTHORIZED => ApiError(StatusCode::UNAUTHORIZED, "invalid_account"),
            StatusCode::SERVICE_UNAVAILABLE => ApiError(status, "set_metadata_unavailable"),
            StatusCode::TOO_MANY_REQUESTS => ApiError(status, "set_metadata_rate_limited"),
            _ => ApiError(StatusCode::BAD_GATEWAY, "set_metadata_failed"),
        });
    }
    let body = bounded_body(response, MAX_METADATA_RESPONSE_BYTES).await?;
    let metadata: OfficialSetMetadata = serde_json::from_slice(&body)
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_set_metadata_response"))?;
    metadata
        .validate(id)
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_set_metadata_response"))?;
    Ok(metadata)
}

fn difficulty_json(map: &OfficialSetDifficulty) -> Value {
    json!({
        "id":map.id,"beatmapset_id":map.beatmapset_id,"checksum":map.checksum,
        "version":map.version,"mode_int":map.mode_int,"mode":(["osu","taiko","fruits","mania"][map.mode_int as usize]),
        "status":native_status(&map.status),"ranked":ranked(&map.status),"difficulty_rating":map.difficulty_rating,
        "bpm":map.bpm,"total_length":map.total_length,"hit_length":map.hit_length,"max_combo":map.max_combo,
        "ar":map.ar,"cs":map.cs,"accuracy":map.od,"drain":map.hp,
        "count_circles":map.count_circles,"count_sliders":map.count_sliders,"count_spinners":map.count_spinners,
        "last_updated":map.last_updated,"convert":false,"user_id":0,"owners":[],"is_scoreable":true
    })
}
fn ranked(status: &str) -> i32 {
    if status == "ranked" {
        1
    } else if status == "loved" {
        4
    } else {
        0
    }
}
fn native_status(status: &str) -> &str {
    if status == "unranked" {
        "pending"
    } else {
        status
    }
}

pub(crate) async fn set_json(state: &AppState, user_id: i64, id: i64) -> Result<Value, ApiError> {
    if !native_maps::valid_id(id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_beatmapset_id"));
    }
    if id >= native_maps::LOCAL_ID_MIN {
        return beatmaps::local_set_json(state, id).await;
    }
    let archived: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM official_beatmapset_archives WHERE official_set_id=$1)",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if archived {
        return beatmaps::local_set_json(state, id).await;
    }
    let metadata = match discover(state, user_id, id).await {
        Ok(metadata) => metadata,
        // Stored local content remains authoritative during an upstream outage
        // or missing metadata. This fallback only advertises already saved maps.
        Err(error) if error.0 != StatusCode::UNAUTHORIZED => {
            let stored: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM beatmaps WHERE native_set_id=$1)")
                    .bind(id)
                    .fetch_one(&state.pool)
                    .await?;
            if stored {
                return beatmaps::local_set_json(state, id).await;
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    let published_during_fetch: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM official_beatmapset_archives WHERE official_set_id=$1)",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if published_during_fetch {
        return beatmaps::local_set_json(state, id).await;
    }
    // Merge fresh moderation every time. Never cache this player-visible result.
    let ids: Vec<i64> = metadata.beatmaps.iter().map(|map| map.id).collect();
    let hashes: Vec<&str> = metadata
        .beatmaps
        .iter()
        .map(|map| map.checksum.as_str())
        .collect();
    let rows = sqlx::query("SELECT id,official_id,official_set_id,native_id,native_set_id,source,md5,mode FROM beatmaps WHERE native_set_id=$1 OR official_id=ANY($2) OR md5=ANY($3) ORDER BY id LIMIT 513")
        .bind(id).bind(&ids).bind(&hashes).fetch_all(&state.pool).await?;
    if rows.len() > 512 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "beatmap_set_version_conflict",
        ));
    }
    #[cfg(test)]
    crate::native_set_metadata_tests::after_stored_snapshot(state).await;
    let mut maps: Vec<Value> = metadata.beatmaps.iter().map(difficulty_json).collect();
    let mut stored_set = None;
    for row in rows {
        let official_id: Option<i64> = row.get("official_id");
        let Some(index) = metadata
            .beatmaps
            .iter()
            .position(|map| Some(map.id) == official_id)
        else {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "beatmap_set_version_conflict",
            ));
        };
        let original = &metadata.beatmaps[index];
        if row.get::<String, _>("source") != "official"
            || row.get::<Option<i64>, _>("official_set_id") != Some(id)
            || row.get::<i64, _>("native_id") != original.id
            || row.get::<i64, _>("native_set_id") != id
            || row.get::<String, _>("md5") != original.checksum
            || row.get::<i16, _>("mode") != original.mode_int
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "beatmap_set_version_conflict",
            ));
        }
        let mut local = native_maps::map_json(state, row.get("id")).await?;
        if local["id"] != original.id
            || local["beatmapset_id"] != id
            || local["checksum"] != original.checksum
            || local["mode_int"] != original.mode_int
        {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "beatmap_set_version_conflict",
            ));
        }
        if stored_set.is_none() {
            stored_set = local.get("beatmapset").cloned();
        }
        local
            .as_object_mut()
            .ok_or(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_beatmap",
            ))?
            .remove("beatmapset");
        // uploaded_by records the ingestor, not a verified mapper association.
        local.as_object_mut().unwrap().remove("user_id");
        if let Some(status) = local["status"]
            .as_str()
            .map(|value| native_status(value).to_owned())
        {
            local["status"] = json!(status);
        }
        maps[index]
            .as_object_mut()
            .unwrap()
            .extend(local.as_object().unwrap().clone());
    }
    // An archive published during discovery can belong to a newer snapshot.
    // Switch the whole response to its authoritative stored manifest rather
    // than advertise that archive alongside this earlier metadata snapshot.
    let now_archived: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM official_beatmapset_archives WHERE official_set_id=$1)",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if now_archived {
        return beatmaps::local_set_json(state, id).await;
    }
    // This metadata-only response does not establish archive membership. An
    // archive committed after the check can be advertised on the next request.
    let disabled = true;
    let mut set = json!({
        "id":id,"artist":metadata.artist,"artist_unicode":metadata.artist_unicode,
        "title":metadata.title,"title_unicode":metadata.title_unicode,"creator":metadata.creator,
        "status":native_status(&metadata.status),"user_id":0,"bpm":metadata.bpm,"source":metadata.source,"tags":metadata.tags,"nsfw":metadata.nsfw,
        "submitted_date":metadata.submitted_date,"last_updated":metadata.last_updated,"ranked_date":metadata.ranked_date,
        "genre":metadata.genre,"language":metadata.language,
        "covers":{"cover":"","cover@2x":"","card":"","card@2x":"","list":"","list@2x":"","slimcover":"","slimcover@2x":""},
        "preview_url":"","video":false,"storyboard":false,"download_disabled":disabled,
        "availability":{"download_disabled":disabled,"more_information":null},"beatmaps":maps
    });
    if let Some(stored) = stored_set {
        // Retain local display text and moderation. No mapper account mapping
        // is inferred from uploaded_by or from an upstream account identifier.
        for field in [
            "artist",
            "artist_unicode",
            "title",
            "title_unicode",
            "creator",
            "status",
        ] {
            if let Some(value) = stored.get(field) {
                set[field] = value.clone();
            }
        }
        if let Some(status) = set["status"]
            .as_str()
            .map(|value| native_status(value).to_owned())
        {
            set["status"] = json!(status);
        }
    }
    Ok(set)
}
