//! Native single-entry operations shared by Windows file transactions.
use std::ffi::{OsStr, OsString, c_void};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::{
    ffi::{OsStrExt, OsStringExt},
    io::{AsRawHandle, FromRawHandle},
};
use std::ptr;
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::*;
use windows_sys::Win32::Foundation::{LocalFree, RtlNtStatusToDosError, UNICODE_STRING};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_DESCRIPTOR;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;

pub(crate) fn valid_component(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide = name.encode_wide().collect::<Vec<_>>();
    if wide.is_empty()
        || wide.len() > 255
        || wide.iter().any(|c| [0, 58, 47, 92].contains(c))
        || name == "."
        || name == ".."
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one native entry name",
        ));
    }
    Ok(wide)
}

pub(crate) struct RelativeOptions {
    pub directory: Option<bool>,
    pub create_new: bool,
    pub access: u32,
    pub sharing: u32,
    pub security: *const SECURITY_DESCRIPTOR,
}

/// Open exactly one leaf under an already opened directory. Creation never
/// replaces an entry; opening a reparse entry never follows its target.
///
/// # Safety
/// A non-null security pointer must reference a valid Windows security descriptor
/// throughout the call. The descriptor is borrowed, never retained or freed.
pub(crate) unsafe fn open_relative(
    parent: &File,
    name: &OsStr,
    options: RelativeOptions,
) -> io::Result<File> {
    let mut wide = valid_component(name)?;
    let unicode = UNICODE_STRING {
        Length: (wide.len() * 2) as u16,
        MaximumLength: (wide.len() * 2) as u16,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &unicode,
        SecurityDescriptor: options.security,
        ..Default::default()
    };
    let mut raw = ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            options.access,
            &attributes,
            &mut status,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            options.sharing,
            if options.create_new {
                FILE_CREATE
            } else {
                FILE_OPEN
            },
            FILE_OPEN_REPARSE_POINT
                | FILE_SYNCHRONOUS_IO_NONALERT
                | match options.directory {
                    Some(true) => FILE_DIRECTORY_FILE,
                    Some(false) => FILE_NON_DIRECTORY_FILE,
                    None => 0,
                },
            ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    Ok(unsafe { File::from_raw_handle(raw.cast()) })
}

/// An owned descriptor allocated by Windows, including test-created descriptors.
pub(crate) struct LocalSecurity(*mut c_void);
impl LocalSecurity {
    pub(crate) fn private() -> io::Result<Self> {
        Self::from_sddl("D:P(A;OICI;FA;;;OW)")
    }
    pub(crate) fn from_sddl(text: &str) -> io::Result<Self> {
        let wide = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut raw = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut raw,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(raw))
    }
    /// # Safety
    /// `raw` must be a live, uniquely owned LocalAlloc allocation. Ownership is transferred.
    #[cfg(test)]
    pub(crate) unsafe fn from_local_alloc(raw: *mut c_void) -> Self {
        Self(raw)
    }
    pub(crate) fn as_raw(&self) -> *mut c_void {
        self.0
    }
}
impl Drop for LocalSecurity {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// A handle opened with the access mode required for per-object ACL updates.
/// SetSecurityInfo documents MAXIMUM_ALLOWED as suppressing child propagation.
/// Keep this contract through duplication; ordinary File handles cannot be
/// promoted to this type. Ownership/binding checks remain the transaction's job.
#[derive(Debug)]
pub(crate) struct PerObjectFile(File);
impl PerObjectFile {
    pub(crate) fn create_private(
        parent: &File,
        name: &OsStr,
        directory: bool,
        sharing: u32,
    ) -> io::Result<Self> {
        create_private_with_access(
            parent,
            name,
            directory,
            sharing,
            windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED | SYNCHRONIZE,
        )
        .map(Self)
    }
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }
    #[cfg(test)]
    pub(crate) fn readonly_test_handle(file: File) -> Self {
        // Deliberately violate the access contract only in kernel-denial tests.
        Self(file)
    }
    #[cfg(test)]
    pub(crate) fn open(parent: &File, name: &OsStr) -> io::Result<Self> {
        // Fixture/recovery-mode experiment only; production recovery is readonly.
        unsafe {
            open_relative(
                parent,
                name,
                RelativeOptions {
                    directory: None,
                    create_new: false,
                    access: windows_sys::Win32::System::SystemServices::MAXIMUM_ALLOWED
                        | SYNCHRONIZE,
                    sharing: FILE_SHARE_READ,
                    security: ptr::null(),
                },
            )
        }
        .map(Self)
    }
}
impl std::ops::Deref for PerObjectFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}

