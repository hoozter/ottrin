//! Durable inverse of a same-volume Replace. Local intent precedes central intent,
//! and both precede any inverse rename. Forward recovery never consumes inverse intent.
use super::super::{
    WINDOWS_LOCAL_JOURNAL_NAME, WINDOWS_REPLACEMENT_NAME, WINDOWS_RETAINED_NAME,
    WindowsOverwritePhase as Phase, WindowsOverwriteRecord as Record,
    load_windows_overwrite_record, observe_windows_identity, persist_windows_overwrite_record,
    rename_windows_handle_no_replace, windows_journal_path,
};
use super::*;

pub(in crate::move_transaction) fn has_inverse_intent(record: &Record, local: &Record) -> bool {
    [record.phase, local.phase]
        .into_iter()
        .any(|phase| matches!(phase, Phase::Undoing | Phase::Undone))
}

fn pending(record: &Record, error: impl std::fmt::Display) -> PlatformError {
    PlatformError::CommittedUndoCleanupPending {
        transaction: record.transaction_id.clone(),
        message: format!(
            "Replace Undo needs attention; all un-restored entries remain retained: {error}"
        ),
    }
}

fn paired(record: &Record, local: &Record) -> Result<Phase, PlatformError> {
    if !record.same_transaction_evidence(local) {
        return Err(stale(&record.transaction_path()?));
    }
    // Central/local pairs are causal: persistence always publishes local first.
    match (record.phase, local.phase) {
        (Phase::Published, Phase::Published) => Ok(Phase::Published),
        (Phase::Published | Phase::Undoing, Phase::Undoing) => Ok(Phase::Undoing),
        (Phase::Undoing | Phase::Undone, Phase::Undone) => Ok(Phase::Undone),
        _ => Err(stale(&record.transaction_path()?)),
    }
}

fn topology(record: &Record) -> Result<(), PlatformError> {
    let source = record.source_path()?;
    let target = record.target_path()?;
    let destination = record.destination_parent_path()?;
    let transaction = record.transaction_path()?;
    let expected_name = format!(".ottrin-move-{}", record.transaction_id);
    record.transaction_id()?;
    if source == target
        || source.file_name().is_none()
        || target.parent() != Some(destination.as_path())
        || transaction.parent() != Some(destination.as_path())
        || transaction.file_name() != Some(OsStr::new(&expected_name))
        || record.replacement_path()? != transaction.join(WINDOWS_REPLACEMENT_NAME)
        || record.retained_path()? != transaction.join(WINDOWS_RETAINED_NAME)
        || record.retained_identity != record.destination_identity
        || !record.source_parent_identity.is_directory_without_reparse()
        || !record
            .destination_parent_identity
            .is_directory_without_reparse()
        || !record
            .transaction_directory_identity
            .is_directory_without_reparse()
        || super::super::windows_same_volume_kind_from_identity(record.source_identity).is_none()
        || super::super::windows_same_volume_kind_from_identity(record.destination_identity)
            .is_none()
        || [
            record.source_parent_identity,
            record.destination_parent_identity,
            record.transaction_directory_identity,
            record.destination_identity,
        ]
        .into_iter()
        .any(|id| id.volume_serial_number() != record.source_identity.volume_serial_number())
    {
        return Err(stale(&transaction));
    }
    for name in [source.file_name().unwrap(), target.file_name().unwrap()] {
        windows_fs::valid_component(name).map_err(io)?;
    }
    Ok(())
}

fn open_parent(path: &Path, expected: WindowsStableIdentity) -> Result<File, PlatformError> {
    let parent = windows_fs::open_directory_with_sharing(
        path,
        move_recovery::WINDOWS_DESTINATION_PARENT_ACCESS_MODE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )
    .map_err(io)?;
    if move_recovery::stable_identity_from_handle(&parent).map_err(io)? != expected
        || move_recovery::stable_identity_of_entry(path).map_err(io)? != expected
    {
        return Err(stale(path));
    }
    Ok(parent)
}

fn open_entry(
    parent: &File,
    name: &OsStr,
    expected: WindowsStableIdentity,
) -> Result<File, PlatformError> {
    let file = unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: None,
                create_new: false,
                access: DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)?;
    if move_recovery::stable_identity_from_handle(&file).map_err(io)? != expected
        || observe(parent, name)? != Some(expected)
    {
        return Err(PlatformError::StalePathMoveUndo(
            name.to_string_lossy().into_owned(),
        ));
    }
    Ok(file)
}

