//! Immutable completion receipts replace old payload-dependent Undo observations.
//! The checksum detects damage; native registry/file identities reject copied
//! receipts. This is local owned state, not a cryptographic signature.
use super::*;
use crate::{PathMoveUndoReceipt, WindowsPathUndo, windows_fs};
use move_recovery::WindowsStableIdentity as Identity;
use sha2::{Digest, Sha256};
use std::{ffi::OsStr, fs::File};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_DISPOSITION_FLAG_ON_CLOSE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
};

pub(in crate::move_transaction) mod controls;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::move_transaction) struct Prepared {
    binding: Binding,
    controls: Vec<controls::Owner>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
enum Binding {
    Same(Box<WindowsOverwriteRecord>),
    Cross {
        origin: Box<windows_undo::cross::Origin>,
        inverse: [u8; 16],
        private: Identity,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    version: u16,
    root: Identity,
    file: Identity,
    binding: Binding,
    controls: Vec<controls::Owner>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    body: Body,
    checksum: [u8; 32],
}
fn io(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Move completion receipt: {e}"))
}
fn checksum(body: &Body) -> Result<[u8; 32], PlatformError> {
    let mut hash = Sha256::new();
    hash.update(b"Ottrin Windows Move completion v2\0");
    hash.update(serde_json::to_vec(body).map_err(io)?);
    Ok(hash.finalize().into())
}
fn name(id: [u8; 16]) -> std::ffi::OsString {
    format!("{}.move-done", encode_transaction_id(id)).into()
}
impl Binding {
    fn id(&self) -> Result<[u8; 16], PlatformError> {
        match self {
            Self::Same(record) => record.transaction_id(),
            Self::Cross { inverse, .. } => Ok(*inverse),
        }
    }
    fn controller(&self) -> Result<Option<[u8; 16]>, PlatformError> {
        match self {
            Self::Same(_) => Ok(None),
            Self::Cross { origin, .. } => origin.completion_controller(),
        }
    }
    fn validate(&self) -> Result<(), PlatformError> {
        match self {
            Self::Same(record) => {
                windows_undo::overwrite::topology(record)?;
                if record.phase != WindowsOverwritePhase::Undone
                    || record.schema_major != move_recovery::JOURNAL_SCHEMA_MAJOR
                    || record.schema_minor > move_recovery::JOURNAL_SCHEMA_MINOR
                {
                    return Err(io("same-volume Undo is not complete"));
                }
            }
            Self::Cross {
                origin,
                inverse,
                private,
            } => {
                origin.validate()?;
                if !private.is_directory_without_reparse()
                    || private.volume_serial_number() != origin.inverse_volume()
                    || origin.forward_id()? == *inverse
                {
                    return Err(io("invalid inverse ownership"));
                }
            }
        }
        Ok(())
    }
    fn kind(&self) -> WindowsJournalKind {
        match self {
            Self::Same(_) => WindowsJournalKind::SameVolumeOverwrite,
            Self::Cross { .. } => WindowsJournalKind::CrossVolume,
        }
    }
}
impl Receipt {
    fn validate_controls(&self) -> Result<(), PlatformError> {
        let mut owners = std::collections::BTreeSet::new();
        for owner in &self.body.controls {
            owner.validate(self.body.root)?;
            if !self.covers(owner.transaction, owner.kind)? || !owners.insert(owner.transaction) {
                return Err(io("completion control owners disagree"));
            }
            let matches = match &self.body.binding {
                Binding::Same(record) => {
                    owner.transaction == record.transaction_id()?
                        && owner.identity == record.transaction_directory_identity
                        && owner.directory == record.transaction_directory
                        && owner.parent == record.destination_parent
                        && owner.parent_identity == record.destination_parent_identity
                }
                Binding::Cross {
                    origin,
                    inverse,
                    private,
                } => origin.matches_control(owner, *inverse, *private)?,
            };
            if !matches {
                return Err(io("completion control topology disagrees"));
            }
        }
        let expected = 1 + usize::from(self.body.binding.controller()?.is_some());
        if owners.len() != expected {
            return Err(io("completion control evidence is incomplete"));
        }
        Ok(())
    }
    pub(super) fn id(&self) -> Result<[u8; 16], PlatformError> {
        self.body.binding.id()
    }
    pub(super) fn covers(
        &self,
        id: [u8; 16],
        kind: WindowsJournalKind,
    ) -> Result<bool, PlatformError> {
        let owns = self.id()? == id || self.body.binding.controller()? == Some(id);
        if owns && self.body.binding.kind() != kind {
            return Err(io(
                "transaction ID also belongs to a different journal family",
            ));
        }
        Ok(owns)
    }
    pub(super) fn owns(&self, id: [u8; 16]) -> Result<bool, PlatformError> {
        Ok(self.id()? == id || self.body.binding.controller()? == Some(id))
    }
    pub(super) fn matches(&self, receipt: &PathMoveUndoReceipt) -> bool {
        use windows_undo::WindowsMoveUndoGuard;
        match (&self.body.binding, receipt.windows_undo.as_ref()) {
            (
                Binding::Same(record),
                Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::Replace(expected))),
            ) => record.same_transaction_evidence(expected),
            (
                Binding::Cross {
                    origin,
                    inverse,
                    private,
                },
                Some(WindowsPathUndo::Move(WindowsMoveUndoGuard::CrossVolume(guard))),
            ) => guard.matches_completion(origin, *inverse, *private),
            _ => false,
        }
    }
    pub(super) fn discovery(
        &self,
    ) -> Result<move_recovery::WindowsDiscoveredMoveTransaction, PlatformError> {
        Ok(move_recovery::WindowsDiscoveredMoveTransaction {
            transaction_id: self.id()?,
            kind: match self.body.binding.kind() {
                WindowsJournalKind::SameVolumeOverwrite => {
                    move_recovery::WindowsMoveTransactionKind::SameVolumeOverwrite
                }
                WindowsJournalKind::CrossVolume => {
                    move_recovery::WindowsMoveTransactionKind::CrossVolume
                }
            },
            undo_status: move_recovery::WindowsMoveUndoStatus::Complete,
        })
    }
}
fn read(parent: &File, id: [u8; 16]) -> Result<Option<Receipt>, PlatformError> {
    let mut file = match unsafe {
        windows_fs::open_relative(
            parent,
            &name(id),
            windows_fs::RelativeOptions {
                directory: Some(false),
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    } {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    let identity = move_recovery::stable_identity_from_handle(&file).map_err(io)?;
    if !identity.is_regular_file_without_reparse() {
        return Err(io("expected an ordinary receipt file"));
    }
    const MAX_BYTES: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io("receipt exceeds size limit"));
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(io)?;
    if receipt.body.version != 2
        || receipt.body.root != move_recovery::stable_identity_from_handle(parent).map_err(io)?
        || receipt.body.file != identity
        || receipt.id()? != id
        || receipt.checksum != checksum(&receipt.body)?
    {
        return Err(io("receipt identity or checksum changed"));
    }
    receipt.body.binding.validate()?;
    receipt.validate_controls()?;
    Ok(Some(receipt))
}

/// Read-only, including the missing-root case. No payload volume is accessed.
pub(super) fn list(root: &Path) -> Result<Vec<Receipt>, PlatformError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(e)),
    };
    let parent = windows_fs::open_directory(root, FILE_GENERIC_READ).map_err(io)?;
    let entries = entries.collect::<Result<Vec<_>, _>>().map_err(io)?;
    let journal_names = entries
        .iter()
        .filter_map(|entry| parse_windows_journal_name(&entry.file_name()))
        .collect::<Vec<_>>();
    let mut receipts = Vec::new();
    let mut owners = std::collections::BTreeSet::new();
    for entry in entries {
        let Some(text) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(encoded) = text.strip_suffix(".move-done") else {
            continue;
        };
        let Some((id, _)) = parse_windows_journal_name(OsStr::new(&format!("{encoded}.journal")))
        else {
            return Err(io("invalid completion filename"));
        };
        if name(id) != entry.file_name() {
            return Err(io("noncanonical completion filename"));
        }
        let receipt =
            read(&parent, id)?.ok_or_else(|| io("receipt disappeared during discovery"))?;
        for owner in std::iter::once(id).chain(receipt.body.binding.controller()?) {
            if !owners.insert(owner) {
                return Err(io("completion owners overlap"));
            }
        }
        for (id, kind) in &journal_names {
            receipt.covers(*id, *kind)?;
        }
        receipts.push(receipt);
    }
    Ok(receipts)
}

