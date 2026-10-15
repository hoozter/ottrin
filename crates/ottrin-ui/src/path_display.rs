//! Human-readable labels only. Never parse these strings back into operation paths.
use std::path::{Path, PathBuf};

pub(crate) fn path_label(path: &Path) -> String {
    let text = path.display().to_string();
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => {
                    return text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
                }
                Prefix::VerbatimUNC(_, _) => {
                    if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
                        return format!(r"\\{tail}");
                    }
                }
                // Volume GUIDs and device namespaces have no equivalent short label.
                _ => {}
            }
        }
    }
    text
}

pub(crate) struct PathCrumb {
    pub label: String,
    pub target: PathBuf,
    pub is_root: bool,
}

pub(crate) fn path_crumbs(path: &Path) -> Vec<PathCrumb> {
    let mut crumbs: Vec<_> = path
        .ancestors()
        .filter(|target| !target.as_os_str().is_empty())
        .map(|target| {
            let is_root = target.has_root() && target.parent().is_none();
            let label = if is_root {
                path_label(target)
            } else {
                target.file_name().map_or_else(
                    || path_label(target),
                    |name| name.to_string_lossy().into_owned(),
                )
            };
            PathCrumb {
                label,
                target: target.to_path_buf(),
                is_root,
            }
        })
        .collect();
    crumbs.reverse();
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_names_and_targets_are_preserved() {
        let path = Path::new("folder").join("日本語.txt");
        let crumbs = path_crumbs(&path);
        assert_eq!(crumbs.len(), 2);
        assert_eq!(crumbs[0].label, "folder");
        assert_eq!(crumbs[0].target, Path::new("folder"));
        assert_eq!(crumbs[1].label, "日本語.txt");
        assert_eq!(crumbs[1].target, path);
        assert!(crumbs.iter().all(|crumb| !crumb.is_root));
    }

    #[cfg(unix)]
    #[test]
    fn unix_root_and_literal_windows_looking_names_are_unchanged() {
        let path = Path::new("/home/日本語/file");
        let crumbs = path_crumbs(path);
        assert_eq!(crumbs[0].label, "/");
        assert_eq!(crumbs[0].target, Path::new("/"));
        assert!(crumbs[0].is_root);
        assert_eq!(crumbs.last().unwrap().target, path);
        let literal = Path::new(r"\\?\C:\literal");
        assert_eq!(path_label(literal), literal.display().to_string());
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_labels_hide_only_namespace_and_keep_original_targets() {
        for (raw, label) in [
            (r"C:\Users\日本語", r"C:\Users\日本語"),
            (r"\\?\C:\Users\日本語", r"C:\Users\日本語"),
            (r"\\?\C:\trailing. ", r"C:\trailing. "),
        ] {
            let path = Path::new(raw);
            assert_eq!(path_label(path), label);
            let crumbs = path_crumbs(path);
            assert_eq!(crumbs[0].label, r"C:\");
            assert!(crumbs[0].is_root);
            assert_eq!(crumbs.len(), path.components().count() - 1);
            assert_eq!(crumbs.last().unwrap().target.as_os_str(), path.as_os_str());
            assert_eq!(
                crumbs[0].target.components().next(),
                path.components().next()
            );
            assert!(crumbs[0].target.is_absolute());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_unc_share_is_one_root_and_remains_the_original_navigation_path() {
        for raw in [r"\\server\share\日本語", r"\\?\UNC\server\share\日本語"] {
            let path = Path::new(raw);
            assert_eq!(path_label(path), r"\\server\share\日本語");
            let crumbs = path_crumbs(path);
            assert_eq!(crumbs.len(), 2);
            assert_eq!(crumbs[0].label, "\\\\server\\share\\");
            assert!(crumbs[0].is_root);
            assert!(crumbs[0].target.is_absolute());
            assert_eq!(crumbs.last().unwrap().target.as_os_str(), path.as_os_str());
            assert_eq!(
                crumbs[0].target.components().next(),
                path.components().next()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_volume_and_device_namespaces_are_not_rewritten() {
        for raw in [
            r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\folder",
            r"\\.\COM1",
        ] {
            let path = Path::new(raw);
            assert_eq!(path_label(path), raw);
            assert_eq!(
                path_crumbs(path).last().unwrap().target.as_os_str(),
                path.as_os_str()
            );
        }
    }
}
