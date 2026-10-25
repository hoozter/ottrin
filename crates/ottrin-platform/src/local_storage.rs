//! Short-lived filesystem provenance for local Linux file operations.
//! Navigation discovery deliberately omits system/temporary mounts. Operations
//! still need to identify the actual mount reached by each source/destination.

use std::ffi::{CStr, CString};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

// Linux 6.8 statx ABI; unlike STATX_MNT_ID, this ID is not reused during boot.
// libc's existing statx layout contains stx_mnt_id but not this mask constant.
const STATX_MNT_ID_UNIQUE: u32 = 0x4000;

/// Notifications on these local inode implementations can invalidate a cached
/// namespace proof. Unknown/remote filesystems require fresh full checks.
pub(crate) fn supports_local_inode_notifications(fd: &impl AsFd) -> bool {
    rustix::fs::fstatfs(fd).is_ok_and(|stat| {
        matches!(
            stat.f_type as u64,
            0xef53 | 0x9123683e | 0x58465342 | 0x01021994
        )
    })
}

/// Worker-scoped mount-table invalidation, including same-inode bind mounts.
/// Holding this proc file does not retain handles to an operation's filesystem.
pub(crate) struct MountNamespaceGuard {
    mounts: std::fs::File,
    namespace: (u64, u64),
}

impl MountNamespaceGuard {
    pub(crate) fn new() -> io::Result<Self> {
        let mounts = std::fs::File::open("/proc/thread-self/mounts")?;
        let namespace = std::fs::metadata("/proc/thread-self/ns/mnt")?;
        Ok(Self {
            mounts,
            namespace: (namespace.dev(), namespace.ino()),
        })
    }

    pub(crate) fn validate(&self) -> io::Result<()> {
        let namespace = std::fs::metadata("/proc/thread-self/ns/mnt")?;
        let mut poll = libc::pollfd {
            fd: self.mounts.as_raw_fd(),
            events: libc::POLLPRI | libc::POLLERR,
            revents: 0,
        };
        // SAFETY: poll addresses one initialized descriptor and never blocks.
        let ready = unsafe { libc::poll(&mut poll, 1, 0) };
        if ready < 0 {
            return Err(io::Error::last_os_error());
        }
        if ready != 0 || self.namespace != (namespace.dev(), namespace.ino()) {
            return Err(io::Error::other("mount namespace changed"));
        }
        Ok(())
    }
}

/// Queries the opened object itself, including an unlinked file or an O_PATH
/// symlink, without resolving /proc/self/fd or requiring a surviving pathname.
pub(crate) fn local_fd_mount_identity(fd: &impl AsFd) -> io::Result<u64> {
    unique_mount_identity(fd.as_fd().as_raw_fd(), c"", libc::AT_EMPTY_PATH)
}

fn unique_mount_identity(dirfd: libc::c_int, name: &CStr, flags: libc::c_int) -> io::Result<u64> {
    let mut info = MaybeUninit::<libc::statx>::uninit();
    // SAFETY: name is NUL-terminated and info is writable for statx's ABI.
    if unsafe {
        libc::statx(
            dirfd,
            name.as_ptr(),
            flags,
            STATX_MNT_ID_UNIQUE,
            info.as_mut_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful statx initialises the entire output structure.
    let info = unsafe { info.assume_init() };
    if info.stx_mask & STATX_MNT_ID_UNIQUE == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unique mount identity requires Linux 6.8 or newer",
        ));
    }
    Ok(info.stx_mnt_id)
}

