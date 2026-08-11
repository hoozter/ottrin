#[cfg(not(target_os = "windows"))]
use ottrin_preview::{PreviewVisualRenderer, preview_profile};
use ottrin_preview::{
    VideoThumbnailError, VideoThumbnailLimits, VideoThumbnailSourceGeneration,
    bounded_video_thumbnail_dimensions, validate_video_thumbnail_bgra, video_thumbnail_cache_key,
};
#[cfg(not(target_os = "windows"))]
use std::path::Path;
use std::time::Duration;

fn limits() -> VideoThumbnailLimits {
    VideoThumbnailLimits {
        max_source_dimension: 8_192,
        max_source_pixels: 33_554_432,
        max_output_dimension: 2_048,
        max_output_pixels: 4_194_304,
        max_output_bytes: 16 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
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
