use crate::{ApiError, ApiResult, AppState, auth, page};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use illya_domain::{Pagination, beatmap_filename::valid_beatmap_filename};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct OfficialState {
    token: Option<String>,
    expires: Option<Instant>,
    next_request: Option<Instant>,
    failure: Option<CachedFailure>,
    resolutions: HashMap<MapSelector, CachedFailure>,
    flights: HashMap<MapSelector, Weak<Mutex<()>>>,
    slots: Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    fixture: Option<OfficialFixture>,
}
impl Default for OfficialState {
    fn default() -> Self {
        Self {
            token: None,
            expires: None,
            next_request: None,
            failure: None,
            resolutions: HashMap::new(),
            flights: HashMap::new(),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            #[cfg(test)]
            fixture: None,
        }
    }
}

const MAX_RESOLUTION_KEYS: usize = 512;
const MAX_OSU_BYTES: usize = 8 * 1024 * 1024;
const MAX_OFFICIAL_ID: i64 = 1_999_999_999;

fn package_conflict(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, "map_package_conflict", message.into())
}

fn official_namespace_conflict() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "official_namespace_conflict",
        "This official identity overlaps the managed local beatmap namespace.".into(),
    )
}

fn filename_ambiguous() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "beatmap_filename_ambiguous",
        "More than one stored beatmap has this filename.".into(),
    )
}

fn filename_conflict() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "beatmap_filename_conflict",
        "The filename result conflicts with an existing beatmap identity.".into(),
    )
}

const PUBLIC_MAP: &str = r#"to_jsonb(b)-'osu_path'-'archive_path' || jsonb_build_object(
    'play_count',(SELECT count(*) FROM scores s WHERE s.beatmap_id=b.id AND verification='verified'),
    'download_url','/api/v1/beatmaps/'||b.id||'/download',
    'download_kind',CASE WHEN right(COALESCE(b.archive_path,b.osu_path),4)='.osz' THEN 'osz' ELSE 'osu' END,
    'package',(SELECT jsonb_build_object(
        'id',p.id,'native_set_id',p.native_set_id,'kind',p.kind,'legacy',p.legacy,
        'difficulty_count',cardinality(p.member_md5),
        'beatmap_ids',(SELECT jsonb_agg(m.id ORDER BY m.id) FROM beatmaps m WHERE m.local_package_id=p.id),
        'download_url','/api/v1/beatmaps/'||(SELECT min(m.id) FROM beatmaps m WHERE m.local_package_id=p.id)||'/download'
    ) FROM beatmap_packages p WHERE p.id=b.local_package_id)
)"#;

fn commit_error_confirms_rollback(severity: sqlx::postgres::PgSeverity, code: &str) -> bool {
    severity == sqlx::postgres::PgSeverity::Error
        && matches!(
            code,
            "23000"
                | "23001"
                | "23502"
                | "23503"
                | "23505"
                | "23514"
                | "23P01"
                | "25P02"
                | "40000"
                | "40001"
                | "40002"
                | "40P01"
                | "P0001"
        )
}

pub(crate) fn commit_definitely_rolled_back(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(database) = error else {
        return false;
    };
    let Some(postgres) = database.try_downcast_ref::<sqlx::postgres::PgDatabaseError>() else {
        return false;
    };
    // ErrorResponse also carries Fatal/Panic and outcome-unknown conditions.
    // Keep files unless this ordinary ERROR explicitly establishes rollback.
    commit_error_confirms_rollback(postgres.severity(), postgres.code())
}

