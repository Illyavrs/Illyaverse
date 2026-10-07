//! Original official-set ingestion from an operator-provided complete archive.
//! Metadata authenticates difficulty identities, not the provenance of assets.
use super::*;
use sqlx::postgres::PgRow;

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

fn archive_invalid() -> ApiError {
    ApiError(
        StatusCode::BAD_REQUEST,
        "official_archive_invalid",
        "Supply one valid .osz archive and a positive official set ID.".into(),
    )
}
fn archive_too_large() -> ApiError {
    ApiError(
        StatusCode::PAYLOAD_TOO_LARGE,
        "official_archive_too_large",
        "The archive must be at most 64 MiB.".into(),
    )
}
fn archive_conflict() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "official_archive_conflict",
        "Existing content conflicts with the verified official archive.".into(),
    )
}
fn manifest_mismatch() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "official_archive_manifest_mismatch",
        "The archive must contain the complete original difficulty set matching official metadata."
            .into(),
    )
}
fn set_version_conflict() -> ApiError {
    ApiError(
        StatusCode::CONFLICT,
        "official_set_version_conflict",
        "A different version or member of this official set is already stored.".into(),
    )
}
fn invalid_metadata() -> ApiError {
    upstream_error(
        "official_identity_mismatch",
        "The official set metadata was invalid.",
    )
}
fn multipart_error(error: axum::extract::multipart::MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        archive_too_large()
    } else {
        archive_invalid()
    }
}

pub(super) async fn lock_set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    set_id: i64,
) -> ApiResult<()> {
    // All official single-map and archive publishers take this namespace after
    // authority and before filename, checksum or row resources.
    let key = i32::try_from(set_id).map_err(|_| official_namespace_conflict())?;
    sqlx::query("SELECT pg_advisory_xact_lock($1,$2)")
        .bind(0x494c4f53_i32)
        .bind(key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn require_published_member(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    official: &OfficialMap,
) -> ApiResult<()> {
    let permitted: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM official_beatmapset_archives WHERE official_set_id=$1) OR EXISTS(SELECT 1 FROM official_beatmapset_archives a JOIN official_beatmapset_archive_members m ON m.archive_id=a.id JOIN beatmaps b ON b.id=m.beatmap_id WHERE a.official_set_id=$1 AND b.source='official' AND b.official_id=$2 AND b.md5=$3 AND b.mode=$4)")
        .bind(official.set_id).bind(official.id).bind(&official.map.md5).bind(official.map.mode).fetch_one(&mut **tx).await?;
    if permitted {
        Ok(())
    } else {
        Err(set_version_conflict())
    }
}

struct ArchiveInput {
    set_id: i64,
    bytes: Vec<u8>,
    sha256: String,
    maps: Vec<UploadMap>,
    manifest: Vec<String>,
}

async fn read_input(mut multipart: Multipart) -> ApiResult<ArchiveInput> {
    let mut set_id = None;
    let mut bytes = None;
    while let Some(mut field) = multipart.next_field().await.map_err(multipart_error)? {
        let is_file = match field.name() {
            Some("official_set_id") if set_id.is_none() => false,
            Some("file") if bytes.is_none() => true,
            _ => return Err(archive_invalid()),
        };
        if is_file
            && !field
                .file_name()
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(".osz"))
        {
            return Err(archive_invalid());
        }
        let mut value = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
            let limit = if is_file { MAX_ARCHIVE_BYTES } else { 20 };
            if chunk.len() > limit - value.len() {
                return Err(if is_file {
                    archive_too_large()
                } else {
                    archive_invalid()
                });
            }
            value.extend_from_slice(&chunk);
        }
        if is_file {
            bytes = Some(value);
        } else {
            set_id = Some(
                std::str::from_utf8(&value)
                    .ok()
                    .and_then(|value| value.parse::<i64>().ok())
                    .filter(|id| *id > 0)
                    .ok_or_else(archive_invalid)?,
            );
        }
    }
    let set_id = set_id.ok_or_else(archive_invalid)?;
    if set_id > MAX_OFFICIAL_ID {
        return Err(official_namespace_conflict());
    }
    let bytes = bytes
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(archive_invalid)?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    let parsing = bytes.clone();
    let mut maps = tokio::task::spawn_blocking(move || -> ApiResult<Vec<UploadMap>> {
        unpack_archive_with_policy(&parsing, true)
            .map_err(|_| archive_invalid())?
            .into_iter()
            .map(|(filename, bytes)| {
                Ok(UploadMap {
                    filename,
                    map: parse_map(bytes).map_err(|_| archive_invalid())?,
                })
            })
            .collect()
    })
    .await
    .map_err(ApiError::internal)??;
    maps.sort_unstable_by(|left, right| left.map.md5.cmp(&right.map.md5));
    let mut ids = HashSet::new();
    for input in &maps {
        let map = &input.map;
        if !map
            .official_id
            .is_some_and(|id| (1..=MAX_OFFICIAL_ID).contains(&id))
            || map.official_set_id != Some(set_id)
            || !ids.insert(map.official_id)
        {
            return Err(manifest_mismatch());
        }
    }
    if maps
        .windows(2)
        .any(|pair| pair[0].map.md5 == pair[1].map.md5)
    {
        return Err(archive_invalid());
    }
    let manifest = maps.iter().map(|input| input.map.md5.clone()).collect();
    Ok(ArchiveInput {
        set_id,
        bytes,
        sha256,
        maps,
        manifest,
    })
}

