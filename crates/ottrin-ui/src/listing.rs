use ottrin_core::{EntryKind, FileEntry, SortBy, default_home_dir};
use std::path::{Path, PathBuf};

pub(crate) fn list_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    use std::time::UNIX_EPOCH;

    let read_dir = std::fs::read_dir(path).map_err(|e| format_list_dir_error(path, &e))?;

    let mut entries = Vec::new();
    for item in read_dir {
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let name = item.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let file_type = match item.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
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
        let size_bytes = if matches!(kind, EntryKind::File) {
            Some(meta.len())
        } else {
            None
        };
        let modified_unix_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let symlink_target_is_dir = if matches!(kind, EntryKind::Symlink) {
            Some(meta.is_dir())
        } else {
            None
        };
        let is_executable = detect_executable(&meta);

        entries.push(FileEntry {
            name,
            path: item.path(),
            kind,
            is_executable,
            symlink_target_is_dir,
            size_bytes,
            modified_unix_secs,
        });
    }

    Ok(entries)
}

pub(crate) fn detect_executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

pub(crate) fn sort_entries(entries: &mut [FileEntry], config: &ottrin_core::SortConfig) {
    // Directories always come first
    entries.sort_by(|a, b| {
        let a_dir = matches!(a.kind, EntryKind::Directory);
        let b_dir = matches!(b.kind, EntryKind::Directory);
        if a_dir != b_dir {
            return b_dir.cmp(&a_dir); // dirs first
        }
        let ord = match config.by {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Size => b
                .size_bytes
                .unwrap_or(0)
                .cmp(&a.size_bytes.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortBy::Modified => b
                .modified_unix_secs
                .unwrap_or(0)
                .cmp(&a.modified_unix_secs.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortBy::Kind => kind_sort_rank(a).cmp(&kind_sort_rank(b)).then_with(|| {
                let a_name = a.name.to_lowercase();
                let b_name = b.name.to_lowercase();
                if config.ascending {
                    a_name.cmp(&b_name)
                } else {
                    b_name.cmp(&a_name)
                }
            }),
        };
        match config.by {
            SortBy::Name => {
                if config.ascending {
                    ord
                } else {
                    ord.reverse()
                }
            }
            SortBy::Kind | SortBy::Size | SortBy::Modified => ord,
        }
    });
}

pub(crate) fn nearest_existing_dir(path: &Path) -> PathBuf {
    let mut cursor = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_home_dir)
    };
    loop {
        if cursor.is_dir() {
            return cursor;
        }
        if !cursor.pop() {
            return default_home_dir();
        }
    }
}

pub(crate) fn sibling_from_entries(entries: &[FileEntry], target: &Path) -> Option<PathBuf> {
    let idx = entries.iter().position(|entry| entry.path == target)?;
    if idx > 0 {
        entries.get(idx - 1).map(|entry| entry.path.clone())
    } else {
        entries.get(idx + 1).map(|entry| entry.path.clone())
    }
}

pub(crate) fn sort_supports_direction(sort_by: SortBy) -> bool {
    matches!(sort_by, SortBy::Name | SortBy::Kind)
}

pub(crate) fn default_sort_direction(sort_by: SortBy) -> bool {
    matches!(sort_by, SortBy::Name | SortBy::Kind)
}

fn kind_sort_rank(entry: &FileEntry) -> u8 {
    match entry.kind {
        EntryKind::Directory => 0,
        EntryKind::Symlink => 1,
        EntryKind::File => 2,
        EntryKind::Other => 3,
    }
}

fn format_list_dir_error(path: &Path, e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => {
            format!(
                "Permission denied\nYou don't have permission to read this folder:\n{}",
                path.display()
            )
        }
        ErrorKind::NotFound => {
            format!(
                "Folder not found\nThis folder no longer exists:\n{}",
                path.display()
            )
        }
        _ => format!("Could not open folder:\n{}\n({})", path.display(), e),
    }
}
