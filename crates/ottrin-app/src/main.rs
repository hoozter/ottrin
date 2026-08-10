#![cfg_attr(windows, windows_subsystem = "windows")]

use eframe::{NativeOptions, egui};
use ottrin_core::{
    ConflictAction, FileCommand, GraphicsEvidence, GraphicsRendererChoice, GraphicsRuntimeStatus,
    SearchIndexStatus, SearchQuery, format_size, select_graphics_renderer,
};
use ottrin_platform::{DefaultPlatform, PlatformOps, set_privileged_helper_override};
use ottrin_ui::{OttrinApp, clear_preview_cache_storage, load_config, remove_config_file};
use std::collections::HashSet;
#[cfg(any(target_os = "windows", test))]
use std::fs::OpenOptions;
#[cfg(any(target_os = "windows", test))]
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadlessAction {
    IndexUpdate { json: bool },
    IndexStatus { json: bool },
    IndexRebuild { json: bool },
    Search { query: String, json: bool },
    Preview(PathBuf),
    DropStatus,
    DropCopy(Vec<PathBuf>),
    Invalid(String),
}

fn renderer_for_selection(is_windows: bool, choice: GraphicsRendererChoice) -> eframe::Renderer {
    if choice == GraphicsRendererChoice::OpenGlCompatibility || !is_windows {
        eframe::Renderer::Glow
    } else {
        eframe::Renderer::Wgpu
    }
}

fn main_viewport_transparent(is_windows: bool) -> bool {
    !is_windows
}

fn graphics_runtime_status(
    command_line_opengl: bool,
    config: &ottrin_core::AppConfig,
) -> GraphicsRuntimeStatus {
    let is_windows = cfg!(target_os = "windows");
    let evidence = if is_windows {
        windows_graphics_evidence()
    } else {
        GraphicsEvidence::default()
    };
    GraphicsRuntimeStatus {
        is_windows,
        selection: select_graphics_renderer(
            command_line_opengl,
            config.graphics_renderer,
            evidence,
        ),
    }
}

#[cfg(target_os = "windows")]
fn windows_graphics_evidence() -> GraphicsEvidence {
    use windows_sys::Win32::Graphics::Gdi::{
        DISPLAY_DEVICE_MIRRORING_DRIVER, DISPLAY_DEVICE_REMOTE, DISPLAY_DEVICEW,
        EnumDisplayDevicesW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

    let remote_session = unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 };
    let mut virtual_or_problem_adapter = false;
    for index in 0..32 {
        let mut adapter = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut adapter, 0) } == 0 {
            break;
        }
        let description = String::from_utf16_lossy(&adapter.DeviceString)
            .trim_end_matches('\0')
            .to_ascii_lowercase();
        let flagged =
            adapter.StateFlags & (DISPLAY_DEVICE_MIRRORING_DRIVER | DISPLAY_DEVICE_REMOTE) != 0;
        let recognized_name = [
            "microsoft basic display",
            "vmware",
            "virtualbox",
            "qxl",
            "parallels",
            "virtual display",
        ]
        .iter()
        .any(|name| description.contains(name));
        if flagged || recognized_name {
            virtual_or_problem_adapter = true;
            break;
        }
    }
    GraphicsEvidence {
        remote_session,
        virtual_or_problem_adapter,
    }
}

#[cfg(not(target_os = "windows"))]
fn windows_graphics_evidence() -> GraphicsEvidence {
    GraphicsEvidence::default()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let preview_path = match cli_headless_action(&args) {
        Some(HeadlessAction::Preview(path)) => match resolve_preview_path(&path) {
            Ok(path) => Some(path),
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(2);
            }
        },
        _ => None,
    };
    if preview_path.is_none() && handle_cli_action() {
        return;
    }
    install_windows_startup_panic_hook();
    if let Err(error) = run_gui(preview_path) {
        eprintln!("ottrin: {error}");
        report_windows_startup_failure("Ottrin could not start", &error.to_string());
        std::process::exit(1);
    }
}

