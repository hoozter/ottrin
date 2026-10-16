#![cfg(any(target_os = "linux", target_os = "windows"))]

use ottrin_preview::{eps_preview_runtime_is_available, render_eps_content_preview_png};

#[test]
fn installed_ghostscript_renders_a_representative_eps_inside_isolation() {
    let available = eps_preview_runtime_is_available();
    if std::env::var_os("OTTRIN_REQUIRE_EPS_RUNTIME").is_some() {
        assert!(
            available,
            "the configured native lane requires its installed renderer"
        );
    }
    if !available {
        return;
    }

    let fixture = tempfile::tempdir().expect("private EPS fixture");
    let path = fixture.path().join("drawing.eps");
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
}
