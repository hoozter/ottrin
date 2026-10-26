use crate::mutation::OwnedItem;
use crate::plan::ScanPlan;
use ottrin_core::{EntryKind, SearchResultItem};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanProgress {
    pub(crate) message: String,
    pub(crate) active_root: PathBuf,
    pub(crate) active_root_index: usize,
    pub(crate) total_roots: usize,
    pub(crate) indexed_items: usize,
    pub(crate) last_indexed_path: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, allow(dead_code))]
pub(crate) enum ScanOutcome {
    Completed(Vec<OwnedItem>),
    Staged { indexed_items: usize },
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BatchedScanOutcome {
    Completed { indexed_items: usize },
    Cancelled,
}

fn walk_error_is_skippable(depth: usize, kind: Option<std::io::ErrorKind>) -> bool {
    depth > 0
        && matches!(
            kind,
            Some(std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound)
        )
}

#[cfg(test)]
pub(crate) fn scan(
    plan: &ScanPlan,
    cancelled: &AtomicBool,
    on_progress: impl FnMut(ScanProgress),
) -> Result<ScanOutcome, String> {
    let mut items = Vec::new();
    match scan_batched(
        plan,
        cancelled,
        usize::MAX,
        |batch| {
            items.extend(batch);
            Ok(())
        },
        on_progress,
    )? {
        BatchedScanOutcome::Completed { .. } => Ok(ScanOutcome::Completed(items)),
        BatchedScanOutcome::Cancelled => Ok(ScanOutcome::Cancelled),
    }
}

pub(crate) fn scan_batched(
    plan: &ScanPlan,
    cancelled: &AtomicBool,
    batch_size: usize,
    mut on_batch: impl FnMut(Vec<OwnedItem>) -> Result<(), String>,
    mut on_progress: impl FnMut(ScanProgress),
) -> Result<BatchedScanOutcome, String> {
    if batch_size == 0 {
        return Err("scan batch size must be greater than zero".to_string());
    }
    let roots = plan.roots();
    let total_roots = roots.len();
    let mut batch = Vec::with_capacity(batch_size.min(4096));
    let mut indexed_items = 0_usize;
    let placeholder = roots.first().cloned().unwrap_or_default();
    on_progress(ScanProgress {
        message: "Indexing · preparing filters…".to_string(),
        active_root: placeholder,
        active_root_index: 0,
        total_roots,
        indexed_items: 0,
        last_indexed_path: None,
    });
    if cancelled.load(Ordering::Acquire) {
        return Ok(BatchedScanOutcome::Cancelled);
    }

    for (root_index, root) in roots.iter().enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(BatchedScanOutcome::Cancelled);
        }
        if let Err(error) = std::fs::symlink_metadata(root) {
            if !plan.is_metadata_root(root) && walk_error_is_skippable(1, Some(error.kind())) {
                on_progress(ScanProgress {
                    message: format!("Content folder unavailable: {}: {error}", root.display()),
                    active_root: root.clone(),
                    active_root_index: root_index + 1,
                    total_roots,
                    indexed_items,
                    last_indexed_path: None,
                });
                continue;
            }
            return Err(format!(
                "configured root is unavailable: {}: {error}",
                root.display()
            ));
        }
        let walker = WalkDir::new(root)
            .follow_links(plan.follow_links())
            .same_file_system(plan.same_file_system())
            .into_iter()
            .filter_entry(|entry| plan.includes_path_for_root(entry.path(), root));
        let root_start_count = indexed_items;
        let mut reported_count = indexed_items;
        let mut last_path = None;
        let mut last_progress_at = Instant::now();
        for entry in walker {
            if cancelled.load(Ordering::Acquire) {
                return Ok(BatchedScanOutcome::Cancelled);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(error)
                    if walk_error_is_skippable(
                        if plan.is_metadata_root(root) {
                            error.depth()
                        } else {
                            error.depth().max(1)
                        },
                        error.io_error().map(std::io::Error::kind),
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            if entry.path() == root && !plan.indexes_metadata_for(root) {
                continue;
            }
            let Some(item) = candidate_for_path(plan, entry.path())? else {
                continue;
            };
            last_path = Some(item.item().path.clone());
            batch.push(item);
            indexed_items = indexed_items.saturating_add(1);
            if batch.len() == batch_size {
                on_batch(std::mem::replace(
                    &mut batch,
                    Vec::with_capacity(batch_size.min(4096)),
                ))?;
            }
            // Progress is display state, not one worker command per file. Keep
            // cancellation and durable batches independent of this UI cadence.
            if indexed_items == root_start_count + 1
                || last_progress_at.elapsed() >= Duration::from_millis(100)
            {
                on_progress(found_progress(
                    root,
                    root_index,
                    total_roots,
                    indexed_items,
                    last_path.clone(),
                ));
                reported_count = indexed_items;
                last_progress_at = Instant::now();
            }
        }
        if reported_count != indexed_items {
            on_progress(found_progress(
                root,
                root_index,
                total_roots,
                indexed_items,
                last_path,
            ));
        }
    }
    if cancelled.load(Ordering::Acquire) {
        return Ok(BatchedScanOutcome::Cancelled);
    }
    if !batch.is_empty() {
        on_batch(batch)?;
    }
    Ok(BatchedScanOutcome::Completed { indexed_items })
}

fn found_progress(
    root: &Path,
    root_index: usize,
    total_roots: usize,
    indexed_items: usize,
    last_indexed_path: Option<PathBuf>,
) -> ScanProgress {
    ScanProgress {
        message: crate::format_progress(
            root_index + 1,
            total_roots.max(1),
            indexed_items,
            indexed_items,
        ),
        active_root: root.to_path_buf(),
        active_root_index: root_index + 1,
        total_roots,
        indexed_items,
        last_indexed_path,
    }
}

/// Build a private candidate for one exact configured root.
pub(crate) fn scan_root(
    plan: &ScanPlan,
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<ScanOutcome, String> {
    if !plan.roots().iter().any(|candidate| candidate == root) {
        return Err(format!(
            "root is not part of the active scan plan: {}",
            root.display()
        ));
    }
    if let Err(error) = std::fs::symlink_metadata(root) {
        if !plan.is_metadata_root(root) && walk_error_is_skippable(1, Some(error.kind())) {
            return Ok(ScanOutcome::Completed(Vec::new()));
        }
        return Err(format!(
            "configured root is unavailable: {}: {error}",
            root.display()
        ));
    }
    let mut items = Vec::new();
    let walker = WalkDir::new(root)
        .follow_links(plan.follow_links())
        .same_file_system(plan.same_file_system())
        .into_iter()
        .filter_entry(|entry| plan.includes_path_for_root(entry.path(), root));
    for entry in walker {
        if cancelled.load(Ordering::Acquire) {
            return Ok(ScanOutcome::Cancelled);
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error)
                if walk_error_is_skippable(
                    if plan.is_metadata_root(root) {
                        error.depth()
                    } else {
                        error.depth().max(1)
                    },
                    error.io_error().map(std::io::Error::kind),
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if entry.path() == root && !plan.indexes_metadata_for(root) {
            continue;
        }
        if let Some(item) = candidate_for_path(plan, entry.path())? {
            items.push(item);
        }
    }
    Ok(ScanOutcome::Completed(items))
}

/// Classify one notify path with the same ownership and filter semantics as a full scan.
pub(crate) fn candidate_for_path(
    plan: &ScanPlan,
    path: &Path,
) -> Result<Option<OwnedItem>, String> {
    let Some(root) = plan.root_for(path) else {
        return Ok(None);
    };
    if (path == root && !plan.indexes_metadata_for(path))
        || !plan.includes_path_for_root(path, root)
    {
        return Ok(None);
    }
    let Some(name) = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return Ok(None);
    };
    if !plan.includes_item_name(&name) {
        return Ok(None);
    }
    let link_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let file_type = link_metadata.file_type();
    let metadata = if file_type.is_symlink() {
        std::fs::metadata(path).unwrap_or(link_metadata)
    } else {
        link_metadata
    };
    let kind = if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    if !(plan.indexes_metadata_for(path)
        || matches!(kind, EntryKind::Directory)
        || (matches!(kind, EntryKind::File) && plan.indexes_content_for(path)))
    {
        return Ok(None);
    }
    let mut item = SearchResultItem {
        path: path.to_path_buf(),
        parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        name,
        kind,
        size_bytes: matches!(kind, EntryKind::File).then(|| metadata.len()),
        modified_unix_secs: metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs()),
        is_executable: crate::detect_executable(&metadata),
        symlink_target_is_dir: matches!(kind, EntryKind::Symlink).then(|| metadata.is_dir()),
        content_snippet: None,
        name_lower: String::new(),
        path_str: String::new(),
        path_lower: String::new(),
    };
    item.prepare();
    Ok(Some(OwnedItem::new(root.to_path_buf(), item)))
}

/// Classify a notify path and, when it is a directory, its complete current subtree.
pub(crate) fn candidates_for_path(plan: &ScanPlan, path: &Path) -> Result<Vec<OwnedItem>, String> {
    if plan.roots().iter().any(|root| root == path) {
        let cancelled = AtomicBool::new(false);
        let mut items = Vec::new();
        // Root entries need not be records. Reconcile all configured owners
        // below this boundary, including independently selected mounted roots.
        for root in plan.roots().iter().filter(|root| root.starts_with(path)) {
            match scan_root(plan, root, &cancelled)? {
                ScanOutcome::Completed(scanned) => items.extend(scanned),
                _ => return Err("Configured-root reconciliation did not complete".into()),
            }
        }
        return Ok(items);
    }
    let Some(candidate) = candidate_for_path(plan, path)? else {
        return Ok(Vec::new());
    };
    if !matches!(candidate.item().kind, EntryKind::Directory) {
        return Ok(vec![candidate]);
    }

    let mut items = vec![candidate];
    let walker = WalkDir::new(path)
        .follow_links(plan.follow_links())
        .same_file_system(plan.same_file_system())
        .into_iter()
        .filter_entry(|entry| {
            plan.root_for(entry.path())
                .is_some_and(|root| plan.includes_path_for_root(entry.path(), root))
        });
    for entry in walker.skip(1) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error)
                if walk_error_is_skippable(
                    error.depth(),
                    error.io_error().map(std::io::Error::kind),
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        if let Some(item) = candidate_for_path(plan, entry.path())? {
            items.push(item);
        }
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::SearchConfig;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = crate::test_temp_dir().join(format!(
            "ottrin-scanner-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create scanner root");
        root
    }

    fn plan(root: std::path::PathBuf) -> crate::plan::ScanPlan {
        crate::plan::ScanPlan::new(&SearchConfig {
            include_roots: vec![root],
            include_hidden_system: true,
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        })
        .expect("valid scan plan")
    }

    #[test]
    fn permission_denied_below_a_readable_root_is_skipped() {
        assert!(walk_error_is_skippable(
            2,
            Some(std::io::ErrorKind::PermissionDenied)
        ));
    }

    #[test]
    fn permission_denied_at_the_configured_root_remains_fatal() {
        assert!(!walk_error_is_skippable(
            0,
            Some(std::io::ErrorKind::PermissionDenied)
        ));
    }

    #[test]
    fn full_scan_rejects_an_unavailable_configured_root() {
        let root = temp_root("unavailable-root");
        let plan = plan(root.clone());
        std::fs::remove_dir(&root).expect("remove configured root");

        let result = scan(&plan, &AtomicBool::new(false), |_| {});

        assert!(
            result
                .expect_err("unavailable configured root must fail the full scan")
                .contains("configured root is unavailable")
        );
    }

    #[test]
    fn cancellation_is_explicit_and_discards_private_candidate() {
        let root = temp_root("cancelled");
        std::fs::write(root.join("first.txt"), b"first").expect("write first");
        std::fs::write(root.join("second.txt"), b"second").expect("write second");
        let plan = plan(root.clone());
        let cancelled = AtomicBool::new(false);

        let outcome = scan(&plan, &cancelled, |_| {
            cancelled.store(true, Ordering::SeqCst);
        })
        .expect("scan should not fail");

        assert!(matches!(outcome, ScanOutcome::Cancelled));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scan_batched_emits_a_complete_generation_without_an_unbounded_result() {
        let root = temp_root("batched-generation");
        for index in 0..25 {
            std::fs::write(root.join(format!("item-{index:02}.txt")), b"item")
                .expect("write fixture");
        }
        let plan = plan(root.clone());
        let cancelled = AtomicBool::new(false);
        let mut batch_sizes = Vec::new();
        let mut paths = Vec::new();

        let mut progress = Vec::new();
        let started = Instant::now();
        let outcome = scan_batched(
            &plan,
            &cancelled,
            7,
            |batch: Vec<OwnedItem>| {
                batch_sizes.push(batch.len());
                paths.extend(batch.into_iter().map(|owned| owned.item().path.clone()));
                Ok(())
            },
            |update| progress.push(update),
        )
        .expect("scan succeeds");

        let final_progress = progress.last().expect("final progress");
        assert_eq!(final_progress.indexed_items, paths.len());
        assert_eq!(final_progress.last_indexed_path.as_ref(), paths.last());
        assert_eq!(final_progress.active_root, root);
        assert!(
            progress
                .windows(2)
                .all(|pair| pair[0].indexed_items <= pair[1].indexed_items)
        );
        assert!(
            progress.len() as u128 <= 3 + started.elapsed().as_millis() / 100,
            "progress must be bounded by elapsed time, not file count: {} updates",
            progress.len()
        );
        assert_eq!(outcome, BatchedScanOutcome::Completed { indexed_items: 25 });
        assert_eq!(batch_sizes, vec![7, 7, 7, 4]);
        assert_eq!(paths.len(), 25);
        assert_eq!(
            paths
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            25
        );
        std::fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn completed_scan_returns_items_with_exact_plan_ownership() {
        let root = temp_root("completed");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested");
        std::fs::write(nested.join("kept.txt"), b"kept").expect("write kept");
        std::fs::write(root.join("ignored.tmp"), b"ignored").expect("write ignored");
        let plan = crate::plan::ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone(), nested.clone()],
            exclude_globs: vec!["*.tmp".to_string()],
            include_hidden_system: true,
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        })
        .expect("valid scan plan");
        let cancelled = AtomicBool::new(false);

        let ScanOutcome::Completed(items) = scan(&plan, &cancelled, |_| {}).expect("scan succeeds")
        else {
            panic!("scan unexpectedly cancelled");
        };

        let matching = items
            .iter()
            .filter(|owned| owned.item().path == nested.join("kept.txt"))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].root(), nested.as_path());
        assert!(
            !items
                .iter()
                .any(|owned| owned.item().path == root.join("ignored.tmp"))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn single_root_scan_uses_exact_plan_ownership_and_filters() {
        let root = temp_root("single-root");
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested");
        std::fs::write(root.join("parent.txt"), b"parent").expect("write parent");
        std::fs::write(nested.join("child.txt"), b"child").expect("write child");
        let plan = crate::plan::ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone(), nested.clone()],
            include_hidden_system: true,
            ..SearchConfig::default()
        })
        .expect("plan");

        let ScanOutcome::Completed(items) =
            scan_root(&plan, &nested, &AtomicBool::new(false)).expect("root scan")
        else {
            panic!("root scan cancelled");
        };

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].root(), nested);
        assert_eq!(items[0].item().path, nested.join("child.txt"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn path_candidate_rejects_no_longer_included_existing_path() {
        let root = temp_root("path-candidate");
        let filtered = root.join("renamed.tmp");
        std::fs::write(&filtered, b"filtered").expect("write filtered");
        let plan = crate::plan::ScanPlan::new(&SearchConfig {
            include_roots: vec![root.clone()],
            exclude_globs: vec!["*.tmp".to_string()],
            include_hidden_system: true,
            ..SearchConfig::default()
        })
        .expect("plan");

        assert!(
            candidate_for_path(&plan, &filtered)
                .expect("classify")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
