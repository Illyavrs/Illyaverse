//! Authenticated native discovery delegates official access to Sanctuary.
//! Known catalogue content is authoritative and never needs an upstream call.
use crate::common::{ApiError, AppState};
use axum::http::StatusCode;
use illya_domain::beatmap_filename::valid_beatmap_filename;
use resonance_protocol::stable::is_md5;
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;

pub(crate) const LOCAL_ID_MIN: i64 = 2_000_000_000;
pub(crate) const MAX_NATIVE_ID: i64 = i32::MAX as i64;

/// Both native clients store online beatmap and set identities in signed ints.
pub(crate) fn valid_id(id: i64) -> bool {
    (1..=MAX_NATIVE_ID).contains(&id)
}

pub(crate) fn require_id(id: i64) -> Result<(), ApiError> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ))
    }
}

/// Internal row keys never enter the native identity namespace.
pub(crate) fn row_ids(row: &sqlx::postgres::PgRow) -> Result<(i64, i64), ApiError> {
    let id: i64 = row.get("native_id");
    let set: i64 = row.get("native_set_id");
    if !valid_id(id) || !valid_id(set) {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_stored_native_identity",
        ));
    }
    Ok((id, set))
}

/// Local downloads belong to the immutable upload package. New official
/// archives have a complete verified set manifest; ordinary discovery has none.
pub(crate) async fn archive_path(state: &AppState, set: i64) -> Result<Option<String>, ApiError> {
    require_id(set)?;
    if set >= LOCAL_ID_MIN {
        return Ok(sqlx::query_scalar(
            "SELECT archive_path FROM beatmap_packages WHERE native_set_id=$1",
        )
        .bind(set)
        .fetch_optional(&state.pool)
        .await?
        .flatten());
    }
    if let Some(archive) = sqlx::query(
        "SELECT a.archive_path,a.member_md5,ARRAY(SELECT b.md5 FROM official_beatmapset_archive_members m JOIN beatmaps b ON b.id=m.beatmap_id WHERE m.archive_id=a.id ORDER BY b.md5) AS stored_manifest,ARRAY(SELECT b.md5 FROM beatmaps b WHERE b.native_set_id=a.official_set_id ORDER BY b.md5) AS set_manifest,NOT EXISTS(SELECT 1 FROM official_beatmapset_archive_members m JOIN beatmaps b ON b.id=m.beatmap_id WHERE m.archive_id=a.id AND (b.source<>'official' OR b.official_id IS NULL OR b.official_set_id IS DISTINCT FROM a.official_set_id OR b.native_set_id<>a.official_set_id)) AS valid_members FROM official_beatmapset_archives a WHERE a.official_set_id=$1",
    )
    .bind(set)
    .fetch_optional(&state.pool)
    .await?
    {
        let manifest: Vec<String> = archive.get("member_md5");
        if !archive.get::<bool, _>("valid_members")
            || manifest != archive.get::<Vec<String>, _>("stored_manifest")
            || manifest != archive.get::<Vec<String>, _>("set_manifest")
        {
            return Err(ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_stored_beatmap_archive",
            ));
        }
        return Ok(Some(archive.get("archive_path")));
    }
    // Older explicitly retained per-difficulty archives remain readable. They
    // do not gain the new verified-package identity or manifest claim.
    Ok(sqlx::query_scalar("SELECT archive_path FROM beatmaps WHERE native_set_id=$1 AND official_set_id IS NOT NULL AND archive_path IS NOT NULL ORDER BY id LIMIT 1")
        .bind(set).fetch_optional(&state.pool).await?)
}

