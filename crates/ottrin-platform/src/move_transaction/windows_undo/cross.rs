//! An inverse uses the ordinary cross-volume engine with an exact target and an
//! immutable original-publication binding. Terminal inverse journals remain as
//! completion proof; they cannot be replayed as a fresh forward Move.
use super::super::{
    WINDOWS_LOCAL_JOURNAL_NAME, WINDOWS_RETAINED_NAME, WindowsCrossVolumePhase as Phase,
    WindowsCrossVolumeRecord as Record, load_windows_cross_volume_record, observe_windows_identity,
    persist_windows_cross_volume_record, resume_windows_cross_volume,
    windows_cross_volume_journal_path, windows_directory_source_claim_path,
};
use super::*;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::move_transaction) struct Origin {
    forward_transaction_id: String,
    original: move_recovery::NativePath,
    original_parent: WindowsStableIdentity,
    publication: move_recovery::NativePath,
    publication_parent: WindowsStableIdentity,
    publication_identity: WindowsStableIdentity,
    #[serde(default)]
    retained: Option<retained::Binding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Guard {
    origin: Origin,
    inverse: Arc<OnceLock<Record>>,
}

impl Guard {
    pub(in crate::move_transaction) fn matches_completion(
        &self,
        origin: &Origin,
        inverse: [u8; 16],
        private: WindowsStableIdentity,
    ) -> bool {
        self.origin == *origin
            && self.inverse.get().is_none_or(|record| {
                record.transaction_id().ok() == Some(inverse)
                    && record.transaction_directory_identity == private
            })
    }
    // Observation only: another owner may have started this publication's
    // inverse. If this session already bound an inverse, it must match too.
    pub(in crate::move_transaction) fn matches_recovery_record(&self, record: &Record) -> bool {
        record.undo_origin.as_ref() == Some(&self.origin)
            && self
                .inverse
                .get()
                .is_none_or(|initial| initial.same_transaction_evidence(record))
    }
}

fn path(encoded: &move_recovery::NativePath) -> Result<std::path::PathBuf, PlatformError> {
    encoded.decode().map_err(io)
}
fn pending(record: &Record, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::CommittedUndoCleanupPending {
        transaction: record.transaction_id.clone(),
        message: format!(
            "Cross-volume Undo needs attention; source and recovery evidence were preserved: {error}"
        ),
    }
}

impl Origin {
    pub(in crate::move_transaction) fn matches_control(
        &self,
        owner: &super::super::windows_completion::controls::Owner,
        inverse: [u8; 16],
        private: WindowsStableIdentity,
    ) -> Result<bool, PlatformError> {
        let (directory, identity, parent_identity) = if owner.transaction == inverse {
            (
                path(&self.original)?
                    .parent()
                    .ok_or_else(|| stale(Path::new("")))?
                    .join(format!(
                        ".ottrin-move-{}",
                        super::super::encode_transaction_id(inverse)
                    )),
                private,
                self.original_parent,
            )
        } else if owner.transaction == self.forward_id()? {
            let Some(binding) = &self.retained else {
                return Ok(false);
            };
            (
                binding.retained_path()?.parent().unwrap().to_path_buf(),
                binding.directory_identity(),
                self.publication_parent,
            )
        } else {
            return Ok(false);
        };
        Ok(owner.identity == identity
            && owner.directory.decode().map_err(io)? == directory
            && owner.parent.decode().map_err(io)? == directory.parent().unwrap()
            && owner.parent_identity == parent_identity)
    }

    pub(in crate::move_transaction) fn forward_id(&self) -> Result<[u8; 16], PlatformError> {
        super::super::parse_windows_journal_name(OsStr::new(&format!(
            "{}.cross-journal",
            self.forward_transaction_id
        )))
        .map(|(id, _)| id)
        .ok_or_else(|| PlatformError::Io("Invalid inverse forward transaction ID".into()))
    }
    pub(in crate::move_transaction) fn inverse_volume(&self) -> u64 {
        self.original_parent.volume_serial_number()
    }
    pub(in crate::move_transaction) fn completion_controller(
        &self,
    ) -> Result<Option<[u8; 16]>, PlatformError> {
        self.validate()?;
        Ok(self.retained.as_ref().map(|_| {
            super::super::parse_windows_journal_name(OsStr::new(&format!(
                "{}.cross-journal",
                self.forward_transaction_id
            )))
            .expect("validated transaction ID")
            .0
        }))
    }
    pub(in crate::move_transaction) fn validate(&self) -> Result<(), PlatformError> {
        let original = path(&self.original)?;
        let publication = path(&self.publication)?;
        let journal = format!("{}.cross-journal", self.forward_transaction_id);
        if super::super::parse_windows_journal_name(OsStr::new(&journal)).is_none()
            || !original.is_absolute()
            || !publication.is_absolute()
            || original == publication
            || original.file_name().is_none()
            || publication.file_name().is_none()
            || !self.original_parent.is_directory_without_reparse()
            || !self.publication_parent.is_directory_without_reparse()
            || self.original_parent.volume_serial_number()
                == self.publication_parent.volume_serial_number()
            || self.publication_identity.volume_serial_number()
                != self.publication_parent.volume_serial_number()
            || super::super::windows_cross_volume_source_kind(
                self.publication_identity.is_regular_file_without_reparse(),
                self.publication_identity.is_directory_without_reparse(),
                self.publication_identity.is_symbolic_link(),
            )
            .is_none()
        {
            return Err(stale(&publication));
        }
        if let Some(binding) = &self.retained {
            binding.validate(self)?;
        }
        windows_fs::valid_component(original.file_name().unwrap()).map_err(io)?;
        windows_fs::valid_component(publication.file_name().unwrap()).map_err(io)?;
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "validates the existing engine's endpoint names, native handle identities and no-replace admission together"
    )]
    pub(in crate::move_transaction) fn validate_inputs(
        &self,
        source: &Path,
        target: &Path,
        source_identity: WindowsStableIdentity,
        source_parent: WindowsStableIdentity,
        destination_parent: WindowsStableIdentity,
        conflict: crate::ConflictAction,
        destination_identity: Option<WindowsStableIdentity>,
    ) -> Result<(), PlatformError> {
        self.validate()?;
        if source != path(&self.publication)?
            || target != path(&self.original)?
            || source_identity != self.publication_identity
            || source_parent != self.publication_parent
            || destination_parent != self.original_parent
            || conflict != crate::ConflictAction::Skip
            || destination_identity.is_some()
        {
            return Err(stale(source));
        }
        Ok(())
    }
}

