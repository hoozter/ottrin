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
    pub directory: bool,
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
                | if options.directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
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

/// Create with the private DACL in the same native operation that creates the
/// entry. There is no inherited-permission interval and no path-based repair.
pub(crate) fn create_private(
    parent: &File,
    name: &OsStr,
    directory: bool,
    sharing: u32,
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
                directory,
                create_new: true,
                access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC,
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
