#![cfg_attr(windows, windows_subsystem = "windows")]

use eframe::{NativeOptions, egui};
use ottrin_core::{ConflictAction, FileCommand, SearchIndexStatus, SearchQuery, format_size};
use ottrin_platform::{DefaultPlatform, PlatformOps, set_privileged_helper_override};
use ottrin_ui::{OttrinApp, clear_preview_cache_storage, load_config, remove_config_file};
use std::collections::HashSet;
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

fn renderer_for_target(is_windows: bool) -> eframe::Renderer {
    if is_windows {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    }
}

/// Keeps the native WGPU path as the default while allowing an explicit recovery
/// route for Windows adapters that need an OpenGL driver supplied by the user.
fn renderer_for_target_with_override(
    is_windows: bool,
    renderer_override: Option<&str>,
) -> eframe::Renderer {
    match renderer_override.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("opengl") => eframe::Renderer::Glow,
        _ => renderer_for_target(is_windows),
    }
}

fn main() -> eframe::Result<()> {
    if handle_cli_action() {
        return Ok(());
    }
    apply_cli_overrides();
    ensure_dev_helper();

    let renderer_override = match graphics_renderer_override() {
        Ok(renderer_override) => renderer_override,
        Err(error) => {
            eprintln!("ottrin: {error}");
            std::process::exit(2);
        }
    };

    let config = load_config();
    let window_size = config.window_size.unwrap_or([1024.0, 680.0]);

    let options = NativeOptions {
        renderer: renderer_for_target_with_override(
            cfg!(target_os = "windows"),
            renderer_override.as_deref(),
        ),
        viewport: egui::ViewportBuilder::default()
            .with_title("Ottrin")
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size(window_size)
            .with_min_inner_size([800.0, 500.0])
            .with_icon(load_window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Ottrin",
        options,
        Box::new(|cc| Ok(Box::new(OttrinApp::new(cc)))),
    )
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
    let helper_path = if cfg!(target_os = "windows") {
        manifest_dir
            .join("target")
            .join("debug")
            .join(format!("{helper_name}.exe"))
    } else {
        manifest_dir.join("target").join("debug").join(helper_name)
    };
    if helper_path.is_file() {
        set_privileged_helper_override(Some(helper_path));
        return;
    }

    let status = std::process::Command::new("cargo")
        .args(["build", "-p", "ottrin-platform", "--bin", helper_name])
        .current_dir(&manifest_dir)
        .status();

    if status.map(|s| s.success()).unwrap_or(false) && helper_path.is_file() {
        set_privileged_helper_override(Some(helper_path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_renderer_does_not_require_opengl() {
        assert_eq!(renderer_for_target(true), eframe::Renderer::Wgpu);
    }

    #[test]
    fn graphics_recovery_override_can_select_opengl_without_changing_windows_default() {
        assert_eq!(
            renderer_for_target_with_override(true, Some("opengl")),
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
}
