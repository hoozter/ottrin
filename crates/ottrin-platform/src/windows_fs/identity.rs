//! Effective Windows owner identity shared by file transactions.
use crate::PlatformError;
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_TOKEN, HANDLE},
    Security::*,
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
};
fn io(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(e.to_string())
}
pub(crate) fn valid_sid(s: &[u8]) -> bool {
    s.len() >= 8 && s[0] == 1 && s[1] <= 15 && s.len() == 8 + usize::from(s[1]) * 4
}
pub(crate) fn current_token() -> Result<OwnedHandle, PlatformError> {
    let mut token: HANDLE = ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(io(error));
        }
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(token.cast()) })
}

pub(crate) fn current_owner() -> Result<Vec<u8>, PlatformError> {
    owner_for_token(&current_token()?)
}
pub(crate) fn owner_for_token(token: &OwnedHandle) -> Result<Vec<u8>, PlatformError> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenOwner,
            ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 || needed > 65536 {
        return Err(io("invalid effective token owner size"));
    }
    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenOwner,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    let owner = unsafe { &*storage.as_ptr().cast::<TOKEN_OWNER>() }.Owner;
    if owner.is_null() {
        return Err(io("effective token has no default owner"));
    }
    let sid =
        unsafe { std::slice::from_raw_parts(owner.cast::<u8>(), GetLengthSid(owner) as usize) }
            .to_vec();
    if !valid_sid(&sid) {
        return Err(io("effective token owner SID is invalid"));
    }
    Ok(sid)
}
