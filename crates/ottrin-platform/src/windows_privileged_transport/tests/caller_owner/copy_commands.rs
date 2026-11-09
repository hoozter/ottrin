//! Actual framed commands over the existing kernel-bound ordinary/SYSTEM pipe.
use super::*;
use ottrin_core::{ConflictAction, FileCommand, PrivilegedCopyHistoryState, PrivilegedCopyReceipt};

const READY: &str = "owned caller Copy command route ready";
const DONE: &str = "owned caller Copy command route complete";
const STOP: &str = "owned caller Copy stop callback";
const STREAM: &str = "owned caller Copy streamed command";

pub(super) fn streamed(
    pipe: &mut LocalPipeStream,
    command: PrivilegedCommand,
) -> (PrivilegedResponse, Vec<u64>) {
    let mut values = Vec::new();
    let response = crate::windows_privileged_transport::copy_stream::exchange(
        pipe,
        &request(command, STREAM),
        &mut |bytes| {
            values.push(bytes);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        response.status,
        PrivilegedStatus::Success,
        "{:?}",
        response.message
    );
    (response, values)
}

pub(super) fn request(command: PrivilegedCommand, reason: &str) -> PrivilegedRequest {
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
pub(super) fn history(pipe: &mut LocalPipeStream) -> Vec<ottrin_core::PrivilegedCopyHistoryEntry> {
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

pub(super) fn exercise_symlink_overwrite(
    channel: &HelperChannel,
    caller: &CallerCopyContext,
    root: &Path,
) {
    use std::os::windows::fs::{OpenOptionsExt, symlink_dir, symlink_file};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let execute = |command| {
        let response = channel.execute_copy_command(&command).unwrap();
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        response.payload
    };
    let open_link = |path: &Path| {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .unwrap()
    };
    let destination = root.join("system-only");
    let probe = root.join("probe-data");
    for directory in [false, true] {
        let link_name = format!("caller-wire-link-{directory}");
        let source_link = probe.join(&link_name);
        let published = destination.join(&link_name);
        let source_target = probe.join(format!("new-link-target-{directory}"));
        let original_target = probe.join(format!("old-link-target-{directory}"));
        let target_data = |target: &Path| {
            if directory {
                target.join("member.txt")
            } else {
                target.to_path_buf()
            }
        };
        caller
            .with_caller_token(|| {
                if directory {
                    std::fs::create_dir(&source_target)?;
                    std::fs::create_dir(&original_target)?;
                }
                std::fs::write(target_data(&source_target), b"source link target")?;
                std::fs::write(target_data(&original_target), b"original link target")
            })
            .unwrap();
        let make_link = if directory { symlink_dir } else { symlink_file };
        make_link(&source_target, &source_link).unwrap();
        make_link(&original_target, &published).unwrap();
        let original = open_link(&published);
        let original_id = crate::move_recovery::stable_identity_from_handle(&original).unwrap();
        let original_security =
            crate::copy_transaction::owner_group_dacl_digest(&original).unwrap();
        drop(original);
        let source_link_target = caller
            .with_caller_token(|| std::fs::read_link(&source_link))
            .unwrap();
        let original_link_target = std::fs::read_link(&published).unwrap();
        assert_ne!(source_link_target, original_link_target);
        let Some(PrivilegedPayload::Copy(receipt)) =
            execute(PrivilegedCommand::File(FileCommand::Copy {
                sources: vec![source_link.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Overwrite,
            }))
        else {
            panic!("Missing symbolic-link Copy receipt");
        };
        assert_eq!(receipt.completed_sources, 1);
        assert_eq!(receipt.transactions.len(), 1);
        let [(retained, publication)] = receipt.paths.as_slice() else {
            panic!("Expected one retained candidate and publication");
        };
        for path in [retained, publication] {
            assert_eq!(
                path.parent().unwrap().canonicalize().unwrap(),
                destination.canonicalize().unwrap()
            );
        }
        assert_eq!(publication.file_name(), published.file_name());
        assert_eq!(
            retained.file_name().unwrap(),
            OsStr::new(&format!(
                ".ottrin-copy-{}.candidate",
                receipt.transactions[0]
            ))
        );
        assert_eq!(std::fs::read_link(&published).unwrap(), source_link_target);
        assert_eq!(
            file_owner(&open_link(&published)),
            caller.default_file_owner_sid().unwrap()
        );
        execute(PrivilegedCommand::UndoCopy {
            transactions: receipt.transactions.clone(),
        });
        assert_eq!(
            std::fs::read_link(&published).unwrap(),
            original_link_target
        );
        let restored = open_link(&published);
        assert_eq!(
            crate::move_recovery::stable_identity_from_handle(&restored).unwrap(),
            original_id
        );
        assert_eq!(
            crate::copy_transaction::owner_group_dacl_digest(&restored).unwrap(),
            original_security
        );
        drop(restored);
        execute(PrivilegedCommand::CleanupCopy {
            transactions: receipt.transactions,
        });
        assert_eq!(
            std::fs::read_link(&source_link).unwrap(),
            source_link_target
        );
        assert_eq!(
            std::fs::read(target_data(&source_target)).unwrap(),
            b"source link target"
        );
        assert_eq!(
            std::fs::read(target_data(&original_target)).unwrap(),
            b"original link target"
        );
        println!(
            "caller symbolic-link overwrite directory={directory}: publication, original identity and security restoration, cleanup and target preservation passed"
        );
    }
}

pub(super) fn exercise_readonly_alias_overwrite(
    channel: &HelperChannel,
    caller: &CallerCopyContext,
    root: &Path,
) {
    use crate::copy_transaction::owner_group_dacl_digest;
    use crate::move_recovery::stable_identity_from_handle;
    use std::{
        fs,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_READONLY, SetFileAttributesW};

    let execute = |command| {
        let response = channel.execute_copy_command(&command).unwrap();
        assert_eq!(
            response.status,
            PrivilegedStatus::Success,
            "{:?}",
            response.message
        );
        response.payload
    };
    let destination = root.join("system-only");
    for undo in [true, false] {
        let name = format!("caller-readonly-alias-{undo}.txt");
        let source = root.join("probe-data").join(&name);
        let target = destination.join(&name);
        let alias = destination.join(format!("outside-{name}"));
        let named_stream = |path: &Path| {
            let mut stream = path.as_os_str().to_os_string();
            stream.push(":ottrin-test");
            PathBuf::from(stream)
        };
        caller
            .with_caller_token(|| fs::write(&source, b"replacement payload"))
            .unwrap();
        fs::write(&target, b"original payload").unwrap();
        fs::write(named_stream(&target), b"original named stream").unwrap();
        fs::hard_link(&target, &alias).unwrap();
        let original_permissions = fs::metadata(&target).unwrap().permissions();
        // Match the installed fixture: the read-only bit is set and the
        // archive bit is clear before either original rename.
        let wide_target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
        assert_ne!(
            unsafe { SetFileAttributesW(wide_target.as_ptr(), FILE_ATTRIBUTE_READONLY) },
            0
        );
        assert_eq!(
            fs::metadata(&target).unwrap().file_attributes(),
            FILE_ATTRIBUTE_READONLY
        );
        let original = fs::File::open(&target).unwrap();
        let identity = stable_identity_from_handle(&original).unwrap();
        let security = owner_group_dacl_digest(&original).unwrap();
        drop(original);
        let assert_original = |path: &Path| {
            let file = fs::File::open(path).unwrap();
            assert_eq!(stable_identity_from_handle(&file).unwrap(), identity);
            assert_eq!(owner_group_dacl_digest(&file).unwrap(), security);
            assert_eq!(
                file.metadata().unwrap().file_attributes(),
                FILE_ATTRIBUTE_READONLY
            );
            assert_eq!(fs::read(path).unwrap(), b"original payload");
            assert_eq!(
                fs::read(named_stream(path)).unwrap(),
                b"original named stream"
            );
        };
        let Some(PrivilegedPayload::Copy(receipt)) =
            execute(PrivilegedCommand::File(FileCommand::Copy {
                sources: vec![source.clone()],
                destination: destination.clone(),
                conflict: ConflictAction::Overwrite,
            }))
        else {
            panic!("Missing read-only hard-link Copy receipt");
        };
        assert_eq!(receipt.completed_sources, 1);
        assert_eq!(receipt.transactions.len(), 1);
        let retained =
            destination.join(format!(".ottrin-copy-{}.original", receipt.transactions[0]));
        assert_eq!(fs::read(&target).unwrap(), b"replacement payload");
        assert_original(&alias);
        assert_original(&retained);

        if undo {
            execute(PrivilegedCommand::UndoCopy {
                transactions: receipt.transactions.clone(),
            });
            assert_original(&target);
        }
        execute(PrivilegedCommand::CleanupCopy {
            transactions: receipt.transactions,
        });
        assert!(!retained.exists());
        assert_original(&alias);
        if undo {
            assert_original(&target);
        } else {
            assert_eq!(fs::read(&target).unwrap(), b"replacement payload");
        }
        assert_eq!(fs::read(&source).unwrap(), b"replacement payload");

        // The fixture owns both names and has finished observing their flags.
        fs::set_permissions(&alias, original_permissions).unwrap();
        for path in [alias, target, source] {
            fs::remove_file(path).unwrap();
        }
        println!(
            "caller read-only hard-link overwrite undo={undo}: alias identity, permissions, streams and cleanup passed"
        );
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
    let before = history(pipe);
    let id = "a".repeat(32);
    let mut stopped = vec![
        PrivilegedCommand::File(FileCommand::Copy {
            sources: vec![wire_file.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Rename,
        }),
        PrivilegedCommand::DiscoverCopyHistory,
        PrivilegedCommand::UndoCopy {
            transactions: vec![id.clone()],
        },
        PrivilegedCommand::CleanupCopy {
            transactions: vec![id.clone()],
        },
    ];
    stopped.extend(
        [
            ottrin_core::PrivilegedCopyRecoveryAction::Resume,
            ottrin_core::PrivilegedCopyRecoveryAction::EndAttempt,
            ottrin_core::PrivilegedCopyRecoveryAction::ReleaseName,
        ]
        .map(|action| PrivilegedCommand::RecoverCopy {
            transaction: id.clone(),
            action,
        }),
    );
    for command in stopped {
        write_json(pipe, &request(command, STOP)).unwrap();
        let response: PrivilegedResponse = read_json(pipe).unwrap();
        assert_eq!(response.status, PrivilegedStatus::Failed);
        assert!(response.message.unwrap().contains("Owned callback stop"));
        if let Some(PrivilegedPayload::Copy(reference)) = response.payload {
            assert_eq!(reference.completed_sources, 0);
            assert!(reference.transactions.is_empty() && reference.paths.is_empty());
        }
    }
    assert_eq!(history(pipe), before);
    println!(
        "caller helper seven command kinds stop before work with unchanged history and no completed Copy references"
    );
    let (response, progress) = streamed(
        pipe,
        PrivilegedCommand::File(FileCommand::Copy {
            sources: vec![wire_file.clone()],
            destination: destination.clone(),
            conflict: ConflictAction::Rename,
        }),
    );
    let Some(PrivilegedPayload::Copy(reference)) = response.payload else {
        panic!("Stream omitted Copy receipt")
    };
    assert_eq!(reference.completed_sources, 1);
    assert_eq!(reference.transactions.len(), 1);
    assert!(progress.iter().any(|bytes| *bytes > 0));
    let (response, _) = streamed(pipe, PrivilegedCommand::DiscoverCopyHistory);
    let Some(PrivilegedPayload::CopyHistory(entries)) = response.payload else {
        panic!("Stream omitted history")
    };
    assert!(entries.iter().any(
        |entry| entry.reference.transactions == reference.transactions
            && entry.recovery == PrivilegedCopyHistoryState::UndoAvailable
    ));
    streamed(
        pipe,
        PrivilegedCommand::UndoCopy {
            transactions: reference.transactions.clone(),
        },
    );
    streamed(
        pipe,
        PrivilegedCommand::CleanupCopy {
            transactions: reference.transactions,
        },
    );
    println!(
        "caller helper streamed Copy/history/Undo/cleanup: actual byte progress, original-caller publication and actionable receipts passed"
    );
    super::stream_commands::source_stop(pipe, &wire_file, &destination, None);
    super::stream_commands::source_stop(pipe, &wire_file, &destination, Some(&tree));
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
        if matches!(
            request.context.reason.as_deref(),
            Some(STREAM) | Some(super::stream_commands::REASON)
        ) {
            channel
                .serve_copy_command(&request.command)
                .unwrap()
                .unwrap();
            assert_eq!(token_state(), original_token);
            if let PrivilegedCommand::File(FileCommand::Copy {
                sources,
                destination,
                ..
            }) = &request.command
            {
                let caller = Arc::new(channel.caller_copy_context().unwrap());
                let published = if request.context.reason.as_deref()
                    == Some(super::stream_commands::REASON)
                {
                    assert!((1..=2).contains(&sources.len()));
                    let (stopped, prefix) = sources.split_last().unwrap();
                    assert_eq!(
                        std::fs::symlink_metadata(destination.join(stopped.file_name().unwrap()))
                            .unwrap_err()
                            .kind(),
                        std::io::ErrorKind::NotFound
                    );
                    prefix
                } else {
                    assert_eq!(sources.len(), 1);
                    sources.as_slice()
                };
                for source in published {
                    crate::WindowsCopyRegistry::assert_caller_wire_publication(
                        Arc::clone(&caller),
                        &destination.join(source.file_name().unwrap()),
                    );
                }
            }
            count += 1;
            continue;
        }
        let response = if request.context.reason.as_deref() == Some(STOP) {
            let mut callbacks = 0;
            let response = channel
                .execute_copy_command_with_progress(&request.command, &mut |_| {
                    callbacks += 1;
                    Err(crate::PlatformError::Io("Owned callback stop".into()))
                })
                .unwrap();
            assert_eq!(callbacks, 1);
            response
        } else {
            recovery.execute(channel, &request)
        };
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
