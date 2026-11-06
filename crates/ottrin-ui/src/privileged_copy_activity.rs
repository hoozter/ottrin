//! Activity retains references to elevated Copy journals. Only the helper can
//! reopen their filesystem authority; the ordinary UI never reads root journals.
use super::*;
use ottrin_core::PrivilegedCopyHistoryState;

type HistoryResult = Result<Vec<ottrin_core::PrivilegedCopyHistoryEntry>, String>;

#[derive(Default)]
pub(super) struct HistoryUi {
    loader: Option<RetainedActivityLoader<HistoryResult>>,
    message: Option<String>,
    review: Vec<String>,
    cancellation: Option<Arc<AtomicBool>>,
}

impl Drop for HistoryUi {
    fn drop(&mut self) {
        if let Some(flag) = &self.cancellation {
            flag.store(true, Ordering::Release);
        }
    }
}

fn load_history(platform: &dyn PlatformOps, cancelled: &AtomicBool) -> HistoryResult {
    let mut progress = |_| {
        if cancelled.load(Ordering::Acquire) {
            Err(PlatformError::Io(
                "Administrator Copy history check stopped; existing activity is unchanged.".into(),
            ))
        } else {
            Ok(())
        }
    };
    let response = platform
        .execute_privileged_with_progress(
            &PrivilegedRequest {
                protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
                command: PrivilegedCommand::DiscoverCopyHistory,
                context: PrivilegedContext {
                    reason: Some("Load administrator Copy history for recovery".into()),
                    cwd: None,
                },
            },
            &mut progress,
        )
        .map_err(|error| error.to_string())?;
    progress(0).map_err(|error| error.to_string())?;
    if response.status != PrivilegedStatus::Success {
        return Err(response.message.unwrap_or_else(|| {
            "Administrator Copy history was not loaded. Retained data is unchanged.".into()
        }));
    }
    let Some(PrivilegedPayload::CopyHistory(history)) = response.payload else {
        return Err("Administrator helper omitted Copy history. Update the application and helper together.".into());
    };
    validate_history(&history)?;
    Ok(history)
}

fn validate_history(history: &[ottrin_core::PrivilegedCopyHistoryEntry]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for entry in history {
        let reference = &entry.reference;
        let windows_state = matches!(
            entry.recovery,
            PrivilegedCopyHistoryState::RecoveryAvailable { .. }
                | PrivilegedCopyHistoryState::AttemptEnded
        );
        let unrecognized = cfg!(target_os = "windows")
            && reference.transactions.is_empty()
            && reference.paths.is_empty()
            && matches!(
                entry.recovery,
                PrivilegedCopyHistoryState::NeedsReview { .. }
            );
        if (windows_state && !cfg!(target_os = "windows"))
            || (reference.transactions.is_empty() && !unrecognized)
            || (matches!(
                entry.recovery,
                PrivilegedCopyHistoryState::RecoveryAvailable { .. }
            ) && reference.transactions.len() != 1)
            || reference.paths.len() > reference.transactions.len()
            || reference.transactions.iter().any(|id| {
                id.len() != 32
                    || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
                    || !seen.insert(id)
            })
            || reference
                .paths
                .iter()
                .any(|(source, target)| !source.is_absolute() || !target.is_absolute())
            || (matches!(&entry.recovery, PrivilegedCopyHistoryState::UndoAvailable)
                && reference.paths.len() != reference.transactions.len())
        {
            return Err("Administrator helper returned inconsistent Copy history. Retained data is unchanged.".into());
        }
    }
    Ok(())
}

