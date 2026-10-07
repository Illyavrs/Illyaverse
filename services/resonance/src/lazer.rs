use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::{Duration, Utc};
use resonance_protocol::lazer::{
    ApiMod, OAuthGrant, OAuthToken, ScoreTokenRequest, SoloScoreSubmission,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashMap, net::SocketAddr};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/token", post(oauth))
        .route("/api/v2/me", get(me))
        .route("/api/v2/me/{ruleset}", get(me_mode))
        .route("/api/v2/users/{id}", get(user))
        .route("/api/v2/users/{id}/{ruleset}", get(user_mode))
        .route("/api/v2/beatmaps/{id}", get(beatmap))
        .route("/api/v2/beatmaps/{id}/solo/scores", post(create_score))
        .route(
            "/api/v2/beatmaps/{id}/solo/scores/{token}",
            put(submit_score),
        )
        .route("/api/v2/scores/{id}", get(score))
        .route("/api/v2/scores/{id}/download", get(download_replay))
        .route("/api/v2/chat/channels", get(channels))
        .route("/api/v2/chat/channels/{id}", get(channel))
        .route(
            "/api/v2/chat/channels/{id}/messages",
            get(messages).post(send_message),
        )
        .route(
            "/api/v2/chat/channels/{id}/users/{user}",
            put(join_channel).delete(leave_channel),
        )
        .route("/api/v2/friends", get(friends).post(add_friend))
        .route("/api/v2/friends/{id}", axum::routing::delete(remove_friend))
        .route("/api/v2/blocks", get(blocks).post(add_block))
        .route("/api/v2/blocks/{id}", axum::routing::delete(remove_block))
        .route("/api/v2/notifications", get(empty_notifications))
        .route("/api/v2/seasonal-backgrounds", get(backgrounds))
        .route("/api/v2/news", get(news))
}

async fn oauth(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<OAuthToken>, ApiError> {
    common::rate_limit(
        &state,
        format!("lazer-login:{}", common::client_key(&state, peer, &headers)),
        30,
    )
    .await?;
    if body.len() > 8192 {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "oauth_request_too_large",
        ));
    }
    let grant: OAuthGrant = if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
    {
        serde_json::from_slice(&body)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_oauth_grant"))?
    } else {
        serde_urlencoded::from_bytes(&body)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_oauth_grant"))?
    };
    grant.validate()?;
    let access = common::secret();
    let refresh = common::secret();
    let mut tx = state.pool.begin().await?;
    let player = match grant {
        OAuthGrant::Password { username, password } => {
            common::authenticate(&state, &username, &password, false).await?
        }
        OAuthGrant::Refresh { refresh_token } => {
            // Lock then rotate both secrets; concurrent refreshes cannot reuse a token.
            let row=sqlx::query("SELECT id,user_id FROM lazer_sessions WHERE refresh_hash=$1 AND refresh_expires_at>now() FOR UPDATE").bind(common::token_hash(&refresh_token)).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_grant"))?;
            sqlx::query("DELETE FROM lazer_sessions WHERE id=$1")
                .bind(row.get::<uuid::Uuid, _>("id"))
                .execute(&mut *tx)
                .await?;
            common::account(&state, row.get("user_id")).await?
        }
    };
    sqlx::query("DELETE FROM lazer_sessions WHERE refresh_expires_at<now()")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO lazer_sessions(id,user_id,access_hash,refresh_hash,access_expires_at,refresh_expires_at) VALUES($1,$2,$3,$4,$5,$6)").bind(uuid::Uuid::new_v4()).bind(player.id).bind(common::token_hash(&access)).bind(common::token_hash(&refresh)).bind(Utc::now()+Duration::hours(1)).bind(Utc::now()+Duration::days(30)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(OAuthToken {
        token_type: "Bearer".into(),
        expires_in: 3600,
        access_token: access,
        refresh_token: refresh,
    }))
}

