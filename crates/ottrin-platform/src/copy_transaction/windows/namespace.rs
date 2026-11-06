//! Private destination-local Copy entries; no publication or destructive cleanup.
use super::authority::Authority;
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
    pub(super) authority: Authority,
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

/// Observe a leaf through its held ordinary parent without following reparse
/// targets. This checks an existing identity; it never establishes ownership.
pub(super) fn validate_binding(
    parent: &File,
    name: &OsStr,
    expected: WindowsStableIdentity,
) -> Result<(), PlatformError> {
    validate_binding_with(parent, expected, || unsafe {
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
    })
}

/// The authority supplies its single-component, no-follow presence opener.
/// Native parent/leaf identity comparison remains one implementation.
pub(super) fn validate_binding_with(
    parent: &File,
    expected: WindowsStableIdentity,
    open: impl FnOnce() -> std::io::Result<File>,
) -> Result<(), PlatformError> {
    let before = directory_identity(parent)?;
    let observed = open().map_err(io)?;
    if stable_identity_from_handle(&observed).map_err(io)? != expected
        || directory_identity(parent)? != before
    {
        return Err(io("Copy entry binding changed; all data retained"));
    }
    Ok(())
}

/// New Copy entries must not share their permissions with another hard-link name.
/// Source hard links are valid and are never passed to this ownership guard.
pub(super) fn validate_single_link(file: &File) -> Result<(), PlatformError> {
    if link_count(file)? != 1 {
        return Err(io("Copy candidate is aliased by another hard-link name"));
    }
    Ok(())
}

pub(super) fn link_count(file: &File) -> Result<u32, PlatformError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0 {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(info.nNumberOfLinks)
}

