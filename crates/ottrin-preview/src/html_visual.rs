use crate::{
    IsolatedProcessFailure, IsolatedProcessInput, IsolatedProcessLauncher, IsolatedProcessRequest,
    LinuxBubblewrapIsolation,
};
use image::GenericImageView;
use lol_html::{RewriteStrSettings, element, html_content::ContentType, rewrite_str};
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_HTML_INPUT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_HTML_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
const HTML_RENDER_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_HTML_RENDER_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
// Chromium's process tree also counts renderer threads against systemd TasksMax.
// Keep the ceiling finite while allowing a single headless page to initialize.
const MAX_HTML_RENDER_PROCESSES: u64 = 256;
const MAX_LOCAL_STYLESHEET_BYTES: u64 = 256 * 1024;
const BLOCKED_ELEMENTS: &str = "script,iframe,frame,object,embed,audio,video,source,track,svg";
const CONTENT_SECURITY_POLICY: &str = "<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src 'none'; form-action 'none'; base-uri 'none'\">";

#[derive(Debug, thiserror::Error)]
pub enum HtmlVisualError {
    #[error("HTML visual previews are available only for .html and .htm files")]
    UnsupportedFormat,
    #[error("HTML preview source is not a regular local file")]
    InputNotRegular,
    #[error("HTML preview source is too large ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("HTML preview source could not be read: {0}")]
    Read(String),
    #[error("HTML preview source is not UTF-8")]
    UnsafeText,
    #[error("no supported system HTML renderer and sandbox are installed")]
    RuntimeUnavailable,
    #[error("HTML preview could not be sanitized: {0}")]
    Sanitize(String),
    #[error("system HTML renderer failed")]
    RenderFailed,
    #[error("system HTML renderer exceeded its time limit")]
    Timeout,
    #[error("system HTML renderer returned an invalid image")]
    InvalidOutput,
}

struct RenderDirectory(PathBuf);

impl Drop for RenderDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn browser_path() -> Option<&'static Path> {
    [
        "/opt/google/chrome/chrome",
        "/usr/lib/chromium/chromium",
        "/usr/lib/chromium-browser/chromium-browser",
    ]
    .into_iter()
    .map(Path::new)
    .find(|path| path.is_file())
}

pub fn html_preview_runtime_available() -> bool {
    Path::new("/usr/bin/bwrap").is_file()
        && Path::new("/usr/bin/systemd-run").is_file()
        && Path::new("/usr/bin/systemctl").is_file()
        && browser_path().is_some()
}

fn is_html_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "html" | "htm"))
}