pub fn user_json(player: &Account) -> Value {
    json!({"id":player.id,"username":player.username,"country_code":player.country,"avatar_url":player.avatar_url,"is_active":true,"is_bot":false,"is_online":player.is_online,"is_supporter":true,"is_deleted":false,"pm_friends_only":false,"last_visit":null,"join_date":player.joined_at,"profile_colour":null,"playmode":"osu","playstyle":[],"country":{"code":player.country,"name":player.country},"cover":{"custom_url":null,"url":null,"id":"0"},"groups":[],"user_achievements":[],"kudosu":{"total":0,"available":0},"statistics":null})
}
async fn user_response(
    state: &AppState,
    player: Account,
    mode: u8,
) -> Result<Json<Value>, ApiError> {
    let row=sqlx::query("SELECT count(*)::bigint plays,COALESCE(sum(score),0)::bigint total,COALESCE(avg(accuracy),0)::float8 accuracy,COALESCE(max(max_combo),0)::integer combo,COALESCE(sum(COALESCE((hit_counts->>'n300')::bigint,0)+COALESCE((hit_counts->>'n100')::bigint,0)+COALESCE((hit_counts->>'misses')::bigint,0)+CASE WHEN mode<>1 THEN COALESCE((hit_counts->>'n50')::bigint,0) ELSE 0 END+CASE WHEN mode=3 THEN COALESCE((hit_counts->>'geki')::bigint,0) ELSE 0 END+CASE WHEN mode IN(2,3) THEN COALESCE((hit_counts->>'katu')::bigint,0) ELSE 0 END),0)::bigint hits FROM scores WHERE user_id=$1 AND mode=$2 AND verification='verified'").bind(player.id).bind(i16::from(mode)).fetch_one(&state.pool).await?;
    let ranking=sqlx::query("WITH best AS (SELECT s.user_id,s.beatmap_id,max(s.pp) pp,max(s.score) best_score FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.verification='verified' AND s.passed AND b.status='ranked' AND (s.mods & 10368)=0 AND s.mode=$2 GROUP BY s.user_id,s.beatmap_id),weighted AS (SELECT user_id,pp,best_score,row_number() OVER(PARTITION BY user_id ORDER BY pp DESC) n FROM best),totals AS (SELECT user_id,sum(pp*power(0.95,n-1)) pp,sum(best_score) ranked_score FROM weighted GROUP BY user_id),placed AS (SELECT u.id,COALESCE(t.pp,0)::float8 pp,COALESCE(t.ranked_score,0)::bigint ranked_score,row_number() OVER(ORDER BY COALESCE(t.pp,0) DESC,u.id) global_rank,row_number() OVER(PARTITION BY u.country ORDER BY COALESCE(t.pp,0) DESC,u.id) country_rank FROM users u LEFT JOIN totals t ON t.user_id=u.id WHERE NOT u.is_restricted) SELECT * FROM placed WHERE id=$1").bind(player.id).bind(i16::from(mode)).fetch_one(&state.pool).await?;
    let grades=sqlx::query("SELECT count(*) FILTER(WHERE s.passed AND s.accuracy=100 AND (s.mods & 1032)=0)::bigint ss,count(*) FILTER(WHERE s.passed AND s.accuracy=100 AND (s.mods & 1032)<>0)::bigint ssh,count(*) FILTER(WHERE COALESCE(t.submission->>'rank',a.details->>'rank')='S')::bigint s,count(*) FILTER(WHERE COALESCE(t.submission->>'rank',a.details->>'rank')='SH')::bigint sh,count(*) FILTER(WHERE COALESCE(t.submission->>'rank',a.details->>'rank')='A')::bigint a FROM scores s LEFT JOIN lazer_score_tokens t ON t.score_id=s.id LEFT JOIN LATERAL(SELECT details FROM audit_events WHERE action='scores.stable_ingest' AND target_id=s.id::text ORDER BY id DESC LIMIT 1)a ON true WHERE s.user_id=$1 AND s.mode=$2 AND s.verification='verified'").bind(player.id).bind(i16::from(mode)).fetch_one(&state.pool).await?;
    let mut value = user_json(&player);
    let total = row.get::<i64, _>("total");
    let pp = ranking.get::<f64, _>("pp");
    let is_ranked = pp > 0.0;
    value["statistics"] = json!({"level":level(total),"global_rank":if is_ranked{Some(ranking.get::<i64,_>("global_rank"))}else{None},"country_rank":if is_ranked{Some(ranking.get::<i64,_>("country_rank"))}else{None},"pp":pp,"ranked_score":ranking.get::<i64,_>("ranked_score"),"hit_accuracy":row.get::<f64,_>("accuracy"),"play_count":row.get::<i64,_>("plays"),"total_score":total,"total_hits":row.get::<i64,_>("hits"),"maximum_combo":row.get::<i32,_>("combo"),"is_ranked":is_ranked,"grade_counts":{"ss":grades.get::<i64,_>("ss"),"ssh":grades.get::<i64,_>("ssh"),"s":grades.get::<i64,_>("s"),"sh":grades.get::<i64,_>("sh"),"a":grades.get::<i64,_>("a")}});
    value["playmode"] = json!(["osu", "taiko", "fruits", "mania"][mode as usize]);
    Ok(Json(value))
}
fn level(total: i64) -> Value {
    let formula = |n: f64| {
        5000.0 / 3.0 * (4.0 * n.powi(3) - 3.0 * n.powi(2) - n) + 1.25 * 1.8f64.powf(n - 60.0)
    };
    let mut current = 1u64;
    let mut start = 0u64;
    while current < 100 {
        let next = start + (formula((current + 1) as f64) - formula(current as f64)).round() as u64;
        if (total.max(0) as u64) < next {
            return json!({"current":current,"progress":100.0*(total.max(0)as u64-start)as f64/(next-start)as f64});
        }
        start = next;
        current += 1;
    }
    let remaining = (total.max(0) as u64).saturating_sub(start);
    current += remaining / 99_999_999_999;
    json!({"current":current,"progress":100.0*(remaining%99_999_999_999)as f64/99_999_999_999.0})
}
fn mode(value: &str) -> Result<u8, ApiError> {
    match value {
        "osu" => Ok(0),
        "taiko" => Ok(1),
        "fruits" => Ok(2),
        "mania" => Ok(3),
        _ => Err(ApiError(StatusCode::BAD_REQUEST, "unsupported_ruleset")),
    }
}
async fn me(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    user_response(&state, common::bearer(&state, &headers).await?, 0).await
}
async fn me_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ruleset): Path<String>,
) -> Result<Json<Value>, ApiError> {
    user_response(
        &state,
        common::bearer(&state, &headers).await?,
        mode(&ruleset)?,
    )
    .await
}
async fn find_user(state: &AppState, id: &str) -> Result<Account, ApiError> {
    let numeric = if let Ok(id) = id.parse::<i64>() {
        id
    } else {
        sqlx::query_scalar("SELECT id FROM users WHERE lower(username)=lower($1)")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "user_not_found"))?
    };
    common::account(state, numeric).await
}
async fn user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    user_response(&state, find_user(&state, &id).await?, 0).await
}
async fn user_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, ruleset)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    user_response(&state, find_user(&state, &id).await?, mode(&ruleset)?).await
}

