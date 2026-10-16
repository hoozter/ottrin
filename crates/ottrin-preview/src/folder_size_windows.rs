//! Bounded, read-only enumeration beneath opened Windows directories.
//! Entry lengths are observations, not a filesystem snapshot. Never follow
//! reparse points or reopen descendants through concatenated absolute paths.
use super::{FolderRecursiveSize, MAX_FOLDER_RECURSIVE_DEPTH};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    mem::{offset_of, size_of, size_of_val},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::{
    Wdk::{Foundation::OBJECT_ATTRIBUTES, Storage::FileSystem::*},
    Win32::{
        Foundation::{ERROR_NO_MORE_FILES, RtlNtStatusToDosError, UNICODE_STRING},
        Storage::FileSystem::*,
        System::IO::IO_STATUS_BLOCK,
    },
};

const ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
const SHARING: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

fn check_directory(file: &File) -> io::Result<()> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    if unsafe { GetFileType(file.as_raw_handle().cast()) } != FILE_TYPE_DISK {
        return Err(io::Error::other("not a disk directory"));
    }
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        || info.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) != 0
    {
        return Err(io::Error::other("not an ordinary directory"));
    }
    Ok(())
}

fn open_child(parent: &File, name: &OsStr) -> io::Result<File> {
    let mut wide = name.encode_wide().collect::<Vec<_>>();
    // A native name must remain exactly one leaf, including ill-formed UTF-16.
    if wide.is_empty()
        || wide.len() > 255
        || wide.iter().any(|c| [0, 58, 47, 92].contains(c))
        || name == "."
        || name == ".."
    {
        return Err(io::Error::other("invalid directory child name"));
    }
    let unicode = UNICODE_STRING {
        Length: (wide.len() * 2) as u16,
        MaximumLength: (wide.len() * 2) as u16,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &unicode,
        ..Default::default()
    };
    let mut handle = ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            ACCESS,
            &attributes,
            &mut status,
            ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            SHARING,
            FILE_OPEN,
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    if result < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(result) } as i32,
        ));
    }
    let file = unsafe { File::from_raw_handle(handle.cast()) };
    check_directory(&file)?;
    Ok(file)
}

pub(super) fn summarize(
    path: &Path,
    entry_limit: usize,
    mut before_directory_open: impl FnMut(&OsStr),
) -> FolderRecursiveSize {
    let mut size = FolderRecursiveSize {
        bytes: 0,
        incomplete: false,
    };
    let root = (|| {
        super::preview_source::filesystem_path(path)
            .map_err(|_| io::Error::other("not a filesystem directory path"))?;
        let file = OpenOptions::new()
            .access_mode(ACCESS)
            .share_mode(SHARING)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        check_directory(&file)?;
        Ok::<_, io::Error>(file)
    })();
    let Ok(root) = root else {
        size.incomplete = true;
        return size;
    };
    let mut remaining = entry_limit;
    if scan(
        &root,
        0,
        &mut remaining,
        &mut size,
        &mut before_directory_open,
    )
    .is_err()
    {
        size.incomplete = true;
    }
    size
}

fn scan(
    directory: &File,
    depth: usize,
    remaining: &mut usize,
    size: &mut FolderRecursiveSize,
    before_directory_open: &mut impl FnMut(&OsStr),
) -> io::Result<()> {
    // One fixed, aligned buffer per active depth; no unbounded name/handle list.
    let mut buffer = vec![0u64; 8192];
    let mut class = FileFullDirectoryRestartInfo;
    loop {
        buffer.fill(0);
        if unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle().cast(),
                class,
                buffer.as_mut_ptr().cast(),
                size_of_val(buffer.as_slice()) as u32,
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                Ok(())
            } else {
                Err(error)
            };
        }
        class = FileFullDirectoryInfo;
        // The API returns a linked series of FILE_FULL_DIR_INFO records. Decode
        // fields without creating references to variable-length native structs.
        let bytes = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size_of_val(buffer.as_slice()))
        };
        let mut offset = 0;
        loop {
            let record = entry(&bytes[offset..])?;
            if record.name != "." && record.name != ".." {
                if *remaining == 0 {
                    return Err(io::Error::other("folder entry limit reached"));
                }
                *remaining -= 1;
                if record.attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) == 0 {
                    if record.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                        if depth >= MAX_FOLDER_RECURSIVE_DEPTH {
                            size.incomplete = true;
                        } else {
                            before_directory_open(&record.name);
                            match open_child(directory, &record.name) {
                                Ok(child) => {
                                    if scan(
                                        &child,
                                        depth + 1,
                                        remaining,
                                        size,
                                        before_directory_open,
                                    )
                                    .is_err()
                                    {
                                        size.incomplete = true;
                                    }
                                }
                                Err(_) => size.incomplete = true,
                            }
                        }
                    } else {
                        size.bytes = size.bytes.saturating_add(record.length);
                    }
                }
            }
            if record.next == 0 {
                break;
            }
            offset += record.next;
        }
    }
}

