//! Activity retains references to elevated Copy journals. Only the helper can
//! reopen their filesystem authority; the ordinary UI never reads root journals.
use super::*;

pub(super) fn execute_recovery(
    platform: &dyn PlatformOps,
    command: PrivilegedCommand,
) -> Result<(), PlatformError> {
    let response = platform.execute_privileged(&PrivilegedRequest {
        command,
        context: PrivilegedContext {
            reason: Some("Manage the retained administrator Copy operation".into()),
            cwd: None,
        },
    })?;
    if response.status == PrivilegedStatus::Success {
        Ok(())
    } else {
        Err(PlatformError::Io(response.message.unwrap_or_else(|| {
            "Administrator Copy recovery was refused; retained data is preserved.".into()
        })))
    }
}

pub(super) fn is_privileged(record: &OperationRecord) -> bool {
    matches!(
        record
            .path_move_undo_token
            .as_ref()
            .map(|token| &token.receipt),
        Some(OperationUndoReceipt::CopyBatch(
            CopyBatchUndoReceipt::Privileged { .. }
        ))
    )
}

impl OttrinApp {
    pub(super) fn record_privileged_copy(
        &mut self,
        command: &FileCommand,
        response: &ottrin_core::PrivilegedResponse,
        tab: Option<u64>,
        bindings: Vec<StorageOperationBinding>,
    ) -> Result<(), String> {
        let FileCommand::Copy {
            sources,
            destination,
            ..
        } = command
        else {
            return Ok(());
        };
        let Some(PrivilegedPayload::Copy(reference)) = &response.payload else {
            // Cancellation/denial may occur before the helper starts a Copy.
            return if response.status == PrivilegedStatus::Success {
                Err("Administrator helper omitted Copy recovery references; review retained data before retrying.".into())
            } else {
                Ok(())
            };
        };
        if reference.completed_sources > sources.len()
            || reference.transactions.len() != reference.paths.len()
            || reference.transactions.iter().collect::<HashSet<_>>().len()
                != reference.transactions.len()
            || reference
                .paths
                .iter()
                .any(|(_, publication)| publication.parent() != Some(destination.as_path()))
        {
            return Err("Administrator helper returned inconsistent Copy recovery references; retained data requires review.".into());
        }
        let id = self.queue_operation_record(command.clone());
        let completed = response.status == PrivilegedStatus::Success;
        self.finish_operation_record(
            id,
            if completed {
                OperationStatus::Completed
            } else if reference.completed_sources > 0 {
                OperationStatus::Stopped
            } else {
                OperationStatus::Failed
            },
            response.message.clone(),
        );
        let record = self
            .operation_history
            .iter_mut()
            .find(|record| record.id == id)
            .expect("new administrator Copy record");
        record.title = "Copy with administrator privileges".into();
        if !reference.transactions.is_empty() {
            record.undo_kind = Some(PathMoveUndoKind::Copy);
            record.path_move_undo = Some(PathMoveUndoState::Available);
            record.path_move_undo_token = Some(PathMoveUndoToken {
                origin_tab_id: tab,
                storage_bindings: bindings
                    .into_iter()
                    .filter(|binding| destination.starts_with(&binding.root))
                    .collect(),
                receipt: OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
                    reference: reference.clone(),
                    pending: true,
                }),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(status: PrivilegedStatus, completed: usize) -> ottrin_core::PrivilegedResponse {
        ottrin_core::PrivilegedResponse {
            status,
            message: (status != PrivilegedStatus::Success).then(|| "Later Copy item failed".into()),
            payload: Some(PrivilegedPayload::Copy(
                ottrin_core::PrivilegedCopyReceipt {
                    completed_sources: completed,
                    transactions: vec!["a".repeat(32)],
                    paths: vec![(
                        PathBuf::from("/owned/output/.ottrin-txn-a/new"),
                        PathBuf::from("/owned/output/a"),
                    )],
                },
            )),
        }
    }
    fn command() -> FileCommand {
        FileCommand::Copy {
            sources: vec![
                PathBuf::from("/owned/input/a"),
                PathBuf::from("/owned/input/b"),
            ],
            destination: PathBuf::from("/owned/output"),
            conflict: ConflictAction::Overwrite,
        }
    }

    #[test]
    fn administrator_copy_activity_retains_undo_and_cleanup_ownership_after_success_or_partial_failure()
     {
        for status in [PrivilegedStatus::Success, PrivilegedStatus::Failed] {
            let mut app = OttrinApp::default();
            app.operation_history.clear();
            app.record_privileged_copy(&command(), &response(status, 1), Some(7), Vec::new())
                .unwrap();
            let record = app.operation_history.front().unwrap();
            assert_eq!(record.title, "Copy with administrator privileges");
            assert!(is_privileged(record));
            assert!(path_move_undo_action_is_available(
                record.status,
                record.path_move_undo.as_ref()
            ));
            assert!(copy_activity_cleanup::can_cleanup(record));
            assert_eq!(copy_receipt_transaction_ids(record), vec!["a".repeat(32)]);
            assert_eq!(
                record.path_move_undo_token.as_ref().unwrap().origin_tab_id,
                Some(7)
            );
            let token = record.path_move_undo_token.as_ref().unwrap();
            assert_eq!(
                token.receipt.path_pairs()[0].1,
                Path::new("/owned/output/a")
            );
            app.clear_finished_activity();
            assert_eq!(
                app.operation_history.len(),
                1,
                "Clear history cannot abandon elevated Copy ownership"
            );
            assert_eq!(
                app.operation_history.front().unwrap().status,
                if status == PrivilegedStatus::Success {
                    OperationStatus::Completed
                } else {
                    OperationStatus::Stopped
                }
            );
        }
    }

    #[test]
    fn administrator_copy_rejects_missing_or_inconsistent_success_payload() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let mut invalid = response(PrivilegedStatus::Success, 1);
        invalid.payload = None;
        assert!(
            app.record_privileged_copy(&command(), &invalid, None, Vec::new())
                .is_err()
        );
        invalid = response(PrivilegedStatus::Success, 3);
        assert!(
            app.record_privileged_copy(&command(), &invalid, None, Vec::new())
                .is_err()
        );
        invalid = response(PrivilegedStatus::Success, 1);
        let Some(PrivilegedPayload::Copy(reference)) = &mut invalid.payload else {
            unreachable!()
        };
        reference.paths[0].1 = PathBuf::from("/unrelated/a");
        assert!(
            app.record_privileged_copy(&command(), &invalid, None, Vec::new())
                .is_err()
        );
        assert!(app.operation_history.is_empty());
    }

    #[test]
    fn administrator_copy_denial_creates_no_fictitious_completed_activity() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let denied = ottrin_core::PrivilegedResponse {
            status: PrivilegedStatus::Denied,
            message: Some("Cancelled".into()),
            payload: None,
        };
        app.record_privileged_copy(&command(), &denied, None, Vec::new())
            .unwrap();
        assert!(app.operation_history.is_empty());
    }
}
