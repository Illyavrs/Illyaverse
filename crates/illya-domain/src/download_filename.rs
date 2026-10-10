//! Player-facing download names, independent of storage paths and identities.
//! Unicode is carried with RFC 5987; the quoted fallback is ordinary ASCII.

const MAX_NAME_BYTES: usize = 240;

fn clean_component(value: &str) -> String {
    let mut result = String::new();
    let mut space = false;
    for ch in value.chars() {
        if ch.is_control()
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            continue;
        }
        if ch.is_whitespace() {
            space = !result.is_empty();
        } else {
            if space {
                result.push(' ');
                space = false;
            }
            result.push(ch);
        }
    }
    result.trim_matches([' ', '.']).to_owned()
}

/// The ID is the canonical native set identity, including the reserved local
/// namespace. This name does not change the archive's contents or authority.
pub fn archive_filename(set_id: i64, artist: &str, title: &str) -> Option<String> {
    if set_id <= 0 {
        return None;
    }
    let artist = clean_component(artist);
    let title = clean_component(title);
    let mut stem = format!(
        "{set_id} {} - {}",
        if artist.is_empty() {
            "Unknown Artist"
        } else {
            &artist
        },
        if title.is_empty() { "Untitled" } else { &title }
    );
    while stem.len() > MAX_NAME_BYTES - 4 {
        stem.pop();
    }
    Some(format!("{}.osz", stem.trim_end_matches([' ', '.'])))
}

pub fn replay_filename(mode: i16, difficulty_id: i64, score_id: i64) -> Option<String> {
    if difficulty_id <= 0 || score_id <= 0 {
        return None;
    }
    let mode = match mode {
        0 => "osu",
        1 => "taiko",
        2 => "fruits",
        3 => "mania",
        _ => return None,
    };
    Some(format!(
        "illyaverse-replay-{mode}_{difficulty_id}_{score_id}.osr"
    ))
}

/// Input names are re-sanitized so even a metadata value containing header or
/// path syntax can never escape the attachment. The extension stays intact.
pub fn attachment(filename: &str) -> String {
    let filename = clean_component(filename);
    let fallback: String = filename
        .chars()
        .map(|ch| if ch.is_ascii() { ch } else { '_' })
        .collect();
    let mut encoded = String::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~'
            )
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

/// Read only the exact disposition contract emitted above. Internal service
/// transport must not turn an arbitrary upstream header into a browser name.
pub fn attachment_filename(value: &str) -> Option<String> {
    if value.len() > 1200 || !value.is_ascii() {
        return None;
    }
    let (_, encoded) = value.split_once("; filename*=UTF-8''")?;
    let mut bytes = Vec::new();
    let mut input = encoded.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = char::from(input.next()?).to_digit(16)?;
            let low = char::from(input.next()?).to_digit(16)?;
            bytes.push(((high << 4) | low) as u8);
        } else {
            bytes.push(byte);
        }
        if bytes.len() > MAX_NAME_BYTES {
            return None;
        }
    }
    let filename = String::from_utf8(bytes).ok()?;
    (!filename.is_empty() && attachment(&filename) == value).then_some(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_style_archive_names_preserve_readable_metadata() {
        let name = archive_filename(
            2630282,
            "Yorushika",
            "Kumo o Nuke Kazashimo e Watashi dake (TV Size)",
        )
        .unwrap();
        assert_eq!(
            name,
            "2630282 Yorushika - Kumo o Nuke Kazashimo e Watashi dake (TV Size).osz"
        );
        assert!(attachment(&name).starts_with(&format!("attachment; filename=\"{name}\";")));
        assert_eq!(archive_filename(0, "Artist", "Title"), None);
    }

    #[test]
    fn unicode_header_keeps_utf8_name_and_safe_ascii_fallback() {
        let name = archive_filename(2_000_000_001, "ヨルシカ", "雲を抜け風下へ私だけ").unwrap();
        let header = attachment(&name);
        assert!(header.is_ascii());
        assert!(header.contains("filename=\"2000000001 ____ - __________.osz\""));
        assert!(header.contains("filename*=UTF-8''2000000001%20%E3%83%A8"));
        assert!(!header.contains(['\r', '\n', '\\', '\0']));
        assert_eq!(attachment_filename(&header), Some(name));
    }

    #[test]
    fn hostile_metadata_cannot_inject_paths_headers_or_bidi() {
        let name = archive_filename(42, " ../A\\B:\"\r\nX ", "C?*<>|\u{202e}D%';. ").unwrap();
        assert_eq!(name, "42 ABX - CD%';.osz");
        let header = attachment(&name);
        assert!(header.contains("CD%25%27%3B.osz"));
        assert!(!header.contains(['\r', '\n', '\\', '\0']));
        assert!(header.matches("filename=").count() == 1);
    }

    #[test]
    fn filename_bound_keeps_unicode_boundaries_and_extension() {
        let name = archive_filename(i64::MAX, &"界".repeat(300), &"音".repeat(300)).unwrap();
        assert!(name.len() <= MAX_NAME_BYTES);
        assert!(name.ends_with(".osz"));
        assert!(std::str::from_utf8(name.as_bytes()).is_ok());
        assert!(attachment(&name).is_ascii());
    }

    #[test]
    fn replay_names_bind_mode_native_difficulty_and_score() {
        for (mode, slug) in [(0, "osu"), (1, "taiko"), (2, "fruits"), (3, "mania")] {
            assert_eq!(
                replay_filename(mode, 5463158, 7210038779),
                Some(format!("illyaverse-replay-{slug}_5463158_7210038779.osr"))
            );
        }
        assert_eq!(replay_filename(4, 1, 1), None);
        assert_eq!(replay_filename(0, 0, 1), None);
        assert_eq!(replay_filename(0, 1, -1), None);
    }

    #[test]
    fn internal_header_contract_rejects_mismatched_names_and_path_encoding() {
        for header in [
            "attachment; filename=\"safe.osz\"; filename*=UTF-8''evil.osz",
            "attachment; filename=\"a.osz\"; filename*=UTF-8''%2Fa.osz",
            "attachment; filename=\"a.osz\"; filename*=UTF-8''a%0D%0A.osz",
            "attachment; filename=\"a.osz\"; filename*=UTF-8''a%GG.osz",
            "attachment; filename=\"a.osz\"; filename*=UTF-8''a%FF.osz",
            "attachment; filename=\"a.osz\"; filename*=UTF-8''a.osz; extra=1",
        ] {
            assert_eq!(attachment_filename(header), None, "{header}");
        }
    }
}
