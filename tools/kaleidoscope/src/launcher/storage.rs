//! Read-only resolution of the installed client's existing library. Only the
//! storage selector is read: account config, Realm and resources are never read
//! or copied by the launcher. See the pinned upstream contracts in the guide.

use super::install::directory_identity;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_SELECTOR_BYTES: u64 = 16 * 1024;

/// Match Environment.GetFolderPath rather than an overridden APPDATA variable.
#[cfg(windows)]
pub(super) fn roaming_app_data() -> Result<PathBuf> {
    known_folder(windows_sys::core::GUID::from_u128(
        0x3eb685db_65f9_4cf6_a03a_e3ef65729f3d,
    ))
}

#[cfg(windows)]
pub(super) fn local_app_data() -> Result<PathBuf> {
    known_folder(windows_sys::core::GUID::from_u128(
        0xf1b32785_6fba_4fcf_9d55_7b8e7f157091,
    ))
}

#[cfg(windows)]
fn known_folder(identifier: windows_sys::core::GUID) -> Result<PathBuf> {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(
            identifier: *const windows_sys::core::GUID,
            flags: u32,
            token: *mut c_void,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(pointer: *mut c_void);
    }
    struct AllocatedPath(*mut u16);
    impl Drop for AllocatedPath {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(self.0.cast());
            }
        }
    }
    let mut raw = ptr::null_mut();
    // No KF_FLAG_CREATE: discovering a library must not create another one.
    let result = unsafe { SHGetKnownFolderPath(&identifier, 0, ptr::null_mut(), &mut raw) };
    let allocated = AllocatedPath(raw);
    if result < 0 || raw.is_null() {
        return Err("無法取得 Windows 使用者資料資料夾".into());
    }
    let length = (0..32768)
        .find(|index| unsafe { *allocated.0.add(*index) } == 0)
        .ok_or("Windows 使用者資料路徑超過長度限制")?;
    let path = PathBuf::from(OsString::from_wide(unsafe {
        std::slice::from_raw_parts(allocated.0, length)
    }));
    check_components(&path)?;
    Ok(path)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Library {
    pub root: PathBuf,
    pub identity: String,
}

impl Library {
    pub fn open(root: &Path) -> Result<Self> {
        check_components(root)?;
        if !fs::symlink_metadata(root)
            .map_err(|_| "無法檢查 lazer 圖譜資料夾")?
            .is_dir()
        {
            return Err("lazer 圖譜資料路徑不是資料夾".into());
        }
        let root = fs::canonicalize(root).map_err(|_| "無法解析 lazer 圖譜資料夾")?;
        let identity = directory_identity(&root)?;
        Ok(Self { root, identity })
    }

    pub fn validate(&self) -> Result<()> {
        let current = Self::open(&self.root)?;
        if current.root != self.root || current.identity != self.identity {
            return Err("lazer 圖譜資料夾身分已變更；檔案已保留".into());
        }
        Ok(())
    }

    pub fn rulesets(&self) -> PathBuf {
        self.root.join("rulesets")
    }

    pub fn validate_record(&self) -> Result<()> {
        if self.identity.is_empty()
            || self.identity.len() > 128
            || !self.root.is_absolute()
            || self
                .root
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err("圖譜資料夾身分紀錄無效；檔案已保留".into());
        }
        Ok(())
    }
}

/// osu-framework ab102608: Windows uses APPDATA/osu, except a framework.ini
/// marker in RuntimeInfo.StartupDirectory (the selected application's folder).
/// OsuStorage follows FullPath in that default folder's storage.ini. An invalid
/// custom location is an error here; silently creating a replacement would hide
/// the player's existing library.
pub(super) fn resolve(roaming: &Path, control: &Path, executable: &Path) -> Result<Library> {
    let install = executable.parent().ok_or("lazer 安裝路徑缺少父資料夾")?;
    check_components(install)?;
    let portable_marker = install.join("framework.ini");
    let default_root = if exists(&portable_marker)? {
        regular_file(&portable_marker)?;
        install.to_path_buf()
    } else {
        roaming.join("osu")
    };
    if !exists(&default_root)? {
        return Err("找不到現有 lazer 資料夾；請先正常啟動已安裝的 lazer 一次".into());
    }
    let default_library = Library::open(&default_root)?;
    let selector = default_library.root.join("storage.ini");
    let target = if exists(&selector)? {
        regular_file(&selector)?;
        let mut bytes = Vec::new();
        File::open(&selector)
            .map_err(|_| "無法讀取 lazer storage.ini 路徑設定")?
            .take(MAX_SELECTOR_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "無法讀取 lazer storage.ini 路徑設定")?;
        if bytes.len() as u64 > MAX_SELECTOR_BYTES {
            return Err("lazer storage.ini 超過路徑設定大小限制".into());
        }
        parse_target(&bytes)?
    } else {
        None
    };
    let library = match target {
        Some(target) => Library::open(&target)
            .map_err(|_| "自訂 lazer 資料夾無法安全使用；不會改用空白或另一個圖譜庫")?,
        None => default_library,
    };
    if library.root.starts_with(control) || control.starts_with(&library.root) {
        return Err("遊戲圖譜資料夾不能與啟動器控制資料夾重疊".into());
    }
    Ok(library)
}

