//! Stable multiplayer and spectating. PostgreSQL owns membership and delivery,
//! so authenticated polling requests cannot forge another slot or room host.
use crate::common::{Account, ApiError, AppState};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::http::StatusCode;
use resonance_protocol::{
    multiplayer as room,
    stable::{self as wire, ChatMessage, Reader},
};
use sqlx::{PgConnection, Row};

// Packet numbers are interoperability facts, independent of authorization.
const UPDATE: u16 = 26;
const NEW: u16 = 27;
const DISPOSE: u16 = 28;
const JOIN_OK: u16 = 36;
const JOIN_FAIL: u16 = 37;
const START: u16 = 46;
const SCORE: u16 = 48;
const TRANSFER: u16 = 50;
const LOADED: u16 = 53;
const FAILED: u16 = 57;
const COMPLETE: u16 = 58;
const SKIP: u16 = 61;
const PLAYER_SKIP: u16 = 81;

fn id32(id: i64) -> Result<i32, ApiError> {
    i32::try_from(id).map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid_player_id"))
}
fn invalid() -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, "invalid_room_operation")
}
fn append(out: &mut Vec<u8>, value: resonance_protocol::Result<Vec<u8>>) -> Result<(), ApiError> {
    out.extend(value?);
    Ok(())
}
fn empty(payload: &[u8]) -> Result<(), ApiError> {
    if payload.is_empty() {
        Ok(())
    } else {
        Err(invalid())
    }
}
fn integer(payload: &[u8]) -> Result<i32, ApiError> {
    let mut r = Reader::new(payload);
    let value = r.i32()?;
    r.finish()?;
    Ok(value)
}
async fn queue(db: &mut PgConnection, user: i64, packet: &[u8]) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT $1,$2 WHERE EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$1 AND expires_at>now())").bind(user).bind(packet).execute(db).await?;
    Ok(())
}
async fn send_room(
    db: &mut PgConnection,
    id: i16,
    packet: &[u8],
    except: Option<i64>,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT m.user_id,$2 FROM stable_room_members m JOIN bancho_sessions b ON b.user_id=m.user_id WHERE m.room_id=$1 AND b.expires_at>now() AND ($3::bigint IS NULL OR m.user_id<>$3)").bind(id).bind(packet).bind(except).execute(db).await?;
    Ok(())
}
async fn lobby(db: &mut PgConnection, packet: &[u8]) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT user_id,$1 FROM bancho_sessions WHERE lobby AND expires_at>now() AND NOT EXISTS(SELECT 1 FROM stable_room_members WHERE user_id=bancho_sessions.user_id)").bind(packet).execute(db).await?;
    Ok(())
}
async fn save(db: &mut PgConnection, m: &room::Match) -> Result<(), ApiError> {
    m.validate()?;
    sqlx::query("UPDATE stable_rooms SET host_id=$2,state=$3,updated_at=now() WHERE id=$1")
        .bind(m.id as i16)
        .bind(i64::from(m.host_id))
        .bind(serde_json::to_value(m).map_err(|_| invalid())?)
        .execute(db)
        .await?;
    Ok(())
}
async fn broadcast(db: &mut PgConnection, m: &room::Match) -> Result<(), ApiError> {
    let packet = wire::encode_packet(UPDATE, &m.encode(true)?)?;
    send_room(db, m.id as i16, &packet, None).await?;
    lobby(db, &packet).await
}
async fn current(
    db: &mut PgConnection,
    user: i64,
) -> Result<Option<(room::Match, usize)>, ApiError> {
    let row=sqlx::query("SELECT r.state,m.slot FROM stable_rooms r JOIN stable_room_members m ON m.room_id=r.id WHERE m.user_id=$1 FOR UPDATE OF r").bind(user).fetch_optional(db).await?;
    row.map(|r| {
        Ok((
            serde_json::from_value(r.get("state")).map_err(|_| {
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, "invalid_persisted_room")
            })?,
            r.get::<i16, _>("slot") as usize,
        ))
    })
    .transpose()
}

