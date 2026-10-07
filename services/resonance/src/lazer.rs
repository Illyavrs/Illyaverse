use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{ConnectInfo, FromRequest, Multipart, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::{Duration, Utc};
use illya_domain::ScoreVariant;
use resonance_protocol::lazer::{
    ApiMod, OAuthGrant, OAuthToken, ScoreTokenRequest, SoloScoreSubmission,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/oauth/token", post(oauth))
        .route("/api/v2/me", get(me))
        .route("/api/v2/me/{ruleset}", get(me_mode))
        .route("/api/v2/users/{id}", get(user))
        .route("/api/v2/users/{id}/{ruleset}", get(user_mode))
        .route("/api/v2/users/{id}/scores/{kind}", get(user_scores))
        .route("/api/v2/rankings/{ruleset}/performance", get(rankings))
        .route("/api/v2/beatmaps/{id}", get(beatmap))
        .route("/api/v2/beatmaps/{id}/solo/scores", post(create_score))
        .route(
            "/api/v2/beatmaps/{id}/solo/scores/{token}",
            put(submit_score),
        )
        .route("/api/v2/scores/{id}", get(score))
        .route("/api/v2/scores/{id}/download", get(download_replay))
        .route("/api/v2/friends", get(friends).post(add_friend))
        .route("/api/v2/friends/{id}", axum::routing::delete(remove_friend))
        .route("/api/v2/blocks", get(blocks).post(add_block))
        .route("/api/v2/blocks/{id}", axum::routing::delete(remove_block))
        .route("/api/v2/seasonal-backgrounds", get(backgrounds))
        .route("/api/v2/news", get(news))
}

async fn oauth(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<OAuthToken>, ApiError> {
    common::rate_limit(
        &state,
        format!("lazer-login:{}", common::client_key(&state, peer, &headers)),
        30,
    )
    .await?;
    let grant: OAuthGrant = native_form(request).await?;
    grant.validate()?;
    let access = common::secret();
    let refresh = common::secret();
    let credentials = if let OAuthGrant::Password { username, password } = &grant {
        Some(common::verify_credentials(&state, username, password, false).await?)
    } else {
        None
    };
    let mut tx = state.pool.begin().await?;
    let player = match grant {
        OAuthGrant::Password { .. } => {
            common::lock_authenticated(
                &mut tx,
                credentials
                    .as_ref()
                    .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_grant"))?,
            )
            .await?
        }
        OAuthGrant::Refresh { refresh_token } => {
            let refresh_hash = common::token_hash(&refresh_token);
            // Discover without locking, then use the same users -> sessions lock
            // order as password reset. Re-read the token after acquiring the user
            // lock so a revoked token cannot rotate itself back into existence.
            let user_id = sqlx::query_scalar::<_, i64>("SELECT user_id FROM lazer_sessions WHERE refresh_hash=$1 AND refresh_expires_at>now()")
                .bind(&refresh_hash).fetch_optional(&mut *tx).await?
                .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_grant"))?;
            sqlx::query("SELECT id FROM users WHERE id=$1 AND NOT is_restricted FOR UPDATE")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_grant"))?;
            let row=sqlx::query("SELECT id FROM lazer_sessions WHERE refresh_hash=$1 AND user_id=$2 AND refresh_expires_at>now() FOR UPDATE").bind(&refresh_hash).bind(user_id).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::UNAUTHORIZED,"invalid_grant"))?;
            sqlx::query("DELETE FROM lazer_sessions WHERE id=$1")
                .bind(row.get::<uuid::Uuid, _>("id"))
                .execute(&mut *tx)
                .await?;
            common::account_in_tx(&mut tx, user_id).await?
        }
    };
    sqlx::query("DELETE FROM lazer_sessions WHERE user_id=$1 AND refresh_expires_at<now()")
        .bind(player.id)
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

/// osu-framework's AddParameter requests use multipart, while API callers also
/// use URL-encoded forms. Bound the entire body before processing either form.
pub(crate) async fn native_form<T: DeserializeOwned>(request: Request) -> Result<T, ApiError> {
    const MAX_BODY: usize = 8192;
    let content_type = request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        content_type.as_str(),
        "application/json" | "application/x-www-form-urlencoded" | "multipart/form-data"
    ) {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_native_form_type",
        ));
    }
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_BODY)
        .await
        .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "native_request_too_large"))?;
    if content_type == "application/json" {
        return serde_json::from_slice(&body)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"));
    }
    let fields: Vec<(String, String)> = if content_type == "multipart/form-data" {
        let request = Request::from_parts(parts, Body::from(body));
        let mut multipart = Multipart::from_request(request, &())
            .await
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?;
        let mut fields = Vec::new();
        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?
        {
            if fields.len() >= 32 || field.file_name().is_some() {
                return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"));
            }
            let name = field
                .name()
                .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?
                .to_owned();
            let value = field
                .bytes()
                .await
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?;
            let value = std::str::from_utf8(&value)
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?
                .to_owned();
            fields.push((name, value));
        }
        fields
    } else {
        serde_urlencoded::from_bytes(&body)
            .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?
    };
    let mut names = HashSet::new();
    if fields.len() > 32
        || fields.iter().any(|(name, _)| {
            name.is_empty()
                || name.len() > 64
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !names.insert(name.as_str())
        })
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"));
    }
    let encoded = serde_urlencoded::to_string(&fields)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))?;
    serde_urlencoded::from_str(&encoded)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_native_form"))
}

