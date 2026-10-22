//! The original retained forward record controls the second half of Replace Undo.
//! Its inverse ID is durable before copying back. Paired Undone events follow
//! restoration of the exact retained entry; no terminal public name is replayed.
use super::*;
#[cfg(test)]
use crate::move_transaction::WindowsMoveBoundary;
use crate::move_transaction::{
    WINDOWS_CROSS_VOLUME_CANDIDATE_NAME, rename_windows_handle_no_replace,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::move_transaction) struct Binding {
    directory: move_recovery::NativePath,
    identity: WindowsStableIdentity,
    pub(super) entry: WindowsStableIdentity,
}
impl Binding {
    pub(super) fn directory_identity(&self) -> WindowsStableIdentity {
        self.identity
    }

    pub(super) fn retained_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        Ok(path(&self.directory)?.join(WINDOWS_RETAINED_NAME))
    }
    pub(super) fn validate(&self, origin: &Origin) -> Result<(), PlatformError> {
        let directory = path(&self.directory)?;
        let publication = path(&origin.publication)?;
        if directory.parent() != publication.parent()
            || directory.file_name()
                != Some(OsStr::new(&format!(
                    ".ottrin-move-{}",
                    origin.forward_transaction_id
                )))
            || !self.identity.is_directory_without_reparse()
            || self.identity.volume_serial_number()
                != origin.publication_parent.volume_serial_number()
            || self.entry.volume_serial_number() != origin.publication_parent.volume_serial_number()
            || super::super::super::windows_same_volume_kind_from_identity(self.entry).is_none()
        {
            return Err(stale(&directory));
        }
        Ok(())
    }
}

pub(in crate::move_transaction) fn has_intent(record: &Record) -> bool {
    record.undo_transaction.is_some() || matches!(record.phase, Phase::Undoing | Phase::Undone)
}

fn canonical(record: &Record) -> Record {
    let mut value = record.clone();
    value.phase = Phase::SourceRemoved;
    value.undo_transaction = None;
    value
}
fn pair(root: &Path, origin: &Origin) -> Result<(Record, bool), PlatformError> {
    let binding = origin.retained.as_ref().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt("Replace inverse has no retained binding".into())
    })?;
    binding.validate(origin)?;
    let id = super::super::super::parse_windows_journal_name(OsStr::new(&format!(
        "{}.cross-journal",
        origin.forward_transaction_id
    )))
    .ok_or_else(|| stale(&path(&binding.directory).unwrap_or_default()))?
    .0;
    let central = load_windows_cross_volume_record(&windows_cross_volume_journal_path(root, id))?;
    let directory = path(&binding.directory)?;
    let held = super::super::super::windows_control::directory(
        root,
        binding.identity,
        &path(&binding.directory)?,
    )?;
    let bytes = super::super::super::windows_control::read_local(&held)?
        .ok_or_else(|| stale(&directory))?;
    let local = super::super::super::decode_windows_cross_volume_record(&bytes)?;
    let before = path(&origin.original)?;
    let after = path(&origin.publication)?;
    if canonical(&central) != canonical(&local)
        || central.transaction_id()? != id
        || central.undo_origin.is_some()
        || central.conflict != crate::ConflictAction::Overwrite
        || std::path::absolute(central.source_path()?).map_err(io)? != before
        || std::path::absolute(central.target_path()?).map_err(io)? != after
        || central.source_parent_identity != origin.original_parent
        || central.destination_parent_identity != origin.publication_parent
        || central.candidate_identity != Some(origin.publication_identity)
        || central.destination_identity != Some(binding.entry)
        || central.transaction_directory_identity != binding.identity
        || std::path::absolute(central.transaction_path()?).map_err(io)? != directory
        || central
            .retained_path()?
            .map(|p| std::path::absolute(p).map_err(io))
            .transpose()?
            != Some(binding.retained_path()?)
        || central.candidate_path()?
            != central
                .transaction_path()?
                .join(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME)
    {
        return Err(stale(&directory));
    }
    let allowed = match (
        central.phase,
        local.phase,
        central.undo_transaction,
        local.undo_transaction,
    ) {
        (Phase::SourceRemoved, Phase::SourceRemoved, None, None) => true,
        (Phase::Undoing, Phase::SourceRemoved, Some(_), None) => true,
        (Phase::Undoing, Phase::Undoing, Some(a), Some(b)) => a == b,
        (Phase::Undone, Phase::Undoing | Phase::Undone, Some(a), Some(b)) => a == b,
        _ => false,
    };
    if !allowed {
        return Err(stale(&directory));
    }
    if observe(&held, OsStr::new(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME))?.is_some() {
        return Err(stale(&directory));
    }
    Ok((central, local.phase == Phase::Undone))
}

