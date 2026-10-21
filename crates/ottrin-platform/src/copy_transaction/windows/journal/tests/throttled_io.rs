//! Explicit native fixture: the host throttles only a disposable secondary disk.
//! Control files stay on the responsive OS disk. The host must always release
//! throttling before this test joins its operation worker or removes fixtures.
use super::blocked_io::Recovery;
use super::*;
use std::{
    os::windows::io::AsRawHandle,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Threading::GetThreadIOPendingFlag;

fn wait_marker(control: &Path, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !control.join(name).try_exists().unwrap() {
        assert!(Instant::now() < deadline, "Host did not provide {name}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "Requires an owned secondary NTFS disk and a host throttle controller"]
fn windows_native_copy_throttled_journal_io_stop_preserves_data_and_recovers() {
    run_case("forward");
}
#[test]
#[ignore = "Requires an owned secondary NTFS disk and a host throttle controller"]
fn windows_native_copy_throttled_journal_undo_stop_preserves_data_and_recovers() {
    run_case("undo");
}
#[test]
#[ignore = "Requires an owned secondary NTFS disk and a host throttle controller"]
fn windows_native_copy_throttled_journal_cleanup_stop_preserves_data_and_recovers() {
    run_case("cleanup");
}

// Assertion/unwind paths must join the worker before TempDir removes its data.
struct OwnedWorker(Option<std::thread::JoinHandle<()>>);
impl OwnedWorker {
    fn join(mut self) -> std::thread::Result<()> {
        self.0.take().unwrap().join()
    }
}
impl Drop for OwnedWorker {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}
fn run_case(operation: &str) {
    let volume = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_IO_VOLUME")
            .expect("Owned secondary volume required"),
    );
    let control = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_TEST_WINDOWS_COPY_IO_CONTROL")
            .expect("OS-disk control directory required"),
    );
    let token =
        std::env::var("OTTRIN_TEST_WINDOWS_COPY_IO_TOKEN").expect("Volume owner token required");
    assert_eq!(
        std::fs::read_to_string(volume.join(".ottrin-owned-secondary-volume"))
            .unwrap()
            .trim(),
        token
    );
    assert_ne!(
        volume.components().next(),
        control.components().next(),
        "Control must remain on another volume"
    );
    let root = tempfile::tempdir_in(&volume).unwrap();
    let registry = root.path().join("central");
    let destination = root.path().join("destination");
    let (name, candidate, _cleanup) =
        staged_resume_fixture_at(root.path(), &destination, true, false, Some(&registry));
    remove_undo_fixture_source(root.path());
    let local = destination.join(&name).join(LOCAL);
    let header = decode(&std::fs::read(&local).unwrap())
        .unwrap()
        .state
        .header
        .unwrap();
    if operation != "forward" {
        let parent = owner(&destination);
        let central = owner(&registry);
        let mut publication =
            PermissionStage::publish(&parent, &central, &name, |_| Ok(())).unwrap();
        publication.apply(|_| Ok(())).unwrap();
        if operation == "cleanup" {
            drop(publication);
            undo_completed_copy(&parent, &central, &name, |_| Ok(())).unwrap();
        }
    }
    let payload = if operation == "undo" {
        destination.join("copy/nested/file")
    } else {
        destination.join(&candidate).join("nested/file")
    };
    let payload_before = std::fs::read(&payload).unwrap();
    std::fs::write(destination.join("foreign"), b"unrelated data").unwrap();
    let mut plan = match operation {
        "forward" => Recovery::Forward(
            discovery::RetainedCopyResumePlan::admit(&registry, &header.transaction, |_| Ok(()))
                .unwrap(),
        ),
        "cleanup" => Recovery::Cleanup(Box::new(
            RetainedCopyCleanupPlan::observe_at(
                &registry,
                std::slice::from_ref(&header.transaction),
                |_| Ok(()),
            )
            .unwrap(),
        )),
        "undo" => Recovery::SessionUndo {
            receipt: Box::new(
                discovery::session_receipt(&owner(&destination), &owner(&registry), &name, |_| {
                    Ok(())
                })
                .unwrap(),
            ),
            complete: false,
        },
        _ => unreachable!(),
    };
    let flag = Arc::new(AtomicBool::new(false));
    let worker_flag = flag.clone();
    let (ready_tx, ready) = mpsc::channel();
    let (go_tx, go) = mpsc::channel();
    let (result_tx, result) = mpsc::channel();
    let worker = OwnedWorker(Some(std::thread::spawn(move || {
        let guard = crate::NativeIoCancellation::start(worker_flag.clone()).unwrap();
        APPEND_HANDOFF.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                ready_tx.send(()).unwrap();
                go.recv_timeout(Duration::from_secs(30)).unwrap();
            }))
        });
        let outcome = plan.run(|_| {
            if worker_flag.load(Ordering::Acquire) {
                Err(io("Stopped between native I/O calls"))
            } else {
                Ok(())
            }
        });
        let finish = guard.finish();
        result_tx.send((outcome, finish, plan)).unwrap();
    })));
    if let Err(error) = ready.recv_timeout(Duration::from_secs(15)) {
        worker.join().unwrap();
        panic!(
            "Operation never reached journal append: {error}; outcome={:?}",
            result.try_recv().map(|value| value.0)
        );
    }
    std::fs::write(
        control.join("ready"),
        root.path().to_string_lossy().as_bytes(),
    )
    .unwrap();
    wait_marker(&control, "go");
    let _ = go_tx.send(());
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut pending_seen = false;
    while Instant::now() < deadline {
        let mut pending = 0;
        let ok = unsafe {
            GetThreadIOPendingFlag(
                worker.0.as_ref().unwrap().as_raw_handle().cast(),
                &mut pending,
            )
        };
        if ok != 0 && pending != 0 {
            pending_seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let start = Instant::now();
    flag.store(true, Ordering::Release);
    let stopped = result.recv_timeout(Duration::from_secs(3));
    let elapsed = start.elapsed();
    std::fs::write(
        control.join("cancel-observed"),
        format!(
            "pending={pending_seen}; returned={}; elapsed_ms={}",
            stopped.is_ok(),
            elapsed.as_millis()
        ),
    )
    .unwrap();
    // Even a failed cancellation must be unthrottled before joining the worker.
    wait_marker(&control, "released");
    let mut completion = match stopped {
        Ok(value) => value,
        Err(_) => result.recv_timeout(Duration::from_secs(15)).unwrap(),
    };
    worker.join().unwrap();
    assert!(pending_seen, "No real pending worker I/O was observed");
    completion.1.unwrap();
    println!(
        "throttled journal result: elapsed_ms={} result={:?} complete={}",
        elapsed.as_millis(),
        completion.0,
        completion.2.complete()
    );
    assert!(!completion.2.complete(), "Stop must not complete operation");
    assert!(completion.0.is_err());
    assert_eq!(destination.join("copy").exists(), operation == "undo");
    assert_eq!(std::fs::read(&payload).unwrap(), payload_before);
    assert_eq!(
        std::fs::read(destination.join("foreign")).unwrap(),
        b"unrelated data"
    );
    assert!(!root.path().join("source").exists());
    // Retry the retained operation with the same authority after storage resumes.
    completion.2.run(|_| Ok(())).unwrap();
    assert!(completion.2.complete());
    if operation == "cleanup" {
        assert!(!payload.exists());
        assert!(!local.exists());
        assert!(discover_at(&registry).unwrap().is_empty());
    } else {
        let restored = if operation == "forward" {
            destination.join("copy/nested/file")
        } else {
            destination.join(&candidate).join("nested/file")
        };
        assert_eq!(std::fs::read(restored).unwrap(), payload_before);
    }
    assert_eq!(
        std::fs::read(destination.join("foreign")).unwrap(),
        b"unrelated data"
    );
}
