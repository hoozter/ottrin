use ottrin_preview::{
    PdfVisualError, render_pdf_first_page_png, render_pdf_first_page_png_with_library,
};
use std::path::Path;

#[test]
fn explicit_missing_runtime_fails_closed() {
    let fixture = std::env::temp_dir().join(format!(
        "ottrin-pdf-missing-runtime-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&fixture, b"%PDF-1.4\n%%EOF\n").expect("write PDF fixture");
    let error = render_pdf_first_page_png_with_library(
        &fixture,
        256,
        Path::new("/definitely/missing/libpdfium.so"),
    )
    .expect_err("missing packaged runtime must fail closed");

    assert!(matches!(error, PdfVisualError::RuntimeLoad(_)));
    std::fs::remove_file(fixture).expect("remove PDF fixture");
}

#[test]
#[ignore = "requires the pinned packaged PDFium runtime"]
fn packaged_runtime_renders_bounded_non_blank_first_page_png() {
    let library = std::env::var_os("OTTRIN_TEST_PDFIUM_LIBRARY")
        .map(std::path::PathBuf::from)
        .and_then(|path| {
            std::fs::canonicalize(&path)
                .or_else(|_| std::env::current_dir().map(|cwd| cwd.join(&path)))
                .ok()
        })
        .expect("OTTRIN_TEST_PDFIUM_LIBRARY must point to the packaged runtime");
    let fixture =
        std::env::temp_dir().join(format!("ottrin-pdf-render-{}.pdf", std::process::id()));
    std::fs::write(&fixture, pdf_fixture()).expect("write PDF fixture");

    let png = render_pdf_first_page_png_with_library(&fixture, 512, &library)
        .expect("render first PDF page");
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .expect("decode rendered PNG");

    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(image.width() <= 512);
    assert!(image.height() <= 512);
    assert!(
        image.to_luma8().pixels().any(|pixel| pixel[0] < 240),
        "fixture text must produce non-white pixels"
    );
    std::fs::remove_file(fixture).expect("remove PDF fixture");
}

#[test]
#[ignore = "requires the pinned packaged PDFium runtime"]
fn packaged_runtime_renders_more_than_one_pdf_per_process() {
    let library = packaged_pdfium_library().expect("packaged PDFium runtime");
    let first = std::env::temp_dir().join(format!("ottrin-pdf-first-{}.pdf", std::process::id()));
    let second = std::env::temp_dir().join(format!("ottrin-pdf-second-{}.pdf", std::process::id()));
    std::fs::write(&first, pdf_fixture()).expect("write first PDF fixture");
    std::fs::write(&second, pdf_fixture()).expect("write second PDF fixture");

    render_pdf_first_page_png_with_library(&first, 512, &library)
        .expect("render first PDF in process");
    render_pdf_first_page_png_with_library(&second, 512, &library)
        .expect("reuse process-wide PDFium binding for second PDF");

    let _ = std::fs::remove_file(first);
    let _ = std::fs::remove_file(second);
}

#[test]
#[ignore = "requires the development package produced by build-all.sh"]
fn debug_runtime_discovers_packaged_pdfium_automatically() {
    let fixture = std::env::temp_dir().join(format!(
        "ottrin-pdf-debug-runtime-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&fixture, pdf_fixture()).expect("write PDF fixture");
    let png = render_pdf_first_page_png(&fixture, 512)
        .expect("debug runtime should discover the workspace packaged PDFium");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    std::fs::remove_file(fixture).expect("remove PDF fixture");
}

fn packaged_pdfium_library() -> Option<std::path::PathBuf> {
    std::env::var_os("OTTRIN_TEST_PDFIUM_LIBRARY")
        .map(std::path::PathBuf::from)
        .and_then(|path| {
            std::fs::canonicalize(&path)
                .or_else(|_| std::env::current_dir().map(|cwd| cwd.join(path)))
                .ok()
        })
}

fn pdf_fixture() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Length 56 >>\nstream\nBT /F1 24 Tf 72 700 Td (Ottrin PDF visual preview) Tj ET\nendstream".to_string(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0_u64];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len() as u64);
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}
