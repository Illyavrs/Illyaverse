//! Bounded original-difficulty metadata shared across the internal service boundary.
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const MAX_OFFICIAL_ID: i64 = 1_999_999_999;
pub const MAX_METADATA_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 1024;
const INVALID: &str = "Invalid official beatmap set metadata.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficialSetMetadata {
    pub id: i64,
    pub artist: String,
    pub artist_unicode: String,
    pub title: String,
    pub title_unicode: String,
    pub creator: String,
    pub status: String,
    pub bpm: f64,
    pub source: String,
    pub tags: String,
    pub nsfw: bool,
    /// Unknown upstream policy prohibits new acquisition; retained archives
    /// keep their independent immutable-storage availability contract.
    #[serde(default = "disabled_by_default")]
    pub download_disabled: bool,
    #[serde(default)]
    pub archive_download_available: bool,
    pub submitted_date: String,
    pub last_updated: Option<String>,
    pub ranked_date: Option<String>,
    pub genre: OfficialSetClassification,
    pub language: OfficialSetClassification,
    pub beatmaps: Vec<OfficialSetDifficulty>,
}
fn disabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficialSetClassification {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfficialSetDifficulty {
    pub id: i64,
    pub beatmapset_id: i64,
    pub checksum: String,
    pub version: String,
    pub mode_int: i16,
    pub status: String,
    pub last_updated: String,
    pub difficulty_rating: f64,
    pub bpm: f64,
    pub total_length: i32,
    pub hit_length: i32,
    pub max_combo: Option<i32>,
    pub ar: f64,
    pub cs: f64,
    pub od: f64,
    pub hp: f64,
    pub count_circles: i32,
    pub count_sliders: i32,
    pub count_spinners: i32,
}

fn normalized_status(value: &Value) -> Result<String, &'static str> {
    match value.as_str() {
        Some("ranked" | "approved") => Ok("ranked".into()),
        Some("loved") => Ok("loved".into()),
        Some("pending" | "wip" | "graveyard" | "qualified" | "unranked") => Ok("unranked".into()),
        _ => Err(INVALID),
    }
}

fn string(value: &Value, optional_empty: bool) -> Result<String, &'static str> {
    bounded_string(value, optional_empty, MAX_STRING_BYTES)
}
fn bounded_string(
    value: &Value,
    optional_empty: bool,
    maximum: usize,
) -> Result<String, &'static str> {
    let value = value
        .as_str()
        .filter(|value| value.len() <= maximum)
        .ok_or(INVALID)?;
    let value: String = value
        .chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .collect();
    let value = value.trim().to_owned();
    if !optional_empty && value.is_empty() {
        return Err(INVALID);
    }
    Ok(value)
}

fn canonical_string(value: &str, optional_empty: bool) -> bool {
    canonical_bounded_string(value, optional_empty, MAX_STRING_BYTES)
}
fn canonical_bounded_string(value: &str, optional_empty: bool, maximum: usize) -> bool {
    value.len() <= maximum
        && (optional_empty || !value.is_empty())
        && value.trim() == value
        && !value.chars().any(|c| c.is_control() || c == '|')
}

fn integer(value: &Value) -> Result<i32, &'static str> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value >= 0)
        .ok_or(INVALID)
}

fn number(value: &Value, maximum: f64) -> Result<f64, &'static str> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && (0.0..=maximum).contains(value))
        .ok_or(INVALID)
}

fn valid_date(value: &str) -> bool {
    value.len() <= 64
        && value.trim() == value
        && chrono::DateTime::parse_from_rfc3339(value)
            .is_ok_and(|date| (1970..=9999).contains(&date.year()))
}
fn date(value: &Value) -> Result<String, &'static str> {
    value
        .as_str()
        .filter(|value| valid_date(value))
        .map(str::to_owned)
        .ok_or(INVALID)
}
fn optional_date(value: Option<&Value>) -> Result<Option<String>, &'static str> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => date(value).map(Some),
    }
}
fn classification(value: &Value) -> Result<OfficialSetClassification, &'static str> {
    let result = OfficialSetClassification {
        id: integer(&value["id"])?,
        name: string(&value["name"], false)?,
    };
    if !(1..=1000).contains(&result.id) || result.name.len() > 128 {
        return Err(INVALID);
    }
    Ok(result)
}

