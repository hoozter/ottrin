use lopdf::dictionary;
#[cfg(target_os = "linux")]
use ottrin_preview::LinuxBubblewrapIsolation;
use ottrin_preview::{
    InstalledLibreOfficeConverter, IsolatedProcessFailure, IsolatedProcessInput,
    IsolatedProcessLauncher, IsolatedProcessOutput, IsolatedProcessRequest, OfficeConversionOutput,
    OfficeConverterLauncher, OfficeFormat, OfficeProviderCapability, OfficeProviderId,
    OfficeProviderProbeResult, OfficeProviderUnavailableReason, OfficeRenderFailure,
    OfficeRenderPolicy, OfficeRenderRequest, detect_installed_libreoffice_from_candidates,
    extract_legacy_doc_text_at, installed_libreoffice_candidates, office_format_for_path,
    probe_installed_libreoffice_at, render_legacy_doc_pdf_with_generation_at, render_office_pdf,
    render_office_preview_page,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ottrin-office-provider-{}-{unique}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temporary directory");
    path
}

fn write_one_page_pdf(path: &Path) {
    use lopdf::{Document, Object, Stream, dictionary};

    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {},
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    document.compress();
    document.save(path).expect("write PDF fixture");
}

fn write_encrypted_marker_pdf(path: &Path) {
    write_one_page_pdf(path);
    let mut document = lopdf::Document::load(path).expect("load PDF fixture");
    let encryption_id = document.add_object(lopdf::dictionary! {
        "Filter" => "Standard",
        "V" => 1,
        "R" => 2,
        "Length" => 40,
        "O" => lopdf::Object::string_literal("owner"),
        "U" => lopdf::Object::string_literal("user"),
        "P" => -4,
    });
    document.trailer.set("Encrypt", encryption_id);
    document.save(path).expect("write encrypted PDF fixture");
}

fn capability(id: OfficeProviderId, version: &str) -> OfficeProviderCapability {
    OfficeProviderCapability {
        id,
        version: version.to_string(),
        formats: vec![OfficeFormat::Docx],
    }
}

#[test]
fn production_provider_format_mapping_accepts_only_the_bounded_office_contract() {
    assert_eq!(
        office_format_for_path(Path::new("report.DOCX")),
        Some(OfficeFormat::Docx)
    );
    assert_eq!(
        office_format_for_path(Path::new("budget.xlsm")),
        Some(OfficeFormat::Xlsm)
    );
    assert_eq!(
        office_format_for_path(Path::new("slides.odp")),
        Some(OfficeFormat::Odp)
    );
    assert_eq!(
        office_format_for_path(Path::new("legacy.doc")),
        Some(OfficeFormat::Doc)
    );
    assert_eq!(
        office_format_for_path(Path::new("legacy.xls")),
        Some(OfficeFormat::Xls)
    );
    assert_eq!(office_format_for_path(Path::new("legacy.ppt")), None);
}

