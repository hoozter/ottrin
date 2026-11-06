//! Interrupted states are owned fixture seeds; all recovery uses actual framed
//! requests through the same authenticated routing method as the helper binary.
use super::*;
use ottrin_core::{
    PrivilegedCopyHistoryEntry, PrivilegedCopyHistoryState, PrivilegedCopyRecoveryAction,
};
use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
};

const PREPARE: &str = "owned caller Copy recovery prepare ";
const INSPECT: &str = "owned caller Copy recovery inspect ";
const RELEASE_FAULT: &str = "owned caller Copy recovery release fault";
type Check = Box<dyn FnMut()>;
type UserData = [(
    Vec<u8>,
    crate::move_recovery::WindowsStableIdentity,
    [u8; 32],
); 2];

fn observe(caller: &CallerCopyContext, files: &[File; 2]) -> UserData {
    caller
        .with_caller_token(|| {
            Ok(files.each_ref().map(|file| {
                let mut reader = file.try_clone().unwrap();
                std::io::Seek::rewind(&mut reader).unwrap();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut reader, &mut bytes).unwrap();
                (
                    bytes,
                    crate::move_recovery::stable_identity_from_handle(file).unwrap(),
                    crate::copy_transaction::owner_group_dacl_digest(file).unwrap(),
                )
            }))
        })
        .unwrap()
}

