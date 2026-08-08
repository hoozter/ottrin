use pdfium_render::prelude::*;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const PDFIUM_DIR_ENV: &str = "OTTRIN_PDFIUM_DIR";
const MAX_PDF_VISUAL_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum PdfVisualError {
    #[error("could not inspect PDF input: {0}")]
    InputMetadata(String),
    #[error("PDF input is too large for a visual preview ({actual} bytes; limit {limit} bytes)")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error(
        "PDFium runtime is unavailable; set {PDFIUM_DIR_ENV} or package pdfium/lib beside Ottrin"
    )]
    RuntimeUnavailable,
    #[error("could not load PDFium runtime: {0}")]
    RuntimeLoad(String),
    #[error("could not load PDF: {0}")]
    DocumentLoad(String),
    #[error("PDF has no pages")]
    EmptyDocument,
    #[error("PDF page {requested} is outside the document's {page_count} pages")]
    PageOutOfBounds { requested: usize, page_count: usize },
    #[error("could not render PDF page: {0}")]
    Render(String),
    #[error("could not encode PDF preview: {0}")]
    Encode(String),
}

pub fn render_pdf_first_page_png(path: &Path, target_dim: u32) -> Result<Vec<u8>, PdfVisualError> {
    render_pdf_page_png(path, 0, target_dim)
}

pub fn render_pdf_page_png(
    path: &Path,
    page_index: usize,
    target_dim: u32,
) -> Result<Vec<u8>, PdfVisualError> {
    let library_dirs = pdfium_runtime_library_dirs();
    render_pdf_page_png_from_dirs(path, page_index, target_dim, library_dirs)
}

fn pdfium_runtime_library_dirs() -> Vec<PathBuf> {
    let env_dir = std::env::var_os(PDFIUM_DIR_ENV).map(PathBuf::from);
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    pdfium_library_dirs_from(env_dir, executable_dir, development_pdfium_dirs())
}

/// Returns whether the first discovered runtime can provide PDFium bindings.
///
/// The renderer and cached-preview admission must share this exact discovery
/// order: a filename is not evidence that the runtime can actually render.
pub fn pdfium_runtime_is_available() -> bool {
    pdfium_runtime_is_available_from_dirs(pdfium_runtime_library_dirs())
}

fn pdfium_runtime_is_available_from_dirs(dirs: Vec<PathBuf>) -> bool {
    let Some(library) = find_pdfium_library_from(dirs) else {
        return false;
    };
    matches!(
        Pdfium::bind_to_library(library),
        Ok(_) | Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized)
    )
}

pub fn render_pdf_first_page_png_with_library(
    path: &Path,
    target_dim: u32,
    library: &Path,
) -> Result<Vec<u8>, PdfVisualError> {
    render_pdf_page_png_with_library(path, 0, target_dim, library)
}

pub fn render_pdf_page_png_with_library(
    path: &Path,
    page_index: usize,
    target_dim: u32,
    library: &Path,
) -> Result<Vec<u8>, PdfVisualError> {
    let input_size = std::fs::metadata(path)
        .map_err(|error| PdfVisualError::InputMetadata(error.to_string()))?
        .len();
    if input_size > MAX_PDF_VISUAL_BYTES {
        return Err(PdfVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_PDF_VISUAL_BYTES,
        });
    }
    let target_dim = target_dim.clamp(64, 2_048) as i32;
    let pdfium = match Pdfium::bind_to_library(library) {
        Ok(bindings) => Pdfium::new(bindings),
        // pdfium-render promotes the first binding into a process-wide
        // OnceCell. Every later bind attempt reports this sentinel even
        // though the existing binding is healthy and intentionally reusable.
        // Treating it as a render failure made only the first PDF viewed in an
        // Ottrin process work.
        Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => Pdfium::default(),
        Err(error) => return Err(PdfVisualError::RuntimeLoad(error.to_string())),
    };
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| PdfVisualError::DocumentLoad(error.to_string()))?;
    let pages = document.pages();
    let page_count = pages.len() as usize;
    if page_count == 0 {
        return Err(PdfVisualError::EmptyDocument);
    }
    let page = pages
        .get(page_index as i32)
        .map_err(|_| PdfVisualError::PageOutOfBounds {
            requested: page_index,
            page_count,
        })?;
    let image = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(target_dim)
                .set_maximum_height(target_dim),
        )
        .map_err(|error| PdfVisualError::Render(error.to_string()))?
        .as_image()
        .map_err(|error| PdfVisualError::Render(error.to_string()))?;
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|error| PdfVisualError::Encode(error.to_string()))?;
    Ok(bytes)
}

fn pdfium_library_dirs_from(
    env_dir: Option<PathBuf>,
    executable_dir: Option<PathBuf>,
    development_dirs: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(env_dir) = env_dir {
        dirs.push(env_dir.clone());
        dirs.push(env_dir.join("lib"));
    }
    if let Some(executable_dir) = executable_dir {
        dirs.push(executable_dir.join("pdfium/lib"));
        dirs.push(executable_dir.join("pdfium"));
        dirs.push(executable_dir);
    }
    for development_dir in development_dirs {
        dirs.push(development_dir.join("lib"));
        dirs.push(development_dir);
    }
    dirs
}

