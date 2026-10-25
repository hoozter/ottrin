use ottrin_core::{
    EntryKind, FileCommand, FileEntry, PrivilegedCommand, PrivilegedPayload, PrivilegedRequest,
    PrivilegedResponse, PrivilegedStatus, SystemLocateMaintenance,
};
use ottrin_platform::{DefaultPlatform, PlatformOps};
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::path::PathBuf;

enum IoMode {
    Stdio,
    Files {
        request_file: PathBuf,
        response_file: PathBuf,
    },
}

#[cfg(target_os = "linux")]
mod setup;

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().skip(1).eq(["--install"]) {
        match setup::install() {
            Ok(()) => println!("Administrator helper installed."),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::args().skip(1).eq(["--configure-html-sandbox"]) {
        if let Err(error) = setup::configure_html_sandbox() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::args()
        .skip(1)
        .eq(["--remove-html-sandbox-policy"])
    {
        if let Err(error) = setup::remove_html_policy() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let mode = parse_mode(std::env::args().skip(1).collect());
    let mode = match mode {
        Ok(m) => m,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            std::process::exit(2);
        }
    };

    let req = match read_request(&mode) {
        Ok(r) => r,
        Err(msg) => {
            let resp = PrivilegedResponse {
                status: PrivilegedStatus::Failed,
                message: Some(msg),
                payload: None,
            };
            write_response(&mode, resp);
            return;
        }
    };

    let platform = DefaultPlatform;
    let response = match req.command {
        PrivilegedCommand::File(cmd) => run_file_command(&platform, &cmd),
        PrivilegedCommand::ListDirectory { path, show_hidden } => {
            run_list_directory(&path, show_hidden)
        }
        PrivilegedCommand::SystemLocateMaintenance(operation) => {
            run_system_locate_maintenance(operation)
        }
    };
    write_response(&mode, response);
}

fn parse_mode(args: Vec<String>) -> Result<IoMode, String> {
    if args.iter().any(|a| a == "--stdio-json") {
        return Ok(IoMode::Stdio);
    }
    let mut req: Option<PathBuf> = None;
    let mut res: Option<PathBuf> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--request-file" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing path after --request-file".to_string());
                }
                req = Some(PathBuf::from(&args[i]));
            }
            "--response-file" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing path after --response-file".to_string());
                }
                res = Some(PathBuf::from(&args[i]));
            }
            _ => {}
        }
        i += 1;
    }
    if let (Some(request_file), Some(response_file)) = (req, res) {
        Ok(IoMode::Files {
            request_file,
            response_file,
        })
    } else {
        Err(
            "Usage: ottrin-priv-helper --stdio-json OR --request-file <path> --response-file <path>"
                .to_string(),
        )
    }
}

fn read_request(mode: &IoMode) -> Result<PrivilegedRequest, String> {
    let input = match mode {
        IoMode::Stdio => {
            let mut b = Vec::new();
            std::io::stdin()
                .read_to_end(&mut b)
                .map_err(|_| "Failed to read request from stdin".to_string())?;
            b
        }
        IoMode::Files { request_file, .. } => std::fs::read(request_file)
            .map_err(|e| format!("Failed to read request file: {}", e))?,
    };
    serde_json::from_slice(&input).map_err(|e| format!("Invalid request JSON: {}", e))
}

fn run_file_command(platform: &DefaultPlatform, cmd: &FileCommand) -> PrivilegedResponse {
    match platform.execute_command(cmd) {
        Ok(_) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload: None,
        },
        Err(e) => map_platform_error(e.to_string()),
    }
}

fn run_list_directory(path: &Path, show_hidden: bool) -> PrivilegedResponse {
    match list_directory(path, show_hidden) {
        Ok(entries) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload: Some(PrivilegedPayload::Entries(entries)),
        },
        Err(e) => map_platform_error(e),
    }
}

