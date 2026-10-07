use kaleidoscope::launcher::install::{
    artifact_sha256, CleanupOutcome, LifecycleState, ManagedRuleset, JOURNAL_FILENAME,
    RULESET_FILENAME,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const ARTIFACT: &[u8] = b"fixture managed transfiguration artifact, never a real client DLL";

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    source: PathBuf,
    hash: String,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "kaleidoscope-launcher-test-{}",
            Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&directory).unwrap();
        let directory = fs::canonicalize(directory).unwrap();
        let root = directory.join("rulesets");
        fs::create_dir(&root).unwrap();
        let source = directory.join("release.fixture");
        fs::write(&source, ARTIFACT).unwrap();
        let hash = artifact_sha256(&source).unwrap();
        Self {
            directory,
            root,
            source,
            hash,
        }
    }

    fn manager(&self) -> ManagedRuleset {
        ManagedRuleset::open(&self.root).unwrap()
    }

    fn journal(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join(JOURNAL_FILENAME)).unwrap()).unwrap()
    }

    fn write_journal(&self, value: &Value) {
        fs::write(
            self.root.join(JOURNAL_FILENAME),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }

    fn stage(&self, journal: &Value) -> PathBuf {
        self.root.join(format!(
            ".illyaverse-launcher-{}.stage",
            journal["session_id"].as_str().unwrap()
        ))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // No arbitrary or user directory is ever recursively removed by tests.
        let directory = fs::canonicalize(&self.directory).unwrap();
        let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(directory.parent(), Some(temporary.as_path()));
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kaleidoscope-launcher-test-"));
        fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn exact_installation_cleanup_is_repeatable_and_preserves_other_rulesets() {
    let fixture = Fixture::new();
    let unrelated = fixture.root.join("osu.Game.Rulesets.Other.dll");
    fs::write(&unrelated, b"other ruleset").unwrap();
    let mut manager = fixture.manager();
    assert_eq!(manager.cleanup().unwrap(), CleanupOutcome::NothingToDo);
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    assert_eq!(receipt.state, LifecycleState::Installed);
    assert_eq!(receipt.artifact_sha256, fixture.hash);
    assert_eq!(
        receipt.installed_path,
        manager.root().join(RULESET_FILENAME)
    );
    assert_eq!(fs::read(&receipt.installed_path).unwrap(), ARTIFACT);
    assert_eq!(manager.active_session().unwrap().unwrap(), receipt);
    assert!(manager.install(&fixture.source, &fixture.hash).is_err());
    assert_eq!(manager.cleanup().unwrap(), CleanupOutcome::Recovered);
    assert!(!receipt.installed_path.exists());
    assert!(!receipt.journal_path.exists());
    assert_eq!(manager.cleanup().unwrap(), CleanupOutcome::NothingToDo);
    assert_eq!(fs::read(unrelated).unwrap(), b"other ruleset");
}

#[test]
fn exclusive_session_lock_is_released_on_drop_and_recovery_owns_old_session() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    manager.install(&fixture.source, &fixture.hash).unwrap();
    let root = fixture.root.clone();
    let rejected = std::thread::spawn(move || ManagedRuleset::open(root).is_err());
    assert!(rejected.join().unwrap());
    assert!(fixture.root.join(RULESET_FILENAME).exists());
    drop(manager);
    let mut next_session = fixture.manager();
    assert_eq!(next_session.recover().unwrap(), CleanupOutcome::Recovered);
    assert!(!fixture.root.join(RULESET_FILENAME).exists());
}

#[test]
fn manifest_mismatch_empty_source_and_invalid_hash_never_install_or_journal() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    assert!(manager.install(&fixture.source, &"0".repeat(64)).is_err());
    assert!(manager.install(&fixture.source, "../dll").is_err());
    assert!(manager.install(&fixture.source, &"A".repeat(64)).is_err());
    fs::write(&fixture.source, []).unwrap();
    assert!(manager.install(&fixture.source, &fixture.hash).is_err());
    assert!(!fixture.root.join(RULESET_FILENAME).exists());
    assert!(!fixture.root.join(JOURNAL_FILENAME).exists());
}

#[test]
fn a_foreign_file_is_preserved_even_if_its_bytes_match_the_manifest() {
    let fixture = Fixture::new();
    let destination = fixture.root.join(RULESET_FILENAME);
    fs::write(&destination, ARTIFACT).unwrap();
    let mut manager = fixture.manager();
    assert!(manager.install(&fixture.source, &fixture.hash).is_err());
    assert!(manager.recover().is_err());
    assert_eq!(fs::read(&destination).unwrap(), ARTIFACT);
    assert!(!fixture.root.join(JOURNAL_FILENAME).exists());
}