async fn beatmap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    Ok(Json(
        map_json(&state, crate::beatmaps::resolve(&state, id).await?).await?,
    ))
}
pub(crate) async fn map_json(state: &AppState, id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query("SELECT * FROM beatmaps WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    let status: String = row.get("status");
    let mode = row.get::<i16, _>("mode") as usize;
    let archive: Option<String> = row.get("archive_path");
    let native_id = row.get::<Option<i64>, _>("official_id").unwrap_or(id);
    let set_id = row.get::<Option<i64>, _>("official_set_id").unwrap_or(id);
    Ok(
        json!({"id":native_id,"beatmapset_id":set_id,"checksum":row.get::<String,_>("md5"),"version":row.get::<String,_>("version"),"mode":(["osu","taiko","fruits","mania"][mode]),"mode_int":mode,"status":status,"ranked":if status=="ranked"{1}else if status=="loved"{4}else{0},"difficulty_rating":row.get::<f64,_>("stars"),"bpm":row.get::<f64,_>("bpm"),"total_length":row.get::<i32,_>("length_seconds"),"hit_length":row.get::<i32,_>("length_seconds"),"max_combo":row.get::<i32,_>("max_combo"),"user_id":row.get::<Option<i64>,_>("uploaded_by").unwrap_or(0),"is_scoreable":true,"url":format!("/beatmaps/{id}"),"beatmapset":{"id":set_id,"artist":row.get::<String,_>("artist"),"artist_unicode":row.get::<String,_>("artist"),"title":row.get::<String,_>("title"),"title_unicode":row.get::<String,_>("title"),"creator":row.get::<String,_>("creator"),"status":status,"user_id":row.get::<Option<i64>,_>("uploaded_by").unwrap_or(0),"covers":{"cover":"","cover@2x":"","card":"","card@2x":"","list":"","list@2x":"","slimcover":"","slimcover@2x":""},"preview_url":"","download_disabled":archive.is_none(),"video":false,"storyboard":false,"availability":{"download_disabled":archive.is_none(),"more_information":null},"beatmaps":[]}}),
    )
}
async fn create_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(input): Form<ScoreTokenRequest>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    input.validate()?;
    common::rate_limit(&state, format!("score-token:{}", player.id), 60).await?;
    let map = sqlx::query("SELECT id,md5,mode FROM beatmaps WHERE (id=$1 OR official_id=$1) AND md5=$2 ORDER BY (official_id=$1) DESC LIMIT 1")
        .bind(id)
        .bind(input.beatmap_hash.to_ascii_lowercase())
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    if !map
        .get::<String, _>("md5")
        .eq_ignore_ascii_case(&input.beatmap_hash)
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "beatmap_hash_mismatch"));
    }
    let source_mode: i16 = map.get("mode");
    if source_mode != 0 && source_mode != i16::from(input.ruleset_id) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "unsupported_ruleset_conversion",
        ));
    }
    let token:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at) VALUES($1,$2,$3,$4,$5) RETURNING id").bind(player.id).bind(map.get::<i64,_>("id")).bind(i16::from(input.ruleset_id)).bind(input.version_hash).bind(Utc::now()+Duration::hours(6)).fetch_one(&state.pool).await?;
    Ok(Json(json!({"id":token})))
}

