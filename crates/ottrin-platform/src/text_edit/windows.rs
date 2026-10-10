//! Native candidate, deliberately unreachable through the public acceptance gate.
//! All namespace changes are relative to directory handles and refuse replacement.
use super::windows_contract::{self, Step};
use super::{MAX_EDITABLE_TEXT_BYTES, TextEditError};
use crate::move_transaction::rename_windows_handle_no_replace;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::os::windows::{
    ffi::OsStrExt,
    fs::OpenOptionsExt,
    io::{AsRawHandle, FromRawHandle},
};
use std::path::{Path, PathBuf};
use std::ptr;
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::*;
use windows_sys::Win32::Foundation::{HANDLE, LocalFree, RtlNtStatusToDosError, UNICODE_STRING};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetSecurityInfo,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetKernelObjectSecurity,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
    LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK};
use windows_sys::Win32::System::Ioctl::{FILE_OBJECTID_BUFFER, FSCTL_GET_OBJECT_ID};

#[derive(Debug)]
pub(super) struct Session {
    parent: File,
    name: OsString,
    source: Snapshot,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    identity: (u32, u32, u32),
    creation: i64,
    modified: i64,
    changed: i64,
    attributes: u32,
    security: Vec<u8>,
    bytes: Vec<u8>,
}

const CAPTURED_SECURITY_INFORMATION: u32 = OWNER_SECURITY_INFORMATION
    | GROUP_SECURITY_INFORMATION
    | DACL_SECURITY_INFORMATION
    | LABEL_SECURITY_INFORMATION;

impl Session {
    pub(super) fn open(path: &Path) -> Result<Self, TextEditError> {
        let name = path
            .file_name()
            .ok_or(TextEditError::NotRegularFile)?
            .to_owned();
        valid_name(&name)?;
        // The parent handle is the namespace authority, even if an ancestor moves.
        let parent = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path.parent().ok_or(TextEditError::NotRegularFile)?)
            .map_err(read_error)?;
        require_local_ntfs(&parent)?;
        let mut source = open_source(&parent, &name)?;
        let snapshot = Snapshot::capture(&mut source)?;
        Ok(Self {
            parent,
            name,
            source: snapshot,
        })
    }

    pub(super) fn contents(&self) -> &str {
        std::str::from_utf8(&self.source.bytes).expect("validated UTF-8")
    }

    pub(super) fn save(&self, text: &str) -> Result<(), TextEditError> {
        self.save_with_hook(text, |_| Ok(()))
    }

    fn save_with_hook(
        &self,
        text: &str,
        mut hook: impl FnMut(Step) -> Result<(), TextEditError>,
    ) -> Result<(), TextEditError> {
        validate_text(text.as_bytes())?;
        let mut source =
            open_source(&self.parent, &self.name).map_err(|_| TextEditError::Conflict)?;
        if Snapshot::capture(&mut source)? != self.source {
            return Err(TextEditError::Conflict);
        }
        if text.as_bytes() == self.source.bytes {
            return Ok(());
        }
        let name = random_name()?;
        let private = PrivateSecurity::new()?;
        let stage = relative(&self.parent, &name, true, true, private.0.cast())?;
        let recovery = location(&stage)?;
        let retained = |error: TextEditError| {
            let recovery = location(&stage).unwrap_or_else(|_| recovery.clone());
            TextEditError::StageRetentionRequired(format!(
                "{error}; recovery directory: {recovery:?}; sibling publication file: {:?}; no cleanup attempted",
                name.to_string_lossy().to_string() + "-publication"
            ))
        };
        let mut draft =
            relative(&stage, OsStr::new("edits"), false, true, ptr::null()).map_err(retained)?;
        draft
            .write_all(text.as_bytes())
            .map_err(write_error)
            .map_err(retained)?;
        draft.sync_all().map_err(write_error).map_err(retained)?;
        // Create beside the original so inherited ACLs have the same parent.
        // The exact owner/group/DACL must compare equal before source withdrawal.
        let publication_name = OsString::from(name.to_string_lossy().to_string() + "-publication");
        let mut publication = relative(
            &self.parent,
            &publication_name,
            false,
            true,
            self.source.security.as_ptr().cast(),
        )
        .map_err(retained)?;
        apply_publication_security(&publication, &self.source.security).map_err(retained)?;
        publication
            .write_all(text.as_bytes())
            .map_err(write_error)
            .map_err(retained)?;
        let basic = FILE_BASIC_INFO {
            CreationTime: self.source.creation,
            FileAttributes: self.source.attributes,
            ..Default::default()
        };
        if unsafe {
            SetFileInformationByHandle(
                handle(&publication),
                FileBasicInfo,
                ptr::from_ref(&basic).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(retained(last_error()));
        }
        publication
            .sync_all()
            .map_err(write_error)
            .map_err(retained)?;
        let candidate = Snapshot::capture(&mut publication).map_err(retained)?;
        if candidate.security != self.source.security
            || candidate.creation != self.source.creation
            || candidate.attributes != self.source.attributes
        {
            return Err(retained(TextEditError::UnsupportedMetadata));
        }
        sync_directories(&stage, &self.parent).map_err(retained)?;
        let result = windows_contract::publish(&recovery, |step| {
            hook(step)?;
            match step {
                Step::VerifySource => {
                    if Snapshot::capture(&mut source)? != self.source {
                        return Err(TextEditError::Conflict);
                    }
                    Ok(())
                }
                Step::RetainSource => {
                    rename_windows_handle_no_replace(&source, &stage, OsStr::new("candidate"))
                        .map_err(write_error)
                }
                Step::VerifyRetained => self.source.verify_after_rename(&mut source),
                Step::FlushRetained => sync_directories(&stage, &self.parent),
                Step::Publish => {
                    rename_windows_handle_no_replace(&publication, &self.parent, &self.name)
                        .map_err(write_error)
                }
                Step::VerifyPublished => {
                    self.source.verify_after_rename(&mut source)?;
                    candidate.verify_after_rename(&mut publication)
                }
                Step::FlushPublished => {
                    publication.sync_all().map_err(write_error)?;
                    sync_directories(&stage, &self.parent)
                }
                Step::RestoreVacant => link_no_replace(&source, &self.parent, &self.name)
                    .and_then(|()| sync_directories(&stage, &self.parent)),
            }
        });
        result.map_err(|error| match error {
            TextEditError::RecoveryConflict { detail, .. } => TextEditError::RecoveryConflict {
                recovery: location(&stage).unwrap_or_else(|_| recovery.clone()),
                detail,
            },
            TextEditError::StageRetentionRequired(detail) => retained(TextEditError::Write(detail)),
            error => error,
        })
    }
}