#[test]
fn concurrent_same_hash_publication_collision_is_not_claimed_as_owned() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    let mut journal = fixture.journal();
    let staged = fixture.stage(&journal);
    fs::hard_link(&receipt.installed_path, &staged).unwrap();
    fs::remove_file(&receipt.installed_path).unwrap();
    // A foreign create-new won the fixed filename while publication was pending.
    fs::write(&receipt.installed_path, ARTIFACT).unwrap();
    journal["state"] = Value::String("staged".into());
    fixture.write_journal(&journal);
    assert!(manager.recover().unwrap_err().contains("identity"));
    assert_eq!(fs::read(&receipt.installed_path).unwrap(), ARTIFACT);
    assert_eq!(fs::read(staged).unwrap(), ARTIFACT);
    assert!(receipt.journal_path.exists());
}

#[test]
fn changed_installed_artifact_is_preserved_and_cleanup_remains_pending() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    fs::write(&receipt.installed_path, b"external replacement").unwrap();
    assert!(manager.cleanup().is_err());
    assert_eq!(
        manager.active_session().unwrap().unwrap().state,
        LifecycleState::CleanupPending
    );
    assert_eq!(
        fs::read(&receipt.installed_path).unwrap(),
        b"external replacement"
    );
    drop(manager);
    let mut recovered = fixture.manager();
    assert!(recovered.recover().is_err());
    assert_eq!(
        fs::read(&receipt.installed_path).unwrap(),
        b"external replacement"
    );
    // A deliberate operator restoration of the exact release bytes unblocks recovery.
    fs::write(&receipt.installed_path, ARTIFACT).unwrap();
    assert_eq!(recovered.recover().unwrap(), CleanupOutcome::Recovered);
}

#[test]
fn planned_and_published_crash_windows_recover_without_partial_dlls() {
    for publication_happened in [false, true] {
        let fixture = Fixture::new();
        let mut manager = fixture.manager();
        let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
        let mut journal = fixture.journal();
        let staged = fixture.stage(&journal);
        fs::hard_link(&receipt.installed_path, &staged).unwrap();
        if !publication_happened {
            fs::remove_file(&receipt.installed_path).unwrap();
        }
        journal["state"] = Value::String("planned".into());
        fixture.write_journal(&journal);
        drop(manager);
        let mut recovered = fixture.manager();
        assert_eq!(recovered.recover().unwrap(), CleanupOutcome::Recovered);
        assert!(!receipt.installed_path.exists());
        assert!(!staged.exists());
        assert!(!receipt.journal_path.exists());
    }
}

#[test]
fn an_unpublished_partial_stage_is_preserved_with_an_actionable_recovery_error() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    let mut journal = fixture.journal();
    let staged = fixture.stage(&journal);
    fs::remove_file(&receipt.installed_path).unwrap();
    fs::write(&staged, b"interrupted write").unwrap();
    journal["state"] = Value::String("planned".into());
    fixture.write_journal(&journal);
    let error = manager.recover().unwrap_err();
    assert!(error.contains("preserved"));
    assert_eq!(fs::read(&staged).unwrap(), b"interrupted write");
    assert!(!receipt.installed_path.exists());
    assert!(receipt.journal_path.exists());
}

#[test]
fn malformed_or_redirected_journals_never_select_another_path_for_deletion() {
    let fixture = Fixture::new();
    let mut manager = fixture.manager();
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    let original = fixture.journal();
    let outside = fixture.directory.join("outside.dll");
    fs::write(&outside, ARTIFACT).unwrap();
    let mut redirected = original.clone();
    redirected["root"] = Value::String(outside.to_string_lossy().into_owned());
    fixture.write_journal(&redirected);
    assert!(manager.recover().is_err());
    assert_eq!(fs::read(&outside).unwrap(), ARTIFACT);
    assert_eq!(fs::read(&receipt.installed_path).unwrap(), ARTIFACT);
    let mut traversal = original.clone();
    traversal["session_id"] = Value::String("../../outside.dll".into());
    fixture.write_journal(&traversal);
    assert!(manager.recover().is_err());
    let mut extra_path = original.clone();
    extra_path["installed_path"] = Value::String(outside.to_string_lossy().into_owned());
    fixture.write_journal(&extra_path);
    assert!(manager.recover().is_err());
    let mut wrong_identity = original.clone();
    wrong_identity["root_identity"] = Value::String("unrelated-directory".into());
    fixture.write_journal(&wrong_identity);
    assert!(manager.recover().is_err());
    fs::write(&receipt.journal_path, b"{incomplete journal").unwrap();
    assert!(manager.recover().is_err());
    assert_eq!(fs::read(&outside).unwrap(), ARTIFACT);
    assert_eq!(fs::read(&receipt.installed_path).unwrap(), ARTIFACT);
    fixture.write_journal(&original);
    assert_eq!(manager.recover().unwrap(), CleanupOutcome::Recovered);
    assert_eq!(fs::read(outside).unwrap(), ARTIFACT);
}

