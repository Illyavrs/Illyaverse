//! Exact-file ruleset installation with a persistent recovery record.
//!
//! The guard holds an OS lock for the whole client session. A verified, synced
//! staging file is published with create-new hard-link semantics, so a crash can
//! never expose a partially written DLL. Recovery only removes bytes whose hash
//! still matches the record; ambiguous or externally modified files stay intact.

use crate::Result;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

pub const RULESET_FILENAME: &str = "osu.Game.Rulesets.Transfiguration.dll";
pub const JOURNAL_FILENAME: &str = ".illyaverse-launcher.session.json";
const LOCK_FILENAME: &str = ".illyaverse-launcher.lock";
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_JOURNAL_BYTES: u64 = 16 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleState {
    Planned,
    Staged,
    Installed,
    CleanupPending,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u8,
    root: PathBuf,
    root_identity: String,
    session_id: Uuid,
    artifact_sha256: String,
    artifact_identity: Option<String>,
    state: LifecycleState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallReceipt {
    pub session_id: Uuid,
    pub installed_path: PathBuf,
    pub journal_path: PathBuf,
    pub artifact_sha256: String,
    pub state: LifecycleState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CleanupOutcome {
    /// No journal or owned DLL existed.
    NothingToDo,
    /// The owned DLL, any complete staging artifact, and recovery record were removed.
    Recovered,
}

/// An exclusive installation guard. Drop releases the lock but deliberately does
/// not hide cleanup failures. Call `cleanup` after the client exits; on the next
/// launch, call `recover` before attempting another installation.
pub struct ManagedRuleset {
    root: PathBuf,
    root_identity: String,
    lock: File,
}

impl ManagedRuleset {
    /// The ruleset directory must already exist and be an absolute, non-linked
    /// directory. The launcher may hold this guard while monitoring its client.
    pub fn open(ruleset_root: impl AsRef<Path>) -> Result<Self> {
        let root = checked_directory(ruleset_root.as_ref())?;
        let root_identity = directory_identity(&root)?;
        let lock_path = root.join(LOCK_FILENAME);
        let lock = match fresh_file(&lock_path) {
            Ok(lock) => lock,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                check_regular_file(&lock_path)?;
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock_path)
                    .map_err(|error| format!("Cannot open launcher lock: {error}"))?
            }
            Err(error) => return Err(format!("Cannot create launcher lock: {error}")),
        };
        reject_link(&lock.metadata().map_err(|error| error.to_string())?)?;
        if !lock
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("Launcher lock is not a regular file".into());
        }
        FileExt::try_lock_exclusive(&lock)
            .map_err(|_| "Another launcher session owns this ruleset directory".to_owned())?;
        let guard = Self {
            root,
            root_identity,
            lock,
        };
        guard.check_root()?;
        Ok(guard)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join(JOURNAL_FILENAME)
    }

    pub fn installed_path(&self) -> PathBuf {
        self.root.join(RULESET_FILENAME)
    }

    /// Inspect a validated recovery record without changing any files.
    pub fn active_session(&self) -> Result<Option<InstallReceipt>> {
        self.check_root()?;
        Ok(self.read_journal()?.map(|journal| self.receipt(&journal)))
    }

    /// Install exactly the artifact identified by the release manifest hash.
    /// No destination or pre-existing recovery record is overwritten.
    pub fn install(
        &mut self,
        source: impl AsRef<Path>,
        expected_sha256: &str,
    ) -> Result<InstallReceipt> {
        validate_hash(expected_sha256)?;
        self.check_root()?;
        if self.read_journal()?.is_some() {
            return Err("An earlier launcher session requires recovery before installation".into());
        }
        if exists(&self.installed_path())? {
            return Err(
                "The fixed ruleset filename is already occupied; nothing was overwritten".into(),
            );
        }
        let artifact = read_artifact(source.as_ref())?;
        if hash_bytes(&artifact) != expected_sha256 {
            return Err("Ruleset source SHA256 does not match the release manifest".into());
        }

        let mut journal = Journal {
            version: 1,
            root: self.root.clone(),
            root_identity: self.root_identity.clone(),
            session_id: Uuid::new_v4(),
            artifact_sha256: expected_sha256.to_owned(),
            artifact_identity: None,
            state: LifecycleState::Planned,
        };
        // The recovery record is durable before any ruleset artifact is created.
        self.write_journal(&journal, true)?;
        let stage_path = self.stage_path(&journal);
        let mut stage = fresh_file(&stage_path)
            .map_err(|error| format!("Cannot create new ruleset staging file: {error}"))?;
        stage
            .write_all(&artifact)
            .and_then(|()| stage.sync_all())
            .map_err(|error| format!("Ruleset staging failed; recovery is required: {error}"))?;
        drop(stage);
        self.check_root()?;
        verify_owned_file(&stage_path, &journal.artifact_sha256)?;
        journal.artifact_identity = Some(file_identity(&stage_path)?);
        journal.state = LifecycleState::Staged;
        self.write_journal(&journal, false)?;
        // hard_link is atomic and refuses an existing destination on Windows and
        // Unix. It provides create-new semantics without a partial DLL window.
        fs::hard_link(&stage_path, self.installed_path()).map_err(|error| {
            format!("Ruleset publication failed; recovery is required: {error}")
        })?;
        sync_directory(&self.root)?;
        self.verify_artifact(&self.installed_path(), &journal, true)?;
        journal.state = LifecycleState::Installed;
        self.write_journal(&journal, false)?;
        remove_owned_file(&stage_path, &journal.artifact_sha256)?;
        sync_directory(&self.root)?;
        Ok(self.receipt(&journal))
    }

    /// Recover a terminated or crashed launcher session. This also handles the
    /// publication-before-state-update window because publication is atomic.
    pub fn recover(&mut self) -> Result<CleanupOutcome> {
        self.cleanup()
    }

    /// Delete only the fixed DLL and exact UUID-derived staging path whose bytes
    /// match the validated journal. Changed files remain available for review.
    pub fn cleanup(&mut self) -> Result<CleanupOutcome> {
        self.check_root()?;
        let Some(mut journal) = self.read_journal()? else {
            if exists(&self.installed_path())? {
                return Err(
                    "A ruleset exists without an ownership journal; it was preserved".into(),
                );
            }
            return Ok(CleanupOutcome::NothingToDo);
        };
        journal.state = LifecycleState::CleanupPending;
        self.write_journal(&journal, false)?;
        let installed = self.installed_path();
        let staged = self.stage_path(&journal);
        // Verify every candidate first, so an ambiguous staging file cannot
        // produce a misleading partially successful cleanup.
        for (path, destination) in [(&installed, true), (&staged, false)] {
            if exists(path)? {
                self.verify_artifact(path, &journal, destination)?;
            }
        }
        self.check_root()?;
        for (path, destination) in [(&installed, true), (&staged, false)] {
            if exists(path)? {
                self.check_root()?;
                self.verify_artifact(path, &journal, destination)?;
                fs::remove_file(path)
                    .map_err(|error| format!("Cannot remove exact owned artifact: {error}"))?;
            }
        }
        self.check_root()?;
        // Validate the current record again before deleting it. An external edit
        // cannot silently replace our directory or ownership information.
        let current = self
            .read_journal()?
            .ok_or("Recovery journal disappeared during cleanup")?;
        if current.session_id != journal.session_id
            || current.artifact_sha256 != journal.artifact_sha256
            || current.artifact_identity != journal.artifact_identity
            || current.state != LifecycleState::CleanupPending
        {
            return Err("Recovery journal changed during cleanup; it was preserved".into());
        }
        fs::remove_file(self.journal_path())
            .map_err(|error| format!("Cannot remove completed recovery journal: {error}"))?;
        sync_directory(&self.root)?;
        Ok(CleanupOutcome::Recovered)
    }

    fn receipt(&self, journal: &Journal) -> InstallReceipt {
        InstallReceipt {
            session_id: journal.session_id,
            installed_path: self.installed_path(),
            journal_path: self.journal_path(),
            artifact_sha256: journal.artifact_sha256.clone(),
            state: journal.state,
        }
    }

    fn stage_path(&self, journal: &Journal) -> PathBuf {
        self.root.join(format!(
            ".illyaverse-launcher-{}.stage",
            journal.session_id.hyphenated()
        ))
    }

    fn check_root(&self) -> Result<()> {
        if checked_directory(&self.root)? != self.root
            || directory_identity(&self.root)? != self.root_identity
        {
            return Err("The ruleset directory identity changed; no files were removed".into());
        }
        Ok(())
    }

    fn verify_artifact(&self, path: &Path, journal: &Journal, destination: bool) -> Result<()> {
        verify_owned_file(path, &journal.artifact_sha256)?;
        match &journal.artifact_identity {
            Some(expected) if file_identity(path)? == *expected => Ok(()),
            None if !destination => Ok(()),
            _ => Err(format!(
                "Artifact file identity is not owned by this session; it was preserved: {}",
                path.display()
            )),
        }
    }

    fn read_journal(&self) -> Result<Option<Journal>> {
        let path = self.journal_path();
        if !exists(&path)? {
            return Ok(None);
        }
        check_regular_file(&path)?;
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(|error| format!("Cannot open recovery journal: {error}"))?
            .take(MAX_JOURNAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot read recovery journal: {error}"))?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err("Recovery journal exceeds the size limit".into());
        }
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| "Recovery journal is invalid; existing files were preserved".to_owned())?;
        if journal.version != 1
            || journal.root != self.root
            || journal.root_identity != self.root_identity
            || journal.session_id.is_nil()
        {
            return Err("Recovery journal does not identify this ruleset directory".into());
        }
        validate_hash(&journal.artifact_sha256)?;
        if journal
            .artifact_identity
            .as_ref()
            .is_some_and(|identity| identity.is_empty() || identity.len() > 128)
            || (matches!(
                journal.state,
                LifecycleState::Staged | LifecycleState::Installed
            ) && journal.artifact_identity.is_none())
        {
            return Err("Recovery journal has an invalid artifact identity".into());
        }
        Ok(Some(journal))
    }

    fn write_journal(&self, journal: &Journal, initial: bool) -> Result<()> {
        self.check_root()?;
        let path = self.journal_path();
        if !initial {
            let previous = self
                .read_journal()?
                .ok_or("Recovery journal disappeared before a state update")?;
            if previous.session_id != journal.session_id
                || previous.artifact_sha256 != journal.artifact_sha256
                || (previous.artifact_identity.is_some()
                    && previous.artifact_identity != journal.artifact_identity)
            {
                return Err("Recovery journal changed before a state update".into());
            }
        }
        let temporary = self.root.join(format!(
            ".illyaverse-journal-{}.tmp",
            Uuid::new_v4().hyphenated()
        ));
        let bytes = serde_json::to_vec(journal).map_err(|error| error.to_string())?;
        let mut file = fresh_file(&temporary)
            .map_err(|error| format!("Cannot create recovery journal stage: {error}"))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("Cannot persist recovery journal: {error}"))?;
        drop(file);
        self.check_root()?;
        let publication = if initial {
            fs::hard_link(&temporary, &path)
        } else {
            replace_journal(&temporary, &path)
        };
        if let Err(error) = publication {
            // This exact file was create-new in this call, and its bytes have
            // already been synced. It is never a wildcard or a journal path.
            let _ = remove_owned_file(&temporary, &hash_bytes(&bytes));
            return Err(format!("Cannot publish recovery journal: {error}"));
        }
        if initial {
            remove_owned_file(&temporary, &hash_bytes(&bytes))?;
        }
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| format!("Cannot sync published recovery journal: {error}"))?;
        sync_directory(&self.root)
    }
}

