#![cfg(target_os = "linux")]

use image::GenericImageView;
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(std::path::PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn installed_html_renderer_produces_bounded_png() {
    if !ottrin_preview::html_preview_runtime_available() {
        eprintln!("skipping HTML renderer smoke test: supported browser/isolation runtime absent");
        return;
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ottrin-html-render-smoke-{}-{unique}.html",
        std::process::id()
    ));
    std::fs::write(
        &path,
        "<!doctype html><title>Ottrin</title><main style='color:#d58b45'>Bounded HTML preview</main>",
    )
    .unwrap();
    let _fixture = Fixture(path.clone());

    let png = ottrin_preview::render_html_content_preview_png(&path, 320).unwrap();
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!(image.dimensions(), (320, 320));
}
