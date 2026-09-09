use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
#[cfg(feature = "system-gstreamer")]
use std::os::fd::FromRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
#[cfg(not(feature = "system-gstreamer"))]
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

pub const MAX_MEDIA_PLAYBACK_SOURCE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION: u32 = 4_096;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaPlaybackFrameError {
    #[error("decoded media frame dimensions exceed the playback bound")]
    DimensionsExceeded { width: u32, height: u32 },
    #[error("decoded media frame has {actual} RGBA bytes; expected {expected}")]
    InvalidPayload { expected: usize, actual: usize },
}

pub fn validate_media_playback_rgba_frame(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), MediaPlaybackFrameError> {
    if width == 0
        || height == 0
        || width > MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION
        || height > MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION
    {
        return Err(MediaPlaybackFrameError::DimensionsExceeded { width, height });
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(MediaPlaybackFrameError::DimensionsExceeded { width, height })?;
    if rgba.len() != expected {
        return Err(MediaPlaybackFrameError::InvalidPayload {
            expected,
            actual: rgba.len(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPlaybackFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPlaybackRuntimeFailure {
    RuntimeUnavailable,
    MissingComponents,
}

impl MediaPlaybackRuntimeFailure {
    pub const fn user_message(self) -> &'static str {
        match self {
            Self::RuntimeUnavailable => {
                "System GStreamer playback is unavailable on this installation. This does not mean this file is invalid. Ottrin does not install codecs. You can open the file in another application."
            }
            Self::MissingComponents => {
                "This GStreamer installation does not provide the components needed to play this file. Ottrin does not install codecs. You can manage media support through your operating system or open the file in another application."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaPlaybackEvent {
    Playing,
    Paused,
    Ended,
    Frame(MediaPlaybackFrame),
    Unavailable(MediaPlaybackRuntimeFailure),
    Failed(String),
}

#[cfg_attr(not(feature = "system-gstreamer"), allow(dead_code))]
fn media_playback_failure_event(missing_components: bool, details: String) -> MediaPlaybackEvent {
    if missing_components {
        MediaPlaybackEvent::Unavailable(MediaPlaybackRuntimeFailure::MissingComponents)
    } else {
        MediaPlaybackEvent::Failed(details)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaPlaybackCommand {
    Play,
    Pause,
    Stop,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaPlaybackStartError {
    #[error("could not start the media playback worker")]
    Spawn(#[source] io::Error),
}

#[derive(Debug)]
pub struct MediaPlaybackSession {
    commands: SyncSender<MediaPlaybackCommand>,
    events: Receiver<MediaPlaybackEvent>,
    worker: Option<JoinHandle<()>>,
}

impl MediaPlaybackSession {
    pub fn play(&self) {
        let _ = self.commands.try_send(MediaPlaybackCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.try_send(MediaPlaybackCommand::Pause);
    }

    pub fn try_recv(&self) -> Result<MediaPlaybackEvent, std::sync::mpsc::TryRecvError> {
        self.events.try_recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<MediaPlaybackEvent, RecvTimeoutError> {
        self.events.recv_timeout(timeout)
    }

    fn request_stop(&self) {
        let _ = self.commands.try_send(MediaPlaybackCommand::Stop);
    }

    pub fn stop(&mut self) {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for MediaPlaybackSession {
    fn drop(&mut self) {
        self.request_stop();
        // Inspector closure must stay bounded even if a system decoder stalls
        // during teardown. Dropping the sender guarantees the detached worker
        // will observe disconnection after draining any already queued command.
        let _ = self.worker.take();
    }
}

pub fn start_media_playback(
    source: AdmittedMediaPlaybackSource,
) -> Result<MediaPlaybackSession, MediaPlaybackStartError> {
    let (commands_tx, commands_rx) = sync_channel(4);
    let (events_tx, events_rx) = sync_channel(3);
    let worker = std::thread::Builder::new()
        .name("ottrin-media-playback".to_string())
        .spawn(move || media_playback_worker(source, commands_rx, events_tx))
        .map_err(MediaPlaybackStartError::Spawn)?;
    Ok(MediaPlaybackSession {
        commands: commands_tx,
        events: events_rx,
        worker: Some(worker),
    })
}

const MEDIA_HELPER_SOURCE_FD: i32 = 3;
const MEDIA_HELPER_CONTROL_FD: i32 = 4;
const MEDIA_HELPER_MAX_ERROR_BYTES: usize = 64 * 1024;

#[cfg(not(feature = "system-gstreamer"))]
fn media_playback_worker(
    source: AdmittedMediaPlaybackSource,
    commands: Receiver<MediaPlaybackCommand>,
    events: SyncSender<MediaPlaybackEvent>,
) {
    if let Err(error) = run_external_media_helper(source, commands, &events) {
        let _ = events.try_send(MediaPlaybackEvent::Failed(error.to_string()));
    }
}

#[cfg(not(feature = "system-gstreamer"))]
fn run_external_media_helper(
    source: AdmittedMediaPlaybackSource,
    commands: Receiver<MediaPlaybackCommand>,
    events: &SyncSender<MediaPlaybackEvent>,
) -> io::Result<()> {
    use std::os::unix::process::CommandExt;

    let helper = std::env::var_os("OTTRIN_MEDIA_HELPER")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|executable| executable.parent().map(Path::to_path_buf))
                .map(|directory| directory.join("ottrin-media-playback"))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "media helper path unavailable"))?;
    let (mut control, child_control) = UnixStream::pair()?;
    let source_fd = unsafe { libc::fcntl(source.file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if source_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let control_fd = unsafe { libc::fcntl(child_control.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    if control_fd < 0 {
        unsafe { libc::close(source_fd) };
        return Err(io::Error::last_os_error());
    }
    let mut process = Command::new(&helper);
    process
        .arg("--inherited-media-session")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("LD_AUDIT")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    unsafe {
        process.pre_exec(move || {
            if libc::dup2(source_fd, MEDIA_HELPER_SOURCE_FD) < 0
                || libc::dup2(control_fd, MEDIA_HELPER_CONTROL_FD) < 0
            {
                return Err(io::Error::last_os_error());
            }
            libc::close(source_fd);
            libc::close(control_fd);
            Ok(())
        });
    }
    let child = process.spawn();
    unsafe {
        libc::close(source_fd);
        libc::close(control_fd);
    }
    let mut child = child.map_err(|error| {
        io::Error::new(error.kind(), format!(
            "Could not start media helper {}: {error}. For Linux development, run `cargo build -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer` with the same target directory and profile as ottrin-app (see docs/DEVELOPMENT.md). Installed packages require the system GStreamer runtime.",
            helper.display()
        ))
    })?;
    drop(child_control);

    let mut event_stream = control.try_clone()?;
    let event_sender = events.clone();
    std::thread::Builder::new()
        .name("ottrin-media-helper-events".to_string())
        .spawn(move || {
            while let Ok(event) = read_media_helper_event(&mut event_stream) {
                let _ = event_sender.try_send(event);
            }
        })?;

    loop {
        match commands.recv_timeout(Duration::from_millis(20)) {
            Ok(command) => {
                control.write_all(&[encode_media_helper_command(command)])?;
                if command == MediaPlaybackCommand::Stop {
                    stop_media_helper(&mut child);
                    return Ok(());
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ =
                    control.write_all(&[encode_media_helper_command(MediaPlaybackCommand::Stop)]);
                stop_media_helper(&mut child);
                return Ok(());
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(status) = child.try_wait()? {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(io::Error::other(format!(
                            "Media helper {} exited with {status}. Check its terminal diagnostics and system GStreamer runtime dependencies.",
                            helper.display()
                        )))
                    };
                }
            }
        }
    }
}

#[cfg(not(feature = "system-gstreamer"))]
fn stop_media_helper(child: &mut Child) {
    for _ in 0..5 {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(feature = "system-gstreamer"))]
fn encode_media_helper_command(command: MediaPlaybackCommand) -> u8 {
    match command {
        MediaPlaybackCommand::Play => 1,
        MediaPlaybackCommand::Pause => 2,
        MediaPlaybackCommand::Stop => 3,
    }
}

#[cfg(feature = "system-gstreamer")]
fn decode_media_helper_command(value: u8) -> io::Result<MediaPlaybackCommand> {
    match value {
        1 => Ok(MediaPlaybackCommand::Play),
        2 => Ok(MediaPlaybackCommand::Pause),
        3 => Ok(MediaPlaybackCommand::Stop),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid media helper command",
        )),
    }
}

#[cfg(feature = "system-gstreamer")]
fn write_media_helper_event(stream: &mut UnixStream, event: MediaPlaybackEvent) -> io::Result<()> {
    match event {
        MediaPlaybackEvent::Playing => stream.write_all(&[1]),
        MediaPlaybackEvent::Paused => stream.write_all(&[2]),
        MediaPlaybackEvent::Ended => stream.write_all(&[3]),
        MediaPlaybackEvent::Unavailable(failure) => stream.write_all(&[
            4,
            match failure {
                MediaPlaybackRuntimeFailure::RuntimeUnavailable => 1,
                MediaPlaybackRuntimeFailure::MissingComponents => 2,
            },
        ]),
        MediaPlaybackEvent::Failed(details) => {
            let bytes = details.as_bytes();
            let len = u32::try_from(bytes.len().min(MEDIA_HELPER_MAX_ERROR_BYTES)).unwrap();
            stream.write_all(&[5])?;
            stream.write_all(&len.to_le_bytes())?;
            stream.write_all(&bytes[..len as usize])
        }
        MediaPlaybackEvent::Frame(frame) => {
            validate_media_playback_rgba_frame(frame.width, frame.height, &frame.rgba)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            stream.write_all(&[6])?;
            stream.write_all(&frame.width.to_le_bytes())?;
            stream.write_all(&frame.height.to_le_bytes())?;
            stream.write_all(&(frame.rgba.len() as u32).to_le_bytes())?;
            stream.write_all(&frame.rgba)
        }
    }
}

#[cfg(not(feature = "system-gstreamer"))]
fn read_media_helper_event(stream: &mut UnixStream) -> io::Result<MediaPlaybackEvent> {
    let mut kind = [0_u8; 1];
    stream.read_exact(&mut kind)?;
    match kind[0] {
        1 => Ok(MediaPlaybackEvent::Playing),
        2 => Ok(MediaPlaybackEvent::Paused),
        3 => Ok(MediaPlaybackEvent::Ended),
        4 => {
            let mut failure = [0_u8; 1];
            stream.read_exact(&mut failure)?;
            match failure[0] {
                1 => Ok(MediaPlaybackEvent::Unavailable(
                    MediaPlaybackRuntimeFailure::RuntimeUnavailable,
                )),
                2 => Ok(MediaPlaybackEvent::Unavailable(
                    MediaPlaybackRuntimeFailure::MissingComponents,
                )),
                _ => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid media helper failure",
                )),
            }
        }
        5 => {
            let len = read_media_helper_u32(stream)? as usize;
            if len > MEDIA_HELPER_MAX_ERROR_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "media helper error is too large",
                ));
            }
            let mut bytes = vec![0; len];
            stream.read_exact(&mut bytes)?;
            let details = String::from_utf8(bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "media helper error is not UTF-8",
                )
            })?;
            Ok(MediaPlaybackEvent::Failed(details))
        }
        6 => {
            let width = read_media_helper_u32(stream)?;
            let height = read_media_helper_u32(stream)?;
            let len = read_media_helper_u32(stream)? as usize;
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(4))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid media frame dimensions")
                })?;
            if width > MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION
                || height > MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION
                || len != expected
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid media helper frame",
                ));
            }
            let mut rgba = vec![0; len];
            stream.read_exact(&mut rgba)?;
            validate_media_playback_rgba_frame(width, height, &rgba)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            Ok(MediaPlaybackEvent::Frame(MediaPlaybackFrame {
                width,
                height,
                rgba,
            }))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid media helper event",
        )),
    }
}

#[cfg(not(feature = "system-gstreamer"))]
fn read_media_helper_u32(stream: &mut UnixStream) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    stream.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

#[cfg(feature = "system-gstreamer")]
pub fn run_media_playback_helper() -> io::Result<()> {
    let source_file = unsafe { File::from_raw_fd(MEDIA_HELPER_SOURCE_FD) };
    let source = AdmittedMediaPlaybackSource::from_retained_file(source_file)?;
    let control = unsafe { UnixStream::from_raw_fd(MEDIA_HELPER_CONTROL_FD) };
    let mut event_stream = control.try_clone()?;
    let mut command_stream = control;
    let (commands_tx, commands_rx) = sync_channel(4);
    let (events_tx, events_rx) = sync_channel(3);
    std::thread::Builder::new()
        .name("ottrin-media-helper-commands".to_string())
        .spawn(move || {
            let mut command = [0_u8; 1];
            while command_stream.read_exact(&mut command).is_ok() {
                let Ok(command) = decode_media_helper_command(command[0]) else {
                    break;
                };
                let stopping = command == MediaPlaybackCommand::Stop;
                if commands_tx.send(command).is_err() || stopping {
                    break;
                }
            }
        })?;
    let writer = std::thread::Builder::new()
        .name("ottrin-media-helper-writer".to_string())
        .spawn(move || {
            while let Ok(event) = events_rx.recv() {
                if write_media_helper_event(&mut event_stream, event).is_err() {
                    break;
                }
            }
        })?;
    if let Err(failure) = run_gstreamer_playback(source, commands_rx, &events_tx) {
        let _ = events_tx.send(MediaPlaybackEvent::Unavailable(failure));
    }
    drop(events_tx);
    let _ = writer.join();
    Ok(())
}

#[cfg(feature = "system-gstreamer")]
fn media_playback_worker(
    source: AdmittedMediaPlaybackSource,
    commands: Receiver<MediaPlaybackCommand>,
    events: SyncSender<MediaPlaybackEvent>,
) {
    if let Err(failure) = run_gstreamer_playback(source, commands, &events) {
        let _ = events.try_send(MediaPlaybackEvent::Unavailable(failure));
    }
}

#[cfg(feature = "system-gstreamer")]
fn run_gstreamer_playback(
    source: AdmittedMediaPlaybackSource,
    commands: Receiver<MediaPlaybackCommand>,
    events: &SyncSender<MediaPlaybackEvent>,
) -> Result<(), MediaPlaybackRuntimeFailure> {
    use gst::prelude::*;
    use gstreamer as gst;

    gst::init().map_err(|_| MediaPlaybackRuntimeFailure::RuntimeUnavailable)?;
    let caps = gst::Caps::builder("video/x-raw")
        .field("format", "RGBA")
        .field(
            "width",
            gst::IntRange::new(1i32, MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION as i32),
        )
        .field(
            "height",
            gst::IntRange::new(1i32, MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION as i32),
        )
        .build();
    let frame_events = events.clone();
    let appsink = gstreamer_app::AppSink::builder()
        .caps(&caps)
        .max_buffers(1)
        .drop(true)
        .wait_on_eos(false)
        .sync(true)
        .callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Error)?;
                    let caps = sample.caps().ok_or(gst::FlowError::NotNegotiated)?;
                    let structure = caps.structure(0).ok_or(gst::FlowError::NotNegotiated)?;
                    let width = structure
                        .get::<i32>("width")
                        .map_err(|_| gst::FlowError::NotNegotiated)?;
                    let height = structure
                        .get::<i32>("height")
                        .map_err(|_| gst::FlowError::NotNegotiated)?;
                    let width = u32::try_from(width).map_err(|_| gst::FlowError::Error)?;
                    let height = u32::try_from(height).map_err(|_| gst::FlowError::Error)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    validate_media_playback_rgba_frame(width, height, map.as_slice())
                        .map_err(|_| gst::FlowError::Error)?;
                    let _ = frame_events.try_send(MediaPlaybackEvent::Frame(MediaPlaybackFrame {
                        width,
                        height,
                        rgba: map.as_slice().to_vec(),
                    }));
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        )
        .build();
    let playbin = gst::ElementFactory::make("playbin")
        .build()
        .map_err(|_| MediaPlaybackRuntimeFailure::MissingComponents)?;
    playbin.set_property("uri", format!("fd://{}", source.file.as_raw_fd()));
    playbin.set_property("video-sink", appsink.upcast_ref::<gst::Element>());
    let bus = playbin
        .bus()
        .ok_or(MediaPlaybackRuntimeFailure::RuntimeUnavailable)?;
    playbin
        .set_state(gst::State::Playing)
        .map_err(|_| MediaPlaybackRuntimeFailure::MissingComponents)?;
    let _ = events.try_send(MediaPlaybackEvent::Playing);

    let mut stopping = false;
    while !stopping {
        match commands.recv_timeout(Duration::from_millis(20)) {
            Ok(MediaPlaybackCommand::Play) => {
                if playbin.set_state(gst::State::Playing).is_ok() {
                    let _ = events.try_send(MediaPlaybackEvent::Playing);
                }
            }
            Ok(MediaPlaybackCommand::Pause) => {
                if playbin.set_state(gst::State::Paused).is_ok() {
                    let _ = events.try_send(MediaPlaybackEvent::Paused);
                }
            }
            Ok(MediaPlaybackCommand::Stop) | Err(RecvTimeoutError::Disconnected) => {
                stopping = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        while let Some(message) = bus.timed_pop(gst::ClockTime::ZERO) {
            match message.view() {
                gst::MessageView::Eos(_) => {
                    let _ = events.try_send(MediaPlaybackEvent::Ended);
                    stopping = true;
                }
                gst::MessageView::Error(error) => {
                    let playback_error = error.error();
                    let missing_components = playback_error.matches(gst::CoreError::MissingPlugin)
                        || playback_error.matches(gst::StreamError::CodecNotFound);
                    let details = error
                        .debug()
                        .map(|debug| format!("{playback_error} ({debug})"))
                        .unwrap_or_else(|| playback_error.to_string());
                    let _ =
                        events.try_send(media_playback_failure_event(missing_components, details));
                    stopping = true;
                }
                _ => {}
            }
        }
    }
    let _ = playbin.set_state(gst::State::Null);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MediaPlaybackSourceIdentity {
    device: u64,
    inode: u64,
}

impl MediaPlaybackSourceIdentity {
    pub const fn device(self) -> u64 {
        self.device
    }

    pub const fn inode(self) -> u64 {
        self.inode
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaPlaybackAdmissionError {
    #[error("media source admission failed")]
    Open(#[source] io::Error),
    #[error("media playback accepts only no-follow regular files")]
    UnsafeSource,
    #[error("media source exceeds the playback admission limit")]
    SourceTooLarge,
}

#[derive(Debug)]
pub struct AdmittedMediaPlaybackSource {
    file: File,
    identity: MediaPlaybackSourceIdentity,
    len: u64,
}

impl AdmittedMediaPlaybackSource {
    #[cfg(feature = "system-gstreamer")]
    fn from_retained_file(file: File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_MEDIA_PLAYBACK_SOURCE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe inherited media source",
            ));
        }
        Ok(Self {
            identity: MediaPlaybackSourceIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            },
            len: metadata.len(),
            file,
        })
    }

    pub const fn identity(&self) -> MediaPlaybackSourceIdentity {
        self.identity
    }

    pub const fn len(&self) -> u64 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn try_clone_file(&self) -> io::Result<File> {
        self.file.try_clone()
    }
}

pub fn admit_media_playback_source(
    path: &Path,
) -> Result<AdmittedMediaPlaybackSource, MediaPlaybackAdmissionError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| {
            if error.raw_os_error() == Some(libc::ELOOP) {
                MediaPlaybackAdmissionError::UnsafeSource
            } else {
                MediaPlaybackAdmissionError::Open(error)
            }
        })?;
    let metadata = file.metadata().map_err(MediaPlaybackAdmissionError::Open)?;
    if !metadata.file_type().is_file() {
        return Err(MediaPlaybackAdmissionError::UnsafeSource);
    }
    if metadata.len() > MAX_MEDIA_PLAYBACK_SOURCE_BYTES {
        return Err(MediaPlaybackAdmissionError::SourceTooLarge);
    }
    Ok(AdmittedMediaPlaybackSource {
        identity: MediaPlaybackSourceIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        len: metadata.len(),
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_system_component_errors_use_the_codec_fallback() {
        assert_eq!(
            media_playback_failure_event(true, "missing decoder".to_string()),
            MediaPlaybackEvent::Unavailable(MediaPlaybackRuntimeFailure::MissingComponents)
        );
    }

    #[test]
    fn dropping_a_playback_session_does_not_wait_for_decoder_teardown() {
        let (commands, _command_rx) = sync_channel(1);
        let (_events_tx, events) = sync_channel(1);
        let worker = std::thread::spawn(|| std::thread::sleep(Duration::from_millis(300)));
        let session = MediaPlaybackSession {
            commands,
            events,
            worker: Some(worker),
        };

        let started = std::time::Instant::now();
        drop(session);

        assert!(
            started.elapsed() < Duration::from_millis(100),
            "closing the Inspector must not block on decoder teardown"
        );
    }
}