#[derive(Clone)]
struct CachedFailure {
    until: Instant,
    status: StatusCode,
    code: &'static str,
    message: String,
}
impl CachedFailure {
    fn new(error: &ApiError, duration: Duration) -> Self {
        Self {
            until: Instant::now() + duration,
            status: error.0,
            code: error.1,
            message: error.2.clone(),
        }
    }
    fn error(&self) -> Option<ApiError> {
        (self.until > Instant::now())
            .then(|| ApiError(self.status, self.code, self.message.clone()))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum MapSelector {
    Checksum(String),
    OfficialId(i64),
    Filename(String),
}

#[cfg(test)]
struct OfficialFixture {
    origin: String,
    configured: bool,
}
#[cfg(test)]
impl OfficialState {
    pub(crate) fn fixture(origin: String, configured: bool) -> Self {
        Self {
            fixture: Some(OfficialFixture { origin, configured }),
            ..Self::default()
        }
    }
    pub(crate) fn backoff_remaining(&self) -> Option<Duration> {
        self.failure
            .as_ref()
            .filter(|failure| failure.status == StatusCode::TOO_MANY_REQUESTS)
            .map(|failure| failure.until.saturating_duration_since(Instant::now()))
    }
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
    let query = format!(
        "SELECT {PUBLIC_MAP} FROM beatmaps b WHERE ($1::text IS NULL OR title ILIKE $1 OR artist ILIKE $1 OR creator ILIKE $1) AND ($2::smallint IS NULL OR mode=$2) AND ($3::text IS NULL OR source=$3) AND ($4::text IS NULL OR status=$4) ORDER BY b.created_at DESC,b.id DESC LIMIT $5 OFFSET $6"
    );
    let items = sqlx::query_scalar::<_, Value>(&query)
        .bind(q)
        .bind(filter.mode)
        .bind(filter.source)
        .bind(filter.status)
        .bind(size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
    Ok(page(items, total, p, size))
}
pub async fn get(state: &AppState, id: i64) -> ApiResult<Value> {
    let query = format!("SELECT {PUBLIC_MAP} FROM beatmaps b WHERE id=$1");
    sqlx::query_scalar(&query)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)
}
pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    Ok(Json(get(&state, id).await?))
}

pub async fn package_difficulties(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(pagination): Query<Pagination>,
) -> ApiResult {
    let package: Uuid = sqlx::query_scalar(
        "SELECT local_package_id FROM beatmaps WHERE id=$1 AND local_package_id IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    let (p, size, offset) = pagination.bounds();
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM beatmaps WHERE local_package_id=$1")
        .bind(package)
        .fetch_one(&state.db)
        .await?;
    // A package summary is intentionally smaller than PUBLIC_MAP. Fetch the
    // current member rows in one slice, without nested package/member metadata.
    let items = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_object('id',id,'title',title,'artist',artist,'creator',creator,
        'version',version,'mode',mode,'status',status,'source',source,'stars',stars,
        'length_seconds',length_seconds,'bpm',bpm,'max_combo',max_combo)
        FROM beatmaps WHERE local_package_id=$1 ORDER BY id ASC LIMIT $2 OFFSET $3",
    )
    .bind(package)
    .bind(size)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(page(items, total, p, size))
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
    pub official_id: Option<i64>,
    pub official_set_id: Option<i64>,
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
        official_id: fields.get("BeatmapID").and_then(|value| value.parse().ok()),
        official_set_id: fields
            .get("BeatmapSetID")
            .and_then(|value| value.parse().ok()),
        bytes,
    })
}
pub fn unpack_archive(bytes: &[u8]) -> ApiResult<Vec<(String, Vec<u8>)>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError::bad("Invalid .osz archive."))?;
    if archive.len() > 512 {
        return Err(ApiError::bad("Archive may contain at most 512 entries."));
    }
    let mut total = 0_u64;
    let mut maps = Vec::new();
    let mut paths = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ApiError::bad("Unreadable archive entry."))?;
        // Keep the zip crate's decoded entry name; do not guess legacy encodings.
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
        // Compare extracted paths after slash/segment normalization and Unicode
        // lowercase, so case variants cannot overwrite another package member.
        let normalized = name
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .collect::<Vec<_>>()
            .join("/")
            .to_lowercase();
        if !paths.insert(normalized) {
            return Err(ApiError::bad("Archive contains duplicate file paths."));
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
            let filename = name.rsplit('/').next().unwrap_or_default();
            if !valid_beatmap_filename(filename) {
                return Err(ApiError::bad(
                    "Archive contains an invalid beatmap filename.",
                ));
            }
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
            maps.push((filename.to_owned(), buffer));
        }
    }
    if maps.is_empty() {
        return Err(ApiError::bad("Archive contains no .osu beatmap files."));
    }
    Ok(maps)
}

fn package_write_error(error: sqlx::Error) -> ApiError {
    if error
        .as_database_error()
        .is_some_and(|error| error.is_unique_violation())
    {
        package_conflict(
            "A difficulty or package already exists. Upload the original complete package to retry.",
        )
    } else {
        error.into()
    }
}

async fn write_owned_map_file(
    path: std::path::PathBuf,
    bytes: &[u8],
    owned_files: &mut Vec<std::path::PathBuf>,
) -> ApiResult<()> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await?;
    owned_files.push(path);
    file.write_all(bytes).await?;
    file.sync_all().await?;
    Ok(())
}

