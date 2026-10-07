//! Read-only, bounded official set discovery for authenticated native clients.
use super::*;
use axum::body::to_bytes;
use axum::extract::Request;
use illya_domain::official_set::OfficialSetMetadata;

const MAX_POSITIVE_ENTRIES: usize = 32;
const POSITIVE_TTL: Duration = Duration::from_secs(300);
const OPERATION_DEADLINE: Duration = Duration::from_secs(6);

pub(super) struct CachedMetadata {
    until: Instant,
    bytes: usize,
    value: OfficialSetMetadata,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    user_id: i64,
    official_set_id: i64,
}

fn busy() -> ApiError {
    ApiError(
        StatusCode::TOO_MANY_REQUESTS,
        "official_resolution_busy",
        "Official beatmap discovery is busy. Please try shortly.".into(),
    )
}
fn timeout_error() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "official_set_metadata_timeout",
        "Official set metadata could not be retrieved within its deadline. Please try shortly."
            .into(),
    )
}

pub async fn metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> ApiResult {
    // Read the service key before parsing the body, so unauthenticated callers
    // cannot make this endpoint perform JSON or database work.
    require_service_key(&state, &headers)?;
    let bytes = to_bytes(request.into_body(), 4 * 1024).await.map_err(|_| {
        ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_too_large",
            "The metadata request exceeds its size limit.".into(),
        )
    })?;
    let input: Input = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::bad("Supply a user ID and one official set ID."))?;
    if input.user_id <= 0 || input.official_set_id <= 0 {
        return Err(ApiError::bad(
            "Supply a positive user ID and official set ID.",
        ));
    }
    if input.official_set_id > MAX_OFFICIAL_ID {
        return Err(official_namespace_conflict());
    }
    require_active_user(&state, input.user_id).await?;
    let result = match tokio::time::timeout(
        OPERATION_DEADLINE,
        resolve_metadata(&state, input.user_id, input.official_set_id),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let error = timeout_error();
            // Do not extend this deadline while another official request holds
            // the global rate mutex. Cache when it is immediately available.
            if let Ok(mut official) = state.official.try_lock() {
                cache_failure(&mut official, input.official_set_id, &error);
            }
            Err(error)
        }
    };
    // A restriction during flight wait, an upstream request, or a cached lookup
    // must take precedence over both success and cached upstream failures.
    require_service_key(&state, &headers)?;
    require_active_user(&state, input.user_id).await?;
    Ok(Json(
        serde_json::to_value(result?).map_err(ApiError::internal)?,
    ))
}

fn cached(official: &mut OfficialState, set_id: i64) -> Option<ApiResult<OfficialSetMetadata>> {
    official
        .set_metadata
        .retain(|_, cached| cached.until > Instant::now());
    official
        .resolutions
        .retain(|_, cached| cached.until > Instant::now());
    if let Some(cached) = official.set_metadata.get(&set_id) {
        return Some(Ok(cached.value.clone()));
    }
    official
        .resolutions
        .get(&MapSelector::OfficialSetMetadata(set_id))
        .and_then(CachedFailure::error)
        .map(Err)
}

