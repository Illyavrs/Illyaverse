//! Handwritten ZIP records using owned synthetic names and contents.
use super::*;
use crate::maps::{unpack_archive, unpack_archive_with_policy};

struct Member<'a> {
    name: &'a [u8],
    data: &'a [u8],
    flags: u16,
    extra: Vec<u8>,
    local_extra: Option<Vec<u8>>,
    attributes: u32,
    size: Option<u32>,
    comment: Vec<u8>,
    descriptor: bool,
    descriptor_wide: bool,
    descriptor_signed: bool,
    central_wide_sizes: bool,
}

impl<'a> Member<'a> {
    fn new(name: &'a [u8], data: &'a [u8]) -> Self {
        Self {
            name,
            data,
            flags: 0,
            extra: Vec::new(),
            local_extra: None,
            attributes: 0,
            size: None,
            comment: Vec::new(),
            descriptor: false,
            descriptor_wide: false,
            descriptor_signed: true,
            central_wide_sizes: false,
        }
    }
}

struct Fixture {
    bytes: Vec<u8>,
    local: Vec<usize>,
    central: Vec<usize>,
    end: usize,
}

fn push16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend(value.to_le_bytes());
}
fn push32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}
fn push64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend(value.to_le_bytes());
}
fn set16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn set32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn extra(tag: u16, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    push16(&mut bytes, tag);
    push16(&mut bytes, data.len().try_into().unwrap());
    bytes.extend(data);
    bytes
}

fn unicode_path(raw: &[u8], name: &[u8]) -> Vec<u8> {
    let mut data = vec![1];
    push32(&mut data, crc32fast::hash(raw));
    data.extend(name);
    extra(0x7075, &data)
}

