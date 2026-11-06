//! Private Copy directories provisioned through an external native-identity anchor.
use super::{WindowsStableIdentity, io, namespace, security, stable_identity_from_handle};
use crate::{PlatformError, windows_fs};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr,
    sync::Arc,
};
use windows_sys::Win32::Storage::FileSystem::*;

const ANCHOR: &str = ".ottrin-copy-recovery-root";
const CLAIMS_ROOT: &str = ".ottrin-copy-target-claims";
const CLAIMS_ANCHOR: &str = ".ottrin-copy-target-claims-root";
const COHORTS_ROOT: &str = ".ottrin-copy-cleanup-cohorts";
const COHORTS_ANCHOR: &str = ".ottrin-copy-cleanup-cohorts-root";
const MAX_ANCHOR: u64 = 65_536;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Anchor {
    protocol: String,
    case_policy: Option<bool>,
    parent: WindowsStableIdentity,
    marker: WindowsStableIdentity,
    root: WindowsStableIdentity,
    name: Vec<u16>,
    creator: Vec<u8>,
    security: security::PrivateSecurity,
}

/// Handle-free evidence for reopening the same admitted registry. Serialized
/// security is compared with live handles, never installed as an ACL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    anchor: Anchor,
    marker_security: security::PrivateSecurity,
    parent_location: Vec<u16>,
    parent_security: [u8; 32],
    cases: (bool, bool),
}

/// Held registry admission. Private creation/reopen retains its authenticated
/// caller authority; this alone does not admit a Copy transaction or cleanup.
pub struct AnchoredRoot {
    authority: Authority,
    root: File,
    anchor_name: OsString,
    expected_case: Option<bool>,
    marker: File,
    marker_security: security::PrivateSecurity,
    parent: File,
    record: Anchor,
    parent_location: PathBuf,
    path: PathBuf,
    cases: (bool, bool),
    parent_security: [u8; 32],
}

#[derive(Clone)]
enum Authority {
    Process,
    Caller(Arc<crate::windows_privileged_transport::CallerCopyContext>),
}
impl Authority {
    fn validate(&self) -> Result<(), PlatformError> {
        match self {
            Self::Process => Ok(()),
            Self::Caller(caller) => caller.validate().map_err(io),
        }
    }
    fn owner(&self) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Process => windows_fs::current_owner(),
            Self::Caller(caller) => caller.default_file_owner_sid().map(Vec::from).map_err(io),
        }
    }
    fn create(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> std::io::Result<File> {
        match self {
            Self::Process => windows_fs::create_private(parent, name, directory, sharing),
            Self::Caller(caller) => caller.create_private_entry(parent, name, directory, sharing),
        }
    }
    fn child(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        access: u32,
        sharing: u32,
    ) -> std::io::Result<File> {
        match self {
            Self::Process => child(parent, name, directory, access, sharing),
            Self::Caller(caller) => {
                caller.validate()?;
                let file = windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        parent,
                        name,
                        Some(directory),
                        access,
                        sharing,
                    )
                })?;
                security::PrivateSecurity::capture_for_caller(
                    &file,
                    caller.default_file_owner_sid()?,
                    caller.default_file_group_sid()?,
                )?;
                caller.validate()?;
                Ok(file)
            }
        }
    }
    fn probe(&self, parent: &File, name: &OsStr) -> std::io::Result<File> {
        let open = || unsafe {
            windows_fs::open_relative(
                parent,
                name,
                windows_fs::RelativeOptions {
                    directory: None,
                    create_new: false,
                    access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        };
        match self {
            Self::Process => open(),
            Self::Caller(caller) => {
                caller.validate()?;
                let file = windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        parent,
                        name,
                        None,
                        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )
                })?;
                caller.validate()?;
                Ok(file)
            }
        }
    }
    fn binding(
        &self,
        parent: &File,
        name: &OsStr,
        expected: WindowsStableIdentity,
    ) -> Result<(), PlatformError> {
        self.validate()?;
        namespace::validate_binding_with(parent, expected, || self.probe(parent, name))?;
        self.validate()
    }
    fn writable_parent(&self, path: &Path) -> Result<File, PlatformError> {
        self.validate()?;
        let open = || windows_fs::open_directory(path, FILE_GENERIC_READ | FILE_GENERIC_WRITE);
        let file = match self {
            Self::Process => open(),
            Self::Caller(_) => windows_fs::with_file_privileges(open),
        }
        .map_err(io)?;
        self.validate()?;
        Ok(file)
    }
}

pub(super) fn configured_path() -> Result<PathBuf, PlatformError> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| io("LOCALAPPDATA is unavailable"))?;
    let local = PathBuf::from(local);
    if !local.is_absolute() {
        return Err(io("LOCALAPPDATA must be absolute"));
    }
    Ok(local.join("Ottrin").join("State").join("copies"))
}

fn child(
    parent: &File,
    name: &OsStr,
    directory: bool,
    access: u32,
    sharing: u32,
) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            parent,
            name,
            windows_fs::RelativeOptions {
                directory: Some(directory),
                create_new: false,
                access,
                sharing,
                security: ptr::null(),
            },
        )
    }
}

