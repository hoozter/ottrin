#![cfg(target_os = "linux")]

use std::io::{Read, Seek, SeekFrom};
#[cfg(not(feature = "system-gstreamer"))]
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::PathBuf;
#[cfg(not(feature = "system-gstreamer"))]
use std::sync::Mutex;
#[cfg(not(feature = "system-gstreamer"))]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use ottrin_preview::{
    MAX_MEDIA_PLAYBACK_SOURCE_BYTES, MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION,
    MediaPlaybackAdmissionError, MediaPlaybackFrameError, admit_media_playback_source,
    validate_media_playback_rgba_frame,
};
#[cfg(not(feature = "system-gstreamer"))]
use ottrin_preview::{MediaPlaybackEvent, start_media_playback};

#[cfg(not(feature = "system-gstreamer"))]
static MEDIA_HELPER_ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ottrin-media-playback-{}-{nonce}-{name}",
        std::process::id()
    ))
}

#[test]
fn admitted_media_source_retains_the_open_regular_file_after_path_replacement() {
    let path = temp_path("retained.mp4");
    std::fs::write(&path, b"admitted media").expect("write admitted source");
    let original_inode = std::fs::metadata(&path).expect("source metadata").ino();

    let source = admit_media_playback_source(&path).expect("admit regular media source");
    let replacement = temp_path("replacement.mp4");
    std::fs::write(&replacement, b"replacement").expect("write replacement");
    std::fs::rename(&replacement, &path).expect("replace path");

    assert_eq!(source.len(), 14);
    assert_eq!(
        source.identity().device(),
        std::fs::metadata(&path).unwrap().dev()
    );
    assert_eq!(source.identity().inode(), original_inode);
    let mut retained = source.try_clone_file().expect("clone retained descriptor");
    retained.seek(SeekFrom::Start(0)).expect("rewind source");
    let mut bytes = Vec::new();
    retained
        .read_to_end(&mut bytes)
        .expect("read retained source");
    assert_eq!(bytes, b"admitted media");

    std::fs::remove_file(path).expect("remove fixture");
}

#[test]
fn media_source_admission_rejects_symlinks_without_following_them() {
    let target = temp_path("symlink-target.mp4");
    let link = temp_path("symlink.mp4");
    std::fs::write(&target, b"media").expect("write symlink target");
    symlink(&target, &link).expect("create symlink");

    let error = admit_media_playback_source(&link).expect_err("reject symlink source");

    assert!(matches!(error, MediaPlaybackAdmissionError::UnsafeSource));
    std::fs::remove_file(link).expect("remove symlink");
    std::fs::remove_file(target).expect("remove target");
}

#[test]
fn media_source_admission_rejects_non_regular_files() {
    let directory = temp_path("directory");
    std::fs::create_dir(&directory).expect("create fixture directory");

    let error = admit_media_playback_source(&directory).expect_err("reject directory source");

    assert!(matches!(error, MediaPlaybackAdmissionError::UnsafeSource));
    std::fs::remove_dir(directory).expect("remove fixture directory");
}

#[test]
fn media_source_admission_rejects_sources_over_the_bound() {
    let path = temp_path("oversized.mp4");
    let file = std::fs::File::create(&path).expect("create sparse fixture");
    file.set_len(MAX_MEDIA_PLAYBACK_SOURCE_BYTES + 1)
        .expect("size sparse fixture");

    let error = admit_media_playback_source(&path).expect_err("reject oversized source");

    assert!(matches!(error, MediaPlaybackAdmissionError::SourceTooLarge));
    std::fs::remove_file(path).expect("remove sparse fixture");
}

#[test]
fn decoded_video_frame_rejects_dimensions_above_the_playback_bound() {
    let error = validate_media_playback_rgba_frame(MAX_MEDIA_PLAYBACK_VIDEO_DIMENSION + 1, 1, &[])
        .expect_err("oversized decoded dimensions must be rejected");

    assert!(matches!(
        error,
        MediaPlaybackFrameError::DimensionsExceeded { .. }
    ));
}

