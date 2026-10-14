//! Local, per-user authenticated named-pipe transport for index daemon frames.
use crate::IndexDaemonIdentity;
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

pub(super) const MAX_INSTANCES: u32 = 64;
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Keeps the next listening instance open while accepted connections are served.
pub struct WindowsIndexPipeListener {
    identity: IndexDaemonIdentity,
    next: OwnedHandle,
}

impl WindowsIndexPipeListener {
    pub fn bind(identity: IndexDaemonIdentity) -> io::Result<Self> {
        let next = create_instance(&identity, true)?;
        Ok(Self { identity, next })
    }

    /// A timeout cancels and drains the pending connect before returning None.
    pub fn accept(&mut self, timeout: Duration) -> io::Result<Option<WindowsIndexPipeStream>> {
        let operation = PendingIo::new()?;
        // SAFETY: handle and OVERLAPPED storage remain live through completion/cancellation.
        let started = unsafe { ConnectNamedPipe(self.next.as_raw_handle(), operation.pointer()) };
        let error = io::Error::last_os_error();
        if started == 0 && error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            if error.raw_os_error() == Some(ERROR_NO_DATA as i32) {
                // Disconnect leaves an instance unavailable until ConnectNamedPipe runs.
                // A fresh instance is immediately connectable; keep the old one alive
                // through creation so the namespace cannot be claimed in between.
                self.next = create_instance(&self.identity, false)?;
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
            self.identity
                .verify_process(pid)
                .map_err(io::Error::other)?;
            // Preserve namespace ownership before transferring the connected instance.
            Ok::<_, io::Error>((pid, create_instance(&self.identity, false)?))
        })();
        let (pid, replacement) = match ready {
            Ok(ready) => ready,
            Err(error) => {
                // Close rejected peers and leave a fresh, immediately connectable instance.
                match create_instance(&self.identity, false) {
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
        Ok(Some(WindowsIndexPipeStream {
            handle,
            timeout: DEFAULT_IO_TIMEOUT,
            peer_pid: pid,
        }))
    }
}

/// Byte-mode duplex stream. Each I/O has a deadline and cancellation is drained.
pub struct WindowsIndexPipeStream {
    handle: OwnedHandle,
    timeout: Duration,
    peer_pid: u32,
}

impl WindowsIndexPipeStream {
    pub fn connect(identity: &IndexDaemonIdentity, timeout: Duration) -> io::Result<Self> {
        let handle = connect_raw(&identity.pipe_name(), timeout)?;
        let pid = pipe_peer_pid(handle.as_raw_handle(), false)?;
        // Authenticate the actual connected process before sending any request data.
        identity
            .verify_process(pid)
            .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))?;
        Ok(Self {
            handle,
            timeout: DEFAULT_IO_TIMEOUT,
            peer_pid: pid,
        })
    }
    pub fn set_io_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    pub fn peer_pid(&self) -> u32 {
        self.peer_pid
    }
}

impl Read for WindowsIndexPipeStream {
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
impl Write for WindowsIndexPipeStream {
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
fn create_instance(identity: &IndexDaemonIdentity, first: bool) -> io::Result<OwnedHandle> {
    let sddl = wide(&format!("D:P(A;;GA;;;{})", identity.user_sid()));
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
    let name = wide(&identity.pipe_name());
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
fn connect_raw(name: &str, timeout: Duration) -> io::Result<OwnedHandle> {
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
                "Index pipe connection timed out",
            ));
        }
        // SAFETY: name remains live; bounded wait for a server instance.
        if unsafe { WaitNamedPipeW(name.as_ptr(), milliseconds(remaining).max(1)) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_SEM_TIMEOUT as i32) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Index pipe connection timed out",
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
            io::Error::new(io::ErrorKind::TimedOut, "Index pipe I/O timed out")
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_pipe_round_trip_authenticates_and_keeps_namespace_exclusive() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        assert!(WindowsIndexPipeListener::bind(identity.clone()).is_err());
        let client = std::thread::spawn(move || {
            let mut stream =
                WindowsIndexPipeStream::connect(&identity, Duration::from_secs(2)).unwrap();
            assert_eq!(stream.peer_pid(), std::process::id());
            stream.write_all(b"request").unwrap();
            let mut reply = [0; 5];
            stream.read_exact(&mut reply).unwrap();
            assert_eq!(&reply, b"reply");
        });
        let mut stream = listener.accept(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(stream.peer_pid(), std::process::id());
        let mut request = [0; 7];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"request");
        stream.write_all(b"reply").unwrap();
        client.join().unwrap();
    }
    #[test]
    fn windows_pipe_accept_timeout_can_be_retried() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        assert!(
            listener
                .accept(Duration::from_millis(20))
                .unwrap()
                .is_none()
        );
        let client = WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        let _stream = listener.accept(Duration::from_secs(1)).unwrap().unwrap();
        drop(client);
    }
    #[test]
    fn windows_pipe_read_timeout_drains_cancellation_and_recovers() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        let mut client =
            WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        let mut server = listener.accept(Duration::from_secs(1)).unwrap().unwrap();
        server.set_io_timeout(Duration::from_millis(20));
        let mut data = [0; 1];
        assert_eq!(
            server.read(&mut data).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        client.write_all(b"x").unwrap();
        server.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"x");
    }
    #[test]
    fn windows_pipe_buffered_response_survives_server_close() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        let mut client =
            WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        let mut server = listener.accept(Duration::from_secs(1)).unwrap().unwrap();
        server.write_all(b"reply").unwrap();
        drop(server);
        let mut data = [0; 5];
        client.read_exact(&mut data).unwrap();
        assert_eq!(&data, b"reply");
    }
    #[test]
    #[ignore = "requires coordinated owned cross-user pipe probes"]
    fn windows_pipe_hold_owner_only_endpoint_for_foreign_probe() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let _listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        let ready = std::env::var("OTTRIN_QA_PIPE_READY").unwrap();
        let stop = std::env::var("OTTRIN_QA_PIPE_STOP").unwrap();
        std::fs::write(ready, identity.pipe_name()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !std::path::Path::new(&stop).exists() {
            assert!(
                Instant::now() < deadline,
                "Probe controller did not stop owned server"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    #[ignore = "requires an owned endpoint created by another Windows user"]
    fn windows_pipe_rejects_foreign_user_client_at_acl_boundary() {
        let name = std::env::var("OTTRIN_QA_PIPE_NAME").unwrap();
        let error = connect_raw(&name, Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    }
    #[test]
    #[ignore = "requires an owned foreign-user counterfeit server"]
    fn windows_pipe_rejects_foreign_server_before_sending_data() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let error = match WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)) {
            Ok(_) => panic!("Foreign server accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("another Windows user"));
    }
    #[test]
    fn windows_pipe_disconnected_client_does_not_poison_listener() {
        let identity = IndexDaemonIdentity::for_current_user().unwrap();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        let client = WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        drop(client);
        drop(listener.accept(Duration::from_millis(20)).unwrap());
        let _client = WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        let _server = listener.accept(Duration::from_secs(1)).unwrap().unwrap();
    }
}