fn fixture(members: &[Member<'_>], prefix: &[u8], zip64: bool) -> Fixture {
    let mut bytes = prefix.to_vec();
    let mut local = Vec::new();
    for member in members {
        local.push(bytes.len());
        let local_extra = member.local_extra.as_ref().unwrap_or(&member.extra);
        push32(&mut bytes, LOCAL);
        push16(&mut bytes, if member.descriptor_wide { 45 } else { 20 });
        push16(&mut bytes, member.flags);
        push16(&mut bytes, 0); // stored
        push32(&mut bytes, 0); // time and date
        push32(
            &mut bytes,
            if member.descriptor {
                0
            } else {
                crc32fast::hash(member.data)
            },
        );
        push32(
            &mut bytes,
            if member.descriptor {
                0
            } else {
                member.data.len() as u32
            },
        );
        push32(
            &mut bytes,
            if member.descriptor {
                0
            } else {
                member.size.unwrap_or(member.data.len() as u32)
            },
        );
        push16(&mut bytes, member.name.len().try_into().unwrap());
        push16(&mut bytes, local_extra.len().try_into().unwrap());
        bytes.extend(member.name);
        bytes.extend(local_extra);
        bytes.extend(member.data);
        if member.descriptor {
            if member.descriptor_signed {
                push32(&mut bytes, 0x0807_4b50);
            }
            push32(&mut bytes, crc32fast::hash(member.data));
            if member.descriptor_wide {
                push64(&mut bytes, member.data.len() as u64);
                push64(
                    &mut bytes,
                    u64::from(member.size.unwrap_or(member.data.len() as u32)),
                );
            } else {
                push32(&mut bytes, member.data.len() as u32);
                push32(&mut bytes, member.size.unwrap_or(member.data.len() as u32));
            }
        }
    }
    let directory_start = bytes.len();
    let mut central = Vec::new();
    for (index, member) in members.iter().enumerate() {
        central.push(bytes.len());
        push32(&mut bytes, CENTRAL);
        push16(&mut bytes, (3 << 8) | 20); // Unix host, original synthetic fixture
        push16(&mut bytes, if member.descriptor_wide { 45 } else { 20 });
        push16(&mut bytes, member.flags);
        push16(&mut bytes, 0);
        push32(&mut bytes, 0);
        push32(&mut bytes, crc32fast::hash(member.data));
        push32(
            &mut bytes,
            if member.central_wide_sizes {
                u32::MAX
            } else {
                member.data.len() as u32
            },
        );
        push32(
            &mut bytes,
            if member.central_wide_sizes {
                u32::MAX
            } else {
                member.size.unwrap_or(member.data.len() as u32)
            },
        );
        push16(&mut bytes, member.name.len().try_into().unwrap());
        push16(&mut bytes, member.extra.len().try_into().unwrap());
        push16(&mut bytes, member.comment.len().try_into().unwrap());
        push16(&mut bytes, 0); // disk
        push16(&mut bytes, 0); // internal attributes
        push32(&mut bytes, member.attributes);
        push32(&mut bytes, (local[index] - prefix.len()) as u32);
        bytes.extend(member.name);
        bytes.extend(&member.extra);
        bytes.extend(&member.comment);
    }
    let directory_size = bytes.len() - directory_start;
    if zip64 {
        let zip64_start = bytes.len();
        push32(&mut bytes, ZIP64_END);
        push64(&mut bytes, 44);
        push16(&mut bytes, 45);
        push16(&mut bytes, 45);
        push32(&mut bytes, 0);
        push32(&mut bytes, 0);
        push64(&mut bytes, members.len() as u64);
        push64(&mut bytes, members.len() as u64);
        push64(&mut bytes, directory_size as u64);
        push64(&mut bytes, (directory_start - prefix.len()) as u64);
        push32(&mut bytes, ZIP64_LOCATOR);
        push32(&mut bytes, 0);
        push64(&mut bytes, (zip64_start - prefix.len()) as u64);
        push32(&mut bytes, 1);
    }
    let end = bytes.len();
    push32(&mut bytes, END);
    push16(&mut bytes, 0);
    push16(&mut bytes, 0);
    push16(
        &mut bytes,
        if zip64 {
            u16::MAX
        } else {
            members.len() as u16
        },
    );
    push16(
        &mut bytes,
        if zip64 {
            u16::MAX
        } else {
            members.len() as u16
        },
    );
    push32(
        &mut bytes,
        if zip64 {
            u32::MAX
        } else {
            directory_size as u32
        },
    );
    push32(
        &mut bytes,
        if zip64 {
            u32::MAX
        } else {
            (directory_start - prefix.len()) as u32
        },
    );
    push16(&mut bytes, 0);
    Fixture {
        bytes,
        local,
        central,
        end,
    }
}

pub(crate) fn stored_archive(members: &[(&[u8], u16, &[u8])]) -> Vec<u8> {
    let members = members
        .iter()
        .map(|(name, flags, data)| {
            let mut member = Member::new(name, data);
            member.flags = *flags;
            member
        })
        .collect::<Vec<_>>();
    fixture(&members, &[], false).bytes
}

fn append_signature(mut archive: Fixture, payload: &[u8], zip64: bool) -> Fixture {
    let at = if zip64 { archive.end - 76 } else { archive.end };
    let old_size = if zip64 {
        u64_at(&archive.bytes, at + 40).unwrap()
    } else {
        u64::from(u32_at(&archive.bytes, archive.end + 12).unwrap())
    };
    let old_zip64_offset = if zip64 {
        u64_at(&archive.bytes, archive.end - 12).unwrap()
    } else {
        0
    };
    let mut signature = Vec::new();
    push32(&mut signature, 0x0505_4b50);
    push16(&mut signature, payload.len().try_into().unwrap());
    signature.extend(payload);
    let length = signature.len();
    drop(archive.bytes.splice(at..at, signature));
    archive.end += length;
    if zip64 {
        archive.bytes[at + length + 40..at + length + 48]
            .copy_from_slice(&(old_size + length as u64).to_le_bytes());
        archive.bytes[archive.end - 12..archive.end - 4]
            .copy_from_slice(&(old_zip64_offset + length as u64).to_le_bytes());
    } else {
        set32(
            &mut archive.bytes,
            archive.end + 12,
            (old_size + length as u64) as u32,
        );
    }
    archive
}

#[test]
fn original_names_follow_declared_utf8_cp932_and_unicode_path() {
    for (raw, flags, expected) in [
        (b"Easy.osu".as_slice(), 0, "Easy.osu"),
        ("星.osu".as_bytes(), 1 << 11, "星.osu"),
        (b"\x90\xaf.osu".as_slice(), 0, "星.osu"),
        (b"\x83\x5c.osu".as_slice(), 0, "ソ.osu"),
        ("é.osu".as_bytes(), 0, "ﾃｩ.osu"),
    ] {
        let bytes = stored_archive(&[(raw, flags, b"owned map bytes")]);
        let maps = unpack_archive(&bytes).unwrap();
        assert_eq!(maps, [(expected.to_owned(), b"owned map bytes".to_vec())]);
    }
    let raw = b"legacy.osu";
    let mut member = Member::new(raw, b"owned Unicode override bytes");
    member.extra = unicode_path(raw, "雪.osu".as_bytes());
    member.local_extra = Some(Vec::new()); // a central-only override is valid
    let bytes = fixture(&[member], &[], false).bytes;
    assert_eq!(unpack_archive(&bytes).unwrap()[0].0, "雪.osu");
    let raw = b"\x83\xfd.osu";
    let mut member = Member::new(raw, b"owned overridden legacy bytes");
    member.extra = unicode_path(raw, "星.osu".as_bytes());
    assert_eq!(
        unpack_archive(&fixture(&[member], &[], false).bytes).unwrap()[0].0,
        "星.osu"
    );
}

#[test]
fn cp932_standalone_extensions_preserve_valid_trails_and_reject_invalid_trails() {
    for (raw, expected) in [
        (b"\xa0.osu".as_slice(), "\u{f8f0}.osu"),
        (b"\xfd.osu".as_slice(), "\u{f8f1}.osu"),
        (b"\xfe.osu".as_slice(), "\u{f8f2}.osu"),
        (b"\xff.osu".as_slice(), "\u{f8f3}.osu"),
        (b"\x83\xa0.osu".as_slice(), "Β.osu"),
        (
            b"mix-\x83\x5c\xa0\x83\xa0\xfd\xfe\xff.osu".as_slice(),
            "mix-ソ\u{f8f0}Β\u{f8f1}\u{f8f2}\u{f8f3}.osu",
        ),
    ] {
        assert_eq!(
            unpack_archive(&stored_archive(&[(raw, 0, b"map")])).unwrap()[0].0,
            expected
        );
    }
    assert_eq!(decode_cp932(b"\x80.bin").unwrap(), "\u{0080}.bin");
    for trail in [0xfd, 0xfe, 0xff] {
        let raw = [0x83, trail, b'.', b'o', b's', b'u'];
        assert!(unpack_archive(&stored_archive(&[(&raw, 0, b"map")])).is_err());
    }
    assert!(unpack_archive(&stored_archive(&[(b"\xa0.osu", 1 << 11, b"map")])).is_err());
}

#[test]
fn ordinary_central_comment_can_contain_a_zip64_locator_marker() {
    for payload in [[0_u8; 16], [0xa5_u8; 16]] {
        let mut member = Member::new(b"comment.osu", b"map");
        member.comment.extend(ZIP64_LOCATOR.to_le_bytes());
        member.comment.extend(payload);
        let original = fixture(&[member], &[], false);
        assert_eq!(
            u32_at(&original.bytes, original.end - 20).unwrap(),
            ZIP64_LOCATOR
        );
        assert_eq!(unpack_archive(&original.bytes).unwrap()[0].0, "comment.osu");
    }
}

#[test]
fn one_bounded_central_directory_signature_is_supported() {
    let payload = b"owned opaque signature";
    for zip64 in [false, true] {
        let original = fixture(&[Member::new(b"signed.osu", b"map")], &[], zip64);
        let at = if zip64 {
            original.end - 76
        } else {
            original.end
        };
        let signed = append_signature(original, payload, zip64);
        assert_eq!(unpack_archive(&signed.bytes).unwrap()[0].0, "signed.osu");
        let mut bad = signed.bytes.clone();
        set16(&mut bad, at + 4, (payload.len() + 1) as u16);
        assert!(preflight(&bad).is_err());
        let mut trailing = signed.bytes.clone();
        set16(&mut trailing, at + 4, 0);
        assert!(preflight(&trailing).is_err());
        let duplicate = append_signature(signed, payload, zip64);
        assert!(preflight(&duplicate.bytes).is_err());
    }
}

#[test]
fn actual_beatmap_size_must_equal_its_consistent_local_and_central_declarations() {
    let mut member = Member::new(b"size.osu", b"owned map bytes");
    member.size = Some(1);
    let bytes = fixture(&[member], &[], false).bytes;
    assert_eq!(preflight(&bytes).unwrap()[0].size, 1);
    assert!(
        unpack_archive(&bytes)
            .unwrap_err()
            .2
            .contains("size does not match its metadata")
    );
}

#[test]
fn decode_happens_before_separator_normalization_and_official_path_policy() {
    let bytes = stored_archive(&[(b"\x83\x5c.osu", 0, b"map")]);
    assert_eq!(
        unpack_archive_with_policy(&bytes, true).unwrap()[0].0,
        "ソ.osu"
    );
    let bytes = stored_archive(&[(b"folder\\\x83\x5c.osu", 0, b"map")]);
    assert_eq!(unpack_archive(&bytes).unwrap()[0].0, "ソ.osu");
    assert!(unpack_archive_with_policy(&bytes, true).is_err());
}

#[test]
fn invalid_declared_encoding_is_rejected_without_lossy_or_utf8_guessing() {
    for (raw, flags) in [
        (b"\x83.osu".as_slice(), 0),
        (b"\xff.osu".as_slice(), 1 << 11),
        ("星.osu".as_bytes(), 0),
    ] {
        assert!(unpack_archive(&stored_archive(&[(raw, flags, b"map")])).is_err());
    }
}

#[test]
fn decoded_cp932_aliases_and_normalized_paths_are_duplicates() {
    for (first, second) in [
        (b"\x87\x54.osu".as_slice(), b"\xfa\x4a.osu".as_slice()),
        (b"Maps\\Easy.osu".as_slice(), b"maps/easy.OSU".as_slice()),
        (b"maps/./Easy.osu".as_slice(), b"maps//easy.osu".as_slice()),
        (b"same.osu".as_slice(), b"same.osu".as_slice()),
    ] {
        let bytes = stored_archive(&[(first, 0, b"first"), (second, 0, b"second")]);
        let error = unpack_archive(&bytes).unwrap_err();
        assert!(error.2.contains("duplicate file paths"));
    }
    let mut second = Member::new(b"different.osu", b"second");
    second.extra = unicode_path(second.name, b"FIRST.osu");
    let bytes = fixture(&[Member::new(b"first.osu", b"first"), second], &[], false).bytes;
    assert!(
        unpack_archive(&bytes)
            .unwrap_err()
            .2
            .contains("duplicate file paths")
    );
}

#[test]
fn decompressor_filename_index_cannot_drop_distinct_original_records() {
    // zip's CP437 decoding of the first unflagged name equals the second
    // declared UTF-8 name. CP932 correctly gives the first a different name.
    let bytes = stored_archive(&[
        (b"\xc3\xa9.osu", 0, b"first"),
        ("├⌐.osu".as_bytes(), 1 << 11, b"second"),
    ]);
    assert_eq!(preflight(&bytes).unwrap().len(), 2);
    assert!(
        unpack_archive(&bytes)
            .unwrap_err()
            .2
            .contains("metadata is ambiguous")
    );
}

#[test]
fn every_original_duplicate_record_is_checked_before_library_indexing() {
    for defect in ["symlink", "oversize", "duplicate"] {
        let mut shadowed = Member::new(b"same.osu", b"second");
        match defect {
            "symlink" => shadowed.attributes = 0o120777 << 16,
            "oversize" => shadowed.size = Some((MAX_FILE_BYTES + 1) as u32),
            _ => {}
        }
        let bytes = fixture(&[Member::new(b"same.osu", b"first"), shadowed], &[], false).bytes;
        assert!(unpack_archive(&bytes).is_err(), "{defect}");
    }
    let members = (0..513)
        .map(|_| Member::new(b"same.osu", b"x"))
        .collect::<Vec<_>>();
    let bytes = fixture(&members, &[], false).bytes;
    assert!(
        unpack_archive(&bytes)
            .unwrap_err()
            .2
            .contains("at most 512")
    );
    for name in [
        b"../unsafe.osu".as_slice(),
        b"C:\\unsafe.osu".as_slice(),
        b"/unsafe.osu".as_slice(),
        b"nul\0.osu".as_slice(),
    ] {
        assert!(unpack_archive(&stored_archive(&[(name, 0, b"map")])).is_err());
    }
}

#[test]
fn count_limit_accepts_512_original_unique_records_and_rejects_unaccounted_records() {
    let names = (0..512)
        .map(|index| format!("asset{index}.bin"))
        .collect::<Vec<_>>();
    let members = names
        .iter()
        .map(|name| Member::new(name.as_bytes(), b"asset"))
        .collect::<Vec<_>>();
    let mut original = fixture(&members, &[], false);
    assert_eq!(preflight(&original.bytes).unwrap().len(), 512);
    set16(&mut original.bytes, original.end + 8, 511);
    set16(&mut original.bytes, original.end + 10, 511);
    assert!(preflight(&original.bytes).is_err());
}

#[test]
fn total_declared_size_is_bounded_for_every_raw_record() {
    let names = (0..5)
        .map(|index| format!("asset{index}.bin"))
        .collect::<Vec<_>>();
    let members = names
        .iter()
        .map(|name| {
            let mut member = Member::new(name.as_bytes(), b"x");
            member.size = Some(MAX_FILE_BYTES as u32);
            member
        })
        .collect::<Vec<_>>();
    let bytes = fixture(&members, &[], false).bytes;
    assert!(
        preflight(&bytes)
            .err()
            .unwrap()
            .2
            .contains("uncompressed size limit")
    );
}

#[test]
fn unicode_path_extra_requires_exact_structure_crc_version_and_utf8() {
    let raw = b"original.osu";
    let valid = unicode_path(raw, "星.osu".as_bytes());
    let mut wrong_version = valid.clone();
    wrong_version[4] = 2;
    let mut wrong_crc = valid.clone();
    wrong_crc[5] ^= 1;
    let mut short = valid.clone();
    short.truncate(short.len() - 1);
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut duplicate = valid.clone();
    duplicate.extend(&valid);
    let mut wrong_utf8 = vec![1];
    push32(&mut wrong_utf8, crc32fast::hash(raw));
    wrong_utf8.push(0xff);
    for malformed in [
        wrong_version,
        wrong_crc,
        short,
        trailing,
        duplicate,
        extra(0x7075, &wrong_utf8),
        extra(0x7075, &[1, 0, 0, 0]),
    ] {
        let mut member = Member::new(raw, b"map");
        member.extra = malformed;
        assert!(unpack_archive(&fixture(&[member], &[], false).bytes).is_err());
    }
    let mut member = Member::new(raw, b"map");
    member.extra = valid;
    member.local_extra = Some(unicode_path(raw, "別.osu".as_bytes()));
    assert!(unpack_archive(&fixture(&[member], &[], false).bytes).is_err());
}

#[test]
fn footer_and_record_bounds_counts_local_metadata_and_ranges_are_validated() {
    let original = fixture(&[Member::new(b"Easy.osu", b"owned map")], &[], false);
    for length in 0..original.bytes.len() {
        assert!(
            preflight(&original.bytes[..length]).is_err(),
            "length {length}"
        );
    }
    for (at, value) in [
        (original.end + 8, 2),
        (original.end + 10, 2),
        (original.end + 4, 1),
        (original.central[0] + 34, 1),
        (original.central[0] + 28, u16::MAX),
        (original.central[0] + 30, u16::MAX),
        (original.local[0] + 6, 1 << 11),
        (original.local[0] + 26, 1),
    ] {
        let mut bytes = original.bytes.clone();
        set16(&mut bytes, at, value);
        assert!(preflight(&bytes).is_err(), "field at {at}");
    }
    for at in [
        original.central[0] + 42,
        original.central[0] + 20,
        original.local[0] + 18,
        original.local[0] + 14,
        original.end + 12,
    ] {
        let mut bytes = original.bytes.clone();
        set32(&mut bytes, at, u32::MAX);
        assert!(preflight(&bytes).is_err(), "field at {at}");
    }
    let mut bytes = original.bytes.clone();
    bytes[original.local[0] + 30] = b'X';
    assert!(preflight(&bytes).is_err());
    let mut overlapping = fixture(
        &[Member::new(b"a.osu", b"a"), Member::new(b"b.osu", b"b")],
        &[],
        false,
    );
    set32(
        &mut overlapping.bytes,
        overlapping.central[1] + 42,
        overlapping.local[0] as u32,
    );
    assert!(preflight(&overlapping.bytes).is_err());
}

#[test]
fn valid_single_disk_zip64_prefix_and_data_descriptor_remain_supported() {
    for zip64 in [false, true] {
        for prefix in [b"".as_slice(), b"owned prepended stub".as_slice()] {
            let bytes = fixture(&[Member::new(b"Easy.osu", b"map")], prefix, zip64).bytes;
            assert_eq!(unpack_archive(&bytes).unwrap()[0].0, "Easy.osu");
        }
    }
    let mut member = Member::new(b"descriptor.osu", b"owned descriptor map");
    member.flags = 8;
    member.descriptor = true;
    let original = fixture(&[member], &[], false);
    assert_eq!(
        unpack_archive(&original.bytes).unwrap()[0].0,
        "descriptor.osu"
    );
    let mut bad = original.bytes;
    bad[original.central[0] - 12] ^= 1; // descriptor CRC
    assert!(preflight(&bad).is_err());
    let mut member = Member::new(b"wide-descriptor.osu", b"owned ZIP64 descriptor");
    member.flags = 8;
    member.descriptor = true;
    member.descriptor_wide = true;
    member.central_wide_sizes = true;
    let mut sizes = Vec::new();
    push64(&mut sizes, member.data.len() as u64);
    push64(&mut sizes, member.data.len() as u64);
    member.extra = extra(1, &sizes);
    member.local_extra = Some(Vec::new());
    assert_eq!(
        unpack_archive(&fixture(&[member], &[], true).bytes).unwrap()[0].0,
        "wide-descriptor.osu"
    );
}

#[test]
fn unsigned_descriptors_and_offset_only_or_local_only_zip64_are_supported() {
    let mut narrow = Member::new(b"unsigned.osu", b"map");
    narrow.flags = 8;
    narrow.descriptor = true;
    narrow.descriptor_signed = false;
    assert_eq!(
        unpack_archive(&fixture(&[narrow], &[], false).bytes).unwrap()[0].0,
        "unsigned.osu"
    );
    for signed in [false, true] {
        for zip64_footer in [false, true] {
            let mut offset_only = Member::new(b"offset-only.osu", b"map");
            offset_only.flags = 8;
            offset_only.descriptor = true;
            offset_only.descriptor_wide = true;
            offset_only.descriptor_signed = signed;
            offset_only.extra = extra(1, &0_u64.to_le_bytes());
            offset_only.local_extra = Some(Vec::new());
            let mut archive = fixture(&[offset_only], &[], zip64_footer);
            set32(&mut archive.bytes, archive.central[0] + 42, u32::MAX);
            assert_eq!(
                unpack_archive(&archive.bytes).unwrap()[0].0,
                "offset-only.osu"
            );
        }
        let mut local_only = Member::new(b"local-only.osu", b"map");
        local_only.flags = 8;
        local_only.descriptor = true;
        local_only.descriptor_wide = true;
        local_only.descriptor_signed = signed;
        local_only.local_extra = Some(extra(1, &[0_u8; 16]));
        let mut archive = fixture(&[local_only], &[], false);
        set32(&mut archive.bytes, archive.local[0] + 18, u32::MAX);
        set32(&mut archive.bytes, archive.local[0] + 22, u32::MAX);
        assert_eq!(
            unpack_archive(&archive.bytes).unwrap()[0].0,
            "local-only.osu"
        );
    }
}

#[test]
fn zip64_extended_footer_and_entry_fields_are_validated() {
    let original = fixture(&[Member::new(b"wide.osu", b"map")], &[], true);
    // A legitimate extensible ZIP64 end record is not limited to 56 bytes.
    let zip64_at = original.end - 76;
    let mut extended = original.bytes.clone();
    drop(extended.splice(zip64_at + 56..zip64_at + 56, [1, 2, 3, 4]));
    extended[zip64_at + 4..zip64_at + 12].copy_from_slice(&48_u64.to_le_bytes());
    assert_eq!(unpack_archive(&extended).unwrap()[0].0, "wide.osu");
    for at in [
        zip64_at + 16,
        zip64_at + 20,
        original.end - 16,
        original.end - 4,
    ] {
        let mut bytes = original.bytes.clone();
        set32(&mut bytes, at, 2);
        assert!(preflight(&bytes).is_err());
    }
    for at in [
        zip64_at + 24,
        zip64_at + 32,
        zip64_at + 40,
        original.end - 12,
    ] {
        let mut bytes = original.bytes.clone();
        bytes[at] ^= 1;
        assert!(preflight(&bytes).is_err());
    }
    let mut field = Vec::new();
    push64(&mut field, 3);
    push64(&mut field, 3);
    push64(&mut field, 0);
    let mut member = Member::new(b"wide.osu", b"map");
    member.extra = extra(1, &field);
    member.local_extra = Some(extra(1, &field[..16]));
    let mut wide = fixture(&[member], &[], true);
    for at in [
        wide.central[0] + 20,
        wide.central[0] + 24,
        wide.central[0] + 42,
        wide.local[0] + 18,
        wide.local[0] + 22,
    ] {
        set32(&mut wide.bytes, at, u32::MAX);
    }
    set16(&mut wide.bytes, wide.local[0] + 4, 45);
    set16(&mut wide.bytes, wide.central[0] + 6, 45);
    assert_eq!(unpack_archive(&wide.bytes).unwrap()[0].0, "wide.osu");
    let mut duplicate = Member::new(b"wide.osu", b"map");
    duplicate.extra = extra(1, &[]);
    duplicate.extra.extend(extra(1, &[]));
    assert!(preflight(&fixture(&[duplicate], &[], false).bytes).is_err());
}

#[test]
fn valid_zip64_footer_is_supported_when_ordinary_end_values_fit() {
    for prefix in [b"".as_slice(), b"owned prepended stub".as_slice()] {
        for signed in [false, true] {
            let original = fixture(&[Member::new(b"optional-zip64.osu", b"map")], prefix, true);
            let mut archive = if signed {
                append_signature(original, b"owned signature", true)
            } else {
                original
            };
            let zip64_at = archive.end - 76;
            let count = u64_at(&archive.bytes, zip64_at + 32).unwrap() as u16;
            let size = u64_at(&archive.bytes, zip64_at + 40).unwrap() as u32;
            let start = u64_at(&archive.bytes, zip64_at + 48).unwrap() as u32;
            set16(&mut archive.bytes, archive.end + 8, count);
            set16(&mut archive.bytes, archive.end + 10, count);
            set32(&mut archive.bytes, archive.end + 12, size);
            set32(&mut archive.bytes, archive.end + 16, start);
            assert_eq!(
                unpack_archive(&archive.bytes).unwrap(),
                [("optional-zip64.osu".to_owned(), b"map".to_vec())]
            );
            set32(&mut archive.bytes, archive.end + 12, size - 1);
            assert!(unpack_archive(&archive.bytes).is_err());
        }
    }
}
