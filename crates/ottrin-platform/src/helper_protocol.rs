//! A helper is probed without elevation before any mutation or authorization.
//! Output is discarded, so a broken helper cannot fill a pipe or retain readers.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn guidance() -> &'static str {
    if cfg!(target_os = "linux") {
        "Administrator helper needs an update. Open Services and choose Update / reinstall helper."
    } else {
        "Administrator helper does not match Ottrin. Reinstall Ottrin or restore the complete portable package."
    }
}

fn executable(name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let candidate = if path.components().count() > 1 || path.is_absolute() {
        path.to_owned()
    } else {
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .map(|directory| directory.join(path))
            .find(|path| path.is_file())
            .ok_or_else(|| guidance().to_owned())?
    };
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(candidate)
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&absolute)
            .map_err(|_| guidance().to_owned())?
            .permissions()
            .mode()
            & 0o6000
            != 0
        {
            return Err("Administrator helper must not have set-user-ID or set-group-ID permissions. Repair its installation.".into());
        }
    }
    Ok(absolute)
}

pub(super) fn check(name: &str, copy_recovery: bool) -> Result<PathBuf, String> {
    check_with_timeout(name, copy_recovery, Duration::from_secs(2))
}

fn check_with_timeout(
    name: &str,
    copy_recovery: bool,
    timeout: Duration,
) -> Result<PathBuf, String> {
    let path = executable(name)?;
    let mut command = Command::new(&path);
    command
        .arg("--check-protocol")
        .arg(ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if copy_recovery {
        command.arg("--copy-recovery");
    }
    let mut child = command.spawn().map_err(|_| guidance().to_owned())?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => Ok(path),
                    Some(69) => {
                        Err("Administrator Copy recovery is unavailable in this helper.".into())
                    }
                    _ => Err(guidance().into()),
                };
            }
            Ok(None) => {}
            Err(error) => {
                child.kill().map_err(|stop| {
                    format!(
                        "Helper probe failed: {error}; could not stop process {}: {stop}",
                        child.id()
                    )
                })?;
                child.wait().map_err(|stop| stop.to_string())?;
                return Err(format!(
                    "Helper compatibility check failed: {error}. {}",
                    guidance()
                ));
            }
        }
        if Instant::now() >= deadline {
            // Reap even a racing natural exit. This probe owns no child threads or pipes.
            if let Err(error) = child.kill()
                && child.try_wait().map_err(|wait| wait.to_string())?.is_none()
            {
                return Err(format!(
                    "Could not stop helper compatibility probe {}: {error}",
                    child.id()
                ));
            }
            child.wait().map_err(|error| error.to_string())?;
            return Err(format!(
                "Helper compatibility check timed out. {}",
                guidance()
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn helper(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("helper with spaces");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[test]
    fn helper_protocol_probes_exact_version_and_required_feature_without_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let path = helper(
            dir.path(),
            "test \"$1\" = --check-protocol || exit 3\ntest \"$2\" = 2 || exit 4\ntest \"$3\" = --copy-recovery || exit 5\nif read -r value; then exit 6; fi\nexit 0",
        );
        assert_eq!(check(path.to_str().unwrap(), true).unwrap(), path);
        assert!(check(path.to_str().unwrap(), false).is_err());
    }
    #[test]
    fn helper_protocol_refuses_legacy_mismatch_and_unavailable_features() {
        let dir = tempfile::tempdir().unwrap();
        for code in [2, 78, 69] {
            let path = helper(dir.path(), &format!("exit {code}"));
            let error = check(path.to_str().unwrap(), true).unwrap_err();
            assert!(if code == 69 {
                error.contains("unavailable")
            } else {
                error.contains("update")
            });
        }
    }
    #[test]
    fn helper_protocol_kills_and_reaps_a_nonresponsive_probe() {
        let dir = tempfile::tempdir().unwrap();
        let path = helper(
            dir.path(),
            "printf '%s\\n' \"$$\" > \"$0.pid\"\nexec sleep 30",
        );
        let start = Instant::now();
        assert!(
            check_with_timeout(path.to_str().unwrap(), true, Duration::from_millis(200))
                .unwrap_err()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        let pid: u32 = std::fs::read_to_string(format!("{}.pid", path.display()))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            !Path::new("/proc").join(pid.to_string()).exists(),
            "Owned timed-out probe must be reaped"
        );
    }
    #[test]
    fn helper_protocol_refuses_set_id_helper_before_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = helper(dir.path(), "exit 0");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o4755)).unwrap();
        assert!(
            check(path.to_str().unwrap(), true)
                .unwrap_err()
                .contains("set-user-ID")
        );
    }
}
