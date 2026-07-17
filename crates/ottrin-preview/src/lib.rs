use ottrin_core::{EntryKind, format_modified};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

const EXTRACTED_TEXT_MARKER: &str = "\n\nExtracted text:\n";
const EXTRACTED_TEXT_LIMIT: usize = 24_000;

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
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" => {
            PreviewKind::OfficeMetadata
        }
        _ => PreviewKind::Unsupported,
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
    let is_ooxml = matches!(ext.as_str(), "docx" | "xlsx" | "pptx");

    if is_ooxml
        && let Ok(file) = std::fs::File::open(path)
        && let Ok(mut archive) = zip::ZipArchive::new(file)
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
        let extracted_text = if ext == "docx" {
            extract_docx_text(&mut archive)
        } else {
            None
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
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .ok()?
        .read_to_string(&mut xml)
        .ok()?;

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
                if matches!(event.name().as_ref(), b"w:p" | b"w:tr") {
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

fn normalized_preview_text(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut previous_blank = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !previous_blank && !out.is_empty() {
                out.push('\n');
                previous_blank = true;
            }
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(trimmed);
        previous_blank = false;
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
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
}
