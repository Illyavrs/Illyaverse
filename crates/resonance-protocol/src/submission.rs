//! osu!stable's encrypted score envelope. The legacy transport encryption and
//! public MD5 checksum provide interoperability, not a trustworthy anti-cheat.
use crate::{
    ProtocolError, Result,
    stable::{Writer, is_md5},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::NaiveDateTime;
use simple_rijndael::rijndael::Rijndael;
use std::io::{self, Write};

pub const MAX_ENVELOPE_BYTES: usize = 16_384;
pub const MAX_FRAME_TEXT: usize = 32 * 1024 * 1024;
/// Reserve room for stable's metadata and online score ID inside the shared
/// 16 MiB stored/downloadable replay bound. Use the same cap at upload time.
pub const MAX_STABLE_COMPRESSED_REPLAY_BYTES: usize = crate::replay::MAX_REPLAY_BYTES - 1024;

pub fn decrypt_score_field(encoded: &str, iv_encoded: &str, version: &str) -> Result<String> {
    if encoded.len() > MAX_ENVELOPE_BYTES * 2 {
        return Err(ProtocolError::Limit("encrypted score field"));
    }
    if version.len() != 8 || !version.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ProtocolError::Invalid("score version"));
    }
    let iv = STANDARD
        .decode(iv_encoded)
        .map_err(|_| ProtocolError::Invalid("score IV"))?;
    if iv.len() != 32 {
        return Err(ProtocolError::Invalid("score IV"));
    }
    let ciphertext = STANDARD
        .decode(encoded)
        .map_err(|_| ProtocolError::Invalid("score base64"))?;
    if ciphertext.is_empty() || ciphertext.len() % 32 != 0 || ciphertext.len() > MAX_ENVELOPE_BYTES
    {
        return Err(ProtocolError::Invalid("score ciphertext size"));
    }
    // The date-based key is a public constant of the stable wire format.
    let mut key = b"osu!-scoreburgr".to_vec();
    key.extend([b'-'; 9]);
    key.extend(version.as_bytes());
    let cipher = Rijndael::new(&key, 32).map_err(|_| ProtocolError::Invalid("score cipher"))?;
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    let mut previous = iv.as_slice();
    for block in ciphertext.chunks_exact(32) {
        let decrypted = cipher
            .decrypt(block)
            .map_err(|_| ProtocolError::Invalid("score cipher"))?;
        plaintext.extend(decrypted.iter().zip(previous).map(|(a, b)| a ^ b));
        previous = block;
    }
    let padding = usize::from(
        *plaintext
            .last()
            .ok_or(ProtocolError::Invalid("score padding"))?,
    );
    if padding == 0
        || padding > 32
        || plaintext[plaintext.len() - padding..]
            .iter()
            .any(|&byte| usize::from(byte) != padding)
    {
        return Err(ProtocolError::Invalid("score padding"));
    }
    plaintext.truncate(plaintext.len() - padding);
    String::from_utf8(plaintext).map_err(|_| ProtocolError::Utf8)
}

