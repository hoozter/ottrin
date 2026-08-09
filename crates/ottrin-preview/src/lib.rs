use ottrin_core::{EntryKind, format_modified};
use rusqlite::config::DbConfig;
use rusqlite::hooks::{AuthAction, Authorization};
use rusqlite::limits::Limit;
use rusqlite::serialize::OwnedData;
use rusqlite::{Connection, DatabaseName, OpenFlags};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(unix)]
use std::ffi::{CStr, CString, OsStr, OsString};
#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use sha2::{Digest, Sha256};

mod affinity_visual;
mod mesh_visual;
mod obj_visual;
mod pdf_visual;
mod ply_visual;
mod stl_visual;
mod threemf_visual;
pub use affinity_visual::{
    AffinityPreviewSourceGeneration, AffinityVisualError, RenderedAffinityPreview,
    affinity_preview_is_available, affinity_preview_source_generation,
    render_affinity_embedded_preview_png, render_affinity_embedded_preview_png_with_generation,
};
pub use obj_visual::{
    ObjMetadata, ObjPreviewSourceGeneration, ObjVisualError, RenderedObjPreview,
    inspect_obj_metadata, obj_preview_source_generation, render_obj_preview_png,
    render_obj_preview_png_with_generation,
};
pub use pdf_visual::{
    PdfVisualError, pdfium_runtime_is_available, render_pdf_first_page_png,
    render_pdf_first_page_png_with_library, render_pdf_page_png, render_pdf_page_png_with_library,
};
pub use ply_visual::{
    PlyMetadata, PlyPreviewSourceGeneration, PlyVisualError, RenderedPlyPreview,
    inspect_ply_metadata, ply_preview_source_generation, render_ply_preview_png,
    render_ply_preview_png_with_generation,
};
pub use stl_visual::{
    StlPreviewSourceGeneration, StlVisualError, inspect_stl_metadata, render_stl_preview_png,
    stl_preview_source_generation,
};
pub use threemf_visual::{
    ThreeMfMetadata, ThreeMfPreviewSourceGeneration, ThreeMfVisualError, inspect_3mf_metadata,
    render_3mf_preview_png, render_3mf_preview_png_with_generation,
    threemf_preview_source_generation,
};

const EXTRACTED_TEXT_MARKER: &str = "\n\nExtracted text:\n";
const EXTRACTED_TEXT_LIMIT: usize = 24_000;
const MAX_OFFICE_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_OFFICE_PARTS: usize = 4_096;
const MAX_OFFICE_XML_BYTES: u64 = 8 * 1024 * 1024;
const MAX_OFFICE_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OFFICE_XML_DEPTH: usize = 64;
const PRESENTATIONML_NAMESPACE: &[u8] =
    b"http://schemas.openxmlformats.org/presentationml/2006/main";
const DRAWINGML_NAMESPACE: &[u8] = b"http://schemas.openxmlformats.org/drawingml/2006/main";
const RELATIONSHIPS_NAMESPACE: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/relationships";
const MAX_EPUB_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EPUB_PARTS: usize = 4_096;
const MAX_EPUB_PACKAGE_PATH_BYTES: usize = 1_024;
const MAX_EPUB_METADATA_CHARS: usize = 1_024;
const MAX_EPUB_CENTRAL_DIRECTORY_BYTES: u64 = 8 * 1024 * 1024;
const EPUB_CONTAINER_NAMESPACE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:container";
const EPUB_PACKAGE_NAMESPACE: &[u8] = b"http://www.idpf.org/2007/opf";
const EPUB_DC_NAMESPACE: &[u8] = b"http://purl.org/dc/elements/1.1/";
const MAX_SHARED_STRINGS: usize = 4_096;
const OFFICE_VISUAL_TEXT_LIMIT: usize = 4_096;
const MAX_TEXT_VISUAL_INPUT_BYTES: u64 = 128 * 1024;
const TEXT_VISUAL_LINE_LIMIT: usize = 18;
const TEXT_VISUAL_CHAR_LIMIT: usize = 1_600;
// SVG admission is deliberately structural rather than a small serialized-byte
// cliff. The snapshot cap still bounds allocation, while XML work caps bound
// the parser before resvg receives a tree.
const MAX_SVG_VISUAL_INPUT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SVG_XML_ELEMENTS: usize = 16 * 1024;
const MAX_SVG_XML_ATTRIBUTES: usize = 64 * 1024;
const MAX_SVG_XML_ATTRIBUTE_BYTES: usize = 1024 * 1024;
const MAX_SVG_XML_DEPTH: usize = 64;
const MAX_SVG_PATH_DATA_BYTES: usize = 1024 * 1024;
const MAX_SVG_LOCAL_PAINT_REFERENCES: usize = 512;
const MAX_SVG_FILTER_DEFINITIONS: usize = 4;
const MAX_SVG_FILTER_PRIMITIVES: usize = 8;
const MAX_SVG_FILTER_APPLICATIONS: usize = 128;
const MAX_SVG_FILTER_STD_DEVIATION: f32 = 16.0;
const MAX_SVG_PREVIEW_DIMENSION: u32 = 2048;
const MAX_SVG_PREVIEW_PIXELS: u64 = 4_194_304;
const MAX_CAMERA_RAW_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CAMERA_RAW_EMBEDDED_JPEG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CAMERA_RAW_EMBEDDED_JPEG_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_CAMERA_RAW_PREVIEW_DIMENSION: u32 = 4096;
const MAX_FONT_PREVIEW_PACKAGE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FONT_PREVIEW_DIMENSION: u32 = 2048;
const MAX_FONT_PREVIEW_PIXELS: u64 = 4_194_304;
const MAX_FONT_TEXT_GLYPHS: usize = 32;
const MAX_FONT_TABLES: usize = 128;
const MAX_FONT_NAME_TABLE_BYTES: usize = 64 * 1024;
const MAX_FONT_NAME_RECORDS: usize = 128;
const MAX_FONT_NAME_BYTES: usize = 512;
const MAX_FONT_NAME_CHARS: usize = 96;
const MAX_FONT_GLYPH_PIXELS: u64 = 256 * 256;
const MAX_FONT_TOTAL_GLYPH_PIXELS: u64 = 512 * 1024;
const MAX_FONT_OUTLINE_SEGMENTS: usize = 512;
const MAX_FONT_OUTLINE_RASTER_STEPS: u64 = 512 * 1024;
const MAX_FONT_SIMPLE_GLYPH_CONTOURS: usize = 64;
const MAX_FONT_SIMPLE_GLYPH_POINTS: usize = 256;
pub const MAX_SQLITE_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SQLITE_SCHEMA_ROWS: usize = 64;
const MAX_SQLITE_NAME_BYTES: usize = 128;
const MAX_SQLITE_OUTPUT_BYTES: usize = 4 * 1024;
const MAX_SQLITE_PREVIEW_DIMENSION: u32 = 2_048;
const MAX_SQLITE_PREVIEW_PIXELS: u64 = 4_194_304;
const MAX_SQLITE_PROGRESS_CALLBACKS: usize = 512;
const SQLITE_PROGRESS_OP_INTERVAL: i32 = 100;
const MAX_CAMERA_RAW_TIFF_IFDS: usize = 8;
const MAX_CAMERA_RAW_TIFF_ENTRIES_PER_IFD: usize = 256;
const MAX_CAMERA_RAW_TIFF_TOTAL_ENTRIES: usize = 512;
const MAX_TAR_COMPRESSED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TAR_SCAN_ENTRIES: usize = 4_096;
const MAX_TAR_SCAN_BYTES: u64 = 256 * 1024 * 1024;
const MAX_TAR_PATH_BYTES: usize = 768;
const MAX_TAR_LISTED_ENTRIES: usize = 200;
const MAX_FOLDER_RECURSIVE_ENTRIES: usize = 16_384;
#[cfg(unix)]
const MAX_FOLDER_RECURSIVE_DEPTH: usize = 64;

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
    #[error("Office preview source is not a regular file")]
    InputNotRegular,
    #[error(
        "Office visual previews are disabled on this platform until native safe-file admission is available"
    )]
    PreviewDisabledOnPlatform,
    #[error("Office input is too large for a visual preview ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read immutable Office package snapshot: {0}")]
    Read(String),
    #[error("Office package changed while its bounded snapshot was read")]
    InputChanged,
    #[error("could not open Office package: {0}")]
    Package(String),
    #[error("Office package has too many parts ({actual}; limit {limit})")]
    TooManyParts { actual: usize, limit: usize },
    #[error("Office document has no safe extractable content")]
    NoContent,
    #[error("could not encode Office visual preview: {0}")]
    Encode(String),
}

/// Content identity for a safely admitted Office package. Unlike path metadata,
/// this remains correct when a package is rewritten in place with restored mtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficePreviewSourceGeneration(pub(crate) [u8; 32]);

impl OfficePreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOfficePreview {
    pub png: Vec<u8>,
    pub source_generation: OfficePreviewSourceGeneration,
}

struct OfficeSnapshot {
    bytes: Vec<u8>,
    generation: OfficePreviewSourceGeneration,
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
    #[error("SVG exceeds the bounded visual-preview complexity policy: {0}")]
    ComplexityLimit(String),
    #[error("SVG contains blocked references (file/http/https image references are not permitted)")]
    BlockedReference,
    #[error("SVG could not be parsed for visual preview: {0}")]
    Parse(String),
    #[error("SVG visual preview could not be rasterized: {0}")]
    Rasterize(String),
}

#[derive(Debug, thiserror::Error)]
pub enum FontVisualError {
    #[error("format is not TTF/OTF")]
    UnsupportedFormat,
    #[error("could not inspect font input: {0}")]
    InputMetadata(String),
    #[error("Font input is too large for a bounded preview ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("Font input could not be read: {0}")]
    Read(String),
    #[error("font input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("TrueType collection fonts are not supported in this bounded preview")]
    UnsupportedCollection,
    #[error("font extension does not match its single-face SFNT container")]
    ExtensionMismatch,
    #[error("font table directory is unsafe: {0}")]
    InvalidTableDirectory(String),
    #[error("font contains an unsupported {tag:?} color, bitmap, or SVG surface")]
    UnsupportedSurface { tag: [u8; 4] },
    #[error("font could not be parsed for preview: {0}")]
    Parse(String),
    #[error("font outline work is unsafe: {0}")]
    UnsafeOutlineWork(String),
    #[error("font preview dimensions are unbounded ({dimension}px)")]
    UnboundedDimensions { dimension: u32 },
    #[error("font preview could not be encoded: {0}")]
    Encode(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SqliteVisualError {
    #[error("format is not a supported SQLite database candidate")]
    UnsupportedFormat,
    #[error("could not inspect SQLite input: {0}")]
    InputMetadata(String),
    #[error(
        "SQLite input is too large for a bounded preview ({actual} bytes; limit {limit} bytes)"
    )]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read SQLite input: {0}")]
    Read(String),
    #[error("SQLite input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("SQLite header is unsafe: {0}")]
    InvalidHeader(String),
    #[error("could not create an isolated SQLite snapshot: {0}")]
    Snapshot(String),
    #[error("SQLite schema metadata exceeds safe preview bounds: {0}")]
    SchemaBounds(String),
    #[error("could not query inert SQLite schema metadata: {0}")]
    Query(String),
    #[error("could not encode SQLite metadata card: {0}")]
    Encode(String),
}

/// A bounded SQLite card paired with the fingerprint captured from the same
/// open source handle that supplied its snapshot.
#[derive(Debug)]
pub struct RenderedSqlitePreview {
    pub png: Vec<u8>,
    pub source_generation: SqlitePreviewSourceGeneration,
}

/// Collision-resistant fingerprint of exact bounded SQLite snapshot bytes.
///
/// This deliberately is neither object identity nor a truncated hash: callers
/// use it to reject cache entries and asynchronous cards from older contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlitePreviewSourceGeneration([u8; 32]);

impl SqlitePreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

struct SqliteSourceSnapshot {
    bytes: Vec<u8>,
    source_generation: SqlitePreviewSourceGeneration,
}

struct SqliteInspection {
    metadata: SqliteMetadata,
    source_generation: SqlitePreviewSourceGeneration,
}

#[derive(Debug, Clone, Copy)]
struct FontTableRecord {
    tag: [u8; 4],
    offset: usize,
    length: usize,
}

#[derive(Debug, Clone, Copy)]
enum FontOutlineCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Default)]
struct BoundedFontOutline {
    commands: Vec<FontOutlineCommand>,
    limit_exceeded: bool,
}

impl BoundedFontOutline {
    fn push(&mut self, command: FontOutlineCommand) {
        if self.commands.len() >= MAX_FONT_OUTLINE_SEGMENTS {
            self.limit_exceeded = true;
            return;
        }
        self.commands.push(command);
    }
}

impl ttf_parser::OutlineBuilder for BoundedFontOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push(FontOutlineCommand::MoveTo(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(FontOutlineCommand::LineTo(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push(FontOutlineCommand::QuadTo(x1, y1, x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push(FontOutlineCommand::CurveTo(x1, y1, x2, y2, x, y));
    }

    fn close(&mut self) {
        self.push(FontOutlineCommand::Close);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PreviewKind {
    Text,
    Image,
    FolderMetadata,
    FontMetadata,
    SqliteMetadata,
    StlMetadata,
    ObjMetadata,
    PlyMetadata,
    ThreeMfMetadata,
    Pdf,
    VideoMetadata,
    AudioMetadata,
    ArchiveMetadata,
    CameraRawMetadata,
    AffinityMetadata,
    OfficeMetadata,
    EpsMetadata,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub kind_hint: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetPreview {
    pub sheets: Vec<SpreadsheetSheet>,
    pub omissions: Vec<SpreadsheetOmission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetSheet {
    pub id: String,
    pub name: String,
    pub cells: Vec<SpreadsheetCell>,
    pub omissions: Vec<SpreadsheetOmission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetCell {
    pub row: usize,
    pub column: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpreadsheetOmission {
    pub reason: SpreadsheetOmissionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpreadsheetOmissionReason {
    SparseRowGap {
        from: usize,
        to: usize,
    },
    SparseColumnGap {
        from: usize,
        to: usize,
    },
    SheetLimit {
        omitted: usize,
    },
    /// Inclusive zero-based coordinate range outside the preview's row bound.
    RowLimit {
        from: usize,
        to: usize,
    },
    /// Inclusive zero-based coordinate range outside the preview's column bound.
    ColumnLimit {
        from: usize,
        to: usize,
    },
    CellLimit {
        omitted: usize,
    },
    TextLimit {
        omitted: usize,
    },
    /// A shared-string cell was intentionally withheld rather than displaying
    /// its numeric index as though it were user content.
    SharedStringUnavailable {
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewData {
    pub title: String,
    pub kind: PreviewKind,
    pub body: String,
    #[serde(default)]
    pub spreadsheet: Option<SpreadsheetPreview>,
    pub open_external: bool,
}

/// True only when this is a complete, directly-read text preview rather than a
/// bounded/truncated display card. Callers must still take a fresh source
/// snapshot before permitting mutation.
pub fn is_complete_text_preview(preview: &PreviewData) -> bool {
    preview.kind == PreviewKind::Text
        && preview
            .body
            .split_once(EXTRACTED_TEXT_MARKER)
            .is_some_and(|(metadata, _)| metadata.lines().any(|line| line == "Preview: Complete"))
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
    Font,
    Sqlite,
    Stl,
    Obj,
    Ply,
    ThreeMf,
    CameraRaw,
    Affinity,
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
        PreviewKind::FontMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Font,
            raw_fallback_allowed: false,
        },
        PreviewKind::SqliteMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Sqlite,
            raw_fallback_allowed: false,
        },
        PreviewKind::StlMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Stl,
            raw_fallback_allowed: false,
        },
        PreviewKind::ObjMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Obj,
            raw_fallback_allowed: false,
        },
        PreviewKind::PlyMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Ply,
            raw_fallback_allowed: false,
        },
        PreviewKind::ThreeMfMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::ThreeMf,
            raw_fallback_allowed: false,
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
            // A video container is never image data. Until the bounded native
            // backend produces a validated still, keep it metadata-only rather
            // than handing arbitrary container bytes to an image decoder.
            raw_fallback_allowed: false,
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
        PreviewKind::CameraRawMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::CameraRaw,
            raw_fallback_allowed: false,
        },
        PreviewKind::AffinityMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Affinity,
            raw_fallback_allowed: false,
        },
        PreviewKind::OfficeMetadata => PreviewProfile {
            kind,
            views: [true, false, true],
            visual_renderer: PreviewVisualRenderer::Office,
            raw_fallback_allowed: false,
        },
        // EPS is PostScript. This candidate has no vetted isolated renderer,
        // so it exposes metadata and an actionable unavailable state rather
        // than raw bytes or an in-process interpreter.
        PreviewKind::EpsMetadata => PreviewProfile {
            kind,
            views: [true, false, false],
            visual_renderer: PreviewVisualRenderer::None,
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
    let mut profile = profile_from_kind(classify_path(path));
    if is_epub_path(path) {
        // EPUB is deliberately metadata-only. Its package bytes must never be
        // offered to an image decoder or displayed through the raw fallback.
        profile.raw_fallback_allowed = false;
    }
    if is_camera_raw_path(path) {
        // Camera RAW containers are never handed to the generic image decoder:
        // only a bounded, verified embedded JPEG may reach the visual surface.
        profile.raw_fallback_allowed = false;
    }
    if is_affinity_path(path) {
        // Affinity containers are never handed to a generic image decoder.
        // A dedicated bounded parser may expose only a validated embedded PNG.
        profile.raw_fallback_allowed = false;
    }
    if is_font_path(path) {
        // Font containers are always visualized through the dedicated preview
        // renderer; they do not fall back to raw bytes.
        profile.raw_fallback_allowed = false;
    }
    if is_stl_path(path) {
        // Mesh source bytes never reach image decoding or the generic fallback.
        profile.raw_fallback_allowed = false;
    }
    if is_obj_path(path) {
        // OBJ source is parsed by a bounded, in-memory mesh renderer only.
        profile.raw_fallback_allowed = false;
    }
    if is_ply_path(path) {
        // PLY source bytes are only handled by the bounded immutable-snapshot
        // mesh parser; they never reach an image decoder or raw fallback.
        profile.raw_fallback_allowed = false;
    }
    if is_3mf_path(path) {
        // 3MF package bytes only reach the dedicated bounded parser.
        profile.raw_fallback_allowed = false;
    }
    profile
}

pub fn is_camera_raw_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "dng" | "arw"))
}

pub fn is_affinity_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "afphoto" | "afdesign" | "afpub"
            )
        })
}

pub fn is_font_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "ttf" | "otf"))
}

pub fn is_stl_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("stl"))
}

pub fn is_obj_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("obj"))
}

pub fn is_ply_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ply"))
}

pub fn is_3mf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("3mf"))
}

pub fn is_epub_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"))
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
        PreviewKind::FontMetadata => preview_font(title, &request.path),
        PreviewKind::SqliteMetadata => preview_sqlite(title, &request.path),
        PreviewKind::StlMetadata => preview_stl(title, &request.path),
        PreviewKind::ObjMetadata => preview_obj(title, &request.path),
        PreviewKind::PlyMetadata => preview_ply(title, &request.path),
        PreviewKind::ThreeMfMetadata => preview_3mf(title, &request.path),
        PreviewKind::Pdf => preview_pdf(title, &request.path),
        PreviewKind::VideoMetadata => preview_video(title, &request.path),
        PreviewKind::AudioMetadata => preview_audio(title, &request.path),
        PreviewKind::ArchiveMetadata => preview_archive(title, &request.path),
        PreviewKind::CameraRawMetadata => preview_camera_raw(title, &request.path),
        PreviewKind::AffinityMetadata => preview_affinity(title, &request.path),
        PreviewKind::OfficeMetadata => preview_office(title, &request.path),
        PreviewKind::EpsMetadata => preview_eps(title, &request.path),
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
        "ttf" | "otf" => PreviewKind::FontMetadata,
        "sqlite" | "sqlite3" => PreviewKind::SqliteMetadata,
        "stl" => PreviewKind::StlMetadata,
        "obj" => PreviewKind::ObjMetadata,
        "ply" => PreviewKind::PlyMetadata,
        "3mf" => PreviewKind::ThreeMfMetadata,
        "db" if has_sqlite_header(path) => PreviewKind::SqliteMetadata,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "ico" | "svg" => {
            PreviewKind::Image
        }
        "eps" => PreviewKind::EpsMetadata,
        "dng" | "arw" => PreviewKind::CameraRawMetadata,
        "afphoto" | "afdesign" | "afpub" => PreviewKind::AffinityMetadata,
        "pdf" => PreviewKind::Pdf,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => PreviewKind::VideoMetadata,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => PreviewKind::AudioMetadata,
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" | "epub" => PreviewKind::ArchiveMetadata,
        "doc" | "docx" | "xls" | "xlsx" | "xlsm" | "ppt" | "pptx" | "odt" | "ods" | "odp" => {
            PreviewKind::OfficeMetadata
        }
        _ => PreviewKind::Unsupported,
    }
}

fn sqlite_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

/// Returns whether a path has an extension reserved for bounded SQLite
/// metadata preview handling. Callers must apply this before any generic cache
/// or raw-source fallback, because `.db` is otherwise content-classified only
/// after opening the source.
pub fn is_sqlite_preview_candidate(path: &Path) -> bool {
    matches!(
        sqlite_extension(path).as_deref(),
        Some("sqlite" | "sqlite3" | "db")
    )
}

/// Every SQLite preview candidate is content-bound to the SQLite signature
/// before cached cards or the renderer can use it. This is especially
/// important for generic `.db` files, but also prevents a stale card being
/// reused after a `.sqlite`/`.sqlite3` path changes type.
pub fn is_sqlite_preview_path(path: &Path) -> bool {
    is_sqlite_preview_candidate(path) && has_sqlite_header(path)
}

fn has_sqlite_header(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && header == *b"SQLite format 3\0"
}

#[derive(Debug, Default)]
struct SqliteMetadata {
    tables: Vec<String>,
    indexes: Vec<String>,
    views: Vec<String>,
    triggers: Vec<String>,
}

impl SqliteMetadata {
    fn push(&mut self, kind: &str, name: String) -> Result<(), SqliteVisualError> {
        if name.len() > MAX_SQLITE_NAME_BYTES
            || name.chars().any(|character| character.is_control())
        {
            return Err(SqliteVisualError::SchemaBounds(
                "schema object name is not safe for the bounded card".to_string(),
            ));
        }
        let destination = match kind {
            "table" => &mut self.tables,
            "index" => &mut self.indexes,
            "view" => &mut self.views,
            "trigger" => &mut self.triggers,
            _ => {
                return Err(SqliteVisualError::SchemaBounds(
                    "unexpected schema object kind".into(),
                ));
            }
        };
        destination.push(name);
        if self.total_count() > MAX_SQLITE_SCHEMA_ROWS {
            return Err(SqliteVisualError::SchemaBounds(
                "schema row count exceeds preview limit".to_string(),
            ));
        }
        Ok(())
    }

    fn total_count(&self) -> usize {
        self.tables.len() + self.indexes.len() + self.views.len() + self.triggers.len()
    }

    fn card_text(&self) -> Result<String, SqliteVisualError> {
        let mut lines = vec![
            "SQLite database".to_string(),
            format!("Tables: {}", self.tables.len()),
            format!("Indexes: {}", self.indexes.len()),
            format!("Views: {}", self.views.len()),
            format!("Triggers: {}", self.triggers.len()),
        ];
        for (label, names) in [
            ("Tables", &self.tables),
            ("Indexes", &self.indexes),
            ("Views", &self.views),
            ("Triggers", &self.triggers),
        ] {
            if !names.is_empty() {
                lines.push(format!("{label}: {}", names.join(", ")));
            }
        }
        let text = lines.join("\n");
        (text.len() <= MAX_SQLITE_OUTPUT_BYTES)
            .then_some(text)
            .ok_or_else(|| {
                SqliteVisualError::SchemaBounds("metadata card text exceeds output limit".into())
            })
    }
}

/// Renders only inert SQLite schema metadata from an owned in-memory snapshot.
/// It never opens the source database with SQLite, follows WAL state, runs
/// schema SQL, or exposes row contents to the preview surface.
pub fn render_sqlite_metadata_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, SqliteVisualError> {
    Ok(render_sqlite_metadata_preview_png_with_generation(path, target_dim)?.png)
}

/// Renders a SQLite card with the identity captured from the exact file
/// handle used for its bounded snapshot. Queueing callers must compare this
/// generation to the one they requested before accepting the card.
pub fn render_sqlite_metadata_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedSqlitePreview, SqliteVisualError> {
    let inspection = inspect_sqlite_snapshot(path)?;
    let text = inspection.metadata.card_text()?;
    let dimension = target_dim.clamp(64, MAX_SQLITE_PREVIEW_DIMENSION);
    if u64::from(dimension).saturating_mul(u64::from(dimension)) > MAX_SQLITE_PREVIEW_PIXELS {
        return Err(SqliteVisualError::Encode(
            "requested card dimensions exceed preview bounds".to_string(),
        ));
    }
    let mut image =
        image::RgbaImage::from_pixel(dimension, dimension, image::Rgba([247, 249, 252, 255]));
    let margin = (dimension / 10).clamp(6, 96);
    let header_bottom = margin + (dimension / 5).clamp(24, 160);
    fill_rect(
        &mut image,
        0,
        0,
        dimension,
        header_bottom,
        [53, 103, 151, 255],
    );
    render_document_text(
        &mut image,
        "SQLite metadata\nRead-only snapshot",
        margin,
        margin,
        dimension - margin,
        header_bottom.saturating_sub(margin / 3),
    );
    render_document_text(
        &mut image,
        &text,
        margin,
        header_bottom + margin / 2,
        dimension - margin,
        dimension.saturating_sub(margin),
    );
    let mut png = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error: image::ImageError| SqliteVisualError::Encode(error.to_string()))?;
    Ok(RenderedSqlitePreview {
        png,
        source_generation: inspection.source_generation,
    })
}

fn inspect_sqlite_metadata(path: &Path) -> Result<SqliteMetadata, SqliteVisualError> {
    Ok(inspect_sqlite_snapshot(path)?.metadata)
}

fn inspect_sqlite_snapshot(path: &Path) -> Result<SqliteInspection, SqliteVisualError> {
    if !is_sqlite_preview_candidate(path) {
        return Err(SqliteVisualError::UnsupportedFormat);
    }
    let source = read_bounded_sqlite_snapshot(path)?;
    if sqlite_extension(path).as_deref() == Some("db")
        && source.bytes.get(..16) != Some(b"SQLite format 3\0")
    {
        return Err(SqliteVisualError::UnsupportedFormat);
    }
    validate_sqlite_header(&source.bytes)?;

    let mut connection = Connection::open_in_memory_with_flags(
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE,
    )
    .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    configure_sqlite_preview_connection(&connection)?;
    connection
        .deserialize(DatabaseName::Main, sqlite_owned_data(source.bytes)?, true)
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;

    let callbacks = Arc::new(AtomicUsize::new(0));
    let progress_callbacks = Arc::clone(&callbacks);
    connection.progress_handler(
        SQLITE_PROGRESS_OP_INTERVAL,
        Some(move || {
            progress_callbacks.fetch_add(1, Ordering::Relaxed) >= MAX_SQLITE_PROGRESS_CALLBACKS
        }),
    );
    connection.authorizer(Some(
        |context: rusqlite::hooks::AuthContext<'_>| match context.action {
            AuthAction::Select => Authorization::Allow,
            AuthAction::Read { table_name, .. }
                if matches!(table_name, "sqlite_master" | "sqlite_schema")
                    && context.accessor.is_none() =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        },
    ));

    let query = "SELECT type, name FROM sqlite_schema \
        WHERE type IN ('table', 'index', 'view', 'trigger') \
        ORDER BY type, name LIMIT 65";
    let mut statement = connection
        .prepare(query)
        .map_err(|error| SqliteVisualError::Query(error.to_string()))?;
    let mut rows = statement
        .query([])
        .map_err(|error| SqliteVisualError::Query(error.to_string()))?;
    let mut metadata = SqliteMetadata::default();
    while let Some(row) = rows
        .next()
        .map_err(|error| SqliteVisualError::Query(error.to_string()))?
    {
        let kind: String = row
            .get(0)
            .map_err(|error| SqliteVisualError::Query(error.to_string()))?;
        let name: String = row
            .get(1)
            .map_err(|error| SqliteVisualError::Query(error.to_string()))?;
        if !name.starts_with("sqlite_") {
            metadata.push(&kind, name)?;
        }
    }
    drop(rows);
    drop(statement);
    connection.authorizer(None::<fn(rusqlite::hooks::AuthContext<'_>) -> Authorization>);
    connection.progress_handler(0, None::<fn() -> bool>);
    (callbacks.load(Ordering::Relaxed) <= MAX_SQLITE_PROGRESS_CALLBACKS)
        .then_some(SqliteInspection {
            metadata,
            source_generation: source.source_generation,
        })
        .ok_or_else(|| {
            SqliteVisualError::SchemaBounds("schema query exceeded progress limit".into())
        })
}

fn configure_sqlite_preview_connection(connection: &Connection) -> Result<(), SqliteVisualError> {
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, MAX_SQLITE_OUTPUT_BYTES as i32),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 512),
        (Limit::SQLITE_LIMIT_COLUMN, 8),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 8),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 1),
        (Limit::SQLITE_LIMIT_VDBE_OP, 4_096),
        (Limit::SQLITE_LIMIT_FUNCTION_ARG, 0),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 0),
        (Limit::SQLITE_LIMIT_TRIGGER_DEPTH, 0),
        (Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        connection.set_limit(limit, value);
    }
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false)
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FTS3_TOKENIZER, false)
        .map_err(|error| SqliteVisualError::Snapshot(error.to_string()))?;
    Ok(())
}

/// Returns the collision-resistant fingerprint of one bounded source snapshot.
///
/// This probe uses the same one-handle, 64 MiB plus sentinel bound as
/// rendering and fails closed for unreadable, growing, or oversized sources.
pub fn sqlite_preview_source_generation(path: &Path) -> Option<SqlitePreviewSourceGeneration> {
    read_bounded_sqlite_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.source_generation)
}

fn read_bounded_sqlite_snapshot(path: &Path) -> Result<SqliteSourceSnapshot, SqliteVisualError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| SqliteVisualError::InputMetadata(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| SqliteVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_SQLITE_PREVIEW_BYTES {
        return Err(SqliteVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_SQLITE_PREVIEW_BYTES,
        });
    }
    let capacity = usize::try_from(input_size).map_err(|_| SqliteVisualError::InputTooLarge {
        actual: input_size,
        limit: MAX_SQLITE_PREVIEW_BYTES,
    })?;
    let mut source = Vec::with_capacity(capacity);
    file.by_ref()
        .take(MAX_SQLITE_PREVIEW_BYTES + 1)
        .read_to_end(&mut source)
        .map_err(|error| SqliteVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| SqliteVisualError::Read(error.to_string()))?
        .len();
    if source.len() as u64 > MAX_SQLITE_PREVIEW_BYTES {
        return Err(SqliteVisualError::InputTooLarge {
            actual: source.len() as u64,
            limit: MAX_SQLITE_PREVIEW_BYTES,
        });
    }
    if source.len() as u64 != input_size || final_size != input_size {
        return Err(SqliteVisualError::InputChanged);
    }
    Ok(SqliteSourceSnapshot {
        source_generation: SqlitePreviewSourceGeneration(Sha256::digest(&source).into()),
        bytes: source,
    })
}

fn validate_sqlite_header(source: &[u8]) -> Result<(), SqliteVisualError> {
    let header = source.get(..100).ok_or_else(|| {
        SqliteVisualError::InvalidHeader("truncated 100-byte database header".into())
    })?;
    if &header[..16] != b"SQLite format 3\0" {
        return Err(SqliteVisualError::InvalidHeader(
            "missing SQLite format 3 signature".to_string(),
        ));
    }
    let encoded_page_size = u16::from_be_bytes([header[16], header[17]]);
    let page_size = if encoded_page_size == 1 {
        65_536u64
    } else {
        u64::from(encoded_page_size)
    };
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return Err(SqliteVisualError::InvalidHeader(
            "page size is outside SQLite preview bounds".to_string(),
        ));
    }
    if header[18] != 1 || header[19] != 1 {
        return Err(SqliteVisualError::InvalidHeader(
            "WAL-dependent or non-rollback journal mode is not previewed".to_string(),
        ));
    }
    if header[20] != 0 {
        return Err(SqliteVisualError::InvalidHeader(
            "reserved page space is not supported by the bounded preview".to_string(),
        ));
    }
    if header[21..24] != [64, 32, 32] {
        return Err(SqliteVisualError::InvalidHeader(
            "payload fractions do not match SQLite format 3".to_string(),
        ));
    }
    if header[72..92].iter().any(|&byte| byte != 0) {
        return Err(SqliteVisualError::InvalidHeader(
            "reserved SQLite header bytes must be zero".to_string(),
        ));
    }
    let schema_format = u32::from_be_bytes([header[44], header[45], header[46], header[47]]);
    if !(1..=4).contains(&schema_format) {
        return Err(SqliteVisualError::InvalidHeader(
            "schema format is not a supported SQLite version".to_string(),
        ));
    }
    let text_encoding = u32::from_be_bytes([header[56], header[57], header[58], header[59]]);
    if !(1..=3).contains(&text_encoding) {
        return Err(SqliteVisualError::InvalidHeader(
            "text encoding is not a supported SQLite value".to_string(),
        ));
    }
    if !(source.len() as u64).is_multiple_of(page_size) {
        return Err(SqliteVisualError::InvalidHeader(
            "database size is not an exact page multiple".to_string(),
        ));
    }
    let page_count = u64::from(u32::from_be_bytes([
        header[28], header[29], header[30], header[31],
    ]));
    if page_count == 0
        || page_count
            .checked_mul(page_size)
            .filter(|expected| *expected == source.len() as u64)
            .is_none()
    {
        return Err(SqliteVisualError::InvalidHeader(
            "page count does not match bounded snapshot length".to_string(),
        ));
    }
    Ok(())
}

fn sqlite_owned_data(source: Vec<u8>) -> Result<OwnedData, SqliteVisualError> {
    let length = source.len();
    // SQLite owns the returned allocation and accepts this bounded nonzero
    // size; a null result is handled below before it can be dereferenced.
    let allocation = unsafe { rusqlite::ffi::sqlite3_malloc64(length as u64) }.cast::<u8>();
    let pointer = NonNull::new(allocation)
        .ok_or_else(|| SqliteVisualError::Snapshot("SQLite snapshot allocation failed".into()))?;
    // `OwnedData` requires SQLite-owned allocation because SQLite frees it on
    // connection close. The bounded Vec is copied once into that allocator,
    // after which the source path is no longer touched by SQLite.
    // The source and destination are distinct valid allocations of `length`
    // bytes, and `OwnedData` documents this SQLite-allocated ownership model.
    unsafe {
        std::ptr::copy_nonoverlapping(source.as_ptr(), pointer.as_ptr(), length);
        Ok(OwnedData::from_raw_nonnull(pointer, length))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CameraRawVisualError {
    #[error("camera RAW visual previews support only DNG and ARW files")]
    UnsupportedFormat,
    #[error("could not read camera RAW metadata: {0}")]
    InputMetadata(String),
    #[error(
        "camera RAW file is too large for bounded preview parsing ({actual} bytes; limit {limit})"
    )]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read camera RAW container: {0}")]
    Read(String),
    #[error("camera RAW file changed while its bounded snapshot was read")]
    InputChanged,
    #[error("camera RAW container is not a supported TIFF-based RAW file: {0}")]
    Container(String),
    #[error("camera RAW container has no unambiguous embedded JPEG")]
    NoEmbeddedJpeg,
    #[error("camera RAW embedded JPEG is too large ({actual} bytes; limit {limit})")]
    EmbeddedJpegTooLarge { actual: u64, limit: u64 },
    #[error("camera RAW embedded JPEG range is invalid")]
    EmbeddedJpegRange,
    #[error("camera RAW embedded JPEG is not a valid bounded JPEG: {0}")]
    EmbeddedJpeg(String),
    #[error("could not encode camera RAW visual preview: {0}")]
    Encode(String),
}