fn run_gui(preview_path: Option<PathBuf>) -> eframe::Result<()> {
    apply_cli_overrides();
    ensure_dev_helper();

    let renderer_override = match graphics_renderer_override() {
        Ok(renderer_override) => renderer_override,
        Err(error) => {
            eprintln!("ottrin: {error}");
            report_windows_startup_failure("Ottrin could not select a renderer", &error);
            std::process::exit(2);
        }
    };

    let config = load_config();
    let graphics_runtime = graphics_runtime_status(renderer_override.is_some(), &config);
    let window_size = config.window_size.unwrap_or([1024.0, 680.0]);

    let options = NativeOptions {
        renderer: renderer_for_selection(
            graphics_runtime.is_windows,
            graphics_runtime.selection.choice,
        ),
        viewport: egui::ViewportBuilder::default()
            .with_title("Ottrin")
            .with_visible(preview_path.is_none())
            .with_decorations(false)
            .with_transparent(main_viewport_transparent(graphics_runtime.is_windows))
            .with_inner_size(window_size)
            .with_min_inner_size([800.0, 500.0])
            .with_icon(load_window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Ottrin",
        options,
        Box::new(move |cc| {
            let mut app = OttrinApp::new_with_graphics_status(cc, graphics_runtime);
            if let Some(path) = preview_path {
                app.open_quick_inspector_file(path);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(target_os = "windows")]
fn startup_failure_log_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Ottrin")
}

#[cfg(any(target_os = "windows", test))]
fn write_startup_failure_log_in(
    directory: &std::path::Path,
    summary: &str,
    details: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("startup.log");
    let mut log = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(log, "Ottrin startup failure")?;
    writeln!(log, "Summary: {summary}")?;
    writeln!(log, "Details: {details}")?;
    writeln!(log)?;
    log.sync_data()?;
    Ok(path)
}

fn report_windows_startup_failure(summary: &str, details: &str) {
    #[cfg(target_os = "windows")]
    {
        let log_path =
            write_startup_failure_log_in(&startup_failure_log_directory(), summary, details).ok();
        let location = log_path.as_deref().map_or_else(
            || "the local Ottrin data directory".to_string(),
            |path| path.display().to_string(),
        );
        show_windows_startup_failure_dialog(&format!(
            "Ottrin could not start. Details were saved to:\n{location}"
        ));
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (summary, details);
}

#[cfg(target_os = "windows")]
fn install_windows_startup_panic_hook() {
    std::panic::set_hook(Box::new(|panic| {
        report_windows_startup_failure(
            "Ottrin encountered a fatal startup error",
            &panic.to_string(),
        );
    }));
}

#[cfg(not(target_os = "windows"))]
fn install_windows_startup_panic_hook() {}

#[cfg(target_os = "windows")]
fn show_windows_startup_failure_dialog(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title: Vec<u16> = "Ottrin startup error"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let body: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_OK,
        );
    }
}

fn handle_cli_action() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_cli_help();
        return true;
    }
    if let Some(action) = cli_headless_action(&args) {
        if let Err(error) = run_headless_action(action) {
            eprintln!("ottrin: {error}");
            std::process::exit(1);
        }
        return true;
    }
    if !args.iter().any(|arg| arg == "--uninstall") {
        return false;
    }

    let purge_config = args.iter().any(|arg| arg == "--purge-config");
    let mut freed = 0_u64;
    let mut errors = Vec::new();

    match ottrin_search::SearchService::uninstall_sqlite_schedule() {
        Ok(msg) => println!("{msg}"),
        Err(err) => errors.push(err),
    }
    match ottrin_search::clear_search_storage() {
        Ok(bytes) => {
            freed = freed.saturating_add(bytes);
            println!("Removed search storage: {}", format_size(bytes));
        }
        Err(err) => errors.push(err),
    }
    let preview_freed = clear_preview_cache_storage();
    freed = freed.saturating_add(preview_freed);
    println!("Removed thumbnail cache: {}", format_size(preview_freed));
    match remove_terminal_launcher_file() {
        Ok(msg) => println!("{msg}"),
        Err(err) => errors.push(err),
    }
    if purge_config {
        match remove_config_file() {
            Ok(bytes) => {
                freed = freed.saturating_add(bytes);
                println!("Removed preferences: {}", format_size(bytes));
            }
            Err(err) => errors.push(err),
        }
    } else {
        println!("Saved preferences kept. Use --uninstall --purge-config to remove them too.");
    }

    println!("Cleanup freed {}.", format_size(freed));
    if errors.is_empty() {
        true
    } else {
        eprintln!("Cleanup completed with errors:");
        for err in errors {
            eprintln!("  {err}");
        }
        std::process::exit(1);
    }
}

fn print_cli_help() {
    println!("Ottrin — find, inspect, and manage files");
    println!();
    println!("Usage:");
    println!("  ottrin                         Open the file manager");
    println!("  ottrin search <QUERY> [--json] Search indexed files (-s)");
    println!("  ottrin index [update] [--json]");
    println!("                                 Update the configured search index");
    println!("  ottrin index status [--json]   Show persisted index status without starting it");
    println!("  ottrin index rebuild [--json]");
    println!("                                 Build and atomically publish a fresh index");
    println!("  ottrin preview <FILE>          Open the full Quick Inspector (-p)");
    println!("  ottrin drop status             Show Drop Folder state");
    println!("  ottrin drop copy <PATH>…       Copy paths into the Drop Folder");
    println!();
    println!("Options:");
    println!("  --renderer opengl  Recovery override for a compatible OpenGL driver");
    println!("  --helper-path PATH Developer override for privileged helper discovery");
    println!("  --uninstall        Remove Ottrin background jobs and generated data");
    println!("  --purge-config     With --uninstall, also remove saved preferences");
}

fn cli_headless_action(args: &[String]) -> Option<HeadlessAction> {
    let command = args.first().map(String::as_str)?;
    match command {
        "search" | "-s" => {
            let json = args[1..].iter().any(|arg| arg == "--json");
            let values = args[1..]
                .iter()
                .filter(|arg| arg.as_str() != "--json")
                .collect::<Vec<_>>();
            match values.as_slice() {
                [query] if !query.is_empty() => Some(HeadlessAction::Search {
                    query: (*query).clone(),
                    json,
                }),
                [] => Some(HeadlessAction::Invalid(
                    "search requires a query".to_string(),
                )),
                _ => Some(HeadlessAction::Invalid(
                    "search accepts exactly one query argument".to_string(),
                )),
            }
        }
        "index" => Some(parse_index_action(&args[1..])),
        "preview" | "-p" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::Preview(PathBuf::from(path)),
            [] => HeadlessAction::Invalid("preview requires a file path".to_string()),
            _ => HeadlessAction::Invalid("preview accepts exactly one file path".to_string()),
        }),
        "drop" => Some(match args.get(1).map(String::as_str) {
            Some("status") if args.len() == 2 => HeadlessAction::DropStatus,
            Some("copy") => HeadlessAction::DropCopy(args[2..].iter().map(PathBuf::from).collect()),
            Some(value) => HeadlessAction::Invalid(format!("unknown drop action: {value}")),
            None => HeadlessAction::Invalid("drop requires status or copy".to_string()),
        }),
        "--renderer" | "--helper-path" | "--uninstall" | "--purge-config" => None,
        value if value.starts_with("--renderer=") || value.starts_with("--helper-path=") => None,
        _ => Some(HeadlessAction::Invalid(format!(
            "unknown command: {command}"
        ))),
    }
}

