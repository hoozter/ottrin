use ottrin_core::{EntryKind, format_modified};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

mod pdf_visual;
pub use pdf_visual::{
    PdfVisualError, render_pdf_first_page_png, render_pdf_first_page_png_with_library,
    render_pdf_page_png, render_pdf_page_png_with_library,
};

const EXTRACTED_TEXT_MARKER: &str = "\n\nExtracted text:\n";
const EXTRACTED_TEXT_LIMIT: usize = 24_000;
const MAX_OFFICE_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_PARTS: usize = 4_096;
const MAX_OFFICE_XML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SHARED_STRINGS: usize = 4_096;
const OFFICE_VISUAL_TEXT_LIMIT: usize = 4_096;
const MAX_TEXT_VISUAL_INPUT_BYTES: u64 = 128 * 1024;
const TEXT_VISUAL_LINE_LIMIT: usize = 18;
const TEXT_VISUAL_CHAR_LIMIT: usize = 1_600;
const MAX_SVG_VISUAL_INPUT_BYTES: u64 = 256 * 1024;
const MAX_SVG_PREVIEW_DIMENSION: u32 = 2048;
const MAX_SVG_PREVIEW_PIXELS: u64 = 4_194_304;
const MAX_TAR_COMPRESSED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TAR_SCAN_ENTRIES: usize = 4_096;
const MAX_TAR_SCAN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TAR_PATH_BYTES: usize = 768;
const MAX_TAR_LISTED_ENTRIES: usize = 200;

/// Stops an archive stream at a fixed input budget.
struct LimitedReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> LimitedReader<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            remaining: limit,
        }
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(std::io::Error::other(
                "archive input exceeded safe preview bounds",
            ));
        }

        let readable = buffer.len().min(self.remaining as usize);
        let read = self.inner.read(&mut buffer[..readable])?;
        self.remaining -= read as u64;
        Ok(read)
    }
}

/// Validates extension headers before `tar` materializes their payloads.
struct TarHeaderGuard<R> {
    inner: R,
    pending_header: Option<[u8; 512]>,
    pending_offset: usize,
    entry_bytes_remaining: u64,
    pax_payload: Option<Vec<u8>>,
    pax_payload_remaining: u64,
    next_pax_size: Option<u64>,
    zero_headers: u8,
    after_terminator: bool,
}

fn padded_tar_entry_size(size: u64) -> std::io::Result<u64> {
    let padding = (512 - size % 512) % 512;
    size.checked_add(padding)
        .ok_or_else(|| std::io::Error::other("archive entry size exceeded safe preview bounds"))
}

fn pax_size_override(pax_payload: &[u8]) -> std::io::Result<Option<u64>> {
    let mut size_override = None;
    for record in pax_payload.split_inclusive(|byte| *byte == b'\n') {
        if !record.ends_with(b"\n") {
            return Err(std::io::Error::other("invalid PAX extension record"));
        }
        let Some(space) = record.iter().position(|byte| *byte == b' ') else {
            return Err(std::io::Error::other("invalid PAX extension record"));
        };
        std::str::from_utf8(&record[..space])
            .ok()
            .and_then(|length| length.parse::<usize>().ok())
            .filter(|length| *length == record.len())
            .ok_or_else(|| std::io::Error::other("invalid PAX extension record"))?;
        let key_value = &record[space + 1..record.len() - 1];
        let Some(equals) = key_value.iter().position(|byte| *byte == b'=') else {
            return Err(std::io::Error::other("invalid PAX extension record"));
        };
        if size_override.is_none() && key_value[..equals] == *b"size" {
            size_override = Some(
                std::str::from_utf8(&key_value[equals + 1..])
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| std::io::Error::other("invalid PAX size extension"))?,
            );
        }
    }
    Ok(size_override)
}

impl<R> TarHeaderGuard<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending_header: None,
            pending_offset: 0,
            entry_bytes_remaining: 0,
            pax_payload: None,
            pax_payload_remaining: 0,
            next_pax_size: None,
            zero_headers: 0,
            after_terminator: false,
        }
    }

    fn prepare_header(&mut self) -> std::io::Result<()>
    where
        R: Read,
    {
        let mut header_bytes = [0; 512];
        let first_byte_count = self.inner.read(&mut header_bytes[..1])?;
        if first_byte_count == 0 {
            return if self.zero_headers >= 2 {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "archive ended before its TAR terminator",
                ))
            };
        }
        self.inner.read_exact(&mut header_bytes[1..])?;

        if header_bytes.iter().all(|byte| *byte == 0) {
            self.zero_headers = self.zero_headers.saturating_add(1);
        } else {
            self.zero_headers = 0;
            let header = tar::Header::from_byte_slice(&header_bytes);
            let entry_type = header.entry_type();
            let extension = entry_type.is_gnu_longname()
                || entry_type.is_gnu_longlink()
                || entry_type.is_pax_global_extensions()
                || entry_type.is_pax_local_extensions();
            let entry_size = header.entry_size()?;
            if extension && entry_size > MAX_TAR_PATH_BYTES as u64 {
                return Err(std::io::Error::other(
                    "archive extension exceeded safe preview path bounds",
                ));
            }
            if entry_type.is_pax_local_extensions() {
                self.pax_payload = Some(Vec::with_capacity(entry_size as usize));
                self.pax_payload_remaining = entry_size;
            }
            let effective_size = if extension {
                entry_size
            } else {
                self.next_pax_size.take().unwrap_or(entry_size)
            };
            self.entry_bytes_remaining = padded_tar_entry_size(effective_size)?;
        }

        self.pending_header = Some(header_bytes);
        self.pending_offset = 0;
        Ok(())
    }
}

impl<R: Read> Read for TarHeaderGuard<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let mut written = 0;
        while written < buffer.len() {
            if let Some(header) = self.pending_header.as_ref() {
                let available = 512 - self.pending_offset;
                let copy_len = available.min(buffer.len() - written);
                buffer[written..written + copy_len]
                    .copy_from_slice(&header[self.pending_offset..self.pending_offset + copy_len]);
                self.pending_offset += copy_len;
                written += copy_len;
                if self.pending_offset == 512 {
                    self.pending_header = None;
                    self.pending_offset = 0;
                    if self.zero_headers >= 2 {
                        self.after_terminator = true;
                    }
                }
                continue;
            }

            if self.after_terminator {
                let read = self.inner.read(&mut buffer[written..])?;
                return Ok(written + read);
            }

            if self.entry_bytes_remaining > 0 {
                let remaining = usize::try_from(self.entry_bytes_remaining).unwrap_or(usize::MAX);
                let readable = (buffer.len() - written).min(remaining);
                let read = self.inner.read(&mut buffer[written..written + readable])?;
                if let Some(pax_payload) = self.pax_payload.as_mut() {
                    let capture = usize::try_from(self.pax_payload_remaining)
                        .unwrap_or(usize::MAX)
                        .min(read);
                    pax_payload.extend_from_slice(&buffer[written..written + capture]);
                    self.pax_payload_remaining -= capture as u64;
                }
                self.entry_bytes_remaining -= read as u64;
                written += read;
                if read == 0 {
                    return Ok(written);
                }
                if self.entry_bytes_remaining == 0
                    && let Some(pax_payload) = self.pax_payload.take()
                {
                    self.next_pax_size = pax_size_override(&pax_payload)?;
                    self.pax_payload_remaining = 0;
                }
                continue;
            }

            self.prepare_header()?;
            if self.pending_header.is_none() {
                return Ok(written);
            }
        }
        Ok(written)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OfficeVisualError {
    #[error("Office format has no bounded visual renderer")]
    UnsupportedFormat,
    #[error("could not inspect Office input: {0}")]
    InputMetadata(String),
    #[error("Office input is too large for a visual preview ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not open Office package: {0}")]
    Package(String),
    #[error("Office package has too many parts ({actual}; limit {limit})")]
    TooManyParts { actual: usize, limit: usize },
    #[error("Office document has no safe extractable content")]
    NoContent,
    #[error("could not encode Office visual preview: {0}")]
    Encode(String),
}

