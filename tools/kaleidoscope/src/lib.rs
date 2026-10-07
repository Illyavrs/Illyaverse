//! Original, dependency-free connection tooling. No client or reference-server code is vendored.

pub mod launcher;

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::IpAddr;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const BEGIN: &str = "# BEGIN ILLYAVERSE KALEIDOSCOPE";
pub const END: &str = "# END ILLYAVERSE KALEIDOSCOPE";
const ADDED_SEPARATOR: &str = "# kaleidoscope: preceding newline added";
const MAX_HOSTS_BYTES: usize = 1024 * 1024;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug)]
pub struct Mapping {
    ip: IpAddr,
    domains: Vec<String>,
}

impl Mapping {
    pub fn new(ip: &str, domains: &[String]) -> Result<Self> {
        let ip: IpAddr = ip
            .parse()
            .map_err(|_| "--ip must be a literal IPv4 or IPv6 address")?;
        if ip.is_unspecified() || ip.is_multicast() || ip == IpAddr::from([255, 255, 255, 255]) {
            return Err(
                "An unspecified, multicast, or broadcast IP cannot be a server address".into(),
            );
        }
        if domains.is_empty() {
            return Err("At least one explicit --domain is required".into());
        }
        if domains.len() > 64 {
            return Err(
                "A Kaleidoscope mapping supports at most 64 explicitly named domains".into(),
            );
        }
        let mut seen = HashSet::new();
        let mut normalized = Vec::new();
        for domain in domains {
            let domain = validate_domain(domain)?;
            if seen.insert(domain.clone()) {
                normalized.push(domain);
            }
        }
        Ok(Self {
            ip,
            domains: normalized,
        })
    }
}

pub fn validate_domain(input: &str) -> Result<String> {
    let domain = input.to_ascii_lowercase();
    if domain.len() > 253 || !domain.is_ascii() || domain.parse::<IpAddr>().is_ok() {
        return Err("Use a DNS hostname, without an IP, scheme, port, or path".into());
    }
    let labels: Vec<_> = domain.split('.').collect();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
        })
    {
        return Err(format!(
            "Invalid DNS hostname: {input}. Use ASCII/punycode labels, without wildcards"
        ));
    }
    if domain == "ppy.sh" || domain.ends_with(".ppy.sh") {
        return Err(
            "Official ppy.sh domains cannot be redirected. Use an operator-owned server domain"
                .into(),
        );
    }
    Ok(domain)
}

/// Locate exactly one well-formed owned block. Malformed or duplicate markers fail closed.
pub fn managed_range(text: &str) -> Result<Option<Range<usize>>> {
    let mut start = None;
    let mut found = None;
    let mut offset = 0;
    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']).trim();
        let line = if offset == 0 {
            line.trim_start_matches('\u{feff}')
        } else {
            line
        };
        let bom = if offset == 0 && raw_line.starts_with('\u{feff}') {
            3
        } else {
            0
        };
        if line == BEGIN {
            if start.is_some() || found.is_some() {
                return Err(
                    "Duplicate or nested Kaleidoscope block. Repair markers manually before continuing"
                        .into(),
                );
            }
            start = Some(offset + bom);
        } else if line == END {
            let mut block_start = start
                .take()
                .ok_or("Kaleidoscope END marker has no BEGIN marker")?;
            let end = offset + raw_line.len();
            if text[block_start..end]
                .lines()
                .any(|line| line == ADDED_SEPARATOR)
            {
                if text[..block_start].ends_with("\r\n") {
                    block_start -= 2;
                } else if text[..block_start].ends_with('\n') {
                    block_start -= 1;
                } else {
                    return Err("Kaleidoscope block has inconsistent newline metadata".into());
                }
            }
            found = Some(block_start..end);
        }
        offset += raw_line.len();
    }
    if start.is_some() {
        return Err("Kaleidoscope BEGIN marker has no END marker".into());
    }
    Ok(found)
}

