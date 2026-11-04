//! Local named-pipe I/O shared by the index and administrator helper.
//! Callers supply their endpoint ACL and authenticate kernel-reported peer PIDs.
use std::io::{self, Read, Write};
use std::mem;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

pub const MAX_INSTANCES: u32 = 64;
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Keeps the next listening instance open while accepted connections are served.
pub struct LocalPipeListener {
    name: String,
    sddl: String,
    next: OwnedHandle,
}

impl LocalPipeListener {
    pub fn bind(name: &str, sddl: &str) -> io::Result<Self> {
        validate_name(name)?;
        if sddl.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid local pipe endpoint",
            ));
        }
        let next = create_instance(name, sddl, true)?;
        Ok(Self {
            name: name.into(),
            sddl: sddl.into(),
            next,
        })
    }

    /// A timeout cancels and drains the pending connect before returning None.
    pub fn accept(
        &mut self,
        timeout: Duration,
        verify: impl FnOnce(u32) -> io::Result<()>,
    ) -> io::Result<Option<LocalPipeStream>> {
        let operation = PendingIo::new()?;
        // SAFETY: handle and OVERLAPPED storage remain live through completion/cancellation.
        let started = unsafe { ConnectNamedPipe(self.next.as_raw_handle(), operation.pointer()) };
        let error = io::Error::last_os_error();
        if started == 0 && error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            if error.raw_os_error() == Some(ERROR_NO_DATA as i32) {
                // Disconnect leaves an instance unavailable until ConnectNamedPipe runs.
                // A fresh instance is immediately connectable; keep the old one alive
                // through creation so the namespace cannot be claimed in between.
                self.next = create_instance(&self.name, &self.sddl, false)?;
                return Ok(None);
            }
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                return Err(error);
            }
            match operation.wait(self.next.as_raw_handle(), timeout) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::TimedOut => return Ok(None),
                Err(error) => return Err(error),
            }
        }
        let ready = (|| {
            let pid = pipe_peer_pid(self.next.as_raw_handle(), true)?;
            verify(pid)?;
            // Preserve namespace ownership before transferring the connected instance.
            Ok::<_, io::Error>((pid, create_instance(&self.name, &self.sddl, false)?))
        })();
        let (pid, replacement) = match ready {
            Ok(ready) => ready,
            Err(error) => {
                // Close rejected peers and leave a fresh, immediately connectable instance.
                match create_instance(&self.name, &self.sddl, false) {
                    Ok(next) => self.next = next,
                    Err(reset) => {
                        // SAFETY: connect is complete; free the saturated connection slot.
                        if unsafe { DisconnectNamedPipe(self.next.as_raw_handle()) } == 0 {
                            return Err(io::Error::other(format!(
                                "{error}; listener reset failed: {reset}; disconnect failed: {}",
                                io::Error::last_os_error()
                            )));
                        }
                        return Err(io::Error::other(format!(
                            "{error}; listener reset failed: {reset}"
                        )));
                    }
                }
                return Err(error);
            }
        };
        let handle = mem::replace(&mut self.next, replacement);
        Ok(Some(LocalPipeStream {
            handle,
            timeout: DEFAULT_IO_TIMEOUT,
            peer_pid: pid,
        }))
    }
}

/// Byte-mode duplex stream. Each I/O has a deadline and cancellation is drained.
pub struct LocalPipeStream {
    handle: OwnedHandle,
    timeout: Duration,
    peer_pid: u32,
}

impl LocalPipeStream {
    pub fn connect(
        name: &str,
        timeout: Duration,
        verify: impl FnOnce(u32) -> io::Result<()>,
    ) -> io::Result<Self> {
        let handle = connect_raw(name, timeout)?;
        let pid = pipe_peer_pid(handle.as_raw_handle(), false)?;
        verify(pid)?;
        Ok(Self {
            handle,
            timeout: DEFAULT_IO_TIMEOUT,
            peer_pid: pid,
        })
    }
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            handle: self.handle.try_clone()?,
            timeout: self.timeout,
            peer_pid: self.peer_pid,
        })
    }

    pub fn cancel_pending_io(&self) -> io::Result<()> {
        // SAFETY: the duplicated owned handle stays live; null selects pending
        // operations on this connection. Each operation drains cancellation.
        if unsafe { CancelIoEx(self.handle.as_raw_handle(), ptr::null()) } != 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub fn set_io_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    pub fn peer_pid(&self) -> u32 {
        self.peer_pid
    }
}

