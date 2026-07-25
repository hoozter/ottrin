use ottrin_core::{EntryKind, format_modified};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

mod pdf_visual;
pub use pdf_visual::{
    PdfVisualError, render_pdf_first_page_png, render_pdf_first_page_png_with_library,
};

const EXTRACTED_TEXT_MARKER: &str = "\n\nExtracted text:\n";
const EXTRACTED_TEXT_LIMIT: usize = 24_000;
const MAX_OFFICE_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_PARTS: usize = 4_096;
const MAX_OFFICE_XML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SHARED_STRINGS: usize = 4_096;
const OFFICE_VISUAL_TEXT_LIMIT: usize = 4_096;

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
        | "svg" | "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "rb" | "go" | "java" | "c"
        | "cpp" | "h" | "hpp" | "m" | "mm" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat"
        | "vcf" | "ics" | "ical" | "ldif" | "sql" | "rtf" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => PreviewKind::Image,
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

/// Renders bounded extracted DOCX/XLSX/XLSM content as a PNG. This is a
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
    if !matches!(ext.as_str(), "docx" | "xlsx" | "xlsm") {
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
        _ => None,
    }
    .ok_or(OfficeVisualError::NoContent)?;
    render_office_content_png(&text, &ext, target_dim)
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
    let mut image = RgbaImage::from_pixel(dimension, dimension, Rgba([250, 250, 248, 255]));
    let margin = (dimension / 12).clamp(6, 48);
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
    let body_bottom = dimension.saturating_sub(margin);
    if format == "docx" {
        render_document_lines(
            &mut image,
            &text,
            margin,
            body_top,
            dimension - margin,
            body_bottom,
        );
    } else {
        render_sheet_grid(
            &mut image,
            &text,
            margin,
            body_top,
            dimension - margin,
            body_bottom,
        );
    }
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
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

fn render_document_lines(
    image: &mut image::RgbaImage,
    text: &str,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) {
    let line_height = ((bottom.saturating_sub(top)) / 18).clamp(3, 24);
    for (index, line) in text.lines().take(18).enumerate() {
        let y = top + index as u32 * line_height;
        let width =
            ((line.chars().take(96).count() as u32 * (right - left) / 96).max(8)).min(right - left);
        fill_rect(
            image,
            left,
            y,
            left + width,
            y + (line_height / 3).max(2),
            [82, 86, 92, 255],
        );
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
    let is_ooxml = matches!(ext.as_str(), "docx" | "xlsx" | "xlsm" | "pptx");

    let package_is_bounded = std::fs::metadata(path)
        .map(|metadata| metadata.len() <= MAX_OFFICE_PACKAGE_BYTES)
        .unwrap_or(false);
    if is_ooxml
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
        assert_eq!(decoded.dimensions(), (64, 64));
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
