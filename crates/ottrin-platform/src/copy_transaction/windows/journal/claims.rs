//! Destination-local name ownership. Each ledger holds one claim and release;
//! paired terminal journals outlive the discharged reservation.
use super::super::registry::AnchoredRoot;
use super::*;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, SYNCHRONIZE,
};

mod preparations;

const CLAIM_MAGIC: &[u8; 8] = b"OTCWCL03";

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
    fn capture(
        parent: &File,
        central: &File,
        root: &AnchoredRoot,
        bucket: &AnchoredRoot,
        file: &File,
        target: &OsStr,
        transaction: &str,
    ) -> Result<Self, PlatformError> {
        let creator = security::current_owner()?;
        Ok(Self {
            parent: stable_identity_from_handle(parent).map_err(io)?,
            root: stable_identity_from_handle(root.root()).map_err(io)?,
            anchor: root.anchor_identity(),
            bucket: stable_identity_from_handle(bucket.root()).map_err(io)?,
            bucket_anchor: bucket.anchor_identity(),
            file: stable_identity_from_handle(file).map_err(io)?,
            central: stable_identity_from_handle(central).map_err(io)?,
            target: target.encode_wide().collect(),
            transaction: transaction.to_owned(),
            security: security::PrivateSecurity::capture(file, &creator)?,
            creator,
        })
    }
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
    pub(super) fn test_release_frame(&self, bytes: &[u8]) -> Vec<u8> {
        let log = Log::decode(bytes).unwrap();
        assert!(!log.generation(self).unwrap().released);
        let entry = Entry::Release {
            sequence: self.sequence,
            digest: self.digest,
        };
        log.check(&entry).unwrap();
        encode_frame(log.sequence, &log.previous, &entry).unwrap()
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
            assert_eq!(next.owner.is_some(), prior.owner.is_some());
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
            && self.sequence == 0
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreparationPoint {
    Created,
    Prepared,
    Published,
    Flushed,
    Persisted,
}
#[cfg(not(test))]
fn preparation_boundary(_: PreparationPoint, _: &mut File) -> Result<(), PlatformError> {
    Ok(())
}
#[cfg(test)]
thread_local! { pub(super) static PREPARATION_FAULT: std::cell::Cell<Option<(PreparationPoint, u8)>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
thread_local! { pub(super) static PREPARATION_HANDOFF: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
fn preparation_boundary(point: PreparationPoint, file: &mut File) -> Result<(), PlatformError> {
    if point == PreparationPoint::Prepared {
        let handoff = PREPARATION_HANDOFF.with(|hook| hook.borrow_mut().take());
        if let Some(handoff) = handoff {
            handoff();
        }
    }
    match PREPARATION_FAULT.with(|fault| fault.get()) {
        Some((selected, 1)) if selected == point => std::process::exit(86),
        Some((selected, 2)) if selected == point => {
            // Model process death with only a first-frame prefix persisted.
            file.write_all(&CLAIM_MAGIC[..3]).map_err(io)?;
            file.sync_all().map_err(io)?;
            std::process::exit(86)
        }
        Some((selected, _)) if selected == point => {
            Err(io("injected Copy claim preparation failure"))
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetirementPoint {
    BeforeUnlink,
    Marked,
    Unlinked,
    Flushed,
}
#[cfg(not(test))]
fn retirement_boundary(_: RetirementPoint) -> Result<(), PlatformError> {
    Ok(())
}
#[cfg(test)]
type RetirementHandoff = (RetirementPoint, Box<dyn FnOnce()>);
#[cfg(test)]
thread_local! {
    pub(super) static RETIREMENT_FAULT: std::cell::Cell<Option<(RetirementPoint, bool)>> = const { std::cell::Cell::new(None) };
    pub(super) static RETIREMENT_HANDOFF: std::cell::RefCell<Option<RetirementHandoff>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn retirement_boundary(point: RetirementPoint) -> Result<(), PlatformError> {
    let handoff = RETIREMENT_HANDOFF.with(|hook| {
        let mut hook = hook.borrow_mut();
        if hook
            .as_ref()
            .is_some_and(|(selected, _)| *selected == point)
        {
            hook.take()
        } else {
            None
        }
    });
    if let Some((_, handoff)) = handoff {
        handoff();
    }
    match RETIREMENT_FAULT.with(|fault| fault.get()) {
        Some((selected, true)) if selected == point => std::process::exit(86),
        Some((selected, false)) if selected == point => {
            Err(io("injected Copy claim retirement failure"))
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReleasePoint {
    BeforeTruncate,
    Truncated,
    TruncateFlushed,
    BeforeWrite,
    Written,
    Flushed,
}
#[cfg(not(test))]
fn release_boundary(_: ReleasePoint) -> Result<(), PlatformError> {
    Ok(())
}
#[cfg(test)]
thread_local! { pub(super) static RELEASE_FAULT: std::cell::Cell<Option<(ReleasePoint, bool)>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
thread_local! { pub(super) static RELEASE_HANDOFF: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
fn release_boundary(point: ReleasePoint) -> Result<(), PlatformError> {
    if point == ReleasePoint::BeforeWrite {
        let handoff = RELEASE_HANDOFF.with(|hook| hook.borrow_mut().take());
        if let Some(handoff) = handoff {
            handoff();
        }
    }
    match RELEASE_FAULT.with(|fault| fault.get()) {
        Some((selected, true)) if selected == point => std::process::exit(86),
        Some((selected, false)) if selected == point => {
            Err(io("injected Copy claim release I/O boundary failure"))
        }
        _ => Ok(()),
    }
}

struct Generation {
    binding: Binding,
    released: bool,
}
struct Log {
    owner: Option<Generation>,
    sequence: u64,
    previous: [u8; 32],
    length: u64,
}
impl Log {
    fn empty() -> Self {
        Self {
            owner: None,
            sequence: 0,
            previous: [0; 32],
            length: 0,
        }
    }
    fn check(&self, entry: &Entry) -> Result<(), PlatformError> {
        match entry {
            Entry::Claim { claim } => {
                if !claim.valid() || self.owner.is_some() {
                    return Err(io("Invalid Copy name-claim generation"));
                }
            }
            Entry::Release { sequence, digest } => {
                let generation = self
                    .owner
                    .as_ref()
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
            Entry::Claim { claim } => {
                self.owner = Some(Generation {
                    binding: Binding {
                        claim: *claim,
                        sequence: self.sequence,
                        digest: hash,
                    },
                    released: false,
                })
            }
            Entry::Release { .. } => self.owner.as_mut().unwrap().released = true,
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
        if result.owner.is_none() {
            return Err(io("Empty Copy name-claim log; retained for review"));
        }
        result.length = at as u64;
        Ok(result)
    }
    fn for_binding(
        bytes: &[u8],
        binding: &Binding,
        recognize_release: bool,
    ) -> Result<(Self, bool), PlatformError> {
        Self::prefix(bytes)?.checked_tail(bytes, binding, recognize_release)
    }
    /// Recognition is read-only. The admission caller must prove absence of
    /// recovery records before it may invoke the authorized writer.
    fn for_admission(bytes: &[u8]) -> Result<(Self, bool), PlatformError> {
        let log = Self::prefix(bytes)?;
        let binding = log.owner.as_ref().unwrap().binding.clone();
        log.checked_tail(bytes, &binding, true)
    }
    fn checked_tail(
        self,
        bytes: &[u8],
        binding: &Binding,
        recognize_release: bool,
    ) -> Result<(Self, bool), PlatformError> {
        let log = self;
        if log.length == bytes.len() as u64 {
            return Ok((log, false));
        }
        let own_active = log
            .owner
            .as_ref()
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
        if !recognize_release
            || !own_active
            || tail.len() >= expected.len()
            || !expected.starts_with(tail)
        {
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
        let release = matches!(entry, Entry::Release { .. });
        if release {
            release_boundary(ReleasePoint::BeforeWrite)?;
        }
        file.seek(SeekFrom::End(0)).map_err(io)?;
        file.write_all(&frame).map_err(io)?;
        if release {
            release_boundary(ReleasePoint::Written)?;
        }
        file.sync_all().map_err(io)?;
        if release {
            release_boundary(ReleasePoint::Flushed)?;
        }
        self.apply(entry, hash)?;
        self.length += frame.len() as u64;
        Ok(self.owner.as_ref().unwrap().binding.clone())
    }
    /// The caller holds the exclusive writer and independently established
    /// either paired terminal state or absence of this generation's records.
    fn finish_release(
        &mut self,
        file: &mut File,
        binding: &Binding,
        pending: bool,
    ) -> Result<(), PlatformError> {
        let entry = Entry::Release {
            sequence: binding.sequence,
            digest: binding.digest,
        };
        self.check(&entry)?;
        if pending {
            release_boundary(ReleasePoint::BeforeTruncate)?;
            file.set_len(self.length).map_err(io)?;
            release_boundary(ReleasePoint::Truncated)?;
            file.sync_all().map_err(io)?;
            release_boundary(ReleasePoint::TruncateFlushed)?;
        }
        self.append(file, entry).map(|_| ())
    }
    fn generation(&self, expected: &Binding) -> Result<&Generation, PlatformError> {
        self.owner
            .as_ref()
            .filter(|g| g.binding == *expected)
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
fn observed_file(root: &File, name: &OsStr) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            root,
            name,
            RelativeOptions {
                directory: Some(false),
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    }
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
        .owner
        .as_ref()
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
        Ok::<bool, PlatformError>(log.owner.as_ref().is_some_and(|g| !g.released))
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
// Only an exclusive writer with neither canonical recovery record may cancel
// an abandoned active claim. Released claims need no historical record scan.
fn records_absent(parent: &File, central: &File, claim: &Claim) -> Result<bool, PlatformError> {
    fn vacant(parent: &File, name: &OsStr) -> Result<bool, PlatformError> {
        match unsafe {
            windows_fs::open_relative(
                parent,
                name,
                RelativeOptions {
                    directory: None,
                    create_new: false,
                    access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        } {
            Ok(_) => Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(io(error)),
        }
    }
    if !vacant(
        parent,
        OsStr::new(&format!(".ottrin-copy-{}", claim.transaction)),
    )? {
        return Ok(false);
    }
    let Ok(registry) = discovery::Volumes::new(central)?.resolve(
        claim.central,
        FILE_GENERIC_READ,
        &mut |_| Ok(()),
    ) else {
        return Ok(false);
    };
    vacant(
        &registry,
        OsStr::new(&format!("{}.copy-journal", claim.transaction)),
    )
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
        preparations::collect(parent, &root, central)?;
        let mut file = match open_file(
            bucket.root(),
            &key,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Self::prepare_new(parent, central, target, transaction, root, bucket);
            }
            Err(error) => return Err(busy_claim(&root, &bucket, target, error)),
        };
        let (mut log, pending_release) = Log::for_admission(&read(&file)?)?;
        {
            let mut bytes = read(&file)?;
            validate_control(&root, &bucket, &file, &bytes, &log)?;
            let last = log.owner.as_ref().unwrap();
            if !last.released && records_absent(parent, central, &last.binding.claim)? {
                // The prior kernel writer is gone and no transaction records
                // exist to authorize any further payload work.
                // Release only this orphan before replacing its control.
                let binding = last.binding.clone();
                validate_control(&root, &bucket, &file, &bytes, &log)?;
                log.finish_release(&mut file, &binding, pending_release)?;
                bucket.writable_root()?.sync_all().map_err(io)?;
                bytes = read(&file)?;
            }
            if log
                .owner
                .as_ref()
                .is_some_and(|generation| generation.released)
            {
                // Terminal journal recovery is independent of this discharged
                // control. Retire it before publishing a fresh complete claim.
                validate_control(&root, &bucket, &file, &bytes, &log)?;
                retirement_boundary(RetirementPoint::BeforeUnlink)?;
                cleanup_tree::dispose(&file)?;
                retirement_boundary(RetirementPoint::Marked)?;
                drop(file);
                retirement_boundary(RetirementPoint::Unlinked)?;
                bucket.writable_root()?.sync_all().map_err(io)?;
                retirement_boundary(RetirementPoint::Flushed)?;
                return Self::prepare_new(parent, central, target, transaction, root, bucket);
            }
        }
        Err(PlatformError::DestinationExists(format!(
            "{} is reserved by retained Copy",
            target.to_string_lossy()
        )))
    }
    fn prepare_new(
        parent: &File,
        central: &File,
        target: &OsStr,
        transaction: &str,
        root: AnchoredRoot,
        bucket: AnchoredRoot,
    ) -> Result<Self, PlatformError> {
        // Root-level preparations cannot collide with keys in either key bucket.
        // The canonical name is never occupied by an empty or partial first frame.
        let name = OsString::from(format!(".preparing-{transaction}"));
        let mut file =
            windows_fs::create_private_temporary_file(root.root(), &name, FILE_SHARE_READ)
                .map_err(io)?;
        let claim = Claim::capture(parent, central, &root, &bucket, &file, target, transaction)?;
        let key = ledger_key(target).1;
        let mut published = false;
        let mut persisted = false;
        let result = (|| {
            preparation_boundary(PreparationPoint::Created, &mut file)?;
            let binding = Log::empty().append(
                &mut file,
                Entry::Claim {
                    claim: Box::new(claim.clone()),
                },
            )?;
            let bytes = read(&file)?;
            let decoded = Log::decode(&bytes)?;
            if decoded.sequence != 1 || decoded.generation(&binding)?.released {
                return Err(io("Prepared Copy claim is not its complete first frame"));
            }
            root.validate()?;
            bucket.validate()?;
            namespace::validate_binding(root.root(), &name, claim.file)?;
            cleanup_finalize::metadata(
                &file,
                claim.file,
                &claim.security,
                &claim.creator,
                Some(&bytes),
                &mut |_| Ok(()),
            )?;
            root.writable_root()?.sync_all().map_err(io)?;
            preparation_boundary(PreparationPoint::Prepared, &mut file)?;
            crate::move_transaction::rename_windows_handle_no_replace(
                &file,
                &bucket.writable_root()?,
                &key,
            )
            .map_err(|error| match error {
                PlatformError::DestinationExists(_) => PlatformError::DestinationExists(format!(
                    "{} is reserved by an active Copy",
                    target.to_string_lossy()
                )),
                error => error,
            })?;
            published = true;
            preparation_boundary(PreparationPoint::Published, &mut file)?;
            root.writable_root()?.sync_all().map_err(io)?;
            bucket.writable_root()?.sync_all().map_err(io)?;
            preparation_boundary(PreparationPoint::Flushed, &mut file)?;
            windows_fs::persist_temporary_file(&file).map_err(io)?;
            persisted = true;
            preparation_boundary(PreparationPoint::Persisted, &mut file)?;
            Ok(binding)
        })();
        match result {
            Ok(binding) => {
                let location = bucket.path().join(&key);
                Self::from_file(root, bucket, binding, file, true, false).map_err(|error| {
                    PlatformError::CleanupPending {
                        transaction: location.display().to_string(),
                        message: format!("{error}; complete unstarted claim retained"),
                    }
                })
            }
            Err(error) if !persisted => {
                // Close only the file created by this admission. Kernel cleanup
                // also covers abrupt exit before the first byte was written.
                drop(file);
                let cleanup = (|| {
                    root.validate()?;
                    bucket.validate()?;
                    if identity_at(root.root(), &name, claim.file)?
                        || published && identity_at(bucket.root(), &key, claim.file)?
                    {
                        return Err(io("Temporary claim remains after owner close"));
                    }
                    root.writable_root()?.sync_all().map_err(io)?;
                    bucket.writable_root()?.sync_all().map_err(io)
                })();
                match cleanup {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(PlatformError::CleanupPending {
                        transaction: if published {
                            bucket.path().join(&key)
                        } else {
                            root.path().join(&name)
                        }
                        .display()
                        .to_string(),
                        message: format!(
                            "{error}; temporary claim close/cleanup requires review: {cleanup}"
                        ),
                    }),
                }
            }
            Err(error) => Err(PlatformError::CleanupPending {
                transaction: bucket.path().join(&key).display().to_string(),
                message: format!("{error}; complete unstarted claim retained after publication"),
            }),
        }
    }
    /// Call only while independently verified paired terminal journals remain
    /// held. A new owner is neither parsed nor mutated on the old one's behalf.
    pub(super) fn observe_terminal(
        parent: &File,
        binding: &Binding,
    ) -> Result<Option<Self>, PlatformError> {
        let Some(root) = AnchoredRoot::claims(parent, false)? else {
            return Ok(None);
        };
        if stable_identity_from_handle(root.root()).map_err(io)? != binding.claim.root
            || root.anchor_identity() != binding.claim.anchor
        {
            return Ok(None);
        }
        let (short, name) = ledger_key(&OsString::from_wide(&binding.claim.target));
        let Some(bucket) = AnchoredRoot::claim_bucket(root.root(), short, false)? else {
            return Ok(None);
        };
        if stable_identity_from_handle(bucket.root()).map_err(io)? != binding.claim.bucket
            || bucket.anchor_identity() != binding.claim.bucket_anchor
        {
            return Ok(None);
        }
        let file = match observed_file(bucket.root(), &name) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io(error)),
        };
        // Type/reparse changes on the same object are corruption to validate,
        // not a different owner whose bytes may be ignored.
        if !stable_identity_from_handle(&file)
            .map_err(io)?
            .same_object(binding.claim.file)
        {
            return Ok(None);
        }
        Self::from_file(root, bucket, binding.clone(), file, false, true).map(Some)
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
            observed_file(bucket.root(), &name).map_err(io)?
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
            log.finish_release(&mut writer, &binding, pending_release)?;
            bucket.writable_root()?.sync_all().map_err(io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_native_claim_retirement_preserves_retained_records_and_unknown_control_content() {
        let root = tempfile::tempdir().unwrap();
        let parent = windows_fs::open_directory(
            root.path(),
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED,
        )
        .unwrap();
        let registry_path = root.path().join("registry");
        std::fs::create_dir(&registry_path).unwrap();
        let registry = windows_fs::open_directory(&registry_path, FILE_GENERIC_READ).unwrap();
        let first_id = "1".repeat(32);
        let first = Lease::acquire(&parent, &registry, OsStr::new("copy"), &first_id).unwrap();
        let binding = first.binding.clone();
        first.release_after_terminal().unwrap();
        let ledger = root.path().join(binding.test_relative_path());
        let mirror = registry_path.join(format!("{first_id}.copy-journal"));
        std::fs::write(&mirror, b"preserve retained registry record").unwrap();
        let second =
            Lease::acquire(&parent, &registry, OsStr::new("copy"), &"2".repeat(32)).unwrap();
        assert_ne!(second.binding.claim.file, binding.claim.file);
        assert_eq!(second.binding.sequence, 0);
        second.release_after_terminal().unwrap();
        assert_eq!(
            std::fs::read(&mirror).unwrap(),
            b"preserve retained registry record"
        );
        std::fs::remove_file(&mirror).unwrap();
        let stage = root.path().join(format!(".ottrin-copy-{first_id}"));
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("unknown"), b"keep stage content").unwrap();
        let third =
            Lease::acquire(&parent, &registry, OsStr::new("copy"), &"3".repeat(32)).unwrap();
        assert_ne!(third.binding.claim.file, binding.claim.file);
        third.release_after_terminal().unwrap();
        assert_eq!(
            std::fs::read(stage.join("unknown")).unwrap(),
            b"keep stage content"
        );
        std::fs::remove_dir_all(&stage).unwrap();
        let before = std::fs::read(&ledger).unwrap();
        let stream = ledger.with_file_name(format!(
            "{}:foreign",
            ledger.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&stream, b"keep unknown stream").unwrap();
        assert!(Lease::acquire(&parent, &registry, OsStr::new("copy"), &"4".repeat(32)).is_err());
        assert_eq!(std::fs::read(&ledger).unwrap(), before);
        assert_eq!(std::fs::read(&stream).unwrap(), b"keep unknown stream");
        std::fs::remove_file(&stream).unwrap();
        // Resolve the old registry by native ID, not its original pathname.
        drop(registry);
        std::fs::rename(&registry_path, root.path().join("moved-registry")).unwrap();
        std::fs::create_dir(&registry_path).unwrap();
        let registry = windows_fs::open_directory(&registry_path, FILE_GENERIC_READ).unwrap();
        let fresh =
            Lease::acquire(&parent, &registry, OsStr::new("copy"), &"5".repeat(32)).unwrap();
        assert_ne!(fresh.binding.claim.file, binding.claim.file);
        assert_eq!(fresh.binding.sequence, 0);
        fresh.release_after_terminal().unwrap();
    }

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
        let before = read(&first.file).unwrap();
        let entry = Entry::Claim {
            claim: Box::new(binding.claim.clone()),
        };
        let release = binding.test_release_frame(&before);
        struct ResetBudget;
        impl Drop for ResetBudget {
            fn drop(&mut self) {
                CLAIM_BUDGET.with(|limit| limit.set(MAX_LOG));
            }
        }
        let reset = ResetBudget;
        // A complete first frame alone is insufficient: reserve its release
        // before admitting any owner, and leave bytes untouched on rejection.
        CLAIM_BUDGET.with(|limit| limit.set(before.len() as u64));
        let mut log = Log::empty();
        let mut file = first.file.try_clone().unwrap();
        assert!(log.append(&mut file, entry).is_err());
        assert_eq!(read(&file).unwrap(), before);
        assert_eq!(log.length, 0);
        assert_eq!(log.sequence, 0);
        let exact = before.len() + release.len();
        CLAIM_BUDGET.with(|limit| limit.set(exact as u64));
        drop(file);
        first.release_after_terminal().unwrap();
        let observed = Lease::open(&parent, &binding, true, false).unwrap();
        assert_eq!(read(&observed.file).unwrap().len(), exact);
        drop(reset);
    }
}
