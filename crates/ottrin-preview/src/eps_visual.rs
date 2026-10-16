#[cfg(target_os = "linux")]
use crate::LinuxBubblewrapIsolation;
#[cfg(windows)]
use crate::WindowsJobObjectIsolation;
use crate::{
    IsolatedProcessFailure, IsolatedProcessInput, IsolatedProcessLauncher, IsolatedProcessRequest,
};
use image::ImageFormat;
use std::ffi::OsString;
use std::io::Read;
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
    #[cfg(target_os = "linux")]
    let isolation =
        Path::new("/usr/bin/bwrap").is_file() && Path::new("/usr/bin/systemd-run").is_file();
    #[cfg(windows)]
    let isolation = true; // Native launch still fails closed if AppContainer setup fails.
    isolation && detect_ghostscript().is_some()
}

pub fn render_eps_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, EpsVisualError> {
    #[cfg(target_os = "linux")]
    let launcher = &LinuxBubblewrapIsolation;
    #[cfg(windows)]
    let launcher = &WindowsJobObjectIsolation;
    let executable = detect_ghostscript().ok_or(EpsVisualError::RendererUnavailable)?;
    render_eps_with_runtime(
        path,
        target_dim,
        executable,
        launcher,
        &AtomicBool::new(false),
    )
}

fn render_eps_with_runtime(
    path: &Path,
    target_dim: u32,
    executable: PathBuf,
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
    let mut source =
        crate::preview_source::Source::open(path).map_err(|_| EpsVisualError::InvalidSource)?;
    if source.len() > MAX_EPS_INPUT_BYTES {
        return Err(EpsVisualError::InvalidSource);
    }
    let root = private_render_root()?;
    let _cleanup = RenderDirectory(root.clone());
    let profile = root.join("profile");
    let output = root.join("output");
    std::fs::create_dir(&profile).map_err(|_| EpsVisualError::RenderFailed)?;
    std::fs::create_dir(&output).map_err(|_| EpsVisualError::RenderFailed)?;
    // The child receives a bounded, validated snapshot, never the mutable path
    // inspected above. Its launcher can only read this admitted input copy.
    let snapshot = root.join("document.eps");
    let mut copy = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot)
        .map_err(|_| EpsVisualError::RenderFailed)?;
    let copied = std::io::copy(
        &mut source.file.by_ref().take(MAX_EPS_INPUT_BYTES + 1),
        &mut copy,
    )
    .map_err(|_| EpsVisualError::InvalidSource)?;
    drop(copy);
    if copied != source.len() || copied > MAX_EPS_INPUT_BYTES {
        return Err(EpsVisualError::InvalidSource);
    }
    source
        .validate(path)
        .map_err(|_| EpsVisualError::InvalidSource)?;
    #[cfg(target_os = "linux")]
    let output_argument = OsString::from("-sOutputFile=/output/page.png");
    #[cfg(windows)]
    let output_argument = {
        // Ghostscript treats % as a page-number pattern, including in user
        // temp-directory names. Escape it for Ghostscript, not a shell.
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let mut wide = "-sOutputFile=".encode_utf16().collect::<Vec<_>>();
        for c in output.join("page.png").as_os_str().encode_wide() {
            wide.push(c);
            if c == 37 {
                wide.push(c);
            }
        }
        OsString::from_wide(&wide)
    };

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
                    OsString::from("-dFirstPage=1"),
                    OsString::from("-dLastPage=1"),
                    output_argument,
                    OsString::from("/input/document.eps"),
                ],
                input: Some(IsolatedProcessInput {
                    host_path: snapshot,
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

    let rendered_path = output.join("page.png");
    let mut rendered = crate::preview_source::Source::open(&rendered_path)
        .map_err(|_| EpsVisualError::InvalidOutput)?;
    if rendered.len() > MAX_EPS_OUTPUT_BYTES {
        return Err(EpsVisualError::InvalidOutput);
    }
    let mut png = Vec::new();
    rendered
        .file
        .by_ref()
        .take(MAX_EPS_OUTPUT_BYTES + 1)
        .read_to_end(&mut png)
        .map_err(|_| EpsVisualError::InvalidOutput)?;
    if png.len() as u64 != rendered.len() {
        return Err(EpsVisualError::InvalidOutput);
    }
    rendered
        .validate(&rendered_path)
        .map_err(|_| EpsVisualError::InvalidOutput)?;
    normalize_rendered_png(&png, target_dim)
}

#[cfg(target_os = "linux")]
fn detect_ghostscript() -> Option<PathBuf> {
    ["/usr/bin/gs", "/usr/local/bin/gs"]
        .into_iter()
        .map(Path::new)
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
}

#[cfg(windows)]
fn detect_ghostscript() -> Option<PathBuf> {
    let directories = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    windows_candidates(&directories, std::env::var_os("PATH").as_deref())
        .into_iter()
        .find_map(|path| {
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return None;
            }
            path.canonicalize().ok()
        })
}