fn parse_index_action(args: &[String]) -> HeadlessAction {
    let mut action = "update";
    let mut action_seen = false;
    let mut json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "update" | "status" | "rebuild" if !action_seen => {
                action = args[index].as_str();
                action_seen = true;
            }
            "update" | "status" | "rebuild" => {
                return HeadlessAction::Invalid(
                    "index accepts one action: update, status, or rebuild".to_string(),
                );
            }
            "--json" if !json => json = true,
            "--json" => {
                return HeadlessAction::Invalid("--json may be specified only once".to_string());
            }

            value if value.starts_with('-') => {
                return HeadlessAction::Invalid(format!("unknown index option: {value}"));
            }
            value => {
                return HeadlessAction::Invalid(format!("unknown index action: {value}"));
            }
        }
        index += 1;
    }
    match action {
        "status" => HeadlessAction::IndexStatus { json },
        "rebuild" => HeadlessAction::IndexRebuild { json },
        _ => HeadlessAction::IndexUpdate { json },
    }
}

fn configured_search_service() -> ottrin_search::SearchService {
    ottrin_search::SearchService::new(configured_search_config())
}

fn configured_search_config() -> ottrin_core::SearchConfig {
    let mut config = load_config().search;
    if config.include_roots.is_empty() {
        config.include_roots = ottrin_search::default_roots();
    }
    config
}

