#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod file_manager;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod portal_options;

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
use std::ffi::{OsStr, OsString};
#[cfg(any(target_os = "windows", test))]
use std::fs::OpenOptions;
#[cfg(any(target_os = "windows", test))]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
#[cfg(any(unix, target_os = "windows"))]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(any(unix, test))]
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
enum GuiLaunchTarget {
    QuickInspector(PathBuf),
    Directory(PathBuf),
    Select(PathBuf),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct CliArguments {
    #[cfg(target_os = "linux")]
    startup: ottrin_ui::StartupNotification,
    command: Vec<OsString>,
    renderer_override: Option<String>,
    helper_path: Option<PathBuf>,
    help: bool,
}

/// The prefix is ASCII, so stripping it ends at a valid native encoding boundary.
fn native_option_value<'a>(argument: &'a OsStr, prefix: &str) -> Option<&'a OsStr> {
    let suffix = argument
        .as_encoded_bytes()
        .strip_prefix(prefix.as_bytes())?;
    // SAFETY: suffix is from this process's native string encoding, split just
    // after an ASCII prefix, never inside an encoded native character.
    Some(unsafe { OsStr::from_encoded_bytes_unchecked(suffix) })
}

/// Separate global GUI options from the command before interpreting a target.
/// Values after `--` belong to the command, including option-like filenames.
fn parse_cli_arguments(args: &[OsString]) -> Result<CliArguments, String> {
    let mut parsed = CliArguments::default();
    let mut index = 0;
    while let Some(argument) = args.get(index) {
        if argument == "--" {
            parsed.command.extend_from_slice(&args[index..]);
            break;
        }
        if matches!(argument.to_str(), Some("--help" | "-h")) {
            parsed.help = true;
            index += 1;
            continue;
        }
        let option = if argument == "--renderer" {
            Some(("--renderer", None))
        } else if argument == "--helper-path" {
            Some(("--helper-path", None))
        } else if let Some(value) = native_option_value(argument, "--renderer=") {
            Some(("--renderer", Some(value)))
        } else {
            native_option_value(argument, "--helper-path=")
                .map(|value| ("--helper-path", Some(value)))
        };
        if let Some((name, inline)) = option {
            let value = match inline {
                Some(value) if !value.is_empty() => value.to_owned(),
                None => {
                    index += 1;
                    args.get(index)
                        .filter(|value| {
                            !value.is_empty() && !value.as_encoded_bytes().starts_with(b"-")
                        })
                        .cloned()
                        .ok_or_else(|| format!("{name} requires a value"))?
                }
                _ => return Err(format!("{name} requires a value")),
            };
            if name == "--renderer" {
                if parsed.renderer_override.is_some() {
                    return Err("--renderer cannot be repeated".into());
                }
                parsed.renderer_override = Some(
                    validate_renderer_override(Some(
                        value
                            .to_str()
                            .ok_or("--renderer requires a UTF-8 renderer name")?,
                    ))?
                    .ok_or_else(|| "--renderer requires a value".to_string())?,
                );
            } else {
                if parsed.helper_path.is_some() {
                    return Err("--helper-path cannot be repeated".into());
                }
                parsed.helper_path = Some(PathBuf::from(value));
            }
        } else {
            parsed.command.push(argument.clone());
        }
        index += 1;
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadlessAction {
    #[cfg(target_os = "linux")]
    Portal,
    #[cfg(target_os = "linux")]
    FileManager,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    IntegrationManagement {
        integration: DesktopIntegration,
        action: IntegrationAction,
        json: bool,
    },
    IndexEnsure,
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
    Properties(Vec<PathBuf>),
    Picker {
        cancel_on_disconnect: bool,
    },
    OpenPath(PathBuf),
    SelectPath(PathBuf),
    DropStatus,
    DropCopy(Vec<PathBuf>),
    Invalid(String),
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegrationAction {
    Status,
    Enable,
    Disable,
    #[cfg(target_os = "windows")]
    DefaultEnable,
    #[cfg(target_os = "windows")]
    DefaultDisable,
    #[cfg(target_os = "linux")]
    Apply,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopIntegration {
    #[cfg(target_os = "linux")]
    Portal,
    Folders,
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
    #[cfg(target_os = "linux")]
    let startup = {
        let value = ottrin_ui::StartupNotification {
            x11: std::env::var("DESKTOP_STARTUP_ID")
                .ok()
                .filter(|v| !v.is_empty()),
            wayland: std::env::var("XDG_ACTIVATION_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
        };
        // SAFETY: this is the first work in main, before spawning any application
        // threads or calling GUI/D-Bus libraries. Descendants must not inherit used tokens.
        unsafe {
            std::env::remove_var("DESKTOP_STARTUP_ID");
            std::env::remove_var("XDG_ACTIVATION_TOKEN");
        }
        value
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.as_slice() == ["--index-daemon"] {
        use std::os::unix::process::CommandExt;
        let error = index_daemon_executable()
            .map(|executable| Command::new(executable).args(&args).exec())
            .unwrap_or_else(|error| error);
        eprintln!("ottrin index daemon: {error}");
        std::process::exit(1);
    }
    let cli = match parse_cli_arguments(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("ottrin: {error}");
            std::process::exit(2);
        }
    };
    #[cfg(target_os = "linux")]
    let cli = CliArguments { startup, ..cli };
    if cli.help {
        print_cli_help();
        return;
    }
    let action = cli_headless_action(&cli.command);
    if let Some(HeadlessAction::Properties(paths)) = &action {
        install_windows_startup_panic_hook();
        if let Err(error) = run_properties_gui(paths.clone(), cli) {
            eprintln!("ottrin properties: {error}");
            report_windows_startup_failure("Ottrin properties could not start", &error);
            std::process::exit(1);
        }
        return;
    }
    if let Some(HeadlessAction::Picker {
        cancel_on_disconnect,
    }) = &action
    {
        let cancel_on_disconnect = *cancel_on_disconnect;
        let request = match if cancel_on_disconnect {
            read_picker_request_line(std::io::stdin().lock())
        } else {
            read_picker_request(std::io::stdin().lock())
        } {
            Ok(request) => request,
            Err(error) => {
                eprintln!("ottrin picker: {error}");
                std::process::exit(2);
            }
        };
        install_windows_startup_panic_hook();
        if let Err(error) = run_picker_gui(request, cli, cancel_on_disconnect) {
            eprintln!("ottrin picker: {error}");
            std::process::exit(1);
        }
        return;
    }
    let gui_target = match action {
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
        Some(HeadlessAction::SelectPath(path)) => match resolve_select_path(&path) {
            Ok(path) => Some(GuiLaunchTarget::Select(path)),
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(2);
            }
        },
        _ => None,
    };
    if gui_target.is_none() && handle_cli_action(&cli.command) {
        return;
    }
    install_windows_startup_panic_hook();
    if let Err(error) = run_gui(gui_target, cli) {
        eprintln!("ottrin: {error}");
        report_windows_startup_failure("Ottrin could not start", &error.to_string());
        std::process::exit(1);
    }
}

fn run_gui(gui_target: Option<GuiLaunchTarget>, cli: CliArguments) -> eframe::Result<()> {
    if let Some(path) = cli.helper_path {
        set_privileged_helper_override(Some(path));
    } else {
        ensure_dev_helper();
    }

    let renderer_override = match graphics_renderer_override(cli.renderer_override.as_deref()) {
        Ok(renderer_override) => renderer_override,
        Err(error) => {
            eprintln!("ottrin: {error}");
            report_windows_startup_failure("Ottrin could not select a renderer", &error);
            std::process::exit(2);
        }
    };

    let config = load_config().map_err(|error| eframe::Error::AppCreation(error.into()))?;
    let graphics_runtime = graphics_runtime_status(renderer_override.is_some(), &config);
    let window_size = config.window_size.unwrap_or([1024.0, 680.0]);

    #[cfg(target_os = "linux")]
    let startup = cli.startup;
    let standalone_inspector = matches!(gui_target, Some(GuiLaunchTarget::QuickInspector(_)));
    let main_viewport = main_viewport_geometry(standalone_inspector, window_size);
    let window_title = application_window_title();
    let options = application_window_options(
        egui::ViewportBuilder::default()
            .with_title(window_title.clone())
            .with_visible(main_viewport.visible)
            .with_inner_size(main_viewport.inner_size)
            .with_min_inner_size(main_viewport.min_inner_size),
        &graphics_runtime,
    );
    #[cfg(target_os = "windows")]
    let options = windows_initial_window_options(options, standalone_inspector);
    #[cfg(target_os = "linux")]
    let options = startup_window_options(options, &startup);
    eframe::run_native(
        &window_title,
        options,
        Box::new(move |cc| {
            let mut app = OttrinApp::new_with_graphics_status(cc, graphics_runtime)?;
            #[cfg(any(unix, target_os = "windows"))]
            {
                #[cfg(unix)]
                // SAFETY: geteuid has no preconditions and no side effects.
                let required = should_ensure_index_daemon(unsafe { libc::geteuid() });
                #[cfg(target_os = "windows")]
                let required = true;
                if required && !standalone_inspector {
                    let ctx = cc.egui_ctx.clone();
                    app.observe_index_startup(spawn_index_startup(
                        run_index_bootstrap_process,
                        move || ctx.request_repaint(),
                    ));
                }
            }
            match gui_target {
                Some(GuiLaunchTarget::QuickInspector(path)) => {
                    app.open_quick_inspector_file(path);
                }
                Some(GuiLaunchTarget::Directory(path)) => {
                    app.open_directory_at_startup(path);
                }
                Some(GuiLaunchTarget::Select(path)) => {
                    app.reveal_path_at_startup(path);
                }
                None => {}
            }
            #[cfg(target_os = "linux")]
            startup.apply(cc);
            Ok(Box::new(app))
        }),
    )
}

/// A headless process owns runtime upgrades so closing the window cannot interrupt them.
#[cfg(any(unix, target_os = "windows"))]
fn run_index_bootstrap_process() -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(executable);
    command
        .args(["index", "ensure"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let output = command
        .output()
        .map_err(|error| format!("Cannot launch index startup: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            format!("Index startup failed ({})", output.status)
        } else {
            detail
        })
    }
}

/// Launch availability is independent of index readiness; errors are reported in the open UI.
#[cfg(any(unix, target_os = "windows"))]
fn spawn_index_startup(
    start: impl FnOnce() -> Result<(), String> + Send + 'static,
    wake: impl FnOnce() + Send + 'static,
) -> std::sync::mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_tx = tx.clone();
    let worker = std::thread::Builder::new()
        .name("ottrin-index-startup".into())
        .spawn(move || {
            let result = start();
            if let Err(error) = &result {
                eprintln!("ottrin: Background indexing unavailable: {error}");
            }
            let _ = worker_tx.send(result);
            wake();
        });
    if let Err(error) = worker {
        let _ = tx.send(Err(format!("Cannot start index startup worker: {error}")));
    }
    rx
}

const MAX_PICKER_REQUEST_BYTES: usize = 128 * 1024;

fn read_picker_request(
    reader: impl std::io::Read,
) -> Result<ottrin_core::picker::PickerRequest, String> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    reader
        .take(MAX_PICKER_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    decode_picker_request(&bytes)
}

fn decode_picker_request(bytes: &[u8]) -> Result<ottrin_core::picker::PickerRequest, String> {
    if bytes.len() > MAX_PICKER_REQUEST_BYTES {
        return Err("Picker request exceeds 128 KiB".into());
    }
    let request = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid picker request: {error}"))?;
    ottrin_ui::PickerApp::validate_request(&request)?;
    Ok(request)
}

/// The disconnect-aware form uses one newline-terminated JSON frame; stdin stays
/// open as a lifetime lease. It does not require a polling thread or a service.
fn read_picker_request_line(
    reader: impl std::io::BufRead,
) -> Result<ottrin_core::picker::PickerRequest, String> {
    use std::io::BufRead as _;
    let mut bytes = Vec::new();
    reader
        .take(MAX_PICKER_REQUEST_BYTES as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.last() != Some(&b'\n') {
        return Err(
            "Disconnect-aware picker requires one newline-terminated JSON request, at most128KiB"
                .into(),
        );
    }
    bytes.pop();
    decode_picker_request(&bytes)
}

fn watch_picker_disconnect(
    mut reader: impl std::io::Read + Send + 'static,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    std::thread::Builder::new()
        .name("picker-caller".into())
        .spawn(move || {
            let mut byte = [0];
            loop {
                match reader.read(&mut byte) {
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => eprintln!("ottrin picker: caller connection failed: {error}"),
                    Ok(0) => {}
                    Ok(_) => {
                        eprintln!("ottrin picker: unexpected data after the request; cancelling")
                    }
                }
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                wake();
                break;
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn picker_window_system_options(
    mut options: NativeOptions,
    parent: &str,
) -> Result<NativeOptions, String> {
    use ottrin_core::picker::PickerParentWindow;
    let parent = PickerParentWindow::parse(parent)?;
    if parent == PickerParentWindow::None {
        return Ok(options);
    }
    if matches!(parent, PickerParentWindow::Windows(_)) {
        return Err("Windows picker parents are unavailable on Linux".into());
    }
    options.event_loop_builder = Some(Box::new(move |builder| match parent {
        PickerParentWindow::X11(_) => {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }
        PickerParentWindow::Wayland(_) => {
            use winit::platform::wayland::EventLoopBuilderExtWayland;
            builder.with_wayland();
        }
        _ => {}
    }));
    Ok(options)
}

fn application_window_options(
    viewport: egui::ViewportBuilder,
    runtime: &GraphicsRuntimeStatus,
) -> NativeOptions {
    NativeOptions {
        renderer: renderer_for_selection(runtime.is_windows, runtime.selection.choice),
        viewport: viewport
            .with_app_id(ottrin_ui::DESKTOP_APPLICATION_ID)
            .with_decorations(false)
            .with_transparent(main_viewport_transparent(runtime.is_windows))
            .with_icon(load_window_icon()),
        ..Default::default()
    }
}
#[cfg(target_os = "linux")]
fn startup_window_options(
    mut options: NativeOptions,
    startup: &ottrin_ui::StartupNotification,
) -> NativeOptions {
    if startup.pending() {
        options.viewport = options.viewport.with_visible(false);
    }
    options
}

fn run_properties_gui(paths: Vec<PathBuf>, cli: CliArguments) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let startup = cli.startup;
    let current = std::env::current_dir().map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let paths: Vec<_> = paths
        .into_iter()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                current.join(path)
            }
        })
        .filter(|path| seen.insert(path.clone()))
        .collect();
    if paths.is_empty() {
        return Err("--properties needs at least one entry path".into());
    }
    let renderer = graphics_renderer_override(cli.renderer_override.as_deref())?;
    let config = load_config()?;
    let runtime = graphics_runtime_status(renderer.is_some(), &config);
    let title = format!(
        "{} — {}",
        application_window_title(),
        ottrin_ui::PropertiesApp::title(&paths)
    );
    let options = application_window_options(
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([720.0, 540.0])
            .with_min_inner_size([480.0, 360.0]),
        &runtime,
    );
    #[cfg(target_os = "windows")]
    let options = windows_initial_window_options(options, false);
    #[cfg(target_os = "linux")]
    let options = startup_window_options(options, &startup);
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            let app = ottrin_ui::PropertiesApp::new(cc, paths, runtime)?;
            #[cfg(target_os = "linux")]
            startup.apply(cc);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

fn run_picker_gui(
    request: ottrin_core::picker::PickerRequest,
    cli: CliArguments,
    cancel_on_disconnect: bool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let startup = cli.startup;
    let renderer_override = graphics_renderer_override(cli.renderer_override.as_deref())?;
    let config = load_config()?;
    let runtime = graphics_runtime_status(renderer_override.is_some(), &config);
    let title = format!(
        "{} — {}",
        application_window_title(),
        request.window_title()
    );
    let options = application_window_options(
        egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([940.0, 640.0])
            .with_min_inner_size([680.0, 480.0]),
        &runtime,
    );
    #[cfg(target_os = "linux")]
    let options = picker_window_system_options(options, &request.parent_window)?;
    #[cfg(target_os = "windows")]
    let options = windows_initial_window_options(options, false);
    #[cfg(target_os = "linux")]
    let options = startup_window_options(options, &startup);
    let outcome = std::sync::Arc::new(std::sync::Mutex::new(None));
    let result = outcome.clone();
    let cancelled = request.cancelled_response();
    let validate = request.clone();
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            if cancel_on_disconnect {
                let ctx = cc.egui_ctx.clone();
                watch_picker_disconnect(std::io::stdin(), cancelled.clone(), move || {
                    ctx.request_repaint()
                })?;
            }
            let app = ottrin_ui::PickerApp::new(cc, request, runtime, outcome, cancelled)?;
            #[cfg(target_os = "linux")]
            startup.apply(cc);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| error.to_string())?;
    let response = result
        .lock()
        .map_err(|_| "Picker result lock was poisoned")?
        .take()
        .unwrap_or(cancelled);
    response.validate_for(&validate)?;
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|error| error.to_string())?
    );
    Ok(())
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
        let log_detail = match write_startup_failure_log_in(
            &startup_failure_log_directory(),
            summary,
            details,
        ) {
            Ok(path) => format!("Diagnostic log:\n{}", path.display()),
            Err(error) => format!("Could not save the diagnostic log: {error}"),
        };
        show_windows_startup_failure_dialog(&format!("{summary}\n\n{details}\n\n{log_detail}"));
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

fn handle_cli_action(args: &[OsString]) -> bool {
    #[cfg(target_os = "linux")]
    if args
        .first()
        .is_some_and(|arg| arg == "--native-package-desktop")
    {
        let result = match args.get(1).and_then(|arg| arg.to_str()) {
            Some("check-online") if args.len() == 2 => {
                ottrin_platform::native_desktop::maintain(true, true)
            }
            Some("check-offline") if args.len() == 2 => {
                ottrin_platform::native_desktop::maintain(true, false)
            }
            Some("remove-online") if args.len() == 2 => {
                ottrin_platform::native_desktop::maintain(false, true)
            }
            Some("remove-offline") if args.len() == 2 => {
                ottrin_platform::native_desktop::maintain(false, false)
            }
            _ => Err("Invalid native desktop maintenance operation".into()),
        };
        match result {
            Ok(true) => println!("retain-portal-definition"),
            Ok(false) => {}
            Err(error) => {
                eprintln!("ottrin: {error}");
                std::process::exit(1);
            }
        }
        return true;
    }
    if let Some(action) = cli_headless_action(args) {
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

    // A refused data cleanup must retain usable folder/picker registrations and payload.
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if errors.is_empty() {
        match ottrin_platform::remove_desktop_integration() {
            Ok(messages) => {
                for message in messages {
                    println!("{message}");
                }
            }
            Err(error) => errors.push(error),
        }
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
    #[cfg(target_os = "linux")]
    {
        println!("  ottrin portal                   Run the desktop file-dialog backend");
        println!("  ottrin portal enable|disable|apply|status [--json]");
        println!("                                 Manage optional desktop file dialogs");
    }
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    {
        println!("  ottrin folders enable|disable|status [--json]");
        println!("                                 Manage optional folder integration");
    }
    println!("  ottrin picker                   Open an Ottrin chooser using a JSON stdin request");
    println!(
        "    --cancel-on-disconnect        Read one JSON line and cancel when the caller closes stdin"
    );
    println!("  ottrin                         Open the file manager");
    println!("  ottrin <FILE>                  Open the file directly in Quick Inspector");
    println!("  ottrin <DIRECTORY>             Open the file manager in that directory");
    #[cfg(target_os = "linux")]
    println!("  ottrin file-manager           Run the on-demand desktop file-manager interface");
    #[cfg(target_os = "windows")]
    println!("  ottrin folders default-enable|default-disable  Choose or restore folder defaults");
    println!("  ottrin --properties <PATH>... Show a read-only themed properties window");
    println!("  ottrin --select <PATH>         Reveal an entry in its parent folder");
    println!("  ottrin search <QUERY> [--json] Search indexed files (-s)");
    println!("                       [--content] Also search the configured content folders");
    println!("                       [--limit N] [--offset N] Select a result page");
    println!("  ottrin index [update] [--json]");
    println!("                                 Update the configured search index");
    println!(
        "  ottrin index ensure            Start this package's user index and wait for readiness"
    );
    println!("  ottrin index status [--json]   Show persisted index status without starting it");
    println!("  ottrin index rebuild [--json]");
    println!("                                 Build and atomically publish a fresh index");
    println!("  ottrin preview <FILE>          Open the full Quick Inspector (-p)");
    println!("  ottrin drop status             Show Drop Folder state");
    println!("  ottrin drop copy <PATH>…       Copy paths into the Drop Folder");
    println!();
    println!("Options (GUI options may appear before or after the file/directory target):");
    println!("  --renderer opengl  Recovery override for a compatible OpenGL driver");
    println!("  --helper-path PATH Developer override for privileged helper discovery");
    println!("  --uninstall        Remove Ottrin background jobs and generated data");
    println!("  --purge-config     With --uninstall, also remove saved preferences");
    println!("Use -- before a literal option-like target or search query.");
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

fn text_command_arguments(args: &[OsString]) -> Result<Vec<String>, String> {
    args.iter()
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "Search/index arguments must be UTF-8 text".into())
        })
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn parse_integration_management(
    args: &[OsString],
    integration: DesktopIntegration,
) -> HeadlessAction {
    let (action, json) =
        match args {
            [action] if action == "enable" => (IntegrationAction::Enable, false),
            [action] if action == "disable" => (IntegrationAction::Disable, false),
            #[cfg(target_os="windows")]
            [action] if action=="default-enable" => (IntegrationAction::DefaultEnable,false),
            #[cfg(target_os="windows")]
            [action] if action=="default-disable" => (IntegrationAction::DefaultDisable,false),
            #[cfg(target_os = "linux")]
            [action] if action == "apply" && integration == DesktopIntegration::Portal => {
                (IntegrationAction::Apply, false)
            }
            [action] if action == "status" => (IntegrationAction::Status, false),
            [action, flag] if action == "status" && flag == "--json" => {
                (IntegrationAction::Status, true)
            }
            _ => return HeadlessAction::Invalid(
                if cfg!(target_os="windows") {"Folders accepts enable, disable, default-enable, default-disable or status [--json]"}else{"Integration accepts enable, disable or status [--json]; portal also accepts apply"}.into(),
            ),
        };
    HeadlessAction::IntegrationManagement {
        integration,
        action,
        json,
    }
}

fn cli_headless_action(args: &[OsString]) -> Option<HeadlessAction> {
    let first = args.first()?;
    let command = first.to_str().unwrap_or("");
    match command {
        "search" | "-s" => Some(
            text_command_arguments(&args[1..])
                .map(|args| parse_search_action(&args))
                .unwrap_or_else(HeadlessAction::Invalid),
        ),
        "index" => Some(
            text_command_arguments(&args[1..])
                .map(|args| parse_index_action(&args))
                .unwrap_or_else(HeadlessAction::Invalid),
        ),
        #[cfg(target_os = "linux")]
        "portal" if args.len() == 1 => Some(HeadlessAction::Portal),
        #[cfg(target_os = "linux")]
        "portal" => Some(parse_integration_management(
            &args[1..],
            DesktopIntegration::Portal,
        )),
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        "folders" => Some(parse_integration_management(
            &args[1..],
            DesktopIntegration::Folders,
        )),
        "picker" => Some(
            if args.len() == 1 || (args.len() == 2 && args[1] == "--cancel-on-disconnect") {
                HeadlessAction::Picker {
                    cancel_on_disconnect: args.len() == 2,
                }
            } else {
                HeadlessAction::Invalid("picker reads one JSON request from standard input".into())
            },
        ),
        "preview" | "-p" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::Preview(PathBuf::from(path)),
            [separator, path] if separator == "--" && !path.is_empty() => {
                HeadlessAction::Preview(PathBuf::from(path))
            }
            [] => HeadlessAction::Invalid("preview requires a file path".to_string()),
            _ => HeadlessAction::Invalid("preview accepts exactly one file path".to_string()),
        }),
        #[cfg(target_os = "linux")]
        "file-manager" => Some(if args.len() == 1 {
            HeadlessAction::FileManager
        } else {
            HeadlessAction::Invalid("file-manager accepts no arguments".into())
        }),
        "--properties" => {
            let paths = if args.get(1).is_some_and(|value| value == "--") {
                &args[2..]
            } else {
                &args[1..]
            };
            Some(
                if paths.is_empty() || paths.iter().any(|path| path.is_empty()) {
                    HeadlessAction::Invalid("--properties needs at least one entry path".into())
                } else {
                    HeadlessAction::Properties(paths.iter().map(PathBuf::from).collect())
                },
            )
        }
        "--select" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::SelectPath(PathBuf::from(path)),
            [separator, path] if separator == "--" && !path.is_empty() => {
                HeadlessAction::SelectPath(PathBuf::from(path))
            }
            [] => HeadlessAction::Invalid("--select requires an entry path".to_string()),
            _ => HeadlessAction::Invalid("--select accepts exactly one entry path".to_string()),
        }),
        "drop" => Some(match args.get(1).and_then(|value| value.to_str()) {
            Some("status") if args.len() == 2 => HeadlessAction::DropStatus,
            Some("copy") => HeadlessAction::DropCopy(args[2..].iter().map(PathBuf::from).collect()),
            Some(value) => HeadlessAction::Invalid(format!("unknown drop action: {value}")),
            None => HeadlessAction::Invalid("drop requires status or copy".to_string()),
        }),
        "--" => Some(match &args[1..] {
            [path] if !path.is_empty() => HeadlessAction::OpenPath(PathBuf::from(path)),
            _ => HeadlessAction::Invalid("-- requires exactly one file or directory path".into()),
        }),
        "--uninstall" | "--purge-config" => None,
        _ => Some(
            direct_open_path(args)
                .map(HeadlessAction::OpenPath)
                .unwrap_or_else(|| {
                    HeadlessAction::Invalid(format!("unknown command: {}", first.to_string_lossy()))
                }),
        ),
    }
}

