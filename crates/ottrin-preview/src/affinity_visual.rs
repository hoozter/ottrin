use crc32fast::Hasher;
use flate2::{Decompress, FlushDecompress, Status};
use image::GenericImageView;
#[cfg(unix)]
use sha2::{Digest, Sha256};
use std::ops::Range;
use std::path::Path;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
pub const MAX_AFFINITY_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CANDIDATES: usize = 128;
const MAX_CANDIDATE_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_CANDIDATE_IDAT_BYTES: usize = 6 * 1024 * 1024;
const MAX_AGGREGATE_CANDIDATE_BYTES: usize = 24 * 1024 * 1024;
const MIN_PREVIEW_SIDE: u32 = 128;
const MAX_PREVIEW_SIDE: u32 = 1024;
const MAX_PREVIEW_PIXELS: u64 = 1024 * 1024;
const MAX_DECODED_PNG_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffinityPreviewSourceGeneration(pub(crate) [u8; 32]);

impl AffinityPreviewSourceGeneration {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedAffinityPreview {
    pub png: Vec<u8>,
    pub source_generation: AffinityPreviewSourceGeneration,
}

#[derive(Debug, thiserror::Error)]
pub enum AffinityVisualError {
    #[error("Affinity previews support only .afphoto, .afdesign, and .afpub files")]
    UnsupportedFormat,
    #[error("could not inspect Affinity input: {0}")]
    InputMetadata(String),
    #[error("Affinity source is not a regular file")]
    InputNotRegular,
    #[error(
        "Affinity previews are disabled on this platform until native safe-file admission is available"
    )]
    PreviewDisabledOnPlatform,
    #[error("Affinity input is too large for a bounded preview ({actual} bytes; limit {limit})")]
    InputTooLarge { actual: u64, limit: u64 },
    #[error("could not read Affinity input: {0}")]
    Read(String),
    #[error("Affinity input changed while its bounded snapshot was read")]
    InputChanged,
    #[error("Affinity input contains no qualifying complete embedded PNG")]
    NoCandidate,
    #[error("embedded PNG is malformed or outside bounded preview limits")]
    InvalidCandidate,
    #[error("embedded PNG decoder rejected the bounded candidate")]
    Decode,
    #[error("could not encode Affinity preview PNG: {0}")]
    Encode(String),
}

#[derive(Debug)]
struct Snapshot {
    bytes: Vec<u8>,
    generation: AffinityPreviewSourceGeneration,
}

#[derive(Debug)]
struct Candidate {
    span: Range<usize>,
    width: u32,
    height: u32,
}

pub fn affinity_preview_source_generation(path: &Path) -> Option<AffinityPreviewSourceGeneration> {
    read_snapshot(path).ok().map(|snapshot| snapshot.generation)
}

/// True only if the current bounded source snapshot contains an embedded PNG
/// that is safe to offer to the dedicated renderer.
pub fn affinity_preview_is_available(path: &Path) -> bool {
    parse_affinity_snapshot(path).is_ok()
}

pub fn render_affinity_embedded_preview_png(
    path: &Path,
    target_dim: u32,
) -> Result<Vec<u8>, AffinityVisualError> {
    Ok(render_affinity_embedded_preview_png_with_generation(path, target_dim)?.png)
}

pub fn render_affinity_embedded_preview_png_with_generation(
    path: &Path,
    target_dim: u32,
) -> Result<RenderedAffinityPreview, AffinityVisualError> {
    let (snapshot, candidate) = parse_affinity_snapshot(path)?;
    let png = snapshot
        .bytes
        .get(candidate.span)
        .ok_or(AffinityVisualError::InvalidCandidate)?;
    let image = image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png)
        .decode()
        .map_err(|_| AffinityVisualError::Decode)?;
    if image.dimensions() != (candidate.width, candidate.height) {
        return Err(AffinityVisualError::Decode);
    }
    let bounded_dim = target_dim.clamp(64, MAX_PREVIEW_SIDE);
    let thumbnail = image.thumbnail(bounded_dim, bounded_dim);
    let mut output = Vec::new();
    thumbnail
        .write_to(
            &mut std::io::Cursor::new(&mut output),
            image::ImageFormat::Png,
        )
        .map_err(|error| AffinityVisualError::Encode(error.to_string()))?;
    Ok(RenderedAffinityPreview {
        png: output,
        source_generation: snapshot.generation,
    })
}