impl Drop for ManagedRuleset {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
        // Keep the lock inode: removing it could let a second process lock a new
        // inode while another process still owns the old one.
    }
}

/// Useful for packaging a manifest, but callers must obtain the trusted expected
/// hash from their release manifest rather than trusting arbitrary local input.
pub fn artifact_sha256(path: impl AsRef<Path>) -> Result<String> {
    Ok(hash_bytes(&read_artifact(path.as_ref())?))
}

fn validate_hash(hash: &str) -> Result<()> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Artifact SHA256 must be exactly 64 lowercase hexadecimal characters".into());
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_artifact(path: &Path) -> Result<Vec<u8>> {
    check_absolute_components(path)?;
    let metadata = check_regular_file(path)?;
    if metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
        return Err("Ruleset artifact must be non-empty and at most 64 MiB".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("Cannot open ruleset artifact: {error}"))?
        .take(MAX_ARTIFACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read ruleset artifact: {error}"))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err("Ruleset artifact changed size while reading".into());
    }
    Ok(bytes)
}

fn verify_owned_file(path: &Path, expected_hash: &str) -> Result<()> {
    check_regular_file(path)?;
    if artifact_sha256(path)? != expected_hash {
        return Err(format!(
            "Owned file changed; it was preserved for review: {}",
            path.display()
        ));
    }
    Ok(())
}