pub fn plan_apply(text: &str, mapping: &Mapping) -> Result<String> {
    let range = managed_range(text)?;
    let (prefix, suffix) = match range {
        Some(ref range) => (&text[..range.start], &text[range.end..]),
        None => (text, ""),
    };
    // Hosts resolution precedence is platform dependent. Never silently compete with an existing entry.
    for line in prefix.lines().chain(suffix.lines()) {
        let uncommented = line.split('#').next().unwrap_or("");
        for hostname in uncommented.split_whitespace().skip(1) {
            if mapping
                .domains
                .iter()
                .any(|domain| domain.eq_ignore_ascii_case(hostname))
            {
                return Err(format!(
                    "{hostname} already exists outside the Kaleidoscope block. Resolve that entry first"
                ));
            }
        }
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let add_separator = !prefix.is_empty() && prefix != "\u{feff}" && !prefix.ends_with('\n');
    let mut result = String::with_capacity(text.len() + mapping.domains.len() * 80);
    result.push_str(prefix);
    if add_separator {
        result.push_str(newline);
    }
    result.push_str(BEGIN);
    result.push_str(newline);
    if add_separator {
        result.push_str(ADDED_SEPARATOR);
        result.push_str(newline);
    }
    result.push_str(
        "# Managed by Kaleidoscope. TLS and client compatibility must be configured separately.",
    );
    result.push_str(newline);
    for domain in &mapping.domains {
        result.push_str(&format!("{}\t{}{}", mapping.ip, domain, newline));
    }
    result.push_str(END);
    result.push_str(newline);
    result.push_str(suffix);
    if result.len() > MAX_HOSTS_BYTES {
        return Err("Proposed hosts contents exceed the 1 MiB safety limit".into());
    }
    Ok(result)
}

/// Remove only the owned block; keep operator edits made elsewhere after apply.
pub fn plan_restore(text: &str) -> Result<String> {
    match managed_range(text)? {
        Some(range) => {
            let prefix = &text[..range.start];
            let suffix = &text[range.end..];
            let needs_separator = !prefix.is_empty()
                && prefix != "\u{feff}"
                && !prefix.ends_with('\n')
                && !suffix.is_empty()
                && !suffix.starts_with('\n')
                && !suffix.starts_with("\r\n");
            let separator = if needs_separator {
                if text[range.start..].starts_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                }
            } else {
                ""
            };
            Ok(format!("{prefix}{separator}{suffix}"))
        }
        None => Ok(text.to_owned()),
    }
}

pub fn default_hosts_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/hosts")
    }
}

fn regular_file(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "Refusing a symlink or non-regular hosts file: {}",
            path.display()
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Refusing a Windows reparse point as the hosts file".into());
        }
    }
    Ok(metadata)
}

pub fn read_hosts(path: &Path) -> Result<String> {
    regular_file(path)?;
    let file =
        File::open(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_HOSTS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    if bytes.len() > MAX_HOSTS_BYTES {
        return Err("Hosts file exceeds the 1 MiB safety limit".into());
    }
    String::from_utf8(bytes)
        .map_err(|_| "Hosts must be UTF-8/ASCII; original bytes have not been changed".into())
}

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let filename = path.file_name().ok_or("Hosts path has no filename")?;
    let mut filename = filename.to_os_string();
    filename.push(suffix);
    Ok(path.with_file_name(filename))
}

fn fresh_file(path: &Path) -> Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|error| format!("Cannot create {}: {error}. Windows system hosts writes require an elevated terminal", path.display()))
}