/// One serializer is shared by map lookup and room playlists. Its argument is
/// an internal FK, while every identity it advertises is a canonical native ID.
pub(crate) async fn map_json(
    state: &AppState,
    internal: i64,
) -> Result<serde_json::Value, ApiError> {
    let row = sqlx::query("SELECT * FROM beatmaps WHERE id=$1")
        .bind(internal)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    let (id, set) = row_ids(&row)?;
    let status: String = row.get("status");
    let mode = row.get::<i16, _>("mode") as usize;
    let disabled = archive_path(state, set).await?.is_none();
    Ok(
        json!({"id":id,"beatmapset_id":set,"checksum":row.get::<String,_>("md5"),"version":row.get::<String,_>("version"),"mode":(["osu","taiko","fruits","mania"][mode]),"mode_int":mode,"status":status,"ranked":if status=="ranked"{1}else if status=="loved"{4}else{0},"difficulty_rating":row.get::<f64,_>("stars"),"bpm":row.get::<f64,_>("bpm"),"total_length":row.get::<i32,_>("length_seconds"),"hit_length":row.get::<i32,_>("length_seconds"),"max_combo":row.get::<i32,_>("max_combo"),"user_id":row.get::<Option<i64>,_>("uploaded_by").unwrap_or(0),"is_scoreable":true,"url":format!("/beatmaps/{internal}"),"beatmapset":{"id":set,"artist":row.get::<String,_>("artist"),"artist_unicode":row.get::<String,_>("artist"),"title":row.get::<String,_>("title"),"title_unicode":row.get::<String,_>("title"),"creator":row.get::<String,_>("creator"),"status":status,"user_id":row.get::<Option<i64>,_>("uploaded_by").unwrap_or(0),"covers":{"cover":"","cover@2x":"","card":"","card@2x":"","list":"","list@2x":"","slimcover":"","slimcover@2x":""},"preview_url":"","download_disabled":disabled,"video":false,"storyboard":false,"availability":{"download_disabled":disabled,"more_information":null},"beatmaps":[]}}),
    )
}

#[derive(Debug, Deserialize, PartialEq)]
struct Identity {
    id: i64,
    official_id: Option<i64>,
    official_set_id: Option<i64>,
    md5: String,
    status: String,
    source: String,
    mode: i16,
}

#[derive(Deserialize)]
struct ServiceError {
    error: ServiceErrorCode,
}

#[derive(Deserialize)]
struct ServiceErrorCode {
    code: String,
}

enum Selector<'a> {
    Checksum(&'a str),
    OfficialId(i64),
    Filename(&'a str),
}

impl Identity {
    fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        Self {
            id: row.get("id"),
            official_id: row.get("official_id"),
            official_set_id: row.get("official_set_id"),
            md5: row.get("md5"),
            status: row.get("status"),
            source: row.get("source"),
            mode: row.get("mode"),
        }
    }

    fn valid_for(&self, selector: &Selector<'_>) -> bool {
        self.id > 0
            && self
                .official_id
                .is_none_or(|id| (1..LOCAL_ID_MIN).contains(&id))
            && self
                .official_set_id
                .is_none_or(|id| (1..LOCAL_ID_MIN).contains(&id))
            && is_md5(&self.md5)
            && match selector {
                Selector::Checksum(checksum) => self.md5 == *checksum,
                Selector::OfficialId(id) => self.official_id == Some(*id),
                // The database alias binding is checked after the response.
                Selector::Filename(filename) => valid_beatmap_filename(filename),
            }
            && matches!(
                self.status.as_str(),
                "pending" | "ranked" | "loved" | "unranked"
            )
            && matches!(self.source.as_str(), "local" | "official")
            && (0..=3).contains(&self.mode)
    }
}

/// Resolve public difficulty IDs without allowing an imported row's internal
/// primary key to masquerade as a different official difficulty.
pub async fn known_id(state: &AppState, id: i64) -> Result<Option<i64>, ApiError> {
    require_id(id)?;
    Ok(
        sqlx::query_scalar("SELECT id FROM beatmaps WHERE native_id=$1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?,
    )
}

/// Resolve a public ID only after native bearer authentication. A local-only
/// map keeps its own public ID; an unknown ID is looked up as an official ID.
pub async fn resolve_id(state: &AppState, user_id: i64, id: i64) -> Result<Option<i64>, ApiError> {
    if user_id <= 0 || !valid_id(id) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ));
    }
    if let Some(known) = known_id(state, id).await? {
        return Ok(Some(known));
    }
    if id >= LOCAL_ID_MIN {
        return Ok(None);
    }
    discover(state, user_id, Selector::OfficialId(id)).await
}

