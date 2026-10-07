//! Authenticated native discovery delegates official access to Sanctuary.
//! Known catalogue content is authoritative and never needs an upstream call.
use crate::common::{ApiError, AppState};
use axum::http::StatusCode;
use resonance_protocol::stable::is_md5;
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;

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
            && self.official_id.is_none_or(|id| id > 0)
            && self.official_set_id.is_none_or(|id| id > 0)
            && is_md5(&self.md5)
            && match selector {
                Selector::Checksum(checksum) => self.md5 == *checksum,
                Selector::OfficialId(id) => self.official_id == Some(*id),
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
    Ok(sqlx::query_scalar("SELECT id FROM beatmaps WHERE official_id=$1 OR (official_id IS NULL AND id=$1) ORDER BY (official_id=$1) DESC NULLS LAST LIMIT 1")
        .bind(id).fetch_optional(&state.pool).await?)
}

/// Resolve a public ID only after native bearer authentication. A local-only
/// map keeps its own public ID; an unknown ID is looked up as an official ID.
pub async fn resolve_id(state: &AppState, user_id: i64, id: i64) -> Result<Option<i64>, ApiError> {
    if user_id <= 0 || id <= 0 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_beatmap_identity",
        ));
    }
    if let Some(known) = known_id(state, id).await? {
        return Ok(Some(known));
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
            return Err(ApiError(StatusCode::CONFLICT, "beatmap_version_conflict"));
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
    Ok(Some(identity.id))
}

#[cfg(test)]
mod tests {
    use super::*;

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