fn parse_target(bytes: &[u8]) -> Result<Option<PathBuf>> {
    // Framework's StreamReader auto-detects a BOM. Stock files are UTF-8; also
    // accept UTF-16 selectors rather than accidentally choosing the default.
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 || bytes.starts_with(&[0xff, 0xfe, 0, 0]) {
            return Err("lazer storage.ini 的文字編碼無效或不支援".into());
        }
        let little_endian = bytes[0] == 0xff;
        let code_units = bytes[2..].chunks_exact(2).map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        });
        String::from_utf16(&code_units.collect::<Vec<_>>())
            .map_err(|_| "lazer storage.ini 的 UTF-16 路徑設定無效")?
    } else {
        std::str::from_utf8(bytes)
            .map_err(|_| "lazer storage.ini 不是支援的路徑設定編碼")?
            .trim_start_matches('\u{feff}')
            .to_owned()
    };
    let mut result = None;
    for line in text.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "FullPath" {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            result = None;
            continue;
        }
        let target = PathBuf::from(value);
        if value.contains('\0')
            || !target.is_absolute()
            || target
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err("lazer 自訂資料路徑必須是沒有 traversal 的絕對路徑".into());
        }
        result = Some(target);
    }
    Ok(result)
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("無法檢查 lazer 路徑設定".into()),
    }
}

