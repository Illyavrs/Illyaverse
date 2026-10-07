use crate::common::{self, Account, ApiError, AppState};
use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use resonance_protocol::stable::{
    self as wire, ChatMessage, LoginRequest, Presence, Reader, UserAction, UserStats,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashMap, net::SocketAddr};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/web/osu-getfriends.php", get(friends))
        .route("/web/osu-getreplay.php", get(replay))
        .route("/web/osu-osz2-getscores.php", get(leaderboard))
        .route("/web/osu-search.php", get(search))
        .route("/web/osu-search-set.php", get(search_set))
        .route("/web/osu-getbeatmapinfo.php", post(beatmap_info))
        .route(
            "/web/osu-submit-modular-selector.php",
            post(crate::stable_scores::submit)
                .layer(axum::extract::DefaultBodyLimit::max(20_971_520)),
        )
        .route("/osu/{id}", get(beatmap_file))
        .route("/d/{id}", get(beatmap_archive))
}

fn binary(body: Vec<u8>, token: Option<&str>) -> Response {
    let mut response = (
        [
            ("content-type", "application/octet-stream"),
            ("cho-protocol", "19"),
        ],
        body,
    )
        .into_response();
    if let Some(token) = token.and_then(|v| v.parse().ok()) {
        response.headers_mut().insert("cho-token", token);
    }
    response
}
fn append(
    output: &mut Vec<u8>,
    packet: resonance_protocol::Result<Vec<u8>>,
) -> Result<(), ApiError> {
    output.extend(packet?);
    Ok(())
}
fn id32(id: i64) -> Result<i32, ApiError> {
    i32::try_from(id)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stable_user_id_overflow"))
}
fn failure(code: i32, message: &str) -> Response {
    let mut bytes = wire::int_packet(wire::server::USER_ID, code).unwrap_or_default();
    bytes.extend(wire::string_packet(wire::server::NOTIFICATION, message).unwrap_or_default());
    binary(bytes, Some("no"))
}

pub async fn bancho(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    match headers.get("osu-token") {
        None => login(&state, &common::client_key(&state, peer, &headers), &body).await,
        Some(token) => {
            let token = token
                .to_str()
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_session_token"))?;
            if token.len() > 128 {
                return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_session_token"));
            }
            poll(&state, token, &body).await
        }
    }
}

