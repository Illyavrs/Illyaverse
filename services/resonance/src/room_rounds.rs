//! Durable identity for one play of a playlist item. Callers serialize on the
//! room row; these helpers use only their transaction and perform no HTTP.
use crate::common::ApiError;
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{BTreeMap, HashSet};

fn invalid(code: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, code)
}

pub(crate) async fn record_playlist(
    tx: &mut Transaction<'_, Postgres>,
    room_id: i64,
    snapshot: &Value,
) -> Result<i64, ApiError> {
    let item = snapshot["id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| invalid("invalid_playlist_item"))?;
    Ok(sqlx::query_scalar("INSERT INTO lazer_room_playlist_versions(room_id,playlist_item_id,snapshot) VALUES($1,$2,$3) RETURNING id")
        .bind(room_id).bind(item).bind(snapshot).fetch_one(&mut **tx).await?)
}

/// Normalize supported mods so omitted settings and input ordering cannot move
/// an attempt between different contexts. This is also used by score ingress.
pub(crate) fn canonical_mods(ruleset: u8, value: &Value) -> Result<Value, ApiError> {
    let values = value
        .as_array()
        .ok_or_else(|| invalid("invalid_room_mods"))?;
    let mut sorted = BTreeMap::new();
    for modifier in values {
        let acronym = modifier["acronym"]
            .as_str()
            .ok_or_else(|| invalid("invalid_room_mods"))?;
        let mut object = modifier
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("invalid_room_mods"))?;
        object.entry("settings").or_insert_with(|| json!({}));
        if sorted
            .insert(acronym.to_owned(), Value::Object(object))
            .is_some()
        {
            return Err(invalid("invalid_room_mods"));
        }
    }
    let result = json!(sorted.into_values().collect::<Vec<_>>());
    illya_domain::score_mods::ValidatedMods::parse(ruleset, &result)
        .map_err(|_| invalid("invalid_room_mods"))?;
    Ok(result)
}

fn effective_mods(ruleset: u8, snapshot: &Value, selected: &Value) -> Result<Value, ApiError> {
    let required = canonical_mods(ruleset, &snapshot["requiredMods"])?;
    let selected = canonical_mods(ruleset, selected)?;
    // Allowed alternatives may themselves be incompatible (DT and HT). Validate
    // each option separately, and the final combination together.
    let mut allowed = BTreeMap::new();
    for modifier in snapshot["allowedMods"]
        .as_array()
        .ok_or_else(|| invalid("invalid_room_mods"))?
    {
        let canonical = canonical_mods(ruleset, &json!([modifier]))?;
        let modifier = canonical[0].clone();
        let acronym = modifier["acronym"].as_str().unwrap().to_owned();
        if allowed.insert(acronym, modifier).is_some() {
            return Err(invalid("invalid_room_mods"));
        }
    }
    let mut merged = BTreeMap::new();
    for modifier in required.as_array().unwrap() {
        merged.insert(
            modifier["acronym"].as_str().unwrap().to_owned(),
            modifier.clone(),
        );
    }
    for modifier in selected.as_array().unwrap() {
        let acronym = modifier["acronym"].as_str().unwrap();
        if let Some(required) = merged.get(acronym) {
            if required != modifier {
                return Err(invalid("required_room_mod_mismatch"));
            }
            continue;
        }
        let permitted = allowed
            .get(acronym)
            .ok_or_else(|| invalid("room_mod_not_allowed"))?;
        if permitted["settings"]
            .as_object()
            .is_none_or(|settings| !settings.is_empty())
            && permitted != modifier
        {
            return Err(invalid("allowed_room_mod_settings_mismatch"));
        }
        merged.insert(acronym.to_owned(), modifier.clone());
    }
    canonical_mods(ruleset, &json!(merged.into_values().collect::<Vec<_>>()))
}

