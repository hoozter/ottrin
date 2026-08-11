use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VideoThumbnailSourceGeneration([u8; 32]);

impl VideoThumbnailSourceGeneration {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoThumbnailLimits {
    pub max_source_dimension: u32,
    pub max_source_pixels: u64,
    pub max_output_dimension: u32,
    pub max_output_pixels: u64,
    pub max_output_bytes: usize,
    pub deadline: Duration,
}

impl VideoThumbnailLimits {
    pub const fn for_target(target_dimension: u32) -> Self {
        Self {
            max_source_dimension: 8_192,
            max_source_pixels: 33_554_432,
            max_output_dimension: target_dimension,
            max_output_pixels: 4_194_304,
            max_output_bytes: 16 * 1024 * 1024,
            deadline: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedVideoThumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub source_generation: VideoThumbnailSourceGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedVideoThumbnail {
    pub png: Vec<u8>,
    pub source_generation: VideoThumbnailSourceGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VideoThumbnailError {
    #[error("video thumbnails are unavailable on this platform")]
    UnsupportedPlatform,
    #[error("video source admission failed")]
    AdmissionFailed,
    #[error("video source is not a no-follow regular file")]
    UnsafeSource,
    #[error("Windows Media Foundation is unavailable")]
    NativeBackendUnavailable,
    #[error("the native video backend does not support this media")]
    UnsupportedMedia,
    #[error("video thumbnail decoding was cancelled")]
    Cancelled,
    #[error("video thumbnail decoding exceeded its deadline")]
    TimedOut,
    #[error("video thumbnail exceeded its resource limits")]
    ResourceLimit,
    #[error("the native video backend returned an invalid frame")]
    InvalidFrame,
    #[error("the admitted video changed while its thumbnail was decoded")]
    SourceChanged,
    #[error("the native video backend could not decode a frame")]
    DecodeFailed,
}

#[derive(Debug)]
pub struct AuthorizedVideoSource {
    generation: VideoThumbnailSourceGeneration,
    #[cfg(target_os = "windows")]
    file: Arc<std::fs::File>,
    #[cfg(target_os = "windows")]
    identity: WindowsFileIdentity,
}

impl AuthorizedVideoSource {
    pub fn generation(&self) -> &VideoThumbnailSourceGeneration {
        &self.generation
    }
}

#[cfg(not(target_os = "windows"))]
pub fn admit_video_thumbnail_source(
    _path: &Path,
) -> Result<AuthorizedVideoSource, VideoThumbnailError> {
    Err(VideoThumbnailError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsFileIdentity {
    volume_serial_number: u32,
    file_index: u64,
    file_size: u64,
    last_write_time: u64,
    change_time: i64,
}

#[cfg(target_os = "windows")]
fn windows_file_identity(file: &std::fs::File) -> Result<WindowsFileIdentity, VideoThumbnailError> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FileBasicInfo,
        GetFileInformationByHandle, GetFileInformationByHandleEx,
    };

    let metadata = file
        .metadata()
        .map_err(|_| VideoThumbnailError::AdmissionFailed)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the Rust File owns a valid handle for this call and the output
    // structure remains live and exclusively borrowed until the call returns.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
        .map_err(|_| VideoThumbnailError::AdmissionFailed)?;
    let mut basic_information = FILE_BASIC_INFO::default();
    // SAFETY: the same owned handle stays valid and the typed output buffer is
    // writable for exactly the advertised structure size.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            (&raw mut basic_information).cast(),
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .map_err(|_| VideoThumbnailError::AdmissionFailed)?;
    if !metadata.file_type().is_file()
        || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(VideoThumbnailError::UnsafeSource);
    }
    Ok(WindowsFileIdentity {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
        file_size: (u64::from(information.nFileSizeHigh) << 32)
            | u64::from(information.nFileSizeLow),
        last_write_time: (u64::from(information.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(information.ftLastWriteTime.dwLowDateTime),
        change_time: basic_information.ChangeTime,
    })
}

#[cfg(target_os = "windows")]
pub fn admit_video_thumbnail_source(
    path: &Path,
) -> Result<AuthorizedVideoSource, VideoThumbnailError> {
    use sha2::{Digest, Sha256};
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|_| VideoThumbnailError::AdmissionFailed)?;
    let identity = windows_file_identity(&file)?;
    let mut digest = Sha256::new();
    digest.update(identity.volume_serial_number.to_le_bytes());
    digest.update(identity.file_index.to_le_bytes());
    digest.update(identity.file_size.to_le_bytes());
    digest.update(identity.last_write_time.to_le_bytes());
    digest.update(identity.change_time.to_le_bytes());
    let generation = VideoThumbnailSourceGeneration(digest.finalize().into());

    Ok(AuthorizedVideoSource {
        generation,
        file: Arc::new(file),
        identity,
    })
}

/// Converts one already-bounded native BGRA frame into the sole image form
/// accepted by the cache and UI. Padding is discarded and alpha is normalized
/// because Media Foundation's RGB32 output does not define a useful alpha byte.
pub fn validate_video_thumbnail_bgra(
    source_generation: VideoThumbnailSourceGeneration,
    width: u32,
    height: u32,
    stride: usize,
    bgra: &[u8],
    limits: &VideoThumbnailLimits,
) -> Result<ValidatedVideoThumbnail, VideoThumbnailError> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(VideoThumbnailError::ResourceLimit)?;
    let row_bytes = usize::try_from(
        u64::from(width)
            .checked_mul(4)
            .ok_or(VideoThumbnailError::ResourceLimit)?,
    )
    .map_err(|_| VideoThumbnailError::ResourceLimit)?;
    let output_bytes = usize::try_from(
        pixels
            .checked_mul(4)
            .ok_or(VideoThumbnailError::ResourceLimit)?,
    )
    .map_err(|_| VideoThumbnailError::ResourceLimit)?;

    if width == 0
        || height == 0
        || width > limits.max_output_dimension
        || height > limits.max_output_dimension
        || pixels > limits.max_output_pixels
        || output_bytes > limits.max_output_bytes
    {
        return Err(VideoThumbnailError::ResourceLimit);
    }
    let required_input = stride
        .checked_mul(usize::try_from(height - 1).map_err(|_| VideoThumbnailError::InvalidFrame)?)
        .and_then(|prefix| prefix.checked_add(row_bytes))
        .ok_or(VideoThumbnailError::InvalidFrame)?;
    if stride < row_bytes || bgra.len() < required_input {
        return Err(VideoThumbnailError::InvalidFrame);
    }

    let mut rgba = Vec::with_capacity(output_bytes);
    for row in 0..height as usize {
        let start = row
            .checked_mul(stride)
            .ok_or(VideoThumbnailError::InvalidFrame)?;
        let source = bgra
            .get(start..start + row_bytes)
            .ok_or(VideoThumbnailError::InvalidFrame)?;
        for pixel in source.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }

    Ok(ValidatedVideoThumbnail {
        width,
        height,
        rgba,
        source_generation,
    })
}

pub fn bounded_video_thumbnail_dimensions(
    width: u32,
    height: u32,
    limits: &VideoThumbnailLimits,
) -> Result<(u32, u32), VideoThumbnailError> {
    let source_pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(VideoThumbnailError::ResourceLimit)?;
    if width == 0
        || height == 0
        || width > limits.max_source_dimension
        || height > limits.max_source_dimension
        || source_pixels > limits.max_source_pixels
        || limits.max_output_dimension == 0
    {
        return Err(VideoThumbnailError::ResourceLimit);
    }
    let target = limits.max_output_dimension;
    if width <= target && height <= target {
        return Ok((width, height));
    }
    if width >= height {
        let scaled_height = (u64::from(height) * u64::from(target) / u64::from(width)).max(1);
        Ok((target, scaled_height as u32))
    } else {
        let scaled_width = (u64::from(width) * u64::from(target) / u64::from(height)).max(1);
        Ok((scaled_width as u32, target))
    }
}

pub fn video_thumbnail_cache_key(
    generation: &VideoThumbnailSourceGeneration,
    target_dimension: u32,
) -> String {
    let generation = generation
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("video-mf-v1-{generation}-{target_dimension}.png")
}

#[cfg(not(target_os = "windows"))]
pub fn render_video_thumbnail_png(
    _source: AuthorizedVideoSource,
    _target_dimension: u32,
    _cancelled: Arc<AtomicBool>,
) -> Result<RenderedVideoThumbnail, VideoThumbnailError> {
    Err(VideoThumbnailError::UnsupportedPlatform)
}

#[cfg(target_os = "windows")]
pub fn render_video_thumbnail_png(
    source: AuthorizedVideoSource,
    target_dimension: u32,
    cancelled: Arc<AtomicBool>,
) -> Result<RenderedVideoThumbnail, VideoThumbnailError> {
    use image::ImageEncoder;

    let limits = VideoThumbnailLimits::for_target(target_dimension);
    let started = Instant::now();
    if target_dimension == 0 {
        return Err(VideoThumbnailError::ResourceLimit);
    }
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(VideoThumbnailError::Cancelled);
    }
    let frame = windows_backend::decode_one_frame(source, Arc::clone(&cancelled), &limits)?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &frame.rgba,
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|_| VideoThumbnailError::DecodeFailed)?;
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(VideoThumbnailError::Cancelled);
    }
    if started.elapsed() >= limits.deadline {
        return Err(VideoThumbnailError::TimedOut);
    }
    if png.len() > limits.max_output_bytes {
        return Err(VideoThumbnailError::ResourceLimit);
    }
    Ok(RenderedVideoThumbnail {
        png,
        source_generation: frame.source_generation,
    })
}

#[cfg(target_os = "windows")]
mod windows_backend {
    use super::*;
    use std::os::windows::fs::FileExt;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{
        E_NOTIMPL, ERROR_CANCELLED, ERROR_TIMEOUT, S_FALSE, S_OK, STG_E_ACCESSDENIED,
        STG_E_INVALIDFUNCTION, STG_E_INVALIDPOINTER,
    };
    use windows::Win32::Media::MediaFoundation::{
        IMF2DBuffer2, IMFMediaType, IMFSourceReader, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
        MF_MT_SUBTYPE, MF_SOURCE_READER_ALL_STREAMS,
        MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
        MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION,
        MF2DBuffer_LockFlags_Read, MFCreateAttributes, MFCreateMFByteStreamOnStreamEx,
        MFCreateMediaType, MFCreateSourceReaderFromByteStream, MFMediaType_Video, MFShutdown,
        MFStartup, MFVideoFormat_RGB32,
    };
    use windows::Win32::System::Com::{
        COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, ISequentialStream_Impl, IStream,
        IStream_Impl, LOCKTYPE, STATFLAG, STATSTG, STGC, STGM_READ, STGTY_STREAM, STREAM_SEEK,
        STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET,
    };
    use windows::core::{Error, HRESULT, Interface, implement};

