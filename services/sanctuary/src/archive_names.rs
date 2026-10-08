//! Bounded validation of original ZIP metadata before the decompressor indexes it.
//! Filename decoding follows the pinned lazer reader: UTF-8 when bit 11 is set,
//! otherwise Windows CP932, with a validated Info-ZIP Unicode Path override.
use crate::{ApiError, ApiResult};
use encoding_rs::SHIFT_JIS;
use std::collections::HashSet;

const MAX_ENTRIES: u64 = 512;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
const END: u32 = 0x0605_4b50;
const ZIP64_END: u32 = 0x0606_4b50;
const ZIP64_LOCATOR: u32 = 0x0706_4b50;

pub(super) struct ArchiveEntry {
    pub name: String,
    pub central_start: u64,
    pub header_start: u64,
    pub data_start: u64,
    pub compressed_size: u64,
    pub size: u64,
    pub crc32: u32,
}

fn invalid() -> ApiError {
    ApiError::bad("Invalid or ambiguous .osz archive metadata.")
}

fn slice(bytes: &[u8], start: usize, length: usize) -> ApiResult<&[u8]> {
    bytes
        .get(start..start.checked_add(length).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}

fn u16_at(bytes: &[u8], at: usize) -> ApiResult<u16> {
    Ok(u16::from_le_bytes(slice(bytes, at, 2)?.try_into().unwrap()))
}

fn u32_at(bytes: &[u8], at: usize) -> ApiResult<u32> {
    Ok(u32::from_le_bytes(slice(bytes, at, 4)?.try_into().unwrap()))
}

fn u64_at(bytes: &[u8], at: usize) -> ApiResult<u64> {
    Ok(u64::from_le_bytes(slice(bytes, at, 8)?.try_into().unwrap()))
}

fn offset(value: u64) -> ApiResult<usize> {
    usize::try_from(value).map_err(|_| invalid())
}

#[derive(Default)]
struct Extras<'a> {
    zip64: Option<&'a [u8]>,
    unicode_name: Option<&'a str>,
}

fn extras<'a>(bytes: &'a [u8], raw_name: &[u8]) -> ApiResult<Extras<'a>> {
    let mut result = Extras::default();
    let mut at = 0;
    while at < bytes.len() {
        let tag = u16_at(bytes, at)?;
        let length = usize::from(u16_at(bytes, at + 2)?);
        at += 4;
        let field = slice(bytes, at, length)?;
        match tag {
            0x0001 => {
                if result.zip64.replace(field).is_some() {
                    return Err(invalid());
                }
            }
            0x7075 => {
                if result.unicode_name.is_some()
                    || field.len() < 5
                    || field[0] != 1
                    || u32_at(field, 1)? != crc32fast::hash(raw_name)
                {
                    return Err(invalid());
                }
                result.unicode_name =
                    Some(std::str::from_utf8(&field[5..]).map_err(|_| invalid())?);
            }
            _ => {}
        }
        at += length;
    }
    Ok(result)
}

fn decode_name(raw: &[u8], flags: u16, extra: &Extras<'_>) -> ApiResult<String> {
    if let Some(name) = extra.unicode_name {
        return Ok(name.to_owned());
    }
    if flags & (1 << 11) != 0 {
        Ok(std::str::from_utf8(raw).map_err(|_| invalid())?.to_owned())
    } else {
        decode_cp932(raw)
    }
}