pub fn user_json(player: &Account) -> Value {
    json!({"id":player.id,"username":player.username,"country_code":player.country,"avatar_url":player.avatar_url,"is_active":true,"is_bot":false,"is_online":player.is_online,"is_supporter":true,"is_deleted":false,"pm_friends_only":false,"last_visit":null,"join_date":player.joined_at,"profile_colour":null,"playmode":"osu","playstyle":[],"country":{"code":player.country,"name":player.country},"cover":{"custom_url":null,"url":null,"id":"0"},"groups":[],"user_achievements":[],"kudosu":{"total":0,"available":0},"statistics":null})
}
async fn user_response(
    state: &AppState,
    player: Account,
    mode: u8,
    variant: ScoreVariant,
) -> Result<Json<Value>, ApiError> {
    let row =
        crate::score_aggregates::lifetime(&state.pool, player.id, i16::from(mode), variant).await?;
    let ranking =
        crate::score_aggregates::ranking(&state.pool, player.id, i16::from(mode), variant).await?;
    let grades=sqlx::query("SELECT count(*) FILTER(WHERE s.passed AND s.accuracy=100 AND (s.mods & 1032)=0)::bigint ss,count(*) FILTER(WHERE s.passed AND s.accuracy=100 AND (s.mods & 1032)<>0)::bigint ssh,count(*) FILTER(WHERE s.passed AND COALESCE(s.native_submission->>'rank',t.submission->>'rank',a.details->>'rank')='S')::bigint s,count(*) FILTER(WHERE s.passed AND COALESCE(s.native_submission->>'rank',t.submission->>'rank',a.details->>'rank')='SH')::bigint sh,count(*) FILTER(WHERE s.passed AND COALESCE(s.native_submission->>'rank',t.submission->>'rank',a.details->>'rank')='A')::bigint a FROM scores s LEFT JOIN lazer_score_tokens t ON t.score_id=s.id LEFT JOIN LATERAL(SELECT details FROM audit_events WHERE action='scores.stable_ingest' AND target_id=s.id::text ORDER BY id DESC LIMIT 1)a ON true WHERE s.user_id=$1 AND s.mode=$2 AND s.score_variant=$3 AND s.verification='verified'").bind(player.id).bind(i16::from(mode)).bind(variant.as_str()).fetch_one(&state.pool).await?;
    let mut value = user_json(&player);
    let total = row.get::<i64, _>("total");
    let pp = ranking.get::<f64, _>("pp");
    let is_ranked = pp > 0.0;
    value["statistics"] = json!({"level":level(total),"global_rank":if is_ranked{Some(ranking.get::<i64,_>("global_rank"))}else{None},"country_rank":if is_ranked{Some(ranking.get::<i64,_>("country_rank"))}else{None},"pp":pp,"ranked_score":ranking.get::<i64,_>("ranked_score"),"hit_accuracy":row.get::<f64,_>("accuracy"),"play_count":row.get::<i64,_>("plays"),"total_score":total,"total_hits":row.get::<i64,_>("hits"),"maximum_combo":row.get::<i32,_>("combo"),"is_ranked":is_ranked,"grade_counts":{"ss":grades.get::<i64,_>("ss"),"ssh":grades.get::<i64,_>("ssh"),"s":grades.get::<i64,_>("s"),"sh":grades.get::<i64,_>("sh"),"a":grades.get::<i64,_>("a")}});
    value["score_scale_coverage"] = json!({"total_plays":row.get::<i64,_>("plays"),"included_plays":row.get::<i64,_>("scaled_plays"),"display":"classic_with_legacy_history"});
    value["playmode"] = json!(["osu", "taiko", "fruits", "mania"][mode as usize]);
    value["score_variant"] = json!(variant);
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
            let progress = (((total.max(0) as u64 - start) as u128 * 100) / (next - start) as u128)
                .min(100) as u32;
            return json!({"current":current,"progress":progress});
        }
        start = next;
        current += 1;
    }
    let remaining = (total.max(0) as u64).saturating_sub(start);
    current += remaining / 99_999_999_999;
    let progress = ((remaining % 99_999_999_999) as u128 * 100 / 99_999_999_999).min(100) as u32;
    json!({"current":current,"progress":progress})
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
async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    user_response(
        &state,
        common::bearer(&state, &headers).await?,
        0,
        requested_variant(0, query.get("variant").map(String::as_str))?,
    )
    .await
}
async fn me_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ruleset): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    user_response(
        &state,
        common::bearer(&state, &headers).await?,
        mode(&ruleset)?,
        requested_variant(mode(&ruleset)?, query.get("variant").map(String::as_str))?,
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
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    user_response(
        &state,
        find_user(&state, &id).await?,
        0,
        requested_variant(0, query.get("variant").map(String::as_str))?,
    )
    .await
}
async fn user_mode(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, ruleset)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    common::bearer(&state, &headers).await?;
    user_response(
        &state,
        find_user(&state, &id).await?,
        mode(&ruleset)?,
        requested_variant(mode(&ruleset)?, query.get("variant").map(String::as_str))?,
    )
    .await
}

