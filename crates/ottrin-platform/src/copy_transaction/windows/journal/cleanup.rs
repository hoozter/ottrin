//! Retained-payload admission and journaled native removal for explicit cleanup.
//!
//! Read-only proof grants no deletion authority. The private writer separately
//! admits dependencies, reserves and persists progress, and revalidates after
//! exchanging read capabilities. Journal/stage finalization remains separate.
use super::cleanup_dependencies::Dependencies;
use super::cleanup_protocol::{CleanupRecord, Progress};
use super::cleanup_tree::Remaining;
use super::*;

pub(super) enum Payload {
    None,
    Copy(HeldTree),
    Original(HeldOriginal),
}

/// A handle-free read-only observation. Digests bind the complete journal,
/// native payload epoch, private metadata and dependency catalog through the
/// later writer handoff without retaining raw journals or decoded peer graphs.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Admission {
    header: Header,
    journal: [u8; 32],
    metadata: [u8; 32],
    observed: [u8; 32],
    catalog: [u8; 32],
    pending_absent: bool,
    sequence: u64,
    started: bool,
    location: std::path::PathBuf,
    parent_security: [u8; 32],
}

/// The central control epoch comes from the successful held journal writer.
pub(super) struct ControlChange {
    pub(super) admission: Admission,
    pub(super) before: [u8; 32],
    pub(super) after: [u8; 32],
    pub(super) central: EntryVersion,
}

fn cleanup_error(error: PlatformError, started: bool, transaction: String) -> PlatformError {
    if started {
        PlatformError::CleanupPending {
            transaction,
            message: format!("{error}; cleanup stopped, remaining data and journals retained"),
        }
    } else {
        PlatformError::CopyCleanupRefused(format!("{error}; {transaction}"))
    }
}

