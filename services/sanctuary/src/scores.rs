use crate::{
    ApiError, ApiResult, AppState, auth, beatmap_availability as availability, page, score_scales,
};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use illya_domain::{
    HitCounts, Pagination, ScoreVariant, has_permission, score_mods::ValidatedMods,
    score_variant::legacy_pp_eligible,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ScoreQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    user_id: Option<i64>,
    beatmap_id: Option<i64>,
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
    verification: Option<String>,
}

#[derive(Deserialize)]
pub struct BestForUserQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
}

pub async fn best_for_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
    Query(query): Query<BestForUserQuery>,
) -> ApiResult {
    let mode = query.mode.unwrap_or(0);
    let variant = query.variant.unwrap_or_default();
    validate_partition(mode, variant)?;
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();

    // Visibility, qualification, the deduplicated count and globally weighted
    // positions all use one database snapshot, including for authenticated viewers.
    let mut response = sqlx::query_scalar::<_, Value>(
        "WITH public_player AS (
            SELECT id,username FROM users WHERE id=$1 AND NOT is_restricted
        ),eligible AS (
            SELECT s.*,u.username,b.title,b.version
            FROM scores s JOIN public_player u ON u.id=s.user_id
            JOIN active_beatmaps b ON b.id=s.beatmap_id
            WHERE s.mode=$2 AND s.score_variant=$3 AND s.verification='verified'
                AND s.passed AND b.status='ranked' AND s.pp_eligible
        ),best AS (
            SELECT DISTINCT ON(beatmap_id) * FROM eligible
            ORDER BY beatmap_id,pp DESC,accuracy DESC,max_combo DESC,id
        ),positioned AS (
            SELECT *,row_number() OVER(ORDER BY pp DESC,beatmap_id,id) position FROM best
        ),weighted AS (
            SELECT *,power(0.95,position-1) pp_weight,
                pp*power(0.95,position-1) weighted_pp FROM positioned
        ),paged AS (
            SELECT * FROM weighted ORDER BY position LIMIT $4 OFFSET $5
        )
        SELECT jsonb_build_object(
            'total',(SELECT count(*) FROM best),
            'items',COALESCE((SELECT jsonb_agg(
                to_jsonb(p)-'replay_path'-'ingestion_key'||jsonb_build_object(
                    'has_replay',p.replay_path IS NOT NULL,
                    'accuracy_source',CASE WHEN p.client='lazer' THEN 'client_reported' ELSE 'judgements' END
                ) ORDER BY p.position) FROM paged p),'[]'::jsonb)
        ) FROM public_player",
    )
    .bind(user_id)
    .bind(mode)
    .bind(variant.as_str())
    .bind(size)
    .bind(offset)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(ApiError::missing)?;
    response["page"] = json!(p);
    response["page_size"] = json!(size);
    response["mode"] = json!(mode);
    response["score_variant"] = json!(variant);
    response["collection"] = json!("best");
    response["ordered_by"] = json!("pp");
    Ok(Json(response))
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScoreQuery>,
) -> ApiResult {
    let scoped = query.user_id.is_some()
        || query.beatmap_id.is_some()
        || query.mode.is_some()
        || query.variant.is_some();
    let mode = if scoped {
        Some(query.mode.unwrap_or(0))
    } else {
        None
    };
    let variant = if scoped {
        Some(query.variant.unwrap_or_default())
    } else {
        None
    };
    if let (Some(mode), Some(variant)) = (mode, variant) {
        validate_partition(mode, variant)?;
    }
    let user = auth::optional(&state, &headers).await?;
    let moderator = user
        .as_ref()
        .is_some_and(|u| has_permission(&u.permissions, "scores.moderate"));
    if query.verification.as_ref().is_some_and(|v| v != "verified") && !moderator {
        return Err(ApiError::forbidden());
    }
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let own_id = user.map(|u| u.id);
    let all = moderator;
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM scores s JOIN users u ON u.id=s.user_id WHERE ($1::bigint IS NULL OR user_id=$1) AND ($2::bigint IS NULL OR beatmap_id=$2) AND ($3::smallint IS NULL OR mode=$3) AND ($4::text IS NULL OR verification=$4) AND (verification='verified' OR user_id=$5 OR $6) AND (NOT u.is_restricted OR $6) AND ($7::text IS NULL OR s.score_variant=$7)")
        .bind(query.user_id).bind(query.beatmap_id).bind(mode).bind(&query.verification).bind(own_id).bind(all).bind(variant.map(ScoreVariant::as_str)).fetch_one(&state.db).await?;
    let items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s)-'replay_path'-'ingestion_key' || jsonb_build_object('username',u.username,'title',b.title,'version',b.version,'has_replay',s.replay_path IS NOT NULL,'accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM scores s JOIN users u ON u.id=s.user_id JOIN beatmaps b ON b.id=s.beatmap_id WHERE ($1::bigint IS NULL OR user_id=$1) AND ($2::bigint IS NULL OR beatmap_id=$2) AND ($3::smallint IS NULL OR s.mode=$3) AND ($4::text IS NULL OR verification=$4) AND (verification='verified' OR user_id=$5 OR $6) AND (NOT u.is_restricted OR $6) AND ($9::text IS NULL OR s.score_variant=$9) ORDER BY s.created_at DESC,s.id DESC LIMIT $7 OFFSET $8")
        .bind(query.user_id).bind(query.beatmap_id).bind(mode).bind(query.verification).bind(own_id).bind(all).bind(size).bind(offset).bind(variant.map(ScoreVariant::as_str)).fetch_all(&state.db).await?;
    let mut response = page(items, total, p, size).0;
    response["mode"] = json!(mode);
    response["score_variant"] = json!(variant);
    Ok(Json(response))
}
pub async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult {
    let viewer = auth::optional(&state, &headers).await?;
    let moderator = viewer
        .as_ref()
        .is_some_and(|user| has_permission(&user.permissions, "scores.moderate"));
    let own_id = viewer.map(|user| user.id);
    let value=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s)-'replay_path'-'ingestion_key' || jsonb_build_object('username',u.username,'title',b.title,'version',b.version,'has_replay',s.replay_path IS NOT NULL,'accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END,'replay_url',CASE WHEN s.replay_path IS NOT NULL THEN '/api/v1/scores/'||s.id||'/replay' ELSE NULL END,'user',jsonb_build_object('id',u.id,'username',u.username,'country',u.country,'avatar_url',u.avatar_url),'beatmap',to_jsonb(b)-'osu_path'-'archive_path') FROM scores s JOIN users u ON u.id=s.user_id JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.id=$1 AND (s.verification='verified' OR s.user_id=$2 OR $3) AND (NOT u.is_restricted OR $3)").bind(id).bind(own_id).bind(moderator).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    Ok(Json(value))
}
#[derive(Deserialize)]
pub struct LeaderboardQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
    country: Option<String>,
}
pub async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> ApiResult {
    let mode = query.mode.unwrap_or(0);
    let variant = query.variant.unwrap_or_default();
    validate_partition(mode, variant)?;
    validate_country(query.country.as_deref())?;
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE NOT is_restricted AND ($1::text IS NULL OR country=$1)",
    )
    .bind(&query.country)
    .fetch_one(&state.db)
    .await?;
    let sql = format!(
        "{RANKING_CTE} SELECT to_jsonb(r)||jsonb_build_object('mode',$1,'score_variant',$2,'total_score',CASE WHEN $2='vanilla' THEN r.total_score END,'ranked_score',CASE WHEN $2='vanilla' THEN r.ranked_score END,'score_totals_complete',$2='vanilla' AND r.play_count=r.scaled_plays) FROM ranked r WHERE ($3::text IS NULL OR country=$3) ORDER BY rank LIMIT $4 OFFSET $5"
    );
    let items = sqlx::query_scalar::<_, Value>(&sql)
        .bind(mode)
        .bind(variant.as_str())
        .bind(query.country)
        .bind(size)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;
    let mut response = page(items, total, p, size).0;
    response["mode"] = json!(mode);
    response["score_variant"] = json!(variant);
    Ok(Json(response))
}

pub(crate) fn validate_partition(mode: i16, variant: ScoreVariant) -> ApiResult<()> {
    if !(0..=3).contains(&mode) {
        return Err(ApiError::bad("Unsupported mode."));
    }
    ScoreVariant::from_legacy(
        mode as u8,
        if variant == ScoreVariant::Relax {
            128
        } else {
            0
        },
    )
    .map_err(ApiError::bad)?;
    Ok(())
}

