#![cfg_attr(windows, windows_subsystem = "windows")]

use eframe::{NativeOptions, egui};
use ottrin_core::{
    ConflictAction, FileCommand, GraphicsEvidence, GraphicsRendererChoice, GraphicsRuntimeStatus,
    SearchIndexStatus, SearchQuery, SearchScope, format_size, select_graphics_renderer,
};
use ottrin_platform::{DefaultPlatform, PlatformOps, set_privileged_helper_override};
use ottrin_ui::{
    OttrinApp, administrator_surface_label, clear_preview_cache_storage, load_config,
    remove_config_file,
};
use std::collections::HashSet;
#[cfg(any(target_os = "windows", test))]
use std::fs::OpenOptions;
#[cfg(any(target_os = "windows", test))]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
enum GuiLaunchTarget {
    QuickInspector(PathBuf),
    Directory(PathBuf),
    Select(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadlessAction {
    IndexUpdate {
        json: bool,
    },
    IndexStatus {
        json: bool,
    },
    IndexRebuild {
        json: bool,
    },
    IndexServiceInstall,
    IndexServiceStatus,
    IndexServiceRestart,
    IndexServiceRemove,
    Search {
        query: String,
        json: bool,
        content: bool,
        limit: Option<usize>,
        offset: usize,
    },
    Preview(PathBuf),
    OpenPath(PathBuf),
    SelectPath(PathBuf),
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

/// Fit the initial main window to usable desktop space, excluding taskbars.
/// An explicit position is left to the caller/window manager.
#[cfg(any(target_os = "windows", test))]
fn fit_initial_viewport(
    mut viewport: egui::ViewportBuilder,
    work_area: egui::Rect,
) -> egui::ViewportBuilder {
    if viewport.position.is_some() {
        return viewport;
    }
    let available = work_area.size();
    let requested = viewport.inner_size.unwrap_or(egui::vec2(1024.0, 680.0));
    let minimum = viewport
        .min_inner_size
        .unwrap_or(egui::Vec2::ZERO)
        .min(available);
    let size = requested.max(minimum).min(available);
    if viewport.min_inner_size.is_some() {
        viewport.min_inner_size = Some(minimum);
    }
    viewport
        .with_inner_size(size)
        .with_position(work_area.min + (available - size) * 0.5)
}

#[cfg(target_os = "windows")]
fn windows_initial_window_options(
    mut options: NativeOptions,
    standalone_inspector: bool,
) -> NativeOptions {
    if !standalone_inspector {
        options.window_builder = Some(Box::new(|viewport| {
            use windows_sys::Win32::Foundation::POINT;
            use windows_sys::Win32::Graphics::Gdi::{
                GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
            };
            use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
            // Eframe invokes this hook after Winit establishes DPI awareness.
            // SAFETY: monitor lookup has no pointer inputs or ownership transfer.
            let monitor =
                unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
            // SAFETY: MONITORINFO permits zero initialization; cbSize is required.
            let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            // SAFETY: info is a writable, correctly sized structure.
            if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
                eprintln!(
                    "ottrin: Cannot read desktop work area: {}",
                    std::io::Error::last_os_error()
                );
                return viewport;
            }
            // SAFETY: querying the current system DPI has no preconditions.
            let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
            let r = info.rcWork;
            if scale <= 0.0 || r.right <= r.left || r.bottom <= r.top {
                eprintln!("ottrin: Invalid desktop work area or DPI; using native placement");
                return viewport;
            }
            let work_area = egui::Rect::from_min_max(
                egui::pos2(r.left as f32 / scale, r.top as f32 / scale),
                egui::pos2(r.right as f32 / scale, r.bottom as f32 / scale),
            );
            fit_initial_viewport(viewport, work_area)
        }));
    }
    options
}

fn main_viewport_transparent(is_windows: bool) -> bool {
    !is_windows
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MainViewportGeometry {
    visible: bool,
    inner_size: [f32; 2],
    min_inner_size: [f32; 2],
}

fn main_viewport_geometry(
    standalone_inspector: bool,
    configured_size: [f32; 2],
) -> MainViewportGeometry {
    if standalone_inspector {
        // Eframe secondary viewports need their root viewport to remain mapped on Linux.
        // A transparent 1×1 root keeps the Quick Inspector painting without exposing
        // the otherwise-unused file-manager window.
        MainViewportGeometry {
            visible: true,
            inner_size: [1.0, 1.0],
            min_inner_size: [1.0, 1.0],
        }
    } else {
        MainViewportGeometry {
            visible: true,
            inner_size: configured_size,
            min_inner_size: [800.0, 500.0],
        }
    }
}

#[cfg(unix)]
fn application_window_title_for_unix_user(effective_uid: u32) -> String {
    administrator_surface_label("Ottrin", effective_uid == 0)
}

fn application_window_title() -> String {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and no side effects.
        application_window_title_for_unix_user(unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        administrator_surface_label("Ottrin", false)
    }
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
    #[cfg(unix)]
    if args.as_slice() == ["--index-daemon"] {
        use std::os::unix::process::CommandExt;
        let error = index_daemon_executable()
            .map(|executable| Command::new(executable).args(&args).exec())
            .unwrap_or_else(|error| error);
        eprintln!("ottrin index daemon: {error}");
        std::process::exit(1);
    }
    let gui_target = match cli_headless_action(&args) {
        Some(HeadlessAction::Preview(path)) => match resolve_preview_path(&path) {
            Ok(path) => Some(GuiLaunchTarget::QuickInspector(path)),
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(2);
            }
        },
        Some(HeadlessAction::OpenPath(path)) => match resolve_direct_open_path(&path) {
            Ok(target) => Some(target),
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(2);
            }
        },
        Some(HeadlessAction::SelectPath(path)) => match resolve_preview_path(&path) {
            Ok(path) => Some(GuiLaunchTarget::Select(path)),
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(2);
            }
        },
        _ => None,
    };
    if gui_target.is_none() && handle_cli_action() {
        return;
    }
    #[cfg(unix)]
    if should_ensure_index_daemon(unsafe { libc::geteuid() })
        && let Err(error) = ensure_index_daemon()
    {
        eprintln!("ottrin: {error}");
        std::process::exit(1);
    }
    #[cfg(target_os = "windows")]
    if let Err(error) = ensure_index_daemon() {
        report_windows_startup_failure("Ottrin index could not start", &error);
        std::process::exit(1);
    }
    install_windows_startup_panic_hook();
    if let Err(error) = run_gui(gui_target) {
        eprintln!("ottrin: {error}");
        report_windows_startup_failure("Ottrin could not start", &error.to_string());
        std::process::exit(1);
    }
}

