//! Durable evidence for destination-local Copy transactions.
//!
//! Records describe publication intent and its completion. They do not grant
//! restart Undo or deletion authority: recovery must validate both namespace
//! bindings and contents before making any change.

use super::move_recovery::{NativePath, SourceTreeSnapshot};
use super::{PathMoveIdentity, PlatformError};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Identity-only filesystem evidence, scoped by the record's boot and unique
/// mount IDs. Its filesystem-defined identifier can distinguish reused inode
/// numbers. No descriptor is retained between sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectHandle {
    kind: i32,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TreeObject {
    path: NativePath,
    mount: u64,
    handle: Option<ObjectHandle>,
}

pub(super) fn tree_evidence(
    path: &Path,
) -> Result<(SourceTreeSnapshot, Vec<TreeObject>), PlatformError> {
    sample_tree(path, None)
}

fn sample_tree(
    path: &Path,
    expected: Option<&[TreeObject]>,
) -> Result<(SourceTreeSnapshot, Vec<TreeObject>), PlatformError> {
    use rustix::fd::AsRawFd;
    let expected = expected
        .map(|objects| {
            objects
                .iter()
                .map(|object| Ok((object.path.decode().map_err(journal_error)?, object)))
                .collect::<Result<std::collections::BTreeMap<_, _>, PlatformError>>()
        })
        .transpose()?;
    let mut objects = Vec::new();
    let content =
        super::move_transaction::source_tree_snapshot_with_observer(path, &mut |relative, fd| {
            let object = TreeObject {
                path: NativePath::encode(relative),
                mount: super::local_storage::local_path_mount_identity(
                    &PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd())),
                    true,
                )
                .map_err(journal_error)?,
                handle: ObjectHandle::capture(fd).map_err(journal_error)?,
            };
            if let Some(expected) = &expected
                && expected.get(relative).copied() != Some(&object)
            {
                return Err(PlatformError::Io(
                    "Copy tree objects or mount bindings changed".into(),
                ));
            }
            objects.push((relative.to_path_buf(), object));
            Ok(())
        })?;
    objects.sort_by(|(left, _), (right, _)| left.cmp(right));
    Ok((
        content,
        objects.into_iter().map(|(_, object)| object).collect(),
    ))
}

fn valid_objects(content: &SourceTreeSnapshot, objects: &[TreeObject]) -> bool {
    let Ok(paths) = content.relative_paths() else {
        return false;
    };
    paths.len() == objects.len()
        && paths.into_iter().zip(objects).all(|(path, object)| {
            object.path.decode().ok().as_ref() == Some(&path)
                && object.mount != 0
                && object.handle.as_ref().is_none_or(ObjectHandle::valid)
        })
}

impl ObjectHandle {
    fn capture(fd: &impl rustix::fd::AsFd) -> std::io::Result<Option<Self>> {
        use rustix::fd::AsRawFd;
        // file_handle has a flexible byte array. This storage supplies its
        // correctly aligned header and Linux's current MAX_HANDLE_SZ capacity.
        #[repr(C)]
        struct Storage {
            size: libc::c_uint,
            kind: libc::c_int,
            bytes: [u8; 128],
        }
        let mut storage = Storage {
            size: 128,
            kind: 0,
            bytes: [0; 128],
        };
        let mut mount_id = 0;
        // SAFETY: fd is live; the empty C string and both output buffers remain
        // valid for the call. Storage matches file_handle's header layout and
        // provides the advertised byte capacity. AT_EMPTY_PATH addresses the
        // opened object, including a symlink opened with O_PATH | O_NOFOLLOW.
        let result = unsafe {
            libc::syscall(
                libc::SYS_name_to_handle_at,
                fd.as_fd().as_raw_fd(),
                c"".as_ptr(),
                &mut storage as *mut Storage,
                &mut mount_id as *mut libc::c_int,
                // Linux UAPI AT_HANDLE_FID aliases AT_REMOVEDIR (0x200).
                libc::AT_EMPTY_PATH | libc::AT_REMOVEDIR,
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::EOPNOTSUPP | libc::ENOSYS) => Ok(None),
                // An unchanged capacity means this particular object cannot
                // supply a handle; an increased capacity is a real size error.
                Some(libc::EOVERFLOW) if storage.size <= 128 => Ok(None),
                _ => Err(error),
            };
        }
        if storage.size == 0 || storage.size > 128 {
            return Err(std::io::Error::other(
                "invalid filesystem object handle size",
            ));
        }
        Ok(Some(Self {
            kind: storage.kind,
            bytes: storage.bytes[..storage.size as usize].to_vec(),
        }))
    }

    fn valid(&self) -> bool {
        !self.bytes.is_empty() && self.bytes.len() <= 128
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopyPhase {
    PublicationIntent,
    Published,
    UndoIntent,
    Undone,
}

/// Durable grouping and original ordering for a Copy operation. Missing item
/// records can mean Skip, failure, or interruption; this is not completion proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyOperation {
    pub id: String,
    pub item_index: usize,
    pub item_count: usize,
}

impl CopyOperation {
    pub(super) fn new_batch(item_count: usize) -> Result<Self, PlatformError> {
        let operation = Self {
            id: super::copy_transaction::random_transaction_id()?,
            item_index: 0,
            item_count,
        };
        operation.validate()?;
        Ok(operation)
    }

