//! Stop registration shared by forward Copy and Copy Undo workers.
use super::*;

#[derive(Default)]
pub(super) struct CopyWorkUi {
    cancellations: HashMap<u64, Arc<AtomicBool>>,
}
impl CopyWorkUi {
    pub(super) fn start(&mut self, id: u64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        assert!(!self.cancellations.contains_key(&id));
        self.cancellations.insert(id, flag.clone());
        flag
    }
    pub(super) fn running(&self, id: u64) -> bool {
        self.cancellations.contains_key(&id)
    }
    pub(super) fn stopping(&self, id: u64) -> bool {
        self.cancellations
            .get(&id)
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
    pub(super) fn stop(&self, id: u64) {
        if let Some(flag) = self.cancellations.get(&id) {
            flag.store(true, Ordering::Release);
        }
    }
    pub(super) fn finish(&mut self, id: u64) {
        self.cancellations.remove(&id);
    }
}
impl Drop for CopyWorkUi {
    fn drop(&mut self) {
        for flag in self.cancellations.values() {
            flag.store(true, Ordering::Release);
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