fn run_gui(gui_target: Option<GuiLaunchTarget>) -> eframe::Result<()> {
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

    let standalone_inspector = matches!(gui_target, Some(GuiLaunchTarget::QuickInspector(_)));
    let main_viewport = main_viewport_geometry(standalone_inspector, window_size);
    let window_title = application_window_title();
    let options = NativeOptions {
        renderer: renderer_for_selection(
            graphics_runtime.is_windows,
            graphics_runtime.selection.choice,
        ),
        viewport: egui::ViewportBuilder::default()
            .with_app_id(ottrin_ui::DESKTOP_APPLICATION_ID)
            .with_title(window_title.clone())
            .with_visible(main_viewport.visible)
            .with_decorations(false)
            .with_transparent(main_viewport_transparent(graphics_runtime.is_windows))
            .with_inner_size(main_viewport.inner_size)
            .with_min_inner_size(main_viewport.min_inner_size)
            .with_icon(load_window_icon()),
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    let options = windows_initial_window_options(options, standalone_inspector);
    eframe::run_native(
        &window_title,
        options,
        Box::new(move |cc| {
            let mut app = OttrinApp::new_with_graphics_status(cc, graphics_runtime);
            match gui_target {
                Some(GuiLaunchTarget::QuickInspector(path)) => {
                    app.open_quick_inspector_file(path);
                }
                Some(GuiLaunchTarget::Directory(path)) => {
                    app.open_directory_at_startup(path);
                }
                Some(GuiLaunchTarget::Select(path)) => {
                    app.select_file_at_startup(path);
                }
                None => {}
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

    // A blocked thumbnail cache must not dismantle a usable index/service.
    // This expendable cache is the first cleanup boundary; report its failure
    // before changing the background task or saved search storage.
    match clear_preview_cache_storage() {
        Ok(bytes) => {
            freed = freed.saturating_add(bytes);
            println!("Removed thumbnail cache: {}", format_size(bytes));
        }
        Err(error) => {
            eprintln!("ottrin: {error}");
            std::process::exit(1);
        }
    }
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
    println!("  ottrin <FILE>                  Open the file directly in Quick Inspector");
    println!("  ottrin <DIRECTORY>             Open the file manager in that directory");
    println!("  ottrin --select <FILE>         Open the parent folder with the file selected");
    println!("  ottrin search <QUERY> [--json] Search indexed files (-s)");
    println!("                       [--content] Also search the configured content folders");
    println!("                       [--limit N] [--offset N] Select a result page");
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

fn parse_search_action(args: &[String]) -> HeadlessAction {
    let (mut json, mut content, mut limit, mut offset, mut query) =
        (false, false, None, None, None);
    let mut args = args.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--json" => json = true,
            "--content" => content = true,
            "--limit" | "--offset" => {
                let value = args.next().and_then(|value| value.parse::<usize>().ok());
                let valid = match (argument.as_str(), value) {
                    ("--limit", Some(value)) if value > 0 && limit.is_none() => {
                        limit = Some(value);
                        true
                    }
                    ("--offset", Some(value)) if offset.is_none() => {
                        offset = Some(value);
                        true
                    }
                    _ => false,
                };
                if !valid {
                    return HeadlessAction::Invalid(format!(
                        "{argument} requires one valid {} integer and cannot be repeated",
                        if argument == "--limit" {
                            "positive"
                        } else {
                            "nonnegative"
                        }
                    ));
                }
            }
            "--" => {
                if query.is_some() {
                    return HeadlessAction::Invalid(
                        "search accepts exactly one query argument".into(),
                    );
                }
                query = args.next().cloned();
                if args.next().is_some() {
                    return HeadlessAction::Invalid(
                        "search accepts exactly one query argument".into(),
                    );
                }
                break;
            }
            argument if argument.starts_with("--") => {
                return HeadlessAction::Invalid(format!("Unknown search option: {argument}"));
            }
            _ if query.is_none() => query = Some(argument.clone()),
            _ => {
                return HeadlessAction::Invalid("search accepts exactly one query argument".into());
            }
        }
    }
    match query {
        Some(query) if !query.is_empty() => HeadlessAction::Search {
            query,
            json,
            content,
            limit,
            offset: offset.unwrap_or(0),
        },
        _ => HeadlessAction::Invalid("search requires a query".into()),
    }
}

fn cli_headless_action(args: &[String]) -> Option<HeadlessAction> {
    let command = args.first().map(String::as_str)?;
    match command {
        "search" | "-s" => Some(parse_search_action(&args[1..])),
        "index" => Some(parse_index_action(&args[1..])),
        "preview" | "-p" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::Preview(PathBuf::from(path)),
            [] => HeadlessAction::Invalid("preview requires a file path".to_string()),
            _ => HeadlessAction::Invalid("preview accepts exactly one file path".to_string()),
        }),
        "--select" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::SelectPath(PathBuf::from(path)),
            [] => HeadlessAction::Invalid("--select requires a file path".to_string()),
            _ => HeadlessAction::Invalid("--select accepts exactly one file path".to_string()),
        }),
        "drop" => Some(match args.get(1).map(String::as_str) {
            Some("status") if args.len() == 2 => HeadlessAction::DropStatus,
            Some("copy") => HeadlessAction::DropCopy(args[2..].iter().map(PathBuf::from).collect()),
            Some(value) => HeadlessAction::Invalid(format!("unknown drop action: {value}")),
            None => HeadlessAction::Invalid("drop requires status or copy".to_string()),
        }),
        "--renderer" | "--helper-path" | "--uninstall" | "--purge-config" => None,
        value if value.starts_with("--renderer=") || value.starts_with("--helper-path=") => None,
        _ => Some(
            direct_open_path(args)
                .map(HeadlessAction::OpenPath)
                .unwrap_or_else(|| HeadlessAction::Invalid(format!("unknown command: {command}"))),
        ),
    }
}

