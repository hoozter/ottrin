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

mod copy_transaction;
use copy_transaction::{CopyPublication, copy_entry_transactional};
pub mod move_recovery;
mod move_transaction;
use move_transaction::move_entry_transactional;

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
/// same-user desktop trust model. The desktop-file path remains opaque, so
/// callers can show the name but cannot turn it into an arbitrary command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWithApplication {
    name: String,
    #[cfg(target_os = "linux")]
    native_id: String,
    #[cfg(target_os = "linux")]
    target: PathBuf,
}

impl OpenWithApplication {
    pub fn name(&self) -> &str {
        &self.name
    }
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
    #[cfg(not(target_os = "linux"))]
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

fn resolve_helper_name(default_name: &str) -> String {
    if let Ok(p) = helper_override_store().read()
        && let Some(path) = p.as_ref()
    {
        return path.display().to_string();
    }
    if let Ok(path) = std::env::var("OTTRIN_PRIV_HELPER") {
        // Invalid env overrides should not break packaged auto-discovery.
        if is_executable_available(&path) {
            return path;
        }
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
                let target = parent.join(name);
                std::fs::create_dir(&target).map_err(|e| PlatformError::Io(e.to_string()))?;
                Ok(None)
            }
            FileCommand::Rename { source, new_name } => {
                let parent = source
                    .parent()
                    .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
                ensure_existing_dir(parent)?;
                let target = parent.join(new_name);
                rename_without_replacement(source, &target)?;
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
                ensure_existing_dir(destination)?;
                for source in sources {
                    validate_copy_source(source)?;
                    reject_directory_copy_into_itself(source, destination)?;
                }
                execute_copy_batch(sources, destination, *conflict)?;
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
                execute_batch(sources, |source| {
                    move_entry_transactional(source, destination, *conflict).map(|_| ())
                })?;
                Ok(None)
            }
            #[cfg(target_os = "linux")]
            FileCommand::RecoverRetainedMove { transaction_id } => {
                move_transaction::recover_retained_move_pending_destination(*transaction_id)?;
                Ok(None)
            }
            #[cfg(not(target_os = "linux"))]
            FileCommand::RecoverRetainedMove { .. } => Err(PlatformError::NotAvailable(
                "Retained Move recovery is supported only on Linux",
            )),
            FileCommand::ShowProperties { target } => {
                let props = read_properties(target)?;
                Ok(Some(props))
            }
            FileCommand::Chmod { target, mode_str } => {
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
                })?;
                Ok((None, completed_dispositions))
            }
            _ => Ok((self.execute_command(command)?, Vec::new())),
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
                    "pkexec not found in PATH".to_string(),
                );
            }
            let helper = resolve_helper_name("ottrin-priv-helper");
            if !is_executable_available(&helper) {
                return PrivilegedAvailability::Misconfigured(format!(
                    "Privileged helper '{}' not found (set OTTRIN_PRIV_HELPER or use --helper-path)",
                    helper
                ));
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
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (target, application);
            Err(PlatformError::NotAvailable("native Open With discovery"))
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_open_with {
    use super::*;
    use gio::prelude::*;
    use std::collections::BTreeMap;

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
    for (index, source) in sources.iter().enumerate() {
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
                let completed = index + usize::from(current_committed);
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

fn execute_copy_batch(
    sources: &[PathBuf],
    destination: &Path,
    conflict: ConflictAction,
) -> Result<(), PlatformError> {
    execute_copy_batch_with(sources, |source| {
        copy_entry_transactional(source, destination, conflict)
    })
}

fn execute_copy_batch_with(
    sources: &[PathBuf],
    mut execute: impl FnMut(&Path) -> Result<CopyPublication, PlatformError>,
) -> Result<(), PlatformError> {
    let total = sources.len();
    let mut retained = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        match execute(source) {
            Ok(CopyPublication::Published {
                retained_transaction,
            }) => retained.push(retained_transaction),
            Ok(CopyPublication::Skipped {
                retained_transaction: Some(transaction),
            }) => retained.push(transaction),
            Ok(CopyPublication::Skipped {
                retained_transaction: None,
            }) => {}
            Err(error) => {
                let current_committed =
                    matches!(error, PlatformError::CommittedCleanupPending { .. });
                let completed = index + usize::from(current_committed);
                if total > 1 && completed > 0 {
                    let mut recovery_paths = retained.clone();
                    recovery_paths.extend(error_recovery_paths(&error));
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
    }
    if retained.is_empty() {
        Ok(())
    } else {
        Err(PlatformError::CommittedCleanupPending {
            transaction: retained
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            message: "copy completed; private transaction stages were retained because pathname-based cleanup cannot prove ownership"
                .into(),
        })
    }
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
    std::fs::rename(source, target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists
            || std::fs::symlink_metadata(target).is_ok()
        {
            PlatformError::DestinationExists(target.display().to_string())
        } else {
            PlatformError::Io(error.to_string())
        }
    })
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

#[cfg(target_os = "linux")]
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

fn chmod_path(path: &Path, mode_str: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        let current = meta.permissions().mode();
        let new_mode = if let Some(stripped) = mode_str.strip_prefix('+') {
            let bits = symbolic_bits(stripped)?;
            current | bits
        } else if let Some(stripped) = mode_str.strip_prefix('-') {
            let bits = symbolic_bits(stripped)?;
            current & !bits
        } else {
            u32::from_str_radix(mode_str, 8).map_err(|_| format!("Invalid mode: {}", mode_str))?
        };
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
    fn move_batch_collects_ordered_dispositions_for_partial_source_failures() {
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
                assert_eq!(completed, 2);
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

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            Self::new_at(&std::env::temp_dir(), name)
        }

        fn new_at(parent: &Path, name: &str) -> Self {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                "ottrin-platform-{name}-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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

        assert!(matches!(result, Err(PlatformError::DestinationExists(_))));
        assert_eq!(std::fs::read_to_string(source).unwrap(), "source contents");
        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            "destination contents"
        );
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
                Ok(CopyPublication::Published {
                    retained_transaction: first_stage.clone(),
                })
            } else {
                Err(PlatformError::CleanupPending {
                    transaction: second_stage.display().to_string(),
                    message: "injected staging failure".into(),
                })
            }
        });

        match result {
            Err(PlatformError::PartialBatch {
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

    #[cfg(not(target_os = "linux"))]
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
    fn linux_cross_filesystem_move_returns_typed_error_without_mutation() {
        use std::os::unix::fs::MetadataExt;

        let dir = TestDir::new("move-cross-device-rejection");
        let source = dir.path().join("source.txt");
        let destination_root = std::path::Path::new("/dev/shm");
        if std::fs::metadata(dir.path()).unwrap().dev()
            == std::fs::metadata(destination_root).unwrap().dev()
        {
            return;
        }
        let destination =
            destination_root.join(format!("ottrin-move-cross-device-{}", std::process::id()));
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(&source, "source contents").unwrap();

        let result = DefaultPlatform.execute_command(&FileCommand::Move {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Skip,
        });

        assert!(matches!(
            result,
            Err(PlatformError::UnsupportedSafeCrossDeviceMove(_))
        ));
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "source contents");
        assert!(!destination.join("source.txt").exists());
        std::fs::remove_dir(&destination).unwrap();
    }

    #[cfg(not(target_os = "linux"))]
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

    #[cfg(not(target_os = "linux"))]
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

    #[cfg(not(target_os = "linux"))]
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

    #[cfg(windows)]
    #[test]
    fn move_is_rejected_before_mutating_even_for_a_valid_symlink_source() {
        use std::os::windows::fs::symlink_file as symlink;

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

        assert!(matches!(result, Err(PlatformError::UnsupportedSafeMove(_))));
        assert_eq!(
            std::fs::read_link(&source).unwrap(),
            Path::new("missing-target")
        );
        assert!(std::fs::symlink_metadata(destination.join("dangling")).is_err());
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
        static STATE_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        let _lock = STATE_HOME_LOCK.lock().unwrap();
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
        move_transaction::set_test_recovery_root(Some(recovery_root.clone()));

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
        move_transaction::set_test_recovery_root(None);
    }
}
