//! Explicit authority shared by registry and private transaction staging.
//! No ambient caller override; saved evidence never chooses a token.
use super::{WindowsStableIdentity, io, namespace, security, stable_identity_from_handle};
use crate::{PlatformError, windows_fs};
use std::{ffi::OsStr, fs::File, path::Path, ptr, sync::Arc};
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Clone)]
pub(super) enum Authority {
    Process,
    Caller(Arc<crate::windows_privileged_transport::CallerCopyContext>),
}
impl Authority {
    pub(super) fn validate(&self) -> Result<(), PlatformError> {
        match self {
            Self::Process => Ok(()),
            Self::Caller(caller) => caller.validate().map_err(io),
        }
    }
    pub(super) fn owner(&self) -> Result<Vec<u8>, PlatformError> {
        match self {
            Self::Process => windows_fs::current_owner(),
            Self::Caller(caller) => caller.default_file_owner_sid().map(Vec::from).map_err(io),
        }
    }
    pub(super) fn token(&self) -> Result<std::os::windows::io::OwnedHandle, PlatformError> {
        self.validate()?;
        match self {
            Self::Process => windows_fs::current_token(),
            Self::Caller(caller) => caller.snapshot_token().map_err(io),
        }
    }
    /// Scoped helper file access keeps the original caller's policy separate
    /// from the effective privileges needed by native filesystem operations.
    pub(super) fn file_operation<T>(
        &self,
        operation: impl FnOnce() -> Result<T, PlatformError>,
    ) -> Result<T, PlatformError> {
        self.validate()?;
        let result = match self {
            Self::Process => operation(),
            Self::Caller(_) => windows_fs::with_file_privileges(|| Ok(operation())).map_err(io)?,
        };
        self.validate()?;
        result
    }
    pub(super) fn identity_at(
        &self,
        parent: &File,
        name: &OsStr,
        expected: WindowsStableIdentity,
    ) -> Result<bool, PlatformError> {
        match self.probe(parent, name) {
            Ok(file) => Ok(stable_identity_from_handle(&file).map_err(io)? == expected),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io(error)),
        }
    }
    pub(super) fn create(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> std::io::Result<File> {
        match self {
            Self::Process => windows_fs::create_private(parent, name, directory, sharing),
            Self::Caller(caller) => caller.create_private_entry(parent, name, directory, sharing),
        }
    }
    pub(super) fn child(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        access: u32,
        sharing: u32,
    ) -> std::io::Result<File> {
        let file = self.open_child(parent, name, directory, access, sharing)?;
        if let Self::Caller(caller) = self {
            security::PrivateSecurity::capture_for_caller(
                &file,
                caller.default_file_owner_sid()?,
                caller.default_file_group_sid()?,
            )?;
            caller.validate()?;
        }
        Ok(file)
    }
    /// Readonly identity observation may encounter a replacement control. Its
    /// bytes/security cannot be adopted until the expected native ID matches.
    pub(super) fn observe_file(&self, parent: &File, name: &OsStr) -> std::io::Result<File> {
        self.open_child(
            parent,
            name,
            false,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
    }
    // A native handle is access, not ownership. Its consumer must validate
    // the expected native binding/security before adopting state or mutating.
    pub(super) fn open_child(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        access: u32,
        sharing: u32,
    ) -> std::io::Result<File> {
        match self {
            Self::Process => ordinary_child(parent, name, directory, access, sharing),
            Self::Caller(caller) => {
                caller.validate()?;
                let file = windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        parent,
                        name,
                        Some(directory),
                        access,
                        sharing,
                    )
                })?;
                caller.validate()?;
                Ok(file)
            }
        }
    }
    pub(super) fn probe(&self, parent: &File, name: &OsStr) -> std::io::Result<File> {
        let open = || unsafe {
            windows_fs::open_relative(
                parent,
                name,
                windows_fs::RelativeOptions {
                    directory: None,
                    create_new: false,
                    access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        };
        match self {
            Self::Process => open(),
            Self::Caller(caller) => {
                caller.validate()?;
                let file = windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        parent,
                        name,
                        None,
                        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    )
                })?;
                caller.validate()?;
                Ok(file)
            }
        }
    }
    pub(super) fn binding(
        &self,
        parent: &File,
        name: &OsStr,
        expected: WindowsStableIdentity,
    ) -> Result<(), PlatformError> {
        self.validate()?;
        namespace::validate_binding_with(parent, expected, || self.probe(parent, name))?;
        self.validate()
    }
    pub(super) fn writable_parent(&self, path: &Path) -> Result<File, PlatformError> {
        self.validate()?;
        let open = || windows_fs::open_directory(path, FILE_GENERIC_READ | FILE_GENERIC_WRITE);
        let file = match self {
            Self::Process => open(),
            Self::Caller(_) => windows_fs::with_file_privileges(open),
        }
        .map_err(io)?;
        self.validate()?;
        Ok(file)
    }

    pub(super) fn create_candidate(
        &self,
        parent: &File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> std::io::Result<windows_fs::PerObjectFile> {
        match self {
            Self::Process => {
                windows_fs::PerObjectFile::create_private(parent, name, directory, sharing)
            }
            Self::Caller(caller) => {
                caller.create_private_candidate(parent, name, directory, sharing)
            }
        }
    }
    pub(super) fn case_policy(
        &self,
        directory: &File,
        expected: bool,
    ) -> Result<(), PlatformError> {
        self.validate()?;
        match self {
            Self::Process => {
                windows_fs::set_empty_directory_case_policy(directory, expected).map_err(io)?
            }
            Self::Caller(caller) => caller
                .set_private_directory_case_policy(
                    directory,
                    stable_identity_from_handle(directory).map_err(io)?,
                    expected,
                )
                .map_err(io)?,
        }
        self.validate()
    }
}

pub(super) fn ordinary_child(
    parent: &File,
    name: &OsStr,
    directory: bool,
    access: u32,
    sharing: u32,
) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            parent,
            name,
            windows_fs::RelativeOptions {
                directory: Some(directory),
                create_new: false,
                access,
                sharing,
                security: ptr::null(),
            },
        )
    }
}