/// The current pp adapter supports legacy modifier bits at their standard speed.
/// Reject custom settings rather than silently calculate the wrong performance.
pub(crate) fn legacy_mods(mods: &[ApiMod]) -> Result<u32, ApiError> {
    let mut bits = 0;
    let mut seen = std::collections::HashSet::new();
    for modifier in mods {
        if !seen.insert(&modifier.acronym) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "duplicate_mod"));
        }
        let bit = match modifier.acronym.as_str() {
            "NF" => 1,
            "EZ" => 2,
            "HD" => 8,
            "HR" => 16,
            "SD" => 32,
            "DT" => 64,
            "RX" => 128,
            "HT" => 256,
            "NC" => 512 | 64,
            "FL" => 1024,
            "SO" => 4096,
            "AP" => 8192,
            "PF" => 16384 | 32,
            _ => {
                return Err(ApiError(
                    StatusCode::NOT_IMPLEMENTED,
                    "mod_not_supported_by_pp_adapter",
                ));
            }
        };
        for (key, value) in &modifier.settings {
            let allowed = match (modifier.acronym.as_str(), key.as_str()) {
                ("DT" | "NC", "speed_change") => value.as_f64() == Some(1.5),
                ("HT", "speed_change") => value.as_f64() == Some(0.75),
                ("DT" | "NC" | "HT", "adjust_pitch") => value.is_boolean(),
                _ => false,
            };
            if !allowed {
                return Err(ApiError(
                    StatusCode::NOT_IMPLEMENTED,
                    "custom_mod_settings_not_supported",
                ));
            }
        }
        bits |= bit;
    }
    if bits & 64 != 0 && bits & 256 != 0
        || bits & 16 != 0 && bits & 2 != 0
        || bits & 1 != 0 && bits & 32 != 0
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "incompatible_mods"));
    }
    Ok(bits)
}
fn hit_counts(input: &SoloScoreSubmission) -> Value {
    let count = |name| input.statistics.get(name).copied().unwrap_or(0);
    let (n100, n50, katu) = if input.ruleset_id == 2 {
        (
            count("large_tick_hit"),
            count("small_tick_hit"),
            count("small_tick_miss"),
        )
    } else {
        (count("ok"), count("meh"), count("good"))
    };
    json!({"n300":count("great"),"n100":n100,"n50":n50,"misses":count("miss"),"geki":count("perfect"),"katu":katu,"large_tick_hits":count("large_tick_hit"),"small_tick_hits":count("small_tick_hit"),"slider_end_hits":count("slider_tail_hit")})
}