#[derive(Debug, thiserror::Error)]
pub enum TextVisualError {
    #[error("format has no generated text card")]
    UnsupportedFormat,
    #[error("could not inspect text input: {0}")]
    InputMetadata(String),
    #[error("text input is too large for a generated card ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read text input: {0}")]
    Read(String),
    #[error("text input is not safe UTF-8 text")]
    UnsafeText,
    #[error("could not encode generated text card: {0}")]
    Encode(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SvgVisualError {
    #[error("format is not SVG")]
    UnsupportedFormat,
    #[error("could not inspect SVG input: {0}")]
    InputMetadata(String),
    #[error("SVG input is too large for a visual preview ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("SVG contains blocked references (file/http/https image references are not permitted)")]
    BlockedReference,
    #[error("SVG could not be parsed for visual preview: {0}")]
    Parse(String),
    #[error("SVG visual preview could not be rasterized: {0}")]
    Rasterize(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewKind {
    Text,
    Image,
    FolderMetadata,
    Pdf,
    VideoMetadata,
    AudioMetadata,
    ArchiveMetadata,
    OfficeMetadata,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub kind_hint: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewData {
    pub title: String,
    pub kind: PreviewKind,
    pub body: String,
    pub open_external: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewView {
    Metadata,
    TextCard,
    Visual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewVisualRenderer {
    None,
    Image,
    Pdf,
    Office,
    TextCard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewProfile {
    kind: PreviewKind,
    views: [bool; 3],
    visual_renderer: PreviewVisualRenderer,
    raw_fallback_allowed: bool,
}

impl PreviewProfile {
    pub const fn kind(&self) -> PreviewKind {
        self.kind
    }

    pub const fn supports_view(&self, view: PreviewView) -> bool {
        match view {
            PreviewView::Metadata => self.views[0],
            PreviewView::TextCard => self.views[1],
            PreviewView::Visual => self.views[2],
        }
    }

    pub const fn visual_renderer(&self) -> PreviewVisualRenderer {
        self.visual_renderer
    }

    pub const fn raw_fallback_allowed(&self) -> bool {
        self.raw_fallback_allowed
    }

    pub const fn supports_thumbnail_visual(&self) -> bool {
        self.supports_view(PreviewView::TextCard) || self.supports_view(PreviewView::Visual)
    }
}

const fn profile_from_kind(kind: PreviewKind) -> PreviewProfile {
    match kind {
        PreviewKind::Text => PreviewProfile {
            kind,
            views: [true, true, true],
            visual_renderer: PreviewVisualRenderer::TextCard,
            raw_fallback_allowed: false,
        },
        PreviewKind::Image => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Image,
            raw_fallback_allowed: true,
        },
        PreviewKind::Pdf => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Pdf,
            raw_fallback_allowed: false,
        },
        PreviewKind::VideoMetadata => PreviewProfile {
            kind,
            views: [true, false, false],
            visual_renderer: PreviewVisualRenderer::None,
            raw_fallback_allowed: true,
        },
        PreviewKind::AudioMetadata => PreviewProfile {
            kind,
            views: [true, false, false],
            visual_renderer: PreviewVisualRenderer::None,
            raw_fallback_allowed: true,
        },
        PreviewKind::ArchiveMetadata => PreviewProfile {
            kind,
            views: [true, false, false],
            visual_renderer: PreviewVisualRenderer::None,
            raw_fallback_allowed: true,
        },
        PreviewKind::OfficeMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Office,
            raw_fallback_allowed: false,
        },
        PreviewKind::FolderMetadata => PreviewProfile {
            kind,
            views: [true, false, false],
            visual_renderer: PreviewVisualRenderer::None,
            raw_fallback_allowed: false,
        },
        PreviewKind::Unsupported => PreviewProfile {
            kind,
            views: [false, false, false],
            visual_renderer: PreviewVisualRenderer::None,
            raw_fallback_allowed: true,
        },
    }
}

pub fn preview_profile(path: &Path) -> PreviewProfile {
    profile_from_kind(classify_path(path))
}

pub fn load_preview(request: &PreviewRequest) -> PreviewData {
    let title = request
        .path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Preview")
        .to_string();

    if request.kind_hint == EntryKind::Directory {
        return preview_folder(title, &request.path);
    }

    match classify_path(&request.path) {
        PreviewKind::Text => preview_text(title, &request.path),
        PreviewKind::Image => preview_image(title, &request.path),
        PreviewKind::Pdf => preview_pdf(title, &request.path),
        PreviewKind::VideoMetadata => preview_video(title, &request.path),
        PreviewKind::AudioMetadata => preview_audio(title, &request.path),
        PreviewKind::ArchiveMetadata => preview_archive(title, &request.path),
        PreviewKind::OfficeMetadata => preview_office(title, &request.path),
        PreviewKind::FolderMetadata => preview_folder(title, &request.path),
        PreviewKind::Unsupported => preview_fallback(title, &request.path),
    }
}

pub fn classify_path(path: &std::path::Path) -> PreviewKind {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml" | "yml"
        | "ini" | "cfg" | "conf" | "env" | "xml" | "html" | "htm" | "css" | "scss" | "less"
        | "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "rb" | "go" | "java" | "c" | "cpp" | "h"
        | "hpp" | "m" | "mm" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "vcf" | "ics"
        | "ical" | "ldif" | "sql" | "rtf" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "svg" => {
            PreviewKind::Image
        }
        "pdf" => PreviewKind::Pdf,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => PreviewKind::VideoMetadata,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => PreviewKind::AudioMetadata,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" => PreviewKind::ArchiveMetadata,
        "doc" | "docx" | "xls" | "xlsx" | "xlsm" | "ppt" | "pptx" | "odt" | "ods" | "odp" => {
            PreviewKind::OfficeMetadata
        }
        _ => PreviewKind::Unsupported,
    }
}

/// Renders bounded extracted DOCX/XLSX/XLSM/ODS content as a PNG. This is a
/// content preview, not a faithful Office layout engine; unsupported or
/// malformed packages fail closed so callers retain the metadata/text view.
pub fn render_office_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, OfficeVisualError> {
    let ext = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "docx" | "xlsx" | "xlsm" | "ods") {
        return Err(OfficeVisualError::UnsupportedFormat);
    }
    let input_size = std::fs::metadata(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    let file =
        std::fs::File::open(path).map_err(|error| OfficeVisualError::Package(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| OfficeVisualError::Package(error.to_string()))?;
    if archive.len() > MAX_OFFICE_PARTS {
        return Err(OfficeVisualError::TooManyParts {
            actual: archive.len(),
            limit: MAX_OFFICE_PARTS,
        });
    }
    let text = match ext.as_str() {
        "docx" => extract_docx_text(&mut archive),
        "xlsx" | "xlsm" => extract_xlsx_text(&mut archive),
        "ods" => extract_ods_text(&mut archive),
        _ => None,
    }
    .ok_or(OfficeVisualError::NoContent)?;
    render_office_content_png(&text, &ext, target_dim)
}

/// Renders a bounded, syntax-neutral card for text/source files. The input is
/// deliberately constrained before decoding so thumbnail generation never
/// treats arbitrary bytes as an image or consumes an unbounded document.
pub fn render_text_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, TextVisualError> {
    if classify_path(path) != PreviewKind::Text {
        return Err(TextVisualError::UnsupportedFormat);
    }
    let input_size = std::fs::metadata(path)
        .map_err(|error| TextVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_TEXT_VISUAL_INPUT_BYTES {
        return Err(TextVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_TEXT_VISUAL_INPUT_BYTES,
        });
    }
    let bytes = std::fs::read(path).map_err(|error| TextVisualError::Read(error.to_string()))?;
    let source = std::str::from_utf8(&bytes).map_err(|_| TextVisualError::UnsafeText)?;
    if source.contains('\0')
        || source
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(TextVisualError::UnsafeText);
    }

    use image::{ImageEncoder, Rgba, RgbaImage};

    let dimension = target_dim.clamp(64, 2_048);
    let mut image = RgbaImage::from_pixel(dimension, dimension, Rgba([248, 249, 251, 255]));
    let margin = (dimension / 11).clamp(6, 96);
    let header_bottom = margin + (dimension / 5).clamp(24, 160);
    fill_rect(
        &mut image,
        0,
        0,
        dimension,
        header_bottom,
        [218, 228, 242, 255],
    );
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Text file");
    let kind = text_kind_label(path);
    let leading = source
        .lines()
        .take(TEXT_VISUAL_LINE_LIMIT)
        .collect::<Vec<_>>()
        .join("\n");
    let leading: String = leading.chars().take(TEXT_VISUAL_CHAR_LIMIT).collect();
    let header = format!("{filename}\n{kind}");
    render_document_text(
        &mut image,
        &header,
        margin,
        margin,
        dimension - margin,
        header_bottom.saturating_sub(margin / 3),
    );
    render_document_text(
        &mut image,
        &leading,
        margin,
        header_bottom + margin / 2,
        dimension - margin,
        dimension.saturating_sub(margin),
    );
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| TextVisualError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn render_office_content_png(
    text: &str,
    format: &str,
    target_dim: u32,
) -> Result<Vec<u8>, OfficeVisualError> {
    use image::{ImageEncoder, Rgba, RgbaImage};

    let text = text
        .chars()
        .take(OFFICE_VISUAL_TEXT_LIMIT)
        .collect::<String>();
    let dimension = target_dim.clamp(64, 2_048);
    let width = if format == "docx" {
        (dimension as f32 * 8.5 / 11.0).round() as u32
    } else {
        dimension
    };
    let height = dimension;
    let mut image = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    let margin = (width / 9).clamp(6, 96);
    if format == "docx" {
        render_document_text(
            &mut image,
            &text,
            margin,
            margin,
            width - margin,
            height.saturating_sub(margin),
        );
    } else {
        let header_height = (dimension / 9).clamp(12, 96);
        fill_rect(
            &mut image,
            margin,
            margin,
            dimension - margin,
            margin + header_height,
            [42, 92, 154, 255],
        );
        let body_top = margin + header_height + (margin / 2);
        render_sheet_grid(
            &mut image,
            &text,
            margin,
            body_top,
            dimension - margin,
            dimension.saturating_sub(margin),
        );
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| OfficeVisualError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn fill_rect(
    image: &mut image::RgbaImage,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    color: [u8; 4],
) {
    for y in top.min(image.height())..bottom.min(image.height()) {
        for x in left.min(image.width())..right.min(image.width()) {
            image.put_pixel(x, y, image::Rgba(color));
        }
    }
}

fn render_document_text(
    image: &mut image::RgbaImage,
    text: &str,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) {
    use ab_glyph::{Font, FontArc, Glyph, PxScale, ScaleFont, point};

    let Ok(font) = FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT) else {
        return;
    };
    let font_size = (image.width() as f32 / 54.0).clamp(7.0, 30.0);
    let scale = PxScale::from(font_size);
    let scaled = font.as_scaled(scale);
    let line_height = (scaled.height() * 1.35).max(font_size + 2.0);
    let max_width = right.saturating_sub(left) as f32;
    let mut x = left as f32;
    let mut baseline = top as f32 + scaled.ascent();
    let mut previous = None;

    for ch in text.chars() {
        if ch == '\n' {
            x = left as f32;
            baseline += line_height;
            previous = None;
            if baseline > bottom as f32 {
                break;
            }
            continue;
        }
        let id = scaled.glyph_id(ch);
        let kern = previous.map_or(0.0, |prior| scaled.kern(prior, id));
        let advance = scaled.h_advance(id);
        if x + kern + advance > left as f32 + max_width && x > left as f32 {
            x = left as f32;
            baseline += line_height;
            if baseline > bottom as f32 {
                break;
            }
        }
        x += kern;
        let glyph = Glyph {
            id,
            scale,
            position: point(x, baseline),
        };
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x.floor() as i32 + gx as i32;
                let py = bounds.min.y.floor() as i32 + gy as i32;
                if px >= 0 && py >= 0 && (px as u32) < image.width() && (py as u32) < image.height()
                {
                    let shade = (255.0 * (1.0 - coverage * 0.88)).round() as u8;
                    image.put_pixel(
                        px as u32,
                        py as u32,
                        image::Rgba([shade, shade, shade, 255]),
                    );
                }
            });
        }
        x += advance;
        previous = Some(id);
    }
}

fn render_sheet_grid(
    image: &mut image::RgbaImage,
    text: &str,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) {
    let rows = text
        .lines()
        .filter(|line| !line.starts_with("Sheet:"))
        .take(12)
        .collect::<Vec<_>>();
    let row_count = rows.len().max(1) as u32;
    let max_columns = rows
        .iter()
        .map(|line| line.split('\t').count())
        .max()
        .unwrap_or(1)
        .clamp(1, 8) as u32;
    let row_height = ((bottom.saturating_sub(top)) / (row_count + 1)).clamp(4, 48);
    let column_width = ((right - left) / max_columns).max(4);
    for row in 0..=row_count {
        fill_rect(
            image,
            left,
            top + row * row_height,
            right,
            top + row * row_height + 1,
            [174, 181, 190, 255],
        );
    }
    for column in 0..=max_columns {
        fill_rect(
            image,
            left + column * column_width,
            top,
            left + column * column_width + 1,
            top + row_count * row_height,
            [174, 181, 190, 255],
        );
    }
    for (row, line) in rows.iter().enumerate() {
        for (column, value) in line.split('\t').take(max_columns as usize).enumerate() {
            let inset = (row_height / 5).max(1);
            let width = ((value.chars().take(24).count() as u32 * (column_width - 2 * inset) / 24)
                .max(2))
            .min(column_width - 2 * inset);
            fill_rect(
                image,
                left + column as u32 * column_width + inset,
                top + row as u32 * row_height + inset,
                left + column as u32 * column_width + inset + width,
                top + row as u32 * row_height + inset + (row_height / 4).max(1),
                [82, 86, 92, 255],
            );
        }
    }
}

fn metadata_summary(path: &Path, kind_label: &str, extra: Option<&str>) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mut lines = vec![
                format!("Filetype: {}", kind_label.trim_end_matches('.')),
                format!("Size: {}", format_size(meta.len())),
            ];
            if let Ok(modified) = meta.modified()
                && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
            }
            if let Some(ext) = path
                .extension()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
            {
                lines.push(format!("Format: {}", ext.to_ascii_uppercase()));
            }
            if let Some(extra) = extra.filter(|s| !s.is_empty()) {
                lines.push(extra.to_string());
            }
            lines.join("\n")
        }
        Err(_) => format!("{kind_label}\nMetadata unavailable."),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn preview_text(title: String, path: &Path) -> PreviewData {
    const LIMIT: usize = 128 * 1024;
    match std::fs::read(path) {
        Ok(bytes) => {
            let head = &bytes[..bytes.len().min(LIMIT)];
            match std::str::from_utf8(head) {
                Ok(s) => {
                    let line_count = s.lines().count();
                    let mut lines = vec![
                        format!("Filetype: {}", text_kind_label(path)),
                        "Encoding: UTF-8".to_string(),
                        format!("Lines: {line_count}"),
                    ];
                    if let Ok(meta) = std::fs::metadata(path) {
                        lines.push(format!("Size: {}", format_size(meta.len())));
                        if let Ok(modified) = meta.modified()
                            && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
                        {
                            lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
                        }
                    }
                    let text = if bytes.len() > LIMIT {
                        format!("{s}\n\n...(text preview truncated)")
                    } else {
                        s.to_string()
                    };
                    PreviewData {
                        title,
                        kind: PreviewKind::Text,
                        body: format!("{}{}{}", lines.join("\n"), EXTRACTED_TEXT_MARKER, text),
                        open_external: false,
                    }
                }
                Err(_) => PreviewData {
                    title,
                    kind: PreviewKind::Unsupported,
                    body: format!(
                        "{}\nFiletype: {}\nEncoding: Not valid UTF-8\n{}",
                        metadata_summary(path, &file_kind_label(path), None),
                        file_kind_label(path),
                        "Text preview unavailable."
                    ),
                    open_external: true,
                },
            }
        }
        Err(err) => PreviewData {
            title,
            kind: PreviewKind::Unsupported,
            body: format!("Failed to read file: {err}"),
            open_external: true,
        },
    }
}

fn preview_image(title: String, path: &Path) -> PreviewData {
    let dimensions = image::ImageReader::open(path)
        .ok()
        .and_then(|reader| reader.with_guessed_format().ok())
        .and_then(|reader| reader.into_dimensions().ok())
        .map(|(w, h)| format!("{w}x{h}"));
    let image_format = path_format(path).unwrap_or_else(|| "Image".to_string());
    let mut lines = vec![format!(
        "Filetype: {} image",
        image_format.to_ascii_uppercase()
    )];
    if let Some(dimensions) = dimensions {
        lines.push(format!("Resolution: {dimensions}"));
    }
    if let Ok(meta) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(meta.len())));
        if let Ok(modified) = meta.modified()
            && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
        }
    }
    PreviewData {
        title,
        kind: PreviewKind::Image,
        body: lines.join("\n"),
        open_external: true,
    }
}

pub fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svg"))
        .unwrap_or(false)
}