pub(super) fn bind(record: &Record) -> Result<Binding, PlatformError> {
    if record.phase != Phase::SourceRemoved
        || record.undo_transaction.is_some()
        || record.undo_origin.is_some()
        || record.conflict != crate::ConflictAction::Overwrite
    {
        return Err(stale(&record.transaction_path()?));
    }
    let local = load_windows_cross_volume_record(
        &record.transaction_path()?.join(WINDOWS_LOCAL_JOURNAL_NAME),
    )?;
    let retained = record
        .retained_path()?
        .ok_or_else(|| stale(&record.transaction_path().unwrap_or_default()))?;
    let entry = record
        .destination_identity
        .ok_or_else(|| stale(&retained))?;
    if local != *record
        || observe_windows_identity(&record.transaction_path()?)?
            != Some(record.transaction_directory_identity)
        || observe_windows_identity(&retained)? != Some(entry)
        || observe_windows_identity(&record.candidate_path()?)?.is_some()
    {
        return Err(stale(&retained));
    }
    Ok(Binding {
        directory: move_recovery::NativePath::encode(
            &std::path::absolute(record.transaction_path()?).map_err(io)?,
        ),
        identity: record.transaction_directory_identity,
        entry,
    })
}

fn origin(inverse: &Record) -> Result<&Origin, PlatformError> {
    inverse.undo_origin.as_ref().ok_or_else(|| {
        PlatformError::InvalidPathMoveUndoReceipt("Cross-volume inverse origin is missing".into())
    })
}
fn bound_controller(
    root: &Path,
    inverse: &Record,
) -> Result<Option<(Record, bool)>, PlatformError> {
    let origin = origin(inverse)?;
    if origin.retained.is_none() {
        return Ok(None);
    }
    let (controller, paired_done) = pair(root, origin)?;
    if controller
        .undo_transaction
        .is_some_and(|id| Some(id) != inverse.transaction_id().ok())
    {
        return Err(stale(&controller.transaction_path()?));
    }
    Ok(Some((controller, paired_done)))
}

pub(in crate::move_transaction) fn presentation_controller(
    root: &Path,
    inverse: &Record,
) -> Result<Option<[u8; 16]>, PlatformError> {
    bound_controller(root, inverse)?
        .map(|(record, _)| record.transaction_id())
        .transpose()
}

