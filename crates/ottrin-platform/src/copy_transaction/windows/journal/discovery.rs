//! Read-only central Copy discovery. Presentation fields grant no mutation authority.
use super::*;
use std::{
    collections::BTreeMap,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};

const MAX_REGISTRY_ENTRIES: usize = 10_000;
const MAX_VOLUMES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsCopyPhase {
    Staging,
    Staged,
    PlanningPermissions,
    ReadyToPublish,
    PublicationPending,
    ApplyingPermissions,
    Finalizing,
    Completed,
    UndoPending,
    Undone,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsCopyRecoveryEvidence {
    UnverifiedStage,
    VerifiedTree,
    NeedsReview { reason: String },
}
/// A temporary observation, never an Undo/cleanup token. Discovery retains no
/// file handles and never creates the configured registry or repairs its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsRetainedCopyStatus {
    pub transaction_id: String,
    pub phase: Option<WindowsCopyPhase>,
    pub publication: Option<PathBuf>,
    pub retained: Option<PathBuf>,
    /// Last identity-bound root location observed, not mutation authority.
    pub owned_location: Option<PathBuf>,
    pub transaction: Option<PathBuf>,
    pub source_label: Option<OsString>,
    pub evidence: WindowsCopyRecoveryEvidence,
}
fn phase(state: &State) -> WindowsCopyPhase {
    if state.undone.is_some() {
        WindowsCopyPhase::Undone
    } else if state.undo_intent {
        WindowsCopyPhase::UndoPending
    } else if state.completion_verified {
        WindowsCopyPhase::Completed
    } else if state.native_complete() {
        WindowsCopyPhase::Finalizing
    } else if state.published {
        WindowsCopyPhase::ApplyingPermissions
    } else if state.publication_intent {
        WindowsCopyPhase::PublicationPending
    } else if state.planned.is_some() {
        WindowsCopyPhase::ReadyToPublish
    } else if state.planning.is_some() {
        WindowsCopyPhase::PlanningPermissions
    } else if state.verified {
        WindowsCopyPhase::Staged
    } else {
        WindowsCopyPhase::Staging
    }
}
fn registry_id(name: &OsStr) -> Option<String> {
    let text = name.to_str()?.strip_suffix(".copy-journal")?;
    (text.len() == 32
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| text.to_owned())
}
fn open_directory(path: &Path, access: u32) -> std::io::Result<File> {
    let file = std::fs::OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if !stable_identity_from_handle(&file)
        .map_err(std::io::Error::other)?
        .is_directory_without_reparse()
    {
        return Err(std::io::Error::other(
            "Copy registry/volume is not an ordinary directory",
        ));
    }
    Ok(file)
}
struct VolumeCursor(HANDLE);
impl Drop for VolumeCursor {
    fn drop(&mut self) {
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}
struct Volumes {
    anchors: BTreeMap<u64, File>,
    enumerated: bool,
}
impl Volumes {
    fn new(central: &File) -> Result<Self, PlatformError> {
        let mut anchors = BTreeMap::new();
        anchors.insert(
            stable_identity_from_handle(central)
                .map_err(io)?
                .volume_serial_number(),
            central.try_clone().map_err(io)?,
        );
        Ok(Self {
            anchors,
            enumerated: false,
        })
    }
    fn resolve(
        &mut self,
        expected: WindowsStableIdentity,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<File, PlatformError> {
        let serial = expected.volume_serial_number();
        if !self.anchors.contains_key(&serial) && !self.enumerated {
            self.enumerated = true;
            let mut name = [0u16; 1024];
            let raw = unsafe { FindFirstVolumeW(name.as_mut_ptr(), name.len() as u32) };
            if raw == INVALID_HANDLE_VALUE {
                return Err(io(std::io::Error::last_os_error()));
            }
            let _cursor = VolumeCursor(raw);
            for index in 0..MAX_VOLUMES {
                progress(0)?;
                let end = name
                    .iter()
                    .position(|ch| *ch == 0)
                    .ok_or_else(|| io("unterminated native volume name"))?;
                let path = PathBuf::from(OsString::from_wide(&name[..end]));
                // Names come from the local volume manager, never a journal path.
                if let Ok(file) = open_directory(&path, FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
                    let key = stable_identity_from_handle(&file)
                        .map_err(io)?
                        .volume_serial_number();
                    self.anchors.entry(key).or_insert(file);
                }
                if unsafe { FindNextVolumeW(raw, name.as_mut_ptr(), name.len() as u32) } == 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
                        return Err(io(error));
                    }
                    break;
                }
                if index + 1 == MAX_VOLUMES {
                    return Err(io("native recovery volume budget exceeded"));
                }
            }
        }
        let anchor = self
            .anchors
            .get(&serial)
            .ok_or_else(|| io("Copy destination volume is unavailable"))?;
        windows_fs::open_directory_by_identity(anchor, expected).map_err(io)
    }
}
/// Does not create directories, alter settings or consult cached destination paths.
pub fn discover_at(root: &Path) -> Result<Vec<WindowsRetainedCopyStatus>, PlatformError> {
    discover_at_with_progress(root, |_| Ok(()))
}
pub fn discover_at_with_progress(
    root: &Path,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Vec<WindowsRetainedCopyStatus>, PlatformError> {
    let cancelled = std::cell::Cell::new(false);
    let mut progress = |bytes| {
        let result = progress(bytes);
        if result.is_err() {
            cancelled.set(true);
        }
        result
    };
    progress(0)?;
    let central = match open_directory(root, FILE_GENERIC_READ) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(error)),
    };
    let identity = stable_identity_from_handle(&central).map_err(io)?;
    let mut volumes = Volumes::new(&central)?;
    let mut output = Vec::new();
    for name in
        windows_fs::directory_children_limited(&central, MAX_REGISTRY_ENTRIES).map_err(io)?
    {
        progress(0)?;
        let Some(id) = registry_id(&name) else {
            continue;
        };
        let mut status = WindowsRetainedCopyStatus {
            transaction_id: id,
            phase: None,
            publication: None,
            retained: None,
            owned_location: None,
            transaction: None,
            source_label: None,
            evidence: WindowsCopyRecoveryEvidence::NeedsReview {
                reason: "Copy journal was not inspected".into(),
            },
        };
        let result = (|| {
            let file = open(&central, &name, Some(false), FILE_GENERIC_READ)?;
            let actual = stable_identity_from_handle(&file).map_err(io)?;
            if !actual.is_regular_file_without_reparse() {
                return Err(io("Copy registry entry is not an ordinary journal"));
            }
            namespace::validate_single_link(&file)?;
            let bytes = read(&file)?;
            let decoded = decode(&bytes)?;
            let header = decoded.state.header.as_ref().unwrap();
            if header.transaction != status.transaction_id
                || header.central != identity
                || header.central_file != actual
                || header.creator != security::current_owner()?
            {
                return Err(io(
                    "Copy central journal identity or creator does not match",
                ));
            }
            header.central_security.validate(&file, &header.creator)?;
            status.phase = Some(phase(&decoded.state));
            status.source_label = Some(OsString::from_wide(&header.source_label));
            let parent = volumes.resolve(header.parent, &mut progress)?;
            let location = windows_fs::location(&parent).map_err(io)?;
            status.publication = Some(location.join(OsString::from_wide(&header.target)));
            status.retained = Some(location.join(header.candidate_name()));
            status.transaction = Some(location.join(header.stage_name()));
            let snapshot = Snapshot::load(
                &parent,
                &central,
                &header.stage_name(),
                SnapshotAccess::Read,
            )?;
            if snapshot.bytes != bytes {
                return Err(io("Copy mirror changed during discovery"));
            }
            if let Some(root) = snapshot
                .state
                .nodes
                .first()
                .and_then(|node| node.staged.as_ref())
            {
                let root_name = snapshot.root_name(&parent)?;
                bound(&parent, &root_name, root.candidate_identity)?;
                status.owned_location = Some(location.join(root_name));
            }
            if !snapshot.state.verified {
                snapshot.revalidate(&parent, &central)?;
                return Ok(WindowsCopyRecoveryEvidence::UnverifiedStage);
            }
            drop(snapshot);
            let retained =
                RetainedStage::open(&parent, &central, &header.stage_name(), &mut progress)?;
            if retained.snapshot.bytes != bytes {
                return Err(io("Copy journals changed during payload discovery"));
            }
            if retained.snapshot.state.permissions == security::PermissionProgress::default()
                && (retained.snapshot.state.planned.is_some()
                    || retained.snapshot.state.planning.is_some())
            {
                retained.derive_destination_security(|| progress(0))?;
            }
            Ok(WindowsCopyRecoveryEvidence::VerifiedTree)
        })();
        status.evidence = match result {
            Ok(evidence) => evidence,
            Err(error) if cancelled.get() => return Err(error),
            Err(error) => WindowsCopyRecoveryEvidence::NeedsReview {
                reason: error.to_string(),
            },
        };
        output.push(status);
    }
    Ok(output)
}
pub fn discover() -> Result<Vec<WindowsRetainedCopyStatus>, PlatformError> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| io("LOCALAPPDATA is unavailable"))?;
    discover_at(
        &PathBuf::from(local)
            .join("Ottrin")
            .join("State")
            .join("copies"),
    )
}