/// Other Copy writers must not invalidate an interrupted cohort's frozen
/// catalog. This is a refusal gate, never authority to mutate or repair state.
pub(super) fn require_uncoordinated(central: &File) -> Result<(), PlatformError> {
    let path = windows_fs::location(central).map_err(io)?;
    let parent = windows_fs::open_directory(
        path.parent()
            .ok_or_else(|| io("Copy registry has no parent"))?,
        FILE_GENERIC_READ,
    )
    .map_err(io)?;
    match child(
        &parent,
        OsStr::new(COHORTS_ROOT),
        true,
        FILE_GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    ) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(error)),
        Ok(file) => drop(file),
    }
    let manager = AnchoredRoot::open_at(&path, false)?
        .ok_or_else(|| io("Copy registry admission is missing"))?;
    if stable_identity_from_handle(manager.root()).map_err(io)?
        != stable_identity_from_handle(central).map_err(io)?
    {
        return Err(io(
            "Copy registry changed while checking interrupted cleanup",
        ));
    }
    let coordinator = manager
        .cohorts(false)?
        .ok_or_else(|| io("Copy coordinator admission is missing"))?;
    if !windows_fs::directory_children_limited(coordinator.root(), 1)
        .map_err(io)?
        .is_empty()
    {
        return Err(PlatformError::CopyCleanupRefused(
            "Finish the interrupted Copy cleanup in Activity before starting another Copy or recovery action.".into(),
        ));
    }
    manager.validate()
}

fn parent_directory(path: &Path, initialize: bool) -> Result<Option<File>, PlatformError> {
    let access = if initialize {
        FILE_GENERIC_READ | FILE_GENERIC_WRITE
    } else {
        FILE_GENERIC_READ
    };
    match windows_fs::open_directory(path, access) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !initialize => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| io("Registry parent has no existing ancestor"))?;
            let name = path
                .file_name()
                .ok_or_else(|| io("Registry parent has no native name"))?;
            let parent = parent_directory(parent, true)?.unwrap();
            let file = match windows_fs::create_private(
                &parent,
                name,
                true,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => child(
                    &parent,
                    name,
                    true,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .map_err(io)?,
                Err(error) => return Err(io(error)),
            };
            let identity = stable_identity_from_handle(&file).map_err(io)?;
            if !identity.is_directory_without_reparse() {
                return Err(io("Registry parent is a reparse entry"));
            }
            namespace::validate_binding(&parent, name, identity)?;
            parent.sync_all().map_err(io)?;
            Ok(Some(file))
        }
        Err(error) => Err(io(error)),
    }
}

impl AnchoredRoot {
    /// The engine must independently admit this held parent and reject source
    /// overlap before initialization. No caller-supplied path, SID or saved ACL
    /// selects authority. Missing discovery never provisions state.
    pub(crate) fn for_caller(
        parent: File,
        caller: Arc<crate::windows_privileged_transport::CallerCopyContext>,
        initialize: bool,
    ) -> Result<Option<Self>, PlatformError> {
        Self::open_under(
            parent,
            OsStr::new("copies"),
            OsStr::new(ANCHOR),
            None,
            initialize,
            Authority::Caller(caller),
        )
    }

