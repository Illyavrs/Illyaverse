//! Complete read-only difficulty lists for known retained cards. Official
//! metadata discovery does not publish any difficulty or archive.
use super::*;
use illya_domain::official_set::OfficialSetMetadata;
use serde::Serialize;

const OPERATION_DEADLINE: Duration = Duration::from_secs(6);

#[derive(Clone, Eq, PartialEq)]
struct Mapping {
    native_set_id: i64,
    source: String,
    official_set_id: Option<i64>,
    official_id: Option<i64>,
}
#[derive(Serialize)]
struct Item {
    id: Option<i64>,
    official_id: Option<i64>,
    version: String,
    mode: i16,
    stars: f64,
    length_seconds: i32,
    status: String,
}
#[derive(Serialize)]
struct Preview {
    native_set_id: i64,
    source: String,
    items: Vec<Item>,
    total: usize,
    stored_count: usize,
}
fn unavailable() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "beatmap_preview_unavailable",
        "The complete difficulty preview is unavailable.".into(),
    )
}
fn timeout_error() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "beatmap_preview_timeout",
        "The complete difficulty preview could not finish within its deadline.".into(),
    )
}
async fn mapping(state: &AppState, id: i64) -> ApiResult<Mapping> {
    if id <= 0 {
        return Err(ApiError::missing());
    }
    let row = sqlx::query("SELECT native_set_id,source,official_set_id,official_id FROM beatmaps WHERE id=$1 AND illya_beatmapset_active(native_set_id)")
        .bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    let value = Mapping {
        native_set_id: row.get("native_set_id"),
        source: row.get("source"),
        official_set_id: row.get("official_set_id"),
        official_id: row.get("official_id"),
    };
    if value.source == "official"
        && (value.official_set_id != Some(value.native_set_id)
            || !(1..=MAX_OFFICIAL_ID).contains(&value.native_set_id)
            || value
                .official_id
                .is_none_or(|id| !(1..=MAX_OFFICIAL_ID).contains(&id)))
    {
        return Err(unavailable());
    }
    if !matches!(value.source.as_str(), "official" | "local") {
        return Err(unavailable());
    }
    Ok(value)
}
fn official_preview(
    metadata: OfficialSetMetadata,
    retained: &HashMap<i64, i64>,
) -> ApiResult<Preview> {
    metadata.validate(metadata.id).map_err(|_| unavailable())?;
    let mut items = metadata
        .beatmaps
        .into_iter()
        .map(|map| Item {
            id: retained.get(&map.id).copied(),
            official_id: Some(map.id),
            version: map.version,
            mode: map.mode_int,
            stars: map.difficulty_rating,
            length_seconds: map.total_length,
            status: map.status,
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        a.mode
            .cmp(&b.mode)
            .then_with(|| a.stars.total_cmp(&b.stars))
            .then(a.official_id.cmp(&b.official_id))
    });
    let stored_count = items.iter().filter(|item| item.id.is_some()).count();
    Ok(Preview {
        native_set_id: metadata.id,
        source: "official".into(),
        total: items.len(),
        stored_count,
        items,
    })
}
async fn resolve(state: &AppState, known: &Mapping) -> ApiResult<Preview> {
    if known.source == "official" {
        let metadata = official_set_metadata::preview_metadata(state, known.native_set_id).await?;
        let rows = sqlx::query("SELECT id,official_id FROM beatmaps WHERE native_set_id=$1 AND official_set_id=$1 AND source='official' AND official_id IS NOT NULL ORDER BY id LIMIT 513")
            .bind(known.native_set_id).fetch_all(&state.db).await?;
        if rows.len() > 512 {
            return Err(unavailable());
        }
        let retained = rows
            .into_iter()
            .map(|row| (row.get::<i64, _>("official_id"), row.get::<i64, _>("id")))
            .collect::<HashMap<_, _>>();
        return official_preview(metadata, &retained);
    }
    let rows = sqlx::query("SELECT id,version,mode,stars,length_seconds,status FROM beatmaps WHERE native_set_id=$1 AND source='local' ORDER BY mode,stars,id LIMIT 513")
        .bind(known.native_set_id).fetch_all(&state.db).await?;
    if rows.is_empty() || rows.len() > 512 {
        return Err(unavailable());
    }
    let items = rows
        .into_iter()
        .map(|row| Item {
            id: Some(row.get("id")),
            official_id: None,
            version: row.get("version"),
            mode: row.get("mode"),
            stars: row.get("stars"),
            length_seconds: row.get("length_seconds"),
            status: row.get("status"),
        })
        .collect::<Vec<_>>();
    if items.iter().any(|item| {
        item.version.is_empty()
            || item.version.len() > 1024
            || item.version.chars().any(char::is_control)
            || !(0..=3).contains(&item.mode)
            || !item.stars.is_finite()
            || !(0.0..=1000.0).contains(&item.stars)
            || item.length_seconds < 0
    }) {
        return Err(unavailable());
    }
    Ok(Preview {
        native_set_id: known.native_set_id,
        source: "local".into(),
        total: items.len(),
        stored_count: items.len(),
        items,
    })
}
pub async fn preview(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let known = mapping(&state, id).await?;
    let result = tokio::time::timeout(OPERATION_DEADLINE, resolve(&state, &known)).await;
    // Recheck even when the upstream timed out or a cached failure was returned.
    // Withdrawn sets share the unknown-card response and expose no retained IDs.
    if mapping(&state, id).await? != known {
        return Err(ApiError::missing());
    }
    let value = match result {
        Ok(value) => value?,
        Err(_) => {
            let error = timeout_error();
            if known.source == "official"
                && let Ok(mut official) = state.official.try_lock()
            {
                official_set_metadata::remember_preview_failure(
                    &mut official,
                    known.native_set_id,
                    &error,
                );
            }
            return Err(error);
        }
    };
    Ok(Json(
        serde_json::to_value(value).map_err(ApiError::internal)?,
    ))
}

#[cfg(test)]
#[path = "map_card_preview_tests.rs"]
mod tests;
