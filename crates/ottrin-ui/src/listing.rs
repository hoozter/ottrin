use crate::path_display::path_label;
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
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
            };
            if !show_hidden
                && meta.file_attributes() & (FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM) != 0
            {
                continue;
            }
        }
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
        let created_unix_secs = ottrin_core::metadata_created_unix_secs(&meta);
        let symlink_target_is_dir = if matches!(kind, EntryKind::Symlink) {
            std::fs::metadata(item.path())
                .ok()
                .map(|target| target.is_dir())
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
            created_unix_secs,
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
    entries.sort_by(|a, b| compare_entries(a, b, config));
}

/// The one ordering used by scans, visible-index caches and indexed results.
///
/// Folder grouping is applied first and is deliberately independent of the
/// requested direction. Size and date keys place entries with missing metadata
/// last in both directions. Name ordering is a case-insensitive, deterministic
/// tie-break; it reverses with kind groups but stays ascending for numeric and
/// date ties.
fn compare_entries(
    a: &FileEntry,
    b: &FileEntry,
    config: &ottrin_core::SortConfig,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    if config.folders_first {
        let folder_order = match (is_folder(a), is_folder(b)) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        };
        if folder_order != Ordering::Equal {
            return folder_order;
        }
    }

    let (primary, name_tie_ascending) = match config.by {
        SortBy::Name => return compare_names(a, b, config.ascending),
        SortBy::Size => (
            compare_optional(a.size_bytes, b.size_bytes, config.ascending),
            true,
        ),
        SortBy::Modified => (
            compare_optional(a.modified_unix_secs, b.modified_unix_secs, config.ascending),
            true,
        ),
        SortBy::Created => (
            compare_optional(a.created_unix_secs, b.created_unix_secs, config.ascending),
            true,
        ),
        SortBy::Kind => (
            compare_ordered(kind_sort_key(a), kind_sort_key(b), config.ascending),
            config.ascending,
        ),
    };
    primary.then_with(|| compare_names(a, b, name_tie_ascending))
}

fn compare_ordered<T: Ord>(a: T, b: T, ascending: bool) -> std::cmp::Ordering {
    if ascending { a.cmp(&b) } else { b.cmp(&a) }
}

