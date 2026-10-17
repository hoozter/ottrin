#![cfg(any(target_os = "linux", target_os = "windows"))]

use ottrin_preview::IsolatedProcessLauncher;
#[cfg(target_os = "linux")]
use ottrin_preview::LinuxBubblewrapIsolation as NativeOfficeIsolation;
#[cfg(target_os = "windows")]
use ottrin_preview::WindowsJobObjectIsolation as NativeOfficeIsolation;

struct PrivateDirectory(std::path::PathBuf);

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_office_boundary_is_an_explicit_launcher_capability() {
    fn assert_launcher<T: IsolatedProcessLauncher + Copy + Default>() {}

    assert_launcher::<NativeOfficeIsolation>();
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires installed LibreOffice and the production helper; also run in a batch session"]
fn windows_native_office_metadata_probe_is_desktop_independent() {
    let detected = ottrin_preview::detect_installed_libreoffice(
        &NativeOfficeIsolation,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .expect("sandboxed metadata probing must not initialize a Windows desktop");
    assert!(detected.executable.is_file());
}

#[test]
#[ignore = "requires installed LibreOffice and a packaged PDFium runtime"]
fn native_installed_office_converts_and_cleans_private_state() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeFormat, OfficeProviderId, OfficeRenderError,
        OfficeRenderPolicy, OfficeRenderRequest, detect_installed_libreoffice, render_office_pdf,
        render_office_preview_page,
    };
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let launcher = NativeOfficeIsolation;
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
    let _cleanup = PrivateDirectory(root.clone());
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
fn native_installed_office_renders_each_advertised_format() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeRenderPolicy, OfficeRenderRequest,
        detect_installed_libreoffice, office_format_for_path, render_office_preview_page,
    };
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let fixtures = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_FIXTURES").expect("owned native fixtures"),
    );
    let launcher = NativeOfficeIsolation;
    let cancelled = AtomicBool::new(false);
    let detected = detect_installed_libreoffice(&launcher, &cancelled).unwrap();
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
    let providers = [detected.capability];
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-formats-{nonce}"));
    let _cleanup = PrivateDirectory(root.clone());
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
#[ignore = "requires installed LibreOffice"]
fn native_installed_office_cancels_after_provider_start_and_releases_files() {
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
                let worker = scope.spawn(|| NativeOfficeIsolation.run(request, cancelled));
                let marker = request.private_profile_dir.join(".lock");
                // A cold installed provider may need more than ten seconds to
                // initialize. Use its actual child budget, retaining time to
                // cancel before the launcher's own timeout can win the race.
                let started = Instant::now();
                let readiness_budget = request.timeout.saturating_sub(Duration::from_secs(1));
                let deadline = started + readiness_budget;
                while !marker.exists() && !worker.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                self.observed_start
                    .store(marker.exists(), Ordering::Release);
                eprintln!(
                    "Provider profile readiness: {} after {:?} (budget {:?})",
                    marker.exists(),
                    started.elapsed(),
                    readiness_budget
                );
                cancelled.store(true, Ordering::Release);
                worker.join().unwrap()
            })
        }
    }

    let detected =
        detect_installed_libreoffice(&NativeOfficeIsolation, &AtomicBool::new(false)).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-cancel-{nonce}"));
    let _cleanup = PrivateDirectory(root.clone());
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