/// Generates a bounded PNG preview for SVG files with reference-safe parsing.
pub fn render_svg_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, SvgVisualError> {
    if !is_svg_path(path) {
        return Err(SvgVisualError::UnsupportedFormat);
    }
    let input_size = std::fs::metadata(path)
        .map_err(|error| SvgVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_SVG_VISUAL_INPUT_BYTES {
        return Err(SvgVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_SVG_VISUAL_INPUT_BYTES,
        });
    }

    let input =
        std::fs::read(path).map_err(|error| SvgVisualError::InputMetadata(error.to_string()))?;
    if has_disallowed_svg_external_references(&input) {
        return Err(SvgVisualError::BlockedReference);
    }
    let options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(&input, &options)
        .map_err(|error| SvgVisualError::Parse(error.to_string()))?;

    let source = tree.size();
    let preview_dimension = target_dim.clamp(1, MAX_SVG_PREVIEW_DIMENSION);
    let preview_dimension_f32 = preview_dimension as f32;
    let scale = ((preview_dimension_f32 / source.width())
        .min(preview_dimension_f32 / source.height()))
    .min(1.0);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(SvgVisualError::Rasterize(
            "SVG has invalid dimensions".to_string(),
        ));
    }
    let width = (source.width() * scale)
        .round()
        .clamp(1.0, preview_dimension_f32) as u32;
    let height = (source.height() * scale)
        .round()
        .clamp(1.0, preview_dimension_f32) as u32;
    if u64::from(width) * u64::from(height) > MAX_SVG_PREVIEW_PIXELS {
        return Err(SvgVisualError::Rasterize(
            "SVG rasterization exceeded configured pixel cap".to_string(),
        ));
    }

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        SvgVisualError::Rasterize("Could not allocate SVG render surface".to_string())
    })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let mut bytes = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            pixmap.data(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error: image::ImageError| SvgVisualError::Rasterize(error.to_string()))?;
    Ok(bytes)
}

fn has_disallowed_svg_external_references(data: &[u8]) -> bool {
    let Ok(svg) = std::str::from_utf8(data) else {
        return false;
    };
    let lower = svg.to_ascii_lowercase();
    const BLOCKED_ATTRS: [&str; 12] = [
        "href=\"file://",
        "href='file://",
        "xlink:href=\"file://",
        "xlink:href='file://",
        "href=\"http://",
        "href='http://",
        "xlink:href=\"http://",
        "xlink:href='http://",
        "href=\"https://",
        "href='https://",
        "xlink:href=\"https://",
        "xlink:href='https://",
    ];
    lower.contains("<script") || BLOCKED_ATTRS.iter().any(|pattern| lower.contains(pattern))
}

fn preview_folder(title: String, path: &Path) -> PreviewData {
    const MAX_SAMPLES: usize = 18;
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut files = 0usize;
            let mut folders = 0usize;
            let mut symlinks = 0usize;
            let mut other = 0usize;
            let mut direct_file_bytes = 0u64;
            let mut samples = Vec::new();

            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if samples.len() < MAX_SAMPLES {
                    samples.push(name);
                }
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => folders += 1,
                    Ok(kind) if kind.is_file() => {
                        files += 1;
                        if let Ok(meta) = entry.metadata() {
                            direct_file_bytes = direct_file_bytes.saturating_add(meta.len());
                        }
                    }
                    Ok(kind) if kind.is_symlink() => symlinks += 1,
                    Ok(_) => other += 1,
                    Err(_) => other += 1,
                }
            }

            let total = files + folders + symlinks + other;
            let mut lines = vec![
                "Filetype: Folder".to_string(),
                format!("Items: {total}"),
                format!("Folders: {folders}"),
                format!("Files: {files}"),
                format!("Known direct file size: {}", format_size(direct_file_bytes)),
            ];
            if symlinks > 0 {
                lines.push(format!("Symlinks: {symlinks}"));
            }
            if other > 0 {
                lines.push(format!("Other: {other}"));
            }
            if let Ok(meta) = std::fs::metadata(path)
                && let Ok(modified) = meta.modified()
                && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
            }
            if !samples.is_empty() {
                lines.push(String::new());
                lines.push("Sample:".to_string());
                lines.extend(samples.into_iter().map(|name| format!("  {name}")));
            }

            PreviewData {
                title,
                kind: PreviewKind::FolderMetadata,
                body: lines.join("\n"),
                open_external: false,
            }
        }
        Err(err) => PreviewData {
            title,
            kind: PreviewKind::FolderMetadata,
            body: format!("Folder metadata unavailable: {err}"),
            open_external: false,
        },
    }
}

// ── PDF preview via lopdf ──────────────────────────────────────────────

fn preview_pdf(title: String, path: &Path) -> PreviewData {
    match lopdf::Document::load(path) {
        Ok(doc) => {
            let page_count = doc.get_pages().len();

            let mut lines = vec![
                "Filetype: PDF document".to_string(),
                format!("Pages: {page_count}"),
            ];

            // Extract document info (title, author, subject, creator)
            if let Ok(info_ref) = doc.trailer.get(b"Info")
                && let Ok(lopdf::Object::Dictionary(dict)) =
                    doc.get_object(info_ref.as_reference().unwrap_or_default())
            {
                for (key_name, label) in [
                    ("Title", "Title"),
                    ("Author", "Author"),
                    ("Subject", "Subject"),
                    ("Creator", "Creator"),
                ] {
                    if let Ok(val) = dict.get(key_name.as_bytes())
                        && let Some(s) = pdf_obj_to_string(val)
                        && !s.trim().is_empty()
                    {
                        lines.push(format!("{label}: {s}"));
                    }
                }
            }

            // PDF version
            lines.push(format!("PDF version: {}", doc.version));

            // File size
            if let Ok(meta) = std::fs::metadata(path) {
                lines.push(format!("Size: {}", format_size(meta.len())));
                if let Ok(modified) = meta.modified()
                    && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
                }
            }
            let extracted_text = extract_pdf_text(&doc);
            let body = if let Some(text) = extracted_text {
                format!(
                    "{}{EXTRACTED_TEXT_MARKER}{}",
                    lines.join("\n"),
                    limit_preview_text(&text)
                )
            } else {
                lines.join("\n")
            };

            PreviewData {
                title,
                kind: PreviewKind::Pdf,
                body,
                open_external: true,
            }
        }
        Err(_) => {
            let info = metadata_summary(
                path,
                "PDF document.",
                Some("Could not parse PDF structure."),
            );
            PreviewData {
                title,
                kind: PreviewKind::Pdf,
                body: info,
                open_external: true,
            }
        }
    }
}

fn pdf_obj_to_string(obj: &lopdf::Object) -> Option<String> {
    match obj {
        lopdf::Object::String(bytes, _) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

fn extract_pdf_text(doc: &lopdf::Document) -> Option<String> {
    let pages: Vec<u32> = doc.get_pages().keys().copied().take(12).collect();
    if pages.is_empty() {
        return None;
    }
    let text = doc.extract_text(&pages).ok()?;
    normalized_preview_text(&text)
}

// ── Audio preview via lofty ────────────────────────────────────────────

fn preview_audio(title: String, path: &Path) -> PreviewData {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::Accessor;

    match lofty::read_from_path(path) {
        Ok(tagged_file) => {
            let mut lines = Vec::new();

            // Duration and bitrate from properties
            let dur = tagged_file.properties().duration();
            let total_secs = dur.as_secs();
            if total_secs > 0 {
                let mins = total_secs / 60;
                let secs = total_secs % 60;
                lines.push(format!("Duration: {mins}:{secs:02}"));
            }

            if let Some(br) = tagged_file.properties().audio_bitrate() {
                lines.push(format!("Bitrate: {br} kbps"));
            }

            if let Some(sr) = tagged_file.properties().sample_rate() {
                lines.push(format!("Sample rate: {sr} Hz"));
            }

            if let Some(ch) = tagged_file.properties().channels() {
                lines.push(format!("Channels: {ch}"));
            }

            // Tags
            if let Some(tag) = tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag())
            {
                if let Some(t) = tag.title() {
                    lines.push(format!("Title: {t}"));
                }
                if let Some(a) = tag.artist() {
                    lines.push(format!("Artist: {a}"));
                }
                if let Some(a) = tag.album() {
                    lines.push(format!("Album: {a}"));
                }
                if let Some(y) = tag.year() {
                    lines.push(format!("Year: {y}"));
                }
                if let Some(g) = tag.genre() {
                    lines.push(format!("Genre: {g}"));
                }
                if let Some(t) = tag.track() {
                    lines.push(format!("Track: {t}"));
                }
            }

            lines.insert(0, "Filetype: Audio file".to_string());
            if let Some(format) = path_format(path) {
                lines.insert(1, format!("Format: {}", format.to_ascii_uppercase()));
            }

            // File size
            if let Ok(meta) = std::fs::metadata(path) {
                lines.push(format!("Size: {}", format_size(meta.len())));
            }

            if lines.is_empty() {
                lines.push("Audio file — no metadata available.".to_string());
            }

            PreviewData {
                title,
                kind: PreviewKind::AudioMetadata,
                body: lines.join("\n"),
                open_external: true,
            }
        }
        Err(_) => {
            let info = metadata_summary(path, "Audio file.", Some("Could not read audio tags."));
            PreviewData {
                title,
                kind: PreviewKind::AudioMetadata,
                body: info,
                open_external: true,
            }
        }
    }
}

// ── Video preview (metadata-only, no heavy deps) ───────────────────────

fn preview_video(title: String, path: &Path) -> PreviewData {
    let mut lines = vec!["Filetype: Video file".to_string()];
    if let Some(format) = path_format(path) {
        lines.push(format!("Format: {}", format.to_ascii_uppercase()));
    }
    if let Ok(meta) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(meta.len())));
        if let Ok(modified) = meta.modified()
            && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
        }
    }
    lines.push("Duration/codec: unavailable without opening in an external player.".to_string());
    PreviewData {
        title,
        kind: PreviewKind::VideoMetadata,
        body: lines.join("\n"),
        open_external: true,
    }
}

// ── Archive preview via zip crate ──────────────────────────────────────

fn preview_archive(title: String, path: &Path) -> PreviewData {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if ext == "zip" {
        return preview_zip(title, path);
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".tar.gz"))
    {
        return preview_tar_gz(title, path);
    }
    if ext == "tar" {
        return preview_tar(title, path);
    }

    // For non-zip archives, show metadata only
    let info = metadata_summary(
        path,
        &format!("{} archive.", ext.to_ascii_uppercase()),
        None,
    );
    PreviewData {
        title,
        kind: PreviewKind::ArchiveMetadata,
        body: info,
        open_external: true,
    }
}

fn preview_tar(title: String, path: &Path) -> PreviewData {
    let compressed_len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if compressed_len > MAX_TAR_COMPRESSED_BYTES {
        let info = metadata_summary(
            path,
            "TAR archive.",
            Some("Archive is too large for safe metadata scanning."),
        );
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            open_external: true,
        };
    }

    let Ok(file) = std::fs::File::open(path) else {
        let info = metadata_summary(path, "TAR archive.", Some("Could not open archive."));
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            open_external: true,
        };
    };

    preview_tar_reader(
        title,
        path,
        TarHeaderGuard::new(LimitedReader::new(file, MAX_TAR_COMPRESSED_BYTES)),
        "TAR archive",
    )
}

fn preview_tar_gz(title: String, path: &Path) -> PreviewData {
    let compressed_len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if compressed_len > MAX_TAR_COMPRESSED_BYTES {
        let info = metadata_summary(
            path,
            "TAR.GZ archive.",
            Some("Archive is too large for safe metadata scanning."),
        );
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            open_external: true,
        };
    }
    let Ok(file) = std::fs::File::open(path) else {
        let info = metadata_summary(path, "TAR.GZ archive.", Some("Could not open archive."));
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            open_external: true,
        };
    };
    preview_tar_reader(
        title,
        path,
        TarHeaderGuard::new(LimitedReader::new(
            flate2::read::MultiGzDecoder::new(LimitedReader::new(file, MAX_TAR_COMPRESSED_BYTES)),
            MAX_TAR_SCAN_BYTES,
        )),
        "TAR.GZ archive",
    )
}

