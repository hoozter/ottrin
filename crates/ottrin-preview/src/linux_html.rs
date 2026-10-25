//! Fixed HTML protocol for the root-owned, unprivileged AppArmor entry point.
use crate::{
    IsolatedProcessFailure, IsolatedProcessInput, IsolatedProcessLauncher, IsolatedProcessOutput,
    IsolatedProcessRequest,
};
use std::ffi::OsString;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub(crate) const HELPER: &str = "/usr/libexec/ottrin/ottrin-html-render";
const PROFILE: &str = "ottrin-html-render (enforce)";
pub(crate) const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
const BROWSERS: [&str; 4] = [
    "/opt/google/chrome/chrome",
    "/usr/lib/chromium/chromium",
    "/usr/lib/chromium-browser/chromium-browser",
    // Fedora package file list: https://packages.fedoraproject.org/pkgs/chromium/chromium/fedora-44.html
    "/usr/lib64/chromium-browser/chromium-browser",
];

fn protected_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o6022 != 0 {
        return false;
    }
    path.ancestors().skip(1).all(|parent| {
        std::fs::symlink_metadata(parent)
            .is_ok_and(|m| m.is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0)
    })
}

pub(crate) fn browser_path() -> Option<&'static Path> {
    BROWSERS.into_iter().map(Path::new).find(|path| {
        if !protected_file(path) {
            return false;
        }
        let mut magic = [0; 4];
        std::fs::File::open(path)
            .and_then(|mut f| f.read_exact(&mut magic))
            .is_ok()
            && magic == *b"\x7fELF"
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlPreviewRuntimeStatus {
    Ready,
    NeedsSetup,
    MissingBrowser,
    BrowserNeedsRepair,
    MissingTools,
    AdministratorSession,
}

impl HtmlPreviewRuntimeStatus {
    pub fn message(self) -> &'static str {
        match self {
            Self::Ready => "Installed and ready",
            Self::NeedsSetup => "HTML sandbox setup needed",
            Self::MissingBrowser => "Install a supported system Chrome or Chromium browser",
            Self::BrowserNeedsRepair => {
                "The browser needs a protected native executable and secure system permissions"
            }
            Self::MissingTools => "Install Bubblewrap and systemd user-session tools",
            Self::AdministratorSession => "HTML previews require an ordinary user session",
        }
    }
}

pub fn html_preview_runtime_status() -> HtmlPreviewRuntimeStatus {
    // SAFETY: read-only identity inspection.
    if unsafe { libc::geteuid() } == 0 {
        return HtmlPreviewRuntimeStatus::AdministratorSession;
    }
    if ![
        crate::linux_namespace::BWRAP,
        "/usr/bin/systemd-run",
        "/usr/bin/systemctl",
    ]
    .iter()
    .all(|p| Path::new(p).is_file())
    {
        return HtmlPreviewRuntimeStatus::MissingTools;
    }
    if browser_path().is_none() {
        return if BROWSERS.iter().any(|path| Path::new(path).exists()) {
            HtmlPreviewRuntimeStatus::BrowserNeedsRepair
        } else {
            HtmlPreviewRuntimeStatus::MissingBrowser
        };
    }
    if !protected_file(Path::new(HELPER)) {
        return HtmlPreviewRuntimeStatus::NeedsSetup;
    }
    if Command::new(HELPER)
        .arg("--check")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        HtmlPreviewRuntimeStatus::Ready
    } else {
        HtmlPreviewRuntimeStatus::NeedsSetup
    }
}

pub(crate) fn runtime_available() -> bool {
    html_preview_runtime_status() == HtmlPreviewRuntimeStatus::Ready
}

