//! Private entry creation and identity-bound reopen for an authenticated caller.
//! Native permissions are never repaired to admit an existing entry.
use super::*;

pub(crate) struct CallerCreationSecurity {
    descriptor: Derived,
    expected: Parts,
    directory: bool,
}

pub(crate) fn open_caller_private_entry(
    parent: &File,
    name: &OsStr,
    expected: WindowsStableIdentity,
    owner: &[u8],
    group: &[u8],
    writable: bool,
    sharing: u32,
) -> std::io::Result<File> {
    let directory = expected.is_directory_without_reparse();
    if (!directory && !expected.is_regular_file_without_reparse())
        || !stable_identity_from_handle(parent)
            .map_err(std::io::Error::other)?
            .is_directory_without_reparse()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private reopen requires ordinary native entry and parent identities",
        ));
    }
    let access = FILE_GENERIC_READ
        | if writable {
            FILE_GENERIC_WRITE | DELETE | WRITE_DAC
        } else {
            0
        };
    let file = windows_fs::with_file_privileges(|| {
        windows_fs::open_existing_for_caller(parent, name, Some(directory), access, sharing)
    })?;
    if stable_identity_from_handle(&file).map_err(std::io::Error::other)? != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "private entry identity changed; data retained",
        ));
    }
    PrivateSecurity::capture_for_caller(&file, owner, group)?;
    Ok(file)
}
impl CallerCreationSecurity {
    pub(crate) fn capture(
        token: &OwnedHandle,
        owner: &[u8],
        group: &[u8],
        directory: bool,
    ) -> Result<Self, PlatformError> {
        let private = windows_fs::LocalSecurity::private().map_err(io)?;
        // Private entries have a protected creator DACL and no parent-derived
        // inheritance. Requesting auto-inheritance misstates their native control.
        let (descriptor, expected) =
            derive(ptr::null_mut(), private.as_raw(), token, directory, 0)?;
        // The token can be modified by its owning process. Bind the derived
        // descriptor to the immutable admitted values, not another mutable read.
        if !PrivateSecurity(expected.clone()).valid_for(owner) || expected.group != group {
            return Err(io(
                "Native caller-derived private creation policy is invalid",
            ));
        }
        Ok(Self {
            descriptor,
            expected,
            directory,
        })
    }
    pub(crate) fn create(
        &self,
        parent: &File,
        name: &OsStr,
        sharing: u32,
    ) -> std::io::Result<File> {
        let file = windows_fs::with_file_privileges(|| unsafe {
            windows_fs::create_private_for_caller(
                parent,
                name,
                self.directory,
                sharing,
                self.descriptor.0.cast(),
            )
        })?;
        let observed =
            PrivateSecurity::capture(&file, &self.expected.owner).map_err(std::io::Error::other)?;
        if observed.0 != self.expected {
            return Err(std::io::Error::other(format!(
                "Created caller-owned entry has unexpected native security: owner matches={}, group matches={}, DACL matches={}, control expected={:#x} actual={:#x}; data retained",
                observed.0.owner == self.expected.owner,
                observed.0.group == self.expected.group,
                observed.0.dacl == self.expected.dacl,
                self.expected.control,
                observed.0.control,
            )));
        }
        Ok(file)
    }
}
