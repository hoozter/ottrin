//! Explicit Activity cleanup. Filesystem validation and mutation run off-thread;
//! UI ownership is released only after durable finalization succeeds.
use super::*;

#[derive(Clone)]
struct Selection {
    transactions: Vec<String>,
    #[cfg(target_os = "linux")]
    finalization: Option<String>,
}

#[derive(Default)]
pub(super) struct CopyCleanupUi {
    confirmation: Option<Vec<u64>>,
    receiver: Option<Receiver<Event>>,
    active: Vec<Selection>,
    errors: HashMap<String, String>,
    progress: String,
}

impl CopyCleanupUi {
    pub(super) fn running(&self) -> bool {
        self.receiver.is_some()
    }
    pub(super) fn owns(&self, record: &OperationRecord) -> bool {
        let ids = transactions(record);
        self.active
            .iter()
            .any(|selection| selection.transactions.iter().any(|id| ids.contains(id)))
    }
    pub(super) fn progress(&self) -> &str {
        &self.progress
    }
}

enum Event {
    Progress(String),
    Finished(Outcome),
}

#[derive(Default)]
struct Outcome {
    completed: HashSet<String>,
    finalization: Option<(String, Vec<String>)>,
    error: Option<String>,
}

fn transactions(record: &OperationRecord) -> Vec<String> {
    record
        .retained_copy
        .as_ref()
        .map(|owner| owner.transactions.clone())
        .unwrap_or_else(|| copy_receipt_transaction_ids(record))
}

pub(super) fn can_cleanup(record: &OperationRecord) -> bool {
    cfg!(target_os = "linux")
        && record.undo_kind == Some(PathMoveUndoKind::Copy)
        && record.status != OperationStatus::Queued
        && !matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
        && !transactions(record).is_empty()
}

#[cfg(target_os = "linux")]
fn run(selections: &[Selection], progress: &mut impl FnMut(String)) -> Outcome {
    let mut outcome = Outcome::default();
    let result = (|| -> Result<(), PlatformError> {
        let finalizations = selections
            .iter()
            .filter_map(|selection| selection.finalization.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for id in finalizations {
            progress("Finishing interrupted Copy cleanup…".into());
            let plan = ottrin_platform::resume_retained_copy_finalization(&id)?;
            let ids = plan
                .transaction_ids()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            outcome.finalization = Some((id, ids.clone()));
            plan.finish()?;
            outcome.completed.extend(ids);
            outcome.finalization = None;
        }
        let selected = selections
            .iter()
            .flat_map(|selection| selection.transactions.iter().cloned())
            .filter(|id| !outcome.completed.contains(id))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Ok(());
        }
        progress("Checking retained Copy data…".into());
        let observed = ottrin_platform::observe_retained_copy_cleanup(&selected)?;
        let all_pending = observed.iter().all(|entry| {
            matches!(
                entry.phase,
                Some(
                    ottrin_platform::CopyPhase::CleanupPending
                        | ottrin_platform::CopyPhase::CleanupComplete
                )
            )
        });
        let mut plan = if all_pending {
            ottrin_platform::resume_retained_copy_cleanup(&selected)?
        } else {
            ottrin_platform::plan_retained_copy_cleanup(&observed)?
        };
        let count = plan.remaining_payloads();
        let mut done = 0;
        progress(format!("Reclaiming Copy data · 0 of {count} items"));
        while plan.reclaim_payload_next()? {
            done += 1;
            progress(format!("Reclaiming Copy data · {done} of {count} items"));
        }
        progress("Removing Copy recovery bookkeeping…".into());
        let finalization = plan.begin_finalization()?;
        let ids = finalization
            .transaction_ids()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        outcome.finalization = Some((finalization.id().into(), ids.clone()));
        finalization.finish()?;
        outcome.completed.extend(ids);
        outcome.finalization = None;
        Ok(())
    })();
    outcome.error = result.err().map(|error| error.to_string());
    outcome
}

#[cfg(not(target_os = "linux"))]
fn run(_selections: &[Selection], _progress: &mut impl FnMut(String)) -> Outcome {
    Outcome {
        error: Some("Retained Copy cleanup is not available on this platform".into()),
        ..Default::default()
    }
}