/// Identifies the mount reached by a path without canonicalising or changing it.
/// Sources use `follow_final_symlink=false` so copying/moving a link binds its
/// own filesystem. Directory destinations follow their final link. For a new
/// entry, the nearest existing ancestor identifies its containing filesystem;
/// validation repeats this lookup so a newly attached nested mount is detected.
/// Missing kernel support and permission errors are returned, never converted
/// into a parent-volume identity or an unprotected operation.
pub fn local_path_mount_identity(path: &Path, follow_final_symlink: bool) -> io::Result<u64> {
    let absolute = std::path::absolute(path)?;
    let mut probe = absolute.as_path();
    let mut follow = follow_final_symlink;
    loop {
        let name = CString::new(probe.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))?;
        let flags = if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW };
        let error = match unique_mount_identity(libc::AT_FDCWD, &name, flags) {
            Ok(identity) => return Ok(identity),
            Err(error) => error,
        };
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
        match std::fs::symlink_metadata(probe) {
            // A dangling link is an existing entry, not a new path under the
            // parent filesystem. Never use that parent for a followed link.
            Ok(_) => return Err(error),
            Err(metadata_error) if metadata_error.kind() == io::ErrorKind::NotFound => {}
            Err(metadata_error) => return Err(metadata_error),
        }
        probe = probe.parent().ok_or(error)?;
        // Ancestors are traversed by the eventual pathname operation, so a
        // symlink ancestor must identify the filesystem it actually reaches.
        follow = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn opened_mount_identity_follows_the_descriptor_after_unlink_and_for_symlinks() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir_in("/dev/shm").unwrap();
        let source_id = local_path_mount_identity(source.path(), true).unwrap();
        let target_id = local_path_mount_identity(target.path(), true).unwrap();
        assert_ne!(source_id, target_id);
        let link = source.path().join("link");
        symlink(target.path(), &link).unwrap();
        let own = rustix::fs::open(
            &link,
            rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .unwrap();
        let followed = std::fs::File::open(&link).unwrap();
        assert_eq!(local_fd_mount_identity(&own).unwrap(), source_id);
        assert_eq!(local_fd_mount_identity(&followed).unwrap(), target_id);
        std::fs::remove_file(&link).unwrap();
        assert_eq!(local_fd_mount_identity(&own).unwrap(), source_id);
        std::fs::remove_dir(target.path()).unwrap();
        assert_eq!(local_fd_mount_identity(&followed).unwrap(), target_id);
    }

    #[test]
    fn source_links_bind_their_own_mount_and_destinations_follow_the_target() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir_in("/dev/shm").unwrap();
        let source_id = local_path_mount_identity(source.path(), true).unwrap();
        let target_id = local_path_mount_identity(target.path(), true).unwrap();
        assert_ne!(source_id, target_id, "fixture needs independent mounts");
        let link = source.path().join("link");
        symlink(target.path(), &link).unwrap();
        assert_eq!(local_path_mount_identity(&link, false).unwrap(), source_id);
        assert_eq!(local_path_mount_identity(&link, true).unwrap(), target_id);
        std::fs::remove_dir(target.path()).unwrap();
        assert_eq!(local_path_mount_identity(&link, false).unwrap(), source_id);
        assert!(local_path_mount_identity(&link, true).is_err());
    }

    #[test]
    fn newly_created_ancestor_is_resolved_again_instead_of_binding_its_old_parent() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir_in("/dev/shm").unwrap();
        let path = source.path().join("new/entry.txt");
        let captured = local_path_mount_identity(&path, false).unwrap();
        symlink(target.path(), source.path().join("new")).unwrap();
        assert_ne!(local_path_mount_identity(&path, false).unwrap(), captured);
    }

    #[test]
    fn invalid_paths_are_not_converted_to_parent_provenance() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("file");
        std::fs::write(&file, b"data").unwrap();
        assert!(local_path_mount_identity(&file.join("child"), false).is_err());
        assert!(local_path_mount_identity(Path::new("bad\0path"), false).is_err());
    }
}

/// Identity-only filesystem evidence, scoped by the record's boot and unique
/// mount IDs. Its filesystem-defined identifier can distinguish reused inode
/// numbers. No descriptor is retained between sessions.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObjectHandle {
    kind: i32,
    bytes: Vec<u8>,
}

impl ObjectHandle {
    pub(crate) fn capture(fd: &impl rustix::fd::AsFd) -> std::io::Result<Option<Self>> {
        use rustix::fd::AsRawFd;
        // file_handle has a flexible byte array. This storage supplies its
        // correctly aligned header and Linux's current MAX_HANDLE_SZ capacity.
        #[repr(C)]
        struct Storage {
            size: libc::c_uint,
            kind: libc::c_int,
            bytes: [u8; 128],
        }
        let mut storage = Storage {
            size: 128,
            kind: 0,
            bytes: [0; 128],
        };
        let mut mount_id = 0;
        // SAFETY: fd is live; the empty C string and both output buffers remain
        // valid for the call. Storage matches file_handle's header layout and
        // provides the advertised byte capacity. AT_EMPTY_PATH addresses the
        // opened object, including a symlink opened with O_PATH | O_NOFOLLOW.
        let result = unsafe {
            libc::syscall(
                libc::SYS_name_to_handle_at,
                fd.as_fd().as_raw_fd(),
                c"".as_ptr(),
                &mut storage as *mut Storage,
                &mut mount_id as *mut libc::c_int,
                // Linux UAPI AT_HANDLE_FID aliases AT_REMOVEDIR (0x200).
                libc::AT_EMPTY_PATH | libc::AT_REMOVEDIR,
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::EOPNOTSUPP | libc::ENOSYS) => Ok(None),
                // An unchanged capacity means this particular object cannot
                // supply a handle; an increased capacity is a real size error.
                Some(libc::EOVERFLOW) if storage.size <= 128 => Ok(None),
                _ => Err(error),
            };
        }
        if storage.size == 0 || storage.size > 128 {
            return Err(std::io::Error::other(
                "invalid filesystem object handle size",
            ));
        }
        Ok(Some(Self {
            kind: storage.kind,
            bytes: storage.bytes[..storage.size as usize].to_vec(),
        }))
    }

    pub(crate) fn valid(&self) -> bool {
        !self.bytes.is_empty() && self.bytes.len() <= 128
    }
}