fn direct_open_path(args: &[String]) -> Option<PathBuf> {
    let [value] = args else {
        return None;
    };
    if value.starts_with('-') {
        return None;
    }
    let path = PathBuf::from(value);
    let explicitly_path_like = path.is_absolute() || path.components().count() > 1;
    (path.exists() || explicitly_path_like).then_some(path)
}

fn parse_index_action(args: &[String]) -> HeadlessAction {
    let mut action = "update";
    let mut action_seen = false;
    let mut json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "update" | "status" | "rebuild" | "service-install" | "service-status"
            | "service-restart" | "service-remove"
                if !action_seen =>
            {
                action = args[index].as_str();
                action_seen = true;
            }
            "update" | "status" | "rebuild" | "service-install" | "service-status"
            | "service-restart" | "service-remove" => {
                return HeadlessAction::Invalid("index accepts exactly one action".to_string());
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
        "service-install" if !json => HeadlessAction::IndexServiceInstall,
        "service-status" if !json => HeadlessAction::IndexServiceStatus,
        "service-restart" if !json => HeadlessAction::IndexServiceRestart,
        "service-remove" if !json => HeadlessAction::IndexServiceRemove,
        action if action.starts_with("service-") => HeadlessAction::Invalid(
            "--json is available only for index update, status, and rebuild".to_string(),
        ),
        _ => HeadlessAction::IndexUpdate { json },
    }
}