#[test]
fn installed_libreoffice_probe_uses_absolute_executable_and_exposes_supported_formats() {
    struct ProbeLauncher {
        requests: Mutex<Vec<IsolatedProcessRequest>>,
    }

    impl IsolatedProcessLauncher for ProbeLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            Ok(IsolatedProcessOutput {
                stdout: b"LibreOffice 24.2.7.2 420(Build:2)\n".to_vec(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("libreoffice-probe");
    let executable = root.join("soffice");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    let launcher = ProbeLauncher {
        requests: Mutex::new(Vec::new()),
    };

    let result = probe_installed_libreoffice_at(&executable, &launcher, &AtomicBool::new(false));

    let capability = result.capability().expect("probe should expose capability");
    assert_eq!(capability.id, OfficeProviderId::InstalledLibreOffice);
    assert_eq!(capability.version, "24.2.7.2");
    assert_eq!(
        capability.formats,
        vec![
            OfficeFormat::Doc,
            OfficeFormat::Docx,
            OfficeFormat::Xls,
            OfficeFormat::Xlsx,
            OfficeFormat::Xlsm,
            OfficeFormat::Pptx,
            OfficeFormat::Odt,
            OfficeFormat::Ods,
            OfficeFormat::Odp,
        ]
    );
    let requests = launcher.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 1);
    #[cfg(not(target_os = "windows"))]
    assert_eq!(requests[0].executable, executable);
    #[cfg(target_os = "windows")]
    assert_eq!(
        requests[0].executable,
        requests[0].private_profile_dir.join("office-render.exe")
    );
    #[cfg(not(target_os = "windows"))]
    assert_eq!(requests[0].arguments, vec!["--version"]);
    #[cfg(target_os = "windows")]
    assert_eq!(
        requests[0].arguments,
        vec![
            std::ffi::OsString::from("--version"),
            executable.as_os_str().to_owned()
        ]
    );
    assert!(requests[0].input.is_none());
    assert!(requests[0].executable.is_absolute());

    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn installed_libreoffice_detection_skips_missing_candidates_and_returns_canonical_executable() {
    struct DetectionLauncher;

    impl IsolatedProcessLauncher for DetectionLauncher {
        fn run(
            &self,
            _request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            Ok(IsolatedProcessOutput {
                stdout: b"LibreOffice 24.2.7.2\n".to_vec(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("libreoffice-detection");
    let executable = root.join("program/soffice.bin");
    std::fs::create_dir_all(executable.parent().expect("executable parent"))
        .expect("create executable parent");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("mark executable fixture executable");
    }
    #[cfg(unix)]
    let alias = root.join("soffice");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&executable, &alias).expect("create executable symlink");
    #[cfg(not(unix))]
    let alias = root.join("program/../program/soffice.bin");

    let detected = detect_installed_libreoffice_from_candidates(
        &[root.join("missing"), alias],
        &DetectionLauncher,
        &AtomicBool::new(false),
    )
    .expect("healthy candidate should be detected");

    assert_eq!(detected.executable, executable.canonicalize().unwrap());
    assert_eq!(detected.capability.version, "24.2.7.2");
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn installed_libreoffice_candidates_include_both_launch_names_from_each_search_directory() {
    let root = temp_dir("libreoffice-search-path");
    let primary = root.join("primary");
    let secondary = root.join("secondary");
    let search_path = std::env::join_paths([&primary, &secondary]).expect("construct search path");

    let candidates = installed_libreoffice_candidates(Some(&search_path));

    #[cfg(target_os = "windows")]
    let names = ["soffice.com", "soffice.exe"];
    #[cfg(not(target_os = "windows"))]
    let names = ["libreoffice", "soffice"];
    for directory in [&primary, &secondary] {
        for name in names {
            assert!(candidates.contains(&directory.join(name)));
        }
    }
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn legacy_doc_extraction_is_bounded_isolated_and_sanitized() {
    struct ExtractionLauncher {
        requests: Mutex<Vec<IsolatedProcessRequest>>,
    }

    impl IsolatedProcessLauncher for ExtractionLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            Ok(IsolatedProcessOutput {
                stdout: b"Hello\r\nWorld\0\t!\n".to_vec(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("legacy-doc-extraction");
    let executable = root.join("catdoc");
    let source = root.join("document.doc");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    std::fs::write(&source, b"fixture document").expect("write source fixture");
    let launcher = ExtractionLauncher {
        requests: Mutex::new(Vec::new()),
    };

    let text = extract_legacy_doc_text_at(&source, &executable, &launcher, &AtomicBool::new(false))
        .expect("bounded extracted text");

    assert_eq!(text, "Hello\nWorld\t!");
    let requests = launcher.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.executable, executable);
    assert_eq!(request.arguments, vec!["/input/document.doc"]);
    assert_eq!(
        request.input,
        Some(IsolatedProcessInput {
            host_path: source.clone(),
            sandbox_path: PathBuf::from("/input/document.doc"),
        })
    );
    assert_eq!(request.timeout, Duration::from_secs(5));
    assert_eq!(request.max_memory_bytes, 128 * 1024 * 1024);
    assert_eq!(request.max_processes, 4);
    assert_eq!(request.max_output_bytes, 64 * 1024);
    assert!(!request.private_profile_dir.exists());
    assert!(!request.private_output_dir.exists());

    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn native_legacy_doc_rendering_uses_an_isolated_helper_and_rejects_oversized_pdf_output() {
    struct RenderLauncher {
        requests: Mutex<Vec<IsolatedProcessRequest>>,
        oversized: bool,
    }

    impl IsolatedProcessLauncher for RenderLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            let output = request.private_output_dir.join("rendered.pdf");
            if self.oversized {
                let file = std::fs::File::create(&output).expect("create oversized PDF fixture");
                file.set_len(128 * 1024 * 1024 + 1)
                    .expect("size oversized PDF fixture");
            } else {
                write_one_page_pdf(&output);
            }
            Ok(IsolatedProcessOutput {
                stdout: b"1\n".to_vec(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("native-legacy-doc-rendering");
    let executable = root.join("ottrin-legacy-doc-render");
    let source = root.join("document.doc");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    std::fs::write(&source, b"fixture document").expect("write source fixture");
    let launcher = RenderLauncher {
        requests: Mutex::new(Vec::new()),
        oversized: false,
    };

    let rendered = render_legacy_doc_pdf_with_generation_at(
        &source,
        &executable,
        &launcher,
        &AtomicBool::new(false),
    )
    .expect("isolated render should succeed");
    assert_eq!(rendered.page_count, 1);
    let requests = launcher.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.executable, executable);
    assert_eq!(
        request.arguments,
        vec![
            "--input",
            "/input/document.doc",
            "--output",
            "/output/rendered.pdf"
        ]
    );
    assert_eq!(request.timeout, Duration::from_secs(10));
    assert_eq!(request.max_memory_bytes, 512 * 1024 * 1024);
    assert_eq!(request.max_processes, 4);
    assert_eq!(request.max_output_bytes, 128 * 1024 * 1024 + 1);
    assert!(!request.private_profile_dir.exists());
    assert!(!request.private_output_dir.exists());
    drop(requests);

    let oversized = RenderLauncher {
        requests: Mutex::new(Vec::new()),
        oversized: true,
    };
    let error = render_legacy_doc_pdf_with_generation_at(
        &source,
        &executable,
        &oversized,
        &AtomicBool::new(false),
    )
    .expect_err("oversized child output must be rejected");
    assert!(matches!(
        error,
        ottrin_preview::OfficeVisualError::RenderedOutputTooLarge { .. }
    ));

    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn installed_libreoffice_conversion_uses_disposable_profile_and_private_pdf_output() {
    struct ConversionLauncher {
        requests: Mutex<Vec<IsolatedProcessRequest>>,
    }

    impl IsolatedProcessLauncher for ConversionLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            let pdf_name = Path::new("source.pdf");
            write_one_page_pdf(&request.private_output_dir.join(pdf_name));
            Ok(IsolatedProcessOutput {
                stdout: Vec::new(),
                diagnostics: b"convert /input/source.docx as pdf".to_vec(),
            })
        }
    }

    let root = temp_dir("libreoffice-conversion");
    let executable = root.join("soffice");
    let source = root.join("project brief.docx");
    let output = root.join("output");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    std::fs::write(&source, b"fixture document").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output");
    let process = ConversionLauncher {
        requests: Mutex::new(Vec::new()),
    };
    let converter = InstalledLibreOfficeConverter::new(executable.clone(), &process);
    let request = OfficeRenderRequest {
        source_path: source.clone(),
        format: OfficeFormat::Docx,
        private_output_dir: output.clone(),
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_secs(2),
        },
    };

    let converted = converter
        .convert(
            &capability(OfficeProviderId::InstalledLibreOffice, "24.2.7.2"),
            &request,
            &AtomicBool::new(false),
        )
        .expect("conversion should succeed");

    assert_eq!(converted.pdf_path, output.join("source.pdf"));
    assert!(converted.pdf_path.is_file());
    assert_eq!(converted.diagnostics, "convert /input/source.docx as pdf");
    let requests = process.requests.lock().expect("requests lock");
    assert_eq!(requests.len(), 1);
    #[cfg(not(target_os = "windows"))]
    assert_eq!(requests[0].executable, executable);
    #[cfg(target_os = "windows")]
    assert_eq!(
        requests[0].executable,
        requests[0].private_profile_dir.join("office-render.exe")
    );
    assert_eq!(
        requests[0]
            .input
            .as_ref()
            .map(|input| input.host_path.as_path()),
        Some(source.as_path())
    );
    assert_eq!(
        requests[0]
            .input
            .as_ref()
            .map(|input| input.sandbox_path.as_path()),
        Some(Path::new("/input/source.docx"))
    );
    assert!(requests[0].arguments.iter().any(|argument| {
        argument
            .to_string_lossy()
            .starts_with("-env:UserInstallation=file:///profile")
    }));
    assert_eq!(requests[0].timeout, request.policy.timeout);
    assert!(!requests[0].private_profile_dir.exists());

    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn installed_libreoffice_conversion_rejects_a_changed_source_generation() {
    struct MutatingLauncher;

    impl IsolatedProcessLauncher for MutatingLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            let source = &request.input.as_ref().expect("mapped input").host_path;
            std::fs::write(source, b"changed while provider was running")
                .expect("mutate source fixture");
            write_one_page_pdf(&request.private_output_dir.join("source.pdf"));
            Ok(IsolatedProcessOutput {
                stdout: Vec::new(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("libreoffice-source-generation");
    let executable = root.join("soffice");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    std::fs::write(&source, b"original document").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output");
    let converter = InstalledLibreOfficeConverter::new(executable, &MutatingLauncher);
    let request = OfficeRenderRequest {
        source_path: source,
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_secs(2),
        },
    };

    let failure = converter
        .convert(
            &capability(OfficeProviderId::InstalledLibreOffice, "24.2.7.2"),
            &request,
            &AtomicBool::new(false),
        )
        .expect_err("changed source must invalidate provider output");

    assert_eq!(failure, OfficeRenderFailure::InvalidOutput);
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn installed_libreoffice_conversion_rejects_changed_source_metadata() {
    struct MetadataMutatingLauncher;

    impl IsolatedProcessLauncher for MetadataMutatingLauncher {
        fn run(
            &self,
            request: &IsolatedProcessRequest,
            _cancelled: &AtomicBool,
        ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
            let source = &request.input.as_ref().expect("mapped input").host_path;
            let mut permissions = std::fs::metadata(source)
                .expect("source metadata")
                .permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(source, permissions).expect("change source metadata");
            write_one_page_pdf(&request.private_output_dir.join("source.pdf"));
            Ok(IsolatedProcessOutput {
                stdout: Vec::new(),
                diagnostics: Vec::new(),
            })
        }
    }

    let root = temp_dir("libreoffice-source-metadata");
    let executable = root.join("soffice");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&executable, b"fixture executable").expect("write executable fixture");
    std::fs::write(&source, b"original document").expect("write source fixture");
    let original_permissions = std::fs::metadata(&source)
        .expect("source metadata")
        .permissions();
    std::fs::create_dir(&output).expect("create private output");
    let converter = InstalledLibreOfficeConverter::new(executable, &MetadataMutatingLauncher);
    let request = OfficeRenderRequest {
        source_path: source.clone(),
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_secs(2),
        },
    };

    let failure = converter
        .convert(
            &capability(OfficeProviderId::InstalledLibreOffice, "24.2.7.2"),
            &request,
            &AtomicBool::new(false),
        )
        .expect_err("changed source metadata must invalidate provider output");

    assert_eq!(failure, OfficeRenderFailure::InvalidOutput);
    std::fs::set_permissions(&source, original_permissions).expect("restore source permissions");
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_process_boundary_exposes_only_mapped_input_profile_output_and_sanitized_environment() {
    let root = temp_dir("linux-isolation");
    let script = root.join("check.sh");
    let profile = root.join("profile");
    let output = root.join("output");
    std::fs::create_dir(&profile).expect("create profile");
    std::fs::create_dir(&output).expect("create output");
    std::fs::write(
        &script,
        b"set -eu\ntest \"$HOME\" = /profile\ntest ! -e /home/campbell\ntest \"$(cat /input/check.sh | wc -c)\" -gt 0\nprintf isolated > /output/result\nprintf boundary-ok\n",
    )
    .expect("write script");
    let request = IsolatedProcessRequest {
        executable: PathBuf::from("/usr/bin/sh"),
        arguments: vec!["/input/check.sh".into()],
        input: Some(IsolatedProcessInput {
            host_path: script,
            sandbox_path: PathBuf::from("/input/check.sh"),
        }),
        private_profile_dir: profile,
        private_output_dir: output.clone(),
        timeout: Duration::from_secs(2),
        max_memory_bytes: 256 * 1024 * 1024,
        max_processes: 8,
        max_output_bytes: 4096,
    };

    let result = LinuxBubblewrapIsolation
        .run(&request, &AtomicBool::new(false))
        .expect("isolated process should succeed");

    assert_eq!(result.stdout, b"boundary-ok");
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        std::fs::read_to_string(output.join("result")).expect("read isolated output"),
        "isolated"
    );
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[cfg(target_os = "linux")]
#[test]
fn linux_process_boundary_timeout_kills_descendants_and_removes_controller_logs() {
    let root = temp_dir("linux-timeout-cleanup");
    let script = root.join("timeout.sh");
    let profile = root.join("profile");
    let output = root.join("output");
    std::fs::create_dir(&profile).expect("create profile");
    std::fs::create_dir(&output).expect("create output");
    std::fs::write(
        &script,
        b"(sleep 0.5; printf escaped > /output/late) &\nwait\n",
    )
    .expect("write timeout script");
    let request = IsolatedProcessRequest {
        executable: PathBuf::from("/usr/bin/sh"),
        arguments: vec!["/input/timeout.sh".into()],
        input: Some(IsolatedProcessInput {
            host_path: script,
            sandbox_path: PathBuf::from("/input/timeout.sh"),
        }),
        private_profile_dir: profile.clone(),
        private_output_dir: output.clone(),
        timeout: Duration::from_millis(100),
        max_memory_bytes: 256 * 1024 * 1024,
        max_processes: 8,
        max_output_bytes: 4096,
    };

    let failure = LinuxBubblewrapIsolation
        .run(&request, &AtomicBool::new(false))
        .expect_err("long-running process should time out");

    assert_eq!(failure, IsolatedProcessFailure::Timeout);
    assert_eq!(
        std::fs::read_dir(&profile).expect("read profile").count(),
        0,
        "controller logs must be removed on timeout"
    );
    std::thread::sleep(Duration::from_millis(600));
    assert!(
        !output.join("late").exists(),
        "descendant escaped timeout cleanup"
    );
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

struct FallbackLauncher {
    calls: Mutex<Vec<OfficeProviderId>>,
}

impl OfficeConverterLauncher for FallbackLauncher {
    fn convert(
        &self,
        provider: &OfficeProviderCapability,
        request: &OfficeRenderRequest,
        _cancelled: &AtomicBool,
    ) -> Result<OfficeConversionOutput, OfficeRenderFailure> {
        self.calls.lock().expect("calls lock").push(provider.id);
        match provider.id {
            OfficeProviderId::InstalledLibreOffice => Err(OfficeRenderFailure::Unsupported),
            OfficeProviderId::LightweightPack => {
                let pdf_path = request.private_output_dir.join("converted.pdf");
                write_one_page_pdf(&pdf_path);
                Ok(OfficeConversionOutput {
                    pdf_path,
                    diagnostics: "fixture conversion".to_string(),
                })
            }
            OfficeProviderId::FidelityPack => panic!("lower-priority provider must not run"),
        }
    }
}

#[test]
fn provider_probe_exposes_capability_only_when_available() {
    let available = OfficeProviderProbeResult::Available(capability(
        OfficeProviderId::InstalledLibreOffice,
        "26.2.5",
    ));
    let unavailable = OfficeProviderProbeResult::Unavailable {
        id: OfficeProviderId::LightweightPack,
        reason: OfficeProviderUnavailableReason::NotInstalled,
    };

    assert_eq!(
        available.capability().map(|provider| provider.id),
        Some(OfficeProviderId::InstalledLibreOffice)
    );
    assert_eq!(unavailable.capability(), None);
}

#[test]
fn cancelled_office_rendering_stops_before_request_or_provider_work() {
    struct PanicLauncher;

    impl OfficeConverterLauncher for PanicLauncher {
        fn convert(
            &self,
            _provider: &OfficeProviderCapability,
            _request: &OfficeRenderRequest,
            _cancelled: &AtomicBool,
        ) -> Result<OfficeConversionOutput, OfficeRenderFailure> {
            panic!("cancelled rendering must not launch a provider");
        }
    }

    let cancelled = AtomicBool::new(true);
    let request = OfficeRenderRequest {
        source_path: PathBuf::from("/missing/cancelled.docx"),
        format: OfficeFormat::Docx,
        private_output_dir: PathBuf::from("/missing/output"),
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1,
            max_output_bytes: 1,
            max_pages: 1,
            timeout: Duration::from_millis(1),
        },
    };

    let error = render_office_pdf(&[], &request, &PanicLauncher, &cancelled)
        .expect_err("pre-cancelled rendering must stop");

    assert_eq!(error, ottrin_preview::OfficeRenderError::Cancelled);
}

#[test]
fn provider_timeout_is_typed_and_falls_back_without_losing_attempt_evidence() {
    struct TimeoutThenValidLauncher;

    impl OfficeConverterLauncher for TimeoutThenValidLauncher {
        fn convert(
            &self,
            provider: &OfficeProviderCapability,
            request: &OfficeRenderRequest,
            _cancelled: &AtomicBool,
        ) -> Result<OfficeConversionOutput, OfficeRenderFailure> {
            if provider.id == OfficeProviderId::InstalledLibreOffice {
                return Err(OfficeRenderFailure::Timeout);
            }
            let pdf_path = request.private_output_dir.join("after-timeout.pdf");
            write_one_page_pdf(&pdf_path);
            Ok(OfficeConversionOutput {
                pdf_path,
                diagnostics: String::new(),
            })
        }
    }

    let root = temp_dir("timeout-fallback");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&source, b"bounded source fixture").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output directory");
    let request = OfficeRenderRequest {
        source_path: source,
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_millis(10),
        },
    };

    let rendered = render_office_pdf(
        &[
            capability(OfficeProviderId::InstalledLibreOffice, "26.2.5"),
            capability(OfficeProviderId::LightweightPack, "0.38.4"),
        ],
        &request,
        &TimeoutThenValidLauncher,
        &AtomicBool::new(false),
    )
    .expect("timeout should fall back");

    assert_eq!(rendered.provider.id, OfficeProviderId::LightweightPack);
    assert_eq!(rendered.fallback_failures.len(), 1);
    assert_eq!(
        rendered.fallback_failures[0].failure,
        OfficeRenderFailure::Timeout
    );
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn provider_output_rejects_encrypted_pdf_before_falling_back() {
    struct InvalidThenValidLauncher;

    impl OfficeConverterLauncher for InvalidThenValidLauncher {
        fn convert(
            &self,
            provider: &OfficeProviderCapability,
            request: &OfficeRenderRequest,
            _cancelled: &AtomicBool,
        ) -> Result<OfficeConversionOutput, OfficeRenderFailure> {
            let pdf_path = match provider.id {
                OfficeProviderId::InstalledLibreOffice => {
                    let path = request.private_output_dir.join("encrypted.pdf");
                    write_encrypted_marker_pdf(&path);
                    path
                }
                OfficeProviderId::LightweightPack => {
                    let path = request.private_output_dir.join("valid.pdf");
                    write_one_page_pdf(&path);
                    path
                }
                OfficeProviderId::FidelityPack => panic!("lower-priority provider must not run"),
            };
            Ok(OfficeConversionOutput {
                pdf_path,
                diagnostics: String::new(),
            })
        }
    }

    let root = temp_dir("encrypted-output");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&source, b"bounded source fixture").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output directory");
    let request = OfficeRenderRequest {
        source_path: source,
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy: OfficeRenderPolicy {
            version: 1,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_secs(2),
        },
    };

    let rendered = render_office_pdf(
        &[
            capability(OfficeProviderId::InstalledLibreOffice, "26.2.5"),
            capability(OfficeProviderId::LightweightPack, "0.38.4"),
        ],
        &request,
        &InvalidThenValidLauncher,
        &AtomicBool::new(false),
    )
    .expect("encrypted output should fall back");

    assert_eq!(rendered.provider.id, OfficeProviderId::LightweightPack);
    assert_eq!(rendered.pdf_path.file_name().unwrap(), "valid.pdf");
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn office_rendering_uses_deterministic_fallback_order_and_provider_cache_identity() {
    let root = temp_dir("fallback-order");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&source, b"bounded source fixture").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output directory");
    let launcher = FallbackLauncher {
        calls: Mutex::new(Vec::new()),
    };
    let policy = OfficeRenderPolicy {
        version: 7,
        max_input_bytes: 1024,
        max_output_bytes: 4096,
        max_pages: 4,
        timeout: Duration::from_secs(2),
    };
    let request = OfficeRenderRequest {
        source_path: source,
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy,
    };
    let providers = vec![
        capability(OfficeProviderId::FidelityPack, "26.2.5"),
        capability(OfficeProviderId::LightweightPack, "0.38.4"),
        capability(OfficeProviderId::InstalledLibreOffice, "26.2.5"),
    ];

    let rendered = render_office_pdf(&providers, &request, &launcher, &AtomicBool::new(false))
        .expect("fallback provider should render");

    assert_eq!(
        *launcher.calls.lock().expect("calls lock"),
        vec![
            OfficeProviderId::InstalledLibreOffice,
            OfficeProviderId::LightweightPack,
        ]
    );
    assert_eq!(rendered.provider.id, OfficeProviderId::LightweightPack);
    assert_eq!(rendered.provider.version, "0.38.4");
    assert_eq!(rendered.page_count, 1);
    assert_eq!(
        rendered.cache_identity.provider,
        OfficeProviderId::LightweightPack
    );
    assert_eq!(rendered.cache_identity.provider_version, "0.38.4");
    assert_eq!(rendered.cache_identity.policy_version, 7);

    std::fs::remove_dir_all(root).expect("remove temporary directory");
}

#[test]
fn production_page_orchestration_renders_only_the_validated_provider_pdf_and_reports_provenance() {
    let root = temp_dir("production-page");
    let source = root.join("source.docx");
    let output = root.join("output");
    std::fs::write(&source, b"bounded source fixture").expect("write source fixture");
    std::fs::create_dir(&output).expect("create private output directory");
    let request = OfficeRenderRequest {
        source_path: source,
        format: OfficeFormat::Docx,
        private_output_dir: output,
        policy: OfficeRenderPolicy {
            version: 9,
            max_input_bytes: 1024,
            max_output_bytes: 4096,
            max_pages: 4,
            timeout: Duration::from_secs(2),
        },
    };
    let launcher = FallbackLauncher {
        calls: Mutex::new(Vec::new()),
    };
    let rendered_path = Mutex::new(None);

    let page = render_office_preview_page(
        &[capability(OfficeProviderId::LightweightPack, "0.38.4")],
        &request,
        &launcher,
        0,
        1280,
        &AtomicBool::new(false),
        |pdf_path, page_index, target_dim| {
            *rendered_path.lock().expect("rendered path lock") = Some(pdf_path.to_path_buf());
            assert_eq!(page_index, 0);
            assert_eq!(target_dim, 1280);
            Ok::<_, String>(b"validated PDF pixels".to_vec())
        },
    )
    .expect("validated provider PDF should render");

    assert_eq!(page.png, b"validated PDF pixels");
    assert_eq!(page.page_count, 1);
    assert_eq!(
        page.provider_label,
        "Rendered with MiniPdf 0.38.4 — lightweight preview"
    );
    assert_eq!(page.cache_identity.policy_version, 9);
    assert_eq!(
        *rendered_path.lock().expect("rendered path lock"),
        Some(page.validated_pdf_path.clone())
    );
    std::fs::remove_dir_all(root).expect("remove temporary directory");
}