fn preview_tar_reader<R: std::io::Read>(
    title: String,
    path: &Path,
    reader: R,
    archive_label: &str,
) -> PreviewData {
    let archive_description = format!("{archive_label}.");
    let mut archive = tar::Archive::new(reader);
    let mut total = 0usize;
    let mut total_uncompressed: u64 = 0;
    let mut dirs = 0usize;
    let mut files = 0usize;
    let mut listing = Vec::with_capacity(MAX_TAR_LISTED_ENTRIES);

    {
        let mut entries_iter = match archive.entries() {
            Ok(entries) => entries,
            Err(_) => {
                let info = metadata_summary(
                    path,
                    &archive_description,
                    Some("Could not read archive entries."),
                );
                return PreviewData {
                    title,
                    kind: PreviewKind::ArchiveMetadata,
                    body: info,
                    open_external: true,
                };
            }
        };

        for result in entries_iter.by_ref() {
            total = total.saturating_add(1);
            if total > MAX_TAR_SCAN_ENTRIES {
                let info = metadata_summary(
                    path,
                    &archive_description,
                    Some("Archive has too many entries for safe scanning."),
                );
                return PreviewData {
                    title,
                    kind: PreviewKind::ArchiveMetadata,
                    body: info,
                    open_external: true,
                };
            }

            let entry = match result {
                Ok(entry) => entry,
                Err(_) => {
                    let info = metadata_summary(
                        path,
                        &archive_description,
                        Some("Could not read archive entries."),
                    );
                    return PreviewData {
                        title,
                        kind: PreviewKind::ArchiveMetadata,
                        body: info,
                        open_external: true,
                    };
                }
            };
            let entry_path = match entry.path() {
                Ok(entry_path) => {
                    let candidate = entry_path.to_string_lossy().into_owned();
                    if candidate.len() > MAX_TAR_PATH_BYTES {
                        let info = metadata_summary(
                            path,
                            &archive_description,
                            Some("Archive entry path exceeded safe preview bounds."),
                        );
                        return PreviewData {
                            title,
                            kind: PreviewKind::ArchiveMetadata,
                            body: info,
                            open_external: true,
                        };
                    }
                    candidate
                }
                Err(_) => {
                    let info = metadata_summary(
                        path,
                        &archive_description,
                        Some("Could not read archive entry paths."),
                    );
                    return PreviewData {
                        title,
                        kind: PreviewKind::ArchiveMetadata,
                        body: info,
                        open_external: true,
                    };
                }
            };

            let is_dir = entry.header().entry_type().is_dir();
            let size = entry.size();
            if is_dir {
                dirs += 1;
                if listing.len() < MAX_TAR_LISTED_ENTRIES {
                    listing.push(format!("  {entry_path}"));
                }
            } else {
                files += 1;
                total_uncompressed = total_uncompressed.saturating_add(size);
                if total_uncompressed > MAX_TAR_SCAN_BYTES {
                    let info = metadata_summary(
                        path,
                        &archive_description,
                        Some("Archive has too much data for safe preview scanning."),
                    );
                    return PreviewData {
                        title,
                        kind: PreviewKind::ArchiveMetadata,
                        body: info,
                        open_external: true,
                    };
                }
                if listing.len() < MAX_TAR_LISTED_ENTRIES {
                    listing.push(format!("  {entry_path}  ({})", format_size(size)));
                }
            }
        }
    }

    let mut remaining_input = archive.into_inner();
    let mut trailing_bytes = [0; 8_192];
    loop {
        let read = match remaining_input.read(&mut trailing_bytes) {
            Ok(read) => read,
            Err(_) => {
                let info = metadata_summary(
                    path,
                    &archive_description,
                    Some("Could not read archive entries."),
                );
                return PreviewData {
                    title,
                    kind: PreviewKind::ArchiveMetadata,
                    body: info,
                    open_external: true,
                };
            }
        };
        if read == 0 {
            break;
        }
        if trailing_bytes[..read].iter().any(|byte| *byte != 0) {
            let info = metadata_summary(
                path,
                &archive_description,
                Some("Could not read archive entries."),
            );
            return PreviewData {
                title,
                kind: PreviewKind::ArchiveMetadata,
                body: info,
                open_external: true,
            };
        }
    }

    let mut lines = vec![format!("Filetype: {archive_label}")];
    lines.push(format!("Entries: {total}"));
    lines.push(format!("Files: {files}"));
    lines.push(format!("Folders: {dirs}"));
    lines.push(format!("Uncompressed: {}", format_size(total_uncompressed)));

    if let Ok(meta) = std::fs::metadata(path) {
        lines.push(format!("Compressed: {}", format_size(meta.len())));
        if total_uncompressed > 0 {
            let ratio = (meta.len() as f64 / total_uncompressed as f64) * 100.0;
            lines.push(format!("Ratio: {ratio:.0}%"));
        }
    }

    lines.push(String::new());
    lines.push("Contents:".to_string());
    lines.extend(listing);
    if total > MAX_TAR_LISTED_ENTRIES {
        lines.push(format!(
            "  ... and {} more entries",
            total - MAX_TAR_LISTED_ENTRIES
        ));
    }

    PreviewData {
        title,
        kind: PreviewKind::ArchiveMetadata,
        body: lines.join("\n"),
        open_external: true,
    }
}

fn preview_zip(title: String, path: &Path) -> PreviewData {
    const MAX_ENTRIES: usize = 200;

    match std::fs::File::open(path)
        .and_then(|f| zip::ZipArchive::new(f).map_err(std::io::Error::other))
    {
        Ok(mut archive) => {
            let total = archive.len();
            let mut total_uncompressed: u64 = 0;
            let mut dirs = 0usize;
            let mut files = 0usize;
            let mut listing = Vec::with_capacity(total.min(MAX_ENTRIES));

            for i in 0..total {
                if let Ok(entry) = archive.by_index_raw(i) {
                    let name = entry.name().to_string();
                    let size = entry.size();
                    total_uncompressed += size;

                    if entry.is_dir() {
                        dirs += 1;
                    } else {
                        files += 1;
                    }

                    if listing.len() < MAX_ENTRIES {
                        if entry.is_dir() {
                            listing.push(format!("  {name}"));
                        } else {
                            listing.push(format!("  {name}  ({})", format_size(size)));
                        }
                    }
                }
            }

            let mut lines = vec![
                "Filetype: ZIP archive".to_string(),
                format!("Files: {files}"),
                format!("Folders: {dirs}"),
                format!("Uncompressed: {}", format_size(total_uncompressed)),
            ];

            if let Ok(meta) = std::fs::metadata(path) {
                lines.push(format!("Compressed: {}", format_size(meta.len())));
                if total_uncompressed > 0 {
                    let ratio = (meta.len() as f64 / total_uncompressed as f64) * 100.0;
                    lines.push(format!("Ratio: {ratio:.0}%"));
                }
            }

            lines.push(String::new());
            lines.push("Contents:".to_string());
            lines.extend(listing);
            if total > MAX_ENTRIES {
                lines.push(format!("  ... and {} more entries", total - MAX_ENTRIES));
            }

            PreviewData {
                title,
                kind: PreviewKind::ArchiveMetadata,
                body: lines.join("\n"),
                open_external: true,
            }
        }
        Err(_) => {
            let info = metadata_summary(
                path,
                "ZIP archive.",
                Some("Could not read archive contents."),
            );
            PreviewData {
                title,
                kind: PreviewKind::ArchiveMetadata,
                body: info,
                open_external: true,
            }
        }
    }
}

// ── Office preview (metadata-only) ─────────────────────────────────────

fn preview_office(title: String, path: &Path) -> PreviewData {
    // OOXML files (.docx, .xlsx, .pptx) are ZIP archives — try to read basic info
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_office_package = matches!(ext.as_str(), "docx" | "xlsx" | "xlsm" | "pptx" | "ods");

    let package_is_bounded = std::fs::metadata(path)
        .map(|metadata| metadata.len() <= MAX_OFFICE_PACKAGE_BYTES)
        .unwrap_or(false);
    if is_office_package
        && package_is_bounded
        && let Ok(file) = std::fs::File::open(path)
        && let Ok(mut archive) = zip::ZipArchive::new(file)
        && archive.len() <= MAX_OFFICE_PARTS
    {
        let mut lines = vec![
            format!("Filetype: {} document", ext.to_ascii_uppercase()),
            format!("Format: {}", ext.to_ascii_uppercase()),
        ];
        let total_parts = archive.len();
        lines.push(format!("Internal parts: {total_parts}"));

        if let Ok(meta) = std::fs::metadata(path) {
            lines.push(format!("Size: {}", format_size(meta.len())));
            if let Ok(modified) = meta.modified()
                && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
            }
        }
        let extracted_text = match ext.as_str() {
            "docx" => extract_docx_text(&mut archive),
            "xlsx" | "xlsm" => extract_xlsx_text(&mut archive),
            "pptx" => extract_pptx_text(&mut archive),
            "ods" => extract_ods_text(&mut archive),
            _ => None,
        };
        let body = if let Some(text) = extracted_text {
            format!(
                "{}{EXTRACTED_TEXT_MARKER}{}",
                lines.join("\n"),
                limit_preview_text(&text)
            )
        } else {
            lines.join("\n")
        };

        return PreviewData {
            title,
            kind: PreviewKind::OfficeMetadata,
            body,
            open_external: true,
        };
    }

    let info = metadata_summary(
        path,
        &format!("{} document.", ext.to_ascii_uppercase()),
        None,
    );
    PreviewData {
        title,
        kind: PreviewKind::OfficeMetadata,
        body: info,
        open_external: true,
    }
}

fn extract_docx_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let xml = zip_entry_text(archive, "word/document.xml")?;

    let mut reader = quick_xml::Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Text(event)) => {
                if let Ok(chunk) = event.decode() {
                    text.push_str(&chunk);
                }
            }
            Ok(quick_xml::events::Event::Empty(event))
            | Ok(quick_xml::events::Event::Start(event)) => match event.name().as_ref() {
                b"w:tab" => text.push('\t'),
                b"w:br" => text.push('\n'),
                _ => {}
            },
            Ok(quick_xml::events::Event::End(event)) => {
                if event.name().as_ref() == b"w:tc" {
                    text.push('\t');
                } else if matches!(event.name().as_ref(), b"w:p" | b"w:tr") {
                    text.push('\n');
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    normalized_preview_text(&text)
}

fn zip_entry_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > MAX_OFFICE_XML_BYTES {
        return None;
    }
    let mut limited = entry.take(MAX_OFFICE_XML_BYTES + 1);
    let mut xml = String::new();
    limited.read_to_string(&mut xml).ok()?;
    (xml.len() as u64 <= MAX_OFFICE_XML_BYTES).then_some(xml)
}

