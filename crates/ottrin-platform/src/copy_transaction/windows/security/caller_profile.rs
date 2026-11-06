//! Native original-caller profile and fixed registry ancestry admission.
//! Existing application directories retain all contents and permissions. Cached
//! evidence is compared with live handles, never installed as native security.
use super::*;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const PARENTS: [&str; 2] = ["Ottrin", "State"];
const SHARING: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CallerDirectoryBinding {
    identity: WindowsStableIdentity,
    location: Vec<u16>,
    security: Parts,
    case_sensitive: bool,
}
struct Directory {
    file: File,
    binding: CallerDirectoryBinding,
}
impl Directory {
    fn capture(file: File, user: &[u8], owner: &[u8]) -> Result<Self, PlatformError> {
        let identity = stable_identity_from_handle(&file).map_err(io)?;
        if !identity.is_directory_without_reparse() {
            return Err(io(
                "Original caller profile/state entry is not an ordinary native directory",
            ));
        }
        let mut raw = capture(&file)?;
        let security = parts(raw.as_mut_ptr().cast())?;
        // Shared application/profile directories need not have a private DACL.
        // Their full native policy is pinned; ownership must belong to the
        // authenticated user or its distinct default file owner.
        if !security.valid_record()
            || !security.present
            || security.dacl.is_none()
            || (security.owner != user && security.owner != owner)
        {
            return Err(io(
                "Original caller profile/state owner/group/DACL admission failed",
            ));
        }
        let binding = CallerDirectoryBinding {
            identity,
            location: windows_fs::location(&file)
                .map_err(io)?
                .as_os_str()
                .encode_wide()
                .collect(),
            security,
            case_sensitive: windows_fs::directory_case_sensitive(&file).map_err(io)?,
        };
        Ok(Self { file, binding })
    }
    fn validate_file(&self, file: &File) -> Result<(), PlatformError> {
        let mut raw = capture(file)?;
        if stable_identity_from_handle(file).map_err(io)? != self.binding.identity
            || windows_fs::location(file)
                .map_err(io)?
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>()
                != self.binding.location
            || windows_fs::directory_case_sensitive(file).map_err(io)?
                != self.binding.case_sensitive
            || parts(raw.as_mut_ptr().cast())? != self.binding.security
        {
            return Err(io(
                "Original caller profile/state identity/location/security/case policy changed",
            ));
        }
        Ok(())
    }
}

pub(crate) struct CallerProfile {
    registered: PathBuf,
    directory: Directory,
}
/// In-memory held ancestry. Every existing directory is observed, not adopted
/// for deletion. Fixed native names cannot be selected by request or environment.
pub(crate) struct CallerRegistryParent {
    base: Directory,
    directories: Vec<Directory>,
}
impl CallerProfile {
    pub(crate) fn capture(path: &Path, user: &[u8], owner: &[u8]) -> Result<Self, PlatformError> {
        let file = windows_fs::open_directory(path, FILE_GENERIC_READ).map_err(io)?;
        let result = Self {
            registered: path.to_path_buf(),
            directory: Directory::capture(file, user, owner)?,
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
        self.directory.validate_file(&self.directory.file)?;
        self.directory.validate_file(&current)
    }
    pub(crate) fn registry_parent(
        &self,
        user: &[u8],
        owner: &[u8],
        initialize: bool,
        mut create: impl FnMut(&File, &OsStr) -> std::io::Result<File>,
    ) -> Result<Option<CallerRegistryParent>, PlatformError> {
        self.validate()?;
        let access = FILE_GENERIC_READ | if initialize { FILE_GENERIC_WRITE } else { 0 };
        let base = if initialize {
            windows_fs::with_file_privileges(|| {
                windows_fs::open_directory(&self.registered, access)
            })
            .map_err(io)?
        } else {
            self.directory.file.try_clone().map_err(io)?
        };
        self.directory.validate_file(&base)?;
        let mut chain = CallerRegistryParent {
            base: Directory {
                file: base,
                binding: self.directory.binding.clone(),
            },
            directories: Vec::new(),
        };
        for name in PARENTS {
            chain.validate(self)?;
            let parent = chain.directories.last().unwrap_or(&chain.base);
            let open = || {
                windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        &parent.file,
                        OsStr::new(name),
                        Some(true),
                        access,
                        SHARING,
                    )
                })
            };
            let file = match open() {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && !initialize => {
                    chain.validate(self)?;
                    return Ok(None);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let file = match create(&parent.file, OsStr::new(name)) {
                        Ok(file) => file,
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            open().map_err(io)?
                        }
                        Err(error) => return Err(io(error)),
                    };
                    parent.file.sync_all().map_err(io)?;
                    file
                }
                Err(error) => return Err(io(error)),
            };
            let directory = Directory::capture(file, user, owner)?;
            namespace::validate_binding_with(&parent.file, directory.binding.identity, || {
                windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        &parent.file,
                        OsStr::new(name),
                        Some(true),
                        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                        SHARING,
                    )
                })
            })?;
            chain.directories.push(directory);
            chain.validate(self)?;
        }
        Ok(Some(chain))
    }
}
impl CallerRegistryParent {
    pub(crate) fn state(&self) -> Result<&File, PlatformError> {
        if self.directories.len() != PARENTS.len() {
            return Err(io("Incomplete caller registry ancestry"));
        }
        Ok(&self.directories.last().unwrap().file)
    }
    pub(crate) fn binding(&self) -> Vec<CallerDirectoryBinding> {
        std::iter::once(&self.base)
            .chain(&self.directories)
            .map(|directory| directory.binding.clone())
            .collect()
    }
    pub(crate) fn validate(&self, profile: &CallerProfile) -> Result<(), PlatformError> {
        profile.validate()?;
        if self.base.binding != profile.directory.binding || self.directories.len() > PARENTS.len()
        {
            return Err(io(
                "Caller registry ancestry does not match its admitted profile",
            ));
        }
        self.base.validate_file(&self.base.file)?;
        let mut parent = &self.base;
        for (directory, name) in self.directories.iter().zip(PARENTS) {
            parent.validate_file(&parent.file)?;
            directory.validate_file(&directory.file)?;
            namespace::validate_binding_with(&parent.file, directory.binding.identity, || {
                windows_fs::with_file_privileges(|| {
                    windows_fs::open_existing_for_caller(
                        &parent.file,
                        OsStr::new(name),
                        Some(true),
                        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                        SHARING,
                    )
                })
            })?;
            parent = directory;
        }
        profile.validate()
    }
}
