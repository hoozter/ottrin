//! Explicit session Undo admission. Native directory IDs select current names;
//! the queue owns those names until strict worker-time pins have been acquired.
use super::super::{windows_completion, windows_control, windows_recovery_root};
use super::*;
use std::path::PathBuf;
use windows_control::DirectoryBinding;

#[derive(Clone, Debug)]
pub struct WindowsMoveUndoPlan {
    receipt: PathMoveUndoReceipt,
    directories: Vec<DirectoryBinding>,
    paths: Vec<PathBuf>,
    pairs: Vec<(PathBuf, PathBuf)>,
}
impl WindowsMoveUndoPlan {
    /// Read-only and potentially involves old volumes; run off the UI thread.
    pub fn capture(
        receipt: &PathMoveUndoReceipt,
        presentation_directories: &[PathBuf],
    ) -> Result<Self, PlatformError> {
        let Some(WindowsPathUndo::Move(guard)) = &receipt.windows_undo else {
            return Err(PlatformError::InvalidPathMoveUndoReceipt(
                "Move has no native Undo binding".into(),
            ));
        };
        let root = windows_recovery_root()?;
        let mut directories = Vec::new();
        let durable = !matches!(guard, WindowsMoveUndoGuard::Atomic { .. });
        if durable {
            let identity = move_recovery::stable_identity_of_entry(&root).map_err(io)?;
            directories.push(DirectoryBinding::capture(&root, &root, identity)?);
            if windows_completion::list(&root)?
                .iter()
                .any(|done| done.matches(receipt))
            {
                return Ok(Self {
                    receipt: receipt.clone(),
                    paths: vec![directories[0].path.clone()],
                    directories,
                    pairs: vec![(receipt.before_path.clone(), receipt.after_path.clone())],
                });
            }
        }
        let descriptors = match guard {
            WindowsMoveUndoGuard::Atomic {
                before_parent,
                after_parent,
            } => vec![
                (
                    receipt
                        .before_path
                        .parent()
                        .ok_or_else(|| stale(&receipt.before_path))?
                        .to_path_buf(),
                    *before_parent,
                ),
                (
                    receipt
                        .after_path
                        .parent()
                        .ok_or_else(|| stale(&receipt.after_path))?
                        .to_path_buf(),
                    *after_parent,
                ),
            ],
            WindowsMoveUndoGuard::Replace(record) => {
                overwrite::topology(record)?;
                vec![
                    (
                        record
                            .source_path()?
                            .parent()
                            .ok_or_else(|| stale(&receipt.before_path))?
                            .to_path_buf(),
                        record.source_parent_identity,
                    ),
                    (
                        record.destination_parent_path()?,
                        record.destination_parent_identity,
                    ),
                    (
                        record.transaction_path()?,
                        record.transaction_directory_identity,
                    ),
                ]
            }
            WindowsMoveUndoGuard::CrossVolume(guard) => guard.directories()?,
        };
        let mut captured = descriptors
            .iter()
            .map(|(hint, id)| DirectoryBinding::capture(&root, hint, *id))
            .collect::<Result<Vec<_>, _>>()?;
        let before = captured[0].label.join(
            receipt
                .before_path
                .file_name()
                .ok_or_else(|| stale(&receipt.before_path))?,
        );
        let after = captured[1].label.join(
            receipt
                .after_path
                .file_name()
                .ok_or_else(|| stale(&receipt.after_path))?,
        );
        let mut pairs = vec![(before, after)];
        if let Some(retained) = &receipt.retained_displaced_path {
            let parent = retained.parent().ok_or_else(|| stale(retained))?;
            let (_, binding) = descriptors
                .iter()
                .zip(&captured)
                .find(|((hint, _), _)| hint == parent)
                .ok_or_else(|| stale(retained))?;
            pairs.push((
                binding
                    .path
                    .join(retained.file_name().ok_or_else(|| stale(retained))?),
                pairs[0].1.clone(),
            ));
        }
        // Directory identities, not UI labels, decide which visible spellings
        // refer to an admitted parent. Keep the canonical queue/lease paths
        // unchanged; extra pairs are only for refreshing visible listings.
        let admitted_pairs = pairs.clone();
        for visible in presentation_directories
            .iter()
            .filter(|path| path.is_absolute())
        {
            let Ok(named) = windows_fs::open_directory(
                visible,
                FILE_READ_ATTRIBUTES
                    | windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE
                    | SYNCHRONIZE,
            ) else {
                continue;
            };
            // An unrelated or inaccessible presentation is not needed to admit
            // the actual, independently verified mutation directories.
            let Ok(identity) = move_recovery::stable_identity_from_handle(&named) else {
                continue;
            };
            let matches_parent = |path: &Path| {
                captured.iter().any(|binding| {
                    binding.identity == identity
                        && path
                            .parent()
                            .is_some_and(|parent| parent == binding.path || parent == binding.label)
                })
            };
            for (before, after) in &admitted_pairs {
                let before_matches = matches_parent(before);
                let after_matches = matches_parent(after);
                if !before_matches && !after_matches {
                    continue;
                }
                let visible = std::path::absolute(visible).map_err(io)?;
                let presented = (
                    if before_matches {
                        visible.join(before.file_name().ok_or_else(|| stale(before))?)
                    } else {
                        before.clone()
                    },
                    if after_matches {
                        visible.join(after.file_name().ok_or_else(|| stale(after))?)
                    } else {
                        after.clone()
                    },
                );
                if !pairs.contains(&presented) {
                    pairs.push(presented);
                }
            }
        }
        directories.append(&mut captured);
        directories.sort_by(|a, b| a.path.cmp(&b.path));
        if directories
            .windows(2)
            .any(|pair| pair[0].path == pair[1].path && pair[0].identity != pair[1].identity)
        {
            return Err(io("Undo namespace changed during admission"));
        }
        directories.dedup();
        let paths = directories.iter().map(|d| d.path.clone()).collect();
        Ok(Self {
            receipt: receipt.clone(),
            directories,
            paths,
            pairs,
        })
    }
    pub fn receipt(&self) -> &PathMoveUndoReceipt {
        &self.receipt
    }
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
    pub fn before_path(&self) -> &Path {
        &self.pairs[0].0
    }
    pub fn after_path(&self) -> &Path {
        &self.pairs[0].1
    }
    /// Presentation labels, including verified visible aliases. Queue and
    /// mutation authority always comes from `paths()` and the pinned bindings.
    pub fn path_pairs(&self) -> Vec<(&Path, &Path)> {
        self.pairs
            .iter()
            .map(|(before, after)| (before.as_path(), after.as_path()))
            .collect()
    }
    pub fn execute(&self) -> Result<(), PlatformError> {
        // Keep every namespace lease through the existing native inverse. A
        // rename after queue admission must fail instead of moving the worker
        // outside its reserved names or rebinding to recreated old folders.
        let _pins = self
            .directories
            .iter()
            .map(DirectoryBinding::pin)
            .collect::<Result<Vec<_>, _>>()?;
        super::undo(&self.receipt)
    }
}

#[cfg(test)]
mod tests;