    fn validate(&self) -> Result<(), PlatformError> {
        if !valid_transaction_id(&self.id) || self.item_index >= self.item_count {
            return Err(PlatformError::Io(
                "invalid Copy operation identity or ordering".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CopyRecord {
    schema: u16,
    phase: CopyPhase,
    operation: CopyOperation,
    source: NativePath,
    publication: NativePath,
    transaction: NativePath,
    parent_identity: PathMoveIdentity,
    transaction_identity: PathMoveIdentity,
    parent_mount: u64,
    transaction_mount: u64,
    parent_handle: Option<ObjectHandle>,
    transaction_handle: Option<ObjectHandle>,
    boot_id: String,
    publication_content: SourceTreeSnapshot,
    publication_objects: Vec<TreeObject>,
    displaced_content: Option<SourceTreeSnapshot>,
    displaced_objects: Option<Vec<TreeObject>>,
}

pub(super) type CopyRegistryEntry = (String, Result<CopyRecord, PlatformError>);

impl CopyRecord {
    fn validate_opened_directory(
        &self,
        fd: &impl rustix::fd::AsFd,
        parent: bool,
    ) -> Result<(), PlatformError> {
        use rustix::fd::AsRawFd;
        let (identity, mount, handle) = if parent {
            (
                &self.parent_identity,
                self.parent_mount,
                &self.parent_handle,
            )
        } else {
            (
                &self.transaction_identity,
                self.transaction_mount,
                &self.transaction_handle,
            )
        };
        let metadata = rustix::fs::fstat(fd).map_err(journal_error)?;
        let opened_mount = super::local_storage::local_path_mount_identity(
            &PathBuf::from(format!("/proc/self/fd/{}", fd.as_fd().as_raw_fd())),
            true,
        )
        .map_err(journal_error)?;
        if rustix::fs::FileType::from_raw_mode(metadata.st_mode) != rustix::fs::FileType::Directory
            || (metadata.st_dev, metadata.st_ino) != (identity.device, identity.inode)
            || opened_mount != mount
            || (!parent
                && (metadata.st_uid != rustix::process::geteuid().as_raw()
                    || metadata.st_mode & 0o777 != 0o700))
        {
            return Err(PlatformError::Io(
                "opened Copy directory does not match its journal".into(),
            ));
        }
        if let Some(expected) = handle
            && ObjectHandle::capture(fd).map_err(journal_error)?.as_ref() != Some(expected)
        {
            return Err(PlatformError::Io(
                "opened Copy directory object generation changed".into(),
            ));
        }
        Ok(())
    }
    pub(super) fn publication_intent(
        source: &Path,
        publication: &Path,
        transaction: &Path,
        publication_evidence: (SourceTreeSnapshot, Vec<TreeObject>),
        directories: [&rustix::fd::OwnedFd; 2],
        operation: Option<&CopyOperation>,
    ) -> Result<Self, PlatformError> {
        let parent = transaction
            .parent()
            .ok_or_else(|| PlatformError::InvalidDestination(transaction.display().to_string()))?;
        use rustix::fd::AsRawFd;
        let mut bindings = Vec::with_capacity(2);
        for (path, directory) in [parent, transaction].into_iter().zip(directories) {
            let observed = rustix::fs::fstat(directory)
                .map_err(|error| PlatformError::Io(error.to_string()))?;
            let identity = PathMoveIdentity {
                device: observed.st_dev,
                inode: observed.st_ino,
            };
            let mount = super::local_storage::local_path_mount_identity(
                &std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())),
                true,
            )
            .map_err(|error| PlatformError::Io(error.to_string()))?;
            if super::rename_identity(path).map_err(|error| PlatformError::Io(error.to_string()))?
                != identity
                || super::local_storage::local_path_mount_identity(path, false)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
                    != mount
            {
                return Err(PlatformError::Io(
                    "Copy journal directory binding changed".into(),
                ));
            }
            let handle = ObjectHandle::capture(directory).map_err(journal_error)?;
            bindings.push((identity, mount, handle));
        }
        let [
            (parent_identity, parent_mount, parent_handle),
            (transaction_identity, transaction_mount, transaction_handle),
        ] = <[_; 2]>::try_from(bindings).expect("two verified Copy directories");
        let source_parent = source
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let source = std::fs::canonicalize(source_parent)
            .map_err(|error| PlatformError::Io(error.to_string()))?
            .join(
                source
                    .file_name()
                    .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?,
            );
        let operation = operation.cloned().unwrap_or_else(|| CopyOperation {
            id: transaction
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(".ottrin-txn-"))
                .unwrap_or_default()
                .to_owned(),
            item_index: 0,
            item_count: 1,
        });
        operation.validate()?;
        Ok(Self {
            schema: 4,
            phase: CopyPhase::PublicationIntent,
            operation,
            source: NativePath::encode(&source),
            publication: NativePath::encode(publication),
            transaction: NativePath::encode(transaction),
            parent_identity,
            transaction_identity,
            parent_mount,
            transaction_mount,
            parent_handle,
            transaction_handle,
            boot_id: std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .map_err(|error| PlatformError::Io(error.to_string()))?
                .trim()
                .to_owned(),
            publication_content: publication_evidence.0,
            publication_objects: publication_evidence.1,
            displaced_content: None,
            displaced_objects: None,
        })
    }

    pub(super) fn set_target(&mut self, target: &Path) {
        self.publication = NativePath::encode(target);
    }

    pub(super) fn set_displaced(&mut self, evidence: (SourceTreeSnapshot, Vec<TreeObject>)) {
        self.displaced_content = Some(evidence.0);
        self.displaced_objects = Some(evidence.1);
    }

    pub(super) fn clear_displaced(&mut self) {
        self.displaced_content = None;
        self.displaced_objects = None;
    }

    pub(super) fn mark_published(&mut self) {
        self.phase = CopyPhase::Published;
    }
}

impl CopyRecord {
    pub(super) fn transaction_id(&self) -> Result<String, PlatformError> {
        let path = self.transaction.decode().map_err(journal_error)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(".ottrin-txn-"))
            .filter(|name| valid_transaction_id(name))
            .ok_or_else(|| PlatformError::Io("invalid Copy transaction name".into()))?;
        Ok(name.to_owned())
    }

    pub(super) fn parse(encoded: &[u8]) -> Result<Self, PlatformError> {
        let record: Self = serde_json::from_slice(encoded).map_err(journal_error)?;
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), PlatformError> {
        let source = self.source.decode().map_err(journal_error)?;
        let publication = self.publication.decode().map_err(journal_error)?;
        let transaction = self.transaction.decode().map_err(journal_error)?;
        self.operation.validate()?;
        if self.schema != 4
            || !valid_objects(&self.publication_content, &self.publication_objects)
            || self.displaced_content.is_some() != self.displaced_objects.is_some()
            || self
                .displaced_content
                .as_ref()
                .zip(self.displaced_objects.as_ref())
                .is_some_and(|(content, objects)| !valid_objects(content, objects))
            || [&self.parent_handle, &self.transaction_handle]
                .into_iter()
                .flatten()
                .any(|handle| !handle.valid())
            || [
                source.as_path(),
                publication.as_path(),
                transaction.as_path(),
            ]
            .into_iter()
            .any(|p| {
                !p.is_absolute()
                    || p.components()
                        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            })
            || publication.parent() != transaction.parent()
            || publication == transaction
            || self.boot_id.is_empty()
            || !self.publication_content.has_complete_content_evidence()
            || self
                .displaced_content
                .as_ref()
                .is_some_and(|c| !c.has_complete_content_evidence())
        {
            return Err(PlatformError::Io(
                "invalid Copy journal schema, paths or content evidence".into(),
            ));
        }
        self.transaction_id()?;
        Ok(())
    }
}

fn journal_error(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(error.to_string())
}

pub(super) fn valid_transaction_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(super) fn restore_receipt(id: &str) -> Result<super::PathMoveUndoReceipt, PlatformError> {
    use super::move_recovery::LinuxJournalDirectory;
    use rustix::fs::{Mode, OFlags};
    if !valid_transaction_id(id) {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "invalid Copy transaction ID".into(),
        ));
    }
    let central = LinuxJournalDirectory::open_copy_registry(&configured_root()?, false)
        .map_err(journal_error)?;
    let record = central.read_copy_record(&format!("{id}.journal"))?;
    if record.transaction_id()? != id {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Copy registry filename does not match transaction".into(),
        ));
    }
    record.require_object_evidence()?;
    record.validate_namespace()?;
    let transaction = record.transaction.decode().map_err(journal_error)?;
    let publication = record.publication.decode().map_err(journal_error)?;
    let parent_path = transaction.parent().expect("validated transaction parent");
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let parent = std::fs::File::from(
        rustix::fs::open(parent_path, flags, Mode::empty()).map_err(journal_error)?,
    );
    let stage = std::fs::File::from(
        rustix::fs::openat(
            &parent,
            transaction.file_name().expect("validated stage name"),
            flags,
            Mode::empty(),
        )
        .map_err(journal_error)?,
    );
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
    let _lock = local.try_lock_recovery().map_err(journal_error)?;
    if local.read_copy_record("copy-journal")? != record
        || central.read_copy_record(&format!("{id}.journal"))? != record
    {
        return Err(PlatformError::Io(
            "central and destination-local Copy journals differ or changed during restoration"
                .into(),
        ));
    }
    let guard = super::RetainedCopyTransaction::from_opened(&parent, &stage, &transaction, true)
        .map_err(journal_error)?;
    match record.phase {
        CopyPhase::Published => record.validate_published_data(&stage)?,
        CopyPhase::UndoIntent => {
            record.undo_topology(&stage)?;
        }
        CopyPhase::PublicationIntent => {
            return Err(PlatformError::NotAvailable(
                "Copy publication needs reconciliation before Undo",
            ));
        }
        CopyPhase::Undone => {
            return Err(PlatformError::NotAvailable("Copy Undo is already complete"));
        }
    }
    record.validate_namespace()?;
    record.validate_opened_directory(&parent, true)?;
    record.validate_opened_directory(&stage, false)?;
    guard.validate_path(&transaction)?;
    guard.validate_publication_parent(&publication)?;
    let identity = record
        .publication_content
        .root_path_identity()
        .ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt("Copy snapshot has no root identity".into())
        })?;
    let displaced_identity = record
        .displaced_content
        .as_ref()
        .map(|content| {
            content.root_path_identity().ok_or_else(|| {
                PlatformError::InvalidPathMoveUndoReceipt(
                    "Copy backup snapshot has no root identity".into(),
                )
            })
        })
        .transpose()?;
    Ok(super::PathMoveUndoReceipt {
        before_path: transaction.join(super::copy_transaction::CANDIDATE),
        after_path: publication,
        identity,
        displaced_identity,
        retained_displaced_path: None,
        requirement: super::PathMoveUndoRequirement::None,
        published_content: Some(record.publication_content),
        retained_displaced_content: record.displaced_content,
        cross_filesystem_state: Default::default(),
        retained_copy_directory: Some(guard),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum UndoTopology {
    Published,
    Withdrawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UndoPoint {
    IntentDurable,
    WithdrawalDone,
    DirectoriesSynced,
    CompletedDurable,
}

pub(super) fn undo_with_journals(
    parent: &std::fs::File,
    stage: &std::fs::File,
    receipt: &super::PathMoveUndoReceipt,
) -> Result<(), PlatformError> {
    undo_with_journals_with_hook(parent, stage, receipt, &mut |_| Ok(()))
}

fn undo_with_journals_with_hook(
    parent: &std::fs::File,
    stage: &std::fs::File,
    receipt: &super::PathMoveUndoReceipt,
    hook: &mut impl FnMut(UndoPoint) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    use super::move_recovery::LinuxJournalDirectory;
    use std::os::unix::fs::MetadataExt;
    let stage_metadata = stage.metadata().map_err(journal_error)?;
    if !stage_metadata.is_dir()
        || stage_metadata.uid() != rustix::process::geteuid().as_raw()
        || stage_metadata.mode() & 0o777 != 0o700
    {
        return Err(PlatformError::StalePathMoveUndo(
            receipt.before_path.display().to_string(),
        ));
    }
    let local = LinuxJournalDirectory::for_copy_stage(stage).map_err(journal_error)?;
    let mut lock = local.try_lock_recovery().map_err(journal_error)?;
    let mut record = local.read_copy_record("copy-journal")?;
    let central = LinuxJournalDirectory::open_copy_registry(&configured_root()?, false)
        .map_err(journal_error)?;
    let id = record.transaction_id()?;
    if central.read_copy_record(&format!("{id}.journal"))? != record {
        return Err(PlatformError::Io(
            "central and destination-local Copy journals differ".into(),
        ));
    }
    let transaction = record.transaction.decode().map_err(journal_error)?;
    let publication = record.publication.decode().map_err(journal_error)?;
    let receipt_parent = receipt.after_path.parent().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt("publication has no parent".into())
    })?;
    let receipt_name = receipt.after_path.file_name().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt("publication has no name".into())
    })?;
    let normalized_publication = std::fs::canonicalize(receipt_parent)
        .map_err(journal_error)?
        .join(receipt_name);
    if receipt.before_path != transaction.join(super::copy_transaction::CANDIDATE)
        || normalized_publication != publication
        || receipt.requirement != super::PathMoveUndoRequirement::None
        || receipt.retained_displaced_path.is_some()
        || receipt.published_content.as_ref() != Some(&record.publication_content)
        || receipt.retained_displaced_content != record.displaced_content
        || !record
            .publication_content
            .root_matches_path_identity(&receipt.identity)
        || receipt.displaced_identity.is_some() != record.displaced_content.is_some()
        || receipt
            .displaced_identity
            .as_ref()
            .zip(record.displaced_content.as_ref())
            .is_some_and(|(identity, content)| !content.root_matches_path_identity(identity))
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Copy receipt does not match its durable record".into(),
        ));
    }
    record.validate_namespace()?;
    record.validate_opened_directory(parent, true)?;
    record.validate_opened_directory(stage, false)?;
    let topology = match record.phase {
        CopyPhase::PublicationIntent => {
            return Err(PlatformError::InvalidPathMoveUndoReceipt(
                "Copy publication has not been durably completed".into(),
            ));
        }
        CopyPhase::Published => {
            if super::rename_identity(&publication).ok().as_ref() != Some(&receipt.identity) {
                return Err(PlatformError::StalePathMoveUndo(
                    publication.display().to_string(),
                ));
            }
            if let Some(identity) = &receipt.displaced_identity
                && super::rename_identity(&receipt.before_path).ok().as_ref() != Some(identity)
            {
                return Err(PlatformError::StalePathMoveUndo(
                    receipt.before_path.display().to_string(),
                ));
            }
            record.validate_published_data(stage)?;
            UndoTopology::Published
        }
        CopyPhase::UndoIntent => record.undo_topology(stage)?,
        CopyPhase::Undone => {
            record.validate_completed_data()?;
            return Ok(());
        }
    };
    record.validate_namespace()?;
    if topology == UndoTopology::Published {
        record.phase = CopyPhase::UndoIntent;
        lock.persist_copy_in_registry(&central, &record)
            .map_err(journal_error)?;
        hook(UndoPoint::IntentDurable)?;
        super::copy_transaction::withdraw_opened(
            parent,
            stage,
            &publication,
            &receipt.before_path,
            record.displaced_content.is_some(),
        )?;
    }
    let finish = (|| {
        hook(UndoPoint::WithdrawalDone)?;
        rustix::fs::fsync(parent).map_err(journal_error)?;
        rustix::fs::fsync(stage).map_err(journal_error)?;
        hook(UndoPoint::DirectoriesSynced)?;
        record.validate_namespace()?;
        record.validate_withdrawn_data()?;
        record.phase = CopyPhase::Undone;
        lock.persist_copy_in_registry(&central, &record)
            .map_err(journal_error)?;
        hook(UndoPoint::CompletedDurable)?;
        Ok(())
    })();
    finish.map_err(
        |error: PlatformError| PlatformError::CommittedCleanupPending {
            transaction: transaction.display().to_string(),
            message: format!(
                "Copy Undo withdrawal completed but durable completion needs recovery: {error}"
            ),
        },
    )
}

