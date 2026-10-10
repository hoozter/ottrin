use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

pub(super) fn install(package_helper: &Path) -> Result<String, String> {
    install_with(package_helper, |helper| {
        Command::new("pkexec")
            .arg("--disable-internal-agent")
            .arg(helper)
            .arg("--install")
            .output()
    })
}

fn install_with(
    package_helper: &Path,
    run: impl FnOnce(&Path) -> std::io::Result<Output>,
) -> Result<String, String> {
    // Root cannot traverse a user-owned AppImage FUSE mount. Stage the complete
    // executable on a normal filesystem before pkexec inspects it or authenticates.
    // The private directory prevents other users from replacing the staged binary.
    let directory = tempfile::Builder::new()
        .prefix("ottrin-helper-setup-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .map_err(|error| format!("Cannot prepare helper setup: {error}"))?;
    let helper = directory.path().join("ottrin-priv-helper");
    std::fs::copy(package_helper, &helper)
        .and_then(|_| std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)))
        .map_err(|error| format!("Cannot prepare the bundled helper: {error}"))?;
    let output = run(&helper)
        .map_err(|error| format!("Cannot open administrator authentication: {error}"))?;
    if output.status.success() {
        Ok("Administrator helper installed and ready.".to_string())
    } else if output.status.code() == Some(126) {
        Err("Helper setup was cancelled.".to_string())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        Err(if detail.is_empty() {
            format!(
                "Helper setup failed ({}). Administrator authentication or helper execution could not complete.",
                output.status
            )
        } else {
            format!("Helper setup failed: {detail}")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn output(code: i32, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn stages_private_executable_and_cleans_up_after_every_result() {
        let package = tempfile::tempdir().unwrap();
        let original = package.path().join("helper");
        std::fs::write(&original, b"packaged executable").unwrap();
        for code in [0, 126, 127] {
            let mut staged = None;
            let result = install_with(&original, |helper| {
                assert_ne!(helper, original);
                assert_eq!(std::fs::read(helper).unwrap(), b"packaged executable");
                assert_eq!(
                    std::fs::metadata(helper).unwrap().permissions().mode() & 0o777,
                    0o755
                );
                assert_eq!(
                    std::fs::metadata(helper.parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
                staged = Some(helper.to_path_buf());
                Ok(output(code, "Error accessing helper: Permission denied"))
            });
            assert_eq!(result.is_ok(), code == 0);
            assert!(!staged.unwrap().parent().unwrap().exists());
            if code == 126 {
                assert_eq!(result.as_ref().unwrap_err(), "Helper setup was cancelled.");
            }
            if code == 127 {
                let error = result.as_ref().unwrap_err();
                assert!(error.contains("Error accessing helper: Permission denied"));
                assert!(!error.contains("cancelled"));
            }
        }
    }

    #[test]
    fn launch_failure_retains_cause_and_removes_staging() {
        let package = tempfile::tempdir().unwrap();
        let original = package.path().join("helper");
        std::fs::write(&original, b"executable").unwrap();
        let mut staged = None;
        let error = install_with(&original, |helper| {
            staged = Some(helper.to_path_buf());
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "pkexec unavailable",
            ))
        })
        .unwrap_err();
        assert!(error.contains("pkexec unavailable"));
        assert!(!staged.unwrap().parent().unwrap().exists());
    }
}
