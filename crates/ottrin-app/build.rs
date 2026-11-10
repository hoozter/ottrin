use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile_for(
            "windows.rc",
            ["ottrin-app", "ottrin-indexd"],
            embed_resource::NONE,
        )
        .manifest_required()
        .expect("Windows executables must contain Ottrin's application icon");
        delay_load_windows_opengl();
    }
}

fn delay_load_windows_opengl() {
    // The compatibility DLL lives beside the executable. Loading it at
    // process entry pulls in Mesa even for the default Direct3D renderer.
    match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => {
            println!("cargo:rustc-link-arg-bin=ottrin-app=/DELAYLOAD:opengl32.dll");
            println!("cargo:rustc-link-arg-bin=ottrin-app=delayimp.lib");
        }
        Ok("gnu") => {
            println!("cargo:rerun-if-changed=windows-opengl.def");
            println!("cargo:rerun-if-env-changed=DLLTOOL");
            let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output path"));
            let definition =
                PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
                    .join("windows-opengl.def");
            let default_tool = if env::var("HOST").is_ok_and(|host| host.ends_with("windows-gnu")) {
                "dlltool"
            } else {
                "x86_64-w64-mingw32-dlltool"
            };
            let tool = env::var_os("DLLTOOL").unwrap_or_else(|| default_tool.into());
            let status = Command::new(tool)
                .current_dir(&output)
                .arg("-d")
                .arg(definition)
                .args(["-p", "__wrap_", "-y", "libottrin_opengl_delay.a"])
                .status()
                .expect("Run MinGW dlltool to create the delayed OpenGL import library");
            assert!(
                status.success(),
                "MinGW could not generate delayed OpenGL imports"
            );
            // A transitive Windows import library can be searched before
            // -lopengl32. Redirect both call and import-pointer references
            // so they bind to the same delay-load thunk.
            for symbol in include_str!("windows-opengl.def")
                .lines()
                .skip(2)
                .map(str::trim)
            {
                println!(
                    "cargo:rustc-link-arg-bin=ottrin-app=-Wl,--wrap={symbol},--wrap=__imp_{symbol},--defsym=__wrap___imp_{symbol}=__imp___wrap_{symbol}"
                );
            }
            println!(
                "cargo:rustc-link-arg-bin=ottrin-app={}",
                output.join("libottrin_opengl_delay.a").display()
            );
            // The generated archive references MinGW's delay helper.
            // Rescan mingwex after it because Rust's normal link group
            // precedes these binary-specific arguments.
            println!("cargo:rustc-link-arg-bin=ottrin-app=-lmingwex");
            println!("cargo:rustc-link-arg-bin=ottrin-app=-lkernel32");
        }
        _ => {}
    }
}