impl OttrinApp {
    pub(super) fn render_privileged_copy_history(&mut self, ui: &mut egui::Ui) {
        let busy = self.copy_cleanup.running()
            || self.operation_history.iter().any(|record| {
                record.status == OperationStatus::Queued
                    || matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
            });
        #[cfg(target_os = "windows")]
        let busy = busy
            || self.copy_recovery.running()
            || self.native_undo_preparation.is_some()
            || self.copy_cleanup.retains_plan();
        let loading = self
            .privileged_copy_history
            .loader
            .as_ref()
            .is_some_and(|loader| loader.pending);
        if ui.add_enabled(!busy && !loading, egui::Button::new(if loading {
            "Loading administrator Copy history…"
        } else {
            "Load administrator Copy history"
        }).themed(&self.colors))
            .on_hover_text("Requests administrator permission to find Copy recovery records from previous sessions. This does not change files.")
            .clicked()
        {
            let cancellation = self.privileged_copy_history.cancellation
                .get_or_insert_with(|| Arc::new(AtomicBool::new(false))).clone();
            cancellation.store(false, Ordering::Release);
            if self.privileged_copy_history.loader.is_none() {
                let platform = self.platform.clone();
                self.privileged_copy_history.loader = Some(RetainedActivityLoader::spawn_with(move || load_history(platform.as_ref(), &cancellation)));
            }
            self.privileged_copy_history.message = self.privileged_copy_history.loader.as_mut().unwrap().request().err().map(str::to_owned);
        }
        if loading {
            let stopping = self
                .privileged_copy_history
                .cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire));
            if ui
                .add_enabled(
                    !stopping,
                    egui::Button::new(if stopping {
                        "Cancelling history load…"
                    } else {
                        "Cancel loading history"
                    })
                    .themed(&self.colors),
                )
                .clicked()
                && let Some(flag) = &self.privileged_copy_history.cancellation
            {
                flag.store(true, Ordering::Release);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !busy {
            let result = self
                .privileged_copy_history
                .loader
                .as_mut()
                .and_then(RetainedActivityLoader::poll);
            if let Some(result) = result {
                self.privileged_copy_history.message = Some(match result {
                    Ok(Ok(history)) => {
                        self.merge_privileged_copy_history(history);
                        "Administrator Copy history loaded. Undo and clearing retained data may request authentication again.".into()
                    }
                    Ok(Err(error)) => error,
                    Err(error) => error.into(),
                });
            }
        }
        if let Some(message) = &self.privileged_copy_history.message {
            ui.label(RichText::new(message).color(self.colors.text_muted));
        }
        for reason in &self.privileged_copy_history.review {
            ui.label(RichText::new(reason).color(self.colors.error));
        }
    }

