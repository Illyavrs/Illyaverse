use crate::{ApiError, ApiResult, AppState, auth, page, score_scales};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use illya_domain::{HitCounts, Pagination, has_permission, score_mods::ValidatedMods};
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
    verification: Option<String>,
}
pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ScoreQuery>,
) -> ApiResult {
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
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM scores s JOIN users u ON u.id=s.user_id WHERE ($1::bigint IS NULL OR user_id=$1) AND ($2::bigint IS NULL OR beatmap_id=$2) AND ($3::smallint IS NULL OR mode=$3) AND ($4::text IS NULL OR verification=$4) AND (verification='verified' OR user_id=$5 OR $6) AND (NOT u.is_restricted OR $6)")
        .bind(query.user_id).bind(query.beatmap_id).bind(query.mode).bind(&query.verification).bind(own_id).bind(all).fetch_one(&state.db).await?;
    let items=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s)-'replay_path' || jsonb_build_object('username',u.username,'title',b.title,'version',b.version,'has_replay',s.replay_path IS NOT NULL,'accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM scores s JOIN users u ON u.id=s.user_id JOIN beatmaps b ON b.id=s.beatmap_id WHERE ($1::bigint IS NULL OR user_id=$1) AND ($2::bigint IS NULL OR beatmap_id=$2) AND ($3::smallint IS NULL OR s.mode=$3) AND ($4::text IS NULL OR verification=$4) AND (verification='verified' OR user_id=$5 OR $6) AND (NOT u.is_restricted OR $6) ORDER BY s.created_at DESC,s.id DESC LIMIT $7 OFFSET $8")
        .bind(query.user_id).bind(query.beatmap_id).bind(query.mode).bind(query.verification).bind(own_id).bind(all).bind(size).bind(offset).fetch_all(&state.db).await?;
    Ok(page(items, total, p, size))
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
    country: Option<String>,
}
pub async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> ApiResult {
    if query.mode.is_some_and(|m| !(0..=3).contains(&m)) {
        return Err(ApiError::bad("Unsupported mode."));
    }
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
    let items=sqlx::query_scalar::<_,Value>("WITH best AS (SELECT s.user_id,s.beatmap_id,max(s.pp) pp FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.verification='verified' AND s.passed AND b.status='ranked' AND s.pp_eligible AND ($1::smallint IS NULL OR s.mode=$1) GROUP BY s.user_id,s.beatmap_id),weighted AS (SELECT user_id,pp,row_number() OVER(PARTITION BY user_id ORDER BY pp DESC) n FROM best),totals AS (SELECT user_id,sum(pp*power(0.95,n-1)) pp FROM weighted GROUP BY user_id),ranked AS (SELECT u.id,u.username,u.country,u.avatar_url,COALESCE(t.pp,0) pp,row_number() OVER(ORDER BY COALESCE(t.pp,0) DESC,u.id) rank,(SELECT count(*) FROM scores s WHERE s.user_id=u.id AND verification='verified' AND ($1::smallint IS NULL OR s.mode=$1)) play_count,COALESCE((SELECT avg(accuracy) FROM scores s WHERE s.user_id=u.id AND verification='verified' AND ($1::smallint IS NULL OR s.mode=$1)),0) accuracy FROM users u LEFT JOIN totals t ON t.user_id=u.id WHERE NOT u.is_restricted) SELECT to_jsonb(r) FROM ranked r WHERE ($2::text IS NULL OR country=$2) ORDER BY rank LIMIT $3 OFFSET $4")
        .bind(query.mode).bind(query.country).bind(size).bind(offset).fetch_all(&state.db).await?;
    Ok(page(items, total, p, size))
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
    if let Some(ref key) = input.ingestion_key {
        if key.len() > 128 || key.is_empty() {
            return Err(ApiError::bad("Invalid ingestion key."));
        }
        if let Some(existing)=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s)-'replay_path'||jsonb_build_object('accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM scores s WHERE ingestion_key=$1").bind(key).fetch_optional(&state.db).await?{
            if existing["user_id"].as_i64()!=Some(user_id)||existing["beatmap_id"].as_i64()!=Some(input.beatmap_id){return Err(ApiError(axum::http::StatusCode::CONFLICT,"ingestion_conflict","The ingestion key belongs to another score.".into()));}return Ok(existing);
        }
    }
    let pp_eligible = if let Some(ref native) = input.api_mods {
        if client != "lazer" {
            return Err(ApiError::bad(
                "Structured mods require native lazer ingestion.",
            ));
        }
        let validated = ValidatedMods::parse(input.mode as u8, native).map_err(ApiError::bad)?;
        if validated.legacy_bits != input.mods {
            return Err(ApiError::bad(
                "Legacy mod projection does not match native mods.",
            ));
        }
        validated.pp_eligible
    } else {
        input.mods & 10368 == 0
    };
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
    let map = sqlx::query("SELECT osu_path,mode,max_combo FROM beatmaps WHERE id=$1")
        .bind(input.beatmap_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(ApiError::missing)?;
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
    let mut value:Value=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,pp_eligible,standardised_score,standardised_without_mods,classic_score,native_submission,native_basic_maximum,score_scale_status,score_scale_version,score_scale_reason,legacy_attributes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23) ON CONFLICT(ingestion_key) DO UPDATE SET ingestion_key=scores.ingestion_key WHERE scores.user_id=excluded.user_id AND scores.beatmap_id=excluded.beatmap_id RETURNING to_jsonb(scores)-'replay_path'")
        .bind(user_id).bind(input.beatmap_id).bind(input.mode).bind(input.score).bind(accuracy).bind(input.max_combo as i32).bind(i64::from(input.mods)).bind(calculated.pp).bind(input.passed).bind(json!(input.hit_counts)).bind(client).bind(input.ingestion_key).bind(input.api_mods).bind(pp_eligible).bind(scales.standardised_score).bind(scales.standardised_without_mods).bind(scales.classic_score).bind(scales.native_submission).bind(scales.native_basic_maximum).bind(scales.status).bind(scales.version).bind(scales.reason).bind(scales.legacy_attributes).fetch_optional(&state.db).await?.ok_or_else(||ApiError(axum::http::StatusCode::CONFLICT,"ingestion_conflict","The ingestion key belongs to another score.".into()))?;
    value["accuracy_source"] = json!(if client == "lazer" {
        "client_reported"
    } else {
        "judgements"
    });
    Ok(value)
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
    let scales = if lazer {
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
    Ok(Json(ingest(&state, user.id, input, "api").await?))
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
    Ok(Json(ingest(&state, user_id, input, &client).await?))
}
pub async fn upload_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> ApiResult {
    let user = auth::require(&state, &headers).await?;
    let _permit = state.upload_permit()?;
    let score=sqlx::query("SELECT s.*,b.md5,u.username FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1").bind(id).fetch_optional(&state.db).await?.ok_or_else(ApiError::missing)?;
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
    let mut tx = state.db.begin().await?;
    let score=sqlx::query("SELECT s.*,b.md5,u.username FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND s.user_id=$2 FOR UPDATE OF s").bind(id).bind(user.id).fetch_optional(&mut *tx).await?.ok_or_else(ApiError::missing)?;
    let lazer = score.get::<String, _>("client") == "lazer";
    let submission: Option<Value> = if lazer {
        let persisted: Option<Value> = score.get("native_submission");
        if persisted.is_some() {
            persisted
        } else {
            sqlx::query_scalar("SELECT submission FROM lazer_score_tokens WHERE score_id=$1 AND user_id=$2 ORDER BY id LIMIT 1").bind(id).bind(user.id).fetch_optional(&mut *tx).await?.flatten()
        }
    } else {
        None
    };
    let expected_user = score.get::<i64, _>("user_id");
    let username = score.get::<String, _>("username");
    let md5 = score.get::<String, _>("md5");
    let mode = score.get::<i16, _>("mode");
    let total = score.get::<i64, _>("score");
    let combo = score.get::<i32, _>("max_combo");
    let mods = score.get::<i64, _>("mods");
    let hits: Value = score.get("hit_counts");
    let (bytes, sha) = tokio::task::spawn_blocking(move || -> ApiResult<(Vec<u8>, String)> {
        use sha2::{Digest, Sha256};
        if lazer {
            let input: resonance_protocol::lazer::SoloScoreSubmission = serde_json::from_value(
                submission.ok_or_else(|| ApiError::bad("Stored lazer submission is required."))?,
            )
            .map_err(ApiError::internal)?;
            let replay = resonance_protocol::lazer_replay::ExtendedReplay::parse(&bytes)
                .map_err(|e| ApiError::bad(format!("Invalid extended replay: {e}")))?;
            let binding = resonance_protocol::lazer_replay::ReplayScore {
                online_id: id,
                user_id: expected_user,
                username: &username,
                beatmap_md5: &md5,
                client_version: "",
                mods_bits: mods as u32,
                timestamp_ticks: 0,
                submission: &input,
            };
            replay
                .matches(&binding)
                .map_err(|_| ApiError::bad("Replay metadata does not match the stored score."))?;
        } else {
            let replay = resonance_protocol::replay::LegacyReplay::parse(&bytes)
                .map_err(|e| ApiError::bad(format!("Invalid replay container: {e}")))?;
            let h: HitCounts = serde_json::from_value(hits).map_err(ApiError::internal)?;
            if replay.beatmap_md5 != md5
                || replay.player_name != username
                || i16::from(replay.mode) != mode
                || i64::from(replay.total_score) != total
                || i32::from(replay.max_combo) != combo
                || i64::from(replay.mods) != mods
                || replay.hit_counts.map(u32::from)
                    != [h.n300, h.n100, h.n50, h.geki, h.katu, h.misses]
            {
                return Err(ApiError::bad(
                    "Replay metadata does not match the stored score.",
                ));
            }
        }
        let sha = format!("{:x}", Sha256::digest(&bytes));
        Ok((bytes, sha))
    })
    .await
    .map_err(ApiError::internal)??;
    if let Some(previous) = score.get::<Option<String>, _>("replay_path") {
        let existing = if let Some(sha) = score.get::<Option<String>, _>("replay_sha256") {
            sha
        } else {
            use sha2::{Digest, Sha256};
            if previous.contains(['/', '\\']) || previous.contains("..") {
                return Err(ApiError::internal("unsafe stored replay path"));
            }
            let stored = tokio::fs::read(state.replay_root.join(previous)).await?;
            format!("{:x}", Sha256::digest(stored))
        };
        if existing != sha {
            return Err(ApiError(
                axum::http::StatusCode::CONFLICT,
                "replay_already_uploaded",
                "This score already has a different replay.".into(),
            ));
        }
        return Ok(Json(
            json!({"ok":true,"verification":score.get::<String,_>("verification"),"has_replay":true,"idempotent":true}),
        ));
    }
    let path = format!("{}-{}.osr", id, Uuid::new_v4());
    let temporary = state.replay_root.join(format!("{path}.tmp"));
    let destination = state.replay_root.join(&path);
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, &destination).await?;
    let result:ApiResult<()>=async {sqlx::query("UPDATE scores SET replay_path=$2,replay_sha256=$3,replay_format=$4,replay_complete=NULL,verification='pending' WHERE id=$1")
        .bind(id)
        .bind(&path)
        .bind(&sha).bind(if lazer {"lazer"} else {"legacy"})
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(actor_id,action,target_type,target_id,details) VALUES($1,'scores.replay_upload','score',$2,$3)").bind(user.id).bind(id.to_string()).bind(json!({"metadata_matches":true,"gameplay_verified":false,"sha256":sha,"format":if lazer {"lazer"} else {"legacy"}})).execute(&mut *tx).await?;tx.commit().await?;Ok(())}.await;
    // Retain the full file on an uncertain database commit acknowledgement;
    // deleting it could invalidate a reference that actually committed.
    result?;
    Ok(Json(
        json!({"ok":true,"verification":"pending","has_replay":true}),
    ))
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
