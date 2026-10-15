use ottrin_core::{
    ConflictAction, DeleteMode, EntryKind, FileCommand, PrivilegedRequest, PrivilegedResponse,
};
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::ffi::OsString;
#[cfg(target_os = "linux")]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;
use thiserror::Error;

#[cfg(target_os = "linux")]
pub use copy_transaction::RetainedCopyTransaction;
#[cfg(target_os = "linux")]
mod copy_recovery;
#[cfg(target_os = "linux")]
pub use copy_recovery::{
    CopyOperation, CopyPhase, CopyRecoveryEvidence, RetainedCopyCleanupPlan, RetainedCopyStatus,
    RetainedCopyUndoPlan,
};

/// Reads retained Copy status without creating recovery data or granting Undo authority.
#[cfg(target_os = "linux")]
pub fn discover_retained_copy_transactions_at(
    root: &Path,
) -> Result<Vec<RetainedCopyStatus>, PlatformError> {
    copy_recovery::discover_at(root)
}

#[cfg(target_os = "linux")]
pub fn discover_retained_copy_transactions() -> Result<Vec<RetainedCopyStatus>, PlatformError> {
    copy_recovery::discover_at(&copy_recovery::configured_root()?)
}

/// Restores a guarded receipt from the configured Copy registry without changing
/// data or journals. Undo revalidates evidence and acquires its transaction lock.
#[cfg(target_os = "linux")]
pub fn restore_retained_copy_undo_receipt(
    transaction_id: &str,
) -> Result<PathMoveUndoReceipt, PlatformError> {
    copy_recovery::restore_receipt(transaction_id)
}
/// Admits one restored Copy operation without changing its data or journals.
/// The plan binds private observations from discovery and lazily restores
/// reverse-order authority without rereading unrelated operations.
#[cfg(target_os = "linux")]
pub fn restore_retained_copy_undo_plan(
    captured: &[RetainedCopyStatus],
) -> Result<RetainedCopyUndoPlan, PlatformError> {
    copy_recovery::restore_plan(captured)
}
/// Read-only admission for discarding retained Copy data. Visible publications
/// may have changed; the original retained objects, journals and dependencies
/// must still match. This plan alone grants no deletion authority.
#[cfg(target_os = "linux")]
pub fn plan_retained_copy_cleanup(
    captured: &[RetainedCopyStatus],
) -> Result<RetainedCopyCleanupPlan, PlatformError> {
    copy_recovery::plan_cleanup(captured)
}
mod copy_transaction;
#[cfg(target_os = "linux")]
mod directory_watch;
#[cfg(target_os = "linux")]
use copy_transaction::CopyPublication;
#[cfg(not(target_os = "linux"))]
use copy_transaction::copy_entry_transactional;
pub mod move_recovery;
mod move_transaction;
use move_transaction::move_entry_transactional;
#[cfg(target_os = "linux")]
pub mod local_storage;
pub mod text_edit;

/// Lists retained Linux Move journals without creating recovery state or attempting cleanup.
#[cfg(target_os = "linux")]
pub fn discover_retained_move_transactions_at(
    recovery_root: &Path,
) -> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    move_transaction::discover_retained_move_transactions_at(recovery_root)
}

/// Lists retained Linux Move journals at Ottrin's configured state-home recovery root.
///
/// This is a read-only status facade: it never creates the configured state home, recovery
/// root, journal, anchor, or transaction directory, and never attempts cleanup.
#[cfg(target_os = "linux")]
pub fn discover_retained_move_transactions()
-> Result<Vec<move_recovery::LinuxDiscoveredMoveTransaction>, PlatformError> {
    move_transaction::discover_retained_move_transactions()
}

/// Lists retained Windows Move journals without creating recovery state or attempting cleanup.
#[cfg(target_os = "windows")]
pub fn discover_retained_move_transactions_at(
    recovery_root: &Path,
) -> Result<Vec<move_recovery::WindowsDiscoveredMoveTransaction>, PlatformError> {
    move_transaction::discover_retained_windows_move_transactions_at(recovery_root)
}

/// Lists retained Windows Move journals at Ottrin's configured state-home recovery root.
#[cfg(target_os = "windows")]
pub fn discover_retained_move_transactions()
-> Result<Vec<move_recovery::WindowsDiscoveredMoveTransaction>, PlatformError> {
    move_transaction::discover_retained_windows_move_transactions()
}

#[derive(Debug, Clone)]
pub struct FileProperties {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size_bytes: u64,
    pub readonly: bool,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDisposition {
    Moved,
    Skipped,
}

/// Keeps every admitted Undo receipt even when a later Copy stops the batch.
#[derive(Debug)]
pub struct CopyBatchResult {
    pub completed_sources: usize,
    pub undo_receipts: Vec<PathMoveUndoReceipt>,
    pub error: Option<PlatformError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashOriginBinding {
    /// The source root proven by native Trash metadata. This is deliberately
    /// absent when the native implementation only exposes a mutable original
    /// pathname, rather than a durable source-volume identity.
    pub root: PathBuf,
    pub stable_identity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TrashItem {
    pub id: OsString,
    pub original_name: OsString,
    pub original_parent: PathBuf,
    pub trash_path: PathBuf,
    pub original_path: PathBuf,
    pub deletion_date: Option<String>,
    pub time_deleted: i64,
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: Option<u64>,
    /// Native or durable provenance for the original storage. `None` means an
    /// item must not be assumed to belong to a currently mounted volume just
    /// because that volume now occupies its old pathname.
    pub origin_storage: Option<TrashOriginBinding>,
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("operation is not available on this platform: {0}")]
    NotAvailable(&'static str),
    #[error("filesystem operation failed: {0}")]
    Io(String),
    #[error("invalid source path: {0}")]
    InvalidSource(String),
    #[error("destination must be an existing directory: {0}")]
    InvalidDestination(String),
    #[error("destination entry already exists: {0}")]
    DestinationExists(String),
    #[error("filesystem undo receipt is invalid: {0}")]
    InvalidPathMoveUndoReceipt(String),
    #[error("Copy cleanup refused: {0}")]
    CopyCleanupRefused(String),
    #[error("undo target is missing or no longer the same entry: {0}")]
    StalePathMoveUndo(String),
    #[error("contents changed since this operation: {0}")]
    UndoContentChanged(String),
    #[error("undo completed but recovery cleanup is pending at {transaction}: {message}")]
    CommittedUndoCleanupPending {
        transaction: String,
        message: String,
    },
    #[error("permission undo target is missing, replaced, or changed again: {0}")]
    StaleChmodUndo(String),
    #[error("safe copy is unsupported on this platform: {0}")]
    UnsupportedSafeCopy(String),
    #[error("safe overwrite of an existing entry is unsupported on this platform: {0}")]
    UnsupportedSafeOverwrite(String),
    #[error("safe cross-device move cleanup is unsupported on this platform: {0}")]
    UnsupportedSafeCrossDeviceMove(String),
    #[error("handle-anchored safe move is unsupported on this platform: {0}")]
    UnsupportedSafeMove(String),
    #[error("filesystem operation committed, but cleanup is pending at {transaction}: {message}")]
    CommittedCleanupPending {
        transaction: String,
        message: String,
    },
    #[error("move source is retained for recovery before publication at {transaction}: {message}")]
    ClaimedRecoveryPending {
        transaction: String,
        message: String,
    },
    #[error("copy failed and transaction cleanup is pending at {transaction}: {message}")]
    CleanupPending {
        transaction: String,
        message: String,
    },
    #[error(
        "batch completed {completed} of {total} items, then stopped at {stopped_source}: {message}"
    )]
    PartialBatch {
        completed: usize,
        total: usize,
        stopped_source: PathBuf,
        recovery_paths: Vec<PathBuf>,
        completed_dispositions: Vec<MoveDisposition>,
        message: String,
    },
}

/// Validates a user-provided entry name before combining it with a parent path.
/// FileCommand can also reach the platform through the privileged helper, so
/// this boundary must not rely on UI-only validation to contain a creation.
pub fn validate_creation_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Enter a non-empty name.".to_string());
    }
    let candidate = Path::new(name);
    let has_drive_prefix =
        name.len() >= 2 && name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[1] == b':';
    if matches!(name, "." | "..")
        || name.contains('\0')
        || name.contains('/')
        || name.contains('\\')
        || candidate.is_absolute()
        || has_drive_prefix
        || candidate.components().count() != 1
    {
        return Err("Names must be a single path component.".to_string());
    }
    #[cfg(target_os = "windows")]
    if is_windows_reserved_creation_name(name) {
        return Err("That name is reserved or invalid on Windows.".to_string());
    }
    Ok(())
}

/// Windows namespace rules are platform-independent syntax, so tests and UI
/// preflight can use this predicate on every host while execution enforces it
/// when targeting Windows.
pub fn is_windows_reserved_creation_name(name: &str) -> bool {
    if name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|ch| ch < ' ' || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return true;
    }
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].into_iter().any(|prefix| {
            matches!(
                base.strip_prefix(prefix),
                Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
            )
        })
}

/// Link-entry identity captured for a reversible Linux pathname publication.
///
/// On Linux this is `symlink_metadata`'s device and inode pair, so it binds a
/// symlink entry rather than the symlink target. It is intentionally opaque to
/// callers: only this crate may compare it before an inverse rename.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(target_os = "linux", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(target_os = "linux", serde(deny_unknown_fields))]
pub struct PathMoveIdentity {
    #[cfg(target_os = "linux")]
    device: u64,
    #[cfg(target_os = "linux")]
    inode: u64,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathMoveUndoRequirement {
    None,
    EmptyFile,
    EmptyDirectory,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct CrossFilesystemUndoState {
    restored_identity: PathMoveIdentity,
    restoration_transaction: PathBuf,
    destination_transaction: Option<PathBuf>,
}

/// Session-only proof required to attempt one inverse of a completed pathname publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMoveUndoReceipt {
    before_path: PathBuf,
    after_path: PathBuf,
    identity: PathMoveIdentity,
    displaced_identity: Option<PathMoveIdentity>,
    retained_displaced_path: Option<PathBuf>,
    requirement: PathMoveUndoRequirement,
    #[cfg(target_os = "linux")]
    published_content: Option<move_recovery::SourceTreeSnapshot>,
    #[cfg(target_os = "linux")]
    retained_displaced_content: Option<move_recovery::SourceTreeSnapshot>,
    #[cfg(target_os = "linux")]
    cross_filesystem_state: std::sync::Arc<std::sync::OnceLock<CrossFilesystemUndoState>>,
    #[cfg(target_os = "linux")]
    retained_copy_directory: Option<RetainedCopyTransaction>,
}

/// Session-only proof for restoring a Linux permission change to the exact
/// unchanged non-symlink entry. The retained descriptor prevents an inverse
/// from ever chmodding a pathname replacement.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct ChmodUndoReceipt {
    path: PathBuf,
    identity: PathMoveIdentity,
    before_mode: u32,
    after_mode: u32,
    #[cfg(target_os = "linux")]
    descriptor: std::sync::Arc<std::os::fd::OwnedFd>,
}

impl ChmodUndoReceipt {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Session-only proof for restoring the exact Linux home-Trash payload created
/// by one completed delete. Mounted-volume and Windows Trash entries do not
/// expose enough durable origin evidence to construct this receipt.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct TrashUndoReceipt {
    item: TrashItem,
    identity: PathMoveIdentity,
}

impl TrashUndoReceipt {
    pub fn original_path(&self) -> &Path {
        &self.item.original_path
    }

    pub fn trash_path(&self) -> &Path {
        &self.item.trash_path
    }
}

impl PathMoveUndoReceipt {
    pub fn before_path(&self) -> &Path {
        &self.before_path
    }

    pub fn after_path(&self) -> &Path {
        &self.after_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLocationKind {
    Internal,
    Removable,
    Optical,
    Network,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLocation {
    pub path: PathBuf,
    /// Discovery key for this currently visible root. It is suitable for labels
    /// and same-snapshot bookkeeping, but may be a mutable device node or drive
    /// letter and therefore is never sufficient to reconnect an unavailable tab.
    pub identity: String,
    /// Stable volume/share identity when the platform can prove one. `None`
    /// means this root must not be automatically reconnected after removal.
    pub stable_identity: Option<String>,
    pub kind: StorageLocationKind,
    /// User-facing volume or mapped-share name, when the platform can provide one.
    pub display_name: Option<String>,
}

/// Discovers user-visible filesystem roots for direct navigation only.
///
/// Callers must opt locations into indexing separately; attaching storage never changes
/// indexed-search configuration.
pub fn discover_storage_locations() -> Vec<StorageLocation> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/mounts")
            .map(|mounts| discover_linux_storage_locations_from_mounts(&mounts))
            .unwrap_or_default()
    }
    #[cfg(target_os = "windows")]
    {
        discover_windows_storage_locations()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
const LINUX_PSEUDO_FILESYSTEM_TYPES: [&str; 16] = [
    "autofs",
    "binfmt_misc",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "mqueue",
    "proc",
    "pstore",
    "securityfs",
    "selinuxfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

#[cfg(target_os = "linux")]
fn discover_linux_storage_locations_from_mounts(mounts: &str) -> Vec<StorageLocation> {
    let mut locations = Vec::new();
    let mut seen = HashSet::new();
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let source = fields.next().unwrap_or_default();
        let mount_point = fields.next().unwrap_or_default();
        let filesystem = fields.next().unwrap_or_default();
        if source.is_empty()
            || mount_point.is_empty()
            || LINUX_PSEUDO_FILESYSTEM_TYPES.contains(&filesystem)
        {
            continue;
        }
        let path = PathBuf::from(unescape_linux_mount_path(mount_point));
        if !(path.starts_with("/media")
            || path.starts_with("/mnt")
            || path.starts_with("/run/media"))
            || !seen.insert(path.clone())
        {
            continue;
        }
        let (identity, stable_identity) = linux_storage_identity(source);
        locations.push(StorageLocation {
            kind: classify_linux_storage_kind(&path, source, filesystem),
            path,
            identity,
            stable_identity,
            display_name: None,
        });
    }
    locations
}

#[cfg(target_os = "linux")]
fn linux_storage_identity(source: &str) -> (String, Option<String>) {
    let source_path = Path::new(source);
    if source_path.starts_with("/dev/")
        && let Ok(canonical_source) = std::fs::canonicalize(source_path)
        && let Ok(entries) = std::fs::read_dir("/dev/disk/by-uuid")
    {
        for entry in entries.flatten() {
            if std::fs::canonicalize(entry.path()).ok().as_ref() == Some(&canonical_source) {
                let identity = format!("linux-uuid:{}", entry.file_name().to_string_lossy());
                return (identity.clone(), Some(identity));
            }
        }
    }
    // Network sources and unresolved block devices retain a conservative source
    // identity rather than incorrectly merging distinct locations.
    (format!("linux-source:{source}"), None)
}

#[cfg(target_os = "linux")]
fn unescape_linux_mount_path(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(target_os = "linux")]
fn classify_linux_storage_kind(path: &Path, source: &str, filesystem: &str) -> StorageLocationKind {
    if matches!(
        filesystem,
        "cifs" | "nfs" | "nfs4" | "smbfs" | "sshfs" | "davfs"
    ) || source.starts_with("//")
    {
        StorageLocationKind::Network
    } else if source.starts_with("/dev/sr") || matches!(filesystem, "iso9660" | "udf") {
        StorageLocationKind::Optical
    } else if path.starts_with("/media") || path.starts_with("/run/media") {
        StorageLocationKind::Removable
    } else {
        StorageLocationKind::Internal
    }
}

#[cfg(target_os = "windows")]
fn discover_windows_storage_locations() -> Vec<StorageLocation> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};

    const REMOVABLE: u32 = 2;
    const FIXED: u32 = 3;
    const REMOTE: u32 = 4;
    const CDROM: u32 = 5;
    let drive_mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter_map(|index| {
            ((drive_mask & (1 << index)) != 0).then(|| {
                let drive_identity = format!("{}:", (b'A' + index as u8) as char);
                let path = PathBuf::from(format!("{drive_identity}\\"));
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
                let kind = match unsafe { GetDriveTypeW(wide.as_ptr()) } {
                    REMOVABLE => StorageLocationKind::Removable,
                    FIXED => StorageLocationKind::Internal,
                    REMOTE => StorageLocationKind::Network,
                    CDROM => StorageLocationKind::Optical,
                    _ => return None,
                };
                let volume_label = windows_volume_label(&wide);
                let volume_serial = windows_volume_serial(&wide);
                let drive_identity_wide: Vec<u16> =
                    drive_identity.encode_utf16().chain(once(0)).collect();
                let mapped_share = (kind == StorageLocationKind::Network)
                    .then(|| windows_mapped_share(&drive_identity_wide))
                    .flatten();
                let stable_identity = mapped_share
                    .as_ref()
                    .map(|share| format!("windows-share:{share}"))
                    .or_else(|| volume_serial.map(|serial| format!("windows-volume:{serial:08x}")));
                // Navigation uses canonical Windows paths (including verbatim prefixes).
                // Keep unavailable drive slots visible, but bind ready roots in that same form.
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                Some(StorageLocation {
                    path,
                    identity: stable_identity
                        .clone()
                        .unwrap_or_else(|| format!("windows-drive:{drive_identity}")),
                    stable_identity,
                    kind,
                    display_name: Some(windows_storage_display_name(
                        volume_label.as_deref(),
                        mapped_share.as_deref(),
                        &drive_identity,
                    )),
                })
            })?
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_volume_label(root: &[u16]) -> Option<String> {
    let mut label = [0_u16; 261];
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW(
            root.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    let terminator = label.iter().position(|&unit| unit == 0)?;
    let label = String::from_utf16_lossy(&label[..terminator]);
    (!label.trim().is_empty()).then_some(label)
}

#[cfg(target_os = "windows")]
fn windows_volume_serial(root: &[u16]) -> Option<u32> {
    let mut serial = 0_u32;
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    (ok != 0).then_some(serial)
}

#[cfg(target_os = "windows")]
fn windows_mapped_share(drive_root: &[u16]) -> Option<String> {
    let mut remote_name = vec![0_u16; 32_768];
    let mut length = remote_name.len() as u32;
    let result = unsafe {
        windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW(
            drive_root.as_ptr(),
            remote_name.as_mut_ptr(),
            &mut length,
        )
    };
    (result == 0)
        .then(|| String::from_utf16_lossy(&remote_name[..length as usize]))
        .filter(|name| !name.trim().is_empty())
}

#[cfg(any(target_os = "windows", test))]
fn windows_storage_display_name(
    volume_label: Option<&str>,
    mapped_share: Option<&str>,
    drive_identity: &str,
) -> String {
    volume_label
        .filter(|label| !label.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            mapped_share
                .map(str::trim)
                .filter(|share| !share.is_empty())
                .map(|share| share.trim_start_matches('\\').to_string())
        })
        .unwrap_or_else(|| drive_identity.to_string())
}

#[cfg(all(test, target_os = "windows"))]
#[test]
fn windows_storage_discovery_binds_canonical_entry_paths() {
    let entry = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let locations = discover_storage_locations();
    let location = locations
        .iter()
        .find(|location| entry.starts_with(&location.path))
        .expect("canonical application paths must bind to their discovered storage root");
    assert!(location.stable_identity.is_some());
    assert_eq!(
        std::fs::canonicalize(&location.path).unwrap(),
        location.path
    );
}

#[cfg(all(test, target_os = "linux"))]
mod storage_location_tests {
    use super::*;

    #[test]
    fn windows_storage_display_name_prefers_volume_or_share_over_drive_identity() {
        assert_eq!(
            windows_storage_display_name(Some("Archive"), None, "D:"),
            "Archive"
        );
        assert_eq!(
            windows_storage_display_name(None, Some("\\\\files.example\\team"), "P:"),
            "files.example\\team"
        );
        assert_eq!(windows_storage_display_name(None, None, "E:"), "E:");
    }

    #[test]
    fn linux_storage_discovery_exposes_user_visible_mounts_without_home() {
        let locations = discover_linux_storage_locations_from_mounts(
            "/dev/sdb1 /media/alice/Backup ext4 rw 0 0\n\\
             server:/share /mnt/team nfs4 rw 0 0\n\\
             /dev/sr0 /run/media/alice/Install iso9660 ro 0 0\n\\
             proc /proc proc rw 0 0\n",
        );

        assert_eq!(
            locations,
            vec![
                StorageLocation {
                    path: PathBuf::from("/media/alice/Backup"),
                    identity: "linux-source:/dev/sdb1".to_string(),
                    stable_identity: None,
                    kind: StorageLocationKind::Removable,
                    display_name: None,
                },
                StorageLocation {
                    path: PathBuf::from("/mnt/team"),
                    identity: "linux-source:server:/share".to_string(),
                    stable_identity: None,
                    kind: StorageLocationKind::Network,
                    display_name: None,
                },
                StorageLocation {
                    path: PathBuf::from("/run/media/alice/Install"),
                    identity: "linux-source:/dev/sr0".to_string(),
                    stable_identity: None,
                    kind: StorageLocationKind::Optical,
                    display_name: None,
                },
            ]
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedAvailability {
    Ready,
    Misconfigured(String),
    Unsupported(String),
}

/// A display-safe application choice discovered for one exact target. The
/// target binding is lexical `PathBuf` equality, not inode or immutable-file
/// identity; desktop associations and the target can change within the
/// same-user desktop trust model. The native handler identity remains opaque,
/// so callers can show the name but cannot turn it into an arbitrary command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWithApplication {
    name: String,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    native_id: String,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    target: PathBuf,
}

impl OpenWithApplication {
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeOpenWithApplication {
    id: String,
    name: String,
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
impl NativeOpenWithApplication {
    fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn open_with_choices_from_native(
    target: &Path,
    applications: Vec<NativeOpenWithApplication>,
) -> Vec<OpenWithApplication> {
    use std::collections::BTreeMap;

    let mut unique = BTreeMap::new();
    for application in applications {
        if !application.id.is_empty() && !application.name.is_empty() {
            unique.entry(application.id).or_insert(application.name);
        }
    }
    let mut choices = unique
        .into_iter()
        .map(|(native_id, name)| OpenWithApplication {
            name,
            native_id,
            target: target.to_path_buf(),
        })
        .collect::<Vec<_>>();
    choices.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.native_id.cmp(&right.native_id))
    });
    choices
}

#[cfg(any(target_os = "windows", test))]
fn select_native_open_with_handler<T>(
    handlers: Vec<(NativeOpenWithApplication, T)>,
    selected_id: &str,
    launch: impl FnOnce(T) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let (_, handler) = handlers
        .into_iter()
        .find(|(descriptor, _)| descriptor.id == selected_id)
        .ok_or_else(|| {
            PlatformError::InvalidSource(
                "Open With choice is no longer discovered for this target".into(),
            )
        })?;
    launch(handler)
}

/// Lists native Open With choices.  Platforms without safe native discovery
/// deliberately return no choices so the UI retains its manual picker.
pub fn discover_open_with_applications(
    target: &Path,
) -> Result<Vec<OpenWithApplication>, PlatformError> {
    #[cfg(target_os = "linux")]
    {
        linux_open_with::discover(target)
    }
    #[cfg(target_os = "windows")]
    {
        windows_open_with::discover(target)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = target;
        Ok(Vec::new())
    }
}

pub trait PlatformOps: Send + Sync {
    fn delete_path(&self, target: &Path, mode: DeleteMode) -> Result<(), PlatformError>;
    fn reveal_in_system(&self, target: &Path) -> Result<(), PlatformError>;
    fn open_with_application(&self, target: &Path, application: &Path)
    -> Result<(), PlatformError>;
    fn execute_command(
        &self,
        command: &FileCommand,
    ) -> Result<Option<FileProperties>, PlatformError>;
    fn execute_command_with_move_dispositions(
        &self,
        command: &FileCommand,
    ) -> Result<(Option<FileProperties>, Vec<MoveDisposition>), PlatformError> {
        self.execute_command(command)
            .map(|result| (result, Vec::new()))
    }
    /// Moves one entry to Trash and returns an inverse receipt only when the
    /// backend exposes the exact payload plus proven original-storage context.
    fn execute_trash_with_undo_receipt(
        &self,
        target: &Path,
    ) -> Result<Option<TrashUndoReceipt>, PlatformError> {
        self.delete_path(target, DeleteMode::Trash).map(|_| None)
    }
    fn undo_trash_with_receipt(&self, receipt: &TrashUndoReceipt) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only Trash undo is supported only for proven Linux home-Trash entries",
        ))
    }
    /// Performs one same-parent Rename and returns an inverse receipt only
    /// when the link entry has the same identity before and after the rename.
    fn execute_rename_with_undo_receipt(
        &self,
        source: &Path,
        new_name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::Rename {
            source: source.to_path_buf(),
            new_name: new_name.to_string(),
        })
        .map(|_| None)
    }
    /// Attempts the receipt's inverse with the same atomic no-replace primitive
    /// as forward Rename. The identity preflight detects staleness but cannot
    /// make Linux's pathname rename an identity-conditional compare-and-swap.
    fn undo_rename_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only rename undo is supported only on Linux",
        ))
    }
    fn execute_create_file_with_undo_receipt(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::CreateFile {
            parent: parent.to_path_buf(),
            name: name.to_string(),
        })
        .map(|_| None)
    }
    fn undo_create_file_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only create file undo is supported only on Linux",
        ))
    }
    fn execute_create_folder_with_undo_receipt(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::CreateFolder {
            parent: parent.to_path_buf(),
            name: name.to_string(),
        })
        .map(|_| None)
    }
    fn undo_create_folder_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only create folder undo is supported only on Linux",
        ))
    }
    fn execute_symlink_with_undo_receipt(
        &self,
        link_path: &Path,
        target: &Path,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::Symlink {
            link_path: link_path.to_path_buf(),
            target: target.to_path_buf(),
        })
        .map(|_| None)
    }
    fn undo_symlink_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only symlink undo is supported only on Linux",
        ))
    }
    fn execute_chmod_with_undo_receipt(
        &self,
        target: &Path,
        mode_str: &str,
    ) -> Result<Option<ChmodUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::Chmod {
            target: target.to_path_buf(),
            mode_str: mode_str.to_string(),
        })
        .map(|_| None)
    }
    fn undo_chmod_with_receipt(&self, receipt: &ChmodUndoReceipt) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only chmod undo is supported only on Linux",
        ))
    }
    fn execute_copy_with_undo_receipt(
        &self,
        source: &Path,
        destination: &Path,
        conflict: ConflictAction,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        self.execute_command(&FileCommand::Copy {
            sources: vec![source.to_path_buf()],
            destination: destination.to_path_buf(),
            conflict,
        })
        .map(|_| None)
    }
    fn execute_copy_batch_with_undo_receipts(
        &self,
        sources: &[PathBuf],
        destination: &Path,
        conflict: ConflictAction,
    ) -> CopyBatchResult {
        execute_copy_batch_with(sources, |source| {
            self.execute_copy_with_undo_receipt(source, destination, conflict)
        })
    }
    fn undo_copy_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only copy undo is supported only on Linux",
        ))
    }
    fn execute_move_with_undo_receipt(
        &self,
        source: &Path,
        destination: &Path,
        conflict: ConflictAction,
    ) -> Result<(MoveDisposition, Option<PathMoveUndoReceipt>), PlatformError> {
        move_entry_transactional(source, destination, conflict)
            .map(|publication| (publication.disposition, None))
    }
    fn undo_move_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        let _ = receipt;
        Err(PlatformError::NotAvailable(
            "session-only move undo is supported only on Linux",
        ))
    }
    fn execute_privileged(
        &self,
        request: &PrivilegedRequest,
    ) -> Result<PrivilegedResponse, PlatformError>;
    fn privileged_availability(&self) -> PrivilegedAvailability;
}

