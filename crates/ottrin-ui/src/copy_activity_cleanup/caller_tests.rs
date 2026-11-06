use super::*;
use crate::privileged_copy_activity::test_support::ProbePlatform;

fn selections() -> Vec<Selection> {
    vec![
        Selection {
            transactions: vec!["a".repeat(32)],
            privileged: false,
        },
        Selection {
            transactions: vec!["b".repeat(32)],
            privileged: true,
        },
    ]
}

#[test]
fn mixed_cleanup_uses_one_authenticated_command_for_the_complete_selection() {
    let platform = ProbePlatform::new(PrivilegedStatus::Success);
    let slot = Arc::new(Mutex::new(None));
    let outcome = run(
        &selections(),
        &platform,
        &slot,
        &AtomicBool::new(false),
        None,
        &mut |_| {},
    );
    assert!(outcome.error.is_none());
    assert_eq!(
        outcome.completed,
        HashSet::from(["a".repeat(32), "b".repeat(32)])
    );
    assert!(slot.lock().unwrap().is_none());
    let commands = platform.commands.lock().unwrap();
    assert!(
        matches!(commands.as_slice(), [PrivilegedCommand::CleanupCopy { transactions }]
        if transactions == &vec!["a".repeat(32), "b".repeat(32)])
    );
}

#[test]
fn stopped_or_refused_mixed_cleanup_keeps_the_entire_selection_owned() {
    for stopped in [false, true] {
        let platform = ProbePlatform::new(PrivilegedStatus::Failed);
        let slot = Arc::new(Mutex::new(None));
        let outcome = run(
            &selections(),
            &platform,
            &slot,
            &AtomicBool::new(stopped),
            None,
            &mut |_| {},
        );
        assert!(outcome.error.is_some());
        assert!(outcome.completed.is_empty());
        assert!(slot.lock().unwrap().is_none());
        assert_eq!(
            platform.commands.lock().unwrap().len(),
            usize::from(!stopped)
        );
    }
}

#[test]
fn failed_cleanup_poll_preserves_helper_references_for_explicit_retry() {
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    let id = app.queue_operation_record(FileCommand::Copy {
        sources: vec![PathBuf::from(r"C:\OwnedSource")],
        destination: PathBuf::from(r"C:\OwnedDestination"),
        conflict: ConflictAction::Rename,
    });
    app.finish_operation_record(id, OperationStatus::Completed, None);
    let record = app.operation_history.front_mut().unwrap();
    record.undo_kind = Some(PathMoveUndoKind::Copy);
    record.path_move_undo = Some(PathMoveUndoState::Available);
    record.path_move_undo_token = Some(PathMoveUndoToken {
        origin_tab_id: None,
        storage_bindings: Vec::new(),
        receipt: OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
            reference: ottrin_core::PrivilegedCopyReceipt {
                transactions: vec!["b".repeat(32)],
                completed_sources: 1,
                paths: vec![(
                    PathBuf::from(r"C:\OwnedPayload"),
                    PathBuf::from(r"C:\OwnedDestination\copy"),
                )],
            },
            pending: true,
            restored: true,
        }),
    });
    let (sender, receiver) = mpsc::channel();
    app.copy_cleanup.active = vec![Selection {
        transactions: vec!["b".repeat(32)],
        privileged: true,
    }];
    app.copy_cleanup.receiver = Some(receiver);
    sender
        .send(Event::Finished(Outcome {
            error: Some("Owned helper refusal".into()),
            ..Default::default()
        }))
        .unwrap();
    app.poll_copy_cleanup();
    let record = &app.operation_history[0];
    assert!(privileged_copy_activity::is_privileged(record));
    assert_eq!(transactions(record), vec!["b".repeat(32)]);
    assert!(copy_activity_cleanup::can_cleanup(record));
    assert!(matches!(
        record.path_move_undo,
        Some(PathMoveUndoState::Unavailable { .. })
    ));
    assert_eq!(app.copy_cleanup.action_label(record), "Retry Copy cleanup");
    assert!(
        record
            .message
            .as_ref()
            .unwrap()
            .contains("Owned helper refusal")
    );
}