pub(in crate::move_transaction) fn prepare(
    root: &Path,
    inverse: &Record,
) -> Result<(), PlatformError> {
    let Some((mut controller, paired_done)) = bound_controller(root, inverse)? else {
        return Ok(());
    };
    if controller.phase == Phase::Undone {
        if terminal(root, inverse)?.is_none() {
            return Err(pending(
                inverse,
                "Terminal controller has no terminal inverse proof",
            ));
        }
        if !paired_done {
            persist_windows_cross_volume_record(root, &controller)
                .map_err(|e| pending(inverse, e))?;
        }
        return Ok(());
    }
    let origin = origin(inverse)?;
    let binding = origin.retained.as_ref().unwrap();
    if terminal(root, inverse)?.is_none()
        && observe_windows_identity(&binding.retained_path()?)? != Some(binding.entry)
    {
        return Err(stale(&binding.retained_path()?));
    }
    if controller.phase == Phase::SourceRemoved {
        if inverse.phase != Phase::Prepared
            || inverse.candidate_identity.is_some()
            || observe_windows_identity(&inverse.candidate_path()?)?.is_some()
        {
            return Err(pending(
                inverse,
                "Payload cannot precede retained Undo intent",
            ));
        }
        controller.phase = Phase::Undoing;
        controller.undo_transaction = Some(inverse.transaction_id()?);
    }
    // Converge a central-first controller write gap before payload mutation.
    persist_windows_cross_volume_record(root, &controller).map_err(|e| pending(inverse, e))?;
    #[cfg(test)]
    super::super::super::windows_move_boundary(
        WindowsMoveBoundary::CrossRetainedUndoIntent,
        &controller.transaction_path()?,
    );
    Ok(())
}

fn pair_complete(
    root: &Path,
    inverse: &Record,
    controller: &Record,
    paired_done: bool,
) -> Result<bool, PlatformError> {
    if controller.phase != Phase::Undone || !paired_done {
        return Ok(false);
    }
    let binding = origin(inverse)?.retained.as_ref().unwrap();
    let held = super::super::super::windows_control::directory(
        root,
        binding.identity,
        &path(&binding.directory)?,
    )?;
    if observe(&held, OsStr::new(WINDOWS_RETAINED_NAME))?.is_some() {
        return Err(stale(&binding.retained_path()?));
    }
    Ok(true)
}
pub(super) fn inverse_complete(root: &Path, inverse: &Record) -> Result<bool, PlatformError> {
    let Some((controller, paired)) = bound_controller(root, inverse)? else {
        return Ok(true);
    };
    pair_complete(root, inverse, &controller, paired)
}
pub(in crate::move_transaction) fn completed_control(
    root: &Path,
    inverse: &Record,
) -> Result<Option<Record>, PlatformError> {
    let Some((controller, paired)) = bound_controller(root, inverse)? else {
        return Ok(None);
    };
    if !pair_complete(root, inverse, &controller, paired)? {
        return Err(stale(&controller.transaction_path()?));
    }
    Ok(Some(controller))
}

