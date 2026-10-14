use std::path::{Path, PathBuf};

/// Use the filesystem's Windows spelling for identity, preserving verbatim names
/// and resolving normal/verbatim, case and directory-link aliases consistently.
/// Missing children retain their suffix beneath the closest resolvable ancestor.
pub(crate) fn resolved_index_path(path: &Path) -> PathBuf {
    if !path.is_absolute() {
        return path.to_path_buf();
    }
    for ancestor in path.ancestors() {
        if let Ok(resolved) = std::fs::canonicalize(ancestor)
            && let Ok(suffix) = path.strip_prefix(ancestor)
        {
            return if suffix.as_os_str().is_empty() {
                resolved
            } else {
                resolved.join(suffix)
            };
        }
    }
    path.to_path_buf()
}
