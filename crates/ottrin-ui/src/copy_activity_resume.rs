//! Explicit Windows forward recovery. Only an opaque, freshly admitted platform
//! plan can start work; payload verification and publication run off-thread.
use super::*;

#[derive(Default)]
pub(super) struct CopyResumeUi {
    receiver: Option<Receiver<Result<(), String>>>,
    operation: Option<u64>,
    cancellation: Option<Arc<AtomicBool>>,
    errors: HashMap<u64, String>,
    // Test builds can hold the real worker before its first platform callback,
    // so a rendered Stop click is deterministic even for a tiny fixture.
    #[cfg(test)]
    start_gate: Option<Receiver<()>>,
}

impl CopyResumeUi {
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

impl Drop for CopyResumeUi {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn can_resume(record: &OperationRecord) -> bool {
    record.retained_copy.as_ref().is_some_and(|owner| {
        owner
            .resume
            .as_ref()
            .is_some_and(|plan| !plan.complete() && owner.transactions == [plan.transaction_id()])
    })
}

impl OttrinApp {
    pub(super) fn run_copy_resume(&mut self, id: u64) {
        if self.copy_resume.running() || self.copy_cleanup.running() {
            return;
        }
        let Some(record) = self.operation_history.iter().find(|record| {
            record.id == id
                && can_resume(record)
                && record.status != OperationStatus::Queued
                && !matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
        }) else {
            return;
        };
        let mut plan = record
            .retained_copy
            .as_ref()
            .unwrap()
            .resume
            .as_ref()
            .unwrap()
            .clone();
        if self.activity_paths_conflict(id, &plan.path_pairs()) {
            self.set_status_message(
                "Wait for the operation using this Copy destination to finish.",
                false,
                6,
            );
            return;
        }
        let paths = plan.path_pairs();
        let paths = (paths[0].0.to_path_buf(), paths[0].1.to_path_buf());
        let record = self
            .operation_history
            .iter_mut()
            .find(|record| record.id == id)
            .unwrap();
        record.status = OperationStatus::Queued;
        record.finished_at = None;
        record.message = None;
        record.path_move_paths = Some(paths);
        self.copy_resume.errors.remove(&id);
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancellation.clone();
        let (tx, rx) = mpsc::channel();
        self.copy_resume.receiver = Some(rx);
        self.copy_resume.operation = Some(id);
        self.copy_resume.cancellation = Some(cancellation);
        let repaint = self.result_repaint.clone();
        #[cfg(test)]
        let start_gate = self.copy_resume.start_gate.take();
        std::thread::spawn(move || {
            #[cfg(test)]
            if let Some(gate) = start_gate
                && gate.recv().is_err()
            {
                return;
            }
            let result = plan
                .resume(|_| {
                    if worker_cancel.load(Ordering::Acquire) {
                        Err(PlatformError::Io(
                            "Copy recovery stopped; retained data is preserved for retry.".into(),
                        ))
                    } else {
                        Ok(())
                    }
                })
                .map_err(|error| error.to_string());
            repaint.send(&tx, result);
        });
    }

    pub(super) fn poll_copy_resume(&mut self) {
        let Some(receiver) = &self.copy_resume.receiver else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Copy recovery worker stopped; retained data needs review.".into())
            }
        };
        self.copy_resume.receiver = None;
        self.copy_resume.cancellation = None;
        let id = self.copy_resume.operation.take().unwrap();
        if let Some(record) = self
            .operation_history
            .iter_mut()
            .find(|record| record.id == id)
        {
            record.finished_at = Some(Instant::now());
            record.status = if result.is_ok() {
                OperationStatus::Completed
            } else {
                OperationStatus::NeedsReview
            };
            // Fresh discovery must admit the next action, including Undo after
            // successful completion or a retry after a stopped/failed attempt.
            if let Some(owner) = &mut record.retained_copy {
                owner.resume = None;
            }
            if let Err(error) = &result {
                self.copy_resume.errors.insert(id, error.clone());
            }
        }
        if result.is_ok() {
            self.set_status_message("Copy recovery completed.", true, 6);
        } else {
            self.set_status_message("Copy recovery needs review in Activity.", false, 10);
        }
        self.refresh_active_tab();
        self.refresh_retained_copy_activity();
        self.restore_copy_resume_errors();
    }

    pub(super) fn restore_copy_resume_errors(&mut self) {
        for record in &mut self.operation_history {
            if let Some(error) = self.copy_resume.errors.get(&record.id) {
                record.message = Some(error.clone());
            }
        }
    }
}

#[cfg(test)]
pub(super) fn test_case(root: &Path) {
    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.poll_copy_resume();
            app.poll_path_move_undo_results();
            app.poll_retained_copy_activity();
            if !app.copy_resume.running()
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
    let rejected = windows_retained_copy_activity(entry, None, Some(Ok(plan)));
    assert!(
        rejected.ownership.resume.is_none(),
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
    assert!(can_resume(&app.operation_history[0]));
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
        app.copy_resume.start_gate = Some(rx);
        Some(tx)
    } else {
        None
    };
    crate::tests::click_activity_action(&mut app, &ctx, "Resume copy");
    assert!(app.copy_resume.running());
    assert_eq!(app.operation_history[0].status, OperationStatus::Queued);
    if let Some(gate) = start_gate {
        crate::tests::click_activity_action(&mut app, &ctx, "Stop recovery");
        assert!(app.copy_resume.stopping());
        gate.send(()).unwrap();
        wait(&mut app);
        assert!(can_resume(&app.operation_history[0]));
        assert!(!app.copy_resume.stopping());
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
        assert!(!can_resume(&app.operation_history[0]));
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
            can_resume(&app.operation_history[0]),
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
    assert!(!can_resume(&app.operation_history[0]));
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