fn read_bounded_html(path: &Path) -> Result<String, HtmlVisualError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    if !metadata.is_file() {
        return Err(HtmlVisualError::InputNotRegular);
    }
    if metadata.len() > MAX_HTML_INPUT_BYTES {
        return Err(HtmlVisualError::InputTooLarge {
            actual: metadata.len(),
            limit: MAX_HTML_INPUT_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_HTML_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    if bytes.len() as u64 > MAX_HTML_INPUT_BYTES {
        return Err(HtmlVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_HTML_INPUT_BYTES,
        });
    }
    String::from_utf8(bytes).map_err(|_| HtmlVisualError::UnsafeText)
}

fn safe_offline_reference(value: &str, allow_data_image: bool) -> bool {
    let value = value.trim();
    if value.is_empty() || value.starts_with('#') {
        return true;
    }
    if !allow_data_image {
        return false;
    }
    let value = value.to_ascii_lowercase();
    [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/gif;base64,",
        "data:image/webp;base64,",
        "data:image/bmp;base64,",
        "data:image/x-icon;base64,",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn unsafe_inline_style(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    ["url(", "@import", "expression(", "behavior:"]
        .iter()
        .any(|token| value.contains(token))
}

fn read_local_stylesheet(source_path: &Path, reference: &str) -> Option<String> {
    let relative = Path::new(reference);
    if reference.is_empty()
        || reference.contains(['%', '\\', ':', '?', '#'])
        || relative.is_absolute()
        || !relative.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
        || !relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("css"))
    {
        return None;
    }

    let source_directory = source_path.parent()?.canonicalize().ok()?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(source_directory.join(relative))
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_LOCAL_STYLESHEET_BYTES {
        return None;
    }
    let opened_path = std::fs::canonicalize(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
    if !opened_path.starts_with(&source_directory) {
        return None;
    }

    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_LOCAL_STYLESHEET_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_LOCAL_STYLESHEET_BYTES {
        return None;
    }
    let stylesheet = String::from_utf8(bytes).ok()?;
    if stylesheet.contains(['<', '\0']) || unsafe_inline_style(&stylesheet) {
        return None;
    }
    Some(stylesheet)
}

fn inert_offline_document(source_path: &Path, source: &str) -> Result<String, HtmlVisualError> {
    let stylesheet_source = source_path.to_path_buf();
    let sanitized = rewrite_str(
        source,
        RewriteStrSettings::new()
            .append_element_content_handler(element!(BLOCKED_ELEMENTS, |element| {
                element.remove();
                Ok(())
            }))
            .append_element_content_handler(element!("base", |element| {
                element.remove();
                Ok(())
            }))
            .append_element_content_handler(element!("meta", |element| {
                if element
                    .get_attribute("http-equiv")
                    .is_some_and(|value| value.eq_ignore_ascii_case("refresh"))
                {
                    element.remove();
                }
                Ok(())
            }))
            .append_element_content_handler(element!("link", move |element| {
                let stylesheet = element
                    .get_attribute("rel")
                    .filter(|rel| {
                        rel.split_ascii_whitespace()
                            .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                    })
                    .and_then(|_| element.get_attribute("href"))
                    .and_then(|reference| read_local_stylesheet(&stylesheet_source, &reference));
                if let Some(stylesheet) = stylesheet {
                    element.replace(&format!("<style>{stylesheet}</style>"), ContentType::Html);
                } else {
                    element.remove();
                }
                Ok(())
            }))
            .append_element_content_handler(element!("*", |element| {
                let tag_name = element.tag_name();
                let blocked_attributes = element
                    .attributes()
                    .iter()
                    .map(|attribute| attribute.name().to_ascii_lowercase())
                    .filter(|name| {
                        name.starts_with("on")
                            || matches!(
                                name.as_str(),
                                "action"
                                    | "background"
                                    | "cite"
                                    | "data"
                                    | "formaction"
                                    | "manifest"
                                    | "ping"
                                    | "poster"
                                    | "srcset"
                                    | "xlink:href"
                            )
                            || (name == "style"
                                && element
                                    .get_attribute(name)
                                    .is_none_or(|value| unsafe_inline_style(&value)))
                            || (name == "src"
                                && element.get_attribute(name).is_none_or(|value| {
                                    tag_name != "img" || !safe_offline_reference(&value, true)
                                }))
                            || (name == "href"
                                && element.get_attribute(name).is_none_or(|value| {
                                    tag_name != "link" && !safe_offline_reference(&value, false)
                                }))
                    })
                    .collect::<Vec<_>>();
                for name in blocked_attributes {
                    element.remove_attribute(&name);
                }
                Ok(())
            })),
    )
    .map_err(|error| HtmlVisualError::Sanitize(error.to_string()))?;
    Ok(format!("{CONTENT_SECURITY_POLICY}{sanitized}"))
}

fn create_render_directory() -> Result<RenderDirectory, HtmlVisualError> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ottrin-html-preview-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&path).map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    std::fs::create_dir(path.join("profile"))
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    std::fs::create_dir(path.join("output"))
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    Ok(RenderDirectory(path))
}

pub fn render_html_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, HtmlVisualError> {
    render_html_content_preview_png_with_launcher(
        path,
        target_dim,
        &LinuxBubblewrapIsolation,
        &AtomicBool::new(false),
    )
}

fn render_html_content_preview_png_with_launcher(
    path: &Path,
    target_dim: u32,
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, HtmlVisualError> {
    if !is_html_path(path) {
        return Err(HtmlVisualError::UnsupportedFormat);
    }
    let browser = browser_path().ok_or(HtmlVisualError::RuntimeUnavailable)?;
    if !html_preview_runtime_available() {
        return Err(HtmlVisualError::RuntimeUnavailable);
    }
    render_html_content_preview_png_with_launcher_and_browser(
        path, target_dim, launcher, cancelled, browser,
    )
}

fn render_html_content_preview_png_with_launcher_and_browser(
    path: &Path,
    target_dim: u32,
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
    browser: &Path,
) -> Result<Vec<u8>, HtmlVisualError> {
    if !is_html_path(path) {
        return Err(HtmlVisualError::UnsupportedFormat);
    }
    let source = read_bounded_html(path)?;
    let document = inert_offline_document(path, &source)?;
    let render_dir = create_render_directory()?;
    let input = render_dir.0.join("input.html");
    let mut input_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&input)
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    input_file
        .write_all(document.as_bytes())
        .map_err(|error| HtmlVisualError::Read(error.to_string()))?;
    drop(input_file);

    let dimension = target_dim.clamp(128, 2_048);
    let output_directory = render_dir.0.join("output");
    launcher
        .run(
            &IsolatedProcessRequest {
                executable: browser.to_path_buf(),
                arguments: vec![
                    OsString::from("--headless=new"),
                    OsString::from("--disable-gpu"),
                    OsString::from("--disable-javascript"),
                    OsString::from("--disable-background-networking"),
                    OsString::from("--disable-component-update"),
                    OsString::from("--disable-default-apps"),
                    OsString::from("--disable-extensions"),
                    OsString::from("--disable-sync"),
                    OsString::from("--metrics-recording-only"),
                    OsString::from("--no-first-run"),
                    OsString::from("--no-default-browser-check"),
                    OsString::from("--user-data-dir=/profile"),
                    OsString::from("--screenshot=/output/output.png"),
                    OsString::from(format!("--window-size={dimension},{dimension}")),
                    OsString::from("file:///input/document.html"),
                ],
                input: Some(IsolatedProcessInput {
                    host_path: input,
                    sandbox_path: PathBuf::from("/input/document.html"),
                }),
                private_profile_dir: render_dir.0.join("profile"),
                private_output_dir: output_directory.clone(),
                timeout: HTML_RENDER_TIMEOUT,
                max_memory_bytes: MAX_HTML_RENDER_MEMORY_BYTES,
                max_processes: MAX_HTML_RENDER_PROCESSES,
                max_output_bytes: MAX_HTML_OUTPUT_BYTES,
            },
            cancelled,
        )
        .map_err(map_process_failure)?;

    let output = output_directory.join("output.png");
    let metadata = std::fs::metadata(&output).map_err(|_| HtmlVisualError::InvalidOutput)?;
    if metadata.len() == 0 || metadata.len() > MAX_HTML_OUTPUT_BYTES {
        return Err(HtmlVisualError::InvalidOutput);
    }
    let bytes = std::fs::read(output).map_err(|_| HtmlVisualError::InvalidOutput)?;
    let decoded = image::load_from_memory(&bytes).map_err(|_| HtmlVisualError::InvalidOutput)?;
    if decoded.dimensions() != (dimension, dimension) {
        return Err(HtmlVisualError::InvalidOutput);
    }
    Ok(bytes)
}

fn map_process_failure(failure: IsolatedProcessFailure) -> HtmlVisualError {
    match failure {
        IsolatedProcessFailure::Unavailable | IsolatedProcessFailure::InvalidRequest => {
            HtmlVisualError::RuntimeUnavailable
        }
        IsolatedProcessFailure::Timeout => HtmlVisualError::Timeout,
        IsolatedProcessFailure::LimitExceeded
        | IsolatedProcessFailure::Crash
        | IsolatedProcessFailure::Cancelled => HtmlVisualError::RenderFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct RecordingLauncher;

    impl crate::IsolatedProcessLauncher for RecordingLauncher {
        fn run(
            &self,
            request: &crate::IsolatedProcessRequest,
            _cancelled: &std::sync::atomic::AtomicBool,
        ) -> Result<crate::IsolatedProcessOutput, crate::IsolatedProcessFailure> {
            let input = request.input.as_ref().expect("sanitized HTML input");
            assert_eq!(input.sandbox_path, Path::new("/input/document.html"));
            let html = std::fs::read_to_string(&input.host_path).expect("read sanitized input");
            assert!(html.contains("Visible text"));
            assert!(!html.contains("<script"));
            assert!(request.arguments.iter().any(|argument| {
                argument
                    .to_str()
                    .is_some_and(|value| value == "file:///input/document.html")
            }));
            assert!(request.max_memory_bytes > 0);
            assert!(request.max_processes > 0);

            let image = image::DynamicImage::new_rgb8(256, 256);
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, image::ImageFormat::Png).unwrap();
            std::fs::write(
                request.private_output_dir.join("output.png"),
                png.into_inner(),
            )
            .unwrap();
            Ok(crate::IsolatedProcessOutput {
                stdout: Vec::new(),
                diagnostics: Vec::new(),
            })
        }
    }

    #[test]
    fn offline_references_reject_encoded_and_backslash_traversal() {
        for reference in [
            "../secret.png",
            "..\\secret.png",
            "%2e%2e/secret.png",
            "%2E%2E\\secret.png",
        ] {
            assert!(
                !safe_offline_reference(reference, true),
                "must reject {reference}"
            );
        }
    }

    #[test]
    fn data_images_are_limited_to_inert_raster_formats() {
        for reference in [
            "data:image/svg+xml,<svg onload=alert(1)></svg>",
            "data:image/svg+xml;base64,PHN2Zy8+",
            "data:image/html;base64,PGh0bWw+",
        ] {
            assert!(
                !safe_offline_reference(reference, true),
                "must reject {reference}"
            );
        }
        assert!(safe_offline_reference(
            "data:image/png;base64,iVBORw0KGgo=",
            true
        ));
    }

    #[test]
    fn sanitizer_removes_active_and_external_content() {
        let sanitized = inert_offline_document(
            Path::new("/tmp/page.html"),
            r#"<meta http-equiv="refresh" content="0;url=file:///etc/passwd">
<script>alert(1)</script>
<img src="data:image/svg+xml,<svg onload=alert(1)></svg>" onerror="alert(2)">
<p style="background:url(file:///etc/passwd)">Visible text</p>"#,
        )
        .unwrap();

        assert!(!sanitized.contains("<script"));
        assert!(!sanitized.contains("http-equiv=\"refresh\""));
        assert!(!sanitized.contains("onerror"));
        assert!(!sanitized.contains("data:image/svg+xml"));
        assert!(!sanitized.contains("file:///etc/passwd"));
        assert!(sanitized.contains("Visible text"));
    }

    #[test]
    fn renderer_submits_only_sanitized_html_to_the_bounded_isolation_boundary() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-html-visual-test-{}.html",
            std::process::id()
        ));
        std::fs::write(&path, "<script>alert(1)</script><p>Visible text</p>").unwrap();

        let result = render_html_content_preview_png_with_launcher_and_browser(
            &path,
            256,
            &RecordingLauncher,
            &std::sync::atomic::AtomicBool::new(false),
            Path::new("/test/chromium"),
        );

        std::fs::remove_file(path).unwrap();
        let png = result.expect("bounded HTML PNG");
        assert_eq!(
            image::load_from_memory(&png).unwrap().dimensions(),
            (256, 256)
        );
    }
}