    const MAX_SAMPLE_READ_ATTEMPTS: usize = 8;

    struct ComApartment;

    impl ComApartment {
        fn initialize() -> Result<Self, VideoThumbnailError> {
            // SAFETY: the dedicated preview worker initializes COM once for this
            // decode scope and this guard balances every successful HRESULT.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .map_err(|_| VideoThumbnailError::NativeBackendUnavailable)?;
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            // SAFETY: paired with this guard's successful CoInitializeEx call.
            unsafe { CoUninitialize() };
        }
    }

    struct MediaFoundationSession;

    impl MediaFoundationSession {
        fn start() -> Result<Self, VideoThumbnailError> {
            // SAFETY: MFStartup is process-refcounted and balanced by this guard.
            unsafe { MFStartup(MF_VERSION, 0) }
                .map_err(|_| VideoThumbnailError::NativeBackendUnavailable)?;
            Ok(Self)
        }
    }

    impl Drop for MediaFoundationSession {
        fn drop(&mut self) {
            // SAFETY: all Media Foundation objects created after this guard are
            // dropped before it, and this balances the successful MFStartup.
            let _ = unsafe { MFShutdown() };
        }
    }

    #[implement(IStream)]
    struct AdmittedFileStream {
        file: Arc<std::fs::File>,
        position: Mutex<u64>,
        length: u64,
        cancelled: Arc<AtomicBool>,
        deadline: Instant,
    }

