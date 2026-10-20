//! Sparse stream evidence independent of allocation and backup block boundaries.
//! Zero chunks are implicit; work and memory scale with allocated data, not holes.
use super::*;
#[cfg(test)]
use std::io::{Read, Write};

pub(super) const ATTRIBUTE: u32 = 8;
pub(super) const BLOCK: u32 = 9;
const CHUNK_BYTES: usize = 64 * 1024;

pub(super) struct Stream {
    kind: u32,
    name: Vec<u16>,
    hash: Sha256,
    chunk: Vec<u8>,
    chunk_index: Option<u64>,
    position: u64,
    end: u64,
    eof: Option<u64>,
}
impl Stream {
    pub(super) fn new(kind: u32, name: Vec<u16>) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"ottrin-windows-sparse-v1");
        Self {
            kind,
            name,
            hash,
            chunk: vec![0; CHUNK_BYTES],
            chunk_index: None,
            position: 0,
            end: 0,
            eof: None,
        }
    }
    pub(super) fn block(&mut self, offset: u64, bytes: u64) -> Result<(), PlatformError> {
        if self.eof.is_some() || self.position != self.end || offset < self.end {
            return Err(io("overlapping, incomplete or post-EOF sparse block"));
        }
        let end = offset
            .checked_add(bytes)
            .filter(|end| *end <= i64::MAX as u64)
            .ok_or_else(|| io("sparse block exceeds the native file range"))?;
        self.position = offset;
        self.end = end;
        if bytes == 0 {
            self.eof = Some(offset);
        }
        Ok(())
    }
    fn flush_chunk(&mut self) {
        if self.chunk.iter().any(|byte| *byte != 0) {
            self.hash.update(self.chunk_index.unwrap().to_le_bytes());
            self.hash.update(&self.chunk);
        }
        self.chunk.fill(0);
    }
    pub(super) fn data(&mut self, mut bytes: &[u8]) -> Result<(), PlatformError> {
        if self.eof.is_some() || bytes.len() as u64 > self.end - self.position {
            return Err(io("sparse data exceeds its declared block"));
        }
        while !bytes.is_empty() {
            let index = self.position / CHUNK_BYTES as u64;
            if self.chunk_index != Some(index) {
                self.flush_chunk();
                self.chunk_index = Some(index);
            }
            let offset = (self.position % CHUNK_BYTES as u64) as usize;
            let count = bytes.len().min(CHUNK_BYTES - offset);
            self.chunk[offset..offset + count].copy_from_slice(&bytes[..count]);
            self.position += count as u64;
            bytes = &bytes[count..];
        }
        Ok(())
    }
    pub(super) fn finish(mut self) -> Result<StreamEvidence, PlatformError> {
        if self.position != self.end || (self.eof.is_none() && self.end != 0) {
            return Err(io("sparse stream has no complete native EOF record"));
        }
        self.flush_chunk();
        let bytes = self.eof.unwrap_or(0);
        self.hash.update(bytes.to_le_bytes());
        Ok(StreamEvidence {
            kind: self.kind,
            attributes: ATTRIBUTE,
            name: self.name,
            bytes,
            sha256: format!("{:x}", self.hash.finalize()),
        })
    }
}

