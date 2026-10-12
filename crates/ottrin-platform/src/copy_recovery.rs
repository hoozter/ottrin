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
    fn capture(fd: &rustix::fd::OwnedFd) -> std::io::Result<Option<Self>> {
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
                fd.as_raw_fd(),
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
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CopyRecord {
    schema: u16,
    phase: CopyPhase,
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
    pub(super) fn publication_intent(
        source: &Path,
        publication: &Path,
        transaction: &Path,
        publication_evidence: (SourceTreeSnapshot, Vec<TreeObject>),
        directories: [&rustix::fd::OwnedFd; 2],
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
        Ok(Self {
            schema: 3,
            phase: CopyPhase::PublicationIntent,
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
            .filter(|name| {
                name.len() == 32
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
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
        if self.schema != 3
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
    entries
        .into_iter()
        .map(|(id, record)| {
            let mut status = RetainedCopyStatus {
                transaction_id: id.clone(),
                phase: None,
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
                let metadata = rustix::fs::fstat(&fd).map_err(journal_error)?;
                use rustix::fd::AsRawFd;
                let mount = super::local_storage::local_path_mount_identity(
                    &PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd())),
                    true,
                )
                .map_err(journal_error)?;
                if (metadata.st_dev, metadata.st_ino)
                    != (
                        record.transaction_identity.device,
                        record.transaction_identity.inode,
                    )
                    || metadata.st_uid != rustix::process::geteuid().as_raw()
                    || metadata.st_mode & 0o777 != 0o700
                    || mount != record.transaction_mount
                {
                    return Err(PlatformError::Io(
                        "opened Copy transaction directory does not match its journal".into(),
                    ));
                }
                if let Some(expected) = &record.transaction_handle
                    && ObjectHandle::capture(&fd).map_err(journal_error)?.as_ref() != Some(expected)
                {
                    return Err(PlatformError::Io(
                        "opened Copy transaction object generation changed".into(),
                    ));
                }
                let local = super::move_recovery::LinuxJournalDirectory::for_copy_stage(&fd)
                    .map_err(journal_error)?;
                let local = local.read_copy_record("copy-journal")?;
                if local != record {
                    return Err(PlatformError::Io(
                        "central and destination-local Copy journals differ".into(),
                    ));
                }
                record.validate_namespace()?;
                let evidence = if record.phase == CopyPhase::Published {
                    record.validate_published_data(&fd)?;
                    CopyRecoveryEvidence::PublishedObjectsAndContentsMatch
                } else {
                    CopyRecoveryEvidence::MatchingJournals
                };
                record.validate_namespace()?;
                Ok(evidence)
            })();
            status.evidence = match result {
                Ok(evidence) => evidence,
                Err(error) => CopyRecoveryEvidence::NeedsReview {
                    reason: error.to_string(),
                },
            };
            Ok(status)
        })
        .collect()
}

impl CopyRecord {
    fn validate_published_data(&self, stage: &rustix::fd::OwnedFd) -> Result<(), PlatformError> {
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
        let publication = self.publication.decode().map_err(journal_error)?;
        self.validate_tree(
            &publication,
            &self.publication_content,
            &self.publication_objects,
        )?;
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
