use ottrin_core::{EntryKind, format_modified};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewKind {
    Text,
    Image,
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
        return PreviewData {
            title,
            kind: PreviewKind::Unsupported,
            body: format!("Directory:\n{}", request.path.display()),
            open_external: false,
        };
    }

    match classify_path(&request.path) {
        PreviewKind::Text => preview_text(title, &request.path),
        PreviewKind::Image => preview_image(title, &request.path),
        PreviewKind::Pdf => preview_pdf(title, &request.path),
        PreviewKind::VideoMetadata => preview_video(title, &request.path),
        PreviewKind::AudioMetadata => preview_audio(title, &request.path),
        PreviewKind::ArchiveMetadata => preview_archive(title, &request.path),
        PreviewKind::OfficeMetadata => preview_office(title, &request.path),
        PreviewKind::Unsupported => preview_fallback(title, &request.path),
    }
}

pub fn classify_path(path: &std::path::Path) -> PreviewKind {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();

    match ext.as_str() {
        "txt" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml" | "yml" | "ini"
        | "cfg" | "conf" | "env" | "xml" | "html" | "htm" | "css" | "scss" | "less" | "svg"
        | "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "rb" | "go" | "java" | "c" | "cpp" | "h"
        | "hpp" | "m" | "mm" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => PreviewKind::Image,
        "pdf" => PreviewKind::Pdf,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => PreviewKind::VideoMetadata,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => PreviewKind::AudioMetadata,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" => PreviewKind::ArchiveMetadata,
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" => PreviewKind::OfficeMetadata,
        _ => PreviewKind::Unsupported,
    }
}

fn metadata_summary(path: &std::path::Path, kind_label: &str, extra: Option<&str>) -> String {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mut lines = vec![
                format!("{kind_label}"),
                format!("Size: {}", format_size(meta.len())),
            ];
            if let Ok(modified) = meta.modified()
                && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
            }
            if let Some(ext) = path.extension().and_then(|s| s.to_str()).filter(|s| !s.is_empty()) {
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

fn preview_text(title: String, path: &std::path::Path) -> PreviewData {
    const LIMIT: usize = 128 * 1024;
    match std::fs::read(path) {
        Ok(bytes) => {
            let head = &bytes[..bytes.len().min(LIMIT)];
            match std::str::from_utf8(head) {
                Ok(s) => PreviewData {
                    title,
                    kind: PreviewKind::Text,
                    body: if bytes.len() > LIMIT {
                        format!("{s}\n\n...(truncated)")
                    } else {
                        s.to_string()
                    },
                    open_external: false,
                },
                Err(_) => PreviewData {
                    title,
                    kind: PreviewKind::Unsupported,
                    body: "File is not valid UTF-8 text.".to_string(),
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

fn preview_image(title: String, path: &std::path::Path) -> PreviewData {
    let info = metadata_summary(path, "Image preview available.", Some("Open externally for full editing."));
    PreviewData {
        title,
        kind: PreviewKind::Image,
        body: info,
        open_external: true,
    }
}

// ── PDF preview via lopdf ──────────────────────────────────────────────

fn preview_pdf(title: String, path: &std::path::Path) -> PreviewData {
    match lopdf::Document::load(path) {
        Ok(doc) => {
            let page_count = doc.get_pages().len();

            let mut lines = vec![format!("PDF document — {page_count} page{}", if page_count == 1 { "" } else { "s" })];

            // Extract document info (title, author, subject, creator)
            if let Ok(info_ref) = doc.trailer.get(b"Info") {
                if let Ok(info_obj) = doc.get_object(info_ref.as_reference().unwrap_or_default()) {
                    if let lopdf::Object::Dictionary(dict) = info_obj {
                        for (key_name, label) in [("Title", "Title"), ("Author", "Author"), ("Subject", "Subject"), ("Creator", "Creator")] {
                            if let Ok(val) = dict.get(key_name.as_bytes()) {
                                if let Some(s) = pdf_obj_to_string(val) {
                                    if !s.trim().is_empty() {
                                        lines.push(format!("{label}: {s}"));
                                    }
                                }
                            }
                        }
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

            PreviewData {
                title,
                kind: PreviewKind::Pdf,
                body: lines.join("\n"),
                open_external: true,
            }
        }
        Err(_) => {
            let info = metadata_summary(path, "PDF document.", Some("Could not parse PDF structure."));
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

// ── Audio preview via lofty ────────────────────────────────────────────

fn preview_audio(title: String, path: &std::path::Path) -> PreviewData {
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
            if let Some(tag) = tagged_file.primary_tag().or_else(|| tagged_file.first_tag()) {
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

fn preview_video(title: String, path: &std::path::Path) -> PreviewData {
    let info = metadata_summary(path, "Video file.", None);
    PreviewData {
        title,
        kind: PreviewKind::VideoMetadata,
        body: info,
        open_external: true,
    }
}

// ── Archive preview via zip crate ──────────────────────────────────────

fn preview_archive(title: String, path: &std::path::Path) -> PreviewData {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();

    if ext == "zip" {
        return preview_zip(title, path);
    }

    // For non-zip archives, show metadata only
    let info = metadata_summary(path, &format!("{} archive.", ext.to_ascii_uppercase()), None);
    PreviewData {
        title,
        kind: PreviewKind::ArchiveMetadata,
        body: info,
        open_external: true,
    }
}

fn preview_zip(title: String, path: &std::path::Path) -> PreviewData {
    const MAX_ENTRIES: usize = 200;

    match std::fs::File::open(path).and_then(|f| {
        zip::ZipArchive::new(f).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }) {
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
                format!("ZIP archive — {files} file{}, {dirs} folder{}",
                    if files == 1 { "" } else { "s" },
                    if dirs == 1 { "" } else { "s" }),
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
            let info = metadata_summary(path, "ZIP archive.", Some("Could not read archive contents."));
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

fn preview_office(title: String, path: &std::path::Path) -> PreviewData {
    // OOXML files (.docx, .xlsx, .pptx) are ZIP archives — try to read basic info
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let is_ooxml = matches!(ext.as_str(), "docx" | "xlsx" | "pptx");

    if is_ooxml {
        if let Ok(file) = std::fs::File::open(path) {
            if let Ok(archive) = zip::ZipArchive::new(file) {
                let mut lines = vec![format!("{} document.", ext.to_ascii_uppercase())];
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

                return PreviewData {
                    title,
                    kind: PreviewKind::OfficeMetadata,
                    body: lines.join("\n"),
                    open_external: true,
                };
            }
        }
    }

    let info = metadata_summary(path, &format!("{} document.", ext.to_ascii_uppercase()), None);
    PreviewData {
        title,
        kind: PreviewKind::OfficeMetadata,
        body: info,
        open_external: true,
    }
}

fn preview_fallback(title: String, path: &std::path::Path) -> PreviewData {
    let info = metadata_summary(path, "File preview unavailable.", None);
    PreviewData {
        title,
        kind: PreviewKind::Unsupported,
        body: info,
        open_external: true,
    }
}
