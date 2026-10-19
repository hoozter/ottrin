//! Paired Copy staging/publication/permission records. Inspection never grants mutation.
use super::{
    EntryVersion, PlatformError, WindowsStableIdentity, io, links, metadata, namespace, security,
    stable_identity_from_handle, tree,
};
use crate::{
    move_recovery::NativePath,
    windows_fs::{self, RelativeOptions},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::windows::ffi::{OsStrExt, OsStringExt},
    ptr,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_COMPRESSED, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

mod claims;
mod cleanup;
mod cleanup_action;
mod initialization;
pub use cleanup_action::RetainedCopyCleanupPlan;
mod cleanup_dependencies;
mod cleanup_finalize;
mod cleanup_group;
mod cleanup_peer;
mod cleanup_protocol;
mod cleanup_tree;
mod discovery;
mod name_release;
pub use name_release::RetainedCopyNameReleasePlan;
pub(crate) use name_release::{restore_name_release, restore_name_release_at};
mod rename_creation;
pub use discovery::{
    RetainedCopyResumePlan, RetainedCopyUndoPlan, WindowsCopyPhase, WindowsCopyRecoveryEvidence,
    WindowsRetainedCopyStatus,
};
pub(crate) use discovery::{
    discover, discover_at, discover_at_with_progress, restore_resume, restore_resume_at,
    restore_undo, restore_undo_at,
};
use rename_creation::{RenameCreation, RenameKind, pending_undo_epochs};

const MAGIC: &[u8; 8] = b"OTCPWJ17";
const FRAME_HEADER: usize = 84;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_LOG: u64 = 64 * 1024 * 1024;
const LOCAL: &str = "journal";

#[cfg(not(test))]
fn log_budget() -> u64 {
    MAX_LOG
}
#[cfg(test)]
thread_local! {static LOG_BUDGET: std::cell::Cell<u64> = const { std::cell::Cell::new(MAX_LOG) };}
#[cfg(test)]
fn log_budget() -> u64 {
    LOG_BUDGET.with(|limit| limit.get())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalAdmission {
    count: usize,
    digest: [u8; 32],
}
/// The exact destination decision made before staging. No handles survive
/// admission; the complete original digest becomes part of both journal headers.
pub(super) struct DestinationAdmission {
    parent: WindowsStableIdentity,
    case_sensitive: bool,
    location: std::path::PathBuf,
    target: OsString,
    original: Option<(EntryVersion, OriginalAdmission)>,
    original_objects: BTreeSet<(u64, [u8; 16])>,
}
impl DestinationAdmission {
    fn at_parent(parent: &File, target: &OsStr) -> Result<Self, PlatformError> {
        windows_fs::valid_component(target).map_err(io)?;
        let parent_id = stable_identity_from_handle(parent).map_err(io)?;
        if !parent_id.is_directory_without_reparse() {
            return Err(io("Copy destination parent is not a real directory"));
        }
        Ok(Self {
            parent: parent_id,
            case_sensitive: windows_fs::directory_case_sensitive(parent).map_err(io)?,
            location: windows_fs::location(parent).map_err(io)?,
            target: target.to_os_string(),
            original: None,
            original_objects: BTreeSet::new(),
        })
    }
    pub(super) fn creation(parent: &File, target: &OsStr) -> Result<Self, PlatformError> {
        let result = Self::at_parent(parent, target)?;
        result.validate(parent)?;
        Ok(result)
    }
    pub(super) fn overwrite(
        parent: &File,
        target: &OsStr,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let mut result = Self::at_parent(parent, target)?;
        let root = namespace::open_source(parent, target, None)?;
        let tree = tree::PinnedTree::open(&root, max_entries, &mut progress)?;
        let evidence = tree.freeze_existing(&mut progress)?;
        let canonical = windows_fs::location(&root).map_err(io)?;
        result.target = canonical
            .file_name()
            .ok_or_else(|| io("Overwrite target has no native leaf"))?
            .to_os_string();
        namespace::validate_binding(parent, &result.target, tree.root().identity)?;
        result.original_objects = evidence
            .entries
            .iter()
            .map(|entry| entry.version.identity.object_key())
            .collect();
        result.original = Some((
            tree.root().clone(),
            OriginalAdmission {
                count: evidence.entries.len(),
                digest: super::evidence_digest(&evidence)?,
            },
        ));
        result.validate(parent)?;
        Ok(result)
    }
    /// Keep existing retained-object claims stable through an Overwrite. This
    /// uses native identities, including descendants and aliases, rather than
    /// comparing presentation paths. It is not an absent-name reservation or a
    /// simultaneous-admission barrier.
    pub(super) fn guard_overwrite_dependencies(
        &self,
        parent: &File,
        central: &File,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Vec<File>, PlatformError> {
        if self.original_objects.is_empty() {
            return Ok(Vec::new());
        }
        let creator = security::current_owner()?;
        let security = security::PrivateSecurity::capture(central, &creator)?;
        let stamp = super::version(central)?;
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut leases = Vec::new();
        let mut volumes = discovery::Volumes::new(central)?;
        for name in &names {
            progress(0)?;
            if discovery::registry_id(name).is_none() {
                if discovery::registry_candidate(name) {
                    return Err(io(
                        "Overwrite needs review of an unrecognized Copy recovery record",
                    ));
                }
                continue;
            }
            let peer = discovery::RegisteredJournal::dependency(central, name)?;
            let header = peer.state.header.as_ref().unwrap();
            if !peer.state.verified
                || header.original_admission.is_some()
                    && !peer.state.original.as_ref().is_some_and(|plan| plan.sealed)
            {
                return Err(io("Overwrite needs completed Copy dependency evidence"));
            }
            let peer_parent = if header.parent == self.parent {
                parent.try_clone().map_err(io)?
            } else {
                volumes.resolve(header.parent, FILE_GENERIC_READ, &mut progress)?
            };
            let snapshot = Snapshot::read_with_control(
                &peer_parent,
                central,
                &header.stage_name(),
                &peer.file,
            )?;
            if snapshot.bytes != peer.bytes {
                return Err(io("Copy dependency journals disagree; Overwrite refused"));
            }
            if cleanup_dependencies::references(&peer.state)
                .iter()
                .any(|identity| self.original_objects.contains(&identity.object_key()))
            {
                return Err(io(format!(
                    "Overwrite would affect retained Copy transaction {}; resolve its recovery or retained data first",
                    header.transaction
                )));
            }
            // Drop the decoded graph and bytes at this iteration. Only the
            // read-only native control/binding leases survive; appends,
            // replacement and writable mappings remain excluded for the
            // operation's lifetime.
            leases.extend([peer.file, snapshot.local, snapshot.stage, peer_parent]);
        }
        if windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
            .map_err(io)?
            != names
            || super::version(central)? != stamp
        {
            return Err(io(
                "Copy registry changed during Overwrite dependency admission",
            ));
        }
        security.validate(central, &creator)?;
        progress(0)?;
        Ok(leases)
    }
    fn validate_parent(&self, parent: &File) -> Result<(), PlatformError> {
        if stable_identity_from_handle(parent).map_err(io)? != self.parent
            || windows_fs::location(parent).map_err(io)? != self.location
        {
            return Err(io("Copy destination parent changed since admission"));
        }
        if windows_fs::directory_case_sensitive(parent).map_err(io)? != self.case_sensitive {
            return Err(io("Copy destination case policy changed since admission"));
        }
        Ok(())
    }
    fn validate(&self, parent: &File) -> Result<(), PlatformError> {
        self.validate_parent(parent)?;
        if let Some((expected, _)) = &self.original {
            let root = namespace::open_source(parent, &self.target, None)?;
            if super::version(&root)? != *expected {
                return Err(io("Overwrite destination changed since admission"));
            }
            namespace::validate_binding(parent, &self.target, expected.identity)
        } else {
            match unsafe {
                windows_fs::open_relative(
                    parent,
                    &self.target,
                    RelativeOptions {
                        directory: None,
                        create_new: false,
                        access: windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
                            | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
                        sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        security: ptr::null(),
                    },
                )
            } {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(io(error)),
                Ok(_) => Err(io("Creation destination is occupied; admission changed")),
            }
        }?;
        self.validate_parent(parent)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    protocol: String,
    schema: u16,
    transaction: String,
    parent: WindowsStableIdentity,
    parent_case_sensitive: bool,
    stage: WindowsStableIdentity,
    central: WindowsStableIdentity,
    local_file: WindowsStableIdentity,
    central_file: WindowsStableIdentity,
    parent_location: NativePath,
    target: Vec<u16>,
    original_admission: Option<OriginalAdmission>,
    source_label: Vec<u16>,
    claim: claims::Binding,
    creator: Vec<u8>,
    stage_security: security::PrivateSecurity,
    local_security: security::PrivateSecurity,
    central_security: security::PrivateSecurity,
}
impl Header {
    fn validate_parent_policy(&self, parent: &File) -> Result<(), PlatformError> {
        if windows_fs::directory_case_sensitive(parent).map_err(io)? != self.parent_case_sensitive {
            return Err(io(
                "Copy destination case policy changed; all data retained",
            ));
        }
        Ok(())
    }
    fn stage_name(&self) -> OsString {
        format!(".ottrin-copy-{}", self.transaction).into()
    }
    fn candidate_name(&self) -> OsString {
        format!(".ottrin-copy-{}.candidate", self.transaction).into()
    }
    fn original_name(&self) -> OsString {
        format!(".ottrin-copy-{}.original", self.transaction).into()
    }
    fn central_name(&self) -> OsString {
        format!("{}.copy-journal", self.transaction).into()
    }
    fn valid(&self) -> bool {
        self.protocol == "ottrin-windows-copy"
            && self.schema == 17
            && self.claim.matches(
                self.parent,
                self.central,
                &self.target,
                &self.transaction,
                &self.creator,
            )
            && self.stage_security.valid_for(&self.creator)
            && self.local_security.valid_for(&self.creator)
            && self.central_security.valid_for(&self.creator)
            && self.transaction.len() == 32
            && self
                .transaction
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            && self.parent.is_directory_without_reparse()
            && self.stage.is_directory_without_reparse()
            && self.central.is_directory_without_reparse()
            && self.local_file.is_regular_file_without_reparse()
            && self.central_file.is_regular_file_without_reparse()
            && !self.parent.same_object(self.stage)
            && !self.local_file.same_object(self.central_file)
            && self.parent.volume_serial_number() == self.stage.volume_serial_number()
            && self.stage.volume_serial_number() == self.local_file.volume_serial_number()
            && self.central.volume_serial_number() == self.central_file.volume_serial_number()
            && self
                .parent_location
                .decode()
                .is_ok_and(|path| path.is_absolute())
            && self.original_admission.as_ref().is_none_or(|original| {
                original.count > 0 && original.count <= (MAX_LOG / FRAME_HEADER as u64) as usize
            })
            && valid_leaf(&self.target)
            && valid_leaf(&self.source_label)
            && OsString::from_wide(&self.target) != self.stage_name()
            && OsString::from_wide(&self.target) != self.candidate_name()
            && OsString::from_wide(&self.target) != self.original_name()
            && !(self.parent.same_object(self.central)
                && OsString::from_wide(&self.target) == self.central_name())
    }
}
fn valid_leaf(name: &[u16]) -> bool {
    windows_fs::valid_component(&OsString::from_wide(name)).is_ok()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
enum Record {
    Begin {
        header: Box<Header>,
    },
    CreateIntent {
        index: usize,
        parent: Option<usize>,
        name: Vec<u16>,
        source: EntryVersion,
    },
    Created {
        index: usize,
        identity: WindowsStableIdentity,
        security: security::PrivateSecurity,
    },
    EntryStaged {
        index: usize,
        evidence: metadata::StagedEntry,
    },
    TreeVerified {
        count: usize,
    },
    SecurityPlanBegin {
        header: Box<security::SecurityPlanHeader>,
    },
    SecurityPlanEntry {
        index: usize,
        entry: Box<security::PlannedEntry>,
    },
    SecurityPlanComplete {
        count: usize,
        digest: [u8; 32],
    },
    ReadonlyReopenIntent {
        index: usize,
    },
    ReadonlyReopened {
        index: usize,
        version: Box<EntryVersion>,
    },
    OriginalPlanBegin {
        count: usize,
        digest: [u8; 32],
    },
    OriginalPlanEntry {
        index: usize,
        entry: Box<tree::ExistingEntry>,
    },
    OriginalPlanComplete {
        count: usize,
        digest: [u8; 32],
    },
    OriginalRetainIntent,
    OriginalRetained {
        root_version: Box<EntryVersion>,
    },
    OriginalRestoreIntent,
    OriginalRestored {
        root_version: Box<EntryVersion>,
    },
    PublicationIntent,
    Published,
    RenameCreationIntent {
        kind: RenameKind,
        before: Box<EntryVersion>,
    },
    RenameCreationApplied {
        kind: RenameKind,
        after: Box<EntryVersion>,
    },
    SecurityApplyIntent {
        index: usize,
    },
    SecurityApplied {
        index: usize,
    },
    FinalEntry {
        index: usize,
        version: Box<EntryVersion>,
    },
    Finalized {
        count: usize,
        digest: [u8; 32],
    },
    UndoIntent,
    Undone {
        root_version: Box<EntryVersion>,
    },
    Cleanup {
        entry: cleanup_protocol::CleanupRecord,
    },
}
#[derive(Debug)]
struct Node {
    parent: Option<usize>,
    name: Vec<u16>,
    source: EntryVersion,
    created: Option<WindowsStableIdentity>,
    security: Option<security::PrivateSecurity>,
    staged: Option<metadata::StagedEntry>,
}
#[derive(Default, Debug)]
struct State {
    header: Option<Header>,
    nodes: Vec<Node>,
    verified: bool,
    planned: Option<security::TreeSecurityEvidence>,
    planning: Option<Planning>,
    permissions: security::PermissionProgress,
    original: Option<OriginalPlan>,
    original_retain_intent: bool,
    original_retained: Option<EntryVersion>,
    original_restore_intent: bool,
    original_restored: Option<EntryVersion>,
    publication_intent: bool,
    published: bool,
    completion_verified: bool,
    final_versions: Vec<EntryVersion>,
    undo_intent: bool,
    undone: Option<EntryVersion>,
    readonly_pending: Option<usize>,
    rename_creation: Option<RenameCreation>,
    cleanup: Option<cleanup_protocol::Progress>,
    source_ids: BTreeSet<(u64, [u8; 16])>,
    candidate_ids: BTreeSet<(u64, [u8; 16])>,
    names: BTreeSet<(Option<usize>, Vec<u16>)>,
}
#[derive(Debug)]
struct OriginalPlan {
    expected: usize,
    digest: [u8; 32],
    evidence: tree::ExistingTreeEvidence,
    sealed: bool,
    names: BTreeSet<(Option<usize>, Vec<u16>)>,
}
#[derive(Debug)]
struct Planning {
    expected: usize,
    evidence: security::TreeSecurityEvidence,
}

fn valid_source(source: &EntryVersion) -> bool {
    if source.attributes & !metadata::ADMITTED_ATTRIBUTES != 0
        || source.length > i64::MAX as u64
        || (source.attributes & FILE_ATTRIBUTE_DIRECTORY != 0) != source.directory
        || (source.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0) != source.link.is_some()
        || (source.directory && source.length != 0)
        || source.case_sensitive.is_some() != (source.directory && source.link.is_none())
    {
        return false;
    }
    if let Some(link) = &source.link {
        links::Link::parse(link.bytes.clone(), link.directory).is_ok()
            && link.directory == source.directory
            && source.identity.reparse_tag()
                == Some(u32::from_le_bytes(link.bytes[..4].try_into().unwrap()))
    } else if source.directory {
        source.identity.is_directory_without_reparse()
    } else {
        source.identity.is_regular_file_without_reparse()
    }
}
fn valid_payload(evidence: &metadata::StagedEntry) -> bool {
    evidence
        .candidate_identity
        .same_type(evidence.source.identity)
        && valid_streams(&evidence.source, &evidence.streams, evidence.compression)
}
fn valid_streams(
    source: &EntryVersion,
    streams: &[super::StreamEvidence],
    compression: u16,
) -> bool {
    if !valid_source(source)
        || ![0, 2].contains(&compression)
        || (source.attributes & FILE_ATTRIBUTE_COMPRESSED != 0) != (compression != 0)
    {
        return false;
    }
    let mut primary = 0;
    let mut reparse = 0;
    for stream in streams {
        if stream.sha256.len() != 64
            || !stream
                .sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || stream.bytes > i64::MAX as u64
            || !matches!(
                stream.kind,
                super::DATA
                    | super::EXTENDED_ATTRIBUTES
                    | super::ALTERNATE_DATA
                    | super::PROPERTY_DATA
                    | super::REPARSE_DATA
            )
        {
            return false;
        }
        if stream.kind == super::ALTERNATE_DATA {
            if stream.name.len() < 8
                || stream.name.len() * 2 > super::MAX_NAME_BYTES
                || stream.name.first() != Some(&58)
                || !stream.name.ends_with(&[58, 36, 68, 65, 84, 65])
                || stream.name[1..stream.name.len() - 6]
                    .iter()
                    .any(|u| [0, 47, 58, 92].contains(u))
            {
                return false;
            }
        } else if !stream.name.is_empty() {
            return false;
        }
        if stream.kind == super::DATA {
            primary += 1;
            if source.directory || stream.bytes != source.length {
                return false;
            }
        }
        if stream.kind == super::REPARSE_DATA {
            reparse += 1;
            let Some(link) = &source.link else {
                return false;
            };
            if stream.bytes != link.bytes.len() as u64
                || stream.sha256 != format!("{:x}", Sha256::digest(&link.bytes))
            {
                return false;
            }
        }
    }
    primary == usize::from(!source.directory)
        && reparse == usize::from(source.link.is_some())
        && streams
            .windows(2)
            .all(|p| (p[0].kind, &p[0].name) < (p[1].kind, &p[1].name))
}
impl State {
    fn claim_terminal(&self) -> bool {
        self.undo_complete() || self.cleanup.is_some()
    }
    fn undo_complete(&self) -> bool {
        self.undone.is_some() && (self.original.is_none() || self.original_restored.is_some())
    }
    fn original_ready_for_forward(&self) -> bool {
        match (&self.header, &self.original) {
            (Some(header), None) => header.original_admission.is_none(),
            (Some(header), Some(plan)) => {
                header.original_admission.as_ref().is_some_and(|approved| {
                    approved.count == plan.expected
                        && approved.digest == plan.digest
                        && plan.sealed
                        && self.original_retain_intent
                        && !self.original_restore_intent
                })
            }
            _ => false,
        }
    }
    fn original_ready_for_publication(&self) -> bool {
        self.original_ready_for_forward()
            && (self.original.is_none() || self.original_retained.is_some())
    }
    fn valid_original_entry(
        &self,
        previous: &[tree::ExistingEntry],
        names: &BTreeSet<(Option<usize>, Vec<u16>)>,
        entry: &tree::ExistingEntry,
    ) -> bool {
        let Some(header) = &self.header else {
            return false;
        };
        let id = entry.version.identity;
        let key = id.object_key();
        if entry.links == 0
            || !valid_streams(&entry.version, &entry.streams, entry.compression)
            || self.source_ids.contains(&key)
            || self.candidate_ids.contains(&key)
            || [
                header.parent,
                header.stage,
                header.central,
                header.local_file,
                header.central_file,
            ]
            .iter()
            .any(|reserved| reserved.same_object(id))
        {
            return false;
        }
        if previous.is_empty() {
            return entry.parent.is_none() && entry.name.is_empty();
        }
        entry
            .parent
            .and_then(|p| previous.get(p))
            .is_some_and(|parent| parent.version.directory && parent.version.link.is_none())
            && valid_leaf(&entry.name)
            && !names.contains(&(entry.parent, entry.name.clone()))
    }
    fn native_complete(&self) -> bool {
        self.verified
            && self.planned.is_some()
            && self.published
            && !self.permissions.pending
            && self.permissions.completed == self.nodes.len()
    }
    fn readonly_reopen_allowed(&self, index: usize) -> bool {
        if self.undo_intent {
            index == 0 && self.undone.is_none()
        } else {
            !self.native_complete()
        }
    }
    fn validate_final_version(
        &self,
        index: usize,
        observed: &EntryVersion,
        root_name: &OsStr,
    ) -> Result<(), PlatformError> {
        let Some(original) = self.final_versions.get(index) else {
            return Ok(());
        };
        let mut expected = if index == 0 {
            self.undone.as_ref().unwrap_or(original).clone()
        } else {
            original.clone()
        };
        if index == 0
            && self.undo_intent
            && self.undone.is_none()
            && root_name == self.header.as_ref().unwrap().candidate_name()
        {
            // Only the identity-resolved retained root may have the rename's
            // native change time. Persistent metadata remains checked separately.
            self.validate_creation_epoch(RenameKind::Withdraw, observed)?;
            expected.created = observed.created;
            expected.changed = observed.changed;
            if self.readonly_pending == Some(0) {
                expected.attributes = observed.attributes;
            }
        }
        if expected != *observed {
            return Err(io("Copy final receipt version changed; all data retained"));
        }
        Ok(())
    }
    fn check(&self, record: &Record) -> Result<(), PlatformError> {
        let invalid = || io("invalid Windows Copy staging journal transition or evidence");
        if self.check_cleanup_record(record)? {
            return Ok(());
        }
        if self.check_creation_record(record)? {
            return Ok(());
        }
        if self.undone.is_some()
            && !matches!(
                record,
                Record::OriginalRestoreIntent | Record::OriginalRestored { .. }
            )
        {
            return Err(invalid());
        }
        if self.readonly_pending.is_some() && !matches!(record, Record::ReadonlyReopened { .. }) {
            return Err(invalid());
        }
        match record {
            Record::UndoIntent => {
                if !self.completion_verified || self.undo_intent {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::Undone { root_version } => {
                let mut expected = self.final_versions.first().ok_or_else(invalid)?.clone();
                expected.changed = root_version.changed;
                if !self.undo_intent || **root_version != expected {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::ReadonlyReopenIntent { index } => {
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                if self.planned.is_none()
                    || !self.readonly_reopen_allowed(*index)
                    || self.readonly_pending.is_some()
                    || node.source.directory
                    || node.source.attributes
                        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                        == 0
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::ReadonlyReopened { index, version } => {
                if self.readonly_pending != Some(*index) {
                    return Err(invalid());
                }
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                let mut expected = node.source.clone();
                expected.identity = node.staged.as_ref().ok_or_else(invalid)?.candidate_identity;
                expected.changed = version.changed;
                if *index == 0
                    && (self.publication_intent && !self.published
                        || self.undo_intent && self.undone.is_none())
                {
                    expected.created = version.created;
                    if let Some(intent) = &self.rename_creation {
                        self.validate_creation_epoch(intent.kind, version)?;
                    }
                }
                if **version != expected {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::FinalEntry { index, version } => {
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                let mut expected = node.source.clone();
                expected.identity = node.staged.as_ref().ok_or_else(invalid)?.candidate_identity;
                expected.changed = version.changed;
                if !self.native_complete()
                    || self.completion_verified
                    || *index != self.final_versions.len()
                    || **version != expected
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::Finalized { count, digest } => {
                if !self.native_complete()
                    || self.completion_verified
                    || *count != self.nodes.len()
                    || *count != self.final_versions.len()
                    || *digest != super::evidence_digest(&self.final_versions)?
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalRetainIntent => {
                if self.original.as_ref().is_none_or(|plan| !plan.sealed)
                    || self.planned.is_none()
                    || self.original_retain_intent
                    || self.publication_intent
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalRetained { root_version } => {
                let mut expected = self.original.as_ref().ok_or_else(invalid)?.evidence.entries[0]
                    .version
                    .clone();
                expected.changed = root_version.changed;
                if !self.original_retain_intent
                    || self.original_retained.is_some()
                    || **root_version != expected
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalRestoreIntent => {
                if self.original_retained.is_none()
                    || self.original_restore_intent
                    || (self.publication_intent && self.undone.is_none())
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalRestored { root_version } => {
                let mut expected = self.original_retained.as_ref().ok_or_else(invalid)?.clone();
                expected.changed = root_version.changed;
                if !self.original_restore_intent
                    || self.original_restored.is_some()
                    || **root_version != expected
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalPlanBegin { count, digest } => {
                let approved = self
                    .header
                    .as_ref()
                    .and_then(|header| header.original_admission.as_ref())
                    .ok_or_else(invalid)?;
                if *count != approved.count
                    || *digest != approved.digest
                    || !self.verified
                    || self.original.is_some()
                    || self.publication_intent
                    || self.original_retain_intent
                    || *count == 0
                    || *count > (MAX_LOG / FRAME_HEADER as u64) as usize
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalPlanEntry { index, entry } => {
                let plan = self.original.as_ref().ok_or_else(invalid)?;
                if plan.sealed
                    || self.publication_intent
                    || *index != plan.evidence.entries.len()
                    || *index >= plan.expected
                    || !self.valid_original_entry(&plan.evidence.entries, &plan.names, entry)
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::OriginalPlanComplete { count, digest } => {
                let plan = self.original.as_ref().ok_or_else(invalid)?;
                if plan.sealed
                    || *count != plan.expected
                    || *count != plan.evidence.entries.len()
                    || *digest != plan.digest
                    || *digest != super::evidence_digest(&plan.evidence)?
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::PublicationIntent => {
                if self.planned.is_none()
                    || !self.original_ready_for_publication()
                    || self.publication_intent
                    || self.published
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::Published => {
                if self.planned.is_none() || !self.publication_intent || self.published {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::SecurityApplyIntent { index } => {
                let plan = self.planned.as_ref().ok_or_else(invalid)?;
                if !self.published
                    || !self.verified
                    || self.permissions.pending
                    || *index != self.permissions.completed
                    || *index >= plan.len()
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::SecurityApplied { index } => {
                if self.planned.is_none()
                    || !self.permissions.pending
                    || *index != self.permissions.completed
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::SecurityPlanBegin { header } => {
                let owner = self.header.as_ref().ok_or_else(invalid)?;
                if !self.verified
                    || self.planned.is_some()
                    || self.planning.is_some()
                    || !header.valid_for(&owner.creator, owner.parent, self.nodes.len())
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::SecurityPlanEntry { index, entry } => {
                let plan = self.planning.as_ref().ok_or_else(invalid)?;
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                if !self.verified
                    || self.planned.is_some()
                    || *index != plan.evidence.len()
                    || *index >= plan.expected
                    || !entry.binds_staged(
                        node.parent,
                        &node.name,
                        node.staged.as_ref().ok_or_else(invalid)?.candidate_identity,
                        node.source.directory,
                        node.security.as_ref().ok_or_else(invalid)?,
                    )
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            Record::SecurityPlanComplete { count, digest } => {
                let plan = self.planning.as_ref().ok_or_else(invalid)?;
                if !self.verified
                    || self.planned.is_some()
                    || *count != plan.expected
                    || *count != plan.evidence.len()
                    || *digest != plan.evidence.digest()?
                {
                    return Err(invalid());
                }
                return Ok(());
            }
            _ => {}
        }
        if self.verified {
            return Err(invalid());
        }
        match record {
            Record::Begin { header } => {
                if self.header.is_some() || !header.valid() {
                    return Err(invalid());
                }
            }
            Record::CreateIntent {
                index,
                parent,
                name,
                source,
            } => {
                let header = self.header.as_ref().ok_or_else(invalid)?;
                if *index != self.nodes.len()
                    || !valid_source(source)
                    || !valid_leaf(name)
                    || self.nodes.last().is_some_and(|node| node.staged.is_none())
                    || self.candidate_ids.contains(&source.identity.object_key())
                {
                    return Err(invalid());
                }
                if *index == 0 {
                    if parent.is_some() || OsString::from_wide(name) != header.candidate_name() {
                        return Err(invalid());
                    }
                } else {
                    let p = parent.and_then(|p| self.nodes.get(p)).ok_or_else(invalid)?;
                    if !p.source.directory
                        || p.source.link.is_some()
                        || self.names.contains(&(*parent, name.clone()))
                    {
                        return Err(invalid());
                    }
                }
            }
            Record::Created {
                index,
                identity,
                security,
            } => {
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                let header = self.header.as_ref().ok_or_else(invalid)?;
                if !security.valid_for(&header.creator)
                    || node.created.is_some()
                    || identity.volume_serial_number() != header.parent.volume_serial_number()
                    || self.candidate_ids.contains(&identity.object_key())
                    || self.source_ids.contains(&identity.object_key())
                    || if node.source.directory {
                        !identity.is_directory_without_reparse()
                    } else {
                        !identity.is_regular_file_without_reparse()
                    }
                {
                    return Err(invalid());
                }
            }
            Record::EntryStaged { index, evidence } => {
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                if node.staged.is_some()
                    || node.source != evidence.source
                    || !valid_payload(evidence)
                    || !node
                        .created
                        .is_some_and(|id| id.same_object(evidence.candidate_identity))
                {
                    return Err(invalid());
                }
            }
            Record::OriginalRetainIntent
            | Record::OriginalRetained { .. }
            | Record::OriginalRestoreIntent
            | Record::OriginalRestored { .. }
            | Record::OriginalPlanBegin { .. }
            | Record::OriginalPlanEntry { .. }
            | Record::OriginalPlanComplete { .. }
            | Record::UndoIntent
            | Record::Undone { .. }
            | Record::ReadonlyReopenIntent { .. }
            | Record::ReadonlyReopened { .. }
            | Record::FinalEntry { .. }
            | Record::Finalized { .. }
            | Record::PublicationIntent
            | Record::Published
            | Record::RenameCreationIntent { .. }
            | Record::RenameCreationApplied { .. }
            | Record::SecurityApplyIntent { .. }
            | Record::SecurityApplied { .. }
            | Record::SecurityPlanBegin { .. }
            | Record::SecurityPlanEntry { .. }
            | Record::SecurityPlanComplete { .. } => return Err(invalid()),
            Record::Cleanup { .. } => return Err(invalid()),
            Record::TreeVerified { count } => {
                if self.header.is_none()
                    || *count == 0
                    || *count != self.nodes.len()
                    || self.nodes.iter().any(|n| n.staged.is_none())
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
    fn apply(&mut self, record: Record) -> Result<(), PlatformError> {
        self.check(&record)?;
        if self
            .rename_creation
            .as_ref()
            .is_some_and(|intent| intent.kind.acknowledges(&record))
        {
            self.rename_creation = None;
        }
        match record {
            Record::Cleanup { entry } => self.apply_cleanup_record(entry)?,
            Record::RenameCreationIntent { kind, before } => {
                self.rename_creation = Some(RenameCreation {
                    kind,
                    observed_before: (*before).clone(),
                    epoch: *before,
                    applied: None,
                })
            }
            Record::RenameCreationApplied { after, .. } => {
                self.rename_creation.as_mut().unwrap().applied = Some(*after)
            }
            Record::OriginalRetainIntent => self.original_retain_intent = true,
            Record::OriginalRetained { root_version } => {
                self.original_retained = Some(*root_version)
            }
            Record::OriginalRestoreIntent => self.original_restore_intent = true,
            Record::OriginalRestored { root_version } => {
                self.original_restored = Some(*root_version)
            }
            Record::OriginalPlanBegin { count, digest } => {
                self.original = Some(OriginalPlan {
                    expected: count,
                    digest,
                    evidence: tree::ExistingTreeEvidence {
                        entries: Vec::new(),
                    },
                    sealed: false,
                    names: BTreeSet::new(),
                })
            }
            Record::OriginalPlanEntry { entry, .. } => {
                let plan = self.original.as_mut().unwrap();
                plan.names.insert((entry.parent, entry.name.clone()));
                plan.evidence.entries.push(*entry);
            }
            Record::OriginalPlanComplete { .. } => self.original.as_mut().unwrap().sealed = true,
            Record::UndoIntent => self.undo_intent = true,
            Record::Undone { root_version } => self.undone = Some(*root_version),
            Record::ReadonlyReopenIntent { index } => self.readonly_pending = Some(index),
            Record::ReadonlyReopened { index, version } => {
                if index == 0
                    && let Some(intent) = &mut self.rename_creation
                {
                    intent.epoch = (*version).clone();
                    if intent.applied.is_some() {
                        intent.applied = Some(*version);
                    }
                }
                self.readonly_pending = None;
            }
            Record::FinalEntry { version, .. } => self.final_versions.push(*version),
            Record::Finalized { .. } => self.completion_verified = true,
            Record::PublicationIntent => self.publication_intent = true,
            Record::Published => self.published = true,
            Record::SecurityApplyIntent { .. } => self.permissions.pending = true,
            Record::SecurityApplied { .. } => {
                self.permissions.completed += 1;
                self.permissions.pending = false;
            }
            Record::Begin { header } => self.header = Some(*header),
            Record::CreateIntent {
                parent,
                name,
                source,
                ..
            } => {
                self.source_ids.insert(source.identity.object_key());
                self.names.insert((parent, name.clone()));
                self.nodes.push(Node {
                    parent,
                    name,
                    source,
                    created: None,
                    security: None,
                    staged: None,
                });
            }
            Record::Created {
                index,
                identity,
                security,
            } => {
                self.candidate_ids.insert(identity.object_key());
                self.nodes[index].created = Some(identity);
                self.nodes[index].security = Some(security);
            }
            Record::EntryStaged { index, evidence } => self.nodes[index].staged = Some(evidence),
            Record::TreeVerified { .. } => self.verified = true,
            Record::SecurityPlanBegin { header } => {
                self.planning = Some(Planning {
                    expected: header.count(),
                    evidence: security::TreeSecurityEvidence::from_header(*header),
                });
            }
            Record::SecurityPlanEntry { entry, .. } => {
                self.planning.as_mut().unwrap().evidence.push(*entry)
            }
            Record::SecurityPlanComplete { .. } => {
                self.planned = Some(self.planning.take().unwrap().evidence)
            }
        }
        Ok(())
    }
}
fn record_payload(record: &Record) -> Result<Vec<u8>, PlatformError> {
    let payload = serde_json::to_vec(record).map_err(io)?;
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(io("Copy journal record exceeds frame budget"));
    }
    Ok(payload)
}
fn frame_hash(sequence: u64, previous: &[u8; 32], payload: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(sequence.to_le_bytes());
    hash.update(previous);
    hash.update((payload.len() as u32).to_le_bytes());
    hash.update(payload);
    hash.finalize().into()
}
struct Decoded {
    state: State,
    sequence: u64,
    previous: [u8; 32],
}
fn decode(bytes: &[u8]) -> Result<Decoded, PlatformError> {
    decode_until(bytes, None).map(|(decoded, _)| decoded)
}

fn decode_until(bytes: &[u8], limit: Option<u64>) -> Result<(Decoded, usize), PlatformError> {
    if bytes.len() as u64 > MAX_LOG {
        return Err(io("Copy journal exceeds inspection budget"));
    }
    let mut state = State::default();
    let mut at = 0usize;
    let mut sequence = 0u64;
    let mut previous = [0u8; 32];
    while at < bytes.len() && limit.is_none_or(|limit| sequence < limit) {
        let header = bytes
            .get(at..at + FRAME_HEADER)
            .ok_or_else(|| io("torn Copy journal frame; data retained"))?;
        if &header[..8] != MAGIC {
            return Err(io("wrong Copy journal format"));
        }
        let size = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        if size == 0
            || size > MAX_FRAME
            || u64::from_le_bytes(header[12..20].try_into().unwrap()) != sequence
            || header[20..52] != previous
        {
            return Err(io("invalid Copy journal frame sequence/size"));
        }
        let end = at
            .checked_add(FRAME_HEADER + size)
            .ok_or_else(|| io("Copy journal size overflow"))?;
        let payload = bytes
            .get(at + FRAME_HEADER..end)
            .ok_or_else(|| io("torn Copy journal payload; data retained"))?;
        let hash = frame_hash(sequence, &previous, payload);
        if header[52..84] != hash {
            return Err(io("Copy journal digest mismatch; data retained"));
        }
        state.apply(serde_json::from_slice(payload).map_err(io)?)?;
        previous = hash;
        sequence += 1;
        at = end;
    }
    if limit.is_some_and(|limit| sequence != limit) {
        return Err(io("Copy journal prefix sequence is unavailable"));
    }
    if state.header.is_none() {
        return Err(io("empty Copy journal; data retained"));
    }
    Ok((
        Decoded {
            state,
            sequence,
            previous,
        },
        at,
    ))
}
fn open(
    parent: &File,
    name: &OsStr,
    directory: Option<bool>,
    access: u32,
) -> Result<File, PlatformError> {
    unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory,
                create_new: false,
                access,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)
}
fn bound(
    parent: &File,
    name: &OsStr,
    identity: WindowsStableIdentity,
) -> Result<(), PlatformError> {
    namespace::validate_binding(parent, name, identity)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OriginalMove {
    Retain,
    Restore,
}
impl OriginalMove {
    fn intent(self) -> Record {
        match self {
            Self::Retain => Record::OriginalRetainIntent,
            Self::Restore => Record::OriginalRestoreIntent,
        }
    }
    fn ack(self, version: EntryVersion) -> Record {
        match self {
            Self::Retain => Record::OriginalRetained {
                root_version: Box::new(version),
            },
            Self::Restore => Record::OriginalRestored {
                root_version: Box::new(version),
            },
        }
    }
}
fn identity_at(
    parent: &File,
    name: &OsStr,
    expected: WindowsStableIdentity,
) -> Result<bool, PlatformError> {
    match unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: None,
                create_new: false,
                access: windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
                    | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    } {
        Ok(file) => Ok(stable_identity_from_handle(&file).map_err(io)? == expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io(error)),
    }
}
struct Journal {
    claim: Option<claims::Lease>,
    local: File,
    central: File,
    stage_dir: File,
    parent_dir: File,
    central_dir: File,
    header: Header,
    state: State,
    sequence: u64,
    previous: [u8; 32],
    length: u64,
    failed: bool,
    candidates: Vec<File>,
}
impl Journal {
    fn readonly_ack_bound(&self, index: usize) -> Result<Record, PlatformError> {
        let node = self
            .state
            .nodes
            .get(index)
            .ok_or_else(|| io("Missing readonly Copy node"))?;
        let mut version = node.source.clone();
        version.identity = node
            .staged
            .as_ref()
            .ok_or_else(|| io("Missing readonly Copy evidence"))?
            .candidate_identity;
        version.changed = i64::MIN;
        if index == 0 {
            version.created = i64::MIN;
        }
        Ok(Record::ReadonlyReopened {
            index,
            version: Box::new(version),
        })
    }
    fn create(
        stage: &namespace::PrivateStage<'_>,
        central: &File,
        destination: &DestinationAdmission,
        source_label: &OsStr,
        claim: &mut Option<claims::Lease>,
    ) -> Result<Self, PlatformError> {
        stage.validate()?;
        destination.validate(stage.parent())?;
        let (parent_id, stage_id) = stage.identities();
        let central_id = stable_identity_from_handle(central).map_err(io)?;
        if !central_id.is_directory_without_reparse() {
            return Err(io("Copy journal index is not an ordinary directory"));
        }
        let name = stage
            .name
            .to_str()
            .and_then(|name| name.strip_prefix(".ottrin-copy-"))
            .ok_or_else(|| io("invalid Copy transaction name"))?
            .to_owned();
        let central_name = OsString::from(format!("{name}.copy-journal"));
        let local = windows_fs::create_private_temporary_file(
            &stage.directory,
            OsStr::new(LOCAL),
            FILE_SHARE_READ,
        )
        .map_err(io)?;
        initialization::boundary(initialization::Point::LocalCreated, stage, central)?;
        let central_file =
            windows_fs::create_private_temporary_file(central, &central_name, FILE_SHARE_READ)
                .map_err(io)?;
        initialization::boundary(initialization::Point::ControlsCreated, stage, central)?;
        let creator = security::current_owner()?;
        let header = Header {
            stage_security: security::PrivateSecurity::capture(&stage.directory, &creator)?,
            local_security: security::PrivateSecurity::capture(&local, &creator)?,
            central_security: security::PrivateSecurity::capture(&central_file, &creator)?,
            creator,
            protocol: "ottrin-windows-copy".into(),
            schema: 17,
            transaction: name,
            parent: parent_id,
            parent_case_sensitive: destination.case_sensitive,
            stage: stage_id,
            central: central_id,
            local_file: stable_identity_from_handle(&local).map_err(io)?,
            central_file: stable_identity_from_handle(&central_file).map_err(io)?,
            parent_location: NativePath::encode(&windows_fs::location(stage.parent()).map_err(io)?),
            target: destination.target.encode_wide().collect(),
            original_admission: destination.original.as_ref().map(|(_, seal)| seal.clone()),
            source_label: source_label.encode_wide().collect(),
            claim: claim
                .as_ref()
                .ok_or_else(|| io("Missing initial Copy claim"))?
                .binding()
                .clone(),
        };
        if !header.valid() {
            return Err(io("invalid Copy journal header"));
        }
        let stage_dir = stage.directory.try_clone().map_err(io)?;
        let parent_dir = stage.parent().try_clone().map_err(io)?;
        let central_dir = central.try_clone().map_err(io)?;
        stage_dir.sync_all().map_err(io)?;
        central_dir.sync_all().map_err(io)?;
        parent_dir.sync_all().map_err(io)?;
        initialization::boundary(initialization::Point::DirectoriesFlushed, stage, central)?;
        // From this point onward a failed durability transition retains recovery
        // state. Before it, the caller still owns the unstarted reservation.
        let mut result = Self {
            claim: claim.take(),
            local,
            central: central_file,
            stage_dir,
            parent_dir,
            central_dir,
            header: header.clone(),
            state: State::default(),
            sequence: 0,
            previous: [0; 32],
            length: 0,
            failed: false,
            candidates: Vec::new(),
        };
        windows_fs::persist_temporary_file(&result.local).map_err(io)?;
        windows_fs::persist_temporary_file(&result.central).map_err(io)?;
        result.persist(Record::Begin {
            header: Box::new(header),
        })?;
        Ok(result)
    }
    fn from_snapshot(
        parent: &File,
        central: &File,
        snapshot: Snapshot,
    ) -> Result<Self, PlatformError> {
        snapshot.revalidate(parent, central)?;
        let length = snapshot.bytes.len() as u64;
        let header = snapshot.state.header.as_ref().unwrap().clone();
        drop(snapshot.claim);
        let claim = if snapshot.state.claim_terminal() {
            claims::Lease::observe_terminal(parent, &header.claim)?
        } else {
            Some(claims::Lease::open(parent, &header.claim, false, true)?)
        };
        let mut result = Self {
            claim,
            local: snapshot.local,
            central: snapshot.mirror,
            stage_dir: snapshot.stage,
            parent_dir: parent.try_clone().map_err(io)?,
            central_dir: central.try_clone().map_err(io)?,
            header,
            state: snapshot.state,
            sequence: snapshot.sequence,
            previous: snapshot.previous,
            length,
            failed: false,
            candidates: Vec::new(),
        };
        result.release_claim_if_terminal()?;
        Ok(result)
    }
    fn original_restore_tail(&self) -> Result<Vec<Record>, PlatformError> {
        let Some(plan) = &self.state.original else {
            return Ok(Vec::new());
        };
        if self.state.original_restored.is_some() {
            return Ok(Vec::new());
        }
        // A begin-only preparation prefix has no root yet and grants no move
        // authority. Retention reserves the complete tail after the seal.
        let Some(root) = plan.evidence.entries.first() else {
            return Ok(Vec::new());
        };
        let mut version = root.version.clone();
        version.changed = i64::MIN;
        let mut records = Vec::new();
        if !self.state.original_restore_intent {
            records.push(Record::OriginalRestoreIntent);
        }
        records.extend(self.creation_tail(RenameKind::RestoreOriginal)?);
        records.push(OriginalMove::Restore.ack(version));
        Ok(records)
    }
    fn undo_tail_size(&self) -> Result<u64, PlatformError> {
        let mut records = self.original_restore_tail()?;
        if self.state.undone.is_none() {
            if !self.state.undo_intent {
                records.push(Record::UndoIntent);
            }
            let root = &self.state.nodes[0];
            if !root.source.directory
                && root.source.attributes
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                    != 0
            {
                if self.state.readonly_pending.is_none() {
                    records.push(Record::ReadonlyReopenIntent { index: 0 });
                }
                records.push(self.readonly_ack_bound(0)?);
            }
            let mut version = root.source.clone();
            version.identity = root.staged.as_ref().unwrap().candidate_identity;
            version.changed = i64::MIN;
            records.extend(self.creation_tail(RenameKind::Withdraw)?);
            records.push(Record::Undone {
                root_version: Box::new(version),
            });
        }
        records.iter().try_fold(0u64, |total, record| {
            total
                .checked_add(FRAME_HEADER as u64 + record_payload(record)?.len() as u64)
                .ok_or_else(|| io("Copy Undo reservation overflow"))
        })
    }
    fn reserve_undo_tail(&self) -> Result<(), PlatformError> {
        if self
            .length
            .checked_add(self.undo_tail_size()?)
            .is_none_or(|size| size > log_budget())
        {
            return Err(io(
                "Copy journal has insufficient Undo/restoration space; data retained",
            ));
        }
        Ok(())
    }
    fn permission_tail_size(&self) -> Result<u64, PlatformError> {
        // Once an original is recorded, publication must leave room for the
        // entire inverse, including withdrawal of the new copy and restoration.
        let mut total = if self.state.original.is_some() {
            self.undo_tail_size()?
        } else {
            0
        };
        let mut reserve = |record: Record| -> Result<(), PlatformError> {
            total = total
                .checked_add(FRAME_HEADER as u64 + record_payload(&record)?.len() as u64)
                .ok_or_else(|| io("Copy permission journal reservation overflow"))?;
            Ok(())
        };
        if !self.state.published {
            if !self.state.publication_intent {
                reserve(Record::PublicationIntent)?;
            }
            for record in self.creation_tail(RenameKind::Publish)? {
                reserve(record)?;
            }
            reserve(Record::Published)?;
        }
        for index in self.state.permissions.completed..self.state.nodes.len() {
            if !(index == self.state.permissions.completed && self.state.permissions.pending) {
                reserve(Record::SecurityApplyIntent { index })?;
            }
            reserve(Record::SecurityApplied { index })?;
        }
        if !self.state.completion_verified {
            if !self.state.native_complete() {
                if let Some(index) = self.state.readonly_pending {
                    reserve(self.readonly_ack_bound(index)?)?;
                }
                for (index, node) in self.state.nodes.iter().enumerate() {
                    if !node.source.directory
                        && node.source.attributes
                            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                            != 0
                    {
                        reserve(Record::ReadonlyReopenIntent { index })?;
                        reserve(self.readonly_ack_bound(index)?)?;
                    }
                }
            }
            for (index, node) in self
                .state
                .nodes
                .iter()
                .enumerate()
                .skip(self.state.final_versions.len())
            {
                let mut version = node.source.clone();
                version.identity = node.staged.as_ref().unwrap().candidate_identity;
                // The native DACL change time is not known until application.
                // A signed i64's minimum is its longest possible JSON encoding.
                version.changed = i64::MIN;
                reserve(Record::FinalEntry {
                    index,
                    version: Box::new(version),
                })?;
            }
            reserve(Record::Finalized {
                count: self.state.nodes.len(),
                digest: [255; 32],
            })?;
        }
        Ok(total)
    }
    fn reserve_permission_tail(&self) -> Result<(), PlatformError> {
        if self
            .length
            .checked_add(self.permission_tail_size()?)
            .is_none_or(|length| length > log_budget())
        {
            return Err(io(
                "Copy journal has insufficient space for publication and permission acknowledgements; data retained",
            ));
        }
        Ok(())
    }
    /// One streaming append path for live preparation and source-free recovery.
    fn prepare_plan(
        &mut self,
        evidence: &security::TreeSecurityEvidence,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self
            .state
            .planned
            .as_ref()
            .is_some_and(|recorded| recorded != evidence)
            || self
                .state
                .planning
                .as_ref()
                .is_some_and(|partial| !partial.evidence.is_prefix_of(evidence, partial.expected))
        {
            return Err(io(
                "recorded Copy destination security plan changed; data retained",
            ));
        }
        let sealed = self.state.planned.is_some();
        let begin = !sealed && self.state.planning.is_none();
        let written = if sealed {
            evidence.len()
        } else {
            self.state
                .planning
                .as_ref()
                .map_or(0, |partial| partial.evidence.len())
        };
        let digest = if sealed { [0; 32] } else { evidence.digest()? };
        let records = || {
            begin
                .then(|| Record::SecurityPlanBegin {
                    header: Box::new(evidence.header()),
                })
                .into_iter()
                .chain(
                    evidence
                        .entries()
                        .enumerate()
                        .skip(written)
                        .map(|(index, entry)| Record::SecurityPlanEntry {
                            index,
                            entry: Box::new(entry.clone()),
                        }),
                )
                .chain((!sealed).then_some(Record::SecurityPlanComplete {
                    count: evidence.len(),
                    digest,
                }))
        };
        // Reserve both the remaining plan and every later mutation acknowledgement
        // before appending. Do not strand a valid prefix against the log budget.
        let required = records().try_fold(self.permission_tail_size()?, |total, record| {
            total
                .checked_add(FRAME_HEADER as u64 + record_payload(&record)?.len() as u64)
                .ok_or_else(|| io("Copy plan reservation overflow"))
        })?;
        if self
            .length
            .checked_add(required)
            .is_none_or(|length| length > log_budget())
        {
            return Err(io(
                "Copy journal has insufficient space for plan and application; data retained",
            ));
        }
        for record in records() {
            progress()?;
            self.persist(record)?;
        }
        Ok(())
    }
    /// Record the existing destination without changing it. This is evidence
    /// preparation, not authority to displace or publish either version.
    fn prepare_original(
        &mut self,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        progress(0)?;
        if self.failed
            || self.state.original_retain_intent
            || !self.state.verified
            || self.state.publication_intent
            || self.state.undo_intent
            || self.state.readonly_pending.is_some()
        {
            return Err(io(
                "original destination planning requires verified unpublished Copy data",
            ));
        }
        self.bindings()?;
        if self.header.original_admission.is_none() {
            return Err(io("Creation admission cannot become an overwrite"));
        }
        let target = OsString::from_wide(&self.header.target);
        let root = namespace::open_source(&self.parent_dir, &target, None)?;
        let tree = tree::PinnedTree::open(&root, max_entries, &mut progress)?;
        namespace::validate_binding(&self.parent_dir, &target, tree.root().identity)?;
        let evidence = tree.freeze_existing(&mut progress)?;
        let mut names = BTreeSet::new();
        for (index, entry) in evidence.entries.iter().enumerate() {
            if !self
                .state
                .valid_original_entry(&evidence.entries[..index], &names, entry)
            {
                return Err(io(
                    "original destination aliases Copy data, its source or journal ownership, or has invalid evidence",
                ));
            }
            names.insert((entry.parent, entry.name.clone()));
        }
        let digest = super::evidence_digest(&evidence)?;
        let approved = self
            .header
            .original_admission
            .as_ref()
            .ok_or_else(|| io("Creation admission cannot become an overwrite"))?;
        if evidence.entries.len() != approved.count || digest != approved.digest {
            return Err(io(
                "Original destination differs from the approved snapshot; all versions retained",
            ));
        }
        if self.state.original.as_ref().is_some_and(|plan| {
            plan.expected != evidence.entries.len()
                || plan.digest != digest
                || !evidence.entries.starts_with(&plan.evidence.entries)
                || (plan.sealed && plan.evidence != evidence)
        }) {
            return Err(io(
                "recorded original destination changed; all versions retained",
            ));
        }
        let sealed = self.state.original.as_ref().is_some_and(|plan| plan.sealed);
        let begin = self.state.original.is_none();
        let written = self
            .state
            .original
            .as_ref()
            .map_or(0, |plan| plan.evidence.entries.len());

        let records =
            || {
                begin
                    .then_some(Record::OriginalPlanBegin {
                        count: evidence.entries.len(),
                        digest,
                    })
                    .into_iter()
                    .chain(evidence.entries.iter().enumerate().skip(written).map(
                        |(index, entry)| Record::OriginalPlanEntry {
                            index,
                            entry: Box::new(entry.clone()),
                        },
                    ))
                    .chain((!sealed).then_some(Record::OriginalPlanComplete {
                        count: evidence.entries.len(),
                        digest,
                    }))
            };
        let required = records().try_fold(self.permission_tail_size()?, |sum, record| {
            sum.checked_add(FRAME_HEADER as u64 + record_payload(&record)?.len() as u64)
                .ok_or_else(|| io("original plan reservation overflow"))
        })?;
        if self
            .length
            .checked_add(required)
            .is_none_or(|size| size > log_budget())
        {
            return Err(io(
                "Copy journal has insufficient space for original destination evidence",
            ));
        }
        for record in records() {
            progress(0)?;
            // Complete observations still need live validation; a seal alone
            // cannot turn an external change into an owned original version.
            if matches!(record, Record::OriginalPlanComplete { .. }) {
                tree.validate_existing(&evidence, &mut progress)?;
                namespace::validate_binding(&self.parent_dir, &target, tree.root().identity)?;
            }
            self.persist(record)?;
        }
        Ok(())
    }
    /// Retain or restore only the identity-bound original. The copy remains
    /// unpublished; restoration here cancels a pre-publication overwrite.
    fn move_original(
        &mut self,
        admitted: Option<&EntryVersion>,
        direction: OriginalMove,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.move_original_inner(admitted,direction, progress).map_err(|error| {
            if !self.state.original_retain_intent { return error; }
            let parent = windows_fs::location(&self.parent_dir).map(|p| p.display().to_string())
                .unwrap_or_else(|_| "recorded native parent".into());
            PlatformError::CleanupPending {
                transaction: windows_fs::location(&self.stage_dir).map(|p| p.display().to_string())
                    .unwrap_or_else(|_| self.header.stage_name().to_string_lossy().into_owned()),
                message: format!("{error}; all versions retained. Original locations to inspect: {parent} / {:?} or {:?}; new copy: {:?}",
                    OsString::from_wide(&self.header.target), self.header.original_name(), self.header.candidate_name()),
            }
        })
    }
    fn move_original_inner(
        &mut self,
        admitted: Option<&EntryVersion>,
        direction: OriginalMove,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        progress(0)?;
        if self.failed
            || self.state.readonly_pending.is_some()
            || (direction == OriginalMove::Retain
                && (self.state.publication_intent || self.state.undo_intent))
            || (direction == OriginalMove::Restore
                && self.state.publication_intent
                && self.state.undone.is_none())
        {
            return Err(io(
                "original move requires an unpublished or fully withdrawn copy",
            ));
        }
        self.bindings()?;
        let evidence = self
            .state
            .original
            .as_ref()
            .filter(|plan| plan.sealed)
            .ok_or_else(|| io("original destination has no sealed evidence"))?
            .evidence
            .clone();
        let (intent, completed, base, from, to) = match direction {
            OriginalMove::Retain => {
                if self.state.original_restore_intent {
                    return Err(io("original restoration already owns this transaction"));
                }
                (
                    self.state.original_retain_intent,
                    self.state.original_retained.clone(),
                    evidence.entries[0].version.clone(),
                    OsString::from_wide(&self.header.target),
                    self.header.original_name(),
                )
            }
            OriginalMove::Restore => (
                self.state.original_restore_intent,
                self.state.original_restored.clone(),
                self.state
                    .original_retained
                    .clone()
                    .ok_or_else(|| io("original was not retained"))?,
                self.header.original_name(),
                OsString::from_wide(&self.header.target),
            ),
        };
        let kind = if direction == OriginalMove::Retain {
            RenameKind::RetainOriginal
        } else {
            RenameKind::RestoreOriginal
        };
        let resolve = |intent: bool, completed: bool| -> Result<OsString, PlatformError> {
            match (
                identity_at(&self.parent_dir, &from, base.identity)?,
                identity_at(&self.parent_dir, &to, base.identity)?,
            ) {
                (true, false) if !completed => Ok(from.clone()),
                (false, true) if intent => Ok(to.clone()),
                _ => Err(io(
                    "original identity has ambiguous or unrecorded location; all data retained",
                )),
            }
        };
        let validate = |tree: &tree::PinnedTree,
                        name: &OsStr,
                        intent: bool,
                        progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>|
         -> Result<EntryVersion, PlatformError> {
            let mut expected = evidence.clone();
            let mut version = completed.as_ref().unwrap_or(&base).clone();
            if completed.is_none() && intent && name == to {
                self.state.validate_creation_epoch(kind, tree.root())?;
                version.created = tree.root().created;
                version.changed = tree.root().changed;
            }
            expected.entries[0].version = version;
            tree.validate_existing(&expected, progress)?;
            namespace::validate_binding(&self.parent_dir, name, base.identity)?;
            Ok(tree.root().clone())
        };
        let name = resolve(intent, completed.is_some())?;
        self.state.validate_creation_location(kind, &name)?;
        let read = namespace::open_source(&self.parent_dir, &name, None)?;
        let tree = tree::PinnedTree::open(&read, evidence.entries.len(), &mut progress)?;
        validate(&tree, &name, intent, &mut progress)?;
        if let Some(admitted) = admitted
            && name == to
            && !self
                .state
                .admitted_rename_matches(kind, admitted, tree.root())
        {
            return Err(io("Original rename admission epoch changed"));
        }
        let observed_name = name.clone();
        let observed_version = tree.root().clone();
        if completed.is_some() {
            return Ok(());
        }
        // A flushable readonly-root capability needs a separately journaled
        // strategy; never clear attributes on an original as an implicit repair.
        if !base.directory
            && base.attributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                != 0
        {
            return Err(PlatformError::UnsupportedSafeCopy(
                "readonly original-root retention needs its own access strategy".into(),
            ));
        }
        let mut bound = base.clone();
        bound.changed = i64::MIN;
        let mut tail = self.creation_tail(kind)?;
        tail.push(direction.ack(bound.clone()));
        if !intent {
            tail.push(direction.intent());
        }
        // Restoration is terminal: no publication/application tail follows it.
        // Retention reserves the full inverse already; do not count restoration
        // twice when spending that reserved space at the end of Undo.
        let following = if direction == OriginalMove::Retain {
            self.permission_tail_size()?
        } else {
            0
        };
        let required = tail.iter().try_fold(following, |sum, record| {
            sum.checked_add(FRAME_HEADER as u64 + record_payload(record)?.len() as u64)
                .ok_or_else(|| io("original move reservation overflow"))
        })?;
        if self
            .length
            .checked_add(required)
            .is_none_or(|size| size > log_budget())
        {
            return Err(io("insufficient Copy journal space for original move"));
        }
        // Keep the independently verified new copy pinned during retention.
        // Restoration must remain possible even if that private copy changed.
        let candidate = if direction == OriginalMove::Retain && name == from {
            let tree = HeldTree::open(
                &self.parent_dir,
                &self.state,
                self.header.candidate_name(),
                None,
                &mut progress,
            )?;
            tree.revalidate(&self.parent_dir, &self.state, &mut progress)?;
            Some(tree)
        } else {
            None
        };
        if !intent {
            self.persist(direction.intent())?;
        }
        drop((tree, read));
        original_handoff(self.sequence)?;
        let name = match (
            identity_at(&self.parent_dir, &from, base.identity)?,
            identity_at(&self.parent_dir, &to, base.identity)?,
        ) {
            (true, false) => from.clone(),
            (false, true) => to.clone(),
            _ => {
                return Err(io(
                    "original binding changed at writer handoff; all data retained",
                ));
            }
        };
        let root = windows_fs::PerObjectFile::open(&self.parent_dir, &name).map_err(io)?;
        self.state.validate_creation_location(kind, &name)?;
        let tree = tree::PinnedTree::open(&root, evidence.entries.len(), &mut progress)?;
        if name == observed_name && tree.root() != &observed_version {
            return Err(io("Original metadata changed at writer handoff"));
        }
        // The shared verifier checks every stream, member, alias and descriptor
        // again under the root capability. Close descendants for native rename.
        let mut expected = evidence.clone();
        let mut version = base.clone();
        if name == to {
            self.state.validate_creation_epoch(kind, tree.root())?;
            version.created = tree.root().created;
            version.changed = tree.root().changed;
        }
        expected.entries[0].version = version;
        tree.validate_existing(&expected, &mut progress)?;
        namespace::validate_binding(&self.parent_dir, &name, base.identity)?;
        drop(tree);
        progress(0)?;
        root.sync_all().map_err(io)?;
        if name != to {
            crate::move_transaction::rename_windows_handle_no_replace(
                &root,
                &self.parent_dir,
                &to,
            )?;
        }
        boundary(self.sequence, Step::OriginalRenamed)?;
        root.sync_all().map_err(io)?;
        self.parent_dir.sync_all().map_err(io)?;
        boundary(self.sequence, Step::OriginalFlushed)?;
        let tree = tree::PinnedTree::open(&root, evidence.entries.len(), &mut progress)?;
        self.state.validate_creation_epoch(kind, tree.root())?;
        let mut expected = evidence.clone();
        let mut version = base.clone();
        version.created = tree.root().created;
        version.changed = tree.root().changed;
        expected.entries[0].version = version;
        tree.validate_existing(&expected, &mut progress)?;
        namespace::validate_binding(&self.parent_dir, &to, expected.entries[0].version.identity)?;
        drop(tree);
        self.finish_creation(kind, &root)?;
        let tree = tree::PinnedTree::open(&root, evidence.entries.len(), &mut progress)?;
        self.state.validate_creation_epoch(kind, tree.root())?;
        let mut expected = evidence;
        let mut version = base;
        version.changed = tree.root().changed;
        expected.entries[0].version = version;
        tree.validate_existing(&expected, &mut progress)?;
        let root_version = tree.root().clone();
        progress(0)?;
        let result = self.persist(direction.ack(root_version));
        drop((tree, root, candidate));
        result
    }
    /// A paired intent authorizes only the readonly bit transition on one
    /// fully verified candidate. Restore and flush through the newly writable
    /// capability before acknowledging; retain that capability for later flushes.
    fn reopen_readonly(
        &mut self,
        index: usize,
        owner: &File,
        name: &OsStr,
        file: windows_fs::PerObjectFile,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<windows_fs::PerObjectFile, PlatformError> {
        let node = &self.state.nodes[index];
        if node.source.directory
            || node.source.attributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                == 0
            || !self.state.readonly_reopen_allowed(index)
        {
            return Ok(file);
        }
        let mut evidence = node.staged.as_ref().unwrap().clone();
        self.state.validate_copy_metadata(index, &file, name)?;
        if index == 0 && self.state.copy_rename_at(name).is_some() {
            evidence.source.created = super::version(&file)?.created;
        }
        self.state
            .planned
            .as_ref()
            .unwrap()
            .validate_entry_security(index, &file, self.state.permissions)?;
        namespace::validate_single_link(&file)?;
        bound(owner, name, evidence.candidate_identity)?;
        if self.state.undo_intent {
            self.reserve_undo_tail()?;
        } else {
            self.reserve_permission_tail()?;
        }
        if self.state.readonly_pending.is_none() {
            self.persist(Record::ReadonlyReopenIntent { index })?;
        } else if self.state.readonly_pending != Some(index) {
            return Err(io("another Copy readonly reopen must finish first"));
        }
        self.failed = true;
        evidence.set_reopening_attributes(&file, true)?;
        boundary(self.sequence, Step::ReadonlyWritable)?;
        drop(file);
        let file = windows_fs::PerObjectFile::open(owner, name).map_err(io)?;
        bound(owner, name, evidence.candidate_identity)?;
        namespace::validate_single_link(&file)?;
        self.state
            .planned
            .as_ref()
            .unwrap()
            .validate_entry_security(index, &file, self.state.permissions)?;
        evidence.validate_reopening_metadata(&file, true)?;
        if super::transfer(&file, None, progress)? != evidence.streams {
            return Err(io(
                "Copy contents changed during readonly reopening; data retained",
            ));
        }
        evidence.set_reopening_attributes(&file, false)?;
        boundary(self.sequence, Step::ReadonlyRestored)?;
        file.sync_all().map_err(io)?;
        boundary(self.sequence, Step::ReadonlyFlushed)?;
        evidence.validate_retained_metadata(&file)?;
        bound(owner, name, evidence.candidate_identity)?;
        self.bindings()?;
        self.failed = false;
        self.persist(Record::ReadonlyReopened {
            index,
            version: Box::new(super::version(&file)?),
        })?;
        Ok(file)
    }

    /// Finish the one interrupted descendant lease before opening other writable
    /// capabilities. Ancestors are directories; no link target is traversed.
    fn finish_pending_readonly(
        &mut self,
        root: &windows_fs::PerObjectFile,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let Some(index) = self.state.readonly_pending.filter(|index| *index != 0) else {
            return Ok(());
        };
        let mut ancestors = Vec::new();
        let mut parent = self.state.nodes[index].parent;
        while let Some(index) = parent {
            if index != 0 {
                ancestors.push(index);
            }
            parent = self.state.nodes[index].parent;
        }
        let mut owner = root.try_clone().map_err(io)?;
        for index in ancestors.into_iter().rev() {
            let node = &self.state.nodes[index];
            let name = OsString::from_wide(&node.name);
            let file = windows_fs::PerObjectFile::open(&owner, &name).map_err(io)?;
            bound(
                &owner,
                &name,
                node.staged.as_ref().unwrap().candidate_identity,
            )?;
            if !node.source.directory || node.source.link.is_some() {
                return Err(io(
                    "Copy readonly recovery ancestor is not an ordinary directory",
                ));
            }
            owner = file;
        }
        let name = OsString::from_wide(&self.state.nodes[index].name);
        let file = windows_fs::PerObjectFile::open(&owner, &name).map_err(io)?;
        drop(self.reopen_readonly(index, &owner, &name, file, progress)?);
        Ok(())
    }

    fn candidate_security(&self, index: usize) -> Result<(), PlatformError> {
        let node = self
            .state
            .nodes
            .get(index)
            .ok_or_else(|| io("missing Copy security record"))?;
        let security = node
            .security
            .as_ref()
            .ok_or_else(|| io("missing private Copy security evidence"))?;
        let candidate = self
            .candidates
            .get(index)
            .ok_or_else(|| io("missing held Copy candidate"))?;
        security.validate(candidate, &self.header.creator)
    }
    fn release_claim_if_terminal(&mut self) -> Result<(), PlatformError> {
        if self.state.claim_terminal()
            && let Some(claim) = self.claim.take()
        {
            self.failed = true;
            claim.release_after_terminal()?;
            self.failed = false;
        }
        Ok(())
    }
    fn bindings(&self) -> Result<(), PlatformError> {
        match &self.claim {
            Some(claim) => claim.validate(self.state.claim_terminal())?,
            None if self.state.claim_terminal() => {}
            None => return Err(io("Copy writer lost its destination claim")),
        }
        self.header.validate_parent_policy(&self.parent_dir)?;
        if security::current_owner()? != self.header.creator {
            return Err(io("Copy journal writer creator context changed"));
        }
        namespace::validate_single_link(&self.local)?;
        namespace::validate_single_link(&self.central)?;
        self.header
            .stage_security
            .validate(&self.stage_dir, &self.header.creator)?;
        self.header
            .local_security
            .validate(&self.local, &self.header.creator)?;
        self.header
            .central_security
            .validate(&self.central, &self.header.creator)?;
        if stable_identity_from_handle(&self.stage_dir).map_err(io)? != self.header.stage
            || stable_identity_from_handle(&self.central_dir).map_err(io)? != self.header.central
        {
            return Err(io("Copy journal directory identity changed"));
        }
        bound(
            &self.parent_dir,
            &self.header.stage_name(),
            self.header.stage,
        )?;
        bound(&self.stage_dir, OsStr::new(LOCAL), self.header.local_file)?;
        bound(
            &self.central_dir,
            &self.header.central_name(),
            self.header.central_file,
        )?;
        if self.local.metadata().map_err(io)?.len() != self.length
            || self.central.metadata().map_err(io)?.len() != self.length
        {
            return Err(io(
                "Copy journal length changed; no further mutation allowed",
            ));
        }
        Ok(())
    }
    fn persist(&mut self, record: Record) -> Result<(), PlatformError> {
        if self.failed {
            return Err(io("Copy journal write is uncertain; data retained"));
        }
        self.state.check(&record)?;
        self.bindings()?;
        let payload = record_payload(&record)?;
        if self.length + FRAME_HEADER as u64 + payload.len() as u64 > log_budget() {
            return Err(io("Copy journal exceeds its staging record budget"));
        }
        let hash = frame_hash(self.sequence, &self.previous, &payload);
        let mut frame = Vec::with_capacity(FRAME_HEADER + payload.len());
        frame.extend_from_slice(MAGIC);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&self.sequence.to_le_bytes());
        frame.extend_from_slice(&self.previous);
        frame.extend_from_slice(&hash);
        frame.extend_from_slice(&payload);
        // Any failure after starting an append poisons this owner. Never append
        // a second interpretation of an uncertain sequence or repair one mirror.
        self.failed = true;
        for (file, step) in [
            (&mut self.local, Step::LocalFlushed),
            (&mut self.central, Step::CentralFlushed),
        ] {
            file.seek(SeekFrom::End(0)).map_err(io)?;
            file.write_all(&frame).map_err(io)?;
            file.sync_all().map_err(io)?;
            boundary(self.sequence, step)?;
        }
        self.length += frame.len() as u64;
        self.bindings()?;
        self.state.apply(record)?;
        self.sequence += 1;
        self.previous = hash;
        self.failed = false;
        self.release_claim_if_terminal()?;
        Ok(())
    }
}
impl tree::StagingJournal for Journal {
    fn intent(
        &mut self,
        index: usize,
        parent: Option<usize>,
        name: &OsStr,
        source: &EntryVersion,
    ) -> Result<(), PlatformError> {
        if let Some(parent) = parent {
            self.candidate_security(parent)?;
        }
        self.persist(Record::CreateIntent {
            index,
            parent,
            name: name.encode_wide().collect(),
            source: source.clone(),
        })
    }
    fn created(
        &mut self,
        index: usize,
        parent: &File,
        candidate: &File,
    ) -> Result<(), PlatformError> {
        let identity = stable_identity_from_handle(candidate).map_err(io)?;
        if self.failed {
            return Err(io("Copy journal owner is uncertain"));
        }
        if self
            .state
            .nodes
            .get(index)
            .is_some_and(|node| node.created == Some(identity) && node.staged.is_none())
        {
            self.candidate_security(index)?;
            return self.bindings();
        }
        if index != self.candidates.len() {
            return Err(io("Copy candidate security order changed"));
        }
        let node = self
            .state
            .nodes
            .get(index)
            .ok_or_else(|| io("Copy creation has no durable intent"))?;
        if let Some(parent) = node.parent {
            self.candidate_security(parent)?;
        }
        let security = security::PrivateSecurity::capture(candidate, &self.header.creator)?;
        let held = candidate.try_clone().map_err(io)?;
        candidate.sync_all().map_err(io)?;
        parent.sync_all().map_err(io)?;
        self.persist(Record::Created {
            index,
            identity,
            security,
        })?;
        self.candidates.push(held);
        self.candidate_security(index)
    }
    fn staged(
        &mut self,
        index: usize,
        evidence: &metadata::StagedEntry,
    ) -> Result<(), PlatformError> {
        self.candidate_security(index)?;
        self.persist(Record::EntryStaged {
            index,
            evidence: evidence.clone(),
        })
    }
    fn verified(&mut self, count: usize) -> Result<(), PlatformError> {
        if count != self.candidates.len() {
            return Err(io("Copy security candidate count changed"));
        }
        for index in 0..count {
            self.candidate_security(index)?;
        }
        self.persist(Record::TreeVerified { count })
    }
}

pub(super) struct DurableStage<'a> {
    pub(super) stage: namespace::PrivateStage<'a>,
    journal: Journal,
    pub(super) tree: tree::StagedTree,
}
impl<'a> DurableStage<'a> {
    pub(super) fn create(
        source: &File,
        parent: &'a File,
        central: &File,
        destination: &DestinationAdmission,
        source_label: &OsStr,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        destination.validate(parent)?;
        windows_fs::valid_component(source_label).map_err(io)?;
        let source = tree::PinnedTree::open(source, max_entries, &mut progress)?;
        source.reject_destination_parent(parent)?;
        source.reject_destination_parent(central)?;
        destination.validate(parent)?;
        let transaction = super::super::random_transaction_id()?;
        let claim = claims::Lease::acquire(parent, central, &destination.target, &transaction)?;
        if let Err(error) = destination.validate(parent) {
            return Err(claim.abort_unstarted(error));
        }
        let stage = match namespace::PrivateStage::for_transaction(parent, &transaction) {
            Ok(stage) => stage,
            Err(error) => return Err(claim.abort_unstarted(error)),
        };
        let mut claim = Some(claim);
        let mut journal =
            match Journal::create(&stage, central, destination, source_label, &mut claim) {
                Ok(journal) => journal,
                Err(error) => {
                    if let Some(claim) = claim {
                        // Temporary empty controls have closed before cleanup. Only
                        // this live owner's validated empty stage may be discarded.
                        let location = windows_fs::location(&stage.directory)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| format!("native Copy stage {:?}", stage.name));
                        return Err(match initialization::discard_empty(stage, central) {
                            Ok(()) => claim.abort_unstarted(error),
                            Err(cleanup) => PlatformError::CleanupPending {
                                transaction: location,
                                message: format!(
                                    "{error}; unstarted Copy cleanup needs review: {cleanup}"
                                ),
                            },
                        });
                    }
                    return Err(stage_error(&stage, error));
                }
            };
        let retained = |error| stage_error(&stage, error);
        tree::StagingJournal::intent(&mut journal, 0, None, &stage.candidate_name, source.root())
            .map_err(retained)?;
        let candidate = stage
            .create_candidate(source.root().directory)
            .map_err(retained)?;
        tree::StagingJournal::created(&mut journal, 0, parent, &candidate).map_err(retained)?;
        let tree = source
            .stage(&candidate, parent, &mut journal, progress)
            .map_err(retained)?;
        stage
            .validate_candidate(&candidate, journal.state.nodes[0].source.directory)
            .map_err(retained)?;
        Ok(Self {
            stage,
            journal,
            tree,
        })
    }
    /// Complete one admitted Copy through the same staging, original-retention,
    /// publication and receipt admission engines used by restart recovery.
    pub(super) fn execute_with_receipt(
        source: &File,
        parent: &'a File,
        central: &File,
        destination: &DestinationAdmission,
        source_label: &OsStr,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<crate::PathMoveUndoReceipt, PlatformError> {
        let mut staged = Self::create(
            source,
            parent,
            central,
            destination,
            source_label,
            max_entries,
            &mut progress,
        )?;
        let name = staged.stage.name.clone();
        let transaction = windows_fs::location(&staged.stage.directory).map_err(io)?;
        let result = (|| {
            drop(staged.prepare_destination_security(|| progress(0))?);
            // Native directory rename needs descendant capabilities released.
            drop(staged);
            if destination.original.is_some() {
                let snapshot = Snapshot::load(parent, central, &name, SnapshotAccess::Write)?;
                let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
                journal.prepare_original(max_entries, &mut progress)?;
                journal.move_original(None, OriginalMove::Retain, &mut progress)?;
            }
            let mut publication = PermissionStage::publish(parent, central, &name, &mut progress)?;
            publication.apply(&mut progress)?;
            drop(publication);
            discovery::session_receipt(parent, central, &name, &mut progress)
        })();
        result.map_err(|error| {
            let committed = inspect(parent, central, &name).is_ok_and(|state| state.completion_verified)
                || matches!(error, PlatformError::CommittedCleanupPending { .. });
            let transaction = transaction.display().to_string();
            let message = format!("{error}; Copy data and recovery evidence remain at the transaction's recorded locations");
            if committed { PlatformError::CommittedCleanupPending { transaction, message } }
            else { PlatformError::CleanupPending { transaction, message } }
        })
    }
    pub(super) fn prepare_destination_security(
        &mut self,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        if !self.journal.state.verified {
            return Err(io("Copy staging must be verified before security planning"));
        }
        self.stage.validate()?;
        self.journal.bindings()?;
        for index in 0..self.tree.entry_count() {
            self.journal.candidate_security(index)?;
        }
        let plan = self.tree.destination_security_plan(
            self.stage.parent(),
            &self.stage.candidate_name,
            &mut progress,
        )?;
        self.journal.bindings()?;
        let evidence = plan.evidence();
        self.journal
            .prepare_plan(&evidence, &mut progress)
            .map_err(|e| stage_error(&self.stage, e))?;
        plan.validate().map_err(|e| stage_error(&self.stage, e))?;
        Ok(plan)
    }
    pub(super) fn inspect(&self, central: &File) -> Result<Inspection, PlatformError> {
        inspect(self.stage.parent(), central, &self.stage.name)
    }
}
/// Explicit no-replace publication followed by journaled permission application.
/// Source-free recovery retains every entry and stops on any uncertain outcome.
pub(super) struct PermissionStage {
    journal: Journal,
    candidates: Vec<windows_fs::PerObjectFile>,
    plan: security::TreeSecurityPlan,
    original: Option<HeldOriginal>,
}

#[derive(Clone, Debug)]
struct CopyResumeBinding {
    header: Header,
    staged_digest: [u8; 32],
    // Observed retained-root version, including a rename not yet acknowledged.
    original_retained: Option<EntryVersion>,
    pending_copy: Option<EntryVersion>,
    permissions_digest: [u8; 32],
    parent_location: std::path::PathBuf,
}
fn staged_digest(state: &State) -> Result<[u8; 32], PlatformError> {
    let frozen = state
        .nodes
        .iter()
        .map(|node| {
            (
                node.parent,
                &node.name,
                &node.source,
                &node.created,
                &node.security,
                &node.staged,
            )
        })
        .collect::<Vec<_>>();
    super::evidence_digest(&(frozen, state.original.as_ref().map(|plan| &plan.evidence)))
}
fn completion_digest(state: &State) -> Result<[u8; 32], PlatformError> {
    super::evidence_digest(&(
        &state.final_versions,
        state.original.as_ref().map(|plan| &plan.evidence),
        &state.original_retained,
    ))
}
fn prepare_forward(
    retained: &RetainedStage<'_>,
    binding: Option<&CopyResumeBinding>,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<security::TreeSecurityPlan, PlatformError> {
    if !retained.snapshot.state.original_ready_for_forward() {
        return Err(io(
            "Overwrite retention has no durable intent or restoration already owns it",
        ));
    }
    if retained.snapshot.state.undo_intent {
        return Err(io("Copy is retracting or undone; use Undo recovery"));
    }
    if let Some(binding) = binding
        && (retained.snapshot.state.header.as_ref() != Some(&binding.header)
            || staged_digest(&retained.snapshot.state)? != binding.staged_digest
            || binding.original_retained.as_ref().is_some_and(|expected| {
                retained.retained_original_version().is_none_or(|actual| {
                    !retained.snapshot.state.admitted_rename_matches(
                        RenameKind::RetainOriginal,
                        expected,
                        actual,
                    )
                })
            })
            || (!retained.snapshot.state.published
                && binding.pending_copy.as_ref().is_some_and(|expected| {
                    retained.pending_copy_version().is_none_or(|actual| {
                        !retained.snapshot.state.admitted_rename_matches(
                            RenameKind::Publish,
                            expected,
                            actual,
                        )
                    })
                }))
            || windows_fs::location(retained.parent).map_err(io)? != binding.parent_location)
    {
        return Err(io("Copy resume admission changed; refresh recovery status"));
    }
    let prepared = retained.derive_destination_security(|| progress(0))?;
    if let Some(binding) = binding
        && super::evidence_digest(
            &prepared
                .evidence()
                .at_root(&binding.header.candidate_name()),
        )? != binding.permissions_digest
    {
        return Err(io("Copy destination policy changed after resume admission"));
    }
    Ok(prepared)
}

fn resume_bound_copy(
    parent: &File,
    central: &File,
    binding: &CopyResumeBinding,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let mut binding = binding.clone();
    let mut retained =
        RetainedStage::open(parent, central, &binding.header.stage_name(), &mut progress)?;
    if retained.snapshot.state.original_retain_intent
        && retained.snapshot.state.original_retained.is_none()
    {
        // Validate the admitted copy, original and destination policy read-only
        // before releasing this epoch. Preparation alone grants no retention.
        drop(prepare_forward(&retained, Some(&binding), &mut progress)?);
        let expected = retained.snapshot.bytes.clone();
        drop(retained);
        let snapshot = Snapshot::load(
            parent,
            central,
            &binding.header.stage_name(),
            SnapshotAccess::Write,
        )?;
        if snapshot.bytes != expected {
            return Err(io(
                "Copy journal changed before original-retention recovery",
            ));
        }
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        journal.move_original(
            binding.original_retained.as_ref(),
            OriginalMove::Retain,
            &mut progress,
        )?;
        drop(journal);
        retained =
            RetainedStage::open(parent, central, &binding.header.stage_name(), &mut progress)?;
        binding.original_retained = retained.retained_original_version().cloned();
    }
    if retained.snapshot.state.completion_verified {
        // Completed replay stays read-only, including readonly file roots.
        prepare_forward(&retained, Some(&binding), progress)?;
        return Ok(());
    }
    let mut publication = PermissionStage::from_retained(retained, Some(&binding), &mut progress)?;
    publication.apply(progress)
}

impl PermissionStage {
    pub(super) fn publish(
        parent: &File,
        central: &File,
        name: &OsStr,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let retained = RetainedStage::open(parent, central, name, &mut progress)?;
        Self::from_retained(retained, None, progress)
    }
    fn from_retained(
        retained: RetainedStage<'_>,
        binding: Option<&CopyResumeBinding>,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let parent = retained.parent;
        let central = retained.central;
        let name = retained
            .snapshot
            .state
            .header
            .as_ref()
            .unwrap()
            .stage_name();
        if !retained.snapshot.state.original_ready_for_publication() {
            return Err(io(
                "Original retention must be acknowledged before publication",
            ));
        }
        let prepared = prepare_forward(&retained, binding, &mut progress)?;
        let evidence = prepared.evidence().at_root(
            &retained
                .snapshot
                .state
                .header
                .as_ref()
                .unwrap()
                .candidate_name(),
        );
        let parent_policy = prepared.into_root_policy();
        let expected = retained.snapshot.bytes.clone();
        let root_name = retained.tree.root_name.clone();
        drop(retained);
        let snapshot = Snapshot::load(parent, central, &name, SnapshotAccess::Write)?;
        if snapshot.bytes != expected {
            return Err(io(
                "Copy journal changed between validation and writer acquisition",
            ));
        }
        let root = windows_fs::PerObjectFile::open(parent, &root_name)
            .map_err(|e| io(format!("opening publication root: {e}")))?;
        if let Some(expected) = binding.and_then(|binding| binding.pending_copy.as_ref())
            && snapshot.state.copy_rename_at(&root_name) == Some(RenameKind::Publish)
            && !snapshot.state.admitted_rename_matches(
                RenameKind::Publish,
                expected,
                &super::version(&root)?,
            )
        {
            return Err(io("Copy publication epoch changed at writer handoff"));
        }
        let root_evidence = snapshot.state.nodes[0].staged.as_ref().unwrap();
        if stable_identity_from_handle(&root).map_err(io)? != root_evidence.candidate_identity {
            return Err(io("Copy publication root identity changed"));
        }
        snapshot
            .state
            .validate_copy_metadata(0, &root, &root_name)?;
        namespace::validate_single_link(&root)?;
        parent_policy.validate_parent(parent)?;
        let phase = snapshot.state.permissions;
        if phase.completed > 0 {
            parent_policy.validate_candidate(&root)?;
        } else if !(phase.pending && parent_policy.matches_candidate(&root)?) {
            snapshot.state.nodes[0]
                .security
                .as_ref()
                .unwrap()
                .validate(&root, &snapshot.state.header.as_ref().unwrap().creator)?;
        }

        let original = HeldOriginal::open(parent, &snapshot.state, &mut progress)?;
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        journal.prepare_plan(&evidence, || progress(0))?;
        journal.finish_pending_readonly(&root, &mut progress)?;
        let root = journal.reopen_readonly(0, parent, &root_name, root, &mut progress)?;
        journal.reserve_permission_tail()?;
        if !journal.state.published {
            if !journal.state.publication_intent {
                journal.persist(Record::PublicationIntent)?;
            }
            let target = OsString::from_wide(&journal.header.target);
            if let Some(original) = &original {
                original.revalidate(parent, &mut progress)?;
            }
            parent_policy.validate_parent(parent)?;
            namespace::validate_single_link(&root)?;
            if root_name != target {
                crate::move_transaction::rename_windows_handle_no_replace(&root, parent, &target)?;
            }
            boundary(journal.sequence, Step::PublicationRenamed)?;
            root.sync_all()
                .map_err(|e| io(format!("flushing published root: {e}")))?;
            parent.sync_all().map_err(io)?;
            boundary(journal.sequence, Step::PublicationFlushed)?;
            namespace::validate_single_link(&root)?;
            bound(
                parent,
                &target,
                journal.state.nodes[0]
                    .staged
                    .as_ref()
                    .unwrap()
                    .candidate_identity,
            )?;
            parent_policy.validate_parent(parent)?;
            if let Some(original) = &original {
                original.revalidate(parent, &mut progress)?;
            }
            if journal.state.rename_creation.is_some()
                || super::version(&root)?.created
                    != journal.state.rename_expected(RenameKind::Publish)?.created
            {
                let proof = HeldTree::open(
                    parent,
                    &journal.state,
                    target.clone(),
                    Some(&root),
                    &mut progress,
                )?;
                proof.revalidate(parent, &journal.state, &mut progress)?;
                journal.finish_creation(RenameKind::Publish, &root)?;
                drop(proof);
                let proof = HeldTree::open(
                    parent,
                    &journal.state,
                    target.clone(),
                    Some(&root),
                    &mut progress,
                )?;
                proof.revalidate(parent, &journal.state, &mut progress)?;
            } else {
                journal.finish_creation(RenameKind::Publish, &root)?;
            }
            journal.persist(Record::Published)?;
        }
        let root_name = OsString::from_wide(&journal.header.target);
        let mut candidates = vec![root];
        for index in 1..journal.state.nodes.len() {
            let node = &journal.state.nodes[index];
            progress(0)?;
            let owner = &*candidates[node.parent.unwrap()];
            let file = windows_fs::PerObjectFile::open(owner, &OsString::from_wide(&node.name))
                .map_err(io)?;
            if stable_identity_from_handle(&file).map_err(io)?
                != node.staged.as_ref().unwrap().candidate_identity
            {
                return Err(io("Copy permission capability binding changed"));
            }
            let name = OsString::from_wide(&node.name);
            let file = journal.reopen_readonly(index, owner, &name, file, &mut progress)?;
            candidates.push(file);
        }
        let recorded = journal.state.planned.as_ref().unwrap().at_root(&root_name);
        let plan =
            security::TreeSecurityPlan::capture_at(
                parent,
                journal.state.nodes.iter().zip(&candidates).enumerate().map(
                    |(index, (node, file))| security::SecurityEntry {
                        parent: node.parent,
                        name: if index == 0 {
                            root_name.clone()
                        } else {
                            OsString::from_wide(&node.name)
                        },
                        candidate: file,
                        directory: node.source.directory,
                    },
                ),
                Some(&recorded),
                journal.state.permissions,
                || progress(0),
            )?;
        let result = Self {
            journal,
            candidates,
            plan,
            original,
        };
        result.validate(&mut progress)?;
        Ok(result)
    }
    fn validate(
        &self,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.journal.failed {
            return Err(io(
                "Copy permission owner is uncertain; reopen for validated recovery",
            ));
        }
        self.journal.bindings()?;
        if let Some(original) = &self.original {
            original.revalidate(&self.journal.parent_dir, progress)?;
        }
        self.plan.validate_at(self.journal.state.permissions)?;
        if self
            .journal
            .state
            .final_versions
            .iter()
            .zip(self.plan.final_versions())
            .any(|(recorded, current)| recorded != current)
        {
            return Err(io("Copy final receipt prefix changed; all data retained"));
        }
        for index in 0..self.candidates.len() {
            self.validate_payload(index, progress)?;
        }
        if windows_fs::directory_children_limited(&self.journal.stage_dir, 1).map_err(io)?
            != [OsString::from(LOCAL)]
        {
            return Err(io(
                "Unexpected Copy transaction entry; no permission change allowed",
            ));
        }
        self.plan.validate_at(self.journal.state.permissions)?;
        self.journal.bindings()
    }
    fn validate_payload(
        &self,
        index: usize,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        progress(0)?;
        let evidence = self.journal.state.nodes[index].staged.as_ref().unwrap();
        let file = &self.candidates[index];
        self.journal.state.validate_copy_metadata(
            index,
            file,
            &OsString::from_wide(&self.journal.header.target),
        )?;
        if super::transfer(file, None, progress)? != evidence.streams
            || links::capture(file)? != evidence.source.link
        {
            return Err(io(
                "Copy permission candidate contents changed; all data retained",
            ));
        }
        self.journal.state.validate_copy_metadata(
            index,
            file,
            &OsString::from_wide(&self.journal.header.target),
        )
    }
    pub(super) fn apply(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.validate(&mut progress)?;
        while self.journal.state.permissions.completed < self.candidates.len() {
            progress(0)?;
            self.journal.bindings()?;
            let index = self.journal.state.permissions.completed;
            self.validate_payload(index, &mut progress)?;
            if !self.journal.state.permissions.pending {
                self.journal
                    .persist(Record::SecurityApplyIntent { index })?;
            }
            // Native outcome can be uncertain even if the API returns failure.
            // No in-process retry from stale observations: fresh recovery only.
            self.journal.failed = true;
            self.plan.apply_pending(
                self.journal.state.permissions,
                &self.candidates[index],
                |flushed| {
                    boundary(
                        self.journal.sequence,
                        if flushed {
                            Step::PermissionsFlushed
                        } else {
                            Step::PermissionsChanged
                        },
                    )
                },
            )?;
            self.journal.failed = false;
            self.journal.persist(Record::SecurityApplied { index })?;
        }
        self.validate(&mut progress)?;
        if !self.journal.state.completion_verified {
            self.journal.reserve_permission_tail()?;
            for (index, version) in self
                .plan
                .final_versions()
                .enumerate()
                .skip(self.journal.state.final_versions.len())
            {
                progress(0)?;
                self.journal.persist(Record::FinalEntry {
                    index,
                    version: Box::new(version.clone()),
                })?;
            }
            self.validate(&mut progress)?;
            self.journal.persist(Record::Finalized {
                count: self.candidates.len(),
                digest: super::evidence_digest(&self.journal.state.final_versions)?,
            })?;
        }
        Ok(())
    }
    pub(super) fn completed(&self) -> usize {
        self.journal.state.permissions.completed
    }
}

fn stage_error(stage: &namespace::PrivateStage<'_>, error: PlatformError) -> PlatformError {
    PlatformError::CleanupPending {
        transaction: windows_fs::location(&stage.directory)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| format!("{:?}", stage.name)),
        message: format!("{error}; Copy staging retained; no publication or cleanup attempted"),
    }
}

#[derive(Debug)]
pub(super) struct Inspection {
    pub(super) entries: usize,
    pub(super) created: usize,
    pub(super) staged: usize,
    pub(super) verified: bool,
    pub(super) security_planned: bool,
    pub(super) published: bool,
    pub(super) publication_pending: bool,
    pub(super) completion_verified: bool,
    pub(super) completion_entries: usize,
    pub(super) readonly_reopening: Option<usize>,
    pub(super) undo_pending: bool,
    pub(super) undone: bool,
    pub(super) permissions_completed: usize,
    pub(super) permission_pending: bool,
}
fn read(file: &File) -> Result<Vec<u8>, PlatformError> {
    let size = file.metadata().map_err(io)?.len();
    if size == 0 || size > MAX_LOG {
        return Err(io("invalid Copy journal size"));
    }
    let mut bytes = Vec::new();
    let mut reader = file;
    reader.seek(SeekFrom::Start(0)).map_err(io)?;
    reader.take(size + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 != size || file.metadata().map_err(io)?.len() != size {
        return Err(io("Copy journal changed while reading"));
    }
    Ok(bytes)
}
/// Held journal evidence. Read-only recovery refuses an active writer; explicit
/// application opens fresh exclusive write handles after read-only validation.
struct Snapshot {
    claim: Option<claims::Lease>,
    stage: File,
    local: File,
    mirror: File,
    bytes: Vec<u8>,
    state: State,
    requires_creator: bool,
    sequence: u64,
    previous: [u8; 32],
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapshotAccess {
    Inspect,
    Read,
    Write,
}
impl Snapshot {
    fn root_name(&self, parent: &File) -> Result<OsString, PlatformError> {
        let header = self.state.header.as_ref().unwrap();
        let candidate = header.candidate_name();
        let target = OsString::from_wide(&header.target);
        if self.state.published && !self.state.undo_intent {
            return Ok(target);
        }
        if !self.state.publication_intent && !self.state.undo_intent {
            return Ok(candidate);
        }
        let expected = self.state.nodes[0]
            .staged
            .as_ref()
            .unwrap()
            .candidate_identity;
        let probe = |name: &OsStr| -> Result<bool, PlatformError> {
            match unsafe {
                windows_fs::open_relative(
                    parent,
                    name,
                    RelativeOptions {
                        directory: None,
                        create_new: false,
                        access: windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
                            | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
                        sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        security: ptr::null(),
                    },
                )
            } {
                Ok(file) => Ok(stable_identity_from_handle(&file).map_err(io)? == expected),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(io(error)),
            }
        };
        match (probe(&candidate)?, probe(&target)?) {
            (true, false) => Ok(candidate),
            (false, true)
                if self.state.undone.is_none()
                    && !(self.state.undo_intent && self.state.readonly_pending.is_some()) =>
            {
                Ok(target)
            }
            _ => Err(io(
                "Pending Copy publication/Undo has missing or aliased root bindings; data retained",
            )),
        }
    }
    fn load(
        parent: &File,
        central: &File,
        name: &OsStr,
        access: SnapshotAccess,
    ) -> Result<Self, PlatformError> {
        Self::load_control(parent, central, name, access, None)
    }

    fn read_with_control(
        parent: &File,
        central: &File,
        name: &OsStr,
        control: &File,
    ) -> Result<Self, PlatformError> {
        Self::load_control(parent, central, name, SnapshotAccess::Read, Some(control))
    }

    fn load_control(
        parent: &File,
        central: &File,
        name: &OsStr,
        access: SnapshotAccess,
        control: Option<&File>,
    ) -> Result<Self, PlatformError> {
        if control.is_some() && access != SnapshotAccess::Read {
            return Err(io("A held peer control is only a read capability"));
        }
        let entry = |parent: &File, name: &OsStr, directory| {
            if access == SnapshotAccess::Inspect {
                open(parent, name, Some(directory), FILE_GENERIC_READ)
            } else if access == SnapshotAccess::Read || directory {
                namespace::open_source(parent, name, Some(directory))
            } else {
                if !stable_identity_from_handle(parent)
                    .map_err(io)?
                    .is_directory_without_reparse()
                {
                    return Err(io("Copy journal parent is not an ordinary directory"));
                }
                let file = unsafe {
                    windows_fs::open_relative(
                        parent,
                        name,
                        RelativeOptions {
                            directory: Some(false),
                            create_new: false,
                            access: FILE_GENERIC_READ
                                | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                            sharing: FILE_SHARE_READ,
                            security: ptr::null(),
                        },
                    )
                }
                .map_err(io)?;
                if !stable_identity_from_handle(&file)
                    .map_err(io)?
                    .is_regular_file_without_reparse()
                {
                    return Err(io("Copy journal is not an ordinary file"));
                }
                Ok(file)
            }
        };
        let stage = entry(parent, name, true)?;
        let local = entry(&stage, OsStr::new(LOCAL), false)?;
        let bytes = read(&local)?;
        let Decoded {
            state,
            sequence,
            previous,
        } = decode(&bytes)?;
        let header = state.header.as_ref().unwrap();
        if name != header.stage_name() {
            return Err(io("Copy journal name does not match its transaction"));
        }
        let mirror = if let Some(control) = control {
            control.try_clone().map_err(io)?
        } else {
            entry(central, &header.central_name(), false)?
        };
        // A recognized partial release is only an observation here. The full
        // paired-journal/identity checks below must pass before this snapshot
        // can supply a writer with terminal release authority.
        let claim = if state.claim_terminal() {
            None
        } else {
            Some(claims::Lease::open(parent, &header.claim, false, false)?)
        };
        let mut result = Self {
            claim,
            stage,
            local,
            mirror,
            bytes,
            state,
            requires_creator: access != SnapshotAccess::Inspect,
            sequence,
            previous,
        };
        // Establish paired terminal authority before a missing/replaced name
        // control can be treated as discharged. No public mutation follows.
        result.revalidate(parent, central)?;
        if result.state.claim_terminal() {
            result.claim = claims::Lease::observe_terminal(
                parent,
                &result.state.header.as_ref().unwrap().claim,
            )?;
            result.revalidate(parent, central)?;
        }
        Ok(result)
    }
    fn revalidate(&self, parent: &File, central: &File) -> Result<(), PlatformError> {
        match &self.claim {
            Some(claim) => claim.validate(self.state.claim_terminal())?,
            None if self.state.claim_terminal() => {}
            None => return Err(io("Active Copy snapshot lost its destination claim")),
        }
        let header = self.state.header.as_ref().unwrap();
        header.validate_parent_policy(parent)?;
        namespace::validate_single_link(&self.local)?;
        namespace::validate_single_link(&self.mirror)?;
        if self.requires_creator && security::current_owner()? != header.creator {
            return Err(io(
                "retained Copy creator does not match the effective token owner",
            ));
        }
        header
            .stage_security
            .validate(&self.stage, &header.creator)?;
        header
            .local_security
            .validate(&self.local, &header.creator)?;
        header
            .central_security
            .validate(&self.mirror, &header.creator)?;
        for (file, expected) in [
            (parent, header.parent),
            (&self.stage, header.stage),
            (central, header.central),
            (&self.local, header.local_file),
            (&self.mirror, header.central_file),
        ] {
            if stable_identity_from_handle(file).map_err(io)? != expected {
                return Err(io("Copy journal header does not match held namespace"));
            }
        }
        if read(&self.local)? != self.bytes || read(&self.mirror)? != self.bytes {
            return Err(io("Copy journal mirrors differ or changed; data retained"));
        }
        bound(parent, &header.stage_name(), header.stage)?;
        bound(&self.stage, OsStr::new(LOCAL), header.local_file)?;
        bound(central, &header.central_name(), header.central_file)?;
        Ok(())
    }
}
/// Journal-only inspection never grants publication, restoration or deletion.
pub(super) fn inspect(
    parent: &File,
    central: &File,
    name: &OsStr,
) -> Result<Inspection, PlatformError> {
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Inspect)?;
    Ok(Inspection {
        entries: snapshot.state.nodes.len(),
        created: snapshot
            .state
            .nodes
            .iter()
            .filter(|n| n.created.is_some())
            .count(),
        staged: snapshot
            .state
            .nodes
            .iter()
            .filter(|n| n.staged.is_some())
            .count(),
        verified: snapshot.state.verified,
        security_planned: snapshot.state.planned.is_some(),
        published: snapshot.state.published,
        publication_pending: snapshot.state.publication_intent && !snapshot.state.published,
        completion_verified: snapshot.state.completion_verified,
        completion_entries: snapshot.state.final_versions.len(),
        readonly_reopening: snapshot.state.readonly_pending,
        undo_pending: snapshot.state.undo_intent && !snapshot.state.undo_complete(),
        undone: snapshot.state.undo_complete(),
        permissions_completed: snapshot.state.permissions.completed,
        permission_pending: snapshot.state.permissions.pending,
    })
}

struct RetainedEntry {
    handle: File,
    observed: EntryVersion,
    children: Vec<OsString>,
}
/// Content/namespace validation of a completed private stage, without acquiring
/// source handles or write/delete rights. This is not cleanup authority. Keep
/// handles alive and revalidate before any later transaction phase; publication
/// and guarded mutation require their own state/rights/namespace protocol.
struct HeldOriginal {
    tree: tree::PinnedTree,
    evidence: tree::ExistingTreeEvidence,
    name: OsString,
}
impl HeldOriginal {
    fn open(
        parent: &File,
        state: &State,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Option<Self>, PlatformError> {
        let Some(plan) = &state.original else {
            return Ok(None);
        };
        if !plan.sealed {
            return Ok(None);
        }
        let header = state.header.as_ref().unwrap();
        let public = OsString::from_wide(&header.target);
        let retained = header.original_name();
        let (from, to, intent, completed, base) = if state.original_restore_intent {
            (
                retained,
                public,
                true,
                state.original_restored.as_ref(),
                state.original_retained.as_ref().unwrap(),
            )
        } else {
            (
                public,
                retained,
                state.original_retain_intent,
                state.original_retained.as_ref(),
                &plan.evidence.entries[0].version,
            )
        };
        let name = match (
            identity_at(parent, &from, base.identity)?,
            identity_at(parent, &to, base.identity)?,
        ) {
            (true, false) if completed.is_none() => from,
            (false, true) if intent => to.clone(),
            _ => {
                return Err(io(
                    "original destination identity changed or has ambiguous locations",
                ));
            }
        };
        let kind = if state.original_restore_intent {
            RenameKind::RestoreOriginal
        } else {
            RenameKind::RetainOriginal
        };
        state.validate_creation_location(kind, &name)?;
        let root = namespace::open_source(parent, &name, None)?;
        let tree = tree::PinnedTree::open(&root, plan.evidence.entries.len(), &mut *progress)?;
        let mut evidence = plan.evidence.clone();
        evidence.entries[0].version = completed.unwrap_or(base).clone();
        if intent && completed.is_none() && name == to {
            state.validate_creation_epoch(kind, tree.root())?;
            evidence.entries[0].version.created = tree.root().created;
            evidence.entries[0].version.changed = tree.root().changed;
        }
        let held = Self {
            tree,
            evidence,
            name,
        };
        held.revalidate(parent, progress)?;
        Ok(Some(held))
    }
    fn revalidate(
        &self,
        parent: &File,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.tree
            .validate_existing(&self.evidence, &mut *progress)?;
        namespace::validate_binding(
            parent,
            &self.name,
            self.evidence.entries[0].version.identity,
        )
    }
}
pub(super) struct RetainedStage<'a> {
    parent: &'a File,
    central: &'a File,
    snapshot: Snapshot,
    tree: HeldTree,
    original: Option<HeldOriginal>,
}
impl<'a> RetainedStage<'a> {
    pub(super) fn open(
        parent: &'a File,
        central: &'a File,
        name: &OsStr,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
        Self::from_snapshot(parent, central, snapshot, progress)
    }
    fn from_snapshot(
        parent: &'a File,
        central: &'a File,
        snapshot: Snapshot,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if snapshot.state.cleanup.is_some() {
            return Err(io(
                "Copy cleanup has started; forward/receipt admission refused",
            ));
        }
        let root_name = snapshot.root_name(parent)?;
        let tree = HeldTree::open(parent, &snapshot.state, root_name, None, &mut progress)?;
        let original = HeldOriginal::open(parent, &snapshot.state, &mut progress)?;
        let result = Self {
            parent,
            central,
            snapshot,
            tree,
            original,
        };
        result.revalidate(progress)?;
        Ok(result)
    }
    fn retained_original_version(&self) -> Option<&EntryVersion> {
        self.original
            .as_ref()
            .filter(|original| {
                original.name == self.snapshot.state.header.as_ref().unwrap().original_name()
            })
            .map(|original| &original.evidence.entries[0].version)
    }
    pub(super) fn plan_destination_security(
        &self,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        self.snapshot.revalidate(self.parent, self.central)?;
        self.tree
            .validate_contents(self.parent, &self.snapshot.state, &mut |_| progress())?;
        let plan = self.derive_destination_security(progress)?;
        self.snapshot.revalidate(self.parent, self.central)?;
        Ok(plan)
    }
    fn derive_destination_security(
        &self,
        progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        self.tree
            .derive_destination_security(self.parent, &self.snapshot.state, progress)
    }
    pub(super) fn entry_count(&self) -> usize {
        self.tree.entries.len()
    }
    pub(super) fn revalidate(
        &self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.snapshot.revalidate(self.parent, self.central)?;
        validate_stage_entries(&self.snapshot.stage)?;
        if let Some(original) = &self.original {
            original.revalidate(self.parent, &mut progress)?;
        }
        self.tree
            .revalidate(self.parent, &self.snapshot.state, &mut progress)?;
        self.snapshot.revalidate(self.parent, self.central)
    }
}
fn validate_stage_entries(stage: &File) -> Result<(), PlatformError> {
    if windows_fs::directory_children_limited(stage, 1).map_err(io)? != [OsString::from(LOCAL)] {
        return Err(io("unexpected Copy transaction entries; all data retained"));
    }
    Ok(())
}
/// One payload/namespace/security verifier for read-only inspection and the
/// explicitly owned Undo epoch. Windows requires descendant readers to close
/// for a directory rename; all frozen versions are revalidated after that handoff.
struct HeldTree {
    entries: Vec<RetainedEntry>,
    root_name: OsString,
}
impl HeldTree {
    fn open(
        parent: &File,
        state: &State,
        root_name: OsString,
        root_capability: Option<&windows_fs::PerObjectFile>,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if !state.verified {
            return Err(io(
                "Copy staging was interrupted before full verification; retain for recovery",
            ));
        }
        let mut children = vec![Vec::new(); state.nodes.len()];
        for node in &state.nodes {
            if let Some(parent) = node.parent {
                children[parent].push(OsString::from_wide(&node.name));
            }
        }
        for names in &mut children {
            names.sort_unstable_by(|a, b| a.encode_wide().cmp(b.encode_wide()));
        }
        let mut entries: Vec<RetainedEntry> = Vec::with_capacity(children.len());
        for (index, node) in state.nodes.iter().enumerate() {
            progress(0)?;
            let owner = node.parent.map(|p| &entries[p].handle).unwrap_or(parent);
            let name = if index == 0 {
                root_name.clone()
            } else {
                OsString::from_wide(&node.name)
            };
            let handle = match (index, root_capability) {
                (0, Some(capability)) => std::fs::File::try_clone(capability).map_err(io)?,
                _ => namespace::open_source(owner, &name, None)?,
            };
            let evidence = node.staged.as_ref().unwrap();
            if state.permissions == security::PermissionProgress::default() {
                node.security
                    .as_ref()
                    .unwrap()
                    .validate(&handle, &state.header.as_ref().unwrap().creator)?;
            }
            state.validate_copy_metadata(index, &handle, &root_name)?;
            let observed = super::version(&handle)?;
            if observed.link != evidence.source.link {
                return Err(io("retained Copy link data changed; no target followed"));
            }
            entries.push(RetainedEntry {
                handle,
                observed,
                children: std::mem::take(&mut children[index]),
            });
        }
        Ok(Self { entries, root_name })
    }
    fn derive_destination_security(
        &self,
        parent: &File,
        state: &State,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        let recorded = state
            .planned
            .as_ref()
            .map(|plan| plan.at_root(&self.root_name));
        let plan = security::TreeSecurityPlan::capture_at(
            parent,
            state
                .nodes
                .iter()
                .zip(&self.entries)
                .enumerate()
                .map(|(index, (node, entry))| security::SecurityEntry {
                    parent: node.parent,
                    name: if index == 0 {
                        self.root_name.clone()
                    } else {
                        OsString::from_wide(&node.name)
                    },
                    candidate: &entry.handle,
                    directory: node.source.directory,
                }),
            recorded.as_ref(),
            state.permissions,
            &mut progress,
        )?;
        self.namespace(parent, state)?;
        plan.validate_at(state.permissions)?;
        let evidence = plan.evidence();
        if state
            .planned
            .as_ref()
            .is_some_and(|recorded| recorded.at_root(&self.root_name) != evidence)
            || state
                .planning
                .as_ref()
                .is_some_and(|partial| !partial.evidence.is_prefix_of(&evidence, partial.expected))
        {
            return Err(io(
                "recorded Copy destination security plan no longer matches native inheritance; data retained",
            ));
        }
        Ok(plan)
    }
    fn namespace(&self, parent: &File, state: &State) -> Result<(), PlatformError> {
        for (index, (node, entry)) in state.nodes.iter().zip(&self.entries).enumerate() {
            if state.permissions == security::PermissionProgress::default() {
                node.security
                    .as_ref()
                    .unwrap()
                    .validate(&entry.handle, &state.header.as_ref().unwrap().creator)?;
            }
            let owner = node
                .parent
                .map(|p| &self.entries[p].handle)
                .unwrap_or(parent);
            let name = if index == 0 {
                self.root_name.clone()
            } else {
                OsString::from_wide(&node.name)
            };
            bound(owner, &name, entry.observed.identity)?;
            namespace::validate_single_link(&entry.handle)?;
            state.validate_final_version(index, &entry.observed, &self.root_name)?;
            if super::version(&entry.handle)? != entry.observed {
                return Err(io("retained Copy entry changed during validation"));
            }
            if entry.observed.directory
                && entry.observed.link.is_none()
                && windows_fs::directory_children_limited(
                    &entry.handle,
                    entry.children.len().saturating_add(1),
                )
                .map_err(io)?
                    != entry.children
            {
                return Err(io("retained Copy directory membership changed"));
            }
        }
        Ok(())
    }
    fn revalidate(
        &self,
        parent: &File,
        state: &State,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.validate_contents(parent, state, progress)?;
        if state.permissions != security::PermissionProgress::default() {
            self.derive_destination_security(parent, state, || Ok(()))?;
        }
        Ok(())
    }
    fn validate_contents(
        &self,
        parent: &File,
        state: &State,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.namespace(parent, state)?;
        let mut total = 0u64;
        for (node, entry) in state.nodes.iter().zip(&self.entries) {
            let evidence = node.staged.as_ref().unwrap();
            let actual = super::transfer(&entry.handle, None, &mut |count| {
                progress(
                    total
                        .checked_add(count)
                        .ok_or_else(|| io("Copy recovery progress overflow"))?,
                )
            })?;
            if actual != evidence.streams {
                return Err(io(
                    "retained Copy stream contents changed; all data retained",
                ));
            }
            total = actual.iter().try_fold(total, |sum, stream| {
                sum.checked_add(stream.bytes)
                    .ok_or_else(|| io("Copy recovery stream length overflow"))
            })?;
        }
        self.namespace(parent, state)
    }
}

/// Undo a completed creation by retaining its exact copied tree at the original
/// candidate slot. Never delete data, overwrite a name, or read/mutate the source.
/// A fresh call also resolves and finishes an interrupted inverse by identity.
pub(super) fn undo_completed_copy(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    undo_bound_copy(parent, central, name, None, &mut progress)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CopyUndoBinding {
    header: Header,
    completion_digest: [u8; 32],
    parent_location: std::path::PathBuf,
    pending_copy: Option<EntryVersion>,
    pending_original: Option<EntryVersion>,
}

fn read_undo_snapshot(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Snapshot, PlatformError> {
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
    if !snapshot.state.completion_verified || snapshot.state.cleanup.is_some() {
        return Err(io("Copy has no completed receipt; Undo refused"));
    }
    if snapshot.state.undone.is_some() && snapshot.state.original.is_some() {
        // The replacement was already durably withdrawn. Restoring the original
        // must not depend on its private copy remaining unchanged afterward.
        let original = HeldOriginal::open(parent, &snapshot.state, &mut progress)?;
        snapshot.revalidate(parent, central)?;
        drop(original);
        Ok(snapshot)
    } else {
        Ok(RetainedStage::from_snapshot(parent, central, snapshot, progress)?.snapshot)
    }
}
fn undo_bound_copy(
    parent: &File,
    central: &File,
    name: &OsStr,
    binding: Option<&CopyUndoBinding>,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let snapshot = read_undo_snapshot(parent, central, name, &mut progress)?;
    if let Some(binding) = binding {
        let (copy, original) = pending_undo_epochs(parent, &snapshot)?;
        if binding
            .pending_copy
            .as_ref()
            .zip(copy.as_ref())
            .is_some_and(|(expected, actual)| {
                !snapshot
                    .state
                    .admitted_rename_matches(RenameKind::Withdraw, expected, actual)
            })
            || binding
                .pending_original
                .as_ref()
                .zip(original.as_ref())
                .is_some_and(|(expected, actual)| {
                    !snapshot.state.admitted_rename_matches(
                        RenameKind::RestoreOriginal,
                        expected,
                        actual,
                    )
                })
        {
            return Err(io("Copy Undo admission epoch changed"));
        }
    }
    if let Some(binding) = binding
        && (snapshot.state.header.as_ref() != Some(&binding.header)
            || completion_digest(&snapshot.state)? != binding.completion_digest
            || windows_fs::location(parent).map_err(io)? != binding.parent_location)
    {
        return Err(io("Copy Undo admission changed; refresh recovery status"));
    }
    if snapshot.state.undo_complete() {
        let claim = snapshot.claim;
        // Keep both terminal journals and their stage leased until the release
        // is durable. Dropping them first would let another writer invalidate
        // the paired evidence that authorizes relinquishing this name.
        if let Some(claim) = claim {
            claim.release_after_terminal()?;
        }
        drop((snapshot.stage, snapshot.local, snapshot.mirror));
        return Ok(());
    }
    let expected = snapshot.bytes.clone();
    drop(snapshot);
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Write)?;
    if snapshot.bytes != expected {
        return Err(io("Copy journal changed before Undo writer acquisition"));
    }
    if snapshot.state.undone.is_some() {
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        return journal.move_original(
            binding.and_then(|binding| binding.pending_original.as_ref()),
            OriginalMove::Restore,
            progress,
        );
    }
    let original = HeldOriginal::open(parent, &snapshot.state, &mut progress)?;
    let root_name = snapshot.root_name(parent)?;
    let mut root = windows_fs::PerObjectFile::open(parent, &root_name).map_err(io)?;
    if let Some(expected) = binding.and_then(|binding| binding.pending_copy.as_ref())
        && snapshot.state.copy_rename_at(&root_name) == Some(RenameKind::Withdraw)
        && !snapshot.state.admitted_rename_matches(
            RenameKind::Withdraw,
            expected,
            &super::version(&root)?,
        )
    {
        return Err(io("Copy withdrawal epoch changed at writer handoff"));
    }
    let mut tree = HeldTree::open(
        parent,
        &snapshot.state,
        root_name,
        Some(&root),
        &mut progress,
    )?;
    validate_stage_entries(&snapshot.stage)?;
    tree.revalidate(parent, &snapshot.state, &mut progress)?;
    let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
    journal.reserve_undo_tail()?;
    let result = (|| {
        if !journal.state.undo_intent {
            journal.persist(Record::UndoIntent)?;
        }
        progress(0)?;
        tree.revalidate(parent, &journal.state, &mut progress)?;
        journal.bindings()?;
        let candidate = journal.header.candidate_name();
        let needs_rename = tree.root_name != candidate;
        // NTFS refuses a directory rename while descendant handles remain
        // open, regardless of their delete-sharing flags. Keep the root bound,
        // release readers only for this handoff, then reacquire and verify all
        // immutable evidence before acknowledging Undo. No data is deleted.
        drop(tree);
        if needs_rename {
            undo_handoff(journal.sequence)?;
            crate::move_transaction::rename_windows_handle_no_replace(&root, parent, &candidate)?;
            boundary(journal.sequence, Step::UndoRenamed)?;
        }
        root = journal.reopen_readonly(0, parent, &candidate, root, &mut progress)?;
        tree = HeldTree::open(
            parent,
            &journal.state,
            candidate,
            Some(&root),
            &mut progress,
        )?;
        root.sync_all().map_err(io)?;
        parent.sync_all().map_err(io)?;
        boundary(journal.sequence, Step::UndoFlushed)?;
        journal.bindings()?;
        validate_stage_entries(&journal.stage_dir)?;
        tree.revalidate(parent, &journal.state, &mut progress)?;
        if let Some(original) = &original {
            original.revalidate(parent, &mut progress)?;
        }
        journal.finish_creation(RenameKind::Withdraw, &root)?;
        drop(tree);
        tree = HeldTree::open(
            parent,
            &journal.state,
            journal.header.candidate_name(),
            Some(&root),
            &mut progress,
        )?;
        tree.revalidate(parent, &journal.state, &mut progress)?;
        journal.persist(Record::Undone {
            root_version: Box::new(tree.entries[0].observed.clone()),
        })?;
        drop(original);
        if journal.state.original.is_some() {
            journal.move_original(None, OriginalMove::Restore, &mut progress)?;
        }
        Ok(())
    })();
    result.map_err(|error: PlatformError| PlatformError::CleanupPending {
        transaction: windows_fs::location(&journal.stage_dir)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| format!("{:?}", journal.header.stage_name())),
        message: format!(
            "{error}; Copy Undo retains all data at its published name or at {}; reopen the transaction for recovery",
            windows_fs::location(&journal.parent_dir)
                .map(|path| path.join(journal.header.candidate_name()).display().to_string())
                .unwrap_or_else(|_| format!("{:?}", journal.header.candidate_name())),
        ),
    })
}

/// An opened, source-free, native-validated durable completion receipt.
/// This read-only proof alone does not grant Undo, deletion or cleanup authority.
/// The paired journal remains the only persisted representation.
pub(super) struct CompletedCopy<'a> {
    retained: RetainedStage<'a>,
}
impl<'a> CompletedCopy<'a> {
    pub(super) fn open(
        parent: &'a File,
        central: &'a File,
        name: &OsStr,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let retained = RetainedStage::open(parent, central, name, progress)?;
        if retained.snapshot.state.undo_intent {
            return Err(io(
                "Copy is retracting or undone; completed-copy receipt unavailable",
            ));
        }
        if !retained.snapshot.state.completion_verified {
            return Err(io("Copy has no sealed completion receipt; data retained"));
        }
        Ok(Self { retained })
    }
    pub(super) fn entry_count(&self) -> usize {
        self.retained.snapshot.state.final_versions.len()
    }
    pub(super) fn root_version(&self) -> &EntryVersion {
        &self.retained.snapshot.state.final_versions[0]
    }
    pub(super) fn revalidate(
        &self,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.retained.revalidate(progress)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupMetadataRole {
    Local,
    Stage,
    Central,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupMetadataPoint {
    DispositionSet,
    HandleClosed,
    Flushed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    OriginalReadersReleased,
    OriginalRenamed,
    OriginalFlushed,
    UndoReadersReleased,
    UndoRenamed,
    UndoFlushed,
    ReadonlyWritable,
    ReadonlyRestored,
    ReadonlyFlushed,
    LocalFlushed,
    CentralFlushed,
    PublicationRenamed,
    PublicationFlushed,
    CreationRestored,
    CreationFlushed,
    CleanupDispositionSet,
    CleanupReadersReleased,
    CleanupHandleClosed,
    CleanupFlushed,
    CleanupMetadata {
        role: CleanupMetadataRole,
        point: CleanupMetadataPoint,
    },
    PermissionsChanged,
    PermissionsFlushed,
}
#[cfg(test)]
thread_local! {static UNDO_HANDOFF:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=const {std::cell::RefCell::new(None)};}
fn undo_handoff(sequence: u64) -> Result<(), PlatformError> {
    boundary(sequence, Step::UndoReadersReleased)?;
    #[cfg(test)]
    if let Some(hook) = UNDO_HANDOFF.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    Ok(())
}

#[cfg(test)]
thread_local! {static ORIGINAL_HANDOFF:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=const {std::cell::RefCell::new(None)};}
fn original_handoff(sequence: u64) -> Result<(), PlatformError> {
    boundary(sequence, Step::OriginalReadersReleased)?;
    #[cfg(test)]
    if let Some(hook) = ORIGINAL_HANDOFF.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    Ok(())
}

#[cfg(test)]
thread_local! {static CLEANUP_HANDOFF:std::cell::RefCell<Option<Box<dyn FnOnce()>>>=const {std::cell::RefCell::new(None)};}
fn cleanup_handoff(sequence: u64) -> Result<(), PlatformError> {
    boundary(sequence, Step::CleanupReadersReleased)?;
    #[cfg(test)]
    if let Some(hook) = CLEANUP_HANDOFF.with(|slot| slot.borrow_mut().take()) {
        hook();
    }
    Ok(())
}

#[cfg(not(test))]
fn boundary(_: u64, _: Step) -> Result<(), PlatformError> {
    Ok(())
}
#[cfg(test)]
thread_local! {static FAULT:std::cell::Cell<Option<(u64,Step,bool)>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
fn boundary(sequence: u64, step: Step) -> Result<(), PlatformError> {
    if FAULT.with(|fault| fault.get()) == Some((sequence, step, true)) {
        std::process::exit(86);
    }
    if FAULT.with(|fault| fault.get()) == Some((sequence, step, false)) {
        return Err(io("injected Copy journal flush boundary failure"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    mod claim_orphans;
    mod claim_processes;
    mod claim_publication;
    mod claim_releases;
    mod claim_repair;
    mod claim_retirement;
    mod cleanup;
    mod creation_times;
    mod initialization;
    mod name_release_action;

    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, path::Path};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    fn source(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn owner(path: &Path) -> File {
        super::super::tests::parent(path)
    }
    fn fixture(root: &Path) {
        std::fs::create_dir(root.join("source")).unwrap();
        std::fs::create_dir(root.join("source/nested")).unwrap();
        std::fs::write(root.join("source/nested/file"), b"original data").unwrap();
        std::fs::create_dir(root.join("destination")).unwrap();
        drop(
            windows_fs::create_private(
                &owner(root),
                OsStr::new("central"),
                true,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
            .unwrap(),
        );
    }
    fn stage<'a>(
        src: &File,
        parent: &'a File,
        central: &File,
    ) -> Result<DurableStage<'a>, PlatformError> {
        DurableStage::create(
            src,
            parent,
            central,
            &DestinationAdmission::creation(parent, OsStr::new("copy"))?,
            OsStr::new("source"),
            100,
            |_| Ok(()),
        )
    }
    fn stage_name(parent: &Path) -> OsString {
        let names: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| {
                n.to_str()
                    .and_then(|name| name.strip_prefix(".ottrin-copy-"))
                    .is_some_and(|id| {
                        id.len() == 32
                            && id
                                .bytes()
                                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                    })
            })
            .collect();
        assert_eq!(names.len(), 1);
        names[0].clone()
    }

    fn original_fixture(root: &Path, directory: bool, readonly: bool) -> OsString {
        fixture(root);
        let target = root.join("destination/copy");
        if directory {
            std::fs::create_dir(&target).unwrap();
            std::fs::write(target.join("old"), b"old destination").unwrap();
            std::fs::write(target.join("second"), b"other destination").unwrap();
        } else {
            std::fs::write(&target, b"old destination").unwrap();
        }
        if readonly {
            let mut permissions = std::fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&target, permissions).unwrap();
        }
        let src = source(&root.join("source"));
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let destination =
            DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap();
        let mut staged = DurableStage::create(
            &src,
            &parent,
            &central,
            &destination,
            OsStr::new("source"),
            100,
            |_| Ok(()),
        )
        .unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        staged.stage.name.clone()
    }
    #[test]
    fn windows_native_destination_admission_freezes_the_original_before_source_staging() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let target = root.path().join("destination/copy");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("old"), b"approved original").unwrap();
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission =
            DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(())).unwrap();
        let root_before = super::super::version(&source(&target)).unwrap();
        let mut changed = false;
        let mut staged = DurableStage::create(
            &source(&root.path().join("source")),
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |bytes| {
                if bytes > 0 && !changed {
                    std::fs::write(target.join("old"), b"changed during staging").unwrap();
                    changed = true;
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            super::super::version(&source(&target)).unwrap(),
            root_before,
            "Root-only validation must miss this child-content change"
        );
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        drop(staged);
        remove_undo_fixture_source(root.path());
        let local = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&local).unwrap();
        let mut journal = original_journal(root.path(), &name);
        assert!(
            journal
                .prepare_original(100, |_| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("approved snapshot")
        );
        assert!(journal.state.original.is_none());
        assert_eq!(std::fs::read(&local).unwrap(), before);
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("central")
                    .join(journal.header.central_name())
            )
            .unwrap(),
            before
        );
        assert_eq!(
            std::fs::read(target.join("old")).unwrap(),
            b"changed during staging"
        );
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(candidate)
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
    }
    #[test]
    fn windows_native_destination_admission_rejects_stale_parent_target_and_creation_before_staging()
     {
        for case in ["creation-occupied", "overwrite-changed", "wrong-parent"] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let target = root.path().join("destination/copy");
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let admission = if case == "creation-occupied" {
                DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
            } else {
                std::fs::write(&target, b"approved").unwrap();
                DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(()))
                    .unwrap()
            };
            if case != "wrong-parent" {
                std::fs::write(&target, b"unrelated current occupant").unwrap();
            }
            let other = root.path().join("other");
            std::fs::create_dir(&other).unwrap();
            let actual_parent = if case == "wrong-parent" {
                owner(&other)
            } else {
                owner(&root.path().join("destination"))
            };
            assert!(
                DurableStage::create(
                    &source(&root.path().join("source")),
                    &actual_parent,
                    &central,
                    &admission,
                    OsStr::new("source"),
                    100,
                    |_| Ok(())
                )
                .is_err()
            );
            assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
            assert_eq!(
                std::fs::read_dir(root.path().join("central"))
                    .unwrap()
                    .count(),
                0
            );
            assert_eq!(
                std::fs::read_dir(root.path().join("destination"))
                    .unwrap()
                    .count(),
                1
            );
            assert_eq!(
                std::fs::read(target).unwrap(),
                if case == "wrong-parent" {
                    b"approved".as_slice()
                } else {
                    b"unrelated current occupant".as_slice()
                }
            );
        }
    }
    #[test]
    fn windows_native_destination_admission_respects_existing_target_case_aliases() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let directory = root.path().join("destination");
        let target = directory.join("Mixed.txt");
        std::fs::write(&target, b"original must survive").unwrap();
        // Prove the fixture's actual filesystem policy before asserting native
        // admission: ordinary Win32 lookup resolves this different spelling.
        assert_eq!(
            std::fs::read(directory.join("mIXED.TXT")).unwrap(),
            b"original must survive"
        );
        let parent = owner(&directory);
        assert!(
            DestinationAdmission::creation(&parent, OsStr::new("mIXED.TXT")).is_err(),
            "Creation must not admit an occupied native case alias"
        );
        let overwrite =
            DestinationAdmission::overwrite(&parent, OsStr::new("mIXED.TXT"), 100, |_| Ok(()))
                .unwrap();
        overwrite.validate(&parent).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"original must survive");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        assert_eq!(
            std::fs::read_dir(root.path().join("central"))
                .unwrap()
                .count(),
            0
        );
    }

    fn set_case_policy(parent: &File, enabled: bool) {
        use std::{mem::size_of, os::windows::io::AsRawHandle};
        use windows_sys::{
            Wdk::Storage::FileSystem::{
                FILE_CASE_SENSITIVE_INFORMATION, FileCaseSensitiveInformation,
                NtQueryInformationFile, NtSetInformationFile,
            },
            Win32::System::{IO::IO_STATUS_BLOCK, SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR},
        };
        let expected = if enabled {
            FILE_CS_FLAG_CASE_SENSITIVE_DIR
        } else {
            0
        };
        let value = FILE_CASE_SENSITIVE_INFORMATION { Flags: expected };
        let mut status = IO_STATUS_BLOCK::default();
        let result = unsafe {
            NtSetInformationFile(
                parent.as_raw_handle().cast(),
                &mut status,
                ptr::from_ref(&value).cast(),
                size_of::<FILE_CASE_SENSITIVE_INFORMATION>() as u32,
                FileCaseSensitiveInformation,
            )
        };
        assert!(result >= 0, "set native directory case policy: {result:#x}");
        let mut observed = FILE_CASE_SENSITIVE_INFORMATION::default();
        let result = unsafe {
            NtQueryInformationFile(
                parent.as_raw_handle().cast(),
                &mut status,
                ptr::from_mut(&mut observed).cast(),
                size_of::<FILE_CASE_SENSITIVE_INFORMATION>() as u32,
                FileCaseSensitiveInformation,
            )
        };
        assert!(
            result >= 0,
            "query native directory case policy: {result:#x}"
        );
        assert_eq!(observed.Flags, expected);
    }

    #[test]
    fn windows_native_destination_admission_respects_case_sensitive_directory_policy() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let directory = root.path().join("destination");
        let parent = owner(&directory);
        for initial in [false, true] {
            set_case_policy(&parent, initial);
            let empty = DestinationAdmission::creation(&parent, OsStr::new("unused.txt")).unwrap();
            set_case_policy(&parent, !initial);
            assert!(
                empty
                    .validate(&parent)
                    .unwrap_err()
                    .to_string()
                    .contains("case policy"),
                "A policy change must invalidate admission even while the target stays absent"
            );
            assert!(!directory.join("unused.txt").try_exists().unwrap());
        }
        set_case_policy(&parent, true);
        std::fs::write(directory.join("Report.txt"), b"upper original").unwrap();
        let unchanged =
            DestinationAdmission::overwrite(&parent, OsStr::new("Report.txt"), 100, |_| Ok(()))
                .unwrap();
        // The differently cased name really is absent, not an alias. Admission
        // must preserve that native distinction without creating a placeholder.
        assert!(!directory.join("report.txt").try_exists().unwrap());
        let creation = DestinationAdmission::creation(&parent, OsStr::new("report.txt")).unwrap();
        creation.validate(&parent).unwrap();
        assert!(
            DestinationAdmission::overwrite(&parent, OsStr::new("report.txt"), 100, |_| Ok(()))
                .is_err()
        );
        std::fs::write(directory.join("report.txt"), b"lower original").unwrap();
        assert!(creation.validate(&parent).is_err());
        for name in ["Report.txt", "report.txt"] {
            assert!(DestinationAdmission::creation(&parent, OsStr::new(name)).is_err());
            DestinationAdmission::overwrite(&parent, OsStr::new(name), 100, |_| Ok(()))
                .unwrap()
                .validate(&parent)
                .unwrap();
        }
        assert_eq!(
            std::fs::read(directory.join("Report.txt")).unwrap(),
            b"upper original"
        );
        assert_eq!(
            std::fs::read(directory.join("report.txt")).unwrap(),
            b"lower original"
        );
        std::fs::remove_file(directory.join("report.txt")).unwrap();
        let creation = DestinationAdmission::creation(&parent, OsStr::new("report.txt")).unwrap();
        // Changing the live policy makes the previously absent spelling resolve
        // to the existing upper-case file; old creation authority must fail.
        set_case_policy(&parent, false);
        assert_eq!(
            std::fs::read(directory.join("report.txt")).unwrap(),
            b"upper original"
        );
        assert!(creation.validate(&parent).is_err());
        assert!(
            unchanged
                .validate(&parent)
                .unwrap_err()
                .to_string()
                .contains("case policy"),
            "The same original bytes do not authorize a changed native naming policy"
        );
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        assert_eq!(
            std::fs::read_dir(root.path().join("central"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn windows_native_destination_admission_case_policy_is_bound_through_publication_and_undo() {
        // Windows allows disabling this flag for these unambiguous names, but
        // refuses enabling it on a populated directory. Test both actual phases
        // independently rather than relocating retained data to reset a fixture.
        for published in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let directory = root.path().join("destination");
            let parent = owner(&directory);
            let central = owner(&root.path().join("central"));
            set_case_policy(&parent, true);
            let staged = stage(
                &source(&root.path().join("source/nested/file")),
                &parent,
                &central,
            )
            .unwrap();
            assert!(staged.journal.header.parent_case_sensitive);
            let name = staged.stage.name.clone();
            let candidate = staged.stage.candidate_name.clone();
            let local = directory.join(&name).join(LOCAL);
            let mirror = root
                .path()
                .join("central")
                .join(staged.journal.header.central_name());
            if published {
                drop(staged);
                let mut publication =
                    PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
                publication.apply(|_| Ok(())).unwrap();
            } else {
                let mut staged = staged;
                let before = std::fs::read(&local).unwrap();
                set_case_policy(&parent, false);
                assert!(staged.prepare_destination_security(|| Ok(())).is_err());
                assert_eq!(std::fs::read(&local).unwrap(), before);
            }
            let before = std::fs::read(&local).unwrap();
            assert_eq!(before, std::fs::read(&mirror).unwrap());
            if published {
                set_case_policy(&parent, false);
            }
            assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
            assert!(RetainedStage::open(&parent, &central, &name, &mut |_| Ok(())).is_err());
            assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
            assert_eq!(std::fs::read(&local).unwrap(), before);
            assert_eq!(std::fs::read(&mirror).unwrap(), before);
            let data = if published {
                directory.join("copy")
            } else {
                directory.join(candidate)
            };
            assert_eq!(std::fs::read(data).unwrap(), b"original data");
            assert_eq!(directory.join("copy").try_exists().unwrap(), published);
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
        }
    }

    #[test]
    fn windows_native_copy_directory_case_policy_preserves_mixed_trees_and_rejects_drift() {
        fn fixture(source_sensitive: bool, parent_sensitive: bool) -> tempfile::TempDir {
            let root = tempfile::tempdir().unwrap();
            for name in ["source", "destination"] {
                std::fs::create_dir(root.path().join(name)).unwrap();
            }
            set_case_policy(&owner(&root.path().join("source")), source_sensitive);
            set_case_policy(&owner(&root.path().join("destination")), parent_sensitive);
            drop(
                windows_fs::create_private(
                    &owner(root.path()),
                    OsStr::new("central"),
                    true,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                )
                .unwrap(),
            );
            root
        }
        for populated in [false, true] {
            for source_sensitive in [false, true] {
                for parent_sensitive in [false, true] {
                    let root = fixture(source_sensitive, parent_sensitive);
                    let src = root.path().join("source");
                    if populated {
                        std::fs::create_dir(src.join("nested")).unwrap();
                        set_case_policy(&owner(&src.join("nested")), !source_sensitive);
                        for (path, sensitive) in [
                            (&src, source_sensitive),
                            (&src.join("nested"), !source_sensitive),
                        ] {
                            std::fs::write(path.join("Report.txt"), b"upper data").unwrap();
                            if sensitive {
                                std::fs::write(path.join("report.txt"), b"lower data").unwrap();
                            }
                        }
                    }
                    let destination = root.path().join("destination");
                    let parent = owner(&destination);
                    let central = owner(&root.path().join("central"));
                    let staged = stage(&source(&src), &parent, &central).unwrap();
                    let name = staged.stage.name.clone();
                    let candidate = staged.stage.candidate_name.clone();
                    drop(staged);
                    let mut publication =
                        PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
                    publication.apply(|_| Ok(())).unwrap();
                    drop(publication);
                    let copied = destination.join("copy");
                    assert_eq!(
                        windows_fs::directory_case_sensitive(&owner(&copied)).unwrap(),
                        source_sensitive,
                        "populated={populated}, source={source_sensitive}, parent={parent_sensitive}"
                    );
                    assert_eq!(
                        windows_fs::directory_case_sensitive(&parent).unwrap(),
                        parent_sensitive
                    );
                    if populated {
                        assert_eq!(
                            windows_fs::directory_case_sensitive(&owner(&copied.join("nested")))
                                .unwrap(),
                            !source_sensitive
                        );
                        for (original, copy, sensitive) in [
                            (&src, &copied, source_sensitive),
                            (
                                &src.join("nested"),
                                &copied.join("nested"),
                                !source_sensitive,
                            ),
                        ] {
                            for path in [original, copy] {
                                assert_eq!(
                                    std::fs::read(path.join("Report.txt")).unwrap(),
                                    b"upper data"
                                );
                                assert_eq!(
                                    std::fs::read(path.join("report.txt")).unwrap(),
                                    if sensitive {
                                        b"lower data"
                                    } else {
                                        b"upper data"
                                    }
                                );
                            }
                        }
                    }
                    undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
                    assert!(!copied.try_exists().unwrap());
                    assert_eq!(
                        windows_fs::directory_case_sensitive(&owner(&destination.join(candidate)))
                            .unwrap(),
                        source_sensitive
                    );
                    assert_eq!(
                        windows_fs::directory_case_sensitive(&owner(&src)).unwrap(),
                        source_sensitive
                    );
                }
            }
        }
        // A changed retained candidate is refused, never silently repaired.
        let root = fixture(true, false);
        let directory = root.path().join("destination");
        let parent = owner(&directory);
        let central = owner(&root.path().join("central"));
        let staged = stage(&source(&root.path().join("source")), &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = directory.join(&staged.stage.candidate_name);
        let local = directory.join(&name).join(LOCAL);
        let mirror = root
            .path()
            .join("central")
            .join(staged.journal.header.central_name());
        drop(staged);
        assert!(windows_fs::directory_case_sensitive(&owner(&candidate)).unwrap());
        set_case_policy(&owner(&candidate), false);
        let before = std::fs::read(&local).unwrap();
        assert_eq!(before, std::fs::read(&mirror).unwrap());
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(local).unwrap(), before);
        assert_eq!(std::fs::read(mirror).unwrap(), before);
        assert!(!windows_fs::directory_case_sensitive(&owner(&candidate)).unwrap());
        assert!(!directory.join("copy").try_exists().unwrap());
        assert!(windows_fs::directory_case_sensitive(&owner(&root.path().join("source"))).unwrap());
    }

    #[test]
    fn windows_native_destination_admission_never_converts_creation_or_skips_an_approved_original()
    {
        for overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let target = root.path().join("destination/copy");
            let admission = if overwrite {
                std::fs::write(&target, b"approved original").unwrap();
                DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(()))
                    .unwrap()
            } else {
                DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
            };
            let mut staged = DurableStage::create(
                &source(&root.path().join("source")),
                &parent,
                &central,
                &admission,
                OsStr::new("source"),
                100,
                |_| Ok(()),
            )
            .unwrap();
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            let name = staged.stage.name.clone();
            let candidate = staged.stage.candidate_name.clone();
            drop(staged);
            if overwrite {
                std::fs::rename(&target, root.path().join("destination/moved-original")).unwrap();
            } else {
                std::fs::write(&target, b"unrelated occupant").unwrap();
            }
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&local).unwrap();
            if overwrite {
                assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
                assert_eq!(
                    std::fs::read(root.path().join("destination/moved-original")).unwrap(),
                    b"approved original"
                );
                assert!(!target.exists());
            } else {
                let mut journal = original_journal(root.path(), &name);
                assert!(
                    journal
                        .prepare_original(100, |_| Ok(()))
                        .unwrap_err()
                        .to_string()
                        .contains("cannot become an overwrite")
                );
                assert!(
                    journal
                        .state
                        .check(&Record::OriginalPlanBegin {
                            count: 1,
                            digest: [0; 32]
                        })
                        .is_err()
                );
                assert_eq!(std::fs::read(&target).unwrap(), b"unrelated occupant");
            }
            assert_eq!(std::fs::read(local).unwrap(), before);
            assert_eq!(
                std::fs::read(
                    root.path()
                        .join("destination")
                        .join(candidate)
                        .join("nested/file")
                )
                .unwrap(),
                b"original data"
            );
        }
    }

    #[test]
    fn windows_native_copy_session_receipt_executes_admitted_creation_and_overwrite_then_public_undo()
     {
        use crate::PlatformOps;
        for original in [None, Some(false), Some(true)] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let target = root.path().join("destination/copy");
            if let Some(directory) = original {
                if directory {
                    std::fs::create_dir(&target).unwrap();
                    std::fs::write(target.join("old"), b"old data").unwrap();
                } else {
                    std::fs::write(&target, b"old data").unwrap();
                }
            }
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let admission = if original.is_some() {
                DestinationAdmission::overwrite(&parent, OsStr::new("copy"), 100, |_| Ok(()))
                    .unwrap()
            } else {
                DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap()
            };
            let src = source(&root.path().join("source"));
            let receipt = DurableStage::execute_with_receipt(
                &src,
                &parent,
                &central,
                &admission,
                OsStr::new("source"),
                100,
                |_| Ok(()),
            )
            .unwrap();
            drop(src);
            assert_eq!(
                receipt.path_pairs().len(),
                if original.is_some() { 2 } else { 1 }
            );
            assert!(receipt.copy_transaction_id().is_some());
            assert_eq!(
                std::fs::read(receipt.after_path().join("nested/file")).unwrap(),
                b"original data"
            );
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            remove_undo_fixture_source(root.path());
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt)
                .unwrap();
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt.clone())
                .unwrap();
            assert_eq!(
                std::fs::read(receipt.before_path().join("nested/file")).unwrap(),
                b"original data"
            );
            match original {
                None => assert!(!target.exists()),
                Some(directory) => assert_eq!(
                    std::fs::read(if directory {
                        target.join("old")
                    } else {
                        target
                    })
                    .unwrap(),
                    b"old data"
                ),
            }
        }
    }
    #[test]
    fn windows_native_copy_session_receipt_rejects_missing_authority_and_changed_publications() {
        use crate::PlatformOps;
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission = DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap();
        let receipt = DurableStage::execute_with_receipt(
            &source(&root.path().join("source")),
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |_| Ok(()),
        )
        .unwrap();
        let mut malformed = receipt.clone();
        malformed.windows_copy = None;
        assert!(matches!(
            crate::DefaultPlatform.undo_copy_with_receipt(&malformed),
            Err(PlatformError::InvalidPathMoveUndoReceipt(_))
        ));
        let published = receipt.after_path().join("nested/file");
        std::fs::write(&published, b"external data").unwrap();
        assert!(
            crate::DefaultPlatform
                .undo_copy_with_receipt(&receipt)
                .is_err()
        );
        assert_eq!(std::fs::read(published).unwrap(), b"external data");
        assert!(!receipt.before_path().exists());
    }
    #[test]
    fn windows_native_copy_session_receipt_failure_preserves_committed_classification_and_data() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let admission = DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap();
        let mut changed = false;
        let result = DurableStage::execute_with_receipt(
            &source(&root.path().join("source")),
            &parent,
            &central,
            &admission,
            OsStr::new("source"),
            100,
            |_| {
                if !changed {
                    let names = std::fs::read_dir(root.path().join("destination"))
                        .unwrap()
                        .filter_map(Result::ok)
                        .filter(|entry| entry.path().join(LOCAL).is_file())
                        .map(|entry| entry.file_name())
                        .collect::<Vec<_>>();
                    if let Some(name) = names.first()
                        && inspect(&parent, &central, name)
                            .is_ok_and(|state| state.completion_verified)
                    {
                        std::fs::write(
                            root.path().join("destination/copy/nested/file"),
                            b"external after completion",
                        )
                        .unwrap();
                        changed = true;
                    }
                }
                Ok(())
            },
        );
        assert!(
            changed,
            "The receipt failure must happen after verified publication"
        );
        assert!(
            matches!(result, Err(PlatformError::CommittedCleanupPending { .. })),
            "{result:?}"
        );
        assert_eq!(
            std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
            b"external after completion"
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
    }

    fn original_journal(root: &Path, name: &OsStr) -> Journal {
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        let snapshot = Snapshot::load(&parent, &central, name, SnapshotAccess::Write).unwrap();
        Journal::from_snapshot(&parent, &central, snapshot).unwrap()
    }

    fn original_move_fixture(root: &Path, directory: bool, restore: bool) -> OsString {
        let name = original_fixture(root, directory, false);
        remove_undo_fixture_source(root);
        let mut journal = original_journal(root, &name);
        journal.prepare_original(100, |_| Ok(())).unwrap();
        if restore {
            journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .unwrap();
        }
        name
    }
    fn overwrite_publish(root: &Path, name: &OsStr) -> Result<(), PlatformError> {
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        PermissionStage::publish(&parent, &central, name, |_| Ok(()))?.apply(|_| Ok(()))
    }
    fn overwrite_undo(root: &Path, name: &OsStr) -> Result<(), PlatformError> {
        undo_completed_copy(
            &owner(&root.join("destination")),
            &owner(&root.join("central")),
            name,
            |_| Ok(()),
        )
    }
    fn overwrite_fixture(root: &Path, directory: bool, undo: bool) -> OsString {
        let name = original_move_fixture(root, directory, true);
        if undo {
            overwrite_publish(root, &name).unwrap();
        }
        name
    }
    fn assert_overwrite_versions(root: &Path, name: &OsStr, directory: bool) {
        let journal = original_journal(root, name);
        let old_slot = if journal.state.original_restored.is_some()
            || (journal.state.original_restore_intent
                && identity_at(
                    &journal.parent_dir,
                    OsStr::new("copy"),
                    journal.state.original_retained.as_ref().unwrap().identity,
                )
                .unwrap())
        {
            root.join("destination/copy")
        } else {
            root.join("destination")
                .join(journal.header.original_name())
        };
        assert_eq!(
            std::fs::read(if directory {
                old_slot.join("old")
            } else {
                old_slot
            })
            .unwrap(),
            b"old destination"
        );
        let candidate = root
            .join("destination")
            .join(journal.header.candidate_name());
        let new_slot = if candidate.exists() {
            candidate
        } else {
            root.join("destination/copy")
        };
        assert_eq!(
            std::fs::read(new_slot.join("nested/file")).unwrap(),
            b"original data"
        );
    }
    #[test]
    fn windows_native_overwrite_resume_and_undo_restore_original_and_replay_without_sources() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let name = overwrite_fixture(root.path(), directory, false);
            let journal = original_journal(root.path(), &name);
            let id = journal.header.transaction.clone();
            drop(journal);
            let mut resume =
                RetainedCopyResumePlan::admit(&root.path().join("central"), &id, |_| Ok(()))
                    .unwrap();
            assert_eq!(resume.path_pairs().len(), 2);
            resume.resume(|_| Ok(())).unwrap();
            assert!(resume.complete());
            assert_overwrite_versions(root.path(), &name, directory);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let receipt = CompletedCopy::open(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(receipt.entry_count(), 3);
            drop(receipt);
            let mut undo =
                RetainedCopyUndoPlan::admit(&root.path().join("central"), &id, |_| Ok(())).unwrap();
            assert_eq!(undo.path_pairs().len(), 2);
            undo.undo_next().unwrap();
            assert_eq!(undo.remaining(), 0);
            let journal = original_journal(root.path(), &name);
            assert!(journal.state.undo_complete());
            let length = journal.length;
            drop(journal);
            overwrite_undo(root.path(), &name).unwrap();
            assert_eq!(original_journal(root.path(), &name).length, length);
            assert_eq!(
                phase_for_test(&parent, &central, &name),
                WindowsCopyPhase::Undone
            );
            assert_overwrite_versions(root.path(), &name, directory);
        }
    }
    fn phase_for_test(parent: &File, central: &File, name: &OsStr) -> WindowsCopyPhase {
        discovery::phase(
            &Snapshot::load(parent, central, name, SnapshotAccess::Read)
                .unwrap()
                .state,
        )
    }
    fn overwrite_faults(undo: bool) -> Vec<(u64, Step)> {
        let mut faults = Vec::new();
        // Publication: intent/ack, three security intent/ack pairs, three final
        // entries and seal. Undo: withdrawal intent/ack and restore intent/ack.
        for sequence in 0..if undo { 4 } else { 12 } {
            faults.push((sequence, Step::LocalFlushed));
            faults.push((sequence, Step::CentralFlushed));
        }
        if undo {
            for step in [
                Step::UndoReadersReleased,
                Step::UndoRenamed,
                Step::UndoFlushed,
            ] {
                faults.push((1, step));
            }
            for step in [
                Step::OriginalReadersReleased,
                Step::OriginalRenamed,
                Step::OriginalFlushed,
            ] {
                faults.push((3, step));
            }
        } else {
            for step in [Step::PublicationRenamed, Step::PublicationFlushed] {
                faults.push((1, step));
            }
            for sequence in [3, 5, 7] {
                for step in [Step::PermissionsChanged, Step::PermissionsFlushed] {
                    faults.push((sequence, step));
                }
            }
        }
        faults
    }
    fn recover_overwrite_fault(root: &Path, directory: bool, undo: bool, step: Step) {
        let name = stage_name(&root.join("destination"));
        let local = root.join("destination").join(&name).join(LOCAL);
        let bytes = std::fs::read(&local).unwrap();
        let header = decode(&bytes).unwrap().state.header.unwrap();
        let mirror = root.join("central").join(header.central_name());
        let central_bytes = std::fs::read(&mirror).unwrap();
        if step == Step::LocalFlushed {
            assert_ne!(bytes, central_bytes);
            assert!(
                if undo {
                    overwrite_undo(root, &name)
                } else {
                    overwrite_publish(root, &name)
                }
                .is_err()
            );
            assert_eq!(std::fs::read(&local).unwrap(), bytes);
            assert_eq!(std::fs::read(&mirror).unwrap(), central_bytes);
            // Inspect each identity-bound payload without trusting a mismatched writer.
            let state = decode(&bytes).unwrap().state;
            let old_id = state.original.as_ref().unwrap().evidence.entries[0]
                .version
                .identity;
            let parent = owner(&root.join("destination"));
            let old = if identity_at(&parent, OsStr::new("copy"), old_id).unwrap() {
                root.join("destination/copy")
            } else {
                root.join("destination").join(header.original_name())
            };
            assert_eq!(
                std::fs::read(if directory { old.join("old") } else { old }).unwrap(),
                b"old destination"
            );
            let candidate = root.join("destination").join(header.candidate_name());
            let new = if candidate.exists() {
                candidate
            } else {
                root.join("destination/copy")
            };
            assert_eq!(
                std::fs::read(new.join("nested/file")).unwrap(),
                b"original data"
            );
        } else {
            if undo {
                overwrite_undo(root, &name).unwrap();
                assert!(original_journal(root, &name).state.undo_complete());
            } else {
                overwrite_publish(root, &name).unwrap();
                assert!(original_journal(root, &name).state.completion_verified);
            }
            assert_overwrite_versions(root, &name, directory);
        }
    }
    #[test]
    fn windows_native_overwrite_all_mutation_prefixes_recover_or_preserve_divergent_mirrors() {
        for directory in [false, true] {
            for undo in [false, true] {
                for (offset, step) in overwrite_faults(undo) {
                    let root = tempfile::tempdir().unwrap();
                    let name = overwrite_fixture(root.path(), directory, undo);
                    let sequence = original_journal(root.path(), &name).sequence;
                    FAULT.with(|f| f.set(Some((sequence + offset, step, false))));
                    let result = if undo {
                        overwrite_undo(root.path(), &name)
                    } else {
                        overwrite_publish(root.path(), &name)
                    };
                    FAULT.with(|f| f.set(None));
                    assert!(result.is_err(), "{directory} {undo} {offset} {step:?}");
                    recover_overwrite_fault(root.path(), directory, undo, step);
                }
            }
        }
    }
    #[test]
    #[ignore = "abrupt-exit child; owned empty root required"]
    fn windows_native_overwrite_exit_child() {
        let root = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_OVERWRITE_CHILD").expect("owned root"),
        );
        assert!(root.is_dir() && std::fs::read_dir(&root).unwrap().next().is_none());
        let directory = std::env::var("OTTRIN_OVERWRITE_DIRECTORY").unwrap() == "1";
        let undo = std::env::var("OTTRIN_OVERWRITE_UNDO").unwrap() == "1";
        let index: usize = std::env::var("OTTRIN_OVERWRITE_FAULT")
            .unwrap()
            .parse()
            .unwrap();
        let name = overwrite_fixture(&root, directory, undo);
        let sequence = original_journal(&root, &name).sequence;
        let (offset, step) = overwrite_faults(undo)[index];
        FAULT.with(|f| f.set(Some((sequence + offset, step, true))));
        let result = if undo {
            overwrite_undo(&root, &name)
        } else {
            overwrite_publish(&root, &name)
        };
        panic!("expected abrupt exit: {result:?}");
    }
    #[test]
    fn windows_native_overwrite_real_process_exits_recover_original_and_replacement() {
        for directory in [false, true] {
            for undo in [false, true] {
                for (index, (_, step)) in overwrite_faults(undo).into_iter().enumerate() {
                    let root = tempfile::tempdir().unwrap();
                    let status=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","copy_transaction::windows::journal::tests::windows_native_overwrite_exit_child","--ignored","--nocapture"])
                .env("OTTRIN_OVERWRITE_CHILD",root.path()).env("OTTRIN_OVERWRITE_DIRECTORY",if directory {"1"}else{"0"}).env("OTTRIN_OVERWRITE_UNDO",if undo {"1"}else{"0"}).env("OTTRIN_OVERWRITE_FAULT",index.to_string()).status().unwrap();
                    assert_eq!(status.code(), Some(86), "{directory} {undo} {index}");
                    recover_overwrite_fault(root.path(), directory, undo, step);
                }
            }
        }
    }
    #[test]
    fn windows_native_overwrite_restore_after_withdrawal_preserves_changed_private_replacement() {
        let root = tempfile::tempdir().unwrap();
        let name = overwrite_fixture(root.path(), true, true);
        let journal = original_journal(root.path(), &name);
        let sequence = journal.sequence;
        let header = journal.header.clone();
        drop(journal);
        FAULT.with(|f| f.set(Some((sequence + 1, Step::CentralFlushed, false))));
        assert!(overwrite_undo(root.path(), &name).is_err());
        FAULT.with(|f| f.set(None));
        assert!(original_journal(root.path(), &name).state.undone.is_some());
        let candidate = root
            .path()
            .join("destination")
            .join(header.candidate_name())
            .join("nested/file");
        std::fs::write(&candidate, b"changed retained replacement").unwrap();
        let statuses = discover_at(&root.path().join("central")).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::UndoPending));
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedOriginal
        );

        let mut undo =
            RetainedCopyUndoPlan::admit(&root.path().join("central"), &header.transaction, |_| {
                Ok(())
            })
            .unwrap();
        assert_eq!(undo.admitted_phase(), WindowsCopyPhase::UndoPending);
        undo.undo_next().unwrap();
        assert_eq!(
            std::fs::read(root.path().join("destination/copy/old")).unwrap(),
            b"old destination"
        );
        assert_eq!(
            std::fs::read(&candidate).unwrap(),
            b"changed retained replacement"
        );
        overwrite_undo(root.path(), &name).unwrap();
        assert_eq!(
            std::fs::read(candidate).unwrap(),
            b"changed retained replacement"
        );
    }

    struct LogBudget(u64);
    impl LogBudget {
        fn new(limit: u64) -> Self {
            Self(LOG_BUDGET.with(|value| value.replace(limit)))
        }
    }
    impl Drop for LogBudget {
        fn drop(&mut self) {
            LOG_BUDGET.with(|value| value.set(self.0));
        }
    }
    #[test]
    fn windows_native_overwrite_reserves_the_inverse_before_retention_and_spends_it_through_undo() {
        for insufficient in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let name = original_move_fixture(root.path(), true, false);
            let mut journal = original_journal(root.path(), &name);
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&local).unwrap();
            let sequence = journal.sequence;
            let inverse = journal.undo_tail_size().unwrap();
            assert!(inverse > 0);
            let publication = journal.permission_tail_size().unwrap() - inverse;
            let mut version = journal.state.original.as_ref().unwrap().evidence.entries[0]
                .version
                .clone();
            version.changed = i64::MIN;
            let mut retention_records = journal.creation_tail(RenameKind::RetainOriginal).unwrap();
            retention_records.extend([
                Record::OriginalRetainIntent,
                OriginalMove::Retain.ack(version),
            ]);
            let retention = retention_records
                .iter()
                .map(|record| FRAME_HEADER as u64 + record_payload(record).unwrap().len() as u64)
                .sum::<u64>();
            let budget =
                journal.length + publication + retention + if insufficient { 0 } else { inverse };
            // Lower the test-only reservation ceiling, keeping the real journal
            // bytes, lengths, owner and decoded history coherent through restart.
            let _limit = LogBudget::new(budget);
            let result = journal.move_original(None, OriginalMove::Retain, |_| Ok(()));
            if insufficient {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains("insufficient Copy journal space for original move"),
                    "{error}"
                );
                assert_eq!(journal.sequence, sequence);
                assert_eq!(std::fs::read(local).unwrap(), before);
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy/old")).unwrap(),
                    b"old destination"
                );
            } else {
                result.unwrap();
                drop(journal);
                overwrite_publish(root.path(), &name).unwrap();
                overwrite_undo(root.path(), &name).unwrap();
                assert!(original_journal(root.path(), &name).length <= budget);
                assert_overwrite_versions(root.path(), &name, true);
            }
        }
    }

    #[test]
    fn windows_native_overwrite_refuses_drift_and_occupants_without_deleting_either_version() {
        for case in [
            "original-before-publish",
            "occupied-publication",
            "original-before-undo",
            "replacement-before-undo",
            "occupied-candidate",
            "occupied-restoration",
        ] {
            let root = tempfile::tempdir().unwrap();
            let undo = case.contains("undo")
                || case == "occupied-candidate"
                || case == "occupied-restoration";
            let name = overwrite_fixture(root.path(), true, undo);
            let journal = original_journal(root.path(), &name);
            let original = root
                .path()
                .join("destination")
                .join(journal.header.original_name());
            let candidate = root
                .path()
                .join("destination")
                .join(journal.header.candidate_name());
            drop(journal);
            match case {
                "original-before-publish" | "original-before-undo" => {
                    std::fs::write(original.join("old"), b"external original").unwrap()
                }
                "occupied-publication" => {
                    std::fs::write(root.path().join("destination/copy"), b"unrelated occupant")
                        .unwrap()
                }
                "replacement-before-undo" => std::fs::write(
                    root.path().join("destination/copy/nested/file"),
                    b"external replacement",
                )
                .unwrap(),
                "occupied-candidate" => std::fs::write(&candidate, b"unrelated candidate").unwrap(),
                "occupied-restoration" => {
                    let target = root.path().join("destination/copy");
                    ORIGINAL_HANDOFF.with(|h| {
                        *h.borrow_mut() = Some(Box::new(move || {
                            std::fs::write(target, b"unrelated restoration occupant").unwrap()
                        }))
                    });
                }
                _ => unreachable!(),
            }
            let result = if undo {
                overwrite_undo(root.path(), &name)
            } else {
                overwrite_publish(root.path(), &name)
            };
            ORIGINAL_HANDOFF.with(|h| h.borrow_mut().take());
            assert!(result.is_err(), "{case}");
            assert!(original.exists());
            if case.starts_with("original-before") {
                assert_eq!(
                    std::fs::read(original.join("old")).unwrap(),
                    b"external original"
                );
            }
            if case == "occupied-restoration" {
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy")).unwrap(),
                    b"unrelated restoration occupant"
                );
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                assert_eq!(
                    phase_for_test(&parent, &central, &name),
                    WindowsCopyPhase::UndoPending
                );
                std::fs::remove_file(root.path().join("destination/copy")).unwrap();
                overwrite_undo(root.path(), &name).unwrap();
                assert_overwrite_versions(root.path(), &name, true);
            }
        }
    }

    fn pending_retention_fixture(root: &Path, directory: bool) -> OsString {
        let name = original_move_fixture(root, directory, false);
        let mut journal = original_journal(root, &name);
        FAULT.with(|fault| fault.set(Some((journal.sequence, Step::CentralFlushed, false))));
        assert!(
            journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .is_err()
        );
        FAULT.with(|fault| fault.set(None));
        name
    }
    #[test]
    fn windows_native_retention_resume_recovers_injected_and_real_exit_prefixes_without_source() {
        for real_exit in [false, true] {
            for directory in [false, true] {
                for (index, (offset, step)) in move_faults().into_iter().enumerate() {
                    let root = tempfile::tempdir().unwrap();
                    let name = if real_exit {
                        let status = std::process::Command::new(std::env::current_exe().unwrap())
                            .args(["--exact", "copy_transaction::windows::journal::tests::windows_native_original_move_exit_child", "--ignored", "--nocapture"])
                            .env("OTTRIN_ORIGINAL_MOVE_CHILD", root.path())
                            .env("OTTRIN_ORIGINAL_MOVE_DIRECTORY", if directory {"1"} else {"0"})
                            .env("OTTRIN_ORIGINAL_MOVE_RESTORE", "0")
                            .env("OTTRIN_ORIGINAL_MOVE_FAULT", index.to_string()).status().unwrap();
                        assert_eq!(status.code(), Some(86));
                        stage_name(&root.path().join("destination"))
                    } else {
                        let name = original_move_fixture(root.path(), directory, false);
                        let mut journal = original_journal(root.path(), &name);
                        FAULT.with(|fault| {
                            fault.set(Some((journal.sequence + offset, step, false)))
                        });
                        assert!(
                            journal
                                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                                .is_err()
                        );
                        FAULT.with(|fault| fault.set(None));
                        name
                    };
                    let local = root.path().join("destination").join(&name).join(LOCAL);
                    let bytes = std::fs::read(&local).unwrap();
                    let decoded = decode(&bytes).unwrap();
                    let header = decoded.state.header.as_ref().unwrap();
                    let mirror = root.path().join("central").join(header.central_name());
                    let mirror_bytes = std::fs::read(&mirror).unwrap();
                    let admission = RetainedCopyResumePlan::admit(
                        &root.path().join("central"),
                        &header.transaction,
                        |_| Ok(()),
                    );
                    let public = root.path().join("destination/copy");
                    let original = if public.exists() {
                        public
                    } else {
                        root.path().join("destination").join(header.original_name())
                    };
                    assert_eq!(
                        std::fs::read(if directory {
                            original.join("old")
                        } else {
                            original
                        })
                        .unwrap(),
                        b"old destination"
                    );
                    assert_eq!(
                        std::fs::read(
                            root.path()
                                .join("destination")
                                .join(header.candidate_name())
                                .join("nested/file")
                        )
                        .unwrap(),
                        b"original data"
                    );
                    if step == Step::LocalFlushed {
                        assert!(admission.is_err());
                        assert_ne!(bytes, mirror_bytes);
                        assert_eq!(std::fs::read(local).unwrap(), bytes);
                        assert_eq!(std::fs::read(mirror).unwrap(), mirror_bytes);
                    } else {
                        let mut resume = admission.unwrap();
                        let expected_phase = if decoded.state.original_retained.is_none() {
                            WindowsCopyPhase::RetainingOriginal
                        } else {
                            WindowsCopyPhase::ReadyToPublish
                        };
                        assert_eq!(resume.admitted_phase(), expected_phase);
                        let status = discover_at(&root.path().join("central")).unwrap();
                        assert_eq!(status[0].phase, Some(expected_phase));
                        assert_eq!(
                            status[0].evidence,
                            WindowsCopyRecoveryEvidence::VerifiedTree
                        );
                        assert_eq!(resume.path_pairs().len(), 2);
                        // Read-only admission cannot finish retention or append records.
                        assert_eq!(std::fs::read(&local).unwrap(), bytes);
                        assert_eq!(std::fs::read(&mirror).unwrap(), mirror_bytes);
                        resume.resume(|_| Ok(())).unwrap();
                        assert!(resume.complete());
                        assert_overwrite_versions(root.path(), &name, directory);
                        overwrite_undo(root.path(), &name).unwrap();
                        assert_overwrite_versions(root.path(), &name, directory);
                    }
                }
            }
        }
    }
    #[test]
    fn windows_native_retention_resume_refuses_observation_only_drift_and_restoration() {
        for case in [
            "observation-only",
            "original-drift",
            "candidate-drift",
            "moved-original",
            "retained-time-drift",
            "restored",
        ] {
            let root = tempfile::tempdir().unwrap();
            let name = if case == "observation-only" {
                original_move_fixture(root.path(), true, false)
            } else {
                pending_retention_fixture(root.path(), true)
            };
            if case == "retained-time-drift" {
                let mut journal = original_journal(root.path(), &name);
                FAULT.with(|fault| {
                    fault.set(Some((journal.sequence, Step::OriginalRenamed, false)))
                });
                assert!(
                    journal
                        .move_original(None, OriginalMove::Retain, |_| Ok(()))
                        .is_err()
                );
                FAULT.with(|fault| fault.set(None));
            }
            let journal = original_journal(root.path(), &name);
            let header = journal.header.clone();
            drop(journal);
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&local).unwrap();
            let admission = RetainedCopyResumePlan::admit(
                &root.path().join("central"),
                &header.transaction,
                |_| Ok(()),
            );
            if case == "observation-only" {
                assert!(
                    admission
                        .unwrap_err()
                        .to_string()
                        .contains("no durable intent")
                );
                assert_eq!(std::fs::read(local).unwrap(), before);
                continue;
            }
            let mut resume = admission.unwrap();
            let target = root.path().join("destination/copy");
            let candidate = root
                .path()
                .join("destination")
                .join(header.candidate_name());
            match case {
                "original-drift" => {
                    std::fs::write(target.join("old"), b"external original").unwrap()
                }
                "candidate-drift" => {
                    std::fs::write(candidate.join("nested/file"), b"external copy").unwrap()
                }
                "moved-original" => {
                    std::fs::rename(&target, root.path().join("destination/elsewhere")).unwrap()
                }
                "retained-time-drift" => {
                    let original = root.path().join("destination").join(header.original_name());
                    let elsewhere = root.path().join("destination/elsewhere");
                    let observed = super::super::version(&source(&original)).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::fs::rename(&original, &elsewhere).unwrap();
                    std::fs::rename(&elsewhere, &original).unwrap();
                    let actual = super::super::version(&source(&original)).unwrap();
                    let mut expected = observed.clone();
                    expected.changed = actual.changed;
                    assert_eq!(
                        actual, expected,
                        "Fixture changes only the retained root change time"
                    );
                    assert_ne!(actual.changed, observed.changed);
                }
                "restored" => {
                    let mut journal = original_journal(root.path(), &name);
                    journal
                        .move_original(None, OriginalMove::Retain, |_| Ok(()))
                        .unwrap();
                    journal
                        .move_original(None, OriginalMove::Restore, |_| Ok(()))
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let before = std::fs::read(&local).unwrap();
            assert!(resume.resume(|_| Ok(())).is_err());
            assert!(!resume.complete());
            assert_eq!(std::fs::read(local).unwrap(), before);
            assert_eq!(
                std::fs::read(candidate.join("nested/file")).unwrap(),
                if case == "candidate-drift" {
                    b"external copy".as_slice()
                } else {
                    b"original data".as_slice()
                }
            );
            let old = if case == "moved-original" {
                root.path().join("destination/elsewhere/old")
            } else if case == "retained-time-drift" {
                root.path()
                    .join("destination")
                    .join(header.original_name())
                    .join("old")
            } else {
                target.join("old")
            };
            assert_eq!(
                std::fs::read(old).unwrap(),
                if case == "original-drift" {
                    b"external original".as_slice()
                } else {
                    b"old destination".as_slice()
                }
            );
        }
    }
    #[test]
    fn windows_native_retention_resume_cancellation_retries_the_same_admission() {
        let root = tempfile::tempdir().unwrap();
        let name = pending_retention_fixture(root.path(), true);
        let journal = original_journal(root.path(), &name);
        let id = journal.header.transaction.clone();
        drop(journal);
        let mut plan =
            RetainedCopyResumePlan::admit(&root.path().join("central"), &id, |_| Ok(())).unwrap();
        let cancel = std::rc::Rc::new(std::cell::Cell::new(false));
        let handoff = cancel.clone();
        ORIGINAL_HANDOFF
            .with(|hook| *hook.borrow_mut() = Some(Box::new(move || handoff.set(true))));
        assert!(
            plan.resume(|_| if cancel.get() {
                Err(io("cancel retention resume"))
            } else {
                Ok(())
            })
            .is_err()
        );
        ORIGINAL_HANDOFF.with(|hook| hook.borrow_mut().take());
        assert!(
            cancel.get(),
            "The cancellation fixture must reach original handoff"
        );
        assert!(!plan.complete());
        plan.resume(|_| Ok(())).unwrap();
        assert!(plan.complete());
        assert_overwrite_versions(root.path(), &name, true);
        overwrite_undo(root.path(), &name).unwrap();
        assert_overwrite_versions(root.path(), &name, true);
    }

    fn move_faults() -> [(u64, Step); 7] {
        [
            (0, Step::LocalFlushed),
            (0, Step::CentralFlushed),
            (1, Step::OriginalReadersReleased),
            (1, Step::OriginalRenamed),
            (1, Step::OriginalFlushed),
            (1, Step::LocalFlushed),
            (1, Step::CentralFlushed),
        ]
    }

    #[test]
    #[ignore = "abrupt-exit child; owned temporary root supplied by parent"]
    fn windows_native_original_move_exit_child() {
        let root = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_ORIGINAL_MOVE_CHILD").expect("owned root required"),
        );
        assert!(root.is_dir() && std::fs::read_dir(&root).unwrap().next().is_none());
        let directory = std::env::var("OTTRIN_ORIGINAL_MOVE_DIRECTORY").unwrap() == "1";
        let restore = std::env::var("OTTRIN_ORIGINAL_MOVE_RESTORE").unwrap() == "1";
        let index: usize = std::env::var("OTTRIN_ORIGINAL_MOVE_FAULT")
            .unwrap()
            .parse()
            .unwrap();
        let name = original_move_fixture(&root, directory, restore);
        let mut journal = original_journal(&root, &name);
        let (offset, step) = move_faults()[index];
        FAULT.with(|f| f.set(Some((journal.sequence + offset, step, true))));
        let result = journal.move_original(
            None,
            if restore {
                OriginalMove::Restore
            } else {
                OriginalMove::Retain
            },
            |_| Ok(()),
        );
        panic!("Expected abrupt exit: {result:?}");
    }

    #[test]
    fn windows_native_original_move_real_exits_recover_without_source_and_preserve_mismatches() {
        for directory in [false, true] {
            for restore in [false, true] {
                for (index, (_, step)) in move_faults().into_iter().enumerate() {
                    let root = tempfile::tempdir().unwrap();
                    let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "copy_transaction::windows::journal::tests::windows_native_original_move_exit_child", "--ignored", "--nocapture"])
                .env("OTTRIN_ORIGINAL_MOVE_CHILD", root.path())
                .env("OTTRIN_ORIGINAL_MOVE_DIRECTORY", if directory { "1" } else { "0" })
                .env("OTTRIN_ORIGINAL_MOVE_RESTORE", if restore { "1" } else { "0" })
                .env("OTTRIN_ORIGINAL_MOVE_FAULT", index.to_string()).status().unwrap();
                    assert_eq!(status.code(), Some(86), "{directory} {restore} {index}");
                    assert!(!root.path().join("source").exists());
                    let name = stage_name(&root.path().join("destination"));
                    let local = root.path().join("destination").join(&name).join(LOCAL);
                    let bytes = std::fs::read(&local).unwrap();
                    let header = decode(&bytes).unwrap().state.header.unwrap();
                    let central = root.path().join("central").join(header.central_name());
                    if step == Step::LocalFlushed {
                        let mirror = std::fs::read(&central).unwrap();
                        assert_ne!(bytes, mirror);
                        assert!(
                            Snapshot::load(
                                &owner(&root.path().join("destination")),
                                &owner(&root.path().join("central")),
                                &name,
                                SnapshotAccess::Write
                            )
                            .is_err()
                        );
                        assert_eq!(std::fs::read(&local).unwrap(), bytes);
                        assert_eq!(std::fs::read(&central).unwrap(), mirror);
                    } else {
                        let mut journal = original_journal(root.path(), &name);
                        journal
                            .move_original(
                                None,
                                if restore {
                                    OriginalMove::Restore
                                } else {
                                    OriginalMove::Retain
                                },
                                |_| Ok(()),
                            )
                            .unwrap();
                        drop(journal);
                        assert_original_location(root.path(), &name, directory, restore);
                    }
                    let public = root.path().join("destination/copy");
                    let original = if public.exists() {
                        public
                    } else {
                        root.path().join("destination").join(header.original_name())
                    };
                    assert_eq!(
                        std::fs::read(if directory {
                            original.join("old")
                        } else {
                            original
                        })
                        .unwrap(),
                        b"old destination"
                    );
                    assert_eq!(
                        std::fs::read(
                            root.path()
                                .join("destination")
                                .join(header.candidate_name())
                                .join("nested/file")
                        )
                        .unwrap(),
                        b"original data"
                    );
                }
            }
        }
    }

    #[test]
    fn windows_native_original_move_restore_preserves_changed_private_copy() {
        for pending in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let name = original_move_fixture(root.path(), true, !pending);
            let mut journal = original_journal(root.path(), &name);
            if pending {
                FAULT.with(|f| f.set(Some((journal.sequence + 1, Step::OriginalRenamed, false))));
                assert!(
                    journal
                        .move_original(None, OriginalMove::Retain, |_| Ok(()))
                        .is_err()
                );
                FAULT.with(|f| f.set(None));
                assert!(journal.state.original_retained.is_none());
            }
            let candidate = root
                .path()
                .join("destination")
                .join(journal.header.candidate_name())
                .join("nested/file");
            std::fs::write(&candidate, b"external private copy").unwrap();
            drop(journal);
            let mut journal = original_journal(root.path(), &name);
            if pending {
                journal
                    .move_original(None, OriginalMove::Retain, |_| Ok(()))
                    .unwrap();
            }
            journal
                .move_original(None, OriginalMove::Restore, |_| Ok(()))
                .unwrap();
            assert_eq!(
                std::fs::read(root.path().join("destination/copy/old")).unwrap(),
                b"old destination"
            );
            assert_eq!(std::fs::read(candidate).unwrap(), b"external private copy");
        }
    }

    #[test]
    fn windows_native_original_move_retains_and_restores_without_source_or_new_publication() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let name = original_move_fixture(root.path(), directory, false);
            let mut journal = original_journal(root.path(), &name);
            journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .unwrap();
            assert!(journal.state.original_retained.is_some());
            assert!(!root.path().join("destination/copy").exists());
            let length = journal.length;
            journal
                .move_original(None, OriginalMove::Retain, |_| Ok(()))
                .unwrap();
            assert_eq!(journal.length, length);
            drop(journal);
            assert_original_location(root.path(), &name, directory, false);
            let mut journal = original_journal(root.path(), &name);
            journal
                .move_original(None, OriginalMove::Restore, |_| Ok(()))
                .unwrap();
            assert!(journal.state.original_restored.is_some());
            let length = journal.length;
            journal
                .move_original(None, OriginalMove::Restore, |_| Ok(()))
                .unwrap();
            assert_eq!(journal.length, length);
            assert!(
                journal
                    .move_original(None, OriginalMove::Retain, |_| Ok(()))
                    .is_err()
            );
            drop(journal);
            assert_original_location(root.path(), &name, directory, true);
        }
    }

    fn assert_original_location(root: &Path, name: &OsStr, directory: bool, restored: bool) {
        let journal = original_journal(root, name);
        let base = root.join("destination").join(if restored {
            OsString::from("copy")
        } else {
            journal.header.original_name()
        });
        assert_eq!(
            std::fs::read(if directory { base.join("old") } else { base }).unwrap(),
            b"old destination"
        );
        assert_eq!(
            std::fs::read(
                root.join("destination")
                    .join(journal.header.candidate_name())
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
    }

    #[test]
    fn windows_native_original_move_fault_prefixes_keep_both_versions_and_never_repair_mirrors() {
        for directory in [false, true] {
            for restore in [false, true] {
                for (offset, step) in move_faults() {
                    let root = tempfile::tempdir().unwrap();
                    let name = original_move_fixture(root.path(), directory, restore);
                    let mut journal = original_journal(root.path(), &name);
                    let local = root.path().join("destination").join(&name).join(LOCAL);
                    let central = root
                        .path()
                        .join("central")
                        .join(journal.header.central_name());
                    let sequence = journal.sequence + offset;
                    FAULT.with(|f| f.set(Some((sequence, step, false))));
                    let result = journal.move_original(
                        None,
                        if restore {
                            OriginalMove::Restore
                        } else {
                            OriginalMove::Retain
                        },
                        |_| Ok(()),
                    );
                    FAULT.with(|f| f.set(None));
                    assert!(result.is_err(), "{directory} {restore} {offset} {step:?}");
                    drop(journal);
                    let before = std::fs::read(&local).unwrap();
                    let mirror = std::fs::read(&central).unwrap();
                    if step == Step::LocalFlushed {
                        assert_ne!(before, mirror);
                        assert!(
                            Snapshot::load(
                                &owner(&root.path().join("destination")),
                                &owner(&root.path().join("central")),
                                &name,
                                SnapshotAccess::Write
                            )
                            .is_err()
                        );
                        assert_eq!(std::fs::read(&local).unwrap(), before);
                        assert_eq!(std::fs::read(&central).unwrap(), mirror);
                    } else {
                        let mut journal = original_journal(root.path(), &name);
                        journal
                            .move_original(
                                None,
                                if restore {
                                    OriginalMove::Restore
                                } else {
                                    OriginalMove::Retain
                                },
                                |_| Ok(()),
                            )
                            .unwrap();
                        drop(journal);
                        assert_original_location(root.path(), &name, directory, restore);
                    }
                }
            }
        }
    }

    #[test]
    fn windows_native_original_move_refuses_occupants_handoff_changes_and_cancellation() {
        for case in [
            "retain-occupied",
            "restore-occupied",
            "handoff",
            "cancel",
            "readonly",
            "budget",
            "candidate",
        ] {
            let root = tempfile::tempdir().unwrap();
            let restore = case == "restore-occupied";
            let name = if case == "readonly" {
                let name = original_fixture(root.path(), false, true);
                let mut journal = original_journal(root.path(), &name);
                journal.prepare_original(100, |_| Ok(())).unwrap();
                drop(journal);
                remove_undo_fixture_source(root.path());
                name
            } else {
                original_move_fixture(root.path(), false, restore)
            };
            let mut journal = original_journal(root.path(), &name);
            let retention = root
                .path()
                .join("destination")
                .join(journal.header.original_name());
            let target = root.path().join("destination/copy");
            if case == "retain-occupied" {
                std::fs::write(&retention, b"unrelated occupant").unwrap();
            }
            if restore {
                std::fs::write(&target, b"unrelated occupant").unwrap();
            }
            if case == "handoff" {
                let target = target.clone();
                ORIGINAL_HANDOFF.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        std::fs::write(target, b"external data").unwrap()
                    }))
                });
            }
            let _limit = (case == "budget").then(|| LogBudget::new(journal.length));
            if case == "candidate" {
                std::fs::write(
                    root.path()
                        .join("destination")
                        .join(journal.header.candidate_name())
                        .join("nested/file"),
                    b"external candidate data",
                )
                .unwrap();
            }
            let result = journal.move_original(
                None,
                if restore {
                    OriginalMove::Restore
                } else {
                    OriginalMove::Retain
                },
                |_| {
                    if case == "cancel" {
                        Err(io("stop original move"))
                    } else {
                        Ok(())
                    }
                },
            );
            ORIGINAL_HANDOFF.with(|hook| *hook.borrow_mut() = None);
            assert!(result.is_err(), "{case}");
            if case == "budget" {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("insufficient Copy journal space for original move")
                );
            }
            if case == "readonly" {
                let mut permissions = std::fs::metadata(&target).unwrap().permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                std::fs::set_permissions(&target, permissions).unwrap();
            }
            if case == "handoff" {
                assert_eq!(std::fs::read(target).unwrap(), b"external data");
            } else if case == "retain-occupied" {
                assert_eq!(std::fs::read(retention).unwrap(), b"unrelated occupant");
                assert_eq!(std::fs::read(target).unwrap(), b"old destination");
            } else if restore {
                assert_eq!(std::fs::read(target).unwrap(), b"unrelated occupant");
                assert_eq!(std::fs::read(retention).unwrap(), b"old destination");
            } else {
                assert_eq!(std::fs::read(target).unwrap(), b"old destination");
            }
        }
    }

    #[test]
    fn windows_native_original_plan_is_paired_source_free_and_blocks_creation_publication() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let name = original_fixture(root.path(), directory, false);
            remove_undo_fixture_source(root.path());
            let mut journal = original_journal(root.path(), &name);
            journal.prepare_original(100, |_| Ok(())).unwrap();
            assert!(journal.state.original.as_ref().unwrap().sealed);
            assert!(journal.state.check(&Record::PublicationIntent).is_err());
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let central = root
                .path()
                .join("central")
                .join(journal.header.central_name());
            let before = std::fs::read(&local).unwrap();
            assert_eq!(before, std::fs::read(central).unwrap());
            journal.prepare_original(100, |_| Ok(())).unwrap();
            assert_eq!(std::fs::read(&local).unwrap(), before);
            drop(journal);
            assert!(
                PermissionStage::publish(
                    &owner(&root.path().join("destination")),
                    &owner(&root.path().join("central")),
                    &name,
                    |_| Ok(())
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&local).unwrap(), before);
            assert_eq!(
                std::fs::read(root.path().join(if directory {
                    "destination/copy/old"
                } else {
                    "destination/copy"
                }))
                .unwrap(),
                b"old destination"
            );
            let mut old = before.clone();
            old[..8].copy_from_slice(b"OTCPWJ06");
            assert!(decode(&old).is_err(), "No superseded-schema reader");
        }
    }

    #[test]
    fn windows_native_original_plan_resumes_every_paired_prefix_and_refuses_local_only_tail() {
        for (offset, step) in (0..5)
            .map(|offset| (offset, Step::CentralFlushed))
            .chain([(1, Step::LocalFlushed)])
        {
            let root = tempfile::tempdir().unwrap();
            let name = original_fixture(root.path(), true, false);
            remove_undo_fixture_source(root.path());
            let mut journal = original_journal(root.path(), &name);
            let sequence = journal.sequence + offset;
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let central = root
                .path()
                .join("central")
                .join(journal.header.central_name());
            FAULT.with(|f| f.set(Some((sequence, step, false))));
            let result = journal.prepare_original(100, |_| Ok(()));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err(), "{offset} {step:?}");
            drop(journal);
            let before = std::fs::read(&local).unwrap();
            let mirror = std::fs::read(&central).unwrap();
            if step == Step::LocalFlushed {
                assert_ne!(before, mirror);
                let parent = owner(&root.path().join("destination"));
                let central_dir = owner(&root.path().join("central"));
                assert!(
                    Snapshot::load(&parent, &central_dir, &name, SnapshotAccess::Write).is_err()
                );
                assert_eq!(std::fs::read(&local).unwrap(), before);
                assert_eq!(std::fs::read(&central).unwrap(), mirror);
            } else {
                let mut journal = original_journal(root.path(), &name);
                journal.prepare_original(100, |_| Ok(())).unwrap();
                assert!(journal.state.original.as_ref().unwrap().sealed);
                let records = journal_records(&std::fs::read(&local).unwrap());
                assert_eq!(
                    records
                        .iter()
                        .filter(|record| matches!(record, Record::OriginalPlanBegin { .. }))
                        .count(),
                    1
                );
                assert_eq!(
                    records
                        .iter()
                        .filter(|record| matches!(record, Record::OriginalPlanEntry { .. }))
                        .count(),
                    3
                );
                assert_eq!(
                    records
                        .iter()
                        .filter(|record| matches!(record, Record::OriginalPlanComplete { .. }))
                        .count(),
                    1
                );
            }
            assert_eq!(
                std::fs::read(root.path().join("destination/copy/old")).unwrap(),
                b"old destination"
            );
        }
    }

    #[test]
    fn windows_native_original_plan_binds_unwritten_children_and_refuses_source_alias_and_budget() {
        for case in ["unwritten", "source-alias", "budget"] {
            let root = tempfile::tempdir().unwrap();
            let name = original_fixture(root.path(), true, false);
            if case == "source-alias" {
                std::fs::remove_dir_all(root.path().join("destination/copy")).unwrap();
                std::fs::hard_link(
                    root.path().join("source/nested/file"),
                    root.path().join("destination/copy"),
                )
                .unwrap();
            }
            let mut journal = original_journal(root.path(), &name);
            if case == "unwritten" {
                let sequence = journal.sequence + 1;
                FAULT.with(|f| f.set(Some((sequence, Step::CentralFlushed, false))));
                assert!(journal.prepare_original(100, |_| Ok(())).is_err());
                FAULT.with(|f| f.set(None));
                drop(journal);
                std::fs::write(
                    root.path().join("destination/copy/old"),
                    b"external modification",
                )
                .unwrap();
                journal = original_journal(root.path(), &name);
                assert_eq!(
                    journal
                        .state
                        .original
                        .as_ref()
                        .unwrap()
                        .evidence
                        .entries
                        .len(),
                    1
                );
                let current = tree::PinnedTree::open(
                    &source(&root.path().join("destination/copy")),
                    100,
                    |_| Ok(()),
                )
                .unwrap()
                .freeze_existing(|_| Ok(()))
                .unwrap();
                let partial = journal.state.original.as_ref().unwrap();
                assert_eq!(
                    partial.evidence.entries[0], current.entries[0],
                    "Written prefix must stay unchanged in this fixture"
                );
                assert_ne!(
                    partial.digest,
                    super::super::evidence_digest(&current).unwrap()
                );
            }
            let local = root.path().join("destination").join(&name).join(LOCAL);
            let central = root
                .path()
                .join("central")
                .join(journal.header.central_name());
            let before = std::fs::read(&local).unwrap();
            let _limit = (case == "budget").then(|| LogBudget::new(journal.length));
            let result = journal.prepare_original(100, |_| Ok(()));
            assert!(result.is_err(), "{case}");
            if case == "budget" {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("insufficient space for original destination evidence")
                );
            }
            assert_eq!(std::fs::read(&local).unwrap(), before);
            assert_eq!(std::fs::read(&central).unwrap(), before);
        }
    }
    #[test]
    fn windows_native_copy_publication_and_permissions_are_source_free_and_idempotent() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let reserved = staged.journal.permission_tail_size().unwrap();
        let creation_reserve = staged
            .journal
            .creation_tail(RenameKind::Publish)
            .unwrap()
            .iter()
            .map(|record| FRAME_HEADER as u64 + record_payload(record).unwrap().len() as u64)
            .sum::<u64>();
        let initial_length = staged.journal.length;
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        drop((staged, src));
        let mut application =
            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(!root.path().join("destination").join(candidate).exists());
        assert!(application.journal.state.published);
        assert_eq!(application.completed(), 0);
        application.apply(|_| Ok(())).unwrap();
        assert_eq!(application.completed(), 3);
        let length = application.journal.length;
        let actual = length - initial_length;
        assert!(
            reserved >= actual,
            "all native publication/completion records must fit"
        );
        assert!(
            reserved - actual <= creation_reserve + 19 * 3 + 64,
            "only optional creation repair and signed change-time/digest widths may over-reserve"
        );
        assert_eq!(application.journal.permission_tail_size().unwrap(), 0);
        drop(application);
        let report = inspect(&parent, &central, &name).unwrap();
        assert!(report.completion_verified);
        assert_eq!(
            (
                report.published,
                report.publication_pending,
                report.permissions_completed,
                report.permission_pending
            ),
            (true, false, 3, false)
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
        assert_eq!(
            std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
            b"original data"
        );
        // Recovery requires no source path or handles and never reapplies exact ACLs.
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(retained.entry_count(), 3);
        drop(retained);
        let mut again = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        again.apply(|_| Ok(())).unwrap();
        assert_eq!(again.journal.length, length);
    }
    #[test]
    fn windows_native_copy_completion_prefix_resumes_after_failure_or_cancellation_without_source()
    {
        for cancel in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            drop((staged, src));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            if !cancel {
                FAULT.with(|f| f.set(Some((24, Step::CentralFlushed, false))));
            }
            let result = app.apply(|_| {
                if cancel
                    && inspect(&parent, &central, &name)
                        .unwrap()
                        .completion_entries
                        == 1
                {
                    Err(io("cancelled completion recording"))
                } else {
                    Ok(())
                }
            });
            FAULT.with(|f| f.set(None));
            assert!(result.is_err());
            drop(app);
            let report = inspect(&parent, &central, &name).unwrap();
            assert_eq!(report.completion_entries, 1);
            assert!(!report.completion_verified);
            assert!(CompletedCopy::open(&parent, &central, &name, |_| Ok(())).is_err());
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            std::fs::remove_dir_all(root.path().join("source")).unwrap();
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            drop(app);
            let after = std::fs::read(journal).unwrap();
            assert!(after.starts_with(&before));
            let records = journal_records(&after);
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record, Record::FinalEntry { .. }))
                    .count(),
                3
            );
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record, Record::Finalized { .. }))
                    .count(),
                1
            );
            let receipt = CompletedCopy::open(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(receipt.entry_count(), 3);
            assert!(receipt.root_version().directory);
            receipt.revalidate(|_| Ok(())).unwrap();
        }
    }
    #[test]
    fn windows_native_copy_final_versions_refuse_same_bytes_and_restored_write_time() {
        for sealed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source/nested/file"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            drop((staged, src));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            if !sealed {
                FAULT.with(|f| f.set(Some((12, Step::CentralFlushed, false))));
            }
            let result = app.apply(|_| Ok(()));
            FAULT.with(|f| f.set(None));
            assert_eq!(result.is_ok(), sealed);
            drop(app);
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            let state = decode(&before).unwrap().state;
            let recorded = &state.final_versions[0];
            if sealed {
                let receipt = CompletedCopy::open(&parent, &central, &name, |_| Ok(())).unwrap();
                assert_eq!(receipt.root_version(), recorded);
                receipt.revalidate(|_| Ok(())).unwrap();
            }
            std::fs::remove_dir_all(root.path().join("source")).unwrap();
            let target = root.path().join("destination/copy");
            let modified = std::fs::metadata(&target).unwrap().modified().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .unwrap();
            file.write_all(b"original data").unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
            file.sync_all().unwrap();
            drop(file);
            let observed = super::super::version(&source(&target)).unwrap();
            assert_ne!(observed.changed, recorded.changed);
            let mut comparable = observed;
            comparable.changed = recorded.changed;
            assert_eq!(
                &comparable, recorded,
                "only the native change time may differ"
            );
            assert!(CompletedCopy::open(&parent, &central, &name, |_| Ok(())).is_err());
            assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
            assert_eq!(std::fs::read(journal).unwrap(), before);
            assert_eq!(std::fs::read(&target).unwrap(), b"original data");
            assert_eq!(
                std::fs::metadata(target).unwrap().modified().unwrap(),
                modified
            );
        }
    }

    fn fixture_data_path(base: std::path::PathBuf, nested: bool) -> std::path::PathBuf {
        if nested {
            base.join("nested/file")
        } else {
            base
        }
    }
    fn completed_undo_fixture(
        path: &Path,
        nested: bool,
        readonly: bool,
    ) -> (OsString, OsString, ReadonlyCleanup) {
        completed_undo_fixture_at(path, &path.join("destination"), nested, readonly, None)
    }
    fn completed_undo_fixture_at(
        path: &Path,
        destination: &Path,
        nested: bool,
        readonly: bool,
        registry: Option<&Path>,
    ) -> (OsString, OsString, ReadonlyCleanup) {
        let (name, candidate, cleanup) =
            staged_resume_fixture_at(path, destination, nested, readonly, registry);
        let parent = owner(destination);
        let central = owner(registry.unwrap_or(&path.join("central")));
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        (name, candidate, cleanup)
    }
    fn staged_resume_fixture_at(
        path: &Path,
        destination: &Path,
        nested: bool,
        readonly: bool,
        registry: Option<&Path>,
    ) -> (OsString, OsString, ReadonlyCleanup) {
        let mut cleanup = if readonly {
            readonly_fixture(path, nested)
        } else {
            fixture(path);
            ReadonlyCleanup(Vec::new())
        };
        let src = source(&path.join(if nested {
            "source"
        } else {
            "source/nested/file"
        }));
        std::fs::create_dir_all(destination).unwrap();
        let parent = owner(destination);
        let central = if let Some(registry) = registry {
            std::fs::remove_dir(path.join("central")).unwrap();
            drop(
                super::super::registry::AnchoredRoot::open_at(registry, true)
                    .unwrap()
                    .unwrap(),
            );
            owner(registry)
        } else {
            owner(&path.join("central"))
        };
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        cleanup
            .0
            .push(fixture_data_path(destination.join(&candidate), nested));
        drop((staged, src));
        (name, candidate, cleanup)
    }
    fn remove_undo_fixture_source(path: &Path) {
        let file = path.join("source/nested/file");
        let mut permissions = std::fs::metadata(&file).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(file, permissions).unwrap();
        std::fs::remove_dir_all(path.join("source")).unwrap();
    }
    #[test]
    fn windows_native_registered_copy_resume_is_source_free_and_completed_replay_is_readonly() {
        for (nested, readonly) in [(false, false), (true, false), (false, true), (true, true)] {
            let root = tempfile::tempdir().unwrap();
            let (name, _candidate, _cleanup) = staged_resume_fixture_at(
                root.path(),
                &root.path().join("destination"),
                nested,
                readonly,
                None,
            );
            remove_undo_fixture_source(root.path());
            let path = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&path).unwrap();
            let id = decode(&before).unwrap().state.header.unwrap().transaction;
            let mut plan =
                discovery::RetainedCopyResumePlan::admit(&root.path().join("central"), &id, |_| {
                    Ok(())
                })
                .unwrap();
            let mut replay = plan.clone();
            let mut inverse_stale = plan.clone();
            assert!(!plan.complete());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            plan.resume(|_| Ok(())).unwrap();
            assert!(plan.complete());
            assert!(plan.resume(|_| Ok(())).is_err());
            assert_eq!(
                std::fs::read(fixture_data_path(
                    root.path().join("destination/copy"),
                    nested
                ))
                .unwrap(),
                b"original data"
            );
            let sealed = std::fs::read(&path).unwrap();
            assert!(decode(&sealed).unwrap().state.completion_verified);
            replay.resume(|_| Ok(())).unwrap();
            assert_eq!(
                std::fs::read(&path).unwrap(),
                sealed,
                "Completed replay must not acquire writer authority or append records"
            );
            assert!(
                discovery::RetainedCopyResumePlan::admit(
                    &root.path().join("central"),
                    &id,
                    |_| Ok(())
                )
                .is_err()
            );
            let mut undo =
                crate::restore_retained_copy_undo_plan_at(&root.path().join("central"), &id)
                    .unwrap();
            undo.undo_next().unwrap();
            let undone = std::fs::read(&path).unwrap();
            assert!(
                inverse_stale.resume(|_| Ok(())).is_err(),
                "A forward plan must not republish an undone copy"
            );
            assert_eq!(std::fs::read(&path).unwrap(), undone);
            assert!(!root.path().join("destination/copy").exists());
        }
    }
    #[test]
    fn windows_native_registered_copy_resume_finishes_partial_finalization_without_payload_writers()
    {
        for (nested, readonly) in [(false, false), (true, false), (false, true), (true, true)] {
            let reference = tempfile::tempdir().unwrap();
            let (name, _, _reference_cleanup) =
                completed_undo_fixture(reference.path(), nested, readonly);
            let records = journal_records(
                &std::fs::read(reference.path().join("destination").join(name).join(LOCAL))
                    .unwrap(),
            );
            let last = if nested { 2 } else { 0 };
            let boundaries = records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| match record {
                    Record::SecurityApplied { index: entry } if *entry == last => {
                        Some(index as u64)
                    }
                    Record::FinalEntry { index: 0, .. } => Some(index as u64),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(boundaries.len(), 2);
            for sequence in boundaries {
                let root = tempfile::tempdir().unwrap();
                let (name, _, _cleanup) = staged_resume_fixture_at(
                    root.path(),
                    &root.path().join("destination"),
                    nested,
                    readonly,
                    None,
                );
                remove_undo_fixture_source(root.path());
                let path = root.path().join("destination").join(&name).join(LOCAL);
                let id = decode(&std::fs::read(&path).unwrap())
                    .unwrap()
                    .state
                    .header
                    .unwrap()
                    .transaction;
                let mut plan = discovery::RetainedCopyResumePlan::admit(
                    &root.path().join("central"),
                    &id,
                    |_| Ok(()),
                )
                .unwrap();
                FAULT.with(|f| f.set(Some((sequence, Step::CentralFlushed, false))));
                let result = plan.resume(|_| Ok(()));
                FAULT.with(|f| f.set(None));
                assert!(result.is_err(), "{nested} {readonly} {sequence}");
                let before = decode(&std::fs::read(&path).unwrap()).unwrap().state;
                assert!(before.native_complete() && !before.completion_verified);
                let published = root.path().join("destination/copy");
                let data = fixture_data_path(published.clone(), nested);
                let version_before = super::super::version(&source(&data)).unwrap();
                plan.resume(|_| Ok(())).unwrap_or_else(|error| panic!("Finalization recovery nested={nested} readonly={readonly} sequence={sequence}: {error}"));
                assert!(plan.complete());
                assert_eq!(
                    super::super::version(&source(&data)).unwrap(),
                    version_before,
                    "Finalization must not change payload metadata"
                );
                assert_eq!(std::fs::read(data).unwrap(), b"original data");
                assert!(
                    decode(&std::fs::read(path).unwrap())
                        .unwrap()
                        .state
                        .completion_verified
                );
            }
        }
    }
    #[test]
    fn windows_native_registered_copy_resume_refuses_coherent_rewritten_frozen_evidence() {
        let root = tempfile::tempdir().unwrap();
        let (name, _, _cleanup) = staged_resume_fixture_at(
            root.path(),
            &root.path().join("destination"),
            true,
            false,
            None,
        );
        let registry = root.path().join("central");
        let local = root.path().join("destination").join(&name).join(LOCAL);
        let bytes = std::fs::read(&local).unwrap();
        let header = decode(&bytes).unwrap().state.header.unwrap();
        let mut plan =
            discovery::RetainedCopyResumePlan::admit(&registry, &header.transaction, |_| Ok(()))
                .unwrap();
        let mut records = journal_records(&bytes);
        let original = if let Record::CreateIntent { source, .. } = &records[1] {
            source.identity
        } else {
            unreachable!()
        };
        let mut value = serde_json::to_value(original).unwrap();
        value["file_id"] = serde_json::to_value([0xabu8; 16]).unwrap();
        let changed = serde_json::from_value(value).unwrap();
        for record in &mut records {
            match record {
                Record::CreateIntent {
                    index: 0, source, ..
                } => source.identity = changed,
                Record::EntryStaged { index: 0, evidence } => evidence.source.identity = changed,
                _ => {}
            }
        }
        let rewritten = encode_records(&records);
        decode(&rewritten).unwrap();
        std::fs::write(&local, &rewritten).unwrap();
        std::fs::write(registry.join(header.central_name()), &rewritten).unwrap();
        let parent = owner(&root.path().join("destination"));
        let central = owner(&registry);
        drop(RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap());
        assert!(
            plan.resume(|_| Ok(())).is_err(),
            "Fresh coherent evidence must not replace an earlier admission's frozen evidence"
        );
        assert_eq!(std::fs::read(local).unwrap(), rewritten);
        assert!(!root.path().join("destination/copy").exists());
    }
    #[test]
    fn windows_native_registered_copy_resume_retries_publication_and_permission_outcomes_with_original_binding()
     {
        for (sequence, step) in [
            (17, Step::PublicationRenamed),
            (19, Step::PermissionsChanged),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (name, _, _cleanup) = staged_resume_fixture_at(
                root.path(),
                &root.path().join("destination"),
                true,
                false,
                None,
            );
            remove_undo_fixture_source(root.path());
            let path = root.path().join("destination").join(&name).join(LOCAL);
            let id = decode(&std::fs::read(&path).unwrap())
                .unwrap()
                .state
                .header
                .unwrap()
                .transaction;
            let mut plan =
                discovery::RetainedCopyResumePlan::admit(&root.path().join("central"), &id, |_| {
                    Ok(())
                })
                .unwrap();
            FAULT.with(|f| f.set(Some((sequence, step, false))));
            let result = plan.resume(|_| Ok(()));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err(), "{sequence} {step:?}");
            assert!(!plan.complete());
            plan.resume(|_| Ok(())).unwrap();
            assert!(plan.complete());
            assert_eq!(
                std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
                b"original data"
            );
            assert!(
                decode(&std::fs::read(path).unwrap())
                    .unwrap()
                    .state
                    .completion_verified
            );
        }
    }
    #[test]
    fn windows_native_registered_copy_resume_refuses_stale_policy_paths_registry_and_payload_before_writes()
     {
        for case in ["policy", "path", "registry", "payload"] {
            let root = tempfile::tempdir().unwrap();
            let (name, candidate, _cleanup) = staged_resume_fixture_at(
                root.path(),
                &root.path().join("destination"),
                true,
                false,
                None,
            );
            let registry = root.path().join("central");
            let destination = root.path().join("destination");
            let journal = destination.join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            let id = decode(&before).unwrap().state.header.unwrap().transaction;
            let mut plan =
                discovery::RetainedCopyResumePlan::admit(&registry, &id, |_| Ok(())).unwrap();
            match case {
                "policy" => super::super::tests::set_dacl(
                    &destination,
                    "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
                ),
                "path" => {
                    std::fs::rename(&destination, root.path().join("moved")).unwrap();
                    std::fs::create_dir(&destination).unwrap();
                }
                "registry" => {
                    std::fs::rename(&registry, root.path().join("saved-central")).unwrap();
                    std::fs::create_dir(&registry).unwrap();
                }
                "payload" => std::fs::write(
                    destination.join(&candidate).join("nested/file"),
                    b"external data",
                )
                .unwrap(),
                _ => unreachable!(),
            }
            assert!(plan.resume(|_| Ok(())).is_err(), "{case}");
            assert!(!plan.complete());
            let actual = if case == "path" {
                root.path().join("moved")
            } else {
                destination
            };
            assert_eq!(
                std::fs::read(actual.join(name).join(LOCAL)).unwrap(),
                before
            );
            assert!(!actual.join("copy").exists());
            assert_eq!(
                std::fs::read(actual.join(candidate).join("nested/file")).unwrap(),
                if case == "payload" {
                    b"external data".as_slice()
                } else {
                    b"original data".as_slice()
                }
            );
        }
    }
    #[test]
    fn windows_native_registered_copy_resume_cancellation_preserves_stage_and_unverified_staging_is_not_admitted()
     {
        let root = tempfile::tempdir().unwrap();
        let (name, _, _cleanup) = staged_resume_fixture_at(
            root.path(),
            &root.path().join("destination"),
            true,
            false,
            None,
        );
        let path = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&path).unwrap();
        let id = decode(&before).unwrap().state.header.unwrap().transaction;
        let registry = root.path().join("central");
        let mut plan =
            discovery::RetainedCopyResumePlan::admit(&registry, &id, |_| Ok(())).unwrap();
        let mut cancelled = false;
        assert!(
            plan.resume(|bytes| if bytes > 0 {
                cancelled = true;
                Err(io("cancelled"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert!(cancelled);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!root.path().join("destination/copy").exists());
        plan.resume(|_| Ok(())).unwrap();
        let incomplete = tempfile::tempdir().unwrap();
        fixture(incomplete.path());
        let src = source(&incomplete.path().join("source"));
        let parent = owner(&incomplete.path().join("destination"));
        let central = owner(&incomplete.path().join("central"));
        FAULT.with(|f| f.set(Some((0, Step::CentralFlushed, false))));
        let result = stage(&src, &parent, &central);
        FAULT.with(|f| f.set(None));
        assert!(result.is_err());
        drop((result, src));
        let name = stage_name(&incomplete.path().join("destination"));
        let path = incomplete.path().join("destination").join(name).join(LOCAL);
        let bytes = std::fs::read(&path).unwrap();
        let id = decode(&bytes).unwrap().state.header.unwrap().transaction;
        assert!(
            discovery::RetainedCopyResumePlan::admit(
                &incomplete.path().join("central"),
                &id,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    #[ignore = "exports an owned fixture to the native Activity integration consumer"]
    fn windows_copy_activity_fixture() {
        let root = std::path::PathBuf::from(
            std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_ROOT")
                .expect("owned fixture root missing"),
        );
        let local = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap());
        assert_eq!(local, root.join("profile"), "Refuse a real user profile");
        let fixture_root = root.join("fixture");
        std::fs::create_dir(&fixture_root).unwrap();
        let destination = std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_DESTINATION")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| fixture_root.join("destination"));
        if destination != fixture_root.join("destination") {
            let volume = std::path::PathBuf::from(
                std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_VOLUME")
                    .expect("owned secondary volume missing"),
            );
            assert!(destination.starts_with(volume));
        }
        let registry = local.join("Ottrin/State/copies");
        let case = std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").unwrap();
        let (name, candidate, _cleanup) = if case.starts_with("resume") {
            staged_resume_fixture_at(&fixture_root, &destination, true, false, Some(&registry))
        } else {
            completed_undo_fixture_at(&fixture_root, &destination, true, false, Some(&registry))
        };
        if case == "resume-finalizing" {
            let reference = tempfile::tempdir().unwrap();
            let (reference_name, _, _reference_cleanup) =
                completed_undo_fixture(reference.path(), true, false);
            let records = journal_records(
                &std::fs::read(
                    reference
                        .path()
                        .join("destination")
                        .join(reference_name)
                        .join(LOCAL),
                )
                .unwrap(),
            );
            let sequence = records
                .iter()
                .position(|record| matches!(record, Record::SecurityApplied { index: 2 }))
                .unwrap() as u64;
            FAULT.with(|f| f.set(Some((sequence, Step::CentralFlushed, false))));
            let parent = owner(&destination);
            let central = owner(&registry);
            let result = PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                .and_then(|mut stage| stage.apply(|_| Ok(())));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err());
            let state = decode(&std::fs::read(destination.join(&name).join(LOCAL)).unwrap())
                .unwrap()
                .state;
            assert!(state.native_complete() && !state.completion_verified);
        }
        if destination != fixture_root.join("destination") {
            assert_ne!(
                stable_identity_from_handle(&owner(&destination))
                    .unwrap()
                    .volume_serial_number(),
                stable_identity_from_handle(&owner(&registry))
                    .unwrap()
                    .volume_serial_number(),
                "Cross-volume fixture must use distinct native volumes"
            );
        }
        remove_undo_fixture_source(&fixture_root);
        if std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").as_deref() == Ok("pending") {
            let parent = owner(&destination);
            let central = owner(&registry);
            FAULT.with(|f| f.set(Some((29, Step::UndoRenamed, false))));
            let result = undo_completed_copy(&parent, &central, &name, |_| Ok(()));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err());
            assert!(destination.join(candidate).exists());
        }
        if case.starts_with("name-release") {
            let parent = owner(&destination);
            let central = owner(&registry);
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            let header = decode(&std::fs::read(destination.join(&name).join(LOCAL)).unwrap())
                .unwrap()
                .state
                .header
                .unwrap();
            let claim = destination.join(header.claim.test_relative_path());
            let full = std::fs::read(&claim).unwrap();
            let end = FRAME_HEADER + u32::from_le_bytes(full[8..12].try_into().unwrap()) as usize;
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&claim)
                .unwrap();
            file.write_all(&full[..end + 12]).unwrap();
            file.sync_all().unwrap();
            std::fs::write(
                root.join("name-release.json"),
                serde_json::to_vec(&serde_json::json!({
                    "claim": claim.as_os_str().encode_wide().collect::<Vec<_>>(), "full": full,
                }))
                .unwrap(),
            )
            .unwrap();
        }
        let retain_for_cleanup = |destination: &Path, name: &OsStr| {
            undo_completed_copy(&owner(destination), &owner(&registry), name, |_| Ok(())).unwrap();
            std::fs::create_dir_all(destination.join("copy/nested")).unwrap();
            std::fs::write(
                destination.join("copy/nested/file"),
                b"new independent public data",
            )
            .unwrap();
        };
        if case.starts_with("cleanup") {
            drop(_cleanup); // Clear fixture attributes before freezing the withdrawn copy.
            retain_for_cleanup(&destination, &name);
        }
        if case.starts_with("cleanup-cohort") {
            let second = root.join("fixture-second");
            std::fs::create_dir(&second).unwrap();
            let (second_name, _, second_cleanup) = completed_undo_fixture_at(
                &second,
                &second.join("destination"),
                true,
                false,
                Some(&registry),
            );
            remove_undo_fixture_source(&second);
            drop(second_cleanup);
            retain_for_cleanup(&second.join("destination"), &second_name);
        }
        // The calling UI test owns root and tears it down after its child exits.
    }
    #[test]
    fn windows_native_registered_copy_undo_admission_is_readonly_and_source_free() {
        for (nested, readonly) in [(false, false), (true, false), (false, true), (true, true)] {
            let root = tempfile::tempdir().unwrap();
            let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), nested, readonly);
            remove_undo_fixture_source(root.path());
            let registry = root.path().join("central");
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            let id = decode(&before).unwrap().state.header.unwrap().transaction;
            let mut plan = crate::restore_retained_copy_undo_plan_at(&registry, &id).unwrap();
            assert_eq!(plan.remaining(), 1);
            assert_eq!(
                plan.path_pairs()[0].0.file_name(),
                Some(candidate.as_os_str())
            );
            assert_eq!(plan.path_pairs()[0].1.file_name(), Some(OsStr::new("copy")));
            assert_eq!(std::fs::read(&journal).unwrap(), before);
            plan.undo_next_with_progress(|_| Ok(())).unwrap();
            assert_eq!(plan.remaining(), 0);
            assert!(plan.undo_next_with_progress(|_| Ok(())).is_err());
            assert!(!root.path().join("destination/copy").exists());
            assert_eq!(
                std::fs::read(fixture_data_path(
                    root.path().join("destination").join(candidate),
                    nested
                ))
                .unwrap(),
                b"original data"
            );
            assert!(crate::restore_retained_copy_undo_plan_at(&registry, &id).is_err());
        }
    }
    #[test]
    fn windows_native_registered_copy_undo_rejects_moved_display_path_then_fresh_admission_works() {
        let root = tempfile::tempdir().unwrap();
        let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let registry = root.path().join("central");
        let original = root.path().join("destination");
        let before = std::fs::read(original.join(&name).join(LOCAL)).unwrap();
        let id = decode(&before).unwrap().state.header.unwrap().transaction;
        let mut plan = crate::restore_retained_copy_undo_plan_at(&registry, &id).unwrap();
        let moved = root.path().join("moved");
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("copy"), b"unrelated new name").unwrap();
        assert!(plan.undo_next_with_progress(|_| Ok(())).is_err());
        assert_eq!(plan.remaining(), 1);
        assert_eq!(
            std::fs::read(moved.join(&name).join(LOCAL)).unwrap(),
            before
        );
        assert_eq!(
            std::fs::read(original.join("copy")).unwrap(),
            b"unrelated new name"
        );
        let mut fresh = crate::restore_retained_copy_undo_plan_at(&registry, &id).unwrap();
        assert!(fresh.path_pairs()[0].1.ends_with(Path::new("moved/copy")));
        fresh.undo_next_with_progress(|_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(moved.join(candidate).join("nested/file")).unwrap(),
            b"original data"
        );
        assert_eq!(
            std::fs::read(original.join("copy")).unwrap(),
            b"unrelated new name"
        );
    }
    #[test]
    fn windows_native_registered_copy_undo_refuses_replaced_registry_and_changed_payload() {
        for replace_registry in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _, _cleanup) = completed_undo_fixture(root.path(), true, false);
            let registry = root.path().join("central");
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            let id = decode(&before).unwrap().state.header.unwrap().transaction;
            let mut plan = crate::restore_retained_copy_undo_plan_at(&registry, &id).unwrap();
            if replace_registry {
                std::fs::rename(&registry, root.path().join("saved-central")).unwrap();
                std::fs::create_dir(&registry).unwrap();
            } else {
                std::fs::write(
                    root.path().join("destination/copy/nested/file"),
                    b"external data",
                )
                .unwrap();
            }
            assert!(plan.undo_next_with_progress(|_| Ok(())).is_err());
            assert_eq!(plan.remaining(), 1);
            assert_eq!(std::fs::read(&journal).unwrap(), before);
            assert!(root.path().join("destination/copy").exists());
        }
        let error =
            crate::restore_retained_copy_undo_plan_at(Path::new("Z:\\absent"), "../outside")
                .unwrap_err();
        assert!(error.to_string().contains("invalid Copy transaction ID"));
    }
    #[test]
    fn windows_native_registered_copy_undo_cancellation_and_interrupted_retry_keep_binding() {
        let root = tempfile::tempdir().unwrap();
        let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let registry = root.path().join("central");
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        let id = decode(&before).unwrap().state.header.unwrap().transaction;
        let mut plan = crate::restore_retained_copy_undo_plan_at(&registry, &id).unwrap();
        let mut cancelled = false;
        assert!(
            plan.undo_next_with_progress(|bytes| if bytes > 0 {
                cancelled = true;
                Err(io("cancelled"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert!(cancelled);
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        FAULT.with(|f| f.set(Some((29, Step::UndoRenamed, false))));
        assert!(plan.undo_next_with_progress(|_| Ok(())).is_err());
        FAULT.with(|f| f.set(None));
        assert_eq!(plan.remaining(), 1);
        assert!(root.path().join("destination").join(&candidate).exists());
        plan.undo_next_with_progress(|_| Ok(())).unwrap();
        assert_eq!(plan.remaining(), 0);
    }
    #[test]
    fn windows_native_copy_undo_retains_files_trees_and_readonly_data_without_source() {
        for (nested, readonly) in [(false, false), (true, false), (false, true), (true, true)] {
            let root = tempfile::tempdir().unwrap();
            let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), nested, readonly);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            remove_undo_fixture_source(root.path());
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            assert!(!root.path().join("destination/copy").exists());
            let retained =
                fixture_data_path(root.path().join("destination").join(candidate), nested);
            assert_eq!(std::fs::read(&retained).unwrap(), b"original data");
            assert_eq!(
                std::fs::metadata(&retained)
                    .unwrap()
                    .permissions()
                    .readonly(),
                readonly
            );
            let report = inspect(&parent, &central, &name).unwrap();
            assert!(report.undone);
            assert!(!report.undo_pending);
            assert!(CompletedCopy::open(&parent, &central, &name, |_| Ok(())).is_err());
            assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(std::fs::read(journal).unwrap(), before);
        }
    }
    #[test]
    fn windows_native_copy_undo_refuses_changed_data_and_unrecorded_aliases_before_intent() {
        for alias in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, _candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let target = root.path().join("destination/copy/nested/file");
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            if alias {
                std::fs::hard_link(&target, root.path().join("external-alias")).unwrap();
            } else {
                std::fs::write(&target, b"external data").unwrap();
            }
            assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
            assert_eq!(std::fs::read(journal).unwrap(), before);
            assert_eq!(
                std::fs::read(&target).unwrap(),
                if alias {
                    b"original data"
                } else {
                    b"external data"
                }
            );
            if alias {
                assert_eq!(
                    std::fs::read(root.path().join("external-alias")).unwrap(),
                    b"original data"
                );
            }
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
        }
    }
    #[test]
    fn windows_native_copy_undo_preserves_occupied_retention_and_new_publication_names() {
        for occupied_retention in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let slot = root.path().join("destination").join(&candidate);
            if occupied_retention {
                std::fs::write(&slot, b"foreign retention occupant").unwrap();
            } else {
                let sequence = journal_records(
                    &std::fs::read(root.path().join("destination").join(&name).join(LOCAL))
                        .unwrap(),
                )
                .len() as u64
                    + 1;
                FAULT.with(|f| f.set(Some((sequence, Step::UndoRenamed, false))));
            }
            let result = undo_completed_copy(&parent, &central, &name, |_| Ok(()));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err());
            let report = inspect(&parent, &central, &name).unwrap();
            assert!(report.undo_pending);
            assert!(!report.undone);
            if occupied_retention {
                assert_eq!(std::fs::read(&slot).unwrap(), b"foreign retention occupant");
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
                    b"original data"
                );
                std::fs::remove_file(&slot).unwrap();
            } else {
                std::fs::write(
                    root.path().join("destination/copy"),
                    b"new unrelated publication",
                )
                .unwrap();
            }
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(
                std::fs::read(slot.join("nested/file")).unwrap(),
                b"original data"
            );
            if !occupied_retention {
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy")).unwrap(),
                    b"new unrelated publication"
                );
            }
        }
    }
    #[test]
    fn windows_native_copy_undo_cancellation_after_retraction_keeps_a_resumable_intent() {
        for readonly in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), false, readonly);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let slot = root.path().join("destination").join(&candidate);
            let mut cancelled = false;
            let result = undo_completed_copy(&parent, &central, &name, |_| {
                if slot.exists() {
                    cancelled = true;
                    Err(io("cancelled Undo verification"))
                } else {
                    Ok(())
                }
            });
            assert!(cancelled);
            assert!(result.is_err());
            assert_eq!(std::fs::read(&slot).unwrap(), b"original data");
            assert!(!root.path().join("destination/copy").exists());
            let report = inspect(&parent, &central, &name).unwrap();
            assert!(report.undo_pending);
            assert!(!report.undone);
            remove_undo_fixture_source(root.path());
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            assert!(inspect(&parent, &central, &name).unwrap().undone);
            assert_eq!(std::fs::read(&slot).unwrap(), b"original data");
            assert_eq!(
                std::fs::metadata(slot).unwrap().permissions().readonly(),
                readonly
            );
        }
    }

    #[test]
    fn windows_native_copy_undo_refuses_an_active_writer_without_journal_mutation_then_retries() {
        let root = tempfile::tempdir().unwrap();
        let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), false, false);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let target = root.path().join("destination/copy");
        let writer = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&target)
            .unwrap();
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        assert!(target.exists());
        drop(writer);
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(!target.exists());
        assert_eq!(
            std::fs::read(root.path().join("destination").join(candidate)).unwrap(),
            b"original data"
        );
    }

    #[test]
    fn windows_native_copy_undo_directory_handoff_drift_retains_changed_data_without_completion() {
        struct ClearHook;
        impl Drop for ClearHook {
            fn drop(&mut self) {
                UNDO_HANDOFF.with(|slot| slot.borrow_mut().take());
            }
        }
        let root = tempfile::tempdir().unwrap();
        let (name, candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let changed = root.path().join("destination/copy/nested/file");
        let _clear = ClearHook;
        UNDO_HANDOFF.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                std::fs::write(changed, b"external data").unwrap()
            }))
        });
        let error = undo_completed_copy(&parent, &central, &name, |_| Ok(()))
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains(candidate.to_string_lossy().as_ref())
        );
        let report = inspect(&parent, &central, &name).unwrap();
        assert!(report.undo_pending);
        assert!(!report.undone);
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(&candidate)
                    .join("nested/file")
            )
            .unwrap(),
            b"external data"
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(journal).unwrap(), before);
    }

    #[test]
    fn windows_native_copy_undo_flush_failures_and_process_exits_recover_or_retain() {
        for (nested, readonly) in [(true, false), (false, false), (false, true)] {
            for process_exit in [false, true] {
                // Frame indices follow each fixture's actual sealed completion.
                let first = if nested {
                    28
                } else if readonly {
                    16
                } else {
                    14
                };
                let last = first + if readonly { 3 } else { 1 };
                let mut faults = (first..=last)
                    .flat_map(|sequence| {
                        [Step::LocalFlushed, Step::CentralFlushed].map(|step| (sequence, step))
                    })
                    .collect::<Vec<_>>();
                faults.push((first + 1, Step::UndoReadersReleased));
                faults.push((first + 1, Step::UndoRenamed));
                faults.push((last, Step::UndoFlushed));
                if readonly {
                    faults.extend(
                        [
                            Step::ReadonlyWritable,
                            Step::ReadonlyRestored,
                            Step::ReadonlyFlushed,
                        ]
                        .map(|step| (first + 2, step)),
                    );
                }
                for (sequence, step) in faults {
                    let root = tempfile::tempdir().unwrap();
                    let (name, candidate, _cleanup) =
                        completed_undo_fixture(root.path(), nested, readonly);
                    let parent = owner(&root.path().join("destination"));
                    let central = owner(&root.path().join("central"));
                    remove_undo_fixture_source(root.path());
                    if process_exit {
                        let label = match step {
                            Step::LocalFlushed => "local",
                            Step::CentralFlushed => "central",
                            Step::UndoReadersReleased => "undo-readers-released",
                            Step::UndoRenamed => "undo-renamed",
                            Step::UndoFlushed => "undo-flushed",
                            Step::ReadonlyWritable => "readonly-writable",
                            Step::ReadonlyRestored => "readonly-restored",
                            Step::ReadonlyFlushed => "readonly-flushed",
                            _ => unreachable!(),
                        };
                        let status = std::process::Command::new(std::env::current_exe().unwrap())
                            .args(["--exact","copy_transaction::windows::journal::tests::windows_native_copy_journal_exit_child","--ignored","--nocapture"])
                            .env("OTTRIN_COPY_JOURNAL_CHILD",root.path()).env("OTTRIN_COPY_JOURNAL_SEQUENCE",sequence.to_string())
                            .env("OTTRIN_COPY_JOURNAL_STEP",label).env("OTTRIN_COPY_UNDO_CHILD","1").status().unwrap();
                        assert_eq!(
                            status.code(),
                            Some(86),
                            "{nested} {readonly} {sequence} {step:?}"
                        );
                    } else {
                        FAULT.with(|f| f.set(Some((sequence, step, false))));
                        let result = undo_completed_copy(&parent, &central, &name, |_| Ok(()));
                        FAULT.with(|f| f.set(None));
                        assert!(result.is_err(), "{nested} {readonly} {sequence} {step:?}");
                    }
                    let journal = root.path().join("destination").join(&name).join(LOCAL);
                    let before = std::fs::read(&journal).unwrap();
                    if step == Step::LocalFlushed {
                        assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
                        assert_eq!(std::fs::read(journal).unwrap(), before);
                    } else {
                        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap_or_else(
                            |error| {
                                panic!("resume {nested} {readonly} {sequence} {step:?}: {error}")
                            },
                        );
                        assert!(std::fs::read(journal).unwrap().starts_with(&before));
                        assert!(!root.path().join("destination/copy").exists());
                    }
                    let slot = root.path().join("destination").join(&candidate);
                    let base = if slot.exists() {
                        slot
                    } else {
                        root.path().join("destination/copy")
                    };
                    assert_eq!(
                        std::fs::read(fixture_data_path(base, nested)).unwrap(),
                        b"original data"
                    );
                }
            }
        }
    }

    #[test]
    fn windows_native_copy_discovery_distinguishes_staged_completed_pending_and_undone() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        let busy =
            crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap();
        assert!(matches!(
            &busy[0].evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        drop((staged, src));
        let observe =
            || crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap();
        let statuses = observe();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::Staged));
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        drop(app);
        assert_eq!(observe()[0].phase, Some(WindowsCopyPhase::Completed));
        FAULT.with(|f| f.set(Some((29, Step::UndoRenamed, false))));
        assert!(undo_completed_copy(&parent, &central, &name, |_| Ok(())).is_err());
        FAULT.with(|f| f.set(None));
        let pending = observe();
        assert_eq!(pending[0].owned_location, pending[0].retained);
        assert_eq!(pending[0].phase, Some(WindowsCopyPhase::UndoPending));
        assert_eq!(
            pending[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        let undone = observe();
        assert_eq!(undone[0].owned_location, undone[0].retained);
        assert_eq!(undone[0].phase, Some(WindowsCopyPhase::Undone));
        assert_eq!(
            undone[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        let retained = root
            .path()
            .join("destination")
            .join(&candidate)
            .join("nested/file");
        std::fs::write(&retained, b"external data").unwrap();
        assert!(matches!(
            &observe()[0].evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert_eq!(std::fs::read(journal).unwrap(), before);
        assert_eq!(std::fs::read(retained).unwrap(), b"external data");
    }
    #[test]
    fn windows_native_copy_discovery_uses_native_parent_identity_after_rename_and_ignores_path_hints()
     {
        let root = tempfile::tempdir().unwrap();
        let (name, _candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let central_root = root.path().join("central");
        let original = root.path().join("destination");
        let journal = original.join(&name).join(LOCAL);
        let bytes = std::fs::read(&journal).unwrap();
        let mut records = journal_records(&bytes);
        let central_name = if let Record::Begin { header } = &mut records[0] {
            header.parent_location =
                NativePath::encode(Path::new(r"\\untrusted.invalid\share\must-not-open"));
            header.central_name()
        } else {
            unreachable!()
        };
        let patched = encode_records(&records);
        std::fs::write(&journal, &patched).unwrap();
        std::fs::write(central_root.join(central_name), &patched).unwrap();
        let moved = root.path().join("renamed-destination");
        std::fs::rename(&original, &moved).unwrap();
        let statuses = crate::discover_retained_copy_transactions_at(&central_root).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].owned_location, statuses[0].publication);
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        assert!(
            statuses[0]
                .publication
                .as_ref()
                .unwrap()
                .ends_with(Path::new("renamed-destination").join("copy"))
        );
        assert_eq!(
            std::fs::read(moved.join(&name).join(LOCAL)).unwrap(),
            patched
        );
    }
    #[test]
    fn windows_native_copy_discovery_reports_bad_entries_without_hiding_valid_records_or_following_links()
     {
        let root = tempfile::tempdir().unwrap();
        let (_name, _candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let central_root = root.path().join("central");
        std::fs::write(
            central_root.join(format!("{}.copy-journal", "0".repeat(32))),
            b"corrupt central data",
        )
        .unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"outside bytes remain untouched").unwrap();
        std::os::windows::fs::symlink_file(
            &outside,
            central_root.join(format!("{}.copy-journal", "1".repeat(32))),
        )
        .unwrap();
        std::fs::write(central_root.join("unrelated.txt"), b"not a registry entry").unwrap();
        let statuses = crate::discover_retained_copy_transactions_at(&central_root).unwrap();
        assert_eq!(statuses.len(), 3);
        assert_eq!(
            statuses
                .iter()
                .filter(|status| status.evidence == WindowsCopyRecoveryEvidence::VerifiedTree)
                .count(),
            1
        );
        assert!(
            statuses
                .iter()
                .filter(|status| status.phase.is_none())
                .all(|status| matches!(
                    &status.evidence,
                    WindowsCopyRecoveryEvidence::NeedsReview { .. }
                ) && status.publication.is_none())
        );
        let linked = statuses
            .iter()
            .find(|status| status.transaction_id == Some("1".repeat(32)))
            .unwrap();
        assert!(
            matches!(&linked.evidence, WindowsCopyRecoveryEvidence::NeedsReview { reason } if reason.contains("ordinary journal"))
        );
        assert_eq!(
            std::fs::read(outside).unwrap(),
            b"outside bytes remain untouched"
        );
    }
    #[test]
    fn windows_native_copy_discovery_preserves_mismatched_mirrors_and_unverified_staging() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        FAULT.with(|f| f.set(Some((0, Step::CentralFlushed, false))));
        assert!(stage(&src, &parent, &central).is_err());
        FAULT.with(|f| f.set(None));
        drop(src);
        let name = stage_name(&root.path().join("destination"));
        let statuses =
            crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap();
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::Staging));
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::UnverifiedStage
        );
        let path = root.path().join("destination").join(&name).join(LOCAL);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"incoherent local tail").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let before = std::fs::read(&path).unwrap();
        let statuses =
            crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap();
        assert!(matches!(
            &statuses[0].evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn windows_native_copy_discovery_missing_registry_is_readonly_and_root_junction_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing-registry");
        assert!(
            crate::discover_retained_copy_transactions_at(&missing)
                .unwrap()
                .is_empty()
        );
        assert!(!missing.exists());
        let central = root.path().join("central");
        std::fs::create_dir(&central).unwrap();
        let base = owner(root.path());
        let link = windows_fs::create_private(
            &base,
            OsStr::new("registry-link"),
            true,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        links::junction_fixture(&central).install(&link).unwrap();
        drop(link);
        assert!(
            crate::discover_retained_copy_transactions_at(&root.path().join("registry-link"))
                .is_err()
        );
        assert_eq!(std::fs::read_dir(central).unwrap().count(), 0);
    }
    #[test]
    fn windows_native_copy_discovery_cancels_payload_read_without_any_journal_mutation() {
        let root = tempfile::tempdir().unwrap();
        let (name, _candidate, _cleanup) = completed_undo_fixture(root.path(), true, false);
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        let mut cancelled = false;
        let result = crate::discover_retained_copy_transactions_at_with_progress(
            &root.path().join("central"),
            |bytes| {
                if bytes > 0 {
                    cancelled = true;
                    Err(io("cancelled discovery"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(cancelled);
        assert!(result.is_err());
        assert_eq!(std::fs::read(journal).unwrap(), before);
        assert_eq!(
            std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
            b"original data"
        );
        assert_eq!(
            crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap()[0]
                .evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
    }

    #[test]
    fn windows_native_copy_discovery_refuses_drifted_staged_permission_plan_without_repair() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        drop((staged, src));
        let registry = root.path().join("central");
        let ready = crate::discover_retained_copy_transactions_at(&registry).unwrap();
        assert_eq!(ready[0].phase, Some(WindowsCopyPhase::ReadyToPublish));
        assert_eq!(ready[0].evidence, WindowsCopyRecoveryEvidence::VerifiedTree);
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        super::super::tests::set_dacl(
            &root.path().join("destination"),
            "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
        );
        let policy = security::DestinationSecurity::capture(&parent, false).unwrap();
        let changed = crate::discover_retained_copy_transactions_at(&registry).unwrap();
        assert!(matches!(
            &changed[0].evidence,
            WindowsCopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert_eq!(std::fs::read(journal).unwrap(), before);
        policy.validate_parent(&parent).unwrap();
    }
    #[test]
    #[ignore = "requires an owned secondary NTFS volume provisioned by the native QA controller"]
    fn windows_native_copy_discovery_resolves_parent_on_an_owned_other_volume() {
        let volume = std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_DISCOVERY_VOLUME")
            .expect("owned volume missing");
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let destination = tempfile::tempdir_in(std::path::PathBuf::from(volume)).unwrap();
        let src = source(&root.path().join("source"));
        let parent = owner(destination.path());
        let central = owner(&root.path().join("central"));
        assert_ne!(
            stable_identity_from_handle(&parent)
                .unwrap()
                .volume_serial_number(),
            stable_identity_from_handle(&central)
                .unwrap()
                .volume_serial_number()
        );
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        drop((staged, src));
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        drop(app);
        let statuses =
            crate::discover_retained_copy_transactions_at(&root.path().join("central")).unwrap();
        assert_eq!(statuses.len(), 1);
        assert_eq!(
            statuses[0].evidence,
            WindowsCopyRecoveryEvidence::VerifiedTree
        );
        assert_eq!(statuses[0].phase, Some(WindowsCopyPhase::Completed));
        assert_eq!(
            std::fs::read(destination.path().join("copy/nested/file")).unwrap(),
            b"original data"
        );
    }

    fn application_faults() -> Vec<(u64, Step)> {
        let mut result = Vec::new();
        for sequence in 16..28 {
            for step in [Step::LocalFlushed, Step::CentralFlushed] {
                result.push((sequence, step));
            }
        }
        result.extend([
            (17, Step::PublicationRenamed),
            (17, Step::PublicationFlushed),
        ]);
        for sequence in [19, 21, 23] {
            result.extend([
                (sequence, Step::PermissionsChanged),
                (sequence, Step::PermissionsFlushed),
            ]);
        }
        result
    }
    #[test]
    fn windows_native_copy_application_flush_and_native_failures_recover_or_retain() {
        for (sequence, step) in application_faults() {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            let name = staged.stage.name.clone();
            drop((staged, src));
            FAULT.with(|f| f.set(Some((sequence, step, false))));
            let result = PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                .and_then(|mut app| app.apply(|_| Ok(())));
            FAULT.with(|f| f.set(None));
            assert!(result.is_err(), "did not reach {sequence} {step:?}");
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            if step == Step::LocalFlushed {
                let path = root.path().join("destination").join(&name).join(LOCAL);
                let bytes = std::fs::read(&path).unwrap();
                assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    bytes,
                    "mismatched mirrors must not be repaired"
                );
            } else {
                std::fs::remove_dir_all(root.path().join("source")).unwrap();
                let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                    .unwrap_or_else(|error| panic!("reopen {sequence} {step:?}: {error}"));
                app.apply(|_| Ok(()))
                    .unwrap_or_else(|error| panic!("apply {sequence} {step:?}: {error}"));
                assert_eq!(app.completed(), 3);
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
                    b"original data"
                );
            }
        }
    }
    #[test]
    fn windows_native_copy_application_process_exits_recover_without_sources() {
        for (sequence, step) in application_faults() {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let label = match step {
                Step::LocalFlushed => "local",
                Step::CentralFlushed => "central",
                Step::PublicationRenamed => "renamed",
                Step::PublicationFlushed => "published-flushed",
                Step::PermissionsChanged => "changed",
                Step::PermissionsFlushed => "permissions-flushed",
                Step::ReadonlyWritable => "readonly-writable",
                Step::ReadonlyRestored => "readonly-restored",
                Step::ReadonlyFlushed => "readonly-flushed",
                Step::UndoReadersReleased => "undo-readers-released",
                Step::UndoRenamed => "undo-renamed",
                Step::UndoFlushed => "undo-flushed",
                Step::OriginalReadersReleased => "original-readers-released",
                Step::OriginalRenamed => "original-renamed",
                Step::OriginalFlushed => "original-flushed",
                Step::CreationRestored => "creation-restored",
                Step::CreationFlushed => "creation-flushed",
                Step::CleanupDispositionSet => "cleanup-disposition",
                Step::CleanupReadersReleased => "cleanup-readers-released",
                Step::CleanupHandleClosed => "cleanup-closed",
                Step::CleanupFlushed => "cleanup-flushed",
                Step::CleanupMetadata { .. } => "cleanup-metadata",
            };
            let status=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","copy_transaction::windows::journal::tests::windows_native_copy_journal_exit_child","--ignored","--nocapture"])
                .env("OTTRIN_COPY_JOURNAL_CHILD",root.path()).env("OTTRIN_COPY_JOURNAL_SEQUENCE",sequence.to_string())
                .env("OTTRIN_COPY_JOURNAL_STEP",label).status().unwrap();
            assert_eq!(status.code(), Some(86), "{sequence} {step:?}");
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let name = stage_name(&root.path().join("destination"));
            if step == Step::LocalFlushed {
                assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
            } else {
                std::fs::remove_dir_all(root.path().join("source")).unwrap();
                let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                    .unwrap_or_else(|error| panic!("reopen exit {sequence} {step:?}: {error}"));
                app.apply(|_| Ok(())).unwrap();
                assert_eq!(app.completed(), 3);
                assert_eq!(
                    std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
                    b"original data"
                );
            }
        }
    }
    #[test]
    fn windows_native_copy_publication_collision_keeps_both_entries_and_retries() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        drop((staged, src));
        let external = root.path().join("destination/copy");
        std::fs::write(&external, b"external occupant").unwrap();
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&external).unwrap(), b"external occupant");
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(candidate)
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
        std::fs::remove_file(external).unwrap();
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        assert_eq!(app.completed(), 3);
    }
    #[test]
    fn windows_native_copy_published_link_roots_leave_populated_targets_unchanged() {
        for kind in 0..3 {
            let directory = kind != 0;
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("destination")).unwrap();
            std::fs::create_dir(root.path().join("central")).unwrap();
            let base = owner(root.path());
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
            let target =
                windows_fs::create_private(&base, OsStr::new("target"), directory, sharing)
                    .unwrap();
            let leaf = directory.then(|| {
                windows_fs::create_private(&target, OsStr::new("keep"), false, sharing).unwrap()
            });
            let data = leaf.as_ref().unwrap_or(&target);
            (&*data).write_all(b"target bytes stay intact").unwrap();
            let creator = security::current_owner().unwrap();
            let target_security = security::PrivateSecurity::capture(&target, &creator).unwrap();
            let leaf_security = leaf
                .as_ref()
                .map(|file| security::PrivateSecurity::capture(file, &creator).unwrap());
            let target_path = root.path().join("target");
            let link_path = root.path().join("source-link");
            if kind == 2 {
                let link =
                    windows_fs::create_private(&base, OsStr::new("source-link"), true, sharing)
                        .unwrap();
                links::junction_fixture(&target_path)
                    .install(&link)
                    .unwrap();
            } else if directory {
                std::os::windows::fs::symlink_dir(&target_path, &link_path).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&target_path, &link_path).unwrap();
            }
            let src = source(&link_path);
            let raw = links::capture(&src).unwrap();
            let mut staged = stage(&src, &parent, &central).unwrap();
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            let name = staged.stage.name.clone();
            let candidate = staged.stage.candidate_name.clone();
            drop((staged, src));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            assert_eq!(app.completed(), 1);
            assert_eq!(links::capture(&app.candidates[0]).unwrap(), raw);
            drop(app);
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
            let retained = source(&root.path().join("destination").join(candidate));
            assert_eq!(links::capture(&retained).unwrap(), raw);
            drop(retained);
            assert!(!root.path().join("destination/copy").exists());
            assert_eq!(
                security::PrivateSecurity::capture(&target, &creator).unwrap(),
                target_security
            );
            assert_eq!(
                leaf.as_ref()
                    .map(|file| security::PrivateSecurity::capture(file, &creator).unwrap()),
                leaf_security
            );
            assert_eq!(
                std::fs::read(if directory {
                    target_path.join("keep")
                } else {
                    target_path
                })
                .unwrap(),
                b"target bytes stay intact"
            );
        }
    }
    #[test]
    fn windows_native_copy_application_refuses_unrecorded_candidate_hard_links() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source/nested/file"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        drop((staged, src));
        let file = root.path().join("destination").join(candidate);
        let alias = root.path().join("alias");
        std::fs::hard_link(&file, &alias).unwrap();
        let before =
            std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap();
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(std::fs::read(&alias).unwrap(), b"original data");
        assert_eq!(
            std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap(),
            before
        );
        std::fs::remove_file(alias).unwrap();
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        assert_eq!(app.completed(), 1);
    }
    #[test]
    fn windows_native_copy_application_cancellation_resumes_between_entries() {
        for completed in 0..3 {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            let name = staged.stage.name.clone();
            drop((staged, src));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            let result = app.apply(|_| {
                if inspect(&parent, &central, &name)?.permissions_completed == completed {
                    Err(io("cancelled permission application"))
                } else {
                    Ok(())
                }
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cancelled permission application")
            );
            assert_eq!(app.completed(), completed);
            assert!(!app.journal.failed);
            drop(app);
            let report = inspect(&parent, &central, &name).unwrap();
            assert!(!report.permission_pending);
            assert!(!report.completion_verified);
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            std::fs::remove_dir_all(root.path().join("source")).unwrap();
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            assert_eq!(app.completed(), 3);
        }
    }
    #[test]
    fn windows_native_copy_application_refuses_permission_drift_outside_pending_entry() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        drop((staged, src));
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        FAULT.with(|f| f.set(Some((19, Step::PermissionsChanged, false))));
        let result = app.apply(|_| Ok(()));
        FAULT.with(|f| f.set(None));
        assert!(result.is_err());
        drop(app);
        let changed = root.path().join("destination/copy/nested");
        super::super::tests::set_dacl(&changed, "D:P(A;OICI;FA;;;OW)(A;;FR;;;WD)");
        let file = owner(&changed);
        let unchanged = security::DestinationSecurity::capture(&file, false).unwrap();
        drop(file);
        let path = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&path).unwrap();
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
        unchanged.validate_parent(&owner(&changed)).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
            b"original data"
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
    }
    #[test]
    fn windows_native_copy_publication_refuses_changed_parent_policy_before_rename() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        drop((staged, src));
        let path = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&path).unwrap();
        super::super::tests::set_dacl(
            &root.path().join("destination"),
            "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
        );
        let unchanged = security::DestinationSecurity::capture(&parent, true).unwrap();
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(std::fs::read(path).unwrap(), before);
        unchanged.validate_parent(&parent).unwrap();
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(candidate)
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
    }
    #[test]
    fn windows_native_copy_application_records_require_publication_and_ordered_acknowledgements() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let bytes = read(&staged.journal.local).unwrap();
        let mut state = decode(&bytes).unwrap().state;
        assert!(
            state
                .check(&Record::Finalized {
                    count: 3,
                    digest: [0; 32]
                })
                .is_err()
        );
        assert!(
            state
                .check(&Record::SecurityApplyIntent { index: 0 })
                .is_err()
        );
        assert!(state.check(&Record::Published).is_err());
        state.apply(Record::PublicationIntent).unwrap();
        assert!(state.check(&Record::PublicationIntent).is_err());
        state.apply(Record::Published).unwrap();
        assert!(state.check(&Record::Published).is_err());
        for index in 0..3 {
            assert!(state.check(&Record::SecurityApplied { index }).is_err());
            assert!(
                state
                    .check(&Record::SecurityApplyIntent { index: index + 1 })
                    .is_err()
            );
            state.apply(Record::SecurityApplyIntent { index }).unwrap();
            assert!(state.check(&Record::SecurityApplyIntent { index }).is_err());
            assert!(
                state
                    .check(&Record::SecurityApplied { index: index + 1 })
                    .is_err()
            );
            state.apply(Record::SecurityApplied { index }).unwrap();
        }
        assert!(
            state
                .check(&Record::SecurityApplyIntent { index: 3 })
                .is_err()
        );
        assert!(state.check(&Record::SecurityApplied { index: 2 }).is_err());
        assert!(
            state
                .check(&Record::Finalized {
                    count: 3,
                    digest: [0; 32]
                })
                .is_err()
        );
        for index in 0..3 {
            let node = &state.nodes[index];
            let mut version = node.source.clone();
            version.identity = node.staged.as_ref().unwrap().candidate_identity;
            assert!(
                state
                    .check(&Record::FinalEntry {
                        index: index + 1,
                        version: Box::new(version.clone())
                    })
                    .is_err()
            );
            state
                .apply(Record::FinalEntry {
                    index,
                    version: Box::new(version),
                })
                .unwrap();
        }
        let digest = super::super::evidence_digest(&state.final_versions).unwrap();
        assert!(
            state
                .check(&Record::Finalized {
                    count: 3,
                    digest: [0; 32]
                })
                .is_err()
        );
        state.apply(Record::Finalized { count: 3, digest }).unwrap();
        assert!(state.completion_verified);
        assert!(
            state
                .check(&Record::Finalized {
                    count: 3,
                    digest: [0; 32]
                })
                .is_err()
        );
        let mut old = journal_records(&bytes);
        if let Record::Begin { header } = &mut old[0] {
            header.schema = 2;
        } else {
            unreachable!()
        }
        assert!(
            decode(&encode_records(&old)).is_err(),
            "superseded schema must be refused"
        );
    }
    #[test]
    fn windows_native_copy_journal_pairs_tree_records_and_reopens_without_writer() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let report = staged.inspect(&central).unwrap();
        assert_eq!(
            (
                report.entries,
                report.created,
                report.staged,
                report.verified
            ),
            (3, 3, 3, true)
        );
        super::super::tests::assert_owner_only(&staged.journal.local);
        super::super::tests::assert_owner_only(&staged.journal.central);
        staged.tree.verify_and_finalize(|_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(
                root.path()
                    .join("destination")
                    .join(&staged.stage.candidate_name)
                    .join("nested/file")
            )
            .unwrap(),
            b"original data"
        );
        let name = staged.stage.name.clone();
        drop(staged);
        assert!(inspect(&parent, &central, &name).unwrap().verified);
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
    }
    #[test]
    fn windows_native_copy_journal_rejects_overlap_and_budget_before_destination_mutation() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let inside = owner(&root.path().join("source/nested"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        assert!(stage(&src, &inside, &central).is_err());
        assert!(stage(&src, &parent, &inside).is_err());
        assert!(
            DurableStage::create(
                &src,
                &parent,
                &central,
                &DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap(),
                OsStr::new("source"),
                1,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("destination"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("central"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("source/nested"))
                .unwrap()
                .count(),
            1
        );
    }
    #[test]
    fn windows_native_copy_journal_rejects_torn_modified_and_reordered_frames() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let bytes = std::fs::read(
            root.path()
                .join("destination")
                .join(&staged.stage.name)
                .join(LOCAL),
        )
        .unwrap();
        assert!(decode(&bytes).unwrap().state.verified);
        for end in [0, 1, FRAME_HEADER - 1, bytes.len() - 1] {
            assert!(decode(&bytes[..end]).is_err(), "accepted truncation {end}");
        }
        let mut bad = bytes.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad.extend_from_slice(&bytes);
        assert!(decode(&bad).is_err());
        let first_end =
            FRAME_HEADER + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        assert!(decode(&bytes[first_end..]).is_err());
        // A self-consistent checksum cannot admit an event from another protocol.
        let payload = br#"{"event":"source-deleted"}"#;
        let mut bad = Vec::from(MAGIC.as_slice());
        bad.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bad.extend_from_slice(&0u64.to_le_bytes());
        bad.extend_from_slice(&[0; 32]);
        bad.extend_from_slice(&frame_hash(0, &[0; 32], payload));
        bad.extend_from_slice(payload);
        assert!(decode(&bad).is_err());
    }
    #[test]
    fn windows_native_copy_journal_flush_failures_retain_and_never_publish() {
        // Begin, root intent, root Created, root staged, child intent/Created,
        // payload acknowledgements, and terminal TreeVerified.
        for sequence in 0..=10 {
            for step in [Step::LocalFlushed, Step::CentralFlushed] {
                let root = tempfile::tempdir().unwrap();
                fixture(root.path());
                let src = source(&root.path().join("source"));
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                FAULT.with(|f| f.set(Some((sequence, step, false))));
                let result = stage(&src, &parent, &central);
                FAULT.with(|f| f.set(None));
                assert!(
                    matches!(result, Err(PlatformError::CleanupPending { .. })),
                    "sequence {sequence} {step:?}"
                );
                assert!(!root.path().join("destination/copy").exists());
                assert_eq!(
                    std::fs::read(root.path().join("source/nested/file")).unwrap(),
                    b"original data"
                );
                let name = stage_name(&root.path().join("destination"));
                let inspection = inspect(&parent, &central, &name);
                if step == Step::LocalFlushed {
                    assert!(inspection.is_err());
                } else {
                    let report = inspection.unwrap();
                    assert_eq!(report.verified, sequence == 10);
                }
                if sequence <= 2 {
                    let candidates: Vec<_> = std::fs::read_dir(root.path().join("destination"))
                        .unwrap()
                        .filter_map(|e| {
                            let p = e.unwrap().path();
                            p.to_string_lossy().ends_with(".candidate").then_some(p)
                        })
                        .collect();
                    assert_eq!(candidates.len(), usize::from(sequence == 2));
                    for candidate in candidates {
                        assert_eq!(std::fs::read_dir(candidate).unwrap().count(), 0);
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "child process selected by interrupted staging test"]
    fn windows_native_copy_journal_exit_child() {
        let root = std::env::var_os("OTTRIN_COPY_JOURNAL_CHILD").expect("owned child fixture");
        let root = Path::new(&root);
        let sequence = std::env::var("OTTRIN_COPY_JOURNAL_SEQUENCE")
            .unwrap()
            .parse()
            .unwrap();
        let step = match std::env::var("OTTRIN_COPY_JOURNAL_STEP").unwrap().as_str() {
            "local" => Step::LocalFlushed,
            "central" => Step::CentralFlushed,
            "renamed" => Step::PublicationRenamed,
            "published-flushed" => Step::PublicationFlushed,
            "creation-restored" => Step::CreationRestored,
            "creation-flushed" => Step::CreationFlushed,
            "changed" => Step::PermissionsChanged,
            "permissions-flushed" => Step::PermissionsFlushed,
            "readonly-writable" => Step::ReadonlyWritable,
            "readonly-restored" => Step::ReadonlyRestored,
            "readonly-flushed" => Step::ReadonlyFlushed,
            "undo-readers-released" => Step::UndoReadersReleased,
            "undo-renamed" => Step::UndoRenamed,
            "undo-flushed" => Step::UndoFlushed,
            other => panic!("Unknown owned exit boundary: {other}"),
        };
        if std::env::var_os("OTTRIN_COPY_UNDO_CHILD").is_some() {
            let parent = owner(&root.join("destination"));
            let central = owner(&root.join("central"));
            let name = stage_name(&root.join("destination"));
            FAULT.with(|f| f.set(Some((sequence, step, true))));
            let _ = undo_completed_copy(&parent, &central, &name, |_| Ok(()));
            panic!("Undo child did not reach exit boundary");
        }
        if std::env::var_os("OTTRIN_COPY_PLAN_RECOVERY_CHILD").is_some() {
            let parent = owner(&root.join("destination"));
            let central = owner(&root.join("central"));
            let name = stage_name(&root.join("destination"));
            FAULT.with(|f| f.set(Some((sequence, step, true))));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            let _ = app.apply(|_| Ok(()));
            panic!("source-free child did not reach crash boundary");
        }
        let src = source(&root.join("source"));
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        FAULT.with(|f| f.set(Some((sequence, step, true))));
        let mut staged = stage(&src, &parent, &central).unwrap();
        let _ = staged.prepare_destination_security(|| Ok(()));
        let name = staged.stage.name.clone();
        drop((staged, src));
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        let _ = app.apply(|_| Ok(()));
        panic!("child did not reach crash boundary");
    }
    #[test]
    fn windows_native_copy_journal_process_exit_retains_unpublished_staging() {
        for sequence in [1, 2, 6, 10, 11, 12, 13, 14, 15] {
            for step in ["local", "central"] {
                let root = tempfile::tempdir().unwrap();
                fixture(root.path());
                let status=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","copy_transaction::windows::journal::tests::windows_native_copy_journal_exit_child","--ignored","--nocapture"])
                .env("OTTRIN_COPY_JOURNAL_CHILD",root.path()).env("OTTRIN_COPY_JOURNAL_SEQUENCE",sequence.to_string()).env("OTTRIN_COPY_JOURNAL_STEP",step).status().unwrap();
                assert_eq!(status.code(), Some(86));
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                let name = stage_name(&root.path().join("destination"));
                let report = inspect(&parent, &central, &name);
                if step == "local" {
                    assert!(report.is_err());
                } else {
                    assert_eq!(report.unwrap().verified, sequence >= 10);
                }
                assert_eq!(
                    std::fs::read(root.path().join("source/nested/file")).unwrap(),
                    b"original data"
                );
                assert!(!root.path().join("destination/copy").exists());
                let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(()));
                assert_eq!(retained.is_ok(), step == "central" && sequence >= 10);
                if let Ok(retained) = retained {
                    if sequence >= 11 {
                        std::fs::remove_dir_all(root.path().join("source")).unwrap();
                    }
                    let plan = retained.plan_destination_security(|| Ok(())).unwrap();
                    assert_eq!(plan.entry_count(), 3);
                }
            }
        }
    }
    #[test]
    fn windows_native_copy_journal_rejects_rebound_and_damaged_mirror_without_repair() {
        for replace in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            let mirror = root
                .path()
                .join("central")
                .join(staged.journal.header.central_name());
            drop(staged);
            let original = std::fs::read(&mirror).unwrap();
            if replace {
                let held = mirror.with_extension("retained");
                std::fs::rename(&mirror, &held).unwrap();
                std::fs::write(&mirror, &original).unwrap();
            } else {
                let mut changed = original.clone();
                *changed.last_mut().unwrap() ^= 1;
                std::fs::write(&mirror, changed).unwrap();
            }
            let before = std::fs::read(&mirror).unwrap();
            assert!(inspect(&parent, &central, &name).is_err());
            assert_eq!(std::fs::read(&mirror).unwrap(), before);
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
        }
    }
    #[test]
    fn windows_native_copy_claim_survives_stage_drop_and_replaces_a_released_reservation() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let src = source(&root.path().join("source"));
        let mut first = stage(&src, &parent, &central).unwrap();
        let name = first.stage.name.clone();
        let binding = first.journal.header.claim.clone();
        drop(first.prepare_destination_security(|| Ok(())).unwrap());
        let before =
            std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap();
        assert!(!root.path().join("destination/copy").exists());
        assert!(matches!(
            stage(&src, &parent, &central),
            Err(PlatformError::DestinationExists(_))
        ));
        assert_eq!(
            std::fs::read(root.path().join("destination").join(&name).join(LOCAL)).unwrap(),
            before
        );
        let incoming = root.path().join("incoming/copy");
        std::fs::create_dir_all(&incoming).unwrap();
        std::fs::write(incoming.join("file"), b"other copy").unwrap();
        let destination = root.path().join("destination");
        assert!(
            super::super::dispatch::copy_at(
                &incoming,
                &destination,
                crate::ConflictAction::Skip,
                &central,
                100,
                |_| Ok(())
            )
            .unwrap()
            .is_none()
        );
        let renamed = super::super::dispatch::copy_at(
            &incoming,
            &destination,
            crate::ConflictAction::Rename,
            &central,
            100,
            |_| Ok(()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            std::fs::read(destination.join("copy (1)/file")).unwrap(),
            b"other copy"
        );
        crate::PlatformOps::undo_copy_with_receipt(&crate::DefaultPlatform, &renamed).unwrap();
        drop(first);
        assert!(matches!(
            stage(&src, &parent, &central),
            Err(PlatformError::DestinationExists(_))
        ));
        let mut publication =
            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        publication.apply(|_| Ok(())).unwrap();
        drop(publication);
        undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(!root.path().join("destination/copy").exists());
        let reused = stage(&src, &parent, &central).unwrap();
        assert_ne!(reused.journal.header.claim, binding);
        assert_ne!(
            reused.journal.header.claim.identities()[4],
            binding.identities()[4]
        );
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
    }

    #[test]
    fn windows_native_copy_claim_name_keys_separate_short_long_and_case_collisions() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        // The first two would collide if encoded short keys shared a directory
        // with unchanged long names. Exercise both sides of the 12-unit boundary
        // and the NTFS component limit without truncating or hashing names.
        let names = [
            "copy".to_owned(),
            "copy.ottrin-claim".to_owned(),
            "a".repeat(12),
            "a".repeat(13),
            "z".repeat(255),
            "Long native reservation record.txt".to_owned(),
            "LONGNA~1.TXT".to_owned(),
        ];
        let mut claims = Vec::new();
        for name in &names {
            let transaction = super::super::super::random_transaction_id().unwrap();
            let claim =
                claims::Lease::acquire(&parent, &central, OsStr::new(name), &transaction).unwrap();
            claim.validate(false).unwrap();
            claims.push(claim);
        }
        for name in &names {
            let transaction = super::super::super::random_transaction_id().unwrap();
            assert!(matches!(
                claims::Lease::acquire(
                    &parent,
                    &central,
                    OsStr::new(&name.to_uppercase()),
                    &transaction
                ),
                Err(PlatformError::DestinationExists(_))
            ));
        }
        for claim in claims {
            let error = claim.abort_unstarted(PlatformError::Io("cancelled before staging".into()));
            assert!(matches!(error, PlatformError::Io(_)));
        }
        for name in names {
            let transaction = super::super::super::random_transaction_id().unwrap();
            let claim =
                claims::Lease::acquire(&parent, &central, OsStr::new(&name), &transaction).unwrap();
            claim.validate(false).unwrap();
            let error = claim.abort_unstarted(PlatformError::Io("cancelled before staging".into()));
            assert!(matches!(error, PlatformError::Io(_)));
        }
    }

    #[test]
    fn windows_native_copy_journal_uncertain_owner_cannot_retry_or_repair() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let transaction = super::super::super::random_transaction_id().unwrap();
        let claim =
            claims::Lease::acquire(&parent, &central, OsStr::new("copy"), &transaction).unwrap();
        let stage = namespace::PrivateStage::for_transaction(&parent, &transaction).unwrap();
        let mut journal = Journal::create(
            &stage,
            &central,
            &DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap(),
            OsStr::new("source"),
            &mut Some(claim),
        )
        .unwrap();
        let src = source(&root.path().join("source"));
        let record = Record::CreateIntent {
            index: 0,
            parent: None,
            name: stage.candidate_name.encode_wide().collect(),
            source: super::super::version(&src).unwrap(),
        };
        FAULT.with(|f| f.set(Some((1, Step::LocalFlushed, false))));
        let failed = journal.persist(record.clone());
        FAULT.with(|f| f.set(None));
        assert!(failed.is_err());
        let lengths = (
            journal.local.metadata().unwrap().len(),
            journal.central.metadata().unwrap().len(),
        );
        assert_ne!(lengths.0, lengths.1);
        assert!(journal.persist(record).is_err());
        assert_eq!(
            lengths,
            (
                journal.local.metadata().unwrap().len(),
                journal.central.metadata().unwrap().len()
            )
        );
        assert!(
            !root
                .path()
                .join("destination")
                .join(&stage.candidate_name)
                .exists()
        );
    }
    #[test]
    fn windows_native_copy_journal_acknowledges_planned_link_type_transition() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let target = root.path().join("target");
        std::fs::write(&target, b"never staged through link").unwrap();
        let link = root.path().join("link");
        std::os::windows::fs::symlink_file(&target, &link).unwrap();
        let src = source(&link);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let report = staged.inspect(&central).unwrap();
        assert_eq!(
            (
                report.entries,
                report.created,
                report.staged,
                report.verified
            ),
            (1, 1, 1, true)
        );
        assert_eq!(
            std::fs::read_link(
                root.path()
                    .join("destination")
                    .join(&staged.stage.candidate_name)
            )
            .unwrap(),
            target
        );
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"never staged through link"
        );
        let node = &staged.journal.state.nodes[0];
        assert!(
            !node
                .created
                .unwrap()
                .same_type(node.staged.as_ref().unwrap().candidate_identity)
        );
        assert!(
            node.created
                .unwrap()
                .same_object(node.staged.as_ref().unwrap().candidate_identity)
        );
    }
    #[test]
    fn windows_native_copy_retained_validation_needs_no_source_and_holds_read_only_handles() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        std::fs::write(
            root.path().join("source/nested/file:note:$DATA"),
            b"named data",
        )
        .unwrap();
        for name in [
            OsString::from_wide(&[0xd800]),
            OsString::from("🦀"),
            OsString::from("\u{e000}"),
        ] {
            std::fs::write(root.path().join("source").join(name), b"raw-name data").unwrap();
        }
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = root
            .path()
            .join("destination")
            .join(&staged.stage.candidate_name);
        drop((staged, src));
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        let validated = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
        assert_eq!(validated.entry_count(), 6);
        validated.revalidate(|_| Ok(())).unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .open(candidate.join("nested/file"))
                .is_err()
        );
        assert_eq!(
            std::fs::read(candidate.join("nested/file")).unwrap(),
            b"original data"
        );
        assert_eq!(
            std::fs::read(candidate.join("nested/file:note:$DATA")).unwrap(),
            b"named data"
        );
        drop(validated);
        assert!(
            OpenOptions::new()
                .write(true)
                .open(candidate.join("nested/file"))
                .is_ok()
        );
    }
    #[test]
    fn windows_native_copy_retained_validation_refuses_active_writer_and_unverified_journal() {
        for interrupted in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            if interrupted {
                FAULT.with(|f| f.set(Some((6, Step::CentralFlushed, false))));
                let result = stage(&src, &parent, &central);
                FAULT.with(|f| f.set(None));
                assert!(result.is_err());
                let name = stage_name(&root.path().join("destination"));
                assert!(!inspect(&parent, &central, &name).unwrap().verified);
                assert!(RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_err());
            } else {
                let staged = stage(&src, &parent, &central).unwrap();
                let name = staged.stage.name.clone();
                assert!(staged.inspect(&central).unwrap().verified);
                assert!(RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_err());
                drop(staged);
                assert!(RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_ok());
            }
        }
    }
    #[test]
    fn windows_native_copy_retained_validation_hashes_streams_even_with_restored_metadata() {
        for named in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            std::fs::write(
                root.path().join("source/nested/file:note:$DATA"),
                b"named data",
            )
            .unwrap();
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            let candidate = root
                .path()
                .join("destination")
                .join(&staged.stage.candidate_name);
            let evidence = staged
                .journal
                .state
                .nodes
                .last()
                .unwrap()
                .staged
                .clone()
                .unwrap();
            drop(staged);
            let file = candidate.join("nested/file");
            let changed = if named {
                candidate.join("nested/file:note:$DATA")
            } else {
                file.clone()
            };
            std::fs::write(
                &changed,
                if named {
                    b"changed!!!".as_slice()
                } else {
                    b"tampered data".as_slice()
                },
            )
            .unwrap();
            let handle = owner(&file);
            evidence.apply_final_metadata(&handle).unwrap();
            drop(handle);
            let error = RetainedStage::open(&parent, &central, &name, |_| Ok(()))
                .err()
                .expect("altered stream must be refused");
            assert!(
                error.to_string().contains("stream contents changed"),
                "expected content hash refusal, got: {error}"
            );
            assert_eq!(
                std::fs::read(&changed).unwrap(),
                if named {
                    b"changed!!!".as_slice()
                } else {
                    b"tampered data".as_slice()
                }
            );
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
        }
    }
    #[test]
    fn windows_native_copy_retained_validation_refuses_membership_rebinding_and_extra_records() {
        for mode in 0..5 {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            let transaction = root.path().join("destination").join(&name);
            let candidate = root
                .path()
                .join("destination")
                .join(&staged.stage.candidate_name);
            drop(staged);
            match mode {
                0 => std::fs::write(candidate.join("extra"), b"unexpected data").unwrap(),
                1 => std::fs::remove_file(candidate.join("nested/file")).unwrap(),
                2 => {
                    std::fs::rename(
                        candidate.join("nested/file"),
                        root.path().join("retained-original"),
                    )
                    .unwrap();
                    std::fs::write(candidate.join("nested/file"), b"original data").unwrap();
                }
                3 => std::fs::write(transaction.join("extra"), b"unexpected transaction entry")
                    .unwrap(),
                _ => std::fs::write(
                    candidate.join("nested/file:extra:$DATA"),
                    b"extra named stream",
                )
                .unwrap(),
            }
            assert!(
                RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_err(),
                "mode {mode}"
            );
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            assert!(transaction.join(LOCAL).exists());
            assert!(candidate.exists());
            if mode == 0 {
                assert_eq!(
                    std::fs::read(candidate.join("extra")).unwrap(),
                    b"unexpected data"
                );
            }
            if mode == 2 {
                assert_eq!(
                    std::fs::read(root.path().join("retained-original")).unwrap(),
                    b"original data"
                );
            }
        }
    }
    #[test]
    fn windows_native_copy_retained_validation_never_follows_a_dangling_link() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let target = root.path().join("target");
            let link = root.path().join("link");
            if directory {
                std::fs::create_dir(&target).unwrap();
                std::fs::write(target.join("file"), b"target bytes").unwrap();
                std::os::windows::fs::symlink_dir(&target, &link).unwrap();
            } else {
                std::fs::write(&target, b"target bytes").unwrap();
                std::os::windows::fs::symlink_file(&target, &link).unwrap();
            }
            let src = source(&link);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            drop((staged, src));
            if directory {
                std::fs::remove_dir(&link).unwrap();
                std::fs::remove_dir_all(&target).unwrap();
            } else {
                std::fs::remove_file(&link).unwrap();
                std::fs::remove_file(&target).unwrap();
            }
            let validated = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
            assert_eq!(validated.entry_count(), 1);
            validated.revalidate(|_| Ok(())).unwrap();
            assert!(!target.exists());
        }
    }
    #[test]
    fn windows_native_copy_retained_validation_cancellation_releases_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let original = vec![93; 200_000];
        std::fs::write(root.path().join("source/nested/file"), &original).unwrap();
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = root
            .path()
            .join("destination")
            .join(&staged.stage.candidate_name);
        drop(staged);
        let mut cancelled = false;
        assert!(
            RetainedStage::open(&parent, &central, &name, |bytes| if bytes > 0 {
                cancelled = true;
                Err(io("cancelled retained scan"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert!(cancelled);
        assert_eq!(
            std::fs::read(candidate.join("nested/file")).unwrap(),
            original
        );
        assert!(
            OpenOptions::new()
                .write(true)
                .open(candidate.join("nested/file"))
                .is_ok()
        );
        assert!(RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_ok());
    }
    #[test]
    fn windows_native_copy_private_security_records_and_rejects_permission_drift() {
        for mode in 0..6 {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            let candidate = root
                .path()
                .join("destination")
                .join(&staged.stage.candidate_name);
            let dir = root.path().join("destination").join(&name);
            let mirror = root
                .path()
                .join("central")
                .join(staged.journal.header.central_name());
            let file = candidate.join("nested/file");
            let recorded = staged
                .journal
                .state
                .nodes
                .last()
                .unwrap()
                .security
                .as_ref()
                .unwrap();
            let raw = serde_json::to_value(recorded).unwrap();
            let flags = raw["dacl"][9].as_u64().unwrap();
            let changed = match mode {
                0 => candidate.clone(),
                1 => file.clone(),
                2 => dir.clone(),
                3 => dir.join(LOCAL),
                4 => mirror.clone(),
                _ => file.clone(),
            };
            drop(staged);
            let dacl = if mode == 5 {
                if flags == 0 {
                    "D:P(A;OICI;FA;;;OW)"
                } else {
                    "D:P(A;;FA;;;OW)"
                }
            } else {
                "D:P(A;OICI;FA;;;OW)(A;;FR;;;WD)"
            };
            super::super::tests::set_dacl(&changed, dacl);
            let creator = security::current_owner().unwrap();
            let changed_handle = owner(&changed);
            if mode == 5 {
                assert!(security::PrivateSecurity::capture(&changed_handle, &creator).is_ok());
            } else {
                assert!(security::PrivateSecurity::capture(&changed_handle, &creator).is_err());
            }
            drop(changed_handle);
            assert!(
                RetainedStage::open(&parent, &central, &name, |_| Ok(())).is_err(),
                "mode {mode}"
            );
            assert_eq!(std::fs::read(&file).unwrap(), b"original data");
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            // Failed inspection must not repair permissions.
            let changed_handle = owner(&changed);
            if mode != 5 {
                assert!(security::PrivateSecurity::capture(&changed_handle, &creator).is_err());
            }
        }
    }
    #[test]
    fn windows_native_copy_private_security_drift_during_validation_is_retained() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let file = root
            .path()
            .join("destination")
            .join(&staged.stage.candidate_name)
            .join("nested/file");
        drop(staged);
        let mut changed = false;
        let result = RetainedStage::open(&parent, &central, &name, |bytes| {
            if bytes > 0 && !changed {
                super::super::tests::set_dacl(&file, "D:P(A;OICI;FA;;;OW)(A;;FR;;;WD)");
                changed = true;
            }
            Ok(())
        });
        assert!(changed);
        let error = result.err().expect("permission drift must be refused");
        assert!(error.to_string().contains("security"), "{error}");
        assert_eq!(std::fs::read(&file).unwrap(), b"original data");
    }
    #[test]
    fn windows_native_copy_private_security_staging_drift_never_records_success() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source/nested/file"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut changed = None;
        let result = DurableStage::create(
            &src,
            &parent,
            &central,
            &DestinationAdmission::creation(&parent, OsStr::new("copy")).unwrap(),
            OsStr::new("file"),
            100,
            |bytes| {
                if bytes > 0 && changed.is_none() {
                    let file = std::fs::read_dir(root.path().join("destination"))
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .find(|p| p.to_string_lossy().ends_with(".candidate"))
                        .unwrap();
                    super::super::tests::set_dacl(&file, "D:P(A;OICI;FA;;;OW)(A;;FR;;;WD)");
                    changed = Some(file);
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
        let file = changed.expect("injected security change");
        assert_eq!(std::fs::read(&file).unwrap(), b"original data");
        let name = stage_name(&root.path().join("destination"));
        let report = inspect(&parent, &central, &name).unwrap();
        assert_eq!(
            (report.created, report.staged, report.verified),
            (1, 0, false)
        );
        assert!(!root.path().join("destination/copy").exists());
    }
    #[test]
    fn windows_native_copy_private_security_rejects_foreign_creator_and_malformed_records() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let header = &staged.journal.header;
        assert!(header.valid());
        for field in ["stage_security", "local_security", "central_security"] {
            for bad in [
                serde_json::json!([]),
                serde_json::json!([1]),
                serde_json::json!(vec![0; 28]),
            ] {
                let mut value = serde_json::to_value(header).unwrap();
                value[field]["dacl"] = bad;
                let header: Header = serde_json::from_value(value).unwrap();
                assert!(!header.valid());
                assert!(
                    State::default()
                        .apply(Record::Begin {
                            header: Box::new(header)
                        })
                        .is_err()
                );
            }
        }
        let mut old = header.clone();
        old.schema = 1;
        assert!(!old.valid());
        let mut foreign = header.clone();
        foreign.creator[8] ^= 1;
        assert!(!foreign.valid());
        let mut missing = serde_json::to_value(header).unwrap();
        missing.as_object_mut().unwrap().remove("stage_security");
        assert!(serde_json::from_value::<Header>(missing).is_err());
    }
    #[test]
    fn windows_native_copy_tree_security_live_and_retained_plans_agree_without_sources() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        super::super::tests::set_dacl(
            &root.path().join("destination"),
            "D:P(A;OICI;FA;;;CO)(A;;FA;;;OW)(A;OICI;FR;;;WD)",
        );
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let plan = staged.prepare_destination_security(|| Ok(())).unwrap();
        assert_eq!(plan.entry_count(), 3);
        let expected = serde_json::to_value(plan.evidence()).unwrap();
        assert_ne!(
            expected["entries"][2]["private"],
            expected["entries"][2]["final_security"]
        );
        drop((plan, staged, src));
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
        let plan = retained.plan_destination_security(|| Ok(())).unwrap();
        assert_eq!(serde_json::to_value(plan.evidence()).unwrap(), expected);
        plan.validate().unwrap();
        assert!(!root.path().join("destination/copy").exists());
        assert!(!root.path().join("source").exists());
    }
    #[test]
    fn windows_native_copy_tree_security_plans_dangling_file_and_directory_link_leaves() {
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let missing = root.path().join("missing");
            let link = root.path().join("link");
            if directory {
                std::os::windows::fs::symlink_dir(&missing, &link).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&missing, &link).unwrap();
            }
            let src = source(&link);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            let plan = staged.prepare_destination_security(|| Ok(())).unwrap();
            assert_eq!(plan.entry_count(), 1);
            let expected = serde_json::to_value(plan.evidence()).unwrap();
            drop((plan, staged, src));
            let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
            let plan = retained.plan_destination_security(|| Ok(())).unwrap();
            assert_eq!(serde_json::to_value(plan.evidence()).unwrap(), expected);
            assert!(!missing.exists());
        }
    }
    #[test]
    fn windows_native_copy_tree_security_plan_is_durable_idempotent_and_refuses_reinterpretation() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        assert!(!staged.inspect(&central).unwrap().security_planned);
        let plan = staged.prepare_destination_security(|| Ok(())).unwrap();
        let expected = plan.evidence();
        drop(plan);
        assert!(staged.inspect(&central).unwrap().security_planned);
        let length = staged.journal.length;
        assert_eq!(
            staged
                .prepare_destination_security(|| Ok(()))
                .unwrap()
                .evidence(),
            expected
        );
        assert_eq!(staged.journal.length, length);
        let name = staged.stage.name.clone();
        super::super::tests::set_dacl(
            &root.path().join("destination"),
            "D:P(A;OICI;FA;;;OW)(A;OICI;FRFX;;;WD)",
        );
        assert!(staged.prepare_destination_security(|| Ok(())).is_err());
        assert_eq!(staged.journal.length, length);
        drop((staged, src));
        std::fs::remove_dir_all(root.path().join("source")).unwrap();
        let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
        assert!(retained.plan_destination_security(|| Ok(())).is_err());
        assert!(!root.path().join("destination/copy").exists());
    }
    #[test]
    fn windows_native_copy_tree_security_plan_flush_faults_keep_private_data_and_both_versions() {
        for sequence in 11..=15 {
            for step in [Step::LocalFlushed, Step::CentralFlushed] {
                let root = tempfile::tempdir().unwrap();
                fixture(root.path());
                let src = source(&root.path().join("source"));
                let parent = owner(&root.path().join("destination"));
                let central = owner(&root.path().join("central"));
                let mut staged = stage(&src, &parent, &central).unwrap();
                FAULT.with(|f| f.set(Some((sequence, step, false))));
                let result = staged.prepare_destination_security(|| Ok(()));
                FAULT.with(|f| f.set(None));
                assert!(matches!(result, Err(PlatformError::CleanupPending { .. })));
                let lengths = (
                    staged.journal.local.metadata().unwrap().len(),
                    staged.journal.central.metadata().unwrap().len(),
                );
                assert!(staged.prepare_destination_security(|| Ok(())).is_err());
                assert_eq!(
                    lengths,
                    (
                        staged.journal.local.metadata().unwrap().len(),
                        staged.journal.central.metadata().unwrap().len()
                    )
                );
                let name = staged.stage.name.clone();
                let candidate = root
                    .path()
                    .join("destination")
                    .join(&staged.stage.candidate_name);
                drop((staged, src));
                std::fs::remove_dir_all(root.path().join("source")).unwrap();
                let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(()));
                if step == Step::LocalFlushed {
                    assert!(retained.is_err());
                } else {
                    let retained = retained.unwrap();
                    assert_eq!(
                        inspect(&parent, &central, &name).unwrap().security_planned,
                        sequence == 15
                    );
                    assert_eq!(
                        retained
                            .plan_destination_security(|| Ok(()))
                            .unwrap()
                            .entry_count(),
                        3
                    );
                }
                assert_eq!(
                    std::fs::read(candidate.join("nested/file")).unwrap(),
                    b"original data"
                );
                assert!(!root.path().join("destination/copy").exists());
            }
        }
    }
    #[test]
    fn windows_native_copy_tree_security_cached_acl_is_not_native_authority_even_with_valid_hashes()
    {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let name = staged.stage.name.clone();
        let local = root.path().join("destination").join(&name).join(LOCAL);
        let mirror = root
            .path()
            .join("central")
            .join(staged.journal.header.central_name());
        drop((staged, src));
        let bytes = std::fs::read(&local).unwrap();
        let mut at = 0;
        let mut sequence = 0;
        let mut previous = [0u8; 32];
        let mut patched = Vec::new();
        let mut state = State::default();
        let mut changed = false;
        while at < bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            let mut value: serde_json::Value =
                serde_json::from_slice(&bytes[at + FRAME_HEADER..at + FRAME_HEADER + size])
                    .unwrap();
            if value["event"] == "security-plan-entry" && value["index"] == 0 {
                let old = value["entry"]["final_security"]["dacl"][12]
                    .as_u64()
                    .unwrap();
                value["entry"]["final_security"]["dacl"][12] = serde_json::json!(old ^ 1);
                changed = true;
            }
            let mut record: Record = serde_json::from_value(value).unwrap();
            if let Record::SecurityPlanComplete { digest, .. } = &mut record {
                *digest = state.planning.as_ref().unwrap().evidence.digest().unwrap();
            }
            let payload = serde_json::to_vec(&record).unwrap();
            let hash = frame_hash(sequence, &previous, &payload);
            patched.extend_from_slice(MAGIC);
            patched.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            patched.extend_from_slice(&sequence.to_le_bytes());
            patched.extend_from_slice(&previous);
            patched.extend_from_slice(&hash);
            patched.extend_from_slice(&payload);
            state.apply(record).unwrap();
            sequence += 1;
            previous = hash;
            at += FRAME_HEADER + size;
        }
        assert!(changed);
        assert!(
            decode(&patched).unwrap().state.planned.is_some(),
            "structurally valid cached evidence must still require native rederivation"
        );
        std::fs::write(&local, &patched).unwrap();
        std::fs::write(&mirror, &patched).unwrap();
        let retained = RetainedStage::open(&parent, &central, &name, |_| Ok(())).unwrap();
        let error = retained
            .plan_destination_security(|| Ok(()))
            .err()
            .expect("forged cached ACL must not authorize native permissions");
        assert!(error.to_string().contains("native inheritance"), "{error}");
        for entry in &retained.tree.entries {
            super::super::tests::assert_owner_only(&entry.handle);
        }
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(std::fs::read(&local).unwrap(), patched);
    }
    fn journal_records(bytes: &[u8]) -> Vec<Record> {
        let mut at = 0;
        let mut records = Vec::new();
        while at < bytes.len() {
            let size = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            records.push(
                serde_json::from_slice(&bytes[at + FRAME_HEADER..at + FRAME_HEADER + size])
                    .unwrap(),
            );
            at += FRAME_HEADER + size;
        }
        records
    }
    fn encode_records(records: &[Record]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut previous = [0; 32];
        for (sequence, record) in records.iter().enumerate() {
            let payload = serde_json::to_vec(record).unwrap();
            assert!(payload.len() <= MAX_FRAME);
            let hash = frame_hash(sequence as u64, &previous, &payload);
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(sequence as u64).to_le_bytes());
            bytes.extend_from_slice(&previous);
            bytes.extend_from_slice(&hash);
            bytes.extend_from_slice(&payload);
            previous = hash;
        }
        bytes
    }
    #[test]
    fn windows_native_copy_streamed_plan_requires_ordered_complete_seal() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        drop(staged.prepare_destination_security(|| Ok(())).unwrap());
        let bytes = std::fs::read(
            root.path()
                .join("destination")
                .join(&staged.stage.name)
                .join(LOCAL),
        )
        .unwrap();
        let records = journal_records(&bytes);
        assert_eq!(records.len(), 16);
        for end in 11..16 {
            let state = decode(&encode_records(&records[..end])).unwrap().state;
            assert!(state.verified);
            assert!(state.planned.is_none());
            if end > 11 {
                assert_eq!(state.planning.as_ref().unwrap().evidence.len(), end - 12);
            }
        }
        for mode in 0..5 {
            let mut bad = records.clone();
            match mode {
                0 => {
                    bad.swap(12, 13);
                }
                1 => {
                    bad.remove(13);
                }
                2 => {
                    bad.insert(13, bad[12].clone());
                }
                3 => {
                    if let Record::SecurityPlanComplete { digest, .. } = &mut bad[15] {
                        digest[0] ^= 1;
                    }
                }
                _ => {
                    bad.push(bad[15].clone());
                }
            }
            assert!(decode(&encode_records(&bad)).is_err(), "mode {mode}");
        }
        assert!(decode(&bytes).unwrap().state.planned.is_some());
    }
    #[test]
    fn windows_native_copy_streamed_plan_codec_exceeds_old_single_frame_without_native_authority() {
        // Synthetic recorded graph: exercises framing/state/resource limits,
        // not real large-tree file copying or permission application.
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let src = source(&root.path().join("source"));
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let mut staged = stage(&src, &parent, &central).unwrap();
        let native = staged
            .prepare_destination_security(|| Ok(()))
            .unwrap()
            .evidence();
        let actual_root = &staged.journal.state.nodes[0];
        let actual_file = &staged.journal.state.nodes[2];
        let count = 2001usize;
        let mut records = vec![Record::Begin {
            header: Box::new(staged.journal.header.clone()),
        }];
        let mut planned = Vec::new();
        for index in 0..count {
            let template = if index == 0 { actual_root } else { actual_file };
            let mut source = template.source.clone();
            let mut created = template.created.unwrap();
            let mut payload = template.staged.clone().unwrap();
            let parent = if index == 0 { None } else { Some(0) };
            let name = if index == 0 {
                template.name.clone()
            } else {
                format!("file-{index:05}").encode_utf16().collect()
            };
            if index != 0 {
                let fake = |id: WindowsStableIdentity, marker: u8| {
                    let mut value = serde_json::to_value(id).unwrap();
                    let mut bytes = [marker; 16];
                    bytes[..8].copy_from_slice(&(index as u64).to_le_bytes());
                    value["file_id"] = serde_json::json!(bytes);
                    serde_json::from_value(value).unwrap()
                };
                source.identity = fake(source.identity, 0xab);
                created = fake(created, 0xcd);
                payload.source = source.clone();
                payload.candidate_identity = created;
            }
            records.push(Record::CreateIntent {
                index,
                parent,
                name: name.clone(),
                source,
            });
            records.push(Record::Created {
                index,
                identity: created,
                security: template.security.clone().unwrap(),
            });
            records.push(Record::EntryStaged {
                index,
                evidence: payload.clone(),
            });
            let mut entry = serde_json::to_value(
                native
                    .entries()
                    .nth(if index == 0 { 0 } else { 2 })
                    .unwrap(),
            )
            .unwrap();
            entry["parent"] = serde_json::json!(parent);
            entry["name"] = serde_json::json!(name);
            entry["identity"] = serde_json::to_value(payload.candidate_identity).unwrap();
            if index != 0 {
                let mut acl: Vec<u8> =
                    serde_json::from_value(entry["final_security"]["dacl"].clone()).unwrap();
                acl.resize(6000, 0);
                acl[2..4].copy_from_slice(&6000u16.to_le_bytes());
                entry["final_security"]["dacl"] = serde_json::json!(acl);
            }
            planned.push(serde_json::from_value::<security::PlannedEntry>(entry).unwrap());
        }
        records.push(Record::TreeVerified { count });
        let mut header = serde_json::to_value(native.header()).unwrap();
        header["count"] = serde_json::json!(count);
        let header: security::SecurityPlanHeader = serde_json::from_value(header).unwrap();
        let mut complete = security::TreeSecurityEvidence::from_header(header.clone());
        records.push(Record::SecurityPlanBegin {
            header: Box::new(header),
        });
        for (index, entry) in planned.into_iter().enumerate() {
            complete.push(entry.clone());
            records.push(Record::SecurityPlanEntry {
                index,
                entry: Box::new(entry),
            });
        }
        assert!(serde_json::to_vec(&complete).unwrap().len() > MAX_FRAME);
        records.push(Record::SecurityPlanComplete {
            count,
            digest: complete.digest().unwrap(),
        });
        let bytes = encode_records(&records);
        assert!(bytes.len() as u64 <= MAX_LOG);
        let state = decode(&bytes).unwrap().state;
        assert_eq!(state.nodes.len(), count);
        assert_eq!(state.planned.unwrap(), complete);
    }
    struct ReadonlyCleanup(Vec<std::path::PathBuf>);
    impl Drop for ReadonlyCleanup {
        fn drop(&mut self) {
            for path in &self.0 {
                if let Ok(metadata) = std::fs::metadata(path) {
                    let mut permissions = metadata.permissions();
                    // Windows-only fixture cleanup clears the DOS readonly bit.
                    #[allow(clippy::permissions_set_readonly_false)]
                    permissions.set_readonly(false);
                    if let Err(error) = std::fs::set_permissions(path, permissions) {
                        eprintln!("owned readonly fixture cleanup: {error}");
                    }
                }
            }
        }
    }
    fn readonly_fixture(path: &Path, nested: bool) -> ReadonlyCleanup {
        fixture(path);
        let source = path.join("source/nested/file");
        let mut permissions = std::fs::metadata(&source).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&source, permissions).unwrap();
        ReadonlyCleanup(vec![
            source,
            path.join(if nested {
                "destination/copy/nested/file"
            } else {
                "destination/copy"
            }),
        ])
    }
    #[test]
    fn windows_native_copy_readonly_root_and_child_publish_and_reopen_with_attributes_intact() {
        for nested in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut cleanup = readonly_fixture(root.path(), nested);
            let source_path = root.path().join(if nested {
                "source"
            } else {
                "source/nested/file"
            });
            let src = source(&source_path);
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let staged = stage(&src, &parent, &central).unwrap();
            let name = staged.stage.name.clone();
            cleanup.0.push(fixture_data_path(
                root.path()
                    .join("destination")
                    .join(&staged.stage.candidate_name),
                nested,
            ));
            drop((staged, src));
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            assert_eq!(app.completed(), if nested { 3 } else { 1 });
            drop(app);
            let target = &cleanup.0[1];
            assert!(std::fs::metadata(target).unwrap().permissions().readonly());
            assert!(
                std::fs::metadata(&cleanup.0[0])
                    .unwrap()
                    .permissions()
                    .readonly()
            );
            assert_eq!(std::fs::read(target).unwrap(), b"original data");
            assert_eq!(std::fs::read(&cleanup.0[0]).unwrap(), b"original data");
            let journal = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&journal).unwrap();
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            drop(app);
            assert_eq!(std::fs::read(journal).unwrap(), before);
        }
    }
    #[test]
    fn windows_native_copy_readonly_reopening_failures_and_exits_resume_without_source() {
        for nested in [false, true] {
            let first = if nested { 18 } else { 8 };
            let last = if nested { 29 } else { 15 };
            let faults = (first..=last)
                .flat_map(|sequence| {
                    [Step::LocalFlushed, Step::CentralFlushed].map(|step| (sequence, step))
                })
                .chain(
                    [
                        Step::ReadonlyWritable,
                        Step::ReadonlyRestored,
                        Step::ReadonlyFlushed,
                    ]
                    .map(|step| (first + 1, step)),
                )
                .collect::<Vec<_>>();
            for process_exit in [false, true] {
                for &(sequence, step) in &faults {
                    let root = tempfile::tempdir().unwrap();
                    let mut cleanup = readonly_fixture(root.path(), nested);
                    let src = source(&if nested {
                        root.path().join("source")
                    } else {
                        cleanup.0[0].clone()
                    });
                    let parent = owner(&root.path().join("destination"));
                    let central = owner(&root.path().join("central"));
                    let staged = stage(&src, &parent, &central).unwrap();
                    let name = staged.stage.name.clone();
                    cleanup.0.push(fixture_data_path(
                        root.path()
                            .join("destination")
                            .join(&staged.stage.candidate_name),
                        nested,
                    ));
                    drop((staged, src));
                    let mut permissions = std::fs::metadata(&cleanup.0[0]).unwrap().permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    permissions.set_readonly(false);
                    std::fs::set_permissions(&cleanup.0[0], permissions).unwrap();
                    std::fs::remove_file(&cleanup.0[0]).unwrap();
                    std::fs::remove_dir_all(root.path().join("source")).unwrap();
                    if process_exit {
                        let label = match step {
                            Step::LocalFlushed => "local",
                            Step::CentralFlushed => "central",
                            Step::ReadonlyWritable => "readonly-writable",
                            Step::ReadonlyRestored => "readonly-restored",
                            Step::ReadonlyFlushed => "readonly-flushed",
                            _ => unreachable!(),
                        };
                        let status = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", "copy_transaction::windows::journal::tests::windows_native_copy_journal_exit_child", "--ignored", "--nocapture"])
                        .env("OTTRIN_COPY_JOURNAL_CHILD", root.path()).env("OTTRIN_COPY_JOURNAL_SEQUENCE", sequence.to_string())
                        .env("OTTRIN_COPY_JOURNAL_STEP", label).env("OTTRIN_COPY_PLAN_RECOVERY_CHILD", "1").status().unwrap();
                        assert_eq!(status.code(), Some(86), "{sequence} {step:?}");
                    } else {
                        FAULT.with(|f| f.set(Some((sequence, step, false))));
                        let result = PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                            .and_then(|mut app| app.apply(|_| Ok(())));
                        FAULT.with(|f| f.set(None));
                        assert!(result.is_err(), "{sequence} {step:?}");
                    }
                    let journal = root.path().join("destination").join(&name).join(LOCAL);
                    let before = std::fs::read(&journal).unwrap();
                    if step == Step::LocalFlushed {
                        assert!(
                            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err()
                        );
                        assert_eq!(std::fs::read(journal).unwrap(), before);
                    } else {
                        let mut app =
                            PermissionStage::publish(&parent, &central, &name, |_| Ok(()))
                                .unwrap_or_else(|error| {
                                    panic!("resume {sequence} {step:?}: {error}")
                                });
                        app.apply(|_| Ok(())).unwrap();
                        drop(app);
                        assert!(std::fs::read(&journal).unwrap().starts_with(&before));
                        assert!(
                            std::fs::metadata(&cleanup.0[1])
                                .unwrap()
                                .permissions()
                                .readonly()
                        );
                        assert_eq!(std::fs::read(&cleanup.0[1]).unwrap(), b"original data");
                    }
                }
            }
        }
    }
    #[test]
    fn windows_native_copy_readonly_stream_revalidation_cancels_and_resumes_under_its_intent() {
        let root = tempfile::tempdir().unwrap();
        let mut cleanup = readonly_fixture(root.path(), false);
        let src = source(&cleanup.0[0]);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = root
            .path()
            .join("destination")
            .join(&staged.stage.candidate_name);
        cleanup.0.push(candidate.clone());
        drop((staged, src));
        let mut cancelled = false;
        let result = PermissionStage::publish(&parent, &central, &name, |bytes| {
            if bytes > 0
                && !std::fs::metadata(&candidate)
                    .unwrap()
                    .permissions()
                    .readonly()
            {
                cancelled = true;
                Err(io("cancelled readonly verification"))
            } else {
                Ok(())
            }
        });
        assert!(cancelled);
        assert!(result.is_err());
        assert!(!root.path().join("destination/copy").exists());
        let report = inspect(&parent, &central, &name).unwrap();
        assert_eq!(report.readonly_reopening, Some(0));
        assert_eq!(report.completion_entries, 0);
        assert!(!report.completion_verified);
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        assert!(
            std::fs::metadata(&cleanup.0[1])
                .unwrap()
                .permissions()
                .readonly()
        );
        assert_eq!(std::fs::read(&cleanup.0[1]).unwrap(), b"original data");
    }

    #[test]
    fn windows_native_copy_pending_readonly_reopen_refuses_changed_payload_without_restoring_it() {
        let root = tempfile::tempdir().unwrap();
        let mut cleanup = readonly_fixture(root.path(), false);
        let src = source(&cleanup.0[0]);
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = root
            .path()
            .join("destination")
            .join(&staged.stage.candidate_name);
        cleanup.0.push(candidate.clone());
        drop((staged, src));
        FAULT.with(|f| f.set(Some((9, Step::ReadonlyWritable, false))));
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        FAULT.with(|f| f.set(None));
        assert!(
            !std::fs::metadata(&candidate)
                .unwrap()
                .permissions()
                .readonly()
        );
        let modified = std::fs::metadata(&candidate).unwrap().modified().unwrap();
        std::fs::write(&candidate, b"external data").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&candidate)
            .unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        file.sync_all().unwrap();
        drop(file);
        let journal = root.path().join("destination").join(&name).join(LOCAL);
        let before = std::fs::read(&journal).unwrap();
        assert!(PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(journal).unwrap(), before);
        assert_eq!(std::fs::read(&candidate).unwrap(), b"external data");
        assert!(
            !std::fs::metadata(candidate)
                .unwrap()
                .permissions()
                .readonly()
        );
        assert!(!root.path().join("destination/copy").exists());
    }

    #[test]
    fn windows_native_copy_plan_prefix_reopens_without_source_or_original_writer() {
        for stop in 4..=8 {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            let mut calls = 0;
            assert!(
                staged
                    .prepare_destination_security(|| {
                        calls += 1;
                        if calls == stop {
                            Err(io("cancelled prefix"))
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
            );
            assert_eq!(calls, stop);
            let name = staged.stage.name.clone();
            drop((staged, src));
            let path = root.path().join("destination").join(&name).join(LOCAL);
            let before = std::fs::read(&path).unwrap();
            std::fs::remove_dir_all(root.path().join("source")).unwrap();
            let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
            app.apply(|_| Ok(())).unwrap();
            assert_eq!(app.completed(), 3);
            drop(app);
            let after = std::fs::read(path).unwrap();
            assert!(after.starts_with(&before));
            let records = journal_records(&after);
            assert_eq!(records.len(), 28);
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record, Record::SecurityPlanBegin { .. }))
                    .count(),
                1
            );
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record, Record::SecurityPlanEntry { .. }))
                    .count(),
                3
            );
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(record, Record::SecurityPlanComplete { .. }))
                    .count(),
                1
            );
            assert_eq!(
                std::fs::read(root.path().join("destination/copy/nested/file")).unwrap(),
                b"original data"
            );
        }
    }
    #[test]
    fn windows_native_copy_plan_recovery_flush_failures_and_process_exits_keep_evidence() {
        for process_exit in [false, true] {
            for sequence in 11..=15 {
                for step in [Step::LocalFlushed, Step::CentralFlushed] {
                    let root = tempfile::tempdir().unwrap();
                    fixture(root.path());
                    let src = source(&root.path().join("source"));
                    let parent = owner(&root.path().join("destination"));
                    let central = owner(&root.path().join("central"));
                    let staged = stage(&src, &parent, &central).unwrap();
                    let name = staged.stage.name.clone();
                    drop((staged, src));
                    std::fs::remove_dir_all(root.path().join("source")).unwrap();
                    if process_exit {
                        let status=std::process::Command::new(std::env::current_exe().unwrap())
                            .args(["--exact","copy_transaction::windows::journal::tests::windows_native_copy_journal_exit_child","--ignored","--nocapture"])
                            .env("OTTRIN_COPY_JOURNAL_CHILD",root.path()).env("OTTRIN_COPY_JOURNAL_SEQUENCE",sequence.to_string())
                            .env("OTTRIN_COPY_JOURNAL_STEP",if step==Step::LocalFlushed {"local"}else{"central"})
                            .env("OTTRIN_COPY_PLAN_RECOVERY_CHILD","1").status().unwrap();
                        assert_eq!(status.code(), Some(86));
                    } else {
                        FAULT.with(|f| f.set(Some((sequence, step, false))));
                        let result = PermissionStage::publish(&parent, &central, &name, |_| Ok(()));
                        FAULT.with(|f| f.set(None));
                        assert!(result.is_err());
                    }
                    let path = root.path().join("destination").join(&name).join(LOCAL);
                    let before = std::fs::read(&path).unwrap();
                    if step == Step::LocalFlushed {
                        assert!(
                            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).is_err()
                        );
                        assert_eq!(std::fs::read(path).unwrap(), before);
                    } else {
                        let mut app =
                            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
                        app.apply(|_| Ok(())).unwrap();
                        assert_eq!(app.completed(), 3);
                        drop(app);
                        assert!(std::fs::read(path).unwrap().starts_with(&before));
                        assert_eq!(
                            std::fs::read(root.path().join("destination/copy/nested/file"))
                                .unwrap(),
                            b"original data"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn windows_native_copy_streamed_plan_cancelled_prefix_resumes_without_duplicate_records() {
        for stop in 4..=8 {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            let mut calls = 0;
            assert!(
                staged
                    .prepare_destination_security(|| {
                        calls += 1;
                        if calls == stop {
                            Err(io("cancelled streamed intent"))
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
            );
            assert_eq!(calls, stop);
            assert!(!staged.journal.failed);
            assert!(!staged.inspect(&central).unwrap().security_planned);
            let before = std::fs::read(
                root.path()
                    .join("destination")
                    .join(&staged.stage.name)
                    .join(LOCAL),
            )
            .unwrap();
            let prefix = decode(&before).unwrap().state;
            if stop > 4 {
                assert_eq!(prefix.planning.unwrap().evidence.len(), stop - 5);
            }
            drop(staged.prepare_destination_security(|| Ok(())).unwrap());
            assert!(staged.inspect(&central).unwrap().security_planned);
            let after = std::fs::read(
                root.path()
                    .join("destination")
                    .join(&staged.stage.name)
                    .join(LOCAL),
            )
            .unwrap();
            assert!(after.starts_with(&before));
            assert_eq!(journal_records(&after).len(), 16);
            assert!(decode(&after).unwrap().state.planned.is_some());
            assert_eq!(
                std::fs::read(root.path().join("source/nested/file")).unwrap(),
                b"original data"
            );
            assert!(!root.path().join("destination/copy").exists());
        }
    }
}