pub(crate) fn request(root: &Path, browser: &Path, dimension: u32) -> IsolatedProcessRequest {
    IsolatedProcessRequest {
        executable: browser.to_path_buf(),
        arguments: [
            "--headless=new",
            "--disable-gpu",
            "--disable-javascript",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-extensions",
            "--disable-sync",
            "--metrics-recording-only",
            "--no-first-run",
            "--no-default-browser-check",
            "--user-data-dir=/profile",
            "--screenshot=/output/output.png",
        ]
        .into_iter()
        .map(OsString::from)
        .chain([
            OsString::from(format!("--window-size={dimension},{dimension}")),
            OsString::from("file:///input/document.html"),
        ])
        .collect(),
        input: Some(IsolatedProcessInput {
            host_path: root.join("input.html"),
            sandbox_path: PathBuf::from("/input/document.html"),
        }),
        private_profile_dir: root.join("profile"),
        private_output_dir: root.join("output"),
        timeout: Duration::from_secs(8),
        max_memory_bytes: 512 * 1024 * 1024,
        max_processes: 256,
        max_output_bytes: MAX_OUTPUT,
    }
}

pub(crate) struct LinuxHtmlIsolation;
impl IsolatedProcessLauncher for LinuxHtmlIsolation {
    fn run(
        &self,
        req: &IsolatedProcessRequest,
        cancelled: &AtomicBool,
    ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
        if !protected_file(Path::new(HELPER)) {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        let (root, dimension) = admit_request(req)?;
        if browser_path() != Some(req.executable.as_path()) {
            return Err(IsolatedProcessFailure::Unavailable);
        }
        let mut command = Command::new(HELPER);
        command.arg(root).arg(dimension.to_string());
        crate::isolated_process::linux::run_command(req, cancelled, command)
    }
}

fn admit_request(req: &IsolatedProcessRequest) -> Result<(&Path, u32), IsolatedProcessFailure> {
    let root = req
        .private_profile_dir
        .parent()
        .ok_or(IsolatedProcessFailure::InvalidRequest)?;
    let dimensions = req
        .arguments
        .iter()
        .filter_map(|a| a.to_str())
        .find_map(|a| a.strip_prefix("--window-size="))
        .ok_or(IsolatedProcessFailure::InvalidRequest)?;
    let (width, height) = dimensions
        .split_once(',')
        .ok_or(IsolatedProcessFailure::InvalidRequest)?;
    let dimension: u32 = width
        .parse()
        .map_err(|_| IsolatedProcessFailure::InvalidRequest)?;
    if !(128..=2048).contains(&dimension) || width != height {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    if !root.is_absolute() || req != &request(root, &req.executable, dimension) {
        return Err(IsolatedProcessFailure::InvalidRequest);
    }
    Ok((root, dimension))
}

fn private_path(path: &Path, uid: u32, directory: bool) -> Result<(), String> {
    let m = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect private HTML workspace: {e}"))?;
    if m.uid() != uid
        || m.mode() & 0o077 != 0
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.len() > MAX_DOCUMENT_BYTES))
    {
        return Err(
            "HTML workspace must contain only private, caller-owned regular input and directories"
                .into(),
        );
    }
    Ok(())
}