fn decode_cp932(raw: &[u8]) -> ApiResult<String> {
    // Windows CP932 has four standalone private-use mappings omitted by
    // WHATWG Shift_JIS. Keep multibyte sequences intact for strict decoding:
    // A0 can be a valid trail, while FD/FE/FF are never valid trails.
    let mut result = String::new();
    let mut start = 0;
    let mut at = 0;
    while at < raw.len() {
        let byte = raw[at];
        if (0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte) {
            raw.get(at + 1).ok_or_else(invalid)?;
            at += 2;
            continue;
        }
        let standalone = match byte {
            0xa0 => Some('\u{f8f0}'),
            0xfd => Some('\u{f8f1}'),
            0xfe => Some('\u{f8f2}'),
            0xff => Some('\u{f8f3}'),
            _ => None,
        };
        if let Some(character) = standalone {
            result.push_str(
                &SHIFT_JIS
                    .decode_without_bom_handling_and_without_replacement(&raw[start..at])
                    .ok_or_else(invalid)?,
            );
            result.push(character);
            start = at + 1;
        }
        at += 1;
    }
    result.push_str(
        &SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(&raw[start..])
            .ok_or_else(invalid)?,
    );
    Ok(result)
}

fn normalized_path(name: &str, external_attributes: u32) -> ApiResult<(String, String)> {
    // Decode before replacing separators: 0x5c is also a CP932 trail byte.
    let name = name.replace('\\', "/");
    if name.starts_with('/')
        || name.contains('\0')
        || name
            .split('/')
            .any(|part| part == ".." || part.contains(':'))
        || (external_attributes >> 16) & 0o170000 == 0o120000
    {
        return Err(ApiError::bad(
            "Archive contains an unsafe path or symbolic link.",
        ));
    }
    let normalized = name
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
        .to_lowercase();
    if normalized.is_empty() {
        return Err(invalid());
    }
    Ok((name, normalized))
}

struct Directory {
    start: usize,
    end: usize,
    count: usize,
    archive_offset: u64,
}

fn complete_directory_tail(bytes: &[u8], at: usize, end: usize) -> bool {
    at == end
        || (at
            .checked_add(6)
            .is_some_and(|header_end| header_end <= end)
            && u32_at(bytes, at).ok() == Some(0x0505_4b50)
            && u16_at(bytes, at + 4)
                .ok()
                .is_some_and(|length| at.checked_add(6 + usize::from(length)) == Some(end)))
}

fn complete_directory_layout(bytes: &[u8], directory: &Directory) -> bool {
    let mut at = directory.start;
    if directory.count > MAX_ENTRIES as usize {
        return false;
    }
    for _ in 0..directory.count {
        let Ok(header) = slice(bytes, at, 46) else {
            return false;
        };
        if u32_at(header, 0).ok() != Some(CENTRAL) {
            return false;
        }
        // All reads below are within the already bounded fixed header.
        let variable_length = usize::from(u16_at(header, 28).unwrap())
            + usize::from(u16_at(header, 30).unwrap())
            + usize::from(u16_at(header, 32).unwrap());
        let Some(next) = at.checked_add(46 + variable_length) else {
            return false;
        };
        if next > directory.end {
            return false;
        }
        at = next;
    }
    complete_directory_tail(bytes, at, directory.end)
}

