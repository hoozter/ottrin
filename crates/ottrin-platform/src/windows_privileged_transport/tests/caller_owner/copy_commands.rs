//! Actual framed commands over the existing kernel-bound ordinary/SYSTEM pipe.
use super::*;
use ottrin_core::{ConflictAction, FileCommand, PrivilegedCopyHistoryState, PrivilegedCopyReceipt};

const READY: &str = "owned caller Copy command route ready";
const DONE: &str = "owned caller Copy command route complete";

fn request(command: PrivilegedCommand, reason: &str) -> PrivilegedRequest {
    PrivilegedRequest {
        protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
        command,
        context: PrivilegedContext {
            reason: Some(reason.into()),
            cwd: None,
        },
    }
}
fn exchange(pipe: &mut LocalPipeStream, command: PrivilegedCommand) -> PrivilegedResponse {
    write_json(pipe, &request(command, "owned caller Copy command")).unwrap();
    read_json(pipe).unwrap()
}
fn copy(
    pipe: &mut LocalPipeStream,
    sources: Vec<PathBuf>,
    destination: &Path,
    conflict: ConflictAction,
    status: PrivilegedStatus,
) -> PrivilegedCopyReceipt {
    let response = exchange(
        pipe,
        PrivilegedCommand::File(FileCommand::Copy {
            sources,
            destination: destination.into(),
            conflict,
        }),
    );
    assert_eq!(response.status, status, "{:?}", response.message);
    let Some(PrivilegedPayload::Copy(receipt)) = response.payload else {
        panic!("Missing partial/success Copy payload")
    };
    receipt
}
fn history(pipe: &mut LocalPipeStream) -> Vec<ottrin_core::PrivilegedCopyHistoryEntry> {
    let response = exchange(pipe, PrivilegedCommand::DiscoverCopyHistory);
    assert_eq!(
        response.status,
        PrivilegedStatus::Success,
        "{:?}",
        response.message
    );
    let Some(PrivilegedPayload::CopyHistory(history)) = response.payload else {
        panic!("Missing history")
    };
    history
}
fn assert_state(pipe: &mut LocalPipeStream, ids: &[String], state: PrivilegedCopyHistoryState) {
    let entries = history(pipe);
    for id in ids {
        let entry = entries
            .iter()
            .find(|entry| entry.reference.transactions.contains(id))
            .unwrap();
        assert_eq!(entry.recovery, state);
        assert_eq!(entry.reference.completed_sources, 0);
    }
}

