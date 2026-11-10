use super::*;
use crate::privileged_copy_activity::test_support::ProbePlatform;

#[test]
fn history_cancel_control_cancels_the_loader_and_preserves_existing_activity() {
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.record_privileged_copy(
        &command(),
        &response(PrivilegedStatus::Success, 1),
        Some(7),
        Vec::new(),
    )
    .unwrap();
    let original = app.operation_history[0].id;
    let flag = Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let mut loader = RetainedActivityLoader::spawn_with(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !worker_flag.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "Rendered history cancellation never reached its worker"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let platform = ProbePlatform::new(PrivilegedStatus::Success);
        let result = load_history(&platform, &worker_flag);
        assert!(platform.commands.lock().unwrap().is_empty());
        result
    });
    loader.request().unwrap();
    app.privileged_copy_history.loader = Some(loader);
    app.privileged_copy_history.cancellation = Some(flag.clone());
    let ctx = Context::default();
    let frame = |app: &mut OttrinApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 350.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| app.render_privileged_copy_history(ui));
            },
        )
    };
    frame(&mut app, Vec::new());
    let output = frame(&mut app, Vec::new());
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Cancel loading history" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()).center())
            }
            _ => None,
        })
        .expect("Loading history must render its actual cancellation control");
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    assert!(flag.load(Ordering::Acquire));
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.privileged_copy_history.loader.as_ref().unwrap().pending {
        frame(&mut app, Vec::new());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        app.privileged_copy_history
            .message
            .as_ref()
            .unwrap()
            .contains("stopped")
    );
    assert_eq!(app.operation_history.len(), 1);
    assert_eq!(app.operation_history[0].id, original);
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Available)
    );
}

