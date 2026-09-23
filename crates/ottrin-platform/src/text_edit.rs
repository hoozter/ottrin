//! Bounded Linux text-edit snapshots and descriptor-relative publication.
//!
//! This module deliberately does not classify file formats. The preview layer
//! decides whether a file is a supported text preview; this boundary admits
//! only complete, demonstrably plain, UTF-8 local files. Linux saves retain a
//! descriptor for the source parent so a later pathname substitution cannot
//! redirect staging or publication. Other platforms stay read-only.

use std::path::Path;
use thiserror::Error;

/// The largest complete text file the first Inspector editor will load.
///
/// This matches the existing bounded text preview surface. Larger files remain
/// read-only rather than presenting a truncated buffer as editable content.
pub const MAX_EDITABLE_TEXT_BYTES: u64 = 128 * 1024;

/// Atomic replacement is enabled only on Linux, where the implementation pins
/// the source parent and uses descriptor-relative primitives throughout save.
/// Other platforms stay read-only until their native replacement boundary is
/// proven.
pub const fn text_editing_supported() -> bool {
    cfg!(target_os = "linux")
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextEditError {
    #[error("atomic text editing is not available on this platform")]
    NotAvailable,
    #[error("only regular local files can be edited")]
    NotRegularFile,
    #[error("symbolic links cannot be edited")]
    Symlink,
    #[error("hard-linked files cannot be atomically replaced without changing their link topology")]
    HardLinked,
    #[error("this file has metadata that safe text editing cannot preserve")]
    UnsupportedMetadata,
    #[error("text files larger than {limit} bytes remain read-only")]
    TooLarge { limit: u64 },
    #[error("only valid UTF-8 text can be edited")]
    InvalidUtf8,
    #[error("binary control characters cannot be edited as plain text")]
    BinaryText,
    #[error("the file changed after editing began; your unsaved changes are still in the editor")]
    Conflict,
    #[error(
        "Recovery conflict: {detail}. Your draft is still in the editor. Recovery directory: {recovery:?} (edits = your draft; candidate = displaced entry). Copy your edits before reloading."
    )]
    RecoveryConflict {
        recovery: std::path::PathBuf,
        detail: String,
    },
    #[error(
        "the edited text may be published, but directory durability could not be confirmed: {0}"
    )]
    DurabilityUncertain(String),
    #[error(
        "save stopped after staging; no cleanup was attempted because the stage name may no longer be owned: {0}"
    )]
    StageRetentionRequired(String),
    #[error("could not save the edited text: {0}")]
    Write(String),
    #[error("could not open the text file: {0}")]
    Read(String),
}

#[derive(Debug)]
pub struct TextEditSession {
    #[cfg(target_os = "linux")]
    inner: linux::Session,
}

impl TextEditSession {
    /// Opens one complete UTF-8 regular file and captures its current identity
    /// and content generation. The caller must still decide whether the
    /// preview format itself is editable.
    pub fn open(path: &Path) -> Result<Self, TextEditError> {
        #[cfg(target_os = "linux")]
        {
            linux::Session::open(path).map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(TextEditError::NotAvailable)
        }
    }

    pub fn contents(&self) -> &str {
        #[cfg(target_os = "linux")]
        {
            self.inner.contents()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = self;
            unreachable!("text-edit sessions cannot be created on this platform")
        }
    }

