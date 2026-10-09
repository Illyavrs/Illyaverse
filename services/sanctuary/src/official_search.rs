//! Staff discovery of official sets. The cache contains only public upstream
//! summaries; retained catalog state and authorization are rechecked per request.
use super::*;
use axum::extract::RawQuery;
use serde::Serialize;

const MAX_ENTRIES: usize = 16;
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const POSITIVE_TTL: Duration = Duration::from_secs(60);
const OPERATION_DEADLINE: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct SearchKey {
    q: String,
    mode: String,
    status: String,
    sort: String,
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    q: String,
    #[serde(default = "all")]
    mode: String,
    #[serde(default = "ranked")]
    status: String,
    #[serde(default = "ranked_desc")]
    sort: String,
    cursor_string: Option<String>,
}
fn all() -> String {
    "all".into()
}
fn ranked() -> String {
    "ranked".into()
}
fn ranked_desc() -> String {
    "ranked_desc".into()
}

fn cursor_valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2048 && value.bytes().all(|c| (0x20..=0x7e).contains(&c))
}

fn parse(raw: Option<String>) -> ApiResult<SearchKey> {
    let raw = raw.unwrap_or_default();
    if raw.len() > 8192 {
        return Err(ApiError::bad("The search query exceeds its size limit."));
    }
    let uri = format!("/?{raw}")
        .parse()
        .map_err(|_| ApiError::bad("Invalid search query."))?;
    let Query(input) = Query::<Input>::try_from_uri(&uri)
        .map_err(|_| ApiError::bad("Supply supported official search fields."))?;
    if input.q.len() > 800
        || input.q.chars().count() > 200
        || input.q.chars().any(char::is_control)
        || !matches!(input.mode.as_str(), "all" | "0" | "1" | "2" | "3")
        || !matches!(
            input.status.as_str(),
            "any" | "ranked" | "qualified" | "loved" | "pending" | "graveyard"
        )
        || !matches!(
            input.sort.as_str(),
            "ranked_desc" | "date_desc" | "title_asc" | "difficulty_desc"
        )
        || input
            .cursor_string
            .as_deref()
            .is_some_and(|cursor| !cursor_valid(cursor))
    {
        return Err(ApiError::bad(
            "Supply a bounded search term, supported filters and cursor.",
        ));
    }
    Ok(SearchKey {
        q: input.q.trim().into(),
        mode: input.mode,
        status: input.status,
        sort: input.sort,
        cursor: input.cursor_string,
    })
}

impl SearchKey {
    fn path(&self) -> String {
        // Url's query encoder keeps opaque cursors and Unicode terms in one
        // parameter. Neither input can choose the production host or route.
        let mut url = reqwest::Url::parse("https://osu.ppy.sh/api/v2/beatmapsets/search")
            .expect("fixed official URL");
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("q", &self.q)
                .append_pair("s", &self.status)
                .append_pair(
                    "sort",
                    if self.sort == "date_desc" {
                        "updated_desc"
                    } else {
                        &self.sort
                    },
                );
            if self.mode != "all" {
                pairs.append_pair("m", &self.mode);
            }
            if let Some(cursor) = &self.cursor {
                pairs.append_pair("cursor_string", cursor);
            }
        }
        format!("{}?{}", url.path(), url.query().unwrap_or_default())
    }
}

#[derive(Clone, Serialize)]
struct Difficulty {
    official_id: i64,
    version: String,
    mode: i16,
    stars: f64,
    length_seconds: i32,
}
#[derive(Clone, Default, Serialize)]
struct Local {
    stored_count: i64,
    archive_available: bool,
    withdrawn: bool,
}
#[derive(Clone, Serialize)]
struct Item {
    official_set_id: i64,
    title: String,
    artist: String,
    creator: String,
    status: String,
    cover_url: String,
    official_url: String,
    download_disabled: bool,
    difficulties: Vec<Difficulty>,
    local: Local,
}
#[derive(Clone, Serialize)]
struct SearchResult {
    items: Vec<Item>,
    total: i64,
    cursor_string: Option<String>,
}
pub(super) struct CachedSearch {
    until: Instant,
    bytes: usize,
    value: Result<SearchResult, CachedFailure>,
}

