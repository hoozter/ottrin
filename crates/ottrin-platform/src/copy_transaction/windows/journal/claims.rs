//! Destination-local, append-only name ownership. A released generation remains
//! verifiable while its Copy journals retain Undo/cleanup evidence.
use super::super::registry::AnchoredRoot;
use super::*;
use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_WRITE};

const CLAIM_MAGIC: &[u8; 8] = b"OTCWCL02";

#[cfg(not(test))]
fn claim_budget() -> u64 {
    MAX_LOG
}
#[cfg(test)]
thread_local! {static CLAIM_BUDGET: std::cell::Cell<u64> = const { std::cell::Cell::new(MAX_LOG) };}
#[cfg(test)]
fn claim_budget() -> u64 {
    CLAIM_BUDGET.with(|limit| limit.get())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    parent: WindowsStableIdentity,
    root: WindowsStableIdentity,
    anchor: WindowsStableIdentity,
    bucket: WindowsStableIdentity,
    bucket_anchor: WindowsStableIdentity,
    file: WindowsStableIdentity,
    central: WindowsStableIdentity,
    target: Vec<u16>,
    transaction: String,
    creator: Vec<u8>,
    security: security::PrivateSecurity,
}
impl Claim {
    fn valid(&self) -> bool {
        self.parent.is_directory_without_reparse()
            && self.root.is_directory_without_reparse()
            && self.central.is_directory_without_reparse()
            && self.file.is_regular_file_without_reparse()
            && self.anchor.is_regular_file_without_reparse()
            && self.bucket.is_directory_without_reparse()
            && self.bucket_anchor.is_regular_file_without_reparse()
            && self.bucket.volume_serial_number() == self.parent.volume_serial_number()
            && self.bucket_anchor.volume_serial_number() == self.parent.volume_serial_number()
            && !self.bucket.same_object(self.root)
            && !self.bucket_anchor.same_object(self.file)
            && self.anchor.volume_serial_number() == self.parent.volume_serial_number()
            && !self.anchor.same_object(self.file)
            && !self.parent.same_object(self.root)
            && self.parent.volume_serial_number() == self.root.volume_serial_number()
            && self.root.volume_serial_number() == self.file.volume_serial_number()
            && self.security.valid_for(&self.creator)
            && valid_leaf(&self.target)
            && self.transaction.len() == 32
            && self
                .transaction
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    claim: Claim,
    sequence: u64,
    digest: [u8; 32],
}
impl Binding {
    #[cfg(test)]
    pub(super) fn test_relative_path(&self) -> std::path::PathBuf {
        let (short, key) = ledger_key(&OsString::from_wide(&self.claim.target));
        std::path::PathBuf::from(".ottrin-copy-target-claims")
            .join(if short {
                "short-names.ottrin-claims"
            } else {
                "long-names.ottrin-claims"
            })
            .join(key)
    }