    /// Rechecks the descriptor-relative identity and captured generation after
    /// staging I/O, then atomically exchanges the candidate with the source.
    /// The displaced source is retained in private staging. If it is not the
    /// captured source, both entries are retained and a recovery conflict is
    /// returned. Restoration only fills an absent source name without replacing
    /// any entry. Successful saves also retain the prior inode for late writers.
    pub fn save(&self, edited_text: &str) -> Result<(), TextEditError> {
        #[cfg(target_os = "linux")]
        {
            self.inner.save(edited_text)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = edited_text;
            Err(TextEditError::NotAvailable)
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    fn save_with_test_hook(
        &self,
        edited_text: &str,
        hook: impl FnMut(linux::TestPoint) -> Result<(), TextEditError>,
    ) -> Result<(), TextEditError> {
        self.inner.save_with_test_hook(edited_text, hook)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{MAX_EDITABLE_TEXT_BYTES, TextEditError};
    use rustix::fd::OwnedFd;
    use rustix::fs::{
        AtFlags, CWD, FileType, Mode, OFlags, RenameFlags, fchmod, flistxattr, fstat, fsync,
        linkat, mkdirat, openat, renameat_with, statat,
    };
    use rustix::io::Errno;
    use sha2::{Digest, Sha256};
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::path::Path;

    pub(super) const STAGED_CANDIDATE: &str = "candidate";

    #[derive(Debug)]
    pub(super) struct Session {
        parent: OwnedFd,
        name: OsString,
        source: Snapshot,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Identity {
        device: u64,
        inode: u64,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct PlainMetadata {
        identity: Identity,
        mode: u32,
        uid: u32,
        gid: u32,
        size: u64,
        blocks: u64,
        modified: (i64, u64),
        changed: (i64, u64),
    }

    #[derive(Debug)]
    struct Snapshot {
        metadata: PlainMetadata,
        bytes: Vec<u8>,
        digest: [u8; 32],
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum TestPoint {
        BeforePublication,
        AfterSourceVerification,
        AfterExchange,
        BeforeRecovery,
        BeforeDirectorySync,
    }

    impl Session {
        pub(super) fn open(path: &Path) -> Result<Self, TextEditError> {
            let parent_path = path.parent().ok_or_else(|| {
                TextEditError::Read("the text file does not have a parent directory".to_string())
            })?;
            let name = path.file_name().ok_or_else(|| {
                TextEditError::Read("the text file does not have a file name".to_string())
            })?;
            let parent = openat(
                CWD,
                parent_path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(read_error)?;
            let source = Snapshot::capture(&parent, name)?;
            Ok(Self {
                parent,
                name: name.to_os_string(),
                source,
            })
        }

        pub(super) fn contents(&self) -> &str {
            std::str::from_utf8(&self.source.bytes).expect("text-edit session stores valid UTF-8")
        }

        pub(super) fn save(&self, edited_text: &str) -> Result<(), TextEditError> {
            self.save_with_hook(edited_text, |_| Ok(()))
        }

        #[cfg(test)]
        pub(super) fn save_with_test_hook(
            &self,
            edited_text: &str,
            hook: impl FnMut(TestPoint) -> Result<(), TextEditError>,
        ) -> Result<(), TextEditError> {
            self.save_with_hook(edited_text, hook)
        }

        fn save_with_hook(
            &self,
            edited_text: &str,
            hook: impl FnMut(TestPoint) -> Result<(), TextEditError>,
        ) -> Result<(), TextEditError> {
            validate_editable_text(edited_text)?;
            let current =
                Snapshot::capture(&self.parent, &self.name).map_err(|_| TextEditError::Conflict)?;
            if !self.source.matches(&current) {
                return Err(TextEditError::Conflict);
            }
            if edited_text.as_bytes() == self.source.bytes {
                return Ok(());
            }

            let stage = Stage::create(&self.parent, edited_text.as_bytes(), &current.metadata)?;
            stage.publish(&self.parent, &self.name, &self.source, hook)
        }
    }

    impl Snapshot {
        fn capture(parent: &OwnedFd, name: &OsStr) -> Result<Self, TextEditError> {
            let named = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(read_error)?;
            if FileType::from_raw_mode(named.st_mode) == FileType::Symlink {
                return Err(TextEditError::Symlink);
            }
            if FileType::from_raw_mode(named.st_mode) != FileType::RegularFile {
                return Err(TextEditError::NotRegularFile);
            }

            let fd = openat(
                parent,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(read_error)?;
            let mut file = File::from(fd);
            let initial = PlainMetadata::classify(&file)?;
            if !initial.matches_stat(&named) {
                return Err(TextEditError::Conflict);
            }
            if initial.size > MAX_EDITABLE_TEXT_BYTES {
                return Err(TextEditError::TooLarge {
                    limit: MAX_EDITABLE_TEXT_BYTES,
                });
            }

            let mut bytes = Vec::with_capacity(initial.size as usize);
            Read::by_ref(&mut file)
                .take(MAX_EDITABLE_TEXT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(read_error)?;
            if bytes.len() as u64 > MAX_EDITABLE_TEXT_BYTES {
                return Err(TextEditError::TooLarge {
                    limit: MAX_EDITABLE_TEXT_BYTES,
                });
            }

            let final_metadata = PlainMetadata::classify(&file)?;
            let final_named =
                statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(read_error)?;
            if initial != final_metadata || !final_metadata.matches_stat(&final_named) {
                return Err(TextEditError::Conflict);
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| TextEditError::InvalidUtf8)?;
            validate_editable_text(text)?;

            Ok(Self {
                metadata: final_metadata,
                digest: Sha256::digest(&bytes).into(),
                bytes,
            })
        }

        fn matches(&self, current: &Self) -> bool {
            self.metadata == current.metadata
                && self.bytes.len() == current.bytes.len()
                && self.digest == current.digest
        }

        fn matches_displaced_after_exchange(&self, current: &Self) -> bool {
            // Exchange itself changes ctime. Every other admitted metadata field
            // and the inode/content identity must still match. Never delete this
            // entry: ctime-only races and later writes through open descriptors
            // cannot be excluded by a post-exchange observation.
            let mut expected = self.metadata;
            expected.changed = current.metadata.changed;
            expected == current.metadata
                && self.bytes.len() == current.bytes.len()
                && self.digest == current.digest
        }
    }

    impl PlainMetadata {
        fn classify(file: &File) -> Result<Self, TextEditError> {
            let stat = fstat(file).map_err(read_error)?;
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
                return Err(TextEditError::NotRegularFile);
            }
            if stat.st_nlink != 1 {
                return Err(TextEditError::HardLinked);
            }
            if stat.st_mode & 0o7000 != 0 {
                return Err(TextEditError::UnsupportedMetadata);
            }
            let size = u64::try_from(stat.st_size).map_err(|_| TextEditError::Conflict)?;
            let blocks =
                u64::try_from(stat.st_blocks).map_err(|_| TextEditError::UnsupportedMetadata)?;
            require_dense_logical_range(file, size)?;
            reject_inode_flags(file)?;
            let mut names = [];
            if flistxattr(file, &mut names).map_err(|_| TextEditError::UnsupportedMetadata)? != 0 {
                return Err(TextEditError::UnsupportedMetadata);
            }
            Ok(Self {
                identity: Identity {
                    device: stat.st_dev,
                    inode: stat.st_ino,
                },
                mode: stat.st_mode & 0o777,
                uid: stat.st_uid,
                gid: stat.st_gid,
                size,
                blocks,
                modified: (stat.st_mtime as i64, stat.st_mtime_nsec),
                changed: (stat.st_ctime as i64, stat.st_ctime_nsec),
            })
        }

        fn matches_stat(&self, stat: &rustix::fs::Stat) -> bool {
            self.identity
                == Identity {
                    device: stat.st_dev,
                    inode: stat.st_ino,
                }
        }
    }

    const FIEMAP_FLAG_SYNC: u32 = 0x0000_0001;
    pub(super) const FIEMAP_EXTENT_LAST: u32 = 0x0000_0001;
    #[cfg(test)]
    pub(super) const FIEMAP_EXTENT_UNKNOWN: u32 = 0x0000_0002;
    #[cfg(test)]
    pub(super) const FIEMAP_EXTENT_UNWRITTEN: u32 = 0x0000_0800;
    const FIEMAP_BATCH_EXTENTS: usize = 32;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Fiemap {
        start: u64,
        length: u64,
        flags: u32,
        mapped_extents: u32,
        extent_count: u32,
        _reserved: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(super) struct FiemapExtent {
        logical: u64,
        _physical: u64,
        length: u64,
        _reserved64: [u64; 2],
        flags: u32,
        _reserved: [u32; 3],
    }

    #[cfg(test)]
    impl FiemapExtent {
        pub(super) fn for_test(logical: u64, length: u64, flags: u32) -> Self {
            Self {
                logical,
                length,
                flags,
                ..Self::default()
            }
        }
    }

    #[repr(C)]
    struct FiemapRequest {
        map: Fiemap,
        extents: [FiemapExtent; FIEMAP_BATCH_EXTENTS],
    }

    fn require_dense_logical_range(file: &File, size: u64) -> Result<(), TextEditError> {
        if size == 0 {
            return Ok(());
        }

        let mut extents = Vec::new();
        let mut next_start = 0;
        loop {
            let page = fiemap_page(file, next_start, size - next_start)?;
            let last = page.last().ok_or(TextEditError::UnsupportedMetadata)?;
            let page_end = last
                .logical
                .checked_add(last.length)
                .ok_or(TextEditError::UnsupportedMetadata)?;
            if extents.len() + page.len() > (MAX_EDITABLE_TEXT_BYTES / 512) as usize {
                return Err(TextEditError::UnsupportedMetadata);
            }
            let page_is_last = last.flags & FIEMAP_EXTENT_LAST != 0;
            if page[..page.len() - 1]
                .iter()
                .any(|extent| extent.flags & FIEMAP_EXTENT_LAST != 0)
            {
                return Err(TextEditError::UnsupportedMetadata);
            }
            extents.extend_from_slice(&page);
            if page_is_last {
                break;
            }
            if page_end <= next_start || page_end >= size {
                return Err(TextEditError::UnsupportedMetadata);
            }
            next_start = page_end;
        }

        if fiemap_extents_cover_range(size, &extents) {
            Ok(())
        } else {
            Err(TextEditError::UnsupportedMetadata)
        }
    }

    fn fiemap_page(
        file: &File,
        start: u64,
        length: u64,
    ) -> Result<Vec<FiemapExtent>, TextEditError> {
        const IOC_WRITE: libc::c_ulong = 1;
        const IOC_READ: libc::c_ulong = 2;
        const IOC_SIZE_SHIFT: libc::c_ulong = 16;
        const IOC_DIR_SHIFT: libc::c_ulong = 30;
        const FS_IOC_FIEMAP: libc::c_ulong = ((IOC_READ | IOC_WRITE) << IOC_DIR_SHIFT)
            | ((std::mem::size_of::<Fiemap>() as libc::c_ulong) << IOC_SIZE_SHIFT)
            | ((b'f' as libc::c_ulong) << 8)
            | 11;

        let mut request = FiemapRequest {
            map: Fiemap {
                start,
                length,
                flags: FIEMAP_FLAG_SYNC,
                extent_count: FIEMAP_BATCH_EXTENTS as u32,
                ..Fiemap::default()
            },
            extents: [FiemapExtent::default(); FIEMAP_BATCH_EXTENTS],
        };
        // FIEMAP reads extent metadata into the request buffer without mutating the file.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_FIEMAP, &mut request.map) };
        let mapped = request.map.mapped_extents as usize;
        if result != 0
            || request.map.start != start
            || request.map.length != length
            || request.map.flags != FIEMAP_FLAG_SYNC
            || request.map.extent_count != FIEMAP_BATCH_EXTENTS as u32
            || mapped > FIEMAP_BATCH_EXTENTS
        {
            return Err(TextEditError::UnsupportedMetadata);
        }
        Ok(request.extents[..mapped].to_vec())
    }

    pub(super) fn fiemap_extents_cover_range(size: u64, extents: &[FiemapExtent]) -> bool {
        if size == 0 {
            return extents.is_empty();
        }

        let mut expected_start = 0;
        for (index, extent) in extents.iter().enumerate() {
            if extent.flags & !FIEMAP_EXTENT_LAST != 0 || extent.logical != expected_start {
                return false;
            }
            let Some(end) = extent.logical.checked_add(extent.length) else {
                return false;
            };
            if extent.length == 0 {
                return false;
            }
            let is_last = extent.flags & FIEMAP_EXTENT_LAST != 0;
            if is_last {
                return index + 1 == extents.len() && end >= size;
            }
            if end >= size {
                return false;
            }
            expected_start = end;
        }
        false
    }

    fn reject_inode_flags(file: &File) -> Result<(), TextEditError> {
        const IOC_READ: libc::c_ulong = 2;
        const IOC_SIZE_SHIFT: libc::c_ulong = 16;
        const IOC_DIR_SHIFT: libc::c_ulong = 30;
        const FS_IOC_GETFLAGS: libc::c_ulong = (IOC_READ << IOC_DIR_SHIFT)
            | ((std::mem::size_of::<libc::c_long>() as libc::c_ulong) << IOC_SIZE_SHIFT)
            | ((b'f' as libc::c_ulong) << 8)
            | 1;

        let mut flags: libc::c_long = 0;
        // FS_IOC_GETFLAGS reads the inode flags without changing filesystem state.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), FS_IOC_GETFLAGS, &mut flags) };
        // EXTENTS describes ext4's ordinary storage layout; it is not user
        // metadata requiring preservation. All behavioral inode flags still
        // fail closed (immutable, append-only, encryption, compression, etc.).
        const FS_EXTENT_FL: libc::c_long = 0x0008_0000;
        if result != 0 || flags & !FS_EXTENT_FL != 0 {
            return Err(TextEditError::UnsupportedMetadata);
        }
        Ok(())
    }

    struct Stage {
        directory: OwnedFd,
        file: File,
        source_mode: u32,
        recovery_path: std::path::PathBuf,
    }

    impl Stage {
        fn create(
            parent: &OwnedFd,
            contents: &[u8],
            source: &PlainMetadata,
        ) -> Result<Self, TextEditError> {
            for _ in 0..32 {
                let name = random_stage_name()?;
                match mkdirat(parent, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                    Ok(()) => {}
                    Err(Errno::EXIST) => continue,
                    Err(error) => return Err(write_error(error)),
                }
                let directory = match openat(
                    parent,
                    &name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(directory) => directory,
                    Err(error) => {
                        return Err(TextEditError::StageRetentionRequired(error.to_string()));
                    }
                };
                let opened = match fstat(&directory) {
                    Ok(stat) => stat,
                    Err(error) => {
                        return Err(TextEditError::StageRetentionRequired(error.to_string()));
                    }
                };
                let named = match statat(parent, &name, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(stat) => stat,
                    Err(error) => {
                        return Err(TextEditError::StageRetentionRequired(error.to_string()));
                    }
                };
                let private_directory = FileType::from_raw_mode(opened.st_mode)
                    == FileType::Directory
                    && opened.st_mode & 0o777 == 0o700
                    && opened.st_uid == rustix::process::geteuid().as_raw()
                    && opened.st_dev == named.st_dev
                    && opened.st_ino == named.st_ino;
                if !private_directory {
                    return Err(TextEditError::StageRetentionRequired(
                        "the private staging directory could not be verified".to_string(),
                    ));
                }
                let recovery_path =
                    std::fs::read_link(format!("/proc/self/fd/{}", directory.as_raw_fd()))
                        .map_err(|error| {
                            TextEditError::StageRetentionRequired(format!(
                                "cannot resolve private staging directory {name:?}: {error}"
                            ))
                        })?;
                let fd = match openat(
                    &directory,
                    STAGED_CANDIDATE,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::RUSR | Mode::WUSR,
                ) {
                    Ok(fd) => fd,
                    Err(error) => {
                        return Err(TextEditError::StageRetentionRequired(error.to_string()));
                    }
                };
                let mut file = File::from(fd);
                let result = (|| {
                    fchmod(&file, Mode::RUSR | Mode::WUSR).map_err(write_error)?;
                    file.write_all(contents).map_err(write_error)?;
                    file.sync_all().map_err(write_error)?;
                    let staged = PlainMetadata::classify(&file)?;
                    if staged.mode != 0o600 || staged.uid != source.uid || staged.gid != source.gid
                    {
                        return Err(TextEditError::UnsupportedMetadata);
                    }
                    // Independent durable draft: publication can immediately be
                    // followed by another writer replacing or modifying target.
                    let draft_fd = openat(
                        &directory,
                        "edits",
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC,
                        Mode::RUSR | Mode::WUSR,
                    )
                    .map_err(write_error)?;
                    let mut draft = File::from(draft_fd);
                    draft.write_all(contents).map_err(write_error)?;
                    draft.sync_all().map_err(write_error)?;
                    fsync(&directory).map_err(write_error)?;
                    fsync(parent).map_err(write_error)
                })();
                if let Err(error) = result {
                    return Err(TextEditError::StageRetentionRequired(format!(
                        "{error}; recovery directory: {recovery_path:?}"
                    )));
                }
                return Ok(Self {
                    directory,
                    file,
                    source_mode: source.mode,
                    recovery_path,
                });
            }
            Err(TextEditError::Write(
                "could not reserve a private staging file".to_string(),
            ))
        }

        fn publish(
            self,
            parent: &OwnedFd,
            target: &OsStr,
            source: &Snapshot,
            mut hook: impl FnMut(TestPoint) -> Result<(), TextEditError>,
        ) -> Result<(), TextEditError> {
            if let Err(error) = fchmod(&self.file, Mode::from_raw_mode(self.source_mode)) {
                return Err(self.retention_required(&error.to_string()));
            }
            if let Err(error) = self.file.sync_all() {
                return Err(self.retention_required(&error.to_string()));
            }
            if let Err(error) = fsync(&self.directory) {
                return Err(self.retention_required(&error.to_string()));
            }
            if let Err(error) = hook(TestPoint::BeforePublication) {
                return Err(self.retention_required(&error.to_string()));
            }
            let candidate = match Snapshot::capture(&self.directory, OsStr::new(STAGED_CANDIDATE)) {
                Ok(candidate) => candidate,
                Err(error) => return Err(self.retention_required(&error.to_string())),
            };
            // Revalidate after all staging I/O, immediately before publication.
            let current = Snapshot::capture(parent, target);
            if !matches!(current.as_ref(), Ok(current) if source.matches(current)) {
                return Err(self.retention_required(
                    "the source changed before publication; it was not replaced",
                ));
            }
            if let Err(error) = hook(TestPoint::AfterSourceVerification) {
                return Err(self.retention_required(&error.to_string()));
            }
            if let Err(error) = renameat_with(
                &self.directory,
                STAGED_CANDIDATE,
                parent,
                target,
                RenameFlags::EXCHANGE,
            ) {
                return Err(self.retention_required(&error.to_string()));
            }
            let observation_error = hook(TestPoint::AfterExchange).err();
            let displaced = Snapshot::capture(&self.directory, OsStr::new(STAGED_CANDIDATE));
            let displaced_matches_source = matches!(
                displaced.as_ref(),
                Ok(snapshot) if source.matches_displaced_after_exchange(snapshot)
            );
            let published = Snapshot::capture(parent, target);
            let publication_matches = matches!(published.as_ref(), Ok(snapshot) if candidate.matches_displaced_after_exchange(snapshot));
            let mut detail = String::new();
            if !displaced_matches_source || !publication_matches || observation_error.is_some() {
                // Linux has no inode-conditional exchange. Exchanging back after
                // a pathname check could displace a third writer. Only restore
                // into an absent name, atomically without replacement; keep the
                // recovery entry even when that restoration succeeds.
                let restored = hook(TestPoint::BeforeRecovery).and_then(|()| {
                    linkat(
                        &self.directory,
                        STAGED_CANDIDATE,
                        parent,
                        target,
                        AtFlags::empty(),
                    )
                    .map_err(write_error)
                });
                detail = match restored {
                    Ok(()) => "an external change was detected; its entry was restored to the vacant source name without replacement".into(),
                    Err(error) => format!("an external change was detected; automatic restoration could not safely complete ({error}); the source name may contain a newer version"),
                };
            }
            let durability = hook(TestPoint::BeforeDirectorySync).and_then(|()| {
                fsync(&self.directory).map_err(write_error)?;
                fsync(parent).map_err(write_error)
            });
            if !detail.is_empty() {
                if let Err(error) = durability {
                    detail.push_str(&format!(
                        "; directory durability could not be confirmed: {error}"
                    ));
                }
                return Err(TextEditError::RecoveryConflict {
                    recovery: self.location(),
                    detail,
                });
            }
            if let Err(error) = durability {
                return Err(TextEditError::DurabilityUncertain(format!(
                    "{error}; recovery directory: {:?}",
                    self.location()
                )));
            }
            // Retention is intentional even on success: an external writer may
            // still hold the displaced inode open. Automatic unlink would lose
            // that writer's subsequent data. No Drop cleanup owns these entries.
            Ok(())
        }

        fn location(&self) -> std::path::PathBuf {
            // Resolve the pinned directory, not the original replaceable parent
            // pathname. This also reports parent renames that occurred in save.
            let descriptor = format!("/proc/self/fd/{}", self.directory.as_raw_fd());
            std::fs::read_link(&descriptor).unwrap_or_else(|_| self.recovery_path.clone())
        }

        fn retention_required(self, reason: &str) -> TextEditError {
            TextEditError::StageRetentionRequired(format!(
                "{reason}; retained recovery directory: {:?}; no automatic cleanup was attempted",
                self.location()
            ))
        }
    }

    fn validate_editable_text(text: &str) -> Result<(), TextEditError> {
        if text.len() as u64 > MAX_EDITABLE_TEXT_BYTES {
            return Err(TextEditError::TooLarge {
                limit: MAX_EDITABLE_TEXT_BYTES,
            });
        }
        if text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
        {
            return Err(TextEditError::BinaryText);
        }
        Ok(())
    }

    fn random_stage_name() -> Result<OsString, TextEditError> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| TextEditError::Write(error.to_string()))?;
        let mut name = String::from(".ottrin-text-edit-");
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut name, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Ok(name.into())
    }

    fn read_error(error: impl std::fmt::Display) -> TextEditError {
        TextEditError::Read(error.to_string())
    }

    fn write_error(error: impl std::fmt::Display) -> TextEditError {
        TextEditError::Write(error.to_string())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::linux::{
        FIEMAP_EXTENT_LAST, FIEMAP_EXTENT_UNKNOWN, FIEMAP_EXTENT_UNWRITTEN, FiemapExtent,
        STAGED_CANDIDATE, fiemap_extents_cover_range,
    };
    use super::*;
    use std::fs::Permissions;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("ottrin-text-edit-{}-{id}", std::process::id()));
            std::fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn fiemap_fixture() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let executable = std::env::current_exe().expect("locate test executable");
            let target_dir = executable
                .parent()
                .and_then(|path| path.parent())
                .and_then(|path| path.parent())
                .expect("locate Cargo target directory");
            let path = target_dir.join(format!("ottrin-text-edit-{}-{id}", std::process::id()));
            std::fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("write fixture");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn text_edit_fixture_supported() -> bool {
        let root = TestDir::new();
        let path = root.file("support.txt", "ordinary text\n");
        TextEditSession::open(&path).is_ok()
    }

    #[test]
    fn unicode_text_can_be_saved_without_normalizing_line_endings() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let path = root.file("notes.txt", "Gr\u{00f6}na\r\nrad\r\n");

        let session = TextEditSession::open(&path).expect("open editable text");
        session
            .save("Gr\u{00f6}na\r\nbl\u{00e5}\r\n")
            .expect("save unicode text");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read saved text"),
            "Gr\u{00f6}na\r\nbl\u{00e5}\r\n"
        );
    }

    #[test]
    fn source_change_at_publication_is_refused() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("editable fixture");
        let result = session.save_with_test_hook("editor buffer\n", |_| {
            std::fs::write(&path, "external\n").unwrap();
            Ok(())
        });
        assert!(result.is_err(), "late external changes must refuse save");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external\n");
    }

    #[test]
    fn identical_content_replacement_and_restored_mtime_are_conflicts() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("editable fixture");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let replacement = root.file("replacement.txt", "original\n");
        std::fs::rename(replacement, &path).unwrap();
        assert_eq!(session.save("my edits\n"), Err(TextEditError::Conflict));
        let session = TextEditSession::open(&path).expect("new generation");
        std::fs::write(&path, "external\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert_eq!(session.save("my edits\n"), Err(TextEditError::Conflict));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external\n");
    }

    #[test]
    fn final_window_permission_change_is_retained_and_reported() {
        use std::os::unix::fs::PermissionsExt;
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).unwrap();
        let error = session
            .save_with_test_hook("draft\n", |point| {
                if point == linux::TestPoint::AfterSourceVerification {
                    std::fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
                }
                Ok(())
            })
            .expect_err("metadata-only race must not be called a clean save");
        let recovery = recovery_directory(&root);
        assert!(error.to_string().contains(recovery.to_str().unwrap()));
        assert_eq!(
            std::fs::read_to_string(recovery.join("edits")).unwrap(),
            "draft\n"
        );
        assert_eq!(
            std::fs::metadata(recovery.join(STAGED_CANDIDATE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
    }

    fn recovery_directory(root: &TestDir) -> PathBuf {
        std::fs::read_dir(&root.0)
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".ottrin-text-edit-")
            })
            .unwrap()
    }

    #[test]
    fn successful_save_retains_both_versions_for_late_descriptor_writers() {
        use std::io::{Seek, Write};
        use std::os::unix::fs::PermissionsExt;
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        std::fs::set_permissions(&path, Permissions::from_mode(0o640)).unwrap();
        let mut external = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        let session = TextEditSession::open(&path).unwrap();
        session.save("draft\n").unwrap();
        external.rewind().unwrap();
        external.write_all(b"external\n").unwrap();
        let recovery = recovery_directory(&root);
        assert_eq!(
            std::fs::read_to_string(recovery.join(STAGED_CANDIDATE)).unwrap(),
            "external\n"
        );
        assert_eq!(
            std::fs::read_to_string(recovery.join("edits")).unwrap(),
            "draft\n"
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn recovery_only_restores_a_vacant_name_and_never_overwrites_a_third_writer() {
        for (vacant, fail_restore, fail_sync) in [
            (true, false, false),
            (true, true, false),
            (false, false, true),
        ] {
            let root = TestDir::fiemap_fixture();
            let path = root.file("notes.txt", "original\n");
            let session = TextEditSession::open(&path).unwrap();
            let error = session
                .save_with_test_hook("draft\n", |point| {
                    match point {
                        linux::TestPoint::AfterSourceVerification => {
                            std::fs::write(&path, "external\n").unwrap()
                        }
                        linux::TestPoint::AfterExchange => {
                            std::fs::remove_file(&path).unwrap();
                            if !vacant {
                                std::fs::write(&path, "third writer\n").unwrap();
                            }
                        }
                        linux::TestPoint::BeforeRecovery if fail_restore => {
                            return Err(TextEditError::Write(
                                "injected restoration failure".into(),
                            ));
                        }
                        linux::TestPoint::BeforeDirectorySync if fail_sync => {
                            return Err(TextEditError::Write("injected sync failure".into()));
                        }
                        _ => {}
                    }
                    Ok(())
                })
                .unwrap_err();
            assert!(matches!(error, TextEditError::RecoveryConflict { .. }));
            if fail_sync {
                assert!(
                    error
                        .to_string()
                        .contains("durability could not be confirmed")
                );
            }
            if vacant && !fail_restore {
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
            } else if !vacant {
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "third writer\n");
            } else {
                assert!(!path.exists());
            }
            let recovery = recovery_directory(&root);
            assert_eq!(
                std::fs::read_to_string(recovery.join("edits")).unwrap(),
                "draft\n"
            );
            assert_eq!(
                std::fs::read_to_string(recovery.join(STAGED_CANDIDATE)).unwrap(),
                "external\n"
            );
        }
    }

    #[test]
    fn publication_followed_by_another_writer_is_a_recovery_conflict() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).unwrap();
        let error = session
            .save_with_test_hook("draft\n", |point| {
                if point == linux::TestPoint::AfterExchange {
                    std::fs::write(&path, "newer writer\n").unwrap();
                }
                Ok(())
            })
            .expect_err("a changed publication must not discard the editor");
        assert!(matches!(error, TextEditError::RecoveryConflict { .. }));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "newer writer\n");
        assert_eq!(
            std::fs::read_to_string(recovery_directory(&root).join("edits")).unwrap(),
            "draft\n"
        );
    }