async fn submit_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((map_id, token_id)): Path<(i64, i64)>,
    Json(input): Json<SoloScoreSubmission>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    input.validate()?;
    let mods = legacy_mods(&input.mods)?;
    if state.service_key.len() < 32 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "internal_ingestion_not_configured",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let token=sqlx::query("SELECT t.* FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1 AND t.user_id=$2 AND (b.id=$3 OR b.official_id=$3) AND (t.expires_at>now() OR t.score_id IS NOT NULL) FOR UPDATE OF t").bind(token_id).bind(player.id).bind(map_id).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"invalid_score_token"))?;
    let map_id: i64 = token.get("beatmap_id");
    if token.get::<i16, _>("ruleset_id") != i16::from(input.ruleset_id) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "score_ruleset_mismatch"));
    }
    let submission = json!(input);
    if token
        .get::<Option<Value>, _>("submission")
        .is_some_and(|previous| previous != submission)
    {
        return Err(ApiError(StatusCode::CONFLICT, "score_token_already_used"));
    }
    let score_id = if let Some(score_id) = token.get::<Option<i64>, _>("score_id") {
        score_id
    } else {
        let response=state.http.post(format!("{}/api/v1/internal/scores",state.sanctuary_url.trim_end_matches('/'))).header("x-illya-service-key",state.service_key.as_str()).json(&json!({"user_id":player.id,"beatmap_id":map_id,"mode":input.ruleset_id,"score":input.total_score,"accuracy":input.accuracy*100.0,"max_combo":input.max_combo,"mods":mods,"passed":input.passed,"hit_counts":hit_counts(&input),"client":"lazer","ingestion_key":format!("lazer:{token_id}")})).send().await.map_err(|_|ApiError(StatusCode::SERVICE_UNAVAILABLE,"score_ingestion_unavailable"))?;
        if !response.status().is_success() {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "score_rejected_by_ingestion",
            ));
        }
        let result: Value = response
            .json()
            .await
            .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "invalid_ingestion_response"))?;
        let score_id = result.get("id").and_then(Value::as_i64).ok_or(ApiError(
            StatusCode::BAD_GATEWAY,
            "invalid_ingestion_response",
        ))?;
        sqlx::query(
            "UPDATE lazer_score_tokens SET consumed_at=now(),score_id=$2,submission=$3 WHERE id=$1",
        )
        .bind(token_id)
        .bind(score_id)
        .bind(&submission)
        .execute(&mut *tx)
        .await?;
        score_id
    };
    tx.commit().await?;
    let mut result = score_json(&state, score_id, &player).await?;
    result["mods"] = json!(input.mods);
    result["statistics"] = json!(input.statistics);
    result["maximum_statistics"] = json!(input.maximum_statistics);
    result["rank"] = json!(input.rank);
    Ok(Json(result))
}
pub(crate) async fn score_json(
    state: &AppState,
    id: i64,
    player: &Account,
) -> Result<Value, ApiError> {
    let row =
        sqlx::query("SELECT s.*,COALESCE(b.official_id,b.id) native_map_id,b.status map_status FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.id=$1 AND (s.verification='verified' OR s.user_id=$2)")
            .bind(id)
            .bind(player.id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "score_not_found"))?;
    let owner = common::account(state, row.get("user_id")).await?;
    let verified = row.get::<String, _>("verification") == "verified";
    let stable = row.get::<String, _>("client") == "stable";
    let token = sqlx::query("SELECT submission FROM lazer_score_tokens WHERE score_id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    let submission: Value = token
        .and_then(|t| t.get::<Option<Value>, _>("submission"))
        .unwrap_or_else(|| json!({}));
    let audit:Value=sqlx::query_scalar("SELECT details FROM audit_events WHERE action='scores.stable_ingest' AND target_id=$1 ORDER BY id DESC LIMIT 1").bind(id.to_string()).fetch_optional(&state.pool).await?.unwrap_or_else(||json!({}));
    let grade = submission.get("rank").or_else(|| audit.get("rank"));
    let bits = row.get::<i64, _>("mods") as u32;
    let legacy_mods = rosu_mods::GameModsIntermode::from_bits(bits)
        .into_iter()
        .map(|m| json!({"acronym":m.acronym().to_string()}))
        .collect::<Vec<_>>();
    let hits: Value = row.get("hit_counts");
    let count = |key: &str| hits.get(key).and_then(Value::as_u64).unwrap_or(0);
    let stats = if row.get::<i16, _>("mode") == 2 {
        json!({"great":count("n300"),"large_tick_hit":count("n100"),"small_tick_hit":count("n50"),"small_tick_miss":count("katu"),"miss":count("misses")})
    } else {
        json!({"great":count("n300"),"ok":count("n100"),"meh":count("n50"),"perfect":count("geki"),"good":count("katu"),"miss":count("misses")})
    };
    let mut output = json!({"id":id,"user_id":owner.id,"user":user_json(&owner),"ruleset_id":row.get::<i16,_>("mode"),"beatmap_id":row.get::<i64,_>("native_map_id"),"total_score":row.get::<i64,_>("score"),"accuracy":row.get::<f64,_>("accuracy")/100.0,"max_combo":row.get::<i32,_>("max_combo"),"passed":row.get::<bool,_>("passed"),"pp":row.get::<f64,_>("pp"),"mods":submission.get("mods").cloned().unwrap_or(json!(legacy_mods)),"statistics":submission.get("statistics").cloned().unwrap_or(stats),"maximum_statistics":submission.get("maximum_statistics").cloned().unwrap_or(json!({})),"ended_at":audit.get("ended_at").cloned().unwrap_or(json!(row.get::<chrono::DateTime<Utc>,_>("created_at"))),"has_replay":row.get::<Option<String>,_>("replay_path").is_some(),"ranked":verified&&row.get::<String,_>("map_status")=="ranked"&&(bits&10368)==0,"processed":true,"position":null,"scores_around":null,"verification":row.get::<String,_>("verification")});
    if let Some(grade) = grade {
        output["rank"] = grade.clone();
    }
    if stable {
        output["legacy_score_id"] = json!(id);
        output["legacy_total_score"] = json!(row.get::<i64, _>("score"));
    }
    if let Some(total) = submission.get("total_score_without_mods") {
        output["total_score_without_mods"] = total.clone();
    }
    Ok(output)
}
async fn score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    Ok(Json(score_json(&state, id, &player).await?))
}
async fn download_replay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let path = sqlx::query_scalar::<_, Option<String>>(
        "SELECT replay_path FROM scores WHERE id=$1 AND (verification='verified' OR user_id=$2)",
    )
    .bind(id)
    .bind(player.id)
    .fetch_optional(&state.pool)
    .await?
    .flatten()
    .ok_or(ApiError(StatusCode::NOT_FOUND, "replay_not_found"))?;
    let bytes = common::stored_file(&state, &path, "replays", 16_777_216).await?;
    Ok((
        [
            ("content-type", "application/x-osu-replay"),
            ("content-disposition", "attachment; filename=score.osr"),
        ],
        bytes,
    )
        .into_response())
}

