//! Read-only admission of a selected cleanup cohort. This records relationships;
//! it does not turn a selected ID list into permission to remove anything.
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
    Metadata(cleanup_finalize::Plan),
}

impl Member {
    pub(super) fn header(&self) -> &Header {
        match self {
            Self::Payload(value) => value.header(),
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
                Member::Metadata(plan) => {
                    let current = cleanup_finalize::Plan::observe_in_group(
                        &parent,
                        central,
                        &plan.header().stage_name(),
                        &selection,
                        &mut cancel,
                    )?;
                    if current.header() != plan.header() {
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
