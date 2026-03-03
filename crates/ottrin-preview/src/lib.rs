use ottrin_core::EntryKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewKind {
    Text,
    Image,
    Pdf,
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
        PreviewKind::OfficeMetadata => preview_office(title, &request.path),
        PreviewKind::Unsupported => preview_fallback(title, &request.path),
    }
}

pub fn classify_path(path: &std::path::Path) -> PreviewKind {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();

    match ext.as_str() {
        "txt" | "md" | "json" | "toml" | "yaml" | "yml" | "log" | "rs" | "js" | "ts" => PreviewKind::Text,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => PreviewKind::Image,
        "pdf" => PreviewKind::Pdf,
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => PreviewKind::OfficeMetadata,
        _ => PreviewKind::Unsupported,
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
    let info = std::fs::metadata(path)
        .map(|m| format!("Image file size: {} bytes", m.len()))
        .unwrap_or_else(|_| "Image metadata unavailable.".to_string());
    PreviewData {
        title,
        kind: PreviewKind::Image,
        body: info,
        open_external: true,
    }
}

fn preview_pdf(title: String, path: &std::path::Path) -> PreviewData {
    let info = std::fs::metadata(path)
        .map(|m| format!("PDF file size: {} bytes", m.len()))
        .unwrap_or_else(|_| "PDF metadata unavailable.".to_string());
    PreviewData {
        title,
        kind: PreviewKind::Pdf,
        body: info,
        open_external: true,
    }
}

fn preview_office(title: String, path: &std::path::Path) -> PreviewData {
    let info = std::fs::metadata(path)
        .map(|m| format!("Office file size: {} bytes", m.len()))
        .unwrap_or_else(|_| "Office metadata unavailable.".to_string());
    PreviewData {
        title,
        kind: PreviewKind::OfficeMetadata,
        body: format!("{info}\n\nInline rendering is not implemented yet."),
        open_external: true,
    }
}

fn preview_fallback(title: String, path: &std::path::Path) -> PreviewData {
    let info = std::fs::metadata(path)
        .map(|m| format!("File size: {} bytes", m.len()))
        .unwrap_or_else(|_| "Metadata unavailable.".to_string());
    PreviewData {
        title,
        kind: PreviewKind::Unsupported,
        body: info,
        open_external: true,
    }
}