async fn resolve_metadata(
    state: &AppState,
    user_id: i64,
    set_id: i64,
) -> ApiResult<OfficialSetMetadata> {
    let selector = MapSelector::OfficialSet(set_id);
    let (flight, slots) = {
        let mut official = state.official.lock().await;
        if let Some(result) = cached(&mut official, set_id) {
            return result;
        }
        official
            .flights
            .retain(|_, flight| flight.strong_count() > 0);
        let flight = if let Some(flight) = official.flights.get(&selector).and_then(Weak::upgrade) {
            flight
        } else {
            if official.flights.len() >= MAX_RESOLUTION_KEYS {
                return Err(busy());
            }
            let flight = Arc::new(Mutex::new(()));
            official.flights.insert(selector, Arc::downgrade(&flight));
            flight
        };
        (flight, official.slots.clone())
    };
    let _flight = flight.lock().await;
    require_active_user(state, user_id).await?;
    {
        let mut official = state.official.lock().await;
        if let Some(result) = cached(&mut official, set_id) {
            return result;
        }
    }
    let _permit = slots.try_acquire_owned().map_err(|_| busy())?;
    let result = fetch_metadata(state, set_id).await;
    match &result {
        Ok(value) => {
            let bytes = serde_json::to_vec(value).map_err(ApiError::internal)?.len();
            let mut official = state.official.lock().await;
            official
                .set_metadata
                .retain(|_, cached| cached.until > Instant::now());
            while official.set_metadata.len() >= MAX_POSITIVE_ENTRIES
                || official
                    .set_metadata
                    .values()
                    .map(|cached| cached.bytes)
                    .sum::<usize>()
                    + bytes
                    > 16 * 1024 * 1024
            {
                let Some(oldest) = official
                    .set_metadata
                    .iter()
                    .min_by_key(|(_, cached)| cached.until)
                    .map(|(id, _)| *id)
                else {
                    break;
                };
                official.set_metadata.remove(&oldest);
            }
            official.set_metadata.insert(
                set_id,
                CachedMetadata {
                    until: Instant::now() + POSITIVE_TTL,
                    bytes,
                    value: value.clone(),
                },
            );
        }
        Err(error) => remember_failure(state, set_id, error).await,
    }
    result
}

async fn fetch_metadata(state: &AppState, set_id: i64) -> ApiResult<OfficialSetMetadata> {
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
    let value: Value = serde_json::from_slice(
        &read_official_bytes(response, 4 * 1024 * 1024).await?,
    )
    .map_err(|_| {
        upstream_error(
            "official_api_failed",
            "The official set response was invalid.",
        )
    })?;
    OfficialSetMetadata::from_upstream(&value, set_id).map_err(|_| {
        upstream_error(
            "official_identity_mismatch",
            "The official set metadata was invalid.",
        )
    })
}

async fn remember_failure(state: &AppState, set_id: i64, error: &ApiError) {
    let mut official = state.official.lock().await;
    cache_failure(&mut official, set_id, error);
}

fn cache_failure(official: &mut OfficialState, set_id: i64, error: &ApiError) {
    let duration = if error.0 == StatusCode::NOT_FOUND {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(5)
    };
    official
        .resolutions
        .retain(|_, cached| cached.until > Instant::now());
    if official.resolutions.len() >= MAX_RESOLUTION_KEYS
        && let Some(oldest) = official
            .resolutions
            .iter()
            .min_by_key(|(_, cached)| cached.until)
            .map(|(selector, _)| selector.clone())
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
    official
        .resolutions
        .insert(MapSelector::OfficialSetMetadata(set_id), cached);
}

#[cfg(test)]
pub(super) fn cache_shape(official: &OfficialState) -> (usize, usize, Option<Duration>) {
    (
        official.set_metadata.len(),
        official
            .set_metadata
            .values()
            .map(|cached| cached.bytes)
            .sum(),
        official
            .set_metadata
            .values()
            .next()
            .map(|cached| cached.until.saturating_duration_since(Instant::now())),
    )
}
#[cfg(test)]
pub(super) fn expire_metadata(official: &mut OfficialState, set_id: i64) {
    if let Some(cached) = official.set_metadata.get_mut(&set_id) {
        cached.until = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_metadata_shares_the_production_one_second_minimum() {
        for configured in [
            None,
            Some(""),
            Some("invalid"),
            Some("0"),
            Some("1"),
            Some("500"),
            Some("999"),
            Some("1000"),
        ] {
            assert_eq!(
                official_request_interval(configured),
                Duration::from_secs(1)
            );
        }
        assert_eq!(
            official_request_interval(Some("1500")),
            Duration::from_millis(1500)
        );
    }
}