fn validate_country(country: Option<&str>) -> ApiResult<()> {
    if country.is_some_and(|value| {
        value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_uppercase())
    }) {
        return Err(ApiError::bad(
            "Country must be a two-letter uppercase country code.",
        ));
    }
    Ok(())
}

pub(crate) const RANKING_CTE: &str = "WITH eligible AS (
    SELECT s.* FROM scores s JOIN active_beatmaps b ON b.id=s.beatmap_id
    WHERE s.mode=$1 AND s.score_variant=$2 AND s.verification='verified' AND s.passed AND b.status='ranked' AND s.pp_eligible
),best AS (
    SELECT user_id,beatmap_id,max(pp) pp FROM eligible GROUP BY user_id,beatmap_id
),weighted AS (
    SELECT user_id,pp,row_number() OVER(PARTITION BY user_id ORDER BY pp DESC,beatmap_id) n FROM best
),totals AS (
    SELECT user_id,sum(pp*power(0.95,n-1)) pp FROM weighted GROUP BY user_id
),best_score AS (
    SELECT DISTINCT ON(user_id,beatmap_id) user_id,beatmap_id,classic_score
    FROM eligible WHERE standardised_score IS NOT NULL AND classic_score IS NOT NULL
    ORDER BY user_id,beatmap_id,standardised_score DESC,id
),score_totals AS (
    SELECT user_id,LEAST(sum(classic_score),9223372036854775807)::bigint ranked_score FROM best_score GROUP BY user_id
),lifetime AS (
    SELECT user_id,count(*)::bigint play_count,COALESCE(avg(accuracy),0)::float8 accuracy,
    LEAST(COALESCE(sum(CASE WHEN client='stable' AND (mods & 536870912)=0 THEN score WHEN client IN ('stable','lazer') THEN classic_score END),0),9223372036854775807)::bigint total_score,
    count(*) FILTER(WHERE (client='stable' AND (mods & 536870912)=0) OR (client IN ('stable','lazer') AND classic_score IS NOT NULL))::bigint scaled_plays,
    COALESCE(max(max_combo),0)::integer maximum_combo,
    COALESCE(sum(COALESCE((hit_counts->>'n300')::bigint,0)+COALESCE((hit_counts->>'n100')::bigint,0)+COALESCE((hit_counts->>'misses')::bigint,0)+CASE WHEN mode<>1 THEN COALESCE((hit_counts->>'n50')::bigint,0) ELSE 0 END+CASE WHEN mode=3 THEN COALESCE((hit_counts->>'geki')::bigint,0) ELSE 0 END+CASE WHEN mode IN(2,3) THEN COALESCE((hit_counts->>'katu')::bigint,0) ELSE 0 END),0)::bigint total_hits
    FROM scores WHERE mode=$1 AND score_variant=$2 AND verification='verified' GROUP BY user_id
),ranked AS (
    SELECT u.id,u.username,u.country,u.avatar_url,COALESCE(t.pp,0)::float8 pp,
    row_number() OVER(ORDER BY COALESCE(t.pp,0) DESC,u.id) rank,
    row_number() OVER(PARTITION BY u.country ORDER BY COALESCE(t.pp,0) DESC,u.id) country_rank,
    COALESCE(l.play_count,0)::bigint play_count,COALESCE(l.accuracy,0)::float8 accuracy,
    COALESCE(l.total_score,0)::bigint total_score,COALESCE(l.scaled_plays,0)::bigint scaled_plays,
    COALESCE(l.maximum_combo,0)::integer maximum_combo,COALESCE(l.total_hits,0)::bigint total_hits,
    COALESCE(st.ranked_score,0)::bigint ranked_score
    FROM users u LEFT JOIN totals t ON t.user_id=u.id LEFT JOIN lifetime l ON l.user_id=u.id LEFT JOIN score_totals st ON st.user_id=u.id
    WHERE NOT u.is_restricted
)";

pub(crate) async fn statistics(
    state: &AppState,
    user_id: i64,
    mode: i16,
    variant: ScoreVariant,
) -> ApiResult<Value> {
    validate_partition(mode, variant)?;
    let sql = format!("{RANKING_CTE} SELECT * FROM ranked WHERE id=$3");
    let stats = sqlx::query(&sql)
        .bind(mode)
        .bind(variant.as_str())
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    let grades:Value=sqlx::query_scalar("WITH played AS (SELECT CASE WHEN NOT s.passed THEN 'F' ELSE COALESCE(s.native_submission->>'rank',t.submission->>'rank',a.details->>'rank',CASE WHEN s.accuracy=100 THEN CASE WHEN (s.mods & 1032)=0 THEN 'X' ELSE 'XH' END END) END grade FROM scores s LEFT JOIN LATERAL(SELECT submission FROM lazer_score_tokens WHERE score_id=s.id ORDER BY id DESC LIMIT 1)t ON true LEFT JOIN LATERAL(SELECT details FROM audit_events WHERE action='scores.stable_ingest' AND target_id=s.id::text ORDER BY id DESC LIMIT 1)a ON true WHERE s.user_id=$1 AND s.mode=$2 AND s.score_variant=$3 AND s.verification='verified') SELECT jsonb_build_object('ss',count(*) FILTER(WHERE grade='X'),'ssh',count(*) FILTER(WHERE grade='XH'),'s',count(*) FILTER(WHERE grade='S'),'sh',count(*) FILTER(WHERE grade='SH'),'a',count(*) FILTER(WHERE grade='A'),'b',count(*) FILTER(WHERE grade='B'),'c',count(*) FILTER(WHERE grade='C'),'d',count(*) FILTER(WHERE grade='D'),'f',count(*) FILTER(WHERE grade='F')) FROM played")
        .bind(user_id).bind(mode).bind(variant.as_str()).fetch_one(&state.db).await?;
    let pp = stats.get::<f64, _>("pp");
    let plays = stats.get::<i64, _>("play_count");
    let totals_available = variant == ScoreVariant::Vanilla;
    let rank = if pp > 0.0 {
        Some(stats.get::<i64, _>("rank"))
    } else {
        None
    };
    Ok(
        json!({"mode":mode,"score_variant":variant,"pp":pp,"rank":rank,"global_rank":rank,"country_rank":if pp>0.0{Some(stats.get::<i64,_>("country_rank"))}else{None},"play_count":plays,"accuracy":stats.get::<f64,_>("accuracy"),"maximum_combo":stats.get::<i32,_>("maximum_combo"),"total_hits":stats.get::<i64,_>("total_hits"),"total_score":if totals_available{Some(stats.get::<i64,_>("total_score"))}else{None},"ranked_score":if totals_available{Some(stats.get::<i64,_>("ranked_score"))}else{None},"score_totals_complete":totals_available&&plays==stats.get::<i64,_>("scaled_plays"),"score_scale":if totals_available{"classic_display"}else{"unavailable_for_relax"},"grade_counts":grades}),
    )
}