impl<'a> PrivateStage<'a> {
    #[cfg(test)]
    pub(super) fn create(parent: &'a File) -> Result<Self, PlatformError> {
        Self::for_transaction_with_authority(
            parent,
            &super::super::random_transaction_id()?,
            Authority::Process,
        )
    }
    pub(super) fn for_transaction_with_authority(
        parent: &'a File,
        transaction: &str,
        authority: Authority,
    ) -> Result<Self, PlatformError> {
        authority.validate()?;
        if transaction.len() != 32
            || !transaction
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(io("Invalid Copy transaction identifier"));
        }
        let parent_identity = directory_identity(parent)?;
        let name = OsString::from(format!(".ottrin-copy-{transaction}"));
        let directory = authority
            .create(parent, &name, true, FILE_SHARE_READ)
            .map_err(io)?;
        let identity =
            directory_identity(&directory).map_err(|error| retained(&directory, &name, error))?;
        let mut candidate_name = name.clone();
        candidate_name.push(".candidate");
        let stage = Self {
            parent,
            authority,
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
        self.authority
            .binding(self.parent, &self.name, self.identity)
            .map_err(|e| self.retained(e))?;
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
    pub(super) fn create_candidate(
        &self,
        directory: bool,
    ) -> Result<windows_fs::PerObjectFile, PlatformError> {
        self.validate()?;
        let file = self
            .authority
            .create_candidate(
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
        self.authority
            .binding(self.parent, &self.candidate_name, identity)
            .map_err(|e| self.retained(e))?;
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
    open_source_with(&Authority::Process, parent, name, directory)
}

pub(super) fn open_source_with(
    authority: &Authority,
    parent: &File,
    name: &OsStr,
    directory: Option<bool>,
) -> Result<File, PlatformError> {
    authority.validate()?;
    let expected = directory_identity(parent)?;
    let file = authority
        .open_entry(parent, name, directory, FILE_GENERIC_READ, FILE_SHARE_READ)
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

#[cfg(test)]
pub(super) fn exercise_caller_staging(
    context: &std::sync::Arc<crate::windows_privileged_transport::CallerCopyContext>,
    parent: &File,
    source: &std::path::Path,
) {
    use super::{EntryVersion, tree};
    use tree::StagingJournal;
    let authority = Authority::Caller(std::sync::Arc::clone(context));
    let source = open_source_path(source).unwrap();
    let transaction = super::super::random_transaction_id().unwrap();
    let stage =
        PrivateStage::for_transaction_with_authority(parent, &transaction, authority.clone())
            .unwrap();
    let owner = authority.owner().unwrap();
    super::security::PrivateSecurity::capture(&stage.directory, &owner).unwrap();
    let root = stage.create_candidate(true).unwrap();
    super::security::PrivateSecurity::capture(&root, &owner).unwrap();
    assert!(!windows_fs::directory_case_sensitive(&root).unwrap());
    struct Events {
        next: usize,
        pending: bool,
        created: bool,
        complete: bool,
    }
    impl tree::StagingJournal for Events {
        fn intent(
            &mut self,
            index: usize,
            _: Option<usize>,
            _: &OsStr,
            _: &EntryVersion,
        ) -> Result<(), PlatformError> {
            assert_eq!(index, self.next);
            assert!(!self.pending);
            self.pending = true;
            Ok(())
        }
        fn created(&mut self, index: usize, _: &File, _: &File) -> Result<(), PlatformError> {
            assert_eq!(index, self.next);
            assert!(self.pending);
            assert!(!self.created);
            self.created = true;
            Ok(())
        }
        fn staged(
            &mut self,
            index: usize,
            _: &super::metadata::StagedEntry,
        ) -> Result<(), PlatformError> {
            assert_eq!(index, self.next);
            assert!(self.pending && self.created);
            self.next += 1;
            self.pending = false;
            self.created = false;
            Ok(())
        }
        fn verified(&mut self, count: usize) -> Result<(), PlatformError> {
            assert_eq!(count, self.next);
            assert!(!self.pending);
            self.complete = true;
            Ok(())
        }
    }
    let mut events = Events {
        next: 0,
        pending: false,
        created: false,
        complete: false,
    };
    let pinned = tree::PinnedTree::open(&source, 16, &mut |_| Ok(())).unwrap();
    events
        .intent(0, None, &stage.candidate_name, pinned.root())
        .unwrap();
    let staged = pinned
        .stage(&root, parent, &mut events, &authority, |_| Ok(()))
        .unwrap();
    assert!(events.complete);
    assert_eq!(staged.entry_count(), 4);
    staged.verify_and_finalize(|_| Ok(())).unwrap();
    stage.validate_candidate(&root, true).unwrap();
    assert!(windows_fs::directory_case_sensitive(&root).unwrap());
    let nested = authority
        .child(
            &root,
            OsStr::new("Nested"),
            true,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
    super::security::PrivateSecurity::capture(&nested, &owner).unwrap();
    assert!(windows_fs::directory_case_sensitive(&nested).unwrap());
    let mut bytes = authority
        .child(
            &nested,
            OsStr::new("payload.txt"),
            false,
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
    super::security::PrivateSecurity::capture(&bytes, &owner).unwrap();
    let mut content = Vec::new();
    std::io::Read::read_to_end(&mut bytes, &mut content).unwrap();
    assert_eq!(content, b"nested original-caller payload");
    assert!(
        windows_fs::PerObjectFile::create_private(
            parent,
            &stage.candidate_name,
            true,
            FILE_SHARE_READ
        )
        .is_err()
    );
    // The retained root policy must keep caller authority after its whole-tree
    // holder is consumed; this remains a component, not publication authority.
    let plan = staged
        .destination_security_plan(parent, &stage.candidate_name, || Ok(()))
        .unwrap();
    assert_eq!(plan.entry_count(), 4);
    let policy = plan.into_root_policy();
    policy.apply(parent, &stage.candidate_name, &root).unwrap();
    policy.validate_candidate(&root).unwrap();
    stage.validate_candidate(&root, true).unwrap();
    context.validate().unwrap();
}
#[cfg(test)]
fn open_source_path(path: &std::path::Path) -> Result<File, PlatformError> {
    windows_fs::open_directory(path, FILE_GENERIC_READ).map_err(io)
}
