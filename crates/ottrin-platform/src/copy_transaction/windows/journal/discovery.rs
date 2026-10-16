//! Central Copy discovery and opaque Undo admission. Presentation fields grant no mutation authority.
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

/// Source-free forward admission. Presentation fields cannot construct a plan.
/// This never resumes unverified staging or retains open file handles.
#[derive(Clone, Debug)]
pub struct RetainedCopyResumePlan {
    registry: PathBuf,
    binding: Box<CopyResumeBinding>,
    complete: bool,
    admitted_phase: WindowsCopyPhase,
    observed_location: PathBuf,
    publication: PathBuf,
    retention: PathBuf,
    original: Option<PathBuf>,
}
impl RetainedCopyResumePlan {
    pub(super) fn admit(
        root: &Path,
        id: &str,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let name = OsString::from(format!("{id}.copy-journal"));
        registry_id(&name).ok_or_else(|| io("invalid Copy transaction ID"))?;
        progress(0)?;
        let central = open_directory(root, FILE_GENERIC_READ).map_err(io)?;
        let (bytes, state) = registered_journal(&central, &name)?;
        if !state.verified || state.completion_verified || state.undo_intent {
            return Err(io("Copy is not a verified unfinished forward transaction"));
        }
        let header = state.header.as_ref().unwrap();
        let parent =
            Volumes::new(&central)?.resolve(header.parent, FILE_GENERIC_READ, &mut progress)?;
        let retained = RetainedStage::open(&parent, &central, &header.stage_name(), &mut progress)?;
        if retained.snapshot.bytes != bytes {
            return Err(io("Copy journals changed during forward admission"));
        }
        let prepared = prepare_publication(&retained, None, progress)?;
        let location = windows_fs::location(&parent).map_err(io)?;
        Ok(Self {
            registry: windows_fs::location(&central).map_err(io)?,
            binding: Box::new(CopyResumeBinding {
                header: header.clone(),
                staged_digest: staged_digest(&state)?,
                permissions_digest: super::super::evidence_digest(
                    &prepared.evidence().at_root(&header.candidate_name()),
                )?,
                parent_location: location.clone(),
            }),
            complete: false,
            admitted_phase: phase(&retained.snapshot.state),
            observed_location: location.join(retained.snapshot.root_name(&parent)?),
            publication: location.join(OsString::from_wide(&header.target)),
            retention: location.join(header.candidate_name()),
            original: state
                .original
                .as_ref()
                .map(|_| location.join(header.original_name())),
        })
    }
    pub fn transaction_id(&self) -> &str {
        &self.binding.header.transaction
    }
    pub fn path_pairs(&self) -> Vec<(&Path, &Path)> {
        let mut pairs = vec![(self.retention.as_path(), self.publication.as_path())];
        if let Some(original) = &self.original {
            pairs.push((original.as_path(), self.publication.as_path()));
        }
        pairs
    }
    pub fn admitted_phase(&self) -> WindowsCopyPhase {
        self.admitted_phase
    }
    pub fn observed_location(&self) -> &Path {
        &self.observed_location
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn resume(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.complete {
            return Err(io("Copy forward recovery is already complete"));
        }
        progress(0)?;
        let central = open_directory(&self.registry, FILE_GENERIC_READ).map_err(io)?;
        if stable_identity_from_handle(&central).map_err(io)? != self.binding.header.central {
            return Err(io("Copy registry was replaced; resume refused"));
        }
        let (_, state) = registered_journal(&central, &self.binding.header.central_name())?;
        if state.header.as_ref() != Some(&self.binding.header) {
            return Err(io("Copy journal was replaced; resume refused"));
        }
        let parent = Volumes::new(&central)?.resolve(
            self.binding.header.parent,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
            &mut progress,
        )?;
        resume_bound_copy(&parent, &central, &self.binding, progress)?;
        self.complete = true;
        Ok(())
    }
}

/// Opaque action admission, distinct from public presentation fields. No file
/// handles survive admission; execution reopens and checks the bound identities.
#[derive(Clone, Debug)]
pub struct RetainedCopyUndoPlan {
    registry: PathBuf,
    binding: Box<CopyUndoBinding>,
    publication: PathBuf,
    retention: PathBuf,
    original: Option<PathBuf>,
    remaining: bool,
    admitted_phase: WindowsCopyPhase,
    observed_location: PathBuf,
}
impl RetainedCopyUndoPlan {
    pub(crate) fn admit(
        root: &Path,
        transaction_id: &str,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let name = OsString::from(format!("{transaction_id}.copy-journal"));
        // Validate before any filesystem access, including relative components.
        registry_id(&name).ok_or_else(|| io("invalid Copy transaction ID"))?;
        progress(0)?;
        let central = open_directory(root, FILE_GENERIC_READ).map_err(io)?;
        let (bytes, state) = registered_journal(&central, &name)?;
        let header = state.header.as_ref().unwrap();
        let parent =
            Volumes::new(&central)?.resolve(header.parent, FILE_GENERIC_READ, &mut progress)?;
        let snapshot = read_undo_snapshot(&parent, &central, &header.stage_name(), &mut progress)?;
        if snapshot.bytes != bytes || !snapshot.state.completion_verified {
            return Err(io(
                "Copy has no stable completed receipt; Undo admission refused",
            ));
        }
        if snapshot.state.undo_complete() {
            return Err(io("Copy Undo is already complete"));
        }
        let location = windows_fs::location(&parent).map_err(io)?;
        let observed_name = if snapshot.state.undone.is_some() && snapshot.state.original.is_some()
        {
            HeldOriginal::open(&parent, &snapshot.state, &mut progress)?
                .unwrap()
                .name
        } else {
            snapshot.root_name(&parent)?
        };
        Ok(Self {
            registry: windows_fs::location(&central).map_err(io)?,
            binding: Box::new(CopyUndoBinding {
                header: header.clone(),
                completion_digest: completion_digest(&state)?,
                parent_location: location.clone(),
            }),
            publication: location.join(OsString::from_wide(&header.target)),
            retention: location.join(header.candidate_name()),
            original: state
                .original
                .as_ref()
                .map(|_| location.join(header.original_name())),
            remaining: true,
            admitted_phase: phase(&snapshot.state),
            observed_location: location.join(observed_name),
        })
    }
    pub fn path_pairs(&self) -> Vec<(&Path, &Path)> {
        let mut pairs = vec![(self.retention.as_path(), self.publication.as_path())];
        if let Some(original) = &self.original {
            pairs.push((original.as_path(), self.publication.as_path()));
        }
        pairs
    }
    pub fn transaction_ids(&self) -> Vec<&str> {
        vec![&self.binding.header.transaction]
    }
    pub fn remaining(&self) -> usize {
        usize::from(self.remaining)
    }
    /// Phase observed during read-only admission, before any inverse execution.
    pub fn admitted_phase(&self) -> WindowsCopyPhase {
        self.admitted_phase
    }
    /// Identity-bound data location observed at admission, never action authority.
    pub fn observed_location(&self) -> &Path {
        &self.observed_location
    }
    pub fn undo_next(&mut self) -> Result<(), PlatformError> {
        self.undo_next_with_progress(|_| Ok(()))
    }
    pub fn undo_next_with_progress(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if !self.remaining {
            return Err(io("Copy Undo is already complete"));
        }
        progress(0)?;
        let central = open_directory(&self.registry, FILE_GENERIC_READ).map_err(io)?;
        if stable_identity_from_handle(&central).map_err(io)? != self.binding.header.central {
            return Err(io("Copy registry was replaced; Undo refused"));
        }
        let (_, state) = registered_journal(&central, &self.binding.header.central_name())?;
        if state.header.as_ref() != Some(&self.binding.header) {
            return Err(io("Copy journal was replaced; Undo refused"));
        }
        let parent = Volumes::new(&central)?.resolve(
            self.binding.header.parent,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
            &mut progress,
        )?;
        undo_bound_copy(
            &parent,
            &central,
            &self.binding.header.stage_name(),
            Some(&self.binding),
            progress,
        )?;
        self.remaining = false;
        Ok(())
    }
}

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
    /// The replacement was durably withdrawn; only the original is verified.
    VerifiedOriginal,
    NeedsReview {
        reason: String,
    },
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
pub(super) fn phase(state: &State) -> WindowsCopyPhase {
    if state.undo_complete() {
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
fn registered_journal(central: &File, name: &OsStr) -> Result<(Vec<u8>, State), PlatformError> {
    let id = registry_id(name).ok_or_else(|| io("invalid Copy transaction ID"))?;
    let file = open(central, name, Some(false), FILE_GENERIC_READ)?;
    let actual = stable_identity_from_handle(&file).map_err(io)?;
    if !actual.is_regular_file_without_reparse() {
        return Err(io("Copy registry entry is not an ordinary journal"));
    }
    namespace::validate_single_link(&file)?;
    let bytes = read(&file)?;
    let decoded = decode(&bytes)?;
    let header = decoded.state.header.as_ref().unwrap();
    if header.transaction != id
        || header.central != stable_identity_from_handle(central).map_err(io)?
        || header.central_file != actual
        || header.creator != security::current_owner()?
    {
        return Err(io(
            "Copy central journal identity or creator does not match",
        ));
    }
    header.central_security.validate(&file, &header.creator)?;
    Ok((bytes, decoded.state))
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
        access: u32,
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
        windows_fs::open_directory_by_identity(anchor, expected, access).map_err(io)
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
            let (bytes, state) = registered_journal(&central, &name)?;
            let header = state.header.as_ref().unwrap();
            status.phase = Some(phase(&state));
            status.source_label = Some(OsString::from_wide(&header.source_label));
            let parent = volumes.resolve(header.parent, FILE_GENERIC_READ, &mut progress)?;
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
            if snapshot.state.undone.is_some() && snapshot.state.original.is_some() {
                let original = HeldOriginal::open(&parent, &snapshot.state, &mut progress)?
                    .ok_or_else(|| io("withdrawn overwrite has no original evidence"))?;
                status.owned_location = Some(location.join(&original.name));
                snapshot.revalidate(&parent, &central)?;
                return Ok(WindowsCopyRecoveryEvidence::VerifiedOriginal);
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
pub(crate) fn configured_root() -> Result<PathBuf, PlatformError> {
    let local =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| io("LOCALAPPDATA is unavailable"))?;
    Ok(PathBuf::from(local)
        .join("Ottrin")
        .join("State")
        .join("copies"))
}
pub fn discover() -> Result<Vec<WindowsRetainedCopyStatus>, PlatformError> {
    discover_at(&configured_root()?)
}
pub(crate) fn restore_undo_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyUndoPlan, PlatformError> {
    RetainedCopyUndoPlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_undo(id: &str) -> Result<RetainedCopyUndoPlan, PlatformError> {
    restore_undo_at(&configured_root()?, id)
}

pub(crate) fn restore_resume_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyResumePlan, PlatformError> {
    RetainedCopyResumePlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_resume(id: &str) -> Result<RetainedCopyResumePlan, PlatformError> {
    restore_resume_at(&configured_root()?, id)
}