#[derive(Deserialize)]
pub struct BeatmapLeaderboardQuery {
    page: Option<u32>,
    page_size: Option<u32>,
    mode: Option<i16>,
    variant: Option<ScoreVariant>,
    country: Option<String>,
}
pub async fn beatmap_leaderboard(
    State(state): State<AppState>,
    Path(beatmap_id): Path<i64>,
    Query(query): Query<BeatmapLeaderboardQuery>,
) -> ApiResult {
    let mode = query.mode.unwrap_or(0);
    let variant = query.variant.unwrap_or_default();
    validate_partition(mode, variant)?;
    validate_country(query.country.as_deref())?;
    let native_set_id = availability::require_active_map(&state.db, beatmap_id).await?;
    let map = sqlx::query("SELECT mode,title,version FROM beatmaps WHERE id=$1")
        .bind(beatmap_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    let source: i16 = map.get("mode");
    if source != 0 && source != mode {
        return Err(ApiError::bad(
            "This beatmap does not support the requested ruleset.",
        ));
    }
    let (p, size, offset) = Pagination {
        page: query.page,
        page_size: query.page_size,
    }
    .bounds();
    let rx = variant == ScoreVariant::Relax;
    let total:i64=sqlx::query_scalar("SELECT count(DISTINCT s.user_id) FROM scores s JOIN users u ON u.id=s.user_id WHERE s.beatmap_id=$1 AND s.mode=$2 AND s.score_variant=$3 AND s.verification='verified' AND s.passed AND s.pp_eligible AND NOT u.is_restricted AND ($4 OR s.standardised_score IS NOT NULL) AND ($5::text IS NULL OR u.country=$5)")
        .bind(beatmap_id).bind(mode).bind(variant.as_str()).bind(rx).bind(&query.country).fetch_one(&state.db).await?;
    let items=sqlx::query_scalar::<_,Value>("WITH best AS (SELECT DISTINCT ON(s.user_id) s.*,u.username,u.country,u.avatar_url FROM scores s JOIN users u ON u.id=s.user_id WHERE s.beatmap_id=$1 AND s.mode=$2 AND s.score_variant=$3 AND s.verification='verified' AND s.passed AND s.pp_eligible AND NOT u.is_restricted AND ($4 OR s.standardised_score IS NOT NULL) AND ($5::text IS NULL OR u.country=$5) ORDER BY s.user_id,CASE WHEN $4 THEN s.pp END DESC NULLS LAST,CASE WHEN NOT $4 THEN s.standardised_score END DESC NULLS LAST,s.accuracy DESC,s.max_combo DESC,s.id),placed AS (SELECT *,row_number() OVER(ORDER BY CASE WHEN $4 THEN pp END DESC NULLS LAST,CASE WHEN NOT $4 THEN standardised_score END DESC NULLS LAST,accuracy DESC,max_combo DESC,id) position FROM best) SELECT to_jsonb(p)-'replay_path'-'ingestion_key'||jsonb_build_object('title',$8::text,'version',$9::text,'has_replay',replay_path IS NOT NULL,'accuracy_source',CASE WHEN client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM placed p ORDER BY position LIMIT $6 OFFSET $7")
        .bind(beatmap_id).bind(mode).bind(variant.as_str()).bind(rx).bind(query.country).bind(size).bind(offset).bind(map.get::<String,_>("title")).bind(map.get::<String,_>("version")).fetch_all(&state.db).await?;
    let mut response = page(items, total, p, size).0;
    response["mode"] = json!(mode);
    response["score_variant"] = json!(variant);
    response["ordered_by"] = json!(if rx { "pp" } else { "standardised_score" });
    availability::require_active_set(&state.db, native_set_id).await?;
    Ok(Json(response))
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submission {
    pub(crate) user_id: Option<i64>,
    pub(crate) beatmap_id: i64,
    pub(crate) mode: i16,
    pub(crate) score: i64,
    pub(crate) accuracy: f64,
    pub(crate) max_combo: u32,
    pub(crate) mods: u32,
    #[serde(default)]
    pub(crate) api_mods: Option<Value>,
    #[serde(default)]
    pub(crate) native_submission: Option<resonance_protocol::lazer::SoloScoreSubmission>,
    #[serde(default = "passed_default")]
    pub(crate) passed: bool,
    pub(crate) hit_counts: HitCounts,
    pub(crate) client: Option<String>,
    pub(crate) ingestion_key: Option<String>,
}
fn passed_default() -> bool {
    true
}
pub fn accuracy(mode: i16, h: &HitCounts) -> ApiResult<f64> {
    let (a, b, c, m, g, k) = (
        h.n300 as f64,
        h.n100 as f64,
        h.n50 as f64,
        h.misses as f64,
        h.geki as f64,
        h.katu as f64,
    );
    let (numerator, denominator) = match mode {
        0 => (6.0 * a + 2.0 * b + c, 6.0 * (a + b + c + m)),
        1 => (2.0 * a + b, 2.0 * (a + b + m)),
        2 => (a + b + c, a + b + c + k + m),
        3 => (
            6.0 * (g + a) + 4.0 * k + 2.0 * b + c,
            6.0 * (g + a + k + b + c + m),
        ),
        _ => return Err(ApiError::bad("Unsupported mode.")),
    };
    if denominator == 0.0 {
        return Err(ApiError::bad(
            "Hit counts must include at least one judgement.",
        ));
    }
    Ok(100.0 * numerator / denominator)
}
async fn ingest(
    state: &AppState,
    user_id: i64,
    input: Submission,
    client: &str,
    website: Option<&auth::User>,
) -> ApiResult<Value> {
    if input.score < 0
        || !input.accuracy.is_finite()
        || !(0.0..=100.0).contains(&input.accuracy)
        || input.max_combo > 1_000_000
        || !(0..=3).contains(&input.mode)
    {
        return Err(ApiError::bad("Invalid score values."));
    }
    if input.native_submission.is_some() && client != "lazer" {
        return Err(ApiError::bad(
            "Native context requires internal lazer ingestion.",
        ));
    }
    score_scales::validate_native_binding(&input)?;
    let (variant, pp_eligible) = mod_partition(&input, client)?;
    if let Some(ref key) = input.ingestion_key {
        if key.len() > 128 || key.is_empty() {
            return Err(ApiError::bad("Invalid ingestion key."));
        }
        if sqlx::query_scalar::<_, i64>("SELECT id FROM scores WHERE ingestion_key=$1")
            .bind(key)
            .fetch_optional(&state.db)
            .await?
            .is_some()
        {
            let mut tx = state.db.begin().await?;
            let actor = score_publication_actor(&mut tx, user_id, website).await?;
            let existing = retained_submission(&mut tx, key, user_id, &input, client, variant)
                .await?
                .ok_or_else(ApiError::missing)?;
            if let Some(actor) = &actor {
                auth::ensure_mutation_actor(&mut tx, actor).await?;
            }
            tx.commit().await?;
            return Ok(existing);
        }
    }
    let counts = &input.hit_counts;
    if [
        counts.n300,
        counts.n100,
        counts.n50,
        counts.misses,
        counts.geki,
        counts.katu,
        counts.large_tick_hits,
        counts.small_tick_hits,
        counts.slider_end_hits,
    ]
    .iter()
    .any(|&v| v > 1_000_000)
    {
        return Err(ApiError::bad("Hit counts exceed processing limits."));
    }
    let valid: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1 AND NOT is_restricted)")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    if !valid {
        return Err(ApiError::unauthorized());
    }
    let map = sqlx::query("SELECT osu_path,mode,max_combo,native_set_id FROM beatmaps WHERE id=$1")
        .bind(input.beatmap_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
    let native_set_id: i64 = map.get("native_set_id");
    if let Err(error) = availability::require_active_set(&state.db, native_set_id).await {
        // An exact score may have committed after the initial retry lookup.
        if let Some(key) = &input.ingestion_key {
            let mut tx = state.db.begin().await?;
            let actor = score_publication_actor(&mut tx, user_id, website).await?;
            if let Some(value) =
                retained_submission(&mut tx, key, user_id, &input, client, variant).await?
            {
                if let Some(actor) = &actor {
                    auth::ensure_mutation_actor(&mut tx, actor).await?;
                }
                tx.commit().await?;
                return Ok(value);
            }
        }
        return Err(error);
    }
    let source_mode = map.get::<i16, _>("mode");
    if source_mode != input.mode && source_mode != 0 {
        return Err(ApiError::bad(
            "Only osu!standard beatmaps can be converted to another ruleset.",
        ));
    }
    // Lazer includes slider judgements in its native accuracy. Until a complete
    // server score-processor is implemented, preserve this reported value and
    // disclose its source; these scores remain pending moderation.
    let accuracy = if client == "lazer" {
        input.accuracy
    } else {
        accuracy(input.mode, &input.hit_counts)?
    };
    let path: String = map.get("osu_path");
    if std::path::Path::new(&path).is_absolute() || path.contains("..") {
        return Err(ApiError::internal("unsafe stored map path"));
    }
    let bytes = tokio::fs::read(state.maps_root.join(path)).await?;
    let mut calc = input.clone();
    // The caller selected the authenticated transport; an input JSON field
    // cannot turn public API submissions into stable normalization candidates.
    calc.client = Some(client.to_owned());
    let lazer = client == "lazer";
    let calculated = tokio::task::spawn_blocking(move || calculate_score(&bytes, &calc, lazer))
        .await
        .map_err(ApiError::internal)??;
    let scales = calculated.scales;
    let mut tx = state.db.begin().await?;
    let actor = score_publication_actor(&mut tx, user_id, website).await?;
    sqlx::query("SELECT illya_lock_beatmapset($1)")
        .bind(native_set_id)
        .execute(&mut *tx)
        .await?;
    // A publisher holding this shared fence may have saved the exact result
    // while our calculation or admission wait ran. Retained retries survive
    // withdrawal; incomplete intent never establishes that exception.
    if let Some(key) = &input.ingestion_key
        && let Some(value) =
            retained_submission(&mut tx, key, user_id, &input, client, variant).await?
    {
        if let Some(actor) = &actor {
            auth::ensure_mutation_actor(&mut tx, actor).await?;
        }
        tx.commit().await?;
        return Ok(value);
    }
    availability::lock_active_set(&mut tx, native_set_id).await?;
    let mut value:Value=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,pp_eligible,standardised_score,standardised_without_mods,classic_score,native_submission,native_basic_maximum,score_scale_status,score_scale_version,score_scale_reason,legacy_attributes,score_variant,pp_calculation_version) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,'rosu_pp4.v1') ON CONFLICT(ingestion_key) DO UPDATE SET ingestion_key=scores.ingestion_key WHERE scores.user_id=excluded.user_id AND scores.beatmap_id=excluded.beatmap_id AND scores.mode=excluded.mode AND scores.score_variant=excluded.score_variant RETURNING to_jsonb(scores)-'replay_path'")
        .bind(user_id).bind(input.beatmap_id).bind(input.mode).bind(input.score).bind(accuracy).bind(input.max_combo as i32).bind(i64::from(input.mods)).bind(calculated.pp).bind(input.passed).bind(json!(input.hit_counts)).bind(client).bind(&input.ingestion_key).bind(&input.api_mods).bind(pp_eligible).bind(scales.standardised_score).bind(scales.standardised_without_mods).bind(scales.classic_score).bind(scales.native_submission).bind(scales.native_basic_maximum).bind(scales.status).bind(scales.version).bind(scales.reason).bind(scales.legacy_attributes).bind(variant.as_str()).fetch_optional(&mut *tx).await?.ok_or_else(||ApiError(axum::http::StatusCode::CONFLICT,"ingestion_conflict","The ingestion key belongs to another score.".into()))?;
    // ON CONFLICT may observe a winner published after the locked retry lookup.
    // Compare its immutable input before committing even the no-op update.
    ensure_retained_input(&value, user_id, &input, client, variant)?;
    if let Some(actor) = &actor {
        auth::ensure_mutation_actor(&mut tx, actor).await?;
    }
    tx.commit().await?;
    value["accuracy_source"] = json!(if client == "lazer" {
        "client_reported"
    } else {
        "judgements"
    });
    Ok(value)
}

