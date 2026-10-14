#![cfg(target_os = "windows")]

use ottrin_core::{SearchConfig, SearchQuery, SearchScope, SearchSort};
use ottrin_search::SearchDaemonClient;
use ottrin_search::{SearchService, run_windows_search_daemon};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
struct Fixture {
    root: PathBuf,
    client: SearchDaemonClient,
    server: Option<Child>,
}
impl Fixture {
    fn start() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ottrin-windows-daemon-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("files")).unwrap();
        fs::write(root.join("files/original.txt"), "original content").unwrap();
        let server = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "windows_daemon_actor",
                "--ignored",
                "--nocapture",
            ])
            .env("OTTRIN_DAEMON_TEST_ROOT", &root)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let fixture = Self {
            root,
            client: SearchDaemonClient::connect_default().unwrap(),
            server: Some(server),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while fixture.client.ping().is_err() {
            assert!(Instant::now() < deadline, "Daemon did not become ready");
            std::thread::sleep(Duration::from_millis(20));
        }
        fixture
    }
    fn query(&self, text: &str) -> ottrin_search::SearchQueryResult {
        self.client
            .query(SearchQuery {
                text: text.into(),
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: false,
                sort: SearchSort::Relevance,
                limit: 20,
                offset: 0,
                content_search: false,
            })
            .unwrap()
    }
    fn shutdown(&mut self) {
        self.client.prepare_upgrade_and_wait().unwrap();
        assert!(self.server.take().unwrap().wait().unwrap().success());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = self.client.shutdown();
            let mut server = server;
            let deadline = Instant::now() + Duration::from_secs(5);
            while server.try_wait().unwrap().is_none() {
                if Instant::now() >= deadline {
                    server.kill().unwrap();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = server.wait();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn windows_persistent_daemon_serves_commands_watches_changes_and_releases_store() {
    let mut fixture = Fixture::start();
    assert_eq!(
        fixture.client.server_pid().unwrap(),
        fixture.server.as_ref().unwrap().id()
    );
    assert_eq!(
        fixture.client.server_executable().unwrap(),
        std::env::current_exe().unwrap()
    );
    fixture.client.reconcile().unwrap();
    assert!(!fixture.query("original.txt").response.items.is_empty());
    fs::write(fixture.root.join("files/watched.txt"), "watched content").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while fixture.query("watched.txt").response.items.is_empty() {
        assert!(
            Instant::now() < deadline,
            "Filesystem watch did not publish change"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    fixture.client.rebuild().unwrap();
    assert!(fixture.client.diagnostics().unwrap().last_error.is_none());
    let alternate = fixture.root.join("alternate");
    fs::create_dir_all(&alternate).unwrap();
    fs::write(alternate.join("alternate.txt"), "alternate content").unwrap();
    fixture
        .client
        .update_config(SearchConfig {
            include_roots: vec![alternate],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        })
        .unwrap();
    fixture.client.reconcile().unwrap();
    assert!(!fixture.query("alternate.txt").response.items.is_empty());
    assert!(fixture.client.clear_storage().unwrap() > 0);
    assert!(
        fixture
            .client
            .rebuild()
            .unwrap_err()
            .contains("not running")
    );
    fixture.shutdown();
    assert!(fixture.client.ping().is_err());
    let mut reopened =
        SearchService::new_with_store_path(SearchConfig::default(), fixture.root.join("index.db"));
    reopened.start();
    assert!(
        reopened.diagnostics().last_error.is_none(),
        "Daemon left its writer lock held"
    );
    reopened.stop();
}

#[test]
#[ignore = "daemon subprocess launched only by the owning integration fixture"]
fn windows_daemon_actor() {
    let root =
        PathBuf::from(std::env::var_os("OTTRIN_DAEMON_TEST_ROOT").expect("owned fixture path"));
    let config = SearchConfig {
        include_roots: vec![root.join("files")],
        use_system_locate: false,
        manage_locate_db: false,
        refresh_interval_hours: 0,
        ..SearchConfig::default()
    };
    run_windows_search_daemon(config, root.join("index.db")).unwrap();
}

#[test]
#[ignore = "requires an explicit private profile and current standalone indexd executable"]
fn windows_standalone_daemon_cold_start_reuse_and_restart() {
    let executable =
        PathBuf::from(std::env::var_os("OTTRIN_QA_INDEXD").expect("owned standalone executable"));
    let profile =
        PathBuf::from(std::env::var_os("OTTRIN_QA_PROFILE").expect("owned private profile"));
    assert_eq!(
        std::env::var_os("APPDATA").map(PathBuf::from),
        Some(profile.clone())
    );
    assert_eq!(
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        Some(profile.join("cache"))
    );
    assert!(
        profile
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("ottrin-standalone-qa-")
    );
    fs::create_dir_all(profile.join("files")).unwrap();
    for number in 0..1000 {
        fs::write(
            profile
                .join("files")
                .join(format!("idle-fixture-{number}.txt")),
            "quiet file",
        )
        .unwrap();
    }
    fs::create_dir_all(profile.join("ottrin")).unwrap();
    fs::write(
        profile.join("files/standalone-fixture.txt"),
        "standalone fixture",
    )
    .unwrap();
    let mut config = ottrin_core::AppConfig::default();
    config.search.include_roots = vec![profile.join("files")];
    config.search.refresh_interval_hours = 0;
    config.search.use_system_locate = false;
    config.search.manage_locate_db = false;
    fs::write(
        profile.join("ottrin/config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let client = ottrin_search::ensure_windows_search_daemon(&executable).unwrap();
    struct Cleanup(SearchDaemonClient, PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.prepare_upgrade_and_wait();
            fs::remove_dir_all(&self.1).unwrap();
        }
    }
    let _cleanup = Cleanup(client.clone(), profile);
    let pid = client.server_pid().unwrap();
    assert_ne!(pid, std::process::id());
    assert_eq!(client.server_executable().unwrap(), executable);
    let reused = ottrin_search::ensure_windows_search_daemon(&executable).unwrap();
    assert_eq!(
        reused.server_pid().unwrap(),
        pid,
        "Second startup replaced a current daemon"
    );
    client.reconcile().unwrap();
    let query = SearchQuery {
        text: "standalone-fixture.txt".into(),
        scope: SearchScope::Global,
        root_path: None,
        include_hidden_system: false,
        sort: SearchSort::Relevance,
        limit: 20,
        offset: 0,
        content_search: false,
    };
    assert_eq!(client.query(query.clone()).unwrap().response.items.len(), 1);
    drop(reused);
    client.ping().unwrap();
    assert!(client.endpoint_present().unwrap());
    SearchService::start_sqlite_schedule().unwrap();
    let running = client.server_pid().unwrap();
    assert_ne!(
        running, pid,
        "Explicit session restart must replace its process"
    );
    client.reconcile().unwrap();
    let diagnostics = client.diagnostics().unwrap();
    assert_eq!(diagnostics.indexed_items, 1001);
    assert!(diagnostics.watcher_ready);
    measure_quiet_daemon_cpu(running);
    SearchService::stop_sqlite_schedule().unwrap();
    assert!(!client.endpoint_present().unwrap());
    let restarted = ottrin_search::ensure_windows_search_daemon(&executable).unwrap();
    assert_ne!(restarted.server_pid().unwrap(), pid);
    restarted.reconcile().unwrap();
    assert_eq!(restarted.query(query).unwrap().response.items.len(), 1);
    restarted.prepare_upgrade_and_wait().unwrap();
    assert!(client.ping().is_err());
}

fn measure_quiet_daemon_cpu(pid: u32) {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: no pointers, read-only process query access, non-inheritable handle.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    assert!(!raw.is_null(), "Cannot query owned daemon process");
    // SAFETY: successful OpenProcess transfers ownership; handle is closed once.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let cpu_ticks = || {
        let blank = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (blank, blank, blank, blank);
        // SAFETY: process stays live, all output pointers target writable FILETIME values.
        assert_ne!(
            unsafe {
                GetProcessTimes(
                    process.as_raw_handle(),
                    &mut created,
                    &mut exited,
                    &mut kernel,
                    &mut user,
                )
            },
            0
        );
        let ticks =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        ticks(kernel) + ticks(user)
    };
    std::thread::sleep(Duration::from_millis(200));
    let before = cpu_ticks();
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(12));
    let elapsed = start.elapsed().as_secs_f64();
    let cpu_seconds = (cpu_ticks() - before) as f64 / 10_000_000.0;
    let percent_of_one_core = cpu_seconds / elapsed * 100.0;
    println!(
        "Quiet populated index: {cpu_seconds:.6} CPU seconds / {elapsed:.3} wall seconds = {percent_of_one_core:.4}% of one core"
    );
    assert!(
        percent_of_one_core < 1.0,
        "Quiet daemon consumed noticeable CPU: {percent_of_one_core}%"
    );
}
