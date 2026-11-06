//! Stop registration shared by forward Copy and Copy Undo workers.
use super::*;

#[derive(Default)]
pub(super) struct CopyWorkUi {
    cancellations: HashMap<u64, Work>,
}
struct Work {
    cancelled: Arc<AtomicBool>,
    bytes: Arc<AtomicU64>,
}
pub(super) struct CopyProgress {
    bytes: Arc<AtomicU64>,
    repaint: ResultRepaint,
    last_repaint: Instant,
}
impl CopyProgress {
    pub(super) fn report(&mut self, count: u64) {
        self.bytes.store(count, Ordering::Relaxed);
        // Large Copy batches can report every buffer. Update the counter each
        // time while keeping GUI wakes bounded; terminal results wake immediately.
        if self.last_repaint.elapsed() >= Duration::from_millis(50) {
            self.repaint.wake();
            self.last_repaint = Instant::now();
        }
    }
}
impl CopyWorkUi {
    pub(super) fn start(&mut self, id: u64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        assert!(!self.cancellations.contains_key(&id));
        self.cancellations.insert(
            id,
            Work {
                cancelled: flag.clone(),
                bytes: Arc::new(AtomicU64::new(0)),
            },
        );
        flag
    }
    pub(super) fn progress(&self, id: u64, repaint: ResultRepaint) -> CopyProgress {
        CopyProgress {
            bytes: self
                .cancellations
                .get(&id)
                .expect("registered Copy work")
                .bytes
                .clone(),
            repaint,
            last_repaint: Instant::now(),
        }
    }
    pub(super) fn transferred(&self, id: u64) -> Option<u64> {
        self.cancellations
            .get(&id)
            .map(|work| work.bytes.load(Ordering::Relaxed))
    }
    pub(super) fn running(&self, id: u64) -> bool {
        self.cancellations.contains_key(&id)
    }
    pub(super) fn stopping(&self, id: u64) -> bool {
        self.cancellations
            .get(&id)
            .is_some_and(|flag| flag.cancelled.load(Ordering::Acquire))
    }
    pub(super) fn stop(&self, id: u64) {
        if let Some(flag) = self.cancellations.get(&id) {
            flag.cancelled.store(true, Ordering::Release);
        }
    }
    pub(super) fn finish(&mut self, id: u64) {
        self.cancellations.remove(&id);
    }
}
impl Drop for CopyWorkUi {
    fn drop(&mut self) {
        for flag in self.cancellations.values() {
            flag.cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(target_os = "windows")]
pub(super) fn refresh_undo_receipt(
    receipt: &OperationUndoReceipt,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<OperationUndoReceipt, PlatformError> {
    progress(0)?;
    Ok(match receipt {
        OperationUndoReceipt::PathMove(receipt) => {
            OperationUndoReceipt::PathMove(receipt.refresh_windows_copy_undo(progress)?)
        }
        OperationUndoReceipt::CopyBatch(batch) => OperationUndoReceipt::CopyBatch(match batch {
            // These IDs are lookup references. Only the authenticated helper
            // re-admits their filesystem authority when Undo actually runs.
            CopyBatchUndoReceipt::Privileged { .. } => batch.clone(),
            CopyBatchUndoReceipt::Retained(plan) => {
                CopyBatchUndoReceipt::Retained(Box::new(plan.refreshed_with_progress(progress)?))
            }
            CopyBatchUndoReceipt::Session {
                receipts,
                remaining,
            } => {
                let mut receipts = receipts.clone();
                for receipt in receipts.iter_mut().take(*remaining) {
                    *receipt = receipt.refresh_windows_copy_undo(&mut *progress)?;
                }
                CopyBatchUndoReceipt::Session {
                    receipts,
                    remaining: *remaining,
                }
            }
        }),
        _ => return Err(PlatformError::Io("Copy Undo has no native receipt".into())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_work_stop_is_per_operation_and_close_stops_remaining_workers() {
        let mut ui = CopyWorkUi::default();
        let first = ui.start(1);
        let second = ui.start(2);
        let mut progress = ui.progress(1, ResultRepaint::default());
        progress.report(8192);
        assert_eq!(ui.transferred(1), Some(8192));
        progress.report(0); // Engine copy/verification steps each report their own bytes.
        assert_eq!(ui.transferred(1), Some(0));
        assert_eq!(ui.transferred(2), Some(0));
        ui.stop(1);
        assert!(first.load(Ordering::Acquire));
        assert!(!second.load(Ordering::Acquire));
        ui.finish(1);
        assert!(!ui.running(1));
        assert!(ui.running(2));
        drop(ui);
        assert!(second.load(Ordering::Acquire));
    }
}
