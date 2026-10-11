#![cfg(target_os = "windows")]

//! Native process fixtures for the Windows Office boundary. They exercise the
//! real launcher; they do not stand in for installed-LibreOffice conversion QA.
use ottrin_preview::{
    IsolatedProcessFailure, IsolatedProcessLauncher, IsolatedProcessRequest,
    WindowsJobObjectIsolation,
};
use std::fs;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

struct Fixture {
    root: PathBuf,
    request: IsolatedProcessRequest,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ottrin native job {} {nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let profile = root.join("profile with spaces");
        let output = root.join("output with spaces");
        // Own the root before fallible setup so unwinding removes partial fixtures.
        let fixture = Self {
            root,
            request: IsolatedProcessRequest {
                executable: profile.join("fixture.exe"),
                arguments: ["--exact", "fixture_root", "--ignored", "--nocapture"]
                    .map(Into::into)
                    .to_vec(),
                input: None,
                private_profile_dir: profile.clone(),
                private_output_dir: output.clone(),
                timeout: Duration::from_secs(10),
                max_memory_bytes: 64 * 1024 * 1024,
                max_processes: 4,
                max_output_bytes: 1024,
            },
        };
        fs::create_dir(&profile).unwrap();
        fs::create_dir(&output).unwrap();
        // Descendants need an executable inside the same private access policy.
        // Copy bytes into a new file without inheriting the build artifact's ACL.
        std::io::copy(
            &mut fs::File::open(std::env::current_exe().unwrap()).unwrap(),
            &mut fs::File::create(&fixture.request.executable).unwrap(),
        )
        .unwrap();
        fs::write(profile.join("mode"), mode).unwrap();
        fs::write(profile.join("output-dir"), output.to_str().unwrap()).unwrap();
        fixture
    }

    fn assert_logs_removed(&self) {
        assert!(
            fs::read_dir(&self.request.private_profile_dir)
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-process-")
                })
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("native fixture must release all private files");
    }
}

#[test]
#[ignore = "subprocess fixture selected explicitly by native_job tests"]
fn fixture_root() {
    let mode = fs::read_to_string("mode").unwrap();
    let output = PathBuf::from(fs::read_to_string("output-dir").unwrap());
    let mut in_job = 0;
    // SAFETY: current-process pseudo handle and writable Boolean output.
    assert_ne!(
        unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) },
        0
    );
    assert_ne!(in_job, 0, "the fixture must start inside the Job Object");
    match mode.as_str() {
        "environment" => {
            for key in ["OTTRIN_NATIVE_JOB_SECRET", "PYTHONHOME", "HTTP_PROXY"] {
                if std::env::var_os(key).is_some() {
                    fs::write(output.join("environment-leak"), key).unwrap();
                    return;
                }
            }
            let profile = std::env::current_dir().unwrap();
            for key in [
                "TEMP",
                "TMP",
                "HOME",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
            ] {
                assert_eq!(PathBuf::from(std::env::var_os(key).unwrap()), profile);
            }
            assert!(
                !std::env::var_os("PATH")
                    .unwrap()
                    .to_string_lossy()
                    .contains("untrusted-bin")
            );
            assert!(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).is_dir());
            fs::write(output.join("environment-restricted"), b"yes").unwrap();
        }
        "normal" => {
            println!("native stdout marker");
            eprintln!("native stderr marker");
            fs::write(output.join("result"), b"fixture output").unwrap();
        }
        "descendant" | "cancel" | "process-limit" => {
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "fixture_descendant", "--ignored", "--nocapture"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if mode == "process-limit" {
                assert!(
                    child.is_err(),
                    "active-process limit must reject the descendant"
                );
                fs::write(output.join("limit-enforced"), b"process").unwrap();
            } else {
                let child = child.unwrap();
                fs::write("descendant.pid", child.id().to_string()).unwrap();
                fs::write("parent.pid", std::process::id().to_string()).unwrap();
                // Deliberately leave the descendant alive when the parent exits.
                if mode == "cancel" {
                    std::thread::sleep(Duration::from_secs(60));
                }
                // Exit the fixture parent explicitly: waiting here would hide
                // the orphan-descendant lifetime that this test must exercise.
                std::process::exit(0);
            }
        }
        "within-memory-limit" | "aggregate-memory" => {
            let mut allocation = Vec::<u8>::new();
            allocation.try_reserve_exact(32 * 1024 * 1024).unwrap();
            if mode == "aggregate-memory" {
                let status = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "fixture_descendant", "--ignored", "--nocapture"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap();
                assert!(
                    status.success(),
                    "aggregate-memory child must report its allocation result"
                );
            } else {
                fs::write(output.join("within-memory-limit"), b"allowed").unwrap();
            }
            std::hint::black_box(&allocation);
        }
        "memory-limit" => {
            let mut allocation = Vec::<u8>::new();
            assert!(
                allocation.try_reserve_exact(128 * 1024 * 1024).is_err(),
                "process memory limit must reject allocation"
            );
            fs::write(output.join("limit-enforced"), b"memory").unwrap();
        }
        "inheritance" => {
            let raw: usize = fs::read_to_string("unrelated-handle")
                .unwrap()
                .parse()
                .unwrap();
            let mut flags = 0;
            // SAFETY: GetHandleInformation validates the supplied handle value;
            // it neither dereferences it in Rust nor transfers ownership.
            assert_eq!(
                unsafe { GetHandleInformation(raw as HANDLE, &mut flags) },
                0,
                "unrelated inheritable parent handle must not reach the provider"
            );
            fs::write(output.join("handle-restricted"), b"yes").unwrap();
        }
        _ => panic!("unknown fixture mode"),
    }
}

