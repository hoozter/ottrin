//! Handle-bound Windows Copy staging, journaled no-replace publication and DACL application.
//! Sources are never mutated; Copy and recovery share one native transaction engine.
//! Sealed creation recovery exposes only freshly admitted, identity-bound Undo plans.
//!
//! Backup streams preserve directory/file named data and extended attributes that ordinary
//! ReadFile/WriteFile copying loses. Known link records are validated and installed
//! through native handles; security and other unsupported
//! backup records are refused rather than silently omitted.

mod dispatch;
pub(crate) use dispatch::copy_configured;
/// Bound pinned source descriptors before any destination mutation.
pub(crate) const MAX_COPY_ENTRIES: usize = 100_000;
mod journal;
mod registry;
pub use journal::{
    RetainedCopyAbortPlan, RetainedCopyCleanupPlan, RetainedCopyNameReleasePlan,
    RetainedCopyResumePlan, RetainedCopyUndoPlan, WindowsCopyPhase, WindowsCopyRecoveryEvidence,
    WindowsRetainedCopyStatus,
};
pub(crate) use journal::{
    discover as discover_windows_copy_transactions,
    discover_at as discover_windows_copy_transactions_at,
    discover_at_with_progress as discover_windows_copy_transactions_at_with_progress,
    restore_abort as restore_windows_copy_abort, restore_abort_at as restore_windows_copy_abort_at,
    restore_name_release as restore_windows_copy_name_release,
    restore_name_release_at as restore_windows_copy_name_release_at,
    restore_resume as restore_windows_copy_resume,
    restore_resume_at as restore_windows_copy_resume_at, restore_undo as restore_windows_copy_undo,
    restore_undo_at as restore_windows_copy_undo_at,
};
mod links;
mod metadata;
mod namespace;
mod security;
mod tree;

use super::PlatformError;
use crate::move_recovery::{WindowsStableIdentity, stable_identity_from_handle};
use sha2::{Digest, Sha256};
use std::ffi::c_void;
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::ptr;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_NO_INTERMEDIATE_BUFFERING, FILE_SYNCHRONOUS_IO_ALERT, FILE_SYNCHRONOUS_IO_NONALERT,
    FileModeInformation, NtQueryInformationFile,
};
use windows_sys::Win32::Foundation::RtlNtStatusToDosError;
use windows_sys::Win32::Storage::FileSystem::{
    BackupRead, BackupWrite, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_ENCRYPTED,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

const HEADER_BYTES: usize = 20;
const BUFFER_BYTES: usize = 64 * 1024;
const MAX_NAME_BYTES: usize = 65_534;
const DATA: u32 = 1;
const EXTENDED_ATTRIBUTES: u32 = 2;
mod sparse;

const ALTERNATE_DATA: u32 = 4;
const PROPERTY_DATA: u32 = 6;
const REPARSE_DATA: u32 = 8;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StreamEvidence {
    kind: u32,
    attributes: u32,
    name: Vec<u16>,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryVersion {
    identity: WindowsStableIdentity,
    directory: bool,
    link: Option<links::Link>,
    case_sensitive: Option<bool>,
    length: u64,
    created: i64,
    modified: i64,
    changed: i64,
    attributes: u32,
}

fn io(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Windows Copy stream staging: {error}"))
}

/// Hash serialized evidence without allocating its complete JSON representation.
fn evidence_digest(value: &(impl serde::Serialize + ?Sized)) -> Result<[u8; 32], PlatformError> {
    struct Writer(Sha256);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(io)?;
    Ok(writer.0.finalize().into())
}

fn version(file: &File) -> Result<EntryVersion, PlatformError> {
    // The backup API does not validate overlapped handles and documents subtle
    // failures for them. Enforce its buffered synchronous contract ourselves.
    let mut mode = 0u32;
    let mut status = IO_STATUS_BLOCK::default();
    let result = unsafe {
        NtQueryInformationFile(
            file.as_raw_handle().cast(),
            &mut status,
            ptr::from_mut(&mut mode).cast(),
            size_of::<u32>() as u32,
            FileModeInformation,
        )
    };
    if result < 0 {
        return Err(io(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        )));
    }
    if mode & FILE_NO_INTERMEDIATE_BUFFERING != 0
        || mode & (FILE_SYNCHRONOUS_IO_ALERT | FILE_SYNCHRONOUS_IO_NONALERT) == 0
    {
        return Err(PlatformError::UnsupportedSafeCopy(
            "backup staging requires buffered synchronous handles".into(),
        ));
    }
    let identity = stable_identity_from_handle(file).map_err(io)?;
    let basic = metadata::basic(file)?;
    let directory = basic.FileAttributes
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
        != 0;
    let link = if basic.FileAttributes
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        links::capture(file)?
    } else {
        None
    };
    if !(identity.is_regular_file_without_reparse()
        || identity.is_directory_without_reparse()
        || link.is_some())
        || basic.FileAttributes & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_ENCRYPTED) != 0
    {
        return Err(PlatformError::UnsupportedSafeCopy(
            "this staging engine requires an ordinary file/directory or supported link, without EFS data".into(),
        ));
    }
    Ok(EntryVersion {
        identity,
        directory,
        case_sensitive: if directory && link.is_none() {
            Some(crate::windows_fs::directory_case_sensitive(file).map_err(io)?)
        } else {
            None
        },
        link,
        length: if directory {
            0
        } else {
            file.metadata().map_err(io)?.len()
        },
        created: basic.CreationTime,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
        attributes: basic.FileAttributes,
    })
}

#[derive(Clone, Copy)]
enum Direction {
    Read,
    Write,
}

/// The native backup context must be aborted even when a read, write or caller
/// cancellation fails. Explicit finish reports errors; Drop retries on unwind.
struct Backup<'a> {
    file: &'a File,
    direction: Direction,
    context: *mut c_void,
    buffer: Vec<u8>,
    unread: std::ops::Range<usize>,
}

