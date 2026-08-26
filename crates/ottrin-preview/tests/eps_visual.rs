#![cfg(target_os = "linux")]

use ottrin_preview::{eps_preview_runtime_is_available, render_eps_content_preview_png};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn installed_ghostscript_renders_a_representative_eps_inside_isolation() {
    if !eps_preview_runtime_is_available() {
        return;
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ottrin-eps-visual-contract-{}-{unique}.eps",
        std::process::id()
    ));
    std::fs::write(
        &path,
        b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 300 180\n/Courier findfont 24 scalefont setfont\n20 100 moveto (Ottrin EPS Preview) show\nnewpath 20 20 moveto 280 20 lineto 280 160 lineto 20 160 lineto closepath stroke\nshowpage\n%%EOF\n",
    )
    .unwrap();

    let rendered =
        render_eps_content_preview_png(&path, 512).expect("render installed EPS provider");
    let image = image::load_from_memory(&rendered).expect("decode rendered PNG");
    assert!(image.width() > 100 && image.width() <= 512);
    assert!(image.height() > 50 && image.height() <= 512);
    assert!(
        rendered.len() > 1_000,
        "render must contain visible EPS content"
    );

    let _ = std::fs::remove_file(path);
}
