#![cfg(target_os = "windows")]

use ottrin_core::{SearchConfig, SearchQuery, SearchScope, SearchSort};
use ottrin_search::SearchDaemonClient;
use ottrin_search::{SearchService, run_windows_search_daemon};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
struct Fixture {
    root: PathBuf,
    client: SearchDaemonClient,
    server: Option<std::thread::JoinHandle<Result<(), String>>>,
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
        let config = SearchConfig {
            include_roots: vec![root.join("files")],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        };
        let store = root.join("index.db");
        let server = std::thread::spawn(move || run_windows_search_daemon(config, store));
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
        self.client.shutdown().unwrap();
        self.server.take().unwrap().join().unwrap().unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = self.client.shutdown();
            let _ = server.join();
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn windows_persistent_daemon_serves_commands_watches_changes_and_releases_store() {
    let mut fixture = Fixture::start();
    assert_eq!(fixture.client.server_pid().unwrap(), std::process::id());
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
