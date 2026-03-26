use ottrin_core::{default_home_dir, SearchConfig};
use ottrin_core::SearchIndexStatus;
use ottrin_search::SearchService;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() {
    let mut roots: Vec<PathBuf> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    if roots.is_empty() {
        roots.push(default_home_dir());
    }

    println!("roots:");
    for root in &roots {
        let exists = if root.exists() { "ok" } else { "missing" };
        println!("  {} ({})", root.display(), exists);
    }

    let mut config = SearchConfig::default();
    config.include_roots = roots;
    let service = SearchService::new(config);
    service.start();

    let mut last_line = String::new();
    loop {
        let diag = service.diagnostics();
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let progress_age = diag.last_progress_unix_secs.map(|t| now_secs.saturating_sub(t));
        let line = format!(
            "status={:?} indexed={} detail={} root={} ({}/{}) configured={} watched={} last_progress={}",
            diag.status,
            diag.indexed_items,
            diag.detail.clone().unwrap_or_else(|| "-".to_string()),
            diag.active_root.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "-".to_string()),
            diag.active_root_index,
            diag.total_roots,
            diag.configured_root_count,
            diag.watched_root_count,
            progress_age.map(|a| format!("{}s", a)).unwrap_or_else(|| "-".to_string()),
        );
        if line != last_line {
            println!("{line}");
            last_line = line;
        }

        match diag.status {
            SearchIndexStatus::Ready | SearchIndexStatus::Unavailable => break,
            SearchIndexStatus::Indexing => {}
        }
        sleep(Duration::from_millis(500));
    }
}