pub(super) fn configured_root() -> Result<PathBuf, PlatformError> {
    Ok(super::move_transaction::default_recovery_root()?.with_file_name("copies"))
}

pub(super) fn open_registry() -> Result<super::move_recovery::LinuxJournalDirectory, PlatformError>
{
    use std::os::unix::fs::DirBuilderExt;
    let root = configured_root()?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)
        .map_err(journal_error)?;
    super::move_recovery::LinuxJournalDirectory::open_copy_registry(&root, true)
        .map_err(journal_error)
}

#[derive(Debug, PartialEq, Eq)]
pub enum CopyRecoveryEvidence {
    MatchingJournals,
    /// Point-in-time read-only validation; no Undo or deletion authority.
    PublishedObjectsAndContentsMatch,
    /// The exact publication is retained by a newer validated overwrite.
    /// No receipt is available until that overwrite is undone and revalidated.
    PublicationRetainedByCopy {
        transaction_id: String,
    },
    UndoReadyToWithdraw,
    UndoWithdrawalMatches,
    UndoneRetainedObjectsAndContentsMatch,
    NeedsReview {
        reason: String,
    },
}

/// Read-only retained Copy status. Matching journals do not grant Undo authority;
/// object/content matches do not grant Undo or deletion authority either.
#[derive(Debug)]
pub struct RetainedCopyStatus {
    pub transaction_id: String,
    pub phase: Option<CopyPhase>,
    pub operation: Option<CopyOperation>,
    pub source: Option<PathBuf>,
    pub publication: Option<PathBuf>,
    pub transaction: Option<PathBuf>,
    pub evidence: CopyRecoveryEvidence,
}

pub(super) fn discover_at(root: &Path) -> Result<Vec<RetainedCopyStatus>, PlatformError> {
    match std::fs::symlink_metadata(root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(journal_error(error)),
    }
    let central = super::move_recovery::LinuxJournalDirectory::open_copy_registry(root, false)
        .map_err(journal_error)?;
    let entries = central.discover_copy_records().map_err(journal_error)?;
    let mut records = std::collections::BTreeMap::new();
    let mut statuses: Vec<_> = entries
        .into_iter()
        .map(|(id, record)| {
            let mut status = RetainedCopyStatus {
                transaction_id: id.clone(),
                phase: None,
                operation: None,
                source: None,
                publication: None,
                transaction: None,
                evidence: CopyRecoveryEvidence::NeedsReview {
                    reason: String::new(),
                },
            };
            let result = (|| {
                let record = record?;
                if record.transaction_id()? != id {
                    return Err(PlatformError::Io(
                        "Copy registry filename does not match transaction".into(),
                    ));
                }
                status.phase = Some(record.phase);
                status.operation = Some(record.operation.clone());
                status.source = Some(record.source.decode().map_err(journal_error)?);
                status.publication = Some(record.publication.decode().map_err(journal_error)?);
                let transaction = record.transaction.decode().map_err(journal_error)?;
                status.transaction = Some(transaction.clone());
                record.validate_namespace()?;
                let fd = rustix::fs::open(
                    &transaction,
                    rustix::fs::OFlags::RDONLY
                        | rustix::fs::OFlags::DIRECTORY
                        | rustix::fs::OFlags::NOFOLLOW
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )
                .map_err(journal_error)?;
                record.validate_opened_directory(&fd, false)?;
                let local = super::move_recovery::LinuxJournalDirectory::for_copy_stage(&fd)
                    .map_err(journal_error)?;
                let local = local.read_copy_record("copy-journal")?;
                if local != record {
                    return Err(PlatformError::Io(
                        "central and destination-local Copy journals differ".into(),
                    ));
                }
                record.validate_namespace()?;
                let evidence = (|| {
                    Ok(match record.phase {
                        CopyPhase::Published => {
                            record.validate_published_data(&fd)?;
                            CopyRecoveryEvidence::PublishedObjectsAndContentsMatch
                        }
                        CopyPhase::PublicationIntent => CopyRecoveryEvidence::MatchingJournals,
                        CopyPhase::UndoIntent => match record.undo_topology(&fd)? {
                            UndoTopology::Published => CopyRecoveryEvidence::UndoReadyToWithdraw,
                            UndoTopology::Withdrawn => CopyRecoveryEvidence::UndoWithdrawalMatches,
                        },
                        CopyPhase::Undone => {
                            record.validate_completed_data()?;
                            CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
                        }
                    })
                })();
                record.validate_namespace()?;
                records.insert(id.clone(), record);
                evidence
            })();
            status.evidence = match result {
                Ok(evidence) => evidence,
                Err(error) => CopyRecoveryEvidence::NeedsReview {
                    reason: error.to_string(),
                },
            };
            Ok(status)
        })
        .collect::<Result<_, PlatformError>>()?;
    // Establish ordering from exact retained object identity, never timestamps
    // or random transaction IDs. Only chains ending in a validated publication
    // can become dependencies; cycles and ambiguous owners remain NeedsReview.
    let mut by_publication = std::collections::BTreeMap::<PathBuf, Vec<usize>>::new();
    for (index, status) in statuses.iter().enumerate() {
        if records.contains_key(&status.transaction_id)
            && status.phase == Some(CopyPhase::Published)
        {
            by_publication
                .entry(status.publication.clone().expect("validated publication"))
                .or_default()
                .push(index);
        }
    }
    let mut dependents = vec![Vec::new(); statuses.len()];
    for (index, status) in statuses.iter().enumerate() {
        if !matches!(status.evidence, CopyRecoveryEvidence::NeedsReview { .. }) {
            continue;
        }
        let Some(record) = records.get(&status.transaction_id) else {
            continue;
        };
        let Some(candidates) = status
            .publication
            .as_ref()
            .and_then(|path| by_publication.get(path))
        else {
            continue;
        };
        let owners: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|&owner| {
                let newer = &records[&statuses[owner].transaction_id];
                owner != index
                    && newer.displaced_content.as_ref() == Some(&record.publication_content)
                    && newer.displaced_objects.as_ref() == Some(&record.publication_objects)
                    && (newer.operation.id != record.operation.id
                        || newer.operation.item_index > record.operation.item_index)
            })
            .collect();
        if owners.len() == 1 {
            dependents[owners[0]].push(index);
        }
    }
    let mut ready: std::collections::VecDeque<_> = statuses
        .iter()
        .enumerate()
        .filter_map(|(index, status)| {
            matches!(
                status.evidence,
                CopyRecoveryEvidence::PublishedObjectsAndContentsMatch
            )
            .then_some(index)
        })
        .collect();
    while let Some(owner_index) = ready.pop_front() {
        for &index in &dependents[owner_index] {
            let status = &statuses[index];
            if status.phase != Some(CopyPhase::Published)
                || !matches!(status.evidence, CopyRecoveryEvidence::NeedsReview { .. })
            {
                continue;
            }
            let record = &records[&status.transaction_id];
            let owner = &statuses[owner_index];
            let validation = (|| {
                record.require_object_evidence()?;
                record.validate_namespace()?;
                let transaction = record.transaction.decode().map_err(journal_error)?;
                let stage = rustix::fs::open(
                    &transaction,
                    rustix::fs::OFlags::RDONLY
                        | rustix::fs::OFlags::DIRECTORY
                        | rustix::fs::OFlags::NOFOLLOW
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )
                .map_err(journal_error)?;
                record.validate_opened_directory(&stage, false)?;
                record.validate_retained_backup(&stage)?;
                let newer = &records[&owner.transaction_id];
                newer.validate_namespace()?;
                let backup = newer
                    .transaction
                    .decode()
                    .map_err(journal_error)?
                    .join(super::copy_transaction::CANDIDATE);
                record.validate_tree(
                    &backup,
                    &record.publication_content,
                    &record.publication_objects,
                )?;
                newer.validate_namespace()?;
                record.validate_namespace()
            })();
            if validation.is_ok() {
                let transaction_id = owner.transaction_id.clone();
                statuses[index].evidence =
                    CopyRecoveryEvidence::PublicationRetainedByCopy { transaction_id };
                ready.push_back(index);
            }
        }
    }
    Ok(statuses)
}