struct MemberMetadata {
    checksum: String,
    mode: i16,
    status: &'static str,
}
fn metadata_members(metadata: &Value, set_id: i64) -> ApiResult<HashMap<i64, MemberMetadata>> {
    if metadata["id"].as_i64() != Some(set_id) {
        return Err(invalid_metadata());
    }
    let maps = metadata["beatmaps"]
        .as_array()
        .filter(|maps| !maps.is_empty() && maps.len() <= 512)
        .ok_or_else(invalid_metadata)?;
    let mut members = HashMap::new();
    let mut hashes = HashSet::new();
    for map in maps {
        // The normal beatmaps relation is authoritative. Converts are never
        // original archive members; older minimal metadata may omit this flag.
        if map.get("convert").is_some_and(|value| value != false)
            || map.get("deleted_at").is_some_and(|value| !value.is_null())
        {
            return Err(invalid_metadata());
        }
        let id = map["id"]
            .as_i64()
            .filter(|id| (1..=MAX_OFFICIAL_ID).contains(id))
            .ok_or_else(invalid_metadata)?;
        if map["beatmapset_id"].as_i64() != Some(set_id) {
            return Err(invalid_metadata());
        }
        let checksum = map["checksum"]
            .as_str()
            .filter(|hash| valid_checksum(hash))
            .ok_or_else(invalid_metadata)?
            .to_ascii_lowercase();
        let mode = map["mode_int"]
            .as_i64()
            .filter(|mode| (0..=3).contains(mode))
            .ok_or_else(invalid_metadata)? as i16;
        let status = match map["status"].as_str() {
            Some("ranked" | "approved") => "ranked",
            Some("loved") => "loved",
            Some("pending" | "wip" | "graveyard" | "qualified" | "unranked") => "unranked",
            _ => return Err(invalid_metadata()),
        };
        if !hashes.insert(checksum.clone())
            || members
                .insert(
                    id,
                    MemberMetadata {
                        checksum,
                        mode,
                        status,
                    },
                )
                .is_some()
        {
            return Err(invalid_metadata());
        }
    }
    Ok(members)
}
fn verify_manifest(input: &ArchiveInput, members: &HashMap<i64, MemberMetadata>) -> ApiResult<()> {
    if input.maps.len() != members.len()
        || input.maps.iter().any(|input| {
            members
                .get(&input.map.official_id.unwrap())
                .is_none_or(|member| {
                    member.checksum != input.map.md5 || member.mode != input.map.mode
                })
        })
    {
        return Err(manifest_mismatch());
    }
    Ok(())
}