    #[test]
    fn final_window_identical_bytes_replacement_preserves_inode_identity_check() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).unwrap();
        let error = session
            .save_with_test_hook("draft\n", |point| {
                if point == linux::TestPoint::AfterSourceVerification {
                    let replacement = root.file("replacement", "original\n");
                    std::fs::rename(replacement, &path).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(error, TextEditError::RecoveryConflict { .. }));
    }

    #[test]
    fn unsupported_exchange_retains_draft_without_touching_source() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).unwrap();
        let result = session.save_with_test_hook("draft\n", |point| {
            if point == linux::TestPoint::AfterSourceVerification {
                return Err(TextEditError::Write("operation not supported".into()));
            }
            Ok(())
        });
        assert!(matches!(
            result,
            Err(TextEditError::StageRetentionRequired(_))
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
        assert_eq!(
            std::fs::read_to_string(recovery_directory(&root).join("edits")).unwrap(),
            "draft\n"
        );
    }

    #[test]
    fn binary_controls_and_oversized_edits_are_not_admitted() {
        let root = TestDir::fiemap_fixture();
        let path = root.file("notes.txt", "ordinary\n");
        let session = TextEditSession::open(&path).expect("editable fixture");
        assert!(session.save("binary\0data").is_err());
        assert!(
            session
                .save(&"a".repeat(MAX_EDITABLE_TEXT_BYTES as usize + 1))
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "ordinary\n");
        std::fs::write(&path, "binary\0data").unwrap();
        assert!(TextEditSession::open(&path).is_err());
    }

    #[test]
    fn canceling_a_session_never_mutates_the_source() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");

        let session = TextEditSession::open(&path).expect("open editable text");
        assert_eq!(session.contents(), "original\n");
        drop(session);

        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    }

    #[test]
    fn changed_source_refuses_overwrite_and_keeps_the_callers_buffer() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        let unsaved_buffer = "my unsaved edit\n".to_string();
        std::fs::write(&path, "external update\n").expect("change source");

        assert_eq!(session.save(&unsaved_buffer), Err(TextEditError::Conflict));
        assert_eq!(unsaved_buffer, "my unsaved edit\n");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external update\n");
    }

    #[test]
    fn missing_source_is_a_conflict_not_a_new_file_save() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        std::fs::remove_file(&path).expect("remove source");

        assert_eq!(
            session.save("my unsaved edit\n"),
            Err(TextEditError::Conflict)
        );
        assert!(!path.exists());
    }

    #[test]
    fn failed_publication_keeps_the_source_and_callers_buffer() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        let unsaved_buffer = "my unsaved edit\n".to_string();

        let result = session.save_with_test_hook(&unsaved_buffer, |_| {
            Err(TextEditError::Write("forced test failure".to_string()))
        });

        let error = result.expect_err("replaced stage must not be removed");
        assert!(matches!(error, TextEditError::StageRetentionRequired(_)));
        assert!(
            error.to_string().contains(".ottrin-text-edit-"),
            "retention error must identify the recovery location"
        );
        assert_eq!(unsaved_buffer, "my unsaved edit\n");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    }

    #[test]
    fn parent_path_replacement_cannot_redirect_publication() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let parent = root.0.join("parent");
        let moved_parent = root.0.join("moved-parent");
        std::fs::create_dir(&parent).expect("create source parent");
        let path = parent.join("notes.txt");
        std::fs::write(&path, "original\n").expect("write source text");
        let session = TextEditSession::open(&path).expect("open editable text");

        session
            .save_with_test_hook("edited\n", |point| {
                if point != linux::TestPoint::BeforePublication {
                    return Ok(());
                }
                std::fs::rename(&parent, &moved_parent).expect("move validated parent");
                std::fs::create_dir(&parent).expect("replace parent path");
                std::fs::write(parent.join("notes.txt"), "unrelated\n")
                    .expect("write unrelated replacement");
                Ok(())
            })
            .expect("publish through the pinned parent");

        assert_eq!(
            std::fs::read_to_string(moved_parent.join("notes.txt")).unwrap(),
            "edited\n"
        );
        assert_eq!(
            std::fs::read_to_string(parent.join("notes.txt")).unwrap(),
            "unrelated\n"
        );
    }

    #[test]
    fn replaced_stage_name_is_never_unlinked_during_failed_save_cleanup() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        let retained_stage = root.0.join("retained-stage");

        let result = session.save_with_test_hook("edited\n", |_| {
            let stage = std::fs::read_dir(&root.0)
                .expect("read stage directory")
                .map(Result::unwrap)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with(".ottrin-text-edit-"))
                })
                .expect("find private stage");
            std::fs::rename(&stage, &retained_stage).expect("move private stage");
            std::fs::write(&stage, "attacker replacement\n").expect("replace private stage name");
            Err(TextEditError::Write("force failed publication".to_string()))
        });

        assert!(matches!(
            result,
            Err(TextEditError::StageRetentionRequired(_))
        ));
        assert_eq!(
            std::fs::read_to_string(retained_stage.join(STAGED_CANDIDATE)).unwrap(),
            "edited\n"
        );
        assert_eq!(
            std::fs::metadata(&retained_stage)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let replacement = std::fs::read_dir(&root.0)
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".ottrin-text-edit-"))
            })
            .expect("attacker replacement remains");
        assert_eq!(
            std::fs::read_to_string(replacement).unwrap(),
            "attacker replacement\n"
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    }

    #[test]
    fn late_source_replacement_is_a_conflict_and_retains_private_staging() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        std::fs::set_permissions(&path, Permissions::from_mode(0o644))
            .expect("set ordinary source mode");
        let session = TextEditSession::open(&path).expect("open editable text");

        let result = session.save_with_test_hook("edited\n", |_| {
            std::fs::remove_file(&path).expect("remove source before rename");
            std::fs::create_dir(&path).expect("make rename target incompatible");
            Ok(())
        });

        assert!(matches!(
            result,
            Err(TextEditError::StageRetentionRequired(_))
        ));
        let stage = std::fs::read_dir(&root.0)
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".ottrin-text-edit-"))
            })
            .expect("private stage directory remains");
        assert_eq!(
            std::fs::metadata(&stage).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(stage.join(STAGED_CANDIDATE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }

    #[test]
    fn source_replaced_after_final_check_publishes_with_a_recovery_conflict() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }

        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");

        let error = session
            .save_with_test_hook("edited\n", |point| {
                if matches!(point, super::linux::TestPoint::AfterSourceVerification) {
                    std::fs::write(&path, "external edit\n")
                        .expect("replace source after final verification");
                }
                Ok(())
            })
            .expect_err("the displaced external source must be reported for recovery");

        if !matches!(error, TextEditError::RecoveryConflict { .. }) {
            panic!("expected a committed recovery conflict, got {error:?}");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited\n");
        let recovery = std::fs::read_dir(&root.0)
            .unwrap()
            .map(Result::unwrap)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".ottrin-text-edit-"))
            })
            .expect("the displaced external source must be retained privately");
        assert_eq!(
            std::fs::read_to_string(recovery.join(STAGED_CANDIDATE)).unwrap(),
            "external edit\n"
        );
    }

    #[test]
    fn invalid_or_incomplete_sources_remain_read_only() {
        if !text_edit_fixture_supported() {
            eprintln!("skipping text-edit lifecycle fixture: FIEMAP is unavailable");
            return;
        }
        let root = TestDir::new();
        let invalid = root.0.join("invalid.txt");
        std::fs::write(&invalid, [0xff, 0xfe]).expect("write invalid UTF-8");
        assert!(matches!(
            TextEditSession::open(&invalid),
            Err(TextEditError::InvalidUtf8)
        ));

        let oversized = root.0.join("large.txt");
        std::fs::write(&oversized, vec![b'x'; MAX_EDITABLE_TEXT_BYTES as usize + 1])
            .expect("write large text");
        assert!(matches!(
            TextEditSession::open(&oversized),
            Err(TextEditError::TooLarge {
                limit: MAX_EDITABLE_TEXT_BYTES
            })
        ));
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn symbolic_links_remain_read_only() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new();
        let target = root.file("target.txt", "target\n");
        let link = root.0.join("link.txt");
        symlink(&target, &link).expect("create text link");

        assert!(matches!(
            TextEditSession::open(&link),
            Err(TextEditError::Symlink)
        ));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "target\n");
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn hard_linked_text_remains_read_only() {
        let root = TestDir::new();
        let target = root.file("target.txt", "target\n");
        let alias = root.0.join("alias.txt");
        std::fs::hard_link(&target, &alias).expect("create hard link");

        assert!(matches!(
            TextEditSession::open(&target),
            Err(TextEditError::HardLinked)
        ));
        assert_eq!(std::fs::read_to_string(alias).unwrap(), "target\n");
    }

    #[test]
    fn fiemap_extent_coverage_rejects_ambiguous_or_incomplete_layouts() {
        assert!(fiemap_extents_cover_range(
            8192,
            &[
                FiemapExtent::for_test(0, 4096, 0),
                FiemapExtent::for_test(4096, 4096, FIEMAP_EXTENT_LAST),
            ],
        ));

        assert!(!fiemap_extents_cover_range(
            8192,
            &[
                FiemapExtent::for_test(0, 4096, 0),
                FiemapExtent::for_test(6144, 2048, FIEMAP_EXTENT_LAST),
            ],
        ));
        assert!(!fiemap_extents_cover_range(
            8192,
            &[FiemapExtent::for_test(
                0,
                8192,
                FIEMAP_EXTENT_UNWRITTEN | FIEMAP_EXTENT_LAST,
            )],
        ));
        assert!(!fiemap_extents_cover_range(
            8192,
            &[FiemapExtent::for_test(
                0,
                8192,
                FIEMAP_EXTENT_UNKNOWN | FIEMAP_EXTENT_LAST,
            )],
        ));
        assert!(!fiemap_extents_cover_range(
            8192,
            &[FiemapExtent::for_test(0, 8192, 0)],
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sparse_text_with_preallocation_past_eof_remains_read_only() {
        use rustix::fs::{FallocateFlags, SeekFrom, fallocate, seek};
        use rustix::io::Errno;
        use std::io::{Seek, SeekFrom as IoSeekFrom, Write};
        use std::os::unix::fs::MetadataExt;

        let root = TestDir::fiemap_fixture();
        let path = root.0.join("sparse.txt");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create sparse fixture");
        file.seek(IoSeekFrom::Start(4096))
            .expect("seek to second logical page");
        file.write_all(&[b'x'; 4096])
            .expect("write second logical page");
        file.sync_all().expect("sync sparse fixture");

        let allocation = fallocate(&file, FallocateFlags::KEEP_SIZE, 8192, 8192);
        if let Err(error @ (Errno::OPNOTSUPP | Errno::NOSYS | Errno::INVAL)) = allocation {
            eprintln!("skipping sparse fixture: fallocate is unsupported: {error}");
            return;
        }
        allocation.expect("preallocate beyond EOF");

        let data = seek(&file, SeekFrom::Data(0));
        let hole = seek(&file, SeekFrom::Hole(4096));
        if matches!(data, Err(Errno::OPNOTSUPP | Errno::NOSYS | Errno::INVAL))
            || matches!(hole, Err(Errno::OPNOTSUPP | Errno::NOSYS | Errno::INVAL))
        {
            eprintln!(
                "skipping sparse fixture: extent queries are unsupported: {data:?}, {hole:?}"
            );
            return;
        }
        assert_eq!(data.expect("find data after first-page hole"), 4096);
        assert_eq!(hole.expect("find hole after second-page data"), 8192);

        let metadata = file.metadata().expect("stat sparse fixture");
        assert!(
            metadata.blocks().saturating_mul(512) >= metadata.len(),
            "the previous st_blocks admission heuristic must accept this fixture"
        );
        drop(file);

        assert!(matches!(
            TextEditSession::open(&path),
            Err(TextEditError::UnsupportedMetadata)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn xattr_bearing_text_remains_read_only_when_supported_by_the_fixture() {
        use rustix::fs::{XattrFlags, fsetxattr};

        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let file = std::fs::File::open(&path).expect("open xattr fixture");
        if fsetxattr(
            &file,
            "user.ottrin-text-edit-test",
            b"present",
            XattrFlags::empty(),
        )
        .is_err()
        {
            return;
        }

        assert!(matches!(
            TextEditSession::open(&path),
            Err(TextEditError::UnsupportedMetadata)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn special_mode_text_remains_read_only_when_supported_by_the_fixture() {
        use std::os::unix::fs::PermissionsExt;

        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        std::fs::set_permissions(&path, Permissions::from_mode(0o4755))
            .expect("set setuid fixture mode");
        if std::fs::metadata(&path).unwrap().permissions().mode() & 0o7000 == 0 {
            return;
        }

        assert!(matches!(
            TextEditSession::open(&path),
            Err(TextEditError::UnsupportedMetadata)
        ));
    }
}
