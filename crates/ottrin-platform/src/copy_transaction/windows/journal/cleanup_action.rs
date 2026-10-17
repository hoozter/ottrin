//! Configured, handle-free cleanup authority. Presentation and IDs alone never
//! authorize removal; each transition retains its admitted native-object proof.
use super::super::registry::{ConfiguredRegistry, configured_path};
use super::*;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;

#[derive(Clone, Debug)]
enum Action {
    Payload(cleanup::Admission),
    Started(cleanup::Admission),
    Completion(cleanup_finalize::Completion),
    Metadata(cleanup_finalize::Plan),
    Complete,
}

/// Read-only admission of one retained Windows Copy transaction under its
/// configured registry anchor. No files remain open while awaiting confirmation.
/// Independent/co-owned batch coordination is a separate admission responsibility.
#[derive(Clone, Debug)]
pub struct RetainedCopyCleanupPlan {
    registry: PathBuf,
    header: Box<Header>,
    admitted_phase: WindowsCopyPhase,
    recovery_location: PathBuf,
    action: Action,
}

impl RetainedCopyCleanupPlan {
    pub fn observe(
        transaction_id: &str,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        Self::observe_at(&configured_path()?, transaction_id, progress)
    }

    /// Existing-only anchored admission, also usable with an explicitly chosen
    /// profile. Never initializes missing state or adopts an unanchored registry.
    pub fn observe_at(
        root: &Path,
        transaction_id: &str,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let name = OsString::from(format!("{transaction_id}.copy-journal"));
        discovery::registry_id(&name).ok_or_else(|| io("invalid Copy transaction ID"))?;
        progress(0)?;
        let registry = ConfiguredRegistry::open_at(root, false)?
            .ok_or_else(|| io("Configured Copy registry is missing"))?;
        let central = registry.root();
        let control = discovery::RegisteredJournal::open(central, &name)?;
        let header = control.state.header.as_ref().unwrap().clone();
        let admitted_phase = discovery::phase(&control.state);
        let metadata = control
            .state
            .cleanup
            .as_ref()
            .is_some_and(|p| p.metadata_removal_intent);
        let digest: [u8; 32] = Sha256::digest(&control.bytes).into();
        drop(control);
        let parent = discovery::Volumes::new(central)?.resolve(
            header.parent,
            FILE_GENERIC_READ,
            &mut progress,
        )?;
        let action = if metadata {
            let plan = cleanup_finalize::Plan::observe(
                &parent,
                central,
                &header.stage_name(),
                &mut progress,
            )?;
            if plan.header() != &header {
                return Err(io("Copy cleanup metadata changed during admission"));
            }
            Action::Metadata(plan)
        } else {
            let plan =
                cleanup::Admission::observe(&parent, central, &header.stage_name(), &mut progress)?;
            if plan.header() != &header {
                return Err(io("Copy cleanup payload changed during admission"));
            }
            Action::Payload(plan)
        };
        let control = discovery::RegisteredJournal::open(central, &name)?;
        if <[u8; 32]>::from(Sha256::digest(&control.bytes)) != digest {
            return Err(io("Copy cleanup record changed during admission"));
        }
        registry.validate()?;
        Ok(Self {
            registry: registry.path().to_path_buf(),
            recovery_location: registry.path().join(name),
            header: Box::new(header),
            admitted_phase,
            action,
        })
    }

    pub fn transaction_id(&self) -> &str {
        &self.header.transaction
    }
    pub fn admitted_phase(&self) -> WindowsCopyPhase {
        self.admitted_phase
    }
    pub fn recovery_location(&self) -> &Path {
        &self.recovery_location
    }
    pub fn complete(&self) -> bool {
        matches!(self.action, Action::Complete)
    }

    /// Execute this admitted transaction only. On uncertain payload/intent
    /// writes, a fresh read-only admission is required. Once metadata removal
    /// begins, this object retains the proof needed to retry even after central
    /// unlink; a new admission cannot manufacture authority from absent IDs.
    pub fn reclaim(
        &mut self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.complete() {
            return Ok(());
        }
        let result = (|| {
            progress(0)?;
            let registry = ConfiguredRegistry::open_at(&self.registry, false)?
                .ok_or_else(|| io("Configured Copy registry is missing"))?;
            let central = registry.writable_root()?;
            if stable_identity_from_handle(&central).map_err(io)? != self.header.central {
                return Err(io("Copy cleanup registry was replaced"));
            }
            // The pinned manager deliberately does not share DELETE on its
            // root. Flushing/removing its children requires read/write, not
            // deletion rights on the registry directory itself.
            let parent = if self.header.parent.same_object(self.header.central) {
                central.try_clone().map_err(io)?
            } else {
                discovery::Volumes::new(&central)?.resolve(
                    self.header.parent,
                    FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                    &mut progress,
                )?
            };
            if let Action::Payload(plan) = &self.action {
                self.action = Action::Started(plan.start(&parent, &central, &mut progress)?);
            }
            if let Action::Started(plan) = &self.action {
                self.action = Action::Completion(plan.reclaim(&parent, &central, &mut progress)?);
            }
            if let Action::Completion(completion) = &self.action {
                self.action =
                    Action::Metadata(completion.begin(&parent, &central, &mut progress)?);
            }
            if let Action::Metadata(plan) = &self.action {
                plan.finish(&parent, &central, &mut progress)?;
                registry.validate()?;
                self.action = Action::Complete;
            }
            Ok(())
        })();
        result.map_err(|error| match error {
            PlatformError::CleanupPending { .. } | PlatformError::CopyCleanupRefused(_) => error,
            error
                if self.admitted_phase == WindowsCopyPhase::CleaningUp
                    || !matches!(self.action, Action::Payload(_)) =>
            {
                PlatformError::CleanupPending {
                    transaction: self.recovery_location.display().to_string(),
                    message: format!("{error}; remaining Copy recovery data retained"),
                }
            }
            error => PlatformError::CopyCleanupRefused(error.to_string()),
        })
    }
}
