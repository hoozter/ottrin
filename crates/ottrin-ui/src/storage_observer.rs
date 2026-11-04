//! Observe storage off the GUI thread; unchanged roots never request a frame.
use crate::ResultRepaint;
use ottrin_platform::StorageLocation;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

struct Snapshot {
    observed: Instant,
    locations: Vec<StorageLocation>,
}

#[derive(Default)]
struct Shared {
    snapshot: Mutex<Option<Snapshot>>,
    stopped: AtomicBool,
    repaint: ResultRepaint,
}

impl Shared {
    fn publish(&self, observed: Instant, locations: Vec<StorageLocation>) {
        let mut snapshot = self.snapshot.lock().expect("storage snapshot");
        // An in-flight discovery must not overwrite a newer mutation-time check.
        if snapshot.as_ref().is_some_and(|old| old.observed > observed) {
            return;
        }
        let changed = snapshot
            .as_ref()
            .is_none_or(|old| old.locations != locations);
        *snapshot = Some(Snapshot {
            observed,
            locations,
        });
        drop(snapshot);
        if changed {
            self.repaint.wake();
        }
    }
}

pub(crate) struct StorageObserver {
    shared: Arc<Shared>,
    stop: Option<mpsc::Sender<()>>,
}

impl StorageObserver {
    pub(crate) fn new(repaint: ResultRepaint) -> Self {
        Self {
            shared: Arc::new(Shared {
                repaint,
                ..Default::default()
            }),
            stop: None,
        }
    }

    #[cfg(not(test))]
    pub(crate) fn start(&mut self) {
        if self.stop.is_none() {
            self.start_worker(ottrin_platform::discover_storage_locations);
        }
    }

    #[cfg(test)]
    fn with_discovery(
        repaint: ResultRepaint,
        discover: impl FnMut() -> Vec<StorageLocation> + Send + 'static,
    ) -> Self {
        let mut observer = Self::new(repaint);
        observer.start_worker(discover);
        observer
    }

    fn start_worker(
        &mut self,
        mut discover: impl FnMut() -> Vec<StorageLocation> + Send + 'static,
    ) {
        assert!(self.stop.is_none(), "Storage observer already started");
        let (stop, requests) = mpsc::channel();
        let state = self.shared.clone();
        std::thread::Builder::new()
            .name("ottrin-storage-ui".into())
            .spawn(move || {
                loop {
                    let observed = Instant::now();
                    let locations = discover();
                    if state.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    state.publish(observed, locations);
                    match requests.recv_timeout(POLL_INTERVAL) {
                        Err(mpsc::RecvTimeoutError::Timeout) => (),
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("Cannot start storage observation worker");
        self.stop = Some(stop);
    }

    pub(crate) fn snapshot(&self) -> Option<Vec<StorageLocation>> {
        self.shared
            .snapshot
            .lock()
            .expect("storage snapshot")
            .as_ref()
            .map(|snapshot| snapshot.locations.clone())
    }

    /// Publish the result of an existing fresh file-operation guard. The next
    /// background observation compares against this same authoritative snapshot.
    pub(crate) fn record_current(&self, locations: Vec<StorageLocation>) {
        self.shared.publish(Instant::now(), locations);
    }
}

impl Drop for StorageObserver {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Platform drive queries may block on disconnected network storage.
        // Closing the app must not join such a query; its result is discarded.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_platform::StorageLocationKind;

    fn location(name: &str) -> StorageLocation {
        StorageLocation {
            path: name.into(),
            identity: name.into(),
            stable_identity: Some(name.into()),
            kind: StorageLocationKind::Removable,
            display_name: Some(name.into()),
        }
    }

    fn bind(repaint: &ResultRepaint) -> (egui::Context, mpsc::Receiver<Duration>) {
        let ctx = egui::Context::default();
        let (wake, wakes) = mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            let _ = wake.send(info.delay);
        });
        repaint.bind(&ctx);
        (ctx, wakes)
    }

    #[test]
    fn unchanged_observations_are_quiet_and_stale_discovery_cannot_replace_a_fresh_guard() {
        let repaint = ResultRepaint::default();
        let shared = Shared {
            repaint: repaint.clone(),
            ..Default::default()
        };
        let old = Instant::now();
        let roots = vec![location("fresh-volume")];
        let (_ctx, first) = bind(&repaint);
        shared.publish(Instant::now(), roots.clone());
        assert_eq!(
            first.recv_timeout(Duration::from_secs(1)).unwrap(),
            Duration::ZERO
        );
        let (_ctx, unchanged) = bind(&repaint);
        shared.publish(Instant::now(), roots.clone());
        shared.publish(old, vec![location("stale-slot")]);
        assert!(unchanged.try_recv().is_err());
        assert_eq!(
            shared.snapshot.lock().unwrap().as_ref().unwrap().locations,
            roots
        );
        shared.publish(Instant::now(), vec![]);
        assert_eq!(
            unchanged.recv_timeout(Duration::from_secs(1)).unwrap(),
            Duration::ZERO
        );
    }

    #[test]
    fn a_sleeping_gui_receives_changed_roots_from_the_background_worker() {
        let repaint = ResultRepaint::default();
        let (_ctx, wakes) = bind(&repaint);
        let (query, queries) = mpsc::channel();
        let (reply, replies) = mpsc::channel();
        let (gone, dropped) = mpsc::channel();
        struct Finished(mpsc::Sender<()>);
        impl Drop for Finished {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let finished = Finished(gone);
        let observer = StorageObserver::with_discovery(repaint.clone(), move || {
            let _keep = &finished;
            query.send(()).unwrap();
            replies.recv_timeout(Duration::from_secs(5)).unwrap()
        });
        queries.recv_timeout(Duration::from_secs(1)).unwrap();
        reply.send(vec![location("mounted")]).unwrap();
        wakes.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(observer.snapshot(), Some(vec![location("mounted")]));
        let (_ctx, quiet) = bind(&repaint);
        queries.recv_timeout(Duration::from_secs(3)).unwrap();
        reply.send(vec![location("mounted")]).unwrap();
        queries.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            quiet.try_recv().is_err(),
            "Unchanged storage repainted the GUI"
        );
        reply.send(vec![]).unwrap();
        quiet.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(observer.snapshot(), Some(vec![]));
        drop(observer);
        dropped.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn closing_does_not_wait_for_a_blocked_drive_query_or_publish_its_result() {
        let (entered, running) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let repaint = ResultRepaint::default();
        let (_ctx, wakes) = bind(&repaint);
        let observer = StorageObserver::with_discovery(repaint, move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(4)).unwrap();
            vec![location("disconnected-network-drive")]
        });
        running.recv_timeout(Duration::from_secs(1)).unwrap();
        let shared = observer.shared.clone();
        let (closed, closed_rx) = mpsc::channel();
        let closing = std::thread::spawn(move || {
            drop(observer);
            closed.send(()).unwrap();
        });
        let result = closed_rx.recv_timeout(Duration::from_secs(1));
        release.send(()).unwrap();
        closing.join().unwrap();
        assert!(result.is_ok(), "App close waited for an OS drive query");
        // A second discovery would fail its now-disconnected entered channel.
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&shared) != 1 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(shared.snapshot.lock().unwrap().is_none());
        assert!(wakes.try_recv().is_err());
    }
}
