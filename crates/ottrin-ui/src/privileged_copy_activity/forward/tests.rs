use super::*;
use ottrin_platform::FileProperties;

struct Platform {
    reply: ottrin_core::PrivilegedResponse,
    entered: Sender<()>,
    release: Mutex<Receiver<()>>,
    observed_stop: AtomicBool,
}
impl PlatformOps for Platform {
    fn delete_path(&self, _: &Path, _: DeleteMode) -> Result<(), PlatformError> {
        panic!("ambient mutation")
    }
    fn reveal_in_system(&self, _: &Path) -> Result<(), PlatformError> {
        panic!("desktop operation")
    }
    fn open_with_application(&self, _: &Path, _: &Path) -> Result<(), PlatformError> {
        panic!("desktop operation")
    }
    fn execute_command(&self, _: &FileCommand) -> Result<Option<FileProperties>, PlatformError> {
        panic!("ambient command")
    }
    fn execute_privileged(
        &self,
        _: &PrivilegedRequest,
    ) -> Result<ottrin_core::PrivilegedResponse, PlatformError> {
        panic!("blocking legacy helper API")
    }
    fn execute_privileged_with_progress(
        &self,
        request: &PrivilegedRequest,
        progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ottrin_core::PrivilegedResponse, PlatformError> {
        assert!(matches!(
            request.command,
            PrivilegedCommand::File(FileCommand::Copy { .. })
        ));
        progress(0)?;
        progress(8192)?;
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        self.observed_stop
            .store(progress(8192).is_err(), Ordering::Release);
        // An already-received terminal response can follow a late Stop.
        Ok(self.reply.clone())
    }
    fn privileged_availability(&self) -> PrivilegedAvailability {
        PrivilegedAvailability::Ready
    }
}

struct Fixture {
    root: tempfile::TempDir,
    retry: PendingPrivilegedRetry,
    platform: Arc<Platform>,
    entered: Receiver<()>,
    release: Sender<()>,
}
impl Fixture {
    fn new(status: PrivilegedStatus, payload: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let destination = root.path().join("destination");
        std::fs::write(&source, b"original source").unwrap();
        std::fs::create_dir(&destination).unwrap();
        let cmd = FileCommand::Copy {
            sources: vec![source.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Rename,
        };
        let retry = PendingPrivilegedRetry::capture(
            cmd.clone(),
            None,
            &operation_storage_bindings(&cmd, &discover_storage_locations()).unwrap(),
        );
        let (tx, entered) = mpsc::channel();
        let (release, rx) = mpsc::channel();
        let platform = Arc::new(Platform {
            reply: ottrin_core::PrivilegedResponse {
                status,
                message: (status != PrivilegedStatus::Success)
                    .then(|| "Owned helper refusal after completed prefix".into()),
                payload: payload.then(|| {
                    PrivilegedPayload::Copy(ottrin_core::PrivilegedCopyReceipt {
                        transactions: vec!["a".repeat(32)],
                        completed_sources: 1,
                        paths: vec![(source, destination.join("source.txt"))],
                    })
                }),
            },
            entered: tx,
            release: Mutex::new(rx),
            observed_stop: AtomicBool::new(false),
        });
        Self {
            root,
            retry,
            platform,
            entered,
            release,
        }
    }
    fn app(&self) -> OttrinApp {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        app
    }
    fn finish(&self, app: &mut OttrinApp, id: u64) {
        self.release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.copy_work.running(id) {
            app.poll_op_results();
            assert!(
                Instant::now() < deadline,
                "Queued Copy did not publish its result"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            std::fs::read(self.root.path().join("source.txt")).unwrap(),
            b"original source"
        );
        assert_eq!(
            std::fs::read_dir(self.root.path().join("destination"))
                .unwrap()
                .count(),
            0,
            "Adapter fixture must never perform ambient Copy"
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

#[test]
fn queued_copy_renders_progress_and_stop_keeps_a_late_success_on_the_original_row() {
    if crate::tests::private_activity_process(
        "privileged_copy_activity::forward::tests::queued_copy_renders_progress_and_stop_keeps_a_late_success_on_the_original_row",
    ) {
        return;
    }
    let fixture = Fixture::new(PrivilegedStatus::Success, true);
    let mut app = fixture.app();
    app.start_privileged_copy(fixture.retry.clone(), fixture.platform.clone())
        .unwrap();
    let id = app.operation_history[0].id;
    fixture
        .entered
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert_eq!(app.operation_history[0].status, OperationStatus::Queued);
    assert_eq!(app.copy_work.transferred(id), Some(8192));
    let ctx = Context::default();
    let text = crate::tests::activity_text(&mut app, &ctx);
    assert!(text.iter().any(|t| t == "Stop Copy"), "{text:?}");
    assert!(
        text.iter()
            .any(|t| t == &format!("Current step: {} processed", format_size(8192))),
        "{text:?}"
    );
    crate::tests::click_activity_action(&mut app, &ctx, "Stop Copy");
    assert!(app.copy_work.stopping(id));
    fixture.finish(&mut app, id);
    assert!(fixture.platform.observed_stop.load(Ordering::Acquire));
    assert_eq!(app.operation_history.len(), 1);
    let record = &app.operation_history[0];
    assert_eq!(record.id, id);
    assert_eq!(record.status, OperationStatus::Completed);
    assert!(is_privileged(record));
    assert!(matches!(
        record.path_move_undo,
        Some(PathMoveUndoState::Available)
    ));
    assert!(copy_activity_cleanup::can_cleanup(record));
    assert!(
        record
            .path_move_undo_token
            .as_ref()
            .unwrap()
            .storage_bindings
            .iter()
            .all(|binding| {
                let FileCommand::Copy { destination, .. } = &fixture.retry.cmd else {
                    unreachable!()
                };
                destination.starts_with(&binding.root)
            })
    );
    app.clear_finished_activity();
    assert_eq!(app.operation_history.len(), 1);
}

#[test]
fn partial_failure_keeps_undo_while_denial_and_missing_payload_never_claim_completion() {
    if crate::tests::private_activity_process(
        "privileged_copy_activity::forward::tests::partial_failure_keeps_undo_while_denial_and_missing_payload_never_claim_completion",
    ) {
        return;
    }
    for (status, payload) in [
        (PrivilegedStatus::Failed, true),
        (PrivilegedStatus::Denied, false),
        (PrivilegedStatus::Success, false),
    ] {
        let fixture = Fixture::new(status, payload);
        let mut app = fixture.app();
        app.start_privileged_copy(fixture.retry.clone(), fixture.platform.clone())
            .unwrap();
        let id = app.operation_history[0].id;
        fixture
            .entered
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        if status == PrivilegedStatus::Failed {
            app.copy_work.stop(id);
        }
        fixture.finish(&mut app, id);
        assert_eq!(
            fixture.platform.observed_stop.load(Ordering::Acquire),
            status == PrivilegedStatus::Failed
        );
        assert_eq!(app.operation_history.len(), 1);
        let record = &app.operation_history[0];
        assert_eq!(record.id, id);
        assert_ne!(record.status, OperationStatus::Completed);
        assert_eq!(is_privileged(record), payload);
        if payload {
            assert_eq!(record.status, OperationStatus::Stopped);
            assert!(record.detail.ends_with("1 of 1 items copied"));
            assert_eq!(
                record.message.as_deref(),
                Some("Owned helper refusal after completed prefix")
            );
            assert_eq!(
                app.command_frame.message.as_deref(),
                Some("Copy stopped before finishing. See Activity for details.")
            );
            assert_eq!(copy_receipt_transaction_ids(record), vec!["a".repeat(32)]);
            assert!(matches!(
                record.path_move_undo,
                Some(PathMoveUndoState::Available)
            ));
            assert!(copy_activity_cleanup::can_cleanup(record));
            app.clear_finished_activity();
            assert_eq!(app.operation_history.len(), 1);
        } else {
            assert_eq!(record.status, OperationStatus::Failed);
        }
        assert!(app.pending_privileged_retry.is_none());
    }
}

#[test]
fn worker_rechecks_captured_storage_and_prework_stop_before_calling_the_helper() {
    if crate::tests::private_activity_process(
        "privileged_copy_activity::forward::tests::worker_rechecks_captured_storage_and_prework_stop_before_calling_the_helper",
    ) {
        return;
    }
    for stop in [false, true] {
        let fixture = Fixture::new(PrivilegedStatus::Success, true);
        let mut retry = fixture.retry.clone();
        if !stop {
            retry.bindings.clear();
        }
        let mut work = copy_activity_work::CopyWorkUi::default();
        work.start(19);
        let result = run_copy(
            19,
            retry,
            fixture.root.path().to_path_buf(),
            fixture.platform.as_ref(),
            &AtomicBool::new(stop),
            &mut work.progress(19, ResultRepaint::default()),
        );
        assert_eq!(result.stopped, stop);
        assert_eq!(result.error.is_some(), !stop);
        assert_eq!(result.warning.is_some(), stop);
        assert!(result.path_move_receipt.is_none());
        assert!(fixture.entered.try_recv().is_err());
    }
}