fn pin(
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
                // Attribute-only handles do not participate in Windows delete sharing.
                // Request DELETE so denying FILE_SHARE_DELETE actually pins this name.
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

pub(super) fn finish(root: &Path, inverse: &Record) -> Result<(), PlatformError> {
    let Some((mut controller, paired_done)) = bound_controller(root, inverse)? else {
        return Ok(());
    };
    let origin = origin(inverse)?;
    let binding = origin.retained.as_ref().unwrap();
    if controller.phase == Phase::Undone {
        let held = super::super::super::windows_control::directory(
            root,
            binding.identity,
            &path(&binding.directory)?,
        )?;
        if observe(&held, OsStr::new(WINDOWS_RETAINED_NAME))?.is_some() {
            return Err(stale(&binding.retained_path()?));
        }
        if !paired_done {
            persist_windows_cross_volume_record(root, &controller)
                .map_err(|e| pending(inverse, e))?;
        }
        return Ok(());
    }
    if controller.phase != Phase::Undoing
        || controller.undo_transaction != Some(inverse.transaction_id()?)
    {
        return Err(pending(
            inverse,
            "Retained controller does not own this inverse",
        ));
    }
    let original = path(&origin.original)?;
    let publication = path(&origin.publication)?;
    let original_parent = parent(
        original.parent().ok_or_else(|| stale(&original))?,
        origin.original_parent,
    )?;
    let restored_id = inverse
        .candidate_identity
        .ok_or_else(|| pending(inverse, "Restored candidate identity is missing"))?;
    let _restored = pin(
        &original_parent,
        original.file_name().ok_or_else(|| stale(&original))?,
        restored_id,
    )?;
    let destination = parent(
        publication.parent().ok_or_else(|| stale(&publication))?,
        origin.publication_parent,
    )?;
    let private = parent(&path(&binding.directory)?, binding.identity)?;
    let target_name = publication.file_name().ok_or_else(|| stale(&publication))?;
    #[cfg(test)]
    super::super::super::windows_move_boundary(
        WindowsMoveBoundary::CrossRetainedBeforeRestore,
        &publication,
    );
    match (
        observe(&private, OsStr::new(WINDOWS_RETAINED_NAME))?,
        observe(&destination, target_name)?,
    ) {
        (Some(id), None) if id == binding.entry => {
            let entry = pin(&private, OsStr::new(WINDOWS_RETAINED_NAME), binding.entry)?;
            rename_windows_handle_no_replace(&entry, &destination, target_name)
                .map_err(|e| pending(inverse, e))?;
        }
        (None, Some(id)) if id == binding.entry => {
            let _entry = pin(&destination, target_name, binding.entry)?;
        }
        _ => {
            return Err(pending(
                inverse,
                "Retained entry or target name changed; no entry was replaced",
            ));
        }
    }
    private
        .sync_all()
        .and_then(|()| destination.sync_all())
        .and_then(|()| original_parent.sync_all())
        .map_err(|e| pending(inverse, e))?;
    #[cfg(test)]
    super::super::super::windows_move_boundary(
        WindowsMoveBoundary::CrossRetainedRestored,
        &publication,
    );
    controller.phase = Phase::Undone;
    persist_windows_cross_volume_record(root, &controller).map_err(|e| pending(inverse, e))?;
    #[cfg(test)]
    super::super::super::windows_move_boundary(
        WindowsMoveBoundary::CrossRetainedUndoComplete,
        &publication,
    );
    Ok(())
}

pub(in crate::move_transaction) fn controller_status(
    root: &Path,
    record: &Record,
) -> Result<move_recovery::WindowsMoveUndoStatus, PlatformError> {
    let inverse_id = record
        .undo_transaction
        .ok_or_else(|| stale(&record.transaction_path().unwrap_or_default()))?;
    let inverse =
        load_windows_cross_volume_record(&windows_cross_volume_journal_path(root, inverse_id))?;
    validate_record(&inverse)?;
    let Some((controller, paired_done)) = bound_controller(root, &inverse)? else {
        return Err(stale(&record.transaction_path()?));
    };
    if controller.transaction_id()? != record.transaction_id()? {
        return Err(stale(&record.transaction_path()?));
    }
    if controller.phase == Phase::Undone
        && paired_done
        && terminal(root, &inverse)?.is_some_and(|(_, paired)| paired)
        && inverse_complete(root, &inverse)?
    {
        Ok(move_recovery::WindowsMoveUndoStatus::Complete)
    } else {
        Ok(move_recovery::WindowsMoveUndoStatus::Pending)
    }
}

pub(in crate::move_transaction) fn resume_controller(
    root: &Path,
    record: &mut Record,
) -> Result<super::super::super::MovePublication, PlatformError> {
    let inverse_id = record
        .undo_transaction
        .ok_or_else(|| stale(&record.transaction_path().unwrap_or_default()))?;
    let mut inverse =
        load_windows_cross_volume_record(&windows_cross_volume_journal_path(root, inverse_id))?;
    let Some((controller, _)) = bound_controller(root, &inverse)? else {
        return Err(stale(&record.transaction_path()?));
    };
    if controller.transaction_id()? != record.transaction_id()? {
        return Err(stale(&record.transaction_path()?));
    }
    let result = resume_windows_cross_volume(root, &mut inverse, None)?;
    *record = load_windows_cross_volume_record(&windows_cross_volume_journal_path(
        root,
        record.transaction_id()?,
    ))?;
    Ok(result)
}

#[cfg(test)]
mod tests;