fn resolve_preview_path(path: &std::path::Path) -> Result<PathBuf, String> {
    let resolved = std::fs::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!("preview file does not exist: {}", path.display())
        }
        _ => format!("could not resolve preview file {}: {error}", path.display()),
    })?;
    if !resolved.is_file() {
        return Err(format!(
            "preview path is not a file: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn run_headless_action(action: HeadlessAction) -> Result<(), String> {
    if let HeadlessAction::Invalid(error) = &action {
        return Err(error.clone());
    }
    match action {
        HeadlessAction::DropStatus => {
            print_drop_status(&load_config().target);
            return Ok(());
        }
        HeadlessAction::DropCopy(sources) => return run_drop_copy(sources),
        _ => {}
    }

    match action {
        HeadlessAction::IndexStatus { json } => {
            let service =
                ottrin_search::SearchService::load_persisted_default(configured_search_config())?;
            print_index_diagnostics(&service.diagnostics(), json)
        }
        HeadlessAction::Search { query, json } => {
            let config = configured_search_config();
            let service = ottrin_search::SearchService::load_persisted_default(config.clone())?;
            let response = service.query(SearchQuery {
                text: query,
                scope: config.default_scope,
                root_path: None,
                include_hidden_system: config.include_hidden_system,
                sort: config.default_sort,
                limit: config.result_limit.max(1),
                offset: 0,
                content_search: false,
            });
            print_search_response(response, json)
        }
        HeadlessAction::IndexUpdate { json } => run_index_writer(json, false),
        HeadlessAction::IndexRebuild { json } => run_index_writer(json, true),
        HeadlessAction::Preview(_) => {
            unreachable!("preview starts the inspector before headless routing")
        }
        HeadlessAction::DropStatus | HeadlessAction::DropCopy(_) => {
            unreachable!("drop actions return before search routing")
        }
        HeadlessAction::Invalid(_) => unreachable!("invalid actions return before search routing"),
    }
}

fn run_index_writer(json: bool, rebuild: bool) -> Result<(), String> {
    let mut service = configured_search_service();
    service.start();
    let startup = service.diagnostics();
    if matches!(startup.status, SearchIndexStatus::Unavailable) {
        service.stop();
        return Err(startup
            .last_error
            .unwrap_or_else(|| "search service unavailable".to_string()));
    }
    let operation = if rebuild {
        service.rebuild_index_blocking()
    } else {
        service.update_index_blocking()
    };
    let result = operation.and_then(|()| print_index_diagnostics(&service.diagnostics(), json));
    service.stop();
    result
}

fn print_search_response(response: ottrin_core::SearchResponse, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&response)
                .map_err(|error| format!("could not encode search response: {error}"))?
        );
    } else {
        for item in &response.items {
            println!("{}", item.path.display());
        }
    }
    response.error.map_or(Ok(()), Err)
}

