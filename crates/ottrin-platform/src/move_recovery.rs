use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[cfg(target_os = "linux")]
use rustix::fd::OwnedFd;

pub const JOURNAL_SCHEMA_MAJOR: u16 = 2;
pub const JOURNAL_SCHEMA_MINOR: u16 = 0;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativePathEncoding {
    UnixBytes,
    WindowsUtf16Le,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePath {
    encoding: NativePathEncoding,
    units_hex: String,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalPhase {
    Prepared,
    ClaimIntent,
    Claimed,
    UnexpectedClaimRetained,
    PendingDestination,
    Published,
}

#[cfg(target_os = "linux")]
impl JournalPhase {
    pub fn validate_transition(self, to: Self) -> Result<(), JournalError> {
        if self == to
            || matches!(
                (self, to),
                (Self::Prepared, Self::ClaimIntent)
                    | (Self::ClaimIntent, Self::Claimed)
                    | (Self::ClaimIntent, Self::UnexpectedClaimRetained)
                    | (Self::Claimed, Self::PendingDestination)
                    | (Self::PendingDestination, Self::Published)
            )
        {
            Ok(())
        } else {
            Err(JournalError::InvalidPhaseTransition { from: self, to })
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinuxFileType {
    Regular,
    Directory,
    Symlink,
    Other,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinuxStableIdentity {
    device: u64,
    inode: u64,
    file_type: LinuxFileType,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PreparedRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
    source_transaction_name: NativePath,
    source_transaction_identity: LinuxStableIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPreparedRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
    source_transaction_name: NativePath,
    source_transaction_identity: LinuxStableIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClaimIntentRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
    source_transaction_name: NativePath,
    source_transaction_identity: LinuxStableIdentity,
    expected_source_identity: LinuxStableIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClaimIntentRecord {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
    phase: JournalPhase,
    source_parent: NativePath,
    original_name: NativePath,
    source_parent_identity: LinuxStableIdentity,
    source_entry_identity: LinuxStableIdentity,
    source_transaction_name: NativePath,
    source_transaction_identity: LinuxStableIdentity,
    expected_source_identity: LinuxStableIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JournalHeader {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawJournalHeader {
    schema_major: u16,
    schema_minor: u16,
    transaction_id: String,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum JournalError {
    #[error("journal JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("unsupported journal schema {major}.{minor}")]
    UnsupportedSchema { major: u16, minor: u16 },
    #[error("invalid transaction ID")]
    InvalidTransactionId,
    #[error("journal path encoding does not match this platform")]
    WrongPlatformEncoding,
    #[error("invalid native path units")]
    InvalidNativePathUnits,
    #[error("invalid prepared source path")]
    InvalidPreparedSourcePath,
    #[error("invalid Linux stable identity")]
    InvalidLinuxIdentity,
    #[cfg(target_os = "linux")]
    #[error("invalid journal phase transition from {from:?} to {to:?}")]
    InvalidPhaseTransition {
        from: JournalPhase,
        to: JournalPhase,
    },
    #[cfg(target_os = "linux")]
    #[error("prepared journal cannot contain phase {0:?}")]
    InvalidPreparedPhase(JournalPhase),
    #[cfg(target_os = "linux")]
    #[error("claim-intent journal cannot contain phase {0:?}")]
    InvalidClaimIntentPhase(JournalPhase),
    #[cfg(target_os = "linux")]
    #[error("claim-intent expected source identity does not match the prepared source identity")]
    MismatchedExpectedSourceIdentity,
    #[cfg(target_os = "linux")]
    #[error("source transaction name does not match the transaction ID")]
    MismatchedSourceTransactionName,
    #[error("source transaction name already exists")]
    SourceTransactionExists,
    #[error("journal I/O failed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
/// Handle-relative journal mutation authority created only inside trusted recovery code.
///
/// Arbitrary pathnames cannot be promoted to journal authority:
///
/// ```compile_fail
/// use ottrin_platform::move_recovery::LinuxJournalDirectory;
/// use std::path::Path;
///
/// let _ = LinuxJournalDirectory::open(Path::new("."));
/// ```
#[derive(Debug)]
pub struct LinuxJournalDirectory {
    directory: OwnedFd,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct LinuxSourceParentDirectory {
    directory: OwnedFd,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxIdentityHandle {
    _directory: OwnedFd,
}

#[cfg(target_os = "linux")]
/// An observed source-transaction candidate without ownership or mutation authority.
///
/// The retained identity descriptor is deliberately not exposed to callers:
///
/// ```compile_fail
/// use ottrin_platform::move_recovery::LinuxSourceParentDirectory;
/// use std::path::Path;
///
/// let parent = LinuxSourceParentDirectory::open(Path::new(".")).unwrap();
/// let candidate = parent.create_transaction([0_u8; 16]).unwrap();
/// let _ = candidate.identity_handle();
/// ```
#[derive(Debug)]
pub struct LinuxUnprovenSourceTransaction {
    name: PathBuf,
    observed_identity: LinuxStableIdentity,
    _identity_handle: Option<LinuxIdentityHandle>,
}

#[cfg(target_os = "linux")]
/// Source-transaction evidence retained after setup failure, without journal authority.
///
/// ```compile_fail
/// use ottrin_platform::move_recovery::LinuxRetainedSourceTransaction;
///
/// fn promote(retained: &LinuxRetainedSourceTransaction) {
///     let _ = retained.journal_directory();
/// }
/// ```
#[derive(Debug)]
pub struct LinuxRetainedSourceTransaction {
    name: PathBuf,
    parent_directory: LinuxSourceParentDirectory,
    identity: Option<LinuxStableIdentity>,
    _identity_handle: Option<LinuxIdentityHandle>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error)]
pub enum LinuxCreateSourceTransactionError {
    #[error(transparent)]
    Creation(JournalError),
    #[error("source transaction was created but setup failed: {failure}")]
    Retained {
        #[source]
        failure: JournalError,
        retained: LinuxRetainedSourceTransaction,
    },
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxPreparedJournals {
    MatchingPair,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxPrepareJournalsError {
    #[error("central prepared journal failed: {0}")]
    Central(JournalError),
    #[error("central prepared journal is durable but source-local journal failed: {0}")]
    CentralOnly(JournalError),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxPreparedPairError {
    #[error("central prepared journal is corrupt: {0}")]
    CentralCorrupt(JournalError),
    #[error("source-local prepared journal is corrupt: {0}")]
    LocalCorrupt(JournalError),
    #[error("central and source-local prepared journals disagree")]
    Mismatch,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxJournalReadError {
    #[error("journal is missing")]
    Missing,
    #[error("journal read failed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxLoadPreparedPairError {
    #[error("central prepared journal could not be read: {0}")]
    CentralRead(LinuxJournalReadError),
    #[error("source-local prepared journal could not be read: {0}")]
    LocalRead(LinuxJournalReadError),
    #[error("prepared journal pair is invalid: {0}")]
    InvalidPair(LinuxPreparedPairError),
    #[error("prepared journals do not belong to the requested transaction")]
    RequestedTransactionMismatch,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxDiscoverPreparedError {
    #[error("central journal directory could not be read: {0}")]
    DirectoryRead(String),
    #[error("central journal filename bytes are invalid: {0:?}")]
    InvalidFilename(Vec<u8>),
    #[error("central prepared journal {name} could not be read: {error}")]
    JournalRead {
        name: String,
        error: LinuxJournalReadError,
    },
    #[error("central prepared journal {name} is invalid: {error}")]
    InvalidRecord { name: String, error: JournalError },
    #[error(
        "central journal filename transaction ID {filename:?} and record transaction ID {record:?} disagree"
    )]
    FilenameTransactionMismatch {
        filename: [u8; 16],
        record: [u8; 16],
    },
}

#[cfg(target_os = "linux")]
pub fn validate_prepared_pair(
    central: &[u8],
    local: &[u8],
) -> Result<LinuxPreparedJournals, LinuxPreparedPairError> {
    let central = PreparedRecord::parse(central).map_err(LinuxPreparedPairError::CentralCorrupt)?;
    let local = PreparedRecord::parse(local).map_err(LinuxPreparedPairError::LocalCorrupt)?;
    if central != local {
        return Err(LinuxPreparedPairError::Mismatch);
    }
    Ok(LinuxPreparedJournals::MatchingPair)
}

#[cfg(target_os = "linux")]
impl LinuxSourceParentDirectory {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        use rustix::fs::{CWD, Mode, OFlags, openat};

        let directory = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        Ok(Self { directory })
    }

    pub fn create_transaction(
        &self,
        transaction_id: [u8; 16],
    ) -> Result<LinuxUnprovenSourceTransaction, LinuxCreateSourceTransactionError> {
        self.create_transaction_with_hooks(transaction_id, || Ok(()), || Ok(()))
    }

    #[cfg(test)]
    fn create_transaction_with_post_create<F>(
        &self,
        transaction_id: [u8; 16],
        post_create: F,
    ) -> Result<LinuxUnprovenSourceTransaction, LinuxCreateSourceTransactionError>
    where
        F: FnOnce() -> Result<(), JournalError>,
    {
        self.create_transaction_with_hooks(transaction_id, post_create, || Ok(()))
    }

    fn create_transaction_with_hooks<F, G>(
        &self,
        transaction_id: [u8; 16],
        post_create: F,
        post_identity_open: G,
    ) -> Result<LinuxUnprovenSourceTransaction, LinuxCreateSourceTransactionError>
    where
        F: FnOnce() -> Result<(), JournalError>,
        G: FnOnce() -> Result<(), JournalError>,
    {
        use rustix::fs::{AtFlags, FileType, Mode, OFlags, fstat, mkdirat, openat, statat};

        let name = PathBuf::from(format!(".ottrin-move-{}", encode_hex(&transaction_id)));
        let mut retained_parent = Some(LinuxSourceParentDirectory {
            directory: rustix::io::fcntl_dupfd_cloexec(&self.directory, 0).map_err(|error| {
                LinuxCreateSourceTransactionError::Creation(JournalError::Io(error.to_string()))
            })?,
        });
        mkdirat(&self.directory, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(|error| {
            let failure = if error == rustix::io::Errno::EXIST {
                JournalError::SourceTransactionExists
            } else {
                JournalError::Io(error.to_string())
            };
            LinuxCreateSourceTransactionError::Creation(failure)
        })?;
        post_create().map_err(|failure| {
            Self::retained_error(
                &mut retained_parent,
                name.clone(),
                failure,
                None,
                None,
                None,
            )
        })?;
        let identity_directory = openat(
            &self.directory,
            &name,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            Self::retained_error(
                &mut retained_parent,
                name.clone(),
                JournalError::Io(error.to_string()),
                None,
                None,
                None,
            )
        })?;
        let opened = match fstat(&identity_directory) {
            Ok(opened) => opened,
            Err(error) => {
                return Err(Self::retained_error(
                    &mut retained_parent,
                    name,
                    JournalError::Io(error.to_string()),
                    Some(identity_directory),
                    None,
                    None,
                ));
            }
        };
        let named = match statat(&self.directory, &name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(named) => named,
            Err(error) => {
                return Err(Self::retained_error(
                    &mut retained_parent,
                    name,
                    JournalError::Io(error.to_string()),
                    Some(identity_directory),
                    None,
                    None,
                ));
            }
        };
        if FileType::from_raw_mode(opened.st_mode) != FileType::Directory
            || opened.st_uid != rustix::process::geteuid().as_raw()
            || opened.st_dev != named.st_dev
            || opened.st_ino != named.st_ino
        {
            return Err(Self::retained_error(
                &mut retained_parent,
                name,
                JournalError::InvalidLinuxIdentity,
                Some(identity_directory),
                None,
                None,
            ));
        }
        let identity =
            LinuxStableIdentity::new(opened.st_dev, opened.st_ino, LinuxFileType::Directory);
        if let Err(failure) = post_identity_open() {
            return Err(Self::retained_error(
                &mut retained_parent,
                name,
                failure,
                Some(identity_directory),
                Some(identity),
                None,
            ));
        }
        Ok(LinuxUnprovenSourceTransaction {
            name,
            observed_identity: identity,
            _identity_handle: Some(LinuxIdentityHandle {
                _directory: identity_directory,
            }),
        })
    }

    fn retained_error(
        parent: &mut Option<LinuxSourceParentDirectory>,
        name: PathBuf,
        failure: JournalError,
        identity_directory: Option<OwnedFd>,
        identity: Option<LinuxStableIdentity>,
        unverified_journal_directory: Option<OwnedFd>,
    ) -> LinuxCreateSourceTransactionError {
        // A directory opened from the current pathname is not journal authority until every
        // identity check succeeds. Dropping it here prevents retained failures from exposing a
        // mutable LinuxJournalDirectory capability for a replacement entry.
        drop(unverified_journal_directory);
        LinuxCreateSourceTransactionError::Retained {
            failure,
            retained: LinuxRetainedSourceTransaction {
                name,
                parent_directory: parent
                    .take()
                    .expect("the retained parent is consumed only on an error return"),
                identity,
                _identity_handle: identity_directory.map(|directory| LinuxIdentityHandle {
                    _directory: directory,
                }),
            },
        }
    }

    pub fn sync(&self) -> Result<(), JournalError> {
        rustix::fs::fsync(&self.directory).map_err(|error| JournalError::Io(error.to_string()))
    }
}

#[cfg(target_os = "linux")]
impl LinuxUnprovenSourceTransaction {
    pub fn name(&self) -> &Path {
        &self.name
    }

    pub fn observed_identity(&self) -> LinuxStableIdentity {
        self.observed_identity
    }
}

#[cfg(target_os = "linux")]
impl LinuxRetainedSourceTransaction {
    pub fn name(&self) -> &Path {
        &self.name
    }

    pub fn identity(&self) -> Option<LinuxStableIdentity> {
        self.identity
    }

    pub fn parent_directory(&self) -> &LinuxSourceParentDirectory {
        &self.parent_directory
    }
}

#[cfg(target_os = "linux")]
impl LinuxJournalDirectory {
    #[cfg(test)]
    fn open(path: &Path) -> Result<Self, JournalError> {
        use rustix::fs::{CWD, Mode, OFlags, openat};

        let directory = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        Ok(Self { directory })
    }

    pub fn replace(&self, header: &JournalHeader) -> Result<(), JournalError> {
        self.replace_named("journal", header)
    }

    pub fn persist_discovery_header(&self, header: &JournalHeader) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", header.transaction_id), header)
    }

    pub fn persist_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", record.transaction_id), record)
    }

    pub fn persist_local_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named("journal", record)
    }

    pub fn prepare_with_local(
        &self,
        local: &Self,
        record: &PreparedRecord,
    ) -> Result<LinuxPreparedJournals, LinuxPrepareJournalsError> {
        self.persist_prepared(record)
            .map_err(LinuxPrepareJournalsError::Central)?;
        local
            .persist_local_prepared(record)
            .map_err(LinuxPrepareJournalsError::CentralOnly)?;
        Ok(LinuxPreparedJournals::MatchingPair)
    }

    pub fn load_prepared_pair(
        &self,
        local: &Self,
        transaction_id: [u8; 16],
    ) -> Result<LinuxPreparedJournals, LinuxLoadPreparedPairError> {
        let central = self
            .read_named(&format!("{}.journal", encode_hex(&transaction_id)))
            .map_err(LinuxLoadPreparedPairError::CentralRead)?;
        let local = local
            .read_named("journal")
            .map_err(LinuxLoadPreparedPairError::LocalRead)?;
        let matching = validate_prepared_pair(&central, &local)
            .map_err(LinuxLoadPreparedPairError::InvalidPair)?;
        let central_record = PreparedRecord::parse(&central).map_err(|error| {
            LinuxLoadPreparedPairError::InvalidPair(LinuxPreparedPairError::CentralCorrupt(error))
        })?;
        if central_record.transaction_id != encode_hex(&transaction_id) {
            return Err(LinuxLoadPreparedPairError::RequestedTransactionMismatch);
        }
        Ok(matching)
    }

    pub fn discover_prepared(&self) -> Result<Vec<[u8; 16]>, LinuxDiscoverPreparedError> {
        use rustix::fs::Dir;

        let mut directory = Dir::read_from(&self.directory)
            .map_err(|error| LinuxDiscoverPreparedError::DirectoryRead(error.to_string()))?;
        let mut transaction_ids = Vec::new();
        while let Some(entry) = directory.read() {
            let entry = entry
                .map_err(|error| LinuxDiscoverPreparedError::DirectoryRead(error.to_string()))?;
            let name = entry.file_name().to_bytes();
            let valid_stale_update = name.len() == 78
                && name[0..32]
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                && &name[32..46] == b".journal.next-"
                && name[46..]
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
            if valid_stale_update {
                continue;
            }
            if name
                .windows(b".journal.next".len())
                .any(|window| window == b".journal.next")
            {
                return Err(LinuxDiscoverPreparedError::InvalidFilename(name.to_vec()));
            }
            let Some(stem) = name.strip_suffix(b".journal") else {
                continue;
            };
            if stem.len() != 32
                || !stem
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(LinuxDiscoverPreparedError::InvalidFilename(name.to_vec()));
            }
            let name = std::str::from_utf8(name).expect("validated journal names are ASCII");
            let encoded =
                self.read_named(name)
                    .map_err(|error| LinuxDiscoverPreparedError::JournalRead {
                        name: name.to_owned(),
                        error,
                    })?;
            let record = PreparedRecord::parse(&encoded).map_err(|error| {
                LinuxDiscoverPreparedError::InvalidRecord {
                    name: name.to_owned(),
                    error,
                }
            })?;
            let filename_decoded =
                decode_hex(std::str::from_utf8(stem).expect("validated journal stems are ASCII"))
                    .expect("validated journal stems are hexadecimal");
            let mut filename_transaction_id = [0_u8; 16];
            filename_transaction_id.copy_from_slice(&filename_decoded);
            let record_decoded = decode_hex(&record.transaction_id)
                .expect("validated prepared record has a hexadecimal transaction ID");
            let mut record_transaction_id = [0_u8; 16];
            record_transaction_id.copy_from_slice(&record_decoded);
            if record.transaction_id.as_bytes() != stem {
                return Err(LinuxDiscoverPreparedError::FilenameTransactionMismatch {
                    filename: filename_transaction_id,
                    record: record_transaction_id,
                });
            }
            transaction_ids.push(record_transaction_id);
        }
        transaction_ids.sort_unstable();
        Ok(transaction_ids)
    }

    fn read_named(&self, name: &str) -> Result<Vec<u8>, LinuxJournalReadError> {
        use rustix::fs::{Mode, OFlags, openat};
        use std::fs::File;
        use std::io::Read;

        let journal = openat(
            &self.directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::NOENT {
                LinuxJournalReadError::Missing
            } else {
                LinuxJournalReadError::Io(error.to_string())
            }
        })?;
        let mut encoded = Vec::new();
        File::from(journal)
            .read_to_end(&mut encoded)
            .map_err(|error| LinuxJournalReadError::Io(error.to_string()))?;
        Ok(encoded)
    }

    fn replace_named<T: Serialize>(&self, name: &str, record: &T) -> Result<(), JournalError> {
        use rustix::fs::{Mode, OFlags, fsync, openat, renameat};
        use std::fs::File;
        use std::io::Write;

        let encoded = serde_json::to_vec(record)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| JournalError::Io(error.to_string()))?;
        let next_name = format!("{name}.next-{}", encode_hex(&random));
        let next = openat(
            &self.directory,
            next_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| JournalError::Io(error.to_string()))?;
        let mut next = File::from(next);
        next.write_all(&encoded)
            .and_then(|()| next.sync_all())
            .map_err(|error| JournalError::Io(error.to_string()))?;
        renameat(&self.directory, next_name.as_str(), &self.directory, name)
            .map_err(|error| JournalError::Io(error.to_string()))?;
        fsync(&self.directory).map_err(|error| JournalError::Io(error.to_string()))
    }
}

impl NativePath {
    pub fn encode(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self {
                encoding: NativePathEncoding::UnixBytes,
                units_hex: encode_hex(path.as_os_str().as_bytes()),
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let units_hex = path
                .as_os_str()
                .encode_wide()
                .map(|unit| format!("{unit:04x}"))
                .collect();
            Self {
                encoding: NativePathEncoding::WindowsUtf16Le,
                units_hex,
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self {
                encoding: NativePathEncoding::UnixBytes,
                units_hex: encode_hex(path.to_string_lossy().as_bytes()),
            }
        }
    }

    pub fn decode(&self) -> Result<PathBuf, JournalError> {
        #[cfg(unix)]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(JournalError::WrongPlatformEncoding);
            }
            Ok(PathBuf::from(OsString::from_vec(decode_hex(
                &self.units_hex,
            )?)))
        }
        #[cfg(windows)]
        {
            use std::ffi::OsString;
            use std::os::windows::ffi::OsStringExt;
            if self.encoding != NativePathEncoding::WindowsUtf16Le {
                return Err(JournalError::WrongPlatformEncoding);
            }
            if !self.units_hex.len().is_multiple_of(4) {
                return Err(JournalError::InvalidNativePathUnits);
            }
            let units = self
                .units_hex
                .as_bytes()
                .chunks_exact(4)
                .map(decode_u16_hex)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(PathBuf::from(OsString::from_wide(&units)))
        }
        #[cfg(not(any(unix, windows)))]
        {
            if self.encoding != NativePathEncoding::UnixBytes {
                return Err(JournalError::WrongPlatformEncoding);
            }
            let bytes = decode_hex(&self.units_hex)?;
            let text =
                String::from_utf8(bytes).map_err(|_| JournalError::InvalidNativePathUnits)?;
            Ok(PathBuf::from(text))
        }
    }
}

impl JournalHeader {
    pub fn new(transaction_id: [u8; 16]) -> Self {
        Self {
            schema_major: JOURNAL_SCHEMA_MAJOR,
            schema_minor: JOURNAL_SCHEMA_MINOR,
            transaction_id: encode_hex(&transaction_id),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawJournalHeader = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

#[cfg(target_os = "linux")]
impl LinuxStableIdentity {
    pub fn new(device: u64, inode: u64, file_type: LinuxFileType) -> Self {
        Self {
            device,
            inode,
            file_type,
        }
    }
}

#[cfg(target_os = "linux")]
impl PreparedRecord {
    pub fn new_linux(
        transaction_id: [u8; 16],
        source_parent: &Path,
        original_name: &Path,
        source_parent_identity: LinuxStableIdentity,
        source_entry_identity: LinuxStableIdentity,
        source_transaction_name: &Path,
        source_transaction_identity: LinuxStableIdentity,
    ) -> Result<Self, JournalError> {
        Self::try_from(RawPreparedRecord {
            schema_major: JOURNAL_SCHEMA_MAJOR,
            schema_minor: JOURNAL_SCHEMA_MINOR,
            transaction_id: encode_hex(&transaction_id),
            phase: JournalPhase::Prepared,
            source_parent: NativePath::encode(source_parent),
            original_name: NativePath::encode(original_name),
            source_parent_identity,
            source_entry_identity,
            source_transaction_name: NativePath::encode(source_transaction_name),
            source_transaction_identity,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawPreparedRecord = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

#[cfg(target_os = "linux")]
impl ClaimIntentRecord {
    pub fn from_prepared(prepared: &PreparedRecord) -> Result<Self, JournalError> {
        validate_source_transaction_name(
            &prepared.transaction_id,
            &prepared.source_transaction_name,
        )?;
        Ok(Self {
            schema_major: prepared.schema_major,
            schema_minor: prepared.schema_minor,
            transaction_id: prepared.transaction_id.clone(),
            phase: JournalPhase::ClaimIntent,
            source_parent: prepared.source_parent.clone(),
            original_name: prepared.original_name.clone(),
            source_parent_identity: prepared.source_parent_identity,
            source_entry_identity: prepared.source_entry_identity,
            source_transaction_name: prepared.source_transaction_name.clone(),
            source_transaction_identity: prepared.source_transaction_identity,
            expected_source_identity: prepared.source_entry_identity,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawClaimIntentRecord = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<RawClaimIntentRecord> for ClaimIntentRecord {
    type Error = JournalError;

    fn try_from(raw: RawClaimIntentRecord) -> Result<Self, Self::Error> {
        if raw.phase != JournalPhase::ClaimIntent {
            return Err(JournalError::InvalidClaimIntentPhase(raw.phase));
        }
        let prepared = PreparedRecord::try_from(RawPreparedRecord {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
            phase: JournalPhase::Prepared,
            source_parent: raw.source_parent,
            original_name: raw.original_name,
            source_parent_identity: raw.source_parent_identity,
            source_entry_identity: raw.source_entry_identity,
            source_transaction_name: raw.source_transaction_name,
            source_transaction_identity: raw.source_transaction_identity,
        })?;
        validate_source_transaction_name(
            &prepared.transaction_id,
            &prepared.source_transaction_name,
        )?;
        if raw.expected_source_identity != prepared.source_entry_identity {
            return Err(JournalError::MismatchedExpectedSourceIdentity);
        }
        Ok(Self {
            schema_major: prepared.schema_major,
            schema_minor: prepared.schema_minor,
            transaction_id: prepared.transaction_id,
            phase: raw.phase,
            source_parent: prepared.source_parent,
            original_name: prepared.original_name,
            source_parent_identity: prepared.source_parent_identity,
            source_entry_identity: prepared.source_entry_identity,
            source_transaction_name: prepared.source_transaction_name,
            source_transaction_identity: prepared.source_transaction_identity,
            expected_source_identity: raw.expected_source_identity,
        })
    }
}

#[cfg(target_os = "linux")]
fn validate_source_transaction_name(
    transaction_id: &str,
    source_transaction_name: &NativePath,
) -> Result<(), JournalError> {
    let expected = PathBuf::from(format!(".ottrin-move-{transaction_id}"));
    if source_transaction_name.decode()? != expected {
        return Err(JournalError::MismatchedSourceTransactionName);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
impl<'de> Deserialize<'de> for ClaimIntentRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawClaimIntentRecord::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<RawPreparedRecord> for PreparedRecord {
    type Error = JournalError;

    fn try_from(raw: RawPreparedRecord) -> Result<Self, Self::Error> {
        JournalHeader::try_from(RawJournalHeader {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id.clone(),
        })?;
        if raw.phase != JournalPhase::Prepared {
            return Err(JournalError::InvalidPreparedPhase(raw.phase));
        }
        let source_parent = raw.source_parent.decode()?;
        let original_name = raw.original_name.decode()?;
        let source_transaction_name = raw.source_transaction_name.decode()?;
        let mut original_components = original_name.components();
        let mut transaction_components = source_transaction_name.components();
        if !source_parent.is_absolute()
            || !matches!(
                original_components.next(),
                Some(std::path::Component::Normal(_))
            )
            || original_components.next().is_some()
            || !matches!(
                transaction_components.next(),
                Some(std::path::Component::Normal(_))
            )
            || transaction_components.next().is_some()
            || source_transaction_name.file_name() != Some(source_transaction_name.as_os_str())
        {
            return Err(JournalError::InvalidPreparedSourcePath);
        }
        if raw.source_parent_identity.file_type != LinuxFileType::Directory
            || raw.source_transaction_identity.file_type != LinuxFileType::Directory
        {
            return Err(JournalError::InvalidLinuxIdentity);
        }
        Ok(Self {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
            phase: raw.phase,
            source_parent: raw.source_parent,
            original_name: raw.original_name,
            source_parent_identity: raw.source_parent_identity,
            source_entry_identity: raw.source_entry_identity,
            source_transaction_name: raw.source_transaction_name,
            source_transaction_identity: raw.source_transaction_identity,
        })
    }
}

#[cfg(target_os = "linux")]
impl<'de> Deserialize<'de> for PreparedRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPreparedRecord::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawJournalHeader> for JournalHeader {
    type Error = JournalError;

    fn try_from(raw: RawJournalHeader) -> Result<Self, Self::Error> {
        if raw.schema_major != JOURNAL_SCHEMA_MAJOR || raw.schema_minor != JOURNAL_SCHEMA_MINOR {
            return Err(JournalError::UnsupportedSchema {
                major: raw.schema_major,
                minor: raw.schema_minor,
            });
        }
        if raw.transaction_id.len() != 32 || decode_hex(&raw.transaction_id).is_err() {
            return Err(JournalError::InvalidTransactionId);
        }
        Ok(Self {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
        })
    }
}

impl<'de> Deserialize<'de> for JournalHeader {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawJournalHeader::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>, JournalError> {
    if !encoded.len().is_multiple_of(2) {
        return Err(JournalError::InvalidNativePathUnits);
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let high = decode_nibble(digits[0])?;
            let low = decode_nibble(digits[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

#[cfg(windows)]
fn decode_u16_hex(digits: &[u8]) -> Result<u16, JournalError> {
    let mut value = 0_u16;
    for digit in digits {
        value = (value << 4) | u16::from(decode_nibble(*digit)?);
    }
    Ok(value)
}

fn decode_nibble(digit: u8) -> Result<u8, JournalError> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        _ => Err(JournalError::InvalidNativePathUnits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn native_path_round_trips_non_utf8_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(b"folder/item\x80.txt".to_vec()));
        let encoded = NativePath::encode(&path);
        assert_eq!(encoded.decode().unwrap(), path);
    }

    #[test]
    fn header_round_trips_and_uses_fixed_width_lowercase_id() {
        let header = JournalHeader::new([0xab; 16]);
        let json = serde_json::to_vec(&header).unwrap();
        let parsed = JournalHeader::parse(&json).unwrap();
        assert_eq!(parsed, header);
        assert_eq!(parsed.transaction_id, "abababababababababababababababab");
    }

    #[test]
    fn unknown_header_fields_are_rejected() {
        let json = br#"{
            "schema_major": 2,
            "schema_minor": 0,
            "transaction_id": "00000000000000000000000000000000",
            "unexpected": true
        }"#;
        assert!(matches!(
            JournalHeader::parse(json),
            Err(JournalError::InvalidJson(_))
        ));
    }

    #[test]
    fn unsupported_major_version_is_rejected() {
        let json = br#"{
            "schema_major": 3,
            "schema_minor": 0,
            "transaction_id": "00000000000000000000000000000000"
        }"#;
        assert_eq!(
            JournalHeader::parse(json),
            Err(JournalError::UnsupportedSchema { major: 3, minor: 0 })
        );
    }

    #[test]
    fn direct_serde_deserialization_enforces_header_invariants() {
        for json in [
            br#"{"schema_major":3,"schema_minor":0,"transaction_id":"00000000000000000000000000000000"}"#.as_slice(),
            br#"{"schema_major":2,"schema_minor":0,"transaction_id":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#.as_slice(),
        ] {
            assert!(serde_json::from_slice::<JournalHeader>(json).is_err());
        }
    }

    #[test]
    fn malformed_transaction_id_is_rejected() {
        for transaction_id in [
            "abc",
            "0000000000000000000000000000000g",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let json = format!(
                r#"{{"schema_major":2,"schema_minor":0,"transaction_id":"{transaction_id}"}}"#
            );
            assert_eq!(
                JournalHeader::parse(json.as_bytes()),
                Err(JournalError::InvalidTransactionId)
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn journal_phases_reject_every_undocumented_edge() {
        let phases = [
            JournalPhase::Prepared,
            JournalPhase::ClaimIntent,
            JournalPhase::Claimed,
            JournalPhase::UnexpectedClaimRetained,
            JournalPhase::PendingDestination,
            JournalPhase::Published,
        ];
        for from in phases {
            for to in phases {
                let documented = from == to
                    || matches!(
                        (from, to),
                        (JournalPhase::Prepared, JournalPhase::ClaimIntent)
                            | (JournalPhase::ClaimIntent, JournalPhase::Claimed)
                            | (
                                JournalPhase::ClaimIntent,
                                JournalPhase::UnexpectedClaimRetained
                            )
                            | (JournalPhase::Claimed, JournalPhase::PendingDestination)
                            | (JournalPhase::PendingDestination, JournalPhase::Published)
                    );
                if !documented {
                    assert_eq!(
                        from.validate_transition(to),
                        Err(JournalError::InvalidPhaseTransition { from, to })
                    );
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_phase_advances_to_durable_claim_intent() {
        assert_eq!(
            JournalPhase::Prepared.validate_transition(JournalPhase::ClaimIntent),
            Ok(())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn journal_phases_accept_the_durable_forward_sequence() {
        for (from, to) in [
            (JournalPhase::ClaimIntent, JournalPhase::Claimed),
            (JournalPhase::Claimed, JournalPhase::PendingDestination),
            (JournalPhase::PendingDestination, JournalPhase::Published),
        ] {
            assert_eq!(from.validate_transition(to), Ok(()));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_can_advance_to_unexpected_claim_retention() {
        assert_eq!(
            JournalPhase::ClaimIntent.validate_transition(JournalPhase::UnexpectedClaimRetained),
            Ok(())
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn journal_phase_can_be_rewritten_without_phase_regression() {
        for phase in [
            JournalPhase::Prepared,
            JournalPhase::ClaimIntent,
            JournalPhase::Claimed,
            JournalPhase::UnexpectedClaimRetained,
            JournalPhase::PendingDestination,
            JournalPhase::Published,
        ] {
            assert_eq!(phase.validate_transition(phase), Ok(()));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_schema_requires_and_round_trips_expected_source_identity() {
        let prepared = PreparedRecord::new_linux(
            [0x42; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-42424242424242424242424242424242"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let record = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        let encoded = serde_json::to_vec(&record).unwrap();
        let parsed = ClaimIntentRecord::parse(&encoded).unwrap();

        assert_eq!(parsed, record);
        assert_eq!(parsed.phase, JournalPhase::ClaimIntent);
        assert_eq!(parsed.source_parent, prepared.source_parent);
        assert_eq!(parsed.original_name, prepared.original_name);
        assert_eq!(
            parsed.source_parent_identity,
            prepared.source_parent_identity
        );
        assert_eq!(parsed.source_entry_identity, prepared.source_entry_identity);
        assert_eq!(
            parsed.source_transaction_name,
            prepared.source_transaction_name
        );
        assert_eq!(
            parsed.source_transaction_identity,
            prepared.source_transaction_identity
        );
        assert_eq!(
            parsed.expected_source_identity,
            prepared.source_entry_identity
        );

        let mut missing_expected: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        missing_expected
            .as_object_mut()
            .unwrap()
            .remove("expected_source_identity");
        assert!(matches!(
            ClaimIntentRecord::parse(&serde_json::to_vec(&missing_expected).unwrap()),
            Err(JournalError::InvalidJson(_))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_record_rejects_phase_inappropriate_evidence() {
        let prepared = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-43434343434343434343434343434343"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let mut encoded =
            serde_json::to_value(ClaimIntentRecord::from_prepared(&prepared).unwrap()).unwrap();
        encoded["phase"] = serde_json::json!("prepared");

        assert_eq!(
            ClaimIntentRecord::parse(&serde_json::to_vec(&encoded).unwrap()),
            Err(JournalError::InvalidClaimIntentPhase(
                JournalPhase::Prepared
            ))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_record_rejects_expected_identity_that_differs_from_prepared_source() {
        let prepared = PreparedRecord::new_linux(
            [0x44; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-44444444444444444444444444444444"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let mut encoded =
            serde_json::to_value(ClaimIntentRecord::from_prepared(&prepared).unwrap()).unwrap();
        encoded["expected_source_identity"]["inode"] = serde_json::json!(34);

        assert_eq!(
            ClaimIntentRecord::parse(&serde_json::to_vec(&encoded).unwrap()),
            Err(JournalError::MismatchedExpectedSourceIdentity)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_record_rejects_transaction_name_for_another_id() {
        let prepared = PreparedRecord::new_linux(
            [0x45; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-unrelated"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();

        assert_eq!(
            ClaimIntentRecord::from_prepared(&prepared),
            Err(JournalError::MismatchedSourceTransactionName)
        );

        let valid_prepared = PreparedRecord::new_linux(
            [0x45; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-45454545454545454545454545454545"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let mut encoded =
            serde_json::to_value(ClaimIntentRecord::from_prepared(&valid_prepared).unwrap())
                .unwrap();
        encoded["source_transaction_name"] =
            serde_json::to_value(NativePath::encode(Path::new(".ottrin-move-unrelated"))).unwrap();
        assert_eq!(
            ClaimIntentRecord::parse(&serde_json::to_vec(&encoded).unwrap()),
            Err(JournalError::MismatchedSourceTransactionName)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_rejects_a_later_phase_without_required_fields() {
        let encoded = serde_json::json!({
            "schema_major": 2,
            "schema_minor": 0,
            "transaction_id": "42424242424242424242424242424242",
            "phase": "claim-intent",
            "source_parent": {"encoding": "unix-bytes", "units_hex": "2f736f75726365"},
            "original_name": {"encoding": "unix-bytes", "units_hex": "6974656d"},
            "source_parent_identity": {"device": 11, "inode": 22, "file_type": "directory"},
            "source_entry_identity": {"device": 11, "inode": 33, "file_type": "regular"},
            "source_transaction_name": {"encoding": "unix-bytes", "units_hex": "2e6f747472696e2d6d6f76652d74657374"},
            "source_transaction_identity": {"device": 11, "inode": 44, "file_type": "directory"}
        });

        assert_eq!(
            PreparedRecord::parse(&serde_json::to_vec(&encoded).unwrap()),
            Err(JournalError::InvalidPreparedPhase(
                JournalPhase::ClaimIntent
            ))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_persists_lossless_linux_source_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let source_parent = PathBuf::from(OsString::from_vec(b"/source\x80".to_vec()));
        let original_name = PathBuf::from(OsString::from_vec(b"item\xff".to_vec()));
        let record = PreparedRecord::new_linux(
            [0x42; 16],
            &source_parent,
            &original_name,
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();

        let encoded = serde_json::to_vec(&record).unwrap();
        let parsed = PreparedRecord::parse(&encoded).unwrap();
        assert_eq!(parsed, record);
        assert_eq!(parsed.phase, JournalPhase::Prepared);
        assert_eq!(parsed.source_parent.decode().unwrap(), source_parent);
        assert_eq!(parsed.original_name.decode().unwrap(), original_name);
        assert_eq!(parsed.source_parent_identity.device, 11);
        assert_eq!(parsed.source_parent_identity.inode, 22);
        assert_eq!(parsed.source_entry_identity.inode, 33);

        let invalid = String::from_utf8(encoded)
            .unwrap()
            .replace("\"file_type\":\"directory\"", "\"file_type\":\"regular\"");
        assert!(serde_json::from_str::<PreparedRecord>(&invalid).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_round_trips_source_transaction_directory_identity() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let transaction_name = PathBuf::from(OsString::from_vec(
            b".ottrin-move-42424242424242424242424242424242\x80".to_vec(),
        ));
        let encoded = serde_json::json!({
            "schema_major": 2,
            "schema_minor": 0,
            "transaction_id": "42424242424242424242424242424242",
            "phase": "prepared",
            "source_parent": {"encoding": "unix-bytes", "units_hex": "2f736f75726365"},
            "original_name": {"encoding": "unix-bytes", "units_hex": "6974656d"},
            "source_parent_identity": {"device": 11, "inode": 22, "file_type": "directory"},
            "source_entry_identity": {"device": 11, "inode": 33, "file_type": "regular"},
            "source_transaction_name": NativePath::encode(&transaction_name),
            "source_transaction_identity": {"device": 11, "inode": 44, "file_type": "directory"}
        });

        let parsed = PreparedRecord::parse(&serde_json::to_vec(&encoded).unwrap()).unwrap();

        assert_eq!(
            parsed.source_transaction_name.decode().unwrap(),
            transaction_name
        );
        assert_eq!(parsed.source_transaction_identity.device, 11);
        assert_eq!(parsed.source_transaction_identity.inode, 44);
        assert_eq!(
            parsed.source_transaction_identity.file_type,
            LinuxFileType::Directory
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_schema_marks_transaction_identity_as_breaking_change() {
        let record = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();

        assert_eq!(record.schema_major, 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_rejects_non_directory_transaction_identity() {
        let result = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Regular),
        );

        assert_eq!(result, Err(JournalError::InvalidLinuxIdentity));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_rejects_transaction_name_with_separator() {
        let result = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-test/"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        );

        assert_eq!(result, Err(JournalError::InvalidPreparedSourcePath));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_rejects_root_as_original_name() {
        let result = PreparedRecord::new_linux(
            [0x43; 16],
            Path::new("/source"),
            Path::new("/"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        );

        assert_eq!(result, Err(JournalError::InvalidPreparedSourcePath));
    }

    #[test]
    fn malformed_native_path_units_are_rejected() {
        for units_hex in ["0", "0x"] {
            let encoded = NativePath {
                #[cfg(unix)]
                encoding: NativePathEncoding::UnixBytes,
                #[cfg(windows)]
                encoding: NativePathEncoding::WindowsUtf16Le,
                #[cfg(not(any(unix, windows)))]
                encoding: NativePathEncoding::UnixBytes,
                units_hex: units_hex.into(),
            };
            assert_eq!(encoded.decode(), Err(JournalError::InvalidNativePathUnits));
        }
    }

    #[test]
    fn foreign_platform_path_encoding_is_rejected() {
        let encoded = NativePath {
            #[cfg(unix)]
            encoding: NativePathEncoding::WindowsUtf16Le,
            #[cfg(windows)]
            encoding: NativePathEncoding::UnixBytes,
            #[cfg(not(any(unix, windows)))]
            encoding: NativePathEncoding::WindowsUtf16Le,
            units_hex: String::new(),
        };
        assert_eq!(encoded.decode(), Err(JournalError::WrongPlatformEncoding));
    }

    #[test]
    fn unknown_native_path_fields_are_rejected() {
        let json = br#"{
            "encoding": "unix-bytes",
            "units_hex": "00",
            "unexpected": true
        }"#;
        assert!(serde_json::from_slice::<NativePath>(json).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn transaction_headers_create_independent_central_discovery_records() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-central-discovery-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();

        let directory = LinuxJournalDirectory::open(&root).unwrap();
        let first = JournalHeader::new([0x12; 16]);
        let second = JournalHeader::new([0x34; 16]);
        directory.persist_discovery_header(&first).unwrap();
        directory.persist_discovery_header(&second).unwrap();

        for (name, expected) in [
            ("12121212121212121212121212121212.journal", first),
            ("34343434343434343434343434343434.journal", second),
        ] {
            let persisted = std::fs::read(root.join(name)).unwrap();
            assert_eq!(JournalHeader::parse(&persisted).unwrap(), expected);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_is_persisted_as_complete_central_discovery_record() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-prepared-discovery-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();

        let record = PreparedRecord::new_linux(
            [0x56; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 11, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 13, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();
        directory.persist_prepared(&record).unwrap();

        let expected_path = root.join("56565656565656565656565656565656.journal");
        let persisted = std::fs::read(expected_path).unwrap();
        assert_eq!(PreparedRecord::parse(&persisted).unwrap(), record);
        assert!(JournalHeader::parse(&persisted).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_discovery_ignores_stale_updates_and_returns_valid_transaction_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-discover-stale-update-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let transaction_id = [0x57; 16];
        let record = PreparedRecord::new_linux(
            transaction_id,
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 11, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 13, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();
        directory.persist_prepared(&record).unwrap();
        let stale_name =
            "57575757575757575757575757575757.journal.next-58585858585858585858585858585858";
        let stale = b"interrupted update";
        std::fs::write(root.join(stale_name), stale).unwrap();
        let journal_path = root.join("57575757575757575757575757575757.journal");
        let journal_before = std::fs::read(&journal_path).unwrap();

        let discovered = directory.discover_prepared();

        assert_eq!(discovered, Ok(vec![transaction_id]));
        assert_eq!(std::fs::read(journal_path).unwrap(), journal_before);
        assert_eq!(std::fs::read(root.join(stale_name)).unwrap(), stale);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_discovery_surfaces_malformed_journal_filename_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-discover-malformed-name-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let name = "not-a-transaction.journal";
        let contents = b"retained malformed record";
        std::fs::write(root.join(name), contents).unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();

        let discovered = directory.discover_prepared();

        assert_eq!(
            discovered,
            Err(LinuxDiscoverPreparedError::InvalidFilename(
                name.as_bytes().to_vec()
            ))
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), contents);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_discovery_reports_malformed_non_utf8_filename_losslessly_without_mutation() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-discover-non-utf8-name-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let name_bytes = b"bad-\x80.journal".to_vec();
        let name = OsString::from_vec(name_bytes.clone());
        let contents = b"retained non-UTF-8 record";
        std::fs::write(root.join(&name), contents).unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();

        let discovered = directory.discover_prepared();

        assert_eq!(
            discovered,
            Err(LinuxDiscoverPreparedError::InvalidFilename(name_bytes))
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), contents);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_discovery_surfaces_malformed_stale_update_name_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-discover-malformed-update-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let name = "57575757575757575757575757575757.journal.next-invalid";
        let contents = b"retained malformed update";
        std::fs::write(root.join(name), contents).unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();

        let discovered = directory.discover_prepared();

        assert_eq!(
            discovered,
            Err(LinuxDiscoverPreparedError::InvalidFilename(
                name.as_bytes().to_vec()
            ))
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), contents);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_discovery_surfaces_filename_record_transaction_mismatch_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-discover-id-mismatch-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let filename_transaction_id = [0x58; 16];
        let record_transaction_id = [0x59; 16];
        let record = PreparedRecord::new_linux(
            record_transaction_id,
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 11, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 13, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let name = "58585858585858585858585858585858.journal";
        let encoded = serde_json::to_vec(&record).unwrap();
        std::fs::write(root.join(name), &encoded).unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();

        let discovered = directory.discover_prepared();

        assert_eq!(
            discovered,
            Err(LinuxDiscoverPreparedError::FilenameTransactionMismatch {
                filename: filename_transaction_id,
                record: record_transaction_id,
            })
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), encoded);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_record_is_persisted_to_anchored_source_local_journal() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-local-prepared-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original = root.join("source-transaction");
        let moved = root.join("moved-source-transaction");
        std::fs::create_dir_all(&original).unwrap();

        let directory = LinuxJournalDirectory::open(&original).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();
        let record = PreparedRecord::new_linux(
            [0x78; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 17, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 19, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        directory.persist_local_prepared(&record).unwrap();

        let persisted = std::fs::read(moved.join("journal")).unwrap();
        assert_eq!(PreparedRecord::parse(&persisted).unwrap(), record);
        assert!(JournalHeader::parse(&persisted).is_err());
        assert!(!original.join("journal").exists());
        assert_eq!(std::fs::read_dir(&moved).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_journals_persist_the_same_record_centrally_and_source_locally() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-prepared-pair-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let record = PreparedRecord::new_linux(
            [0x9a; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 23, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 29, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        let prepared = central.prepare_with_local(&local, &record).unwrap();

        assert_eq!(prepared, LinuxPreparedJournals::MatchingPair);
        for path in [
            central_path.join("9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a.journal"),
            local_path.join("journal"),
        ] {
            let persisted = std::fs::read(path).unwrap();
            assert_eq!(PreparedRecord::parse(&persisted).unwrap(), record);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_failure_reports_a_durable_central_only_preparation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-central-only-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        std::fs::remove_dir(&local_path).unwrap();
        let record = PreparedRecord::new_linux(
            [0xbc; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 31, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 37, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        let result = central.prepare_with_local(&local, &record);

        assert!(matches!(
            result,
            Err(LinuxPrepareJournalsError::CentralOnly(JournalError::Io(_)))
        ));
        let central_record =
            std::fs::read(central_path.join("bcbcbcbcbcbcbcbcbcbcbcbcbcbcbcbc.journal")).unwrap();
        assert_eq!(PreparedRecord::parse(&central_record).unwrap(), record);
        assert!(!local_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_load_rejects_matching_records_for_another_transaction() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-wrong-transaction-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let requested_transaction_id = [0xd1; 16];
        let other_record = PreparedRecord::new_linux(
            [0xd2; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central
            .replace_named("d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1.journal", &other_record)
            .unwrap();
        local.persist_local_prepared(&other_record).unwrap();

        let result = central.load_prepared_pair(&local, requested_transaction_id);

        assert_eq!(
            result,
            Err(LinuxLoadPreparedPairError::RequestedTransactionMismatch)
        );
        assert!(
            central_path
                .join("d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1.journal")
                .exists()
        );
        assert!(local_path.join("journal").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_load_accepts_matching_anchored_records_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-matching-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let transaction_id = [0xd0; 16];
        let record = PreparedRecord::new_linux(
            transaction_id,
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &record).unwrap();
        let central_before =
            std::fs::read(central_path.join("d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0.journal")).unwrap();
        let local_before = std::fs::read(local_path.join("journal")).unwrap();

        let result = central.load_prepared_pair(&local, transaction_id);

        assert_eq!(result, Ok(LinuxPreparedJournals::MatchingPair));
        assert_eq!(
            std::fs::read(central_path.join("d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0.journal")).unwrap(),
            central_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_before
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_load_reports_missing_central_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-missing-central-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let transaction_id = [0xcf; 16];
        let record = PreparedRecord::new_linux(
            transaction_id,
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        local.persist_local_prepared(&record).unwrap();
        let local_before = std::fs::read(local_path.join("journal")).unwrap();

        let result = central.load_prepared_pair(&local, transaction_id);

        assert_eq!(
            result,
            Err(LinuxLoadPreparedPairError::CentralRead(
                LinuxJournalReadError::Missing
            ))
        );
        assert_eq!(std::fs::read_dir(&central_path).unwrap().count(), 0);
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_before
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_load_reports_missing_local_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-missing-local-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let transaction_id = [0xce; 16];
        let record = PreparedRecord::new_linux(
            transaction_id,
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.persist_prepared(&record).unwrap();
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();

        let result = central.load_prepared_pair(&local, transaction_id);

        assert_eq!(
            result,
            Err(LinuxLoadPreparedPairError::LocalRead(
                LinuxJournalReadError::Missing
            ))
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(central_after, central_before);
        assert_eq!(std::fs::read_dir(&local_path).unwrap().count(), 0);
        assert_eq!(
            PreparedRecord::parse(
                &std::fs::read(central_path.join("cececececececececececececececece.journal"))
                    .unwrap()
            )
            .unwrap(),
            record
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_validation_rejects_disagreeing_valid_records() {
        let central = PreparedRecord::new_linux(
            [0xcd; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let local = PreparedRecord::new_linux(
            [0xef; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        let result = validate_prepared_pair(
            &serde_json::to_vec(&central).unwrap(),
            &serde_json::to_vec(&local).unwrap(),
        );

        assert_eq!(result, Err(LinuxPreparedPairError::Mismatch));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_validation_identifies_central_corruption() {
        let local = PreparedRecord::new_linux(
            [0xcd; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        let result = validate_prepared_pair(b"not-json", &serde_json::to_vec(&local).unwrap());

        assert!(matches!(
            result,
            Err(LinuxPreparedPairError::CentralCorrupt(
                JournalError::InvalidJson(_)
            ))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_validation_identifies_local_corruption() {
        let central = PreparedRecord::new_linux(
            [0xcd; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();

        let result = validate_prepared_pair(&serde_json::to_vec(&central).unwrap(), b"not-json");

        assert!(matches!(
            result,
            Err(LinuxPreparedPairError::LocalCorrupt(
                JournalError::InvalidJson(_)
            ))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepared_pair_validation_accepts_matching_valid_records() {
        let record = PreparedRecord::new_linux(
            [0xcd; 16],
            Path::new("/source-parent"),
            Path::new("original-name"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-test"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let encoded = serde_json::to_vec(&record).unwrap();

        let result = validate_prepared_pair(&encoded, &encoded);

        assert_eq!(result, Ok(LinuxPreparedJournals::MatchingPair));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stale_fixed_next_entry_does_not_block_journal_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-journal-stale-next-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let stale = b"interrupted journal update";
        std::fs::write(root.join("journal.next"), stale).unwrap();

        let directory = LinuxJournalDirectory::open(&root).unwrap();
        let header = JournalHeader::new([0x6b; 16]);
        directory.replace(&header).unwrap();

        let persisted = std::fs::read(root.join("journal")).unwrap();
        assert_eq!(JournalHeader::parse(&persisted).unwrap(), header);
        assert_eq!(std::fs::read(root.join("journal.next")).unwrap(), stale);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_is_created_through_anchored_parent_with_verified_identity() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-anchor-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original_parent = root.join("source-parent");
        let moved_parent = root.join("moved-source-parent");
        std::fs::create_dir_all(&original_parent).unwrap();
        let parent = LinuxSourceParentDirectory::open(&original_parent).unwrap();
        std::fs::rename(&original_parent, &moved_parent).unwrap();
        std::fs::create_dir(&original_parent).unwrap();

        let transaction_id = [0xab; 16];
        let transaction = parent.create_transaction(transaction_id).unwrap();

        let expected_name = ".ottrin-move-abababababababababababababababab";
        let created = moved_parent.join(expected_name);
        let metadata = std::fs::symlink_metadata(&created).unwrap();
        assert!(!original_parent.join(expected_name).exists());
        assert!(metadata.is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        assert_eq!(transaction.name(), Path::new(expected_name));
        assert_eq!(
            transaction.observed_identity(),
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Directory)
        );

        assert!(transaction._identity_handle.is_some());
        assert!(!created.join("journal").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_parent_sync_uses_the_anchored_directory_after_path_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-parent-sync-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original_parent = root.join("source-parent");
        let moved_parent = root.join("moved-source-parent");
        std::fs::create_dir_all(&original_parent).unwrap();
        let parent = LinuxSourceParentDirectory::open(&original_parent).unwrap();
        std::fs::rename(&original_parent, &moved_parent).unwrap();
        std::fs::create_dir(&original_parent).unwrap();

        parent.sync().unwrap();

        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_replacement_after_creation_is_only_an_unproven_candidate() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-unproven-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let transaction_name = ".ottrin-move-b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
        let transaction_path = root.join(transaction_name);
        let replacement = root.join("same-euid-replacement");
        std::fs::create_dir(&replacement).unwrap();
        std::fs::write(replacement.join("not-owned-by-ottrin"), b"retain").unwrap();
        let replacement_metadata = std::fs::symlink_metadata(&replacement).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let candidate: LinuxUnprovenSourceTransaction = parent
            .create_transaction_with_post_create([0xb2; 16], || {
                std::fs::remove_dir(&transaction_path).unwrap();
                std::fs::rename(&replacement, &transaction_path).unwrap();
                Ok(())
            })
            .unwrap();

        assert_eq!(candidate.name(), Path::new(transaction_name));
        assert_eq!(
            candidate.observed_identity(),
            LinuxStableIdentity::new(
                replacement_metadata.dev(),
                replacement_metadata.ino(),
                LinuxFileType::Directory,
            )
        );
        assert!(candidate._identity_handle.is_some());
        assert_eq!(
            std::fs::read(transaction_path.join("not-owned-by-ottrin")).unwrap(),
            b"retain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unproven_source_transaction_does_not_chmod_a_replacement() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-unproven-mode-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let transaction_path = root.join(".ottrin-move-b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3");
        let replacement = root.join("same-euid-replacement");
        std::fs::create_dir(&replacement).unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o750)).unwrap();
        let replacement_metadata = std::fs::symlink_metadata(&replacement).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let candidate = parent
            .create_transaction_with_post_create([0xb3; 16], || {
                std::fs::remove_dir(&transaction_path).unwrap();
                std::fs::rename(&replacement, &transaction_path).unwrap();
                Ok(())
            })
            .unwrap();

        assert_eq!(
            candidate.observed_identity(),
            LinuxStableIdentity::new(
                replacement_metadata.dev(),
                replacement_metadata.ino(),
                LinuxFileType::Directory,
            )
        );
        assert_eq!(
            std::fs::symlink_metadata(&transaction_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_reports_retained_name_when_post_create_setup_fails() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-retained-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let result = parent.create_transaction_with_post_create([0xaf; 16], || {
            Err(JournalError::Io("injected post-create failure".to_owned()))
        });

        let Err(LinuxCreateSourceTransactionError::Retained { failure, retained }) = result else {
            panic!("a post-create failure must retain explicit transaction ownership");
        };
        assert_eq!(
            failure,
            JournalError::Io("injected post-create failure".to_owned())
        );
        assert_eq!(
            retained.name(),
            Path::new(".ottrin-move-afafafafafafafafafafafafafafafaf")
        );
        assert!(retained.identity().is_none());
        assert!(root.join(retained.name()).is_dir());
        drop(parent);
        retained.parent_directory().sync().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_retains_identity_handle_separately_after_identity_open_failure() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-retained-identity-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let result = parent.create_transaction_with_hooks(
            [0xb0; 16],
            || Ok(()),
            || {
                Err(JournalError::Io(
                    "injected identity-open failure".to_owned(),
                ))
            },
        );

        let Err(LinuxCreateSourceTransactionError::Retained { failure, retained }) = result else {
            panic!("a post-identity-open failure must retain identity ownership");
        };
        assert_eq!(
            failure,
            JournalError::Io("injected identity-open failure".to_owned())
        );
        assert!(retained.identity().is_some());
        assert!(retained._identity_handle.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unproven_source_transaction_does_not_create_a_journal() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-no-journal-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let candidate = parent.create_transaction([0xb1; 16]).unwrap();

        assert!(candidate._identity_handle.is_some());
        assert!(!root.join(candidate.name()).join("journal").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unproven_source_transaction_preserves_restrictive_umask_result() {
        const CHILD_ENV: &str = "OTTRIN_TEST_RESTRICTIVE_MOVE_UMASK";
        if std::env::var_os(CHILD_ENV).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("move_recovery::tests::unproven_source_transaction_preserves_restrictive_umask_result")
                .arg("--nocapture")
                .env(CHILD_ENV, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }

        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-umask-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();
        rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o777));

        let transaction = parent.create_transaction([0xae; 16]).unwrap();

        let metadata = std::fs::symlink_metadata(root.join(transaction.name())).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o000);
        std::fs::set_permissions(
            root.join(transaction.name()),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_rejects_an_existing_directory_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-existing-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let transaction_id = [0xac; 16];
        let existing = root.join(".ottrin-move-acacacacacacacacacacacacacacacac");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("retained"), b"not Ottrin-owned").unwrap();
        let identity_before = std::fs::symlink_metadata(&existing).unwrap().ino();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let result = parent.create_transaction(transaction_id);

        let Err(error) = result else {
            panic!("an existing transaction name must be rejected");
        };
        assert_eq!(error.to_string(), "source transaction name already exists");
        assert_eq!(
            std::fs::symlink_metadata(&existing).unwrap().ino(),
            identity_before
        );
        assert_eq!(
            std::fs::read(existing.join("retained")).unwrap(),
            b"not Ottrin-owned"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_transaction_rejects_an_existing_symlink_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-source-transaction-symlink-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let transaction_id = [0xad; 16];
        let existing = root.join(".ottrin-move-adadadadadadadadadadadadadadadad");
        let target = root.join("symlink-target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("retained"), b"not Ottrin-owned").unwrap();
        std::os::unix::fs::symlink(&target, &existing).unwrap();
        let link_before = std::fs::read_link(&existing).unwrap();
        let parent = LinuxSourceParentDirectory::open(&root).unwrap();

        let result = parent.create_transaction(transaction_id);

        assert!(matches!(
            result,
            Err(LinuxCreateSourceTransactionError::Creation(
                JournalError::SourceTransactionExists
            ))
        ));
        assert_eq!(std::fs::read_link(&existing).unwrap(), link_before);
        assert_eq!(
            std::fs::read(target.join("retained")).unwrap(),
            b"not Ottrin-owned"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_journal_directory_cannot_be_retargeted_by_path_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-journal-anchor-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let original = root.join("journal-directory");
        let moved = root.join("moved-journal-directory");
        std::fs::create_dir_all(&original).unwrap();

        let directory = LinuxJournalDirectory::open(&original).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();

        let header = JournalHeader::new([0x5a; 16]);
        directory.replace(&header).unwrap();

        let persisted = std::fs::read(moved.join("journal")).unwrap();
        assert_eq!(JournalHeader::parse(&persisted).unwrap(), header);
        assert!(!original.join("journal").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
