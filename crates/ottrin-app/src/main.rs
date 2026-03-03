use eframe::{NativeOptions, egui};
use ottrin_ui::OttrinApp;

fn main() -> eframe::Result<()> {
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ottrin")
            .with_decorations(false)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Ottrin",
        options,
        Box::new(|cc| Ok(Box::new(OttrinApp::new(cc)))),
    )
}
