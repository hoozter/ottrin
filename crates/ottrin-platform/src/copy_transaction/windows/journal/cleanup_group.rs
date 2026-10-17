//! Bound admission and start barrier for selected cleanup cohorts. Physical
//! removal is separate; selected IDs alone never grant mutation authority.
use super::super::registry::ConfiguredRegistry;
use super::*;
use std::collections::BTreeMap;
use std::path::Path;

type Object = (u64, [u8; 16]);

#[derive(Clone, Debug)]
struct Control {
    digest: [u8; 32],
    version: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
}

/// The initial catalog, including unselected peers. Only this module constructs
/// a selection from held native records; callers cannot supply exclusion IDs.
#[derive(Clone, Debug)]
pub(super) struct Selection {
    selected: BTreeSet<String>,
    records: BTreeMap<OsString, Control>,
    names: Vec<OsString>,
    root: EntryVersion,
    security: security::PrivateSecurity,
    creator: Vec<u8>,
}

impl Selection {
    fn capture(
        central: &File,
        ids: &[String],
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if ids.is_empty() || ids.len() > discovery::MAX_REGISTRY_ENTRIES {
            return Err(io("Invalid Copy cleanup selection size"));
        }
        let mut selected = BTreeSet::new();
        for id in ids {
            let name = OsString::from(format!("{id}.copy-journal"));
            if discovery::registry_id(&name).as_deref() != Some(id.as_str())
                || !selected.insert(id.clone())
            {
                return Err(io("Invalid or duplicate Copy cleanup selection"));
            }
        }
        let root = super::super::version(central)?;
        let creator = security::current_owner()?;
        let security = security::PrivateSecurity::capture(central, &creator)?;
        let names =
            windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?;
        let mut records = BTreeMap::new();
        for name in &names {
            cancel(0)?;
            let Some(_) = discovery::registry_id(name) else {
                if discovery::registry_candidate(name) {
                    return Err(io("Copy group catalog has an unrecognized recovery record"));
                }
                continue;
            };
            let record = discovery::RegisteredJournal::dependency(central, name)?;
            let header = record.state.header.as_ref().unwrap();
            records.insert(
                name.clone(),
                Control {
                    digest: Sha256::digest(&record.bytes).into(),
                    version: super::super::version(&record.file)?,
                    security: header.central_security.clone(),
                    creator: header.creator.clone(),
                },
            );
        }
        for id in &selected {
            if !records.contains_key(&OsString::from(format!("{id}.copy-journal"))) {
                return Err(io("Selected Copy cleanup record is missing"));
            }
        }
        let result = Self {
            selected,
            records,
            names,
            root,
            security,
            creator,
        };
        result.validate(central, cancel)?;
        Ok(result)
    }

    pub(super) fn fingerprint(&self, header: &Header) -> Result<[u8; 32], PlatformError> {
        if header.central != self.root.identity {
            return Err(io("Copy group registry binding changed"));
        }
        cleanup_dependencies::catalog_fingerprint(
            header.central,
            &self.root,
            &self.security,
            &self.creator,
            &self.names,
            self.names.iter().filter_map(|name| {
                if *name == header.central_name() {
                    return None;
                }
                self.records
                    .get(name)
                    .map(|record| (name.as_os_str(), record.digest, &record.version))
            }),
        )
    }

    pub(super) fn advance_catalog(
        &self,
        header: &Header,
        catalog: &mut [u8; 32],
        after: &Self,
    ) -> Result<(), PlatformError> {
        if *catalog != self.fingerprint(header)? {
            return Err(io("Copy group catalog handoff does not match admission"));
        }
        *catalog = after.fingerprint(header)?;
        Ok(())
    }

