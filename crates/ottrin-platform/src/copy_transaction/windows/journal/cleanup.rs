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

/// Reclaim only the recorded retained payload. Journal/stage finalization is a
/// separate guarded phase; public cleanup remains unavailable until it exists.
pub(super) fn reclaim_payload(
    parent: &File,
    central: &File,
    name: &OsStr,
    mut cancel: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let transaction = windows_fs::location(parent)
        .map_err(io)?
        .join(name)
        .display()
        .to_string();
    let mut started = false;
    let result = (|| {
        cancel(0)?;
        let location = windows_fs::location(parent).map_err(io)?;
        let parent_security = security::owner_group_dacl_digest(parent)?;
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Read)?;
        validate_stage_entries(&snapshot.stage)?;
        started = snapshot.state.cleanup.is_some();
        let (bytes, observed, pending_absent, sequence, mut dependencies) =
            if let Some(progress) = &snapshot.state.cleanup {
                let remaining = Remaining::open(parent, &snapshot.state, progress, &mut cancel)?;
                let dependencies = Dependencies::open(
                    central,
                    snapshot.state.header.as_ref().unwrap(),
                    &progress.manifest,
                    &mut cancel,
                )?;
                snapshot.revalidate(parent, central)?;
                let observation = (
                    snapshot.bytes.clone(),
                    remaining.observed.clone(),
                    remaining.pending_absent,
                    snapshot.sequence,
                    dependencies,
                );
                drop(remaining);
                drop(snapshot);
                observation
            } else {
                drop(snapshot);
                let held = HeldCleanup::open(parent, central, name, &mut cancel)?;
                let manifest = held.snapshot.state.cleanup_manifest()?;
                let dependencies = Dependencies::open(
                    central,
                    held.snapshot.state.header.as_ref().unwrap(),
                    &manifest,
                    &mut cancel,
                )?;
                held.revalidate(&mut cancel)?;
                let observed = manifest
                    .entries
                    .iter()
                    .map(|entry| Some(entry.version.clone()))
                    .collect();
                (
                    held.snapshot.bytes.clone(),
                    observed,
                    false,
                    held.snapshot.sequence,
                    dependencies,
                )
            };
        cleanup_handoff(sequence)?;
        let snapshot = Snapshot::load(parent, central, name, SnapshotAccess::Write)?;
        validate_stage_entries(&snapshot.stage)?;
        if snapshot.bytes != bytes
            || windows_fs::location(parent).map_err(io)? != location
            || security::owner_group_dacl_digest(parent)? != parent_security
        {
            return Err(io(
                "Copy cleanup admission changed before writer acquisition",
            ));
        }
        let mut journal = Journal::from_snapshot(parent, central, snapshot)?;
        dependencies.revalidate_peers(central)?;
        let initial;
        let progress = if let Some(progress) = &journal.state.cleanup {
            progress
        } else {
            initial = Progress::new(journal.state.cleanup_manifest()?);
            &initial
        };
        let mut remaining = Remaining::open(parent, &journal.state, progress, &mut cancel)?;
        if remaining.observed != observed || remaining.pending_absent != pending_absent {
            return Err(io(
                "Copy cleanup retained epoch changed after read-only admission",
            ));
        }
        journal.reserve_cleanup_tail()?;
        if journal.state.cleanup.is_none() {
            let manifest = journal.state.cleanup_manifest()?;
            started = true;
            journal.persist(Record::Cleanup {
                entry: CleanupRecord::Begin {
                    owner: manifest.owner,
                    count: manifest.entries.len(),
                    digest: manifest.digest,
                },
            })?;
        }
        loop {
            let progress = journal.state.cleanup.as_ref().unwrap();
            if progress.payload_removed {
                return Ok(());
            }
            dependencies.validate(central)?;
            journal.reserve_cleanup_tail()?;
            if progress.completed == progress.manifest.entries.len() {
                Remaining::open(parent, &journal.state, progress, &mut cancel)?;
                journal.persist(Record::Cleanup {
                    entry: CleanupRecord::PayloadRemoved,
                })?;
                return Ok(());
            }
            let index = progress.manifest.order[progress.completed];
            remaining
                .acquire(parent, &journal.state, progress, index, &mut cancel)
                .map_err(|error| io(format!("Copy cleanup entry {index} acquisition: {error}")))?;
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
    result.map_err(|error: PlatformError| {
        if started {
            PlatformError::CleanupPending {
                transaction,
                message: format!("{error}; cleanup stopped, remaining data and journals retained"),
            }
        } else {
            PlatformError::CopyCleanupRefused(format!("{error}; {transaction}"))
        }
    })
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
