//! Fixed-server launcher sessions. An independent guardian owns installation,
//! client lifetime, and exact-file cleanup even after the visible UI closes.

use super::install::{artifact_sha256, ManagedRuleset, RULESET_FILENAME};
use super::storage::Library;
use super::{host, process, storage};
use crate::Result;
use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const SERVER: &str = "https://illya.test";
const DOMAIN: &str = "illya.test";
// Only stock assemblies independently inspected and tested with the bridge.
const SUPPORTED_GAME_SHA256: &str =
    "ad9527e411111cb4b4045c76df61c3474fd872a04af73160c05d24e44a588249";
const SUPPORTED_FRAMEWORK_SHA256: &str =
    "93b62f61c72539ed6fdb3c8eba18d276a74de8e1c094280b2531f5a76caea528";
const SUPPORTED_ENTRY_SHA256: &str =
    "18ae9bb6bb441e9a654ac72c859f527041bde52e5dce4109f8d9074584fa57a4";
const PREFS_FILENAME: &str = "prefs.json";
const SESSION_FILENAME: &str = "managed-session.json";
const MAX_RECORD_BYTES: u64 = 16 * 1024;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(15);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Client {
    Stable,
    Lazer,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Preparing,
    Running,
    Closing,
    Cleaning,
    CleanupFailed,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Compatibility {
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StableSnapshot {
    pub path: Option<String>,
    pub running: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LazerSnapshot {
    pub path: Option<String>,
    pub running: bool,
    pub compatibility: Compatibility,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub stable: StableSnapshot,
    pub lazer: LazerSnapshot,
    pub phase: Phase,
    pub detail: String,
    pub plugin_present: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    stable: Option<PathBuf>,
    lazer: Option<PathBuf>,
}

impl Preferences {
    fn path(&self, client: Client) -> Option<&Path> {
        match client {
            Client::Stable => self.stable.as_deref(),
            Client::Lazer => self.lazer.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Session {
    version: u8,
    nonce: String,
    client: Client,
    selected_executable: PathBuf,
    selected_identity: String,
    launcher_executable: PathBuf,
    launcher_identity: String,
    guardian: Option<process::Identity>,
    child: Option<process::Identity>,
    install_session: Option<Uuid>,
    // Version 2 records the resolved library before the guardian is started.
    // Recovery must never follow a subsequently changed storage.ini/preferences.
    #[serde(default)]
    library: Option<Library>,
    // Version 1 wrote readiness beside the old private library. This flag only
    // permits that fixed historical location while migrating recovery records.
    #[serde(default)]
    legacy_ready: bool,
    phase: Phase,
    detail: String,
    stop_requested: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    protocol: u8,
    session: String,
    status: String,
    endpoint: String,
    fixture: bool,
}

#[derive(Clone)]
struct Paths {
    control: PathBuf,
    roaming: PathBuf,
    legacy_data: PathBuf,
    launcher: PathBuf,
}

impl Paths {
    fn production() -> Result<Self> {
        #[cfg(not(windows))]
        return Err("Kaleidoscope 原生啟動器目前僅支援 Windows".into());
        #[cfg(windows)]
        {
            let local = storage::local_app_data()?;
            check_components(&local)?;
            let base = local.join("Illyaverse");
            let control = base.join("Kaleidoscope");
            ensure_directory(&control)?;
            let control = fs::canonicalize(control).map_err(|error| error.to_string())?;
            let roaming = storage::roaming_app_data()?;
            check_components(&roaming)?;
            let legacy_data = base.join("lazer");
            let launcher =
                canonical_file(&std::env::current_exe().map_err(|error| error.to_string())?)?;
            Ok(Self {
                control,
                roaming,
                legacy_data,
                launcher,
            })
        }
    }
    fn prefs(&self) -> PathBuf {
        self.control.join(PREFS_FILENAME)
    }
    fn session(&self) -> PathBuf {
        self.control.join(SESSION_FILENAME)
    }
    fn ready(&self, nonce: &str) -> PathBuf {
        self.control.join(format!("session-ready-{nonce}.json"))
    }
    fn bridge(&self) -> Result<PathBuf> {
        Ok(self
            .launcher
            .parent()
            .ok_or("啟動器缺少安裝資料夾")?
            .join(RULESET_FILENAME))
    }
}

fn resolve_library(paths: &Paths, executable: &Path) -> Result<Library> {
    storage::resolve(&paths.roaming, &paths.control, executable)
}

fn refuse_other_lazer_instances() -> Result<()> {
    // Also catches a selected client restarted during the launch race window.
    // Only start() may gracefully close the exact user-selected installation.
    if !process::find_lazer_candidates()?.is_empty() {
        return Err("另一個 lazer 正在執行；請先關閉後再使用現有圖譜庫".into());
    }
    Ok(())
}

/// Returns only client paths and launcher state. No hostname setting is exposed.
pub fn snapshot() -> Result<Snapshot> {
    snapshot_at(&Paths::production()?)
}

/// The caller owns the native file chooser; this validates and stores one client
/// executable path. Preferences never contain commands, arguments, or servers.
pub fn save_client_path(client: Client, executable: impl AsRef<Path>) -> Result<Snapshot> {
    let paths = Paths::production()?;
    let executable = validate_client(executable.as_ref(), client)?;
    let _lock = ControlLock::open(&paths)?;
    if let Some(session) = read_session(&paths)? {
        if session_is_live(&session)? {
            return Err("遊戲執行期間不能變更安裝路徑".into());
        }
    }
    let mut preferences = read_preferences(&paths)?;
    match client {
        Client::Stable => preferences.stable = Some(executable),
        Client::Lazer => preferences.lazer = Some(executable),
    }
    write_json(&paths.prefs(), &preferences)?;
    drop(_lock);
    snapshot_at(&paths)
}

/// A click authorizes gracefully restarting the exact selected installation.
/// No unrelated process or official-profile data is modified.
pub fn start(client: Client) -> Result<Snapshot> {
    let paths = Paths::production()?;
    let preferences = read_preferences(&paths)?;
    let selected = preferences
        .path(client)
        .ok_or("請先選擇 osu!.exe 安裝路徑")?;
    let selected = validate_client(selected, client)?;
    if client == Client::Lazer {
        verify_lazer_compatibility(&paths, &selected)?;
    }
    if read_session(&paths)?
        .as_ref()
        .map(session_is_live)
        .transpose()?
        .unwrap_or(false)
    {
        stop_at(&paths)?;
        wait_for_session_end(&paths, CLOSE_TIMEOUT + Duration::from_secs(5))?;
    }
    recover_at(&paths)?;
    // All shutdown identity checks happen before a ruleset is installed.
    for running in process::find_by_executable(&selected)? {
        process::close_gracefully(&running, CLOSE_TIMEOUT)?;
    }
    let library = if client == Client::Lazer {
        refuse_other_lazer_instances()?;
        Some(resolve_library(&paths, &selected)?)
    } else {
        None
    };
    let _lock = ControlLock::open(&paths)?;
    if read_session(&paths)?
        .as_ref()
        .map(session_is_live)
        .transpose()?
        .unwrap_or(false)
    {
        return Err("另一個啟動器已建立遊戲工作階段".into());
    }
    let nonce = fresh_nonce()?;
    let session = Session {
        version: 2,
        nonce: nonce.clone(),
        client,
        selected_identity: process::executable_identity(&selected)?,
        selected_executable: selected,
        launcher_identity: process::executable_identity(&paths.launcher)?,
        launcher_executable: paths.launcher.clone(),
        guardian: None,
        child: None,
        install_session: None,
        library,
        legacy_ready: false,
        phase: Phase::Preparing,
        detail: "正在準備 Illyaverse 連線".into(),
        stop_requested: false,
    };
    write_json(&paths.session(), &session)?;
    let mut command = Command::new(&paths.launcher);
    command.arg("__launch-session").arg(&nonce);
    hidden_child(&mut command);
    if let Err(error) = command.spawn() {
        let mut failed = session;
        failed.phase = Phase::Error;
        failed.detail = "無法啟動獨立的遊戲清理守護程序".into();
        write_json(&paths.session(), &failed)?;
        return Err(format!("無法啟動遊戲守護程序：{error}"));
    }
    drop(_lock);
    snapshot_at(&paths)
}

pub fn stop() -> Result<Snapshot> {
    let paths = Paths::production()?;
    stop_at(&paths)?;
    snapshot_at(&paths)
}

/// Recovery refuses to remove an installed bridge while its recorded guardian or
/// child is alive, and preserves ambiguous ownership instead of closing a PID.
pub fn recover() -> Result<Snapshot> {
    let paths = Paths::production()?;
    recover_at(&paths)?;
    snapshot_at(&paths)
}

/// Invoke before creating any CLI or Tauri UI. Arguments exclude the executable.
pub fn handle_internal(arguments: &[String]) -> Option<Result<i32>> {
    let operation = arguments.first()?.as_str();
    if operation != "__launch-session" && operation != "__host-lazer" {
        return None;
    }
    Some((|| {
        if arguments.len() != 2 || !valid_nonce(&arguments[1]) {
            return Err("內部啟動器命令缺少有效的工作階段識別碼".into());
        }
        let paths = Paths::production()?;
        match operation {
            "__launch-session" => run_guardian(&paths, &arguments[1]),
            "__host-lazer" => run_lazer_host(&paths, &arguments[1]),
            _ => unreachable!(),
        }
    })())
}

fn snapshot_at(paths: &Paths) -> Result<Snapshot> {
    let preferences = read_preferences(paths)?;
    let session = read_session(paths)?;
    let selected_running = |client: Client| -> Result<bool> {
        if let Some(path) = preferences.path(client) {
            if path.exists() && !process::find_by_executable(path)?.is_empty() {
                return Ok(true);
            }
        }
        if let Some(session) = &session {
            if session.client == client {
                if let Some(child) = &session.child {
                    return Ok(process::state(child)? == process::State::Live);
                }
            }
        }
        Ok(false)
    };
    let compatibility = match &preferences.lazer {
        None => Compatibility::Unknown,
        Some(path) => {
            if validate_client(path, Client::Lazer).is_ok()
                && verify_lazer_compatibility(paths, path).is_ok()
            {
                Compatibility::Supported
            } else {
                Compatibility::Unsupported
            }
        }
    };
    let current_library = session
        .as_ref()
        .filter(|record| !session_completed(record))
        .and_then(|record| record.library.clone())
        .or_else(|| {
            preferences
                .lazer
                .as_deref()
                .and_then(|path| resolve_library(paths, path).ok())
        });
    let plugin_present = match current_library {
        Some(library) => path_exists(&library.rulesets().join(RULESET_FILENAME))?,
        None => path_exists(&paths.legacy_data.join("rulesets").join(RULESET_FILENAME))?,
    };
    Ok(Snapshot {
        stable: StableSnapshot {
            path: preferences.stable.as_deref().map(display_path),
            running: selected_running(Client::Stable)?,
        },
        lazer: LazerSnapshot {
            path: preferences.lazer.as_deref().map(display_path),
            running: selected_running(Client::Lazer)?,
            compatibility,
        },
        phase: session
            .as_ref()
            .map(|session| session.phase)
            .unwrap_or(Phase::Idle),
        detail: session
            .map(|session| session.detail)
            .unwrap_or_else(|| "選擇遊戲安裝路徑以連線至 Illyaverse".into()),
        plugin_present,
    })
}

fn stop_at(paths: &Paths) -> Result<()> {
    let mut session = {
        let _lock = ControlLock::open(paths)?;
        let Some(mut session) = read_session(paths)? else {
            return Err("目前沒有受管理的遊戲工作階段".into());
        };
        session.stop_requested = true;
        session.phase = Phase::Closing;
        session.detail = "正在等待遊戲安全關閉".into();
        write_json(&paths.session(), &session)?;
        session
    };
    if let Some(child) = &session.child {
        match process::state(child)? {
            process::State::Gone => {}
            process::State::Ambiguous => return Err("無法確認遊戲程序身分；未送出關閉訊息".into()),
            process::State::Live => {
                if let Err(error) = process::close_gracefully(child, CLOSE_TIMEOUT) {
                    session.phase = Phase::Error;
                    session.detail = error.clone();
                    let _lock = ControlLock::open(paths)?;
                    if read_session(paths)?.is_some_and(|current| {
                        current.nonce == session.nonce && current.phase == Phase::Closing
                    }) {
                        write_json(&paths.session(), &session)?;
                    }
                    return Err(error);
                }
            }
        }
    }
    Ok(())
}

fn wait_for_session_end(paths: &Paths, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if !read_session(paths)?
            .as_ref()
            .map(session_is_live)
            .transpose()?
            .unwrap_or(false)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("遊戲或清理守護程序仍在執行；尚未重新啟動".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn session_is_live(session: &Session) -> Result<bool> {
    for identity in [&session.child, &session.guardian].into_iter().flatten() {
        match process::state(identity)? {
            process::State::Live => return Ok(true),
            process::State::Ambiguous => {
                return Err("工作階段程序身分無法確認；請先關閉遊戲並檢查狀態".into())
            }
            process::State::Gone => {}
        }
    }
    Ok(false)
}

fn session_completed(session: &Session) -> bool {
    matches!(session.phase, Phase::Idle | Phase::Error)
        && session.install_session.is_none()
        && session.child.is_none()
        && session.guardian.is_none()
        && !session.legacy_ready
}

fn recover_at(paths: &Paths) -> Result<()> {
    let _lock = ControlLock::open(paths)?;
    let session = read_session(paths)?;
    if session
        .as_ref()
        .map(session_is_live)
        .transpose()?
        .unwrap_or(false)
    {
        return Err("遊戲或其守護程序仍在執行；不會移除正在使用的連線模組".into());
    }
    let completed = session.as_ref().is_some_and(session_completed);
    let library = match &session {
        Some(_) if completed => None,
        Some(record) => record.library.clone(),
        None => {
            let preferences = read_preferences(paths)?;
            if let Some(selected) = preferences.lazer.as_deref() {
                Some(resolve_library(paths, selected)?)
            } else if path_exists(&paths.legacy_data)? {
                Some(Library::open(&paths.legacy_data)?)
            } else {
                None
            }
        }
    };
    if let Some(library) = library {
        library.validate()?;
        let rulesets = library.rulesets();
        if path_exists(&rulesets)? {
            let mut managed = ManagedRuleset::open(&rulesets)?;
            let journal = managed.active_session()?;
            if journal.is_some() {
                refuse_other_lazer_instances()?;
            }
            if journal.is_some() && session.is_none() {
                return Err("連線模組有復原紀錄，但缺少程序所有權紀錄；檔案已保留".into());
            }
            if let (Some(owned), Some(receipt)) = (
                session.as_ref().and_then(|value| value.install_session),
                journal.as_ref(),
            ) {
                if owned != receipt.session_id {
                    return Err("連線模組的工作階段身分不符；檔案已保留".into());
                }
            }
            if let Err(error) = managed.recover() {
                if let Some(mut failed) = session.clone() {
                    failed.phase = Phase::CleanupFailed;
                    failed.detail = error.clone();
                    write_json(&paths.session(), &failed)?;
                }
                return Err(error);
            }
        }
    }
    if let Some(mut session) = session {
        cleanup_session_ready(paths, &session)?;
        session.phase = Phase::Idle;
        session.detail = "已關閉遊戲並移除 Illyaverse 連線模組".into();
        session.guardian = None;
        session.child = None;
        session.install_session = None;
        session.legacy_ready = false;
        session.stop_requested = false;
        write_json(&paths.session(), &session)?;
    }
    Ok(())
}

fn run_guardian(paths: &Paths, nonce: &str) -> Result<i32> {
    let _guardian_lock = exclusive_file(&paths.control.join(".guardian.lock"))?;
    let mut session = {
        let _lock = ControlLock::open(paths)?;
        let mut session = matching_session(paths, nonce)?;
        if session.phase != Phase::Preparing
            || session.guardian.is_some()
            || session.child.is_some()
        {
            return Err("這個工作階段已被使用或取消".into());
        }
        let current = process::current_identity()?;
        if current.executable != session.launcher_executable
            || current.executable_identity != session.launcher_identity
        {
            return Err("守護程序並非此工作階段的啟動器".into());
        }
        session.guardian = Some(current);
        write_json(&paths.session(), &session)?;
        session
    };
    let mut managed: Option<ManagedRuleset> = None;
    let mut child: Option<Child> = None;
    let result = run_client(paths, &mut session, &mut managed, &mut child);
    if let Err(error) = &result {
        let _ = update_session(paths, nonce, |record| {
            record.phase = Phase::Error;
            record.detail = error.clone();
        });
    }
    // Never clean up on an error path while a spawned child remains alive.
    if let Some(child) = &mut child {
        match child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                child
                    .wait()
                    .map_err(|_| "無法確認遊戲關閉；復原紀錄已保留")?;
            }
            Err(_) => return Err("遊戲狀態無法確認；連線模組與復原紀錄已保留".into()),
        }
    }
    update_session(paths, nonce, |record| {
        record.phase = Phase::Cleaning;
        record.detail = "正在移除連線模組".into();
    })?;
    let cleanup = match &mut managed {
        Some(managed) => session
            .library
            .as_ref()
            .ok_or("清理缺少原有圖譜資料夾紀錄".into())
            .and_then(Library::validate)
            .and_then(|()| refuse_other_lazer_instances())
            .and_then(|()| managed.cleanup().map(|_| ())),
        None => Ok(()),
    }
    .and_then(|()| cleanup_ready(paths, nonce));
    let _lock = ControlLock::open(paths)?;
    let mut current = matching_session(paths, nonce)?;
    current.guardian = None;
    current.child = None;
    match cleanup {
        Ok(()) => {
            current.install_session = None;
            current.phase = if result.is_ok() {
                Phase::Idle
            } else {
                Phase::Error
            };
            current.detail = result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "已關閉遊戲並移除 Illyaverse 連線模組".into());
        }
        Err(error) => {
            current.phase = Phase::CleanupFailed;
            current.detail = error;
        }
    }
    write_json(&paths.session(), &current)?;
    Ok(if result.is_ok() && current.phase != Phase::CleanupFailed {
        0
    } else {
        1
    })
}

fn run_client(
    paths: &Paths,
    session: &mut Session,
    managed: &mut Option<ManagedRuleset>,
    child: &mut Option<Child>,
) -> Result<()> {
    let selected = validate_client(&session.selected_executable, session.client)?;
    if process::executable_identity(&selected)? != session.selected_identity {
        return Err("選擇的遊戲執行檔已變更；請重新選擇路徑".into());
    }
    if session.client == Client::Lazer {
        verify_lazer_compatibility(paths, &selected)?;
        refuse_other_lazer_instances()?;
        session
            .library
            .as_ref()
            .ok_or("lazer 缺少原有圖譜資料夾紀錄")?
            .validate()?;
    }
    if matching_session(paths, &session.nonce)?.stop_requested {
        return Err("啟動已取消".into());
    }
    if !process::find_by_executable(&selected)?.is_empty() {
        return Err("選擇的遊戲再次啟動；尚未安裝連線模組".into());
    }
    let mut command;
    match session.client {
        Client::Stable => {
            command = Command::new(&selected);
            command.arg("-devserver").arg(DOMAIN);
        }
        Client::Lazer => {
            let (bridge, hash) = verify_bridge(paths)?;
            let library = session
                .library
                .as_ref()
                .ok_or("lazer 缺少原有圖譜資料夾紀錄")?;
            library.validate()?;
            ensure_directory(&library.rulesets())?;
            *managed = Some(ManagedRuleset::open(library.rulesets())?);
            library.validate()?;
            let receipt = managed
                .as_mut()
                .expect("installation guard exists")
                .install(&bridge, &hash)?;
            session.install_session = Some(receipt.session_id);
            // Keep the guard even if installing or persisting the receipt fails.
            update_session(paths, &session.nonce, |record| {
                record.install_session = session.install_session
            })?;
            command = Command::new(&paths.launcher);
            command.arg("__host-lazer").arg(&session.nonce);
            command.env(
                "ILLYAVERSE_LAZER_INSTALL_ROOT",
                selected.parent().ok_or("lazer 安裝路徑缺少父資料夾")?,
            );
            command.env("ILLYAVERSE_LAZER_DATA_ROOT", &library.root);
            command.env("ILLYAVERSE_LAZER_CONTROL_ROOT", &paths.control);
            command.env("ILLYAVERSE_LAZER_SESSION", &session.nonce);
            command.env("ILLYAVERSE_LAZER_READY_FILE", paths.ready(&session.nonce));
            command.env("OSU_DISABLE_ERROR_REPORTING", "1");
            command.env("OSU_EXTERNAL_UPDATE_PROVIDER", "Illyaverse");
        }
    }
    sanitize_injection_environment(&mut command);
    hidden_child(&mut command);
    let spawned = command.spawn().map_err(|_| "無法啟動選擇的遊戲")?;
    let pid = spawned.id();
    *child = Some(spawned);
    if session.client == Client::Stable {
        let identity = process::identity(pid)?.ok_or("stable 在啟動完成前已關閉")?;
        update_session(paths, &session.nonce, |record| {
            record.child = Some(identity);
            record.phase = Phase::Running;
            record.detail = "stable 已啟動，使用 Illyaverse 私服參數".into();
        })?;
    }
    let ready_deadline = Instant::now() + READY_TIMEOUT;
    let mut ready = session.client == Client::Stable;
    loop {
        let status = child
            .as_mut()
            .expect("child exists after spawn")
            .try_wait()
            .map_err(|_| "無法查詢遊戲狀態")?;
        if let Some(status) = status {
            return if status.success() {
                Ok(())
            } else {
                Err("遊戲啟動或連線模組驗證失敗；已保留錯誤狀態".into())
            };
        }
        let record = matching_session(paths, &session.nonce)?;
        if session.client == Client::Lazer && !ready {
            if let Some(signal) = read_ready(paths, &session.nonce)? {
                validate_ready(&signal, &session.nonce)?;
                if record.child.as_ref().map(process::state).transpose()?
                    != Some(process::State::Live)
                {
                    return Err("安全啟動訊號缺少有效的遊戲程序身分".into());
                }
                ready = true;
                update_session(paths, &session.nonce, |record| {
                    record.phase = Phase::Running;
                    record.detail = "lazer 安全啟動驗證完成，已使用 Illyaverse 私服設定".into();
                })?;
            } else if Instant::now() >= ready_deadline {
                return Err("未收到安全啟動確認；關閉遊戲後才會清理連線模組".into());
            }
        }
        if record.stop_requested {
            if let Some(identity) = &record.child {
                if process::state(identity)? == process::State::Live {
                    // The stop caller also requests closure; a duplicate WM_CLOSE
                    // is harmless and ownership is revalidated before each post.
                    process::close_gracefully(identity, CLOSE_TIMEOUT)?;
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn run_lazer_host(paths: &Paths, nonce: &str) -> Result<i32> {
    if std::env::var("ILLYAVERSE_LAZER_SESSION").ok().as_deref() != Some(nonce) {
        return Err("缺少受管理的 lazer 工作階段環境".into());
    }
    let session = {
        let _lock = ControlLock::open(paths)?;
        let mut session = matching_session(paths, nonce)?;
        if session.client != Client::Lazer
            || session.stop_requested
            || session.phase != Phase::Preparing
            || session.child.is_some()
        {
            return Err("lazer 工作階段已取消或已被使用".into());
        }
        let guardian = session.guardian.as_ref().ok_or("lazer 缺少清理守護程序")?;
        if process::state(guardian)? != process::State::Live {
            return Err("清理守護程序已結束；不會進入遊戲 Main".into());
        }
        let own = process::current_identity()?;
        if own.executable != session.launcher_executable
            || own.executable_identity != session.launcher_identity
        {
            return Err("lazer 主機程序身分不符".into());
        }
        session.child = Some(own);
        // Persist identity BEFORE executing any managed startup hook or Main.
        write_json(&paths.session(), &session)?;
        session
    };
    let selected = validate_client(&session.selected_executable, Client::Lazer)?;
    verify_lazer_compatibility(paths, &selected)?;
    let install = selected.parent().ok_or("lazer 缺少安裝資料夾")?;
    let library = session
        .library
        .as_ref()
        .ok_or("lazer 缺少原有圖譜資料夾紀錄")?;
    library.validate()?;
    refuse_other_lazer_instances()?;
    validate_child_environment(paths, nonce, install, library)?;
    let (_, hash) = verify_bridge(paths)?;
    let bridge = library.rulesets().join(RULESET_FILENAME);
    if artifact_sha256(&bridge)? != hash {
        return Err("已安裝的連線模組與發行雜湊不符；不會進入 Main".into());
    }
    host::run_managed(
        &install.join("osu!.dll"),
        &install.join("hostfxr.dll"),
        &bridge,
    )
}

fn validate_child_environment(
    paths: &Paths,
    nonce: &str,
    install: &Path,
    library: &Library,
) -> Result<()> {
    for (name, expected) in [
        ("ILLYAVERSE_LAZER_INSTALL_ROOT", install.to_path_buf()),
        ("ILLYAVERSE_LAZER_DATA_ROOT", library.root.clone()),
        ("ILLYAVERSE_LAZER_CONTROL_ROOT", paths.control.clone()),
        ("ILLYAVERSE_LAZER_READY_FILE", paths.ready(nonce)),
    ] {
        if std::env::var_os(name).map(PathBuf::from).as_ref() != Some(&expected) {
            return Err("lazer 工作階段路徑環境不符".into());
        }
    }
    Ok(())
}

fn update_session(paths: &Paths, nonce: &str, update: impl FnOnce(&mut Session)) -> Result<()> {
    let _lock = ControlLock::open(paths)?;
    let mut session = matching_session(paths, nonce)?;
    update(&mut session);
    write_json(&paths.session(), &session)
}

fn matching_session(paths: &Paths, nonce: &str) -> Result<Session> {
    let session = read_session(paths)?.ok_or("缺少工作階段復原紀錄")?;
    if session.nonce != nonce {
        return Err("工作階段識別碼不符".into());
    }
    Ok(session)
}

fn read_preferences(paths: &Paths) -> Result<Preferences> {
    let preferences: Preferences = read_json(&paths.prefs())?.unwrap_or_default();
    for path in [&preferences.stable, &preferences.lazer]
        .into_iter()
        .flatten()
    {
        if !path.is_absolute()
            || path.as_os_str().len() > 32768
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("安裝路徑設定含有無效路徑".into());
        }
    }
    Ok(preferences)
}

fn read_session(paths: &Paths) -> Result<Option<Session>> {
    let mut session: Option<Session> = read_json(&paths.session())?;
    if let Some(session) = &mut session {
        if !matches!(session.version, 1 | 2)
            || !valid_nonce(&session.nonce)
            || session.detail.len() > 4096
            || !session.selected_executable.is_absolute()
            || !session.launcher_executable.is_absolute()
            || session.selected_identity.len() > 128
            || session.launcher_identity.len() > 128
            || !session
                .selected_executable
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("osu!.exe"))
            || session
                .selected_executable
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("工作階段復原紀錄無效；現有檔案已保留".into());
        }
        // The recorded launcher belongs to the historical session. Moving or
        // upgrading the current package must not prevent status or recovery;
        // guardian/host entry and live process ownership still match that record.
        if session.version == 1 {
            if session.library.is_some() {
                return Err("舊版工作階段不得指定另一個圖譜資料夾".into());
            }
            if session.client == Client::Lazer && path_exists(&paths.legacy_data)? {
                session.library = Some(Library::open(&paths.legacy_data)?);
            } else if session.install_session.is_some() {
                return Err("舊版私有圖譜資料夾已不見；復原紀錄已保留".into());
            } else if session.client == Client::Lazer {
                // No historical root, owned install or process means the old
                // guardian never reached installation. Permit journal recovery.
                if session.child.is_some() || session.guardian.is_some() {
                    return Err("舊版遊戲工作階段缺少原有資料夾；紀錄已保留".into());
                }
                session.phase = Phase::Error;
            }
            session.legacy_ready = session.client == Client::Lazer;
            session.version = 2;
        }
        if session.legacy_ready
            && (session.client != Client::Lazer
                || session.library.as_ref().is_some_and(|library| {
                    library.root != fs::canonicalize(&paths.legacy_data).unwrap_or_default()
                }))
        {
            return Err("舊版啟動訊號不得指定另一個資料夾".into());
        }
        match (&session.client, &session.library) {
            (Client::Lazer, Some(library)) => {
                library.validate_record()?;
            }
            (Client::Lazer, None)
                if session.install_session.is_none()
                    && matches!(session.phase, Phase::Idle | Phase::Error) => {}
            (Client::Stable, None) => {}
            _ => return Err("工作階段缺少一致的圖譜資料夾紀錄；檔案已保留".into()),
        }
        for (identity, guardian) in [(&session.guardian, true), (&session.child, false)] {
            let Some(identity) = identity else {
                continue;
            };
            if identity.pid == 0
                || identity.creation_time == 0
                || identity.executable_identity.len() > 128
                || if guardian || session.client == Client::Lazer {
                    identity.executable != session.launcher_executable
                        || identity.executable_identity != session.launcher_identity
                } else {
                    identity.executable != session.selected_executable
                        || identity.executable_identity != session.selected_identity
                }
            {
                return Err("工作階段程序所有權紀錄無效".into());
            }
        }
    }
    Ok(session)
}

fn read_ready(paths: &Paths, nonce: &str) -> Result<Option<Ready>> {
    read_json(&paths.ready(nonce))
}
fn validate_ready(signal: &Ready, nonce: &str) -> Result<()> {
    if signal.protocol != 1
        || signal.session != nonce
        || signal.status != "ready"
        || signal.endpoint != SERVER
        || signal.fixture
    {
        return Err("lazer 安全啟動訊號不符合正式私服契約".into());
    }
    Ok(())
}
fn cleanup_ready(paths: &Paths, nonce: &str) -> Result<()> {
    if let Some(ready) = read_ready(paths, nonce)? {
        validate_ready(&ready, nonce)?;
        let path = paths.ready(nonce);
        ordinary_file(&path)?;
        fs::remove_file(path).map_err(|_| "無法移除工作階段啟動訊號")?;
    }
    Ok(())
}

fn cleanup_session_ready(paths: &Paths, session: &Session) -> Result<()> {
    if !session.legacy_ready {
        return cleanup_ready(paths, &session.nonce);
    }
    let path = paths
        .legacy_data
        .join(format!("session-ready-{}.json", session.nonce));
    if let Some(ready) = read_json::<Ready>(&path)? {
        validate_ready(&ready, &session.nonce)?;
        ordinary_file(&path)?;
        fs::remove_file(path).map_err(|_| "無法移除舊版工作階段啟動訊號")?;
    }
    Ok(())
}

fn verify_bridge(paths: &Paths) -> Result<(PathBuf, String)> {
    let hash = option_env!("ILLYA_BRIDGE_SHA256")
        .ok_or("此啟動器缺少正式連線模組發行雜湊，暫不支援 lazer")?;
    verify_bridge_with_hash(paths, hash)
}
fn verify_bridge_with_hash(paths: &Paths, hash: &str) -> Result<(PathBuf, String)> {
    if !valid_nonce(hash) {
        return Err("正式連線模組雜湊格式無效".into());
    }
    let artifact = paths.bridge()?;
    if artifact_sha256(&artifact)? != hash {
        return Err("連線模組缺失或與正式發行雜湊不符".into());
    }
    Ok((artifact, hash.to_owned()))
}

fn verify_lazer_compatibility(paths: &Paths, selected: &Path) -> Result<()> {
    verify_bridge(paths)?;
    let directory = selected.parent().ok_or("lazer 缺少安裝資料夾")?;
    for (filename, expected) in [
        ("osu.Game.dll", SUPPORTED_GAME_SHA256),
        ("osu.Framework.dll", SUPPORTED_FRAMEWORK_SHA256),
        ("osu!.dll", SUPPORTED_ENTRY_SHA256),
    ] {
        if artifact_sha256(directory.join(filename))? != expected {
            return Err("這個 lazer 版本尚未通過相容性驗證；不會啟動或切回官方伺服器".into());
        }
    }
    Ok(())
}

fn validate_client(path: &Path, client: Client) -> Result<PathBuf> {
    let path = canonical_file(path)?;
    if !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("osu!.exe"))
    {
        return Err("請選擇 osu!.exe，而不是其他執行檔或捷徑".into());
    }
    validate_pe(&path, client)?;
    if client == Client::Lazer {
        let directory = path.parent().ok_or("lazer 缺少安裝資料夾")?;
        for filename in [
            "osu!.dll",
            "osu!.deps.json",
            "osu!.runtimeconfig.json",
            "osu.Game.dll",
            "osu.Framework.dll",
            "hostfxr.dll",
        ] {
            canonical_file(&directory.join(filename))
                .map_err(|_| format!("lazer 安裝資料夾缺少安全可讀取的 {filename}"))?;
        }
        validate_pe(&directory.join("hostfxr.dll"), Client::Lazer)?;
    }
    Ok(path)
}

fn validate_pe(path: &Path, client: Client) -> Result<()> {
    let mut file = File::open(path).map_err(|_| "無法讀取遊戲 PE 標頭")?;
    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)
        .map_err(|_| "執行檔缺少 DOS/PE 標頭")?;
    if &dos[..2] != b"MZ" {
        return Err("選擇的檔案不是 Windows PE 執行檔".into());
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().expect("four-byte PE offset"));
    if !(64..=1024 * 1024).contains(&offset) {
        return Err("Windows PE 標頭位置無效".into());
    }
    file.seek(SeekFrom::Start(u64::from(offset)))
        .map_err(|_| "無法讀取 PE 標頭")?;
    let mut pe = [0u8; 6];
    file.read_exact(&mut pe)
        .map_err(|_| "Windows PE 標頭已截斷")?;
    let machine = u16::from_le_bytes([pe[4], pe[5]]);
    if &pe[..4] != b"PE\0\0"
        || !matches!(machine, 0x14c | 0x8664)
        || client == Client::Lazer && machine != 0x8664
    {
        return Err("目前啟動器需要 x86 stable 或 x64 lazer 執行檔".into());
    }
    Ok(())
}

fn fresh_nonce() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "無法建立安全的工作階段識別碼")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn valid_nonce(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
}

fn hidden_child(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW; independent process lifetime.
    }
}

fn sanitize_injection_environment(command: &mut Command) {
    // Remove case-insensitive inherited CLR/profiler hooks. The mandatory host
    // sets its own STARTUP_HOOKS runtime property before running the application.
    for (name, _) in std::env::vars_os() {
        if is_injection_environment_name(&name.to_string_lossy()) {
            command.env_remove(name);
        }
    }
    command.env("DOTNET_ENABLEDIAGNOSTICS", "0");
}
fn is_injection_environment_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("COMPLUS_")
        || upper.starts_with("CORECLR_")
        || upper.starts_with("COR_")
        || upper.starts_with("DOTNET_ROOT")
        || matches!(
            upper.as_str(),
            "DOTNET_STARTUP_HOOKS"
                | "DOTNET_ADDITIONAL_DEPS"
                | "DOTNET_SHARED_STORE"
                | "DOTNET_DIAGNOSTICPORTS"
                | "DOTNET_MODIFIABLE_ASSEMBLIES"
                | "DOTNET_ENABLEDIAGNOSTICS"
                | "DOTNET_MULTILEVEL_LOOKUP"
                | "DOTNET_ROLL_FORWARD"
                | "DOTNET_ROLL_FORWARD_TO_PRERELEASE"
        )
}

struct ControlLock(File);
impl ControlLock {
    fn open(paths: &Paths) -> Result<Self> {
        let file = lock_file(&paths.control.join(".control.lock"))?;
        FileExt::lock_exclusive(&file).map_err(|_| "無法取得啟動器紀錄鎖")?;
        Ok(Self(file))
    }
}
impl Drop for ControlLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
fn exclusive_file(path: &Path) -> Result<File> {
    let file = lock_file(path)?;
    FileExt::try_lock_exclusive(&file).map_err(|_| "另一個啟動器正在更新工作階段，請稍後重試")?;
    Ok(file)
}
fn lock_file(path: &Path) -> Result<File> {
    if path_exists(path)? {
        ordinary_file(path)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| "無法開啟啟動器鎖定檔")?;
    check_metadata(&file.metadata().map_err(|error| error.to_string())?)?;
    // Control records are held only for brief atomic transitions. Guardian lock
    // uses the same API but is intentionally held for the whole session.
    Ok(file)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path_exists(path)? {
        return Ok(None);
    }
    ordinary_file(path)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "無法開啟啟動器紀錄")?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "無法讀取啟動器紀錄")?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("啟動器紀錄超過大小限制；現有檔案已保留".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "啟動器紀錄格式無效；現有檔案已保留".into())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let directory = path.parent().ok_or("紀錄缺少父資料夾")?;
    check_components(directory)?;
    if path_exists(path)? {
        ordinary_file(path)?;
    }
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("啟動器紀錄超過大小限制".into());
    }
    let temporary = directory.join(format!(
        ".illyaverse-record-{}.tmp",
        Uuid::new_v4().hyphenated()
    ));
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "無法建立啟動器紀錄暫存檔")?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "無法同步啟動器紀錄")?;
    drop(file);
    replace_record(&temporary, path).map_err(|_| "無法原子更新啟動器紀錄")?;
    #[cfg(unix)]
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| "無法同步紀錄資料夾")?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_record(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}
#[cfg(windows)]
fn replace_record(source: &Path, destination: &Path) -> std::io::Result<()> {
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
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn path_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("無法檢查啟動器路徑".into()),
    }
}
fn check_metadata(metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() {
        return Err("啟動器不接受 symlink 路徑".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("啟動器不接受 Windows reparse point 路徑".into());
        }
    }
    Ok(())
}
fn check_components(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("啟動器路徑必須是沒有 traversal 的絕對路徑".into());
    }
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        check_metadata(&fs::symlink_metadata(ancestor).map_err(|_| "無法檢查啟動器路徑組件")?)?;
    }
    Ok(())
}
fn ordinary_file(path: &Path) -> Result<()> {
    check_components(path)?;
    if !fs::symlink_metadata(path)
        .map_err(|_| "無法檢查檔案")?
        .is_file()
    {
        return Err("啟動器需要一般檔案".into());
    }
    Ok(())
}
fn canonical_file(path: &Path) -> Result<PathBuf> {
    ordinary_file(path)?;
    fs::canonicalize(path).map_err(|_| "無法解析一般檔案路徑".into())
}
fn ensure_directory(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("資料資料夾路徑必須是絕對路徑".into());
    }
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if !path_exists(ancestor)? {
            fs::create_dir(ancestor).map_err(|_| "無法建立 Illyaverse 私有資料夾")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(ancestor, fs::Permissions::from_mode(0o700))
                    .map_err(|error| error.to_string())?;
            }
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| "無法檢查資料夾")?;
        check_metadata(&metadata)?;
        if !metadata.is_dir() {
            return Err("Illyaverse 資料路徑不是資料夾".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture {
        directory: PathBuf,
        paths: Paths,
        client: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "kaleidoscope-runtime-test-{}",
                Uuid::new_v4().hyphenated()
            ));
            fs::create_dir(&directory).unwrap();
            let directory = fs::canonicalize(directory).unwrap();
            let control = directory.join("control");
            fs::create_dir(&control).unwrap();
            let package = directory.join("package");
            fs::create_dir(&package).unwrap();
            let launcher = package.join("kaleidoscope.fixture.exe");
            fs::write(&launcher, b"launcher identity fixture").unwrap();
            let client_root = directory.join("game");
            fs::create_dir(&client_root).unwrap();
            let client = client_root.join("osu!.exe");
            write_pe(&client, 0x8664);
            let paths = Paths {
                control,
                roaming: directory.join("roaming"),
                legacy_data: directory.join("private-lazer"),
                launcher,
            };
            fs::create_dir(&paths.roaming).unwrap();
            fs::create_dir(paths.roaming.join("osu")).unwrap();
            Self {
                directory,
                paths,
                client,
            }
        }
        fn bridge(&self) -> (PathBuf, String) {
            let bridge = self.paths.bridge().unwrap();
            fs::write(&bridge, b"owned startup bridge fixture").unwrap();
            let hash = artifact_sha256(&bridge).unwrap();
            (bridge, hash)
        }
        fn upgraded_paths(&self) -> Paths {
            let mut paths = self.paths.clone();
            let package = self.directory.join("upgraded-package");
            fs::create_dir(&package).unwrap();
            paths.launcher = package.join("kaleidoscope.fixture.exe");
            fs::write(&paths.launcher, b"upgraded launcher identity fixture").unwrap();
            paths
        }
        fn layout(&self) {
            let root = self.client.parent().unwrap();
            for filename in ["osu!.dll", "osu.Game.dll", "osu.Framework.dll"] {
                fs::write(root.join(filename), b"managed assembly fixture").unwrap();
            }
            for filename in ["osu!.deps.json", "osu!.runtimeconfig.json"] {
                fs::write(root.join(filename), b"{}").unwrap();
            }
            write_pe(&root.join("hostfxr.dll"), 0x8664);
        }
        #[cfg(windows)]
        fn live_paths(&self) -> Paths {
            let mut paths = self.paths.clone();
            paths.launcher = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
            paths
        }
        fn session(&self, paths: &Paths) -> Session {
            Session {
                version: 2,
                nonce: fresh_nonce().unwrap(),
                client: Client::Lazer,
                selected_executable: self.client.clone(),
                selected_identity: "fixture-selected-id".into(),
                launcher_executable: paths.launcher.clone(),
                launcher_identity: "fixture-launcher-id".into(),
                guardian: None,
                child: None,
                install_session: None,
                library: Some(resolve_library(paths, &self.client).unwrap()),
                legacy_ready: false,
                phase: Phase::Preparing,
                detail: "fixture preparing".into(),
                stop_requested: false,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let directory = fs::canonicalize(&self.directory).unwrap();
            let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
            assert_eq!(directory.parent(), Some(temporary.as_path()));
            assert!(directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("kaleidoscope-runtime-test-"));
            fs::remove_dir_all(directory).unwrap();
        }
    }
    fn write_pe(path: &Path, machine: u16) {
        let mut bytes = [0u8; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        bytes[68..70].copy_from_slice(&machine.to_le_bytes());
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn snapshot_contract_has_exact_public_fields_and_no_server_selector() {
        let fixture = Fixture::new();
        let snapshot = snapshot_at(&fixture.paths).unwrap();
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "stable":{"path":null,"running":false},
                "lazer":{"path":null,"running":false,"compatibility":"unknown"},
                "phase":"idle","detail":"選擇遊戲安裝路徑以連線至 Illyaverse","pluginPresent":false
            })
        );
        assert_eq!(
            serde_json::to_value(Phase::CleanupFailed).unwrap(),
            "cleanup_failed"
        );
        assert_eq!(serde_json::to_value(Client::Lazer).unwrap(), "lazer");
        assert!(
            handle_internal(&["__host-lazer".into(), "../../outside".into()])
                .unwrap()
                .is_err()
        );
        assert!(handle_internal(&["help".into()]).is_none());
    }

    #[test]
    fn production_ready_contract_rejects_test_fixture_and_cross_server_signals() {
        let nonce = fresh_nonce().unwrap();
        assert!(valid_nonce(&nonce));
        assert_ne!(nonce, fresh_nonce().unwrap());
        let normal = json!({"protocol":1,"session":nonce,"status":"ready","endpoint":SERVER,"fixture":false});
        let signal: Ready = serde_json::from_value(normal.clone()).unwrap();
        validate_ready(&signal, &nonce).unwrap();
        for (key, value) in [
            ("fixture", json!(true)),
            ("protocol", json!(2)),
            ("endpoint", json!("https://osu.ppy.sh")),
            ("session", json!("0".repeat(64))),
            ("status", json!("pending")),
        ] {
            let mut wrong = normal.clone();
            wrong[key] = value;
            assert!(validate_ready(&serde_json::from_value(wrong).unwrap(), &nonce).is_err());
        }
        let mut unknown = normal;
        unknown["extra"] = json!(true);
        assert!(serde_json::from_value::<Ready>(unknown).is_err());
    }

    #[test]
    fn recovery_uses_recorded_library_even_after_preferences_and_selector_change() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        let library = resolve_library(paths, &fixture.client).unwrap();
        ensure_directory(&library.rulesets()).unwrap();
        fs::write(library.root.join("client.realm"), b"realm marker").unwrap();
        fs::create_dir(library.root.join("files")).unwrap();
        fs::write(library.root.join("files/map.fixture"), b"resource marker").unwrap();
        fs::write(library.root.join("game.ini"), b"official config marker").unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut manager = ManagedRuleset::open(library.rulesets()).unwrap();
        let receipt = manager.install(bridge, &hash).unwrap();
        drop(manager);
        let mut record = fixture.session(paths);
        record.install_session = Some(receipt.session_id);
        write_json(&paths.session(), &record).unwrap();
        let different = fixture.directory.join("different-library");
        fs::create_dir(&different).unwrap();
        fs::create_dir(different.join("rulesets")).unwrap();
        let foreign = different.join("rulesets").join(RULESET_FILENAME);
        fs::write(&foreign, b"foreign DLL must remain").unwrap();
        fs::write(
            library.root.join("storage.ini"),
            format!("FullPath = {}", different.display()),
        )
        .unwrap();
        write_json(&paths.prefs(), &Preferences::default()).unwrap();
        recover_at(paths).unwrap();
        assert!(!receipt.installed_path.exists());
        assert_eq!(fs::read(foreign).unwrap(), b"foreign DLL must remain");
        assert_eq!(
            fs::read(library.root.join("client.realm")).unwrap(),
            b"realm marker"
        );
        assert_eq!(
            fs::read(library.root.join("files/map.fixture")).unwrap(),
            b"resource marker"
        );
        assert_eq!(
            fs::read(library.root.join("game.ini")).unwrap(),
            b"official config marker"
        );
        assert_eq!(read_session(paths).unwrap().unwrap().phase, Phase::Idle);
    }

    #[test]
    fn recovery_preserves_owned_artifact_when_library_parent_identity_changes() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        let library = resolve_library(paths, &fixture.client).unwrap();
        ensure_directory(&library.rulesets()).unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut manager = ManagedRuleset::open(library.rulesets()).unwrap();
        let receipt = manager.install(bridge, &hash).unwrap();
        drop(manager);
        let mut record = fixture.session(paths);
        record.install_session = Some(receipt.session_id);
        write_json(&paths.session(), &record).unwrap();
        let displaced = fixture.directory.join("displaced-library");
        fs::rename(&library.root, &displaced).unwrap();
        fs::create_dir(&library.root).unwrap();
        // Even preserving the inner rulesets directory ID cannot authorize a
        // replacement library root. The separate session root ID must win.
        fs::rename(displaced.join("rulesets"), library.rulesets()).unwrap();
        assert!(recover_at(paths).unwrap_err().contains("身分已變更"));
        assert!(receipt.installed_path.exists());
        assert!(receipt.journal_path.exists());
        assert!(paths.session().exists());
    }

    #[test]
    fn cleanup_recovers_install_publication_before_receipt_persistence() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        let record = fixture.session(paths);
        write_json(&paths.session(), &record).unwrap();
        let library = record.library.as_ref().unwrap();
        ensure_directory(&library.rulesets()).unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut manager = ManagedRuleset::open(library.rulesets()).unwrap();
        let receipt = manager.install(bridge, &hash).unwrap();
        drop(manager);
        // Crash happened before the receipt UUID reached the session journal.
        assert!(record.install_session.is_none());
        recover_at(paths).unwrap();
        assert!(!receipt.installed_path.exists());
        assert!(!receipt.journal_path.exists());
    }

    #[test]
    fn legacy_v1_recovery_is_fixed_to_original_private_library_and_ready_path() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        fs::create_dir(&paths.legacy_data).unwrap();
        fs::create_dir(paths.legacy_data.join("rulesets")).unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut manager = ManagedRuleset::open(paths.legacy_data.join("rulesets")).unwrap();
        let receipt = manager.install(bridge, &hash).unwrap();
        drop(manager);
        let mut record = fixture.session(paths);
        record.version = 1;
        record.library = None;
        record.install_session = Some(receipt.session_id);
        write_json(&paths.session(), &record).unwrap();
        let signal = json!({"protocol":1,"session":record.nonce,"status":"ready","endpoint":SERVER,"fixture":false});
        let legacy_ready = paths
            .legacy_data
            .join(format!("session-ready-{}.json", record.nonce));
        write_json(&legacy_ready, &signal).unwrap();
        let modern_root = resolve_library(paths, &fixture.client).unwrap();
        ensure_directory(&modern_root.rulesets()).unwrap();
        let untouched = modern_root.rulesets().join(RULESET_FILENAME);
        fs::write(&untouched, b"foreign shared library DLL").unwrap();
        write_json(
            &paths.prefs(),
            &Preferences {
                stable: None,
                lazer: Some(fixture.client.clone()),
            },
        )
        .unwrap();
        recover_at(paths).unwrap();
        assert!(!receipt.installed_path.exists());
        assert!(!legacy_ready.exists());
        assert_eq!(fs::read(untouched).unwrap(), b"foreign shared library DLL");
        let migrated = read_session(paths).unwrap().unwrap();
        assert_eq!(migrated.version, 2);
        assert!(!migrated.legacy_ready);
        assert_eq!(migrated.phase, Phase::Idle);
    }

    #[test]
    fn new_ready_path_is_control_only_and_completed_records_allow_library_moves() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        let mut record = fixture.session(paths);
        let library = record.library.as_ref().unwrap();
        let signal = json!({"protocol":1,"session":record.nonce,"status":"ready","endpoint":SERVER,"fixture":false});
        assert_eq!(
            paths.ready(&record.nonce).parent(),
            Some(paths.control.as_path())
        );
        write_json(&paths.ready(&record.nonce), &signal).unwrap();
        fs::write(
            library.root.join("session-ready-unrelated.json"),
            b"unrelated marker",
        )
        .unwrap();
        cleanup_session_ready(paths, &record).unwrap();
        assert!(!paths.ready(&record.nonce).exists());
        assert_eq!(
            fs::read(library.root.join("session-ready-unrelated.json")).unwrap(),
            b"unrelated marker"
        );
        record.phase = Phase::Idle;
        write_json(&paths.session(), &record).unwrap();
        fs::rename(&library.root, fixture.directory.join("moved-library")).unwrap();
        recover_at(paths).unwrap();
        assert_eq!(read_session(paths).unwrap().unwrap().phase, Phase::Idle);
    }

    #[test]
    fn completed_idle_session_allows_package_move_for_status_and_recovery() {
        for client in [Client::Stable, Client::Lazer] {
            let fixture = Fixture::new();
            let original = &fixture.paths;
            let mut record = fixture.session(original);
            record.client = client;
            record.phase = Phase::Idle;
            if client == Client::Stable {
                record.library = None;
            }
            write_json(&original.session(), &record).unwrap();
            let upgraded = fixture.upgraded_paths();
            fs::remove_file(&original.launcher).unwrap();

            assert_eq!(snapshot_at(&upgraded).unwrap().phase, Phase::Idle);
            recover_at(&upgraded).unwrap();
            let retained = matching_session(&upgraded, &record.nonce).unwrap();
            assert_eq!(retained.phase, Phase::Idle);
            assert_eq!(retained.launcher_executable, original.launcher);
            assert_eq!(retained.launcher_identity, record.launcher_identity);
            assert!(session_completed(&retained));
        }
    }

    #[test]
    fn upgraded_package_recovers_v2_owned_install_and_control_ready_signal() {
        let fixture = Fixture::new();
        let original = &fixture.paths;
        let mut record = fixture.session(original);
        let library = record.library.as_ref().unwrap();
        ensure_directory(&library.rulesets()).unwrap();
        fs::write(library.root.join("client.realm"), b"shared realm marker").unwrap();
        fs::write(library.root.join("game.ini"), b"official config marker").unwrap();
        let foreign = library.rulesets().join("foreign.ruleset.dll");
        fs::write(&foreign, b"foreign ruleset marker").unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut managed = ManagedRuleset::open(library.rulesets()).unwrap();
        let receipt = managed.install(bridge, &hash).unwrap();
        drop(managed);
        record.install_session = Some(receipt.session_id);
        record.phase = Phase::CleanupFailed;
        write_json(&original.session(), &record).unwrap();
        let signal = json!({"protocol":1,"session":record.nonce,"status":"ready","endpoint":SERVER,"fixture":false});
        write_json(&original.ready(&record.nonce), &signal).unwrap();
        let upgraded = fixture.upgraded_paths();
        fs::remove_file(&original.launcher).unwrap();

        assert!(snapshot_at(&upgraded).unwrap().plugin_present);
        recover_at(&upgraded).unwrap();
        assert!(!receipt.installed_path.exists());
        assert!(!receipt.journal_path.exists());
        assert!(!original.ready(&record.nonce).exists());
        assert_eq!(fs::read(foreign).unwrap(), b"foreign ruleset marker");
        assert_eq!(
            fs::read(library.root.join("client.realm")).unwrap(),
            b"shared realm marker"
        );
        assert_eq!(
            fs::read(library.root.join("game.ini")).unwrap(),
            b"official config marker"
        );
        assert_eq!(read_session(&upgraded).unwrap().unwrap().phase, Phase::Idle);
    }

    #[test]
    fn upgraded_package_recovers_v1_only_from_its_historical_library() {
        let fixture = Fixture::new();
        let original = &fixture.paths;
        ensure_directory(&original.legacy_data.join("rulesets")).unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut managed = ManagedRuleset::open(original.legacy_data.join("rulesets")).unwrap();
        let receipt = managed.install(bridge, &hash).unwrap();
        drop(managed);
        let mut record = fixture.session(original);
        let shared = record.library.as_ref().unwrap();
        ensure_directory(&shared.rulesets()).unwrap();
        let untouched = shared.rulesets().join(RULESET_FILENAME);
        fs::write(&untouched, b"foreign shared library DLL").unwrap();
        record.version = 1;
        record.library = None;
        record.install_session = Some(receipt.session_id);
        write_json(&original.session(), &record).unwrap();
        let signal = json!({"protocol":1,"session":record.nonce,"status":"ready","endpoint":SERVER,"fixture":false});
        let ready = original
            .legacy_data
            .join(format!("session-ready-{}.json", record.nonce));
        write_json(&ready, &signal).unwrap();
        let upgraded = fixture.upgraded_paths();
        fs::remove_file(&original.launcher).unwrap();

        recover_at(&upgraded).unwrap();
        assert!(!receipt.installed_path.exists());
        assert!(!receipt.journal_path.exists());
        assert!(!ready.exists());
        assert_eq!(fs::read(untouched).unwrap(), b"foreign shared library DLL");
        let migrated = read_session(&upgraded).unwrap().unwrap();
        assert_eq!(migrated.version, 2);
        assert_eq!(migrated.phase, Phase::Idle);
        assert_eq!(migrated.launcher_executable, original.launcher);
        assert!(!migrated.legacy_ready);
    }

    #[test]
    fn malformed_library_and_cross_library_install_session_preserve_artifacts() {
        let fixture = Fixture::new();
        let paths = &fixture.paths;
        let mut record = fixture.session(paths);
        let library = record.library.as_ref().unwrap();
        ensure_directory(&library.rulesets()).unwrap();
        let (bridge, hash) = fixture.bridge();
        let mut manager = ManagedRuleset::open(library.rulesets()).unwrap();
        let receipt = manager.install(bridge, &hash).unwrap();
        drop(manager);
        record.install_session = Some(Uuid::new_v4());
        write_json(&paths.session(), &record).unwrap();
        assert!(recover_at(paths).unwrap_err().contains("工作階段身分不符"));
        assert!(receipt.installed_path.exists());
        let original = serde_json::to_value(&record).unwrap();
        for invalid in [
            json!({"root":"relative", "identity":"unknown"}),
            json!({"root":library.root, "identity":""}),
            json!({"root":library.root,"identity":library.identity,"deletePath":fixture.directory}),
        ] {
            let mut value = original.clone();
            value["library"] = invalid;
            write_json(&paths.session(), &value).unwrap();
            assert!(read_session(paths).is_err());
            assert!(receipt.installed_path.exists());
        }
        let mut legacy = original;
        legacy["version"] = json!(1);
        write_json(&paths.session(), &legacy).unwrap();
        assert!(read_session(paths).is_err());
    }

    #[test]
    fn legacy_preinstallation_crash_can_recover_but_new_session_requires_library() {
        let fixture = Fixture::new();
        let mut record = fixture.session(&fixture.paths);
        record.library = None;
        write_json(&fixture.paths.session(), &record).unwrap();
        assert!(read_session(&fixture.paths).is_err());
        record.version = 1;
        write_json(&fixture.paths.session(), &record).unwrap();
        recover_at(&fixture.paths).unwrap();
        assert_eq!(
            read_session(&fixture.paths).unwrap().unwrap().phase,
            Phase::Idle
        );
        assert!(!fixture.paths.legacy_data.exists());
    }

    #[test]
    fn preferences_and_session_parsing_are_bounded_and_cannot_add_commands_or_paths() {
        let fixture = Fixture::new();
        write_json(
            &fixture.paths.prefs(),
            &json!({"stable":null,"lazer":null,"server":"https://elsewhere.test"}),
        )
        .unwrap();
        assert!(read_preferences(&fixture.paths).is_err());
        fs::write(
            fixture.paths.prefs(),
            vec![b' '; MAX_RECORD_BYTES as usize + 1],
        )
        .unwrap();
        assert!(read_preferences(&fixture.paths).is_err());
        let mut session = serde_json::to_value(fixture.session(&fixture.paths)).unwrap();
        session["launcherExecutable"] = json!("relative-launcher.exe");
        write_json(&fixture.paths.session(), &session).unwrap();
        assert!(read_session(&fixture.paths).is_err());
        session = serde_json::to_value(fixture.session(&fixture.paths)).unwrap();
        session["selectedExecutable"] = json!(fixture.directory.join("foreign.exe"));
        write_json(&fixture.paths.session(), &session).unwrap();
        assert!(read_session(&fixture.paths).is_err());
        assert!(fixture.paths.session().exists());
    }

    #[test]
    fn executable_pe_architecture_layout_and_release_hash_are_required() {
        let fixture = Fixture::new();
        validate_client(&fixture.client, Client::Stable).unwrap();
        assert!(validate_client(&fixture.client, Client::Lazer).is_err());
        fixture.layout();
        validate_client(&fixture.client, Client::Lazer).unwrap();
        let (bridge, hash) = fixture.bridge();
        verify_bridge_with_hash(&fixture.paths, &hash).unwrap();
        assert!(verify_bridge_with_hash(&fixture.paths, &"0".repeat(64)).is_err());
        assert!(verify_bridge_with_hash(&fixture.paths, "not-a-hash").is_err());
        assert!(verify_lazer_compatibility(&fixture.paths, &fixture.client).is_err());
        fs::write(&bridge, b"tampered bridge").unwrap();
        assert!(verify_bridge_with_hash(&fixture.paths, &hash).is_err());
        write_pe(&fixture.client, 0x14c);
        validate_client(&fixture.client, Client::Stable).unwrap();
        assert!(validate_client(&fixture.client, Client::Lazer).is_err());
        fs::write(&fixture.client, b"not PE").unwrap();
        assert!(validate_client(&fixture.client, Client::Stable).is_err());
    }

    #[test]
    fn an_install_journal_without_process_ownership_is_preserved() {
        let fixture = Fixture::new();
        write_json(
            &fixture.paths.prefs(),
            &Preferences {
                stable: None,
                lazer: Some(fixture.client.clone()),
            },
        )
        .unwrap();
        let (bridge, hash) = fixture.bridge();
        ensure_directory(
            &resolve_library(&fixture.paths, &fixture.client)
                .unwrap()
                .rulesets(),
        )
        .unwrap();
        let mut installation = ManagedRuleset::open(
            resolve_library(&fixture.paths, &fixture.client)
                .unwrap()
                .rulesets(),
        )
        .unwrap();
        let receipt = installation.install(bridge, &hash).unwrap();
        drop(installation);
        assert!(recover_at(&fixture.paths)
            .unwrap_err()
            .contains("程序所有權"));
        assert!(receipt.installed_path.exists());
        assert!(receipt.journal_path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn upgraded_launcher_refuses_live_child_and_handles_pid_reuse_without_closing_it() {
        let fixture = Fixture::new();
        let paths = fixture.live_paths();
        let (bridge, hash) = fixture.bridge();
        ensure_directory(&resolve_library(&paths, &fixture.client).unwrap().rulesets()).unwrap();
        let mut managed =
            ManagedRuleset::open(resolve_library(&paths, &fixture.client).unwrap().rulesets())
                .unwrap();
        let receipt = managed.install(bridge, &hash).unwrap();
        drop(managed);
        let own = process::current_identity().unwrap();
        let mut record = fixture.session(&paths);
        record.launcher_identity = own.executable_identity.clone();
        record.child = Some(own.clone());
        record.install_session = Some(receipt.session_id);
        record.phase = Phase::Running;
        write_json(&paths.session(), &record).unwrap();
        let upgraded = fixture.upgraded_paths();
        assert_eq!(snapshot_at(&upgraded).unwrap().phase, Phase::Running);
        assert!(recover_at(&upgraded).unwrap_err().contains("仍在執行"));
        assert!(receipt.installed_path.exists());
        // Same PID, different creation time proves the recorded child is gone.
        record.child.as_mut().unwrap().creation_time += 1;
        write_json(&paths.session(), &record).unwrap();
        recover_at(&upgraded).unwrap();
        assert!(!receipt.installed_path.exists());
        assert_eq!(read_session(&upgraded).unwrap().unwrap().phase, Phase::Idle);
        assert_eq!(process::state(&own).unwrap(), process::State::Live);
    }

    #[cfg(windows)]
    #[test]
    fn upgraded_launcher_preserves_live_guardian_and_ambiguous_process_identity() {
        let fixture = Fixture::new();
        let paths = fixture.live_paths();
        let own = process::current_identity().unwrap();
        let mut record = fixture.session(&paths);
        record.launcher_identity = own.executable_identity.clone();
        record.guardian = Some(own);
        write_json(&paths.session(), &record).unwrap();
        let upgraded = fixture.upgraded_paths();
        assert!(recover_at(&upgraded).unwrap_err().contains("仍在執行"));
        // An internally consistent record with a changed image identity remains ambiguous.
        record.launcher_identity.push_str("-changed");
        record.guardian.as_mut().unwrap().executable_identity = record.launcher_identity.clone();
        write_json(&paths.session(), &record).unwrap();
        assert!(recover_at(&upgraded).unwrap_err().contains("無法確認"));
        assert!(paths.session().exists());
    }

    #[cfg(windows)]
    #[test]
    fn upgraded_package_cannot_take_over_a_previous_guardian_launch() {
        let fixture = Fixture::new();
        let record = fixture.session(&fixture.paths);
        write_json(&fixture.paths.session(), &record).unwrap();
        let before = fs::read(fixture.paths.session()).unwrap();
        let upgraded = fixture.upgraded_paths();
        assert!(matching_session(&upgraded, &record.nonce).is_ok());
        assert!(run_guardian(&upgraded, &record.nonce)
            .unwrap_err()
            .contains("守護程序並非此工作階段的啟動器"));
        assert_eq!(fs::read(fixture.paths.session()).unwrap(), before);
        assert!(read_session(&upgraded).unwrap().unwrap().guardian.is_none());
    }

    #[test]
    fn control_lock_serializes_concurrent_record_transitions() {
        let fixture = Fixture::new();
        let paths = fixture.paths.clone();
        let lock = ControlLock::open(&paths).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let other = thread::spawn(move || {
            let _lock = ControlLock::open(&paths).unwrap();
            send.send(()).unwrap();
        });
        assert!(receive.recv_timeout(Duration::from_millis(30)).is_err());
        drop(lock);
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        other.join().unwrap();
    }

    #[test]
    fn inherited_clr_hooks_are_removed_case_insensitively() {
        for name in [
            "DOTNET_STARTUP_HOOKS",
            "dotnet_startup_hooks",
            "COR_ENABLE_PROFILING",
            "CORECLR_PROFILER_PATH",
            "COMPlus_ReadyToRun",
            "DOTNET_ROOT_X64",
            "DOTNET_ADDITIONAL_DEPS",
            "DOTNET_DiagnosticPorts",
        ] {
            assert!(is_injection_environment_name(name));
        }
        for name in [
            "LOCALAPPDATA",
            "PATH",
            "ILLYAVERSE_LAZER_SESSION",
            "OSU_DISABLE_ERROR_REPORTING",
        ] {
            assert!(!is_injection_environment_name(name));
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn linked_preferences_and_profile_roots_are_refused() {
        let fixture = Fixture::new();
        let target = fixture.directory.join("foreign.json");
        fs::write(&target, b"{\"stable\":null,\"lazer\":null}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, fixture.paths.prefs()).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&target, fixture.paths.prefs()).is_err() {
            eprintln!("SKIPPED runtime symlink fixture: missing Windows symlink privilege");
            return;
        }
        assert!(read_preferences(&fixture.paths).is_err());
        assert_eq!(
            fs::read(target).unwrap(),
            b"{\"stable\":null,\"lazer\":null}"
        );
        let profile_link = fixture.directory.join("profile-link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&fixture.paths.control, &profile_link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&fixture.paths.control, &profile_link).unwrap();
        assert!(ensure_directory(&profile_link.join("rulesets")).is_err());
        assert!(!fixture.paths.control.join("rulesets").exists());
    }
}
