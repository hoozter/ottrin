//! Central Copy discovery and opaque Undo admission. Presentation fields grant no mutation authority.
use super::*;
use crate::windows_fs::open_directory;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::*,
};

pub(super) const MAX_REGISTRY_ENTRIES: usize = 10_000;
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
        let prepared = prepare_forward(&retained, None, progress)?;
        let location = windows_fs::location(&parent).map_err(io)?;
        Ok(Self {
            registry: windows_fs::location(&central).map_err(io)?,
            binding: Box::new(CopyResumeBinding {
                header: header.clone(),
                staged_digest: staged_digest(&state)?,
                original_retained: retained.retained_original_version().cloned(),
                pending_copy: retained.pending_copy_version().cloned(),
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

/// Bridge a freshly verified completion into the ordinary session receipt.
/// The persisted journals remain authoritative; paths alone grant no Undo.
pub(super) fn session_receipt(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<crate::PathMoveUndoReceipt, PlatformError> {
    let completed = CompletedCopy::open(parent, central, name, &mut progress)?;
    let state = &completed.retained.snapshot.state;
    let header = state.header.as_ref().unwrap();
    let location = windows_fs::location(parent).map_err(io)?;
    let registry = windows_fs::location(central).map_err(io)?;
    let plan = RetainedCopyUndoPlan::admit(&registry, &header.transaction, progress)?;
    if &plan.binding.header != header || plan.binding.completion_digest != completion_digest(state)?
    {
        return Err(io(
            "Copy completion changed while issuing the session receipt",
        ));
    }
    Ok(crate::PathMoveUndoReceipt {
        before_path: location.join(header.candidate_name()),
        after_path: location.join(OsString::from_wide(&header.target)),
        identity: crate::PathMoveIdentity {
            windows: state.final_versions[0].identity,
        },
        displaced_identity: state.original_retained.as_ref().map(|version| {
            crate::PathMoveIdentity {
                windows: version.identity,
            }
        }),
        retained_displaced_path: state
            .original
            .as_ref()
            .map(|_| location.join(header.original_name())),
        requirement: crate::PathMoveUndoRequirement::None,
        windows_copy: Some(plan),
    })
}

/// Opaque action admission, distinct from public presentation fields. No file
/// handles survive admission; execution reopens and checks the bound identities.
#[derive(Clone, Debug, PartialEq, Eq)]
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
        let (pending_copy, pending_original) = pending_undo_epochs(&parent, &snapshot)?;
        Ok(Self {
            registry: windows_fs::location(&central).map_err(io)?,
            binding: Box::new(CopyUndoBinding {
                header: header.clone(),
                completion_digest: completion_digest(&state)?,
                parent_location: location.clone(),
                pending_copy,
                pending_original,
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
    RetainingOriginal,
    PublicationPending,
    ApplyingPermissions,
    Finalizing,
    Completed,
    UndoPending,
    Undone,
    CleaningUp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsCopyRecoveryEvidence {
    /// Paired terminal state is verified; only private name release remains.
    PendingNameRelease,
    UnverifiedStage,
    VerifiedTree,
    /// The replacement was durably withdrawn; only the original is verified.
    VerifiedOriginal,
    /// Remaining cleanup payload or metadata is verified, not a mutation token.
    VerifiedCleanup,
    NeedsReview {
        reason: String,
    },
}
/// A temporary observation, never an Undo/cleanup token. Discovery retains no
/// file handles and never creates the configured registry or repairs its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsRetainedCopyStatus {
    /// Unrecognized recovery filenames have no admitted transaction identity.
    pub transaction_id: Option<String>,
    pub phase: Option<WindowsCopyPhase>,
    pub publication: Option<PathBuf>,
    pub retained: Option<PathBuf>,
    /// Last identity-bound root location observed, not mutation authority.
    pub owned_location: Option<PathBuf>,
    /// Recovery metadata location: stage, or central record during cleanup.
    pub transaction: Option<PathBuf>,
    pub source_label: Option<OsString>,
    pub evidence: WindowsCopyRecoveryEvidence,
}
pub(super) fn phase(state: &State) -> WindowsCopyPhase {
    if state.cleanup.is_some() {
        WindowsCopyPhase::CleaningUp
    } else if state.undo_complete() {
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
    } else if state.original_retain_intent && state.original_retained.is_none() {
        WindowsCopyPhase::RetainingOriginal
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
pub(super) fn registry_id(name: &OsStr) -> Option<String> {
    let text = name.to_str()?.strip_suffix(".copy-journal")?;
    (text.len() == 32
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| text.to_owned())
}
/// Recognize possible journal claims without lossy UTF-16 conversion or
/// silently treating a case-variant/corrupt filename as unrelated user data.
pub(super) fn registry_candidate(name: &OsStr) -> bool {
    let units: Vec<_> = name.encode_wide().collect();
    let suffix = b".copy-journal";
    units.len() >= suffix.len()
        && units[units.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(unit, byte)| {
                let folded = if (65..=90).contains(unit) {
                    *unit + 32
                } else {
                    *unit
                };
                folded == u16::from(*byte)
            })
}

fn registered_journal(central: &File, name: &OsStr) -> Result<(Vec<u8>, State), PlatformError> {
    let record = RegisteredJournal::open(central, name)?;
    Ok((record.bytes, record.state))
}

/// A dependency observation keeps its read-only lease alive, so a peer cannot
/// replace or append to the record while another transaction reclaims payloads.
pub(super) struct RegisteredJournal {
    pub(super) file: File,
    pub(super) bytes: Vec<u8>,
    pub(super) state: State,
}
impl RegisteredJournal {
    pub(super) fn open(central: &File, name: &OsStr) -> Result<Self, PlatformError> {
        Self::with_access(central, name, FILE_GENERIC_READ)
    }
    pub(super) fn dependency(central: &File, name: &OsStr) -> Result<Self, PlatformError> {
        // DELETE participates in Windows' closed-handle writable-mapping exclusion.
        // This control lease never calls a mutation API on the peer.
        Self::with_access(
            central,
            name,
            FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::DELETE,
        )
    }
    fn with_access(central: &File, name: &OsStr, access: u32) -> Result<Self, PlatformError> {
        registry_id(name).ok_or_else(|| io("invalid Copy transaction ID"))?;
        let file = unsafe {
            windows_fs::open_relative(
                central,
                name,
                RelativeOptions {
                    directory: Some(false),
                    create_new: false,
                    access,
                    sharing: FILE_SHARE_READ,
                    security: ptr::null(),
                },
            )
        }
        .map_err(io)?;
        Self::from_file(central, name, file)
    }
    pub(super) fn from_file(
        central: &File,
        name: &OsStr,
        file: File,
    ) -> Result<Self, PlatformError> {
        let id = registry_id(name).ok_or_else(|| io("invalid Copy transaction ID"))?;
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
        Ok(Self {
            file,
            bytes,
            state: decoded.state,
        })
    }
}
struct VolumeCursor(HANDLE);
impl Drop for VolumeCursor {
    fn drop(&mut self) {
        unsafe {
            FindVolumeClose(self.0);
        }
    }
}
pub(super) struct Volumes {
    anchors: BTreeMap<u64, File>,
    enumerated: bool,
}
impl Volumes {
    pub(super) fn new(central: &File) -> Result<Self, PlatformError> {
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
    pub(super) fn resolve(
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
        let id = registry_id(&name);
        if id.is_none() && !registry_candidate(&name) {
            continue;
        }
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
        if status.transaction_id.is_none() {
            status.transaction = Some(windows_fs::location(&central).map_err(io)?.join(&name));
            status.evidence = WindowsCopyRecoveryEvidence::NeedsReview {
                reason: "Unrecognized Copy recovery filename; record preserved without adopting a transaction identity".into(),
            };
            output.push(status);
            continue;
        }
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
            if state.claim_terminal()
                && !state
                    .cleanup
                    .as_ref()
                    .is_some_and(|cleanup| cleanup.metadata_removal_intent)
            {
                let snapshot = Snapshot::load(
                    &parent,
                    &central,
                    &header.stage_name(),
                    SnapshotAccess::Read,
                )?;
                if snapshot.bytes != bytes {
                    return Err(io("Copy journals changed during release discovery"));
                }
                if snapshot
                    .claim
                    .as_ref()
                    .is_some_and(claims::Lease::needs_release)
                {
                    snapshot.revalidate(&parent, &central)?;
                    return Ok(WindowsCopyRecoveryEvidence::PendingNameRelease);
                }
            }
            if let Some(cleanup) = &state.cleanup {
                if cleanup.metadata_removal_intent {
                    super::cleanup_finalize::Plan::observe(
                        &parent,
                        &central,
                        &header.stage_name(),
                        &mut progress,
                    )?;
                } else {
                    super::cleanup::Admission::observe(
                        &parent,
                        &central,
                        &header.stage_name(),
                        &mut progress,
                    )?;
                }
                let (current, _) = registered_journal(&central, &name)?;
                if current != bytes {
                    return Err(io("Copy cleanup record changed during discovery"));
                }
                // The stage may already be gone. Its surviving authenticated
                // central record is the recovery location, never a stale path.
                status.transaction = Some(windows_fs::location(&central).map_err(io)?.join(&name));
                return Ok(WindowsCopyRecoveryEvidence::VerifiedCleanup);
            }
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
pub fn discover() -> Result<Vec<WindowsRetainedCopyStatus>, PlatformError> {
    let Some(registry) = super::super::registry::AnchoredRoot::open_at(
        &super::super::registry::configured_path()?,
        false,
    )?
    else {
        return Ok(Vec::new());
    };
    let result = discover_at(registry.path());
    registry.validate()?;
    result
}
pub(crate) fn restore_undo_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyUndoPlan, PlatformError> {
    RetainedCopyUndoPlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_undo(id: &str) -> Result<RetainedCopyUndoPlan, PlatformError> {
    let registry = super::super::registry::AnchoredRoot::open_at(
        &super::super::registry::configured_path()?,
        false,
    )?
    .ok_or_else(|| io("Configured Copy registry is missing"))?;
    let result = restore_undo_at(registry.path(), id);
    registry.validate()?;
    result
}

pub(crate) fn restore_resume_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyResumePlan, PlatformError> {
    RetainedCopyResumePlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_resume(id: &str) -> Result<RetainedCopyResumePlan, PlatformError> {
    let registry = super::super::registry::AnchoredRoot::open_at(
        &super::super::registry::configured_path()?,
        false,
    )?
    .ok_or_else(|| io("Configured Copy registry is missing"))?;
    let result = restore_resume_at(registry.path(), id);
    registry.validate()?;
    result
}
