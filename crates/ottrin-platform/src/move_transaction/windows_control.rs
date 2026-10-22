//! Read retained controls by their recorded native directory identity. Historical
//! path strings remain labels; they never select a different private directory.
use super::*;
use crate::windows_fs::{self, RelativeOptions};
use std::{ffi::OsStr, fs::File, io::Read, ptr};
use windows_sys::Win32::Storage::FileSystem::*;

pub(super) fn directory(
    root: &Path,
    identity: move_recovery::WindowsStableIdentity,
    hint: &Path,
) -> Result<File, PlatformError> {
    directory_with_access(root, identity, hint, FILE_GENERIC_READ)
}

fn directory_with_access(
    root: &Path,
    identity: move_recovery::WindowsStableIdentity,
    hint: &Path,
    access: u32,
) -> Result<File, PlatformError> {
    if identity.is_directory_without_reparse()
        && let Ok(directory) = windows_fs::open_directory(hint, access)
        && move_recovery::stable_identity_from_handle(&directory).map_err(io)? == identity
    {
        return Ok(directory);
    }
    // Atomic Moves need no journal registry. An existing ancestor supplies only
    // a volume-manager anchor, never authority for a payload or private object.
    let anchor = root
        .ancestors()
        .find_map(|path| windows_fs::open_directory(path, FILE_READ_ATTRIBUTES | SYNCHRONIZE).ok())
        .ok_or_else(|| io("Move recovery has no accessible volume anchor"))?;
    windows_fs::Volumes::new(&anchor)?.resolve(identity, access, &mut |_| Ok(()))
}

/// Captured read-only naming evidence; only strict worker-time pinning mutates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::move_transaction) struct DirectoryBinding {
    pub path: PathBuf,
    pub identity: move_recovery::WindowsStableIdentity,
    pub label: PathBuf,
}
impl DirectoryBinding {
    pub fn capture(
        root: &Path,
        hint: &Path,
        identity: move_recovery::WindowsStableIdentity,
    ) -> Result<Self, PlatformError> {
        let held = directory_with_access(
            root,
            identity,
            hint,
            FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
        )?;
        let path = windows_fs::location(&held).map_err(io)?;
        let label = current_path(&held, hint)?;
        Ok(Self {
            path,
            identity,
            label,
        })
    }
    pub fn pin(&self) -> Result<File, PlatformError> {
        let file = windows_fs::open_directory_with_sharing(
            &self.path,
            move_recovery::WINDOWS_DESTINATION_PARENT_ACCESS_MODE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .map_err(io)?;
        if move_recovery::stable_identity_from_handle(&file).map_err(io)? != self.identity {
            return Err(io(
                "Move namespace changed after admission; refresh and retry",
            ));
        }
        Ok(file)
    }
}

fn current_path(held: &File, hint: &Path) -> Result<PathBuf, PlatformError> {
    // Preserve the caller's existing ordinary/verbatim spelling when it still
    // names this exact directory. Only a stale label needs a native final name.
    if hint.is_absolute()
        && let Ok(named) =
            windows_fs::open_directory(hint, FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE)
        && move_recovery::stable_identity_from_handle(&named).map_err(io)?
            == move_recovery::stable_identity_from_handle(held).map_err(io)?
    {
        return std::path::absolute(hint).map_err(io);
    }
    windows_fs::location(held).map_err(io)
}

pub(in crate::move_transaction) fn child_path(
    parent: &File,
    label: &Path,
) -> Result<PathBuf, PlatformError> {
    let name = label
        .file_name()
        .ok_or_else(|| io("Move entry has no native leaf name"))?;
    windows_fs::valid_component(name).map_err(io)?;
    Ok(windows_fs::location(parent).map_err(io)?.join(name))
}

/// A presentation label never selects the object or supplies queue authority.
pub(in crate::move_transaction) fn label_path(
    parent: &File,
    label: &Path,
) -> Result<PathBuf, PlatformError> {
    let name = label
        .file_name()
        .ok_or_else(|| io("Move label has no leaf"))?;
    windows_fs::valid_component(name).map_err(io)?;
    let hint = label
        .parent()
        .ok_or_else(|| io("Move label has no parent"))?;
    Ok(current_path(parent, hint)?.join(name))
}

pub(super) fn pin_original(
    path: &Path,
    identity: move_recovery::WindowsStableIdentity,
) -> Result<File, PlatformError> {
    let directory = windows_fs::open_directory_with_sharing(
        path,
        FILE_GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .map_err(io)?;
    if move_recovery::stable_identity_from_handle(&directory).map_err(io)? != identity {
        return Err(io("Move journal directory changed; no journal was written"));
    }
    Ok(directory)
}

fn io(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(error.to_string())
}

pub(super) fn read_local(directory: &File) -> Result<Option<Vec<u8>>, PlatformError> {
    let mut file = match unsafe {
        windows_fs::open_relative(
            directory,
            OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME),
            RelativeOptions {
                directory: Some(false),
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    } {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error)),
    };
    if !move_recovery::stable_identity_from_handle(&file)
        .map_err(io)?
        .is_regular_file_without_reparse()
    {
        return Err(io("Retained Move local journal is not an ordinary file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(io)?;
    Ok(Some(bytes))
}

pub(super) fn overwrite(
    root: &Path,
    record: &WindowsOverwriteRecord,
) -> Result<WindowsOverwriteRecord, PlatformError> {
    let directory = directory(
        root,
        record.transaction_directory_identity,
        &record.transaction_path()?,
    )?;
    let bytes =
        read_local(&directory)?.ok_or_else(|| io("Retained Move local journal is missing"))?;
    decode_windows_overwrite_record(&bytes)
}
