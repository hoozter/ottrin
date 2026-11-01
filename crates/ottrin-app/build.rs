fn main() {
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for(
            "windows.rc",
            ["ottrin-app", "ottrin-indexd"],
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("Windows executables must contain Ottrin's application icon");
    }
}
