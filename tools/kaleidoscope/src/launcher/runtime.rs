//! Fixed-server launcher sessions. An independent guardian owns installation,
//! client lifetime, and exact-file cleanup even after the visible UI closes.

use super::install::{artifact_sha256, ManagedRuleset, RULESET_FILENAME};
use super::{host, process};
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
    data: PathBuf,
    launcher: PathBuf,
}

impl Paths {
    fn production() -> Result<Self> {
        #[cfg(not(windows))]
        return Err("Kaleidoscope 原生啟動器目前僅支援 Windows".into());
        #[cfg(windows)]
        {
            let local = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .ok_or("無法找到目前使用者的 LOCALAPPDATA")?;
            check_components(&local)?;
            let base = local.join("Illyaverse");
            let control = base.join("Kaleidoscope");
            ensure_directory(&control)?;
            let control = fs::canonicalize(control).map_err(|error| error.to_string())?;
            let data = base.join("lazer");
            let launcher =
                canonical_file(&std::env::current_exe().map_err(|error| error.to_string())?)?;
            Ok(Self {
                control,
                data,
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
    fn rulesets(&self) -> PathBuf {
        self.data.join("rulesets")
    }
    fn ready(&self, nonce: &str) -> PathBuf {
        self.data.join(format!("session-ready-{nonce}.json"))
    }
    fn bridge(&self) -> Result<PathBuf> {
        Ok(self
            .launcher
            .parent()
            .ok_or("啟動器缺少安裝資料夾")?
            .join(RULESET_FILENAME))
    }
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
        version: 1,
        nonce: nonce.clone(),
        client,
        selected_identity: process::executable_identity(&selected)?,
        selected_executable: selected,
        launcher_identity: process::executable_identity(&paths.launcher)?,
        launcher_executable: paths.launcher.clone(),
        guardian: None,
        child: None,
        install_session: None,
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
        plugin_present: path_exists(&paths.rulesets().join(RULESET_FILENAME))?,
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
    let rulesets = paths.rulesets();
    if rulesets.exists() {
        let mut managed = ManagedRuleset::open(&rulesets)?;
        let journal = managed.active_session()?;
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
    if let Some(mut session) = session {
        cleanup_ready(paths, &session.nonce)?;
        session.phase = Phase::Idle;
        session.detail = "已關閉遊戲並移除 Illyaverse 連線模組".into();
        session.guardian = None;
        session.child = None;
        session.install_session = None;
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
        Some(managed) => managed.cleanup().map(|_| ()),
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
            ensure_directory(&paths.rulesets())?;
            *managed = Some(ManagedRuleset::open(paths.rulesets())?);
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
            command.env("ILLYAVERSE_LAZER_DATA_ROOT", &paths.data);
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
    validate_child_environment(paths, nonce, install)?;
    let (_, hash) = verify_bridge(paths)?;
    let bridge = paths.rulesets().join(RULESET_FILENAME);
    if artifact_sha256(&bridge)? != hash {
        return Err("已安裝的連線模組與發行雜湊不符；不會進入 Main".into());
    }
    host::run_managed(
        &install.join("osu!.dll"),
        &install.join("hostfxr.dll"),
        &bridge,
    )
}

fn validate_child_environment(paths: &Paths, nonce: &str, install: &Path) -> Result<()> {
    for (name, expected) in [
        ("ILLYAVERSE_LAZER_INSTALL_ROOT", install.to_path_buf()),
        ("ILLYAVERSE_LAZER_DATA_ROOT", paths.data.clone()),
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
    let session: Option<Session> = read_json(&paths.session())?;
    if let Some(session) = &session {
        if session.version != 1
            || !valid_nonce(&session.nonce)
            || session.detail.len() > 4096
            || !session.selected_executable.is_absolute()
            || !session.launcher_executable.is_absolute()
            || session.selected_identity.len() > 128
            || session.launcher_identity.len() > 128
            || session.launcher_executable != paths.launcher
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
                data: directory.join("private-lazer"),
                launcher,
            };
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
                version: 1,
                nonce: fresh_nonce().unwrap(),
                client: Client::Lazer,
                selected_executable: self.client.clone(),
                selected_identity: "fixture-selected-id".into(),
                launcher_executable: paths.launcher.clone(),
                launcher_identity: "fixture-launcher-id".into(),
                guardian: None,
                child: None,
                install_session: None,
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
        session["launcherExecutable"] = json!(fixture.client);
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
        let (bridge, hash) = fixture.bridge();
        ensure_directory(&fixture.paths.rulesets()).unwrap();
        let mut installation = ManagedRuleset::open(fixture.paths.rulesets()).unwrap();
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
    fn recovery_refuses_live_child_and_handles_pid_reuse_without_closing_it() {
        let fixture = Fixture::new();
        let paths = fixture.live_paths();
        let (bridge, hash) = fixture.bridge();
        ensure_directory(&paths.rulesets()).unwrap();
        let mut managed = ManagedRuleset::open(paths.rulesets()).unwrap();
        let receipt = managed.install(bridge, &hash).unwrap();
        drop(managed);
        let own = process::current_identity().unwrap();
        let mut record = fixture.session(&paths);
        record.launcher_identity = own.executable_identity.clone();
        record.child = Some(own.clone());
        record.install_session = Some(receipt.session_id);
        record.phase = Phase::Running;
        write_json(&paths.session(), &record).unwrap();
        assert!(recover_at(&paths).unwrap_err().contains("仍在執行"));
        assert!(receipt.installed_path.exists());
        // Same PID, different creation time proves the recorded child is gone.
        record.child.as_mut().unwrap().creation_time += 1;
        write_json(&paths.session(), &record).unwrap();
        recover_at(&paths).unwrap();
        assert!(!receipt.installed_path.exists());
        assert_eq!(read_session(&paths).unwrap().unwrap().phase, Phase::Idle);
        assert_eq!(process::state(&own).unwrap(), process::State::Live);
    }

    #[cfg(windows)]
    #[test]
    fn live_guardian_and_ambiguous_process_identity_prevent_recovery() {
        let fixture = Fixture::new();
        let paths = fixture.live_paths();
        let own = process::current_identity().unwrap();
        let mut record = fixture.session(&paths);
        record.launcher_identity = own.executable_identity.clone();
        record.guardian = Some(own);
        write_json(&paths.session(), &record).unwrap();
        assert!(recover_at(&paths).is_err());
        // An internally consistent record with a changed image identity remains ambiguous.
        record.launcher_identity.push_str("-changed");
        record.guardian.as_mut().unwrap().executable_identity = record.launcher_identity.clone();
        write_json(&paths.session(), &record).unwrap();
        assert!(recover_at(&paths).unwrap_err().contains("無法確認"));
        assert!(paths.session().exists());
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
