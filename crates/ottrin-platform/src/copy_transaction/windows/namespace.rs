//! Private destination-local Copy entries; no publication or destructive cleanup.
use super::{PlatformError, WindowsStableIdentity, io, stable_identity_from_handle};
use crate::windows_fs::{self, RelativeOptions};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::ptr;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(super) struct PrivateStage<'a> {
    parent: &'a File,
    parent_identity: WindowsStableIdentity,
    pub(super) directory: File,
    pub(super) name: OsString,
    identity: WindowsStableIdentity,
}

fn directory_identity(file: &File) -> Result<WindowsStableIdentity, PlatformError> {
    let identity = stable_identity_from_handle(file).map_err(io)?;
    if !identity.is_directory_without_reparse() {
        return Err(io("Copy parent/stage is not a non-reparse directory"));
    }
    Ok(identity)
}

impl<'a> PrivateStage<'a> {
    pub(super) fn create(parent: &'a File) -> Result<Self, PlatformError> {
        let parent_identity = directory_identity(parent)?;
        let (name, directory) = loop {
            let id = super::super::random_transaction_id()?;
            let name = OsString::from(format!(".ottrin-copy-{id}"));
            match windows_fs::create_private(parent, &name, true, FILE_SHARE_READ) {
                Ok(file) => break (name, file),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io(error)),
            }
        };
        let identity =
            directory_identity(&directory).map_err(|error| retained(&directory, &name, error))?;
        let stage = Self {
            parent,
            parent_identity,
            directory,
            name,
            identity,
        };
        stage.validate()?;
        Ok(stage)
    }

    pub(super) fn validate(&self) -> Result<(), PlatformError> {
        if directory_identity(self.parent).map_err(|error| self.retained(error))?
            != self.parent_identity
            || directory_identity(&self.directory).map_err(|error| self.retained(error))?
                != self.identity
        {
            return Err(self.retained("Copy directory identity changed"));
        }
        // Observe the binding through the pinned parent, not its remembered path.
        let observed = unsafe {
            windows_fs::open_relative(
                self.parent,
                &self.name,
                RelativeOptions {
                    directory: true,
                    create_new: false,
                    access: FILE_GENERIC_READ,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        }
        .map_err(|error| self.retained(error))?;
        if directory_identity(&observed).map_err(|error| self.retained(error))? != self.identity {
            return Err(self.retained("Copy stage name no longer names its directory"));
        }
        Ok(())
    }

    fn retained(&self, error: impl std::fmt::Display) -> PlatformError {
        retained(&self.directory, &self.name, error)
    }

    pub(super) fn create_candidate(&self) -> Result<File, PlatformError> {
        self.validate()?;
        let file = windows_fs::create_private(
            &self.directory,
            OsStr::new("candidate"),
            false,
            FILE_SHARE_READ,
        )
        .map_err(|error| self.retained(error))?;
        self.validate()?;
        Ok(file)
    }
}

fn retained(directory: &File, name: &OsStr, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::CleanupPending {
        transaction: windows_fs::location(directory)
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| format!("native Copy directory {name:?}")),
        message: format!("{error}; unpublished entries retained, no cleanup attempted"),
    }
}

/// A Copy source needs read access, never the DELETE right needed by Move.
pub(super) fn open_source(parent: &File, name: &OsStr) -> Result<File, PlatformError> {
    let expected = directory_identity(parent)?;
    let file = unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: false,
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)?;
    let identity = stable_identity_from_handle(&file).map_err(io)?;
    if !identity.is_regular_file_without_reparse() || directory_identity(parent)? != expected {
        return Err(io("Copy source is a reparse entry or its parent changed"));
    }
    Ok(file)
}
