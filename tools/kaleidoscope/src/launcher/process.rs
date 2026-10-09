//! Process ownership is an executable identity plus PID and creation time, never
//! a process name alone. Production shutdown uses WM_CLOSE and does not force kill.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub pid: u32,
    pub creation_time: u64,
    pub executable: PathBuf,
    pub executable_identity: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Live,
    Gone,
    /// The PID is still present but cannot safely be attributed to the record.
    Ambiguous,
}

pub fn current_identity() -> Result<Identity> {
    identity(std::process::id())?.ok_or("Cannot identify this launcher process".into())
}

pub fn identity(pid: u32) -> Result<Option<Identity>> {
    platform::identity(pid)
}

pub fn state(expected: &Identity) -> Result<State> {
    match identity(expected.pid)? {
        None => Ok(State::Gone),
        Some(actual) if actual.creation_time != expected.creation_time => Ok(State::Gone),
        Some(actual) if actual == *expected => Ok(State::Live),
        Some(_) => Ok(State::Ambiguous),
    }
}

pub fn find_by_executable(executable: &Path) -> Result<Vec<Identity>> {
    platform::find_by_executable(executable)
}

/// Refuse concurrent library users without closing another installation. A
/// matching filename is only a discovery hint; unreadable candidates prevent
/// launch rather than being treated as a safely unrelated stable client.
pub fn find_lazer_candidates() -> Result<Vec<Identity>> {
    platform::find_lazer_candidates()
}

pub fn close_gracefully(expected: &Identity, timeout: Duration) -> Result<()> {
    platform::close_gracefully(expected, timeout)
}

pub fn executable_identity(path: &Path) -> Result<String> {
    platform::executable_identity(path)
}

