//! Private staging ownership and final destination DACL policy.
//! No publication/cleanup or complete SACL/audit recovery policy.
use super::{PlatformError, WindowsStableIdentity, io, namespace, stable_identity_from_handle};
use crate::windows_fs;
#[cfg(test)]
use crate::windows_fs::RelativeOptions;
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::Foundation::{ERROR_NO_TOKEN, HANDLE};
use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

mod tree;
pub(super) use tree::{
    PlannedEntry, SecurityEntry, SecurityPlanHeader, TreeSecurityEvidence, TreeSecurityPlan,
};

const INFORMATION: u32 =
    OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Parts {
    owner: Vec<u8>,
    group: Vec<u8>,
    present: bool,
    dacl: Option<Vec<u8>>,
    control: u16,
}

impl Parts {
    // Structural validation of cached evidence only. Deserialized ACLs never
    // become Windows descriptors; recovery must rederive and compare the plan.
    fn valid_record(&self) -> bool {
        if !valid_sid(&self.owner)
            || !valid_sid(&self.group)
            || self.control & !(SE_DACL_PROTECTED | SE_DACL_AUTO_INHERITED) != 0
        {
            return false;
        }
        let Some(acl) = &self.dacl else {
            return true;
        };
        if !self.present
            || acl.len() < 8
            || !matches!(acl[0], 2 | 4)
            || acl[1] != 0
            || acl[6..8] != [0, 0]
            || usize::from(u16::from_le_bytes([acl[2], acl[3]])) != acl.len()
        {
            return false;
        }
        let mut at = 8usize;
        for _ in 0..u16::from_le_bytes([acl[4], acl[5]]) {
            let Some(header) = acl.get(at..at + 4) else {
                return false;
            };
            let size = usize::from(u16::from_le_bytes([header[2], header[3]]));
            if size < 4 || size % 4 != 0 || at + size > acl.len() {
                return false;
            }
            at += size;
        }
        true
    }
}

fn parts(raw: PSECURITY_DESCRIPTOR) -> Result<Parts, PlatformError> {
    let (mut owner, mut group, mut dacl) = (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    let (mut defaulted, mut present, mut control, mut revision) = (0, 0, 0, 0);
    if unsafe { GetSecurityDescriptorOwner(raw, &mut owner, &mut defaulted) } == 0
        || unsafe { GetSecurityDescriptorGroup(raw, &mut group, &mut defaulted) } == 0
        || unsafe { GetSecurityDescriptorDacl(raw, &mut present, &mut dacl, &mut defaulted) } == 0
        || unsafe { GetSecurityDescriptorControl(raw, &mut control, &mut revision) } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if owner.is_null() || group.is_null() {
        return Err(io("native file security has no owner/group"));
    }
    let sid = |raw: PSID| unsafe {
        std::slice::from_raw_parts(raw.cast::<u8>(), GetLengthSid(raw) as usize).to_vec()
    };
    let acl = if dacl.is_null() {
        None
    } else {
        Some(
            unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), (*dacl).AclSize as usize) }
                .to_vec(),
        )
    };
    Ok(Parts {
        owner: sid(owner),
        group: sid(group),
        present: present != 0,
        dacl: acl,
        control: control & (SE_DACL_PROTECTED | SE_DACL_AUTO_INHERITED),
    })
}

fn capture(file: &File) -> Result<Vec<u64>, PlatformError> {
    let mut needed = 0;
    unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle().cast(),
            INFORMATION,
            ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 || needed > 65536 {
        return Err(io("invalid native file security length"));
    }
    let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
    if unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle().cast(),
            INFORMATION,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(storage)
}