/// Persist a reviewed plan, with an optimistic concurrency check and a byte-for-byte backup.
/// The adjacent lock coordinates Kaleidoscope processes. Other editors must be closed while writing.
pub fn write_plan(path: &Path, original: &str, replacement: &str) -> Result<Option<PathBuf>> {
    if original.len() > MAX_HOSTS_BYTES || replacement.len() > MAX_HOSTS_BYTES {
        return Err("Hosts contents exceed the 1 MiB safety limit".into());
    }
    if original == replacement {
        return Ok(None);
    }
    let metadata = regular_file(path)?;
    let lock_path = sibling(path, ".kaleidoscope.lock")?;
    let mut lock = fresh_file(&lock_path)?;
    let _lock_cleanup = Cleanup(lock_path);
    writeln!(lock, "Kaleidoscope PID {}", std::process::id()).map_err(|error| error.to_string())?;
    lock.sync_all().map_err(|error| error.to_string())?;
    if read_hosts(path)? != original {
        return Err("Hosts changed since preview; generate a new plan before writing".into());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let backup_path = sibling(
        path,
        &format!(".kaleidoscope-{nonce}-{}.bak", std::process::id()),
    )?;
    let mut backup = fresh_file(&backup_path)?;
    backup
        .write_all(original.as_bytes())
        .map_err(|error| format!("Backup failed: {error}"))?;
    backup
        .sync_all()
        .map_err(|error| format!("Backup sync failed: {error}"))?;
    fs::set_permissions(&backup_path, metadata.permissions()).map_err(|error| error.to_string())?;
    drop(backup);

    let stage_path = sibling(
        path,
        &format!(".kaleidoscope-{nonce}-{}.tmp", std::process::id()),
    )?;
    let mut stage = fresh_file(&stage_path)?;
    stage
        .write_all(replacement.as_bytes())
        .map_err(|error| format!("Staging failed: {error}"))?;
    stage
        .sync_all()
        .map_err(|error| format!("Staging sync failed: {error}"))?;
    fs::set_permissions(&stage_path, metadata.permissions()).map_err(|error| error.to_string())?;
    drop(stage);
    if read_hosts(path)? != original {
        let _ = fs::remove_file(&stage_path);
        return Err(format!(
            "Hosts changed while preparing; nothing replaced. Backup: {}",
            backup_path.display()
        ));
    }
    // On failure leave the stage and backup in place for explicit recovery.
    replace_file(path, &stage_path).map_err(|error| format!(
        "Replacement failed: {error}. Inspect the hosts file before retrying. Backup: {}; staged plan: {}",
        backup_path.display(), stage_path.display()
    ))?;
    if read_hosts(path)? != replacement {
        return Err(format!(
            "Post-write verification failed. Backup: {}",
            backup_path.display()
        ));
    }
    Ok(Some(backup_path))
}

#[cfg(not(windows))]
fn replace_file(path: &Path, stage: &Path) -> io::Result<()> {
    fs::rename(stage, path)
}

#[cfg(windows)]
fn replace_file(path: &Path, stage: &Path) -> io::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let stage: Vec<u16> = stage.as_os_str().encode_wide().chain(Some(0)).collect();
    // Windows merges the original ACLs and attributes; errors are never ignored.
    let success = unsafe {
        ReplaceFileW(
            path.as_ptr(),
            stage.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn stable_domains(domain: &str) -> Result<Vec<String>> {
    let domain = validate_domain(domain)?;
    let mut domains = vec![domain.clone()];
    for prefix in ["osu", "c", "ce", "c4", "a", "b", "api"] {
        domains.push(validate_domain(&format!("{prefix}.{domain}"))?);
    }
    Ok(domains)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping() -> Mapping {
        Mapping::new(
            "127.0.0.1",
            &["osu.illya.test".into(), "c.illya.test".into()],
        )
        .unwrap()
    }
    fn scratch() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("kaleidoscope-test-{}-{nonce}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        directory
    }

    fn cleanup(directory: PathBuf) {
        let directory = fs::canonicalize(directory).unwrap();
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(directory.parent(), Some(temp.as_path()));
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kaleidoscope-test-"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_injection_and_official_redirects() {
        for domain in [
            "https://illya.test",
            "illya.test:443",
            "*.illya.test",
            "evil.test\n127.0.0.1 ppy.sh",
            "illya.test/",
            "-bad.test",
            "bad-.test",
            "a..test",
            "localhost",
            "127.0.0.1",
            "osu.ppy.sh",
            "PPY.SH",
            "日.test",
        ] {
            assert!(validate_domain(domain).is_err(), "{domain}");
        }
        assert_eq!(validate_domain("OSU.Illya.Test").unwrap(), "osu.illya.test");
        assert_eq!(
            validate_domain("xn--wgv71a.illya.test").unwrap(),
            "xn--wgv71a.illya.test"
        );
    }

    #[test]
    fn accepts_ipv6_and_deduplicates() {
        let mapping =
            Mapping::new("::1", &["OSU.Illya.Test".into(), "osu.illya.test".into()]).unwrap();
        assert_eq!(mapping.domains.len(), 1);
        assert!(plan_apply("", &mapping)
            .unwrap()
            .contains("::1\tosu.illya.test"));
        for ip in [
            "0.0.0.0",
            "::",
            "224.0.0.1",
            "ff02::1",
            "255.255.255.255",
            "not-an-ip",
        ] {
            assert!(Mapping::new(ip, &["illya.test".into()]).is_err());
        }
    }

    #[test]
    fn preserves_every_unmanaged_byte_on_round_trip() {
        for original in [
            "",
            "\u{feff}",
            "127.0.0.1 localhost",
            "127.0.0.1 localhost\n",
            "\u{feff}# 系統 hosts\r\n127.0.0.1\tlocalhost\r\n",
            "# only a comment\r\n# last",
        ] {
            let installed = plan_apply(original, &mapping()).unwrap();
            assert_eq!(plan_apply(&installed, &mapping()).unwrap(), installed);
            assert_eq!(plan_restore(&installed).unwrap(), original);
        }
    }

    #[test]
    fn restore_preserves_new_operator_entries() {
        let original = "127.0.0.1 localhost\r\n";
        let installed = format!(
            "{}10.0.0.1 unrelated.test\r\n",
            plan_apply(original, &mapping()).unwrap()
        );
        assert_eq!(
            plan_restore(&installed).unwrap(),
            format!("{original}10.0.0.1 unrelated.test\r\n")
        );
        let changed = Mapping::new("::1", &["osu.illya.test".into()]).unwrap();
        assert!(plan_apply(&installed, &changed)
            .unwrap()
            .ends_with("10.0.0.1 unrelated.test\r\n"));
        let original = "127.0.0.1 localhost";
        let installed = format!(
            "{}10.0.0.1 unrelated.test\n",
            plan_apply(original, &mapping()).unwrap()
        );
        assert_eq!(
            plan_restore(&installed).unwrap(),
            format!("{original}\n10.0.0.1 unrelated.test\n")
        );
    }

    #[test]
    fn fails_closed_on_conflicts_and_malformed_blocks() {
        assert!(plan_apply("1.2.3.4 OTHER.test OSU.ILLYA.TEST # existing\n", &mapping()).is_err());
        assert!(plan_apply("# 1.2.3.4 osu.illya.test\n", &mapping()).is_ok());
        for text in [
            format!("{BEGIN}\n"),
            format!("{END}\n"),
            format!("{BEGIN}\n{BEGIN}\n{END}\n"),
            format!("{BEGIN}\n{END}\n{BEGIN}\n{END}\n"),
            format!("{BEGIN}\n{ADDED_SEPARATOR}\n{END}\n"),
        ] {
            assert!(plan_apply(&text, &mapping()).is_err());
            assert!(plan_restore(&text).is_err());
        }
    }

    #[test]
    fn writes_backup_and_restore_without_system_hosts() {
        let directory = scratch();
        let path = directory.join("hosts.fixture");
        let original = "\u{feff}127.0.0.1 localhost\r\n";
        fs::write(&path, original).unwrap();
        let plan = plan_apply(original, &mapping()).unwrap();
        let backup = write_plan(&path, original, &plan).unwrap().unwrap();
        assert_eq!(fs::read(&backup).unwrap(), original.as_bytes());
        assert_eq!(read_hosts(&path).unwrap(), plan);
        let with_other_edit = format!("{plan}10.1.2.3 another.test\r\n");
        fs::write(&path, &with_other_edit).unwrap();
        let restored = plan_restore(&with_other_edit).unwrap();
        write_plan(&path, &with_other_edit, &restored).unwrap();
        assert_eq!(
            read_hosts(&path).unwrap(),
            format!("{original}10.1.2.3 another.test\r\n")
        );
        assert!(write_plan(&path, &restored, &restored).unwrap().is_none());
        cleanup(directory);
    }

    #[test]
    fn rejects_changed_input_and_existing_lock() {
        let directory = scratch();
        let path = directory.join("hosts.fixture");
        fs::write(&path, "# changed\n").unwrap();
        assert!(write_plan(&path, "# original\n", "# new\n").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# changed\n");
        assert!(!sibling(&path, ".kaleidoscope.lock").unwrap().exists());
        fs::write(
            sibling(&path, ".kaleidoscope.lock").unwrap(),
            "other process",
        )
        .unwrap();
        assert!(write_plan(&path, "# changed\n", "# new\n").is_err());
        assert_eq!(
            fs::read_to_string(sibling(&path, ".kaleidoscope.lock").unwrap()).unwrap(),
            "other process"
        );
        cleanup(directory);
    }

    #[test]
    fn rejects_non_utf8_and_non_files() {
        let directory = scratch();
        let path = directory.join("hosts.fixture");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read_hosts(&path).is_err());
        assert!(read_hosts(&directory).is_err());
        assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
        cleanup(directory);
    }

    #[test]
    fn refuses_oversize_input_and_output_before_writing() {
        let directory = scratch();
        let path = directory.join("hosts.fixture");
        let too_large = "#".repeat(MAX_HOSTS_BYTES + 1);
        fs::write(&path, &too_large).unwrap();
        assert!(read_hosts(&path).is_err());
        assert!(write_plan(&path, &too_large, "small").is_err());
        assert!(write_plan(&path, "small", &too_large).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), too_large);
        let near_limit = "#".repeat(MAX_HOSTS_BYTES);
        assert!(plan_apply(&near_limit, &mapping()).is_err());
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        cleanup(directory);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_targets() {
        let directory = scratch();
        let path = directory.join("actual");
        let link = directory.join("hosts.link");
        fs::write(&path, "original").unwrap();
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(read_hosts(&link).is_err());
        assert!(write_plan(&link, "original", "changed").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        cleanup(directory);
    }
}
