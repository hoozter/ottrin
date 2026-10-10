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
    ATTRIBUTE_SECURITY_INFORMATION, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
    GetKernelObjectSecurity, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
    LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

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

const PRESERVED_SECURITY_INFORMATION: u32 = OWNER_SECURITY_INFORMATION
    | GROUP_SECURITY_INFORMATION
    | DACL_SECURITY_INFORMATION
    | LABEL_SECURITY_INFORMATION
    | ATTRIBUTE_SECURITY_INFORMATION;

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
        apply_publication_dacl(&publication, &self.source.security).map_err(retained)?;
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
// inheritance state of a supplied descriptor. Finalize the new file's DACL
// through the filesystem API before the exact metadata comparison.
fn apply_publication_dacl(file: &File, security: &[u8]) -> Result<(), TextEditError> {
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
            PRESERVED_SECURITY_INFORMATION,
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
            PRESERVED_SECURITY_INFORMATION,
            security.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last_error());
    }
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

    #[test]
    fn windows_security_snapshot_includes_resource_attributes() {
        assert_ne!(
            PRESERVED_SECURITY_INFORMATION & ATTRIBUTE_SECURITY_INFORMATION,
            0
        );
    }

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
