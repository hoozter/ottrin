//! Retained Copy cleanup, bound to original journals and object/content evidence.

use super::*;
use crate::move_recovery::LinuxJournalDirectory;
use rustix::fs::{Mode, OFlags};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, MetadataExt};

#[path = "copy_finalization.rs"]
mod finalization;
pub use finalization::{RetainedCopyFinalization, discover_finalizations, resume_finalization};

const LEDGER: &str = "copy-cleanup";
const QUARANTINE: &str = "cleanup-entry";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CleanupIntent {
    origin: CopyPhase,
    identity: PathMoveIdentity,
    handle: ObjectHandle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Progress {
    index: usize,
    pending: bool,
    previous: String,
}

struct Ledger {
    file: std::fs::File,
    state: Progress,
    digest: String,
    stamp: LedgerStamp,
}

/// Constant-size checks protect an active ledger without rereading its prefix.
#[derive(Clone, PartialEq, Eq)]
struct LedgerStamp {
    device: u64,
    inode: u64,
    size: u64,
    mode: u32,
    uid: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl LedgerStamp {
    fn read(file: &std::fs::File) -> Result<Self, PlatformError> {
        let m = file.metadata().map_err(journal_error)?;
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
            size: m.len(),
            mode: m.mode(),
            uid: m.uid(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        })
    }
}

struct Node {
    relative: PathBuf,
    content: SourceTreeSnapshot,
    objects: Vec<TreeObject>,
}

pub(super) fn validate_progress(record: &CopyRecord) -> Result<(), PlatformError> {
    match (record.phase, &record.cleanup) {
        (CopyPhase::CleanupPending | CopyPhase::CleanupComplete, Some(intent))
            if matches!(intent.origin, CopyPhase::Published | CopyPhase::Undone)
                && intent.handle.valid() =>
        {
            Ok(())
        }
        (
            CopyPhase::PublicationIntent
            | CopyPhase::Published
            | CopyPhase::UndoIntent
            | CopyPhase::Undone,
            None,
        ) => Ok(()),
        _ => Err(refusal("Copy cleanup intent does not match its phase")),
    }
}

fn nodes(record: &CopyRecord) -> Result<Vec<Node>, PlatformError> {
    let Some((content, objects)) = retained_evidence(record)? else {
        return Ok(Vec::new());
    };
    let mut objects = objects
        .iter()
        .map(|object| Ok((object.path.decode().map_err(journal_error)?, object.clone())))
        .collect::<Result<BTreeMap<_, _>, PlatformError>>()?;
    let mut result = Vec::new();
    for (relative, content) in content
        .cleanup_nodes()
        .map_err(journal_error)?
        .into_iter()
        .rev()
    {
        let mut object = objects
            .remove(&relative)
            .ok_or_else(|| refusal("Missing Copy cleanup object evidence"))?;
        object.path = NativePath::encode(Path::new(""));
        result.push(Node {
            relative,
            content,
            objects: vec![object],
        });
    }
    Ok(result)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn origin_digest(record: &CopyRecord) -> Result<String, PlatformError> {
    let mut original = record.clone();
    if let Some(intent) = original.cleanup.take() {
        original.phase = intent.origin;
    }
    Ok(digest(
        &serde_json::to_vec(&original).map_err(journal_error)?,
    ))
}

fn mount(fd: &impl rustix::fd::AsFd) -> Result<u64, PlatformError> {
    crate::local_storage::local_fd_mount_identity(fd).map_err(journal_error)
}

impl Ledger {
    fn create(
        stage: &impl rustix::fd::AsFd,
        record: &CopyRecord,
    ) -> Result<(Self, CleanupIntent), PlatformError> {
        let fd = rustix::fs::openat(
            stage,
            LEDGER,
            OFlags::CREATE
                | OFlags::EXCL
                | OFlags::RDWR
                | OFlags::APPEND
                | OFlags::CLOEXEC
                | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(journal_error)?;
        let metadata = rustix::fs::fstat(&fd).map_err(journal_error)?;
        let handle = ObjectHandle::capture(&fd)
            .map_err(journal_error)?
            .ok_or_else(|| refusal("Cleanup ledger generation evidence is unavailable"))?;
        let identity = PathMoveIdentity {
            device: metadata.st_dev,
            inode: metadata.st_ino,
        };
        let header = origin_digest(record)?;
        let mut file = std::fs::File::from(fd);
        file.write_all(format!("{header}\n").as_bytes())
            .map_err(journal_error)?;
        let stamp = LedgerStamp::read(&file)?;
        let mut ledger = Self {
            stamp,
            file,
            state: Progress {
                index: 0,
                pending: false,
                previous: header.clone(),
            },
            digest: header,
        };
        ledger.append(0, false)?;
        rustix::fs::fsync(stage).map_err(journal_error)?;
        Ok((
            ledger,
            CleanupIntent {
                origin: record.phase,
                identity,
                handle,
            },
        ))
    }

    fn open(stage: &impl rustix::fd::AsFd, record: &CopyRecord) -> Result<Self, PlatformError> {
        let intent = record
            .cleanup
            .as_ref()
            .ok_or_else(|| refusal("Copy has no cleanup intent"))?;
        let fd = rustix::fs::openat(
            stage,
            LEDGER,
            OFlags::RDWR | OFlags::APPEND | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        let stat = rustix::fs::fstat(&fd).map_err(journal_error)?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
            || stat.st_uid != rustix::process::geteuid().as_raw()
            || stat.st_mode & 0o777 != 0o600
            || (stat.st_dev, stat.st_ino) != (intent.identity.device, intent.identity.inode)
            || mount(&fd)? != record.transaction_mount
            || ObjectHandle::capture(&fd).map_err(journal_error)?.as_ref() != Some(&intent.handle)
        {
            return Err(refusal(
                "Cleanup ledger identity, permissions or mount changed",
            ));
        }
        let mut file = std::fs::File::from(fd);
        let stamp = LedgerStamp::read(&file)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(journal_error)?;
        if LedgerStamp::read(&file)? != stamp {
            return Err(refusal("Cleanup ledger changed while reading progress"));
        }
        if !bytes.ends_with(b"\n") {
            return Err(refusal(
                "Cleanup ledger has an incomplete write; retain it for review",
            ));
        }
        let mut lines = bytes[..bytes.len() - 1].split(|byte| *byte == b'\n');
        let header = origin_digest(record)?;
        if lines.next() != Some(header.as_bytes()) {
            return Err(refusal("Cleanup ledger belongs to another Copy record"));
        }
        let mut previous = header;
        let mut state: Option<Progress> = None;
        for line in lines {
            let next: Progress = serde_json::from_slice(line).map_err(journal_error)?;
            if next.previous != previous
                || serde_json::to_vec(&next).map_err(journal_error)? != line
            {
                return Err(refusal("Cleanup ledger hash chain or encoding changed"));
            }
            let valid = match &state {
                None => next.index == 0 && !next.pending,
                Some(old) if !old.pending => next.index == old.index && next.pending,
                Some(old) => {
                    next.index
                        == old
                            .index
                            .checked_add(1)
                            .ok_or_else(|| refusal("Cleanup progress overflow"))?
                        && !next.pending
                }
            };
            if !valid {
                return Err(refusal("Cleanup ledger progress is not consecutive"));
            }
            previous = digest(line);
            state = Some(next);
        }
        let state = state.ok_or_else(|| refusal("Cleanup ledger has no durable progress"))?;
        let count = nodes(record)?.len();
        if state.index > count
            || (state.pending && state.index == count)
            || (record.phase == CopyPhase::CleanupComplete && state.index != count)
        {
            return Err(refusal("Cleanup ledger progress exceeds its original tree"));
        }
        Ok(Self {
            file,
            state,
            digest: previous,
            stamp,
        })
    }

    fn validate_named(&self, stage: &std::fs::File) -> Result<(), PlatformError> {
        let named = rustix::fs::openat(
            stage,
            LEDGER,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        if mount(&named)? != mount(stage)?
            || LedgerStamp::read(&std::fs::File::from(named))? != self.stamp
            || LedgerStamp::read(&self.file)? != self.stamp
        {
            return Err(refusal("Cleanup ledger changed during reclamation"));
        }
        Ok(())
    }

    fn append(&mut self, index: usize, pending: bool) -> Result<(), PlatformError> {
        if LedgerStamp::read(&self.file)? != self.stamp {
            return Err(refusal("Cleanup ledger changed before appending progress"));
        }
        let next = Progress {
            index,
            pending,
            previous: self.digest.clone(),
        };
        let bytes = serde_json::to_vec(&next).map_err(journal_error)?;
        self.file
            .write_all(&bytes)
            .and_then(|()| self.file.write_all(b"\n"))
            .and_then(|()| self.file.sync_all())
            .map_err(journal_error)?;
        let stamp = LedgerStamp::read(&self.file)?;
        let expected_size = self
            .stamp
            .size
            .checked_add(bytes.len() as u64 + 1)
            .ok_or_else(|| refusal("Cleanup ledger size overflow"))?;
        let mut tail = vec![0; bytes.len() + 1];
        self.file
            .read_exact_at(&mut tail, self.stamp.size)
            .map_err(journal_error)?;
        if stamp.size != expected_size || tail[..bytes.len()] != bytes || tail[bytes.len()] != b'\n'
        {
            return Err(refusal("Cleanup progress append was changed"));
        }
        self.stamp = stamp;
        self.digest = digest(&bytes);
        self.state = next;
        Ok(())
    }
}

fn path_from_fd(fd: &impl rustix::fd::AsFd) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", fd.as_fd().as_raw_fd()))
}

fn node_source_path(stage: &impl rustix::fd::AsFd, relative: &Path) -> PathBuf {
    let mut path = path_from_fd(stage).join(crate::copy_transaction::CANDIDATE);
    // Joining an empty path appends a slash, which changes regular-file and
    // symlink semantics. The root node is the candidate entry itself.
    if !relative.as_os_str().is_empty() {
        path.push(relative);
    }
    path
}

pub(super) fn validate_partial_data(
    record: &CopyRecord,
    stage: &impl rustix::fd::AsFd,
) -> Result<(), PlatformError> {
    let ledger = Ledger::open(stage, record)?;
    validate_partial(record, stage, &ledger.state)
}

fn validate_partial(
    record: &CopyRecord,
    stage: &impl rustix::fd::AsFd,
    state: &Progress,
) -> Result<(), PlatformError> {
    let entries = nodes(record)?;
    let quarantine = path_from_fd(stage).join(QUARANTINE);
    let quarantined = exists(&quarantine)?;
    let mut removed = entries
        .iter()
        .take(state.index)
        .map(|entry| entry.relative.clone())
        .collect::<BTreeSet<_>>();
    if quarantined {
        if !state.pending {
            return Err(refusal("Unexpected cleanup quarantine entry"));
        }
        let node = &entries[state.index];
        record.validate_tree(&quarantine, &node.content, &node.objects)?;
        removed.insert(node.relative.clone());
    } else if state.pending {
        // A completed unlink may precede its next durable ledger frame.
        let node = &entries[state.index];
        let candidate = node_source_path(stage, &node.relative);
        match std::fs::symlink_metadata(candidate) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                removed.insert(node.relative.clone());
            }
            Err(error) => return Err(journal_error(error)),
            Ok(_) => {}
        }
    }
    let candidate = path_from_fd(stage).join(crate::copy_transaction::CANDIDATE);
    if let Some((content, objects)) = retained_evidence(record)? {
        if removed.contains(Path::new("")) {
            match std::fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(journal_error(error)),
                Ok(_) => return Err(refusal("A completed cleanup pathname was reused")),
            }
        } else {
            let content = content.excluding_paths(&removed).map_err(journal_error)?;
            let objects = objects
                .iter()
                .filter_map(|object| match object.path.decode() {
                    Ok(path) if !removed.contains(&path) => Some(Ok(object.clone())),
                    Ok(_) => None,
                    Err(error) => Some(Err(journal_error(error))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            record.validate_tree(&candidate, &content, &objects)?;
        }
    } else if exists(&candidate)? {
        return Err(refusal(
            "Cleanup has no authority for an unexpected candidate",
        ));
    }
    Ok(())
}

fn exists(path: &Path) -> Result<bool, PlatformError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(journal_error(error)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CleanupPoint {
    IntentDurable,
    EntryIntentDurable,
    Claimed,
    BeforeUnlink,
    Unlinked,
    ProgressDurable,
    CompleteDurable,
}

fn original_record(record: &CopyRecord) -> CopyRecord {
    let mut result = record.clone();
    if let Some(intent) = result.cleanup.take() {
        result.phase = intent.origin;
    }
    result
}

fn open_directories(record: &CopyRecord) -> Result<(std::fs::File, std::fs::File), PlatformError> {
    record.require_object_evidence()?;
    record.validate_namespace()?;
    let transaction = record.transaction.decode().map_err(journal_error)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let parent = std::fs::File::from(
        rustix::fs::open(
            transaction.parent().expect("validated parent"),
            flags,
            Mode::empty(),
        )
        .map_err(journal_error)?,
    );
    let stage = std::fs::File::from(
        rustix::fs::openat(
            &parent,
            transaction.file_name().expect("validated name"),
            flags,
            Mode::empty(),
        )
        .map_err(journal_error)?,
    );
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    Ok((parent, stage))
}

fn pair_matches(
    central: &LinuxJournalDirectory,
    local: &LinuxJournalDirectory,
    record: &CopyRecord,
) -> Result<(), PlatformError> {
    let id = record.transaction_id()?;
    if central.read_copy_record(&format!("{id}.journal"))? != *record
        || local.read_copy_record("copy-journal")? != *record
    {
        return Err(refusal("Copy cleanup journals changed during mutation"));
    }
    Ok(())
}

fn check_ordered_pair(
    central: &LinuxJournalDirectory,
    local: &LinuxJournalDirectory,
    stage: &impl rustix::fd::AsFd,
    record: &CopyRecord,
) -> Result<CopyRecord, PlatformError> {
    let previous = local.read_copy_record("copy-journal")?;
    if previous == *record {
        pair_matches(central, local, record)?;
        return Ok(previous);
    }
    if original_record(&previous) != original_record(record) {
        return Err(refusal(
            "Copy cleanup journal pair has different original evidence",
        ));
    }
    let forward = matches!(
        (previous.phase, record.phase),
        (
            CopyPhase::Published | CopyPhase::Undone,
            CopyPhase::CleanupPending
        ) | (CopyPhase::CleanupPending, CopyPhase::CleanupComplete)
    );
    if !forward {
        return Err(refusal(
            "Copy cleanup journal pair is not one ordered forward transition",
        ));
    }
    let ledger = Ledger::open(stage, record)?;
    if record.phase == CopyPhase::CleanupPending
        && (ledger.state.index != 0 || ledger.state.pending)
    {
        return Err(refusal(
            "Cleanup data advanced before both intent journals were durable",
        ));
    }
    validate_partial(record, stage, &ledger.state)?;
    if central.read_copy_record(&format!("{}.journal", record.transaction_id()?))? != *record
        || local.read_copy_record("copy-journal")? != previous
    {
        return Err(refusal(
            "Cleanup journal pair changed before reconciliation",
        ));
    }
    Ok(previous)
}

fn reconcile_pair(
    central: &LinuxJournalDirectory,
    local: &LinuxJournalDirectory,
    stage: &std::fs::File,
    record: &CopyRecord,
) -> Result<(), PlatformError> {
    if check_ordered_pair(central, local, stage, record)? != *record {
        local.persist_copy_record(record).map_err(journal_error)?;
    }
    pair_matches(central, local, record)
}

pub(super) fn validate_observed_pair(
    stage: &impl rustix::fd::AsFd,
    record: &CopyRecord,
) -> Result<(), PlatformError> {
    let root = configured_root()?;
    let central = LinuxJournalDirectory::open_copy_registry(&root, false).map_err(journal_error)?;
    let local = LinuxJournalDirectory::for_copy_stage(stage).map_err(journal_error)?;
    check_ordered_pair(&central, &local, stage, record)?;
    Ok(())
}

fn mutation_boundary(
    central: &LinuxJournalDirectory,
    local: &LinuxJournalDirectory,
    parent: &std::fs::File,
    stage: &std::fs::File,
    record: &CopyRecord,
    selected: &BTreeSet<String>,
    parsed: &mut CopyRecordCache,
) -> Result<(), PlatformError> {
    pair_matches(central, local, record)?;
    mutation_scope(central, parent, stage, record, selected, parsed)
}

fn mutation_scope(
    central: &LinuxJournalDirectory,
    parent: &std::fs::File,
    stage: &std::fs::File,
    record: &CopyRecord,
    selected: &BTreeSet<String>,
    parsed: &mut CopyRecordCache,
) -> Result<(), PlatformError> {
    record.validate_namespace()?;
    record.validate_opened_directory(parent, true)?;
    record.validate_opened_directory(stage, false)?;
    let all = registry_records_cached(central, parsed)?;
    for (id, other) in &all {
        if other.operation.id == record.operation.id && !selected.contains(id) {
            return Err(refusal("Copy operation membership changed during cleanup"));
        }
        if !selected.contains(id)
            && let Some((_, objects)) = retained_evidence(record)?
            && possible_reference(other, objects)?
        {
            return Err(refusal(
                "Another Copy operation now references retained cleanup data",
            ));
        }
    }
    let mut entries = rustix::fs::Dir::read_from(stage).map_err(journal_error)?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(journal_error)?;
        if !matches!(
            entry.file_name().to_bytes(),
            b"." | b".." | b"candidate" | b"copy-journal" | b"copy-cleanup" | b"cleanup-entry"
        ) {
            return Err(refusal("An unknown entry appeared in the Copy transaction"));
        }
    }
    Ok(())
}

/// A worker-scoped watch invalidates the initial journal/dependency proof on
/// any metadata change. It never holds mounts while Activity is idle.
struct CleanupMonitor {
    events: rustix::fd::OwnedFd,
    registry: rustix::fd::OwnedFd,
    root: PathBuf,
    registry_watch: i32,
    stage_watch: i32,
    mounts: crate::local_storage::MountNamespaceGuard,
    full_revalidation: bool,
    selected: BTreeSet<String>,
}

impl CleanupMonitor {
    fn new(
        central: &LinuxJournalDirectory,
        stage: &std::fs::File,
        root: &Path,
        selected: &BTreeSet<String>,
    ) -> Result<Self, PlatformError> {
        use rustix::fs::inotify;
        let mounts = crate::local_storage::MountNamespaceGuard::new().map_err(journal_error)?;
        let registry = central.duplicate_copy_directory().map_err(journal_error)?;
        let events = inotify::init(inotify::CreateFlags::NONBLOCK | inotify::CreateFlags::CLOEXEC)
            .map_err(journal_error)?;
        let mask = inotify::WatchFlags::CREATE
            | inotify::WatchFlags::DELETE
            | inotify::WatchFlags::MOVED_FROM
            | inotify::WatchFlags::MOVED_TO
            | inotify::WatchFlags::MODIFY
            | inotify::WatchFlags::ATTRIB
            | inotify::WatchFlags::DELETE_SELF
            | inotify::WatchFlags::MOVE_SELF
            | inotify::WatchFlags::ONLYDIR;
        let registry_watch =
            inotify::add_watch(&events, path_from_fd(&registry), mask).map_err(journal_error)?;
        let stage_watch =
            inotify::add_watch(&events, path_from_fd(stage), mask).map_err(journal_error)?;
        let mut full_revalidation =
            !crate::local_storage::supports_local_inode_notifications(&registry)
                || !crate::local_storage::supports_local_inode_notifications(stage);
        let file_mask = mask & !inotify::WatchFlags::ONLYDIR;
        let mut watch_file =
            |directory: &dyn rustix::fd::AsFd, name: &str| -> Result<(), PlatformError> {
                let fd = rustix::fs::openat(
                    directory,
                    name,
                    OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(journal_error)?;
                let stat = rustix::fs::fstat(&fd).map_err(journal_error)?;
                if rustix::fs::FileType::from_raw_mode(stat.st_mode)
                    != rustix::fs::FileType::RegularFile
                {
                    return Err(refusal("Copy cleanup journal is not a regular file"));
                }
                full_revalidation |= !crate::local_storage::supports_local_inode_notifications(&fd);
                // Directory watches miss writes through hard links outside the
                // registry/stage. Any event from a journal inode is unexpected.
                if inotify::add_watch(&events, path_from_fd(&fd), file_mask).is_err() {
                    full_revalidation = true;
                }
                Ok(())
            };
        central
            .discover_copy_records_with(|name| watch_file(&registry, name))
            .map_err(journal_error)?
            .into_iter()
            .try_for_each(|(_, result)| result)?;
        watch_file(stage, "copy-journal")?;
        Ok(Self {
            mounts,
            full_revalidation,
            selected: selected.clone(),
            events,
            registry,
            root: root.into(),
            registry_watch,
            stage_watch,
        })
    }

    fn check(
        &self,
        record: &CopyRecord,
        parent: &std::fs::File,
        stage: &std::fs::File,
    ) -> Result<(), PlatformError> {
        use rustix::fs::inotify;
        self.mounts.validate().map_err(journal_error)?;
        if self.full_revalidation {
            let central =
                LinuxJournalDirectory::for_copy_stage(&self.registry).map_err(journal_error)?;
            let local = LinuxJournalDirectory::for_copy_stage(stage).map_err(journal_error)?;
            mutation_boundary(
                &central,
                &local,
                parent,
                stage,
                record,
                &self.selected,
                &mut CopyRecordCache::uncached(),
            )?;
        }
        record.validate_namespace()?;
        record.validate_opened_directory(parent, true)?;
        record.validate_opened_directory(stage, false)?;
        let named = rustix::fs::open(
            &self.root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        let original = rustix::fs::fstat(&self.registry).map_err(journal_error)?;
        let current = rustix::fs::fstat(&named).map_err(journal_error)?;
        if (original.st_dev, original.st_ino) != (current.st_dev, current.st_ino)
            || mount(&named)? != mount(&self.registry)?
        {
            return Err(refusal("Copy cleanup registry namespace changed"));
        }
        let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.events, &mut buffer);
        loop {
            let event = match reader.next() {
                Ok(event) => event,
                Err(rustix::io::Errno::WOULDBLOCK) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(journal_error(error)),
            };
            let name = event.file_name().map(|name| name.to_bytes());
            let expected_payload_event = event.wd() == self.stage_watch
                && matches!(name, Some(b"candidate" | b"cleanup-entry"));
            // Appends are checked against the pinned ledger's size and stamps;
            // replacing, chmodding or removing its name is never our append.
            let expected_ledger_append = event.wd() == self.stage_watch
                && name == Some(LEDGER.as_bytes())
                && event.events() == inotify::ReadFlags::MODIFY;
            if event.wd() == self.registry_watch
                || (!expected_payload_event && !expected_ledger_append)
                || event.events().contains(inotify::ReadFlags::QUEUE_OVERFLOW)
            {
                return Err(refusal(
                    "Copy cleanup metadata changed; revalidation is required",
                ));
            }
        }
    }
}

fn source_parent(
    stage: &std::fs::File,
    relative: &Path,
    original: &BTreeMap<PathBuf, TreeObject>,
) -> Result<(rustix::fd::OwnedFd, std::ffi::OsString), PlatformError> {
    let mut full = PathBuf::from(crate::copy_transaction::CANDIDATE);
    if !relative.as_os_str().is_empty() {
        full.push(relative);
    }
    let name = full
        .file_name()
        .ok_or_else(|| refusal("Cleanup node has no name"))?
        .to_owned();
    let mut directory = rustix::io::fcntl_dupfd_cloexec(stage, 0).map_err(journal_error)?;
    let mut logical = PathBuf::new();
    for (index, part) in full
        .parent()
        .expect("candidate parent")
        .components()
        .enumerate()
    {
        let Component::Normal(part) = part else {
            return Err(refusal("Invalid cleanup node parent"));
        };
        directory = rustix::fs::openat(
            &directory,
            part,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        if index > 0 {
            logical.push(part);
        }
        let expected = original
            .get(&logical)
            .ok_or_else(|| refusal("Cleanup ancestor has no original evidence"))?;
        if mount(&directory)? != expected.mount
            || ObjectHandle::capture(&directory).map_err(journal_error)? != expected.handle
        {
            return Err(refusal("Cleanup ancestor directory or mount was replaced"));
        }
    }
    Ok((directory, name))
}

fn reclaim_payload(
    root: &Path,
    expected: &CopyRecord,
    selected: &BTreeSet<String>,
    hook: &mut impl FnMut(CleanupPoint) -> Result<(), PlatformError>,
    parsed: &mut CopyRecordCache,
) -> Result<CopyRecord, PlatformError> {
    let central = LinuxJournalDirectory::open_copy_registry(root, false).map_err(journal_error)?;
    let mut record =
        central.read_copy_record(&format!("{}.journal", expected.transaction_id()?))?;
    if original_record(&record) != original_record(expected) {
        return Err(refusal("Copy original evidence changed before cleanup"));
    }
    let (parent, stage) = open_directories(&record)?;
    let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
    let mut lock = local.try_lock_recovery().map_err(journal_error)?;
    let mut ledger = if record.cleanup.is_none() {
        pair_matches(&central, &local, &record)?;
        if exists(&path_from_fd(&stage).join(LEDGER))?
            || exists(&path_from_fd(&stage).join(QUARANTINE))?
        {
            return Err(refusal("Copy transaction contains unowned cleanup state"));
        }
        match record.phase {
            CopyPhase::Published => record.validate_retained_backup(&stage)?,
            CopyPhase::Undone => record.validate_completed_data()?,
            _ => return Err(refusal("Copy is not ready for cleanup")),
        }
        mutation_boundary(&central, &local, &parent, &stage, &record, selected, parsed)?;
        let (ledger, intent) = Ledger::create(&stage, &record)?;
        record.phase = CopyPhase::CleanupPending;
        record.cleanup = Some(intent);
        lock.persist_copy_in_registry(&central, &record)
            .map_err(journal_error)?;
        hook(CleanupPoint::IntentDurable)?;
        ledger
    } else {
        reconcile_pair(&central, &local, &stage, &record)?;
        Ledger::open(&stage, &record)?
    };
    let monitor = CleanupMonitor::new(&central, &stage, root, selected)?;
    ledger.validate_named(&stage)?;
    validate_partial(&record, &stage, &ledger.state)?;
    mutation_boundary(&central, &local, &parent, &stage, &record, selected, parsed)?;
    monitor.check(&record, &parent, &stage)?;
    if record.phase == CopyPhase::CleanupComplete {
        return Ok(record);
    }
    let nodes = nodes(&record)?;
    let original = nodes
        .iter()
        .map(|node| (node.relative.clone(), node.objects[0].clone()))
        .collect();
    while ledger.state.index < nodes.len() {
        let node = &nodes[ledger.state.index];
        if !ledger.state.pending {
            ledger.append(ledger.state.index, true)?;
            hook(CleanupPoint::EntryIntentDurable)?;
        }
        ledger.validate_named(&stage)?;
        monitor.check(&record, &parent, &stage)?;
        let quarantine = path_from_fd(&stage).join(QUARANTINE);
        if !exists(&quarantine)? {
            let source = node_source_path(&stage, &node.relative);
            if exists(&source)? {
                record.validate_tree(&source, &node.content, &node.objects)?;
                let (directory, name) = source_parent(&stage, &node.relative, &original)?;
                rustix::fs::renameat_with(
                    &directory,
                    &name,
                    &stage,
                    QUARANTINE,
                    rustix::fs::RenameFlags::NOREPLACE,
                )
                .map_err(journal_error)?;
                rustix::fs::fsync(&directory).map_err(journal_error)?;
                rustix::fs::fsync(&stage).map_err(journal_error)?;
                hook(CleanupPoint::Claimed)?;
            }
        }
        if exists(&quarantine)? {
            hook(CleanupPoint::BeforeUnlink)?;
            ledger.validate_named(&stage)?;
            monitor.check(&record, &parent, &stage)?;
            // Reopen every named ancestor after claim as well. A matching leaf
            // hard link inside a replacement directory is not the old namespace.
            source_parent(&stage, &node.relative, &original)?;
            record.validate_tree(&quarantine, &node.content, &node.objects)?;
            let pinned = rustix::fs::openat(
                &stage,
                QUARANTINE,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(journal_error)?;
            if ObjectHandle::capture(&pinned).map_err(journal_error)? != node.objects[0].handle
                || mount(&pinned)? != record.transaction_mount
            {
                return Err(refusal(
                    "Cleanup quarantine object or mount changed before unlink",
                ));
            }
            let stat = rustix::fs::fstat(&pinned).map_err(journal_error)?;
            let named =
                rustix::fs::statat(&stage, QUARANTINE, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(journal_error)?;
            if (stat.st_dev, stat.st_ino, stat.st_mode)
                != (named.st_dev, named.st_ino, named.st_mode)
            {
                return Err(refusal(
                    "Cleanup quarantine pathname was replaced before unlink",
                ));
            }
            let flags = if rustix::fs::FileType::from_raw_mode(stat.st_mode)
                == rustix::fs::FileType::Directory
            {
                rustix::fs::AtFlags::REMOVEDIR
            } else {
                rustix::fs::AtFlags::empty()
            };
            // Content verification may have taken time. Poll again immediately
            // before unlink so new dependencies or replaced metadata invalidate it.
            ledger.validate_named(&stage)?;
            monitor.check(&record, &parent, &stage)?;
            rustix::fs::unlinkat(&stage, QUARANTINE, flags).map_err(journal_error)?;
            rustix::fs::fsync(&stage).map_err(journal_error)?;
            hook(CleanupPoint::Unlinked)?;
        }
        ledger.append(ledger.state.index + 1, false)?;
        hook(CleanupPoint::ProgressDurable)?;
    }
    ledger.validate_named(&stage)?;
    validate_partial(&record, &stage, &ledger.state)?;
    mutation_boundary(&central, &local, &parent, &stage, &record, selected, parsed)?;
    monitor.check(&record, &parent, &stage)?;
    record.phase = CopyPhase::CleanupComplete;
    lock.persist_copy_in_registry(&central, &record)
        .map_err(journal_error)?;
    hook(CleanupPoint::CompleteDurable)?;
    Ok(record)
}

/// Original-record-bound cleanup selection. No handles are kept between calls,
/// so inspecting cleanup eligibility does not prevent ordinary unmount.
/// Deletion still requires durable intent and per-entry mutation authority.
#[derive(Debug)]
pub struct RetainedCopyCleanupPlan {
    root: PathBuf,
    records: BTreeMap<String, CopyRecord>,
    pending: BTreeSet<String>,
    parsed: CopyRecordCache,
}

impl RetainedCopyCleanupPlan {
    pub fn transaction_ids(&self) -> Vec<&str> {
        self.records.keys().map(String::as_str).collect()
    }

    /// Transactions still requiring payload validation/reclamation or journal
    /// reconciliation in this worker, including completed records on reopen.
    pub fn remaining_payloads(&self) -> usize {
        self.pending.len()
    }

    /// Reclaims one transaction's retained payload, with compact durable
    /// per-entry progress. Journals and the private stage remain until final
    /// history cleanup. A retry resumes only this plan's original evidence.
    pub fn reclaim_payload_next(&mut self) -> Result<bool, PlatformError> {
        self.reclaim_payload_next_with_hook(&mut |_| Ok(()))
    }

    fn reclaim_payload_next_with_hook(
        &mut self,
        hook: &mut impl FnMut(CleanupPoint) -> Result<(), PlatformError>,
    ) -> Result<bool, PlatformError> {
        let Some(id) = self.pending.first().cloned() else {
            return Ok(false);
        };
        let expected = &self.records[&id];
        let selected = self.records.keys().cloned().collect();
        let updated = reclaim_payload(&self.root, expected, &selected, hook, &mut self.parsed)
            .map_err(|error| PlatformError::CleanupPending {
                transaction: expected
                    .transaction
                    .decode()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| id.clone()),
                message: format!(
                    "Copy cleanup is incomplete; preserve its data and progress: {error}"
                ),
            })?;
        self.pending.remove(&id);
        self.records.insert(id, updated);
        if self.pending.is_empty() {
            // Finalization uses the owned records, not the decoding memo.
            self.parsed = CopyRecordCache::default();
        }
        Ok(true)
    }

    /// Persist group-wide finalization authority before removing private
    /// bookkeeping. The manifest remains until all selected journals are gone.
    pub fn begin_finalization(&self) -> Result<RetainedCopyFinalization, PlatformError> {
        finalization::begin(self)
    }

    /// Rechecks original records, complete operation membership, dependencies,
    /// namespace identity and retained contents. Never adopts a newer record.
    pub fn revalidate(&self) -> Result<(), PlatformError> {
        let central =
            LinuxJournalDirectory::open_copy_registry(&self.root, false).map_err(journal_error)?;
        let all = registry_records(&central)?;
        for (id, expected) in &self.records {
            if all.get(id) != Some(expected) {
                return Err(refusal("Copy journal changed after cleanup admission"));
            }
            for (other_id, other) in &all {
                if other.operation.id == expected.operation.id
                    && !self.records.contains_key(other_id)
                {
                    return Err(refusal(
                        "Cleanup requires the complete recorded Copy operation",
                    ));
                }
            }
            validate_retained(&central, expected)?;
            if let Some((_, objects)) = retained_evidence(expected)? {
                for (owner_id, owner) in &all {
                    if owner_id != id
                        && !self.records.contains_key(owner_id)
                        && possible_reference(owner, objects)?
                    {
                        return Err(refusal(
                            "Retained Copy data is still referenced by another operation",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Resume already-authorised cleanup. IDs alone cannot start cleanup: every
/// selected record must carry an existing durable intent and original evidence.
pub fn resume_cleanup(ids: &[String]) -> Result<RetainedCopyCleanupPlan, PlatformError> {
    if ids.is_empty() {
        return Err(refusal("No pending Copy cleanup was selected"));
    }
    let root = configured_root()?;
    let central = LinuxJournalDirectory::open_copy_registry(&root, false).map_err(journal_error)?;
    let all = registry_records(&central)?;
    let mut records = BTreeMap::new();
    let mut parsed = CopyRecordCache::default();
    for id in ids {
        let record = all
            .get(id)
            .ok_or_else(|| refusal("Pending Copy cleanup record is missing"))?;
        if !matches!(
            record.phase,
            CopyPhase::CleanupPending | CopyPhase::CleanupComplete
        ) || record.cleanup.is_none()
        {
            return Err(refusal("Copy record has no prior cleanup intent"));
        }
        if records.insert(id.clone(), record.clone()).is_some() {
            return Err(refusal("Copy cleanup ID selected more than once"));
        }
    }
    for (id, record) in &records {
        let (parent, stage) = open_directories(record)?;
        let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
        let _lock = local.try_lock_recovery().map_err(journal_error)?;
        // Admission only classifies a possible one-step lag. Mutation owns the
        // actual reconciliation, so merely opening a retry does not write.
        let previous = local.read_copy_record("copy-journal")?;
        if previous != *record {
            let forward = matches!(
                (previous.phase, record.phase),
                (
                    CopyPhase::Published | CopyPhase::Undone,
                    CopyPhase::CleanupPending
                ) | (CopyPhase::CleanupPending, CopyPhase::CleanupComplete)
            );
            if !forward || original_record(&previous) != original_record(record) {
                return Err(refusal(
                    "Pending Copy cleanup journals do not have one original owner",
                ));
            }
        }
        validate_partial_data(record, &stage)?;
        mutation_scope(
            &central,
            &parent,
            &stage,
            record,
            &records.keys().cloned().collect(),
            &mut parsed,
        )?;
        if local.read_copy_record("copy-journal")? != previous {
            return Err(refusal(
                "Pending Copy cleanup local journal changed during admission",
            ));
        }
        if central.read_copy_record(&format!("{id}.journal"))? != *record {
            return Err(refusal("Pending Copy cleanup changed during admission"));
        }
    }
    Ok(RetainedCopyCleanupPlan {
        parsed,
        root,
        pending: records.keys().cloned().collect(),
        records,
    })
}

fn possible_reference(owner: &CopyRecord, retained: &[TreeObject]) -> Result<bool, PlatformError> {
    // An ancestor overwrite can contain an older transaction itself, including
    // its backup even when that older publication has already been deleted.
    // Whole-tree publication comparisons cannot establish absence of a reference.
    let mut roots = vec![
        owner.parent_handle.as_ref(),
        owner.transaction_handle.as_ref(),
    ];
    if owner.phase != CopyPhase::Undone {
        roots.push(
            owner
                .publication_objects
                .first()
                .and_then(|object| object.handle.as_ref()),
        );
    }
    for root in roots {
        let root = root.ok_or_else(|| refusal("Copy dependency object evidence is unavailable"))?;
        // A generation handle alone is not a global filesystem identity. A
        // possible match across namespaces is conservatively retained, never
        // used to grant mutation authority or adopt an alternate pathname.
        if retained
            .iter()
            .any(|object| object.handle.as_ref() == Some(root))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn plan_cleanup(
    captured: &[RetainedCopyStatus],
) -> Result<RetainedCopyCleanupPlan, PlatformError> {
    if captured.is_empty() {
        return Err(refusal("No retained Copy transactions were selected"));
    }
    let mut records = BTreeMap::new();
    let mut captured_groups = BTreeMap::<String, BTreeSet<String>>::new();
    let mut ordering = BTreeMap::<String, (usize, BTreeSet<usize>)>::new();
    for status in captured {
        let observed = status
            .observation
            .as_ref()
            .ok_or_else(|| refusal("Copy cleanup requires an original journal observation"))?;
        let record = &observed.record;
        if !status.matches_observation()?
            || records
                .insert(status.transaction_id.clone(), record.clone())
                .is_some()
        {
            return Err(refusal(
                "Copy observation was changed or selected more than once",
            ));
        }
        retained_evidence(record)?;
        let (count, positions) = ordering
            .entry(record.operation.id.clone())
            .or_insert_with(|| (record.operation.item_count, BTreeSet::new()));
        if *count != record.operation.item_count || !positions.insert(record.operation.item_index) {
            return Err(refusal("Copy operation ordering is inconsistent"));
        }
        captured_groups
            .entry(record.operation.id.clone())
            .or_default()
            .insert(status.transaction_id.clone());
    }
    for status in captured {
        let observed = status.observation.as_ref().expect("validated observation");
        let group = &captured_groups[&observed.record.operation.id];
        if group.iter().ne(observed.members.iter()) {
            return Err(refusal(
                "Cleanup requires the complete captured Copy operation",
            ));
        }
    }
    let plan = RetainedCopyCleanupPlan {
        parsed: CopyRecordCache::default(),
        root: configured_root()?,
        pending: records.keys().cloned().collect(),
        records,
    };
    plan.revalidate()?;
    Ok(plan)
}

fn refusal(message: &str) -> PlatformError {
    PlatformError::CopyCleanupRefused(message.into())
}

pub(super) fn registry_records(
    central: &LinuxJournalDirectory,
) -> Result<BTreeMap<String, CopyRecord>, PlatformError> {
    Ok(
        registry_records_cached(central, &mut CopyRecordCache::uncached())?
            .into_iter()
            .map(|(id, record)| (id, std::sync::Arc::unwrap_or_clone(record)))
            .collect(),
    )
}

fn registry_records_cached(
    central: &LinuxJournalDirectory,
    parsed: &mut CopyRecordCache,
) -> Result<BTreeMap<String, std::sync::Arc<CopyRecord>>, PlatformError> {
    central
        .discover_copy_records_with(|name| {
            // Fresh bytes and the existing read_named checks are never skipped.
            parsed.parse(name, central.read_record_bytes(name)?.into())
        })
        .map_err(journal_error)?
        .into_iter()
        .map(|(id, record)| {
            let record = record?;
            if record.transaction_id()? != id {
                return Err(refusal("Copy registry filename differs from its record"));
            }
            Ok((id, record))
        })
        .collect()
}

fn retained_evidence(
    record: &CopyRecord,
) -> Result<Option<(&SourceTreeSnapshot, &[TreeObject])>, PlatformError> {
    let phase = record
        .cleanup
        .as_ref()
        .map_or(record.phase, |intent| intent.origin);
    let evidence: Option<(&SourceTreeSnapshot, &[TreeObject])> = match phase {
        CopyPhase::Published => record
            .displaced_content
            .as_ref()
            .zip(record.displaced_objects.as_deref()),
        CopyPhase::Undone => Some((&record.publication_content, &record.publication_objects)),
        _ => {
            return Err(refusal(
                "Interrupted Copy publication or Undo must be reconciled before cleanup",
            ));
        }
    };
    if evidence.is_some_and(|(_, objects)| {
        objects
            .iter()
            .any(|object| object.mount != record.transaction_mount)
    }) {
        return Err(refusal("Retained Copy cleanup crosses a mount boundary"));
    }
    Ok(evidence)
}

fn validate_retained(
    central: &LinuxJournalDirectory,
    record: &CopyRecord,
) -> Result<(), PlatformError> {
    record.require_object_evidence()?;
    record.validate_namespace()?;
    let transaction = record.transaction.decode().map_err(journal_error)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let parent = rustix::fs::open(
        transaction.parent().expect("validated parent"),
        flags,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    let stage = rustix::fs::openat(
        &parent,
        transaction.file_name().expect("validated stage name"),
        flags,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
    let _lock = local.try_lock_recovery().map_err(journal_error)?;
    if record.cleanup.is_some() {
        check_ordered_pair(central, &local, &stage, record)?;
    } else {
        pair_matches(central, &local, record)?;
    }
    let mut directory = rustix::fs::Dir::read_from(&stage).map_err(journal_error)?;
    while let Some(entry) = directory.read() {
        let entry = entry.map_err(journal_error)?;
        if !matches!(
            entry.file_name().to_bytes(),
            b"." | b".." | b"candidate" | b"copy-journal" | b"copy-cleanup" | b"cleanup-entry"
        ) {
            return Err(refusal(
                "Copy transaction contains an unrecognized retained entry",
            ));
        }
        if record.cleanup.is_none()
            && matches!(
                entry.file_name().to_bytes(),
                b"copy-cleanup" | b"cleanup-entry"
            )
        {
            return Err(refusal("Copy transaction contains unowned cleanup state"));
        }
    }
    match record.phase {
        CopyPhase::CleanupPending | CopyPhase::CleanupComplete => {
            validate_partial_data(record, &stage)?
        }
        CopyPhase::Published => record.validate_retained_backup(&stage)?,
        CopyPhase::Undone => record.validate_completed_data()?,
        _ => return Err(refusal("Copy transaction is not ready for cleanup")),
    }
    record.validate_namespace()?;
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    if record.cleanup.is_some() {
        check_ordered_pair(central, &local, &stage, record)?;
    } else {
        pair_matches(central, &local, record)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConflictAction, DefaultPlatform, PlatformOps};

    pub(super) struct Registry(Option<PathBuf>);
    impl Registry {
        pub(super) fn new(root: &Path) -> Self {
            Self(crate::move_transaction::replace_test_recovery_root(Some(
                root.join("moves"),
            )))
        }
    }
    impl Drop for Registry {
        fn drop(&mut self) {
            crate::move_transaction::replace_test_recovery_root(self.0.take());
        }
    }

    pub(super) fn copy(root: &Path, version: &str) -> crate::PathMoveUndoReceipt {
        let directory = root.join(version);
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("item");
        std::fs::write(&source, version).unwrap();
        let destination = root.join("destination");
        std::fs::create_dir_all(&destination).unwrap();
        DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .unwrap()
            .unwrap()
    }

    pub(super) fn statuses(root: &Path) -> Vec<RetainedCopyStatus> {
        discover_at(&root.join("copies")).unwrap()
    }

    pub(super) fn stage(receipt: &crate::PathMoveUndoReceipt) -> PathBuf {
        receipt.before_path().parent().unwrap().to_path_buf()
    }

    #[test]
    fn cached_registry_reads_reject_changed_and_invalid_bytes_without_reusing_old_values() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let _receipt = copy(root.path(), "cached");
        let central =
            LinuxJournalDirectory::open_copy_registry(&root.path().join("copies"), false).unwrap();
        let mut cache = CopyRecordCache::default();
        let first = registry_records_cached(&central, &mut cache).unwrap();
        let (id, original) = first.iter().next().unwrap();
        let path = root.path().join("copies").join(format!("{id}.journal"));
        let bytes = std::fs::read(&path).unwrap();
        let mut changed = original.as_ref().clone();
        changed.operation.item_count += 1;
        let encoded = serde_json::to_vec(&changed).unwrap();
        assert_eq!(
            encoded.len(),
            bytes.len(),
            "same-length mutation must not hit the old decoded value"
        );
        std::fs::write(&path, encoded).unwrap();
        assert_eq!(
            registry_records_cached(&central, &mut cache).unwrap()[id]
                .operation
                .item_count,
            2
        );
        changed.schema = 0;
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(registry_records_cached(&central, &mut cache).is_err());
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(
            registry_records_cached(&central, &mut cache).unwrap()[id].as_ref(),
            original.as_ref()
        );
    }

    #[test]
    fn payload_monitor_fallback_rechecks_dependencies_without_notifications() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = directory_backup(root.path());
        let selected = statuses(root.path())[0]
            .observation
            .as_ref()
            .unwrap()
            .record
            .clone();
        let source = root.path().join("unrelated-source");
        let destination = root.path().join("unrelated-destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("item"), "unrelated").unwrap();
        let unrelated = DefaultPlatform
            .execute_copy_with_undo_receipt(
                &source.join("item"),
                &destination,
                ConflictAction::Overwrite,
            )
            .unwrap()
            .unwrap();
        let other =
            CopyRecord::parse(&std::fs::read(stage(&unrelated).join("copy-journal")).unwrap())
                .unwrap();
        let canonical = root
            .path()
            .join("copies")
            .join(format!("{}.journal", other.transaction_id().unwrap()));
        let central =
            LinuxJournalDirectory::open_copy_registry(&root.path().join("copies"), false).unwrap();
        let (parent, directory) = open_directories(&selected).unwrap();
        let mut monitor = CleanupMonitor::new(
            &central,
            &directory,
            &root.path().join("copies"),
            &BTreeSet::from([selected.transaction_id().unwrap()]),
        )
        .unwrap();
        monitor.full_revalidation = true;
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&canonical).unwrap()).unwrap();
        value["parent_handle"] =
            serde_json::to_value(&selected.displaced_objects.as_ref().unwrap()[0].handle).unwrap();
        std::fs::write(&canonical, serde_json::to_vec(&value).unwrap()).unwrap();
        // Model remote writes that do not generate local notification events.
        let mut buffer = [0u8; 4096];
        loop {
            match rustix::io::read(&monitor.events, &mut buffer) {
                Ok(_) => {}
                Err(rustix::io::Errno::WOULDBLOCK) => break,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => panic!("{error}"),
            }
        }
        assert!(monitor.check(&selected, &parent, &directory).is_err());
        assert!(receipt.before_path().join("nested/deeper/a").exists());
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn cleanup_observation_refuses_interrupted_phases_without_reading_publications() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "selected");
        let local = stage(&receipt).join("copy-journal");
        let original = CopyRecord::parse(&std::fs::read(&local).unwrap()).unwrap();
        let id = original.transaction_id().unwrap();
        let central = root.path().join("copies").join(format!("{id}.journal"));
        let large = vec![b'x'; 4 * 1024 * 1024];
        std::fs::write(root.path().join("destination/item"), &large).unwrap();
        let read_chars = || {
            std::fs::read_to_string("/proc/thread-self/io")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("rchar: ")
                        .map(|n| n.parse::<u64>().unwrap())
                })
                .unwrap()
        };
        for phase in [CopyPhase::PublicationIntent, CopyPhase::UndoIntent] {
            let mut record = original.clone();
            record.phase = phase;
            let bytes = serde_json::to_vec(&record).unwrap();
            std::fs::write(&local, &bytes).unwrap();
            std::fs::write(&central, &bytes).unwrap();
            let before = read_chars();
            let captured = super::super::observe_cleanup(std::slice::from_ref(&id)).unwrap();
            assert!(plan_cleanup(&captured).is_err());
            assert!(
                read_chars() - before < 1024 * 1024,
                "Cleanup observation hashed a visible publication in unsupported phase {phase:?}"
            );
            assert_eq!(std::fs::read(&local).unwrap(), bytes);
            assert_eq!(std::fs::read(&central).unwrap(), bytes);
            assert_eq!(
                std::fs::read(root.path().join("destination/item")).unwrap(),
                large
            );
        }
    }

    #[test]
    fn cleanup_observation_never_reads_selected_publication_or_unrelated_payloads() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        std::fs::create_dir(root.path().join("destination")).unwrap();
        std::fs::write(root.path().join("destination/item"), "old selected").unwrap();
        let receipt = copy(root.path(), "selected");
        let id = stage(&receipt)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix(".ottrin-txn-")
            .unwrap()
            .to_owned();
        let unrelated = root.path().join("unrelated");
        std::fs::create_dir_all(unrelated.join("destination")).unwrap();
        let large = vec![b'x'; 4 * 1024 * 1024];
        std::fs::write(unrelated.join("destination/item"), &large).unwrap();
        let other = copy(&unrelated, "other");
        std::fs::write(root.path().join("destination/item"), &large).unwrap();
        let read_chars = || {
            std::fs::read_to_string("/proc/thread-self/io")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("rchar: ")
                        .map(|n| n.parse::<u64>().unwrap())
                })
                .unwrap()
        };
        let before = read_chars();
        let observed = super::super::observe_cleanup(&[id]).unwrap();
        assert_eq!(observed[0].evidence, CopyRecoveryEvidence::CleanupReady);
        assert!(super::super::restore_plan(&observed).is_err());
        let mut plan = plan_cleanup(&observed).unwrap();
        assert!(
            read_chars() - before < 1024 * 1024,
            "Cleanup observation read unrelated 4MiB content"
        );
        while plan.reclaim_payload_next().unwrap() {}
        plan.begin_finalization().unwrap().finish().unwrap();
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            large
        );
        assert_eq!(std::fs::read(other.before_path()).unwrap(), large);
    }

    #[test]
    fn explicit_cleanup_readmission_recovers_a_mixed_batch_with_one_sided_intent() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let sources = ["alpha", "beta"].map(|name| {
            std::fs::write(destination.join(name), format!("old {name}")).unwrap();
            let path = root.path().join(name);
            std::fs::write(&path, format!("new {name}")).unwrap();
            path
        });
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &sources,
            &destination,
            ConflictAction::Overwrite,
        );
        assert!(batch.error.is_none());
        let captured = statuses(root.path());
        let mut plan = plan_cleanup(&captured).unwrap();
        let first = plan.records.values().next().unwrap();
        let local = first.transaction.decode().unwrap().join("copy-journal");
        let old = std::fs::read(&local).unwrap();
        let ids = plan
            .transaction_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(
            plan.reclaim_payload_next_with_hook(&mut |point| {
                if point == CleanupPoint::IntentDurable {
                    std::fs::write(&local, &old).unwrap();
                    return Err(refusal("interrupted before local intent"));
                }
                Ok(())
            })
            .is_err()
        );
        drop(plan);
        let observed = super::super::observe_cleanup(&ids).unwrap();
        assert!(
            observed
                .iter()
                .any(|entry| entry.phase == Some(CopyPhase::Published))
        );
        assert!(
            observed
                .iter()
                .any(|entry| entry.phase == Some(CopyPhase::CleanupPending))
        );
        let mut resumed = plan_cleanup(&observed).unwrap();
        assert_eq!(
            std::fs::read(&local).unwrap(),
            old,
            "Observation/admission wrote the lagging journal"
        );
        while resumed.reclaim_payload_next().unwrap() {}
        resumed.begin_finalization().unwrap().finish().unwrap();
        assert!(statuses(root.path()).is_empty());
        for name in ["alpha", "beta"] {
            assert_eq!(
                std::fs::read(destination.join(name)).unwrap(),
                format!("new {name}").as_bytes()
            );
        }
    }

    #[test]
    fn published_cleanup_uses_retained_data_without_reading_changed_publication() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("item"), "old destination").unwrap();
        let receipt = copy(root.path(), "new");
        let visible = vec![b'x'; 4 * 1024 * 1024];
        std::fs::write(destination.join("item"), &visible).unwrap();
        let captured = statuses(root.path());
        assert!(matches!(
            captured[0].evidence,
            CopyRecoveryEvidence::NeedsReview { .. }
        ));
        let local = stage(&receipt).join("copy-journal");
        let before = std::fs::read(&local).unwrap();
        let read_chars = || {
            std::fs::read_to_string("/proc/thread-self/io")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("rchar: ")
                        .map(|n| n.parse::<u64>().unwrap())
                })
                .unwrap()
        };
        let reads_before = read_chars();
        let plan = plan_cleanup(&captured).unwrap();
        plan.revalidate().unwrap();
        assert!(
            read_chars() - reads_before < 1024 * 1024,
            "Cleanup admission read the unrelated 4 MiB visible publication"
        );
        assert_eq!(std::fs::read(&local).unwrap(), before);
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"old destination"
        );
        assert_eq!(std::fs::read(destination.join("item")).unwrap(), visible);
        std::fs::write(receipt.before_path(), "changed retained data").unwrap();
        assert!(plan.revalidate().is_err());
        assert!(plan_cleanup(&statuses(root.path())).is_err());
        assert_eq!(
            std::fs::read(receipt.before_path()).unwrap(),
            b"changed retained data"
        );
    }

    #[test]
    fn completed_undo_cleanup_does_not_adopt_the_reused_destination() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        DefaultPlatform
            .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
            .unwrap();
        std::fs::write(root.path().join("destination/item"), "later work").unwrap();
        let plan = plan_cleanup(&statuses(root.path())).unwrap();
        plan.revalidate().unwrap();
        assert_eq!(std::fs::read(receipt.before_path()).unwrap(), b"new");
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"later work"
        );
        std::fs::write(stage(&receipt).join("unrelated"), "preserve me").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(
            std::fs::read(stage(&receipt).join("unrelated")).unwrap(),
            b"preserve me"
        );
    }

    #[test]
    fn cleanup_requires_the_whole_batch_and_protects_external_copy_dependencies() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let first = copy(root.path(), "first");
        let second = copy(root.path(), "second");
        let captured = statuses(root.path());
        let second_only = captured
            .iter()
            .filter(|entry| entry.transaction.as_ref() == Some(&stage(&second)))
            .map(|entry| entry.transaction_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(second_only.len(), 1);
        let selected = statuses(root.path())
            .into_iter()
            .filter(|entry| second_only.contains(&entry.transaction_id))
            .collect::<Vec<_>>();
        assert!(
            plan_cleanup(&selected)
                .unwrap_err()
                .to_string()
                .contains("another operation")
        );
        let plan = plan_cleanup(&captured).unwrap();
        assert_eq!(plan.transaction_ids().len(), 2);
        plan.revalidate().unwrap();
        assert_eq!(std::fs::read(second.before_path()).unwrap(), b"first");
        assert!(first.before_path().parent().unwrap().exists());

        let sources = ["batch-a", "batch-b"].map(|version| {
            let directory = root.path().join(version);
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("batch-item");
            std::fs::write(&path, version).unwrap();
            path
        });
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &sources,
            &root.path().join("destination"),
            ConflictAction::Overwrite,
        );
        assert!(batch.error.is_none());
        let batch_stages: Vec<_> = batch.undo_receipts.iter().map(stage).collect();
        let selected = statuses(root.path())
            .into_iter()
            .filter(|entry| batch_stages.contains(entry.transaction.as_ref().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 2);
        assert!(plan_cleanup(&selected[..1]).is_err());
        plan_cleanup(&selected).unwrap().revalidate().unwrap();
    }

    #[test]
    fn cleanup_never_adopts_changed_observations_journals_or_stage_paths() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        let mut captured = statuses(root.path());
        captured[0].publication = Some(root.path().join("unrelated"));
        assert!(plan_cleanup(&captured).is_err());
        let plan = plan_cleanup(&statuses(root.path())).unwrap();
        let local = stage(&receipt).join("copy-journal");
        let before = std::fs::read(&local).unwrap();
        std::fs::write(&local, b"unrelated journal").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(std::fs::read(&local).unwrap(), b"unrelated journal");
        std::fs::write(&local, &before).unwrap();
        let original = stage(&receipt);
        let renamed = root.path().join("old-stage");
        std::fs::rename(&original, &renamed).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("user-file"), "preserve me").unwrap();
        assert!(plan.revalidate().is_err());
        assert_eq!(
            std::fs::read(original.join("user-file")).unwrap(),
            b"preserve me"
        );
        assert_eq!(std::fs::read(renamed.join("copy-journal")).unwrap(), before);
    }

    #[test]
    fn cleanup_checks_nested_backup_contents_and_symlink_entries_without_following_targets() {
        for kind in ["directory", "symlink"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let destination = root.path().join("destination");
            let outside = root.path().join("outside");
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("user-file"), "outside data").unwrap();
            let target = destination.join("item");
            if kind == "directory" {
                std::fs::create_dir_all(target.join("nested")).unwrap();
                std::fs::write(target.join("nested/leaf"), "retained data").unwrap();
            } else {
                std::os::unix::fs::symlink(&outside, &target).unwrap();
            }
            let receipt = copy(root.path(), "new");
            std::fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(&outside, &target).unwrap();
            let plan = plan_cleanup(&statuses(root.path())).unwrap();
            if kind == "directory" {
                std::fs::write(receipt.before_path().join("nested/leaf"), "changed data").unwrap();
            } else {
                std::fs::remove_file(receipt.before_path()).unwrap();
                std::os::unix::fs::symlink(root.path().join("other-target"), receipt.before_path())
                    .unwrap();
            }
            assert!(plan.revalidate().is_err());
            assert_eq!(
                std::fs::read(outside.join("user-file")).unwrap(),
                b"outside data"
            );
            assert_eq!(std::fs::read_link(&target).unwrap(), outside);
        }
    }

    #[test]
    fn cleanup_rejects_changed_central_records_and_interrupted_phases() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = copy(root.path(), "new");
        let captured = statuses(root.path());
        let id = captured[0].transaction_id.clone();
        let plan = plan_cleanup(&captured).unwrap();
        let central_path = root.path().join("copies").join(format!("{id}.journal"));
        let original = std::fs::read(&central_path).unwrap();
        let record = CopyRecord::parse(&original).unwrap();
        let local_path = stage(&receipt).join("copy-journal");
        for phase in [CopyPhase::PublicationIntent, CopyPhase::UndoIntent] {
            let mut changed = record.clone();
            changed.phase = phase;
            let bytes = serde_json::to_vec(&changed).unwrap();
            std::fs::write(&central_path, &bytes).unwrap();
            assert!(plan.revalidate().is_err());
            std::fs::write(&local_path, &bytes).unwrap();
            let observed = statuses(root.path());
            assert!(
                plan_cleanup(&observed)
                    .unwrap_err()
                    .to_string()
                    .contains("Interrupted")
            );
            assert_eq!(std::fs::read(&central_path).unwrap(), bytes);
            assert_eq!(std::fs::read(&local_path).unwrap(), bytes);
        }
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn ancestor_overwrite_cannot_discard_a_nested_transactions_private_backup() {
        for keep_publication in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let destination = root.path().join("destination");
            let outer = destination.join("outer");
            std::fs::create_dir_all(&outer).unwrap();
            std::fs::write(outer.join("item"), "before first copy").unwrap();
            let source = root.path().join("source");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("item"), "first copy").unwrap();
            let first = DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &source.join("item"),
                    &outer,
                    ConflictAction::Overwrite,
                )
                .unwrap()
                .unwrap();
            if !keep_publication {
                std::fs::remove_file(outer.join("item")).unwrap();
            }
            let replacement = root.path().join("replacement/outer");
            std::fs::create_dir_all(&replacement).unwrap();
            std::fs::write(replacement.join("new"), "second copy").unwrap();
            let second = DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &replacement,
                    &destination,
                    ConflictAction::Overwrite,
                )
                .unwrap()
                .unwrap();
            let nested_backup = second
                .before_path()
                .join(stage(&first).file_name().unwrap())
                .join("candidate");
            assert_eq!(std::fs::read(&nested_backup).unwrap(), b"before first copy");
            let selected = statuses(root.path())
                .into_iter()
                .filter(|entry| entry.transaction.as_ref() == Some(&stage(&second)))
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            assert!(
                plan_cleanup(&selected)
                    .unwrap_err()
                    .to_string()
                    .contains("another operation")
            );
            assert_eq!(std::fs::read(nested_backup).unwrap(), b"before first copy");
            assert_eq!(std::fs::read(outer.join("new")).unwrap(), b"second copy");
        }
    }

    #[test]
    fn payload_reclamation_preserves_visible_edits_and_revokes_undo() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("item"), "old backup").unwrap();
        let receipt = copy(root.path(), "new");
        std::fs::write(destination.join("item"), "later visible edit").unwrap();
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        assert!(plan.reclaim_payload_next().unwrap());
        assert_eq!(plan.remaining_payloads(), 0);
        assert!(!plan.reclaim_payload_next().unwrap());
        assert!(!receipt.before_path().exists());
        assert_eq!(
            std::fs::read(destination.join("item")).unwrap(),
            b"later visible edit"
        );
        assert!(
            DefaultPlatform
                .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
                .is_err()
        );
        let observed = statuses(root.path());
        assert_eq!(observed[0].phase, Some(CopyPhase::CleanupComplete));
        assert_eq!(observed[0].evidence, CopyRecoveryEvidence::CleanupComplete);
        plan.revalidate().unwrap();
        assert!(stage(&receipt).join("copy-journal").exists());
        assert!(stage(&receipt).join(LEDGER).exists());
    }

    pub(super) fn directory_backup(root: &Path) -> crate::PathMoveUndoReceipt {
        let destination = root.join("destination");
        std::fs::create_dir_all(destination.join("item/nested/deeper")).unwrap();
        std::fs::write(destination.join("item/nested/deeper/a"), "retained a").unwrap();
        std::fs::write(destination.join("item/z"), "retained z").unwrap();
        let outside = root.join("outside");
        std::fs::write(&outside, "outside user data").unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("item/link")).unwrap();
        copy(root, "new")
    }

    #[test]
    fn payload_cleanup_refuses_dependency_changes_through_an_outside_journal_alias_before_unlink() {
        let root = tempfile::tempdir().unwrap();
        payload_dependency_change_fixture(root.path(), false);
    }

    pub(super) fn payload_dependency_change_fixture(root: &Path, bind: bool) {
        let _registry = Registry::new(root);
        let receipt = directory_backup(root);
        let captured = statuses(root);
        let selected = &captured[0].observation.as_ref().unwrap().record;
        let reference = selected.displaced_objects.as_ref().unwrap()[0]
            .handle
            .clone();
        let mut plan = plan_cleanup(&captured).unwrap();
        let source = root.join("unrelated-source");
        let destination = root.join("unrelated-destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("item"), "unrelated").unwrap();
        let unrelated = DefaultPlatform
            .execute_copy_with_undo_receipt(
                &source.join("item"),
                &destination,
                ConflictAction::Overwrite,
            )
            .unwrap()
            .unwrap();
        let record =
            CopyRecord::parse(&std::fs::read(stage(&unrelated).join("copy-journal")).unwrap())
                .unwrap();
        let canonical = root
            .join("copies")
            .join(format!("{}.journal", record.transaction_id().unwrap()));
        let alias = root.join("outside-journal-alias");
        std::fs::hard_link(&canonical, &alias).unwrap();
        let original = std::fs::read(&canonical).unwrap();
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        changed["parent_handle"] = serde_json::to_value(reference).unwrap();
        let bytes = serde_json::to_vec(&changed).unwrap();
        let quarantine = stage(&receipt).join(QUARANTINE);
        let mut attacked = false;
        let mut inode = 0;
        assert!(
            plan.reclaim_payload_next_with_hook(&mut |point| {
                if !attacked && point == CleanupPoint::BeforeUnlink {
                    inode = std::fs::symlink_metadata(&quarantine).unwrap().ino();
                    attacked = true;
                    if bind {
                        let path = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(
                            canonical.as_os_str(),
                        ))
                        .unwrap();
                        // Dedicated private mount-namespace child only.
                        assert_eq!(
                            unsafe {
                                libc::mount(
                                    path.as_ptr(),
                                    path.as_ptr(),
                                    std::ptr::null(),
                                    libc::MS_BIND,
                                    std::ptr::null(),
                                )
                            },
                            0
                        );
                    } else {
                        std::fs::write(&alias, &bytes).unwrap();
                    }
                }
                Ok(())
            })
            .is_err()
        );
        assert!(attacked);
        assert_eq!(
            std::fs::symlink_metadata(&quarantine)
                .expect(
                    "changed dependency must be refused before deleting the claimed retained object"
                )
                .ino(),
            inode
        );
        assert_eq!(
            std::fs::read(&canonical).unwrap(),
            if bind { original } else { bytes }
        );
        if bind {
            let path = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(
                canonical.as_os_str(),
            ))
            .unwrap();
            assert_eq!(unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) }, 0);
        }
        assert_eq!(
            std::fs::read(root.join("outside")).unwrap(),
            b"outside user data"
        );
        assert_eq!(
            std::fs::read(root.join("destination/item")).unwrap(),
            b"new"
        );
        assert_eq!(std::fs::read(source.join("item")).unwrap(), b"unrelated");
        assert_eq!(
            std::fs::read(destination.join("item")).unwrap(),
            b"unrelated"
        );
    }

    #[test]
    fn recursive_payload_cleanup_uses_compact_progress_and_never_follows_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = directory_backup(root.path());
        let captured = statuses(root.path());
        let original = captured[0].observation.as_ref().unwrap().record.clone();
        let count = nodes(&original).unwrap().len();
        let mut plan = plan_cleanup(&captured).unwrap();
        assert!(plan.reclaim_payload_next().unwrap());
        assert!(!receipt.before_path().exists());
        assert_eq!(
            std::fs::read(root.path().join("outside")).unwrap(),
            b"outside user data"
        );
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"new"
        );
        let observed = statuses(root.path());
        let finished = &observed[0].observation.as_ref().unwrap().record;
        assert_eq!(original_record(finished), original);
        let ledger = std::fs::read(stage(&receipt).join(LEDGER)).unwrap();
        assert!(
            ledger.len() < 4096 + count * 256,
            "Progress repeatedly wrote the full tree: {} bytes",
            ledger.len()
        );
    }

    #[test]
    fn payload_cleanup_restarts_at_each_durable_boundary_without_repeating_unlinks() {
        for point in [
            CleanupPoint::IntentDurable,
            CleanupPoint::EntryIntentDurable,
            CleanupPoint::Claimed,
            CleanupPoint::BeforeUnlink,
            CleanupPoint::Unlinked,
            CleanupPoint::ProgressDurable,
            CleanupPoint::CompleteDurable,
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let receipt = directory_backup(root.path());
            let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
            let ids = plan
                .transaction_ids()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut stopped = false;
            assert!(
                plan.reclaim_payload_next_with_hook(&mut |observed| {
                    if observed == point && !stopped {
                        stopped = true;
                        return Err(refusal("owned interruption"));
                    }
                    Ok(())
                })
                .is_err()
            );
            assert!(stopped);
            drop(plan);
            let ledger_before = std::fs::read(stage(&receipt).join(LEDGER)).unwrap();
            let mut resumed = resume_cleanup(&ids).unwrap();
            assert_eq!(
                std::fs::read(stage(&receipt).join(LEDGER)).unwrap(),
                ledger_before,
                "Read-only resume modified progress"
            );
            while resumed.reclaim_payload_next().unwrap() {}
            assert!(!receipt.before_path().exists(), "{point:?}");
            assert_eq!(
                std::fs::read(root.path().join("outside")).unwrap(),
                b"outside user data"
            );
            assert_eq!(
                std::fs::read(root.path().join("destination/item")).unwrap(),
                b"new"
            );
            resumed.revalidate().unwrap();
        }
    }

    #[test]
    fn changed_quarantine_and_ledger_are_retained_instead_of_deleted() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("item"), "original backup").unwrap();
        let receipt = copy(root.path(), "new");
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        let ids = plan
            .transaction_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(
            plan.reclaim_payload_next_with_hook(&mut |point| {
                if point == CleanupPoint::BeforeUnlink {
                    std::fs::write(stage(&receipt).join(QUARANTINE), "changed backup").unwrap();
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(
            std::fs::read(stage(&receipt).join(QUARANTINE)).unwrap(),
            b"changed backup"
        );
        assert!(resume_cleanup(&ids).is_err());
        std::fs::write(stage(&receipt).join(QUARANTINE), "original backup").unwrap();
        let original = std::fs::read(stage(&receipt).join(LEDGER)).unwrap();
        let mut damaged = original.clone();
        damaged.extend_from_slice(b"partial frame");
        std::fs::write(stage(&receipt).join(LEDGER), &damaged).unwrap();
        assert!(resume_cleanup(&ids).is_err());
        assert_eq!(
            std::fs::read(stage(&receipt).join(LEDGER)).unwrap(),
            damaged
        );
        assert_eq!(
            std::fs::read(stage(&receipt).join(QUARANTINE)).unwrap(),
            b"original backup"
        );
    }

    #[test]
    fn payload_cleanup_reconciles_one_sided_intent_and_completion_journals() {
        for point in [CleanupPoint::IntentDurable, CleanupPoint::CompleteDurable] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let receipt = directory_backup(root.path());
            let local_path = stage(&receipt).join("copy-journal");
            let mut previous = std::fs::read(&local_path).unwrap();
            let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
            let ids = plan
                .transaction_ids()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert!(
                plan.reclaim_payload_next_with_hook(&mut |observed| {
                    if observed == CleanupPoint::IntentDurable
                        && point == CleanupPoint::CompleteDurable
                    {
                        previous = std::fs::read(&local_path).unwrap();
                    }
                    if observed == point {
                        // Canonical writer is first. Restore the preceding local
                        // record to model termination between the two writes.
                        std::fs::write(&local_path, &previous).unwrap();
                        return Err(refusal("one-sided durable write"));
                    }
                    Ok(())
                })
                .is_err()
            );
            let mut resumed = resume_cleanup(&ids).unwrap();
            assert_eq!(std::fs::read(&local_path).unwrap(), previous);
            while resumed.reclaim_payload_next().unwrap() {}
            resumed.revalidate().unwrap();
            assert!(!receipt.before_path().exists());
            assert_eq!(
                std::fs::read(root.path().join("destination/item")).unwrap(),
                b"new"
            );
        }
    }

    #[test]
    fn large_tree_cleanup_does_not_reread_full_journals_for_every_entry() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination/item");
        std::fs::create_dir_all(&destination).unwrap();
        for index in 0..128 {
            std::fs::write(
                destination.join(format!("entry-{index:03}")),
                "small retained data",
            )
            .unwrap();
        }
        let receipt = copy(root.path(), "new");
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        let journal_size = std::fs::metadata(stage(&receipt).join("copy-journal"))
            .unwrap()
            .len();
        let read_chars = || {
            std::fs::read_to_string("/proc/thread-self/io")
                .unwrap()
                .lines()
                .find_map(|line| {
                    line.strip_prefix("rchar: ")
                        .map(|n| n.parse::<u64>().unwrap())
                })
                .unwrap()
        };
        let before = read_chars();
        plan.reclaim_payload_next().unwrap();
        let reads = read_chars() - before;
        assert!(
            reads < journal_size * 20 + 512 * 1024,
            "Cleanup repeatedly read full-tree journals: {reads} bytes for {journal_size}-byte record"
        );
        assert!(!receipt.before_path().exists());
    }

    #[test]
    fn active_cleanup_refuses_ledger_and_metadata_mutation_before_unlink() {
        for attack in [
            "ledger-write",
            "ledger-replace",
            "local-journal",
            "registry-entry",
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let receipt = directory_backup(root.path());
            let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
            let mut attacked = false;
            let result = plan.reclaim_payload_next_with_hook(&mut |point| {
                if point == CleanupPoint::BeforeUnlink && !attacked {
                    attacked = true;
                    let private = stage(&receipt);
                    match attack {
                        "ledger-write" => {
                            let ledger = private.join(LEDGER);
                            let mut bytes = std::fs::read(&ledger).unwrap();
                            bytes[0] ^= 1;
                            std::fs::write(ledger, bytes).unwrap();
                        }
                        "ledger-replace" => {
                            let ledger = private.join(LEDGER);
                            let bytes = std::fs::read(&ledger).unwrap();
                            std::fs::rename(&ledger, root.path().join("old-ledger")).unwrap();
                            std::fs::write(ledger, bytes).unwrap();
                        }
                        "local-journal" => {
                            let journal = private.join("copy-journal");
                            let bytes = std::fs::read(&journal).unwrap();
                            // Even rewriting identical bytes invalidates the active proof.
                            std::fs::write(journal, bytes).unwrap();
                        }
                        "registry-entry" => {
                            std::fs::write(root.path().join("copies/unknown"), "foreign").unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
                Ok(())
            });
            assert!(attacked);
            assert!(result.is_err(), "{attack}");
            assert_eq!(
                std::fs::read(stage(&receipt).join(QUARANTINE)).unwrap(),
                b"retained z"
            );
            assert_eq!(
                std::fs::read(root.path().join("destination/item")).unwrap(),
                b"new"
            );
            assert_eq!(
                std::fs::read(root.path().join("outside")).unwrap(),
                b"outside user data"
            );
        }
    }

    #[test]
    fn cleanup_sigkill_child() {
        let Some(root) = std::env::var_os("OTTRIN_CLEANUP_KILL_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let point = std::env::var("OTTRIN_CLEANUP_KILL_POINT").unwrap();
        let _registry = Registry::new(&root);
        let mut plan = plan_cleanup(&statuses(&root)).unwrap();
        plan.reclaim_payload_next_with_hook(&mut |observed| {
            if format!("{observed:?}") == point {
                // This runs only in the dedicated subprocess and bypasses Drop,
                // proving durable progress rather than error-return recovery.
                unsafe {
                    libc::kill(libc::getpid(), libc::SIGKILL);
                }
                unreachable!("SIGKILL returned");
            }
            Ok(())
        })
        .unwrap();
        panic!("Child did not reach requested cleanup boundary");
    }

    #[test]
    fn fresh_process_resumes_payload_after_sigkill_at_durable_boundaries() {
        use std::os::unix::process::ExitStatusExt;
        for point in [
            CleanupPoint::IntentDurable,
            CleanupPoint::EntryIntentDurable,
            CleanupPoint::Claimed,
            CleanupPoint::BeforeUnlink,
            CleanupPoint::Unlinked,
            CleanupPoint::ProgressDurable,
            CleanupPoint::CompleteDurable,
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let receipt = directory_backup(root.path());
            let ids = statuses(root.path())
                .into_iter()
                .map(|entry| entry.transaction_id)
                .collect::<Vec<_>>();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "copy_recovery::cleanup::tests::cleanup_sigkill_child",
                ])
                .env("OTTRIN_CLEANUP_KILL_ROOT", root.path())
                .env("OTTRIN_CLEANUP_KILL_POINT", format!("{point:?}"))
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.signal(), Some(libc::SIGKILL), "{point:?}");
            let mut plan = resume_cleanup(&ids).unwrap();
            while plan.reclaim_payload_next().unwrap() {}
            plan.revalidate().unwrap();
            assert!(!receipt.before_path().exists());
            assert_eq!(
                std::fs::read(root.path().join("outside")).unwrap(),
                b"outside user data"
            );
            assert_eq!(
                std::fs::read(root.path().join("destination/item")).unwrap(),
                b"new"
            );
        }
    }

    #[test]
    fn root_symlink_cleanup_removes_only_the_link_entry() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let destination = root.path().join("destination");
        let outside = root.path().join("outside");
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("user-file"), "outside data").unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("item")).unwrap();
        let receipt = copy(root.path(), "new");
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        plan.reclaim_payload_next().unwrap();
        assert!(std::fs::symlink_metadata(receipt.before_path()).is_err());
        assert_eq!(
            std::fs::read(outside.join("user-file")).unwrap(),
            b"outside data"
        );
        assert_eq!(std::fs::read(destination.join("item")).unwrap(), b"new");
    }

    #[test]
    fn ancestor_replacement_after_claim_preserves_quarantined_data() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = directory_backup(root.path());
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        let moved = root.path().join("moved-original");
        let mut replaced = false;
        assert!(
            plan.reclaim_payload_next_with_hook(&mut |point| {
                if point == CleanupPoint::Claimed && !replaced {
                    replaced = true;
                    std::fs::rename(receipt.before_path(), &moved).unwrap();
                    std::fs::create_dir(receipt.before_path()).unwrap();
                    std::fs::hard_link(
                        moved.join("nested/deeper/a"),
                        receipt.before_path().join("alias"),
                    )
                    .unwrap();
                }
                Ok(())
            })
            .is_err()
        );
        assert!(replaced);
        assert_eq!(
            std::fs::read(stage(&receipt).join(QUARANTINE)).unwrap(),
            b"retained z"
        );
        assert_eq!(
            std::fs::read(receipt.before_path().join("alias")).unwrap(),
            b"retained a"
        );
        assert_eq!(
            std::fs::read(moved.join("nested/deeper/a")).unwrap(),
            b"retained a"
        );
    }
}