fn directory(bytes: &[u8]) -> ApiResult<Directory> {
    if bytes.len() < 22 {
        return Err(invalid());
    }
    // The comment length must account for every trailing byte. Reject competing
    // complete footers instead of accepting whichever a reader happens to find.
    let mut ends = (bytes.len().saturating_sub(22 + usize::from(u16::MAX))..=bytes.len() - 22)
        .filter(|at| {
            u32_at(bytes, *at).ok() == Some(END)
                && u16_at(bytes, *at + 20)
                    .ok()
                    .is_some_and(|length| *at + 22 + usize::from(length) == bytes.len())
        });
    let end = ends.next().ok_or_else(invalid)?;
    if ends.next().is_some() || u16_at(bytes, end + 4)? != 0 || u16_at(bytes, end + 6)? != 0 {
        return Err(invalid());
    }
    let count_disk = u16_at(bytes, end + 8)?;
    let count = u16_at(bytes, end + 10)?;
    let size = u32_at(bytes, end + 12)?;
    let relative_start = u32_at(bytes, end + 16)?;
    let sentinel = count_disk == u16::MAX
        || count == u16::MAX
        || size == u32::MAX
        || relative_start == u32::MAX;
    // Prefer a complete ordinary directory: a comment can incidentally contain
    // the locator marker. ZIP64 footers are also legal when all narrow values
    // fit, so otherwise recognize their full structure and consistent bounds.
    if !sentinel && count_disk == count {
        let archive_offset = u64::try_from(end)
            .map_err(|_| invalid())?
            .checked_sub(u64::from(size))
            .and_then(|at| at.checked_sub(u64::from(relative_start)));
        if let Some(archive_offset) = archive_offset {
            let start = offset(
                u64::from(relative_start)
                    .checked_add(archive_offset)
                    .ok_or_else(invalid)?,
            )?;
            let ordinary = Directory {
                start,
                end,
                count: usize::from(count),
                archive_offset,
            };
            if complete_directory_layout(bytes, &ordinary) {
                return Ok(ordinary);
            }
        }
    }
    let has_zip64 = end >= 20 && u32_at(bytes, end - 20)? == ZIP64_LOCATOR;
    let (count, size, relative_start, directory_end, archive_offset) = if has_zip64 {
        let locator = end - 20;
        if u32_at(bytes, locator + 4)? != 0 || u32_at(bytes, locator + 16)? != 1 {
            return Err(invalid());
        }
        // ZIP64 permits an extensible record and a prepended stub. Locate the
        // record by its exact ending, then validate its declared relative offset.
        let mut candidates = bytes[..locator]
            .windows(4)
            .enumerate()
            .filter_map(|(at, word)| {
                (word == ZIP64_END.to_le_bytes()
                    && u64_at(bytes, at + 4).ok().is_some_and(|length| {
                        length >= 44
                            && length
                                .checked_add(12)
                                .and_then(|length| u64::try_from(at).ok()?.checked_add(length))
                                == u64::try_from(locator).ok()
                    }))
                .then_some(at)
            });
        let zip64 = candidates.next().ok_or_else(invalid)?;
        if candidates.next().is_some()
            || u32_at(bytes, zip64 + 16)? != 0
            || u32_at(bytes, zip64 + 20)? != 0
        {
            return Err(invalid());
        }
        let wide_count = u64_at(bytes, zip64 + 32)?;
        let wide_size = u64_at(bytes, zip64 + 40)?;
        let wide_start = u64_at(bytes, zip64 + 48)?;
        if u64_at(bytes, zip64 + 24)? != wide_count
            || (count_disk != u16::MAX && u64::from(count_disk) != wide_count)
            || (count != u16::MAX && u64::from(count) != wide_count)
            || (size != u32::MAX && u64::from(size) != wide_size)
            || (relative_start != u32::MAX && u64::from(relative_start) != wide_start)
        {
            return Err(invalid());
        }
        let archive_offset = u64::try_from(zip64)
            .map_err(|_| invalid())?
            .checked_sub(u64_at(bytes, locator + 8)?)
            .ok_or_else(invalid)?;
        (wide_count, wide_size, wide_start, zip64, archive_offset)
    } else {
        if sentinel || count_disk != count {
            return Err(invalid());
        }
        let archive_offset = u64::try_from(end)
            .map_err(|_| invalid())?
            .checked_sub(u64::from(size))
            .and_then(|at| at.checked_sub(u64::from(relative_start)))
            .ok_or_else(invalid)?;
        (
            u64::from(count),
            u64::from(size),
            u64::from(relative_start),
            end,
            archive_offset,
        )
    };
    if count > MAX_ENTRIES {
        return Err(ApiError::bad("Archive may contain at most 512 entries."));
    }
    let start = offset(
        relative_start
            .checked_add(archive_offset)
            .ok_or_else(invalid)?,
    )?;
    if start.checked_add(offset(size)?) != Some(directory_end) {
        return Err(invalid());
    }
    Ok(Directory {
        start,
        end: directory_end,
        count: offset(count)?,
        archive_offset,
    })
}