pub(super) fn find(root: &Path, id: [u8; 16]) -> Result<Option<Receipt>, PlatformError> {
    for receipt in list(root)? {
        if receipt.owns(id)? {
            return Ok(Some(receipt));
        }
    }
    Ok(None)
}

fn publish(
    root: &Path,
    binding: Binding,
    prepare: impl FnOnce() -> Result<Prepared, PlatformError>,
) -> Result<(), PlatformError> {
    binding.validate()?;
    let id = binding.id()?;
    let parent = windows_fs::open_directory_with_sharing(
        root,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        FILE_SHARE_READ | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
    )
    .map_err(io)?;
    if let Some(existing) = read(&parent, id)? {
        if existing.body.binding != binding {
            return Err(io("completion belongs to another operation"));
        }
        return parent.sync_all().map_err(io);
    }
    let prepared = prepare()?;
    if prepared.binding != binding {
        return Err(io("prepared completion belongs to another operation"));
    }
    publish_prepared_at(root, &parent, prepared)
}

pub(super) fn publish_prepared(root: &Path, prepared: Prepared) -> Result<(), PlatformError> {
    publish(root, prepared.binding.clone(), || Ok(prepared))
}

fn publish_prepared_at(
    root: &Path,
    parent: &File,
    prepared: Prepared,
) -> Result<(), PlatformError> {
    if move_recovery::stable_identity_of_entry(root).map_err(io)?
        != move_recovery::stable_identity_from_handle(parent).map_err(io)?
    {
        return Err(io("completion registry namespace changed"));
    }
    let id = prepared.binding.id()?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io)?;
    let temporary = format!(".preparing-{}.move-done", encode_transaction_id(random));
    let mut file =
        windows_fs::create_temporary_relative(parent, OsStr::new(&temporary)).map_err(io)?;
    let body = Body {
        version: 2,
        root: move_recovery::stable_identity_from_handle(parent).map_err(io)?,
        file: move_recovery::stable_identity_from_handle(&file).map_err(io)?,
        binding: prepared.binding,
        controls: prepared.controls,
    };
    let receipt = Receipt {
        checksum: checksum(&body)?,
        body,
    };
    receipt.validate_controls()?;
    file.write_all(&serde_json::to_vec(&receipt).map_err(io)?)
        .map_err(io)?;
    file.sync_all().map_err(io)?;
    #[cfg(test)]
    windows_move_boundary(WindowsMoveBoundary::CompletionPrepared, root);
    rename_windows_handle_no_replace(&file, parent, &name(id))?;
    // Until this cancellation succeeds, even a process exit removes the file.
    // Full paired journals remain the fallback throughout that publication gap.
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_ON_CLOSE,
    };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileDispositionInfoEx,
            ptr::from_ref(&disposition).cast_mut().cast(),
            mem::size_of_val(&disposition) as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    #[cfg(test)]
    windows_move_boundary(WindowsMoveBoundary::CompletionPublished, root);
    file.sync_all()
        .and_then(|()| parent.sync_all())
        .map_err(io)?;
    #[cfg(test)]
    windows_move_boundary(WindowsMoveBoundary::CompletionFlushed, root);
    Ok(())
}