pub(super) fn client(pipe: &mut LocalPipeStream, root: &Path, source: &Path) {
    let ready: PrivilegedResponse = read_json(pipe).unwrap();
    assert_eq!(ready.status, PrivilegedStatus::Success);
    assert_eq!(ready.message.as_deref(), Some(READY));
    let destination = root.join("system-only");
    let wire_file = root.join("probe-data/wire-source-file Ω.txt");
    std::fs::write(&wire_file, b"unchanged caller source").unwrap();
    let tree = root.join("probe-data/wire-source-tree Ω");
    std::fs::create_dir(&tree).unwrap();
    std::fs::write(tree.join("member.txt"), b"wire directory payload").unwrap();
    for source in [wire_file.as_path(), tree.as_path()] {
        let first = copy(
            pipe,
            vec![source.into()],
            &destination,
            ConflictAction::Rename,
            PrivilegedStatus::Success,
        );
        assert_eq!(first.completed_sources, 1);
        assert_eq!(first.transactions.len(), 1);
        let skipped = copy(
            pipe,
            vec![source.into()],
            &destination,
            ConflictAction::Skip,
            PrivilegedStatus::Success,
        );
        assert_eq!(skipped.completed_sources, 1);
        assert!(skipped.transactions.is_empty() && skipped.paths.is_empty());
        let renamed = copy(
            pipe,
            vec![source.into()],
            &destination,
            ConflictAction::Rename,
            PrivilegedStatus::Success,
        );
        assert_eq!(renamed.transactions.len(), 1);
        assert_ne!(first.paths[0].1, renamed.paths[0].1);
        let protected_history = history(pipe);
        let refused = copy(
            pipe,
            vec![source.into()],
            &destination,
            ConflictAction::Overwrite,
            PrivilegedStatus::Failed,
        );
        assert_eq!(refused.completed_sources, 0);
        assert!(refused.transactions.is_empty());
        assert_eq!(history(pipe), protected_history);
        let mut ids = first.transactions;
        ids.extend(renamed.transactions);
        let before = history(pipe);
        assert_eq!(
            exchange(
                pipe,
                PrivilegedCommand::UndoCopy {
                    transactions: vec![ids[0].clone(), ids[0].clone()]
                }
            )
            .status,
            PrivilegedStatus::Failed
        );
        assert_eq!(
            exchange(
                pipe,
                PrivilegedCommand::UndoCopy {
                    transactions: vec![ids[0].clone(), "0".repeat(32)]
                }
            )
            .status,
            PrivilegedStatus::Failed
        );
        assert_eq!(history(pipe), before);
        let response = exchange(
            pipe,
            PrivilegedCommand::UndoCopy {
                transactions: ids.clone(),
            },
        );
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        assert_state(pipe, &ids, PrivilegedCopyHistoryState::Undone);
        assert_eq!(
            exchange(
                pipe,
                PrivilegedCommand::CleanupCopy {
                    transactions: vec![ids[0].clone(), ids[0].clone()]
                }
            )
            .status,
            PrivilegedStatus::Failed
        );
        let response = exchange(
            pipe,
            PrivilegedCommand::CleanupCopy {
                transactions: ids.clone(),
            },
        );
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        assert!(history(pipe).iter().all(|entry| {
            entry
                .reference
                .transactions
                .iter()
                .all(|id| !ids.contains(id))
        }));
        // Overwrite an independent existing original, never a live receipt.
        let overwrite_source = root.join(if source == tree {
            "probe-data/wire-overwrite-tree Ω"
        } else {
            "probe-data/wire-overwrite-file Ω.txt"
        });
        if source == tree {
            std::fs::create_dir(&overwrite_source).unwrap();
            std::fs::write(
                overwrite_source.join("member.txt"),
                b"wire directory payload",
            )
            .unwrap();
        } else {
            std::fs::write(&overwrite_source, b"unchanged caller source").unwrap();
        }
        let overwritten = copy(
            pipe,
            vec![overwrite_source],
            &destination,
            ConflictAction::Overwrite,
            PrivilegedStatus::Success,
        );
        assert_eq!(overwritten.completed_sources, 1);
        assert_eq!(overwritten.transactions.len(), 1);
        assert_state(
            pipe,
            &overwritten.transactions,
            PrivilegedCopyHistoryState::UndoAvailable,
        );
        let response = exchange(
            pipe,
            PrivilegedCommand::UndoCopy {
                transactions: overwritten.transactions.clone(),
            },
        );
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        assert_state(
            pipe,
            &overwritten.transactions,
            PrivilegedCopyHistoryState::Undone,
        );
        let response = exchange(
            pipe,
            PrivilegedCommand::CleanupCopy {
                transactions: overwritten.transactions,
            },
        );
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        println!(
            "caller helper wire source_directory={}: creation/Skip/Rename, retained-overlap refusal, independent Overwrite/original restoration, fresh history, invalid selection preservation, reverse Undo and cohort cleanup passed",
            source == tree
        );
    }
    // A failed later item must keep the earlier completion's actionable receipt.
    let partial = copy(
        pipe,
        vec![
            wire_file.clone(),
            root.join("probe-data/missing-wire-source"),
        ],
        &destination,
        ConflictAction::Rename,
        PrivilegedStatus::Failed,
    );
    assert_eq!(partial.completed_sources, 1);
    assert_eq!(partial.transactions.len(), 1);
    assert_state(
        pipe,
        &partial.transactions,
        PrivilegedCopyHistoryState::UndoAvailable,
    );
    assert_eq!(
        exchange(
            pipe,
            PrivilegedCommand::UndoCopy {
                transactions: partial.transactions.clone()
            }
        )
        .status,
        PrivilegedStatus::Success
    );
    assert_eq!(
        exchange(
            pipe,
            PrivilegedCommand::CleanupCopy {
                transactions: partial.transactions
            }
        )
        .status,
        PrivilegedStatus::Success
    );
    assert_eq!(
        exchange(
            pipe,
            PrivilegedCommand::UndoCopy {
                transactions: vec!["../not-a-reference".into()]
            }
        )
        .status,
        PrivilegedStatus::Failed
    );
    assert_eq!(
        exchange(
            pipe,
            PrivilegedCommand::CleanupCopy {
                transactions: Vec::new()
            }
        )
        .status,
        PrivilegedStatus::Failed
    );
    println!(
        "caller helper wire partial batch preserves completed receipt; invalid/missing/duplicate selections refused without mutation"
    );
    super::recovery_commands::client(pipe, root);
    write_json(
        pipe,
        &request(
            PrivilegedCommand::ListDirectory {
                path: destination,
                show_hidden: false,
            },
            DONE,
        ),
    )
    .unwrap();
    assert_eq!(std::fs::read(source).unwrap(), b"unchanged caller source");
    assert_eq!(
        std::fs::read(tree.join("member.txt")).unwrap(),
        b"wire directory payload"
    );
}