fn direct_open_path(args: &[OsString]) -> Option<PathBuf> {
    let [value] = args else {
        return None;
    };
    if value.as_encoded_bytes().starts_with(b"-") {
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
            "ensure" | "update" | "status" | "rebuild" | "service-install" | "service-status"
            | "service-restart" | "service-remove"
                if !action_seen =>
            {
                action = args[index].as_str();
                action_seen = true;
            }
            "ensure" | "update" | "status" | "rebuild" | "service-install" | "service-status"
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
        "ensure" if !json => HeadlessAction::IndexEnsure,
        "ensure" => HeadlessAction::Invalid("index ensure does not accept --json".into()),
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

fn configured_search_config() -> Result<ottrin_core::SearchConfig, String> {
    Ok(load_config()?.search)
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
    #[cfg(target_os = "linux")]
    if let Ok(client) = ottrin_search::SearchDaemonClient::connect_default()
        && client.server_pid().is_ok()
        && client
            .ping()
            .is_err_and(|error| error.starts_with("index daemon protocol mismatch:"))
    {
        if unit_path.is_file() {
            ottrin_search::SearchService::install_sqlite_schedule(&configured_search_config()?)?;
        } else {
            client.retire_incompatible_daemon()?;
        }
    }
    if let Some(client) = connected_index_daemon() {
        let executable = index_daemon_executable()
            .map_err(|error| format!("could not locate the Ottrin index daemon: {error}"))?;
        if daemon_uses_executable(&client, &executable)? {
            return Ok(client);
        }
        if unit_path.is_file() {
            // Use the existing installer to prepare, stop and update the service
            // before starting it. A plain start can race an exiting active unit.
            ottrin_search::SearchService::install_sqlite_schedule(&configured_search_config()?)?;
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

fn resolve_select_path(path: &std::path::Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    // Resolve ancestors while keeping a symbolic link itself as the selected item.
    let resolved = match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map_err(|e| format!("Cannot open the parent folder: {e}"))?
            .join(name),
        _ => std::fs::canonicalize(&absolute).map_err(|e| e.to_string())?,
    };
    std::fs::symlink_metadata(&resolved)
        .map_err(|e| format!("Cannot reveal {}: {e}", resolved.display()))?;
    Ok(resolved)
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
        #[cfg(target_os = "linux")]
        HeadlessAction::FileManager => return file_manager::run(),
        #[cfg(target_os = "linux")]
        HeadlessAction::Portal => return portal::run(),
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        HeadlessAction::IntegrationManagement {
            integration,
            action,
            json,
        } => {
            let message = match (integration, action) {
                #[cfg(target_os = "linux")]
                (DesktopIntegration::Portal, IntegrationAction::Enable) => {
                    ottrin_platform::portal_integration::enable()?
                }
                #[cfg(target_os = "linux")]
                (DesktopIntegration::Portal, IntegrationAction::Disable) => {
                    ottrin_platform::portal_integration::disable()?
                }
                #[cfg(target_os = "linux")]
                (DesktopIntegration::Portal, IntegrationAction::Apply) => {
                    ottrin_platform::portal_integration::apply()?
                }
                #[cfg(target_os = "linux")]
                (DesktopIntegration::Portal, IntegrationAction::Status) => {
                    let status = ottrin_platform::portal_integration::status();
                    if json {
                        serde_json::to_string(&status).map_err(|e| e.to_string())?
                    } else {
                        status.detail
                    }
                }
                (DesktopIntegration::Folders, IntegrationAction::Enable) => {
                    ottrin_platform::folder_integration::enable()?
                }
                (DesktopIntegration::Folders, IntegrationAction::Disable) => {
                    ottrin_platform::folder_integration::disable()?
                }
                #[cfg(target_os = "windows")]
                (DesktopIntegration::Folders, IntegrationAction::DefaultEnable) => {
                    ottrin_platform::folder_integration::enable_default()?
                }
                #[cfg(target_os = "windows")]
                (DesktopIntegration::Folders, IntegrationAction::DefaultDisable) => {
                    ottrin_platform::folder_integration::disable_default()?
                }
                (DesktopIntegration::Folders, IntegrationAction::Status) => {
                    let status = ottrin_platform::folder_integration::status();
                    if json {
                        serde_json::to_string(&status).map_err(|e| e.to_string())?
                    } else {
                        status.detail
                    }
                }
                #[cfg(target_os = "linux")]
                (DesktopIntegration::Folders, IntegrationAction::Apply) => {
                    return Err("Folder changes apply immediately".into());
                }
            };
            println!("{message}");
            return Ok(());
        }
        HeadlessAction::DropStatus => {
            print_drop_status(&load_config()?.target);
            return Ok(());
        }
        HeadlessAction::DropCopy(sources) => return run_drop_copy(sources),
        _ => {}
    }

    match action {
        #[cfg(target_os = "linux")]
        HeadlessAction::Portal | HeadlessAction::FileManager => {
            unreachable!("desktop interfaces start before index routing")
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        HeadlessAction::IntegrationManagement { .. } => {
            unreachable!("integration starts before index routing")
        }
        HeadlessAction::IndexEnsure => {
            #[cfg(unix)]
            // SAFETY: geteuid has no preconditions and no side effects.
            if !should_ensure_index_daemon(unsafe { libc::geteuid() }) {
                return Err(
                    "Administrator indexing requires explicit service setup and scope selection"
                        .into(),
                );
            }
            #[cfg(any(unix, target_os = "windows"))]
            {
                ensure_index_daemon().map(|_| ())
            }
            #[cfg(not(any(unix, target_os = "windows")))]
            {
                Err("Background index startup is unavailable on this platform".into())
            }
        }
        HeadlessAction::IndexStatus { json } => {
            #[cfg(any(unix, target_os = "windows"))]
            if let Some(client) = connected_index_daemon() {
                return print_index_diagnostics(&client.diagnostics()?, json);
            }
            let snapshot =
                ottrin_search::SearchService::load_persisted_default(configured_search_config()?)?;
            print_index_diagnostics(&snapshot.diagnostics(), json)
        }
        HeadlessAction::Search {
            query,
            json,
            content,
            limit,
            offset,
        } => {
            let config = configured_search_config()?;
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
            let config = configured_search_config()?;
            println!(
                "{}",
                ottrin_search::SearchService::install_sqlite_schedule(&config)?
            );
            Ok(())
        }
        HeadlessAction::IndexServiceStatus => {
            let status = ottrin_search::SearchService::sqlite_schedule_status();
            println!("source={}", status.source);
            println!(
                "enabled={}",
                status
                    .enabled
                    .map_or("unknown", |enabled| if enabled { "true" } else { "false" })
            );
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
        | HeadlessAction::Properties(_)
        | HeadlessAction::Picker { .. }
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
        client.update_config(configured_search_config()?)?;
        if rebuild {
            client.rebuild()?;
        } else {
            client.reconcile()?;
        }
        print_index_diagnostics(&client.diagnostics()?, json)
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let diagnostics = ottrin_search::SearchService::new(configured_search_config()?)
            .run_single_update(rebuild)?;
        print_index_diagnostics(&diagnostics, json)
    }
}

fn print_search_response(response: ottrin_core::SearchResponse, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            ottrin_search::encode_search_response(&response)
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

    let destination = load_config()?
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

fn graphics_renderer_override(cli_override: Option<&str>) -> Result<Option<String>, String> {
    let environment_override = std::env::var("OTTRIN_RENDERER").ok();
    validate_renderer_override(cli_override.or(environment_override.as_deref()))
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

    #[cfg(any(unix, target_os = "windows"))]
    fn native_argument_fixture() -> OsString {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"native\x80.txt".to_vec())
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[110, 97, 116, 105, 118, 101, 0xd800, 46, 116, 120, 116])
        }
    }

    #[test]
    fn properties_accepts_native_batches_and_reveal_accepts_folders_and_roots() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            resolve_select_path(&folder).unwrap(),
            std::fs::canonicalize(&folder).unwrap()
        );
        let absolute = std::fs::canonicalize(&folder).unwrap();
        let volume = absolute.ancestors().last().unwrap();
        assert_eq!(resolve_select_path(volume).unwrap(), volume);
        let native = PathBuf::from(native_argument_fixture());
        assert_eq!(
            cli_headless_action(&[
                "--properties".into(),
                "--".into(),
                native.clone().into_os_string(),
                folder.clone().into_os_string()
            ]),
            Some(HeadlessAction::Properties(vec![native, folder]))
        );
        assert!(matches!(
            cli_headless_action(&["--properties".into()]),
            Some(HeadlessAction::Invalid(_))
        ));
    }
    #[cfg(unix)]
    #[test]
    fn reveal_selects_a_link_in_its_parent_instead_of_following_the_target() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join(native_argument_fixture());
        std::os::unix::fs::symlink("missing target", &link).unwrap();
        assert_eq!(
            resolve_select_path(&link).unwrap(),
            std::fs::canonicalize(root.path())
                .unwrap()
                .join(link.file_name().unwrap())
        );
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn native_cli_paths_survive_open_reveal_preview_drop_and_helper_options() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join(native_argument_fixture());
        std::fs::write(&file, b"preserve native launch source").unwrap();
        let path = file.clone().into_os_string();
        for (command, expected) in [
            (vec![path.clone()], HeadlessAction::OpenPath(file.clone())),
            (
                vec!["--".into(), path.clone()],
                HeadlessAction::OpenPath(file.clone()),
            ),
            (
                vec!["--select".into(), path.clone()],
                HeadlessAction::SelectPath(file.clone()),
            ),
            (
                vec!["preview".into(), path.clone()],
                HeadlessAction::Preview(file.clone()),
            ),
            (
                vec!["drop".into(), "copy".into(), path.clone()],
                HeadlessAction::DropCopy(vec![file.clone()]),
            ),
        ] {
            let parsed = parse_cli_arguments(&command).unwrap();
            assert_eq!(cli_headless_action(&parsed.command), Some(expected));
        }
        assert_eq!(
            resolve_direct_open_path(&file).unwrap(),
            GuiLaunchTarget::QuickInspector(std::fs::canonicalize(&file).unwrap())
        );
        let mut inline = OsString::from("--helper-path=");
        inline.push(&path);
        for command in [vec![inline], vec!["--helper-path".into(), path]] {
            assert_eq!(
                parse_cli_arguments(&command).unwrap().helper_path,
                Some(file.clone())
            );
        }
        assert_eq!(
            std::fs::read(file).unwrap(),
            b"preserve native launch source"
        );
    }

    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn text_only_cli_options_reject_native_non_text_without_panicking() {
        let native = native_argument_fixture();
        let mut renderer = OsString::from("--renderer=");
        renderer.push(&native);
        assert!(parse_cli_arguments(&[renderer]).is_err());
        for command in ["search", "index"] {
            assert!(matches!(
                cli_headless_action(&[command.into(), native.clone()]),
                Some(HeadlessAction::Invalid(_))
            ));
        }
        assert!(
            parse_cli_arguments(&[native, "--help".into()])
                .unwrap()
                .help
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn portal_entry_is_headless_and_rejects_unowned_arguments() {
        assert_eq!(
            cli_headless_action(&["portal".into()]),
            Some(HeadlessAction::Portal)
        );
        for (name, action) in [
            ("enable", IntegrationAction::Enable),
            ("disable", IntegrationAction::Disable),
            ("apply", IntegrationAction::Apply),
            ("status", IntegrationAction::Status),
        ] {
            assert_eq!(
                cli_headless_action(&["portal".into(), name.into()]),
                Some(HeadlessAction::IntegrationManagement {
                    integration: DesktopIntegration::Portal,
                    action,
                    json: false
                })
            );
        }
        assert_eq!(
            cli_headless_action(&["portal".into(), "status".into(), "--json".into()]),
            Some(HeadlessAction::IntegrationManagement {
                integration: DesktopIntegration::Portal,
                action: IntegrationAction::Status,
                json: true
            })
        );
        assert!(matches!(
            cli_headless_action(&["portal".into(), "enable".into(), "--json".into()]),
            Some(HeadlessAction::Invalid(_))
        ));
        for (name, action) in [
            ("enable", IntegrationAction::Enable),
            ("disable", IntegrationAction::Disable),
            ("status", IntegrationAction::Status),
        ] {
            assert_eq!(
                cli_headless_action(&["folders".into(), name.into()]),
                Some(HeadlessAction::IntegrationManagement {
                    integration: DesktopIntegration::Folders,
                    action,
                    json: false
                })
            );
        }
        assert_eq!(
            cli_headless_action(&["folders".into(), "status".into(), "--json".into()]),
            Some(HeadlessAction::IntegrationManagement {
                integration: DesktopIntegration::Folders,
                action: IntegrationAction::Status,
                json: true
            })
        );
        assert!(matches!(
            cli_headless_action(&["folders".into(), "apply".into()]),
            Some(HeadlessAction::Invalid(_))
        ));
        assert!(matches!(
            cli_headless_action(&["portal".into(), "extra".into()]),
            Some(HeadlessAction::Invalid(_))
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[test]
    fn folder_commands_are_headless_and_reject_unowned_modifiers() {
        for (name, action) in [
            ("enable", IntegrationAction::Enable),
            ("disable", IntegrationAction::Disable),
            ("status", IntegrationAction::Status),
        ] {
            assert_eq!(
                cli_headless_action(&["folders".into(), name.into()]),
                Some(HeadlessAction::IntegrationManagement {
                    integration: DesktopIntegration::Folders,
                    action,
                    json: false
                })
            );
        }
        for arguments in [
            vec!["folders", "apply"],
            vec!["folders", "enable", "--json"],
            vec!["folders", "disable", "extra"],
        ] {
            assert!(matches!(
                cli_headless_action(
                    &arguments
                        .into_iter()
                        .map(OsString::from)
                        .collect::<Vec<_>>()
                ),
                Some(HeadlessAction::Invalid(_))
            ));
        }
    }

    #[test]
    fn disconnect_protocol_has_one_bounded_request_frame_and_keeps_the_lease() {
        use std::io::Read as _;
        let mut reader = std::io::Cursor::new(b"{\"mode\":\"open\"}\nlease".as_slice());
        assert_eq!(
            read_picker_request_line(&mut reader).unwrap().mode,
            ottrin_core::picker::PickerMode::Open
        );
        let mut remaining = String::new();
        reader.read_to_string(&mut remaining).unwrap();
        assert_eq!(remaining, "lease");
        assert!(read_picker_request_line(b"{}".as_slice()).is_err());
        assert!(
            read_picker_request_line(vec![b'x'; MAX_PICKER_REQUEST_BYTES + 2].as_slice()).is_err()
        );
        assert!(matches!(
            cli_headless_action(&["picker".into(), "--cancel-on-disconnect".into()]),
            Some(HeadlessAction::Picker {
                cancel_on_disconnect: true
            })
        ));
        assert!(matches!(
            cli_headless_action(&[
                "picker".into(),
                "--cancel-on-disconnect".into(),
                "extra".into()
            ]),
            Some(HeadlessAction::Invalid(_))
        ));
    }

    #[test]
    fn caller_lease_blocks_without_polling_retries_interrupts_and_wakes_on_eof() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        struct Reader {
            input: mpsc::Receiver<std::io::Result<usize>>,
            entered: mpsc::Sender<()>,
        }
        impl std::io::Read for Reader {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.entered.send(()).unwrap();
                self.input.recv().unwrap()
            }
        }
        let (tx, rx) = mpsc::channel();
        let (entered, blocked) = mpsc::channel();
        let (wake, woken) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        watch_picker_disconnect(
            Reader { input: rx, entered },
            cancelled.clone(),
            move || wake.send(()).unwrap(),
        )
        .unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(!cancelled.load(Ordering::Acquire));
        assert!(woken.try_recv().is_err());
        tx.send(Err(std::io::Error::from(std::io::ErrorKind::Interrupted)))
            .unwrap();
        blocked.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(!cancelled.load(Ordering::Acquire));
        tx.send(Ok(0)).unwrap();
        woken.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[cfg(unix)]
    #[test]
    fn picker_requests_are_bounded_and_validated_before_graphics_startup() {
        let request =
            read_picker_request(br#"{"mode":"open","title":"Choose a document"}"#.as_slice())
                .unwrap();
        assert_eq!(request.title, "Choose a document");
        assert!(
            read_picker_request(vec![b' '; MAX_PICKER_REQUEST_BYTES + 1].as_slice())
                .unwrap_err()
                .contains("128 KiB")
        );
        for invalid in [
            r#"{"mode":"save","multiple":true}"#,
            r#"{"unknown":true}"#,
            r#"{"filters":[{"name":"Invalid glob","patterns":[{"kind":"glob","value":"["}]}]}"#,
        ] {
            assert!(
                read_picker_request(invalid.as_bytes()).is_err(),
                "{invalid}"
            );
        }
        let cli =
            parse_cli_arguments(&["picker".into(), "--renderer".into(), "opengl".into()]).unwrap();
        assert!(matches!(
            cli_headless_action(&cli.command),
            Some(HeadlessAction::Picker {
                cancel_on_disconnect: false
            })
        ));
        assert!(matches!(
            cli_headless_action(&["picker".into(), "extra".into()]),
            Some(HeadlessAction::Invalid(_))
        ));
    }

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
            parse_cli_arguments(&["--renderer".into(), "opengl".into()])
                .unwrap()
                .renderer_override,
            Some("opengl".into())
        );
        assert_eq!(
            parse_cli_arguments(&["--renderer=opengl".into()])
                .unwrap()
                .renderer_override,
            Some("opengl".into())
        );
    }

    #[test]
    fn global_gui_options_preserve_target_and_action_in_either_position() {
        let fixture = tempfile::tempdir().unwrap();
        let directory = fixture.path().join("folder with spaces");
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("contact.vcf");
        std::fs::write(&file, "BEGIN:VCARD\nEND:VCARD\n").unwrap();
        for (command, expected) in [
            (
                vec![directory.clone().into_os_string()],
                HeadlessAction::OpenPath(directory.clone()),
            ),
            (
                vec![file.clone().into_os_string()],
                HeadlessAction::OpenPath(file.clone()),
            ),
            (
                vec!["preview".into(), file.clone().into_os_string()],
                HeadlessAction::Preview(file.clone()),
            ),
            (
                vec!["--select".into(), file.clone().into_os_string()],
                HeadlessAction::SelectPath(file.clone()),
            ),
        ] {
            for flags in [
                vec![OsString::from("--renderer"), "opengl".into()],
                vec![OsString::from("--renderer=OpenGL")],
            ] {
                for before in [true, false] {
                    let args = if before {
                        [flags.clone(), command.clone()].concat()
                    } else {
                        [command.clone(), flags.clone()].concat()
                    };
                    let parsed = parse_cli_arguments(&args).unwrap();
                    assert_eq!(parsed.renderer_override.as_deref(), Some("opengl"));
                    assert_eq!(
                        cli_headless_action(&parsed.command),
                        Some(expected.clone()),
                        "{args:?}"
                    );
                }
            }
        }
        assert_eq!(
            resolve_direct_open_path(&directory).unwrap(),
            GuiLaunchTarget::Directory(std::fs::canonicalize(&directory).unwrap())
        );
        assert_eq!(
            resolve_direct_open_path(&file).unwrap(),
            GuiLaunchTarget::QuickInspector(std::fs::canonicalize(&file).unwrap())
        );
        for before in [true, false] {
            let target = vec![directory.clone().into_os_string()];
            let flags = vec![
                "--helper-path".into(),
                "helper with spaces".into(),
                "--renderer=opengl".into(),
            ];
            let args = if before {
                [flags, target].concat()
            } else {
                [target, flags].concat()
            };
            let parsed = parse_cli_arguments(&args).unwrap();
            assert_eq!(
                parsed.helper_path,
                Some(PathBuf::from("helper with spaces"))
            );
            assert_eq!(
                cli_headless_action(&parsed.command),
                Some(HeadlessAction::OpenPath(directory.clone()))
            );
        }
    }

    #[test]
    fn global_gui_options_reject_missing_unsupported_or_repeated_values() {
        for args in [
            vec!["--renderer"],
            vec!["--renderer="],
            vec!["--renderer", "--help"],
            vec!["--renderer=vulkan"],
            vec!["--renderer", "   "],
            vec!["--renderer", "opengl", "--renderer=opengl"],
            vec!["--helper-path"],
            vec!["--helper-path="],
            vec!["--helper-path", "--renderer=opengl"],
            vec!["--helper-path=first", "--helper-path", "second"],
        ] {
            assert!(
                parse_cli_arguments(&args.iter().map(OsString::from).collect::<Vec<_>>()).is_err(),
                "{args:?}"
            );
        }
        let parsed = parse_cli_arguments(&["--helper-path=--literal".into()]).unwrap();
        assert_eq!(parsed.helper_path, Some(PathBuf::from("--literal")));
    }

    #[test]
    fn command_separator_preserves_literal_options_and_help_queries() {
        for query in ["--renderer=opengl", "--helper-path", "--help"] {
            let parsed =
                parse_cli_arguments(&["search".into(), "--".into(), query.into()]).unwrap();
            assert!(!parsed.help);
            assert!(parsed.renderer_override.is_none() && parsed.helper_path.is_none());
            assert_eq!(
                cli_headless_action(&parsed.command),
                Some(HeadlessAction::Search {
                    query: query.into(),
                    json: false,
                    content: false,
                    limit: None,
                    offset: 0,
                })
            );
        }
        for (prefix, expected) in [
            (
                vec![OsString::from("--")],
                HeadlessAction::OpenPath(PathBuf::from("--renderer")),
            ),
            (
                vec!["preview".into(), "--".into()],
                HeadlessAction::Preview(PathBuf::from("--renderer")),
            ),
            (
                vec!["--select".into(), "--".into()],
                HeadlessAction::SelectPath(PathBuf::from("--renderer")),
            ),
        ] {
            let parsed =
                parse_cli_arguments(&[prefix, vec!["--renderer".into()]].concat()).unwrap();
            assert_eq!(cli_headless_action(&parsed.command), Some(expected));
        }
        assert!(
            parse_cli_arguments(&["--renderer=opengl".into(), "--help".into()])
                .unwrap()
                .help
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
                "--select requires an entry path".to_string()
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
        let value = root.clone().into_os_string();

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
            cli_headless_action(&[file.clone().into_os_string()]),
            Some(HeadlessAction::OpenPath(file.clone()))
        );
        assert_eq!(
            cli_headless_action(&[root.join("missing.conf").into_os_string()]),
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
    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn slow_index_startup_returns_control_before_the_daemon_is_ready() {
        let (entered, started) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let (wake, woke) = std::sync::mpsc::channel();
        let result = spawn_index_startup(
            move || {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            },
            move || {
                wake.send(()).unwrap();
            },
        );
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            result.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        assert_eq!(result.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));
        woke.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    #[cfg(any(unix, target_os = "windows"))]
    #[test]
    fn index_startup_failure_is_a_ui_result_instead_of_a_process_exit() {
        let result = spawn_index_startup(|| Err("index unavailable".into()), || {});
        assert_eq!(
            result.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err("index unavailable".into())
        );
    }

    #[test]
    fn index_startup_broker_is_headless_and_rejects_ambiguous_arguments() {
        assert_eq!(
            cli_headless_action(&["index".into(), "ensure".into()]),
            Some(HeadlessAction::IndexEnsure)
        );
        for args in [
            vec!["index".into(), "ensure".into(), "--json".into()],
            vec!["index".into(), "ensure".into(), "rebuild".into()],
        ] {
            assert!(matches!(
                cli_headless_action(&args),
                Some(HeadlessAction::Invalid(_))
            ));
        }
    }
}