/// Caller has just proved paired terminal state under the shared recovery lock.
pub(super) fn prepare_same(
    root: &Path,
    record: &WindowsOverwriteRecord,
) -> Result<Prepared, PlatformError> {
    Ok(Prepared {
        binding: Binding::Same(Box::new(record.clone())),
        controls: vec![controls::Owner::same(root, record)?],
    })
}
pub(super) fn same(root: &Path, record: &WindowsOverwriteRecord) -> Result<(), PlatformError> {
    publish(root, Binding::Same(Box::new(record.clone())), || {
        prepare_same(root, record)
    })
}
fn cross_binding(record: &WindowsCrossVolumeRecord) -> Result<Binding, PlatformError> {
    windows_undo::cross::validate_record(record)?;
    if record.phase != WindowsCrossVolumePhase::SourceRemoved {
        return Err(io("inverse source removal is not complete"));
    }
    Ok(Binding::Cross {
        origin: Box::new(
            record
                .undo_origin
                .clone()
                .ok_or_else(|| io("missing inverse origin"))?,
        ),
        inverse: record.transaction_id()?,
        private: record.transaction_directory_identity,
    })
}
pub(super) fn prepare_cross(
    root: &Path,
    record: &WindowsCrossVolumeRecord,
) -> Result<Prepared, PlatformError> {
    let binding = cross_binding(record)?;
    let mut controls = vec![controls::Owner::cross(root, record)?];
    if let Some(controller) = windows_undo::cross::retained::completed_control(root, record)? {
        controls.push(controls::Owner::cross(root, &controller)?);
    }
    Ok(Prepared { binding, controls })
}
pub(super) fn cross(root: &Path, record: &WindowsCrossVolumeRecord) -> Result<(), PlatformError> {
    publish(root, cross_binding(record)?, || prepare_cross(root, record))
}

#[cfg(test)]
mod tests;