    fn merge_privileged_copy_history(
        &mut self,
        history: Vec<ottrin_core::PrivilegedCopyHistoryEntry>,
    ) {
        self.privileged_copy_history.review.clear();
        // Only a successfully merged fresh cohort can renew a restored action.
        // Missing or refused entries never prove deletion, Undo or completion.
        let previous = self
            .operation_history
            .iter()
            .filter(|record| {
                is_restored(record)
                    && record.status != OperationStatus::Queued
                    && record.path_move_undo != Some(PathMoveUndoState::UndoQueued)
                    && !self.copy_cleanup.owns(record)
            })
            .map(|record| record.id)
            .collect::<HashSet<_>>();
        let mut refreshed = HashSet::new();
        for entry in history {
            if entry.reference.transactions.is_empty() {
                if let PrivilegedCopyHistoryState::NeedsReview { reason } = entry.recovery {
                    self.privileged_copy_history.review.push(reason);
                }
                continue;
            }
            // Only refresh an exact restored cohort. Live receipts and cleanup
            // workers retain their own progress and must never lose ownership.
            let overlapping = self
                .operation_history
                .iter()
                .filter(|record| {
                    history_ids(record)
                        .iter()
                        .any(|id| entry.reference.transactions.contains(id))
                })
                .collect::<Vec<_>>();
            let same_cohort = overlapping.is_empty()
                || matches!(entry.recovery, PrivilegedCopyHistoryState::CleanupPending)
                || (overlapping.len() == 1
                    && history_ids(overlapping[0]).iter().collect::<HashSet<_>>()
                        == entry.reference.transactions.iter().collect::<HashSet<_>>());
            let replace = same_cohort
                && overlapping.iter().all(|record| {
                    let replaceable = replaceable_history(record);
                    #[cfg(target_os = "windows")]
                    let replaceable = replaceable
                        || (matches!(entry.recovery, PrivilegedCopyHistoryState::CleanupPending)
                            && record.retained_copy.as_ref().is_some_and(|owner| {
                                owner.restored && owner.finalization.is_none()
                            }));
                    replaceable
                        && !self.copy_cleanup.owns(record)
                        && record.path_move_undo != Some(PathMoveUndoState::UndoQueued)
                        && record.status != OperationStatus::Queued
                        && history_ids(record)
                            .iter()
                            .all(|id| entry.reference.transactions.contains(id))
                });
            if !replace {
                let reason = "Administrator history overlaps an existing owned Copy activity. Finish or review that activity before reloading history.";
                if !self
                    .privileged_copy_history
                    .review
                    .iter()
                    .any(|item| item == reason)
                {
                    self.privileged_copy_history.review.push(reason.into());
                }
                continue;
            }
            let existing = overlapping.first().map(|record| record.id);
            let replaced = overlapping
                .iter()
                .skip(1)
                .map(|record| record.id)
                .collect::<HashSet<_>>();
            // A fresh canonical cleanup cohort may combine several inactive
            // restored cards. Keep its entire membership in one card; current-
            // session receipts and active workers were excluded above.
            self.operation_history
                .retain(|record| !replaced.contains(&record.id));
            let detail = if entry.reference.paths.is_empty() {
                "Retained administrator Copy bookkeeping".to_owned()
            } else {
                entry
                    .reference
                    .paths
                    .iter()
                    .map(|(_, path)| path_label(path))
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            let id = existing.unwrap_or_else(|| {
                self.queue_activity_record_with_recoverability(
                    "Administrator Copy from previous session",
                    detail.clone(),
                    OperationRecoverability::Restorable,
                )
            });
            refreshed.insert(id);
            let available = matches!(&entry.recovery, PrivilegedCopyHistoryState::UndoAvailable);
            self.finish_operation_record(
                id,
                if matches!(
                    &entry.recovery,
                    PrivilegedCopyHistoryState::NeedsReview { .. }
                        | PrivilegedCopyHistoryState::RecoveryAvailable { .. }
                        | PrivilegedCopyHistoryState::AttemptEnded
                        | PrivilegedCopyHistoryState::CleanupPending
                ) {
                    OperationStatus::NeedsReview
                } else {
                    OperationStatus::Completed
                },
                None,
            );
            let record = self
                .operation_history
                .iter_mut()
                .find(|record| record.id == id)
                .unwrap();
            record.detail = detail;
            record.operation_paths = entry
                .reference
                .paths
                .iter()
                .map(|(_, path)| OperationPath::write(path.clone()))
                .collect();
            record.finished_at = None; // Original operation timing is not in the snapshot.
            record.undo_kind = Some(PathMoveUndoKind::Copy);
            #[cfg(target_os = "windows")]
            {
                use ottrin_platform::WindowsCopyPhase as Phase;
                let recovery = match &entry.recovery {
                    PrivilegedCopyHistoryState::RecoveryAvailable { action } => {
                        Some(CopyRecoveryAction::Administrator {
                            transaction: entry.reference.transactions[0].clone(),
                            action: *action,
                            paths: entry.reference.paths.clone(),
                        })
                    }
                    _ => None,
                };
                record.retained_copy = Some(RetainedCopyOwnership {
                    key: format!("administrator:{}", entry.reference.transactions.join(":")),
                    transactions: entry.reference.transactions.clone(),
                    blocked_by: Vec::new(),
                    restored: true,
                    finalization: None,
                    recovery,
                    cleanup_phase: match entry.recovery {
                        PrivilegedCopyHistoryState::UndoAvailable => Some(Phase::Completed),
                        PrivilegedCopyHistoryState::Undone => Some(Phase::Undone),
                        PrivilegedCopyHistoryState::CleanupPending => Some(Phase::CleaningUp),
                        _ => None,
                    },
                    recovery_location: None,
                });
            }
            record.path_move_undo = Some(match entry.recovery {
                PrivilegedCopyHistoryState::UndoAvailable => PathMoveUndoState::Available,
                PrivilegedCopyHistoryState::Undone => PathMoveUndoState::Undone,
                PrivilegedCopyHistoryState::NeedsReview { reason } => {
                    PathMoveUndoState::Unavailable { reason }
                }
                PrivilegedCopyHistoryState::RecoveryAvailable { .. } => PathMoveUndoState::Unavailable { reason: "Use this activity's recovery action. The administrator helper rechecks the retained operation before changing files.".into() },
                PrivilegedCopyHistoryState::AttemptEnded => PathMoveUndoState::Unavailable { reason: "Copy attempt ended; partial files and recovery records are retained.".into() },
                PrivilegedCopyHistoryState::CleanupPending => PathMoveUndoState::Unavailable { reason: "Copy cleanup was interrupted. Undo is revoked; finish clearing retained data.".into() },
            });
            record.path_move_undo_token = Some(PathMoveUndoToken {
                origin_tab_id: None,
                storage_bindings: Vec::new(),
                receipt: OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
                    reference: entry.reference,
                    pending: available,
                    restored: true,
                }),
            });
        }
        for record in &mut self.operation_history {
            if !previous.contains(&record.id) || refreshed.contains(&record.id) {
                continue;
            }
            let reason = "The latest administrator history did not confirm this retained Copy operation. Its references are preserved; reload administrator history before recovery.";
            record.status = OperationStatus::NeedsReview;
            record.path_move_undo = Some(PathMoveUndoState::Unavailable {
                reason: reason.into(),
            });
            record.message = Some(reason.into());
            if let Some(PathMoveUndoToken {
                receipt:
                    OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
                        pending, ..
                    }),
                ..
            }) = &mut record.path_move_undo_token
            {
                *pending = false;
            }
            #[cfg(target_os = "windows")]
            if let Some(owner) = &mut record.retained_copy {
                owner.recovery = None;
                owner.cleanup_phase = None;
            }
        }
    }
}