fn map_platform_error(message: String) -> PrivilegedResponse {
    let lower = message.to_ascii_lowercase();
    let denied = lower.contains("permission denied") || lower.contains("access is denied");
    PrivilegedResponse {
        status: if denied {
            PrivilegedStatus::Denied
        } else {
            PrivilegedStatus::Failed
        },
        message: Some(message),
        payload: None,
    }
}

#[cfg(target_os = "linux")]
const SYSTEM_LOCATE_DAILY_JOB: &str = "#!/bin/sh\n/usr/bin/updatedb\n";

#[cfg(target_os = "linux")]
fn run_system_locate_maintenance(operation: SystemLocateMaintenance) -> PrivilegedResponse {
    run_system_locate_maintenance_with(
        operation,
        Path::new("/etc/cron.daily/ottrin-updatedb"),
        run_system_updatedb,
    )
}

#[cfg(not(target_os = "linux"))]
fn run_system_locate_maintenance(_operation: SystemLocateMaintenance) -> PrivilegedResponse {
    PrivilegedResponse {
        status: PrivilegedStatus::Unsupported,
        message: Some("System locate maintenance is supported only on Linux.".to_string()),
        payload: None,
    }
}

#[cfg(target_os = "linux")]
fn run_system_locate_maintenance_with(
    operation: SystemLocateMaintenance,
    daily_job_path: &Path,
    mut run_updatedb: impl FnMut() -> Result<(), String>,
) -> PrivilegedResponse {
    let result = match operation {
        SystemLocateMaintenance::InstallDaily => install_system_locate_daily_job(daily_job_path),
        SystemLocateMaintenance::Remove => remove_system_locate_daily_job(daily_job_path),
        SystemLocateMaintenance::UpdateNow => {
            run_updatedb().map(|()| "System locate database update completed.".to_string())
        }
    };
    match result {
        Ok(message) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: Some(message),
            payload: None,
        },
        Err(message) => map_platform_error(message),
    }
}

#[cfg(target_os = "linux")]
fn run_system_updatedb() -> Result<(), String> {
    let executable = ["/usr/bin/updatedb", "/usr/sbin/updatedb"]
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .ok_or_else(|| "updatedb is not installed at a supported system path".to_string())?;
    let status = std::process::Command::new(executable)
        .status()
        .map_err(|error| format!("Failed to run updatedb: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("updatedb exited with status {status}"))
    }
}

#[cfg(target_os = "linux")]
fn install_system_locate_daily_job(path: &Path) -> Result<String, String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    use std::os::unix::fs::PermissionsExt as _;

    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(format!(
                    "Refusing to replace non-file maintenance entry at {}",
                    path.display()
                ));
            }
            let existing = std::fs::read_to_string(path)
                .map_err(|error| format!("Failed to inspect existing maintenance job: {error}"))?;
            if existing != SYSTEM_LOCATE_DAILY_JOB {
                return Err(format!(
                    "Refusing to replace non-Ottrin maintenance job at {}",
                    path.display()
                ));
            }
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(
                |error| format!("Failed to repair maintenance job permissions: {error}"),
            )?;
            return Ok("Daily system locate maintenance is already installed.".to_string());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to inspect maintenance job: {error}")),
    }

    let parent = path
        .parent()
        .ok_or_else(|| "Maintenance job path has no parent directory".to_string())?;
    let temporary = parent.join(format!(".ottrin-updatedb.{}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o755);
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("Failed to create maintenance job: {error}"))?;
    if let Err(error) = file
        .write_all(SYSTEM_LOCATE_DAILY_JOB.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("Failed to write maintenance job: {error}"));
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("Failed to install maintenance job: {error}"));
    }
    Ok("Daily system locate maintenance installed.".to_string())
}