fn channel_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"channel_id":row.get::<i64,_>("id"),"name":format!("#{}",row.get::<String,_>("name")),"description":row.get::<String,_>("description"),"type":"PUBLIC","moderated":false,"message_length_limit":1000,"current_user_attributes":{"can_message":true,"can_message_error":null},"users":[]})
}
async fn channel_allowed(
    state: &AppState,
    player: &Account,
    row: &sqlx::postgres::PgRow,
) -> Result<bool, ApiError> {
    match row.get::<Option<String>,_>("required_permission").as_deref() {
        Some("lazer.room") => Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM lazer_room_members m JOIN lazer_rooms r ON r.id=m.room_id WHERE m.user_id=$1 AND r.channel_id=$2)").bind(player.id).bind(row.get::<i64,_>("id")).fetch_one(&state.pool).await?),
        Some(permission) => Ok(player.can(permission)),
        None => Ok(true),
    }
}
async fn channel_access(
    state: &AppState,
    player: &Account,
    id: i64,
) -> Result<sqlx::postgres::PgRow, ApiError> {
    let row = sqlx::query("SELECT * FROM channels WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
    if !channel_allowed(state, player, &row).await? {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "channel_permission_required",
        ));
    }
    Ok(row)
}
async fn channels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let rows = sqlx::query("SELECT * FROM channels ORDER BY id")
        .fetch_all(&state.pool)
        .await?;
    let mut channels = Vec::new();
    for row in rows {
        if channel_allowed(&state, &player, &row).await? {
            let mut channel = channel_json(&row);
            channel["current_user_attributes"]["can_message"] = json!(player.can("chat.send"));
            channels.push(channel);
        }
    }
    Ok(Json(json!(channels)))
}
async fn channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let row = channel_access(&state, &player, id).await?;
    Ok(Json(json!({"channel":channel_json(&row),"users":[]})))
}
async fn join_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user)): Path<(i64, i64)>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    if user != player.id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "cannot_join_as_another_user",
        ));
    }
    let row = channel_access(&state, &player, id).await?;
    sqlx::query("INSERT INTO lazer_channel_members(user_id,channel_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(player.id).bind(id).execute(&state.pool).await?;
    Ok(Json(channel_json(&row)))
}
async fn leave_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, user)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    if user != player.id {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "cannot_leave_as_another_user",
        ));
    }
    sqlx::query("DELETE FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2")
        .bind(player.id)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