#[test]
fn root_replacement_and_traversal_are_rejected_before_cleanup() {
    let fixture = Fixture::new();
    // PathBuf::push normalizes `..` on Windows verbatim canonical paths. Use an
    // ordinary absolute spelling to keep the actual traversal in this fixture.
    let root_spelling = fixture.root.to_string_lossy();
    let ordinary_root = root_spelling
        .strip_prefix(r"\\?\")
        .unwrap_or(&root_spelling);
    assert!(ManagedRuleset::open(Path::new(ordinary_root).join("..").join("rulesets")).is_err());
    assert!(ManagedRuleset::open(Path::new("relative/rulesets")).is_err());
    let mut manager = fixture.manager();
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    drop(manager);
    let old_root = fixture.directory.join("original-rulesets");
    fs::rename(&fixture.root, &old_root).unwrap();
    fs::create_dir(&fixture.root).unwrap();
    let replacement = fixture.root.join(RULESET_FILENAME);
    fs::write(&replacement, ARTIFACT).unwrap();
    fs::copy(
        old_root.join(JOURNAL_FILENAME),
        fixture.root.join(JOURNAL_FILENAME),
    )
    .unwrap();
    let mut restarted = fixture.manager();
    assert!(restarted.recover().is_err());
    assert_eq!(fs::read(replacement).unwrap(), ARTIFACT);
    assert_eq!(fs::read(old_root.join(RULESET_FILENAME)).unwrap(), ARTIFACT);
    assert!(old_root
        .join(receipt.journal_path.file_name().unwrap())
        .exists());
}

#[cfg(any(unix, windows))]
#[test]
fn symlinked_root_source_destination_and_journal_are_preserved() {
    let fixture = Fixture::new();
    let source_link = fixture.directory.join("source-link");
    let root_link = fixture.directory.join("root-link");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&fixture.source, &source_link).unwrap();
        std::os::unix::fs::symlink(&fixture.root, &root_link).unwrap();
    }
    #[cfg(windows)]
    {
        // Windows symlink tests require Developer Mode or the symlink privilege.
        if std::os::windows::fs::symlink_file(&fixture.source, &source_link).is_err() {
            eprintln!(
                "SKIPPED symlink fixtures: Windows Developer Mode / symlink privilege unavailable"
            );
            return;
        }
        std::os::windows::fs::symlink_dir(&fixture.root, &root_link).unwrap();
    }
    assert!(ManagedRuleset::open(&root_link).is_err());
    let mut manager = fixture.manager();
    assert!(manager.install(&source_link, &fixture.hash).is_err());
    let receipt = manager.install(&fixture.source, &fixture.hash).unwrap();
    fs::remove_file(&receipt.installed_path).unwrap();
    symlink_file(&fixture.source, &receipt.installed_path);
    assert!(manager.cleanup().is_err());
    assert_eq!(fs::read(&fixture.source).unwrap(), ARTIFACT);
    fs::remove_file(&receipt.installed_path).unwrap();
    fs::write(&receipt.installed_path, ARTIFACT).unwrap();
    fs::remove_file(&receipt.journal_path).unwrap();
    symlink_file(&fixture.source, &receipt.journal_path);
    assert!(manager.cleanup().is_err());
    assert_eq!(fs::read(&fixture.source).unwrap(), ARTIFACT);
    assert_eq!(fs::read(&receipt.installed_path).unwrap(), ARTIFACT);
}

#[cfg(unix)]
fn symlink_file(source: &Path, destination: &Path) {
    std::os::unix::fs::symlink(source, destination).unwrap();
}

#[cfg(windows)]
fn symlink_file(source: &Path, destination: &Path) {
    std::os::windows::fs::symlink_file(source, destination).unwrap();
}