/// The completed forward journal supplies the original identities; current names
/// are only checked against that authority, never adopted into a new receipt.
pub(in crate::move_transaction) fn issue(
    record: &Record,
) -> Result<PathMoveUndoReceipt, PlatformError> {
    let verify = || -> Result<PathMoveUndoReceipt, PlatformError> {
        topology(record)?;
        let transaction = record.transaction_path()?;
        let local = load_windows_overwrite_record(&transaction.join(WINDOWS_LOCAL_JOURNAL_NAME))?;
        let central = load_windows_overwrite_record(&windows_journal_path(
            &super::super::windows_recovery_root()?,
            record.transaction_id()?,
        ))?;
        if central != *record
            || paired(record, &local)? != Phase::Published
            || observe_windows_identity(&transaction)?
                != Some(record.transaction_directory_identity)
            || observe_windows_identity(&record.replacement_path()?)?.is_some()
            || observe_windows_identity(&record.retained_path()?)? != Some(record.retained_identity)
        {
            return Err(stale(&transaction));
        }
        let mut receipt = super::issue(
            &record.source_path()?,
            &record.target_path()?,
            record.source_identity,
            record.source_parent_identity,
            record.destination_parent_identity,
        )?;
        receipt.windows_undo = Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::Replace(
            Box::new(record.clone()),
        )));
        receipt.displaced_identity = Some(PathMoveIdentity {
            windows: record.retained_identity,
        });
        receipt.retained_displaced_path =
            Some(std::path::absolute(record.retained_path()?).map_err(io)?);
        Ok(receipt)
    };
    verify().map_err(|error| PlatformError::CommittedCleanupPending {
        transaction: record.transaction_id.clone(),
        message: format!(
            "Replace completed, but its Undo ownership could not be verified: {error}"
        ),
    })
}

pub(in crate::move_transaction) fn undo(
    receipt: &PathMoveUndoReceipt,
    expected: &Record,
) -> Result<(), PlatformError> {
    topology(expected)?;
    if receipt.before_path != std::path::absolute(expected.source_path()?).map_err(io)?
        || receipt.after_path != std::path::absolute(expected.target_path()?).map_err(io)?
        || receipt.identity.windows != expected.source_identity
        || receipt.displaced_identity.as_ref().map(|id| id.windows)
            != Some(expected.retained_identity)
        || receipt.retained_displaced_path.as_ref()
            != Some(&std::path::absolute(expected.retained_path()?).map_err(io)?)
        || receipt.requirement != PathMoveUndoRequirement::None
    {
        return Err(PlatformError::InvalidPathMoveUndoReceipt(
            "Replace inverse receipt disagrees with its publication".into(),
        ));
    }
    let root = super::super::windows_recovery_root()?;
    let record =
        load_windows_overwrite_record(&windows_journal_path(&root, expected.transaction_id()?))?;
    let local = load_windows_overwrite_record(
        &record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME),
    )?;
    if !record.same_transaction_evidence(expected) || !local.same_transaction_evidence(expected) {
        return Err(stale(&record.transaction_path()?));
    }
    resume(&root, record, local)
}

/// Read-only terminal proof releases public names without probing or mutating
/// later entries under them. The paired terminal events prove both flushes.
pub(in crate::move_transaction) fn completed(
    _root: &Path,
    record: &Record,
    local: &Record,
) -> Result<bool, PlatformError> {
    topology(record)?;
    if paired(record, local)? != Phase::Undone {
        return Ok(false);
    }
    let transaction = record.transaction_path()?;
    let parent = open_parent(&transaction, record.transaction_directory_identity)?;
    if observe(&parent, OsStr::new(WINDOWS_REPLACEMENT_NAME))?.is_some()
        || observe(&parent, OsStr::new(WINDOWS_RETAINED_NAME))?.is_some()
    {
        return Err(stale(&transaction));
    }
    Ok(record.phase == Phase::Undone && local.phase == Phase::Undone)
}

