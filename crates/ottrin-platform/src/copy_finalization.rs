//! Group-wide retirement after retained payloads have been reclaimed.
use super::*;
use rustix::fd::OwnedFd;
use rustix::fs::{AtFlags, FlockOperation, flock};

#[path = "copy_finalization_guard.rs"]
mod namespace_guard;
use namespace_guard::NamespaceGuard;

const PREFIX: &str = "copy-finalize-";
const SUFFIX: &str = ".cleanup";

struct PinnedFile {
    file: std::fs::File,
    stamp: LedgerStamp,
    locked: bool,
}

impl PinnedFile {
    fn lock(file: std::fs::File) -> Result<Self, PlatformError> {
        let stamp = LedgerStamp::read(&file)?;
        let mut pinned = Self {
            file,
            stamp,
            locked: false,
        };
        flock(&pinned.file, FlockOperation::NonBlockingLockExclusive).map_err(journal_error)?;
        pinned.locked = true;
        Ok(pinned)
    }
}

impl Drop for PinnedFile {
    fn drop(&mut self) {
        if self.locked {
            // An unrelated thread may fork while this descriptor is open.
            // Explicit unlock releases the shared OFD lock before its inherited
            // copy reaches exec; waiting for the last close can spuriously block
            // an immediate retry in this process.
            let _ = flock(&self.file, FlockOperation::Unlock);
        }
    }
}

// Worker-local content memoization, never deletion authority. Every lookup still
// opens the current name without following links and checks generation, mount,
// and the complete inode stamp. This bounded worker cache holds only one
// notification descriptor and no per-file descriptors.
#[derive(Default)]
struct MetadataReads {
    files: BTreeMap<(u64, u64), CachedMetadata>,
    bytes: usize,
    enabled: bool,
    events: Option<OwnedFd>,
    changed: BTreeSet<i32>,
    guard: Option<NamespaceGuard>,
    parsed: CopyRecordCache,
}
struct CachedMetadata {
    stamp: LedgerStamp,
    watch: i32,
    proof: FileProof,
    bytes: std::sync::Arc<[u8]>,
}