async fn retained_submission(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
    user_id: i64,
    input: &Submission,
    client: &str,
    variant: ScoreVariant,
) -> ApiResult<Option<Value>> {
    let value: Option<Value> = sqlx::query_scalar("SELECT to_jsonb(s)-'replay_path'||jsonb_build_object('accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM scores s WHERE ingestion_key=$1 FOR SHARE OF s")
        .bind(key).fetch_optional(&mut **tx).await?;
    if let Some(value) = &value {
        ensure_retained_input(value, user_id, input, client, variant)?;
    }
    Ok(value)
}

fn ingestion_conflict() -> ApiError {
    ApiError(
        axum::http::StatusCode::CONFLICT,
        "ingestion_conflict",
        "The ingestion key belongs to a different score submission.".into(),
    )
}

fn same_structured_mods(mode: i16, saved: &Value, supplied: Option<&Value>) -> bool {
    // Historical NULL and an empty structured array both mean that no native
    // modifier parameters were supplied. Raw legacy bits are checked separately.
    let empty = json!([]);
    let saved = if saved.is_null() { &empty } else { saved };
    let supplied = supplied.unwrap_or(&empty);
    match (
        ValidatedMods::parse(mode as u8, saved),
        ValidatedMods::parse(mode as u8, supplied),
    ) {
        (Ok(saved), Ok(supplied)) => saved.mods == supplied.mods,
        _ => false,
    }
}

fn same_native_context(
    saved: &Value,
    supplied: Option<&resonance_protocol::lazer::SoloScoreSubmission>,
) -> bool {
    match (saved.is_null(), supplied) {
        (true, None) => true,
        (false, Some(supplied)) => {
            let Ok(mut saved) = serde_json::from_value::<
                resonance_protocol::lazer::SoloScoreSubmission,
            >(saved.clone()) else {
                return false;
            };
            if saved.validate().is_err()
                || !same_structured_mods(
                    i16::from(supplied.ruleset_id),
                    &json!(saved.mods),
                    Some(&json!(supplied.mods)),
                )
                || (saved.accuracy - supplied.accuracy).abs() > 1e-12
            {
                return false;
            }
            // Modifier order and explicit zero counters do not change input
            // semantics. All remaining typed native fields stay immutable.
            let mut supplied = supplied.clone();
            for context in [&mut saved, &mut supplied] {
                context.mods.clear();
                context.accuracy = 0.0;
                context.statistics.retain(|_, value| *value != 0);
                context.maximum_statistics.retain(|_, value| *value != 0);
            }
            json!(saved) == json!(supplied)
        }
        _ => false,
    }
}

/// Compare submitted semantic context, not mutable review or calculation caches.
/// Historical sparse counters deserialize with the same default-zero semantics
/// as incoming HitCounts. Derived scale/PP versions may legitimately change.
fn ensure_retained_input(
    saved: &Value,
    user_id: i64,
    input: &Submission,
    client: &str,
    variant: ScoreVariant,
) -> ApiResult<()> {
    let counts = serde_json::from_value::<HitCounts>(saved["hit_counts"].clone())
        .map_err(|_| ingestion_conflict())?;
    let expected_accuracy = if client == "lazer" {
        input.accuracy
    } else {
        // The stable/API caller's accuracy is not authoritative: publication
        // derives it from judgements, and retries retain that same meaning.
        accuracy(input.mode, &input.hit_counts).map_err(|_| ingestion_conflict())?
    };
    let same_accuracy = saved["accuracy"]
        .as_f64()
        .is_some_and(|saved| saved.is_finite() && (saved - expected_accuracy).abs() <= 1e-7);
    if saved["user_id"].as_i64() != Some(user_id)
        || saved["beatmap_id"].as_i64() != Some(input.beatmap_id)
        || saved["mode"].as_i64() != Some(i64::from(input.mode))
        || saved["score_variant"].as_str() != Some(variant.as_str())
        || saved["client"].as_str() != Some(client)
        || saved["score"].as_i64() != Some(input.score)
        || saved["max_combo"].as_u64() != Some(u64::from(input.max_combo))
        || saved["mods"].as_u64() != Some(u64::from(input.mods))
        || saved["passed"].as_bool() != Some(input.passed)
        || json!(counts) != json!(input.hit_counts)
        || !same_accuracy
        || !same_structured_mods(input.mode, &saved["api_mods"], input.api_mods.as_ref())
        || !same_native_context(
            &saved["native_submission"],
            input.native_submission.as_ref(),
        )
    {
        return Err(ingestion_conflict());
    }
    Ok(())
}

async fn score_publication_actor(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: i64,
    website: Option<&auth::User>,
) -> ApiResult<Option<auth::MutationActor>> {
    if let Some(snapshot) = website {
        if snapshot.id != user_id {
            return Err(ApiError::unauthorized());
        }
        Ok(Some(
            auth::mutation_actor(tx, snapshot, &[], auth::MutationScope::Session).await?,
        ))
    } else {
        // Native ingress authenticates the service separately, then retains the
        // player's restriction lock through publication without a web session.
        let active = sqlx::query_scalar::<_, bool>(
            "SELECT NOT is_restricted FROM users WHERE id=$1 FOR SHARE",
        )
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(false);
        if !active {
            return Err(ApiError::unauthorized());
        }
        Ok(None)
    }
}