impl CopyRecord {
    fn require_object_evidence(&self) -> Result<(), PlatformError> {
        if self.parent_handle.is_none()
            || self.transaction_handle.is_none()
            || self
                .publication_objects
                .iter()
                .chain(self.displaced_objects.iter().flatten())
                .any(|object| object.handle.is_none())
        {
            return Err(PlatformError::Io(
                "Copy filesystem object evidence is unavailable; review is required".into(),
            ));
        }
        Ok(())
    }

    fn validate_published_data(&self, stage: &impl rustix::fd::AsFd) -> Result<(), PlatformError> {
        self.require_object_evidence()?;
        let publication = self.publication.decode().map_err(journal_error)?;
        self.validate_tree(
            &publication,
            &self.publication_content,
            &self.publication_objects,
        )?;
        self.validate_retained_backup(stage)
    }

    fn validate_retained_backup(&self, stage: &impl rustix::fd::AsFd) -> Result<(), PlatformError> {
        let transaction = self.transaction.decode().map_err(journal_error)?;
        if let Some((content, objects)) = self
            .displaced_content
            .as_ref()
            .zip(self.displaced_objects.as_ref())
        {
            self.validate_tree(
                &transaction.join(super::copy_transaction::CANDIDATE),
                content,
                objects,
            )?;
        } else {
            match rustix::fs::statat(
                stage,
                super::copy_transaction::CANDIDATE,
                rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
            ) {
                Err(rustix::io::Errno::NOENT) => {}
                Ok(_) => {
                    return Err(PlatformError::Io(
                        "unexpected entry in published Copy staging directory".into(),
                    ));
                }
                Err(error) => return Err(journal_error(error)),
            }
        }
        Ok(())
    }

    // After durable completion, publication names belong to subsequent user
    // work. Only the private copied candidate remains owned by this transaction.
    fn validate_completed_data(&self) -> Result<(), PlatformError> {
        self.require_object_evidence()?;
        let transaction = self.transaction.decode().map_err(journal_error)?;
        self.validate_tree(
            &transaction.join(super::copy_transaction::CANDIDATE),
            &self.publication_content,
            &self.publication_objects,
        )
    }