impl<'a> Backup<'a> {
    fn new(file: &'a File, direction: Direction) -> Result<Self, PlatformError> {
        // Directory handles have no byte offset. BackupRead/BackupWrite still
        // preserve their named streams and extended attributes.
        if metadata::basic(file)?.FileAttributes
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
            == 0
        {
            let mut position = file;
            position.seek(SeekFrom::Start(0)).map_err(io)?;
        }
        Ok(Self {
            file,
            direction,
            context: ptr::null_mut(),
            buffer: match direction {
                Direction::Read => vec![0; BUFFER_BYTES],
                Direction::Write => Vec::with_capacity(BUFFER_BYTES),
            },
            unread: 0..0,
        })
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, PlatformError> {
        if !self.unread.is_empty() {
            let count = bytes.len().min(self.unread.len());
            bytes[..count]
                .copy_from_slice(&self.buffer[self.unread.start..self.unread.start + count]);
            self.unread.start += count;
            return Ok(count);
        }
        let mut count = 0;
        if unsafe {
            BackupRead(
                self.file.as_raw_handle().cast(),
                self.buffer.as_mut_ptr(),
                self.buffer.len() as u32,
                &mut count,
                0,
                0,
                &mut self.context,
            )
        } == 0
        {
            return Err(io(std::io::Error::last_os_error()));
        }
        let count = count as usize;
        if count > self.buffer.len() {
            return Err(io("invalid backup read length"));
        }
        if count == 0 {
            return Ok(0);
        }
        self.unread = 0..count;
        self.read(bytes)
    }

    fn read_exact(&mut self, mut bytes: &mut [u8], allow_eof: bool) -> Result<bool, PlatformError> {
        let mut first = true;
        while !bytes.is_empty() {
            let count = self.read(bytes)?;
            if count == 0 {
                if first && allow_eof {
                    return Ok(false);
                }
                return Err(io("truncated backup record"));
            }
            first = false;
            bytes = &mut bytes[count..];
        }
        Ok(true)
    }

    fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), PlatformError> {
        // Native calls use a full-size backing buffer, independently of the
        // parser's 20-byte headers and variable-length names. Small records and
        // the final tail are coalesced rather than issuing one call per field.
        while !bytes.is_empty() {
            let count = bytes.len().min(BUFFER_BYTES - self.buffer.len());
            self.buffer.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if self.buffer.len() == BUFFER_BYTES {
                self.flush()?;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), PlatformError> {
        let mut bytes = &self.buffer[..];
        while !bytes.is_empty() {
            let mut count = 0;
            if unsafe {
                BackupWrite(
                    self.file.as_raw_handle().cast(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut count,
                    0,
                    0,
                    &mut self.context,
                )
            } == 0
            {
                return Err(io(std::io::Error::last_os_error()));
            }
            let count = count as usize;
            if count == 0 || count > bytes.len() {
                return Err(io("invalid backup write length"));
            }
            bytes = &bytes[count..];
        }
        self.buffer.clear();
        Ok(())
    }

    fn finish(&mut self) -> Result<(), PlatformError> {
        if matches!(self.direction, Direction::Write) {
            self.flush()?;
        }
        self.abort()
    }

    fn abort(&mut self) -> Result<(), PlatformError> {
        if self.context.is_null() {
            return Ok(());
        }
        let mut count = 0;
        let success = unsafe {
            match self.direction {
                Direction::Read => BackupRead(
                    self.file.as_raw_handle().cast(),
                    ptr::null_mut(),
                    0,
                    &mut count,
                    1,
                    0,
                    &mut self.context,
                ),
                Direction::Write => BackupWrite(
                    self.file.as_raw_handle().cast(),
                    ptr::null(),
                    0,
                    &mut count,
                    1,
                    0,
                    &mut self.context,
                ),
            }
        };
        if success == 0 {
            return Err(io(std::io::Error::last_os_error()));
        }
        self.context = ptr::null_mut();
        Ok(())
    }
}

impl Drop for Backup<'_> {
    fn drop(&mut self) {
        // Cancellation and failed writes discard pending bytes; only normal
        // finish flushes them into the caller's unpublished candidate.
        let _ = self.abort();
    }
}

fn validate_header(header: &[u8; HEADER_BYTES]) -> Result<(u32, u32, u64, usize), PlatformError> {
    let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
    let attributes = u32::from_le_bytes(header[4..8].try_into().unwrap());
    let bytes = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let name = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    if !matches!(
        kind,
        DATA | EXTENDED_ATTRIBUTES | ALTERNATE_DATA | PROPERTY_DATA | REPARSE_DATA | sparse::BLOCK
    ) || bytes > i64::MAX as u64
        || name > MAX_NAME_BYTES
        || !name.is_multiple_of(2)
        || (kind == ALTERNATE_DATA) != (name != 0)
        || (kind == sparse::BLOCK && (attributes != sparse::ATTRIBUTE || bytes < 8))
        || (attributes & sparse::ATTRIBUTE != 0
            && (attributes != sparse::ATTRIBUTE
                || !matches!(kind, DATA | ALTERNATE_DATA | sparse::BLOCK)))
    {
        return Err(PlatformError::UnsupportedSafeCopy(
            "unsupported or malformed native backup record; candidate remains unpublished".into(),
        ));
    }
    Ok((kind, attributes, bytes, name))
}

fn transfer(
    source: &File,
    target: Option<&File>,
    progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Vec<StreamEvidence>, PlatformError> {
    let source_version = version(source)?;
    let directory = source_version.directory;
    let mut saw_reparse = false;
    let mut reader = Backup::new(source, Direction::Read)?;
    let mut writer = target
        .map(|file| Backup::new(file, Direction::Write))
        .transpose()?;
    let mut evidence = Vec::new();
    let mut sparse_stream: Option<sparse::Stream> = None;
    let mut buffer = [0u8; BUFFER_BYTES];
    let mut total = 0u64;
    progress(0)?;
    loop {
        let mut header = [0u8; HEADER_BYTES];
        if !reader.read_exact(&mut header, true)? {
            break;
        }
        let (kind, attributes, bytes, name_bytes) = validate_header(&header)?;
        if directory && kind == DATA {
            return Err(io(
                "directory backup unexpectedly contains unnamed file data",
            ));
        }
        let mut name = vec![0u8; name_bytes];
        reader.read_exact(&mut name, false)?;
        let name_units = name
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        if kind == ALTERNATE_DATA
            && (name_units.len() < 8
                || name_units.first() != Some(&58)
                || !name_units.ends_with(&[58, 36, 68, 65, 84, 65])
                || name_units[1..name_units.len() - 6]
                    .iter()
                    .any(|unit| [0, 47, 58, 92].contains(unit)))
        {
            return Err(io("invalid alternate stream name"));
        }
        if matches!(kind, DATA | ALTERNATE_DATA)
            && let Some(previous) = sparse_stream.take()
        {
            evidence.push(previous.finish()?);
        }
        if kind == sparse::BLOCK {
            let stream = sparse_stream
                .as_mut()
                .ok_or_else(|| io("sparse block has no sparse data stream"))?;
            let mut offset = [0; 8];
            reader.read_exact(&mut offset, false)?;
            stream.block(u64::from_le_bytes(offset), bytes - 8)?;
            if let Some(writer) = &mut writer {
                writer.write_all(&header)?;
                writer.write_all(&offset)?;
            }
            let mut left = bytes - 8;
            while left != 0 {
                let count = left.min(BUFFER_BYTES as u64) as usize;
                reader.read_exact(&mut buffer[..count], false)?;
                stream.data(&buffer[..count])?;
                if let Some(writer) = &mut writer {
                    writer.write_all(&buffer[..count])?;
                }
                left -= count as u64;
                total = total
                    .checked_add(count as u64)
                    .ok_or_else(|| io("stream length overflow"))?;
                progress(total)?;
            }
            progress(total)?;
            continue;
        }
        if attributes == sparse::ATTRIBUTE {
            if bytes != 0 {
                return Err(io(
                    "sparse data header contains an unrecognized inline payload",
                ));
            }
            sparse_stream = Some(sparse::Stream::new(kind, name_units));
            if let Some(writer) = &mut writer {
                writer.write_all(&header)?;
                writer.write_all(&name)?;
            }
            continue;
        }
        if kind == REPARSE_DATA {
            let expected = source_version
                .link
                .as_ref()
                .ok_or_else(|| io("unexpected reparse stream on an ordinary entry"))?;
            if saw_reparse || bytes != expected.bytes.len() as u64 {
                return Err(io("duplicate or inconsistent native link record"));
            }
            let mut record = vec![0u8; expected.bytes.len()];
            reader.read_exact(&mut record, false)?;
            if record != expected.bytes {
                return Err(io("backup link record differs from held source"));
            }
            saw_reparse = true;
            total = total
                .checked_add(bytes)
                .ok_or_else(|| io("stream length overflow"))?;
            progress(total)?;
            evidence.push(StreamEvidence {
                kind,
                attributes,
                name: name_units,
                bytes,
                sha256: format!("{:x}", Sha256::digest(&record)),
            });
            // Never pass opaque reparse records to BackupWrite. After ordinary
            // payload copying, install this validated record on the held inode.
            continue;
        }
        if let Some(writer) = &mut writer {
            writer.write_all(&header)?;
            writer.write_all(&name)?;
        }
        let mut left = bytes;
        let mut hash = Sha256::new();
        while left > 0 {
            let count = left.min(BUFFER_BYTES as u64) as usize;
            reader.read_exact(&mut buffer[..count], false)?;
            if let Some(writer) = &mut writer {
                writer.write_all(&buffer[..count])?;
            }
            hash.update(&buffer[..count]);
            left -= count as u64;
            total = total
                .checked_add(count as u64)
                .ok_or_else(|| io("stream length overflow"))?;
            progress(total)?;
        }
        evidence.push(StreamEvidence {
            kind,
            attributes,
            name: name_units,
            bytes,
            sha256: format!("{:x}", hash.finalize()),
        });
    }
    if let Some(stream) = sparse_stream {
        evidence.push(stream.finish()?);
    }
    reader.finish()?;
    if let Some(writer) = &mut writer {
        writer.finish()?;
    }
    if source_version.link.is_some() != saw_reparse {
        return Err(io("native backup omitted the held link reparse record"));
    }
    if let (Some(link), Some(candidate)) = (&source_version.link, target) {
        link.install(candidate)?;
    }
    // BackupRead omits an empty unnamed stream. The transaction manifest still
    // needs its logical existence, independent of native record enumeration.
    if !directory && !evidence.iter().any(|stream| stream.kind == DATA) {
        if source.metadata().map_err(io)?.len() != 0 {
            return Err(io("native backup omitted nonempty primary data"));
        }
        evidence.push(StreamEvidence {
            kind: DATA,
            attributes: 0,
            name: Vec::new(),
            bytes: 0,
            sha256: format!("{:x}", Sha256::digest([])),
        });
    }
    evidence.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    if evidence
        .windows(2)
        .any(|pair| (pair[0].kind, &pair[0].name) == (pair[1].kind, &pair[1].name))
    {
        return Err(io("duplicate native backup streams"));
    }
    Ok(evidence)
}

/// Copy one entry's streams, not directory children, into an independently
/// created, payload-empty private candidate held open
/// by the transaction owner. No pathname is reopened or removed here. Sharing
/// policy and ownership of that candidate are the caller's native stage boundary.
pub(super) fn stage_entry_streams(
    source: &File,
    candidate: &File,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Vec<StreamEvidence>, PlatformError> {
    let before = version(source)?;
    let target = version(candidate)?;
    if before.identity.same_object(target.identity)
        || before.directory != target.directory
        || target.length != 0
        || target.link.is_some()
    {
        return Err(io(
            "candidate aliases the source, has a different type, or is not empty",
        ));
    }
    if transfer(candidate, None, &mut |_| progress(0))?
        .iter()
        .any(|stream| stream.kind != DATA || stream.bytes != 0)
    {
        return Err(io("candidate already contains named data or metadata"));
    }
    let mut copied_bytes = 0;
    let copied = transfer(source, Some(candidate), &mut |bytes| {
        progress(bytes)?;
        copied_bytes = bytes;
        Ok(())
    })?;
    candidate.sync_all().map_err(io)?;
    if !target
        .identity
        .same_object(stable_identity_from_handle(candidate).map_err(io)?)
        || version(source)? != before
        || transfer(source, None, &mut |_| progress(copied_bytes))? != copied
        || version(source)? != before
        || transfer(candidate, None, &mut |_| progress(copied_bytes))? != copied
    {
        return Err(io("source changed or staged streams do not match"));
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_fs;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fn source(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn candidate(path: &Path) -> File {
        OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }

    #[test]
    fn windows_native_copy_sparse_backup_round_trip_preserves_trailing_holes() {
        use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE};
        fn sparse(file: &File, length: u64, offset: u64) {
            let mut returned = 0;
            assert_ne!(
                unsafe {
                    DeviceIoControl(
                        file.as_raw_handle().cast(),
                        FSCTL_SET_SPARSE,
                        ptr::null(),
                        0,
                        ptr::null_mut(),
                        0,
                        &mut returned,
                        ptr::null_mut(),
                    )
                },
                0,
                "{}",
                std::io::Error::last_os_error()
            );
            file.set_len(length).unwrap();
            let mut file = file;
            file.seek(SeekFrom::Start(offset)).unwrap();
            file.write_all(b"allocated sparse data").unwrap();
            file.sync_all().unwrap();
        }
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source.bin");
        let dst = root.path().join("candidate.bin");
        sparse(&candidate(&src), 16 * 1024 * 1024, 65536);
        let source_named = format!("{}:thin", src.display());
        let target_named = format!("{}:thin", dst.display());
        sparse(
            &candidate(Path::new(&source_named)),
            4 * 1024 * 1024,
            131072,
        );
        let read = source(&src);
        let target = candidate(&dst);
        let mut reader = Backup::new(&read, Direction::Read).unwrap();
        let mut writer = Backup::new(&target, Direction::Write).unwrap();
        let mut buffer = [0u8; BUFFER_BYTES];
        loop {
            let mut header = [0u8; HEADER_BYTES];
            if !reader.read_exact(&mut header, true).unwrap() {
                break;
            }
            let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
            let attributes = u32::from_le_bytes(header[4..8].try_into().unwrap());
            let bytes = u64::from_le_bytes(header[8..16].try_into().unwrap());
            let name_bytes = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
            assert!(name_bytes <= MAX_NAME_BYTES);
            let mut name = vec![0; name_bytes];
            reader.read_exact(&mut name, false).unwrap();
            println!(
                "sparse backup record: kind={kind} attributes={attributes} bytes={bytes} name={:?}",
                String::from_utf16_lossy(
                    &name
                        .chunks_exact(2)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>()
                )
            );
            writer.write_all(&header).unwrap();
            writer.write_all(&name).unwrap();
            let mut left = bytes;
            while left != 0 {
                let count = left.min(BUFFER_BYTES as u64) as usize;
                reader.read_exact(&mut buffer[..count], false).unwrap();
                if kind == 9 && left == bytes && count >= 8 {
                    println!(
                        "sparse block offset={}",
                        u64::from_le_bytes(buffer[..8].try_into().unwrap())
                    );
                }
                writer.write_all(&buffer[..count]).unwrap();
                left -= count as u64;
            }
        }
        reader.finish().unwrap();
        writer.finish().unwrap();
        target.sync_all().unwrap();
        println!(
            "source length={} target length={} source attributes={} target attributes={}",
            read.metadata().unwrap().len(),
            target.metadata().unwrap().len(),
            metadata::basic(&read).unwrap().FileAttributes,
            metadata::basic(&target).unwrap().FileAttributes
        );
        assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(&src).unwrap());
        assert_eq!(
            std::fs::read(target_named).unwrap(),
            std::fs::read(source_named).unwrap()
        );
    }

    #[test]
    fn windows_native_copy_staging_preserves_large_unicode_and_empty_streams() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source.bin");
        let dst = root.path().join("candidate.bin");
        let data = (0..200_003).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        std::fs::write(&src, &data).unwrap();
        let named = ":réservé-🦀:$DATA";
        std::fs::write(format!("{}{named}", src.display()), b"named payload").unwrap();
        std::fs::write(format!("{}:empty:$DATA", src.display()), b"").unwrap();
        let read = source(&src);
        let staged = candidate(&dst);
        let before = version(&read).unwrap();
        let streams = stage_entry_streams(&read, &staged, |_| Ok(())).unwrap();
        assert_eq!(streams.len(), 3);
        assert_eq!(
            streams.iter().filter(|s| s.kind == ALTERNATE_DATA).count(),
            2
        );
        assert_ne!(version(&staged).unwrap().identity, before.identity);
        assert_eq!(std::fs::read(&dst).unwrap(), data);
        assert_eq!(
            std::fs::read(format!("{}{named}", dst.display())).unwrap(),
            b"named payload"
        );
        assert!(
            std::fs::metadata(format!("{}:empty:$DATA", dst.display()))
                .unwrap()
                .len()
                == 0
        );
        assert_eq!(version(&read).unwrap(), before);
        drop(read);
        std::fs::write(&src, "later source version").unwrap();
        assert_eq!(
            std::fs::read(&dst).unwrap(),
            data,
            "copy must own independent bytes"
        );
    }

    #[test]
    fn windows_native_copy_staging_keeps_empty_files_and_named_streams() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("empty");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"").unwrap();
        std::fs::write(format!("{}:note:$DATA", src.display()), b"only in ADS").unwrap();
        let streams = stage_entry_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
        assert!(streams.iter().any(|s| s.kind == DATA && s.bytes == 0));
        assert_eq!(
            std::fs::read(format!("{}:note:$DATA", dst.display())).unwrap(),
            b"only in ADS"
        );
        assert_eq!(std::fs::read(&dst).unwrap(), b"");
    }

    #[test]
    fn windows_native_copy_staging_handles_small_and_buffer_boundary_tails() {
        let root = tempfile::tempdir().unwrap();
        for length in [
            1,
            BUFFER_BYTES - HEADER_BYTES,
            BUFFER_BYTES - HEADER_BYTES + 1,
            BUFFER_BYTES,
            BUFFER_BYTES + 1,
        ] {
            let src = root.path().join(format!("source-{length}"));
            let dst = root.path().join(format!("candidate-{length}"));
            let bytes = vec![93u8; length];
            std::fs::write(&src, &bytes).unwrap();
            stage_entry_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
            assert_eq!(std::fs::read(&dst).unwrap(), bytes);
        }
    }

    pub(super) fn set_fixture_ea(file: &File) {
        use windows_sys::Wdk::Storage::FileSystem::NtSetEaFile;
        #[repr(align(4))]
        struct Ea([u8; 16]);
        let mut storage = Ea([0; 16]);
        let ea = &mut storage.0;
        ea[5] = 4;
        ea[6..8].copy_from_slice(&3u16.to_le_bytes());
        ea[8..13].copy_from_slice(b"user\0");
        ea[13..].copy_from_slice(b"abc");
        let mut status = IO_STATUS_BLOCK::default();
        let result = unsafe {
            NtSetEaFile(
                file.as_raw_handle().cast(),
                &mut status,
                ea.as_ptr().cast(),
                ea.len() as u32,
            )
        };
        assert!(
            result >= 0,
            "EA fixture NtSetEaFile failed: {result:#x} ({})",
            std::io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(result) } as i32)
        );
    }

    #[test]
    fn windows_native_copy_staging_preserves_extended_attributes() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"primary").unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&src)
            .unwrap();
        set_fixture_ea(&file);
        drop(file);
        let streams = stage_entry_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
        assert!(
            streams
                .iter()
                .any(|stream| stream.kind == EXTENDED_ATTRIBUTES && stream.bytes > 0)
        );
        assert_eq!(std::fs::read(&src).unwrap(), b"primary");
        assert_eq!(std::fs::read(&dst).unwrap(), b"primary");
    }

    // These permissions apply only to owned fixtures. Restore even on panic so
    // failed native tests do not leave an undeletable temporary directory.
    pub(super) struct ReadOnlyAcl(pub(super) Vec<PathBuf>);
    pub(super) fn set_dacl(path: &Path, text: &str) {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
            SetNamedSecurityInfoW,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };
        let text = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut sd = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    ptr::null_mut(),
                )
            },
            0
        );
        struct Owned(*mut c_void);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
        let _owned = Owned(sd);
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        assert_ne!(
            unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) },
            0
        );
        assert_ne!(present, 0);
        let mut path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        assert_eq!(
            unsafe {
                SetNamedSecurityInfoW(
                    path.as_mut_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    acl,
                    ptr::null(),
                )
            },
            0
        );
    }
    impl Drop for ReadOnlyAcl {
        fn drop(&mut self) {
            for path in &self.0 {
                set_dacl(path, "D:P(A;OICI;FA;;;OW)");
            }
        }
    }

    #[test]
    fn windows_native_copy_staging_reads_without_source_write_or_delete_access() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("read-only-parent");
        std::fs::create_dir(&parent).unwrap();
        let src = parent.join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"readable source").unwrap();
        let guard = ReadOnlyAcl(vec![parent.clone(), src.clone()]);
        // FR: read, WD: restore this owned fixture's DACL. No data-write,
        // delete or parent delete-child rights are granted.
        for path in &guard.0 {
            set_dacl(path, "D:P(A;OICI;FRWD;;;OW)");
        }
        assert!(OpenOptions::new().access_mode(DELETE).open(&src).is_err());
        assert!(OpenOptions::new().write(true).open(&src).is_err());
        let parent_handle = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&parent)
            .unwrap();
        let read =
            namespace::open_source(&parent_handle, std::ffi::OsStr::new("source"), Some(false))
                .unwrap();
        metadata::stage_entry_payload(&read, &candidate(&dst), |_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"readable source");
        assert_eq!(std::fs::read(&dst).unwrap(), b"readable source");
        drop(guard);
    }

    #[test]
    fn windows_native_copy_directory_payload_uses_read_only_source_and_private_candidate() {
        let root = tempfile::tempdir().unwrap();
        let source_parent = root.path().join("read-only-parent");
        let src = source_parent.join("source");
        std::fs::create_dir(&source_parent).unwrap();
        std::fs::create_dir(&src).unwrap();
        std::fs::write(
            format!("{}:note:$DATA", src.display()),
            b"private directory payload",
        )
        .unwrap();
        let guard = ReadOnlyAcl(vec![source_parent.clone(), src.clone()]);
        for path in &guard.0 {
            set_dacl(path, "D:P(A;OICI;FRWD;;;OW)");
        }
        for access in [
            DELETE,
            windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_WRITE,
        ] {
            assert!(
                OpenOptions::new()
                    .access_mode(access)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                    .open(&src)
                    .is_err()
            );
        }
        let parent_read = source(&source_parent);
        let read = namespace::open_source(&parent_read, std::ffi::OsStr::new("source"), Some(true))
            .unwrap();
        assert!(
            namespace::open_source(&parent_read, std::ffi::OsStr::new("source"), Some(false))
                .is_err()
        );
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        metadata::stage_entry_payload(&read, &candidate, |_| Ok(())).unwrap();
        stage.validate_candidate(&candidate, true).unwrap();
        assert_owner_only(&candidate);
        assert_eq!(
            std::fs::read(format!(
                "{}:note:$DATA",
                windows_fs::location(&candidate).unwrap().display()
            ))
            .unwrap(),
            b"private directory payload"
        );
        assert_eq!(
            std::fs::read(format!("{}:note:$DATA", src.display())).unwrap(),
            b"private directory payload"
        );
        drop((read, parent_read));
        drop(guard);
    }

    pub(super) fn parent(path: &Path) -> File {
        use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE};
        OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }

    pub(super) fn assert_owner_only(file: &File) {
        use windows_sys::Win32::Security::{
            ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, GetAce, GetKernelObjectSecurity,
            GetLengthSid, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
            SE_DACL_PROTECTED,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
        use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
        let mut length = 0;
        unsafe {
            GetKernelObjectSecurity(
                file.as_raw_handle().cast(),
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                0,
                &mut length,
            );
        }
        assert!(length > 0 && length <= 65536);
        let mut storage = vec![0u64; (length as usize).div_ceil(8)];
        let descriptor = storage.as_mut_ptr().cast();
        assert_ne!(
            unsafe {
                GetKernelObjectSecurity(
                    file.as_raw_handle().cast(),
                    DACL_SECURITY_INFORMATION,
                    descriptor,
                    length,
                    &mut length,
                )
            },
            0
        );
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "private DACL must not inherit public parent entries"
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
            },
            0
        );
        assert_ne!(present, 0);
        assert!(!acl.is_null());
        assert_eq!(unsafe { (*acl).AceCount }, 1);
        let mut raw = ptr::null_mut();
        assert_ne!(unsafe { GetAce(acl, 0, &mut raw) }, 0);
        let ace = unsafe { &*raw.cast::<ACCESS_ALLOWED_ACE>() };
        assert_eq!(u32::from(ace.Header.AceType), ACCESS_ALLOWED_ACE_TYPE);
        assert_eq!(ace.Mask, FILE_ALL_ACCESS);
        let sid: *mut c_void = ptr::from_ref(&ace.SidStart).cast_mut().cast();
        let bytes =
            unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize) };
        assert_eq!(
            bytes,
            &[1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0],
            "only OWNER RIGHTS may be granted access"
        );
    }

    #[test]
    fn windows_native_copy_namespace_is_private_at_creation_in_public_parent() {
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        set_dacl(&destination, "D:P(A;OICI;FA;;;WD)");
        let parent = parent(&destination);
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        assert_owner_only(&stage.directory);
        let staged = stage.create_candidate(false).unwrap();
        assert_owner_only(&staged);
        let src = root.path().join("source");
        std::fs::write(&src, b"private candidate data").unwrap();
        stage_entry_streams(&source(&src), &staged, |_| Ok(())).unwrap();
        stage.validate().unwrap();
        let collision = stage.create_candidate(false).unwrap_err();
        assert!(matches!(collision, PlatformError::CleanupPending { .. }));
        let location = crate::windows_fs::location(&staged).unwrap();
        assert_eq!(std::fs::read(&location).unwrap(), b"private candidate data");
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 2);
        assert!(stage.name.to_string_lossy().starts_with(".ottrin-copy-"));
        assert!(namespace::open_source(&parent, OsStr::new("missing"), Some(false)).is_err());
    }

    #[test]
    fn windows_native_copy_namespace_rejects_non_leaf_names_without_mutation() {
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("destination");
        std::fs::create_dir(&destination).unwrap();
        let parent = parent(&destination);
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "sub\\file",
            "file:stream",
            "nul\0name",
            &"x".repeat(256),
        ] {
            let error = crate::windows_fs::create_private(
                &parent,
                OsStr::new(name),
                false,
                FILE_SHARE_READ,
            )
            .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
            assert!(namespace::open_source(&parent, OsStr::new(name), Some(false)).is_err());
        }
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        assert!(!root.path().join("escape").exists());
    }

    #[test]
    fn windows_native_copy_namespace_create_never_replaces_existing_entries() {
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        std::fs::create_dir(root.path().join("occupied")).unwrap();
        std::fs::write(root.path().join("occupied/keep"), b"user data").unwrap();
        let error = crate::windows_fs::create_private(
            &parent,
            OsStr::new("occupied"),
            true,
            FILE_SHARE_READ,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(root.path().join("occupied/keep")).unwrap(),
            b"user data"
        );
    }

    #[test]
    fn windows_native_copy_candidate_collision_never_adopts_an_existing_entry() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let occupied = root.path().join(&stage.candidate_name);
        std::fs::write(&occupied, b"unrelated user data").unwrap();
        assert!(matches!(
            stage.create_candidate(false),
            Err(PlatformError::CleanupPending { .. })
        ));
        assert_eq!(std::fs::read(occupied).unwrap(), b"unrelated user data");
        assert_eq!(
            std::fs::read_dir(windows_fs::location(&stage.directory).unwrap())
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn windows_native_copy_candidate_rebinding_retains_both_objects() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(false).unwrap();
        (&*candidate).write_all(b"verified staged data").unwrap();
        crate::move_transaction::rename_windows_handle_no_replace(
            &candidate,
            &parent,
            std::ffi::OsStr::new("retained-candidate"),
        )
        .unwrap();
        let replacement = root.path().join(&stage.candidate_name);
        std::fs::write(&replacement, b"unrelated replacement").unwrap();
        assert!(matches!(
            stage.validate_candidate(&candidate, false),
            Err(PlatformError::CleanupPending { .. })
        ));
        assert_eq!(
            std::fs::read(replacement).unwrap(),
            b"unrelated replacement"
        );
        assert_eq!(
            std::fs::read(root.path().join("retained-candidate")).unwrap(),
            b"verified staged data"
        );
    }

    #[test]
    fn windows_native_copy_directory_candidate_is_private_and_type_checked() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        assert_owner_only(&candidate);
        let candidate_location = windows_fs::location(&candidate).unwrap();
        let parent_location = windows_fs::location(&parent).unwrap();
        assert_eq!(candidate_location.parent(), Some(parent_location.as_path()));
        stage.validate_candidate(&candidate, true).unwrap();
        assert!(stage.validate_candidate(&candidate, false).is_err());
        let child = windows_fs::create_private(
            &candidate,
            std::ffi::OsStr::new("child"),
            false,
            FILE_SHARE_READ,
        )
        .unwrap();
        assert_owner_only(&child);
        (&child).write_all(b"nested private data").unwrap();
    }

    #[test]
    fn windows_native_copy_namespace_excludes_other_directory_writers() {
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let open_writer = |path: &Path| {
            OpenOptions::new()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
        };
        assert!(
            open_writer(root.path()).is_ok(),
            "control directory is writable"
        );
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let path = crate::windows_fs::location(&stage.directory).unwrap();
        let error = open_writer(&path)
            .expect_err("an unrelated directory writer could change reparse metadata");
        assert_eq!(
            error.raw_os_error(),
            Some(32),
            "must refuse by sharing, not merely a fixture ACL"
        );
        stage.create_candidate(false).unwrap();
    }

    #[test]
    fn windows_native_copy_namespace_keeps_raw_utf16_names() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let name = OsString::from_wide(&[102, 111, 0xd800, 111]);
        let created =
            crate::windows_fs::create_private(&parent, &name, false, FILE_SHARE_READ).unwrap();
        (&created).write_all(b"native name").unwrap();
        drop(created);
        let opened = namespace::open_source(&parent, &name, Some(false)).unwrap();
        let candidate = candidate(&root.path().join("candidate"));
        stage_entry_streams(&opened, &candidate, |_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("candidate")).unwrap(),
            b"native name"
        );
    }

    #[test]
    fn windows_native_copy_namespace_rejects_reparse_sources_and_parents() {
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"unchanged").unwrap();
        std::os::windows::fs::symlink_file(outside.join("keep"), root.path().join("link")).unwrap();
        std::os::windows::fs::symlink_dir(&outside, root.path().join("directory-link")).unwrap();
        let parent_handle = parent(root.path());
        assert!(namespace::open_source(&parent_handle, OsStr::new("link"), Some(false)).is_err());
        assert!(
            namespace::open_source(&parent_handle, OsStr::new("directory-link"), Some(true))
                .is_err()
        );
        let linked_parent = parent(&root.path().join("directory-link"));
        assert!(namespace::PrivateStage::create(&linked_parent).is_err());
        assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"unchanged");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
    }

    #[test]
    fn windows_native_copy_namespace_alias_retarget_cannot_redirect_creation() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        for base in [&first, &second] {
            std::fs::create_dir_all(base.join("destination")).unwrap();
        }
        let alias = root.path().join("alias");
        std::os::windows::fs::symlink_dir(&first, &alias).unwrap();
        let parent = parent(&alias.join("destination"));
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        std::fs::remove_dir(&alias).unwrap();
        std::os::windows::fs::symlink_dir(&second, &alias).unwrap();
        let created = stage.create_candidate(false).unwrap();
        (&*created).write_all(b"original parent").unwrap();
        assert_eq!(
            std::fs::read(first.join("destination").join(&stage.candidate_name)).unwrap(),
            b"original parent"
        );
        assert_eq!(
            std::fs::read_dir(second.join("destination"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn windows_native_copy_namespace_binding_change_preserves_both_directories() {
        use std::ffi::OsStr;
        let root = tempfile::tempdir().unwrap();
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        crate::move_transaction::rename_windows_handle_no_replace(
            &stage.directory,
            &parent,
            OsStr::new("retained"),
        )
        .unwrap();
        std::fs::create_dir(root.path().join(&stage.name)).unwrap();
        let unrelated = root.path().join(&stage.name).join("keep");
        std::fs::write(&unrelated, b"unrelated").unwrap();
        assert!(matches!(
            stage.create_candidate(false),
            Err(PlatformError::CleanupPending { .. })
        ));
        assert_eq!(std::fs::read(&unrelated).unwrap(), b"unrelated");
        assert_eq!(
            std::fs::read_dir(root.path().join("retained"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn windows_native_copy_staging_refuses_occupied_or_named_stream_candidates() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"new").unwrap();
        let file = candidate(&dst);
        (&file).write_all(b"existing").unwrap();
        assert!(stage_entry_streams(&source(&src), &file, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"existing");
        drop(file);
        std::fs::write(&dst, b"").unwrap();
        std::fs::write(format!("{}:keep:$DATA", dst.display()), b"keep").unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&dst)
            .unwrap();
        assert!(stage_entry_streams(&source(&src), &file, |_| Ok(())).is_err());
        assert_eq!(
            std::fs::read(format!("{}:keep:$DATA", dst.display())).unwrap(),
            b"keep"
        );
        assert_eq!(std::fs::read(&src).unwrap(), b"new");
    }

    #[test]
    fn windows_native_copy_staging_refuses_a_hard_link_to_the_empty_source() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let alias = root.path().join("alias");
        std::fs::write(&src, b"").unwrap();
        std::fs::hard_link(&src, &alias).unwrap();
        let src_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&src)
            .unwrap();
        let alias_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&alias)
            .unwrap();
        assert!(stage_entry_streams(&src_file, &alias_file, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&src).unwrap(), b"");
    }

    #[test]
    fn windows_native_copy_staging_cancellation_releases_contexts_and_keeps_source() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        let original = vec![17u8; BUFFER_BYTES * 3 + 1];
        std::fs::write(&src, &original).unwrap();
        let staged = candidate(&dst);
        let result = stage_entry_streams(&source(&src), &staged, |copied| {
            if copied >= BUFFER_BYTES as u64 {
                Err(io("injected cancellation"))
            } else {
                Ok(())
            }
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("injected cancellation")
        );
        assert_eq!(std::fs::read(&src).unwrap(), original);
        drop(staged);
        let mut reopened = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&dst)
            .unwrap();
        reopened
            .write_all(b"caller still owns unpublished candidate")
            .unwrap();
    }

    #[test]
    fn windows_native_copy_verification_cancellation_preserves_source_and_retains_candidate() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        let original = vec![37u8; BUFFER_BYTES * 4 + 17];
        std::fs::write(&src, &original).unwrap();
        let source = source(&src);
        let target = candidate(&dst);
        let mut complete_callbacks = 0;
        let mut previous = 0;
        let error = stage_entry_streams(&source, &target, |bytes| {
            assert!(
                bytes >= previous,
                "verification must not reset copy progress"
            );
            previous = bytes;
            if bytes == original.len() as u64 {
                complete_callbacks += 1;
                // First is copy completion, second starts source verification,
                // third follows its first native read chunk.
                if complete_callbacks == 3 {
                    return Err(io("stopped during stream verification"));
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("stopped during stream verification")
        );
        assert_eq!(complete_callbacks, 3);
        assert_eq!(std::fs::read(&src).unwrap(), original);
        assert_eq!(std::fs::read(&dst).unwrap(), original);
        // Aborted backup contexts must leave no native handle behind.
        drop((source, target));
        let exclusive = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&dst)
            .unwrap();
        assert_eq!(exclusive.metadata().unwrap().len(), original.len() as u64);
    }

    #[test]
    fn windows_native_copy_verification_metadata_reads_observe_cancellation_and_retry() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        let original = vec![37u8; BUFFER_BYTES * 4 + 17];
        std::fs::write(&src, &original).unwrap();
        let source = source(&src);
        let target = candidate(&dst);
        let evidence = metadata::stage_entry_payload(&source, &target, |_| Ok(())).unwrap();
        for verify_source in [true, false] {
            let mut calls = 0;
            let mut cancellation = || {
                calls += 1;
                if calls == 3 {
                    Err(io("stopped during metadata payload verification"))
                } else {
                    Ok(())
                }
            };
            let result = if verify_source {
                evidence.validate_source(&source, &mut cancellation)
            } else {
                evidence.validate_candidate_streams(&target, &mut cancellation)
            };
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("stopped during metadata payload verification")
            );
            assert_eq!(calls, 3);
            evidence.validate_source(&source, || Ok(())).unwrap();
            evidence
                .validate_candidate_streams(&target, || Ok(()))
                .unwrap();
        }
        assert_eq!(std::fs::read(&src).unwrap(), original);
        assert_eq!(std::fs::read(&dst).unwrap(), original);
    }

    #[test]
    fn windows_native_copy_staging_write_failure_releases_native_contexts() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"original").unwrap();
        std::fs::write(&dst, b"").unwrap();
        let read_only_candidate = source(&dst);
        let error =
            stage_entry_streams(&source(&src), &read_only_candidate, |_| Ok(())).unwrap_err();
        assert!(error.to_string().contains("staging"));
        assert_eq!(std::fs::read(&src).unwrap(), b"original");
        assert_eq!(std::fs::read(&dst).unwrap(), b"");
        drop(read_only_candidate);
        OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(&dst)
            .unwrap()
            .write_all(b"still owned by caller")
            .unwrap();
    }

    #[test]
    fn windows_native_copy_staging_accepts_a_hard_link_source_as_independent_data() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let alias = root.path().join("alias");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"linked source").unwrap();
        std::fs::hard_link(&src, &alias).unwrap();
        let read = source(&alias);
        let copied = candidate(&dst);
        stage_entry_streams(&read, &copied, |_| Ok(())).unwrap();
        assert_ne!(
            version(&read).unwrap().identity,
            version(&copied).unwrap().identity
        );
        assert_eq!(std::fs::read(&src).unwrap(), b"linked source");
        assert_eq!(std::fs::read(&dst).unwrap(), b"linked source");
    }

    #[test]
    fn windows_native_copy_staging_refuses_overlapped_and_unbuffered_handles() {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
        };
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        std::fs::write(&src, b"unchanged").unwrap();
        for (index, flags) in [FILE_FLAG_OVERLAPPED, FILE_FLAG_NO_BUFFERING]
            .into_iter()
            .enumerate()
        {
            let read = OpenOptions::new()
                .read(true)
                .custom_flags(flags)
                .open(&src)
                .unwrap();
            let dst = root.path().join(format!("candidate-{index}"));
            let staged = candidate(&dst);
            assert!(
                stage_entry_streams(&read, &staged, |_| Ok(()))
                    .unwrap_err()
                    .to_string()
                    .contains("buffered synchronous")
            );
            assert_eq!(std::fs::read(&dst).unwrap(), b"");
        }
        assert_eq!(std::fs::read(&src).unwrap(), b"unchanged");
    }

    #[test]
    fn windows_native_copy_staging_detects_changes_during_copy() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, vec![5u8; BUFFER_BYTES * 3]).unwrap();
        let read = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&src)
            .unwrap();
        let mut changed = false;
        let result = stage_entry_streams(&read, &candidate(&dst), |bytes| {
            if bytes > 0 && !changed {
                let mut writer = OpenOptions::new()
                    .write(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                    .open(&src)
                    .unwrap();
                writer.write_all(b"external modification").unwrap();
                changed = true;
            }
            Ok(())
        });
        assert!(changed);
        assert!(
            result.is_err(),
            "changed source must not be accepted as a complete copy"
        );
        assert!(
            std::fs::read(&src)
                .unwrap()
                .starts_with(b"external modification")
        );
    }

    #[test]
    fn windows_native_copy_staging_rejects_unknown_security_link_and_malformed_headers() {
        for kind in [0u32, 3, 5, 7, 9, u32::MAX] {
            let mut header = [0u8; HEADER_BYTES];
            header[..4].copy_from_slice(&kind.to_le_bytes());
            assert!(validate_header(&header).is_err());
        }
        for (bytes, name) in [(u64::MAX, 0u32), (0, 3), (0, MAX_NAME_BYTES as u32 + 2)] {
            let mut header = [0u8; HEADER_BYTES];
            header[..4].copy_from_slice(&DATA.to_le_bytes());
            header[8..16].copy_from_slice(&bytes.to_le_bytes());
            header[16..].copy_from_slice(&name.to_le_bytes());
            assert!(validate_header(&header).is_err());
        }
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
        path::Path,
    };
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_COMPRESSION;

    struct ResetAttributes<'a>(&'a Path, &'a Path);
    impl Drop for ResetAttributes<'_> {
        fn drop(&mut self) {
            for path in [self.0, self.1] {
                let wide = path
                    .as_os_str()
                    .encode_wide()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                assert_ne!(
                    unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_NORMAL) },
                    0
                );
            }
        }
    }
    fn rw(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn set_basic(file: &File, info: &FILE_BASIC_INFO) {
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle().cast(),
                    FileBasicInfo,
                    ptr::from_ref(info).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                )
            },
            0
        );
    }
    fn compress(file: &File, format: u16) {
        let mut returned = 0;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    file.as_raw_handle().cast(),
                    FSCTL_SET_COMPRESSION,
                    ptr::from_ref(&format).cast(),
                    2,
                    ptr::null_mut(),
                    0,
                    &mut returned,
                    ptr::null_mut(),
                )
            },
            0
        );
    }
    fn paths(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let src = root.join("source");
        let dst = root.join("candidate");
        std::fs::write(&src, b"source primary data").unwrap();
        std::fs::write(&dst, b"").unwrap();
        (src, dst)
    }
    #[test]
    fn windows_native_copy_directory_payload_preserves_streams_eas_and_times() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source-directory");
        let dst = root.path().join("candidate-directory");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        let _reset = ResetAttributes(&src, &dst);
        std::fs::write(src.join("child"), b"source child must stay untouched").unwrap();
        let named = ":réservé-🦀:$DATA";
        std::fs::write(format!("{}{named}", src.display()), b"directory ADS").unwrap();
        std::fs::write(format!("{}:empty:$DATA", src.display()), b"").unwrap();
        let source = rw(&src);
        super::tests::set_fixture_ea(&source);
        set_basic(
            &source,
            &FILE_BASIC_INFO {
                CreationTime: 132_000_000_000_000_000,
                LastAccessTime: 132_100_000_000_000_000,
                LastWriteTime: 132_200_000_000_000_000,
                ChangeTime: 0,
                FileAttributes: FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
            },
        );
        drop(source);
        let source = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&src)
            .unwrap();
        let candidate = rw(&dst);
        let staged = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
        assert!(staged.streams.iter().all(|stream| stream.kind != DATA));
        assert!(
            staged
                .streams
                .iter()
                .any(|stream| stream.kind == EXTENDED_ATTRIBUTES)
        );
        assert_eq!(
            staged
                .streams
                .iter()
                .filter(|stream| stream.kind == ALTERNATE_DATA)
                .count(),
            2
        );
        staged.validate_source(&source, || Ok(())).unwrap();
        staged
            .validate_candidate_streams(&candidate, || Ok(()))
            .unwrap();
        // Stream rereads may update access time. The operation itself checked
        // final metadata after stream verification; compare the stable fields.
        let before = metadata::basic(&source).unwrap();
        let after = metadata::basic(&candidate).unwrap();
        assert_eq!(
            (
                before.CreationTime,
                before.LastWriteTime,
                before.FileAttributes
            ),
            (
                after.CreationTime,
                after.LastWriteTime,
                after.FileAttributes
            )
        );
        assert_eq!(
            std::fs::read(format!("{}{named}", dst.display())).unwrap(),
            b"directory ADS"
        );
        assert_eq!(
            std::fs::metadata(format!("{}:empty:$DATA", dst.display()))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            std::fs::read(src.join("child")).unwrap(),
            b"source child must stay untouched"
        );
        assert_eq!(
            std::fs::read_dir(&dst).unwrap().count(),
            0,
            "payload staging must not claim tree staging"
        );
    }

    #[test]
    fn windows_native_copy_directory_payload_handles_compression_and_empty_directories() {
        let root = tempfile::tempdir().unwrap();
        for compressed_source in [true, false] {
            let folder = root.path().join(if compressed_source {
                "compressed"
            } else {
                "ordinary"
            });
            std::fs::create_dir(&folder).unwrap();
            let src = folder.join("source");
            let dst = folder.join("candidate");
            std::fs::create_dir(&src).unwrap();
            std::fs::create_dir(&dst).unwrap();
            let source = rw(&src);
            let candidate = rw(&dst);
            let yes = COMPRESSION_FORMAT_LZNT1;
            let no = COMPRESSION_FORMAT_NONE;
            compress(&source, if compressed_source { yes } else { no });
            compress(&candidate, if compressed_source { no } else { yes });
            let staged = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
            assert!(
                staged.streams.is_empty(),
                "directories have no synthetic unnamed stream"
            );
            staged.validate_candidate_metadata(&candidate).unwrap();
            staged.validate_source(&source, || Ok(())).unwrap();
        }
    }

    #[test]
    fn windows_native_copy_directory_payload_rejects_type_alias_and_occupied_streams() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        let regular = root.path().join("file");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        std::fs::write(&regular, b"").unwrap();
        std::fs::write(format!("{}:keep:$DATA", dst.display()), b"unrelated").unwrap();
        let source = rw(&src);
        let candidate = rw(&dst);
        assert!(metadata::stage_entry_payload(&source, &source, |_| Ok(())).is_err());
        assert!(metadata::stage_entry_payload(&source, &rw(&regular), |_| Ok(())).is_err());
        assert!(metadata::stage_entry_payload(&rw(&regular), &candidate, |_| Ok(())).is_err());
        assert!(metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).is_err());
        assert_eq!(
            std::fs::read(format!("{}:keep:$DATA", dst.display())).unwrap(),
            b"unrelated"
        );
        assert!(std::fs::read_dir(src).unwrap().next().is_none());
    }

    #[test]
    fn windows_native_copy_directory_payload_detects_same_size_stream_tampering() {
        let root = tempfile::tempdir().unwrap();
        for mutate_source in [true, false] {
            let folder = root.path().join(if mutate_source {
                "source-change"
            } else {
                "candidate-change"
            });
            std::fs::create_dir(&folder).unwrap();
            let src = folder.join("source");
            let dst = folder.join("candidate");
            std::fs::create_dir(&src).unwrap();
            std::fs::create_dir(&dst).unwrap();
            std::fs::write(format!("{}:note:$DATA", src.display()), b"original").unwrap();
            let source = rw(&src);
            let candidate = rw(&dst);
            let staged = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
            let changed = if mutate_source { &src } else { &dst };
            std::fs::write(format!("{}:note:$DATA", changed.display()), b"modified").unwrap();
            if mutate_source {
                assert!(staged.validate_source(&source, || Ok(())).is_err());
                staged
                    .validate_candidate_streams(&candidate, || Ok(()))
                    .unwrap();
            } else {
                assert!(
                    staged
                        .validate_candidate_streams(&candidate, || Ok(()))
                        .is_err()
                );
                staged.validate_source(&source, || Ok(())).unwrap();
            }
            let retained = if mutate_source { &dst } else { &src };
            assert_eq!(
                std::fs::read(format!("{}:note:$DATA", retained.display())).unwrap(),
                b"original"
            );
            assert_eq!(
                std::fs::read(format!("{}:note:$DATA", changed.display())).unwrap(),
                b"modified"
            );
        }
    }

    #[test]
    fn windows_native_copy_directory_payload_cancellation_preserves_the_source() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        let bytes = vec![47u8; BUFFER_BYTES * 3];
        std::fs::write(format!("{}:large:$DATA", src.display()), &bytes).unwrap();
        let source = rw(&src);
        let candidate = rw(&dst);
        assert!(
            metadata::stage_entry_payload(&source, &candidate, |copied| {
                if copied > 0 {
                    Err(io("injected directory cancellation"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err()
            .to_string()
            .contains("injected directory cancellation")
        );
        drop((source, candidate));
        assert_eq!(
            std::fs::read(format!("{}:large:$DATA", src.display())).unwrap(),
            bytes
        );
        // Native backup contexts were aborted; both entries remain owned by
        // their caller and can be reopened exclusively after failure.
        for path in [&src, &dst] {
            OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap();
        }
    }

    #[test]
    fn windows_native_copy_metadata_preserves_times_attributes_and_streams() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = paths(root.path());
        let _reset = ResetAttributes(&src, &dst);
        std::fs::write(format!("{}:named:$DATA", src.display()), b"alternate").unwrap();
        let info = FILE_BASIC_INFO {
            CreationTime: 132_000_000_000_000_000,
            LastAccessTime: 132_100_000_000_000_000,
            LastWriteTime: 132_200_000_000_000_000,
            ChangeTime: 0,
            FileAttributes: FILE_ATTRIBUTE_READONLY
                | FILE_ATTRIBUTE_HIDDEN
                | FILE_ATTRIBUTE_SYSTEM
                | FILE_ATTRIBUTE_ARCHIVE
                | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
        };
        set_basic(&rw(&src), &info);
        let source = File::open(&src).unwrap();
        let candidate = rw(&dst);
        let evidence = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
        assert_eq!(evidence.streams.len(), 2);
        evidence.validate_source(&source, || Ok(())).unwrap();
        evidence.validate_candidate_metadata(&candidate).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(&dst).unwrap());
        assert_eq!(
            std::fs::read(format!("{}:named:$DATA", dst.display())).unwrap(),
            b"alternate"
        );
        drop((source, candidate));
    }
    #[test]
    fn windows_native_copy_metadata_preserves_compression_and_removes_inherited_compression() {
        let root = tempfile::tempdir().unwrap();
        for compressed_source in [true, false] {
            let folder = root.path().join(if compressed_source {
                "compressed"
            } else {
                "ordinary"
            });
            std::fs::create_dir(&folder).unwrap();
            let (src, dst) = paths(&folder);
            let source = rw(&src);
            let candidate = rw(&dst);
            compress(
                if compressed_source {
                    &source
                } else {
                    &candidate
                },
                COMPRESSION_FORMAT_LZNT1,
            );
            let evidence = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
            evidence.validate_candidate_metadata(&candidate).unwrap();
            assert_eq!(
                metadata::basic(&candidate).unwrap().FileAttributes & FILE_ATTRIBUTE_COMPRESSED
                    != 0,
                compressed_source
            );
            assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(&dst).unwrap());
        }
    }
    #[test]
    fn windows_native_copy_metadata_keeps_private_candidate_security() {
        use super::tests::{assert_owner_only, parent};
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        std::fs::write(&src, b"private until published").unwrap();
        let parent = parent(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(false).unwrap();
        let source = File::open(src).unwrap();
        metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
        assert_owner_only(&candidate);
        assert_owner_only(&stage.directory);
    }
    #[test]
    fn windows_native_copy_metadata_detects_changed_source_and_candidate() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = paths(root.path());
        let source = rw(&src);
        let candidate = rw(&dst);
        let evidence = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
        let mut info = metadata::basic(&candidate).unwrap();
        info.LastWriteTime -= 10_000_000;
        set_basic(&candidate, &info);
        assert!(evidence.validate_candidate_metadata(&candidate).is_err());
        let mut info = metadata::basic(&source).unwrap();
        info.FileAttributes = FILE_ATTRIBUTE_HIDDEN;
        set_basic(&source, &info);
        assert!(evidence.validate_source(&source, || Ok(())).is_err());
    }
    #[test]
    fn windows_native_copy_metadata_refuses_candidate_alias_and_same_size_tampering() {
        use std::io::{Seek, SeekFrom, Write};
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = paths(root.path());
        let source = rw(&src);
        let mut candidate = rw(&dst);
        let evidence = metadata::stage_entry_payload(&source, &candidate, |_| Ok(())).unwrap();
        assert!(evidence.validate_candidate_metadata(&source).is_err());
        let before = metadata::basic(&candidate).unwrap();
        candidate.seek(SeekFrom::Start(0)).unwrap();
        candidate.write_all(b"edited primary data").unwrap();
        set_basic(&candidate, &before);
        assert!(
            evidence
                .validate_candidate_streams(&candidate, || Ok(()))
                .is_err()
        );
        assert_eq!(std::fs::read(src).unwrap(), b"source primary data");
    }

    #[test]
    fn windows_native_copy_metadata_cancellation_never_applies_final_metadata() {
        let root = tempfile::tempdir().unwrap();
        let (src, dst) = paths(root.path());
        let source = rw(&src);
        let candidate = rw(&dst);
        let mut info = metadata::basic(&source).unwrap();
        info.CreationTime = 132_000_000_000_000_000;
        set_basic(&source, &info);
        let before = metadata::basic(&candidate).unwrap().CreationTime;
        assert!(
            metadata::stage_entry_payload(&source, &candidate, |_| Err(io("cancelled"))).is_err()
        );
        assert_eq!(metadata::basic(&candidate).unwrap().CreationTime, before);
        assert_eq!(std::fs::read(src).unwrap(), b"source primary data");
    }
}