impl Read for LocalPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let operation = PendingIo::new()?;
        let count = buffer.len().min(u32::MAX as usize) as u32;
        // SAFETY: mutable buffer and OVERLAPPED remain live until operation is drained.
        let started = unsafe {
            ReadFile(
                self.handle.as_raw_handle(),
                buffer.as_mut_ptr(),
                count,
                ptr::null_mut(),
                operation.pointer(),
            )
        };
        match finish_io(
            started,
            &operation,
            self.handle.as_raw_handle(),
            self.timeout,
        ) {
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
            result => result.map(|count| count as usize),
        }
    }
}
impl Write for LocalPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let operation = PendingIo::new()?;
        // SAFETY: immutable buffer and OVERLAPPED remain live until operation is drained.
        let started = unsafe {
            WriteFile(
                self.handle.as_raw_handle(),
                buffer.as_ptr(),
                buffer.len().min(u32::MAX as usize) as u32,
                ptr::null_mut(),
                operation.pointer(),
            )
        };
        finish_io(
            started,
            &operation,
            self.handle.as_raw_handle(),
            self.timeout,
        )
        .map(|count| count as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        // Every write has completed. FlushFileBuffers would wait for peer reads without a deadline.
        Ok(())
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: only successful, transferred kernel handles enter this function.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}
fn milliseconds(timeout: Duration) -> u32 {
    timeout.as_millis().min((u32::MAX - 1) as u128) as u32
}

struct SecurityDescriptor(*mut std::ffi::c_void);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: descriptor came from the Windows SDDL allocator and is freed once.
        unsafe { LocalFree(self.0) };
    }
}
fn create_instance(name: &str, sddl: &str, first: bool) -> io::Result<OwnedHandle> {
    let sddl = wide(sddl);
    let mut descriptor = ptr::null_mut();
    // SAFETY: NUL-terminated SDDL and writable allocation pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let descriptor = SecurityDescriptor(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name = wide(name);
    let flags = PIPE_ACCESS_DUPLEX
        | FILE_FLAG_OVERLAPPED
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    // SAFETY: name/security remain live for creation; kernel copies the descriptor.
    owned(unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_INSTANCES,
            65536,
            65536,
            0,
            &attributes,
        )
    })
}
fn validate_name(name: &str) -> io::Result<()> {
    if !name.starts_with(r"\\.\pipe\") || name.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid local pipe endpoint",
        ));
    }
    Ok(())
}

pub fn connect_raw(name: &str, timeout: Duration) -> io::Result<OwnedHandle> {
    validate_name(name)?;
    let name = wide(name);
    let deadline = Instant::now() + timeout;
    loop {
        // SAFETY: name remains live; returned handle is non-inheritable and overlapped.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return owned(handle);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
            return Err(error);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Local pipe connection timed out",
            ));
        }
        // SAFETY: name remains live; bounded wait for a server instance.
        if unsafe { WaitNamedPipeW(name.as_ptr(), milliseconds(remaining).max(1)) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Local pipe connection timed out",
                ));
            }
            return Err(error);
        }
    }
}
fn pipe_peer_pid(handle: HANDLE, server: bool) -> io::Result<u32> {
    let mut pid = 0;
    // SAFETY: connected pipe handle and writable PID output; kernel supplies the identity.
    let result = unsafe {
        if server {
            GetNamedPipeClientProcessId(handle, &mut pid)
        } else {
            GetNamedPipeServerProcessId(handle, &mut pid)
        }
    };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

struct PendingIo {
    overlapped: std::cell::UnsafeCell<OVERLAPPED>,
    _event: OwnedHandle,
}
impl PendingIo {
    fn new() -> io::Result<Self> {
        // SAFETY: default security, manual reset, initially unsignaled, unnamed event.
        let event = owned(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?;
        // SAFETY: all-zero OVERLAPPED is valid prior to assigning its event.
        let mut overlapped: OVERLAPPED = unsafe { mem::zeroed() };
        overlapped.hEvent = event.as_raw_handle();
        Ok(Self {
            overlapped: std::cell::UnsafeCell::new(overlapped),
            _event: event,
        })
    }
    fn pointer(&self) -> *mut OVERLAPPED {
        self.overlapped.get()
    }
    fn wait(&self, handle: HANDLE, timeout: Duration) -> io::Result<u32> {
        // SAFETY: event and OVERLAPPED remain live; no concurrent use by our code.
        let wait =
            unsafe { WaitForSingleObject(self._event.as_raw_handle(), milliseconds(timeout)) };
        if wait == WAIT_OBJECT_0 {
            return self.result(handle, false);
        }
        let error = if wait == WAIT_TIMEOUT {
            io::Error::new(io::ErrorKind::TimedOut, "Local pipe I/O timed out")
        } else {
            io::Error::last_os_error()
        };
        // SAFETY: cancel exactly this operation, then observe completion before releasing storage.
        unsafe { CancelIoEx(handle, self.pointer()) };
        let completed = self.result(handle, true);
        if wait == WAIT_TIMEOUT && completed.is_ok() {
            return completed;
        }
        Err(error)
    }
    fn result(&self, handle: HANDLE, wait: bool) -> io::Result<u32> {
        let mut count = 0;
        // SAFETY: operation storage stays live, count is writable; wait drains cancellation.
        if unsafe { GetOverlappedResult(handle, self.pointer(), &mut count, i32::from(wait)) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(count)
    }
}
fn finish_io(
    started: i32,
    operation: &PendingIo,
    handle: HANDLE,
    timeout: Duration,
) -> io::Result<u32> {
    if started != 0 {
        return operation.result(handle, false);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
        return Err(error);
    }
    operation.wait(handle, timeout)
}
