use ottrin_core::{
    ConflictAction, DeleteMode, EntryKind, FileCommand, PrivilegedRequest, PrivilegedResponse,
};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct FileProperties {
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size_bytes: u64,
    pub readonly: bool,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedAvailability {
    Ready,
    Misconfigured(String),
    Unsupported(String),
}

pub trait PlatformOps: Send + Sync {
    fn delete_path(&self, target: &Path, mode: DeleteMode) -> Result<(), PlatformError>;
    fn reveal_in_system(&self, target: &Path) -> Result<(), PlatformError>;
    fn execute_command(&self, command: &FileCommand) -> Result<Option<FileProperties>, PlatformError>;
    fn execute_privileged(&self, request: &PrivilegedRequest) -> Result<PrivilegedResponse, PlatformError>;
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

    fn execute_command(&self, command: &FileCommand) -> Result<Option<FileProperties>, PlatformError> {
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
                std::fs::rename(source, target).map_err(|e| PlatformError::Io(e.to_string()))?;
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
                    if let Some(target) = resolve_destination(source, destination, *conflict)? {
                        copy_entry(source, &target)?;
                    }
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
                    if let Some(target) = resolve_destination(source, destination, *conflict)? {
                        move_entry(source, &target)?;
                    }
                }
                Ok(None)
            }
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

    fn execute_privileged(&self, request: &PrivilegedRequest) -> Result<PrivilegedResponse, PlatformError> {
        #[cfg(target_os = "linux")]
        {
            execute_privileged_linux(request)
        }
        #[cfg(target_os = "windows")]
        {
            return execute_privileged_windows(request);
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = request;
            Ok(PrivilegedResponse {
                status: ottrin_core::PrivilegedStatus::Unsupported,
                message: Some("Privileged helper integration is not implemented on this OS yet.".to_string()),
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
            if !is_executable_available("powershell.exe") && !is_executable_available("powershell") {
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
            return PrivilegedAvailability::Ready;
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            PrivilegedAvailability::Unsupported(
                "Integrated privilege management is not implemented on this platform.".to_string(),
            )
        }
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
fn execute_privileged_linux(request: &PrivilegedRequest) -> Result<PrivilegedResponse, PlatformError> {
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
        return Err(PlatformError::Io("privileged helper returned no data".to_string()));
    }

    serde_json::from_slice::<PrivilegedResponse>(&out.stdout)
        .map_err(|e| PlatformError::Io(format!("decode response failed: {}", e)))
}

#[cfg(target_os = "windows")]
fn execute_privileged_windows(request: &PrivilegedRequest) -> Result<PrivilegedResponse, PlatformError> {
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
        Err(PlatformError::InvalidDestination(path.display().to_string()))
    }
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

fn resolve_destination(
    source: &Path,
    destination_dir: &Path,
    conflict: ConflictAction,
) -> Result<Option<PathBuf>, PlatformError> {
    if !source.exists() {
        return Err(PlatformError::InvalidSource(source.display().to_string()));
    }

    let name = source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    let destination = destination_dir.join(name);

    if !destination.exists() {
        return Ok(Some(destination));
    }

    match conflict {
        ConflictAction::Skip => Ok(None),
        ConflictAction::Overwrite => {
            delete_permanent(&destination)?;
            Ok(Some(destination))
        }
        ConflictAction::Rename => Ok(Some(next_available_name(&destination))),
    }
}

fn next_available_name(base: &Path) -> PathBuf {
    let parent = base.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = base
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("item")
        .to_string();
    let ext = base.extension().and_then(|e| e.to_str()).map(str::to_string);

    for i in 1.. {
        let candidate_name = if let Some(ext) = &ext {
            format!("{stem} ({i}).{ext}")
        } else {
            format!("{stem} ({i})")
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    base.to_path_buf()
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    if !source.exists() {
        return Err(PlatformError::InvalidSource(source.display().to_string()));
    }

    let meta = std::fs::symlink_metadata(source).map_err(|e| PlatformError::Io(e.to_string()))?;
    if meta.is_dir() {
        copy_dir_recursive(source, destination)
    } else {
        std::fs::copy(source, destination)
            .map(|_| ())
            .map_err(|e| PlatformError::Io(e.to_string()))
    }
}

fn copy_dir_recursive(source_dir: &Path, target_dir: &Path) -> Result<(), PlatformError> {
    std::fs::create_dir_all(target_dir).map_err(|e| PlatformError::Io(e.to_string()))?;

    let read_dir = std::fs::read_dir(source_dir).map_err(|e| PlatformError::Io(e.to_string()))?;
    for child in read_dir {
        let child = child.map_err(|e| PlatformError::Io(e.to_string()))?;
        let source = child.path();
        let target = target_dir.join(child.file_name());

        let meta = child
            .metadata()
            .map_err(|e| PlatformError::Io(e.to_string()))?;
        if meta.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else {
            std::fs::copy(&source, &target).map_err(|e| PlatformError::Io(e.to_string()))?;
        }
    }

    Ok(())
}

fn move_entry(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    if !source.exists() {
        return Err(PlatformError::InvalidSource(source.display().to_string()));
    }

    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_entry(source, destination)?;
            delete_permanent(source)?;
            Ok(())
        }
    }
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