/// Extracts only a classic TIFF/Exif JPEG thumbnail from DNG or ARW. The RAW
/// sensor payload is never decoded or passed to an image renderer.
pub fn render_camera_raw_embedded_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, CameraRawVisualError> {
    if !is_camera_raw_path(path) {
        return Err(CameraRawVisualError::UnsupportedFormat);
    }
    let container = read_camera_raw_container(path)?;
    let jpeg = camera_raw_embedded_jpeg(&container)?;
    validate_exact_jpeg(jpeg)?;
    let reader = image::ImageReader::with_format(Cursor::new(jpeg), image::ImageFormat::Jpeg);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| CameraRawVisualError::EmbeddedJpeg(error.to_string()))?;
    if width > MAX_CAMERA_RAW_PREVIEW_DIMENSION
        || height > MAX_CAMERA_RAW_PREVIEW_DIMENSION
        || u64::from(width)
            .checked_mul(u64::from(height))
            .is_none_or(|pixels| pixels > MAX_CAMERA_RAW_EMBEDDED_JPEG_PIXELS)
    {
        return Err(CameraRawVisualError::EmbeddedJpeg(
            "embedded dimensions exceed preview bounds".to_string(),
        ));
    }
    let image = image::ImageReader::with_format(Cursor::new(jpeg), image::ImageFormat::Jpeg)
        .decode()
        .map_err(|error| CameraRawVisualError::EmbeddedJpeg(error.to_string()))?;
    let dimension = target_dim.clamp(64, 2_048);
    let thumbnail = image.thumbnail(dimension, dimension);
    let mut png = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| CameraRawVisualError::Encode(error.to_string()))?;
    Ok(png)
}

fn read_camera_raw_container(path: &Path) -> Result<Vec<u8>, CameraRawVisualError> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| CameraRawVisualError::InputMetadata(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| CameraRawVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_CAMERA_RAW_PACKAGE_BYTES {
        return Err(CameraRawVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_CAMERA_RAW_PACKAGE_BYTES,
        });
    }
    let capacity =
        usize::try_from(input_size).map_err(|_| CameraRawVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_CAMERA_RAW_PACKAGE_BYTES,
        })?;
    let mut container = Vec::with_capacity(capacity);
    file.by_ref()
        .take(MAX_CAMERA_RAW_PACKAGE_BYTES + 1)
        .read_to_end(&mut container)
        .map_err(|error| CameraRawVisualError::Read(error.to_string()))?;
    if container.len() as u64 > MAX_CAMERA_RAW_PACKAGE_BYTES {
        return Err(CameraRawVisualError::InputTooLarge {
            actual: container.len() as u64,
            limit: MAX_CAMERA_RAW_PACKAGE_BYTES,
        });
    }
    if container.len() as u64 != input_size {
        return Err(CameraRawVisualError::InputChanged);
    }
    Ok(container)
}

#[derive(Clone, Copy)]
enum TiffByteOrder {
    Little,
    Big,
}

impl TiffByteOrder {
    fn u16(self, bytes: &[u8], offset: usize) -> Option<u16> {
        let value = bytes.get(offset..offset.checked_add(2)?)?;
        Some(match self {
            Self::Little => u16::from_le_bytes([value[0], value[1]]),
            Self::Big => u16::from_be_bytes([value[0], value[1]]),
        })
    }

    fn u32(self, bytes: &[u8], offset: usize) -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?;
        Some(match self {
            Self::Little => u32::from_le_bytes([value[0], value[1], value[2], value[3]]),
            Self::Big => u32::from_be_bytes([value[0], value[1], value[2], value[3]]),
        })
    }
}

fn camera_raw_embedded_jpeg(container: &[u8]) -> Result<&[u8], CameraRawVisualError> {
    let byte_order = match container.get(..2) {
        Some(b"II") => TiffByteOrder::Little,
        Some(b"MM") => TiffByteOrder::Big,
        _ => {
            return Err(CameraRawVisualError::Container(
                "missing classic TIFF byte order".into(),
            ));
        }
    };
    if byte_order.u16(container, 2) != Some(42) {
        return Err(CameraRawVisualError::Container(
            "missing classic TIFF marker".into(),
        ));
    }
    let mut next_ifd = usize::try_from(
        byte_order
            .u32(container, 4)
            .ok_or_else(|| CameraRawVisualError::Container("truncated TIFF header".into()))?,
    )
    .map_err(|_| CameraRawVisualError::Container("IFD offset exceeds address space".into()))?;
    let mut seen_ifds = Vec::new();
    let mut metadata_ranges = Vec::with_capacity(MAX_CAMERA_RAW_TIFF_IFDS + 1);
    metadata_ranges.push(0..8);
    let mut total_entries = 0usize;
    let mut candidate = None;

    while next_ifd != 0 {
        if seen_ifds.len() == MAX_CAMERA_RAW_TIFF_IFDS || seen_ifds.contains(&next_ifd) {
            return Err(CameraRawVisualError::Container(
                "IFD chain exceeds safe bounds".into(),
            ));
        }
        if next_ifd < 8 {
            return Err(CameraRawVisualError::Container(
                "IFD overlaps TIFF header".into(),
            ));
        }
        seen_ifds.push(next_ifd);
        let entry_count =
            usize::from(byte_order.u16(container, next_ifd).ok_or_else(|| {
                CameraRawVisualError::Container("truncated IFD entry count".into())
            })?);
        if entry_count > MAX_CAMERA_RAW_TIFF_ENTRIES_PER_IFD
            || total_entries.saturating_add(entry_count) > MAX_CAMERA_RAW_TIFF_TOTAL_ENTRIES
        {
            return Err(CameraRawVisualError::Container(
                "IFD entries exceed safe bounds".into(),
            ));
        }
        total_entries += entry_count;
        let entries_offset = next_ifd
            .checked_add(2)
            .ok_or_else(|| CameraRawVisualError::Container("IFD offset overflow".into()))?;
        let entries_bytes = entry_count
            .checked_mul(12)
            .ok_or_else(|| CameraRawVisualError::Container("IFD entry size overflow".into()))?;
        let next_offset = entries_offset
            .checked_add(entries_bytes)
            .ok_or_else(|| CameraRawVisualError::Container("IFD range overflow".into()))?;
        let ifd_end = next_offset
            .checked_add(4)
            .ok_or_else(|| CameraRawVisualError::Container("IFD range overflow".into()))?;
        if ifd_end > container.len() {
            return Err(CameraRawVisualError::Container(
                "truncated IFD entries".into(),
            ));
        }
        metadata_ranges.push(next_ifd..ifd_end);

        let mut jpeg_offset = None;
        let mut jpeg_length = None;
        let mut has_strip_or_tile_fields = false;
        for index in 0..entry_count {
            let offset = entries_offset + index * 12;
            let tag = byte_order
                .u16(container, offset)
                .expect("validated IFD entry");
            let field_type = byte_order
                .u16(container, offset + 2)
                .expect("validated IFD entry");
            let count = byte_order
                .u32(container, offset + 4)
                .expect("validated IFD entry");
            let value = byte_order
                .u32(container, offset + 8)
                .expect("validated IFD entry");
            match tag {
                0x0201 => {
                    if field_type != 4 || count != 1 || jpeg_offset.replace(value).is_some() {
                        return Err(CameraRawVisualError::NoEmbeddedJpeg);
                    }
                }
                0x0202 => {
                    if field_type != 4 || count != 1 || jpeg_length.replace(value).is_some() {
                        return Err(CameraRawVisualError::NoEmbeddedJpeg);
                    }
                }
                0x0111 | 0x0117 | 0x0144 | 0x0145 => has_strip_or_tile_fields = true,
                _ => {}
            }
        }
        match (jpeg_offset, jpeg_length) {
            (None, None) => {}
            (Some(offset), Some(length)) if !has_strip_or_tile_fields => {
                if candidate
                    .replace((u64::from(offset), u64::from(length)))
                    .is_some()
                {
                    return Err(CameraRawVisualError::NoEmbeddedJpeg);
                }
            }
            _ => return Err(CameraRawVisualError::NoEmbeddedJpeg),
        }
        next_ifd = usize::try_from(
            byte_order
                .u32(container, next_offset)
                .expect("validated IFD next offset"),
        )
        .map_err(|_| CameraRawVisualError::Container("next IFD exceeds address space".into()))?;
    }

    let Some((offset, length)) = candidate else {
        return Err(CameraRawVisualError::NoEmbeddedJpeg);
    };
    if length == 0 || length > MAX_CAMERA_RAW_EMBEDDED_JPEG_BYTES {
        return Err(CameraRawVisualError::EmbeddedJpegTooLarge {
            actual: length,
            limit: MAX_CAMERA_RAW_EMBEDDED_JPEG_BYTES,
        });
    }
    let end = offset
        .checked_add(length)
        .ok_or(CameraRawVisualError::EmbeddedJpegRange)?;
    let start = usize::try_from(offset).map_err(|_| CameraRawVisualError::EmbeddedJpegRange)?;
    let end = usize::try_from(end).map_err(|_| CameraRawVisualError::EmbeddedJpegRange)?;
    if metadata_ranges
        .iter()
        .any(|range| start < range.end && range.start < end)
    {
        return Err(CameraRawVisualError::EmbeddedJpegRange);
    }
    container
        .get(start..end)
        .ok_or(CameraRawVisualError::EmbeddedJpegRange)
}

fn validate_exact_jpeg(jpeg: &[u8]) -> Result<(), CameraRawVisualError> {
    if jpeg.get(..2) != Some(b"\xff\xd8") {
        return Err(CameraRawVisualError::EmbeddedJpeg(
            "missing JPEG start marker".into(),
        ));
    }
    let mut position = 2usize;
    let mut saw_scan = false;
    let mut in_entropy_data = false;
    loop {
        let marker = if in_entropy_data {
            let Some(relative) = jpeg[position..].iter().position(|byte| *byte == 0xff) else {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "missing JPEG end marker".into(),
                ));
            };
            position += relative + 1;
            while jpeg.get(position) == Some(&0xff) {
                position += 1;
            }
            let Some(marker) = jpeg.get(position).copied() else {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "truncated JPEG marker".into(),
                ));
            };
            position += 1;
            match marker {
                0x00 | 0xd0..=0xd7 => continue,
                0xd9 => {
                    return if position == jpeg.len() {
                        Ok(())
                    } else {
                        Err(CameraRawVisualError::EmbeddedJpeg(
                            "JPEG contains trailing bytes".into(),
                        ))
                    };
                }
                marker => {
                    in_entropy_data = false;
                    marker
                }
            }
        } else {
            if jpeg.get(position) != Some(&0xff) {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "invalid JPEG marker stream".into(),
                ));
            }
            position += 1;
            while jpeg.get(position) == Some(&0xff) {
                position += 1;
            }
            let Some(marker) = jpeg.get(position).copied() else {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "truncated JPEG marker".into(),
                ));
            };
            position += 1;
            marker
        };
        match marker {
            0xd9 if saw_scan && position == jpeg.len() => return Ok(()),
            0xd9 => {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "unexpected JPEG end marker".into(),
                ));
            }
            0xd8 | 0x00 | 0xd0..=0xd7 => {
                return Err(CameraRawVisualError::EmbeddedJpeg(
                    "invalid JPEG marker".into(),
                ));
            }
            0x01 => {}
            _ => {
                let length_bytes =
                    jpeg.get(position..position.saturating_add(2))
                        .ok_or_else(|| {
                            CameraRawVisualError::EmbeddedJpeg("truncated JPEG segment".into())
                        })?;
                let length = u16::from_be_bytes([length_bytes[0], length_bytes[1]]);
                if length < 2 {
                    return Err(CameraRawVisualError::EmbeddedJpeg(
                        "invalid JPEG segment length".into(),
                    ));
                }
                position = position.checked_add(usize::from(length)).ok_or_else(|| {
                    CameraRawVisualError::EmbeddedJpeg("JPEG segment overflow".into())
                })?;
                if position > jpeg.len() {
                    return Err(CameraRawVisualError::EmbeddedJpeg(
                        "truncated JPEG segment".into(),
                    ));
                }
                if marker == 0xda {
                    saw_scan = true;
                    in_entropy_data = true;
                }
            }
        }
    }
}

/// Renders bounded extracted DOCX/XLSX/XLSM/ODS/PPTX content as a PNG. This is a
/// content preview, not a faithful Office layout engine; unsupported or
/// malformed packages fail closed so callers retain the metadata/text view.
pub fn render_office_content_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, OfficeVisualError> {
    Ok(render_office_content_preview_png_with_generation(path, target_dim)?.png)
}

pub fn office_preview_source_generation(path: &Path) -> Option<OfficePreviewSourceGeneration> {
    // PPTX snapshots belong exclusively to the worker.  Calling this from a
    // widget/cache-key path used to synchronously open and hash up to 128 MiB.
    if is_pptx_visual_path(path) {
        return None;
    }
    read_office_snapshot(path)
        .ok()
        .map(|snapshot| snapshot.generation)
}

pub fn is_pptx_visual_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pptx"))
}

pub fn is_office_visual_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "docx" | "xlsx" | "xlsm" | "ods" | "pptx"
    )
}

pub fn render_office_content_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedOfficePreview, OfficeVisualError> {
    let ext = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "docx" | "xlsx" | "xlsm" | "ods" | "pptx") {
        return Err(OfficeVisualError::UnsupportedFormat);
    }
    // Native opened-handle admission is not yet available on non-Unix
    // platforms. Keep PPTX fail-closed before touching its path there, while
    // preserving the established DOCX/XLSX/XLSM/ODS renderer behaviour.
    if ext == "pptx" && !cfg!(unix) {
        return Err(OfficeVisualError::PreviewDisabledOnPlatform);
    }
    let snapshot = if ext == "pptx" {
        read_pptx_snapshot(path)?
    } else {
        read_office_snapshot(path)?
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(&snapshot.bytes)).map_err(|error| {
        if ext == "pptx" {
            // A concurrent in-place writer can leave an otherwise admitted
            // single read with a non-package byte sequence. Treat that as a
            // rejected stable-read attempt, never as renderable content.
            OfficeVisualError::InputChanged
        } else {
            OfficeVisualError::Package(error.to_string())
        }
    })?;
    if archive.len() > MAX_OFFICE_PARTS {
        return Err(OfficeVisualError::TooManyParts {
            actual: archive.len(),
            limit: MAX_OFFICE_PARTS,
        });
    }
    if ext == "pptx" && !pptx_package_is_safe(&snapshot.bytes, &mut archive) {
        return Err(OfficeVisualError::NoContent);
    }
    let text = match ext.as_str() {
        "docx" => extract_docx_text(&mut archive),
        "xlsx" | "xlsm" => extract_xlsx_text(&mut archive),
        "ods" => extract_ods_text(&mut archive),
        "pptx" => extract_pptx_visual_text(&mut archive),
        _ => None,
    }
    .ok_or(OfficeVisualError::NoContent)?;
    Ok(RenderedOfficePreview {
        png: render_office_content_png(&text, &ext, target_dim)?,
        source_generation: snapshot.generation,
    })
}