impl OfficialSetMetadata {
    /// Select only original difficulties and the fields needed by native clients.
    /// Unknown upstream fields, including asset URLs and conversion relations,
    /// are never carried through this DTO.
    pub fn from_upstream(value: &Value, requested_id: i64) -> Result<Self, &'static str> {
        if !(1..=MAX_OFFICIAL_ID).contains(&requested_id)
            || value["id"].as_i64() != Some(requested_id)
        {
            return Err(INVALID);
        }
        let maps = value["beatmaps"]
            .as_array()
            .filter(|maps| (1..=512).contains(&maps.len()))
            .ok_or(INVALID)?;
        let mut beatmaps = Vec::with_capacity(maps.len());
        for map in maps {
            if map.get("convert").is_some_and(|value| value != false)
                || map.get("deleted_at").is_some_and(|value| !value.is_null())
            {
                return Err(INVALID);
            }
            let mode_int = map["mode_int"]
                .as_i64()
                .filter(|mode| (0..=3).contains(mode))
                .ok_or(INVALID)? as i16;
            if map.get("mode").is_some_and(|value| {
                value.as_str() != Some(["osu", "taiko", "fruits", "mania"][mode_int as usize])
            }) {
                return Err(INVALID);
            }
            beatmaps.push(OfficialSetDifficulty {
                id: map["id"].as_i64().ok_or(INVALID)?,
                beatmapset_id: map["beatmapset_id"].as_i64().ok_or(INVALID)?,
                checksum: map["checksum"]
                    .as_str()
                    .ok_or(INVALID)?
                    .to_ascii_lowercase(),
                version: string(&map["version"], false)?,
                mode_int,
                status: normalized_status(&map["status"])?,
                last_updated: date(&map["last_updated"])?,
                difficulty_rating: number(&map["difficulty_rating"], 1000.0)?,
                bpm: number(&map["bpm"], 10000.0)?,
                total_length: integer(&map["total_length"])?,
                hit_length: integer(&map["hit_length"])?,
                max_combo: match map.get("max_combo") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(integer(value)?),
                },
                ar: number(&map["ar"], 20.0)?,
                cs: number(&map["cs"], 20.0)?,
                od: number(&map["accuracy"], 20.0)?,
                hp: number(&map["drain"], 20.0)?,
                count_circles: integer(&map["count_circles"])?,
                count_sliders: integer(&map["count_sliders"])?,
                count_spinners: integer(&map["count_spinners"])?,
            });
        }
        let result = Self {
            id: requested_id,
            artist: string(&value["artist"], false)?,
            artist_unicode: string(&value["artist_unicode"], true)?,
            title: string(&value["title"], false)?,
            title_unicode: string(&value["title_unicode"], true)?,
            creator: string(&value["creator"], false)?,
            status: normalized_status(&value["status"])?,
            bpm: number(&value["bpm"], 10000.0)?,
            source: string(&value["source"], true)?,
            tags: bounded_string(&value["tags"], true, 4096)?,
            nsfw: value["nsfw"].as_bool().ok_or(INVALID)?,
            download_disabled: value
                .get("availability")
                .and_then(|v| v.get("download_disabled"))
                .and_then(Value::as_bool)
                != Some(false),
            archive_download_available: false,
            submitted_date: date(&value["submitted_date"])?,
            last_updated: optional_date(value.get("last_updated"))?,
            ranked_date: optional_date(value.get("ranked_date"))?,
            genre: classification(&value["genre"])?,
            language: classification(&value["language"])?,
            beatmaps,
        };
        result.validate(requested_id)?;
        Ok(result)
    }

    /// Validate an already normalized response received from the service.
    pub fn validate(&self, requested_id: i64) -> Result<(), &'static str> {
        if self.id != requested_id
            || !(1..=MAX_OFFICIAL_ID).contains(&self.id)
            || !(1..=512).contains(&self.beatmaps.len())
            || !canonical_string(&self.artist, false)
            || !canonical_string(&self.artist_unicode, true)
            || !canonical_string(&self.title, false)
            || !canonical_string(&self.title_unicode, true)
            || !canonical_string(&self.creator, false)
            || !self.bpm.is_finite()
            || !(0.0..=10000.0).contains(&self.bpm)
            || self.bpm == 0.0
            || !canonical_string(&self.source, true)
            || !canonical_bounded_string(&self.tags, true, 4096)
            || !valid_date(&self.submitted_date)
            || self
                .last_updated
                .as_ref()
                .is_some_and(|date| !valid_date(date))
            || self
                .ranked_date
                .as_ref()
                .is_some_and(|date| !valid_date(date))
            || [&self.genre, &self.language].iter().any(|classification| {
                !(1..=1000).contains(&classification.id)
                    || classification.name.len() > 128
                    || !canonical_string(&classification.name, false)
            })
            || !matches!(self.status.as_str(), "ranked" | "loved" | "unranked")
        {
            return Err(INVALID);
        }
        let mut ids = HashSet::new();
        let mut hashes = HashSet::new();
        for map in &self.beatmaps {
            if !(1..=MAX_OFFICIAL_ID).contains(&map.id)
                || map.beatmapset_id != self.id
                || !ids.insert(map.id)
                || !hashes.insert(map.checksum.as_str())
                || map.checksum.len() != 32
                || !map
                    .checksum
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                || !canonical_string(&map.version, false)
                || !(0..=3).contains(&map.mode_int)
                || !matches!(map.status.as_str(), "ranked" | "loved" | "unranked")
                || !valid_date(&map.last_updated)
                || !map.difficulty_rating.is_finite()
                || !(0.0..=1000.0).contains(&map.difficulty_rating)
                || !map.bpm.is_finite()
                || !(0.0..=10000.0).contains(&map.bpm)
                || [map.ar, map.cs, map.od, map.hp]
                    .iter()
                    .any(|number| !number.is_finite() || !(0.0..=20.0).contains(number))
                || map.total_length < 0
                || map.hit_length < 0
                || map.hit_length > map.total_length
                || map.max_combo.is_some_and(|combo| combo < 0)
                || [map.count_circles, map.count_sliders, map.count_spinners]
                    .iter()
                    .any(|count| *count < 0)
                || i64::from(map.count_circles)
                    + i64::from(map.count_sliders)
                    + i64::from(map.count_spinners)
                    > i64::from(i32::MAX)
            {
                return Err(INVALID);
            }
        }
        if serde_json::to_vec(self).map_err(|_| INVALID)?.len() > MAX_METADATA_RESPONSE_BYTES {
            return Err(INVALID);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn upstream() -> Value {
        json!({"id":42,"artist":"Artist","artist_unicode":"歌手","title":"Title","title_unicode":"標題","creator":"Creator","status":"approved","bpm":180.5,"source":"","tags":"original fixture","nsfw":false,"submitted_date":"2020-01-02T03:04:05Z","last_updated":"2020-02-02T03:04:05Z","ranked_date":null,"genre":{"id":5,"name":"Video Game"},"language":{"id":3,"name":"Japanese"},"beatmaps":[{"id":100,"beatmapset_id":42,"checksum":"ABCDEF0123456789ABCDEF0123456789","version":"Hard","mode_int":0,"mode":"osu","status":"ranked","last_updated":"2020-02-02T03:04:05Z","difficulty_rating":5.25,"bpm":180.5,"total_length":150,"hit_length":135,"max_combo":432,"ar":9.5,"cs":4.0,"accuracy":8.5,"drain":6.0,"count_circles":100,"count_sliders":140,"count_spinners":2}]})
    }

    #[test]
    fn original_metadata_preserves_numbers_and_filters_upstream_fields() {
        let mut value = upstream();
        value["converts"] = json!([{"id":9999999999_i64}]);
        value["user_id"] = json!(99);
        value["covers"] = json!({"cover":"https://example.invalid/private"});
        value["artist"] = json!(" Artist|\nname ");
        let parsed = OfficialSetMetadata::from_upstream(&value, 42).unwrap();
        assert_eq!(parsed.artist, "Artist  name");
        assert_eq!(parsed.status, "ranked");
        assert_eq!(parsed.beatmaps[0].od, 8.5);
        assert_eq!(parsed.beatmaps[0].hp, 6.0);
        assert_eq!(parsed.beatmaps[0].bpm, 180.5);
        assert_eq!(
            parsed.beatmaps[0].checksum,
            "abcdef0123456789abcdef0123456789"
        );
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert!(serialized.get("covers").is_none());
        assert!(serialized.get("user_id").is_none());
        assert!(serialized.get("converts").is_none());
        assert_eq!(
            serde_json::from_value::<OfficialSetMetadata>(serialized).unwrap(),
            parsed
        );
    }

    #[test]
    fn invalid_identity_members_and_numbers_are_rejected() {
        for (pointer, replacement) in [
            ("/id", json!(43)),
            ("/status", json!("unknown")),
            ("/artist", json!(" ")),
            ("/title", json!("x".repeat(1025))),
            ("/bpm", json!(0)),
            ("/nsfw", json!("false")),
            ("/tags", json!("x".repeat(4097))),
            ("/submitted_date", json!("0001-01-01T00:00:00Z")),
            ("/last_updated", json!("not a date")),
            ("/genre/id", json!(0)),
            ("/language/name", json!("x".repeat(129))),
            ("/beatmaps/0/last_updated", Value::Null),
            ("/beatmaps/0/id", json!(MAX_OFFICIAL_ID + 1)),
            ("/beatmaps/0/id", json!(0)),
            ("/beatmaps/0/beatmapset_id", json!(43)),
            ("/beatmaps/0/checksum", json!("g".repeat(32))),
            ("/beatmaps/0/mode_int", json!(4)),
            ("/beatmaps/0/mode", json!("mania")),
            ("/beatmaps/0/difficulty_rating", json!(-1)),
            ("/beatmaps/0/bpm", json!("180")),
            ("/beatmaps/0/hit_length", json!(151)),
            ("/beatmaps/0/count_sliders", json!(-1)),
            ("/beatmaps/0/accuracy", json!(21)),
            ("/beatmaps/0/drain", Value::Null),
            ("/beatmaps/0/max_combo", json!(1.5)),
        ] {
            let mut value = upstream();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                OfficialSetMetadata::from_upstream(&value, 42).is_err(),
                "{pointer}"
            );
        }
        for field in ["convert", "deleted_at"] {
            let mut value = upstream();
            value["beatmaps"][0][field] = json!(true);
            assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
        }
        let mut value = upstream();
        value["beatmaps"][0]["convert"] = Value::Null;
        assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
        let map = value["beatmaps"][0].clone();
        for count in [0, 513] {
            value["beatmaps"] = json!(vec![map.clone(); count]);
            assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
        }
        value = upstream();
        let duplicate = value["beatmaps"][0].clone();
        value["beatmaps"].as_array_mut().unwrap().push(duplicate);
        assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
        value["beatmaps"][1]["id"] = json!(101);
        assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
    }

    #[test]
    fn service_response_is_strict_and_revalidated() {
        let parsed = OfficialSetMetadata::from_upstream(&upstream(), 42).unwrap();
        let mut value = serde_json::to_value(&parsed).unwrap();
        value["asset_url"] = json!("https://example.invalid");
        assert!(serde_json::from_value::<OfficialSetMetadata>(value).is_err());
        let mut value = serde_json::to_value(&parsed).unwrap();
        value["beatmaps"][0]["user_id"] = json!(1);
        assert!(serde_json::from_value::<OfficialSetMetadata>(value).is_err());
        assert!(parsed.validate(43).is_err());
        let mut parsed = parsed;
        parsed.beatmaps[0].ar = f64::NAN;
        assert!(parsed.validate(42).is_err());
        parsed.beatmaps[0].ar = 9.5;
        parsed.title.push('|');
        assert!(parsed.validate(42).is_err());
    }

    #[test]
    fn optional_combo_and_modes_accept_only_real_original_values() {
        for (mode, name) in ["osu", "taiko", "fruits", "mania"].into_iter().enumerate() {
            let mut value = upstream();
            value["beatmaps"][0]["mode_int"] = json!(mode);
            value["beatmaps"][0]["mode"] = json!(name);
            value["beatmaps"][0]["max_combo"] = Value::Null;
            value["beatmaps"][0]["convert"] = json!(false);
            value["beatmaps"][0]["deleted_at"] = Value::Null;
            value["beatmaps"][0]["status"] = json!("qualified");
            let parsed = OfficialSetMetadata::from_upstream(&value, 42).unwrap();
            assert_eq!(parsed.beatmaps[0].max_combo, None);
            assert_eq!(parsed.beatmaps[0].mode_int, mode as i16);
            assert_eq!(parsed.beatmaps[0].status, "unranked");
            value["beatmaps"][0]
                .as_object_mut()
                .unwrap()
                .remove("max_combo");
            assert!(OfficialSetMetadata::from_upstream(&value, 42).is_ok());
        }
    }

    #[test]
    fn unknown_download_policy_never_grants_new_acquisition() {
        for policy in [
            Value::Null,
            json!({}),
            json!({"download_disabled":true}),
            json!({"download_disabled":"false"}),
        ] {
            let mut value = upstream();
            value["availability"] = policy;
            let dto = OfficialSetMetadata::from_upstream(&value, 42).unwrap();
            assert!(dto.download_disabled);
            assert!(!dto.archive_download_available);
        }
        let mut value = upstream();
        value["availability"] = json!({"download_disabled":false});
        let dto = OfficialSetMetadata::from_upstream(&value, 42).unwrap();
        assert!(!dto.download_disabled);
        assert!(
            !dto.archive_download_available,
            "metadata does not certify retained assets"
        );
        let mut encoded = serde_json::to_value(dto).unwrap();
        encoded.as_object_mut().unwrap().remove("download_disabled");
        encoded
            .as_object_mut()
            .unwrap()
            .remove("archive_download_available");
        let legacy: OfficialSetMetadata = serde_json::from_value(encoded).unwrap();
        assert!(legacy.download_disabled);
        assert!(!legacy.archive_download_available);
    }

    #[test]
    fn total_object_count_stays_within_native_integer_range() {
        let mut value = upstream();
        value["beatmaps"][0]["count_circles"] = json!(i32::MAX - 1);
        value["beatmaps"][0]["count_sliders"] = json!(1);
        value["beatmaps"][0]["count_spinners"] = json!(0);
        let mut parsed = OfficialSetMetadata::from_upstream(&value, 42).unwrap();
        parsed.validate(42).unwrap();
        value["beatmaps"][0]["count_spinners"] = json!(1);
        assert!(OfficialSetMetadata::from_upstream(&value, 42).is_err());
        parsed.beatmaps[0].count_spinners = 1;
        assert!(parsed.validate(42).is_err());
    }
}
