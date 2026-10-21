//! Explicit Windows forward, staging-end and name-release recovery. Only a freshly admitted
//! typed plan can start the shared off-thread worker.
use super::*;

struct Completion {
    message: &'static str,
    released_phase: Option<ottrin_platform::WindowsCopyPhase>,
}

#[derive(Default)]
pub(super) struct CopyRecoveryUi {
    receiver: Option<Receiver<Result<Completion, String>>>,
    operation: Option<u64>,
    cancellation: Option<Arc<AtomicBool>>,
    errors: HashMap<u64, String>,
    confirmation: Option<(u64, CopyRecoveryAction)>,
    // Test builds can hold the real worker before its first platform callback,
    // so a rendered Stop click is deterministic even for a tiny fixture.
    #[cfg(test)]
    start_gate: Option<Receiver<()>>,
}

impl CopyRecoveryUi {
    pub(super) fn running(&self) -> bool {
        self.receiver.is_some()
    }
    pub(super) fn owns(&self, id: u64) -> bool {
        self.running() && self.operation == Some(id)
    }
    pub(super) fn stop(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Release);
        }
    }
    pub(super) fn stopping(&self) -> bool {
        self.running()
            && self
                .cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
}

impl Drop for CopyRecoveryUi {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn name_release_pending(record: &OperationRecord) -> bool {
    record
        .retained_copy
        .as_ref()
        .and_then(|owner| owner.recovery.as_ref())
        .is_some_and(CopyRecoveryAction::is_name_release)
}

pub(super) fn can_recover(record: &OperationRecord) -> bool {
    record.retained_copy.as_ref().is_some_and(|owner| {
        owner.recovery.as_ref().is_some_and(|plan| {
            !plan.complete()
                && plan
                    .transaction_id()
                    .is_some_and(|id| owner.transactions == [id])
        })
    })
}

impl OttrinApp {
    pub(super) fn request_copy_recovery(&mut self, id: u64) {
        if self.copy_recovery.running() || self.copy_cleanup.running() {
            return;
        }
        let plan = self
            .operation_history
            .iter()
            .find(|record| record.id == id && can_recover(record))
            .and_then(|record| record.retained_copy.as_ref())
            .and_then(|owner| owner.recovery.as_ref());
        if let Some(plan @ CopyRecoveryAction::Abort(_)) = plan {
            self.copy_recovery.confirmation = Some((id, plan.clone()));
        } else {
            self.start_copy_recovery(id, None);
        }
    }