fn mod_partition(score: &Submission, client: &str) -> ApiResult<(ScoreVariant, bool)> {
    if !(0..=3).contains(&score.mode) {
        return Err(ApiError::bad("Unsupported ruleset."));
    }
    if let Some(ref native) = score.api_mods {
        if client != "lazer" {
            return Err(ApiError::bad(
                "Structured mods require native lazer ingestion.",
            ));
        }
        let validated = ValidatedMods::parse(score.mode as u8, native).map_err(ApiError::bad)?;
        if validated.legacy_bits != score.mods {
            return Err(ApiError::bad(
                "Legacy mod projection does not match native mods.",
            ));
        }
        Ok((validated.score_variant, validated.pp_eligible))
    } else {
        Ok((
            ScoreVariant::from_legacy(score.mode as u8, score.mods).map_err(ApiError::bad)?,
            legacy_pp_eligible(score.mode as u8, score.mods).map_err(ApiError::bad)?,
        ))
    }
}
pub(crate) struct CalculatedScore {
    pub pp: f64,
    pub scales: score_scales::NativeScales,
}

/// Reconstruct calculation inputs without changing original score/PP/verification.
/// A persisted native context (or the recovery query's token fallback) supplies
/// the correct catch miss projection for calculations without rewriting raw rows.
pub(crate) fn from_stored_row(row: &sqlx::postgres::PgRow) -> ApiResult<Submission> {
    let client: String = row.try_get("client")?;
    let recovered = row
        .try_get::<Option<Value>, _>("recovered_native_submission")
        .ok()
        .flatten();
    let stored: Option<Value> = row.try_get("native_submission")?;
    let native_submission = stored
        .or(recovered)
        .map(serde_json::from_value::<resonance_protocol::lazer::SoloScoreSubmission>)
        .transpose()
        .map_err(|_| ApiError::bad("Stored native context is invalid."))?;
    let mut hit_counts: HitCounts = serde_json::from_value(row.try_get("hit_counts")?)
        .map_err(|_| ApiError::bad("Stored hit counts are invalid."))?;
    let mut api_mods: Option<Value> = row.try_get("api_mods")?;
    if client == "lazer"
        && let Some(context) = &native_submission
    {
        hit_counts = serde_json::from_value(context.legacy_hit_counts())
            .map_err(|_| ApiError::bad("Stored native hit counts exceed processing limits."))?;
        api_mods = Some(json!(context.mods));
    }
    Ok(Submission {
        user_id: Some(row.try_get("user_id")?),
        beatmap_id: row.try_get("beatmap_id")?,
        mode: row.try_get("mode")?,
        score: row.try_get("score")?,
        accuracy: row.try_get("accuracy")?,
        max_combo: u32::try_from(row.try_get::<i32, _>("max_combo")?)
            .map_err(|_| ApiError::bad("Stored combo is invalid."))?,
        mods: u32::try_from(row.try_get::<i64, _>("mods")?)
            .map_err(|_| ApiError::bad("Stored modifiers are invalid."))?,
        api_mods,
        native_submission,
        passed: row.try_get("passed")?,
        hit_counts,
        client: Some(client),
        ingestion_key: row.try_get("ingestion_key")?,
    })
}

struct NativeManiaResults;
impl rosu_pp::any::HitResultGenerator<rosu_pp::mania::Mania> for NativeManiaResults {
    fn generate_hitresults(
        input: rosu_pp::mania::InspectManiaPerformance<'_>,
    ) -> rosu_pp::mania::ManiaHitResults {
        rosu_pp::mania::ManiaHitResults {
            n320: input.n320.unwrap_or(0),
            n300: input.n300.unwrap_or(0),
            n200: input.n200.unwrap_or(0),
            n100: input.n100.unwrap_or(0),
            n50: input.n50.unwrap_or(0),
            misses: input.misses.unwrap_or(0),
        }
    }
}

pub(crate) fn calculate_score(
    bytes: &[u8],
    score: &Submission,
    lazer: bool,
) -> ApiResult<CalculatedScore> {
    let (variant, _) = mod_partition(score, if lazer { "lazer" } else { "stable" })?;
    let map =
        rosu_pp::Beatmap::from_bytes(bytes).map_err(|_| ApiError::bad("Stored map is invalid."))?;
    let mode = match score.mode {
        0 => rosu_pp::model::mode::GameMode::Osu,
        1 => rosu_pp::model::mode::GameMode::Taiko,
        2 => rosu_pp::model::mode::GameMode::Catch,
        3 => rosu_pp::model::mode::GameMode::Mania,
        _ => return Err(ApiError::bad("Unsupported ruleset.")),
    };
    let mods = if let Some(ref native) = score.api_mods {
        if !lazer {
            return Err(ApiError::bad(
                "Structured mods require native lazer ingestion.",
            ));
        }
        let validated = ValidatedMods::parse(score.mode as u8, native).map_err(ApiError::bad)?;
        if validated.legacy_bits != score.mods {
            return Err(ApiError::bad(
                "Legacy mod projection does not match native mods.",
            ));
        }
        rosu_pp::GameMods::from(validated.mods)
    } else {
        rosu_pp::GameMods::from(score.mods)
    };
    let scales_map = map.clone();
    let map = map
        .convert(mode, &mods)
        .map_err(|_| ApiError::bad("This beatmap cannot be converted to the requested ruleset."))?;
    let attrs = rosu_pp::Difficulty::new()
        .mods(mods.clone())
        .lazer(lazer)
        .checked_calculate(&map)
        .map_err(|_| ApiError::bad("Beatmap exceeds safe calculation limits."))?;
    if score.max_combo > attrs.max_combo() {
        return Err(ApiError::bad(
            "Combo exceeds this ruleset's beatmap maximum.",
        ));
    }
    let mut scales = if lazer {
        score_scales::native_scales(score, &attrs)?
    } else {
        if score.native_submission.is_some() {
            return Err(ApiError::bad(
                "Native context requires internal lazer ingestion.",
            ));
        }
        if score.client.as_deref() == Some("stable") {
            score_scales::legacy_scales(score, &scales_map, &attrs)?
        } else {
            score_scales::NativeScales::unavailable("unsupported_score_source")
        }
    };
    if variant == ScoreVariant::Relax {
        // RX's legacy source multiplier is zero, so the summary does not define
        // a common display scale. Preserve native/replay context and map checks,
        // while keeping PP and raw scores independent of display conversion.
        scales.standardised_score = None;
        scales.standardised_without_mods = None;
        scales.classic_score = None;
        scales.status = "unavailable";
        scales.version = Some("relax_score_scale.unavailable.v1".into());
        scales.reason =
            Some("Relax has no supported common score display scale; rankings use PP.".into());
    }
    if lazer && score.mode == 3 {
        let h = &score.hit_counts;
        let actual = [h.geki, h.n300, h.katu, h.n100, h.n50, h.misses]
            .into_iter()
            .map(u64::from)
            .sum::<u64>();
        let maximum = u64::from(score_scales::basic_n(&attrs)?);
        if actual > maximum || (score.passed && actual != maximum) {
            return Err(ApiError::bad(
                "Native mania head/tail judgments do not match this beatmap.",
            ));
        }
    }
    let h = &score.hit_counts;
    let state = rosu_pp::any::ScoreState {
        max_combo: score.max_combo,
        n300: h.n300,
        n100: h.n100,
        n50: h.n50,
        misses: h.misses,
        n_geki: h.geki,
        n_katu: h.katu,
        osu_large_tick_hits: h.large_tick_hits,
        osu_small_tick_hits: h.small_tick_hits,
        slider_end_hits: h.slider_end_hits,
        legacy_total_score: u32::try_from(score.score).ok(),
    };
    let performance = rosu_pp::Performance::new(attrs)
        .mods(mods)
        .lazer(lazer)
        .state(state);
    let pp = match performance {
        rosu_pp::Performance::Mania(p) if lazer => {
            p.hitresult_generator::<NativeManiaResults>()
                .calculate()
                .map_err(|_| ApiError::bad("Native mania performance attributes are invalid."))?
                .pp
        }
        other => other.calculate().pp(),
    };
    if !pp.is_finite() {
        return Err(ApiError::bad("Performance calculation was not finite."));
    }
    Ok(CalculatedScore {
        pp: if score.passed { pp } else { 0.0 },
        scales,
    })
}