fn current_token() -> Result<OwnedHandle, PlatformError> {
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

/// Private staging security is recorded separately from final destination
/// inheritance. Serialized ACLs are validated as bytes, never passed to Windows.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(super) struct PrivateSecurity(Parts);

fn valid_sid(sid: &[u8]) -> bool {
    sid.len() >= 8 && sid[0] == 1 && sid[1] <= 15 && sid.len() == 8 + usize::from(sid[1]) * 4
}
impl PrivateSecurity {
    pub(super) fn valid_for(&self, creator: &[u8]) -> bool {
        let parts = &self.0;
        let Some(acl) = &parts.dacl else {
            return false;
        };
        // One explicit OWNER RIGHTS full-access ACE. Only its normal child
        // inheritance flags are admitted; inherited/inherit-only ACEs are not.
        valid_sid(creator)
            && parts.owner == creator
            && valid_sid(&parts.group)
            && parts.present
            && parts.control & SE_DACL_PROTECTED != 0
            && parts.control & !(SE_DACL_PROTECTED | SE_DACL_AUTO_INHERITED) == 0
            && acl.len() == 28
            && acl[0] == 2
            && acl[1] == 0
            && u16::from_le_bytes([acl[2], acl[3]]) == 28
            && acl[4..8] == [1, 0, 0, 0]
            && acl[8] == 0
            && matches!(acl[9], 0 | 3)
            && u16::from_le_bytes([acl[10], acl[11]]) == 20
            && u32::from_le_bytes(acl[12..16].try_into().unwrap()) == FILE_ALL_ACCESS
            && acl[16..] == [1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0]
    }
    pub(super) fn capture(file: &File, creator: &[u8]) -> Result<Self, PlatformError> {
        let mut raw = capture(file)?;
        let result = Self(parts(raw.as_mut_ptr().cast())?);
        if !result.valid_for(creator) {
            return Err(io(
                "Copy staging security is not protected creator-owned OWNER RIGHTS access; data retained",
            ));
        }
        Ok(result)
    }
    pub(super) fn validate(&self, file: &File, creator: &[u8]) -> Result<(), PlatformError> {
        if Self::capture(file, creator)? != *self {
            return Err(io("private Copy owner/group/DACL changed; data retained"));
        }
        Ok(())
    }
}

pub(super) fn current_owner() -> Result<Vec<u8>, PlatformError> {
    owner_for_token(&current_token()?)
}
fn owner_for_token(token: &OwnedHandle) -> Result<Vec<u8>, PlatformError> {
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

struct Derived(PSECURITY_DESCRIPTOR);
impl Drop for Derived {
    fn drop(&mut self) {
        unsafe {
            DestroyPrivateObjectSecurity(&self.0);
        }
    }
}

// All descriptors here are owned native API outputs, never deserialized bytes.
fn derive(
    parent: PSECURITY_DESCRIPTOR,
    token: &OwnedHandle,
    directory: bool,
) -> Result<(Derived, Parts), PlatformError> {
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };
    let mut raw = ptr::null_mut();
    if unsafe {
        CreatePrivateObjectSecurityEx(
            parent,
            ptr::null_mut(),
            &mut raw,
            ptr::null(),
            i32::from(directory),
            SEF_DACL_AUTO_INHERIT,
            token.as_raw_handle().cast(),
            &mapping,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    let derived = Derived(raw);
    let expected = parts(raw)?;
    if !expected.present || expected.dacl.is_none() {
        return Err(PlatformError::UnsupportedSafeCopy(
            "derived destination DACL is absent/null".into(),
        ));
    }
    Ok((derived, expected))
}

/// New-copy ownership and DACL follow an ordinary new child at the destination.
/// Source ACLs and the private staging DACL are deliberately not creator inputs.
pub(super) struct DestinationSecurity {
    parent: WindowsStableIdentity,
    parent_security: Parts,
    directory: bool,
    derived: Derived,
    expected: Parts,
}
impl DestinationSecurity {
    pub(super) fn capture(parent: &File, directory: bool) -> Result<Self, PlatformError> {
        Self::capture_with_token(parent, directory, &current_token()?)
    }
    fn capture_with_token(
        parent: &File,
        directory: bool,
        token: &OwnedHandle,
    ) -> Result<Self, PlatformError> {
        let identity = stable_identity_from_handle(parent).map_err(io)?;
        if !identity.is_directory_without_reparse() {
            return Err(io("security parent is not a non-reparse directory"));
        }
        let mut security = capture(parent)?;
        let parent_security = parts(security.as_mut_ptr().cast())?;
        let (derived, expected) = derive(security.as_mut_ptr().cast(), token, directory)?;
        let result = Self {
            parent: identity,
            parent_security,
            directory,
            derived,
            expected,
        };
        result.validate_parent(parent)?;
        Ok(result)
    }
    pub(super) fn validate_parent(&self, parent: &File) -> Result<(), PlatformError> {
        let mut security = capture(parent)?;
        if stable_identity_from_handle(parent).map_err(io)? != self.parent
            || parts(security.as_mut_ptr().cast())? != self.parent_security
        {
            return Err(io("destination parent identity or permissions changed"));
        }
        Ok(())
    }
    fn binding(&self, parent: &File, name: &OsStr, candidate: &File) -> Result<(), PlatformError> {
        self.validate_parent(parent)?;
        let identity = stable_identity_from_handle(candidate).map_err(io)?;
        if if self.directory {
            !identity.is_directory_without_reparse()
        } else {
            !identity.is_regular_file_without_reparse()
        } {
            return Err(io("destination entry type does not match security policy"));
        }
        namespace::validate_binding(parent, name, identity)?;
        Ok(())
    }
    pub(super) fn validate_candidate(&self, candidate: &File) -> Result<(), PlatformError> {
        let mut security = capture(candidate)?;
        if parts(security.as_mut_ptr().cast())? != self.expected {
            return Err(io(
                "published owner/group/DACL does not match destination policy",
            ));
        }
        Ok(())
    }
    /// Only after publication beneath the real parent. A durable publication
    /// owner must retain/recover this entry on every failure, including after
    /// SetSecurityInfo succeeds. This method never removes or rolls back data.
    pub(super) fn apply(
        &self,
        parent: &File,
        name: &OsStr,
        candidate: &File,
    ) -> Result<(), PlatformError> {
        self.binding(parent, name, candidate)?;
        let mut actual = capture(candidate)?;
        let actual = parts(actual.as_mut_ptr().cast())?;
        if actual.owner != self.expected.owner || actual.group != self.expected.group {
            return Err(io(
                "candidate owner/group differ from new-copy token policy",
            ));
        }
        let (mut present, mut defaulted, mut dacl) = (0, 0, ptr::null_mut());
        if unsafe {
            GetSecurityDescriptorDacl(self.derived.0, &mut present, &mut dacl, &mut defaulted)
        } == 0
        {
            return Err(io(std::io::Error::last_os_error()));
        }
        let error = unsafe {
            SetSecurityInfo(
                candidate.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null(),
            )
        };
        if error != 0 {
            return Err(io(std::io::Error::from_raw_os_error(error as i32)));
        }
        self.validate_candidate(candidate)?;
        self.binding(parent, name, candidate)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_fs::LocalSecurity;
    use std::io::Write;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::System::SystemServices::ACCESS_SYSTEM_SECURITY;

    fn parent(path: &std::path::Path) -> File {
        OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn dacl(file: &File, sddl: &str) {
        let descriptor = LocalSecurity::from_sddl(sddl).unwrap();
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(
                    descriptor.as_raw(),
                    &mut present,
                    &mut acl,
                    &mut defaulted,
                )
            },
            0
        );
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    file.as_raw_handle().cast(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null(),
                )
            },
            0
        );
    }
    fn ordinary(parent: &File, name: &str, directory: bool) -> File {
        unsafe {
            windows_fs::open_relative(
                parent,
                OsStr::new(name),
                RelativeOptions {
                    directory: Some(directory),
                    create_new: true,
                    access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        }
        .unwrap()
    }
    fn observed(file: &File) -> Parts {
        let mut value = capture(file).unwrap();
        parts(value.as_mut_ptr().cast()).unwrap()
    }
    fn private(parent: &File, name: &str, directory: bool) -> File {
        windows_fs::create_private(
            parent,
            OsStr::new(name),
            directory,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .unwrap()
    }

    fn set_sacl(file: &File, text: &str, information: u32) {
        let descriptor = LocalSecurity::from_sddl(text).unwrap();
        let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
        assert_ne!(
            unsafe {
                GetSecurityDescriptorSacl(
                    descriptor.as_raw(),
                    &mut present,
                    &mut acl,
                    &mut defaulted,
                )
            },
            0
        );
        assert_ne!(present, 0);
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    file.as_raw_handle().cast(),
                    SE_FILE_OBJECT,
                    information,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                    acl,
                )
            },
            0
        );
    }

    fn observed_sacl(file: &File, information: u32) -> (bool, u16, Option<Vec<u8>>) {
        // The audit fixture explicitly owns SeSecurityPrivilege. Metadata access
        // does not conflict with the candidate's data-sharing restriction.
        let audit_handle;
        let file = if information == SACL_SECURITY_INFORMATION {
            audit_handle = OpenOptions::new()
                .access_mode(READ_CONTROL | ACCESS_SYSTEM_SECURITY)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(windows_fs::location(file).unwrap())
                .unwrap();
            assert_eq!(
                stable_identity_from_handle(&audit_handle).unwrap(),
                stable_identity_from_handle(file).unwrap()
            );
            &audit_handle
        } else {
            file
        };
        let mut needed = 0;
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle().cast(),
                information,
                ptr::null_mut(),
                0,
                &mut needed,
            );
        }
        assert!(needed > 0 && needed <= 65536);
        let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
        let raw = storage.as_mut_ptr().cast();
        assert_ne!(
            unsafe {
                GetKernelObjectSecurity(
                    file.as_raw_handle().cast(),
                    information,
                    raw,
                    needed,
                    &mut needed,
                )
            },
            0
        );
        let (mut present, mut defaulted, mut acl, mut control, mut revision) =
            (0, 0, ptr::null_mut(), 0, 0);
        assert_ne!(
            unsafe { GetSecurityDescriptorSacl(raw, &mut present, &mut acl, &mut defaulted) },
            0
        );
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(raw, &mut control, &mut revision) },
            0
        );
        let bytes = if acl.is_null() {
            None
        } else {
            Some(
                unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize) }
                    .to_vec(),
            )
        };
        (
            present != 0,
            control & (SE_SACL_PROTECTED | SE_SACL_AUTO_INHERITED),
            bytes,
        )
    }

    fn check_new_child_security(parent: &File, information: u32, flags: &str) {
        for directory in [false, true] {
            let name = if directory {
                "reference-directory"
            } else {
                "reference-file"
            };
            let reference = ordinary(parent, name, directory);
            let expected = observed_sacl(&reference, information);
            assert!(
                expected.2.as_ref().is_some_and(|acl| acl.len() > 8),
                "fixture must have inherited security"
            );
            let policy = DestinationSecurity::capture(parent, directory).unwrap();
            let stage = super::super::namespace::PrivateStage::create(parent).unwrap();
            let candidate = stage.create_candidate(directory).unwrap();
            super::super::tests::assert_owner_only(&candidate);
            assert_eq!(
                observed_sacl(&candidate, information),
                expected,
                "inheritance differs for {flags}, directory={directory}"
            );
            let children = directory.then(|| {
                let reference_child = ordinary(&reference, "child", false);
                let child = private(&candidate, "child", false);
                super::super::tests::assert_owner_only(&child);
                assert_eq!(
                    observed_sacl(&child, information),
                    observed_sacl(&reference_child, information)
                );
                (reference_child, child)
            });
            policy
                .apply(parent, &stage.candidate_name, &candidate)
                .unwrap();
            assert_eq!(
                observed_sacl(&candidate, information),
                expected,
                "DACL finalization changed inherited security"
            );
            if let Some((reference_child, child)) = children {
                assert_eq!(
                    observed_sacl(&child, information),
                    observed_sacl(&reference_child, information)
                );
                super::super::tests::assert_owner_only(&child);
            }
            stage.validate_candidate(&candidate, directory).unwrap();
        }
    }

    #[test]
    fn new_copy_candidate_inherits_the_real_parent_mandatory_label() {
        for flags in ["OICI", "OICINP"] {
            let root = tempfile::tempdir().unwrap();
            let parent = OpenOptions::new()
                .access_mode(
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC | WRITE_OWNER,
                )
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(root.path())
                .unwrap();
            set_sacl(
                &parent,
                &format!("S:(ML;{flags};NW;;;LW)"),
                LABEL_SECURITY_INFORMATION,
            );
            check_new_child_security(&parent, LABEL_SECURITY_INFORMATION, flags);
        }
    }

    #[test]
    #[ignore = "requires SeSecurityPrivilege; selected explicitly in the native SYSTEM audit lane"]
    fn new_copy_candidate_inherits_the_real_parent_audit_sacl() {
        use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
        struct Privilege {
            token: OwnedHandle,
            previous: TOKEN_PRIVILEGES,
        }
        impl Drop for Privilege {
            fn drop(&mut self) {
                if self.previous.PrivilegeCount != 0 {
                    assert_ne!(
                        unsafe {
                            AdjustTokenPrivileges(
                                self.token.as_raw_handle().cast(),
                                0,
                                &self.previous,
                                0,
                                ptr::null_mut(),
                                ptr::null_mut(),
                            )
                        },
                        0
                    );
                }
            }
        }
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe {
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES,
                    &mut raw,
                )
            },
            0
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mut luid = Default::default();
        let name = "SeSecurityPrivilege"
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        assert_ne!(
            unsafe { LookupPrivilegeValueW(ptr::null(), name.as_ptr(), &mut luid) },
            0
        );
        let requested = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut previous = TOKEN_PRIVILEGES::default();
        let mut returned = 0;
        unsafe {
            SetLastError(0);
        }
        assert_ne!(
            unsafe {
                AdjustTokenPrivileges(
                    token.as_raw_handle().cast(),
                    0,
                    &requested,
                    std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
                    &mut previous,
                    &mut returned,
                )
            },
            0
        );
        assert_eq!(
            unsafe { GetLastError() },
            0,
            "SYSTEM audit fixture privilege is required"
        );
        let _privilege = Privilege { token, previous };
        for flags in ["OICI", "OICINP"] {
            let root = tempfile::tempdir().unwrap();
            let parent = OpenOptions::new()
                .access_mode(
                    FILE_GENERIC_READ
                        | FILE_GENERIC_WRITE
                        | DELETE
                        | WRITE_DAC
                        | ACCESS_SYSTEM_SECURITY,
                )
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(root.path())
                .unwrap();
            set_sacl(
                &parent,
                &format!("S:(AU;{flags}SAFA;GR;;;WD)"),
                SACL_SECURITY_INFORMATION,
            );
            check_new_child_security(&parent, SACL_SECURITY_INFORMATION, flags);
        }
    }
    #[test]
    fn destination_dacl_matches_kernel_created_file_inheritance_and_token_defaults() {
        let root = tempfile::tempdir().unwrap();
        for (index, sddl) in [
            "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
            "D:P(A;;FA;;;OW)",
            "D:P(A;OICI;FA;;;CO)(A;;FA;;;OW)",
            "D:P(D;OICI;GW;;;BG)(A;OICI;FA;;;OW)",
        ]
        .iter()
        .enumerate()
        {
            let path = root.path().join(index.to_string());
            std::fs::create_dir(&path).unwrap();
            let parent = parent(&path);
            dacl(&parent, sddl);
            let policy = DestinationSecurity::capture(&parent, false).unwrap();
            let reference = ordinary(&parent, "reference", false);
            let candidate = private(&parent, "candidate", false);
            assert_ne!(observed(&candidate), observed(&reference));
            policy
                .apply(&parent, OsStr::new("candidate"), &candidate)
                .unwrap();
            assert_eq!(
                observed(&candidate),
                observed(&reference),
                "native child differs for {sddl}"
            );
            policy.validate_candidate(&candidate).unwrap();
            dacl(&parent, "D:P(A;OICI;FA;;;OW)(A;OICI;FRFX;;;WD)");
            assert_eq!(
                observed(&candidate),
                observed(&reference),
                "later parent ACL changes must propagate normally"
            );
            assert!(policy.validate_parent(&parent).is_err());
        }
    }
    #[test]
    fn destination_directory_dacl_and_child_inheritance_match_native_creation() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        dacl(&parent, "D:P(A;OICI;FA;;;CO)(A;;FA;;;OW)(A;OICI;FR;;;WD)");
        let policy = DestinationSecurity::capture(&parent, true).unwrap();
        let reference = ordinary(&parent, "reference", true);
        let candidate = private(&parent, "candidate", true);
        policy
            .apply(&parent, OsStr::new("candidate"), &candidate)
            .unwrap();
        assert_eq!(observed(&candidate), observed(&reference));
        let actual_child = ordinary(&candidate, "child", false);
        let reference_child = ordinary(&reference, "child", false);
        assert_eq!(observed(&actual_child), observed(&reference_child));
    }
    #[test]
    fn destination_security_refuses_changed_parent_permissions_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        dacl(&parent, "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)");
        let policy = DestinationSecurity::capture(&parent, false).unwrap();
        let candidate = private(&parent, "candidate", false);
        let before = observed(&candidate);
        dacl(&parent, "D:P(A;OICI;FA;;;OW)(A;OICI;FW;;;WD)");
        assert!(
            policy
                .apply(&parent, OsStr::new("candidate"), &candidate)
                .unwrap_err()
                .to_string()
                .contains("parent identity or permissions changed")
        );
        assert_eq!(observed(&candidate), before);
    }
    #[test]
    fn destination_security_refuses_private_stage_as_publication_parent() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let policy = DestinationSecurity::capture(&parent, false).unwrap();
        let stage = private(&parent, "stage", true);
        let candidate = private(&stage, "candidate", false);
        let before = observed(&candidate);
        assert!(
            policy
                .apply(&stage, OsStr::new("candidate"), &candidate)
                .is_err()
        );
        assert_eq!(observed(&candidate), before);
    }
    #[test]
    fn destination_security_refuses_wrong_name_identity_and_type() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let policy = DestinationSecurity::capture(&parent, false).unwrap();
        let candidate = private(&parent, "candidate", false);
        let unrelated = private(&parent, "unrelated", false);
        let directory = private(&parent, "directory", true);
        let before = observed(&candidate);
        for name in ["unrelated", "../candidate", "candidate:stream"] {
            assert!(policy.apply(&parent, OsStr::new(name), &candidate).is_err());
        }
        assert!(
            policy
                .apply(&parent, OsStr::new("directory"), &directory)
                .is_err()
        );
        assert_eq!(observed(&candidate), before);
        assert_eq!(observed(&unrelated), before);
    }
    #[test]
    fn destination_security_reports_write_dac_denial_without_changing_bytes_or_acl() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let path = root.path().join("candidate");
        let policy = DestinationSecurity::capture(&parent, false).unwrap();
        let candidate = private(&parent, "candidate", false);
        let before = observed(&candidate);
        (&candidate).write_all(b"retained candidate bytes").unwrap();
        drop(candidate);
        let read = File::open(&path).unwrap();
        assert!(
            policy
                .apply(&parent, OsStr::new("candidate"), &read)
                .is_err()
        );
        assert_eq!(observed(&read), before);
        assert_eq!(std::fs::read(path).unwrap(), b"retained candidate bytes");
    }
    #[test]
    fn windows_native_copy_private_group_drift_is_detected_without_payload_mutation() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let file = private(&parent, "private", false);
        std::fs::write(root.path().join("private"), b"private bytes").unwrap();
        let creator = current_owner().unwrap();
        let expected = PrivateSecurity::capture(&file, &creator).unwrap();
        assert_ne!(
            expected.0.owner, expected.0.group,
            "owned ordinary-user fixture must have a distinct primary group"
        );
        let mut raw = capture(&file).unwrap();
        let (mut owner, mut defaulted) = (ptr::null_mut(), 0);
        assert_ne!(
            unsafe {
                GetSecurityDescriptorOwner(raw.as_mut_ptr().cast(), &mut owner, &mut defaulted)
            },
            0
        );
        // Production staging deliberately does not request WRITE_OWNER.
        // Only this controlled group-change fixture opens that extra right.
        let mutation = OpenOptions::new()
            .access_mode(WRITE_OWNER | READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root.path().join("private"))
            .unwrap();
        assert_eq!(
            unsafe {
                SetSecurityInfo(
                    mutation.as_raw_handle().cast(),
                    SE_FILE_OBJECT,
                    GROUP_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    owner,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            0
        );
        let changed = PrivateSecurity::capture(&file, &creator).unwrap();
        assert_eq!(changed.0.owner, expected.0.owner);
        assert_eq!(changed.0.group, expected.0.owner);
        assert!(expected.validate(&file, &creator).is_err());
        assert_eq!(
            std::fs::read(root.path().join("private")).unwrap(),
            b"private bytes"
        );
        assert_eq!(
            PrivateSecurity::capture(&file, &creator).unwrap(),
            changed,
            "validation must not restore the old group"
        );
    }
}
