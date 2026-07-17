use ottrin_core::{EntryKind, format_modified};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub(crate) struct InfoPanelDetails {
    pub(crate) readonly: bool,
    pub(crate) child_count: Option<usize>,
    pub(crate) extension: Option<String>,
    pub(crate) created: Option<String>,
    pub(crate) accessed: Option<String>,
    pub(crate) permissions: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) symlink_target: Option<String>,
    pub(crate) inode: Option<u64>,
    pub(crate) device: Option<u64>,
    pub(crate) links: Option<u64>,
    pub(crate) image_dimensions: Option<String>,
    pub(crate) image_format: Option<String>,
}

pub(crate) fn entry_kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "Folder",
        EntryKind::File => "File",
        EntryKind::Symlink => "Symlink",
        EntryKind::Other => "Other",
    }
}

/// Human-readable file type description based on extension.
/// Returns e.g. "PNG Image", "Rust Source", "PDF Document".
pub(crate) fn file_type_description(path: &Path, kind: EntryKind) -> String {
    if kind == EntryKind::Directory {
        return "Folder".into();
    }
    if kind == EntryKind::Symlink {
        return "Symbolic Link".into();
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        // Images
        "png" => "PNG Image",
        "jpg" | "jpeg" => "JPEG Image",
        "gif" => "GIF Image",
        "svg" => "SVG Image",
        "webp" => "WebP Image",
        "bmp" => "Bitmap Image",
        "ico" => "Icon File",
        "tiff" | "tif" => "TIFF Image",
        "avif" => "AVIF Image",
        "heic" | "heif" => "HEIC Image",
        // Audio
        "mp3" => "MP3 Audio",
        "flac" => "FLAC Audio",
        "wav" => "WAV Audio",
        "ogg" => "Ogg Audio",
        "m4a" | "aac" => "AAC Audio",
        "opus" => "Opus Audio",
        "wma" => "WMA Audio",
        // Video
        "mp4" | "m4v" => "MP4 Video",
        "mkv" => "Matroska Video",
        "avi" => "AVI Video",
        "mov" => "QuickTime Video",
        "webm" => "WebM Video",
        "wmv" => "WMV Video",
        "flv" => "Flash Video",
        // Documents
        "pdf" => "PDF Document",
        "doc" | "docx" => "Word Document",
        "xls" | "xlsx" => "Excel Spreadsheet",
        "ppt" | "pptx" => "PowerPoint Presentation",
        "odt" => "OpenDocument Text",
        "ods" => "OpenDocument Spreadsheet",
        "odp" => "OpenDocument Presentation",
        "rtf" => "Rich Text Document",
        "txt" => "Plain Text",
        "md" | "markdown" => "Markdown Document",
        "csv" => "CSV Data",
        // Code
        "rs" => "Rust Source",
        "py" => "Python Script",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "jsx" => "React JSX",
        "tsx" => "React TSX",
        "html" | "htm" => "HTML Document",
        "css" => "CSS Stylesheet",
        "scss" | "sass" => "Sass Stylesheet",
        "json" => "JSON Data",
        "xml" => "XML Document",
        "yaml" | "yml" => "YAML Data",
        "toml" => "TOML Config",
        "sh" | "bash" => "Shell Script",
        "zsh" => "Zsh Script",
        "c" => "C Source",
        "cpp" | "cc" | "cxx" => "C++ Source",
        "h" | "hpp" => "C/C++ Header",
        "java" => "Java Source",
        "kt" => "Kotlin Source",
        "go" => "Go Source",
        "rb" => "Ruby Script",
        "php" => "PHP Script",
        "swift" => "Swift Source",
        "lua" => "Lua Script",
        "sql" => "SQL Script",
        "r" => "R Script",
        // Config
        "ini" | "cfg" => "Config File",
        "conf" => "Config File",
        "env" => "Environment File",
        "lock" => "Lock File",
        "log" => "Log File",
        // Archives
        "zip" => "ZIP Archive",
        "tar" => "Tar Archive",
        "gz" | "gzip" => "GZip Archive",
        "bz2" => "BZip2 Archive",
        "xz" => "XZ Archive",
        "7z" => "7-Zip Archive",
        "rar" => "RAR Archive",
        "zst" => "Zstandard Archive",
        // Design
        "afdesign" => "Affinity Designer File",
        "afphoto" => "Affinity Photo File",
        "psd" => "Photoshop Document",
        "ai" => "Illustrator Document",
        "sketch" => "Sketch File",
        "fig" | "figma" => "Figma File",
        "xcf" => "GIMP Image",
        // Packages
        "deb" => "Debian Package",
        "rpm" => "RPM Package",
        "appimage" => "AppImage",
        "dmg" => "Disk Image",
        "iso" => "ISO Disk Image",
        "exe" => "Windows Executable",
        "msi" => "Windows Installer",
        // Fonts
        "ttf" => "TrueType Font",
        "otf" => "OpenType Font",
        "woff" | "woff2" => "Web Font",
        // Web
        "wasm" => "WebAssembly",
        "webmanifest" => "Web Manifest",
        // Other
        _ => {
            if ext.is_empty() {
                return "File".into();
            }
            return format!("{} File", ext.to_ascii_uppercase());
        }
    }
    .into()
}

