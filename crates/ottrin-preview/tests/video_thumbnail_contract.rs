use ottrin_preview::{
    MAX_VIDEO_THUMBNAIL_SOURCE_BYTES, VideoThumbnailError, VideoThumbnailLimits,
    VideoThumbnailSourceGeneration, bounded_video_thumbnail_dimensions,
    bounded_video_thumbnail_source_length, validate_video_thumbnail_bgra,
    video_thumbnail_cache_key,
};
#[cfg(not(target_os = "windows"))]
use ottrin_preview::{PreviewVisualRenderer, preview_profile};
#[cfg(not(target_os = "windows"))]
use std::path::Path;
use std::time::Duration;

fn limits() -> VideoThumbnailLimits {
    VideoThumbnailLimits {
        max_source_bytes: MAX_VIDEO_THUMBNAIL_SOURCE_BYTES,
        max_source_dimension: 8_192,
        max_source_pixels: 33_554_432,
        max_output_dimension: 2_048,
        max_output_pixels: 4_194_304,
        max_output_bytes: 16 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

#[test]
fn user_facing_video_fallback_distinguishes_missing_system_support_from_bad_media() {
    let missing = VideoThumbnailError::NativeBackendUnavailable.fallback_message();
    let unsupported = VideoThumbnailError::UnsupportedMedia.fallback_message();
    let malformed = VideoThumbnailError::DecodeFailed.fallback_message();

    assert!(missing.contains("operating-system video components"));
    assert!(missing.contains("does not mean the file is invalid"));
    assert!(unsupported.contains("installed codec"));
    assert!(unsupported.contains("does not mean the file is invalid"));
    assert!(malformed.contains("malformed or unsupported"));
    assert!(!malformed.contains("does not mean the file is invalid"));
}

#[test]
fn compressed_video_source_size_is_hard_bounded_at_admission() {
    let limits = VideoThumbnailLimits::for_target(256);

    assert_eq!(limits.max_source_bytes, MAX_VIDEO_THUMBNAIL_SOURCE_BYTES);
    assert_eq!(
        bounded_video_thumbnail_source_length(MAX_VIDEO_THUMBNAIL_SOURCE_BYTES, &limits),
        Ok(MAX_VIDEO_THUMBNAIL_SOURCE_BYTES)
    );
    assert_eq!(
        bounded_video_thumbnail_source_length(
            MAX_VIDEO_THUMBNAIL_SOURCE_BYTES
                .checked_add(1)
                .expect("source limit has arithmetic headroom"),
            &limits,
        ),
        Err(VideoThumbnailError::ResourceLimit)
    );
    assert_eq!(
        bounded_video_thumbnail_source_length(u64::MAX, &limits),
        Err(VideoThumbnailError::ResourceLimit)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn windows_source_admission_rejects_an_oversize_retained_handle() {
    use std::fs::OpenOptions;

    let path = std::env::temp_dir().join(format!(
        "ottrin-oversize-video-admission-{}-{:?}.mp4",
        std::process::id(),
        std::thread::current().id()
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .expect("create sparse oversize video fixture");
    file.set_len(MAX_VIDEO_THUMBNAIL_SOURCE_BYTES + 1)
        .expect("size sparse oversize video fixture");
    drop(file);

    let admitted = ottrin_preview::admit_video_thumbnail_source(&path);
    let _ = std::fs::remove_file(&path);

    assert_eq!(admitted.unwrap_err(), VideoThumbnailError::ResourceLimit);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn video_visuals_are_deterministically_unavailable_off_windows() {
    let path = Path::new("/definitely/not/opened/movie.mp4");

    assert_eq!(
        preview_profile(path).visual_renderer(),
        PreviewVisualRenderer::None
    );
    assert_eq!(
        ottrin_preview::admit_video_thumbnail_source(path).unwrap_err(),
        VideoThumbnailError::UnsupportedPlatform
    );
}

#[test]
fn validated_video_frame_normalizes_bgra_and_rejects_bad_shape_or_bounds() {
    let generation = VideoThumbnailSourceGeneration::from_bytes([7; 32]);
    let frame = validate_video_thumbnail_bgra(
        generation.clone(),
        2,
        1,
        8,
        &[10, 20, 30, 0, 40, 50, 60, 90],
        &limits(),
    )
    .expect("bounded frame");

    assert_eq!(frame.width, 2);
    assert_eq!(frame.height, 1);
    assert_eq!(frame.rgba, [30, 20, 10, 255, 60, 50, 40, 255]);
    assert_eq!(frame.source_generation, generation);

    assert_eq!(
        validate_video_thumbnail_bgra(
            VideoThumbnailSourceGeneration::from_bytes([1; 32]),
            2,
            2,
            7,
            &[0; 16],
            &limits(),
        ),
        Err(VideoThumbnailError::InvalidFrame)
    );

    let mut tight = limits();
    tight.max_output_pixels = 1;
    assert_eq!(
        validate_video_thumbnail_bgra(
            VideoThumbnailSourceGeneration::from_bytes([2; 32]),
            2,
            1,
            8,
            &[0; 8],
            &tight,
        ),
        Err(VideoThumbnailError::ResourceLimit)
    );
}

#[test]
fn validated_video_frame_normalizes_padded_and_signed_stride_orientation() {
    let top_down = [
        10, 20, 30, 0, 40, 50, 60, 0, 1, 2, 3, 4, // top row + padding
        70, 80, 90, 0, 100, 110, 120, 0, 5, 6, 7, 8, // bottom row + padding
    ];
    let expected = [
        30, 20, 10, 255, 60, 50, 40, 255, // top row
        90, 80, 70, 255, 120, 110, 100, 255, // bottom row
    ];
    let padded = validate_video_thumbnail_bgra(
        VideoThumbnailSourceGeneration::from_bytes([8; 32]),
        2,
        2,
        12,
        &top_down,
        &limits(),
    )
    .expect("positive padded stride");
    assert_eq!(padded.rgba, expected);

    let bottom_up = [
        70, 80, 90, 0, 100, 110, 120, 0, 5, 6, 7, 8, // bottom row + padding
        10, 20, 30, 0, 40, 50, 60, 0, 1, 2, 3, 4, // top row + padding
    ];
    let signed = validate_video_thumbnail_bgra(
        VideoThumbnailSourceGeneration::from_bytes([9; 32]),
        2,
        2,
        -12,
        &bottom_up,
        &limits(),
    )
    .expect("negative bottom-up stride");
    assert_eq!(signed.rgba, expected);
}

#[test]
fn video_cache_identity_includes_generation_and_target_dimension() {
    let first = VideoThumbnailSourceGeneration::from_bytes([3; 32]);
    let replacement = VideoThumbnailSourceGeneration::from_bytes([4; 32]);

    assert_ne!(
        video_thumbnail_cache_key(&first, 128),
        video_thumbnail_cache_key(&replacement, 128)
    );
    assert_ne!(
        video_thumbnail_cache_key(&first, 128),
        video_thumbnail_cache_key(&first, 256)
    );
}

#[test]
fn source_dimensions_are_bounded_before_scaling_and_output_allocation() {
    let mut limits = limits();
    limits.max_output_dimension = 256;

    assert_eq!(
        bounded_video_thumbnail_dimensions(1_920, 1_080, &limits),
        Ok((256, 144))
    );
    assert_eq!(
        bounded_video_thumbnail_dimensions(9_000, 10, &limits),
        Err(VideoThumbnailError::ResourceLimit)
    );
    assert_eq!(
        bounded_video_thumbnail_dimensions(0, 10, &limits),
        Err(VideoThumbnailError::ResourceLimit)
    );
}