/// Create with the private DACL in the same native operation that creates the
/// entry. There is no inherited-permission interval and no path-based repair.
pub(crate) fn create_private(
    parent: &File,
    name: &OsStr,
    directory: bool,
    sharing: u32,
) -> io::Result<File> {
    create_private_with_access(
        parent,
        name,
        directory,
        sharing,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC,
    )
}

fn create_private_with_access(
    parent: &File,
    name: &OsStr,
    directory: bool,
    sharing: u32,
    access: u32,
) -> io::Result<File> {
    let identity =
        crate::move_recovery::stable_identity_from_handle(parent).map_err(io::Error::other)?;
    if !identity.is_directory_without_reparse() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private parent is not a non-reparse directory",
        ));
    }
    let mut flags = 0;
    if unsafe {
        GetVolumeInformationByHandleW(
            parent.as_raw_handle().cast(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut flags,
            ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "private staging requires filesystem ACL support",
        ));
    }
    let security = LocalSecurity::private()?;
    // The owned descriptor remains alive until NtCreateFile returns.
    unsafe {
        open_relative(
            parent,
            name,
            RelativeOptions {
                directory: Some(directory),
                create_new: true,
                access,
                // Sharing is part of the caller's operation contract. In
                // particular, Windows rename into a directory needs write sharing.
                sharing,
                security: security.as_raw().cast(),
            },
        )
    }
}

/// Diagnostic/recovery location of this actual object, never namespace authority.
pub(crate) fn location(file: &File) -> io::Result<std::path::PathBuf> {
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle().cast(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED,
        )
    };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize >= buffer.len() {
        return Err(io::Error::other(
            "native final path exceeds the path buffer",
        ));
    }
    Ok(OsString::from_wide(&buffer[..length as usize]).into())
}

// Validate every offset before reading the variable-length part of a record.
pub(crate) fn record_names(
    buffer: &[u64],
    name_offset: usize,
    length_offset: usize,
) -> io::Result<Vec<std::ffi::OsString>> {
    let bytes = unsafe {
        std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), std::mem::size_of_val(buffer))
    };
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid native enumeration records",
        )
    };
    let mut names = Vec::new();
    let mut offset = 0;
    loop {
        let record = bytes.get(offset..).ok_or_else(invalid)?;
        let read_u32 = |at: usize| -> io::Result<usize> {
            let value = record.get(at..at + 4).ok_or_else(invalid)?;
            Ok(u32::from_le_bytes(value.try_into().map_err(|_| invalid())?) as usize)
        };
        let next = read_u32(0)?;
        let length = read_u32(length_offset)?;
        let end = name_offset.checked_add(length).ok_or_else(invalid)?;
        if length == 0 || length % 2 != 0 || (next != 0 && (next < end || next % 8 != 0)) {
            return Err(invalid());
        }
        let name = record.get(name_offset..end).ok_or_else(invalid)?;
        let units = name
            .chunks_exact(2)
            .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
            .collect::<Vec<_>>();
        names.push(std::ffi::OsString::from_wide(&units));
        if next == 0 {
            return Ok(names);
        }
        offset = offset.checked_add(next).ok_or_else(invalid)?;
    }
}

pub(crate) fn directory_children(handle: &std::fs::File) -> io::Result<Vec<std::ffi::OsString>> {
    directory_children_limited(handle, usize::MAX)
}

pub(crate) fn directory_children_limited(handle: &File, limit: usize) -> io::Result<Vec<OsString>> {
    let mut buffer = vec![0_u64; 8192];
    let mut names = Vec::new();
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle().cast(),
                class,
                buffer.as_mut_ptr().cast(),
                std::mem::size_of_val(buffer.as_slice()) as u32,
            )
        };
        if succeeded == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error()
                == Some(windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES as i32)
            {
                names.sort_unstable_by(|left: &std::ffi::OsString, right| {
                    left.encode_wide().cmp(right.encode_wide())
                });
                if names.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "duplicate native child name",
                    ));
                }
                return Ok(names);
            }
            return Err(error);
        }
        for name in record_names(
            &buffer,
            std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName),
            std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength),
        )? {
            if name != "." && name != ".." {
                valid_component(&name)?;
                if names.len() >= limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "native directory exceeds entry budget",
                    ));
                }
                names.push(name);
            }
        }
        class = FileIdBothDirectoryInfo;
    }
}
