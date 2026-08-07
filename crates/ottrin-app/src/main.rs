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
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeadlessAction {
    IndexStatus,
    IndexRebuild,
    Search(String),
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
    if handle_cli_action() {
        return;
    }
    install_windows_startup_panic_hook();
    if let Err(error) = run_gui() {
        report_windows_startup_failure("Ottrin could not start", &error.to_string());
        std::process::exit(1);
    }
}

fn run_gui() -> eframe::Result<()> {
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
            Ok(Box::new(OttrinApp::new_with_graphics_status(
                cc,
                graphics_runtime,
            )))
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
        println!("Ottrin");
        println!("  --uninstall        Remove Ottrin background jobs and generated data");
        println!("  --purge-config     With --uninstall, also remove saved preferences");
        println!("  --helper-path PATH Developer override for privileged helper discovery");
        println!("  --renderer opengl  Recovery override for a compatible OpenGL driver");
        println!("  --index status     Print the persisted-search service status");
        println!("  --index rebuild    Rebuild the configured search index and wait for it");
        println!("  --search QUERY     Search the configured index after it is ready");
        println!("  --drop status      Print configured Drop Folder state without changing it");
        println!("  --drop copy PATH…  Copy paths into the configured Drop Folder");
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

fn cli_headless_action(args: &[String]) -> Option<HeadlessAction> {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--index" => {
                return Some(match args.get(index + 1).map(String::as_str) {
                    Some("status") => HeadlessAction::IndexStatus,
                    Some("rebuild") => HeadlessAction::IndexRebuild,
                    Some(value) => {
                        HeadlessAction::Invalid(format!("unknown index action: {value}"))
                    }
                    None => {
                        HeadlessAction::Invalid("--index requires status or rebuild".to_string())
                    }
                });
            }
            "--search" => {
                return Some(
                    match args.get(index + 1).filter(|value| !value.is_empty()) {
                        Some(query) => HeadlessAction::Search(query.clone()),
                        None => HeadlessAction::Invalid("--search requires a query".to_string()),
                    },
                );
            }
            "--drop" => {
                return Some(match args.get(index + 1).map(String::as_str) {
                    Some("status") => HeadlessAction::DropStatus,
                    Some("copy") => HeadlessAction::DropCopy(
                        args[index + 2..].iter().map(PathBuf::from).collect(),
                    ),
                    Some(value) => HeadlessAction::Invalid(format!("unknown drop action: {value}")),
                    None => HeadlessAction::Invalid("--drop requires status or copy".to_string()),
                });
            }
            _ => index += 1,
        }
    }
    None
}

fn configured_search_service() -> ottrin_search::SearchService {
    let mut config = load_config().search;
    if config.include_roots.is_empty() {
        config.include_roots = ottrin_search::default_roots();
    }
    ottrin_search::SearchService::new(config)
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
    let mut service = configured_search_service();
    service.start();
    if matches!(service.diagnostics().status, SearchIndexStatus::Unavailable) {
        return Err(service
            .diagnostics()
            .last_error
            .unwrap_or_else(|| "search service unavailable".to_string()));
    }

    let result = match action {
        HeadlessAction::IndexStatus => {
            print_index_diagnostics(&service.diagnostics());
            Ok(())
        }
        HeadlessAction::IndexRebuild => {
            service.rebuild_index_blocking()?;
            print_index_diagnostics(&service.diagnostics());
            Ok(())
        }
        HeadlessAction::Search(query) => {
            wait_for_index_ready(&service)?;
            let config = load_config().search;
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
            println!("status={:?}", response.status);
            println!("total={}", response.total);
            for item in response.items {
                println!("result={}", item.path.display());
            }
            response.error.map_or(Ok(()), Err)
        }
        HeadlessAction::DropStatus | HeadlessAction::DropCopy(_) => {
            unreachable!("drop actions return before service startup")
        }
        HeadlessAction::Invalid(_) => unreachable!("invalid actions return before service startup"),
    };
    service.stop();
    result
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

fn wait_for_index_ready(service: &ottrin_search::SearchService) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let diagnostics = service.diagnostics();
        match diagnostics.status {
            SearchIndexStatus::Ready => return Ok(()),
            SearchIndexStatus::Unavailable => {
                return Err(diagnostics
                    .last_error
                    .unwrap_or_else(|| "search service unavailable".to_string()));
            }
            SearchIndexStatus::Indexing if Instant::now() >= deadline => {
                return Err("search index did not become ready within 30 seconds".to_string());
            }
            SearchIndexStatus::Indexing => sleep(Duration::from_millis(50)),
        }
    }
}

fn print_index_diagnostics(diagnostics: &ottrin_search::SearchIndexDiagnostics) {
    println!("status={:?}", diagnostics.status);
    println!("indexed_items={}", diagnostics.indexed_items);
    println!("snapshot_revision={}", diagnostics.snapshot_revision);
    println!("configured_roots={}", diagnostics.configured_root_count);
    println!("watched_roots={}", diagnostics.watched_root_count);
    if let Some(detail) = &diagnostics.detail {
        println!("detail={detail}");
    }
    if let Some(error) = &diagnostics.last_error {
        println!("error={error}");
    }
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
    fn headless_index_actions_are_recognized_before_the_gui_starts() {
        assert_eq!(
            cli_headless_action(&["--index".into(), "status".into()]),
            Some(HeadlessAction::IndexStatus)
        );
        assert_eq!(
            cli_headless_action(&["--index".into(), "rebuild".into()]),
            Some(HeadlessAction::IndexRebuild)
        );
        assert_eq!(
            cli_headless_action(&["--search".into(), "invoice".into()]),
            Some(HeadlessAction::Search("invoice".into()))
        );
        assert_eq!(
            cli_headless_action(&["--drop".into(), "status".into()]),
            Some(HeadlessAction::DropStatus)
        );
        assert_eq!(
            cli_headless_action(&[
                "--drop".into(),
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