async fn password_hash(password: String) -> Result<Option<String>, ApiError> {
    if password.is_empty() {
        return Ok(None);
    }
    tokio::task::spawn_blocking(move || {
        let salt =
            SaltString::encode_b64(uuid::Uuid::new_v4().as_bytes()).map_err(|_| invalid())?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|v| Some(v.to_string()))
            .map_err(|_| invalid())
    })
    .await
    .map_err(|_| invalid())?
}
async fn verify_password(hash: Option<String>, password: String) -> bool {
    let Some(hash) = hash else { return true };
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

pub async fn leave(db: &mut PgConnection, user: i64) -> Result<(), ApiError> {
    let Some((mut m, slot)) = current(db, user).await? else {
        return Ok(());
    };
    sqlx::query("DELETE FROM stable_room_members WHERE user_id=$1")
        .bind(user)
        .execute(&mut *db)
        .await?;
    m.statuses[slot] = room::OPEN;
    m.users[slot] = None;
    m.teams[slot] = 0;
    m.slot_mods[slot] = 0;
    queue(
        db,
        user,
        &wire::string_packet(wire::server::CHANNEL_KICK, "#multiplayer")?,
    )
    .await?;
    if m.users.iter().all(Option::is_none) {
        sqlx::query("DELETE FROM stable_rooms WHERE id=$1")
            .bind(m.id as i16)
            .execute(&mut *db)
            .await?;
        lobby(db, &wire::int_packet(DISPOSE, i32::from(m.id))?).await?;
        return Ok(());
    }
    if m.host_id == id32(user)? {
        m.host_id = m
            .users
            .iter()
            .flatten()
            .next()
            .copied()
            .ok_or_else(invalid)?;
        queue(
            db,
            i64::from(m.host_id),
            &wire::encode_packet(TRANSFER, &[])?,
        )
        .await?;
    }
    if m.in_progress && !m.statuses.contains(&room::PLAYING) {
        m.in_progress = false;
        for i in 0..room::SLOTS {
            if m.users[i].is_some() {
                m.statuses[i] = room::NOT_READY;
            }
        }
        send_room(db, m.id as i16, &wire::encode_packet(COMPLETE, &[])?, None).await?;
    }
    save(db, &m).await?;
    broadcast(db, &m).await
}

/// All stable room operations use one transaction advisory lock. This keeps
/// slot allocation, host transfer and expiry cleanup atomic across instances.
pub async fn cleanup(db: &mut PgConnection) -> Result<(), ApiError> {
    sqlx::query("SELECT pg_advisory_xact_lock(512918376)")
        .execute(&mut *db)
        .await?;
    let stale=sqlx::query_scalar::<_,i64>("SELECT m.user_id FROM stable_room_members m LEFT JOIN bancho_sessions b ON b.user_id=m.user_id LEFT JOIN users u ON u.id=m.user_id WHERE b.user_id IS NULL OR b.expires_at<=now() OR u.is_restricted ORDER BY m.user_id") .fetch_all(&mut *db).await?;
    for user in stale {
        leave(db, user).await?;
    }
    Ok(())
}

pub async fn handle(
    state: &AppState,
    db: &mut PgConnection,
    user: &Account,
    id: u16,
    payload: &[u8],
    out: &mut Vec<u8>,
) -> Result<bool, ApiError> {
    if matches!(id, 16 | 17 | 18 | 21) {
        cleanup(db).await?;
        spectate(state, db, user, id, payload, out).await?;
        return Ok(true);
    }
    if !matches!(
        id,
        29 | 30
            | 31
            | 32
            | 33
            | 38
            | 39
            | 40
            | 41
            | 44
            | 47
            | 49
            | 51
            | 52
            | 54
            | 55
            | 56
            | 59
            | 60
            | 70
            | 77
            | 87
            | 90
    ) {
        return Ok(false);
    }
    cleanup(db).await?;
    crate::common::rate_limit(state, format!("stable-room:{}", user.id), 600).await?;
    match id {
        29 => {
            empty(payload)?;
            sqlx::query("UPDATE bancho_sessions SET lobby=false WHERE user_id=$1")
                .bind(user.id)
                .execute(db)
                .await?;
            return Ok(true);
        }
        30 => {
            empty(payload)?;
            sqlx::query("UPDATE bancho_sessions SET lobby=true WHERE user_id=$1")
                .bind(user.id)
                .execute(&mut *db)
                .await?;
            let rooms = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT state FROM stable_rooms ORDER BY id LIMIT 1000",
            )
            .fetch_all(&mut *db)
            .await?;
            for value in rooms {
                let m: room::Match = serde_json::from_value(value).map_err(|_| invalid())?;
                append(out, wire::encode_packet(NEW, &m.encode(true)?))?;
            }
            return Ok(true);
        }
        31 => {
            crate::common::rate_limit(state, format!("stable-room-create:{}", user.id), 10).await?;
            if current(db, user.id).await?.is_some() {
                append(out, wire::encode_packet(JOIN_FAIL, &[]))?;
                return Ok(true);
            }
            let mut m = room::Match::decode(payload)?;
            let hash = password_hash(m.password.clone()).await?;
            let next=sqlx::query_scalar::<_,i32>("SELECT n FROM generate_series(1,32767) n WHERE NOT EXISTS(SELECT 1 FROM stable_rooms WHERE id=n) ORDER BY n LIMIT 1").fetch_optional(&mut *db).await?.ok_or(ApiError(StatusCode::SERVICE_UNAVAILABLE,"room_limit_reached"))?;
            m.id = next as u16;
            m.host_id = id32(user.id)?;
            m.in_progress = false;
            m.password = if hash.is_some() {
                "#".into()
            } else {
                String::new()
            };
            m.statuses = [room::OPEN; 16];
            m.users = [None; 16];
            m.teams = [0; 16];
            m.slot_mods = [0; 16];
            m.statuses[0] = room::NOT_READY;
            m.users[0] = Some(id32(user.id)?);
            sqlx::query(
                "INSERT INTO stable_rooms(id,host_id,password_hash,state) VALUES($1,$2,$3,$4)",
            )
            .bind(m.id as i16)
            .bind(user.id)
            .bind(hash)
            .bind(serde_json::to_value(&m).map_err(|_| invalid())?)
            .execute(&mut *db)
            .await?;
            sqlx::query("INSERT INTO stable_room_members(user_id,room_id,slot) VALUES($1,$2,0)")
                .bind(user.id)
                .bind(m.id as i16)
                .execute(&mut *db)
                .await?;
            append(out, wire::encode_packet(JOIN_OK, &m.encode(true)?))?;
            append(
                out,
                wire::string_packet(wire::server::CHANNEL_JOIN_SUCCESS, "#multiplayer"),
            )?;
            lobby(db, &wire::encode_packet(NEW, &m.encode(true)?)?).await?;
            return Ok(true);
        }
        32 => {
            let mut r = Reader::new(payload);
            let target = r.i32()?;
            let password = r.string()?.to_owned();
            r.finish()?;
            crate::common::rate_limit(state, format!("stable-room-join:{}", user.id), 30).await?;
            if !(1..=32767).contains(&target) || current(db, user.id).await?.is_some() {
                append(out, wire::encode_packet(JOIN_FAIL, &[]))?;
                return Ok(true);
            }
            let Some(row) =
                sqlx::query("SELECT state,password_hash FROM stable_rooms WHERE id=$1 FOR UPDATE")
                    .bind(target as i16)
                    .fetch_optional(&mut *db)
                    .await?
            else {
                append(out, wire::encode_packet(JOIN_FAIL, &[]))?;
                return Ok(true);
            };
            let mut m: room::Match =
                serde_json::from_value(row.get("state")).map_err(|_| invalid())?;
            if m.in_progress || !verify_password(row.get("password_hash"), password).await {
                append(out, wire::encode_packet(JOIN_FAIL, &[]))?;
                return Ok(true);
            }
            let Some(slot) = m.statuses.iter().position(|s| *s == room::OPEN) else {
                append(out, wire::encode_packet(JOIN_FAIL, &[]))?;
                return Ok(true);
            };
            m.statuses[slot] = room::NOT_READY;
            m.users[slot] = Some(id32(user.id)?);
            m.teams[slot] = if m.team_type >= 2 { 1 } else { 0 };
            sqlx::query("INSERT INTO stable_room_members(user_id,room_id,slot) VALUES($1,$2,$3)")
                .bind(user.id)
                .bind(m.id as i16)
                .bind(slot as i16)
                .execute(&mut *db)
                .await?;
            save(db, &m).await?;
            append(out, wire::encode_packet(JOIN_OK, &m.encode(true)?))?;
            append(
                out,
                wire::string_packet(wire::server::CHANNEL_JOIN_SUCCESS, "#multiplayer"),
            )?;
            broadcast(db, &m).await?;
            return Ok(true);
        }
        33 => {
            empty(payload)?;
            leave(db, user.id).await?;
            return Ok(true);
        }
        _ => {}
    }
    let Some((mut m, slot)) = current(db, user.id).await? else {
        return Err(ApiError(StatusCode::FORBIDDEN, "room_membership_required"));
    };
    let host = m.host_id == id32(user.id)?;
    let need_host = |host: bool| {
        if host {
            Ok(())
        } else {
            Err(ApiError(StatusCode::FORBIDDEN, "room_host_required"))
        }
    };
    match id {
        38 => {
            let target = integer(payload)?;
            if !(0..16).contains(&target)
                || m.statuses[target as usize] != room::OPEN
                || m.in_progress
            {
                return Err(invalid());
            }
            let target = target as usize;
            m.statuses.swap(slot, target);
            m.users.swap(slot, target);
            m.teams.swap(slot, target);
            m.slot_mods.swap(slot, target);
            sqlx::query("UPDATE stable_room_members SET slot=$2 WHERE user_id=$1")
                .bind(user.id)
                .bind(target as i16)
                .execute(&mut *db)
                .await?;
        }
        39 | 55 | 54 | 59 => {
            empty(payload)?;
            if m.in_progress {
                return Err(invalid());
            }
            m.statuses[slot] = match id {
                39 => room::READY,
                54 => room::NO_MAP,
                _ => room::NOT_READY,
            };
        }
        40 => {
            need_host(host)?;
            if m.in_progress {
                return Err(invalid());
            }
            let target = integer(payload)?;
            if !(0..16).contains(&target) || target as usize == slot {
                return Err(invalid());
            }
            let target = target as usize;
            if let Some(kicked) = m.users[target] {
                leave(db, i64::from(kicked)).await?;
                let refreshed = current(db, user.id).await?.ok_or_else(invalid)?;
                m = refreshed.0;
            }
            m.statuses[target] = if m.statuses[target] == room::LOCKED {
                room::OPEN
            } else {
                room::LOCKED
            };
        }
        41 => {
            need_host(host)?;
            if m.in_progress {
                return Err(invalid());
            }
            let input = room::Match::decode(payload)?;
            let changed = input.map_md5 != m.map_md5 || input.mode != m.mode;
            m.name = input.name;
            m.map_name = input.map_name;
            m.map_id = input.map_id;
            m.map_md5 = input.map_md5;
            m.mods = input.mods;
            m.mode = input.mode;
            m.win_condition = input.win_condition;
            m.team_type = input.team_type;
            m.seed = input.seed;
            if input.freemods != m.freemods {
                for i in 0..16 {
                    m.slot_mods[i] = if input.freemods {
                        m.mods & !(64 | 256 | 512)
                    } else {
                        0
                    };
                }
                if input.freemods {
                    m.mods &= 64 | 256 | 512;
                }
                m.freemods = input.freemods;
            }
            for i in 0..16 {
                if m.users[i].is_some() {
                    if changed {
                        m.statuses[i] = room::NOT_READY;
                    }
                    m.teams[i] = if m.team_type >= 2 {
                        m.teams[i].max(1)
                    } else {
                        0
                    };
                }
            }
        }
        44 => {
            need_host(host)?;
            empty(payload)?;
            if m.in_progress {
                return Err(invalid());
            }
            let mut participants = 0;
            for status in &mut m.statuses {
                if matches!(*status, room::READY | room::NOT_READY) {
                    *status = room::PLAYING;
                    participants += 1;
                }
            }
            if participants == 0 {
                return Err(invalid());
            }
            m.in_progress = true;
            sqlx::query("UPDATE stable_room_members SET loaded=false,skipped=false,failed=false WHERE room_id=$1").bind(m.id as i16).execute(&mut *db).await?;
            send_room(
                db,
                m.id as i16,
                &wire::encode_packet(START, &m.encode(true)?)?,
                None,
            )
            .await?;
        }
        47 => {
            if !m.in_progress || m.statuses[slot] != room::PLAYING {
                return Err(invalid());
            }
            room::validate_score_frame(payload)?;
            let mut frame = payload.to_vec();
            frame[4] = slot as u8;
            send_room(
                db,
                m.id as i16,
                &wire::encode_packet(SCORE, &frame)?,
                Some(user.id),
            )
            .await?;
            return Ok(true);
        }
        49 => {
            empty(payload)?;
            if !m.in_progress || m.statuses[slot] != room::PLAYING {
                return Err(invalid());
            }
            m.statuses[slot] = room::COMPLETE;
            if !m.statuses.contains(&room::PLAYING) {
                m.in_progress = false;
                for i in 0..16 {
                    if m.users[i].is_some() && m.statuses[i] != room::NO_MAP {
                        m.statuses[i] = room::NOT_READY;
                    }
                }
                send_room(db, m.id as i16, &wire::encode_packet(COMPLETE, &[])?, None).await?;
            }
        }
        51 => {
            let mods = integer(payload)?;
            if mods < 0 || m.in_progress {
                return Err(invalid());
            }
            if m.freemods {
                m.slot_mods[slot] = mods as u32 & !(64 | 256 | 512);
                if host {
                    m.mods = mods as u32 & (64 | 256 | 512);
                }
            } else {
                need_host(host)?;
                m.mods = mods as u32;
            }
        }
        52 | 56 | 60 => {
            empty(payload)?;
            if !m.in_progress || m.statuses[slot] != room::PLAYING {
                return Err(invalid());
            }
            let column = match id {
                52 => "loaded",
                56 => "failed",
                _ => "skipped",
            };
            let query = format!("UPDATE stable_room_members SET {column}=true WHERE user_id=$1");
            sqlx::query(&query).bind(user.id).execute(&mut *db).await?;
            match id {
                56 => {
                    send_room(
                        db,
                        m.id as i16,
                        &wire::int_packet(FAILED, slot as i32)?,
                        None,
                    )
                    .await?
                }
                60 => {
                    send_room(
                        db,
                        m.id as i16,
                        &wire::int_packet(PLAYER_SKIP, id32(user.id)?)?,
                        None,
                    )
                    .await?
                }
                _ => {}
            }
            if id != 56 {
                let active: Vec<i64> = m
                    .users
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| m.statuses[*i] == room::PLAYING)
                    .filter_map(|(_, id)| id.map(i64::from))
                    .collect();
                let query = format!(
                    "SELECT count(*) FROM stable_room_members WHERE user_id=ANY($1) AND NOT {column}"
                );
                let missing: i64 = sqlx::query_scalar(&query)
                    .bind(active)
                    .fetch_one(&mut *db)
                    .await?;
                if missing == 0 {
                    send_room(
                        db,
                        m.id as i16,
                        &wire::encode_packet(if id == 52 { LOADED } else { SKIP }, &[])?,
                        None,
                    )
                    .await?;
                }
            }
            return Ok(true);
        }
        70 => {
            need_host(host)?;
            let target = integer(payload)?;
            if !(0..16).contains(&target) {
                return Err(invalid());
            }
            m.host_id = m.users[target as usize].ok_or_else(invalid)?;
            queue(
                db,
                i64::from(m.host_id),
                &wire::encode_packet(TRANSFER, &[])?,
            )
            .await?;
        }
        77 => {
            empty(payload)?;
            if m.in_progress || m.team_type < 2 {
                return Err(invalid());
            }
            m.teams[slot] = if m.teams[slot] == 1 { 2 } else { 1 };
        }
        87 => {
            let target = integer(payload)?;
            let target_account =
                crate::common::account_on_connection(&mut *db, i64::from(target)).await?;
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='block')").bind(user.id).bind(target_account.id).fetch_one(&mut *db).await?;
            if blocked {
                return Err(ApiError(StatusCode::FORBIDDEN, "relationship_blocked"));
            }
            crate::common::rate_limit(state, format!("stable-invite:{}", user.id), 10).await?;
            let invite = ChatMessage {
                sender: user.username.clone(),
                text: format!("Join my room: [osump://{}/ {}]", m.id, m.name),
                recipient: target_account.username,
                sender_id: id32(user.id)?,
            };
            queue(
                db,
                i64::from(target),
                &wire::encode_packet(88, &invite.encode()?)?,
            )
            .await?;
            return Ok(true);
        }
        90 => {
            need_host(host)?;
            let input = room::Match::decode(payload)?;
            let hash = password_hash(input.password).await?;
            m.password = if hash.is_some() {
                "#".into()
            } else {
                String::new()
            };
            sqlx::query("UPDATE stable_rooms SET password_hash=$2 WHERE id=$1")
                .bind(m.id as i16)
                .bind(hash)
                .execute(&mut *db)
                .await?;
        }
        _ => return Err(invalid()),
    }
    save(db, &m).await?;
    broadcast(db, &m).await?;
    Ok(true)
}

