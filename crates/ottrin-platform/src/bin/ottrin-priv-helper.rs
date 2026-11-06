use ottrin_core::{
    EntryKind, FileCommand, FileEntry, PrivilegedCommand, PrivilegedPayload, PrivilegedRequest,
    PrivilegedResponse, PrivilegedStatus, SystemLocateMaintenance,
};
use ottrin_platform::{DefaultPlatform, PlatformOps};
#[cfg(not(target_os = "windows"))]
use std::io::Read as _;
use std::io::Write as _;
use std::path::Path;

enum IoMode {
    #[cfg(not(target_os = "windows"))]
    Stdio,
    #[cfg(target_os = "windows")]
    Pipe {
        nonce: String,
        parent_pid: u32,
        parent_start: u64,
        channel: Option<ottrin_platform::windows_privileged_transport::HelperChannel>,
    },
}

#[cfg(target_os = "linux")]
mod setup;

fn main() {
    if let Some(exit_code) = protocol_check(&std::env::args().skip(1).collect::<Vec<_>>()) {
        std::process::exit(exit_code);
    }
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
    if let Some(option) = std::env::args().nth(1)
        && std::env::args().len() == 2
        && matches!(
            option.as_str(),
            "--install-portal-definition" | "--remove-portal-definition"
        )
    {
        if let Err(error) =
            setup::configure_portal_definition(option == "--remove-portal-definition")
        {
            eprintln!("{error}");
            std::process::exit(1);
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
    let mut mode = match mode {
        Ok(m) => m,
        Err(msg) => {
            let _ = writeln!(std::io::stderr(), "{msg}");
            std::process::exit(2);
        }
    };

    let req = match read_request(&mut mode) {
        Ok(r) => r,
        Err(msg) => {
            let resp = PrivilegedResponse {
                status: PrivilegedStatus::Failed,
                message: Some(msg),
                payload: None,
            };
            write_response(&mut mode, resp);
            return;
        }
    };

    #[cfg(target_os = "windows")]
    if let IoMode::Pipe {
        channel: Some(channel),
        ..
    } = &mut mode
        && let Some(result) = channel.serve_copy_command(&req.command)
    {
        if let Err(error) = result {
            eprintln!("Administrator Copy stream failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    let platform = DefaultPlatform;
    let response = match req.command {
        PrivilegedCommand::File(cmd) => run_file_command(&platform, &cmd),
        PrivilegedCommand::DiscoverCopyHistory => discover_copy_history(),
        PrivilegedCommand::UndoCopy { transactions } => copy_recovery_command(&transactions, false),
        PrivilegedCommand::CleanupCopy { transactions } => {
            copy_recovery_command(&transactions, true)
        }
        PrivilegedCommand::RecoverCopy { .. } => PrivilegedResponse {
            status: PrivilegedStatus::Unsupported,
            message: Some("This Copy recovery action is unavailable on this platform.".into()),
            payload: None,
        },
        PrivilegedCommand::ListDirectory { path, show_hidden } => {
            run_list_directory(&path, show_hidden)
        }
        PrivilegedCommand::SystemLocateMaintenance(operation) => {
            run_system_locate_maintenance(operation)
        }
    };
    write_response(&mut mode, response);
}

/// Read-only feature negotiation. No stdin, privilege request or filesystem work.
fn protocol_check(args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some("--check-protocol") {
        return None;
    }
    if args.get(1).and_then(|value| value.parse::<u32>().ok())
        != Some(ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION)
    {
        return Some(78);
    }
    if args[2..].iter().any(|feature| {
        !matches!(feature.as_str(),
            "--copy-recovery" if ottrin_core::PRIVILEGED_COPY_RECOVERY_SUPPORTED)
            && !matches!(feature.as_str(), "--bound-local-pipe" if cfg!(target_os = "windows"))
    }) {
        return Some(69);
    }
    Some(0)
}

fn parse_mode(args: Vec<String>) -> Result<IoMode, String> {
    match args.as_slice() {
        #[cfg(not(target_os = "windows"))]
        [mode] if mode == "--stdio-json" => Ok(IoMode::Stdio),
        #[cfg(target_os = "windows")]
        [flag, nonce, parent_flag, parent, start_flag, start]
            if flag == "--local-pipe"
                && parent_flag == "--parent-pid"
                && start_flag == "--parent-start" =>
        {
            let parent_pid = parent
                .parse::<u32>()
                .map_err(|_| "Invalid helper parent PID")?;
            let parent_start = start
                .parse::<u64>()
                .map_err(|_| "Invalid helper parent creation time")?;
            if parent_pid == 0
                || parent_start == 0
                || nonce.len() != 32
                || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("Invalid helper pipe identity".into());
            }
            Ok(IoMode::Pipe {
                nonce: nonce.clone(),
                parent_pid,
                parent_start,
                channel: None,
            })
        }
        _ => {
            #[cfg(target_os = "windows")]
            let usage = "Usage: ottrin-priv-helper --local-pipe <nonce> --parent-pid <PID> --parent-start <creation-time>";
            #[cfg(not(target_os = "windows"))]
            let usage = "Usage: ottrin-priv-helper --stdio-json";
            Err(usage.into())
        }
    }
}

fn read_request(mode: &mut IoMode) -> Result<PrivilegedRequest, String> {
    match mode {
        #[cfg(not(target_os = "windows"))]
        IoMode::Stdio => {
            let mut input = Vec::new();
            std::io::stdin()
                .read_to_end(&mut input)
                .map_err(|_| "Failed to read request from stdin".to_string())?;
            let request: PrivilegedRequest =
                serde_json::from_slice(&input).map_err(|e| format!("Invalid request JSON: {e}"))?;
            request.validate_protocol().map_err(str::to_owned)?;
            Ok(request)
        }
        #[cfg(target_os = "windows")]
        IoMode::Pipe {
            nonce,
            parent_pid,
            parent_start,
            channel,
        } => {
            let connected = ottrin_platform::windows_privileged_transport::HelperChannel::connect(
                nonce,
                *parent_pid,
                *parent_start,
            )
            .map_err(|error| error.to_string())?;
            *channel = Some(connected);
            channel.as_mut().unwrap().read_request()
        }
    }
}

fn run_file_command(platform: &DefaultPlatform, cmd: &FileCommand) -> PrivilegedResponse {
    #[cfg(target_os = "linux")]
    if let FileCommand::Copy {
        sources,
        destination,
        conflict,
    } = cmd
    {
        return ottrin_platform::privileged_copy::copy(platform, sources, destination, *conflict);
    }
    #[cfg(not(target_os = "linux"))]
    if matches!(cmd, FileCommand::Copy { .. }) {
        return PrivilegedResponse {
            status: PrivilegedStatus::Unsupported,
            message: Some(
                "Administrator Copy recovery is not implemented on this platform yet.".into(),
            ),
            payload: None,
        };
    }
    match platform.execute_command(cmd) {
        Ok(_) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload: None,
        },
        Err(e) => map_platform_error(e.to_string()),
    }
}

fn discover_copy_history() -> PrivilegedResponse {
    #[cfg(target_os = "linux")]
    let result = ottrin_platform::privileged_copy::discover_history();
    #[cfg(not(target_os = "linux"))]
    let result = Err::<Vec<ottrin_core::PrivilegedCopyHistoryEntry>, _>(
        ottrin_platform::PlatformError::NotAvailable(
            "Administrator Copy history is unavailable on this platform",
        ),
    );
    match result {
        Ok(history) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload: Some(PrivilegedPayload::CopyHistory(history)),
        },
        Err(error) => map_platform_error(error.to_string()),
    }
}

fn copy_recovery_command(transactions: &[String], cleanup: bool) -> PrivilegedResponse {
    #[cfg(target_os = "linux")]
    let result = if cleanup {
        ottrin_platform::privileged_copy::cleanup(transactions)
    } else {
        ottrin_platform::privileged_copy::undo(transactions)
    };
    #[cfg(not(target_os = "linux"))]
    let result = {
        let _ = (transactions, cleanup);
        Err(ottrin_platform::PlatformError::NotAvailable(
            "Privileged Copy recovery is not implemented on this platform",
        ))
    };
    match result {
        Ok(()) => PrivilegedResponse {
            status: PrivilegedStatus::Success,
            message: None,
            payload: None,
        },
        Err(error) => map_platform_error(error.to_string()),
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

fn write_response(mode: &mut IoMode, resp: PrivilegedResponse) {
    let result = match mode {
        #[cfg(not(target_os = "windows"))]
        IoMode::Stdio => serde_json::to_vec(&resp)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                std::io::stdout().write_all(&bytes)?;
                std::io::stdout().flush()
            }),
        #[cfg(target_os = "windows")]
        IoMode::Pipe { channel, .. } => channel
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Helper pipe was not connected"))
            .and_then(|channel| channel.write_response(&resp)),
    };
    if let Err(error) = result {
        eprintln!("Failed to deliver helper response: {error}");
        std::process::exit(1);
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
    #[test]
    fn mutation_modes_refuse_unknown_flags_and_mixed_probe_arguments() {
        for args in [
            vec!["--stdio-json", "--check-protocol", "1"],
            vec!["--stdio-json", "--unknown"],
            vec!["--request-file", "input"],
            vec![
                "--stdio-json",
                "--request-file",
                "input",
                "--response-file",
                "output",
            ],
        ] {
            assert!(parse_mode(args.into_iter().map(str::to_owned).collect()).is_err());
        }
        assert!(matches!(
            parse_mode(vec!["--stdio-json".into()]),
            Ok(IoMode::Stdio)
        ));
        assert!(
            parse_mode(vec![
                "--request-file".into(),
                "input".into(),
                "--response-file".into(),
                "output".into(),
            ])
            .is_err()
        );
    }

    #[test]
    fn protocol_probe_refuses_missing_malformed_future_and_unknown_feature_contracts() {
        for args in [
            vec!["--check-protocol"],
            vec!["--check-protocol", "bad"],
            vec!["--check-protocol", "1"],
            vec!["--check-protocol", "2"],
        ] {
            assert_eq!(
                protocol_check(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()),
                Some(78)
            );
        }
        assert_eq!(
            protocol_check(&[
                "--check-protocol".into(),
                (ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION + 1).to_string()
            ]),
            Some(78)
        );
        assert_eq!(
            protocol_check(&[
                "--check-protocol".into(),
                ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string()
            ]),
            Some(0)
        );
        assert_eq!(
            protocol_check(&[
                "--check-protocol".into(),
                ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string(),
                "--copy-recovery".into()
            ]),
            Some(0)
        );
        assert_eq!(
            protocol_check(&[
                "--check-protocol".into(),
                ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string(),
                "--unknown-feature".into()
            ]),
            Some(69)
        );
        assert_eq!(protocol_check(&["--install".into()]), None);
    }

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

#[cfg(all(test, target_os = "windows"))]
mod windows_mode_tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|x| (*x).to_owned()).collect()
    }
    #[test]
    fn windows_helper_requires_bound_pipe_mode() {
        let nonce = "a".repeat(32);
        for input in [
            args(&["--stdio-json"]),
            args(&["--local-pipe", &nonce, "--parent-pid", "1"]),
            args(&[
                "--local-pipe",
                &nonce,
                "--parent-pid",
                "1",
                "--parent-start",
                "0",
            ]),
            args(&[
                "--local-pipe",
                &nonce,
                "--parent-pid",
                "1",
                "--parent-start",
                "bad",
            ]),
        ] {
            assert!(parse_mode(input).is_err());
        }
        assert!(matches!(
            parse_mode(args(&[
                "--local-pipe",
                &nonce,
                "--parent-pid",
                "1",
                "--parent-start",
                "123"
            ])),
            Ok(IoMode::Pipe {
                parent_start: 123,
                ..
            })
        ));
        let version = ottrin_core::PRIVILEGED_HELPER_PROTOCOL_VERSION.to_string();
        assert_eq!(
            protocol_check(&args(&["--check-protocol", &version, "--bound-local-pipe"])),
            Some(0)
        );
        assert_eq!(
            protocol_check(&args(&["--check-protocol", &version, "--local-pipe"])),
            Some(69)
        );
    }
}