#[derive(Debug, Clone)]
pub struct StableSubmission {
    pub beatmap_md5: String,
    pub username: String,
    pub checksum: String,
    pub hit_counts: [u16; 6],
    pub total_score: i32,
    pub max_combo: u16,
    pub perfect: bool,
    pub rank: String,
    pub mods: u32,
    pub passed: bool,
    pub mode: u8,
    pub played_at: NaiveDateTime,
    pub client_version: String,
}
impl StableSubmission {
    pub fn parse(plaintext: &str) -> Result<Self> {
        if plaintext.len() > MAX_ENVELOPE_BYTES {
            return Err(ProtocolError::Limit("score envelope"));
        }
        // The first 18 fields are the canonical stable score. Newer clients
        // may append opaque extension data; never interpret it as score,
        // account, checksum or replay authority. Keep the tail in one slice
        // so a bounded envelope cannot expand the field allocation.
        let fields: Vec<_> = plaintext.splitn(19, ':').collect();
        if fields.len() < 18 {
            return Err(ProtocolError::Invalid("score field count"));
        }
        if !is_md5(fields[0]) || !is_md5(fields[2]) {
            return Err(ProtocolError::Invalid("score digest"));
        }
        let username = fields[1].strip_suffix(' ').unwrap_or(fields[1]);
        if username.is_empty() || username.len() > 64 || username.chars().any(char::is_control) {
            return Err(ProtocolError::Invalid("score username"));
        }
        let mut hit_counts = [0; 6];
        for (index, count) in hit_counts.iter_mut().enumerate() {
            *count = fields[index + 3]
                .parse()
                .map_err(|_| ProtocolError::Invalid("score hit counts"))?;
        }
        let total_score: i32 = fields[9]
            .parse()
            .map_err(|_| ProtocolError::Invalid("score total"))?;
        if total_score < 0 {
            return Err(ProtocolError::Invalid("score total"));
        }
        let max_combo = fields[10]
            .parse()
            .map_err(|_| ProtocolError::Invalid("score combo"))?;
        let boolean = |value| match value {
            "True" => Ok(true),
            "False" => Ok(false),
            _ => Err(ProtocolError::Invalid("score boolean")),
        };
        let rank = fields[12].to_ascii_uppercase();
        if !matches!(
            rank.as_str(),
            "XH" | "X" | "SH" | "S" | "A" | "B" | "C" | "D" | "F" | "N"
        ) {
            return Err(ProtocolError::Invalid("score grade"));
        }
        let mods: u32 = fields[13]
            .parse()
            .map_err(|_| ProtocolError::Invalid("score mods"))?;
        if mods > i32::MAX as u32 {
            return Err(ProtocolError::Invalid("score mods"));
        }
        let mode: u8 = fields[15]
            .parse()
            .map_err(|_| ProtocolError::Invalid("score mode"))?;
        if mode > 3 {
            return Err(ProtocolError::Invalid("score mode"));
        }
        if fields[16].len() != 12 {
            return Err(ProtocolError::Invalid("score timestamp"));
        }
        let played_at = NaiveDateTime::parse_from_str(fields[16], "%y%m%d%H%M%S")
            .map_err(|_| ProtocolError::Invalid("score timestamp"))?;
        // Stable appends one ASCII space per legacy client-flag value to the
        // version field. Bound the version separately from that suffix; the
        // complete decrypted envelope remains bounded above.
        let client_version = fields[17].trim_end_matches(' ');
        if client_version.len() > 128 || client_version.trim().len() < 8 {
            return Err(ProtocolError::Invalid("score client version"));
        }
        Ok(Self {
            beatmap_md5: fields[0].to_ascii_lowercase(),
            username: username.to_owned(),
            checksum: fields[2].to_ascii_lowercase(),
            hit_counts,
            total_score,
            max_combo,
            perfect: boolean(fields[11])?,
            rank,
            mods,
            passed: boolean(fields[14])?,
            mode,
            played_at,
            client_version: client_version.trim().to_owned(),
        })
    }
    pub fn online_checksum(
        &self,
        version: &str,
        client_hash: &str,
        storyboard_hash: &str,
    ) -> String {
        let [n300, n100, n50, geki, katu, miss] = self.hit_counts;
        let mut data = String::new();
        let mut append = |value: &str| data.push_str(value);
        append("chickenmcnuggets");
        append(&(u32::from(n100) + u32::from(n300)).to_string());
        append("o15");
        append(&n50.to_string());
        append(&geki.to_string());
        append("smustard");
        append(&katu.to_string());
        append(&miss.to_string());
        append("uu");
        append(&self.beatmap_md5);
        append(&self.max_combo.to_string());
        append(if self.perfect { "True" } else { "False" });
        append(&self.username);
        append(&self.total_score.to_string());
        append(&self.rank);
        append(&self.mods.to_string());
        append("Q");
        append(if self.passed { "True" } else { "False" });
        append(&self.mode.to_string());
        append(version);
        append(&self.played_at.format("%y%m%d%H%M%S").to_string());
        append(client_hash);
        append(storyboard_hash);
        format!("{:x}", md5::compute(data.as_bytes()))
    }
    pub fn validate_checksum(
        &self,
        version: &str,
        client_hash: &str,
        storyboard_hash: &str,
    ) -> Result<()> {
        if self.online_checksum(version, client_hash, storyboard_hash) != self.checksum {
            return Err(ProtocolError::Invalid("online score checksum"));
        }
        Ok(())
    }
    pub fn replay_container(&self, version: i32, frames: &[u8], score_id: i64) -> Result<Vec<u8>> {
        if frames.len() > MAX_STABLE_COMPRESSED_REPLAY_BYTES {
            return Err(ProtocolError::Limit("replay frames"));
        }
        let mut writer = Writer::new();
        writer
            .u8(self.mode)
            .i32(version)
            .string(&self.beatmap_md5)?
            .string(&self.username)?
            .string(&self.checksum)?;
        for count in self.hit_counts {
            writer.u16(count);
        }
        // No life-bar data is present in the stable submission endpoint.
        let ticks = self
            .played_at
            .and_utc()
            .timestamp()
            .checked_mul(10_000_000)
            .and_then(|v| v.checked_add(621_355_968_000_000_000))
            .ok_or(ProtocolError::Invalid("replay timestamp"))?;
        writer
            .i32(self.total_score)
            .u16(self.max_combo)
            .boolean(self.perfect)
            .u32(self.mods)
            .string("")?
            .i64(ticks)
            .i32(frames.len() as i32);
        let mut output = writer.into_bytes();
        if output.len().saturating_add(frames.len()).saturating_add(8)
            > crate::replay::MAX_REPLAY_BYTES
        {
            return Err(ProtocolError::Limit("replay size"));
        }
        output.extend(frames);
        output.extend(score_id.to_le_bytes());
        Ok(output)
    }
}

