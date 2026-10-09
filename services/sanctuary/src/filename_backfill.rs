//! Recover exact aliases from the retained original local archive. This action
//! never reconstructs names from metadata or changes package content/identity.
use super::{availability, lock_filename_aliases, lock_package_checksums, unpack_archive};
use crate::{ApiError, ApiResult, AppState, auth};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgRow};
use std::{
    collections::BTreeMap,
    path::{Component, Path as FsPath},
};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MEMBER_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Input {
    #[serde(default)]
    apply: bool,
}

fn conflict(code: &'static str, message: &'static str) -> ApiError {
    ApiError(StatusCode::CONFLICT, code, message.into())
}

fn manifest_mismatch() -> ApiError {
    conflict(
        "filename_backfill_manifest_mismatch",
        "The original archive and complete stored package membership do not match.",
    )
}

fn unsafe_file() -> ApiError {
    conflict(
        "filename_backfill_unsafe_file",
        "A retained package file has an unsafe path or is not a regular file.",
    )
}

fn missing_file() -> ApiError {
    conflict(
        "filename_backfill_missing_file",
        "An original retained package file is missing or unreadable.",
    )
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Junctions and other reparse points must also be rejected, including
        // links whose destination happens to remain inside the maps root.
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Check every component before opening; canonical containment also rejects
/// Windows junctions. Paths are portable relative storage keys, not host paths.
async fn retained_bytes(root: &FsPath, stored: &str, maximum: u64) -> ApiResult<Vec<u8>> {
    let relative = FsPath::new(stored);
    if stored.is_empty()
        || stored.contains(['\\', ':', '\0'])
        || stored
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(unsafe_file());
    }
    let root_metadata = tokio::fs::symlink_metadata(root)
        .await
        .map_err(|_| missing_file())?;
    if is_link(&root_metadata) || !root_metadata.is_dir() {
        return Err(unsafe_file());
    }
    let canonical_root = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| missing_file())?;
    let mut path = root.to_owned();
    let parts = relative.components().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        path.push(part.as_os_str());
        let metadata = tokio::fs::symlink_metadata(&path)
            .await
            .map_err(|_| missing_file())?;
        if is_link(&metadata)
            || (index + 1 < parts.len() && !metadata.is_dir())
            || (index + 1 == parts.len() && !metadata.is_file())
        {
            return Err(unsafe_file());
        }
        let canonical = tokio::fs::canonicalize(&path)
            .await
            .map_err(|_| missing_file())?;
        if !canonical.starts_with(&canonical_root) {
            return Err(unsafe_file());
        }
    }
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| missing_file())?;
    let metadata = file.metadata().await.map_err(|_| missing_file())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(unsafe_file());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| missing_file())?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > maximum {
        return Err(unsafe_file());
    }
    Ok(bytes)
}

fn archive_members(bytes: &[u8]) -> ApiResult<BTreeMap<String, (String, Vec<u8>)>> {
    let entries = unpack_archive(bytes).map_err(|_| {
        conflict(
            "filename_backfill_malformed_archive",
            "The retained original archive cannot be safely decoded.",
        )
    })?;
    let mut members = BTreeMap::new();
    for (filename, bytes) in entries {
        let checksum = format!("{:x}", md5::compute(&bytes));
        if members.insert(checksum, (filename, bytes)).is_some() {
            return Err(manifest_mismatch());
        }
    }
    Ok(members)
}

fn matching_identity(row: &PgRow, package: &PgRow, id: Uuid) -> bool {
    row.get::<String, _>("source") == "local"
        && row.get::<Option<Uuid>, _>("local_package_id") == Some(id)
        && row.get::<Option<i64>, _>("uploaded_by") == package.get::<Option<i64>, _>("uploaded_by")
        && row.get::<Option<String>, _>("archive_path")
            == package.get::<Option<String>, _>("archive_path")
        && row.get::<i64, _>("native_set_id") == package.get::<i64, _>("native_set_id")
        && row.get::<Option<i64>, _>("official_id").is_none()
        && row.get::<Option<i64>, _>("official_set_id").is_none()
        && row.get::<Option<i64>, _>("local_native_id") == Some(row.get::<i64, _>("native_id"))
        && (2_000_000_000..=i64::from(i32::MAX)).contains(&row.get::<i64, _>("native_id"))
}