fn parse_affinity_snapshot(path: &Path) -> Result<(Snapshot, Candidate), AffinityVisualError> {
    if !crate::is_affinity_path(path) {
        return Err(AffinityVisualError::UnsupportedFormat);
    }
    let snapshot = read_snapshot(path)?;
    let candidate = select_candidate(&snapshot.bytes)?;
    Ok((snapshot, candidate))
}

#[cfg(unix)]
fn read_snapshot(path: &Path) -> Result<Snapshot, AffinityVisualError> {
    let path_metadata = std::fs::symlink_metadata(path)
        .map_err(|error| AffinityVisualError::InputMetadata(error.to_string()))?;
    if !path_metadata.is_file() {
        return Err(AffinityVisualError::InputNotRegular);
    }
    let mut file = open_affinity_source(path)?;
    let opened_metadata = file
        .metadata()
        .map_err(|error| AffinityVisualError::InputMetadata(error.to_string()))?;
    if !opened_metadata.is_file() || !same_source_identity(&path_metadata, &opened_metadata) {
        return Err(AffinityVisualError::InputNotRegular);
    }
    let input_size = opened_metadata.len();
    if input_size > MAX_AFFINITY_PREVIEW_BYTES {
        return Err(AffinityVisualError::InputTooLarge {
            actual: input_size,
            limit: MAX_AFFINITY_PREVIEW_BYTES,
        });
    }
    let capacity = usize::try_from(input_size).map_err(|_| AffinityVisualError::InputTooLarge {
        actual: input_size,
        limit: MAX_AFFINITY_PREVIEW_BYTES,
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    use std::io::Read;
    file.by_ref()
        .take(MAX_AFFINITY_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AffinityVisualError::Read(error.to_string()))?;
    let final_size = file
        .metadata()
        .map_err(|error| AffinityVisualError::Read(error.to_string()))?
        .len();
    if bytes.len() as u64 > MAX_AFFINITY_PREVIEW_BYTES {
        return Err(AffinityVisualError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_AFFINITY_PREVIEW_BYTES,
        });
    }
    let current_metadata =
        std::fs::symlink_metadata(path).map_err(|_| AffinityVisualError::InputChanged)?;
    if bytes.len() as u64 != input_size
        || final_size != input_size
        || !current_metadata.is_file()
        || !same_source_identity(&opened_metadata, &current_metadata)
    {
        return Err(AffinityVisualError::InputChanged);
    }
    Ok(Snapshot {
        generation: AffinityPreviewSourceGeneration(Sha256::digest(&bytes).into()),
        bytes,
    })
}

#[cfg(unix)]
fn open_affinity_source(path: &Path) -> Result<std::fs::File, AffinityVisualError> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| AffinityVisualError::InputMetadata(error.to_string()))
}

#[cfg(unix)]
fn same_source_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn read_snapshot(_path: &Path) -> Result<Snapshot, AffinityVisualError> {
    Err(AffinityVisualError::PreviewDisabledOnPlatform)
}

