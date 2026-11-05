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
    let storage = token_information(token, TokenOwner, std::mem::size_of::<TOKEN_OWNER>())?;
    // SAFETY: aligned output was validated against the requested structure size.
    let sid = unsafe { &*storage.as_ptr().cast::<TOKEN_OWNER>() }.Owner;
    token_sid(&storage, sid)
}

/// The token's user is the application principal, independently of the default
/// file owner. Elevated tokens can have a group as TokenOwner; do not conflate it
/// with the original application's TokenUser when binding helper authority.
pub(crate) fn user_for_token(token: &OwnedHandle) -> Result<Vec<u8>, PlatformError> {
    let storage = token_information(token, TokenUser, std::mem::size_of::<TOKEN_USER>())?;
    // SAFETY: aligned output was validated against the requested structure size.
    let sid = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
    token_sid(&storage, sid)
}

fn token_information(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
    minimum: usize,
) -> Result<Vec<u64>, PlatformError> {
    let mut needed = 0;
    // SAFETY: the sizing call writes only needed; no output buffer is supplied.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            class,
            ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if (needed as usize) < minimum || needed > 65536 {
        return Err(io("invalid token identity information size"));
    }
    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the aligned initialized allocation covers the requested byte count.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            class,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if (needed as usize) < minimum || needed as usize > storage.len() * 8 {
        return Err(io("token identity information changed size"));
    }
    Ok(storage)
}

fn token_sid(storage: &[u64], sid: PSID) -> Result<Vec<u8>, PlatformError> {
    // GetTokenInformation returns SID data inside its caller-owned output.
    // Bound the header and subauthorities before reading through that pointer.
    let offset = (sid as usize)
        .checked_sub(storage.as_ptr() as usize)
        .ok_or_else(|| io("token identity SID is outside its output buffer"))?;
    // SAFETY: the entire u64 allocation is initialized and borrowed for this read.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            storage.as_ptr().cast::<u8>(),
            std::mem::size_of_val(storage),
        )
    };
    let header = bytes
        .get(
            offset
                ..offset
                    .checked_add(8)
                    .ok_or_else(|| io("invalid token SID offset"))?,
        )
        .ok_or_else(|| io("token identity SID header is truncated"))?;
    let end = offset
        .checked_add(8 + usize::from(header[1]) * 4)
        .ok_or_else(|| io("invalid token SID length"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| io("token identity SID is truncated"))?;
    if !valid_sid(value) {
        return Err(io("token identity SID is invalid"));
    }
    Ok(value.to_vec())
}