/// Entry used by the installed HTML helper. It never elevates or accepts a command.
pub fn run_html_renderer_helper(arguments: Vec<OsString>) -> Result<(), String> {
    // SAFETY: these calls only inspect the process identity.
    let (uid, effective) = unsafe { (libc::getuid(), libc::geteuid()) };
    if uid == 0 || effective != uid {
        return Err("HTML previews must run as the ordinary calling user".into());
    }
    let label = std::fs::read_to_string("/proc/self/attr/current").unwrap_or_default();
    if std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
        .is_ok_and(|value| value.trim() == "1")
        && label.trim() != PROFILE
    {
        return Err(
            "HTML sandbox policy is not enforced; update system integration in Settings > Services"
                .into(),
        );
    }
    let browser =
        browser_path().ok_or("Install a supported, protected system Chrome or Chromium runtime")?;
    if !Path::new(crate::linux_namespace::BWRAP).is_file() {
        return Err("Install Bubblewrap for HTML previews".into());
    }
    if arguments == [OsString::from("--check")] {
        return Ok(());
    }
    if arguments.len() != 2 {
        return Err("Usage: ottrin-html-render PRIVATE_WORKSPACE IMAGE_SIZE".into());
    }
    let root = Path::new(&arguments[0]);
    let dimension = arguments[1]
        .to_str()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| (128..=2048).contains(n))
        .ok_or("HTML image size must be between 128 and 2048 pixels")?;
    if !root.is_absolute() || root.canonicalize().map_err(|e| e.to_string())? != root {
        return Err("HTML workspace must be an absolute canonical directory".into());
    }
    for path in [
        root.to_path_buf(),
        root.join("profile"),
        root.join("output"),
    ] {
        private_path(&path, uid, true)?;
    }
    private_path(&root.join("input.html"), uid, false)?;
    // SAFETY: this only removes the ability to gain privilege through later exec.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "Cannot restrict HTML helper privilege: {}",
            std::io::Error::last_os_error()
        ));
    }
    let req = request(root, browser, dimension);
    let error = crate::linux_namespace::command(&req).env_clear().exec();
    Err(format!("Cannot launch isolated HTML renderer: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn html_protocol_refuses_arbitrary_commands_mounts_paths_and_unbounded_resources() {
        let valid = request(
            Path::new("/tmp/private-html"),
            Path::new("/opt/google/chrome/chrome"),
            256,
        );
        assert!(admit_request(&valid).is_ok());
        let mut invalid = valid.clone();
        invalid.arguments.push("--no-sandbox".into());
        assert!(admit_request(&invalid).is_err());
        let mut invalid = valid.clone();
        invalid
            .arguments
            .extend(["--ro-bind".into(), "/home".into(), "/usr".into()]);
        assert!(admit_request(&invalid).is_err());
        let mut invalid = valid.clone();
        invalid.private_profile_dir = PathBuf::from("/tmp/private-html/unrelated");
        assert!(admit_request(&invalid).is_err());
        let mut invalid = valid.clone();
        invalid.input.as_mut().unwrap().sandbox_path = PathBuf::from("/opt/google/chrome/chrome");
        assert!(admit_request(&invalid).is_err());
        let mut invalid = valid.clone();
        invalid.max_memory_bytes = u64::MAX;
        assert!(admit_request(&invalid).is_err());
        let mut invalid = valid.clone();
        invalid.timeout = Duration::ZERO;
        assert!(admit_request(&invalid).is_err());
        let invalid = request(Path::new("relative"), &valid.executable, 256);
        assert!(admit_request(&invalid).is_err());
        for dimension in [0, 127, 2049, u32::MAX] {
            let invalid = request(Path::new("/tmp/private-html"), &valid.executable, dimension);
            assert!(admit_request(&invalid).is_err());
        }
    }

    #[test]
    fn html_workspace_admission_rejects_public_wrong_owner_symlink_and_oversized_input() {
        let fixture = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        // SAFETY: read-only process identity inspection.
        let uid = unsafe { libc::geteuid() };
        assert!(private_path(fixture.path(), uid, true).is_ok());
        assert!(private_path(fixture.path(), uid.wrapping_add(1), true).is_err());
        let input = fixture.path().join("input.html");
        std::fs::write(&input, b"private HTML").unwrap();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(private_path(&input, uid, false).is_ok());
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(private_path(&input, uid, false).is_err());
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
        let alias = fixture.path().join("alias");
        symlink(&input, &alias).unwrap();
        assert!(private_path(&alias, uid, false).is_err());
        std::fs::OpenOptions::new()
            .write(true)
            .open(&input)
            .unwrap()
            .set_len(MAX_DOCUMENT_BYTES + 1)
            .unwrap();
        assert!(private_path(&input, uid, false).is_err());
    }
}
