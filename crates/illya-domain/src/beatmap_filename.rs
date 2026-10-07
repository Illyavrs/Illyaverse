//! Exact native beatmap filename selectors. Names are identities, never paths.

pub const MAX_BEATMAP_FILENAME_BYTES: usize = 1024;

pub fn valid_beatmap_filename(filename: &str) -> bool {
    (5..=MAX_BEATMAP_FILENAME_BYTES).contains(&filename.len())
        && filename.as_bytes()[filename.len() - 4..].eq_ignore_ascii_case(b".osu")
        && !filename
            .chars()
            .any(|character| matches!(character, '/' | '\\' | ':') || character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_exact_unicode_basenames_with_an_ascii_osu_suffix() {
        for filename in [
            "a.osu",
            "Movement.OSU",
            "星の曲 [難しい].osu",
            " leading.osu",
            "e\u{301}.osu",
        ] {
            assert!(valid_beatmap_filename(filename));
        }
        for filename in [
            "",
            ".osu",
            "a.osz",
            "a.osu ",
            "a/track.osu",
            "a\\track.osu",
            "C:track.osu",
            "a\0.osu",
            "a\n.osu",
            "a\u{85}.osu",
        ] {
            assert!(!valid_beatmap_filename(filename));
        }
    }

    #[test]
    fn bounds_count_utf8_bytes_without_normalizing_the_name() {
        let exact = format!("{}.osu", "星".repeat(340));
        assert_eq!(exact.len(), MAX_BEATMAP_FILENAME_BYTES);
        assert!(valid_beatmap_filename(&exact));
        assert!(!valid_beatmap_filename(&format!("x{exact}")));
        assert!(valid_beatmap_filename("é.osu"));
        assert!(valid_beatmap_filename("e\u{301}.osu"));
        assert_ne!("é.osu", "e\u{301}.osu");
    }
}