impl Admission {
    pub(super) fn header(&self) -> &Header {
        &self.header
    }

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
        let location = windows_fs::location(parent).map_err(io)?;
        let transaction = location.join(name).display().to_string();
        let mut started = false;
        let result = (|| {
            cancel(0)?;
            let parent_security = security::owner_group_dacl_digest(parent)?;
            let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
            started = snapshot.state.cleanup.is_some();
            let (snapshot, observed, pending_absent, catalog) = if let Some(progress) =
                &snapshot.state.cleanup
            {
                let remaining = Remaining::open(parent, &snapshot.state, progress, &mut cancel)?;
                let dependencies = Dependencies::open_selected(
                    central,
                    snapshot.state.header.as_ref().unwrap(),
                    &progress.manifest,
                    selection,
                    &mut cancel,
                )?;
                let observed = super::super::evidence_digest(&remaining.observed)?;
                let absent = remaining.pending_absent;
                let catalog = dependencies.fingerprint()?;
                remaining.revalidate(parent, &snapshot.state, progress, &mut cancel)?;
                drop((remaining, dependencies));
                (snapshot, observed, absent, catalog)
            } else {
                drop(snapshot);
                let held = HeldCleanup::open(parent, central, name, &mut cancel)?;
                let manifest = held.snapshot.state.cleanup_manifest()?;
                let dependencies = Dependencies::open_selected(
                    central,
                    held.snapshot.state.header.as_ref().unwrap(),
                    &manifest,
                    selection,
                    &mut cancel,
                )?;
                held.revalidate(&mut cancel)?;
                let observed = manifest
                    .entries
                    .iter()
                    .map(|entry| Some(&entry.version))
                    .collect::<Vec<_>>();
                let observed = super::super::evidence_digest(&observed)?;
                let catalog = dependencies.fingerprint()?;
                drop(dependencies);
                let HeldCleanup {
                    snapshot, payload, ..
                } = held;
                drop(payload);
                (snapshot, observed, false, catalog)
            };
            let metadata = super::cleanup_finalize::snapshot_metadata(&snapshot, &mut cancel)?;
            snapshot.revalidate(parent, central)?;
            Ok(Self {
                header: snapshot.state.header.as_ref().unwrap().clone(),
                journal: Sha256::digest(&snapshot.bytes).into(),
                metadata: super::super::evidence_digest(&metadata)?,
                observed,
                catalog,
                pending_absent,
                sequence: snapshot.sequence,
                started,
                location,
                parent_security,
            })
        })();
        result.map_err(|error| cleanup_error(error, started, transaction))
    }

    pub(super) fn validate_metadata(
        &self,
        parent: &File,
        central: &File,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let snapshot = Snapshot::load(
            parent,
            central,
            &self.header.stage_name(),
            SnapshotAccess::Read,
        )?;
        if snapshot.state.header.as_ref() != Some(&self.header)
            || <[u8; 32]>::from(Sha256::digest(&snapshot.bytes)) != self.journal
            || super::super::evidence_digest(&super::cleanup_finalize::snapshot_metadata(
                &snapshot,
                &mut cancel,
            )?)? != self.metadata
            || windows_fs::location(parent).map_err(io)? != self.location
            || security::owner_group_dacl_digest(parent)? != self.parent_security
        {
            return Err(io("Copy group member changed during read-only admission"));
        }
        snapshot.revalidate(parent, central)
    }

    pub(super) fn validate_in_group(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        let current = Self::observe_in_group(
            parent,
            central,
            &self.header.stage_name(),
            selection,
            cancel,
        )?;
        if current != *self {
            return Err(io(
                "Copy group member no longer matches confirmed admission",
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

    /// Persist cleanup intent without removing payload or metadata. The
    /// returned proof is captured from this writer, not readmitted by ID.
    pub(super) fn start(
        &self,
        parent: &File,
        central: &File,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        self.start_selected(parent, central, None, cancel)
            .map(|started| started.admission)
    }

    pub(super) fn start_in_group(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ControlChange, PlatformError> {
        self.start_selected(parent, central, Some(selection), cancel)
    }

    fn start_selected(
        &self,
        parent: &File,
        central: &File,
        selection: Option<&super::cleanup_group::Selection>,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ControlChange, PlatformError> {
        self.change_selected(parent, central, selection, |_| Ok(()), cancel)
            .map(|(started, ())| started)
    }

    pub(super) fn prepare_owner_unlink(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        shared: &BTreeSet<(u64, [u8; 16])>,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(ControlChange, super::cleanup_peer::Intent), PlatformError> {
        self.change_selected(
            parent,
            central,
            Some(selection),
            |journal| {
                let progress = journal.state.cleanup.as_ref().unwrap();
                let index = *progress
                    .manifest
                    .order
                    .get(progress.completed)
                    .ok_or_else(|| io("No remaining group payload"))?;
                let node = &progress.manifest.entries[index];
                if progress.manifest.owner != super::cleanup_protocol::Owner::Original
                    || node.version.directory
                    || !shared.contains(&node.version.identity.object_key())
                    || progress.peer_unlink.is_some()
                    || progress.expected_links(index)? < 2
                {
                    return Err(io(
                        "Next owner entry is not a selected shared original file",
                    ));
                }
                if !progress.pending {
                    journal.persist(Record::Cleanup {
                        entry: CleanupRecord::EntryIntent { index },
                    })?;
                }
                super::cleanup_peer::Intent::capture(journal)
            },
            cancel,
        )
    }

    pub(super) fn prepare_peer_unlink(
        &self,
        parent: &File,
        central: &File,
        selection: &super::cleanup_group::Selection,
        intent: &super::cleanup_peer::Intent,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ControlChange, PlatformError> {
        self.change_selected(
            parent,
            central,
            Some(selection),
            |journal| journal.prepare_peer_unlink(intent.bind_local(&journal.state)?),
            cancel,
        )
        .map(|(started, ())| started)
    }

    fn change_selected<T>(
        &self,
        parent: &File,
        central: &File,
        selection: Option<&super::cleanup_group::Selection>,
        operation: impl FnOnce(&mut Journal) -> Result<T, PlatformError>,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(ControlChange, T), PlatformError> {
        let mut started = self.started;
        let result = (|| {
            let (mut journal, remaining, dependencies) =
                self.writer(parent, central, &mut started, selection, &mut cancel)?;
            let value = operation(&mut journal)?;
            remaining.revalidate(
                parent,
                &journal.state,
                journal.state.cleanup.as_ref().unwrap(),
                &mut cancel,
            )?;
            dependencies.revalidate_peers(central)?;
            journal.bindings()?;
            if windows_fs::location(parent).map_err(io)? != self.location
                || security::owner_group_dacl_digest(parent)? != self.parent_security
                || dependencies.fingerprint()? != self.catalog
            {
                return Err(io("Copy cleanup admission changed during control update"));
            }
            let bytes = read(&journal.local)?;
            let metadata = super::cleanup_finalize::metadata_files(
                &journal.header,
                [&journal.stage_dir, &journal.local, &journal.central],
                &bytes,
                &mut cancel,
            )?;
            let admission = Self {
                header: journal.header.clone(),
                journal: Sha256::digest(&bytes).into(),
                metadata: super::super::evidence_digest(&metadata)?,
                observed: self.observed,
                catalog: self.catalog,
                pending_absent: self.pending_absent,
                sequence: journal.sequence,
                started: true,
                location: self.location.clone(),
                parent_security: self.parent_security,
            };
            Ok((
                ControlChange {
                    after: admission.journal,
                    admission,
                    before: self.journal,
                    central: metadata[2].clone(),
                },
                value,
            ))
        })();
        result.map_err(|error| {
            cleanup_error(
                error,
                started,
                self.location
                    .join(self.header.stage_name())
                    .display()
                    .to_string(),
            )
        })
    }

    fn writer(
        &self,
        parent: &File,
        central: &File,
        started: &mut bool,
        selection: Option<&super::cleanup_group::Selection>,
        cancel: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(Journal, Remaining, Dependencies), PlatformError> {
        let name = self.header.stage_name();
        cancel(0)?;
        cleanup_handoff(self.sequence)?;
        let snapshot = Snapshot::load(parent, central, &name, SnapshotAccess::Write)?;
        if snapshot.state.header.as_ref() != Some(&self.header)
            || <[u8; 32]>::from(Sha256::digest(&snapshot.bytes)) != self.journal
            || windows_fs::location(parent).map_err(io)? != self.location
            || security::owner_group_dacl_digest(parent)? != self.parent_security
            || super::super::evidence_digest(&super::cleanup_finalize::snapshot_metadata(
                &snapshot,
                &mut *cancel,
            )?)? != self.metadata
        {
            return Err(io(
                "Copy cleanup admission changed before writer acquisition",
            ));
        }
        let initial = if snapshot.state.cleanup.is_none() {
            Some(Progress::new(snapshot.state.cleanup_manifest()?))
        } else {
            None
        };
        let progress = snapshot
            .state
            .cleanup
            .as_ref()
            .or(initial.as_ref())
            .unwrap();
        let dependencies = Dependencies::open_selected(
            central,
            &self.header,
            &progress.manifest,
            selection,
            &mut *cancel,
        )?;
        if dependencies.fingerprint()? != self.catalog {
            return Err(io("Copy cleanup dependency admission changed"));
        }
        let remaining = Remaining::open(parent, &snapshot.state, progress, &mut *cancel)?;
        if super::super::evidence_digest(&remaining.observed)? != self.observed
            || remaining.pending_absent != self.pending_absent
        {
            return Err(io(
                "Copy cleanup retained epoch changed after read-only admission",
            ));
        }
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        journal.reserve_cleanup_tail()?;
        if journal.state.cleanup.is_none() {
            let manifest = journal.state.cleanup_manifest()?;
            *started = true;
            journal.persist(Record::Cleanup {
                entry: CleanupRecord::Begin {
                    owner: manifest.owner,
                    count: manifest.entries.len(),
                    digest: manifest.digest,
                },
            })?;
        }
        drop(initial);
        Ok((journal, remaining, dependencies))
    }

    pub(super) fn reclaim(
        &self,
        parent: &File,
        central: &File,
        cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<super::cleanup_finalize::Completion, PlatformError> {
        self.reclaim_with_ack(parent, central, cancel)
            .map(|result| result.completion)
    }

    pub(super) fn reclaim_with_ack(
        &self,
        parent: &File,
        central: &File,
        mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<super::cleanup_finalize::Reclaimed, PlatformError> {
        let name = self.header.stage_name();
        let transaction = self.location.join(&name).display().to_string();
        let mut started = self.started;
        let result = (|| {
            let (mut journal, mut remaining, mut dependencies) =
                self.writer(parent, central, &mut started, None, &mut cancel)?;
            loop {
                let progress = journal.state.cleanup.as_ref().unwrap();
                if progress.payload_removed {
                    return super::cleanup_finalize::Reclaimed::capture(
                        &journal,
                        self.journal,
                        &dependencies,
                        parent,
                        central,
                        &mut cancel,
                    );
                }
                dependencies.validate(central)?;
                journal.reserve_cleanup_tail()?;
                if progress.completed == progress.manifest.entries.len() {
                    Remaining::open(parent, &journal.state, progress, &mut cancel)?;
                    journal.persist(Record::Cleanup {
                        entry: CleanupRecord::PayloadRemoved,
                    })?;
                    return super::cleanup_finalize::Reclaimed::capture(
                        &journal,
                        self.journal,
                        &dependencies,
                        parent,
                        central,
                        &mut cancel,
                    );
                }
                let index = progress.manifest.order[progress.completed];
                remaining
                    .acquire(parent, &journal.state, progress, index, &mut cancel)
                    .map_err(|error| {
                        io(format!("Copy cleanup entry {index} acquisition: {error}"))
                    })?;
                if !progress.pending {
                    journal.persist(Record::Cleanup {
                        entry: CleanupRecord::EntryIntent { index },
                    })?;
                }
                let progress = journal.state.cleanup.as_ref().unwrap();
                remaining.revalidate_path(parent, &journal.state, progress, index, &mut cancel)?;
                dependencies.validate(central)?;
                journal.bindings()?;
                let registry_root_unlink = progress.manifest.entries[index].parent.is_none()
                    && journal.header.parent.same_object(journal.header.central)
                    && !remaining.pending_absent;
                let root_name = OsString::from_wide(&progress.manifest.entries[index].name);
                // Any uncertain native result requires fresh admission. In
                // particular, unwinding can close a pending POSIX delete handle.
                journal.failed = true;
                let acknowledgement = remaining
                    .remove(
                        parent,
                        &journal.state,
                        progress,
                        index,
                        journal.sequence,
                        &mut cancel,
                    )
                    .map_err(|error| io(format!("Copy cleanup entry {index} removal: {error}")))?;
                if registry_root_unlink {
                    dependencies.own_root_removed(central, &root_name)?;
                }
                dependencies.validate(central)?;
                journal.failed = false;
                journal.persist(Record::Cleanup {
                    entry: acknowledgement,
                })?;
                remaining.acknowledged();
            }
        })();
        result.map_err(|error| cleanup_error(error, started, transaction))
    }
}

/// Reclaim the recorded retained payload through the same observation used by
/// longer-lived confirmation plans. Metadata finalization is a separate phase.
pub(super) fn reclaim_payload(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    Admission::observe(parent, central, name, &mut cancel)?
        .reclaim(parent, central, cancel)
        .map(|_| ())
}

/// Keep only transaction-owned retained payloads pinned. A successful public
/// copy or restored original belongs to the user and is never cleanup input.
pub(super) struct HeldCleanup<'a> {
    parent: &'a File,
    central: &'a File,
    snapshot: Snapshot,
    pub(super) payload: Payload,
}

impl<'a> HeldCleanup<'a> {
    pub(super) fn open(
        parent: &'a File,
        central: &'a File,
        name: &OsStr,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        progress(0)?;
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
        validate_stage_entries(&snapshot.stage)?;
        let state = &snapshot.state;
        if !state.completion_verified
            || state.undo_intent && !state.undo_complete()
            || state.cleanup.is_some()
        {
            return Err(io(
                "Copy cleanup requires a completed forward or Undo transaction",
            ));
        }
        let header = state.header.as_ref().unwrap();
        let payload = if state.undo_complete() {
            Payload::Copy(HeldTree::open(
                parent,
                state,
                header.candidate_name(),
                None,
                &mut progress,
            )?)
        } else if state.original.is_some() {
            let original = HeldOriginal::open(parent, state, &mut progress)?
                .ok_or_else(|| io("Completed overwrite has no retained original proof"))?;
            if original.name != header.original_name() {
                return Err(io("Copy cleanup original is not at its retained name"));
            }
            Payload::Original(original)
        } else {
            Payload::None
        };
        let held = Self {
            parent,
            central,
            snapshot,
            payload,
        };
        held.revalidate(progress)?;
        Ok(held)
    }

    pub(super) fn revalidate(
        &self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        progress(0)?;
        self.snapshot.revalidate(self.parent, self.central)?;
        validate_stage_entries(&self.snapshot.stage)?;
        match &self.payload {
            Payload::None => {}
            Payload::Copy(tree) => {
                tree.revalidate(self.parent, &self.snapshot.state, &mut progress)?
            }
            Payload::Original(original) => original.revalidate(self.parent, &mut progress)?,
        }
        self.snapshot.revalidate(self.parent, self.central)
    }
}