    fn accept_payload(
        &mut self,
        completion: &cleanup_finalize::Completion,
        change: &cleanup_finalize::PayloadChange,
    ) -> Result<(), PlatformError> {
        let header = completion.header();
        let record = self
            .records
            .get_mut(&header.central_name())
            .ok_or_else(|| io("Completed Copy member is absent from group"))?;
        if !self.selected.contains(&header.transaction)
            || record.digest != change.before_journal
            || record.version.identity != change.control_version.identity
            || change.control_version.identity != header.central_file
        {
            return Err(io(
                "Copy payload acknowledgement does not match frozen control",
            ));
        }
        let mut names = self.names.clone();
        let mut root = self.root.clone();
        if names != change.registry_names {
            if !header.parent.same_object(header.central) {
                return Err(io("Copy payload changed an unrelated registry namespace"));
            }
            let retained = change
                .retained
                .as_ref()
                .ok_or_else(|| io("Copy payload has no retained root acknowledgement"))?;
            let index = names
                .iter()
                .position(|name| name == retained)
                .ok_or_else(|| io("Copy payload root was not in the frozen registry"))?;
            names.remove(index);
            root.modified = change.registry_stamp.modified;
            root.changed = change.registry_stamp.changed;
        }
        if names != change.registry_names || root != change.registry_stamp {
            return Err(io(
                "Copy payload acknowledgement has an unexpected registry change",
            ));
        }
        record.digest = completion.journal_digest();
        record.version = change.control_version.clone();
        self.names = names;
        self.root = root;
        Ok(())
    }

    /// Only a successful held writer supplies this epoch. No reopened ID or
    /// freshly observed external catalog change is accepted as our own work.
    fn accept_start(&mut self, started: &cleanup::Started) -> Result<(), PlatformError> {
        let header = started.admission.header();
        let record = self
            .records
            .get_mut(&header.central_name())
            .ok_or_else(|| io("Started Copy member is absent from group"))?;
        if !self.selected.contains(&header.transaction)
            || record.digest != started.before
            || record.version.identity != started.central.identity
            || started.central.identity != header.central_file
        {
            return Err(io(
                "Copy group start acknowledgement does not match its frozen control",
            ));
        }
        record.digest = started.after;
        record.version = started.central.clone();
        Ok(())
    }

    fn check(&self, record: &discovery::RegisteredJournal) -> Result<(), PlatformError> {
        let header = record.state.header.as_ref().unwrap();
        let expected = self
            .records
            .get(&header.central_name())
            .ok_or_else(|| io("Copy group catalog has an unexpected record"))?;
        if super::super::version(&record.file)? != expected.version
            || <[u8; 32]>::from(Sha256::digest(&record.bytes)) != expected.digest
        {
            return Err(io("Copy group catalog record changed during admission"));
        }
        expected.security.validate(&record.file, &expected.creator)
    }

    pub(super) fn admits(
        &self,
        record: &discovery::RegisteredJournal,
    ) -> Result<bool, PlatformError> {
        self.check(record)?;
        Ok(self
            .selected
            .contains(&record.state.header.as_ref().unwrap().transaction))
    }