pub(crate) fn requested_variant(
    ruleset: u8,
    value: Option<&str>,
) -> Result<ScoreVariant, ApiError> {
    let mods = match value.unwrap_or("vanilla") {
        "vanilla" => 0,
        "relax" => 128,
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_score_variant")),
    };
    ScoreVariant::from_legacy(ruleset, mods)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_variant"))
}

async fn user_scores(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let viewer = common::bearer(&state, &headers).await?;
    let owner = find_user(&state, &id).await?;
    let ruleset = mode(query.get("mode").map(String::as_str).unwrap_or("osu"))?;
    let variant = requested_variant(ruleset, query.get("variant").map(String::as_str))?;
    let limit = query
        .get("limit")
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_limit"))?
        .unwrap_or(50);
    let offset = query
        .get("offset")
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_offset"))?
        .unwrap_or(0);
    if !(1..=100).contains(&limit) || !(0..=10000).contains(&offset) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_score_pagination",
        ));
    }
    let ids = match kind.as_str() {
        "best" => sqlx::query_scalar::<_,i64>("WITH best AS (SELECT DISTINCT ON(s.beatmap_id) s.id,s.pp FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.user_id=$1 AND s.mode=$2 AND s.score_variant=$3 AND s.verification='verified' AND s.passed AND s.pp_eligible AND b.status='ranked' ORDER BY s.beatmap_id,s.pp DESC,s.id) SELECT id FROM best ORDER BY pp DESC,id LIMIT $4 OFFSET $5")
            .bind(owner.id).bind(i16::from(ruleset)).bind(variant.as_str()).bind(limit).bind(offset).fetch_all(&state.pool).await?,
        "recent" => {
            let include_fails = match query.get("include_fails").map(String::as_str).unwrap_or("0") {
                "0" => false, "1" => true,
                _ => return Err(ApiError(StatusCode::BAD_REQUEST,"invalid_include_fails")),
            };
            sqlx::query_scalar::<_,i64>("SELECT id FROM scores WHERE user_id=$1 AND mode=$2 AND score_variant=$3 AND (verification='verified' OR user_id=$4) AND ($5 OR passed) ORDER BY id DESC LIMIT $6 OFFSET $7")
                .bind(owner.id).bind(i16::from(ruleset)).bind(variant.as_str()).bind(viewer.id).bind(include_fails).bind(limit).bind(offset).fetch_all(&state.pool).await?
        },
        _ => return Err(ApiError(StatusCode::BAD_REQUEST,"unsupported_score_list")),
    };
    let mut scores = Vec::with_capacity(ids.len());
    for score in ids {
        scores.push(score_json(&state, score, &viewer).await?);
    }
    Ok(Json(json!(scores)))
}

