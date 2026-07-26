use eframe::{NativeOptions, egui};
use ottrin_core::format_size;
use ottrin_platform::set_privileged_helper_override;
use ottrin_ui::{OttrinApp, clear_preview_cache_storage, load_config, remove_config_file};
use std::path::PathBuf;

fn renderer_for_target(is_windows: bool) -> eframe::Renderer {
    if is_windows {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    }
}

fn main() -> eframe::Result<()> {
    if handle_cli_action() {
        return Ok(());
    }
    apply_cli_overrides();
    ensure_dev_helper();

    let config = load_config();
    let window_size = config.window_size.unwrap_or([1024.0, 680.0]);

    let options = NativeOptions {
        renderer: renderer_for_target(cfg!(target_os = "windows")),
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
}