    fn validate(
        &self,
        central: &File,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if super::super::version(central)? != self.root
            || windows_fs::directory_children_limited(central, discovery::MAX_REGISTRY_ENTRIES)
                .map_err(io)?
                != self.names
        {
            return Err(io("Copy group catalog namespace or epoch changed"));
        }
        self.security.validate(central, &self.creator)?;
        for name in self.records.keys() {
            cancel(0)?;
            self.check(&discovery::RegisteredJournal::dependency(central, name)?)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) enum Member {
    Payload(cleanup::Admission),
    Completion(cleanup_finalize::Completion),
    Metadata(cleanup_finalize::Plan),
}

impl Member {
    pub(super) fn header(&self) -> &Header {
        match self {
            Self::Payload(value) => value.header(),
            Self::Completion(value) => value.header(),
            Self::Metadata(value) => value.header(),
        }
    }
}

/// No file handles, raw journals or decoded peer graphs survive this observation.
/// Shared retained objects and cross-member references are explicit inputs for
/// the eventual executor; they are not silently declined or treated as absent.
#[derive(Clone, Debug)]
pub(super) struct Observation {
    pub(super) registry: std::path::PathBuf,
    pub(super) selection: Selection,
    pub(super) members: Vec<Member>,
    pub(super) references: BTreeSet<(usize, usize)>,
    pub(super) shared: BTreeMap<Object, Vec<(usize, usize)>>,
}

impl Observation {
    /// Commit every selected payload member's cleanup intent before physical
    /// removal is allowed. Each successful start advances only that writer's
    /// catalog record; failures preserve acknowledged earlier starts for retry.
    /// This method never removes payloads or transaction metadata.
    pub(super) fn start_all(
        &mut self,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        cancel(0)?;
        let manager = ConfiguredRegistry::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy group registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let mut volumes = discovery::Volumes::new(&central)?;
        // Validate every confirmed payload before the first durable begin.
        for member in &self.members {
            let header = member.header();
            let parent = volumes.resolve(header.parent, FILE_GENERIC_READ, &mut cancel)?;
            match member {
                Member::Payload(admission) => {
                    admission.validate_in_group(&parent, &central, &self.selection, &mut cancel)?
                }
                Member::Completion(completion) => {
                    completion.validate_in_group(&parent, &central, &self.selection, &mut cancel)?
                }
                Member::Metadata(plan) => {
                    let current = cleanup_finalize::Plan::observe_in_group(
                        &parent,
                        &central,
                        &header.stage_name(),
                        &self.selection,
                        &mut cancel,
                    )?;
                    if current != *plan {
                        return Err(io("Copy group metadata member changed before start"));
                    }
                }
            }
        }
        self.selection.validate(&central, &mut cancel)?;
        for index in 0..self.members.len() {
            let Member::Payload(admission) = &self.members[index] else {
                continue;
            };
            cancel(0)?;
            let parent = if admission
                .header()
                .parent
                .same_object(admission.header().central)
            {
                central.try_clone().map_err(io)?
            } else {
                volumes.resolve(
                    admission.header().parent,
                    FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                    &mut cancel,
                )?
            };
            let started =
                admission.start_in_group(&parent, &central, &self.selection, &mut cancel)?;
            let mut selection = self.selection.clone();
            selection.accept_start(&started)?;
            let mut members = self.members.clone();
            members[index] = Member::Payload(started.admission);
            for member in &mut members {
                match member {
                    Member::Payload(admission) => {
                        admission.advance_catalog(&self.selection, &selection)?
                    }
                    Member::Completion(completion) => {
                        completion.advance_catalog(&self.selection, &selection)?
                    }
                    Member::Metadata(_) => {}
                }
            }
            // Retain our writer proof before a cancellation or external-state
            // recheck can fail. No handles escape this method.
            self.selection = selection;
            self.members = members;
            self.selection.validate(&central, &mut cancel)?;
        }
        manager.validate()
    }

    /// Reclaim one member through the ordinary physical writer. This retains
    /// its completion proof and advances peers only from that writer's exact
    /// control/registry acknowledgement. Shared aliases need a coordinated
    /// per-unlink protocol; this primitive cannot bypass their protection.
    pub(super) fn reclaim_payload(
        &mut self,
        index: usize,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if index >= self.members.len() {
            return Err(io("Copy group member index is invalid"));
        }
        if self
            .shared
            .values()
            .any(|owners| owners.iter().any(|&(owner, _)| owner == index))
        {
            return Err(io(
                "Shared retained aliases require coordinated unlink acknowledgements",
            ));
        }
        self.start_all(&mut cancel)?;
        let Member::Payload(admission) = &self.members[index] else {
            return Ok(());
        };
        let manager = ConfiguredRegistry::open_at(&self.registry, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = manager.writable_root()?;
        self.selection.validate(&central, &mut cancel)?;
        let parent = if admission
            .header()
            .parent
            .same_object(admission.header().central)
        {
            central.try_clone().map_err(io)?
        } else {
            discovery::Volumes::new(&central)?.resolve(
                admission.header().parent,
                FILE_GENERIC_READ | windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
                &mut cancel,
            )?
        };
        let reclaimed = admission.reclaim_with_ack(&parent, &central, &mut cancel)?;
        let completion = reclaimed.completion;
        let mut selection = self.selection.clone();
        selection.accept_payload(&completion, &reclaimed.change)?;
        let mut members = self.members.clone();
        for (other, member) in members.iter_mut().enumerate() {
            if other == index {
                continue;
            }
            match member {
                Member::Payload(admission) => {
                    admission.advance_catalog(&self.selection, &selection)?
                }
                Member::Completion(completion) => {
                    completion.advance_catalog(&self.selection, &selection)?
                }
                Member::Metadata(_) => {}
            }
        }
        // The new completion already carries the post-unlink catalog; validate
        // it against the acknowledged catalog rather than applying its delta twice.
        completion.validate_catalog(&selection)?;
        members[index] = Member::Completion(completion);
        self.members = members;
        self.selection = selection;
        self.selection.validate(&central, &mut cancel)?;
        manager.validate()
    }

    pub(super) fn observe_at(
        path: &Path,
        ids: &[String],
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        cancel(0)?;
        let manager = ConfiguredRegistry::open_at(path, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = manager.root();
        let selection = Selection::capture(central, ids, &mut cancel)?;
        let mut volumes = discovery::Volumes::new(central)?;
        let mut members = Vec::new();
        let mut protected: BTreeMap<Object, BTreeSet<usize>> = BTreeMap::new();
        let mut retained: BTreeMap<Object, Vec<(usize, usize)>> = BTreeMap::new();
        for (member_index, id) in ids.iter().enumerate() {
            cancel(0)?;
            let record = discovery::RegisteredJournal::open(
                central,
                &OsString::from(format!("{id}.copy-journal")),
            )?;
            selection.check(&record)?;
            let header = record.state.header.as_ref().unwrap().clone();
            let metadata = record
                .state
                .cleanup
                .as_ref()
                .is_some_and(|p| p.metadata_removal_intent);
            drop(record);
            let parent = volumes.resolve(header.parent, FILE_GENERIC_READ, &mut cancel)?;
            let member = if metadata {
                Member::Metadata(cleanup_finalize::Plan::observe_in_group(
                    &parent,
                    central,
                    &header.stage_name(),
                    &selection,
                    &mut cancel,
                )?)
            } else {
                let admission = cleanup::Admission::observe_in_group(
                    &parent,
                    central,
                    &header.stage_name(),
                    &selection,
                    &mut cancel,
                )?;
                let snapshot =
                    Snapshot::load(&parent, central, &header.stage_name(), SnapshotAccess::Read)?;
                let manifest;
                let (manifest, completed) = if let Some(progress) = &snapshot.state.cleanup {
                    (&progress.manifest, progress.completed)
                } else {
                    manifest = snapshot.state.cleanup_manifest()?;
                    (&manifest, 0)
                };
                for &entry in &manifest.order[completed..] {
                    let key = manifest.entries[entry].version.identity.object_key();
                    protected.entry(key).or_default().insert(member_index);
                    retained.entry(key).or_default().push((member_index, entry));
                }
                admission.validate_metadata(&parent, central, &mut cancel)?;
                Member::Payload(admission)
            };
            if member.header() != &header {
                return Err(io("Copy group member header changed"));
            }
            for identity in [header.stage, header.local_file, header.central_file] {
                protected
                    .entry(identity.object_key())
                    .or_default()
                    .insert(member_index);
            }
            members.push(member);
        }
        let mut references = BTreeSet::new();
        for (index, member) in members.iter().enumerate() {
            cancel(0)?;
            let record =
                discovery::RegisteredJournal::open(central, &member.header().central_name())?;
            selection.check(&record)?;
            for identity in cleanup_dependencies::references(&record.state) {
                if let Some(owners) = protected.get(&identity.object_key()) {
                    references.extend(
                        owners
                            .iter()
                            .filter(|&&owner| owner != index)
                            .map(|&owner| (index, owner)),
                    );
                }
            }
            drop(record);
            let parent = volumes.resolve(member.header().parent, FILE_GENERIC_READ, &mut cancel)?;
            match member {
                Member::Payload(admission) => {
                    admission.validate_metadata(&parent, central, &mut cancel)?
                }
                Member::Completion(completion) => {
                    completion.validate_in_group(&parent, central, &selection, &mut cancel)?
                }
                Member::Metadata(plan) => {
                    let current = cleanup_finalize::Plan::observe_in_group(
                        &parent,
                        central,
                        &plan.header().stage_name(),
                        &selection,
                        &mut cancel,
                    )?;
                    if current != *plan {
                        return Err(io("Copy metadata group member changed"));
                    }
                }
            }
        }
        selection.validate(central, &mut cancel)?;
        manager.validate()?;
        retained.retain(|_, owners| {
            owners
                .iter()
                .map(|(owner, _)| owner)
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        });
        Ok(Self {
            registry: manager.path().to_path_buf(),
            selection,
            members,
            references,
            shared: retained,
        })
    }
}
