//! Progress and Stop share the authenticated invocation's duplex pipe. A control
//! reader can cancel the operation thread while its synchronous I/O is blocked.
//! Joining every owned reader before return prevents late cancellation or IO.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use windows_sys::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED};

#[cfg(test)]
mod tests;

const STOP: u8 = 1;
const POLL: Duration = Duration::from_millis(100);

#[derive(Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Progress { bytes: u64 },
    Finished { response: PrivilegedResponse },
}

struct Control {
    finished: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl Control {
    fn start(stream: &LocalPipeStream, cancelled: Arc<AtomicBool>) -> io::Result<Self> {
        let mut reader = stream.try_clone()?;
        reader.set_io_timeout(POLL);
        let finished = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&finished);
        let worker = thread::Builder::new()
            .name("ottrin-helper-stop".into())
            .spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let mut byte = [0];
                    match reader.read(&mut byte) {
                        Ok(1) if byte[0] == STOP => cancelled.store(true, Ordering::Release),
                        Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
                        result => {
                            cancelled.store(true, Ordering::Release);
                            return Err(match result {
                                Err(error) => error,
                                Ok(0) => io::Error::new(
                                    io::ErrorKind::UnexpectedEof,
                                    "Copy client disconnected",
                                ),
                                _ => io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Invalid Copy control byte",
                                ),
                            });
                        }
                    }
                }
                Ok(())
            })?;
        Ok(Self {
            finished,
            worker: Some(worker),
        })
    }

    fn finish(&mut self) -> io::Result<()> {
        self.finished.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("Copy control reader panicked"))??;
        }
        Ok(())
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            tracing::error!(%error, "Could not finish Copy control reader");
        }
    }
}

pub(super) fn response_error(response: &mut PrivilegedResponse, error: impl std::fmt::Display) {
    response.status = PrivilegedStatus::Failed;
    response.message = Some(match response.message.take() {
        Some(message) => format!("{message}; {error}"),
        None => error.to_string(),
    });
    // A completed prefix's Copy receipt remains actionable even if cancellation
    // or transport cleanup fails after the engine returns it.
}

pub(super) fn serve(
    stream: &mut LocalPipeStream,
    operation: impl FnOnce(&mut dyn FnMut(u64) -> Result<(), PlatformError>) -> PrivilegedResponse,
) -> io::Result<()> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut control = Control::start(stream, Arc::clone(&cancelled))?;
    let mut response = match crate::NativeIoCancellation::start(Arc::clone(&cancelled)) {
        Err(error) => PrivilegedResponse {
            status: PrivilegedStatus::Failed,
            message: Some(error.to_string()),
            payload: None,
        },
        Ok(io_cancellation) => {
            let mut last = None;
            let mut latest = 0;
            let mut sent = 0;
            let mut response = operation(&mut |bytes| {
                if cancelled.load(Ordering::Acquire) {
                    return Err(PlatformError::Io(
                        "Copy operation stopped; retained data is preserved for recovery.".into(),
                    ));
                }
                latest = bytes;
                // The engine reports a counter, not byte deltas. Bound repeated
                // validation notifications; retain the latest counter at completion.
                if last.is_none_or(|at: Instant| at.elapsed() >= POLL) || (sent == 0 && bytes > 0) {
                    write_json(stream, &Event::Progress { bytes })
                        .map_err(|error| PlatformError::Io(error.to_string()))?;
                    last = Some(Instant::now());
                    sent = bytes;
                }
                Ok(())
            });
            if let Err(error) = io_cancellation.finish() {
                response_error(&mut response, error);
            }
            if latest != sent
                && let Err(error) = write_json(stream, &Event::Progress { bytes: latest })
            {
                response_error(&mut response, error);
            }
            response
        }
    };
    // End the byte reader before publishing completion, so a later request on
    // a fixture connection cannot be consumed as a Stop byte.
    if let Err(error) = control.finish() {
        response_error(&mut response, error);
    }
    write_json(stream, &Event::Finished { response })
}

