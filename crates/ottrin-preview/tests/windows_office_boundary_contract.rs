#![cfg(target_os = "windows")]

use ottrin_preview::{IsolatedProcessLauncher, WindowsJobObjectIsolation};

#[test]
fn windows_office_boundary_is_an_explicit_launcher_capability() {
    fn assert_launcher<T: IsolatedProcessLauncher + Copy + Default>() {}

    assert_launcher::<WindowsJobObjectIsolation>();
}

#[test]
#[ignore = "requires installed Windows LibreOffice and a packaged PDFium runtime"]
fn windows_installed_office_converts_and_cleans_private_state() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeFormat, OfficeProviderId, OfficeRenderError,
        OfficeRenderPolicy, OfficeRenderRequest, detect_installed_libreoffice, render_office_pdf,
        render_office_preview_page,
    };
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let launcher = WindowsJobObjectIsolation;
    let cancelled = AtomicBool::new(false);
    let detected = detect_installed_libreoffice(&launcher, &cancelled)
        .expect("detect the installed provider through the production search path");
    assert_eq!(
        detected.capability.id,
        OfficeProviderId::InstalledLibreOffice
    );
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-native-{unique}"));
    let output = root.join("output");
    std::fs::create_dir_all(&output).unwrap();
    let source = root.join("source.docx");
    let original = include_bytes!("fixtures/docx-baseline.docx");
    std::fs::write(&source, original).unwrap();
    let request = OfficeRenderRequest {
        source_path: source.clone(),
        format: OfficeFormat::Docx,
        private_output_dir: output.clone(),
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024 * 1024,
            max_output_bytes: 8 * 1024 * 1024,
            max_pages: 8,
            timeout: Duration::from_secs(30),
        },
    };
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
    let providers = [detected.capability];
    let rendered = render_office_preview_page(
        &providers,
        &request,
        &converter,
        0,
        256,
        &cancelled,
        |path, page, dimension| {
            ottrin_preview::render_pdf_page_png(path, page, dimension)
                .map_err(|error| error.to_string())
        },
    )
    .expect("native isolated DOCX conversion and production page rendering");
    let image = image::load_from_memory(&rendered.png).unwrap().to_rgba8();
    assert!(
        image
            .pixels()
            .filter(|pixel| pixel[0] < 128 && pixel[3] == 255)
            .count()
            > 20
    );
    let pdfium = pdfium_render::prelude::Pdfium::default();
    let pdf = pdfium
        .load_pdf_from_file(&rendered.validated_pdf_path, None)
        .unwrap();
    assert!(
        pdf.pages()
            .get(0)
            .unwrap()
            .text()
            .unwrap()
            .all()
            .contains("DOCX BASELINE")
    );
    drop(pdf);
    assert!(
        rendered
            .provider_label
            .starts_with("Rendered with LibreOffice ")
    );
    assert!(rendered.fallback_failures.is_empty());
    assert_eq!(
        rendered.cache_identity.provider,
        OfficeProviderId::InstalledLibreOffice
    );
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(&output).unwrap().count(),
        1,
        "only the PDF remains"
    );
    assert!(matches!(
        render_office_pdf(&providers, &request, &converter, &AtomicBool::new(true)),
        Err(OfficeRenderError::Cancelled)
    ));
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}
