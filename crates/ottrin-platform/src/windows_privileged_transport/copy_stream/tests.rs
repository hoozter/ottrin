use super::*;
use std::sync::atomic::AtomicI32;

mod invocation;

type WorkerResult = (io::Result<()>, io::Result<[u8; 5]>);
type WatchResult = (bool, io::Result<()>);

// Both threads are owned before any assertion can run. The deadline releases
// both synchronous IO peers even when Stop is broken, then every exit joins them.
struct Case {
    done: Option<mpsc::Sender<()>>,
    watch: Option<JoinHandle<WatchResult>>,
    worker: Option<JoinHandle<WorkerResult>>,
    ready: Arc<AtomicBool>,
    code: Arc<AtomicI32>,
}
impl Case {
    fn finish(
        &mut self,
    ) -> (
        Option<thread::Result<WatchResult>>,
        Option<thread::Result<WorkerResult>>,
    ) {
        if let Some(done) = self.done.take() {
            let _ = done.send(());
        }
        let watch = self.watch.take().map(|worker| worker.join());
        let worker = self.worker.take().map(|worker| worker.join());
        (watch, worker)
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let (watch, worker) = self.finish();
        if watch.is_some_and(|result| result.is_err())
            || worker.is_some_and(|result| result.is_err())
        {
            tracing::error!("Copy transport fixture thread panicked during teardown");
        }
    }
}

fn receipt() -> ottrin_core::PrivilegedPayload {
    ottrin_core::PrivilegedPayload::Copy(ottrin_core::PrivilegedCopyReceipt {
        completed_sources: 1,
        transactions: vec!["a".repeat(32)],
        paths: vec![(
            PathBuf::from(r"C:\OttrinOwnedTransport\source"),
            PathBuf::from(r"C:\OttrinOwnedTransport\copy"),
        )],
    })
}

fn synchronous_read<const N: usize>(reader: &std::io::PipeReader) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    let mut count = 0;
    // Use the native result directly: Read::read_exact may retry Interrupted,
    // which would conceal the cancellation this fixture must observe.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::ReadFile(
            reader.as_raw_handle(),
            bytes.as_mut_ptr(),
            N as u32,
            &mut count,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if count != N as u32 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Owned IO peer closed",
        ));
    }
    Ok(bytes)
}

fn pair() -> (LocalPipeStream, LocalPipeStream) {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).unwrap();
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let name = endpoint(&nonce).unwrap();
    let mut listener = LocalPipeListener::bind(&name, "D:P(A;;GA;;;OW)(A;;GA;;;SY)").unwrap();
    let client = LocalPipeStream::connect(&name, Duration::from_secs(1), |pid| {
        verify_pid(pid, std::process::id())
    })
    .unwrap();
    let server = listener
        .accept(Duration::from_secs(1), |pid| {
            verify_pid(pid, std::process::id())
        })
        .unwrap()
        .unwrap();
    (client, server)
}

fn start(blocked: bool) -> (LocalPipeStream, Case) {
    let (client, mut server) = pair();
    let (blocked_reader, unblock) = std::io::pipe().unwrap();
    let (later_reader, mut later_writer) = std::io::pipe().unwrap();
    let (done, deadline) = mpsc::channel();
    let mut case = Case {
        done: Some(done),
        watch: None,
        worker: None,
        ready: Arc::new(AtomicBool::new(false)),
        code: Arc::new(AtomicI32::new(0)),
    };
    case.watch = Some(
        thread::Builder::new()
            .name("ottrin-copy-fixture-deadline".into())
            .spawn(move || {
                let expired = deadline.recv_timeout(Duration::from_secs(5)).is_err();
                drop(unblock);
                thread::sleep(Duration::from_millis(60));
                (expired, later_writer.write_all(b"later"))
            })
            .unwrap(),
    );
    let ready = Arc::clone(&case.ready);
    let code = Arc::clone(&case.code);
    case.worker = Some(
        thread::Builder::new()
            .name("ottrin-copy-fixture-operation".into())
            .spawn(move || {
                let transport = serve(&mut server, |progress| {
                    let result = (|| {
                        progress(0)?;
                        ready.store(true, Ordering::Release);
                        if blocked && let Err(error) = synchronous_read::<1>(&blocked_reader) {
                            code.store(error.raw_os_error().unwrap_or(-1), Ordering::Release);
                            return Err(PlatformError::Io(error.to_string()));
                        }
                        progress(23)?;
                        Ok(())
                    })();
                    PrivilegedResponse {
                        status: if result.is_ok() {
                            PrivilegedStatus::Success
                        } else {
                            PrivilegedStatus::Failed
                        },
                        message: result.err().map(|error: PlatformError| error.to_string()),
                        // Represents an earlier completed prefix. This fixture verifies
                        // byte/receipt transport, not filesystem authority or real Copy.
                        payload: Some(receipt()),
                    }
                });
                let later = synchronous_read::<5>(&later_reader);
                (transport, later)
            })
            .unwrap(),
    );
    (client, case)
}