    impl AdmittedFileStream {
        fn interruption(&self) -> Option<HRESULT> {
            if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                Some(HRESULT::from_win32(ERROR_CANCELLED.0))
            } else if Instant::now() >= self.deadline {
                Some(HRESULT::from_win32(ERROR_TIMEOUT.0))
            } else {
                None
            }
        }
    }

    impl ISequentialStream_Impl for AdmittedFileStream_Impl {
        fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
            if !pcbread.is_null() {
                // SAFETY: COM supplied an optional writable out parameter.
                unsafe { pcbread.write(0) };
            }
            if let Some(error) = self.interruption() {
                return error;
            }
            if cb > 0 && pv.is_null() {
                return STG_E_INVALIDPOINTER;
            }
            let Ok(mut position) = self.position.lock() else {
                return STG_E_INVALIDFUNCTION;
            };
            let remaining = self.length.saturating_sub(*position);
            let requested = usize::try_from(u64::from(cb).min(remaining)).unwrap_or(0);
            if requested == 0 {
                return if cb == 0 { S_OK } else { S_FALSE };
            }
            // SAFETY: COM promises `pv` is writable for `cb` bytes. The null case
            // was rejected above, and this slice never exceeds that byte count.
            let buffer = unsafe { std::slice::from_raw_parts_mut(pv.cast::<u8>(), requested) };
            match self.file.seek_read(buffer, *position) {
                Ok(read) => {
                    *position = position.saturating_add(read as u64);
                    if !pcbread.is_null() {
                        // SAFETY: same validated optional out parameter as above.
                        unsafe { pcbread.write(read as u32) };
                    }
                    if read == cb as usize { S_OK } else { S_FALSE }
                }
                Err(_) => STG_E_INVALIDFUNCTION,
            }
        }

