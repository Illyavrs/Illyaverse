//! Public discovery of saved, active sets. Metadata and packages remain local.
use crate::{ApiError, ApiResult, AppState, beatmap_availability as availability, maps, page};
use axum::extract::{Path, Query, State};
use illya_domain::Pagination;
use serde::Deserialize;
use serde_json::Value;
use sqlx::Row;

#[derive(Deserialize)]
pub(crate) struct Filter {
    page: Option<u32>,
    page_size: Option<u32>,
    q: Option<String>,
    mode: Option<i16>,
    source: Option<String>,
    status: Option<String>,
    uploaded_by: Option<String>,
    sort: Option<String>,
}

// Both previews and the full set selector use the existing compact difficulty
// shape. Package truth is carried only by the representative's public DTO.
const DIFFICULTY: &str = "jsonb_build_object('id',b.id,'title',b.title,'artist',b.artist,'creator',b.creator,\
    'version',b.version,'mode',b.mode,'status',b.status,'source',b.source,'stars',b.stars,\
    'length_seconds',b.length_seconds,'bpm',b.bpm,'max_combo',b.max_combo)";

pub(crate) async fn list(State(state): State<AppState>, Query(filter): Query<Filter>) -> ApiResult {
    if filter.q.as_ref().is_some_and(|q| q.len() > 100) {
        return Err(ApiError::bad("Search is too long."));
    }
    if filter.mode.is_some_and(|mode| !(0..=3).contains(&mode)) {
        return Err(ApiError::bad("Supply a mode between 0 and 3."));
    }
    if filter
        .source
        .as_deref()
        .is_some_and(|source| !matches!(source, "official" | "local"))
    {
        return Err(ApiError::bad("Unknown source filter."));
    }
    if filter
        .status
        .as_deref()
        .is_some_and(|status| !matches!(status, "pending" | "ranked" | "loved" | "unranked"))
    {
        return Err(ApiError::bad("Unknown status filter."));
    }
    let uploader = filter
        .uploaded_by
        .as_deref()
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| ApiError::bad("Enter a positive uploader ID."))
        })
        .transpose()?;
    // Only these fixed fragments enter SQL; all user text is bound below.
    let order = match filter.sort.as_deref().unwrap_or("recent") {
        "recent" => "g.added_at DESC,g.native_set_id ASC",
        "title" => "g.representative_title ASC,g.native_set_id ASC",
        "stars" => "g.stars_max DESC,g.native_set_id ASC",
        "length" => "g.length_max DESC,g.native_set_id ASC",
        _ => return Err(ApiError::bad("Unknown sort order.")),
    };
    let (current, size, offset) = Pagination {
        page: filter.page,
        page_size: filter.page_size,
    }
    .bounds();
    let q = filter.q.map(|q| format!("%{q}%"));
    // One statement gives counts and page items the same read snapshot, even
    // when the requested page is empty. Identity never depends on metadata,
    // source, archive membership, or a package's retained manifest size.
    let query = format!(
        r#"WITH matching AS MATERIALIZED (
            SELECT b.* FROM active_beatmaps b
            WHERE ($1::text IS NULL OR b.title ILIKE $1 OR b.artist ILIKE $1
                OR b.creator ILIKE $1 OR b.version ILIKE $1)
                AND ($2::smallint IS NULL OR b.mode=$2)
                AND ($3::text IS NULL OR b.source=$3)
                AND ($4::text IS NULL OR b.status=$4)
                AND ($5::bigint IS NULL OR b.uploaded_by=$5)
        ), grouped AS (
            SELECT native_set_id,min(id) AS representative_id,count(*) AS matching_count,
                array_agg(DISTINCT mode ORDER BY mode) AS matching_modes,
                array_agg(DISTINCT status ORDER BY status) AS matching_statuses,
                min(stars) AS stars_min,max(stars) AS stars_max,
                min(length_seconds) AS length_min,max(length_seconds) AS length_max,
                max(created_at) AS added_at
            FROM matching GROUP BY native_set_id
        ), named AS (
            SELECT g.*,b.title AS representative_title
            FROM grouped g JOIN matching b ON b.id=g.representative_id
        ), selected AS (
            SELECT g.*,row_number() OVER (ORDER BY {order}) AS item_order
            FROM named g ORDER BY {order} LIMIT $6 OFFSET $7
        ), projected AS (
            SELECT g.item_order,jsonb_build_object(
                'native_set_id',g.native_set_id,
                'representative',{public_map}||jsonb_build_object('availability',{public_state}),
                'matching_count',g.matching_count,
                'stored_count',(SELECT count(*) FROM beatmaps stored WHERE stored.native_set_id=g.native_set_id),
                'matching_modes',g.matching_modes,'matching_statuses',g.matching_statuses,
                'stars_min',g.stars_min,'stars_max',g.stars_max,
                'length_min',g.length_min,'length_max',g.length_max,'added_at',g.added_at,
                'preview',(SELECT COALESCE(jsonb_agg(d.summary ORDER BY d.mode,d.stars,d.id),'[]'::jsonb)
                    FROM (SELECT {difficulty} AS summary,b.mode,b.stars,b.id
                        FROM matching b WHERE b.native_set_id=g.native_set_id
                        ORDER BY b.mode,b.stars,b.id LIMIT 8) d)
            ) AS item
            FROM selected g JOIN matching b ON b.id=g.representative_id
            LEFT JOIN beatmapset_availability av ON av.native_set_id=g.native_set_id
        )
        SELECT COALESCE((SELECT jsonb_agg(item ORDER BY item_order) FROM projected),'[]'::jsonb) AS items,
            (SELECT count(*) FROM grouped) AS total"#,
        public_map = maps::PUBLIC_MAP,
        public_state = availability::PUBLIC_STATE,
        difficulty = DIFFICULTY,
    );
    let row = sqlx::query(&query)
        .bind(q)
        .bind(filter.mode)
        .bind(filter.source)
        .bind(filter.status)
        .bind(uploader)
        .bind(size)
        .bind(offset)
        .fetch_one(&state.db)
        .await?;
    let items: Value = row.try_get("items")?;
    let items = items
        .as_array()
        .cloned()
        .ok_or_else(|| ApiError::internal("invalid catalogue projection"))?;
    Ok(page(items, row.try_get("total")?, current, size))
}

pub(crate) async fn difficulties(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(pagination): Query<Pagination>,
) -> ApiResult {
    let native_set_id = availability::require_active_map(&state.db, id).await?;
    let (current, size, offset) = pagination.bounds();
    let query = format!(
        r#"WITH members AS MATERIALIZED (
            SELECT b.* FROM active_beatmaps b WHERE b.native_set_id=$1
        ), selected AS (
            SELECT {DIFFICULTY} AS summary,b.mode,b.stars,b.id
            FROM members b ORDER BY b.mode,b.stars,b.id LIMIT $2 OFFSET $3
        )
        SELECT COALESCE((SELECT jsonb_agg(summary ORDER BY mode,stars,id) FROM selected),'[]'::jsonb) AS items,
            (SELECT count(*) FROM members) AS total"#
    );
    let row = sqlx::query(&query)
        .bind(native_set_id)
        .bind(size)
        .bind(offset)
        .fetch_one(&state.db)
        .await?;
    let items: Value = row.try_get("items")?;
    let items = items
        .as_array()
        .cloned()
        .ok_or_else(|| ApiError::internal("invalid difficulty projection"))?;
    availability::require_active_set(&state.db, native_set_id).await?;
    Ok(page(items, row.try_get("total")?, current, size))
}
