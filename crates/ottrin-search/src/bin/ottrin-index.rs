use ottrin_core::SearchIndexStatus;
use ottrin_core::{SearchConfig, default_home_dir};
use ottrin_search::SearchService;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    if let Err(err) = run() {
        eprintln!("ottrin-index: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    fn print_status(config: &SearchConfig) {
        let diagnostics = SearchService::persisted_diagnostics(config);
        println!("status={:?}", diagnostics.status);
        println!("indexed_items={}", diagnostics.indexed_items);
        println!("snapshot_revision={}", diagnostics.snapshot_revision);
        println!("configured_roots={}", diagnostics.configured_root_count);
        println!("watched_roots={}", diagnostics.watched_root_count);
        if let Some(detail) = &diagnostics.detail {
            println!("detail={detail}");
        }
        if let Some(error) = &diagnostics.last_error {
            println!("error={error}");
        }
    }

    let mut force_rebuild = false;
    let mut print_status_only = false;
    let mut remove_schedule = false;
    let mut roots: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter_map(|a| {
            if a == "--rebuild" || a == "--force" {
                force_rebuild = true;
                None
            } else if a == "--status" {
                print_status_only = true;
                None
            } else if a == "--remove-schedule" || a == "--uninstall-schedule" {
                remove_schedule = true;
                None
            } else if a == "--help" || a == "-h" {
                println!(
                    "Usage: ottrin-index [--status] [--rebuild] [--remove-schedule] [ROOT ...]"
                );
                println!("Builds or refreshes Ottrin's SQLite search index.");
                println!("  --status    Print persisted search status and exit.");
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

    if print_status_only {
        let config = SearchConfig {
            include_roots: if roots.is_empty() {
                vec![default_home_dir()]
            } else {
                roots
            },
            ..SearchConfig::default()
        };
        print_status(&config);
        return Ok(());
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
        let unavailable_error = diag
            .last_error
            .clone()
            .unwrap_or_else(|| "search service unavailable".to_string());
        eprintln!("ottrin-index: {}", unavailable_error.clone());
        return Err(unavailable_error);
    }
    if force_rebuild && let Err(error) = service.rebuild_index_blocking() {
        return Err(format!("rebuild failed: {error}"));
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
            "status={:?} indexed={} detail={} root={} ({}/{}) configured={} watched={} last_progress={}",
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
                return Err(diag
                    .last_error
                    .unwrap_or_else(|| "search service unavailable".to_string()));
            }
            SearchIndexStatus::Stale => {
                return Err(diag
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "search index status is stale".to_string()));
            }
            SearchIndexStatus::NeedsRebuild => {
                return Err(diag
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "search index requires rebuild".to_string()));
            }
            SearchIndexStatus::Indexing => {}
        }
        sleep(Duration::from_millis(500));
    }
    Ok(())
}
