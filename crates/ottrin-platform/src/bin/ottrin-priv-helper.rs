use ottrin_core::{
    EntryKind, FileCommand, FileEntry, PrivilegedCommand, PrivilegedPayload, PrivilegedRequest,
    PrivilegedResponse, PrivilegedStatus,
};
use ottrin_platform::{DefaultPlatform, PlatformOps};
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::path::Path;

enum IoMode {
    Stdio,
    Files { request_file: PathBuf, response_file: PathBuf },
}

fn main() {
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
        IoMode::Files { request_file, .. } => {
            std::fs::read(request_file)
                .map_err(|e| format!("Failed to read request file: {}", e))?
        }
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

fn write_response(mode: &IoMode, resp: PrivilegedResponse) {
    match serde_json::to_vec(&resp) {
        Ok(bytes) => {
            match mode {
                IoMode::Stdio => {
                    let _ = std::io::stdout().write_all(&bytes);
                    let _ = std::io::stdout().flush();
                }
                IoMode::Files { response_file, .. } => {
                    let _ = std::fs::write(response_file, bytes);
                }
            }
        }
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
        let size_bytes = if matches!(kind, EntryKind::File) { Some(meta.len()) } else { None };
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