pub(in crate::move_transaction) fn validate_record(record: &Record) -> Result<(), PlatformError> {
    let origin = record
        .undo_origin
        .as_ref()
        .ok_or_else(|| stale(&record.source_path().unwrap_or_default()))?;
    origin.validate_inputs(
        &record.source_path()?,
        &record.target_path()?,
        record.source_identity,
        record.source_parent_identity,
        record.destination_parent_identity,
        record.conflict,
        record.destination_identity,
    )?;
    let destination = record.destination_parent_path()?;
    let transaction = record.transaction_path()?;
    let expected = destination.join(format!(".ottrin-move-{}", record.transaction_id));
    record.transaction_id()?;
    let kind = |identity: WindowsStableIdentity| {
        super::super::windows_cross_volume_source_kind(
            identity.is_regular_file_without_reparse(),
            identity.is_directory_without_reparse(),
            identity.is_symbolic_link(),
        )
    };
    if kind(record.source_identity) != Some(record.source_kind)
        || record.candidate_identity.is_some_and(|identity| {
            identity.volume_serial_number() != origin.original_parent.volume_serial_number()
                || kind(identity) != Some(record.source_kind)
        })
        || record.target_candidates.len() != 1
        || record.target_path()?.parent() != Some(destination.as_path())
        || transaction != expected
        || record.retained_displaced.is_some()
        || record.undo_transaction.is_some()
        || !record.destination_manifest.is_empty()
        || !record
            .transaction_directory_identity
            .is_directory_without_reparse()
        || record.transaction_directory_identity.volume_serial_number()
            != origin.original_parent.volume_serial_number()
        || record.candidate_path()?
            != transaction.join(super::super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME)
    {
        return Err(stale(&transaction));
    }
    Ok(())
}

