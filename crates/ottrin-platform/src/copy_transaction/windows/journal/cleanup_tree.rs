//! Held remaining-tree evidence and narrowly scoped cleanup capabilities.
use super::cleanup_protocol::{CleanupRecord, Owner, Progress};
use super::*;
use std::os::windows::io::AsRawHandle;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    FILE_DISPOSITION_INFO_EX, FileDispositionInfoEx, SYNCHRONIZE, SetFileInformationByHandle,
};

fn entry(parent: &File, name: &[u16], access: u32) -> std::io::Result<File> {
    unsafe {
        windows_fs::open_relative(
            parent,
            &OsString::from_wide(name),
            RelativeOptions {
                directory: None,
                create_new: false,
                access,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    }
}

pub(super) fn dispose(file: &File) -> Result<(), PlatformError> {
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE
            | FILE_DISPOSITION_FLAG_FORCE_IMAGE_SECTION_CHECK,
    };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    } == 0
    {
        return Err(io(format!(
            "Copy cleanup delete disposition: {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

pub(super) fn absent(parent: &File, name: &OsStr) -> Result<(), PlatformError> {
    match entry(
        parent,
        &name.encode_wide().collect::<Vec<_>>(),
        FILE_GENERIC_READ,
    ) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io(error)),
        Ok(_) => Err(io("Copy cleanup slot has an unexpected occupant")),
    }
}

pub(super) struct Remaining {
    handles: Vec<Option<File>>,
    pub(super) observed: Vec<Option<EntryVersion>>,
    present: Vec<bool>,
    pub(super) pending_absent: bool,
    writable: BTreeSet<usize>,
    peer_change: Option<WindowsStableIdentity>,
}

impl Remaining {
    fn parent<'a>(&'a self, parent: &'a File, progress: &Progress, index: usize) -> &'a File {
        progress.manifest.entries[index]
            .parent
            .map(|p| self.handles[p].as_ref().unwrap())
            .unwrap_or(parent)
    }

    pub(super) fn open(
        parent: &File,
        state: &State,
        progress: &Progress,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::open_adjusted(parent, state, progress, None, &mut cancel)
    }

    /// Read capabilities only: the peer observer must certify its recorded
    /// absence before this permits one link-count/change-time adjustment.
    pub(super) fn after_peer_unlink(
        parent: &File,
        state: &State,
        observation: &super::cleanup_peer::Observation,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let identity = observation.removed_identity(state)?;
        Self::open_adjusted(
            parent,
            state,
            state.cleanup.as_ref().unwrap(),
            Some(identity),
            cancel,
        )
    }

    fn open_adjusted(
        parent: &File,
        state: &State,
        progress: &Progress,
        peer_change: Option<WindowsStableIdentity>,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let manifest = &progress.manifest;
        let mut present = vec![true; manifest.entries.len()];
        for &index in &manifest.order[..progress.completed] {
            present[index] = false;
        }
        let pending = progress.pending.then(|| manifest.order[progress.completed]);
        let mut result = Self {
            handles: (0..present.len()).map(|_| None).collect(),
            observed: vec![None; present.len()],
            present,
            pending_absent: false,
            writable: BTreeSet::new(),
            peer_change,
        };
        for index in 0..manifest.entries.len() {
            cancel(0)?;
            if !result.present[index] {
                continue;
            }
            match entry(
                result.parent(parent, progress, index),
                &manifest.entries[index].name,
                FILE_GENERIC_READ,
            ) {
                Ok(file) => result.handles[index] = Some(file),
                Err(error)
                    if Some(index) == pending && error.kind() == std::io::ErrorKind::NotFound =>
                {
                    result.present[index] = false;
                    result.pending_absent = true;
                }
                Err(error) => return Err(io(error)),
            }
        }
        for index in 0..manifest.entries.len() {
            if result.present[index] {
                result.observed[index] =
                    Some(result.validate_entry(state, progress, index, true, &mut cancel)?);
            }
        }
        result.revalidate(parent, state, progress, &mut cancel)?;
        Ok(result)
    }

    fn expected_version(
        &self,
        progress: &Progress,
        index: usize,
        actual: &EntryVersion,
    ) -> EntryVersion {
        let mut expected = progress.expected(index).clone();
        if self.pending_absent {
            let removed = &progress.manifest.entries[progress.manifest.order[progress.completed]];
            if removed.parent == Some(index) {
                expected.modified = actual.modified;
                expected.changed = actual.changed;
            }
            if removed.version.identity == actual.identity {
                expected.changed = actual.changed;
            }
        }
        if self.peer_change == Some(actual.identity) {
            expected.changed = actual.changed;
        }
        expected
    }

    fn validate_entry(
        &self,
        state: &State,
        progress: &Progress,
        index: usize,
        payload: bool,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<EntryVersion, PlatformError> {
        cancel(0)?;
        let file = self.handles[index].as_ref().unwrap();
        let actual = super::super::version(file)?;
        let mut links = progress.expected_links(index)?;
        if self.pending_absent
            && progress.manifest.entries[progress.manifest.order[progress.completed]]
                .version
                .identity
                == actual.identity
        {
            links = links
                .checked_sub(1)
                .ok_or_else(|| io("Copy cleanup pending alias underflow"))?;
        }
        if self.peer_change == Some(actual.identity) {
            links = links
                .checked_sub(1)
                .ok_or_else(|| io("Copy cleanup peer alias underflow"))?;
        }
        if actual != self.expected_version(progress, index, &actual)
            || namespace::link_count(file)? != links
        {
            return Err(io("Copy cleanup retained version or aliases changed"));
        }
        let (streams, compression) = match progress.manifest.owner {
            Owner::Copy => {
                let evidence = state.nodes[index].staged.as_ref().unwrap();
                state.planned.as_ref().unwrap().validate_entry_security(
                    index,
                    file,
                    state.permissions,
                )?;
                (&evidence.streams, evidence.compression)
            }
            Owner::Original => {
                let evidence = &state.original.as_ref().unwrap().evidence.entries[index];
                if security::owner_group_dacl_digest(file)? != evidence.owner_group_dacl {
                    return Err(io("Copy cleanup original permissions changed"));
                }
                (&evidence.streams, evidence.compression)
            }
            Owner::None => return Err(io("Copy cleanup has no payload authority")),
        };
        if metadata::compression(file)? != compression
            || payload && super::super::transfer(file, None, &mut *cancel)? != *streams
            || super::super::version(file)? != actual
        {
            return Err(io("Copy cleanup retained payload changed"));
        }
        Ok(actual)
    }

    pub(super) fn revalidate(
        &self,
        parent: &File,
        state: &State,
        progress: &Progress,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if !progress.manifest.entries.is_empty() && !self.present[0] {
            // Recheck absence on every observation, not just initial admission.
            absent(
                parent,
                &OsString::from_wide(&progress.manifest.entries[0].name),
            )?;
        }
        let mut children = vec![Vec::new(); self.present.len()];
        for (index, node) in progress.manifest.entries.iter().enumerate() {
            if self.present[index]
                && let Some(p) = node.parent
            {
                children[p].push(OsString::from_wide(&node.name));
            }
        }
        for names in &mut children {
            names.sort_unstable_by(|a, b| a.encode_wide().cmp(b.encode_wide()));
        }
        for (index, observed) in self.observed.iter().enumerate() {
            if let Some(observed) = observed {
                cancel(0)?;
                let node = &progress.manifest.entries[index];
                let file = self.handles[index].as_ref().unwrap();
                bound(
                    self.parent(parent, progress, index),
                    &OsString::from_wide(&node.name),
                    observed.identity,
                )?;
                if self.validate_entry(state, progress, index, false, cancel)? != *observed {
                    return Err(io("Copy cleanup observation epoch changed"));
                }
                if observed.directory
                    && observed.link.is_none()
                    && windows_fs::directory_children_limited(
                        file,
                        children[index].len().saturating_add(1),
                    )
                    .map_err(io)?
                        != children[index]
                {
                    return Err(io(
                        "Copy cleanup directory contains changed or unexpected entries",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn acquire(
        &mut self,
        parent: &File,
        state: &State,
        progress: &Progress,
        index: usize,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.peer_change.is_some() {
            return Err(io(
                "Peer acknowledgement observation grants no removal capability",
            ));
        }
        if let Some(p) = progress.manifest.entries[index].parent
            && !self.writable.contains(&p)
        {
            self.handles[p] = None;
            self.handles[p] = Some(
                entry(
                    self.parent(parent, progress, p),
                    &progress.manifest.entries[p].name,
                    // open_relative uses synchronous native I/O; MAXIMUM_ALLOWED
                    // alone does not satisfy its explicit SYNCHRONIZE contract.
                    windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED | SYNCHRONIZE,
                )
                .map_err(|error| io(format!("Copy cleanup parent capability: {error}")))?,
            );
            self.writable.insert(p);
        }
        if self.present[index] {
            for &other in progress.manifest.same_object(index) {
                self.handles[other] = None;
            }
            let file = entry(
                self.parent(parent, progress, index),
                &progress.manifest.entries[index].name,
                FILE_GENERIC_READ | DELETE,
            )
            .map_err(|error| io(format!("Copy cleanup delete capability: {error}")))?;
            for &other in progress.manifest.same_object(index) {
                if self.present[other] {
                    self.handles[other] = Some(file.try_clone().map_err(io)?);
                }
            }
        }
        // Flushability is an admission condition, never discovered after an
        // avoidable first unlink. Reopened capabilities must preserve the epoch.
        self.parent(parent, progress, index)
            .sync_all()
            .map_err(|error| io(format!("Copy cleanup parent preflight flush: {error}")))?;
        if self.present[index]
            && self.validate_entry(state, progress, index, true, cancel)?
                != *self.observed[index].as_ref().unwrap()
        {
            return Err(io("Copy cleanup entry changed during capability handoff"));
        }
        self.revalidate_path(parent, state, progress, index, cancel)
    }

    pub(super) fn revalidate_path(
        &self,
        parent: &File,
        state: &State,
        progress: &Progress,
        index: usize,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let mut current = Some(index);
        while let Some(index) = current {
            if let Some(observed) = &self.observed[index] {
                let node = &progress.manifest.entries[index];
                bound(
                    self.parent(parent, progress, index),
                    &OsString::from_wide(&node.name),
                    observed.identity,
                )?;
                if self.validate_entry(state, progress, index, false, cancel)? != *observed {
                    return Err(io("Copy cleanup mutation epoch changed"));
                }
            }
            current = progress.manifest.entries[index].parent;
        }
        if self.present[index] {
            let observed = self.observed[index].as_ref().unwrap();
            if observed.directory
                && observed.link.is_none()
                && !windows_fs::directory_children_limited(self.handles[index].as_ref().unwrap(), 1)
                    .map_err(io)?
                    .is_empty()
            {
                return Err(io(
                    "Copy cleanup directory is not empty after its child acknowledgements",
                ));
            }
        }
        Ok(())
    }

    pub(super) fn remove(
        &mut self,
        parent: &File,
        state: &State,
        progress: &Progress,
        index: usize,
        sequence: u64,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<CleanupRecord, PlatformError> {
        if self.peer_change.is_some() {
            return Err(io(
                "Peer acknowledgement observation grants no removal capability",
            ));
        }
        if self.present[index] {
            let file = self.handles[index].as_ref().unwrap();
            dispose(file)?;
            boundary(sequence, Step::CleanupDispositionSet)?;
            for &other in progress.manifest.same_object(index) {
                self.handles[other] = None;
            }
            boundary(sequence, Step::CleanupHandleClosed)?;
            absent(
                self.parent(parent, progress, index),
                &OsString::from_wide(&progress.manifest.entries[index].name),
            )?;
            self.present[index] = false;
            self.observed[index] = None;
            self.pending_absent = true;
        }
        self.parent(parent, progress, index)
            .sync_all()
            .map_err(|error| io(format!("Copy cleanup parent unlink flush: {error}")))?;
        boundary(sequence, Step::CleanupFlushed)?;
        // Only this pending unlink may advance its parent and surviving aliases.
        let identity = progress.manifest.entries[index].version.identity;
        let mut touched = BTreeSet::new();
        touched.extend(progress.manifest.entries[index].parent);
        touched.extend(progress.manifest.same_object(index).iter().copied());
        let mut checked_payload = false;
        for other in touched {
            if !self.present[other] {
                continue;
            }
            if self.handles[other].is_none() {
                self.handles[other] = Some(
                    entry(
                        self.parent(parent, progress, other),
                        &progress.manifest.entries[other].name,
                        FILE_GENERIC_READ,
                    )
                    .map_err(io)?,
                );
            }
            let alias = progress.manifest.entries[other].version.identity == identity;
            self.observed[other] = Some(self.validate_entry(
                state,
                progress,
                other,
                alias && !checked_payload,
                cancel,
            )?);
            checked_payload |= alias;
        }
        self.revalidate_path(parent, state, progress, index, cancel)?;
        let alias_after = progress
            .manifest
            .same_object(index)
            .iter()
            .find_map(|&other| {
                self.present[other]
                    .then(|| Box::new(self.observed[other].as_ref().unwrap().clone()))
            });
        let parent_after = progress.manifest.entries[index]
            .parent
            .map(|p| Box::new(self.observed[p].as_ref().unwrap().clone()));
        Ok(CleanupRecord::EntryRemoved {
            index,
            parent_after,
            alias_after,
        })
    }

    pub(super) fn acknowledged(&mut self) {
        self.pending_absent = false;
    }
}