/// Caller holds the shared Windows recovery lock. No physical inverse starts
/// until both journals carry intent. A later terminal event cannot route forward.
pub(in crate::move_transaction) fn resume(
    root: &Path,
    mut record: Record,
    local: Record,
) -> Result<(), PlatformError> {
    topology(&record)?;
    let phase = paired(&record, &local)?;
    let source = record.source_path()?;
    let target = record.target_path()?;
    let transaction = record.transaction_path()?;
    let private_parent = open_parent(&transaction, record.transaction_directory_identity)?;
    if observe(&private_parent, OsStr::new(WINDOWS_REPLACEMENT_NAME))?.is_some() {
        return Err(stale(&transaction));
    }
    if phase == Phase::Undone {
        if observe(&private_parent, OsStr::new(WINDOWS_RETAINED_NAME))?.is_some() {
            return Err(stale(&transaction));
        }
        record.phase = Phase::Undone;
        return persist_windows_overwrite_record(root, &record).map_err(|e| pending(&record, e));
    }
    let before_parent = open_parent(
        source.parent().ok_or_else(|| stale(&source))?,
        record.source_parent_identity,
    )?;
    let after_parent = open_parent(
        &record.destination_parent_path()?,
        record.destination_parent_identity,
    )?;
    let before_name = source.file_name().ok_or_else(|| stale(&source))?;
    let after_name = target.file_name().ok_or_else(|| stale(&target))?;
    let original = observe(&before_parent, before_name)?;
    let published = observe(&after_parent, after_name)?;
    let retained = observe(&private_parent, OsStr::new(WINDOWS_RETAINED_NAME))?;
    let start = original.is_none()
        && published == Some(record.source_identity)
        && retained == Some(record.retained_identity);
    let source_restored = original == Some(record.source_identity)
        && published.is_none()
        && retained == Some(record.retained_identity);
    let all_restored = original == Some(record.source_identity)
        && published == Some(record.retained_identity)
        && retained.is_none();
    if phase == Phase::Published && !start {
        if original.is_some() {
            return Err(PlatformError::DestinationExists(
                source.display().to_string(),
            ));
        }
        return Err(stale(&target));
    }
    if !start && !source_restored && !all_restored {
        return Err(pending(
            &record,
            "Inverse identities or original names changed",
        ));
    }
    // Acquire exact entry handles before intent and hold them across both renames.
    let source_handle = if start {
        open_entry(&after_parent, after_name, record.source_identity)?
    } else {
        open_entry(&before_parent, before_name, record.source_identity)?
    };
    let retained_handle = if !all_restored {
        Some(open_entry(
            &private_parent,
            OsStr::new(WINDOWS_RETAINED_NAME),
            record.retained_identity,
        )?)
    } else {
        None
    };
    record.phase = Phase::Undoing;
    persist_windows_overwrite_record(root, &record).map_err(|e| pending(&record, e))?;
    #[cfg(test)]
    super::super::windows_move_boundary(
        super::super::WindowsMoveBoundary::OverwriteUndoIntent,
        &source,
    );
    if start {
        rename_windows_handle_no_replace(&source_handle, &before_parent, before_name)
            .map_err(|e| pending(&record, e))?;
        before_parent
            .sync_all()
            .and_then(|()| after_parent.sync_all())
            .map_err(|e| pending(&record, e))?;
        #[cfg(test)]
        super::super::windows_move_boundary(
            super::super::WindowsMoveBoundary::OverwriteUndoSourceRestored,
            &source,
        );
    }
    if let Some(retained_handle) = retained_handle {
        // The restored source is still pinned; a new target is never overwritten.
        rename_windows_handle_no_replace(&retained_handle, &after_parent, after_name)
            .map_err(|e| pending(&record, e))?;
    }
    after_parent
        .sync_all()
        .and_then(|()| private_parent.sync_all())
        .and_then(|()| before_parent.sync_all())
        .map_err(|e| pending(&record, e))?;
    #[cfg(test)]
    super::super::windows_move_boundary(
        super::super::WindowsMoveBoundary::OverwriteUndoDisplacedRestored,
        &target,
    );
    record.phase = Phase::Undone;
    persist_windows_overwrite_record(root, &record).map_err(|e| pending(&record, e))?;
    #[cfg(test)]
    super::super::windows_move_boundary(
        super::super::WindowsMoveBoundary::OverwriteUndoComplete,
        &target,
    );
    Ok(())
}

#[cfg(test)]
mod tests;
