//! Stop registration for the existing per-operation Windows Copy Undo workers.
use super::*;

#[derive(Default)]
pub(super) struct CopyUndoUi {
    cancellations: HashMap<u64, Arc<AtomicBool>>,
}
impl CopyUndoUi {
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
impl Drop for CopyUndoUi {
    fn drop(&mut self) {
        for flag in self.cancellations.values() {
            flag.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_undo_stop_is_per_operation_and_close_stops_remaining_workers() {
        let mut ui = CopyUndoUi::default();
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