pub(super) struct Server {
    root: PathBuf,
    parent: File,
    caller: Arc<CallerCopyContext>,
    checks: HashMap<String, Vec<Check>>,
    actions: HashMap<String, PrivilegedCopyRecoveryAction>,
    parents: BTreeSet<OsString>,
    user_data: [File; 2],
    before: UserData,
    progress: HashMap<&'static str, (usize, u64)>,
}
impl Server {
    pub(super) fn new(
        channel: &HelperChannel,
        root: &Path,
        preferences: &File,
        index: &File,
    ) -> Self {
        let caller = Arc::new(channel.caller_copy_context().unwrap());
        let user_data = [preferences.try_clone().unwrap(), index.try_clone().unwrap()];
        let before = observe(&caller, &user_data);
        Self {
            root: root.into(),
            parent: crate::windows_fs::open_directory(
                &root.join("system-only"),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            )
            .unwrap(),
            caller,
            checks: HashMap::new(),
            actions: HashMap::new(),
            parents: BTreeSet::new(),
            user_data,
            before,
            progress: HashMap::new(),
        }
    }
    pub(super) fn execute(
        &mut self,
        channel: &HelperChannel,
        request: &PrivilegedRequest,
    ) -> PrivilegedResponse {
        let reason = request.context.reason.as_deref().unwrap();
        let mut prepared_id = None;
        if let Some(case) = reason.strip_prefix(PREPARE) {
            assert!(matches!(
                request.command,
                PrivilegedCommand::DiscoverCopyHistory
            ));
            let (action, directory) = match case {
                "resume-file" => (PrivilegedCopyRecoveryAction::Resume, false),
                "resume-directory" => (PrivilegedCopyRecoveryAction::Resume, true),
                "release-file" => (PrivilegedCopyRecoveryAction::ReleaseName, false),
                "release-directory" => (PrivilegedCopyRecoveryAction::ReleaseName, true),
                "cleanup-release-file" => (PrivilegedCopyRecoveryAction::ReleaseName, false),
                "cleanup-release-directory" => (PrivilegedCopyRecoveryAction::ReleaseName, true),
                "end-file" => (PrivilegedCopyRecoveryAction::EndAttempt, false),
                "end-directory" => (PrivilegedCopyRecoveryAction::EndAttempt, true),
                _ => panic!("Unknown owned recovery seed"),
            };
            let path = self.root.join(if directory {
                "probe-data/wire-source-tree Ω"
            } else {
                "probe-data/wire-source-file Ω.txt"
            });
            let source = if directory {
                crate::windows_fs::open_directory(&path, FILE_GENERIC_READ).unwrap()
            } else {
                File::open(&path).unwrap()
            };
            let (id, check) = crate::WindowsCopyRegistry::prepare_caller_wire_recovery(
                Arc::clone(&self.caller),
                &self.parent,
                &source,
                case,
                action,
            );
            prepared_id = Some(id.clone());
            assert!(self.checks.insert(id.clone(), vec![check]).is_none());
            assert!(self.actions.insert(id.clone(), action).is_none());
            if case.starts_with("cleanup-release-") {
                self.checks.get_mut(&id).unwrap().push(
                    crate::WindowsCopyRegistry::hold_caller_wire_terminal_payload(
                        Arc::clone(&self.caller),
                        &id,
                    ),
                );
            }
            assert!(
                self.parents
                    .insert(OsString::from(format!("wire-recovery-{case}")))
            );
        } else if let Some(id) = reason.strip_prefix(INSPECT) {
            assert!(matches!(
                request.command,
                PrivilegedCommand::DiscoverCopyHistory
            ));
            for check in self.checks.get_mut(id).unwrap() {
                check();
            }
        } else if reason == RELEASE_FAULT {
            let PrivilegedCommand::UndoCopy { transactions } = &request.command else {
                panic!("Wrong release-fault command")
            };
            assert_eq!(transactions.len(), 1);
            let id = &transactions[0];
            assert_eq!(self.actions[id], PrivilegedCopyRecoveryAction::ReleaseName);
            let response = crate::WindowsCopyRegistry::with_caller_wire_release_fault(|| {
                channel.execute_copy_command(&request.command).unwrap()
            });
            assert_eq!(response.status, PrivilegedStatus::Failed);
            assert!(
                response
                    .message
                    .as_ref()
                    .unwrap()
                    .contains("injected Copy claim release")
            );
            self.checks.get_mut(id).unwrap().push(
                crate::WindowsCopyRegistry::hold_caller_wire_terminal_payload(
                    Arc::clone(&self.caller),
                    id,
                ),
            );
            return response;
        } else {
            assert_eq!(reason, "owned caller Copy command");
        }
        let kind = match &request.command {
            PrivilegedCommand::File(ottrin_core::FileCommand::Copy { .. }) => "copy",
            PrivilegedCommand::DiscoverCopyHistory => "history",
            PrivilegedCommand::UndoCopy { .. } => "undo",
            PrivilegedCommand::CleanupCopy { .. } => "cleanup",
            PrivilegedCommand::RecoverCopy { action, .. } => match action {
                PrivilegedCopyRecoveryAction::Resume => "resume",
                PrivilegedCopyRecoveryAction::EndAttempt => "end",
                PrivilegedCopyRecoveryAction::ReleaseName => "release",
            },
            _ => panic!("Unexpected caller Copy command"),
        };
        let mut callbacks = 0;
        let mut bytes = 0;
        let response = channel
            .execute_copy_command_with_progress(&request.command, &mut |value| {
                callbacks += 1;
                bytes = bytes.max(value);
                Ok(())
            })
            .unwrap();
        let maximum = self.progress.entry(kind).or_default();
        maximum.0 = maximum.0.max(callbacks);
        maximum.1 = maximum.1.max(bytes);
        if let Some(id) = prepared_id {
            let Some(PrivilegedPayload::CopyHistory(entries)) = &response.payload else {
                panic!("Seeded history failed: {response:?}")
            };
            let entry = entries
                .iter()
                .find(|entry| entry.reference.transactions == [id.clone()])
                .unwrap_or_else(|| panic!("Seed {id} omitted from {entries:?}"));
            assert_eq!(entry.reference.paths.len(), 1, "{entry:?}");
            println!(
                "owned wire recovery seed {} returned {:?}",
                id, entry.reference.paths
            );
        }
        response
    }
    pub(super) fn finish(&mut self) {
        for kind in [
            "copy", "history", "undo", "cleanup", "resume", "end", "release",
        ] {
            assert!(
                self.progress[kind].0 > 1,
                "{kind} did not forward engine callbacks: {:?}",
                self.progress
            );
        }
        assert!(self.progress["copy"].1 > 0);
        println!(
            "caller helper progress reaches engine callbacks for all seven command kinds; copied byte progress observed"
        );
        assert_eq!(self.parents.len(), 8);
        for checks in self.checks.values_mut() {
            for check in checks {
                check();
            }
        }
        assert_eq!(observe(&self.caller, &self.user_data), self.before);
        self.caller.validate().unwrap();
        println!(
            "caller helper recovery eight seeded file/directory cases: sources/preferences/index identities/security/bytes and edited unknown payloads preserved; terminal release leaves journal pairs byte-identical"
        );
    }
    pub(super) fn parents(&self) -> impl Iterator<Item = OsString> + '_ {
        self.parents.iter().cloned()
    }
}