/// Executes only the existing production Copy command.  In particular, this
/// headless surface has no Move variant, never creates a Drop Folder, and asks
/// the platform layer to preflight every source before it publishes anything.
fn run_drop_copy(sources: Vec<PathBuf>) -> Result<(), String> {
    if sources.is_empty() {
        return Err("--drop copy requires at least one source path".to_string());
    }

    let destination = load_config()
        .target
        .current
        .ok_or_else(|| "no Drop Folder is configured".to_string())?;
    if !destination.is_dir() {
        return Err(format!(
            "configured Drop Folder is not an existing directory: {}",
            destination.display()
        ));
    }

    let mut seen = HashSet::new();
    for source in &sources {
        let identity = std::fs::canonicalize(source).unwrap_or_else(|_| source.clone());
        if !seen.insert(identity) {
            return Err(format!(
                "duplicate Drop Folder source: {}",
                source.display()
            ));
        }
    }

    DefaultPlatform
        .execute_command(&FileCommand::Copy {
            sources,
            destination,
            conflict: ConflictAction::Rename,
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn print_drop_status(target: &ottrin_core::TargetState) {
    println!("configured={}", target.current.is_some());
    println!("pinned={}", target.pinned.len());
    println!("recent={}", target.recent.len());
    if let Some(path) = &target.current {
        println!("path={}", path.display());
        println!("is_directory={}", path.is_dir());
    }
}

fn print_index_diagnostics(
    diagnostics: &ottrin_search::SearchIndexDiagnostics,
    json: bool,
) -> Result<(), String> {
    let status = match diagnostics.status {
        SearchIndexStatus::Unavailable => "unavailable",
        SearchIndexStatus::Indexing => "indexing",
        SearchIndexStatus::Ready => "ready",
    };
    if json {
        let value = serde_json::json!({
            "status": status,
            "indexed_items": diagnostics.indexed_items,
            "last_completed_indexed_items": diagnostics.last_completed_indexed_items,
            "estimated_total_items": diagnostics.estimated_total_items,
            "snapshot_revision": diagnostics.snapshot_revision,
            "configured_roots": diagnostics.configured_root_count,
            "watched_roots": diagnostics.watched_root_count,
            "watcher_ready": diagnostics.watcher_ready,
            "watcher_error": diagnostics.watcher_last_error,
            "active_root": diagnostics.active_root,
            "active_root_index": diagnostics.active_root_index,
            "total_roots": diagnostics.total_roots,
            "last_scan_started_unix_secs": diagnostics.last_scan_started_unix_secs,
            "last_scan_completed_unix_secs": diagnostics.last_scan_completed_unix_secs,
            "last_scan_duration_ms": diagnostics.last_scan_duration_ms,
            "last_progress_unix_secs": diagnostics.last_progress_unix_secs,
            "detail": diagnostics.detail,
            "error": diagnostics.last_error,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value)
                .map_err(|error| format!("could not encode index status: {error}"))?
        );
        return Ok(());
    }

    println!("Index: {status}");
    println!("Indexed: {} files", diagnostics.indexed_items);
    if let Some(estimated) = diagnostics.estimated_total_items {
        println!("Estimated total: ~{estimated}");
    }
    println!("Roots configured: {}", diagnostics.configured_root_count);
    println!("Roots watched: {}", diagnostics.watched_root_count);
    println!(
        "Filesystem watching: {}",
        if diagnostics.watcher_ready {
            "active"
        } else {
            "unavailable"
        }
    );
    if let Some(detail) = &diagnostics.detail {
        println!("Detail: {detail}");
    }
    if let Some(error) = diagnostics
        .last_error
        .as_ref()
        .or(diagnostics.watcher_last_error.as_ref())
    {
        println!("Warning: {error}");
    }
    Ok(())
}

fn graphics_renderer_override() -> Result<Option<String>, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    validate_renderer_override(
        cli_renderer_override(&args)
            .or_else(|| std::env::var("OTTRIN_RENDERER").ok())
            .as_deref(),
    )
}

fn validate_renderer_override(renderer_override: Option<&str>) -> Result<Option<String>, String> {
    match renderer_override.map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) if value.eq_ignore_ascii_case("opengl") => Ok(Some("opengl".to_string())),
        Some(value) => Err(format!(
            "unsupported renderer override `{value}`; expected `opengl`"
        )),
    }
}