/// PPTX gets exactly one worker-owned stable read. The bytes admitted here are
/// the bytes parsed and rendered below; callers never preflight a second copy
/// just to construct a cache key.
#[cfg(unix)]
fn read_pptx_snapshot(path: &Path) -> Result<OfficeSnapshot, OfficeVisualError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    if !path_metadata.is_file() {
        return Err(OfficeVisualError::InputNotRegular);
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    let opened = file
        .metadata()
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    let same_state = |left: &std::fs::Metadata, right: &std::fs::Metadata| {
        left.is_file()
            && right.is_file()
            && left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    };
    if !opened.is_file()
        || path_metadata.dev() != opened.dev()
        || path_metadata.ino() != opened.ino()
    {
        return Err(OfficeVisualError::InputNotRegular);
    }
    if !same_state(&path_metadata, &opened) {
        return Err(OfficeVisualError::InputChanged);
    }
    if opened.len() > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: opened.len(),
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).map_err(|_| {
        OfficeVisualError::InputTooLarge {
            actual: opened.len(),
            limit: MAX_OFFICE_PACKAGE_BYTES,
        }
    })?);
    file.by_ref()
        .take(MAX_OFFICE_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    let finished = file
        .metadata()
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    let current = std::fs::symlink_metadata(path).map_err(|_| OfficeVisualError::InputChanged)?;
    if bytes.len() as u64 != opened.len()
        || bytes.len() as u64 > MAX_OFFICE_PACKAGE_BYTES
        || !same_state(&opened, &finished)
        || !same_state(&opened, &current)
    {
        return Err(OfficeVisualError::InputChanged);
    }
    Ok(OfficeSnapshot {
        generation: OfficePreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

#[cfg(not(unix))]
fn read_pptx_snapshot(_path: &Path) -> Result<OfficeSnapshot, OfficeVisualError> {
    Err(OfficeVisualError::PreviewDisabledOnPlatform)
}

#[cfg(unix)]
fn read_office_snapshot(path: &Path) -> Result<OfficeSnapshot, OfficeVisualError> {
    use std::io::{Seek, SeekFrom};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    if !path_metadata.is_file() {
        return Err(OfficeVisualError::InputNotRegular);
    }
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    let opened = file
        .metadata()
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    let same_identity = |left: &std::fs::Metadata, right: &std::fs::Metadata| {
        left.dev() == right.dev() && left.ino() == right.ino()
    };
    let same_state = |left: &std::fs::Metadata, right: &std::fs::Metadata| {
        same_identity(left, right)
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    };
    if !opened.is_file() || !same_identity(&path_metadata, &opened) {
        return Err(OfficeVisualError::InputNotRegular);
    }
    if opened.len() > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: opened.len(),
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(opened.len()).map_err(|_| {
        OfficeVisualError::InputTooLarge {
            actual: opened.len(),
            limit: MAX_OFFICE_PACKAGE_BYTES,
        }
    })?);
    file.by_ref()
        .take(MAX_OFFICE_PACKAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    let first = file
        .metadata()
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    if bytes.len() as u64 > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    if bytes.len() as u64 != opened.len() || !same_state(&opened, &first) {
        return Err(OfficeVisualError::InputChanged);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    let mut compared = 0usize;
    let mut chunk = [0u8; 64 * 1024];
    while compared < bytes.len() {
        let expected = (bytes.len() - compared).min(chunk.len());
        let count = file
            .read(&mut chunk[..expected])
            .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
        if count == 0 || chunk[..count] != bytes[compared..compared + count] {
            return Err(OfficeVisualError::InputChanged);
        }
        compared = compared
            .checked_add(count)
            .ok_or(OfficeVisualError::InputChanged)?;
    }
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?
        != 0
    {
        return Err(OfficeVisualError::InputChanged);
    }
    let second = file
        .metadata()
        .map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    let current = std::fs::symlink_metadata(path).map_err(|_| OfficeVisualError::InputChanged)?;
    if !same_state(&opened, &second) || !current.is_file() || !same_state(&opened, &current) {
        return Err(OfficeVisualError::InputChanged);
    }
    Ok(OfficeSnapshot {
        generation: OfficePreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

#[cfg(not(unix))]
fn read_office_snapshot(path: &Path) -> Result<OfficeSnapshot, OfficeVisualError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| OfficeVisualError::InputMetadata(error.to_string()))?;
    if !metadata.is_file() {
        return Err(OfficeVisualError::InputNotRegular);
    }
    if metadata.len() > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: metadata.len(),
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    let bytes = std::fs::read(path).map_err(|error| OfficeVisualError::Read(error.to_string()))?;
    if bytes.len() as u64 > MAX_OFFICE_PACKAGE_BYTES {
        return Err(OfficeVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_OFFICE_PACKAGE_BYTES,
        });
    }
    Ok(OfficeSnapshot {
        generation: OfficePreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
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
    match std::fs::File::open(path) {
        Ok(mut file) => {
            let initial_metadata = match file.metadata() {
                Ok(metadata) => metadata,
                Err(err) => {
                    return PreviewData {
                        title,
                        kind: PreviewKind::Unsupported,
                        body: format!("Failed to read file: {err}"),
                        spreadsheet: None,
                        open_external: true,
                    };
                }
            };
            let initial_size = initial_metadata.len();
            let initial_modified = initial_metadata.modified().ok();
            let mut bytes = Vec::with_capacity(initial_size.min((LIMIT + 1) as u64) as usize);
            if let Err(err) = file.by_ref().take(LIMIT as u64 + 1).read_to_end(&mut bytes) {
                return PreviewData {
                    title,
                    kind: PreviewKind::Unsupported,
                    body: format!("Failed to read file: {err}"),
                    spreadsheet: None,
                    open_external: true,
                };
            }
            let complete = initial_size <= LIMIT as u64
                && bytes.len() as u64 == initial_size
                && file.metadata().is_ok_and(|metadata| {
                    metadata.len() == initial_size && metadata.modified().ok() == initial_modified
                });
            let head = &bytes[..bytes.len().min(LIMIT)];
            match std::str::from_utf8(head) {
                Ok(s) => {
                    let line_count = s.lines().count();
                    let mut lines = vec![
                        format!("Filetype: {}", text_kind_label(path)),
                        "Encoding: UTF-8".to_string(),
                        format!("Lines: {line_count}"),
                        if !complete {
                            "Preview: Truncated".to_string()
                        } else {
                            "Preview: Complete".to_string()
                        },
                    ];
                    lines.push(format!("Size: {}", format_size(initial_size)));
                    if let Some(modified) = initial_modified
                        && let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH)
                    {
                        lines.push(format!("Modified: {}", format_modified(dur.as_secs())));
                    }
                    let text = if !complete {
                        format!("{s}\n\n...(text preview truncated)")
                    } else {
                        s.to_string()
                    };
                    PreviewData {
                        title,
                        kind: PreviewKind::Text,
                        body: format!("{}{}{}", lines.join("\n"), EXTRACTED_TEXT_MARKER, text),
                        spreadsheet: None,
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
                    spreadsheet: None,
                    open_external: true,
                },
            }
        }
        Err(err) => PreviewData {
            title,
            kind: PreviewKind::Unsupported,
            body: format!("Failed to read file: {err}"),
            spreadsheet: None,
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
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_camera_raw(title: String, path: &Path) -> PreviewData {
    PreviewData {
        title,
        kind: PreviewKind::CameraRawMetadata,
        body: metadata_summary(
            path,
            "Camera RAW image.",
            Some("Visual preview uses a bounded embedded JPEG when available."),
        ),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_affinity(title: String, path: &Path) -> PreviewData {
    PreviewData {
        title,
        kind: PreviewKind::AffinityMetadata,
        body: metadata_summary(
            path,
            "Affinity document.",
            Some("Best-effort embedded preview — may differ from the saved canvas."),
        ),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_eps(title: String, path: &Path) -> PreviewData {
    PreviewData {
        title,
        kind: PreviewKind::EpsMetadata,
        body: metadata_summary(
            path,
            "EPS image",
            Some(
                "Preview: No safe isolated EPS renderer is available in this build. Ottrin will not execute PostScript directly; open this file in a trusted external viewer.",
            ),
        ),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_sqlite(title: String, path: &Path) -> PreviewData {
    let mut lines = vec!["Filetype: SQLite database".to_string()];
    if let Ok(metadata) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(metadata.len())));
    }
    match inspect_sqlite_metadata(path) {
        Ok(schema) => {
            lines.extend([
                format!("Tables: {}", schema.tables.len()),
                format!("Indexes: {}", schema.indexes.len()),
                format!("Views: {}", schema.views.len()),
                format!("Triggers: {}", schema.triggers.len()),
                "Preview: Read-only bounded metadata card".to_string(),
            ]);
            if let Ok(card) = schema.card_text() {
                lines.extend(card.lines().skip(5).map(str::to_string));
            }
        }
        Err(_) => lines.push("Preview: SQLite metadata unavailable for this file.".to_string()),
    }
    PreviewData {
        title,
        kind: PreviewKind::SqliteMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_stl(title: String, path: &Path) -> PreviewData {
    let mut lines = vec!["Filetype: STL mesh".to_string(), "Format: STL".to_string()];
    if let Ok(metadata) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(metadata.len())));
    }
    match inspect_stl_metadata(path) {
        Ok(metadata) => {
            lines.push(format!("Triangles: {}", metadata.triangle_count));
            lines.push(format!(
                "Bounds: {:.4} x {:.4} x {:.4}",
                metadata.dimensions[0], metadata.dimensions[1], metadata.dimensions[2]
            ));
            lines.push("Preview: Bounded CPU mesh render".to_string());
        }
        Err(_) => lines.push("Preview: STL mesh metadata unavailable for this file.".to_string()),
    }
    PreviewData {
        title,
        kind: PreviewKind::StlMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_obj(title: String, path: &Path) -> PreviewData {
    let mut lines = vec![
        "Filetype: Wavefront OBJ mesh".to_string(),
        "Format: OBJ".to_string(),
    ];
    if let Ok(metadata) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(metadata.len())));
    }
    match inspect_obj_metadata(path) {
        Ok(metadata) => {
            lines.push(format!("Objects: {}", metadata.object_count));
            lines.push(format!("Groups: {}", metadata.group_count));
            lines.push(format!("Vertices: {}", metadata.vertex_count));
            lines.push(format!("Faces: {}", metadata.face_count));
            lines.push(format!("Triangles: {}", metadata.triangle_count));
            lines.push(format!(
                "Bounds: {:.4} x {:.4} x {:.4}",
                metadata.dimensions[0], metadata.dimensions[1], metadata.dimensions[2]
            ));
            lines.push("Materials/textures: ignored for this bounded preview".to_string());
            lines.push("Preview: Bounded CPU mesh render".to_string());
        }
        Err(_) => lines.push("Preview: OBJ mesh metadata unavailable for this file.".to_string()),
    }
    PreviewData {
        title,
        kind: PreviewKind::ObjMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_ply(title: String, path: &Path) -> PreviewData {
    let mut lines = vec![
        "Filetype: Polygon File Format mesh".to_string(),
        "Format: PLY".to_string(),
    ];
    if let Ok(metadata) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(metadata.len())));
    }
    match inspect_ply_metadata(path) {
        Ok(metadata) => {
            lines.push(format!("Vertices: {}", metadata.vertex_count));
            lines.push(format!("Faces: {}", metadata.face_count));
            lines.push(format!("Triangles: {}", metadata.triangle_count));
            lines.push(format!(
                "Bounds: {:.4} x {:.4} x {:.4}",
                metadata.dimensions[0], metadata.dimensions[1], metadata.dimensions[2]
            ));
            lines.push("Preview: Bounded CPU mesh render".to_string());
        }
        Err(_) => lines.push("Preview: PLY mesh metadata unavailable for this file.".to_string()),
    }
    PreviewData {
        title,
        kind: PreviewKind::PlyMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_3mf(title: String, path: &Path) -> PreviewData {
    let mut lines = vec![
        "Filetype: 3D Manufacturing Format package".to_string(),
        "Format: 3MF".to_string(),
    ];
    if let Ok(metadata) = std::fs::metadata(path) {
        lines.push(format!("Size: {}", format_size(metadata.len())));
    }
    match inspect_3mf_metadata(path) {
        Ok(metadata) => {
            lines.push(format!("Units: {}", metadata.unit));
            lines.push(format!("Objects: {}", metadata.object_count));
            lines.push(format!("Build items: {}", metadata.build_item_count));
            lines.push(format!("Vertices: {}", metadata.vertex_count));
            lines.push(format!("Triangles: {}", metadata.triangle_count));
            lines.push(format!(
                "Bounds: {:.4} x {:.4} x {:.4}",
                metadata.dimensions[0], metadata.dimensions[1], metadata.dimensions[2]
            ));
            if metadata.ignored_optional_features {
                lines.push(
                    "Materials/textures/print options: ignored for this bounded preview"
                        .to_string(),
                );
            }
            lines.push("Preview: Bounded CPU mesh render".to_string());
        }
        Err(_) => lines.push("Preview: 3MF metadata unavailable for this package.".to_string()),
    }
    PreviewData {
        title,
        kind: PreviewKind::ThreeMfMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_font(title: String, path: &Path) -> PreviewData {
    let mut lines = vec![
        "Filetype: OpenType/TrueType font".to_string(),
        format!(
            "Format: {}",
            path_format(path).unwrap_or_else(|| "Font".to_string())
        ),
    ];
    if let Some(size) = std::fs::metadata(path).ok().map(|metadata| metadata.len()) {
        lines.push(format!("Size: {}", format_size(size)));
    }
    if let Ok(source) = read_bounded_font_bytes(path)
        && let Ok(tables) = validate_font_container(path, &source)
        && let Ok(face) = ttf_parser::Face::parse(&source, 0)
        && let Some(identity) = bounded_font_identity(&face, &tables)
    {
        lines.push(format!("Family: {identity}"));
    }
    lines.push("Preview: Bounded font sample".to_string());
    PreviewData {
        title,
        kind: PreviewKind::FontMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

/// Generates a bounded PNG preview for OpenType/TrueType fonts.
pub fn render_font_preview_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, FontVisualError> {
    if !is_font_path(path) {
        return Err(FontVisualError::UnsupportedFormat);
    }
    let source = read_bounded_font_bytes(path)?;
    let tables = validate_font_container(path, &source)?;
    let face = ttf_parser::Face::parse(&source, 0)
        .map_err(|error| FontVisualError::Parse(error.to_string()))?;
    let specimen = font_specimen(bounded_font_identity(&face, &tables));

    let dimension = target_dim.clamp(64, MAX_FONT_PREVIEW_DIMENSION);
    let dimension_pixels = u64::from(dimension).saturating_mul(dimension.into());
    if dimension_pixels > MAX_FONT_PREVIEW_PIXELS {
        return Err(FontVisualError::UnboundedDimensions { dimension });
    }

    if source.starts_with(b"OTTO") {
        // CFF charstrings have no bounded abort callback. Keep their preview
        // useful but do not execute an unbounded outline decoder in-process.
        return render_cff_font_card(bounded_font_identity(&face, &tables).as_deref(), dimension);
    }
    preflight_ttf_specimen_glyphs(&source, &tables, &face, &specimen)?;
    render_font_sample_png(&face, &specimen, dimension)
}

fn read_bounded_font_bytes(path: &Path) -> Result<Vec<u8>, FontVisualError> {
    let mut file =
        std::fs::File::open(path).map_err(|error| FontVisualError::Read(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| FontVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_FONT_PREVIEW_PACKAGE_BYTES {
        return Err(FontVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_FONT_PREVIEW_PACKAGE_BYTES,
        });
    }
    let capacity = usize::try_from(input_size).map_err(|_| FontVisualError::InputTooLarge {
        actual: input_size,
        limit: MAX_FONT_PREVIEW_PACKAGE_BYTES,
    })?;
    let mut source = Vec::with_capacity(capacity);
    file.by_ref()
        .take(MAX_FONT_PREVIEW_PACKAGE_BYTES + 1)
        .read_to_end(&mut source)
        .map_err(|error| FontVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| FontVisualError::Read(error.to_string()))?
        .len();
    if source.len() as u64 != input_size || final_size != input_size {
        return Err(FontVisualError::InputChanged);
    }
    Ok(source)
}

fn validate_font_container(
    path: &Path,
    source: &[u8],
) -> Result<Vec<FontTableRecord>, FontVisualError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    let magic = source
        .get(..4)
        .ok_or_else(|| FontVisualError::InvalidTableDirectory("missing SFNT header".to_string()))?;
    if magic == b"ttcf" {
        return Err(FontVisualError::UnsupportedCollection);
    }
    let expected_magic: &[&[u8]] = match extension.as_deref() {
        Some("ttf") => &[b"\0\x01\0\0", b"true"],
        Some("otf") => &[b"OTTO"],
        _ => return Err(FontVisualError::UnsupportedFormat),
    };
    if !expected_magic.contains(&magic) {
        return Err(FontVisualError::ExtensionMismatch);
    }
    if source.len() < 12 {
        return Err(FontVisualError::InvalidTableDirectory(
            "truncated SFNT offset table".to_string(),
        ));
    }
    let table_count = usize::from(u16::from_be_bytes([source[4], source[5]]));
    if table_count == 0 || table_count > MAX_FONT_TABLES {
        return Err(FontVisualError::InvalidTableDirectory(format!(
            "table count {table_count} exceeds bounded preview limits"
        )));
    }
    let directory_bytes = table_count
        .checked_mul(16)
        .and_then(|bytes| bytes.checked_add(12))
        .ok_or_else(|| FontVisualError::InvalidTableDirectory("directory overflow".to_string()))?;
    if directory_bytes > source.len() {
        return Err(FontVisualError::InvalidTableDirectory(
            "truncated SFNT table directory".to_string(),
        ));
    }

    let mut tables = Vec::with_capacity(table_count);
    let mut ranges = Vec::with_capacity(table_count);
    for index in 0..table_count {
        let entry_offset = 12 + index * 16;
        let tag = source[entry_offset..entry_offset + 4]
            .try_into()
            .expect("four-byte SFNT tag");
        if tables
            .iter()
            .any(|table: &FontTableRecord| table.tag == tag)
        {
            return Err(FontVisualError::InvalidTableDirectory(format!(
                "duplicate table tag {tag:?}"
            )));
        }
        if is_unsupported_font_surface(tag) {
            return Err(FontVisualError::UnsupportedSurface { tag });
        }
        let offset = usize::try_from(u32::from_be_bytes(
            source[entry_offset + 8..entry_offset + 12]
                .try_into()
                .expect("four-byte SFNT offset"),
        ))
        .map_err(|_| FontVisualError::InvalidTableDirectory("table offset overflow".to_string()))?;
        let length = usize::try_from(u32::from_be_bytes(
            source[entry_offset + 12..entry_offset + 16]
                .try_into()
                .expect("four-byte SFNT length"),
        ))
        .map_err(|_| FontVisualError::InvalidTableDirectory("table length overflow".to_string()))?;
        let end = offset.checked_add(length).ok_or_else(|| {
            FontVisualError::InvalidTableDirectory("table offset plus length overflow".to_string())
        })?;
        if end > source.len() || (length > 0 && offset < directory_bytes) {
            return Err(FontVisualError::InvalidTableDirectory(format!(
                "table {tag:?} is outside the bounded SFNT payload"
            )));
        }
        if length > 0 {
            ranges.push((offset, end));
        }
        tables.push(FontTableRecord {
            tag,
            offset,
            length,
        });
    }
    ranges.sort_unstable_by_key(|range| range.0);
    if ranges.windows(2).any(|ranges| ranges[0].1 > ranges[1].0) {
        return Err(FontVisualError::InvalidTableDirectory(
            "nonempty SFNT tables overlap".to_string(),
        ));
    }
    if let Some(name) = tables.iter().find(|table| table.tag == *b"name") {
        validate_font_name_table(&source[name.offset..name.offset + name.length])?;
    }
    Ok(tables)
}

fn is_unsupported_font_surface(tag: [u8; 4]) -> bool {
    [
        *b"SVG ", *b"CBDT", *b"CBLC", *b"sbix", *b"COLR", *b"CPAL", *b"EBDT", *b"EBLC", *b"bdat",
        *b"bloc",
    ]
    .contains(&tag)
}

fn validate_font_name_table(data: &[u8]) -> Result<(), FontVisualError> {
    if data.len() > MAX_FONT_NAME_TABLE_BYTES {
        return Err(FontVisualError::InvalidTableDirectory(
            "name table exceeds bounded metadata limits".to_string(),
        ));
    }
    if data.len() < 6 {
        return Err(FontVisualError::InvalidTableDirectory(
            "truncated name table".to_string(),
        ));
    }
    let record_count = usize::from(u16::from_be_bytes([data[2], data[3]]));
    if record_count > MAX_FONT_NAME_RECORDS {
        return Err(FontVisualError::InvalidTableDirectory(
            "name record count exceeds bounded metadata limits".to_string(),
        ));
    }
    let strings_offset = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let records_end = record_count
        .checked_mul(12)
        .and_then(|bytes| bytes.checked_add(6))
        .ok_or_else(|| {
            FontVisualError::InvalidTableDirectory("name record overflow".to_string())
        })?;
    if strings_offset < records_end || strings_offset > data.len() {
        return Err(FontVisualError::InvalidTableDirectory(
            "invalid name string storage offset".to_string(),
        ));
    }
    for index in 0..record_count {
        let record_offset = 6 + index * 12;
        let length = usize::from(u16::from_be_bytes([
            data[record_offset + 8],
            data[record_offset + 9],
        ]));
        let offset = usize::from(u16::from_be_bytes([
            data[record_offset + 10],
            data[record_offset + 11],
        ]));
        let end = strings_offset
            .checked_add(offset)
            .and_then(|start| start.checked_add(length));
        if length > MAX_FONT_NAME_BYTES || end.is_none_or(|end| end > data.len()) {
            return Err(FontVisualError::InvalidTableDirectory(
                "name record is outside bounded string storage".to_string(),
            ));
        }
    }
    Ok(())
}

fn bounded_font_identity(
    face: &ttf_parser::Face<'_>,
    tables: &[FontTableRecord],
) -> Option<String> {
    if !tables.iter().any(|table| table.tag == *b"name") {
        return None;
    }
    for name_id in [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
        ttf_parser::name_id::FULL_NAME,
    ] {
        for name in face.names() {
            if name.name_id == name_id
                && name.name.len() <= MAX_FONT_NAME_BYTES
                && let Some(value) = name.to_string()
            {
                let value: String = value
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(MAX_FONT_NAME_CHARS)
                    .collect();
                if !value.trim().is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

fn font_specimen(identity: Option<String>) -> String {
    match identity {
        Some(identity) => format!("Aa Bb Cc 123\n{identity}"),
        None => "Aa Bb Cc 123\nFont preview".to_string(),
    }
}

fn font_table<'a>(
    source: &'a [u8],
    tables: &[FontTableRecord],
    tag: [u8; 4],
) -> Result<&'a [u8], FontVisualError> {
    let table = tables
        .iter()
        .find(|table| table.tag == tag)
        .ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork(format!("missing required TTF table {tag:?}"))
        })?;
    source
        .get(table.offset..table.offset + table.length)
        .ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("invalid validated TTF table".to_string())
        })
}

fn preflight_ttf_specimen_glyphs(
    source: &[u8],
    tables: &[FontTableRecord],
    face: &ttf_parser::Face<'_>,
    specimen: &str,
) -> Result<(), FontVisualError> {
    let head = font_table(source, tables, *b"head")?;
    let maxp = font_table(source, tables, *b"maxp")?;
    let loca = font_table(source, tables, *b"loca")?;
    let glyf = font_table(source, tables, *b"glyf")?;
    let index_format = i16::from_be_bytes(
        head.get(50..52)
            .ok_or_else(|| FontVisualError::UnsafeOutlineWork("truncated head table".to_string()))?
            .try_into()
            .expect("two-byte head index format"),
    );
    let glyph_count = usize::from(u16::from_be_bytes(
        maxp.get(4..6)
            .ok_or_else(|| FontVisualError::UnsafeOutlineWork("truncated maxp table".to_string()))?
            .try_into()
            .expect("two-byte maxp glyph count"),
    ));
    let entry_size = match index_format {
        0 => 2,
        1 => 4,
        _ => {
            return Err(FontVisualError::UnsafeOutlineWork(
                "unsupported loca index format".to_string(),
            ));
        }
    };
    let required_loca_bytes = glyph_count
        .checked_add(1)
        .and_then(|entries| entries.checked_mul(entry_size))
        .ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("loca table size overflow".to_string())
        })?;
    if loca.len() < required_loca_bytes {
        return Err(FontVisualError::UnsafeOutlineWork(
            "truncated loca table".to_string(),
        ));
    }
    let mut glyphs = Vec::new();
    for character in specimen.chars().take(MAX_FONT_TEXT_GLYPHS) {
        if character == '\n' {
            continue;
        }
        let glyph = face
            .glyph_index(character)
            .unwrap_or(ttf_parser::GlyphId(0));
        if !glyphs.contains(&glyph) {
            glyphs.push(glyph);
        }
    }
    for glyph in glyphs {
        let glyph_id = usize::from(glyph.0);
        if glyph_id >= glyph_count {
            return Err(FontVisualError::UnsafeOutlineWork(
                "cmap returned an out-of-range glyph".to_string(),
            ));
        }
        let start = ttf_loca_offset(loca, glyph_id, entry_size, index_format)?;
        let end = ttf_loca_offset(loca, glyph_id + 1, entry_size, index_format)?;
        if start > end || end > glyf.len() {
            return Err(FontVisualError::UnsafeOutlineWork(
                "glyph loca range is invalid".to_string(),
            ));
        }
        preflight_simple_ttf_glyph(&glyf[start..end])?;
    }
    Ok(())
}

fn ttf_loca_offset(
    loca: &[u8],
    index: usize,
    entry_size: usize,
    index_format: i16,
) -> Result<usize, FontVisualError> {
    let offset = index
        .checked_mul(entry_size)
        .ok_or_else(|| FontVisualError::UnsafeOutlineWork("loca index overflow".to_string()))?;
    match index_format {
        0 => Ok(usize::from(u16::from_be_bytes(
            loca.get(offset..offset + 2)
                .ok_or_else(|| {
                    FontVisualError::UnsafeOutlineWork("truncated loca entry".to_string())
                })?
                .try_into()
                .expect("two-byte loca entry"),
        )) * 2),
        1 => Ok(usize::try_from(u32::from_be_bytes(
            loca.get(offset..offset + 4)
                .ok_or_else(|| {
                    FontVisualError::UnsafeOutlineWork("truncated loca entry".to_string())
                })?
                .try_into()
                .expect("four-byte loca entry"),
        ))
        .expect("u32 fits usize on supported Ottrin targets")),
        _ => unreachable!("validated loca index format"),
    }
}

fn preflight_simple_ttf_glyph(data: &[u8]) -> Result<(), FontVisualError> {
    if data.is_empty() {
        return Ok(());
    }
    let contour_count = i16::from_be_bytes(
        data.get(..2)
            .ok_or_else(|| {
                FontVisualError::UnsafeOutlineWork("truncated glyph header".to_string())
            })?
            .try_into()
            .expect("two-byte contour count"),
    );
    if contour_count < 0 {
        return Err(FontVisualError::UnsafeOutlineWork(
            "composite glyphs are outside the bounded preview subset".to_string(),
        ));
    }
    let contour_count = contour_count as usize;
    if contour_count > MAX_FONT_SIMPLE_GLYPH_CONTOURS {
        return Err(FontVisualError::UnsafeOutlineWork(
            "simple glyph exceeds contour limit".to_string(),
        ));
    }
    let endpoints_end = 10usize
        .checked_add(contour_count.checked_mul(2).ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("glyph endpoint overflow".to_string())
        })?)
        .ok_or_else(|| FontVisualError::UnsafeOutlineWork("glyph endpoint overflow".to_string()))?;
    let last_point = if contour_count == 0 {
        0
    } else {
        usize::from(u16::from_be_bytes(
            data.get(endpoints_end - 2..endpoints_end)
                .ok_or_else(|| {
                    FontVisualError::UnsafeOutlineWork("truncated glyph endpoints".to_string())
                })?
                .try_into()
                .expect("two-byte endpoint"),
        ))
        .checked_add(1)
        .ok_or_else(|| FontVisualError::UnsafeOutlineWork("glyph point overflow".to_string()))?
    };
    if last_point > MAX_FONT_SIMPLE_GLYPH_POINTS {
        return Err(FontVisualError::UnsafeOutlineWork(
            "simple glyph exceeds point limit".to_string(),
        ));
    }
    let instruction_length_offset = endpoints_end;
    let instruction_length = usize::from(u16::from_be_bytes(
        data.get(instruction_length_offset..instruction_length_offset + 2)
            .ok_or_else(|| {
                FontVisualError::UnsafeOutlineWork("truncated glyph instructions".to_string())
            })?
            .try_into()
            .expect("two-byte instruction length"),
    ));
    let mut offset = instruction_length_offset
        .checked_add(2)
        .and_then(|value| value.checked_add(instruction_length))
        .ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("glyph instruction overflow".to_string())
        })?;
    let mut flags = Vec::with_capacity(last_point);
    while flags.len() < last_point {
        let flag = *data.get(offset).ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("truncated glyph flags".to_string())
        })?;
        offset += 1;
        let repetitions = if flag & 0x08 != 0 {
            usize::from(*data.get(offset).ok_or_else(|| {
                FontVisualError::UnsafeOutlineWork("truncated repeated glyph flag".to_string())
            })?) + 1
        } else {
            1
        };
        if flag & 0x08 != 0 {
            offset += 1;
        }
        if flags
            .len()
            .checked_add(repetitions)
            .is_none_or(|count| count > last_point)
        {
            return Err(FontVisualError::UnsafeOutlineWork(
                "glyph flag repetitions exceed point count".to_string(),
            ));
        }
        flags.extend(std::iter::repeat_n(flag, repetitions));
    }
    let coordinate_bytes = flags.iter().try_fold(0usize, |total, flag| {
        let x = if flag & 0x02 != 0 {
            1
        } else if flag & 0x10 != 0 {
            0
        } else {
            2
        };
        let y = if flag & 0x04 != 0 {
            1
        } else if flag & 0x20 != 0 {
            0
        } else {
            2
        };
        total.checked_add(x + y)
    });
    if coordinate_bytes
        .is_none_or(|bytes| offset.checked_add(bytes).is_none_or(|end| end > data.len()))
    {
        return Err(FontVisualError::UnsafeOutlineWork(
            "truncated glyph coordinates".to_string(),
        ));
    }
    Ok(())
}

fn render_cff_font_card(
    identity: Option<&str>,
    dimension: u32,
) -> Result<Vec<u8>, FontVisualError> {
    let mut image =
        image::RgbaImage::from_pixel(dimension, dimension, image::Rgba([255, 255, 255, 255]));
    let identity = identity.unwrap_or("Identity unavailable");
    render_document_text(
        &mut image,
        &format!(
            "CFF font\n{identity}\nCFF outlines are not rendered;\nmetadata identity card only"
        ),
        dimension / 10,
        dimension / 10,
        dimension.saturating_sub(dimension / 10),
        dimension.saturating_sub(dimension / 10),
    );
    let mut bytes = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error: image::ImageError| FontVisualError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn render_font_sample_png(
    face: &ttf_parser::Face<'_>,
    specimen: &str,
    dimension: u32,
) -> Result<Vec<u8>, FontVisualError> {
    let mut image =
        image::RgbaImage::from_pixel(dimension, dimension, image::Rgba([255, 255, 255, 255]));
    render_text_with_font(
        &mut image,
        face,
        specimen,
        dimension / 10,
        dimension / 10,
        dimension.saturating_sub(dimension / 10),
        dimension.saturating_sub(dimension / 10),
    )?;
    let mut bytes = Vec::new();
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            dimension,
            dimension,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error: image::ImageError| FontVisualError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn render_text_with_font(
    image: &mut image::RgbaImage,
    face: &ttf_parser::Face<'_>,
    text: &str,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> Result<(), FontVisualError> {
    use std::string::String;

    let sample = text.chars().take(MAX_FONT_TEXT_GLYPHS).collect::<String>();
    let font_size = (image.width() as f32 / 12.0).clamp(8.0, 36.0);
    let units_per_em = face.units_per_em();
    if units_per_em == 0 {
        return Err(FontVisualError::UnsafeOutlineWork(
            "font has zero units per em".to_string(),
        ));
    }
    let scale = font_size / f32::from(units_per_em);
    let ascent = f32::from(face.ascender()) * scale;
    let line_height = (f32::from(face.height()) * scale * 1.4).max(font_size + 2.0);
    if !scale.is_finite() || !ascent.is_finite() || !line_height.is_finite() || line_height <= 0.0 {
        return Err(FontVisualError::UnsafeOutlineWork(
            "non-finite font metrics".to_string(),
        ));
    }
    let max_width = right.saturating_sub(left) as f32;
    let mut x = left as f32;
    let mut baseline = top as f32 + ascent;
    let mut total_pixels = 0_u64;
    let mut total_raster_steps = 0_u64;
    for ch in sample.chars() {
        if ch == '\n' {
            x = left as f32;
            baseline += line_height;
            if !baseline.is_finite() || baseline > bottom as f32 {
                return Ok(());
            }
            continue;
        }
        let id = face.glyph_index(ch).unwrap_or(ttf_parser::GlyphId(0));
        let advance = face
            .glyph_hor_advance(id)
            .map(f32::from)
            .unwrap_or(f32::from(units_per_em) / 2.0)
            * scale;
        if !advance.is_finite() || !x.is_finite() || !baseline.is_finite() {
            return Err(FontVisualError::UnsafeOutlineWork(
                "non-finite glyph positioning".to_string(),
            ));
        }
        if x + advance > left as f32 + max_width && x > left as f32 {
            x = left as f32;
            baseline += line_height;
            if !baseline.is_finite() || baseline > bottom as f32 {
                return Ok(());
            }
        }
        let mut outline = BoundedFontOutline::default();
        let Some(bounds) = face.outline_glyph(id, &mut outline) else {
            x += advance;
            continue;
        };
        if outline.limit_exceeded {
            return Err(FontVisualError::UnsafeOutlineWork(
                "glyph outline exceeds segment limit".to_string(),
            ));
        }
        let min_x = x + f32::from(bounds.x_min) * scale;
        let max_x = x + f32::from(bounds.x_max) * scale;
        let min_y = baseline - f32::from(bounds.y_max) * scale;
        let max_y = baseline - f32::from(bounds.y_min) * scale;
        if !min_x.is_finite()
            || !min_y.is_finite()
            || !max_x.is_finite()
            || !max_y.is_finite()
            || min_x < 0.0
            || min_y < 0.0
            || max_x > image.width() as f32
            || max_y > image.height() as f32
            || max_x < min_x
            || max_y < min_y
        {
            return Err(FontVisualError::UnsafeOutlineWork(
                "glyph raster bounds are outside the preview surface".to_string(),
            ));
        }
        let width = (max_x.ceil() - min_x.floor()) as u64;
        let height = (max_y.ceil() - min_y.floor()) as u64;
        let glyph_pixels = width.checked_mul(height).ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("glyph pixel work overflow".to_string())
        })?;
        total_pixels = total_pixels.checked_add(glyph_pixels).ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("total glyph pixel work overflow".to_string())
        })?;
        if glyph_pixels > MAX_FONT_GLYPH_PIXELS || total_pixels > MAX_FONT_TOTAL_GLYPH_PIXELS {
            return Err(FontVisualError::UnsafeOutlineWork(
                "glyph raster work exceeds preview limits".to_string(),
            ));
        }
        rasterize_font_outline(
            image,
            &outline.commands,
            x,
            baseline,
            scale,
            &mut total_raster_steps,
        )?;
        x += advance;
    }
    Ok(())
}

fn rasterize_font_outline(
    image: &mut image::RgbaImage,
    commands: &[FontOutlineCommand],
    x: f32,
    baseline: f32,
    scale: f32,
    total_steps: &mut u64,
) -> Result<(), FontVisualError> {
    let mut current = None;
    let mut contour_start = None;
    for command in commands {
        match *command {
            FontOutlineCommand::MoveTo(px, py) => {
                let point = scale_font_point(px, py, x, baseline, scale)?;
                current = Some(point);
                contour_start = Some(point);
            }
            FontOutlineCommand::LineTo(px, py) => {
                let next = scale_font_point(px, py, x, baseline, scale)?;
                if let Some(start) = current {
                    rasterize_font_line(image, start, next, total_steps)?;
                }
                current = Some(next);
            }
            FontOutlineCommand::QuadTo(cx, cy, px, py) => {
                let control = scale_font_point(cx, cy, x, baseline, scale)?;
                let next = scale_font_point(px, py, x, baseline, scale)?;
                if let Some(start) = current {
                    let mut previous = start;
                    for step in 1..=8 {
                        let t = step as f32 / 8.0;
                        let point = quadratic_point(start, control, next, t);
                        rasterize_font_line(image, previous, point, total_steps)?;
                        previous = point;
                    }
                }
                current = Some(next);
            }
            FontOutlineCommand::CurveTo(cx1, cy1, cx2, cy2, px, py) => {
                let control1 = scale_font_point(cx1, cy1, x, baseline, scale)?;
                let control2 = scale_font_point(cx2, cy2, x, baseline, scale)?;
                let next = scale_font_point(px, py, x, baseline, scale)?;
                if let Some(start) = current {
                    let mut previous = start;
                    for step in 1..=12 {
                        let t = step as f32 / 12.0;
                        let point = cubic_point(start, control1, control2, next, t);
                        rasterize_font_line(image, previous, point, total_steps)?;
                        previous = point;
                    }
                }
                current = Some(next);
            }
            FontOutlineCommand::Close => {
                if let (Some(start), Some(end)) = (current, contour_start) {
                    rasterize_font_line(image, start, end, total_steps)?;
                    current = Some(end);
                }
            }
        }
    }
    Ok(())
}

fn scale_font_point(
    point_x: f32,
    point_y: f32,
    x: f32,
    baseline: f32,
    scale: f32,
) -> Result<(f32, f32), FontVisualError> {
    let point = (x + point_x * scale, baseline - point_y * scale);
    if !point.0.is_finite() || !point.1.is_finite() {
        return Err(FontVisualError::UnsafeOutlineWork(
            "non-finite glyph outline point".to_string(),
        ));
    }
    Ok(point)
}

fn quadratic_point(start: (f32, f32), control: (f32, f32), end: (f32, f32), t: f32) -> (f32, f32) {
    let inverse = 1.0 - t;
    (
        inverse * inverse * start.0 + 2.0 * inverse * t * control.0 + t * t * end.0,
        inverse * inverse * start.1 + 2.0 * inverse * t * control.1 + t * t * end.1,
    )
}

fn cubic_point(
    start: (f32, f32),
    control1: (f32, f32),
    control2: (f32, f32),
    end: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let inverse = 1.0 - t;
    (
        inverse.powi(3) * start.0
            + 3.0 * inverse * inverse * t * control1.0
            + 3.0 * inverse * t * t * control2.0
            + t.powi(3) * end.0,
        inverse.powi(3) * start.1
            + 3.0 * inverse * inverse * t * control1.1
            + 3.0 * inverse * t * t * control2.1
            + t.powi(3) * end.1,
    )
}

fn rasterize_font_line(
    image: &mut image::RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    total_steps: &mut u64,
) -> Result<(), FontVisualError> {
    if !start.0.is_finite() || !start.1.is_finite() || !end.0.is_finite() || !end.1.is_finite() {
        return Err(FontVisualError::UnsafeOutlineWork(
            "non-finite outline raster segment".to_string(),
        ));
    }
    let steps = (end.0 - start.0).abs().max((end.1 - start.1).abs()).ceil() as u64;
    *total_steps = total_steps
        .checked_add(steps.saturating_add(1))
        .ok_or_else(|| {
            FontVisualError::UnsafeOutlineWork("outline raster step overflow".to_string())
        })?;
    if *total_steps > MAX_FONT_OUTLINE_RASTER_STEPS {
        return Err(FontVisualError::UnsafeOutlineWork(
            "outline raster work exceeds preview limits".to_string(),
        ));
    }
    for step in 0..=steps {
        let progress = if steps == 0 {
            0.0
        } else {
            step as f32 / steps as f32
        };
        let px = (start.0 + (end.0 - start.0) * progress).round() as i32;
        let py = (start.1 + (end.1 - start.1) * progress).round() as i32;
        if px < 0 || py < 0 || px >= image.width() as i32 || py >= image.height() as i32 {
            return Err(FontVisualError::UnsafeOutlineWork(
                "outline segment escapes the preview surface".to_string(),
            ));
        }
        image.put_pixel(px as u32, py as u32, image::Rgba([32, 32, 32, 255]));
    }
    Ok(())
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
    let mut file = std::fs::File::open(path)
        .map_err(|error| SvgVisualError::InputMetadata(error.to_string()))?;
    let input_size = file
        .metadata()
        .map_err(|error| SvgVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_SVG_VISUAL_INPUT_BYTES {
        return Err(SvgVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_SVG_VISUAL_INPUT_BYTES,
        });
    }

    let mut input = Vec::with_capacity(input_size.min(MAX_SVG_VISUAL_INPUT_BYTES) as usize);
    file.by_ref()
        .take(MAX_SVG_VISUAL_INPUT_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|error| SvgVisualError::InputMetadata(error.to_string()))?;
    if input.len() as u64 > MAX_SVG_VISUAL_INPUT_BYTES {
        return Err(SvgVisualError::InputTooLarge {
            actual: input.len() as u64,
            limit: MAX_SVG_VISUAL_INPUT_BYTES,
        });
    }
    if has_disallowed_svg_external_references(&input) {
        return Err(SvgVisualError::BlockedReference);
    }
    admit_svg_visual_complexity(&input)?;
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

fn admit_svg_visual_complexity(input: &[u8]) -> Result<(), SvgVisualError> {
    use quick_xml::events::Event;

    let mut reader = quick_xml::Reader::from_reader(input);
    reader.config_mut().check_end_names = true;
    let mut budget = SvgAdmissionBudget::default();
    let mut depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                budget.elements = budget.elements.saturating_add(1);
                depth = depth.saturating_add(1);
                admit_svg_element_bounds(&event, &mut budget, depth)?;
            }
            Ok(Event::Empty(event)) => {
                budget.elements = budget.elements.saturating_add(1);
                admit_svg_element_bounds(&event, &mut budget, depth.saturating_add(1))?;
            }
            Ok(Event::End(_)) => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    SvgVisualError::ComplexityLimit("XML depth underflow".to_string())
                })?;
            }
            Ok(Event::DocType(_) | Event::PI(_)) => {
                return Err(SvgVisualError::ComplexityLimit(
                    "DTD and processing instructions are not permitted".to_string(),
                ));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(SvgVisualError::Parse(error.to_string())),
        }
    }
    if depth != 0 {
        return Err(SvgVisualError::Parse(
            "SVG has unbalanced XML elements".to_string(),
        ));
    }
    Ok(())
}

#[derive(Default)]
struct SvgAdmissionBudget {
    elements: usize,
    attributes: usize,
    attribute_bytes: usize,
    path_data_bytes: usize,
    local_paint_references: usize,
    filter_definitions: usize,
    filter_primitives: usize,
    filter_applications: usize,
}

fn admit_svg_element_bounds(
    event: &quick_xml::events::BytesStart<'_>,
    budget: &mut SvgAdmissionBudget,
    depth: usize,
) -> Result<(), SvgVisualError> {
    if budget.elements > MAX_SVG_XML_ELEMENTS {
        return Err(SvgVisualError::ComplexityLimit(format!(
            "more than {MAX_SVG_XML_ELEMENTS} XML elements"
        )));
    }
    if depth > MAX_SVG_XML_DEPTH {
        return Err(SvgVisualError::ComplexityLimit(format!(
            "XML nesting exceeds {MAX_SVG_XML_DEPTH} levels"
        )));
    }
    let raw_name = event.name();
    let element_name = raw_name
        .as_ref()
        .rsplit(|byte| *byte == b':')
        .next()
        .unwrap_or(raw_name.as_ref());
    if matches!(
        element_name,
        b"use"
            | b"pattern"
            | b"mask"
            | b"clipPath"
            | b"marker"
            | b"image"
            | b"foreignObject"
            | b"style"
            | b"script"
            | b"animate"
            | b"animateMotion"
            | b"animateTransform"
            | b"set"
    ) {
        return Err(SvgVisualError::ComplexityLimit(format!(
            "{0} elements are not admitted because they permit semantic reuse, embedded content, CSS, or animation",
            String::from_utf8_lossy(element_name)
        )));
    }
    if element_name == b"filter" {
        budget.filter_definitions = budget.filter_definitions.saturating_add(1);
        if budget.filter_definitions > MAX_SVG_FILTER_DEFINITIONS {
            return Err(SvgVisualError::ComplexityLimit(format!(
                "more than {MAX_SVG_FILTER_DEFINITIONS} filter definitions"
            )));
        }
    } else if element_name.starts_with(b"fe") {
        if element_name != b"feDropShadow" {
            return Err(SvgVisualError::ComplexityLimit(format!(
                "{} filter primitives are not admitted",
                String::from_utf8_lossy(element_name)
            )));
        }
        budget.filter_primitives = budget.filter_primitives.saturating_add(1);
        if budget.filter_primitives > MAX_SVG_FILTER_PRIMITIVES {
            return Err(SvgVisualError::ComplexityLimit(format!(
                "more than {MAX_SVG_FILTER_PRIMITIVES} filter primitives"
            )));
        }
    }

    for attribute in event.attributes() {
        let attribute = attribute.map_err(|error| SvgVisualError::Parse(error.to_string()))?;
        let attribute_name = attribute
            .key
            .as_ref()
            .rsplit(|byte| *byte == b':')
            .next()
            .unwrap_or(attribute.key.as_ref());
        budget.attributes = budget.attributes.saturating_add(1);
        budget.attribute_bytes = budget.attribute_bytes.saturating_add(attribute.value.len());
        if budget.attributes > MAX_SVG_XML_ATTRIBUTES {
            return Err(SvgVisualError::ComplexityLimit(format!(
                "more than {MAX_SVG_XML_ATTRIBUTES} XML attributes"
            )));
        }
        if budget.attribute_bytes > MAX_SVG_XML_ATTRIBUTE_BYTES {
            return Err(SvgVisualError::ComplexityLimit(format!(
                "more than {MAX_SVG_XML_ATTRIBUTE_BYTES} bytes of XML attribute data"
            )));
        }
        if attribute_name == b"style" {
            return Err(SvgVisualError::ComplexityLimit(
                "CSS style attributes are not admitted because their render work is not structurally countable"
                    .to_string(),
            ));
        }
        if attribute_name == b"href" {
            return Err(SvgVisualError::ComplexityLimit(
                "local or external href reuse is not admitted".to_string(),
            ));
        }
        if attribute
            .value
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"url("))
        {
            let value = std::str::from_utf8(attribute.value.as_ref())
                .map_err(|error| SvgVisualError::Parse(error.to_string()))?;
            if !value.trim().starts_with("url(#") {
                return Err(SvgVisualError::ComplexityLimit(
                    "only local paint-server references are admitted".to_string(),
                ));
            }
            budget.local_paint_references = budget.local_paint_references.saturating_add(1);
            if budget.local_paint_references > MAX_SVG_LOCAL_PAINT_REFERENCES {
                return Err(SvgVisualError::ComplexityLimit(format!(
                    "more than {MAX_SVG_LOCAL_PAINT_REFERENCES} local paint-server references"
                )));
            }
        }
        if attribute_name == b"filter" {
            budget.filter_applications = budget.filter_applications.saturating_add(1);
            if budget.filter_applications > MAX_SVG_FILTER_APPLICATIONS {
                return Err(SvgVisualError::ComplexityLimit(format!(
                    "more than {MAX_SVG_FILTER_APPLICATIONS} filter applications"
                )));
            }
        }
        if element_name == b"feDropShadow" && attribute_name == b"stdDeviation" {
            let value = std::str::from_utf8(attribute.value.as_ref())
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| {
                    SvgVisualError::ComplexityLimit(
                        "feDropShadow stdDeviation must be one finite non-negative number"
                            .to_string(),
                    )
                })?;
            if value > MAX_SVG_FILTER_STD_DEVIATION {
                return Err(SvgVisualError::ComplexityLimit(format!(
                    "feDropShadow stdDeviation exceeds {MAX_SVG_FILTER_STD_DEVIATION}"
                )));
            }
        }
        if element_name == b"path" && attribute_name == b"d" {
            budget.path_data_bytes = budget.path_data_bytes.saturating_add(attribute.value.len());
            if budget.path_data_bytes > MAX_SVG_PATH_DATA_BYTES {
                return Err(SvgVisualError::ComplexityLimit(format!(
                    "more than {MAX_SVG_PATH_DATA_BYTES} bytes of path data"
                )));
            }
        }
    }
    Ok(())
}

fn has_disallowed_svg_external_references(data: &[u8]) -> bool {
    let Ok(svg) = std::str::from_utf8(data) else {
        return false;
    };
    // XML permits whitespace around `=`. Normalize it before checking so a
    // file or network reference cannot evade the admission boundary with
    // otherwise harmless formatting changes.
    let compact = svg
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let compact = String::from_utf8(compact).expect("ASCII lowercasing preserves UTF-8 bytes");
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
    compact.contains("<script")
        || BLOCKED_ATTRS
            .iter()
            .any(|pattern| compact.contains(pattern))
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
            let recursive_size = summarize_folder_recursive(path, MAX_FOLDER_RECURSIVE_ENTRIES);
            let mut lines = vec![
                "Filetype: Folder".to_string(),
                format!("Items: {total}"),
                format!("Folders: {folders}"),
                format!("Files: {files}"),
                format!("Known direct file size: {}", format_size(direct_file_bytes)),
                recursive_size.display_line(),
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
                spreadsheet: None,
                open_external: false,
            }
        }
        Err(err) => PreviewData {
            title,
            kind: PreviewKind::FolderMetadata,
            body: format!("Folder metadata unavailable: {err}"),
            spreadsheet: None,
            open_external: false,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FolderRecursiveSize {
    bytes: u64,
    incomplete: bool,
}

impl FolderRecursiveSize {
    fn display_line(self) -> String {
        let prefix = if self.incomplete { "At least " } else { "" };
        let suffix = if self.incomplete {
            " (scan bounded)"
        } else {
            ""
        };
        format!(
            "Known recursive file size: {prefix}{}{suffix}",
            format_size(self.bytes)
        )
    }
}

fn summarize_folder_recursive(path: &Path, entry_limit: usize) -> FolderRecursiveSize {
    #[cfg(unix)]
    {
        summarize_folder_recursive_with_before_directory_open(path, entry_limit, |_| {})
    }

    #[cfg(not(unix))]
    {
        let _ = (path, entry_limit);
        FolderRecursiveSize {
            bytes: 0,
            incomplete: true,
        }
    }
}

#[cfg(unix)]
fn summarize_folder_recursive_with_before_directory_open(
    path: &Path,
    entry_limit: usize,
    mut before_directory_open: impl FnMut(&OsStr),
) -> FolderRecursiveSize {
    let mut size = FolderRecursiveSize {
        bytes: 0,
        incomplete: false,
    };
    let root = match open_directory_no_follow(path) {
        Ok(root) => root,
        Err(_) => {
            size.incomplete = true;
            return size;
        }
    };
    let mut pending = vec![(root, 0usize)];
    let mut visited_entries = 0usize;

    while let Some((directory, depth)) = pending.pop() {
        let entries = match pinned_directory_entries(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                size.incomplete = true;
                continue;
            }
        };
        for name in entries {
            if visited_entries >= entry_limit {
                size.incomplete = true;
                return size;
            }
            visited_entries += 1;
            let metadata = match metadata_at_no_follow(&directory, &name) {
                Ok(metadata) => metadata,
                Err(_) => {
                    size.incomplete = true;
                    continue;
                }
            };
            match metadata.st_mode & libc::S_IFMT {
                libc::S_IFLNK => {}
                libc::S_IFREG => {
                    size.bytes = size.bytes.saturating_add(metadata.st_size.max(0) as u64);
                }
                libc::S_IFDIR => {
                    if depth < MAX_FOLDER_RECURSIVE_DEPTH {
                        before_directory_open(&name);
                        match open_directory_at_no_follow(&directory, &name) {
                            Ok(child) => pending.push((child, depth + 1)),
                            Err(_) => size.incomplete = true,
                        }
                    } else {
                        size.incomplete = true;
                    }
                }
                _ => {}
            }
        }
    }
    size
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
fn open_directory_at_no_follow(directory: &File, name: &OsStr) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes()).expect("directory entry names cannot contain NUL");
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn metadata_at_no_follow(directory: &File, name: &OsStr) -> std::io::Result<libc::stat> {
    let name = CString::new(name.as_bytes()).expect("directory entry names cannot contain NUL");
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { metadata.assume_init() })
}

#[cfg(unix)]
fn pinned_directory_entries(directory: &File) -> std::io::Result<Vec<OsString>> {
    let duplicate = unsafe { libc::dup(directory.as_raw_fd()) };
    if duplicate < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe { libc::close(duplicate) };
        return Err(std::io::Error::last_os_error());
    }

    let mut names = Vec::new();
    loop {
        clear_errno();
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let result = std::io::Error::last_os_error();
            unsafe { libc::closedir(stream) };
            return if result.raw_os_error() == Some(0) {
                Ok(names)
            } else {
                Err(result)
            };
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name != b"." && name != b".." {
            names.push(OsString::from_vec(name.to_vec()));
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn clear_errno() {
    unsafe { *libc::__errno_location() = 0 };
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn clear_errno() {
    unsafe { *libc::__error() = 0 };
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
                spreadsheet: None,
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
                spreadsheet: None,
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
                spreadsheet: None,
                open_external: true,
            }
        }
        Err(_) => {
            let info = metadata_summary(path, "Audio file.", Some("Could not read audio tags."));
            PreviewData {
                title,
                kind: PreviewKind::AudioMetadata,
                body: info,
                spreadsheet: None,
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
        spreadsheet: None,
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
    if ext == "epub" {
        return preview_epub(title, path);
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
        spreadsheet: None,
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
            spreadsheet: None,
            open_external: true,
        };
    }

    let Ok(file) = std::fs::File::open(path) else {
        let info = metadata_summary(path, "TAR archive.", Some("Could not open archive."));
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            spreadsheet: None,
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
            spreadsheet: None,
            open_external: true,
        };
    }
    let Ok(file) = std::fs::File::open(path) else {
        let info = metadata_summary(path, "TAR.GZ archive.", Some("Could not open archive."));
        return PreviewData {
            title,
            kind: PreviewKind::ArchiveMetadata,
            body: info,
            spreadsheet: None,
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
                    spreadsheet: None,
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
                    spreadsheet: None,
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
                        spreadsheet: None,
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
                            spreadsheet: None,
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
                        spreadsheet: None,
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
                        spreadsheet: None,
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
                    spreadsheet: None,
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
                spreadsheet: None,
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
        spreadsheet: None,
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
                spreadsheet: None,
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
                spreadsheet: None,
                open_external: true,
            }
        }
    }
}