async fn rankings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ruleset): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let viewer = common::bearer(&state, &headers).await?;
    let ruleset = mode(&ruleset)?;
    let variant = requested_variant(ruleset, query.get("variant").map(String::as_str))?;
    let page = query
        .get("page")
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_ranking_page"))?
        .unwrap_or(1);
    if !(1..=201).contains(&page) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_ranking_page"));
    }
    let country = query.get("country").map(|value| value.to_ascii_uppercase());
    if country.as_ref().is_some_and(|value| {
        value.len() != 2 || !value.bytes().all(|byte| byte.is_ascii_uppercase())
    }) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_ranking_country"));
    }
    let friends = match query.get("filter").map(String::as_str).unwrap_or("all") {
        "all" => None,
        "friends" => {
            let mut friends = common::friend_ids(&state, viewer.id)
                .await?
                .into_iter()
                .map(i64::from)
                .collect::<Vec<_>>();
            friends.push(viewer.id);
            Some(friends)
        }
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_ranking_filter")),
    };
    let rows = crate::score_aggregates::ranking_page(
        &state.pool,
        i16::from(ruleset),
        variant,
        country.as_deref(),
        friends.clone(),
        (page - 1) * 50,
        50,
    )
    .await?;
    let total = if let Some(row) = rows.first() {
        row.get::<i64, _>("total")
    } else {
        // The rankings include every unrestricted user, including zero-PP
        // accounts. An empty page still needs that filtered population count;
        // ruleset and variant determine placement, not membership.
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM users WHERE NOT is_restricted
             AND ($1::text IS NULL OR country=$1)
             AND ($2::bigint[] IS NULL OR id=ANY($2))",
        )
        .bind(country.as_deref())
        .bind(friends)
        .fetch_one(&state.pool)
        .await?
    };
    let mut ranking = Vec::with_capacity(rows.len());
    for row in rows {
        let profile = user_response(
            &state,
            common::account(&state, row.get("id")).await?,
            ruleset,
            variant,
        )
        .await?
        .0;
        let mut statistics = profile["statistics"].clone();
        statistics["user"] = profile;
        statistics["score_variant"] = json!(variant);
        ranking.push(statistics);
    }
    Ok(Json(
        json!({"ranking":ranking,"total":total,"score_variant":variant,"ruleset_id":ruleset}),
    ))
}

