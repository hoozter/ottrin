fn main() {
    #[cfg(target_os = "linux")]
    let result = ottrin_preview::run_html_renderer_helper(std::env::args_os().skip(1).collect());
    #[cfg(not(target_os = "linux"))]
    let result: Result<(), String> = Err("The HTML namespace helper is Linux-only".into());
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