fn epub_error(title: String, path: &Path) -> PreviewData {
    PreviewData {
        title,
        kind: PreviewKind::ArchiveMetadata,
        body: metadata_summary(
            path,
            "EPUB ebook.",
            Some("Could not read EPUB metadata safely."),
        ),
        spreadsheet: None,
        open_external: true,
    }
}

fn preview_epub(title: String, path: &Path) -> PreviewData {
    let Some(snapshot) = read_bounded_epub_snapshot(path) else {
        return epub_error(title, path);
    };
    let input_size = snapshot.len() as u64;
    let Some(directory) = preflight_epub_zip(&snapshot) else {
        return epub_error(title, path);
    };
    // Preflight and ZIP parsing intentionally consume this same immutable byte
    // snapshot; metadata followed by reopening the pathname would leave a
    // mutable-file race between the two safety decisions.
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(snapshot)) else {
        return epub_error(title, path);
    };
    let mimetype_is_first = archive
        .by_index_raw(0)
        .is_ok_and(|entry| entry.name() == "mimetype" && entry.header_start() == 0);
    if archive.len() != directory.entry_count
        || !mimetype_is_first
        || !zip_entry_is_unique(&archive, "mimetype")
    {
        return epub_error(title, path);
    }

    let mut enumerated = 0usize;
    for index in 0..archive.len() {
        enumerated += 1;
        if enumerated > MAX_EPUB_PARTS || archive.by_index_raw(index).is_err() {
            return epub_error(title, path);
        }
    }
    if enumerated != directory.entry_count {
        return epub_error(title, path);
    }

    let mimetype_is_valid = match archive.by_name("mimetype") {
        Ok(entry)
            if !entry.encrypted()
                && entry.compression() == zip::CompressionMethod::Stored
                && entry.size() == b"application/epub+zip".len() as u64 =>
        {
            let mut bytes = Vec::with_capacity(b"application/epub+zip".len());
            entry
                .take(b"application/epub+zip".len() as u64 + 1)
                .read_to_end(&mut bytes)
                .is_ok_and(|_| bytes == b"application/epub+zip")
        }
        _ => false,
    };
    if !mimetype_is_valid || !zip_entry_is_unique(&archive, "META-INF/container.xml") {
        return epub_error(title, path);
    }

    let Some(container) = zip_entry_text(&mut archive, "META-INF/container.xml") else {
        return epub_error(title, path);
    };
    let Some(package_path) = epub_package_path(&container) else {
        return epub_error(title, path);
    };
    if !zip_entry_is_unique(&archive, &package_path) {
        return epub_error(title, path);
    }
    let Some(package) = zip_entry_text(&mut archive, &package_path) else {
        return epub_error(title, path);
    };
    let Some((book_title, author)) = extract_epub_metadata(&package) else {
        return epub_error(title, path);
    };

    let mut lines = vec!["Filetype: EPUB ebook".to_string()];
    if let Some(book_title) = book_title {
        lines.push(format!("Title: {book_title}"));
    }
    if let Some(author) = author {
        lines.push(format!("Author: {author}"));
    }
    lines.push(format!("Internal parts: {}", archive.len()));
    lines.push(format!("Size: {}", format_size(input_size)));
    PreviewData {
        title,
        kind: PreviewKind::ArchiveMetadata,
        body: lines.join("\n"),
        spreadsheet: None,
        open_external: true,
    }
}

fn read_bounded_epub_snapshot(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bounded = file.take(MAX_EPUB_PACKAGE_BYTES.checked_add(1)?);
    let mut snapshot = Vec::new();
    bounded.read_to_end(&mut snapshot).ok()?;
    (snapshot.len() as u64 <= MAX_EPUB_PACKAGE_BYTES).then_some(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EpubZipDirectory {
    entry_count: usize,
}

/// Parses the bounded ZIP footer before `ZipArchive` can materialize central
/// directory entries. EPUB has no need for multi-disk archives, so every
/// offset is required to describe one contiguous, in-file directory.
fn preflight_epub_zip(bytes: &[u8]) -> Option<EpubZipDirectory> {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";
    const EOCD_MIN_BYTES: usize = 22;

    let file_len = bytes.len() as u64;
    if file_len < EOCD_MIN_BYTES as u64 || file_len > MAX_EPUB_PACKAGE_BYTES {
        return None;
    }

    // EOCD comments can be at most 65,535 bytes, but an appended complete
    // archive can put an earlier valid footer much farther away. Scan the
    // already-bounded immutable snapshot once and reject any second valid
    // directory/footer pair instead of trusting only the final comment window.
    let mut valid_footer_count = 0usize;
    let mut final_directory = None;
    for (offset, window) in bytes.windows(EOCD_SIGNATURE.len()).enumerate() {
        if window != EOCD_SIGNATURE {
            continue;
        }
        let Some((directory, footer_end)) = epub_zip_directory_at_eocd(bytes, offset as u64) else {
            continue;
        };
        valid_footer_count = valid_footer_count.checked_add(1)?;
        if valid_footer_count > 1 {
            return None;
        }
        if footer_end == file_len {
            final_directory = Some(directory);
        }
    }
    (valid_footer_count == 1).then_some(final_directory?)
}

/// Validates one complete archive footer and the central directory it governs.
/// `footer_end` is returned separately so callers can distinguish an appended
/// competing archive from the sole footer that terminates the snapshot.
fn epub_zip_directory_at_eocd(bytes: &[u8], eocd_offset: u64) -> Option<(EpubZipDirectory, u64)> {
    const EOCD_SIGNATURE: &[u8; 4] = b"PK\x05\x06";

    const EOCD_MIN_BYTES: u64 = 22;

    let file_len = bytes.len() as u64;
    let eocd_end = eocd_offset.checked_add(EOCD_MIN_BYTES)?;
    let eocd = bytes.get(eocd_offset as usize..eocd_end as usize)?;
    if eocd[..4] != *EOCD_SIGNATURE {
        return None;
    }
    let footer_end = eocd_end.checked_add(u64::from(read_le_u16(&eocd[20..22])?))?;
    if footer_end > file_len {
        return None;
    }
    let disk = read_le_u16(&eocd[4..6])?;
    let directory_disk = read_le_u16(&eocd[6..8])?;
    let entries_on_disk = read_le_u16(&eocd[8..10])?;
    let total_entries = read_le_u16(&eocd[10..12])?;
    let directory_size = read_le_u32(&eocd[12..16])?;
    let directory_offset = read_le_u32(&eocd[16..20])?;

    let uses_zip64 = entries_on_disk == u16::MAX
        || total_entries == u16::MAX
        || directory_size == u32::MAX
        || directory_offset == u32::MAX;
    // The bounded EPUB surface cannot need ZIP64: packages are at most
    // 128 MiB with at most 4,096 entries and an 8 MiB central directory.
    // Refuse the alternate metadata graph rather than reconcile two sets of
    // attacker-controlled sizes and offsets before `ZipArchive`.
    if uses_zip64 || disk != 0 || directory_disk != 0 || entries_on_disk != total_entries {
        return None;
    }
    let entry_count = u64::from(total_entries);
    let directory_size = u64::from(directory_size);
    let directory_offset = u64::from(directory_offset);
    let directory_end = eocd_offset;

    let entry_count = usize::try_from(entry_count).ok()?;
    if entry_count > MAX_EPUB_PARTS || directory_size > MAX_EPUB_CENTRAL_DIRECTORY_BYTES {
        return None;
    }
    if directory_offset.checked_add(directory_size)? != directory_end || directory_end > file_len {
        return None;
    }
    if !epub_central_directory_is_well_formed(bytes, directory_offset, directory_size, entry_count)
    {
        return None;
    }
    Some((EpubZipDirectory { entry_count }, footer_end))
}

fn epub_central_directory_is_well_formed(
    bytes: &[u8],
    directory_offset: u64,
    directory_size: u64,
    entry_count: usize,
) -> bool {
    const CENTRAL_DIRECTORY_SIGNATURE: &[u8; 4] = b"PK\x01\x02";
    const CENTRAL_DIRECTORY_HEADER_BYTES: u64 = 46;

    let Some(directory_end) = directory_offset.checked_add(directory_size) else {
        return false;
    };
    let mut offset = directory_offset;
    let mut local_regions = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let Some(header_end) = offset.checked_add(CENTRAL_DIRECTORY_HEADER_BYTES) else {
            return false;
        };
        let Some(header) = bytes.get(offset as usize..header_end as usize) else {
            return false;
        };
        if header[..4] != *CENTRAL_DIRECTORY_SIGNATURE {
            return false;
        }
        let Some(name_len) = read_le_u16(&header[28..30]).map(u64::from) else {
            return false;
        };
        let Some(extra_len) = read_le_u16(&header[30..32]).map(u64::from) else {
            return false;
        };
        let Some(comment_len) = read_le_u16(&header[32..34]).map(u64::from) else {
            return false;
        };
        let Some(record_len) = CENTRAL_DIRECTORY_HEADER_BYTES
            .checked_add(name_len)
            .and_then(|value| value.checked_add(extra_len))
            .and_then(|value| value.checked_add(comment_len))
        else {
            return false;
        };
        let Some(next) = offset.checked_add(record_len) else {
            return false;
        };
        if next > directory_end {
            return false;
        }
        let Some(extra_start) = header_end.checked_add(name_len) else {
            return false;
        };
        let Some(extra_end) = extra_start.checked_add(extra_len) else {
            return false;
        };
        let Some(extra) = bytes.get(extra_start as usize..extra_end as usize) else {
            return false;
        };
        let Some(name) = bytes.get(header_end as usize..extra_start as usize) else {
            return false;
        };
        if !epub_central_directory_zip64_is_consistent(header, extra) {
            return false;
        }
        let Some(local_header_offset) = read_le_u32(&header[42..46]).map(u64::from) else {
            return false;
        };
        let Some(local_region) =
            epub_local_entry_region(bytes, local_header_offset, directory_offset, header, name)
        else {
            return false;
        };
        local_regions.push(local_region);
        offset = next;
    }
    local_regions.sort_unstable_by_key(|region| region.0);
    let local_regions_are_disjoint = local_regions
        .windows(2)
        .all(|regions| regions[0].1 <= regions[1].0);
    offset == directory_end && local_regions_are_disjoint
}

/// The bounded EPUB surface accepts at most 128 MiB and 4,096 entries, so no
/// central-directory size, offset, or disk field can legitimately require a
/// ZIP64 replacement. Reject ZIP64 entry metadata instead of maintaining a
/// second archive parser whose values could disagree with `ZipArchive`.
fn epub_central_directory_zip64_is_consistent(header: &[u8], extra: &[u8]) -> bool {
    read_le_u32(&header[24..28]).is_some_and(|value| value != u32::MAX)
        && read_le_u32(&header[20..24]).is_some_and(|value| value != u32::MAX)
        && read_le_u32(&header[42..46]).is_some_and(|value| value != u32::MAX)
        && read_le_u16(&header[34..36]) == Some(0)
        && epub_extra_fields_are_classic(extra)
}

fn epub_local_entry_region(
    bytes: &[u8],
    offset: u64,
    directory_offset: u64,
    central: &[u8],
    central_name: &[u8],
) -> Option<(u64, u64)> {
    const LOCAL_HEADER_BYTES: u64 = 30;
    const DATA_DESCRIPTOR_SIGNATURE: u32 = 0x0807_4b50;
    let header_end = offset.checked_add(LOCAL_HEADER_BYTES)?;
    let header = bytes.get(offset as usize..header_end as usize)?;
    if &header[..4] != b"PK\x03\x04"
        || read_le_u32(&header[18..22]) == Some(u32::MAX)
        || read_le_u32(&header[22..26]) == Some(u32::MAX)
        || header[6..10] != central[8..12]
    {
        return None;
    }
    let name_len = u64::from(read_le_u16(&header[26..28])?);
    let extra_len = u64::from(read_le_u16(&header[28..30])?);
    let extra_start = header_end.checked_add(name_len)?;
    let extra_end = extra_start.checked_add(extra_len)?;
    if extra_end > directory_offset {
        return None;
    }
    let extra = bytes.get(extra_start as usize..extra_end as usize)?;
    let name = bytes.get(header_end as usize..extra_start as usize)?;
    if name != central_name || !epub_extra_fields_are_classic(extra) {
        return None;
    }

    let flags = read_le_u16(&central[8..10])?;
    let crc = read_le_u32(&central[16..20])?;
    let compressed_size = read_le_u32(&central[20..24])?;
    let uncompressed_size = read_le_u32(&central[24..28])?;
    let data_end = extra_end.checked_add(u64::from(compressed_size))?;
    if data_end > directory_offset {
        return None;
    }

    let entry_end = if flags & 0x0008 == 0 {
        if read_le_u32(&header[14..18]) != Some(crc)
            || read_le_u32(&header[18..22]) != Some(compressed_size)
            || read_le_u32(&header[22..26]) != Some(uncompressed_size)
        {
            return None;
        }
        data_end
    } else {
        let signatureless_end = data_end.checked_add(12)?;
        let signatureless_matches = signatureless_end <= directory_offset
            && bytes
                .get(data_end as usize..signatureless_end as usize)
                .is_some_and(|values| {
                    read_le_u32(&values[0..4]) == Some(crc)
                        && read_le_u32(&values[4..8]) == Some(compressed_size)
                        && read_le_u32(&values[8..12]) == Some(uncompressed_size)
                });

        let signed_end = data_end.checked_add(16)?;
        let signed_matches = signed_end <= directory_offset
            && bytes
                .get(data_end as usize..signed_end as usize)
                .is_some_and(|descriptor| {
                    read_le_u32(&descriptor[0..4]) == Some(DATA_DESCRIPTOR_SIGNATURE)
                        && read_le_u32(&descriptor[4..8]) == Some(crc)
                        && read_le_u32(&descriptor[8..12]) == Some(compressed_size)
                        && read_le_u32(&descriptor[12..16]) == Some(uncompressed_size)
                });

        match (signatureless_matches, signed_matches) {
            (true, false) => signatureless_end,
            (false, true) => signed_end,
            _ => return None,
        }
    };
    Some((offset, entry_end))
}

fn epub_extra_fields_are_classic(extra: &[u8]) -> bool {
    const ZIP64_EXTRA_FIELD_ID: u16 = 0x0001;

    let mut offset = 0usize;
    while offset < extra.len() {
        let Some(field_header_end) = offset.checked_add(4) else {
            return false;
        };
        let Some(field_header) = extra.get(offset..field_header_end) else {
            return false;
        };
        let Some(field_len) = read_le_u16(&field_header[2..4]).map(usize::from) else {
            return false;
        };
        let Some(field_end) = field_header_end.checked_add(field_len) else {
            return false;
        };
        if extra.get(field_header_end..field_end).is_none()
            || read_le_u16(&field_header[..2]) == Some(ZIP64_EXTRA_FIELD_ID)
        {
            return false;
        }
        offset = field_end;
    }

    true
}

fn read_le_u16(bytes: &[u8]) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.try_into().ok()?))
}

fn read_le_u32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn zip_entry_is_unique<R: std::io::Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
    name: &str,
) -> bool {
    archive
        .file_names()
        .filter(|entry| *entry == name)
        .take(2)
        .count()
        == 1
}

fn epub_package_path(container: &str) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;

    let mut reader = quick_xml::NsReader::from_str(container);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut saw_container = false;
    let mut saw_rootfiles = false;
    let mut package_path = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                depth = depth.checked_add(1)?;
                let (namespace, local) = reader.resolve_element(event.name());
                let is_container = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_CONTAINER_NAMESPACE)
                    && local.as_ref() == b"container";
                let is_rootfiles = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_CONTAINER_NAMESPACE)
                    && local.as_ref() == b"rootfiles";
                match depth {
                    1 if !saw_container && is_container => {
                        saw_container = true;
                        let mut version = None;
                        for attribute in event.attributes() {
                            let attribute = attribute.ok()?;
                            if attribute.key.as_ref() == b"version"
                                && version
                                    .replace(attribute.unescape_value().ok()?.into_owned())
                                    .is_some()
                            {
                                return None;
                            }
                        }
                        if version.as_deref() != Some("1.0") {
                            return None;
                        }
                    }
                    2 if saw_container && !saw_rootfiles && is_rootfiles => saw_rootfiles = true,
                    _ => return None,
                }
            }
            Ok(Event::Empty(event)) => {
                let (namespace, local) = reader.resolve_element(event.name());
                let is_rootfile = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_CONTAINER_NAMESPACE)
                    && local.as_ref() == b"rootfile";
                if depth != 2 || !saw_rootfiles || !is_rootfile || package_path.is_some() {
                    return None;
                }
                let mut full_path = None;
                let mut media_type = None;
                for attribute in event.attributes() {
                    let attribute = attribute.ok()?;
                    let value = attribute.unescape_value().ok()?.into_owned();
                    if attribute.key.as_ref() == b"full-path" {
                        if full_path.replace(value).is_some() {
                            return None;
                        }
                    } else if attribute.key.as_ref() == b"media-type"
                        && media_type.replace(value).is_some()
                    {
                        return None;
                    }
                }
                let full_path = full_path?;
                if media_type.as_deref() != Some("application/oebps-package+xml")
                    || !epub_path_is_safe(&full_path)
                {
                    return None;
                }
                package_path = Some(full_path);
            }
            Ok(Event::End(event)) => {
                let local = event.local_name();
                match depth {
                    2 if local.as_ref() == b"rootfiles" && saw_rootfiles => {}
                    1 if local.as_ref() == b"container" && saw_container => {}
                    _ => return None,
                }
                depth -= 1;
            }
            Ok(Event::Text(event)) if event.decode().ok()?.trim().is_empty() => {}
            Ok(Event::Decl(_) | Event::Comment(_)) => {}
            Ok(Event::Eof) => {
                return (depth == 0 && saw_container && saw_rootfiles)
                    .then_some(package_path)
                    .flatten();
            }
            Ok(Event::DocType(_) | Event::GeneralRef(_) | Event::CData(_)) => return None,
            Err(_) => return None,
            _ => {}
        }
    }
}

fn epub_path_is_safe(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_EPUB_PACKAGE_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && path
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

fn extract_epub_metadata(package: &str) -> Option<(Option<String>, Option<String>)> {
    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;

    let mut reader = quick_xml::NsReader::from_str(package);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut saw_package = false;
    let mut saw_metadata = false;
    let mut metadata_depth = None;
    let mut title = String::new();
    let mut author = String::new();
    let mut active_field = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                depth = depth.checked_add(1)?;
                let (namespace, local) = reader.resolve_element(event.name());
                let is_package = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_PACKAGE_NAMESPACE)
                    && local.as_ref() == b"package";
                let is_metadata = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_PACKAGE_NAMESPACE)
                    && local.as_ref() == b"metadata";
                let is_dc_title = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_DC_NAMESPACE)
                    && local.as_ref() == b"title";
                let is_dc_creator = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_DC_NAMESPACE)
                    && local.as_ref() == b"creator";
                if depth == 1 {
                    if saw_package || !is_package {
                        return None;
                    }
                    saw_package = true;
                } else if local.as_ref() == b"metadata" {
                    if depth != 2 || !is_metadata || !saw_package || saw_metadata {
                        return None;
                    }
                    saw_metadata = true;
                    metadata_depth = Some(depth);
                } else if metadata_depth.is_some() && depth > 3 {
                    return None;
                } else if metadata_depth == Some(2) && depth == 3 {
                    if is_dc_title && title.is_empty() {
                        active_field = Some(true);
                    } else if is_dc_creator && author.is_empty() {
                        active_field = Some(false);
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                let (namespace, local) = reader.resolve_element(event.name());
                let is_metadata = matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == EPUB_PACKAGE_NAMESPACE)
                    && local.as_ref() == b"metadata";
                if local.as_ref() == b"metadata" {
                    if depth != 1 || !is_metadata || !saw_package || saw_metadata {
                        return None;
                    }
                    saw_metadata = true;
                } else if metadata_depth.is_some() {
                    return None;
                }
            }
            Ok(Event::End(event)) => {
                if metadata_depth == Some(depth) {
                    if event.local_name().as_ref() != b"metadata" {
                        return None;
                    }
                    metadata_depth = None;
                } else if active_field.is_some() && depth == 3 {
                    active_field = None;
                }
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            Ok(Event::Text(event)) => {
                let text = event.decode().ok()?;
                match active_field {
                    Some(true) => title.extend(
                        text.chars()
                            .take(MAX_EPUB_METADATA_CHARS.saturating_sub(title.chars().count())),
                    ),
                    Some(false) => author.extend(
                        text.chars()
                            .take(MAX_EPUB_METADATA_CHARS.saturating_sub(author.chars().count())),
                    ),
                    None => {}
                }
            }
            Ok(Event::Decl(_) | Event::Comment(_)) => {}
            Ok(Event::DocType(_) | Event::GeneralRef(_) | Event::CData(_)) => return None,
            Ok(Event::Eof) => {
                if depth != 0 || !saw_package || !saw_metadata || metadata_depth.is_some() {
                    return None;
                }
                let title = (!title.trim().is_empty()).then(|| title.trim().to_string());
                let author = (!author.trim().is_empty()).then(|| author.trim().to_string());
                return Some((title, author));
            }
            Err(_) => return None,
            _ => {}
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
        if matches!(ext.as_str(), "xlsx" | "xlsm" | "ods") {
            lines.push(
                "Spreadsheet preview limits: up to 12 worksheets, 4,096 populated cells per worksheet, and 1,024 characters per cell."
                    .to_string(),
            );
        }

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
        let spreadsheet = match ext.as_str() {
            "xlsx" | "xlsm" => extract_xlsx_spreadsheet(&mut archive),
            "ods" => extract_ods_spreadsheet(&mut archive),
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
            spreadsheet,
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
        spreadsheet: None,
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

/// Visual PPTX previews deliberately admit only a canonical, bounded first
/// slide. Metadata text can summarize more slides, but rendering a second
/// slide after an empty or malformed first slide would hide a damaged package
/// and make the preview's source ambiguous.
fn extract_pptx_visual_text<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<String> {
    let (_, name) = pptx_first_slide(archive)?;
    let xml = zip_entry_text(archive, &name)?;
    extract_pptx_slide_text(&xml)
}

fn pptx_package_is_safe<R: std::io::Read + std::io::Seek>(
    bytes: &[u8],
    archive: &mut zip::ZipArchive<R>,
) -> bool {
    let Some(directory) = preflight_epub_zip(bytes) else {
        return false;
    };
    if directory.entry_count != archive.len() || archive.len() > MAX_OFFICE_PARTS {
        return false;
    }
    let Some(raw_entry_types) = office_raw_zip_entry_types(bytes, directory.entry_count) else {
        return false;
    };
    let mut names = std::collections::HashSet::new();
    let mut total_uncompressed = 0u64;
    let mut relationship_parts = Vec::new();
    for (index, raw_entry_type) in raw_entry_types.iter().enumerate() {
        let Ok(entry) = archive.by_index_raw(index) else {
            return false;
        };
        let name = entry.name().to_string();
        // Package policy is case-insensitive even though ZIP names are not.
        // Keep the original name only for the exact `zip` lookup below.
        let policy_name = name.to_ascii_lowercase();
        if !safe_office_zip_name(&name)
            || pptx_part_is_forbidden(&policy_name)
            || !names.insert(policy_name.clone())
            || entry.size() > MAX_OFFICE_XML_BYTES
            || entry.compressed_size() > MAX_OFFICE_XML_BYTES
            || office_local_header_uses_data_descriptor(bytes, entry.header_start())
            || entry.encrypted()
            || entry.is_symlink()
            || (!entry.is_file() && !entry.is_dir())
            || !raw_entry_type.allows(entry.is_file(), entry.is_dir())
            || (entry.is_dir()
                && (entry.size() != 0 || entry.compressed_size() != 0 || entry.crc32() != 0))
            || (!entry.is_dir()
                && !matches!(
                    entry.compression(),
                    zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
                ))
        {
            return false;
        }
        let Some(next_total) = total_uncompressed.checked_add(entry.size()) else {
            return false;
        };
        total_uncompressed = next_total;
        if total_uncompressed > MAX_OFFICE_UNCOMPRESSED_BYTES
            || policy_name.starts_with("ppt/media/")
        {
            return false;
        }
        if policy_name.ends_with(".rels") {
            relationship_parts.push(name);
        }
    }
    if !(0..archive.len()).all(|index| office_zip_entry_decodes_to_eof(archive, index)) {
        return false;
    }
    if pptx_first_slide(archive).is_none() {
        return false;
    }
    relationship_parts.into_iter().all(|name| {
        zip_entry_text(archive, &name)
            .and_then(|xml| pptx_relationships_are_internal(&xml))
            .unwrap_or(false)
    })
}

fn safe_office_zip_name(name: &str) -> bool {
    let name = name.strip_suffix('/').unwrap_or(name);
    !name.is_empty()
        && !name.starts_with('/')
        && !name.starts_with('\\')
        && !name.contains(['\\', ':'])
        && !name.contains('\0')
        && name
            .split('/')
            .all(|component| !matches!(component, "" | "." | ".."))
}

/// Visual PPTX cards do not inspect or render packages that contain active,
/// auxiliary, or embedded content. Callers pass the case-normalized ZIP name.
fn pptx_part_is_forbidden(name: &str) -> bool {
    name.starts_with("ppt/vba")
        || name.starts_with("ppt/moderncomment")
        || matches!(name, "ppt/commentauthors.xml")
        || [
            "ppt/notesslides/",
            "ppt/comments/",
            "ppt/embeddings/",
            "ppt/externallinks/",
            "ppt/activex/",
            "ppt/ctrlprops/",
            "ppt/notesmasters/",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[derive(Clone, Copy)]
enum OfficeRawZipEntryType {
    NonUnix,
    Unix(u16),
}

impl OfficeRawZipEntryType {
    fn allows(self, is_file: bool, is_dir: bool) -> bool {
        match self {
            Self::NonUnix => true,
            Self::Unix(mode) if is_file => mode & 0o170000 == 0o100000,
            Self::Unix(mode) if is_dir => mode & 0o170000 == 0o040000,
            Self::Unix(_) => false,
        }
    }
}

/// Validates the raw ZIP names before `zip` decodes them. Office package part
/// names are UTF-8; accepting CP437 or a lossy replacement would make policy
/// classification diverge from the bytes that the central directory names.
fn office_raw_zip_entry_types(
    bytes: &[u8],
    expected_entries: usize,
) -> Option<Vec<OfficeRawZipEntryType>> {
    let eocd = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(offset, window)| {
            (window == b"PK\x05\x06")
                .then(|| bytes.get(offset + 20..offset + 22))
                .flatten()
                .map(|comment| {
                    (
                        offset,
                        usize::from(u16::from_le_bytes([comment[0], comment[1]])),
                    )
                })
        })
        .find(|(offset, comment_len)| offset.checked_add(22 + comment_len) == Some(bytes.len()))?;
    let footer = bytes.get(eocd.0..eocd.0.checked_add(22)?)?;
    let entries = usize::from(u16::from_le_bytes([footer[10], footer[11]]));
    if entries != expected_entries {
        return None;
    }
    let mut offset = usize::try_from(u32::from_le_bytes([
        footer[16], footer[17], footer[18], footer[19],
    ]))
    .ok()?;
    let mut result = Vec::with_capacity(entries);
    for _ in 0..entries {
        let header = bytes.get(offset..offset.checked_add(46)?)?;
        if header.get(..4) != Some(b"PK\x01\x02") {
            return None;
        }
        let made_by = u16::from_le_bytes([header[4], header[5]]);
        let attributes = u32::from_le_bytes([header[38], header[39], header[40], header[41]]);
        let entry_type = if (made_by >> 8) == 3 {
            OfficeRawZipEntryType::Unix((attributes >> 16) as u16)
        } else {
            OfficeRawZipEntryType::NonUnix
        };
        let name = usize::from(u16::from_le_bytes([header[28], header[29]]));
        let extra = usize::from(u16::from_le_bytes([header[30], header[31]]));
        let comment = usize::from(u16::from_le_bytes([header[32], header[33]]));
        let name_start = offset.checked_add(46)?;
        let name_end = name_start.checked_add(name)?;
        let central_name = bytes.get(name_start..name_end)?;
        std::str::from_utf8(central_name).ok()?;
        let central_extra_end = name_end.checked_add(extra)?;
        let central_extra = bytes.get(name_end..central_extra_end)?;

        let local_offset = usize::try_from(u32::from_le_bytes([
            header[42], header[43], header[44], header[45],
        ]))
        .ok()?;
        let local_header = bytes.get(local_offset..local_offset.checked_add(30)?)?;
        if local_header.get(..4) != Some(b"PK\x03\x04") {
            return None;
        }
        let local_name_len = usize::from(u16::from_le_bytes([local_header[26], local_header[27]]));
        let local_name_start = local_offset.checked_add(30)?;
        let local_name_end = local_name_start.checked_add(local_name_len)?;
        let local_name = bytes.get(local_name_start..local_name_end)?;
        std::str::from_utf8(local_name).ok()?;
        let local_extra_len = usize::from(u16::from_le_bytes([local_header[28], local_header[29]]));
        let local_extra_end = local_name_end.checked_add(local_extra_len)?;
        let local_extra = bytes.get(local_name_end..local_extra_end)?;
        if local_name != central_name || !office_extra_fields_match(central_extra, local_extra) {
            return None;
        }

        result.push(entry_type);
        offset = name_end.checked_add(extra)?.checked_add(comment)?;
    }
    Some(result)
}

/// The ZIP reader lets Info-ZIP's Unicode Path field replace the archive name
/// it reports. PPTX policy must classify the original central/local names, so
/// refuse that alternate name channel rather than reconcile two identities.
fn office_extra_fields_are_safe(extra: &[u8]) -> bool {
    const UNICODE_PATH_EXTRA_FIELD_ID: u16 = 0x7075;

    let mut offset = 0usize;
    let mut field_ids = std::collections::HashSet::new();
    while offset < extra.len() {
        let Some(header_end) = offset.checked_add(4) else {
            return false;
        };
        let Some(header) = extra.get(offset..header_end) else {
            return false;
        };
        let Some(field_len) = read_le_u16(&header[2..4]).map(usize::from) else {
            return false;
        };
        let Some(field_end) = header_end.checked_add(field_len) else {
            return false;
        };
        let Some(field_id) = read_le_u16(&header[..2]) else {
            return false;
        };
        if extra.get(header_end..field_end).is_none()
            || field_id == UNICODE_PATH_EXTRA_FIELD_ID
            || !field_ids.insert(field_id)
        {
            return false;
        }
        offset = field_end;
    }
    true
}

fn office_extra_fields_match(central: &[u8], local: &[u8]) -> bool {
    central == local && office_extra_fields_are_safe(central)
}

/// Forces `zip` to decompress and CRC-check every already-admitted file with
/// bounded memory. The logical content readers below may re-read selected
/// entries from the same immutable snapshot.
fn office_zip_entry_decodes_to_eof<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    index: usize,
) -> bool {
    let Ok(mut entry) = archive.by_index(index) else {
        return false;
    };
    if entry.is_dir() {
        return entry.size() == 0;
    }

    let expected_size = entry.size();
    let mut actual_size = 0u64;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let Ok(read) = entry.read(&mut buffer) else {
            return false;
        };
        if read == 0 {
            return actual_size == expected_size;
        }
        let Some(next_size) = actual_size.checked_add(read as u64) else {
            return false;
        };
        if next_size > expected_size {
            return false;
        }
        actual_size = next_size;
    }
}

fn office_local_header_uses_data_descriptor(bytes: &[u8], offset: u64) -> bool {
    usize::try_from(offset)
        .ok()
        .and_then(|start| bytes.get(start..start.checked_add(30)?))
        .and_then(|header| {
            (header.get(..4) == Some(b"PK\x03\x04"))
                .then(|| u16::from_le_bytes([header[6], header[7]]))
        })
        .is_none_or(|flags| flags & 0x0008 != 0)
}

fn pptx_first_slide<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<(u32, String)> {
    let mut slides = std::collections::BTreeMap::new();
    for index in 0..archive.len() {
        let name = archive.by_index_raw(index).ok()?.name().to_string();
        let Some(stem) = name
            .strip_prefix("ppt/slides/slide")
            .and_then(|name| name.strip_suffix(".xml"))
        else {
            continue;
        };
        let number = stem.parse::<u32>().ok()?;
        if number == 0 || stem != number.to_string() || slides.insert(number, name).is_some() {
            return None;
        }
    }
    slides.into_iter().next()
}

fn pptx_relationships_are_internal(xml: &str) -> Option<bool> {
    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;

    let mut reader = quick_xml::NsReader::from_str(xml);
    let mut depth = 0usize;
    let mut saw_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                depth = depth.checked_add(1)?;
                if depth > MAX_OFFICE_XML_DEPTH {
                    return None;
                }
                let (namespace, local) = reader.resolve_element(event.name());
                if !pptx_xml_attributes_are_well_formed(&reader, &event)
                    || (depth == 1
                        && (!matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == RELATIONSHIPS_NAMESPACE)
                            || local.as_ref() != b"Relationships"))
                    || (depth == 2
                        && (!matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == RELATIONSHIPS_NAMESPACE)
                            || local.as_ref() != b"Relationship"))
                    || (depth > 2)
                    || !pptx_relationship_event_is_internal(&reader, &event)?
                {
                    return Some(false);
                }
                saw_root = true;
            }
            Ok(Event::Empty(event)) => {
                let (namespace, local) = reader.resolve_element(event.name());
                if !pptx_xml_attributes_are_well_formed(&reader, &event)
                    || depth != 1
                    || !matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == RELATIONSHIPS_NAMESPACE)
                    || local.as_ref() != b"Relationship"
                    || !pptx_relationship_event_is_internal(&reader, &event)?
                {
                    return Some(false);
                }
            }
            Ok(Event::End(_)) => depth = depth.checked_sub(1)?,
            Ok(Event::Decl(_)) => {}
            Ok(Event::Text(event)) if event.decode().ok()?.trim().is_empty() => {}
            Ok(
                Event::Text(_)
                | Event::Comment(_)
                | Event::PI(_)
                | Event::DocType(_)
                | Event::GeneralRef(_)
                | Event::CData(_),
            ) => return None,
            Ok(Event::Eof) => return (depth == 0 && saw_root).then_some(true),
            Err(_) => return None,
        }
    }
}