fn configured_search_config() -> ottrin_core::SearchConfig {
    load_config().search
}

#[cfg(unix)]
fn should_ensure_index_daemon(effective_uid: u32) -> bool {
    effective_uid != 0
}

#[cfg(any(unix, target_os = "windows"))]
fn connected_index_daemon() -> Option<ottrin_search::SearchDaemonClient> {
    let client = ottrin_search::SearchDaemonClient::connect_default().ok()?;
    client.ping().ok()?;
    Some(client)
}

#[cfg(unix)]
fn index_daemon_executable() -> std::io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    if let Some(directory) = current.parent() {
        let dedicated = directory.join("ottrin-indexd");
        if dedicated.is_file() {
            return Ok(dedicated);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "ottrin-indexd is missing from this installation",
    ))
}

#[cfg(target_os = "linux")]
fn same_executable_content(
    left: &std::path::Path,
    right: &std::path::Path,
) -> std::io::Result<bool> {
    use std::io::Read;
    let mut left = std::fs::File::open(left)?;
    let mut right = std::fs::File::open(right)?;
    let lm = left.metadata()?;
    let rm = right.metadata()?;
    if (lm.dev(), lm.ino()) == (rm.dev(), rm.ino()) {
        return Ok(true);
    }
    if lm.len() != rm.len() {
        return Ok(false);
    }
    // Separate AppImage mounts have different device identities. Compare their
    // actual daemon bytes so opening another window does not trigger an upgrade.
    let mut lbuf = [0u8; 65536];
    let mut rbuf = [0u8; 65536];
    let mut remaining = lm.len();
    while remaining != 0 {
        let length = remaining.min(lbuf.len() as u64) as usize;
        left.read_exact(&mut lbuf[..length])?;
        right.read_exact(&mut rbuf[..length])?;
        if lbuf[..length] != rbuf[..length] {
            return Ok(false);
        }
        remaining -= length as u64;
    }
    Ok(true)
}

#[cfg(target_os = "linux")]
fn daemon_uses_executable(
    client: &ottrin_search::SearchDaemonClient,
    expected: &std::path::Path,
) -> Result<bool, String> {
    let pid = client.server_pid()?;
    same_executable_content(std::path::Path::new(&format!("/proc/{pid}/exe")), expected).map_err(
        |error| {
            format!("could not compare the running index daemon with this installation: {error}")
        },
    )
}

#[cfg(all(unix, not(target_os = "linux")))]
fn daemon_uses_executable(
    _client: &ottrin_search::SearchDaemonClient,
    _expected: &std::path::Path,
) -> Result<bool, String> {
    Ok(true)
}