pub(super) fn serve(
    channel: &mut HelperChannel,
    root: &Path,
    preferences: &std::fs::File,
    index: &std::fs::File,
) {
    let mut recovery = super::recovery_commands::Server::new(channel, root, preferences, index);
    let original_token = token_state();
    let original_file = root.join("system-only/wire-overwrite-file Ω.txt");
    let original_tree = root.join("system-only/wire-overwrite-tree Ω");
    std::fs::write(&original_file, b"independent original file").unwrap();
    std::fs::create_dir(&original_tree).unwrap();
    let original_member = original_tree.join("original.txt");
    std::fs::write(&original_member, b"independent original directory member").unwrap();
    let originals = [original_file, original_tree, original_member];
    let snapshot = || {
        originals
            .iter()
            .map(|path| {
                let directory = path.is_dir();
                let file = if directory {
                    crate::windows_fs::open_directory(path, FILE_GENERIC_READ).unwrap()
                } else {
                    std::fs::File::open(path).unwrap()
                };
                (
                    crate::move_recovery::stable_identity_from_handle(&file).unwrap(),
                    crate::copy_transaction::owner_group_dacl_digest(&file).unwrap(),
                    if directory {
                        None
                    } else {
                        Some(std::fs::read(path).unwrap())
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    let original_state = snapshot();
    let names = || {
        std::fs::read_dir(root.join("system-only"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let before = names();
    channel
        .write_response(&PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: Some(READY.into()),
            payload: None,
        })
        .unwrap();
    let mut count = 0;
    loop {
        let request = channel.read_request().unwrap();
        if request.context.reason.as_deref() == Some(DONE) {
            assert!(matches!(
                request.command,
                PrivilegedCommand::ListDirectory { .. }
            ));
            assert!(channel.execute_copy_command(&request.command).is_none());
            break;
        }
        let response = recovery.execute(channel, &request);
        assert_eq!(token_state(), original_token);
        if let Some(PrivilegedPayload::Copy(receipt)) = &response.payload {
            for (_, publication) in &receipt.paths {
                let caller = Arc::new(channel.caller_copy_context().unwrap());
                crate::WindowsCopyRegistry::assert_caller_wire_publication(caller, publication);
            }
        }
        assert_eq!(token_state(), original_token);
        channel.write_response(&response).unwrap();
        count += 1;
        assert!(count <= 160);
    }
    assert!(count >= 120);
    recovery.finish();
    let expected = before
        .into_iter()
        .chain(recovery.parents())
        .chain([
            OsString::from(".ottrin-copy-target-claims"),
            OsString::from(".ottrin-copy-target-claims-root"),
        ])
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names(), expected);
    let caller = Arc::new(channel.caller_copy_context().unwrap());
    let parent =
        crate::windows_fs::open_directory(&root.join("system-only"), FILE_GENERIC_READ).unwrap();
    crate::WindowsCopyRegistry::assert_caller_destination_claims(&caller, &parent);
    assert_eq!(snapshot(), original_state);
    println!(
        "caller helper wire {count} real framed commands passed; protected publications verified, original helper token, original identities/security/bytes and unrelated namespace preserved"
    );
}
