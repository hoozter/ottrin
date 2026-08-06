//! Bounded local text-edit snapshots and atomic publication.
//!
//! This module deliberately does not classify file formats. The preview layer
//! decides whether a file is a supported text preview; this boundary admits
//! only a complete, regular, UTF-8 local file and rechecks its identity and
//! content generation immediately before it publishes a replacement.

use same_file::Handle;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

/// The largest complete text file the first Inspector editor will load.
///
/// This matches the existing bounded text preview surface. Larger files remain
/// read-only rather than presenting a truncated buffer as editable content.
pub const MAX_EDITABLE_TEXT_BYTES: u64 = 128 * 1024;

/// Atomic replacement is enabled only where this implementation has the
/// required same-directory publication semantics. Other platforms stay
/// read-only until their native replacement boundary is proven.
pub const fn text_editing_supported() -> bool {
    cfg!(target_family = "unix")
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TextEditError {
    #[error("atomic text editing is not available on this platform")]
    NotAvailable,
    #[error("only regular local files can be edited")]
    NotRegularFile,
    #[error("symbolic links cannot be edited")]
    Symlink,
    #[error("hard-linked files cannot be atomically replaced without changing their link topology")]
    HardLinked,
    #[error("text files larger than {limit} bytes remain read-only")]
    TooLarge { limit: u64 },
    #[error("only valid UTF-8 text can be edited")]
    InvalidUtf8,
    #[error("the file changed after editing began; your unsaved changes are still in the editor")]
    Conflict,
    #[error("could not save the edited text: {0}")]
    Write(String),
    #[error("could not open the text file: {0}")]
    Read(String),
}

/// An immutable source binding captured when editing begins.
///
/// The buffer belongs to the UI. Keeping this binding immutable means a failed
/// save cannot consume or replace that buffer.
#[derive(Debug)]
pub struct TextEditSession {
    path: PathBuf,
    source: TextEditSnapshot,
}

#[derive(Debug)]
struct TextEditSnapshot {
    identity: Handle,
    bytes: Vec<u8>,
    digest: [u8; 32],
    modified: Option<SystemTime>,
    permissions: Permissions,
}

impl TextEditSession {
    /// Opens one complete UTF-8 regular file and captures its current identity
    /// and content generation. The caller must still decide whether the
    /// preview format itself is editable.
    pub fn open(path: &Path) -> Result<Self, TextEditError> {
        if !text_editing_supported() {
            return Err(TextEditError::NotAvailable);
        }
        Ok(Self {
            path: path.to_path_buf(),
            source: TextEditSnapshot::capture(path)?,
        })
    }

    pub fn contents(&self) -> &str {
        // `capture` validates UTF-8 before constructing a session.
        std::str::from_utf8(&self.source.bytes).expect("text-edit session stores valid UTF-8")
    }

    /// Atomically replaces the original file only when its identity and exact
    /// captured generation still match. Any pre-publication mismatch is a
    /// conflict and leaves the caller's buffer untouched.
    pub fn save(&self, edited_text: &str) -> Result<(), TextEditError> {
        self.save_with_publisher(edited_text, publish_atomically)
    }

    fn save_with_publisher<F>(&self, edited_text: &str, publish: F) -> Result<(), TextEditError>
    where
        F: FnOnce(&Path, &[u8], &Permissions) -> Result<(), TextEditError>,
    {
        let current = TextEditSnapshot::capture(&self.path).map_err(|_| TextEditError::Conflict)?;
        if !self.source.matches(&current) {
            return Err(TextEditError::Conflict);
        }
        if edited_text.as_bytes() == self.source.bytes {
            return Ok(());
        }
        publish(&self.path, edited_text.as_bytes(), &current.permissions)
    }
}

impl TextEditSnapshot {
    fn capture(path: &Path) -> Result<Self, TextEditError> {
        let link_metadata = std::fs::symlink_metadata(path).map_err(read_error)?;
        if link_metadata.file_type().is_symlink() {
            return Err(TextEditError::Symlink);
        }
        if !link_metadata.is_file() {
            return Err(TextEditError::NotRegularFile);
        }

        let mut file = File::open(path).map_err(read_error)?;
        let identity =
            Handle::from_file(file.try_clone().map_err(read_error)?).map_err(read_error)?;
        let path_identity = Handle::from_path(path).map_err(read_error)?;
        if identity != path_identity {
            return Err(TextEditError::Conflict);
        }

        let initial_metadata = file.metadata().map_err(read_error)?;
        if !initial_metadata.is_file() {
            return Err(TextEditError::NotRegularFile);
        }
        reject_hard_linked_file(&initial_metadata)?;
        let initial_size = initial_metadata.len();
        if initial_size > MAX_EDITABLE_TEXT_BYTES {
            return Err(TextEditError::TooLarge {
                limit: MAX_EDITABLE_TEXT_BYTES,
            });
        }
        let initial_modified = initial_metadata.modified().ok();

        let mut bytes = Vec::with_capacity(initial_size as usize);
        Read::by_ref(&mut file)
            .take(MAX_EDITABLE_TEXT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        if bytes.len() as u64 > MAX_EDITABLE_TEXT_BYTES {
            return Err(TextEditError::TooLarge {
                limit: MAX_EDITABLE_TEXT_BYTES,
            });
        }
        let final_metadata = file.metadata().map_err(read_error)?;
        if final_metadata.len() != initial_size
            || final_metadata.modified().ok() != initial_modified
        {
            return Err(TextEditError::Conflict);
        }
        let final_link_metadata = std::fs::symlink_metadata(path).map_err(read_error)?;
        if final_link_metadata.file_type().is_symlink() {
            return Err(TextEditError::Conflict);
        }
        let final_path_identity = Handle::from_path(path).map_err(read_error)?;
        if final_path_identity != identity {
            return Err(TextEditError::Conflict);
        }
        std::str::from_utf8(&bytes).map_err(|_| TextEditError::InvalidUtf8)?;

        Ok(Self {
            identity,
            digest: Sha256::digest(&bytes).into(),
            bytes,
            modified: initial_modified,
            permissions: final_metadata.permissions(),
        })
    }

    fn matches(&self, current: &Self) -> bool {
        self.identity == current.identity
            && self.bytes.len() == current.bytes.len()
            && self.digest == current.digest
            && self.modified == current.modified
    }
}

#[cfg(target_family = "unix")]
fn reject_hard_linked_file(metadata: &std::fs::Metadata) -> Result<(), TextEditError> {
    use std::os::unix::fs::MetadataExt;

    (metadata.nlink() == 1)
        .then_some(())
        .ok_or(TextEditError::HardLinked)
}

#[cfg(not(target_family = "unix"))]
fn reject_hard_linked_file(_metadata: &std::fs::Metadata) -> Result<(), TextEditError> {
    Ok(())
}

fn publish_atomically(
    path: &Path,
    contents: &[u8],
    permissions: &Permissions,
) -> Result<(), TextEditError> {
    let parent = path.parent().ok_or_else(|| {
        TextEditError::Write("the text file does not have a parent directory".to_string())
    })?;
    let temp_path = create_private_temp_file(parent, contents, permissions)?;
    let publish_result = std::fs::rename(&temp_path, path).map_err(write_error);
    if publish_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    publish_result
}

fn create_private_temp_file(
    parent: &Path,
    contents: &[u8],
    permissions: &Permissions,
) -> Result<PathBuf, TextEditError> {
    for _ in 0..32 {
        let path = parent.join(format!(".ottrin-text-edit-{}", random_suffix()?));
        let file = open_private_temp_file(&path);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(write_error(error)),
        };
        let write_result = (|| {
            file.write_all(contents).map_err(write_error)?;
            file.set_permissions(permissions.clone())
                .map_err(write_error)?;
            file.sync_all().map_err(write_error)
        })();
        if let Err(error) = write_result {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        return Ok(path);
    }
    Err(TextEditError::Write(
        "could not reserve a private temporary file".to_string(),
    ))
}

#[cfg(target_family = "unix")]
fn open_private_temp_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(target_family = "unix"))]
fn open_private_temp_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn random_suffix() -> Result<String, TextEditError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| TextEditError::Write(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn read_error(error: std::io::Error) -> TextEditError {
    TextEditError::Read(error.to_string())
}

fn write_error(error: std::io::Error) -> TextEditError {
    TextEditError::Write(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("ottrin-text-edit-{}-{id}", std::process::id()));
            std::fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("write fixture");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unicode_text_can_be_saved_without_normalizing_line_endings() {
        let root = TestDir::new();
        let path = root.file("notes.txt", "Gr\u{00f6}na\r\nrad\r\n");

        let session = TextEditSession::open(&path).expect("open editable text");
        session
            .save("Gr\u{00f6}na\r\nbl\u{00e5}\r\n")
            .expect("save unicode text");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read saved text"),
            "Gr\u{00f6}na\r\nbl\u{00e5}\r\n"
        );
    }

    #[test]
    fn canceling_a_session_never_mutates_the_source() {
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");

        let session = TextEditSession::open(&path).expect("open editable text");
        assert_eq!(session.contents(), "original\n");
        drop(session);

        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    }

    #[test]
    fn changed_source_refuses_overwrite_and_keeps_the_callers_buffer() {
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        let unsaved_buffer = "my unsaved edit\n".to_string();
        std::fs::write(&path, "external update\n").expect("change source");

        assert_eq!(session.save(&unsaved_buffer), Err(TextEditError::Conflict));
        assert_eq!(unsaved_buffer, "my unsaved edit\n");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external update\n");
    }

    #[test]
    fn missing_source_is_a_conflict_not_a_new_file_save() {
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        std::fs::remove_file(&path).expect("remove source");

        assert_eq!(
            session.save("my unsaved edit\n"),
            Err(TextEditError::Conflict)
        );
        assert!(!path.exists());
    }

    #[test]
    fn failed_publication_keeps_the_source_and_callers_buffer() {
        let root = TestDir::new();
        let path = root.file("notes.txt", "original\n");
        let session = TextEditSession::open(&path).expect("open editable text");
        let unsaved_buffer = "my unsaved edit\n".to_string();

        let result = session.save_with_publisher(&unsaved_buffer, |_, _, _| {
            Err(TextEditError::Write("forced test failure".to_string()))
        });

        assert!(matches!(result, Err(TextEditError::Write(_))));
        assert_eq!(unsaved_buffer, "my unsaved edit\n");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original\n");
    }

    #[test]
    fn invalid_or_incomplete_sources_remain_read_only() {
        let root = TestDir::new();
        let invalid = root.0.join("invalid.txt");
        std::fs::write(&invalid, [0xff, 0xfe]).expect("write invalid UTF-8");
        assert!(matches!(
            TextEditSession::open(&invalid),
            Err(TextEditError::InvalidUtf8)
        ));

        let oversized = root.0.join("large.txt");
        std::fs::write(&oversized, vec![b'x'; MAX_EDITABLE_TEXT_BYTES as usize + 1])
            .expect("write large text");
        assert!(matches!(
            TextEditSession::open(&oversized),
            Err(TextEditError::TooLarge {
                limit: MAX_EDITABLE_TEXT_BYTES
            })
        ));
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn symbolic_links_remain_read_only() {
        use std::os::unix::fs::symlink;

        let root = TestDir::new();
        let target = root.file("target.txt", "target\n");
        let link = root.0.join("link.txt");
        symlink(&target, &link).expect("create text link");

        assert!(matches!(
            TextEditSession::open(&link),
            Err(TextEditError::Symlink)
        ));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "target\n");
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn hard_linked_text_remains_read_only() {
        let root = TestDir::new();
        let target = root.file("target.txt", "target\n");
        let alias = root.0.join("alias.txt");
        std::fs::hard_link(&target, &alias).expect("create hard link");

        assert!(matches!(
            TextEditSession::open(&target),
            Err(TextEditError::HardLinked)
        ));
        assert_eq!(std::fs::read_to_string(alias).unwrap(), "target\n");
    }
}
