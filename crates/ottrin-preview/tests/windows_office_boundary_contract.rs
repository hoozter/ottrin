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
    let output = root.join("output-with-a-long-name");
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

#[test]
#[ignore = "requires OTTRIN_NATIVE_OFFICE_FIXTURES and installed LibreOffice/PDFium"]
fn windows_installed_office_renders_each_advertised_format() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeRenderPolicy, OfficeRenderRequest,
        detect_installed_libreoffice, office_format_for_path, render_office_preview_page,
    };
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let fixtures = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_FIXTURES").expect("owned native fixtures"),
    );
    let launcher = WindowsJobObjectIsolation;
    let cancelled = AtomicBool::new(false);
    let detected = detect_installed_libreoffice(&launcher, &cancelled).unwrap();
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
    let providers = [detected.capability];
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-formats-{nonce}"));
    std::fs::create_dir(&root).unwrap();
    let mut failures = Vec::new();
    for (name, marker) in [
        ("document.doc", "DOCX BASELINE"),
        ("document.docx", "DOCX BASELINE"),
        ("document.odt", "DOCX BASELINE"),
        ("sheet.xls", "OTTRIN SHEET FORMAT"),
        ("sheet.xlsx", "OTTRIN SHEET FORMAT"),
        ("sheet.xlsm", "OTTRIN SHEET FORMAT"),
        ("sheet.ods", "OTTRIN SHEET FORMAT"),
        ("slides.pptx", "OTTRIN SLIDE FORMAT"),
        ("slides.odp", "OTTRIN SLIDE FORMAT"),
    ] {
        let original = std::fs::read(fixtures.join(name)).unwrap();
        let source = root.join(name);
        std::fs::write(&source, &original).unwrap();
        let output = root.join(format!("output-{name}"));
        std::fs::create_dir(&output).unwrap();
        let request = OfficeRenderRequest {
            source_path: source.clone(),
            format: office_format_for_path(&source).unwrap(),
            private_output_dir: output.clone(),
            policy: OfficeRenderPolicy {
                version: 1,
                max_input_bytes: 1024 * 1024,
                max_output_bytes: 8 * 1024 * 1024,
                max_pages: 8,
                timeout: Duration::from_secs(30),
            },
        };
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
        );
        assert_eq!(
            std::fs::read(&source).unwrap(),
            original,
            "{name}: source changed"
        );
        let rendered = match rendered {
            Ok(rendered) => rendered,
            Err(error) => {
                eprintln!("rejected {name}: {error:?}");
                failures.push(format!("{name}: {error:?}"));
                assert!(
                    !std::fs::read_dir(&output).unwrap().any(|entry| entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".ottrin-")),
                    "{name}: profile/log leak"
                );
                continue;
            }
        };
        let image = image::load_from_memory(&rendered.png).unwrap().to_rgba8();
        assert!(
            image
                .pixels()
                .any(|pixel| pixel[0] < 240 && pixel[3] == 255),
            "{name}: blank page"
        );
        let pdfium = pdfium_render::prelude::Pdfium::default();
        let document = pdfium
            .load_pdf_from_file(&rendered.validated_pdf_path, None)
            .unwrap();
        let text = document.pages().get(0).unwrap().text().unwrap().all();
        assert!(
            text.contains(marker),
            "{name}: expected {marker:?}, got {text:?}"
        );
        drop(document);
        assert_eq!(
            std::fs::read(&source).unwrap(),
            original,
            "{name}: source changed"
        );
        assert_eq!(
            std::fs::read_dir(&output).unwrap().count(),
            1,
            "{name}: profile/log leak"
        );
        println!(
            "accepted {name}; darkest rendered pixel {}",
            image.pixels().map(|pixel| pixel[0]).min().unwrap()
        );
    }
    std::fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "format failures: {failures:?}");
}

#[test]
#[ignore = "requires installed Windows LibreOffice"]
fn windows_installed_office_cancels_after_provider_start_and_releases_files() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, IsolatedProcessFailure, IsolatedProcessOutput,
        IsolatedProcessRequest, OfficeFormat, OfficeRenderError, OfficeRenderPolicy,
        OfficeRenderRequest, detect_installed_libreoffice, render_office_pdf,
    };
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct CancelAfterProfileLock {
        observed_start: AtomicBool,
    }
    impl IsolatedProcessLauncher for CancelAfterProfileLock {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            std::thread::scope(|scope| {
                let worker = scope.spawn(|| WindowsJobObjectIsolation.run(request, cancelled));
                let marker = request.private_profile_dir.join(".lock");
                let deadline = Instant::now() + Duration::from_secs(10);
                while !marker.exists() && !worker.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                self.observed_start
                    .store(marker.exists(), Ordering::Release);
                cancelled.store(true, Ordering::Release);
                worker.join().unwrap()
            })
        }
    }

    let detected =
        detect_installed_libreoffice(&WindowsJobObjectIsolation, &AtomicBool::new(false)).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-cancel-{nonce}"));
    let output = root.join("output");
    std::fs::create_dir_all(&output).unwrap();
    let source = root.join("source.docx");
    let mut archive =
        zip::ZipArchive::new(Cursor::new(include_bytes!("fixtures/docx-baseline.docx"))).unwrap();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&source).unwrap());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        writer
            .start_file(
                &name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        if name == "word/document.xml" {
            writer.write_all(br#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#).unwrap();
            for paragraph in 0..2000 {
                write!(
                    writer,
                    "<w:p><w:r><w:t>Cancellation acceptance paragraph {paragraph}</w:t></w:r></w:p>"
                )
                .unwrap();
            }
            writer.write_all(b"</w:body></w:document>").unwrap();
        } else {
            std::io::copy(&mut entry, &mut writer).unwrap();
        }
    }
    writer.finish().unwrap();
    let original = std::fs::read(&source).unwrap();
    let launcher = CancelAfterProfileLock {
        observed_start: AtomicBool::new(false),
    };
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
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
    let result = render_office_pdf(
        &[detected.capability],
        &request,
        &converter,
        &AtomicBool::new(false),
    );
    assert!(
        launcher.observed_start.load(Ordering::Acquire),
        "provider must acquire its real profile lock before cancellation"
    );
    assert_eq!(result, Err(OfficeRenderError::Cancelled));
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert!(std::fs::read_dir(&output).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ottrin-")
    }));
    std::fs::rename(&source, root.join("released.docx")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
