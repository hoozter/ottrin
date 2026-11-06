use super::*;
use crate::privileged_copy_activity::test_support::restore_history;

#[test]
fn helper_recovery_completion_requires_explicit_refresh_and_displays_its_outcome() {
    for refused in [false, true] {
        let mut app = OttrinApp::default();
        let id = restore_history(
            &mut app,
            &"a".repeat(32),
            ottrin_core::PrivilegedCopyHistoryState::RecoveryAvailable {
                action: ottrin_core::PrivilegedCopyRecoveryAction::Resume,
            },
        );
        let (sender, receiver) = mpsc::channel();
        app.copy_recovery.receiver = Some(receiver);
        app.copy_recovery.operation = Some(id);
        app.copy_recovery.cancellation = Some(Arc::new(AtomicBool::new(false)));
        app.operation_history[0].status = OperationStatus::Queued;
        sender.send(if refused { Err("Owned recovery refusal".into()) } else {
            Ok(Completion {
                message: "Administrator recovery completed. Reload administrator Copy history to inspect its current state.",
                released_phase: None,
            })
        }).unwrap();
        app.poll_copy_recovery();
        let record = &app.operation_history[0];
        assert!(privileged_copy_activity::is_privileged(record));
        assert!(!can_recover(record));
        assert!(!copy_activity_cleanup::can_cleanup(record));
        assert!(matches!(
            record.path_move_undo,
            Some(PathMoveUndoState::Unavailable { .. })
        ));
        // Completion does not infer a new Undo/cleanup capability or launch a
        // second authorization prompt; only explicit history loading can do so.
        assert!(!app.copy_recovery.running());
        assert!(!crate::privileged_copy_activity::test_support::history_requested(&app));
        let text = crate::tests::activity_text(&mut app, &Context::default());
        assert!(
            text.iter()
                .any(|text| text.contains("Reload administrator Copy history")),
            "{text:?}"
        );
        if refused {
            assert!(
                text.iter()
                    .any(|text| text.contains("Owned recovery refusal")),
                "{text:?}"
            );
        }
    }
}