    pub(super) fn render_copy_abort_confirmation(&mut self, ctx: &Context) {
        let Some((id, plan)) = self.copy_recovery.confirmation.clone() else {
            return;
        };
        let c = self.colors;
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("copy_abort_confirmation"))
            .frame(Frame::new().fill(c.panel).stroke(Stroke::new(c.window_border_width, c.window_border))
                .corner_radius(c.app_radius).inner_margin(18))
            .show(ctx, |ui| {
                ui.set_width(400.0 * c.font_scale);
                ui.label(RichText::new("End this copy attempt?").color(c.heading).strong().size(15.0 * c.font_scale));
                ui.add_space(8.0);
                if let CopyRecoveryAction::Abort(plan) = &plan {
                    ui.label(format!("Destination: {}", path_label(&plan.target())));
                }
                ui.label("This attempt will no longer be able to resume or publish. Its reserved file name will become available for another copy.");
                ui.label("All partial files, later edits and recovery records are kept. This does not reclaim disk space.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Keep attempt").themed(&c)).clicked() { choice = Some(false); }
                    if ui.add(egui::Button::new("End attempt").themed(&c)).clicked() { choice = Some(true); }
                });
            });
        if choice == Some(true) {
            self.copy_recovery.confirmation = None;
            self.start_copy_recovery(id, Some(plan));
        } else if choice == Some(false) || response.should_close() {
            self.copy_recovery.confirmation = None;
        }
    }

    fn start_copy_recovery(&mut self, id: u64, confirmed: Option<CopyRecoveryAction>) {
        if self.copy_recovery.running() || self.copy_cleanup.running() {
            return;
        }
        let Some(record) = self.operation_history.iter().find(|record| {
            record.id == id
                && can_recover(record)
                && record.status != OperationStatus::Queued
                && !matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
        }) else {
            return;
        };
        let owner = record.retained_copy.as_ref().unwrap();
        let mut plan = confirmed.unwrap_or_else(|| owner.recovery.as_ref().unwrap().clone());
        if plan.complete()
            || plan
                .transaction_id()
                .is_none_or(|id| owner.transactions != [id])
        {
            return;
        }
        if self.activity_paths_conflict(id, &plan.path_pairs()) {
            self.set_status_message(
                "Wait for the operation using this Copy destination to finish.",
                false,
                6,
            );
            return;
        }
        let paths = plan.path_pairs();
        let paths = paths
            .into_iter()
            .flat_map(|(before, after)| [before.to_path_buf(), after.to_path_buf()])
            .map(OperationPath::write)
            .collect();
        let record = self
            .operation_history
            .iter_mut()
            .find(|record| record.id == id)
            .unwrap();
        record.status = OperationStatus::Queued;
        record.finished_at = None;
        record.message = None;
        record.operation_paths = paths;
        self.copy_recovery.errors.remove(&id);
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancellation.clone();
        let (tx, rx) = mpsc::channel();
        self.copy_recovery.receiver = Some(rx);
        self.copy_recovery.operation = Some(id);
        self.copy_recovery.cancellation = Some(cancellation);
        let repaint = self.result_repaint.clone();
        let success = Completion {
            message: plan.success_message(),
            released_phase: plan.released_phase(),
        };
        #[cfg(test)]
        let start_gate = self.copy_recovery.start_gate.take();
        std::thread::spawn(move || {
            #[cfg(test)]
            if let Some(gate) = start_gate
                && gate.recv().is_err()
            {
                return;
            }
            let result = match ottrin_platform::NativeIoCancellation::start(worker_cancel.clone()) {
                Err(error) => Err(error.to_string()),
                Ok(io_cancellation) => {
                    let result = plan
                        .run(|_| {
                            if worker_cancel.load(Ordering::Acquire) {
                                Err(PlatformError::Io(
                                    "Copy recovery stopped; retained data is preserved for retry."
                                        .into(),
                                ))
                            } else {
                                Ok(())
                            }
                        })
                        .map(|()| success)
                        .map_err(|error| error.to_string());
                    match io_cancellation.finish() {
                        Ok(()) => result,
                        Err(error) => Err(match result {
                            Ok(_) => error.to_string(),
                            Err(operation) => format!("{operation}; {error}"),
                        }),
                    }
                }
            };
            repaint.send(&tx, result);
        });
    }

    pub(super) fn poll_copy_recovery(&mut self) {
        let Some(receiver) = &self.copy_recovery.receiver else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Copy recovery worker stopped; retained data needs review.".into())
            }
        };
        self.copy_recovery.receiver = None;
        self.copy_recovery.cancellation = None;
        let id = self.copy_recovery.operation.take().unwrap();
        if let Some(record) = self
            .operation_history
            .iter_mut()
            .find(|record| record.id == id)
        {
            record.operation_paths = Vec::new();
            record.finished_at = Some(Instant::now());
            record.status = if result.is_ok() {
                OperationStatus::Completed
            } else {
                OperationStatus::NeedsReview
            };
            if let Ok(Completion {
                released_phase: Some(phase),
                ..
            }) = &result
            {
                // Terminal state comes from the executed opaque platform plan,
                // not the row's presentation fields. Keep ownership separately.
                record.path_move_undo_token = None;
                record.path_move_undo = Some(match phase {
                    ottrin_platform::WindowsCopyPhase::Undone => PathMoveUndoState::Undone,
                    ottrin_platform::WindowsCopyPhase::Aborted => PathMoveUndoState::Unavailable {
                        reason: "This copy attempt has ended. Partial files and recovery records are preserved.".into(),
                    },
                    _ => PathMoveUndoState::Unavailable { reason: "Cleanup has already revoked Undo. Retained data is kept until cleanup is resumed.".into() },
                });
            }
            // Fresh discovery must admit the next action, including Undo after
            // successful completion or a retry after a stopped/failed attempt.
            if let Some(owner) = &mut record.retained_copy {
                owner.recovery = None;
            }
            if let Err(error) = &result {
                self.copy_recovery.errors.insert(id, error.clone());
            }
        }
        if let Ok(completion) = result {
            self.set_status_message(completion.message, true, 6);
        } else {
            self.set_status_message("Copy recovery needs review in Activity.", false, 10);
        }
        self.refresh_active_tab();
        self.refresh_retained_copy_activity();
        self.restore_copy_recovery_errors();
    }

    pub(super) fn restore_copy_recovery_errors(&mut self) {
        for record in &mut self.operation_history {
            if let Some(error) = self.copy_recovery.errors.get(&record.id) {
                record.message = Some(error.clone());
            }
        }
    }
}

