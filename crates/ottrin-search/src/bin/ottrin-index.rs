use ottrin_core::SearchIndexStatus;
use ottrin_core::{SearchConfig, default_home_dir};
use ottrin_search::SearchService;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let mut force_rebuild = false;
    let mut remove_schedule = false;
    let mut roots: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter_map(|a| {
            if a == "--rebuild" || a == "--force" {
                force_rebuild = true;
                None
            } else if a == "--remove-schedule" || a == "--uninstall-schedule" {
                remove_schedule = true;
                None
            } else if a == "--help" || a == "-h" {
                println!("Usage: ottrin-index [--rebuild] [--remove-schedule] [ROOT ...]");
                println!("Builds or refreshes Ottrin's SQLite search index.");
                println!("  --rebuild   Force a full rescan even if a saved index exists.");
                println!("  --remove-schedule");
                println!("              Remove Ottrin's background SQLite refresh schedule.");
                std::process::exit(0);
            } else if a.starts_with("--") {
                eprintln!("unknown option: {a}");
                std::process::exit(2);
            } else {
                Some(PathBuf::from(a))
            }
        })
        .collect();
    if remove_schedule {
        match SearchService::uninstall_sqlite_schedule() {
            Ok(msg) => {
                println!("{msg}");
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }
    if roots.is_empty() {
        roots.push(default_home_dir());
    }

    println!("roots:");
    for root in &roots {
        let exists = if root.exists() { "ok" } else { "missing" };
        println!("  {} ({})", root.display(), exists);
    }

    let config = SearchConfig {
        include_roots: roots,
        ..SearchConfig::default()
    };
    let mut service = SearchService::new(config);
    service.start();
    if matches!(service.diagnostics().status, SearchIndexStatus::Unavailable) {
        let diag = service.diagnostics();
        eprintln!(
            "ottrin-index: {}",
            diag.last_error
                .unwrap_or_else(|| "search service unavailable".to_string())
        );
        std::process::exit(1);
    }
    if force_rebuild && let Err(error) = service.rebuild_index_blocking() {
        eprintln!("ottrin-index: rebuild failed: {error}");
        std::process::exit(1);
    }

    let mut last_line = String::new();
    loop {
        let diag = service.diagnostics();
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let progress_age = diag
            .last_progress_unix_secs
            .map(|t| now_secs.saturating_sub(t));
        let line = format!(
            "status={:?} indexed={} detail={} root={} ({}/{}) configured={} watched={} db_bytes={} wal_bytes={} freelist_pages={} freelist_bytes={} last_progress={}",
            diag.status,
            diag.indexed_items,
            diag.detail.clone().unwrap_or_else(|| "-".to_string()),
            diag.active_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "-".to_string()),
            diag.active_root_index,
            diag.total_roots,
            diag.configured_root_count,
            diag.watched_root_count,
            diag.database_bytes,
            diag.wal_bytes,
            diag.freelist_pages,
            diag.freelist_bytes,
            progress_age
                .map(|a| format!("{}s", a))
                .unwrap_or_else(|| "-".to_string()),
        );
        if line != last_line {
            println!("{line}");
            last_line = line;
        }

        match diag.status {
            SearchIndexStatus::Ready => break,
            SearchIndexStatus::Unavailable => {
                eprintln!(
                    "ottrin-index: {}",
                    diag.last_error
                        .unwrap_or_else(|| "search service unavailable".to_string())
                );
                std::process::exit(1);
            }
            SearchIndexStatus::Loading | SearchIndexStatus::Indexing => {}
        }
        sleep(Duration::from_millis(500));
    }
}
