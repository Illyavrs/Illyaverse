//! Original bounded implementation of the public extended .osr container contract.
//! Lazer keeps the legacy binary header and frames, then appends length-prefixed
//! LZMA JSON metadata. Structural agreement does not prove authentic gameplay.
use crate::{
    ProtocolError, Result,
    lazer::{ApiMod, SoloScoreSubmission},
    replay::{LegacyReplay, MAX_REPLAY_BYTES},
    stable::{Reader, Writer},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Write},
};

pub const LAZER_VERSION: i32 = 30_000_019;
pub const MAX_METADATA_BYTES: usize = 1_048_576;
pub const MAX_FRAME_TEXT: usize = 32 * 1024 * 1024;
pub const MAX_FRAMES: usize = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetadata {
    pub online_id: i64,
    pub user_id: i64,
    #[serde(default)]
    pub mods: Vec<ApiMod>,
    #[serde(default)]
    pub statistics: BTreeMap<String, u32>,
    #[serde(default)]
    pub maximum_statistics: BTreeMap<String, u32>,
    #[serde(default)]
    pub client_version: String,
    #[serde(default)]
    pub rank: Option<String>,
    #[serde(default)]
    pub total_score_without_mods: Option<u64>,
    #[serde(default)]
    pub pauses: Vec<u32>,
}
#[derive(Debug)]
pub struct ExtendedReplay<'a> {
    pub legacy: LegacyReplay<'a>,
    pub metadata: ReplayMetadata,
    pub frame_count: usize,
}
struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(io::Error::other("replay decompression limit"));
        }
        self.bytes.extend(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn decompress(compressed: &[u8], limit: usize) -> Result<Vec<u8>> {
    if compressed.len() < 13 || compressed.len() > MAX_REPLAY_BYTES {
        return Err(ProtocolError::Invalid("compressed replay size"));
    }
    let dictionary = u32::from_le_bytes(compressed[1..5].try_into().unwrap());
    if dictionary > 8 * 1024 * 1024 {
        return Err(ProtocolError::Limit("LZMA dictionary"));
    }
    let declared = u64::from_le_bytes(compressed[5..13].try_into().unwrap());
    if declared != u64::MAX && declared > limit as u64 {
        return Err(ProtocolError::Limit("LZMA output size"));
    }
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        limit,
    };
    let mut input = Cursor::new(compressed);
    let options = lzma_rs::decompress::Options {
        memlimit: Some(64 * 1024 * 1024),
        ..Default::default()
    };
    lzma_rs::lzma_decompress_with_options(&mut input, &mut output, &options)
        .map_err(|_| ProtocolError::Invalid("compressed replay data"))?;
    if input.position() != compressed.len() as u64 {
        return Err(ProtocolError::TrailingData);
    }
    Ok(output.bytes)
}
fn compress(text: &[u8]) -> Result<Vec<u8>> {
    let mut compressed = Vec::new();
    lzma_rs::lzma_compress(&mut Cursor::new(text), &mut compressed)
        .map_err(|_| ProtocolError::Invalid("replay compression"))?;
    Ok(compressed)
}
pub fn validate_frame_text(bytes: &[u8]) -> Result<usize> {
    if bytes.len() > MAX_FRAME_TEXT {
        return Err(ProtocolError::Limit("replay frame text"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ProtocolError::Utf8)?;
    let mut count = 0;
    for frame in text.split(',').filter(|s| !s.is_empty()) {
        let fields = frame.split('|').collect::<Vec<_>>();
        if fields.len() != 4 {
            return Err(ProtocolError::Invalid("replay frame"));
        }
        let delta = fields[0]
            .parse::<i64>()
            .map_err(|_| ProtocolError::Invalid("frame delta"))?;
        if !(-86_400_000..=86_400_000).contains(&delta) {
            return Err(ProtocolError::Invalid("frame delta"));
        }
        for coordinate in &fields[1..3] {
            if !coordinate
                .parse::<f64>()
                .is_ok_and(|v| v.is_finite() && v.abs() <= 1_048_575.0)
            {
                return Err(ProtocolError::Invalid("frame coordinate"));
            }
        }
        let keys = fields[3]
            .parse::<u32>()
            .map_err(|_| ProtocolError::Invalid("frame buttons"))?;
        if delta == -12345 {
            continue;
        }
        if keys > 31 {
            return Err(ProtocolError::Invalid("frame buttons"));
        }
        count += 1;
        if count > MAX_FRAMES {
            return Err(ProtocolError::Limit("replay frame count"));
        }
    }
    if count == 0 {
        return Err(ProtocolError::Invalid("empty replay"));
    }
    Ok(count)
}
pub fn canonical_statistics(stats: &BTreeMap<String, u32>) -> BTreeMap<String, u32> {
    stats
        .iter()
        .filter(|(_, v)| **v != 0)
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}
pub fn statistics_from_signalr(value: &serde_json::Value) -> Result<BTreeMap<String, u32>> {
    let names = [
        "none",
        "miss",
        "meh",
        "ok",
        "good",
        "great",
        "perfect",
        "small_tick_miss",
        "small_tick_hit",
        "large_tick_miss",
        "large_tick_hit",
        "small_bonus",
        "large_bonus",
        "ignore_miss",
        "ignore_hit",
        "combo_break",
        "slider_tail_hit",
    ];
    let values = value
        .as_object()
        .filter(|v| v.len() <= 40)
        .ok_or(ProtocolError::Invalid("statistics"))?;
    let mut stats = BTreeMap::new();
    for (key, value) in values {
        let name = key
            .parse::<usize>()
            .ok()
            .and_then(|i| names.get(i))
            .copied()
            .or_else(|| names.iter().find(|name| **name == key).copied())
            .or_else(|| {
                (key == "99" || key == "legacy_combo_increase").then_some("legacy_combo_increase")
            })
            .ok_or(ProtocolError::Invalid("hit result"))?;
        let count = value
            .as_u64()
            .filter(|v| *v <= 10_000_000)
            .ok_or(ProtocolError::Invalid("hit count"))? as u32;
        if stats.insert(name.into(), count).is_some() {
            return Err(ProtocolError::Invalid("duplicate hit result"));
        }
    }
    Ok(canonical_statistics(&stats))
}
impl<'a> ExtendedReplay<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        Self::parse_container(bytes, false)
    }

    /// Offline exports may have no online identity. This does not authorize
    /// native publication; callers must bind an authenticated owner and review.
    pub fn parse_for_review(bytes: &'a [u8]) -> Result<Self> {
        Self::parse_container(bytes, true)
    }

    fn parse_container(bytes: &'a [u8], for_review: bool) -> Result<Self> {
        if bytes.len() > MAX_REPLAY_BYTES {
            return Err(ProtocolError::Limit("replay size"));
        }
        let mut reader = Reader::new(bytes);
        reader.u8()?;
        let version = reader.i32()?;
        if !(30_000_001..31_000_000).contains(&version) {
            return Err(ProtocolError::Invalid("extended replay version"));
        }
        for _ in 0..3 {
            reader.string()?;
        }
        reader.take(12 + 4 + 2 + 1 + 4)?;
        reader.string()?;
        reader.i64()?;
        let length = reader.i32()?;
        if length < 13 {
            return Err(ProtocolError::Invalid("replay frame size"));
        }
        reader.take(length as usize)?;
        reader.i64()?;
        let prefix_len = bytes.len() - reader.remaining();
        let legacy = LegacyReplay::parse(&bytes[..prefix_len])?;
        if legacy.timestamp_ticks <= 0 || legacy.timestamp_ticks > 3_155_378_975_999_999_999 {
            return Err(ProtocolError::Invalid("replay timestamp"));
        }
        let length = reader.i32()?;
        if length < 13 {
            return Err(ProtocolError::Invalid("replay metadata size"));
        }
        let text = decompress(reader.take(length as usize)?, MAX_METADATA_BYTES)?;
        reader.finish()?;
        let metadata: ReplayMetadata = serde_json::from_slice(&text)
            .map_err(|_| ProtocolError::Invalid("replay metadata JSON"))?;
        if for_review {
            if metadata.online_id < -1 || metadata.user_id < -1 {
                return Err(ProtocolError::Invalid("replay identity"));
            }
            let mut validation = metadata.clone();
            validation.online_id = validation.online_id.max(1);
            validation.user_id = validation.user_id.max(1);
            validation.validate()?;
        } else {
            metadata.validate()?;
        }
        let frame_count =
            validate_frame_text(&decompress(legacy.compressed_frames, MAX_FRAME_TEXT)?)?;
        Ok(Self {
            legacy,
            metadata,
            frame_count,
        })
    }
    pub fn matches(&self, score: &ReplayScore<'_>) -> Result<()> {
        let actual = &self.legacy;
        let expected = score.submission;
        if actual.beatmap_md5 != score.beatmap_md5
            || actual.player_name != score.username
            || actual.mode != expected.ruleset_id
            || i64::from(actual.total_score) != expected.total_score as i64
            || u32::from(actual.max_combo) != expected.max_combo
            || actual.mods != score.mods_bits
            || self.metadata.online_id != score.online_id
            || self.metadata.user_id != score.user_id
            || serde_json::to_value(&self.metadata.mods).ok()
                != serde_json::to_value(&expected.mods).ok()
            || canonical_statistics(&self.metadata.statistics)
                != canonical_statistics(&expected.statistics)
            || canonical_statistics(&self.metadata.maximum_statistics)
                != canonical_statistics(&expected.maximum_statistics)
            || self.metadata.rank.as_deref() != Some(expected.rank.as_str())
            || self.metadata.pauses != expected.pauses
            || self.metadata.total_score_without_mods.unwrap_or(0)
                != expected.total_score_without_mods
            || actual.hit_counts != legacy_counts(expected)?
        {
            return Err(ProtocolError::Invalid(
                "replay metadata does not match score",
            ));
        }
        Ok(())
    }
}
impl ReplayMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.online_id < 1
            || !(1..=i64::from(i32::MAX)).contains(&self.user_id)
            || self.client_version.len() > 128
            || self.client_version.chars().any(char::is_control)
        {
            return Err(ProtocolError::Invalid("replay identity"));
        }
        let submission = SoloScoreSubmission {
            ruleset_id: 0,
            passed: true,
            total_score: 0,
            total_score_without_mods: self.total_score_without_mods.unwrap_or(0),
            accuracy: 1.0,
            max_combo: 0,
            rank: self.rank.clone().unwrap_or_else(|| "F".into()),
            mods: self.mods.clone(),
            statistics: self.statistics.clone(),
            maximum_statistics: self.maximum_statistics.clone(),
            pauses: self.pauses.clone(),
        };
        submission.validate()?;
        for stats in [&self.statistics, &self.maximum_statistics] {
            statistics_from_signalr(
                &serde_json::to_value(stats).map_err(|_| ProtocolError::Invalid("statistics"))?,
            )?;
        }
        if self.pauses.iter().any(|p| *p > 86_400_000) {
            return Err(ProtocolError::Invalid("pause time"));
        }
        Ok(())
    }
}
pub struct ReplayScore<'a> {
    pub online_id: i64,
    pub user_id: i64,
    pub username: &'a str,
    pub beatmap_md5: &'a str,
    pub client_version: &'a str,
    pub mods_bits: u32,
    pub timestamp_ticks: i64,
    pub submission: &'a SoloScoreSubmission,
}
fn legacy_counts(score: &SoloScoreSubmission) -> Result<[u16; 6]> {
    let n = |key| {
        u16::try_from(*score.statistics.get(key).unwrap_or(&0))
            .map_err(|_| ProtocolError::Limit("legacy hit count"))
    };
    let (n100, n50, katu) = if score.ruleset_id == 2 {
        ("large_tick_hit", "small_tick_hit", "small_tick_miss")
    } else {
        ("ok", "meh", "good")
    };
    Ok([
        n("great")?,
        n(n100)?,
        n(n50)?,
        n("perfect")?,
        n(katu)?,
        n("miss")?,
    ])
}
pub fn encode(score: &ReplayScore<'_>, frame_text: &[u8]) -> Result<Vec<u8>> {
    score.submission.validate()?;
    validate_frame_text(frame_text)?;
    let metadata = ReplayMetadata {
        online_id: score.online_id,
        user_id: score.user_id,
        mods: score.submission.mods.clone(),
        statistics: canonical_statistics(&score.submission.statistics),
        maximum_statistics: canonical_statistics(&score.submission.maximum_statistics),
        client_version: score.client_version.into(),
        rank: Some(score.submission.rank.clone()),
        total_score_without_mods: (score.submission.total_score_without_mods > 0)
            .then_some(score.submission.total_score_without_mods),
        pauses: score.submission.pauses.clone(),
    };
    metadata.validate()?;
    let mut writer = Writer::new();
    writer
        .u8(score.submission.ruleset_id)
        .i32(LAZER_VERSION)
        .string(score.beatmap_md5)?
        .string(score.username)?
        .string("")?;
    for count in legacy_counts(score.submission)? {
        writer.u16(count);
    }
    writer
        .i32(
            i32::try_from(score.submission.total_score)
                .map_err(|_| ProtocolError::Limit("replay total score"))?,
        )
        .u16(
            u16::try_from(score.submission.max_combo)
                .map_err(|_| ProtocolError::Limit("replay combo"))?,
        )
        .boolean(false)
        .u32(score.mods_bits)
        .string("")?
        .i64(score.timestamp_ticks);
    let mut bytes = writer.into_bytes();
    let frames = compress(frame_text)?;
    bytes.extend((frames.len() as i32).to_le_bytes());
    bytes.extend(frames);
    bytes.extend((-1i64).to_le_bytes());
    let metadata = compress(
        &serde_json::to_vec(&metadata).map_err(|_| ProtocolError::Invalid("metadata JSON"))?,
    )?;
    bytes.extend((metadata.len() as i32).to_le_bytes());
    bytes.extend(metadata);
    if bytes.len() > MAX_REPLAY_BYTES {
        return Err(ProtocolError::Limit("replay size"));
    }
    ExtendedReplay::parse(&bytes)?.matches(score)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_exports_are_only_accepted_for_manual_review() {
        let bytes = include_bytes!("fixtures/lazer-native.osr");
        let parsed = ExtendedReplay::parse(bytes).unwrap();
        let offset = parsed.legacy.compressed_frames.as_ptr() as usize - bytes.as_ptr() as usize
            + parsed.legacy.compressed_frames.len()
            + 8;
        for id in [0, -1, -2] {
            let mut metadata = parsed.metadata.clone();
            metadata.online_id = id;
            metadata.user_id = id;
            let compressed = compress(&serde_json::to_vec(&metadata).unwrap()).unwrap();
            let mut offline = bytes[..offset].to_vec();
            offline.extend((compressed.len() as i32).to_le_bytes());
            offline.extend(compressed);
            assert!(ExtendedReplay::parse(&offline).is_err());
            if id >= -1 {
                let review = ExtendedReplay::parse_for_review(&offline).unwrap();
                assert_eq!(review.metadata.user_id, id);
                assert_eq!(review.metadata.online_id, id);
                assert_eq!(review.frame_count, parsed.frame_count);
            } else {
                assert!(ExtendedReplay::parse_for_review(&offline).is_err());
            }
        }
    }
    #[test]
    fn reads_actual_installed_native_export() {
        // Synthetic fixture, generated by the installed official DLL encoder:
        // osu.Game 2026.1005.1-tachyon+6cbcae7fbb764839a66c1adf564c2e1679242072.
        // No user files, account data or upstream implementation are included.
        let bytes = include_bytes!("fixtures/lazer-native.osr");
        let parsed = ExtendedReplay::parse(bytes).unwrap();
        assert_eq!(parsed.legacy.client_version, LAZER_VERSION);
        assert_eq!(parsed.frame_count, 2);
        assert_eq!(parsed.metadata.online_id, 42);
        assert_eq!(parsed.metadata.user_id, 7);
        assert_eq!(parsed.metadata.rank.as_deref(), Some("X"));
        assert_eq!(parsed.metadata.statistics.get("great"), Some(&2));
        assert_eq!(parsed.metadata.mods[0].acronym, "DT");
        assert_eq!(parsed.metadata.mods[0].settings["speed_change"], 1.2);
        assert_eq!(parsed.legacy.mods, 64);
    }
    #[test]
    fn extended_container_binds_identity_mods_and_judgements() {
        let submission = SoloScoreSubmission {
            ruleset_id: 0,
            passed: true,
            total_score: 1234,
            total_score_without_mods: 1234,
            accuracy: 1.0,
            max_combo: 2,
            rank: "X".into(),
            mods: vec![],
            statistics: BTreeMap::from([("great".into(), 2)]),
            maximum_statistics: BTreeMap::from([("great".into(), 2)]),
            pauses: vec![],
        };
        let mut score = ReplayScore {
            online_id: 42,
            user_id: 7,
            username: "Fixture",
            beatmap_md5: "0123456789abcdef0123456789abcdef",
            client_version: "2026.1005.1",
            mods_bits: 0,
            timestamp_ticks: 639269732961234567,
            submission: &submission,
        };
        let bytes = encode(&score, b"0|256|192|0,100|256|192|1,-12345|0|0|0").unwrap();
        let replay = ExtendedReplay::parse(&bytes).unwrap();
        assert_eq!(replay.frame_count, 2);
        replay.matches(&score).unwrap();
        score.online_id = 43;
        assert!(replay.matches(&score).is_err());
        assert!(LegacyReplay::parse(&bytes).is_err());
        for n in 0..bytes.len() {
            assert!(ExtendedReplay::parse(&bytes[..n]).is_err());
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(ExtendedReplay::parse(&trailing).is_err());
    }
    #[test]
    fn rejects_decompression_bombs_and_invalid_frames() {
        let compressed = compress(&vec![b'x'; 1024]).unwrap();
        assert!(decompress(&compressed, 100).is_err());
        let mut dictionary = compressed.clone();
        dictionary[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decompress(&dictionary, 2048).is_err());
        assert!(validate_frame_text(b"0|NaN|0|0").is_err());
        assert!(validate_frame_text(b"0|0|0|32").is_err());
        assert!(validate_frame_text(b"-12345|0|0|0").is_err());
        assert_eq!(
            statistics_from_signalr(&serde_json::json!({"5":2,"1":0})).unwrap(),
            BTreeMap::from([("great".into(), 2)])
        );
    }
}