/// Call only after authenticating the native player. A genuine missing map is
/// distinct from an unavailable official service or a mismatched map version.
pub async fn resolve_checksum(
    state: &AppState,
    user_id: i64,
    checksum: &str,
) -> Result<Option<i64>, ApiError> {
    if user_id <= 0 || !is_md5(checksum) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ));
    }
    let checksum = checksum.to_ascii_lowercase();
    if let Some(id) = sqlx::query_scalar("SELECT id FROM beatmaps WHERE md5=$1")
        .bind(&checksum)
        .fetch_optional(&state.pool)
        .await?
    {
        return Ok(Some(id));
    }
    discover(state, user_id, Selector::Checksum(&checksum)).await
}

/// Exact basename aliases are indexed data, never filesystem paths. Multiple
/// distinct difficulties with the same alias are intentionally ambiguous.
pub(crate) async fn known_filename(
    state: &AppState,
    filename: &str,
) -> Result<Option<i64>, ApiError> {
    if !valid_beatmap_filename(filename) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_filename",
        ));
    }
    let ids = sqlx::query_scalar::<_, i64>(
        "SELECT beatmap_id FROM beatmap_filename_aliases WHERE filename=$1 ORDER BY beatmap_id LIMIT 2",
    )
    .bind(filename)
    .fetch_all(&state.pool)
    .await?;
    if ids.len() > 1 {
        return Err(ApiError(StatusCode::CONFLICT, "beatmap_filename_ambiguous"));
    }
    Ok(ids.first().copied())
}

/// Only authenticated native routes may discover an absent exact filename.
/// Checksum/ID callers continue to use their stronger immutable selectors.
pub(crate) async fn resolve_filename(
    state: &AppState,
    user_id: i64,
    filename: &str,
) -> Result<Option<i64>, ApiError> {
    if user_id <= 0 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ));
    }
    if let Some(id) = known_filename(state, filename).await? {
        return Ok(Some(id));
    }
    discover(state, user_id, Selector::Filename(filename)).await
}

/// A supplied ID and checksum must identify the same immutable difficulty.
/// Reserved local misses are terminal, even when a checksum is also supplied.
pub(crate) async fn resolve_checksum_for_id(
    state: &AppState,
    user_id: i64,
    checksum: &str,
    native_id: i64,
) -> Result<Option<i64>, ApiError> {
    require_id(native_id)?;
    if user_id <= 0 || !is_md5(checksum) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ));
    }
    let known = known_id(state, native_id).await?;
    if let Some(internal) = known {
        let hash: String = sqlx::query_scalar("SELECT md5 FROM beatmaps WHERE id=$1")
            .bind(internal)
            .fetch_one(&state.pool)
            .await?;
        if !hash.eq_ignore_ascii_case(checksum) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "beatmap_id_mismatch"));
        }
        return Ok(Some(internal));
    }
    if native_id >= LOCAL_ID_MIN {
        return Ok(None);
    }
    let Some(internal) = resolve_checksum(state, user_id, checksum).await? else {
        return Ok(None);
    };
    let actual: i64 = sqlx::query_scalar("SELECT native_id FROM beatmaps WHERE id=$1")
        .bind(internal)
        .fetch_one(&state.pool)
        .await?;
    if actual != native_id {
        return Err(ApiError(StatusCode::BAD_REQUEST, "beatmap_id_mismatch"));
    }
    Ok(Some(internal))
}