fn cli_renderer_override(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--renderer" {
            if let Some(value) = args.get(index + 1).filter(|value| !value.is_empty()) {
                return Some(value.clone());
            }
            index += 1;
        } else if let Some(value) = args[index].strip_prefix("--renderer=")
            && !value.is_empty()
        {
            return Some(value.to_owned());
        }
        index += 1;
    }
    None
}

fn remove_terminal_launcher_file() -> Result<String, String> {
    let path = if cfg!(target_os = "windows") {
        ottrin_core::default_home_dir().join("ottrin.bat")
    } else {
        ottrin_core::default_home_dir()
            .join(".local")
            .join("bin")
            .join("ottrin")
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(format!("Removed terminal launcher at {}.", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok("No terminal launcher was installed.".to_string())
        }
        Err(err) => Err(format!("Failed to remove {}: {}", path.display(), err)),
    }
}

fn load_window_icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icon.png");
    let image = image::load_from_memory(bytes)
        .expect("embedded Ottrin icon should decode")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn apply_cli_overrides() {
    let args: Vec<String> = std::env::args().collect();
    let mut helper_path: Option<PathBuf> = None;
    let mut i = 1usize;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--helper-path" {
            if i + 1 < args.len() {
                helper_path = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
        } else if let Some(v) = arg.strip_prefix("--helper-path=")
            && !v.is_empty()
        {
            helper_path = Some(PathBuf::from(v));
        }
        i += 1;
    }
    if let Some(path) = helper_path {
        set_privileged_helper_override(Some(path));
    }
}

