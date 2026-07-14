use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf, ptr};

use winapi::{
    shared::{
        minwindef::{DWORD, MAX_PATH},
        winerror::ERROR_INSUFFICIENT_BUFFER,
    },
    um::{errhandlingapi::GetLastError, libloaderapi::GetModuleFileNameW},
};

pub fn get_plugin_path() -> Option<PathBuf> {
    fn get_plugin_path(len: usize) -> Option<PathBuf> {
        let mut buf = Vec::with_capacity(len);
        #[expect(clippy::as_conversions, reason = "We should never get that far")]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "We should never get that far"
        )]
        // SAFETY: `buf` has capacity for `len`.
        let ret =
            unsafe { GetModuleFileNameW(ptr::null_mut(), buf.as_mut_ptr(), len as DWORD) } as usize;
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
                get_plugin_path(len * 2)
            } else {
                None
            }
        }
    }

    get_plugin_path(MAX_PATH)
}
