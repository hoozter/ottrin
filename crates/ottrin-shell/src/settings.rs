use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows::{
    Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER, Storage::Packaging::Appx::GetCurrentPackagePath,
    },
    core::PWSTR,
};
use windows_sys::{
    Win32::{Foundation::*, System::Registry::*},
    core::w,
};

// A package-virtualized preference shared by the app and its COM surrogate. One
// DWORD is the complete state, so a single registry write is the commit. Windows
// removes the package registration and its virtualized state at uninstall.
const KEY: *const u16 = w!("Software\\Ottrin\\PackagedFolderMenu");
const VALUE: *const u16 = w!("Enabled");

pub fn package_directory() -> Result<PathBuf, String> {
    let mut length = 0;
    let code = unsafe { GetCurrentPackagePath(&mut length, PWSTR::null()) };
    if code != ERROR_INSUFFICIENT_BUFFER || length == 0 || length > 32768 {
        return Err(format!(
            "Cannot locate the current Windows package: {}",
            code.0
        ));
    }
    let mut path = vec![0; length as usize];
    let code = unsafe { GetCurrentPackagePath(&mut length, PWSTR(path.as_mut_ptr())) };
    if code.0 != 0 {
        return Err(format!(
            "Cannot read the current Windows package path: {}",
            code.0
        ));
    }
    if length == 0 || length as usize > path.len() || path[length as usize - 1] != 0 {
        return Err("Windows returned an invalid package path".into());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &path[..length as usize - 1],
    )))
}

pub fn enabled() -> Result<bool, String> {
    // Do not silently read a different user's or an unpackaged app's state.
    package_directory()?;
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            VALUE,
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut bytes,
        )
    };
    match code {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        ERROR_SUCCESS if bytes == 4 && value <= 1 => Ok(value == 1),
        ERROR_SUCCESS => Err("The packaged folder-menu preference is invalid; retained".into()),
        error => Err(format!(
            "Cannot read the packaged folder-menu preference (Windows error {error})"
        )),
    }
}

pub fn set_enabled(value: bool) -> Result<(), String> {
    // Validate existing state before replacing it. Foreign/malformed state is
    // retained. Missing state is the explicit default Off.
    enabled()?;
    let mut key = std::ptr::null_mut();
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            KEY,
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(format!(
            "Cannot open the packaged folder-menu preference (Windows error {code})"
        ));
    }
    let value = u32::from(value);
    let code = unsafe {
        RegSetValueExW(
            key,
            VALUE,
            0,
            REG_DWORD,
            (&value as *const u32).cast(),
            size_of::<u32>() as u32,
        )
    };
    // Every successful open transfers one owned handle, even if the write fails.
    unsafe {
        RegCloseKey(key);
    }
    if code != ERROR_SUCCESS {
        return Err(format!(
            "Cannot save the packaged folder-menu preference (Windows error {code})"
        ));
    }
    Ok(())
}