    #[cfg(test)]
    pub(super) fn assert_only_release(&self, before: &[u8], after: &[u8]) {
        let prior = Log::decode(before).unwrap();
        let next = Log::decode(after).unwrap();
        if before == after {
            assert_eq!(
                prior.generation(self).unwrap().released,
                next.generation(self).unwrap().released
            );
        } else {
            assert!(!prior.generation(self).unwrap().released);
            assert!(next.generation(self).unwrap().released);
            assert!(after.starts_with(before));
            assert_eq!(next.sequence, prior.sequence + 1);
            assert_eq!(next.generations.len(), prior.generations.len());
        }
    }
    pub(super) fn identities(&self) -> [WindowsStableIdentity; 5] {
        [
            self.claim.root,
            self.claim.anchor,
            self.claim.bucket,
            self.claim.bucket_anchor,
            self.claim.file,
        ]
    }
    pub(super) fn matches(
        &self,
        parent: WindowsStableIdentity,
        central: WindowsStableIdentity,
        target: &[u16],
        transaction: &str,
        creator: &[u8],
    ) -> bool {
        self.claim.valid()
            && self.sequence < MAX_LOG / FRAME_HEADER as u64
            && self.claim.parent == parent
            && self.claim.central == central
            && self.claim.target == target
            && self.claim.transaction == transaction
            && self.claim.creator == creator
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
enum Entry {
    Claim { claim: Box<Claim> },
    Release { sequence: u64, digest: [u8; 32] },
}
fn encode_frame(
    sequence: u64,
    previous: &[u8; 32],
    entry: &Entry,
) -> Result<Vec<u8>, PlatformError> {
    let payload = serde_json::to_vec(entry).map_err(io)?;
    if payload.len() > MAX_FRAME {
        return Err(io("Copy name-claim frame exceeds its bound"));
    }
    let hash = frame_hash(sequence, previous, &payload);
    let mut frame = Vec::with_capacity(FRAME_HEADER + payload.len());
    frame.extend_from_slice(CLAIM_MAGIC);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&sequence.to_le_bytes());
    frame.extend_from_slice(previous);
    frame.extend_from_slice(&hash);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

struct Generation {
    binding: Binding,
    released: bool,
}
struct Log {
    generations: Vec<Generation>,
    sequence: u64,
    previous: [u8; 32],
    length: u64,
}
impl Log {
    fn empty() -> Self {
        Self {
            generations: Vec::new(),
            sequence: 0,
            previous: [0; 32],
            length: 0,
        }
    }
    fn check(&self, entry: &Entry) -> Result<(), PlatformError> {
        match entry {
            Entry::Claim { claim } => {
                if !claim.valid()
                    || self.generations.last().is_some_and(|g| !g.released)
                    || self.generations.first().is_some_and(|g| {
                        let first = &g.binding.claim;
                        first.parent != claim.parent
                            || first.root != claim.root
                            || first.file != claim.file
                            || first.anchor != claim.anchor
                            || first.bucket != claim.bucket
                            || first.bucket_anchor != claim.bucket_anchor
                            || first.creator != claim.creator
                            || first.security != claim.security
                    })
                {
                    return Err(io("Invalid Copy name-claim generation"));
                }
            }
            Entry::Release { sequence, digest } => {
                let generation = self
                    .generations
                    .last()
                    .ok_or_else(|| io("Copy claim release has no owner"))?;
                if generation.released
                    || generation.binding.sequence != *sequence
                    || generation.binding.digest != *digest
                {
                    return Err(io(
                        "Copy claim release does not match its active generation",
                    ));
                }
            }
        }
        Ok(())
    }
    fn apply(&mut self, entry: Entry, hash: [u8; 32]) -> Result<(), PlatformError> {
        self.check(&entry)?;
        match entry {
            Entry::Claim { claim } => self.generations.push(Generation {
                binding: Binding {
                    claim: *claim,
                    sequence: self.sequence,
                    digest: hash,
                },
                released: false,
            }),
            Entry::Release { .. } => self.generations.last_mut().unwrap().released = true,
        }
        self.sequence += 1;
        self.previous = hash;
        Ok(())
    }
    fn decode(bytes: &[u8]) -> Result<Self, PlatformError> {
        let result = Self::prefix(bytes)?;
        if result.length != bytes.len() as u64 {
            return Err(io("Torn Copy name-claim frame; data retained"));
        }
        Ok(result)
    }
    fn prefix(bytes: &[u8]) -> Result<Self, PlatformError> {
        let mut result = Self::empty();
        let mut at = 0usize;
        while at < bytes.len() {
            let Some(header) = bytes.get(at..at + FRAME_HEADER) else {
                break;
            };
            let size = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
            if &header[..8] != CLAIM_MAGIC
                || size == 0
                || size > MAX_FRAME
                || u64::from_le_bytes(header[12..20].try_into().unwrap()) != result.sequence
                || header[20..52] != result.previous
            {
                return Err(io("Invalid Copy name-claim frame"));
            }
            let end = at
                .checked_add(FRAME_HEADER + size)
                .ok_or_else(|| io("Copy claim size overflow"))?;
            let Some(payload) = bytes.get(at + FRAME_HEADER..end) else {
                break;
            };
            let hash = frame_hash(result.sequence, &result.previous, payload);
            if header[52..84] != hash {
                return Err(io("Copy name-claim digest changed"));
            }
            result.apply(serde_json::from_slice(payload).map_err(io)?, hash)?;
            at = end;
        }
        if result.generations.is_empty() {
            return Err(io("Empty Copy name-claim log; retained for review"));
        }
        result.length = at as u64;
        Ok(result)
    }
    fn for_binding(
        bytes: &[u8],
        binding: &Binding,
        terminal: bool,
    ) -> Result<(Self, bool), PlatformError> {
        let log = Self::prefix(bytes)?;
        if log.length == bytes.len() as u64 {
            return Ok((log, false));
        }
        let own_active = log
            .generations
            .last()
            .is_some_and(|generation| !generation.released && generation.binding == *binding);
        let expected = encode_frame(
            log.sequence,
            &log.previous,
            &Entry::Release {
                sequence: binding.sequence,
                digest: binding.digest,
            },
        )?;
        let tail = &bytes[log.length as usize..];
        if !terminal || !own_active || tail.len() >= expected.len() || !expected.starts_with(tail) {
            return Err(io(
                "Unrecognized or unauthorized Copy claim tail; data retained",
            ));
        }
        Ok((log, true))
    }
    fn append(&mut self, file: &mut File, entry: Entry) -> Result<Binding, PlatformError> {
        self.check(&entry)?;
        let frame = encode_frame(self.sequence, &self.previous, &entry)?;
        let hash: [u8; 32] = frame[52..84].try_into().unwrap();
        // Admission reserves its exact release before issuing the new owner.
        let release_bytes = if matches!(entry, Entry::Claim { .. }) {
            encode_frame(
                self.sequence + 1,
                &hash,
                &Entry::Release {
                    sequence: self.sequence,
                    digest: hash,
                },
            )?
            .len() as u64
        } else {
            0
        };
        if self.length + frame.len() as u64 + release_bytes > claim_budget() {
            return Err(io(
                "Copy name-claim log lacks space for claim and release; retained for review",
            ));
        }
        // Never repair or retry an uncertain append in this owner.
        file.seek(SeekFrom::End(0)).map_err(io)?;
        file.write_all(&frame).map_err(io)?;
        file.sync_all().map_err(io)?;
        self.apply(entry, hash)?;
        self.length += frame.len() as u64;
        Ok(self.generations.last().unwrap().binding.clone())
    }
    fn generation(&self, expected: &Binding) -> Result<&Generation, PlatformError> {
        self.generations
            .iter()
            .find(|g| g.binding.sequence == expected.sequence && g.binding == *expected)
            .ok_or_else(|| io("Copy name-claim generation changed"))
    }
}

pub(super) struct Lease {
    root: AnchoredRoot,
    bucket: AnchoredRoot,
    file: File,
    binding: Binding,
    stamp: EntryVersion,
    digest: [u8; 32],
    released: bool,
    pending_release: bool,
    writer: bool,
}
fn open_file(root: &File, name: &OsStr, access: u32) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            root,
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
}
// A lookup of at least 13 UTF-16 units cannot resolve an NTFS 8.3 alias.
// Separate buckets prevent an encoded short key from colliding with a literal
// long filename. Case mapping remains the destination filesystem's operation.
fn ledger_key(target: &OsStr) -> (bool, OsString) {
    let short = target.encode_wide().count() <= 12;
    let mut name = target.to_os_string();
    if short {
        name.push(".ottrin-claim");
    }
    (short, name)
}
fn validate_control(
    root: &AnchoredRoot,
    bucket: &AnchoredRoot,
    file: &File,
    bytes: &[u8],
    log: &Log,
) -> Result<EntryVersion, PlatformError> {
    root.validate()?;
    bucket.validate()?;
    let first = &log
        .generations
        .first()
        .ok_or_else(|| io("Copy claim log has no generation"))?
        .binding
        .claim;
    if first.parent != root.parent_identity()
        || first.root != stable_identity_from_handle(root.root()).map_err(io)?
        || first.anchor != root.anchor_identity()
        || first.bucket != stable_identity_from_handle(bucket.root()).map_err(io)?
        || first.bucket_anchor != bucket.anchor_identity()
        || bucket.parent_identity() != first.root
        || first.creator != security::current_owner()?
    {
        return Err(io("Copy name-claim control belongs to another namespace"));
    }
    namespace::validate_binding(
        bucket.root(),
        &ledger_key(&OsString::from_wide(&first.target)).1,
        first.file,
    )?;
    cleanup_finalize::metadata(
        file,
        first.file,
        &first.security,
        &first.creator,
        Some(bytes),
        &mut |_| Ok(()),
    )
}
fn busy_claim(
    root: &AnchoredRoot,
    bucket: &AnchoredRoot,
    target: &OsStr,
    cause: std::io::Error,
) -> PlatformError {
    if cause.raw_os_error() != Some(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32)
    {
        return io(cause);
    }
    let observed = (|| {
        let file = unsafe {
            windows_fs::open_relative(
                bucket.root(),
                &ledger_key(target).1,
                RelativeOptions {
                    directory: Some(false),
                    create_new: false,
                    access: FILE_GENERIC_READ,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        }
        .map_err(io)?;
        let bytes = read(&file)?;
        let log = Log::decode(&bytes)?;
        validate_control(root, bucket, &file, &bytes, &log)?;
        Ok::<bool, PlatformError>(log.generations.last().is_some_and(|g| !g.released))
    })();
    match observed {
        Ok(true) => PlatformError::DestinationExists(format!(
            "{} is reserved by an active Copy",
            target.to_string_lossy()
        )),
        Ok(false) => io(cause),
        Err(error) => error,
    }
}
impl Lease {
    pub(super) fn acquire(
        parent: &File,
        central: &File,
        target: &OsStr,
        transaction: &str,
    ) -> Result<Self, PlatformError> {
        if windows_fs::valid_component(target).is_err()
            || discovery::registry_id(OsStr::new(&format!("{transaction}.copy-journal"))).is_none()
            || !stable_identity_from_handle(parent)
                .map_err(io)?
                .is_directory_without_reparse()
            || !stable_identity_from_handle(central)
                .map_err(io)?
                .is_directory_without_reparse()
        {
            return Err(io("Invalid Copy name-claim context"));
        }
        let root =
            AnchoredRoot::claims(parent, true)?.ok_or_else(|| io("Copy claim root is missing"))?;
        if identity_at(
            parent,
            target,
            stable_identity_from_handle(root.root()).map_err(io)?,
        )? || identity_at(parent, target, root.anchor_identity())?
        {
            return Err(io(
                "Copy destination is an Ottrin claim control, not a user entry",
            ));
        }
        let (short, key) = ledger_key(target);
        let bucket = AnchoredRoot::claim_bucket(root.root(), short, true)?
            .ok_or_else(|| io("Copy claim bucket is missing"))?;
        let (mut file, created) =
            match windows_fs::create_private(bucket.root(), &key, false, FILE_SHARE_READ) {
                Ok(file) => (file, true),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing = open_file(
                        bucket.root(),
                        &key,
                        FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
                    )
                    .map_err(|error| busy_claim(&root, &bucket, target, error))?;
                    (existing, false)
                }
                Err(error) => return Err(io(error)),
            };
        let mut log = if created {
            Log::empty()
        } else {
            Log::decode(&read(&file)?)?
        };
        let creator = security::current_owner()?;
        let claim = Claim {
            parent: stable_identity_from_handle(parent).map_err(io)?,
            root: stable_identity_from_handle(root.root()).map_err(io)?,
            anchor: root.anchor_identity(),
            bucket: stable_identity_from_handle(bucket.root()).map_err(io)?,
            bucket_anchor: bucket.anchor_identity(),
            file: stable_identity_from_handle(&file).map_err(io)?,
            central: stable_identity_from_handle(central).map_err(io)?,
            target: target.encode_wide().collect(),
            transaction: transaction.to_owned(),
            security: security::PrivateSecurity::capture(&file, &creator)?,
            creator,
        };
        if !created {
            validate_control(&root, &bucket, &file, &read(&file)?, &log)?;
        }
        if log.generations.last().is_some_and(|g| !g.released) {
            return Err(PlatformError::DestinationExists(format!(
                "{} is reserved by retained Copy",
                target.to_string_lossy()
            )));
        }
        root.validate()?;
        let binding = log.append(
            &mut file,
            Entry::Claim {
                claim: Box::new(claim),
            },
        )?;
        bucket.writable_root()?.sync_all().map_err(io)?;
        Self::from_file(root, bucket, binding, file, true, false)
    }
    pub(super) fn open(
        parent: &File,
        binding: &Binding,
        terminal: bool,
        strong: bool,
    ) -> Result<Self, PlatformError> {
        let root = AnchoredRoot::claims(parent, false)?
            .ok_or_else(|| io("Copy claim root disappeared"))?;
        let lease = Self::load(root, binding.clone(), strong, terminal)?;
        if lease.released && !terminal {
            return Err(io("Copy target claim was released before terminal state"));
        }
        Ok(lease)
    }
    fn load(
        root: AnchoredRoot,
        binding: Binding,
        writer: bool,
        terminal: bool,
    ) -> Result<Self, PlatformError> {
        let (short, name) = ledger_key(&OsString::from_wide(&binding.claim.target));
        let bucket = AnchoredRoot::claim_bucket(root.root(), short, false)?
            .ok_or_else(|| io("Copy claim bucket disappeared"))?;
        let file = if writer {
            open_file(
                bucket.root(),
                &name,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            )
            .map_err(io)?
        } else {
            unsafe {
                windows_fs::open_relative(
                    bucket.root(),
                    &name,
                    RelativeOptions {
                        directory: Some(false),
                        create_new: false,
                        access: FILE_GENERIC_READ,
                        sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        security: ptr::null(),
                    },
                )
            }
            .map_err(io)?
        };
        Self::from_file(root, bucket, binding, file, writer, terminal)
    }
    fn from_file(
        root: AnchoredRoot,
        bucket: AnchoredRoot,
        binding: Binding,
        file: File,
        writer: bool,
        terminal: bool,
    ) -> Result<Self, PlatformError> {
        let bytes = read(&file)?;
        let (log, pending_release) = Log::for_binding(&bytes, &binding, terminal)?;
        let generation = log.generation(&binding)?;
        let stamp = validate_control(&root, &bucket, &file, &bytes, &log)?;
        Ok(Self {
            root,
            bucket,
            file,
            binding,
            stamp,
            digest: Sha256::digest(&bytes).into(),
            released: generation.released,
            pending_release,
            writer,
        })
    }
    pub(super) fn needs_release(&self) -> bool {
        !self.released
    }
    pub(super) fn observation(&self) -> (EntryVersion, [u8; 32]) {
        (self.stamp.clone(), self.digest)
    }
    pub(super) fn binding(&self) -> &Binding {
        &self.binding
    }
    pub(super) fn validate(&self, terminal: bool) -> Result<(), PlatformError> {
        self.root.validate()?;
        self.bucket.validate()?;
        if (self.released || self.pending_release) && !terminal {
            return Err(io("Copy target claim no longer belongs to this operation"));
        }
        if super::super::version(&self.file)? != self.stamp {
            return Err(io("Copy name-claim control changed"));
        }
        namespace::validate_single_link(&self.file)?;
        self.binding
            .claim
            .security
            .validate(&self.file, &self.binding.claim.creator)?;
        namespace::validate_binding(
            self.bucket.root(),
            &ledger_key(&OsString::from_wide(&self.binding.claim.target)).1,
            self.binding.claim.file,
        )
    }
    /// Only a verified paired terminal state permits this transition.
    pub(super) fn release_after_terminal(self) -> Result<(), PlatformError> {
        self.release_authorized()
    }
    /// Construction has not attempted Journal::create, so no payload/publication
    /// authority was ever issued. Retain the original error if cancellation of
    /// this owned admission succeeds; otherwise report both failures.
    pub(super) fn abort_unstarted(self, original: PlatformError) -> PlatformError {
        let path = self
            .bucket
            .path()
            .join(ledger_key(&OsString::from_wide(&self.binding.claim.target)).1);
        match self.release_authorized() {
            Ok(()) => original,
            Err(release) => PlatformError::CleanupPending {
                transaction: path.display().to_string(),
                message: format!(
                    "{original}; cancelling the unstarted Copy claim failed: {release}"
                ),
            },
        }
    }
    fn release_authorized(self) -> Result<(), PlatformError> {
        self.validate(true)?;
        if self.released {
            return Ok(());
        }
        let Self {
            root,
            bucket,
            file,
            binding,
            writer,
            ..
        } = self;
        let name = ledger_key(&OsString::from_wide(&binding.claim.target)).1;
        let mut writer = if writer {
            file
        } else {
            drop(file);
            open_file(
                bucket.root(),
                &name,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
            )
            .map_err(io)?
        };
        let bytes = read(&writer)?;
        let (mut log, pending_release) = Log::for_binding(&bytes, &binding, true)?;
        cleanup_finalize::metadata(
            &writer,
            binding.claim.file,
            &binding.claim.security,
            &binding.claim.creator,
            Some(&bytes),
            &mut |_| Ok(()),
        )?;
        if !log.generation(&binding)?.released {
            root.validate()?;
            bucket.validate()?;
            if pending_release {
                // The caller has independently verified both terminal journals.
                // Only the exact partial encoding of this generation's release
                // may be removed. Failure consumes this owner; no blind retry.
                writer.set_len(log.length).map_err(io)?;
                writer.sync_all().map_err(io)?;
            }
            log.append(
                &mut writer,
                Entry::Release {
                    sequence: binding.sequence,
                    digest: binding.digest,
                },
            )?;
            bucket.writable_root()?.sync_all().map_err(io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_native_claim_admission_reserves_release_without_changing_rejected_log() {
        let root = tempfile::tempdir().unwrap();
        let parent = windows_fs::open_directory(
            root.path(),
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
        )
        .unwrap();
        let central_path = root.path().join("registry");
        std::fs::create_dir(&central_path).unwrap();
        let central = windows_fs::open_directory(&central_path, FILE_GENERIC_READ).unwrap();
        let first = Lease::acquire(&parent, &central, OsStr::new("copy"), &"1".repeat(32)).unwrap();
        let binding = first.binding.clone();
        first.release_after_terminal().unwrap();
        let lease = Lease::open(&parent, &binding, true, true).unwrap();
        let before = read(&lease.file).unwrap();
        let mut next = binding.claim.clone();
        next.transaction = "2".repeat(32);
        let entry = Entry::Claim {
            claim: Box::new(next),
        };
        let claim_bytes = FRAME_HEADER + serde_json::to_vec(&entry).unwrap().len();
        struct ResetBudget;
        impl Drop for ResetBudget {
            fn drop(&mut self) {
                CLAIM_BUDGET.with(|limit| limit.set(MAX_LOG));
            }
        }
        let reset = ResetBudget;
        // This permits the complete claim frame, but none of its release.
        CLAIM_BUDGET.with(|limit| limit.set(before.len() as u64 + claim_bytes as u64));
        let mut log = Log::decode(&before).unwrap();
        let mut file = lease.file.try_clone().unwrap();
        assert!(log.append(&mut file, entry).is_err());
        assert_eq!(read(&file).unwrap(), before);
        assert_eq!(log.length, before.len() as u64);
        assert_eq!(log.sequence, 2);
        let mut next = binding.claim.clone();
        next.transaction = "2".repeat(32);
        let payload = serde_json::to_vec(&Entry::Claim {
            claim: Box::new(next),
        })
        .unwrap();
        let release = Entry::Release {
            sequence: log.sequence,
            digest: frame_hash(log.sequence, &log.previous, &payload),
        };
        let exact =
            before.len() + claim_bytes + FRAME_HEADER + serde_json::to_vec(&release).unwrap().len();
        CLAIM_BUDGET.with(|limit| limit.set(exact as u64));
        drop((file, lease));
        let next = Lease::acquire(&parent, &central, OsStr::new("copy"), &"2".repeat(32)).unwrap();
        assert_eq!(next.binding.sequence, 2);
        let completed = next.binding.clone();
        next.release_after_terminal().unwrap();
        let observed = Lease::open(&parent, &completed, true, false).unwrap();
        assert_eq!(read(&observed.file).unwrap().len(), exact);
        drop(reset);
    }
}
