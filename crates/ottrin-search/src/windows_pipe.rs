//! Per-user index endpoint policy over the shared native pipe implementation.
use crate::IndexDaemonIdentity;
pub(super) use ottrin_platform::windows_pipe::MAX_INSTANCES;
#[cfg(test)]
use ottrin_platform::windows_pipe::connect_raw;
use ottrin_platform::windows_pipe::{LocalPipeListener, LocalPipeStream};
use std::io::{self, Read, Write};
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;
#[cfg(test)]
use windows_sys::Win32::Foundation::*;

pub struct WindowsIndexPipeListener {
    identity: IndexDaemonIdentity,
    pipe: LocalPipeListener,
}
impl WindowsIndexPipeListener {
    pub fn bind(identity: IndexDaemonIdentity) -> io::Result<Self> {
        let pipe = LocalPipeListener::bind(
            &identity.pipe_name(),
            &format!("D:P(A;;GA;;;{})", identity.user_sid()),
        )?;
        Ok(Self { identity, pipe })
    }
    pub fn accept(&mut self, timeout: Duration) -> io::Result<Option<WindowsIndexPipeStream>> {
        self.pipe
            .accept(timeout, |pid| {
                self.identity
                    .verify_process(pid)
                    .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))
            })
            .map(|stream| stream.map(WindowsIndexPipeStream))
    }
}
pub struct WindowsIndexPipeStream(LocalPipeStream);
impl WindowsIndexPipeStream {
    pub fn connect(identity: &IndexDaemonIdentity, timeout: Duration) -> io::Result<Self> {
        LocalPipeStream::connect(&identity.pipe_name(), timeout, |pid| {
            identity
                .verify_process(pid)
                .map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))
        })
        .map(Self)
    }
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }
    pub(crate) fn cancel_pending_io(&self) -> io::Result<()> {
        self.0.cancel_pending_io()
    }
    pub fn set_io_timeout(&mut self, timeout: Duration) {
        self.0.set_io_timeout(timeout);
    }
    pub fn peer_pid(&self) -> u32 {
        self.0.peer_pid()
    }
}
impl Read for WindowsIndexPipeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}
impl Write for WindowsIndexPipeStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_pipe_round_trip_authenticates_and_keeps_namespace_exclusive() {
        let identity = IndexDaemonIdentity::isolated_for_test();
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
        let identity = IndexDaemonIdentity::isolated_for_test();
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
        let identity = IndexDaemonIdentity::isolated_for_test();
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
        let identity = IndexDaemonIdentity::isolated_for_test();
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
        let identity = IndexDaemonIdentity::isolated_for_test();
        let mut listener = WindowsIndexPipeListener::bind(identity.clone()).unwrap();
        let client = WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        drop(client);
        drop(listener.accept(Duration::from_millis(20)).unwrap());
        let _client = WindowsIndexPipeStream::connect(&identity, Duration::from_secs(1)).unwrap();
        let _server = listener.accept(Duration::from_secs(1)).unwrap().unwrap();
    }
}
