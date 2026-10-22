//! Queue-time recovery evidence and its complete mutation footprint. A plan
//! never grants authority from paths: execution still uses the native engine.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Evidence {
    Same(PathBuf, Option<Box<WindowsOverwriteRecord>>),
    Cross(PathBuf, Option<Box<WindowsCrossVolumeRecord>>),
}

/// Read-only recovery admission. Private evidence binds the worker to exactly
/// what Activity observed; exposed paths are for scheduling and drive admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsMoveRecoveryPlan {
    root: PathBuf,
    root_identity: move_recovery::WindowsStableIdentity,
    transaction_id: [u8; 16],
    evidence: Vec<Evidence>,
    paths: Vec<PathBuf>,
    undo_status: move_recovery::WindowsMoveUndoStatus,
}

impl WindowsMoveRecoveryPlan {
    /// Captures existing journals without creating or modifying recovery state.
    pub fn capture(transaction_id: [u8; 16]) -> Result<Self, PlatformError> {
        let root = windows_recovery_root()?;
        let root_identity = move_recovery::stable_identity_of_entry(&root)
            .map_err(|e| PlatformError::Io(e.to_string()))?;
        if !root_identity.is_directory_without_reparse() {
            return Err(PlatformError::Io(
                "Move recovery root is not an ordinary directory".into(),
            ));
        }
        let mut plan = Self {
            paths: vec![root.clone()],
            root,
            root_identity,
            transaction_id,
            evidence: Vec::new(),
            undo_status: move_recovery::WindowsMoveUndoStatus::NotStarted,
        };
        let mut visited = std::collections::BTreeSet::new();
        plan.capture_owner(transaction_id, &mut visited)?;
        plan.undo_status = plan.observe_undo_status()?;
        plan.paths.sort();
        plan.paths.dedup();
        Ok(plan)
    }

    pub fn transaction_id(&self) -> [u8; 16] {
        self.transaction_id
    }

    pub fn undo_status(&self) -> move_recovery::WindowsMoveUndoStatus {
        self.undo_status
    }

    /// All affected parents, private stages and the central journal namespace.
    /// Recovery may select another candidate name within a destination parent.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The captured native Undo observation, only for the exact session receipt
    /// that owns this inverse. This performs no I/O on the UI thread. Matching
    /// public paths never suffices, and a forward recovery never completes Undo.
    pub fn undo_status_for(
        &self,
        receipt: &crate::PathMoveUndoReceipt,
    ) -> Option<move_recovery::WindowsMoveUndoStatus> {
        use windows_undo::WindowsMoveUndoGuard;
        if self.undo_status == move_recovery::WindowsMoveUndoStatus::NotStarted {
            return None;
        }
        let crate::WindowsPathUndo::Move(guard) = receipt.windows_undo.as_ref()? else {
            return None;
        };
        self.evidence
            .iter()
            .any(|evidence| match (guard, evidence) {
                (WindowsMoveUndoGuard::Replace(expected), Evidence::Same(_, Some(record))) => {
                    expected.same_transaction_evidence(record)
                }
                (WindowsMoveUndoGuard::CrossVolume(expected), Evidence::Cross(_, Some(record))) => {
                    expected.matches_recovery_record(record)
                }
                _ => false,
            })
            .then_some(self.undo_status)
    }