#[cfg(target_os = "windows")]
#[test]
#[ignore = "requires paired macro-policy controls and installed LibreOffice/PDFium"]
fn native_installed_office_suppresses_document_macros() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeFormat, OfficeRenderPolicy, OfficeRenderRequest,
        detect_installed_libreoffice, render_office_pdf,
    };
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // Companion ordinary-user Kit controls explicitly invoke the same harmless
    // document macro with macros enabled and with the production load policy.
    // Automatic document events alone are not a sufficient positive control.
    let fixture = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_MACRO_FIXTURE").expect("verified macro fixture"),
    );
    let original = std::fs::read(fixture).unwrap();
    assert!(ottrin_preview::pdfium_runtime_is_available());
    let pdfium = pdfium_render::prelude::Pdfium::default();
    let control_path = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_MACRO_CONTROL_PDF")
            .expect("macro positive control PDF"),
    );
    let control = pdfium.load_pdf_from_file(&control_path, None).unwrap();
    let control_text = control
        .pages()
        .iter()
        .map(|page| page.text().unwrap().all())
        .collect::<String>();
    assert!(
        control_text.contains("OTTRIN MACRO EXECUTED"),
        "positive control macro did not run: {control_text:?}"
    );
    drop(control);
    let denied_path = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_MACRO_DENIED_CONTROL_PDF")
            .expect("production-policy macro challenge PDF"),
    );
    let denied = pdfium.load_pdf_from_file(&denied_path, None).unwrap();
    let denied_text = denied
        .pages()
        .iter()
        .map(|page| page.text().unwrap().all())
        .collect::<String>();
    assert!(
        denied_text.contains("DOCX BASELINE") && !denied_text.contains("OTTRIN MACRO EXECUTED"),
        "production-policy control must load the document and deny the same macro: {denied_text:?}"
    );
    drop(denied);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("ottrin-office-macro-{nonce}"));
    let _cleanup = PrivateDirectory(root.clone());
    let output = root.join("output");
    std::fs::create_dir_all(&output).unwrap();
    let source = root.join("macro.odt");
    std::fs::write(&source, &original).unwrap();
    let cancelled = AtomicBool::new(false);
    let launcher = NativeOfficeIsolation;
    let detected = detect_installed_libreoffice(&launcher, &cancelled).unwrap();
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
    let request = OfficeRenderRequest {
        source_path: source.clone(),
        format: OfficeFormat::Odt,
        private_output_dir: output.clone(),
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024 * 1024,
            max_output_bytes: 8 * 1024 * 1024,
            max_pages: 8,
            timeout: Duration::from_secs(30),
        },
    };
    let rendered =
        render_office_pdf(&[detected.capability], &request, &converter, &cancelled).unwrap();
    let pdf = pdfium.load_pdf_from_file(&rendered.pdf_path, None).unwrap();
    let text = pdf
        .pages()
        .iter()
        .map(|page| page.text().unwrap().all())
        .collect::<String>();
    assert!(
        text.contains("DOCX BASELINE"),
        "document must actually load: {text:?}"
    );
    assert!(
        !text.contains("OTTRIN MACRO EXECUTED"),
        "document macro ran: {text:?}"
    );
    drop(pdf);
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
}