    /// Read-only admission never initializes directories, anchors or journals.
    /// Existing unanchored/partial/foreign state is retained and refused.
    pub(super) fn open_at(path: &Path, initialize: bool) -> Result<Option<Self>, PlatformError> {
        if !path.is_absolute()
            || path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(io(
                "Copy private root requires an unambiguous absolute path",
            ));
        }
        let name = path
            .file_name()
            .ok_or_else(|| io("Copy private root has no native leaf"))?;
        windows_fs::valid_component(name).map_err(io)?;
        let parent_path = path
            .parent()
            .ok_or_else(|| io("Copy private root has no parent"))?;
        let Some(parent) = parent_directory(parent_path, initialize)? else {
            return Ok(None);
        };
        Self::open_under(
            parent,
            name,
            OsStr::new(ANCHOR),
            None,
            initialize,
            Authority::Process,
        )
    }

    /// A destination-local namespace lets the filesystem arbitrate target names
    /// using this volume's own name rules, independently of recovery registries.
    /// Missing roots are not created by read-only observation.
    pub(super) fn claims(parent: &File, initialize: bool) -> Result<Option<Self>, PlatformError> {
        let expected = windows_fs::directory_case_sensitive(parent).map_err(io)?;
        Self::open_under(
            parent.try_clone().map_err(io)?,
            OsStr::new(CLAIMS_ROOT),
            OsStr::new(CLAIMS_ANCHOR),
            Some(expected),
            initialize,
            Authority::Process,
        )
    }

    /// Coordinator writes must never change the guarded registry's namespace
    /// or epoch. Its sibling has a distinct anchor and no deletion authority
    /// until a cohort record binds both admitted roots.
    pub(super) fn cohorts(&self, initialize: bool) -> Result<Option<Self>, PlatformError> {
        self.validate()?;
        let parent = if initialize {
            self.authority.writable_parent(&self.parent_location)?
        } else {
            self.parent.try_clone().map_err(io)?
        };
        if stable_identity_from_handle(&parent).map_err(io)? != self.record.parent
            || security::owner_group_dacl_digest(&parent)? != self.parent_security
        {
            return Err(io(
                "Copy coordinator parent differs from registry admission",
            ));
        }
        let result = Self::open_under(
            parent,
            OsStr::new(COHORTS_ROOT),
            OsStr::new(COHORTS_ANCHOR),
            Some(self.cases.0),
            initialize,
            self.authority.clone(),
        )?;
        self.validate()?;
        Ok(result)
    }

    pub(super) fn claim_bucket(
        parent: &File,
        short: bool,
        initialize: bool,
    ) -> Result<Option<Self>, PlatformError> {
        let name = if short {
            "short-names.ottrin-claims"
        } else {
            "long-names.ottrin-claims"
        };
        let marker = format!("{name}.root");
        let expected = windows_fs::directory_case_sensitive(parent).map_err(io)?;
        Self::open_under(
            parent.try_clone().map_err(io)?,
            OsStr::new(name),
            OsStr::new(&marker),
            Some(expected),
            initialize,
            Authority::Process,
        )
    }

    fn open_under(
        parent: File,
        name: &OsStr,
        anchor_name: &OsStr,
        expected_case: Option<bool>,
        initialize: bool,
        authority: Authority,
    ) -> Result<Option<Self>, PlatformError> {
        authority.validate()?;
        if !stable_identity_from_handle(&parent)
            .map_err(io)?
            .is_directory_without_reparse()
        {
            return Err(io("Copy anchored root requires an ordinary parent"));
        }
        windows_fs::valid_component(name).map_err(io)?;
        windows_fs::valid_component(anchor_name).map_err(io)?;
        if name == anchor_name {
            return Err(io("Copy root and anchor names must differ"));
        }
        let parent_location = windows_fs::location(&parent).map_err(io)?;
        let creator = authority.owner()?;
        let marker = match authority.child(
            &parent,
            anchor_name,
            false,
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match authority.probe(&parent, name) {
                    Ok(_) => {
                        return Err(io(
                            "Copy private root exists without its identity anchor; retained for review",
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(io(error)),
                }
                if !initialize {
                    return Ok(None);
                }
                // Reserve the external anchor before creating the registry. A
                // partial setup never authorizes adoption or automatic deletion.
                let mut marker = authority
                    .create(&parent, anchor_name, false, FILE_SHARE_READ)
                    .map_err(io)?;
                let result = (|| {
                    parent.sync_all().map_err(io)?;
                    let root = authority
                        .create(
                            &parent,
                            name,
                            true,
                            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        )
                        .map_err(io)?;
                    if let Some(expected) = expected_case {
                        windows_fs::set_empty_directory_case_policy(&root, expected).map_err(io)?;
                    }
                    let record = Anchor {
                        protocol: "OTCRW02".into(),
                        case_policy: expected_case,
                        parent: stable_identity_from_handle(&parent).map_err(io)?,
                        marker: stable_identity_from_handle(&marker).map_err(io)?,
                        root: stable_identity_from_handle(&root).map_err(io)?,
                        name: name.encode_wide().collect(),
                        creator: creator.clone(),
                        security: security::PrivateSecurity::capture(&root, &creator)?,
                    };
                    serde_json::to_writer(&mut marker, &record).map_err(io)?;
                    marker.sync_all().map_err(io)?;
                    root.sync_all().map_err(io)?;
                    parent.sync_all().map_err(io)
                })();
                result.map_err(|error| PlatformError::CleanupPending {transaction:parent_location.display().to_string(),message:format!("Copy private root setup needs review: {error}; anchor and any directory retained")})?;
                drop(marker);
                authority
                    .child(
                        &parent,
                        anchor_name,
                        false,
                        FILE_GENERIC_READ,
                        FILE_SHARE_READ,
                    )
                    .map_err(io)?
            }
            Err(error) => return Err(io(error)),
        };
        let marker_id = stable_identity_from_handle(&marker).map_err(io)?;
        if !marker_id.is_regular_file_without_reparse() {
            return Err(io("Copy private root anchor is not an ordinary file"));
        }
        namespace::validate_single_link(&marker)?;
        let marker_security = security::PrivateSecurity::capture(&marker, &creator)?;
        let mut bytes = Vec::new();
        (&marker)
            .take(MAX_ANCHOR + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > MAX_ANCHOR {
            return Err(io("Copy private root anchor exceeds its bound"));
        }
        let record: Anchor = serde_json::from_slice(&bytes).map_err(|error| {
            io(format!(
                "Copy private root anchor is incomplete or invalid; retained for review: {error}"
            ))
        })?;
        if record.protocol != "OTCRW02"
            || record.parent != stable_identity_from_handle(&parent).map_err(io)?
            || record.marker != marker_id
            || record.name != name.encode_wide().collect::<Vec<_>>()
            || record.creator != creator
        {
            return Err(io(
                "Copy private root anchor identity or creator changed; retained for review",
            ));
        }
        if record.case_policy != expected_case {
            return Err(io(
                "Copy private root naming policy differs from its anchor; data retained",
            ));
        }
        let access = if initialize {
            FILE_GENERIC_READ | FILE_GENERIC_WRITE
        } else {
            FILE_GENERIC_READ
        };
        let root = authority
            .child(
                &parent,
                name,
                true,
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
            )
            .map_err(io)?;
        let result = Self {
            authority,
            parent_security: security::owner_group_dacl_digest(&parent)?,
            cases: (
                windows_fs::directory_case_sensitive(&parent).map_err(io)?,
                windows_fs::directory_case_sensitive(&root).map_err(io)?,
            ),
            root,
            anchor_name: anchor_name.to_os_string(),
            expected_case,
            marker,
            marker_security,
            parent,
            record,
            parent_location: parent_location.clone(),
            path: parent_location.join(name),
        };
        result.validate()?;
        Ok(Some(result))
    }
    pub(super) fn binding(&self) -> Result<Binding, PlatformError> {
        self.validate()?;
        Ok(Binding {
            anchor: self.record.clone(),
            marker_security: self.marker_security.clone(),
            parent_location: self.parent_location.as_os_str().encode_wide().collect(),
            parent_security: self.parent_security,
            cases: self.cases,
        })
    }

    /// A path is only a locator: fresh admission must match every original
    /// identity, ownership/security and naming-policy proof before reuse.
    pub(super) fn open_bound(path: &Path, binding: &Binding) -> Result<Self, PlatformError> {
        let root =
            Self::open_at(path, false)?.ok_or_else(|| io("Configured Copy registry is missing"))?;
        if root.binding()? != *binding {
            return Err(io("Copy registry differs from its original admission"));
        }
        Ok(root)
    }
    pub(super) fn parent_identity(&self) -> WindowsStableIdentity {
        self.record.parent
    }
    pub(super) fn anchor_identity(&self) -> WindowsStableIdentity {
        self.record.marker
    }
    pub fn root(&self) -> &File {
        &self.root
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    /// Reopen only this already anchored root with directory-flush rights.
    /// Discovery remains read-only; this never initializes or repairs state.
    pub fn writable_root(&self) -> Result<File, PlatformError> {
        self.validate()?;
        let root = self
            .authority
            .child(
                &self.parent,
                self.path.file_name().unwrap(),
                true,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
            )
            .map_err(io)?;
        if stable_identity_from_handle(&root).map_err(io)? != self.record.root {
            return Err(io(
                "Copy private root changed while acquiring directory-flush rights",
            ));
        }
        self.record.security.validate(&root, &self.record.creator)?;
        self.validate()?;
        Ok(root)
    }
    pub fn validate(&self) -> Result<(), PlatformError> {
        self.authority.validate()?;
        if !self.record.root.is_directory_without_reparse()
            || windows_fs::location(&self.parent).map_err(io)? != self.parent_location
            || stable_identity_from_handle(&self.parent).map_err(io)? != self.record.parent
            || stable_identity_from_handle(&self.root).map_err(io)? != self.record.root
            || stable_identity_from_handle(&self.marker).map_err(io)? != self.record.marker
            || self.authority.owner()? != self.record.creator
            || security::owner_group_dacl_digest(&self.parent)? != self.parent_security
            || windows_fs::directory_case_sensitive(&self.parent).map_err(io)? != self.cases.0
            || windows_fs::directory_case_sensitive(&self.root).map_err(io)? != self.cases.1
        {
            return Err(io("Copy anchored root binding changed"));
        }
        self.record
            .security
            .validate(&self.root, &self.record.creator)?;
        self.marker_security
            .validate(&self.marker, &self.record.creator)?;
        if let Some(expected) = self.expected_case
            && (windows_fs::directory_case_sensitive(&self.parent).map_err(io)? != expected
                || windows_fs::directory_case_sensitive(&self.root).map_err(io)? != expected)
        {
            return Err(io(
                "Copy claim namespace case policy changed; root retained without repair",
            ));
        }
        self.authority
            .binding(&self.parent, &self.anchor_name, self.record.marker)?;
        self.authority.binding(
            &self.parent,
            self.path.file_name().unwrap(),
            self.record.root,
        )
    }
}

#[cfg(test)]
impl AnchoredRoot {
    pub(crate) fn exercise_caller_authority(
        caller: &Arc<crate::windows_privileged_transport::CallerCopyContext>,
        protected_parent: &File,
    ) {
        use std::io::{Seek, SeekFrom, Write};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Wdk::Storage::FileSystem::{
            FILE_ACCESS_INFORMATION, FileAccessInformation, NtQueryInformationFile,
        };
        use windows_sys::Win32::Security::{
            Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };
        use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
        fn bytes(file: &File) -> Vec<u8> {
            let mut reader = file.try_clone().unwrap();
            reader.seek(SeekFrom::Start(0)).unwrap();
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).unwrap();
            bytes
        }
        fn widen(file: &File) {
            let policy =
                windows_fs::LocalSecurity::from_sddl("D:P(A;;FA;;;OW)(A;;FR;;;WD)").unwrap();
            let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorDacl(
                        policy.as_raw(),
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
        let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        for sensitive in [false, true] {
            let parent = windows_fs::create_private(
                protected_parent,
                OsStr::new(&format!("registry-authority-{sensitive} Ω")),
                true,
                sharing,
            )
            .unwrap();
            windows_fs::set_empty_directory_case_policy(&parent, sensitive).unwrap();
            let control = caller
                .create_private_entry(&parent, OsStr::new("naming-control"), true, sharing)
                .unwrap();
            assert_eq!(
                windows_fs::directory_case_sensitive(&control).unwrap(),
                sensitive
            );
            let before_policy = security::owner_group_dacl_digest(&control).unwrap();
            let value = windows_sys::Wdk::Storage::FileSystem::FILE_CASE_SENSITIVE_INFORMATION {
                Flags: if sensitive {
                    windows_sys::Win32::System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR
                } else {
                    0
                },
            };
            let mut status = IO_STATUS_BLOCK::default();
            let raw_set = unsafe {
                windows_sys::Wdk::Storage::FileSystem::NtSetInformationFile(
                    control.as_raw_handle().cast(),
                    &mut status,
                    ptr::from_ref(&value).cast(),
                    std::mem::size_of_val(&value) as u32,
                    windows_sys::Wdk::Storage::FileSystem::FileCaseSensitiveInformation,
                )
            };
            assert_eq!(
                unsafe { windows_sys::Win32::Foundation::RtlNtStatusToDosError(raw_set) },
                5,
                "Native same-policy write must reproduce the caller-owned coordinator failure"
            );
            windows_fs::set_empty_directory_case_policy(&control, sensitive).unwrap();
            assert!(
                windows_fs::set_empty_directory_case_policy(&control, !sensitive).is_err(),
                "A required policy change must still report native access denial"
            );
            assert_eq!(
                windows_fs::directory_case_sensitive(&control).unwrap(),
                sensitive
            );
            assert_eq!(
                security::owner_group_dacl_digest(&control).unwrap(),
                before_policy
            );
            let parent_id = stable_identity_from_handle(&parent).unwrap();
            let names_before = windows_fs::directory_children_limited(&parent, 8).unwrap();
            assert!(
                caller
                    .copy_registry_under(parent.try_clone().unwrap(), false)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                windows_fs::directory_children_limited(&parent, 8).unwrap(),
                names_before
            );
            let root = caller
                .copy_registry_under(parent.try_clone().unwrap(), true)
                .unwrap()
                .unwrap();
            let original_binding = root.binding().unwrap();
            let original_marker = bytes(&root.marker);
            let owner = caller.default_file_owner_sid().unwrap();
            let group = caller.default_file_group_sid().unwrap();
            assert_eq!(root.record.creator, owner);
            assert_ne!(root.record.creator, windows_fs::current_owner().unwrap());
            security::PrivateSecurity::capture_for_caller(root.root(), owner, group).unwrap();
            security::PrivateSecurity::capture_for_caller(&root.marker, owner, group).unwrap();
            let writer = root.writable_root().unwrap();
            writer.sync_all().unwrap();
            let mut info = FILE_ACCESS_INFORMATION::default();
            let mut status = IO_STATUS_BLOCK::default();
            assert_eq!(
                unsafe {
                    NtQueryInformationFile(
                        writer.as_raw_handle().cast(),
                        &mut status,
                        ptr::from_mut(&mut info).cast(),
                        std::mem::size_of_val(&info) as u32,
                        FileAccessInformation,
                    )
                },
                0
            );
            assert_eq!(
                info.AccessFlags & (DELETE | WRITE_DAC),
                0,
                "Registry flush handles must not acquire publication/delete rights"
            );
            assert_eq!(info.AccessFlags & FILE_GENERIC_WRITE, FILE_GENERIC_WRITE);
            assert!(root.cohorts(false).unwrap().is_none());
            let coordinator = root.cohorts(true).unwrap().unwrap();
            assert_eq!(coordinator.record.creator, owner);
            assert_eq!(coordinator.record.case_policy, Some(sensitive));
            assert_eq!(
                windows_fs::directory_case_sensitive(coordinator.root()).unwrap(),
                sensitive
            );
            security::PrivateSecurity::capture_for_caller(coordinator.root(), owner, group)
                .unwrap();
            coordinator.writable_root().unwrap().sync_all().unwrap();
            let coordinator_binding = coordinator.binding().unwrap();
            drop(coordinator);
            assert_eq!(
                root.cohorts(false).unwrap().unwrap().binding().unwrap(),
                coordinator_binding
            );
            assert!(
                windows_fs::directory_children_limited(root.root(), 1)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(root.binding().unwrap(), original_binding);
            drop((writer, root));
            let reopened = caller
                .copy_registry_under(parent.try_clone().unwrap(), false)
                .unwrap()
                .unwrap();
            assert_eq!(reopened.binding().unwrap(), original_binding);
            assert_eq!(bytes(&reopened.marker), original_marker);
            assert!(
                Self::open_at(reopened.path(), false).is_err(),
                "A helper's ambient identity must not admit a caller registry"
            );
            assert_eq!(stable_identity_from_handle(&parent).unwrap(), parent_id);
            let policy_handle = reopened
                .authority
                .child(
                    &parent,
                    OsStr::new("copies"),
                    true,
                    FILE_GENERIC_READ | WRITE_DAC,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                )
                .unwrap();
            widen(&policy_handle);
            let altered = security::owner_group_dacl_digest(reopened.root()).unwrap();
            assert!(reopened.validate().is_err());
            assert!(reopened.writable_root().is_err());
            drop(reopened);
            for initialize in [false, true] {
                assert!(
                    caller
                        .copy_registry_under(parent.try_clone().unwrap(), initialize)
                        .is_err()
                );
                assert_eq!(
                    security::owner_group_dacl_digest(&policy_handle).unwrap(),
                    altered
                );
            }
        }
        // Refuse unanchored caller-owned data without leaving a marker.
        let unanchored = windows_fs::create_private(
            protected_parent,
            OsStr::new("unanchored-registry"),
            true,
            sharing,
        )
        .unwrap();
        let saved = caller
            .create_private_entry(&unanchored, OsStr::new("copies"), true, sharing)
            .unwrap();
        let mut saved_file = caller
            .create_private_entry(&saved, OsStr::new("user-data"), false, sharing)
            .unwrap();
        saved_file
            .write_all(b"preserve unanchored caller data")
            .unwrap();
        saved_file.sync_all().unwrap();
        for initialize in [false, true] {
            assert!(
                caller
                    .copy_registry_under(unanchored.try_clone().unwrap(), initialize)
                    .is_err()
            );
            assert_eq!(bytes(&saved_file), b"preserve unanchored caller data");
            assert_eq!(
                windows_fs::directory_children_limited(&unanchored, 8).unwrap(),
                vec![OsString::from("copies")]
            );
        }
        // Existing helper-owned anchors/roots cannot be adopted by the caller.
        let foreign = windows_fs::create_private(
            protected_parent,
            OsStr::new("foreign-registry"),
            true,
            sharing,
        )
        .unwrap();
        let foreign_root = Self::open_under(
            foreign.try_clone().unwrap(),
            OsStr::new("copies"),
            OsStr::new(ANCHOR),
            None,
            true,
            Authority::Process,
        )
        .unwrap()
        .unwrap();
        let foreign_binding = foreign_root.binding().unwrap();
        let foreign_marker = bytes(&foreign_root.marker);
        for initialize in [false, true] {
            assert!(
                caller
                    .copy_registry_under(foreign.try_clone().unwrap(), initialize)
                    .is_err()
            );
            assert_eq!(foreign_root.binding().unwrap(), foreign_binding);
            assert_eq!(bytes(&foreign_root.marker), foreign_marker);
        }
        caller.validate().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn windows_native_copy_registry_binding_survives_reopen_and_refuses_case_drift() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("copies");
        let root = AnchoredRoot::open_at(&path, true).unwrap().unwrap();
        let binding = root.binding().unwrap();
        let encoded = serde_json::to_vec(&binding).unwrap();
        let decoded: Binding = serde_json::from_slice(&encoded).unwrap();
        drop(root);
        let root = AnchoredRoot::open_bound(&path, &decoded).unwrap();
        let writer = root.writable_root().unwrap();
        windows_fs::set_empty_directory_case_policy(&writer, !binding.cases.1).unwrap();
        assert!(root.validate().is_err());
        assert!(root.writable_root().is_err());
        drop((writer, root));
        assert!(AnchoredRoot::open_bound(&path, &decoded).is_err());
        let current = AnchoredRoot::open_at(&path, false).unwrap().unwrap();
        assert_eq!(current.binding().unwrap().cases.1, !binding.cases.1);
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
    }

    #[test]
    fn windows_native_copy_registry_binding_refuses_self_consistent_new_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("copies");
        let root = AnchoredRoot::open_at(&path, true).unwrap().unwrap();
        let binding = root.binding().unwrap();
        let mut record = root.record.clone();
        drop(root);
        let parent =
            windows_fs::open_directory(temp.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                .unwrap();
        let replacement = temp.path().join("replacement");
        let mut marker =
            windows_fs::create_private(&parent, OsStr::new("replacement"), false, FILE_SHARE_READ)
                .unwrap();
        record.marker = stable_identity_from_handle(&marker).unwrap();
        assert_ne!(record.marker, binding.anchor.marker);
        serde_json::to_writer(&mut marker, &record).unwrap();
        marker.sync_all().unwrap();
        drop(marker);
        std::fs::remove_file(temp.path().join(ANCHOR)).unwrap();
        std::fs::rename(replacement, temp.path().join(ANCHOR)).unwrap();
        parent.sync_all().unwrap();
        // The replacement is internally valid, but cannot extend a previous
        // cleanup observation's authority even though the registry is unchanged.
        let fresh = AnchoredRoot::open_at(&path, false).unwrap().unwrap();
        assert_eq!(fresh.record.root, binding.anchor.root);
        assert!(AnchoredRoot::open_bound(&path, &binding).is_err());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        assert_eq!(
            std::fs::read(temp.path().join(ANCHOR)).unwrap(),
            serde_json::to_vec(&record).unwrap()
        );
    }

    #[test]
    fn windows_native_copy_claim_namespace_uses_native_volume_name_rules() {
        use std::os::windows::ffi::OsStringExt;
        for sensitive in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let parent =
                windows_fs::open_directory(temp.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                    .unwrap();
            windows_fs::set_empty_directory_case_policy(&parent, sensitive).unwrap();
            assert!(AnchoredRoot::claims(&parent, false).unwrap().is_none());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
            let claims = AnchoredRoot::claims(&parent, true).unwrap().unwrap();
            assert_eq!(
                windows_fs::directory_case_sensitive(claims.root()).unwrap(),
                sensitive
            );
            let id = stable_identity_from_handle(claims.root()).unwrap();
            let pairs = [
                (
                    OsString::from("Report.txt"),
                    OsString::from("report.txt"),
                    !sensitive,
                ),
                (
                    OsString::from("Äpfel.txt"),
                    OsString::from("äpfel.txt"),
                    !sensitive,
                ),
                (
                    OsString::from("é.txt"),
                    OsString::from("e\u{301}.txt"),
                    false,
                ),
                (
                    OsString::from_wide(&[0xd800, 65]),
                    OsString::from_wide(&[0xd800, 97]),
                    !sensitive,
                ),
            ];
            for (first, second, collision) in pairs {
                let mut file =
                    windows_fs::create_private(claims.root(), &first, false, FILE_SHARE_READ)
                        .unwrap();
                file.write_all(b"owned name").unwrap();
                file.sync_all().unwrap();
                claims.root().sync_all().unwrap();
                drop(file);
                let next =
                    windows_fs::create_private(claims.root(), &second, false, FILE_SHARE_READ);
                if collision {
                    assert_eq!(next.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
                } else {
                    drop(next.unwrap());
                }
                claims.validate().unwrap();
            }
            drop(claims);
            let reopened = AnchoredRoot::claims(&parent, false).unwrap().unwrap();
            assert_eq!(stable_identity_from_handle(reopened.root()).unwrap(), id);
            assert_eq!(
                windows_fs::directory_case_sensitive(reopened.root()).unwrap(),
                sensitive
            );
            assert_eq!(
                std::fs::read(reopened.path().join("Report.txt")).unwrap(),
                b"owned name"
            );
        }
    }

    #[test]
    fn windows_native_copy_claim_namespace_refuses_policy_drift_and_unanchored_data() {
        for unanchored in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let parent =
                windows_fs::open_directory(temp.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                    .unwrap();
            windows_fs::set_empty_directory_case_policy(&parent, false).unwrap();
            if unanchored {
                std::fs::create_dir(temp.path().join(CLAIMS_ROOT)).unwrap();
                std::fs::write(
                    temp.path().join(CLAIMS_ROOT).join("user-data"),
                    b"preserve me",
                )
                .unwrap();
            } else {
                let claims = AnchoredRoot::claims(&parent, true).unwrap().unwrap();
                windows_fs::set_empty_directory_case_policy(claims.root(), true).unwrap();
                assert!(claims.validate().is_err());
            }
            let anchor = std::fs::read(temp.path().join(CLAIMS_ANCHOR)).ok();
            let count = std::fs::read_dir(temp.path()).unwrap().count();
            for initialize in [false, true] {
                assert!(AnchoredRoot::claims(&parent, initialize).is_err());
            }
            assert_eq!(std::fs::read(temp.path().join(CLAIMS_ANCHOR)).ok(), anchor);
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), count);
            if unanchored {
                assert_eq!(
                    std::fs::read(temp.path().join(CLAIMS_ROOT).join("user-data")).unwrap(),
                    b"preserve me"
                );
            } else {
                let root =
                    windows_fs::open_directory(&temp.path().join(CLAIMS_ROOT), FILE_GENERIC_READ)
                        .unwrap();
                assert!(windows_fs::directory_case_sensitive(&root).unwrap());
            }
        }
    }

    #[test]
    fn windows_native_copy_claim_namespace_arbitrates_separate_processes() {
        use std::{
            process::Command,
            time::{Duration, Instant},
        };
        const ROOT: &str = "OTTRIN_TEST_CLAIM_NAMESPACE_ROOT";
        const SELF: &str = "copy_transaction::windows::registry::tests::windows_native_copy_claim_namespace_arbitrates_separate_processes";
        if let Some(path) = std::env::var_os(ROOT) {
            let path = PathBuf::from(path);
            let id = std::env::var("OTTRIN_TEST_CLAIM_ACTOR").unwrap();
            assert!(matches!(id.as_str(), "one" | "two"));
            let target = std::env::var_os("OTTRIN_TEST_CLAIM_TARGET").unwrap();
            let parent =
                windows_fs::open_directory(&path, FILE_GENERIC_READ | FILE_GENERIC_WRITE).unwrap();
            let claims = AnchoredRoot::claims(&parent, false).unwrap().unwrap();
            std::fs::write(path.join(format!("{id}.ready")), b"ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !path.join("go").try_exists().unwrap() {
                assert!(
                    Instant::now() < deadline,
                    "parent did not release native claim actors"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            claims.validate().unwrap();
            let (code, _claim) =
                match windows_fs::create_private(claims.root(), &target, false, FILE_SHARE_READ) {
                    Ok(mut file) => {
                        file.write_all(id.as_bytes()).unwrap();
                        file.sync_all().unwrap();
                        claims.writable_root().unwrap().sync_all().unwrap();
                        (86, Some(file))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (87, None),
                    Err(error) => panic!("native claim create failed: {error}"),
                };
            // Exercise kernel handle teardown without Rust destructors.
            std::process::exit(code);
        }
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                }
                let _ = self.0.wait();
            }
        }
        for (sensitive, second, expected) in [
            (false, "report.txt", vec![86, 87]),
            (true, "report.txt", vec![86, 86]),
            (false, "Independent.txt", vec![86, 86]),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let parent =
                windows_fs::open_directory(temp.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                    .unwrap();
            windows_fs::set_empty_directory_case_policy(&parent, sensitive).unwrap();
            let claims = AnchoredRoot::claims(&parent, true).unwrap().unwrap();
            let mut children: Vec<_> = [("one", "Report.txt"), ("two", second)]
                .into_iter()
                .map(|(id, target)| {
                    OwnedChild(
                        Command::new(std::env::current_exe().unwrap())
                            .args(["--exact", SELF, "--nocapture"])
                            .env(ROOT, temp.path())
                            .env("OTTRIN_TEST_CLAIM_ACTOR", id)
                            .env("OTTRIN_TEST_CLAIM_TARGET", target)
                            .stdout(std::process::Stdio::null())
                            .spawn()
                            .unwrap(),
                    )
                })
                .collect();
            let deadline = Instant::now() + Duration::from_secs(30);
            while !temp.path().join("one.ready").try_exists().unwrap()
                || !temp.path().join("two.ready").try_exists().unwrap()
            {
                assert!(
                    Instant::now() < deadline,
                    "native claim actors did not become ready"
                );
                for child in &mut children {
                    assert!(
                        child.0.try_wait().unwrap().is_none(),
                        "claim actor exited before barrier"
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            std::fs::write(temp.path().join("go"), b"go").unwrap();
            let mut codes = Vec::new();
            for child in &mut children {
                loop {
                    if let Some(status) = child.0.try_wait().unwrap() {
                        codes.push(status.code().unwrap());
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "native claim actor did not finish"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            codes.sort();
            assert_eq!(codes, expected);
            claims.validate().unwrap();
            let names = windows_fs::directory_children(claims.root()).unwrap();
            assert_eq!(names.len(), if expected == [86, 87] { 1 } else { 2 });
            for name in names {
                let bytes = std::fs::read(claims.path().join(&name)).unwrap();
                assert!(bytes == b"one" || bytes == b"two");
                let error =
                    windows_fs::create_private(claims.root(), &name, false, FILE_SHARE_READ)
                        .unwrap_err();
                assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
            }
        }
    }

    #[test]
    fn windows_native_copy_registry_provisions_once_and_readers_hold_the_binding() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("profile/Ottrin/State/copies");
        assert!(AnchoredRoot::open_at(&path, false).unwrap().is_none());
        assert!(!root.path().join("profile").exists());
        let writer = AnchoredRoot::open_at(&path, true).unwrap().unwrap();
        let id = stable_identity_from_handle(writer.root()).unwrap();
        let marker = std::fs::read(path.parent().unwrap().join(ANCHOR)).unwrap();
        let reader = AnchoredRoot::open_at(&path, false).unwrap().unwrap();
        assert_eq!(stable_identity_from_handle(reader.root()).unwrap(), id);
        reader.validate().unwrap();
        assert!(std::fs::rename(&path, path.with_file_name("moved")).is_err());
        assert!(!path.with_file_name("moved").exists());
        drop((reader, writer));
        let next = AnchoredRoot::open_at(&path, true).unwrap().unwrap();
        assert_eq!(stable_identity_from_handle(next.root()).unwrap(), id);
        assert_eq!(
            std::fs::read(path.parent().unwrap().join(ANCHOR)).unwrap(),
            marker
        );
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    }

    #[test]
    fn windows_native_copy_registry_rejects_replaced_root_marker_and_parent() {
        for case in ["root", "marker", "parent"] {
            let root = tempfile::tempdir().unwrap();
            let parent = root.path().join("state");
            let path = parent.join("copies");
            drop(AnchoredRoot::open_at(&path, true).unwrap().unwrap());
            std::fs::write(path.join("user-data"), b"preserved").unwrap();
            let marker = std::fs::read(parent.join(ANCHOR)).unwrap();
            match case {
                "root" => {
                    std::fs::rename(&path, parent.join("saved")).unwrap();
                    let owner =
                        windows_fs::open_directory(&parent, FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                            .unwrap();
                    drop(
                        windows_fs::create_private(
                            &owner,
                            OsStr::new("copies"),
                            true,
                            FILE_SHARE_READ,
                        )
                        .unwrap(),
                    );
                }
                "marker" => {
                    std::fs::rename(parent.join(ANCHOR), parent.join("saved-marker")).unwrap();
                    let owner =
                        windows_fs::open_directory(&parent, FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                            .unwrap();
                    let mut replacement = windows_fs::create_private(
                        &owner,
                        OsStr::new(ANCHOR),
                        false,
                        FILE_SHARE_READ,
                    )
                    .unwrap();
                    replacement.write_all(&marker).unwrap();
                    replacement.sync_all().unwrap();
                }
                "parent" => {
                    std::fs::rename(&parent, root.path().join("saved-state")).unwrap();
                    std::fs::create_dir(&parent).unwrap();
                    std::fs::rename(root.path().join("saved-state/copies"), &path).unwrap();
                    std::fs::rename(
                        root.path().join("saved-state").join(ANCHOR),
                        parent.join(ANCHOR),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            for initialize in [false, true] {
                assert!(AnchoredRoot::open_at(&path, initialize).is_err(), "{case}");
            }
            assert_eq!(std::fs::read(parent.join(ANCHOR)).unwrap(), marker);
            let saved = if case == "root" {
                parent.join("saved/user-data")
            } else {
                path.join("user-data")
            };
            assert_eq!(std::fs::read(saved).unwrap(), b"preserved");
        }
    }

    #[test]
    fn windows_native_copy_registry_refuses_unanchored_partial_and_foreign_state_without_repair() {
        for case in ["unanchored", "partial", "missing-root", "permissions"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("copies");
            if case == "unanchored" {
                std::fs::create_dir(&path).unwrap();
                std::fs::write(path.join("user-data"), b"retained").unwrap();
            } else if case == "partial" {
                let parent =
                    windows_fs::open_directory(root.path(), FILE_GENERIC_READ | FILE_GENERIC_WRITE)
                        .unwrap();
                let mut marker =
                    windows_fs::create_private(&parent, OsStr::new(ANCHOR), false, FILE_SHARE_READ)
                        .unwrap();
                marker.write_all(b"{\"protocol\":").unwrap();
                marker.sync_all().unwrap();
            } else {
                drop(AnchoredRoot::open_at(&path, true).unwrap().unwrap());
                if case == "missing-root" {
                    std::fs::rename(&path, root.path().join("saved")).unwrap();
                } else {
                    super::super::tests::set_dacl(&path, "D:P(A;OICI;FA;;;OW)(A;OICI;GR;;;WD)");
                }
            }
            let marker = std::fs::read(root.path().join(ANCHOR)).ok();
            let entries = std::fs::read_dir(root.path()).unwrap().count();
            for initialize in [false, true] {
                assert!(AnchoredRoot::open_at(&path, initialize).is_err(), "{case}");
            }
            assert_eq!(std::fs::read(root.path().join(ANCHOR)).ok(), marker);
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), entries);
            if case == "unanchored" {
                assert_eq!(std::fs::read(path.join("user-data")).unwrap(), b"retained");
            }
        }
    }

    #[test]
    fn windows_native_copy_registry_configured_dispatch_discovery_and_source_free_undo() {
        const ROOT: &str = "OTTRIN_TEST_CONFIGURED_COPY_REGISTRY";
        if let Some(root) = std::env::var_os(ROOT) {
            let root = PathBuf::from(root);
            assert_eq!(
                std::env::var_os("LOCALAPPDATA").unwrap(),
                root.join("profile")
            );
            let outside = tempfile::tempdir().unwrap();
            assert!(
                super::super::dispatch::copy_configured(
                    &root,
                    outside.path(),
                    crate::ConflictAction::Overwrite,
                    100,
                    |_| Ok(())
                )
                .is_err()
            );
            assert!(
                !root.join("profile").exists(),
                "Rejected Copy must not provision anything inside its source"
            );
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
            let source = root.join("source.txt");
            let destination = root.join("destination");
            std::fs::write(&source, b"configured copy").unwrap();
            std::fs::create_dir(&destination).unwrap();
            let receipt = super::super::dispatch::copy_configured(
                &source,
                &destination,
                crate::ConflictAction::Overwrite,
                100,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
            let entries = crate::discover_retained_copy_transactions().unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0].transaction_id.as_deref(),
                receipt.copy_transaction_id()
            );
            std::fs::remove_file(&source).unwrap();
            let mut plan = crate::restore_retained_copy_undo_plan(
                entries[0].transaction_id.as_deref().unwrap(),
            )
            .unwrap();
            plan.undo_next().unwrap();
            assert!(!destination.join("source.txt").exists());
            assert_eq!(
                std::fs::read(receipt.before_path()).unwrap(),
                b"configured copy"
            );
            assert_eq!(
                crate::discover_retained_copy_transactions().unwrap().len(),
                1
            );
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let output=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","copy_transaction::windows::registry::tests::windows_native_copy_registry_configured_dispatch_discovery_and_source_free_undo","--nocapture"])
            .env(ROOT,root.path()).env("LOCALAPPDATA",root.path().join("profile")).output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
