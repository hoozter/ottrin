use super::*;
use std::fs::File;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, FreeSid, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
    LABEL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_CAPABILITIES,
};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

pub(super) struct Sandbox {
    name: Vec<u16>,
    sid: PSID,
    finished: bool,
    pub capabilities: SECURITY_CAPABILITIES,
    pub input_path: Option<PathBuf>,
    input_guard: Option<File>,
}

impl Sandbox {
    pub fn new(request: &IsolatedProcessRequest) -> Result<Self, IsolatedProcessFailure> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| IsolatedProcessFailure::Unavailable)?
            .as_nanos();
        let moniker = format!("ottrin.office.{}.{nonce}", std::process::id());
        let name: Vec<u16> = moniker.encode_utf16().chain(Some(0)).collect();
        let mut sid = null_mut();
        // SAFETY: the name buffers and SID output remain live for the call;
        // no capabilities are granted, including network capabilities.
        let hr = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        if hr < 0 {
            eprintln!("Office AppContainer creation failed: {hr:#x}");
            return Err(IsolatedProcessFailure::Unavailable);
        }
        let mut sandbox = Self {
            name,
            sid,
            finished: false,
            capabilities: SECURITY_CAPABILITIES {
                AppContainerSid: sid,
                ..Default::default()
            },
            input_path: None,
            input_guard: None,
        };
        let sid_text = sid_string(sid)?;
        private_access(&request.private_profile_dir, &sid_text, "FA", true)?;
        private_access(&request.private_output_dir, &sid_text, "FA", true)?;
        // Windows redirects LOCALAPPDATA, TEMP and TMP under this package name,
        // even with an explicit environment. Create their private destination
        // before launch; the real user's registered package folder is not used.
        std::fs::create_dir_all(
            request
                .private_profile_dir
                .join("Packages")
                .join(&moniker)
                .join("AC")
                .join("Temp"),
        )
        .map_err(|_| IsolatedProcessFailure::Unavailable)?;
        if let Some(input) = &request.input {
            let directory = request.private_profile_dir.join("input");
            std::fs::create_dir(&directory).map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
            let name = input
                .host_path
                .file_name()
                .ok_or(IsolatedProcessFailure::InvalidRequest)?;
            let path = directory.join(name);
            let mut source =
                File::open(&input.host_path).map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
            let mut target = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
            std::io::copy(&mut source, &mut target)
                .map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
            drop(target);
            private_access(&directory, &sid_text, "FRFX", true)?;
            private_access(&path, &sid_text, "FR", false)?;
            // Deny write/delete sharing for the admitted snapshot through process exit.
            sandbox.input_guard = Some(
                std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ)
                    .open(&path)
                    .map_err(|_| IsolatedProcessFailure::InvalidRequest)?,
            );
            sandbox.input_path = Some(path);
        }
        Ok(sandbox)
    }

    pub fn finish(mut self) -> Result<(), IsolatedProcessFailure> {
        self.input_guard.take();
        self.finished = true;
        // SAFETY: this unique profile was created by this instance and no child
        // remains after the process runner has returned and closed its Job Object.
        let hr = unsafe { DeleteAppContainerProfile(self.name.as_ptr()) };
        if hr < 0 {
            eprintln!("Office AppContainer cleanup failed: {hr:#x}");
            return Err(IsolatedProcessFailure::Unavailable);
        }
        Ok(())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if !self.finished {
            // SAFETY: constructor rollback owns exactly this newly created profile.
            let hr = unsafe { DeleteAppContainerProfile(self.name.as_ptr()) };
            if hr < 0 {
                eprintln!("Office AppContainer rollback failed: {hr:#x}");
            }
        }
        // SAFETY: CreateAppContainerProfile allocated this SID; this is its sole owner.
        unsafe {
            FreeSid(self.sid);
        }
    }
}

fn sid_string(sid: PSID) -> Result<String, IsolatedProcessFailure> {
    let mut text = null_mut();
    // SAFETY: sid is live and the API allocates the NUL-terminated result.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(IsolatedProcessFailure::Unavailable);
    }
    let mut length = 0;
    // SAFETY: the API returned a NUL-terminated buffer owned until LocalFree.
    let value = unsafe {
        while *text.add(length) != 0 {
            length += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        value
    };
    Ok(value)
}

fn private_access(
    path: &Path,
    sid: &str,
    rights: &str,
    directory: bool,
) -> Result<(), IsolatedProcessFailure> {
    let inheritance = if directory { "OICI" } else { "" };
    let descriptor: Vec<u16> = format!("D:P(A;{inheritance};FA;;;OW)(A;{inheritance};FA;;;SY)(A;{inheritance};{rights};;;{sid})S:(ML;{inheritance};NW;;;LW)")
        .encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut sd = null_mut();
    // SAFETY: descriptor and output pointers are valid; revision 1 is SDDL_REVISION_1.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            1,
            &mut sd,
            null_mut(),
        )
    } == 0
    {
        return Err(IsolatedProcessFailure::Unavailable);
    }
    let mut dacl = null_mut();
    let mut sacl = null_mut();
    let mut present = 0;
    let mut defaulted = 0;
    // SAFETY: ACL pointers borrow the returned descriptor until LocalFree.
    let status = unsafe {
        if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0
            || GetSecurityDescriptorSacl(sd, &mut present, &mut sacl, &mut defaulted) == 0
        {
            87
        } else {
            let dacl_status = SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            );
            if dacl_status != 0 {
                dacl_status
            } else {
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    LABEL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null(),
                    sacl,
                )
            }
        }
    };
    // SAFETY: this is the allocation returned by the descriptor conversion.
    unsafe {
        LocalFree(sd);
    }
    if status != 0 {
        eprintln!("Office private access policy failed: {status}");
        return Err(IsolatedProcessFailure::Unavailable);
    }
    Ok(())
}
