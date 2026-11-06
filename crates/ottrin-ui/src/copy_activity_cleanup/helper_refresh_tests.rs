use super::*;

#[test]
fn failed_cleanup_poll_preserves_helper_references_for_explicit_retry() {
    if crate::tests::private_activity_process(
        "copy_activity_cleanup::helper_refresh_tests::failed_cleanup_poll_preserves_helper_references_for_explicit_retry",
    ) {
        return;
    }
    let root = std::env::temp_dir().join("OttrinOwnedCleanupRefresh");
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    let id = app.queue_operation_record(FileCommand::Copy {
        sources: vec![root.join("source")],
        destination: root.join("destination"),
        conflict: ConflictAction::Rename,
    });
    app.finish_operation_record(id, OperationStatus::Completed, None);
    let reference = ottrin_core::PrivilegedCopyReceipt {
        transactions: vec!["b".repeat(32)],
        completed_sources: 1,
        paths: vec![(root.join("retained"), root.join("destination/copy"))],
    };
    let record = app.operation_history.front_mut().unwrap();
    record.undo_kind = Some(PathMoveUndoKind::Copy);
    record.path_move_undo = Some(PathMoveUndoState::Available);
    record.path_move_undo_token = Some(PathMoveUndoToken {
        origin_tab_id: None,
        storage_bindings: Vec::new(),
        receipt: OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
            reference: reference.clone(),
            pending: true,
            restored: false,
        }),
    });
    let (sender, receiver) = mpsc::channel();
    app.copy_cleanup.active = vec![Selection {
        transactions: reference.transactions.clone(),
        privileged: true,
        #[cfg(target_os = "linux")]
        finalization: None,
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
    assert!(
        !privileged_copy_activity::is_restored(record),
        "This must exercise a current-session helper receipt"
    );
    assert_eq!(transactions(record), reference.transactions);
    assert!(can_cleanup(record));
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
    assert!(app.copy_cleanup.failed(record));
    app.copy_cleanup
        .errors
        .insert("c".repeat(32), "Unrelated cleanup failure".into());
    let history = ottrin_core::PrivilegedCopyHistoryEntry {
        reference: reference.clone(),
        recovery: ottrin_core::PrivilegedCopyHistoryState::UndoAvailable,
    };
    // A retained cleanup writer owns its retry even when helper IDs match.
    app.operation_history[0]
        .retained_copy
        .as_mut()
        .unwrap()
        .finalization = Some("owned-writer".into());
    app.merge_privileged_copy_history(vec![history.clone()]);
    assert!(!privileged_copy_activity::is_restored(
        &app.operation_history[0]
    ));
    assert!(app.copy_cleanup.failed(&app.operation_history[0]));
    assert_eq!(
        transactions(&app.operation_history[0]),
        reference.transactions
    );
    app.operation_history[0]
        .retained_copy
        .as_mut()
        .unwrap()
        .finalization = None;
    app.merge_privileged_copy_history(vec![history]);
    let record = &app.operation_history[0];
    assert_eq!(record.id, id);
    assert!(privileged_copy_activity::is_restored(record));
    assert!(matches!(
        record.path_move_undo,
        Some(PathMoveUndoState::Available)
    ));
    assert!(
        !app.copy_cleanup.failed(record),
        "A fresh admitted state must retire the stale failure overlay"
    );
    assert_eq!(
        app.copy_cleanup.action_label(record),
        "Discard Copy undo and clear…"
    );
    app.restore_copy_cleanup_errors();
    assert!(app.operation_history[0].message.is_none());
    app.merge_privileged_copy_history(Vec::new());
    let record = &app.operation_history[0];
    assert_eq!(record.id, id);
    assert_eq!(transactions(record), reference.transactions);
    #[cfg(target_os = "windows")]
    assert!(!can_cleanup(record));
    assert!(matches!(
        record.path_move_undo,
        Some(PathMoveUndoState::Unavailable { .. })
    ));
    app.clear_finished_activity();
    assert_eq!(app.operation_history.len(), 1);
    app.merge_privileged_copy_history(vec![ottrin_core::PrivilegedCopyHistoryEntry {
        reference: reference.clone(),
        recovery: ottrin_core::PrivilegedCopyHistoryState::Undone,
    }]);
    assert_eq!(app.operation_history[0].id, id);
    assert!(matches!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Undone)
    ));
    assert!(can_cleanup(&app.operation_history[0]));
    #[cfg(target_os = "windows")]
    {
        app.merge_privileged_copy_history(vec![ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: ottrin_core::PrivilegedCopyHistoryState::CleanupPending,
        }]);
        let record = &app.operation_history[0];
        assert_eq!(record.id, id);
        assert!(can_cleanup(record));
        assert_eq!(app.copy_cleanup.action_label(record), "Finish Copy cleanup");
        assert!(!privileged_copy_activity::test_support::history_requested(
            &app
        ));
    }
    assert_eq!(
        app.copy_cleanup
            .errors
            .get(&"c".repeat(32))
            .map(String::as_str),
        Some("Unrelated cleanup failure")
    );
}
