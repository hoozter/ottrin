//! Native ownership and atomic same-volume Move inverse. No public name is replaced.
use crate::{
    PathMoveIdentity, PathMoveUndoReceipt, PathMoveUndoRequirement, PlatformError, WindowsPathUndo,
    move_recovery, windows_fs,
};
use move_recovery::WindowsStableIdentity;
use std::{ffi::OsStr, fs::File, path::Path, ptr};
use windows_fs::RelativeOptions;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WindowsMoveUndoGuard {
    Atomic {
        before_parent: WindowsStableIdentity,
        after_parent: WindowsStableIdentity,
    },
    Replace(Box<super::WindowsOverwriteRecord>),
    CrossVolume(Box<cross::Guard>),
}

fn io(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(error.to_string())
}
fn stale(path: &Path) -> PlatformError {
    PlatformError::StalePathMoveUndo(path.display().to_string())
}

pub(super) fn issue(
    before: &Path,
    after: &Path,
    identity: WindowsStableIdentity,
    before_parent: WindowsStableIdentity,
    after_parent: WindowsStableIdentity,
) -> Result<PathMoveUndoReceipt, PlatformError> {
    let pending = |error| PlatformError::CommittedCleanupPending {
        transaction: after.display().to_string(),
        message: format!("Move completed, but its Undo ownership could not be verified: {error}"),
    };
    let before_absolute = std::path::absolute(before).map_err(io).map_err(&pending)?;
    let after_absolute = std::path::absolute(after).map_err(io).map_err(&pending)?;
    let before = before_absolute.as_path();
    let after = after_absolute.as_path();
    let verify = || -> Result<(), PlatformError> {
        if before == after
            || !before_parent.is_directory_without_reparse()
            || !after_parent.is_directory_without_reparse()
            || identity.volume_serial_number() != before_parent.volume_serial_number()
            || identity.volume_serial_number() != after_parent.volume_serial_number()
            || move_recovery::stable_identity_of_entry(after).map_err(io)? != identity
            || move_recovery::stable_identity_of_entry(
                before.parent().ok_or_else(|| stale(before))?,
            )
            .map_err(io)?
                != before_parent
            || move_recovery::stable_identity_of_entry(after.parent().ok_or_else(|| stale(after))?)
                .map_err(io)?
                != after_parent
        {
            return Err(stale(after));
        }
        Ok(())
    };
    verify().map_err(pending)?;
    Ok(PathMoveUndoReceipt {
        windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::Atomic {
            before_parent,
            after_parent,
        })),
        before_path: before.to_path_buf(),
        after_path: after.to_path_buf(),
        identity: PathMoveIdentity { windows: identity },
        displaced_identity: None,
        retained_displaced_path: None,
        requirement: PathMoveUndoRequirement::None,
    })
}

pub(in crate::move_transaction) fn observe(
    parent: &File,
    name: &OsStr,
) -> Result<Option<WindowsStableIdentity>, PlatformError> {
    let file = unsafe {
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
    };
    match file {
        Ok(file) => move_recovery::stable_identity_from_handle(&file)
            .map(Some)
            .map_err(io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

pub(crate) fn undo(receipt: &PathMoveUndoReceipt) -> Result<(), PlatformError> {
    let Some(WindowsPathUndo::Move(guard)) = receipt.windows_undo.as_ref() else {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Windows Move receipt has no native publication binding".into(),
        ));
    };
    let _lock = super::acquire_windows_recovery_lock()?;
    let (before_parent_identity, after_parent_identity) = match guard {
        WindowsMoveUndoGuard::Replace(record) => return overwrite::undo(receipt, record),
        WindowsMoveUndoGuard::CrossVolume(guard) => return cross::undo(receipt, guard),
        WindowsMoveUndoGuard::Atomic {
            before_parent,
            after_parent,
        } => (*before_parent, *after_parent),
    };
    if receipt.before_path == receipt.after_path
        || super::windows_same_volume_kind_from_identity(receipt.identity.windows).is_none()
        || receipt.displaced_identity.is_some()
        || receipt.retained_displaced_path.is_some()
        || receipt.requirement != PathMoveUndoRequirement::None
        || !before_parent_identity.is_directory_without_reparse()
        || !after_parent_identity.is_directory_without_reparse()
        || receipt.identity.windows.volume_serial_number()
            != before_parent_identity.volume_serial_number()
        || receipt.identity.windows.volume_serial_number()
            != after_parent_identity.volume_serial_number()
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Unsupported Windows Move inverse topology".into(),
        ));
    }
    let before = &receipt.before_path;
    let after = &receipt.after_path;
    let before_dir = before.parent().ok_or_else(|| stale(before))?;
    let after_dir = after.parent().ok_or_else(|| stale(after))?;
    let before_name = before.file_name().ok_or_else(|| stale(before))?;
    let after_name = after.file_name().ok_or_else(|| stale(after))?;
    windows_fs::valid_component(before_name).map_err(io)?;
    windows_fs::valid_component(after_name).map_err(io)?;
    // Native parents stay pinned only during the inverse. No receipt retains a
    // handle that could prevent a later ordinary volume unmount.
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE;
    let root = super::windows_recovery_root()?;
    let before_binding = super::windows_control::DirectoryBinding::capture(
        &root,
        before_dir,
        before_parent_identity,
    )?;
    let after_binding =
        super::windows_control::DirectoryBinding::capture(&root, after_dir, after_parent_identity)?;
    let before_parent = before_binding.pin()?;
    let after_parent = after_binding.pin()?;
    #[cfg(test)]
    super::windows_move_boundary(super::WindowsMoveBoundary::UndoParentsOpened, before_dir);
    let published = observe(&after_parent, after_name)?;
    let original = observe(&before_parent, before_name)?;
    let flush = || -> Result<(), PlatformError> {
        before_parent
            .sync_all()
            .and_then(|()| after_parent.sync_all())
            .map_err(|error| PlatformError::CommittedUndoCleanupPending {
                transaction: after.display().to_string(),
                message: format!("Move was restored, but its namespace flush needs retry: {error}"),
            })
    };
    if published.is_none() && original == Some(receipt.identity.windows) {
        return flush();
    }
    if published != Some(receipt.identity.windows) {
        return Err(stale(after));
    }
    if original.is_some() {
        return Err(PlatformError::DestinationExists(
            before.display().to_string(),
        ));
    }
    // Deny other DELETE-access opens while this exact entry is renamed. Open it
    // relative to its verified parent, without following a link's target.
    let entry = unsafe {
        windows_fs::open_relative(
            &after_parent,
            after_name,
            RelativeOptions {
                directory: None,
                create_new: false,
                access: DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                sharing,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)?;
    if move_recovery::stable_identity_from_handle(&entry).map_err(io)? != receipt.identity.windows
        || observe(&after_parent, after_name)? != Some(receipt.identity.windows)
    {
        return Err(stale(after));
    }
    #[cfg(test)]
    super::windows_move_boundary(super::WindowsMoveBoundary::BeforeUndoRename, after);
    super::rename_windows_handle_no_replace(&entry, &before_parent, before_name)?;
    #[cfg(test)]
    super::windows_move_boundary(super::WindowsMoveBoundary::UndoRenamed, before);
    flush()
}

#[cfg(test)]
mod tests;

pub(in crate::move_transaction) mod overwrite;

pub(in crate::move_transaction) mod cross;

mod plan;
pub use plan::WindowsMoveUndoPlan;
