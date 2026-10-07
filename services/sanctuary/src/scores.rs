use crate::{ApiError, ApiResult, AppState, auth, page};
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
    user_id: Option<i64>,
    beatmap_id: i64,
    mode: i16,
    score: i64,
    accuracy: f64,
    max_combo: u32,
    mods: u32,
    #[serde(default)]
    api_mods: Option<Value>,
    #[serde(default = "passed_default")]
    passed: bool,
    hit_counts: HitCounts,
    client: Option<String>,
    ingestion_key: Option<String>,
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
    if let Some(ref key) = input.ingestion_key {
        if key.len() > 128 || key.is_empty() {
            return Err(ApiError::bad("Invalid ingestion key."));
        }
        if let Some(existing)=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(s)-'replay_path'||jsonb_build_object('accuracy_source',CASE WHEN s.client='lazer' THEN 'client_reported' ELSE 'judgements' END) FROM scores s WHERE ingestion_key=$1").bind(key).fetch_optional(&state.db).await?{
            if existing["user_id"].as_i64()!=Some(user_id)||existing["beatmap_id"].as_i64()!=Some(input.beatmap_id){return Err(ApiError(axum::http::StatusCode::CONFLICT,"ingestion_conflict","The ingestion key belongs to another score.".into()));}return Ok(existing);
        }
    }
    if input.score < 0
        || !input.accuracy.is_finite()
        || !(0.0..=100.0).contains(&input.accuracy)
        || input.max_combo > 1_000_000
    {
        return Err(ApiError::bad("Invalid score values."));
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
    let judgement_accuracy = accuracy(input.mode, &input.hit_counts)?;
    // Lazer includes slider judgements in its native accuracy. Until a complete
    // server score-processor is implemented, preserve this reported value and
    // disclose its source; these scores remain pending moderation.
    let accuracy = if client == "lazer" {
        input.accuracy
    } else {
        judgement_accuracy
    };
    let path: String = map.get("osu_path");
    if std::path::Path::new(&path).is_absolute() || path.contains("..") {
        return Err(ApiError::internal("unsafe stored map path"));
    }
    let bytes = tokio::fs::read(state.maps_root.join(path)).await?;
    let calc = input.clone();
    let lazer = client == "lazer";
    let pp = tokio::task::spawn_blocking(move || calculate_pp(&bytes, &calc, lazer))
        .await
        .map_err(ApiError::internal)??;
    let mut value:Value=sqlx::query_scalar("INSERT INTO scores(user_id,beatmap_id,mode,score,accuracy,max_combo,mods,pp,passed,hit_counts,client,ingestion_key,api_mods,pp_eligible) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT(ingestion_key) DO UPDATE SET ingestion_key=scores.ingestion_key WHERE scores.user_id=excluded.user_id AND scores.beatmap_id=excluded.beatmap_id RETURNING to_jsonb(scores)-'replay_path'")
        .bind(user_id).bind(input.beatmap_id).bind(input.mode).bind(input.score).bind(accuracy).bind(input.max_combo as i32).bind(i64::from(input.mods)).bind(pp).bind(input.passed).bind(json!(input.hit_counts)).bind(client).bind(input.ingestion_key).bind(input.api_mods).bind(pp_eligible).fetch_optional(&state.db).await?.ok_or_else(||ApiError(axum::http::StatusCode::CONFLICT,"ingestion_conflict","The ingestion key belongs to another score.".into()))?;
    value["accuracy_source"] = json!(if client == "lazer" {
        "client_reported"
    } else {
        "judgements"
    });
    Ok(value)
}
fn calculate_pp(bytes: &[u8], score: &Submission, lazer: bool) -> ApiResult<f64> {
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
    let pp = rosu_pp::Performance::new(attrs)
        .mods(mods)
        .lazer(lazer)
        .state(state)
        .calculate()
        .pp();
    if !pp.is_finite() {
        return Err(ApiError::bad("Performance calculation was not finite."));
    }
    Ok(if score.passed { pp } else { 0.0 })
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
        sqlx::query_scalar("SELECT submission FROM lazer_score_tokens WHERE score_id=$1 AND user_id=$2 ORDER BY id LIMIT 1").bind(id).bind(user.id).fetch_optional(&mut *tx).await?.flatten()
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
