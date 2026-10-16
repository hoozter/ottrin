//! Paired, append-only Copy staging records. Inspection never grants deletion.
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

const MAGIC: &[u8; 8] = b"OTCPWJ02";
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
            && self.schema == 2
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
    SecurityPlanned {
        plan: Box<security::TreeSecurityEvidence>,
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
    fn check(&self, record: &Record) -> Result<(), PlatformError> {
        let invalid = || io("invalid Windows Copy staging journal transition or evidence");
        if let Record::SecurityPlanned { plan } = record {
            let header = self.header.as_ref().ok_or_else(invalid)?;
            if !self.verified
                || self.planned.is_some()
                || !plan.binds_staged(
                    &header.creator,
                    header.parent,
                    self.nodes.iter().map(|node| {
                        (
                            node.parent,
                            node.name.as_slice(),
                            node.staged.as_ref().unwrap().candidate_identity,
                            node.source.directory,
                            node.security.as_ref().unwrap(),
                        )
                    }),
                )
            {
                return Err(invalid());
            }
            return Ok(());
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
                    || self.nodes.iter().any(|node| {
                        node.created
                            .is_some_and(|id| id.same_object(source.identity))
                    })
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
                        || self
                            .nodes
                            .iter()
                            .any(|node| node.parent == *parent && node.name == *name)
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
                    || self.nodes.iter().any(|n| {
                        n.created.is_some_and(|id| id.same_object(*identity))
                            || n.source.identity.same_object(*identity)
                    })
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
            Record::SecurityPlanned { .. } => return Err(invalid()),
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
            Record::Begin { header } => self.header = Some(*header),
            Record::CreateIntent {
                parent,
                name,
                source,
                ..
            } => self.nodes.push(Node {
                parent,
                name,
                source,
                created: None,
                security: None,
                staged: None,
            }),
            Record::Created {
                index,
                identity,
                security,
            } => {
                self.nodes[index].created = Some(identity);
                self.nodes[index].security = Some(security);
            }
            Record::EntryStaged { index, evidence } => self.nodes[index].staged = Some(evidence),
            Record::TreeVerified { .. } => self.verified = true,
            Record::SecurityPlanned { plan } => self.planned = Some(*plan),
        }
        Ok(())
    }
}
fn frame_hash(sequence: u64, previous: &[u8; 32], payload: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(sequence.to_le_bytes());
    hash.update(previous);
    hash.update((payload.len() as u32).to_le_bytes());
    hash.update(payload);
    hash.finalize().into()
}
fn decode(bytes: &[u8]) -> Result<State, PlatformError> {
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
    Ok(state)
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
            schema: 2,
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
        let payload = serde_json::to_vec(&record).map_err(io)?;
        if payload.is_empty()
            || payload.len() > MAX_FRAME
            || self.length + FRAME_HEADER as u64 + payload.len() as u64 > MAX_LOG
        {
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
        progress: impl FnMut() -> Result<(), PlatformError>,
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
            progress,
        )?;
        self.journal.bindings()?;
        let evidence = plan.evidence();
        if let Some(recorded) = &self.journal.state.planned {
            if recorded != &evidence {
                return Err(stage_error(
                    &self.stage,
                    io("recorded Copy destination security plan changed; data retained"),
                ));
            }
        } else {
            self.journal
                .persist(Record::SecurityPlanned {
                    plan: Box::new(evidence),
                })
                .map_err(|e| stage_error(&self.stage, e))?;
        }
        plan.validate().map_err(|e| stage_error(&self.stage, e))?;
        Ok(plan)
    }
    pub(super) fn inspect(&self, central: &File) -> Result<Inspection, PlatformError> {
        inspect(self.stage.parent(), central, &self.stage.name)
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
/// Held journal evidence. A recovery validator uses read-sharing only, refusing
/// an active writer rather than racing its in-progress append or payload writes.
struct Snapshot {
    stage: File,
    local: File,
    mirror: File,
    bytes: Vec<u8>,
    state: State,
    requires_creator: bool,
}
impl Snapshot {
    fn load(
        parent: &File,
        central: &File,
        name: &OsStr,
        lock: bool,
    ) -> Result<Self, PlatformError> {
        let entry = |parent: &File, name: &OsStr, directory| {
            if lock {
                namespace::open_source(parent, name, Some(directory))
            } else {
                open(parent, name, Some(directory), FILE_GENERIC_READ)
            }
        };
        let stage = entry(parent, name, true)?;
        let local = entry(&stage, OsStr::new(LOCAL), false)?;
        let bytes = read(&local)?;
        let state = decode(&bytes)?;
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
            requires_creator: lock,
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
    let snapshot = Snapshot::load(parent, central, name, false)?;
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
    entries: Vec<RetainedEntry>,
}
impl<'a> RetainedStage<'a> {
    pub(super) fn open(
        parent: &'a File,
        central: &'a File,
        name: &OsStr,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let snapshot = Snapshot::load(parent, central, name, true)?;
        if !snapshot.state.verified {
            return Err(io(
                "Copy staging was interrupted before full verification; retain for recovery",
            ));
        }
        let mut children = vec![Vec::new(); snapshot.state.nodes.len()];
        for node in &snapshot.state.nodes {
            if let Some(parent) = node.parent {
                children[parent].push(OsString::from_wide(&node.name));
            }
        }
        for names in &mut children {
            names.sort_unstable_by(|left, right| left.encode_wide().cmp(right.encode_wide()));
        }
        let mut entries: Vec<RetainedEntry> = Vec::with_capacity(children.len());
        for (index, node) in snapshot.state.nodes.iter().enumerate() {
            progress(0)?;
            let owner = node.parent.map(|p| &entries[p].handle).unwrap_or(parent);
            let handle = namespace::open_source(owner, &OsString::from_wide(&node.name), None)?;
            let evidence = node.staged.as_ref().unwrap();
            node.security
                .as_ref()
                .unwrap()
                .validate(&handle, &snapshot.state.header.as_ref().unwrap().creator)?;
            evidence.validate_retained_metadata(&handle)?;
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
        let result = Self {
            parent,
            central,
            snapshot,
            entries,
        };
        result.revalidate(progress)?;
        Ok(result)
    }
    pub(super) fn plan_destination_security(
        &self,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        self.revalidate(|_| progress())?;
        let plan = security::TreeSecurityPlan::capture(
            self.parent,
            self.snapshot
                .state
                .nodes
                .iter()
                .zip(&self.entries)
                .map(|(node, entry)| security::SecurityEntry {
                    parent: node.parent,
                    name: OsString::from_wide(&node.name),
                    candidate: &entry.handle,
                    directory: node.source.directory,
                }),
            &mut progress,
        )?;
        self.namespace()?;
        plan.validate()?;
        if self
            .snapshot
            .state
            .planned
            .as_ref()
            .is_some_and(|recorded| *recorded != plan.evidence())
        {
            return Err(io(
                "recorded Copy destination security plan no longer matches native inheritance; data retained",
            ));
        }
        Ok(plan)
    }
    pub(super) fn entry_count(&self) -> usize {
        self.entries.len()
    }
    fn namespace(&self) -> Result<(), PlatformError> {
        self.snapshot.revalidate(self.parent, self.central)?;
        if windows_fs::directory_children_limited(&self.snapshot.stage, 1).map_err(io)?
            != [OsString::from(LOCAL)]
        {
            return Err(io("unexpected Copy transaction entries; all data retained"));
        }
        for (node, entry) in self.snapshot.state.nodes.iter().zip(&self.entries) {
            node.security.as_ref().unwrap().validate(
                &entry.handle,
                &self.snapshot.state.header.as_ref().unwrap().creator,
            )?;
            let owner = node
                .parent
                .map(|p| &self.entries[p].handle)
                .unwrap_or(self.parent);
            bound(
                owner,
                &OsString::from_wide(&node.name),
                entry.observed.identity,
            )?;
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
    pub(super) fn revalidate(
        &self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.namespace()?;
        let mut total = 0u64;
        for (node, entry) in self.snapshot.state.nodes.iter().zip(&self.entries) {
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
        self.namespace()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    LocalFlushed,
    CentralFlushed,
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
            .filter(|n| !n.to_string_lossy().ends_with(".candidate"))
            .collect();
        assert_eq!(names.len(), 1);
        names[0].clone()
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
        assert!(decode(&bytes).unwrap().verified);
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
        let step = if std::env::var("OTTRIN_COPY_JOURNAL_STEP").unwrap() == "local" {
            Step::LocalFlushed
        } else {
            Step::CentralFlushed
        };
        let src = source(&root.join("source"));
        let parent = owner(&root.join("destination"));
        let central = owner(&root.join("central"));
        FAULT.with(|f| f.set(Some((sequence, step, true))));
        let mut staged = stage(&src, &parent, &central).unwrap();
        let _ = staged.prepare_destination_security(|| Ok(()));
        panic!("child did not reach crash boundary");
    }
    #[test]
    fn windows_native_copy_journal_process_exit_retains_unpublished_staging() {
        for sequence in [1, 2, 6, 10, 11] {
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
                    if sequence == 11 {
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
        for step in [Step::LocalFlushed, Step::CentralFlushed] {
            let root = tempfile::tempdir().unwrap();
            fixture(root.path());
            let src = source(&root.path().join("source"));
            let parent = owner(&root.path().join("destination"));
            let central = owner(&root.path().join("central"));
            let mut staged = stage(&src, &parent, &central).unwrap();
            FAULT.with(|f| f.set(Some((11, step, false))));
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
                assert!(inspect(&parent, &central, &name).unwrap().security_planned);
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
        loop {
            let size = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            if at + FRAME_HEADER + size == bytes.len() {
                break;
            }
            at += FRAME_HEADER + size;
        }
        let sequence = u64::from_le_bytes(bytes[at + 12..at + 20].try_into().unwrap());
        let previous: [u8; 32] = bytes[at + 20..at + 52].try_into().unwrap();
        let mut record: serde_json::Value =
            serde_json::from_slice(&bytes[at + FRAME_HEADER..]).unwrap();
        let old = record["plan"]["entries"][0]["final_security"]["dacl"][12]
            .as_u64()
            .unwrap();
        record["plan"]["entries"][0]["final_security"]["dacl"][12] = serde_json::json!(old ^ 1);
        let payload = serde_json::to_vec(&record).unwrap();
        let mut patched = bytes[..at].to_vec();
        patched.extend_from_slice(MAGIC);
        patched.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        patched.extend_from_slice(&sequence.to_le_bytes());
        patched.extend_from_slice(&previous);
        patched.extend_from_slice(&frame_hash(sequence, &previous, &payload));
        patched.extend_from_slice(&payload);
        assert!(
            decode(&patched).unwrap().planned.is_some(),
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
        for entry in &retained.entries {
            super::super::tests::assert_owner_only(&entry.handle);
        }
        assert_eq!(
            std::fs::read(root.path().join("source/nested/file")).unwrap(),
            b"original data"
        );
        assert!(!root.path().join("destination/copy").exists());
        assert_eq!(std::fs::read(&local).unwrap(), patched);
    }
}
