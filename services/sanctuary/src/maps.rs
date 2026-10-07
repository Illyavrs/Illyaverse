use crate::{ApiError, ApiResult, AppState, audit, auth, page};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use illya_domain::Pagination;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Default)]
pub struct OfficialState {
    token: Option<String>,
    expires: Option<Instant>,
    next_request: Option<Instant>,
}
#[derive(Deserialize)]
pub struct Filter {
    page: Option<u32>,
    page_size: Option<u32>,
    q: Option<String>,
    mode: Option<i16>,
    source: Option<String>,
    status: Option<String>,
}
pub async fn list(State(state): State<AppState>, Query(filter): Query<Filter>) -> ApiResult {
    list_inner(&state, filter).await
}
pub async fn admin_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> ApiResult {
    auth::require(&state, &headers)
        .await?
        .require("admin.view")?;
    list_inner(&state, filter).await
}
async fn list_inner(state: &AppState, filter: Filter) -> ApiResult {
    if filter.q.as_ref().is_some_and(|v| v.len() > 100) {
        return Err(ApiError::bad("Search is too long."));
    }
    let (p, size, offset) = Pagination {
        page: filter.page,
        page_size: filter.page_size,
    }
    .bounds();
    let q = filter.q.map(|q| format!("%{q}%"));
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM beatmaps WHERE ($1::text IS NULL OR title ILIKE $1 OR artist ILIKE $1 OR creator ILIKE $1) AND ($2::smallint IS NULL OR mode=$2) AND ($3::text IS NULL OR source=$3) AND ($4::text IS NULL OR status=$4)").bind(&q).bind(filter.mode).bind(&filter.source).bind(&filter.status).fetch_one(&state.db).await?;
    let items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(b)-'osu_path'-'archive_path' || jsonb_build_object('play_count',(SELECT count(*) FROM scores s WHERE s.beatmap_id=b.id AND verification='verified')) FROM beatmaps b WHERE ($1::text IS NULL OR title ILIKE $1 OR artist ILIKE $1 OR creator ILIKE $1) AND ($2::smallint IS NULL OR mode=$2) AND ($3::text IS NULL OR source=$3) AND ($4::text IS NULL OR status=$4) ORDER BY b.created_at DESC,b.id DESC LIMIT $5 OFFSET $6").bind(q).bind(filter.mode).bind(filter.source).bind(filter.status).bind(size).bind(offset).fetch_all(&state.db).await?;
    Ok(page(items, total, p, size))
}
pub async fn get(state: &AppState, id: i64) -> ApiResult<Value> {
    sqlx::query_scalar("SELECT to_jsonb(b)-'osu_path'-'archive_path' || jsonb_build_object('play_count',(SELECT count(*) FROM scores s WHERE s.beatmap_id=b.id AND verification='verified'),'download_url','/api/v1/beatmaps/'||b.id||'/download') FROM beatmaps b WHERE id=$1").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)
}
pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(get(&state, id).await?))
}
async fn file_response(state: &AppState, id: i64, archive: bool) -> ApiResult<Response> {
    let row = sqlx::query("SELECT osu_path,archive_path FROM beatmaps WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    let path = if archive {
        row.get::<Option<String>, _>("archive_path")
            .unwrap_or_else(|| row.get("osu_path"))
    } else {
        row.get("osu_path")
    };
    if std::path::Path::new(&path).is_absolute() || path.contains("..") {
        return Err(ApiError::internal("unsafe stored map path"));
    }
    let bytes = tokio::fs::read(state.maps_root.join(&path)).await?;
    let filename = format!(
        "illyaverse-{id}.{}",
        if path.ends_with(".osz") { "osz" } else { "osu" }
    );
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
pub async fn download(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Response> {
    file_response(&state, id, true).await
}
pub async fn osu_file(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Response> {
    file_response(&state, id, false).await
}

pub struct ParsedMap {
    pub bytes: Vec<u8>,
    pub title: String,
    pub artist: String,
    pub creator: String,
    pub version: String,
    pub mode: i16,
    pub stars: f64,
    pub bpm: f64,
    pub length: i32,
    pub max_combo: i32,
    pub md5: String,
}
pub fn parse_map(bytes: Vec<u8>) -> ApiResult<ParsedMap> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err(ApiError::bad("Each .osu file must be at most 8 MiB."));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ApiError::bad("Beatmap must be UTF-8."))?
        .trim_start_matches('\u{feff}');
    if !text.starts_with("osu file format v") {
        return Err(ApiError::bad("Invalid .osu file header."));
    }
    let mut section = "";
    let mut fields = HashMap::new();
    let mut object_count = 0;
    let mut last_time = 0.0_f64;
    let mut bpm = 0.0;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line;
            continue;
        }
        if matches!(section, "[Metadata]" | "[General]")
            && let Some((key, value)) = line.split_once(':')
        {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
        if section == "[HitObjects]" && !line.is_empty() && !line.starts_with("//") {
            object_count += 1;
            let parts: Vec<&str> = line.split(',').collect();
            if let Some(time) = parts.get(2).and_then(|v| v.parse::<f64>().ok()) {
                last_time = last_time.max(time);
            }
        }
        if section == "[TimingPoints]"
            && bpm == 0.0
            && let Some(beat_length) = line
                .split(',')
                .nth(1)
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
        {
            bpm = 60000.0 / beat_length;
        }
    }
    if object_count == 0
        || object_count > 100_000
        || !last_time.is_finite()
        || last_time > 24.0 * 3600.0 * 1000.0
    {
        return Err(ApiError::bad(
            "Beatmap has no objects or exceeds processing limits.",
        ));
    }
    let field = |key: &str| -> ApiResult<String> {
        let value = fields
            .get(key)
            .filter(|v| !v.is_empty() && v.len() <= 512 && !v.contains('\0'))
            .ok_or_else(|| ApiError::bad(format!("Missing or invalid {key}.")))?;
        Ok(value.clone())
    };
    let mode: i16 = fields
        .get("Mode")
        .map(|v| v.parse().unwrap_or(-1))
        .unwrap_or(0);
    if !(0..=3).contains(&mode) {
        return Err(ApiError::bad("Unsupported game mode."));
    }
    let map = rosu_pp::Beatmap::from_bytes(&bytes)
        .map_err(|_| ApiError::bad("Beatmap cannot be decoded."))?;
    let attrs = rosu_pp::Difficulty::new()
        .checked_calculate(&map)
        .map_err(|_| ApiError::bad("Beatmap exceeds safe difficulty calculation limits."))?;
    let stars = attrs.stars();
    if !stars.is_finite() {
        return Err(ApiError::bad("Beatmap has invalid difficulty."));
    }
    Ok(ParsedMap {
        title: field("Title")?,
        artist: field("Artist")?,
        creator: field("Creator")?,
        version: field("Version")?,
        mode,
        stars,
        bpm: if bpm.is_finite() { bpm } else { 0.0 },
        length: (last_time / 1000.0) as i32,
        max_combo: attrs.max_combo().min(i32::MAX as u32) as i32,
        md5: format!("{:x}", md5::compute(&bytes)),
        bytes,
    })
}
pub fn unpack_archive(bytes: &[u8]) -> ApiResult<Vec<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError::bad("Invalid .osz archive."))?;
    if archive.len() > 512 {
        return Err(ApiError::bad("Archive may contain at most 512 entries."));
    }
    let mut total = 0_u64;
    let mut maps = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ApiError::bad("Unreadable archive entry."))?;
        let name = entry.name().replace('\\', "/");
        if entry.enclosed_name().is_none()
            || name.starts_with('/')
            || name.split('/').any(|p| p == ".." || p.contains(':'))
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err(ApiError::bad(
                "Archive contains an unsafe path or symbolic link.",
            ));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| ApiError::bad("Archive size overflow."))?;
        if total > 256 * 1024 * 1024 || entry.size() > 64 * 1024 * 1024 {
            return Err(ApiError::bad(
                "Archive exceeds its uncompressed size limit.",
            ));
        }
        if name.to_ascii_lowercase().ends_with(".osu") {
            if entry.size() > 8 * 1024 * 1024 {
                return Err(ApiError::bad("Beatmap file exceeds 8 MiB."));
            }
            let mut buffer = Vec::new();
            entry
                .by_ref()
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut buffer)
                .map_err(|_| ApiError::bad("Archive decompression failed."))?;
            if buffer.len() > 8 * 1024 * 1024 {
                return Err(ApiError::bad("Beatmap file exceeds 8 MiB."));
            }
            maps.push(buffer);
        }
    }
    if maps.is_empty() {
        return Err(ApiError::bad("Archive contains no .osu beatmap files."));
    }
    Ok(maps)
}
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.upload")?;
    let _permit = state.upload_permit()?;
    let mut upload = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or("").to_ascii_lowercase();
        if !filename.ends_with(".osu") && !filename.ends_with(".osz") {
            return Err(ApiError::bad("Upload an .osu file or .osz archive."));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
        {
            if bytes.len() + chunk.len() > 64 * 1024 * 1024 {
                return Err(ApiError::bad("Upload must be at most 64 MiB."));
            }
            bytes.extend_from_slice(&chunk);
        }
        upload = Some((filename, bytes));
    }
    let (filename, bytes) = upload.ok_or_else(|| ApiError::bad("Attach a file."))?;
    let is_archive = filename.ends_with(".osz");
    let archive_bytes = if is_archive {
        Some(bytes.clone())
    } else {
        None
    };
    let parsed = tokio::task::spawn_blocking(move || -> ApiResult<Vec<ParsedMap>> {
        let inputs = if is_archive {
            unpack_archive(&bytes)?
        } else {
            vec![bytes]
        };
        inputs.into_iter().map(parse_map).collect()
    })
    .await
    .map_err(ApiError::internal)??;
    let package = Uuid::new_v4().to_string();
    tokio::fs::create_dir_all(state.maps_root.join(&package)).await?;
    let archive_path = archive_bytes
        .as_ref()
        .map(|_| format!("{package}/package.osz"));
    if let Some(bytes) = archive_bytes {
        tokio::fs::write(state.maps_root.join(archive_path.as_ref().unwrap()), bytes).await?;
    }
    let mut tx = state.db.begin().await?;
    let mut ids = Vec::new();
    for map in parsed {
        let path = format!("{package}/{}.osu", map.md5);
        tokio::fs::write(state.maps_root.join(&path), &map.bytes).await?;
        let id:i64=sqlx::query_scalar("INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,stars,bpm,length_seconds,max_combo,md5,osu_path,archive_path) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id").bind(user.id).bind(map.title).bind(map.artist).bind(map.creator).bind(map.version).bind(map.mode).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(map.md5).bind(path).bind(&archive_path).fetch_one(&mut *tx).await?;
        ids.push(id);
    }
    tx.commit().await?;
    audit(
        &state,
        user.id,
        "beatmaps.upload",
        "beatmap_package",
        package,
        json!({"beatmap_ids":ids}),
    )
    .await?;
    let mut items = Vec::new();
    for id in ids {
        items.push(get(&state, id).await?);
    }
    Ok(Json(json!({"items":items})))
}

