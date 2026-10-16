//! Known Windows link reparse records. Every read/write uses a no-follow handle.
use super::{PlatformError, io, metadata, stable_identity_from_handle};
use std::{fs::File, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{FSCTL_GET_REPARSE_POINT, FSCTL_SET_REPARSE_POINT};
use windows_sys::Win32::System::SystemServices::{
    IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
};

const MAX_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Link {
    pub(super) bytes: Vec<u8>,
    pub(super) directory: bool,
}

impl Link {
    pub(super) fn parse(bytes: Vec<u8>, directory: bool) -> Result<Self, PlatformError> {
        let invalid = || io("invalid or unsupported link reparse record");
        if bytes.len() < 8 || bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let tag = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let length = u16::from_le_bytes(bytes[4..6].try_into().unwrap()) as usize;
        if length + 8 != bytes.len() || bytes[6..8] != [0, 0] {
            return Err(invalid());
        }
        let path_start = match tag {
            IO_REPARSE_TAG_SYMLINK => 20,
            IO_REPARSE_TAG_MOUNT_POINT if directory => 16,
            _ => {
                return Err(PlatformError::UnsupportedSafeCopy(
                    "unimplemented reparse tag; link target was not followed".into(),
                ));
            }
        };
        if bytes.len() < path_start {
            return Err(invalid());
        }
        if tag == IO_REPARSE_TAG_SYMLINK
            && u32::from_le_bytes(bytes[16..20].try_into().unwrap()) & !1 != 0
        {
            return Err(invalid());
        }
        for at in [8, 12] {
            let offset = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
            let size = u16::from_le_bytes(bytes[at + 2..at + 4].try_into().unwrap()) as usize;
            if !offset.is_multiple_of(2) || !size.is_multiple_of(2) || (at == 8 && size == 0) {
                return Err(invalid());
            }
            let start = path_start.checked_add(offset).ok_or_else(invalid)?;
            let end = start.checked_add(size).ok_or_else(invalid)?;
            let path = bytes.get(start..end).ok_or_else(invalid)?;
            if path.chunks_exact(2).any(|unit| unit == [0, 0]) {
                return Err(invalid());
            }
        }
        Ok(Self { bytes, directory })
    }

    /// The enclosing stage owns this ordinary, primary-data-empty candidate.
    /// Preserve its existing named payload and DACL. Never adopt an existing
    /// reparse point, reopen a pathname, follow a target, enable a privilege or
    /// attempt destructive cleanup after failure.
    pub(super) fn install(&self, candidate: &File) -> Result<(), PlatformError> {
        let before = stable_identity_from_handle(candidate).map_err(io)?;
        let compatible = if self.directory {
            before.is_directory_without_reparse()
        } else {
            before.is_regular_file_without_reparse()
        };
        if !compatible || (!self.directory && candidate.metadata().map_err(io)?.len() != 0) {
            return Err(io("link candidate is occupied or has a different type"));
        }
        if self.directory {
            crate::windows_fs::directory_children_limited(candidate, 0).map_err(io)?;
        }
        let mut returned = 0;
        if unsafe {
            DeviceIoControl(
                candidate.as_raw_handle().cast(),
                FSCTL_SET_REPARSE_POINT,
                self.bytes.as_ptr().cast(),
                self.bytes.len() as u32,
                ptr::null_mut(),
                0,
                &mut returned,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io(std::io::Error::last_os_error()));
        }
        if !before.same_object(stable_identity_from_handle(candidate).map_err(io)?)
            || capture(candidate)?.as_ref() != Some(self)
        {
            return Err(io(
                "installed link identity or reparse bytes did not match; candidate retained",
            ));
        }
        candidate.sync_all().map_err(io)?;
        Ok(())
    }
}

pub(super) fn capture(file: &File) -> Result<Option<Link>, PlatformError> {
    let basic = metadata::basic(file)?;
    if basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(None);
    }
    let before = stable_identity_from_handle(file).map_err(io)?;
    let mut bytes = vec![0u8; MAX_BYTES];
    let mut returned = 0;
    if unsafe {
        DeviceIoControl(
            file.as_raw_handle().cast(),
            FSCTL_GET_REPARSE_POINT,
            ptr::null(),
            0,
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io(std::io::Error::last_os_error()));
    }
    if returned as usize > bytes.len() {
        return Err(io("invalid native link record size"));
    }
    bytes.truncate(returned as usize);
    let link = Link::parse(bytes, basic.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)?;
    if stable_identity_from_handle(file).map_err(io)? != before {
        return Err(io("link changed while capturing native reparse record"));
    }
    Ok(Some(link))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, path::Path};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };
    fn read(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }

    #[test]
    fn windows_native_copy_link_capture_and_install_preserves_relative_absolute_and_dangling_links()
    {
        use std::os::windows::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        std::fs::write(
            root.path().join("target-file"),
            b"target bytes must never change",
        )
        .unwrap();
        std::fs::create_dir(root.path().join("target-directory")).unwrap();
        std::fs::write(
            root.path().join("target-directory/keep"),
            b"directory target unchanged",
        )
        .unwrap();
        std::fs::write(root.path().join("réservé-🦀"), b"Unicode target unchanged").unwrap();
        for (i, directory, relative, target) in [
            (0, false, true, std::path::PathBuf::from("target-file")),
            (1, false, false, root.path().join("target-file")),
            (2, true, true, std::path::PathBuf::from("target-directory")),
            (3, true, false, root.path().join("target-directory")),
            (4, false, true, std::path::PathBuf::from("missing-file")),
            (5, true, true, std::path::PathBuf::from("missing-directory")),
            (6, false, true, std::path::PathBuf::from("réservé-🦀")),
            (
                7,
                false,
                true,
                std::path::PathBuf::from(std::ffi::OsString::from_wide(&[b'x' as u16, 0xd800])),
            ),
        ] {
            let source = root.path().join(format!("source-{i}"));
            if directory {
                std::os::windows::fs::symlink_dir(&target, &source).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&target, &source).unwrap();
            }
            let source = read(&source);
            let captured = capture(&source).unwrap().unwrap();
            assert_eq!(captured.directory, directory);
            assert_eq!(
                u32::from_le_bytes(captured.bytes[16..20].try_into().unwrap()) & 1 != 0,
                relative
            );
            let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(directory).unwrap();
            captured.install(&candidate).unwrap();
            assert_eq!(capture(&candidate).unwrap(), Some(captured));
            super::super::tests::assert_owner_only(&candidate);
        }
        assert_eq!(
            std::fs::read(root.path().join("target-file")).unwrap(),
            b"target bytes must never change"
        );
        assert_eq!(
            std::fs::read(root.path().join("target-directory/keep")).unwrap(),
            b"directory target unchanged"
        );
    }
    fn relative_record(target: &[u16]) -> Link {
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&IO_REPARSE_TAG_SYMLINK.to_le_bytes());
        let size = (target.len() * 2) as u16;
        bytes[10..12].copy_from_slice(&size.to_le_bytes());
        bytes[14..16].copy_from_slice(&size.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        for unit in target {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let length = (bytes.len() - 8) as u16;
        bytes[4..6].copy_from_slice(&length.to_le_bytes());
        Link::parse(bytes, false).unwrap()
    }
    fn named_payload(file: &File, name: &str, data: &[u8]) {
        let units = name.encode_utf16().collect::<Vec<_>>();
        let mut header = [0u8; super::super::HEADER_BYTES];
        header[..4].copy_from_slice(&super::super::ALTERNATE_DATA.to_le_bytes());
        header[8..16].copy_from_slice(&(data.len() as u64).to_le_bytes());
        header[16..20].copy_from_slice(&((units.len() * 2) as u32).to_le_bytes());
        let mut writer = super::super::Backup::new(file, super::super::Direction::Write).unwrap();
        writer.write_all(&header).unwrap();
        for unit in units {
            writer.write_all(&unit.to_le_bytes()).unwrap();
        }
        writer.write_all(data).unwrap();
        writer.finish().unwrap();
    }

    #[test]
    fn windows_native_copy_link_staging_preserves_its_own_unicode_named_streams() {
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        let source_stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
        let source = source_stage.create_candidate(false).unwrap();
        let bytes = vec![73u8; super::super::BUFFER_BYTES + 37];
        named_payload(&source, ":réservé-🦀:$DATA", &bytes);
        named_payload(&source, ":empty:$DATA", &[]);
        let before = super::super::transfer(&source, None, &mut |_| Ok(())).unwrap();
        assert_eq!(
            before
                .iter()
                .filter(|stream| stream.kind == super::super::ALTERNATE_DATA)
                .count(),
            2
        );
        let link = relative_record(&"missing-target".encode_utf16().collect::<Vec<_>>());
        link.install(&source).unwrap();
        let own = super::super::transfer(&source, None, &mut |_| Ok(())).unwrap();
        for original in &before {
            assert!(
                own.contains(original),
                "setting a link must preserve its own named payload"
            );
        }
        let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(false).unwrap();
        let staged = super::super::tree::stage_tree(&source, &candidate, 1, |_| Ok(())).unwrap();
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        assert_eq!(
            super::super::transfer(&candidate, None, &mut |_| Ok(())).unwrap(),
            own
        );
        assert_eq!(capture(&candidate).unwrap().as_ref(), Some(&link));
    }

    fn set_fixture(file: &File, bytes: &[u8]) {
        let mut returned = 0;
        assert_ne!(
            unsafe {
                DeviceIoControl(
                    file.as_raw_handle().cast(),
                    FSCTL_SET_REPARSE_POINT,
                    bytes.as_ptr().cast(),
                    bytes.len() as u32,
                    ptr::null_mut(),
                    0,
                    &mut returned,
                    ptr::null_mut(),
                )
            },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
    }

    #[test]
    fn link_record_validation_preserves_raw_utf16_and_rejects_malformed_or_unknown_data() {
        let record = relative_record(&[
            b'.' as u16,
            b'.' as u16,
            b'\\' as u16,
            0xd800,
            0xd83e,
            0xdd80,
        ]);
        assert_eq!(Link::parse(record.bytes.clone(), false).unwrap(), record);
        for (at, value) in [
            (4, 0u8),
            (6, 1),
            (8, 1),
            (10, 0),
            (12, 255),
            (16, 2),
            (20, 0),
        ] {
            let mut bytes = relative_record(&[b'a' as u16]).bytes;
            bytes[at] = value;
            assert!(Link::parse(bytes, false).is_err(), "field {at}");
        }
        for length in 0..20 {
            assert!(Link::parse(record.bytes[..length].to_vec(), false).is_err());
        }
        let mut bytes = record.bytes.clone();
        bytes[..4].copy_from_slice(&0x80000042u32.to_le_bytes());
        assert!(matches!(
            Link::parse(bytes, false),
            Err(PlatformError::UnsupportedSafeCopy(_))
        ));
        let mut bytes = record.bytes;
        bytes[..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        assert!(
            Link::parse(bytes, false).is_err(),
            "mount point must be a directory"
        );
    }

    #[test]
    fn windows_native_copy_junction_record_preserves_target_and_private_security() {
        use std::os::windows::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep"), b"junction target unchanged").unwrap();
        let mut nt = std::ffi::OsString::from(r"\??\");
        nt.push(target.as_os_str());
        let substitute = nt.encode_wide().collect::<Vec<_>>();
        let print = target.as_os_str().encode_wide().collect::<Vec<_>>();
        let mut bytes = vec![0u8; 16];
        bytes[..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        bytes[10..12].copy_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        bytes[12..14].copy_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        bytes[14..16].copy_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        for unit in substitute
            .into_iter()
            .chain(Some(0))
            .chain(print)
            .chain(Some(0))
        {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let size = (bytes.len() - 8) as u16;
        bytes[4..6].copy_from_slice(&size.to_le_bytes());
        let parent = super::super::tests::parent(root.path());
        let source_stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
        let source = source_stage.create_candidate(true).unwrap();
        set_fixture(&source, &bytes);
        assert_eq!(
            std::fs::read(crate::windows_fs::location(&source).unwrap().join("keep")).unwrap(),
            b"junction target unchanged"
        );
        let captured = capture(&source).unwrap().unwrap();
        assert_eq!(captured.bytes, bytes);
        let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let staged = super::super::tree::stage_tree(&source, &candidate, 1, |_| Ok(())).unwrap();
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        assert_eq!(capture(&candidate).unwrap(), Some(captured));
        stage.validate_candidate(&candidate, true).unwrap();
        super::super::tests::assert_owner_only(&candidate);
        assert_eq!(
            std::fs::read(target.join("keep")).unwrap(),
            b"junction target unchanged"
        );
    }

    #[test]
    fn windows_native_copy_link_root_staging_does_not_read_unreadable_targets() {
        use super::super::tests::{ReadOnlyAcl, set_dacl};
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        for directory in [false, true] {
            let target = root.path().join(if directory {
                "directory-target"
            } else {
                "file-target"
            });
            let data = if directory {
                std::fs::create_dir(&target).unwrap();
                target.join("keep")
            } else {
                target.clone()
            };
            std::fs::write(&data, b"target unchanged").unwrap();
            let source = root.path().join(if directory {
                "directory-link"
            } else {
                "file-link"
            });
            if directory {
                std::os::windows::fs::symlink_dir(&target, &source).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&target, &source).unwrap();
            }
            let mut paths = vec![target.clone()];
            if directory {
                paths.push(data.clone());
            }
            let guard = ReadOnlyAcl(paths);
            for path in &guard.0 {
                // Keep descriptor access for fixture restoration; deny file data,
                // read attributes, extended attributes and directory listing.
                set_dacl(path, "D:P(A;OICI;RCWD;;;OW)");
            }
            assert!(
                OpenOptions::new()
                    .read(true)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                    .open(&target)
                    .is_err()
            );
            assert!(std::fs::read(&data).is_err());
            let source = read(&source);
            let expected = capture(&source).unwrap();
            let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(directory).unwrap();
            let staged =
                super::super::tree::stage_tree(&source, &candidate, 1, |_| Ok(())).unwrap();
            assert_eq!(staged.entry_count(), 1);
            staged.verify_and_finalize(|_| Ok(())).unwrap();
            stage.validate_candidate(&candidate, directory).unwrap();
            assert_eq!(capture(&candidate).unwrap(), expected);
            super::super::tests::assert_owner_only(&candidate);
            drop(staged);
            drop(source);
            drop(guard);
            assert_eq!(std::fs::read(data).unwrap(), b"target unchanged");
        }
    }

    #[test]
    fn windows_native_copy_link_tampering_retains_staged_and_changed_records() {
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        let original = relative_record(&"original-target".encode_utf16().collect::<Vec<_>>());
        let modified = relative_record(&"modified-target".encode_utf16().collect::<Vec<_>>());
        for change_source in [true, false] {
            let source_stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
            let source = source_stage.create_candidate(false).unwrap();
            original.install(&source).unwrap();
            let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(false).unwrap();
            let staged =
                super::super::tree::stage_tree(&source, &candidate, 1, |_| Ok(())).unwrap();
            let changed = if change_source { &source } else { &candidate };
            let retained = if change_source { &candidate } else { &source };
            let identity = stable_identity_from_handle(changed).unwrap();
            set_fixture(changed, &modified.bytes);
            assert_eq!(stable_identity_from_handle(changed).unwrap(), identity);
            assert!(staged.verify_and_finalize(|_| Ok(())).is_err());
            assert_eq!(capture(changed).unwrap().as_ref(), Some(&modified));
            assert_eq!(capture(retained).unwrap().as_ref(), Some(&original));
            assert!(
                original.install(changed).is_err(),
                "never retarget an existing link"
            );
            assert_eq!(capture(changed).unwrap().as_ref(), Some(&modified));
        }
    }

    #[test]
    fn windows_native_copy_link_install_refuses_type_mismatch_and_occupied_data() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        let link = relative_record(&"missing".encode_utf16().collect::<Vec<_>>());
        for directory in [false, true] {
            let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(directory).unwrap();
            if !directory {
                (&candidate).write_all(b"unrelated candidate data").unwrap();
            }
            assert!(link.install(&candidate).is_err());
            assert!(capture(&candidate).unwrap().is_none());
            if !directory {
                assert_eq!(
                    std::fs::read(crate::windows_fs::location(&candidate).unwrap()).unwrap(),
                    b"unrelated candidate data"
                );
            }
        }
    }
    #[test]
    #[ignore = "requires Developer Mode disabled and no symlink privilege; selected explicitly in the ordinary-user policy lane"]
    fn windows_native_copy_symlink_policy_denial_retains_private_candidate() {
        let root = tempfile::tempdir().unwrap();
        let parent = super::super::tests::parent(root.path());
        std::fs::write(root.path().join("target"), b"target retained").unwrap();
        let link = relative_record(&"target".encode_utf16().collect::<Vec<_>>());
        let stage = super::super::namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(false).unwrap();
        named_payload(&candidate, ":retained:$DATA", b"unpublished named payload");
        let before = super::super::transfer(&candidate, None, &mut |_| Ok(())).unwrap();
        let error = link.install(&candidate).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&windows_sys::Win32::Foundation::ERROR_PRIVILEGE_NOT_HELD.to_string()),
            "{error}"
        );
        assert!(capture(&candidate).unwrap().is_none());
        assert_eq!(
            super::super::transfer(&candidate, None, &mut |_| Ok(())).unwrap(),
            before
        );
        super::super::tests::assert_owner_only(&candidate);
        stage.validate_candidate(&candidate, false).unwrap();
        assert_eq!(
            std::fs::read(root.path().join("target")).unwrap(),
            b"target retained"
        );
    }
}
