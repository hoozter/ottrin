#![cfg(unix)]

use ottrin_core::AppConfig;
#[cfg(target_os = "linux")]
use ottrin_search::SearchDaemonClient;
use serde_json::Value;
use std::fs;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct FixtureDaemon(Child);

impl Drop for FixtureDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct FixtureRoot(std::path::PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(target_os = "linux")]
struct SocketDaemon(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for SocketDaemon {
    fn drop(&mut self) {
        let _ = SearchDaemonClient::new(self.0.clone()).shutdown();
    }
}

fn run_app(config_home: &Path, cache_home: &Path, runtime_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ottrin-app"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_CACHE_HOME", cache_home)
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .output()
        .expect("run Ottrin CLI")
}

#[test]
fn persisted_no_match_cli_query_does_not_scan_fuzzy_candidates() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    // Keep the fixture root short because the daemon appends a Unix socket path,
    // and CI may already provide a relatively long isolated TMPDIR.
    let root = std::env::temp_dir().join(format!("ops-{:x}-{unique:x}", std::process::id()));
    let _fixture = FixtureRoot(root.clone());
    let config_home = root.join("config");
    let cache_home = root.join("cache");
    let runtime_dir = root.join("runtime");
    let files = root.join("files");
    fs::create_dir_all(config_home.join("ottrin")).expect("create config directory");
    fs::create_dir_all(&cache_home).expect("create cache directory");
    fs::create_dir_all(&runtime_dir).expect("create runtime directory");
    fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
        .expect("make runtime directory private");
    fs::create_dir_all(&files).expect("create indexed files directory");

    let mut config = AppConfig::default();
    config.search.include_roots = vec![files.clone()];
    config.search.exclude_roots.clear();
    config.search.include_globs.clear();
    config.search.exclude_globs.clear();
    config.search.content_indexing_enabled = false;
    config.search.refresh_interval_hours = 0;
    config.search.use_system_locate = false;
    config.search.manage_locate_db = false;
    config.search.result_limit = 50;
    fs::write(
        config_home.join("ottrin/config.json"),
        serde_json::to_vec(&config).expect("serialize fixture config"),
    )
    .expect("write fixture config");

    for index in 0..10_000 {
        // These are fuzzy subsequence matches for "needle" only. An exact persisted
        // page must return no rows without scanning them for optional fuzzy results.
        fs::write(files.join(format!("n-{index:05}-e-e-d-l-e.txt")), [])
            .expect("write fixture file");
    }

    let daemon = Command::new(env!("CARGO_BIN_EXE_ottrin-indexd"))
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start fixture index daemon");
    let _daemon = FixtureDaemon(daemon);

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = run_app(
            &config_home,
            &cache_home,
            &runtime_dir,
            &["index", "status"],
        );
        if status.status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "fixture daemon did not become ready"
        );
        thread::sleep(Duration::from_millis(25));
    }

    let rebuild = run_app(
        &config_home,
        &cache_home,
        &runtime_dir,
        &["index", "rebuild", "--json"],
    );
    assert!(
        rebuild.status.success(),
        "rebuild failed: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );

    let query = run_app(
        &config_home,
        &cache_home,
        &runtime_dir,
        &["search", "needle", "--json"],
    );
    assert!(
        query.status.success(),
        "query failed: {}",
        String::from_utf8_lossy(&query.stderr)
    );
    let response: Value = serde_json::from_slice(&query.stdout).expect("parse search response");
    assert_eq!(response["total"], 0);
    assert_eq!(response["items"], Value::Array(Vec::new()));
    assert!(response["error"].is_null());
}

#[cfg(target_os = "linux")]
#[test]
fn stale_on_demand_daemon_is_replaced_before_a_rebuild() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    // Keep the fixture root short because the daemon appends a Unix socket path,
    // and CI may already provide a relatively long isolated TMPDIR.
    let root = std::env::temp_dir().join(format!("ops-{:x}-{unique:x}", std::process::id()));
    let _fixture = FixtureRoot(root.clone());
    let config_home = root.join("config");
    let cache_home = root.join("cache");
    let runtime_dir = root.join("runtime");
    let files = root.join("files");
    fs::create_dir_all(config_home.join("ottrin")).expect("create config directory");
    fs::create_dir_all(&cache_home).expect("create cache directory");
    fs::create_dir_all(&runtime_dir).expect("create runtime directory");
    fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700))
        .expect("make runtime directory private");
    fs::create_dir_all(&files).expect("create indexed files directory");
    fs::write(files.join("ready.txt"), []).expect("write fixture file");

    let mut config = AppConfig::default();
    config.search.include_roots = vec![files];
    config.search.exclude_roots.clear();
    config.search.include_globs.clear();
    config.search.exclude_globs.clear();
    config.search.content_indexing_enabled = false;
    config.search.refresh_interval_hours = 0;
    config.search.use_system_locate = false;
    config.search.manage_locate_db = false;
    fs::write(
        config_home.join("ottrin/config.json"),
        serde_json::to_vec(&config).expect("serialize fixture config"),
    )
    .expect("write fixture config");

    let current_daemon = Path::new(env!("CARGO_BIN_EXE_ottrin-indexd"));
    let stale_daemon = root.join("stale-ottrin-indexd");
    fs::copy(current_daemon, &stale_daemon).expect("copy stale daemon fixture");
    fs::set_permissions(&stale_daemon, fs::Permissions::from_mode(0o700))
        .expect("make stale daemon executable");
    let daemon = Command::new(&stale_daemon)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", &cache_home)
        .env("XDG_RUNTIME_DIR", &runtime_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start stale daemon");
    let stale_pid = daemon.id();
    let mut stale_daemon = FixtureDaemon(daemon);
    let socket = runtime_dir.join("ottrin/index-v1.sock");
    let _replacement = SocketDaemon(socket.clone());

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = run_app(
            &config_home,
            &cache_home,
            &runtime_dir,
            &["index", "status"],
        );
        if status.status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stale daemon did not become ready"
        );
        thread::sleep(Duration::from_millis(25));
    }

    let rebuild = run_app(
        &config_home,
        &cache_home,
        &runtime_dir,
        &["index", "rebuild", "--json"],
    );
    assert!(
        rebuild.status.success(),
        "rebuild through replacement failed: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    let client = SearchDaemonClient::new(socket);
    let replacement_pid = client.server_pid().expect("read replacement daemon pid");
    assert_ne!(replacement_pid, stale_pid, "stale daemon remained active");
    assert!(
        stale_daemon
            .0
            .try_wait()
            .expect("inspect stale daemon")
            .is_some(),
        "stale daemon did not exit for replacement"
    );
    let replacement = fs::metadata(format!("/proc/{replacement_pid}/exe"))
        .expect("inspect replacement executable");
    let expected = fs::metadata(current_daemon).expect("inspect current daemon executable");
    assert_eq!(
        (replacement.dev(), replacement.ino()),
        (expected.dev(), expected.ino()),
        "replacement must use the current dedicated daemon binary"
    );
    client.shutdown().expect("stop replacement daemon");
}
