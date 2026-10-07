//! Mandatory .NET startup bridge. This hosts the installed application without
//! changing its assemblies or runtimeconfig. Run only in a dedicated child.

use crate::Result;
use std::path::Path;

#[cfg(not(windows))]
pub fn run_managed(_application: &Path, _hostfxr: &Path, _bridge: &Path) -> Result<i32> {
    Err("The native lazer host currently supports Windows only".into())
}

#[cfg(windows)]
pub fn run_managed(application: &Path, hostfxr: &Path, bridge: &Path) -> Result<i32> {
    windows::run(application, hostfxr, bridge)
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    };

    type Initialize =
        unsafe extern "C" fn(i32, *const *const u16, *const c_void, *mut *mut c_void) -> i32;
    type SetProperty = unsafe extern "C" fn(*mut c_void, *const u16, *const u16) -> i32;
    type Run = unsafe extern "C" fn(*mut c_void) -> i32;
    type Close = unsafe extern "C" fn(*mut c_void) -> i32;

    struct Library(HMODULE);
    impl Drop for Library {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
    struct Context {
        handle: *mut c_void,
        close: Close,
    }
    impl Drop for Context {
        fn drop(&mut self) {
            unsafe {
                (self.close)(self.handle);
            }
        }
    }

    fn wide_path(value: &Path) -> Result<Vec<u16>> {
        if !value.is_absolute() || !value.is_file() {
            return Err("The runtime host requires absolute existing files".into());
        }
        let mut encoded: Vec<u16> = value.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err("A runtime path contains a NUL character".into());
        }
        encoded.push(0);
        Ok(encoded)
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    unsafe fn symbol(
        library: HMODULE,
        name: &'static [u8],
    ) -> Result<unsafe extern "system" fn() -> isize> {
        unsafe { GetProcAddress(library, name.as_ptr()) }
            .ok_or_else(|| "The installed .NET host is missing a required hosting API".into())
    }

    pub(super) fn run(application: &Path, hostfxr: &Path, bridge: &Path) -> Result<i32> {
        let app_name = wide_path(application)?;
        let library_name = wide_path(hostfxr)?;
        let bridge_name = wide_path(bridge)?;
        let loaded = unsafe {
            LoadLibraryExW(
                library_name.as_ptr(),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        };
        if loaded.is_null() {
            return Err("Cannot load the selected client's .NET host".into());
        }
        let library = Library(loaded);
        // The documented hostfxr ABI uses __cdecl and UTF-16 on Windows.
        let initialize: Initialize = unsafe {
            std::mem::transmute(symbol(
                library.0,
                b"hostfxr_initialize_for_dotnet_command_line\0",
            )?)
        };
        let set_property: SetProperty = unsafe {
            std::mem::transmute(symbol(library.0, b"hostfxr_set_runtime_property_value\0")?)
        };
        let run: Run = unsafe { std::mem::transmute(symbol(library.0, b"hostfxr_run_app\0")?) };
        let close: Close = unsafe { std::mem::transmute(symbol(library.0, b"hostfxr_close\0")?) };
        let args = [app_name.as_ptr()];
        let mut handle = ptr::null_mut();
        let result = unsafe { initialize(1, args.as_ptr(), ptr::null(), &mut handle) };
        // This child must own the first context. Reusing another loaded runtime
        // would make pre-Main enforcement impossible, so positive statuses fail.
        if result != 0 || handle.is_null() {
            if !handle.is_null() {
                unsafe {
                    close(handle);
                }
            }
            return Err(format!(
                "Client runtime initialization failed ({result:#x})"
            ));
        }
        let context = Context { handle, close };
        let support_key = wide("System.StartupHookProvider.IsSupported");
        let enabled = wide("true");
        let hook_key = wide("STARTUP_HOOKS");
        for (key, value) in [
            (support_key.as_ptr(), enabled.as_ptr()),
            (hook_key.as_ptr(), bridge_name.as_ptr()),
        ] {
            let status = unsafe { set_property(context.handle, key, value) };
            if status != 0 {
                return Err(format!(
                    "Mandatory startup bridge configuration failed ({status:#x})"
                ));
            }
        }
        // No fallback branch launches osu!.exe. A missing or throwing startup
        // hook is an application failure and never invokes the original Main.
        Ok(unsafe { run(context.handle) })
    }
}
