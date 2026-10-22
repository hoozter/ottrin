//! Native identities and exact default-stream hashes captured before old
//! journals become superseded. Paths are labels, never later deletion authority.
use super::*;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::move_transaction) struct FileEvidence {
    pub identity: Identity,
    pub bytes: u64,
    pub sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::move_transaction) struct Owner {
    pub transaction: [u8; 16],
    pub kind: WindowsJournalKind,
    pub directory: move_recovery::NativePath,
    pub identity: Identity,
    pub parent: move_recovery::NativePath,
    pub parent_identity: Identity,
    pub central: FileEvidence,
    pub local: FileEvidence,
}
struct HashWriter {
    hash: Sha256,
    bytes: u64,
}
impl HashWriter {
    fn new() -> Self {
        Self {
            hash: Sha256::new(),
            bytes: 0,
        }
    }
    fn finish(self) -> (u64, [u8; 32]) {
        (self.bytes, self.hash.finalize().into())
    }
}
impl io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| io::Error::other("journal size overflow"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn expected(record: &impl serde::Serialize) -> Result<(u64, [u8; 32]), PlatformError> {
    let mut writer = HashWriter::new();
    serde_json::to_writer(&mut writer, record).map_err(super::io)?;
    Ok(writer.finish())
}
fn file(
    parent: &File,
    name: &OsStr,
    expected: (u64, [u8; 32]),
) -> Result<FileEvidence, PlatformError> {
    let mut file = unsafe {
        windows_fs::open_relative(
            parent,
            name,
            windows_fs::RelativeOptions {
                directory: Some(false),
                create_new: false,
                access: FILE_GENERIC_READ,
                sharing: FILE_SHARE_READ,
                security: ptr::null(),
            },
        )
    }
    .map_err(super::io)?;
    let identity = move_recovery::stable_identity_from_handle(&file).map_err(super::io)?;
    if !identity.is_regular_file_without_reparse() {
        return Err(super::io("completion journal is not an ordinary file"));
    }
    let mut writer = HashWriter::new();
    io::copy(&mut file, &mut writer).map_err(super::io)?;
    let actual = writer.finish();
    if actual != expected {
        return Err(super::io(
            "completion journal changed before ownership capture",
        ));
    }
    Ok(FileEvidence {
        identity,
        bytes: actual.0,
        sha256: actual.1,
    })
}
impl Owner {
    fn capture(
        root: &Path,
        transaction: [u8; 16],
        kind: WindowsJournalKind,
        directory: &Path,
        identity: Identity,
        parent_identity: Identity,
        expected: (u64, [u8; 32]),
    ) -> Result<Self, PlatformError> {
        let registry = windows_fs::open_directory(root, FILE_GENERIC_READ).map_err(super::io)?;
        let private = windows_control::directory(root, identity, directory)?;
        let central = match kind {
            WindowsJournalKind::SameVolumeOverwrite => windows_journal_path(root, transaction),
            WindowsJournalKind::CrossVolume => windows_cross_volume_journal_path(root, transaction),
        };
        let result = Self {
            transaction,
            kind,
            directory: move_recovery::NativePath::encode(directory),
            identity,
            parent: move_recovery::NativePath::encode(
                directory
                    .parent()
                    .ok_or_else(|| super::io("journal directory has no parent"))?,
            ),
            parent_identity,
            central: file(&registry, central.file_name().unwrap(), expected)?,
            local: file(&private, OsStr::new(WINDOWS_LOCAL_JOURNAL_NAME), expected)?,
        };
        result
            .validate(move_recovery::stable_identity_from_handle(&registry).map_err(super::io)?)?;
        Ok(result)
    }
    pub(super) fn same(
        root: &Path,
        record: &WindowsOverwriteRecord,
    ) -> Result<Self, PlatformError> {
        Self::capture(
            root,
            record.transaction_id()?,
            WindowsJournalKind::SameVolumeOverwrite,
            &record.transaction_path()?,
            record.transaction_directory_identity,
            record.destination_parent_identity,
            expected(record)?,
        )
    }
    pub(super) fn cross(
        root: &Path,
        record: &WindowsCrossVolumeRecord,
    ) -> Result<Self, PlatformError> {
        Self::capture(
            root,
            record.transaction_id()?,
            WindowsJournalKind::CrossVolume,
            &record.transaction_path()?,
            record.transaction_directory_identity,
            record.destination_parent_identity,
            expected(record)?,
        )
    }
    pub(super) fn validate(&self, registry: Identity) -> Result<(), PlatformError> {
        let directory = self.directory.decode().map_err(super::io)?;
        let parent = self.parent.decode().map_err(super::io)?;
        if !directory.is_absolute()
            || directory.parent() != Some(parent.as_path())
            || directory.file_name()
                != Some(OsStr::new(&format!(
                    ".ottrin-move-{}",
                    encode_transaction_id(self.transaction)
                )))
            || !self.identity.is_directory_without_reparse()
            || !self.parent_identity.is_directory_without_reparse()
            || self.identity.volume_serial_number() != self.parent_identity.volume_serial_number()
            || !self.central.identity.is_regular_file_without_reparse()
            || !self.local.identity.is_regular_file_without_reparse()
            || self.central.identity.volume_serial_number() != registry.volume_serial_number()
            || self.local.identity.volume_serial_number() != self.identity.volume_serial_number()
            || self.central.bytes == 0
            || self.central.bytes != self.local.bytes
            || self.central.sha256 != self.local.sha256
        {
            return Err(super::io("invalid superseded control ownership"));
        }
        Ok(())
    }
}