fn invalid() -> ApiError {
    upstream_error(
        "official_search_invalid",
        "The official search response was invalid.",
    )
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
        "official_search_timeout",
        "Official search could not finish within its deadline. Please try shortly.".into(),
    )
}
fn text(value: &Value) -> ApiResult<String> {
    let value = value
        .as_str()
        .filter(|value| value.len() <= 1024)
        .ok_or_else(invalid)?;
    let value = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid());
    }
    Ok(value.into())
}
fn id(value: &Value) -> ApiResult<i64> {
    value
        .as_i64()
        .filter(|id| (1..=MAX_OFFICIAL_ID).contains(id))
        .ok_or_else(invalid)
}
fn transform(value: &Value) -> ApiResult<SearchResult> {
    let sets = value["beatmapsets"]
        .as_array()
        .filter(|sets| sets.len() <= 50)
        .ok_or_else(invalid)?;
    let total = value["total"]
        .as_i64()
        .filter(|total| (0..=i32::MAX as i64).contains(total))
        .ok_or_else(invalid)?;
    let cursor_string = match value.get("cursor_string") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|value| cursor_valid(value))
                .ok_or_else(invalid)?
                .into(),
        ),
    };
    let mut items = Vec::with_capacity(sets.len());
    let mut set_ids = HashSet::new();
    let mut map_ids = HashSet::new();
    for set in sets {
        if set.get("deleted_at").is_some_and(|value| !value.is_null()) {
            continue;
        }
        let set_id = id(&set["id"])?;
        if !set_ids.insert(set_id) {
            return Err(invalid());
        }
        let status = set["status"]
            .as_str()
            .filter(|status| {
                matches!(
                    *status,
                    "ranked" | "approved" | "qualified" | "loved" | "pending" | "wip" | "graveyard"
                )
            })
            .ok_or_else(invalid)?
            .to_owned();
        let maps = set["beatmaps"]
            .as_array()
            .filter(|maps| maps.len() <= 1024)
            .ok_or_else(invalid)?;
        let mut difficulties = Vec::new();
        for map in maps {
            match map.get("convert") {
                Some(Value::Bool(true)) => continue,
                None | Some(Value::Bool(false)) => {}
                _ => return Err(invalid()),
            }
            if map.get("deleted_at").is_some_and(|value| !value.is_null()) {
                continue;
            }
            let official_id = id(&map["id"])?;
            if map["beatmapset_id"].as_i64() != Some(set_id) || !map_ids.insert(official_id) {
                return Err(invalid());
            }
            let mode = map["mode_int"]
                .as_i64()
                .filter(|mode| (0..=3).contains(mode))
                .ok_or_else(invalid)? as i16;
            if map.get("mode").is_some_and(|value| {
                value.as_str() != Some(["osu", "taiko", "fruits", "mania"][mode as usize])
            }) {
                return Err(invalid());
            }
            let stars = map["difficulty_rating"]
                .as_f64()
                .filter(|stars| stars.is_finite() && (0.0..=1000.0).contains(stars))
                .ok_or_else(invalid)?;
            let length_seconds = map["total_length"]
                .as_i64()
                .and_then(|length| i32::try_from(length).ok())
                .filter(|length| *length >= 0)
                .ok_or_else(invalid)?;
            difficulties.push(Difficulty {
                official_id,
                version: text(&map["version"])?,
                mode,
                stars,
                length_seconds,
            });
            if difficulties.len() > 512 {
                return Err(invalid());
            }
        }
        if difficulties.is_empty() {
            continue;
        }
        difficulties.sort_by(|a, b| {
            a.mode
                .cmp(&b.mode)
                .then_with(|| a.stars.total_cmp(&b.stars))
                .then(a.official_id.cmp(&b.official_id))
        });
        let download_disabled = match set.get("availability") {
            None | Some(Value::Null) => false,
            Some(Value::Object(value)) => match value.get("download_disabled") {
                None => false,
                Some(value) => value.as_bool().ok_or_else(invalid)?,
            },
            _ => return Err(invalid()),
        };
        items.push(Item {
            official_set_id: set_id,
            title: text(&set["title"])?,
            artist: text(&set["artist"])?,
            creator: text(&set["creator"])?,
            status,
            cover_url: format!("https://assets.ppy.sh/beatmaps/{set_id}/covers/card.jpg"),
            official_url: format!("https://osu.ppy.sh/beatmapsets/{set_id}"),
            download_disabled,
            difficulties,
            local: Local::default(),
        });
    }
    Ok(SearchResult {
        items,
        total,
        cursor_string,
    })
}