fn ensure_dev_helper() {
    if !cfg!(debug_assertions) {
        return;
    }
    if std::env::var("OTTRIN_PRIV_HELPER").is_ok() {
        return;
    }
    if std::env::var("OTTRIN_DEV_HELPER_AUTOBUILD")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        .unwrap_or(false)
    {
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let helper_name = "ottrin-priv-helper";
    let build = std::process::Command::new("cargo")
        .args(["build", "-p", "ottrin-platform", "--bin", helper_name])
        .arg("--message-format=json-render-diagnostics")
        .current_dir(&manifest_dir)
        .output();

    match build {
        Ok(output) if output.status.success() => {
            let helper_path = match cargo_helper_artifact_from_messages(
                &output.stdout,
                cfg!(target_os = "windows"),
            ) {
                Ok(path) if path.is_file() => path,
                Ok(path) => {
                    eprintln!(
                        "ottrin: Cargo reported the development helper at {}, but it was not a regular file; refusing to probe another target directory",
                        path.display()
                    );
                    return;
                }
                Err(error) => {
                    eprintln!(
                        "ottrin: Cargo built the development helper but did not report one bounded executable artifact: {error}"
                    );
                    return;
                }
            };
            set_privileged_helper_override(Some(helper_path));
        }
        Ok(output) => {
            eprintln!(
                "ottrin: Cargo could not build the development helper: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Err(error) => {
            eprintln!("ottrin: could not start Cargo for the development helper: {error}");
        }
    }
}

fn cargo_helper_artifact_from_messages(
    messages: &[u8],
    is_windows: bool,
) -> Result<PathBuf, String> {
    let helper_name = if is_windows {
        "ottrin-priv-helper.exe"
    } else {
        "ottrin-priv-helper"
    };
    let mut helper_path = None;
    for line in messages
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let message: serde_json::Value = serde_json::from_slice(line)
            .map_err(|error| format!("invalid Cargo message: {error}"))?;
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact")
            || message
                .get("target")
                .and_then(|target| target.get("name"))
                .and_then(serde_json::Value::as_str)
                != Some("ottrin-priv-helper")
        {
            continue;
        }
        let executable = message
            .get("executable")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| "helper compiler artifact omitted executable".to_string())?;
        let path = PathBuf::from(executable);
        if path.file_name().and_then(|name| name.to_str()) != Some(helper_name) {
            return Err("helper compiler artifact named a different executable".to_string());
        }
        if helper_path.replace(path).is_some() {
            return Err("Cargo reported multiple helper executable artifacts".to_string());
        }
    }
    helper_path.ok_or_else(|| "Cargo did not report the helper executable artifact".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_renderer_uses_wgpu_on_windows_and_glow_elsewhere() {
        assert_eq!(
            renderer_for_selection(true, GraphicsRendererChoice::Native),
            eframe::Renderer::Wgpu
        );
        assert_eq!(
            renderer_for_selection(false, GraphicsRendererChoice::Native),
            eframe::Renderer::Glow
        );
    }

    #[test]
    fn graphics_recovery_override_can_select_opengl_without_changing_windows_default() {
        assert_eq!(
            renderer_for_selection(true, GraphicsRendererChoice::OpenGlCompatibility),
            eframe::Renderer::Glow
        );
    }

    #[test]
    fn invalid_renderer_override_is_rejected_instead_of_silently_using_wgpu() {
        assert_eq!(
            validate_renderer_override(Some("vulkan")),
            Err("unsupported renderer override `vulkan`; expected `opengl`".to_string())
        );
    }

    #[test]
    fn windows_composition_is_opaque_while_other_platforms_keep_transparency() {
        assert!(!main_viewport_transparent(true));
        assert!(main_viewport_transparent(false));
    }

    #[test]
    fn graphics_recovery_override_accepts_both_cli_forms() {
        assert_eq!(
            cli_renderer_override(&["--renderer".into(), "opengl".into()]),
            Some("opengl".into())
        );
        assert_eq!(
            cli_renderer_override(&["--renderer=opengl".into()]),
            Some("opengl".into())
        );
    }

    #[test]
    fn cargo_helper_artifact_parser_uses_cargo_reported_executable_for_all_target_modes() {
        let helper_artifact = |executable: &str| {
            serde_json::json!({
                "reason": "compiler-artifact",
                "target": {"name": "ottrin-priv-helper"},
                "executable": executable,
            })
            .to_string()
        };

        for (mode, executable) in [
            ("default", "/repo/target/debug/ottrin-priv-helper"),
            (
                "CARGO_TARGET_DIR",
                "/tmp/ottrin-target/debug/ottrin-priv-helper",
            ),
            (
                "CARGO_BUILD_TARGET",
                "/repo/target/x86_64-unknown-linux-gnu/debug/ottrin-priv-helper",
            ),
            (
                "CARGO_TARGET_DIR + CARGO_BUILD_TARGET",
                "/tmp/ottrin-target/x86_64-unknown-linux-gnu/debug/ottrin-priv-helper",
            ),
        ] {
            assert_eq!(
                cargo_helper_artifact_from_messages(
                    format!("{}\n", helper_artifact(executable)).as_bytes(),
                    false,
                ),
                Ok(PathBuf::from(executable)),
                "{mode} must use Cargo's reported executable instead of reconstructing its target path"
            );
        }
    }

    #[test]
    fn cargo_helper_artifact_parser_rejects_missing_or_wrong_artifacts() {
        assert!(cargo_helper_artifact_from_messages(b"", false).is_err());
        assert!(
            cargo_helper_artifact_from_messages(
                serde_json::json!({
                    "reason": "compiler-artifact",
                    "target": {"name": "other-bin"},
                    "executable": "/tmp/other-bin",
                })
                .to_string()
                .as_bytes(),
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn canonical_drop_actions_are_recognized_before_the_gui_starts() {
        assert_eq!(
            cli_headless_action(&["drop".into(), "status".into()]),
            Some(HeadlessAction::DropStatus)
        );
        assert_eq!(
            cli_headless_action(&[
                "drop".into(),
                "copy".into(),
                "one.txt".into(),
                "two.txt".into(),
            ]),
            Some(HeadlessAction::DropCopy(vec![
                PathBuf::from("one.txt"),
                PathBuf::from("two.txt"),
            ]))
        );
    }

    #[test]
    fn canonical_search_and_index_commands_are_recognized_before_the_gui_starts() {
        for command in ["search", "-s"] {
            assert_eq!(
                cli_headless_action(&[command.into(), "invoice".into()]),
                Some(HeadlessAction::Search {
                    query: "invoice".into(),
                    json: false,
                })
            );
        }
        assert_eq!(
            cli_headless_action(&["search".into(), "invoice".into(), "--json".into()]),
            Some(HeadlessAction::Search {
                query: "invoice".into(),
                json: true,
            })
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "status".into()]),
            Some(HeadlessAction::IndexStatus { json: false })
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "rebuild".into(), "--json".into()]),
            Some(HeadlessAction::IndexRebuild { json: true })
        );
        assert_eq!(
            cli_headless_action(&["index".into()]),
            Some(HeadlessAction::IndexUpdate { json: false })
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "update".into()]),
            Some(HeadlessAction::IndexUpdate { json: false })
        );
    }

    #[test]
    fn unowned_index_modifiers_are_rejected() {
        assert_eq!(
            cli_headless_action(&[
                "index".into(),
                "update".into(),
                "--root".into(),
                "/srv/files".into(),
            ]),
            Some(HeadlessAction::Invalid(
                "unknown index option: --root".to_string()
            ))
        );
    }

    #[test]
    fn canonical_preview_command_and_alias_preserve_the_requested_path() {
        for command in ["preview", "-p"] {
            assert_eq!(
                cli_headless_action(&[command.into(), "fixture.pdf".into()]),
                Some(HeadlessAction::Preview(PathBuf::from("fixture.pdf")))
            );
        }
    }

    #[test]
    fn preview_path_resolution_rejects_missing_and_non_file_paths() {
        let root =
            std::env::temp_dir().join(format!("ottrin-cli-preview-path-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create preview root");
        let file = root.join("fixture.txt");
        std::fs::write(&file, "preview\n").expect("write preview fixture");

        assert_eq!(
            resolve_preview_path(&file).expect("resolve file"),
            std::fs::canonicalize(&file).expect("canonical fixture")
        );
        assert!(
            resolve_preview_path(&root)
                .unwrap_err()
                .contains("not a file")
        );
        assert!(
            resolve_preview_path(&root.join("missing.txt"))
                .unwrap_err()
                .contains("does not exist")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_cli_syntax_is_rejected_instead_of_opening_the_gui() {
        assert_eq!(
            cli_headless_action(&["frobnicate".into()]),
            Some(HeadlessAction::Invalid(
                "unknown command: frobnicate".to_string()
            ))
        );
        assert_eq!(
            cli_headless_action(&["search".into(), "one".into(), "two".into()]),
            Some(HeadlessAction::Invalid(
                "search accepts exactly one query argument".to_string()
            ))
        );
    }

    #[test]
    fn startup_failure_diagnostic_is_durable_and_names_its_location() {
        let directory = std::env::temp_dir().join(format!(
            "ottrin-startup-diagnostic-test-{}",
            std::process::id()
        ));
        let log = write_startup_failure_log_in(
            &directory,
            "WGPU initialization failed",
            "No wgpu backend feature that is implemented for the target platform was enabled.",
        )
        .expect("startup diagnostic should be written");

        assert_eq!(log, directory.join("startup.log"));
        let contents = std::fs::read_to_string(log).expect("startup diagnostic should be readable");
        assert!(contents.contains("Ottrin startup failure"));
        assert!(contents.contains("Summary: WGPU initialization failed"));
        assert!(contents.contains("Details: No wgpu backend feature"));
    }
}
