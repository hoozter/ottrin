//! Query lifetime controls shared by authenticated socket and pipe connections.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(unix)]
pub(super) type DaemonStream = std::os::unix::net::UnixStream;
#[cfg(target_os = "windows")]
pub(super) type DaemonStream = crate::WindowsIndexPipeStream;

pub(super) fn monitor_timeout(stream: &mut DaemonStream, timeout: Duration) -> io::Result<()> {
    #[cfg(unix)]
    {
        stream.set_read_timeout(Some(timeout))
    }
    #[cfg(target_os = "windows")]
    {
        stream.set_io_timeout(timeout);
        Ok(())
    }
}

pub(super) fn interrupt(stream: &DaemonStream) -> io::Result<()> {
    #[cfg(unix)]
    {
        stream.shutdown(std::net::Shutdown::Both)
    }
    #[cfg(target_os = "windows")]
    {
        stream.cancel_pending_io()
    }
}

/// Release scoped monitor threads on ordinary completion and unwind alike.
pub(super) struct QueryFinished<'a>(pub(super) &'a AtomicBool);
impl Drop for QueryFinished<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