fn remove_owned_file(path: &Path, expected_hash: &str) -> Result<()> {
    verify_owned_file(path, expected_hash)?;
    fs::remove_file(path)
        .map_err(|error| format!("Cannot remove exact owned file {}: {error}", path.display()))
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect {}: {error}", path.display())),
    }
}

fn fresh_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

fn reject_link(metadata: &Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() {
        return Err("Linked paths are not permitted for launcher ownership".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Windows reparse points are not permitted for launcher ownership".into());
        }
    }
    Ok(())
}

fn check_regular_file(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect {}: {error}", path.display()))?;
    reject_link(&metadata)?;
    if !metadata.is_file() {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    Ok(metadata)
}

fn check_absolute_components(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err("Launcher paths must be absolute without traversal components".into());
    }
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| {
            format!(
                "Cannot inspect path component {}: {error}",
                ancestor.display()
            )
        })?;
        reject_link(&metadata)?;
    }
    Ok(())
}

fn checked_directory(path: &Path) -> Result<PathBuf> {
    check_absolute_components(path)?;
    if !fs::symlink_metadata(path)
        .map_err(|error| error.to_string())?
        .is_dir()
    {
        return Err("Ruleset root is not a directory".into());
    }
    fs::canonicalize(path).map_err(|error| format!("Cannot resolve ruleset directory: {error}"))
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn file_identity(path: &Path) -> Result<String> {
    use std::ffi::c_void;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct FileIdInformation {
        volume_serial: u64,
        identifier: [u8; 16],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            handle: *mut c_void,
            information_class: i32,
            information: *mut c_void,
            buffer_size: u32,
        ) -> i32;
    }
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000 | 0x00200000)
        .open(path)
        .map_err(|error| format!("Cannot open directory identity: {error}"))?;
    reject_link(&directory.metadata().map_err(|error| error.to_string())?)?;
    let mut information = std::mem::MaybeUninit::<FileIdInformation>::uninit();
    // FileIdInfo (18) uses the full 128-bit ID on both NTFS and ReFS. The OS
    // initializes the documented FILE_ID_INFO layout only when it succeeds.
    if unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            18,
            information.as_mut_ptr().cast(),
            std::mem::size_of::<FileIdInformation>() as u32,
        )
    } == 0
    {
        return Err(format!(
            "Cannot read directory identity: {}",
            std::io::Error::last_os_error()
        ));
    }
    let information = unsafe { information.assume_init() };
    Ok(format!(
        "windows:{}:{}",
        information.volume_serial,
        hash_bytes(&information.identifier)
    ))
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_path: &Path) -> Result<String> {
    Err("Managed launcher installation is unsupported on this platform".into())
}

pub(super) fn directory_identity(path: &Path) -> Result<String> {
    file_identity(path)
}

#[cfg(unix)]
fn replace_journal(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_journal(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<_> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Replace-existing plus write-through; both names were checked under the session lock.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn replace_journal(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("Unsupported journal platform"))
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Cannot sync ruleset directory: {error}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
