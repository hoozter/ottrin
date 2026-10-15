//! Worker-scoped invalidation of a fully verified finalization namespace.
use super::*;
use rustix::fs::inotify::{self, ReadFlags, WatchFlags};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

#[derive(Default)]
struct Directory {
    all_names: bool,
    names: BTreeSet<Vec<u8>>,
    retired: bool,
}

pub(super) struct NamespaceGuard {
    directories: BTreeMap<i32, Directory>,
    directory_ids: BTreeMap<(u64, u64), i32>,
    files: BTreeMap<(u64, u64), i32>,
    file_wds: BTreeSet<i32>,
    retired_files: BTreeSet<i32>,
    expected_deletes: BTreeSet<(i32, Vec<u8>)>,
    mounts: std::fs::File,
    namespace: (u64, u64),
}

impl NamespaceGuard {
    pub(super) fn new(
        reads: &mut MetadataReads,
        root: &Path,
        registry: &OwnedFd,
        manifest_name: &str,
        entries: &BTreeMap<String, Entry>,
    ) -> Result<Self, PlatformError> {
        let mounts = std::fs::File::open("/proc/thread-self/mounts").map_err(journal_error)?;
        let ns = std::fs::metadata("/proc/thread-self/ns/mnt").map_err(journal_error)?;
        let mut guard = Self {
            directories: BTreeMap::new(),
            directory_ids: BTreeMap::new(),
            files: BTreeMap::new(),
            file_wds: BTreeSet::new(),
            retired_files: BTreeSet::new(),
            expected_deletes: BTreeSet::new(),
            mounts,
            namespace: (ns.dev(), ns.ino()),
        };
        reads.events = Some(
            inotify::init(inotify::CreateFlags::NONBLOCK | inotify::CreateFlags::CLOEXEC)
                .map_err(journal_error)?,
        );
        guard.watch_chain(reads, root)?;
        guard.watch_directory(reads, root, true)?;
        let mut directory = rustix::fs::Dir::read_from(registry).map_err(journal_error)?;
        while let Some(entry) = directory.read() {
            let entry = entry.map_err(journal_error)?;
            let name = entry.file_name();
            if name.to_bytes().ends_with(b".journal") || name.to_bytes() == manifest_name.as_bytes()
            {
                guard.watch_file(reads, registry, OsStr::from_bytes(name.to_bytes()))?;
            }
        }
        for entry in entries.values() {
            let stage = entry.record.transaction.decode().map_err(journal_error)?;
            let parent = stage.parent().expect("validated stage parent");
            guard.watch_chain(reads, parent)?;
            let wd = guard.watch_directory(reads, parent, false)?;
            guard.directories.get_mut(&wd).unwrap().names.insert(
                stage
                    .file_name()
                    .expect("validated stage name")
                    .as_bytes()
                    .to_vec(),
            );
            if exists(&stage)? {
                guard.watch_directory(reads, &stage, true)?;
                let fd = rustix::fs::open(
                    &stage,
                    OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(journal_error)?;
                for name in ["copy-journal", LEDGER] {
                    if exists(&path_from_fd(&fd).join(name))? {
                        guard.watch_file(reads, &fd, OsStr::new(name))?;
                    }
                }
            }
        }
        guard.check_mounts()?;
        Ok(guard)
    }

    fn watch_chain(&mut self, reads: &mut MetadataReads, path: &Path) -> Result<(), PlatformError> {
        if !path.is_absolute() {
            return Err(refusal("Finalization notification path is not absolute"));
        }
        let mut prefix = PathBuf::from("/");
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    let wd = self.watch_directory(reads, &prefix, false)?;
                    self.directories
                        .get_mut(&wd)
                        .unwrap()
                        .names
                        .insert(name.as_bytes().to_vec());
                    prefix.push(name);
                }
                _ => return Err(refusal("Finalization notification path is not normalized")),
            }
        }
        // Refuse the optimization for symlink ancestors. Watching only their
        // lexical parents misses rebinding inside an indirect target's ancestry.
        self.watch_directory(reads, &prefix, false)?;
        Ok(())
    }

    fn watch_directory(
        &mut self,
        reads: &mut MetadataReads,
        path: &Path,
        all_names: bool,
    ) -> Result<i32, PlatformError> {
        let fd = rustix::fs::open(
            path,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        if !MetadataReads::notifications_supported(&fd) {
            return Err(refusal(
                "Finalization directory notifications are unavailable",
            ));
        }
        let stat = rustix::fs::fstat(&fd).map_err(journal_error)?;
        let id = (stat.st_dev, stat.st_ino);
        let wd = if let Some(wd) = self.directory_ids.get(&id) {
            *wd
        } else {
            self.check_capacity()?;
            let mask = WatchFlags::CREATE
                | WatchFlags::DELETE
                | WatchFlags::MOVED_FROM
                | WatchFlags::MOVED_TO
                | WatchFlags::MODIFY
                | WatchFlags::ATTRIB
                | WatchFlags::DELETE_SELF
                | WatchFlags::MOVE_SELF
                | WatchFlags::ONLYDIR;
            let wd = inotify::add_watch(reads.events.as_ref().unwrap(), path_from_fd(&fd), mask)
                .map_err(journal_error)?;
            self.directory_ids.insert(id, wd);
            wd
        };
        self.directories.entry(wd).or_default().all_names |= all_names;
        Ok(wd)
    }

    fn watch_file(
        &mut self,
        reads: &mut MetadataReads,
        directory: &OwnedFd,
        name: &OsStr,
    ) -> Result<(), PlatformError> {
        let fd = rustix::fs::openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(journal_error)?;
        let file = std::fs::File::from(fd);
        let stat = rustix::fs::fstat(&file).map_err(journal_error)?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
            return Err(refusal("Finalization notification target is not a file"));
        }
        self.check_capacity()?;
        let wd = reads
            .watch(&file)
            .ok_or_else(|| refusal("Finalization file notifications are unavailable"))?;
        self.files.insert((stat.st_dev, stat.st_ino), wd);
        self.file_wds.insert(wd);
        Ok(())
    }

    fn check_capacity(&self) -> Result<(), PlatformError> {
        if self.directories.len() + self.files.len() >= 8192 {
            return Err(refusal("Finalization notification capacity reached"));
        }
        Ok(())
    }

    pub(super) fn check_mounts(&self) -> Result<(), PlatformError> {
        let ns = std::fs::metadata("/proc/thread-self/ns/mnt").map_err(journal_error)?;
        let mut poll = libc::pollfd {
            fd: self.mounts.as_raw_fd(),
            events: libc::POLLPRI | libc::POLLERR,
            revents: 0,
        };
        // SAFETY: poll points to one initialized descriptor and never blocks.
        let ready = unsafe { libc::poll(&mut poll, 1, 0) };
        if ready < 0 {
            return Err(journal_error(std::io::Error::last_os_error()));
        }
        if ready != 0 || self.namespace != (ns.dev(), ns.ino()) {
            return Err(refusal("Copy finalization mount namespace changed"));
        }
        Ok(())
    }

    /// Returns true for an event governed by this namespace proof.
    pub(super) fn event(
        &mut self,
        wd: i32,
        flags: ReadFlags,
        name: Option<&[u8]>,
    ) -> Result<bool, PlatformError> {
        let retirement = ReadFlags::ATTRIB | ReadFlags::DELETE_SELF | ReadFlags::IGNORED;
        if let Some(directory) = self.directories.get(&wd) {
            if directory.retired && name.is_none() && (flags & !retirement).is_empty() {
                return Ok(true);
            }
            if let Some(name) = name {
                if !directory.all_names && !directory.names.contains(name) {
                    return Ok(true);
                }
                let deletion = ReadFlags::DELETE | ReadFlags::ISDIR;
                if flags.contains(ReadFlags::DELETE)
                    && (flags & !deletion).is_empty()
                    && self.expected_deletes.remove(&(wd, name.to_vec()))
                {
                    return Ok(true);
                }
            }
            return Err(refusal(
                "Copy finalization directory changed; retry after review",
            ));
        }
        if self.file_wds.contains(&wd) {
            if self.retired_files.contains(&wd) && (flags & !retirement).is_empty() {
                return Ok(true);
            }
            return Err(refusal(
                "Copy finalization metadata changed; retry after review",
            ));
        }
        Ok(false)
    }

    pub(super) fn removed_file(
        &mut self,
        directory: &impl rustix::fd::AsFd,
        name: &str,
        file: &PinnedFile,
    ) -> Result<(), PlatformError> {
        self.expect_delete(directory, OsStr::new(name))?;
        if let Some(wd) = self.files.get(&(file.stamp.device, file.stamp.inode)) {
            self.retired_files.insert(*wd);
        }
        Ok(())
    }

    pub(super) fn removed_stage(
        &mut self,
        parent: &impl rustix::fd::AsFd,
        name: &OsStr,
        stage: &impl rustix::fd::AsFd,
    ) -> Result<(), PlatformError> {
        self.expect_delete(parent, name)?;
        let stat = rustix::fs::fstat(stage).map_err(journal_error)?;
        let wd = self
            .directory_ids
            .get(&(stat.st_dev, stat.st_ino))
            .ok_or_else(|| refusal("Finalization stage watch is missing"))?;
        self.directories.get_mut(wd).unwrap().retired = true;
        Ok(())
    }

    fn expect_delete(
        &mut self,
        directory: &impl rustix::fd::AsFd,
        name: &OsStr,
    ) -> Result<(), PlatformError> {
        let stat = rustix::fs::fstat(directory).map_err(journal_error)?;
        let wd = self
            .directory_ids
            .get(&(stat.st_dev, stat.st_ino))
            .ok_or_else(|| refusal("Finalization parent watch is missing"))?;
        self.expected_deletes
            .insert((*wd, name.as_bytes().to_vec()));
        Ok(())
    }
}