async fn stop_spectating(db: &mut PgConnection, user: i64) -> Result<(), ApiError> {
    let host = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(&mut *db)
    .await?
    .flatten();
    let Some(host) = host else { return Ok(()) };
    sqlx::query("UPDATE bancho_sessions SET spectator_host_id=NULL WHERE user_id=$1")
        .bind(user)
        .execute(&mut *db)
        .await?;
    queue(db, host, &wire::int_packet(14, id32(user)?)?).await?;
    let others = sqlx::query_scalar::<_, i64>(
        "SELECT user_id FROM bancho_sessions WHERE spectator_host_id=$1 AND expires_at>now()",
    )
    .bind(host)
    .fetch_all(&mut *db)
    .await?;
    for other in others {
        queue(db, other, &wire::int_packet(43, id32(user)?)?).await?;
    }
    queue(
        db,
        user,
        &wire::string_packet(wire::server::CHANNEL_KICK, "#spectator")?,
    )
    .await?;
    Ok(())
}
pub async fn disconnect(db: &mut PgConnection, user: i64) -> Result<(), ApiError> {
    cleanup(db).await?;
    leave(db, user).await?;
    stop_spectating(db, user).await?;
    let watchers = sqlx::query_scalar::<_, i64>(
        "SELECT user_id FROM bancho_sessions WHERE spectator_host_id=$1 AND expires_at>now()",
    )
    .bind(user)
    .fetch_all(&mut *db)
    .await?;
    for watcher in watchers {
        queue(
            db,
            watcher,
            &wire::string_packet(wire::server::NOTIFICATION, "Spectated player disconnected.")?,
        )
        .await?;
    }
    Ok(())
}