/// Completion proof is independent of later public-name reuse. This routine
/// reads only; callers decide whether to converge a terminal paired-write gap.
fn terminal(root: &Path, record: &Record) -> Result<Option<(Record, bool)>, PlatformError> {
    validate_record(record)?;
    let transaction = record.transaction_path()?;
    if !matches!(record.phase, Phase::Published | Phase::SourceRemoved) {
        return Ok(None);
    }
    let private = super::super::windows_control::directory(
        root,
        record.transaction_directory_identity,
        &transaction,
    )?;
    let bytes =
        super::super::windows_control::read_local(&private)?.ok_or_else(|| stale(&transaction))?;
    let local = super::super::decode_windows_cross_volume_record(&bytes)?;
    if record.phase != Phase::SourceRemoved && local.phase != Phase::SourceRemoved {
        return Ok(None);
    }
    let mut canonical = record.clone();
    canonical.phase = Phase::SourceRemoved;
    let mut local_canonical = local.clone();
    local_canonical.phase = Phase::SourceRemoved;
    let source_parent = super::super::windows_control::directory(
        root,
        record.source_parent_identity,
        record
            .source_path()?
            .parent()
            .ok_or_else(|| stale(&transaction))?,
    )?;
    let claim = windows_directory_source_claim_path(Path::new(""), record)?;
    if !matches!(local.phase, Phase::Published | Phase::SourceRemoved)
        || canonical != local_canonical
        || record.candidate_identity.is_none()
        || observe(
            &private,
            OsStr::new(super::super::WINDOWS_CROSS_VOLUME_CANDIDATE_NAME),
        )?
        .is_some()
        || observe(&private, OsStr::new(WINDOWS_RETAINED_NAME))?.is_some()
        || observe(&source_parent, claim.as_os_str())?.is_some()
    {
        return Err(stale(&transaction));
    }
    Ok(Some((
        canonical,
        record.phase == Phase::SourceRemoved && local.phase == Phase::SourceRemoved,
    )))
}

pub(in crate::move_transaction) fn status(
    root: &Path,
    record: &Record,
) -> Result<move_recovery::WindowsMoveUndoStatus, PlatformError> {
    validate_record(record)?;
    Ok(
        if terminal(root, record)?.is_some_and(|(_, paired)| paired)
            && retained::inverse_complete(root, record)?
        {
            move_recovery::WindowsMoveUndoStatus::Complete
        } else {
            move_recovery::WindowsMoveUndoStatus::Pending
        },
    )
}

pub(in crate::move_transaction) fn completed(
    root: &Path,
    record: &mut Record,
) -> Result<Option<super::super::MovePublication>, PlatformError> {
    let Some((canonical, paired)) = terminal(root, record)? else {
        return Ok(None);
    };
    if !paired {
        *record = canonical;
        persist_windows_cross_volume_record(root, record).map_err(|e| pending(record, e))?;
    }
    retained::finish(root, record)?;
    super::super::windows_completion::cross(root, record).map_err(|e| pending(record, e))?;
    Ok(Some(super::super::MovePublication {
        windows_undo: None,
        disposition: crate::MoveDisposition::Moved,
        published_path: Some(record.target_path()?),
        retained_displaced_path: None,
    }))
}

/// Bind only the journal's pre-existing candidate identity; a current name is
/// never adopted as authority. Historical cleanup may see later names, in which
/// case it produces no session Undo receipt.
pub(in crate::move_transaction) fn issue_if_owned(
    record: &Record,
) -> Result<Option<PathMoveUndoReceipt>, PlatformError> {
    if record.undo_origin.is_some() || retained::has_intent(record) {
        return Ok(None);
    }
    let Some(identity) = record.candidate_identity else {
        return Ok(None);
    };
    let before = std::path::absolute(record.source_path()?).map_err(io)?;
    let after = std::path::absolute(record.target_path()?).map_err(io)?;
    if record.phase != Phase::SourceRemoved
        || observe_windows_identity(&after)? != Some(identity)
        || observe_windows_identity(before.parent().ok_or_else(|| stale(&before))?)?
            != Some(record.source_parent_identity)
        || observe_windows_identity(after.parent().ok_or_else(|| stale(&after))?)?
            != Some(record.destination_parent_identity)
    {
        return Ok(None);
    }
    let origin = Origin {
        forward_transaction_id: record.transaction_id.clone(),
        original: move_recovery::NativePath::encode(&before),
        original_parent: record.source_parent_identity,
        publication: move_recovery::NativePath::encode(&after),
        publication_parent: record.destination_parent_identity,
        publication_identity: identity,
        retained: if record.conflict == crate::ConflictAction::Overwrite {
            Some(retained::bind(record)?)
        } else {
            if record.retained_displaced.is_some() {
                return Err(stale(&after));
            }
            None
        },
    };
    origin.validate()?;
    let displaced_identity = origin.retained.as_ref().map(|binding| PathMoveIdentity {
        windows: binding.entry,
    });
    let retained_displaced_path = origin
        .retained
        .as_ref()
        .map(|binding| binding.retained_path())
        .transpose()?;
    Ok(Some(PathMoveUndoReceipt {
        windows_undo: Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(
            Box::new(Guard {
                origin,
                inverse: Arc::new(OnceLock::new()),
            }),
        ))),
        before_path: before,
        after_path: after,
        identity: PathMoveIdentity { windows: identity },
        displaced_identity,
        retained_displaced_path,
        requirement: PathMoveUndoRequirement::None,
    }))
}

