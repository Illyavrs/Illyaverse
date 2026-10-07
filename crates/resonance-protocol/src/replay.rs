//! Structural parsing of the legacy .osr container. Compressed frame data is
//! retained as bytes; this module does not decompress or verify gameplay.

use crate::{
    ProtocolError, Result,
    stable::{Reader, is_md5},
};

pub const MAX_REPLAY_BYTES: usize = 16_777_216;

#[derive(Debug, PartialEq)]
pub struct LegacyReplay<'a> {
    pub mode: u8,
    pub client_version: i32,
    pub beatmap_md5: &'a str,
    pub player_name: &'a str,
    pub replay_md5: &'a str,
    /// 300, 100, 50, geki, katu, miss, in the container's order.
    pub hit_counts: [u16; 6],
    pub total_score: i32,
    pub max_combo: u16,
    pub perfect: bool,
    pub mods: u32,
    pub life_bar: &'a str,
    /// .NET ticks, not Unix seconds.
    pub timestamp_ticks: i64,
    pub compressed_frames: &'a [u8],
    pub online_score_id: Option<i64>,
    pub target_practice_accuracy: Option<f64>,
}

impl<'a> LegacyReplay<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > MAX_REPLAY_BYTES {
            return Err(ProtocolError::Limit("replay size"));
        }
        let mut reader = Reader::new(bytes);
        let mode = reader.u8()?;
        if mode > 3 {
            return Err(ProtocolError::Invalid("replay mode"));
        }
        let client_version = reader.i32()?;
        let beatmap_md5 = reader.string()?;
        let player_name = reader.string()?;
        let replay_md5 = reader.string()?;
        if !is_md5(beatmap_md5) || (!replay_md5.is_empty() && !is_md5(replay_md5)) {
            return Err(ProtocolError::Invalid("replay digest"));
        }
        let mut hit_counts = [0; 6];
        for count in &mut hit_counts {
            *count = reader.u16()?;
        }
        let total_score = reader.i32()?;
        if total_score < 0 {
            return Err(ProtocolError::Invalid("replay score"));
        }
        let max_combo = reader.u16()?;
        let perfect = reader.boolean()?;
        let mods = reader.u32()?;
        let life_bar = reader.string()?;
        let timestamp_ticks = reader.i64()?;
        let frame_bytes = reader.i32()?;
        if frame_bytes < 0 {
            return Err(ProtocolError::Invalid("compressed replay length"));
        }
        let compressed_frames = reader.take(frame_bytes as usize)?;
        // Older legacy files omit the online score ID. Never synthesize one.
        let online_score_id = if reader.remaining() >= 8 {
            Some(reader.i64()?)
        } else {
            None
        };
        let target_practice_accuracy = if mods & (1 << 23) != 0 && reader.remaining() >= 8 {
            let value = reader.f64()?;
            if !value.is_finite() {
                return Err(ProtocolError::Invalid("target practice accuracy"));
            }
            Some(value)
        } else {
            None
        };
        reader.finish()?;
        Ok(Self {
            mode,
            client_version,
            beatmap_md5,
            player_name,
            replay_md5,
            hit_counts,
            total_score,
            max_combo,
            perfect,
            mods,
            life_bar,
            timestamp_ticks,
            compressed_frames,
            online_score_id,
            target_practice_accuracy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stable::Writer;

    fn fixture(frame_length: i32) -> Vec<u8> {
        let mut writer = Writer::new();
        writer
            .u8(0)
            .i32(20261007)
            .string("0123456789abcdef0123456789abcdef")
            .unwrap()
            .string("Illya")
            .unwrap()
            .string("")
            .unwrap();
        for _ in 0..6 {
            writer.u16(0);
        }
        writer
            .i32(1000)
            .u16(10)
            .boolean(false)
            .u32(0)
            .string("")
            .unwrap()
            .i64(638000000000000000)
            .i32(frame_length);
        writer.into_bytes()
    }
    #[test]
    fn replay_container_parses_without_claiming_frame_verification() {
        let mut bytes = fixture(3);
        bytes.extend([1, 2, 3]);
        bytes.extend(42i64.to_le_bytes());
        let replay = LegacyReplay::parse(&bytes).unwrap();
        assert_eq!(replay.compressed_frames, &[1, 2, 3]);
        assert_eq!(replay.online_score_id, Some(42));
    }
    #[test]
    fn replay_rejects_negative_lengths_and_truncated_frames() {
        assert!(LegacyReplay::parse(&fixture(-1)).is_err());
        assert!(LegacyReplay::parse(&fixture(100)).is_err());
        let bytes = fixture(0);
        for length in 0..bytes.len() {
            assert!(LegacyReplay::parse(&bytes[..length]).is_err());
        }
    }
}