#[cfg(test)]
pub(super) fn abort_test_case(root: &Path) {
    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_copy_recovery();
            app.poll_retained_copy_activity();
            if !app.copy_recovery.running() && !app.retained_copy_activity_loader.pending {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Staging-end Activity worker timed out"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.retained_copy_activity_error.is_none());
    }
    let case = std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").unwrap();
    let destination = std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_DESTINATION")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("fixture/destination"));
    let entry = ottrin_platform::discover_retained_copy_transactions()
        .unwrap()
        .remove(0);
    let transaction = entry.transaction_id.as_deref().unwrap().to_owned();
    let journal = root
        .join("profile/Ottrin/State/copies")
        .join(format!("{transaction}.copy-journal"));
    let local_journal = entry.transaction.as_ref().unwrap().join("journal");
    let before = std::fs::read(&journal).unwrap();
    let local_before = std::fs::read(&local_journal).unwrap();
    let payload = destination.join(format!(".ottrin-copy-{transaction}.candidate"));
    let check_payload = || {
        assert_eq!(
            std::fs::read(payload.join("nested/file")).unwrap(),
            b"later retained edits"
        );
        assert_eq!(
            std::fs::read(payload.join("nested/file:keep")).unwrap(),
            b"later named stream"
        );
        assert_eq!(
            std::fs::read(payload.join("unrecognized")).unwrap(),
            b"unexpected retained file"
        );
        assert_eq!(
            std::fs::read(destination.join("foreign")).unwrap(),
            b"unrelated public data"
        );
        assert!(!destination.join("copy").exists());
        assert!(!root.join("fixture/source").exists());
    };
    let plan = ottrin_platform::restore_retained_copy_abort_plan(&transaction).unwrap();
    let mut wrong = entry;
    wrong.transaction_id = Some("0".repeat(32));
    assert!(
        windows_retained_copy_activity(wrong, None, Some(Ok(CopyRecoveryAction::Abort(plan))))
            .ownership
            .recovery
            .is_none(),
        "An end-attempt plan cannot attach to another transaction"
    );
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.retained_copy_activity_loader =
        RetainedActivityLoader::spawn_with(load_retained_copy_activity);
    app.refresh_retained_copy_activity();
    wait(&mut app);
    assert_eq!(app.operation_history.len(), 1);
    let id = app.operation_history[0].id;
    assert!(can_recover(&app.operation_history[0]));
    assert!(!name_release_pending(&app.operation_history[0]));
    assert!(!copy_activity_cleanup::can_cleanup(
        &app.operation_history[0]
    ));
    assert!(app.operation_history[0].path_move_undo_token.is_none());
    let ctx = Context::default();
    let text = crate::tests::activity_text(&mut app, &ctx);
    assert!(text.iter().any(|text| text == "Incomplete copy"));
    assert!(!text.iter().any(|text| text == "Resume copy"));
    crate::tests::click_activity_action(&mut app, &ctx, "End copy attempt…");
    assert!(app.copy_recovery.confirmation.is_some() && !app.copy_recovery.running());
    let text = crate::tests::activity_text(&mut app, &ctx);
    assert!(
        text.iter()
            .any(|text| text.contains("does not reclaim disk space"))
    );
    crate::tests::click_activity_action(&mut app, &ctx, "Keep attempt");
    assert!(app.copy_recovery.confirmation.is_none() && !app.copy_recovery.running());
    assert_eq!(std::fs::read(&journal).unwrap(), before);
    assert_eq!(std::fs::read(&local_journal).unwrap(), local_before);
    check_payload();
    crate::tests::click_activity_action(&mut app, &ctx, "End copy attempt…");
    let mut expected = before.clone();
    if case == "abort-changed-controls" {
        expected.push(7);
        std::fs::write(&journal, &expected).unwrap();
        std::fs::write(&local_journal, &expected).unwrap();
        // A refresh cannot silently replace the plan the user confirmed.
        app.refresh_retained_copy_activity();
    }
    crate::tests::click_activity_action(&mut app, &ctx, "End attempt");
    wait(&mut app);
    assert_eq!(app.operation_history[0].id, id);
    check_payload();
    assert!(app.copy_recovery.confirmation.is_none());
    if case == "abort-changed-controls" {
        assert_eq!(std::fs::read(&journal).unwrap(), expected);
        assert_eq!(std::fs::read(&local_journal).unwrap(), expected);
        assert!(ottrin_platform::restore_retained_copy_abort_plan(&transaction).is_err());
        assert!(!app.operation_history.iter().any(can_recover));
    } else {
        let entry = ottrin_platform::discover_retained_copy_transactions()
            .unwrap()
            .remove(0);
        assert_eq!(
            entry.phase,
            Some(ottrin_platform::WindowsCopyPhase::Aborted)
        );
        assert!(!can_recover(&app.operation_history[0]));
        assert!(!copy_activity_cleanup::can_cleanup(
            &app.operation_history[0]
        ));
        assert!(ottrin_platform::restore_retained_copy_resume_plan(&transaction).is_err());
        assert!(ottrin_platform::restore_retained_copy_abort_plan(&transaction).is_err());
        app.clear_finished_activity();
        assert_eq!(
            app.operation_history.len(),
            1,
            "Ending keeps the recovery row and records"
        );
    }
}

