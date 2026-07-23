use super::{ConflictAction, PlatformError, reject_directory_copy_into_itself};
use std::path::Path;

/// Reject Move before mutation until publication and source cleanup can be bound to
/// stable filesystem identities on every supported platform.
///
/// A pathname identity check followed by `rename`/`unlink` is inherently racy: the
/// checked entry can be replaced before the name-based syscall. Claiming the source
/// first would require a durable recovery journal so a crash cannot strand it under a
/// private name. Ottrin v1 does not yet own that journal, so failing safely is the only
/// honest behavior.
pub(super) fn move_entry_transactional(
    source: &Path,
    destination_dir: &Path,
    _conflict: ConflictAction,
) -> Result<(), PlatformError> {
    std::fs::symlink_metadata(source)
        .map_err(|_| PlatformError::InvalidSource(source.display().to_string()))?;
    source
        .file_name()
        .ok_or_else(|| PlatformError::InvalidSource(source.display().to_string()))?;
    reject_directory_copy_into_itself(source, destination_dir)?;
    Err(PlatformError::UnsupportedSafeMove(
        source.display().to_string(),
    ))
}