struct Entry {
    name: OsString,
    attributes: u32,
    length: u64,
    next: usize,
}

fn entry(bytes: &[u8]) -> io::Result<Entry> {
    const NAME: usize = offset_of!(FILE_FULL_DIR_INFO, FileName);
    let invalid = || io::Error::other("invalid native directory record");
    if bytes.len() < NAME {
        return Err(invalid());
    }
    let field = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let next = field(0) as usize;
    let name_len = field(offset_of!(FILE_FULL_DIR_INFO, FileNameLength)) as usize;
    let end = NAME.checked_add(name_len).ok_or_else(invalid)?;
    if name_len == 0
        || !name_len.is_multiple_of(2)
        || name_len > 510
        || end > bytes.len()
        || (next != 0
            && (next < end || !next.is_multiple_of(8) || next > bytes.len().saturating_sub(NAME)))
    {
        return Err(invalid());
    }
    let wide = bytes[NAME..end]
        .chunks_exact(2)
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect::<Vec<_>>();
    if wide.iter().any(|c| [0, 58, 47, 92].contains(c)) {
        return Err(invalid());
    }
    let at = offset_of!(FILE_FULL_DIR_INFO, EndOfFile);
    let length = i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    if length < 0 {
        return Err(invalid());
    }
    Ok(Entry {
        name: OsString::from_wide(&wide),
        attributes: field(offset_of!(FILE_FULL_DIR_INFO, FileAttributes)),
        length: length as u64,
        next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::{symlink_dir, symlink_file};

    #[test]
    fn native_folder_size_skips_file_links_and_directory_junctions() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("own"), b"123").unwrap();
        std::fs::write(outside.join("large"), [7u8; 1024]).unwrap();
        symlink_file(outside.join("large"), root.join("file-link")).unwrap();
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(root.join("junction"))
            .arg(&outside)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        assert_eq!(
            summarize(&root, 100, |_| {}),
            FolderRecursiveSize {
                bytes: 3,
                incomplete: false
            }
        );
        assert_eq!(
            summarize(&root.join("junction"), 100, |_| {}),
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true
            }
        );
        // Remove the junction entry explicitly; never give recursive teardown its target.
        std::fs::remove_dir(root.join("junction")).unwrap();
    }

    #[test]
    fn native_folder_size_refuses_child_replaced_with_reparse_entry() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret"), [9u8; 1024]).unwrap();
        let size = summarize(&root, 100, |name| {
            if name == "nested" {
                std::fs::remove_dir(root.join(name)).unwrap();
                symlink_dir(&outside, root.join(name)).unwrap();
            }
        });
        assert_eq!(
            size,
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true
            }
        );
    }

    #[test]
    fn native_folder_size_keeps_parent_handle_when_path_is_redirected() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("root");
        let moved = fixture.path().join("moved");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(outside.join("nested")).unwrap();
        std::fs::write(root.join("nested/own"), b"12345").unwrap();
        std::fs::write(outside.join("nested/secret"), [9u8; 1024]).unwrap();
        let size = summarize(&root, 100, |name| {
            if name == "nested" {
                std::fs::rename(&root, &moved).unwrap();
                symlink_dir(&outside, &root).unwrap();
            }
        });
        assert_eq!(
            size,
            FolderRecursiveSize {
                bytes: 5,
                incomplete: false
            }
        );
    }

    #[test]
    fn native_folder_size_pages_entries_and_reports_limits_and_raw_names() {
        let fixture = tempfile::tempdir().unwrap();
        // More than one native 64 KiB page, including non-Unicode UTF-16.
        for i in 0..1000 {
            std::fs::write(fixture.path().join(format!("file-{i:04}")), b"abc").unwrap();
        }
        let raw_name = OsString::from_wide(&[0xd800, 0x0061]);
        std::fs::create_dir(fixture.path().join(&raw_name)).unwrap();
        std::fs::write(fixture.path().join(raw_name).join("child"), b"de").unwrap();
        assert_eq!(
            summarize(fixture.path(), 1100, |_| {}),
            FolderRecursiveSize {
                bytes: 3002,
                incomplete: false
            }
        );
        let limited = summarize(fixture.path(), 10, |_| {});
        assert!(limited.incomplete);
        assert!(limited.bytes <= 30);
        assert_eq!(
            summarize(fixture.path(), 0, |_| {}),
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true
            }
        );
    }

    #[test]
    fn native_folder_size_marks_depth_and_open_failures_incomplete() {
        let fixture = tempfile::tempdir().unwrap();
        let mut deepest = fixture.path().to_path_buf();
        for _ in 0..MAX_FOLDER_RECURSIVE_DEPTH + 1 {
            deepest.push("d");
            std::fs::create_dir(&deepest).unwrap();
        }
        std::fs::write(deepest.join("too-deep"), b"12345").unwrap();
        assert_eq!(
            summarize(fixture.path(), 1000, |_| {}),
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true
            }
        );
        assert_eq!(
            summarize(&fixture.path().join("missing"), 100, |_| {}),
            FolderRecursiveSize {
                bytes: 0,
                incomplete: true
            }
        );
    }

    #[test]
    fn native_folder_size_preserves_partial_count_when_child_listing_is_denied() {
        let fixture = tempfile::tempdir().unwrap();
        let child = fixture.path().join("private");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(fixture.path().join("visible"), b"abc").unwrap();
        std::fs::write(child.join("hidden"), b"do not count").unwrap();
        let user = std::env::var("USERNAME").unwrap();
        // Deny only directory listing, keeping the owner's DACL repair rights.
        // Restore even if an assertion unwinds, before TempDir tears down files.
        struct Denial<'a>(&'a Path, String);
        impl Drop for Denial<'_> {
            fn drop(&mut self) {
                let result = std::process::Command::new("icacls.exe")
                    .arg(self.0)
                    .arg("/remove:d")
                    .arg(&self.1)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "ACL restoration failed: {result:?}"
                );
            }
        }
        let result = std::process::Command::new("icacls.exe")
            .arg(&child)
            .arg("/deny")
            .arg(format!("{user}:(RD)"))
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
        let _denial = Denial(&child, user);
        assert_eq!(
            summarize(fixture.path(), 100, |_| {}),
            FolderRecursiveSize {
                bytes: 3,
                incomplete: true
            }
        );
    }

    #[test]
    fn native_folder_record_parser_rejects_invalid_offsets_and_names() {
        const NAME: usize = offset_of!(FILE_FULL_DIR_INFO, FileName);
        let mut bytes = vec![0u8; NAME + 16];
        let field = offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
        bytes[field..field + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[NAME..NAME + 2].copy_from_slice(&65u16.to_le_bytes());
        assert_eq!(entry(&bytes).unwrap().name, "A");
        for next in [1u32, 64, 80, u32::MAX] {
            bytes[..4].copy_from_slice(&next.to_le_bytes());
            assert!(entry(&bytes).is_err());
        }
        bytes[..4].fill(0);
        for len in [0u32, 1, 18, u32::MAX] {
            bytes[field..field + 4].copy_from_slice(&len.to_le_bytes());
            assert!(entry(&bytes).is_err());
        }
        bytes[field..field + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[NAME..NAME + 2].copy_from_slice(&92u16.to_le_bytes());
        assert!(entry(&bytes).is_err());
    }
}