impl MetadataReads {
    fn drain(&mut self) -> Result<(), PlatformError> {
        use rustix::fs::inotify;
        let Some(events) = &self.events else {
            return Ok(());
        };
        let mut buffer = [std::mem::MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(events, &mut buffer);
        loop {
            match reader.next() {
                Ok(event) => {
                    if event.events().contains(inotify::ReadFlags::QUEUE_OVERFLOW) {
                        return Err(refusal("Copy finalization metadata watch overflowed"));
                    }
                    if let Some(guard) = &mut self.guard
                        && guard.event(
                            event.wd(),
                            event.events(),
                            event.file_name().map(|name| name.to_bytes()),
                        )?
                    {
                        continue;
                    }
                    self.changed.insert(event.wd());
                }
                Err(rustix::io::Errno::WOULDBLOCK) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(journal_error(error)),
            }
        }
    }

    fn record(
        &mut self,
        registry: &OwnedFd,
        name: &str,
        expected_mount: u64,
    ) -> Result<std::sync::Arc<CopyRecord>, PlatformError> {
        let (_, _, bytes) = FileProof::capture_with_reads(registry, name, expected_mount, self)?;
        self.parsed.parse(name, bytes)
    }

    fn check_guard(&mut self) -> Result<(), PlatformError> {
        if let Some(guard) = &self.guard {
            guard.check_mounts()?;
        }
        self.drain()
    }

    fn watch(&mut self, file: &std::fs::File) -> Option<i32> {
        use rustix::fs::inotify;
        // Remote/userspace filesystems retain full rereads and group checks.
        if !crate::local_storage::supports_local_inode_notifications(file) {
            return None;
        }
        if self.events.is_none() {
            self.events =
                inotify::init(inotify::CreateFlags::NONBLOCK | inotify::CreateFlags::CLOEXEC).ok();
        }
        inotify::add_watch(
            self.events.as_ref()?,
            path_from_fd(file),
            inotify::WatchFlags::MODIFY
                | inotify::WatchFlags::ATTRIB
                | inotify::WatchFlags::DELETE_SELF
                | inotify::WatchFlags::MOVE_SELF,
        )
        .ok()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileProof {
    identity: PathMoveIdentity,
    handle: ObjectHandle,
    length: u64,
    digest: String,
}

impl FileProof {
    fn capture(
        directory: &impl rustix::fd::AsFd,
        name: &str,
        expected_mount: u64,
    ) -> Result<(PinnedFile, Self, std::sync::Arc<[u8]>), PlatformError> {
        Self::capture_with_reads(
            directory,
            name,
            expected_mount,
            &mut MetadataReads::default(),
        )
    }

    fn capture_with_reads(
        directory: &impl rustix::fd::AsFd,
        name: &str,
        expected_mount: u64,
        reads: &mut MetadataReads,
    ) -> Result<(PinnedFile, Self, std::sync::Arc<[u8]>), PlatformError> {
        let fd = rustix::fs::openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        let mut file = std::fs::File::from(fd);
        let stat = rustix::fs::fstat(&file).map_err(journal_error)?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
            || stat.st_uid != rustix::process::geteuid().as_raw()
            || stat.st_mode & 0o777 != 0o600
            || mount(&file)? != expected_mount
        {
            return Err(refusal(
                "Copy finalization metadata type, owner, mode or mount changed",
            ));
        }
        let handle = ObjectHandle::capture(&file)
            .map_err(journal_error)?
            .ok_or_else(|| refusal("Copy finalization file generation is unavailable"))?;
        let before = LedgerStamp::read(&file)?;
        let key = (stat.st_dev, stat.st_ino);
        reads.drain()?;
        if let Some(cached) = reads.files.get(&key) {
            if cached.stamp != before
                || cached.proof.handle != handle
                || reads.changed.contains(&cached.watch)
                || LedgerStamp::read(&file)? != before
            {
                return Err(refusal("Copy finalization metadata changed during cleanup"));
            }
            return Ok((
                PinnedFile {
                    file,
                    stamp: before,
                    locked: false,
                },
                cached.proof.clone(),
                cached.bytes.clone(),
            ));
        }
        let watch = if reads.enabled && reads.bytes < 64 * 1024 * 1024 && reads.files.len() < 4096 {
            reads.watch(&file)
        } else {
            None
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(journal_error)?;
        if LedgerStamp::read(&file)? != before || before.size != bytes.len() as u64 {
            return Err(refusal("Copy finalization metadata changed while reading"));
        }
        let proof = Self {
            identity: PathMoveIdentity {
                device: stat.st_dev,
                inode: stat.st_ino,
            },
            handle,
            length: bytes.len() as u64,
            digest: digest(&bytes),
        };
        let bytes: std::sync::Arc<[u8]> = bytes.into();
        // Large groups fall back to ordinary verified reads beyond this bound.
        if let Some(watch) =
            watch.filter(|_| reads.bytes.saturating_add(bytes.len()) <= 64 * 1024 * 1024)
        {
            reads.bytes += bytes.len();
            reads.files.insert(
                key,
                CachedMetadata {
                    stamp: before.clone(),
                    watch,
                    proof: proof.clone(),
                    bytes: bytes.clone(),
                },
            );
        }
        Ok((
            PinnedFile {
                file,
                stamp: before,
                locked: false,
            },
            proof,
            bytes,
        ))
    }

    fn open_exact(
        &self,
        directory: &impl rustix::fd::AsFd,
        name: &str,
        expected_mount: u64,
    ) -> Result<PinnedFile, PlatformError> {
        self.open_exact_with_reads(
            directory,
            name,
            expected_mount,
            &mut MetadataReads::default(),
        )
    }

    fn open_exact_with_reads(
        &self,
        directory: &impl rustix::fd::AsFd,
        name: &str,
        expected_mount: u64,
        reads: &mut MetadataReads,
    ) -> Result<PinnedFile, PlatformError> {
        let (file, current, _) = Self::capture_with_reads(directory, name, expected_mount, reads)?;
        if current != *self {
            return Err(refusal(
                "Copy finalization file identity or contents changed",
            ));
        }
        Ok(file)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    record: CopyRecord,
    local: FileProof,
    ledger: FileProof,
    canonical: FileProof,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    registry_identity: PathMoveIdentity,
    registry_handle: ObjectHandle,
    registry_mount: u64,
    entries: BTreeMap<String, Entry>,
}

impl Manifest {
    fn bytes(&self) -> Result<Vec<u8>, PlatformError> {
        serde_json::to_vec(self).map_err(journal_error)
    }
    fn id(&self) -> Result<String, PlatformError> {
        Ok(digest(&self.bytes()?))
    }
    fn validate(&self) -> Result<(), PlatformError> {
        if self.schema != 1 || self.entries.is_empty() || !self.registry_handle.valid() {
            return Err(refusal("Invalid Copy finalization manifest"));
        }
        for (id, entry) in &self.entries {
            entry.record.validate()?;
            if entry.record.phase != CopyPhase::CleanupComplete
                || entry.record.transaction_id()? != *id
                || !entry.local.handle.valid()
                || !entry.ledger.handle.valid()
                || !entry.canonical.handle.valid()
            {
                return Err(refusal(
                    "Finalization is not bound to completed Copy payloads",
                ));
            }
            let intent = entry
                .record
                .cleanup
                .as_ref()
                .expect("validated complete intent");
            if entry.ledger.identity != intent.identity || entry.ledger.handle != intent.handle {
                return Err(refusal(
                    "Finalization ledger does not match durable cleanup intent",
                ));
            }
        }
        Ok(())
    }
}

/// Restartable authority for retiring the private metadata of a complete Copy
/// group. Holds no descriptors between calls; its on-disk manifest is removed last.
#[derive(Debug)]
pub struct RetainedCopyFinalization {
    root: PathBuf,
    id: String,
    manifest: Manifest,
}

impl RetainedCopyFinalization {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn transaction_ids(&self) -> Vec<&str> {
        self.manifest.entries.keys().map(String::as_str).collect()
    }

    /// Success means all selected private stages, their journals and the group
    /// manifest are durably absent. On error retain ownership and reopen the ID.
    pub fn finish(&self) -> Result<(), PlatformError> {
        self.finish_with_hook(&mut |_| Ok(()))
    }

    fn finish_with_hook(
        &self,
        hook: &mut impl FnMut(FinalizationPoint) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let result = self.finish_inner(hook);
        result.map_err(|error| PlatformError::CleanupPending {
            transaction: self.root.join(filename(&self.id)).display().to_string(),
            message: format!("Copy finalization is incomplete; retain its manifest: {error}"),
        })
    }

    fn finish_inner(
        &self,
        hook: &mut impl FnMut(FinalizationPoint) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let central =
            LinuxJournalDirectory::open_copy_registry(&self.root, false).map_err(journal_error)?;
        let registry = central.duplicate_copy_directory().map_err(journal_error)?;
        if !exists(&path_from_fd(&registry).join(filename(&self.id)))? {
            // An unlink can succeed before its final fsync reports failure.
            // Only this original in-memory authority can finish that last sync;
            // IDs alone cannot reconstruct a missing manifest.
            self.validate_state(&central, &registry)?;
            self.require_absent_stages()?;
            for id in self.manifest.entries.keys() {
                if exists(&path_from_fd(&registry).join(format!("{id}.journal")))? {
                    return Err(refusal(
                        "Copy finalization manifest is missing before retirement completed",
                    ));
                }
            }
            rustix::fs::fsync(&registry).map_err(journal_error)?;
            return Ok(());
        }
        let (manifest_file, manifest_proof, manifest) = load(&registry, &self.id)?;
        if manifest != self.manifest {
            return Err(refusal("Copy finalization manifest changed"));
        }
        let mut reads = MetadataReads {
            enabled: true,
            ..Default::default()
        };
        // Monitoring is optional acceleration of the same full validator. Set
        // it up before admission so queued changes invalidate that admission.
        match NamespaceGuard::new(
            &mut reads,
            &self.root,
            &registry,
            &filename(&self.id),
            &self.manifest.entries,
        ) {
            Ok(guard) => reads.guard = Some(guard),
            Err(_) => {
                reads.events = None;
                reads.changed.clear();
            }
        }
        // Close the interval between loading authority and starting its watch,
        // including filesystems with coarse inode timestamps.
        manifest_proof.open_exact(&registry, &filename(&self.id), self.manifest.registry_mount)?;
        // The manifest lock serializes finalizers, and local locks serialize
        // stage mutation against the existing Copy/Undo workers.
        self.validate_state_with_reads(&central, &registry, &mut reads)?;
        reads.check_guard()?;
        for entry in self.manifest.entries.values() {
            let record = &entry.record;
            let transaction = record.transaction.decode().map_err(journal_error)?;
            if !exists(&transaction)? {
                continue;
            }
            let (parent, stage) = open_directories(record)?;
            let local = LinuxJournalDirectory::for_copy_stage(&stage).map_err(journal_error)?;
            let _lock = local.try_lock_recovery().map_err(journal_error)?;
            self.revalidate(&central, &registry, &mut reads)?;
            self.validate_stage(entry, &mut reads)?;
            for (name, proof, point) in [
                (LEDGER, &entry.ledger, FinalizationPoint::LedgerRemoved),
                (
                    "copy-journal",
                    &entry.local,
                    FinalizationPoint::LocalJournalRemoved,
                ),
            ] {
                if exists(&path_from_fd(&stage).join(name))? {
                    let pinned = proof.open_exact(&stage, name, record.transaction_mount)?;
                    record.validate_namespace()?;
                    self.check_registry(&registry)?;
                    self.check_manifest(&registry, &manifest_file)?;
                    hook(FinalizationPoint::BeforeMetadataUnlink)?;
                    reads.check_guard()?;
                    self.check_registry(&registry)?;
                    self.check_manifest(&registry, &manifest_file)?;
                    record.validate_namespace()?;
                    unlink_pinned(&stage, name, &pinned)?;
                    if let Some(guard) = &mut reads.guard {
                        guard.removed_file(&stage, name, &pinned)?;
                    }
                    rustix::fs::fsync(&stage).map_err(journal_error)?;
                    hook(point)?;
                }
            }
            // rmdir alone is the authority to remove an empty original stage.
            // Unknown children, aliases and replaced names can never be recursed.
            record.validate_namespace()?;
            record.validate_opened_directory(&parent, true)?;
            record.validate_opened_directory(&stage, false)?;
            self.revalidate(&central, &registry, &mut reads)?;
            self.validate_stage(entry, &mut reads)?;
            self.check_manifest(&registry, &manifest_file)?;
            hook(FinalizationPoint::BeforeStageRemove)?;
            reads.check_guard()?;
            self.check_registry(&registry)?;
            self.check_manifest(&registry, &manifest_file)?;
            record.validate_namespace()?;
            record.validate_opened_directory(&stage, false)?;
            rustix::fs::unlinkat(
                &parent,
                transaction.file_name().expect("validated stage"),
                AtFlags::REMOVEDIR,
            )
            .map_err(journal_error)?;
            if let Some(guard) = &mut reads.guard {
                guard.removed_stage(&parent, transaction.file_name().unwrap(), &stage)?;
            }
            rustix::fs::fsync(&parent).map_err(journal_error)?;
            hook(FinalizationPoint::StageRemoved)?;
        }
        // All stages must be absent before the first canonical journal goes.
        // Thus a missing journal never authorizes deleting a surviving stage.
        self.validate_state_with_reads(&central, &registry, &mut reads)?;
        self.require_absent_stages()?;
        reads.check_guard()?;
        for (id, entry) in &self.manifest.entries {
            let name = format!("{id}.journal");
            if exists(&path_from_fd(&registry).join(&name))? {
                let pinned =
                    entry
                        .canonical
                        .open_exact(&registry, &name, self.manifest.registry_mount)?;
                self.revalidate(&central, &registry, &mut reads)?;
                self.check_manifest(&registry, &manifest_file)?;
                hook(FinalizationPoint::BeforeCanonicalUnlink)?;
                reads.check_guard()?;
                self.check_registry(&registry)?;
                self.check_manifest(&registry, &manifest_file)?;
                if reads.guard.is_some() {
                    entry.record.validate_parent_namespace()?;
                    if exists(&entry.record.transaction.decode().map_err(journal_error)?)? {
                        return Err(refusal(
                            "A Copy private stage reappeared before canonical retirement",
                        ));
                    }
                } else {
                    self.require_absent_stages()?;
                }
                unlink_pinned(&registry, &name, &pinned)?;
                if let Some(guard) = &mut reads.guard {
                    guard.removed_file(&registry, &name, &pinned)?;
                }
                rustix::fs::fsync(&registry).map_err(journal_error)?;
                hook(FinalizationPoint::CanonicalJournalRemoved)?;
            }
        }
        self.validate_state_with_reads(&central, &registry, &mut reads)?;
        self.check_manifest(&registry, &manifest_file)?;
        hook(FinalizationPoint::BeforeManifestRemove)?;
        reads.check_guard()?;
        self.check_registry(&registry)?;
        self.check_manifest(&registry, &manifest_file)?;
        self.require_absent_stages()?;
        unlink_pinned(&registry, &filename(&self.id), &manifest_file)?;
        rustix::fs::fsync(&registry).map_err(journal_error)?;
        hook(FinalizationPoint::ManifestRemoved)?;
        Ok(())
    }

    fn require_absent_stages(&self) -> Result<(), PlatformError> {
        for entry in self.manifest.entries.values() {
            entry.record.validate_parent_namespace()?;
            if exists(&entry.record.transaction.decode().map_err(journal_error)?)? {
                return Err(refusal(
                    "A Copy private stage remains before canonical retirement",
                ));
            }
        }
        Ok(())
    }

    fn check_manifest(&self, registry: &OwnedFd, pinned: &PinnedFile) -> Result<(), PlatformError> {
        validate_pinned(registry, &filename(&self.id), pinned)
    }

    fn check_registry(&self, registry: &OwnedFd) -> Result<(), PlatformError> {
        let named = rustix::fs::open(
            &self.root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        for fd in [registry, &named] {
            let stat = rustix::fs::fstat(fd).map_err(journal_error)?;
            if (stat.st_dev, stat.st_ino)
                != (
                    self.manifest.registry_identity.device,
                    self.manifest.registry_identity.inode,
                )
                || mount(fd)? != self.manifest.registry_mount
                || ObjectHandle::capture(fd).map_err(journal_error)?.as_ref()
                    != Some(&self.manifest.registry_handle)
            {
                return Err(refusal("Copy finalization registry binding changed"));
            }
        }
        Ok(())
    }

    fn validate_stage(
        &self,
        entry: &Entry,
        reads: &mut MetadataReads,
    ) -> Result<bool, PlatformError> {
        let record = &entry.record;
        record.validate_parent_namespace()?;
        let transaction = record.transaction.decode().map_err(journal_error)?;
        let present = exists(&transaction)?;
        if present {
            let (_, stage) = open_directories(record)?;
            let mut directory = rustix::fs::Dir::read_from(&stage).map_err(journal_error)?;
            while let Some(child) = directory.read() {
                let child = child.map_err(journal_error)?;
                let name = child.file_name();
                match name.to_bytes() {
                    b"." | b".." => {}
                    b"copy-journal" => {
                        entry.local.open_exact_with_reads(
                            &stage,
                            "copy-journal",
                            record.transaction_mount,
                            reads,
                        )?;
                    }
                    b"copy-cleanup" => {
                        entry.ledger.open_exact_with_reads(
                            &stage,
                            LEDGER,
                            record.transaction_mount,
                            reads,
                        )?;
                    }
                    _ => {
                        return Err(refusal(
                            "Unexpected data remains in a Copy finalization stage",
                        ));
                    }
                }
            }
        }
        Ok(present)
    }

    fn revalidate(
        &self,
        central: &LinuxJournalDirectory,
        registry: &OwnedFd,
        reads: &mut MetadataReads,
    ) -> Result<(), PlatformError> {
        if reads.guard.is_some() {
            reads.check_guard()
        } else {
            self.validate_state_with_reads(central, registry, reads)
        }
    }

    fn validate_state(
        &self,
        central: &LinuxJournalDirectory,
        registry: &OwnedFd,
    ) -> Result<(), PlatformError> {
        self.validate_state_with_reads(central, registry, &mut MetadataReads::default())
    }

    fn validate_state_with_reads(
        &self,
        central: &LinuxJournalDirectory,
        registry: &OwnedFd,
        reads: &mut MetadataReads,
    ) -> Result<(), PlatformError> {
        self.check_registry(registry)?;
        let all = central
            .discover_copy_records_with(|name| {
                reads.record(registry, name, self.manifest.registry_mount)
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
            .collect::<Result<BTreeMap<_, _>, PlatformError>>()?;
        let outside: Vec<_> = all
            .iter()
            .filter(|(id, _)| !self.manifest.entries.contains_key(*id))
            .map(|(_, record)| record)
            .collect();
        let mut missing_canonical = false;
        let mut surviving_stage = false;
        for (id, entry) in &self.manifest.entries {
            let record = &entry.record;
            record.require_object_evidence()?;
            match all.get(id) {
                Some(current) if current.as_ref() == record => {
                    entry.canonical.open_exact_with_reads(
                        registry,
                        &format!("{id}.journal"),
                        self.manifest.registry_mount,
                        reads,
                    )?;
                }
                None => missing_canonical = true,
                _ => return Err(refusal("Copy finalization canonical record changed")),
            }
            surviving_stage |= self.validate_stage(entry, reads)?;
            let stage_object = TreeObject {
                path: NativePath::encode(Path::new("")),
                mount: record.transaction_mount,
                handle: record.transaction_handle.clone(),
            };
            for other in &outside {
                if other.operation.id == record.operation.id {
                    return Err(refusal(
                        "Copy operation membership changed during finalization",
                    ));
                }
                if possible_reference(other, std::slice::from_ref(&stage_object))? {
                    return Err(refusal(
                        "Another Copy operation still references the private stage",
                    ));
                }
            }
        }
        if missing_canonical && surviving_stage {
            return Err(refusal(
                "Copy canonical journal disappeared before all private stages were removed",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalizationPoint {
    BeforeMetadataUnlink,
    BeforeStageRemove,
    BeforeCanonicalUnlink,
    BeforeManifestRemove,
    LedgerRemoved,
    LocalJournalRemoved,
    StageRemoved,
    CanonicalJournalRemoved,
    ManifestRemoved,
}

fn filename(id: &str) -> String {
    format!("{PREFIX}{id}{SUFFIX}")
}
fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn validate_pinned(
    directory: &impl rustix::fd::AsFd,
    name: &str,
    pinned: &PinnedFile,
) -> Result<(), PlatformError> {
    let named = rustix::fs::openat(
        directory,
        name,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    let named = std::fs::File::from(named);
    if LedgerStamp::read(&pinned.file)? != pinned.stamp
        || LedgerStamp::read(&named)? != pinned.stamp
        || mount(&named)? != mount(&pinned.file)?
    {
        return Err(refusal("Copy finalization file changed before unlink"));
    }
    Ok(())
}

fn unlink_pinned(
    directory: &impl rustix::fd::AsFd,
    name: &str,
    pinned: &PinnedFile,
) -> Result<(), PlatformError> {
    validate_pinned(directory, name, pinned)?;
    rustix::fs::unlinkat(directory, name, AtFlags::empty()).map_err(journal_error)
}

// load owns a nonblocking exclusive lock, released with its returned File.
fn load(registry: &OwnedFd, id: &str) -> Result<(PinnedFile, FileProof, Manifest), PlatformError> {
    if !valid_id(id) {
        return Err(refusal("Invalid Copy finalization ID"));
    }
    let fd = rustix::fs::openat(
        registry,
        filename(id),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(journal_error)?;
    let locked = PinnedFile::lock(std::fs::File::from(fd))?;
    let (file, proof, bytes) = FileProof::capture(registry, &filename(id), mount(registry)?)?;
    let stat = rustix::fs::fstat(&locked.file).map_err(journal_error)?;
    if (stat.st_dev, stat.st_ino) != (proof.identity.device, proof.identity.inode) {
        return Err(refusal("Copy finalization manifest changed while locking"));
    }
    if LedgerStamp::read(&locked.file)? != file.stamp {
        return Err(refusal("Copy finalization manifest changed while locking"));
    }
    if locked.stamp != file.stamp {
        return Err(refusal("Copy finalization manifest changed while locking"));
    }
    drop(file);
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(journal_error)?;
    manifest.validate()?;
    if manifest.id()? != id || manifest.bytes()?.as_slice() != bytes.as_ref() {
        return Err(refusal(
            "Copy finalization manifest name or encoding changed",
        ));
    }
    Ok((locked, proof, manifest))
}

pub(super) fn begin(
    plan: &RetainedCopyCleanupPlan,
) -> Result<RetainedCopyFinalization, PlatformError> {
    if !plan.pending.is_empty()
        || plan
            .records
            .values()
            .any(|record| record.phase != CopyPhase::CleanupComplete)
    {
        return Err(refusal(
            "Finish all Copy payload reclamation before finalization",
        ));
    }
    plan.revalidate()?;
    let central =
        LinuxJournalDirectory::open_copy_registry(&plan.root, false).map_err(journal_error)?;
    let _creation_lock = central.try_lock_recovery().map_err(journal_error)?;
    let registry = central.duplicate_copy_directory().map_err(journal_error)?;
    let stat = rustix::fs::fstat(&registry).map_err(journal_error)?;
    let registry_mount = mount(&registry)?;
    let mut entries = BTreeMap::new();
    for (id, record) in &plan.records {
        let (_, stage) = open_directories(record)?;
        let (_, local, bytes) =
            FileProof::capture(&stage, "copy-journal", record.transaction_mount)?;
        if CopyRecord::parse(&bytes)? != *record {
            return Err(refusal("Copy local journal changed before finalization"));
        }
        let (_, ledger, _) = FileProof::capture(&stage, LEDGER, record.transaction_mount)?;
        let (_, canonical, bytes) =
            FileProof::capture(&registry, &format!("{id}.journal"), registry_mount)?;
        if CopyRecord::parse(&bytes)? != *record {
            return Err(refusal(
                "Copy canonical journal changed before finalization",
            ));
        }
        entries.insert(
            id.clone(),
            Entry {
                record: record.clone(),
                local,
                ledger,
                canonical,
            },
        );
    }
    let manifest = Manifest {
        schema: 1,
        registry_identity: PathMoveIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
        },
        registry_handle: ObjectHandle::capture(&registry)
            .map_err(journal_error)?
            .ok_or_else(|| refusal("Copy registry generation is unavailable"))?,
        registry_mount,
        entries,
    };
    manifest.validate()?;
    let id = manifest.id()?;
    let result = RetainedCopyFinalization {
        root: plan.root.clone(),
        id,
        manifest,
    };
    result.validate_state(&central, &registry)?;
    for existing_id in manifest_ids(&registry)? {
        if existing_id == result.id {
            continue;
        }
        let (_lock, _, existing) = load(&registry, &existing_id)?;
        if existing
            .entries
            .keys()
            .any(|id| result.manifest.entries.contains_key(id))
        {
            return Err(refusal(
                "A selected Copy transaction already belongs to another finalization; resume that group first",
            ));
        }
    }
    let fd = rustix::fs::openat(
        &registry,
        filename(&result.id),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    );
    match fd {
        Ok(fd) => {
            let mut pinned = PinnedFile::lock(std::fs::File::from(fd))?;
            pinned
                .file
                .write_all(&result.manifest.bytes()?)
                .and_then(|()| pinned.file.sync_all())
                .map_err(journal_error)?;
            rustix::fs::fsync(&registry).map_err(journal_error)?;
        }
        Err(rustix::io::Errno::EXIST) => {
            let (_, _, existing) = load(&registry, &result.id)?;
            if existing != result.manifest {
                return Err(refusal("Existing Copy finalization differs"));
            }
        }
        Err(error) => return Err(journal_error(error)),
    }
    Ok(result)
}

pub fn resume_finalization(id: &str) -> Result<RetainedCopyFinalization, PlatformError> {
    let root = configured_root()?;
    let central = LinuxJournalDirectory::open_copy_registry(&root, false).map_err(journal_error)?;
    let registry = central.duplicate_copy_directory().map_err(journal_error)?;
    let (_lock, _, manifest) = load(&registry, id)?;
    let result = RetainedCopyFinalization {
        root,
        id: id.into(),
        manifest,
    };
    result.validate_state(&central, &registry)?;
    Ok(result)
}

fn manifest_ids(fd: &OwnedFd) -> Result<BTreeSet<String>, PlatformError> {
    let mut directory = rustix::fs::Dir::read_from(fd).map_err(journal_error)?;
    let mut ids = BTreeSet::new();
    while let Some(entry) = directory.read() {
        let entry = entry.map_err(journal_error)?;
        let bytes = entry.file_name().to_bytes().to_vec();
        if let Some(id) = bytes
            .strip_prefix(PREFIX.as_bytes())
            .and_then(|name| name.strip_suffix(SUFFIX.as_bytes()))
        {
            ids.insert(std::str::from_utf8(id).map_err(journal_error)?.to_owned());
        }
    }
    Ok(ids)
}

pub fn discover_finalizations() -> Result<Vec<RetainedCopyFinalization>, PlatformError> {
    let root = configured_root()?;
    if !exists(&root)? {
        return Ok(Vec::new());
    }
    let central = LinuxJournalDirectory::open_copy_registry(&root, false).map_err(journal_error)?;
    let fd = central.duplicate_copy_directory().map_err(journal_error)?;
    manifest_ids(&fd)?
        .into_iter()
        .map(|id| {
            resume_finalization(&id).map_err(|error| PlatformError::CleanupPending {
                transaction: root.join(filename(&id)).display().to_string(),
                message: format!("Preserve Copy finalization for review: {error}"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Registry, directory_backup, stage, statuses};
    use super::*;
    use crate::{ConflictAction, DefaultPlatform, PlatformOps};

    fn completed_group(root: &Path) -> (RetainedCopyFinalization, Vec<PathBuf>) {
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let paths = ["alpha", "beta"].map(|name| {
            std::fs::write(destination.join(name), format!("old {name}")).unwrap();
            let path = source.join(name);
            std::fs::write(&path, format!("new {name}")).unwrap();
            path
        });
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &paths,
            &destination,
            ConflictAction::Overwrite,
        );
        assert!(batch.error.is_none());
        assert_eq!(batch.undo_receipts.len(), 2);
        let stages = batch.undo_receipts.iter().map(stage).collect();
        let mut plan = plan_cleanup(&statuses(root)).unwrap();
        while plan.reclaim_payload_next().unwrap() {}
        (plan.begin_finalization().unwrap(), stages)
    }

    fn assert_finished(root: &Path, stages: &[PathBuf]) {
        for stage in stages {
            assert!(!stage.exists());
        }
        assert!(statuses(root).is_empty());
        assert!(discover_finalizations().unwrap().is_empty());
        for name in ["alpha", "beta"] {
            assert_eq!(
                std::fs::read(root.join("destination").join(name)).unwrap(),
                format!("new {name}").as_bytes()
            );
            assert_eq!(
                std::fs::read(root.join("source").join(name)).unwrap(),
                format!("new {name}").as_bytes()
            );
        }
    }

    #[test]
    fn cached_metadata_detects_alias_writes_even_when_stamps_compare_equal() {
        let root = tempfile::tempdir().unwrap();
        let registry = rustix::fs::open(
            root.path(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .unwrap();
        let file = root.path().join("metadata");
        std::fs::write(&file, b"original").unwrap();
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let alias = root.path().join("alias");
        std::fs::hard_link(&file, &alias).unwrap();
        let mut reads = MetadataReads {
            enabled: true,
            ..Default::default()
        };
        let (_, proof, _) = FileProof::capture_with_reads(
            &registry,
            "metadata",
            mount(&registry).unwrap(),
            &mut reads,
        )
        .unwrap();
        assert_eq!(
            reads.files.len(),
            1,
            "test requires a locally watched inode"
        );
        std::fs::write(&alias, b"modified").unwrap();
        // Model equal timestamps on a filesystem with coarse timestamp precision.
        // The independent inode watch must still invalidate the captured bytes.
        reads
            .files
            .get_mut(&(proof.identity.device, proof.identity.inode))
            .unwrap()
            .stamp = LedgerStamp::read(&std::fs::File::open(&file).unwrap()).unwrap();
        assert!(
            FileProof::capture_with_reads(
                &registry,
                "metadata",
                mount(&registry).unwrap(),
                &mut reads
            )
            .is_err()
        );
        assert_eq!(std::fs::read(file).unwrap(), b"modified");
    }

    #[test]
    fn finalization_refuses_metadata_rewritten_through_an_outside_alias() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (finalization, stages) = completed_group(root.path());
        let id = finalization.transaction_ids().last().unwrap().to_string();
        let canonical = root.path().join("copies").join(format!("{id}.journal"));
        let alias = root.path().join("outside-registry-alias");
        std::fs::hard_link(&canonical, &alias).unwrap();
        let bytes = std::fs::read(&canonical).unwrap();
        let mut rewritten = false;
        let result = finalization.finish_with_hook(&mut |point| {
            if point == FinalizationPoint::LedgerRemoved && !rewritten {
                // Even an identical-byte rewrite invalidates the active proof.
                std::fs::write(&alias, &bytes).unwrap();
                rewritten = true;
            }
            Ok(())
        });
        assert!(rewritten);
        assert!(result.is_err());
        assert_eq!(std::fs::read(&canonical).unwrap(), bytes);
        assert!(stages.iter().all(|stage| stage.exists()));
        // A fresh worker reads and verifies the current metadata again.
        finalization.finish().unwrap();
        assert_finished(root.path(), &stages);
        assert_eq!(std::fs::read(&alias).unwrap(), bytes);
    }

    #[test]
    fn finalizing_large_groups_does_not_reread_every_journal_per_item() {
        finalization_scaling_fixture(32);
    }

    #[test]
    #[ignore = "explicit multi-size resource profile"]
    fn finalization_scaling_profile() {
        for count in [8, 32, 128] {
            finalization_scaling_fixture(count);
        }
    }

    fn finalization_scaling_fixture(count: usize) {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let mut sources = Vec::new();
        for index in 0..count {
            let name = format!("directory-{index:02}");
            let old = destination.join(&name);
            std::fs::create_dir(&old).unwrap();
            for leaf in 0..16 {
                std::fs::write(old.join(format!("old-{leaf:02}")), "retained").unwrap();
            }
            let new = source.join(&name);
            std::fs::create_dir(&new).unwrap();
            std::fs::write(new.join("new"), "visible source").unwrap();
            sources.push(new);
        }
        let phase = std::time::Instant::now();
        let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &sources,
            &destination,
            ConflictAction::Overwrite,
        );
        assert!(batch.error.is_none(), "{:?}", batch.error);
        eprintln!(
            "Cleanup phase: records={count} phase=copy elapsed_ms={}",
            phase.elapsed().as_millis()
        );
        let selected: Vec<_> = batch
            .undo_receipts
            .iter()
            .map(|receipt| {
                stage(receipt)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .strip_prefix(".ottrin-txn-")
                    .unwrap()
                    .to_string()
            })
            .collect();
        let phase = std::time::Instant::now();
        let observed = crate::observe_retained_copy_cleanup(&selected).unwrap();
        eprintln!(
            "Cleanup phase: records={count} phase=observe elapsed_ms={}",
            phase.elapsed().as_millis()
        );
        let phase = std::time::Instant::now();
        let mut plan = plan_cleanup(&observed).unwrap();
        eprintln!(
            "Cleanup phase: records={count} phase=plan elapsed_ms={}",
            phase.elapsed().as_millis()
        );
        let phase = std::time::Instant::now();
        while plan.reclaim_payload_next().unwrap() {}
        eprintln!(
            "Cleanup phase: records={count} phase=reclaim elapsed_ms={}",
            phase.elapsed().as_millis()
        );
        let phase = std::time::Instant::now();
        let finalization = plan.begin_finalization().unwrap();
        eprintln!(
            "Cleanup phase: records={count} phase=manifest elapsed_ms={}",
            phase.elapsed().as_millis()
        );
        let metadata_size = std::fs::read_dir(root.path().join("copies"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                entry.metadata().unwrap().len()
            })
            .sum::<u64>();
        let counters = || {
            let bytes = std::fs::read_to_string("/proc/thread-self/io").unwrap();
            let value = |key: &str| {
                bytes
                    .lines()
                    .find_map(|line| line.strip_prefix(key).map(|v| v.parse::<u64>().unwrap()))
                    .unwrap()
            };
            (value("rchar: "), value("syscr: "))
        };
        let monitored = {
            let central =
                LinuxJournalDirectory::open_copy_registry(&finalization.root, false).unwrap();
            let registry = central.duplicate_copy_directory().unwrap();
            let mut reads = MetadataReads::default();
            NamespaceGuard::new(
                &mut reads,
                &finalization.root,
                &registry,
                &filename(finalization.id()),
                &finalization.manifest.entries,
            )
            .is_ok()
        };
        let cpu_ns = || {
            let mut stamp = std::mem::MaybeUninit::<libc::timespec>::uninit();
            assert_eq!(
                unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, stamp.as_mut_ptr()) },
                0
            );
            let stamp = unsafe { stamp.assume_init() };
            stamp.tv_sec as u64 * 1_000_000_000 + stamp.tv_nsec as u64
        };
        let before = counters();
        let cpu_before = cpu_ns();
        let start = std::time::Instant::now();
        finalization.finish().unwrap();
        let after = counters();
        let reads = after.0 - before.0;
        let read_calls = after.1 - before.1;
        eprintln!(
            "Finalization: records={count} metadata_bytes={metadata_size} read_bytes={reads} elapsed_ms={} cpu_us={} read_calls={read_calls} monitored={monitored}",
            start.elapsed().as_millis(),
            (cpu_ns() - cpu_before) / 1000
        );
        assert!(
            reads < metadata_size * 20 + 1024 * 1024,
            "Finalization repeatedly read whole-group metadata: {reads} bytes for {metadata_size} bytes of original metadata"
        );
        if monitored {
            assert!(
                read_calls < count as u64 * 128,
                "Finalization repeated whole-group reads: {read_calls} for {count} records"
            );
        }
        assert!(statuses(root.path()).is_empty());
        for path in sources {
            assert_eq!(
                std::fs::read(destination.join(path.file_name().unwrap()).join("new")).unwrap(),
                b"visible source"
            );
            assert_eq!(std::fs::read(path.join("new")).unwrap(), b"visible source");
        }
    }

    fn require_monitor(finalization: &RetainedCopyFinalization) {
        let central = LinuxJournalDirectory::open_copy_registry(&finalization.root, false).unwrap();
        let registry = central.duplicate_copy_directory().unwrap();
        let mut reads = MetadataReads::default();
        NamespaceGuard::new(
            &mut reads,
            &finalization.root,
            &registry,
            &filename(finalization.id()),
            &finalization.manifest.entries,
        )
        .unwrap_or_else(|error| panic!("monitored fixture unavailable: {error}"));
    }

    #[test]
    fn monitored_finalization_refuses_other_stage_and_registry_changes_before_unlink() {
        for attack in [
            "other-child",
            "registry-add",
            "other-replace",
            "outside-alias",
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (finalization, _) = completed_group(root.path());
            let entries: Vec<_> = finalization.manifest.entries.iter().collect();
            let current = entries[0].1.record.transaction.decode().unwrap();
            let other = entries[1].1.record.transaction.decode().unwrap();
            let canonical = root
                .path()
                .join("copies")
                .join(format!("{}.journal", entries[1].0));
            let alias = root.path().join("outside-alias");
            let bytes = std::fs::read(&canonical).unwrap();
            if attack == "outside-alias" {
                std::fs::hard_link(&canonical, &alias).unwrap();
            }
            require_monitor(&finalization);
            let before = std::fs::read(current.join(LEDGER)).unwrap();
            let mut attacked = false;
            assert!(
                finalization
                    .finish_with_hook(&mut |point| {
                        if !attacked && point == FinalizationPoint::BeforeMetadataUnlink {
                            attacked = true;
                            match attack {
                                "other-child" => {
                                    std::fs::write(other.join("user-data"), "preserve").unwrap()
                                }
                                "registry-add" => std::fs::write(
                                    root.path().join("copies/foreign.journal"),
                                    "preserve",
                                )
                                .unwrap(),
                                "other-replace" => {
                                    std::fs::rename(&canonical, &alias).unwrap();
                                    std::fs::write(&canonical, &bytes).unwrap();
                                }
                                "outside-alias" => std::fs::write(&alias, &bytes).unwrap(),
                                _ => unreachable!(),
                            }
                        }
                        Ok(())
                    })
                    .is_err(),
                "{attack}"
            );
            assert!(attacked, "{attack}");
            assert_eq!(
                std::fs::read(current.join(LEDGER)).unwrap(),
                before,
                "{attack}"
            );
            assert!(canonical.exists());
            if attack == "other-child" {
                assert_eq!(std::fs::read(other.join("user-data")).unwrap(), b"preserve");
            }
        }
    }

    #[test]
    fn monitored_finalization_refuses_an_unselected_dependency_rewritten_through_an_alias() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (finalization, _) = completed_group(root.path());
        let source = root.path().join("unrelated-source");
        let destination = root.path().join("unrelated-destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("item"), "unrelated").unwrap();
        let unrelated = DefaultPlatform.execute_copy_batch_with_undo_receipts(
            &[source.join("item")],
            &destination,
            ConflictAction::Overwrite,
        );
        assert!(unrelated.error.is_none());
        let id = statuses(root.path())
            .into_iter()
            .map(|s| s.transaction_id)
            .find(|id| !finalization.manifest.entries.contains_key(id))
            .unwrap();
        let canonical = root.path().join("copies").join(format!("{id}.journal"));
        let alias = root.path().join("dependency-alias");
        std::fs::hard_link(&canonical, &alias).unwrap();
        let entry = finalization.manifest.entries.values().next().unwrap();
        let current = entry.record.transaction.decode().unwrap();
        let before = std::fs::read(current.join(LEDGER)).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&canonical).unwrap()).unwrap();
        value["parent_handle"] = serde_json::to_value(&entry.record.transaction_handle).unwrap();
        let changed = serde_json::to_vec(&value).unwrap();
        require_monitor(&finalization);
        let mut attacked = false;
        assert!(
            finalization
                .finish_with_hook(&mut |point| {
                    if !attacked && point == FinalizationPoint::BeforeMetadataUnlink {
                        attacked = true;
                        std::fs::write(&alias, &changed).unwrap();
                    }
                    Ok(())
                })
                .is_err()
        );
        assert!(attacked);
        assert_eq!(std::fs::read(current.join(LEDGER)).unwrap(), before);
        assert_eq!(std::fs::read(canonical).unwrap(), changed);
        assert_eq!(std::fs::read(source.join("item")).unwrap(), b"unrelated");
        assert_eq!(
            std::fs::read(destination.join("item")).unwrap(),
            b"unrelated"
        );
    }

    #[test]
    fn monitored_finalization_refuses_another_stage_reappearing_before_canonical_retirement() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (finalization, _) = completed_group(root.path());
        require_monitor(&finalization);
        let id = finalization.transaction_ids().last().unwrap().to_string();
        let reused = finalization.manifest.entries[&id]
            .record
            .transaction
            .decode()
            .unwrap();
        let mut attacked = false;
        assert!(
            finalization
                .finish_with_hook(&mut |point| {
                    if !attacked && point == FinalizationPoint::BeforeCanonicalUnlink {
                        attacked = true;
                        assert!(!reused.exists());
                        std::fs::create_dir(&reused).unwrap();
                        std::fs::write(reused.join("user-data"), "preserve").unwrap();
                    }
                    Ok(())
                })
                .is_err()
        );
        assert!(attacked);
        for id in finalization.transaction_ids() {
            assert!(
                root.path()
                    .join("copies")
                    .join(format!("{id}.journal"))
                    .exists()
            );
        }
        assert_eq!(
            std::fs::read(reused.join("user-data")).unwrap(),
            b"preserve"
        );
        assert!(resume_finalization(finalization.id()).is_err());
    }

    fn separate_group(root: &Path) -> RetainedCopyFinalization {
        for branch in ["left", "right"] {
            let source = root.join(branch).join("source");
            let destination = root.join(branch).join("destination");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::create_dir_all(&destination).unwrap();
            std::fs::write(source.join("item"), "new").unwrap();
            std::fs::write(destination.join("item"), "old").unwrap();
            let batch = DefaultPlatform.execute_copy_batch_with_undo_receipts(
                &[source.join("item")],
                &destination,
                ConflictAction::Overwrite,
            );
            assert!(batch.error.is_none());
        }
        let mut plan = plan_cleanup(&statuses(root)).unwrap();
        while plan.reclaim_payload_next().unwrap() {}
        plan.begin_finalization().unwrap()
    }

    #[test]
    fn monitored_finalization_refuses_another_parents_ancestor_rebinding() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let finalization = separate_group(root.path());
        require_monitor(&finalization);
        let entries: Vec<_> = finalization.manifest.entries.values().collect();
        let current = entries[0].record.transaction.decode().unwrap();
        let other = entries[1].record.transaction.decode().unwrap();
        let ancestor = other.parent().unwrap().parent().unwrap();
        let before = std::fs::read(current.join(LEDGER)).unwrap();
        let mut attacked = false;
        assert!(
            finalization
                .finish_with_hook(&mut |point| {
                    if !attacked && point == FinalizationPoint::BeforeMetadataUnlink {
                        attacked = true;
                        std::fs::rename(ancestor, root.path().join("moved-ancestor")).unwrap();
                        std::fs::create_dir_all(other.parent().unwrap()).unwrap();
                    }
                    Ok(())
                })
                .is_err()
        );
        assert!(attacked);
        assert_eq!(std::fs::read(current.join(LEDGER)).unwrap(), before);
        assert_eq!(
            std::fs::read(root.path().join("moved-ancestor/destination/item")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn symlink_ancestor_uses_full_validation_and_still_finishes_safely() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let finalization = separate_group(root.path());
        std::fs::rename(root.path().join("left"), root.path().join("real-left")).unwrap();
        std::os::unix::fs::symlink(root.path().join("real-left"), root.path().join("left"))
            .unwrap();
        let central = LinuxJournalDirectory::open_copy_registry(&finalization.root, false).unwrap();
        let registry = central.duplicate_copy_directory().unwrap();
        let mut reads = MetadataReads::default();
        assert!(
            NamespaceGuard::new(
                &mut reads,
                &finalization.root,
                &registry,
                &filename(finalization.id()),
                &finalization.manifest.entries
            )
            .is_err()
        );
        drop(reads);
        finalization.finish().unwrap();
        assert!(statuses(root.path()).is_empty());
        for branch in ["left", "right"] {
            assert_eq!(
                std::fs::read(root.path().join(branch).join("source/item")).unwrap(),
                b"new"
            );
            assert_eq!(
                std::fs::read(root.path().join(branch).join("destination/item")).unwrap(),
                b"new"
            );
        }
    }

    #[test]
    fn mount_guard_private_child() {
        let Some(root) = std::env::var_os("OTTRIN_FINALIZATION_MOUNT_ROOT") else {
            return;
        };
        let parent: u32 = std::env::var("OTTRIN_FINALIZATION_MOUNT_PARENT")
            .unwrap()
            .parse()
            .unwrap();
        // This dedicated child cannot outlive its test parent on cancellation.
        assert_eq!(
            unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) },
            0
        );
        assert_eq!(unsafe { libc::getppid() } as u32, parent);
        let root = PathBuf::from(root);
        assert_ne!(
            std::fs::read_link("/proc/self/ns/mnt")
                .unwrap()
                .to_string_lossy(),
            std::env::var("OTTRIN_FINALIZATION_MOUNT_NAMESPACE").unwrap()
        );
        if std::env::var("OTTRIN_FINALIZATION_MOUNT_MODE").as_deref() == Ok("payload") {
            super::super::tests::payload_dependency_change_fixture(&root, true);
            return;
        }
        let _registry = Registry::new(&root);
        let (finalization, _) = completed_group(&root);
        require_monitor(&finalization);
        let entries: Vec<_> = finalization.manifest.entries.iter().collect();
        let current = entries[0].1.record.transaction.decode().unwrap();
        let canonical = root
            .join("copies")
            .join(format!("{}.journal", entries[1].0));
        let before = std::fs::read(current.join(LEDGER)).unwrap();
        let path = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(
            canonical.as_os_str(),
        ))
        .unwrap();
        let mut mounted = false;
        let result = finalization.finish_with_hook(&mut |point| {
            if !mounted && point == FinalizationPoint::BeforeMetadataUnlink {
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
                mounted = true;
            }
            Ok(())
        });
        assert!(mounted);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("mount namespace changed")
        );
        assert_eq!(std::fs::read(current.join(LEDGER)).unwrap(), before);
        assert_eq!(unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) }, 0);
    }

    #[test]
    #[ignore = "requires unprivileged private mount namespaces"]
    fn mount_rebinding_invalidates_finalization_before_any_metadata_is_removed() {
        for mode in ["finalization", "payload"] {
            let root = tempfile::tempdir().unwrap();
            struct OwnedChild(Option<std::process::Child>);
            impl Drop for OwnedChild {
                fn drop(&mut self) {
                    if let Some(child) = &mut self.0 {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
            let mut child = OwnedChild(Some(
                std::process::Command::new("unshare")
                    .args([
                        "--user",
                        "--map-root-user",
                        "--mount",
                        "--propagation",
                        "private",
                    ])
                    .arg(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "copy_recovery::cleanup::finalization::tests::mount_guard_private_child",
                        "--nocapture",
                    ])
                    .env("OTTRIN_FINALIZATION_MOUNT_ROOT", root.path())
                    .env("OTTRIN_FINALIZATION_MOUNT_MODE", mode)
                    .env(
                        "OTTRIN_FINALIZATION_MOUNT_NAMESPACE",
                        std::fs::read_link("/proc/self/ns/mnt").unwrap(),
                    )
                    .env(
                        "OTTRIN_FINALIZATION_MOUNT_PARENT",
                        std::process::id().to_string(),
                    )
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            ));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                if child.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "private mount child timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let output = child.0.take().unwrap().wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn group_finalization_removes_all_private_metadata_and_preserves_visible_data() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (finalization, stages) = completed_group(root.path());
        assert_eq!(finalization.transaction_ids().len(), 2);
        let resumed = discover_finalizations().unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].id(), finalization.id());
        finalization.finish().unwrap();
        assert_finished(root.path(), &stages);
        // A live retry can confirm absence after the final unlink/sync boundary.
        finalization.finish().unwrap();
    }

    #[test]
    fn finalization_reopens_after_every_metadata_retirement_boundary() {
        for point in [
            FinalizationPoint::BeforeMetadataUnlink,
            FinalizationPoint::BeforeStageRemove,
            FinalizationPoint::BeforeCanonicalUnlink,
            FinalizationPoint::BeforeManifestRemove,
            FinalizationPoint::LedgerRemoved,
            FinalizationPoint::LocalJournalRemoved,
            FinalizationPoint::StageRemoved,
            FinalizationPoint::CanonicalJournalRemoved,
            FinalizationPoint::ManifestRemoved,
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, stages) = completed_group(root.path());
            let mut stopped = false;
            assert!(
                original
                    .finish_with_hook(&mut |observed| {
                        if !stopped && observed == point {
                            stopped = true;
                            return Err(refusal("owned interruption"));
                        }
                        Ok(())
                    })
                    .is_err()
            );
            assert!(stopped);
            if point != FinalizationPoint::ManifestRemoved {
                let resumed = resume_finalization(original.id()).unwrap();
                resumed.finish().unwrap();
            } else {
                original.finish().unwrap();
            }
            assert_finished(root.path(), &stages);
        }
    }

    #[test]
    fn finalization_sigkill_child() {
        let Some(root) = std::env::var_os("OTTRIN_FINALIZE_KILL_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let _registry = Registry::new(&root);
        let id = std::env::var("OTTRIN_FINALIZE_KILL_ID").unwrap();
        let point = std::env::var("OTTRIN_FINALIZE_KILL_POINT").unwrap();
        resume_finalization(&id)
            .unwrap()
            .finish_with_hook(&mut |observed| {
                if format!("{observed:?}") == point {
                    // Dedicated test subprocess only; no destructor recovery.
                    unsafe {
                        libc::kill(libc::getpid(), libc::SIGKILL);
                    }
                    unreachable!("SIGKILL returned");
                }
                Ok(())
            })
            .unwrap();
        panic!("Child never reached requested finalization boundary");
    }

    #[test]
    fn group_finalization_resumes_after_sigkill_with_a_canonical_member_already_gone() {
        use std::os::unix::process::ExitStatusExt;
        for point in [
            FinalizationPoint::BeforeMetadataUnlink,
            FinalizationPoint::BeforeStageRemove,
            FinalizationPoint::BeforeCanonicalUnlink,
            FinalizationPoint::BeforeManifestRemove,
            FinalizationPoint::LedgerRemoved,
            FinalizationPoint::LocalJournalRemoved,
            FinalizationPoint::StageRemoved,
            FinalizationPoint::CanonicalJournalRemoved,
            FinalizationPoint::ManifestRemoved,
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, stages) = completed_group(root.path());
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "copy_recovery::cleanup::finalization::tests::finalization_sigkill_child",
                ])
                .env("OTTRIN_FINALIZE_KILL_ROOT", root.path())
                .env("OTTRIN_FINALIZE_KILL_ID", original.id())
                .env("OTTRIN_FINALIZE_KILL_POINT", format!("{point:?}"))
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert_eq!(status.signal(), Some(libc::SIGKILL), "{point:?}");
            if point != FinalizationPoint::ManifestRemoved {
                let discovered = discover_finalizations().unwrap();
                assert_eq!(discovered.len(), 1);
                discovered[0].finish().unwrap();
            } else {
                original.finish().unwrap();
            }
            assert_finished(root.path(), &stages);
        }
    }

    #[test]
    fn changed_or_replaced_metadata_and_unknown_children_are_never_retired() {
        for attack in [
            "ledger",
            "local",
            "canonical",
            "replacement",
            "unknown",
            "manifest",
        ] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, stages) = completed_group(root.path());
            let first_id = original.transaction_ids()[0].to_owned();
            let first_stage = original.manifest.entries[&first_id]
                .record
                .transaction
                .decode()
                .unwrap();
            let path = match attack {
                "ledger" => first_stage.join(LEDGER),
                "local" | "replacement" => first_stage.join("copy-journal"),
                "canonical" => root
                    .path()
                    .join("copies")
                    .join(format!("{first_id}.journal")),
                "unknown" => first_stage.join("user-file"),
                "manifest" => root.path().join("copies").join(filename(original.id())),
                _ => unreachable!(),
            };
            if attack == "replacement" {
                let bytes = std::fs::read(&path).unwrap();
                std::fs::rename(&path, root.path().join("original-local-journal")).unwrap();
                std::fs::write(&path, bytes).unwrap();
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            } else {
                std::fs::write(&path, "user data changed after finalization began").unwrap();
            }
            let bytes = std::fs::read(&path).unwrap();
            assert!(original.finish().is_err(), "{attack}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes, "{attack}");
            for stage in stages {
                assert!(stage.exists());
            }
        }
    }

    #[test]
    fn interrupted_finalization_refuses_reused_stage_and_parent_names() {
        for replace_parent in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, stages) = completed_group(root.path());
            let victim = if replace_parent {
                root.path().join("destination")
            } else {
                stages[0].clone()
            };
            std::fs::rename(&victim, root.path().join("moved-original")).unwrap();
            std::fs::create_dir(&victim).unwrap();
            std::fs::write(victim.join("user-file"), "keep replacement").unwrap();
            assert!(original.finish().is_err());
            assert!(resume_finalization(original.id()).is_err());
            assert_eq!(
                std::fs::read(victim.join("user-file")).unwrap(),
                b"keep replacement"
            );
        }
    }

    #[test]
    fn final_checks_preserve_metadata_modified_after_it_was_pinned() {
        for attack in ["write", "replace", "canonical-write", "manifest-write"] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, _) = completed_group(root.path());
            let first_id = original.transaction_ids()[0].to_owned();
            let first_stage = original.manifest.entries[&first_id]
                .record
                .transaction
                .decode()
                .unwrap();
            let (path, point) = match attack {
                "write" | "replace" => (
                    first_stage.join(LEDGER),
                    FinalizationPoint::BeforeMetadataUnlink,
                ),
                "canonical-write" => (
                    root.path()
                        .join("copies")
                        .join(format!("{first_id}.journal")),
                    FinalizationPoint::BeforeCanonicalUnlink,
                ),
                "manifest-write" => (
                    root.path().join("copies").join(filename(original.id())),
                    FinalizationPoint::BeforeManifestRemove,
                ),
                _ => unreachable!(),
            };
            let mut attacked = false;
            assert!(
                original
                    .finish_with_hook(&mut |observed| {
                        if !attacked && observed == point {
                            attacked = true;
                            if attack == "replace" {
                                let bytes = std::fs::read(&path).unwrap();
                                std::fs::rename(&path, root.path().join("moved-pinned-file"))
                                    .unwrap();
                                std::fs::write(&path, bytes).unwrap();
                            } else {
                                std::fs::write(&path, "keep user changes after pinning").unwrap();
                            }
                        }
                        Ok(())
                    })
                    .is_err(),
                "{attack}"
            );
            assert!(attacked);
            assert!(path.exists(), "{attack}");
            if attack != "replace" {
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    b"keep user changes after pinning"
                );
            }
        }
    }

    #[test]
    fn final_checks_refuse_replaced_empty_stage_and_late_unknown_children() {
        for replace in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, _) = completed_group(root.path());
            let id = original.transaction_ids()[0];
            let victim = original.manifest.entries[id]
                .record
                .transaction
                .decode()
                .unwrap();
            let mut attacked = false;
            assert!(
                original
                    .finish_with_hook(&mut |point| {
                        if point == FinalizationPoint::BeforeStageRemove && !attacked {
                            attacked = true;
                            if replace {
                                std::fs::rename(&victim, root.path().join("original-empty-stage"))
                                    .unwrap();
                                std::fs::create_dir(&victim).unwrap();
                            } else {
                                std::fs::write(victim.join("late-user-file"), "preserve").unwrap();
                            }
                        }
                        Ok(())
                    })
                    .is_err()
            );
            assert!(attacked);
            assert!(victim.exists());
            assert!(
                root.path()
                    .join("copies")
                    .join(format!("{id}.journal"))
                    .exists()
            );
            if !replace {
                assert_eq!(
                    std::fs::read(victim.join("late-user-file")).unwrap(),
                    b"preserve"
                );
            }
        }
    }

    #[test]
    fn missing_canonical_or_manifest_cannot_authorize_deleting_surviving_stages() {
        for remove_manifest in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let _registry = Registry::new(root.path());
            let (original, stages) = completed_group(root.path());
            let path = if remove_manifest {
                root.path().join("copies").join(filename(original.id()))
            } else {
                root.path()
                    .join("copies")
                    .join(format!("{}.journal", original.transaction_ids()[0]))
            };
            std::fs::remove_file(path).unwrap();
            assert!(original.finish().is_err());
            for stage in stages {
                assert!(stage.join(LEDGER).exists());
                assert!(stage.join("copy-journal").exists());
            }
        }
    }

    #[test]
    fn finalization_lock_excludes_other_workers_and_unlocks_with_an_inherited_descriptor() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (original, stages) = completed_group(root.path());
        let central = LinuxJournalDirectory::open_copy_registry(&original.root, false).unwrap();
        let registry = central.duplicate_copy_directory().unwrap();
        let (lock, _, _) = load(&registry, original.id()).unwrap();
        assert!(resume_finalization(original.id()).is_err());
        // A duplicate shares the open-file description, just as the transient
        // descriptor inherited by another thread's fork does until exec.
        let inherited = rustix::io::fcntl_dupfd_cloexec(&lock.file, 0).unwrap();
        drop(lock);
        let resumed = resume_finalization(original.id()).unwrap();
        resumed.finish().unwrap();
        drop(inherited);
        assert_finished(root.path(), &stages);
    }

    #[test]
    fn a_transaction_cannot_belong_to_overlapping_finalizations() {
        use super::super::tests::copy;
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let (original, stages) = completed_group(root.path());
        let third = copy(root.path(), "third");
        let mut combined = plan_cleanup(&statuses(root.path())).unwrap();
        while combined.reclaim_payload_next().unwrap() {}
        assert!(combined.begin_finalization().is_err());
        let selected = statuses(root.path())
            .into_iter()
            .filter(|entry| entry.transaction.as_ref() == Some(&stage(&third)))
            .collect::<Vec<_>>();
        let mut independent = plan_cleanup(&selected).unwrap();
        while independent.reclaim_payload_next().unwrap() {}
        let independent = independent.begin_finalization().unwrap();
        assert_eq!(discover_finalizations().unwrap().len(), 2);
        original.finish().unwrap();
        independent.finish().unwrap();
        assert_finished(root.path(), &stages);
        assert!(!stage(&third).exists());
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"third"
        );
    }

    #[test]
    fn recursive_payload_and_finalization_preserve_symlink_targets_and_visible_edits() {
        let root = tempfile::tempdir().unwrap();
        let _registry = Registry::new(root.path());
        let receipt = directory_backup(root.path());
        std::fs::write(root.path().join("destination/item"), "later user edit").unwrap();
        let mut plan = plan_cleanup(&statuses(root.path())).unwrap();
        while plan.reclaim_payload_next().unwrap() {}
        plan.begin_finalization().unwrap().finish().unwrap();
        assert!(!stage(&receipt).exists());
        assert!(statuses(root.path()).is_empty());
        assert_eq!(
            std::fs::read(root.path().join("outside")).unwrap(),
            b"outside user data"
        );
        assert_eq!(
            std::fs::read(root.path().join("destination/item")).unwrap(),
            b"later user edit"
        );
    }
}