fn wide_fields(
    extra: Option<&[u8]>,
    size: u32,
    compressed: u32,
    header: Option<u32>,
    disk: Option<u16>,
) -> ApiResult<(u64, u64, Option<u64>)> {
    let needed = usize::from(size == u32::MAX) * 8
        + usize::from(compressed == u32::MAX) * 8
        + usize::from(header == Some(u32::MAX)) * 8
        + usize::from(disk == Some(u16::MAX)) * 4;
    let bytes = extra.unwrap_or_default();
    if bytes.len() != needed || (needed != 0 && extra.is_none()) || (needed == 0 && extra.is_some())
    {
        return Err(invalid());
    }
    let mut at = 0;
    let size = if size == u32::MAX {
        let value = u64_at(bytes, at)?;
        at += 8;
        value
    } else {
        u64::from(size)
    };
    let compressed = if compressed == u32::MAX {
        let value = u64_at(bytes, at)?;
        at += 8;
        value
    } else {
        u64::from(compressed)
    };
    let header = match header {
        Some(u32::MAX) => {
            let value = u64_at(bytes, at)?;
            at += 8;
            Some(value)
        }
        Some(value) => Some(u64::from(value)),
        None => None,
    };
    if disk == Some(u16::MAX) && u32_at(bytes, at)? != 0
        || disk.is_some_and(|disk| disk != 0 && disk != u16::MAX)
    {
        return Err(invalid());
    }
    Ok((size, compressed, header))
}

fn descriptor_end(bytes: &[u8], at: usize, entry: &ArchiveEntry, wide: bool) -> ApiResult<usize> {
    let candidate = |at| -> ApiResult<usize> {
        if u32_at(bytes, at)? != entry.crc32 {
            return Err(invalid());
        }
        let (compressed, size, length) = if wide {
            (u64_at(bytes, at + 4)?, u64_at(bytes, at + 12)?, 20)
        } else {
            (
                u64::from(u32_at(bytes, at + 4)?),
                u64::from(u32_at(bytes, at + 8)?),
                12,
            )
        };
        if compressed != entry.compressed_size || size != entry.size {
            return Err(invalid());
        }
        at.checked_add(length).ok_or_else(invalid)
    };
    // The optional marker can itself be a real CRC. Accept one unambiguous
    // interpretation rather than guessing from that word alone.
    let unsigned = candidate(at).ok();
    let signed = if u32_at(bytes, at)? == 0x0807_4b50 {
        candidate(at + 4).ok()
    } else {
        None
    };
    match (unsigned, signed) {
        (Some(end), None) | (None, Some(end)) => Ok(end),
        _ => Err(invalid()),
    }
}