async fn discover(
    state: &AppState,
    user_id: i64,
    selector: Selector<'_>,
) -> Result<Option<i64>, ApiError> {
    if state.service_key.len() < 32 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "map_resolution_not_configured",
        ));
    }
    let input = match selector {
        Selector::Checksum(checksum) => json!({"user_id":user_id,"checksum":checksum}),
        Selector::OfficialId(id) => json!({"user_id":user_id,"official_id":id}),
        Selector::Filename(filename) => json!({"user_id":user_id,"filename":filename}),
    };
    let response = state
        .http
        .post(format!(
            "{}/api/v1/internal/beatmaps/resolve",
            state.sanctuary_url.trim_end_matches('/')
        ))
        .header("x-illya-service-key", state.service_key.as_str())
        .json(&input)
        .send()
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "map_resolution_unavailable",
            )
        })?;
    match response.status() {
        StatusCode::NOT_FOUND => {
            // An absent internal route can also return 404. Only Sanctuary's
            // explicit official lookup miss means the beatmap is unknown.
            let error = response.json::<ServiceError>().await.map_err(|_| {
                ApiError(StatusCode::BAD_GATEWAY, "invalid_map_resolution_response")
            })?;
            if error.error.code == "beatmap_not_found" {
                return Ok(None);
            }
            return Err(ApiError(StatusCode::BAD_GATEWAY, "map_resolution_failed"));
        }
        StatusCode::OK => {}
        StatusCode::SERVICE_UNAVAILABLE => {
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "map_resolution_unavailable",
            ));
        }
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "map_resolution_rate_limited",
            ));
        }
        StatusCode::CONFLICT => {
            let code = match response.json::<ServiceError>().await {
                Ok(error) if error.error.code == "beatmap_filename_ambiguous" => {
                    "beatmap_filename_ambiguous"
                }
                Ok(error) if error.error.code == "beatmap_filename_conflict" => {
                    "beatmap_filename_conflict"
                }
                Ok(error) if error.error.code == "official_set_version_conflict" => {
                    "beatmap_set_version_conflict"
                }
                _ => "beatmap_version_conflict",
            };
            return Err(ApiError(StatusCode::CONFLICT, code));
        }
        _ => return Err(ApiError(StatusCode::BAD_GATEWAY, "map_resolution_failed")),
    }
    // Do not include upstream bodies, transport details or credentials in logs.
    let identity: Identity = response
        .json()
        .await
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_map_resolution_response"))?;
    if !identity.valid_for(&selector) {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "invalid_map_resolution_response",
        ));
    }
    let stored = sqlx::query(
        "SELECT id,official_id,official_set_id,md5,status,source,mode FROM beatmaps WHERE id=$1",
    )
    .bind(identity.id)
    .fetch_optional(&state.pool)
    .await?;
    if stored.as_ref().map(Identity::from_row).as_ref() != Some(&identity) {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "invalid_map_resolution_response",
        ));
    }
    if let Selector::Filename(filename) = selector
        && known_filename(state, filename).await? != Some(identity.id)
    {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "invalid_map_resolution_response",
        ));
    }
    Ok(Some(identity.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_identity_has_a_signed_client_boundary() {
        for id in [1, LOCAL_ID_MIN - 1, LOCAL_ID_MIN, MAX_NATIVE_ID] {
            assert!(valid_id(id));
        }
        for id in [i64::MIN, -1, 0, MAX_NATIVE_ID + 1, i64::MAX] {
            assert!(!valid_id(id));
        }
    }

    #[test]
    fn resolution_reply_must_match_the_requested_selector() {
        let mut identity = Identity {
            id: 1,
            official_id: Some(700001),
            official_set_id: Some(800001),
            md5: "0123456789abcdef0123456789abcdef".into(),
            status: "ranked".into(),
            source: "official".into(),
            mode: 0,
        };
        let checksum = Selector::Checksum("0123456789abcdef0123456789abcdef");
        assert!(identity.valid_for(&checksum));
        assert!(identity.valid_for(&Selector::OfficialId(700001)));
        assert!(!identity.valid_for(&Selector::OfficialId(1)));
        assert!(!identity.valid_for(&Selector::Checksum("ffffffffffffffffffffffffffffffff")));
        identity.official_id = None;
        identity.source = "local".into();
        assert!(identity.valid_for(&checksum));
        assert!(!identity.valid_for(&Selector::OfficialId(700001)));
        identity.md5 = "not-a-checksum".into();
        assert!(!identity.valid_for(&Selector::OfficialId(700001)));
        assert!(!identity.valid_for(&checksum));
    }
}