#[test]
fn administrator_undo_uses_progress_and_keeps_receipt_after_stop_or_refusal() {
    let Some(PrivilegedPayload::Copy(reference)) = response(PrivilegedStatus::Success, 1).payload
    else {
        unreachable!()
    };
    let original = reference.clone();
    let mut receipt = CopyBatchUndoReceipt::Privileged {
        reference,
        pending: true,
        restored: true,
    };
    let platform = ProbePlatform::new(PrivilegedStatus::Success);
    let mut observed = Vec::new();
    assert!(
        receipt
            .undo_next(&platform, &mut |bytes| {
                observed.push(bytes);
                if bytes > 0 {
                    Err(PlatformError::Io("Owned Stop".into()))
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert_eq!(observed, [0, 0, 23]);
    assert_eq!(receipt.remaining(), 1);
    assert!(
        matches!(&receipt, CopyBatchUndoReceipt::Privileged { reference, .. } if reference == &original)
    );
    let refused = ProbePlatform::new(PrivilegedStatus::Denied);
    assert!(receipt.undo_next(&refused, &mut |_| Ok(())).is_err());
    assert_eq!(receipt.remaining(), 1);
    receipt.undo_next(&platform, &mut |_| Ok(())).unwrap();
    assert_eq!(receipt.remaining(), 0);
    assert!(
        platform
            .commands
            .lock()
            .unwrap()
            .iter()
            .all(|command| matches!(command,
        PrivilegedCommand::UndoCopy { transactions } if transactions == &original.transactions))
    );
}

#[test]
fn administrator_discovery_deduplicates_real_ordinary_receipt_without_taking_ownership() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let destination = root.path().join("destination");
    std::fs::write(&source, b"ordinary copy remains owned").unwrap();
    std::fs::create_dir(&destination).unwrap();
    let receipt = DefaultPlatform
        .execute_copy_with_undo_receipt(&source, &destination, ConflictAction::Rename, &mut |_| {
            Ok(())
        })
        .unwrap()
        .unwrap();
    let transaction = receipt.copy_transaction_id().unwrap().to_owned();
    let target = destination.join("source");
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    let id = app.queue_operation_record(FileCommand::Copy {
        sources: vec![source.clone()],
        destination: destination.clone(),
        conflict: ConflictAction::Rename,
    });
    app.finish_operation_record(id, OperationStatus::Completed, None);
    let record = app.operation_history.front_mut().unwrap();
    record.undo_kind = Some(PathMoveUndoKind::Copy);
    record.path_move_undo = Some(PathMoveUndoState::Available);
    record.path_move_undo_token = Some(PathMoveUndoToken {
        receipt: OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Session {
            receipts: vec![receipt.clone()],
            remaining: 1,
        }),
        storage_bindings: Vec::new(),
        origin_tab_id: Some(7),
    });
    app.merge_privileged_copy_history(vec![ottrin_core::PrivilegedCopyHistoryEntry {
        reference: ottrin_core::PrivilegedCopyReceipt {
            transactions: vec![transaction.clone()],
            paths: vec![(source.clone(), target.clone())],
            completed_sources: 0,
        },
        recovery: PrivilegedCopyHistoryState::UndoAvailable,
    }]);
    assert_eq!(app.operation_history.len(), 1);
    assert_eq!(app.operation_history[0].id, id);
    assert!(!is_privileged(&app.operation_history[0]));
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Available)
    );
    assert_eq!(
        app.operation_history[0]
            .path_move_undo_token
            .as_ref()
            .unwrap()
            .origin_tab_id,
        Some(7)
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"ordinary copy remains owned"
    );
    // Exercise the retained ordinary inverse after discovery to prove the UI
    // did not trade its real receipt for a helper lookup or lose ownership.
    DefaultPlatform
        .undo_copy_with_receipt(&receipt, &mut |_| Ok(()))
        .unwrap();
    #[cfg(target_os = "windows")]
    {
        let mut cleanup =
            ottrin_platform::RetainedCopyCleanupPlan::observe(&[transaction], &mut |_| Ok(()))
                .unwrap();
        cleanup.reclaim(&mut |_| Ok(())).unwrap();
        assert!(cleanup.complete());
    }
    #[cfg(target_os = "linux")]
    {
        let observed = ottrin_platform::observe_retained_copy_cleanup(&[transaction]).unwrap();
        let mut cleanup = ottrin_platform::plan_retained_copy_cleanup(&observed).unwrap();
        while cleanup.reclaim_payload_next().unwrap() {}
        cleanup.begin_finalization().unwrap().finish().unwrap();
    }
    assert_eq!(
        std::fs::read(source).unwrap(),
        b"ordinary copy remains owned"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn windows_helper_history_maps_only_the_selected_explicit_recovery_action() {
    use ottrin_core::PrivilegedCopyRecoveryAction as Action;
    for action in [Action::Resume, Action::EndAttempt, Action::ReleaseName] {
        let Some(PrivilegedPayload::Copy(reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        let entry = ottrin_core::PrivilegedCopyHistoryEntry {
            reference,
            recovery: PrivilegedCopyHistoryState::RecoveryAvailable { action },
        };
        validate_history(std::slice::from_ref(&entry)).unwrap();
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        app.merge_privileged_copy_history(vec![entry]);
        let record = &app.operation_history[0];
        assert!(copy_activity_recovery::can_recover(record));
        assert!(!copy_activity_cleanup::can_cleanup(record));
        let mut plan = record
            .retained_copy
            .as_ref()
            .unwrap()
            .recovery
            .clone()
            .unwrap();
        assert_eq!(plan.is_end_attempt(), action == Action::EndAttempt);
        assert_eq!(plan.is_name_release(), action == Action::ReleaseName);
        assert_eq!(
            plan.released_phase(),
            None,
            "A sent lookup cannot invent a native phase"
        );
        let platform = ProbePlatform::new(PrivilegedStatus::Success);
        let mut progress = Vec::new();
        plan.run(&platform, |bytes| {
            progress.push(bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(progress, [0, 23]);
        let commands = platform.commands.lock().unwrap();
        assert!(
            matches!(commands.as_slice(), [PrivilegedCommand::RecoverCopy {
            transaction, action: sent,
        }] if transaction == &"a".repeat(32) && sent == &action)
        );
        let text = crate::tests::activity_text(&mut app, &Context::default());
        assert!(text.iter().any(|text| text == plan.label()), "{text:?}");
    }
}

#[test]
#[cfg(target_os = "windows")]
fn ordinary_discovery_cannot_downgrade_or_duplicate_helper_owned_history() {
    let Some(PrivilegedPayload::Copy(reference)) = response(PrivilegedStatus::Success, 1).payload
    else {
        unreachable!()
    };
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.merge_privileged_copy_history(vec![ottrin_core::PrivilegedCopyHistoryEntry {
        reference,
        recovery: PrivilegedCopyHistoryState::UndoAvailable,
    }]);
    let id = app.operation_history[0].id;
    let mut ownership = app.operation_history[0].retained_copy.clone().unwrap();
    ownership.key = "ordinary-discovery-of-same-transaction".into();
    ownership.cleanup_phase = None;
    app.merge_retained_copy_activity(Vec::new());
    app.merge_retained_copy_activity(vec![RestoredCopyActivity {
        ownership,
        detail: "Ambient user could not inspect protected payload".into(),
        undo: PathMoveUndoState::Unavailable {
            reason: "Protected payload".into(),
        },
        receipt: None,
    }]);
    assert_eq!(app.operation_history.len(), 1);
    assert_eq!(app.operation_history[0].id, id);
    assert!(is_privileged(&app.operation_history[0]));
    assert_eq!(
        app.operation_history[0].path_move_undo,
        Some(PathMoveUndoState::Available)
    );
}

#[test]
#[cfg(target_os = "windows")]
fn unrecognized_helper_records_stay_visible_without_inventing_mutation_authority() {
    let entry = ottrin_core::PrivilegedCopyHistoryEntry {
        reference: ottrin_core::PrivilegedCopyReceipt {
            transactions: Vec::new(),
            paths: Vec::new(),
            completed_sources: 0,
        },
        recovery: PrivilegedCopyHistoryState::NeedsReview {
            reason: "Unrecognized retained Copy data; preserved for review".into(),
        },
    };
    validate_history(std::slice::from_ref(&entry)).unwrap();
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.merge_privileged_copy_history(vec![entry.clone()]);
    app.merge_privileged_copy_history(vec![entry]);
    assert!(app.operation_history.is_empty());
    assert_eq!(app.privileged_copy_history.review.len(), 1);
    let ctx = Context::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 300.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.render_privileged_copy_history(ui));
        },
    );
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Unrecognized retained Copy data"))));
}

#[test]
#[cfg(target_os = "windows")]
fn helper_cleanup_cohort_coalesces_only_inactive_restored_cards() {
    let Some(PrivilegedPayload::Copy(reference)) = response(PrivilegedStatus::Success, 1).payload
    else {
        unreachable!()
    };
    let mut second = reference.clone();
    second.transactions = vec!["b".repeat(32)];
    second.paths[0].1 = owned("/output/b");
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.merge_privileged_copy_history(vec![
        ottrin_core::PrivilegedCopyHistoryEntry {
            reference: reference.clone(),
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        },
        ottrin_core::PrivilegedCopyHistoryEntry {
            reference: second,
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        },
    ]);
    let first = app.operation_history[0].id;
    let cohort = ottrin_core::PrivilegedCopyHistoryEntry {
        reference: ottrin_core::PrivilegedCopyReceipt {
            transactions: vec!["a".repeat(32), "b".repeat(32)],
            paths: Vec::new(),
            completed_sources: 0,
        },
        recovery: PrivilegedCopyHistoryState::CleanupPending,
    };
    app.merge_privileged_copy_history(vec![cohort.clone()]);
    assert_eq!(app.operation_history.len(), 1);
    assert_eq!(app.operation_history[0].id, first);
    assert_eq!(
        copy_receipt_transaction_ids(&app.operation_history[0]),
        cohort.reference.transactions
    );
    assert!(copy_activity_cleanup::can_cleanup(
        &app.operation_history[0]
    ));
    assert!(!path_move_undo_action_is_available(
        app.operation_history[0].status,
        app.operation_history[0].path_move_undo.as_ref()
    ));
    app.operation_history.clear();
    app.record_privileged_copy(
        &command(),
        &response(PrivilegedStatus::Failed, 1),
        Some(7),
        Vec::new(),
    )
    .unwrap();
    app.merge_privileged_copy_history(vec![cohort]);
    assert_eq!(app.operation_history.len(), 1);
    assert!(!is_restored(&app.operation_history[0]));
    assert_eq!(
        app.operation_history[0]
            .path_move_undo_token
            .as_ref()
            .unwrap()
            .origin_tab_id,
        Some(7)
    );
    assert_eq!(app.privileged_copy_history.review.len(), 1);
}

#[test]
#[cfg(target_os = "windows")]
fn administrator_undo_preparation_keeps_lookup_references_out_of_ambient_filesystem_admission() {
    let Some(PrivilegedPayload::Copy(reference)) = response(PrivilegedStatus::Success, 1).payload
    else {
        unreachable!()
    };
    let receipt = OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
        reference: reference.clone(),
        pending: true,
        restored: true,
    });
    // These presentation paths do not exist. Only the authenticated helper may
    // open the recorded transaction; ordinary Undo preparation must not try.
    let prepared = copy_activity_work::refresh_undo_receipt(&receipt, &mut |_| Ok(())).unwrap();
    assert!(
        matches!(prepared, OperationUndoReceipt::CopyBatch(CopyBatchUndoReceipt::Privileged {
        reference: actual, pending: true, restored: true,
    }) if actual == reference)
    );
    assert!(
        copy_activity_work::refresh_undo_receipt(&receipt, &mut |_| Err(PlatformError::Io(
            "Owned admission Stop".into()
        )))
        .is_err()
    );
}

#[test]
#[cfg(target_os = "windows")]
#[ignore = "Requires the declared native helper executable in the owned Windows fixture"]
fn windows_helper_protocol_and_activity_capability_agree_without_elevation() {
    struct Child(Option<std::process::Child>);
    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                if let Err(error) = child.kill() {
                    eprintln!("Owned helper probe stop failed: {error}");
                }
                if let Err(error) = child.wait() {
                    eprintln!("Owned helper probe wait failed: {error}");
                }
            }
        }
    }
    let helper =
        std::env::var_os("OTTRIN_QA_PRIV_HELPER").expect("Declared helper fixture missing");
    let version = ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string();
    let supported = ottrin_platform::privileged_copy::recovery_supported();
    for (arguments, expected) in [
        (
            vec!["--check-protocol", version.as_str(), "--bound-local-pipe"],
            0,
        ),
        (
            vec![
                "--check-protocol",
                version.as_str(),
                "--bound-local-pipe",
                "--copy-recovery",
            ],
            if supported { 0 } else { 69 },
        ),
        (vec!["--check-protocol", "4", "--bound-local-pipe"], 78),
    ] {
        let mut child = Child(Some(
            std::process::Command::new(&helper)
                .args(&arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.0.as_mut().unwrap().try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "Owned helper probe did not exit");
            std::thread::sleep(Duration::from_millis(10));
        };
        child.0.take(); // Already reaped. The guard owns every earlier failure.
        assert_eq!(status.code(), Some(expected), "{arguments:?}");
    }
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    let text = crate::tests::activity_text(&mut app, &Context::default());
    assert_eq!(
        text.iter()
            .any(|text| text == "Load administrator Copy history"),
        supported
    );
    println!(
        "native helper protocol and Activity capability agree: supported={supported}; current basic pipe accepted, unsupported Copy refused, old protocol rejected; no elevation"
    );
}