#[cfg(any(windows, test))]
fn looks_like_lazer(executable: &Path) -> Result<bool> {
    let install = executable
        .parent()
        .ok_or("Client candidate has no installation folder")?;
    for companion in ["osu!.dll", "osu.Game.dll", "osu.Framework.dll"] {
        match std::fs::symlink_metadata(install.join(companion)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot safely inspect another client installation".into()),
        }
    }
    Ok(false)
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(super) fn identity(_pid: u32) -> Result<Option<Identity>> {
        Err("Native launcher process ownership currently supports Windows only".into())
    }
    pub(super) fn find_by_executable(_path: &Path) -> Result<Vec<Identity>> {
        Err("Native launcher process discovery currently supports Windows only".into())
    }
    pub(super) fn find_lazer_candidates() -> Result<Vec<Identity>> {
        Err("Native launcher process discovery currently supports Windows only".into())
    }
    pub(super) fn close_gracefully(_identity: &Identity, _timeout: Duration) -> Result<()> {
        Err("Native launcher process shutdown currently supports Windows only".into())
    }
    pub(super) fn executable_identity(_path: &Path) -> Result<String> {
        Err("Native launcher executable ownership currently supports Windows only".into())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::fs::{self, OpenOptions};
    use std::mem::{size_of, MaybeUninit};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, FILETIME, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    fn open_process(pid: u32) -> Result<Option<Handle>> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            return match unsafe { GetLastError() } {
                // Invalid parameter means the original PID no longer exists.
                87 => Ok(None),
                code => Err(format!("Cannot safely inspect process ownership ({code})")),
            };
        }
        Ok(Some(Handle(handle)))
    }

    fn inspect(handle: &Handle, pid: u32) -> Result<Option<Identity>> {
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_OBJECT_0 => return Ok(None),
            WAIT_TIMEOUT => {}
            _ => return Err("Cannot inspect process lifetime".into()),
        }
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited: FILETIME = unsafe { std::mem::zeroed() };
        let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
        let mut user: FILETIME = unsafe { std::mem::zeroed() };
        if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err("Cannot inspect process creation time".into());
        }
        let mut name = vec![0u16; 32768];
        let mut length = name.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, name.as_mut_ptr(), &mut length) } == 0 {
            return Err("Cannot inspect process executable path".into());
        }
        name.truncate(length as usize);
        let path = PathBuf::from(
            String::from_utf16(&name).map_err(|_| "Executable path is not valid UTF-16")?,
        );
        let executable =
            fs::canonicalize(path).map_err(|_| "Process executable no longer resolves safely")?;
        let executable_identity = executable_identity(&executable)?;
        Ok(Some(Identity {
            pid,
            creation_time: (u64::from(created.dwHighDateTime) << 32)
                | u64::from(created.dwLowDateTime),
            executable,
            executable_identity,
        }))
    }

    pub(super) fn identity(pid: u32) -> Result<Option<Identity>> {
        let Some(handle) = open_process(pid)? else {
            return Ok(None);
        };
        inspect(&handle, pid)
    }

    pub(super) fn executable_identity(path: &Path) -> Result<String> {
        let metadata =
            fs::symlink_metadata(path).map_err(|_| "Cannot inspect executable identity")?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.file_attributes() & 0x400 != 0
        {
            return Err("An executable identity requires an ordinary non-linked file".into());
        }
        #[repr(C)]
        struct FileIdInfo {
            volume: u64,
            id: [u8; 16],
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetFileInformationByHandleEx(
                handle: *mut c_void,
                class: i32,
                information: *mut c_void,
                size: u32,
            ) -> i32;
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(0x00200000)
            .open(path)
            .map_err(|_| "Cannot open executable identity")?;
        if file
            .metadata()
            .map_err(|error| error.to_string())?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err("Executable became a reparse point".into());
        }
        let mut information = MaybeUninit::<FileIdInfo>::uninit();
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                18,
                information.as_mut_ptr().cast(),
                size_of::<FileIdInfo>() as u32,
            )
        } == 0
        {
            return Err("Cannot inspect the executable's 128-bit file identity".into());
        }
        let information = unsafe { information.assume_init() };
        let id: String = information
            .id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(format!("windows:{}:{id}", information.volume))
    }

    pub(super) fn find_by_executable(executable: &Path) -> Result<Vec<Identity>> {
        let expected_path =
            fs::canonicalize(executable).map_err(|_| "Selected executable does not resolve")?;
        let expected_id = executable_identity(&expected_path)?;
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("Cannot enumerate client processes".into());
        }
        let snapshot = Handle(snapshot);
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        let expected_basename = expected_path
            .file_name()
            .ok_or("Selected executable has no filename")?
            .to_string_lossy();
        let mut results = Vec::new();
        while available {
            let length = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let basename = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if basename.eq_ignore_ascii_case(&expected_basename) {
                // Access-denied processes cannot be identified, so they are never closed.
                if let Ok(Some(candidate)) = identity(entry.th32ProcessID) {
                    if candidate.executable == expected_path
                        && candidate.executable_identity == expected_id
                    {
                        results.push(candidate);
                    }
                }
            }
            available = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(results)
    }

    pub(super) fn find_lazer_candidates() -> Result<Vec<Identity>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("Cannot enumerate client processes".into());
        }
        let snapshot = Handle(snapshot);
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut available = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        let mut results = Vec::new();
        while available {
            let length = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let basename = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if basename.eq_ignore_ascii_case("osu!.exe") {
                // Do not silently ignore an access-denied client using the same
                // Realm. Nothing here sends a close message to these candidates.
                if let Some(candidate) = identity(entry.th32ProcessID)? {
                    if looks_like_lazer(&candidate.executable)? {
                        results.push(candidate);
                    }
                }
            }
            available = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(results)
    }

    struct CloseWindows {
        pid: u32,
        posted: usize,
    }
    unsafe extern "system" fn close_window(window: HWND, argument: LPARAM) -> i32 {
        let context = unsafe { &mut *(argument as *mut CloseWindows) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
        }
        if pid == context.pid && unsafe { PostMessageW(window, WM_CLOSE, 0, 0) } != 0 {
            context.posted += 1;
        }
        1
    }

    pub(super) fn close_gracefully(expected: &Identity, timeout: Duration) -> Result<()> {
        let Some(handle) = open_process(expected.pid)? else {
            return Ok(());
        };
        let Some(actual) = inspect(&handle, expected.pid)? else {
            return Ok(());
        };
        if actual != *expected {
            return Err("Process identity changed; no close message was sent".into());
        }
        // Keep the validated process handle alive throughout window discovery and wait.
        let mut context = CloseWindows {
            pid: expected.pid,
            posted: 0,
        };
        if unsafe {
            EnumWindows(
                Some(close_window),
                &mut context as *mut CloseWindows as LPARAM,
            )
        } == 0
        {
            return Err("Cannot enumerate windows for the selected client".into());
        }
        if context.posted == 0 {
            if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
                return Ok(());
            }
            return Err(
                "The selected client has no closeable window; its process was preserved".into(),
            );
        }
        let milliseconds = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        match unsafe { WaitForSingleObject(handle.0, milliseconds) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => {
                Err("The selected client did not close in time; it was not force killed".into())
            }
            _ => Err("Cannot confirm that the selected client closed".into()),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn creation_time_and_executable_identity_bind_process_ownership() {
        let own = current_identity().unwrap();
        assert_eq!(state(&own).unwrap(), State::Live);
        let mut reused_pid = own.clone();
        reused_pid.creation_time += 1;
        assert_eq!(state(&reused_pid).unwrap(), State::Gone);
        let mut foreign_file = own.clone();
        foreign_file.executable_identity.push_str("-foreign");
        assert_eq!(state(&foreign_file).unwrap(), State::Ambiguous);
        assert!(close_gracefully(&foreign_file, Duration::ZERO)
            .unwrap_err()
            .contains("identity"));
        assert_eq!(state(&own).unwrap(), State::Live);
    }

    #[test]
    fn candidate_detection_distinguishes_stable_and_managed_installations() {
        let directory = std::env::temp_dir().join(format!(
            "kaleidoscope-process-test-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        std::fs::create_dir(&directory).unwrap();
        let directory = std::fs::canonicalize(directory).unwrap();
        let executable = directory.join("osu!.exe");
        std::fs::write(&executable, b"fixture, never executed").unwrap();
        assert!(!looks_like_lazer(&executable).unwrap());
        for companion in ["osu!.dll", "osu.Game.dll", "osu.Framework.dll"] {
            let path = directory.join(companion);
            std::fs::write(&path, b"assembly metadata fixture").unwrap();
            assert!(looks_like_lazer(&executable).unwrap());
            std::fs::remove_file(path).unwrap();
        }
        assert_eq!(
            std::fs::read(&executable).unwrap(),
            b"fixture, never executed"
        );
        let temporary = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(directory.parent(), Some(temporary.as_path()));
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("kaleidoscope-process-test-"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