pub(crate) fn build_info_panel_details(path: &Path) -> InfoPanelDetails {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return InfoPanelDetails::default(),
    };

    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_modified(d.as_secs()));
    let accessed = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_modified(d.as_secs()));
    let child_count = if meta.is_dir() {
        std::fs::read_dir(path)
            .ok()
            .map(|it| it.filter_map(Result::ok).count())
    } else {
        None
    };
    let extension = if meta.is_file() {
        Some(
            path.extension()
                .and_then(|e| e.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("none")
                .to_string(),
        )
    } else {
        None
    };
    let symlink_target = if meta.file_type().is_symlink() {
        std::fs::read_link(path)
            .ok()
            .map(|p| p.display().to_string())
    } else {
        None
    };

    #[cfg(unix)]
    let permissions = Some(format_unix_permissions(&meta));
    #[cfg(not(unix))]
    let permissions = None;

    #[cfg(unix)]
    let (owner, group) = (
        lookup_unix_user(meta.uid()).or_else(|| Some(meta.uid().to_string())),
        lookup_unix_group(meta.gid()).or_else(|| Some(meta.gid().to_string())),
    );
    #[cfg(not(unix))]
    let (owner, group) = (None, None);

    #[cfg(unix)]
    let (inode, device, links) = (Some(meta.ino()), Some(meta.dev()), Some(meta.nlink()));
    #[cfg(not(unix))]
    let (inode, device, links) = (None, None, None);

    let mut image_dimensions = None;
    let mut image_format = None;

    if meta.is_file()
        && matches!(
            ottrin_preview::classify_path(path),
            ottrin_preview::PreviewKind::Image
        )
    {
        if let Ok((w, h)) = image::image_dimensions(path) {
            image_dimensions = Some(format!("{w} × {h}px"));
        }
        if let Ok(fmt) = image::ImageFormat::from_path(path) {
            image_format = Some(format!("{:?}", fmt).to_ascii_uppercase());
        }
    }

    InfoPanelDetails {
        readonly: meta.permissions().readonly(),
        child_count,
        extension,
        created,
        accessed,
        permissions,
        owner,
        group,
        symlink_target,
        inode,
        device,
        links,
        image_dimensions,
        image_format,
    }
}

#[cfg(unix)]
fn format_unix_permissions(meta: &std::fs::Metadata) -> String {
    let mode = meta.mode() & 0o777;
    let mut out = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let part = ((mode >> shift) & 0o7) as u8;
        out.push(if part & 0o4 != 0 { 'r' } else { '-' });
        out.push(if part & 0o2 != 0 { 'w' } else { '-' });
        out.push(if part & 0o1 != 0 { 'x' } else { '-' });
    }
    format!("{out} ({mode:03o})")
}

#[cfg(unix)]
fn lookup_unix_user(uid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        let _passwd = parts.next()?;
        let uid_part = parts.next()?;
        if uid_part.parse::<u32>().ok()? == uid {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(unix)]
fn lookup_unix_group(gid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/group").ok()?;
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        let _passwd = parts.next()?;
        let gid_part = parts.next()?;
        if gid_part.parse::<u32>().ok()? == gid {
            return Some(name.to_string());
        }
    }
    None
}
