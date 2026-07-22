use crate::mutation::OwnedItem;
use crate::plan::ScanPlan;
use ottrin_core::{EntryKind, SearchResultItem};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;
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
pub(crate) enum ScanOutcome {
    Completed(Vec<OwnedItem>),
    Cancelled,
}

pub(crate) fn scan(
    plan: &ScanPlan,
    cancelled: &AtomicBool,
    mut on_progress: impl FnMut(ScanProgress),
) -> Result<ScanOutcome, String> {
    let roots = plan.roots();
    let total_roots = roots.len();
    let mut items = Vec::new();
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
        return Ok(ScanOutcome::Cancelled);
    }

    for (root_index, root) in roots.iter().enumerate() {
        if cancelled.load(Ordering::Acquire) {
            return Ok(ScanOutcome::Cancelled);
        }
        if !root.exists() {
            continue;
        }
        let walker = WalkDir::new(root)
            .follow_links(plan.follow_links())
            .same_file_system(plan.same_file_system())
            .into_iter()
            .filter_entry(|entry| plan.includes_path_for_root(entry.path(), root));
        for entry in walker {
            if cancelled.load(Ordering::Acquire) {
                return Ok(ScanOutcome::Cancelled);
            }
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path == root {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !plan.includes_item_name(&name) {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let file_type = entry.file_type();
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Directory
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            let mut item = SearchResultItem {
                path: path.to_path_buf(),
                parent_path: path
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default(),
                name,
                kind,
                size_bytes: matches!(kind, EntryKind::File).then(|| metadata.len()),
                modified_unix_secs: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs()),
                is_executable: crate::detect_executable(&metadata),
                symlink_target_is_dir: matches!(kind, EntryKind::Symlink)
                    .then(|| metadata.is_dir()),
                content_snippet: None,
                name_lower: String::new(),
                path_str: String::new(),
                path_lower: String::new(),
            };
            item.prepare();
            let last_path = item.path.clone();
            let owner = plan
                .root_for(path)
                .expect("walked path belongs to its exact plan")
                .to_path_buf();
            items.push(OwnedItem::new(owner, item));
            on_progress(ScanProgress {
                message: crate::format_progress(
                    root_index + 1,
                    total_roots.max(1),
                    items.len(),
                    items.len(),
                ),
                active_root: root.clone(),
                active_root_index: root_index + 1,
                total_roots,
                indexed_items: items.len(),
                last_indexed_path: Some(last_path),
            });
        }
    }
    if cancelled.load(Ordering::Acquire) {
        Ok(ScanOutcome::Cancelled)
    } else {
        Ok(ScanOutcome::Completed(items))
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
    if !root.exists() {
        return Err(format!(
            "configured root is unavailable: {}",
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
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path() == root {
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
    if path == root || !plan.includes_path_for_root(path, root) {
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
        let entry = entry.map_err(|error| error.to_string())?;
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
        let root = std::env::temp_dir().join(format!(
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
