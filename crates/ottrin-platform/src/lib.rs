use ottrin_core::{ConflictAction, DeleteMode, EntryKind, FileCommand};
use std::path::{Path, PathBuf};
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

pub trait PlatformOps: Send + Sync {
    fn delete_path(&self, target: &Path, mode: DeleteMode) -> Result<(), PlatformError>;
    fn reveal_in_system(&self, target: &Path) -> Result<(), PlatformError>;
    fn execute_command(&self, command: &FileCommand) -> Result<Option<FileProperties>, PlatformError>;
}

#[derive(Debug, Default)]
pub struct DefaultPlatform;

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
                chmod_path(target, mode_str)
                    .map_err(|e| PlatformError::Io(e))?;
                Ok(None)
            }
            FileCommand::Symlink { link_path, target } => {
                create_symlink(link_path, target)
                    .map_err(|e| PlatformError::Io(e))?;
                Ok(None)
            }
        }
    }
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
        copy_dir_recursive(source, &destination)
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

    match std::fs::rename(source, &destination) {
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