pub(crate) async fn freeze_round(
    tx: &mut Transaction<'_, Postgres>,
    room_id: i64,
    snapshot: &Value,
    participants: &[i64],
) -> Result<i64, ApiError> {
    if participants.is_empty()
        || participants.len() > 16
        || participants.iter().copied().collect::<HashSet<_>>().len() != participants.len()
    {
        return Err(invalid("invalid_round_roster"));
    }
    let item = snapshot["id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| invalid("invalid_playlist_item"))?;
    let default_map = snapshot["beatmapID"]
        .as_i64()
        .ok_or_else(|| invalid("invalid_playlist_map"))?;
    let default_ruleset = snapshot["rulesetID"]
        .as_u64()
        .filter(|mode| *mode <= 3)
        .ok_or_else(|| invalid("invalid_ruleset"))? as u8;
    let map = sqlx::query("SELECT id,md5,mode,official_set_id,archive_path FROM beatmaps WHERE official_id=$1 OR id=$1 ORDER BY (official_id=$1) DESC LIMIT 1")
        .bind(default_map).fetch_optional(&mut **tx).await?.ok_or_else(|| invalid("beatmap_not_found"))?;
    let checksum: String = map.get("md5");
    if snapshot["beatmapChecksum"] != checksum {
        return Err(invalid("beatmap_checksum_mismatch"));
    }
    let version = sqlx::query("SELECT id,snapshot FROM lazer_room_playlist_versions WHERE room_id=$1 AND playlist_item_id=$2 ORDER BY id DESC LIMIT 1")
        .bind(room_id).bind(item).fetch_optional(&mut **tx).await?;
    let version_id =
        if let Some(version) = version.filter(|row| row.get::<Value, _>("snapshot") == *snapshot) {
            version.get::<i64, _>("id")
        } else {
            record_playlist(tx, room_id, snapshot).await?
        };
    let round_id: i64 = sqlx::query_scalar("INSERT INTO lazer_room_rounds(room_id,playlist_item_id,playlist_version_id,status) VALUES($1,$2,$3,'loading') RETURNING id")
        .bind(room_id).bind(item).bind(version_id).fetch_one(&mut **tx).await?;
    // Match the ascending user order used by social and moderation writes.
    let mut ordered_participants = participants.to_vec();
    ordered_participants.sort_unstable();
    for user in &ordered_participants {
        let member = sqlx::query("SELECT m.* FROM lazer_room_members m JOIN users u ON u.id=m.user_id WHERE m.room_id=$1 AND m.user_id=$2 AND NOT u.is_restricted AND m.state=1 AND m.beatmap_availability->>'state'='4' FOR SHARE OF u")
            .bind(room_id).bind(user).fetch_optional(&mut **tx).await?.ok_or_else(|| invalid("round_participant_not_ready"))?;
        let freestyle = snapshot["freestyle"].as_bool().unwrap_or(false);
        let selected_map = member
            .get::<Option<i64>, _>("beatmap_id")
            .unwrap_or(default_map);
        let selected_mode = member
            .get::<Option<i16>, _>("ruleset_id")
            .unwrap_or(i16::from(default_ruleset));
        if !(0..=3).contains(&selected_mode)
            || (!freestyle && selected_mode != i16::from(default_ruleset))
        {
            return Err(invalid("room_style_not_allowed"));
        }
        let actual = sqlx::query("SELECT id,md5,mode,official_set_id,archive_path FROM beatmaps WHERE official_id=$1 OR id=$1 ORDER BY (official_id=$1) DESC LIMIT 1")
            .bind(selected_map).fetch_optional(&mut **tx).await?.ok_or_else(|| invalid("beatmap_not_found"))?;
        let same_map = actual.get::<i64, _>("id") == map.get::<i64, _>("id");
        let same_set = map
            .get::<Option<i64>, _>("official_set_id")
            .is_some_and(|set| actual.get::<Option<i64>, _>("official_set_id") == Some(set));
        let same_archive = map
            .get::<Option<String>, _>("archive_path")
            .is_some_and(|path| {
                actual.get::<Option<String>, _>("archive_path").as_ref() == Some(&path)
            });
        if !same_map && !(freestyle && (same_set || same_archive)) {
            return Err(invalid("room_beatmap_not_allowed"));
        }
        let original_mode: i16 = actual.get("mode");
        if original_mode != 0 && original_mode != selected_mode {
            return Err(invalid("invalid_ruleset_conversion"));
        }
        let mods = effective_mods(
            selected_mode as u8,
            snapshot,
            &member.get::<Value, _>("mods"),
        )?;
        sqlx::query("INSERT INTO lazer_room_round_participants(round_id,user_id,beatmap_id,beatmap_checksum,ruleset_id,mods) VALUES($1,$2,$3,$4,$5,$6)")
            .bind(round_id).bind(user).bind(actual.get::<i64,_>("id")).bind(actual.get::<String,_>("md5")).bind(selected_mode).bind(mods).execute(&mut **tx).await?;
    }
    sqlx::query("UPDATE lazer_rooms SET current_round_id=$2 WHERE id=$1")
        .bind(room_id)
        .bind(round_id)
        .execute(&mut **tx)
        .await?;
    Ok(round_id)
}

pub(crate) async fn mark_playing(
    tx: &mut Transaction<'_, Postgres>,
    round_id: i64,
) -> Result<(), ApiError> {
    let changed = sqlx::query("UPDATE lazer_room_rounds SET status='playing' WHERE id=$1 AND status='loading' AND expires_at>now()")
        .bind(round_id).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        return Err(ApiError(StatusCode::CONFLICT, "round_not_loading"));
    }
    Ok(())
}