impl OttrinApp {
    pub(super) fn request_copy_cleanup(&mut self, ids: Vec<u64>) {
        if self.copy_cleanup.running()
            || self.operation_history.iter().any(|record| {
                record.status == OperationStatus::Queued
                    || matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
            })
        {
            self.set_status_message(
                "Wait for current file operations before clearing Copy history.",
                false,
                6,
            );
            return;
        }
        let selected = ids
            .into_iter()
            .filter(|id| {
                self.operation_history
                    .iter()
                    .any(|record| record.id == *id && can_cleanup(record))
            })
            .collect::<Vec<_>>();
        if !selected.is_empty() {
            self.copy_cleanup.confirmation = Some(selected);
        }
    }

    fn start_copy_cleanup(&mut self, ids: &[u64]) {
        if self.copy_cleanup.running() {
            return;
        }
        if self.operation_history.iter().any(|record| {
            record.status == OperationStatus::Queued
                || matches!(record.path_move_undo, Some(PathMoveUndoState::UndoQueued))
        }) {
            return;
        }
        let selected = self
            .operation_history
            .iter()
            .filter(|record| ids.contains(&record.id) && can_cleanup(record))
            .map(|record| Selection {
                transactions: transactions(record),
                #[cfg(target_os = "linux")]
                finalization: record
                    .retained_copy
                    .as_ref()
                    .and_then(|owner| owner.finalization.clone()),
            })
            .collect::<Vec<_>>();
        if selected.is_empty() {
            self.set_status_message(
                "Copy history changed; select its current entry before clearing.",
                false,
                6,
            );
            return;
        }
        let (tx, rx) = mpsc::channel();
        let active = selected.clone();
        let repaint = self.result_repaint.clone();
        let spawn = std::thread::Builder::new()
            .name("ottrin-copy-cleanup".into())
            .spawn(move || {
                let outcome = run(&selected, &mut |message| {
                    // A closed UI does not revoke already-authorized durable cleanup.
                    repaint.send(&tx, Event::Progress(message));
                });
                repaint.send(&tx, Event::Finished(outcome));
            });
        if let Err(error) = spawn {
            self.set_status_message(format!("Could not start Copy cleanup: {error}"), false, 8);
            return;
        }
        self.copy_cleanup.active = active;
        self.copy_cleanup.progress = "Checking retained Copy data…".into();
        self.copy_cleanup.receiver = Some(rx);
    }

    pub(super) fn poll_copy_cleanup(&mut self) {
        let mut finished = None;
        if let Some(receiver) = &self.copy_cleanup.receiver {
            loop {
                match receiver.try_recv() {
                    Ok(Event::Progress(message)) => self.copy_cleanup.progress = message,
                    Ok(Event::Finished(outcome)) => {
                        finished = Some(outcome);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = Some(Outcome { error: Some("Copy cleanup worker stopped; retained data and recovery ownership stay available for review.".into()), ..Default::default() });
                        break;
                    }
                }
            }
        }
        let Some(outcome) = finished else {
            return;
        };
        self.copy_cleanup.receiver = None;
        let active = std::mem::take(&mut self.copy_cleanup.active);
        for id in &outcome.completed {
            self.copy_cleanup.errors.remove(id);
        }
        self.operation_history.retain(|record| {
            let ids = transactions(record);
            ids.is_empty() || !ids.iter().all(|id| outcome.completed.contains(id))
        });
        if let Some(error) = &outcome.error {
            for id in active.iter().flat_map(|selection| &selection.transactions) {
                if !outcome.completed.contains(id) {
                    self.copy_cleanup.errors.insert(id.clone(), error.clone());
                }
            }
            for record in &mut self.operation_history {
                let ids = transactions(record);
                if ids
                    .iter()
                    .any(|id| self.copy_cleanup.errors.contains_key(id))
                {
                    record.path_move_undo = Some(PathMoveUndoState::Unavailable { reason: "Copy cleanup needs review or retry; checking the retained evidence again.".into() });
                    record.status = OperationStatus::NeedsReview;
                    if record.retained_copy.is_none() {
                        record.retained_copy = Some(RetainedCopyOwnership {
                            key: format!("cleanup-selection:{}", record.id),
                            transactions: ids,
                            blocked_by: Vec::new(),
                            restored: true,
                            finalization: None,
                        });
                    }
                    if let Some(owner) = &mut record.retained_copy {
                        owner.restored = true;
                        if let Some((id, members)) = &outcome.finalization
                            && owner
                                .transactions
                                .iter()
                                .any(|member| members.contains(member))
                        {
                            owner.finalization = Some(id.clone());
                        }
                    }
                }
            }
            self.set_status_message("Copy cleanup could not finish. Retained entries stay in Activity with the error and a retry action.", false, 10);
        } else {
            self.set_status_message(
                "Copy history cleared and retained storage reclaimed.",
                false,
                6,
            );
        }
        self.pending_retained_copy_activity = None;
        self.refresh_retained_copy_activity();
        self.restore_copy_cleanup_errors();
    }