fn pptx_relationship_event_is_internal(
    reader: &quick_xml::NsReader<&[u8]>,
    event: &quick_xml::events::BytesStart<'_>,
) -> Option<bool> {
    for attribute in event.attributes() {
        let attribute = attribute.ok()?;
        let (namespace, local) = reader.resolver().resolve_attribute(attribute.key);
        if matches!(namespace, quick_xml::name::ResolveResult::Unknown(_)) {
            return None;
        }
        let value = attribute.unescape_value().ok()?;
        if (local.as_ref() == b"TargetMode" && !value.eq_ignore_ascii_case("Internal"))
            || (local.as_ref() == b"Target" && pptx_relationship_target_is_external(&value))
        {
            return Some(false);
        }
    }
    Some(true)
}

fn pptx_relationship_target_is_external(target: &str) -> bool {
    let target = target.trim_start();
    target.starts_with("//") || target.starts_with("\\\\") || target.contains(':')
}

fn extract_pptx_slide_text(xml: &str) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::name::ResolveResult;

    let mut reader = quick_xml::NsReader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut text_depth = None;
    let mut text = String::new();
    let mut saw_root = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                depth = depth.checked_add(1)?;
                if depth > MAX_OFFICE_XML_DEPTH {
                    return None;
                }
                if !pptx_xml_attributes_are_well_formed(&reader, &event) {
                    return None;
                }
                let (namespace, local) = reader.resolve_element(event.name());
                if depth == 1 {
                    if saw_root
                        || !matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == PRESENTATIONML_NAMESPACE)
                        || local.as_ref() != b"sld"
                    {
                        return None;
                    }
                    saw_root = true;
                }
                if matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == DRAWINGML_NAMESPACE)
                    && local.as_ref() == b"t"
                    && text_depth.replace(depth).is_some()
                {
                    return None;
                }
            }
            Ok(Event::Empty(event)) => {
                if !pptx_xml_attributes_are_well_formed(&reader, &event) {
                    return None;
                }
                let (namespace, local) = reader.resolve_element(event.name());
                if depth == 0 && !saw_root {
                    return None;
                }
                if matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == DRAWINGML_NAMESPACE)
                    && local.as_ref() == b"t"
                {
                    text.push('\n');
                }
            }
            Ok(Event::End(event)) => {
                let (namespace, local) = reader.resolve_element(event.name());
                if text_depth == Some(depth) {
                    if !matches!(namespace, ResolveResult::Bound(namespace) if namespace.as_ref() == DRAWINGML_NAMESPACE)
                        || local.as_ref() != b"t"
                    {
                        return None;
                    }
                    text_depth = None;
                    text.push('\n');
                }
                depth = depth.checked_sub(1)?;
            }
            Ok(Event::Text(event)) if text_depth.is_some() => text.push_str(&event.decode().ok()?),
            Ok(Event::Text(_) | Event::Decl(_)) => {}
            Ok(
                Event::DocType(_)
                | Event::GeneralRef(_)
                | Event::CData(_)
                | Event::Comment(_)
                | Event::PI(_),
            ) => return None,
            Ok(Event::Eof) => {
                return (depth == 0 && saw_root && text_depth.is_none())
                    .then(|| normalized_preview_text(&text))
                    .flatten();
            }
            Err(_) => return None,
        }
    }
}

fn pptx_xml_attributes_are_well_formed(
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

fn append_bounded_spreadsheet_cell_chunk(
    cell: &mut String,
    omitted: &mut usize,
    chunk: &str,
    limit: usize,
) -> Option<()> {
    let remaining = limit.saturating_sub(cell.chars().count());
    let mut characters = chunk.chars();
    for character in characters.by_ref().take(remaining) {
        cell.push(character);
    }
    *omitted = omitted.checked_add(characters.count())?;
    Some(())
}

#[derive(Debug, Clone)]
struct BoundedSpreadsheetCellText {
    text: String,
    retained: usize,
    omitted: usize,
}

impl BoundedSpreadsheetCellText {
    fn new() -> Self {
        Self {
            text: String::new(),
            retained: 0,
            omitted: 0,
        }
    }

    fn append(&mut self, chunk: &str, limit: usize) -> Option<()> {
        let remaining = limit.saturating_sub(self.retained);
        let mut characters = chunk.chars();
        for character in characters.by_ref().take(remaining) {
            self.text.push(character);
            self.retained = self.retained.checked_add(1)?;
        }
        self.omitted = self.omitted.checked_add(characters.count())?;
        Some(())
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

    bounded_sheet_text(&out)
}

/// Builds the inspector-only spreadsheet payload after the established ODS
/// extractor has validated the archive. Unlike the legacy text card this never
/// serializes cell boundaries into tabs or lines.
fn extract_ods_spreadsheet<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<SpreadsheetPreview> {
    const MAX_SHEETS: usize = 12;
    const MAX_CELLS: usize = 4_096;
    const MAX_CELL_CHARS: usize = 1_024;

    // Keep the typed path fail-closed under exactly the XML validation rules
    // already exercised by the legacy extracted-text reader.
    extract_ods_text(archive)?;
    let xml = zip_entry_text(archive, "content.xml")?;
    let mut reader = quick_xml::NsReader::from_str(&xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().check_comments = true;
    let mut sheets = Vec::new();
    let mut omissions = Vec::new();
    let mut active = None::<SpreadsheetSheet>;
    let mut next_row = 0usize;
    let mut row_start = 0usize;
    let mut row_repeat = 1usize;
    let mut column = 0usize;
    let mut cell_start = 0usize;
    let mut cell_repeat = 1usize;
    let mut cell_text = String::new();
    let mut cell_text_omitted = 0usize;
    let mut in_cell = false;
    let mut in_paragraph = false;
    let mut omitted_cells = 0usize;

    loop {
        let (namespace, event) = reader.read_resolved_event().ok()?;
        let table_namespace = matches!(namespace, quick_xml::name::ResolveResult::Bound(namespace) if namespace.as_ref() == ODS_TABLE_NAMESPACE);
        let text_namespace = matches!(namespace, quick_xml::name::ResolveResult::Bound(namespace) if namespace.as_ref() == ODS_TEXT_NAMESPACE);
        match event {
            quick_xml::events::Event::Start(event) => {
                match (table_namespace, text_namespace, event.local_name().as_ref()) {
                    (true, _, b"table") => {
                        let name = ods_table_attribute(&reader, &event, b"name")
                            .ok()?
                            .unwrap_or_else(|| format!("Sheet {}", sheets.len() + 1));
                        if sheets.len() < MAX_SHEETS {
                            active = Some(SpreadsheetSheet {
                                id: sheets.len().to_string(),
                                name,
                                cells: Vec::new(),
                                omissions: Vec::new(),
                            });
                            next_row = 0;
                        } else {
                            omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::SheetLimit { omitted: 1 },
                            });
                        }
                    }
                    (true, _, b"table-row") if active.is_some() => {
                        row_start = next_row;
                        row_repeat = ods_repeat_value(&reader, &event, b"number-rows-repeated")?;
                        column = 0;
                    }
                    (true, _, b"table-cell" | b"covered-table-cell") if active.is_some() => {
                        cell_start = column;
                        cell_repeat =
                            ods_repeat_value(&reader, &event, b"number-columns-repeated")?;
                        cell_text.clear();
                        cell_text_omitted = 0;
                        in_cell = true;
                        in_paragraph = false;
                    }
                    (_, true, b"p") if in_cell => {
                        if !cell_text.is_empty() {
                            append_bounded_spreadsheet_cell_chunk(
                                &mut cell_text,
                                &mut cell_text_omitted,
                                "\n",
                                MAX_CELL_CHARS,
                            )?;
                        }
                        in_paragraph = true;
                    }
                    (_, true, b"line-break") if in_cell && in_paragraph => {
                        append_bounded_spreadsheet_cell_chunk(
                            &mut cell_text,
                            &mut cell_text_omitted,
                            "\n",
                            MAX_CELL_CHARS,
                        )?
                    }
                    (_, true, b"tab") if in_cell && in_paragraph => {
                        append_bounded_spreadsheet_cell_chunk(
                            &mut cell_text,
                            &mut cell_text_omitted,
                            "\t",
                            MAX_CELL_CHARS,
                        )?
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Empty(event) => {
                match (table_namespace, text_namespace, event.local_name().as_ref()) {
                    (true, _, b"table-row") if active.is_some() => {
                        let repeat = ods_repeat_value(&reader, &event, b"number-rows-repeated")?;
                        let (_, omitted_range) =
                            spreadsheet_coordinate_range(next_row, repeat, MAX_SPREADSHEET_ROWS)?;
                        if let (Some(sheet), Some((from, to))) = (active.as_mut(), omitted_range) {
                            sheet.omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::RowLimit { from, to },
                            });
                        }
                        next_row = next_row.checked_add(repeat)?;
                    }
                    (true, _, b"table-cell" | b"covered-table-cell") if active.is_some() => {
                        let repeat = ods_repeat_value(&reader, &event, b"number-columns-repeated")?;
                        let (_, omitted_range) =
                            spreadsheet_coordinate_range(column, repeat, MAX_SPREADSHEET_COLUMNS)?;
                        if let (Some(sheet), Some((from, to))) = (active.as_mut(), omitted_range) {
                            sheet.omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::ColumnLimit { from, to },
                            });
                        }
                        column = column.checked_add(repeat)?;
                    }
                    (_, true, b"line-break") if in_cell && in_paragraph => {
                        append_bounded_spreadsheet_cell_chunk(
                            &mut cell_text,
                            &mut cell_text_omitted,
                            "\n",
                            MAX_CELL_CHARS,
                        )?
                    }
                    (_, true, b"tab") if in_cell && in_paragraph => {
                        append_bounded_spreadsheet_cell_chunk(
                            &mut cell_text,
                            &mut cell_text_omitted,
                            "\t",
                            MAX_CELL_CHARS,
                        )?
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Text(event) if in_cell && in_paragraph => {
                append_bounded_spreadsheet_cell_chunk(
                    &mut cell_text,
                    &mut cell_text_omitted,
                    &event.xml_content().ok()?,
                    MAX_CELL_CHARS,
                )?
            }
            quick_xml::events::Event::CData(event) if in_cell && in_paragraph => {
                append_bounded_spreadsheet_cell_chunk(
                    &mut cell_text,
                    &mut cell_text_omitted,
                    &event.decode().ok()?,
                    MAX_CELL_CHARS,
                )?
            }
            quick_xml::events::Event::GeneralRef(event) if in_cell && in_paragraph => {
                append_bounded_spreadsheet_cell_chunk(
                    &mut cell_text,
                    &mut cell_text_omitted,
                    &quick_xml::escape::unescape(&format!("&{};", event.decode().ok()?)).ok()?,
                    MAX_CELL_CHARS,
                )?
            }
            quick_xml::events::Event::End(event) => {
                match (table_namespace, event.local_name().as_ref()) {
                    (true, b"table-cell" | b"covered-table-cell") if in_cell => {
                        if let Some(sheet) = active.as_mut() {
                            // Validate the declared repetition before narrowing it to the
                            // product coordinate bounds. A hostile cardinality must not become
                            // harmless merely because only a prefix is displayable.
                            let declared_text_omitted = cell_text_omitted
                                .checked_mul(row_repeat)?
                                .checked_mul(cell_repeat)?;
                            let (retained_rows, omitted_rows) = spreadsheet_coordinate_range(
                                row_start,
                                row_repeat,
                                MAX_SPREADSHEET_ROWS,
                            )?;
                            let (retained_columns, omitted_columns) = spreadsheet_coordinate_range(
                                cell_start,
                                cell_repeat,
                                MAX_SPREADSHEET_COLUMNS,
                            )?;
                            if let Some((from, to)) = omitted_rows {
                                sheet.omissions.push(SpreadsheetOmission {
                                    reason: SpreadsheetOmissionReason::RowLimit { from, to },
                                });
                            }
                            if let Some((from, to)) = omitted_columns {
                                sheet.omissions.push(SpreadsheetOmission {
                                    reason: SpreadsheetOmissionReason::ColumnLimit { from, to },
                                });
                            }
                            let occurrences = retained_rows.checked_mul(retained_columns)?;
                            if cell_text_omitted > 0 {
                                sheet.omissions.push(SpreadsheetOmission {
                                    reason: SpreadsheetOmissionReason::TextLimit {
                                        omitted: declared_text_omitted
                                            .min(cell_text_omitted.checked_mul(occurrences)?),
                                    },
                                });
                            }
                            if !cell_text.is_empty() {
                                let remaining = MAX_CELLS.saturating_sub(sheet.cells.len());
                                let materialized = occurrences.min(remaining);
                                for offset in 0..materialized {
                                    sheet.cells.push(SpreadsheetCell {
                                        row: row_start.checked_add(offset / retained_columns)?,
                                        column: cell_start
                                            .checked_add(offset % retained_columns)?,
                                        text: cell_text.clone(),
                                    });
                                }
                                omitted_cells =
                                    omitted_cells.checked_add(occurrences - materialized)?;
                            }
                        }
                        column = cell_start.checked_add(cell_repeat)?;
                        in_cell = false;
                        in_paragraph = false;
                        cell_text_omitted = 0;
                    }
                    (_, b"p") if in_cell && in_paragraph => in_paragraph = false,
                    (true, b"table-row") if active.is_some() => {
                        let (_, omitted_range) = spreadsheet_coordinate_range(
                            row_start,
                            row_repeat,
                            MAX_SPREADSHEET_ROWS,
                        )?;
                        if let (Some(sheet), Some((from, to))) = (active.as_mut(), omitted_range) {
                            sheet.omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::RowLimit { from, to },
                            });
                        }
                        next_row = row_start.checked_add(row_repeat)?
                    }
                    (true, b"table") => {
                        if let Some(mut sheet) = active.take() {
                            if omitted_cells > 0 {
                                sheet.omissions.push(SpreadsheetOmission {
                                    reason: SpreadsheetOmissionReason::CellLimit {
                                        omitted: omitted_cells,
                                    },
                                });
                                omitted_cells = 0;
                            }
                            sheets.push(sheet);
                        }
                    }
                    _ => {}
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    (!sheets.is_empty()).then_some(SpreadsheetPreview { sheets, omissions })
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
    bounded_sheet_text(&out)
}

const MAX_SPREADSHEET_ROWS: usize = 100_000;
const MAX_SPREADSHEET_COLUMNS: usize = 16_384;
const MAX_SHARED_STRING_SCAN: usize = 8_192;

fn spreadsheet_coordinate_range(
    start: usize,
    count: usize,
    limit: usize,
) -> Option<(usize, Option<(usize, usize)>)> {
    let end = start.checked_add(count.checked_sub(1)?)?;
    let retained = limit.saturating_sub(start).min(count);
    Some((retained, (end >= limit).then_some((start.max(limit), end))))
}

fn extract_xlsx_spreadsheet<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<SpreadsheetPreview> {
    const MAX_SHEETS: usize = 12;
    const MAX_CELLS: usize = 4_096;
    const MAX_CELL_CHARS: usize = 1_024;
    let entries = xlsx_sheet_entries(archive);
    let mut requested_shared_strings = std::collections::HashSet::new();
    for (_, path) in entries.iter().take(MAX_SHEETS) {
        let xml = zip_entry_text(archive, path)?;
        requested_shared_strings.extend(xlsx_referenced_shared_string_indices(&xml)?);
    }
    let shared_strings = zip_entry_text(archive, "xl/sharedStrings.xml")
        .and_then(|xml| {
            extract_xlsx_bounded_shared_strings(&xml, &requested_shared_strings, MAX_CELL_CHARS)
        })
        .unwrap_or_default();
    let mut sheets = Vec::new();
    let mut omissions = Vec::new();
    for (index, (name, path)) in entries.iter().take(MAX_SHEETS).enumerate() {
        let xml = zip_entry_text(archive, path)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(false);
        let mut cells = Vec::new();
        let mut sheet_omissions = Vec::new();
        let mut row = 0usize;
        let mut column = None;
        let mut cell_row = None;
        let mut kind = None::<String>;
        let mut value = BoundedSpreadsheetCellText::new();
        let mut collecting = false;
        let mut previous_row = None;
        let mut previous_column: Option<usize> = None;
        let mut omitted_cells = 0usize;
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"row" => {
                    row = xlsx_row_number(&event).unwrap_or(row);
                }
                Ok(quick_xml::events::Event::Empty(event)) if event.name().as_ref() == b"row" => {
                    row = xlsx_row_number(&event).unwrap_or(row).checked_add(1)?;
                }
                Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"c" => {
                    kind = None;
                    column = None;
                    cell_row = Some(row);
                    value = BoundedSpreadsheetCellText::new();
                    for attribute in event.attributes().flatten() {
                        if attribute.key.as_ref() == b"t" {
                            kind = String::from_utf8(attribute.value.to_vec()).ok();
                        }
                        if attribute.key.as_ref() == b"r" {
                            let reference = std::str::from_utf8(attribute.value.as_ref()).ok()?;
                            column = xlsx_column_index(reference);
                            cell_row = xlsx_row_index(reference).or(cell_row);
                        }
                    }
                }
                Ok(quick_xml::events::Event::Start(event))
                    if matches!(event.name().as_ref(), b"v" | b"t") =>
                {
                    collecting = true
                }
                Ok(quick_xml::events::Event::End(event))
                    if matches!(event.name().as_ref(), b"v" | b"t") =>
                {
                    collecting = false
                }
                Ok(quick_xml::events::Event::Text(event)) if collecting => {
                    value.append(&event.decode().ok()?, MAX_CELL_CHARS)?
                }
                Ok(quick_xml::events::Event::CData(event)) if collecting => {
                    value.append(&event.decode().ok()?, MAX_CELL_CHARS)?
                }
                Ok(quick_xml::events::Event::GeneralRef(event)) if collecting => {
                    let reference = event.decode().ok()?;
                    value.append(
                        &quick_xml::escape::unescape(&format!("&{reference};")).ok()?,
                        MAX_CELL_CHARS,
                    )?;
                }
                Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"c" => {
                    let Some(cell_row) = cell_row else { continue };
                    let cell_column = match column {
                        Some(column) => column,
                        None => match previous_column {
                            Some(value) => value.checked_add(1)?,
                            None => 0,
                        },
                    };
                    if cell_row >= MAX_SPREADSHEET_ROWS || cell_column >= MAX_SPREADSHEET_COLUMNS {
                        if cell_row >= MAX_SPREADSHEET_ROWS {
                            sheet_omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::RowLimit {
                                    from: cell_row,
                                    to: cell_row,
                                },
                            });
                        }
                        if cell_column >= MAX_SPREADSHEET_COLUMNS {
                            sheet_omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::ColumnLimit {
                                    from: cell_column,
                                    to: cell_column,
                                },
                            });
                        }
                        continue;
                    }
                    if cells.len() >= MAX_CELLS {
                        omitted_cells = omitted_cells.checked_add(1)?;
                        continue;
                    }
                    if previous_row.is_none() && cell_row > 0 {
                        sheet_omissions.push(SpreadsheetOmission {
                            reason: SpreadsheetOmissionReason::SparseRowGap {
                                from: 0,
                                to: cell_row - 1,
                            },
                        });
                    } else if let Some(previous) = previous_row
                        && cell_row > previous + 1
                    {
                        sheet_omissions.push(SpreadsheetOmission {
                            reason: SpreadsheetOmissionReason::SparseRowGap {
                                from: previous + 1,
                                to: cell_row - 1,
                            },
                        });
                    }
                    if previous_row != Some(cell_row) {
                        previous_column = None;
                    }
                    if previous_row.is_none() && cell_column > 0 {
                        sheet_omissions.push(SpreadsheetOmission {
                            reason: SpreadsheetOmissionReason::SparseColumnGap {
                                from: 0,
                                to: cell_column - 1,
                            },
                        });
                    } else if let Some(previous) = previous_column
                        && cell_column > previous + 1
                    {
                        sheet_omissions.push(SpreadsheetOmission {
                            reason: SpreadsheetOmissionReason::SparseColumnGap {
                                from: previous + 1,
                                to: cell_column - 1,
                            },
                        });
                    }
                    let raw_value =
                        std::mem::replace(&mut value, BoundedSpreadsheetCellText::new());
                    let value = if kind.as_deref() == Some("s") {
                        let index = raw_value.text.parse::<usize>().ok();
                        if let Some(mut shared) =
                            index.and_then(|index| shared_strings.get(&index)).cloned()
                        {
                            shared.omitted = shared.omitted.checked_add(raw_value.omitted)?;
                            shared
                        } else if let Some(index) = index {
                            sheet_omissions.push(SpreadsheetOmission {
                                reason: SpreadsheetOmissionReason::SharedStringUnavailable {
                                    index,
                                },
                            });
                            continue;
                        } else {
                            return None;
                        }
                    } else {
                        raw_value
                    };
                    if value.omitted > 0 {
                        sheet_omissions.push(SpreadsheetOmission {
                            reason: SpreadsheetOmissionReason::TextLimit {
                                omitted: value.omitted,
                            },
                        });
                    }
                    cells.push(SpreadsheetCell {
                        row: cell_row,
                        column: cell_column,
                        text: value.text,
                    });
                    previous_row = Some(cell_row);
                    previous_column = Some(cell_column);
                }
                Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"row" => {
                    row = row.checked_add(1)?;
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => return None,
                _ => {}
            }
        }
        if omitted_cells > 0 {
            sheet_omissions.push(SpreadsheetOmission {
                reason: SpreadsheetOmissionReason::CellLimit {
                    omitted: omitted_cells,
                },
            });
        }
        sheets.push(SpreadsheetSheet {
            id: index.to_string(),
            name: name.clone(),
            cells,
            omissions: sheet_omissions,
        });
    }
    if entries.len() > MAX_SHEETS {
        omissions.push(SpreadsheetOmission {
            reason: SpreadsheetOmissionReason::SheetLimit {
                omitted: entries.len() - MAX_SHEETS,
            },
        });
    }
    (!sheets.is_empty()).then_some(SpreadsheetPreview { sheets, omissions })
}

fn xlsx_row_index(reference: &str) -> Option<usize> {
    reference
        .bytes()
        .skip_while(|byte| byte.is_ascii_alphabetic())
        .collect::<Vec<_>>()
        .as_slice()
        .iter()
        .map(|byte| char::from(*byte))
        .collect::<String>()
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .map(|value| value - 1)
}