fn extract_tag_text(xml: &str, tag: &[u8]) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut inside = false;
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == tag => {
                inside = true;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == tag => {
                inside = false;
                text.push('\n');
            }
            Ok(quick_xml::events::Event::Text(event)) if inside => {
                if let Ok(chunk) = event.decode() {
                    text.push_str(&chunk);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    normalized_preview_text(&text)
}

fn extract_pptx_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let mut slides = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index_raw(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .collect::<Vec<_>>();
    slides.sort_by_key(|name| {
        name.trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    let mut out = String::new();
    for (index, name) in slides.into_iter().take(24).enumerate() {
        let Some(xml) = zip_entry_text(archive, &name) else {
            continue;
        };
        let Some(text) = extract_tag_text(&xml, b"a:t") else {
            continue;
        };
        let prefix = if out.is_empty() { "" } else { "\n\n" };
        if !append_bounded_preview_text(&mut out, &format!("{prefix}Slide {}\n{text}", index + 1)) {
            break;
        }
    }
    normalized_preview_text(&out)
}

const ODS_TABLE_NAMESPACE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const ODS_TEXT_NAMESPACE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:text:1.0";

fn ods_table_attribute(
    reader: &quick_xml::NsReader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
    local_name: &[u8],
) -> Result<Option<String>, ()> {
    let mut found = None;
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|_| ())?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if matches!(
            namespace,
            quick_xml::name::ResolveResult::Bound(namespace)
                if namespace.as_ref() == ODS_TABLE_NAMESPACE
        ) && local.as_ref() == local_name
        {
            if found.is_some() {
                return Err(());
            }
            found = Some(attribute.unescape_value().map_err(|_| ())?.into_owned());
        }
    }
    Ok(found)
}

fn ods_repeat_value(
    reader: &quick_xml::NsReader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
    local_name: &[u8],
) -> Option<usize> {
    let Some(value) = ods_table_attribute(reader, event, local_name).ok()? else {
        return Some(1);
    };
    let parsed = value.parse::<usize>().ok()?;
    (parsed > 0).then_some(parsed)
}

fn append_ods_cell_chunk(
    cell: &mut String,
    characters: &mut usize,
    saturated: &mut bool,
    chunk: &str,
) {
    if *saturated {
        return;
    }
    let remaining = EXTRACTED_TEXT_LIMIT.saturating_sub(*characters);
    let mut chars = chunk.chars();
    for character in chars.by_ref().take(remaining) {
        cell.push(character);
        *characters += 1;
    }
    if *characters >= EXTRACTED_TEXT_LIMIT || chars.next().is_some() {
        *saturated = true;
    }
}

fn ods_attributes_are_well_formed(
    reader: &quick_xml::NsReader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
) -> bool {
    event.attributes().all(|attribute| {
        let Ok(attribute) = attribute else {
            return false;
        };
        let (namespace, _) = reader.resolver().resolve_attribute(attribute.key);
        !matches!(namespace, quick_xml::name::ResolveResult::Unknown(_))
            && attribute.unescape_value().is_ok()
    })
}

fn extract_ods_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let xml = zip_entry_text(archive, "content.xml")?;
    let mut reader = quick_xml::NsReader::from_str(&xml);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_comments = true;

    let mut out = String::new();
    let mut current_cell = String::new();
    let mut row_cells = Vec::new();
    let mut in_cell = false;
    let mut sheet_count = 0usize;
    let mut row_count = 0usize;
    let mut active_sheet = false;
    let mut row_repeat = 1usize;
    let mut cell_repeat = 1usize;
    let mut cell_characters = 0usize;
    let mut cell_saturated = false;
    let mut output_saturated = false;

    loop {
        let (namespace, event) = match reader.read_resolved_event() {
            Ok(event) => event,
            Err(_) => return None,
        };
        let is_element = matches!(
            &event,
            quick_xml::events::Event::Start(_)
                | quick_xml::events::Event::Empty(_)
                | quick_xml::events::Event::End(_)
        );
        if is_element
            && matches!(
                namespace,
                quick_xml::name::ResolveResult::Unknown(_)
                    | quick_xml::name::ResolveResult::Unbound
            )
        {
            return None;
        }
        let table_namespace = matches!(
            namespace,
            quick_xml::name::ResolveResult::Bound(namespace)
                if namespace.as_ref() == ODS_TABLE_NAMESPACE
        );
        let text_namespace = matches!(
            namespace,
            quick_xml::name::ResolveResult::Bound(namespace)
                if namespace.as_ref() == ODS_TEXT_NAMESPACE
        );

        match event {
            quick_xml::events::Event::Start(event) => {
                if !ods_attributes_are_well_formed(&reader, &event) {
                    return None;
                }
                let local = event.local_name();
                let repeat_value = match (table_namespace, local.as_ref()) {
                    (true, b"table-row") => {
                        ods_repeat_value(&reader, &event, b"number-rows-repeated")?
                    }
                    (true, b"table-cell" | b"covered-table-cell") => {
                        ods_repeat_value(&reader, &event, b"number-columns-repeated")?
                    }
                    _ => 1,
                };
                match (table_namespace, text_namespace, local.as_ref()) {
                    (true, _, b"table") => {
                        sheet_count += 1;
                        active_sheet = sheet_count <= 12;
                        if active_sheet {
                            row_count = 0;
                            let name = ods_table_attribute(&reader, &event, b"name")
                                .ok()?
                                .unwrap_or_else(|| format!("Sheet {sheet_count}"));
                            let prefix = if out.is_empty() { "" } else { "\n\n" };
                            if !output_saturated
                                && !append_bounded_preview_text(
                                    &mut out,
                                    &format!("{prefix}Sheet: {name}\n"),
                                )
                            {
                                output_saturated = true;
                            }
                        }
                    }
                    (true, _, b"table-row") if active_sheet && row_count < 256 => {
                        row_cells.clear();
                        row_repeat = repeat_value.min(256 - row_count);
                    }
                    (true, _, b"table-cell" | b"covered-table-cell")
                        if active_sheet && row_count < 256 && row_cells.len() < 64 =>
                    {
                        in_cell = true;
                        current_cell.clear();
                        cell_characters = 0;
                        cell_saturated = false;
                        cell_repeat = repeat_value.min(64 - row_cells.len());
                    }
                    (_, true, b"line-break") if in_cell => {
                        append_ods_cell_chunk(
                            &mut current_cell,
                            &mut cell_characters,
                            &mut cell_saturated,
                            "\n",
                        );
                    }
                    (_, true, b"tab") if in_cell => {
                        append_ods_cell_chunk(
                            &mut current_cell,
                            &mut cell_characters,
                            &mut cell_saturated,
                            "\t",
                        );
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Empty(event) => {
                if !ods_attributes_are_well_formed(&reader, &event) {
                    return None;
                }
                let local = event.local_name();
                let repeat_value = match (table_namespace, local.as_ref()) {
                    (true, b"table-row") => {
                        ods_repeat_value(&reader, &event, b"number-rows-repeated")?
                    }
                    (true, b"table-cell" | b"covered-table-cell") => {
                        ods_repeat_value(&reader, &event, b"number-columns-repeated")?
                    }
                    _ => 1,
                };
                match (table_namespace, text_namespace, local.as_ref()) {
                    (true, _, b"table") => {
                        sheet_count += 1;
                        if sheet_count <= 12 {
                            let name = ods_table_attribute(&reader, &event, b"name")
                                .ok()?
                                .unwrap_or_else(|| format!("Sheet {sheet_count}"));
                            let prefix = if out.is_empty() { "" } else { "\n\n" };
                            if !output_saturated
                                && !append_bounded_preview_text(
                                    &mut out,
                                    &format!("{prefix}Sheet: {name}\n"),
                                )
                            {
                                output_saturated = true;
                            }
                        }
                    }
                    (true, _, b"table-row") if active_sheet && row_count < 256 => {
                        let repeat = repeat_value.min(256 - row_count);
                        for _ in 0..repeat {
                            row_count += 1;
                            if !output_saturated && !append_bounded_preview_text(&mut out, "\n") {
                                output_saturated = true;
                            }
                        }
                        row_cells.clear();
                    }
                    (true, _, b"table-cell" | b"covered-table-cell")
                        if active_sheet && row_count < 256 && row_cells.len() < 64 =>
                    {
                        let repeat = repeat_value.min(64 - row_cells.len());
                        row_cells.extend(std::iter::repeat_n(String::new(), repeat));
                    }
                    (_, true, b"line-break") if in_cell => {
                        append_ods_cell_chunk(
                            &mut current_cell,
                            &mut cell_characters,
                            &mut cell_saturated,
                            "\n",
                        );
                    }
                    (_, true, b"tab") if in_cell => {
                        append_ods_cell_chunk(
                            &mut current_cell,
                            &mut cell_characters,
                            &mut cell_saturated,
                            "\t",
                        );
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Text(event) => {
                let decoded = event.xml_content().ok()?;
                let chunk = quick_xml::escape::unescape(&decoded).ok()?;
                if in_cell {
                    append_ods_cell_chunk(
                        &mut current_cell,
                        &mut cell_characters,
                        &mut cell_saturated,
                        &chunk,
                    );
                }
            }
            quick_xml::events::Event::CData(event) => {
                let chunk = event.decode().ok()?;
                if in_cell {
                    append_ods_cell_chunk(
                        &mut current_cell,
                        &mut cell_characters,
                        &mut cell_saturated,
                        &chunk,
                    );
                }
            }
            quick_xml::events::Event::GeneralRef(event) => {
                let reference = event.decode().ok()?;
                let encoded = format!("&{reference};");
                let chunk = quick_xml::escape::unescape(&encoded).ok()?;
                if in_cell {
                    append_ods_cell_chunk(
                        &mut current_cell,
                        &mut cell_characters,
                        &mut cell_saturated,
                        &chunk,
                    );
                }
            }
            quick_xml::events::Event::DocType(_) => return None,
            quick_xml::events::Event::End(event) => {
                let local = event.local_name();
                match (table_namespace, local.as_ref()) {
                    (true, b"table-cell" | b"covered-table-cell") if in_cell => {
                        let value = current_cell.trim().to_string();
                        row_cells.extend(std::iter::repeat_n(value, cell_repeat));
                        current_cell.clear();
                        in_cell = false;
                        cell_repeat = 1;
                        cell_characters = 0;
                        cell_saturated = false;
                    }
                    (true, b"table-row") if active_sheet && row_count < 256 => {
                        let row = format!("{}\n", row_cells.join("\t"));
                        for _ in 0..row_repeat {
                            if row_count >= 256 {
                                break;
                            }
                            row_count += 1;
                            if !output_saturated && !append_bounded_preview_text(&mut out, &row) {
                                output_saturated = true;
                            }
                        }
                        row_cells.clear();
                        row_repeat = 1;
                    }
                    (true, b"table") => {
                        active_sheet = false;
                        in_cell = false;
                        current_cell.clear();
                        cell_characters = 0;
                        cell_saturated = false;
                        row_cells.clear();
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }

    normalized_preview_text(&out)
}

fn extract_xlsx_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let shared_strings = zip_entry_text(archive, "xl/sharedStrings.xml")
        .and_then(|xml| extract_xlsx_shared_strings(&xml))
        .unwrap_or_default();
    let sheets = xlsx_sheet_entries(archive);

    let mut out = String::new();
    for (label, name) in sheets.into_iter().take(12) {
        let Some(xml) = zip_entry_text(archive, &name) else {
            continue;
        };
        let Some(table) = extract_xlsx_sheet(&xml, &shared_strings) else {
            continue;
        };
        let prefix = if out.is_empty() { "" } else { "\n\n" };
        if !append_bounded_preview_text(&mut out, &format!("{prefix}Sheet: {label}\n{table}")) {
            break;
        }
    }
    normalized_preview_text(&out)
}

fn xlsx_sheet_entries<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Vec<(String, String)> {
    let workbook = zip_entry_text(archive, "xl/workbook.xml");
    let relationships = zip_entry_text(archive, "xl/_rels/workbook.xml.rels");
    if let (Some(workbook), Some(relationships)) = (workbook, relationships) {
        let mut targets = std::collections::HashMap::new();
        let mut reader = quick_xml::Reader::from_str(&relationships);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Empty(event))
                | Ok(quick_xml::events::Event::Start(event))
                    if event.name().as_ref() == b"Relationship" =>
                {
                    let mut id = None;
                    let mut target = None;
                    for attribute in event.attributes().flatten() {
                        match attribute.key.as_ref() {
                            b"Id" => id = attribute.unescape_value().ok().map(|v| v.into_owned()),
                            b"Target" => {
                                target = attribute.unescape_value().ok().map(|v| v.into_owned())
                            }
                            _ => {}
                        }
                    }
                    if let (Some(id), Some(target)) = (id, target) {
                        let target = target.trim_start_matches('/');
                        let path = if target.starts_with("xl/") {
                            target.to_string()
                        } else {
                            format!("xl/{target}")
                        };
                        targets.insert(id, path);
                    }
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                _ => {}
            }
        }

        let mut sheets = Vec::new();
        let mut reader = quick_xml::Reader::from_str(&workbook);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Empty(event))
                | Ok(quick_xml::events::Event::Start(event))
                    if event.name().as_ref() == b"sheet" =>
                {
                    let mut name = None;
                    let mut relationship = None;
                    for attribute in event.attributes().flatten() {
                        match attribute.key.as_ref() {
                            b"name" => {
                                name = attribute.unescape_value().ok().map(|v| v.into_owned())
                            }
                            b"r:id" => {
                                relationship =
                                    attribute.unescape_value().ok().map(|v| v.into_owned())
                            }
                            _ => {}
                        }
                    }
                    if let (Some(name), Some(path)) =
                        (name, relationship.and_then(|id| targets.get(&id).cloned()))
                    {
                        sheets.push((name, path));
                    }
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        if !sheets.is_empty() {
            return sheets;
        }
    }

    let mut fallback = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index_raw(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .filter(|name| name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        .collect::<Vec<_>>();
    fallback.sort_by_key(|name| {
        name.trim_start_matches("xl/worksheets/sheet")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    fallback
        .into_iter()
        .enumerate()
        .map(|(index, path)| ((index + 1).to_string(), path))
        .collect()
}

fn extract_xlsx_shared_strings(xml: &str) -> Option<Vec<String>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_text = false;
    let mut stored_characters = 0usize;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"t" => {
                in_text = true;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"t" => {
                in_text = false;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"si" => {
                stored_characters += current.chars().count();
                strings.push(std::mem::take(&mut current));
                if strings.len() >= MAX_SHARED_STRINGS || stored_characters >= EXTRACTED_TEXT_LIMIT
                {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Text(event)) if in_text => {
                let chunk = event.decode().ok()?;
                let remaining = EXTRACTED_TEXT_LIMIT
                    .saturating_sub(stored_characters + current.chars().count());
                current.extend(chunk.chars().take(remaining));
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(strings)
}

fn extract_xlsx_sheet(xml: &str, shared_strings: &[String]) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = String::new();
    let mut cell_type = None::<String>;
    let mut value = String::new();
    let mut collecting = false;
    let mut row_cells = Vec::<String>::new();
    let mut cell_column = None;
    let mut rows = 0usize;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"row" => {
                row_cells.clear();
                rows += 1;
            }
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"c" => {
                cell_type = None;
                cell_column = None;
                for attribute in event.attributes().flatten() {
                    match attribute.key.as_ref() {
                        b"t" => {
                            cell_type = String::from_utf8(attribute.value.into_owned()).ok();
                        }
                        b"r" => {
                            cell_column = std::str::from_utf8(attribute.value.as_ref())
                                .ok()
                                .and_then(xlsx_column_index);
                        }
                        _ => {}
                    }
                }
                value.clear();
            }
            Ok(quick_xml::events::Event::Start(event))
                if matches!(event.name().as_ref(), b"v" | b"t") =>
            {
                collecting = true;
            }
            Ok(quick_xml::events::Event::End(event))
                if matches!(event.name().as_ref(), b"v" | b"t") =>
            {
                collecting = false;
            }
            Ok(quick_xml::events::Event::Text(event)) if collecting => {
                value.push_str(&event.decode().ok()?);
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"c" => {
                let value = if cell_type.as_deref() == Some("s") {
                    value
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| shared_strings.get(index))
                        .cloned()
                        .unwrap_or_else(|| value.clone())
                } else {
                    value.clone()
                };
                let column = cell_column.unwrap_or(row_cells.len()).min(63);
                if row_cells.len() <= column {
                    row_cells.resize(column + 1, String::new());
                }
                row_cells[column] = value;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"row" => {
                while row_cells.last().is_some_and(String::is_empty) {
                    row_cells.pop();
                }
                for (index, cell) in row_cells.iter().enumerate() {
                    if index > 0 && !append_bounded_preview_text(&mut out, "\t") {
                        break;
                    }
                    if !append_bounded_preview_text(&mut out, cell) {
                        break;
                    }
                }
                if !append_bounded_preview_text(&mut out, "\n")
                    || out.chars().count() >= EXTRACTED_TEXT_LIMIT
                    || rows >= 80
                {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    normalized_preview_text(&out)
}

fn xlsx_column_index(reference: &str) -> Option<usize> {
    let mut column = 0usize;
    let mut found = false;
    for byte in reference.bytes() {
        if !byte.is_ascii_alphabetic() {
            break;
        }
        found = true;
        column = column
            .checked_mul(26)?
            .checked_add(usize::from(byte.to_ascii_uppercase() - b'A') + 1)?;
    }
    found.then(|| column.saturating_sub(1))
}

fn normalized_preview_text(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut previous_blank = false;
    let mut characters = 0usize;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !previous_blank && !out.is_empty() && characters < EXTRACTED_TEXT_LIMIT {
                out.push('\n');
                characters += 1;
                previous_blank = true;
            }
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            if characters >= EXTRACTED_TEXT_LIMIT {
                break;
            }
            out.push('\n');
            characters += 1;
        }
        let remaining = EXTRACTED_TEXT_LIMIT.saturating_sub(characters);
        let mut chars = trimmed.chars();
        out.extend(chars.by_ref().take(remaining));
        characters += trimmed.chars().take(remaining).count();
        previous_blank = false;
        if chars.next().is_some() || characters >= EXTRACTED_TEXT_LIMIT {
            break;
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn append_bounded_preview_text(out: &mut String, text: &str) -> bool {
    let remaining = EXTRACTED_TEXT_LIMIT.saturating_sub(out.chars().count());
    let mut chars = text.chars();
    out.extend(chars.by_ref().take(remaining));
    chars.next().is_none()
}

fn limit_preview_text(text: &str) -> String {
    let mut out: String = text.chars().take(EXTRACTED_TEXT_LIMIT).collect();
    if text.chars().count() > EXTRACTED_TEXT_LIMIT {
        out.push_str("\n\n...(text preview truncated)");
    }
    out
}

fn preview_fallback(title: String, path: &Path) -> PreviewData {
    let kind = file_kind_label(path);
    let mut lines = vec![format!("Filetype: {kind}")];
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        lines.push(format!("Size: {}", format_size(meta.len())));
        lines.push(format!(
            "Read-only: {}",
            if meta.permissions().readonly() {
                "Yes"
            } else {
                "No"
            }
        ));
        if let Ok(created) = meta.created()
            && let Ok(dur) = created.duration_since(std::time::UNIX_EPOCH)
        {
            lines.push(format!("Created: {}", format_modified(dur.as_secs())));
        }
        if let Ok(modified) = meta.modified()
            && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            lines.push(format!(
                "Permissions: {:o}",
                meta.permissions().mode() & 0o777
            ));
        }
    } else {
        lines.push("Metadata unavailable.".to_string());
    }
    lines.push(format!("Path: {}", path.display()));
    PreviewData {
        title,
        kind: PreviewKind::Unsupported,
        body: lines.join("\n"),
        open_external: true,
    }
}

fn path_format(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
}

fn text_kind_label(path: &Path) -> String {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "Rust source".to_string(),
        "js" => "JavaScript source".to_string(),
        "ts" => "TypeScript source".to_string(),
        "py" => "Python source".to_string(),
        "md" | "markdown" => "Markdown document".to_string(),
        "json" => "JSON document".to_string(),
        "toml" => "TOML document".to_string(),
        "yaml" | "yml" => "YAML document".to_string(),
        "csv" => "CSV document".to_string(),
        "vcf" => "vCard contact file".to_string(),
        "ics" | "ical" => "calendar file".to_string(),
        "ldif" => "LDIF address book".to_string(),
        "sql" => "SQL script".to_string(),
        "rtf" => "rich text document".to_string(),
        "svg" => "SVG image text".to_string(),
        "log" => "log file".to_string(),
        "txt" => "plain text file".to_string(),
        _ => "text file".to_string(),
    }
}

fn file_kind_label(path: &Path) -> String {
    if path.is_dir() {
        return "folder".to_string();
    }
    if let Some(format) = path_format(path) {
        format!("{format} file")
    } else {
        "file".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("ottrin-preview-{unique}-{name}"))
    }

    fn write_gzip(path: &Path, payload: &[u8]) {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let file = std::fs::File::create(path).expect("create gzip archive");
        let mut gzip = GzEncoder::new(file, Compression::default());
        gzip.write_all(payload).expect("write gzip payload");
        gzip.finish().expect("finish gzip archive");
    }

    fn pax_size_override_tar() -> Vec<u8> {
        let pax_record = b"13 size=1024\n";
        let mut archive = Vec::new();

        let mut pax_header = tar::Header::new_gnu();
        pax_header.set_entry_type(tar::EntryType::XHeader);
        pax_header.set_size(pax_record.len() as u64);
        pax_header.set_cksum();
        archive.extend_from_slice(pax_header.as_bytes());
        archive.extend_from_slice(pax_record);
        archive.resize(1024, 0);

        let mut file_header = tar::Header::new_gnu();
        file_header
            .set_path("pax-sized.txt")
            .expect("set entry path");
        file_header.set_size(0);
        file_header.set_mode(0o644);
        file_header.set_cksum();
        archive.extend_from_slice(file_header.as_bytes());
        archive.extend(vec![b'x'; 1024]);
        archive.extend_from_slice(&[0; 1024]);
        archive
    }

    fn unterminated_pax_size_override_tar() -> Vec<u8> {
        // The declared PAX record length matches, but its final byte is not an
        // LF. Treating that byte as a terminator would make this `size=0`
        // override hide the following 512-byte file payload as a TAR marker.
        let pax_record = b"10 size=0X";
        let mut archive = Vec::new();

        let mut pax_header = tar::Header::new_gnu();
        pax_header.set_entry_type(tar::EntryType::XHeader);
        pax_header.set_size(pax_record.len() as u64);
        pax_header.set_cksum();
        archive.extend_from_slice(pax_header.as_bytes());
        archive.extend_from_slice(pax_record);
        archive.resize(1024, 0);

        let mut file_header = tar::Header::new_gnu();
        file_header
            .set_path("unterminated-pax.txt")
            .expect("set entry path");
        file_header.set_size(512);
        file_header.set_mode(0o644);
        file_header.set_cksum();
        archive.extend_from_slice(file_header.as_bytes());
        archive.extend_from_slice(&[0; 512]);
        // One real TAR terminator block: the guard must not mistake the file
        // payload above for the other required terminator.
        archive.extend_from_slice(&[0; 512]);
        archive
    }

    fn malformed_pax_before_size_override_tar() -> Vec<u8> {
        let pax_record = b"10 abcdef\n9 size=0\n";
        let mut archive = Vec::new();

        let mut pax_header = tar::Header::new_gnu();
        pax_header.set_entry_type(tar::EntryType::XHeader);
        pax_header.set_size(pax_record.len() as u64);
        pax_header.set_cksum();
        archive.extend_from_slice(pax_header.as_bytes());
        archive.extend_from_slice(pax_record);
        archive.resize(1024, 0);

        let mut file_header = tar::Header::new_gnu();
        file_header
            .set_path("malformed-pax.txt")
            .expect("set entry path");
        file_header.set_size(512);
        file_header.set_mode(0o644);
        file_header.set_cksum();
        archive.extend_from_slice(file_header.as_bytes());
        archive.extend_from_slice(&[0; 512]);
        archive.extend_from_slice(&[0; 512]);
        archive
    }

    #[test]
    fn classified_formats_expose_shared_preview_contract_views_and_renderers() {
        let path = temp_path("manual.pdf");
        std::fs::write(&path, b"%PDF-1.7").expect("write sample");

        let kind = classify_path(&path);
        let profile = preview_profile(&path);

        assert_eq!(kind, PreviewKind::Pdf);
        assert_eq!(profile.kind(), kind);
        assert!(profile.supports_view(PreviewView::Metadata));
        assert!(profile.supports_view(PreviewView::Visual));
        assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Pdf);
        assert!(!profile.raw_fallback_allowed());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svg_is_rasterized_safely_without_external_references() {
        let path = temp_path("graphic.svg");
        std::fs::write(
            &path,
            r#"<svg width="12" height="12" xmlns="http://www.w3.org/2000/svg"><rect width='12' height='12' fill='#ff0000'/></svg>"#,
        )
        .expect("write sample");

        assert_eq!(preview_profile(&path).kind(), PreviewKind::Image);
        assert_eq!(
            preview_profile(&path).visual_renderer(),
            PreviewVisualRenderer::Image
        );

        let bytes =
            render_svg_content_preview_png(&path, 64).expect("safe svg preview should render");
        let image = image::load_from_memory(&bytes).expect("decode svg preview");
        assert_eq!(image.dimensions(), (12, 12));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn avif_is_not_claimed_as_visual_image_format() {
        let path = temp_path("photo.avif");
        std::fs::write(&path, b"placeholder-avif").expect("write sample");

        let profile = preview_profile(&path);
        assert_eq!(profile.kind(), PreviewKind::Unsupported);
        assert!(profile.raw_fallback_allowed());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ico_remains_a_visual_image_format_in_the_shared_preview_contract() {
        let path = temp_path("application.ico");
        std::fs::write(&path, b"placeholder-ico").expect("write sample");

        let profile = preview_profile(&path);
        assert_eq!(profile.kind(), PreviewKind::Image);
        assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Image);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svg_visual_rejects_blocked_file_and_http_references() {
        let path = temp_path("unsafe_external.svg");
        std::fs::write(
            &path,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image xlink:href="file:///tmp/hosted.png"/></svg>"#,
        )
        .expect("write sample");

        assert!(matches!(
            render_svg_content_preview_png(&path, 64),
            Err(SvgVisualError::BlockedReference)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn office_text_normalization_and_aggregation_are_bounded() {
        let oversized = "x".repeat(EXTRACTED_TEXT_LIMIT * 2);
        let normalized = normalized_preview_text(&oversized).expect("normalized text");
        assert_eq!(normalized.chars().count(), EXTRACTED_TEXT_LIMIT);

        let mut aggregate = String::new();
        assert!(!append_bounded_preview_text(&mut aggregate, &oversized));
        assert_eq!(aggregate.chars().count(), EXTRACTED_TEXT_LIMIT);

        let shared = format!("<sst><si><t>{oversized}</t></si><si><t>ignored</t></si></sst>");
        let strings = extract_xlsx_shared_strings(&shared).expect("shared strings");
        assert_eq!(
            strings
                .iter()
                .map(|value| value.chars().count())
                .sum::<usize>(),
            EXTRACTED_TEXT_LIMIT
        );

        let cells = (0..64)
            .map(|_| "<c t=\"s\"><v>0</v></c>")
            .collect::<String>();
        let sheet = format!("<worksheet><sheetData><row>{cells}</row></sheetData></worksheet>");
        let table = extract_xlsx_sheet(&sheet, &strings).expect("bounded table");
        assert_eq!(table.chars().count(), EXTRACTED_TEXT_LIMIT);
    }

    #[test]
    fn text_preview_includes_metadata_and_copyable_text() {
        let path = temp_path("sample.rs");
        std::fs::write(&path, "fn main() {}\nprintln!(\"hi\");\n").expect("write");

        let preview = preview_text("sample.rs".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::Text);
        assert!(preview.body.contains("Filetype: Rust source"));
        assert!(preview.body.contains("Encoding: UTF-8"));
        assert!(preview.body.contains("Lines: 2"));
        assert!(preview.body.contains(EXTRACTED_TEXT_MARKER));
        assert!(preview.body.contains("fn main()"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn text_visual_card_is_deterministic_bounded_and_includes_context() {
        let path = temp_path("sample.rs");
        std::fs::write(&path, "fn main() {}\nprintln!(\"hi\");\n").expect("write");

        let first = render_text_content_preview_png(&path, 256).expect("first card");
        let second = render_text_content_preview_png(&path, 256).expect("second card");
        let decoded = image::load_from_memory(&first).expect("decode visual PNG");

        assert_eq!(first, second, "the cacheable card must be deterministic");
        assert_eq!(decoded.dimensions(), (256, 256));
        assert!(first.len() < 256 * 1024, "render size must stay bounded");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn text_visual_card_rejects_binary_input() {
        let path = temp_path("unsafe.rs");
        std::fs::write(&path, b"fn main() {}\0").expect("write");

        assert!(render_text_content_preview_png(&path, 256).is_err());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fallback_preview_identifies_not_undoable_metadata_fields() {
        let path = temp_path("opaque.bin");
        std::fs::write(&path, [0_u8, 1, 2, 3]).expect("write");

        let preview = preview_fallback("opaque.bin".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::Unsupported);
        assert!(preview.body.contains("Filetype: BIN file"));
        assert!(preview.body.contains("Read-only:"));
        assert!(preview.body.contains("Path:"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn zip_preview_lists_counts_sizes_and_contents() {
        let path = temp_path("archive.zip");
        {
            let file = std::fs::File::create(&path).expect("create zip");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.add_directory("docs/", options).expect("dir");
            zip.start_file("docs/readme.txt", options).expect("file");
            zip.write_all(b"hello from ottrin").expect("write entry");
            zip.finish().expect("finish");
        }

        let preview = preview_zip("archive.zip".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Filetype: ZIP archive"));
        assert!(preview.body.contains("Uncompressed:"));
        assert!(preview.body.contains("Compressed:"));
        assert!(preview.body.contains("Contents:"));
        assert!(preview.body.contains("docs/readme.txt"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_preview_lists_counts_sizes_and_contents() {
        let path = temp_path("archive.tar");
        {
            let dir = std::env::temp_dir()
                .join(format!("ottrin-preview-tar-source-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("make tar source");
            std::fs::create_dir_all(dir.join("docs")).expect("make docs");
            std::fs::write(dir.join("docs/readme.txt"), "hello from ottrin\n")
                .expect("write sample");

            let file = std::fs::File::create(&path).expect("create tar");
            let mut tar = tar::Builder::new(file);
            tar.append_dir_all("docs", dir.join("docs"))
                .expect("add docs");
            tar.finish().expect("finish tar");

            let _ = std::fs::remove_dir_all(dir);
        }

        let preview = preview_archive("archive.tar".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Filetype: TAR archive"));
        assert!(preview.body.contains("Files: 1"));
        assert!(preview.body.contains("Folders: 1"));
        assert!(preview.body.contains("Contents:"));
        assert!(preview.body.contains("docs/readme.txt"));
        assert!(preview.open_external);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_lists_counts_sizes_and_contents() {
        let path = temp_path("archive.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            let payload = b"hello from compressed ottrin";
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "docs/readme.txt", payload.as_slice())
                .expect("add tar entry");
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);

        let preview = preview_archive("archive.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Filetype: TAR.GZ archive"));
        assert!(preview.body.contains("Files: 1"));
        assert!(preview.body.contains("docs/readme.txt"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_non_gzip_input() {
        let path = temp_path("not-gzip.tar.gz");
        std::fs::write(&path, b"this is not a gzip stream").expect("write invalid gzip");

        let preview = preview_archive("not-gzip.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Could not read archive entries."));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_gzip_payload_that_is_not_a_tar() {
        let path = temp_path("not-tar.tar.gz");
        write_gzip(&path, b"this is valid gzip but not a TAR archive");

        let preview = preview_archive("not-tar.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Could not read archive entries."));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_and_tar_gz_previews_honor_bounded_pax_size_overrides() {
        let tar_bytes = pax_size_override_tar();
        for (name, gzip) in [("pax-sized.tar", false), ("pax-sized.tar.gz", true)] {
            let path = temp_path(name);
            if gzip {
                write_gzip(&path, &tar_bytes);
            } else {
                std::fs::write(&path, &tar_bytes).expect("write TAR archive");
            }

            let preview = preview_archive(name.to_string(), &path);

            assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
            assert!(preview.body.contains("Files: 1"));
            assert!(
                preview.body.contains("Uncompressed: 1.0 KB"),
                "{}",
                preview.body
            );
            assert!(preview.body.contains("pax-sized.txt"));

            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn tar_gz_preview_rejects_tar_without_a_complete_terminator() {
        for (name, zero_blocks) in [
            ("missing-terminator.tar.gz", 0),
            ("one-zero-block.tar.gz", 1),
        ] {
            let path = temp_path(name);
            let payload = b"payload";
            let mut header = tar::Header::new_gnu();
            header
                .set_path("missing-terminator.txt")
                .expect("set entry path");
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            let mut tar_bytes = header.as_bytes().to_vec();
            tar_bytes.extend_from_slice(payload);
            tar_bytes.resize(1024, 0);
            tar_bytes.extend(vec![0; zero_blocks * 512]);
            write_gzip(&path, &tar_bytes);

            let preview = preview_archive(name.to_string(), &path);

            assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
            assert!(preview.body.contains("Could not read archive entries."));

            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn tar_gz_preview_rejects_trailing_non_gzip_data() {
        let path = temp_path("trailing-data.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            let payload = b"payload";
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "safe.txt", payload.as_slice())
                .expect("add tar entry");
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open gzip archive");
        file.write_all(b"not a gzip member")
            .expect("append invalid trailing data");

        let preview = preview_archive("trailing-data.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Could not read archive entries."));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_a_second_member_with_non_tar_data() {
        let path = temp_path("second-member.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            let payload = b"payload";
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, "safe.txt", payload.as_slice())
                .expect("add tar entry");
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);
        {
            use flate2::Compression;
            use flate2::write::GzEncoder;

            let file = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open gzip archive");
            let mut gzip = GzEncoder::new(file, Compression::default());
            gzip.write_all(b"not a TAR continuation")
                .expect("write second member");
            gzip.finish().expect("finish second member");
        }

        let preview = preview_archive("second-member.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Could not read archive entries."));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_oversized_compressed_input() {
        let path = temp_path("huge-archive.tar.gz");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .expect("create sparse gzip archive");
        file.set_len(MAX_TAR_COMPRESSED_BYTES + 1)
            .expect("inflate compressed test archive size");

        let preview = preview_archive("huge-archive.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive is too large for safe metadata scanning.")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_a_header_declaring_excessive_uncompressed_data() {
        let path = temp_path("declared-huge.tar.gz");
        let mut tar_bytes = Vec::new();
        let mut header = tar::Header::new_gnu();
        header
            .set_path("declared-huge.bin")
            .expect("set entry path");
        header.set_size(MAX_TAR_SCAN_BYTES + 1);
        header.set_mode(0o644);
        header.set_cksum();
        tar_bytes.extend_from_slice(header.as_bytes());
        tar_bytes.extend_from_slice(&[0; 1024]);
        write_gzip(&path, &tar_bytes);

        let preview = preview_archive("declared-huge.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive has too much data for safe preview scanning."),
            "a declared-size bomb must be rejected before any payload read"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_entry_paths_past_the_listing_bound() {
        let path = temp_path("long-path.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            let payload = b"payload";
            let long_path = format!("{}.txt", "a".repeat(MAX_TAR_PATH_BYTES));
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, long_path, payload.as_slice())
                .expect("add long-path entry");
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);

        let preview = preview_archive("long-path.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Could not read archive entries."));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_bounds_entry_count_and_listing() {
        let path = temp_path("many-entries.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            for i in 0..(MAX_TAR_LISTED_ENTRIES + 1) {
                let payload = b"payload";
                let mut header = tar::Header::new_gnu();
                header.set_size(payload.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, format!("entry-{i}"), payload.as_slice())
                    .expect("add synthetic entry");
            }
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);

        let preview = preview_archive("many-entries.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Entries: 201"));
        assert!(preview.body.contains("  ... and 1 more entries"));
        assert!(!preview.body.contains("  entry-200  ("));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_gz_preview_rejects_too_many_entries() {
        let path = temp_path("too-many-entries.tar.gz");
        let mut tar_bytes = Vec::new();
        {
            let mut tar = tar::Builder::new(&mut tar_bytes);
            for i in 0..(MAX_TAR_SCAN_ENTRIES + 1) {
                let payload = b"payload";
                let mut header = tar::Header::new_gnu();
                header.set_size(payload.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, format!("entry-{i}"), payload.as_slice())
                    .expect("add synthetic entry");
            }
            tar.finish().expect("finish tar");
        }
        write_gzip(&path, &tar_bytes);

        let preview = preview_archive("too-many-entries.tar.gz".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive has too many entries for safe scanning.")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn limited_reader_rejects_data_past_input_limit() {
        let mut reader = LimitedReader::new(std::io::Cursor::new(b"abc"), 2);
        let mut buffer = [0; 3];

        assert_eq!(reader.read(&mut buffer).expect("read up to limit"), 2);
        assert_eq!(&buffer[..2], b"ab");
        assert!(reader.read(&mut buffer).is_err());
    }

    #[test]
    fn tar_header_guard_rejects_oversized_path_extensions_before_payload_reads() {
        for extension_type in [tar::EntryType::GNULongName, tar::EntryType::XHeader] {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(extension_type);
            header.set_size(MAX_TAR_PATH_BYTES as u64 + 1);
            header.set_cksum();
            let mut reader = TarHeaderGuard::new(std::io::Cursor::new(header.as_bytes()));
            let mut output = [0; 512];

            assert!(reader.read(&mut output).is_err());
        }
    }

    #[test]
    fn pax_size_override_uses_the_first_duplicate_size_record() {
        assert_eq!(
            pax_size_override(b"13 size=1024\n9 size=0\n").expect("parse PAX size"),
            Some(1024)
        );
    }

    #[test]
    fn pax_size_override_rejects_unterminated_records() {
        assert!(pax_size_override(b"10 size=0X").is_err());
    }

    #[test]
    fn pax_size_override_rejects_an_unterminated_duplicate_record() {
        assert!(pax_size_override(b"13 size=1024\n10 size=0X").is_err());
    }

    #[test]
    fn pax_size_override_rejects_a_record_without_a_key_value_separator() {
        assert!(pax_size_override(b"10 abcdef\n9 size=0\n").is_err());
    }

    #[test]
    fn tar_previews_reject_unterminated_pax_that_masks_a_missing_terminator() {
        let tar_bytes = unterminated_pax_size_override_tar();
        for (name, gzip) in [
            ("unterminated-pax.tar", false),
            ("unterminated-pax.tar.gz", true),
        ] {
            let path = temp_path(name);
            if gzip {
                write_gzip(&path, &tar_bytes);
            } else {
                std::fs::write(&path, &tar_bytes).expect("write TAR archive");
            }

            let preview = preview_archive(name.to_string(), &path);

            assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
            assert!(preview.body.contains("Could not read archive entries."));

            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn tar_previews_reject_malformed_pax_that_masks_a_missing_terminator() {
        let tar_bytes = malformed_pax_before_size_override_tar();
        for (name, gzip) in [("malformed-pax.tar", false), ("malformed-pax.tar.gz", true)] {
            let path = temp_path(name);
            if gzip {
                write_gzip(&path, &tar_bytes);
            } else {
                std::fs::write(&path, &tar_bytes).expect("write TAR archive");
            }

            let preview = preview_archive(name.to_string(), &path);

            assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
            assert!(preview.body.contains("Could not read archive entries."));

            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn tar_preview_rejects_oversized_archives() {
        let path = temp_path("huge-archive.tar");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .expect("create sparse archive");
        file.set_len(MAX_TAR_COMPRESSED_BYTES + 1)
            .expect("inflate test archive size");

        let preview = preview_archive("huge-archive.tar".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive is too large for safe metadata scanning.")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_preview_rejects_too_many_entries() {
        let path = temp_path("many-entries.tar");
        {
            let file = std::fs::File::create(&path).expect("create tar");
            let mut tar = tar::Builder::new(file);
            for i in 0..(MAX_TAR_SCAN_ENTRIES + 1) {
                let payload = b"payload";
                let mut header = tar::Header::new_gnu();
                header.set_size(payload.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, format!("entry-{i}"), payload.as_slice())
                    .expect("add synthetic entry");
            }
            tar.finish().expect("finish tar");
        }

        let preview = preview_archive("many-entries.tar".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive has too many entries for safe scanning.")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tar_preview_rejects_a_header_declaring_excessive_uncompressed_data() {
        let path = temp_path("declared-huge.tar");
        {
            let mut header = tar::Header::new_gnu();
            header
                .set_path("declared-huge.bin")
                .expect("set entry path");
            header.set_size(MAX_TAR_SCAN_BYTES + 1);
            header.set_mode(0o644);
            header.set_cksum();

            let mut file = std::fs::File::create(&path).expect("create tar");
            file.write_all(header.as_bytes()).expect("write tar header");
            file.write_all(&[0; 1024]).expect("write tar end markers");
        }

        let preview = preview_archive("declared-huge.tar".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Archive has too much data for safe preview scanning."),
            "a declared-size bomb must be rejected before any payload read"
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_preview_contains_sheet_cells_and_bounded_visual() {
        let path = temp_path("workbook.ods");
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            zip.write_all(
                br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><t:table t:name="Budget"><t:table-row t:number-rows-repeated="2"><t:table-cell t:number-columns-repeated="2"><x:p>Item</x:p></t:table-cell><t:table-cell><x:p>Cost</x:p></t:table-cell></t:table-row><t:table-row><t:table-cell><x:p>Coffee</x:p></t:table-cell><t:table-cell office:value-type="float" office:value="42"><x:p>42</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"#,
            )
            .expect("write content");
            zip.finish().expect("finish");
        }

        let preview = preview_office("workbook.ods".to_string(), &path);
        assert!(preview.body.contains(EXTRACTED_TEXT_MARKER));
        assert!(preview.body.contains("Sheet: Budget"));
        assert!(preview.body.contains("Item\tItem\tCost"));
        assert_eq!(preview.body.matches("Item\tItem\tCost").count(), 2);
        assert!(preview.body.contains("Coffee\t42"));
        let png = render_office_content_preview_png(&path, 256).expect("bounded visual PNG");
        assert_eq!(
            image::load_from_memory(&png)
                .expect("decode visual PNG")
                .dimensions(),
            (256, 256)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_preview_rejects_malformed_xml_after_the_output_budget() {
        let path = temp_path("malformed-workbook.ods");
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            let oversized_cell = "x".repeat(EXTRACTED_TEXT_LIMIT + 1_024);
            write!(
                zip,
                "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Budget\"><t:table-row><t:table-cell><x:p>{oversized_cell}</x:p></t:table-cell></t:table-row><broken"
            )
            .expect("write malformed content");
            zip.finish().expect("finish");
        }

        let file = std::fs::File::open(&path).expect("open ods");
        let mut archive = zip::ZipArchive::new(file).expect("open package");
        assert!(extract_ods_text(&mut archive).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_preview_rejects_malformed_or_duplicate_attributes() {
        let path = temp_path("malformed-attributes.ods");
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            zip.write_all(br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><t:table t:name="Budget"><t:table-row t:number-rows-repeated="2" t:number-rows-repeated="3"><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"#)
                .expect("write malformed attributes");
            zip.finish().expect("finish");
        }

        let file = std::fs::File::open(&path).expect("open ods");
        let mut archive = zip::ZipArchive::new(file).expect("open package");
        assert!(extract_ods_text(&mut archive).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_preview_rejects_invalid_ignored_attributes() {
        for (suffix, attribute) in [
            ("invalid-entity", "inert=\"&bogus;\""),
            ("unbound-prefix", "unknown:flag=\"x\""),
        ] {
            let path = temp_path(&format!("{suffix}.ods"));
            {
                let file = std::fs::File::create(&path).expect("create ods");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", options).expect("content");
                write!(
                    zip,
                    "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Budget\" {attribute}><t:table-row><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"
                )
                .expect("write malformed attribute");
                zip.finish().expect("finish");
            }

            let file = std::fs::File::open(&path).expect("open ods");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            assert!(
                extract_ods_text(&mut archive).is_none(),
                "ignored malformed attribute must fail closed: {suffix}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ods_preview_rejects_unbound_elements_and_invalid_text() {
        for (suffix, inner) in [
            (
                "invalid-text",
                "<t:table-row><t:table-cell><x:p>&bogus;</x:p></t:table-cell></t:table-row>",
            ),
            (
                "unbound-element",
                "<unknown:item/><t:table-row><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row>",
            ),
            (
                "unnamespaced-element",
                "<item/><t:table-row><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row>",
            ),
            (
                "invalid-comment",
                "<!-- malformed -- comment --><t:table-row><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row>",
            ),
        ] {
            let path = temp_path(&format!("{suffix}.ods"));
            {
                let file = std::fs::File::create(&path).expect("create ods");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", options).expect("content");
                write!(
                    zip,
                    "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Budget\">{inner}</t:table></office:spreadsheet></office:body></office:document-content>"
                )
                .expect("write malformed XML");
                zip.finish().expect("finish");
            }

            let file = std::fs::File::open(&path).expect("open ods");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            assert!(
                extract_ods_text(&mut archive).is_none(),
                "malformed namespace/text must fail closed: {suffix}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ods_preview_rejects_invalid_repeat_counts() {
        for (suffix, inner) in [
            (
                "zero-row-repeat",
                "<t:table-row t:number-rows-repeated=\"0\"><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row>",
            ),
            (
                "malformed-row-repeat",
                "<t:table-row t:number-rows-repeated=\"not-a-number\"><t:table-cell><x:p>Value</x:p></t:table-cell></t:table-row>",
            ),
            (
                "overflow-empty-cell-repeat",
                "<t:table-row><t:table-cell t:number-columns-repeated=\"999999999999999999999999999999999999999999999999\"/></t:table-row>",
            ),
        ] {
            let path = temp_path(&format!("{suffix}.ods"));
            {
                let file = std::fs::File::create(&path).expect("create ods");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", options).expect("content");
                write!(
                    zip,
                    "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Budget\">{inner}</t:table></office:spreadsheet></office:body></office:document-content>"
                )
                .expect("write invalid repeat");
                zip.finish().expect("finish");
            }

            let file = std::fs::File::open(&path).expect("open ods");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            assert!(
                extract_ods_text(&mut archive).is_none(),
                "invalid repeat must fail closed: {suffix}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ods_empty_rows_and_tables_consume_display_budgets() {
        let cases = [
            (
                "empty-row-repeat",
                "<t:table t:name=\"Rows\"><t:table-row t:number-rows-repeated=\"256\"/><t:table-row><t:table-cell><x:p>HIDDEN_ROW</x:p></t:table-cell></t:table-row></t:table>".to_string(),
                "HIDDEN_ROW",
            ),
            (
                "empty-table-count",
                format!(
                    "{}<t:table t:name=\"Thirteen\"><t:table-row><t:table-cell><x:p>HIDDEN_SHEET</x:p></t:table-cell></t:table-row></t:table>",
                    "<t:table/>".repeat(12)
                ),
                "HIDDEN_SHEET",
            ),
        ];

        for (suffix, spreadsheet, hidden) in cases {
            let path = temp_path(&format!("{suffix}.ods"));
            {
                let file = std::fs::File::create(&path).expect("create ods");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", options).expect("content");
                write!(
                    zip,
                    "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet>{spreadsheet}</office:spreadsheet></office:body></office:document-content>"
                )
                .expect("write empty budget case");
                zip.finish().expect("finish");
            }

            let file = std::fs::File::open(&path).expect("open ods");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            let preview = extract_ods_text(&mut archive).expect("preview");
            assert!(
                !preview.contains(hidden),
                "empty ODF structures must consume their display budget: {suffix}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ods_preview_validates_repeats_beyond_display_caps() {
        let cases = [
            (
                "row-257",
                format!(
                    "<t:table t:name=\"Rows\">{}<t:table-row t:number-rows-repeated=\"0\"/></t:table>",
                    "<t:table-row/>".repeat(256)
                ),
            ),
            (
                "cell-65",
                format!(
                    "<t:table t:name=\"Cells\"><t:table-row>{}<t:table-cell t:number-columns-repeated=\"invalid\"/></t:table-row></t:table>",
                    "<t:table-cell/>".repeat(64)
                ),
            ),
            (
                "sheet-13",
                format!(
                    "{}<t:table t:name=\"Thirteen\"><t:table-row t:number-rows-repeated=\"999999999999999999999999999999999999999\"/></t:table>",
                    "<t:table/>".repeat(12)
                ),
            ),
        ];

        for (suffix, spreadsheet) in cases {
            let path = temp_path(&format!("{suffix}.ods"));
            {
                let file = std::fs::File::create(&path).expect("create ods");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("content.xml", options).expect("content");
                write!(
                    zip,
                    "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\"><office:body><office:spreadsheet>{spreadsheet}</office:spreadsheet></office:body></office:document-content>"
                )
                .expect("write post-cap repeat");
                zip.finish().expect("finish");
            }

            let file = std::fs::File::open(&path).expect("open ods");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            assert!(
                extract_ods_text(&mut archive).is_none(),
                "post-cap invalid repeat must fail closed: {suffix}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn saturated_ods_cell_stops_materializing_more_chunks() {
        let mut incremental = String::new();
        let mut incremental_characters = 0usize;
        let mut incremental_saturated = false;
        for _ in 0..EXTRACTED_TEXT_LIMIT {
            append_ods_cell_chunk(
                &mut incremental,
                &mut incremental_characters,
                &mut incremental_saturated,
                "x",
            );
        }
        assert_eq!(incremental_characters, EXTRACTED_TEXT_LIMIT);
        assert!(incremental_saturated);

        let mut cell = "x".repeat(EXTRACTED_TEXT_LIMIT);
        let mut characters = EXTRACTED_TEXT_LIMIT;
        let mut saturated = true;
        let original = cell.clone();
        for _ in 0..10_000 {
            append_ods_cell_chunk(&mut cell, &mut characters, &mut saturated, "ignored");
        }
        assert_eq!(cell, original);
        assert_eq!(characters, EXTRACTED_TEXT_LIMIT);
        assert!(saturated);
    }

    #[test]
    fn xlsx_preview_contains_sheet_cells_in_reading_order() {
        let path = temp_path("workbook.xlsx");
        {
            let file = std::fs::File::create(&path).expect("create xlsx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/sharedStrings.xml", options)
                .expect("strings");
            zip.write_all(br#"<sst><si><t>Name</t></si><si><t>Ada</t></si></sst>"#)
                .expect("write strings");
            zip.start_file("xl/workbook.xml", options)
                .expect("workbook");
            zip.write_all(br#"<workbook xmlns:r="relationships"><sheets><sheet name="Quarter 1" r:id="rId7"/></sheets></workbook>"#)
                .expect("write workbook");
            zip.start_file("xl/_rels/workbook.xml.rels", options)
                .expect("relationships");
            zip.write_all(br#"<Relationships><Relationship Id="rId7" Target="worksheets/sheet1.xml"/></Relationships>"#)
                .expect("write relationships");
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            zip.write_all(
                br#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="C1"><v>Score</v></c></row><row><c r="A2" t="s"><v>1</v></c><c r="C2"><v>42</v></c></row></sheetData></worksheet>"#,
            )
            .expect("write sheet");
            zip.finish().expect("finish");
        }

        let preview = preview_office("workbook.xlsx".to_string(), &path);

        assert!(preview.body.contains(EXTRACTED_TEXT_MARKER));
        assert!(preview.body.contains("Sheet: Quarter 1"));
        assert!(preview.body.contains("Name\t\tScore"));
        assert!(preview.body.contains("Ada\t\t42"));
        let png = render_office_content_preview_png(&path, 256).expect("bounded visual PNG");
        let decoded = image::load_from_memory(&png).expect("decode visual PNG");
        assert_eq!(decoded.dimensions(), (256, 256));
        assert!(
            decoded
                .pixels()
                .any(|(_, _, pixel)| pixel.0 != [250, 250, 248, 255])
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn docx_visual_preview_is_bounded_png_not_package_bytes() {
        let path = temp_path("letter.docx");
        {
            let file = std::fs::File::create(&path).expect("create docx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("word/document.xml", options)
                .expect("document");
            zip.write_all(br#"<w:document><w:body><w:p><w:r><w:t>Ottrin visual preview</w:t></w:r></w:p></w:body></w:document>"#)
                .expect("write document");
            zip.finish().expect("finish");
        }

        let png = render_office_content_preview_png(&path, 1).expect("bounded visual PNG");
        let decoded = image::load_from_memory(&png).expect("decode visual PNG");
        assert_eq!(decoded.dimensions(), (49, 64));
        assert!(
            decoded
                .pixels()
                .any(|(_, _, pixel)| pixel.0 != [255, 255, 255, 255]),
            "DOCX preview must rasterize readable glyphs, not an empty page"
        );
        assert_ne!(png, std::fs::read(&path).expect("read package"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pptx_preview_separates_slide_text() {
        let path = temp_path("slides.pptx");
        {
            let file = std::fs::File::create(&path).expect("create pptx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("ppt/slides/slide1.xml", options)
                .expect("slide");
            zip.write_all(br#"<p:sld><a:t>Launch plan</a:t><a:t>Ready for v1</a:t></p:sld>"#)
                .expect("write slide");
            zip.finish().expect("finish");
        }

        let preview = preview_office("slides.pptx".to_string(), &path);

        assert!(preview.body.contains("Slide 1"));
        assert!(preview.body.contains("Launch plan"));
        assert!(preview.body.contains("Ready for v1"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn office_preview_rejects_oversized_decompressed_xml_part() {
        let path = temp_path("oversized.docx");
        {
            let file = std::fs::File::create(&path).expect("create docx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("word/document.xml", options)
                .expect("document");
            zip.write_all(&vec![b'x'; MAX_OFFICE_XML_BYTES as usize + 1])
                .expect("write oversized XML");
            zip.finish().expect("finish");
        }

        let preview = preview_office("oversized.docx".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::OfficeMetadata);
        assert!(!preview.body.contains(EXTRACTED_TEXT_MARKER));
        assert!(preview.body.contains("Internal parts: 1"));
        assert!(matches!(
            render_office_content_preview_png(&path, 256),
            Err(OfficeVisualError::NoContent)
        ));
        let _ = std::fs::remove_file(path);
    }
}