#[cfg(windows)]
fn windows_candidates(
    directories: &[PathBuf],
    search_path: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for directory in directories.iter().filter(|p| p.is_absolute()) {
        if let Ok(entries) = std::fs::read_dir(directory.join("gs")) {
            let mut versions = entries
                .take(64)
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name();
                    let text = name.to_str()?.strip_prefix("gs")?;
                    let version = text
                        .split('.')
                        .map(str::parse::<u32>)
                        .collect::<Result<Vec<_>, _>>()
                        .ok()?;
                    if version.len() < 2 || !entry.file_type().ok()?.is_dir() {
                        return None;
                    }
                    Some((version, entry.path()))
                })
                .collect::<Vec<_>>();
            versions.sort_unstable_by(|a, b| b.0.cmp(&a.0));
            for (_, path) in versions {
                candidates.extend([path.join("bin/gswin64c.exe"), path.join("bin/gswin32c.exe")]);
            }
        }
    }
    if let Some(search_path) = search_path {
        for directory in std::env::split_paths(search_path)
            .filter(|p| p.is_absolute())
            .take(256)
        {
            candidates.extend([
                directory.join("gswin64c.exe"),
                directory.join("gswin32c.exe"),
            ]);
        }
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|path| seen.insert(path.clone()));
    candidates
}