async fn archive_flight(
    state: &AppState,
    set_id: i64,
) -> ApiResult<(
    tokio::sync::OwnedMutexGuard<()>,
    Arc<tokio::sync::Semaphore>,
)> {
    let selector = MapSelector::OfficialSet(set_id);
    let (flight, slots) = {
        let mut official = state.official.lock().await;
        official
            .resolutions
            .retain(|_, cached| cached.until > Instant::now());
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
    Ok((flight.lock_owned().await, slots))
}

async fn fetch_members(state: &AppState, set_id: i64) -> ApiResult<HashMap<i64, MemberMetadata>> {
    let selector = MapSelector::OfficialSet(set_id);
    if let Some(error) = state
        .official
        .lock()
        .await
        .resolutions
        .get(&selector)
        .and_then(CachedFailure::error)
    {
        return Err(error);
    }
    let result = async {
        let response = official_request(state, &format!("/api/v2/beatmapsets/{set_id}"), true)
            .await
            .map_err(|error| {
                if error.0 == StatusCode::NOT_FOUND {
                    ApiError(
                        StatusCode::NOT_FOUND,
                        "beatmapset_not_found",
                        "No official beatmap set matches this ID.".into(),
                    )
                } else {
                    error
                }
            })?;
        let metadata: Value =
            serde_json::from_slice(&read_official_bytes(response, 4 * 1024 * 1024).await?)
                .map_err(|_| {
                    upstream_error(
                        "official_api_failed",
                        "The official set response was invalid.",
                    )
                })?;
        metadata_members(&metadata, set_id)
    }
    .await;
    if let Err(error) = &result {
        let duration = if error.0 == StatusCode::NOT_FOUND {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(5)
        };
        let mut official = state.official.lock().await;
        if official.resolutions.len() >= MAX_RESOLUTION_KEYS
            && let Some(key) = official
                .resolutions
                .iter()
                .min_by_key(|(_, cached)| cached.until)
                .map(|(key, _)| key.clone())
        {
            official.resolutions.remove(&key);
        }
        let mut cached = CachedFailure::new(error, duration);
        if error.0 == StatusCode::TOO_MANY_REQUESTS
            && let Some(failure) = &official.failure
        {
            cached.until = failure.until;
        }
        official.resolutions.insert(selector, cached);
    }
    result
}

#[cfg(test)]
pub(super) async fn metadata_member_count(state: &AppState, set_id: i64) -> ApiResult<usize> {
    fetch_members(state, set_id)
        .await
        .map(|members| members.len())
}

fn row_matches(row: &PgRow, map: &ParsedMap) -> bool {
    row.get::<String, _>("source") == "official"
        && row.get::<Option<i64>, _>("official_id") == map.official_id
        && row.get::<Option<i64>, _>("official_set_id") == map.official_set_id
        && row.get::<String, _>("md5") == map.md5
        && row.get::<i16, _>("mode") == map.mode
}
async fn verify_retained_bytes(
    state: &AppState,
    path: &str,
    expected: &[u8],
    bound: usize,
) -> ApiResult<()> {
    if expected.len() > bound
        || std::path::Path::new(path).is_absolute()
        || path.contains("..")
        || path.contains(':')
        || path.contains('\\')
    {
        return Err(archive_conflict());
    }
    let path = state.maps_root.join(path);
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| archive_conflict())?;
    if !metadata.is_file() || metadata.len() != expected.len() as u64 {
        return Err(archive_conflict());
    }
    use tokio::io::AsyncReadExt;
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|_| archive_conflict())?;
    let mut bytes = Vec::new();
    file.take(bound as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| archive_conflict())?;
    if bytes != expected {
        return Err(archive_conflict());
    }
    Ok(())
}