    fn observe_undo_status(&self) -> Result<move_recovery::WindowsMoveUndoStatus, PlatformError> {
        use move_recovery::WindowsMoveUndoStatus;
        for evidence in &self.evidence {
            match evidence {
                Evidence::Same(path, Some(record))
                    if *path == windows_journal_path(&self.root, self.transaction_id) =>
                {
                    let local_path = record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME);
                    let local = self.evidence.iter().find_map(|entry| match entry {
                        Evidence::Same(path, Some(local)) if *path == local_path => {
                            Some(local.as_ref())
                        }
                        _ => None,
                    });
                    if let Some(local) = local
                        && windows_undo::overwrite::has_inverse_intent(record, local)
                    {
                        return Ok(
                            if windows_undo::overwrite::completed(&self.root, record, local)? {
                                WindowsMoveUndoStatus::Complete
                            } else {
                                WindowsMoveUndoStatus::Pending
                            },
                        );
                    }
                    return Ok(WindowsMoveUndoStatus::NotStarted);
                }
                Evidence::Cross(path, Some(record))
                    if *path
                        == windows_cross_volume_journal_path(&self.root, self.transaction_id) =>
                {
                    return if record.undo_origin.is_some() {
                        windows_undo::cross::status(&self.root, record)
                    } else if windows_undo::cross::retained::has_intent(record) {
                        windows_undo::cross::retained::controller_status(&self.root, record)
                    } else {
                        Ok(WindowsMoveUndoStatus::NotStarted)
                    };
                }
                _ => {}
            }
        }
        Err(PlatformError::Io(
            "Move recovery observation is missing its owner".into(),
        ))
    }

    fn capture_owner(
        &mut self,
        id: [u8; 16],
        visited: &mut std::collections::BTreeSet<[u8; 16]>,
    ) -> Result<(), PlatformError> {
        if !visited.insert(id) {
            return Ok(());
        }
        if visited.len() > 2 {
            return Err(PlatformError::Io(
                "Move recovery owner chain is invalid".into(),
            ));
        }
        let same = windows_journal_path(&self.root, id);
        let cross = windows_cross_volume_journal_path(&self.root, id);
        let exists = |path: &Path| {
            path.try_exists()
                .map_err(|e| PlatformError::Io(e.to_string()))
        };
        match (exists(&same)?, exists(&cross)?) {
            (true, false) => {
                let record = load_windows_overwrite_record(&same)?;
                if record.transaction_id()? != id {
                    return Err(PlatformError::Io(
                        "Move recovery transaction ID changed".into(),
                    ));
                }
                self.add_paths(
                    &record.source_path()?,
                    &record.destination_parent_path()?,
                    &record.transaction_path()?,
                )?;
                let local_path = record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME);
                let local = exists(&local_path)?
                    .then(|| load_windows_overwrite_record(&local_path))
                    .transpose()?;
                self.evidence
                    .push(Evidence::Same(same, Some(Box::new(record))));
                self.evidence
                    .push(Evidence::Same(local_path, local.map(Box::new)));
            }
            (false, true) => {
                let record = load_windows_cross_volume_record(&cross)?;
                if record.transaction_id()? != id {
                    return Err(PlatformError::Io(
                        "Move recovery transaction ID changed".into(),
                    ));
                }
                self.add_paths(
                    &record.source_path()?,
                    &record.destination_parent_path()?,
                    &record.transaction_path()?,
                )?;
                let local_path = record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME);
                let local = exists(&local_path)?
                    .then(|| load_windows_cross_volume_record(&local_path))
                    .transpose()?;
                let linked = if record.undo_origin.is_some() {
                    windows_undo::cross::retained::presentation_controller(&self.root, &record)?
                } else {
                    record.undo_transaction
                };
                self.evidence
                    .push(Evidence::Cross(cross, Some(Box::new(record))));
                self.evidence
                    .push(Evidence::Cross(local_path, local.map(Box::new)));
                if let Some(linked) = linked {
                    self.capture_owner(linked, visited)?;
                }
            }
            (true, true) => {
                return Err(PlatformError::Io(
                    "Multiple Move journal families share this transaction ID".into(),
                ));
            }
            (false, false) => {
                return Err(PlatformError::Io(
                    "Move recovery transaction was not found".into(),
                ));
            }
        }
        Ok(())
    }

    fn add_paths(
        &mut self,
        source: &Path,
        destination: &Path,
        transaction: &Path,
    ) -> Result<(), PlatformError> {
        let source_parent = source
            .parent()
            .ok_or_else(|| PlatformError::Io("Move recovery source has no parent".into()))?;
        for path in [source_parent, destination, transaction] {
            if !path.is_absolute() {
                return Err(PlatformError::Io(
                    "Move recovery path is not absolute".into(),
                ));
            }
            self.paths.push(path.to_owned());
        }
        Ok(())
    }

    /// Revalidates under the existing shared recovery lock, then invokes the
    /// ordinary engine. A stale plan never updates itself from new evidence.
    pub fn execute(&self) -> Result<(), PlatformError> {
        let _lock = acquire_windows_recovery_lock()?;
        if windows_recovery_root()? != self.root
            || move_recovery::stable_identity_of_entry(&self.root)
                .map_err(|e| PlatformError::Io(e.to_string()))?
                != self.root_identity
        {
            return Err(PlatformError::Io(
                "Move recovery root changed; refresh Activity".into(),
            ));
        }
        for evidence in &self.evidence {
            let unchanged = match evidence {
                Evidence::Same(path, expected) => {
                    path.try_exists()
                        .map_err(|e| PlatformError::Io(e.to_string()))?
                        .then(|| load_windows_overwrite_record(path))
                        .transpose()?
                        .as_ref()
                        == expected.as_deref()
                }
                Evidence::Cross(path, expected) => {
                    path.try_exists()
                        .map_err(|e| PlatformError::Io(e.to_string()))?
                        .then(|| load_windows_cross_volume_record(path))
                        .transpose()?
                        .as_ref()
                        == expected.as_deref()
                }
            };
            if !unchanged {
                return Err(PlatformError::Io(
                    "Move recovery evidence changed; refresh Activity".into(),
                ));
            }
        }
        recover_retained_windows_move_transaction_at(&self.root, self.transaction_id)
    }
}