fn finish(case: &mut Case) -> io::Result<()> {
    let (watch, worker) = case.finish();
    let (expired, wrote) = watch.unwrap().unwrap();
    let (transport, later) = worker.unwrap().unwrap();
    assert!(!expired, "Transport fixture exceeded its owned IO deadline");
    wrote.unwrap();
    assert_eq!(
        later.unwrap(),
        *b"later",
        "Late cancellation affected unrelated worker IO"
    );
    transport
}

#[test]
fn windows_copy_stream_progress_preserves_completed_receipt() {
    let (mut client, mut case) = start(false);
    let mut values = Vec::new();
    let response = receive(&mut client, &mut |bytes| {
        values.push(bytes);
        Ok(())
    });
    finish(&mut case).unwrap();
    let response = response.unwrap();
    assert_eq!(response.status, PrivilegedStatus::Success);
    assert_eq!(response.payload, Some(receipt()));
    assert!(values.contains(&23));
}

#[test]
fn windows_copy_stream_stop_interrupts_blocked_io_and_preserves_receipt() {
    let (mut client, mut case) = start(true);
    let response = receive(&mut client, &mut |_| {
        if case.ready.load(Ordering::Acquire) {
            Err(PlatformError::Io("Owned streamed Stop".into()))
        } else {
            Ok(())
        }
    });
    finish(&mut case).unwrap();
    let response = response.unwrap();
    assert_eq!(response.status, PrivilegedStatus::Failed);
    assert_eq!(response.payload, Some(receipt()));
    assert_eq!(
        case.code.load(Ordering::Acquire),
        windows_sys::Win32::Foundation::ERROR_OPERATION_ABORTED as i32
    );
    assert!(response.message.unwrap().contains("Owned streamed Stop"));
}

#[test]
fn windows_copy_stream_completed_operation_wins_late_stop() {
    let (mut client, mut case) = start(false);
    let response = receive(&mut client, &mut |bytes| {
        if bytes == 23 {
            Err(PlatformError::Io("Owned late Stop".into()))
        } else {
            Ok(())
        }
    });
    finish(&mut case).unwrap();
    let response = response.unwrap();
    assert_eq!(response.status, PrivilegedStatus::Success);
    assert_eq!(response.payload, Some(receipt()));
    assert!(response.message.unwrap().contains("Owned late Stop"));
}

#[test]
fn windows_copy_stream_disconnect_interrupts_blocked_io() {
    let (client, mut case) = start(true);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !case.ready.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let ready = case.ready.load(Ordering::Acquire);
    drop(client);
    // Observe cancellation while the original blocking peer is still held.
    // finish() releases that peer as a failure fallback, which otherwise races
    // the cancellation watcher and produces BrokenPipe instead of aborted IO.
    let deadline = Instant::now() + Duration::from_secs(2);
    while case.code.load(Ordering::Acquire) == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let cancelled = case.code.load(Ordering::Acquire);
    let result = finish(&mut case);
    assert!(ready, "Owned operation did not enter its blocking IO");
    assert!(result.is_err());
    assert_eq!(
        cancelled,
        windows_sys::Win32::Foundation::ERROR_OPERATION_ABORTED as i32
    );
}