#[cfg(debug_assertions)]
fn development_pdfium_dirs() -> Vec<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let platform_dir = if cfg!(target_os = "windows") {
        format!("Ottrin-{}-windows-x86_64", env!("CARGO_PKG_VERSION"))
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        format!("Ottrin-{}-linux-x86_64", env!("CARGO_PKG_VERSION"))
    } else {
        return Vec::new();
    };
    vec![
        workspace.join("dist").join(platform_dir).join("bin/pdfium"),
        workspace.join("dist/AppDir/usr/bin/pdfium"),
    ]
}

#[cfg(not(debug_assertions))]
fn development_pdfium_dirs() -> Vec<PathBuf> {
    Vec::new()
}

fn find_pdfium_library_from(dirs: Vec<PathBuf>) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|dir| Pdfium::pdfium_platform_library_name_at_path(&dir))
        .find(|path| path.is_file())
}

fn render_pdf_page_png_from_dirs(
    path: &Path,
    page_index: usize,
    target_dim: u32,
    dirs: Vec<PathBuf>,
) -> Result<Vec<u8>, PdfVisualError> {
    let library = find_pdfium_library_from(dirs).ok_or(PdfVisualError::RuntimeUnavailable)?;
    render_pdf_page_png_with_library(path, page_index, target_dim, &library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn runtime_candidates_prefer_override_then_packaged_layout() {
        let dirs = pdfium_library_dirs_from(
            Some(PathBuf::from("/override")),
            Some(PathBuf::from("/opt/ottrin")),
            vec![PathBuf::from("/workspace/runtime/pdfium")],
        );

        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/override"),
                PathBuf::from("/override/lib"),
                PathBuf::from("/opt/ottrin/pdfium/lib"),
                PathBuf::from("/opt/ottrin/pdfium"),
                PathBuf::from("/opt/ottrin"),
                PathBuf::from("/workspace/runtime/pdfium/lib"),
                PathBuf::from("/workspace/runtime/pdfium"),
            ]
        );
    }

    #[test]
    fn runtime_candidates_include_each_development_payload_layout() {
        let dirs = pdfium_library_dirs_from(
            None,
            None,
            vec![
                PathBuf::from("/workspace/dist/Ottrin-linux/bin/pdfium"),
                PathBuf::from("/workspace/dist/AppDir/usr/bin/pdfium"),
            ],
        );

        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/workspace/dist/Ottrin-linux/bin/pdfium/lib"),
                PathBuf::from("/workspace/dist/Ottrin-linux/bin/pdfium"),
                PathBuf::from("/workspace/dist/AppDir/usr/bin/pdfium/lib"),
                PathBuf::from("/workspace/dist/AppDir/usr/bin/pdfium"),
            ]
        );
    }

    #[test]
    fn runtime_selection_uses_first_existing_platform_library() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-pdfium-candidate-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let runtime_dir = root.join("runtime");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime fixture");
        let runtime = Pdfium::pdfium_platform_library_name_at_path(&runtime_dir);
        std::fs::write(&runtime, b"fixture").expect("write runtime fixture");

        let selected = find_pdfium_library_from(vec![root.join("missing"), runtime_dir]);

        assert_eq!(selected, Some(runtime.clone()));
        std::fs::remove_dir_all(root).expect("remove runtime fixture");
    }

    #[test]
    fn runtime_availability_rejects_an_unloadable_first_discovered_library() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-pdfium-unloadable-runtime-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&root).expect("create runtime fixture");
        let library = Pdfium::pdfium_platform_library_name_at_path(&root);
        std::fs::write(&library, b"not a loadable PDFium library")
            .expect("write unloadable runtime fixture");

        assert!(
            !pdfium_runtime_is_available_from_dirs(vec![root.clone()]),
            "a filename alone must not admit a cached PDF preview when PDFium cannot bind it"
        );

        std::fs::remove_dir_all(root).expect("remove runtime fixture");
    }

    #[test]
    fn empty_runtime_capability_fails_without_system_fallback() {
        let error = render_pdf_page_png_from_dirs(Path::new("missing.pdf"), 0, 256, vec![])
            .expect_err("empty capability set must fail closed");

        assert!(matches!(error, PdfVisualError::RuntimeUnavailable));
    }

    #[test]
    fn oversized_pdf_is_rejected_before_pdfium_load() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-oversized-pdf-{}-{}.pdf",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let file = std::fs::File::create(&path).expect("create oversized fixture");
        file.set_len(MAX_PDF_VISUAL_BYTES + 1)
            .expect("size oversized fixture");

        let error = render_pdf_first_page_png_with_library(
            &path,
            256,
            Path::new("/definitely/missing/libpdfium.so"),
        )
        .expect_err("oversized input must be rejected before runtime loading");

        assert!(matches!(error, PdfVisualError::InputTooLarge { .. }));
        std::fs::remove_file(path).expect("remove oversized fixture");
    }
}