#[cfg(test)]
pub(super) fn test_case(root: &Path) {
    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.poll_copy_recovery();
            app.poll_path_move_undo_results();
            app.poll_retained_copy_activity();
            if !app.copy_recovery.running()
                && !app.retained_copy_activity_loader.pending
                && !app.operation_history.iter().any(|record| {
                    matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
                })
            {
                break;
            }
            assert!(Instant::now() < deadline, "Copy recovery worker timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.retained_copy_activity_error.is_none());
    }
    let case = std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").unwrap();
    let destination = std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_DESTINATION")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("fixture/destination"));
    let mut entry = ottrin_platform::discover_retained_copy_transactions()
        .unwrap()
        .remove(0);
    let journal = root.join("profile/Ottrin/State/copies").join(format!(
        "{}.copy-journal",
        entry.transaction_id.as_deref().unwrap()
    ));
    let before = std::fs::read(&journal).unwrap();
    let local_journal = entry.transaction.as_ref().unwrap().join("journal");
    let local_before = std::fs::read(&local_journal).unwrap();
    let data = entry.owned_location.as_ref().unwrap().join("nested/file");
    let plan = ottrin_platform::restore_retained_copy_resume_plan(
        entry.transaction_id.as_deref().unwrap(),
    )
    .unwrap();
    entry.transaction_id = Some("0".repeat(32));
    let rejected =
        windows_retained_copy_activity(entry, None, Some(Ok(CopyRecoveryAction::Forward(plan))));
    assert!(
        rejected.ownership.recovery.is_none(),
        "A forward plan cannot attach to a different transaction row"
    );
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.retained_copy_activity_loader =
        RetainedActivityLoader::spawn_with(load_retained_copy_activity);
    app.refresh_retained_copy_activity();
    wait(&mut app);
    assert_eq!(app.operation_history.len(), 1);
    let id = app.operation_history[0].id;
    assert!(can_recover(&app.operation_history[0]));
    assert!(
        !copy_activity_cleanup::can_cleanup(&app.operation_history[0]),
        "Forward recovery has no supported cleanup action yet"
    );
    assert!(app.operation_history[0].path_move_undo_token.is_none());
    app.clear_finished_activity();
    assert_eq!(
        app.operation_history.len(),
        1,
        "Retained Copy survives Clear finished"
    );
    if case == "resume-changed" {
        std::fs::write(&data, b"external data").unwrap();
    }
    let actual = if case == "resume-moved" {
        let moved = destination.with_file_name("moved");
        std::fs::rename(&destination, &moved).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("copy"), b"unrelated occupant").unwrap();
        moved
    } else {
        destination.clone()
    };
    let ctx = Context::default();
    let start_gate = if case == "resume-stopped" {
        let (tx, rx) = mpsc::channel();
        app.copy_recovery.start_gate = Some(rx);
        Some(tx)
    } else {
        None
    };
    let mut blocked = case.strip_prefix("resume-blocked-").map(|target| {
        crate::test_oplock::Oplock::request(match target {
            "central" => &journal,
            "local" => &local_journal,
            "payload" => &data,
            _ => panic!("Unknown owned oplock target"),
        })
        .unwrap()
    });
    crate::tests::click_activity_action(&mut app, &ctx, "Resume copy");
    assert!(app.copy_recovery.running());
    assert_eq!(app.operation_history[0].status, OperationStatus::Queued);
    if let Some(lock) = &mut blocked {
        let broken = lock.wait_break();
        std::thread::sleep(Duration::from_millis(100));
        app.poll_copy_recovery();
        let pending = app.copy_recovery.running();
        let owns_paths = !app.operation_history[0].operation_paths.is_empty();
        if pending {
            crate::tests::click_activity_action(&mut app, &ctx, "Stop recovery");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.copy_recovery.running() && Instant::now() < deadline {
            app.poll_copy_recovery();
            std::thread::sleep(Duration::from_millis(5));
        }
        let stopped = !app.copy_recovery.running();
        let message = app.operation_history[0].message.clone();
        // Release the filesystem and let any queued status refresh finish even
        // when an assertion below will fail. A stopped worker must finish first.
        let close = lock.close();
        wait(&mut app);
        close.unwrap();
        assert_eq!(broken, windows_sys::Win32::Foundation::WAIT_OBJECT_0);
        assert!(
            pending && owns_paths,
            "Blocked recovery must retain its operation paths"
        );
        assert!(stopped, "Rendered Stop did not interrupt blocked recovery");
        assert!(message.unwrap().contains("995"));
        assert!(app.operation_history[0].operation_paths.is_empty());
        assert!(can_recover(&app.operation_history[0]));
        assert_eq!(std::fs::read(&journal).unwrap(), before);
        assert_eq!(std::fs::read(&local_journal).unwrap(), local_before);
        assert_eq!(std::fs::read(&data).unwrap(), b"original data");
        assert!(!actual.join("copy").exists());
        crate::tests::click_activity_action(&mut app, &ctx, "Resume copy");
    }
    if let Some(gate) = start_gate {
        crate::tests::click_activity_action(&mut app, &ctx, "Stop recovery");
        assert!(app.copy_recovery.stopping());
        gate.send(()).unwrap();
        wait(&mut app);
        assert!(can_recover(&app.operation_history[0]));
        assert!(!app.copy_recovery.stopping());
        assert!(
            app.operation_history[0]
                .message
                .as_ref()
                .unwrap()
                .contains("stopped")
        );
        assert_eq!(
            std::fs::read(&journal).unwrap(),
            before,
            "Stopped worker must not append records"
        );
        assert_eq!(std::fs::read(&local_journal).unwrap(), local_before);
        assert_eq!(std::fs::read(&data).unwrap(), b"original data");
        crate::tests::click_activity_action(&mut app, &ctx, "Resume copy");
    }
    wait(&mut app);
    assert_eq!(app.operation_history[0].id, id);
    if case == "resume-changed" {
        assert!(!can_recover(&app.operation_history[0]));
        assert!(app.operation_history[0].path_move_undo_token.is_none());
        assert_eq!(std::fs::read(data).unwrap(), b"external data");
        assert_eq!(std::fs::read(journal).unwrap(), before);
        assert!(
            app.operation_history[0].message.is_some(),
            "Recovery failure stays visible after discovery"
        );
        return;
    }
    if case == "resume-moved" {
        assert!(
            can_recover(&app.operation_history[0]),
            "Fresh admission follows the actual parent location"
        );
        assert_eq!(
            std::fs::read(&journal).unwrap(),
            before,
            "Stale location refuses before journal writes"
        );
        assert_eq!(
            std::fs::read(destination.join("copy")).unwrap(),
            b"unrelated occupant"
        );
        crate::tests::click_activity_action(&mut app, &ctx, "Resume copy");
        wait(&mut app);
    }
    assert_eq!(app.operation_history[0].status, OperationStatus::Completed);
    assert!(!can_recover(&app.operation_history[0]));
    assert!(app.operation_history[0].message.is_none());
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Available)
    );
    assert_eq!(
        std::fs::read(actual.join("copy/nested/file")).unwrap(),
        b"original data"
    );
    assert!(!root.join("fixture/source").exists());
    crate::tests::click_activity_action(&mut app, &ctx, "Undo copy");
    wait(&mut app);
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Undone)
    );
    assert!(!actual.join("copy").exists());
    let retained = app.operation_history[0]
        .path_move_undo_token
        .as_ref()
        .unwrap()
        .receipt
        .before_path();
    assert_eq!(
        std::fs::read(retained.join("nested/file")).unwrap(),
        b"original data"
    );
}