pub(super) fn execute_recovery(
    platform: &dyn PlatformOps,
    command: PrivilegedCommand,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    let response = platform.execute_privileged_with_progress(
        &PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command,
            context: PrivilegedContext {
                reason: Some("Manage the retained administrator Copy operation".into()),
                cwd: None,
            },
        },
        progress,
    )?;
    if response.status == PrivilegedStatus::Success {
        Ok(())
    } else {
        Err(PlatformError::Io(response.message.unwrap_or_else(|| {
            "Administrator Copy recovery was refused; retained data is preserved.".into()
        })))
    }
}

fn history_ids(record: &OperationRecord) -> Vec<String> {
    record
        .retained_copy
        .as_ref()
        .map(|owner| owner.transactions.clone())
        .unwrap_or_else(|| copy_receipt_transaction_ids(record))
}

fn replaceable_history(record: &OperationRecord) -> bool {
    if is_restored(record) {
        return true;
    }
    #[cfg(target_os = "windows")]
    if record.path_move_undo_token.is_none()
        && matches!(
            record.path_move_undo,
            Some(PathMoveUndoState::Unavailable { .. })
        )
        && record.retained_copy.as_ref().is_some_and(|owner| {
            owner.restored && owner.recovery.is_none() && owner.finalization.is_none()
        })
    {
        return true;
    }
    false
}