struct BoundedOutput(Vec<u8>);
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FRAME_TEXT {
            return Err(io::Error::other("replay output limit"));
        }
        self.0.extend(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
/// Structural validation only. Authentic cursor timing and judgement agreement
/// still require deterministic gameplay re-simulation before automatic ranking.
pub fn validate_compressed_frames(compressed: &[u8]) -> Result<usize> {
    if compressed.len() < 13 || compressed.len() > MAX_STABLE_COMPRESSED_REPLAY_BYTES {
        return Err(ProtocolError::Invalid("compressed replay size"));
    }
    let mut output = BoundedOutput(Vec::new());
    let mut source = io::Cursor::new(compressed);
    let options = lzma_rs::decompress::Options {
        memlimit: Some(64 * 1024 * 1024),
        ..Default::default()
    };
    lzma_rs::lzma_decompress_with_options(&mut source, &mut output, &options)
        .map_err(|_| ProtocolError::Invalid("compressed replay data"))?;
    if source.position() != compressed.len() as u64 {
        return Err(ProtocolError::TrailingData);
    }
    let text = std::str::from_utf8(&output.0).map_err(|_| ProtocolError::Utf8)?;
    let mut count = 0;
    for entry in text.split(',').filter(|entry| !entry.is_empty()) {
        count += 1;
        if count > 1_000_000 {
            return Err(ProtocolError::Limit("replay frame count"));
        }
        let fields: Vec<_> = entry.split('|').collect();
        if fields.len() != 4 {
            return Err(ProtocolError::Invalid("replay frame"));
        }
        let delta: i64 = fields[0]
            .parse()
            .map_err(|_| ProtocolError::Invalid("replay time"))?;
        if !(-3_600_000..=86_400_000).contains(&delta) {
            return Err(ProtocolError::Invalid("replay time"));
        }
        for field in &fields[1..3] {
            let coordinate: f64 = field
                .parse()
                .map_err(|_| ProtocolError::Invalid("replay coordinate"))?;
            if !coordinate.is_finite() || coordinate.abs() > 1_000_000.0 {
                return Err(ProtocolError::Invalid("replay coordinate"));
            }
        }
        fields[3]
            .parse::<i32>()
            .map_err(|_| ProtocolError::Invalid("replay keys"))?;
    }
    if count == 0 {
        return Err(ProtocolError::Invalid("empty replay"));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope_with_version(version: &str) -> String {
        format!(
            "0123456789abcdef0123456789abcdef:Illya:00000000000000000000000000000000:10:0:0:0:0:0:1000:10:True:X:0:True:0:261007120000:{version}"
        )
    }

    #[test]
    fn stable_canonical_prefix_ignores_opaque_extensions_without_changing_identity() {
        for version in ["20261007", "b20261007"] {
            let unsigned = envelope_with_version(version);
            let checksum = StableSubmission::parse(&unsigned).unwrap().online_checksum(
                "20261007",
                "synthetic-client",
                "",
            );
            let input = unsigned.replacen(&"0".repeat(32), &checksum, 1);
            let baseline = StableSubmission::parse(&input).unwrap();
            let replay = baseline.replay_container(20261007, &[1, 2, 3], 42).unwrap();
            for tail in [
                "",
                "12345",
                "synthetic-opaque-extension",
                "synthetic:first:second:third",
            ] {
                for flag_spaces in [0, 512] {
                    let extended = format!("{input}{}:{tail}", " ".repeat(flag_spaces));
                    let parsed = StableSubmission::parse(&extended).unwrap();
                    assert_eq!(parsed.client_version, version);
                    assert_eq!(parsed.checksum, checksum);
                    parsed
                        .validate_checksum("20261007", "synthetic-client", "")
                        .unwrap();
                    assert_eq!(
                        parsed.replay_container(20261007, &[1, 2, 3], 42).unwrap(),
                        replay,
                    );
                }
            }
        }
    }

    #[test]
    fn stable_opaque_extensions_do_not_bypass_canonical_field_validation() {
        let input = envelope_with_version("20261007");
        assert_eq!(input.split(':').count(), 18);
        let truncated = input.rsplit_once(':').unwrap().0;
        assert_eq!(truncated.split(':').count(), 17);
        assert_eq!(
            StableSubmission::parse(truncated).unwrap_err(),
            ProtocolError::Invalid("score field count"),
        );
        for (index, value, reason) in [
            (0, "invalid-digest", "score digest"),
            (1, "", "score username"),
            (2, "invalid-checksum", "score digest"),
            (3, "-1", "score hit counts"),
            (9, "-1", "score total"),
            (10, "-1", "score combo"),
            (11, "true", "score boolean"),
            (12, "Z", "score grade"),
            (13, "4294967295", "score mods"),
            (14, "false", "score boolean"),
            (15, "4", "score mode"),
            (16, "invalid-date", "score timestamp"),
            (17, "short", "score client version"),
        ] {
            let mut fields: Vec<_> = input.split(':').collect();
            fields[index] = value;
            let extended = format!("{}:opaque:extra", fields.join(":"));
            assert_eq!(
                StableSubmission::parse(&extended).unwrap_err(),
                ProtocolError::Invalid(reason),
            );
        }
    }

    #[test]
    fn stable_opaque_tail_remains_subject_to_full_envelope_bound() {
        let prefix = format!("{}:", envelope_with_version("20261007"));
        let input = format!("{prefix}{}", "x".repeat(MAX_ENVELOPE_BYTES - prefix.len()));
        assert_eq!(input.len(), MAX_ENVELOPE_BYTES);
        assert_eq!(input.split(':').count(), 19);
        assert_eq!(
            StableSubmission::parse(&input).unwrap().client_version,
            "20261007"
        );
        assert_eq!(
            StableSubmission::parse(&format!("{input}x")).unwrap_err(),
            ProtocolError::Limit("score envelope"),
        );
    }

    #[test]
    fn stable_ascii_space_flags_preserve_version_checksum_and_replay_metadata() {
        for version in ["20261007", "b20261007"] {
            let unsigned = envelope_with_version(version);
            let checksum = StableSubmission::parse(&unsigned).unwrap().online_checksum(
                "20261007",
                "synthetic-client",
                "",
            );
            let input = unsigned.replacen(&"0".repeat(32), &checksum, 1);
            let baseline = StableSubmission::parse(&input).unwrap();
            let baseline_replay = baseline.replay_container(20261007, &[1, 2, 3], 42).unwrap();
            for flags in [0, 128, 256, 512] {
                let parsed =
                    StableSubmission::parse(&format!("{input}{}", " ".repeat(flags))).unwrap();
                assert_eq!(parsed.client_version, version);
                assert_eq!(parsed.checksum, checksum);
                parsed
                    .validate_checksum("20261007", "synthetic-client", "")
                    .unwrap();
                assert_eq!(
                    parsed.replay_container(20261007, &[1, 2, 3], 42).unwrap(),
                    baseline_replay,
                );
            }
        }
    }

    #[test]
    fn stable_flag_suffix_keeps_normalized_version_length_bounds() {
        for length in [8, 128] {
            let version = "v".repeat(length);
            for flags in [0, 512] {
                let input = envelope_with_version(&format!("{version}{}", " ".repeat(flags)));
                assert_eq!(
                    StableSubmission::parse(&input).unwrap().client_version,
                    version
                );
            }
        }
        for length in [7, 129] {
            for flags in [0, 512] {
                let input =
                    envelope_with_version(&format!("{}{}", "v".repeat(length), " ".repeat(flags)));
                assert_eq!(
                    StableSubmission::parse(&input).unwrap_err(),
                    ProtocolError::Invalid("score client version"),
                );
            }
        }
    }

    #[test]
    fn stable_flag_suffix_remains_subject_to_full_envelope_bound() {
        let input = envelope_with_version("b20261007");
        let input = format!("{input}{}", " ".repeat(MAX_ENVELOPE_BYTES - input.len()));
        assert_eq!(input.len(), MAX_ENVELOPE_BYTES);
        assert_eq!(
            StableSubmission::parse(&input).unwrap().client_version,
            "b20261007"
        );
        assert_eq!(
            StableSubmission::parse(&format!("{input} ")).unwrap_err(),
            ProtocolError::Limit("score envelope"),
        );
    }

    #[test]
    fn stable_flag_suffix_does_not_relax_non_space_padding_bounds() {
        for version in [
            format!("20261007{}", "\t".repeat(128)),
            format!("20261007{}", "\n".repeat(128)),
            format!("20261007{}", "\u{2003}".repeat(128)),
            format!("{}20261007", " ".repeat(512)),
            format!("2026{}1007", " ".repeat(512)),
            format!("20261007{}\t", " ".repeat(512)),
        ] {
            assert_eq!(
                StableSubmission::parse(&envelope_with_version(&version)).unwrap_err(),
                ProtocolError::Invalid("score client version"),
            );
        }
    }
    #[test]
    fn decrypts_independent_python_rijndael256_fixture() {
        // Produced with py3rijndael 0.3.3, IV bytes 0..31; no reference repo code.
        let encoded = "pJsNdT2HCJw5VHoDWor2p91R3PTIsUiKMJwVdLuRMZ+MdXdKnrVqFGlR/nDhv27jTOlot2VncUKcKZDmYN+5/Q==";
        let iv = STANDARD.encode((0..32).collect::<Vec<u8>>());
        assert_eq!(
            decrypt_score_field(encoded, &iv, "20261007").unwrap(),
            "Illyaverse independent protocol fixture"
        );
        assert!(decrypt_score_field(encoded, &iv, "20261008").is_err());
        assert!(decrypt_score_field("", &iv, "20261007").is_err());
        assert!(decrypt_score_field(encoded, "AA==", "20261007").is_err());
    }
    #[test]
    fn envelope_validates_fields_and_containers_preserve_metadata() {
        let input = "0123456789abcdef0123456789abcdef:Illya:00000000000000000000000000000000:10:0:0:0:0:0:1000:10:True:X:0:True:0:261007120000:20261007";
        let mut score = StableSubmission::parse(input).unwrap();
        score.checksum = score.online_checksum("20261007", "client", "");
        score.validate_checksum("20261007", "client", "").unwrap();
        assert!(
            score
                .validate_checksum("20261007", "different", "")
                .is_err()
        );
        let bytes = score.replay_container(20261007, &[1, 2, 3], 42).unwrap();
        let replay = crate::replay::LegacyReplay::parse(&bytes).unwrap();
        assert_eq!(replay.hit_counts, score.hit_counts);
        assert_eq!(replay.online_score_id, Some(42));
        assert_eq!(replay.timestamp_ticks, 639269712000000000);
        assert!(StableSubmission::parse(&input.replace(":10:0:0", ":-1:0:0")).is_err());
    }

    #[test]
    fn stable_frame_capacity_fits_the_stored_container_bound() {
        let mut score = StableSubmission::parse(&envelope_with_version("20261007")).unwrap();
        score.username = "x".repeat(64);
        // Opaque bytes test serialization capacity only, not valid LZMA frames.
        let frames = vec![0x81; MAX_STABLE_COMPRESSED_REPLAY_BYTES];
        let bytes = score.replay_container(20261007, &frames, 42).unwrap();
        assert!(bytes.len() <= crate::replay::MAX_REPLAY_BYTES);
        let parsed = crate::replay::LegacyReplay::parse(&bytes).unwrap();
        assert_eq!(parsed.compressed_frames, frames);
        assert_eq!(parsed.online_score_id, Some(42));
        let mut too_large = frames;
        too_large.push(0x81);
        assert_eq!(
            score
                .replay_container(20261007, &too_large, 42)
                .unwrap_err(),
            ProtocolError::Limit("replay frames"),
        );
        assert_eq!(
            validate_compressed_frames(&too_large).unwrap_err(),
            ProtocolError::Invalid("compressed replay size"),
        );
    }

    #[test]
    fn stable_serializer_checks_the_complete_container_bound() {
        let mut score = StableSubmission::parse(&envelope_with_version("20261007")).unwrap();
        // Public structs can be constructed without parse's username limit.
        score.username = "x".repeat(2048);
        let frames = vec![0x81; MAX_STABLE_COMPRESSED_REPLAY_BYTES];
        assert_eq!(
            score.replay_container(20261007, &frames, 42).unwrap_err(),
            ProtocolError::Limit("replay size"),
        );
    }

    #[test]
    fn lzma_frames_have_bounded_and_finite_contents() {
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(
            &mut io::Cursor::new(b"0|256|192|0,16|300|200|5,-12345|0|0|123,"),
            &mut compressed,
        )
        .unwrap();
        assert_eq!(validate_compressed_frames(&compressed).unwrap(), 3);
        let mut bad = Vec::new();
        lzma_rs::lzma_compress(&mut io::Cursor::new(b"0|NaN|0|0,"), &mut bad).unwrap();
        assert!(validate_compressed_frames(&bad).is_err());
        assert!(validate_compressed_frames(b"not lzma frames").is_err());
    }
}
