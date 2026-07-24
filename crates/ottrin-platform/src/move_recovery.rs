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

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClaimedRecord {
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
    claimed_source_identity: LinuxStableIdentity,
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClaimedRecord {
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
    claimed_source_identity: LinuxStableIdentity,
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
    #[error("claimed journal cannot contain phase {0:?}")]
    InvalidClaimedPhase(JournalPhase),
    #[cfg(target_os = "linux")]
    #[error("claimed source identity does not match the expected source identity")]
    MismatchedClaimedSourceIdentity,
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
/// Exclusive authority for Move recovery and ClaimIntent journal mutation.
///
/// A journal directory cannot advance ClaimIntent without first acquiring this token:
///
/// ```compile_fail
/// use ottrin_platform::move_recovery::{LinuxJournalDirectory, PreparedRecord};
///
/// fn advance_without_lock(
///     central: &LinuxJournalDirectory,
///     local: &LinuxJournalDirectory,
///     prepared: &PreparedRecord,
/// ) {
///     let _ = central.advance_to_claim_intent_with_local(local, prepared);
/// }
/// ```
///
/// ```compile_fail
/// use ottrin_platform::move_recovery::{LinuxJournalDirectory, PreparedRecord};
///
/// fn prepare_without_lock(
///     central: &LinuxJournalDirectory,
///     local: &LinuxJournalDirectory,
///     prepared: &PreparedRecord,
/// ) {
///     let _ = central.prepare_with_local(local, prepared);
/// }
/// ```
#[derive(Debug)]
pub struct LinuxRecoveryLock<'a> {
    central: &'a LinuxJournalDirectory,
    lock: OwnedFd,
}

#[cfg(target_os = "linux")]
impl Drop for LinuxRecoveryLock<'_> {
    fn drop(&mut self) {
        use rustix::fs::{FlockOperation, flock};

        let _ = flock(&self.lock, FlockOperation::Unlock);
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxRecoveryLockError {
    #[error("another recovery context owns the exclusive recovery lock")]
    Contended,
    #[error("recovery lock failed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxCentralRootOpenError {
    #[error(
        "configured recovery-root authority is unavailable until pathname replacement cannot create a second lock domain"
    )]
    StableIdentityAuthorityUnavailable,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxClaimIntentJournals {
    MatchingPair,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxClaimedJournals {
    MatchingPair,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxAdvanceClaimIntentError {
    #[error("prepared journal pair cannot advance: {0}")]
    PreparedPair(LinuxLoadPreparedPairError),
    #[error("claim-intent evidence is invalid: {0}")]
    InvalidClaimIntent(JournalError),
    #[error("central claim-intent update did not confirm durability; inspect the journal: {0}")]
    CentralUpdateUncertain(JournalError),
    #[error("supplied prepared evidence does not match the anchored journal pair")]
    SuppliedPreparedMismatch,
    #[error(
        "central claim-intent journal is durable but the source-local update is uncertain: {error}"
    )]
    CentralDurableLocalUpdateUncertain {
        #[source]
        error: JournalError,
        durable_central: Box<ClaimIntentRecord>,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxAdvanceClaimedError {
    #[error("claim-intent restart evidence is unsafe: {0}")]
    Restart(#[from] LinuxLoadClaimIntentRestartError),
    #[error("claimed restart evidence is unsafe: {0}")]
    ClaimedRestart(#[from] LinuxLoadClaimedRestartError),
    #[error("central claimed restart journal could not be read: {0}")]
    CentralClaimedRead(LinuxJournalReadError),
    #[error("source-local claim-intent journal has not reached the durable central state")]
    JournalsNotMatching,
    #[error("claimed topology could not be observed safely: {0}")]
    Observe(LinuxObserveClaimedError),
    #[error("claimed topology does not contain the expected source identity: {0:?}")]
    UnsafeTopology(LinuxClaimedTopology),
    #[error("claimed evidence is invalid: {0}")]
    InvalidClaimed(JournalError),
    #[error(
        "central claimed update was rejected before rename; the exact claim-intent pair remains durable: {error}"
    )]
    CentralUpdateRejected {
        #[source]
        error: JournalError,
        durable_central_claim_intent: Box<ClaimIntentRecord>,
        durable_local_claim_intent: Box<ClaimIntentRecord>,
    },
    #[error("central claimed update was renamed but directory durability is uncertain: {0}")]
    CentralUpdateUncertain(JournalError),
    #[error("central claimed journal is durable but the source-local update is uncertain: {error}")]
    CentralDurableLocalUpdateUncertain {
        #[source]
        error: JournalError,
        durable_central: Box<ClaimedRecord>,
    },
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
#[derive(Debug, Eq, PartialEq)]
enum LinuxJournalReplaceError {
    BeforeRename(JournalError),
    AfterRename(JournalError),
}

#[cfg(target_os = "linux")]
impl LinuxJournalReplaceError {
    fn into_inner(self) -> JournalError {
        match self {
            Self::BeforeRename(error) | Self::AfterRename(error) => error,
        }
    }
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinuxClaimIntentRestartState {
    MatchingPair(ClaimIntentRecord),
    CentralClaimIntentLocalPrepared {
        claim_intent: ClaimIntentRecord,
        prepared: PreparedRecord,
    },
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinuxClaimedRestartState {
    MatchingPair {
        central: ClaimedRecord,
        local: ClaimedRecord,
    },
    CentralClaimedLocalClaimIntent {
        claimed: ClaimedRecord,
        claim_intent: ClaimIntentRecord,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxLoadClaimedRestartError {
    #[error("central claimed journal could not be read: {0}")]
    CentralRead(LinuxJournalReadError),
    #[error("source-local recovery journal could not be read: {0}")]
    LocalRead(LinuxJournalReadError),
    #[error("central claimed journal is invalid: {0}")]
    CentralCorrupt(JournalError),
    #[error(
        "source-local journal is neither a valid claimed ({claimed}) nor claim-intent record ({claim_intent})"
    )]
    LocalCorrupt {
        claimed: JournalError,
        claim_intent: JournalError,
    },
    #[error(
        "central recovery journal transaction {recorded:?} does not match requested transaction {requested:?}"
    )]
    RequestedTransactionMismatch {
        requested: [u8; 16],
        recorded: [u8; 16],
    },
    #[error("source-local claimed transaction does not match the central transaction")]
    ClaimedTransactionMismatch {
        durable_central: Box<ClaimedRecord>,
        local: Box<ClaimedRecord>,
    },
    #[error("source-local claim-intent transaction does not match the central transaction")]
    ClaimIntentTransactionMismatch {
        durable_central: Box<ClaimedRecord>,
        local: Box<ClaimIntentRecord>,
    },
    #[error("source-local claimed evidence disagrees with the durable central claimed journal")]
    ClaimedEvidenceMismatch {
        durable_central: Box<ClaimedRecord>,
        local: Box<ClaimedRecord>,
    },
    #[error(
        "source-local claim-intent evidence disagrees with the durable central claimed journal"
    )]
    ClaimIntentEvidenceMismatch {
        durable_central: Box<ClaimedRecord>,
        local: Box<ClaimIntentRecord>,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxLoadClaimIntentRestartError {
    #[error("central claim-intent journal could not be read: {0}")]
    CentralRead(LinuxJournalReadError),
    #[error("source-local recovery journal could not be read: {0}")]
    LocalRead(LinuxJournalReadError),
    #[error("central claim-intent journal is invalid: {0}")]
    CentralCorrupt(JournalError),
    #[error(
        "source-local journal is neither a valid claim-intent ({claim_intent}) nor prepared record ({prepared})"
    )]
    LocalCorrupt {
        claim_intent: JournalError,
        prepared: JournalError,
    },
    #[error("source-local transaction does not match the central transaction")]
    LocalTransactionMismatch { central: [u8; 16], local: [u8; 16] },
    #[error("source-local prepared evidence disagrees with the durable central claim-intent")]
    PreparedEvidenceMismatch {
        durable_central: Box<ClaimIntentRecord>,
        local: Box<PreparedRecord>,
    },
    #[error("source-local claim-intent evidence disagrees with the durable central claim-intent")]
    ClaimIntentEvidenceMismatch {
        durable_central: Box<ClaimIntentRecord>,
        local: Box<ClaimIntentRecord>,
    },
    #[error(
        "central recovery journal transaction {recorded:?} does not match requested transaction {requested:?}"
    )]
    RequestedTransactionMismatch {
        requested: [u8; 16],
        recorded: [u8; 16],
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxResumeClaimIntentError {
    #[error("claim-intent restart evidence is unsafe: {0}")]
    Restart(#[from] LinuxLoadClaimIntentRestartError),
    #[error(
        "central claim-intent is durable but the source-local restart update is uncertain: {error}"
    )]
    LocalUpdateUncertain {
        #[source]
        error: JournalError,
        durable_central: Box<ClaimIntentRecord>,
    },
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinuxClaimedTopology {
    Missing {
        expected: LinuxStableIdentity,
    },
    ExpectedIdentity {
        original_name: PathBuf,
        identity: LinuxStableIdentity,
    },
    UnexpectedIdentity {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxObserveClaimedError {
    #[error("claim-intent restart evidence is unsafe: {0}")]
    Restart(#[from] LinuxLoadClaimIntentRestartError),
    #[error("source-local claim-intent journal has not reached the durable central state")]
    JournalsNotMatching,
    #[error(
        "source transaction identity differs from the durable claim-intent: expected {expected:?}, observed {observed:?}"
    )]
    TransactionIdentityMismatch {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
    #[error("durable original-source name is invalid: {0}")]
    InvalidOriginalName(JournalError),
    #[error("claimed topology could not be observed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LinuxOriginalTopology {
    Missing {
        expected: LinuxStableIdentity,
    },
    ExpectedIdentity(LinuxStableIdentity),
    UnexpectedIdentity {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxObserveOriginalError {
    #[error("claim-intent restart evidence is unsafe: {0}")]
    Restart(#[from] LinuxLoadClaimIntentRestartError),
    #[error("source-local claim-intent journal has not reached the durable central state")]
    JournalsNotMatching,
    #[error(
        "source transaction identity differs from the durable claim-intent: expected {expected:?}, observed {observed:?}"
    )]
    TransactionIdentityMismatch {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
    #[error(
        "source parent identity differs from the durable claim-intent: expected {expected:?}, observed {observed:?}"
    )]
    SourceParentIdentityMismatch {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
    #[error("durable original-source name is invalid: {0}")]
    InvalidOriginalName(JournalError),
    #[error("original-source topology could not be observed: {0}")]
    Io(String),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Error, Eq, PartialEq)]
pub enum LinuxClaimSourceError {
    #[error("claim-intent restart evidence is unsafe: {0}")]
    Restart(#[from] LinuxLoadClaimIntentRestartError),
    #[error("source-local claim-intent journal has not reached the durable central state")]
    JournalsNotMatching,
    #[error(
        "source transaction identity differs from the durable claim-intent: expected {expected:?}, observed {observed:?}"
    )]
    TransactionIdentityMismatch {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
    #[error(
        "source transaction is no longer placed at its durable name: expected {expected:?}, observed {observed:?}"
    )]
    TransactionPlacementMismatch {
        expected: LinuxStableIdentity,
        observed: Option<LinuxStableIdentity>,
    },
    #[error("source was renamed through a transaction whose durable placement changed")]
    PostRenameTransactionPlacementMismatch {
        expected: LinuxStableIdentity,
        observed: Option<LinuxStableIdentity>,
        local_sync_error: Option<String>,
        claimed: Box<Result<LinuxClaimedTopology, LinuxObserveClaimedError>>,
    },
    #[error(
        "source parent identity differs from the durable claim-intent: expected {expected:?}, observed {observed:?}"
    )]
    SourceParentIdentityMismatch {
        expected: LinuxStableIdentity,
        observed: LinuxStableIdentity,
    },
    #[error("durable original-source name is invalid: {0}")]
    InvalidOriginalName(JournalError),
    #[error("durable source-transaction name is invalid: {0}")]
    InvalidTransactionName(JournalError),
    #[error("claimed name is already occupied")]
    ClaimedNameOccupied,
    #[error("source claim failed: {0}")]
    Rename(String),
    #[error("source was renamed but durability or post-claim topology is uncertain")]
    PostRenameUncertain {
        local_sync_error: Option<String>,
        source_parent_sync_error: Option<String>,
        claimed: Result<LinuxClaimedTopology, LinuxObserveClaimedError>,
    },
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
    pub fn open_configured(_path: &Path) -> Result<Self, LinuxCentralRootOpenError> {
        Err(LinuxCentralRootOpenError::StableIdentityAuthorityUnavailable)
    }

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

    pub fn try_lock_recovery(&self) -> Result<LinuxRecoveryLock<'_>, LinuxRecoveryLockError> {
        use rustix::fs::{FlockOperation, Mode, OFlags, flock, openat};

        let lock = openat(
            &self.directory,
            ".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| LinuxRecoveryLockError::Io(error.to_string()))?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::AGAIN {
                LinuxRecoveryLockError::Contended
            } else {
                LinuxRecoveryLockError::Io(error.to_string())
            }
        })?;
        Ok(LinuxRecoveryLock {
            central: self,
            lock,
        })
    }

    #[cfg(test)]
    fn replace(&self, header: &JournalHeader) -> Result<(), JournalError> {
        self.replace_named("journal", header)
    }

    #[cfg(test)]
    fn persist_discovery_header(&self, header: &JournalHeader) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", header.transaction_id), header)
    }

    fn persist_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", record.transaction_id), record)
    }

    fn persist_local_prepared(&self, record: &PreparedRecord) -> Result<(), JournalError> {
        self.replace_named("journal", record)
    }

    fn persist_claim_intent(&self, record: &ClaimIntentRecord) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", record.transaction_id), record)
    }

    fn persist_local_claim_intent(&self, record: &ClaimIntentRecord) -> Result<(), JournalError> {
        self.replace_named("journal", record)
    }

    #[cfg(test)]
    fn persist_claimed(&self, record: &ClaimedRecord) -> Result<(), JournalError> {
        self.replace_named(&format!("{}.journal", record.transaction_id), record)
    }

    fn persist_claimed_staged(
        &self,
        record: &ClaimedRecord,
    ) -> Result<(), LinuxJournalReplaceError> {
        use rustix::fs::fsync;

        self.replace_named_staged_with_sync(
            &format!("{}.journal", record.transaction_id),
            record,
            || fsync(&self.directory).map_err(|error| JournalError::Io(error.to_string())),
        )
    }

    fn persist_local_claimed(&self, record: &ClaimedRecord) -> Result<(), JournalError> {
        self.replace_named("journal", record)
    }

    fn prepare_with_local(
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
        self.load_prepared_record_pair(local, transaction_id)
            .map(|_| LinuxPreparedJournals::MatchingPair)
    }

    fn load_prepared_record_pair(
        &self,
        local: &Self,
        transaction_id: [u8; 16],
    ) -> Result<PreparedRecord, LinuxLoadPreparedPairError> {
        let central = self
            .read_named(&format!("{}.journal", encode_hex(&transaction_id)))
            .map_err(LinuxLoadPreparedPairError::CentralRead)?;
        let local = local
            .read_named("journal")
            .map_err(LinuxLoadPreparedPairError::LocalRead)?;
        validate_prepared_pair(&central, &local)
            .map_err(LinuxLoadPreparedPairError::InvalidPair)?;
        let central_record = PreparedRecord::parse(&central).map_err(|error| {
            LinuxLoadPreparedPairError::InvalidPair(LinuxPreparedPairError::CentralCorrupt(error))
        })?;
        if central_record.transaction_id != encode_hex(&transaction_id) {
            return Err(LinuxLoadPreparedPairError::RequestedTransactionMismatch);
        }
        Ok(central_record)
    }

    pub fn load_claim_intent_restart_state(
        &self,
        local: &Self,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimIntentRestartState, LinuxLoadClaimIntentRestartError> {
        let central = self
            .read_named(&format!("{}.journal", encode_hex(&transaction_id)))
            .map_err(LinuxLoadClaimIntentRestartError::CentralRead)?;
        let local = local
            .read_named("journal")
            .map_err(LinuxLoadClaimIntentRestartError::LocalRead)?;
        let claim_intent = ClaimIntentRecord::parse(&central)
            .map_err(LinuxLoadClaimIntentRestartError::CentralCorrupt)?;
        let decode_transaction_id = |encoded: &str| {
            let decoded = decode_hex(encoded)
                .expect("validated recovery records have hexadecimal transaction IDs");
            let mut transaction_id = [0_u8; 16];
            transaction_id.copy_from_slice(&decoded);
            transaction_id
        };
        let central_transaction_id = decode_transaction_id(&claim_intent.transaction_id);
        if central_transaction_id != transaction_id {
            return Err(
                LinuxLoadClaimIntentRestartError::RequestedTransactionMismatch {
                    requested: transaction_id,
                    recorded: central_transaction_id,
                },
            );
        }
        match ClaimIntentRecord::parse(&local) {
            Ok(local_claim_intent) => {
                let local_transaction_id =
                    decode_transaction_id(&local_claim_intent.transaction_id);
                if local_transaction_id != transaction_id {
                    return Err(LinuxLoadClaimIntentRestartError::LocalTransactionMismatch {
                        central: transaction_id,
                        local: local_transaction_id,
                    });
                }
                if local_claim_intent == claim_intent {
                    Ok(LinuxClaimIntentRestartState::MatchingPair(claim_intent))
                } else {
                    Err(
                        LinuxLoadClaimIntentRestartError::ClaimIntentEvidenceMismatch {
                            durable_central: Box::new(claim_intent),
                            local: Box::new(local_claim_intent),
                        },
                    )
                }
            }
            Err(claim_intent_error) => match PreparedRecord::parse(&local) {
                Ok(prepared) => {
                    let local_transaction_id = decode_transaction_id(&prepared.transaction_id);
                    if local_transaction_id != transaction_id {
                        return Err(LinuxLoadClaimIntentRestartError::LocalTransactionMismatch {
                            central: transaction_id,
                            local: local_transaction_id,
                        });
                    }
                    if ClaimIntentRecord::from_prepared(&prepared).as_ref() != Ok(&claim_intent) {
                        return Err(LinuxLoadClaimIntentRestartError::PreparedEvidenceMismatch {
                            durable_central: Box::new(claim_intent),
                            local: Box::new(prepared),
                        });
                    }
                    Ok(
                        LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared {
                            claim_intent,
                            prepared,
                        },
                    )
                }
                Err(prepared_error) => Err(LinuxLoadClaimIntentRestartError::LocalCorrupt {
                    claim_intent: claim_intent_error,
                    prepared: prepared_error,
                }),
            },
        }
    }

    pub fn load_claimed_restart_state(
        &self,
        local: &Self,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimedRestartState, LinuxLoadClaimedRestartError> {
        let central = self
            .read_named(&format!("{}.journal", encode_hex(&transaction_id)))
            .map_err(LinuxLoadClaimedRestartError::CentralRead)?;
        let local = local
            .read_named("journal")
            .map_err(LinuxLoadClaimedRestartError::LocalRead)?;
        let claimed =
            ClaimedRecord::parse(&central).map_err(LinuxLoadClaimedRestartError::CentralCorrupt)?;
        let decode_transaction_id = |encoded: &str| {
            let decoded = decode_hex(encoded)
                .expect("validated recovery records have hexadecimal transaction IDs");
            let mut decoded_transaction_id = [0_u8; 16];
            decoded_transaction_id.copy_from_slice(&decoded);
            decoded_transaction_id
        };
        let central_transaction_id = decode_transaction_id(&claimed.transaction_id);
        if central_transaction_id != transaction_id {
            return Err(LinuxLoadClaimedRestartError::RequestedTransactionMismatch {
                requested: transaction_id,
                recorded: central_transaction_id,
            });
        }
        match ClaimedRecord::parse(&local) {
            Ok(local_claimed) => {
                let local_transaction_id = decode_transaction_id(&local_claimed.transaction_id);
                if local_transaction_id != transaction_id {
                    return Err(LinuxLoadClaimedRestartError::ClaimedTransactionMismatch {
                        durable_central: Box::new(claimed),
                        local: Box::new(local_claimed),
                    });
                }
                if local_claimed != claimed {
                    return Err(LinuxLoadClaimedRestartError::ClaimedEvidenceMismatch {
                        durable_central: Box::new(claimed),
                        local: Box::new(local_claimed),
                    });
                }
                Ok(LinuxClaimedRestartState::MatchingPair {
                    central: claimed,
                    local: local_claimed,
                })
            }
            Err(claimed_error) => match ClaimIntentRecord::parse(&local) {
                Ok(claim_intent) => {
                    let local_transaction_id = decode_transaction_id(&claim_intent.transaction_id);
                    if local_transaction_id != transaction_id {
                        return Err(
                            LinuxLoadClaimedRestartError::ClaimIntentTransactionMismatch {
                                durable_central: Box::new(claimed),
                                local: Box::new(claim_intent),
                            },
                        );
                    }
                    if ClaimedRecord::from_claim_intent(
                        &claim_intent,
                        claimed.claimed_source_identity,
                    )
                    .as_ref()
                        != Ok(&claimed)
                    {
                        return Err(LinuxLoadClaimedRestartError::ClaimIntentEvidenceMismatch {
                            durable_central: Box::new(claimed),
                            local: Box::new(claim_intent),
                        });
                    }
                    Ok(LinuxClaimedRestartState::CentralClaimedLocalClaimIntent {
                        claimed,
                        claim_intent,
                    })
                }
                Err(claim_intent_error) => Err(LinuxLoadClaimedRestartError::LocalCorrupt {
                    claimed: claimed_error,
                    claim_intent: claim_intent_error,
                }),
            },
        }
    }

    pub fn discover_prepared(&self) -> Result<Vec<[u8; 16]>, LinuxDiscoverPreparedError> {
        self.discover_pending()
    }

    pub fn discover_pending(&self) -> Result<Vec<[u8; 16]>, LinuxDiscoverPreparedError> {
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
            let record_transaction_id_hex = match PreparedRecord::parse(&encoded) {
                Ok(record) => record.transaction_id,
                Err(prepared_error) => match ClaimIntentRecord::parse(&encoded) {
                    Ok(record) => record.transaction_id,
                    Err(_) => match ClaimedRecord::parse(&encoded) {
                        Ok(record) => record.transaction_id,
                        Err(_) => {
                            return Err(LinuxDiscoverPreparedError::InvalidRecord {
                                name: name.to_owned(),
                                error: prepared_error,
                            });
                        }
                    },
                },
            };
            let filename_decoded =
                decode_hex(std::str::from_utf8(stem).expect("validated journal stems are ASCII"))
                    .expect("validated journal stems are hexadecimal");
            let mut filename_transaction_id = [0_u8; 16];
            filename_transaction_id.copy_from_slice(&filename_decoded);
            let record_decoded = decode_hex(&record_transaction_id_hex)
                .expect("validated recovery records have hexadecimal transaction IDs");
            let mut record_transaction_id = [0_u8; 16];
            record_transaction_id.copy_from_slice(&record_decoded);
            if record_transaction_id_hex.as_bytes() != stem {
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
        use rustix::fs::fsync;

        self.replace_named_staged_with_sync(name, record, || {
            fsync(&self.directory).map_err(|error| JournalError::Io(error.to_string()))
        })
        .map_err(LinuxJournalReplaceError::into_inner)
    }

    fn replace_named_staged_with_sync<T, F>(
        &self,
        name: &str,
        record: &T,
        sync_directory: F,
    ) -> Result<(), LinuxJournalReplaceError>
    where
        T: Serialize,
        F: FnOnce() -> Result<(), JournalError>,
    {
        use rustix::fs::{Mode, OFlags, openat, renameat};
        use std::fs::File;
        use std::io::Write;

        let encoded = serde_json::to_vec(record).map_err(|error| {
            LinuxJournalReplaceError::BeforeRename(JournalError::InvalidJson(error.to_string()))
        })?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| {
            LinuxJournalReplaceError::BeforeRename(JournalError::Io(error.to_string()))
        })?;
        let next_name = format!("{name}.next-{}", encode_hex(&random));
        let next = openat(
            &self.directory,
            next_name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| {
            LinuxJournalReplaceError::BeforeRename(JournalError::Io(error.to_string()))
        })?;
        let mut next = File::from(next);
        next.write_all(&encoded)
            .and_then(|()| next.sync_all())
            .map_err(|error| {
                LinuxJournalReplaceError::BeforeRename(JournalError::Io(error.to_string()))
            })?;
        renameat(&self.directory, next_name.as_str(), &self.directory, name).map_err(|error| {
            LinuxJournalReplaceError::BeforeRename(JournalError::Io(error.to_string()))
        })?;
        sync_directory().map_err(LinuxJournalReplaceError::AfterRename)
    }
}

#[cfg(target_os = "linux")]
impl LinuxRecoveryLock<'_> {
    pub fn prepare_with_local(
        &mut self,
        local: &LinuxJournalDirectory,
        record: &PreparedRecord,
    ) -> Result<LinuxPreparedJournals, LinuxPrepareJournalsError> {
        self.central.prepare_with_local(local, record)
    }

    pub fn resume_claim_intent_with_local(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimIntentJournals, LinuxResumeClaimIntentError> {
        match self
            .central
            .load_claim_intent_restart_state(local, transaction_id)?
        {
            LinuxClaimIntentRestartState::MatchingPair(_) => {}
            LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared {
                claim_intent,
                prepared: _,
            } => {
                local
                    .persist_local_claim_intent(&claim_intent)
                    .map_err(|error| LinuxResumeClaimIntentError::LocalUpdateUncertain {
                        error,
                        durable_central: Box::new(claim_intent),
                    })?;
            }
        }
        Ok(LinuxClaimIntentJournals::MatchingPair)
    }

    pub fn advance_to_claimed_with_local(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimedJournals, LinuxAdvanceClaimedError> {
        self.advance_to_claimed_with_local_inner(
            local,
            transaction_id,
            || {},
            || {},
            None::<fn() -> Result<(), JournalError>>,
        )
    }

    #[cfg(test)]
    fn advance_to_claimed_with_local_after_central<F>(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        after_central: F,
    ) -> Result<LinuxClaimedJournals, LinuxAdvanceClaimedError>
    where
        F: FnOnce(),
    {
        self.advance_to_claimed_with_local_inner(
            local,
            transaction_id,
            after_central,
            || {},
            None::<fn() -> Result<(), JournalError>>,
        )
    }

    #[cfg(test)]
    fn advance_to_claimed_with_local_after_local<F>(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        after_local: F,
    ) -> Result<LinuxClaimedJournals, LinuxAdvanceClaimedError>
    where
        F: FnOnce(),
    {
        self.advance_to_claimed_with_local_inner(
            local,
            transaction_id,
            || {},
            after_local,
            None::<fn() -> Result<(), JournalError>>,
        )
    }

    #[cfg(test)]
    fn advance_to_claimed_with_local_with_central_sync<F>(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        central_sync: F,
    ) -> Result<LinuxClaimedJournals, LinuxAdvanceClaimedError>
    where
        F: FnOnce() -> Result<(), JournalError>,
    {
        self.advance_to_claimed_with_local_inner(
            local,
            transaction_id,
            || {},
            || {},
            Some(central_sync),
        )
    }

    fn advance_to_claimed_with_local_inner<F, G, H>(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        after_central: F,
        after_local: G,
        central_sync: Option<H>,
    ) -> Result<LinuxClaimedJournals, LinuxAdvanceClaimedError>
    where
        F: FnOnce(),
        G: FnOnce(),
        H: FnOnce() -> Result<(), JournalError>,
    {
        let mut after_central = Some(after_central);
        let mut after_local = Some(after_local);
        let central_name = format!("{}.journal", encode_hex(&transaction_id));
        let central_bytes = self
            .central
            .read_named(&central_name)
            .map_err(LinuxAdvanceClaimedError::CentralClaimedRead)?;
        if ClaimedRecord::parse(&central_bytes).is_ok() {
            return match self
                .central
                .load_claimed_restart_state(local, transaction_id)?
            {
                LinuxClaimedRestartState::MatchingPair { .. } => {
                    Ok(LinuxClaimedJournals::MatchingPair)
                }
                LinuxClaimedRestartState::CentralClaimedLocalClaimIntent { claimed, .. } => {
                    match central_sync {
                        Some(sync_directory) => sync_directory(),
                        None => rustix::fs::fsync(&self.central.directory)
                            .map_err(|error| JournalError::Io(error.to_string())),
                    }
                    .map_err(LinuxAdvanceClaimedError::CentralUpdateUncertain)?;
                    after_central
                        .take()
                        .expect("claimed durability callback runs at most once")(
                    );
                    local.persist_local_claimed(&claimed).map_err(|error| {
                        LinuxAdvanceClaimedError::CentralDurableLocalUpdateUncertain {
                            error,
                            durable_central: Box::new(claimed),
                        }
                    })?;
                    after_local
                        .take()
                        .expect("claimed local durability callback runs at most once")(
                    );
                    Ok(LinuxClaimedJournals::MatchingPair)
                }
            };
        }
        let claim_intent = match self
            .central
            .load_claim_intent_restart_state(local, transaction_id)?
        {
            LinuxClaimIntentRestartState::MatchingPair(record) => record,
            LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared { .. } => {
                return Err(LinuxAdvanceClaimedError::JournalsNotMatching);
            }
        };
        let claimed_identity = match self
            .observe_claimed_after_restart(local, transaction_id)
            .map_err(LinuxAdvanceClaimedError::Observe)?
        {
            LinuxClaimedTopology::ExpectedIdentity { identity, .. } => identity,
            topology => return Err(LinuxAdvanceClaimedError::UnsafeTopology(topology)),
        };
        let claimed = ClaimedRecord::from_claim_intent(&claim_intent, claimed_identity)
            .map_err(LinuxAdvanceClaimedError::InvalidClaimed)?;
        let central_persisted = match central_sync {
            Some(sync_directory) => self.central.replace_named_staged_with_sync(
                &format!("{}.journal", claimed.transaction_id),
                &claimed,
                sync_directory,
            ),
            None => self.central.persist_claimed_staged(&claimed),
        };
        central_persisted.map_err(|failure| match failure {
            LinuxJournalReplaceError::BeforeRename(error) => {
                LinuxAdvanceClaimedError::CentralUpdateRejected {
                    error,
                    durable_central_claim_intent: Box::new(claim_intent.clone()),
                    durable_local_claim_intent: Box::new(claim_intent),
                }
            }
            LinuxJournalReplaceError::AfterRename(error) => {
                LinuxAdvanceClaimedError::CentralUpdateUncertain(error)
            }
        })?;
        after_central
            .take()
            .expect("claimed durability callback runs at most once")();
        local.persist_local_claimed(&claimed).map_err(|error| {
            LinuxAdvanceClaimedError::CentralDurableLocalUpdateUncertain {
                error,
                durable_central: Box::new(claimed),
            }
        })?;
        after_local
            .take()
            .expect("claimed local durability callback runs at most once")();
        Ok(LinuxClaimedJournals::MatchingPair)
    }

    pub fn observe_claimed_after_restart(
        &mut self,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimedTopology, LinuxObserveClaimedError> {
        use rustix::fs::{FileType, Mode, OFlags, fstat, openat};

        let (expected, expected_transaction, original_name) = match self
            .central
            .load_claim_intent_restart_state(local, transaction_id)?
        {
            LinuxClaimIntentRestartState::MatchingPair(record) => (
                record.expected_source_identity,
                record.source_transaction_identity,
                record.original_name,
            ),
            LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared { .. } => {
                return Err(LinuxObserveClaimedError::JournalsNotMatching);
            }
        };
        let transaction_stat = fstat(&local.directory)
            .map_err(|error| LinuxObserveClaimedError::Io(error.to_string()))?;
        let observed_transaction = LinuxStableIdentity::new(
            transaction_stat.st_dev,
            transaction_stat.st_ino,
            LinuxFileType::Directory,
        );
        if observed_transaction != expected_transaction {
            return Err(LinuxObserveClaimedError::TransactionIdentityMismatch {
                expected: expected_transaction,
                observed: observed_transaction,
            });
        }
        let original_name = original_name
            .decode()
            .map_err(LinuxObserveClaimedError::InvalidOriginalName)?;
        let claimed = match openat(
            &local.directory,
            "claimed",
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(claimed) => claimed,
            Err(error) if error == rustix::io::Errno::NOENT => {
                return Ok(LinuxClaimedTopology::Missing { expected });
            }
            Err(error) => return Err(LinuxObserveClaimedError::Io(error.to_string())),
        };
        let stat =
            fstat(&claimed).map_err(|error| LinuxObserveClaimedError::Io(error.to_string()))?;
        let file_type = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => LinuxFileType::Regular,
            FileType::Directory => LinuxFileType::Directory,
            FileType::Symlink => LinuxFileType::Symlink,
            _ => LinuxFileType::Other,
        };
        let observed = LinuxStableIdentity::new(stat.st_dev, stat.st_ino, file_type);
        if observed == expected {
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name,
                identity: observed,
            })
        } else {
            Ok(LinuxClaimedTopology::UnexpectedIdentity { expected, observed })
        }
    }

    pub fn observe_original_after_restart(
        &mut self,
        source_parent: &LinuxSourceParentDirectory,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
    ) -> Result<LinuxOriginalTopology, LinuxObserveOriginalError> {
        use rustix::fs::{FileType, Mode, OFlags, fstat, openat};

        let (expected_parent, expected_transaction, expected, original_name) = match self
            .central
            .load_claim_intent_restart_state(local, transaction_id)?
        {
            LinuxClaimIntentRestartState::MatchingPair(record) => (
                record.source_parent_identity,
                record.source_transaction_identity,
                record.expected_source_identity,
                record.original_name,
            ),
            LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared { .. } => {
                return Err(LinuxObserveOriginalError::JournalsNotMatching);
            }
        };
        let transaction_stat = fstat(&local.directory)
            .map_err(|error| LinuxObserveOriginalError::Io(error.to_string()))?;
        let observed_transaction = LinuxStableIdentity::new(
            transaction_stat.st_dev,
            transaction_stat.st_ino,
            LinuxFileType::Directory,
        );
        if observed_transaction != expected_transaction {
            return Err(LinuxObserveOriginalError::TransactionIdentityMismatch {
                expected: expected_transaction,
                observed: observed_transaction,
            });
        }
        let parent_stat = fstat(&source_parent.directory)
            .map_err(|error| LinuxObserveOriginalError::Io(error.to_string()))?;
        let observed_parent = LinuxStableIdentity::new(
            parent_stat.st_dev,
            parent_stat.st_ino,
            LinuxFileType::Directory,
        );
        if observed_parent != expected_parent {
            return Err(LinuxObserveOriginalError::SourceParentIdentityMismatch {
                expected: expected_parent,
                observed: observed_parent,
            });
        }
        let original_name = original_name
            .decode()
            .map_err(LinuxObserveOriginalError::InvalidOriginalName)?;
        let original = match openat(
            &source_parent.directory,
            &original_name,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(original) => original,
            Err(error) if error == rustix::io::Errno::NOENT => {
                return Ok(LinuxOriginalTopology::Missing { expected });
            }
            Err(error) => return Err(LinuxObserveOriginalError::Io(error.to_string())),
        };
        let stat =
            fstat(&original).map_err(|error| LinuxObserveOriginalError::Io(error.to_string()))?;
        let file_type = match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => LinuxFileType::Regular,
            FileType::Directory => LinuxFileType::Directory,
            FileType::Symlink => LinuxFileType::Symlink,
            _ => LinuxFileType::Other,
        };
        let observed = LinuxStableIdentity::new(stat.st_dev, stat.st_ino, file_type);
        if observed == expected {
            Ok(LinuxOriginalTopology::ExpectedIdentity(observed))
        } else {
            Ok(LinuxOriginalTopology::UnexpectedIdentity { expected, observed })
        }
    }

    pub fn claim_source_after_intent(
        &mut self,
        source_parent: &LinuxSourceParentDirectory,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
    ) -> Result<LinuxClaimedTopology, LinuxClaimSourceError> {
        use rustix::fs::fsync;

        self.claim_source_after_intent_with_sync(
            source_parent,
            local,
            transaction_id,
            || fsync(&local.directory).map_err(|error| error.to_string()),
            || fsync(&source_parent.directory).map_err(|error| error.to_string()),
        )
    }

    fn claim_source_after_intent_with_sync<F, G>(
        &mut self,
        source_parent: &LinuxSourceParentDirectory,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        sync_local: F,
        sync_source_parent: G,
    ) -> Result<LinuxClaimedTopology, LinuxClaimSourceError>
    where
        F: FnOnce() -> Result<(), String>,
        G: FnOnce() -> Result<(), String>,
    {
        self.claim_source_after_intent_with_hooks(
            source_parent,
            local,
            transaction_id,
            || Ok(()),
            sync_local,
            sync_source_parent,
        )
    }

    fn claim_source_after_intent_with_hooks<H, F, G>(
        &mut self,
        source_parent: &LinuxSourceParentDirectory,
        local: &LinuxJournalDirectory,
        transaction_id: [u8; 16],
        after_placement_check: H,
        sync_local: F,
        sync_source_parent: G,
    ) -> Result<LinuxClaimedTopology, LinuxClaimSourceError>
    where
        H: FnOnce() -> Result<(), String>,
        F: FnOnce() -> Result<(), String>,
        G: FnOnce() -> Result<(), String>,
    {
        use rustix::fs::{AtFlags, FileType, RenameFlags, fstat, renameat_with, statat};

        let (expected_parent, expected_transaction, transaction_name, original_name) = match self
            .central
            .load_claim_intent_restart_state(local, transaction_id)?
        {
            LinuxClaimIntentRestartState::MatchingPair(record) => (
                record.source_parent_identity,
                record.source_transaction_identity,
                record.source_transaction_name,
                record.original_name,
            ),
            LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared { .. } => {
                return Err(LinuxClaimSourceError::JournalsNotMatching);
            }
        };
        let transaction_stat = fstat(&local.directory)
            .map_err(|error| LinuxClaimSourceError::Rename(error.to_string()))?;
        let observed_transaction = LinuxStableIdentity::new(
            transaction_stat.st_dev,
            transaction_stat.st_ino,
            LinuxFileType::Directory,
        );
        if observed_transaction != expected_transaction {
            return Err(LinuxClaimSourceError::TransactionIdentityMismatch {
                expected: expected_transaction,
                observed: observed_transaction,
            });
        }
        let parent_stat = fstat(&source_parent.directory)
            .map_err(|error| LinuxClaimSourceError::Rename(error.to_string()))?;
        let observed_parent = LinuxStableIdentity::new(
            parent_stat.st_dev,
            parent_stat.st_ino,
            LinuxFileType::Directory,
        );
        if observed_parent != expected_parent {
            return Err(LinuxClaimSourceError::SourceParentIdentityMismatch {
                expected: expected_parent,
                observed: observed_parent,
            });
        }
        let transaction_name = transaction_name
            .decode()
            .map_err(LinuxClaimSourceError::InvalidTransactionName)?;
        let named_transaction = match statat(
            &source_parent.directory,
            &transaction_name,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => {
                let file_type = match FileType::from_raw_mode(stat.st_mode) {
                    FileType::RegularFile => LinuxFileType::Regular,
                    FileType::Directory => LinuxFileType::Directory,
                    FileType::Symlink => LinuxFileType::Symlink,
                    _ => LinuxFileType::Other,
                };
                Some(LinuxStableIdentity::new(
                    stat.st_dev,
                    stat.st_ino,
                    file_type,
                ))
            }
            Err(error) if error == rustix::io::Errno::NOENT => None,
            Err(error) => return Err(LinuxClaimSourceError::Rename(error.to_string())),
        };
        if named_transaction != Some(expected_transaction) {
            return Err(LinuxClaimSourceError::TransactionPlacementMismatch {
                expected: expected_transaction,
                observed: named_transaction,
            });
        }
        after_placement_check().map_err(LinuxClaimSourceError::Rename)?;
        let original_name = original_name
            .decode()
            .map_err(LinuxClaimSourceError::InvalidOriginalName)?;
        renameat_with(
            &source_parent.directory,
            &original_name,
            &local.directory,
            "claimed",
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                LinuxClaimSourceError::ClaimedNameOccupied
            } else {
                LinuxClaimSourceError::Rename(error.to_string())
            }
        })?;
        let local_sync_error = sync_local().err();
        let claimed = self.observe_claimed_after_restart(local, transaction_id);
        let named_transaction = match statat(
            &source_parent.directory,
            &transaction_name,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => {
                let file_type = match FileType::from_raw_mode(stat.st_mode) {
                    FileType::RegularFile => LinuxFileType::Regular,
                    FileType::Directory => LinuxFileType::Directory,
                    FileType::Symlink => LinuxFileType::Symlink,
                    _ => LinuxFileType::Other,
                };
                Some(LinuxStableIdentity::new(
                    stat.st_dev,
                    stat.st_ino,
                    file_type,
                ))
            }
            Err(error) if error == rustix::io::Errno::NOENT => None,
            Err(_error) => {
                return Err(
                    LinuxClaimSourceError::PostRenameTransactionPlacementMismatch {
                        expected: expected_transaction,
                        observed: None,
                        local_sync_error,
                        claimed: Box::new(claimed),
                    },
                );
            }
        };
        if named_transaction != Some(expected_transaction) {
            return Err(
                LinuxClaimSourceError::PostRenameTransactionPlacementMismatch {
                    expected: expected_transaction,
                    observed: named_transaction,
                    local_sync_error,
                    claimed: Box::new(claimed),
                },
            );
        }
        let source_parent_sync_error = if local_sync_error.is_none()
            && matches!(claimed, Ok(LinuxClaimedTopology::ExpectedIdentity { .. }))
        {
            sync_source_parent().err()
        } else {
            None
        };
        if let (
            None,
            None,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name,
                identity,
            }),
        ) = (
            local_sync_error.as_ref(),
            source_parent_sync_error.as_ref(),
            claimed.as_ref(),
        ) {
            return Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: original_name.clone(),
                identity: *identity,
            });
        }
        Err(LinuxClaimSourceError::PostRenameUncertain {
            local_sync_error,
            source_parent_sync_error,
            claimed,
        })
    }

    pub fn advance_to_claim_intent_with_local(
        &mut self,
        local: &LinuxJournalDirectory,
        prepared: &PreparedRecord,
    ) -> Result<LinuxClaimIntentJournals, LinuxAdvanceClaimIntentError> {
        let decoded = decode_hex(&prepared.transaction_id)
            .expect("validated prepared transaction IDs are hexadecimal");
        let mut transaction_id = [0_u8; 16];
        transaction_id.copy_from_slice(&decoded);
        let claim_intent = ClaimIntentRecord::from_prepared(prepared)
            .map_err(LinuxAdvanceClaimIntentError::InvalidClaimIntent)?;
        match self
            .central
            .load_prepared_record_pair(local, transaction_id)
        {
            Ok(anchored) => {
                if anchored != *prepared {
                    return Err(LinuxAdvanceClaimIntentError::SuppliedPreparedMismatch);
                }
                self.central
                    .persist_claim_intent(&claim_intent)
                    .map_err(LinuxAdvanceClaimIntentError::CentralUpdateUncertain)?;
            }
            Err(prepared_error) => {
                let central_bytes = self
                    .central
                    .read_named(&format!("{}.journal", prepared.transaction_id))
                    .map_err(|error| {
                        LinuxAdvanceClaimIntentError::PreparedPair(
                            LinuxLoadPreparedPairError::CentralRead(error),
                        )
                    })?;
                let local_bytes = local.read_named("journal").map_err(|error| {
                    LinuxAdvanceClaimIntentError::PreparedPair(
                        LinuxLoadPreparedPairError::LocalRead(error),
                    )
                })?;
                let central_claim = ClaimIntentRecord::parse(&central_bytes).ok();
                let local_claim = ClaimIntentRecord::parse(&local_bytes).ok();
                let local_prepared = PreparedRecord::parse(&local_bytes).ok();
                if central_claim.as_ref() == Some(&claim_intent)
                    && local_claim.as_ref() == Some(&claim_intent)
                {
                    return Ok(LinuxClaimIntentJournals::MatchingPair);
                }
                if central_claim.as_ref() != Some(&claim_intent)
                    || local_prepared.as_ref() != Some(prepared)
                {
                    return Err(LinuxAdvanceClaimIntentError::PreparedPair(prepared_error));
                }
            }
        }
        local
            .persist_local_claim_intent(&claim_intent)
            .map_err(
                |error| LinuxAdvanceClaimIntentError::CentralDurableLocalUpdateUncertain {
                    error,
                    durable_central: Box::new(claim_intent.clone()),
                },
            )?;
        Ok(LinuxClaimIntentJournals::MatchingPair)
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
impl ClaimedRecord {
    pub fn from_claim_intent(
        claim_intent: &ClaimIntentRecord,
        claimed_source_identity: LinuxStableIdentity,
    ) -> Result<Self, JournalError> {
        if claimed_source_identity != claim_intent.expected_source_identity {
            return Err(JournalError::MismatchedClaimedSourceIdentity);
        }
        Ok(Self {
            schema_major: claim_intent.schema_major,
            schema_minor: claim_intent.schema_minor,
            transaction_id: claim_intent.transaction_id.clone(),
            phase: JournalPhase::Claimed,
            source_parent: claim_intent.source_parent.clone(),
            original_name: claim_intent.original_name.clone(),
            source_parent_identity: claim_intent.source_parent_identity,
            source_entry_identity: claim_intent.source_entry_identity,
            source_transaction_name: claim_intent.source_transaction_name.clone(),
            source_transaction_identity: claim_intent.source_transaction_identity,
            expected_source_identity: claim_intent.expected_source_identity,
            claimed_source_identity,
        })
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, JournalError> {
        let raw: RawClaimedRecord = serde_json::from_slice(bytes)
            .map_err(|error| JournalError::InvalidJson(error.to_string()))?;
        Self::try_from(raw)
    }
}

#[cfg(target_os = "linux")]
impl TryFrom<RawClaimedRecord> for ClaimedRecord {
    type Error = JournalError;

    fn try_from(raw: RawClaimedRecord) -> Result<Self, Self::Error> {
        if raw.phase != JournalPhase::Claimed {
            return Err(JournalError::InvalidClaimedPhase(raw.phase));
        }
        let claim_intent = ClaimIntentRecord::try_from(RawClaimIntentRecord {
            schema_major: raw.schema_major,
            schema_minor: raw.schema_minor,
            transaction_id: raw.transaction_id,
            phase: JournalPhase::ClaimIntent,
            source_parent: raw.source_parent,
            original_name: raw.original_name,
            source_parent_identity: raw.source_parent_identity,
            source_entry_identity: raw.source_entry_identity,
            source_transaction_name: raw.source_transaction_name,
            source_transaction_identity: raw.source_transaction_identity,
            expected_source_identity: raw.expected_source_identity,
        })?;
        Self::from_claim_intent(&claim_intent, raw.claimed_source_identity)
    }
}

#[cfg(target_os = "linux")]
impl<'de> Deserialize<'de> for ClaimedRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawClaimedRecord::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
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
    fn claimed_schema_preserves_claim_intent_evidence_and_verified_identity() {
        let prepared = PreparedRecord::new_linux(
            [0x45; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-45454545454545454545454545454545"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();

        let record = ClaimedRecord::from_claim_intent(
            &claim_intent,
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
        )
        .unwrap();
        let encoded = serde_json::to_value(&record).unwrap();
        let parsed = ClaimedRecord::parse(&serde_json::to_vec(&encoded).unwrap()).unwrap();
        let claim_intent_json = serde_json::to_value(&claim_intent).unwrap();

        assert_eq!(parsed, record);
        assert_eq!(encoded["phase"], serde_json::json!("claimed"));
        assert_eq!(
            encoded["claimed_source_identity"],
            claim_intent_json["expected_source_identity"]
        );
        for field in [
            "schema_major",
            "schema_minor",
            "transaction_id",
            "source_parent",
            "original_name",
            "source_parent_identity",
            "source_entry_identity",
            "source_transaction_name",
            "source_transaction_identity",
            "expected_source_identity",
        ] {
            assert_eq!(encoded[field], claim_intent_json[field], "field {field}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_record_rejects_phase_inappropriate_evidence() {
        let prepared = PreparedRecord::new_linux(
            [0x46; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-46464646464646464646464646464646"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        let mut encoded = serde_json::to_value(
            ClaimedRecord::from_claim_intent(&claim_intent, prepared.source_entry_identity)
                .unwrap(),
        )
        .unwrap();
        encoded["phase"] = serde_json::json!("claim-intent");

        assert_eq!(
            ClaimedRecord::parse(&serde_json::to_vec(&encoded).unwrap()),
            Err(JournalError::InvalidClaimedPhase(JournalPhase::ClaimIntent))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_record_rejects_identity_that_differs_from_claim_intent() {
        let prepared = PreparedRecord::new_linux(
            [0x47; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-47474747474747474747474747474747"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();

        assert_eq!(
            ClaimedRecord::from_claim_intent(
                &claim_intent,
                LinuxStableIdentity::new(11, 34, LinuxFileType::Regular),
            ),
            Err(JournalError::MismatchedClaimedSourceIdentity)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn direct_claimed_deserialization_validates_claimed_identity() {
        let prepared = PreparedRecord::new_linux(
            [0x48; 16],
            Path::new("/source"),
            Path::new("item"),
            LinuxStableIdentity::new(11, 22, LinuxFileType::Directory),
            LinuxStableIdentity::new(11, 33, LinuxFileType::Regular),
            Path::new(".ottrin-move-48484848484848484848484848484848"),
            LinuxStableIdentity::new(11, 44, LinuxFileType::Directory),
        )
        .unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        let mut encoded = serde_json::to_value(
            ClaimedRecord::from_claim_intent(&claim_intent, prepared.source_entry_identity)
                .unwrap(),
        )
        .unwrap();
        encoded["claimed_source_identity"]["inode"] = serde_json::json!(34);

        assert!(serde_json::from_value::<ClaimedRecord>(encoded).is_err());
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
    fn anchored_prepared_pair_advances_to_identical_claim_intent_without_source_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-intent-pair-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_path = root.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&source_path, b"must not be mutated").unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let record = PreparedRecord::new_linux(
            [0xd1; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &record).unwrap();
        let central_stale_name =
            "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1.journal.next-11111111111111111111111111111111";
        let local_stale_name = "journal.next-22222222222222222222222222222222";
        std::fs::write(central_path.join(central_stale_name), b"central stale").unwrap();
        std::fs::write(local_path.join(local_stale_name), b"local stale").unwrap();

        let mut recovery = central.try_lock_recovery().unwrap();
        let advanced = recovery.advance_to_claim_intent_with_local(&local, &record);
        drop(recovery);

        assert_eq!(advanced, Ok(LinuxClaimIntentJournals::MatchingPair));
        let central_bytes =
            std::fs::read(central_path.join("d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1.journal")).unwrap();
        let local_bytes = std::fs::read(local_path.join("journal")).unwrap();
        assert_eq!(central_bytes, local_bytes);
        let claim_intent = ClaimIntentRecord::parse(&central_bytes).unwrap();
        assert_eq!(
            claim_intent,
            ClaimIntentRecord::from_prepared(&record).unwrap()
        );
        assert_eq!(
            std::fs::read(central_path.join(central_stale_name)).unwrap(),
            b"central stale"
        );
        assert_eq!(
            std::fs::read(local_path.join(local_stale_name)).unwrap(),
            b"local stale"
        );
        assert_eq!(std::fs::read(source_path).unwrap(), b"must not be mutated");
        assert_eq!(central.discover_pending(), Ok(vec![[0xd1; 16]]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn matching_claim_intent_pair_loads_exact_record_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-claim-intent-pair-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xd4; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd4; 16]);

        assert_eq!(
            loaded,
            Ok(LinuxClaimIntentRestartState::MatchingPair(
                ClaimIntentRecord::from_prepared(&prepared).unwrap()
            ))
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_advances_matching_claim_intent_pair_to_identical_claimed_evidence() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-advance-claimed-pair-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        let destination_path = root.join("destination");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        std::fs::write(&destination_path, b"destination must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xb1; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            claimed_identity,
            Path::new(".ottrin-move-b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }

        let advanced = {
            let mut recovery = central.try_lock_recovery().unwrap();
            assert_eq!(
                recovery.observe_claimed_after_restart(&local, [0xb1; 16]),
                Ok(LinuxClaimedTopology::ExpectedIdentity {
                    original_name: PathBuf::from("source-item"),
                    identity: claimed_identity,
                })
            );
            recovery.advance_to_claimed_with_local(&local, [0xb1; 16])
        };

        assert_eq!(advanced, Ok(LinuxClaimedJournals::MatchingPair));
        let central_bytes =
            std::fs::read(central_path.join("b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1.journal")).unwrap();
        let local_bytes = std::fs::read(local_path.join("journal")).unwrap();
        assert_eq!(central_bytes, local_bytes);
        assert_eq!(
            ClaimedRecord::parse(&central_bytes).unwrap(),
            ClaimedRecord::from_claim_intent(
                &ClaimIntentRecord::from_prepared(&prepared).unwrap(),
                claimed_identity,
            )
            .unwrap()
        );
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"claimed source must remain"
        );
        assert_eq!(
            std::fs::read(&destination_path).unwrap(),
            b"destination must remain"
        );
        assert!(!root.join("source-item").exists());
        assert_eq!(central.discover_pending(), Ok(vec![[0xb1; 16]]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn central_claimed_persistence_failure_returns_exact_unchanged_claim_intent_pair() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-central-claimed-failure-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        let destination_path = root.join("destination");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        std::fs::write(&destination_path, b"destination must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let transaction_id = [0xb7; 16];
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            claimed_identity,
            Path::new(".ottrin-move-b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let expected_claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&transaction_id)));
        let local_journal_path = local_path.join("journal");
        let central_before = std::fs::read(&central_journal_path).unwrap();
        let local_before = std::fs::read(&local_journal_path).unwrap();
        std::fs::set_permissions(&central_path, std::fs::Permissions::from_mode(0o500)).unwrap();

        let failed = recovery.advance_to_claimed_with_local(&local, transaction_id);

        let Err(LinuxAdvanceClaimedError::CentralUpdateRejected {
            error: JournalError::Io(_),
            durable_central_claim_intent,
            durable_local_claim_intent,
        }) = failed
        else {
            panic!("central persistence failure must retain the exact durable claim-intent pair");
        };
        assert_eq!(*durable_central_claim_intent, expected_claim_intent);
        assert_eq!(*durable_local_claim_intent, expected_claim_intent);
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_before
        );
        assert_eq!(std::fs::read(&local_journal_path).unwrap(), local_before);
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"claimed source must remain"
        );
        assert_eq!(
            std::fs::read(&destination_path).unwrap(),
            b"destination must remain"
        );
        assert!(!root.join("source-item").exists());
        std::fs::set_permissions(&central_path, std::fs::Permissions::from_mode(0o700)).unwrap();
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_replacement_distinguishes_post_rename_directory_sync_failure() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claimed-post-rename-failure-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let transaction_id = [0xb9; 16];
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-b9b9b9b9b9b9b9b9b9b9b9b9b9b9b9b9"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        let claimed =
            ClaimedRecord::from_claim_intent(&claim_intent, prepared.source_entry_identity)
                .unwrap();
        let directory = LinuxJournalDirectory::open(&root).unwrap();
        directory.persist_claim_intent(&claim_intent).unwrap();

        let result = directory.replace_named_staged_with_sync(
            &format!("{}.journal", encode_hex(&transaction_id)),
            &claimed,
            || Err(JournalError::Io("injected directory sync failure".into())),
        );

        assert_eq!(
            result,
            Err(LinuxJournalReplaceError::AfterRename(JournalError::Io(
                "injected directory sync failure".into()
            )))
        );
        assert_eq!(
            ClaimedRecord::parse(
                &std::fs::read(root.join(format!("{}.journal", encode_hex(&transaction_id))))
                    .unwrap()
            )
            .unwrap(),
            claimed
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_advance_resumes_by_inspecting_live_journal_after_post_rename_sync_uncertainty() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-resume-post-rename-claimed-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        let destination_path = root.join("destination");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        std::fs::write(&destination_path, b"destination must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let transaction_id = [0xbb; 16];
        let claimed_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            claimed_identity,
            Path::new(".ottrin-move-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            LinuxStableIdentity::new(
                local_metadata.dev(),
                local_metadata.ino(),
                LinuxFileType::Directory,
            ),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let local_claim_intent_before = std::fs::read(local_path.join("journal")).unwrap();

        let uncertain = recovery.advance_to_claimed_with_local_with_central_sync(
            &local,
            transaction_id,
            || Err(JournalError::Io("injected post-rename sync failure".into())),
        );

        assert_eq!(
            uncertain,
            Err(LinuxAdvanceClaimedError::CentralUpdateUncertain(
                JournalError::Io("injected post-rename sync failure".into())
            ))
        );
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&transaction_id)));
        let central_claimed_before = std::fs::read(&central_journal_path).unwrap();
        let claimed = ClaimedRecord::parse(&central_claimed_before).unwrap();
        assert_eq!(
            central.load_claimed_restart_state(&local, transaction_id),
            Ok(LinuxClaimedRestartState::CentralClaimedLocalClaimIntent {
                claimed: claimed.clone(),
                claim_intent: ClaimIntentRecord::parse(&local_claim_intent_before).unwrap(),
            })
        );

        let still_uncertain = recovery.advance_to_claimed_with_local_with_central_sync(
            &local,
            transaction_id,
            || {
                Err(JournalError::Io(
                    "injected restart durability failure".into(),
                ))
            },
        );
        assert_eq!(
            still_uncertain,
            Err(LinuxAdvanceClaimedError::CentralUpdateUncertain(
                JournalError::Io("injected restart durability failure".into())
            ))
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_claim_intent_before
        );
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_claimed_before
        );

        assert_eq!(
            recovery.advance_to_claimed_with_local(&local, transaction_id),
            Ok(LinuxClaimedJournals::MatchingPair)
        );
        assert_eq!(
            central.load_claimed_restart_state(&local, transaction_id),
            Ok(LinuxClaimedRestartState::MatchingPair {
                central: claimed.clone(),
                local: claimed,
            })
        );
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_claimed_before
        );
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"claimed source must remain"
        );
        assert_eq!(
            std::fs::read(&destination_path).unwrap(),
            b"destination must remain"
        );
        assert!(!root.join("source-item").exists());
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_central_claimed_sigkill_child(root: &Path) {
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        std::fs::write(root.join("destination"), b"destination must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let transaction_id = [0xb8; 16];
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(
                claimed_metadata.dev(),
                claimed_metadata.ino(),
                LinuxFileType::Regular,
            ),
            Path::new(".ottrin-move-b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8b8"),
            LinuxStableIdentity::new(
                local_metadata.dev(),
                local_metadata.ino(),
                LinuxFileType::Directory,
            ),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ =
            recovery.advance_to_claimed_with_local_after_central(&local, transaction_id, || {
                let _ = rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::Kill,
                );
                panic!("SIGKILL did not terminate the claimed-journal child");
            });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_after_central_claimed_flush_retains_exact_restart_evidence() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_CENTRAL_CLAIMED_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_CENTRAL_CLAIMED_KILL_ROOT").unwrap();
            run_central_claimed_sigkill_child(Path::new(&root));
            panic!("claimed-journal child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-central-claimed-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_after_central_claimed_flush_retains_exact_restart_evidence",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_CENTRAL_CLAIMED_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_CENTRAL_CLAIMED_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let local_path = root.join("local");
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xb8; 16])));
        let local_journal_path = local_path.join("journal");
        let central_before = std::fs::read(&central_journal_path).unwrap();
        let local_before = std::fs::read(&local_journal_path).unwrap();
        let claimed = ClaimedRecord::parse(&central_before).unwrap();
        let claim_intent = ClaimIntentRecord::parse(&local_before).unwrap();

        assert_eq!(
            central.load_claimed_restart_state(&local, [0xb8; 16]),
            Ok(LinuxClaimedRestartState::CentralClaimedLocalClaimIntent {
                claimed,
                claim_intent,
            })
        );
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_before
        );
        assert_eq!(std::fs::read(&local_journal_path).unwrap(), local_before);
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"claimed source must remain"
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        assert!(!root.join("source-item").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_local_claimed_sigkill_child(root: &Path) {
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        std::fs::write(root.join("destination"), b"destination must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let transaction_id = [0xba; 16];
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(
                claimed_metadata.dev(),
                claimed_metadata.ino(),
                LinuxFileType::Regular,
            ),
            Path::new(".ottrin-move-babababababababababababababababa"),
            LinuxStableIdentity::new(
                local_metadata.dev(),
                local_metadata.ino(),
                LinuxFileType::Directory,
            ),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ = recovery.advance_to_claimed_with_local_after_local(&local, transaction_id, || {
            let _ = rustix::process::kill_process(
                rustix::process::getpid(),
                rustix::process::Signal::Kill,
            );
            panic!("SIGKILL did not terminate the local-claimed child");
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_after_local_claimed_flush_retains_exact_matching_restart_evidence() {
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_LOCAL_CLAIMED_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_LOCAL_CLAIMED_KILL_ROOT").unwrap();
            run_local_claimed_sigkill_child(Path::new(&root));
            panic!("local-claimed child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-local-claimed-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_after_local_claimed_flush_retains_exact_matching_restart_evidence",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_LOCAL_CLAIMED_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_LOCAL_CLAIMED_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let local_path = root.join("local");
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xba; 16])));
        let local_journal_path = local_path.join("journal");
        let central_before = std::fs::read(&central_journal_path).unwrap();
        let local_before = std::fs::read(&local_journal_path).unwrap();
        let central_claimed = ClaimedRecord::parse(&central_before).unwrap();
        let local_claimed = ClaimedRecord::parse(&local_before).unwrap();

        assert_eq!(
            central.load_claimed_restart_state(&local, [0xba; 16]),
            Ok(LinuxClaimedRestartState::MatchingPair {
                central: central_claimed,
                local: local_claimed,
            })
        );
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_before
        );
        assert_eq!(std::fs::read(&local_journal_path).unwrap(), local_before);
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"claimed source must remain"
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        assert!(!root.join("source-item").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_advance_resumes_central_claimed_local_claim_intent_after_write_failure() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-resume-partial-claimed-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&claimed_path, b"claimed source must remain").unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xb2; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            claimed_identity,
            Path::new(".ottrin-move-b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let expected = ClaimedRecord::from_claim_intent(
            &ClaimIntentRecord::from_prepared(&prepared).unwrap(),
            claimed_identity,
        )
        .unwrap();
        std::fs::set_permissions(&local_path, std::fs::Permissions::from_mode(0o500)).unwrap();

        let failed = recovery.advance_to_claimed_with_local(&local, [0xb2; 16]);

        let Err(LinuxAdvanceClaimedError::CentralDurableLocalUpdateUncertain {
            error: JournalError::Io(_),
            durable_central,
        }) = failed
        else {
            panic!("local persistence failure must expose exact durable claimed evidence");
        };
        assert_eq!(*durable_central, expected);
        assert_eq!(
            ClaimedRecord::parse(
                &std::fs::read(central_path.join("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2.journal"))
                    .unwrap()
            )
            .unwrap(),
            expected
        );
        assert!(
            ClaimIntentRecord::parse(&std::fs::read(local_path.join("journal")).unwrap()).is_ok()
        );
        std::fs::set_permissions(&local_path, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(
            recovery.advance_to_claimed_with_local(&local, [0xb2; 16]),
            Ok(LinuxClaimedJournals::MatchingPair)
        );
        assert_eq!(
            std::fs::read(central_path.join("b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2.journal")).unwrap(),
            std::fs::read(local_path.join("journal")).unwrap()
        );
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"claimed source must remain"
        );
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_restart_loader_returns_exact_matching_and_partial_evidence_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-claimed-restart-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xb3; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        central.persist_claim_intent(&claim_intent).unwrap();
        local.persist_local_claim_intent(&claim_intent).unwrap();
        let claimed = ClaimedRecord::from_claim_intent(
            &claim_intent,
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
        )
        .unwrap();
        central.persist_claimed(&claimed).unwrap();
        let central_before =
            std::fs::read(central_path.join("b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3.journal")).unwrap();
        let local_claim_intent_before = std::fs::read(local_path.join("journal")).unwrap();

        assert_eq!(
            central.load_claimed_restart_state(&local, [0xb3; 16]),
            Ok(LinuxClaimedRestartState::CentralClaimedLocalClaimIntent {
                claimed: claimed.clone(),
                claim_intent: claim_intent.clone(),
            })
        );
        assert_eq!(
            std::fs::read(central_path.join("b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3.journal")).unwrap(),
            central_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_claim_intent_before
        );

        local.persist_local_claimed(&claimed).unwrap();
        let local_claimed_before = std::fs::read(local_path.join("journal")).unwrap();
        assert_eq!(
            central.load_claimed_restart_state(&local, [0xb3; 16]),
            Ok(LinuxClaimedRestartState::MatchingPair {
                central: claimed.clone(),
                local: claimed,
            })
        );
        assert_eq!(
            std::fs::read(central_path.join("b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3.journal")).unwrap(),
            central_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_claimed_before
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claimed_restart_loader_returns_exact_cross_transaction_claimed_evidence() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-cross-transaction-claimed-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_prepared = PreparedRecord::new_linux(
            [0xb4; 16],
            &root,
            Path::new("central-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let central_claimed = ClaimedRecord::from_claim_intent(
            &ClaimIntentRecord::from_prepared(&central_prepared).unwrap(),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
        )
        .unwrap();
        central.persist_claimed(&central_claimed).unwrap();
        let local_prepared = PreparedRecord::new_linux(
            [0xb5; 16],
            &root,
            Path::new("local-source"),
            LinuxStableIdentity::new(7, 47, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 53, LinuxFileType::Regular),
            Path::new(".ottrin-move-b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5"),
            LinuxStableIdentity::new(7, 101, LinuxFileType::Directory),
        )
        .unwrap();
        let local_claimed = ClaimedRecord::from_claim_intent(
            &ClaimIntentRecord::from_prepared(&local_prepared).unwrap(),
            LinuxStableIdentity::new(7, 53, LinuxFileType::Regular),
        )
        .unwrap();
        local.persist_local_claimed(&local_claimed).unwrap();
        let central_before =
            std::fs::read(central_path.join("b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4.journal")).unwrap();
        let local_before = std::fs::read(local_path.join("journal")).unwrap();

        assert_eq!(
            central.load_claimed_restart_state(&local, [0xb4; 16]),
            Err(LinuxLoadClaimedRestartError::ClaimedTransactionMismatch {
                durable_central: Box::new(central_claimed),
                local: Box::new(local_claimed),
            })
        );
        assert_eq!(
            std::fs::read(central_path.join("b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4b4.journal")).unwrap(),
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
    fn claimed_restart_loader_returns_exact_cross_transaction_claim_intent_evidence() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-cross-transaction-claim-intent-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_prepared = PreparedRecord::new_linux(
            [0xb6; 16],
            &root,
            Path::new("central-source"),
            LinuxStableIdentity::new(7, 61, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 67, LinuxFileType::Regular),
            Path::new(".ottrin-move-b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6"),
            LinuxStableIdentity::new(7, 103, LinuxFileType::Directory),
        )
        .unwrap();
        let central_claimed = ClaimedRecord::from_claim_intent(
            &ClaimIntentRecord::from_prepared(&central_prepared).unwrap(),
            LinuxStableIdentity::new(7, 67, LinuxFileType::Regular),
        )
        .unwrap();
        central.persist_claimed(&central_claimed).unwrap();
        let local_prepared = PreparedRecord::new_linux(
            [0xb7; 16],
            &root,
            Path::new("local-source"),
            LinuxStableIdentity::new(7, 71, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 73, LinuxFileType::Regular),
            Path::new(".ottrin-move-b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7"),
            LinuxStableIdentity::new(7, 107, LinuxFileType::Directory),
        )
        .unwrap();
        let local_claim_intent = ClaimIntentRecord::from_prepared(&local_prepared).unwrap();
        local
            .persist_local_claim_intent(&local_claim_intent)
            .unwrap();
        let central_before =
            std::fs::read(central_path.join("b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6.journal")).unwrap();
        let local_before = std::fs::read(local_path.join("journal")).unwrap();

        assert_eq!(
            central.load_claimed_restart_state(&local, [0xb6; 16]),
            Err(
                LinuxLoadClaimedRestartError::ClaimIntentTransactionMismatch {
                    durable_central: Box::new(central_claimed),
                    local: Box::new(local_claim_intent),
                }
            )
        );
        assert_eq!(
            std::fs::read(central_path.join("b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6b6.journal")).unwrap(),
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
    fn restart_loader_returns_exact_central_claim_and_local_prepared_evidence() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-partial-claim-intent-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_path = root.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&source_path, b"must remain").unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xd5; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        central.persist_claim_intent(&claim_intent).unwrap();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd5; 16]);

        assert_eq!(
            loaded,
            Ok(
                LinuxClaimIntentRestartState::CentralClaimIntentLocalPrepared {
                    claim_intent,
                    prepared,
                }
            )
        );
        assert_eq!(std::fs::read(&source_path).unwrap(), b"must remain");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_unexpected_claim_identity_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-unexpected-claim-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let expected_path = root.join("expected-source");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&expected_path, b"expected").unwrap();
        std::fs::write(&claimed_path, b"unexpected must remain").unwrap();
        let expected_metadata = std::fs::symlink_metadata(&expected_path).unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let expected_identity = LinuxStableIdentity::new(
            expected_metadata.dev(),
            expected_metadata.ino(),
            LinuxFileType::Regular,
        );
        let unexpected_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xe7; 16],
            &root,
            Path::new("expected-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            expected_identity,
            Path::new(".ottrin-move-e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            let observed = recovery.observe_claimed_after_restart(&local, [0xe7; 16]);
            drop(recovery);
            observed
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::UnexpectedIdentity {
                expected: expected_identity,
                observed: unexpected_identity,
            })
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"unexpected must remain"
        );
        assert_eq!(std::fs::read(&expected_path).unwrap(), b"expected");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_observes_claimed_symlinks_without_following_them() {
        use std::os::unix::fs::{MetadataExt, symlink};

        for (transaction_id, target_exists) in [([0xeb; 16], true), ([0xec; 16], false)] {
            let root = std::env::temp_dir().join(format!(
                "ottrin-move-observe-claimed-symlink-{}-{}",
                target_exists,
                encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
            ));
            let central_path = root.join("central");
            let local_path = root.join("local");
            let target_path = root.join("target");
            let claimed_path = local_path.join("claimed");
            std::fs::create_dir_all(&central_path).unwrap();
            std::fs::create_dir_all(&local_path).unwrap();
            if target_exists {
                std::fs::write(&target_path, b"target must remain").unwrap();
            }
            symlink(&target_path, &claimed_path).unwrap();
            let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
            let observed_symlink = LinuxStableIdentity::new(
                claimed_metadata.dev(),
                claimed_metadata.ino(),
                LinuxFileType::Symlink,
            );
            let expected_identity = if target_exists {
                let target_metadata = std::fs::symlink_metadata(&target_path).unwrap();
                LinuxStableIdentity::new(
                    target_metadata.dev(),
                    target_metadata.ino(),
                    LinuxFileType::Regular,
                )
            } else {
                LinuxStableIdentity::new(38, 867_5310, LinuxFileType::Regular)
            };
            let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
            let local_identity = LinuxStableIdentity::new(
                local_metadata.dev(),
                local_metadata.ino(),
                LinuxFileType::Directory,
            );
            let central = LinuxJournalDirectory::open(&central_path).unwrap();
            let local = LinuxJournalDirectory::open(&local_path).unwrap();
            let transaction_name =
                PathBuf::from(format!(".ottrin-move-{}", encode_hex(&transaction_id)));
            let prepared = PreparedRecord::new_linux(
                transaction_id,
                &root,
                Path::new("source-item"),
                LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
                expected_identity,
                &transaction_name,
                local_identity,
            )
            .unwrap();
            central.prepare_with_local(&local, &prepared).unwrap();
            {
                let mut recovery = central.try_lock_recovery().unwrap();
                let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
                drop(recovery);
                advanced.unwrap();
            }
            let journal_before = std::fs::read(local_path.join("journal")).unwrap();

            let observed = {
                let mut recovery = central.try_lock_recovery().unwrap();
                recovery.observe_claimed_after_restart(&local, transaction_id)
            };

            assert_eq!(
                observed,
                Ok(LinuxClaimedTopology::UnexpectedIdentity {
                    expected: expected_identity,
                    observed: observed_symlink,
                })
            );
            assert_eq!(
                std::fs::symlink_metadata(&claimed_path).unwrap().ino(),
                claimed_metadata.ino()
            );
            assert_eq!(
                std::fs::read(local_path.join("journal")).unwrap(),
                journal_before
            );
            if target_exists {
                assert_eq!(std::fs::read(&target_path).unwrap(), b"target must remain");
            } else {
                assert!(!target_path.exists());
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_rejects_a_substituted_local_transaction_directory() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-substituted-local-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let substituted_path = root.join("substituted-local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir_all(&substituted_path).unwrap();
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let claimed_path = substituted_path.join("claimed");
        std::fs::write(&claimed_path, b"must not become recovery evidence").unwrap();
        let original_path = root.join("source-item");
        std::fs::hard_link(&claimed_path, &original_path).unwrap();
        let claimed_metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let expected_identity = LinuxStableIdentity::new(
            claimed_metadata.dev(),
            claimed_metadata.ino(),
            LinuxFileType::Regular,
        );
        let substituted_metadata = std::fs::symlink_metadata(&substituted_path).unwrap();
        let substituted_identity = LinuxStableIdentity::new(
            substituted_metadata.dev(),
            substituted_metadata.ino(),
            LinuxFileType::Directory,
        );
        let root_metadata = std::fs::symlink_metadata(&root).unwrap();
        let root_identity = LinuxStableIdentity::new(
            root_metadata.dev(),
            root_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let substituted = LinuxJournalDirectory::open(&substituted_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&root).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xea; 16],
            &root,
            Path::new("source-item"),
            root_identity,
            expected_identity,
            Path::new(".ottrin-move-eaeaeaeaeaeaeaeaeaeaeaeaeaeaeaea"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
            drop(recovery);
            advanced.unwrap();
        }
        std::fs::copy(local_path.join("journal"), substituted_path.join("journal")).unwrap();
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let substituted_before = std::fs::read_dir(&substituted_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            let observed = recovery.observe_claimed_after_restart(&substituted, [0xea; 16]);
            drop(recovery);
            observed
        };

        assert_eq!(
            observed,
            Err(LinuxObserveClaimedError::TransactionIdentityMismatch {
                expected: local_identity,
                observed: substituted_identity,
            })
        );
        let observed_original = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_original_after_restart(&source_parent, &substituted, [0xea; 16])
        };
        assert_eq!(
            observed_original,
            Err(LinuxObserveOriginalError::TransactionIdentityMismatch {
                expected: local_identity,
                observed: substituted_identity,
            })
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let substituted_after = std::fs::read_dir(&substituted_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(substituted_after, substituted_before);
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"must not become recovery evidence"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_rejects_a_substituted_source_parent_before_observing_original() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-substituted-source-parent-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_parent_path = root.join("source-parent");
        let substituted_parent_path = root.join("substituted-parent");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir_all(&source_parent_path).unwrap();
        std::fs::create_dir_all(&substituted_parent_path).unwrap();
        let original_path = source_parent_path.join("source-item");
        let substituted_original_path = substituted_parent_path.join("source-item");
        std::fs::write(&original_path, b"source must remain").unwrap();
        std::fs::hard_link(&original_path, &substituted_original_path).unwrap();
        let source_parent_metadata = std::fs::symlink_metadata(&source_parent_path).unwrap();
        let source_parent_identity = LinuxStableIdentity::new(
            source_parent_metadata.dev(),
            source_parent_metadata.ino(),
            LinuxFileType::Directory,
        );
        let substituted_parent_metadata =
            std::fs::symlink_metadata(&substituted_parent_path).unwrap();
        let substituted_parent_identity = LinuxStableIdentity::new(
            substituted_parent_metadata.dev(),
            substituted_parent_metadata.ino(),
            LinuxFileType::Directory,
        );
        let source_metadata = std::fs::symlink_metadata(&original_path).unwrap();
        let source_identity = LinuxStableIdentity::new(
            source_metadata.dev(),
            source_metadata.ino(),
            LinuxFileType::Regular,
        );
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let substituted_parent =
            LinuxSourceParentDirectory::open(&substituted_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xed; 16],
            &source_parent_path,
            Path::new("source-item"),
            source_parent_identity,
            source_identity,
            Path::new(".ottrin-move-edededededededededededededededed"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
            drop(recovery);
            advanced.unwrap();
        }
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_original_after_restart(&substituted_parent, &local, [0xed; 16])
        };

        assert_eq!(
            observed,
            Err(LinuxObserveOriginalError::SourceParentIdentityMismatch {
                expected: source_parent_identity,
                observed: substituted_parent_identity,
            })
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        assert_eq!(
            std::fs::read(&original_path).unwrap(),
            b"source must remain"
        );
        assert_eq!(
            std::fs::read(&substituted_original_path).unwrap(),
            b"source must remain"
        );
        assert_eq!(
            std::fs::symlink_metadata(&source_parent_path)
                .unwrap()
                .ino(),
            source_parent_metadata.ino()
        );
        assert_eq!(
            std::fs::symlink_metadata(&substituted_parent_path)
                .unwrap()
                .ino(),
            substituted_parent_metadata.ino()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_unexpected_original_identity_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-unexpected-original-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_parent_path = root.join("source-parent");
        let expected_path = root.join("expected-source");
        let original_path = source_parent_path.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir_all(&source_parent_path).unwrap();
        std::fs::write(&expected_path, b"expected must remain").unwrap();
        std::fs::write(&original_path, b"unexpected must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_parent_identity = stable_identity(&source_parent_path, LinuxFileType::Directory);
        let expected_identity = stable_identity(&expected_path, LinuxFileType::Regular);
        let unexpected_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let local_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xee; 16],
            &source_parent_path,
            Path::new("source-item"),
            source_parent_identity,
            expected_identity,
            Path::new(".ottrin-move-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
            drop(recovery);
            advanced.unwrap();
        }
        let central_journal_before =
            std::fs::read(central_path.join(format!("{}.journal", encode_hex(&[0xee; 16]))))
                .unwrap();
        let local_journal_before = std::fs::read(local_path.join("journal")).unwrap();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_original_after_restart(&source_parent, &local, [0xee; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxOriginalTopology::UnexpectedIdentity {
                expected: expected_identity,
                observed: unexpected_identity,
            })
        );
        assert_eq!(
            std::fs::read(central_path.join(format!("{}.journal", encode_hex(&[0xee; 16]))))
                .unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(&expected_path).unwrap(),
            b"expected must remain"
        );
        assert_eq!(
            std::fs::read(&original_path).unwrap(),
            b"unexpected must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_expected_original_identity_without_mutation() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-expected-original-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_parent_path = root.join("source-parent");
        let original_name = PathBuf::from(std::ffi::OsString::from_vec(b"source-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir_all(&source_parent_path).unwrap();
        std::fs::write(&original_path, b"source must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_parent_identity = stable_identity(&source_parent_path, LinuxFileType::Directory);
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let local_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xef; 16],
            &source_parent_path,
            &original_name,
            source_parent_identity,
            source_identity,
            Path::new(".ottrin-move-efefefefefefefefefefefefefefefef"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
            drop(recovery);
            advanced.unwrap();
        }
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xef; 16])));
        let central_journal_before = std::fs::read(&central_journal_path).unwrap();
        let local_journal_before = std::fs::read(local_path.join("journal")).unwrap();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_original_after_restart(&source_parent, &local, [0xef; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxOriginalTopology::ExpectedIdentity(source_identity))
        );
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(&original_path).unwrap(),
            b"source must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_claims_the_expected_original_without_replacement() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-expected-original-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let local_path = source_parent_path.join(".ottrin-move-f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3");
        let original_path = source_parent_path.join("source-item");
        let claimed_path = local_path.join("claimed");
        let unrelated_path = source_parent_path.join("unrelated");
        let destination_path = root.join("destination");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        std::fs::write(&unrelated_path, b"unrelated must remain").unwrap();
        std::fs::write(&destination_path, b"destination must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_parent_identity = stable_identity(&source_parent_path, LinuxFileType::Directory);
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let local_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf3; 16],
            &source_parent_path,
            Path::new("source-item"),
            source_parent_identity,
            source_identity,
            Path::new(".ottrin-move-f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xf3; 16])
        };

        assert_eq!(
            claimed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: PathBuf::from("source-item"),
                identity: source_identity,
            })
        );
        assert!(!original_path.exists());
        assert_eq!(
            stable_identity(&claimed_path, LinuxFileType::Regular),
            source_identity
        );
        assert_eq!(std::fs::read(&claimed_path).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(&unrelated_path).unwrap(),
            b"unrelated must remain"
        );
        assert_eq!(
            std::fs::read(&destination_path).unwrap(),
            b"destination must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_claim_evidence_preserves_non_utf8_original_name_and_entry_kind() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-native-name-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa3; 16],
            &source_parent_path,
            &original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xa3; 16])
        };

        assert_eq!(
            claimed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: original_name.clone(),
                identity: source_identity,
            })
        );
        assert!(!original_path.exists());
        assert_eq!(
            stable_identity(&local_path.join("claimed"), LinuxFileType::Regular),
            source_identity
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_claim_evidence_preserves_dangling_symlink_entry_kind() {
        use std::os::unix::fs::{MetadataExt, symlink};

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-dangling-symlink-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4a4";
        let local_path = source_parent_path.join(transaction_name);
        let original_name = Path::new("source-link");
        let original_path = source_parent_path.join(original_name);
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        symlink("missing-target", &original_path).unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Symlink);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa4; 16],
            &source_parent_path,
            original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xa4; 16])
        };

        assert_eq!(
            claimed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: original_name.to_path_buf(),
                identity: source_identity,
            })
        );
        assert_eq!(
            stable_identity(&local_path.join("claimed"), LinuxFileType::Symlink),
            source_identity
        );
        assert_eq!(
            std::fs::read_link(local_path.join("claimed")).unwrap(),
            Path::new("missing-target")
        );
        assert_eq!(
            std::fs::symlink_metadata(&original_path)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_claim_evidence_preserves_directory_entry_kind_and_payload() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-directory-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6";
        let local_path = source_parent_path.join(transaction_name);
        let original_name = Path::new("source-directory");
        let original_path = source_parent_path.join(original_name);
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir(&original_path).unwrap();
        std::fs::write(original_path.join("payload"), b"directory payload").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa6; 16],
            &source_parent_path,
            original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xa6; 16])
        };

        assert_eq!(
            claimed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: original_name.to_path_buf(),
                identity: source_identity,
            })
        );
        assert_eq!(
            stable_identity(&local_path.join("claimed"), LinuxFileType::Directory),
            source_identity
        );
        assert_eq!(
            std::fs::read(local_path.join("claimed/payload")).unwrap(),
            b"directory payload"
        );
        assert!(!original_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_claim_evidence_preserves_multiply_linked_file_identity() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-hard-link-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7";
        let local_path = source_parent_path.join(transaction_name);
        let original_name = Path::new("source-item");
        let original_path = source_parent_path.join(original_name);
        let sibling_path = source_parent_path.join("surviving-link");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"linked payload").unwrap();
        std::fs::hard_link(&original_path, &sibling_path).unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        assert_eq!(
            stable_identity(&sibling_path, LinuxFileType::Regular),
            source_identity
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa7; 16],
            &source_parent_path,
            original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xa7; 16])
        };

        assert_eq!(
            claimed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: original_name.to_path_buf(),
                identity: source_identity,
            })
        );
        assert_eq!(
            stable_identity(&local_path.join("claimed"), LinuxFileType::Regular),
            source_identity
        );
        assert_eq!(
            stable_identity(&sibling_path, LinuxFileType::Regular),
            source_identity
        );
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"linked payload"
        );
        assert_eq!(std::fs::read(sibling_path).unwrap(), b"linked payload");
        assert!(!original_path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_claim_sigkill_child(root: &Path) {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        let destination_path = root.join("destination");
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&source_parent_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        std::fs::write(destination_path, b"destination must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa5; 16],
            &source_parent_path,
            &original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            stable_identity(&original_path, LinuxFileType::Regular),
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ = recovery.claim_source_after_intent_with_sync(
            &source_parent,
            &local,
            [0xa5; 16],
            || {
                let _ = rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::Kill,
                );
                Err("SIGKILL did not terminate the claim child".to_string())
            },
            || Err("source-parent sync unexpectedly reached".to_string()),
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_after_claim_rename_recovers_exact_claim_intent_evidence() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_CLAIM_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_CLAIM_KILL_ROOT").unwrap();
            run_claim_sigkill_child(Path::new(&root));
            panic!("claim child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-process-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_after_claim_rename_recovers_exact_claim_intent_evidence",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_CLAIM_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_CLAIM_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        let claimed_path = local_path.join("claimed");
        let metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity =
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xa5; 16])));
        let local_journal_path = local_path.join("journal");
        let central_journal_before = std::fs::read(&central_journal_path).unwrap();
        let local_journal_before = std::fs::read(&local_journal_path).unwrap();
        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_claimed_after_restart(&local, [0xa5; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name,
                identity: claimed_identity,
            })
        );
        assert!(!original_path.exists());
        assert_eq!(std::fs::read(claimed_path).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(&local_journal_path).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_post_claim_directory_sync_sigkill_child(root: &Path) {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&source_parent_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        std::fs::write(root.join("destination"), b"destination must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa9; 16],
            &source_parent_path,
            &original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            stable_identity(&original_path, LinuxFileType::Regular),
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ = recovery.claim_source_after_intent_with_sync(
            &source_parent,
            &local,
            [0xa9; 16],
            || rustix::fs::fsync(&local.directory).map_err(|error| error.to_string()),
            || {
                let _ = rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::Kill,
                );
                Err("SIGKILL did not terminate the post-sync child".to_string())
            },
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_after_claim_directory_sync_recovers_exact_claim_intent_evidence() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_POST_CLAIM_DIR_SYNC_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_POST_CLAIM_DIR_SYNC_KILL_ROOT").unwrap();
            run_post_claim_directory_sync_sigkill_child(Path::new(&root));
            panic!("post-claim-directory-sync child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-post-claim-directory-sync-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_after_claim_directory_sync_recovers_exact_claim_intent_evidence",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_POST_CLAIM_DIR_SYNC_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_POST_CLAIM_DIR_SYNC_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        let claimed_path = local_path.join("claimed");
        let metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity =
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xa9; 16])));
        let local_journal_path = local_path.join("journal");
        let central_journal_before = std::fs::read(&central_journal_path).unwrap();
        let local_journal_before = std::fs::read(&local_journal_path).unwrap();
        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_claimed_after_restart(&local, [0xa9; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name,
                identity: claimed_identity,
            })
        );
        assert!(!original_path.exists());
        assert_eq!(std::fs::read(claimed_path).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(&local_journal_path).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_post_source_parent_sync_sigkill_child(root: &Path) {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&source_parent_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        std::fs::write(root.join("destination"), b"destination must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xb0; 16],
            &source_parent_path,
            &original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            stable_identity(&original_path, LinuxFileType::Regular),
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ = recovery.claim_source_after_intent_with_sync(
            &source_parent,
            &local,
            [0xb0; 16],
            || rustix::fs::fsync(&local.directory).map_err(|error| error.to_string()),
            || {
                rustix::fs::fsync(&source_parent.directory).map_err(|error| error.to_string())?;
                let _ = rustix::process::kill_process(
                    rustix::process::getpid(),
                    rustix::process::Signal::Kill,
                );
                Err("SIGKILL did not terminate the post-parent-sync child".to_string())
            },
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_after_source_parent_sync_recovers_exact_claim_intent_evidence() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_POST_SOURCE_PARENT_SYNC_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_POST_SOURCE_PARENT_SYNC_KILL_ROOT").unwrap();
            run_post_source_parent_sync_sigkill_child(Path::new(&root));
            panic!("post-source-parent-sync child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-post-source-parent-sync-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_after_source_parent_sync_recovers_exact_claim_intent_evidence",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_POST_SOURCE_PARENT_SYNC_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_POST_SOURCE_PARENT_SYNC_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        let claimed_path = local_path.join("claimed");
        let metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let claimed_identity =
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xb0; 16])));
        let local_journal_path = local_path.join("journal");
        let central_journal_before = std::fs::read(&central_journal_path).unwrap();
        let local_journal_before = std::fs::read(&local_journal_path).unwrap();
        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_claimed_after_restart(&local, [0xb0; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name,
                identity: claimed_identity,
            })
        );
        assert!(!original_path.exists());
        assert_eq!(std::fs::read(claimed_path).unwrap(), b"source payload");
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(&local_journal_path).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn run_pre_claim_sigkill_child(root: &Path) {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        std::fs::create_dir(&central_path).unwrap();
        std::fs::create_dir(&source_parent_path).unwrap();
        std::fs::create_dir(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        std::fs::write(root.join("destination"), b"destination must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xa8; 16],
            &source_parent_path,
            &original_name,
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            stable_identity(&original_path, LinuxFileType::Regular),
            Path::new(transaction_name),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let mut recovery = central.try_lock_recovery().unwrap();
        recovery
            .advance_to_claim_intent_with_local(&local, &prepared)
            .unwrap();
        let _ =
            rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::Kill);
        panic!("SIGKILL did not terminate the pre-claim child");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn process_termination_before_claim_rename_recovers_original_and_missing_claim() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::ExitStatusExt;
        use std::process::Command;

        if std::env::var_os("OTTRIN_MOVE_PRE_CLAIM_KILL_CHILD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            let root = std::env::var_os("OTTRIN_MOVE_PRE_CLAIM_KILL_ROOT").unwrap();
            run_pre_claim_sigkill_child(Path::new(&root));
            panic!("pre-claim child returned without being killed");
        }
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-pre-claim-process-kill-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir(&root).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "move_recovery::tests::process_termination_before_claim_rename_recovers_original_and_missing_claim",
                "--nocapture",
            ])
            .env("OTTRIN_MOVE_PRE_CLAIM_KILL_CHILD", "1")
            .env("OTTRIN_MOVE_PRE_CLAIM_KILL_ROOT", &root)
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(9));

        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8a8";
        let local_path = source_parent_path.join(transaction_name);
        let original_name =
            PathBuf::from(std::ffi::OsString::from_vec(b"source-item-\xff".to_vec()));
        let original_path = source_parent_path.join(&original_name);
        let metadata = std::fs::symlink_metadata(&original_path).unwrap();
        let source_identity =
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let central_journal_path =
            central_path.join(format!("{}.journal", encode_hex(&[0xa8; 16])));
        let local_journal_path = local_path.join("journal");
        let central_journal_before = std::fs::read(&central_journal_path).unwrap();
        let local_journal_before = std::fs::read(&local_journal_path).unwrap();
        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            (
                recovery.observe_original_after_restart(&source_parent, &local, [0xa8; 16]),
                recovery.observe_claimed_after_restart(&local, [0xa8; 16]),
            )
        };

        assert_eq!(
            observed,
            (
                Ok(LinuxOriginalTopology::ExpectedIdentity(source_identity)),
                Ok(LinuxClaimedTopology::Missing {
                    expected: source_identity,
                }),
            )
        );
        assert_eq!(std::fs::read(&original_path).unwrap(), b"source payload");
        assert!(!local_path.join("claimed").exists());
        assert_eq!(
            std::fs::read(&central_journal_path).unwrap(),
            central_journal_before
        );
        assert_eq!(
            std::fs::read(&local_journal_path).unwrap(),
            local_journal_before
        );
        assert_eq!(
            std::fs::read(root.join("destination")).unwrap(),
            b"destination must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_does_not_flush_source_deletion_when_claim_sync_fails() {
        use std::cell::RefCell;
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-sync-failure-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let local_path = source_parent_path.join(".ottrin-move-f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5");
        let original_path = source_parent_path.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf5; 16],
            &source_parent_path,
            Path::new("source-item"),
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(".ottrin-move-f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5f5"),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let sync_order = RefCell::new(Vec::new());

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent_with_sync(
                &source_parent,
                &local,
                [0xf5; 16],
                || {
                    sync_order.borrow_mut().push("local");
                    Err("local sync failed".to_string())
                },
                || {
                    sync_order.borrow_mut().push("source-parent");
                    Ok(())
                },
            )
        };

        assert_eq!(sync_order.into_inner(), ["local"]);
        assert_eq!(
            claimed,
            Err(LinuxClaimSourceError::PostRenameUncertain {
                local_sync_error: Some("local sync failed".to_string()),
                source_parent_sync_error: None,
                claimed: Ok(LinuxClaimedTopology::ExpectedIdentity {
                    original_name: PathBuf::from("source-item"),
                    identity: source_identity,
                }),
            })
        );
        assert!(!original_path.exists());
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"source payload"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_observes_replaced_source_before_flushing_source_deletion() {
        use std::cell::RefCell;
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-replaced-source-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let local_path = source_parent_path.join(".ottrin-move-f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6");
        let original_path = source_parent_path.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"expected source").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let expected_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf6; 16],
            &source_parent_path,
            Path::new("source-item"),
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            expected_identity,
            Path::new(".ottrin-move-f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6"),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        std::fs::remove_file(&original_path).unwrap();
        std::fs::write(&original_path, b"replacement").unwrap();
        let replacement_identity = stable_identity(&original_path, LinuxFileType::Regular);
        assert_ne!(replacement_identity, expected_identity);
        let sync_order = RefCell::new(Vec::new());

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.claim_source_after_intent_with_sync(
                &source_parent,
                &local,
                [0xf6; 16],
                || {
                    sync_order.borrow_mut().push("local");
                    Ok(())
                },
                || {
                    sync_order.borrow_mut().push("source-parent");
                    Ok(())
                },
            )
        };

        assert_eq!(sync_order.into_inner(), ["local"]);
        assert_eq!(
            claimed,
            Err(LinuxClaimSourceError::PostRenameUncertain {
                local_sync_error: None,
                source_parent_sync_error: None,
                claimed: Ok(LinuxClaimedTopology::UnexpectedIdentity {
                    expected: expected_identity,
                    observed: replacement_identity,
                }),
            })
        );
        assert!(!original_path.exists());
        assert_eq!(
            std::fs::read(local_path.join("claimed")).unwrap(),
            b"replacement"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_rejects_reparented_transaction_before_claiming_source() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-reparented-transaction-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7";
        let local_path = source_parent_path.join(transaction_name);
        let moved_local_path = root.join("moved-transaction");
        let original_path = source_parent_path.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let transaction_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf7; 16],
            &source_parent_path,
            Path::new("source-item"),
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            transaction_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        std::fs::rename(&local_path, &moved_local_path).unwrap();

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xf7; 16])
        };

        assert_eq!(
            claimed,
            Err(LinuxClaimSourceError::TransactionPlacementMismatch {
                expected: transaction_identity,
                observed: None,
            })
        );
        assert_eq!(
            std::fs::read(&original_path).unwrap(),
            b"source must remain"
        );
        assert!(!moved_local_path.join("claimed").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_does_not_flush_source_deletion_when_transaction_moves_during_claim() {
        use std::cell::RefCell;
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-concurrent-reparent-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let transaction_name = ".ottrin-move-f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8f8";
        let local_path = source_parent_path.join(transaction_name);
        let moved_local_path = root.join("moved-transaction");
        let original_path = source_parent_path.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source payload").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let transaction_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf8; 16],
            &source_parent_path,
            Path::new("source-item"),
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(transaction_name),
            transaction_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let sync_order = RefCell::new(Vec::new());

        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent_with_hooks(
                &source_parent,
                &local,
                [0xf8; 16],
                || std::fs::rename(&local_path, &moved_local_path).map_err(|e| e.to_string()),
                || {
                    sync_order.borrow_mut().push("local");
                    Ok(())
                },
                || {
                    sync_order.borrow_mut().push("source-parent");
                    Ok(())
                },
            )
        };

        assert_eq!(sync_order.into_inner(), ["local"]);
        assert_eq!(
            claimed,
            Err(
                LinuxClaimSourceError::PostRenameTransactionPlacementMismatch {
                    expected: transaction_identity,
                    observed: None,
                    local_sync_error: None,
                    claimed: Box::new(Ok(LinuxClaimedTopology::ExpectedIdentity {
                        original_name: PathBuf::from("source-item"),
                        identity: source_identity,
                    })),
                }
            )
        );
        assert!(!original_path.exists());
        assert_eq!(
            std::fs::read(moved_local_path.join("claimed")).unwrap(),
            b"source payload"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_does_not_replace_an_occupied_claim_name() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-occupied-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let source_parent_path = root.join("source-parent");
        let local_path = source_parent_path.join(".ottrin-move-f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4");
        let original_path = source_parent_path.join("source-item");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&original_path, b"source must remain").unwrap();
        std::fs::write(&claimed_path, b"claim must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_identity = stable_identity(&original_path, LinuxFileType::Regular);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf4; 16],
            &source_parent_path,
            Path::new("source-item"),
            stable_identity(&source_parent_path, LinuxFileType::Directory),
            source_identity,
            Path::new(".ottrin-move-f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4f4"),
            stable_identity(&local_path, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claimed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
            recovery.claim_source_after_intent(&source_parent, &local, [0xf4; 16])
        };

        assert_eq!(claimed, Err(LinuxClaimSourceError::ClaimedNameOccupied));
        assert_eq!(
            std::fs::read(&original_path).unwrap(),
            b"source must remain"
        );
        assert_eq!(std::fs::read(&claimed_path).unwrap(), b"claim must remain");
        assert_eq!(
            stable_identity(&original_path, LinuxFileType::Regular),
            source_identity
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_missing_original_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-missing-original-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_parent_path = root.join("source-parent");
        let expected_path = root.join("expected-source");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::create_dir_all(&source_parent_path).unwrap();
        std::fs::write(&expected_path, b"expected must remain").unwrap();
        let stable_identity = |path: &Path, file_type| {
            let metadata = std::fs::symlink_metadata(path).unwrap();
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
        };
        let source_parent_identity = stable_identity(&source_parent_path, LinuxFileType::Directory);
        let expected_identity = stable_identity(&expected_path, LinuxFileType::Regular);
        let local_identity = stable_identity(&local_path, LinuxFileType::Directory);
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xf0; 16],
            &source_parent_path,
            Path::new("source-item"),
            source_parent_identity,
            expected_identity,
            Path::new(".ottrin-move-f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
            drop(recovery);
            advanced.unwrap();
        }
        let central_before = std::fs::read_dir(&central_path).unwrap().count();
        let local_journal_before = std::fs::read(local_path.join("journal")).unwrap();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery.observe_original_after_restart(&source_parent, &local, [0xf0; 16])
        };

        assert_eq!(
            observed,
            Ok(LinuxOriginalTopology::Missing {
                expected: expected_identity,
            })
        );
        assert_eq!(
            std::fs::read_dir(&central_path).unwrap().count(),
            central_before
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            local_journal_before
        );
        assert_eq!(std::fs::read_dir(&source_parent_path).unwrap().count(), 0);
        assert_eq!(
            std::fs::read(&expected_path).unwrap(),
            b"expected must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_observes_original_symlinks_without_following_them() {
        use std::os::unix::fs::{MetadataExt, symlink};

        for (transaction_id, target_exists) in [([0xf1; 16], true), ([0xf2; 16], false)] {
            let root = std::env::temp_dir().join(format!(
                "ottrin-move-observe-original-symlink-{}-{}",
                target_exists,
                encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
            ));
            let central_path = root.join("central");
            let local_path = root.join("local");
            let source_parent_path = root.join("source-parent");
            let expected_path = root.join("expected-source");
            let target_path = root.join("target");
            let original_path = source_parent_path.join("source-item");
            std::fs::create_dir_all(&central_path).unwrap();
            std::fs::create_dir_all(&local_path).unwrap();
            std::fs::create_dir_all(&source_parent_path).unwrap();
            std::fs::write(&expected_path, b"expected must remain").unwrap();
            if target_exists {
                std::fs::write(&target_path, b"target must remain").unwrap();
            }
            symlink(&target_path, &original_path).unwrap();
            let stable_identity = |path: &Path, file_type| {
                let metadata = std::fs::symlink_metadata(path).unwrap();
                LinuxStableIdentity::new(metadata.dev(), metadata.ino(), file_type)
            };
            let source_parent_identity =
                stable_identity(&source_parent_path, LinuxFileType::Directory);
            let expected_identity = stable_identity(&expected_path, LinuxFileType::Regular);
            let symlink_identity = stable_identity(&original_path, LinuxFileType::Symlink);
            let local_identity = stable_identity(&local_path, LinuxFileType::Directory);
            let central = LinuxJournalDirectory::open(&central_path).unwrap();
            let local = LinuxJournalDirectory::open(&local_path).unwrap();
            let source_parent = LinuxSourceParentDirectory::open(&source_parent_path).unwrap();
            let prepared = PreparedRecord::new_linux(
                transaction_id,
                &source_parent_path,
                Path::new("source-item"),
                source_parent_identity,
                expected_identity,
                Path::new(&format!(".ottrin-move-{}", encode_hex(&transaction_id))),
                local_identity,
            )
            .unwrap();
            central.prepare_with_local(&local, &prepared).unwrap();
            {
                let mut recovery = central.try_lock_recovery().unwrap();
                let advanced = recovery.advance_to_claim_intent_with_local(&local, &prepared);
                drop(recovery);
                advanced.unwrap();
            }
            let central_journal_path =
                central_path.join(format!("{}.journal", encode_hex(&transaction_id)));
            let central_journal_before = std::fs::read(&central_journal_path).unwrap();
            let local_journal_before = std::fs::read(local_path.join("journal")).unwrap();

            let observed = {
                let mut recovery = central.try_lock_recovery().unwrap();
                recovery.observe_original_after_restart(&source_parent, &local, transaction_id)
            };

            assert_eq!(
                observed,
                Ok(LinuxOriginalTopology::UnexpectedIdentity {
                    expected: expected_identity,
                    observed: symlink_identity,
                })
            );
            assert_eq!(
                std::fs::read(&central_journal_path).unwrap(),
                central_journal_before
            );
            assert_eq!(
                std::fs::read(local_path.join("journal")).unwrap(),
                local_journal_before
            );
            assert_eq!(std::fs::read_link(&original_path).unwrap(), target_path);
            if target_exists {
                assert_eq!(std::fs::read(&target_path).unwrap(), b"target must remain");
            } else {
                assert!(!target_path.exists());
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_expected_claim_identity_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-expected-claim-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let claimed_path = local_path.join("claimed");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&claimed_path, b"expected must remain").unwrap();
        let metadata = std::fs::symlink_metadata(&claimed_path).unwrap();
        let expected_identity =
            LinuxStableIdentity::new(metadata.dev(), metadata.ino(), LinuxFileType::Regular);
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xe8; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            expected_identity,
            Path::new(".ottrin-move-e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            let observed = recovery.observe_claimed_after_restart(&local, [0xe8; 16]);
            drop(recovery);
            observed
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::ExpectedIdentity {
                original_name: PathBuf::from("source-item"),
                identity: expected_identity,
            })
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        assert_eq!(
            std::fs::read(&claimed_path).unwrap(),
            b"expected must remain"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_reports_missing_claim_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-observe-missing-claim-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let expected_identity = LinuxStableIdentity::new(38, 867_5309, LinuxFileType::Regular);
        let local_metadata = std::fs::symlink_metadata(&local_path).unwrap();
        let local_identity = LinuxStableIdentity::new(
            local_metadata.dev(),
            local_metadata.ino(),
            LinuxFileType::Directory,
        );
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xe9; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            expected_identity,
            Path::new(".ottrin-move-e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9e9"),
            local_identity,
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        {
            let mut recovery = central.try_lock_recovery().unwrap();
            recovery
                .advance_to_claim_intent_with_local(&local, &prepared)
                .unwrap();
        }
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let observed = {
            let mut recovery = central.try_lock_recovery().unwrap();
            let observed = recovery.observe_claimed_after_restart(&local, [0xe9; 16]);
            drop(recovery);
            observed
        };

        assert_eq!(
            observed,
            Ok(LinuxClaimedTopology::Missing {
                expected: expected_identity,
            })
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        assert!(!local_path.join("claimed").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_completes_a_partial_claim_intent_pair_after_restart() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-resume-partial-claim-intent-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xe4; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4e4"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        central.persist_claim_intent(&claim_intent).unwrap();

        let mut recovery = central.try_lock_recovery().unwrap();
        let resumed = recovery.resume_claim_intent_with_local(&local, [0xe4; 16]);

        assert_eq!(resumed, Ok(LinuxClaimIntentJournals::MatchingPair));
        assert_eq!(
            central.load_claim_intent_restart_state(&local, [0xe4; 16]),
            Ok(LinuxClaimIntentRestartState::MatchingPair(
                claim_intent.clone()
            ))
        );
        let central_after_first = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after_first = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let repeated = recovery.resume_claim_intent_with_local(&local, [0xe4; 16]);

        assert_eq!(repeated, Ok(LinuxClaimIntentJournals::MatchingPair));
        assert_eq!(
            central.load_claim_intent_restart_state(&local, [0xe4; 16]),
            Ok(LinuxClaimIntentRestartState::MatchingPair(claim_intent))
        );
        let central_after_repeat = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after_repeat = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after_repeat, central_after_first);
        assert_eq!(local_after_repeat, local_after_first);
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn restart_loader_reports_requested_and_recorded_central_transaction_ids() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-mismatched-central-transaction-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let recorded_prepared = PreparedRecord::new_linux(
            [0xe5; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let recorded_claim = ClaimIntentRecord::from_prepared(&recorded_prepared).unwrap();
        central
            .replace_named("d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5d5.journal", &recorded_claim)
            .unwrap();
        local.persist_local_claim_intent(&recorded_claim).unwrap();
        let central_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_before = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd5; 16]);

        assert_eq!(
            loaded,
            Err(
                LinuxLoadClaimIntentRestartError::RequestedTransactionMismatch {
                    requested: [0xd5; 16],
                    recorded: [0xe5; 16],
                }
            )
        );
        let central_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let local_after = std::fs::read_dir(&local_path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(central_after, central_before);
        assert_eq!(local_after, local_before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn restart_loader_preserves_both_local_corruption_diagnostics() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-corrupt-local-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let prepared = PreparedRecord::new_linux(
            [0xd7; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d7d7d7d7d7d7d7d7d7d7d7d7d7d7d7d7"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &prepared).unwrap();
        let claim_intent = ClaimIntentRecord::from_prepared(&prepared).unwrap();
        central.persist_claim_intent(&claim_intent).unwrap();
        std::fs::write(local_path.join("journal"), b"not-json").unwrap();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd7; 16]);

        assert!(matches!(
            loaded,
            Err(LinuxLoadClaimIntentRestartError::LocalCorrupt {
                claim_intent: JournalError::InvalidJson(_),
                prepared: JournalError::InvalidJson(_),
            })
        ));
        assert_eq!(
            ClaimIntentRecord::parse(
                &std::fs::read(central_path.join("d7d7d7d7d7d7d7d7d7d7d7d7d7d7d7d7.journal"))
                    .unwrap()
            )
            .unwrap(),
            claim_intent
        );
        assert_eq!(
            std::fs::read(local_path.join("journal")).unwrap(),
            b"not-json"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn restart_loader_rejects_a_different_local_transaction_without_mutation() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-mismatched-transaction-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let central_prepared = PreparedRecord::new_linux(
            [0xd6; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central
            .prepare_with_local(&local, &central_prepared)
            .unwrap();
        central
            .persist_claim_intent(&ClaimIntentRecord::from_prepared(&central_prepared).unwrap())
            .unwrap();
        let different_local = PreparedRecord::new_linux(
            [0xe6; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6e6"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        local.persist_local_prepared(&different_local).unwrap();
        let central_before =
            std::fs::read(central_path.join("d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6.journal")).unwrap();
        let local_before = std::fs::read(local_path.join("journal")).unwrap();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd6; 16]);

        assert_eq!(
            loaded,
            Err(LinuxLoadClaimIntentRestartError::LocalTransactionMismatch {
                central: [0xd6; 16],
                local: [0xe6; 16],
            })
        );
        assert_eq!(
            std::fs::read(central_path.join("d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6d6.journal")).unwrap(),
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
    fn restart_loader_returns_both_records_for_same_transaction_prepared_mismatch() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-mismatched-prepared-evidence-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let anchored = PreparedRecord::new_linux(
            [0xd8; 16],
            &root,
            Path::new("anchored-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d8d8d8d8d8d8d8d8d8d8d8d8d8d8d8d8"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &anchored).unwrap();
        let durable_central = ClaimIntentRecord::from_prepared(&anchored).unwrap();
        central.persist_claim_intent(&durable_central).unwrap();
        let substituted_local = PreparedRecord::new_linux(
            [0xd8; 16],
            &root,
            Path::new("substituted-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 99, LinuxFileType::Regular),
            Path::new(".ottrin-move-d8d8d8d8d8d8d8d8d8d8d8d8d8d8d8d8"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        local.persist_local_prepared(&substituted_local).unwrap();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd8; 16]);

        assert_eq!(
            loaded,
            Err(LinuxLoadClaimIntentRestartError::PreparedEvidenceMismatch {
                durable_central: Box::new(durable_central),
                local: Box::new(substituted_local),
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn restart_loader_returns_both_records_for_same_transaction_claim_mismatch() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-load-mismatched-claim-evidence-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let anchored = PreparedRecord::new_linux(
            [0xd9; 16],
            &root,
            Path::new("anchored-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &anchored).unwrap();
        let durable_central = ClaimIntentRecord::from_prepared(&anchored).unwrap();
        central.persist_claim_intent(&durable_central).unwrap();
        let substituted = PreparedRecord::new_linux(
            [0xd9; 16],
            &root,
            Path::new("substituted-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 99, LinuxFileType::Regular),
            Path::new(".ottrin-move-d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let substituted_local = ClaimIntentRecord::from_prepared(&substituted).unwrap();
        local
            .persist_local_claim_intent(&substituted_local)
            .unwrap();

        let loaded = central.load_claim_intent_restart_state(&local, [0xd9; 16]);

        assert_eq!(
            loaded,
            Err(
                LinuxLoadClaimIntentRestartError::ClaimIntentEvidenceMismatch {
                    durable_central: Box::new(durable_central),
                    local: Box::new(substituted_local),
                }
            )
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_rejects_a_second_context_without_changing_journals() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-recovery-lock-contention-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let first_context = LinuxJournalDirectory::open(&central_path).unwrap();
        let second_context = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let transaction_id = [0xda; 16];
        let prepared = PreparedRecord::new_linux(
            transaction_id,
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-dadadadadadadadadadadadadadadada"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        first_context.prepare_with_local(&local, &prepared).unwrap();
        let central_journal = central_path.join("dadadadadadadadadadadadadadadada.journal");
        let local_journal = local_path.join("journal");
        let central_before = std::fs::read(&central_journal).unwrap();
        let local_before = std::fs::read(&local_journal).unwrap();

        let held = first_context.try_lock_recovery().unwrap();
        let contended = second_context.try_lock_recovery();

        assert!(matches!(contended, Err(LinuxRecoveryLockError::Contended)));
        assert_eq!(std::fs::read(&central_journal).unwrap(), central_before);
        assert_eq!(std::fs::read(&local_journal).unwrap(), local_before);
        drop(held);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    fn spawn_recovery_lock_holder(central_path: &Path, root: &Path) -> std::process::Child {
        use std::process::Command;
        use std::time::{Duration, Instant};

        let ready_path = root.join("child-ready");
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("move_recovery::tests::recovery_lock_child_process_helper")
            .arg("--nocapture")
            .env("OTTRIN_RECOVERY_LOCK_CHILD_ROOT", central_path)
            .env("OTTRIN_RECOVERY_LOCK_CHILD_READY", &ready_path)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_path.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("recovery-lock child exited before readiness: {status}");
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("recovery-lock child did not signal readiness");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        child
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_child_process_helper() {
        let Some(root) = std::env::var_os("OTTRIN_RECOVERY_LOCK_CHILD_ROOT") else {
            return;
        };
        let ready = std::env::var_os("OTTRIN_RECOVERY_LOCK_CHILD_READY").unwrap();
        let central = LinuxJournalDirectory::open(Path::new(&root)).unwrap();
        let _held = central.try_lock_recovery().unwrap();
        std::fs::write(ready, b"locked").unwrap();
        loop {
            std::thread::park();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_is_released_after_a_lock_holding_process_is_killed() {
        use std::os::unix::process::ExitStatusExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-recovery-lock-process-death-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        std::fs::create_dir_all(&central_path).unwrap();
        let journal_path = central_path.join("existing.journal");
        std::fs::write(&journal_path, b"must not change").unwrap();
        let central_entries_before = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();

        let mut child = spawn_recovery_lock_holder(&central_path, &root);
        let (result_sender, result_receiver) = std::sync::mpsc::channel();
        let contender_path = central_path.clone();
        let contender = std::thread::spawn(move || {
            let central = LinuxJournalDirectory::open(&contender_path).unwrap();
            result_sender
                .send(central.try_lock_recovery().map(drop))
                .unwrap();
        });
        let contended = result_receiver.recv_timeout(std::time::Duration::from_secs(1));
        child.kill().unwrap();
        let killed = child.wait().unwrap();
        contender.join().unwrap();

        assert!(matches!(
            contended,
            Ok(Err(LinuxRecoveryLockError::Contended))
        ));
        assert_eq!(killed.signal(), Some(9));

        let reacquired = central.try_lock_recovery();
        assert!(reacquired.is_ok());
        assert_eq!(std::fs::read(&journal_path).unwrap(), b"must not change");
        let central_entries_after = std::fs::read_dir(&central_path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(central_entries_after, central_entries_before);
        drop(reacquired);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn configured_central_root_opening_stays_fail_closed_across_path_replacement() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-configured-central-root-replacement-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let configured = root.join("moves");
        let displaced = root.join("moves-displaced");
        std::fs::create_dir_all(&configured).unwrap();

        let first = LinuxJournalDirectory::open_configured(&configured);
        std::fs::rename(&configured, &displaced).unwrap();
        std::fs::create_dir(&configured).unwrap();
        let replacement = LinuxJournalDirectory::open_configured(&configured);

        assert!(matches!(
            first,
            Err(LinuxCentralRootOpenError::StableIdentityAuthorityUnavailable)
        ));
        assert!(matches!(
            replacement,
            Err(LinuxCentralRootOpenError::StableIdentityAuthorityUnavailable)
        ));
        assert!(std::fs::read_dir(&configured).unwrap().next().is_none());
        assert!(std::fs::read_dir(&displaced).unwrap().next().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_rejects_reacquisition_through_the_same_context() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-recovery-lock-same-context-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let context = LinuxJournalDirectory::open(&root).unwrap();

        let held = context.try_lock_recovery().unwrap();
        let contended = context.try_lock_recovery();

        assert!(matches!(contended, Err(LinuxRecoveryLockError::Contended)));
        drop(held);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_lock_is_released_when_its_raii_token_is_dropped() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-recovery-lock-release-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        std::fs::create_dir_all(&root).unwrap();
        let first_context = LinuxJournalDirectory::open(&root).unwrap();
        let second_context = LinuxJournalDirectory::open(&root).unwrap();

        let held = first_context.try_lock_recovery().unwrap();
        drop(held);
        let reacquired = second_context.try_lock_recovery();

        assert!(reacquired.is_ok());
        drop(reacquired);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn claim_intent_advance_rejects_same_id_with_different_source_evidence() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-intent-substitution-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let persisted = PreparedRecord::new_linux(
            [0xd3; 16],
            &root,
            Path::new("real-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        let substituted = PreparedRecord::new_linux(
            [0xd3; 16],
            &root,
            Path::new("different-source"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 99, LinuxFileType::Regular),
            Path::new(".ottrin-move-d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &persisted).unwrap();

        let mut recovery = central.try_lock_recovery().unwrap();
        let result = recovery.advance_to_claim_intent_with_local(&local, &substituted);
        drop(recovery);

        assert_eq!(
            result,
            Err(LinuxAdvanceClaimIntentError::SuppliedPreparedMismatch)
        );
        assert_eq!(
            PreparedRecord::parse(
                &std::fs::read(central_path.join("d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3d3.journal"),)
                    .unwrap()
            )
            .unwrap(),
            persisted
        );
        assert_eq!(
            PreparedRecord::parse(&std::fs::read(local_path.join("journal")).unwrap()).unwrap(),
            persisted
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_claim_intent_failure_returns_exact_durable_central_evidence() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "ottrin-move-claim-intent-partial-{}",
            encode_hex(&getrandom::u64().unwrap().to_ne_bytes())
        ));
        let central_path = root.join("central");
        let local_path = root.join("local");
        let source_path = root.join("source-item");
        std::fs::create_dir_all(&central_path).unwrap();
        std::fs::create_dir_all(&local_path).unwrap();
        std::fs::write(&source_path, b"must remain").unwrap();
        let central = LinuxJournalDirectory::open(&central_path).unwrap();
        let local = LinuxJournalDirectory::open(&local_path).unwrap();
        let record = PreparedRecord::new_linux(
            [0xd2; 16],
            &root,
            Path::new("source-item"),
            LinuxStableIdentity::new(7, 41, LinuxFileType::Directory),
            LinuxStableIdentity::new(7, 43, LinuxFileType::Regular),
            Path::new(".ottrin-move-d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2"),
            LinuxStableIdentity::new(7, 97, LinuxFileType::Directory),
        )
        .unwrap();
        central.prepare_with_local(&local, &record).unwrap();
        let expected = ClaimIntentRecord::from_prepared(&record).unwrap();
        std::fs::set_permissions(&local_path, std::fs::Permissions::from_mode(0o500)).unwrap();

        let mut recovery = central.try_lock_recovery().unwrap();
        let result = recovery.advance_to_claim_intent_with_local(&local, &record);

        let Err(LinuxAdvanceClaimIntentError::CentralDurableLocalUpdateUncertain {
            error: JournalError::Io(_),
            durable_central,
        }) = result
        else {
            panic!("local persistence failure must expose exact durable central evidence");
        };
        assert_eq!(*durable_central, expected);
        let central_record = ClaimIntentRecord::parse(
            &std::fs::read(central_path.join("d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2d2.journal")).unwrap(),
        )
        .unwrap();
        let local_record =
            PreparedRecord::parse(&std::fs::read(local_path.join("journal")).unwrap()).unwrap();
        assert_eq!(central_record, expected);
        assert_eq!(local_record, record);
        assert_eq!(std::fs::read(&source_path).unwrap(), b"must remain");
        std::fs::set_permissions(&local_path, std::fs::Permissions::from_mode(0o700)).unwrap();

        assert_eq!(
            recovery.advance_to_claim_intent_with_local(&local, &record),
            Ok(LinuxClaimIntentJournals::MatchingPair)
        );
        let completed_local =
            ClaimIntentRecord::parse(&std::fs::read(local_path.join("journal")).unwrap()).unwrap();
        assert_eq!(completed_local, expected);
        assert_eq!(std::fs::read(&source_path).unwrap(), b"must remain");
        drop(recovery);
        std::fs::remove_dir_all(root).unwrap();
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