fn select_candidate(bytes: &[u8]) -> Result<Candidate, AffinityVisualError> {
    let mut selected = None;
    let mut candidate_count = 0usize;
    let mut aggregate_bytes = 0usize;
    let mut offset = 0usize;
    while let Some(relative) = bytes[offset..]
        .windows(PNG_SIGNATURE.len())
        .position(|window| window == PNG_SIGNATURE)
    {
        let start = offset
            .checked_add(relative)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        offset = start.saturating_add(1);
        let Ok(candidate) = validate_png(bytes, start) else {
            continue;
        };
        candidate_count = candidate_count
            .checked_add(1)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        if candidate_count > MAX_CANDIDATES {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        let candidate_bytes = candidate.span.len();
        aggregate_bytes = aggregate_bytes
            .checked_add(candidate_bytes)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        if aggregate_bytes > MAX_AGGREGATE_CANDIDATE_BYTES {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        if candidate.width >= MIN_PREVIEW_SIDE
            && candidate.height >= MIN_PREVIEW_SIDE
            && candidate.width <= MAX_PREVIEW_SIDE
            && candidate.height <= MAX_PREVIEW_SIDE
        {
            // The documented best-effort policy intentionally picks the last
            // qualifying embedded composite from bounded snapshot order.
            selected = Some(candidate);
        }
    }
    selected.ok_or(AffinityVisualError::NoCandidate)
}

fn validate_png(bytes: &[u8], start: usize) -> Result<Candidate, AffinityVisualError> {
    if bytes.get(
        start
            ..start
                .checked_add(PNG_SIGNATURE.len())
                .ok_or(AffinityVisualError::InvalidCandidate)?,
    ) != Some(PNG_SIGNATURE)
    {
        return Err(AffinityVisualError::InvalidCandidate);
    }
    let mut offset = start + PNG_SIGNATURE.len();
    let mut dimensions = None;
    let mut saw_idat = false;
    let mut after_idat = false;
    let mut compressed = Vec::new();
    loop {
        let header_end = offset
            .checked_add(8)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        let header = bytes
            .get(offset..header_end)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        let length = u32::from_be_bytes(
            header[..4]
                .try_into()
                .map_err(|_| AffinityVisualError::InvalidCandidate)?,
        ) as usize;
        let data_start = header_end;
        let data_end = data_start
            .checked_add(length)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        let chunk_end = data_end
            .checked_add(4)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        if chunk_end
            .checked_sub(start)
            .ok_or(AffinityVisualError::InvalidCandidate)?
            > MAX_CANDIDATE_PNG_BYTES
        {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        let data = bytes
            .get(data_start..data_end)
            .ok_or(AffinityVisualError::InvalidCandidate)?;
        let stored_crc = u32::from_be_bytes(
            bytes
                .get(data_end..chunk_end)
                .ok_or(AffinityVisualError::InvalidCandidate)?
                .try_into()
                .map_err(|_| AffinityVisualError::InvalidCandidate)?,
        );
        let mut hasher = Hasher::new();
        hasher.update(&header[4..]);
        hasher.update(data);
        if hasher.finalize() != stored_crc {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        let kind = &header[4..];
        if dimensions.is_none() {
            if kind != b"IHDR" || data.len() != 13 {
                return Err(AffinityVisualError::InvalidCandidate);
            }
            let width = u32::from_be_bytes(
                data[..4]
                    .try_into()
                    .map_err(|_| AffinityVisualError::InvalidCandidate)?,
            );
            let height = u32::from_be_bytes(
                data[4..8]
                    .try_into()
                    .map_err(|_| AffinityVisualError::InvalidCandidate)?,
            );
            let pixels = u64::from(width)
                .checked_mul(u64::from(height))
                .ok_or(AffinityVisualError::InvalidCandidate)?;
            if width == 0
                || height == 0
                || width > MAX_PREVIEW_SIDE
                || height > MAX_PREVIEW_SIDE
                || pixels > MAX_PREVIEW_PIXELS
            {
                return Err(AffinityVisualError::InvalidCandidate);
            }
            dimensions = Some((width, height));
        } else if kind == b"IHDR" {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        if kind == b"IDAT" {
            if after_idat {
                return Err(AffinityVisualError::InvalidCandidate);
            }
            saw_idat = true;
            let total = compressed
                .len()
                .checked_add(data.len())
                .ok_or(AffinityVisualError::InvalidCandidate)?;
            if total > MAX_CANDIDATE_IDAT_BYTES {
                return Err(AffinityVisualError::InvalidCandidate);
            }
            compressed.extend_from_slice(data);
        } else if saw_idat {
            after_idat = true;
        }
        offset = chunk_end;
        if kind == b"IEND" {
            if !data.is_empty() || !saw_idat || !after_idat {
                return Err(AffinityVisualError::InvalidCandidate);
            }
            validate_zlib_stream(&compressed)?;
            let (width, height) = dimensions.ok_or(AffinityVisualError::InvalidCandidate)?;
            return Ok(Candidate {
                span: start..offset,
                width,
                height,
            });
        }
    }
}

fn validate_zlib_stream(input: &[u8]) -> Result<(), AffinityVisualError> {
    let mut decompressor = Decompress::new(true);
    let mut consumed = 0usize;
    loop {
        let mut output = [0u8; 8192];
        let before_in = decompressor.total_in();
        let before_out = decompressor.total_out();
        let status = decompressor
            .decompress(&input[consumed..], &mut output, FlushDecompress::None)
            .map_err(|_| AffinityVisualError::InvalidCandidate)?;
        consumed = usize::try_from(decompressor.total_in())
            .map_err(|_| AffinityVisualError::InvalidCandidate)?;
        if decompressor.total_out() > MAX_DECODED_PNG_BYTES {
            return Err(AffinityVisualError::InvalidCandidate);
        }
        if status == Status::StreamEnd {
            return (consumed == input.len())
                .then_some(())
                .ok_or(AffinityVisualError::InvalidCandidate);
        }
        if decompressor.total_in() == before_in && decompressor.total_out() == before_out {
            return Err(AffinityVisualError::InvalidCandidate);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("ottrin-affinity-{unique}-{name}"))
    }

    fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba(color));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                width,
                height,
                image::ColorType::Rgba8.into(),
            )
            .expect("encode fixture PNG");
        bytes
    }

    #[test]
    fn last_qualifying_embedded_png_is_selected_from_a_single_snapshot() {
        let path = temp_path("selection.afphoto");
        let small = png(64, 64, [255, 0, 0, 255]);
        let middle = png(256, 128, [0, 255, 0, 255]);
        let final_composite = png(512, 512, [0, 0, 255, 255]);
        let source = [
            b"container".as_slice(),
            small.as_slice(),
            b"middle".as_slice(),
            middle.as_slice(),
            b"later".as_slice(),
            final_composite.as_slice(),
        ]
        .concat();
        std::fs::write(&path, source).expect("write fixture");

        let rendered = render_affinity_embedded_preview_png_with_generation(&path, 512)
            .expect("select final bounded composite");
        let decoded = image::load_from_memory(&rendered.png).expect("decode output");
        assert_eq!(decoded.dimensions(), (512, 512));
        assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert!(affinity_preview_is_available(&path));
        assert_eq!(rendered.source_generation.as_bytes().len(), 32);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn malformed_and_oversized_embedded_pngs_fall_back_without_decoding() {
        let valid = png(256, 256, [10, 20, 30, 255]);
        let mut crc_corrupt = valid.clone();
        let last = crc_corrupt.len() - 1;
        crc_corrupt[last] ^= 0x80;
        let mut oversized_ihdr = valid.clone();
        oversized_ihdr[16..20].copy_from_slice(&2048u32.to_be_bytes());
        let truncated = valid[..valid.len() - 5].to_vec();
        for (name, bytes) in [
            ("crc.afdesign", crc_corrupt),
            ("ihdr.afdesign", oversized_ihdr),
            ("truncated.afdesign", truncated),
            (
                "fake.afdesign",
                b"prefix\x89PNG\r\n\x1a\nnot-a-png".to_vec(),
            ),
        ] {
            let path = temp_path(name);
            std::fs::write(&path, bytes).expect("write hostile fixture");
            assert!(!affinity_preview_is_available(&path));
            assert!(render_affinity_embedded_preview_png(&path, 256).is_err());
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn source_replacement_changes_generation_and_invalidates_availability() {
        let path = temp_path("race.afpub");
        std::fs::write(&path, png(256, 256, [1, 2, 3, 255])).expect("write first source");
        let first = affinity_preview_source_generation(&path).expect("first generation");
        std::fs::write(&path, b"replaced without embedded preview").expect("replace source");
        let second = affinity_preview_source_generation(&path).expect("second generation");
        assert_ne!(first, second);
        assert!(!affinity_preview_is_available(&path));
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_fifos_are_rejected_without_following_or_blocking() {
        use std::os::unix::fs::symlink;
        let target = temp_path("target.afphoto");
        let link = temp_path("link.afphoto");
        std::fs::write(&target, png(256, 256, [1, 2, 3, 255])).expect("write target");
        symlink(&target, &link).expect("make symlink");
        assert!(matches!(
            render_affinity_embedded_preview_png(&link, 256),
            Err(AffinityVisualError::InputNotRegular | AffinityVisualError::InputMetadata(_))
        ));
        let fifo = temp_path("pipe.afphoto");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(status.success());
        assert!(matches!(
            render_affinity_embedded_preview_png(&fifo, 256),
            Err(AffinityVisualError::InputNotRegular | AffinityVisualError::InputMetadata(_))
        ));
        let _ = std::fs::remove_file(target);
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_file(fifo);
    }
}