#[derive(Debug, Default)]
pub struct DefaultPlatform;

static PRIV_HELPER_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn helper_override_store() -> &'static RwLock<Option<PathBuf>> {
    PRIV_HELPER_OVERRIDE.get_or_init(|| RwLock::new(None))
}

pub fn set_privileged_helper_override(path: Option<PathBuf>) {
    if let Ok(mut p) = helper_override_store().write() {
        *p = path;
    }
}

#[cfg(target_os = "linux")]
mod helper_setup;

/// Install the package's helper through the desktop's normal administrator prompt.
pub fn install_privileged_helper() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let directory = exe
            .parent()
            .ok_or("Cannot locate this Ottrin installation")?;
        let helper = [directory.join("../libexec/ottrin/ottrin-priv-helper"), directory.join("ottrin-priv-helper")]
            .into_iter().find(|path| path.is_file())
            .ok_or("This Ottrin package does not contain the administrator helper. Download a complete package.")?;
        helper_setup::install(&helper)
    }
    #[cfg(not(target_os = "linux"))]
    Err("Helper installation is managed by the installer on this platform.".to_string())
}

fn resolve_helper_name(default_name: &str) -> String {
    if let Ok(p) = helper_override_store().read()
        && let Some(path) = p.as_ref()
    {
        return path.display().to_string();
    }
    if let Ok(path) = std::env::var("OTTRIN_PRIV_HELPER") {
        return path;
    }
    if let Some(found) = auto_discover_helper(default_name) {
        return found.display().to_string();
    }
    default_name.to_string()
}

fn auto_discover_helper(default_name: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        // Portable/bundled layout: helper next to app.
        candidates.push(dir.join(default_name));
        // Linux distro layouts.
        #[cfg(target_os = "linux")]
        {
            candidates.push(dir.join("../libexec/ottrin").join(default_name));
            candidates.push(dir.join("../lib/ottrin").join(default_name));
            candidates.push(PathBuf::from("/usr/libexec/ottrin").join(default_name));
            candidates.push(PathBuf::from("/usr/lib/ottrin").join(default_name));
            candidates.push(PathBuf::from("/usr/libexec").join(default_name));
        }
        // Windows installer layouts.
        #[cfg(target_os = "windows")]
        {
            candidates.push(dir.join("helpers").join(default_name));
            candidates.push(dir.join("bin").join(default_name));
        }
    }

    candidates.into_iter().find(|p| p.is_file())
}

impl PlatformOps for DefaultPlatform {
    fn delete_path(&self, target: &Path, mode: DeleteMode) -> Result<(), PlatformError> {
        match mode {
            DeleteMode::Trash => {
                trash::delete(target).map_err(|e| PlatformError::Io(e.to_string()))
            }
            DeleteMode::Permanent => delete_permanent(target),
        }
    }

    fn reveal_in_system(&self, target: &Path) -> Result<(), PlatformError> {
        opener::open(target).map_err(|e| PlatformError::Io(e.to_string()))
    }

    fn open_with_application(
        &self,
        target: &Path,
        application: &Path,
    ) -> Result<(), PlatformError> {
        Command::new(application)
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| PlatformError::Io(e.to_string()))
    }

    fn execute_command(
        &self,
        command: &FileCommand,
    ) -> Result<Option<FileProperties>, PlatformError> {
        match command {
            FileCommand::CreateFile { parent, name } => {
                ensure_existing_dir(parent)?;
                validate_creation_name(name).map_err(PlatformError::Io)?;
                let target = parent.join(name);
                std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&target)
                    .map_err(|e| PlatformError::Io(e.to_string()))?;
                Ok(None)
            }
            FileCommand::CreateFolder { parent, name } => {
                ensure_existing_dir(parent)?;
                validate_creation_name(name).map_err(PlatformError::Io)?;
                let target = parent.join(name);
                std::fs::create_dir(&target).map_err(|e| PlatformError::Io(e.to_string()))?;
                Ok(None)
            }
            FileCommand::Rename { source, new_name } => {
                rename_in_same_parent(source, new_name)?;
                Ok(None)
            }
            FileCommand::Delete { targets, mode } => {
                for target in targets {
                    self.delete_path(target, *mode)?;
                }
                Ok(None)
            }
            FileCommand::Copy {
                sources,
                destination,
                conflict,
            } => {
                let result =
                    self.execute_copy_batch_with_undo_receipts(sources, destination, *conflict);
                if let Some(error) = result.error {
                    return Err(error);
                }
                if !result.undo_receipts.is_empty() {
                    return Err(PlatformError::CommittedCleanupPending {
                        transaction: result.undo_receipts.iter()
                            .filter_map(|receipt| receipt.before_path().parent())
                            .map(|path| path.display().to_string()).collect::<Vec<_>>().join(", "),
                        message: "copy completed; the caller did not retain its Undo receipts and their private transaction stages remain".into(),
                    });
                }
                Ok(None)
            }
            FileCommand::Move {
                sources,
                destination,
                conflict,
            } => {
                ensure_existing_dir(destination)?;
                for source in sources {
                    validate_copy_source(source)?;
                    reject_directory_copy_into_itself(source, destination)?;
                }
                execute_move_batch(sources, |source| {
                    move_entry_transactional(source, destination, *conflict)
                        .map(|publication| publication.disposition)
                })?;
                Ok(None)
            }
            #[cfg(target_os = "linux")]
            FileCommand::RecoverRetainedMove { transaction_id } => {
                move_transaction::recover_retained_move_pending_destination(*transaction_id)?;
                Ok(None)
            }
            #[cfg(target_os = "windows")]
            FileCommand::RecoverRetainedMove { transaction_id } => {
                move_transaction::recover_retained_windows_move_transaction(*transaction_id)?;
                Ok(None)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            FileCommand::RecoverRetainedMove { .. } => Err(PlatformError::NotAvailable(
                "Retained Move recovery is supported only on Linux and Windows",
            )),
            FileCommand::ShowProperties { target } => {
                let props = read_properties(target)?;
                Ok(Some(props))
            }
            FileCommand::Chmod { target, mode_str } => {
                #[cfg(target_os = "linux")]
                let _ = chmod_with_undo_receipt_linux(target, mode_str)?;
                #[cfg(not(target_os = "linux"))]
                chmod_path(target, mode_str).map_err(PlatformError::Io)?;
                Ok(None)
            }
            FileCommand::Symlink { link_path, target } => {
                create_symlink(link_path, target).map_err(PlatformError::Io)?;
                Ok(None)
            }
        }
    }

    fn execute_command_with_move_dispositions(
        &self,
        command: &FileCommand,
    ) -> Result<(Option<FileProperties>, Vec<MoveDisposition>), PlatformError> {
        match command {
            FileCommand::Move {
                sources,
                destination,
                conflict,
            } => {
                ensure_existing_dir(destination)?;
                for source in sources {
                    validate_copy_source(source)?;
                    reject_directory_copy_into_itself(source, destination)?;
                }
                let completed_dispositions = execute_move_batch(sources, |source| {
                    move_entry_transactional(source, destination, *conflict)
                        .map(|publication| publication.disposition)
                })?;
                Ok((None, completed_dispositions))
            }
            _ => Ok((self.execute_command(command)?, Vec::new())),
        }
    }

    fn execute_trash_with_undo_receipt(
        &self,
        target: &Path,
    ) -> Result<Option<TrashUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            let original_path = canonical_trash_source_path(target)?;
            let before_identity = rename_identity(&original_path)
                .map_err(|_| PlatformError::InvalidSource(target.display().to_string()))?;
            self.delete_path(&original_path, DeleteMode::Trash)?;
            Ok(exact_trash_undo_receipt(
                &original_path,
                &before_identity,
                list_trash_items(),
            ))
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.delete_path(target, DeleteMode::Trash).map(|_| None)
        }
    }

    fn undo_trash_with_receipt(&self, receipt: &TrashUndoReceipt) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            let current_identity = rename_identity(&receipt.item.trash_path).map_err(|_| {
                PlatformError::StalePathMoveUndo(receipt.item.trash_path.display().to_string())
            })?;
            if current_identity != receipt.identity {
                return Err(PlatformError::StalePathMoveUndo(
                    receipt.item.trash_path.display().to_string(),
                ));
            }
            match std::fs::symlink_metadata(&receipt.item.original_path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(PlatformError::DestinationExists(
                        receipt.item.original_path.display().to_string(),
                    ));
                }
                Err(error) => return Err(PlatformError::Io(error.to_string())),
            }
            restore_trash_item(&receipt.item)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only Trash undo is supported only for proven Linux home-Trash entries",
            ))
        }
    }

    fn execute_rename_with_undo_receipt(
        &self,
        source: &Path,
        new_name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            let before_identity = rename_identity(source)
                .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
            let after_path = rename_in_same_parent(source, new_name)?;
            let after_identity = rename_identity(&after_path).ok();
            Ok(after_identity
                .filter(|identity| identity == &before_identity)
                .map(|identity| PathMoveUndoReceipt {
                    before_path: source.to_path_buf(),
                    after_path,
                    identity,
                    displaced_identity: None,
                    retained_displaced_path: None,
                    requirement: PathMoveUndoRequirement::None,
                    published_content: None,
                    retained_displaced_content: None,
                    cross_filesystem_state: Default::default(),
                    retained_copy_directory: None,
                }))
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.execute_command(&FileCommand::Rename {
                source: source.to_path_buf(),
                new_name: new_name.to_string(),
            })
            .map(|_| None)
        }
    }

    fn undo_rename_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            if receipt.before_path.parent() != receipt.after_path.parent() {
                return Err(PlatformError::InvalidPathMoveUndoReceipt(
                    "rename undo must remain in the same lexical parent".to_string(),
                ));
            }
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only rename undo is supported only on Linux",
            ))
        }
    }

    fn execute_create_file_with_undo_receipt(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            ensure_existing_dir(parent)?;
            validate_creation_name(name).map_err(PlatformError::Io)?;
            let publication = copy_transaction::create_empty_file(parent, name)?;
            let CopyPublication::Published {
                retained_transaction,
                retained_directory,
                published_path,
                published_identity,
                ..
            } = publication
            else {
                return Ok(None);
            };
            undo_receipt_for_published_identity(
                retained_transaction,
                published_path,
                published_identity,
                None,
                PathMoveUndoRequirement::EmptyFile,
                "file creation",
                Some(retained_directory),
            )
            .map(Some)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.execute_command(&FileCommand::CreateFile {
                parent: parent.to_path_buf(),
                name: name.to_string(),
            })
            .map(|_| None)
        }
    }

    fn undo_create_file_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only create file undo is supported only on Linux",
            ))
        }
    }

    fn execute_create_folder_with_undo_receipt(
        &self,
        parent: &Path,
        name: &str,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            ensure_existing_dir(parent)?;
            validate_creation_name(name).map_err(PlatformError::Io)?;
            let publication = copy_transaction::create_empty_directory(parent, name)?;
            let CopyPublication::Published {
                retained_transaction,
                retained_directory,
                published_path,
                published_identity,
                ..
            } = publication
            else {
                return Ok(None);
            };
            undo_receipt_for_published_identity(
                retained_transaction,
                published_path,
                published_identity,
                None,
                PathMoveUndoRequirement::EmptyDirectory,
                "folder creation",
                Some(retained_directory),
            )
            .map(Some)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.execute_command(&FileCommand::CreateFolder {
                parent: parent.to_path_buf(),
                name: name.to_string(),
            })
            .map(|_| None)
        }
    }

    fn undo_create_folder_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only create folder undo is supported only on Linux",
            ))
        }
    }

    fn execute_symlink_with_undo_receipt(
        &self,
        link_path: &Path,
        target: &Path,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            let parent = link_path.parent().ok_or_else(|| {
                PlatformError::InvalidDestination(link_path.display().to_string())
            })?;
            let name = link_path.file_name().ok_or_else(|| {
                PlatformError::InvalidDestination(link_path.display().to_string())
            })?;
            ensure_existing_dir(parent)?;
            let publication = copy_transaction::create_symlink(parent, name, target)?;
            let CopyPublication::Published {
                retained_transaction,
                retained_directory,
                published_path,
                published_identity,
                ..
            } = publication
            else {
                return Ok(None);
            };
            undo_receipt_for_published_identity(
                retained_transaction,
                published_path,
                published_identity,
                None,
                PathMoveUndoRequirement::None,
                "symlink creation",
                Some(retained_directory),
            )
            .map(Some)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.execute_command(&FileCommand::Symlink {
                link_path: link_path.to_path_buf(),
                target: target.to_path_buf(),
            })
            .map(|_| None)
        }
    }

    fn undo_symlink_with_receipt(
        &self,
        receipt: &PathMoveUndoReceipt,
    ) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only symlink undo is supported only on Linux",
            ))
        }
    }

    fn execute_chmod_with_undo_receipt(
        &self,
        target: &Path,
        mode_str: &str,
    ) -> Result<Option<ChmodUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            chmod_with_undo_receipt_linux(target, mode_str).map(Some)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.execute_command(&FileCommand::Chmod {
                target: target.to_path_buf(),
                mode_str: mode_str.to_string(),
            })
            .map(|_| None)
        }
    }

    fn undo_chmod_with_receipt(&self, receipt: &ChmodUndoReceipt) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            undo_chmod_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only chmod undo is supported only on Linux",
            ))
        }
    }

    fn execute_copy_with_undo_receipt(
        &self,
        source: &Path,
        destination: &Path,
        conflict: ConflictAction,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            self.copy_with_operation(source, destination, conflict, None)
        }
        #[cfg(not(target_os = "linux"))]
        {
            ensure_existing_dir(destination)?;
            validate_copy_source(source)?;
            reject_directory_copy_into_itself(source, destination)?;
            copy_entry_transactional(source, destination, conflict).map(|_| None)
        }
    }

    fn execute_copy_batch_with_undo_receipts(
        &self,
        sources: &[PathBuf],
        destination: &Path,
        conflict: ConflictAction,
    ) -> CopyBatchResult {
        let validation = ensure_existing_dir(destination).and_then(|()| {
            for source in sources {
                validate_copy_source(source)?;
                reject_directory_copy_into_itself(source, destination)?;
            }
            Ok(())
        });
        if let Err(error) = validation {
            return CopyBatchResult {
                completed_sources: 0,
                undo_receipts: Vec::new(),
                error: Some(error),
            };
        }
        #[cfg(target_os = "linux")]
        {
            if sources.is_empty() {
                return CopyBatchResult {
                    completed_sources: 0,
                    undo_receipts: Vec::new(),
                    error: None,
                };
            }
            let mut operation = match copy_recovery::CopyOperation::new_batch(sources.len()) {
                Ok(operation) => operation,
                Err(error) => {
                    return CopyBatchResult {
                        completed_sources: 0,
                        undo_receipts: Vec::new(),
                        error: Some(error),
                    };
                }
            };
            execute_copy_batch_with(sources, |source| {
                let result =
                    self.copy_with_operation(source, destination, conflict, Some(&operation));
                operation.item_index += 1;
                result
            })
        }
        #[cfg(not(target_os = "linux"))]
        execute_copy_batch_with(sources, |source| {
            self.execute_copy_with_undo_receipt(source, destination, conflict)
        })
    }
    fn undo_copy_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            if receipt.retained_copy_directory.is_none() {
                return Err(PlatformError::InvalidPathMoveUndoReceipt(
                    "Copy receipt has no retained directory ownership".into(),
                ));
            }
            if receipt.published_content.is_none() {
                return Err(PlatformError::InvalidPathMoveUndoReceipt(
                    "Copy receipt has no published content evidence".into(),
                ));
            }
            if receipt.displaced_identity.is_some() && receipt.retained_displaced_content.is_none()
            {
                return Err(PlatformError::InvalidPathMoveUndoReceipt(
                    "Copy receipt has no displaced content evidence".into(),
                ));
            }
            if let Some(directory) = &receipt.retained_copy_directory
                && directory.undo_durable_copy(receipt)?
            {
                return Ok(());
            }
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only copy undo is supported only on Linux",
            ))
        }
    }

    fn execute_move_with_undo_receipt(
        &self,
        source: &Path,
        destination: &Path,
        conflict: ConflictAction,
    ) -> Result<(MoveDisposition, Option<PathMoveUndoReceipt>), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            let publication = move_entry_transactional(source, destination, conflict)?;
            let receipt = match publication.published_path.as_ref() {
                None => None,
                Some(after_path) => {
                    let observed = rename_identity(after_path).map_err(|error| {
                        PlatformError::CommittedCleanupPending {
                            transaction: publication
                                .retained_displaced_path
                                .as_ref()
                                .and_then(|path| path.parent())
                                .unwrap_or(after_path)
                                .display()
                                .to_string(),
                            message: format!(
                                "Move completed, but its published entry identity could not be verified: {error}"
                            ),
                        }
                    })?;

                    let displaced_identity = publication
                        .retained_displaced_path
                        .as_ref()
                        .map(|path| {
                            rename_identity(path).map_err(|error| {
                                PlatformError::CommittedCleanupPending {
                                    transaction: path
                                        .parent()
                                        .unwrap_or(path)
                                        .display()
                                        .to_string(),
                                    message: format!(
                                        "Move completed, but its retained displaced destination identity could not be verified: {error}"
                                    ),
                                }
                            })
                        })
                        .transpose()?;
                    let published_content = publication.published_content.clone();
                    Some(PathMoveUndoReceipt {
                        before_path: source.to_path_buf(),
                        after_path: after_path.clone(),
                        identity: observed,
                        displaced_identity,
                        retained_displaced_path: publication.retained_displaced_path.clone(),
                        requirement: PathMoveUndoRequirement::None,
                        published_content,
                        retained_displaced_content: None,
                        cross_filesystem_state: Default::default(),
                        retained_copy_directory: None,
                    })
                }
            };
            Ok((publication.disposition, receipt))
        }
        #[cfg(not(target_os = "linux"))]
        {
            move_entry_transactional(source, destination, conflict)
                .map(|publication| (publication.disposition, None))
        }
    }

    fn undo_move_with_receipt(&self, receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            undo_path_move_linux(receipt)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = receipt;
            Err(PlatformError::NotAvailable(
                "session-only move undo is supported only on Linux",
            ))
        }
    }

    fn execute_privileged(
        &self,
        request: &PrivilegedRequest,
    ) -> Result<PrivilegedResponse, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            execute_privileged_linux(request)
        }
        #[cfg(target_os = "windows")]
        {
            execute_privileged_windows(request)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = request;
            Ok(PrivilegedResponse {
                status: ottrin_core::PrivilegedStatus::Unsupported,
                message: Some(
                    "Privileged helper integration is not implemented on this OS yet.".to_string(),
                ),
                payload: None,
            })
        }
    }

    fn privileged_availability(&self) -> PrivilegedAvailability {
        #[cfg(target_os = "linux")]
        {
            if !is_executable_available("pkexec") {
                return PrivilegedAvailability::Misconfigured(
                    "Administrator authentication is unavailable. Install polkit using your system’s software manager.".to_string(),
                );
            }
            let helper = resolve_helper_name("ottrin-priv-helper");
            if !is_executable_available(&helper) {
                return PrivilegedAvailability::Misconfigured(
                    "Administrator helper is not installed. Open Services to set it up."
                        .to_string(),
                );
            }
            if helper == "/usr/libexec/ottrin/ottrin-priv-helper"
                && std::fs::read("/usr/share/polkit-1/actions/org.ottrin.filesystem.policy")
                    .ok()
                    .as_deref()
                    != Some(
                        include_bytes!("../../../packaging/linux/org.ottrin.filesystem.policy")
                            .as_slice(),
                    )
            {
                return PrivilegedAvailability::Misconfigured("Administrator helper needs its authentication policy. Open Services to repair setup.".to_string());
            }
            PrivilegedAvailability::Ready
        }
        #[cfg(target_os = "windows")]
        {
            if !is_executable_available("powershell.exe") && !is_executable_available("powershell")
            {
                return PrivilegedAvailability::Misconfigured(
                    "PowerShell not found; UAC launcher unavailable".to_string(),
                );
            }
            let helper = resolve_helper_name("ottrin-priv-helper.exe");
            if !is_executable_available(&helper) {
                return PrivilegedAvailability::Misconfigured(format!(
                    "Privileged helper '{}' not found (set OTTRIN_PRIV_HELPER or use --helper-path)",
                    helper
                ));
            }
            PrivilegedAvailability::Ready
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            PrivilegedAvailability::Unsupported(
                "Integrated privilege management is not implemented on this platform.".to_string(),
            )
        }
    }
}