async fn login(state: &AppState, client: &str, body: &[u8]) -> Result<Response, ApiError> {
    common::rate_limit(state, format!("stable-login:{client}"), 20).await?;
    let login = match LoginRequest::parse(body) {
        Ok(value) => value,
        Err(_) => return Ok(failure(-1, "Invalid login request.")),
    };
    let player = match common::authenticate(state, &login.username, &login.password_md5, true).await
    {
        Ok(player) => player,
        Err(ApiError(StatusCode::UNAUTHORIZED, _)) => {
            return Ok(failure(-1, "Incorrect credentials or restricted account."));
        }
        Err(error) => return Err(error),
    };
    id32(player.id)?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("DELETE FROM bancho_sessions WHERE expires_at<now() OR user_id=$1")
        .bind(player.id)
        .execute(&mut *tx)
        .await?;
    crate::multiplayer::disconnect(&mut tx, player.id).await?;
    let channel_rows =
        sqlx::query("SELECT id,name,description,required_permission FROM channels ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
    let mut channel_ids = Vec::new();
    let mut output = Vec::new();
    append(
        &mut output,
        wire::int_packet(wire::server::USER_ID, id32(player.id)?),
    )?;
    append(
        &mut output,
        wire::int_packet(wire::server::PROTOCOL_VERSION, 19),
    )?;
    // No administrative wire flags grant server permissions.
    append(&mut output, wire::int_packet(wire::server::PRIVILEGES, 5))?;
    append(&mut output, wire::int_packet(wire::server::SILENCE_END, 0))?;
    append(
        &mut output,
        wire::friends_list(&common::friend_ids(state, player.id).await?),
    )?;
    for channel in channel_rows {
        let permission: Option<String> = channel.get("required_permission");
        if permission
            .as_deref()
            .is_some_and(|p| p == "lazer.room" || !player.can(p))
        {
            continue;
        }
        let name = format!("#{}", channel.get::<String, _>("name"));
        let topic: String = channel.get("description");
        channel_ids.push(channel.get::<i64, _>("id"));
        append(&mut output, wire::channel_info(&name, &topic, 0))?;
        append(
            &mut output,
            wire::channel_packet(wire::server::CHANNEL_AUTO_JOIN, &name, &topic, 0),
        )?;
    }
    append(
        &mut output,
        wire::encode_packet(wire::server::CHANNEL_INFO_END, &[]),
    )?;
    let last_chat: i64 = sqlx::query_scalar("SELECT COALESCE(max(id),0) FROM chat_messages")
        .fetch_one(&mut *tx)
        .await?;
    let last_dm: i64 =
        sqlx::query_scalar("SELECT COALESCE(max(id),0) FROM protocol_private_messages")
            .fetch_one(&mut *tx)
            .await?;
    let token = common::secret();
    sqlx::query("INSERT INTO bancho_sessions(id,token_hash,user_id,expires_at,last_chat_id,last_dm_id,channels,utc_offset,block_non_friend_dms,action,client_version,client_hash_hash) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(uuid::Uuid::new_v4()).bind(common::token_hash(&token)).bind(player.id).bind(Utc::now()+Duration::seconds(180)).bind(last_chat).bind(last_dm).bind(&channel_ids).bind(i16::from(login.utc_offset)).bind(login.block_non_friend_dms).bind(json!(UserAction::default())).bind(&login.client_version).bind(common::token_hash(&login.client_hashes)).execute(&mut *tx).await?;
    tx.commit().await?;
    append_presence(&mut output, &player, login.utc_offset, 0)?;
    append(
        &mut output,
        stats(state, &player, UserAction::default()).await?.packet(),
    )?;
    let online = sqlx::query("SELECT user_id,utc_offset,action FROM bancho_sessions WHERE expires_at>now() AND user_id<>$1 LIMIT 1000").bind(player.id).fetch_all(&state.pool).await?;
    for session in online {
        if let Ok(other) = common::account(state, session.get("user_id")).await {
            let action: UserAction =
                serde_json::from_value(session.get("action")).unwrap_or_default();
            append_presence(
                &mut output,
                &other,
                session.get::<i16, _>("utc_offset") as i8,
                action.mode,
            )?;
        }
    }
    append(
        &mut output,
        wire::string_packet(
            wire::server::NOTIFICATION,
            "Welcome to Illyaverse. Submitted scores await moderator review before entering rankings.",
        ),
    )?;
    Ok(binary(output, Some(&token)))
}

fn append_presence(
    output: &mut Vec<u8>,
    player: &Account,
    utc_offset: i8,
    mode: u8,
) -> Result<(), ApiError> {
    append(
        output,
        Presence {
            user_id: id32(player.id)?,
            username: player.username.clone(),
            utc_offset,
            country_code: 0,
            client_privileges: 1,
            mode,
            longitude: 0.0,
            latitude: 0.0,
            rank: 0,
        }
        .packet(),
    )
}

async fn stats(
    state: &AppState,
    player: &Account,
    action: UserAction,
) -> Result<UserStats, ApiError> {
    let mode = i16::from(action.mode);
    let row = sqlx::query("SELECT COALESCE(sum(score),0)::bigint AS total,COALESCE(avg(accuracy),0)::float8 AS accuracy,count(*)::bigint AS plays FROM scores WHERE user_id=$1 AND mode=$2 AND verification='verified' AND passed")
        .bind(player.id).bind(mode).fetch_one(&state.pool).await?;
    let ranked: i64 = sqlx::query_scalar("SELECT COALESCE(sum(best),0)::bigint FROM (SELECT max(s.score) AS best FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.user_id=$1 AND s.mode=$2 AND s.verification='verified' AND s.passed AND b.status='ranked' AND (s.mods & 10368)=0 GROUP BY s.beatmap_id) t").bind(player.id).bind(mode).fetch_one(&state.pool).await?;
    let ranking = sqlx::query("WITH best AS (SELECT s.user_id,s.beatmap_id,max(s.pp) pp FROM scores s JOIN beatmaps b ON b.id=s.beatmap_id WHERE s.mode=$2 AND s.verification='verified' AND s.passed AND b.status='ranked' AND (s.mods & 10368)=0 GROUP BY s.user_id,s.beatmap_id),weighted AS (SELECT user_id,pp,row_number() OVER(PARTITION BY user_id ORDER BY pp DESC) n FROM best),totals AS (SELECT user_id,sum(pp*power(0.95,n-1)) pp FROM weighted GROUP BY user_id),placed AS (SELECT u.id,COALESCE(t.pp,0)::float8 pp,row_number() OVER(ORDER BY COALESCE(t.pp,0) DESC,u.id) placement FROM users u LEFT JOIN totals t ON t.user_id=u.id WHERE NOT u.is_restricted) SELECT pp,placement FROM placed WHERE id=$1")
        .bind(player.id).bind(mode).fetch_one(&state.pool).await?;
    let pp: f64 = ranking.get("pp");
    Ok(UserStats {
        user_id: id32(player.id)?,
        action,
        ranked_score: ranked,
        accuracy: (row.get::<f64, _>("accuracy") / 100.0) as f32,
        play_count: row.get::<i64, _>("plays").min(i64::from(i32::MAX)) as i32,
        total_score: row.get("total"),
        rank: ranking.get::<i64, _>("placement").min(i64::from(i32::MAX)) as i32,
        pp: pp.round().clamp(0.0, 65535.0) as u16,
    })
}

async fn poll(state: &AppState, token: &str, body: &[u8]) -> Result<Response, ApiError> {
    let packets = wire::decode_packets(body)?;
    let mut tx = state.pool.begin().await?;
    let session = sqlx::query("SELECT user_id,channels,last_chat_id,last_dm_id,utc_offset,action FROM bancho_sessions WHERE token_hash=$1 AND expires_at>now() FOR UPDATE").bind(common::token_hash(token)).fetch_optional(&mut *tx).await?;
    let Some(session) = session else {
        let mut output = wire::int_packet(wire::server::RESTART, 0)?;
        append(
            &mut output,
            wire::string_packet(
                wire::server::NOTIFICATION,
                "Session expired. Please reconnect.",
            ),
        )?;
        return Ok(binary(output, None));
    };
    let player = common::account(state, session.get("user_id")).await?;
    crate::multiplayer::refresh_spectator(&mut tx, player.id).await?;
    let mut channels: Vec<i64> = session.get("channels");
    let mut action: UserAction = serde_json::from_value(session.get("action")).unwrap_or_default();
    let mut output = Vec::new();
    for packet in packets {
        if crate::multiplayer::handle(
            state,
            &mut tx,
            &player,
            packet.id,
            packet.payload,
            &mut output,
        )
        .await?
        {
            continue;
        }
        match packet.id {
            wire::client::LOGOUT => {
                crate::multiplayer::disconnect(&mut tx, player.id).await?;
                sqlx::query("DELETE FROM bancho_sessions WHERE token_hash=$1")
                    .bind(common::token_hash(token))
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                return Ok(binary(Vec::new(), None));
            }
            wire::client::PING => {
                if !packet.payload.is_empty() {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_ping"));
                }
                append(&mut output, wire::encode_packet(wire::server::PONG, &[]))?;
            }
            wire::client::CHANGE_ACTION => {
                action = UserAction::decode(packet.payload)?;
                append(
                    &mut output,
                    stats(state, &player, action.clone()).await?.packet(),
                )?;
            }
            wire::client::REQUEST_STATUS_UPDATE => {
                append(
                    &mut output,
                    stats(state, &player, action.clone()).await?.packet(),
                )?;
            }
            wire::client::FRIEND_ADD | wire::client::FRIEND_REMOVE => {
                let mut reader = Reader::new(packet.payload);
                let target = i64::from(reader.i32()?);
                reader.finish()?;
                common::change_relationship(
                    state,
                    player.id,
                    target,
                    "friend",
                    packet.id == wire::client::FRIEND_REMOVE,
                )
                .await?;
            }
            wire::client::TOGGLE_BLOCK_NON_FRIEND_DMS => {
                let mut reader = Reader::new(packet.payload);
                let blocked = reader.i32()?;
                reader.finish()?;
                if !matches!(blocked, 0 | 1) {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_dm_setting"));
                }
                sqlx::query("UPDATE bancho_sessions SET block_non_friend_dms=$2 WHERE user_id=$1")
                    .bind(player.id)
                    .bind(blocked == 1)
                    .execute(&mut *tx)
                    .await?;
            }
            wire::client::CHANNEL_JOIN | wire::client::CHANNEL_PART => {
                let mut reader = Reader::new(packet.payload);
                let name = reader.string()?.to_owned();
                reader.finish()?;
                if let Some(row) =
                    sqlx::query("SELECT id,required_permission FROM channels WHERE name=$1")
                        .bind(name.trim_start_matches('#'))
                        .fetch_optional(&mut *tx)
                        .await?
                {
                    let id: i64 = row.get("id");
                    let permission: Option<String> = row.get("required_permission");
                    if packet.id == wire::client::CHANNEL_PART {
                        channels.retain(|value| *value != id);
                    } else if permission
                        .as_deref()
                        .is_none_or(|p| p != "lazer.room" && player.can(p))
                    {
                        if !channels.contains(&id) {
                            channels.push(id);
                        }
                        append(
                            &mut output,
                            wire::string_packet(wire::server::CHANNEL_JOIN_SUCCESS, &name),
                        )?;
                    }
                }
            }
            wire::client::SEND_PUBLIC_MESSAGE | wire::client::SEND_PRIVATE_MESSAGE => {
                let message = ChatMessage::decode(packet.payload)?;
                if !player.can("chat.send") {
                    return Err(ApiError(StatusCode::FORBIDDEN, "chat_permission_required"));
                }
                let content = message.text.trim();
                if content.is_empty() || content.chars().count() > 1000 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_chat_length"));
                }
                common::rate_limit(state, format!("chat:{}", player.id), 30).await?;
                if packet.id == wire::client::SEND_PUBLIC_MESSAGE {
                    if crate::multiplayer::special_chat(
                        &mut tx,
                        &player,
                        &message.recipient,
                        content,
                    )
                    .await?
                    {
                        continue;
                    }
                    if let Some(row) =
                        sqlx::query("SELECT id,required_permission FROM channels WHERE name=$1")
                            .bind(message.recipient.trim_start_matches('#'))
                            .fetch_optional(&mut *tx)
                            .await?
                    {
                        let id: i64 = row.get("id");
                        let permission: Option<String> = row.get("required_permission");
                        if channels.contains(&id)
                            && permission
                                .as_deref()
                                .is_none_or(|p| p != "lazer.room" && player.can(p))
                        {
                            sqlx::query("INSERT INTO chat_messages(channel_id,user_id,content) VALUES($1,$2,$3)").bind(id).bind(player.id).bind(content).execute(&mut *tx).await?;
                        }
                    }
                } else {
                    let recipient = sqlx::query("SELECT u.id,b.block_non_friend_dms,EXISTS(SELECT 1 FROM user_relationships WHERE user_id=u.id AND target_id=$2 AND relation_type='friend') AS friend,EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=u.id AND target_id=$2) OR (user_id=$2 AND target_id=u.id)) AND relation_type='block') AS blocked FROM users u JOIN bancho_sessions b ON b.user_id=u.id WHERE lower(u.username)=lower($1) AND NOT u.is_restricted AND b.expires_at>now()").bind(&message.recipient).bind(player.id).fetch_optional(&mut *tx).await?;
                    if let Some(row) = recipient {
                        if !row.get::<bool, _>("blocked")
                            && (!row.get::<bool, _>("block_non_friend_dms")
                                || row.get::<bool, _>("friend"))
                        {
                            sqlx::query("INSERT INTO protocol_private_messages(sender_id,recipient_id,content) VALUES($1,$2,$3)").bind(player.id).bind(row.get::<i64,_>("id")).bind(content).execute(&mut *tx).await?;
                        } else {
                            append(
                                &mut output,
                                wire::encode_packet(
                                    wire::server::USER_DM_BLOCKED,
                                    &ChatMessage {
                                        sender: String::new(),
                                        text: String::new(),
                                        recipient: message.recipient.clone(),
                                        sender_id: 0,
                                    }
                                    .encode()?,
                                ),
                            )?;
                        }
                    } else {
                        append(
                            &mut output,
                            wire::string_packet(
                                wire::server::NOTIFICATION,
                                "This player is offline.",
                            ),
                        )?;
                    }
                }
            }
            wire::client::USER_STATS_REQUEST | wire::client::USER_PRESENCE_REQUEST => {
                let mut reader = Reader::new(packet.payload);
                let ids = reader.i32_list()?;
                reader.finish()?;
                if ids.len() > 100 {
                    return Err(ApiError(StatusCode::BAD_REQUEST, "too_many_users"));
                }
                for id in ids {
                    let Some(online) = sqlx::query("SELECT action,utc_offset FROM bancho_sessions WHERE user_id=$1 AND expires_at>now()").bind(i64::from(id)).fetch_optional(&mut *tx).await? else {continue};
                    let Ok(other) = common::account(state, i64::from(id)).await else {
                        continue;
                    };
                    let other_action =
                        serde_json::from_value(online.get("action")).unwrap_or_default();
                    if packet.id == wire::client::USER_STATS_REQUEST {
                        append(
                            &mut output,
                            stats(state, &other, other_action).await?.packet(),
                        )?;
                    } else {
                        append_presence(
                            &mut output,
                            &other,
                            online.get::<i16, _>("utc_offset") as i8,
                            other_action.mode,
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    // Recheck channel permissions on every poll; a role change takes effect now.
    let allowed = sqlx::query("SELECT id,name,required_permission FROM channels WHERE id=ANY($1)")
        .bind(&channels)
        .fetch_all(&mut *tx)
        .await?;
    for row in allowed {
        if row
            .get::<Option<String>, _>("required_permission")
            .as_deref()
            .is_some_and(|p| p == "lazer.room" || !player.can(p))
        {
            channels.retain(|id| *id != row.get::<i64, _>("id"));
            append(
                &mut output,
                wire::string_packet(
                    wire::server::CHANNEL_KICK,
                    &format!("#{}", row.get::<String, _>("name")),
                ),
            )?;
        }
    }
    let messages = sqlx::query("SELECT m.id,m.content,m.user_id,u.username,c.name FROM chat_messages m JOIN users u ON u.id=m.user_id JOIN channels c ON c.id=m.channel_id WHERE m.id>$1 AND m.channel_id=ANY($2) AND NOT u.is_restricted ORDER BY m.id LIMIT 100")
        .bind(session.get::<i64,_>("last_chat_id")).bind(&channels).fetch_all(&mut *tx).await?;
    let mut last_chat: i64 = session.get("last_chat_id");
    for row in messages {
        last_chat = row.get("id");
        if row.get::<i64, _>("user_id") != player.id {
            let message = ChatMessage {
                sender: row.get("username"),
                text: row.get("content"),
                recipient: format!("#{}", row.get::<String, _>("name")),
                sender_id: id32(row.get("user_id"))?,
            };
            append(
                &mut output,
                wire::encode_packet(wire::server::SEND_MESSAGE, &message.encode()?),
            )?;
        }
    }
    let messages = sqlx::query("SELECT m.id,m.content,m.sender_id,u.username FROM protocol_private_messages m JOIN users u ON u.id=m.sender_id WHERE m.id>$1 AND m.recipient_id=$2 AND NOT u.is_restricted ORDER BY m.id LIMIT 100")
        .bind(session.get::<i64,_>("last_dm_id")).bind(player.id).fetch_all(&mut *tx).await?;
    let mut last_dm: i64 = session.get("last_dm_id");
    for row in messages {
        last_dm = row.get("id");
        let message = ChatMessage {
            sender: row.get("username"),
            text: row.get("content"),
            recipient: player.username.clone(),
            sender_id: id32(row.get("sender_id"))?,
        };
        append(
            &mut output,
            wire::encode_packet(wire::server::SEND_MESSAGE, &message.encode()?),
        )?;
    }
    sqlx::query("UPDATE bancho_sessions SET expires_at=$2,last_chat_id=$3,last_dm_id=$4,channels=$5,action=$6 WHERE token_hash=$1").bind(common::token_hash(token)).bind(Utc::now()+Duration::seconds(180)).bind(last_chat).bind(last_dm).bind(channels).bind(json!(action)).execute(&mut *tx).await?;
    let queued=sqlx::query("SELECT id,payload FROM bancho_packet_queue WHERE user_id=$1 AND expires_at>now() ORDER BY id LIMIT 100 FOR UPDATE SKIP LOCKED").bind(player.id).fetch_all(&mut *tx).await?;
    let mut consumed = Vec::new();
    for row in queued {
        output.extend(row.get::<Vec<u8>, _>("payload"));
        consumed.push(row.get::<i64, _>("id"));
    }
    sqlx::query("DELETE FROM bancho_packet_queue WHERE id=ANY($1) OR expires_at<=now()")
        .bind(consumed)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(binary(output, None))
}

async fn web_auth(state: &AppState, query: &HashMap<String, String>) -> Result<Account, ApiError> {
    let username = query
        .get("u")
        .or_else(|| query.get("us"))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing_credentials"))?;
    let password = query
        .get("ha")
        .or_else(|| query.get("h"))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing_credentials"))?;
    if !wire::is_md5(password) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    common::rate_limit(
        state,
        format!("stable-web:{}", username.to_ascii_lowercase()),
        120,
    )
    .await?;
    common::authenticate(state, username, &password.to_ascii_lowercase(), true).await
}
async fn friends(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, ApiError> {
    let user = web_auth(&state, &query).await?;
    Ok(common::friend_ids(&state, user.id)
        .await?
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn replay(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let player = web_auth(&state, &query).await?;
    let id: i64 = query
        .get("c")
        .and_then(|s| s.parse().ok())
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_score_id"))?;
    let mode: i16 = query
        .get("m")
        .and_then(|s| s.parse().ok())
        .filter(|m| (0..=3).contains(m))
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_mode"))?;
    let path = sqlx::query_scalar::<_,Option<String>>("SELECT replay_path FROM scores WHERE id=$1 AND mode=$2 AND (verification='verified' OR user_id=$3)").bind(id).bind(mode).bind(player.id).fetch_optional(&state.pool).await?.flatten().ok_or(ApiError(StatusCode::NOT_FOUND,"replay_not_found"))?;
    let bytes = common::stored_file(&state, &path, "replays", 16_777_216).await?;
    // Sanctuary stores .osr containers; stable's web endpoint consumes compressed frames.
    let container = resonance_protocol::replay::LegacyReplay::parse(&bytes)?;
    Ok(binary(container.compressed_frames.to_vec(), None))
}

pub async fn beatmap_file(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let path: String = sqlx::query_scalar("SELECT osu_path FROM beatmaps WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "beatmap_not_found"))?;
    let bytes = common::stored_file(&state, &path, "maps", 8_388_608).await?;
    Ok(([("content-type", "text/plain; charset=utf-8")], bytes).into_response())
}
pub async fn beatmap_archive(
    State(state): State<AppState>,
    Path(raw_id): Path<String>,
) -> Result<Response, ApiError> {
    let id: i64 = raw_id
        .trim_end_matches('n')
        .parse()
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_beatmap_id"))?;
    let path =
        sqlx::query_scalar::<_, Option<String>>("SELECT archive_path FROM beatmaps WHERE id=$1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .flatten()
            .ok_or(ApiError(StatusCode::NOT_FOUND, "archive_not_available"))?;
    let bytes = common::stored_file(&state, &path, "maps", 104_857_600).await?;
    Ok((
        [
            ("content-type", "application/x-osu-beatmap-archive"),
            ("content-disposition", "attachment; filename=beatmap.osz"),
        ],
        bytes,
    )
        .into_response())
}

async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, ApiError> {
    let player = web_auth(&state, &query).await?;
    let md5 = query
        .get("c")
        .filter(|s| wire::is_md5(s))
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_beatmap_hash"))?;
    let mode: i16 = query
        .get("m")
        .and_then(|s| s.parse().ok())
        .filter(|m| (0..=3).contains(m))
        .ok_or(ApiError(StatusCode::BAD_REQUEST, "invalid_mode"))?;
    let Some(map) = sqlx::query(
        "SELECT id,official_set_id,title,artist,version,status FROM beatmaps WHERE md5=$1",
    )
    .bind(md5)
    .fetch_optional(&state.pool)
    .await?
    else {
        return Ok("-1|false|0|0|0|0|\n0\nUnknown beatmap\n0\n\n".into());
    };
    let id: i64 = map.get("id");
    let leaderboard_type: u8 = query.get("v").and_then(|s| s.parse().ok()).unwrap_or(1);
    if !(1..=4).contains(&leaderboard_type) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_leaderboard_type",
        ));
    }
    let requested_mods: i64 = query.get("mods").and_then(|s| s.parse().ok()).unwrap_or(0);
    let friend_filter = if leaderboard_type == 4 {
        let mut ids = common::friend_ids(&state, player.id)
            .await?
            .into_iter()
            .map(i64::from)
            .collect::<Vec<_>>();
        ids.push(player.id);
        Some(ids)
    } else {
        None
    };
    let rows = sqlx::query("WITH best AS (SELECT DISTINCT ON(s.user_id) s.id,s.user_id,s.score,s.max_combo,s.mods,s.hit_counts,s.created_at,s.replay_path,u.username FROM scores s JOIN users u ON u.id=s.user_id WHERE s.beatmap_id=$1 AND s.mode=$2 AND s.verification='verified' AND s.passed AND NOT u.is_restricted AND ($3::bigint IS NULL OR s.mods=$3) AND ($3::bigint IS NOT NULL OR (s.mods & 10368)=0) AND ($4::text IS NULL OR u.country=$4) AND ($5::bigint[] IS NULL OR s.user_id=ANY($5)) ORDER BY s.user_id,s.score DESC,s.id),placed AS (SELECT *,row_number() OVER(ORDER BY score DESC,id) AS placement FROM best) SELECT * FROM placed WHERE placement<=100 OR user_id=$6 ORDER BY placement")
        .bind(id).bind(mode).bind(if leaderboard_type==2 {Some(requested_mods)} else {None})
        .bind(if leaderboard_type==3 {Some(&player.country)}else{None}).bind(friend_filter).bind(player.id).fetch_all(&state.pool).await?;
    let status = match map.get::<String, _>("status").as_str() {
        "ranked" => 2,
        "loved" => 5,
        _ => 0,
    };
    let set_id: i64 = map.get::<Option<i64>, _>("official_set_id").unwrap_or(id);
    let map_name = clean(&format!(
        "{} - {} [{}]",
        map.get::<String, _>("artist"),
        map.get::<String, _>("title"),
        map.get::<String, _>("version")
    ));
    let lines: Vec<String> = rows
        .iter()
        .filter(|row| row.get::<i64, _>("placement") <= 100)
        .map(|row| score_line(row, row.get::<i64, _>("placement") as usize))
        .collect();
    let personal = rows
        .iter()
        .find(|r| r.get::<i64, _>("user_id") == player.id)
        .map(|row| score_line(row, row.get::<i64, _>("placement") as usize))
        .unwrap_or_default();
    Ok(format!(
        "{status}|false|{id}|{set_id}|{}|0|\n0\n{map_name}\n0\n{personal}\n{}\n",
        lines.len(),
        lines.join("\n")
    ))
}
fn clean(value: &str) -> String {
    value.replace(['|', '\n', '\r'], " ")
}
fn score_line(row: &sqlx::postgres::PgRow, rank: usize) -> String {
    let hits: Value = row.get("hit_counts");
    let hit = |key| hits.get(key).and_then(Value::as_u64).unwrap_or(0);
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|0|{}|{}|{}|{}|{}",
        row.get::<i64, _>("id"),
        clean(&row.get::<String, _>("username")),
        row.get::<i64, _>("score"),
        row.get::<i32, _>("max_combo"),
        hit("n50"),
        hit("n100"),
        hit("n300"),
        hit("misses"),
        hit("katu"),
        hit("geki"),
        row.get::<i64, _>("mods"),
        row.get::<i64, _>("user_id"),
        rank,
        row.get::<chrono::DateTime<Utc>, _>("created_at")
            .timestamp(),
        if row.get::<Option<String>, _>("replay_path").is_some() {
            1
        } else {
            0
        }
    )
}

async fn search(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, ApiError> {
    web_auth(&state, &query).await?;
    let search = query.get("q").cloned().unwrap_or_default();
    if search.len() > 128 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "query_too_long"));
    }
    let page: i64 = query
        .get("p")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .clamp(0, 1000);
    let mode: i16 = query.get("m").and_then(|v| v.parse().ok()).unwrap_or(-1);
    let rows = sqlx::query("SELECT * FROM beatmaps WHERE archive_path IS NOT NULL AND ($1=-1 OR mode=$1) AND (title ILIKE $2 OR artist ILIKE $2 OR creator ILIKE $2) ORDER BY created_at DESC LIMIT 100 OFFSET $3").bind(mode).bind(format!("%{search}%")).bind(page*100).fetch_all(&state.pool).await?;
    let mut output = format!("{}\n", rows.len());
    for row in rows {
        output.push_str(&direct_line(&row));
        output.push('\n');
    }
    Ok(output)
}
async fn search_set(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, ApiError> {
    web_auth(&state, &query).await?;
    let id: Option<i64> = query
        .get("b")
        .or_else(|| query.get("s"))
        .and_then(|v| v.parse().ok());
    let row = if let Some(id) = id {
        sqlx::query("SELECT * FROM beatmaps WHERE id=$1 AND archive_path IS NOT NULL")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
    } else if let Some(md5) = query.get("c") {
        sqlx::query("SELECT * FROM beatmaps WHERE md5=$1 AND archive_path IS NOT NULL")
            .bind(md5)
            .fetch_optional(&state.pool)
            .await?
    } else {
        None
    };
    Ok(row.map(|r| direct_line(&r)).unwrap_or_default())
}
fn direct_line(row: &sqlx::postgres::PgRow) -> String {
    let id: i64 = row.get("id");
    let status = match row.get::<String, _>("status").as_str() {
        "ranked" => 1,
        "loved" => 4,
        _ => 0,
    };
    format!(
        "{id}.osz|{}|{}|{}|{status}|0|{}|{id}|0|0|0|0|[{}*] {}@{}",
        clean(&row.get::<String, _>("artist")),
        clean(&row.get::<String, _>("title")),
        clean(&row.get::<String, _>("creator")),
        row.get::<chrono::DateTime<Utc>, _>("created_at")
            .format("%Y-%m-%d %H:%M:%S"),
        row.get::<f64, _>("stars"),
        clean(&row.get::<String, _>("version")),
        row.get::<i16, _>("mode")
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BeatmapInfoRequest {
    #[serde(default)]
    filenames: Vec<String>,
    #[serde(default)]
    ids: Vec<i64>,
}
async fn beatmap_info(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    axum::Json(request): axum::Json<BeatmapInfoRequest>,
) -> Result<String, ApiError> {
    web_auth(&state, &query).await?;
    if request.ids.len() + request.filenames.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "too_many_beatmaps"));
    }
    let mut lines = Vec::new();
    for (index, id) in request.ids.into_iter().enumerate() {
        if let Some(row) =
            sqlx::query("SELECT id,official_set_id,md5,status FROM beatmaps WHERE id=$1")
                .bind(id)
                .fetch_optional(&state.pool)
                .await?
        {
            let status = match row.get::<String, _>("status").as_str() {
                "ranked" => 1,
                "loved" => 4,
                _ => 0,
            };
            lines.push(format!(
                "{index}|{id}|{}|{}|{status}|N|N|N|N",
                row.get::<Option<i64>, _>("official_set_id").unwrap_or(id),
                row.get::<String, _>("md5")
            ));
        }
    }
    // Filename-only lookup is ambiguous for local maps and is intentionally absent.
    Ok(lines.join("\n"))
}
