use super::ResultRepaint;
use ottrin_core::FileCommand;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};

#[derive(Debug)]
pub(super) struct Probe {
    pub id: u64,
    cancelled: Arc<AtomicBool>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

struct Request {
    id: u64,
    command: FileCommand,
    cancelled: Arc<AtomicBool>,
}

pub(super) struct Response {
    pub id: u64,
    pub conflict: bool,
}

/// A blocked filesystem lookup never creates an unbounded set of reader threads.
/// There is one reader, one queued request and one unread response per window.
pub(super) struct Worker {
    requests: SyncSender<Request>,
    responses: Receiver<Response>,
}

impl Worker {
    pub fn spawn(repaint: ResultRepaint) -> Self {
        Self::spawn_with(repaint, has_destination_conflict)
    }

    pub(super) fn spawn_with(
        repaint: ResultRepaint,
        mut check: impl FnMut(&FileCommand, &AtomicBool) -> bool + Send + 'static,
    ) -> Self {
        let (requests, request_rx) = mpsc::sync_channel::<Request>(1);
        let (response_tx, responses) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                if request.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                let conflict = check(&request.command, &request.cancelled);
                if request.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                if response_tx
                    .send(Response {
                        id: request.id,
                        conflict,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.wake();
            }
        });
        Self {
            requests,
            responses,
        }
    }

    pub fn request(&self, id: u64, command: FileCommand) -> Result<Probe, &'static str> {
        let cancelled = Arc::new(AtomicBool::new(false));
        match self.requests.try_send(Request {
            id,
            command,
            cancelled: Arc::clone(&cancelled),
        }) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                return Err(
                    "Destination checking is still busy. No transfer was started; retry when it completes.",
                );
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(
                    "Destination checking stopped. No transfer was started; restart Ottrin to retry.",
                );
            }
        }
        Ok(Probe { id, cancelled })
    }

    pub fn poll(&self) -> Result<Option<Response>, &'static str> {
        match self.responses.try_recv() {
            Ok(response) => Ok(Some(response)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(
                "Destination checking stopped. No transfer was started; restart Ottrin to retry.",
            ),
        }
    }
}

pub(super) fn has_destination_conflict(command: &FileCommand, cancelled: &AtomicBool) -> bool {
    let (FileCommand::Copy {
        sources,
        destination,
        ..
    }
    | FileCommand::Move {
        sources,
        destination,
        ..
    }) = command
    else {
        return false;
    };
    for source in sources {
        if cancelled.load(Ordering::Acquire) {
            return false;
        }
        if source
            .file_name()
            .is_some_and(|name| std::fs::symlink_metadata(destination.join(name)).is_ok())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::ConflictAction;
    use std::time::Duration;

    struct Stopped(mpsc::Sender<()>);
    impl Drop for Stopped {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }

    #[test]
    fn blocked_probe_has_bounded_queue_and_cancelled_requests_do_not_publish() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let stopped = Stopped(stopped_tx);
        let worker = Worker::spawn_with(ResultRepaint::default(), move |_, _| {
            let _ = &stopped;
            entered_tx.send(std::thread::current().id()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            true
        });
        let command = || FileCommand::Copy {
            sources: vec!["source".into()],
            destination: "destination".into(),
            conflict: ConflictAction::Rename,
        };
        let active = worker.request(1, command()).unwrap();
        assert_ne!(
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            std::thread::current().id()
        );
        let queued = worker.request(2, command()).unwrap();
        assert!(
            worker.request(3, command()).is_err(),
            "one blocked reader cannot grow the queue or thread count"
        );
        drop(active);
        drop(queued);
        release_tx.send(()).unwrap();
        let Worker {
            requests,
            responses,
        } = worker;
        drop(requests);
        stopped_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            entered_rx.try_recv().is_err(),
            "cancelled queued work never reaches the filesystem checker"
        );
        assert!(
            matches!(
                responses.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "cancelled work never publishes a late result"
        );
    }
}