pub(super) fn preflight(bytes: &[u8]) -> ApiResult<Vec<ArchiveEntry>> {
    let directory = directory(bytes)?;
    let mut entries = Vec::with_capacity(directory.count);
    let mut paths = HashSet::new();
    let mut ranges = Vec::with_capacity(directory.count);
    let mut total = 0_u64;
    let mut at = directory.start;
    for _ in 0..directory.count {
        let header = slice(bytes, at, 46)?;
        if u32_at(header, 0)? != CENTRAL {
            return Err(invalid());
        }
        let flags = u16_at(header, 8)?;
        if flags & 0x2041 != 0 {
            return Err(ApiError::bad(
                "Encrypted archive entries are not supported.",
            ));
        }
        let raw = slice(bytes, at + 46, usize::from(u16_at(header, 28)?))?;
        let extra_start = at + 46 + raw.len();
        let extra_bytes = slice(bytes, extra_start, usize::from(u16_at(header, 30)?))?;
        let extra = extras(extra_bytes, raw)?;
        let next = extra_start + extra_bytes.len() + usize::from(u16_at(header, 32)?);
        if next > directory.end {
            return Err(invalid());
        }
        let (size, compressed_size, relative_header) = wide_fields(
            extra.zip64,
            u32_at(header, 24)?,
            u32_at(header, 20)?,
            Some(u32_at(header, 42)?),
            Some(u16_at(header, 34)?),
        )?;
        total = total.checked_add(size).ok_or_else(invalid)?;
        if size > MAX_FILE_BYTES || total > MAX_TOTAL_BYTES {
            return Err(ApiError::bad(
                "Archive exceeds its uncompressed size limit.",
            ));
        }
        let (name, normalized) =
            normalized_path(&decode_name(raw, flags, &extra)?, u32_at(header, 38)?)?;
        if !paths.insert(normalized) {
            return Err(ApiError::bad("Archive contains duplicate file paths."));
        }
        let header_start = relative_header
            .unwrap()
            .checked_add(directory.archive_offset)
            .ok_or_else(invalid)?;
        let local_start = offset(header_start)?;
        if local_start < offset(directory.archive_offset)? || local_start >= directory.start {
            return Err(invalid());
        }
        let local = slice(bytes, local_start, 30)?;
        if u32_at(local, 0)? != LOCAL
            || u16_at(local, 6)? != flags
            || u16_at(local, 8)? != u16_at(header, 10)?
        {
            return Err(invalid());
        }
        let local_raw = slice(bytes, local_start + 30, usize::from(u16_at(local, 26)?))?;
        if local_raw != raw {
            return Err(invalid());
        }
        let local_extra_start = local_start + 30 + local_raw.len();
        let local_extra_bytes = slice(bytes, local_extra_start, usize::from(u16_at(local, 28)?))?;
        let local_extra = extras(local_extra_bytes, local_raw)?;
        if local_extra.unicode_name.is_some()
            && decode_name(local_raw, flags, &local_extra)? != decode_name(raw, flags, &extra)?
        {
            return Err(invalid());
        }
        let (local_size, local_compressed, _) = wide_fields(
            local_extra.zip64,
            u32_at(local, 22)?,
            u32_at(local, 18)?,
            None,
            None,
        )?;
        let crc32 = u32_at(header, 16)?;
        if flags & 8 == 0 {
            if u32_at(local, 14)? != crc32
                || local_size != size
                || local_compressed != compressed_size
            {
                return Err(invalid());
            }
        } else if (u32_at(local, 14)? != 0 && u32_at(local, 14)? != crc32)
            || (local_size != 0 && local_size != size)
            || (local_compressed != 0 && local_compressed != compressed_size)
        {
            return Err(invalid());
        }
        let data_start =
            u64::try_from(local_extra_start + local_extra_bytes.len()).map_err(|_| invalid())?;
        let entry = ArchiveEntry {
            name,
            central_start: u64::try_from(at).map_err(|_| invalid())?,
            header_start,
            data_start,
            compressed_size,
            size,
            crc32,
        };
        let data_end = offset(
            data_start
                .checked_add(compressed_size)
                .ok_or_else(invalid)?,
        )?;
        if data_end > directory.start {
            return Err(invalid());
        }
        let end = if flags & 8 != 0 {
            // APPNOTE 4.3.9.2 uses wide descriptor sizes when a member has a
            // ZIP64 extra, including a central extra containing only its offset.
            descriptor_end(
                bytes,
                data_end,
                &entry,
                local_extra.zip64.is_some() || extra.zip64.is_some(),
            )?
        } else {
            data_end
        };
        if end > directory.start {
            return Err(invalid());
        }
        ranges.push((local_start, end));
        entries.push(entry);
        at = next;
    }
    // APPNOTE 4.3.12 permits one opaque central-directory signature. Its
    // cryptographic meaning is outside unpacking, but its bounds are exact.
    if !complete_directory_tail(bytes, at, directory.end) {
        return Err(invalid());
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(invalid());
    }
    Ok(entries)
}

#[cfg(test)]
#[path = "archive_name_tests.rs"]
pub(crate) mod tests;
