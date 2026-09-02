#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::office_provider::{
    IsolatedProcessFailure, IsolatedProcessLauncher, IsolatedProcessOutput, IsolatedProcessRequest,
};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxBubblewrapIsolation;

/// Windows process-lifetime and resource boundary for installed Office providers.
///
/// The child starts inside a kill-on-close Job Object with aggregate memory and
/// active-process limits. Only its three standard-I/O handles are inherited.
/// This is deliberately not a filesystem sandbox or a network-isolation claim;
/// provider admission remains fail-closed until the native acceptance lane has
/// exercised this boundary on supported Windows hosts.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsJobObjectIsolation;

#[cfg(target_os = "windows")]
mod windows_job;

#[cfg(target_os = "windows")]
impl IsolatedProcessLauncher for WindowsJobObjectIsolation {
    fn run(
        &self,
        request: &IsolatedProcessRequest,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
        windows_job::run(request, cancelled)
    }
}

#[cfg(target_os = "linux")]
impl IsolatedProcessLauncher for LinuxBubblewrapIsolation {
    fn run(
        &self,
        request: &IsolatedProcessRequest,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
        linux::run(request, cancelled)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const BWRAP: &str = "/usr/bin/bwrap";
    const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
    const SYSTEMCTL: &str = "/usr/bin/systemctl";
    const MAX_DIAGNOSTIC_BYTES: u64 = 64 * 1024;

    pub(super) fn run(
        request: &IsolatedProcessRequest,
        cancelled: &AtomicBool,
    ) -> Result<IsolatedProcessOutput, IsolatedProcessFailure> {
        validate(request)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(IsolatedProcessFailure::Cancelled);
        }

        let stdout_path = request.private_profile_dir.join(".ottrin-stdout");
        let stderr_path = request.private_profile_dir.join(".ottrin-stderr");
        let stdout = private_log(&stdout_path)?;
        let stderr = match private_log(&stderr_path) {
            Ok(stderr) => stderr,
            Err(failure) => {
                let _ = std::fs::remove_file(&stdout_path);
                return Err(failure);
            }
        };
        let _log_cleanup = ControllerLogCleanup {
            stdout: stdout_path.clone(),
            stderr: stderr_path.clone(),
        };
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let unit = format!("ottrin-render-{}-{unique}", std::process::id());
        let cpu_seconds = request.timeout.as_secs().max(1).saturating_add(1);
        let file_size = request.max_output_bytes.max(MAX_DIAGNOSTIC_BYTES);
        let executable_is_already_visible = ["/usr", "/bin", "/lib", "/lib64", "/opt"]
            .iter()
            .any(|root| request.executable.starts_with(root));
        let sandbox_executable = if executable_is_already_visible {
            request.executable.clone()
        } else {
            PathBuf::from("/run/ottrin-render-helper")
        };
        let bundled_runtime = (!executable_is_already_visible)
            .then(|| request.executable.parent())
            .flatten()
            .and_then(|parent| {
                [parent.join("../lib"), parent.join("../../lib")]
                    .into_iter()
                    .find(|candidate| candidate.is_dir())
            });
        let mut command = Command::new(SYSTEMD_RUN);
        command
            .args(["--user", "--wait", "--collect", "--pipe", "--quiet"])
            .arg(format!("--unit={unit}"))
            .arg("--property=Type=exec")
            .arg(format!("--property=TasksMax={}", request.max_processes))
            .arg(format!("--property=MemoryMax={}", request.max_memory_bytes))
            .arg(format!("--property=LimitFSIZE={file_size}"))
            .arg(format!("--property=LimitCPU={cpu_seconds}s"))
            .arg(BWRAP)
            .args([
                "--unshare-all",
                "--die-with-parent",
                "--clearenv",
                "--setenv",
                "HOME",
                "/profile",
                "--setenv",
                "TMPDIR",
                "/tmp",
                "--setenv",
                "LANG",
                "C.UTF-8",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind",
                "/etc",
                "/etc",
            ]);
        for system_path in ["/lib", "/lib64", "/opt"] {
            if Path::new(system_path).exists() {
                command.args(["--ro-bind", system_path, system_path]);
            }
        }
        command.args([
            "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/input", "--dir",
            "/run", "--bind",
        ]);
        command
            .arg(&request.private_profile_dir)
            .arg("/profile")
            .arg("--bind")
            .arg(&request.private_output_dir)
            .arg("/output");
        if let Some(input) = &request.input {
            command
                .arg("--ro-bind")
                .arg(&input.host_path)
                .arg(&input.sandbox_path);
        }
        if !executable_is_already_visible {
            command
                .arg("--ro-bind")
                .arg(&request.executable)
                .arg(&sandbox_executable);
        }
        if let Some(runtime) = &bundled_runtime {
            command
                .arg("--ro-bind")
                .arg(runtime)
                .arg("/run/ottrin-lib")
                .args(["--setenv", "LD_LIBRARY_PATH", "/run/ottrin-lib"]);
        }
        command
            .arg(&sandbox_executable)
            .args(&request.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));

