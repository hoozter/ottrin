//! Private destination-local Copy entries; no publication or destructive cleanup.
use super::{PlatformError, WindowsStableIdentity, io, stable_identity_from_handle};
use crate::windows_fs::{self, RelativeOptions};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::ptr;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    SYNCHRONIZE,
};

pub(super) struct PrivateStage<'a> {
    parent: &'a File,
    parent_identity: WindowsStableIdentity,
    pub(super) directory: File,
    pub(super) name: OsString,
    pub(super) candidate_name: OsString,
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
        let mut candidate_name = name.clone();
        candidate_name.push(".candidate");
        let stage = Self {
            parent,
            parent_identity,
            directory,
            name,
            candidate_name,
            identity,
        };
        stage.validate()?;
        Ok(stage)
    }

    pub(super) fn parent(&self) -> &File {
        self.parent
    }
    pub(super) fn identities(&self) -> (WindowsStableIdentity, WindowsStableIdentity) {
        (self.parent_identity, self.identity)
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
                    directory: Some(true),
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
        retained(
            &self.directory,
            &self.name,
            format!(
                "{error}; planned destination-local candidate {:?}",
                self.candidate_name
            ),
        )
    }

    /// Create beside the transaction directory, directly under the real parent.
    /// An extra inheritance level would discard NO_PROPAGATE audit/label ACEs.
    /// Only the DACL is supplied; Windows derives the normal new-child SACL and
    /// mandatory label without requiring the caller to read an audit SACL.
    /// No existing entry at the planned name is adopted or replaced.
    pub(super) fn create_candidate(&self, directory: bool) -> Result<File, PlatformError> {
        self.validate()?;
        let file = windows_fs::create_private(
            self.parent,
            &self.candidate_name,
            directory,
            FILE_SHARE_READ,
        )
        .map_err(|error| self.retained(error))?;
        self.validate_candidate(&file, directory)?;
        Ok(file)
    }

    /// Validate the private binding of an already-owned create-new result.
    /// This does not establish ownership of an arbitrary supplied handle. After
    /// publication the transaction must use its recorded destination binding.
    pub(super) fn validate_candidate(
        &self,
        candidate: &File,
        directory: bool,
    ) -> Result<(), PlatformError> {
        self.validate()?;
        let identity = stable_identity_from_handle(candidate).map_err(|e| self.retained(e))?;
        let supported = if super::links::capture(candidate)
            .map_err(|e| self.retained(e))?
            .is_some()
        {
            let is_directory = super::metadata::basic(candidate)
                .map_err(|e| self.retained(e))?
                .FileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                != 0;
            is_directory == directory
        } else if directory {
            identity.is_directory_without_reparse()
        } else {
            identity.is_regular_file_without_reparse()
        };
        if !supported {
            return Err(self.retained("candidate type does not match creation policy"));
        }
        let observed = unsafe {
            windows_fs::open_relative(
                self.parent,
                &self.candidate_name,
                RelativeOptions {
                    directory: Some(directory),
                    create_new: false,
                    access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        }
        .map_err(|e| self.retained(e))?;
        if stable_identity_from_handle(&observed).map_err(|e| self.retained(e))? != identity {
            return Err(self.retained("candidate name no longer names the held object"));
        }
        self.validate()
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
pub(super) fn open_source(
    parent: &File,
    name: &OsStr,
    directory: Option<bool>,
) -> Result<File, PlatformError> {
    let expected = directory_identity(parent)?;
    let file = unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory,
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)?;
    let identity = stable_identity_from_handle(&file).map_err(io)?;
    let correct_type = match directory {
        Some(true) => identity.is_directory_without_reparse(),
        Some(false) => identity.is_regular_file_without_reparse(),
        None => {
            identity.is_directory_without_reparse()
                || identity.is_regular_file_without_reparse()
                || super::links::capture(&file)?.is_some()
        }
    };
    if !correct_type || directory_identity(parent)? != expected {
        return Err(io("Copy source type is unsupported or its parent changed"));
    }
    Ok(file)
}