pub async fn refresh_spectator(db: &mut PgConnection, user: i64) -> Result<(), ApiError> {
    let stale:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bancho_sessions b WHERE b.user_id=$1 AND b.spectator_host_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM bancho_sessions h JOIN users u ON u.id=h.user_id WHERE h.user_id=b.spectator_host_id AND h.expires_at>now() AND NOT u.is_restricted))").bind(user).fetch_one(&mut *db).await?;
    if stale {
        stop_spectating(db, user).await?;
    }
    Ok(())
}

async fn spectate(
    state: &AppState,
    db: &mut PgConnection,
    user: &Account,
    id: u16,
    payload: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), ApiError> {
    match id {
        16 => {
            let host = i64::from(integer(payload)?);
            if host == user.id {
                return Err(invalid());
            }
            crate::common::account_on_connection(&mut *db, host).await?;
            let available:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bancho_sessions WHERE user_id=$1 AND expires_at>now() AND spectator_host_id IS NULL)").bind(host).fetch_one(&mut *db).await?;
            let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_relationships WHERE ((user_id=$1 AND target_id=$2) OR (user_id=$2 AND target_id=$1)) AND relation_type='block')").bind(user.id).bind(host).fetch_one(&mut *db).await?;
            if !available || blocked {
                return Err(ApiError(StatusCode::FORBIDDEN, "spectator_unavailable"));
            }
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM bancho_sessions WHERE spectator_host_id=$1 AND expires_at>now()").bind(host).fetch_one(&mut *db).await?;
            if count >= 100 {
                return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "spectator_limit"));
            }
            stop_spectating(db, user.id).await?;
            sqlx::query("UPDATE bancho_sessions SET spectator_host_id=$2 WHERE user_id=$1")
                .bind(user.id)
                .bind(host)
                .execute(&mut *db)
                .await?;
            queue(db, host, &wire::int_packet(13, id32(user.id)?)?).await?;
            let others=sqlx::query_scalar::<_,i64>("SELECT user_id FROM bancho_sessions WHERE spectator_host_id=$1 AND user_id<>$2 AND expires_at>now()").bind(host).bind(user.id).fetch_all(&mut *db).await?;
            for other in others {
                append(out, wire::int_packet(42, id32(other)?))?;
                queue(db, other, &wire::int_packet(42, id32(user.id)?)?).await?;
            }
            append(
                out,
                wire::string_packet(wire::server::CHANNEL_JOIN_SUCCESS, "#spectator"),
            )?;
            queue(
                db,
                host,
                &wire::string_packet(wire::server::CHANNEL_JOIN_SUCCESS, "#spectator")?,
            )
            .await?;
        }
        17 => {
            empty(payload)?;
            stop_spectating(db, user.id).await?;
        }
        18 => {
            room::validate_spectator_bundle(payload)?;
            crate::common::rate_limit(state, format!("stable-spectator:{}", user.id), 3600).await?;
            let packet = wire::encode_packet(15, payload)?;
            sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload,expires_at) SELECT user_id,$2,now()+interval '15 seconds' FROM bancho_sessions WHERE spectator_host_id=$1 AND expires_at>now()").bind(user.id).bind(packet).execute(db).await?;
        }
        21 => {
            empty(payload)?;
            let host = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1",
            )
            .bind(user.id)
            .fetch_one(&mut *db)
            .await?;
            if let Some(host) = host {
                queue(db, host, &wire::int_packet(22, id32(user.id)?)?).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn special_chat(
    db: &mut PgConnection,
    user: &Account,
    target: &str,
    content: &str,
) -> Result<bool, ApiError> {
    if !matches!(target, "#multiplayer" | "#spectator") {
        return Ok(false);
    }
    cleanup(db).await?;
    let packet = wire::encode_packet(
        wire::server::SEND_MESSAGE,
        &ChatMessage {
            sender: user.username.clone(),
            text: content.into(),
            recipient: target.into(),
            sender_id: id32(user.id)?,
        }
        .encode()?,
    )?;
    match target {
        "#multiplayer" => {
            let Some((m, _)) = current(db, user.id).await? else {
                return Err(ApiError(StatusCode::FORBIDDEN, "room_membership_required"));
            };
            send_room(db, m.id as i16, &packet, Some(user.id)).await?;
            Ok(true)
        }
        "#spectator" => {
            let host = sqlx::query_scalar::<_, Option<i64>>(
                "SELECT spectator_host_id FROM bancho_sessions WHERE user_id=$1",
            )
            .bind(user.id)
            .fetch_one(&mut *db)
            .await?
            .unwrap_or(user.id);
            let count:i64=sqlx::query_scalar("SELECT count(*) FROM bancho_sessions WHERE spectator_host_id=$1 AND expires_at>now()").bind(host).fetch_one(&mut *db).await?;
            if count == 0 {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "spectator_membership_required",
                ));
            }
            sqlx::query("INSERT INTO bancho_packet_queue(user_id,payload) SELECT user_id,$3 FROM bancho_sessions WHERE (spectator_host_id=$1 OR user_id=$1) AND user_id<>$2 AND expires_at>now()").bind(host).bind(user.id).bind(packet).execute(db).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