fn parent(path: &Path, identity: WindowsStableIdentity) -> Result<File, PlatformError> {
    let file = windows_fs::open_directory_with_sharing(
        path,
        move_recovery::WINDOWS_DESTINATION_PARENT_ACCESS_MODE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .map_err(io)?;
    if move_recovery::stable_identity_from_handle(&file).map_err(io)? != identity
        || observe_windows_identity(path)? != Some(identity)
    {
        return Err(stale(path));
    }
    Ok(file)
}

pub(super) fn undo(receipt: &PathMoveUndoReceipt, guard: &Guard) -> Result<(), PlatformError> {
    guard.origin.validate()?;
    let before = path(&guard.origin.original)?;
    let after = path(&guard.origin.publication)?;
    if receipt.before_path != before
        || receipt.after_path != after
        || receipt.identity.windows != guard.origin.publication_identity
        || receipt
            .displaced_identity
            .as_ref()
            .map(|identity| identity.windows)
            != guard.origin.retained.as_ref().map(|binding| binding.entry)
        || receipt.retained_displaced_path
            != guard
                .origin
                .retained
                .as_ref()
                .map(|binding| binding.retained_path())
                .transpose()?
        || receipt.requirement != PathMoveUndoRequirement::None
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Cross-volume receipt disagrees with its native publication".into(),
        ));
    }
    let root = super::super::windows_recovery_root()?;
    if super::super::windows_completion::list(&root)?
        .iter()
        .any(|done| done.matches(receipt))
    {
        return Ok(());
    }
    if let Some(initial) = guard.inverse.get() {
        let central = windows_cross_volume_journal_path(&root, initial.transaction_id()?);
        let mut current = match std::fs::symlink_metadata(&central) {
            Ok(_) => load_windows_cross_volume_record(&central)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // A live owner may retry an intent write that failed before either
                // journal existed. Verify the exact private directory and empty
                // topology before publishing its already-bound Prepared record.
                if initial.phase != Phase::Prepared
                    || initial.candidate_identity.is_some()
                    || observe_windows_identity(&initial.transaction_path()?)?
                        != Some(initial.transaction_directory_identity)
                    || observe_windows_identity(&initial.candidate_path()?)?.is_some()
                    || initial
                        .transaction_path()?
                        .join(WINDOWS_LOCAL_JOURNAL_NAME)
                        .try_exists()
                        .map_err(io)?
                {
                    return Err(pending(initial, "Unpublished inverse intent changed"));
                }
                persist_windows_cross_volume_record(&root, initial)
                    .map_err(|e| pending(initial, e))?;
                initial.clone()
            }
            Err(error) => return Err(io(error)),
        };
        if current.undo_origin.as_ref() != Some(&guard.origin)
            || !initial.same_transaction_evidence(&current)
        {
            return Err(stale(&central));
        }
        return resume_windows_cross_volume(&root, &mut current, None)
            .map(|_| ())
            .map_err(|e| pending(initial, e));
    }
    let _publication_parent = parent(
        after.parent().ok_or_else(|| stale(&after))?,
        guard.origin.publication_parent,
    )?;
    let original_parent = parent(
        before.parent().ok_or_else(|| stale(&before))?,
        guard.origin.original_parent,
    )?;
    if observe(
        &original_parent,
        before.file_name().ok_or_else(|| stale(&before))?,
    )?
    .is_some()
    {
        return Err(PlatformError::DestinationExists(
            before.display().to_string(),
        ));
    }
    let source = move_recovery::open_reparse_safe_cross_volume_source(&after).map_err(io)?;
    if move_recovery::stable_identity_from_handle(&source).map_err(io)?
        != guard.origin.publication_identity
        || observe_windows_identity(&after)? != Some(guard.origin.publication_identity)
    {
        return Err(stale(&after));
    }
    let result = super::super::start_windows_cross_volume_move(
        &after,
        &before,
        crate::ConflictAction::Skip,
        source,
        original_parent,
        None,
        Some((&guard.origin, &guard.inverse)),
    );
    match result {
        Ok(publication) if publication.disposition == crate::MoveDisposition::Moved => Ok(()),
        Ok(_) => Err(PlatformError::DestinationExists(
            before.display().to_string(),
        )),
        Err(error) => match guard.inverse.get() {
            Some(initial) => Err(pending(initial, error)),
            None => Err(error),
        },
    }
}

#[cfg(test)]
mod offline_tests;
#[cfg(test)]
mod tests;

pub(in crate::move_transaction) mod retained;
