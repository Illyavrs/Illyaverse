use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kaleidoscope-cli-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn hosts(&self) -> PathBuf {
        self.0.join("hosts.fixture")
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_kaleidoscope"))
            .args(args)
            .arg("--hosts")
            .arg(self.hosts())
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let path = fs::canonicalize(&self.0).unwrap();
        let temp = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(path.parent(), Some(temp.as_path()));
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kaleidoscope-cli-test-"));
        fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn preview_does_not_create_backups_or_locks() {
    let fixture = Fixture::new();
    let original = b"127.0.0.1 localhost\r\n# keep this\r\n";
    fs::write(fixture.hosts(), original).unwrap();
    for action in ["plan", "apply"] {
        let output = fixture.run(&[action, "--ip", "127.0.0.1", "--domain", "osu.illya.test"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Mode: preview"));
        assert_eq!(fs::read(fixture.hosts()).unwrap(), original);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }
    let output = fixture.run(&[
        "apply",
        "--ip",
        "::1",
        "--domain",
        "osu.illya.test",
        "--dry-run",
    ]);
    assert!(output.status.success());
    assert_eq!(fs::read(fixture.hosts()).unwrap(), original);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn explicit_apply_and_restore_keep_other_edits() {
    let fixture = Fixture::new();
    let original = "127.0.0.1 localhost";
    fs::write(fixture.hosts(), original).unwrap();
    let output = fixture.run(&[
        "apply",
        "--ip",
        "127.0.0.1",
        "--domain",
        "osu.illya.test",
        "--write",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Applied and verified"));
    let installed = fs::read_to_string(fixture.hosts()).unwrap();
    let edited = format!("{installed}10.0.0.1 keep.test\n");
    fs::write(fixture.hosts(), &edited).unwrap();
    assert!(fixture.run(&["restore"]).status.success());
    assert_eq!(fs::read_to_string(fixture.hosts()).unwrap(), edited);
    assert!(fixture.run(&["restore", "--write"]).status.success());
    assert_eq!(
        fs::read_to_string(fixture.hosts()).unwrap(),
        format!("{original}\n10.0.0.1 keep.test\n")
    );
    assert!(!fixture.0.join("hosts.fixture.kaleidoscope.lock").exists());
    let backups: Vec<_> = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bak"))
        .collect();
    assert_eq!(backups.len(), 2);
    assert!(backups
        .iter()
        .any(|path| fs::read_to_string(path).unwrap() == original));
}

#[test]
fn rejects_unsafe_args_without_writing() {
    let fixture = Fixture::new();
    fs::write(fixture.hosts(), "# unchanged\n").unwrap();
    for args in [
        &[
            "apply",
            "--ip",
            "127.0.0.1",
            "--domain",
            "osu.ppy.sh",
            "--write",
        ][..],
        &[
            "apply",
            "--ip",
            "127.0.0.1",
            "--domain",
            "evil.test\nnew-entry",
            "--write",
        ],
        &[
            "apply",
            "--ip",
            "127.0.0.1",
            "--domain",
            "illya.test",
            "--write",
            "--dry-run",
        ],
        &["plan", "--write"],
    ] {
        assert_eq!(fixture.run(args).status.code(), Some(2));
        assert_eq!(
            fs::read_to_string(fixture.hosts()).unwrap(),
            "# unchanged\n"
        );
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }
}

#[test]
fn lazer_profile_does_not_invent_a_server_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_kaleidoscope"))
        .args(["profile", "--client", "lazer", "--domain", "illya.test"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("stock-client-unverified"));
    assert!(output.contains("https://spectator.illya.test/spectator"));
    assert!(!output.contains("--server"));
}

#[test]
fn reviewed_snapshot_rejects_intervening_changes_before_a_write() {
    let fixture = Fixture::new();
    let original = "127.0.0.1 localhost\r\n";
    fs::write(fixture.hosts(), original).unwrap();
    let snapshot = fixture.0.join("reviewed.snapshot");
    fs::write(&snapshot, original).unwrap();
    let snapshot_arg = snapshot.to_str().unwrap();
    let changed = format!("{original}10.0.0.2 added.test\r\n");
    fs::write(fixture.hosts(), &changed).unwrap();
    for action in ["apply", "restore"] {
        let mut args = vec![action, "--write", "--expect-current", snapshot_arg];
        if action == "apply" {
            args.extend(["--ip", "127.0.0.1", "--domain", "illya.test"]);
        }
        let output = fixture.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("reviewed snapshot"));
        assert_eq!(fs::read_to_string(fixture.hosts()).unwrap(), changed);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
    }
    fs::write(fixture.hosts(), original).unwrap();
    assert!(fixture
        .run(&[
            "apply",
            "--write",
            "--expect-current",
            snapshot_arg,
            "--ip",
            "127.0.0.1",
            "--domain",
            "illya.test",
        ])
        .status
        .success());
}