async fn beatmap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    Ok(Json(
        map_json(
            &state,
            crate::beatmaps::resolve(&state, player.id, id).await?,
        )
        .await?,
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
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    let input: ScoreTokenRequest = native_form(request).await?;
    input.validate()?;
    if id <= 0 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_beatmap_id"));
    }
    common::rate_limit(&state, format!("score-token:{}", player.id), 60).await?;
    let internal = crate::native_maps::resolve_checksum(&state, player.id, &input.beatmap_hash)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    let map = sqlx::query(
        "SELECT id,COALESCE(official_id,id) native_id,md5,mode FROM beatmaps WHERE id=$1",
    )
    .bind(internal)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    if map.get::<i64, _>("native_id") != id {
        return Err(ApiError(StatusCode::BAD_REQUEST, "beatmap_id_mismatch"));
    }
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
    // Discovery may wait on official access. Recheck and lock the active account
    // and bearer session only for the short token write after that wait.
    let mut tx = state.pool.begin().await?;
    common::lazer_account_in_tx(&mut tx, &headers, player.id).await?;
    let token:i64=sqlx::query_scalar("INSERT INTO lazer_score_tokens(user_id,beatmap_id,ruleset_id,version_hash,expires_at,scope) VALUES($1,$2,$3,$4,$5,'solo') RETURNING id").bind(player.id).bind(internal).bind(i16::from(input.ruleset_id)).bind(input.version_hash).bind(Utc::now()+Duration::hours(6)).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":token})))
}

/// Validate native parameters before projecting legacy bits for score filters.
pub(crate) fn legacy_mods(ruleset: u8, mods: &[ApiMod]) -> Result<u32, ApiError> {
    let input = json!(mods);
    illya_domain::score_mods::ValidatedMods::parse(ruleset, &input)
        .map(|mods| mods.legacy_bits)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_mods"))
}

async fn submit_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((map_id, token_id)): Path<(i64, i64)>,
    Json(input): Json<SoloScoreSubmission>,
) -> Result<Json<Value>, ApiError> {
    let player = common::bearer(&state, &headers).await?;
    input.validate()?;
    if map_id <= 0 || token_id <= 0 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_score_token"));
    }
    let mods = illya_domain::score_mods::ValidatedMods::parse(input.ruleset_id, &json!(input.mods))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_score_mods"))?
        .legacy_bits;
    if state.service_key.len() < 32 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "internal_ingestion_not_configured",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let token=sqlx::query("SELECT t.* FROM lazer_score_tokens t JOIN beatmaps b ON b.id=t.beatmap_id WHERE t.id=$1 AND t.user_id=$2 AND t.scope='solo' AND COALESCE(b.official_id,b.id)=$3 AND (t.expires_at>now() OR t.score_id IS NOT NULL) FOR UPDATE OF t").bind(token_id).bind(player.id).bind(map_id).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"invalid_score_token"))?;
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
        let response=state.http.post(format!("{}/api/v1/internal/scores",state.sanctuary_url.trim_end_matches('/'))).header("x-illya-service-key",state.service_key.as_str()).json(&json!({"user_id":player.id,"beatmap_id":map_id,"mode":input.ruleset_id,"score":input.total_score,"accuracy":input.accuracy*100.0,"max_combo":input.max_combo,"mods":mods,"api_mods":input.mods,"passed":input.passed,"hit_counts":input.legacy_hit_counts(),"native_submission":input,"client":"lazer","ingestion_key":format!("lazer:{token_id}")})).send().await.map_err(|_|ApiError(StatusCode::SERVICE_UNAVAILABLE,"score_ingestion_unavailable"))?;
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
    if let Err(error) = crate::replay_capture::finalize(&state, player.id, token_id).await {
        tracing::warn!(
            score_id,
            code = error.1,
            "submitted score replay not published"
        );
    }
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
    score_json_from_row(state, row).await
}