    pub(super) fn restore_copy_cleanup_errors(&mut self) {
        for record in &mut self.operation_history {
            if let Some(error) = transactions(record)
                .iter()
                .find_map(|id| self.copy_cleanup.errors.get(id))
            {
                record.message = Some(format!("Cleanup could not finish: {error}"));
            }
        }
    }

    pub(super) fn render_copy_cleanup_confirmation(&mut self, ctx: &Context) {
        let Some(ids) = self.copy_cleanup.confirmation.clone() else {
            return;
        };
        let c = self.colors;
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("copy_cleanup_confirmation"))
            .frame(Frame::new().fill(c.panel).stroke(Stroke::new(c.window_border_width, c.window_border))
                .corner_radius(c.app_radius).inner_margin(18))
            .show(ctx, |ui| {
                ui.set_width(400.0 * c.font_scale);
                ui.label(RichText::new("Discard Copy undo and clear history?").color(c.heading).strong().size(15.0 * c.font_scale));
                ui.add_space(8.0);
                ui.label(format!("This clears {} finished Copy operation(s) and removes their retained undo versions. Current files and original sources stay in place. Undo will no longer be available.", ids.len()));
                ui.label("Changed or uncertain retained data stays available for review and retry.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Cancel").themed(&c)).clicked() { choice = Some(false); }
                    if ui.add(egui::Button::new("Discard undo and clear").themed(&c)).clicked() { choice = Some(true); }
                });
            });
        if choice == Some(false) || response.should_close() {
            self.copy_cleanup.confirmation = None;
        }
        if choice == Some(true) {
            self.copy_cleanup.confirmation = None;
            self.start_copy_cleanup(&ids);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn copy(root: &Path, name: &str, version: &str) -> PathMoveUndoReceipt {
        let source = root.join(version);
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join(name), version).unwrap();
        let destination = root.join("destination");
        std::fs::create_dir_all(&destination).unwrap();
        DefaultPlatform
            .execute_copy_with_undo_receipt(
                &source.join(name),
                &destination,
                ConflictAction::Overwrite,
            )
            .unwrap()
            .unwrap()
    }

    fn id(receipt: &PathMoveUndoReceipt) -> String {
        receipt
            .before_path()
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_prefix(".ottrin-txn-")
            .unwrap()
            .to_owned()
    }

    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.copy_cleanup.running() {
            app.poll_copy_cleanup();
            assert!(Instant::now() < deadline, "Copy cleanup worker timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn cleanup_worker_fixture_child() {
        let Some(root) = std::env::var_os("OTTRIN_COPY_ACTIVITY_FIXTURE") else {
            return;
        };
        let root = PathBuf::from(root);
        assert_eq!(
            std::env::var_os("XDG_STATE_HOME"),
            Some(root.join("state").into_os_string())
        );
        let case = std::env::var("OTTRIN_COPY_ACTIVITY_CASE").unwrap();
        std::fs::create_dir_all(root.join("destination")).unwrap();
        std::fs::write(root.join("destination/item"), "before").unwrap();
        let receipt = copy(&root, "item", "first");
        if case == "prepare-gui" {
            println!("{}", id(&receipt));
            return;
        }
        let mut selected = vec![id(&receipt)];
        let mut second = None;
        if case == "chain" {
            let next = copy(&root, "item", "second");
            selected.push(id(&next));
            second = Some(next);
        }
        if case == "finalization" {
            let observed = ottrin_platform::observe_retained_copy_cleanup(&selected).unwrap();
            let mut plan = ottrin_platform::plan_retained_copy_cleanup(&observed).unwrap();
            while plan.reclaim_payload_next().unwrap() {}
            let _finalization = plan.begin_finalization().unwrap();
            // Model interruption after ledger unlink/sync while its canonical
            // journal and durable manifest still own the remaining stage.
            let stage = receipt.before_path().parent().unwrap();
            std::fs::remove_file(stage.join("copy-cleanup")).unwrap();
            std::fs::File::open(stage).unwrap().sync_all().unwrap();
        }
        let (groups, error) = load_retained_copy_activity();
        assert!(error.is_none(), "{error:?}");
        let mut app = OttrinApp::default();
        app.merge_retained_copy_activity(groups);
        assert_eq!(
            app.operation_history.len(),
            if case == "chain" { 2 } else { 1 }
        );
        if case == "finalization" {
            assert!(
                app.operation_history[0]
                    .retained_copy
                    .as_ref()
                    .unwrap()
                    .finalization
                    .is_some()
            );
            assert!(app.operation_history[0].path_move_undo_token.is_none());
        }
        if case == "changed" {
            std::fs::write(receipt.before_path(), "keep modified backup").unwrap();
        }
        let operations = app
            .operation_history
            .iter()
            .map(|record| record.id)
            .collect::<Vec<_>>();
        app.request_copy_cleanup(operations.clone());
        assert!(app.copy_cleanup.confirmation.is_some());
        assert!(!app.copy_cleanup.running());
        assert!(
            receipt.before_path().parent().unwrap().exists(),
            "Request must not mutate before confirmation"
        );
        app.copy_cleanup.confirmation = None;
        app.start_copy_cleanup(&operations);
        assert!(app.copy_cleanup.running());
        app.run_path_move_undo(operations[0]);
        assert!(!matches!(
            app.operation_history[0].path_move_undo,
            Some(PathMoveUndoState::UndoQueued)
        ));
        wait(&mut app);
        if case == "changed" {
            assert_eq!(app.operation_history.len(), 1);
            assert!(
                app.operation_history[0]
                    .message
                    .as_ref()
                    .unwrap()
                    .contains("Cleanup could not finish")
            );
            assert_eq!(
                std::fs::read(receipt.before_path()).unwrap(),
                b"keep modified backup"
            );
            assert!(can_cleanup(&app.operation_history[0]));
            std::fs::write(receipt.before_path(), "before").unwrap();
            app.start_copy_cleanup(&operations);
            wait(&mut app);
        }
        assert!(app.operation_history.is_empty());
        assert!(app.copy_cleanup.errors.is_empty());
        assert!(
            ottrin_platform::discover_retained_copy_transactions()
                .unwrap()
                .is_empty()
        );
        assert!(
            ottrin_platform::discover_retained_copy_finalizations()
                .unwrap()
                .is_empty()
        );
        assert!(!receipt.before_path().parent().unwrap().exists());
        if let Some(receipt) = second {
            assert!(!receipt.before_path().parent().unwrap().exists());
        }
        assert_eq!(
            std::fs::read(root.join("destination/item")).unwrap(),
            if case == "chain" {
                b"second".as_slice()
            } else {
                b"first".as_slice()
            }
        );
        assert_eq!(std::fs::read(root.join("first/item")).unwrap(), b"first");
    }

    #[test]
    fn background_copy_cleanup_success_retry_chain_and_restart_use_private_processes() {
        for case in ["success", "changed", "chain", "finalization"] {
            let root = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "copy_activity_cleanup::tests::cleanup_worker_fixture_child",
                    "--nocapture",
                ])
                .env("OTTRIN_COPY_ACTIVITY_FIXTURE", root.path())
                .env("OTTRIN_COPY_ACTIVITY_CASE", case)
                .env("XDG_STATE_HOME", root.path().join("state"))
                .env("XDG_CONFIG_HOME", root.path().join("config"))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
