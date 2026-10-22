//! Non-destructive restart admission for a durably relinquished target name.
use super::super::registry::AnchoredRoot;
use super::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedCopyNameReleasePlan {
    registry: PathBuf,
    registry_anchor: WindowsStableIdentity,
    header: Box<Header>,
    journal: [u8; 32],
    claim: (EntryVersion, [u8; 32]),
    parent_location: PathBuf,
    parent_security: [u8; 32],
    phase: discovery::WindowsCopyPhase,
    complete: bool,
}
impl RetainedCopyNameReleasePlan {
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
        let parent = windows_fs::Volumes::new(registry.root())?.resolve(
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
        if snapshot.bytes != record.bytes
            || !snapshot.state.claim_terminal()
            || !snapshot
                .claim
                .as_ref()
                .is_some_and(claims::Lease::needs_release)
        {
            return Err(io("Copy has no stable terminal name release to finish"));
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
            phase: discovery::phase(&snapshot.state),
            complete: false,
        })
    }
    pub fn transaction_id(&self) -> &str {
        &self.header.transaction
    }
    pub fn admitted_phase(&self) -> discovery::WindowsCopyPhase {
        self.phase
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    /// Conservative queue footprint. The operation mutates only private claim
    /// bookkeeping, but shares this destination namespace with new admissions.
    pub fn destination(&self) -> &Path {
        &self.parent_location
    }
    pub fn finish(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.complete {
            return Err(io("Copy name release is already complete"));
        }
        progress(0)?;
        let registry = AnchoredRoot::open_at(&self.registry, false)?
            .ok_or_else(|| io("Copy registry disappeared"))?;
        if registry.anchor_identity() != self.registry_anchor
            || stable_identity_from_handle(registry.root()).map_err(io)? != self.header.central
            || windows_fs::location(registry.root()).map_err(io)? != self.registry
        {
            return Err(io("Copy name-release registry binding changed"));
        }
        let parent = windows_fs::Volumes::new(registry.root())?.resolve(
            self.header.parent,
            FILE_GENERIC_READ,
            &mut progress,
        )?;
        let snapshot = Snapshot::load(
            &parent,
            registry.root(),
            &self.header.stage_name(),
            SnapshotAccess::Read,
        )?;
        if snapshot.state.header.as_ref() != Some(self.header.as_ref())
            || !snapshot.state.claim_terminal()
            || !snapshot
                .claim
                .as_ref()
                .is_some_and(claims::Lease::needs_release)
            || <[u8; 32]>::from(Sha256::digest(&snapshot.bytes)) != self.journal
            || snapshot.claim.as_ref().unwrap().observation() != self.claim
            || windows_fs::location(&parent).map_err(io)? != self.parent_location
            || security::owner_group_dacl_digest(&parent)? != self.parent_security
        {
            return Err(io(
                "Copy name release changed after admission; refresh recovery status",
            ));
        }
        snapshot.revalidate(&parent, registry.root())?;
        registry.validate()?;
        progress(0)?;
        // Both journals remain read-leased against writes/deletion throughout
        // the claim handoff. No payload tree is opened or mutation API called.
        drop(Journal::from_snapshot(&parent, registry.root(), snapshot)?);
        registry.validate()?;
        self.complete = true;
        Ok(())
    }
}

pub(crate) fn restore_name_release_at(
    root: &Path,
    id: &str,
) -> Result<RetainedCopyNameReleasePlan, PlatformError> {
    RetainedCopyNameReleasePlan::admit(root, id, |_| Ok(()))
}
pub(crate) fn restore_name_release(id: &str) -> Result<RetainedCopyNameReleasePlan, PlatformError> {
    restore_name_release_at(&super::super::registry::configured_path()?, id)
}