/// Render a score only after the caller applies its endpoint's visibility gate.
/// Room results supply their frozen-round gate; the global query stays above.
pub(crate) async fn score_json_from_row(
    state: &AppState,
    row: sqlx::postgres::PgRow,
) -> Result<Value, ApiError> {
    let id: i64 = row.get("id");
    let owner = common::account(state, row.get("user_id")).await?;
    let verified = row.get::<String, _>("verification") == "verified";
    let stable = row.get::<String, _>("client") == "stable";
    let submission: Value = if let Some(value) = row.get::<Option<Value>, _>("native_submission") {
        value
    } else {
        sqlx::query_scalar::<_, Option<Value>>(
            "SELECT submission FROM lazer_score_tokens WHERE score_id=$1 ORDER BY id DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .flatten()
        .unwrap_or_else(|| json!({}))
    };
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
    let mut output = json!({"id":id,"user_id":owner.id,"user":user_json(&owner),"ruleset_id":row.get::<i16,_>("mode"),"beatmap_id":row.get::<i64,_>("native_map_id"),"total_score":row.get::<Option<i64>,_>("standardised_score").unwrap_or_else(||if stable{0}else{row.get::<i64,_>("score")}),"accuracy":row.get::<f64,_>("accuracy")/100.0,"max_combo":row.get::<i32,_>("max_combo"),"passed":row.get::<bool,_>("passed"),"pp":row.get::<f64,_>("pp"),"mods":submission.get("mods").cloned().unwrap_or(json!(legacy_mods)),"statistics":submission.get("statistics").cloned().unwrap_or(stats),"maximum_statistics":submission.get("maximum_statistics").cloned().unwrap_or(json!({})),"ended_at":audit.get("ended_at").cloned().unwrap_or(json!(row.get::<chrono::DateTime<Utc>,_>("created_at"))),"has_replay":row.get::<Option<String>,_>("replay_path").is_some(),"ranked":verified&&row.get::<String,_>("map_status")=="ranked"&&row.get::<bool,_>("pp_eligible"),"processed":true,"position":null,"scores_around":null,"verification":row.get::<String,_>("verification")});
    output["replay_complete"] = json!(row.get::<Option<bool>, _>("replay_complete"));
    output["score_variant"] = json!(row.get::<String, _>("score_variant"));
    if let Some(grade) = grade {
        output["rank"] = grade.clone();
    }
    if stable {
        output["legacy_score_id"] = json!(id);
        output["legacy_total_score"] = json!(row.get::<i64, _>("score"));
    }
    if let Some(total) = row.get::<Option<i64>, _>("standardised_without_mods") {
        output["total_score_without_mods"] = json!(total);
    }
    output["classic_total_score"] = json!(row.get::<Option<i64>, _>("classic_score"));
    output["score_scale_status"] = json!(row.get::<String, _>("score_scale_status"));
    output["score_scale_version"] = json!(row.get::<Option<String>, _>("score_scale_version"));
    if stable {
        if let Some(attrs) = row.get::<Option<Value>, _>("legacy_attributes")
            && let Some(maximum) = attrs.get("maximum_statistics")
        {
            output["maximum_statistics"] = maximum.clone();
        }
        if !legacy_mods.iter().any(|m| m["acronym"] == "CL") {
            let mut mods = legacy_mods;
            mods.push(json!({"acronym":"CL"}));
            output["mods"] = json!(mods);
        }
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
        "SELECT replay_path FROM scores s JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND NOT u.is_restricted AND (s.verification='verified' OR s.user_id=$2)",
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

    fn multipart(fields: &[(&str, &str)]) -> Request {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--native-fixture\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--native-fixture--\r\n");
        Request::builder()
            .header(
                "content-type",
                "multipart/form-data; boundary=native-fixture",
            )
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn native_multipart_and_urlencoded_preserve_login_and_score_fields() {
        let grant: OAuthGrant = native_form(multipart(&[
            ("grant_type", "password"),
            ("username", "Illya + 玩家"),
            ("password", "secret&=+ ü"),
            ("client_id", "5"),
            ("client_secret", "native-public-client"),
            ("scope", "*"),
        ]))
        .await
        .unwrap();
        assert!(matches!(grant, OAuthGrant::Password { username, password }
            if username == "Illya + 玩家" && password == "secret&=+ ü"));
        let fields = [
            ("beatmap_hash", "0123456789abcdef0123456789abcdef"),
            ("version_hash", "native-version"),
            ("ruleset_id", "3"),
        ];
        let encoded = serde_urlencoded::to_string(fields).unwrap();
        let urlencoded = Request::builder()
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(encoded))
            .unwrap();
        for request in [multipart(&fields), urlencoded] {
            let input: ScoreTokenRequest = native_form(request).await.unwrap();
            input.validate().unwrap();
            assert_eq!(input.ruleset_id, 3);
            assert_eq!(input.version_hash, "native-version");
            assert_eq!(input.beatmap_hash, fields[0].1);
        }
        let refresh: OAuthGrant = native_form(multipart(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", "refresh+&=value"),
            ("client_id", "5"),
        ]))
        .await
        .unwrap();
        assert!(matches!(refresh, OAuthGrant::Refresh { refresh_token }
            if refresh_token == "refresh+&=value"));
    }

    #[tokio::test]
    async fn native_forms_reject_ambiguous_files_malformed_and_oversized_bodies() {
        let duplicate = multipart(&[("grant_type", "password"), ("grant_type", "refresh_token")]);
        let encoded_duplicate = Request::builder()
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("grant_type=password&grant_type=refresh_token"))
            .unwrap();
        let file = Request::builder()
            .header("content-type", "multipart/form-data; boundary=x")
            .body(Body::from("--x\r\nContent-Disposition: form-data; name=\"password\"; filename=\"secret.txt\"\r\n\r\nsecret\r\n--x--\r\n"))
            .unwrap();
        let malformed = Request::builder()
            .header("content-type", "multipart/form-data")
            .body(Body::from("missing-boundary"))
            .unwrap();
        for request in [duplicate, encoded_duplicate, file, malformed] {
            assert_eq!(
                native_form::<OAuthGrant>(request).await.unwrap_err().0,
                StatusCode::BAD_REQUEST
            );
        }
        let oversized = Request::builder()
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("x".repeat(8193)))
            .unwrap();
        assert_eq!(
            native_form::<OAuthGrant>(oversized).await.unwrap_err().0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let unsupported = Request::builder()
            .header("content-type", "text/plain")
            .body(Body::from("grant_type=password"))
            .unwrap();
        assert_eq!(
            native_form::<OAuthGrant>(unsupported).await.unwrap_err().0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
    }
    #[test]
    fn level_uses_total_score_thresholds_and_bounded_progress() {
        assert_eq!(level(0), json!({"current":1,"progress":0}));
        assert_eq!(level(-1), json!({"current":1,"progress":0}));
        assert_eq!(level(15_599), json!({"current":1,"progress":51}));
        assert_eq!(level(30_000)["current"], 2);
        assert_eq!(level(30_000)["progress"], 0);
        assert_eq!(level(130_000)["current"], 3);
        assert_eq!(level(29_999)["current"], 1);
        assert_eq!(level(29_999)["progress"], 99);
        assert!(
            level(i64::MAX)["progress"]
                .as_u64()
                .is_some_and(|p| p < 100)
        );
    }
    #[test]
    fn pp_adapter_preserves_native_custom_speeds() {
        let mut settings = std::collections::BTreeMap::new();
        settings.insert("speed_change".into(), json!(1.2));
        assert!(
            legacy_mods(
                0,
                &[ApiMod {
                    acronym: "DT".into(),
                    settings
                }]
            )
            .is_ok()
        );
        assert!(
            legacy_mods(
                0,
                &[ApiMod {
                    acronym: "DA".into(),
                    settings: Default::default()
                }]
            )
            .is_ok()
        );
        assert_eq!(
            legacy_mods(
                0,
                &[ApiMod {
                    acronym: "NC".into(),
                    settings: Default::default()
                }]
            )
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