async fn lock_package_checksums(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    manifest: &[String],
) -> ApiResult<()> {
    // All local uploaders acquire the actual advisory keys in the same order,
    // including the unlikely case of two checksum strings hashing to one key.
    // Authority locks are already held; row insertion also follows MD5 order.
    let locks = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT hashtextextended('illyaverse:local-map:'||checksum,0) AS lock_key FROM unnest($1::text[]) AS checksums(checksum) ORDER BY lock_key",
    ).bind(manifest).fetch_all(&mut **tx).await?;
    for key in locks {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(key)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

async fn lock_filename_aliases(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    filenames: &[String],
) -> ApiResult<()> {
    // The two-int advisory namespace is separate from the package checksum's
    // bigint namespace. Sort actual hash keys, including collisions. Every
    // publisher takes authority, then filename locks, then checksum/row locks.
    const NAMESPACE: i32 = 0x494c464e;
    let keys = sqlx::query_scalar::<_, i32>(
        "SELECT DISTINCT hashtext(filename COLLATE \"C\") AS lock_key FROM unnest($1::text[]) AS aliases(filename) ORDER BY lock_key",
    ).bind(filenames).fetch_all(&mut **tx).await?;
    for key in keys {
        sqlx::query("SELECT pg_advisory_xact_lock($1,$2)")
            .bind(NAMESPACE)
            .bind(key)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

struct UploadMap {
    filename: String,
    map: ParsedMap,
}

struct Fingerprint<'a> {
    kind: &'a str,
    sha256: &'a str,
    byte_length: i64,
}

async fn retry_local_package(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &auth::MutationActor,
    existing: &[sqlx::postgres::PgRow],
    manifest: &[String],
    fingerprint: &Fingerprint<'_>,
) -> ApiResult<Vec<i64>> {
    let kind = fingerprint.kind;
    let sha256 = fingerprint.sha256;
    let byte_length = fingerprint.byte_length;
    let conflict = || {
        package_conflict(
            "Existing difficulties belong to a different, incomplete or changed package. Upload its original complete file.",
        )
    };
    if existing.len() != manifest.len() {
        return Err(conflict());
    }
    let package_id = existing[0]
        .get::<Option<Uuid>, _>("local_package_id")
        .ok_or_else(conflict)?;
    if existing.iter().any(|row| {
        row.get::<Option<Uuid>, _>("local_package_id") != Some(package_id)
            || row.get::<Option<i64>, _>("uploaded_by") != Some(actor.id)
            || row.get::<String, _>("source") != "local"
    }) {
        return Err(conflict());
    }
    let package = sqlx::query("SELECT * FROM beatmap_packages WHERE id=$1 FOR UPDATE")
        .bind(package_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(conflict)?;
    let members = sqlx::query_scalar::<_, String>(
        "SELECT md5 FROM beatmaps WHERE local_package_id=$1 ORDER BY md5",
    )
    .bind(package_id)
    .fetch_all(&mut **tx)
    .await?;
    if package.get::<Option<i64>, _>("uploaded_by") != Some(actor.id)
        || package.get::<String, _>("kind") != kind
        || package.get::<Vec<String>, _>("member_md5") != manifest
        || members != manifest
        || existing.iter().any(|row| {
            row.get::<Option<String>, _>("archive_path")
                != package.get::<Option<String>, _>("archive_path")
        })
    {
        return Err(conflict());
    }
    match package.get::<Option<String>, _>("sha256") {
        Some(stored)
            if stored == sha256
                && package.get::<Option<i64>, _>("byte_length") == Some(byte_length) => {}
        Some(_) => return Err(conflict()),
        None => {
            if !package.get::<bool, _>("legacy") {
                return Err(conflict());
            }
            let retained = if kind == "osz" {
                package
                    .get::<Option<String>, _>("archive_path")
                    .ok_or_else(conflict)?
            } else {
                existing[0].get::<String, _>("osu_path")
            };
            let stored_path = std::path::Path::new(&retained);
            if stored_path.is_absolute() || retained.contains("..") {
                return Err(conflict());
            }
            let path = state.maps_root.join(stored_path);
            let metadata = tokio::fs::symlink_metadata(&path)
                .await
                .map_err(|_| conflict())?;
            if !metadata.is_file() || metadata.len() != byte_length as u64 {
                return Err(conflict());
            }
            let bytes = tokio::fs::read(path).await.map_err(|_| conflict())?;
            if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
                return Err(conflict());
            }
            // A migration never invents this fingerprint. Only an exact read of
            // the retained original payload can establish it on a legacy retry.
            sqlx::query(
                "UPDATE beatmap_packages SET sha256=$2,byte_length=$3,legacy=false WHERE id=$1",
            )
            .bind(package_id)
            .bind(sha256)
            .bind(byte_length)
            .execute(&mut **tx)
            .await
            .map_err(package_write_error)?;
        }
    }
    Ok(existing.iter().map(|row| row.get("id")).collect())
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
        if upload.is_some() {
            return Err(ApiError::bad("Attach exactly one beatmap file or archive."));
        }
        let filename = field.file_name().unwrap_or("").to_owned();
        let extension = filename.to_ascii_lowercase();
        if !extension.ends_with(".osu") && !extension.ends_with(".osz") {
            return Err(ApiError::bad("Upload an .osu file or .osz archive."));
        }
        if extension.ends_with(".osu") && !valid_beatmap_filename(&filename) {
            return Err(ApiError::bad(
                "Supply a valid beatmap filename without a path.",
            ));
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
    let is_archive = filename.to_ascii_lowercase().ends_with(".osz");
    let kind = if is_archive { "osz" } else { "osu" };
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let byte_length = bytes.len() as i64;
    let archive_bytes = if is_archive {
        Some(bytes.clone())
    } else {
        None
    };
    let mut parsed = tokio::task::spawn_blocking(move || -> ApiResult<Vec<UploadMap>> {
        let inputs = if is_archive {
            unpack_archive(&bytes)?
        } else {
            vec![(filename, bytes)]
        };
        inputs
            .into_iter()
            .map(|(filename, bytes)| {
                Ok(UploadMap {
                    filename,
                    map: parse_map(bytes)?,
                })
            })
            .collect()
    })
    .await
    .map_err(ApiError::internal)??;
    parsed.sort_unstable_by(|left, right| left.map.md5.cmp(&right.map.md5));
    if parsed
        .windows(2)
        .any(|pair| pair[0].map.md5 == pair[1].map.md5)
    {
        return Err(ApiError::bad(
            "An archive must contain each difficulty checksum only once.",
        ));
    }
    let manifest: Vec<String> = parsed.iter().map(|input| input.map.md5.clone()).collect();
    let filenames: Vec<String> = parsed.iter().map(|input| input.filename.clone()).collect();
    let package_id = Uuid::new_v4();
    let package = package_id.to_string();
    let directory = state.maps_root.join(&package);
    tokio::fs::create_dir(&directory).await?;
    let archive_path = archive_bytes
        .as_ref()
        .map(|_| format!("{package}/package.osz"));
    let mut owned_files = Vec::new();
    let mut retain_files = false;
    let outcome: ApiResult<(Vec<i64>, bool)> = async {
        // Finish the unique, unreferenced package before taking authority locks.
        if let Some(bytes) = archive_bytes {
            let path = state.maps_root.join(archive_path.as_ref().unwrap());
            write_owned_map_file(path, &bytes, &mut owned_files).await?;
        }
        for input in &parsed {
            let map = &input.map;
            let path = directory.join(format!("{}.osu", map.md5));
            write_owned_map_file(path, &map.bytes, &mut owned_files).await?;
        }
        let mut tx = state.db.begin().await?;
        let actor = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Permissions).await?;
        actor.require("beatmaps.upload")?;
        let retry_candidate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM beatmaps WHERE md5=ANY($1))")
            .bind(&manifest).fetch_one(&mut *tx).await?;
        if !retry_candidate { lock_filename_aliases(&mut tx, &filenames).await?; }
        lock_package_checksums(&mut tx, &manifest).await?;
        let existing = sqlx::query("SELECT id,md5,source,uploaded_by,local_package_id,archive_path,osu_path FROM beatmaps WHERE md5=ANY($1) ORDER BY md5 FOR UPDATE")
            .bind(&manifest).fetch_all(&mut *tx).await?;
        if !existing.is_empty() {
            let fingerprint = Fingerprint { kind, sha256: &sha256, byte_length };
            let ids = retry_local_package(&state, &mut tx, &actor, &existing, &manifest, &fingerprint).await?;
            auth::ensure_mutation_actor(&mut tx, &actor).await?;
            tx.commit().await?;
            return Ok((ids, true));
        }
        if retry_candidate {
            return Err(package_conflict("The existing package changed while retrying. Upload its original complete file."));
        }
        if parsed.iter().map(|input| &input.map).any(|map| map.official_id.is_some_and(|id| id > 0)
            || map.official_set_id.is_some_and(|id| id > 0)) {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "official_beatmap_requires_import",
                "This file contains an official beatmap or set ID. Use official beatmap import instead of local upload.".into(),
            ));
        }
        sqlx::query("INSERT INTO beatmap_packages(id,uploaded_by,kind,sha256,byte_length,member_md5,archive_path) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(package_id).bind(actor.id).bind(kind).bind(&sha256).bind(byte_length).bind(&manifest).bind(&archive_path).execute(&mut *tx).await.map_err(package_write_error)?;
        let mut ids = Vec::new();
        for input in parsed {
            let UploadMap { filename, map } = input;
            let path = format!("{package}/{}.osu", map.md5);
            let id:i64=sqlx::query_scalar("INSERT INTO beatmaps(uploaded_by,title,artist,creator,version,mode,stars,bpm,length_seconds,max_combo,md5,osu_path,archive_path,local_package_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING id").bind(actor.id).bind(map.title).bind(map.artist).bind(map.creator).bind(map.version).bind(map.mode).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(map.md5).bind(path).bind(&archive_path).bind(package_id).fetch_one(&mut *tx).await.map_err(package_write_error)?;
            sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2)")
                .bind(filename).bind(id).execute(&mut *tx).await?;
            ids.push(id);
        }
        sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.upload','beatmap_package',$2,$3)")
            .bind(actor.id).bind(&package).bind(json!({"beatmap_ids":ids})).execute(&mut *tx).await?;
        auth::ensure_mutation_actor(&mut tx, &actor).await?;
        // Keep the complete package if COMMIT was sent but its outcome is unknown.
        retain_files = true;
        if let Err(error) = tx.commit().await {
            retain_files = !commit_definitely_rolled_back(&error);
            return Err(error.into());
        }
        Ok((ids, false))
    }.await;
    if !retain_files {
        for path in owned_files {
            let _ = tokio::fs::remove_file(path).await;
        }
        let _ = tokio::fs::remove_dir(directory).await;
    }
    let (ids, idempotent) = outcome?;
    let mut items = Vec::new();
    for id in ids {
        items.push(get(&state, id).await?);
    }
    let package = items
        .first()
        .and_then(|item| item.get("package"))
        .cloned()
        .ok_or_else(|| ApiError::internal("uploaded package has no public metadata"))?;
    Ok(Json(
        json!({"items":items,"package":package,"idempotent":idempotent}),
    ))
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
    #[cfg(test)]
    let interval = if official.fixture.is_some() {
        1
    } else {
        interval
    };
    official.next_request = Some(Instant::now() + Duration::from_millis(interval));
}