#[cfg(unix)]
fn ensure_index_daemon() -> Result<ottrin_search::SearchDaemonClient, String> {
    let unit_path = ottrin_search::sqlite_systemd_service_path();
    if let Some(client) = connected_index_daemon() {
        let executable = index_daemon_executable()
            .map_err(|error| format!("could not locate the Ottrin index daemon: {error}"))?;
        if daemon_uses_executable(&client, &executable)? {
            return Ok(client);
        }
        if unit_path.is_file() {
            // Use the existing installer to prepare, stop and update the service
            // before starting it. A plain start can race an exiting active unit.
            ottrin_search::SearchService::install_sqlite_schedule(&configured_search_config())?;
        } else {
            client.prepare_upgrade_and_wait()?;
        }
    }

    if unit_path.is_file() {
        let status = Command::new("systemctl")
            .args(["--user", "start", "ottrin-index.service"])
            .status()
            .map_err(|error| format!("could not activate the Ottrin index service: {error}"))?;
        if !status.success() {
            return Err(format!(
                "installed Ottrin index service failed to start ({status}); inspect `systemctl --user status ottrin-index.service`"
            ));
        }
    } else {
        let executable = index_daemon_executable()
            .map_err(|error| format!("could not locate the Ottrin index daemon: {error}"))?;
        let mut daemon = Command::new(executable);
        daemon
            .arg("--index-daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        daemon
            .spawn()
            .map_err(|error| format!("could not start the session index daemon: {error}"))?;
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Some(client) = connected_index_daemon() {
            return Ok(client);
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err("Ottrin index daemon did not become ready within 30 seconds".to_string())
}

#[cfg(target_os = "windows")]
fn ensure_index_daemon() -> Result<ottrin_search::SearchDaemonClient, String> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = current
        .parent()
        .ok_or("Cannot locate the application directory")?
        .join("ottrin-indexd.exe");
    ottrin_search::ensure_windows_search_daemon(&executable)
}

fn resolve_direct_open_path(path: &std::path::Path) -> Result<GuiLaunchTarget, String> {
    let resolved = std::fs::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => format!("path does not exist: {}", path.display()),
        _ => format!("could not resolve path {}: {error}", path.display()),
    })?;
    if resolved.is_file() {
        Ok(GuiLaunchTarget::QuickInspector(resolved))
    } else if resolved.is_dir() {
        Ok(GuiLaunchTarget::Directory(resolved))
    } else {
        Err(format!(
            "path is not a regular file or directory: {}",
            resolved.display()
        ))
    }
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
            #[cfg(any(unix, target_os = "windows"))]
            if let Some(client) = connected_index_daemon() {
                return print_index_diagnostics(&client.diagnostics()?, json);
            }
            let snapshot =
                ottrin_search::SearchService::load_persisted_default(configured_search_config())?;
            print_index_diagnostics(&snapshot.diagnostics(), json)
        }
        HeadlessAction::Search {
            query,
            json,
            content,
            limit,
            offset,
        } => {
            let config = configured_search_config();
            if content
                && (!config.content_indexing_enabled || config.content_include_roots.is_empty())
            {
                return Err("Content search is disabled or has no selected folders. Configure it in Settings → Search.".into());
            }
            let root_path = if config.default_scope == SearchScope::CurrentFolder {
                Some(
                    std::env::current_dir()
                        .map_err(|error| format!("Cannot search the current folder: {error}"))?,
                )
            } else {
                None
            };
            let query = SearchQuery {
                text: query,
                scope: config.default_scope,
                root_path,
                include_hidden_system: config.include_hidden_system,
                sort: config.default_sort,
                limit: limit.unwrap_or(config.result_limit).max(1),
                offset,
                content_search: content,
            };
            #[cfg(any(unix, target_os = "windows"))]
            if let Some(client) = connected_index_daemon() {
                return print_search_response(client.query(query)?.response, json);
            }
            let snapshot = ottrin_search::SearchService::load_persisted_default(config)?;
            let response = snapshot.query(query);
            print_search_response(response, json)
        }
        HeadlessAction::IndexUpdate { json } => run_index_writer(json, false),
        HeadlessAction::IndexRebuild { json } => run_index_writer(json, true),
        HeadlessAction::IndexServiceInstall => {
            let config = configured_search_config();
            println!(
                "{}",
                ottrin_search::SearchService::install_sqlite_schedule(&config)?
            );
            Ok(())
        }
        HeadlessAction::IndexServiceStatus => {
            let status = ottrin_search::SearchService::sqlite_schedule_status();
            println!("source={}", status.source);
            println!("enabled={}", status.enabled);
            println!("detail={}", status.detail);
            Ok(())
        }
        HeadlessAction::IndexServiceRestart => {
            println!(
                "{}",
                ottrin_search::SearchService::restart_sqlite_schedule()?
            );
            Ok(())
        }
        HeadlessAction::IndexServiceRemove => {
            println!(
                "{}",
                ottrin_search::SearchService::uninstall_sqlite_schedule()?
            );
            Ok(())
        }
        HeadlessAction::Preview(_)
        | HeadlessAction::OpenPath(_)
        | HeadlessAction::SelectPath(_) => {
            unreachable!("GUI paths start before headless routing")
        }
        HeadlessAction::DropStatus | HeadlessAction::DropCopy(_) => {
            unreachable!("drop actions return before search routing")
        }
        HeadlessAction::Invalid(_) => unreachable!("invalid actions return before search routing"),
    }
}

