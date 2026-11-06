//! Real process-exit observation for the invocation owner, including unwinding.
//! This is a native child fixture, not ShellExecute/UAC or package acceptance.
use super::*;

const CHILD: &str =
    "windows_privileged_transport::copy_stream::tests::invocation::windows_copy_invocation_child";

struct Watch {
    worker: Option<JoinHandle<io::Result<std::process::ExitStatus>>>,
    forced: Arc<AtomicBool>,
    process: Option<OwnedHandle>,
}

struct Child(std::process::Child);

impl Drop for Child {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            if let Err(error) = self.0.kill() {
                eprintln!("Invocation fixture child stop failed: {error}");
            }
            if let Err(error) = self.0.wait() {
                eprintln!("Invocation fixture child wait failed: {error}");
            }
        }
    }
}

impl Watch {
    fn start(child: std::process::Child) -> Self {
        let mut child = Child(child);
        let forced = Arc::new(AtomicBool::new(false));
        let timed_out = Arc::clone(&forced);
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = child.0.try_wait()? {
                    return Ok(status);
                }
                if Instant::now() >= deadline {
                    timed_out.store(true, Ordering::Release);
                    child.0.kill()?;
                    return child.0.wait();
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            worker: Some(worker),
            forced,
            process: None,
        }
    }

    fn exited(&self) -> bool {
        // SAFETY: the fixture retains this exact process generation until its
        // watchdog is joined. This query never waits or substitutes a PID.
        unsafe {
            WaitForSingleObject(self.process.as_ref().unwrap().as_raw_handle(), 0) == WAIT_OBJECT_0
        }
    }

    fn finish(&mut self) -> io::Result<std::process::ExitStatus> {
        self.worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| io::Error::other("Invocation fixture watchdog panicked"))?
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.worker.is_some()
            && let Err(error) = self.finish()
        {
            eprintln!("Invocation fixture teardown failed: {error}");
        }
    }
}

fn invocation(root: &Path, lost_response: bool) -> (CopyInvocation, Watch) {
    let mut nonce = [0; 16];
    getrandom::fill(&mut nonce).unwrap();
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut listener =
        LocalPipeListener::bind(&endpoint(&nonce).unwrap(), "D:P(A;;GA;;;OW)(A;;GA;;;SY)").unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("OTTRIN_INVOCATION_NONCE", nonce)
        .env("OTTRIN_INVOCATION_PARENT", std::process::id().to_string())
        .env(
            "OTTRIN_INVOCATION_WITNESS",
            root.join("exited-after-connection"),
        )
        .env(
            "OTTRIN_INVOCATION_LOST",
            if lost_response { "1" } else { "0" },
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    // Register the bounded watchdog before assertions or pipe admission. A
    // broken production wait can fail, but cannot orphan this owned process.
    let mut watch = Watch::start(child);
    // SAFETY: no pointers are supplied. The successful non-inheritable process
    // handle is retained through the production wait and prevents PID reuse.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null(), "{}", io::Error::last_os_error());
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    watch.process = Some(process.try_clone().unwrap());
    let stream = listener
        .accept(Duration::from_secs(3), |actual| verify_pid(actual, pid))
        .unwrap()
        .unwrap();
    (
        CopyInvocation {
            stream: Some(stream),
            process: Some(process),
        },
        watch,
    )
}

#[test]
#[ignore = "Owned subprocess entry; invoked by the parent lifecycle tests"]
fn windows_copy_invocation_child() {
    let nonce = std::env::var("OTTRIN_INVOCATION_NONCE").unwrap();
    let parent = std::env::var("OTTRIN_INVOCATION_PARENT")
        .unwrap()
        .parse()
        .unwrap();
    let witness = PathBuf::from(std::env::var_os("OTTRIN_INVOCATION_WITNESS").unwrap());
    let mut stream = LocalPipeStream::connect(
        &endpoint(&nonce).unwrap(),
        Duration::from_secs(3),
        |actual| verify_pid(actual, parent),
    )
    .unwrap();
    stream.set_io_timeout(Duration::from_secs(3));
    write_json(&mut stream, &Event::Progress { bytes: 23 }).unwrap();
    if std::env::var("OTTRIN_INVOCATION_LOST").unwrap() == "1" {
        drop(stream);
    } else {
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {}
            result => panic!("Invocation did not close its pipe: {result:?}"),
        }
    }
    // The response/connection ends before the process finishes its work. The
    // parent must observe the actual process handle, not just EOF or a counter.
    thread::sleep(Duration::from_millis(80));
    std::fs::write(witness, b"owned helper finished").unwrap();
}

#[test]
fn windows_copy_invocation_disconnect_and_unwind_wait_for_process_exit() {
    for unwind in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut call, mut watch) = invocation(root.path(), false);
        let event = read_json::<Event>(call.stream.as_mut().unwrap()).unwrap();
        assert!(matches!(event, Event::Progress { bytes: 23 }));
        assert!(!root.path().join("exited-after-connection").exists());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if unwind {
                let mut call = call;
                let _response = receive(call.stream.as_mut().unwrap(), &mut |_| {
                    panic!("Owned invocation callback unwind");
                });
                unreachable!("Owned callback must unwind both reader and invocation");
            }
            call.finish().unwrap();
            // Repeated finalization must not wait again or affect another PID.
            call.finish().unwrap();
        }));
        assert_eq!(result.is_err(), unwind);
        let witness = std::fs::read(root.path().join("exited-after-connection"));
        let exited = watch.exited();
        let status = watch.finish().unwrap();
        assert!(
            exited,
            "Invocation returned while its helper process was live"
        );
        assert!(!watch.forced.load(Ordering::Acquire));
        assert!(status.success());
        assert_eq!(witness.unwrap(), b"owned helper finished");
    }
}

#[test]
fn windows_copy_invocation_lost_response_waits_for_process_exit() {
    let root = tempfile::tempdir().unwrap();
    let (mut call, mut watch) = invocation(root.path(), true);
    let response = receive(call.stream.as_mut().unwrap(), &mut |_| Ok(()));
    call.finish().unwrap();
    let witness = std::fs::read(root.path().join("exited-after-connection"));
    let exited = watch.exited();
    let status = watch.finish().unwrap();
    assert!(
        exited,
        "Lost response returned while its helper process was live"
    );
    assert!(!watch.forced.load(Ordering::Acquire));
    assert!(status.success());
    assert!(
        matches!(response, Err(PlatformError::Io(message)) if message.contains("may have changed files"))
    );
    assert_eq!(witness.unwrap(), b"owned helper finished");
}