#[cfg(test)]
pub(super) fn name_release_test_case(root: &Path) {
    use std::os::windows::ffi::OsStringExt;
    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.poll_copy_recovery();
            app.poll_retained_copy_activity();
            if !app.copy_recovery.running() && !app.retained_copy_activity_loader.pending {
                break;
            }
            assert!(Instant::now() < deadline, "Name-release worker timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.retained_copy_activity_error.is_none());
    }
    let case = std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").unwrap();
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("name-release.json")).unwrap()).unwrap();
    let units: Vec<u16> = serde_json::from_value(fixture["claim"].clone()).unwrap();
    let claim = PathBuf::from(std::ffi::OsString::from_wide(&units));
    let full: Vec<u8> = serde_json::from_value(fixture["full"].clone()).unwrap();
    let prefix = std::fs::read(&claim).unwrap();
    let entry = ottrin_platform::discover_retained_copy_transactions()
        .unwrap()
        .remove(0);
    assert_eq!(
        entry.evidence,
        ottrin_platform::WindowsCopyRecoveryEvidence::PendingNameRelease
    );
    let retained = entry.retained.as_ref().unwrap().clone();
    let local = entry.transaction.as_ref().unwrap().join("journal");
    let mirror = root.join("profile/Ottrin/State/copies").join(format!(
        "{}.copy-journal",
        entry.transaction_id.as_ref().unwrap()
    ));
    let journal = std::fs::read(&local).unwrap();
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    let plan = ottrin_platform::restore_retained_copy_name_release_plan(
        entry.transaction_id.as_deref().unwrap(),
    )
    .unwrap();
    let mut wrong = entry;
    wrong.transaction_id = Some("0".repeat(32));
    let blocked = windows_retained_copy_activity(
        wrong,
        None,
        Some(Ok(CopyRecoveryAction::NameRelease(plan))),
    );
    assert!(matches!(
        blocked.ownership.recovery,
        Some(CopyRecoveryAction::NameReleaseBlocked)
    ));
    assert!(blocked.receipt.is_none());
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.retained_copy_activity_loader =
        RetainedActivityLoader::spawn_with(load_retained_copy_activity);
    app.refresh_retained_copy_activity();
    wait(&mut app);
    assert_eq!(app.operation_history.len(), 1);
    let id = app.operation_history[0].id;
    assert!(can_recover(&app.operation_history[0]));
    assert!(name_release_pending(&app.operation_history[0]));
    assert!(!copy_activity_cleanup::can_cleanup(
        &app.operation_history[0]
    ));
    let ctx = Context::default();
    let text = crate::tests::activity_text(&mut app, &ctx);
    assert!(text.iter().any(|text| text == "Finish releasing name"));
    assert!(!text.iter().any(|text| text == "Clear retained Copy data…"));
    if case == "name-release-stopped" {
        let (gate, receiver) = mpsc::channel();
        app.copy_recovery.start_gate = Some(receiver);
        crate::tests::click_activity_action(&mut app, &ctx, "Finish releasing name");
        assert!(app.copy_recovery.running());
        crate::tests::click_activity_action(&mut app, &ctx, "Stop recovery");
        gate.send(()).unwrap();
        wait(&mut app);
        assert_eq!(std::fs::read(&claim).unwrap(), prefix);
        assert_eq!(std::fs::read(&local).unwrap(), journal);
        assert_eq!(std::fs::read(&mirror).unwrap(), journal);
        assert!(can_recover(&app.operation_history[0]));
    }
    if case == "name-release-changed" {
        // Keep a recognizable tail but invalidate the frozen admission epoch.
        let mut changed = prefix.clone();
        changed.push(full[prefix.len()]);
        std::fs::write(&claim, &changed).unwrap();
        crate::tests::click_activity_action(&mut app, &ctx, "Finish releasing name");
        wait(&mut app);
        assert_eq!(std::fs::read(&claim).unwrap(), changed);
        assert_eq!(std::fs::read(&local).unwrap(), journal);
        assert_eq!(std::fs::read(&mirror).unwrap(), journal);
        assert!(app.operation_history[0].message.is_some());
        assert!(
            can_recover(&app.operation_history[0]),
            "Retry must use a fresh admission"
        );
    }
    crate::tests::click_activity_action(&mut app, &ctx, "Finish releasing name");
    wait(&mut app);
    assert_eq!(app.operation_history[0].id, id);
    assert_eq!(std::fs::read(&claim).unwrap(), full);
    assert_eq!(std::fs::read(&local).unwrap(), journal);
    assert_eq!(std::fs::read(&mirror).unwrap(), journal);
    assert_eq!(
        std::fs::read(retained.join("nested/file")).unwrap(),
        b"original data"
    );
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Undone)
    );
    assert!(app.operation_history[0].path_move_undo_token.is_none());
    assert!(!can_recover(&app.operation_history[0]));
    assert!(!app.operation_history[0].can_clear_history());
    assert!(copy_activity_cleanup::can_cleanup(
        &app.operation_history[0]
    ));
}