#[cfg(target_os = "linux")]
fn remove_system_locate_daily_job(path: &Path) -> Result<String, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("Ottrin system locate maintenance was not installed.".to_string());
        }
        Err(error) => return Err(format!("Failed to inspect maintenance job: {error}")),
    };
    if !metadata.file_type().is_file() {
        return Err(format!(
            "Refusing to remove non-file maintenance entry at {}",
            path.display()
        ));
    }
    let existing = std::fs::read_to_string(path)
        .map_err(|error| format!("Failed to inspect maintenance job: {error}"))?;
    if existing != SYSTEM_LOCATE_DAILY_JOB {
        return Err(format!(
            "Refusing to remove non-Ottrin maintenance job at {}",
            path.display()
        ));
    }
    std::fs::remove_file(path)
        .map_err(|error| format!("Failed to remove maintenance job: {error}"))?;
    Ok("Ottrin system locate maintenance removed.".to_string())
}

fn write_response(mode: &IoMode, resp: PrivilegedResponse) {
    match serde_json::to_vec(&resp) {
        Ok(bytes) => match mode {
            IoMode::Stdio => {
                let _ = std::io::stdout().write_all(&bytes);
                let _ = std::io::stdout().flush();
            }
            IoMode::Files { response_file, .. } => {
                let _ = std::fs::write(response_file, bytes);
            }
        },
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "Failed to serialize response: {}", e);
        }
    }
}

fn list_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    use std::time::UNIX_EPOCH;

    let read_dir = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for item in read_dir {
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let name = item.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let file_type = match item.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let size_bytes = if matches!(kind, EntryKind::File) {
            Some(meta.len())
        } else {
            None
        };
        let modified_unix_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let symlink_target_is_dir = if matches!(kind, EntryKind::Symlink) {
            Some(meta.is_dir())
        } else {
            None
        };
        let is_executable = detect_executable(&meta);

        entries.push(FileEntry {
            name,
            path: item.path(),
            kind,
            is_executable,
            symlink_target_is_dir,
            size_bytes,
            modified_unix_secs,
        });
    }
    Ok(entries)
}

fn detect_executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use ottrin_core::SystemLocateMaintenance;

    #[test]
    fn install_system_locate_maintenance_creates_owned_executable_job() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = std::env::temp_dir().join(format!(
            "ottrin-system-locate-maintenance-install-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let job = dir.join("ottrin-updatedb");

        let response =
            run_system_locate_maintenance_with(SystemLocateMaintenance::InstallDaily, &job, || {
                Ok(())
            });

        assert_eq!(response.status, PrivilegedStatus::Success);
        assert_eq!(
            std::fs::read_to_string(&job).unwrap(),
            "#!/bin/sh\n/usr/bin/updatedb\n"
        );
        assert_eq!(
            std::fs::metadata(&job).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn install_system_locate_maintenance_refuses_a_symlink_job_path() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!(
            "ottrin-system-locate-maintenance-symlink-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target");
        let job = dir.join("ottrin-updatedb");
        std::fs::write(&target, SYSTEM_LOCATE_DAILY_JOB).unwrap();
        symlink(&target, &job).unwrap();

        let response =
            run_system_locate_maintenance_with(SystemLocateMaintenance::InstallDaily, &job, || {
                Ok(())
            });

        assert_eq!(response.status, PrivilegedStatus::Failed);
        assert!(job.is_symlink());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_system_locate_maintenance_deletes_only_the_owned_job() {
        let dir = std::env::temp_dir().join(format!(
            "ottrin-system-locate-maintenance-remove-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let job = dir.join("ottrin-updatedb");
        std::fs::write(&job, SYSTEM_LOCATE_DAILY_JOB).unwrap();

        let response =
            run_system_locate_maintenance_with(SystemLocateMaintenance::Remove, &job, || Ok(()));

        assert_eq!(response.status, PrivilegedStatus::Success);
        assert!(!job.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_system_locate_database_runs_the_fixed_updater_operation() {
        let dir = std::env::temp_dir().join(format!(
            "ottrin-system-locate-maintenance-update-{}",
            std::process::id()
        ));
        let job = dir.join("unused-job");
        let mut called = false;

        let response =
            run_system_locate_maintenance_with(SystemLocateMaintenance::UpdateNow, &job, || {
                called = true;
                Ok(())
            });

        assert_eq!(response.status, PrivilegedStatus::Success);
        assert!(called);
    }
}