fn message_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"message_id":row.get::<i64,_>("id"),"sender_id":row.get::<i64,_>("user_id"),"channel_id":row.get::<i64,_>("channel_id"),"content":row.get::<String,_>("content"),"is_action":false,"timestamp":row.get::<chrono::DateTime<Utc>,_>("created_at")} )
}
async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    channel_access(&state, &player, id).await?;
    let since: i64 = query.get("since").and_then(|v| v.parse().ok()).unwrap_or(0);
    let rows = sqlx::query(
        "SELECT * FROM chat_messages WHERE channel_id=$1 AND id>$2 ORDER BY id LIMIT 100",
    )
    .bind(id)
    .bind(since)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!(
        rows.iter().map(message_json).collect::<Vec<_>>()
    )))
}
#[derive(Deserialize)]
struct ChatInput {
    message: String,
    #[serde(default)]
    is_action: bool,
}
async fn send_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Form(input): Form<ChatInput>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    channel_access(&state, &player, id).await?;
    if !player.can("chat.send") {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_permission_required"));
    }
    if input.is_action {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "action_messages_unavailable",
        ));
    }
    let message = input.message.trim();
    if message.is_empty() || message.chars().count() > 1000 || message.contains('\0') {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_message_length"));
    }
    common::rate_limit(&state, format!("chat:{}", player.id), 30).await?;
    let joined: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM lazer_channel_members WHERE user_id=$1 AND channel_id=$2)",
    )
    .bind(player.id)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    if !joined {
        return Err(ApiError(StatusCode::FORBIDDEN, "join_channel_first"));
    }
    let row = sqlx::query(
        "INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,$3) RETURNING *",
    )
    .bind(id)
    .bind(player.id)
    .bind(message)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(message_json(&row)))
}
async fn relationship_json(
    state: &AppState,
    owner: i64,
    target: i64,
    kind: &str,
) -> Result<Value, ApiError> {
    let user = common::account(state, target).await?;
    let mutual: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE user_id=$1 AND target_id=$2 AND relation_type='friend')")
        .bind(target).bind(owner).fetch_one(&state.pool).await?;
    Ok(
        json!({"target_id":target,"relation_type":kind,"mutual":kind=="friend" && mutual,"target":user_json(&user)}),
    )
}
async fn relationships(
    state: &AppState,
    headers: &HeaderMap,
    kind: &str,
) -> Result<Json<Value>, ApiError> {
    let user = common::bearer(state, headers).await?;
    let ids = sqlx::query_scalar::<_,i64>("SELECT r.target_id FROM user_relationships r JOIN users u ON u.id=r.target_id WHERE r.user_id=$1 AND r.relation_type=$2 AND NOT u.is_restricted ORDER BY r.target_id LIMIT 5000")
        .bind(user.id).bind(kind).fetch_all(&state.pool).await?;
    let mut items = Vec::new();
    for id in ids {
        items.push(relationship_json(state, user.id, id, kind).await?);
    }
    Ok(Json(json!(items)))
}
async fn friends(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    relationships(&state, &headers, "friend").await
}
async fn blocks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    relationships(&state, &headers, "block").await
}
#[derive(Deserialize)]
struct RelationshipTarget {
    target: i64,
}
async fn add_friend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<RelationshipTarget>,
) -> Result<Json<Value>, ApiError> {
    let owner = common::bearer(&state, &headers).await?;
    common::change_relationship(&state, owner.id, input.target, "friend", false).await?;
    Ok(Json(
        json!({"user_relation":relationship_json(&state, owner.id, input.target, "friend").await?}),
    ))
}
async fn add_block(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<RelationshipTarget>,
) -> Result<Json<Value>, ApiError> {
    let owner = common::bearer(&state, &headers).await?;
    common::change_relationship(&state, owner.id, input.target, "block", false).await?;
    Ok(Json(
        json!({"user_relation":relationship_json(&state, owner.id, input.target, "block").await?}),
    ))
}
async fn remove_friend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let owner = common::bearer(&state, &headers).await?;
    common::change_relationship(&state, owner.id, id, "friend", true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove_block(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let owner = common::bearer(&state, &headers).await?;
    common::change_relationship(&state, owner.id, id, "block", true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn empty_notifications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    Ok(Json(
        json!({"notifications":[],"stacks":[],"unread_count":0}),
    ))
}
async fn backgrounds() -> Json<Value> {
    Json(json!({"ends_at":"2099-01-01T00:00:00Z","backgrounds":[]}))
}
async fn news() -> Json<Value> {
    Json(
        json!({"news_posts":[],"news_sidebar":{"current_year":2026,"years":[],"news_posts":[]},"cursor":null,"search":{"limit":12,"sort":"published_desc"}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn level_uses_total_score_thresholds_and_bounded_progress() {
        assert_eq!(level(0), json!({"current":1,"progress":0.0}));
        assert_eq!(level(30_000)["current"], 2);
        assert_eq!(level(130_000)["current"], 3);
        assert_eq!(level(29_999)["current"], 1);
        assert!(
            level(i64::MAX)["progress"]
                .as_f64()
                .is_some_and(|p| (0.0..100.0).contains(&p))
        );
    }
    #[test]
    fn pp_adapter_rejects_unknown_mods_and_custom_speeds() {
        let mut settings = std::collections::BTreeMap::new();
        settings.insert("speed_change".into(), json!(1.2));
        assert!(
            legacy_mods(&[ApiMod {
                acronym: "DT".into(),
                settings
            }])
            .is_err()
        );
        assert!(
            legacy_mods(&[ApiMod {
                acronym: "DA".into(),
                settings: Default::default()
            }])
            .is_err()
        );
        assert_eq!(
            legacy_mods(&[ApiMod {
                acronym: "NC".into(),
                settings: Default::default()
            }])
            .unwrap(),
            576
        );
    }
    #[test]
    fn form_grant_parses_native_client_fields() {
        let grant:OAuthGrant=serde_urlencoded::from_str("grant_type=password&username=Illya&password=secret&client_id=5&client_secret=public-client-value&scope=%2A").unwrap();
        assert!(matches!(grant,OAuthGrant::Password{username,..}if username=="Illya"));
    }
}