impl DefaultPlatform {
    #[cfg(target_os = "linux")]
    fn copy_with_operation(
        &self,
        source: &Path,
        destination: &Path,
        conflict: ConflictAction,
        operation: Option<&copy_recovery::CopyOperation>,
    ) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
        ensure_existing_dir(destination)?;
        validate_copy_source(source)?;
        reject_directory_copy_into_itself(source, destination)?;
        let publication =
            copy_transaction::copy_entry_for_operation(source, destination, conflict, operation)?;
        let (
            retained_transaction,
            after_path,
            published_identity,
            published_content,
            displaced_content,
            displaced_identity,
            retained_directory,
        ) = match publication {
            CopyPublication::Published {
                retained_transaction,
                retained_directory,
                published_path,
                published_identity,
                published_content,
                displaced_content,
                displaced_identity,
            } => (
                retained_transaction,
                published_path,
                published_identity,
                published_content,
                displaced_content,
                displaced_identity,
                retained_directory,
            ),
            CopyPublication::Skipped {
                retained_transaction: None,
            } => return Ok(None),
            CopyPublication::Skipped {
                retained_transaction: Some(transaction),
            } => {
                return Err(PlatformError::CommittedCleanupPending {
                        transaction: transaction.display().to_string(),
                        message: "copy was skipped; its private staged candidate was retained because pathname-based cleanup cannot prove ownership".into(),
                    });
            }
        };
        undo_receipt_for_published_identity(
            retained_transaction,
            after_path,
            published_identity,
            displaced_identity,
            PathMoveUndoRequirement::None,
            "copy",
            Some(retained_directory),
        )
        .map(|mut receipt| {
            receipt.published_content = published_content;
            receipt.retained_displaced_content = displaced_content;
            Some(receipt)
        })
    }

    /// Launches only a descriptor freshly discoverable for this exact target.
    /// This intentionally delegates desktop-entry interpretation to GIO rather
    /// than interpreting an `Exec` field. A successful return means GIO accepted
    /// the launch request, not that the application fully opened.
    pub fn open_with_discovered_application(
        &self,
        target: &Path,
        application: &OpenWithApplication,
    ) -> Result<(), PlatformError> {
        #[cfg(target_os = "linux")]
        {
            if application.target != target {
                return Err(PlatformError::InvalidSource(
                    "Open With choice belongs to another target".into(),
                ));
            }
            linux_open_with::launch_selected(target, &application.native_id)
        }
        #[cfg(target_os = "windows")]
        {
            if application.target != target {
                return Err(PlatformError::InvalidSource(
                    "Open With choice belongs to another target".into(),
                ));
            }
            windows_open_with::launch_selected(target, &application.native_id)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = (target, application);
            Err(PlatformError::NotAvailable("native Open With discovery"))
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_open_with {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, IDataObject,
    };
    use windows::Win32::UI::Shell::{
        ASSOC_FILTER_NONE, BHID_DataObject, IAssocHandler, IShellItem, SHAssocEnumHandlers,
        SHCreateItemFromParsingName,
    };
    use windows::core::{PCWSTR, PWSTR};

    struct ComApartment(bool);

    impl ComApartment {
        fn enter() -> Result<Self, PlatformError> {
            let result =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            if result.is_ok() {
                Ok(Self(true))
            } else if result == RPC_E_CHANGED_MODE {
                Ok(Self(false))
            } else {
                Err(PlatformError::Io(format!(
                    "could not initialize Windows Open With discovery: {result:?}"
                )))
            }
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    fn wide_null(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn owned_shell_string(value: PWSTR) -> Result<String, PlatformError> {
        if value.is_null() {
            return Ok(String::new());
        }
        let converted =
            unsafe { value.to_string() }.map_err(|error| PlatformError::Io(error.to_string()));
        unsafe { CoTaskMemFree(Some(value.as_ptr().cast())) };
        converted
    }

    fn descriptor(handler: &IAssocHandler) -> Result<NativeOpenWithApplication, PlatformError> {
        let id = owned_shell_string(
            unsafe { handler.GetName() }.map_err(|error| PlatformError::Io(error.to_string()))?,
        )?;
        let name = owned_shell_string(
            unsafe { handler.GetUIName() }.map_err(|error| PlatformError::Io(error.to_string()))?,
        )?;
        Ok(NativeOpenWithApplication::new(id, name))
    }

    fn extension_for(target: &Path) -> Option<Vec<u16>> {
        let extension = target.extension()?;
        if extension.is_empty() {
            return None;
        }
        let mut dotted = std::ffi::OsString::from(".");
        dotted.push(extension);
        Some(wide_null(&dotted))
    }

    fn native_handlers(
        target: &Path,
    ) -> Result<Vec<(NativeOpenWithApplication, IAssocHandler)>, PlatformError> {
        let Some(extension) = extension_for(target) else {
            return Ok(Vec::new());
        };
        let enumerator = unsafe {
            SHAssocEnumHandlers(PCWSTR(extension.as_ptr()), ASSOC_FILTER_NONE)
                .map_err(|error| PlatformError::Io(error.to_string()))?
        };
        let mut handlers = Vec::new();
        loop {
            let mut slot = [None];
            unsafe {
                enumerator
                    .Next(&mut slot, None)
                    .map_err(|error| PlatformError::Io(error.to_string()))?;
            }
            let Some(handler) = slot[0].take() else {
                break;
            };
            let descriptor = descriptor(&handler)?;
            // The Shell enumeration is the native filtered Open With source.
            // Reject malformed registrations before exposing them to the UI.
            if !descriptor.id.is_empty() && !descriptor.name.is_empty() {
                handlers.push((descriptor, handler));
            }
        }
        Ok(handlers)
    }

    pub(super) fn discover(target: &Path) -> Result<Vec<OpenWithApplication>, PlatformError> {
        let _apartment = ComApartment::enter()?;
        let applications = native_handlers(target)?
            .into_iter()
            .map(|(descriptor, _)| descriptor)
            .collect();
        Ok(open_with_choices_from_native(target, applications))
    }

    pub(super) fn launch_selected(target: &Path, selected_id: &str) -> Result<(), PlatformError> {
        let _apartment = ComApartment::enter()?;
        select_native_open_with_handler(native_handlers(target)?, selected_id, |handler| {
            let path = wide_null(target.as_os_str());
            let item: IShellItem = unsafe {
                SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
            };
            let data: IDataObject = unsafe {
                item.BindToHandler(None, &BHID_DataObject)
                    .map_err(|error| PlatformError::Io(error.to_string()))?
            };
            unsafe {
                handler
                    .Invoke(&data)
                    .map_err(|error| PlatformError::Io(error.to_string()))
            }
        })
    }
}

#[cfg(target_os = "linux")]
mod linux_open_with {
    use super::*;
    use gio::prelude::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct NativeApplication {
        id: String,
        name: String,
    }

    impl NativeApplication {
        #[cfg(test)]
        pub(super) fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
            Self {
                id: id.into(),
                name: name.into(),
            }
        }
    }

    fn content_type_for(target: &Path) -> Result<String, PlatformError> {
        let file = gio::File::for_path(target);
        let info = file
            .query_info(
                "standard::content-type",
                gio::FileQueryInfoFlags::NONE,
                gio::Cancellable::NONE,
            )
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        info.content_type()
            .map(|value| value.to_string())
            .ok_or_else(|| PlatformError::InvalidSource(target.display().to_string()))
    }

    fn native_handlers(content_type: &str) -> Vec<(NativeApplication, gio::AppInfo)> {
        // GIO owns desktop-entry interpretation. Keep this one filtered
        // collection shared by discovery and the launch requery.
        gio::AppInfo::recommended_for_type(content_type)
            .into_iter()
            .chain(gio::AppInfo::all_for_type(content_type))
            .filter(|application| application.should_show())
            .filter_map(|application| {
                let id = application.id()?.to_string();
                let name = application.display_name().to_string();
                (!id.is_empty() && !name.is_empty())
                    .then_some((NativeApplication { id, name }, application))
            })
            .collect()
    }

    pub(super) fn choices_from_native(
        target: &Path,
        applications: Vec<NativeApplication>,
    ) -> Vec<OpenWithApplication> {
        open_with_choices_from_native(
            target,
            applications
                .into_iter()
                .map(|application| NativeOpenWithApplication::new(application.id, application.name))
                .collect(),
        )
    }

    pub(super) fn discover(target: &Path) -> Result<Vec<OpenWithApplication>, PlatformError> {
        let content_type = content_type_for(target)?;
        let applications = native_handlers(&content_type)
            .into_iter()
            .map(|(descriptor, _)| descriptor)
            .collect();
        Ok(choices_from_native(target, applications))
    }

    pub(super) fn select_and_launch<T>(
        applications: Vec<(NativeApplication, T)>,
        selected_id: &str,
        launch: impl FnOnce(T) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let (_, application) = applications
            .into_iter()
            .find(|(descriptor, _)| descriptor.id == selected_id)
            .ok_or_else(|| {
                PlatformError::InvalidSource(
                    "Open With choice is no longer discovered for this target".into(),
                )
            })?;
        launch(application)
    }

    pub(super) fn launch_selected(target: &Path, selected_id: &str) -> Result<(), PlatformError> {
        let content_type = content_type_for(target)?;
        select_and_launch(native_handlers(&content_type), selected_id, |application| {
            application
                .launch(&[gio::File::for_path(target)], gio::AppLaunchContext::NONE)
                .map_err(|error| PlatformError::Io(error.to_string()))
        })
    }
}

fn error_recovery_paths(error: &PlatformError) -> Vec<PathBuf> {
    match error {
        PlatformError::CleanupPending { transaction, .. }
        | PlatformError::ClaimedRecoveryPending { transaction, .. }
        | PlatformError::CommittedCleanupPending { transaction, .. } => {
            vec![PathBuf::from(transaction)]
        }
        PlatformError::PartialBatch { recovery_paths, .. } => recovery_paths.clone(),
        _ => Vec::new(),
    }
}

fn message_with_recovery(error: &PlatformError, recovery_paths: &[PathBuf]) -> String {
    if recovery_paths.is_empty() {
        error.to_string()
    } else {
        format!(
            "{}; retained transaction stages: {}",
            error,
            recovery_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

#[cfg(test)]
fn execute_batch(
    sources: &[PathBuf],
    mut execute: impl FnMut(&Path) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let total = sources.len();
    for (index, source) in sources.iter().enumerate() {
        if let Err(error) = execute(source) {
            let current_committed = matches!(
                error,
                PlatformError::CommittedCleanupPending { .. } | PlatformError::PartialBatch { .. }
            );
            let completed = index + usize::from(current_committed);
            if total > 1 && completed > 0 {
                let recovery_paths = error_recovery_paths(&error);
                return Err(PlatformError::PartialBatch {
                    completed,
                    total,
                    stopped_source: source.clone(),
                    completed_dispositions: Vec::new(),
                    message: message_with_recovery(&error, &recovery_paths),
                    recovery_paths,
                });
            }
            return Err(error);
        }
    }
    Ok(())
}

fn execute_move_batch(
    sources: &[PathBuf],
    mut execute: impl FnMut(&Path) -> Result<MoveDisposition, PlatformError>,
) -> Result<Vec<MoveDisposition>, PlatformError> {
    let total = sources.len();
    let mut completed_dispositions = Vec::with_capacity(total);
    for source in sources {
        match execute(source) {
            Ok(disposition) => {
                completed_dispositions.push(disposition);
            }
            Err(error) => {
                let current_committed = matches!(
                    error,
                    PlatformError::CommittedCleanupPending { .. }
                        | PlatformError::PartialBatch { .. }
                );
                let completed = completed_dispositions
                    .iter()
                    .filter(|disposition| **disposition == MoveDisposition::Moved)
                    .count()
                    + usize::from(current_committed);
                if current_committed {
                    completed_dispositions.push(MoveDisposition::Moved);
                }
                if total > 1 && completed > 0 {
                    let recovery_paths = error_recovery_paths(&error);
                    return Err(PlatformError::PartialBatch {
                        completed,
                        total,
                        stopped_source: source.clone(),
                        completed_dispositions: completed_dispositions.clone(),
                        message: message_with_recovery(&error, &recovery_paths),
                        recovery_paths,
                    });
                }
                return Err(error);
            }
        }
    }
    Ok(completed_dispositions)
}

fn execute_copy_batch_with(
    sources: &[PathBuf],
    mut execute: impl FnMut(&Path) -> Result<Option<PathMoveUndoReceipt>, PlatformError>,
) -> CopyBatchResult {
    let mut result = CopyBatchResult {
        completed_sources: 0,
        undo_receipts: Vec::new(),
        error: None,
    };
    for source in sources {
        match execute(source) {
            Ok(receipt) => {
                result.completed_sources += 1;
                result.undo_receipts.extend(receipt);
            }
            Err(error) => {
                let current_committed =
                    matches!(error, PlatformError::CommittedCleanupPending { .. });
                result.completed_sources += usize::from(current_committed);
                result.error = Some(if sources.len() > 1 && result.completed_sources > 0 {
                    let mut recovery_paths: Vec<PathBuf> = result
                        .undo_receipts
                        .iter()
                        .filter_map(|receipt| receipt.before_path().parent().map(Path::to_path_buf))
                        .collect();
                    recovery_paths.extend(error_recovery_paths(&error));
                    PlatformError::PartialBatch {
                        completed: result.completed_sources,
                        total: sources.len(),
                        stopped_source: source.clone(),
                        completed_dispositions: Vec::new(),
                        message: message_with_recovery(&error, &recovery_paths),
                        recovery_paths,
                    }
                } else {
                    error
                });
                break;
            }
        }
    }
    result
}

impl DefaultPlatform {
    pub fn list_trash(&self) -> Result<Vec<TrashItem>, PlatformError> {
        list_trash_items()
    }

    pub fn restore_trash_item(&self, item: &TrashItem) -> Result<(), PlatformError> {
        restore_trash_item(item)
    }

    pub fn delete_trash_item_permanently(&self, item: &TrashItem) -> Result<(), PlatformError> {
        delete_trash_item_permanently(item)
    }

    pub fn empty_trash(&self) -> Result<(), PlatformError> {
        empty_trash()
    }
}

fn is_executable_available(name: &str) -> bool {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        return candidate.is_file();
    }
    let path_var = match std::env::var_os("PATH") {
        Some(v) => v,
        None => return false,
    };
    for dir in std::env::split_paths(&path_var) {
        let full = dir.join(name);
        if full.is_file() {
            return true;
        }
        #[cfg(target_os = "windows")]
        {
            if full.extension().is_none() {
                for ext in ["exe", "cmd", "bat"] {
                    if dir.join(format!("{}.{}", name, ext)).is_file() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn execute_privileged_linux(
    request: &PrivilegedRequest,
) -> Result<PrivilegedResponse, PlatformError> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    let helper = resolve_helper_name("ottrin-priv-helper");
    let mut child = Command::new("pkexec")
        .arg(helper)
        .arg("--stdio-json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PlatformError::Io(e.to_string()))?;

    let req_bytes = serde_json::to_vec(request)
        .map_err(|e| PlatformError::Io(format!("serialize request failed: {}", e)))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(&req_bytes)
            .map_err(|e| PlatformError::Io(format!("write request failed: {}", e)))?;
    }

    let out = child
        .wait_with_output()
        .map_err(|e| PlatformError::Io(e.to_string()))?;
    if out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        if !out.status.success() {
            return Err(PlatformError::Io(if stderr.is_empty() {
                "privileged helper failed".to_string()
            } else {
                stderr
            }));
        }
        return Err(PlatformError::Io(
            "privileged helper returned no data".to_string(),
        ));
    }

    serde_json::from_slice::<PrivilegedResponse>(&out.stdout)
        .map_err(|e| PlatformError::Io(format!("decode response failed: {}", e)))
}

#[cfg(target_os = "windows")]
fn execute_privileged_windows(
    request: &PrivilegedRequest,
) -> Result<PrivilegedResponse, PlatformError> {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let helper = resolve_helper_name("ottrin-priv-helper.exe");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let req_path = std::env::temp_dir().join(format!("ottrin-priv-{}-{}.req.json", pid, nonce));
    let res_path = std::env::temp_dir().join(format!("ottrin-priv-{}-{}.res.json", pid, nonce));

    let req_bytes = serde_json::to_vec(request)
        .map_err(|e| PlatformError::Io(format!("serialize request failed: {}", e)))?;
    std::fs::write(&req_path, req_bytes)
        .map_err(|e| PlatformError::Io(format!("write request file failed: {}", e)))?;

    let ps_quote = |s: &str| s.replace('\'', "''");
    let helper_q = ps_quote(&helper);
    let req_q = ps_quote(&req_path.display().to_string());
    let res_q = ps_quote(&res_path.display().to_string());
    let ps = format!(
        "$p = Start-Process -FilePath '{}' -Verb RunAs -ArgumentList @('--request-file','{}','--response-file','{}') -PassThru -Wait; exit $p.ExitCode",
        helper_q, req_q, res_q
    );

    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .map_err(|e| PlatformError::Io(format!("failed to invoke UAC process: {}", e)))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let _ = std::fs::remove_file(&req_path);
        let _ = std::fs::remove_file(&res_path);
        return Err(PlatformError::Io(if stderr.is_empty() {
            "privileged helper process failed or canceled".to_string()
        } else {
            stderr
        }));
    }

    let bytes = std::fs::read(&res_path)
        .map_err(|e| PlatformError::Io(format!("read response file failed: {}", e)))?;
    let _ = std::fs::remove_file(&req_path);
    let _ = std::fs::remove_file(&res_path);
    serde_json::from_slice::<PrivilegedResponse>(&bytes)
        .map_err(|e| PlatformError::Io(format!("decode response failed: {}", e)))
}

fn ensure_existing_dir(path: &Path) -> Result<(), PlatformError> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(PlatformError::InvalidDestination(
            path.display().to_string(),
        ))
    }
}

fn rename_in_same_parent(source: &Path, new_name: &str) -> Result<PathBuf, PlatformError> {
    validate_creation_name(new_name).map_err(PlatformError::InvalidDestination)?;
    let parent = source
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    ensure_existing_dir(parent)?;
    let target = parent.join(new_name);
    rename_without_replacement(source, &target)?;
    Ok(target)
}

#[cfg(target_os = "linux")]
fn rename_identity(path: &Path) -> std::io::Result<PathMoveIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::symlink_metadata(path)?;
    Ok(PathMoveIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(target_os = "linux")]
fn undo_receipt_for_published_identity(
    retained_transaction: PathBuf,
    after_path: PathBuf,
    published_identity: PathMoveIdentity,
    displaced_identity: Option<PathMoveIdentity>,
    requirement: PathMoveUndoRequirement,
    operation: &str,
    retained_copy_directory: Option<RetainedCopyTransaction>,
) -> Result<PathMoveUndoReceipt, PlatformError> {
    let observed_identity =
        rename_identity(&after_path).map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: retained_transaction.display().to_string(),
            message: format!(
                "{operation} completed, but its published entry identity could not be verified: {error}"
            ),
        })?;
    if observed_identity != published_identity {
        return Err(PlatformError::CommittedCleanupPending {
            transaction: retained_transaction.display().to_string(),
            message: format!(
                "{operation} completed, but the destination entry was replaced before its undo receipt could be issued"
            ),
        });
    }
    if let Some(directory) = retained_copy_directory.as_ref() {
        directory.validate_path(&retained_transaction).and_then(|()| directory.validate_publication_parent(&after_path)).map_err(|error| PlatformError::CommittedCleanupPending {
            transaction: retained_transaction.display().to_string(),
            message: format!("{operation} completed, but its retained directory ownership changed before the Undo receipt could be issued: {error}"),
        })?;
    }
    let before_path = retained_transaction.join(copy_transaction::CANDIDATE);
    if let Some(expected_displaced) = displaced_identity.as_ref() {
        let observed_displaced = rename_identity(&before_path).map_err(|error| {
            PlatformError::CommittedCleanupPending {
                transaction: retained_transaction.display().to_string(),
                message: format!(
                    "{operation} completed, but its displaced destination identity could not be verified: {error}"
                ),
            }
        })?;
        if &observed_displaced != expected_displaced {
            return Err(PlatformError::CommittedCleanupPending {
                transaction: retained_transaction.display().to_string(),
                message: format!(
                    "{operation} completed, but its retained displaced destination was replaced before its undo receipt could be issued"
                ),
            });
        }
    }
    Ok(PathMoveUndoReceipt {
        before_path,
        after_path,
        identity: published_identity,
        displaced_identity,
        retained_displaced_path: None,
        requirement,
        published_content: None,
        retained_displaced_content: None,
        cross_filesystem_state: Default::default(),
        retained_copy_directory,
    })
}

#[cfg(target_os = "linux")]
fn validate_receipt_published_content(receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
    if let Some(expected) = receipt.published_content.as_ref() {
        validate_receipt_content(&receipt.after_path, expected)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn validate_receipt_content(
    path: &Path,
    expected: &move_recovery::SourceTreeSnapshot,
) -> Result<(), PlatformError> {
    let observed = move_transaction::source_tree_snapshot_for_path(path)
        .map_err(|_| PlatformError::StalePathMoveUndo(path.display().to_string()))?;
    if !expected.has_same_paths_and_content(&observed) {
        return Err(PlatformError::UndoContentChanged(
            path.display().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn undo_path_move_linux(receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
    if let Some(directory) = receipt.retained_copy_directory.as_ref() {
        directory.validate_path(receipt.before_path.parent().ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt(
                "receipt has no retained transaction parent".into(),
            )
        })?)?;
        directory.validate_publication_parent(&receipt.after_path)?;
    }

    if receipt.before_path == receipt.after_path
        || receipt.before_path.parent().is_none()
        || receipt.after_path.parent().is_none()
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "before and after paths must be distinct entries with parents".to_string(),
        ));
    }
    use std::os::unix::fs::MetadataExt;
    let before_parent = receipt.before_path.parent().expect("validated parent");
    let after_parent = receipt.after_path.parent().expect("validated parent");
    let before_device = std::fs::metadata(before_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .dev();
    let after_device = std::fs::metadata(after_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?
        .dev();
    if before_device != after_device {
        return undo_cross_filesystem_path_move_linux(receipt, before_parent);
    }
    if let (Some(displaced_identity), Some(retained_displaced_path)) = (
        receipt.displaced_identity.as_ref(),
        receipt.retained_displaced_path.as_ref(),
    ) {
        let inverse_already_completed = rename_identity(&receipt.before_path)
            .is_ok_and(|identity| identity == receipt.identity)
            && rename_identity(&receipt.after_path)
                .is_ok_and(|identity| &identity == displaced_identity)
            && std::fs::symlink_metadata(retained_displaced_path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        if inverse_already_completed {
            return cleanup_completed_overwrite_move_undo(retained_displaced_path);
        }
    }
    let observed_identity = rename_identity(&receipt.after_path)
        .map_err(|_| PlatformError::StalePathMoveUndo(receipt.after_path.display().to_string()))?;
    if observed_identity != receipt.identity {
        return Err(PlatformError::StalePathMoveUndo(
            receipt.after_path.display().to_string(),
        ));
    }
    // Read content only after confirming ownership of the publication entry.
    if receipt.retained_copy_directory.is_some() {
        validate_receipt_published_content(receipt)?;
    }
    match receipt.requirement {
        PathMoveUndoRequirement::None => {}
        PathMoveUndoRequirement::EmptyFile => {
            let metadata = std::fs::symlink_metadata(&receipt.after_path).map_err(|_| {
                PlatformError::StalePathMoveUndo(receipt.after_path.display().to_string())
            })?;
            if !metadata.is_file() || metadata.len() != 0 {
                return Err(PlatformError::StalePathMoveUndo(
                    receipt.after_path.display().to_string(),
                ));
            }
        }
        PathMoveUndoRequirement::EmptyDirectory => {
            let is_empty_directory = std::fs::symlink_metadata(&receipt.after_path)
                .is_ok_and(|metadata| metadata.is_dir())
                && std::fs::read_dir(&receipt.after_path)
                    .is_ok_and(|mut entries| entries.next().is_none());
            if !is_empty_directory {
                return Err(PlatformError::StalePathMoveUndo(
                    receipt.after_path.display().to_string(),
                ));
            }
        }
    }
    if let Some(displaced_identity) = receipt.displaced_identity.as_ref() {
        let displaced_path = receipt
            .retained_displaced_path
            .as_ref()
            .unwrap_or(&receipt.before_path);
        let observed_displaced = rename_identity(displaced_path)
            .map_err(|_| PlatformError::StalePathMoveUndo(displaced_path.display().to_string()))?;
        if &observed_displaced != displaced_identity {
            return Err(PlatformError::StalePathMoveUndo(
                displaced_path.display().to_string(),
            ));
        }
        if let Some(expected) = receipt.retained_displaced_content.as_ref() {
            validate_receipt_content(displaced_path, expected)?;
        }
        if receipt.retained_displaced_path.is_some() {
            match std::fs::symlink_metadata(&receipt.before_path) {
                Ok(_) => {
                    return Err(PlatformError::DestinationExists(
                        receipt.before_path.display().to_string(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(PlatformError::Io(error.to_string())),
            }
        }
        if let Some(directory) = receipt.retained_copy_directory.as_ref() {
            directory.withdraw_publication(&receipt.after_path, &receipt.before_path, true)?;
        } else {
            exchange_paths(&receipt.after_path, displaced_path)?;
        }
        if receipt.retained_displaced_path.is_some() {
            rename_without_replacement(displaced_path, &receipt.before_path)?;
            cleanup_completed_overwrite_move_undo(displaced_path)?;
        }
        Ok(())
    } else if let Some(directory) = receipt.retained_copy_directory.as_ref() {
        directory.withdraw_publication(&receipt.after_path, &receipt.before_path, false)
    } else {
        rename_without_replacement(&receipt.after_path, &receipt.before_path)
    }
}

#[cfg(target_os = "linux")]
fn undo_cross_filesystem_path_move_linux(
    receipt: &PathMoveUndoReceipt,
    before_parent: &Path,
) -> Result<(), PlatformError> {
    undo_cross_filesystem_path_move_linux_with_hook(receipt, before_parent, |_| Ok(()))
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CrossFilesystemUndoPoint {
    PlainSourceRestored,
    PlainDestinationQuarantined,
    OverwriteExchangeCompleted,
}

#[cfg(target_os = "linux")]
fn undo_cross_filesystem_path_move_linux_with_hook(
    receipt: &PathMoveUndoReceipt,
    before_parent: &Path,
    mut hook: impl FnMut(CrossFilesystemUndoPoint) -> Result<(), &'static str>,
) -> Result<(), PlatformError> {
    if receipt.displaced_identity.is_some() != receipt.retained_displaced_path.is_some() {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "cross-filesystem overwrite receipt must retain both displaced identity and path"
                .to_string(),
        ));
    }
    if let Some(state) = receipt.cross_filesystem_state.get() {
        return resume_cross_filesystem_path_move_undo(receipt, state);
    }
    let observed_identity = rename_identity(&receipt.after_path)
        .map_err(|_| PlatformError::StalePathMoveUndo(receipt.after_path.display().to_string()))?;
    if observed_identity != receipt.identity {
        return Err(PlatformError::StalePathMoveUndo(
            receipt.after_path.display().to_string(),
        ));
    }
    validate_receipt_published_content(receipt)?;
    match std::fs::symlink_metadata(&receipt.before_path) {
        Ok(_) => {
            return Err(PlatformError::DestinationExists(
                receipt.before_path.display().to_string(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(PlatformError::Io(error.to_string())),
    }

    if let (Some(displaced_identity), Some(retained_displaced_path)) = (
        receipt.displaced_identity.as_ref(),
        receipt.retained_displaced_path.as_ref(),
    ) {
        let observed_displaced = rename_identity(retained_displaced_path).map_err(|_| {
            PlatformError::StalePathMoveUndo(retained_displaced_path.display().to_string())
        })?;
        if &observed_displaced != displaced_identity {
            return Err(PlatformError::StalePathMoveUndo(
                retained_displaced_path.display().to_string(),
            ));
        }
    }

    let destination_transaction = if receipt.displaced_identity.is_none() {
        let after_parent = receipt.after_path.parent().expect("validated parent");
        Some(copy_transaction::create_retained_transaction(after_parent)?)
    } else {
        None
    };
    let restored = match copy_transaction::copy_entry_for_move_undo(
        &receipt.after_path,
        before_parent,
        ConflictAction::Skip,
    ) {
        Ok(restored) => restored,
        Err(error) => {
            if let Some(transaction) = destination_transaction.as_ref() {
                let _ = std::fs::remove_dir(transaction);
            }
            return Err(error);
        }
    };
    let CopyPublication::Published {
        retained_transaction,
        published_path,
        published_identity,
        ..
    } = restored
    else {
        if let Some(transaction) = destination_transaction.as_ref() {
            let _ = std::fs::remove_dir(transaction);
        }
        return Err(PlatformError::StalePathMoveUndo(
            receipt.before_path.display().to_string(),
        ));
    };
    if published_path != receipt.before_path {
        if let Some(transaction) = destination_transaction.as_ref() {
            let _ = std::fs::remove_dir(transaction);
        }
        return Err(PlatformError::StalePathMoveUndo(
            receipt.before_path.display().to_string(),
        ));
    }
    receipt
        .cross_filesystem_state
        .set(CrossFilesystemUndoState {
            restored_identity: published_identity,
            restoration_transaction: retained_transaction.clone(),
            destination_transaction,
        })
        .map_err(|_| {
            PlatformError::InvalidPathMoveUndoReceipt(
                "cross-filesystem undo state was initialized twice".to_string(),
            )
        })?;
    let state = receipt
        .cross_filesystem_state
        .get()
        .expect("cross-filesystem undo state was just initialized");
    if receipt.displaced_identity.is_none() {
        persist_plain_cross_filesystem_undo_intent(receipt, state)?;
        hook(CrossFilesystemUndoPoint::PlainSourceRestored).map_err(|message| {
            PlatformError::CommittedUndoCleanupPending {
                transaction: state
                    .destination_transaction
                    .as_ref()
                    .expect("plain undo has a destination transaction")
                    .display()
                    .to_string(),
                message: format!("undo interrupted after restoring the source: {message}"),
            }
        })?;
    }

    if let (Some(_), Some(retained_displaced_path)) = (
        receipt.displaced_identity.as_ref(),
        receipt.retained_displaced_path.as_ref(),
    ) {
        exchange_paths(&receipt.after_path, retained_displaced_path)?;
        hook(CrossFilesystemUndoPoint::OverwriteExchangeCompleted).map_err(|message| {
            PlatformError::CommittedUndoCleanupPending {
                transaction: retained_displaced_path.display().to_string(),
                message: format!("undo interrupted after overwrite exchange: {message}"),
            }
        })?;
        let observed_displaced = rename_identity(&receipt.after_path).map_err(|_| {
            PlatformError::CommittedUndoCleanupPending {
                transaction: retained_displaced_path.display().to_string(),
                message: "retained destination disappeared after undo exchange".to_string(),
            }
        })?;
        if Some(&observed_displaced) != receipt.displaced_identity.as_ref() {
            return Err(PlatformError::CommittedUndoCleanupPending {
                transaction: retained_displaced_path.display().to_string(),
                message: "retained destination changed during undo exchange".to_string(),
            });
        }
        let quarantine = retained_displaced_path.with_file_name("undo-source-quarantine");
        rename_without_replacement(retained_displaced_path, &quarantine)?;
        let quarantined_identity = rename_identity(&quarantine)
            .map_err(|_| PlatformError::StalePathMoveUndo(quarantine.display().to_string()))?;
        if quarantined_identity != receipt.identity {
            let _ = rename_without_replacement(&quarantine, retained_displaced_path);
            return Err(PlatformError::StalePathMoveUndo(
                retained_displaced_path.display().to_string(),
            ));
        }
        delete_permanent(&quarantine).map_err(|error| {
            PlatformError::CommittedUndoCleanupPending {
                transaction: quarantine
                    .parent()
                    .unwrap_or(&quarantine)
                    .display()
                    .to_string(),
                message: error.to_string(),
            }
        })?;
        cleanup_completed_overwrite_move_undo(retained_displaced_path)?;
    } else {
        let destination_transaction = state
            .destination_transaction
            .as_ref()
            .expect("plain undo has a destination transaction");
        let quarantine = destination_transaction.join("moved-source");
        rename_without_replacement(&receipt.after_path, &quarantine)?;
        hook(CrossFilesystemUndoPoint::PlainDestinationQuarantined).map_err(|message| {
            PlatformError::CommittedUndoCleanupPending {
                transaction: destination_transaction.display().to_string(),
                message: format!("undo interrupted after destination quarantine: {message}"),
            }
        })?;
        let quarantined_identity = rename_identity(&quarantine)
            .map_err(|_| PlatformError::StalePathMoveUndo(quarantine.display().to_string()))?;
        if quarantined_identity != receipt.identity {
            let _ = rename_without_replacement(&quarantine, &receipt.after_path);
            return Err(PlatformError::StalePathMoveUndo(
                receipt.after_path.display().to_string(),
            ));
        }
        delete_permanent(&quarantine)?;
        cleanup_plain_cross_filesystem_undo_transaction(destination_transaction)?;
    }
    match std::fs::remove_dir(&retained_transaction) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::CommittedUndoCleanupPending {
            transaction: retained_transaction.display().to_string(),
            message: error.to_string(),
        }),
    }
}

#[cfg(target_os = "linux")]
const PLAIN_CROSS_FILESYSTEM_UNDO_INTENT: &str = "undo-intent";

#[cfg(target_os = "linux")]
fn persist_plain_cross_filesystem_undo_intent(
    receipt: &PathMoveUndoReceipt,
    state: &CrossFilesystemUndoState,
) -> Result<(), PlatformError> {
    use std::io::Write;

    let transaction = state.destination_transaction.as_ref().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt(
            "plain cross-filesystem undo lost its destination transaction".to_string(),
        )
    })?;
    let transaction_identity = rename_identity(transaction).map_err(|error| {
        PlatformError::CommittedUndoCleanupPending {
            transaction: transaction.display().to_string(),
            message: format!("undo destination transaction identity is unavailable: {error}"),
        }
    })?;
    let contents = format!(
        "ottrin-cross-filesystem-undo-v1\nrestored={}:{}\nmoved={}:{}\ntransaction={}:{}\n",
        state.restored_identity.device,
        state.restored_identity.inode,
        receipt.identity.device,
        receipt.identity.inode,
        transaction_identity.device,
        transaction_identity.inode,
    );
    let intent = transaction.join(PLAIN_CROSS_FILESYSTEM_UNDO_INTENT);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&intent)
    {
        Ok(mut file) => {
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| PlatformError::CommittedUndoCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: format!("undo intent persistence failed: {error}"),
                })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let observed = std::fs::read(&intent).map_err(|error| {
                PlatformError::CommittedUndoCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: format!("undo intent could not be revalidated: {error}"),
                }
            })?;
            if observed != contents.as_bytes() {
                return Err(PlatformError::CommittedUndoCleanupPending {
                    transaction: transaction.display().to_string(),
                    message: "undo intent changed; destination cleanup was refused".to_string(),
                });
            }
        }
        Err(error) => {
            return Err(PlatformError::CommittedUndoCleanupPending {
                transaction: transaction.display().to_string(),
                message: format!("undo intent creation failed: {error}"),
            });
        }
    }
    std::fs::File::open(transaction)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| PlatformError::CommittedUndoCleanupPending {
            transaction: transaction.display().to_string(),
            message: format!("undo intent directory flush failed: {error}"),
        })
}

#[cfg(target_os = "linux")]
fn cleanup_plain_cross_filesystem_undo_transaction(
    transaction: &Path,
) -> Result<(), PlatformError> {
    let intent = transaction.join(PLAIN_CROSS_FILESYSTEM_UNDO_INTENT);
    match std::fs::remove_file(&intent) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(PlatformError::CommittedUndoCleanupPending {
                transaction: transaction.display().to_string(),
                message: format!("undo intent cleanup failed: {error}"),
            });
        }
    }
    std::fs::File::open(transaction)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| PlatformError::CommittedUndoCleanupPending {
            transaction: transaction.display().to_string(),
            message: format!("undo transaction flush failed: {error}"),
        })?;
    match std::fs::remove_dir(transaction) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(PlatformError::CommittedUndoCleanupPending {
                transaction: transaction.display().to_string(),
                message: error.to_string(),
            });
        }
    }
    if let Some(parent) = transaction.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::CommittedUndoCleanupPending {
                transaction: transaction.display().to_string(),
                message: format!("undo transaction parent flush failed: {error}"),
            })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn resume_cross_filesystem_path_move_undo(
    receipt: &PathMoveUndoReceipt,
    state: &CrossFilesystemUndoState,
) -> Result<(), PlatformError> {
    if rename_identity(&receipt.before_path).ok().as_ref() != Some(&state.restored_identity) {
        return Err(PlatformError::StalePathMoveUndo(
            receipt.before_path.display().to_string(),
        ));
    }

    if receipt.displaced_identity.is_none()
        && state
            .destination_transaction
            .as_ref()
            .is_some_and(|transaction| {
                std::fs::symlink_metadata(transaction)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
            })
        && std::fs::symlink_metadata(&receipt.after_path)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return match std::fs::remove_dir(&state.restoration_transaction) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(PlatformError::CommittedUndoCleanupPending {
                transaction: state.restoration_transaction.display().to_string(),
                message: error.to_string(),
            }),
        };
    }

    if let (Some(displaced_identity), Some(retained_displaced_path)) = (
        receipt.displaced_identity.as_ref(),
        receipt.retained_displaced_path.as_ref(),
    ) {
        let after_identity = rename_identity(&receipt.after_path).ok();
        let retained_identity = rename_identity(retained_displaced_path).ok();
        if after_identity.as_ref() == Some(&receipt.identity)
            && retained_identity.as_ref() == Some(displaced_identity)
        {
            exchange_paths(&receipt.after_path, retained_displaced_path)?;
        } else if after_identity.as_ref() != Some(displaced_identity)
            || retained_identity
                .as_ref()
                .is_some_and(|identity| identity != &receipt.identity)
        {
            return Err(PlatformError::StalePathMoveUndo(
                receipt.after_path.display().to_string(),
            ));
        }
        let quarantine = retained_displaced_path.with_file_name("undo-source-quarantine");
        if rename_identity(retained_displaced_path).ok().as_ref() == Some(&receipt.identity) {
            rename_without_replacement(retained_displaced_path, &quarantine)?;
        }
        match rename_identity(&quarantine) {
            Ok(identity) if identity == receipt.identity => {
                delete_permanent(&quarantine).map_err(|error| {
                    PlatformError::CommittedUndoCleanupPending {
                        transaction: quarantine
                            .parent()
                            .unwrap_or(&quarantine)
                            .display()
                            .to_string(),
                        message: error.to_string(),
                    }
                })?;
            }
            Ok(_) => {
                return Err(PlatformError::StalePathMoveUndo(
                    quarantine.display().to_string(),
                ));
            }
            Err(_) if std::fs::symlink_metadata(retained_displaced_path).is_ok() => {
                return Err(PlatformError::StalePathMoveUndo(
                    retained_displaced_path.display().to_string(),
                ));
            }
            Err(_) => {}
        }
        cleanup_completed_overwrite_move_undo(retained_displaced_path)?;
    } else {
        persist_plain_cross_filesystem_undo_intent(receipt, state)?;
        let destination_transaction = state.destination_transaction.as_ref().ok_or_else(|| {
            PlatformError::InvalidPathMoveUndoReceipt(
                "plain cross-filesystem undo lost its destination transaction".to_string(),
            )
        })?;
        let quarantine = destination_transaction.join("moved-source");
        match rename_identity(&receipt.after_path) {
            Ok(identity) if identity == receipt.identity => {
                rename_without_replacement(&receipt.after_path, &quarantine)?;
            }
            Ok(_) => {
                return Err(PlatformError::StalePathMoveUndo(
                    receipt.after_path.display().to_string(),
                ));
            }
            Err(_) => {}
        }
        match rename_identity(&quarantine) {
            Ok(identity) if identity == receipt.identity => delete_permanent(&quarantine)?,
            Ok(_) => {
                return Err(PlatformError::StalePathMoveUndo(
                    quarantine.display().to_string(),
                ));
            }
            Err(_) => {}
        }
        cleanup_plain_cross_filesystem_undo_transaction(destination_transaction)?;
    }

    match std::fs::remove_dir(&state.restoration_transaction) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::CommittedUndoCleanupPending {
            transaction: state.restoration_transaction.display().to_string(),
            message: error.to_string(),
        }),
    }
}

