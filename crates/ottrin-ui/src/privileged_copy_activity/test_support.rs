//! Mechanism probe for the UI adapter. Native pipe/UAC acceptance is separate.
use super::*;
use ottrin_platform::FileProperties;

pub(crate) struct ProbePlatform {
    pub(crate) commands: Mutex<Vec<PrivilegedCommand>>,
    status: PrivilegedStatus,
}

impl ProbePlatform {
    pub(crate) fn new(status: PrivilegedStatus) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            status,
        }
    }
}

impl PlatformOps for ProbePlatform {
    fn delete_path(&self, _: &Path, _: DeleteMode) -> Result<(), PlatformError> {
        panic!("Unexpected ambient filesystem mutation")
    }
    fn reveal_in_system(&self, _: &Path) -> Result<(), PlatformError> {
        panic!("Unexpected desktop operation")
    }
    fn open_with_application(&self, _: &Path, _: &Path) -> Result<(), PlatformError> {
        panic!("Unexpected desktop operation")
    }
    fn execute_command(&self, _: &FileCommand) -> Result<Option<FileProperties>, PlatformError> {
        panic!("Helper references must not use ambient filesystem authority")
    }
    fn execute_privileged(
        &self,
        _: &PrivilegedRequest,
    ) -> Result<ottrin_core::PrivilegedResponse, PlatformError> {
        panic!("Recovery must use the progress/Stop transport")
    }
    fn execute_privileged_with_progress(
        &self,
        request: &PrivilegedRequest,
        progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ottrin_core::PrivilegedResponse, PlatformError> {
        request
            .validate_protocol()
            .map_err(|error| PlatformError::Io(error.into()))?;
        progress(0)?;
        self.commands.lock().unwrap().push(request.command.clone());
        progress(23)?;
        Ok(ottrin_core::PrivilegedResponse {
            status: self.status,
            message: (self.status != PrivilegedStatus::Success)
                .then(|| "Owned helper refusal".into()),
            payload: None,
        })
    }
    fn privileged_availability(&self) -> PrivilegedAvailability {
        PrivilegedAvailability::Ready
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn restore_history(
    app: &mut OttrinApp,
    transaction: &str,
    recovery: ottrin_core::PrivilegedCopyHistoryState,
) -> u64 {
    let root = std::env::temp_dir().join("OttrinOwnedUiHistory");
    let entry = ottrin_core::PrivilegedCopyHistoryEntry {
        reference: ottrin_core::PrivilegedCopyReceipt {
            transactions: vec![transaction.into()],
            completed_sources: 0,
            paths: vec![(root.join("retained"), root.join("publication"))],
        },
        recovery,
    };
    validate_history(std::slice::from_ref(&entry)).unwrap();
    app.operation_history.clear();
    app.merge_privileged_copy_history(vec![entry]);
    app.operation_history[0].id
}

#[cfg(target_os = "windows")]
pub(crate) fn history_requested(app: &OttrinApp) -> bool {
    app.privileged_copy_history.loader.is_some()
}
