use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub(crate) fn rotate_image_file(path: &Path, clockwise: bool) -> Result<(), String> {
    let img = image::ImageReader::open(path)
        .map_err(|err| err.to_string())?
        .with_guessed_format()
        .map_err(|err| err.to_string())?
        .decode()
        .map_err(|err| err.to_string())?;
    let rotated = if clockwise {
        img.rotate90()
    } else {
        img.rotate270()
    };
    rotated.save(path).map_err(|err| err.to_string())
}

pub(crate) fn compute_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    Ok(out)
}
