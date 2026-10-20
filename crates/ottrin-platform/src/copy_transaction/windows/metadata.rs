//! File/directory payload metadata. Directory children are staged separately.
//! Security remains private until publication.
use super::{EntryVersion, PlatformError, StreamEvidence, io, stage_entry_streams, version};
use crate::windows_fs;
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
pub(super) const ADMITTED_ATTRIBUTES: u32 = ORDINARY_ATTRIBUTES
    | FILE_ATTRIBUTE_NORMAL
    | FILE_ATTRIBUTE_COMPRESSED
    | FILE_ATTRIBUTE_DIRECTORY
    | FILE_ATTRIBUTE_REPARSE_POINT
    | FILE_ATTRIBUTE_SPARSE_FILE;

/// Only a paired, identity-bound rename metadata intent authorizes this write.
/// It preserves the creation time through NTFS tunneling without rewriting
/// access/write times, attributes, compression or payload.
pub(super) fn set_creation_time(
    file: &File,
    before: &EntryVersion,
    created: i64,
) -> Result<(), PlatformError> {
    if version(file)? != *before {
        return Err(io("Copy creation-time intent changed before its write"));
    }
    let info = FILE_BASIC_INFO {
        CreationTime: created,
        ..Default::default()
    };
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle().cast(),
            FileBasicInfo,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    Ok(())
}

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

pub(super) fn compression(file: &File) -> Result<u16, PlatformError> {
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StagedEntry {
    pub(super) streams: Vec<StreamEvidence>,
    pub(super) source: EntryVersion,
    pub(super) candidate_identity: crate::move_recovery::WindowsStableIdentity,
    pub(super) accessed: i64,
    pub(super) compression: u16,
}

impl StagedEntry {
    fn validate_case_policy(&self, candidate: &File) -> Result<(), PlatformError> {
        if let Some(expected) = self.source.case_sensitive
            && windows_fs::directory_case_sensitive(candidate).map_err(io)? != expected
        {
            return Err(io("candidate directory case policy changed"));
        }
        Ok(())
    }
    /// Caller must verify the owned candidate's streams before this last write:
    /// native backup rereads may update access times. No namespace mutation.
    pub(super) fn apply_final_metadata(&self, candidate: &File) -> Result<(), PlatformError> {
        if super::stable_identity_from_handle(candidate).map_err(io)? != self.candidate_identity
            || compression(candidate)? != self.compression
        {
            return Err(io(
                "candidate identity/compression changed before final metadata",
            ));
        }
        self.validate_case_policy(candidate)?;
        let attributes = self.source.attributes & ORDINARY_ATTRIBUTES;
        let info = FILE_BASIC_INFO {
            CreationTime: self.source.created,
            LastAccessTime: self.accessed,
            LastWriteTime: self.source.modified,
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
        self.validate_candidate_metadata(candidate)?;
        Ok(())
    }

    /// Child creation may change a directory's write/access times, but not its
    /// identity, creation time, attributes or compression policy.
    pub(super) fn validate_after_children(&self, candidate: &File) -> Result<(), PlatformError> {
        self.validate_case_policy(candidate)?;
        let actual = basic(candidate)?;
        if super::stable_identity_from_handle(candidate).map_err(io)? != self.candidate_identity
            || actual.CreationTime != self.source.created
            || actual.FileAttributes != self.source.attributes
            || (!(self.source.directory && self.source.link.is_none())
                && actual.LastWriteTime != self.source.modified)
            || compression(candidate)? != self.compression
        {
            return Err(io("candidate metadata changed during child staging"));
        }
        Ok(())
    }

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

    fn writable_attributes(&self) -> u32 {
        let attributes = self.source.attributes & !FILE_ATTRIBUTE_READONLY;
        if attributes == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            attributes
        }
    }

    /// Called only under a paired readonly-reopen intent, through an identity-bound
    /// candidate handle. All timestamps and all other attributes remain unchanged.
    pub(super) fn set_reopening_attributes(
        &self,
        candidate: &File,
        writable: bool,
    ) -> Result<(), PlatformError> {
        self.validate_reopening_metadata(candidate, true)?;
        let info = FILE_BASIC_INFO {
            FileAttributes: if writable {
                self.writable_attributes()
            } else {
                self.source.attributes & ORDINARY_ATTRIBUTES
            },
            ..Default::default()
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
        self.validate_reopening_metadata(candidate, writable)
    }

    /// Metadata-only check: BackupRead may update access times through its
    /// internal stream handles. Verify streams before final metadata application.
    pub(super) fn validate_candidate_metadata(
        &self,
        candidate: &File,
    ) -> Result<(), PlatformError> {
        self.validate_metadata(candidate, true, false)
    }

    /// Ordinary reads (including a cancelled recovery scan) may update access
    /// time. Recovery verifies persistent payload metadata without repairing or
    /// treating this filesystem-owned read timestamp as a content mutation.
    pub(super) fn validate_retained_metadata(&self, candidate: &File) -> Result<(), PlatformError> {
        self.validate_metadata(candidate, false, false)
    }

    /// Only a paired readonly-reopen intent authorizes the temporary writable bit.
    pub(super) fn validate_reopening_metadata(
        &self,
        candidate: &File,
        pending: bool,
    ) -> Result<(), PlatformError> {
        self.validate_metadata(candidate, false, pending)
    }

    fn validate_metadata(
        &self,
        candidate: &File,
        require_access_time: bool,
        readonly_reopen_pending: bool,
    ) -> Result<(), PlatformError> {
        self.validate_case_policy(candidate)?;
        if super::stable_identity_from_handle(candidate).map_err(io)? != self.candidate_identity {
            return Err(io("candidate identity changed"));
        }
        let observed = basic(candidate)?;
        if observed.CreationTime != self.source.created
            || observed.LastWriteTime != self.source.modified
            || (require_access_time && observed.LastAccessTime != self.accessed)
            || (observed.FileAttributes != self.source.attributes
                && !(readonly_reopen_pending
                    && !self.source.directory
                    && self.source.attributes & FILE_ATTRIBUTE_READONLY != 0
                    && observed.FileAttributes == self.writable_attributes()))
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
    if target.identity.same_object(before.identity)
        || target.directory != before.directory
        || target.length != 0
        || target.link.is_some()
    {
        return Err(io(
            "candidate aliases the source, has a different type, or is not empty",
        ));
    }
    if let Some(expected) = before.case_sensitive
        && target.case_sensitive != Some(expected)
    {
        windows_fs::set_empty_directory_case_policy(candidate, expected).map_err(io)?;
    }
    let streams = stage_entry_streams(source, candidate, progress)?;
    if compression(candidate)? != format {
        set_compression(candidate, format)?;
    }
    let result = StagedEntry {
        streams,
        source: before,
        candidate_identity: super::stable_identity_from_handle(candidate).map_err(io)?,
        accessed,
        compression: format,
    };
    result.validate_source(source)?;
    result.validate_candidate_streams(candidate)?;
    result.apply_final_metadata(candidate)?;
    result.validate_source(source)?;
    Ok(result)
}
