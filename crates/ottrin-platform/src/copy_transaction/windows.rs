//! Handle-bound Windows Copy staging. This module never publishes or removes names.
//!
//! Backup streams preserve named data and extended attributes that ordinary
//! ReadFile/WriteFile copying loses. Security, reparse and other unsupported
//! backup records are refused rather than silently omitted. Final file metadata,
//! private staging, publication journals and Undo remain separate required work.
#![cfg_attr(not(test), allow(dead_code))]

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
    FILE_ATTRIBUTE_SPARSE_FILE, FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

const HEADER_BYTES: usize = 20;
const BUFFER_BYTES: usize = 64 * 1024;
const MAX_NAME_BYTES: usize = 65_534;
const DATA: u32 = 1;
const EXTENDED_ATTRIBUTES: u32 = 2;
const ALTERNATE_DATA: u32 = 4;
const PROPERTY_DATA: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StreamEvidence {
    kind: u32,
    attributes: u32,
    name: Vec<u16>,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
struct FileVersion {
    identity: WindowsStableIdentity,
    length: u64,
    created: i64,
    modified: i64,
    changed: i64,
    attributes: u32,
}

fn io(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::Io(format!("Windows Copy stream staging: {error}"))
}

fn version(file: &File) -> Result<FileVersion, PlatformError> {
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
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileBasicInfo,
            ptr::from_mut(&mut basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if !identity.is_regular_file_without_reparse()
        || basic.FileAttributes
            & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_ENCRYPTED | FILE_ATTRIBUTE_SPARSE_FILE)
            != 0
    {
        return Err(PlatformError::UnsupportedSafeCopy(
            "this staging engine requires an ordinary non-sparse, unencrypted file".into(),
        ));
    }
    Ok(FileVersion {
        identity,
        length: file.metadata().map_err(io)?.len(),
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
        let mut position = file;
        position.seek(SeekFrom::Start(0)).map_err(io)?;
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
        DATA | EXTENDED_ATTRIBUTES | ALTERNATE_DATA | PROPERTY_DATA
    ) || bytes > i64::MAX as u64
        || name > MAX_NAME_BYTES
        || !name.is_multiple_of(2)
        || (kind == ALTERNATE_DATA) != (name != 0)
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
    let mut reader = Backup::new(source, Direction::Read)?;
    let mut writer = target
        .map(|file| Backup::new(file, Direction::Write))
        .transpose()?;
    let mut evidence = Vec::new();
    let mut buffer = [0u8; BUFFER_BYTES];
    let mut total = 0u64;
    progress(0)?;
    loop {
        let mut header = [0u8; HEADER_BYTES];
        if !reader.read_exact(&mut header, true)? {
            break;
        }
        let (kind, attributes, bytes, name_bytes) = validate_header(&header)?;
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
    reader.finish()?;
    if let Some(writer) = &mut writer {
        writer.finish()?;
    }
    // BackupRead omits an empty unnamed stream. The transaction manifest still
    // needs its logical existence, independent of native record enumeration.
    if !evidence.iter().any(|stream| stream.kind == DATA) {
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

/// Copy only into an independently created, empty private candidate held open
/// by the transaction owner. No pathname is reopened or removed here. Sharing
/// policy and ownership of that candidate are the caller's native stage boundary.
pub(super) fn stage_file_streams(
    source: &File,
    candidate: &File,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<Vec<StreamEvidence>, PlatformError> {
    let before = version(source)?;
    let target = version(candidate)?;
    if before.identity == target.identity || target.length != 0 {
        return Err(io("candidate aliases the source or is not empty"));
    }
    if transfer(candidate, None, &mut |_| Ok(()))?
        .iter()
        .any(|stream| stream.kind != DATA || stream.bytes != 0)
    {
        return Err(io("candidate already contains named data or metadata"));
    }
    let copied = transfer(source, Some(candidate), &mut progress)?;
    candidate.sync_all().map_err(io)?;
    if version(source)? != before
        || transfer(source, None, &mut |_| Ok(()))? != copied
        || version(source)? != before
        || transfer(candidate, None, &mut |_| Ok(()))? != copied
    {
        return Err(io("source changed or staged streams do not match"));
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let streams = stage_file_streams(&read, &staged, |_| Ok(())).unwrap();
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
        let streams = stage_file_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
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
            stage_file_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
            assert_eq!(std::fs::read(&dst).unwrap(), bytes);
        }
    }

    #[test]
    fn windows_native_copy_staging_preserves_extended_attributes() {
        use windows_sys::Wdk::Storage::FileSystem::NtSetEaFile;
        use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"primary").unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&src)
            .unwrap();
        let mut ea = [0u8; 16];
        ea[5] = 4;
        ea[6..8].copy_from_slice(&3u16.to_le_bytes());
        ea[8..13].copy_from_slice(b"user\0");
        ea[13..].copy_from_slice(b"abc");
        let mut status = IO_STATUS_BLOCK::default();
        assert!(
            unsafe {
                NtSetEaFile(
                    file.as_raw_handle().cast(),
                    &mut status,
                    ea.as_ptr().cast(),
                    ea.len() as u32,
                )
            } >= 0
        );
        drop(file);
        let streams = stage_file_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
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
    struct ReadOnlyAcl(Vec<PathBuf>);
    fn set_dacl(path: &Path, text: &str) {
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
        stage_file_streams(&source(&src), &candidate(&dst), |_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"readable source");
        assert_eq!(std::fs::read(&dst).unwrap(), b"readable source");
        drop(guard);
    }

    #[test]
    fn windows_native_copy_staging_refuses_occupied_or_named_stream_candidates() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"new").unwrap();
        let file = candidate(&dst);
        (&file).write_all(b"existing").unwrap();
        assert!(stage_file_streams(&source(&src), &file, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"existing");
        drop(file);
        std::fs::write(&dst, b"").unwrap();
        std::fs::write(format!("{}:keep:$DATA", dst.display()), b"keep").unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&dst)
            .unwrap();
        assert!(stage_file_streams(&source(&src), &file, |_| Ok(())).is_err());
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
        assert!(stage_file_streams(&src_file, &alias_file, |_| Ok(())).is_err());
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
        let result = stage_file_streams(&source(&src), &staged, |copied| {
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
    fn windows_native_copy_staging_write_failure_releases_native_contexts() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"original").unwrap();
        std::fs::write(&dst, b"").unwrap();
        let read_only_candidate = source(&dst);
        let error =
            stage_file_streams(&source(&src), &read_only_candidate, |_| Ok(())).unwrap_err();
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
        stage_file_streams(&read, &copied, |_| Ok(())).unwrap();
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
                stage_file_streams(&read, &staged, |_| Ok(()))
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
        let result = stage_file_streams(&read, &candidate(&dst), |bytes| {
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
        for kind in [0u32, 3, 5, 7, 8, 9, u32::MAX] {
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