fn xlsx_row_number(event: &quick_xml::events::BytesStart<'_>) -> Option<usize> {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == b"r")
        .and_then(|attribute| {
            std::str::from_utf8(attribute.value.as_ref())
                .ok()?
                .parse::<usize>()
                .ok()
        })
        .filter(|value| *value > 0)
        .map(|value| value - 1)
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

fn xlsx_referenced_shared_string_indices(xml: &str) -> Option<std::collections::HashSet<usize>> {
    const MAX_CELLS: usize = 4_096;
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut result = std::collections::HashSet::new();
    let mut row = 0usize;
    let mut column = None;
    let mut cell_row = None;
    let mut kind = None::<String>;
    let mut value = BoundedSpreadsheetCellText::new();
    let mut collecting = false;
    let mut retained_cells = 0usize;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"row" => {
                row = event
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == b"r")
                    .and_then(|a| {
                        std::str::from_utf8(a.value.as_ref())
                            .ok()?
                            .parse::<usize>()
                            .ok()
                    })
                    .filter(|value| *value > 0)
                    .map(|value| value - 1)
                    .unwrap_or(row);
            }
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"c" => {
                kind = None;
                column = None;
                cell_row = Some(row);
                value = BoundedSpreadsheetCellText::new();
                for a in event.attributes().flatten() {
                    if a.key.as_ref() == b"t" {
                        kind = String::from_utf8(a.value.to_vec()).ok();
                    }
                    if a.key.as_ref() == b"r" {
                        let r = std::str::from_utf8(a.value.as_ref()).ok()?;
                        column = xlsx_column_index(r);
                        cell_row = xlsx_row_index(r).or(cell_row);
                    }
                }
            }
            Ok(quick_xml::events::Event::Start(event))
                if matches!(event.name().as_ref(), b"v" | b"t") =>
            {
                collecting = true
            }
            Ok(quick_xml::events::Event::End(event))
                if matches!(event.name().as_ref(), b"v" | b"t") =>
            {
                collecting = false
            }
            Ok(quick_xml::events::Event::Text(event)) if collecting => {
                value.append(&event.decode().ok()?, 1_024)?
            }
            Ok(quick_xml::events::Event::CData(event)) if collecting => {
                value.append(&event.decode().ok()?, 1_024)?
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"c" => {
                let retained = cell_row.is_some_and(|r| r < MAX_SPREADSHEET_ROWS)
                    && column.unwrap_or(0) < MAX_SPREADSHEET_COLUMNS
                    && retained_cells < MAX_CELLS;
                if retained && kind.as_deref() == Some("s") {
                    result.insert(value.text.parse::<usize>().ok()?);
                }
                if retained {
                    retained_cells += 1;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(result)
}

fn extract_xlsx_bounded_shared_strings(
    xml: &str,
    requested: &std::collections::HashSet<usize>,
    cell_limit: usize,
) -> Option<std::collections::HashMap<usize, BoundedSpreadsheetCellText>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut strings = std::collections::HashMap::new();
    let mut current = BoundedSpreadsheetCellText::new();
    let mut in_text = false;
    let mut index = 0usize;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"si" => {
                current = BoundedSpreadsheetCellText::new();
            }
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"t" => {
                in_text = true;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"t" => {
                in_text = false;
            }
            Ok(quick_xml::events::Event::End(event)) if event.name().as_ref() == b"si" => {
                if requested.contains(&index) {
                    strings.insert(
                        index,
                        std::mem::replace(&mut current, BoundedSpreadsheetCellText::new()),
                    );
                }
                index = index.checked_add(1)?;
                if index >= MAX_SHARED_STRING_SCAN {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Text(event)) if in_text => {
                current.append(&event.decode().ok()?, cell_limit)?;
            }
            Ok(quick_xml::events::Event::CData(event)) if in_text => {
                current.append(&event.decode().ok()?, cell_limit)?;
            }
            Ok(quick_xml::events::Event::GeneralRef(event)) if in_text => {
                let reference = event.decode().ok()?;
                current.append(
                    &quick_xml::escape::unescape(&format!("&{reference};")).ok()?,
                    cell_limit,
                )?;
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
    let mut row_number = None;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(event)) if event.name().as_ref() == b"row" => {
                row_cells.clear();
                row_number = event
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"r")
                    .and_then(|attribute| {
                        std::str::from_utf8(attribute.value.as_ref())
                            .ok()?
                            .parse::<usize>()
                            .ok()
                    })
                    .filter(|value| *value > 0)
                    .map(|value| value - 1);
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
                let target_row = row_number.unwrap_or(rows).min(255);
                while rows < target_row {
                    if !append_bounded_preview_text(&mut out, "\n") {
                        return bounded_sheet_text(&out);
                    }
                    rows += 1;
                }
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
                rows += 1;
                if !append_bounded_preview_text(&mut out, "\n")
                    || out.chars().count() >= EXTRACTED_TEXT_LIMIT
                    || rows >= 256
                {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    bounded_sheet_text(&out)
}

fn bounded_sheet_text(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.chars().take(EXTRACTED_TEXT_LIMIT).collect())
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
        spreadsheet: None,
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

    fn material_font_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ottrin-ui/assets/MaterialSymbolsFilled.ttf")
    }

    fn write_font_fixture(name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp_path(name);
        std::fs::write(&path, bytes).expect("write font fixture");
        path
    }

    #[test]
    fn affinity_extensions_use_a_dedicated_best_effort_preview_profile() {
        for name in ["cover.afphoto", "layout.AFDESIGN", "book.AfPuB"] {
            let path = temp_path(name);
            std::fs::write(&path, b"metadata-only fixture").expect("write Affinity fixture");
            let profile = preview_profile(&path);
            assert_eq!(classify_path(&path), PreviewKind::AffinityMetadata);
            assert!(profile.supports_view(PreviewView::Metadata));
            assert!(profile.supports_view(PreviewView::Visual));
            assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Affinity);
            assert!(!profile.raw_fallback_allowed());

            let preview = load_preview(&PreviewRequest {
                path: path.clone(),
                kind_hint: EntryKind::File,
            });
            assert_eq!(preview.kind, PreviewKind::AffinityMetadata);
            assert!(
                preview
                    .body
                    .contains("Best-effort embedded preview — may differ from the saved canvas")
            );
            let _ = std::fs::remove_file(path);
        }
    }

    fn write_sqlite_fixture(path: &Path) {
        let connection = rusqlite::Connection::open(path).expect("create SQLite fixture");
        connection
            .execute_batch(
                "CREATE TABLE projects (id INTEGER PRIMARY KEY, label TEXT);\
                 CREATE INDEX projects_label ON projects(label);\
                 CREATE VIEW project_labels AS SELECT label FROM projects;\
                 CREATE TRIGGER projects_audit AFTER INSERT ON projects BEGIN SELECT 1; END;",
            )
            .expect("create inert SQLite schema fixture");
    }

    #[test]
    fn sqlite_source_generation_changes_after_equal_size_hard_link_write_with_restored_mtime() {
        let source = temp_path("generation.sqlite");
        let alias = temp_path("generation-alias.sqlite");
        std::fs::write(&source, b"SQLite format 3\0first-generation")
            .expect("write first SQLite generation");
        let original_mtime = std::fs::metadata(&source)
            .expect("read original metadata")
            .modified()
            .expect("read original mtime");
        let first = sqlite_preview_source_generation(&source).expect("capture first generation");

        std::fs::hard_link(&source, &alias).expect("create hard-link alias");
        std::fs::write(&alias, b"SQLite format 3\0next--generation")
            .expect("replace bytes through hard-link alias");
        std::fs::File::open(&source)
            .expect("open source to restore mtime")
            .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .expect("restore source mtime");
        let second =
            sqlite_preview_source_generation(&source).expect("capture replacement generation");

        assert_ne!(
            first, second,
            "same-file identity, length, and restored mtime must not accept different SQLite bytes"
        );

        std::fs::remove_file(alias).expect("remove hard-link alias");
        std::fs::remove_file(source).expect("remove SQLite fixture");
    }

    #[test]
    fn sqlite_preview_is_content_bound_and_generates_a_deterministic_metadata_card() {
        let sqlite = temp_path("projects.SQLITE");
        write_sqlite_fixture(&sqlite);

        let profile = preview_profile(&sqlite);
        assert_eq!(classify_path(&sqlite), PreviewKind::SqliteMetadata);
        assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Sqlite);
        assert!(profile.supports_view(PreviewView::Metadata));
        assert!(profile.supports_view(PreviewView::Visual));
        assert!(!profile.raw_fallback_allowed());

        let preview = load_preview(&PreviewRequest {
            path: sqlite.clone(),
            kind_hint: EntryKind::File,
        });
        assert_eq!(preview.kind, PreviewKind::SqliteMetadata);
        assert!(preview.body.contains("Tables: 1"));
        assert!(preview.body.contains("Indexes: 1"));
        assert!(preview.body.contains("Views: 1"));
        assert!(preview.body.contains("Triggers: 1"));
        assert!(preview.body.contains("projects"));
        assert!(!preview.body.contains("label TEXT"));

        let first = render_sqlite_metadata_preview_png(&sqlite, 256).expect("SQLite card");
        let second = render_sqlite_metadata_preview_png(&sqlite, 256).expect("SQLite card");
        assert_eq!(first, second, "SQLite cards must be cacheable");
        let image = image::load_from_memory(&first).expect("decode SQLite card");
        assert!(image.width() <= 256 && image.height() <= 256);
        let expected_generation =
            sqlite_preview_source_generation(&sqlite).expect("capture source generation");
        let rendered = render_sqlite_metadata_preview_png_with_generation(&sqlite, 256)
            .expect("SQLite card with source generation");
        assert_eq!(rendered.source_generation, expected_generation);
        assert_eq!(rendered.png, first);

        let verified_db = temp_path("projects.Db");
        std::fs::copy(&sqlite, &verified_db).expect("copy content-verified .db");
        assert_eq!(classify_path(&verified_db), PreviewKind::SqliteMetadata);
        assert!(render_sqlite_metadata_preview_png(&verified_db, 128).is_ok());

        let mismatch = temp_path("projects.db");
        std::fs::write(&mismatch, b"not a SQLite database").expect("write extension mismatch");
        assert_eq!(classify_path(&mismatch), PreviewKind::Unsupported);
        assert!(matches!(
            render_sqlite_metadata_preview_png(&mismatch, 128),
            Err(SqliteVisualError::UnsupportedFormat)
        ));

        for path in [sqlite, verified_db, mismatch] {
            std::fs::remove_file(path).expect("remove SQLite fixture");
        }
    }

    #[test]
    fn sqlite_preview_rejects_truncated_wal_dependent_and_oversized_inputs() {
        let truncated = temp_path("truncated.sqlite3");
        std::fs::write(&truncated, b"SQLite format 3\0").expect("write truncated header");
        assert!(render_sqlite_metadata_preview_png(&truncated, 128).is_err());

        let wal_dependent = temp_path("wal-dependent.sqlite");
        let mut header = [0u8; 100];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[16..18].copy_from_slice(&4096u16.to_be_bytes());
        header[18] = 2;
        header[19] = 2;
        header[28..32].copy_from_slice(&1u32.to_be_bytes());
        std::fs::write(&wal_dependent, header).expect("write WAL-dependent header");
        assert!(render_sqlite_metadata_preview_png(&wal_dependent, 128).is_err());

        let oversized = temp_path("oversized.sqlite");
        let file = std::fs::File::create(&oversized).expect("create oversized SQLite fixture");
        file.set_len(MAX_SQLITE_PREVIEW_BYTES + 1)
            .expect("make sparse oversized SQLite fixture");
        assert!(matches!(
            render_sqlite_metadata_preview_png(&oversized, 128),
            Err(SqliteVisualError::InputTooLarge { .. })
        ));
        assert!(
            sqlite_preview_source_generation(&oversized).is_none(),
            "oversized sources must not receive a cache or async-response generation"
        );

        for path in [truncated, wal_dependent, oversized] {
            std::fs::remove_file(path).expect("remove SQLite fixture");
        }
    }

    #[test]
    fn sqlite_preview_rejects_incomplete_rollback_header_invariants() {
        let fixture = temp_path("header-invariants.sqlite");
        write_sqlite_fixture(&fixture);
        let source = std::fs::read(&fixture).expect("read SQLite fixture");

        for (label, offset, replacement) in [
            ("payload-fractions", 21, vec![0, 0, 0]),
            ("reserved-space", 20, vec![1]),
            ("reserved-header", 72, vec![1]),
            ("schema-format", 44, 0u32.to_be_bytes().to_vec()),
            ("text-encoding", 56, 0u32.to_be_bytes().to_vec()),
            ("physical-page-count", 28, 0u32.to_be_bytes().to_vec()),
        ] {
            let path = temp_path(&format!("header-{label}.sqlite"));
            let mut mutation = source.clone();
            mutation[offset..offset + replacement.len()].copy_from_slice(&replacement);
            std::fs::write(&path, mutation).expect("write mutated SQLite header");

            assert!(
                matches!(
                    render_sqlite_metadata_preview_png(&path, 128),
                    Err(SqliteVisualError::InvalidHeader(_))
                ),
                "{label} must be rejected before SQLite deserialize"
            );
            std::fs::remove_file(path).expect("remove mutated SQLite fixture");
        }

        std::fs::remove_file(fixture).expect("remove SQLite fixture");
    }

    #[test]
    fn sqlite_preview_rejects_schema_metadata_over_the_card_limit() {
        let sqlite = temp_path("too-many-schema-objects.sqlite");
        let connection = rusqlite::Connection::open(&sqlite).expect("create SQLite fixture");
        for index in 0..=MAX_SQLITE_SCHEMA_ROWS {
            connection
                .execute_batch(&format!("CREATE TABLE table_{index} (id INTEGER);"))
                .expect("create bounded schema object");
        }
        drop(connection);

        assert!(matches!(
            render_sqlite_metadata_preview_png(&sqlite, 128),
            Err(SqliteVisualError::SchemaBounds(_))
        ));
        std::fs::remove_file(sqlite).expect("remove SQLite fixture");
    }

    fn font_table_entry_offset(source: &[u8], tag: [u8; 4]) -> usize {
        let table_count = u16::from_be_bytes([source[4], source[5]]) as usize;
        (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| source[*offset..*offset + 4] == tag)
            .expect("font table entry")
    }

    fn raw_tiff_with_embedded_jpeg(jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const IFD1_OFFSET: u32 = IFD0_OFFSET + 2 + 4;
        const JPEG_OFFSET: u32 = IFD1_OFFSET + 2 + 2 * 12 + 4;

        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&IFD1_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0201u16, JPEG_OFFSET), (0x0202u16, jpeg.len() as u32)] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&4u16.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn raw_tiff_with_ifd_entries(mut entries: Vec<(u16, u16, u32, u32)>, jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const IFD1_OFFSET: u32 = IFD0_OFFSET + 2 + 4;
        let jpeg_offset = IFD1_OFFSET + 2 + (entries.len() as u32) * 12 + 4;
        for (tag, _, _, value) in &mut entries {
            match *tag {
                0x0201 => *value = jpeg_offset,
                0x0202 => *value = jpeg.len() as u32,
                _ => {}
            }
        }

        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&IFD1_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, field_type, count, value) in entries {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&field_type.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn big_endian_raw_tiff_with_embedded_jpeg(jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const IFD1_OFFSET: u32 = IFD0_OFFSET + 2 + 4;
        const JPEG_OFFSET: u32 = IFD1_OFFSET + 2 + 2 * 12 + 4;

        let mut bytes = b"MM\0*".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&IFD1_OFFSET.to_be_bytes());
        bytes.extend_from_slice(&2u16.to_be_bytes());
        for (tag, value) in [(0x0201u16, JPEG_OFFSET), (0x0202u16, jpeg.len() as u32)] {
            bytes.extend_from_slice(&tag.to_be_bytes());
            bytes.extend_from_slice(&4u16.to_be_bytes());
            bytes.extend_from_slice(&1u32.to_be_bytes());
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn raw_tiff_with_two_thumbnail_candidates(jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const IFD1_OFFSET: u32 = IFD0_OFFSET + 2 + 2 * 12 + 4;
        const JPEG_OFFSET: u32 = IFD1_OFFSET + 2 + 2 * 12 + 4;
        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
        for next_ifd in [IFD1_OFFSET, 0] {
            bytes.extend_from_slice(&2u16.to_le_bytes());
            for (tag, value) in [(0x0201u16, JPEG_OFFSET), (0x0202u16, jpeg.len() as u32)] {
                bytes.extend_from_slice(&tag.to_le_bytes());
                bytes.extend_from_slice(&4u16.to_le_bytes());
                bytes.extend_from_slice(&1u32.to_le_bytes());
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&next_ifd.to_le_bytes());
        }
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn raw_tiff_with_subifd_thumbnail(jpeg: &[u8]) -> Vec<u8> {
        const IFD0_OFFSET: u32 = 8;
        const SUB_IFD_OFFSET: u32 = IFD0_OFFSET + 2 + 12 + 4;
        const JPEG_OFFSET: u32 = SUB_IFD_OFFSET + 2 + 2 * 12 + 4;
        let mut bytes = b"II*\0".to_vec();
        bytes.extend_from_slice(&IFD0_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0x014au16.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&SUB_IFD_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0201u16, JPEG_OFFSET), (0x0202u16, jpeg.len() as u32)] {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&4u16.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(jpeg);
        bytes
    }

    fn small_jpeg() -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            12,
            8,
            image::Rgb([32, 160, 224]),
        ))
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .expect("encode embedded JPEG");
        jpeg
    }

    #[test]
    fn camera_raw_embedded_jpeg_is_a_bounded_visual_preview() {
        let path = temp_path("camera.DNG");
        std::fs::write(&path, raw_tiff_with_embedded_jpeg(&small_jpeg()))
            .expect("write TIFF-based RAW fixture");

        let profile = preview_profile(&path);
        assert_eq!(classify_path(&path), PreviewKind::CameraRawMetadata);
        assert!(profile.supports_view(PreviewView::Metadata));
        assert!(profile.supports_view(PreviewView::Visual));
        assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::CameraRaw);
        assert!(!profile.raw_fallback_allowed());

        let preview = load_preview(&PreviewRequest {
            path: path.clone(),
            kind_hint: EntryKind::File,
        });
        assert_eq!(preview.kind, PreviewKind::CameraRawMetadata);
        assert!(preview.body.contains("Camera RAW image"));

        let png = render_camera_raw_embedded_preview_png(&path, 128)
            .expect("embedded JPEG should become a safe PNG");
        let dimensions = image::load_from_memory(&png)
            .expect("decode output PNG")
            .dimensions();
        assert!(dimensions.0 <= 128 && dimensions.1 <= 128);
        assert!(dimensions.0 > 0 && dimensions.1 > 0);
        assert_ne!(png, std::fs::read(&path).expect("read raw fixture"));
        std::fs::remove_file(path).expect("remove RAW fixture");
    }

    #[test]
    fn camera_raw_rejects_extension_mismatch_and_invalid_embedded_ranges() {
        let mismatch = temp_path("mismatch.ARW");
        std::fs::write(&mismatch, small_jpeg()).expect("write non-RAW payload");
        assert!(render_camera_raw_embedded_preview_png(&mismatch, 128).is_err());
        std::fs::remove_file(mismatch).expect("remove mismatch fixture");

        let invalid = temp_path("overflow.dng");
        let mut fixture = raw_tiff_with_embedded_jpeg(&small_jpeg());
        let length_field = 14 + 2 + 12 + 8;
        fixture[length_field..length_field + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&invalid, fixture).expect("write invalid range fixture");
        assert!(render_camera_raw_embedded_preview_png(&invalid, 128).is_err());
        std::fs::remove_file(invalid).expect("remove invalid range fixture");

        let truncated = temp_path("truncated.dng");
        let mut fixture = raw_tiff_with_embedded_jpeg(&small_jpeg());
        fixture.truncate(fixture.len().saturating_sub(2));
        std::fs::write(&truncated, fixture).expect("write truncated RAW fixture");
        assert!(render_camera_raw_embedded_preview_png(&truncated, 128).is_err());
        std::fs::remove_file(truncated).expect("remove truncated RAW fixture");

        let oversized = temp_path("oversized.arw");
        let file = std::fs::File::create(&oversized).expect("create oversized RAW fixture");
        file.set_len(MAX_CAMERA_RAW_PACKAGE_BYTES + 1)
            .expect("make sparse oversized RAW fixture");
        assert!(matches!(
            render_camera_raw_embedded_preview_png(&oversized, 128),
            Err(CameraRawVisualError::InputTooLarge { .. })
        ));
        std::fs::remove_file(oversized).expect("remove oversized RAW fixture");
    }

    #[test]
    fn camera_raw_rejects_nonclassic_or_unbounded_tiff_thumbnail_layouts() {
        let jpeg = small_jpeg();
        let cases = [
            (
                "short-tags.dng",
                raw_tiff_with_ifd_entries(vec![(0x0201, 3, 1, 0), (0x0202, 3, 1, 0)], &jpeg),
            ),
            (
                "multi-long.dng",
                raw_tiff_with_ifd_entries(vec![(0x0201, 4, 2, 0), (0x0202, 4, 1, 0)], &jpeg),
            ),
            (
                "strip-confusion.dng",
                raw_tiff_with_ifd_entries(
                    vec![
                        (0x0201, 4, 1, 0),
                        (0x0202, 4, 1, 0),
                        (0x0111, 4, 1, 0),
                        (0x0117, 4, 1, 0),
                    ],
                    &jpeg,
                ),
            ),
            (
                "too-many-entries.dng",
                raw_tiff_with_ifd_entries(
                    std::iter::once((0x0201, 4, 1, 0))
                        .chain(std::iter::once((0x0202, 4, 1, 0)))
                        .chain((0..300).map(|index| (0xc000 + index, 4, 1, 1)))
                        .collect(),
                    &jpeg,
                ),
            ),
        ];

        for (name, fixture) in cases {
            let path = temp_path(name);
            std::fs::write(&path, fixture).expect("write malformed RAW fixture");
            assert!(
                render_camera_raw_embedded_preview_png(&path, 128).is_err(),
                "{name} must fail closed"
            );
            std::fs::remove_file(path).expect("remove malformed RAW fixture");
        }
    }

    #[test]
    fn camera_raw_accepts_big_endian_classic_tiff_and_rejects_conflicting_or_polyglot_data() {
        let jpeg = small_jpeg();
        let supported = temp_path("big-endian.arw");
        std::fs::write(&supported, big_endian_raw_tiff_with_embedded_jpeg(&jpeg))
            .expect("write big-endian RAW fixture");
        assert!(render_camera_raw_embedded_preview_png(&supported, 128).is_ok());
        std::fs::remove_file(supported).expect("remove big-endian RAW fixture");

        let duplicate = temp_path("duplicate-candidates.dng");
        std::fs::write(&duplicate, raw_tiff_with_two_thumbnail_candidates(&jpeg))
            .expect("write duplicate candidate fixture");
        assert!(render_camera_raw_embedded_preview_png(&duplicate, 128).is_err());
        std::fs::remove_file(duplicate).expect("remove duplicate candidate fixture");

        let polyglot = temp_path("polyglot.dng");
        let mut jpeg_with_trailer = jpeg;
        jpeg_with_trailer.extend_from_slice(b"trailing payload\xff\xd9");
        std::fs::write(&polyglot, raw_tiff_with_embedded_jpeg(&jpeg_with_trailer))
            .expect("write polyglot fixture");
        assert!(render_camera_raw_embedded_preview_png(&polyglot, 128).is_err());
        std::fs::remove_file(polyglot).expect("remove polyglot fixture");
    }

    #[test]
    fn camera_raw_rejects_subifd_only_and_metadata_overlapping_thumbnails() {
        let jpeg = small_jpeg();
        let subifd = temp_path("subifd-only.dng");
        std::fs::write(&subifd, raw_tiff_with_subifd_thumbnail(&jpeg))
            .expect("write SubIFD-only fixture");
        assert!(render_camera_raw_embedded_preview_png(&subifd, 128).is_err());
        std::fs::remove_file(subifd).expect("remove SubIFD-only fixture");

        let overlap = temp_path("metadata-overlap.dng");
        let mut fixture = raw_tiff_with_embedded_jpeg(&jpeg);
        let offset_field = 14 + 2 + 8;
        fixture[offset_field..offset_field + 4].copy_from_slice(&14u32.to_le_bytes());
        std::fs::write(&overlap, fixture).expect("write metadata-overlap fixture");
        assert!(render_camera_raw_embedded_preview_png(&overlap, 128).is_err());
        std::fs::remove_file(overlap).expect("remove metadata-overlap fixture");
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
    fn video_format_is_metadata_only_without_a_raw_fallback() {
        let path = Path::new("sample.MP4");
        let profile = preview_profile(path);

        assert_eq!(classify_path(path), PreviewKind::VideoMetadata);
        assert!(profile.supports_view(PreviewView::Metadata));
        assert!(!profile.supports_view(PreviewView::TextCard));
        assert!(!profile.supports_view(PreviewView::Visual));
        assert!(!profile.raw_fallback_allowed());
    }

    #[test]
    fn font_format_is_classified_as_metadata_with_dedicated_visual_renderer() {
        let path = material_font_path();

        assert_eq!(classify_path(&path), PreviewKind::FontMetadata);
        assert_eq!(
            preview_profile(&path).visual_renderer(),
            PreviewVisualRenderer::Font
        );
        assert!(preview_profile(&path).supports_view(PreviewView::Visual));
        assert!(!preview_profile(&path).raw_fallback_allowed());

        let preview = preview_font("MaterialSymbolsFilled.ttf".to_string(), &path);
        assert_eq!(preview.kind, PreviewKind::FontMetadata);
        assert!(preview.body.contains("OpenType/TrueType"));
        assert!(preview.body.contains("Preview: Bounded font sample"));
    }

    #[test]
    fn obj_format_is_reserved_for_a_dedicated_fail_closed_visual_renderer() {
        let path = temp_path("triangle.obj");
        std::fs::write(&path, "o bracket\nv 0 0 0\nv 1 0 0\nv 0 1 1\nf 1 2 3\n")
            .expect("write OBJ fixture");

        let profile = preview_profile(&path);
        assert_eq!(classify_path(&path), PreviewKind::ObjMetadata);
        assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Obj);
        assert!(profile.supports_view(PreviewView::Visual));
        assert!(!profile.raw_fallback_allowed());
        assert!(render_obj_preview_png(&path, 128).is_ok());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn font_preview_renderer_produces_deterministic_bounded_png_preview() {
        let path = material_font_path();

        let first = render_font_preview_png(&path, 256).expect("font visual render");
        let second = render_font_preview_png(&path, 256).expect("font visual render");
        assert_eq!(first, second);
        let image = image::load_from_memory(&first).expect("decode font png");

        assert!(image.dimensions().0 <= 256);
        assert!(image.dimensions().1 <= 256);
        assert!(
            image.to_rgba8().pixels().any(|pixel| pixel.0[0] < 250),
            "the deterministic missing-glyph fallback must remain a visible specimen"
        );
    }

    #[test]
    fn font_renderer_rejects_extension_mismatch_and_unsupported_collections() {
        let source = std::fs::read(material_font_path()).expect("read material font");
        let mismatch = temp_path("sample.txt");
        std::fs::write(&mismatch, &source).expect("write mismatch fixture");
        assert!(matches!(
            render_font_preview_png(&mismatch, 256),
            Err(FontVisualError::UnsupportedFormat)
        ));
        std::fs::remove_file(mismatch).expect("remove mismatch fixture");

        let collection = temp_path("collection.ttf");
        let mut payload = b"ttcf".to_vec();
        payload.extend_from_slice(&[0; 32]);
        std::fs::write(&collection, payload).expect("write TTC fixture");
        assert!(matches!(
            render_font_preview_png(&collection, 256),
            Err(FontVisualError::UnsupportedCollection)
        ));
        std::fs::remove_file(collection).expect("remove collection fixture");
    }

    #[test]
    fn font_renderer_rejects_oversized_or_malformed_font_inputs() {
        let malformed = temp_path("malformed.otf");
        std::fs::write(&malformed, b"\x00\x01\x02\x03not-font").expect("write malformed fixture");
        assert!(matches!(
            render_font_preview_png(&malformed, 256),
            Err(FontVisualError::ExtensionMismatch)
        ));
        std::fs::remove_file(malformed).expect("remove malformed fixture");

        let oversized = temp_path("oversized.ttf");
        let oversized_file = std::fs::File::create(&oversized).expect("create oversized fixture");
        oversized_file
            .set_len(MAX_FONT_PREVIEW_PACKAGE_BYTES + 1)
            .expect("oversize fixture");
        assert!(matches!(
            render_font_preview_png(&oversized, 256),
            Err(FontVisualError::InputTooLarge { .. })
        ));
        std::fs::remove_file(oversized).expect("remove oversized fixture");
    }

    #[test]
    fn font_renderer_rejects_container_mismatches_and_unsafe_table_directories() {
        let source = std::fs::read(material_font_path()).expect("read material font");

        let extension_mismatch = write_font_fixture("font-mismatch.otf", &source);
        assert!(matches!(
            render_font_preview_png(&extension_mismatch, 256),
            Err(FontVisualError::ExtensionMismatch)
        ));
        std::fs::remove_file(extension_mismatch).expect("remove extension mismatch");

        let truncated = write_font_fixture("font-truncated.ttf", &source[..15]);
        assert!(matches!(
            render_font_preview_png(&truncated, 256),
            Err(FontVisualError::InvalidTableDirectory(_))
        ));
        std::fs::remove_file(truncated).expect("remove truncated fixture");

        let mut overflow = source.clone();
        let entry = font_table_entry_offset(&overflow, *b"cmap");
        overflow[entry + 8..entry + 12].copy_from_slice(&u32::MAX.to_be_bytes());
        let overflow = write_font_fixture("font-offset-overflow.ttf", &overflow);
        assert!(matches!(
            render_font_preview_png(&overflow, 256),
            Err(FontVisualError::InvalidTableDirectory(_))
        ));
        std::fs::remove_file(overflow).expect("remove overflow fixture");

        let mut duplicate = source.clone();
        let first = 12;
        let second = 28;
        let first_tag = duplicate[first..first + 4].to_vec();
        duplicate[second..second + 4].copy_from_slice(&first_tag);
        let duplicate = write_font_fixture("font-duplicate-tag.ttf", &duplicate);
        assert!(matches!(
            render_font_preview_png(&duplicate, 256),
            Err(FontVisualError::InvalidTableDirectory(_))
        ));
        std::fs::remove_file(duplicate).expect("remove duplicate fixture");

        let mut overlap = source.clone();
        let first_offset = font_table_entry_offset(&overlap, *b"cmap");
        let second_offset = font_table_entry_offset(&overlap, *b"head");
        let first_table_offset = overlap[first_offset + 8..first_offset + 12].to_vec();
        overlap[second_offset + 8..second_offset + 12].copy_from_slice(&first_table_offset);
        let overlap = write_font_fixture("font-overlap.ttf", &overlap);
        assert!(matches!(
            render_font_preview_png(&overlap, 256),
            Err(FontVisualError::InvalidTableDirectory(_))
        ));
        std::fs::remove_file(overlap).expect("remove overlap fixture");
    }

    #[test]
    fn font_renderer_accepts_true_type_sfnt_true_magic() {
        let mut source = std::fs::read(material_font_path()).expect("read material font");
        source[..4].copy_from_slice(b"true");
        let fixture = write_font_fixture("font-true-magic.ttf", &source);

        assert!(render_font_preview_png(&fixture, 256).is_ok());

        std::fs::remove_file(fixture).expect("remove true-magic fixture");
    }

    #[test]
    fn font_renderer_rejects_color_bitmap_and_svg_font_surfaces() {
        for tag in [
            *b"SVG ", *b"CBDT", *b"CBLC", *b"sbix", *b"COLR", *b"CPAL", *b"EBDT", *b"EBLC",
            *b"bdat", *b"bloc",
        ] {
            let mut source = std::fs::read(material_font_path()).expect("read material font");
            source[12..16].copy_from_slice(&tag);
            let fixture = write_font_fixture("font-unsupported-surface.ttf", &source);

            assert!(matches!(
                render_font_preview_png(&fixture, 256),
                Err(FontVisualError::UnsupportedSurface { tag: actual }) if actual == tag
            ));

            std::fs::remove_file(fixture).expect("remove surface fixture");
        }
    }

    #[test]
    fn font_identity_safely_falls_back_when_name_encoding_is_unsupported() {
        let mut source = std::fs::read(material_font_path()).expect("read material font");
        let name_entry = font_table_entry_offset(&source, *b"name");
        let name_table_offset = u32::from_be_bytes(
            source[name_entry + 8..name_entry + 12]
                .try_into()
                .expect("name table offset"),
        ) as usize;
        // Windows encoding ID 10 is a valid name record encoding that
        // ttf-parser 0.25.1 intentionally does not decode. Apply it to all
        // records so no supported duplicate can mask the fallback.
        let record_count =
            u16::from_be_bytes([source[name_table_offset + 2], source[name_table_offset + 3]])
                as usize;
        for record in 0..record_count {
            let encoding = name_table_offset + 6 + record * 12 + 2;
            source[encoding..encoding + 2].copy_from_slice(&10u16.to_be_bytes());
        }
        let fixture = write_font_fixture("font-unsupported-name-encoding.ttf", &source);

        let preview = preview_font("unsupported-name-encoding.ttf".to_string(), &fixture);
        assert!(!preview.body.contains("Family:"));
        assert!(preview.body.contains("Preview: Bounded font sample"));
        assert_eq!(
            font_specimen(None),
            "Aa Bb Cc 123\nFont preview",
            "unsupported valid name encodings use the bounded generic identity fallback"
        );

        std::fs::remove_file(fixture).expect("remove unsupported-name fixture");
    }

    #[test]
    fn cff_card_is_identity_only_and_does_not_render_a_bundled_font_specimen() {
        let first = render_cff_font_card(None, 256).expect("render CFF identity card");
        let second = render_cff_font_card(None, 256).expect("render CFF identity card");
        assert_eq!(first, second);
        assert!(image::load_from_memory(&first).is_ok());
    }

    #[test]
    fn font_renderer_rejects_unbounded_name_records_before_parsing() {
        let mut source = std::fs::read(material_font_path()).expect("read material font");
        let name_entry = font_table_entry_offset(&source, *b"name");
        let name_table_offset = u32::from_be_bytes(
            source[name_entry + 8..name_entry + 12]
                .try_into()
                .expect("name table offset"),
        ) as usize;
        source[name_table_offset + 14..name_table_offset + 16]
            .copy_from_slice(&(MAX_FONT_NAME_BYTES as u16 + 1).to_be_bytes());
        let fixture = write_font_fixture("font-unbounded-name.ttf", &source);

        assert!(matches!(
            render_font_preview_png(&fixture, 256),
            Err(FontVisualError::InvalidTableDirectory(_))
        ));

        std::fs::remove_file(fixture).expect("remove name fixture");
    }

    #[test]
    fn font_rasterizer_refuses_outline_work_outside_its_surface_before_drawing() {
        let source = std::fs::read(material_font_path()).expect("read material font");
        let font = ttf_parser::Face::parse(&source, 0).expect("parse material font");
        let mut image = image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]));

        assert!(matches!(
            render_text_with_font(&mut image, &font, "A", 96, 8, 120, 56),
            Err(FontVisualError::UnsafeOutlineWork(_))
        ));
    }

    #[test]
    fn font_preflight_rejects_composite_glyphs_before_outline_decoding() {
        assert!(matches!(
            preflight_simple_ttf_glyph(&[0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(FontVisualError::UnsafeOutlineWork(_))
        ));
    }

    #[test]
    fn font_preview_is_bound_to_metadata_and_profile_for_load_preview() {
        let path = material_font_path();

        let preview = load_preview(&PreviewRequest {
            path: path.clone(),
            kind_hint: EntryKind::File,
        });

        assert_eq!(preview.kind, PreviewKind::FontMetadata);
        assert!(preview.body.contains("Preview: Bounded font sample"));
        assert!(preview.body.contains("Filetype: OpenType/TrueType font"));
        assert!(preview.body.contains("Family: Material Symbols"));
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
    fn svg_admission_accepts_a_bounded_checkerboard_larger_than_the_legacy_byte_cliff() {
        let path = temp_path("bounded-checkerboard.svg");
        let rectangles = (0..5_001)
            .map(|index| {
                let x = index % 101;
                let y = index / 101;
                format!("<rect x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\"/>")
            })
            .collect::<String>();
        let svg = format!(
            "<svg width=\"1000\" height=\"1000\" xmlns=\"http://www.w3.org/2000/svg\">{rectangles}<!--{}--></svg>",
            " ".repeat(64 * 1024),
        );
        assert!(
            svg.len() > 256 * 1024,
            "fixture must cross the prior arbitrary source-byte cutoff"
        );
        std::fs::write(&path, svg).expect("write checkerboard fixture");

        let bytes = render_svg_content_preview_png(&path, 128)
            .expect("bounded self-contained checkerboard should render");
        assert_eq!(
            image::load_from_memory(&bytes)
                .expect("decode checkerboard preview")
                .dimensions(),
            (128, 128)
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn svg_admission_rejects_semantic_render_amplifiers() {
        let cases = [
            (
                "use-expansion.svg",
                r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><path id="p" d="M0 0h1v1z"/></defs><use href="#p"/></svg>"##.to_string(),
                "reuse",
            ),
            (
                "css-render-work.svg",
                r##"<svg xmlns="http://www.w3.org/2000/svg"><style>rect { filter: blur(2px); }</style><rect width="1" height="1"/></svg>"##.to_string(),
                "CSS",
            ),
            (
                "filter-amplification.svg",
                format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><filter id="f"><feDropShadow stdDeviation="1"/></filter></defs>{}</svg>"##,
                    r##"<rect width="1" height="1" filter="url(#f)"/>"##.repeat(129),
                ),
                "filter applications",
            ),
            (
                "marker-amplification.svg",
                r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><marker id="m"><circle r="1"/></marker></defs><path d="M0 0h1h1h1" marker-mid="url(#m)"/></svg>"##.to_string(),
                "marker",
            ),
        ];

        for (name, svg, expected_reason) in cases {
            let path = temp_path(name);
            std::fs::write(&path, svg).expect("write semantic-amplifier fixture");
            let error = render_svg_content_preview_png(&path, 64)
                .expect_err("semantic render amplification must fail closed")
                .to_string();
            assert!(
                error.contains(expected_reason),
                "expected {expected_reason:?} in {error:?}"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn eps_has_an_actionable_safe_renderer_unavailable_state() {
        let path = temp_path("diagram.eps");
        std::fs::write(&path, b"%!PS-Adobe-3.0\nshowpage\n").expect("write EPS fixture");

        let profile = preview_profile(&path);
        assert!(!profile.raw_fallback_allowed());
        let preview = load_preview(&PreviewRequest {
            path: path.clone(),
            kind_hint: EntryKind::File,
        });
        assert!(preview.body.contains("safe isolated EPS renderer"));
        assert!(preview.open_external);

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
        assert!(is_complete_text_preview(&preview));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn only_complete_text_previews_are_editable_candidates() {
        let complete = temp_path("complete.txt");
        std::fs::write(&complete, "complete\n").expect("write complete text");
        let complete_preview = preview_text("complete.txt".to_string(), &complete);
        assert!(is_complete_text_preview(&complete_preview));

        let truncated = temp_path("truncated.txt");
        std::fs::write(&truncated, vec![b'x'; 128 * 1024 + 1]).expect("write large text");
        let truncated_preview = preview_text("truncated.txt".to_string(), &truncated);
        assert_eq!(truncated_preview.kind, PreviewKind::Text);
        assert!(!is_complete_text_preview(&truncated_preview));

        let derived_preview = PreviewData {
            title: "report.pdf".to_string(),
            kind: PreviewKind::Pdf,
            body: "Extracted text is read-only".to_string(),
            spreadsheet: None,
            open_external: true,
        };
        assert!(!is_complete_text_preview(&derived_preview));

        let _ = std::fs::remove_file(complete);
        let _ = std::fs::remove_file(truncated);
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
    fn epub_preview_reports_canonical_title_and_author_without_content_execution() {
        let path = temp_path("guide.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            epub.start_file("mimetype", stored).expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write EPUB mimetype");
            let options = zip::write::SimpleFileOptions::default();
            epub.start_file("META-INF/container.xml", options)
                .expect("write container");
            epub.write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
                .expect("write container XML");
            epub.start_file("OEBPS/content.opf", options)
                .expect("write package");
            epub.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Ottrin Guide</dc:title><dc:creator>David</dc:creator></metadata></package>"#)
                .expect("write package XML");
            epub.start_file("OEBPS/chapter.xhtml", options)
                .expect("write untrusted content");
            epub.write_all(b"<script>untrusted-content</script>")
                .expect("write chapter");
            epub.finish().expect("finish EPUB");
        }

        assert_eq!(classify_path(&path), PreviewKind::ArchiveMetadata);
        let preview = preview_archive("guide.epub".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(preview.body.contains("Filetype: EPUB ebook"));
        assert!(preview.body.contains("Title: Ottrin Guide"));
        assert!(preview.body.contains("Author: David"));
        assert!(!preview.body.contains("untrusted-content"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn epub_xml_requires_canonical_namespaces_and_structure() {
        let container = r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let package = r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Ottrin Guide</dc:title><dc:creator>David</dc:creator></metadata></package>"#;

        assert_eq!(
            epub_package_path(container),
            Some("OEBPS/content.opf".to_string())
        );
        assert_eq!(
            extract_epub_metadata(package),
            Some((Some("Ottrin Guide".to_string()), Some("David".to_string())))
        );

        for invalid in [
            r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            r#"<container xmlns="https://example.invalid/container" version="1.0"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            r#"<!DOCTYPE container [<!ENTITY x "OEBPS/content.opf">]><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="&x;" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></container>"#,
            r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/one.opf" media-type="application/oebps-package+xml"/><rootfile full-path="OEBPS/two.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        ] {
            assert_eq!(epub_package_path(invalid), None, "must reject {invalid}");
        }

        for invalid in [
            r#"<package><metadata><title>Unqualified</title></metadata></package>"#,
            r#"<package xmlns="https://example.invalid/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Wrong package namespace</dc:title></metadata></package>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><manifest><metadata><dc:title>Wrong location</dc:title></metadata></manifest></package>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:foreign="https://example.invalid/opf"><metadata><dc:title>Canonical</dc:title></metadata><foreign:metadata><dc:title>Competing namespace</dc:title></foreign:metadata></package>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Canonical</dc:title></metadata><manifest><metadata><dc:title>Competing depth</dc:title></metadata></manifest></package>"#,
            r#"<!DOCTYPE package [<!ENTITY x "title">]><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:&x;>Entity</dc:&x;></metadata></package>"#,
        ] {
            assert_eq!(
                extract_epub_metadata(invalid),
                None,
                "must reject {invalid}"
            );
        }
    }

    #[test]
    fn epub_zip_preflight_rejects_unbounded_central_directory_counts() {
        let path = temp_path("epub-central-directory-count.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            epub.start_file("mimetype", zip::write::SimpleFileOptions::default())
                .expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.finish().expect("finish EPUB");
        }

        let bytes = std::fs::read(&path).expect("read valid EPUB");
        assert_eq!(
            preflight_epub_zip(&bytes).map(|directory| directory.entry_count),
            Some(1)
        );

        let original = std::fs::read(&path).expect("read EPUB");
        let eocd = original
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find EOCD");
        for (label, mutation) in [
            (
                "entry count",
                Box::new(|bytes: &mut Vec<u8>| {
                    bytes[eocd + 8..eocd + 10].copy_from_slice(&u16::MAX.to_le_bytes());
                    bytes[eocd + 10..eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());
                }) as Box<dyn Fn(&mut Vec<u8>)>,
            ),
            (
                "directory byte length",
                Box::new(|bytes: &mut Vec<u8>| {
                    bytes[eocd + 12..eocd + 16].copy_from_slice(
                        &((MAX_EPUB_CENTRAL_DIRECTORY_BYTES + 1) as u32).to_le_bytes(),
                    );
                }),
            ),
            (
                "directory offset",
                Box::new(|bytes: &mut Vec<u8>| {
                    let offset = u32::from_le_bytes(
                        bytes[eocd + 16..eocd + 20]
                            .try_into()
                            .expect("offset bytes"),
                    );
                    bytes[eocd + 16..eocd + 20]
                        .copy_from_slice(&offset.saturating_add(1).to_le_bytes());
                }),
            ),
        ] {
            let mut bytes = original.clone();
            mutation(&mut bytes);
            std::fs::write(&path, bytes).expect("rewrite malformed EPUB");

            let bytes = std::fs::read(&path).expect("read malformed EPUB");
            assert!(
                preflight_epub_zip(&bytes).is_none(),
                "the central-directory {label} must be bounded before ZipArchive allocation"
            );
        }

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_zip_preflight_rejects_unnecessary_zip64_directory() {
        let path = temp_path("epub-zip64-directory.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            epub.start_file(
                "mimetype",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.finish().expect("finish EPUB");
        }

        let original = std::fs::read(&path).expect("read EPUB");
        let eocd = original
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find EOCD");
        let entries = read_le_u16(&original[eocd + 10..eocd + 12]).expect("entry count");
        let directory_size = read_le_u32(&original[eocd + 12..eocd + 16]).expect("directory size");
        let directory_offset =
            read_le_u32(&original[eocd + 16..eocd + 20]).expect("directory offset");

        let mut zip64 = original[..eocd].to_vec();
        let zip64_eocd_offset = zip64.len() as u64;
        zip64.extend_from_slice(b"PK\x06\x06");
        zip64.extend_from_slice(&44_u64.to_le_bytes());
        zip64.extend_from_slice(&45_u16.to_le_bytes());
        zip64.extend_from_slice(&45_u16.to_le_bytes());
        zip64.extend_from_slice(&0_u32.to_le_bytes());
        zip64.extend_from_slice(&0_u32.to_le_bytes());
        zip64.extend_from_slice(&(entries as u64).to_le_bytes());
        zip64.extend_from_slice(&(entries as u64).to_le_bytes());
        zip64.extend_from_slice(&(directory_size as u64).to_le_bytes());
        zip64.extend_from_slice(&(directory_offset as u64).to_le_bytes());
        zip64.extend_from_slice(b"PK\x06\x07");
        zip64.extend_from_slice(&0_u32.to_le_bytes());
        zip64.extend_from_slice(&zip64_eocd_offset.to_le_bytes());
        zip64.extend_from_slice(&1_u32.to_le_bytes());
        let mut zip64_eocd = original[eocd..].to_vec();
        zip64_eocd[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64_eocd[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64_eocd[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        zip64_eocd[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        zip64.extend_from_slice(&zip64_eocd);
        std::fs::write(&path, zip64).expect("write ZIP64 EPUB");

        let bytes = std::fs::read(&path).expect("read ZIP64 EPUB");
        assert!(
            preflight_epub_zip(&bytes).is_none(),
            "bounded EPUB previews do not accept ZIP64 archive metadata"
        );

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_signatureless_descriptor_crc_equal_to_marker_is_not_misread_as_signed() {
        const MARKER: u32 = 0x0807_4b50;
        let mut local = vec![0; 30];
        local[..4].copy_from_slice(b"PK\x03\x04");
        local[6..8].copy_from_slice(&0x0008u16.to_le_bytes());
        local[14..18].copy_from_slice(&0u32.to_le_bytes());
        local[18..22].copy_from_slice(&0u32.to_le_bytes());
        local[22..26].copy_from_slice(&0u32.to_le_bytes());
        local[26..28].copy_from_slice(&1u16.to_le_bytes());
        local.extend_from_slice(b"x");
        local.extend_from_slice(&[0xac, 0x0a, 0x7a, 0xd5]);
        local.extend_from_slice(&MARKER.to_le_bytes());
        local.extend_from_slice(&4u32.to_le_bytes());
        local.extend_from_slice(&4u32.to_le_bytes());

        let mut central = vec![0; 46];
        central[8..10].copy_from_slice(&0x0008u16.to_le_bytes());
        central[16..20].copy_from_slice(&MARKER.to_le_bytes());
        central[20..24].copy_from_slice(&4u32.to_le_bytes());
        central[24..28].copy_from_slice(&4u32.to_le_bytes());

        assert_eq!(
            epub_local_entry_region(&local, 0, local.len() as u64, &central, b"x"),
            Some((0, local.len() as u64))
        );
    }

    #[test]
    fn epub_zip_preflight_rejects_ambiguous_footers_and_zip64_conflicts() {
        let path = temp_path("epub-ambiguous-footer.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            epub.start_file(
                "mimetype",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.finish().expect("finish EPUB");
        }

        let original = std::fs::read(&path).expect("read EPUB");
        let eocd = original
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find EOCD");
        let directory_offset =
            read_le_u32(&original[eocd + 16..eocd + 20]).expect("directory offset") as usize;
        let mut duplicate_directory = original.clone();
        let duplicate_offset = duplicate_directory.len() as u32;
        duplicate_directory.extend_from_slice(&original[directory_offset..eocd]);
        let mut duplicate_footer = original[eocd..].to_vec();
        duplicate_footer[16..20].copy_from_slice(&duplicate_offset.to_le_bytes());
        duplicate_directory.extend_from_slice(&duplicate_footer);
        std::fs::write(&path, &duplicate_directory).expect("write ambiguous EPUB");
        assert!(
            preflight_epub_zip(&duplicate_directory).is_none(),
            "a second central-directory/footer pair must not replace an earlier archive footer"
        );

        let mut trailing_bytes = original.clone();
        trailing_bytes.extend_from_slice(b"unexpected trailing data");
        assert!(
            preflight_epub_zip(&trailing_bytes).is_none(),
            "a footer must terminate the immutable EPUB snapshot"
        );

        let mut multi_disk = original.clone();
        multi_disk[eocd + 4..eocd + 6].copy_from_slice(&1_u16.to_le_bytes());
        assert!(
            preflight_epub_zip(&multi_disk).is_none(),
            "EPUB must reject multi-disk ZIP metadata before archive allocation"
        );

        let mut zip64_conflict = original[..eocd].to_vec();
        let zip64_eocd_offset = zip64_conflict.len() as u64;
        let entries = read_le_u16(&original[eocd + 10..eocd + 12]).expect("entry count");
        let directory_size = read_le_u32(&original[eocd + 12..eocd + 16]).expect("directory size");
        let directory_offset =
            read_le_u32(&original[eocd + 16..eocd + 20]).expect("directory offset");
        zip64_conflict.extend_from_slice(b"PK\x06\x06");
        zip64_conflict.extend_from_slice(&44_u64.to_le_bytes());
        zip64_conflict.extend_from_slice(&45_u16.to_le_bytes());
        zip64_conflict.extend_from_slice(&45_u16.to_le_bytes());
        zip64_conflict.extend_from_slice(&0_u32.to_le_bytes());
        zip64_conflict.extend_from_slice(&0_u32.to_le_bytes());
        zip64_conflict.extend_from_slice(&(entries as u64).to_le_bytes());
        zip64_conflict.extend_from_slice(&(entries as u64).to_le_bytes());
        zip64_conflict.extend_from_slice(&(directory_size as u64).to_le_bytes());
        zip64_conflict.extend_from_slice(&(directory_offset as u64).to_le_bytes());
        zip64_conflict.extend_from_slice(b"PK\x06\x07");
        zip64_conflict.extend_from_slice(&0_u32.to_le_bytes());
        zip64_conflict.extend_from_slice(&zip64_eocd_offset.to_le_bytes());
        zip64_conflict.extend_from_slice(&1_u32.to_le_bytes());
        let mut zip64_footer = original[eocd..].to_vec();
        zip64_footer[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64_footer[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
        zip64_footer[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        // This deliberately leaves the classic directory offset non-sentinel.
        zip64_conflict.extend_from_slice(&zip64_footer);
        assert!(
            preflight_epub_zip(&zip64_conflict).is_none(),
            "ZIP64 records must not conflict with a partially populated classic footer"
        );

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_preview_rejects_a_competing_directory_outside_the_eocd_tail() {
        let path = temp_path("epub-distant-competing-directory.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            epub.start_file(
                "mimetype",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.finish().expect("finish EPUB");
        }

        let original = std::fs::read(&path).expect("read EPUB");
        let original_eocd = original
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find EOCD");
        let directory_offset = read_le_u32(&original[original_eocd + 16..original_eocd + 20])
            .expect("directory offset") as usize;
        let mut ambiguous = original.clone();
        // Put a complete first archive far enough away that looking only in the
        // final EOCD comment window cannot see its footer.
        ambiguous.extend_from_slice(&vec![0_u8; 65_558]);
        let competing_directory_offset = ambiguous.len() as u32;
        ambiguous.extend_from_slice(&original[directory_offset..original_eocd]);
        let mut competing_footer = original[original_eocd..].to_vec();
        competing_footer[16..20].copy_from_slice(&competing_directory_offset.to_le_bytes());
        ambiguous.extend_from_slice(&competing_footer);
        std::fs::write(&path, &ambiguous).expect("write ambiguous EPUB");

        assert!(
            preflight_epub_zip(&ambiguous).is_none(),
            "a structurally valid directory/footer pair outside the EOCD tail must be rejected"
        );
        let preview = preview_archive("epub-distant-competing-directory.epub".to_string(), &path);
        assert!(
            preview
                .body
                .contains("Could not read EPUB metadata safely."),
            "the public EPUB preview must fail closed on an ambiguous archive"
        );

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_zip_preflight_rejects_multidisk_central_directory_entries() {
        let path = temp_path("epub-central-directory-disk-start.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            epub.start_file(
                "mimetype",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.finish().expect("finish EPUB");
        }

        let original = std::fs::read(&path).expect("read EPUB");
        let eocd = original
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find EOCD");
        let central =
            read_le_u32(&original[eocd + 16..eocd + 20]).expect("directory offset") as usize;
        let name_len =
            read_le_u16(&original[central + 28..central + 30]).expect("name length") as usize;
        let extra_start = central + 46 + name_len;

        let mut nonzero_disk = original.clone();
        nonzero_disk[central + 34..central + 36].copy_from_slice(&1_u16.to_le_bytes());
        assert!(
            preflight_epub_zip(&nonzero_disk).is_none(),
            "a nonzero central-directory disk start conflicts with single-disk EPUB"
        );

        let mut local_zip64 = original.clone();
        let local_name_len = read_le_u16(&local_zip64[26..28]).expect("local name length") as usize;
        let local_extra_start = 30 + local_name_len;
        let compressed_size = read_le_u32(&local_zip64[18..22]).expect("local compressed size");
        let uncompressed_size = read_le_u32(&local_zip64[22..26]).expect("local uncompressed size");
        let mut local_extra = vec![1, 0, 16, 0];
        local_extra.extend_from_slice(&(uncompressed_size as u64).to_le_bytes());
        local_extra.extend_from_slice(&(compressed_size as u64).to_le_bytes());
        local_zip64[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        local_zip64[22..26].copy_from_slice(&u32::MAX.to_le_bytes());
        local_zip64[28..30].copy_from_slice(&(local_extra.len() as u16).to_le_bytes());
        local_zip64.splice(local_extra_start..local_extra_start, local_extra);
        let shifted_eocd = eocd + 20;
        let shifted_directory_offset =
            read_le_u32(&local_zip64[shifted_eocd + 16..shifted_eocd + 20])
                .expect("shifted directory offset");
        local_zip64[shifted_eocd + 16..shifted_eocd + 20]
            .copy_from_slice(&(shifted_directory_offset + 20).to_le_bytes());
        assert!(
            preflight_epub_zip(&local_zip64).is_none(),
            "bounded EPUB previews reject ZIP64 local-header metadata"
        );

        let mut overlapping_data = original.clone();
        let local_data_start = 30
            + read_le_u16(&original[26..28]).expect("local name length") as usize
            + read_le_u16(&original[28..30]).expect("local extra length") as usize;
        let overlapping_size = central
            .checked_sub(local_data_start)
            .and_then(|size| size.checked_add(1))
            .expect("overlapping compressed size") as u32;
        overlapping_data[central + 20..central + 24]
            .copy_from_slice(&overlapping_size.to_le_bytes());
        assert!(
            preflight_epub_zip(&overlapping_data).is_none(),
            "local entry data must not overlap the central directory"
        );

        let duplicate_path = temp_path("epub-duplicate-local-reference.epub");
        {
            let file = std::fs::File::create(&duplicate_path).expect("create duplicate fixture");
            let mut epub = zip::ZipWriter::new(file);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            epub.start_file("mimetype", stored).expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write mimetype bytes");
            epub.start_file("second", stored)
                .expect("write second entry");
            epub.write_all(b"second").expect("write second bytes");
            epub.finish().expect("finish duplicate fixture");
        }
        let mut duplicate_local = std::fs::read(&duplicate_path).expect("read duplicate fixture");
        let duplicate_eocd = duplicate_local
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("find duplicate EOCD");
        let first_central = read_le_u32(&duplicate_local[duplicate_eocd + 16..duplicate_eocd + 20])
            .expect("first central offset") as usize;
        let first_record_len = 46
            + read_le_u16(&duplicate_local[first_central + 28..first_central + 30])
                .expect("first name length") as usize
            + read_le_u16(&duplicate_local[first_central + 30..first_central + 32])
                .expect("first extra length") as usize
            + read_le_u16(&duplicate_local[first_central + 32..first_central + 34])
                .expect("first comment length") as usize;
        let second_central = first_central + first_record_len;
        let first_local_offset = duplicate_local[first_central + 42..first_central + 46].to_vec();
        duplicate_local[second_central + 42..second_central + 46]
            .copy_from_slice(&first_local_offset);
        assert!(
            preflight_epub_zip(&duplicate_local).is_none(),
            "central records must not share a local header"
        );
        std::fs::remove_file(duplicate_path).expect("remove duplicate fixture");

        let mut valid_zip64_disk_start = original.clone();
        let valid_extra = vec![1, 0, 4, 0, 0, 0, 0, 0];
        valid_zip64_disk_start[central + 34..central + 36].copy_from_slice(&u16::MAX.to_le_bytes());
        valid_zip64_disk_start[central + 30..central + 32]
            .copy_from_slice(&(valid_extra.len() as u16).to_le_bytes());
        valid_zip64_disk_start.splice(extra_start..extra_start, valid_extra);
        let valid_eocd = eocd + 8;
        let directory_size = read_le_u32(&valid_zip64_disk_start[valid_eocd + 12..valid_eocd + 16])
            .expect("directory size");
        valid_zip64_disk_start[valid_eocd + 12..valid_eocd + 16]
            .copy_from_slice(&(directory_size + 8).to_le_bytes());
        assert!(
            preflight_epub_zip(&valid_zip64_disk_start).is_none(),
            "bounded EPUB previews reject unnecessary ZIP64 entry metadata"
        );

        // ZIP64 values may only replace classic sentinel fields. A disk-start
        // value is therefore forbidden when the classic disk-start field is
        // already zero, whether it appears alone or after a required value.
        for (label, sentinel_uncompressed_size, zip64_disk_start) in [
            ("unneeded zero disk start alone", false, 0_u32),
            ("unneeded nonzero disk start alone", false, 1_u32),
            ("unneeded zero disk start after size", true, 0_u32),
            ("unneeded nonzero disk start after size", true, 1_u32),
        ] {
            let mut bytes = original.clone();
            let mut zip64_values = Vec::new();
            if sentinel_uncompressed_size {
                let size = read_le_u32(&original[central + 24..central + 28])
                    .expect("classic uncompressed size");
                bytes[central + 24..central + 28].copy_from_slice(&u32::MAX.to_le_bytes());
                zip64_values.extend_from_slice(&(size as u64).to_le_bytes());
            }
            zip64_values.extend_from_slice(&zip64_disk_start.to_le_bytes());
            let mut zip64_extra = Vec::with_capacity(4 + zip64_values.len());
            zip64_extra.extend_from_slice(&1_u16.to_le_bytes());
            zip64_extra.extend_from_slice(&(zip64_values.len() as u16).to_le_bytes());
            zip64_extra.extend_from_slice(&zip64_values);
            bytes[central + 30..central + 32]
                .copy_from_slice(&(zip64_extra.len() as u16).to_le_bytes());
            bytes.splice(extra_start..extra_start, zip64_extra);
            let mutated_eocd = eocd
                + read_le_u16(&bytes[central + 30..central + 32]).expect("extra length") as usize;
            let directory_size =
                read_le_u32(&bytes[mutated_eocd + 12..mutated_eocd + 16]).expect("directory size");
            bytes[mutated_eocd + 12..mutated_eocd + 16]
                .copy_from_slice(&(directory_size + (mutated_eocd - eocd) as u32).to_le_bytes());

            assert!(
                preflight_epub_zip(&bytes).is_none(),
                "a {label} must fail closed"
            );
            std::fs::write(&path, &bytes).expect("write malformed EPUB");
            assert!(
                preview_archive(format!("{label}.epub"), &path)
                    .body
                    .contains("Could not read EPUB metadata safely."),
                "the public EPUB preview must fail closed on a {label}"
            );
        }

        for (label, sentinel_uncompressed_size, sentinel_compressed_size, sentinel_local_offset) in [
            ("uncompressed size", true, false, false),
            ("compressed size", false, true, false),
            ("local-header offset", false, false, true),
            ("uncompressed and compressed sizes", true, true, false),
            (
                "uncompressed size and local-header offset",
                true,
                false,
                true,
            ),
            ("compressed size and local-header offset", false, true, true),
            ("all preceding central-directory fields", true, true, true),
        ] {
            let mut bytes = original.clone();
            let mut zip64_values = Vec::new();
            for (range, is_sentinel) in [
                (24..28, sentinel_uncompressed_size),
                (20..24, sentinel_compressed_size),
                (42..46, sentinel_local_offset),
            ] {
                let value = read_le_u32(&original[central + range.start..central + range.end])
                    .expect("classic central-directory value");
                if is_sentinel {
                    bytes[central + range.start..central + range.end]
                        .copy_from_slice(&u32::MAX.to_le_bytes());
                    zip64_values.extend_from_slice(&(value as u64).to_le_bytes());
                }
            }
            zip64_values.extend_from_slice(&0_u32.to_le_bytes());
            let mut zip64_extra = Vec::with_capacity(4 + zip64_values.len());
            zip64_extra.extend_from_slice(&1_u16.to_le_bytes());
            zip64_extra.extend_from_slice(&(zip64_values.len() as u16).to_le_bytes());
            zip64_extra.extend_from_slice(&zip64_values);
            bytes[central + 34..central + 36].copy_from_slice(&u16::MAX.to_le_bytes());
            bytes[central + 30..central + 32]
                .copy_from_slice(&(zip64_extra.len() as u16).to_le_bytes());
            bytes.splice(extra_start..extra_start, zip64_extra);
            let mutated_eocd = eocd
                + read_le_u16(&bytes[central + 30..central + 32]).expect("extra length") as usize;
            let directory_size =
                read_le_u32(&bytes[mutated_eocd + 12..mutated_eocd + 16]).expect("directory size");
            bytes[mutated_eocd + 12..mutated_eocd + 16]
                .copy_from_slice(&(directory_size + (mutated_eocd - eocd) as u32).to_le_bytes());
            assert!(
                preflight_epub_zip(&bytes).is_none(),
                "bounded EPUB previews reject ZIP64 {label} sentinels"
            );
        }

        for (label, zip64_extra) in [
            ("missing", Vec::new()),
            ("malformed", vec![1, 0, 3, 0, 0, 0, 0]),
            ("nonzero", vec![1, 0, 4, 0, 1, 0, 0, 0]),
            (
                "duplicate",
                vec![1, 0, 4, 0, 0, 0, 0, 0, 1, 0, 4, 0, 0, 0, 0, 0],
            ),
        ] {
            let mut bytes = original.clone();
            bytes[central + 34..central + 36].copy_from_slice(&u16::MAX.to_le_bytes());
            bytes[central + 30..central + 32]
                .copy_from_slice(&(zip64_extra.len() as u16).to_le_bytes());
            bytes.splice(extra_start..extra_start, zip64_extra);
            let mutated_eocd = eocd
                + read_le_u16(&bytes[central + 30..central + 32]).expect("extra length") as usize;
            let directory_size =
                read_le_u32(&bytes[mutated_eocd + 12..mutated_eocd + 16]).expect("directory size");
            bytes[mutated_eocd + 12..mutated_eocd + 16]
                .copy_from_slice(&(directory_size + (mutated_eocd - eocd) as u32).to_le_bytes());
            assert!(
                preflight_epub_zip(&bytes).is_none(),
                "a {label} ZIP64 disk-start value must fail closed"
            );
        }

        std::fs::remove_file(path).expect("remove EPUB fixture");
    }

    #[test]
    fn epub_preview_fails_closed_on_malformed_or_external_package_metadata() {
        for (name, container) in [
            ("malformed-container.epub", b"<container>".as_slice()),
            (
                "external-rootfile.epub",
                br#"<container><rootfiles><rootfile full-path="https://example.invalid/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            ),
        ] {
            let path = temp_path(name);
            {
                let file = std::fs::File::create(&path).expect("create EPUB");
                let mut epub = zip::ZipWriter::new(file);
                let stored = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                epub.start_file("mimetype", stored).expect("write mimetype");
                epub.write_all(b"application/epub+zip")
                    .expect("write EPUB mimetype");
                epub.start_file(
                    "META-INF/container.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("write container");
                epub.write_all(container).expect("write container XML");
                epub.finish().expect("finish EPUB");
            }

            let preview = preview_archive(name.to_string(), &path);

            assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
            assert!(preview.body.contains("Could not read EPUB metadata safely."));

            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn epub_preview_rejects_a_mimetype_local_header_that_is_not_first() {
        let path = temp_path("mimetype-local-header-not-first.epub");
        {
            let file = std::fs::File::create(&path).expect("create EPUB");
            let mut epub = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            epub.start_file("before-mimetype", options)
                .expect("write preceding entry");
            epub.write_all(b"not an EPUB mimetype")
                .expect("write preceding entry data");
            let stored = options.compression_method(zip::CompressionMethod::Stored);
            epub.start_file("mimetype", stored).expect("write mimetype");
            epub.write_all(b"application/epub+zip")
                .expect("write EPUB mimetype");
            epub.start_file("META-INF/container.xml", options)
                .expect("write container");
            epub.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
                .expect("write container XML");
            epub.start_file("OEBPS/content.opf", options)
                .expect("write package");
            epub.write_all(
                br#"<package><metadata><title>Not accepted</title></metadata></package>"#,
            )
            .expect("write package XML");
            epub.finish().expect("finish EPUB");
        }

        // Move the `mimetype` central-directory record before the preceding
        // entry without changing either local-header offset. ZIP readers then
        // report it as index zero even though it is not the first physical
        // file, which EPUB forbids.
        let mut bytes = std::fs::read(&path).expect("read EPUB");
        let central_start = bytes
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .expect("find central directory");
        let central_record_len = |start: usize| {
            46 + u16::from_le_bytes([bytes[start + 28], bytes[start + 29]]) as usize
                + u16::from_le_bytes([bytes[start + 30], bytes[start + 31]]) as usize
                + u16::from_le_bytes([bytes[start + 32], bytes[start + 33]]) as usize
        };
        let first_end = central_start + central_record_len(central_start);
        let second_end = first_end + central_record_len(first_end);
        let mut reordered = bytes[first_end..second_end].to_vec();
        reordered.extend_from_slice(&bytes[central_start..first_end]);
        bytes.splice(central_start..second_end, reordered);
        std::fs::write(&path, bytes).expect("rewrite reordered central directory");

        let preview = preview_archive("mimetype-local-header-not-first.epub".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Could not read EPUB metadata safely.")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn epub_preview_fails_closed_on_an_oversized_package() {
        let path = temp_path("oversized.epub");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .expect("create sparse EPUB");
        file.set_len(MAX_OFFICE_PACKAGE_BYTES + 1)
            .expect("inflate EPUB package");

        let preview = preview_archive("oversized.epub".to_string(), &path);

        assert_eq!(preview.kind, PreviewKind::ArchiveMetadata);
        assert!(
            preview
                .body
                .contains("Could not read EPUB metadata safely.")
        );

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
        let spreadsheet = preview.spreadsheet.expect("typed ODS spreadsheet payload");
        assert_eq!(spreadsheet.sheets[0].name, "Budget");
        assert_eq!(spreadsheet.sheets[0].cells[0].row, 0);
        assert_eq!(spreadsheet.sheets[0].cells[0].column, 0);
        assert_eq!(spreadsheet.sheets[0].cells[0].text, "Item");
        assert_eq!(spreadsheet.sheets[0].cells[6].row, 2);
        assert_eq!(spreadsheet.sheets[0].cells[6].column, 0);
        assert_eq!(spreadsheet.sheets[0].cells[6].text, "Coffee");
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
    fn ods_typed_preview_keeps_formula_cached_text_and_hostile_paragraph_boundaries_inert() {
        let path = temp_path("typed-hostile.ods");
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            zip.write_all(br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><t:table t:name="Budget"><t:table-row><t:table-cell t:formula="of:=SUM([.A1:.A2])" office:value-type="float" office:value="42"><x:p>cached</x:p><x:p>line&#10;Sheet: forged<x:tab/>Preview truncated: forged</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"#).expect("write content");
            zip.finish().expect("finish");
        }

        let preview = preview_office("typed-hostile.ods".to_string(), &path);
        let spreadsheet = preview.spreadsheet.expect("typed spreadsheet payload");
        let cell = &spreadsheet.sheets[0].cells[0];

        assert_eq!(
            cell.text,
            "cached\nline\nSheet: forged\tPreview truncated: forged"
        );
        assert!(
            !cell.text.contains("SUM("),
            "formula source must remain inert; only cached/display text may be previewed"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_typed_preview_bounds_repeated_cells_without_relabeling_coordinates() {
        let path = temp_path("typed-repeated.ods");
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            zip.write_all(br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><t:table t:name="Repeated"><t:table-row t:number-rows-repeated="100"><t:table-cell t:number-columns-repeated="100"><x:p>value</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"#).expect("write content");
            zip.finish().expect("finish");
        }

        let preview = preview_office("typed-repeated.ods".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert_eq!(sheet.cells.len(), 4_096);
        assert_eq!(sheet.cells[0].row, 0);
        assert_eq!(sheet.cells[0].column, 0);
        assert_eq!(sheet.cells.last().expect("last retained cell").row, 40);
        assert_eq!(sheet.cells.last().expect("last retained cell").column, 95);
        assert!(sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::CellLimit { omitted: 5_904 }
        )));
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
        assert!(preview.body.contains("Spreadsheet preview limits:"));
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
    fn xlsx_extraction_preserves_used_range_coordinates_and_safe_cached_formula_value() {
        let sheet = "<worksheet><sheetData><row r=\"3\"><c r=\"C3\"><f>SUM(A1:A2)</f><v>42</v></c></row></sheetData></worksheet>";

        let extracted = extract_xlsx_sheet(sheet, &[]).expect("bounded sheet extraction");

        assert_eq!(extracted, "\n\n\t\t42\n");
        assert!(
            !extracted.contains("SUM(A1:A2)"),
            "formula source must remain inert; only its cached value is previewed"
        );
    }

    #[test]
    fn xlsx_preview_keeps_sparse_coordinates_and_hostile_cell_text_typed() {
        let path = temp_path("sparse-hostile.xlsx");
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/workbook.xml", options)
                .expect("workbook");
            zip.write_all(br#"<workbook xmlns:r="r"><sheets><sheet name="First" r:id="rId1"/><sheet name="Second" r:id="rId2"/></sheets></workbook>"#).expect("write workbook");
            zip.start_file("xl/_rels/workbook.xml.rels", options)
                .expect("relationships");
            zip.write_all(br#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#).expect("write relationships");
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("first sheet");
            zip.write_all(br#"<worksheet><sheetData><row r="100000"><c r="XFD100000" t="inlineStr"><is><t>line one&#10;Sheet: forged&#9;Preview truncated: forged</t></is></c></row></sheetData></worksheet>"#).expect("write first sheet");
            zip.start_file("xl/worksheets/sheet2.xml", options)
                .expect("second sheet");
            zip.write_all(br#"<worksheet><sheetData><row r="2"><c r="B2"><f>SUM(A1:A2)</f><v>42</v></c></row></sheetData></worksheet>"#).expect("write second sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("sparse-hostile.xlsx".to_string(), &path);
        let spreadsheet = preview.spreadsheet.expect("typed spreadsheet payload");

        assert_eq!(spreadsheet.sheets.len(), 2);
        assert_eq!(spreadsheet.sheets[0].name, "First");
        assert_eq!(spreadsheet.sheets[0].cells[0].row, 99_999);
        assert_eq!(spreadsheet.sheets[0].cells[0].column, 16_383);
        assert_eq!(
            spreadsheet.sheets[0].cells[0].text,
            "line one\nSheet: forged\tPreview truncated: forged"
        );
        assert_eq!(spreadsheet.sheets[1].cells[0].row, 1);
        assert_eq!(spreadsheet.sheets[1].cells[0].column, 1);
        assert_eq!(spreadsheet.sheets[1].cells[0].text, "42");
        assert!(!spreadsheet.sheets[1].cells[0].text.contains("SUM("));
        assert!(
            spreadsheet.sheets[0]
                .omissions
                .iter()
                .any(|omission| matches!(
                    omission.reason,
                    SpreadsheetOmissionReason::SparseRowGap {
                        from: 0,
                        to: 99_998
                    }
                ))
        );
        assert!(
            spreadsheet.sheets[0]
                .omissions
                .iter()
                .any(|omission| matches!(
                    omission.reason,
                    SpreadsheetOmissionReason::SparseColumnGap {
                        from: 0,
                        to: 16_382
                    }
                ))
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xlsx_typed_preview_discloses_the_exact_cell_limit_remainder() {
        let path = temp_path("xlsx-cell-limit.xlsx");
        let cells = (1..=4_098)
            .map(|row| format!("<row r=\"{row}\"><c r=\"A{row}\"><v>{row}</v></c></row>"))
            .collect::<String>();
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            write!(zip, "<worksheet><sheetData>{cells}</sheetData></worksheet>")
                .expect("write sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("xlsx-cell-limit.xlsx".to_string(), &path);
        let spreadsheet = preview.spreadsheet.expect("typed spreadsheet payload");

        assert_eq!(spreadsheet.sheets[0].cells.len(), 4_096);
        assert!(
            spreadsheet.sheets[0]
                .omissions
                .iter()
                .any(|omission| matches!(
                    omission.reason,
                    SpreadsheetOmissionReason::CellLimit { omitted: 2 }
                ))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xlsx_typed_preview_advances_implicit_rows_into_the_row_limit() {
        let path = temp_path("xlsx-implicit-row-limit.xlsx");
        let rows = "<row/>".repeat(MAX_SPREADSHEET_ROWS) + "<row><c><v>out of range</v></c></row>";
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            write!(zip, "<worksheet><sheetData>{rows}</sheetData></worksheet>")
                .expect("write sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("xlsx-implicit-row-limit.xlsx".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert!(sheet.cells.is_empty());
        assert!(sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::RowLimit {
                from: MAX_SPREADSHEET_ROWS,
                to: MAX_SPREADSHEET_ROWS
            }
        )));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xlsx_typed_preview_bounds_chunked_shared_inline_and_cached_cell_text() {
        const CELL_CHARACTERS: usize = 1_032;
        let path = temp_path("xlsx-chunked-cell-text.xlsx");
        let oversized = "x".repeat(CELL_CHARACTERS);
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/sharedStrings.xml", options)
                .expect("shared strings");
            write!(zip, "<sst><si><t><![CDATA[{oversized}]]></t></si></sst>")
                .expect("write shared string");
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            write!(
                zip,
                "<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\" t=\"inlineStr\"><is><t><![CDATA[{oversized}]]></t></is></c><c r=\"C1\"><v><![CDATA[{oversized}]]></v></c></row></sheetData></worksheet>"
            )
            .expect("write sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("xlsx-chunked-cell-text.xlsx".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert_eq!(sheet.cells.len(), 3);
        assert!(
            sheet
                .cells
                .iter()
                .all(|cell| cell.text.chars().count() == 1_024)
        );
        assert_eq!(
            sheet
                .omissions
                .iter()
                .filter_map(|omission| match omission.reason {
                    SpreadsheetOmissionReason::TextLimit { omitted } => Some(omitted),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![CELL_CHARACTERS - 1_024; 3]
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xlsx_typed_preview_resolves_referenced_shared_strings_past_the_old_vector_cap() {
        let path = temp_path("xlsx-referenced-shared-string.xlsx");
        let strings = (0..=4_096)
            .map(|index| format!("<si><t>shared-{index}</t></si>"))
            .collect::<String>();
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/sharedStrings.xml", options)
                .expect("shared strings");
            write!(zip, "<sst>{strings}</sst>").expect("write shared strings");
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            zip.write_all(
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>4096</v></c></row></sheetData></worksheet>"#,
            )
            .expect("write sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("xlsx-referenced-shared-string.xlsx".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert_eq!(sheet.cells[0].text, "shared-4096");
        assert!(sheet.omissions.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn xlsx_typed_preview_discloses_shared_string_beyond_the_scan_limit_without_rendering_index() {
        let path = temp_path("xlsx-shared-string-scan-limit.xlsx");
        let strings = "<si><t>filler</t></si>".repeat(8_192) + "<si><t>unavailable-text</t></si>";
        {
            let file = std::fs::File::create(&path).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/sharedStrings.xml", options)
                .expect("shared strings");
            write!(zip, "<sst>{strings}</sst>").expect("write shared strings");
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            zip.write_all(
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>8192</v></c></row></sheetData></worksheet>"#,
            )
            .expect("write sheet");
            zip.finish().expect("finish workbook");
        }

        let preview = preview_office("xlsx-shared-string-scan-limit.xlsx".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert!(
            sheet.cells.is_empty(),
            "the index must never be rendered as cell text"
        );
        assert!(sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::SharedStringUnavailable { index: 8_192 }
        )));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_xlsx_and_ods_previews_disclose_coordinate_ranges_without_clamping() {
        let xlsx = temp_path("xlsx-coordinate-limit.xlsx");
        {
            let file = std::fs::File::create(&xlsx).expect("create workbook");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("xl/worksheets/sheet1.xml", options)
                .expect("sheet");
            zip.write_all(
                br#"<worksheet><sheetData><row r="100000"><c r="XFD100000"><v>accepted</v></c></row><row r="100001"><c r="XFE100001"><v>out of range</v></c></row></sheetData></worksheet>"#,
            )
            .expect("write sheet");
            zip.finish().expect("finish workbook");
        }
        let xlsx_preview = preview_office("xlsx-coordinate-limit.xlsx".to_string(), &xlsx);
        let xlsx_sheet = &xlsx_preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];
        assert_eq!(xlsx_sheet.cells.len(), 1);
        assert_eq!(xlsx_sheet.cells[0].row, 99_999);
        assert_eq!(xlsx_sheet.cells[0].column, 16_383);
        assert!(xlsx_sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::RowLimit {
                from: 100_000,
                to: 100_000
            }
        )));
        assert!(xlsx_sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::ColumnLimit {
                from: 16_384,
                to: 16_384
            }
        )));
        let _ = std::fs::remove_file(xlsx);

        let ods = temp_path("ods-coordinate-limit.ods");
        {
            let file = std::fs::File::create(&ods).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            zip.write_all(br#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:t="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:x="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><t:table t:name="Bounds"><t:table-row t:number-rows-repeated="100001"><t:table-cell t:number-columns-repeated="16385"><x:p>value</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"#).expect("write content");
            zip.finish().expect("finish");
        }
        let ods_preview = preview_office("ods-coordinate-limit.ods".to_string(), &ods);
        let ods_sheet = &ods_preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];
        assert!(ods_sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::RowLimit {
                from: 100_000,
                to: 100_000
            }
        )));
        assert!(ods_sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::ColumnLimit {
                from: 16_384,
                to: 16_384
            }
        )));
        assert!(
            ods_sheet
                .cells
                .iter()
                .all(|cell| cell.row < 100_000 && cell.column < 16_384)
        );
        let _ = std::fs::remove_file(ods);
    }

    #[test]
    fn ods_typed_preview_multiplies_text_omission_for_repeated_cells_and_rows() {
        const CELL_CHARACTERS: usize = 1_032;
        let path = temp_path("ods-repeated-text-limit.ods");
        let oversized = "x".repeat(CELL_CHARACTERS);
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            write!(
                zip,
                "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Repeated\"><t:table-row t:number-rows-repeated=\"2\"><t:table-cell t:number-columns-repeated=\"3\"><x:p>{oversized}</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>"
            )
            .expect("write content");
            zip.finish().expect("finish");
        }

        let preview = preview_office("ods-repeated-text-limit.ods".to_string(), &path);
        let sheet = &preview
            .spreadsheet
            .expect("typed spreadsheet payload")
            .sheets[0];

        assert!(
            sheet
                .cells
                .iter()
                .all(|cell| cell.text.chars().count() == 1_024)
        );
        assert!(sheet.omissions.iter().any(|omission| matches!(
            omission.reason,
            SpreadsheetOmissionReason::TextLimit { omitted: 48 }
        )));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn ods_typed_preview_fails_closed_when_repeated_text_omission_overflows() {
        let path = temp_path("ods-repeated-text-overflow.ods");
        let oversized = "x".repeat(1_025);
        {
            let file = std::fs::File::create(&path).expect("create ods");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("content.xml", options).expect("content");
            write!(
                zip,
                "<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:t=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"><office:body><office:spreadsheet><t:table t:name=\"Overflow\"><t:table-row t:number-rows-repeated=\"{}\"><t:table-cell t:number-columns-repeated=\"2\"><x:p>{oversized}</x:p></t:table-cell></t:table-row></t:table></office:spreadsheet></office:body></office:document-content>",
                usize::MAX
            )
            .expect("write content");
            zip.finish().expect("finish");
        }

        let preview = preview_office("ods-repeated-text-overflow.ods".to_string(), &path);

        assert!(preview.spreadsheet.is_none());
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
    fn pptx_visual_preview_is_bounded_png_not_package_bytes() {
        let path = temp_path("slides.PPTX");
        {
            let file = std::fs::File::create(&path).expect("create pptx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("ppt/slides/slide1.xml", options)
                .expect("slide");
            zip.write_all(
                br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>First slide title</a:t><a:t>Bounded preview text</a:t></p:sld>"#,
            )
            .expect("write slide");
            zip.finish().expect("finish");
        }

        let png = render_office_content_preview_png(&path, 256).expect("bounded visual PNG");
        let decoded = image::load_from_memory(&png).expect("decode visual PNG");
        assert_eq!(decoded.dimensions(), (256, 256));
        assert!(
            decoded
                .pixels()
                .any(|(_, _, pixel)| pixel.0 != [255, 255, 255, 255]),
            "PPTX preview must rasterize slide text, not an empty page"
        );
        assert_ne!(png, std::fs::read(&path).expect("read package"));
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn pptx_visual_preview_rejects_a_symlink_before_opening_the_package() {
        use std::os::unix::fs::symlink;

        let target = temp_path("pptx-symlink-target.pptx");
        let link = temp_path("pptx-symlink.pptx");
        std::fs::write(&target, b"not a package").expect("write target");
        symlink(&target, &link).expect("create symlink");

        assert!(matches!(
            render_office_content_preview_png(&link, 256),
            Err(OfficeVisualError::InputNotRegular)
        ));
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_file(target);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_keeps_document_cards_but_rejects_pptx_before_path_access() {
        let pptx = temp_path("non-unix-unopened.pptx");
        assert!(matches!(
            render_office_content_preview_png(&pptx, 128),
            Err(OfficeVisualError::PreviewDisabledOnPlatform)
        ));

        let document = temp_path("non-unix-document.docx");
        {
            let file = std::fs::File::create(&document).expect("create document");
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .expect("document entry");
            zip.write_all(b"<w:document><w:t>Visible document card</w:t></w:document>")
                .expect("document XML");
            zip.finish().expect("finish document");
        }
        assert!(
            render_office_content_preview_png(&document, 128).is_ok(),
            "DOCX keeps its established bounded renderer on non-Unix platforms"
        );
        let _ = std::fs::remove_file(document);
    }

    #[test]
    fn office_source_generation_changes_for_an_in_place_same_length_document_rewrite() {
        let path = temp_path("office-generation.docx");
        std::fs::write(&path, b"first-package-generation").expect("write first generation");
        let first = office_preview_source_generation(&path).expect("hash first generation");
        std::fs::write(&path, b"other-package-generation").expect("rewrite same length");
        let second = office_preview_source_generation(&path).expect("hash second generation");
        assert_ne!(first, second);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn office_snapshot_generation_changes_when_a_document_path_is_replaced() {
        let path = temp_path("office-snapshot-replacement.docx");
        let replacement = temp_path("office-snapshot-replacement-next.docx");
        std::fs::write(&path, b"first-office-package").expect("write first package");
        let first = read_office_snapshot(&path).expect("snapshot first package");
        std::fs::write(&replacement, b"next--office-package").expect("write replacement");
        std::fs::rename(&replacement, &path).expect("replace package path");
        let next = read_office_snapshot(&path).expect("snapshot replacement package");

        assert_ne!(first.generation, next.generation);
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn office_document_snapshot_never_returns_an_unstable_same_inode_generation() {
        use std::io::{Seek, SeekFrom};
        use std::os::unix::fs::MetadataExt;
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicBool, Ordering},
        };

        let path = temp_path("office-snapshot-same-inode.docx");
        let first = vec![b'A'; 128 * 1024];
        let second = vec![b'B'; 128 * 1024];
        std::fs::write(&path, &first).expect("write first package");
        let inode = std::fs::metadata(&path).expect("first metadata").ino();
        let first_generation = OfficePreviewSourceGeneration(Sha256::digest(&first).into());
        let second_generation = OfficePreviewSourceGeneration(Sha256::digest(&second).into());
        let start = Arc::new(Barrier::new(2));
        let running = Arc::new(AtomicBool::new(true));
        let writer_path = path.clone();
        let writer_start = Arc::clone(&start);
        let writer_running = Arc::clone(&running);
        let writer = std::thread::spawn(move || {
            writer_start.wait();
            for generation in [&first, &second].into_iter().cycle().take(128) {
                if !writer_running.load(Ordering::Acquire) {
                    break;
                }
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&writer_path)
                    .expect("open same inode package");
                file.seek(SeekFrom::Start(0)).expect("rewind package");
                file.write_all(generation)
                    .expect("rewrite same inode package");
            }
        });

        start.wait();
        for _ in 0..128 {
            match read_office_snapshot(&path) {
                Ok(snapshot) => assert!(
                    snapshot.generation == first_generation
                        || snapshot.generation == second_generation,
                    "the admitted snapshot must be one complete writer generation"
                ),
                Err(OfficeVisualError::InputChanged) => {}
                Err(error) => panic!(
                    "same-inode rewrite must either yield a stable snapshot or fail closed: {error}"
                ),
            }
        }
        running.store(false, Ordering::Release);
        writer.join().expect("writer joins");
        assert_eq!(
            std::fs::metadata(&path).expect("final metadata").ino(),
            inode,
            "the concurrent rewrites must retain the same inode"
        );
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn pptx_worker_snapshot_is_stable_during_same_inode_active_writes() {
        use std::io::{Seek, SeekFrom};
        use std::os::unix::fs::MetadataExt;
        use std::sync::{
            Arc, Barrier,
            atomic::{AtomicBool, Ordering},
        };

        fn valid_pptx_bytes(text: &str) -> Vec<u8> {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut bytes);
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                zip.start_file("ppt/slides/slide1.xml", options)
                    .expect("slide entry");
                write!(
                    zip,
                    "<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:t>{text}</a:t></p:sld>"
                )
                .expect("slide text");
                zip.finish().expect("finish package");
            }
            bytes.into_inner()
        }

        let path = temp_path("pptx-active-writer.pptx");
        let first = valid_pptx_bytes("Alpha stable slide");
        let second = valid_pptx_bytes("Bravo stable slide");
        assert_eq!(first.len(), second.len(), "fixtures must have equal length");
        std::fs::write(&path, &first).expect("write first fixture");
        let inode = std::fs::metadata(&path).expect("first metadata").ino();
        let first_generation = OfficePreviewSourceGeneration(Sha256::digest(&first).into());
        let second_generation = OfficePreviewSourceGeneration(Sha256::digest(&second).into());
        let start = Arc::new(Barrier::new(2));
        let running = Arc::new(AtomicBool::new(true));
        let writer_path = path.clone();
        let writer_start = Arc::clone(&start);
        let writer_running = Arc::clone(&running);
        let writer = std::thread::spawn(move || {
            writer_start.wait();
            for generation in [&first, &second].into_iter().cycle().take(256) {
                if !writer_running.load(Ordering::Acquire) {
                    break;
                }
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&writer_path)
                    .expect("open same inode package");
                file.seek(SeekFrom::Start(0)).expect("rewind package");
                file.write_all(generation).expect("rewrite package");
            }
        });

        start.wait();
        for _ in 0..128 {
            match render_office_content_preview_png_with_generation(&path, 128) {
                Ok(rendered) => assert!(
                    rendered.source_generation == first_generation
                        || rendered.source_generation == second_generation,
                    "a rendered PPTX must carry the SHA of its admitted A or B snapshot"
                ),
                Err(OfficeVisualError::InputChanged) => {}
                Err(error) => panic!("active writer must yield A, B, or InputChanged; got {error}"),
            }
        }
        running.store(false, Ordering::Release);
        writer.join().expect("writer joins");
        assert_eq!(
            std::fs::metadata(&path).expect("final metadata").ino(),
            inode,
            "the active writer must retain the inode"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pptx_slide_text_requires_truthful_namespaces() {
        let valid = r#"<slide:sld xmlns:slide="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:text="http://schemas.openxmlformats.org/drawingml/2006/main"><text:t>Visible</text:t></slide:sld>"#;
        assert_eq!(extract_pptx_slide_text(valid), Some("Visible".to_string()));
        for invalid in [
            r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="https://evil.invalid"><a:t>Hidden</a:t></p:sld>"#,
            r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><a:t>Unbound</a:t></p:sld>"#,
            r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" a:x="1" a:x="2"><a:t>Duplicate</a:t></p:sld>"#,
        ] {
            assert_eq!(extract_pptx_slide_text(invalid), None);
        }
    }

    #[test]
    fn office_zip_names_reject_drive_and_cross_platform_escape_forms() {
        for unsafe_name in [
            "C:/ppt/evil.xml",
            "C:\\\\ppt\\\\evil.xml",
            "/ppt/slides/slide1.xml",
            "\\\\server\\share\\slide.xml",
            "ppt/../slides/slide1.xml",
            "ppt\\slides\\slide1.xml",
        ] {
            assert!(
                !safe_office_zip_name(unsafe_name),
                "must reject {unsafe_name}"
            );
        }
        assert!(safe_office_zip_name("ppt/slides/slide1.xml"));
    }

    #[test]
    fn pptx_visual_preview_uses_the_lowest_canonical_slide_number() {
        let path = temp_path("out-of-order.pptx");
        {
            let file = std::fs::File::create(&path).expect("create pptx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for (name, text) in [
                ("ppt/slides/slide2.xml", "Second slide"),
                ("ppt/slides/slide1.xml", "First slide"),
            ] {
                zip.start_file(name, options).expect("slide");
                write!(zip, "<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:t>{text}</a:t></p:sld>").expect("write slide");
            }
            zip.finish().expect("finish");
        }

        let file = std::fs::File::open(&path).expect("open pptx");
        let mut archive = zip::ZipArchive::new(file).expect("open package");
        assert_eq!(
            extract_pptx_visual_text(&mut archive),
            Some("First slide".to_string())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pptx_visual_preview_fails_closed_for_unsafe_or_noncanonical_packages() {
        let cases = [
            (
                "empty-first-slide",
                vec![
                    ("ppt/slides/slide1.xml", "<p:sld><a:t/></p:sld>"),
                    (
                        "ppt/slides/slide2.xml",
                        "<p:sld><a:t>Later slide</a:t></p:sld>",
                    ),
                ],
            ),
            (
                "malformed-slide",
                vec![("ppt/slides/slide1.xml", "<p:sld><a:t>Unclosed</p:sld>")],
            ),
            (
                "colliding-slide-identity",
                vec![
                    ("ppt/slides/slide1.xml", "<p:sld><a:t>One</a:t></p:sld>"),
                    ("ppt/slides/slide01.xml", "<p:sld><a:t>Alias</a:t></p:sld>"),
                ],
            ),
            (
                "traversal-entry",
                vec![
                    ("ppt/slides/slide1.xml", "<p:sld><a:t>Safe</a:t></p:sld>"),
                    (
                        "../ppt/slides/slide2.xml",
                        "<p:sld><a:t>Unsafe</a:t></p:sld>",
                    ),
                ],
            ),
            (
                "media-entry",
                vec![
                    ("ppt/slides/slide1.xml", "<p:sld><a:t>Safe</a:t></p:sld>"),
                    ("ppt/media/video.bin", "not rendered"),
                ],
            ),
            (
                "external-relationship",
                vec![
                    ("ppt/slides/slide1.xml", "<p:sld><a:t>Safe</a:t></p:sld>"),
                    (
                        "ppt/_rels/presentation.xml.rels",
                        "<Relationships><Relationship TargetMode=\"External\" Target=\"https://example.invalid\"/></Relationships>",
                    ),
                ],
            ),
        ];

        for (suffix, entries) in cases {
            let path = temp_path(&format!("{suffix}.pptx"));
            {
                let file = std::fs::File::create(&path).expect("create pptx");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                for (name, contents) in entries {
                    zip.start_file(name, options).expect("entry");
                    zip.write_all(contents.as_bytes()).expect("write entry");
                }
                zip.finish().expect("finish");
            }

            assert!(matches!(
                render_office_content_preview_png(&path, 256),
                Err(OfficeVisualError::NoContent)
            ));
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn pptx_visual_admission_rejects_forbidden_active_content_parts() {
        const SLIDE: &[u8] = br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Safe</a:t></p:sld>"#;
        let forbidden_parts = [
            "ppt/vbaProject.bin",
            "PPT/vbaData.xml",
            "ppt/vbaProjectSignature.bin",
            "ppt/VBAProjectSignatureAgile.bin",
            "ppt/notesSlides/notesSlide1.xml",
            "ppt/comments/comment1.xml",
            "ppt/embeddings/oleObject1.bin",
            "ppt/externalLinks/externalLink1.xml",
            "ppt/activeX/activeX1.bin",
            "ppt/ctrlProps/ctrlProp1.xml",
            "ppt/notesMasters/notesMaster1.xml",
            "ppt/commentAuthors.xml",
            "ppt/modernComment1.xml",
            "PPT/modernCommentAuthors.xml",
        ];

        for (index, forbidden_part) in forbidden_parts.iter().enumerate() {
            let path = temp_path(&format!("forbidden-content-part-{index}.pptx"));
            {
                let file = std::fs::File::create(&path).expect("create pptx");
                let mut zip = zip::ZipWriter::new(file);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("ppt/slides/slide1.xml", options)
                    .expect("slide entry");
                zip.write_all(SLIDE).expect("write slide");
                zip.start_file(*forbidden_part, options)
                    .expect("forbidden entry");
                zip.write_all(b"active content")
                    .expect("write forbidden entry");
                zip.finish().expect("finish package");
            }

            let file = std::fs::File::open(&path).expect("open pptx");
            let mut archive = zip::ZipArchive::new(file).expect("open package");
            assert!(
                !pptx_package_is_safe(&std::fs::read(&path).expect("read package"), &mut archive),
                "must reject {forbidden_part}"
            );
            assert!(matches!(
                render_office_content_preview_png(&path, 256),
                Err(OfficeVisualError::NoContent)
            ));
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn office_extra_fields_require_unique_matching_metadata() {
        let single_unknown = [0xfe, 0xca, 2, 0, 0x11, 0x22];
        let different_unknown = [0xfe, 0xca, 2, 0, 0x33, 0x44];
        let duplicate_unknown = [0xfe, 0xca, 2, 0, 0x11, 0x22, 0xfe, 0xca, 2, 0, 0x33, 0x44];

        assert!(office_extra_fields_are_safe(&single_unknown));
        assert!(office_extra_fields_are_safe(&different_unknown));
        assert!(!office_extra_fields_are_safe(&duplicate_unknown));
        assert!(office_extra_fields_match(&single_unknown, &single_unknown));
        assert!(!office_extra_fields_match(
            &single_unknown,
            &different_unknown
        ));
        assert!(!office_extra_fields_match(
            &duplicate_unknown,
            &duplicate_unknown
        ));
    }

    #[test]
    fn pptx_visual_admission_rejects_hostile_zip_metadata_before_rendering() {
        fn visual_pptx_bytes(with_second_slide: bool) -> Vec<u8> {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut bytes);
                zip.start_file(
                    "ppt/slides/slide1.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("slide");
                zip.write_all(br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Safe</a:t></p:sld>"#)
                    .expect("slide XML");
                if with_second_slide {
                    zip.start_file(
                        "ppt/slides/slide2.xml",
                        zip::write::SimpleFileOptions::default(),
                    )
                    .expect("second slide");
                    zip.write_all(br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Safe</a:t></p:sld>"#)
                        .expect("second slide XML");
                }
                zip.finish().expect("finish package");
            }
            bytes.into_inner()
        }

        fn visual_pptx_with_unicode_name_override() -> Vec<u8> {
            const RAW_NAME: &[u8] = b"ppt/vbaProject.bin";
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut bytes);
                zip.start_file(
                    "ppt/slides/slide1.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .expect("slide");
                zip.write_all(br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Safe</a:t></p:sld>"#)
                    .expect("slide XML");

                let mut unicode_path_data = vec![1];
                // `zip` validates an extra field against an empty synthetic
                // name while writing, so patch the real raw-name CRC below.
                unicode_path_data.extend_from_slice(&crc32fast::hash(b"").to_le_bytes());
                unicode_path_data.extend_from_slice(b"ppt/unused.bin");
                let mut options = zip::write::FullFileOptions::default();
                options
                    .add_extra_data(0x7075, unicode_path_data.into_boxed_slice(), false)
                    .expect("Unicode Path extra field");
                zip.start_file("ppt/vbaProject.bin", options)
                    .expect("active part");
                zip.write_all(b"active content").expect("active content");
                zip.finish().expect("finish package");
            }
            let mut bytes = bytes.into_inner();
            let central = central_offset(&bytes);
            let first_central_name_len = read_le_u16(&bytes[central + 28..central + 30])
                .expect("first name length") as usize;
            let second_central = central + 46 + first_central_name_len;
            let second_local = read_le_u32(&bytes[second_central + 42..second_central + 46])
                .expect("active-part local offset") as usize;
            let local_name_len = read_le_u16(&bytes[second_local + 26..second_local + 28])
                .expect("active-part local name length") as usize;
            let central_name_len = read_le_u16(&bytes[second_central + 28..second_central + 30])
                .expect("active-part central name length")
                as usize;
            let crc = crc32fast::hash(RAW_NAME).to_le_bytes();
            bytes[second_local + 30 + local_name_len + 5..second_local + 30 + local_name_len + 9]
                .copy_from_slice(&crc);
            bytes[second_central + 46 + central_name_len + 5
                ..second_central + 46 + central_name_len + 9]
                .copy_from_slice(&crc);
            bytes
        }

        fn visual_pptx_with_directory() -> Vec<u8> {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut bytes);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("ppt/slides/slide1.xml", options)
                    .expect("slide");
                zip.write_all(br#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Safe</a:t></p:sld>"#)
                    .expect("slide XML");
                zip.add_directory("ppt/", options).expect("directory");
                zip.finish().expect("finish package");
            }
            bytes.into_inner()
        }

        fn central_offset(bytes: &[u8]) -> usize {
            let eocd = bytes
                .windows(4)
                .rposition(|window| window == b"PK\x05\x06")
                .expect("EOCD");
            read_le_u32(&bytes[eocd + 16..eocd + 20]).expect("central offset") as usize
        }

        fn is_rejected(bytes: &[u8]) -> bool {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("open fixture");
            !pptx_package_is_safe(bytes, &mut archive)
        }

        let valid = visual_pptx_bytes(true);
        assert!(
            !is_rejected(&valid),
            "the control package must remain renderable"
        );
        let central = central_offset(&valid);
        let local = read_le_u32(&valid[central + 42..central + 46]).expect("local offset") as usize;

        let mut unix_symlink = valid.clone();
        unix_symlink[central + 5] = 3;
        unix_symlink[central + 38..central + 42]
            .copy_from_slice(&((0o120777_u32) << 16).to_le_bytes());
        assert!(
            is_rejected(&unix_symlink),
            "Unix symlink entries must fail closed"
        );

        let mut unix_special = valid.clone();
        unix_special[central + 5] = 3;
        unix_special[central + 38..central + 42]
            .copy_from_slice(&((0o010644_u32) << 16).to_le_bytes());
        assert!(
            is_rejected(&unix_special),
            "Unix special entries must fail closed"
        );

        let mut encrypted = valid.clone();
        encrypted[local + 6] |= 1;
        encrypted[central + 8] |= 1;
        assert!(
            is_rejected(&encrypted),
            "encrypted entries must fail closed"
        );

        let mut unsupported_method = valid.clone();
        unsupported_method[local + 8..local + 10].copy_from_slice(&12_u16.to_le_bytes());
        unsupported_method[central + 10..central + 12].copy_from_slice(&12_u16.to_le_bytes());
        assert!(
            is_rejected(&unsupported_method),
            "unsupported compression must fail closed"
        );

        let mut inconsistent_local_name = valid.clone();
        let name_start = local + 30;
        inconsistent_local_name[name_start] = b'P';
        assert!(
            is_rejected(&inconsistent_local_name),
            "local and central names must agree"
        );

        let mut zip64_sentinel = valid.clone();
        zip64_sentinel[central + 20..central + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            is_rejected(&zip64_sentinel),
            "ZIP64 metadata must fail closed"
        );

        let mut descriptor = visual_pptx_bytes(false);
        let descriptor_central_before = central_offset(&descriptor);
        let descriptor_local = read_le_u32(
            &descriptor[descriptor_central_before + 42..descriptor_central_before + 46],
        )
        .expect("descriptor local offset") as usize;
        let descriptor_bytes = [
            b'P',
            b'K',
            7,
            8,
            descriptor[descriptor_central_before + 16],
            descriptor[descriptor_central_before + 17],
            descriptor[descriptor_central_before + 18],
            descriptor[descriptor_central_before + 19],
            descriptor[descriptor_central_before + 20],
            descriptor[descriptor_central_before + 21],
            descriptor[descriptor_central_before + 22],
            descriptor[descriptor_central_before + 23],
            descriptor[descriptor_central_before + 24],
            descriptor[descriptor_central_before + 25],
            descriptor[descriptor_central_before + 26],
            descriptor[descriptor_central_before + 27],
        ];
        descriptor.splice(
            descriptor_central_before..descriptor_central_before,
            descriptor_bytes,
        );
        let descriptor_central = descriptor_central_before + 16;
        let descriptor_eocd = descriptor
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("shifted EOCD");
        descriptor[descriptor_eocd + 16..descriptor_eocd + 20]
            .copy_from_slice(&(descriptor_central as u32).to_le_bytes());
        descriptor[descriptor_local + 6] |= 0x08;
        descriptor[descriptor_central + 8] |= 0x08;
        descriptor[descriptor_local + 14..descriptor_local + 26].fill(0);
        assert!(
            preflight_epub_zip(&descriptor).is_some(),
            "the descriptor fixture must be structurally coherent before Office policy rejects it"
        );
        assert!(
            is_rejected(&descriptor),
            "data descriptors must fail closed"
        );
        assert_eq!(
            read_le_u32(&descriptor[descriptor_eocd + 16..descriptor_eocd + 20]),
            Some(descriptor_central as u32),
            "the descriptor must describe its shifted central directory"
        );

        let mut case_collision = valid.clone();
        let name_len =
            read_le_u16(&valid[central + 28..central + 30]).expect("name length") as usize;
        let first_record_len = 46
            + name_len
            + read_le_u16(&valid[central + 30..central + 32]).expect("first extra") as usize
            + read_le_u16(&valid[central + 32..central + 34]).expect("first comment") as usize;
        let second_central = central + first_record_len;
        let second_local = read_le_u32(&valid[second_central + 42..second_central + 46])
            .expect("second local") as usize;
        case_collision[second_central + 46..second_central + 46 + name_len]
            .copy_from_slice(b"PPT/SLIDES/SLIDE1.XML");
        case_collision[second_local + 30..second_local + 30 + name_len]
            .copy_from_slice(b"PPT/SLIDES/SLIDE1.XML");
        assert!(
            is_rejected(&case_collision),
            "case-fold collisions must fail closed"
        );

        let mut corrupt_unused_crc = valid.clone();
        corrupt_unused_crc[second_central + 16..second_central + 20]
            .copy_from_slice(&0xdead_beef_u32.to_le_bytes());
        corrupt_unused_crc[second_local + 14..second_local + 18]
            .copy_from_slice(&0xdead_beef_u32.to_le_bytes());
        assert!(
            is_rejected(&corrupt_unused_crc),
            "unused entries must have a valid decoded CRC"
        );

        let mut invalid_unused_name = valid.clone();
        invalid_unused_name[second_central + 46] = 0xff;
        invalid_unused_name[second_local + 30] = 0xff;
        assert!(
            is_rejected(&invalid_unused_name),
            "unused raw local and central names must be strict UTF-8"
        );

        let unicode_name_override = visual_pptx_with_unicode_name_override();
        assert!(
            is_rejected(&unicode_name_override),
            "Unicode Path extra fields must not override PPTX policy names"
        );

        let mut corrupt_directory_crc = visual_pptx_with_directory();
        let directory_central = central_offset(&corrupt_directory_crc)
            + 46
            + read_le_u16(
                &corrupt_directory_crc[central_offset(&corrupt_directory_crc) + 28
                    ..central_offset(&corrupt_directory_crc) + 30],
            )
            .expect("slide name length") as usize;
        let directory_local =
            read_le_u32(&corrupt_directory_crc[directory_central + 42..directory_central + 46])
                .expect("directory local offset") as usize;
        corrupt_directory_crc[directory_central + 16..directory_central + 20]
            .copy_from_slice(&0xdead_beef_u32.to_le_bytes());
        corrupt_directory_crc[directory_local + 14..directory_local + 18]
            .copy_from_slice(&0xdead_beef_u32.to_le_bytes());
        assert!(
            is_rejected(&corrupt_directory_crc),
            "directories must receive the same CRC validation as files"
        );
    }

    #[test]
    fn pptx_visual_admission_case_normalizes_policy_and_relationships() {
        const SLIDE: &[u8] = br#"<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:t>Safe</a:t></p:sld>"#;

        fn is_rejected(entries: &[(&str, &str)]) -> bool {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut zip = zip::ZipWriter::new(&mut bytes);
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("ppt/slides/slide1.xml", options)
                    .expect("slide entry");
                zip.write_all(SLIDE).expect("slide XML");
                for (name, contents) in entries {
                    zip.start_file(*name, options).expect("fixture entry");
                    zip.write_all(contents.as_bytes())
                        .expect("fixture contents");
                }
                zip.finish().expect("finish fixture");
            }
            let bytes = bytes.into_inner();
            let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).expect("open fixture");
            !pptx_package_is_safe(&bytes, &mut archive)
        }

        assert!(
            !is_rejected(&[("ppt/unused.xml", "unused but valid")]),
            "a valid unused entry remains admissible"
        );
        assert!(
            !is_rejected(&[(
                "ppt/internal.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship TargetMode=\"iNtErNaL\" Target=\"ppt/slides/slide1.xml\"/></Relationships>",
            )]),
            "TargetMode is explicitly case-insensitive only for Internal"
        );
        for (label, entries) in [
            (
                "mixed-case-media",
                vec![("PPT/MEDIA/evil.bin", "not rendered")],
            ),
            (
                "mixed-case-active-family",
                vec![("PPT/EMBEDDINGS/evil.bin", "not rendered")],
            ),
            (
                "mixed-case-relationship-name",
                vec![(
                    "ppt/evil.RELS",
                    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship TargetMode=\"eXtErNaL\" Target=\"https://example.invalid\"/></Relationships>",
                )],
            ),
            (
                "uppercase-external-mode",
                vec![(
                    "ppt/evil.rels",
                    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship TargetMode=\"EXTERNAL\" Target=\"https://example.invalid\"/></Relationships>",
                )],
            ),
            (
                "escaped-external-target",
                vec![(
                    "ppt/evil.rels",
                    "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"https&#58;//example.invalid\"/></Relationships>",
                )],
            ),
        ] {
            assert!(is_rejected(&entries), "must reject {label}");
        }
    }

    #[test]
    fn pptx_visual_preview_rejects_excess_parts_and_oversized_slide_xml() {
        let many_parts = temp_path("too-many-parts.pptx");
        {
            let file = std::fs::File::create(&many_parts).expect("create pptx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for index in 0..=MAX_OFFICE_PARTS {
                zip.start_file(format!("ppt/slides/slide{index}.xml"), options)
                    .expect("entry");
                zip.write_all(b"<p:sld><a:t>Ignored</a:t></p:sld>")
                    .expect("write entry");
            }
            zip.finish().expect("finish");
        }
        assert!(matches!(
            render_office_content_preview_png(&many_parts, 256),
            Err(OfficeVisualError::TooManyParts { .. })
        ));
        let _ = std::fs::remove_file(many_parts);

        let oversized = temp_path("oversized-slide.pptx");
        {
            let file = std::fs::File::create(&oversized).expect("create pptx");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("ppt/slides/slide1.xml", options)
                .expect("slide");
            zip.write_all(&vec![b'x'; MAX_OFFICE_XML_BYTES as usize + 1])
                .expect("write oversized slide");
            zip.finish().expect("finish");
        }
        assert!(matches!(
            render_office_content_preview_png(&oversized, 256),
            Err(OfficeVisualError::NoContent)
        ));
        let _ = std::fs::remove_file(oversized);
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

    #[test]
    fn stl_preview_routes_ascii_and_binary_meshes_to_a_bounded_visual_png() {
        let ascii = temp_path("ascii.stl");
        std::fs::write(
            &ascii,
            b"solid tetra\nfacet normal 0 0 1\n outer loop\n  vertex 0 0 0\n  vertex 1 0 0\n  vertex 0 1 1\n endloop\nendfacet\nendsolid tetra\n",
        )
        .expect("write ASCII STL");

        let binary = temp_path("binary.stl");
        let mut binary_bytes = vec![0_u8; 80];
        binary_bytes[..16].copy_from_slice(b"solid-but-binary");
        binary_bytes.extend_from_slice(&1_u32.to_le_bytes());
        for value in [
            0.0_f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0,
        ] {
            binary_bytes.extend_from_slice(&value.to_le_bytes());
        }
        binary_bytes.extend_from_slice(&0_u16.to_le_bytes());
        std::fs::write(&binary, binary_bytes).expect("write binary STL");

        for path in [&ascii, &binary] {
            let profile = preview_profile(path);
            assert_eq!(classify_path(path), PreviewKind::StlMetadata);
            assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Stl);
            assert!(!profile.raw_fallback_allowed());

            let png = render_stl_preview_png(path, 1).expect("render bounded STL PNG");
            let decoded = image::load_from_memory(&png).expect("decode STL PNG");
            assert_eq!(decoded.dimensions(), (64, 64));
            assert!(
                decoded
                    .pixels()
                    .any(|(_, _, pixel)| pixel.0 != [247, 249, 252, 255])
            );

            let preview = load_preview(&PreviewRequest {
                path: path.clone(),
                kind_hint: EntryKind::File,
            });
            assert!(preview.body.contains("Format: STL"));
            assert!(preview.body.contains("Triangles: 1"));
        }

        let _ = std::fs::remove_file(ascii);
        let _ = std::fs::remove_file(binary);
    }

    #[test]
    fn ply_preview_routes_ascii_and_both_binary_byte_orders_to_bounded_visual_png() {
        let ascii = temp_path("triangle.ply");
        std::fs::write(
            &ascii,
            b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 2\n",
        )
        .expect("write ASCII PLY");

        let binary_fixture = |name: &str, endian: fn(f32) -> [u8; 4]| {
            let path = temp_path(name);
            let mut bytes = b"ply\nformat binary_little_endian 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
            if name.contains("big") {
                bytes = b"ply\nformat binary_big_endian 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".to_vec();
            }
            for vertex in [
                [0.0_f32, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 1.0],
                [0.0, 1.0, 1.0],
            ] {
                for coordinate in vertex {
                    bytes.extend_from_slice(&endian(coordinate));
                }
            }
            bytes.extend_from_slice(&[4, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
            if name.contains("big") {
                bytes.truncate(bytes.len() - 17);
                bytes.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3]);
            }
            std::fs::write(&path, bytes).expect("write binary PLY");
            path
        };
        let little = binary_fixture("quad-little.ply", f32::to_le_bytes);
        let big = binary_fixture("quad-big.ply", f32::to_be_bytes);

        for path in [&ascii, &little, &big] {
            let profile = preview_profile(path);
            assert_eq!(classify_path(path), PreviewKind::PlyMetadata);
            assert_eq!(profile.visual_renderer(), PreviewVisualRenderer::Ply);
            assert!(!profile.raw_fallback_allowed());

            let generation = ply_preview_source_generation(path).expect("PLY source generation");
            assert_eq!(generation.as_bytes().len(), 32);
            let metadata = inspect_ply_metadata(path)
                .unwrap_or_else(|error| panic!("PLY metadata for {}: {error}", path.display()));
            assert!(metadata.vertex_count >= 3);
            assert_eq!(metadata.face_count, 1);
            assert!(metadata.triangle_count >= 1);
            let png = render_ply_preview_png(path, 1).expect("render bounded PLY PNG");
            let decoded = image::load_from_memory(&png).expect("decode PLY PNG");
            assert_eq!(decoded.dimensions(), (64, 64));
            assert!(
                decoded
                    .pixels()
                    .any(|(_, _, pixel)| pixel.0 != [247, 249, 252, 255])
            );

            let preview = load_preview(&PreviewRequest {
                path: path.clone(),
                kind_hint: EntryKind::File,
            });
            assert_eq!(preview.kind, PreviewKind::PlyMetadata);
            assert!(preview.body.contains("Format: PLY"));
            assert!(preview.body.contains("Preview: Bounded CPU mesh render"));
        }

        for path in [ascii, little, big] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ply_preview_rejects_unbounded_headers_and_unsafe_or_incomplete_meshes() {
        let cases = [
            (
                "huge-count.ply",
                b"ply\nformat ascii 1.0\nelement vertex 200001\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n".as_slice(),
            ),
            (
                "ambiguous-face-list.ply",
                b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nproperty list uchar int material_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 2 0\n".as_slice(),
            ),
            (
                "negative-index.ply",
                b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 -1\n".as_slice(),
            ),
            (
                "nan-coordinate.ply",
                b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\nnan 0 0\n1 0 0\n0 1 1\n3 0 1 2\n".as_slice(),
            ),
            (
                "concave-face.ply",
                b"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n2 0 0\n1 0.2 1\n0 2 2\n4 0 1 2 3\n".as_slice(),
            ),
            (
                "trailing-body.ply",
                b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 2\nextra\n".as_slice(),
            ),
        ];
        for (name, source) in cases {
            let path = temp_path(name);
            std::fs::write(&path, source).expect("write rejected PLY fixture");
            assert!(
                inspect_ply_metadata(&path).is_err(),
                "{name} must fail closed"
            );
            assert!(
                render_ply_preview_png(&path, 128).is_err(),
                "{name} must not render"
            );
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn ply_preview_rejects_oversized_input_before_parser_allocation() {
        let path = temp_path("oversized.ply");
        let file = std::fs::File::create(&path).expect("create oversized PLY fixture");
        file.set_len(crate::ply_visual::MAX_PLY_PREVIEW_BYTES + 1)
            .expect("extend oversized PLY fixture");
        assert!(matches!(
            inspect_ply_metadata(&path),
            Err(PlyVisualError::InputTooLarge { .. })
        ));
        assert!(matches!(
            render_ply_preview_png(&path, 128),
            Err(PlyVisualError::InputTooLarge { .. })
        ));
        std::fs::remove_file(path).expect("remove oversized PLY fixture");
    }

    #[test]
    fn folder_preview_reports_nested_file_sizes_without_following_symlinks() {
        let root = temp_path("folder-recursive-size");
        let outside = temp_path("folder-recursive-size-outside");
        std::fs::create_dir_all(root.join("nested")).expect("create nested fixture");
        std::fs::create_dir_all(&outside).expect("create outside fixture");
        std::fs::write(root.join("direct.txt"), b"abc").expect("write direct fixture");
        std::fs::write(root.join("nested/child.txt"), b"de").expect("write nested fixture");
        std::fs::write(outside.join("not-counted.txt"), b"not-counted")
            .expect("write outside fixture");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("outside-link"))
            .expect("create directory symlink");

        let preview = preview_folder("folder-recursive-size".to_string(), &root);

        assert!(
            preview.body.contains("Known recursive file size: 5 B"),
            "nested regular files must contribute while a directory symlink stays untraversed: {}",
            preview.body
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn recursive_folder_size_fails_closed_when_child_directory_becomes_symlink() {
        let root = temp_path("folder-recursive-size-replaced-directory");
        let outside = temp_path("folder-recursive-size-replaced-directory-outside");
        std::fs::create_dir_all(root.join("nested")).expect("create nested fixture");
        std::fs::create_dir_all(&outside).expect("create outside fixture");
        std::fs::write(root.join("nested/inside.txt"), b"inside").expect("write nested fixture");
        std::fs::write(outside.join("must-not-count.txt"), b"outside bytes")
            .expect("write outside fixture");

        let size = summarize_folder_recursive_with_before_directory_open(
            &root,
            MAX_FOLDER_RECURSIVE_ENTRIES,
            |name| {
                if name == std::ffi::OsStr::new("nested") {
                    std::fs::remove_dir_all(root.join("nested"))
                        .expect("replace original child directory");
                    std::os::unix::fs::symlink(&outside, root.join("nested"))
                        .expect("replace child directory with symlink");
                }
            },
        );

        assert_eq!(
            size,
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true,
            },
            "a replacement symlink must never cause outside bytes to be counted"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