async fn retry_in_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &ArchiveInput,
) -> ApiResult<Option<Vec<i64>>> {
    let archive = sqlx::query(
        "SELECT * FROM official_beatmapset_archives WHERE official_set_id=$1 FOR SHARE",
    )
    .bind(input.set_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(archive) = archive else {
        return Ok(None);
    };
    if archive.get::<String, _>("sha256") != input.sha256
        || archive.get::<i64, _>("byte_length") != input.bytes.len() as i64
        || archive.get::<Vec<String>, _>("member_md5") != input.manifest
    {
        return Err(set_version_conflict());
    }
    let rows = sqlx::query("SELECT b.* FROM beatmaps b JOIN official_beatmapset_archive_members m ON m.beatmap_id=b.id WHERE m.archive_id=$1 ORDER BY b.md5 FOR SHARE OF b")
        .bind(archive.get::<Uuid, _>("id")).fetch_all(&mut **tx).await?;
    if rows.len() != input.maps.len()
        || rows
            .iter()
            .zip(&input.maps)
            .any(|(row, input)| !row_matches(row, &input.map))
    {
        return Err(archive_conflict());
    }
    for (row, upload) in rows.iter().zip(&input.maps) {
        verify_retained_bytes(
            state,
            &row.get::<String, _>("osu_path"),
            &upload.map.bytes,
            MAX_OSU_BYTES,
        )
        .await?;
    }
    verify_retained_bytes(
        state,
        &archive.get::<String, _>("archive_path"),
        &input.bytes,
        MAX_ARCHIVE_BYTES,
    )
    .await?;
    Ok(Some(rows.iter().map(|row| row.get("id")).collect()))
}
async fn try_retry(
    state: &AppState,
    user: &auth::User,
    input: &ArchiveInput,
) -> ApiResult<Option<Vec<i64>>> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM official_beatmapset_archives WHERE official_set_id=$1)",
    )
    .bind(input.set_id)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Ok(None);
    }
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(&mut tx, user, &[], auth::MutationScope::Permissions).await?;
    actor.require("beatmaps.import")?;
    lock_set(&mut tx, input.set_id).await?;
    let ids = retry_in_tx(state, &mut tx, input).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(ids)
}