fn private_render_root() -> Result<PathBuf, EpsVisualError> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| EpsVisualError::RenderFailed)?
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ottrin-eps-render-{}-{unique}", std::process::id()));
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder
        .create(&root)
        .map_err(|_| EpsVisualError::RenderFailed)?;
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
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "EPS private render cleanup failed for {}: {error}",
                self.0.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IsolatedProcessOutput;
    use std::sync::{Mutex, atomic::Ordering};

    struct Renderer {
        root: Mutex<Option<PathBuf>>,
        failure: Option<IsolatedProcessFailure>,
        oversized: bool,
    }
    impl Renderer {
        fn new() -> Self {
            Self {
                root: Mutex::new(None),
                failure: None,
                oversized: false,
            }
        }
    }
    impl IsolatedProcessLauncher for Renderer {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            let root = request.private_profile_dir.parent().unwrap().to_path_buf();
            *self.root.lock().unwrap() = Some(root.clone());
            assert_eq!(request.timeout, Duration::from_secs(10));
            assert_eq!(request.max_memory_bytes, 384 * 1024 * 1024);
            assert_eq!(request.max_processes, 8);
            assert_eq!(request.max_output_bytes, MAX_EPS_OUTPUT_BYTES);
            let input = request.input.as_ref().unwrap();
            assert_eq!(input.host_path, root.join("document.eps"));
            assert_eq!(std::fs::read(&input.host_path).unwrap(), b"%!PS fixture");
            assert_eq!(input.sandbox_path, Path::new("/input/document.eps"));
            for argument in [
                "-dSAFER",
                "-dBATCH",
                "-dNOPAUSE",
                "-dFirstPage=1",
                "-dLastPage=1",
            ] {
                assert!(request.arguments.contains(&OsString::from(argument)));
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(IsolatedProcessFailure::Cancelled);
            }
            if let Some(failure) = self.failure {
                return Err(failure);
            }
            let path = request.private_output_dir.join("page.png");
            if self.oversized {
                std::fs::File::create(path)
                    .unwrap()
                    .set_len(MAX_EPS_OUTPUT_BYTES + 1)
                    .unwrap();
            } else {
                image::DynamicImage::new_rgba8(200, 100)
                    .save_with_format(path, ImageFormat::Png)
                    .unwrap();
            }
            Ok(IsolatedProcessOutput {
                stdout: Vec::new(),
                diagnostics: Vec::new(),
            })
        }
    }

    fn with_fixture(test: impl FnOnce(&Path)) {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("original.eps");
        std::fs::write(&path, b"%!PS fixture").unwrap();
        test(&path);
        assert_eq!(std::fs::read(path).unwrap(), b"%!PS fixture");
    }

    #[test]
    fn renderer_gets_a_bounded_snapshot_and_cleans_up_after_success() {
        with_fixture(|path| {
            let renderer = Renderer::new();
            let png = render_eps_with_runtime(
                path,
                100,
                PathBuf::from("unused-runtime"),
                &renderer,
                &AtomicBool::new(false),
            )
            .unwrap();
            let decoded = image::load_from_memory(&png).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (100, 50));
            assert!(!renderer.root.lock().unwrap().as_ref().unwrap().exists());
        });
    }

    #[test]
    fn failures_and_cancellation_are_truthful_and_remove_private_data() {
        with_fixture(|path| {
            for failure in [
                IsolatedProcessFailure::Unavailable,
                IsolatedProcessFailure::InvalidRequest,
                IsolatedProcessFailure::LimitExceeded,
                IsolatedProcessFailure::Timeout,
                IsolatedProcessFailure::Crash,
                IsolatedProcessFailure::Cancelled,
            ] {
                let mut renderer = Renderer::new();
                renderer.failure = Some(failure);
                assert_eq!(
                    render_eps_with_runtime(
                        path,
                        100,
                        PathBuf::from("unused-runtime"),
                        &renderer,
                        &AtomicBool::new(false)
                    ),
                    Err(map_process_failure(failure))
                );
                assert!(!renderer.root.lock().unwrap().as_ref().unwrap().exists());
            }
            let renderer = Renderer::new();
            assert_eq!(
                render_eps_with_runtime(
                    path,
                    100,
                    PathBuf::from("unused-runtime"),
                    &renderer,
                    &AtomicBool::new(true)
                ),
                Err(EpsVisualError::RenderFailed)
            );
            assert!(!renderer.root.lock().unwrap().as_ref().unwrap().exists());
        });
    }

    #[test]
    fn oversized_source_is_rejected_before_launch_and_oversized_output_before_read() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("large.eps");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_EPS_INPUT_BYTES + 1)
            .unwrap();
        let renderer = Renderer::new();
        assert_eq!(
            render_eps_with_runtime(
                &path,
                100,
                PathBuf::from("unused-runtime"),
                &renderer,
                &AtomicBool::new(false)
            ),
            Err(EpsVisualError::InvalidSource)
        );
        assert!(renderer.root.lock().unwrap().is_none());
        with_fixture(|path| {
            let mut renderer = Renderer::new();
            renderer.oversized = true;
            assert_eq!(
                render_eps_with_runtime(
                    path,
                    100,
                    PathBuf::from("unused-runtime"),
                    &renderer,
                    &AtomicBool::new(false)
                ),
                Err(EpsVisualError::InvalidOutput)
            );
            assert!(!renderer.root.lock().unwrap().as_ref().unwrap().exists());
        });
    }

    #[test]
    fn final_symlink_source_never_reaches_the_renderer() {
        with_fixture(|path| {
            let link = path.with_file_name("link.eps");
            #[cfg(unix)]
            std::os::unix::fs::symlink(path, &link).unwrap();
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(path, &link).unwrap();
            let renderer = Renderer::new();
            assert_eq!(
                render_eps_with_runtime(
                    &link,
                    100,
                    PathBuf::from("unused-runtime"),
                    &renderer,
                    &AtomicBool::new(false)
                ),
                Err(EpsVisualError::InvalidSource)
            );
            assert!(renderer.root.lock().unwrap().is_none());
        });
    }

    #[cfg(windows)]
    #[test]
    fn windows_discovery_orders_versions_and_ignores_relative_path_and_nonversions() {
        let fixture = tempfile::tempdir().unwrap();
        for name in ["gs10.9.0", "gs10.10.0", "uninstall", "gsbad"] {
            std::fs::create_dir_all(fixture.path().join("gs").join(name)).unwrap();
        }
        let path = std::env::join_paths([Path::new("relative"), fixture.path()]).unwrap();
        let candidates = windows_candidates(&[fixture.path().to_path_buf()], Some(&path));
        assert_eq!(
            candidates[0],
            fixture.path().join("gs/gs10.10.0/bin/gswin64c.exe")
        );
        assert!(candidates.iter().all(|path| path.is_absolute()));
        assert_eq!(candidates.len(), 6);
    }

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
