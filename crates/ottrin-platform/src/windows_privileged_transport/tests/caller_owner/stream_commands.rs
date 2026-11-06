//! Stop an actual caller Copy while source admission is blocked by an oplock.
use super::*;
use std::{sync::mpsc, thread};

pub(super) const REASON: &str = "owned caller streamed source Stop";

struct Lease {
    release: Option<mpsc::Sender<()>>,
    broken: mpsc::Receiver<u32>,
    worker: Option<thread::JoinHandle<std::io::Result<(u32, bool)>>>,
}
impl Lease {
    fn start(source: &Path) -> Self {
        let source = source.to_path_buf();
        let (ready, readiness) = mpsc::channel();
        let (released, release) = mpsc::channel();
        let (break_sender, broken) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut lease = match crate::test_oplock::Oplock::request(&source) {
                Ok(lease) => lease,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return Err(error);
                }
            };
            ready.send(Ok(())).map_err(std::io::Error::other)?;
            // A completed directory prefix precedes the selected source open.
            // One3s wait is not an operation deadline and must not request Stop
            // before that source has actually reached its oplock.
            let deadline = Instant::now() + Duration::from_secs(30);
            let status = loop {
                let status = lease.wait_break();
                if status != windows_sys::Win32::Foundation::WAIT_TIMEOUT
                    || Instant::now() >= deadline
                {
                    break status;
                }
                if release.try_recv().is_ok() {
                    lease.close()?;
                    return Ok((status, false));
                }
            };
            break_sender.send(status).map_err(std::io::Error::other)?;
            // Failure fallback releases the filesystem before assertions and
            // before the fixture can wait indefinitely for a broken Stop path.
            let expired = release.recv_timeout(Duration::from_secs(5)).is_err();
            lease.close()?;
            Ok((status, expired))
        });
        let result = Self {
            release: Some(released),
            broken,
            worker: Some(worker),
        };
        readiness
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        result
    }
    fn finish(&mut self) -> std::io::Result<(u32, bool)> {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        self.worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| std::io::Error::other("Owned source lease panicked"))?
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.worker.is_some()
            && let Err(error) = self.finish()
        {
            eprintln!("Owned source lease teardown failed: {error}");
        }
    }
}

pub(super) fn source_stop(
    pipe: &mut LocalPipeStream,
    source: &Path,
    destination: &Path,
    prefix: Option<&Path>,
) {
    let before = copy_commands::history(pipe);
    let bytes = std::fs::read(source).unwrap();
    let mut lease = Lease::start(source);
    let request = copy_commands::request(
        PrivilegedCommand::File(ottrin_core::FileCommand::Copy {
            sources: prefix
                .into_iter()
                .chain([source])
                .map(Path::to_path_buf)
                .collect(),
            destination: destination.into(),
            conflict: ottrin_core::ConflictAction::Rename,
        }),
        REASON,
    );
    let response =
        crate::windows_privileged_transport::copy_stream::exchange(pipe, &request, &mut |_| {
            match lease.broken.try_recv() {
                Ok(status) if status == windows_sys::Win32::Foundation::WAIT_OBJECT_0 => Err(
                    crate::PlatformError::Io("Owned streamed source Stop".into()),
                ),
                Ok(status) => Err(crate::PlatformError::Io(format!(
                    "Owned source lease did not break: {status}"
                ))),
                Err(mpsc::TryRecvError::Empty) => Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(crate::PlatformError::Io("Owned source lease ended".into()))
                }
            }
        });
    let close = lease.finish();
    let (status, expired) = close.unwrap();
    let response = response.unwrap();
    assert_eq!(
        status,
        windows_sys::Win32::Foundation::WAIT_OBJECT_0,
        "prefix={} response={response:?}",
        prefix.is_some()
    );
    assert!(
        !expired,
        "Source oplock had to be released to rescue blocked helper IO"
    );
    assert_eq!(response.status, PrivilegedStatus::Failed);
    assert!(response.message.unwrap().contains("995"));
    let Some(PrivilegedPayload::Copy(reference)) = response.payload else {
        panic!("Stopped Copy omitted its receipt summary")
    };
    assert_eq!(reference.completed_sources, usize::from(prefix.is_some()));
    assert_eq!(reference.transactions.len(), reference.completed_sources);
    assert_eq!(reference.paths.len(), reference.completed_sources);
    assert_eq!(std::fs::read(source).unwrap(), bytes);
    if prefix.is_some() {
        let history = copy_commands::history(pipe);
        assert!(history.iter().any(
            |entry| entry.reference.transactions == reference.transactions
                && entry.recovery == ottrin_core::PrivilegedCopyHistoryState::UndoAvailable
        ));
        copy_commands::streamed(
            pipe,
            PrivilegedCommand::UndoCopy {
                transactions: reference.transactions.clone(),
            },
        );
        copy_commands::streamed(
            pipe,
            PrivilegedCommand::CleanupCopy {
                transactions: reference.transactions,
            },
        );
    }
    assert_eq!(copy_commands::history(pipe), before);
    println!(
        "caller helper streamed Stop completed_prefix={}: actual source oplock IO interrupted with error995 before lease release; source preserved and completed receipts remain actionable through streamed Undo/cleanup",
        prefix.is_some()
    );
}