#[test]
#[ignore = "requires installed LibreOffice/PDFium and an owned OTTRIN_NATIVE_OFFICE_FIDELITY_OUTPUT"]
fn native_installed_office_preserves_representative_layout() {
    use ottrin_preview::{
        InstalledLibreOfficeConverter, OfficeRenderPolicy, OfficeRenderRequest,
        detect_installed_libreoffice, office_format_for_path, render_office_pdf,
    };
    use pdfium_render::prelude::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    fn feature(page: &PdfPage<'_>, label: &str) -> (PdfRect, f32, PdfColor, String) {
        let text = page.text().unwrap();
        let search = text.search(label, &PdfSearchOptions::new()).unwrap();
        let found = search
            .find_next()
            .unwrap_or_else(|| panic!("missing {label:?}"));
        let segment = found.first().unwrap();
        let chars = segment.chars().unwrap();
        let first = chars.get(0).unwrap();
        let result = (
            segment.bounds(),
            first.unscaled_font_size().value,
            first.fill_color().unwrap(),
            first.font_name(),
        );
        println!("{label}: {result:?}");
        result
    }

    let artifacts = std::path::PathBuf::from(
        std::env::var_os("OTTRIN_NATIVE_OFFICE_FIDELITY_OUTPUT").expect("owned fidelity output"),
    );
    std::fs::create_dir(&artifacts).expect("fidelity output must not already exist");
    let root = tempfile::tempdir().unwrap();
    let launcher = NativeOfficeIsolation;
    let cancelled = AtomicBool::new(false);
    let detected = detect_installed_libreoffice(&launcher, &cancelled).unwrap();
    let converter = InstalledLibreOfficeConverter::new(detected.executable, &launcher);
    let pdfium = Pdfium::default();
    for (name, original, pages) in [
        (
            "office-fidelity.docx",
            include_bytes!("fixtures/office-fidelity.docx").as_slice(),
            2,
        ),
        (
            "office-fidelity.xlsx",
            include_bytes!("fixtures/office-fidelity.xlsx").as_slice(),
            1,
        ),
        (
            "office-fidelity.pptx",
            include_bytes!("fixtures/office-fidelity.pptx").as_slice(),
            2,
        ),
    ] {
        let source = root.path().join(name);
        std::fs::write(&source, original).unwrap();
        let output = root.path().join(format!("output-{name}"));
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
        let rendered = render_office_pdf(
            std::slice::from_ref(&detected.capability),
            &request,
            &converter,
            &cancelled,
        )
        .unwrap();
        let pdf = pdfium.load_pdf_from_file(&rendered.pdf_path, None).unwrap();
        assert_eq!(pdf.pages().len(), pages, "{name}: pagination");
        std::fs::copy(&rendered.pdf_path, artifacts.join(format!("{name}.pdf"))).unwrap();
        for (index, page) in pdf.pages().iter().enumerate() {
            let png = ottrin_preview::render_pdf_page_png(&rendered.pdf_path, index, 1200).unwrap();
            std::fs::write(artifacts.join(format!("{name}-{}.png", index + 1)), &png).unwrap();
            let image = image::load_from_memory(&png).unwrap().to_rgb8();
            let text = page.text().unwrap().all();
            std::fs::write(artifacts.join(format!("{name}-{}.txt", index + 1)), &text).unwrap();
            match name {
                "office-fidelity.docx" if index == 0 => {
                    assert!((page.width().value - 612.).abs() < 1.);
                    assert!((page.height().value - 792.).abs() < 1.);
                    let (bounds, size, color, font) = feature(&page, "OTTRIN DOCUMENT FIDELITY");
                    assert!(((bounds.left().value + bounds.right().value) / 2. - 306.).abs() < 3.);
                    assert!((size - 20.).abs() < 1.);
                    assert!(color.blue() > 140 && color.red() < 60);
                    assert!(font.to_lowercase().contains("bold"));
                    assert!(
                        feature(&page, "Bold sample")
                            .3
                            .to_lowercase()
                            .contains("bold")
                    );
                    assert!(
                        feature(&page, "italic sample")
                            .3
                            .to_lowercase()
                            .contains("italic")
                    );
                    let left = feature(&page, "Feature").0;
                    let right = feature(&page, "Expected layout").0;
                    assert!(left.right().value < right.left().value);
                    // Descenders change glyph-box bottoms despite a shared
                    // baseline; these capitalized labels share their cap top.
                    assert!((left.top().value - right.top().value).abs() < 2.);
                }
                "office-fidelity.docx" => {
                    let (bounds, ..) = feature(&page, "SECOND PAGE RIGHT ALIGNED");
                    assert!((bounds.right().value - 540.).abs() < 3.);
                    assert!(!text.contains("OTTRIN DOCUMENT FIDELITY"));
                }
                "office-fidelity.xlsx" => {
                    assert!(
                        page.width().value > page.height().value,
                        "landscape print layout"
                    );
                    assert!(
                        text.contains("4,000.00"),
                        "calculated, formatted sum: {text}"
                    );
                    assert!(text.contains("Amounts by item"), "chart title: {text}");
                    let title = feature(&page, "OTTRIN SHEET FIDELITY");
                    assert!(title.2.red() > 240 && title.2.green() > 240 && title.2.blue() > 240);
                    let alpha = feature(&page, "Alpha").0;
                    let beta = feature(&page, "Beta").0;
                    assert!(alpha.bottom().value > beta.top().value, "table row order");
                }
                "office-fidelity.pptx" => {
                    assert!((page.width().value - 720.).abs() < 1.);
                    assert!((page.height().value - 405.).abs() < 1.);
                    let (bounds, size, color, _) =
                        feature(&page, &format!("OTTRIN SLIDE {} FIDELITY", index + 1));
                    assert!(((bounds.left().value + bounds.right().value) / 2. - 360.).abs() < 3.);
                    assert!((size - 24.).abs() < 1.);
                    assert!(color.blue() > 140 && color.red() < 60);
                    if index == 0 {
                        assert!(text.contains("Green shape with text"));
                    } else {
                        assert!(text.contains("Second slide stays separate and ordered"));
                    }
                }
                _ => unreachable!(),
            }
            if index == 0 && name != "office-fidelity.xlsx" {
                assert!(
                    image
                        .pixels()
                        .filter(|p| p[0] > 210 && p[1] > 80 && p[1] < 160 && p[2] < 70)
                        .count()
                        > 1000,
                    "{name}: orange embedded image half"
                );
                assert!(
                    image
                        .pixels()
                        .filter(|p| p[0] < 30 && p[1] > 150 && p[2] > 190)
                        .count()
                        > 1000,
                    "{name}: cyan embedded image half"
                );
            }
            if index == 0 && name != "office-fidelity.docx" {
                assert!(
                    image
                        .pixels()
                        .filter(|p| p[0] < 80 && p[1] > 120 && p[1] < 190 && p[2] < 130)
                        .count()
                        > 1000,
                    "{name}: green chart bars or slide shape"
                );
            }
        }
        drop(pdf);
        assert_eq!(
            std::fs::read(&source).unwrap(),
            original,
            "{name}: changed source"
        );
        assert_eq!(
            std::fs::read_dir(&output).unwrap().count(),
            1,
            "{name}: private state leak"
        );
        println!("accepted representative layout {name}");
    }
    root.close().unwrap();
}
