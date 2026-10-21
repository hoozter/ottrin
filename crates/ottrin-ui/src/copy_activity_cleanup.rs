//! Explicit Activity cleanup. Filesystem validation and mutation run off-thread;
//! UI ownership is released only after durable finalization succeeds.
use super::*;

#[derive(Clone)]
struct Selection {
    transactions: Vec<String>,
    #[cfg(target_os = "linux")]
    finalization: Option<String>,
}

#[cfg(target_os = "windows")]
type WindowsPlan = Arc<std::sync::Mutex<Option<ottrin_platform::RetainedCopyCleanupPlan>>>;

#[derive(Default)]
pub(super) struct CopyCleanupUi {
    #[cfg(target_os = "windows")]
    windows_plan: Option<WindowsPlan>,
    #[cfg(target_os = "windows")]
    cancellation: Option<Arc<AtomicBool>>,
    #[cfg(all(test, target_os = "windows"))]
    stop_when_missing: Option<PathBuf>,
    confirmation: Option<Vec<u64>>,
    receiver: Option<Receiver<Event>>,
    active: Vec<Selection>,
    errors: HashMap<String, String>,
    progress: String,
}

impl CopyCleanupUi {
    #[cfg(target_os = "windows")]
    pub(super) fn stop(&self) {
        if let Some(flag) = &self.cancellation {
            flag.store(true, Ordering::Release);
        }
    }
    #[cfg(target_os = "windows")]
    pub(super) fn stopping(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    pub(super) fn running(&self) -> bool {
        self.receiver.is_some()
    }
    pub(super) fn owns(&self, record: &OperationRecord) -> bool {
        if self.active.is_empty() {
            return false;
        }
        let ids = transactions(record);
        self.active
            .iter()
            .any(|selection| selection.transactions.iter().any(|id| ids.contains(id)))
    }
    pub(super) fn failed(&self, record: &OperationRecord) -> bool {
        !self.errors.is_empty()
            && transactions(record)
                .iter()
                .any(|id| self.errors.contains_key(id))
    }
    pub(super) fn action_label(&self, record: &OperationRecord) -> &'static str {
        if record
            .retained_copy
            .as_ref()
            .is_some_and(|owner| owner.finalization.is_some())
            || (cleanup_started(record) && !self.failed(record))
        {
            "Finish Copy cleanup"
        } else if self.failed(record) {
            "Retry Copy cleanup"
        } else if record.path_move_undo == Some(PathMoveUndoState::Available) {
            "Discard Copy undo and clear…"
        } else {
            "Clear retained Copy data…"
        }
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

fn cleanup_started(record: &OperationRecord) -> bool {
    record.retained_copy.as_ref().is_some_and(|owner| {
        if owner.finalization.is_some() {
            return true;
        }
        #[cfg(target_os = "windows")]
        {
            owner.cleanup_phase == Some(ottrin_platform::WindowsCopyPhase::CleaningUp)
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    })
}

pub(super) fn can_cleanup(record: &OperationRecord) -> bool {
    #[cfg(target_os = "windows")]
    let supported = record.retained_copy.as_ref().map_or(
        record.status == OperationStatus::Completed
            || matches!(record.path_move_undo, Some(PathMoveUndoState::Undone)),
        |owner| {
            matches!(
                owner.cleanup_phase,
                Some(
                    ottrin_platform::WindowsCopyPhase::Completed
                        | ottrin_platform::WindowsCopyPhase::Undone
                        | ottrin_platform::WindowsCopyPhase::CleaningUp
                )
            )
        },
    );
    #[cfg(not(target_os = "windows"))]
    let supported = cfg!(target_os = "linux");
    #[cfg(target_os = "windows")]
    let supported = supported && !copy_activity_recovery::name_release_pending(record);
    supported
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

#[cfg(target_os = "windows")]
fn windows_selection_matches(
    plan: &ottrin_platform::RetainedCopyCleanupPlan,
    selected: &std::collections::BTreeSet<String>,
) -> bool {
    let completed = plan.completed_transaction_ids();
    let all = plan.transaction_ids();
    selected.iter().all(|id| all.contains(&id.as_str()))
        && all
            .iter()
            .filter(|id| !completed.contains(id))
            .all(|id| selected.contains(*id))
}

#[cfg(target_os = "windows")]
fn run(
    selections: &[Selection],
    slot: &WindowsPlan,
    cancellation: &AtomicBool,
    #[cfg(test)] stop_when_missing: Option<&Path>,
    progress: &mut impl FnMut(String),
) -> Outcome {
    let mut outcome = Outcome::default();
    let selected = selections
        .iter()
        .flat_map(|selection| selection.transactions.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut guard = match slot.lock() {
        Ok(guard) => guard,
        Err(_) => {
            return Outcome {
                error: Some(
                    "Copy cleanup worker failed; its recovery proof is retained for review.".into(),
                ),
                ..Default::default()
            };
        }
    };
    let mut native_progress = |_| {
        #[cfg(test)]
        if stop_when_missing.is_some_and(|path| !path.exists()) {
            cancellation.store(true, Ordering::Release);
        }
        if cancellation.load(Ordering::Acquire) {
            Err(PlatformError::Io(
                "Copy cleanup stopped; retained data and recovery proof are preserved for retry."
                    .into(),
            ))
        } else {
            Ok(())
        }
    };
    let retain = guard
        .as_ref()
        .is_some_and(|plan| plan.requires_retained_retry());
    if retain && !windows_selection_matches(guard.as_ref().unwrap(), &selected) {
        outcome.error = Some("Finish the pending Copy cleanup selection first.".into());
        return outcome;
    }
    if !retain {
        progress("Checking retained Copy data…".into());
        match ottrin_platform::RetainedCopyCleanupPlan::observe(
            &selected.into_iter().collect::<Vec<_>>(),
            &mut native_progress,
        ) {
            Ok(plan) => *guard = Some(plan),
            Err(error) => {
                outcome.error = Some(error.to_string());
                return outcome;
            }
        }
    }
    let plan = guard.as_mut().unwrap();
    progress("Reclaiming retained Copy data and recovery bookkeeping…".into());
    outcome.error = plan
        .reclaim(&mut native_progress)
        .err()
        .map(|error| error.to_string());
    outcome.completed.extend(
        plan.completed_transaction_ids()
            .into_iter()
            .map(str::to_owned),
    );
    if plan.complete() || !plan.requires_retained_retry() {
        *guard = None;
    }
    outcome
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
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
            let previously_authorized = selected.iter().all(|id| {
                self.operation_history.iter().any(|record| {
                    record.id == *id
                        && (self.copy_cleanup.failed(record) || cleanup_started(record))
                })
            });
            if previously_authorized {
                self.start_copy_cleanup(&selected);
            } else {
                self.copy_cleanup.confirmation = Some(selected);
            }
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
        #[cfg(target_os = "windows")]
        let (windows_plan, selected) = {
            let mut selected = selected;
            let slot = self
                .copy_cleanup
                .windows_plan
                .get_or_insert_with(|| Arc::new(std::sync::Mutex::new(None)))
                .clone();
            let requested = selected
                .iter()
                .flat_map(|selection| selection.transactions.iter().cloned())
                .collect::<std::collections::BTreeSet<_>>();
            let allowed = match slot.lock() {
                Ok(guard) => match guard.as_ref() {
                    Some(plan) if plan.requires_retained_retry() => {
                        let all = plan
                            .transaction_ids()
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<std::collections::BTreeSet<_>>();
                        let completed = plan
                            .completed_transaction_ids()
                            .into_iter()
                            .map(str::to_owned)
                            .collect::<std::collections::BTreeSet<_>>();
                        let pending = all
                            .difference(&completed)
                            .cloned()
                            .collect::<std::collections::BTreeSet<_>>();
                        if requested.is_subset(&all) && !requested.is_disjoint(&pending) {
                            // Any Retry card resumes the entire already-authorized
                            // cohort, including cards whose control is now absent.
                            selected = self
                                .operation_history
                                .iter()
                                .filter(|record| {
                                    can_cleanup(record)
                                        && transactions(record)
                                            .iter()
                                            .any(|id| pending.contains(id))
                                })
                                .map(|record| Selection {
                                    transactions: transactions(record),
                                })
                                .collect();
                            let expanded = selected
                                .iter()
                                .flat_map(|selection| selection.transactions.iter().cloned())
                                .collect();
                            windows_selection_matches(plan, &expanded)
                        } else {
                            false
                        }
                    }
                    _ => true,
                },
                Err(_) => false,
            };
            if !allowed {
                self.set_status_message("Finish the pending Copy cleanup selection first; its recovery proof is still retained.", false, 8);
                return;
            }
            (slot, selected)
        };
        #[cfg(target_os = "windows")]
        let worker_cancel = {
            let cancellation = Arc::new(AtomicBool::new(false));
            self.copy_cleanup.cancellation = Some(cancellation.clone());
            cancellation
        };
        #[cfg(all(test, target_os = "windows"))]
        let stop_when_missing = self.copy_cleanup.stop_when_missing.take();
        let (tx, rx) = mpsc::channel();
        let active = selected.clone();
        let repaint = self.result_repaint.clone();
        let spawn = std::thread::Builder::new()
            .name("ottrin-copy-cleanup".into())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                let io_cancellation =
                    match ottrin_platform::NativeIoCancellation::start(worker_cancel.clone()) {
                        Ok(guard) => guard,
                        Err(error) => {
                            repaint.send(
                                &tx,
                                Event::Finished(Outcome {
                                    error: Some(error.to_string()),
                                    ..Default::default()
                                }),
                            );
                            return;
                        }
                    };
                let outcome = run(
                    &selected,
                    #[cfg(target_os = "windows")]
                    &windows_plan,
                    #[cfg(target_os = "windows")]
                    &worker_cancel,
                    #[cfg(all(test, target_os = "windows"))]
                    stop_when_missing.as_deref(),
                    &mut |message| {
                        // A closed UI does not revoke already-authorized durable cleanup.
                        repaint.send(&tx, Event::Progress(message));
                    },
                );
                #[cfg(target_os = "windows")]
                let outcome = {
                    let mut outcome = outcome;
                    if let Err(error) = io_cancellation.finish() {
                        outcome.error = Some(match outcome.error {
                            Some(operation) => format!("{operation}; {error}"),
                            None => error.to_string(),
                        });
                    }
                    outcome
                };
                repaint.send(&tx, Event::Finished(outcome));
            });
        if let Err(error) = spawn {
            #[cfg(target_os = "windows")]
            {
                self.copy_cleanup.cancellation = None;
            }
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
        #[cfg(target_os = "windows")]
        {
            self.copy_cleanup.cancellation = None;
        }
        let active = std::mem::take(&mut self.copy_cleanup.active);
        for id in &outcome.completed {
            self.copy_cleanup.errors.remove(id);
        }
        self.operation_history.retain(|record| {
            let ids = transactions(record);
            ids.is_empty() || !ids.iter().all(|id| outcome.completed.contains(id))
        });
        #[cfg(target_os = "windows")]
        for record in &mut self.operation_history {
            let remaining = transactions(record)
                .into_iter()
                .filter(|id| !outcome.completed.contains(id))
                .collect::<Vec<_>>();
            if let Some(owner) = &mut record.retained_copy {
                owner.transactions = remaining;
            } else if !remaining.is_empty()
                && transactions(record)
                    .iter()
                    .any(|id| outcome.completed.contains(id))
            {
                record.retained_copy = Some(RetainedCopyOwnership {
                    key: format!("cleanup-selection:{}", record.id),
                    transactions: remaining,
                    blocked_by: Vec::new(),
                    restored: true,
                    finalization: None,
                    recovery: None,
                    cleanup_phase: Some(ottrin_platform::WindowsCopyPhase::Completed),
                    #[cfg(target_os = "windows")]
                    recovery_location: None,
                });
            }
        }
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
                            #[cfg(target_os = "windows")]
                            recovery: None,
                            #[cfg(target_os = "windows")]
                            cleanup_phase: Some(ottrin_platform::WindowsCopyPhase::Completed),
                            #[cfg(target_os = "windows")]
                            recovery_location: None,
                        });
                    }
                    if let Some(owner) = &mut record.retained_copy {
                        owner.restored = true;
                        #[cfg(target_os = "windows")]
                        {
                            owner.recovery = None;
                            record.path_move_undo_token = None;
                        }
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
            self.set_status_message("Copy cleanup needs review in Activity.", false, 10);
        } else {
            self.set_status_message(
                "Copy history cleared and retained storage reclaimed.",
                true,
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
        let discards_undo = self.operation_history.iter().any(|record| {
            ids.contains(&record.id) && record.path_move_undo == Some(PathMoveUndoState::Available)
        });
        let (title, action, consequence) = if discards_undo {
            (
                "Discard Copy undo and clear history?",
                "Discard undo and clear",
                "Undo will no longer be available.",
            )
        } else {
            (
                "Clear retained Copy data and history?",
                "Clear retained data",
                "Retained versions cannot be restored after clearing.",
            )
        };
        let c = self.colors;
        let mut choice = None;
        let response = egui::Modal::new(egui::Id::new("copy_cleanup_confirmation"))
            .frame(Frame::new().fill(c.panel).stroke(Stroke::new(c.window_border_width, c.window_border))
                .corner_radius(c.app_radius).inner_margin(18))
            .show(ctx, |ui| {
                ui.set_width(400.0 * c.font_scale);
                ui.label(RichText::new(title).color(c.heading).strong().size(15.0 * c.font_scale));
                ui.add_space(8.0);
                ui.label(format!("This clears {} finished Copy operation{} and removes retained versions and recovery records. Current files and original sources stay in place. {consequence}", ids.len(), plural_s(ids.len())));
                ui.label("Changed or uncertain retained data stays available for review and retry.");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Cancel").themed(&c)).clicked() { choice = Some(false); }
                    if ui.add(egui::Button::new(action).themed(&c)).clicked() { choice = Some(true); }
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
        if case == "finalization" {
            app.request_copy_cleanup(operations.clone());
            assert!(app.copy_cleanup.confirmation.is_none());
            assert!(
                app.copy_cleanup.running(),
                "Durable finalization resumes without asking for permission again"
            );
        } else {
            app.request_copy_cleanup(operations.clone());
            assert!(app.copy_cleanup.confirmation.is_some());
            assert!(!app.copy_cleanup.running());
        }
        if case != "finalization" {
            assert!(
                receipt.before_path().parent().unwrap().exists(),
                "Request must not mutate before confirmation"
            );
        }
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
            assert!(
                !app.status_message.as_ref().unwrap().ok,
                "Cleanup failure must use error status styling"
            );
            assert_eq!(
                app.copy_cleanup.action_label(&app.operation_history[0]),
                "Retry Copy cleanup"
            );
            std::fs::write(receipt.before_path(), "before").unwrap();
            app.request_copy_cleanup(operations.clone());
            assert!(app.copy_cleanup.confirmation.is_none());
            assert!(
                app.copy_cleanup.running(),
                "Explicit retry retains prior confirmation"
            );
            wait(&mut app);
        }
        assert!(app.operation_history.is_empty());
        assert!(
            app.status_message.as_ref().unwrap().ok,
            "Successful cleanup must use success status styling"
        );
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

#[cfg(all(test, target_os = "windows"))]
pub(super) fn windows_test_case(root: &Path) {
    use std::os::windows::fs::OpenOptionsExt;
    let case = std::env::var("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE").unwrap();
    let destination = std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_DESTINATION")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("fixture/destination"));
    let registry = root.join("profile/Ottrin/State/copies");
    let mut controls = std::fs::read_dir(&registry)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "copy-journal")
        })
        .collect::<Vec<_>>();
    controls.sort();
    let control = if case == "cleanup-cohort-partial" {
        controls.pop().unwrap()
    } else {
        controls.remove(0)
    };
    let retained = std::fs::read_dir(&destination)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap() != "copy" && path.join("nested/file").is_file())
        .unwrap();
    let public = std::fs::read(destination.join("copy/nested/file")).unwrap();
    let second_public = (case.starts_with("cleanup-cohort")).then(|| {
        let path = root.join("fixture-second/destination/copy/nested/file");
        let bytes = std::fs::read(&path).unwrap();
        (path, bytes)
    });
    let before = std::fs::read(&control).unwrap();
    let mut app = OttrinApp::default();
    app.operation_history.clear();
    app.retained_copy_activity_loader =
        RetainedActivityLoader::spawn_with(load_retained_copy_activity);
    app.refresh_retained_copy_activity();
    fn wait(app: &mut OttrinApp) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            app.poll_copy_cleanup();
            app.poll_retained_copy_activity();
            if !app.copy_cleanup.running() && !app.retained_copy_activity_loader.pending {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Windows cleanup worker did not finish"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    wait(&mut app);
    let count = if case.starts_with("cleanup-cohort") {
        2
    } else {
        1
    };
    assert_eq!(app.operation_history.len(), count);
    let ctx = Context::default();
    let click = |app: &mut OttrinApp, label| super::tests::click_activity_action(app, &ctx, label);
    let initial_action = if count == 2 {
        "Clear finished"
    } else {
        "Clear retained Copy data…"
    };
    click(&mut app, initial_action);
    assert!(app.copy_cleanup.confirmation.is_some());
    assert!(!app.copy_cleanup.running());
    click(&mut app, "Cancel");
    assert!(app.copy_cleanup.confirmation.is_none());
    assert_eq!(std::fs::read(&control).unwrap(), before);
    assert!(retained.join("nested/file").is_file());
    click(&mut app, initial_action);
    let busy = if case == "cleanup-retry" {
        Some(
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(retained.join("nested/file"))
                .unwrap(),
        )
    } else {
        None
    };
    if case == "cleanup-changed" {
        std::fs::write(retained.join("nested/file"), b"foreign retained data").unwrap();
    }
    if matches!(
        case.as_str(),
        "cleanup-stopped-metadata" | "cleanup-cohort" | "cleanup-cohort-partial"
    ) {
        app.copy_cleanup.stop_when_missing = Some(control.clone());
    }
    let mut blocked = case.strip_prefix("cleanup-blocked-").map(|target| {
        let local = std::fs::read_dir(&destination)
            .unwrap()
            .map(|entry| entry.unwrap().path().join("journal"))
            .find(|path| path.is_file())
            .unwrap();
        let local_before = std::fs::read(&local).unwrap();
        let payload = retained.join("nested/file");
        let payload_before = std::fs::read(&payload).unwrap();
        let lock = crate::test_oplock::Oplock::request(match target {
            "central" => &control,
            "local" => &local,
            "payload" => &payload,
            _ => panic!("Unknown owned oplock target"),
        })
        .unwrap();
        (lock, local, local_before, payload, payload_before)
    });
    click(&mut app, "Clear retained data");
    assert!(app.copy_cleanup.running());
    if let Some((lock, local, local_before, payload, payload_before)) = &mut blocked {
        let broken = lock.wait_break();
        std::thread::sleep(Duration::from_millis(100));
        app.poll_copy_cleanup();
        let pending = app.copy_cleanup.running();
        let owns_data = app.copy_cleanup.owns(&app.operation_history[0]);
        if pending {
            click(&mut app, "Stop cleanup");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.copy_cleanup.running() && Instant::now() < deadline {
            app.poll_copy_cleanup();
            std::thread::sleep(Duration::from_millis(5));
        }
        let stopped = !app.copy_cleanup.running();
        let close = lock.close();
        wait(&mut app);
        close.unwrap();
        assert_eq!(broken, windows_sys::Win32::Foundation::WAIT_OBJECT_0);
        assert!(
            pending && owns_data,
            "Blocked cleanup must retain ownership"
        );
        assert!(stopped, "Rendered Stop did not interrupt blocked cleanup");
        assert_eq!(std::fs::read(&control).unwrap(), before);
        assert_eq!(std::fs::read(local).unwrap(), *local_before);
        assert_eq!(std::fs::read(payload).unwrap(), *payload_before);
    }
    wait(&mut app);
    assert_eq!(
        std::fs::read(destination.join("copy/nested/file")).unwrap(),
        public
    );
    if case == "cleanup-changed" {
        assert_eq!(
            std::fs::read(retained.join("nested/file")).unwrap(),
            b"foreign retained data"
        );
        assert_eq!(std::fs::read(&control).unwrap(), before);
        assert_eq!(app.operation_history.len(), count);
        assert!(app.copy_cleanup.failed(&app.operation_history[0]));
        ctx.style_mut(|style| style.animation_time = 0.0);
        let detail = app.operation_history[0].detail.clone();
        let message = app.operation_history[0].message.clone().unwrap();
        let labels = super::tests::activity_text(&mut app, &ctx);
        assert!(labels.iter().any(|label| label
            == "Cleanup could not finish. Remaining data is kept for review and retry."));
        assert!(!labels.contains(&detail));
        assert!(!labels.contains(&message));
        click(&mut app, "Recovery details");
        let labels = super::tests::activity_text(&mut app, &ctx);
        assert!(
            labels.contains(&detail),
            "Recovery location and identity must remain available"
        );
        assert!(
            labels.contains(&message),
            "The complete refusal diagnostic must remain available"
        );
        click(&mut app, "Recovery details");
        let labels = super::tests::activity_text(&mut app, &ctx);
        assert!(!labels.contains(&message));
    } else {
        if busy.is_some()
            || blocked.is_some()
            || matches!(
                case.as_str(),
                "cleanup-stopped-metadata" | "cleanup-cohort" | "cleanup-cohort-partial"
            )
        {
            assert_eq!(
                app.operation_history.len(),
                if case == "cleanup-cohort-partial" {
                    1
                } else {
                    count
                }
            );
            assert!(app.copy_cleanup.failed(&app.operation_history[0]));
            if matches!(
                case.as_str(),
                "cleanup-stopped-metadata" | "cleanup-cohort" | "cleanup-cohort-partial"
            ) {
                assert!(!control.exists());
                assert!(
                    app.copy_cleanup
                        .windows_plan
                        .as_ref()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .requires_retained_retry()
                );
            }
            drop(busy);
            click(&mut app, "Retry Copy cleanup");
            assert!(app.copy_cleanup.confirmation.is_none());
            assert!(app.copy_cleanup.running());
            wait(&mut app);
        }
        assert!(app.operation_history.is_empty());
        assert!(!retained.exists());
        assert!(std::fs::read_dir(&registry).unwrap().next().is_none());
        assert!(
            app.copy_cleanup
                .windows_plan
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        std::fs::read(destination.join("copy/nested/file")).unwrap(),
        public
    );
    if let Some((path, bytes)) = second_public {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
