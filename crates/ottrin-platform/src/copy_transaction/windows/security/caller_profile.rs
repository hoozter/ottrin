//! Read-only admission of the kernel-resolved original caller's profile root.
//! Directory contents are allowed to change; identity, location and permissions
//! are pinned. No saved descriptor is ever installed or used to create files.
use super::*;
use std::path::{Path, PathBuf};

pub(crate) struct CallerProfile {
    registered: PathBuf,
    location: PathBuf,
    file: File,
    identity: WindowsStableIdentity,
    security: Parts,
    case_sensitive: bool,
}
impl CallerProfile {
    pub(crate) fn capture(path: &Path, user: &[u8], owner: &[u8]) -> Result<Self, PlatformError> {
        let file = windows_fs::open_directory(path, FILE_GENERIC_READ).map_err(io)?;
        let identity = stable_identity_from_handle(&file).map_err(io)?;
        if !identity.is_directory_without_reparse() {
            return Err(io(
                "Original caller profile is not an ordinary native directory",
            ));
        }
        let mut raw = capture(&file)?;
        let security = parts(raw.as_mut_ptr().cast())?;
        // Profiles normally belong to TokenUser, even when TokenOwner defaults
        // new files to an owner-capable group. Preserve that distinction.
        if !security.valid_record()
            || !security.present
            || security.dacl.is_none()
            || (security.owner != user && security.owner != owner)
        {
            return Err(io(
                "Original caller profile owner/group/DACL admission failed",
            ));
        }
        let result = Self {
            registered: path.to_path_buf(),
            location: windows_fs::location(&file).map_err(io)?,
            case_sensitive: windows_fs::directory_case_sensitive(&file).map_err(io)?,
            file,
            identity,
            security,
        };
        result.validate()?;
        Ok(result)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.registered
    }
    pub(crate) fn validate(&self) -> Result<(), PlatformError> {
        let current =
            windows_fs::open_directory(&self.registered, FILE_GENERIC_READ).map_err(io)?;
        for file in [&self.file, &current] {
            let mut raw = capture(file)?;
            if stable_identity_from_handle(file).map_err(io)? != self.identity
                || windows_fs::location(file).map_err(io)? != self.location
                || windows_fs::directory_case_sensitive(file).map_err(io)? != self.case_sensitive
                || parts(raw.as_mut_ptr().cast())? != self.security
            {
                return Err(io(
                    "Original caller profile identity/location/security/case policy changed",
                ));
            }
        }
        Ok(())
    }
}