        // SAFETY: this closure executes in the child after fork and before exec. `setpgid` is
        // async-signal-safe and isolates the local systemd-run controller for bounded cleanup.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|_| IsolatedProcessFailure::Unavailable)?;
        let child_pid = child.id() as i32;
        let deadline = Instant::now() + request.timeout;
        let status = loop {
            if cancelled.load(Ordering::Acquire) {
                terminate_unit(&unit, child_pid, &mut child);
                return Err(IsolatedProcessFailure::Cancelled);
            }
            if Instant::now() >= deadline {
                terminate_unit(&unit, child_pid, &mut child);
                return Err(IsolatedProcessFailure::Timeout);
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    terminate_unit(&unit, child_pid, &mut child);
                    return Err(IsolatedProcessFailure::Crash);
                }
            }
        };
        let stdout = read_bounded(&stdout_path)?;
        let diagnostics = read_bounded(&stderr_path)?;
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
        if !status.success() {
            return if diagnostics.windows(8).any(|window| window == b"oom-kill")
                || diagnostics
                    .windows(14)
                    .any(|window| window == b"resource-limit")
            {
                Err(IsolatedProcessFailure::LimitExceeded)
            } else {
                Err(IsolatedProcessFailure::Crash)
            };
        }
        validate_output_budget(&request.private_output_dir, request.max_output_bytes)?;
        Ok(IsolatedProcessOutput {
            stdout,
            diagnostics,
        })
    }

    fn validate(request: &IsolatedProcessRequest) -> Result<(), IsolatedProcessFailure> {
        if !Path::new(BWRAP).is_file()
            || !Path::new(SYSTEMD_RUN).is_file()
            || !Path::new(SYSTEMCTL).is_file()
            || !request.executable.is_absolute()
            || request.timeout.is_zero()
            || request.max_memory_bytes == 0
            || request.max_processes == 0
            || request.max_output_bytes == 0
            || !regular_file(&request.executable)
            || !directory(&request.private_profile_dir)
            || !directory(&request.private_output_dir)
        {
            return Err(IsolatedProcessFailure::InvalidRequest);
        }
        if let Some(input) = &request.input
            && (!regular_file(&input.host_path)
                || !input.sandbox_path.is_absolute()
                || input.sandbox_path.parent() != Some(Path::new("/input")))
        {
            return Err(IsolatedProcessFailure::InvalidRequest);
        }
        Ok(())
    }

    fn regular_file(path: &Path) -> bool {
        std::fs::metadata(path)
            .map(|metadata| metadata.is_file())
            .unwrap_or(false)
    }

    fn directory(path: &Path) -> bool {
        std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false)
    }

    struct ControllerLogCleanup {
        stdout: std::path::PathBuf,
        stderr: std::path::PathBuf,
    }

    impl Drop for ControllerLogCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.stdout);
            let _ = std::fs::remove_file(&self.stderr);
        }
    }

    fn private_log(path: &Path) -> Result<File, IsolatedProcessFailure> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| IsolatedProcessFailure::InvalidRequest)
    }

    fn read_bounded(path: &Path) -> Result<Vec<u8>, IsolatedProcessFailure> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| IsolatedProcessFailure::Crash)?
            .take(MAX_DIAGNOSTIC_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| IsolatedProcessFailure::Crash)?;
        if bytes.len() as u64 > MAX_DIAGNOSTIC_BYTES {
            return Err(IsolatedProcessFailure::LimitExceeded);
        }
        Ok(bytes)
    }

    fn validate_output_budget(directory: &Path, max: u64) -> Result<(), IsolatedProcessFailure> {
        let mut total = 0_u64;
        let entries = std::fs::read_dir(directory).map_err(|_| IsolatedProcessFailure::Crash)?;
        for entry in entries {
            let entry = entry.map_err(|_| IsolatedProcessFailure::Crash)?;
            let metadata = entry
                .metadata()
                .map_err(|_| IsolatedProcessFailure::Crash)?;
            if metadata.is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or(IsolatedProcessFailure::LimitExceeded)?;
                if total > max {
                    return Err(IsolatedProcessFailure::LimitExceeded);
                }
            }
        }
        Ok(())
    }

    fn terminate_unit(unit: &str, pid: i32, child: &mut std::process::Child) {
        let _ = Command::new(SYSTEMCTL)
            .args(["--user", "kill", "--kill-who=all", "--signal=KILL"])
            .arg(format!("{unit}.service"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        // SAFETY: a negative pid targets only the controller process group created in `pre_exec`.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}