#[cfg(test)]
pub(super) fn fixture(file: &File, length: u64, offset: Option<u64>) {
    use windows_sys::Win32::System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_SPARSE};
    let mut returned = 0;
    assert_ne!(
        unsafe {
            DeviceIoControl(
                file.as_raw_handle().cast(),
                FSCTL_SET_SPARSE,
                ptr::null(),
                0,
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
    file.set_len(length).unwrap();
    if let Some(offset) = offset {
        let mut file = file;
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(b"allocated sparse data").unwrap();
    }
    file.sync_all().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_native_copy_sparse_fingerprint_ignores_allocation_and_buffer_boundaries() {
        let mut compact = Stream::new(DATA, vec![]);
        compact.block(65530, 16).unwrap();
        compact.data(b"sparse payload!!").unwrap();
        compact.block(1 << 40, 0).unwrap();
        let mut padded = Stream::new(DATA, vec![]);
        padded.block(0, 131072).unwrap();
        let mut bytes = vec![0; 131072];
        bytes[65530..65546].copy_from_slice(b"sparse payload!!");
        for part in bytes.chunks(313) {
            padded.data(part).unwrap();
        }
        padded.block(1 << 40, 0).unwrap();
        assert_eq!(compact.finish().unwrap(), padded.finish().unwrap());
    }
    #[test]
    fn windows_native_copy_sparse_fingerprint_rejects_malformed_ranges_and_binds_eof() {
        let mut stream = Stream::new(DATA, vec![]);
        stream.block(20, 4).unwrap();
        assert!(stream.block(24, 0).is_err());
        stream.data(b"data").unwrap();
        assert!(stream.block(23, 1).is_err());
        assert!(stream.data(b"extra").is_err());
        assert!(stream.block(i64::MAX as u64, 1).is_err());
        stream.block(100, 0).unwrap();
        assert!(stream.block(100, 0).is_err());
        assert!(stream.data(b"extra").is_err());
        let evidence = stream.finish().unwrap();
        let mut other = Stream::new(DATA, vec![]);
        other.block(20, 4).unwrap();
        other.data(b"data").unwrap();
        other.block(101, 0).unwrap();
        assert_ne!(evidence, other.finish().unwrap());
        let mut unfinished = Stream::new(DATA, vec![]);
        unfinished.block(0, 1).unwrap();
        unfinished.data(b"x").unwrap();
        assert!(unfinished.finish().is_err());
    }
}

#[cfg(test)]
mod native {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::*;
    fn rw(path: &std::path::Path) -> File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn allocation(file: &File) -> i64 {
        let mut info = FILE_STANDARD_INFO::default();
        assert_ne!(
            unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle().cast(),
                    FileStandardInfo,
                    ptr::from_mut(&mut info).cast(),
                    size_of::<FILE_STANDARD_INFO>() as u32,
                )
            },
            0
        );
        info.AllocationSize
    }
    #[test]
    fn windows_native_copy_sparse_payload_preserves_files_directories_named_streams_and_large_holes()
     {
        for (directory, primary_sparse, length, offset) in [
            (false, true, 16 * 1024 * 1024, Some(65536)),
            (false, true, 1 << 40, Some(65536)),
            (false, true, 8 * 1024 * 1024, None),
            (false, true, 0, None),
            (false, false, 0, None),
            (true, false, 0, None),
        ] {
            let root = tempfile::tempdir().unwrap();
            let src = root.path().join("source");
            let dst = root.path().join("candidate");
            if directory {
                std::fs::create_dir(&src).unwrap();
                std::fs::create_dir(&dst).unwrap();
            } else {
                std::fs::write(&src, b"").unwrap();
                std::fs::write(&dst, b"").unwrap();
            }
            if primary_sparse {
                fixture(&rw(&src), length, offset);
            }
            let named_sparse = !(primary_sparse && length == 0);
            if named_sparse {
                let named = format!("{}:thin", src.display());
                std::fs::write(&named, b"").unwrap();
                fixture(
                    &rw(std::path::Path::new(&named)),
                    4 * 1024 * 1024,
                    Some(131072),
                );
                let empty = format!("{}:zero", src.display());
                std::fs::write(&empty, b"").unwrap();
                fixture(&rw(std::path::Path::new(&empty)), 0, None);
            }
            let source = rw(&src);
            let target = rw(&dst);
            if primary_sparse && length == 16 * 1024 * 1024 {
                use windows_sys::Win32::System::{
                    IO::DeviceIoControl, Ioctl::FSCTL_SET_COMPRESSION,
                };
                let format = COMPRESSION_FORMAT_LZNT1;
                let mut returned = 0;
                assert_ne!(
                    unsafe {
                        DeviceIoControl(
                            target.as_raw_handle().cast(),
                            FSCTL_SET_COMPRESSION,
                            ptr::from_ref(&format).cast(),
                            size_of::<u16>() as u32,
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
            let before = version(&source).unwrap();
            let mut physical = 0;
            let evidence = metadata::stage_entry_payload(&source, &target, |bytes| {
                physical = physical.max(bytes);
                Ok(())
            })
            .unwrap();
            evidence.validate_source(&source).unwrap();
            evidence.validate_candidate_metadata(&target).unwrap();
            assert_eq!(version(&source).unwrap(), before);
            assert_eq!(
                target.metadata().unwrap().len(),
                if directory { 0 } else { length }
            );
            assert_eq!(
                metadata::basic(&target).unwrap().FileAttributes,
                before.attributes
            );
            assert!(
                physical < 1024 * 1024,
                "holes caused linear I/O: {physical}"
            );
            if !directory {
                assert!(allocation(&target) <= 131072);
            }
            if named_sparse {
                let named = rw(std::path::Path::new(&format!("{}:thin", dst.display())));
                assert_eq!(named.metadata().unwrap().len(), 4 * 1024 * 1024);
                assert!(allocation(&named) <= 131072);
                let mut named = &named;
                named.seek(SeekFrom::Start(131072)).unwrap();
                let mut data = [0; 21];
                named.read_exact(&mut data).unwrap();
                assert_eq!(&data, b"allocated sparse data");
                assert_eq!(
                    std::fs::metadata(format!("{}:zero", dst.display()))
                        .unwrap()
                        .len(),
                    0
                );
            }
            println!(
                "sparse payload: directory={directory} primary_sparse={primary_sparse} logical={length} physical_progress={physical} attributes={}",
                before.attributes
            );
        }
    }
    #[test]
    fn windows_native_copy_sparse_payload_detects_same_size_changes_and_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dst = root.path().join("candidate");
        std::fs::write(&src, b"").unwrap();
        std::fs::write(&dst, b"").unwrap();
        fixture(&rw(&src), 16 * 1024 * 1024, Some(65536));
        let source = rw(&src);
        let target = rw(&dst);
        let before = version(&source).unwrap();
        assert!(
            metadata::stage_entry_payload(&source, &target, |bytes| if bytes > 0 {
                Err(io("cancel sparse copy"))
            } else {
                Ok(())
            })
            .is_err()
        );
        assert_eq!(version(&source).unwrap(), before);
        drop(target);
        std::fs::remove_file(&dst).unwrap();
        std::fs::write(&dst, b"").unwrap();
        let target = rw(&dst);
        let evidence = metadata::stage_entry_payload(&source, &target, |_| Ok(())).unwrap();
        let mut writer = &target;
        writer.seek(SeekFrom::Start(8 * 1024 * 1024)).unwrap();
        writer.write_all(b"foreign bytes").unwrap();
        target.sync_all().unwrap();
        assert_eq!(target.metadata().unwrap().len(), 16 * 1024 * 1024);
        assert!(evidence.validate_candidate_streams(&target).is_err());
        assert_eq!(version(&source).unwrap(), before);
    }
}