struct Reader {
    finished: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Reader {
    fn finish(&mut self) -> io::Result<()> {
        if self.worker.is_none() {
            return Ok(());
        }
        self.finished.store(true, Ordering::Release);
        let joined = self.worker.take().map(|worker| worker.join());
        if joined.is_some_and(|result| result.is_err()) {
            return Err(io::Error::other("Copy response reader panicked"));
        }
        Ok(())
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            tracing::error!(%error, "Could not finish Copy response reader");
        }
    }
}

struct Input {
    stream: LocalPipeStream,
    finished: Arc<AtomicBool>,
}

impl Read for Input {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        while !self.finished.load(Ordering::Acquire) {
            match self.stream.read(buffer) {
                // LocalPipeStream drains each timed-out operation, returning
                // bytes if completion won the timeout race. Retrying here keeps
                // the exact partially read JSON frame; it never restarts parsing.
                Err(error) if error.kind() == io::ErrorKind::TimedOut => continue,
                result => return result,
            }
        }
        Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "Copy response reader stopped",
        ))
    }
}

pub(super) fn receive(
    stream: &mut LocalPipeStream,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<PrivilegedResponse, PlatformError> {
    let mut input = stream.try_clone().map_err(io_error)?;
    input.set_io_timeout(POLL);
    let finished = Arc::new(AtomicBool::new(false));
    let mut input = Input {
        stream: input,
        finished: Arc::clone(&finished),
    };
    let (sender, receiver) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("ottrin-helper-response".into())
        .spawn(move || {
            loop {
                let event = read_json::<Event>(&mut input);
                let finished = !matches!(event, Ok(Event::Progress { .. }));
                if sender.send(event).is_err() || finished {
                    break;
                }
            }
        })
        .map_err(io_error)?;
    let mut reader = Reader {
        finished,
        worker: Some(worker),
    };
    let mut stopped = None;
    let outcome = loop {
        let bytes = match receiver.recv_timeout(POLL) {
            Ok(Ok(Event::Finished { mut response })) => {
                if let Some(error) = stopped {
                    // Completion can win the race with Stop. Preserve the actual
                    // terminal status and every receipt; report the callback cause.
                    response.message = Some(match response.message.take() {
                        Some(message) => format!("{message}; progress callback: {error}"),
                        None if response.status == PrivilegedStatus::Success => {
                            format!("Operation completed; progress callback: {error}")
                        }
                        None => format!("Progress callback: {error}"),
                    });
                }
                break Ok(response);
            }
            Ok(Ok(Event::Progress { bytes })) => bytes,
            Err(mpsc::RecvTimeoutError::Timeout) => 0,
            Ok(Err(error)) => break Err(response_failure(error)),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(response_failure("Copy response reader stopped"));
            }
        };
        if stopped.is_none()
            && let Err(error) = progress(bytes)
        {
            stopped = Some(error.to_string());
            if let Err(error) = stream.write_all(&[STOP]) {
                let disconnected = error.raw_os_error().is_some_and(|code| {
                    matches!(
                        code as u32,
                        ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED
                    )
                });
                if !disconnected {
                    break Err(response_failure(error));
                }
                // The helper may have written a terminal response and closed
                // its end before Stop was sent. The response reader still
                // owns any queued frame and will report a true disconnect.
            }
        }
    };
    match reader.finish() {
        Ok(()) => outcome,
        Err(error) => match outcome {
            Ok(mut response) => {
                response_error(&mut response, error);
                Ok(response)
            }
            Err(operation) => Err(response_failure(format!("{operation}; {error}"))),
        },
    }
}

pub(super) fn exchange(
    stream: &mut LocalPipeStream,
    request: &PrivilegedRequest,
    progress: &mut dyn FnMut(u64) -> Result<(), PlatformError>,
) -> Result<PrivilegedResponse, PlatformError> {
    // In particular, recheck a Stop requested during the administrator prompt.
    // No authority-bearing request bytes have been sent at this boundary.
    progress(0)?;
    write_json(stream, request).map_err(response_failure)?;
    receive(stream, progress)
}

fn response_failure(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!(
        "Administrator Copy response failed: {error}. The operation may have changed files; review its destination and recovery history before retrying."
    ))
}
