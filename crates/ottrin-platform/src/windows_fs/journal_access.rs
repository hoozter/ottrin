//! Recoverable write exclusion for owned journals and eligible private folders.
//! Only the two known access states are admitted. Cached arbitrary ACLs are
//! never replayed, and no user payload object is accepted by this policy.
use super::{LocalSecurity, PerObjectFile, current_owner, valid_sid};
use crate::{PlatformError, move_recovery::stable_identity_from_handle};
use std::{fs::File, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT, SetSecurityInfo},
        *,
    },
    Storage::FileSystem::*,
};

const OWNER_RIGHTS: &[u8] = &[1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Full,
    Frozen,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    owner: Vec<u8>,
    sid: Vec<u8>,
    flags: u8,
    directory: bool,
}
fn io(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Owned journal access: {e}"))
}

fn sid_text(sid: &[u8]) -> Result<String, PlatformError> {
    if !valid_sid(sid) {
        return Err(io("invalid principal"));
    }
    let authority = sid[2..8].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b));
    let mut text = format!("S-1-{authority}");
    for part in sid[8..].chunks_exact(4) {
        text.push_str(&format!(
            "-{}",
            u32::from_le_bytes(part.try_into().unwrap())
        ));
    }
    Ok(text)
}
fn mask(directory: bool, state: State) -> u32 {
    match state {
        State::Full => FILE_ALL_ACCESS,
        State::Frozen => {
            FILE_GENERIC_READ
                | DELETE
                | WRITE_DAC
                | if directory { FILE_GENERIC_EXECUTE } else { 0 }
        }
    }
}
impl Policy {
    pub(crate) fn valid(&self) -> bool {
        valid_sid(&self.owner)
            && matches!(self.flags, 0 | 3)
            && if self.directory {
                self.sid == self.owner || self.sid == OWNER_RIGHTS
            } else {
                self.sid == OWNER_RIGHTS
            }
    }
    pub(crate) fn observe(file: &File, directory: bool) -> Result<(Self, State), PlatformError> {
        let identity = stable_identity_from_handle(file).map_err(io)?;
        if if directory {
            !identity.is_directory_without_reparse()
        } else {
            !identity.is_regular_file_without_reparse()
        } {
            return Err(io("unexpected object type"));
        }
        let (mut owner, mut dacl, mut raw) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut raw,
            )
        };
        if status != 0 {
            return Err(io(format!("security query failed: {status}")));
        }
        struct Allocation(PSECURITY_DESCRIPTOR);
        impl Drop for Allocation {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
        let _allocation = Allocation(raw);
        let (mut control, mut revision) = (0, 0);
        if owner.is_null()
            || dacl.is_null()
            || unsafe { IsValidSid(owner) } == 0
            || unsafe { IsValidAcl(dacl) } == 0
            || unsafe { GetSecurityDescriptorControl(raw, &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
            || unsafe { (*dacl).AceCount } != 1
        {
            return Err(io("unrecognized security; object preserved"));
        }
        let owner =
            unsafe { std::slice::from_raw_parts(owner.cast(), GetLengthSid(owner) as usize) }
                .to_vec();
        if owner != current_owner()? {
            return Err(io("object owner differs from the effective owner"));
        }
        let mut raw_ace = ptr::null_mut();
        if unsafe { GetAce(dacl, 0, &mut raw_ace) } == 0 || raw_ace.is_null() {
            return Err(io("invalid owned access entry"));
        }
        let ace = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
        if usize::from(ace.Header.AceSize) < 8 + 8 {
            return Err(io("short owned access entry"));
        }
        if ace.Header.AceType != 0 || !matches!(ace.Header.AceFlags, 0 | 3) {
            return Err(io("unrecognized access entry; object preserved"));
        }
        // Bound the SID by its containing ACE before inspecting its declared
        // sub-authority count. Never ask native SID readers to walk past it.
        let sid = unsafe {
            std::slice::from_raw_parts(
                ptr::from_ref(&ace.SidStart).cast::<u8>(),
                usize::from(ace.Header.AceSize) - 8,
            )
        };
        if !valid_sid(sid) {
            return Err(io("invalid owned access size"));
        }
        let policy = Self {
            owner,
            sid: sid.to_vec(),
            flags: ace.Header.AceFlags,
            directory,
        };
        if !policy.valid() {
            return Err(io(format!(
                "unrecognized principal {}; owner {}; flags {}; directory {}; object preserved",
                sid_text(&policy.sid)?,
                sid_text(&policy.owner)?,
                policy.flags,
                policy.directory,
            )));
        }
        let state = if ace.Mask == mask(directory, State::Full) {
            State::Full
        } else if ace.Mask == mask(directory, State::Frozen) {
            State::Frozen
        } else {
            return Err(io("unrecognized permissions; object preserved"));
        };
        Ok((policy, state))
    }
    fn apply_at(&self, file: &File, state: State) -> Result<(), PlatformError> {
        if !self.valid() || Self::observe(file, self.directory)?.0 != *self {
            return Err(io("access evidence changed"));
        }
        let flags = if self.flags == 3 { "OICI" } else { "" };
        let descriptor = LocalSecurity::from_sddl(&format!(
            "D:P(A;{flags};0x{:x};;;{})",
            mask(self.directory, state),
            sid_text(&self.sid)?
        ))
        .map_err(io)?;
        let (mut present, mut defaulted, mut dacl) = (0, 0, ptr::null_mut());
        if unsafe {
            GetSecurityDescriptorDacl(descriptor.as_raw(), &mut present, &mut dacl, &mut defaulted)
        } == 0
            || present == 0
            || dacl.is_null()
        {
            return Err(io("invalid derived access policy"));
        }
        let result = unsafe {
            SetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null(),
            )
        };
        if result != 0 {
            return Err(io(format!("access update failed: {result}")));
        }
        if Self::observe(file, self.directory)? != (self.clone(), state) {
            return Err(io("access update did not persist"));
        }
        Ok(())
    }
    /// The per-object capability prevents directory inheritance propagation.
    pub(crate) fn apply(&self, file: &PerObjectFile, state: State) -> Result<(), PlatformError> {
        self.apply_at(file, state)
    }
}