fn exchange(
    pipe: &mut LocalPipeStream,
    command: PrivilegedCommand,
    reason: &str,
) -> PrivilegedResponse {
    write_json(
        pipe,
        &PrivilegedRequest {
            protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
            command,
            context: PrivilegedContext {
                reason: Some(reason.into()),
                cwd: None,
            },
        },
    )
    .unwrap();
    read_json(pipe).unwrap()
}
fn standard(pipe: &mut LocalPipeStream, command: PrivilegedCommand) -> PrivilegedResponse {
    exchange(pipe, command, "owned caller Copy command")
}
fn entries(response: PrivilegedResponse) -> Vec<PrivilegedCopyHistoryEntry> {
    assert_eq!(
        response.status,
        PrivilegedStatus::Success,
        "{:?}",
        response.message
    );
    let Some(PrivilegedPayload::CopyHistory(entries)) = response.payload else {
        panic!("Missing recovery history")
    };
    entries
}
fn row(pipe: &mut LocalPipeStream, id: &str) -> PrivilegedCopyHistoryEntry {
    entries(standard(pipe, PrivilegedCommand::DiscoverCopyHistory))
        .into_iter()
        .find(|entry| entry.reference.transactions == [id])
        .unwrap()
}
fn action(
    pipe: &mut LocalPipeStream,
    id: &str,
    action: PrivilegedCopyRecoveryAction,
    expected: PrivilegedStatus,
) {
    let response = standard(
        pipe,
        PrivilegedCommand::RecoverCopy {
            transaction: id.into(),
            action,
        },
    );
    assert_eq!(response.status, expected, "{:?}", response.message);
}
fn undo(pipe: &mut LocalPipeStream, id: &str) {
    let response = standard(
        pipe,
        PrivilegedCommand::UndoCopy {
            transactions: vec![id.into()],
        },
    );
    assert_eq!(
        response.status,
        PrivilegedStatus::Success,
        "{:?}",
        response.message
    );
}
fn cleanup(pipe: &mut LocalPipeStream, id: &str, status: PrivilegedStatus) {
    let response = standard(
        pipe,
        PrivilegedCommand::CleanupCopy {
            transactions: vec![id.into()],
        },
    );
    assert_eq!(response.status, status, "{:?}", response.message);
}
fn inspect(pipe: &mut LocalPipeStream, id: &str) {
    entries(exchange(
        pipe,
        PrivilegedCommand::DiscoverCopyHistory,
        &format!("{INSPECT}{id}"),
    ));
}