#[cfg(target_os = "linux")]
fn cleanup_completed_overwrite_move_undo(
    retained_displaced_path: &Path,
) -> Result<(), PlatformError> {
    move_transaction::cleanup_completed_overwrite_undo(retained_displaced_path).map_err(|error| {
        PlatformError::CommittedUndoCleanupPending {
            transaction: retained_displaced_path
                .parent()
                .unwrap_or(retained_displaced_path)
                .display()
                .to_string(),
            message: error.to_string(),
        }
    })
}

#[cfg(target_os = "linux")]
fn exchange_paths(left: &Path, right: &Path) -> Result<(), PlatformError> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};

    renameat_with(CWD, left, CWD, right, RenameFlags::EXCHANGE)
        .map_err(|error| PlatformError::Io(error.to_string()))
}

#[cfg(target_os = "linux")]
fn rename_without_replacement(source: &Path, target: &Path) -> Result<(), PlatformError> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    use rustix::io::Errno;

    renameat_with(CWD, source, CWD, target, RenameFlags::NOREPLACE).map_err(|error| {
        if error == Errno::EXIST {
            PlatformError::DestinationExists(target.display().to_string())
        } else if error == Errno::XDEV {
            PlatformError::UnsupportedSafeCrossDeviceMove(source.display().to_string())
        } else {
            PlatformError::Io(error.to_string())
        }
    })
}

#[cfg(target_os = "windows")]
fn rename_without_replacement(source: &Path, target: &Path) -> Result<(), PlatformError> {
    let target_parent = target
        .parent()
        .ok_or_else(|| PlatformError::InvalidDestination(target.display().to_string()))?;
    let target_name = target
        .file_name()
        .ok_or_else(|| PlatformError::InvalidDestination(target.display().to_string()))?;
    let source_handle = move_recovery::open_reparse_safe_entry_for_rename(source)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    let target_parent_handle = move_recovery::open_reparse_safe_directory_for_rename(target_parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    move_transaction::rename_windows_handle_no_replace(
        &source_handle,
        &target_parent_handle,
        target_name,
    )
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn rename_without_replacement(source: &Path, target: &Path) -> Result<(), PlatformError> {
    let _ = (source, target);
    Err(PlatformError::NotAvailable(
        "atomic no-replace publication is not implemented on this platform",
    ))
}

fn validate_copy_source(source: &Path) -> Result<(), PlatformError> {
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    if metadata.file_type().is_symlink() {
        std::fs::read_link(source).map_err(|e| PlatformError::Io(e.to_string()))?;
        return Ok(());
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(PlatformError::InvalidSource(source.display().to_string()));
    }
    let children = std::fs::read_dir(source).map_err(|e| PlatformError::Io(e.to_string()))?;
    for child in children {
        let child = child.map_err(|e| PlatformError::Io(e.to_string()))?;
        validate_copy_source(&child.path())?;
    }
    Ok(())
}

fn reject_directory_copy_into_itself(
    source: &Path,
    destination_dir: &Path,
) -> Result<(), PlatformError> {
    let metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(()),
    };
    if !metadata.is_dir() {
        return Ok(());
    }

    let name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let target = destination_dir.join(name);
    let lexical_source = absolute_lexical_path(source)?;
    let lexical_target = absolute_lexical_path(&target)?;
    let canonical_source =
        std::fs::canonicalize(source).map_err(|e| PlatformError::Io(e.to_string()))?;
    let canonical_target = std::fs::canonicalize(destination_dir)
        .map_err(|e| PlatformError::Io(e.to_string()))?
        .join(name);

    if lexical_target.starts_with(&lexical_source)
        || canonical_target.starts_with(&canonical_source)
    {
        Err(PlatformError::InvalidDestination(
            target.display().to_string(),
        ))
    } else {
        Ok(())
    }
}

fn absolute_lexical_path(path: &Path) -> Result<PathBuf, PlatformError> {
    use std::path::Component;

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| PlatformError::Io(e.to_string()))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn delete_permanent(target: &Path) -> Result<(), PlatformError> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) => {
            if meta.is_dir() {
                std::fs::remove_dir_all(target).map_err(|e| PlatformError::Io(e.to_string()))
            } else {
                std::fs::remove_file(target).map_err(|e| PlatformError::Io(e.to_string()))
            }
        }
        Err(e) => Err(PlatformError::Io(e.to_string())),
    }
}

#[cfg(target_os = "linux")]
fn canonical_trash_source_path(target: &Path) -> Result<PathBuf, PlatformError> {
    let name = target
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(target.display().to_string()))?;
    let parent = target
        .parent()
        .ok_or_else(|| PlatformError::InvalidSource(target.display().to_string()))?;
    std::fs::canonicalize(parent)
        .map(|parent| parent.join(name))
        .map_err(|_| PlatformError::InvalidSource(target.display().to_string()))
}

#[cfg(target_os = "linux")]
fn exact_trash_undo_receipt(
    original_path: &Path,
    before_identity: &PathMoveIdentity,
    listed_items: Result<Vec<TrashItem>, PlatformError>,
) -> Option<TrashUndoReceipt> {
    let mut matches = listed_items.ok()?.into_iter().filter(|item| {
        item.original_path == original_path
            && item.origin_storage.is_some()
            && rename_identity(&item.trash_path).ok().as_ref() == Some(before_identity)
    });
    let item = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(TrashUndoReceipt {
        item,
        identity: before_identity.clone(),
    })
}

fn list_trash_items() -> Result<Vec<TrashItem>, PlatformError> {
    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        let mut items = trash::os_limited::list()
            .map_err(|e| PlatformError::Io(e.to_string()))?
            .into_iter()
            .map(system_trash_item_to_ottrin)
            .collect::<Vec<_>>();
        items.sort_by(|a, b| {
            b.time_deleted
                .cmp(&a.time_deleted)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(items)
    }
    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        return Err(PlatformError::NotAvailable(
            "Trash listing is not implemented on this platform yet",
        ));
    }
}

fn restore_trash_item(item: &TrashItem) -> Result<(), PlatformError> {
    let current_origin = trash_origin_storage_binding(&item.trash_path, &item.original_path);
    if !matches!(
        (&item.origin_storage, current_origin.as_ref()),
        (Some(expected), Some(current)) if expected == current
    ) {
        return Err(PlatformError::InvalidSource(
            "Trash origin provenance is missing or changed".to_string(),
        ));
    }
    let parent = item.original_path.parent().ok_or_else(|| {
        PlatformError::InvalidDestination(item.original_path.display().to_string())
    })?;
    ensure_existing_dir(parent)?;
    if item.original_path.exists() {
        return Err(PlatformError::DestinationExists(
            item.original_path.display().to_string(),
        ));
    }
    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        trash::os_limited::restore_all([ottrin_trash_item_to_system(item)])
            .map_err(|e| PlatformError::Io(e.to_string()))
    }
    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        let _ = item;
        Err(PlatformError::NotAvailable(
            "Trash restore is not implemented on this platform yet",
        ))
    }
}

fn delete_trash_item_permanently(item: &TrashItem) -> Result<(), PlatformError> {
    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        trash::os_limited::purge_all([ottrin_trash_item_to_system(item)])
            .map_err(|e| PlatformError::Io(e.to_string()))
    }
    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        let _ = item;
        Err(PlatformError::NotAvailable(
            "Trash deletion is not implemented on this platform yet",
        ))
    }
}

fn empty_trash() -> Result<(), PlatformError> {
    #[cfg(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    ))]
    {
        let items = trash::os_limited::list().map_err(|e| PlatformError::Io(e.to_string()))?;
        trash::os_limited::purge_all(items).map_err(|e| PlatformError::Io(e.to_string()))
    }
    #[cfg(not(any(
        target_os = "windows",
        all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        )
    )))]
    {
        Err(PlatformError::NotAvailable(
            "Trash listing is not implemented on this platform yet",
        ))
    }
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn system_trash_item_to_ottrin(item: trash::TrashItem) -> TrashItem {
    let original_path = item.original_path();
    let trash_path =
        trash_file_path_for_system_item(&item).unwrap_or_else(|| PathBuf::from(&item.id));
    let metadata = trash::os_limited::metadata(&item).ok();
    let kind = trash_item_kind(&trash_path, metadata.as_ref());
    let size_bytes = metadata.as_ref().and_then(|m| m.size.size());
    let deletion_date = trash_deletion_date(&item);
    let origin_storage = trash_origin_storage_binding(&trash_path, &original_path);
    TrashItem {
        id: item.id,
        original_name: item.name.clone(),
        original_parent: item.original_parent,
        trash_path,
        original_path,
        deletion_date,
        time_deleted: item.time_deleted,
        name: item.name.to_string_lossy().to_string(),
        kind,
        size_bytes,
        origin_storage,
    }
}

/// Freedesktop Trash keeps only the original pathname for normal mounted-volume
/// entries; it does not retain a volume UUID in `.trashinfo`. Do not manufacture
/// an identity from the volume currently mounted at that pathname. The personal
/// home Trash is the one native case we can prove here: both its storage and the
/// original path are anchored beneath this process's home root.
#[cfg(target_os = "linux")]
fn trash_origin_storage_binding(
    trash_path: &Path,
    original_path: &Path,
) -> Option<TrashOriginBinding> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    trash_origin_storage_binding_at(trash_path, original_path, &home, data_home.as_deref())
}

#[cfg(target_os = "linux")]
fn trash_origin_storage_binding_at(
    trash_path: &Path,
    original_path: &Path,
    home: &Path,
    data_home: Option<&Path>,
) -> Option<TrashOriginBinding> {
    if !home.is_absolute()
        || !original_path.is_absolute()
        || original_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    let data_home = data_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".local/share"));
    let native_home_trash = data_home.join("Trash");

    // Mutable Trash metadata is not proof by itself. Resolve the existing
    // payload and original parent physically, and reject a symlink payload,
    // traversal components, or a home path that resolves outside HOME.
    if std::fs::symlink_metadata(trash_path)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let canonical_home = std::fs::canonicalize(home).ok()?;
    let canonical_trash_root = std::fs::canonicalize(native_home_trash).ok()?;
    let canonical_trash_path = std::fs::canonicalize(trash_path).ok()?;
    let canonical_original_parent = std::fs::canonicalize(original_path.parent()?).ok()?;
    if !canonical_trash_path.starts_with(canonical_trash_root)
        || !canonical_original_parent.starts_with(canonical_home)
    {
        return None;
    }

    Some(TrashOriginBinding {
        root: home.to_path_buf(),
        // This identifies the only native provenance proof offered by the
        // Freedesktop metadata layout, not a guessed external-volume UUID.
        stable_identity: Some("ottrin-native-home-trash-v1".to_string()),
    })
}

#[cfg(not(target_os = "linux"))]
fn trash_origin_storage_binding(
    _trash_path: &Path,
    _original_path: &Path,
) -> Option<TrashOriginBinding> {
    // The Windows Shell item API exposes an original pathname but no durable
    // source-volume identity for Recycle Bin entries.
    None
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn ottrin_trash_item_to_system(item: &TrashItem) -> trash::TrashItem {
    trash::TrashItem {
        id: item.id.clone(),
        name: item.original_name.clone(),
        original_parent: item.original_parent.clone(),
        time_deleted: item.time_deleted,
    }
}

#[cfg(target_os = "linux")]
fn trash_file_path_for_system_item(item: &trash::TrashItem) -> Option<PathBuf> {
    let info_path = PathBuf::from(&item.id);
    let stem = info_path
        .file_name()?
        .to_str()?
        .strip_suffix(".trashinfo")?;
    let trash_root = info_path.parent()?.parent()?;
    Some(trash_root.join("files").join(stem))
}

#[cfg(not(target_os = "linux"))]
fn trash_file_path_for_system_item(_item: &trash::TrashItem) -> Option<PathBuf> {
    None
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn trash_item_kind(trash_path: &Path, metadata: Option<&trash::TrashItemMetadata>) -> EntryKind {
    if let Ok(meta) = std::fs::symlink_metadata(trash_path) {
        if meta.file_type().is_symlink() {
            EntryKind::Symlink
        } else if meta.is_dir() {
            EntryKind::Directory
        } else if meta.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        }
    } else if metadata.and_then(|m| m.size.entries()).is_some() {
        EntryKind::Directory
    } else {
        EntryKind::File
    }
}

#[cfg(target_os = "linux")]
fn trash_deletion_date(item: &trash::TrashItem) -> Option<String> {
    let info_path = PathBuf::from(&item.id);
    let text = std::fs::read_to_string(info_path).ok()?;
    parse_trashinfo_field(&text, "DeletionDate").or_else(|| format_unix_time(item.time_deleted))
}

#[cfg(not(target_os = "linux"))]
fn trash_deletion_date(item: &trash::TrashItem) -> Option<String> {
    format_unix_time(item.time_deleted)
}

#[cfg(any(
    target_os = "windows",
    all(
        unix,
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    )
))]
fn format_unix_time(seconds: i64) -> Option<String> {
    if seconds < 0 {
        return None;
    }
    let dt = chrono::DateTime::from_timestamp(seconds, 0)?;
    Some(
        dt.with_timezone(&chrono::Local)
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string(),
    )
}

