//! Paired authority for ordered local-journal, empty-stage and central removal.
use super::cleanup_dependencies::Dependencies;
use super::cleanup_protocol::CleanupRecord;
use super::cleanup_tree::{Remaining, absent, dispose};
use super::*;
use windows_sys::Win32::Storage::FileSystem::{DELETE, SYNCHRONIZE};

fn optional(parent: &File, name: &OsStr, access: u32) -> Result<Option<File>, PlatformError> {
    match unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: None,
                create_new: false,
                access,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    } {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

/// Metadata has no disposable extra content: directory streams must be empty;
/// a journal must contain exactly its verified unnamed record stream.
fn metadata(
    file: &File,
    identity: WindowsStableIdentity,
    security: &security::PrivateSecurity,
    creator: &[u8],
    bytes: Option<&[u8]>,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<EntryVersion, PlatformError> {
    cancel(0)?;
    let before = super::super::version(file)?;
    if before.identity != identity || before.link.is_some() || before.directory != bytes.is_none() {
        return Err(io("Copy cleanup metadata identity/type changed"));
    }
    namespace::validate_single_link(file)?;
    security.validate(file, creator)?;
    let streams = super::super::transfer(file, None, &mut *cancel)?;
    if let Some(bytes) = bytes {
        if streams.len() != 1
            || streams[0].kind != super::super::DATA
            || streams[0].attributes != 0
            || !streams[0].name.is_empty()
            || streams[0].bytes != bytes.len() as u64
            || streams[0].sha256 != format!("{:x}", Sha256::digest(bytes))
        {
            return Err(io("Copy cleanup journal has unrecorded content"));
        }
    } else if !streams.is_empty() {
        return Err(io("Copy cleanup stage has unrecorded streams"));
    }
    if super::super::version(file)? != before {
        return Err(io("Copy cleanup metadata changed during observation"));
    }
    Ok(before)
}

pub(super) fn snapshot_metadata(
    snapshot: &Snapshot,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<[EntryVersion; 3], PlatformError> {
    metadata_files(
        snapshot.state.header.as_ref().unwrap(),
        [&snapshot.stage, &snapshot.local, &snapshot.mirror],
        &snapshot.bytes,
        cancel,
    )
}

pub(super) fn metadata_files(
    header: &Header,
    files: [&File; 3],
    bytes: &[u8],
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<[EntryVersion; 3], PlatformError> {
    validate_stage_entries(files[0])?;
    Ok([
        metadata(
            files[0],
            header.stage,
            &header.stage_security,
            &header.creator,
            None,
            cancel,
        )?,
        metadata(
            files[1],
            header.local_file,
            &header.local_security,
            &header.creator,
            Some(bytes),
            cancel,
        )?,
        metadata(
            files[2],
            header.central_file,
            &header.central_security,
            &header.creator,
            Some(bytes),
            cancel,
        )?,
    ])
}

fn control_epoch(
    control: &discovery::RegisteredJournal,
    central: &File,
    expected: &EntryVersion,
) -> Result<(), PlatformError> {
    let header = control.state.header.as_ref().unwrap();
    bound(central, &header.central_name(), header.central_file)?;
    namespace::validate_single_link(&control.file)?;
    header
        .central_security
        .validate(&control.file, &header.creator)?;
    if super::super::version(&control.file)? != *expected {
        return Err(io(
            "Copy cleanup control epoch changed before metadata removal",
        ));
    }
    Ok(())
}

pub(super) fn prepare(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Plan, PlatformError> {
    let id = name
        .to_str()
        .and_then(|text| text.strip_prefix(".ottrin-copy-"))
        .ok_or_else(|| io("invalid Copy cleanup stage name"))?;
    let central_name = OsString::from(format!("{id}.copy-journal"));
    let control = discovery::RegisteredJournal::open(central, &central_name)?;
    let authorized = control
        .state
        .cleanup
        .as_ref()
        .is_some_and(|p| p.metadata_removal_intent);
    if control.state.header.as_ref().unwrap().stage_name() != name {
        return Err(io("Copy cleanup stage and registry disagree"));
    }
    drop(control);
    if authorized {
        Plan::observe(parent, central, name, cancel)
    } else {
        super::cleanup::Admission::observe(parent, central, name, &mut cancel)?
            .reclaim(parent, central, &mut cancel)?
            .begin(parent, central, cancel)
    }
}

/// Proof of the exact successful payload writer's final state. No handles or
/// raw journals survive the transition to metadata authorization.
#[derive(Clone, Debug)]
pub(super) struct Completion {
    header: Header,
    journal: [u8; 32],
    metadata: [u8; 32],
    catalog: [u8; 32],
    parent_location: std::path::PathBuf,
    central_location: std::path::PathBuf,
    parent_security: [u8; 32],
}

#[derive(Clone, Debug)]
pub(super) struct PayloadChange {
    pub(super) before_journal: [u8; 32],
    pub(super) control_version: EntryVersion,
    pub(super) registry_stamp: EntryVersion,
    pub(super) registry_names: Vec<OsString>,
    pub(super) retained: Option<OsString>,
}

#[derive(Debug)]
pub(super) struct Reclaimed {
    pub(super) completion: Completion,
    pub(super) change: PayloadChange,
}

impl Reclaimed {
    pub(super) fn capture(
        journal: &Journal,
        before_journal: [u8; 32],
        dependencies: &Dependencies,
        parent: &File,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if journal.failed
            || !journal
                .state
                .cleanup
                .as_ref()
                .is_some_and(|p| p.payload_removed)
        {
            return Err(io("Copy cleanup has no successful payload completion"));
        }
        journal.bindings()?;
        let bytes = read(&journal.local)?;
        let metadata = metadata_files(
            &journal.header,
            [&journal.stage_dir, &journal.local, &journal.central],
            &bytes,
            cancel,
        )?;
        dependencies.revalidate_peers(central)?;
        let (registry_stamp, registry_names) = dependencies.namespace_proof();
        Ok(Self {
            completion: Completion {
                header: journal.header.clone(),
                journal: Sha256::digest(&bytes).into(),
                metadata: super::super::evidence_digest(&metadata)?,
                catalog: dependencies.fingerprint()?,
                parent_location: windows_fs::location(parent).map_err(io)?,
                central_location: windows_fs::location(central).map_err(io)?,
                parent_security: security::owner_group_dacl_digest(parent)?,
            },
            change: PayloadChange {
                before_journal,
                control_version: metadata[2].clone(),
                registry_stamp,
                registry_names,
                retained: journal
                    .state
                    .cleanup
                    .as_ref()
                    .unwrap()
                    .manifest
                    .entries
                    .first()
                    .map(|entry| OsString::from_wide(&entry.name)),
            },
        })
    }
}

impl Completion {
    pub(super) fn header(&self) -> &Header {
        &self.header
    }
    pub(super) fn journal_digest(&self) -> [u8; 32] {
        self.journal
    }

    pub(super) fn validate_catalog(
        &self,
        selection: &super::cleanup_group::Selection,
    ) -> Result<(), PlatformError> {
        if self.catalog != selection.fingerprint(&self.header)? {
            return Err(io(
                "Copy payload completion catalog does not match its acknowledgement",
            ));
        }
        Ok(())
    }

    pub(super) fn advance_catalog(
        &mut self,
        before: &super::cleanup_group::Selection,
        after: &super::cleanup_group::Selection,
    ) -> Result<(), PlatformError> {
        before.advance_catalog(&self.header, &mut self.catalog, after)
    }

    pub(super) fn validate_in_group(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.inspect(parent, central, Some(selection), &mut cancel)
            .map(|_| ())
    }

    fn inspect(
        &self,
        parent: &File,
        central: &File,
        selection: Option<&super::cleanup_group::Selection>,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(Snapshot, Dependencies, [EntryVersion; 3]), PlatformError> {
        let snapshot = Snapshot::load(
            parent,
            central,
            &self.header.stage_name(),
            SnapshotAccess::Read,
        )?;
        let versions = snapshot_metadata(&snapshot, &mut *cancel)?;
        if snapshot.state.header.as_ref() != Some(&self.header)
            || <[u8; 32]>::from(Sha256::digest(&snapshot.bytes)) != self.journal
            || super::super::evidence_digest(&versions)? != self.metadata
            || windows_fs::location(parent).map_err(io)? != self.parent_location
            || windows_fs::location(central).map_err(io)? != self.central_location
            || security::owner_group_dacl_digest(parent)? != self.parent_security
        {
            return Err(io(
                "Copy cleanup completion changed before metadata authorization",
            ));
        }
        let progress = snapshot
            .state
            .cleanup
            .as_ref()
            .filter(|p| p.payload_removed && !p.metadata_removal_intent)
            .ok_or_else(|| io("Copy cleanup payload is not ready for metadata finalization"))?;
        Remaining::open(parent, &snapshot.state, progress, &mut *cancel)?;
        let dependencies = Dependencies::open_selected(
            central,
            &snapshot.state,
            &progress.manifest,
            selection,
            &mut *cancel,
        )?;
        if dependencies.fingerprint()? != self.catalog {
            return Err(io(
                "Copy cleanup dependencies changed before metadata authorization",
            ));
        }
        snapshot.revalidate(parent, central)?;
        Ok((snapshot, dependencies, versions))
    }

    pub(super) fn begin(
        &self,
        parent: &File,
        central: &File,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Plan, PlatformError> {
        self.begin_with_ack(parent, central, cancel)
            .map(|authorized| authorized.plan)
    }

    pub(super) fn begin_with_ack(
        &self,
        parent: &File,
        central: &File,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Authorized, PlatformError> {
        let name = self.header.stage_name();
        let (snapshot, dependencies, versions) =
            self.inspect(parent, central, None, &mut cancel)?;
        let bytes = snapshot.bytes;
        let sequence = snapshot.sequence;
        // Release all read leases before acquiring the paired journal writer.
        drop((snapshot.stage, snapshot.local, snapshot.mirror));
        cleanup_handoff(sequence)?;
        let snapshot = Snapshot::load(parent, central, &name, SnapshotAccess::Write)?;
        if snapshot.bytes != bytes
            || snapshot_metadata(&snapshot, &mut cancel)? != versions
            || windows_fs::location(parent).map_err(io)? != self.parent_location
            || windows_fs::location(central).map_err(io)? != self.central_location
            || security::owner_group_dacl_digest(parent)? != self.parent_security
        {
            return Err(io("Copy cleanup metadata changed during writer handoff"));
        }
        dependencies.revalidate_peers(central)?;
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        journal.reserve_cleanup_tail()?;
        journal.persist(Record::Cleanup {
            entry: CleanupRecord::MetadataRemovalIntent,
        })?;
        // Construct authority from our own successful writer, never from a new
        // by-name admission after releasing it.
        let plan = Plan {
            header: journal.header.clone(),
            digest: Sha256::digest(read(&journal.local)?).into(),
            retained: journal
                .state
                .cleanup
                .as_ref()
                .unwrap()
                .manifest
                .entries
                .first()
                .map(|entry| OsString::from_wide(&entry.name)),
            parent_location: self.parent_location.clone(),
            central_location: self.central_location.clone(),
            parent_security: self.parent_security,
        };
        let versions = metadata_files(
            &journal.header,
            [&journal.stage_dir, &journal.local, &journal.central],
            &read(&journal.local)?,
            &mut cancel,
        )?;
        let control_version = versions[2].clone();
        drop((journal, dependencies));
        plan.run(parent, central, Action::Inspect, None, &mut cancel)?;
        Ok(Authorized {
            plan,
            before_journal: self.journal,
            control_version,
        })
    }
}

#[derive(Debug)]
pub(super) struct Authorized {
    pub(super) plan: Plan,
    pub(super) before_journal: [u8; 32],
    pub(super) control_version: EntryVersion,
}

/// A completed paired metadata intent grants only these exact native objects.
/// Retaining this opaque proof permits an idempotent retry after central unlink.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plan {
    header: Header,
    digest: [u8; 32],
    retained: Option<OsString>,
    parent_location: std::path::PathBuf,
    central_location: std::path::PathBuf,
    parent_security: [u8; 32],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Inspect,
    Remove,
}

impl Action {
    fn file_access(self) -> u32 {
        FILE_GENERIC_READ | if self == Self::Remove { DELETE } else { 0 }
    }

    fn flush(self, file: &File) -> Result<(), PlatformError> {
        if self == Self::Remove {
            file.sync_all().map_err(io)?;
        }
        Ok(())
    }
}

impl Plan {
    pub(super) fn header(&self) -> &Header {
        &self.header
    }

    pub(super) fn journal_digest(&self) -> [u8; 32] {
        self.digest
    }

    pub(super) fn validate_in_group(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.run(
            parent,
            central,
            Action::Inspect,
            Some(selection),
            &mut cancel,
        )
    }

    /// Inspect existing paired removal authority without writing, flushing or
    /// removing anything. All leases are released before returning this proof.
    pub(super) fn observe(
        parent: &File,
        central: &File,
        name: &OsStr,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::observe_selected(parent, central, name, None, &mut cancel)
    }

    pub(super) fn observe_in_group(
        parent: &File,
        central: &File,
        name: &OsStr,
        selection: &super::cleanup_group::Selection,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::observe_selected(parent, central, name, Some(selection), cancel)
    }

    fn observe_selected(
        parent: &File,
        central: &File,
        name: &OsStr,
        selection: Option<&super::cleanup_group::Selection>,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let id = name
            .to_str()
            .and_then(|text| text.strip_prefix(".ottrin-copy-"))
            .ok_or_else(|| io("invalid Copy cleanup stage name"))?;
        let control = discovery::RegisteredJournal::open(
            central,
            &OsString::from(format!("{id}.copy-journal")),
        )?;
        let header = control.state.header.as_ref().unwrap();
        let progress = control
            .state
            .cleanup
            .as_ref()
            .filter(|p| p.metadata_removal_intent)
            .ok_or_else(|| io("Copy cleanup lacks paired metadata removal authority"))?;
        if header.stage_name() != name
            || stable_identity_from_handle(parent).map_err(io)? != header.parent
        {
            return Err(io("Copy cleanup metadata parent/stage mismatch"));
        }
        let retained = progress
            .manifest
            .entries
            .first()
            .map(|entry| OsString::from_wide(&entry.name));
        if let Some(name) = &retained {
            absent(parent, name)?;
        }
        cancel(0)?;
        let plan = Self {
            header: header.clone(),
            digest: Sha256::digest(&control.bytes).into(),
            retained,
            parent_location: windows_fs::location(parent).map_err(io)?,
            central_location: windows_fs::location(central).map_err(io)?,
            parent_security: security::owner_group_dacl_digest(parent)?,
        };
        drop(control);
        plan.run(parent, central, Action::Inspect, selection, &mut cancel)?;
        Ok(plan)
    }

    fn bindings(&self, parent: &File, central: &File) -> Result<(), PlatformError> {
        if stable_identity_from_handle(parent).map_err(io)? != self.header.parent
            || stable_identity_from_handle(central).map_err(io)? != self.header.central
            || windows_fs::location(parent).map_err(io)? != self.parent_location
            || windows_fs::location(central).map_err(io)? != self.central_location
            || security::owner_group_dacl_digest(parent)? != self.parent_security
            || security::current_owner()? != self.header.creator
        {
            return Err(io("Copy cleanup metadata action admission changed"));
        }
        if let Some(name) = &self.retained {
            absent(parent, name)?;
        }
        Ok(())
    }

    pub(super) fn finish(
        &self,
        parent: &File,
        central: &File,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.run(parent, central, Action::Remove, None, &mut cancel)
            .map_err(|error| PlatformError::CleanupPending {
                transaction: self
                    .parent_location
                    .join(self.header.stage_name())
                    .display()
                    .to_string(),
                message: format!("{error}; metadata cleanup stopped, unknown entries retained"),
            })
    }

    fn run(
        &self,
        parent: &File,
        central: &File,
        action: Action,
        selection: Option<&super::cleanup_group::Selection>,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if selection.is_some() && action != Action::Inspect {
            return Err(io(
                "Read-only group selection cannot authorize metadata removal",
            ));
        }
        cancel(0)?;
        self.bindings(parent, central)?;
        let central_name = self.header.central_name();
        let stage_name = self.header.stage_name();
        let Some(file) = optional(central, &central_name, action.file_access())? else {
            // Only a previously admitted intent may report this terminal replay.
            absent(parent, &stage_name)?;
            absent(central, &central_name)?;
            action.flush(parent)?;
            return action.flush(central);
        };
        let control = discovery::RegisteredJournal::from_file(central, &central_name, file)?;
        if control.state.header.as_ref() != Some(&self.header)
            || <[u8; 32]>::from(Sha256::digest(&control.bytes)) != self.digest
            || !control
                .state
                .cleanup
                .as_ref()
                .is_some_and(|p| p.metadata_removal_intent)
        {
            return Err(io("Copy cleanup metadata control record changed"));
        }
        let decoded = decode(&control.bytes)?;
        let sequence = decoded.sequence;
        let progress = control.state.cleanup.as_ref().unwrap();
        let mut dependencies = Dependencies::open_selected(
            central,
            &control.state,
            &progress.manifest,
            selection,
            &mut *cancel,
        )?;
        let central_version = metadata(
            &control.file,
            self.header.central_file,
            &self.header.central_security,
            &self.header.creator,
            Some(&control.bytes),
            cancel,
        )?;
        action.flush(parent)?;
        action.flush(central)?;
        if let Some(stage) = optional(
            parent,
            &stage_name,
            if action == Action::Remove {
                windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED | SYNCHRONIZE
            } else {
                FILE_GENERIC_READ
            },
        )? {
            let mut stage_version = metadata(
                &stage,
                self.header.stage,
                &self.header.stage_security,
                &self.header.creator,
                None,
                cancel,
            )?;
            let names = windows_fs::directory_children_limited(&stage, 1).map_err(io)?;
            if names != [OsString::from(LOCAL)] && !names.is_empty() {
                return Err(io("Copy cleanup stage contains unknown entries"));
            }
            action.flush(&stage)?;
            if let Some(local) = optional(&stage, OsStr::new(LOCAL), action.file_access())? {
                let local_version = metadata(
                    &local,
                    self.header.local_file,
                    &self.header.local_security,
                    &self.header.creator,
                    Some(&control.bytes),
                    cancel,
                )?;
                self.bindings(parent, central)?;
                dependencies.validate(central)?;
                control_epoch(&control, central, &central_version)?;
                bound(parent, &stage_name, self.header.stage)?;
                bound(&stage, OsStr::new(LOCAL), self.header.local_file)?;
                bound(central, &central_name, self.header.central_file)?;
                if super::super::version(&local)? != local_version
                    || super::super::version(&stage)? != stage_version
                {
                    return Err(io("Copy cleanup local metadata epoch changed"));
                }
                if action == Action::Inspect {
                    return Ok(());
                }
                dispose(&local)?;
                metadata_boundary(
                    sequence,
                    CleanupMetadataRole::Local,
                    CleanupMetadataPoint::DispositionSet,
                )?;
                drop(local);
                metadata_boundary(
                    sequence,
                    CleanupMetadataRole::Local,
                    CleanupMetadataPoint::HandleClosed,
                )?;
                absent(&stage, OsStr::new(LOCAL))?;
                stage.sync_all().map_err(io)?;
                metadata_boundary(
                    sequence,
                    CleanupMetadataRole::Local,
                    CleanupMetadataPoint::Flushed,
                )?;
                let actual = super::super::version(&stage)?;
                stage_version.modified = actual.modified;
                stage_version.changed = actual.changed;
                if actual != stage_version {
                    return Err(io("Copy cleanup stage changed beyond local unlink"));
                }
            } else if !names.is_empty() {
                return Err(io("Copy cleanup stage membership changed"));
            }
            if !windows_fs::directory_children_limited(&stage, 1)
                .map_err(io)?
                .is_empty()
                || metadata(
                    &stage,
                    self.header.stage,
                    &self.header.stage_security,
                    &self.header.creator,
                    None,
                    cancel,
                )? != stage_version
            {
                return Err(io("Copy cleanup empty stage changed"));
            }
            self.bindings(parent, central)?;
            dependencies.validate(central)?;
            control_epoch(&control, central, &central_version)?;
            bound(parent, &stage_name, self.header.stage)?;
            if action == Action::Inspect {
                return Ok(());
            }
            dispose(&stage)?;
            metadata_boundary(
                sequence,
                CleanupMetadataRole::Stage,
                CleanupMetadataPoint::DispositionSet,
            )?;
            drop(stage);
            metadata_boundary(
                sequence,
                CleanupMetadataRole::Stage,
                CleanupMetadataPoint::HandleClosed,
            )?;
            absent(parent, &stage_name)?;
            parent.sync_all().map_err(io)?;
            metadata_boundary(
                sequence,
                CleanupMetadataRole::Stage,
                CleanupMetadataPoint::Flushed,
            )?;
            if self.header.parent.same_object(self.header.central) {
                dependencies.own_root_removed(central, &stage_name)?;
            }
        }
        self.bindings(parent, central)?;
        dependencies.validate(central)?;
        absent(parent, &stage_name)?;
        bound(central, &central_name, self.header.central_file)?;
        if metadata(
            &control.file,
            self.header.central_file,
            &self.header.central_security,
            &self.header.creator,
            Some(&control.bytes),
            cancel,
        )? != central_version
        {
            return Err(io("Copy cleanup central metadata epoch changed"));
        }
        if action == Action::Inspect {
            return Ok(());
        }
        dispose(&control.file)?;
        metadata_boundary(
            sequence,
            CleanupMetadataRole::Central,
            CleanupMetadataPoint::DispositionSet,
        )?;
        drop(control);
        metadata_boundary(
            sequence,
            CleanupMetadataRole::Central,
            CleanupMetadataPoint::HandleClosed,
        )?;
        absent(central, &central_name)?;
        central.sync_all().map_err(io)?;
        metadata_boundary(
            sequence,
            CleanupMetadataRole::Central,
            CleanupMetadataPoint::Flushed,
        )
    }
}

fn metadata_boundary(
    sequence: u64,
    role: CleanupMetadataRole,
    point: CleanupMetadataPoint,
) -> Result<(), PlatformError> {
    boundary(sequence, Step::CleanupMetadata { role, point })
}
