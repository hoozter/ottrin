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
    validate_stage_entries(&snapshot.stage)?;
    let header = snapshot.state.header.as_ref().unwrap();
    Ok([
        metadata(
            &snapshot.stage,
            header.stage,
            &header.stage_security,
            &header.creator,
            None,
            cancel,
        )?,
        metadata(
            &snapshot.local,
            header.local_file,
            &header.local_security,
            &header.creator,
            Some(&snapshot.bytes),
            cancel,
        )?,
        metadata(
            &snapshot.mirror,
            header.central_file,
            &header.central_security,
            &header.creator,
            Some(&snapshot.bytes),
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
    if !authorized {
        // Reject unknown metadata before reclaiming an otherwise valid payload.
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
        snapshot_metadata(&snapshot, &mut cancel)?;
        drop(snapshot);
        super::cleanup::reclaim_payload(parent, central, name, &mut cancel)?;
        authorize(parent, central, name, &mut cancel)?;
    }
    Plan::admit(parent, central, name, cancel)
}

fn authorize(
    parent: &File,
    central: &File,
    name: &OsStr,
    cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
    let progress = snapshot
        .state
        .cleanup
        .as_ref()
        .filter(|p| p.payload_removed && !p.metadata_removal_intent)
        .ok_or_else(|| io("Copy cleanup payload is not ready for metadata finalization"))?;
    Remaining::open(parent, &snapshot.state, progress, &mut *cancel)?;
    let versions = snapshot_metadata(&snapshot, cancel)?;
    let dependencies = Dependencies::open(
        central,
        snapshot.state.header.as_ref().unwrap(),
        &progress.manifest,
        &mut *cancel,
    )?;
    snapshot.revalidate(parent, central)?;
    let bytes = snapshot.bytes.clone();
    let sequence = snapshot.sequence;
    let location = windows_fs::location(parent).map_err(io)?;
    let parent_security = security::owner_group_dacl_digest(parent)?;
    drop(snapshot);
    cleanup_handoff(sequence)?;
    let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Write)?;
    if snapshot.bytes != bytes
        || snapshot_metadata(&snapshot, cancel)? != versions
        || windows_fs::location(parent).map_err(io)? != location
        || security::owner_group_dacl_digest(parent)? != parent_security
    {
        return Err(io("Copy cleanup metadata changed during writer handoff"));
    }
    dependencies.revalidate_peers(central)?;
    let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
    journal.reserve_cleanup_tail()?;
    journal.persist(Record::Cleanup {
        entry: CleanupRecord::MetadataRemovalIntent,
    })
}

/// A completed paired metadata intent grants only these exact native objects.
/// Retaining this opaque proof permits an idempotent retry after central unlink.
pub(super) struct Plan {
    header: Header,
    digest: [u8; 32],
    retained: Option<OsString>,
    parent_location: std::path::PathBuf,
    central_location: std::path::PathBuf,
    parent_security: [u8; 32],
}

impl Plan {
    fn admit(
        parent: &File,
        central: &File,
        name: &OsStr,
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
        Ok(Self {
            header: header.clone(),
            digest: Sha256::digest(&control.bytes).into(),
            retained,
            parent_location: windows_fs::location(parent).map_err(io)?,
            central_location: windows_fs::location(central).map_err(io)?,
            parent_security: security::owner_group_dacl_digest(parent)?,
        })
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
        self.finish_inner(parent, central, &mut cancel)
            .map_err(|error| PlatformError::CleanupPending {
                transaction: self
                    .parent_location
                    .join(self.header.stage_name())
                    .display()
                    .to_string(),
                message: format!("{error}; metadata cleanup stopped, unknown entries retained"),
            })
    }

    fn finish_inner(
        &self,
        parent: &File,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        self.bindings(parent, central)?;
        let central_name = self.header.central_name();
        let stage_name = self.header.stage_name();
        let Some(file) = optional(central, &central_name, FILE_GENERIC_READ | DELETE)? else {
            // Only a previously admitted intent may report this terminal replay.
            absent(parent, &stage_name)?;
            absent(central, &central_name)?;
            parent.sync_all().map_err(io)?;
            return central.sync_all().map_err(io);
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
        let mut dependencies =
            Dependencies::open(central, &self.header, &progress.manifest, &mut *cancel)?;
        let central_version = metadata(
            &control.file,
            self.header.central_file,
            &self.header.central_security,
            &self.header.creator,
            Some(&control.bytes),
            cancel,
        )?;
        parent.sync_all().map_err(io)?;
        central.sync_all().map_err(io)?;
        if let Some(stage) = optional(
            parent,
            &stage_name,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED | SYNCHRONIZE,
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
            stage.sync_all().map_err(io)?;
            if let Some(local) = optional(&stage, OsStr::new(LOCAL), FILE_GENERIC_READ | DELETE)? {
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