fn run_index_writer(json: bool, rebuild: bool) -> Result<(), String> {
    #[cfg(any(unix, target_os = "windows"))]
    {
        let client = ensure_index_daemon()?;
        client.update_config(configured_search_config())?;
        if rebuild {
            client.rebuild()?;
        } else {
            client.reconcile()?;
        }
        print_index_diagnostics(&client.diagnostics()?, json)
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let diagnostics = ottrin_search::SearchService::new(configured_search_config())
            .run_single_update(rebuild)?;
        print_index_diagnostics(&diagnostics, json)
    }
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
        SearchIndexStatus::Loading => "loading",
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
            "database_bytes": diagnostics.database_bytes,
            "wal_bytes": diagnostics.wal_bytes,
            "freelist_pages": diagnostics.freelist_pages,
            "freelist_bytes": diagnostics.freelist_bytes,
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
    println!("Database: {} bytes", diagnostics.database_bytes);
    println!("WAL: {} bytes", diagnostics.wal_bytes);
    println!(
        "Free pages: {} ({} bytes)",
        diagnostics.freelist_pages, diagnostics.freelist_bytes
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

    #[cfg(unix)]
    #[test]
    fn initial_viewport_fits_taskbar_work_area_at_normal_and_scaled_dpi() {
        for scale in [1.0, 1.5, 2.0] {
            for pixels in [egui::vec2(1280.0, 752.0), egui::vec2(1280.0, 672.0)] {
                let work = egui::Rect::from_min_size(
                    egui::pos2(48.0 / scale, 24.0 / scale),
                    pixels / scale,
                );
                let viewport = fit_initial_viewport(
                    egui::ViewportBuilder::default()
                        .with_inner_size([1024.0, 680.0])
                        .with_min_inner_size([800.0, 500.0]),
                    work,
                );
                let rect = egui::Rect::from_min_size(
                    viewport.position.unwrap(),
                    viewport.inner_size.unwrap(),
                );
                assert!(work.contains_rect(rect), "{scale}: {work:?} / {rect:?}");
                assert!((rect.center() - work.center()).length() < 0.001);
                assert!(viewport.min_inner_size.unwrap().x <= work.width());
                assert!(viewport.min_inner_size.unwrap().y <= work.height());
            }
        }
        let work = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 752.0));
        for requested in [[100.0, 100.0], [2048.0, 1400.0]] {
            let viewport = fit_initial_viewport(
                egui::ViewportBuilder::default()
                    .with_inner_size(requested)
                    .with_min_inner_size([800.0, 500.0]),
                work,
            );
            let size = viewport.inner_size.unwrap();
            assert!(size.x >= 800.0 && size.y >= 500.0);
            assert!(
                work.contains_rect(egui::Rect::from_min_size(viewport.position.unwrap(), size))
            );
        }
        let positioned = egui::ViewportBuilder::default().with_position([12.0, 25.0]);
        let unchanged = fit_initial_viewport(
            positioned,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1040.0)),
        );
        assert_eq!(unchanged.position, Some(egui::pos2(12.0, 25.0)));
    }

    #[cfg(unix)]
    #[test]
    fn administrator_gui_launch_does_not_start_index() {
        assert!(!should_ensure_index_daemon(0));
        assert!(should_ensure_index_daemon(1000));
    }

    #[cfg(unix)]
    #[test]
    fn administrator_window_title_identifies_the_privileged_session() {
        assert_eq!(
            application_window_title_for_unix_user(0),
            "Ottrin (Administrator)"
        );
        assert_eq!(application_window_title_for_unix_user(1000), "Ottrin");
    }

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
    fn standalone_inspector_keeps_a_mapped_root_for_secondary_viewport_painting() {
        let viewport = main_viewport_geometry(true, [1024.0, 680.0]);

        assert!(viewport.visible);
        assert_eq!(viewport.inner_size, [1.0, 1.0]);
        assert_eq!(viewport.min_inner_size, [1.0, 1.0]);
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
    fn search_page_options_are_validated_before_query_execution() {
        let args = [
            "--content",
            "--offset",
            "17",
            "needle",
            "--limit",
            "7",
            "--json",
        ]
        .map(String::from);
        assert_eq!(
            parse_search_action(&args),
            HeadlessAction::Search {
                query: "needle".into(),
                content: true,
                json: true,
                limit: Some(7),
                offset: 17
            }
        );
        for args in [
            vec!["needle", "--limit", "0"],
            vec!["needle", "--offset", "-1"],
            vec!["needle", "--limit"],
            vec!["needle", "--offset", "3", "--offset", "4"],
            vec!["needle", "--limit", "999999999999999999999999"],
            vec!["needle", "--unknown"],
        ] {
            assert!(matches!(
                parse_search_action(&args.into_iter().map(String::from).collect::<Vec<_>>()),
                HeadlessAction::Invalid(_)
            ));
        }
    }

    #[test]
    fn canonical_search_and_index_commands_are_recognized_before_the_gui_starts() {
        for command in ["search", "-s"] {
            assert_eq!(
                cli_headless_action(&[command.into(), "invoice".into()]),
                Some(HeadlessAction::Search {
                    query: "invoice".into(),
                    json: false,
                    content: false,
                    limit: None,
                    offset: 0,
                })
            );
        }
        assert_eq!(
            cli_headless_action(&["search".into(), "invoice".into(), "--json".into()]),
            Some(HeadlessAction::Search {
                query: "invoice".into(),
                json: true,
                content: false,
                limit: None,
                offset: 0,
            })
        );
        assert_eq!(
            cli_headless_action(&[
                "search".into(),
                "--content".into(),
                "invoice".into(),
                "--json".into()
            ]),
            Some(HeadlessAction::Search {
                query: "invoice".into(),
                json: true,
                content: true,
                limit: None,
                offset: 0,
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
        assert_eq!(
            cli_headless_action(&["index".into(), "service-install".into()]),
            Some(HeadlessAction::IndexServiceInstall)
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "service-status".into()]),
            Some(HeadlessAction::IndexServiceStatus)
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "service-restart".into()]),
            Some(HeadlessAction::IndexServiceRestart)
        );
        assert_eq!(
            cli_headless_action(&["index".into(), "service-remove".into()]),
            Some(HeadlessAction::IndexServiceRemove)
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
    fn direct_path_resolution_distinguishes_files_and_directories() {
        let root =
            std::env::temp_dir().join(format!("ottrin-cli-direct-target-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create direct target root");
        let file = root.join("note.md");
        std::fs::write(&file, "# Note\n").expect("write direct target file");

        assert_eq!(
            resolve_direct_open_path(&file).expect("resolve direct file"),
            GuiLaunchTarget::QuickInspector(std::fs::canonicalize(&file).expect("canonical file"))
        );
        assert_eq!(
            resolve_direct_open_path(&root).expect("resolve direct directory"),
            GuiLaunchTarget::Directory(std::fs::canonicalize(&root).expect("canonical directory"))
        );

        let _ = std::fs::remove_dir_all(root);
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
    fn select_option_reveals_exactly_one_file_in_the_main_window() {
        assert_eq!(
            cli_headless_action(&["--select".into(), "/tmp/report.pdf".into()]),
            Some(HeadlessAction::SelectPath(PathBuf::from("/tmp/report.pdf")))
        );
        assert_eq!(
            cli_headless_action(&["--select".into()]),
            Some(HeadlessAction::Invalid(
                "--select requires a file path".to_string()
            ))
        );
    }

    #[test]
    fn direct_directory_argument_is_a_gui_open_path() {
        let root = std::env::temp_dir().join(format!(
            "ottrin-cli-direct-directory-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create direct directory");
        let value = root.to_string_lossy().into_owned();

        assert_eq!(
            cli_headless_action(std::slice::from_ref(&value)),
            Some(HeadlessAction::OpenPath(root.clone()))
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_file_argument_opens_quick_inspector_without_an_index_command() {
        let root =
            std::env::temp_dir().join(format!("ottrin-cli-direct-preview-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create direct-preview root");
        let file = root.join("fstab");
        std::fs::write(&file, "rootfs / ext4 defaults 0 1\n")
            .expect("write direct-preview fixture");

        assert_eq!(
            cli_headless_action(&[file.display().to_string()]),
            Some(HeadlessAction::OpenPath(file.clone()))
        );
        assert_eq!(
            cli_headless_action(&[root.join("missing.conf").display().to_string()]),
            Some(HeadlessAction::OpenPath(root.join("missing.conf")))
        );

        let _ = std::fs::remove_dir_all(root);
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
        let fixture = tempfile::tempdir().expect("owned startup diagnostic fixture");
        let directory = fixture.path().to_path_buf();
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
