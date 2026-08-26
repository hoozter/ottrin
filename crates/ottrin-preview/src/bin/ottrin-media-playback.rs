#![cfg(target_os = "linux")]

fn main() {
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[1] != "--inherited-media-session" {
        eprintln!("ottrin-media-playback is an internal Ottrin helper");
        std::process::exit(2);
    }
    if let Err(error) = ottrin_preview::run_media_playback_helper() {
        eprintln!("ottrin-media-playback: {error}");
        std::process::exit(1);
    }
}