pub(super) fn is_restored(record: &OperationRecord) -> bool {
    matches!(
        record
            .path_move_undo_token
            .as_ref()
            .map(|token| &token.receipt),
        Some(OperationUndoReceipt::CopyBatch(
            CopyBatchUndoReceipt::Privileged { restored: true, .. }
        ))
    )
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
                    restored: false,
                }),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    mod history;
    #[test]
    fn rendered_loaded_message_does_not_keep_a_second_stale_record_count_after_cleanup() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let mut loader = RetainedActivityLoader::spawn_with(move || {
            Ok(vec![ottrin_core::PrivilegedCopyHistoryEntry {
                reference: reference.clone(),
                recovery: PrivilegedCopyHistoryState::UndoAvailable,
            }])
        });
        loader.request().unwrap();
        app.privileged_copy_history.loader = Some(loader);
        let ctx = Context::default();
        let frame = |app: &mut OttrinApp| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(480.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.render_privileged_copy_history(ui));
                },
            )
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.operation_history.is_empty() {
            frame(&mut app);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.operation_history.len(), 1);
        // Successful cleanup releases the last card. The informational load
        // outcome must not contradict the authoritative live Activity count.
        app.operation_history.clear();
        let output = frame(&mut app);
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text.starts_with("Administrator Copy history loaded.")))
        );
        assert!(
            !output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text.contains("Found 1")))
        );
    }

    #[test]
    fn rendered_history_control_is_opt_in_and_cancelled_loading_preserves_activity() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        app.record_privileged_copy(
            &command(),
            &response(PrivilegedStatus::Success, 1),
            None,
            Vec::new(),
        )
        .unwrap();
        let original = app.operation_history.front().unwrap().id;
        let ctx = Context::default();
        let frame = |app: &mut OttrinApp| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(480.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.render_privileged_copy_history(ui));
                },
            )
        };
        let output = frame(&mut app);
        assert!(
            app.privileged_copy_history.loader.is_none(),
            "Rendering Activity must never request authentication"
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Load administrator Copy history")));
        let mut loader =
            RetainedActivityLoader::spawn_with(|| Err("Authentication cancelled".into()));
        loader.request().unwrap();
        app.privileged_copy_history.loader = Some(loader);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.privileged_copy_history.message.is_none() {
            frame(&mut app);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            app.privileged_copy_history.message.as_deref(),
            Some("Authentication cancelled")
        );
        assert_eq!(app.operation_history.len(), 1);
        assert_eq!(app.operation_history.front().unwrap().id, original);
        assert!(copy_activity_cleanup::can_cleanup(
            app.operation_history.front().unwrap()
        ));
    }

    #[test]
    fn previous_session_administrator_history_keeps_recovery_ownership_without_duplicate_rows() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let history = vec![ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        }];
        validate_history(&history).unwrap();
        app.merge_privileged_copy_history(history.clone());
        app.merge_privileged_copy_history(history);
        assert_eq!(app.operation_history.len(), 1);
        let record = app.operation_history.front().unwrap();
        assert!(is_privileged(record));
        assert!(copy_activity_cleanup::can_cleanup(record));
        assert!(matches!(
            record.path_move_undo,
            Some(PathMoveUndoState::Available)
        ));
        assert_eq!(copy_receipt_transaction_ids(record), vec!["a".repeat(32)]);
        app.clear_finished_activity();
        assert_eq!(
            app.operation_history.len(),
            1,
            "Clear must retain administrator recovery ownership"
        );
    }

    #[test]
    fn restored_history_refreshes_without_claiming_original_copy_completed() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let mut entry = ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        };
        app.merge_privileged_copy_history(vec![entry.clone()]);
        let id = app.operation_history.front().unwrap().id;
        let ctx = Context::default();
        let text = crate::tests::activity_text(&mut app, &ctx);
        assert!(
            text.iter().any(|label| label == "Recovered history"),
            "{text:?}"
        );
        assert!(!text.iter().any(|label| label == "Done"), "{text:?}");
        assert!(path_move_undo_action_is_available(
            app.operation_history[0].status,
            app.operation_history[0].path_move_undo.as_ref()
        ));
        entry.recovery = PrivilegedCopyHistoryState::NeedsReview {
            reason: "Published file changed".into(),
        };
        app.merge_privileged_copy_history(vec![entry.clone()]);
        assert_eq!(app.operation_history.len(), 1);
        assert_eq!(app.operation_history[0].id, id);
        assert_eq!(
            app.operation_history[0].status,
            OperationStatus::NeedsReview
        );
        assert!(
            matches!(&app.operation_history[0].path_move_undo, Some(PathMoveUndoState::Unavailable { reason }) if reason == "Published file changed")
        );
        entry.recovery = PrivilegedCopyHistoryState::UndoAvailable;
        app.merge_privileged_copy_history(vec![entry.clone()]);
        assert!(matches!(
            app.operation_history[0].path_move_undo,
            Some(PathMoveUndoState::Available)
        ));
        entry.recovery = PrivilegedCopyHistoryState::Undone;
        app.merge_privileged_copy_history(vec![entry]);
        assert_eq!(app.operation_history[0].id, id);
        assert_eq!(app.operation_history[0].status, OperationStatus::Completed);
        assert!(matches!(
            app.operation_history[0].path_move_undo,
            Some(PathMoveUndoState::Undone)
        ));
        let text = crate::tests::activity_text(&mut app, &ctx);
        assert!(text.iter().any(|label| label == "Copy undone"), "{text:?}");
        assert!(
            !text
                .iter()
                .any(|label| label == "Needs review" || label == "Undo copy" || label == "Done"),
            "{text:?}"
        );
        assert!(copy_activity_cleanup::can_cleanup(
            &app.operation_history[0]
        ));
    }

    #[test]
    fn discovery_preserves_live_receipts_and_refuses_changed_cohort_refresh() {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        app.record_privileged_copy(
            &command(),
            &response(PrivilegedStatus::Failed, 1),
            Some(7),
            Vec::new(),
        )
        .unwrap();
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let mut entry = ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: PrivilegedCopyHistoryState::NeedsReview {
                reason: "Changed".into(),
            },
        };
        app.merge_privileged_copy_history(vec![entry.clone()]);
        assert_eq!(app.operation_history.len(), 1);
        assert_eq!(app.operation_history[0].status, OperationStatus::Stopped);
        assert!(!is_restored(&app.operation_history[0]));
        assert_eq!(
            app.operation_history[0]
                .path_move_undo_token
                .as_ref()
                .unwrap()
                .origin_tab_id,
            Some(7)
        );
        app.operation_history.clear();
        entry.recovery = PrivilegedCopyHistoryState::UndoAvailable;
        app.merge_privileged_copy_history(vec![entry.clone()]);
        entry.reference.transactions.push("b".repeat(32));
        entry.recovery = PrivilegedCopyHistoryState::NeedsReview {
            reason: "Cohort changed".into(),
        };
        app.merge_privileged_copy_history(vec![entry]);
        assert_eq!(app.operation_history.len(), 1);
        assert_eq!(
            copy_receipt_transaction_ids(&app.operation_history[0]),
            vec!["a".repeat(32)]
        );
        assert!(matches!(
            app.operation_history[0].path_move_undo,
            Some(PathMoveUndoState::Unavailable { .. })
        ));
    }

    #[test]
    fn history_rejects_duplicate_ids_and_does_not_offer_undo_for_cleanup_only_records() {
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let mut entry = ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        };
        assert!(validate_history(&[entry.clone(), entry.clone()]).is_err());
        entry.reference.paths.clear();
        assert!(validate_history(&[entry.clone()]).is_err());
        entry.recovery = PrivilegedCopyHistoryState::NeedsReview {
            reason: "Cleanup interrupted".into(),
        };
        validate_history(&[entry.clone()]).unwrap();
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        app.merge_privileged_copy_history(vec![entry]);
        let record = app.operation_history.front().unwrap();
        assert!(matches!(
            record.path_move_undo,
            Some(PathMoveUndoState::Unavailable { .. })
        ));
        assert_eq!(
            copy_activity_cleanup::can_cleanup(record),
            cfg!(target_os = "linux")
        );
        assert_eq!(record.status, OperationStatus::NeedsReview);
    }

    fn owned(relative: &str) -> PathBuf {
        std::env::temp_dir()
            .join("OttrinOwnedActivityReferences")
            .join(relative.trim_start_matches('/'))
    }

    fn response(status: PrivilegedStatus, completed: usize) -> ottrin_core::PrivilegedResponse {
        ottrin_core::PrivilegedResponse {
            status,
            message: (status != PrivilegedStatus::Success).then(|| "Later Copy item failed".into()),
            payload: Some(PrivilegedPayload::Copy(
                ottrin_core::PrivilegedCopyReceipt {
                    completed_sources: completed,
                    transactions: vec!["a".repeat(32)],
                    paths: vec![(owned("/output/.ottrin-txn-a/new"), owned("/output/a"))],
                },
            )),
        }
    }
    fn command() -> FileCommand {
        FileCommand::Copy {
            sources: vec![owned("/input/a"), owned("/input/b")],
            destination: owned("/output"),
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
                owned("/output/a").as_path()
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
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_history_refuses_windows_only_recovery_before_merging() {
        let reference = ottrin_core::PrivilegedCopyReceipt {
            completed_sources: 0,
            transactions: vec!["a".repeat(32)],
            paths: Vec::new(),
        };
        for recovery in [
            PrivilegedCopyHistoryState::RecoveryAvailable {
                action: ottrin_core::PrivilegedCopyRecoveryAction::Resume,
            },
            PrivilegedCopyHistoryState::RecoveryAvailable {
                action: ottrin_core::PrivilegedCopyRecoveryAction::EndAttempt,
            },
            PrivilegedCopyHistoryState::RecoveryAvailable {
                action: ottrin_core::PrivilegedCopyRecoveryAction::ReleaseName,
            },
            PrivilegedCopyHistoryState::AttemptEnded,
        ] {
            assert!(
                validate_history(&[ottrin_core::PrivilegedCopyHistoryEntry {
                    reference: reference.clone(),
                    recovery
                }])
                .is_err()
            );
        }
        assert!(
            validate_history(&[ottrin_core::PrivilegedCopyHistoryEntry {
                reference,
                recovery: PrivilegedCopyHistoryState::CleanupPending
            }])
            .is_ok()
        );
    }
}
