//! Forward helper Copy uses the ordinary operation queue and terminal consumer.
use super::*;

impl OttrinApp {
    pub(crate) fn start_privileged_copy(
        &mut self,
        retry: PendingPrivilegedRetry,
        platform: Arc<dyn PlatformOps>,
    ) -> Result<(), String> {
        let id = self.queue_operation_record(retry.cmd.clone());
        self.operation_history.front_mut().unwrap().title =
            "Copy with administrator privileges".into();
        let cancelled = self.copy_work.start(id);
        let mut bytes = self.copy_work.progress(id, self.result_repaint.clone());
        let tx = self.op_tx.clone();
        let repaint = self.result_repaint.clone();
        let cwd = self.state.active_tab().current_dir.clone();
        let spawn = std::thread::Builder::new()
            .name("ottrin-administrator-copy".into())
            .spawn(move || {
                let result = run_copy(id, retry, cwd, platform.as_ref(), &cancelled, &mut bytes);
                repaint.send(&tx, result);
            });
        if let Err(error) = spawn {
            self.copy_work.finish(id);
            let error = format!("Could not start administrator Copy: {error}");
            self.finish_operation_record(id, OperationStatus::Failed, Some(error.clone()));
            return Err(error);
        }
        self.privileged_status = None;
        Ok(())
    }
}

fn run_copy(
    id: u64,
    retry: PendingPrivilegedRetry,
    cwd: PathBuf,
    platform: &dyn PlatformOps,
    cancelled: &AtomicBool,
    bytes: &mut copy_activity_work::CopyProgress,
) -> OpResult {
    let mut result = OpResult {
        op_id: id,
        tab_id: retry.tab_id,
        cmd: retry.cmd.clone(),
        delete_nav: Vec::new(),
        error: None,
        warning: None,
        stopped: false,
        retry: None,
        clipboard_generation: None,
        completed_sources: None,
        completed_source_paths: Vec::new(),
        move_dispositions: None,
        path_move_receipt: None,
        path_move_bindings: None,
    };
    let mut progress = |count| {
        bytes.report(count);
        if cancelled.load(Ordering::Acquire) {
            Err(PlatformError::Io(
                "Administrator Copy stopped; completed files and recovery records are preserved."
                    .into(),
            ))
        } else {
            Ok(())
        }
    };
    // Recheck the captured identity inside the worker before requesting elevation.
    let response = progress(0)
        .map_err(|error| error.to_string())
        .and_then(|()| validate_privileged_retry(&retry, &discover_storage_locations()))
        .and_then(|()| {
            platform
                .execute_privileged_with_progress(
                    &PrivilegedRequest {
                        protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
                        command: PrivilegedCommand::File(retry.cmd.clone()),
                        context: PrivilegedContext {
                            reason: Some("Retry denied Copy operation".into()),
                            cwd: Some(cwd),
                        },
                    },
                    &mut progress,
                )
                .map_err(|error| error.to_string())
        });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            if cancelled.load(Ordering::Acquire) {
                result.stopped = true;
                result.warning = Some(error);
            } else {
                result.error = Some(error);
            }
            return result;
        }
    };
    // A terminal success is authoritative even if Stop was requested just before
    // it arrived. Do not discard a committed receipt by checking cancellation now.
    let reference = match copy_reference(&retry.cmd, &response) {
        Ok(reference) => reference,
        Err(error) => {
            result.error = Some(error);
            return result;
        }
    };
    if let Some(reference) = reference {
        result.completed_sources = Some(reference.completed_sources);
        result.completed_source_paths = reference
            .paths
            .iter()
            .map(|(source, _)| source.clone())
            .collect();
        if !reference.transactions.is_empty() {
            let FileCommand::Copy { destination, .. } = &retry.cmd else {
                unreachable!()
            };
            result.path_move_bindings = Some(
                retry
                    .bindings
                    .into_iter()
                    .filter(|binding| destination.starts_with(&binding.root))
                    .collect(),
            );
            result.path_move_receipt = Some(OperationUndoReceipt::CopyBatch(
                CopyBatchUndoReceipt::Privileged {
                    reference: reference.clone(),
                    pending: true,
                    restored: false,
                },
            ));
        }
    }
    if response.status != PrivilegedStatus::Success {
        let error = response.message.unwrap_or_else(|| "Administrator Copy did not complete. Load administrator history to review retained data.".into());
        if result.path_move_receipt.is_some()
            || result.completed_sources.is_some_and(|count| count > 0)
            || cancelled.load(Ordering::Acquire)
        {
            // The normal consumer must attach the completed prefix before showing
            // its failure; its fatal-error branch intentionally has no receipt.
            result.stopped = true;
            result.warning = Some(error);
        } else {
            result.error = Some(error);
        }
    }
    result
}

#[cfg(test)]
mod tests;
