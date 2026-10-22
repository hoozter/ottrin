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
    // The stored name is only an optimization. A foreign or unreadable hint
    // cannot select another directory; native ID resolution remains authoritative.
    if identity.is_directory_without_reparse()
        && let Ok(directory) = windows_fs::open_directory(hint, FILE_GENERIC_READ)
        && let Ok(actual) = move_recovery::stable_identity_from_handle(&directory)
        && actual == identity
    {
        return Ok(directory);
    }
    let anchor =
        windows_fs::open_directory(root, FILE_READ_ATTRIBUTES | SYNCHRONIZE).map_err(io)?;
    windows_fs::Volumes::new(&anchor)?.resolve(identity, FILE_GENERIC_READ, &mut |_| Ok(()))
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

pub(super) fn missing_original(
    parent: &Path,
    identity: move_recovery::WindowsStableIdentity,
    name: &OsStr,
) -> Result<bool, PlatformError> {
    let parent = pin_original(parent, identity)?;
    Ok(windows_undo::observe(&parent, name)?.is_none())
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