fn cached(official: &mut OfficialState, key: &SearchKey) -> Option<ApiResult<SearchResult>> {
    official
        .search_cache
        .retain(|_, entry| entry.until > Instant::now());
    official
        .search_cache
        .get(key)
        .map(|entry| match &entry.value {
            Ok(value) => Ok(value.clone()),
            Err(error) => Err(ApiError(error.status, error.code, error.message.clone())),
        })
}
fn remember(official: &mut OfficialState, key: SearchKey, result: &ApiResult<SearchResult>) {
    let (until, bytes, value) = match result {
        Ok(value) => {
            let bytes = serde_json::to_vec(value).map_or(MAX_CACHE_BYTES + 1, |value| value.len());
            (Instant::now() + POSITIVE_TTL, bytes, Ok(value.clone()))
        }
        Err(error) => {
            let mut failure = CachedFailure::new(error, Duration::from_secs(5));
            if error.0 == StatusCode::TOO_MANY_REQUESTS
                && let Some(global) = &official.failure
                && global.status == StatusCode::TOO_MANY_REQUESTS
            {
                failure.until = global.until;
            }
            (failure.until, failure.message.len() + 128, Err(failure))
        }
    };
    if bytes > MAX_CACHE_BYTES {
        return;
    }
    official
        .search_cache
        .retain(|_, entry| entry.until > Instant::now());
    official.search_cache.remove(&key);
    while official.search_cache.len() >= MAX_ENTRIES
        || official
            .search_cache
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            + bytes
            > MAX_CACHE_BYTES
    {
        let Some(oldest) = official
            .search_cache
            .iter()
            .min_by_key(|(_, entry)| entry.until)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        official.search_cache.remove(&oldest);
    }
    official.search_cache.insert(
        key,
        CachedSearch {
            until,
            bytes,
            value,
        },
    );
}
async fn authorize(state: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    auth::require(state, headers)
        .await?
        .require("beatmaps.import")
}
async fn resolve(
    state: &AppState,
    headers: &HeaderMap,
    key: &SearchKey,
) -> ApiResult<SearchResult> {
    let selector = MapSelector::OfficialSearch(key.clone());
    let (flight, slots) = {
        let mut official = state.official.lock().await;
        if let Some(result) = cached(&mut official, key) {
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
    authorize(state, headers).await?;
    {
        let mut official = state.official.lock().await;
        if let Some(result) = cached(&mut official, key) {
            return result;
        }
    }
    let _permit = slots.try_acquire_owned().map_err(|_| busy())?;
    let result = async {
        let response = official_request(state, &key.path(), true).await?;
        let bytes = read_official_bytes(response, 4 * 1024 * 1024).await?;
        let value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        transform(&value)
    }
    .await;
    remember(&mut *state.official.lock().await, key.clone(), &result);
    result
}
async fn merge_local(state: &AppState, result: &mut SearchResult) -> ApiResult<()> {
    let ids = result
        .items
        .iter()
        .map(|item| item.official_set_id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query("SELECT candidate.id, (SELECT count(*) FROM beatmaps b WHERE b.official_set_id=candidate.id AND b.source='official') AS stored_count, EXISTS(SELECT 1 FROM official_beatmapset_archives a WHERE a.official_set_id=candidate.id) AS archive_available, NOT illya_beatmapset_active(candidate.id) AS withdrawn FROM unnest($1::bigint[]) AS candidate(id)")
        .bind(&ids).fetch_all(&state.db).await?;
    let local = rows
        .into_iter()
        .map(|row| {
            (
                row.get::<i64, _>("id"),
                Local {
                    stored_count: row.get("stored_count"),
                    archive_available: row.get("archive_available"),
                    withdrawn: row.get("withdrawn"),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for item in &mut result.items {
        item.local = local
            .get(&item.official_set_id)
            .cloned()
            .unwrap_or_default();
        if item.local.withdrawn {
            // Staff can identify a blocked official set without exposing retained
            // internal IDs, paths, metadata or archive availability through cache.
            item.local.stored_count = 0;
            item.local.archive_available = false;
        }
    }
    Ok(())
}
pub async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    authorize(&state, &headers).await?;
    let key = parse(raw)?;
    let result = tokio::time::timeout(OPERATION_DEADLINE, async {
        let mut result = resolve(&state, &headers, &key).await?;
        authorize(&state, &headers).await?;
        merge_local(&state, &mut result).await?;
        authorize(&state, &headers).await?;
        Ok::<_, ApiError>(result)
    })
    .await;
    authorize(&state, &headers).await?;
    let result = match result {
        Ok(result) => result?,
        Err(_) => {
            let error = timeout_error();
            if let Ok(mut official) = state.official.try_lock() {
                remember(
                    &mut official,
                    key,
                    &Err(ApiError(error.0, error.1, error.2.clone())),
                );
            }
            return Err(error);
        }
    };
    Ok(Json(
        serde_json::to_value(result).map_err(ApiError::internal)?,
    ))
}

#[cfg(test)]
#[path = "official_search_tests.rs"]
mod tests;