fn upstream_error(code: &'static str, message: &str) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, code, message.into())
}

fn configured_credentials(_official: &OfficialState) -> ApiResult<(String, String)> {
    #[cfg(test)]
    if let Some(fixture) = &_official.fixture {
        return if fixture.configured {
            Ok(("fixture-client".into(), "fixture-secret".into()))
        } else {
            Err(unconfigured())
        };
    }
    let id = std::env::var("OSU_CLIENT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let secret = std::env::var("OSU_CLIENT_SECRET")
        .ok()
        .filter(|value| !value.trim().is_empty());
    id.zip(secret).ok_or_else(unconfigured)
}

fn unconfigured() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "official_api_not_configured",
        "Configure an osu! OAuth client before resolving official maps.".into(),
    )
}

fn remember_upstream_failure(official: &mut OfficialState, error: &ApiError) {
    let duration = if matches!(
        error.1,
        "official_api_not_configured" | "official_auth_failed"
    ) {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(5)
    };
    let mut failure = CachedFailure::new(error, duration);
    if error.0 == StatusCode::TOO_MANY_REQUESTS
        && let Some(next) = official.next_request
    {
        failure.until = next;
    }
    official.failure = Some(failure);
}

// All production requests use this fixed origin. Only a test fixture can replace it.
async fn send_official_request(
    state: &AppState,
    official: &mut OfficialState,
    path: &str,
    oauth_body: Option<&Value>,
    authenticated: bool,
) -> ApiResult<reqwest::Response> {
    let origin = "https://osu.ppy.sh";
    #[cfg(test)]
    let origin = official
        .fixture
        .as_ref()
        .map_or(origin, |fixture| fixture.origin.as_str());
    let url = format!("{origin}{path}");
    throttle(official).await;
    let mut request = if let Some(body) = oauth_body {
        state.client.post(&url).json(body)
    } else {
        state.client.get(&url)
    };
    request = request.header(header::ACCEPT, "application/json");
    if authenticated {
        request = request.bearer_auth(official.token.as_deref().unwrap_or_default());
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            let error = upstream_error(
                "official_api_failed",
                "The official API could not complete the request.",
            );
            remember_upstream_failure(official, &error);
            return Err(error);
        }
    };
    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        let delay = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2)
            .max(1);
        official.next_request = Some(
            Instant::now()
                .checked_add(Duration::from_secs(delay))
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(3600)),
        );
        // Backoff belongs to later requests. Sleeping here would turn an honest
        // upstream 429 into a gateway timeout and block every other lookup.
        let error = ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "official_rate_limited",
            "The official API rate limit was reached. Please try later.".into(),
        );
        remember_upstream_failure(official, &error);
        return Err(error);
    }
    if response.status() == StatusCode::NOT_FOUND && oauth_body.is_none() {
        return Err(ApiError::missing());
    }
    if !response.status().is_success() {
        let error = if oauth_body.is_some()
            || matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) {
            official.token = None;
            official.expires = None;
            upstream_error(
                "official_auth_failed",
                "The official API rejected the configured credentials.",
            )
        } else {
            upstream_error(
                "official_api_failed",
                "The official API could not complete the request.",
            )
        };
        remember_upstream_failure(official, &error);
        return Err(error);
    }
    Ok(response)
}

