use eframe::{NativeOptions, egui};
use ottrin_platform::set_privileged_helper_override;
use ottrin_ui::{OttrinApp, load_config};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    apply_cli_overrides();
    ensure_dev_helper();

    let config = load_config();
    let window_size = config.window_size.unwrap_or([1024.0, 680.0]);

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ottrin")
            .with_decorations(false)
            .with_transparent(true)
            .with_inner_size(window_size)
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ottrin",
        options,
        Box::new(|cc| Ok(Box::new(OttrinApp::new(cc)))),
    )
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
        manifest_dir.join("target").join("debug").join(format!("{helper_name}.exe"))
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
