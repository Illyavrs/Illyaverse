//! Native lazer map lookup and archive routes. The map hash selects immutable
//! stored content; external osu! IDs are translated to internal primary keys.
use crate::{
    common::{self, ApiError, AppState},
    lazer,
};
use axum::{
    Json, Router,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::HashMap;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v2/beatmaps/lookup", get(lookup))
        .route("/api/v2/beatmapsets/lookup", get(set_lookup))
        .route("/api/v2/beatmapsets/search", get(search))
        .route("/api/v2/beatmapsets/{id}", get(beatmapset))
        .route("/api/v2/beatmapsets/{id}/download", get(download))
        .route("/api/v2/beatmaps/{id}/scores", get(scores))
}

pub async fn resolve(state: &AppState, id: i64) -> Result<i64, ApiError> {
    sqlx::query_scalar("SELECT id FROM beatmaps WHERE official_id=$1 OR id=$1 ORDER BY (official_id=$1) DESC LIMIT 1").bind(id).fetch_optional(&state.pool).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"beatmap_not_found"))
}
async fn lookup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    let id = if let Some(hash) = query.get("checksum") {
        if !resonance_protocol::stable::is_md5(hash) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_beatmap_hash"));
        }
        sqlx::query_scalar("SELECT id FROM beatmaps WHERE md5=$1")
            .bind(hash.to_ascii_lowercase())
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?
    } else {
        resolve(
            &state,
            query
                .get("id")
                .and_then(|v| v.parse().ok())
                .ok_or(ApiError(
                    StatusCode::BAD_REQUEST,
                    "beatmap_id_or_checksum_required",
                ))?,
        )
        .await?
    };
    Ok(Json(lazer::map_json(&state, id).await?))
}
async fn set_json(state: &AppState, id: i64) -> Result<Value, ApiError> {
    let rows=sqlx::query("SELECT id FROM beatmaps WHERE official_set_id=$1 OR (official_set_id IS NULL AND id=$1) ORDER BY id LIMIT 256").bind(id).fetch_all(&state.pool).await?;
    if rows.is_empty() {
        return Err(ApiError(StatusCode::NOT_FOUND, "beatmapset_not_found"));
    }
    let mut maps = Vec::new();
    let mut set = None;
    for row in rows {
        let mut map = lazer::map_json(state, row.get("id")).await?;
        if set.is_none() {
            set = map.get("beatmapset").cloned();
        }
        map.as_object_mut()
            .ok_or(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_beatmap",
            ))?
            .remove("beatmapset");
        maps.push(map);
    }
    let mut set = set.ok_or(ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "invalid_beatmapset",
    ))?;
    set["beatmaps"] = json!(maps);
    Ok(set)
}
async fn beatmapset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    Ok(Json(set_json(&state, id).await?))
}
async fn set_lookup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    let native = query
        .get("beatmap_id")
        .and_then(|v| v.parse().ok())
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_beatmap_id"))?;
    let internal = resolve(&state, native).await?;
    let set: i64 =
        sqlx::query_scalar("SELECT COALESCE(official_set_id,id) FROM beatmaps WHERE id=$1")
            .bind(internal)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(set_json(&state, set).await?))
}
async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    common::bearer(&state, &headers).await?;
    let path:String=sqlx::query_scalar("SELECT archive_path FROM beatmaps WHERE (official_set_id=$1 OR (official_set_id IS NULL AND id=$1)) AND archive_path IS NOT NULL ORDER BY id LIMIT 1").bind(id).fetch_optional(&state.pool).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"archive_not_available"))?;
    let bytes = common::stored_file(&state, &path, "maps", 104_857_600).await?;
    Ok((
        [
            ("content-type", "application/x-osu-beatmap-archive"),
            ("content-disposition", "attachment; filename=beatmap.osz"),
        ],
        bytes,
    )
        .into_response())
}
async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    let q = query.get("q").cloned().unwrap_or_default();
    if q.len() > 256 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_search_query"));
    }
    let mode: Option<i16> = query
        .get("m")
        .and_then(|v| v.parse().ok())
        .filter(|m| *m >= 0);
    if mode.is_some_and(|m| m > 3) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_mode"));
    }
    let status = query
        .get("s")
        .filter(|s| matches!(s.as_str(), "ranked" | "loved" | "pending"));
    let offset: i64 = query
        .get("cursor_string")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if !(0..=1_000_000).contains(&offset) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_search_cursor"));
    }
    let total:i64=sqlx::query_scalar("SELECT count(DISTINCT COALESCE(official_set_id,id)) FROM beatmaps WHERE concat_ws(' ',title,artist,creator,version) ILIKE '%'||$1||'%' AND ($2::smallint IS NULL OR mode=$2) AND ($3::text IS NULL OR status=$3)").bind(&q).bind(mode).bind(status).fetch_one(&state.pool).await?;
    let ids=sqlx::query_scalar::<_,i64>("SELECT DISTINCT COALESCE(official_set_id,id) FROM beatmaps WHERE concat_ws(' ',title,artist,creator,version) ILIKE '%'||$1||'%' AND ($2::smallint IS NULL OR mode=$2) AND ($3::text IS NULL OR status=$3) ORDER BY 1 DESC LIMIT 50 OFFSET $4").bind(q).bind(mode).bind(status).bind(offset).fetch_all(&state.pool).await?;
    let mut sets = Vec::new();
    for id in ids {
        sets.push(set_json(&state, id).await?);
    }
    Ok(Json(
        json!({"beatmapsets":sets,"total":total,"cursor_string":if offset+50<total{Some((offset+50).to_string())}else{None},"error":null}),
    ))
}
async fn scores(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let raw = raw.unwrap_or_default();
    if raw.len() > 4096 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_score_query"));
    }
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(&raw)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_query"))?;
    if pairs.len() > 64 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_score_query"));
    }
    let mut query = HashMap::new();
    let mut requested_mods = Vec::new();
    for (key, value) in pairs {
        if key == "mods[]" {
            requested_mods.push(resonance_protocol::lazer::ApiMod {
                acronym: value,
                settings: Default::default(),
            });
        } else if query.insert(key, value).is_some() {
            return Err(ApiError(StatusCode::BAD_REQUEST, "duplicate_score_query"));
        }
    }
    let user = common::bearer(&state, &headers).await?;
    let map = resolve(&state, id).await?;
    let mode = match query.get("mode").map(String::as_str).unwrap_or("osu") {
        "osu" => 0i16,
        "taiko" => 1,
        "fruits" | "catch" => 2,
        "mania" => 3,
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_mode")),
    };
    let scope = query.get("type").map(String::as_str).unwrap_or("global");
    if !matches!(scope, "global" | "country" | "friend" | "friends") {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_score_scope"));
    }
    let limit: i64 = query
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .clamp(1, 100);
    let filter_acronyms = if requested_mods.len() == 1 && requested_mods[0].acronym == "NM" {
        requested_mods.clear();
        Some(Vec::<String>::new())
    } else if requested_mods.is_empty() {
        None
    } else {
        Some(
            requested_mods
                .iter()
                .map(|modifier| modifier.acronym.clone())
                .collect::<Vec<_>>(),
        )
    };
    let mods = if filter_acronyms.is_none() {
        None
    } else {
        Some(i64::from(lazer::legacy_mods(mode as u8, &requested_mods)?))
    };
    let friends = if matches!(scope, "friend" | "friends") {
        let mut ids = common::friend_ids(&state, user.id)
            .await?
            .into_iter()
            .map(i64::from)
            .collect::<Vec<_>>();
        ids.push(user.id);
        Some(ids)
    } else {
        None
    };
    let rows=sqlx::query("WITH best AS (SELECT DISTINCT ON(s.user_id) s.id,s.user_id,s.score FROM scores s JOIN users u ON u.id=s.user_id WHERE s.beatmap_id=$1 AND s.mode=$2 AND s.client='lazer' AND s.verification='verified' AND s.passed AND NOT u.is_restricted AND ($3::bigint IS NULL OR (s.api_mods IS NULL AND s.mods=$3) OR (s.api_mods IS NOT NULL AND jsonb_array_length(s.api_mods)=cardinality($8::text[]) AND NOT EXISTS(SELECT 1 FROM jsonb_array_elements(s.api_mods) modifier WHERE NOT (modifier->>'acronym'=ANY($8::text[]))))) AND ($3::bigint IS NOT NULL OR s.pp_eligible) AND ($4::text IS NULL OR u.country=$4) AND ($5::bigint[] IS NULL OR s.user_id=ANY($5)) ORDER BY s.user_id,s.score DESC,s.id),placed AS (SELECT *,row_number() OVER(ORDER BY score DESC,id) position,count(*) OVER() score_count FROM best) SELECT * FROM placed WHERE position<=$6 OR user_id=$7 ORDER BY position").bind(map).bind(mode).bind(mods).bind(if scope=="country"{Some(&user.country)}else{None}).bind(friends).bind(limit).bind(user.id).bind(filter_acronyms).fetch_all(&state.pool).await?;
    let score_count = rows
        .first()
        .map(|r| r.get::<i64, _>("score_count"))
        .unwrap_or(0);
    let mut scores = Vec::new();
    let mut own = Value::Null;
    for row in rows {
        let score = lazer::score_json(&state, row.get("id"), &user).await?;
        let position: i64 = row.get("position");
        if row.get::<i64, _>("user_id") == user.id {
            own = json!({"position":position,"score":score});
        }
        if position <= limit {
            scores.push(score);
        }
    }
    Ok(Json(
        json!({"scores":scores,"user_score":own,"score_count":score_count}),
    ))
}
