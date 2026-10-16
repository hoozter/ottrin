//! File/directory payload metadata. Directory children are staged separately.
//! Security remains private until publication.
use super::{EntryVersion, PlatformError, StreamEvidence, io, stage_entry_streams, version};
use std::{fs::File, mem::size_of, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_COMPRESSION, FSCTL_SET_COMPRESSION};

const ORDINARY_ATTRIBUTES: u32 = FILE_ATTRIBUTE_READONLY
    | FILE_ATTRIBUTE_HIDDEN
    | FILE_ATTRIBUTE_SYSTEM
    | FILE_ATTRIBUTE_ARCHIVE
    | FILE_ATTRIBUTE_TEMPORARY
    | FILE_ATTRIBUTE_OFFLINE
    | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
const ADMITTED_ATTRIBUTES: u32 = ORDINARY_ATTRIBUTES
    | FILE_ATTRIBUTE_NORMAL
    | FILE_ATTRIBUTE_COMPRESSED
    | FILE_ATTRIBUTE_DIRECTORY;

pub(super) fn basic(file: &File) -> Result<FILE_BASIC_INFO, PlatformError> {
    let mut info = FILE_BASIC_INFO::default();
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileBasicInfo,
            ptr::from_mut(&mut info).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(info)
}

fn compression(file: &File) -> Result<u16, PlatformError> {
    let mut format = 0u16;
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle().cast(),
            FSCTL_GET_COMPRESSION,
            ptr::null(),
            0,
            ptr::from_mut(&mut format).cast(),
            size_of::<u16>() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if returned != size_of::<u16>() as u32
        || ![COMPRESSION_FORMAT_NONE, COMPRESSION_FORMAT_LZNT1].contains(&format)
    {
        return Err(PlatformError::UnsupportedSafeCopy(
            "unrecognized native compression format".into(),
        ));
    }
    Ok(format)
}

fn set_compression(file: &File, format: u16) -> Result<(), PlatformError> {
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle().cast(),
            FSCTL_SET_COMPRESSION,
            ptr::from_ref(&format).cast(),
            size_of::<u16>() as u32,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if compression(file)? != format {
        return Err(io("candidate compression did not match"));
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct StagedEntry {
    pub(super) streams: Vec<StreamEvidence>,
    source: EntryVersion,
    candidate_identity: crate::move_recovery::WindowsStableIdentity,
    accessed: i64,
    compression: u16,
}

impl StagedEntry {
    pub(super) fn validate_source(&self, source: &File) -> Result<(), PlatformError> {
        if version(source)? != self.source
            || compression(source)? != self.compression
            || super::transfer(source, None, &mut |_| Ok(()))? != self.streams
            || version(source)? != self.source
        {
            return Err(io("source metadata changed during staging"));
        }
        Ok(())
    }

    pub(super) fn validate_candidate_streams(&self, candidate: &File) -> Result<(), PlatformError> {
        if super::stable_identity_from_handle(candidate).map_err(io)? != self.candidate_identity
            || super::transfer(candidate, None, &mut |_| Ok(()))? != self.streams
        {
            return Err(io("candidate identity or streams changed"));
        }
        Ok(())
    }

    /// Metadata-only check: BackupRead may update access times through its
    /// internal stream handles. Verify streams before final metadata application.
    pub(super) fn validate_candidate_metadata(
        &self,
        candidate: &File,
    ) -> Result<(), PlatformError> {
        if super::stable_identity_from_handle(candidate).map_err(io)? != self.candidate_identity {
            return Err(io("candidate identity changed"));
        }
        let observed = basic(candidate)?;
        if observed.CreationTime != self.source.created
            || observed.LastWriteTime != self.source.modified
            || observed.LastAccessTime != self.accessed
            || observed.FileAttributes != self.source.attributes
            || (!self.source.directory
                && candidate.metadata().map_err(io)?.len() != self.source.length)
            || compression(candidate)? != self.compression
        {
            return Err(io(format!(
                "candidate file metadata did not match: creation {}/{}, modified {}/{}, accessed {}/{}, attributes {:x}/{:x}, length {}/{}",
                observed.CreationTime,
                self.source.created,
                observed.LastWriteTime,
                self.source.modified,
                observed.LastAccessTime,
                self.accessed,
                observed.FileAttributes,
                self.source.attributes,
                candidate.metadata().map_err(io)?.len(),
                self.source.length
            )));
        }
        Ok(())
    }
}

/// Stage data and ordinary metadata while retaining the candidate's private
/// security descriptor. This does not enumerate or stage directory children.
/// The caller owns retained data on every failure.
pub(super) fn stage_entry_payload(
    source: &File,
    candidate: &File,
    progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<StagedEntry, PlatformError> {
    let before = version(source)?;
    if before.attributes & !ADMITTED_ATTRIBUTES != 0 {
        return Err(PlatformError::UnsupportedSafeCopy(
            "file attributes require an unimplemented preservation path".into(),
        ));
    }
    let accessed = basic(source)?.LastAccessTime;
    let format = compression(source)?;
    if (before.attributes & FILE_ATTRIBUTE_COMPRESSED != 0) != (format != COMPRESSION_FORMAT_NONE) {
        return Err(io("source compression metadata disagrees"));
    }
    // Admission and alias/empty-candidate validation happen before any mutation.
    let target = version(candidate)?;
    if target.identity == before.identity
        || target.directory != before.directory
        || target.length != 0
    {
        return Err(io(
            "candidate aliases the source, has a different type, or is not empty",
        ));
    }
    let streams = stage_entry_streams(source, candidate, progress)?;
    if compression(candidate)? != format {
        set_compression(candidate, format)?;
    }
    let result = StagedEntry {
        streams,
        source: before,
        candidate_identity: target.identity,
        accessed,
        compression: format,
    };
    result.validate_source(source)?;
    result.validate_candidate_streams(candidate)?;
    let attributes = result.source.attributes & ORDINARY_ATTRIBUTES;
    let info = FILE_BASIC_INFO {
        CreationTime: result.source.created,
        LastAccessTime: accessed,
        LastWriteTime: result.source.modified,
        ChangeTime: 0, // A copy is a new object; do not forge its change history.
        FileAttributes: if attributes == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            attributes
        },
    };
    if unsafe {
        SetFileInformationByHandle(
            candidate.as_raw_handle().cast(),
            FileBasicInfo,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    candidate.sync_all().map_err(io)?;
    result.validate_candidate_metadata(candidate)?;
    result.validate_source(source)?;
    Ok(result)
}