    fn validate_withdrawn_data(&self) -> Result<(), PlatformError> {
        self.validate_completed_data()?;
        let publication = self.publication.decode().map_err(journal_error)?;
        if let Some((content, objects)) = self
            .displaced_content
            .as_ref()
            .zip(self.displaced_objects.as_ref())
        {
            self.validate_tree(&publication, content, objects)?;
        } else {
            match std::fs::symlink_metadata(&publication) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(PlatformError::StalePathMoveUndo(
                        publication.display().to_string(),
                    ));
                }
                Err(error) => return Err(journal_error(error)),
            }
        }
        Ok(())
    }

    fn undo_topology(&self, stage: &impl rustix::fd::AsFd) -> Result<UndoTopology, PlatformError> {
        let published = self.validate_published_data(stage);
        let withdrawn = self.validate_withdrawn_data();
        match (published, withdrawn) {
            (Ok(()), Err(_)) => Ok(UndoTopology::Published),
            (Err(_), Ok(())) => Ok(UndoTopology::Withdrawn),
            (Err(before), Err(after)) => Err(PlatformError::Io(format!(
                "Copy Undo evidence does not match either topology: {before}; {after}"
            ))),
            (Ok(()), Ok(())) => Err(PlatformError::Io("Copy Undo topology is ambiguous".into())),
        }
    }

    fn validate_tree(
        &self,
        path: &Path,
        content: &SourceTreeSnapshot,
        objects: &[TreeObject],
    ) -> Result<(), PlatformError> {
        let (observed_content, observed_objects) = sample_tree(path, Some(objects))?;
        if observed_objects != objects {
            return Err(PlatformError::Io(
                "Copy tree objects or mount bindings changed".into(),
            ));
        }
        if observed_content != *content {
            return Err(PlatformError::UndoContentChanged(
                path.display().to_string(),
            ));
        }
        Ok(())
    }

    fn validate_namespace(&self) -> Result<(), PlatformError> {
        let transaction = self.transaction.decode().map_err(journal_error)?;
        let parent = transaction.parent().expect("validated transaction parent");
        if std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map_err(journal_error)?
            .trim()
            != self.boot_id
        {
            return Err(PlatformError::Io(
                "Copy directory evidence belongs to another boot; review is required".into(),
            ));
        }
        for (path, identity, mount, handle) in [
            (
                parent,
                &self.parent_identity,
                self.parent_mount,
                &self.parent_handle,
            ),
            (
                transaction.as_path(),
                &self.transaction_identity,
                self.transaction_mount,
                &self.transaction_handle,
            ),
        ] {
            let metadata = std::fs::symlink_metadata(path).map_err(journal_error)?;
            if !metadata.is_dir()
                || super::rename_identity(path).map_err(journal_error)? != *identity
                || super::local_storage::local_path_mount_identity(path, false)
                    .map_err(journal_error)?
                    != mount
            {
                return Err(PlatformError::Io(
                    "Copy directory or mount binding changed".into(),
                ));
            }
            if let Some(expected) = handle {
                let fd = rustix::fs::open(
                    path,
                    rustix::fs::OFlags::PATH
                        | rustix::fs::OFlags::NOFOLLOW
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )
                .map_err(journal_error)?;
                let opened = rustix::fs::fstat(&fd).map_err(journal_error)?;
                use rustix::fd::AsRawFd;
                let opened_mount = super::local_storage::local_path_mount_identity(
                    &PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd())),
                    true,
                )
                .map_err(journal_error)?;
                if (opened.st_dev, opened.st_ino) != (identity.device, identity.inode)
                    || opened_mount != mount
                    || ObjectHandle::capture(&fd).map_err(journal_error)?.as_ref() != Some(expected)
                {
                    return Err(PlatformError::Io(
                        "Copy directory object generation changed".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DefaultPlatform, PlatformOps};

    struct PrivateRoot(Option<PathBuf>);
    impl PrivateRoot {
        fn new(path: &Path) -> Self {
            Self(super::super::move_transaction::replace_test_recovery_root(
                Some(path.join("moves")),
            ))
        }
    }
    impl Drop for PrivateRoot {
        fn drop(&mut self) {
            super::super::move_transaction::replace_test_recovery_root(self.0.take());
        }
    }

    #[test]
    fn overlapping_copy_chain_is_read_only_and_unblocks_after_reverse_undo() {
        for kind in ["file", "directory", "symlink"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = PrivateRoot::new(root.path());
            let destination = root.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let mut sources = Vec::new();
            for version in 0..3 {
                let directory = root.path().join(format!("source-{version}"));
                std::fs::create_dir(&directory).unwrap();
                let source = directory.join("item");
                match kind {
                    "directory" => {
                        std::fs::create_dir(&source).unwrap();
                        std::fs::write(source.join("leaf"), format!("version-{version}")).unwrap();
                    }
                    "symlink" => {
                        std::os::unix::fs::symlink(format!("target-{version}"), &source).unwrap();
                    }
                    _ => std::fs::write(&source, format!("version-{version}")).unwrap(),
                }
                DefaultPlatform
                    .execute_copy_with_undo_receipt(
                        &source,
                        &destination,
                        crate::ConflictAction::Overwrite,
                    )
                    .unwrap()
                    .unwrap();
                sources.push(source);
            }
            let registry = root.path().join("copies");
            let before: Vec<_> = std::fs::read_dir(&registry)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    let bytes = std::fs::read(&path).unwrap();
                    (path, bytes)
                })
                .collect();
            let statuses = discover_at(&registry).unwrap();
            let ids: Vec<_> = sources
                .iter()
                .map(|source| {
                    statuses
                        .iter()
                        .find(|status| status.source.as_ref() == Some(source))
                        .unwrap()
                        .transaction_id
                        .clone()
                })
                .collect();
            for index in 0..2 {
                let status = statuses
                    .iter()
                    .find(|status| status.transaction_id == ids[index])
                    .unwrap();
                assert_eq!(
                    status.evidence,
                    CopyRecoveryEvidence::PublicationRetainedByCopy {
                        transaction_id: ids[index + 1].clone(),
                    }
                );
                assert!(crate::restore_retained_copy_undo_receipt(&ids[index]).is_err());
            }
            for (path, bytes) in before {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            }
            for index in (0..3).rev() {
                let status = discover_at(&registry)
                    .unwrap()
                    .into_iter()
                    .find(|status| status.transaction_id == ids[index])
                    .unwrap();
                assert_eq!(
                    status.evidence,
                    CopyRecoveryEvidence::PublishedObjectsAndContentsMatch
                );
                let receipt = crate::restore_retained_copy_undo_receipt(&ids[index]).unwrap();
                DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
            }
            assert!(!destination.join("item").exists());
            for (version, source) in sources.iter().enumerate() {
                match kind {
                    "directory" => assert_eq!(
                        std::fs::read_to_string(source.join("leaf")).unwrap(),
                        format!("version-{version}")
                    ),
                    "symlink" => assert_eq!(
                        std::fs::read_link(source).unwrap(),
                        PathBuf::from(format!("target-{version}"))
                    ),
                    _ => assert_eq!(
                        std::fs::read_to_string(source).unwrap(),
                        format!("version-{version}")
                    ),
                }
            }
        }
    }

    #[test]
    fn overlapping_copy_dependencies_refuse_changed_or_missing_evidence() {
        for tamper in ["publication", "backup", "journal", "missing-owner"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = PrivateRoot::new(root.path());
            let destination = root.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let mut sources = Vec::new();
            for version in 0..2 {
                let directory = root.path().join(format!("source-{version}"));
                std::fs::create_dir(&directory).unwrap();
                let source = directory.join("item");
                std::fs::write(&source, format!("version-{version}")).unwrap();
                DefaultPlatform
                    .execute_copy_with_undo_receipt(
                        &source,
                        &destination,
                        crate::ConflictAction::Overwrite,
                    )
                    .unwrap()
                    .unwrap();
                sources.push(source);
            }
            let registry = root.path().join("copies");
            let statuses = discover_at(&registry).unwrap();
            let newest = statuses
                .iter()
                .find(|status| status.source.as_ref() == Some(&sources[1]))
                .unwrap();
            let oldest = statuses
                .iter()
                .find(|status| status.source.as_ref() == Some(&sources[0]))
                .unwrap();
            let backup = newest
                .transaction
                .as_ref()
                .unwrap()
                .join(super::super::copy_transaction::CANDIDATE);
            let local = newest.transaction.as_ref().unwrap().join("copy-journal");
            let central = registry.join(format!("{}.journal", newest.transaction_id));
            match tamper {
                "publication" => std::fs::write(destination.join("item"), "external edit").unwrap(),
                "backup" => std::fs::write(&backup, "external backup edit").unwrap(),
                "journal" => std::fs::write(&local, "invalid journal").unwrap(),
                _ => std::fs::remove_file(&central).unwrap(),
            }
            let paths = [destination.join("item"), backup, local];
            let before: Vec<_> = paths
                .iter()
                .map(|path| std::fs::read(path).unwrap())
                .collect();
            assert!(
                discover_at(&registry)
                    .unwrap()
                    .iter()
                    .all(|status| matches!(
                        status.evidence,
                        CopyRecoveryEvidence::NeedsReview { .. }
                    )),
                "{tamper}"
            );
            assert!(crate::restore_retained_copy_undo_receipt(&oldest.transaction_id).is_err());
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            }
            for (version, source) in sources.iter().enumerate() {
                assert_eq!(
                    std::fs::read_to_string(source).unwrap(),
                    format!("version-{version}")
                );
            }
        }
    }

    #[test]
    fn copy_batch_order_and_skipped_slots_survive_a_fresh_process_and_undo() {
        const ROOT_ENV: &str = "OTTRIN_COPY_OPERATION_TEST_ROOT";
        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let root = PathBuf::from(root);
            let _registry = PrivateRoot::new(&root);
            let destination = root.join("destination");
            std::fs::create_dir(&destination).unwrap();
            let sources: Vec<_> = ["first", "skip", "last"]
                .into_iter()
                .map(|name| {
                    let source = root.join(name);
                    std::fs::write(&source, name).unwrap();
                    source
                })
                .collect();
            std::fs::write(destination.join("skip"), "keep-original").unwrap();
            let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
                &sources,
                &destination,
                super::super::ConflictAction::Skip,
            );
            assert!(batch.error.is_none(), "{:?}", batch.error);
            assert_eq!(batch.completed_sources, 3);
            assert_eq!(batch.undo_receipts.len(), 2);
            let single = root.join("single");
            std::fs::write(&single, "single").unwrap();
            DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &single,
                    &destination,
                    super::super::ConflictAction::Skip,
                )
                .unwrap()
                .unwrap();
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let _registry = PrivateRoot::new(root.path());
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "copy_recovery::tests::copy_batch_order_and_skipped_slots_survive_a_fresh_process_and_undo", "--nocapture"])
            .env(ROOT_ENV, root.path()).spawn().unwrap();
        assert!(wait_owned_child(child).success());
        let mut entries = discover_at(&root.path().join("copies")).unwrap();
        assert_eq!(entries.len(), 3);
        let single = entries
            .iter()
            .find(|entry| entry.source.as_deref() == Some(root.path().join("single").as_path()))
            .unwrap();
        let single_operation = single.operation.clone().unwrap();
        assert_eq!(single_operation.item_count, 1);
        assert_eq!(single_operation.item_index, 0);
        assert_eq!(single_operation.id, single.transaction_id);
        entries.retain(|entry| entry.operation.as_ref().unwrap().id != single_operation.id);
        entries.sort_by_key(|entry| entry.operation.as_ref().unwrap().item_index);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].operation.as_ref().unwrap().item_index, 0);
        assert_eq!(entries[1].operation.as_ref().unwrap().item_index, 2);
        let operation_id = &entries[0].operation.as_ref().unwrap().id;
        assert_ne!(operation_id, &single_operation.id);
        assert_eq!(operation_id, &entries[1].operation.as_ref().unwrap().id);
        let mut journal_bytes = Vec::new();
        for entry in entries.iter().rev() {
            let operation = entry.operation.as_ref().unwrap();
            assert_eq!(operation.item_count, 3);
            let central = root
                .path()
                .join("copies")
                .join(format!("{}.journal", entry.transaction_id));
            let local = entry.transaction.as_ref().unwrap().join("copy-journal");
            let before = std::fs::read(&central).unwrap();
            assert_eq!(before, std::fs::read(&local).unwrap());
            let receipt =
                super::super::restore_retained_copy_undo_receipt(&entry.transaction_id).unwrap();
            assert_eq!(before, std::fs::read(&central).unwrap());
            DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
            journal_bytes.push((central, local, operation.clone()));
        }
        for (central, local, expected) in journal_bytes {
            let record = CopyRecord::parse(&std::fs::read(&central).unwrap()).unwrap();
            assert_eq!(record.operation, expected);
            assert_eq!(record.phase, CopyPhase::Undone);
            assert_eq!(
                std::fs::read(central).unwrap(),
                std::fs::read(local).unwrap()
            );
        }
        let destination = root.path().join("destination");
        for name in ["first", "last"] {
            assert!(!destination.join(name).exists());
            assert_eq!(
                std::fs::read_to_string(root.path().join(name)).unwrap(),
                name
            );
        }
        assert_eq!(
            std::fs::read_to_string(destination.join("skip")).unwrap(),
            "keep-original"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("single")).unwrap(),
            "single"
        );
    }

    #[test]
    fn partial_copy_batch_retains_original_count_without_claiming_missing_items_completed() {
        let root = tempfile::tempdir().unwrap();
        let _registry = PrivateRoot::new(root.path());
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let first = root.path().join("first");
        let second = destination.join("second");
        std::fs::write(&first, "first-data").unwrap();
        std::fs::write(&second, "second-data").unwrap();
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &[first, second.clone()],
            &destination,
            super::super::ConflictAction::Overwrite,
        );
        assert!(batch.error.is_some());
        assert_eq!(batch.completed_sources, 1);
        let entries = discover_at(&root.path().join("copies")).unwrap();
        assert_eq!(entries.len(), 1);
        let operation = entries[0].operation.as_ref().unwrap();
        assert_eq!(operation.item_count, 2);
        assert_eq!(operation.item_index, 0);
        assert_eq!(std::fs::read_to_string(second).unwrap(), "second-data");
    }

    #[test]
    fn invalid_or_missing_copy_operation_metadata_is_retained_for_review_without_mutation() {
        for change in ["id", "zero-count", "index", "missing", "old-schema"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = PrivateRoot::new(root.path());
            let receipt = copy_fixture(root.path(), false, false);
            let entry = discover_at(&root.path().join("copies")).unwrap().remove(0);
            let central = root
                .path()
                .join("copies")
                .join(format!("{}.journal", entry.transaction_id));
            let local = entry.transaction.as_ref().unwrap().join("copy-journal");
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&central).unwrap()).unwrap();
            match change {
                "id" => value["operation"]["id"] = "../bad".into(),
                "zero-count" => value["operation"]["item_count"] = 0.into(),
                "index" => value["operation"]["item_index"] = 1.into(),
                "missing" => {
                    value.as_object_mut().unwrap().remove("operation");
                }
                "old-schema" => value["schema"] = 3.into(),
                _ => unreachable!(),
            }
            let encoded = serde_json::to_vec(&value).unwrap();
            std::fs::write(&central, &encoded).unwrap();
            std::fs::write(&local, &encoded).unwrap();
            let status = discover_at(&root.path().join("copies")).unwrap().remove(0);
            assert!(
                matches!(status.evidence, CopyRecoveryEvidence::NeedsReview { .. }),
                "{change}"
            );
            assert!(status.operation.is_none());
            assert!(
                super::super::restore_retained_copy_undo_receipt(&entry.transaction_id).is_err()
            );
            assert_eq!(std::fs::read(&central).unwrap(), encoded);
            assert_eq!(std::fs::read(&local).unwrap(), encoded);
            assert_eq!(std::fs::read(receipt.after_path()).unwrap(), b"new-data");
            assert_eq!(
                std::fs::read(root.path().join("item")).unwrap(),
                b"new-data"
            );
        }
    }

    #[test]
    fn completed_copy_inverse_releases_publication_names_but_preserves_retained_validation() {
        for overwrite in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = PrivateRoot::new(root.path());
            let receipt = copy_fixture(root.path(), overwrite, false);
            DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
            let entry = discover_at(&root.path().join("copies")).unwrap().remove(0);
            let central = root
                .path()
                .join("copies")
                .join(format!("{}.journal", entry.transaction_id));
            let local = receipt.before_path().parent().unwrap().join("copy-journal");
            let before = std::fs::read(&central).unwrap();
            std::fs::write(receipt.after_path(), "later-user-work").unwrap();
            let completed = discover_at(&root.path().join("copies")).unwrap().remove(0);
            assert_eq!(
                completed.evidence,
                CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
            );
            assert!(
                super::super::restore_retained_copy_undo_receipt(&entry.transaction_id).is_err()
            );
            // An already completed active receipt is idempotent and cannot
            // remove or replace unrelated work now using the released name.
            DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"later-user-work"
            );
            assert_eq!(std::fs::read(&central).unwrap(), before);
            assert_eq!(std::fs::read(&local).unwrap(), before);
            std::fs::write(receipt.before_path(), "changed-retained-copy").unwrap();
            assert!(matches!(
                discover_at(&root.path().join("copies")).unwrap()[0].evidence,
                CopyRecoveryEvidence::NeedsReview { .. }
            ));
            assert!(DefaultPlatform.undo_copy_with_receipt(&receipt).is_err());
            assert_eq!(
                std::fs::read(receipt.after_path()).unwrap(),
                b"later-user-work"
            );
            assert_eq!(std::fs::read(&central).unwrap(), before);
            assert_eq!(std::fs::read(&local).unwrap(), before);
        }
    }

    #[test]
    fn interrupted_copy_inverse_still_requires_the_full_withdrawn_namespace() {
        let root = tempfile::tempdir().unwrap();
        let _registry = PrivateRoot::new(root.path());
        let receipt = copy_fixture(root.path(), false, false);
        let parent = std::fs::File::open(root.path().join("destination")).unwrap();
        let stage = std::fs::File::open(receipt.before_path().parent().unwrap()).unwrap();
        assert!(
            undo_with_journals_with_hook(&parent, &stage, &receipt, &mut |point| {
                if point == UndoPoint::WithdrawalDone {
                    Err(PlatformError::Io("test interruption".into()))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        let entry = discover_at(&root.path().join("copies")).unwrap().remove(0);
        assert_eq!(entry.phase, Some(CopyPhase::UndoIntent));
        let central = root
            .path()
            .join("copies")
            .join(format!("{}.journal", entry.transaction_id));
        let before = std::fs::read(&central).unwrap();
        std::fs::write(receipt.after_path(), "new-user-work").unwrap();
        assert!(matches!(
            discover_at(&root.path().join("copies")).unwrap()[0].evidence,
            CopyRecoveryEvidence::NeedsReview { .. }
        ));
        assert!(super::super::restore_retained_copy_undo_receipt(&entry.transaction_id).is_err());
        assert!(DefaultPlatform.undo_copy_with_receipt(&receipt).is_err());
        assert_eq!(
            std::fs::read(receipt.after_path()).unwrap(),
            b"new-user-work"
        );
        assert_eq!(std::fs::read(receipt.before_path()).unwrap(), b"new-data");
        assert_eq!(std::fs::read(&central).unwrap(), before);
    }

    fn copy_fixture(
        root: &Path,
        overwrite: bool,
        directory: bool,
    ) -> super::super::PathMoveUndoReceipt {
        let source = root.join("item");
        let destination = root.join("destination");
        std::fs::create_dir(&destination).unwrap();
        if directory {
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("leaf"), "new-data").unwrap();
            if overwrite {
                std::fs::create_dir(destination.join("item")).unwrap();
                std::fs::write(destination.join("item/leaf"), "old-data").unwrap();
            }
        } else {
            std::fs::write(&source, "new-data").unwrap();
            if overwrite {
                std::fs::write(destination.join("item"), "old-data").unwrap();
            }
        }
        DefaultPlatform
            .execute_copy_with_undo_receipt(
                &source,
                &destination,
                super::super::ConflictAction::Overwrite,
            )
            .unwrap()
            .unwrap()
    }

    fn wait_owned_child(child: std::process::Child) -> std::process::ExitStatus {
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let mut child = OwnedChild(child);
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "owned recovery fixture timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn published_copy_receipts_restore_after_process_exit_for_files_directories_and_symlinks() {
        const ROOT: &str = "OTTRIN_COPY_RECEIPT_RESTART_ROOT";
        if let Some(root) = std::env::var_os(ROOT) {
            let root = PathBuf::from(root);
            let _private = PrivateRoot::new(&root);
            let kind = std::env::var("OTTRIN_COPY_RECEIPT_RESTART_KIND").unwrap();
            let overwrite =
                std::env::var("OTTRIN_COPY_RECEIPT_RESTART_OVERWRITE").unwrap() == "true";
            if kind == "symlink" {
                let destination = root.join("destination");
                std::fs::create_dir(&destination).unwrap();
                std::fs::write(root.join("new-target"), "new-target-data").unwrap();
                std::fs::write(root.join("old-target"), "old-target-data").unwrap();
                std::os::unix::fs::symlink(root.join("new-target"), root.join("item")).unwrap();
                if overwrite {
                    std::os::unix::fs::symlink(root.join("old-target"), destination.join("item"))
                        .unwrap();
                }
                DefaultPlatform
                    .execute_copy_with_undo_receipt(
                        &root.join("item"),
                        &destination,
                        super::super::ConflictAction::Overwrite,
                    )
                    .unwrap();
            } else {
                copy_fixture(&root, overwrite, kind == "directory");
            }
            return;
        }
        for kind in ["file", "directory", "symlink"] {
            for overwrite in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let _private = PrivateRoot::new(root.path());
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "copy_recovery::tests::published_copy_receipts_restore_after_process_exit_for_files_directories_and_symlinks", "--nocapture"])
                    .env(ROOT, root.path()).env("OTTRIN_COPY_RECEIPT_RESTART_KIND", kind)
                    .env("OTTRIN_COPY_RECEIPT_RESTART_OVERWRITE", overwrite.to_string()).spawn().unwrap();
                assert!(wait_owned_child(child).success());
                let status = discover_at(&root.path().join("copies")).unwrap().remove(0);
                let transaction = status.transaction.unwrap();
                let central = root
                    .path()
                    .join("copies")
                    .join(format!("{}.journal", status.transaction_id));
                let local = transaction.join("copy-journal");
                let journals = (
                    std::fs::read(&central).unwrap(),
                    std::fs::read(&local).unwrap(),
                );
                let receipt =
                    crate::restore_retained_copy_undo_receipt(&status.transaction_id).unwrap();
                assert_eq!(std::fs::read(&central).unwrap(), journals.0);
                assert_eq!(std::fs::read(&local).unwrap(), journals.1);
                let parent = root.path().join("destination");
                for entry in std::fs::read_dir("/proc/self/fd").unwrap() {
                    if let Ok(path) = std::fs::read_link(entry.unwrap().path()) {
                        assert_ne!(
                            path, parent,
                            "restored receipt retained a mount-pinning parent descriptor"
                        );
                        assert_ne!(
                            path, transaction,
                            "restored receipt retained a mount-pinning stage descriptor"
                        );
                    }
                }
                DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
                assert_eq!(
                    discover_at(&root.path().join("copies")).unwrap()[0].evidence,
                    CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
                );
                if kind == "symlink" {
                    assert_eq!(
                        std::fs::read_link(receipt.before_path()).unwrap(),
                        root.path().join("new-target")
                    );
                    assert_eq!(
                        std::fs::read_link(root.path().join("item")).unwrap(),
                        root.path().join("new-target")
                    );
                    if overwrite {
                        assert_eq!(
                            std::fs::read_link(receipt.after_path()).unwrap(),
                            root.path().join("old-target")
                        );
                    }
                    assert_eq!(
                        std::fs::read_to_string(root.path().join("new-target")).unwrap(),
                        "new-target-data"
                    );
                    assert_eq!(
                        std::fs::read_to_string(root.path().join("old-target")).unwrap(),
                        "old-target-data"
                    );
                } else {
                    let data_path = |path: PathBuf| {
                        if kind == "directory" {
                            path.join("leaf")
                        } else {
                            path
                        }
                    };
                    assert_eq!(
                        std::fs::read_to_string(data_path(receipt.before_path().to_path_buf()))
                            .unwrap(),
                        "new-data"
                    );
                    assert_eq!(
                        std::fs::read_to_string(data_path(root.path().join("item"))).unwrap(),
                        "new-data"
                    );
                    if overwrite {
                        assert_eq!(
                            std::fs::read_to_string(data_path(receipt.after_path().to_path_buf()))
                                .unwrap(),
                            "old-data"
                        );
                    }
                }
                if !overwrite {
                    assert!(std::fs::symlink_metadata(receipt.after_path()).is_err());
                }
            }
        }
    }

    #[test]
    fn copy_undo_retries_durable_boundaries_without_repeating_the_withdrawal() {
        for point in [
            UndoPoint::IntentDurable,
            UndoPoint::WithdrawalDone,
            UndoPoint::DirectoriesSynced,
            UndoPoint::CompletedDurable,
        ] {
            for overwrite in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let _private = PrivateRoot::new(root.path());
                let receipt = copy_fixture(root.path(), overwrite, false);
                let parent = std::fs::File::open(root.path().join("destination")).unwrap();
                let stage = std::fs::File::open(receipt.before_path.parent().unwrap()).unwrap();
                let result =
                    undo_with_journals_with_hook(&parent, &stage, &receipt, &mut |current| {
                        if current == point {
                            Err(PlatformError::Io("test interruption".into()))
                        } else {
                            Ok(())
                        }
                    });
                assert!(result.is_err());
                DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
                DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
                let statuses = discover_at(&root.path().join("copies")).unwrap();
                assert_eq!(statuses[0].phase, Some(CopyPhase::Undone));
                assert_eq!(
                    statuses[0].evidence,
                    CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
                );
                assert_eq!(
                    std::fs::read_to_string(receipt.before_path()).unwrap(),
                    "new-data"
                );
                if overwrite {
                    assert_eq!(
                        std::fs::read_to_string(receipt.after_path()).unwrap(),
                        "old-data"
                    );
                } else {
                    assert!(!receipt.after_path().exists());
                }
                assert_eq!(
                    std::fs::read_to_string(root.path().join("item")).unwrap(),
                    "new-data"
                );
            }
        }
    }

    #[test]
    fn copy_undo_refuses_changed_evidence_and_competing_workers_without_repair() {
        for damage in [
            "withdrawn-edit",
            "journal-mismatch",
            "competing-worker",
            "stage-permissions",
        ] {
            let root = tempfile::tempdir().unwrap();
            let _private = PrivateRoot::new(root.path());
            let receipt = copy_fixture(root.path(), true, false);
            let parent = std::fs::File::open(root.path().join("destination")).unwrap();
            let stage_path = receipt.before_path.parent().unwrap();
            let stage = std::fs::File::open(stage_path).unwrap();
            if matches!(damage, "withdrawn-edit" | "journal-mismatch") {
                let point = if damage == "withdrawn-edit" {
                    UndoPoint::WithdrawalDone
                } else {
                    UndoPoint::IntentDurable
                };
                assert!(
                    undo_with_journals_with_hook(&parent, &stage, &receipt, &mut |current| {
                        if current == point {
                            Err(PlatformError::Io("test interruption".into()))
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
                );
            }
            let entries = discover_at(&root.path().join("copies")).unwrap();
            let central = root
                .path()
                .join("copies")
                .join(format!("{}.journal", entries[0].transaction_id));
            let local = stage_path.join("copy-journal");
            let directory =
                super::super::move_recovery::LinuxJournalDirectory::for_copy_stage(&stage).unwrap();
            let mut held_lock = None;
            match damage {
                "withdrawn-edit" => {
                    std::fs::write(receipt.before_path(), "user-edited-copy").unwrap()
                }
                "journal-mismatch" => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&central).unwrap()).unwrap();
                    value["phase"] = "published".into();
                    std::fs::write(&central, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "competing-worker" => held_lock = Some(directory.try_lock_recovery().unwrap()),
                "stage-permissions" => {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(stage_path, std::fs::Permissions::from_mode(0o707))
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let journals = (
                std::fs::read(&local).unwrap(),
                std::fs::read(&central).unwrap(),
            );
            let candidate = std::fs::read(receipt.before_path()).unwrap();
            let published = std::fs::read(receipt.after_path()).unwrap();
            let error = DefaultPlatform
                .undo_copy_with_receipt(&receipt)
                .unwrap_err();
            if damage == "competing-worker" {
                assert!(error.to_string().contains("exclusive recovery lock"));
            }
            assert_eq!(std::fs::read(&local).unwrap(), journals.0);
            assert_eq!(std::fs::read(&central).unwrap(), journals.1);
            assert_eq!(std::fs::read(receipt.before_path()).unwrap(), candidate);
            assert_eq!(std::fs::read(receipt.after_path()).unwrap(), published);
            assert_eq!(
                std::fs::read_to_string(root.path().join("item")).unwrap(),
                "new-data"
            );
            drop(held_lock);
        }
    }

    #[test]
    fn restore_copy_receipt_is_read_only_and_revalidates_changes_before_undo() {
        for damage in [
            "none",
            "edit-before",
            "edit-after",
            "journal-mismatch",
            "stage-replaced",
            "locked",
        ] {
            let root = tempfile::tempdir().unwrap();
            let _private = PrivateRoot::new(root.path());
            let original = copy_fixture(root.path(), true, false);
            let stage_path = original.before_path.parent().unwrap().to_path_buf();
            let publication = original.after_path.clone();
            let id = discover_at(&root.path().join("copies")).unwrap()[0]
                .transaction_id
                .clone();
            drop(original);
            let central = root.path().join("copies").join(format!("{id}.journal"));
            let local_path = stage_path.join("copy-journal");
            let stage = std::fs::File::open(&stage_path).unwrap();
            let local =
                super::super::move_recovery::LinuxJournalDirectory::for_copy_stage(&stage).unwrap();
            let mut held = None;
            match damage {
                "edit-before" => std::fs::write(&publication, "user-edit").unwrap(),
                "journal-mismatch" => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&central).unwrap()).unwrap();
                    value["phase"] = "undo-intent".into();
                    std::fs::write(&central, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "stage-replaced" => {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::rename(&stage_path, root.path().join("retained-original")).unwrap();
                    std::fs::create_dir(&stage_path).unwrap();
                    std::fs::set_permissions(&stage_path, std::fs::Permissions::from_mode(0o700))
                        .unwrap();
                    std::fs::copy(
                        root.path().join("retained-original/copy-journal"),
                        &local_path,
                    )
                    .unwrap();
                }
                "locked" => held = Some(local.try_lock_recovery().unwrap()),
                _ => {}
            }
            let journals = (
                std::fs::read(&central).unwrap(),
                std::fs::read(&local_path).unwrap(),
            );
            let bytes = std::fs::read(&publication).unwrap();
            let result = crate::restore_retained_copy_undo_receipt(&id);
            assert_eq!(std::fs::read(&central).unwrap(), journals.0);
            assert_eq!(std::fs::read(&local_path).unwrap(), journals.1);
            assert_eq!(std::fs::read(&publication).unwrap(), bytes);
            if matches!(damage, "none" | "edit-after") {
                let restored = result.unwrap();
                if damage == "edit-after" {
                    std::fs::write(&publication, "user-edit").unwrap();
                    assert!(matches!(
                        DefaultPlatform.undo_copy_with_receipt(&restored),
                        Err(PlatformError::UndoContentChanged(_))
                    ));
                    assert_eq!(std::fs::read_to_string(&publication).unwrap(), "user-edit");
                    assert_eq!(std::fs::read(&central).unwrap(), journals.0);
                    assert_eq!(std::fs::read(&local_path).unwrap(), journals.1);
                } else {
                    DefaultPlatform.undo_copy_with_receipt(&restored).unwrap();
                    assert_eq!(std::fs::read_to_string(&publication).unwrap(), "old-data");
                }
            } else {
                assert!(result.is_err(), "{damage}");
            }
            drop(held);
            assert_eq!(
                std::fs::read_to_string(root.path().join("item")).unwrap(),
                "new-data"
            );
        }
    }

    #[test]
    fn restore_copy_receipt_rejects_invalid_ids_without_creating_registry_state() {
        let root = tempfile::tempdir().unwrap();
        let _private = PrivateRoot::new(root.path());
        for id in [
            "",
            "../copy-journal",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "bad",
            "00000000000000000000000000000000",
        ] {
            assert!(crate::restore_retained_copy_undo_receipt(id).is_err());
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn sigkill_copy_undo_boundaries_recover_in_a_new_process() {
        const ROOT: &str = "OTTRIN_UNDO_CRASH_ROOT";
        const POINT: &str = "OTTRIN_UNDO_CRASH_POINT";
        if let (Some(root), Ok(point)) = (std::env::var_os(ROOT), std::env::var(POINT)) {
            let root = PathBuf::from(root);
            let _private = PrivateRoot::new(&root);
            let overwrite = std::env::var("OTTRIN_UNDO_CRASH_OVERWRITE").unwrap() == "true";
            let directory = std::env::var("OTTRIN_UNDO_CRASH_DIRECTORY").unwrap() == "true";
            let receipt = copy_fixture(&root, overwrite, directory);
            let parent = std::fs::File::open(root.join("destination")).unwrap();
            let stage = std::fs::File::open(receipt.before_path.parent().unwrap()).unwrap();
            let _ = undo_with_journals_with_hook(&parent, &stage, &receipt, &mut |current| {
                if format!("{current:?}") == point {
                    rustix::process::kill_process(
                        rustix::process::getpid(),
                        rustix::process::Signal::Kill,
                    )
                    .unwrap();
                }
                Ok(())
            });
            panic!("SIGKILL boundary was not reached");
        }
        for point in [
            UndoPoint::IntentDurable,
            UndoPoint::WithdrawalDone,
            UndoPoint::DirectoriesSynced,
            UndoPoint::CompletedDurable,
        ] {
            for overwrite in [false, true] {
                for directory in [false, true] {
                    let root = tempfile::tempdir().unwrap();
                    let _private = PrivateRoot::new(root.path());
                    let child = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["--exact", "copy_recovery::tests::sigkill_copy_undo_boundaries_recover_in_a_new_process", "--nocapture"])
                        .env(ROOT, root.path()).env(POINT, format!("{point:?}"))
                        .env("OTTRIN_UNDO_CRASH_OVERWRITE", overwrite.to_string())
                        .env("OTTRIN_UNDO_CRASH_DIRECTORY", directory.to_string()).spawn().unwrap();
                    let status = wait_owned_child(child);
                    use std::os::unix::process::ExitStatusExt;
                    assert_eq!(status.signal(), Some(9));
                    let statuses = discover_at(&root.path().join("copies")).unwrap();
                    assert_eq!(statuses.len(), 1);
                    let expected = match point {
                        UndoPoint::IntentDurable => CopyRecoveryEvidence::UndoReadyToWithdraw,
                        UndoPoint::WithdrawalDone | UndoPoint::DirectoriesSynced => {
                            CopyRecoveryEvidence::UndoWithdrawalMatches
                        }
                        UndoPoint::CompletedDurable => {
                            CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
                        }
                    };
                    assert_eq!(
                        statuses[0].evidence, expected,
                        "{point:?}, overwrite={overwrite}, directory={directory}"
                    );
                    let transaction = statuses[0].transaction.as_ref().unwrap();
                    assert_eq!(
                        std::fs::read(transaction.join("copy-journal")).unwrap(),
                        std::fs::read(
                            root.path()
                                .join("copies")
                                .join(format!("{}.journal", statuses[0].transaction_id))
                        )
                        .unwrap()
                    );
                    let publication = root.path().join("destination/item");
                    let data_path =
                        |path: PathBuf| if directory { path.join("leaf") } else { path };
                    assert_eq!(
                        std::fs::read_to_string(data_path(root.path().join("item"))).unwrap(),
                        "new-data"
                    );
                    if point == UndoPoint::IntentDurable {
                        assert_eq!(
                            std::fs::read_to_string(data_path(publication.clone())).unwrap(),
                            "new-data"
                        );
                        if overwrite {
                            assert_eq!(
                                std::fs::read_to_string(data_path(
                                    transaction.join(super::super::copy_transaction::CANDIDATE)
                                ))
                                .unwrap(),
                                "old-data"
                            );
                        }
                    } else {
                        assert_eq!(
                            std::fs::read_to_string(data_path(
                                transaction.join(super::super::copy_transaction::CANDIDATE)
                            ))
                            .unwrap(),
                            "new-data"
                        );
                        if overwrite {
                            assert_eq!(
                                std::fs::read_to_string(data_path(publication.clone())).unwrap(),
                                "old-data"
                            );
                        } else {
                            assert!(!publication.exists());
                        }
                    }
                    let local_path = transaction.join("copy-journal");
                    let central_path = root
                        .path()
                        .join("copies")
                        .join(format!("{}.journal", statuses[0].transaction_id));
                    let local_before = std::fs::read(&local_path).unwrap();
                    let central_before = std::fs::read(&central_path).unwrap();
                    let restored =
                        crate::restore_retained_copy_undo_receipt(&statuses[0].transaction_id);
                    assert_eq!(std::fs::read(&local_path).unwrap(), local_before);
                    assert_eq!(std::fs::read(&central_path).unwrap(), central_before);
                    if point == UndoPoint::CompletedDurable {
                        assert!(matches!(restored, Err(PlatformError::NotAvailable(_))));
                    } else {
                        let restored = restored.unwrap();
                        DefaultPlatform.undo_copy_with_receipt(&restored).unwrap();
                        DefaultPlatform.undo_copy_with_receipt(&restored).unwrap();
                    }
                    assert_eq!(
                        discover_at(&root.path().join("copies")).unwrap()[0].evidence,
                        CopyRecoveryEvidence::UndoneRetainedObjectsAndContentsMatch
                    );
                    assert_eq!(
                        std::fs::read_to_string(data_path(
                            transaction.join(super::super::copy_transaction::CANDIDATE)
                        ))
                        .unwrap(),
                        "new-data"
                    );
                    if overwrite {
                        assert_eq!(
                            std::fs::read_to_string(data_path(publication.clone())).unwrap(),
                            "old-data"
                        );
                    } else {
                        assert!(!publication.exists());
                    }
                    assert_eq!(
                        std::fs::read_to_string(data_path(root.path().join("item"))).unwrap(),
                        "new-data"
                    );
                }
            }
        }
    }

    fn handle(path: &Path) -> std::io::Result<Option<ObjectHandle>> {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?;
        ObjectHandle::capture(&fd)
    }

    #[test]
    fn content_sampling_refuses_a_file_binding_swapped_to_fifo_after_open() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("item");
        let retained = root.path().join("original");
        std::fs::write(&path, "original-data").unwrap();
        let result = super::super::move_transaction::source_tree_snapshot_with_observer(
            &path,
            &mut |relative, _| {
                assert!(relative.as_os_str().is_empty());
                std::fs::rename(&path, &retained).unwrap();
                rustix::fs::mknodat(
                    rustix::fs::CWD,
                    &path,
                    rustix::fs::FileType::Fifo,
                    rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
                    0,
                )
                .unwrap();
                Ok(())
            },
        );
        assert!(result.unwrap_err().to_string().contains("binding changed"));
        assert_eq!(std::fs::read_to_string(retained).unwrap(), "original-data");
        use std::os::unix::fs::FileTypeExt;
        assert!(
            std::fs::symlink_metadata(path)
                .unwrap()
                .file_type()
                .is_fifo()
        );
    }

    #[test]
    fn content_sampling_never_traverses_a_directory_redirected_to_an_outside_symlink() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("item");
        let outside = root.path().join("outside");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(path.join("inside"), "original-data").unwrap();
        std::fs::write(outside.join("outside-only"), "unrelated-data").unwrap();
        let retained = root.path().join("original");
        let mut visited = Vec::new();
        let result = super::super::move_transaction::source_tree_snapshot_with_observer(
            &path,
            &mut |relative, _| {
                visited.push(relative.to_path_buf());
                if relative.as_os_str().is_empty() {
                    std::fs::rename(&path, &retained).unwrap();
                    std::os::unix::fs::symlink(&outside, &path).unwrap();
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(visited.contains(&PathBuf::from("inside")));
        assert!(!visited.contains(&PathBuf::from("outside-only")));
        assert_eq!(
            std::fs::read_to_string(retained.join("inside")).unwrap(),
            "original-data"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("outside-only")).unwrap(),
            "unrelated-data"
        );
    }

    #[test]
    fn object_handles_preserve_rename_identity_and_distinguish_symlinks_and_replacements() {
        for base in [std::env::temp_dir(), std::env::current_dir().unwrap()] {
            let root = tempfile::Builder::new()
                .prefix(".ottrin-object-handle-")
                .tempdir_in(base)
                .unwrap();
            let path = root.path().join("item");
            std::fs::write(&path, "contents").unwrap();
            let Some(original) = handle(&path).unwrap() else {
                continue;
            };
            let renamed = root.path().join("renamed");
            std::fs::rename(&path, &renamed).unwrap();
            assert_eq!(handle(&renamed).unwrap().as_ref(), Some(&original));
            std::fs::write(&path, "contents").unwrap();
            assert_ne!(handle(&path).unwrap().as_ref(), Some(&original));
            let link = root.path().join("link");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_ne!(handle(&path).unwrap(), handle(&link).unwrap());
            std::fs::remove_file(&renamed).unwrap();
            assert_eq!(
                handle(&renamed).unwrap_err().kind(),
                std::io::ErrorKind::NotFound
            );
        }
    }
}
