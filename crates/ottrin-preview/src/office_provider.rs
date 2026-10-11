use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const MAX_LEGACY_DOC_INPUT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LEGACY_DOC_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_LEGACY_DOC_TEXT_CHARS: usize = 24_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedProcessInput {
    pub host_path: PathBuf,
    pub sandbox_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedProcessRequest {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub input: Option<IsolatedProcessInput>,
    pub private_profile_dir: PathBuf,
    pub private_output_dir: PathBuf,
    pub timeout: Duration,
    pub max_memory_bytes: u64,
    pub max_processes: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedProcessOutput {
    pub stdout: Vec<u8>,
    pub diagnostics: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolatedProcessFailure {
    Unavailable,
    InvalidRequest,
    LimitExceeded,
    Timeout,
    Crash,
    Cancelled,
}

pub trait IsolatedProcessLauncher {
    fn run(
        &self,
        request: &IsolatedProcessRequest,
        cancelled: &AtomicBool,
    ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfficeProviderId {
    InstalledLibreOffice,
    LightweightPack,
    FidelityPack,
}

impl OfficeProviderId {
    fn fallback_rank(self) -> u8 {
        match self {
            Self::InstalledLibreOffice => 0,
            Self::LightweightPack => 1,
            Self::FidelityPack => 2,
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::InstalledLibreOffice => "LibreOffice",
            Self::LightweightPack => "MiniPdf",
            Self::FidelityPack => "LibreOffice",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OfficeFormat {
    Doc,
    Docx,
    Xls,
    Xlsx,
    Xlsm,
    Pptx,
    Odt,
    Ods,
    Odp,
}

pub fn office_format_for_path(path: &Path) -> Option<OfficeFormat> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "doc" => Some(OfficeFormat::Doc),
        "docx" => Some(OfficeFormat::Docx),
        "xls" => Some(OfficeFormat::Xls),
        "xlsx" => Some(OfficeFormat::Xlsx),
        "xlsm" => Some(OfficeFormat::Xlsm),
        "pptx" => Some(OfficeFormat::Pptx),
        "odt" => Some(OfficeFormat::Odt),
        "ods" => Some(OfficeFormat::Ods),
        "odp" => Some(OfficeFormat::Odp),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeProviderCapability {
    pub id: OfficeProviderId,
    pub version: String,
    pub formats: Vec<OfficeFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeProviderUnavailableReason {
    NotInstalled,
    Disabled,
    IncompatibleVersion,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeProviderProbeResult {
    Available(OfficeProviderCapability),
    Unavailable {
        id: OfficeProviderId,
        reason: OfficeProviderUnavailableReason,
    },
}

impl OfficeProviderProbeResult {
    pub fn capability(&self) -> Option<&OfficeProviderCapability> {
        match self {
            Self::Available(capability) => Some(capability),
            Self::Unavailable { .. } => None,
        }
    }
}

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn extract_legacy_doc_text_at(
    source: &Path,
    executable: &Path,
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> Option<String> {
    let source_metadata = std::fs::symlink_metadata(source).ok()?;
    let executable_metadata = std::fs::symlink_metadata(executable).ok()?;
    if !source_metadata.file_type().is_file()
        || source_metadata.len() > MAX_LEGACY_DOC_INPUT_BYTES
        || !executable.is_absolute()
        || !executable_metadata.file_type().is_file()
    {
        return None;
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ottrin-legacy-doc-{}-{unique}", std::process::id()));
    let profile = root.join("profile");
    let output = root.join("output");
    if std::fs::create_dir(&root).is_err()
        || std::fs::create_dir(&profile).is_err()
        || std::fs::create_dir(&output).is_err()
    {
        let _ = std::fs::remove_dir_all(root);
        return None;
    }
    let _cleanup = ProbeDirectory(root);
    let result = launcher
        .run(
            &IsolatedProcessRequest {
                executable: executable.to_path_buf(),
                arguments: vec![OsString::from("/input/document.doc")],
                input: Some(IsolatedProcessInput {
                    host_path: source.to_path_buf(),
                    sandbox_path: PathBuf::from("/input/document.doc"),
                }),
                private_profile_dir: profile,
                private_output_dir: output,
                timeout: Duration::from_secs(5),
                max_memory_bytes: 128 * 1024 * 1024,
                max_processes: 4,
                max_output_bytes: MAX_LEGACY_DOC_OUTPUT_BYTES,
            },
            cancelled,
        )
        .ok()?;

    let decoded = String::from_utf8_lossy(&result.stdout);
    let mut text = String::new();
    let mut stored = 0_usize;
    let mut characters = decoded.chars().peekable();
    while stored < MAX_LEGACY_DOC_TEXT_CHARS
        && let Some(character) = characters.next()
    {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                text.push('\n');
                stored += 1;
            }
            '\n' | '\t' => {
                text.push(character);
                stored += 1;
            }
            character if !character.is_control() => {
                text.push(character);
                stored += 1;
            }
            _ => {}
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedInstalledLibreOffice {
    pub executable: PathBuf,
    pub capability: OfficeProviderCapability,
}

pub fn installed_libreoffice_candidates(search_path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    let mut candidates = vec![
        PathBuf::from("/usr/bin/libreoffice"),
        PathBuf::from("/usr/bin/soffice"),
        PathBuf::from("/usr/local/bin/libreoffice"),
        PathBuf::from("/usr/local/bin/soffice"),
        PathBuf::from("/opt/libreoffice/program/soffice"),
    ];
    #[cfg(target_os = "windows")]
    let mut candidates = Vec::new();
    let launch_names = if cfg!(target_os = "windows") {
        ["soffice.com", "soffice.exe"]
    } else {
        ["libreoffice", "soffice"]
    };
    #[cfg(target_os = "windows")]
    for directory in ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
    {
        let program = directory.join("LibreOffice").join("program");
        candidates.extend(launch_names.map(|name| program.join(name)));
    }
    if let Some(search_path) = search_path {
        for directory in std::env::split_paths(search_path) {
            if directory.is_absolute() {
                candidates.extend(launch_names.map(|name| directory.join(name)));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));
    candidates
}

pub fn detect_installed_libreoffice(
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> Result<DetectedInstalledLibreOffice, OfficeProviderUnavailableReason> {
    let candidates = installed_libreoffice_candidates(std::env::var_os("PATH").as_deref());
    detect_installed_libreoffice_from_candidates(&candidates, launcher, cancelled)
}

pub fn detect_installed_libreoffice_from_candidates(
    candidates: &[PathBuf],
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> Result<DetectedInstalledLibreOffice, OfficeProviderUnavailableReason> {
    let mut found_candidate = false;
    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        if cancelled.load(Ordering::Acquire) {
            return Err(OfficeProviderUnavailableReason::Unhealthy);
        }
        let Ok(executable) = candidate.canonicalize() else {
            continue;
        };
        if !seen.insert(executable.clone()) || !is_executable_file(&executable) {
            continue;
        }
        found_candidate = true;
        if let OfficeProviderProbeResult::Available(capability) =
            probe_installed_libreoffice_at(&executable, launcher, cancelled)
        {
            return Ok(DetectedInstalledLibreOffice {
                executable,
                capability,
            });
        }
    }
    Err(if found_candidate {
        OfficeProviderUnavailableReason::Unhealthy
    } else {
        OfficeProviderUnavailableReason::NotInstalled
    })
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(target_os = "windows")]
fn installed_office_helper(profile: &Path) -> Option<PathBuf> {
    let path = std::env::var_os("OTTRIN_OFFICE_HELPER")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()?
                .parent()
                .map(|directory| directory.join("ottrin-office-render.exe"))
        })?;
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    // A portable installation may not grant AppContainer executable access.
    // Copy bytes into our private profile so its existing sandbox ACL admits
    // only this helper; do not modify the installed binary's permissions.
    let staged = profile.join("office-render.exe");
    let mut input = std::fs::File::open(path).ok()?;
    let mut output = std::fs::File::create(&staged).ok()?;
    std::io::copy(&mut input, &mut output).ok()?;
    Some(staged)
}

pub fn probe_installed_libreoffice_at(
    executable: &Path,
    launcher: &dyn IsolatedProcessLauncher,
    cancelled: &AtomicBool,
) -> OfficeProviderProbeResult {
    let unavailable = || OfficeProviderProbeResult::Unavailable {
        id: OfficeProviderId::InstalledLibreOffice,
        reason: OfficeProviderUnavailableReason::Unhealthy,
    };
    if cancelled.load(Ordering::Acquire)
        || !executable.is_absolute()
        || std::fs::symlink_metadata(executable)
            .map(|metadata| !metadata.file_type().is_file())
            .unwrap_or(true)
    {
        return unavailable();
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "ottrin-libreoffice-probe-{}-{unique}",
        std::process::id()
    ));
    let profile = root.join("profile");
    let output = root.join("output");
    if std::fs::create_dir(&root).is_err()
        || std::fs::create_dir(&profile).is_err()
        || std::fs::create_dir(&output).is_err()
    {
        let _ = std::fs::remove_dir_all(root);
        return unavailable();
    }
    let _cleanup = ProbeDirectory(root);
    #[cfg(target_os = "windows")]
    let Some(process_executable) = installed_office_helper(&profile) else {
        return unavailable();
    };
    #[cfg(not(target_os = "windows"))]
    let process_executable = executable.to_path_buf();
    #[cfg(target_os = "windows")]
    let arguments = vec![
        OsString::from("--version"),
        executable.as_os_str().to_owned(),
    ];
    #[cfg(not(target_os = "windows"))]
    let arguments = vec![OsString::from("--version")];
    let request = IsolatedProcessRequest {
        executable: process_executable,
        arguments,
        input: None,
        private_profile_dir: profile,
        private_output_dir: output,
        timeout: Duration::from_secs(3),
        max_memory_bytes: 512 * 1024 * 1024,
        max_processes: 32,
        max_output_bytes: 64 * 1024,
    };
    let Ok(result) = launcher.run(&request, cancelled) else {
        return unavailable();
    };
    let Ok(stdout) = std::str::from_utf8(&result.stdout) else {
        return unavailable();
    };
    let Some(version) = stdout
        .split_whitespace()
        .skip_while(|part| !part.eq_ignore_ascii_case("LibreOffice"))
        .nth(1)
        .filter(|version| version.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
    else {
        return unavailable();
    };
    OfficeProviderProbeResult::Available(OfficeProviderCapability {
        id: OfficeProviderId::InstalledLibreOffice,
        version: version.to_string(),
        formats: vec![
            OfficeFormat::Doc,
            OfficeFormat::Docx,
            OfficeFormat::Xls,
            OfficeFormat::Xlsx,
            OfficeFormat::Xlsm,
            OfficeFormat::Pptx,
            OfficeFormat::Odt,
            OfficeFormat::Ods,
            OfficeFormat::Odp,
        ],
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeRenderPolicy {
    pub version: u32,
    pub max_input_bytes: u64,
    pub max_output_bytes: u64,
    pub max_pages: usize,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeRenderRequest {
    pub source_path: PathBuf,
    pub format: OfficeFormat,
    pub private_output_dir: PathBuf,
    pub policy: OfficeRenderPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeConversionOutput {
    pub pdf_path: PathBuf,
    pub diagnostics: String,
}

pub struct InstalledLibreOfficeConverter<'a> {
    executable: PathBuf,
    process_launcher: &'a dyn IsolatedProcessLauncher,
}

impl<'a> InstalledLibreOfficeConverter<'a> {
    pub fn new(executable: PathBuf, process_launcher: &'a dyn IsolatedProcessLauncher) -> Self {
        Self {
            executable,
            process_launcher,
        }
    }
}

impl OfficeConverterLauncher for InstalledLibreOfficeConverter<'_> {
    fn convert(
        &self,
        provider: &OfficeProviderCapability,
        request: &OfficeRenderRequest,
        cancelled: &AtomicBool,
    ) -> Result<OfficeConversionOutput, OfficeRenderFailure> {
        if provider.id != OfficeProviderId::InstalledLibreOffice || !self.executable.is_absolute() {
            return Err(OfficeRenderFailure::Unsupported);
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(OfficeRenderFailure::Cancelled);
        }
        let expected_source_generation =
            source_generation(&request.source_path, request.policy.max_input_bytes)?;
        let extension = match request.format {
            OfficeFormat::Doc => "doc",
            OfficeFormat::Docx => "docx",
            OfficeFormat::Xls => "xls",
            OfficeFormat::Xlsx => "xlsx",
            OfficeFormat::Xlsm => "xlsm",
            OfficeFormat::Pptx => "pptx",
            OfficeFormat::Odt => "odt",
            OfficeFormat::Ods => "ods",
            OfficeFormat::Odp => "odp",
        };
        let sandbox_input = PathBuf::from(format!("/input/source.{extension}"));
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // Leave path-length headroom for the provider's own profile files.
        let profile = request
            .private_output_dir
            .join(format!(".ottrin-{}-{unique}", std::process::id()));
        std::fs::create_dir(&profile).map_err(|_| OfficeRenderFailure::Crash)?;
        let cleanup = ProbeDirectory(profile.clone());
        #[cfg(target_os = "windows")]
        let process_executable =
            installed_office_helper(&profile).ok_or(OfficeRenderFailure::Unsupported)?;
        #[cfg(not(target_os = "windows"))]
        let process_executable = self.executable.clone();
        #[cfg(target_os = "windows")]
        let arguments = vec![
            self.executable
                .parent()
                .ok_or(OfficeRenderFailure::Unsupported)?
                .as_os_str()
                .to_owned(),
            OsString::from("-env:UserInstallation=file:///profile"),
            sandbox_input.clone().into_os_string(),
            OsString::from("/output"),
        ];
        #[cfg(not(target_os = "windows"))]
        let arguments = vec![
            OsString::from("--headless"),
            OsString::from("--nologo"),
            OsString::from("--nodefault"),
            OsString::from("--nolockcheck"),
            OsString::from("--norestore"),
            OsString::from("-env:UserInstallation=file:///profile"),
            OsString::from("--convert-to"),
            OsString::from("pdf"),
            OsString::from("--outdir"),
            OsString::from("/output"),
            sandbox_input.clone().into_os_string(),
        ];
        let process_request = IsolatedProcessRequest {
            executable: process_executable,
            arguments,
            input: Some(IsolatedProcessInput {
                host_path: request.source_path.clone(),
                sandbox_path: sandbox_input,
            }),
            private_profile_dir: profile,
            private_output_dir: request.private_output_dir.clone(),
            timeout: request.policy.timeout,
            max_memory_bytes: 1024 * 1024 * 1024,
            max_processes: 64,
            max_output_bytes: request.policy.max_output_bytes,
        };
        let result = self.process_launcher.run(&process_request, cancelled);
        drop(cleanup);
        let output = result.map_err(|failure| match failure {
            IsolatedProcessFailure::LimitExceeded => OfficeRenderFailure::LimitExceeded,
            IsolatedProcessFailure::Timeout => OfficeRenderFailure::Timeout,
            IsolatedProcessFailure::Cancelled => OfficeRenderFailure::Cancelled,
            IsolatedProcessFailure::Unavailable
            | IsolatedProcessFailure::InvalidRequest
            | IsolatedProcessFailure::Crash => OfficeRenderFailure::Crash,
        })?;
        if source_generation(&request.source_path, request.policy.max_input_bytes)?
            != expected_source_generation
        {
            return Err(OfficeRenderFailure::InvalidOutput);
        }
        Ok(OfficeConversionOutput {
            pdf_path: request.private_output_dir.join("source.pdf"),
            diagnostics: String::from_utf8_lossy(&output.diagnostics).into_owned(),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SourceGeneration {
    digest: [u8; 32],
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    read_only: bool,
}

fn source_generation(path: &Path, max_bytes: u64) -> Result<SourceGeneration, OfficeRenderFailure> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| OfficeRenderFailure::InvalidOutput)?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return Err(OfficeRenderFailure::InvalidOutput);
    }

    let mut file = std::fs::File::open(path).map_err(|_| OfficeRenderFailure::InvalidOutput)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| OfficeRenderFailure::InvalidOutput)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or(OfficeRenderFailure::InvalidOutput)?;
        if total > max_bytes {
            return Err(OfficeRenderFailure::InvalidOutput);
        }
        hasher.update(&buffer[..read]);
    }
    Ok(SourceGeneration {
        digest: hasher.finalize().into(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        read_only: metadata.permissions().readonly(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeRenderFailure {
    Unsupported,
    Encrypted,
    LimitExceeded,
    Timeout,
    Crash,
    InvalidOutput,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficeProviderAttemptFailure {
    pub provider: OfficeProviderCapability,
    pub failure: OfficeRenderFailure,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OfficeRenderError {
    #[error("Office rendering was cancelled")]
    Cancelled,
    #[error("Office render request violates its bounded policy")]
    InvalidRequest,
    #[error("no Office provider produced a valid bounded PDF")]
    NoProviderSucceeded {
        attempts: Vec<OfficeProviderAttemptFailure>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OfficeRenderCacheIdentity {
    pub provider: OfficeProviderId,
    pub provider_version: String,
    pub policy_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOfficePdf {
    pub pdf_path: PathBuf,
    pub page_count: usize,
    pub provider: OfficeProviderCapability,
    pub diagnostics: String,
    pub cache_identity: OfficeRenderCacheIdentity,
    pub fallback_failures: Vec<OfficeProviderAttemptFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOfficePreviewPage {
    pub png: Vec<u8>,
    pub validated_pdf_path: PathBuf,
    pub page_count: usize,
    pub provider_label: String,
    pub cache_identity: OfficeRenderCacheIdentity,
    pub fallback_failures: Vec<OfficeProviderAttemptFailure>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OfficePreviewPageError {
    #[error(transparent)]
    Provider(#[from] OfficeRenderError),
    #[error("Office provider page is outside the validated PDF page range")]
    PageOutOfBounds,
    #[error("validated Office provider PDF could not be rendered: {0}")]
    PdfRender(String),
}

pub trait OfficeConverterLauncher {
    fn convert(
        &self,
        provider: &OfficeProviderCapability,
        request: &OfficeRenderRequest,
        cancelled: &AtomicBool,
    ) -> Result<OfficeConversionOutput, OfficeRenderFailure>;
}

pub fn render_office_pdf(
    providers: &[OfficeProviderCapability],
    request: &OfficeRenderRequest,
    launcher: &dyn OfficeConverterLauncher,
    cancelled: &AtomicBool,
) -> Result<RenderedOfficePdf, OfficeRenderError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(OfficeRenderError::Cancelled);
    }
    validate_request(request)?;

    let mut candidates: Vec<_> = providers
        .iter()
        .filter(|provider| provider.formats.contains(&request.format))
        .collect();
    candidates.sort_by_key(|provider| provider.id.fallback_rank());

    let mut attempts = Vec::new();
    for provider in candidates {
        if cancelled.load(Ordering::Acquire) {
            return Err(OfficeRenderError::Cancelled);
        }
        let output = match launcher.convert(provider, request, cancelled) {
            Ok(output) => output,
            Err(OfficeRenderFailure::Cancelled) => return Err(OfficeRenderError::Cancelled),
            Err(failure) => {
                attempts.push(OfficeProviderAttemptFailure {
                    provider: provider.clone(),
                    failure,
                });
                continue;
            }
        };
        if cancelled.load(Ordering::Acquire) {
            return Err(OfficeRenderError::Cancelled);
        }
        match validate_pdf_output(&output.pdf_path, request) {
            Ok(page_count) => {
                return Ok(RenderedOfficePdf {
                    pdf_path: output.pdf_path,
                    page_count,
                    provider: provider.clone(),
                    diagnostics: output.diagnostics,
                    cache_identity: OfficeRenderCacheIdentity {
                        provider: provider.id,
                        provider_version: provider.version.clone(),
                        policy_version: request.policy.version,
                    },
                    fallback_failures: attempts,
                });
            }
            Err(()) => attempts.push(OfficeProviderAttemptFailure {
                provider: provider.clone(),
                failure: OfficeRenderFailure::InvalidOutput,
            }),
        }
    }

    Err(OfficeRenderError::NoProviderSucceeded { attempts })
}

/// Converts through the provider contract, then gives only its admitted PDF to
/// the existing PDF page renderer. The caller owns the private output directory
/// and may remove it as soon as the returned PNG has been admitted to cache.
pub fn render_office_preview_page(
    providers: &[OfficeProviderCapability],
    request: &OfficeRenderRequest,
    launcher: &dyn OfficeConverterLauncher,
    page_index: usize,
    target_dim: u32,
    cancelled: &AtomicBool,
    render_pdf_page: impl FnOnce(&Path, usize, u32) -> Result<Vec<u8>, String>,
) -> Result<RenderedOfficePreviewPage, OfficePreviewPageError> {
    let rendered = render_office_pdf(providers, request, launcher, cancelled)?;
    if page_index >= rendered.page_count {
        return Err(OfficePreviewPageError::PageOutOfBounds);
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(OfficePreviewPageError::Provider(
            OfficeRenderError::Cancelled,
        ));
    }
    let png = render_pdf_page(&rendered.pdf_path, page_index, target_dim)
        .map_err(OfficePreviewPageError::PdfRender)?;
    let provider_label = match rendered.provider.id {
        OfficeProviderId::LightweightPack => format!(
            "Rendered with {} {} — lightweight preview",
            rendered.provider.id.display_name(),
            rendered.provider.version
        ),
        _ => format!(
            "Rendered with {} {}",
            rendered.provider.id.display_name(),
            rendered.provider.version
        ),
    };
    Ok(RenderedOfficePreviewPage {
        png,
        validated_pdf_path: rendered.pdf_path,
        page_count: rendered.page_count,
        provider_label,
        cache_identity: rendered.cache_identity,
        fallback_failures: rendered.fallback_failures,
    })
}

fn validate_request(request: &OfficeRenderRequest) -> Result<(), OfficeRenderError> {
    if request.policy.version == 0
        || request.policy.max_input_bytes == 0
        || request.policy.max_output_bytes == 0
        || request.policy.max_pages == 0
        || request.policy.timeout.is_zero()
    {
        return Err(OfficeRenderError::InvalidRequest);
    }
    let source = std::fs::symlink_metadata(&request.source_path)
        .map_err(|_| OfficeRenderError::InvalidRequest)?;
    if !source.file_type().is_file() || source.len() > request.policy.max_input_bytes {
        return Err(OfficeRenderError::InvalidRequest);
    }
    let output_dir = std::fs::symlink_metadata(&request.private_output_dir)
        .map_err(|_| OfficeRenderError::InvalidRequest)?;
    if !output_dir.file_type().is_dir() {
        return Err(OfficeRenderError::InvalidRequest);
    }
    Ok(())
}

fn validate_pdf_output(path: &Path, request: &OfficeRenderRequest) -> Result<usize, ()> {
    if path.parent() != Some(request.private_output_dir.as_path()) {
        return Err(());
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.file_type().is_file() || metadata.len() > request.policy.max_output_bytes {
        return Err(());
    }
    let mut bytes = std::fs::read(path).map_err(|_| ())?;
    if bytes.len() as u64 > request.policy.max_output_bytes {
        return Err(());
    }
    let document = lopdf::Document::load_mem(&bytes).map_err(|_| ())?;
    bytes.clear();
    if document.is_encrypted() {
        return Err(());
    }
    let page_count = document.get_pages().len();
    if page_count == 0 || page_count > request.policy.max_pages {
        return Err(());
    }
    Ok(page_count)
}
