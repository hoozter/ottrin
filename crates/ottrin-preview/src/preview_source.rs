//! Regular-file admission shared by bounded PLY and Affinity snapshots.
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

#[derive(Debug)]
pub(super) enum AdmissionError {
    Metadata(std::io::Error),
    NotRegular,
    Changed,
}
pub(super) struct Source {
    pub(super) file: File,
    state: State,
}
impl Source {
    pub(super) fn open(path: &Path) -> Result<Self, AdmissionError> {
        let file = open(path)?;
        let state = state(&file)?;
        let source = Self { file, state };
        source.validate(path)?;
        Ok(source)
    }
    pub(super) fn len(&self) -> u64 {
        self.state.length
    }
    pub(super) fn validate(&self, path: &Path) -> Result<(), AdmissionError> {
        if state(&self.file)? != self.state
            || named_state(path).map_err(|_| AdmissionError::Changed)? != self.state
        {
            return Err(AdmissionError::Changed);
        }
        Ok(())
    }
}

#[cfg(unix)]
#[derive(PartialEq, Eq)]
struct State {
    device: u64,
    inode: u64,
    length: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
#[cfg(unix)]
fn unix_state(info: &std::fs::Metadata) -> Result<State, AdmissionError> {
    use std::os::unix::fs::MetadataExt;
    if !info.is_file() {
        return Err(AdmissionError::NotRegular);
    }
    Ok(State {
        device: info.dev(),
        inode: info.ino(),
        length: info.len(),
        modified: (info.mtime(), info.mtime_nsec()),
        changed: (info.ctime(), info.ctime_nsec()),
    })
}
#[cfg(unix)]
fn state(file: &File) -> Result<State, AdmissionError> {
    unix_state(&file.metadata().map_err(AdmissionError::Metadata)?)
}
#[cfg(unix)]
fn named_state(path: &Path) -> Result<State, AdmissionError> {
    unix_state(&std::fs::symlink_metadata(path).map_err(AdmissionError::Metadata)?)
}
#[cfg(unix)]
fn open(path: &Path) -> Result<File, AdmissionError> {
    use std::os::unix::fs::OpenOptionsExt;
    named_state(path)?;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(AdmissionError::Metadata)
}

#[cfg(windows)]
#[derive(PartialEq, Eq)]
struct State {
    volume: u64,
    id: [u8; 16],
    length: u64,
    created: i64,
    modified: i64,
    changed: i64,
    attributes: u32,
}
#[cfg(windows)]
fn state(file: &File) -> Result<State, AdmissionError> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    let handle = file.as_raw_handle().cast();
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Err(AdmissionError::NotRegular);
    }
    let mut identity = FILE_ID_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    for (class, buffer, length) in [
        (
            FileIdInfo,
            (&mut identity as *mut FILE_ID_INFO).cast(),
            size_of::<FILE_ID_INFO>(),
        ),
        (
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            size_of::<FILE_BASIC_INFO>(),
        ),
        (
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            size_of::<FILE_STANDARD_INFO>(),
        ),
    ] {
        if unsafe { GetFileInformationByHandleEx(handle, class, buffer, length as u32) } == 0 {
            return Err(AdmissionError::Metadata(std::io::Error::last_os_error()));
        }
    }
    if standard.Directory
        || standard.DeletePending
        || standard.EndOfFile < 0
        || basic.FileAttributes
            & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_DEVICE)
            != 0
    {
        return Err(AdmissionError::NotRegular);
    }
    Ok(State {
        volume: identity.VolumeSerialNumber,
        id: identity.FileId.Identifier,
        length: standard.EndOfFile as u64,
        created: basic.CreationTime,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
        attributes: basic.FileAttributes,
    })
}
#[cfg(windows)]
pub(super) fn filesystem_path(path: &Path) -> Result<(), AdmissionError> {
    use std::{
        os::windows::ffi::OsStrExt,
        path::{Component, Prefix},
    };
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(_)
                | Prefix::VerbatimDisk(_)
                | Prefix::UNC(_, _)
                | Prefix::VerbatimUNC(_, _) => {}
                _ => return Err(AdmissionError::NotRegular),
            },
            Component::Normal(name) => {
                let wide = name.encode_wide().collect::<Vec<_>>();
                if wide.contains(&58) || wide.contains(&0) {
                    return Err(AdmissionError::NotRegular);
                }
                let base = wide.split(|c| *c == 46).next().unwrap_or(&[]);
                let base = String::from_utf16_lossy(base)
                    .trim_end_matches([' ', '.'])
                    .to_ascii_uppercase();
                if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
                    || ["COM", "LPT"].iter().any(|prefix| {
                        base.strip_prefix(prefix).is_some_and(|tail| {
                            matches!(
                                tail,
                                "1" | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                                    | "¹"
                                    | "²"
                                    | "³"
                            )
                        })
                    })
                {
                    return Err(AdmissionError::NotRegular);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
#[cfg(windows)]
fn open(path: &Path) -> Result<File, AdmissionError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::*;
    filesystem_path(path)?;
    let metadata = std::fs::symlink_metadata(path).map_err(AdmissionError::Metadata)?;
    if !metadata.is_file()
        || metadata.file_attributes() & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) != 0
    {
        return Err(AdmissionError::NotRegular);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(AdmissionError::Metadata)?;
    state(&file)?;
    Ok(file)
}
#[cfg(windows)]
fn named_state(path: &Path) -> Result<State, AdmissionError> {
    state(&open(path)?)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        io::Read,
        os::windows::{
            ffi::OsStringExt,
            fs::{OpenOptionsExt, symlink_dir, symlink_file},
        },
    };
    use windows_sys::Win32::Storage::FileSystem::*;
    #[test]
    fn native_windows_regular_source_preserves_raw_unicode_names_and_bytes() {
        let root = tempfile::tempdir().unwrap();
        let mut name = "preview-å-".encode_utf16().collect::<Vec<_>>();
        name.push(0xd800);
        name.extend(".ply".encode_utf16());
        let path = root.path().join(std::ffi::OsString::from_wide(&name));
        std::fs::write(&path, b"bounded source").unwrap();
        let mut source = Source::open(&path).unwrap();
        let mut bytes = Vec::new();
        source.file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"bounded source");
        source.validate(&path).unwrap();
    }
    #[test]
    fn native_windows_admission_rejects_devices_pipes_and_named_stream_syntax() {
        for path in [
            r"NUL.ply",
            r"CON.afphoto",
            r"COM1.ply",
            r"LPT².afphoto",
            r"C:\temp\NUL.ply",
            r"\\.\pipe\ottrin-preview.ply",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\file.ply",
            r"file.ply:named.ply",
        ] {
            assert!(
                matches!(
                    Source::open(Path::new(path)),
                    Err(AdmissionError::NotRegular)
                ),
                "{path}"
            );
        }
    }
    #[test]
    fn native_windows_admission_rejects_directories_and_reparse_sources() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ordinary.ply");
        std::fs::write(&path, b"unchanged").unwrap();
        let link = root.path().join("link.ply");
        symlink_file(&path, &link).unwrap();
        for path in [root.path(), link.as_path()] {
            assert!(matches!(
                Source::open(path),
                Err(AdmissionError::NotRegular)
            ));
        }
        assert_eq!(std::fs::read(path).unwrap(), b"unchanged");
    }
    #[test]
    fn native_windows_admission_excludes_existing_and_new_writers() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.ply");
        std::fs::write(&path, b"unchanged").unwrap();
        let writer = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        assert!(Source::open(&path).is_err());
        drop(writer);
        let source = Source::open(&path).unwrap();
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::rename(&path, root.path().join("renamed.ply")).is_err());
        source.validate(&path).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"unchanged");
    }
    #[test]
    fn native_windows_admission_detects_retargeted_ancestor_binding() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a");
        let b = root.path().join("b");
        for path in [&a, &b] {
            std::fs::create_dir(path).unwrap();
            std::fs::write(path.join("source.ply"), b"same bytes distinct identity").unwrap();
        }
        let alias = root.path().join("alias");
        symlink_dir(&a, &alias).unwrap();
        let source = Source::open(&alias.join("source.ply")).unwrap();
        std::fs::remove_dir(&alias).unwrap();
        symlink_dir(&b, &alias).unwrap();
        assert!(matches!(
            source.validate(&alias.join("source.ply")),
            Err(AdmissionError::Changed)
        ));
    }
    #[test]
    fn native_windows_ply_admission_retains_size_and_parser_limits() {
        use crate::ply_visual::*;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("ordinary.ply");
        std::fs::write(&path,b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 1\n3 0 1 2\n").unwrap();
        assert!(ply_preview_source_generation(&path).is_some());
        assert!(
            render_ply_preview_png(&path, 128)
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        let oversized = root.path().join("oversized.ply");
        let file = File::create(&oversized).unwrap();
        file.set_len(MAX_PLY_PREVIEW_BYTES + 1).unwrap();
        drop(file);
        assert!(matches!(
            render_ply_preview_png(&oversized, 128),
            Err(PlyVisualError::InputTooLarge { .. })
        ));
        std::fs::write(&path, b"ply\nformat ascii 1.0\n").unwrap();
        assert!(render_ply_preview_png(&path, 128).is_err());
    }
}