fn handle(file: &File) -> HANDLE {
    file.as_raw_handle().cast()
}
fn read_error(error: impl std::fmt::Display) -> TextEditError {
    TextEditError::Read(error.to_string())
}
fn write_error(error: impl std::fmt::Display) -> TextEditError {
    TextEditError::Write(error.to_string())
}
fn last_error() -> TextEditError {
    write_error(std::io::Error::last_os_error())
}
fn nt_result(status: i32) -> Result<(), TextEditError> {
    if status >= 0 {
        Ok(())
    } else {
        Err(write_error(std::io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        )))
    }
}
fn valid_name(name: &OsStr) -> Result<Vec<u16>, TextEditError> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if wide.is_empty()
        || wide.len() > 255
        || wide
            .iter()
            .any(|c| [0, b':' as u16, b'/' as u16, b'\\' as u16].contains(c))
        || name == "."
        || name == ".."
    {
        return Err(TextEditError::NotRegularFile);
    }
    Ok(wide)
}

/// FILE_CREATE never opens or overwrites an existing entry. FILE_OPEN_REPARSE_POINT
/// and a single leaf name prevent following a source/stage reparse point.
fn relative(
    parent: &File,
    name: &OsStr,
    directory: bool,
    create: bool,
    security: *const windows_sys::Win32::Security::SECURITY_DESCRIPTOR,
) -> Result<File, TextEditError> {
    let mut wide = valid_name(name)?;
    let unicode = UNICODE_STRING {
        Length: (wide.len() * 2) as u16,
        MaximumLength: (wide.len() * 2) as u16,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: handle(parent),
        ObjectName: &unicode,
        SecurityDescriptor: security,
        ..Default::default()
    };
    let mut raw = ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let access = if directory {
        FILE_GENERIC_READ | FILE_GENERIC_WRITE
    } else if create {
        FILE_GENERIC_READ
            | FILE_GENERIC_WRITE
            | DELETE
            | if security.is_null() { 0 } else { WRITE_DAC }
    } else {
        FILE_GENERIC_READ | DELETE
    };
    // Deny write/delete sharing on file handles for the duration of save. This
    // excludes ordinary writers, not pre-existing writable memory mappings.
    nt_result(unsafe {
        NtCreateFile(
            &mut raw,
            access,
            &attributes,
            &mut status,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE
            } else {
                FILE_SHARE_READ
            },
            if create { FILE_CREATE } else { FILE_OPEN },
            FILE_OPEN_REPARSE_POINT
                | FILE_SYNCHRONOUS_IO_NONALERT
                | if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                },
            ptr::null(),
            0,
        )
    })?;
    Ok(unsafe { File::from_raw_handle(raw.cast()) })
}
// NtCreateFile preserves the ACL entries but does not retain the automatic
// inheritance state of a supplied descriptor. Finalize
// the new file's security through the filesystem API before exact comparison.
fn apply_publication_security(file: &File, security: &[u8]) -> Result<(), TextEditError> {
    let descriptor = security.as_ptr().cast_mut().cast();
    let mut control = 0;
    let mut revision = 0;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = ptr::null_mut();
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == 0
    {
        return Err(last_error());
    }
    if present == 0 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    require_absent_sacl(security)?;
    let protection = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    let error = unsafe {
        SetSecurityInfo(
            handle(file),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if error != 0 {
        return Err(write_error(std::io::Error::from_raw_os_error(error as i32)));
    }
    Ok(())
}

// LABEL_SECURITY_INFORMATION preserves SACL presence while filtering out audit
// ACEs. A partial SACL cannot prove that publication would retain audit rules.
// Admit only absence; do not reconstruct security from the visible subset.
fn require_absent_sacl(security: &[u8]) -> Result<(), TextEditError> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut sacl = ptr::null_mut();
    if unsafe {
        GetSecurityDescriptorSacl(
            security.as_ptr().cast_mut().cast(),
            &mut present,
            &mut sacl,
            &mut defaulted,
        )
    } == 0
    {
        return Err(last_error());
    }
    if present != 0 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    Ok(())
}

fn open_source(parent: &File, name: &OsStr) -> Result<File, TextEditError> {
    relative(parent, name, false, false, ptr::null())
}

impl Snapshot {
    fn capture(file: &mut File) -> Result<Self, TextEditError> {
        let before = metadata(file)?;
        file.seek(SeekFrom::Start(0)).map_err(read_error)?;
        let mut bytes = Vec::new();
        Read::by_ref(file)
            .take(MAX_EDITABLE_TEXT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        validate_text(&bytes)?;
        let after = metadata(file)?;
        if before != after {
            return Err(TextEditError::Conflict);
        }
        let (identity, creation, modified, changed, attributes, security, length) = after;
        if bytes.len() as u64 != length {
            return Err(TextEditError::Conflict);
        }
        Ok(Self {
            identity,
            creation,
            modified,
            changed,
            attributes,
            security,
            bytes,
        })
    }
    fn verify_after_rename(&self, file: &mut File) -> Result<(), TextEditError> {
        let mut current = Self::capture(file)?;
        // Rename changes ChangeTime; retained originals cover mapped late writers.
        current.changed = self.changed;
        if *self == current {
            Ok(())
        } else {
            Err(TextEditError::Conflict)
        }
    }
}

type Metadata = ((u32, u32, u32), i64, i64, i64, u32, Vec<u8>, u64);
fn metadata(file: &File) -> Result<Metadata, TextEditError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    let mut basic = FILE_BASIC_INFO::default();
    if unsafe { GetFileInformationByHandle(handle(file), &mut info) } == 0
        || unsafe {
            GetFileInformationByHandleEx(
                handle(file),
                FileBasicInfo,
                ptr::from_mut(&mut basic).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
    {
        return Err(last_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(TextEditError::Symlink);
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(TextEditError::NotRegularFile);
    }
    if info.nNumberOfLinks != 1 {
        return Err(TextEditError::HardLinked);
    }
    if info.dwFileAttributes & !(FILE_ATTRIBUTE_ARCHIVE | FILE_ATTRIBUTE_NORMAL) != 0 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    // Object IDs belong to the original NTFS object and cannot be duplicated
    // onto a new publication while retaining that original. Refuse before staging.
    let mut object_id = FILE_OBJECTID_BUFFER::default();
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            handle(file),
            FSCTL_GET_OBJECT_ID,
            ptr::null(),
            0,
            ptr::from_mut(&mut object_id).cast(),
            size_of::<FILE_OBJECTID_BUFFER>() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    } != 0
    {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND as i32) {
        return Err(read_error(error));
    }
    if info.nFileSizeHigh != 0 || u64::from(info.nFileSizeLow) > MAX_EDITABLE_TEXT_BYTES {
        return Err(TextEditError::TooLarge {
            limit: MAX_EDITABLE_TEXT_BYTES,
        });
    }
    // Only the default data stream is admitted; oversized stream lists fail closed.
    let mut streams = [0u64; 512];
    if unsafe {
        GetFileInformationByHandleEx(
            handle(file),
            FileStreamInfo,
            streams.as_mut_ptr().cast(),
            size_of_val(&streams) as u32,
        )
    } == 0
    {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let stream = unsafe { &*streams.as_ptr().cast::<FILE_STREAM_INFO>() };
    if stream.NextEntryOffset != 0
        || stream.StreamNameLength != 14
        || unsafe { std::slice::from_raw_parts(ptr::addr_of!(stream.StreamName).cast::<u16>(), 7) }
            != [58, 58, 36, 68, 65, 84, 65]
    {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let mut ea = FILE_EA_INFORMATION::default();
    let mut io = IO_STATUS_BLOCK::default();
    nt_result(unsafe {
        NtQueryInformationFile(
            handle(file),
            &mut io,
            ptr::from_mut(&mut ea).cast(),
            size_of::<FILE_EA_INFORMATION>() as u32,
            FileEaInformation,
        )
    })?;
    if ea.EaSize != 0 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let mut needed = 0;
    unsafe {
        GetKernelObjectSecurity(
            handle(file),
            CAPTURED_SECURITY_INFORMATION,
            ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if needed == 0 || needed > 65536 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let mut security = vec![0u8; needed as usize];
    if unsafe {
        GetKernelObjectSecurity(
            handle(file),
            CAPTURED_SECURITY_INFORMATION,
            security.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last_error());
    }
    require_absent_sacl(&security)?;
    Ok((
        (
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ),
        basic.CreationTime,
        basic.LastWriteTime,
        basic.ChangeTime,
        basic.FileAttributes,
        security,
        u64::from(info.nFileSizeLow),
    ))
}
fn validate_text(bytes: &[u8]) -> Result<(), TextEditError> {
    if bytes.len() as u64 > MAX_EDITABLE_TEXT_BYTES {
        return Err(TextEditError::TooLarge {
            limit: MAX_EDITABLE_TEXT_BYTES,
        });
    }
    let text = std::str::from_utf8(bytes).map_err(|_| TextEditError::InvalidUtf8)?;
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(TextEditError::BinaryText);
    }
    Ok(())
}
fn require_local_ntfs(parent: &File) -> Result<(), TextEditError> {
    // FILE_FS_DEVICE_INFORMATION is two ULONGs (DeviceType, Characteristics).
    let mut device = [0u32; 2];
    let mut io = IO_STATUS_BLOCK::default();
    nt_result(unsafe {
        NtQueryVolumeInformationFile(
            handle(parent),
            &mut io,
            ptr::from_mut(&mut device).cast(),
            size_of_val(&device) as u32,
            FileFsDeviceInformation,
        )
    })?;
    // FILE_REMOTE_DEVICE = 0x10. The native boundary is local NTFS only.
    if device[1] & 0x10 != 0 {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let mut name = [0u16; 32];
    if unsafe {
        GetVolumeInformationByHandleW(
            handle(parent),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    } == 0
        || name[..5] != [78, 84, 70, 83, 0]
    {
        return Err(TextEditError::UnsupportedMetadata);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle(parent), &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(TextEditError::UnsupportedMetadata);
    }
    Ok(())
}
fn location(file: &File) -> Result<PathBuf, TextEditError> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle(file),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        return Err(last_error());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}
fn sync_directories(stage: &File, parent: &File) -> Result<(), TextEditError> {
    stage.sync_all().map_err(write_error)?;
    parent.sync_all().map_err(write_error)
}
fn random_name() -> Result<OsString, TextEditError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(write_error)?;
    Ok(format!(
        ".ottrin-text-edit-{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
    .into())
}
struct PrivateSecurity(*mut std::ffi::c_void);
impl PrivateSecurity {
    fn new() -> Result<Self, TextEditError> {
        let text: Vec<u16> = "D:P(A;OICI;FA;;;OW)\0".encode_utf16().collect();
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(Self(descriptor))
    }
}
impl Drop for PrivateSecurity {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn link_no_replace(source: &File, parent: &File, name: &OsStr) -> Result<(), TextEditError> {
    let wide = valid_name(name)?;
    let bytes = size_of::<FILE_LINK_INFORMATION>() + wide.len() * 2;
    let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_LINK_INFORMATION>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = handle(parent);
        (*info).FileNameLength = (wide.len() * 2) as u32;
        ptr::copy_nonoverlapping(
            wide.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast(),
            wide.len(),
        );
    }
    let mut status = IO_STATUS_BLOCK::default();
    nt_result(unsafe {
        NtSetInformationFile(
            handle(source),
            &mut status,
            buffer.as_ptr().cast(),
            bytes as u32,
            FileLinkInformation,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Security::ATTRIBUTE_SECURITY_INFORMATION;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(random_name().unwrap());
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("source.md"), b"original\r\n").unwrap();
            Self(root)
        }
        fn source(&self) -> PathBuf {
            self.0.join("source.md")
        }
        fn recovery(&self) -> PathBuf {
            std::fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.is_dir())
                .expect("retained recovery directory")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn windows_native_public_session_cannot_bypass_acceptance_gate() {
        let fixture = Fixture::new();
        assert!(matches!(
            super::super::TextEditSession::open(&fixture.source()),
            Err(TextEditError::NotAvailable)
        ));
    }

    #[test]
    fn windows_native_save_retains_original_and_independent_draft() {
        for acl in ["inherited", "protected", "explicit"] {
            let fixture = Fixture::new();
            if acl != "inherited" {
                let mut command = std::process::Command::new("icacls.exe");
                command.arg(fixture.source());
                if acl == "protected" {
                    command.arg("/inheritance:d");
                } else {
                    command.args(["/grant:r", "*S-1-5-32-545:R"]);
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "owned fixture ACL: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let session = Session::open(&fixture.source()).expect("native NTFS admission");
            session
                .save("\u{feff}edited\r\n")
                .expect("native publication and directory flush");
            assert_eq!(
                std::fs::read(fixture.source()).unwrap(),
                "\u{feff}edited\r\n".as_bytes()
            );
            let recovery = fixture.recovery();
            assert_eq!(
                std::fs::read(recovery.join("candidate")).unwrap(),
                b"original\r\n"
            );
            assert_eq!(
                std::fs::read(recovery.join("edits")).unwrap(),
                "\u{feff}edited\r\n".as_bytes()
            );
            let saved = Session::open(&fixture.source()).unwrap();
            assert_eq!(saved.source.security, session.source.security);
            assert_eq!(session.contents(), "original\r\n");
        }
    }

    #[test]
    fn windows_native_unicode_and_ancestor_paths_remain_handle_bound() {
        let fixture = Fixture::new();
        let ancestor = fixture.0.join("å-文件-😀");
        let parent = ancestor.join("parent");
        std::fs::create_dir_all(&parent).unwrap();
        let name = OsStr::new("草稿-ä-😀.md");
        let source = parent.join(name);
        std::fs::rename(fixture.source(), &source).unwrap();
        let session = Session::open(&source).unwrap();
        let renamed = fixture.0.join("renamed-ö");
        let actual = match std::fs::rename(&ancestor, &renamed) {
            Ok(()) => {
                eprintln!("native ancestor rename succeeded; verifying pinned parent publication");
                std::fs::create_dir_all(&parent).unwrap();
                std::fs::write(&source, b"replacement ancestor sentinel").unwrap();
                renamed.join("parent").join(name)
            }
            Err(error) => {
                eprintln!("native ancestor rename refused: {error}");
                assert!(
                    matches!(error.raw_os_error(), Some(5 | 32)),
                    "unexpected ancestor rename failure: {error}"
                );
                assert!(!renamed.exists());
                source.clone()
            }
        };
        session.save("Unicode draft: å文件😀").unwrap();
        assert_eq!(
            std::fs::read(&actual).unwrap(),
            "Unicode draft: å文件😀".as_bytes()
        );
        if actual != source {
            assert_eq!(
                std::fs::read(&source).unwrap(),
                b"replacement ancestor sentinel"
            );
        }
        let recovery = std::fs::read_dir(actual.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .unwrap();
        assert_eq!(
            std::fs::read(recovery.join("candidate")).unwrap(),
            b"original\r\n"
        );
        assert_eq!(
            std::fs::read(recovery.join("edits")).unwrap(),
            "Unicode draft: å文件😀".as_bytes()
        );
    }

    #[test]
    fn windows_native_process_exit_preserves_original_and_draft_at_every_step() {
        const CHILD: &str = "OTTRIN_TEST_TEXT_EXIT_STEP";
        if let Ok(boundary) = std::env::var(CHILD) {
            let path = PathBuf::from(std::env::var_os("OTTRIN_TEST_TEXT_EXIT_SOURCE").unwrap());
            let session = Session::open(&path).unwrap();
            let result = session.save_with_hook("draft", |step| {
                if format!("{step:?}") == boundary {
                    std::process::exit(86);
                }
                if boundary == "RestoreVacant" && step == Step::Publish {
                    return Err(TextEditError::Write("fixture requests restoration".into()));
                }
                Ok(())
            });
            panic!("process did not exit at {boundary}: {result:?}");
        }
        for boundary in [
            Step::VerifySource,
            Step::RetainSource,
            Step::VerifyRetained,
            Step::FlushRetained,
            Step::Publish,
            Step::VerifyPublished,
            Step::FlushPublished,
            Step::RestoreVacant,
        ] {
            let fixture = Fixture::new();
            let original = Session::open(&fixture.source()).unwrap().source;
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "text_edit::windows::tests::windows_native_process_exit_preserves_original_and_draft_at_every_step", "--nocapture"])
                .env(CHILD, format!("{boundary:?}"))
                .env("OTTRIN_TEST_TEXT_EXIT_SOURCE", fixture.source())
                .status().unwrap();
            assert_eq!(status.code(), Some(86), "exit at {boundary:?}");
            let recovery = fixture.recovery();
            assert_eq!(std::fs::read(recovery.join("edits")).unwrap(), b"draft");
            let before_withdrawal = matches!(boundary, Step::VerifySource | Step::RetainSource);
            let published = matches!(boundary, Step::VerifyPublished | Step::FlushPublished);
            let retained = if before_withdrawal {
                fixture.source()
            } else {
                recovery.join("candidate")
            };
            let observed = Session::open(&retained).unwrap().source;
            assert_eq!(observed.identity, original.identity);
            assert_eq!(observed.bytes, original.bytes);
            assert_eq!(observed.security, original.security);
            if published {
                assert_eq!(std::fs::read(fixture.source()).unwrap(), b"draft");
            } else if !before_withdrawal {
                assert!(!fixture.source().exists());
            }
            let publication = std::fs::read_dir(&fixture.0)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .ends_with("-publication")
                });
            if published {
                assert!(publication.is_none());
            } else {
                assert_eq!(std::fs::read(publication.unwrap()).unwrap(), b"draft");
            }
        }
    }

    #[test]
    fn windows_native_external_change_refuses_save_before_staging() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        std::fs::write(fixture.source(), b"external").unwrap();
        assert_eq!(session.save("draft"), Err(TextEditError::Conflict));
        assert_eq!(std::fs::read(fixture.source()).unwrap(), b"external");
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn windows_native_unchanged_save_creates_no_recovery_files() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        session.save(session.contents()).unwrap();
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn windows_native_failure_after_withdrawal_restores_without_removing_recovery() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        let result = session.save_with_hook("draft", |step| {
            if step == Step::Publish {
                Err(TextEditError::Write("injected".into()))
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result,
            Err(TextEditError::RecoveryConflict { .. })
        ));
        assert_eq!(std::fs::read(fixture.source()).unwrap(), b"original\r\n");
        assert_eq!(
            std::fs::read(fixture.recovery().join("candidate")).unwrap(),
            b"original\r\n"
        );
        assert_eq!(
            std::fs::read(fixture.recovery().join("edits")).unwrap(),
            b"draft"
        );
    }

    #[test]
    fn windows_native_occupied_name_is_never_replaced_by_publication_or_recovery() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        let result = session.save_with_hook("draft", |step| {
            if step == Step::Publish {
                std::fs::write(fixture.source(), b"third writer").unwrap();
            }
            Ok(())
        });
        assert!(matches!(
            result,
            Err(TextEditError::RecoveryConflict { .. })
        ));
        assert_eq!(std::fs::read(fixture.source()).unwrap(), b"third writer");
        assert_eq!(
            std::fs::read(fixture.recovery().join("candidate")).unwrap(),
            b"original\r\n"
        );
        assert_eq!(
            std::fs::read(fixture.recovery().join("edits")).unwrap(),
            b"draft"
        );
    }

    #[test]
    fn windows_native_post_publication_failure_keeps_all_versions() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        let result = session.save_with_hook("draft", |step| {
            if step == Step::FlushPublished {
                Err(TextEditError::Write("flush failed".into()))
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result,
            Err(TextEditError::RecoveryConflict { .. })
        ));
        assert_eq!(std::fs::read(fixture.source()).unwrap(), b"draft");
        assert_eq!(
            std::fs::read(fixture.recovery().join("candidate")).unwrap(),
            b"original\r\n"
        );
        assert_eq!(
            std::fs::read(fixture.recovery().join("edits")).unwrap(),
            b"draft"
        );
    }

    #[test]
    fn windows_native_closed_handle_writable_mapping_blocks_save_until_unmapped() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_WRITE, FlushViewOfFile, MEMORY_MAPPED_VIEW_ADDRESS,
            MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile,
        };
        struct Mapping {
            object: HANDLE,
            view: MEMORY_MAPPED_VIEW_ADDRESS,
        }
        impl Mapping {
            fn change(&self) {
                // The view spans the unchanged ten-byte fixture file.
                unsafe {
                    ptr::copy_nonoverlapping(
                        b"late-edit\n".as_ptr(),
                        self.view.Value.cast::<u8>(),
                        10,
                    );
                }
                assert_ne!(unsafe { FlushViewOfFile(self.view.Value, 10) }, 0);
            }
        }
        impl Drop for Mapping {
            fn drop(&mut self) {
                unsafe {
                    UnmapViewOfFile(self.view);
                    CloseHandle(self.object);
                }
            }
        }
        for mutate in [false, true] {
            let fixture = Fixture::new();
            let session = Session::open(&fixture.source()).unwrap();
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(fixture.source())
                .unwrap();
            let object = unsafe {
                CreateFileMappingW(
                    handle(&file),
                    ptr::null(),
                    PAGE_READWRITE,
                    0,
                    0,
                    ptr::null(),
                )
            };
            assert!(
                !object.is_null(),
                "create mapping: {}",
                std::io::Error::last_os_error()
            );
            let view = unsafe { MapViewOfFile(object, FILE_MAP_WRITE, 0, 0, 10) };
            let mapping = Mapping { object, view };
            assert!(
                !mapping.view.Value.is_null(),
                "map view: {}",
                std::io::Error::last_os_error()
            );
            drop(file);
            assert_eq!(session.save("draft"), Err(TextEditError::Conflict));
            assert_eq!(std::fs::read(fixture.source()).unwrap(), b"original\r\n");
            assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
            if mutate {
                mapping.change();
                assert_eq!(session.save("draft"), Err(TextEditError::Conflict));
                assert_eq!(std::fs::read(fixture.source()).unwrap(), b"late-edit\n");
                assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
            }
            drop(mapping);
            let expected = if mutate {
                b"late-edit\n"
            } else {
                b"original\r\n"
            };
            if mutate {
                assert_eq!(session.save("draft"), Err(TextEditError::Conflict));
                let reloaded = Session::open(&fixture.source()).unwrap();
                assert_eq!(reloaded.contents().as_bytes(), expected);
                reloaded
                    .save("draft")
                    .expect("save after explicit reload of mapped edits");
            } else {
                session
                    .save("draft")
                    .expect("retry after unmapping without source edits");
            }
            assert_eq!(std::fs::read(fixture.source()).unwrap(), b"draft");
            let recovery = fixture.recovery();
            assert_eq!(std::fs::read(recovery.join("candidate")).unwrap(), expected);
            assert_eq!(std::fs::read(recovery.join("edits")).unwrap(), b"draft");
        }
    }

    #[test]
    fn windows_native_open_writer_blocks_save_without_touching_source() {
        let fixture = Fixture::new();
        let session = Session::open(&fixture.source()).unwrap();
        let _writer = OpenOptions::new()
            .write(true)
            .open(fixture.source())
            .unwrap();
        assert_eq!(session.save("draft"), Err(TextEditError::Conflict));
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn windows_native_recovery_dacl_denies_restricted_reader() {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Security::Authorization::GetSecurityInfo;
        use windows_sys::Win32::Security::{
            AccessCheck, CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE,
            DuplicateToken, GENERIC_MAPPING, PRIVILEGE_SET, SID_AND_ATTRIBUTES,
            SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_QUERY, WinRestrictedCodeSid,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let fixture = Fixture::new();
        Session::open(&fixture.source())
            .unwrap()
            .save("private draft")
            .unwrap();
        let mut raw = ptr::null_mut();
        assert_ne!(
            unsafe {
                OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY, &mut raw)
            },
            0
        );
        let process = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mut sid = [0u64; 9];
        let mut size = size_of_val(&sid) as u32;
        assert_ne!(
            unsafe {
                CreateWellKnownSid(
                    WinRestrictedCodeSid,
                    ptr::null_mut(),
                    sid.as_mut_ptr().cast(),
                    &mut size,
                )
            },
            0
        );
        let restricting = SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr().cast(),
            Attributes: 0,
        };
        assert_ne!(
            unsafe {
                CreateRestrictedToken(
                    process.as_raw_handle().cast(),
                    DISABLE_MAX_PRIVILEGE,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    1,
                    &restricting,
                    &mut raw,
                )
            },
            0
        );
        let restricted = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        assert_ne!(
            unsafe {
                DuplicateToken(
                    restricted.as_raw_handle().cast(),
                    SecurityImpersonation,
                    &mut raw,
                )
            },
            0
        );
        let token = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let can_read = |descriptor| {
            let mapping = GENERIC_MAPPING {
                GenericRead: FILE_GENERIC_READ,
                GenericWrite: FILE_GENERIC_WRITE,
                GenericExecute: FILE_GENERIC_EXECUTE,
                GenericAll: FILE_ALL_ACCESS,
            };
            let mut privileges = PRIVILEGE_SET::default();
            let mut size = size_of::<PRIVILEGE_SET>() as u32;
            let mut granted = 0;
            let mut allowed = 0;
            assert_ne!(
                unsafe {
                    AccessCheck(
                        descriptor,
                        token.as_raw_handle().cast(),
                        FILE_GENERIC_READ,
                        &mapping,
                        &mut privileges,
                        &mut size,
                        &mut granted,
                        &mut allowed,
                    )
                },
                0,
                "native access check: {}",
                std::io::Error::last_os_error()
            );
            allowed != 0
        };
        // A positive control proves the restricted token can be granted read access.
        let public: Vec<u16> = "O:SYG:SYD:(A;;FR;;;WD)(A;;FR;;;RC)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    public.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        let control = PrivateSecurity(descriptor);
        assert!(can_read(control.0));
        let recovery = fixture.recovery();
        for path in [&recovery, &recovery.join("edits")] {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                .open(path)
                .unwrap();
            let mut descriptor = ptr::null_mut();
            assert_eq!(
                unsafe {
                    GetSecurityInfo(
                        handle(&file),
                        SE_FILE_OBJECT,
                        OWNER_SECURITY_INFORMATION
                            | GROUP_SECURITY_INFORMATION
                            | DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut descriptor,
                    )
                },
                0
            );
            let security = PrivateSecurity(descriptor);
            assert!(
                !can_read(security.0),
                "private recovery ACL allowed a restricted reader"
            );
        }
        assert_eq!(
            std::fs::read(recovery.join("edits")).unwrap(),
            b"private draft"
        );
    }

    #[test]
    fn windows_native_scoped_policy_refuses_security_preparation_without_mutation() {
        let fixture = Fixture::new();
        let original = Session::open(&fixture.source()).unwrap().source.security;
        let sddl: Vec<u16> = "O:SYG:SYD:(A;;FA;;;WD)S:(SP;;;;;S-1-17-1)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        let descriptor = PrivateSecurity(descriptor);
        let size =
            unsafe { windows_sys::Win32::Security::GetSecurityDescriptorLength(descriptor.0) };
        assert!(size > 0);
        let security =
            unsafe { std::slice::from_raw_parts(descriptor.0.cast::<u8>(), size as usize) };
        let file = OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC)
            .open(fixture.source())
            .unwrap();
        assert_eq!(
            apply_publication_security(&file, security),
            Err(TextEditError::UnsupportedMetadata)
        );
        drop(file);
        let unchanged = Session::open(&fixture.source()).unwrap();
        assert_eq!(unchanged.source.security, original);
        assert_eq!(unchanged.contents(), "original\r\n");
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    #[ignore = "requires an owned audit fixture seeded and inspected by a separate privileged test process"]
    fn windows_native_preseeded_audit_is_refused_before_staging() {
        let source = PathBuf::from(
            std::env::var_os("OTTRIN_TEST_AUDIT_SOURCE").expect("owned audited fixture path"),
        );
        let before = std::fs::read(&source).unwrap();
        let entries = std::fs::read_dir(source.parent().unwrap()).unwrap().count();
        assert!(matches!(
            Session::open(&source),
            Err(TextEditError::UnsupportedMetadata)
        ));
        assert_eq!(std::fs::read(&source).unwrap(), before);
        assert_eq!(
            std::fs::read_dir(source.parent().unwrap()).unwrap().count(),
            entries
        );
        // The fixture agent must independently verify the full audit SACL before
        // and after this ordinary-user check; this process cannot read audit ACEs.
    }

    #[test]
    fn windows_native_resource_attribute_is_refused_before_staging() {
        use windows_sys::Win32::Security::GetSecurityDescriptorSacl;
        for after_open in [false, true] {
            let fixture = Fixture::new();
            let session = after_open.then(|| Session::open(&fixture.source()).unwrap());
            let sddl: Vec<u16> = "S:(RA;;;;;WD;(\"OttrinFixture\",TS,0x0,\"retained\"))\0"
                .encode_utf16()
                .collect();
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        sddl.as_ptr(),
                        SDDL_REVISION_1,
                        &mut descriptor,
                        ptr::null_mut(),
                    )
                },
                0,
                "parse resource attribute fixture: {}",
                std::io::Error::last_os_error()
            );
            let descriptor = PrivateSecurity(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut sacl = ptr::null_mut();
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorSacl(descriptor.0, &mut present, &mut sacl, &mut defaulted)
                },
                0
            );
            assert_ne!(present, 0);
            assert!(!sacl.is_null());
            let file = OpenOptions::new()
                .access_mode(READ_CONTROL | WRITE_DAC)
                .open(fixture.source())
                .unwrap();
            assert_eq!(
                unsafe {
                    SetSecurityInfo(
                        handle(&file),
                        SE_FILE_OBJECT,
                        ATTRIBUTE_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        ptr::null(),
                        sacl,
                    )
                },
                0,
                "apply attribute to owned fixture"
            );
            drop(file);
            if let Some(session) = session {
                assert_eq!(
                    session.save("draft"),
                    Err(TextEditError::UnsupportedMetadata)
                );
            } else {
                assert!(matches!(
                    Session::open(&fixture.source()),
                    Err(TextEditError::UnsupportedMetadata)
                ));
            }
            assert_eq!(std::fs::read(fixture.source()).unwrap(), b"original\r\n");
            assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn windows_native_object_ids_are_refused_without_changing_source() {
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::{
            FILE_OBJECTID_BUFFER, FSCTL_CREATE_OR_GET_OBJECT_ID, FSCTL_GET_OBJECT_ID,
        };
        for after_open in [false, true] {
            let fixture = Fixture::new();
            let session = after_open.then(|| Session::open(&fixture.source()).unwrap());
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(fixture.source())
                .unwrap();
            let mut object = FILE_OBJECTID_BUFFER::default();
            let mut returned = 0;
            assert_eq!(
                unsafe {
                    DeviceIoControl(
                        handle(&file),
                        FSCTL_GET_OBJECT_ID,
                        ptr::null(),
                        0,
                        ptr::from_mut(&mut object).cast(),
                        size_of::<FILE_OBJECTID_BUFFER>() as u32,
                        &mut returned,
                        ptr::null_mut(),
                    )
                },
                0
            );
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(2));
            assert_ne!(
                unsafe {
                    DeviceIoControl(
                        handle(&file),
                        FSCTL_CREATE_OR_GET_OBJECT_ID,
                        ptr::null(),
                        0,
                        ptr::from_mut(&mut object).cast(),
                        size_of::<FILE_OBJECTID_BUFFER>() as u32,
                        &mut returned,
                        ptr::null_mut(),
                    )
                },
                0,
                "create owned fixture object ID: {}",
                std::io::Error::last_os_error()
            );
            let id = object.ObjectId;
            drop(file);
            if let Some(session) = session {
                assert_eq!(
                    session.save("draft"),
                    Err(TextEditError::UnsupportedMetadata)
                );
            } else {
                assert!(
                    matches!(
                        Session::open(&fixture.source()),
                        Err(TextEditError::UnsupportedMetadata)
                    ),
                    "source object ID must be refused"
                );
            }
            assert_eq!(std::fs::read(fixture.source()).unwrap(), b"original\r\n");
            assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
            let file = File::open(fixture.source()).unwrap();
            assert_ne!(
                unsafe {
                    DeviceIoControl(
                        handle(&file),
                        FSCTL_GET_OBJECT_ID,
                        ptr::null(),
                        0,
                        ptr::from_mut(&mut object).cast(),
                        size_of::<FILE_OBJECTID_BUFFER>() as u32,
                        &mut returned,
                        ptr::null_mut(),
                    )
                },
                0
            );
            assert_eq!(object.ObjectId, id);
        }
    }

    #[test]
    fn windows_native_alternate_streams_and_hard_links_are_rejected() {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("source.md:extra"), b"retained metadata").unwrap();
        assert!(matches!(
            Session::open(&fixture.source()),
            Err(TextEditError::UnsupportedMetadata)
        ));
        let plain = fixture.0.join("plain.md");
        std::fs::write(&plain, b"text").unwrap();
        std::fs::hard_link(&plain, fixture.0.join("linked.md")).unwrap();
        assert!(matches!(
            Session::open(&plain),
            Err(TextEditError::HardLinked)
        ));
    }

    #[test]
    fn windows_native_invalid_text_and_stream_paths_are_rejected() {
        let fixture = Fixture::new();
        assert!(matches!(
            Session::open(&fixture.0.join("source.md:extra")),
            Err(TextEditError::NotRegularFile)
        ));
        for (bytes, expected) in [
            (vec![0xff], TextEditError::InvalidUtf8),
            (vec![0], TextEditError::BinaryText),
            (
                vec![b'a'; MAX_EDITABLE_TEXT_BYTES as usize + 1],
                TextEditError::TooLarge {
                    limit: MAX_EDITABLE_TEXT_BYTES,
                },
            ),
        ] {
            std::fs::write(fixture.source(), bytes).unwrap();
            assert_eq!(Session::open(&fixture.source()).unwrap_err(), expected);
        }
    }
}
