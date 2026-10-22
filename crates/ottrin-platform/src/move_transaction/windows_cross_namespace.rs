//! Current native names for a cross-volume transaction's immutable labels.
use super::*;
use windows_control::DirectoryBinding;

fn invalid() -> PlatformError {
    PlatformError::Io("Cross-volume Move namespace disagrees with its native bindings".into())
}

pub(super) fn validate(record: &WindowsCrossVolumeRecord) -> Result<(), PlatformError> {
    let source = record.source_path()?;
    let destination = record.destination_parent_path()?;
    let transaction = record.transaction_path()?;
    let private_name = format!(
        ".ottrin-move-{}",
        encode_transaction_id(record.transaction_id()?)
    );
    if !source.is_absolute()
        || source.file_name().is_none()
        || !destination.is_absolute()
        || transaction != destination.join(private_name)
        || record.candidate_path()? != transaction.join(WINDOWS_CROSS_VOLUME_CANDIDATE_NAME)
        || record
            .retained_path()?
            .is_some_and(|path| path != transaction.join(WINDOWS_RETAINED_NAME))
        || record.target_candidates.is_empty()
        || !record.source_parent_identity.is_directory_without_reparse()
        || !record
            .destination_parent_identity
            .is_directory_without_reparse()
        || !record
            .transaction_directory_identity
            .is_directory_without_reparse()
        || record.source_identity.volume_serial_number()
            != record.source_parent_identity.volume_serial_number()
        || record.transaction_directory_identity.volume_serial_number()
            != record.destination_parent_identity.volume_serial_number()
        || record.source_parent_identity.volume_serial_number()
            == record.destination_parent_identity.volume_serial_number()
    {
        return Err(invalid());
    }
    crate::windows_fs::valid_component(source.file_name().unwrap())
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    for target in &record.target_candidates {
        let target = target
            .decode()
            .map_err(|error| PlatformError::Io(error.to_string()))?;
        if target.parent() != Some(destination.as_path()) {
            return Err(invalid());
        }
        crate::windows_fs::valid_component(target.file_name().ok_or_else(invalid)?)
            .map_err(|error| PlatformError::Io(error.to_string()))?;
    }
    Ok(())
}

pub(super) fn parents(
    root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<[DirectoryBinding; 2], PlatformError> {
    validate(record)?;
    Ok([
        DirectoryBinding::capture(
            root,
            record.source_path()?.parent().ok_or_else(invalid)?,
            record.source_parent_identity,
        )?,
        DirectoryBinding::capture(
            root,
            &record.destination_parent_path()?,
            record.destination_parent_identity,
        )?,
    ])
}

fn private(
    root: &Path,
    record: &WindowsCrossVolumeRecord,
    destination: &DirectoryBinding,
) -> Result<DirectoryBinding, PlatformError> {
    let binding = DirectoryBinding::capture(
        root,
        &record.transaction_path()?,
        record.transaction_directory_identity,
    )?;
    let name = format!(
        ".ottrin-move-{}",
        encode_transaction_id(record.transaction_id()?)
    );
    // A containing-parent rename is allowed. Moving the private directory away
    // from its original native parent or canonical leaf is not.
    if binding.path != destination.path.join(name) {
        return Err(invalid());
    }
    Ok(binding)
}

pub(super) fn capture(
    root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<[DirectoryBinding; 3], PlatformError> {
    let [source, destination] = parents(root, record)?;
    let private = private(root, record, &destination)?;
    Ok([source, destination, private])
}

/// Journal writes need only the destination and its original private child.
/// Do not require former source volumes just to converge terminal evidence.
pub(super) fn pin_destination(
    root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<(std::fs::File, std::fs::File), PlatformError> {
    validate(record)?;
    let destination = DirectoryBinding::capture(
        root,
        &record.destination_parent_path()?,
        record.destination_parent_identity,
    )?;
    let private = private(root, record, &destination)?;
    let destination = destination.pin()?;
    let private = private.pin()?;
    Ok((destination, private))
}

/// Absence of a historical name is insufficient: the original ID must be
/// retired on the original, currently open destination volume as well.
pub(super) fn private_retired(
    parent: &std::fs::File,
    identity: move_recovery::WindowsStableIdentity,
) -> Result<bool, PlatformError> {
    let parent_identity = move_recovery::stable_identity_from_handle(parent)
        .map_err(|error| PlatformError::Io(error.to_string()))?;
    if !identity.is_directory_without_reparse()
        || !parent_identity.is_directory_without_reparse()
        || identity.volume_serial_number() != parent_identity.volume_serial_number()
    {
        return Err(invalid());
    }
    Ok(crate::windows_fs::Volumes::new(parent)?
        .resolve_optional(
            identity,
            windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES
                | windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE,
            &mut |_| Ok(()),
        )?
        .is_none())
}
