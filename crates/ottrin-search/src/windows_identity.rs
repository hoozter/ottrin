//! Process-token identity used to authenticate the Windows index daemon transport.
use std::ffi::OsString;
use std::io;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::PathBuf;
use std::ptr;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, HANDLE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, WaitForSingleObject,
};

/// Per-user endpoint identity and peer validation for the Windows index daemon.
/// This authenticates processes, not executable provenance or a connected pipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexDaemonIdentity {
    sid: String,
    #[cfg(test)]
    test_endpoint: Option<u64>,
}

impl IndexDaemonIdentity {
    pub fn for_current_user() -> Result<Self, String> {
        Ok(Self {
            sid: process_user_sid(std::process::id())?,
            #[cfg(test)]
            test_endpoint: None,
        })
    }

    // Independent fixtures must not compete for the real per-user endpoint or
    // connect to each other. Peer authentication still uses the actual token SID.
    #[cfg(test)]
    pub(crate) fn isolated_for_test() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            test_endpoint: Some(NEXT.fetch_add(1, Ordering::Relaxed)),
            ..Self::for_current_user().expect("current test process identity")
        }
    }

    pub fn user_sid(&self) -> &str {
        &self.sid
    }

    pub fn pipe_name(&self) -> String {
        #[cfg(test)]
        if let Some(endpoint) = self.test_endpoint {
            return format!(
                r"\\.\pipe\Ottrin.Index.{}.test-{}-{endpoint}",
                self.sid,
                std::process::id()
            );
        }
        format!(r"\\.\pipe\Ottrin.Index.{}", self.sid)
    }

    /// Reject foreign processes and failures to inspect a process token.
    /// The caller must obtain this PID from the connected pipe's kernel metadata.
    pub fn verify_process(&self, pid: u32) -> Result<(), String> {
        if process_user_sid(pid)? != self.sid {
            return Err("The index daemon peer belongs to another Windows user".to_string());
        }
        Ok(())
    }

    /// Capture and authenticate the process object, avoiding PID reuse while waiting.
    pub(crate) fn open_process(&self, pid: u32) -> Result<IndexPeerProcess, String> {
        let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?;
        if process_handle_user_sid(process.as_raw_handle())? != self.sid {
            return Err("The index daemon peer belongs to another Windows user".to_string());
        }
        Ok(IndexPeerProcess(process))
    }
}

pub(crate) struct IndexPeerProcess(OwnedHandle);

impl IndexPeerProcess {
    pub(crate) fn executable(&self) -> Result<PathBuf, String> {
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        // SAFETY: process handle stays live; buffer and length are writable and bounded.
        if unsafe {
            QueryFullProcessImageNameW(self.0.as_raw_handle(), 0, buffer.as_mut_ptr(), &mut length)
        } == 0
        {
            return Err(format!(
                "Cannot inspect index daemon executable: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    }

    pub(crate) fn wait_for_exit(&self, timeout: Duration) -> Result<(), String> {
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: owned process handle has SYNCHRONIZE rights and remains live through the wait.
        match unsafe { WaitForSingleObject(self.0.as_raw_handle(), millis) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(
                "Ottrin index daemon did not finish preparing for upgrade within 30 seconds"
                    .to_string(),
            ),
            _ => Err(format!(
                "Cannot wait for index daemon exit: {}",
                io::Error::last_os_error()
            )),
        }
    }
}

fn open_process(pid: u32, access: u32) -> Result<OwnedHandle, String> {
    // SAFETY: no pointers; requesting query/synchronization access and a non-inheritable handle.
    let process = unsafe { OpenProcess(access, 0, pid) };
    if process.is_null() {
        return Err(format!(
            "Cannot inspect index daemon process {pid}: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: successful OpenProcess transfers ownership; OwnedHandle closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(process) })
}

fn process_user_sid(pid: u32) -> Result<String, String> {
    let process = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    process_handle_user_sid(process.as_raw_handle())
}

fn process_handle_user_sid(process: HANDLE) -> Result<String, String> {
    let mut token = ptr::null_mut();
    // SAFETY: process stays live and token points to writable handle storage.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(format!(
            "Cannot inspect index daemon process token: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: successful OpenProcessToken transfers ownership of its handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    token_user_sid(token.as_raw_handle())
}

fn token_user_sid(token: HANDLE) -> Result<String, String> {
    let mut required = 0;
    // SAFETY: size query has no output buffer; required points to writable storage.
    let query = unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut required) };
    if query != 0
        || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || required < mem::size_of::<TOKEN_USER>() as u32
        || required > 65536
    {
        return Err("Cannot determine index daemon token-user buffer size".to_string());
    }
    // u64 storage provides the alignment required by TOKEN_USER on supported Windows targets.
    let mut buffer = vec![0_u64; (required as usize).div_ceil(mem::size_of::<u64>())];
    // SAFETY: buffer has at least required bytes and TOKEN_USER alignment; token stays live.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(format!(
            "Cannot read index daemon process user: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: successful TokenUser query initialized the structure and its SID inside buffer.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    // SAFETY: SID remains live in buffer; API allocates the NUL-terminated result.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(format!(
            "Cannot format index daemon user identity: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: successful API returns a NUL-terminated UTF-16 string, valid until LocalFree.
    let result = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let result = String::from_utf16(std::slice::from_raw_parts(text, length));
        LocalFree(text.cast());
        result
    };
    result.map_err(|error| format!("Invalid index daemon user identity: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};

    #[test]
    fn windows_daemon_identity_accepts_current_process_and_scopes_its_endpoint() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        assert!(identity.user_sid().starts_with("S-1-"));
        assert_eq!(
            identity.pipe_name(),
            format!(r"\\.\pipe\Ottrin.Index.{}", identity.user_sid())
        );
        identity.verify_process(std::process::id()).unwrap();
    }

    #[test]
    fn windows_daemon_identity_rejects_missing_and_foreign_processes() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        assert!(identity.verify_process(0).is_err());
        let foreign = IndexDaemonIdentity {
            sid: "S-1-0-0".to_string(),
            ..identity.clone()
        };
        assert!(
            foreign
                .verify_process(std::process::id())
                .unwrap_err()
                .contains("another Windows user")
        );
    }

    #[test]
    fn windows_daemon_identity_releases_process_and_token_handles() {
        fn handles() -> u32 {
            let mut count = 0;
            // SAFETY: valid process pseudo-handle and writable output counter.
            assert_ne!(
                unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
                0
            );
            count
        }
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let before = handles();
        for _ in 0..100 {
            identity.verify_process(std::process::id()).unwrap();
        }
        assert_eq!(handles(), before);
    }

    #[test]
    #[ignore = "requires an explicitly owned native comparison process"]
    fn windows_daemon_identity_rejects_real_foreign_user_process() {
        let pid = std::env::var("OTTRIN_QA_FOREIGN_PID")
            .unwrap()
            .parse()
            .unwrap();
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        assert!(
            identity
                .verify_process(pid)
                .unwrap_err()
                .contains("another Windows user")
        );
    }
}
