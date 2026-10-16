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

mod discovery;
pub use discovery::{WindowsCopyPhase, WindowsCopyRecoveryEvidence, WindowsRetainedCopyStatus};
pub(crate) use discovery::{discover, discover_at, discover_at_with_progress};

const MAGIC: &[u8; 8] = b"OTCPWJ06";
const FRAME_HEADER: usize = 84;
const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_LOG: u64 = 64 * 1024 * 1024;
const LOCAL: &str = "journal";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    protocol: String,
    schema: u16,
    transaction: String,
    parent: WindowsStableIdentity,
    stage: WindowsStableIdentity,
    central: WindowsStableIdentity,
    local_file: WindowsStableIdentity,
    central_file: WindowsStableIdentity,
    parent_location: NativePath,
    target: Vec<u16>,
    source_label: Vec<u16>,
    creator: Vec<u8>,
    stage_security: security::PrivateSecurity,
    local_security: security::PrivateSecurity,
    central_security: security::PrivateSecurity,
}
impl Header {
    fn stage_name(&self) -> OsString {
        format!(".ottrin-copy-{}", self.transaction).into()
    }
    fn candidate_name(&self) -> OsString {
        format!(".ottrin-copy-{}.candidate", self.transaction).into()
    }
    fn central_name(&self) -> OsString {
        format!("{}.copy-journal", self.transaction).into()
    }
    fn valid(&self) -> bool {
        self.protocol == "ottrin-windows-copy"
            && self.schema == 6
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
            && valid_leaf(&self.target)
            && valid_leaf(&self.source_label)
            && OsString::from_wide(&self.target) != self.stage_name()
            && OsString::from_wide(&self.target) != self.candidate_name()
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
    },
    PublicationIntent,
    Published,
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
    publication_intent: bool,
    published: bool,
    completion_verified: bool,
    final_versions: Vec<EntryVersion>,
    undo_intent: bool,
    undone: Option<EntryVersion>,
    readonly_pending: Option<usize>,
    source_ids: BTreeSet<(u64, [u8; 16])>,
    candidate_ids: BTreeSet<(u64, [u8; 16])>,
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
    let source = &evidence.source;
    if !valid_source(source)
        || ![0, 2].contains(&evidence.compression)
        || (source.attributes & FILE_ATTRIBUTE_COMPRESSED != 0) != (evidence.compression != 0)
        || !evidence.candidate_identity.same_type(source.identity)
    {
        return false;
    }
    let mut primary = 0;
    let mut reparse = 0;
    for stream in &evidence.streams {
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
        && evidence
            .streams
            .windows(2)
            .all(|p| (p[0].kind, &p[0].name) < (p[1].kind, &p[1].name))
}
impl State {
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
        if self.undone.is_some() {
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
            Record::ReadonlyReopened { index } => {
                if self.readonly_pending != Some(*index) {
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
            Record::PublicationIntent => {
                if self.planned.is_none() || self.publication_intent || self.published {
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
            Record::UndoIntent
            | Record::Undone { .. }
            | Record::ReadonlyReopenIntent { .. }
            | Record::ReadonlyReopened { .. }
            | Record::FinalEntry { .. }
            | Record::Finalized { .. }
            | Record::PublicationIntent
            | Record::Published
            | Record::SecurityApplyIntent { .. }
            | Record::SecurityApplied { .. }
            | Record::SecurityPlanBegin { .. }
            | Record::SecurityPlanEntry { .. }
            | Record::SecurityPlanComplete { .. } => return Err(invalid()),
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
        match record {
            Record::UndoIntent => self.undo_intent = true,
            Record::Undone { root_version } => self.undone = Some(*root_version),
            Record::ReadonlyReopenIntent { index } => self.readonly_pending = Some(index),
            Record::ReadonlyReopened { .. } => self.readonly_pending = None,
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
    if bytes.len() as u64 > MAX_LOG {
        return Err(io("Copy journal exceeds inspection budget"));
    }
    let mut state = State::default();
    let mut at = 0usize;
    let mut sequence = 0u64;
    let mut previous = [0u8; 32];
    while at < bytes.len() {
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
    if state.header.is_none() {
        return Err(io("empty Copy journal; data retained"));
    }
    Ok(Decoded {
        state,
        sequence,
        previous,
    })
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

struct Journal {
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
    fn create(
        stage: &namespace::PrivateStage<'_>,
        central: &File,
        target: &OsStr,
        source_label: &OsStr,
    ) -> Result<Self, PlatformError> {
        stage.validate()?;
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
        let local =
            windows_fs::create_private(&stage.directory, OsStr::new(LOCAL), false, FILE_SHARE_READ)
                .map_err(io)?;
        let central_file =
            windows_fs::create_private(central, &central_name, false, FILE_SHARE_READ)
                .map_err(io)?;
        let creator = security::current_owner()?;
        let header = Header {
            stage_security: security::PrivateSecurity::capture(&stage.directory, &creator)?,
            local_security: security::PrivateSecurity::capture(&local, &creator)?,
            central_security: security::PrivateSecurity::capture(&central_file, &creator)?,
            creator,
            protocol: "ottrin-windows-copy".into(),
            schema: 6,
            transaction: name,
            parent: parent_id,
            stage: stage_id,
            central: central_id,
            local_file: stable_identity_from_handle(&local).map_err(io)?,
            central_file: stable_identity_from_handle(&central_file).map_err(io)?,
            parent_location: NativePath::encode(&windows_fs::location(stage.parent()).map_err(io)?),
            target: target.encode_wide().collect(),
            source_label: source_label.encode_wide().collect(),
        };
        if !header.valid() {
            return Err(io("invalid Copy journal header"));
        }
        let mut result = Self {
            local,
            central: central_file,
            stage_dir: stage.directory.try_clone().map_err(io)?,
            parent_dir: stage.parent().try_clone().map_err(io)?,
            central_dir: central.try_clone().map_err(io)?,
            header: header.clone(),
            state: State::default(),
            sequence: 0,
            previous: [0; 32],
            length: 0,
            failed: false,
            candidates: Vec::new(),
        };
        result.stage_dir.sync_all().map_err(io)?;
        result.central_dir.sync_all().map_err(io)?;
        stage.parent().sync_all().map_err(io)?;
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
        Ok(Self {
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
        })
    }
    fn reserve_undo_tail(&self) -> Result<(), PlatformError> {
        if self.state.undone.is_some() {
            return Ok(());
        }
        let mut records = Vec::new();
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
            records.push(Record::ReadonlyReopened { index: 0 });
        }
        let mut version = self.state.final_versions[0].clone();
        version.changed = i64::MIN;
        records.push(Record::Undone {
            root_version: Box::new(version),
        });
        let required = records.iter().try_fold(0u64, |total, record| {
            total
                .checked_add(FRAME_HEADER as u64 + record_payload(record)?.len() as u64)
                .ok_or_else(|| io("Copy Undo reservation overflow"))
        })?;
        if self
            .length
            .checked_add(required)
            .is_none_or(|length| length > MAX_LOG)
        {
            return Err(io(
                "Copy journal has insufficient Undo space; data retained",
            ));
        }
        Ok(())
    }
    fn permission_tail_size(&self) -> Result<u64, PlatformError> {
        let mut total = 0u64;
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
                    reserve(Record::ReadonlyReopened { index })?;
                }
                for (index, node) in self.state.nodes.iter().enumerate() {
                    if !node.source.directory
                        && node.source.attributes
                            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY
                            != 0
                    {
                        reserve(Record::ReadonlyReopenIntent { index })?;
                        reserve(Record::ReadonlyReopened { index })?;
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
            .is_none_or(|length| length > MAX_LOG)
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
            .is_none_or(|length| length > MAX_LOG)
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
        let evidence = node.staged.as_ref().unwrap().clone();
        evidence.validate_reopening_metadata(&file, self.state.readonly_pending == Some(index))?;
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
        self.persist(Record::ReadonlyReopened { index })?;
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
    fn bindings(&self) -> Result<(), PlatformError> {
        if security::current_owner()? != self.header.creator {
            return Err(io("Copy journal writer creator context changed"));
        }
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
        if self.length + FRAME_HEADER as u64 + payload.len() as u64 > MAX_LOG {
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
        target: &OsStr,
        source_label: &OsStr,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        windows_fs::valid_component(target).map_err(io)?;
        windows_fs::valid_component(source_label).map_err(io)?;
        let source = tree::PinnedTree::open(source, max_entries, &mut progress)?;
        source.reject_destination_parent(parent)?;
        source.reject_destination_parent(central)?;
        let stage = namespace::PrivateStage::create(parent)?;
        let retained = |error| stage_error(&stage, error);
        let mut journal =
            Journal::create(&stage, central, target, source_label).map_err(retained)?;
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
}
impl PermissionStage {
    pub(super) fn publish(
        parent: &File,
        central: &File,
        name: &OsStr,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let retained = RetainedStage::open(parent, central, name, &mut progress)?;
        if retained.snapshot.state.undo_intent {
            return Err(io("Copy is retracting or undone; use Undo recovery"));
        }
        let prepared = retained.derive_destination_security(|| progress(0))?;
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
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Write)?;
        if snapshot.bytes != expected {
            return Err(io(
                "Copy journal changed between validation and writer acquisition",
            ));
        }
        let root = windows_fs::PerObjectFile::open(parent, &root_name)
            .map_err(|e| io(format!("opening publication root: {e}")))?;
        let root_evidence = snapshot.state.nodes[0].staged.as_ref().unwrap();
        if stable_identity_from_handle(&root).map_err(io)? != root_evidence.candidate_identity {
            return Err(io("Copy publication root identity changed"));
        }
        root_evidence
            .validate_reopening_metadata(&root, snapshot.state.readonly_pending == Some(0))?;
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
        evidence.validate_retained_metadata(file)?;
        if super::transfer(file, None, progress)? != evidence.streams
            || links::capture(file)? != evidence.source.link
        {
            return Err(io(
                "Copy permission candidate contents changed; all data retained",
            ));
        }
        evidence.validate_retained_metadata(file)
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
        let mirror = entry(central, &header.central_name(), false)?;
        let result = Self {
            stage,
            local,
            mirror,
            bytes,
            state,
            requires_creator: access != SnapshotAccess::Inspect,
            sequence,
            previous,
        };
        result.revalidate(parent, central)?;
        Ok(result)
    }
    fn revalidate(&self, parent: &File, central: &File) -> Result<(), PlatformError> {
        let header = self.state.header.as_ref().unwrap();
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
        undo_pending: snapshot.state.undo_intent && snapshot.state.undone.is_none(),
        undone: snapshot.state.undone.is_some(),
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
pub(super) struct RetainedStage<'a> {
    parent: &'a File,
    central: &'a File,
    snapshot: Snapshot,
    tree: HeldTree,
}
impl<'a> RetainedStage<'a> {
    pub(super) fn open(
        parent: &'a File,
        central: &'a File,
        name: &OsStr,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
        let root_name = snapshot.root_name(parent)?;
        let tree = HeldTree::open(parent, &snapshot.state, root_name, None, &mut progress)?;
        let result = Self {
            parent,
            central,
            snapshot,
            tree,
        };
        result.revalidate(progress)?;
        Ok(result)
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
            evidence.validate_reopening_metadata(&handle, state.readonly_pending == Some(index))?;
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
    let retained = RetainedStage::open(parent, central, name, &mut progress)?;
    if !retained.snapshot.state.completion_verified {
        return Err(io("Copy has no completed receipt; Undo refused"));
    }
    if retained.snapshot.state.undone.is_some() {
        return Ok(());
    }
    let expected = retained.snapshot.bytes.clone();
    drop(retained);
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Write)?;
    if snapshot.bytes != expected {
        return Err(io("Copy journal changed before Undo writer acquisition"));
    }
    let root_name = snapshot.root_name(parent)?;
    let mut root = windows_fs::PerObjectFile::open(parent, &root_name).map_err(io)?;
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
        journal.persist(Record::Undone {
            root_version: Box::new(tree.entries[0].observed.clone()),
        })?;
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
enum Step {
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
        std::fs::create_dir(root.join("central")).unwrap();
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
            OsStr::new("copy"),
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
                n.to_string_lossy().starts_with(".ottrin-copy-")
                    && !n.to_string_lossy().ends_with(".candidate")
            })
            .collect();
        assert_eq!(names.len(), 1);
        names[0].clone()
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
            reserved - actual <= 19 * 3 + 64,
            "only signed change-time and digest decimal widths may over-reserve"
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
        let parent = owner(&path.join("destination"));
        let central = owner(&path.join("central"));
        let staged = stage(&src, &parent, &central).unwrap();
        let name = staged.stage.name.clone();
        let candidate = staged.stage.candidate_name.clone();
        cleanup.0.push(fixture_data_path(
            path.join("destination").join(&candidate),
            nested,
        ));
        drop((staged, src));
        let mut app = PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        app.apply(|_| Ok(())).unwrap();
        drop(app);
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
            .find(|status| status.transaction_id == "1".repeat(32))
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
                OsStr::new("copy"),
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
    fn windows_native_copy_journal_uncertain_owner_cannot_retry_or_repair() {
        let root = tempfile::tempdir().unwrap();
        fixture(root.path());
        let parent = owner(&root.path().join("destination"));
        let central = owner(&root.path().join("central"));
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let mut journal =
            Journal::create(&stage, &central, OsStr::new("copy"), OsStr::new("source")).unwrap();
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
            OsStr::new("copy"),
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