pub(super) fn client(pipe: &mut LocalPipeStream, root: &Path) {
    use PrivilegedCopyRecoveryAction::{EndAttempt, ReleaseName, Resume};
    for action_kind in [Resume, EndAttempt, ReleaseName] {
        action(
            pipe,
            "../not-a-transaction",
            action_kind,
            PrivilegedStatus::Failed,
        );
        action(pipe, &"0".repeat(32), action_kind, PrivilegedStatus::Failed);
    }
    for (mode, action_kind) in [
        ("resume", Resume),
        ("release", ReleaseName),
        ("cleanup-release", ReleaseName),
        ("end", EndAttempt),
    ] {
        for directory in [false, true] {
            let kind = if directory { "directory" } else { "file" };
            let case = format!("{mode}-{kind}");
            let parent = root
                .join("system-only")
                .join(format!("wire-recovery-{case}"));
            let prepared = entries(exchange(
                pipe,
                PrivilegedCommand::DiscoverCopyHistory,
                &format!("{PREPARE}{case}"),
            ));
            let expected_parent = OsString::from(format!("wire-recovery-{case}"));
            let initial = prepared
                .iter()
                .find(|entry| {
                    entry.reference.paths.iter().any(|(_, publication)| {
                        publication.parent().and_then(Path::file_name)
                            == Some(expected_parent.as_os_str())
                    })
                })
                .unwrap_or_else(|| {
                    panic!("Missing owned recovery parent {expected_parent:?} in {prepared:?}")
                })
                .clone();
            let id = initial.reference.transactions[0].clone();
            assert_eq!(initial.reference.transactions.len(), 1);
            assert_eq!(initial.reference.completed_sources, 0);
            if action_kind == ReleaseName {
                if mode == "cleanup-release" {
                    assert_eq!(
                        initial.recovery,
                        PrivilegedCopyHistoryState::RecoveryAvailable {
                            action: ReleaseName
                        }
                    );
                    action(pipe, &id, Resume, PrivilegedStatus::Failed);
                    assert_eq!(row(pipe, &id), initial);
                } else {
                    assert_eq!(initial.recovery, PrivilegedCopyHistoryState::UndoAvailable);
                    action(pipe, &id, Resume, PrivilegedStatus::Failed);
                    assert_eq!(row(pipe, &id), initial);
                    let response = exchange(
                        pipe,
                        PrivilegedCommand::UndoCopy {
                            transactions: vec![id.clone()],
                        },
                        RELEASE_FAULT,
                    );
                    assert_eq!(response.status, PrivilegedStatus::Failed);
                    assert_eq!(
                        row(pipe, &id).recovery,
                        PrivilegedCopyHistoryState::RecoveryAvailable {
                            action: ReleaseName
                        }
                    );
                }
                action(pipe, &id, ReleaseName, PrivilegedStatus::Success);
                action(pipe, &id, ReleaseName, PrivilegedStatus::Failed);
                inspect(pipe, &id); // Drop the exclusive data lease only after release.
                assert!(matches!(
                    row(pipe, &id).recovery,
                    PrivilegedCopyHistoryState::NeedsReview { .. }
                ));
                cleanup(pipe, &id, PrivilegedStatus::Failed); // Edited payload stays retained.
            } else {
                assert_eq!(
                    initial.recovery,
                    PrivilegedCopyHistoryState::RecoveryAvailable {
                        action: action_kind
                    }
                );
                action(
                    pipe,
                    &id,
                    if action_kind == Resume {
                        EndAttempt
                    } else {
                        Resume
                    },
                    PrivilegedStatus::Failed,
                );
                assert_eq!(row(pipe, &id), initial);
                action(pipe, &id, action_kind, PrivilegedStatus::Success);
                action(pipe, &id, action_kind, PrivilegedStatus::Failed);
                if action_kind == Resume {
                    assert_eq!(
                        row(pipe, &id).recovery,
                        PrivilegedCopyHistoryState::UndoAvailable
                    );
                    undo(pipe, &id);
                    cleanup(pipe, &id, PrivilegedStatus::Success);
                } else {
                    assert_eq!(
                        row(pipe, &id).recovery,
                        PrivilegedCopyHistoryState::AttemptEnded
                    );
                    inspect(pipe, &id); // Busy-file refusal cannot stand in for unknown-data refusal.
                    cleanup(pipe, &id, PrivilegedStatus::Failed);
                    let reuse = root.join("probe-data").join(format!("wire-reuse-{kind}"));
                    std::fs::create_dir(&reuse).unwrap();
                    let source = reuse.join("copy");
                    if directory {
                        std::fs::create_dir(&source).unwrap();
                        std::fs::write(source.join("member.txt"), b"wire directory payload")
                            .unwrap();
                    } else {
                        std::fs::write(&source, b"unchanged caller source").unwrap();
                    }
                    let response = standard(
                        pipe,
                        PrivilegedCommand::File(ottrin_core::FileCommand::Copy {
                            sources: vec![source],
                            destination: parent,
                            conflict: ottrin_core::ConflictAction::Rename,
                        }),
                    );
                    assert_eq!(
                        response.status,
                        PrivilegedStatus::Success,
                        "{:?}",
                        response.message
                    );
                    let Some(PrivilegedPayload::Copy(reference)) = response.payload else {
                        panic!("Missing reused-name receipt")
                    };
                    assert_eq!(reference.transactions.len(), 1);
                    assert_eq!(
                        reference.paths[0].1.file_name().unwrap(),
                        OsStr::new("copy")
                    );
                    undo(pipe, &reference.transactions[0]);
                    cleanup(pipe, &reference.transactions[0], PrivilegedStatus::Success);
                    assert_eq!(
                        row(pipe, &id).recovery,
                        PrivilegedCopyHistoryState::AttemptEnded
                    );
                }
            }
            println!(
                "caller helper recovery source_directory={directory} action={mode}: explicit history, wrong/repeated action refusal, authenticated recovery and retained-data checks passed"
            );
        }
    }
}