#[test]
#[ignore = "subprocess fixture selected explicitly by fixture_root"]
fn fixture_descendant() {
    if fs::read_to_string("mode").unwrap() == "aggregate-memory" {
        let mut allocation = Vec::<u8>::new();
        assert!(
            allocation.try_reserve_exact(32 * 1024 * 1024).is_err(),
            "the combined parent/child allocation must exceed the job limit"
        );
        let output = PathBuf::from(fs::read_to_string("output-dir").unwrap());
        fs::write(output.join("aggregate-memory-limit"), b"denied").unwrap();
    } else {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[test]
fn native_job_captures_output_and_cleans_diagnostics() {
    let f = Fixture::new("normal");
    let output = WindowsJobObjectIsolation
        .run(&f.request, &AtomicBool::new(false))
        .unwrap();
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("native stdout marker")
    );
    assert!(
        String::from_utf8(output.diagnostics)
            .unwrap()
            .contains("native stderr marker")
    );
    assert_eq!(
        fs::read(f.request.private_output_dir.join("result")).unwrap(),
        b"fixture output"
    );
    f.assert_logs_removed();
}

#[test]
fn native_job_enforces_output_budget() {
    let mut f = Fixture::new("normal");
    f.request.max_output_bytes = 1;
    assert_eq!(
        WindowsJobObjectIsolation.run(&f.request, &AtomicBool::new(false)),
        Err(IsolatedProcessFailure::LimitExceeded)
    );
    f.assert_logs_removed();
}

struct ProcessHandle(HANDLE);
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: uniquely owned handle returned by OpenProcess.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn wait_for_process_file(path: &Path) -> ProcessHandle {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(pid) = text.parse::<u32>()
        {
            // SAFETY: only requesting synchronization on the fixture PID.
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!handle.is_null(), "fixture process must still exist");
            return ProcessHandle(handle);
        }
        assert!(
            Instant::now() < deadline,
            "fixture did not publish its PID: {path:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_terminated(process: &ProcessHandle) {
    // SAFETY: the retained process handle prevents a PID-reuse false positive.
    assert_eq!(
        unsafe { WaitForSingleObject(process.0, 5_000) },
        WAIT_OBJECT_0,
        "launcher return must not leave the fixture process alive"
    );
}

#[test]
fn native_job_timeout_kills_descendant_after_parent_exit() {
    let mut f = Fixture::new("descendant");
    f.request.timeout = Duration::from_secs(6);
    std::thread::scope(|scope| {
        let run =
            scope.spawn(|| WindowsJobObjectIsolation.run(&f.request, &AtomicBool::new(false)));
        let descendant =
            wait_for_process_file(&f.request.private_profile_dir.join("descendant.pid"));
        let parent = wait_for_process_file(&f.request.private_profile_dir.join("parent.pid"));
        assert_terminated(&parent);
        // SAFETY: retained fixture handle, queried without waiting.
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.0, 0) },
            WAIT_TIMEOUT
        );
        assert_eq!(run.join().unwrap(), Err(IsolatedProcessFailure::Timeout));
        assert_terminated(&descendant);
    });
    f.assert_logs_removed();
}