#[test]
fn decoded_video_frame_requires_the_exact_bounded_rgba_payload() {
    let error = validate_media_playback_rgba_frame(2, 2, &[0; 15])
        .expect_err("truncated decoded frame must be rejected");

    assert!(matches!(
        error,
        MediaPlaybackFrameError::InvalidPayload {
            expected: 16,
            actual: 15
        }
    ));
}

#[cfg(not(feature = "system-gstreamer"))]
#[test]
fn playback_worker_truthfully_reports_an_unavailable_system_helper() {
    let _environment = MEDIA_HELPER_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    unsafe { std::env::set_var("OTTRIN_MEDIA_HELPER", temp_path("missing-helper")) };
    let path = temp_path("runtime-unavailable.mp4");
    std::fs::write(&path, b"admitted media").expect("write admitted source");
    let source = admit_media_playback_source(&path).expect("admit media source");

    let mut session = start_media_playback(source).expect("start bounded worker");
    let event = session
        .recv_timeout(Duration::from_secs(1))
        .expect("worker must report runtime state");

    assert!(
        matches!(event, MediaPlaybackEvent::Failed(ref message)
        if message.contains("ottrin-media-playback")
            && message.contains("cargo build -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer")
            && message.contains("No such file")),
        "{event:?}"
    );
    session.stop();
    unsafe { std::env::remove_var("OTTRIN_MEDIA_HELPER") };
    std::fs::remove_file(path).expect("remove fixture");
}

#[cfg(not(feature = "system-gstreamer"))]
#[test]
fn playback_worker_passes_only_the_retained_descriptor_to_the_helper() {
    let _environment = MEDIA_HELPER_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let helper = temp_path("fake-helper.py");
    std::fs::write(
        &helper,
        r#"#!/usr/bin/python3
import os
import socket

control = socket.socket(fileno=4)
if os.read(3, 64) != b"retained media":
    raise SystemExit(3)
control.sendall(b"\x01")
if control.recv(1) != b"\x02":
    raise SystemExit(4)
control.sendall(b"\x02")
control.recv(1)
"#,
    )
    .expect("write fake helper");
    let mut permissions = std::fs::metadata(&helper).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&helper, permissions).expect("make fake helper executable");
    unsafe { std::env::set_var("OTTRIN_MEDIA_HELPER", &helper) };

    let path = temp_path("retained-helper-source.mp4");
    std::fs::write(&path, b"retained media").expect("write media source");
    let source = admit_media_playback_source(&path).expect("admit media source");
    let replacement = temp_path("helper-replacement.mp4");
    std::fs::write(&replacement, b"changed path bytes").expect("write replacement");
    std::fs::rename(replacement, &path).expect("replace admitted path");

    let mut session = start_media_playback(source).expect("start helper bridge");
    assert_eq!(
        session.recv_timeout(Duration::from_secs(1)).unwrap(),
        MediaPlaybackEvent::Playing
    );
    session.pause();
    assert_eq!(
        session.recv_timeout(Duration::from_secs(1)).unwrap(),
        MediaPlaybackEvent::Paused
    );
    session.stop();

    unsafe { std::env::remove_var("OTTRIN_MEDIA_HELPER") };
    std::fs::remove_file(path).expect("remove media source");
    std::fs::remove_file(helper).expect("remove fake helper");
}

#[cfg(not(feature = "system-gstreamer"))]
#[test]
fn playback_worker_reports_helper_process_failure() {
    let _environment = MEDIA_HELPER_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let helper = temp_path("failed-helper.sh");
    std::fs::write(&helper, "#!/bin/sh\nexit 42\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    unsafe { std::env::set_var("OTTRIN_MEDIA_HELPER", &helper) };
    let path = temp_path("failed-helper-source.mp4");
    std::fs::write(&path, b"media").unwrap();
    let mut session = start_media_playback(admit_media_playback_source(&path).unwrap()).unwrap();
    let event = session
        .recv_timeout(Duration::from_secs(1))
        .expect("helper failure must be reported");
    assert!(
        matches!(event, MediaPlaybackEvent::Failed(ref message) if message.contains("42")),
        "{event:?}"
    );
    session.stop();
    unsafe { std::env::remove_var("OTTRIN_MEDIA_HELPER") };
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(helper).unwrap();
}