#[test]
fn windows_copy_stream_malformed_control_cannot_report_success() {
    let (mut client, mut case) = start(true);
    client.write_all(&[2]).unwrap();
    let response = receive(&mut client, &mut |_| Ok(()));
    finish(&mut case).unwrap();
    let response = response.unwrap();
    assert_eq!(response.status, PrivilegedStatus::Failed);
    assert_eq!(response.payload, Some(receipt()));
    assert!(
        response
            .message
            .unwrap()
            .contains("Invalid Copy control byte")
    );
}

#[test]
fn windows_copy_stream_presend_stop_sends_no_request() {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).unwrap();
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut listener =
        LocalPipeListener::bind(&endpoint(&nonce).unwrap(), "D:P(A;;GA;;;OW)(A;;GA;;;SY)").unwrap();
    let started = process_start_time(unsafe { GetCurrentProcess() }).unwrap();
    let mut helper = HelperChannel::connect(&nonce, std::process::id(), started).unwrap();
    let mut client = listener
        .accept(Duration::from_secs(1), |pid| {
            verify_pid(pid, std::process::id())
        })
        .unwrap()
        .unwrap();
    let request = PrivilegedRequest {
        protocol_version: ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION,
        command: ottrin_core::PrivilegedCommand::DiscoverCopyHistory,
        context: ottrin_core::PrivilegedContext {
            reason: None,
            cwd: None,
        },
    };
    let result = exchange(&mut client, &request, &mut |_| {
        Err(PlatformError::Io("Owned presend Stop".into()))
    });
    drop(client);
    let incoming = helper.read_request();
    assert!(matches!(result, Err(PlatformError::Io(message)) if message == "Owned presend Stop"));
    assert!(
        incoming.is_err(),
        "Stopped invocation supplied authority-bearing request bytes"
    );
}

#[test]
fn windows_copy_stream_callback_panic_joins_reader() {
    let (mut client, mut case) = start(true);
    let started = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        receive(&mut client, &mut |_| {
            if case.ready.load(Ordering::Acquire) {
                panic!("Owned progress callback unwind");
            }
            Ok(())
        })
    }));
    let elapsed = started.elapsed();
    drop(client);
    let transport = finish(&mut case);
    assert!(result.is_err());
    assert!(transport.is_err());
    assert!(
        elapsed < Duration::from_secs(2),
        "Response reader remained blocked during callback unwind"
    );
}

#[test]
fn windows_copy_stream_fragmented_frame_survives_read_timeouts() {
    let (mut client, mut server) = pair();
    client.set_io_timeout(POLL);
    let mut input = Input {
        stream: client,
        finished: Arc::new(AtomicBool::new(false)),
    };
    let body = serde_json::to_vec(&Event::Progress { bytes: 23 }).unwrap();
    let mut frame = (body.len() as u32).to_be_bytes().to_vec();
    frame.extend(body);
    thread::scope(|scope| {
        let writer = scope.spawn(|| {
            server.write_all(&frame[..2])?;
            thread::sleep(POLL + Duration::from_millis(50));
            let middle = 4 + (frame.len() - 4) / 2;
            server.write_all(&frame[2..middle])?;
            thread::sleep(POLL + Duration::from_millis(50));
            server.write_all(&frame[middle..])
        });
        let result = read_json::<Event>(&mut input);
        writer.join().unwrap().unwrap();
        assert!(matches!(result, Ok(Event::Progress { bytes: 23 })));
    });
}

#[test]
fn windows_copy_stream_reader_stop_without_pending_io() {
    let (mut client, server) = pair();
    client.set_io_timeout(POLL);
    let mut input = Input {
        stream: client,
        finished: Arc::new(AtomicBool::new(true)),
    };
    thread::scope(|scope| {
        // Failure fallback: a missing stop check must not hang this fixture.
        scope.spawn(move || {
            thread::sleep(Duration::from_millis(300));
            drop(server);
        });
        let started = Instant::now();
        let result = input.read(&mut [0]);
        assert!(started.elapsed() < POLL);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
    });
}
