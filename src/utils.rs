use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf, ptr};

use winapi::{
    shared::{
        minwindef::{DWORD, HMODULE, MAX_PATH},
        winerror::ERROR_INSUFFICIENT_BUFFER,
    },
    um::{
        errhandlingapi::GetLastError,
        libloaderapi::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        },
    },
};

/// Returns the handle of the module (DLL) this code is compiled into, rather
/// than the host process. Without this, [`GetModuleFileNameW`] with a null
/// handle would return EuroScope's executable path instead of the plugin's.
fn current_module() -> Option<HMODULE> {
    // Any address that lives inside this module; its own data section will do.
    // Typed `u16` so its address is already an `LPCWSTR` with no pointer cast.
    static ANCHOR: u16 = 0;
    let mut handle: HMODULE = ptr::null_mut();
    // SAFETY: `handle` is a valid out-pointer, `ANCHOR` lies within this module,
    // and `UNCHANGED_REFCOUNT` means we don't take a reference to release.
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            &raw const ANCHOR,
            &raw mut handle,
        )
    };
    if ok == 0 { None } else { Some(handle) }
}

pub fn get_plugin_path() -> Option<PathBuf> {
    fn get_plugin_path(module: HMODULE, len: usize) -> Option<PathBuf> {
        let mut buf = Vec::with_capacity(len);
        #[expect(clippy::as_conversions, reason = "We should never get that far")]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "We should never get that far"
        )]
        // SAFETY: `buf` has capacity for `len`.
        let ret = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), len as DWORD) } as usize;
        if ret == 0 {
            None
        } else if ret < len {
            // Success, we need to trim trailing null bytes from the vec.
            // SAFETY: the call initialized `ret` code units and `ret < len`, so
            // the new length stays within the initialized region.
            unsafe {
                buf.set_len(ret);
            }
            let s = OsString::from_wide(&buf);
            Some(s.into())
        } else {
            // The buffer might not be big enough so we need to check errno.
            // SAFETY: [`GetLastError`] has no preconditions.
            let errno = unsafe { GetLastError() };
            if errno == ERROR_INSUFFICIENT_BUFFER {
                get_plugin_path(module, len * 2)
            } else {
                None
            }
        }
    }

    let module = current_module()?;
    get_plugin_path(module, MAX_PATH)
}