async fn throttle(official: &mut OfficialState) {
    if let Some(next) = official.next_request {
        tokio::time::sleep(next.saturating_duration_since(Instant::now())).await;
    }
    let interval = std::env::var("OSU_API_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1000)
        .max(500);
    official.next_request = Some(Instant::now() + Duration::from_millis(interval));
}
async fn official_request(
    state: &AppState,
    path: &str,
    authenticated: bool,
) -> ApiResult<reqwest::Response> {
    let mut official = state.official.lock().await;
    if authenticated
        && (official.expires.is_none_or(|e| e <= Instant::now()) || official.token.is_none())
    {
        let id = std::env::var("OSU_CLIENT_ID")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "official_api_not_configured",
                    "Configure an osu! OAuth client before importing maps.".into(),
                )
            })?;
        let secret = std::env::var("OSU_CLIENT_SECRET")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "official_api_not_configured",
                    "Configure an osu! OAuth client before importing maps.".into(),
                )
            })?;
        throttle(&mut official).await;
        let response=state.client.post("https://osu.ppy.sh/oauth/token").json(&json!({"client_id":id,"client_secret":secret,"grant_type":"client_credentials","scope":"public"})).send().await.map_err(ApiError::internal)?;
        if !response.status().is_success() {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "official_auth_failed",
                "The official API rejected the configured credentials.".into(),
            ));
        }
        let token: Value = response.json().await.map_err(ApiError::internal)?;
        official.token = Some(
            token["access_token"]
                .as_str()
                .ok_or_else(|| ApiError::internal("missing OAuth access_token"))?
                .into(),
        );
        official.expires = Some(
            Instant::now()
                + Duration::from_secs(
                    token["expires_in"]
                        .as_u64()
                        .unwrap_or(60)
                        .saturating_sub(30),
                ),
        );
    }
    for attempt in 0..3 {
        throttle(&mut official).await;
        let mut request = state.client.get(format!("https://osu.ppy.sh{path}"));
        if authenticated {
            request = request.bearer_auth(official.token.as_ref().unwrap());
        }
        let response = request.send().await.map_err(ApiError::internal)?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let delay = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(2_u64.pow(attempt + 1))
                .clamp(1, 60);
            official.next_request = Some(Instant::now() + Duration::from_secs(delay));
            continue;
        }
        if response.status() == StatusCode::NOT_FOUND {
            return Err(ApiError::missing());
        }
        if !response.status().is_success() {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "official_api_failed",
                "The official API could not complete the request.".into(),
            ));
        }
        return Ok(response);
    }
    Err(ApiError(
        StatusCode::TOO_MANY_REQUESTS,
        "official_rate_limited",
        "The official API rate limit was reached. Please try later.".into(),
    ))
}
#[derive(Deserialize)]
pub struct Import {
    official_id: i64,
}
pub async fn import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Import>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.import")?;
    if input.official_id <= 0 {
        return Err(ApiError::bad("Enter a positive official beatmap ID."));
    }
    let metadata: Value = official_request(
        &state,
        &format!("/api/v2/beatmaps/{}", input.official_id),
        true,
    )
    .await?
    .json()
    .await
    .map_err(ApiError::internal)?;
    let response = official_request(&state, &format!("/osu/{}", input.official_id), false).await?;
    if response
        .content_length()
        .is_some_and(|size| size > 8 * 1024 * 1024)
    {
        return Err(ApiError::bad("Official beatmap exceeds size limit."));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ApiError::internal)?;
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err(ApiError::bad("Official beatmap exceeds size limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    let map = tokio::task::spawn_blocking(move || parse_map(bytes))
        .await
        .map_err(ApiError::internal)??;
    if metadata["checksum"]
        .as_str()
        .is_some_and(|checksum| checksum != map.md5)
    {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "checksum_mismatch",
            "Official metadata and beatmap checksum did not match.".into(),
        ));
    }
    let path = format!("official-{}.osu", map.md5);
    tokio::fs::write(state.maps_root.join(&path), &map.bytes).await?;
    let status = match metadata["status"].as_str() {
        Some("ranked" | "approved") => "ranked",
        Some("loved") => "loved",
        _ => "unranked",
    };
    let id:i64=sqlx::query_scalar("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'official',$10,$11,$12,$13,$14,$15) ON CONFLICT(md5) DO UPDATE SET official_id=excluded.official_id,official_set_id=excluded.official_set_id,status=excluded.status,source='official' RETURNING id")
        .bind(input.official_id).bind(metadata["beatmapset_id"].as_i64()).bind(user.id).bind(map.title).bind(map.artist).bind(map.creator).bind(map.version).bind(map.mode).bind(status).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(map.md5).bind(path).fetch_one(&state.db).await?;
    audit(
        &state,
        user.id,
        "beatmaps.import",
        "beatmap",
        id,
        json!({"official_id":input.official_id}),
    )
    .await?;
    Ok(Json(get(&state, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(name: &str, data: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, data).unwrap();
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn archive_rejects_traversal() {
        assert!(unpack_archive(&archive("../evil.osu", b"x")).is_err());
        assert!(unpack_archive(&archive("C:/evil.osu", b"x")).is_err());
        assert!(unpack_archive(&archive("..\\evil.osu", b"x")).is_err());
    }
    #[test]
    fn invalid_maps_are_rejected() {
        assert!(parse_map(b"not a map".to_vec()).is_err());
        assert!(
            parse_map(b"osu file format v14\n[Metadata]\nTitle:missing objects".to_vec()).is_err()
        );
    }
    #[test]
    fn parses_real_difficulty() {
        let data=b"osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:Test\nArtist:Illyaverse\nCreator:Test\nVersion:Easy\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n";
        let map = parse_map(data.to_vec()).unwrap();
        assert!(map.stars.is_finite());
        assert_eq!(map.max_combo, 2);
        assert_eq!(map.bpm, 120.0);
    }
}
