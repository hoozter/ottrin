use crate::{
    IsolatedProcessFailure, IsolatedProcessInput, IsolatedProcessLauncher, IsolatedProcessRequest,
    LinuxBubblewrapIsolation,
};
use image::ImageFormat;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_EPS_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EPS_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EPS_PIXELS: u64 = 40_000_000;
const MAX_EPS_RENDER_DIMENSION: u32 = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpsVisualError {
    UnsupportedFormat,
    RendererUnavailable,
    InvalidSource,
    RenderFailed,
    InvalidOutput,
}

pub fn eps_preview_runtime_is_available() -> bool {
    detect_ghostscript().is_some()
        && Path::new("/usr/bin/bwrap").is_file()
        && Path::new("/usr/bin/systemd-run").is_file()
}

pub fn render_eps_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, EpsVisualError> {
    render_eps_content_preview_png_with_launcher(
        path,
        target_dim,
        &LinuxBubblewrapIsolation,
        &AtomicBool::new(false),
    )
}

fn render_eps_content_preview_png_with_launcher(
    path: &Path,
    target_dim: u32,
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, EpsVisualError> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("eps"))
    {
        return Err(EpsVisualError::UnsupportedFormat);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| EpsVisualError::InvalidSource)?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_EPS_INPUT_BYTES {
        return Err(EpsVisualError::InvalidSource);
    }
    let executable = detect_ghostscript().ok_or(EpsVisualError::RendererUnavailable)?;
    let root = private_render_root()?;
    let _cleanup = RenderDirectory(root.clone());
    let profile = root.join("profile");
    let output = root.join("output");
    std::fs::create_dir(&profile).map_err(|_| EpsVisualError::RenderFailed)?;
    std::fs::create_dir(&output).map_err(|_| EpsVisualError::RenderFailed)?;

    launcher
        .run(
            &IsolatedProcessRequest {
                executable,
                arguments: vec![
                    OsString::from("-dSAFER"),
                    OsString::from("-dBATCH"),
                    OsString::from("-dNOPAUSE"),
                    OsString::from("-dEPSCrop"),
                    OsString::from("-sDEVICE=pngalpha"),
                    OsString::from("-dTextAlphaBits=4"),
                    OsString::from("-dGraphicsAlphaBits=4"),
                    OsString::from("-r144"),
                    OsString::from("-sOutputFile=/output/page.png"),
                    OsString::from("/input/document.eps"),
                ],
                input: Some(IsolatedProcessInput {
                    host_path: path.to_path_buf(),
                    sandbox_path: PathBuf::from("/input/document.eps"),
                }),
                private_profile_dir: profile,
                private_output_dir: output.clone(),
                timeout: Duration::from_secs(10),
                max_memory_bytes: 384 * 1024 * 1024,
                max_processes: 8,
                max_output_bytes: MAX_EPS_OUTPUT_BYTES,
            },
            cancelled,
        )
        .map_err(map_process_failure)?;

    let png = std::fs::read(output.join("page.png")).map_err(|_| EpsVisualError::InvalidOutput)?;
    normalize_rendered_png(&png, target_dim)
}

fn detect_ghostscript() -> Option<PathBuf> {
    ["/usr/bin/gs", "/usr/local/bin/gs"]
        .into_iter()
        .map(Path::new)
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
}

fn private_render_root() -> Result<PathBuf, EpsVisualError> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| EpsVisualError::RenderFailed)?
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ottrin-eps-render-{}-{unique}", std::process::id()));
    std::fs::create_dir(&root).map_err(|_| EpsVisualError::RenderFailed)?;
    Ok(root)
}

fn normalize_rendered_png(bytes: &[u8], target_dim: u32) -> Result<Vec<u8>, EpsVisualError> {
    validate_png_dimensions(bytes)?;
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|_| EpsVisualError::InvalidOutput)?;
    if u64::from(image.width()) * u64::from(image.height()) > MAX_EPS_PIXELS {
        return Err(EpsVisualError::InvalidOutput);
    }
    let target_dim = target_dim.clamp(64, 4096);
    let thumbnail = image.thumbnail(target_dim, target_dim);
    let mut output = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| EpsVisualError::InvalidOutput)?;
    Ok(output.into_inner())
}

fn validate_png_dimensions(bytes: &[u8]) -> Result<(), EpsVisualError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err(EpsVisualError::InvalidOutput);
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0
        || height == 0
        || width > MAX_EPS_RENDER_DIMENSION
        || height > MAX_EPS_RENDER_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_EPS_PIXELS
    {
        return Err(EpsVisualError::InvalidOutput);
    }
    Ok(())
}

fn map_process_failure(failure: IsolatedProcessFailure) -> EpsVisualError {
    match failure {
        IsolatedProcessFailure::Unavailable | IsolatedProcessFailure::InvalidRequest => {
            EpsVisualError::RendererUnavailable
        }
        IsolatedProcessFailure::LimitExceeded
        | IsolatedProcessFailure::Timeout
        | IsolatedProcessFailure::Crash
        | IsolatedProcessFailure::Cancelled => EpsVisualError::RenderFailed,
    }
}

struct RenderDirectory(PathBuf);

impl Drop for RenderDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_png_is_bounded_to_the_requested_size() {
        let source = image::DynamicImage::new_rgba8(800, 400);
        let mut encoded = std::io::Cursor::new(Vec::new());
        source.write_to(&mut encoded, ImageFormat::Png).unwrap();

        let normalized = normalize_rendered_png(encoded.get_ref(), 200).unwrap();
        let decoded = image::load_from_memory(&normalized).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (200, 100));
    }

    #[test]
    fn invalid_renderer_output_is_rejected() {
        assert_eq!(
            normalize_rendered_png(b"not a png", 200),
            Err(EpsVisualError::InvalidOutput)
        );
    }

    #[test]
    fn oversized_renderer_dimensions_are_rejected_before_decode() {
        let mut png_header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png_header.extend_from_slice(&100_000u32.to_be_bytes());
        png_header.extend_from_slice(&100_000u32.to_be_bytes());
        assert_eq!(
            normalize_rendered_png(&png_header, 100),
            Err(EpsVisualError::InvalidOutput)
        );
    }
}