pub(crate) async fn backfill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(input): Json<Input>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    user.require("beatmaps.moderate")?;
    let _permit = state.upload_permit()?;
    // Discover only the immutable set key before transaction resource locks.
    let set_id: i64 = sqlx::query_scalar("SELECT native_set_id FROM beatmap_packages WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                "filename_backfill_package_not_found",
                "The local package was not found.".into(),
            )
        })?;
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Permissions).await?;
    actor.require("beatmaps.moderate")?;
    availability::lock_exclusive_set(&mut tx, set_id).await?;
    availability::lock_active_set(&mut tx, set_id).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    let initial = sqlx::query("SELECT * FROM beatmap_packages WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(manifest_mismatch)?;
    if initial.get::<String, _>("kind") != "osz" {
        return Err(conflict(
            "filename_backfill_unsupported_package",
            "A standalone .osu package does not retain its original filename.",
        ));
    }
    let path = initial
        .get::<Option<String>, _>("archive_path")
        .ok_or_else(manifest_mismatch)?;
    let archive = retained_bytes(&state.maps_root, &path, MAX_ARCHIVE_BYTES).await?;
    let (members, sha256, archive_length) = tokio::task::spawn_blocking(move || {
        let sha256 = format!("{:x}", Sha256::digest(&archive));
        Ok::<_, ApiError>((archive_members(&archive)?, sha256, archive.len() as i64))
    })
    .await
    .map_err(ApiError::internal)??;
    let manifest = members.keys().cloned().collect::<Vec<_>>();
    let filenames = members
        .values()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    lock_filename_aliases(&mut tx, &filenames).await?;
    lock_package_checksums(&mut tx, &manifest).await?;
    let rows =
        sqlx::query("SELECT * FROM beatmaps WHERE local_package_id=$1 ORDER BY md5 FOR UPDATE")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let package = sqlx::query("SELECT * FROM beatmap_packages WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(manifest_mismatch)?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    if package.get::<i64, _>("native_set_id") != set_id
        || package.get::<String, _>("kind") != "osz"
        || package.get::<Option<String>, _>("archive_path").as_deref() != Some(path.as_str())
        || package.get::<Vec<String>, _>("member_md5") != manifest
        || rows
            .iter()
            .map(|row| row.get::<String, _>("md5"))
            .collect::<Vec<_>>()
            != manifest
        || rows.iter().any(|row| !matching_identity(row, &package, id))
    {
        return Err(manifest_mismatch());
    }
    match (
        package.get::<Option<String>, _>("sha256"),
        package.get::<Option<i64>, _>("byte_length"),
    ) {
        (Some(expected), Some(length)) if expected == sha256 && length == archive_length => {}
        (None, None) if package.get::<bool, _>("legacy") => {}
        _ => {
            return Err(conflict(
                "filename_backfill_fingerprint_mismatch",
                "The retained archive differs from its established original fingerprint.",
            ));
        }
    }
    for row in &rows {
        let checksum = row.get::<String, _>("md5");
        let stored = retained_bytes(
            &state.maps_root,
            &row.get::<String, _>("osu_path"),
            MAX_MEMBER_BYTES,
        )
        .await?;
        if stored != members[&checksum].1 {
            return Err(conflict(
                "filename_backfill_member_mismatch",
                "A retained difficulty differs from the exact original archive member.",
            ));
        }
    }
    let mut existing = 0usize;
    let mut inserted = 0usize;
    let mut ambiguous = 0usize;
    for row in &rows {
        let map_id = row.get::<i64, _>("id");
        let checksum = row.get::<String, _>("md5");
        let filename = &members[&checksum].0;
        if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM beatmap_filename_aliases WHERE filename=$1 AND beatmap_id=$2)")
            .bind(filename).bind(map_id).fetch_one(&mut *tx).await? {
            existing += 1;
        }
        // Include the verified pending associations when previewing ambiguity.
        let other: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM beatmap_filename_aliases WHERE filename=$1 AND beatmap_id<>$2)")
            .bind(filename).bind(map_id).fetch_one(&mut *tx).await?;
        if other
            || members
                .values()
                .filter(|(name, _)| name == filename)
                .count()
                > 1
        {
            ambiguous += 1;
        }
        if input.apply {
            inserted += sqlx::query("INSERT INTO beatmap_filename_aliases(filename,beatmap_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
                .bind(filename).bind(map_id).execute(&mut *tx).await?.rows_affected() as usize;
        }
    }
    let report: Value = json!({"package_id":id,"apply":input.apply,"difficulty_count":rows.len(),
        "verified_aliases":members.len(),"existing_aliases":existing,"missing_aliases":members.len()-existing,
        "inserted_aliases":inserted,"ambiguous_aliases":ambiguous});
    if inserted > 0 {
        sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'beatmaps.filename_aliases.backfill','beatmap_package',$2,$3)")
            .bind(actor.id).bind(id.to_string()).bind(&report).execute(&mut *tx).await?;
    }
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    tx.commit().await?;
    Ok(Json(report))
}

#[cfg(test)]
#[path = "filename_backfill_tests.rs"]
mod filename_backfill_tests;