#[test]
fn native_job_cancellation_kills_parent_and_descendant() {
    let f = Fixture::new("cancel");
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let run = scope.spawn(|| WindowsJobObjectIsolation.run(&f.request, &cancelled));
        let descendant =
            wait_for_process_file(&f.request.private_profile_dir.join("descendant.pid"));
        let parent = wait_for_process_file(&f.request.private_profile_dir.join("parent.pid"));
        cancelled.store(true, Ordering::Release);
        assert_eq!(run.join().unwrap(), Err(IsolatedProcessFailure::Cancelled));
        assert_terminated(&parent);
        assert_terminated(&descendant);
    });
    f.assert_logs_removed();
}

#[test]
fn native_job_enforces_memory_and_process_limits() {
    for (mode, marker) in [
        ("memory-limit", b"memory".as_slice()),
        ("process-limit", b"process".as_slice()),
    ] {
        let mut f = Fixture::new(mode);
        if mode == "process-limit" {
            f.request.max_processes = 1;
        }
        let result = WindowsJobObjectIsolation.run(&f.request, &AtomicBool::new(false));
        assert!(
            result.is_ok() || result == Err(IsolatedProcessFailure::LimitExceeded),
            "{result:?}"
        );
        assert_eq!(
            fs::read(f.request.private_output_dir.join("limit-enforced")).unwrap(),
            marker
        );
        f.assert_logs_removed();
    }
}

#[test]
fn native_job_enforces_aggregate_memory_across_descendants() {
    // The same allocation succeeds alone, then fails in the child when the
    // parent's live allocation brings the job total over its 64 MiB budget.
    for (mode, file, expected) in [
        (
            "within-memory-limit",
            "within-memory-limit",
            b"allowed".as_slice(),
        ),
        (
            "aggregate-memory",
            "aggregate-memory-limit",
            b"denied".as_slice(),
        ),
    ] {
        let f = Fixture::new(mode);
        let result = WindowsJobObjectIsolation.run(&f.request, &AtomicBool::new(false));
        assert!(
            result.is_ok() || result == Err(IsolatedProcessFailure::LimitExceeded),
            "{result:?}"
        );
        assert_eq!(
            fs::read(f.request.private_output_dir.join(file)).unwrap(),
            expected
        );
        f.assert_logs_removed();
    }
}

#[test]
fn native_job_restricts_inherited_handles() {
    let f = Fixture::new("inheritance");
    // Keep enough live handles that the probe cannot coincide with the few
    // unrelated handles allocated during the child test harness startup.
    let files: Vec<_> = (0..256)
        .map(|_| fs::File::open(&f.request.executable).unwrap())
        .collect();
    let handle = files.last().unwrap().as_raw_handle();
    // SAFETY: the File remains alive until after the child completes.
    assert_ne!(
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) },
        0
    );
    fs::write(
        f.request.private_profile_dir.join("unrelated-handle"),
        (handle as usize).to_string(),
    )
    .unwrap();
    WindowsJobObjectIsolation
        .run(&f.request, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        fs::read(f.request.private_output_dir.join("handle-restricted")).unwrap(),
        b"yes"
    );
    f.assert_logs_removed();
}

#[test]
fn native_job_does_not_inherit_parent_environment() {
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fixture_environment_parent",
            "--ignored",
            "--nocapture",
        ])
        .env("OTTRIN_NATIVE_JOB_SECRET", "fixture-secret")
        .env("PYTHONHOME", "fixture-runtime-override")
        .env("HTTP_PROXY", "http://fixture.invalid")
        .env("PATH", r"C:\OttrinQA-20261010\untrusted-bin")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "environment fixture failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "subprocess fixture with a deliberately contaminated parent environment"]
fn fixture_environment_parent() {
    assert_eq!(
        std::env::var("OTTRIN_NATIVE_JOB_SECRET").unwrap(),
        "fixture-secret"
    );
    let mut f = Fixture::new("environment");
    let unicode_profile = f.root.join("private profile å");
    fs::rename(&f.request.private_profile_dir, &unicode_profile).unwrap();
    f.request.private_profile_dir = unicode_profile;
    WindowsJobObjectIsolation
        .run(&f.request, &AtomicBool::new(false))
        .unwrap();
    assert!(
        !f.request
            .private_output_dir
            .join("environment-leak")
            .exists(),
        "fake parent variable reached the renderer: {}",
        fs::read_to_string(f.request.private_output_dir.join("environment-leak"))
            .unwrap_or_default()
    );
    assert_eq!(
        fs::read(f.request.private_output_dir.join("environment-restricted")).unwrap(),
        b"yes"
    );
    f.assert_logs_removed();
}
