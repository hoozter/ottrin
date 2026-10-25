use crate::IsolatedProcessRequest;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) const BWRAP: &str = "/usr/bin/bwrap";

pub(crate) fn command(request: &IsolatedProcessRequest) -> Command {
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
    let mut command = Command::new(BWRAP);
    command.args([
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
    // Distribution launchers such as LibreOffice use a /bin/sh shebang.
    // /usr alone does not expose /bin in a fresh sandbox, even on merged-usr hosts.
    for system_path in ["/bin", "/lib", "/lib64", "/opt"] {
        if Path::new(system_path).exists() {
            command.args(["--ro-bind", system_path, system_path]);
        }
    }
    command.args([
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/input", "--dir", "/run",
        "--bind",
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
    command.arg(&sandbox_executable).args(&request.arguments);
    command
}