#[cfg(target_os = "linux")]
fn parse_trashinfo_field(text: &str, field: &str) -> Option<String> {
    let prefix = format!("{field}=");
    text.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(|value| value.trim().to_string())
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn next_available_name(base: &Path) -> PathBuf {
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base.file_stem().unwrap_or(base.as_os_str());
    let ext = base.extension();

    for i in 1.. {
        let mut candidate_name = stem.to_os_string();
        candidate_name.push(format!(" ({i})"));
        if let Some(ext) = ext {
            candidate_name.push(".");
            candidate_name.push(ext);
        }
        let candidate = parent.join(candidate_name);
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }

    base.to_path_buf()
}

#[cfg(target_os = "linux")]
fn copy_entry(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    let meta = std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    if meta.file_type().is_symlink() {
        copy_symlink(source, destination)
    } else if meta.is_dir() {
        copy_dir_recursive(source, destination)
    } else if meta.is_file() {
        std::fs::copy(source, destination)
            .map(|_| ())
            .map_err(|e| PlatformError::Io(e.to_string()))
    } else {
        Err(PlatformError::InvalidSource(source.display().to_string()))
    }
}

#[cfg(target_os = "linux")]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    let target = std::fs::read_link(source).map_err(|e| PlatformError::Io(e.to_string()))?;

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, destination)
            .map_err(|e| PlatformError::Io(e.to_string()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;

        let file_type = std::fs::symlink_metadata(source)
            .map_err(|e| PlatformError::Io(e.to_string()))?
            .file_type();
        if file_type.is_symlink_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
                .map_err(|e| PlatformError::Io(e.to_string()))
        } else if file_type.is_symlink_file() {
            std::os::windows::fs::symlink_file(target, destination)
                .map_err(|e| PlatformError::Io(e.to_string()))
        } else {
            Err(PlatformError::InvalidSource(source.display().to_string()))
        }
    }
}