fn check_components(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("lazer 圖譜資料路徑必須是沒有 traversal 的絕對路徑".into());
    }
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| "無法檢查 lazer 路徑組件")?;
        if metadata.file_type().is_symlink() {
            return Err("lazer 資料路徑不接受 symlink".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("lazer 資料路徑不接受 Windows reparse point".into());
            }
        }
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<()> {
    check_components(path)?;
    if !fs::symlink_metadata(path)
        .map_err(|_| "無法檢查 lazer 路徑設定檔")?
        .is_file()
    {
        return Err("lazer 路徑設定必須是一般檔案".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct Fixture {
        directory: PathBuf,
        roaming: PathBuf,
        control: PathBuf,
        install: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "kaleidoscope-storage-test-{}",
                Uuid::new_v4().hyphenated()
            ));
            fs::create_dir(&directory).unwrap();
            let directory = fs::canonicalize(directory).unwrap();
            let roaming = directory.join("roaming");
            let control = directory.join("control");
            let install = directory.join("install");
            for root in [&roaming, &control, &install] {
                fs::create_dir(root).unwrap();
            }
            fs::create_dir(roaming.join("osu")).unwrap();
            fs::write(install.join("osu!.exe"), b"not executed").unwrap();
            Self {
                directory,
                roaming,
                control,
                install,
            }
        }

        fn resolve(&self) -> Result<Library> {
            resolve(&self.roaming, &self.control, &self.install.join("osu!.exe"))
        }

        fn selector(&self, value: &Path) {
            fs::write(
                self.roaming.join("osu/storage.ini"),
                format!("# selector fixture\r\nFullPath = {}\r\n", value.display()),
            )
            .unwrap();
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
                .starts_with("kaleidoscope-storage-test-"));
            fs::remove_dir_all(directory).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_known_folder_abi_returns_existing_absolute_roots() {
        // Only user-folder metadata is inspected; no game/config/library paths.
        let roaming = roaming_app_data().unwrap();
        let local = local_app_data().unwrap();
        assert!(roaming.is_absolute() && local.is_absolute());
        assert!(roaming.is_dir() && local.is_dir());
        assert_ne!(roaming, local);
    }

    #[test]
    fn default_library_is_reused_without_reading_or_copying_game_data() {
        let fixture = Fixture::new();
        let root = fixture.roaming.join("osu");
        let realm = root.join("client.realm");
        fs::write(&realm, b"realm fixture marker").unwrap();
        fs::create_dir(root.join("files")).unwrap();
        fs::write(root.join("files/resource.fixture"), b"map fixture marker").unwrap();
        fs::write(root.join("game.ini"), b"official fixture config").unwrap();
        let realm_identity = directory_identity(&realm).unwrap();
        let library = fixture.resolve().unwrap();
        assert_eq!(library.root, fs::canonicalize(&root).unwrap());
        assert_eq!(directory_identity(&realm).unwrap(), realm_identity);
        assert_eq!(fs::read(realm).unwrap(), b"realm fixture marker");
        assert_eq!(
            fs::read(root.join("files/resource.fixture")).unwrap(),
            b"map fixture marker"
        );
        assert_eq!(
            fs::read(root.join("game.ini")).unwrap(),
            b"official fixture config"
        );
        assert_eq!(fs::read_dir(&fixture.control).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&fixture.install).unwrap().count(), 1);
    }

    #[test]
    fn custom_selector_is_read_only_and_does_not_follow_another_selector() {
        let fixture = Fixture::new();
        let custom = fixture.directory.join("現有 圖譜庫");
        let unrelated = fixture.directory.join("other-library");
        fs::create_dir(&custom).unwrap();
        fs::create_dir(&unrelated).unwrap();
        fixture.selector(&custom);
        fs::write(
            custom.join("storage.ini"),
            format!("FullPath = {}", unrelated.display()),
        )
        .unwrap();
        let selector_before = fs::read(fixture.roaming.join("osu/storage.ini")).unwrap();
        assert_eq!(
            fixture.resolve().unwrap().root,
            fs::canonicalize(custom).unwrap()
        );
        assert_eq!(
            fs::read(fixture.roaming.join("osu/storage.ini")).unwrap(),
            selector_before
        );
    }

    #[test]
    fn portable_install_and_its_custom_selector_take_precedence_over_roaming() {
        let fixture = Fixture::new();
        let custom = fixture.directory.join("portable-library");
        fs::create_dir(&custom).unwrap();
        fs::write(fixture.install.join("framework.ini"), b"portable marker").unwrap();
        assert_eq!(
            fixture.resolve().unwrap().root,
            fs::canonicalize(&fixture.install).unwrap()
        );
        fs::write(
            fixture.install.join("storage.ini"),
            format!("FullPath={}", custom.display()),
        )
        .unwrap();
        fixture.selector(&fixture.directory.join("missing-roaming-custom"));
        assert_eq!(
            fixture.resolve().unwrap().root,
            fs::canonicalize(custom).unwrap()
        );
    }

    #[test]
    fn invalid_custom_paths_and_oversize_selectors_fail_without_empty_fallback() {
        let fixture = Fixture::new();
        fixture.selector(&fixture.directory.join("missing-library"));
        assert!(fixture.resolve().unwrap_err().contains("不會改用"));
        assert!(!fixture.directory.join("missing-library").exists());
        for value in [
            b"FullPath=relative".as_slice(),
            b"FullPath=\0invalid",
            &[0xff],
        ] {
            fs::write(fixture.roaming.join("osu/storage.ini"), value).unwrap();
            assert!(fixture.resolve().is_err());
        }
        fs::write(
            fixture.roaming.join("osu/storage.ini"),
            vec![b' '; MAX_SELECTOR_BYTES as usize + 1],
        )
        .unwrap();
        assert!(fixture.resolve().unwrap_err().contains("大小限制"));
        fs::remove_file(fixture.roaming.join("osu/storage.ini")).unwrap();
        fs::remove_dir(fixture.roaming.join("osu")).unwrap();
        assert!(fixture.resolve().is_err());
        assert!(!fixture.roaming.join("osu").exists());
    }

    #[test]
    fn selector_encoding_whitespace_and_last_setting_match_ini_contract() {
        let fixture = Fixture::new();
        let root = fixture.roaming.join("osu");
        let text = format!(
            "# comment\r\n FullPath = {}\r\nignored = value\r\n",
            root.display()
        );
        assert_eq!(
            parse_target(format!("\u{feff}{text}").as_bytes()).unwrap(),
            Some(root.clone())
        );
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in text.encode_utf16() {
                bytes.extend(if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert_eq!(parse_target(&bytes).unwrap(), Some(root.clone()));
        }
        assert_eq!(
            parse_target(format!("{text}FullPath = \r\n").as_bytes()).unwrap(),
            None
        );
        assert_eq!(parse_target(b"fullpath=relative").unwrap(), None);
        assert!(parse_target(&[0xff, 0xfe, 1]).is_err());
    }

    #[test]
    fn directory_identity_change_and_control_overlap_are_refused() {
        let fixture = Fixture::new();
        let library = fixture.resolve().unwrap();
        library.validate().unwrap();
        fs::rename(&library.root, fixture.directory.join("original-library")).unwrap();
        fs::create_dir(&library.root).unwrap();
        assert!(library.validate().is_err());
        fixture.selector(&fixture.control);
        assert!(fixture.resolve().is_err());
        fixture.selector(&fixture.directory);
        assert!(fixture.resolve().is_err());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn symlinked_selector_portable_marker_and_library_are_refused() {
        let fixture = Fixture::new();
        let source = fixture.directory.join("selector.fixture");
        fs::write(&source, b"FullPath=\n").unwrap();
        let selector = fixture.roaming.join("osu/storage.ini");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, &selector).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(&source, &selector).is_err() {
            eprintln!("SKIPPED storage symlink fixture: missing Windows symlink privilege");
            return;
        }
        assert!(fixture.resolve().is_err());
        fs::remove_file(&selector).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&source, fixture.install.join("framework.ini")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&source, fixture.install.join("framework.ini")).unwrap();
        assert!(fixture.resolve().is_err());
        fs::remove_file(fixture.install.join("framework.ini")).unwrap();
        let linked_root = fixture.directory.join("linked-library");
        #[cfg(unix)]
        std::os::unix::fs::symlink(fixture.roaming.join("osu"), &linked_root).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(fixture.roaming.join("osu"), &linked_root).unwrap();
        fixture.selector(&linked_root);
        assert!(fixture.resolve().is_err());
        assert_eq!(fs::read(source).unwrap(), b"FullPath=\n");
    }
}