fn compare_optional<T: Ord>(a: Option<T>, b: Option<T>, ascending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (a, b) {
        (Some(a), Some(b)) => compare_ordered(a, b, ascending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_names(a: &FileEntry, b: &FileEntry, ascending: bool) -> std::cmp::Ordering {
    let ordering = a
        .name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.name.cmp(&b.name))
        // Identical display names are possible in indexed result sets.
        .then_with(|| a.path.cmp(&b.path));
    if ascending {
        ordering
    } else {
        ordering.reverse()
    }
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

pub(crate) fn default_sort_direction(sort_by: SortBy) -> bool {
    matches!(sort_by, SortBy::Name | SortBy::Kind)
}

fn is_folder(entry: &FileEntry) -> bool {
    entry.kind == EntryKind::Directory
        || entry.kind == EntryKind::Symlink && entry.symlink_target_is_dir == Some(true)
}

fn kind_sort_key(entry: &FileEntry) -> (u8, String) {
    let rank = match entry.kind {
        EntryKind::Directory => 0,
        EntryKind::Symlink if entry.symlink_target_is_dir == Some(true) => 0,
        EntryKind::Symlink => 1,
        EntryKind::File => 2,
        EntryKind::Other => 3,
    };
    let extension = if is_folder(entry) {
        String::new()
    } else {
        Path::new(&entry.name)
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    (rank, extension)
}

fn format_list_dir_error(path: &Path, e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => {
            format!(
                "Permission denied\nYou don't have permission to read this folder:\n{}",
                path_label(path)
            )
        }
        ErrorKind::NotFound => {
            format!(
                "Folder not found\nThis folder no longer exists:\n{}",
                path_label(path)
            )
        }
        _ => format!("Could not open folder:\n{}\n({})", path_label(path), e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::SortConfig;
    use std::collections::BTreeSet;

    fn names(path: &Path, show_hidden: bool) -> BTreeSet<String> {
        list_directory(path, show_hidden)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[test]
    fn listing_uses_platform_creation_metadata_when_available() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("created.txt");
        std::fs::write(&path, b"fixture").unwrap();
        let entry = list_directory(root.path(), false)
            .unwrap()
            .into_iter()
            .find(|entry| entry.path == path)
            .unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        assert_eq!(
            entry.created_unix_secs,
            ottrin_core::metadata_created_unix_secs(&metadata)
        );
    }

    #[test]
    fn hidden_toggle_controls_dot_files_and_directories() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("visible.txt"), b"visible").unwrap();
        std::fs::write(root.path().join(".hidden.txt"), b"hidden").unwrap();
        std::fs::create_dir(root.path().join(".hidden-dir")).unwrap();
        assert_eq!(
            names(root.path(), false),
            BTreeSet::from(["visible.txt".into()])
        );
        assert_eq!(names(root.path(), true).len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_directory_metadata_preserves_link_identity() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("target-dir")).unwrap();
        std::fs::write(root.path().join("target-file"), b"target").unwrap();
        for (link, target) in [
            ("dir-link", "target-dir"),
            ("file-link", "target-file"),
            ("broken-link", "missing-target"),
        ] {
            std::os::unix::fs::symlink(target, root.path().join(link)).unwrap();
        }
        let entries = list_directory(root.path(), false).unwrap();
        for (name, target_is_dir) in [
            ("dir-link", Some(true)),
            ("file-link", Some(false)),
            ("broken-link", None),
        ] {
            let entry = entries.iter().find(|entry| entry.name == name).unwrap();
            assert!(matches!(entry.kind, EntryKind::Symlink));
            assert_eq!(entry.path, root.path().join(name));
            assert_eq!(entry.symlink_target_is_dir, target_is_dir, "{name}");
            assert_eq!(entry.size_bytes, None);
        }
        assert!(root.path().join("target-dir").is_dir());
        assert_eq!(
            std::fs::read(root.path().join("target-file")).unwrap(),
            b"target"
        );
    }

    fn entry(
        name: &str,
        kind: EntryKind,
        size: Option<u64>,
        modified: Option<u64>,
        created: Option<u64>,
    ) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from("/sort-fixture").join(name),
            kind,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: size,
            modified_unix_secs: modified,
            created_unix_secs: created,
        }
    }

    fn sorted(mut entries: Vec<FileEntry>, by: SortBy, ascending: bool) -> Vec<String> {
        sort_entries(
            &mut entries,
            &SortConfig {
                by,
                ascending,
                folders_first: true,
            },
        );
        entries.into_iter().map(|entry| entry.name).collect()
    }

    fn entry_names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn name_sort_is_case_insensitive_and_reverses_cleanly() {
        let entries = ["beta.txt", "Alpha.txt", "alpha.txt", "gamma.txt"]
            .map(|name| entry(name, EntryKind::File, None, None, None))
            .to_vec();
        assert_eq!(
            sorted(entries.clone(), SortBy::Name, true),
            ["Alpha.txt", "alpha.txt", "beta.txt", "gamma.txt"]
        );
        assert_eq!(
            sorted(entries, SortBy::Name, false),
            ["gamma.txt", "beta.txt", "alpha.txt", "Alpha.txt"]
        );
    }

    #[test]
    fn folders_stay_first_independently_of_direction_for_every_key() {
        let entries = vec![
            entry("a.txt", EntryKind::File, Some(10), Some(100), Some(100)),
            entry("z-folder", EntryKind::Directory, None, None, None),
            entry("m.txt", EntryKind::File, Some(20), Some(200), Some(200)),
        ];
        for (by, ascending_order, descending_order) in [
            (
                SortBy::Name,
                ["z-folder", "a.txt", "m.txt"],
                ["z-folder", "m.txt", "a.txt"],
            ),
            (
                SortBy::Size,
                ["z-folder", "a.txt", "m.txt"],
                ["z-folder", "m.txt", "a.txt"],
            ),
            (
                SortBy::Modified,
                ["z-folder", "a.txt", "m.txt"],
                ["z-folder", "m.txt", "a.txt"],
            ),
            (
                SortBy::Created,
                ["z-folder", "a.txt", "m.txt"],
                ["z-folder", "m.txt", "a.txt"],
            ),
            (
                SortBy::Kind,
                ["z-folder", "a.txt", "m.txt"],
                ["z-folder", "m.txt", "a.txt"],
            ),
        ] {
            assert_eq!(
                sorted(entries.clone(), by, true),
                ascending_order,
                "{by:?} ascending"
            );
            assert_eq!(
                sorted(entries.clone(), by, false),
                descending_order,
                "{by:?} descending"
            );
        }
    }

    #[test]
    fn folders_first_false_mixes_folders_with_the_primary_order() {
        let entries = vec![
            entry("a.txt", EntryKind::File, Some(10), None, None),
            entry("z-folder", EntryKind::Directory, None, None, None),
            entry("m.txt", EntryKind::File, Some(20), None, None),
        ];
        let mut sorted_entries = entries;
        sort_entries(
            &mut sorted_entries,
            &SortConfig {
                by: SortBy::Size,
                ascending: false,
                folders_first: false,
            },
        );
        assert_eq!(entry_names(&sorted_entries), ["m.txt", "a.txt", "z-folder"]);
    }

    #[test]
    fn metadata_sorts_follow_direction_and_keep_missing_values_last() {
        for by in [SortBy::Size, SortBy::Modified, SortBy::Created] {
            let entries = vec![
                entry("missing.txt", EntryKind::File, None, None, None),
                entry("small.txt", EntryKind::File, Some(10), Some(100), Some(100)),
                entry("large.txt", EntryKind::File, Some(20), Some(200), Some(200)),
            ];
            assert_eq!(
                sorted(entries.clone(), by, true),
                ["small.txt", "large.txt", "missing.txt"],
                "{by:?} ascending"
            );
            assert_eq!(
                sorted(entries, by, false),
                ["large.txt", "small.txt", "missing.txt"],
                "{by:?} descending"
            );
        }
    }

    #[test]
    fn created_sort_orders_known_creation_times_in_both_directions() {
        let entries = vec![
            entry("middle.txt", EntryKind::File, None, None, Some(200)),
            entry("newest.txt", EntryKind::File, None, None, Some(300)),
            entry("oldest.txt", EntryKind::File, None, None, Some(100)),
        ];
        assert_eq!(
            sorted(entries.clone(), SortBy::Created, true),
            ["oldest.txt", "middle.txt", "newest.txt"]
        );
        assert_eq!(
            sorted(entries, SortBy::Created, false),
            ["newest.txt", "middle.txt", "oldest.txt"]
        );
    }

    #[test]
    fn metadata_ties_use_deterministic_ascending_name_breaks() {
        let entries = vec![
            entry("beta.txt", EntryKind::File, Some(10), None, None),
            entry("alpha.txt", EntryKind::File, Some(10), None, None),
        ];
        assert_eq!(
            sorted(entries.clone(), SortBy::Size, true),
            ["alpha.txt", "beta.txt"]
        );
        assert_eq!(
            sorted(entries, SortBy::Size, false),
            ["alpha.txt", "beta.txt"]
        );
    }

    #[test]
    fn kind_sort_reverses_groups_and_names_but_folders_stay_first() {
        let entries = vec![
            entry("beta.txt", EntryKind::File, None, None, None),
            entry("alpha.txt", EntryKind::File, None, None, None),
            entry("docs", EntryKind::Directory, None, None, None),
        ];
        assert_eq!(
            sorted(entries.clone(), SortBy::Kind, true),
            ["docs", "alpha.txt", "beta.txt"]
        );
        assert_eq!(
            sorted(entries, SortBy::Kind, false),
            ["docs", "beta.txt", "alpha.txt"]
        );
    }

    #[test]
    fn duplicate_names_have_a_path_tie_break_for_determinism() {
        let mut entries = vec![
            entry("same.txt", EntryKind::File, None, None, None),
            entry("same.txt", EntryKind::File, None, None, None),
        ];
        entries[0].path = PathBuf::from("/a/same.txt");
        entries[1].path = PathBuf::from("/b/same.txt");
        sort_entries(&mut entries, &SortConfig::default());
        assert_eq!(entries[0].path, PathBuf::from("/a/same.txt"));
        assert_eq!(entries[1].path, PathBuf::from("/b/same.txt"));
    }

    #[test]
    fn sort_keys_have_sensible_default_directions() {
        assert!(default_sort_direction(SortBy::Name));
        assert!(default_sort_direction(SortBy::Kind));
        assert!(!default_sort_direction(SortBy::Size));
        assert!(!default_sort_direction(SortBy::Modified));
        assert!(!default_sort_direction(SortBy::Created));
    }

    #[test]
    fn type_sort_groups_extensions_in_both_directions() {
        let entries = ["a.txt", "z.pdf", "b.TXT"]
            .map(|name| entry(name, EntryKind::File, None, None, None))
            .to_vec();
        assert_eq!(
            sorted(entries.clone(), SortBy::Kind, true),
            ["z.pdf", "a.txt", "b.TXT"]
        );
        assert_eq!(
            sorted(entries, SortBy::Kind, false),
            ["b.TXT", "a.txt", "z.pdf"]
        );
    }

    #[test]
    fn directory_symlinks_participate_in_folders_first() {
        let mut link = entry("z-link", EntryKind::Symlink, None, None, None);
        link.symlink_target_is_dir = Some(true);
        let entries = vec![entry("a.txt", EntryKind::File, Some(100), None, None), link];
        assert_eq!(sorted(entries, SortBy::Size, false), ["z-link", "a.txt"]);
    }

    #[cfg(windows)]
    #[test]
    fn hidden_toggle_controls_windows_hidden_and_system_attributes() {
        use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM,
            SetFileAttributesW,
        };
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("visible.txt"), b"visible").unwrap();
        std::fs::create_dir(root.path().join("visible-dir")).unwrap();
        for (name, directory, attribute) in [
            ("hidden.txt", false, FILE_ATTRIBUTE_HIDDEN),
            ("system.txt", false, FILE_ATTRIBUTE_SYSTEM),
            ("hidden-日本語.txt", false, FILE_ATTRIBUTE_HIDDEN),
            ("hidden-dir", true, FILE_ATTRIBUTE_HIDDEN),
            ("system-dir", true, FILE_ATTRIBUTE_SYSTEM),
            ("archive.txt", false, FILE_ATTRIBUTE_ARCHIVE),
        ] {
            let path = root.path().join(name);
            if directory {
                std::fs::create_dir(&path).unwrap();
            } else {
                std::fs::write(&path, b"attribute fixture").unwrap();
            }
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            assert_ne!(unsafe { SetFileAttributesW(wide.as_ptr(), attribute) }, 0);
            assert_ne!(
                std::fs::symlink_metadata(&path).unwrap().file_attributes() & attribute,
                0
            );
        }
        assert_eq!(
            names(root.path(), false),
            BTreeSet::from([
                "archive.txt".into(),
                "visible-dir".into(),
                "visible.txt".into()
            ])
        );
        assert_eq!(names(root.path(), true).len(), 8);
    }
}