pub(crate) async fn mark_finished(
    tx: &mut Transaction<'_, Postgres>,
    round_id: i64,
) -> Result<(), ApiError> {
    sqlx::query("UPDATE lazer_room_rounds SET status='finished',finished_at=COALESCE(finished_at,now()) WHERE id=$1 AND status IN('loading','playing')")
        .bind(round_id).execute(&mut **tx).await?;
    // Completed attempts retain their original bounded submission window.
    Ok(())
}

pub(crate) async fn abort_round(
    tx: &mut Transaction<'_, Postgres>,
    round_id: i64,
    reason: &str,
) -> Result<(), ApiError> {
    let changed = sqlx::query("UPDATE lazer_room_rounds SET status='aborted',finished_at=COALESCE(finished_at,now()) WHERE id=$1 AND status IN('loading','playing')")
        .bind(round_id).execute(&mut **tx).await?.rows_affected();
    if changed != 0 {
        sqlx::query("UPDATE lazer_room_round_participants SET aborted_at=COALESCE(aborted_at,now()) WHERE round_id=$1").bind(round_id).execute(&mut **tx).await?;
        sqlx::query("UPDATE lazer_score_tokens SET revoked_at=COALESCE(revoked_at,now()),revoke_reason=COALESCE(revoke_reason,$2),processing_status='revoked' WHERE scope='room' AND round_id=$1 AND processing_status<>'processed'")
            .bind(round_id).bind(reason).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(crate) async fn abort_participant(
    tx: &mut Transaction<'_, Postgres>,
    round_id: i64,
    user: i64,
    reason: &str,
) -> Result<(), ApiError> {
    let changed = sqlx::query("UPDATE lazer_room_round_participants p SET aborted_at=COALESCE(aborted_at,now()) FROM lazer_room_rounds r WHERE p.round_id=$1 AND p.user_id=$2 AND r.id=p.round_id AND r.status IN('loading','playing')")
        .bind(round_id).bind(user).execute(&mut **tx).await?.rows_affected();
    if changed != 0 {
        sqlx::query("UPDATE lazer_score_tokens SET revoked_at=COALESCE(revoked_at,now()),revoke_reason=COALESCE(revoke_reason,$3),processing_status='revoked' WHERE scope='room' AND round_id=$1 AND user_id=$2 AND processing_status<>'processed'")
            .bind(round_id).bind(user).bind(reason).execute(&mut **tx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_and_allowed_mods_are_frozen_without_client_override() {
        let snapshot = json!({"requiredMods":[{"acronym":"HD"}],"allowedMods":[{"acronym":"DT"},{"acronym":"HT"}]});
        let effective = effective_mods(
            0,
            &snapshot,
            &json!([{"acronym":"DT","settings":{"speed_change":1.2}}]),
        )
        .unwrap();
        assert_eq!(
            effective,
            json!([{"acronym":"DT","settings":{"speed_change":1.2}},{"acronym":"HD","settings":{}}])
        );
        assert!(effective_mods(0, &snapshot, &json!([{"acronym":"HR"}])).is_err());
        assert!(effective_mods(0, &snapshot, &json!([{"acronym":"DT"},{"acronym":"HT"}])).is_err());
        assert!(
            effective_mods(
                0,
                &snapshot,
                &json!([{"acronym":"HD","settings":{"only_fade_approach_circles":true}}])
            )
            .is_err()
        );
    }
}
