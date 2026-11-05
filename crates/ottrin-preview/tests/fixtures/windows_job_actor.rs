//! Small native process actor for the Windows preview isolation tests.
//! This executable is a QA fixture and is never staged in Ottrin packages.
#[cfg(target_os = "windows")]
#[path = "windows_job_common.rs"]
mod common;

#[cfg(target_os = "windows")]
mod actor {
    use super::common::{current_process_has_handle, pipe_creation_error};
    use std::{
        fs,
        path::PathBuf,
        process::{Command, Stdio},
        time::Duration,
    };
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::{
        JobObjects::IsProcessInJob,
        Threading::{
            GetCurrentProcess, GetProcessMitigationPolicy, ProcessStrictHandleCheckPolicy,
        },
    };

    pub fn run() {
        match std::env::args().nth(1).as_deref() {
            Some("root") => fixture_root(),
            Some("descendant") => fixture_descendant(),
            _ => panic!("Expected an explicit native QA actor mode"),
        }
    }

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
        let mut strict_handles = 0_u32;
        // SAFETY: the documented strict-handle policy is a single DWORD. This is a
        // read-only diagnostic; the fixture never changes process mitigations.
        let mitigation_read = unsafe {
            GetProcessMitigationPolicy(
                GetCurrentProcess(),
                ProcessStrictHandleCheckPolicy,
                (&mut strict_handles as *mut u32).cast(),
                std::mem::size_of::<u32>(),
            )
        };
        eprintln!(
            "Native fixture {mode}: strict_handle_query={mitigation_read}, strict_handles={strict_handles:#x}, NUL_read={:?}, NUL_write={:?}, executable_read={:?}",
            fs::File::open("NUL").map(|_| ()),
            fs::OpenOptions::new().write(true).open("NUL").map(|_| ()),
            fs::File::open(std::env::current_exe().unwrap()).map(|_| ()),
        );
        match mode.as_str() {
            "ipc" => {
                let errors = [pipe_creation_error(""), pipe_creation_error("LOCAL\\")];
                fs::write(output.join("ipc-errors"), format!("{errors:?}")).unwrap();
            }
            "containment" => {
                let outside = PathBuf::from(fs::read_to_string("outside-file").unwrap());
                let input = PathBuf::from(std::env::args().next_back().unwrap());
                let address: std::net::SocketAddr =
                    fs::read_to_string("listener").unwrap().parse().unwrap();
                let flags = [
                    fs::read(&outside).is_ok(),
                    fs::OpenOptions::new().write(true).open(&input).is_ok(),
                    std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok(),
                ];
                fs::write(output.join("access-results"), flags.map(u8::from)).unwrap();
                assert_eq!(fs::read(&input).unwrap(), b"admitted fixture");
            }
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
                    let path = PathBuf::from(std::env::var_os(key).unwrap());
                    assert!(path.starts_with(&profile), "{key} escaped private profile");
                    assert!(path.is_dir(), "{key} does not name a usable directory");
                    let probe = path.join(format!("environment-write-{key}"));
                    fs::write(&probe, b"private environment fixture").unwrap();
                    fs::remove_file(probe).unwrap();
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
                    .arg("descendant")
                    // NUL is not an admitted file inside every AppContainer.
                    // Reuse the broker's already admitted, bounded standard I/O.
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
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
                        .arg("descendant")
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
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
                // Query the valid handle table, not an invalid handle: Windows'
                // permanent strict-handle policy raises an exception for the latter.
                assert!(
                    !current_process_has_handle(raw as HANDLE),
                    "unrelated inheritable parent handle must not reach the provider"
                );
                fs::write(output.join("handle-restricted"), b"yes").unwrap();
            }
            _ => panic!("unknown fixture mode"),
        }
    }

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
}
#[cfg(target_os = "windows")]
fn main() {
    actor::run();
}
#[cfg(not(target_os = "windows"))]
fn main() {}