#[cfg(test)]
fn calculate_pp(bytes: &[u8], score: &Submission, lazer: bool) -> ApiResult<f64> {
    calculate_score(bytes, score, lazer).map(|calculated| calculated.pp)
}
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Submission>,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    if input.user_id.is_some_and(|id| id != user.id)
        || input.client.as_ref().is_some_and(|v| v != "api")
        || input.ingestion_key.is_some()
        || input.native_submission.is_some()
    {
        return Err(ApiError::bad(
            "API submissions belong to the signed-in user, use the api client, and cannot supply an ingestion key.",
        ));
    }
    Ok(Json(
        ingest(&state, user.id, input, "api", Some(&user)).await?,
    ))
}
pub async fn internal_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Submission>,
) -> ApiResult {
    let supplied = headers
        .get("x-illya-service-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Constant-time byte comparison for the fixed shared service secret.
    let expected = state.internal_key.as_bytes();
    let mut mismatch = expected.len() ^ supplied.len();
    for (index, &byte) in expected.iter().enumerate() {
        mismatch |= usize::from(byte ^ supplied.as_bytes().get(index).copied().unwrap_or(0));
    }
    if expected.len() < 32 || mismatch != 0 {
        return Err(ApiError::unauthorized());
    }
    let user_id = input
        .user_id
        .ok_or_else(|| ApiError::bad("Supply user_id."))?;
    let client = input.client.clone().unwrap_or_else(|| "stable".into());
    if !matches!(client.as_str(), "stable" | "lazer") {
        return Err(ApiError::bad("Unsupported internal client."));
    }
    Ok(Json(ingest(&state, user_id, input, &client, None).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelaxRecalculation {
    score_ids: Vec<i64>,
}
fn relax_calculation_binding(row: &sqlx::postgres::PgRow) -> Value {
    json!({
        "user_id":row.get::<i64,_>("user_id"),"beatmap_id":row.get::<i64,_>("beatmap_id"),
        "mode":row.get::<i16,_>("mode"),"score":row.get::<i64,_>("score"),
        "accuracy":row.get::<f64,_>("accuracy"),"max_combo":row.get::<i32,_>("max_combo"),
        "mods":row.get::<i64,_>("mods"),"passed":row.get::<bool,_>("passed"),
        "hit_counts":row.get::<Value,_>("hit_counts"),"api_mods":row.get::<Option<Value>,_>("api_mods"),
        "native_submission":row.get::<Option<Value>,_>("native_submission"),
        "client":row.get::<String,_>("client"),"score_variant":row.get::<String,_>("score_variant"),
        "osu_path":row.get::<String,_>("osu_path")
    })
}
pub async fn recalculate_relax(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RelaxRecalculation>,
) -> ApiResult {
    let operator = auth::require(&state, &headers).await?;
    operator.require("scores.moderate")?;
    if input.score_ids.is_empty()
        || input.score_ids.len() > 16
        || input.score_ids.iter().any(|&id| id <= 0)
    {
        return Err(ApiError::bad(
            "Choose between one and sixteen positive score IDs.",
        ));
    }
    let mut ids = input.score_ids;
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ApiError::bad("Score IDs must be unique."));
    }
    let _permit = state.upload_permit()?;
    // Check live moderation authority before private resource existence, then
    // release these locks while doing bounded file and calculation work.
    let mut preflight = state.db.begin().await?;
    let preflight_actor = crate::admin::lock_actor(&mut preflight, &operator, None).await?;
    preflight_actor.require("scores.moderate")?;
    auth::ensure_mutation_actor(&mut preflight, &preflight_actor).await?;
    preflight.commit().await?;
    let mut prepared = Vec::with_capacity(ids.len());
    for id in ids {
        let row=sqlx::query("SELECT s.*,b.osu_path FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND NOT u.is_restricted")
            .bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
        if row.get::<String, _>("score_variant") != "relax"
            || row.get::<i16, _>("mode") == 3
            || !matches!(
                row.get::<String, _>("verification").as_str(),
                "pending" | "verified"
            )
        {
            return Err(ApiError::bad(
                "Only pending or verified Relax scores in osu!, taiko or catch can be recalculated.",
            ));
        }
        let submission = from_stored_row(&row)?;
        let client = row.get::<String, _>("client");
        let (variant, eligible) = mod_partition(&submission, &client)?;
        if variant != ScoreVariant::Relax || !eligible {
            return Err(ApiError::bad(
                "These modifiers are not eligible for Relax PP.",
            ));
        }
        score_scales::validate_native_binding(&submission)?;
        let path: String = row.get("osu_path");
        if std::path::Path::new(&path).is_absolute() || path.contains("..") {
            return Err(ApiError::internal("unsafe stored map path"));
        }
        let bytes = tokio::fs::read(state.maps_root.join(path)).await?;
        let lazer = client == "lazer";
        let calculated =
            tokio::task::spawn_blocking(move || calculate_score(&bytes, &submission, lazer))
                .await
                .map_err(ApiError::internal)??;
        prepared.push((
            id,
            row.get::<i64, _>("user_id"),
            relax_calculation_binding(&row),
            calculated.pp,
        ));
    }
    let owners: Vec<i64> = prepared.iter().map(|(_, owner, _, _)| *owner).collect();
    let mut tx = state.db.begin().await?;
    let operator = auth::mutation_actor(
        &mut tx,
        &operator,
        &owners,
        auth::MutationScope::RoleChanges,
    )
    .await?;
    operator.require("scores.moderate")?;
    let mut results = Vec::with_capacity(prepared.len());
    for (id, _, binding, pp) in prepared {
        let row=sqlx::query("SELECT s.*,b.osu_path FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND NOT u.is_restricted FOR UPDATE OF s")
            .bind(id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
        if relax_calculation_binding(&row) != binding
            || !matches!(
                row.get::<String, _>("verification").as_str(),
                "pending" | "verified"
            )
        {
            return Err(ApiError(
                axum::http::StatusCode::CONFLICT,
                "score_changed_retry",
                "A score changed while preparing its calculation. Please retry.".into(),
            ));
        }
        let previous = row.get::<f64, _>("pp");
        let previous_eligible = row.get::<bool, _>("pp_eligible");
        sqlx::query("UPDATE scores SET pp=$2,pp_eligible=true,pp_calculation_version='rosu_pp4.relax.v1' WHERE id=$1")
            .bind(id).bind(pp).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.relax_recalculate','score',$2,$3)")
            .bind(operator.id).bind(id.to_string()).bind(json!({"previous_pp":previous,"pp":pp,"previous_pp_eligible":previous_eligible,"pp_eligible":true,"mode":row.get::<i16,_>("mode"),"score_variant":"relax","calculation_version":"rosu_pp4.relax.v1","verification_unchanged":true}))
            .execute(&mut *tx).await?;
        results.push(json!({"id":id,"pp":pp,"pp_eligible":true,"score_variant":"relax","pp_calculation_version":"rosu_pp4.relax.v1","verification":row.get::<String,_>("verification")}));
    }
    auth::ensure_mutation_actor(&mut tx, &operator).await?;
    tx.commit().await?;
    Ok(Json(json!({"items":results,"count":results.len()})))
}
#[derive(Clone, PartialEq)]
struct ReplayUploadBinding {
    user_id: i64,
    username: String,
    md5: String,
    mode: i16,
    total: i64,
    combo: i32,
    mods: i64,
    hits: Value,
    lazer: bool,
    submission: Option<Value>,
}

impl ReplayUploadBinding {
    fn from_row(score: &sqlx::postgres::PgRow) -> Self {
        let lazer = score.get::<String, _>("client") == "lazer";
        Self {
            user_id: score.get("user_id"),
            username: score.get("username"),
            md5: score.get("md5"),
            mode: score.get("mode"),
            total: score.get("score"),
            combo: score.get("max_combo"),
            mods: score.get("mods"),
            hits: score.get("hit_counts"),
            lazer,
            submission: if lazer {
                score.get("replay_submission")
            } else {
                None
            },
        }
    }

    fn validate(&self, id: i64, bytes: &[u8]) -> ApiResult<()> {
        if self.lazer {
            let input: resonance_protocol::lazer::SoloScoreSubmission = serde_json::from_value(
                self.submission
                    .clone()
                    .ok_or_else(|| ApiError::bad("Stored lazer submission is required."))?,
            )
            .map_err(ApiError::internal)?;
            let replay = resonance_protocol::lazer_replay::ExtendedReplay::parse(bytes)
                .map_err(|e| ApiError::bad(format!("Invalid extended replay: {e}")))?;
            let binding = resonance_protocol::lazer_replay::ReplayScore {
                online_id: id,
                user_id: self.user_id,
                username: &self.username,
                beatmap_md5: &self.md5,
                client_version: "",
                mods_bits: self.mods as u32,
                timestamp_ticks: 0,
                submission: &input,
            };
            replay
                .matches(&binding)
                .map_err(|_| ApiError::bad("Replay metadata does not match the stored score."))?;
        } else {
            let replay = resonance_protocol::replay::LegacyReplay::parse(bytes)
                .map_err(|e| ApiError::bad(format!("Invalid replay container: {e}")))?;
            let h: HitCounts =
                serde_json::from_value(self.hits.clone()).map_err(ApiError::internal)?;
            if replay.beatmap_md5 != self.md5
                || replay.player_name != self.username
                || i16::from(replay.mode) != self.mode
                || i64::from(replay.total_score) != self.total
                || i32::from(replay.max_combo) != self.combo
                || i64::from(replay.mods) != self.mods
                || replay.hit_counts.map(u32::from)
                    != [h.n300, h.n100, h.n50, h.geki, h.katu, h.misses]
            {
                return Err(ApiError::bad(
                    "Replay metadata does not match the stored score.",
                ));
            }
        }
        Ok(())
    }
}

pub async fn upload_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    let _permit = state.upload_permit()?;
    let score=sqlx::query("SELECT s.*,b.md5,u.username,COALESCE(s.native_submission,(SELECT submission FROM lazer_score_tokens WHERE score_id=s.id AND user_id=s.user_id ORDER BY id LIMIT 1)) replay_submission FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    if score.get::<i64, _>("user_id") != user.id {
        return Err(ApiError::forbidden());
    }
    let mut bytes = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?
    {
        if field.name() != Some("file") {
            continue;
        }
        if bytes.is_some() {
            return Err(ApiError::bad("Attach exactly one replay file."));
        }
        if !field
            .file_name()
            .unwrap_or("")
            .to_ascii_lowercase()
            .ends_with(".osr")
        {
            return Err(ApiError::bad("Upload an .osr replay."));
        }
        let mut buffer = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
        {
            if buffer.len() + chunk.len() > resonance_protocol::replay::MAX_REPLAY_BYTES {
                return Err(ApiError::bad("Replay exceeds 16 MiB."));
            }
            buffer.extend_from_slice(&chunk);
        }
        bytes = Some(buffer);
    }
    let bytes = bytes.ok_or_else(|| ApiError::bad("Attach a replay file."))?;
    let binding = ReplayUploadBinding::from_row(&score);
    let prepared_binding = binding.clone();
    let (bytes, sha) = tokio::task::spawn_blocking(move || -> ApiResult<(Vec<u8>, String)> {
        use sha2::{Digest, Sha256};
        prepared_binding.validate(id, &bytes)?;
        let sha = format!("{:x}", Sha256::digest(&bytes));
        Ok((bytes, sha))
    })
    .await
    .map_err(ApiError::internal)??;
    // Legacy stored rows may not have a digest. Read their file before the
    // short transaction and use it only if the locked row still names it.
    let initial_path: Option<String> = score.get("replay_path");
    let initial_sha: Option<String> = score.get("replay_sha256");
    let fallback_sha = if let Some(previous) = &initial_path {
        if initial_sha.is_none() {
            use sha2::{Digest, Sha256};
            if std::path::Path::new(previous).is_absolute()
                || previous.contains(['/', '\\'])
                || previous.contains("..")
            {
                return Err(ApiError::internal("unsafe stored replay path"));
            }
            Some(format!(
                "{:x}",
                Sha256::digest(tokio::fs::read(state.replay_root.join(previous)).await?)
            ))
        } else {
            None
        }
    } else {
        None
    };
    let path = format!("{}-{}.osr", id, Uuid::new_v4());
    let temporary = state.replay_root.join(format!("{path}.tmp"));
    let destination = state.replay_root.join(&path);
    use tokio::io::AsyncWriteExt;
    let mut temporary_owned = false;
    let mut published = false;
    let mut commit_attempted = false;
    let outcome: ApiResult = async {
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    temporary_owned = true;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    let mut tx = state.db.begin().await?;
    let actor = auth::mutation_actor(&mut tx, &user, &[], auth::MutationScope::Session).await?;
    let score=sqlx::query("SELECT s.*,b.md5,u.username,COALESCE(s.native_submission,(SELECT submission FROM lazer_score_tokens WHERE score_id=s.id AND user_id=s.user_id ORDER BY id LIMIT 1)) replay_submission FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND s.user_id=$2 FOR UPDATE OF s").bind(id).bind(actor.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    if ReplayUploadBinding::from_row(&score) != binding {
        return Err(ApiError(axum::http::StatusCode::CONFLICT,"replay_binding_changed","The stored score changed while preparing its replay. Please retry.".into()));
    }
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    if let Some(previous) = score.get::<Option<String>, _>("replay_path") {
        let existing = score.get::<Option<String>, _>("replay_sha256").or_else(|| {
            (initial_path.as_ref() == Some(&previous)).then(|| fallback_sha.clone()).flatten()
        }).ok_or_else(|| ApiError(axum::http::StatusCode::CONFLICT,"replay_changed_retry","The stored replay changed. Please retry.".into()))?;
        if existing != sha {
            return Err(ApiError(axum::http::StatusCode::CONFLICT,"replay_already_uploaded","This score already has a different replay.".into()));
        }
        auth::ensure_mutation_actor(&mut tx, &actor).await?;
        tx.commit().await?;
        return Ok(Json(json!({"ok":true,"verification":score.get::<String,_>("verification"),"has_replay":true,"idempotent":true})));
    }
    tokio::fs::rename(&temporary, &destination).await?;
    published = true;
    sqlx::query("UPDATE scores SET replay_path=$2,replay_sha256=$3,replay_format=$4,replay_complete=NULL,verification='pending' WHERE id=$1")
        .bind(id)
        .bind(&path)
        .bind(&sha).bind(if binding.lazer {"lazer"} else {"legacy"})
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.replay_upload','score',$2,$3)").bind(actor.id).bind(id.to_string()).bind(json!({"metadata_matches":true,"gameplay_verified":false,"sha256":sha,"format":if binding.lazer {"lazer"} else {"legacy"}})).execute(&mut *tx).await?;
    auth::ensure_mutation_actor(&mut tx, &actor).await?;
    commit_attempted = true;
        if let Err(error) = tx.commit().await {
            commit_attempted = !crate::maps::commit_definitely_rolled_back(&error);
            return Err(error.into());
        }
    Ok(Json(json!({"ok":true,"verification":"pending","has_replay":true})))
    }.await;
    if temporary_owned {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    // A failed acknowledgement after COMMIT may still have published its row.
    if published && !commit_attempted {
        let _ = tokio::fs::remove_file(&destination).await;
    }
    outcome
}
pub async fn replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    let row=sqlx::query("SELECT user_id,verification,replay_path,u.is_restricted FROM scores s JOIN users u ON u.id=s.user_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
    if row.get::<String, _>("verification") != "verified" || row.get::<bool, _>("is_restricted") {
        let user = auth::require(&state, &headers).await?;
        if user.id != row.get::<i64, _>("user_id") {
            user.require("scores.moderate")?;
        }
    }
    let path = row
        .get::<Option<String>, _>("replay_path")
        .ok_or_else(ApiError::missing)?;
    if std::path::Path::new(&path).is_absolute()
        || path.contains("..")
        || path.contains(['/', '\\'])
    {
        return Err(ApiError::internal("unsafe stored replay path"));
    }
    let path = state.replay_root.join(path);
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    if !metadata.is_file() || metadata.len() > resonance_protocol::replay::MAX_REPLAY_BYTES as u64 {
        return Err(ApiError::internal("invalid stored replay size"));
    }
    let bytes = tokio::fs::read(path).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"illyaverse-score-{id}.osr\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
#[path = "replay_upload_tests.rs"]
mod replay_upload_tests;

#[cfg(test)]
#[path = "score_mutation_tests.rs"]
mod score_mutation_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn supplied_mania_performance(mods: Value, state: rosu_pp::mania::ManiaScoreState) -> f64 {
        let native = ValidatedMods::parse(3, &mods).unwrap();
        rosu_pp::mania::ManiaPerformance::new(rosu_pp::mania::ManiaDifficultyAttributes {
            stars: 3.0,
            max_combo: 6,
            n_objects: 4,
            n_hold_notes: 2,
            is_convert: false,
        })
        .mods(native.mods)
        .lazer(true)
        .state(state)
        .hitresult_generator::<NativeManiaResults>()
        .calculate()
        .unwrap()
        .pp
    }

    #[test]
    fn native_mania_classic_preserves_six_head_tail_results() {
        // Original supplied attributes, independently evaluated by the pinned
        // native DLL. This checks PP arithmetic, not playable-map difficulty.
        let full = rosu_pp::mania::ManiaScoreState {
            n320: 3,
            n300: 2,
            n200: 1,
            ..Default::default()
        };
        for (mods, expected) in [
            (json!([]), 46.75619444178465),
            (json!([{"acronym":"CL"}]), 46.75619444178465),
            (
                json!([{"acronym":"CL"},{"acronym":"NF"}]),
                35.067145831338486,
            ),
            (
                json!([{"acronym":"CL"},{"acronym":"EZ"}]),
                23.378097220892325,
            ),
            (
                json!([{"acronym":"CL"},{"acronym":"HT"}]),
                46.75619444178465,
            ),
            (
                json!([{"acronym":"CL"},{"acronym":"NF"},{"acronym":"EZ"}]),
                17.533572915669243,
            ),
        ] {
            let actual = supplied_mania_performance(mods, full.clone());
            assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
        }
        let collapsed = supplied_mania_performance(
            json!([{"acronym":"CL"}]),
            rosu_pp::mania::ManiaScoreState {
                n320: 3,
                n300: 1,
                ..Default::default()
            },
        );
        assert!((collapsed - 73.88163760008085).abs() < 1e-10);
        assert!((collapsed - supplied_mania_performance(json!([]), full)).abs() > 20.0);
    }

    #[test]
    #[ignore = "requires ignored independently-built native DLL probe artifact"]
    fn native_dll_mania_performance_fixture_parity() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data-native-probe/mania-pp/vectors.json");
        let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            fixture["pinned_source_commit"],
            "6cbcae7fbb764839a66c1adf564c2e1679242072"
        );
        let vectors = fixture["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 8);
        for row in vectors {
            let h = &row["statistics"];
            let count = |key: &str| h[key].as_u64().unwrap() as u32;
            let state = rosu_pp::mania::ManiaScoreState {
                n320: count("perfect"),
                n300: count("great"),
                n200: count("good"),
                n100: count("ok"),
                n50: count("meh"),
                misses: count("miss"),
            };
            let mods: Vec<Value> = row["mods"]
                .as_array()
                .unwrap()
                .iter()
                .map(|acronym| json!({"acronym":acronym}))
                .collect();
            let pp = supplied_mania_performance(json!(mods), state);
            let expected = row["native_total_pp"].as_f64().unwrap();
            assert!(
                (pp - expected).abs() < 1e-10,
                "{}: {pp} != {expected}",
                row["label"]
            );
        }
    }

    #[test]
    fn native_mania_rejects_collapsed_or_inflated_passed_hold_results() {
        let map=b"osu file format v14\n[General]\nMode:3\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,192,1000,1,0,0:0:0:0:\n192,192,1500,1,0,0:0:0:0:\n320,192,2000,128,0,2500:0:0:0:0:\n448,192,3000,128,0,3500:0:0:0:0:\n";
        let mut score:Submission=serde_json::from_value(json!({"beatmap_id":1,"mode":3,"score":900000,"accuracy":100.0,"max_combo":4,"mods":0,"api_mods":[{"acronym":"CL"}],"passed":true,"hit_counts":{"geki":6}})).unwrap();
        let full = calculate_score(map, &score, true).unwrap();
        assert_eq!(full.scales.native_basic_maximum, Some(6));
        score.hit_counts.geki = 4;
        assert!(calculate_score(map, &score, true).is_err());
        score.hit_counts.geki = 7;
        assert!(calculate_score(map, &score, true).is_err());
        score.hit_counts.geki = 4;
        score.passed = false;
        assert_eq!(calculate_score(map, &score, true).unwrap().pp, 0.0);
        score.passed = true;
        assert!(calculate_score(map, &score, false).is_err());
        score.api_mods = None;
        assert!(calculate_score(map, &score, false).is_ok());
    }
    #[test]
    fn accuracy_uses_judgements() {
        let h = HitCounts {
            n300: 9,
            misses: 1,
            ..HitCounts::default()
        };
        assert_eq!(accuracy(0, &h).unwrap(), 90.0);
        assert!(accuracy(0, &HitCounts::default()).is_err());
    }
    #[test]
    fn public_api_source_never_claims_a_stable_estimate() {
        let map=b"osu file format v14\n[General]\nMode:0\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,0,100,1,0\n[HitObjects]\n64,192,1000,1,0\n256,192,2000,1,0\n";
        let mut score:Submission=serde_json::from_value(json!({"beatmap_id":1,"mode":0,"score":600,"accuracy":100.0,"max_combo":2,"mods":0,"passed":true,"hit_counts":{"n300":2},"client":"api"})).unwrap();
        let result = calculate_score(map, &score, false).unwrap();
        assert!(result.pp.is_finite());
        assert_eq!(result.scales.status, "unavailable");
        assert_eq!(
            result.scales.reason.as_deref(),
            Some("unsupported_score_source")
        );
        assert!(result.scales.standardised_score.is_none());
        score.client = Some("stable".into());
        assert_eq!(
            calculate_score(map, &score, false).unwrap().scales.status,
            "legacy_estimated"
        );
    }
    #[test]
    fn mania_accuracy_weights_geki() {
        let h = HitCounts {
            geki: 1,
            katu: 1,
            ..HitCounts::default()
        };
        assert!((accuracy(3, &h).unwrap() - 83.33333333333333).abs() < 1e-8);
    }
    #[test]
    fn standard_maps_convert_to_all_rulesets_with_finite_pp() {
        let bytes=b"osu file format v14\n[General]\nMode:0\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n";
        for mode in 0..=3 {
            let mut score = Submission {
                user_id: None,
                beatmap_id: 1,
                mode,
                score: 1000,
                accuracy: 100.0,
                max_combo: 2,
                mods: 0,
                api_mods: None,
                native_submission: None,
                passed: true,
                hit_counts: HitCounts {
                    n300: 2,
                    ..HitCounts::default()
                },
                client: None,
                ingestion_key: None,
            };
            assert!(calculate_pp(bytes, &score, false).unwrap().is_finite());
            score.max_combo = 1000;
            assert!(calculate_pp(bytes, &score, false).is_err());
        }
    }
    #[test]
    fn native_rate_and_difficulty_settings_change_performance() {
        let mut map = String::from(
            "osu file format v14\n[General]\nMode:0\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n",
        );
        for i in 0..100 {
            map.push_str(&format!(
                "{},{},{},1,0,0:0:0:0:\n",
                if i % 2 == 0 { 64 } else { 448 },
                64 + i % 5 * 64,
                1000 + i * 200
            ));
        }
        let mut score = Submission {
            user_id: None,
            beatmap_id: 1,
            mode: 0,
            score: 1_000_000,
            accuracy: 100.0,
            max_combo: 100,
            mods: 64,
            api_mods: Some(json!([{"acronym":"DT","settings":{"speed_change":1.2}}])),
            native_submission: None,
            passed: true,
            hit_counts: HitCounts {
                n300: 100,
                ..Default::default()
            },
            client: None,
            ingestion_key: None,
        };
        let custom = calculate_pp(map.as_bytes(), &score, true).unwrap();
        score.api_mods = None;
        let projected = calculate_pp(map.as_bytes(), &score, true).unwrap();
        assert!(custom.is_finite() && custom > 0.0);
        assert!((custom - projected).abs() > 0.01);
        score.mods = 0;
        let normal = calculate_pp(map.as_bytes(), &score, true).unwrap();
        score.api_mods = Some(json!([{"acronym":"DA","settings":{"overall_difficulty":9.0}}]));
        let adjusted = calculate_pp(map.as_bytes(), &score, true).unwrap();
        assert!((normal - adjusted).abs() > 0.01);
        score.mods = 64;
        assert!(calculate_pp(map.as_bytes(), &score, true).is_err());
        assert!(calculate_pp(map.as_bytes(), &score, false).is_err());
    }
}
