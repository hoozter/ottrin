//! Paired, append-only Copy staging records. Inspection never grants deletion.
use super::{
    EntryVersion, PlatformError, WindowsStableIdentity, io, links, metadata, namespace,
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
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    SYNCHRONIZE,
};

const MAGIC: &[u8; 8] = b"OTCPWJ01";
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
            && self.schema == 1
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
        header: Header,
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
    },
    EntryStaged {
        index: usize,
        evidence: metadata::StagedEntry,
    },
    TreeVerified {
        count: usize,
    },
}
#[derive(Debug)]
struct Node {
    parent: Option<usize>,
    name: Vec<u16>,
    source: EntryVersion,
    created: Option<WindowsStableIdentity>,
    staged: Option<metadata::StagedEntry>,
}
#[derive(Default, Debug)]
struct State {
    header: Option<Header>,
    nodes: Vec<Node>,
    verified: bool,
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
            Record::Created { index, identity } => {
                let node = self.nodes.get(*index).ok_or_else(invalid)?;
                let header = self.header.as_ref().ok_or_else(invalid)?;
                if node.created.is_some()
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
            Record::Begin { header } => self.header = Some(header),
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
                staged: None,
            }),
            Record::Created { index, identity } => self.nodes[index].created = Some(identity),
            Record::EntryStaged { index, evidence } => self.nodes[index].staged = Some(evidence),
            Record::TreeVerified { .. } => self.verified = true,
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
fn open(parent: &File, name: &OsStr, directory: bool, access: u32) -> Result<File, PlatformError> {
    unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: Some(directory),
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
    let actual = open(
        parent,
        name,
        identity.is_directory_without_reparse(),
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
    )?;
    if stable_identity_from_handle(&actual).map_err(io)? != identity {
        return Err(io("Copy journal namespace binding changed; data retained"));
    }
    Ok(())
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
        let header = Header {
            protocol: "ottrin-windows-copy".into(),
            schema: 1,
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
        };
        result.stage_dir.sync_all().map_err(io)?;
        result.central_dir.sync_all().map_err(io)?;
        stage.parent().sync_all().map_err(io)?;
        result.persist(Record::Begin { header })?;
        Ok(result)
    }
    fn bindings(&self) -> Result<(), PlatformError> {
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
            return self.bindings();
        }
        candidate.sync_all().map_err(io)?;
        parent.sync_all().map_err(io)?;
        self.persist(Record::Created { index, identity })
    }
    fn staged(
        &mut self,
        index: usize,
        evidence: &metadata::StagedEntry,
    ) -> Result<(), PlatformError> {
        self.persist(Record::EntryStaged {
            index,
            evidence: evidence.clone(),
        })
    }
    fn verified(&mut self, count: usize) -> Result<(), PlatformError> {
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
}
fn read(file: &File) -> Result<Vec<u8>, PlatformError> {
    let size = file.metadata().map_err(io)?.len();
    if size == 0 || size > MAX_LOG {
        return Err(io("invalid Copy journal size"));
    }
    let mut bytes = Vec::new();
    file.take(size + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 != size || file.metadata().map_err(io)?.len() != size {
        return Err(io("Copy journal changed while reading"));
    }
    Ok(bytes)
}
/// Inspection validates paired records and native directory/file bindings. It
/// does not acquire source handles, publish, rename, remove, repair or authorize
/// cleanup. Incomplete/torn/foreign/rebound records remain retained.
pub(super) fn inspect(
    parent: &File,
    central: &File,
    name: &OsStr,
) -> Result<Inspection, PlatformError> {
    let stage = open(parent, name, true, FILE_GENERIC_READ)?;
    let local = open(&stage, OsStr::new(LOCAL), false, FILE_GENERIC_READ)?;
    let local_bytes = read(&local)?;
    let state = decode(&local_bytes)?;
    let header = state.header.as_ref().unwrap();
    if name != header.stage_name()
        || stable_identity_from_handle(parent).map_err(io)? != header.parent
        || stable_identity_from_handle(&stage).map_err(io)? != header.stage
        || stable_identity_from_handle(central).map_err(io)? != header.central
        || stable_identity_from_handle(&local).map_err(io)? != header.local_file
    {
        return Err(io("Copy journal header does not match held namespace"));
    }
    let mirror = open(central, &header.central_name(), false, FILE_GENERIC_READ)?;
    if stable_identity_from_handle(&mirror).map_err(io)? != header.central_file
        || read(&mirror)? != local_bytes
    {
        return Err(io(
            "Copy journal mirrors differ or were rebound; data retained",
        ));
    }
    bound(parent, name, header.stage)?;
    bound(&stage, OsStr::new(LOCAL), header.local_file)?;
    bound(central, &header.central_name(), header.central_file)?;
    Ok(Inspection {
        entries: state.nodes.len(),
        created: state.nodes.iter().filter(|n| n.created.is_some()).count(),
        staged: state.nodes.iter().filter(|n| n.staged.is_some()).count(),
        verified: state.verified,
    })
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
        let _ = stage(&src, &parent, &central);
        panic!("child did not reach crash boundary");
    }
    #[test]
    fn windows_native_copy_journal_process_exit_retains_unpublished_staging() {
        for sequence in [1, 2, 6, 10] {
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
                    assert_eq!(report.unwrap().verified, sequence == 10);
                }
                assert_eq!(
                    std::fs::read(root.path().join("source/nested/file")).unwrap(),
                    b"original data"
                );
                assert!(!root.path().join("destination/copy").exists());
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
}