async fn read_official_bytes(response: reqwest::Response, limit: usize) -> ApiResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(upstream_error(
            "official_api_failed",
            "The official response exceeds its size limit.",
        ));
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            upstream_error(
                "official_api_failed",
                "The official response could not be read.",
            )
        })?;
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(upstream_error(
                "official_api_failed",
                "The official response exceeds its size limit.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn official_request(
    state: &AppState,
    path: &str,
    authenticated: bool,
) -> ApiResult<reqwest::Response> {
    let mut official = state.official.lock().await;
    if let Some(error) = official.failure.as_ref().and_then(CachedFailure::error) {
        return Err(error);
    }
    if authenticated
        && (official.expires.is_none_or(|e| e <= Instant::now()) || official.token.is_none())
    {
        let (id, secret) = match configured_credentials(&official) {
            Ok(credentials) => credentials,
            Err(error) => {
                remember_upstream_failure(&mut official, &error);
                return Err(error);
            }
        };
        let body = json!({"client_id":id,"client_secret":secret,"grant_type":"client_credentials","scope":"public"});
        let response =
            send_official_request(state, &mut official, "/oauth/token", Some(&body), false).await?;
        let token = read_official_bytes(response, 64 * 1024)
            .await
            .and_then(|bytes| {
                serde_json::from_slice::<Value>(&bytes).map_err(|_| {
                    upstream_error(
                        "official_auth_failed",
                        "The official OAuth response was invalid.",
                    )
                })
            });
        let token = match token {
            Ok(token)
                if token["access_token"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty()) =>
            {
                token
            }
            _ => {
                let error = upstream_error(
                    "official_auth_failed",
                    "The official OAuth response was invalid.",
                );
                remember_upstream_failure(&mut official, &error);
                return Err(error);
            }
        };
        official.token = token["access_token"].as_str().map(str::to_owned);
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
    send_official_request(state, &mut official, path, None, authenticated).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolve {
    user_id: i64,
    #[serde(default, deserialize_with = "present_selector")]
    checksum: Option<String>,
    #[serde(default, deserialize_with = "present_selector")]
    official_id: Option<i64>,
    #[serde(default, deserialize_with = "present_selector")]
    filename: Option<String>,
}
fn present_selector<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
impl Resolve {
    fn selector(&self) -> ApiResult<MapSelector> {
        if self.user_id <= 0 {
            return Err(ApiError::bad("Supply a positive user_id."));
        }
        match (&self.checksum, self.official_id, &self.filename) {
            (Some(checksum), None, None) if valid_checksum(checksum) => {
                Ok(MapSelector::Checksum(checksum.to_ascii_lowercase()))
            }
            (None, Some(id), None) if id > MAX_OFFICIAL_ID => Err(official_namespace_conflict()),
            (None, Some(id), None) if id > 0 => Ok(MapSelector::OfficialId(id)),
            (None, None, Some(filename)) if valid_beatmap_filename(filename) => {
                Ok(MapSelector::Filename(filename.clone()))
            }
            _ => Err(ApiError::bad(
                "Supply exactly one hexadecimal checksum, positive official_id or valid beatmap filename.",
            )),
        }
    }
}
fn valid_checksum(checksum: &str) -> bool {
    checksum.len() == 32 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn require_service_key(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let supplied = headers
        .get("x-illya-service-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .as_bytes();
    let expected = state.internal_key.as_bytes();
    let mut mismatch = expected.len() ^ supplied.len();
    for (index, &byte) in expected.iter().enumerate() {
        mismatch |= usize::from(byte ^ supplied.get(index).copied().unwrap_or_default());
    }
    if expected.len() < 32 || mismatch != 0 {
        Err(ApiError::unauthorized())
    } else {
        Ok(())
    }
}
async fn require_active_user(state: &AppState, id: i64) -> ApiResult<()> {
    let active: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1 AND NOT is_restricted)")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    if active {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}
pub async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Resolve>,
) -> ApiResult {
    require_service_key(&state, &headers)?;
    let selector = input.selector()?;
    require_active_user(&state, input.user_id).await?;
    Ok(Json(resolve_map(&state, input.user_id, selector).await?))
}

async fn stored_map<'e>(
    executor: impl sqlx::Executor<'e, Database = sqlx::Postgres>,
    selector: &MapSelector,
) -> ApiResult<Option<Value>> {
    let (checksum, official_id) = match selector {
        MapSelector::Checksum(checksum) => (Some(checksum.as_str()), None),
        MapSelector::OfficialId(id) => (None, Some(*id)),
        MapSelector::Filename(filename) => {
            // Bound the exact alias index lookup before joining primary keys.
            // Filename resolution must not scan the full beatmap catalogue.
            let mut values = sqlx::query_scalar("SELECT jsonb_build_object('id',b.id,'native_id',b.native_id,'native_set_id',b.native_set_id,'official_id',b.official_id,'official_set_id',b.official_set_id,'md5',b.md5,'status',b.status,'source',b.source,'mode',b.mode) FROM (SELECT beatmap_id FROM beatmap_filename_aliases WHERE filename=$1 COLLATE \"C\" ORDER BY beatmap_id LIMIT 2) aliases JOIN beatmaps b ON b.id=aliases.beatmap_id ORDER BY b.id LIMIT 2")
                .bind(filename).fetch_all(executor).await?;
            if values.len() > 1 {
                return Err(filename_ambiguous());
            }
            return Ok(values.pop());
        }
    };
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',id,'native_id',native_id,'native_set_id',native_set_id,'official_id',official_id,'official_set_id',official_set_id,'md5',md5,'status',status,'source',source,'mode',mode) FROM beatmaps WHERE ($1::text IS NOT NULL AND md5=$1) OR ($2::bigint IS NOT NULL AND official_id=$2)")
        .bind(checksum).bind(official_id).fetch_optional(executor).await?)
}

struct OfficialMap {
    id: i64,
    set_id: i64,
    status: &'static str,
    map: ParsedMap,
}
async fn fetch_official_map(state: &AppState, selector: &MapSelector) -> ApiResult<OfficialMap> {
    let path = match selector {
        MapSelector::Checksum(checksum) => format!("/api/v2/beatmaps/lookup?checksum={checksum}"),
        MapSelector::OfficialId(id) => format!("/api/v2/beatmaps/{id}"),
        MapSelector::Filename(filename) => {
            let mut url = reqwest::Url::parse("https://osu.ppy.sh/api/v2/beatmaps/lookup")
                .map_err(ApiError::internal)?;
            url.query_pairs_mut().append_pair("filename", filename);
            format!("{}?{}", url.path(), url.query().unwrap_or_default())
        }
    };
    let response = official_request(state, &path, true)
        .await
        .map_err(|error| {
            if error.0 == StatusCode::NOT_FOUND {
                ApiError(
                    StatusCode::NOT_FOUND,
                    "beatmap_not_found",
                    "No official beatmap matches the supplied identity.".into(),
                )
            } else {
                error
            }
        })?;
    let metadata: Value = serde_json::from_slice(
        &read_official_bytes(response, 1024 * 1024).await?,
    )
    .map_err(|_| {
        upstream_error(
            "official_api_failed",
            "The official beatmap metadata was invalid.",
        )
    })?;
    let invalid_identity = || {
        upstream_error(
            "official_identity_mismatch",
            "Official metadata and beatmap identity did not match.",
        )
    };
    let id = metadata["id"]
        .as_i64()
        .filter(|&id| id > 0)
        .ok_or_else(invalid_identity)?;
    let set_id = metadata["beatmapset_id"]
        .as_i64()
        .filter(|&id| id > 0)
        .ok_or_else(invalid_identity)?;
    if id > MAX_OFFICIAL_ID || set_id > MAX_OFFICIAL_ID {
        return Err(official_namespace_conflict());
    }
    let mode = metadata["mode_int"]
        .as_i64()
        .filter(|mode| (0..=3).contains(mode))
        .ok_or_else(invalid_identity)?;
    let checksum = metadata["checksum"]
        .as_str()
        .filter(|checksum| valid_checksum(checksum))
        .ok_or_else(invalid_identity)?
        .to_ascii_lowercase();
    if matches!(selector, MapSelector::OfficialId(requested) if *requested != id) {
        return Err(invalid_identity());
    }
    if matches!(selector, MapSelector::Checksum(requested) if requested != &checksum) {
        return Err(upstream_error(
            "checksum_mismatch",
            "The requested checksum and official metadata did not match.",
        ));
    }
    let status = match metadata["status"].as_str() {
        Some("ranked" | "approved") => "ranked",
        Some("loved") => "loved",
        Some("pending" | "wip" | "graveyard" | "qualified" | "unranked") => "unranked",
        _ => return Err(invalid_identity()),
    };
    let response = official_request(state, &format!("/osu/{id}"), false)
        .await
        .map_err(|error| {
            if error.0 == StatusCode::NOT_FOUND {
                upstream_error(
                    "official_api_failed",
                    "The official beatmap file was unavailable.",
                )
            } else {
                error
            }
        })?;
    let bytes = read_official_bytes(response, MAX_OSU_BYTES).await?;
    let map = tokio::task::spawn_blocking(move || parse_map(bytes))
        .await
        .map_err(ApiError::internal)?
        .map_err(|_| {
            upstream_error(
                "official_api_failed",
                "The official beatmap file was invalid.",
            )
        })?;
    if checksum != map.md5 {
        return Err(upstream_error(
            "checksum_mismatch",
            "Official metadata and beatmap checksum did not match.",
        ));
    }
    if i64::from(map.mode) != mode
        || map.official_id != Some(id)
        || map.official_set_id != Some(set_id)
    {
        return Err(invalid_identity());
    }
    Ok(OfficialMap {
        id,
        set_id,
        status,
        map,
    })
}

fn same_official_identity(value: &Value, official: &OfficialMap) -> bool {
    value["source"] == "official"
        && value["official_id"].as_i64() == Some(official.id)
        && value["official_set_id"].as_i64() == Some(official.set_id)
        && value["md5"].as_str() == Some(official.map.md5.as_str())
        && value["mode"].as_i64() == Some(i64::from(official.map.mode))
}

async fn insert_official_map(
    state: &AppState,
    user_id: i64,
    official: OfficialMap,
    website: Option<&auth::User>,
    selector: &MapSelector,
) -> ApiResult<Value> {
    // A new path is published only by its own successful insert, so concurrent
    // discovery cannot replace bytes used by an existing map, score or replay.
    tokio::fs::create_dir_all(&state.maps_root).await?;
    let file_id = Uuid::new_v4();
    let path = format!("official-{}-{file_id}.osu", official.map.md5);
    let temporary = state.maps_root.join(format!("official-{file_id}.part"));
    let mut temporary_owned = false;
    let write = async {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await?;
        temporary_owned = true;
        file.write_all(&official.map.bytes).await?;
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&temporary, state.maps_root.join(&path)).await
    }
    .await;
    if let Err(error) = write {
        if temporary_owned {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        return Err(error.into());
    }
    let mut may_be_published = false;
    let outcome = async {
        let mut tx = state.db.begin().await?;
        let actor = if let Some(snapshot) = website {
            let actor = auth::mutation_actor(&mut tx, snapshot, &[], auth::MutationScope::Permissions).await?;
            actor.require("beatmaps.import")?;
            Some(actor)
        } else {
            // Trusted native discovery has its own user-only authority policy.
            let active = sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR SHARE")
                .bind(user_id).fetch_optional(&mut *tx).await?;
            if active.is_none() { return Err(ApiError::unauthorized()); }
            None
        };
        let filename = if let MapSelector::Filename(filename) = selector {
            lock_filename_aliases(&mut tx, std::slice::from_ref(filename)).await?;
            if let Some(existing) = stored_map(&mut *tx, selector).await?
                && !same_official_identity(&existing, &official) {
                return Err(filename_conflict());
            }
            Some(filename)
        } else { None };
        let map = &official.map;
        let inserted = sqlx::query_scalar::<_, i64>("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'official',$10,$11,$12,$13,$14,$15) ON CONFLICT DO NOTHING RETURNING id")
            .bind(official.id).bind(official.set_id).bind(user_id).bind(&map.title).bind(&map.artist).bind(&map.creator).bind(&map.version).bind(map.mode).bind(official.status).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(&map.md5).bind(&path).fetch_optional(&mut *tx).await?;
        if let Some(id) = inserted {
            sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.resolve','beatmap',$2,$3)")
                .bind(user_id).bind(id.to_string()).bind(json!({"official_id":official.id,"md5":map.md5})).execute(&mut *tx).await?;
        }
        let value = stored_map(&mut *tx, &MapSelector::Checksum(map.md5.clone())).await?;
        // official_id is unique in this schema. Never mutate an older checksum
        // into a new official version: existing scores/replays depend on it.
        let value = value.ok_or_else(|| ApiError(StatusCode::CONFLICT, "official_version_conflict", "A different checksum for this official beatmap is already stored.".into()))?;
        if let Some(filename) = filename {
            if !same_official_identity(&value, &official) { return Err(filename_conflict()); }
            let id = value["id"].as_i64().ok_or_else(|| ApiError::internal("missing resolved map identity"))?;
            sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
                .bind(filename).bind(id).execute(&mut *tx).await?;
        }
        if let Some(actor) = &actor {
            audit_manual_import(&mut tx, actor, selector, &value).await?;
            auth::ensure_mutation_actor(&mut tx, actor).await?;
        }
        // A transport error during COMMIT has an unknown outcome. Retain bytes
        // once commit is attempted for our insert, rather than risk a dangling row.
        may_be_published = inserted.is_some();
        if let Err(error) = tx.commit().await {
            if commit_definitely_rolled_back(&error) { may_be_published = false; }
            return Err(error.into());
        }
        Ok(value)
    }.await;
    if !may_be_published
        && let Err(error) = tokio::fs::remove_file(state.maps_root.join(&path)).await
    {
        tracing::warn!(%error, "could not remove unused official map file");
    }
    outcome
}

async fn resolve_map(state: &AppState, user_id: i64, selector: MapSelector) -> ApiResult<Value> {
    resolve_map_as(state, user_id, selector, None).await
}

async fn audit_manual_import(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &auth::MutationActor,
    selector: &MapSelector,
    value: &Value,
) -> ApiResult<()> {
    let MapSelector::OfficialId(official_id) = selector else {
        return Err(ApiError::internal("invalid manual import selector"));
    };
    let id = value["id"]
        .as_i64()
        .ok_or_else(|| ApiError::internal("missing resolved map id"))?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.import','beatmap',$2,$3)")
        .bind(actor.id).bind(id.to_string()).bind(json!({"official_id":official_id})).execute(&mut **tx).await?;
    Ok(())
}

async fn finish_stored_resolution(
    state: &AppState,
    selector: &MapSelector,
    website: Option<&auth::User>,
    value: Value,
) -> ApiResult<Value> {
    let Some(snapshot) = website else {
        return Ok(value);
    };
    let mut tx = state.db.begin().await?;
    let actor =
        auth::mutation_actor(&mut tx, snapshot, &[], auth::MutationScope::Permissions).await?;
    actor.require("beatmaps.import")?;
    let id = value["id"]
        .as_i64()
        .ok_or_else(|| ApiError::internal("missing resolved map id"))?;
    sqlx::query("SELECT id FROM beatmaps WHERE id=$1 FOR SHARE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(ApiError::missing)?;
    let value = stored_map(&mut *tx, selector)
        .await?
        .ok_or_else(ApiError::missing)?;
    audit_manual_import(&mut tx, &actor, selector, &value).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(value)
}

async fn resolve_map_as(
    state: &AppState,
    user_id: i64,
    selector: MapSelector,
    website: Option<&auth::User>,
) -> ApiResult<Value> {
    if let Some(value) = stored_map(&state.db, &selector).await? {
        return finish_stored_resolution(state, &selector, website, value).await;
    }
    let (flight, slots) = {
        let mut official = state.official.lock().await;
        official
            .resolutions
            .retain(|_, cached| cached.until > Instant::now());
        if let Some(error) = official
            .resolutions
            .get(&selector)
            .and_then(CachedFailure::error)
        {
            return Err(error);
        }
        official
            .flights
            .retain(|_, flight| flight.strong_count() > 0);
        let flight = if let Some(flight) = official.flights.get(&selector).and_then(Weak::upgrade) {
            flight
        } else {
            if official.flights.len() >= MAX_RESOLUTION_KEYS {
                return Err(ApiError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "official_resolution_busy",
                    "Official beatmap discovery is busy. Please try shortly.".into(),
                ));
            }
            let flight = Arc::new(Mutex::new(()));
            official
                .flights
                .insert(selector.clone(), Arc::downgrade(&flight));
            flight
        };
        (flight, official.slots.clone())
    };
    let _flight = flight.lock().await;
    require_active_user(state, user_id).await?;
    // Repeat after waiting: another caller, manual upload or moderator may have
    // supplied this map. Local authority always precedes negative caches.
    if let Some(value) = stored_map(&state.db, &selector).await? {
        return finish_stored_resolution(state, &selector, website, value).await;
    }
    {
        let official = state.official.lock().await;
        if let Some(error) = official
            .resolutions
            .get(&selector)
            .and_then(CachedFailure::error)
        {
            return Err(error);
        }
    }
    let _permit = slots.try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "official_resolution_busy",
            "Official beatmap discovery is busy. Please try shortly.".into(),
        )
    })?;
    let result = match fetch_official_map(state, &selector).await {
        Ok(map) => insert_official_map(state, user_id, map, website, &selector).await,
        Err(error) => Err(error),
    };
    if let Err(error) = &result
        && matches!(
            error.0,
            StatusCode::NOT_FOUND
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::CONFLICT
        )
    {
        let duration = if error.0 == StatusCode::NOT_FOUND {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(5)
        };
        let mut official = state.official.lock().await;
        if official.resolutions.len() >= MAX_RESOLUTION_KEYS
            && let Some(oldest) = official
                .resolutions
                .iter()
                .min_by_key(|(_, cached)| cached.until)
                .map(|(key, _)| key.clone())
        {
            official.resolutions.remove(&oldest);
        }
        let mut cached = CachedFailure::new(error, duration);
        if error.0 == StatusCode::TOO_MANY_REQUESTS
            && let Some(failure) = &official.failure
            && failure.status == StatusCode::TOO_MANY_REQUESTS
        {
            cached.until = failure.until;
        }
        official.resolutions.insert(selector, cached);
    }
    result
}
#[derive(Deserialize)]
pub struct Import {
    official_id: i64,
}
// Manual import remains staff-only discovery. Already stored content is
// authoritative: importing again does not promote local content, attach a new
// official ID, or refresh a status that a moderator may have chosen.
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
    if input.official_id > MAX_OFFICIAL_ID {
        return Err(official_namespace_conflict());
    }
    let resolved = resolve_map_as(
        &state,
        user.id,
        MapSelector::OfficialId(input.official_id),
        Some(&user),
    )
    .await?;
    let id = resolved["id"]
        .as_i64()
        .ok_or_else(|| ApiError::internal("missing resolved map id"))?;
    Ok(Json(get(&state, id).await?))
}

#[cfg(test)]
#[path = "map_mutation_tests.rs"]
pub(crate) mod map_mutation_tests;

#[cfg(test)]
#[path = "map_package_tests.rs"]
mod map_package_tests;

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
    fn archive_rejects_normalized_duplicate_paths_with_different_contents() {
        let first = b"first difficulty";
        let second = b"second difficulty";
        assert_ne!(md5::compute(first), md5::compute(second));
        for names in [
            ["Maps\\Easy.osu", "maps/easy.OSU"],
            ["maps/./Easy.osu", "maps//easy.osu"],
            ["Maps/Éasy.osu", "maps/éasy.OSU"],
        ] {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, bytes) in names.into_iter().zip([first.as_slice(), second.as_slice()]) {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, bytes).unwrap();
            }
            let bytes = zip.finish().unwrap().into_inner();
            let error = unpack_archive(&bytes).unwrap_err();
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
            assert!(error.2.contains("duplicate file paths"));
        }
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
