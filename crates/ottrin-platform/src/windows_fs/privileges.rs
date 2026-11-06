//! Per-operation file privileges on a duplicated effective helper token.
//! Neither the process token nor any existing impersonation token is modified.
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_TOKEN, GetLastError, SetLastError},
    Security::*,
    System::Threading::{
        GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken, SetThreadToken,
    },
};

struct Restore {
    previous: Option<OwnedHandle>,
    installed: bool,
}
impl Restore {
    fn capture() -> io::Result<Self> {
        let mut raw = ptr::null_mut();
        let previous = if unsafe {
            OpenThreadToken(
                GetCurrentThread(),
                TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
                1,
                &mut raw,
            )
        } != 0
        {
            Some(unsafe { OwnedHandle::from_raw_handle(raw.cast()) })
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
                return Err(error);
            }
            None
        };
        Ok(Self {
            previous,
            installed: false,
        })
    }
    fn install<T>(
        mut self,
        token: &OwnedHandle,
        operation: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        if unsafe { SetThreadToken(ptr::null(), token.as_raw_handle().cast()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.installed = true;
        operation()
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        if !self.installed {
            return;
        }
        let token = self
            .previous
            .as_ref()
            .map_or(ptr::null_mut(), |token| token.as_raw_handle().cast());
        // Never continue with an unexpectedly changed effective identity.
        if unsafe { SetThreadToken(ptr::null(), token) } == 0 {
            std::process::abort();
        }
    }
}

/// The caller provides an already admitted impersonation token. No token is
/// modified; restore the exact prior thread token on return, error or unwind.
pub(crate) fn with_thread_token<T>(
    token: &OwnedHandle,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    Restore::capture()?.install(token, operation)
}

pub(crate) fn with_file_privileges<T>(operation: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let restore = Restore::capture()?;
    let mut raw = ptr::null_mut();
    let process;
    let effective = if let Some(token) = &restore.previous {
        token
    } else {
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut raw) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        process = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        &process
    };
    if unsafe {
        DuplicateTokenEx(
            effective.as_raw_handle().cast(),
            TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_ADJUST_PRIVILEGES,
            ptr::null(),
            SecurityImpersonation,
            TokenImpersonation,
            &mut raw,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let duplicate = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    for name in ["SeBackupPrivilege", "SeRestorePrivilege"] {
        let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut luid = Default::default();
        if unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let requested = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        unsafe {
            SetLastError(0);
        }
        if unsafe {
            AdjustTokenPrivileges(
                duplicate.as_raw_handle().cast(),
                0,
                &requested,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let error = unsafe { GetLastError() };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
    }
    restore.install(&duplicate, operation)
}