#[cfg(target_os = "linux")]
fn copy_dir_recursive(source_dir: &Path, target_dir: &Path) -> Result<(), PlatformError> {
    let source_permissions = std::fs::metadata(source_dir)
        .map_err(|e| PlatformError::Io(e.to_string()))?
        .permissions();
    std::fs::create_dir_all(target_dir).map_err(|e| PlatformError::Io(e.to_string()))?;

    let read_dir = std::fs::read_dir(source_dir).map_err(|e| PlatformError::Io(e.to_string()))?;
    for child in read_dir {
        let child = child.map_err(|e| PlatformError::Io(e.to_string()))?;
        let source = child.path();
        let target = target_dir.join(child.file_name());

        let meta =
            std::fs::symlink_metadata(&source).map_err(|e| PlatformError::Io(e.to_string()))?;
        if meta.file_type().is_symlink() {
            copy_symlink(&source, &target)?;
        } else if meta.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else {
            std::fs::copy(&source, &target).map_err(|e| PlatformError::Io(e.to_string()))?;
        }
    }

    std::fs::set_permissions(target_dir, source_permissions)
        .map_err(|e| PlatformError::Io(e.to_string()))?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn chmod_descriptor_mode(
    descriptor: &std::os::fd::OwnedFd,
    mode: u32,
) -> Result<(), PlatformError> {
    use rustix::fs::{Mode, fchmod};
    use rustix::io::Errno;
    use std::os::fd::AsRawFd;

    match fchmod(descriptor, Mode::from_raw_mode(mode)) {
        Ok(()) => return Ok(()),
        Err(error) if error != Errno::BADF => {
            return Err(PlatformError::Io(error.to_string()));
        }
        Err(_) => {}
    }

    // O_PATH descriptors cannot be passed to fchmod. Linux 6.5 added the
    // identity-anchored AT_EMPTY_PATH form used as the safe fallback for
    // entries that could not be opened read-only (for example mode 000).
    let empty_path = b"\0";
    let result = unsafe {
        libc::syscall(
            libc::SYS_fchmodat2,
            descriptor.as_raw_fd(),
            empty_path.as_ptr().cast::<libc::c_char>(),
            mode as libc::mode_t,
            libc::AT_EMPTY_PATH,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(PlatformError::Io(
            std::io::Error::last_os_error().to_string(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn chmod_descriptor_state(
    descriptor: &std::os::fd::OwnedFd,
) -> Result<(PathMoveIdentity, u32), PlatformError> {
    let stat =
        rustix::fs::fstat(descriptor).map_err(|error| PlatformError::Io(error.to_string()))?;
    if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
        return Err(PlatformError::Io(
            "changing permissions through a symbolic link is not supported".to_string(),
        ));
    }
    Ok((
        PathMoveIdentity {
            device: stat.st_dev,
            inode: stat.st_ino,
        },
        stat.st_mode & 0o7777,
    ))
}

#[cfg(target_os = "linux")]
fn chmod_with_undo_receipt_linux(
    path: &Path,
    mode_str: &str,
) -> Result<ChmodUndoReceipt, PlatformError> {
    use rustix::fs::{CWD, Mode, OFlags, openat};
    use std::sync::Arc;

    let descriptor = Arc::new(
        openat(
            CWD,
            path,
            OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .or_else(|_| {
            openat(
                CWD,
                path,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
        })
        .map_err(|error| PlatformError::Io(error.to_string()))?,
    );
    let (identity, before_mode) = chmod_descriptor_state(&descriptor)?;
    let after_mode = chmod_mode(before_mode, mode_str).map_err(PlatformError::Io)? & 0o7777;
    chmod_descriptor_mode(&descriptor, after_mode)?;
    let path_identity = rename_identity(path)
        .map_err(|_| PlatformError::StaleChmodUndo(path.display().to_string()))?;
    let (descriptor_identity, actual_mode) = chmod_descriptor_state(&descriptor)?;
    if path_identity != identity || descriptor_identity != identity || actual_mode != after_mode {
        return Err(PlatformError::StaleChmodUndo(path.display().to_string()));
    }
    Ok(ChmodUndoReceipt {
        path: path.to_path_buf(),
        identity,
        before_mode,
        after_mode,
        descriptor,
    })
}

#[cfg(target_os = "linux")]
fn undo_chmod_linux(receipt: &ChmodUndoReceipt) -> Result<(), PlatformError> {
    use std::os::unix::fs::PermissionsExt;

    let path_metadata = std::fs::symlink_metadata(&receipt.path)
        .map_err(|_| PlatformError::StaleChmodUndo(receipt.path.display().to_string()))?;
    let path_identity = PathMoveIdentity {
        device: std::os::unix::fs::MetadataExt::dev(&path_metadata),
        inode: std::os::unix::fs::MetadataExt::ino(&path_metadata),
    };
    let path_mode = path_metadata.permissions().mode() & 0o7777;
    let (descriptor_identity, descriptor_mode) = chmod_descriptor_state(&receipt.descriptor)?;
    if path_metadata.file_type().is_symlink()
        || path_identity != receipt.identity
        || descriptor_identity != receipt.identity
        || path_mode != receipt.after_mode
        || descriptor_mode != receipt.after_mode
    {
        return Err(PlatformError::StaleChmodUndo(
            receipt.path.display().to_string(),
        ));
    }
    chmod_descriptor_mode(&receipt.descriptor, receipt.before_mode)
}

#[cfg(unix)]
fn chmod_mode(current: u32, mode_str: &str) -> Result<u32, String> {
    if let Some(stripped) = mode_str.strip_prefix('+') {
        Ok(current | symbolic_bits(stripped)?)
    } else if let Some(stripped) = mode_str.strip_prefix('-') {
        Ok(current & !symbolic_bits(stripped)?)
    } else {
        u32::from_str_radix(mode_str, 8).map_err(|_| format!("Invalid mode: {}", mode_str))
    }
}

#[cfg(not(target_os = "linux"))]
fn chmod_path(path: &Path, mode_str: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let new_mode = chmod_mode(meta.permissions().mode(), mode_str)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(new_mode))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode_str);
        Err("chmod is not supported on this platform".to_string())
    }
}

#[cfg(unix)]
fn symbolic_bits(spec: &str) -> Result<u32, String> {
    let mut bits = 0u32;
    for ch in spec.chars() {
        match ch {
            'r' => bits |= 0o444,
            'w' => bits |= 0o222,
            'x' => bits |= 0o111,
            _ => return Err(format!("Unknown permission bit: {}", ch)),
        }
    }
    Ok(bits)
}

fn create_symlink(link: &Path, target: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
    }
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link).map_err(|e| e.to_string())
        } else {
            std::os::windows::fs::symlink_file(target, link).map_err(|e| e.to_string())
        }
    }
}

fn read_properties(path: &Path) -> Result<FileProperties, PlatformError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| PlatformError::Io(e.to_string()))?;

    let kind = if metadata.is_dir() {
        EntryKind::Directory
    } else if metadata.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };

    Ok(FileProperties {
        path: path.to_path_buf(),
        kind,
        size_bytes: metadata.len(),
        readonly: metadata.permissions().readonly(),
        created: metadata.created().ok(),
        modified: metadata.modified().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_native_choices_bind_target_filter_deduplicate_and_sort() {
        let target = PathBuf::from("sample.txt");
        let choices = open_with_choices_from_native(
            &target,
            vec![
                NativeOpenWithApplication::new("z-handler", "Zed"),
                NativeOpenWithApplication::new("a-handler", "Alpha"),
                NativeOpenWithApplication::new("z-handler", "Other Zed"),
                NativeOpenWithApplication::new("", "Missing ID"),
                NativeOpenWithApplication::new("hidden-handler", ""),
            ],
        );

        assert_eq!(
            choices
                .iter()
                .map(OpenWithApplication::name)
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zed"]
        );
        assert_eq!(choices[0].target, target);
        assert_eq!(choices[0].native_id, "a-handler");
    }

    #[test]
    fn open_with_selection_rejects_stale_ids_and_launches_the_requeried_object() {
        let launched = std::cell::Cell::new(None);
        select_native_open_with_handler(
            vec![
                (NativeOpenWithApplication::new("old-handler", "Old"), 7_u8),
                (NativeOpenWithApplication::new("new-handler", "New"), 9_u8),
            ],
            "new-handler",
            |native_object| {
                launched.set(Some(native_object));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(launched.get(), Some(9));

        let stale = select_native_open_with_handler(
            vec![(NativeOpenWithApplication::new("fresh-handler", "Fresh"), ())],
            "forged-handler",
            |_| Ok(()),
        );
        assert!(matches!(stale, Err(PlatformError::InvalidSource(_))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_with_native_choices_bind_target_deduplicate_and_sort() {
        let target = PathBuf::from("/tmp/target.txt");
        let choices = linux_open_with::choices_from_native(
            &target,
            vec![
                linux_open_with::NativeApplication::new("z.desktop", "Zed"),
                linux_open_with::NativeApplication::new("a.desktop", "Alpha"),
                linux_open_with::NativeApplication::new("z.desktop", "Other Zed"),
                linux_open_with::NativeApplication::new("", "Missing ID"),
            ],
        );
        assert_eq!(
            choices
                .iter()
                .map(OpenWithApplication::name)
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zed"]
        );
        assert_eq!(choices[0].target, target);
        assert_eq!(choices[0].native_id, "a.desktop");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_with_native_selection_rejects_stale_ids_and_launches_the_requeried_object() {
        let launched = std::cell::Cell::new(None);
        linux_open_with::select_and_launch(
            vec![
                (
                    linux_open_with::NativeApplication::new("old.desktop", "Old"),
                    7_u8,
                ),
                (
                    linux_open_with::NativeApplication::new("new.desktop", "New"),
                    9_u8,
                ),
            ],
            "new.desktop",
            |native_object| {
                launched.set(Some(native_object));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(launched.get(), Some(9));
        let stale = linux_open_with::select_and_launch(
            vec![(
                linux_open_with::NativeApplication::new("fresh.desktop", "Fresh"),
                (),
            )],
            "forged.desktop",
            |_| Ok(()),
        );
        assert!(matches!(stale, Err(PlatformError::InvalidSource(_))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn open_with_hides_nodisplay_handlers_in_an_isolated_gio_environment() {
        use gio::prelude::*;
        use std::time::UNIX_EPOCH;

        const ROOT_ENV: &str = "OTTRIN_OPEN_WITH_ISOLATED_GIO_ROOT";

        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let root = PathBuf::from(root);
            let applications = root.join("data/applications");
            std::fs::create_dir_all(&applications).expect("create isolated applications directory");
            std::fs::create_dir_all(root.join("config")).expect("create isolated config directory");
            std::fs::write(root.join("sample.txt"), b"sample").expect("create sample target");
            std::fs::write(
                applications.join("visible.desktop"),
                "[Desktop Entry]\nType=Application\nName=Visible text handler\nExec=/bin/true %f\nMimeType=text/plain;\n",
            )
            .expect("write visible handler");
            std::fs::write(
                applications.join("hidden.desktop"),
                "[Desktop Entry]\nType=Application\nName=Hidden text handler\nNoDisplay=true\nExec=/bin/true %f\nMimeType=text/plain;\n",
            )
            .expect("write hidden handler");
            std::fs::write(
                applications.join("mimeinfo.cache"),
                "[MIME Cache]\ntext/plain=visible.desktop;hidden.desktop;\n",
            )
            .expect("write MIME cache");
            std::fs::write(
                root.join("config/mimeapps.list"),
                "[Added Associations]\ntext/plain=visible.desktop;hidden.desktop;\n",
            )
            .expect("write MIME associations");

            let update = std::process::Command::new("update-desktop-database")
                .arg(&applications)
                .status()
                .expect("run update-desktop-database");
            assert!(update.success(), "refresh isolated desktop database");

            let gio_ids = gio::AppInfo::all_for_type("text/plain")
                .into_iter()
                .filter_map(|application| application.id().map(|id| id.to_string()))
                .collect::<Vec<_>>();
            assert!(
                gio_ids.iter().any(|id| id == "hidden.desktop"),
                "GIO must expose the hidden fixture so the wrapper proves its filtering"
            );
            assert!(
                gio_ids.iter().any(|id| id == "visible.desktop"),
                "GIO must expose the visible fixture"
            );

            let choices = linux_open_with::discover(&root.join("sample.txt"))
                .expect("discover isolated text handlers");
            let choice_ids = choices
                .iter()
                .map(|choice| choice.native_id.as_str())
                .collect::<Vec<_>>();
            assert!(choice_ids.contains(&"visible.desktop"));
            assert!(
                !choice_ids.contains(&"hidden.desktop"),
                "NoDisplay handlers must never be offered by Open With"
            );
            return;
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ottrin-open-with-gio-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create isolated GIO root");
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("tests::open_with_hides_nodisplay_handlers_in_an_isolated_gio_environment")
            .arg("--nocapture")
            .env(ROOT_ENV, &root)
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_DATA_DIRS", root.join("data"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("HOME", root.join("home"))
            .status()
            .expect("run isolated GIO child test");
        let cleanup = std::fs::remove_dir_all(&root);
        assert!(cleanup.is_ok(), "clean isolated GIO state: {cleanup:?}");
        assert!(status.success(), "isolated GIO child test failed");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn committed_trash_delete_does_not_become_failure_when_receipt_discovery_fails() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-trash-postcommit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("report.txt");
        std::fs::write(&source, b"report").unwrap();
        let identity = rename_identity(&source).unwrap();

        let receipt = exact_trash_undo_receipt(
            &source,
            &identity,
            Err(PlatformError::Io("Trash listing unavailable".to_string())),
        );

        assert!(receipt.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn single_home_trash_delete_returns_a_receipt_that_restores_the_exact_item() {
        use std::time::UNIX_EPOCH;

        const ROOT_ENV: &str = "OTTRIN_TRASH_UNDO_TEST_ROOT";
        if let Some(root) = std::env::var_os(ROOT_ENV) {
            let root = PathBuf::from(root);
            let documents = root.join("home/Documents");
            std::fs::create_dir_all(&documents).expect("create isolated home documents");
            let source = documents.join("report.txt");
            std::fs::write(&source, b"exact report").expect("create source fixture");

            let platform = DefaultPlatform;
            let receipt = platform
                .execute_trash_with_undo_receipt(&source)
                .expect("move source to isolated home Trash")
                .expect("home Trash deletion must produce an exact receipt");
            assert!(!source.exists(), "forward Trash removes the original path");

            platform
                .undo_trash_with_receipt(&receipt)
                .expect("restore exact isolated Trash item");
            assert_eq!(
                std::fs::read(&source).expect("read restored source"),
                b"exact report"
            );

            let occupied_receipt = platform
                .execute_trash_with_undo_receipt(&source)
                .expect("move source to Trash again")
                .expect("second home Trash deletion must produce a receipt");
            std::fs::write(&source, b"replacement").expect("occupy original pathname");
            assert!(matches!(
                platform.undo_trash_with_receipt(&occupied_receipt),
                Err(PlatformError::DestinationExists(_))
            ));
            assert_eq!(std::fs::read(&source).unwrap(), b"replacement");
            assert_eq!(
                std::fs::read(occupied_receipt.trash_path()).unwrap(),
                b"exact report"
            );
            std::fs::remove_file(&source).expect("remove replacement");
            platform
                .undo_trash_with_receipt(&occupied_receipt)
                .expect("restore after original pathname is free");

            let stale_receipt = platform
                .execute_trash_with_undo_receipt(&source)
                .expect("move source to Trash for stale test")
                .expect("third home Trash deletion must produce a receipt");
            let displaced_payload = stale_receipt.trash_path().with_extension("displaced");
            std::fs::rename(stale_receipt.trash_path(), &displaced_payload)
                .expect("replace exact Trash payload");
            std::fs::write(stale_receipt.trash_path(), b"other item")
                .expect("publish replacement Trash payload");
            assert!(matches!(
                platform.undo_trash_with_receipt(&stale_receipt),
                Err(PlatformError::StalePathMoveUndo(_))
            ));
            assert!(!source.exists());
            assert_eq!(
                std::fs::read(stale_receipt.trash_path()).unwrap(),
                b"other item"
            );
            return;
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ottrin-trash-undo-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(root.join("home")).expect("create isolated home");
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("tests::single_home_trash_delete_returns_a_receipt_that_restores_the_exact_item")
            .arg("--nocapture")
            .env(ROOT_ENV, &root)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .status()
            .expect("run isolated Trash undo child test");
        let cleanup = std::fs::remove_dir_all(&root);
        assert!(cleanup.is_ok(), "clean isolated Trash state: {cleanup:?}");
        assert!(status.success(), "isolated Trash undo child test failed");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_home_trash_proof_rejects_traversal_and_symlink_payloads() {
        use std::os::unix::fs::symlink;
        use std::time::UNIX_EPOCH;

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ottrin-trash-proof-{}-{nonce}", std::process::id()));
        let home = root.join("home");
        let data_home = root.join("xdg-data");
        let trash_files = data_home.join("Trash/files");
        let original_parent = home.join("Documents");
        std::fs::create_dir_all(&trash_files).expect("create custom XDG Trash");
        std::fs::create_dir_all(&original_parent).expect("create original home parent");
        let payload = trash_files.join("report.txt");
        std::fs::write(&payload, b"report").expect("create Trash payload");
        let original = original_parent.join("report.txt");

        assert!(
            trash_origin_storage_binding_at(&payload, &original, &home, Some(&data_home)).is_some(),
            "a physically verified custom-XDG home Trash payload is proven"
        );
        assert!(
            trash_origin_storage_binding_at(
                &payload,
                &home.join("visible/../../mnt/replacement/report.txt"),
                &home,
                Some(&data_home),
            )
            .is_none(),
            "mutable metadata cannot escape HOME with parent traversal"
        );

        let symlink_payload = trash_files.join("linked.txt");
        symlink(&payload, &symlink_payload).expect("create malicious Trash symlink");
        assert!(
            trash_origin_storage_binding_at(
                &symlink_payload,
                &original_parent.join("linked.txt"),
                &home,
                Some(&data_home),
            )
            .is_none(),
            "a symlink payload is not native origin proof"
        );
        std::fs::remove_dir_all(root).expect("remove Trash proof fixture");
    }

    #[test]
    fn copy_batch_failure_after_a_completed_item_is_typed_as_partial_completion() {
        let sources = vec![PathBuf::from("first"), PathBuf::from("second")];
        let mut calls = 0;

        let result = execute_batch(&sources, |_| {
            calls += 1;
            if calls == 2 {
                Err(PlatformError::Io("permission denied".into()))
            } else {
                Ok(())
            }
        });

        if let Err(PlatformError::PartialBatch {
            completed,
            total,
            stopped_source,
            completed_dispositions,
            ..
        }) = result
        {
            assert_eq!(completed, 1);
            assert_eq!(total, 2);
            assert_eq!(stopped_source, Path::new("second"));
            assert!(completed_dispositions.is_empty());
        } else {
            panic!("expected typed partial copy batch result");
        }
    }

    #[test]
    fn move_batch_committed_warning_counts_the_stopped_item_as_completed() {
        let sources = vec![PathBuf::from("first"), PathBuf::from("second")];
        let result = execute_batch(&sources, |source| {
            if source == Path::new("first") {
                Ok(())
            } else {
                Err(PlatformError::CommittedCleanupPending {
                    transaction: "quarantine".into(),
                    message: "cleanup pending".into(),
                })
            }
        });

        if let Err(PlatformError::PartialBatch {
            completed,
            total,
            stopped_source,
            completed_dispositions,
            ..
        }) = result
        {
            assert_eq!(completed, 2);
            assert_eq!(total, 2);
            assert_eq!(stopped_source, Path::new("second"));
            assert!(completed_dispositions.is_empty());
        } else {
            panic!("expected typed partial move batch result");
        }
    }

    #[test]
    fn move_batch_claimed_recovery_does_not_count_the_stopped_item_as_completed() {
        let sources = vec![PathBuf::from("first"), PathBuf::from("second")];
        let result = execute_move_batch(&sources, |source| {
            if source == Path::new("first") {
                Ok(MoveDisposition::Moved)
            } else {
                Err(PlatformError::ClaimedRecoveryPending {
                    transaction: "claimed-transaction".into(),
                    message: "publication was not reached".into(),
                })
            }
        });

        match result {
            Err(PlatformError::PartialBatch {
                completed,
                completed_dispositions,
                recovery_paths,
                ..
            }) => {
                assert_eq!(completed, 1);
                assert_eq!(completed_dispositions, vec![MoveDisposition::Moved]);
                assert_eq!(recovery_paths, vec![PathBuf::from("claimed-transaction")]);
            }
            other => panic!("expected typed partial move batch result, got {other:?}"),
        }
    }

    #[test]
    fn move_batch_does_not_count_skipped_sources_as_completed() {
        let sources = vec![
            PathBuf::from("first"),
            PathBuf::from("second"),
            PathBuf::from("third"),
        ];

        let result = execute_move_batch(&sources, |source| {
            if source == Path::new("second") {
                Ok(MoveDisposition::Skipped)
            } else if source == Path::new("third") {
                Err(PlatformError::Io("injected failure".into()))
            } else {
                Ok(MoveDisposition::Moved)
            }
        });

        match result {
            Err(PlatformError::PartialBatch {
                completed_dispositions,
                completed,
                total,
                stopped_source,
                ..
            }) => {
                assert_eq!(completed, 1);
                assert_eq!(total, 3);
                assert_eq!(stopped_source, Path::new("third"));
                assert_eq!(
                    completed_dispositions,
                    vec![MoveDisposition::Moved, MoveDisposition::Skipped]
                );
            }
            other => panic!("expected typed partial move batch result, got {other:?}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn public_move_batch_reports_only_published_sources_as_completed() {
        let dir = TestDir::new("move-batch-published-completion-count");
        let recovery_root = dir.path().join("state").join("ottrin").join("moves");
        let _recovery_root = TestMoveRecoveryRoot::new(recovery_root);
        let skipped_source = dir.path().join("occupied.txt");
        let moved_source = dir.path().join("moved.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&skipped_source, "keep at source").unwrap();
        std::fs::write(&moved_source, "publish once").unwrap();
        std::fs::write(destination.join("occupied.txt"), "keep at destination").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![
                skipped_source.clone(),
                moved_source.clone(),
                moved_source.clone(),
            ],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        match result {
            Err(PlatformError::PartialBatch {
                completed,
                total,
                stopped_source,
                completed_dispositions,
                ..
            }) => {
                assert_eq!(completed, 1);
                assert_eq!(total, 3);
                assert_eq!(stopped_source, moved_source);
                assert_eq!(
                    completed_dispositions,
                    vec![MoveDisposition::Skipped, MoveDisposition::Moved]
                );
            }
            other => panic!("expected typed partial move batch result, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(&skipped_source).unwrap(),
            "keep at source"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("occupied.txt")).unwrap(),
            "keep at destination"
        );
        assert!(!moved_source.exists());
        assert_eq!(
            std::fs::read_to_string(destination.join("moved.txt")).unwrap(),
            "publish once"
        );
    }

    #[derive(Default)]
    struct MockDefaultMoveOps {
        called_with: std::sync::atomic::AtomicUsize,
    }

    impl PlatformOps for MockDefaultMoveOps {
        fn delete_path(&self, _target: &Path, _mode: DeleteMode) -> Result<(), PlatformError> {
            Ok(())
        }

        fn reveal_in_system(&self, _target: &Path) -> Result<(), PlatformError> {
            Ok(())
        }

        fn open_with_application(
            &self,
            _target: &Path,
            _application: &Path,
        ) -> Result<(), PlatformError> {
            Ok(())
        }

        fn execute_command(
            &self,
            command: &FileCommand,
        ) -> Result<Option<FileProperties>, PlatformError> {
            self.called_with
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            match command {
                FileCommand::Move { .. } => Err(PlatformError::Io("mock move failure".to_string())),
                _ => Ok(None),
            }
        }

        fn execute_privileged(
            &self,
            _request: &PrivilegedRequest,
        ) -> Result<PrivilegedResponse, PlatformError> {
            Ok(PrivilegedResponse {
                status: ottrin_core::PrivilegedStatus::Unsupported,
                message: None,
                payload: None,
            })
        }

        fn privileged_availability(&self) -> PrivilegedAvailability {
            PrivilegedAvailability::Unsupported("mock helper unavailable".to_string())
        }
    }

    #[test]
    fn execute_command_with_move_dispositions_defaults_with_empty_dispositions() {
        let platform = MockDefaultMoveOps::default();
        let command = FileCommand::Move {
            sources: vec![PathBuf::from("/tmp/source")],
            destination: PathBuf::from("/tmp/destination"),
            conflict: ConflictAction::Rename,
        };
        let error = platform.execute_command_with_move_dispositions(&command);
        assert!(matches!(error, Err(PlatformError::Io(_))));

        let command = FileCommand::Delete {
            targets: vec![PathBuf::from("/tmp/source")],
            mode: DeleteMode::Trash,
        };
        let Ok((result, dispositions)) = platform.execute_command_with_move_dispositions(&command)
        else {
            panic!("expected successful command execution");
        };
        assert!(result.is_none());
        assert!(dispositions.is_empty());
        assert_eq!(
            platform
                .called_with
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn batch_failure_before_any_completion_retains_normal_error_semantics() {
        let sources = vec![PathBuf::from("first"), PathBuf::from("second")];
        let result = execute_batch(&sources, |_| {
            Err(PlatformError::Io("permission denied".into()))
        });

        assert!(matches!(result, Err(PlatformError::Io(_))));
    }
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    pub(super) struct TestDir {
        path: PathBuf,
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        previous_recovery_root: Option<PathBuf>,
    }

    #[cfg(target_os = "linux")]
    struct TestMoveRecoveryRoot {
        _env_guard: std::sync::MutexGuard<'static, ()>,
        previous_recovery_root: Option<PathBuf>,
    }

    #[cfg(target_os = "linux")]
    impl TestMoveRecoveryRoot {
        fn new(root: PathBuf) -> Self {
            let env_guard = move_transaction::TEST_MOVE_ENV_SERIALIZER
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let previous_recovery_root = move_transaction::replace_test_recovery_root(Some(root));
            Self {
                _env_guard: env_guard,
                previous_recovery_root,
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for TestMoveRecoveryRoot {
        fn drop(&mut self) {
            let _ =
                move_transaction::replace_test_recovery_root(self.previous_recovery_root.take());
        }
    }

    impl TestDir {
        pub(super) fn new(name: &str) -> Self {
            Self::new_at(&std::env::temp_dir(), name)
        }

        pub(super) fn new_at(parent: &Path, name: &str) -> Self {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "ottrin-platform-{name}-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let previous_recovery_root =
                move_transaction::replace_test_recovery_root(Some(path.join("move-state")));
            Self {
                path,
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                previous_recovery_root,
            }
        }

        pub(super) fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let _ =
                move_transaction::replace_test_recovery_root(self.previous_recovery_root.take());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_dir_isolates_retained_move_journals_from_outer_discovery() {
        let outer = TestDir::new("outer-move-recovery-discovery");
        let outer_recovery_root = outer.path().join("move-state");
        let _outer_recovery = TestMoveRecoveryRoot::new(outer_recovery_root.clone());
        let fixture = TestDir::new("inner-move-recovery-discovery");
        let source = fixture.path().join("entry.txt");
        let destination = fixture.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(destination.join("entry.txt"), "displaced bytes").unwrap();

        DefaultPlatform
            .execute_command(&FileCommand::Move {
                sources: vec![source],
                destination,
                conflict: ConflictAction::Overwrite,
            })
            .unwrap();

        assert!(
            discover_retained_move_transactions_at(&outer_recovery_root)
                .unwrap()
                .is_empty(),
            "an ordinary test fixture must not publish retained Move state into an outer discovery namespace"
        );
    }

    #[test]
    fn creation_commands_reject_names_that_escape_the_requested_parent() {
        let root = TestDir::new("creation-name-boundary");
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).expect("create nested fixture directory");

        for command in [
            FileCommand::CreateFile {
                parent: root.path().to_path_buf(),
                name: "nested/created.txt".to_string(),
            },
            FileCommand::CreateFolder {
                parent: root.path().to_path_buf(),
                name: "nested/created-folder".to_string(),
            },
        ] {
            let result = DefaultPlatform.execute_command(&command);

            assert!(
                result.is_err(),
                "creation must reject a name with a path separator: {command:?}"
            );
        }

        assert!(
            std::fs::read_dir(&nested)
                .expect("read nested fixture directory")
                .next()
                .is_none(),
            "a rejected name must not create a nested entry"
        );
    }

    #[test]
    fn creation_commands_reject_parent_traversal_without_mutating_an_external_sibling() {
        let root = TestDir::new("creation-parent-traversal");
        let requested_parent = root.path().join("requested");
        let external_sibling = root.path().join("external");
        std::fs::create_dir(&requested_parent).expect("create requested parent");
        std::fs::create_dir(&external_sibling).expect("create external sibling");

        for command in [
            FileCommand::CreateFile {
                parent: requested_parent.clone(),
                name: "../external/escaped.txt".to_string(),
            },
            FileCommand::CreateFolder {
                parent: requested_parent.clone(),
                name: "../external/escaped-folder".to_string(),
            },
        ] {
            assert!(
                DefaultPlatform.execute_command(&command).is_err(),
                "parent traversal must be rejected: {command:?}"
            );
        }

        assert!(
            std::fs::read_dir(&external_sibling)
                .expect("read external sibling")
                .next()
                .is_none(),
            "rejected creation commands must not mutate outside their requested parent"
        );
    }

    #[test]
    fn windows_reserved_creation_names_reject_documented_superscript_device_aliases() {
        for name in [
            "COM¹",
            "COM².txt",
            "COM³.log",
            "LPT¹",
            "LPT².txt",
            "LPT³.log",
        ] {
            assert!(is_windows_reserved_creation_name(name), "{name}");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_creation_commands_reject_superscript_device_aliases_before_mutation() {
        let root = TestDir::new("windows-superscript-device");
        for command in [
            FileCommand::CreateFile {
                parent: root.path().to_path_buf(),
                name: "COM¹.txt".to_string(),
            },
            FileCommand::CreateFolder {
                parent: root.path().to_path_buf(),
                name: "LPT³".to_string(),
            },
        ] {
            assert!(
                DefaultPlatform.execute_command(&command).is_err(),
                "{command:?}"
            );
        }
        assert!(
            std::fs::read_dir(root.path())
                .expect("read root")
                .next()
                .is_none()
        );
    }

    #[cfg(target_os = "linux")]
    fn cross_filesystem_test_dirs(name: &str) -> Result<Option<(TestDir, TestDir)>, String> {
        use std::os::unix::fs::MetadataExt;

        let temporary = std::env::temp_dir();
        let temporary_device = std::fs::metadata(&temporary)
            .map_err(|error| {
                format!(
                    "cannot inspect temporary directory {}: {error}",
                    temporary.display()
                )
            })?
            .dev();
        let mut candidates = std::env::var_os("HOME")
            .map(PathBuf::from)
            .into_iter()
            .collect::<Vec<_>>();
        candidates.push(
            std::env::current_dir()
                .map_err(|error| format!("cannot inspect current directory: {error}"))?,
        );
        let mut inspection_errors = Vec::new();
        for candidate in candidates {
            match std::fs::metadata(&candidate) {
                Ok(metadata) if !metadata.is_dir() => inspection_errors.push(format!(
                    "source parent {} is not a directory",
                    candidate.display()
                )),
                Ok(metadata) if metadata.dev() != temporary_device => {
                    return Ok(Some((
                        TestDir::new_at(&candidate, &format!("{name}-source")),
                        TestDir::new_at(&temporary, &format!("{name}-destination")),
                    )));
                }
                Ok(_) => {}
                Err(error) => inspection_errors.push(format!(
                    "cannot inspect source parent {}: {error}",
                    candidate.display()
                )),
            }
        }
        if inspection_errors.is_empty() {
            Ok(None)
        } else {
            Err(inspection_errors.join("; "))
        }
    }

    #[cfg(target_os = "linux")]
    fn assert_copy_completed_with_retained_stage(
        result: Result<Option<FileProperties>, PlatformError>,
    ) {
        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
    }

    #[test]
    fn rename_collision_does_not_overwrite_either_file() {
        let dir = TestDir::new("rename-collision");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("destination.txt");
        std::fs::write(&source, "source contents").unwrap();
        std::fs::write(&destination, "destination contents").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Rename {
            source: source.clone(),
            new_name: "destination.txt".to_string(),
        });

        assert!(
            matches!(result, Err(PlatformError::DestinationExists(_))),
            "unexpected Rename collision result: {result:?}"
        );
        assert_eq!(std::fs::read_to_string(source).unwrap(), "source contents");
        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            "destination contents"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_receipt_removes_the_exact_published_entry() {
        let dir = TestDir::new("copy-undo");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "copied bytes").unwrap();

        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap()
            .expect("copy receipt");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "copied bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");

        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();

        assert!(!destination.exists());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_preserves_in_place_edits_and_original_overwrite_backups() {
        for directory in [false, true] {
            for overwrite in [false, true] {
                let root = TestDir::new("copy-content-edited");
                let source_dir = root.path().join("source");
                let destination_dir = root.path().join("destination");
                std::fs::create_dir(&source_dir).unwrap();
                std::fs::create_dir(&destination_dir).unwrap();
                let source = source_dir.join("item");
                let destination = destination_dir.join("item");
                let data_path = |entry: &Path| {
                    if directory {
                        entry.join("child.txt")
                    } else {
                        entry.to_path_buf()
                    }
                };
                if directory {
                    std::fs::create_dir(&source).unwrap();
                }
                std::fs::write(data_path(&source), "copied original").unwrap();
                if overwrite {
                    if directory {
                        std::fs::create_dir(&destination).unwrap();
                    }
                    std::fs::write(data_path(&destination), "displaced original").unwrap();
                }
                let receipt = DefaultPlatform
                    .execute_copy_with_undo_receipt(
                        &source,
                        &destination_dir,
                        if overwrite {
                            ConflictAction::Overwrite
                        } else {
                            ConflictAction::Skip
                        },
                    )
                    .unwrap()
                    .unwrap();
                let published_identity = rename_identity(&destination).unwrap();
                std::fs::write(data_path(&destination), "later user edit").unwrap();
                assert_eq!(rename_identity(&destination).unwrap(), published_identity);
                assert!(matches!(
                    DefaultPlatform.undo_copy_with_receipt(&receipt),
                    Err(PlatformError::UndoContentChanged(_))
                ));
                assert_eq!(
                    std::fs::read_to_string(data_path(&destination)).unwrap(),
                    "later user edit"
                );
                assert_eq!(
                    std::fs::read_to_string(data_path(&source)).unwrap(),
                    "copied original"
                );
                if overwrite {
                    assert_eq!(
                        std::fs::read_to_string(data_path(&receipt.before_path)).unwrap(),
                        "displaced original"
                    );
                } else {
                    assert!(!receipt.before_path.exists());
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_preserves_backup_edits_from_an_original_open_writer() {
        use std::os::unix::fs::FileExt;
        for directory in [false, true] {
            let root = TestDir::new("copy-backup-open-writer");
            let source_dir = root.path().join("source");
            let destination_dir = root.path().join("destination");
            std::fs::create_dir(&source_dir).unwrap();
            std::fs::create_dir(&destination_dir).unwrap();
            let source = source_dir.join("item");
            let destination = destination_dir.join("item");
            let data_path = |entry: &Path| {
                if directory {
                    entry.join("child.txt")
                } else {
                    entry.to_path_buf()
                }
            };
            if directory {
                std::fs::create_dir(&source).unwrap();
                std::fs::create_dir(&destination).unwrap();
            }
            std::fs::write(data_path(&source), "replacement bytes").unwrap();
            std::fs::write(data_path(&destination), "original bytes").unwrap();
            let writer = std::fs::OpenOptions::new()
                .write(true)
                .open(data_path(&destination))
                .unwrap();
            let original_identity = rename_identity(&destination).unwrap();
            let receipt = DefaultPlatform
                .execute_copy_with_undo_receipt(
                    &source,
                    &destination_dir,
                    ConflictAction::Overwrite,
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                rename_identity(&receipt.before_path).unwrap(),
                original_identity
            );
            writer.set_len(0).unwrap();
            writer.write_all_at(b"later editor bytes", 0).unwrap();
            writer.sync_all().unwrap();
            assert_eq!(
                rename_identity(&receipt.before_path).unwrap(),
                original_identity
            );
            assert!(matches!(
                DefaultPlatform.undo_copy_with_receipt(&receipt),
                Err(PlatformError::UndoContentChanged(_))
            ));
            assert_eq!(
                std::fs::read_to_string(data_path(&source)).unwrap(),
                "replacement bytes"
            );
            assert_eq!(
                std::fs::read_to_string(data_path(&destination)).unwrap(),
                "replacement bytes"
            );
            assert_eq!(
                std::fs::read_to_string(data_path(&receipt.before_path)).unwrap(),
                "later editor bytes"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn conflict_renamed_copy_undo_removes_the_exact_published_entry() {
        let dir = TestDir::new("copy-rename-undo");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let occupied = destination_dir.join("entry.txt");
        let renamed = destination_dir.join("entry (1).txt");
        std::fs::write(&source, "copied bytes").unwrap();
        std::fs::write(&occupied, "existing bytes").unwrap();

        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination_dir, ConflictAction::Rename)
            .unwrap()
            .expect("renamed copy receipt");
        assert_eq!(receipt.after_path(), renamed);
        assert_eq!(std::fs::read_to_string(&renamed).unwrap(), "copied bytes");

        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();

        assert!(!renamed.exists());
        assert_eq!(
            std::fs::read_to_string(&occupied).unwrap(),
            "existing bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_copy_undo_restores_the_exact_displaced_destination() {
        let dir = TestDir::new("copy-overwrite-undo");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "copied bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let displaced_identity = rename_identity(&destination).unwrap();

        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap()
            .expect("overwrite copy receipt");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "copied bytes"
        );

        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();

        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert_eq!(rename_identity(&destination).unwrap(), displaced_identity);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_copy_undo_refuses_a_replaced_displaced_destination() {
        let dir = TestDir::new("copy-overwrite-undo-stale-displaced");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "copied bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap()
            .expect("overwrite copy receipt");
        std::fs::remove_file(receipt.before_path()).unwrap();
        std::fs::write(receipt.before_path(), "replacement displaced bytes").unwrap();

        let result = DefaultPlatform.undo_copy_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "copied bytes"
        );
        assert_eq!(
            std::fs::read_to_string(receipt.before_path()).unwrap(),
            "replacement displaced bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn create_file_undo_receipt_withdraws_only_the_exact_unchanged_entry() {
        let dir = TestDir::new("create-file-undo");
        let created = dir.path().join("notes.txt");

        let receipt = DefaultPlatform
            .execute_create_file_with_undo_receipt(dir.path(), "notes.txt")
            .unwrap()
            .expect("create file receipt");
        assert_eq!(std::fs::read(&created).unwrap(), b"");

        std::fs::remove_file(&created).unwrap();
        std::fs::write(&created, "replacement bytes").unwrap();
        assert!(matches!(
            DefaultPlatform.undo_create_file_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        assert_eq!(
            std::fs::read_to_string(&created).unwrap(),
            "replacement bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn create_file_undo_refuses_to_withdraw_an_edited_file() {
        let dir = TestDir::new("create-file-undo-edited");
        let created = dir.path().join("notes.txt");
        let receipt = DefaultPlatform
            .execute_create_file_with_undo_receipt(dir.path(), "notes.txt")
            .unwrap()
            .expect("create file receipt");

        std::fs::write(&created, "user content").unwrap();
        assert!(matches!(
            DefaultPlatform.undo_create_file_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        assert_eq!(std::fs::read_to_string(created).unwrap(), "user content");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn create_folder_undo_receipt_withdraws_the_exact_unchanged_empty_directory() {
        let dir = TestDir::new("create-folder-undo");
        let created = dir.path().join("Notes");

        let receipt = DefaultPlatform
            .execute_create_folder_with_undo_receipt(dir.path(), "Notes")
            .unwrap()
            .expect("create folder receipt");
        assert!(created.is_dir());

        DefaultPlatform
            .undo_create_folder_with_receipt(&receipt)
            .unwrap();

        assert!(!created.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn create_folder_undo_refuses_to_withdraw_a_populated_directory() {
        let dir = TestDir::new("create-folder-undo-populated");
        let created = dir.path().join("Notes");
        let child = created.join("saved.txt");
        let receipt = DefaultPlatform
            .execute_create_folder_with_undo_receipt(dir.path(), "Notes")
            .unwrap()
            .expect("create folder receipt");
        std::fs::write(&child, "user content").unwrap();

        assert!(matches!(
            DefaultPlatform.undo_create_folder_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        assert_eq!(std::fs::read_to_string(child).unwrap(), "user content");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlink_undo_receipt_withdraws_the_exact_unchanged_link_entry() {
        let dir = TestDir::new("symlink-undo");
        let link = dir.path().join("shortcut");
        let target = PathBuf::from("missing-relative-target");

        let receipt = DefaultPlatform
            .execute_symlink_with_undo_receipt(&link, &target)
            .unwrap()
            .expect("symlink receipt");
        assert_eq!(std::fs::read_link(&link).unwrap(), target);

        DefaultPlatform.undo_symlink_with_receipt(&receipt).unwrap();

        assert!(std::fs::symlink_metadata(&link).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlink_undo_refuses_to_withdraw_a_pathname_replacement() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("symlink-undo-replacement");
        let link = dir.path().join("shortcut");
        let receipt = DefaultPlatform
            .execute_symlink_with_undo_receipt(&link, Path::new("original-target"))
            .unwrap()
            .expect("symlink receipt");
        std::fs::remove_file(&link).unwrap();
        symlink("replacement-target", &link).unwrap();

        assert!(matches!(
            DefaultPlatform.undo_symlink_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("replacement-target")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chmod_undo_receipt_restores_the_prior_mode_of_the_exact_unchanged_entry() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TestDir::new("chmod-undo");
        let target = dir.path().join("script.sh");
        std::fs::write(&target, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();

        let receipt = DefaultPlatform
            .execute_chmod_with_undo_receipt(&target, "755")
            .unwrap()
            .expect("chmod receipt");
        assert_eq!(
            std::fs::symlink_metadata(&target)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );

        DefaultPlatform.undo_chmod_with_receipt(&receipt).unwrap();

        assert_eq!(
            std::fs::symlink_metadata(&target)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o640
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chmod_undo_refuses_to_mutate_a_pathname_replacement() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TestDir::new("chmod-undo-replacement");
        let target = dir.path().join("script.sh");
        std::fs::write(&target, "original").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        let receipt = DefaultPlatform
            .execute_chmod_with_undo_receipt(&target, "755")
            .unwrap()
            .expect("chmod receipt");

        std::fs::remove_file(&target).unwrap();
        std::fs::write(&target, "replacement").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();

        assert!(matches!(
            DefaultPlatform.undo_chmod_with_receipt(&receipt),
            Err(PlatformError::StaleChmodUndo(_))
        ));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "replacement");
        assert_eq!(
            std::fs::symlink_metadata(target)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn chmod_command_rejects_symlinks_without_changing_the_target() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TestDir::new("chmod-symlink-rejection");
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        std::fs::write(&target, "target bytes").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Chmod {
            target: link,
            mode_str: "755".to_string(),
        });

        assert!(result.is_err());
        assert_eq!(
            std::fs::symlink_metadata(target)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o640
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_rejects_replaced_retained_directory_and_parent_even_with_published_hardlink() {
        for replace_parent in [false, true] {
            let root = TestDir::new("copy-directory-retarget");
            let source = root.path().join("source");
            let destination = root.path().join("destination");
            std::fs::write(&source, "copied bytes").unwrap();
            std::fs::create_dir(&destination).unwrap();
            let receipt = DefaultPlatform
                .execute_copy_with_undo_receipt(&source, &destination, ConflictAction::Skip)
                .unwrap()
                .unwrap();
            let stage = receipt.before_path().parent().unwrap().to_path_buf();
            let saved = root.path().join("held-original");
            if replace_parent {
                std::fs::rename(&destination, &saved).unwrap();
                std::fs::create_dir(&destination).unwrap();
                // Entry identity alone cannot distinguish this new parent.
                std::fs::hard_link(saved.join("source"), destination.join("source")).unwrap();
                std::fs::create_dir(&stage).unwrap();
            } else {
                std::fs::rename(&stage, &saved).unwrap();
                std::fs::create_dir(&stage).unwrap();
            }
            std::fs::write(stage.join("candidate"), "unrelated replacement data").unwrap();
            assert!(matches!(
                DefaultPlatform.undo_copy_with_receipt(&receipt),
                Err(PlatformError::StalePathMoveUndo(_))
            ));
            assert_eq!(
                std::fs::read_to_string(destination.join("source")).unwrap(),
                "copied bytes"
            );
            assert_eq!(
                std::fs::read_to_string(stage.join("candidate")).unwrap(),
                "unrelated replacement data"
            );
            if replace_parent {
                std::fs::rename(&destination, root.path().join("preserved-replacement")).unwrap();
                std::fs::rename(saved, &destination).unwrap();
            } else {
                std::fs::rename(&stage, root.path().join("preserved-replacement")).unwrap();
                std::fs::rename(saved, &stage).unwrap();
            }
            DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
            assert!(!destination.join("source").exists());
            assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
            assert_eq!(
                std::fs::read_to_string(receipt.before_path()).unwrap(),
                "copied bytes"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_rejects_same_filesystem_destination_alias_retarget_with_identical_file_inode() {
        use std::os::unix::fs::symlink;
        let root = TestDir::new("copy-alias-retarget");
        let actual = root.path().join("actual");
        let other = root.path().join("other");
        let alias = root.path().join("alias");
        let source = root.path().join("source");
        std::fs::create_dir(&actual).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::write(&source, "copied bytes").unwrap();
        symlink(&actual, &alias).unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &alias, ConflictAction::Skip)
            .unwrap()
            .unwrap();
        std::fs::hard_link(actual.join("source"), other.join("source")).unwrap();
        std::fs::remove_file(&alias).unwrap();
        symlink(&other, &alias).unwrap();
        assert!(matches!(
            DefaultPlatform.undo_copy_with_receipt(&receipt),
            Err(PlatformError::StalePathMoveUndo(_))
        ));
        for parent in [&actual, &other] {
            assert_eq!(
                std::fs::read_to_string(parent.join("source")).unwrap(),
                "copied bytes"
            );
        }
        std::fs::remove_file(&alias).unwrap();
        symlink(&actual, &alias).unwrap();
        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
        assert!(!actual.join("source").exists());
        assert_eq!(
            std::fs::read_to_string(other.join("source")).unwrap(),
            "copied bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_follows_destination_directory_alias_across_filesystems() {
        use std::os::unix::fs::{MetadataExt, symlink};
        let root = TestDir::new("copy-cross-filesystem-alias");
        let other = tempfile::tempdir_in("/dev/shm").unwrap();
        assert_ne!(
            root.path().metadata().unwrap().dev(),
            other.path().metadata().unwrap().dev(),
            "native fixture requires two filesystems"
        );
        let alias = root.path().join("alias");
        let source = root.path().join("source");
        std::fs::write(&source, "copied bytes").unwrap();
        symlink(other.path(), &alias).unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &alias, ConflictAction::Skip)
            .unwrap()
            .unwrap();
        DefaultPlatform.undo_copy_with_receipt(&receipt).unwrap();
        assert!(!other.path().join("source").exists());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
        assert_eq!(
            std::fs::read_to_string(receipt.before_path()).unwrap(),
            "copied bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_withdrawal_remains_in_original_directory_when_alias_changes_after_validation() {
        use std::os::unix::fs::symlink;
        let root = TestDir::new("copy-withdrawal-alias");
        let actual = root.path().join("actual");
        let other = root.path().join("other");
        let alias = root.path().join("alias");
        let source = root.path().join("source");
        std::fs::create_dir(&actual).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::write(&source, "copied bytes").unwrap();
        symlink(&actual, &alias).unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &alias, ConflictAction::Skip)
            .unwrap()
            .unwrap();
        let directory = receipt.retained_copy_directory.as_ref().unwrap();
        directory
            .validate_publication_parent(receipt.after_path())
            .unwrap();
        std::fs::hard_link(actual.join("source"), other.join("source")).unwrap();
        std::fs::remove_file(&alias).unwrap();
        symlink(&other, &alias).unwrap();
        // Exercise the production primitive after an adversarial alias change
        // between validation and its syscall; pathname rename would hit other.
        directory
            .withdraw_publication(receipt.after_path(), receipt.before_path(), false)
            .unwrap();
        assert!(!actual.join("source").exists());
        assert_eq!(
            std::fs::read_to_string(other.join("source")).unwrap(),
            "copied bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
        assert_eq!(
            std::fs::read_to_string(receipt.before_path()).unwrap(),
            "copied bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires an explicitly owned private user/mount namespace"]
    fn copy_receipt_does_not_pin_mount_and_refuses_after_unmount_and_replacement() {
        use std::process::Command;
        let parent_namespace = std::env::var("OTTRIN_QA_PARENT_MOUNT_NAMESPACE")
            .expect("explicit private namespace required");
        let current_namespace = std::fs::read_link("/proc/self/ns/mnt").unwrap();
        assert_ne!(current_namespace.to_string_lossy(), parent_namespace);
        assert!(
            Command::new("mount")
                .args(["--make-rprivate", "/"])
                .status()
                .unwrap()
                .success()
        );
        // Keep every fixture pathname on a namespace-owned filesystem. Even
        // SIGKILL/cancellation cannot leave fixture data in the host /tmp.
        assert!(
            Command::new("mount")
                .args(["-t", "tmpfs", "-o", "mode=1777", "tmpfs", "/tmp"])
                .status()
                .unwrap()
                .success()
        );
        let root = tempfile::tempdir().unwrap();
        let mount_path = root.path().join("destination");
        std::fs::create_dir(&mount_path).unwrap();
        struct PrivateMount(std::path::PathBuf);
        impl Drop for PrivateMount {
            fn drop(&mut self) {
                // This verified-private pathname is owned by this fixture.
                // Ordinary unmount is asserted below; lazy detach here is only
                // failure/cancellation teardown for the disposable namespace.
                let _ = Command::new("umount").arg("-l").arg(&self.0).output();
            }
        }
        let mount = PrivateMount(mount_path.clone());
        let attach = || {
            assert!(
                Command::new("mount")
                    .args(["-t", "tmpfs", "tmpfs"])
                    .arg(&mount.0)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        attach();
        let source = root.path().join("source");
        std::fs::write(&source, "original source").unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &mount_path, ConflictAction::Skip)
            .unwrap()
            .unwrap();
        assert_eq!(
            std::fs::read(mount_path.join("source")).unwrap(),
            b"original source"
        );
        let unmount = Command::new("umount").arg(&mount.0).output().unwrap();
        assert!(
            unmount.status.success(),
            "live receipt prevented unmount: {}",
            String::from_utf8_lossy(&unmount.stderr)
        );
        attach();
        let replacement = mount_path.join("source");
        std::fs::write(&replacement, "replacement data").unwrap();
        assert!(DefaultPlatform.undo_copy_with_receipt(&receipt).is_err());
        assert_eq!(std::fs::read(&replacement).unwrap(), b"replacement data");
        assert_eq!(std::fs::read(&source).unwrap(), b"original source");
        assert!(
            Command::new("umount")
                .arg(&mount.0)
                .status()
                .unwrap()
                .success()
        );
        // Bind-mount replacement deliberately preserves directory/file inode
        // identities: unique mount generation must still invalidate the receipt.
        let backing = root.path().join("backing");
        std::fs::create_dir(&backing).unwrap();
        let bind = || {
            assert!(
                Command::new("mount")
                    .arg("--bind")
                    .arg(&backing)
                    .arg(&mount.0)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        bind();
        let bind_receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &mount_path, ConflictAction::Skip)
            .unwrap()
            .unwrap();
        assert!(
            Command::new("umount")
                .arg(&mount.0)
                .status()
                .unwrap()
                .success()
        );
        bind();
        assert_eq!(
            rename_identity(bind_receipt.after_path()).unwrap(),
            bind_receipt.identity
        );
        assert!(
            DefaultPlatform
                .undo_copy_with_receipt(&bind_receipt)
                .is_err()
        );
        assert_eq!(
            std::fs::read(mount_path.join("source")).unwrap(),
            b"original source"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"original source");
        assert!(
            Command::new("umount")
                .arg(&mount.0)
                .status()
                .unwrap()
                .success()
        );
        drop(mount);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_receipt_is_not_issued_for_an_entry_replaced_after_publication() {
        let dir = TestDir::new("copy-receipt-publication-race");
        let transaction = dir.path().join("transaction");
        let published = dir.path().join("published.txt");
        std::fs::create_dir(&transaction).unwrap();
        std::fs::write(&published, "copied bytes").unwrap();
        let published_identity = rename_identity(&published).unwrap();
        std::fs::remove_file(&published).unwrap();
        std::fs::write(&published, "replacement bytes").unwrap();

        let result = undo_receipt_for_published_identity(
            transaction,
            published.clone(),
            published_identity,
            None,
            PathMoveUndoRequirement::None,
            "copy",
            None,
        );

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(published).unwrap(),
            "replacement bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_undo_rejects_a_replaced_published_entry_without_mutation() {
        let dir = TestDir::new("copy-undo-stale");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "copied bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_copy_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap()
            .expect("copy receipt");
        std::fs::remove_file(&destination).unwrap();
        std::fs::write(&destination, "replacement bytes").unwrap();

        let result = DefaultPlatform.undo_copy_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "replacement bytes"
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "copied bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_receipt_restores_the_original_entry() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("rename-undo");
        let before = dir.path().join("before.txt");
        let after = dir.path().join("after.txt");
        std::fs::write(&before, "original bytes").unwrap();
        let before_metadata = std::fs::symlink_metadata(&before).unwrap();

        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after.txt")
            .unwrap()
            .expect("rename receipt");
        assert!(!before.exists());
        assert_eq!(std::fs::read_to_string(&after).unwrap(), "original bytes");

        DefaultPlatform.undo_rename_with_receipt(&receipt).unwrap();

        let restored_metadata = std::fs::symlink_metadata(&before).unwrap();
        assert_eq!(std::fs::read_to_string(&before).unwrap(), "original bytes");
        assert!(!after.exists());
        assert_eq!(restored_metadata.dev(), before_metadata.dev());
        assert_eq!(restored_metadata.ino(), before_metadata.ino());
    }

    #[test]
    fn rename_command_rejects_a_cross_parent_name_without_mutation() {
        let dir = TestDir::new("rename-cross-parent-name");
        let source_parent = dir.path().join("source");
        std::fs::create_dir(&source_parent).unwrap();
        let source = source_parent.join("before.txt");
        let outside = dir.path().join("after.txt");
        std::fs::write(&source, "original bytes").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Rename {
            source: source.clone(),
            new_name: "../after.txt".to_string(),
        });

        assert!(matches!(result, Err(PlatformError::InvalidDestination(_))));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "original bytes");
        assert!(!outside.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn move_undo_receipt_restores_the_original_entry() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("move-undo");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "original bytes").unwrap();
        let before_metadata = std::fs::symlink_metadata(&source).unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("move receipt");
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "original bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        let restored_metadata = std::fs::symlink_metadata(&source).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "original bytes");
        assert!(!destination.exists());
        assert_eq!(restored_metadata.dev(), before_metadata.dev());
        assert_eq!(restored_metadata.ino(), before_metadata.ino());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn renamed_move_undo_restores_the_exact_original_entry() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("renamed-move-undo");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let occupied_destination = destination_dir.join("entry.txt");
        let renamed_destination = destination_dir.join("entry (1).txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&occupied_destination, "occupied bytes").unwrap();
        let before_metadata = std::fs::symlink_metadata(&source).unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Rename)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("renamed move receipt");
        assert_eq!(receipt.after_path(), renamed_destination);
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&occupied_destination).unwrap(),
            "occupied bytes"
        );
        assert_eq!(
            std::fs::read_to_string(&renamed_destination).unwrap(),
            "moved bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        let restored_metadata = std::fs::symlink_metadata(&source).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&occupied_destination).unwrap(),
            "occupied bytes"
        );
        assert!(!renamed_destination.exists());
        assert_eq!(restored_metadata.dev(), before_metadata.dev());
        assert_eq!(restored_metadata.ino(), before_metadata.ino());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_move_undo_restores_moved_and_displaced_entries() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("move-overwrite-undo");
        let _recovery = TestMoveRecoveryRoot::new(dir.path().join("move-state"));
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let source_metadata = std::fs::symlink_metadata(&source).unwrap();
        let displaced_metadata = std::fs::symlink_metadata(&destination).unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("overwrite move receipt");
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        let restored_source_metadata = std::fs::symlink_metadata(&source).unwrap();
        let restored_destination_metadata = std::fs::symlink_metadata(&destination).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert_eq!(restored_source_metadata.dev(), source_metadata.dev());
        assert_eq!(restored_source_metadata.ino(), source_metadata.ino());
        assert_eq!(
            restored_destination_metadata.dev(),
            displaced_metadata.dev()
        );
        assert_eq!(
            restored_destination_metadata.ino(),
            displaced_metadata.ino()
        );
        assert!(
            discover_retained_move_transactions_at(&dir.path().join("move-state"))
                .unwrap()
                .is_empty()
        );
        assert!(!std::fs::read_dir(&source_dir).unwrap().any(|entry| {
            entry.is_ok_and(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.len() == ".ottrin-move-".len() + 32
                    && name.starts_with(".ottrin-move-")
                    && name[".ottrin-move-".len()..]
                        .chars()
                        .all(|character| character.is_ascii_hexdigit())
            })
        }));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_move_undo_rejects_published_content_edited_in_place() {
        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-undo-edited-output") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        let destination_dir = destination_root.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();

        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap();
        let receipt = receipt.expect("cross-filesystem Move receipt");
        std::fs::write(&destination, "edited bytes").unwrap();

        let error = DefaultPlatform
            .undo_move_with_receipt(&receipt)
            .expect_err("edited published content must refuse Undo");

        assert!(matches!(error, PlatformError::UndoContentChanged(_)));
        assert!(
            !source.exists(),
            "refused Undo must not recreate the source"
        );
        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            "edited bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_directory_move_undo_restores_exact_tree_and_withdraws_destination() {
        use std::os::unix::fs::symlink;

        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-directory-undo") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_parent = source_root.path().join("source");
        let destination_parent = destination_root.path().join("destination");
        let source = source_parent.join("tree");
        let nested = source.join("nested/deeper");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&destination_parent).unwrap();
        std::fs::write(source.join("root.txt"), b"root payload").unwrap();
        std::fs::write(nested.join("payload.bin"), b"nested payload\0bytes").unwrap();
        symlink("nested/deeper/payload.bin", source.join("payload-link")).unwrap();
        symlink("missing-target", nested.join("dangling-link")).unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_parent, ConflictAction::Skip)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("cross-filesystem directory Move receipt");
        let published = destination_parent.join("tree");
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(
            std::fs::read(published.join("nested/deeper/payload.bin")).unwrap(),
            b"nested payload\0bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert!(
            std::fs::symlink_metadata(&published).is_err(),
            "Undo must withdraw the published destination tree"
        );
        assert_eq!(
            std::fs::read(source.join("root.txt")).unwrap(),
            b"root payload"
        );
        assert_eq!(
            std::fs::read(source.join("nested/deeper/payload.bin")).unwrap(),
            b"nested payload\0bytes"
        );
        assert_eq!(
            std::fs::read_link(source.join("payload-link")).unwrap(),
            PathBuf::from("nested/deeper/payload.bin")
        );
        assert_eq!(
            std::fs::read_link(source.join("nested/deeper/dangling-link")).unwrap(),
            PathBuf::from("missing-target")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_overwrite_undo_rejects_stale_retained_destination_before_restoring_source()
    {
        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-overwrite-undo-stale-retained") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        std::fs::create_dir_all(&source_dir).expect("create source directory");
        let source = source_dir.join("entry.txt");
        let destination = destination_root.path().join("destination");
        std::fs::create_dir_all(&destination).expect("create destination");
        let target = destination.join("entry.txt");
        std::fs::write(&source, b"incoming").expect("write source");
        std::fs::write(&target, b"existing").expect("write destination");

        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination, ConflictAction::Overwrite)
            .expect("cross-filesystem overwrite should move");
        let receipt = receipt.expect("moved entry should retain an undo receipt");
        let retained = receipt
            .retained_displaced_path
            .as_ref()
            .expect("overwrite should retain displaced destination");
        let preserved = retained.with_file_name("preserved-retained-destination");
        std::fs::rename(retained, &preserved).expect("preserve retained destination");
        std::fs::write(retained, b"tampered").expect("replace retained destination");

        let error = DefaultPlatform
            .undo_move_with_receipt(&receipt)
            .expect_err("stale retained data must reject undo");

        assert!(matches!(error, PlatformError::StalePathMoveUndo(_)));
        assert!(
            !source.exists(),
            "rejected undo must not recreate the source"
        );
        assert_eq!(std::fs::read(&target).expect("read target"), b"incoming");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_overwrite_move_is_undoable() {
        let (source_root, destination_root) = match cross_filesystem_test_dirs(
            "cross-device-overwrite-undo",
        ) {
            Ok(Some(dirs)) => dirs,
            Ok(None) => {
                eprintln!(
                    "skipping cross-filesystem Move Undo test: HOME and current directory share /tmp's device"
                );
                return;
            }
            Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
        };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        let destination_dir = destination_root.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("cross-filesystem overwrite Move receipt");
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert!(
            discover_retained_move_transactions_at(&source_root.path().join("move-state"))
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_overwrite_undo_retries_after_exchange_interruption() {
        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-overwrite-undo-retry") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        let destination_dir = destination_root.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();

        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("cross-filesystem overwrite Move receipt");
        let interrupted =
            undo_cross_filesystem_path_move_linux_with_hook(&receipt, &source_dir, |point| {
                match point {
                    CrossFilesystemUndoPoint::PlainSourceRestored => Ok(()),
                    CrossFilesystemUndoPoint::PlainDestinationQuarantined => Ok(()),
                    CrossFilesystemUndoPoint::OverwriteExchangeCompleted => {
                        Err("simulate interruption")
                    }
                }
            });
        assert!(matches!(
            interrupted,
            Err(PlatformError::CommittedUndoCleanupPending { .. })
        ));

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert!(
            discover_retained_move_transactions_at(&source_root.path().join("move-state"))
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plain_cross_filesystem_move_undo_retries_after_source_restore_interruption() {
        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-plain-undo-source-restore-retry") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        let destination_dir = destination_root.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();

        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap();
        let receipt = receipt.expect("cross-filesystem Move receipt");
        let interrupted =
            undo_cross_filesystem_path_move_linux_with_hook(&receipt, &source_dir, |point| {
                match point {
                    CrossFilesystemUndoPoint::PlainSourceRestored => Err("simulate interruption"),
                    CrossFilesystemUndoPoint::PlainDestinationQuarantined => Ok(()),
                    CrossFilesystemUndoPoint::OverwriteExchangeCompleted => Ok(()),
                }
            });
        assert!(matches!(
            interrupted,
            Err(PlatformError::CommittedUndoCleanupPending { .. })
        ));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert!(!destination.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn plain_cross_filesystem_move_undo_retries_after_destination_quarantine_interruption() {
        let (source_root, destination_root) =
            match cross_filesystem_test_dirs("cross-device-plain-undo-quarantine-retry") {
                Ok(Some(dirs)) => dirs,
                Ok(None) => return,
                Err(reason) => panic!("cannot prepare cross-filesystem Move Undo test: {reason}"),
            };
        let _recovery = TestMoveRecoveryRoot::new(source_root.path().join("move-state"));
        let source_dir = source_root.path().join("source");
        let destination_dir = destination_root.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Skip)
            .unwrap();
        let receipt = receipt.expect("cross-filesystem Move receipt");

        let interrupted =
            undo_cross_filesystem_path_move_linux_with_hook(&receipt, &source_dir, |point| {
                match point {
                    CrossFilesystemUndoPoint::PlainDestinationQuarantined => {
                        Err("simulate interruption")
                    }
                    CrossFilesystemUndoPoint::PlainSourceRestored
                    | CrossFilesystemUndoPoint::OverwriteExchangeCompleted => Ok(()),
                }
            });
        assert!(matches!(
            interrupted,
            Err(PlatformError::CommittedUndoCleanupPending { .. })
        ));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert!(!destination.exists());

        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert!(!destination.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwrite_move_without_a_conflict_remains_a_plain_undoable_move() {
        let dir = TestDir::new("move-overwrite-without-conflict");
        let _recovery = TestMoveRecoveryRoot::new(dir.path().join("move-state"));
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();

        let (disposition, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();

        assert_eq!(disposition, MoveDisposition::Moved);
        let receipt = receipt.expect("plain move receipt");
        assert!(receipt.retained_displaced_path.is_none());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert!(!destination.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_move_undo_source_collision_refuses_without_mutation() {
        let dir = TestDir::new("move-overwrite-undo-source-collision");
        let _recovery = TestMoveRecoveryRoot::new(dir.path().join("move-state"));
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("overwrite move receipt");
        std::fs::write(&source, "source collision").unwrap();

        let result = DefaultPlatform.undo_move_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::DestinationExists(_))));
        assert_eq!(
            std::fs::read_to_string(&source).unwrap(),
            "source collision"
        );
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );
        assert_eq!(
            std::fs::read_to_string(receipt.retained_displaced_path.as_ref().unwrap()).unwrap(),
            "displaced bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_move_undo_rejects_a_replaced_published_entry_without_mutation() {
        let dir = TestDir::new("move-overwrite-undo-published-replacement");
        let _recovery = TestMoveRecoveryRoot::new(dir.path().join("move-state"));
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("overwrite move receipt");
        std::fs::remove_file(&destination).unwrap();
        std::fs::write(&destination, "published replacement").unwrap();

        let result = DefaultPlatform.undo_move_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "published replacement"
        );
        assert_eq!(
            std::fs::read_to_string(receipt.retained_displaced_path.as_ref().unwrap()).unwrap(),
            "displaced bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_move_undo_rejects_a_replaced_retained_entry_without_mutation() {
        let dir = TestDir::new("move-overwrite-undo-retained-replacement");
        let _recovery = TestMoveRecoveryRoot::new(dir.path().join("move-state"));
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("overwrite move receipt");
        let retained = receipt.retained_displaced_path.as_ref().unwrap();
        std::fs::remove_file(retained).unwrap();
        std::fs::write(retained, "retained replacement").unwrap();

        let result = DefaultPlatform.undo_move_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "moved bytes"
        );
        assert_eq!(
            std::fs::read_to_string(retained).unwrap(),
            "retained replacement"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn overwritten_move_undo_cleanup_failure_is_retryable_after_paths_are_restored() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TestDir::new("move-overwrite-undo-cleanup-retry");
        let recovery_root = dir.path().join("move-state");
        let _recovery = TestMoveRecoveryRoot::new(recovery_root.clone());
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&destination_dir).unwrap();
        let source = source_dir.join("entry.txt");
        let destination = destination_dir.join("entry.txt");
        std::fs::write(&source, "moved bytes").unwrap();
        std::fs::write(&destination, "displaced bytes").unwrap();
        let (_, receipt) = DefaultPlatform
            .execute_move_with_undo_receipt(&source, &destination_dir, ConflictAction::Overwrite)
            .unwrap();
        let receipt = receipt.expect("overwrite move receipt");
        std::fs::set_permissions(&recovery_root, std::fs::Permissions::from_mode(0o500)).unwrap();

        let first_result = DefaultPlatform.undo_move_with_receipt(&receipt);

        assert!(matches!(
            first_result,
            Err(PlatformError::CommittedUndoCleanupPending { .. })
        ));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert!(
            !discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );

        std::fs::set_permissions(&recovery_root, std::fs::Permissions::from_mode(0o700)).unwrap();
        DefaultPlatform.undo_move_with_receipt(&receipt).unwrap();

        assert_eq!(std::fs::read_to_string(&source).unwrap(), "moved bytes");
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "displaced bytes"
        );
        assert!(
            discover_retained_move_transactions_at(&recovery_root)
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_collision_preserves_both_entries() {
        let dir = TestDir::new("rename-undo-collision");
        let before = dir.path().join("before.txt");
        let after = dir.path().join("after.txt");
        std::fs::write(&before, "original bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after.txt")
            .unwrap()
            .expect("rename receipt");
        std::fs::write(&before, "replacement bytes").unwrap();

        let result = DefaultPlatform.undo_rename_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::DestinationExists(_))));
        assert_eq!(
            std::fs::read_to_string(&before).unwrap(),
            "replacement bytes"
        );
        assert_eq!(std::fs::read_to_string(&after).unwrap(), "original bytes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_rejects_a_replaced_destination_without_mutation() {
        let dir = TestDir::new("rename-undo-stale");
        let before = dir.path().join("before.txt");
        let after = dir.path().join("after.txt");
        std::fs::write(&before, "original bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after.txt")
            .unwrap()
            .expect("rename receipt");
        std::fs::remove_file(&after).unwrap();
        std::fs::write(&after, "replacement bytes").unwrap();

        let result = DefaultPlatform.undo_rename_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert!(!before.exists());
        assert_eq!(
            std::fs::read_to_string(&after).unwrap(),
            "replacement bytes"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_rejects_a_missing_destination_without_creating_the_original_name() {
        let dir = TestDir::new("rename-undo-missing");
        let before = dir.path().join("before.txt");
        let after = dir.path().join("after.txt");
        std::fs::write(&before, "original bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after.txt")
            .unwrap()
            .expect("rename receipt");
        std::fs::remove_file(&after).unwrap();

        let result = DefaultPlatform.undo_rename_with_receipt(&receipt);

        assert!(matches!(result, Err(PlatformError::StalePathMoveUndo(_))));
        assert!(!before.exists());
        assert!(!after.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_preserves_symlink_entries_without_dereferencing() {
        use std::os::unix::fs::{MetadataExt, symlink};

        let dir = TestDir::new("rename-undo-symlink");
        let target = dir.path().join("target.txt");
        let before = dir.path().join("before-link");
        let after = dir.path().join("after-link");
        std::fs::write(&target, "target bytes").unwrap();
        symlink("target.txt", &before).unwrap();
        let before_metadata = std::fs::symlink_metadata(&before).unwrap();

        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after-link")
            .unwrap()
            .expect("rename receipt");
        assert_eq!(
            std::fs::read_link(&after).unwrap(),
            PathBuf::from("target.txt")
        );

        DefaultPlatform.undo_rename_with_receipt(&receipt).unwrap();

        let restored_metadata = std::fs::symlink_metadata(&before).unwrap();
        assert_eq!(
            std::fs::read_link(&before).unwrap(),
            PathBuf::from("target.txt")
        );
        assert_eq!(restored_metadata.dev(), before_metadata.dev());
        assert_eq!(restored_metadata.ino(), before_metadata.ino());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rename_undo_rejects_a_cross_parent_receipt_before_mutation() {
        let dir = TestDir::new("rename-undo-cross-parent");
        let other_parent = dir.path().join("other");
        std::fs::create_dir(&other_parent).unwrap();
        let before = dir.path().join("before.txt");
        let after = dir.path().join("after.txt");
        std::fs::write(&before, "original bytes").unwrap();
        let receipt = DefaultPlatform
            .execute_rename_with_undo_receipt(&before, "after.txt")
            .unwrap()
            .expect("rename receipt");
        let malformed = PathMoveUndoReceipt {
            before_path: other_parent.join("before.txt"),
            ..receipt
        };

        let result = DefaultPlatform.undo_rename_with_receipt(&malformed);

        assert!(matches!(
            result,
            Err(PlatformError::InvalidPathMoveUndoReceipt(_))
        ));
        assert!(!before.exists());
        assert_eq!(std::fs::read_to_string(&after).unwrap(), "original bytes");
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_self_copy_is_rejected_without_changing_the_entry() {
        let dir = TestDir::new("copy-hardlink-self");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        let source = source_dir.join("item.txt");
        let destination = destination_dir.join("item.txt");
        std::fs::write(&source, "original").unwrap();
        std::fs::hard_link(&source, &destination).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source],
            destination: destination_dir,
            conflict: ConflictAction::Overwrite,
        });

        assert!(matches!(result, Err(PlatformError::InvalidDestination(_))));
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "original");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn cross_filesystem_copy_preserves_directory_mode_and_source_across_conflicts() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let (source_root, destination_root) = match cross_filesystem_test_dirs("cross-device") {
            Ok(Some(dirs)) => dirs,
            Ok(None) => {
                eprintln!(
                    "skipping cross-filesystem Copy test: HOME and current directory share /tmp's device"
                );
                return;
            }
            Err(reason) => panic!("cannot prepare cross-filesystem Copy test: {reason}"),
        };
        assert_ne!(
            std::fs::metadata(source_root.path()).unwrap().dev(),
            std::fs::metadata(destination_root.path()).unwrap().dev(),
            "fixture must use distinct filesystem devices"
        );
        let source = source_root.path().join("project");
        let destination = destination_root.path().join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o750)).unwrap();
        std::fs::write(source.join("notes.txt"), "source contents").unwrap();
        std::fs::create_dir(&destination).unwrap();

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            },
        ));
        let copied = destination.join("project");
        assert_eq!(
            std::fs::read_to_string(copied.join("notes.txt")).unwrap(),
            "source contents"
        );
        assert_eq!(
            std::fs::metadata(&copied).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "source contents"
        );

        std::fs::write(copied.join("notes.txt"), "destination contents").unwrap();
        assert!(
            DefaultPlatform
                .execute_command(&FileCommand::Copy {
                    sources: vec![source.clone()],
                    destination: destination.clone(),
                    conflict: ConflictAction::Skip,
                })
                .is_ok()
        );
        assert_eq!(
            std::fs::read_to_string(copied.join("notes.txt")).unwrap(),
            "destination contents"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "source contents"
        );

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Rename,
            },
        ));
        let renamed = destination.join("project (1)");
        assert_eq!(
            std::fs::read_to_string(renamed.join("notes.txt")).unwrap(),
            "source contents"
        );
        assert_eq!(
            std::fs::metadata(renamed).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "source contents"
        );

        let transaction = match DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Overwrite,
        }) {
            Err(PlatformError::CommittedCleanupPending { transaction, .. }) => {
                PathBuf::from(transaction)
            }
            other => {
                panic!("expected published Copy with retained overwrite transaction, got {other:?}")
            }
        };
        assert_eq!(
            std::fs::read_to_string(copied.join("notes.txt")).unwrap(),
            "source contents"
        );
        assert_eq!(
            std::fs::metadata(&copied).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            std::fs::read_to_string(transaction.join("candidate/notes.txt")).unwrap(),
            "destination contents"
        );
        assert_eq!(
            std::fs::read_to_string(source.join("notes.txt")).unwrap(),
            "source contents"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copying_file_symlink_preserves_exact_link_target() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("copy-file-symlink");
        let source_dir = dir.path().join("source");
        let destination_dir = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination_dir).unwrap();
        std::fs::write(source_dir.join("target.txt"), "contents").unwrap();
        let source_link = source_dir.join("link.txt");
        symlink("target.txt", &source_link).unwrap();

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source_link],
                destination: destination_dir.clone(),
                conflict: ConflictAction::Skip,
            },
        ));

        let copied_link = destination_dir.join("link.txt");
        assert!(
            std::fs::symlink_metadata(&copied_link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(copied_link).unwrap(),
            Path::new("target.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn recursive_copy_preserves_tree_symlinks_without_traversing_them() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("copy-tree-symlinks");
        let source = dir.path().join("tree");
        let destination = dir.path().join("destination");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(source.join("file.txt"), "file").unwrap();
        std::fs::create_dir(source.join("real-dir")).unwrap();
        std::fs::write(source.join("real-dir/nested.txt"), "nested").unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        symlink("file.txt", source.join("file-link")).unwrap();
        symlink("real-dir", source.join("dir-link")).unwrap();
        symlink("missing", source.join("dangling-link")).unwrap();
        symlink(&outside, source.join("outside-link")).unwrap();
        symlink(".", source.join("cycle-link")).unwrap();

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            },
        ));

        let copied = destination.join("tree");
        assert_eq!(
            std::fs::read_to_string(copied.join("file.txt")).unwrap(),
            "file"
        );
        assert_eq!(
            std::fs::read_to_string(copied.join("real-dir/nested.txt")).unwrap(),
            "nested"
        );
        for (name, target) in [
            ("file-link", PathBuf::from("file.txt")),
            ("dir-link", PathBuf::from("real-dir")),
            ("dangling-link", PathBuf::from("missing")),
            ("outside-link", outside),
            ("cycle-link", PathBuf::from(".")),
        ] {
            let link = copied.join(name);
            assert!(
                std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(std::fs::read_link(link).unwrap(), target);
        }
    }

    #[test]
    fn directory_copy_into_self_or_descendant_is_rejected_before_creation() {
        let dir = TestDir::new("copy-into-self");
        let source = dir.path().join("tree");
        let descendant = source.join("nested");
        std::fs::create_dir_all(&descendant).unwrap();
        std::fs::write(source.join("file.txt"), "original").unwrap();

        let descendant_target = descendant.join("tree");
        let descendant_result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source.clone()],
            destination: descendant,
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(
            descendant_result,
            Err(PlatformError::InvalidDestination(_))
        ));
        assert!(std::fs::symlink_metadata(&descendant_target).is_err());

        let self_result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source.clone()],
            destination: dir.path().to_path_buf(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(
            self_result,
            Err(PlatformError::InvalidDestination(_))
        ));
        assert_eq!(
            std::fs::read_to_string(source.join("file.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn invalid_later_copy_source_is_rejected_before_any_source_is_copied() {
        let dir = TestDir::new("copy-preflight");
        let first_source = dir.path().join("first.txt");
        let tree = dir.path().join("tree");
        let destination = tree.join("destination");
        std::fs::write(&first_source, "first").unwrap();
        std::fs::create_dir_all(&destination).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![first_source, tree],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(result, Err(PlatformError::InvalidDestination(_))));
        assert!(std::fs::symlink_metadata(destination.join("first.txt")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_special_file_is_rejected_without_creating_destination() {
        use std::os::unix::net::UnixListener;

        let dir = TestDir::new("copy-special-file");
        let source = dir.path().join("source.sock");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let _listener = UnixListener::bind(&source).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(result, Err(PlatformError::InvalidSource(_))));
        assert!(std::fs::symlink_metadata(destination.join("source.sock")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn nested_special_file_is_rejected_before_destination_tree_is_created() {
        use std::os::unix::net::UnixListener;

        let dir = TestDir::new("copy-tree-special-file");
        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("first.txt"), "first").unwrap();
        let _listener = UnixListener::bind(source.join("later.sock")).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![source],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(result, Err(PlatformError::InvalidSource(_))));
        assert!(std::fs::symlink_metadata(destination.join("source")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn renamed_copy_target_skips_dangling_symlink_candidate() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("copy-renamed-dangling-candidate");
        let source_dir = dir.path().join("source");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let source = source_dir.join("item.txt");
        std::fs::write(&source, "source").unwrap();
        std::fs::write(destination.join("item.txt"), "existing").unwrap();
        symlink("redirected.txt", destination.join("item (1).txt")).unwrap();

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source],
                destination: destination.clone(),
                conflict: ConflictAction::Rename,
            },
        ));

        assert_eq!(
            std::fs::read_link(destination.join("item (1).txt")).unwrap(),
            Path::new("redirected.txt")
        );
        assert!(std::fs::symlink_metadata(destination.join("redirected.txt")).is_err());
        assert_eq!(
            std::fs::read_to_string(destination.join("item (2).txt")).unwrap(),
            "source"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_accepted_as_copy_source() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("copy-dangling-source");
        let source = dir.path().join("dangling");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        symlink("missing-target", &source).unwrap();

        assert_copy_completed_with_retained_stage(DefaultPlatform.execute_command(
            &FileCommand::Copy {
                sources: vec![source],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            },
        ));

        let copied = destination.join("dangling");
        assert!(
            std::fs::symlink_metadata(&copied)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(copied).unwrap(),
            Path::new("missing-target")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn copy_batch_publishes_every_item_before_reporting_retained_stages() {
        let dir = TestDir::new("copy-retained-stages");
        let source_dir = dir.path().join("source");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&source_dir).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let first = source_dir.join("first");
        let second = source_dir.join("second");
        std::fs::write(&first, "one").unwrap();
        std::fs::write(&second, "two").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Copy {
            sources: vec![first, second],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(
            result,
            Err(PlatformError::CommittedCleanupPending { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(destination.join("first")).unwrap(),
            "one"
        );
        assert_eq!(
            std::fs::read_to_string(destination.join("second")).unwrap(),
            "two"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn partial_copy_batch_reports_every_retained_recovery_path() {
        let dir = TestDir::new("partial-copy-recovery-paths");
        let first_stage = dir.path().join("first-stage");
        let second_stage = dir.path().join("second-stage");
        std::fs::create_dir(&first_stage).unwrap();
        std::fs::create_dir(&second_stage).unwrap();
        std::fs::write(first_stage.join("candidate"), "displaced-original").unwrap();
        let sources = vec![PathBuf::from("first"), PathBuf::from("second")];

        let result = execute_copy_batch_with(&sources, |source| {
            if source == Path::new("first") {
                Ok(Some(PathMoveUndoReceipt {
                    before_path: first_stage.join("candidate"),
                    after_path: PathBuf::from("first-published"),
                    identity: rename_identity(&first_stage.join("candidate")).unwrap(),
                    displaced_identity: None,
                    retained_displaced_path: None,
                    requirement: PathMoveUndoRequirement::None,
                    published_content: None,
                    retained_displaced_content: None,
                    cross_filesystem_state: Default::default(),
                    retained_copy_directory: None,
                }))
            } else {
                Err(PlatformError::CleanupPending {
                    transaction: second_stage.display().to_string(),
                    message: "injected staging failure".into(),
                })
            }
        });

        assert_eq!(result.undo_receipts.len(), 1);
        match result.error {
            Some(PlatformError::PartialBatch {
                completed,
                recovery_paths,
                message,
                ..
            }) => {
                assert_eq!(completed, 1);
                assert_eq!(recovery_paths, vec![first_stage.clone(), second_stage]);
                assert!(message.contains(&first_stage.display().to_string()));
                assert_eq!(
                    std::fs::read_to_string(first_stage.join("candidate")).unwrap(),
                    "displaced-original"
                );
            }
            other => panic!("expected typed partial batch, got {other:?}"),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn move_is_rejected_before_mutating_even_for_a_valid_file_source() {
        let dir = TestDir::new("move-safe-rejection-file");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "original contents").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(result, Err(PlatformError::UnsupportedSafeMove(_))));
        assert_eq!(
            std::fs::read_to_string(&source).unwrap(),
            "original contents"
        );
        assert!(std::fs::symlink_metadata(destination.join("source.txt")).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_same_volume_overwrite_retains_exact_displaced_entries() {
        for directory in [false, true] {
            let dir = TestDir::new("move-windows-retained-overwrite");
            let source = dir.path().join("entry");
            let destination = dir.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            let target = destination.join("entry");
            let payload = |path: &Path| {
                if directory {
                    path.join("payload")
                } else {
                    path.to_path_buf()
                }
            };
            if directory {
                std::fs::create_dir(&source).unwrap();
                std::fs::create_dir(&target).unwrap();
            }
            std::fs::write(payload(&source), "source bytes").unwrap();
            std::fs::write(payload(&target), "displaced bytes").unwrap();
            let source_identity = move_recovery::stable_identity_of_entry(&source).unwrap();
            let displaced_identity = move_recovery::stable_identity_of_entry(&target).unwrap();
            let publication = move_transaction::move_entry_transactional(
                &source,
                &destination,
                ConflictAction::Overwrite,
            )
            .unwrap_or_else(|error| panic!("directory={directory}: {error:?}"));
            assert_eq!(publication.disposition, MoveDisposition::Moved);
            assert_eq!(
                publication.published_path.as_deref(),
                Some(target.as_path())
            );
            assert!(!source.exists());
            assert_eq!(
                move_recovery::stable_identity_of_entry(&target).unwrap(),
                source_identity
            );
            assert_eq!(std::fs::read(payload(&target)).unwrap(), b"source bytes");
            let retained = publication
                .retained_displaced_path
                .expect("Overwrite retains displaced data");
            assert_eq!(
                move_recovery::stable_identity_of_entry(&retained).unwrap(),
                displaced_identity
            );
            assert_eq!(
                std::fs::read(payload(&retained)).unwrap(),
                b"displaced bytes"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_same_volume_file_move_publishes_the_exact_source() {
        let dir = TestDir::new("move-windows-file");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "original contents").unwrap();

        let (_, dispositions) = DefaultPlatform
            .execute_command_with_move_dispositions(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            })
            .unwrap();

        assert_eq!(dispositions, vec![MoveDisposition::Moved]);
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(
            std::fs::read_to_string(destination.join("source.txt")).unwrap(),
            "original contents"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_same_filesystem_move_uses_native_rename_without_recovery_artifacts() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("move-native-rename-file");
        let source = dir.path().join("source.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let source_file = std::fs::File::create(&source).unwrap();
        source_file.set_len(64 * 1024 * 1024).unwrap();
        let source_identity = std::fs::metadata(&source).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(result.is_ok(), "Move failed: {result:?}");
        let moved = destination.join("source.txt");
        let moved_identity = std::fs::metadata(&moved).unwrap();
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(moved_identity.len(), 64 * 1024 * 1024);
        assert_eq!(moved_identity.dev(), source_identity.dev());
        assert_eq!(moved_identity.ino(), source_identity.ino());
        assert!(!dir.path().join(".ottrin-move-recovery-root").exists());
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".ottrin-move-")
        }));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_same_filesystem_move_conflicts_are_explicit_and_non_copying() {
        for (conflict, expected_name) in [
            (ConflictAction::Skip, None),
            (ConflictAction::Rename, Some("source (1).txt")),
            (ConflictAction::Overwrite, Some("source.txt")),
        ] {
            let dir = TestDir::new("move-native-rename-conflicts");
            let source = dir.path().join("source.txt");
            let destination = dir.path().join("destination");
            let occupied = destination.join("source.txt");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(&source, "source contents").unwrap();
            std::fs::write(&occupied, "destination contents").unwrap();

            let result = DefaultPlatform.execute_command(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict,
            });

            assert!(result.is_ok(), "{conflict:?} Move failed: {result:?}");
            assert_eq!(
                std::fs::read_to_string(&occupied).unwrap(),
                if conflict == ConflictAction::Overwrite {
                    "source contents"
                } else {
                    "destination contents"
                }
            );
            match expected_name {
                Some(name) => {
                    assert!(!source.exists());
                    assert_eq!(
                        std::fs::read_to_string(destination.join(name)).unwrap(),
                        "source contents"
                    );
                }
                None => assert_eq!(std::fs::read_to_string(&source).unwrap(), "source contents"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cross_filesystem_regular_file_move_supports_skip_and_rename() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("move-cross-device-regular-file");
        let recovery_root = dir.path().join("state").join("ottrin").join("moves");
        let _recovery_root = TestMoveRecoveryRoot::new(recovery_root);
        let destination_root = std::path::Path::new("/dev/shm");
        if std::fs::metadata(dir.path()).unwrap().dev()
            == std::fs::metadata(destination_root).unwrap().dev()
        {
            return;
        }
        let destination = destination_root.join(format!(
            "ottrin-move-cross-device-{}-{}",
            std::process::id(),
            getrandom::u64().unwrap()
        ));
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source.txt"), "occupied").unwrap();

        for (conflict, expected_name) in [
            (ConflictAction::Skip, None),
            (ConflictAction::Rename, Some("source (1).txt")),
        ] {
            let source = dir.path().join("source.txt");
            std::fs::write(&source, "source contents").unwrap();

            let (_, dispositions) = DefaultPlatform
                .execute_command_with_move_dispositions(&FileCommand::Move {
                    sources: vec![source.clone()],
                    destination: destination.clone(),
                    conflict,
                })
                .unwrap();

            match expected_name {
                None => {
                    assert_eq!(dispositions, vec![MoveDisposition::Skipped]);
                    assert_eq!(std::fs::read_to_string(&source).unwrap(), "source contents");
                }
                Some(name) => {
                    assert_eq!(dispositions, vec![MoveDisposition::Moved]);
                    assert!(!source.exists());
                    assert_eq!(
                        std::fs::read_to_string(destination.join(name)).unwrap(),
                        "source contents"
                    );
                }
            }
            assert_eq!(
                std::fs::read_to_string(destination.join("source.txt")).unwrap(),
                "occupied"
            );
        }

        std::fs::remove_dir_all(&destination).unwrap();
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn move_is_rejected_before_mutating_even_for_a_valid_directory_source() {
        let dir = TestDir::new("move-safe-rejection-directory");
        let source = dir.path().join("source-dir");
        let source_file = source.join("inner.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(&source_file, "keep-safe").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(result, Err(PlatformError::UnsupportedSafeMove(_))));
        assert!(source.exists());
        assert!(source_file.exists());
        assert!(std::fs::read_to_string(source_file).unwrap() == "keep-safe");
        assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_same_volume_directory_move_preserves_nested_content() {
        let dir = TestDir::new("move-windows-directory");
        let source = dir.path().join("source-dir");
        let source_file = source.join("inner.txt");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(&source_file, "keep-safe").unwrap();
        std::fs::create_dir(&destination).unwrap();

        let (_, dispositions) = DefaultPlatform
            .execute_command_with_move_dispositions(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            })
            .unwrap();

        assert_eq!(dispositions, vec![MoveDisposition::Moved]);
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(
            std::fs::read_to_string(destination.join("source-dir/inner.txt")).unwrap(),
            "keep-safe"
        );
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn move_is_rejected_before_mutating_for_all_conflict_actions() {
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Overwrite,
            ConflictAction::Rename,
        ] {
            let dir = TestDir::new("move-safe-rejection-conflict-matrix");
            let source = dir.path().join("source.txt");
            let destination = dir.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(&source, "original contents").unwrap();

            let result = DefaultPlatform.execute_command(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict,
            });

            assert!(matches!(result, Err(PlatformError::UnsupportedSafeMove(_))));
            assert_eq!(
                std::fs::read_to_string(&source).unwrap(),
                "original contents"
            );
            assert!(std::fs::symlink_metadata(destination.join("source.txt")).is_err());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_same_volume_unoccupied_move_supports_every_conflict_action() {
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Overwrite,
            ConflictAction::Rename,
        ] {
            let dir = TestDir::new("move-windows-unoccupied-conflict-matrix");
            let source = dir.path().join("source.txt");
            let destination = dir.path().join("destination");
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(&source, "original contents").unwrap();

            let (_, dispositions) = DefaultPlatform
                .execute_command_with_move_dispositions(&FileCommand::Move {
                    sources: vec![source.clone()],
                    destination: destination.clone(),
                    conflict,
                })
                .unwrap();

            assert_eq!(dispositions, vec![MoveDisposition::Moved]);
            assert!(std::fs::symlink_metadata(&source).is_err());
            assert_eq!(
                std::fs::read_to_string(destination.join("source.txt")).unwrap(),
                "original contents"
            );
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn move_directory_is_rejected_before_mutating_for_all_conflict_actions() {
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Overwrite,
            ConflictAction::Rename,
        ] {
            let dir = TestDir::new("move-safe-dir-conflict-matrix");
            let source = dir.path().join("source-dir");
            let source_file = source.join("inner.txt");
            let destination = dir.path().join("destination");
            let destination_conflict = destination.join("source-dir");

            std::fs::create_dir(&source).unwrap();
            std::fs::write(&source_file, "nested").unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&destination_conflict).unwrap();
            std::fs::write(
                destination_conflict.join("retained.txt"),
                "destination retained",
            )
            .unwrap();

            let result = DefaultPlatform.execute_command(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict,
            });

            assert!(matches!(result, Err(PlatformError::UnsupportedSafeMove(_))));
            assert!(source.exists());
            assert!(source_file.exists());
            assert_eq!(std::fs::read_to_string(&source_file).unwrap(), "nested");
            assert!(destination_conflict.exists());
            assert!(
                std::fs::read_to_string(destination_conflict.join("retained.txt")).unwrap()
                    == "destination retained"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_same_volume_directory_conflicts_preserve_data() {
        for conflict in [
            ConflictAction::Skip,
            ConflictAction::Overwrite,
            ConflictAction::Rename,
        ] {
            let dir = TestDir::new("move-windows-dir-conflict-matrix");
            let source = dir.path().join("source-dir");
            let source_file = source.join("inner.txt");
            let destination = dir.path().join("destination");
            let destination_conflict = destination.join("source-dir");

            std::fs::create_dir(&source).unwrap();
            std::fs::write(&source_file, "nested").unwrap();
            std::fs::create_dir(&destination).unwrap();
            std::fs::create_dir(&destination_conflict).unwrap();
            std::fs::write(
                destination_conflict.join("retained.txt"),
                "destination retained",
            )
            .unwrap();

            let result =
                DefaultPlatform.execute_command_with_move_dispositions(&FileCommand::Move {
                    sources: vec![source.clone()],
                    destination: destination.clone(),
                    conflict,
                });

            match conflict {
                ConflictAction::Skip => {
                    let (_, dispositions) = result.unwrap();
                    assert_eq!(dispositions, vec![MoveDisposition::Skipped]);
                    assert_eq!(std::fs::read_to_string(&source_file).unwrap(), "nested");
                }
                ConflictAction::Rename => {
                    let (_, dispositions) = result.unwrap();
                    assert_eq!(dispositions, vec![MoveDisposition::Moved]);
                    assert!(std::fs::symlink_metadata(&source).is_err());
                    assert_eq!(
                        std::fs::read_to_string(destination.join("source-dir (1)/inner.txt"))
                            .unwrap(),
                        "nested"
                    );
                }
                ConflictAction::Overwrite => {
                    let (_, dispositions) = result.unwrap();
                    assert_eq!(dispositions, vec![MoveDisposition::Moved]);
                    assert!(!source.exists());
                    assert_eq!(
                        std::fs::read_to_string(destination_conflict.join("inner.txt")).unwrap(),
                        "nested"
                    );
                }
            }
            if conflict != ConflictAction::Overwrite {
                assert_eq!(
                    std::fs::read_to_string(destination_conflict.join("retained.txt")).unwrap(),
                    "destination retained"
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_same_volume_move_preserves_a_dangling_symlink() {
        use std::os::windows::fs::symlink_file as symlink;

        let dir = TestDir::new("move-safe-rejection");
        let source = dir.path().join("dangling");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        symlink("missing-target", &source).unwrap();

        let (_, dispositions) = DefaultPlatform
            .execute_command_with_move_dispositions(&FileCommand::Move {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Skip,
            })
            .unwrap();

        assert_eq!(dispositions, vec![MoveDisposition::Moved]);
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(
            std::fs::read_link(destination.join("dangling")).unwrap(),
            Path::new("missing-target")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_same_filesystem_move_preserves_a_dangling_symlink() {
        use std::os::unix::fs::symlink;

        let dir = TestDir::new("move-safe-rejection");
        let source = dir.path().join("dangling");
        let destination = dir.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        symlink("missing-target", &source).unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(result.is_ok(), "Move failed: {result:?}");
        assert!(std::fs::symlink_metadata(&source).is_err());
        assert_eq!(
            std::fs::read_link(destination.join("dangling")).unwrap(),
            Path::new("missing-target")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn retained_move_status_facade_never_initializes_an_unanchored_root() {
        let dir = TestDir::new("move-retained-status");
        let missing_root = dir.path().join("missing");

        assert!(
            discover_retained_move_transactions_at(&missing_root)
                .unwrap()
                .is_empty()
        );
        assert!(!missing_root.exists());

        let unanchored_root = dir.path().join("unanchored");
        std::fs::create_dir(&unanchored_root).unwrap();
        let result = discover_retained_move_transactions_at(&unanchored_root);

        assert!(matches!(result, Err(PlatformError::Io(_))));
        assert!(!unanchored_root.join(".ottrin-move-recovery-root").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn default_retained_move_status_uses_isolated_state_home_without_initializing_it() {
        let _lock = move_transaction::TEST_MOVE_ENV_SERIALIZER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = TestDir::new("default-move-retained-status");
        let state_home = dir.path().join("state-home");
        let previous = std::env::var_os("XDG_STATE_HOME");
        // Environment changes are process-global. This test serializes its own mutation and the
        // focused command is run with one test thread so the configured root stays isolated.
        unsafe { std::env::set_var("XDG_STATE_HOME", &state_home) };

        let result = discover_retained_move_transactions();

        match previous {
            Some(value) => unsafe { std::env::set_var("XDG_STATE_HOME", value) },
            None => unsafe { std::env::remove_var("XDG_STATE_HOME") },
        }
        assert!(result.unwrap().is_empty());
        assert!(!state_home.exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recover_retained_move_unknown_transaction_is_rejected() {
        let mut transaction_id = [0_u8; 16];
        getrandom::fill(&mut transaction_id)
            .expect("random source available for deterministic test data");

        let result =
            DefaultPlatform.execute_command(&FileCommand::RecoverRetainedMove { transaction_id });

        assert!(matches!(result, Err(PlatformError::Io(_))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recover_retained_move_known_pending_destination_replays_claimed_source() {
        let dir = TestDir::new("move-retained-recover-known");
        let state_home = dir.path().join("state");
        let recovery_root = state_home.join("ottrin").join("moves");
        let _recovery_root = TestMoveRecoveryRoot::new(recovery_root.clone());

        let source = dir.path().join("source");
        let destination = dir.path().join("destination");
        std::fs::write(&source, b"payload").unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("source"), b"occupied").unwrap();

        let result = move_transaction::move_entry_transactional_with_test_hook(
            &source,
            &destination,
            ConflictAction::Rename,
            |point| {
                if point == move_transaction::TestPoint::PendingDestination {
                    Err("force pending destination")
                } else {
                    Ok(())
                }
            },
        );
        assert!(
            matches!(result, Err(PlatformError::Io(message)) if message == "move test hook failed: force pending destination")
        );

        let retained = discover_retained_move_transactions_at(&recovery_root).unwrap();
        let pending = retained
            .iter()
            .find(|entry| entry.phase == move_recovery::JournalPhase::PendingDestination)
            .copied()
            .unwrap_or_else(|| panic!("expected retained pending destination transaction"));
        let recovery_result = move_transaction::recover_retained_move_pending_destination_at(
            &recovery_root,
            pending.transaction_id,
        );
        assert!(
            recovery_result.is_ok(),
            "recovery failed: {recovery_result:?}"
        );

        assert_eq!(std::fs::read(&source).unwrap(), b"payload");
        assert_eq!(
            std::fs::read(destination.join("source")).unwrap(),
            b"occupied"
        );

        let transaction_id_hex: String = pending
            .transaction_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let local_path = dir
            .path()
            .join(format!(".ottrin-move-{transaction_id_hex}"));
        let central_journal = state_home
            .join("ottrin")
            .join("moves")
            .join(format!("{transaction_id_hex}.journal"));
        assert!(!std::fs::exists(&local_path).unwrap_or_default());
        assert!(!std::fs::exists(&central_journal).unwrap_or_default());
    }
}