struct Publication {
    ids: Vec<i64>,
    idempotent: bool,
    new_hashes: HashSet<String>,
}
async fn commit_publication(
    commit: impl std::future::Future<Output = Result<(), sqlx::Error>>,
    retain_files: &mut bool,
) -> ApiResult<()> {
    // Set retention before COMMIT is polled. An unknown transport outcome must
    // never cause cleanup of bytes that a committed archive may reference.
    *retain_files = true;
    if let Err(error) = commit.await {
        *retain_files = !commit_definitely_rolled_back(&error);
        return Err(error.into());
    }
    Ok(())
}
async fn publish(
    state: &AppState,
    user: &auth::User,
    input: &ArchiveInput,
    metadata: &HashMap<i64, MemberMetadata>,
    package_id: Uuid,
    archive_path: &str,
    retain_files: &mut bool,
) -> ApiResult<Publication> {
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(&mut tx, user, &[], auth::MutationScope::Permissions).await?;
    actor.require("beatmaps.import")?;
    lock_set(&mut tx, input.set_id).await?;
    if let Some(ids) = retry_in_tx(state, &mut tx, input).await? {
        auth::ensure_mutation_actor(&mut tx, &actor).await?;
        tx.commit().await?;
        return Ok(Publication {
            ids,
            idempotent: true,
            new_hashes: HashSet::new(),
        });
    }
    let filenames = input
        .maps
        .iter()
        .map(|input| input.filename.clone())
        .collect::<Vec<_>>();
    lock_filename_aliases(&mut tx, &filenames).await?;
    lock_package_checksums(&mut tx, &input.manifest).await?;
    let official_ids = input
        .maps
        .iter()
        .map(|input| input.map.official_id.unwrap())
        .collect::<Vec<_>>();
    let existing = sqlx::query("SELECT * FROM beatmaps WHERE (native_set_id=$1 AND official_set_id=$1) OR official_id=ANY($2) OR md5=ANY($3) ORDER BY md5 FOR UPDATE")
        .bind(input.set_id).bind(&official_ids).bind(&input.manifest).fetch_all(&mut *tx).await?;
    for row in &existing {
        let matching = input
            .maps
            .iter()
            .find(|input| input.map.official_id == row.get::<Option<i64>, _>("official_id"));
        if row.get::<Option<i64>, _>("official_set_id") == Some(input.set_id) && matching.is_none()
        {
            return Err(set_version_conflict());
        }
        if let Some(matching) = matching {
            if row.get::<String, _>("md5") != matching.map.md5 {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "official_version_conflict",
                    "A different checksum for this official beatmap is already stored.".into(),
                ));
            }
            if !row_matches(row, &matching.map) {
                return Err(archive_conflict());
            }
            verify_retained_bytes(
                state,
                &row.get::<String, _>("osu_path"),
                &matching.map.bytes,
                MAX_OSU_BYTES,
            )
            .await?;
            if let Some(path) = row.get::<Option<String>, _>("archive_path") {
                verify_retained_bytes(state, &path, &input.bytes, MAX_ARCHIVE_BYTES).await?;
            }
        } else {
            return Err(archive_conflict());
        }
    }
    let mut ids = Vec::new();
    let mut new_hashes = HashSet::new();
    for upload in &input.maps {
        let map = &upload.map;
        let existing_id = existing
            .iter()
            .find(|row| row.get::<String, _>("md5") == map.md5)
            .map(|row| row.get::<i64, _>("id"));
        let id = if let Some(id) = existing_id {
            id
        } else {
            let path = format!("official-package-{package_id}/{}.osu", map.md5);
            let id = sqlx::query_scalar::<_, i64>("INSERT INTO beatmaps(official_id,official_set_id,uploaded_by,title,artist,creator,version,mode,status,source,stars,bpm,length_seconds,max_combo,md5,osu_path) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'official',$10,$11,$12,$13,$14,$15) RETURNING id")
                .bind(map.official_id).bind(input.set_id).bind(actor.id).bind(&map.title).bind(&map.artist).bind(&map.creator).bind(&map.version).bind(map.mode).bind(metadata[&map.official_id.unwrap()].status).bind(map.stars).bind(map.bpm).bind(map.length).bind(map.max_combo).bind(&map.md5).bind(path).fetch_one(&mut *tx).await.map_err(|error| if error.as_database_error().is_some_and(|error| error.is_unique_violation()) { archive_conflict() } else { error.into() })?;
            new_hashes.insert(map.md5.clone());
            id
        };
        ids.push(id);
        sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&upload.filename).bind(id).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO official_beatmapset_archives(official_set_id,id,archive_path,sha256,byte_length,member_md5,uploaded_by) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(input.set_id).bind(package_id).bind(archive_path).bind(&input.sha256).bind(input.bytes.len() as i64).bind(&input.manifest).bind(actor.id).execute(&mut *tx).await?;
    for id in &ids {
        sqlx::query(
            "INSERT INTO official_beatmapset_archive_members(archive_id,beatmap_id) VALUES($1,$2)",
        )
        .bind(package_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.import_archive','beatmap_archive',$2,$3)")
        .bind(actor.id).bind(package_id.to_string()).bind(json!({"official_set_id":input.set_id,"beatmap_ids":ids,"sha256":input.sha256})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    commit_publication(tx.commit(), retain_files).await?;
    Ok(Publication {
        ids,
        idempotent: false,
        new_hashes,
    })
}

async fn response(state: &AppState, ids: Vec<i64>, idempotent: bool) -> ApiResult {
    let mut items = Vec::new();
    for id in ids {
        items.push(get(state, id).await?);
    }
    let package = items
        .first()
        .and_then(|item| item.get("package"))
        .cloned()
        .ok_or_else(|| ApiError::internal("official archive has no public package"))?;
    Ok(Json(
        json!({"items":items,"package":package,"idempotent":idempotent}),
    ))
}

pub async fn import_archive(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Result<Multipart, axum::extract::multipart::MultipartRejection>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.import")?;
    let _upload = state.upload_permit()?;
    let input = read_input(multipart.map_err(|_| archive_invalid())?).await?;
    if let Some(ids) = try_retry(&state, &user, &input).await? {
        return response(&state, ids, true).await;
    }
    let package_id = Uuid::new_v4();
    let directory = state
        .maps_root
        .join(format!("official-package-{package_id}"));
    tokio::fs::create_dir(&directory).await?;
    let archive_path = format!("official-package-{package_id}/package.osz");
    let mut owned_files = Vec::new();
    let mut retain_files = false;
    let outcome: ApiResult<Publication> = async {
        write_owned_map_file(
            state.maps_root.join(&archive_path),
            &input.bytes,
            &mut owned_files,
        )
        .await?;
        for upload in &input.maps {
            write_owned_map_file(
                directory.join(format!("{}.osu", upload.map.md5)),
                &upload.map.bytes,
                &mut owned_files,
            )
            .await?;
        }
        let (_flight, slots) = archive_flight(&state, input.set_id).await?;
        if let Some(ids) = try_retry(&state, &user, &input).await? {
            return Ok(Publication {
                ids,
                idempotent: true,
                new_hashes: HashSet::new(),
            });
        }
        let _permit = slots.try_acquire_owned().map_err(|_| {
            ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "official_resolution_busy",
                "Official beatmap discovery is busy. Please try shortly.".into(),
            )
        })?;
        let metadata = fetch_members(&state, input.set_id).await?;
        verify_manifest(&input, &metadata)?;
        publish(
            &state,
            &user,
            &input,
            &metadata,
            package_id,
            &archive_path,
            &mut retain_files,
        )
        .await
    }
    .await;
    for path in owned_files {
        let unused = outcome.as_ref().is_ok_and(|published| {
            !published.idempotent
                && path != state.maps_root.join(&archive_path)
                && !published
                    .new_hashes
                    .iter()
                    .any(|hash| path == directory.join(format!("{hash}.osu")))
        });
        if !retain_files || unused {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
    if !retain_files {
        let _ = tokio::fs::remove_dir(&directory).await;
    }
    let published = outcome?;
    response(&state, published.ids, published.idempotent).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_requires_unique_complete_original_members_and_ignores_converts() {
        let map = json!({"id":1,"beatmapset_id":2,"checksum":"a".repeat(32),"mode_int":0,"status":"ranked"});
        let valid = json!({"id":2,"beatmaps":[map.clone()],"converts":[{"id":999}]});
        assert_eq!(metadata_members(&valid, 2).unwrap().len(), 1);
        let mut explicit_original = valid.clone();
        explicit_original["beatmaps"][0]["convert"] = json!(false);
        assert_eq!(metadata_members(&explicit_original, 2).unwrap().len(), 1);
        for invalid in [
            json!({"id":3,"beatmaps":[map.clone()]}),
            json!({"id":2,"beatmaps":[]}),
            json!({"id":2,"beatmaps":[map.clone(),map.clone()]}),
            json!({"id":2,"beatmaps":[{"id":1,"beatmapset_id":2,"checksum":"a".repeat(32),"mode_int":4,"status":"ranked"}]}),
        ] {
            assert_eq!(
                metadata_members(&invalid, 2).err().unwrap().1,
                "official_identity_mismatch"
            );
        }
        for flag in [json!(true), Value::Null, json!("false")] {
            let mut invalid = valid.clone();
            invalid["beatmaps"][0]["convert"] = flag;
            assert_eq!(
                metadata_members(&invalid, 2).err().unwrap().1,
                "official_identity_mismatch"
            );
        }
    }
    #[test]
    fn official_archive_policy_preserves_nested_assets_and_requires_top_level_maps() {
        fn archive(map_name: &str) -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for (name, bytes) in [
                (map_name, b"original map".as_slice()),
                ("assets/music.mp3", b"original asset".as_slice()),
            ] {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, bytes).unwrap();
            }
            zip.finish().unwrap().into_inner()
        }
        assert_eq!(
            unpack_archive_with_policy(&archive("Exact.OSU"), true).unwrap(),
            [("Exact.OSU".into(), b"original map".to_vec())]
        );
        for name in ["nested/Easy.osu", "./Easy.osu", "nested\\Easy.osu"] {
            let bytes = archive(name);
            assert!(unpack_archive_with_policy(&bytes, true).is_err());
            assert!(
                unpack_archive(&bytes).is_ok(),
                "local upload policy is unchanged"
            );
        }
    }
    #[tokio::test]
    async fn archive_commit_transport_uncertainty_retains_owned_content() {
        let mut retain = false;
        let error = commit_publication(
            async {
                Err(sqlx::Error::Io(std::io::Error::other(
                    "synthetic unknown COMMIT outcome",
                )))
            },
            &mut retain,
        )
        .await
        .unwrap_err();
        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(retain);
        retain = false;
        commit_publication(async { Ok(()) }, &mut retain)
            .await
            .unwrap();
        assert!(retain);
    }
}