#[test]
fn missing_or_partial_history_retains_the_complete_cohort_without_stale_undo() {
    for partial in [false, true] {
        let mut app = OttrinApp::default();
        app.operation_history.clear();
        let Some(PrivilegedPayload::Copy(mut reference)) =
            response(PrivilegedStatus::Success, 1).payload
        else {
            unreachable!()
        };
        reference.transactions.push("b".repeat(32));
        reference.paths.push(reference.paths[0].clone());
        let entry = ottrin_core::PrivilegedCopyHistoryEntry {
            reference: reference.clone(),
            recovery: PrivilegedCopyHistoryState::UndoAvailable,
        };
        app.merge_privileged_copy_history(vec![entry.clone()]);
        let original = app.operation_history[0].id;
        // A current-session helper receipt is independently owned and must survive
        // even a successful empty snapshot unchanged.
        let mut live = response(PrivilegedStatus::Success, 1);
        if let Some(PrivilegedPayload::Copy(receipt)) = &mut live.payload {
            receipt.transactions = vec!["c".repeat(32)];
        }
        app.record_privileged_copy(&command(), &live, Some(7), Vec::new())
            .unwrap();
        let live_id = app.operation_history[0].id;
        let snapshot = if partial {
            let mut part = entry.clone();
            part.reference.transactions.truncate(1);
            part.reference.paths.truncate(1);
            vec![part]
        } else {
            Vec::new()
        };
        app.merge_privileged_copy_history(snapshot);
        assert_eq!(app.operation_history.len(), 2);
        let stale = app
            .operation_history
            .iter()
            .find(|r| r.id == original)
            .unwrap();
        assert_eq!(history_ids(stale), reference.transactions);
        assert!(is_restored(stale));
        assert_eq!(stale.status, OperationStatus::NeedsReview);
        assert!(matches!(
            stale.path_move_undo,
            Some(PathMoveUndoState::Unavailable { .. })
        ));
        assert_eq!(
            match &stale.path_move_undo_token.as_ref().unwrap().receipt {
                OperationUndoReceipt::CopyBatch(receipt) => receipt.remaining(),
                _ => unreachable!(),
            },
            0
        );
        assert!(stale.message.as_ref().unwrap().contains("did not confirm"));
        let live = app
            .operation_history
            .iter()
            .find(|r| r.id == live_id)
            .unwrap();
        assert!(!is_restored(live));
        assert_eq!(live.status, OperationStatus::Completed);
        assert!(matches!(
            live.path_move_undo,
            Some(PathMoveUndoState::Available)
        ));
        #[cfg(target_os = "windows")]
        {
            assert!(!copy_activity_cleanup::can_cleanup(stale));
            assert!(!copy_activity_recovery::can_recover(stale));
        }
        app.merge_privileged_copy_history(vec![entry]);
        let renewed = app
            .operation_history
            .iter()
            .find(|r| r.id == original)
            .unwrap();
        assert_eq!(history_ids(renewed), reference.transactions);
        assert!(matches!(
            renewed.path_move_undo,
            Some(PathMoveUndoState::Available)
        ));
        app.clear_finished_activity();
        assert_eq!(app.operation_history.len(), 2);
    }
}