        fn Write(&self, _pv: *const core::ffi::c_void, _cb: u32, pcbwritten: *mut u32) -> HRESULT {
            if !pcbwritten.is_null() {
                // SAFETY: COM supplied an optional writable out parameter.
                unsafe { pcbwritten.write(0) };
            }
            STG_E_ACCESSDENIED
        }
    }

    impl IStream_Impl for AdmittedFileStream_Impl {
        fn Seek(
            &self,
            move_by: i64,
            origin: STREAM_SEEK,
            new_position: *mut u64,
        ) -> windows::core::Result<()> {
            if let Some(error) = self.interruption() {
                return Err(Error::from_hresult(error));
            }
            let mut position = self
                .position
                .lock()
                .map_err(|_| Error::from_hresult(STG_E_INVALIDFUNCTION))?;
            let base = match origin {
                STREAM_SEEK_SET => 0_i128,
                STREAM_SEEK_CUR => i128::from(*position),
                STREAM_SEEK_END => i128::from(self.length),
                _ => return Err(Error::from_hresult(STG_E_INVALIDFUNCTION)),
            };
            let next = base
                .checked_add(i128::from(move_by))
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| Error::from_hresult(STG_E_INVALIDFUNCTION))?;
            *position = next;
            if !new_position.is_null() {
                // SAFETY: COM supplied an optional writable out parameter.
                unsafe { new_position.write(next) };
            }
            Ok(())
        }

        fn SetSize(&self, _new_size: u64) -> windows::core::Result<()> {
            Err(Error::from_hresult(STG_E_ACCESSDENIED))
        }

        fn CopyTo(
            &self,
            _stream: Option<&IStream>,
            _count: u64,
            _read: *mut u64,
            _written: *mut u64,
        ) -> windows::core::Result<()> {
            Err(Error::from_hresult(E_NOTIMPL))
        }

        fn Commit(&self, _flags: &STGC) -> windows::core::Result<()> {
            Ok(())
        }

        fn Revert(&self) -> windows::core::Result<()> {
            Err(Error::from_hresult(STG_E_ACCESSDENIED))
        }

        fn LockRegion(
            &self,
            _offset: u64,
            _count: u64,
            _lock_type: &LOCKTYPE,
        ) -> windows::core::Result<()> {
            Err(Error::from_hresult(E_NOTIMPL))
        }

        fn UnlockRegion(
            &self,
            _offset: u64,
            _count: u64,
            _lock_type: u32,
        ) -> windows::core::Result<()> {
            Err(Error::from_hresult(E_NOTIMPL))
        }

        fn Stat(&self, stat: *mut STATSTG, _flags: &STATFLAG) -> windows::core::Result<()> {
            if stat.is_null() {
                return Err(Error::from_hresult(STG_E_INVALIDPOINTER));
            }
            let value = STATSTG {
                r#type: STGTY_STREAM.0 as u32,
                cbSize: self.length,
                grfMode: STGM_READ,
                ..Default::default()
            };
            // SAFETY: the null out pointer was rejected above.
            unsafe { stat.write(value) };
            Ok(())
        }

        fn Clone(&self) -> windows::core::Result<IStream> {
            let position = *self
                .position
                .lock()
                .map_err(|_| Error::from_hresult(STG_E_INVALIDFUNCTION))?;
            Ok(AdmittedFileStream {
                file: Arc::clone(&self.file),
                position: Mutex::new(position),
                length: self.length,
                cancelled: Arc::clone(&self.cancelled),
                deadline: self.deadline,
            }
            .into())
        }
    }

    pub(super) fn decode_one_frame(
        source: AuthorizedVideoSource,
        cancelled: Arc<AtomicBool>,
        limits: &VideoThumbnailLimits,
    ) -> Result<ValidatedVideoThumbnail, VideoThumbnailError> {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(VideoThumbnailError::Cancelled);
        }
        let started = Instant::now();
        let deadline = started
            .checked_add(limits.deadline)
            .ok_or(VideoThumbnailError::TimedOut)?;
        let _com = ComApartment::initialize()?;
        let _mf = MediaFoundationSession::start()?;
        let stream: IStream = AdmittedFileStream {
            file: Arc::clone(&source.file),
            position: Mutex::new(0),
            length: source.identity.file_size,
            cancelled: Arc::clone(&cancelled),
            deadline,
        }
        .into();

        let decoded = decode_from_stream(
            &stream,
            source.generation.clone(),
            &cancelled,
            deadline,
            limits,
        );
        let frame =
            decoded.map_err(|error| interruption_error(&cancelled, deadline).unwrap_or(error))?;
        if windows_file_identity(&source.file)? != source.identity {
            return Err(VideoThumbnailError::SourceChanged);
        }
        interruption_error(&cancelled, deadline).map_or(Ok(frame), Err)
    }

    fn interruption_error(
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Option<VideoThumbnailError> {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            Some(VideoThumbnailError::Cancelled)
        } else if Instant::now() >= deadline {
            Some(VideoThumbnailError::TimedOut)
        } else {
            None
        }
    }

    fn decode_from_stream(
        stream: &IStream,
        generation: VideoThumbnailSourceGeneration,
        cancelled: &AtomicBool,
        deadline: Instant,
        limits: &VideoThumbnailLimits,
    ) -> Result<ValidatedVideoThumbnail, VideoThumbnailError> {
        let mut attributes = None;
        // SAFETY: all pointers are produced by windows-rs and remain live for
        // the duration of these checked COM calls.
        unsafe { MFCreateAttributes(&mut attributes, 2) }
            .map_err(|_| VideoThumbnailError::NativeBackendUnavailable)?;
        let attributes = attributes.ok_or(VideoThumbnailError::NativeBackendUnavailable)?;
        unsafe { attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1) }
            .map_err(|_| VideoThumbnailError::NativeBackendUnavailable)?;
        let byte_stream = unsafe { MFCreateMFByteStreamOnStreamEx(stream) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        let reader = unsafe { MFCreateSourceReaderFromByteStream(&byte_stream, &attributes) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        check_interruption(cancelled, deadline)?;

        let stream_index = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        unsafe { reader.SetStreamSelection(stream_index, true) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        let native_type = unsafe { reader.GetNativeMediaType(stream_index, 0) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        let native_size = unsafe { native_type.GetUINT64(&MF_MT_FRAME_SIZE) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        let (source_width, source_height) = unpack_dimensions(native_size);
        let (output_width, output_height) =
            bounded_video_thumbnail_dimensions(source_width, source_height, limits)?;

        let output_type: IMFMediaType =
            unsafe { MFCreateMediaType() }.map_err(|_| VideoThumbnailError::DecodeFailed)?;
        unsafe { output_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        unsafe { output_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        unsafe {
            output_type.SetUINT64(
                &MF_MT_FRAME_SIZE,
                (u64::from(output_width) << 32) | u64::from(output_height),
            )
        }
        .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;
        unsafe { reader.SetCurrentMediaType(stream_index, None, &output_type) }
            .map_err(|_| VideoThumbnailError::UnsupportedMedia)?;

        let sample = read_one_sample(&reader, stream_index, cancelled, deadline)?;
        let current_type = unsafe { reader.GetCurrentMediaType(stream_index) }
            .map_err(|_| VideoThumbnailError::InvalidFrame)?;
        let current_size = unsafe { current_type.GetUINT64(&MF_MT_FRAME_SIZE) }
            .map_err(|_| VideoThumbnailError::InvalidFrame)?;
        let (width, height) = unpack_dimensions(current_size);
        if width != output_width || height != output_height {
            return Err(VideoThumbnailError::InvalidFrame);
        }
        let media_buffer = unsafe { sample.ConvertToContiguousBuffer() }
            .map_err(|_| VideoThumbnailError::InvalidFrame)?;
        copy_2d_bgra(&media_buffer, generation, width, height, limits)
    }

    fn check_interruption(
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<(), VideoThumbnailError> {
        interruption_error(cancelled, deadline).map_or(Ok(()), Err)
    }

    fn unpack_dimensions(value: u64) -> (u32, u32) {
        ((value >> 32) as u32, value as u32)
    }

    fn read_one_sample(
        reader: &IMFSourceReader,
        stream_index: u32,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFSample, VideoThumbnailError> {
        for _ in 0..MAX_SAMPLE_READ_ATTEMPTS {
            check_interruption(cancelled, deadline)?;
            let mut flags = 0_u32;
            let mut sample = None;
            unsafe {
                reader.ReadSample(
                    stream_index,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
            }
            .map_err(|_| VideoThumbnailError::DecodeFailed)?;
            check_interruption(cancelled, deadline)?;
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err(VideoThumbnailError::DecodeFailed);
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Err(VideoThumbnailError::UnsupportedMedia);
            }
            if let Some(sample) = sample {
                return Ok(sample);
            }
        }
        Err(VideoThumbnailError::UnsupportedMedia)
    }

    fn copy_2d_bgra(
        buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
        generation: VideoThumbnailSourceGeneration,
        width: u32,
        height: u32,
        limits: &VideoThumbnailLimits,
    ) -> Result<ValidatedVideoThumbnail, VideoThumbnailError> {
        let buffer_2d: IMF2DBuffer2 = buffer
            .cast()
            .map_err(|_| VideoThumbnailError::InvalidFrame)?;
        let mut scanline = std::ptr::null_mut();
        let mut pitch = 0_i32;
        let mut buffer_start = std::ptr::null_mut();
        let mut buffer_length = 0_u32;
        unsafe {
            buffer_2d.Lock2DSize(
                MF2DBuffer_LockFlags_Read,
                &mut scanline,
                &mut pitch,
                &mut buffer_start,
                &mut buffer_length,
            )
        }
        .map_err(|_| VideoThumbnailError::InvalidFrame)?;
        let _unlock = Buffer2dUnlock(&buffer_2d);

        let row_bytes = usize::try_from(
            u64::from(width)
                .checked_mul(4)
                .ok_or(VideoThumbnailError::ResourceLimit)?,
        )
        .map_err(|_| VideoThumbnailError::ResourceLimit)?;
        let frame_bytes = row_bytes
            .checked_mul(height as usize)
            .ok_or(VideoThumbnailError::ResourceLimit)?;
        if frame_bytes > limits.max_output_bytes || scanline.is_null() || buffer_start.is_null() {
            return Err(VideoThumbnailError::ResourceLimit);
        }
        if (pitch.unsigned_abs() as usize) < row_bytes {
            return Err(VideoThumbnailError::InvalidFrame);
        }
        let start_address = buffer_start as usize;
        let end_address = start_address
            .checked_add(buffer_length as usize)
            .ok_or(VideoThumbnailError::InvalidFrame)?;
        let mut packed = Vec::with_capacity(frame_bytes);
        for row in 0..height as isize {
            let offset = (pitch as isize)
                .checked_mul(row)
                .ok_or(VideoThumbnailError::InvalidFrame)?;
            // SAFETY: the computed signed row address is checked against the
            // locked buffer's complete range before a slice is constructed.
            let row_pointer = scanline.wrapping_offset(offset);
            let row_address = row_pointer as usize;
            let row_end = row_address
                .checked_add(row_bytes)
                .ok_or(VideoThumbnailError::InvalidFrame)?;
            if row_address < start_address || row_end > end_address {
                return Err(VideoThumbnailError::InvalidFrame);
            }
            // SAFETY: Lock2DSize keeps this checked row range valid until guard drop.
            packed.extend_from_slice(unsafe {
                std::slice::from_raw_parts(row_pointer.cast_const(), row_bytes)
            });
        }
        validate_video_thumbnail_bgra(generation, width, height, row_bytes, &packed, limits)
    }

    struct Buffer2dUnlock<'a>(&'a IMF2DBuffer2);

    impl Drop for Buffer2dUnlock<'_> {
        fn drop(&mut self) {
            // SAFETY: this guard exists only after a successful Lock2DSize call.
            let _ = unsafe { self.0.Unlock2D() };
        }
    }
}
