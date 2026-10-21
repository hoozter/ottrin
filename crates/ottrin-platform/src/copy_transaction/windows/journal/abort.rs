//! Explicitly end unverified staging without opening or removing any payload.
use super::super::registry::AnchoredRoot;
use super::*;
use std::path::{Path, PathBuf};

/// Read-only admission captures exact private controls. Ending the attempt writes
/// one paired terminal record, then uses the ordinary durable name-release path.
/// Payload contents, including later edits and unknown files, are never adopted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedCopyAbortPlan {
    registry: PathBuf,
    registry_anchor: WindowsStableIdentity,
    header: Box<Header>,
    journal: [u8; 32],
    claim: (EntryVersion, [u8; 32]),
    parent_location: PathBuf,
    parent_security: [u8; 32],
    complete: bool,
}
impl RetainedCopyAbortPlan {
    pub(crate) fn admit(
        root: &Path,
        id: &str,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let name = OsString::from(format!("{id}.copy-journal"));
        discovery::registry_id(&name).ok_or_else(|| io("invalid Copy transaction ID"))?;
        progress(0)?;
        let registry =
            AnchoredRoot::open_at(root, false)?.ok_or_else(|| io("Copy registry is missing"))?;
        let record = discovery::RegisteredJournal::open(registry.root(), &name)?;
        let header = record.state.header.as_ref().unwrap();
        let parent = discovery::Volumes::new(registry.root())?.resolve(
            header.parent,
            FILE_GENERIC_READ,
            &mut progress,
        )?;
        let snapshot = Snapshot::load(
            &parent,
            registry.root(),
            &header.stage_name(),
            SnapshotAccess::Read,
        )?;
        if snapshot.bytes != record.bytes || !snapshot.state.can_abort_staging() {
            return Err(io(
                "Copy is not an unverified staging attempt that can be ended",
            ));
        }
        snapshot.revalidate(&parent, registry.root())?;
        registry.validate()?;
        Ok(Self {
            registry: windows_fs::location(registry.root()).map_err(io)?,
            registry_anchor: registry.anchor_identity(),
            header: Box::new(header.clone()),
            journal: Sha256::digest(&snapshot.bytes).into(),
            claim: snapshot.claim.as_ref().unwrap().observation(),
            parent_location: windows_fs::location(&parent).map_err(io)?,
            parent_security: security::owner_group_dacl_digest(&parent)?,
            complete: false,
        })
    }
    pub fn transaction_id(&self) -> &str {
        &self.header.transaction
    }
    /// Conservative queue footprint: this action shares the destination's
    /// private claim namespace with fresh Copy admission.
    pub fn destination(&self) -> &Path {
        &self.parent_location
    }
    /// Captured publication name shown when confirming the end of this attempt.
    pub fn target(&self) -> PathBuf {
        self.parent_location
            .join(OsString::from_wide(&self.header.target))
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn end_attempt(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.complete {
            return Err(io("Copy staging attempt has already ended"));
        }
        progress(0)?;
        let registry = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Copy registry disappeared"))?;
        if registry.anchor_identity() != self.registry_anchor
            || stable_identity_from_handle(registry.root()).map_err(io)? != self.header.central
            || windows_fs::location(registry.root()).map_err(io)? != self.registry
        {
            return Err(io("Copy staging-end registry binding changed"));
        }
        let parent = discovery::Volumes::new(registry.root())?.resolve(
            self.header.parent,
            FILE_GENERIC_READ,
            &mut progress,
        )?;
        let snapshot = Snapshot::load(
            &parent,
            registry.root(),
            &self.header.stage_name(),
            SnapshotAccess::Write,
        )?;
        if snapshot.state.header.as_ref() != Some(self.header.as_ref())
            || !snapshot.state.can_abort_staging()
            || <[u8; 32]>::from(Sha256::digest(&snapshot.bytes)) != self.journal
            || snapshot.claim.as_ref().unwrap().observation() != self.claim
            || windows_fs::location(&parent).map_err(io)? != self.parent_location
            || security::owner_group_dacl_digest(&parent)? != self.parent_security
        {
            return Err(io(
                "Copy staging-end controls changed after admission; refresh status",
            ));
        }
        snapshot.revalidate(&parent, registry.root())?;
        registry.validate()?;
        progress(0)?;
        // No payload handle is opened, even if its recorded location now holds
        // unexpected or edited data. Only these leased controls grant authority.
        let mut journal = Journal::from_snapshot(&parent, registry.root(), snapshot)?;
        journal.persist(Record::StagingAborted)?;
        registry.validate()?;
        self.complete = true;
        Ok(())
    }
}

pub(crate) fn restore_abort_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyAbortPlan, PlatformError> {
    RetainedCopyAbortPlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_abort(id: &str) -> Result<RetainedCopyAbortPlan, PlatformError> {
    restore_abort_at(&super::super::registry::configured_path()?, id)
}
