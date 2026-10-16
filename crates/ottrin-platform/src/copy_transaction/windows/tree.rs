//! Pinned native source-tree preflight and private child staging. No publication.
use super::{
    EntryVersion, PlatformError, WindowsStableIdentity, io, metadata, namespace,
    stable_identity_from_handle, version,
};
use crate::windows_fs::{self, RelativeOptions};
use std::{ffi::OsString, fs::File, ptr};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
};

struct SourceEntry {
    handle: File,
    before: EntryVersion,
    parent: Option<usize>,
    name: OsString,
    children: Vec<OsString>,
}

struct Entry {
    source: SourceEntry,
    candidate: File,
    candidate_identity: WindowsStableIdentity,
    payload: metadata::StagedEntry,
}

pub(super) struct StagedTree {
    entries: Vec<Entry>,
}

fn children(handle: &File, limit: usize) -> Result<Vec<OsString>, PlatformError> {
    windows_fs::directory_children_limited(handle, limit).map_err(io)
}

fn binding(
    parent: &File,
    name: &std::ffi::OsStr,
    held: &File,
    directory: bool,
) -> Result<(), PlatformError> {
    let observed = unsafe {
        windows_fs::open_relative(
            parent,
            name,
            RelativeOptions {
                directory: Some(directory),
                create_new: false,
                access: FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                security: ptr::null(),
            },
        )
    }
    .map_err(io)?;
    if stable_identity_from_handle(&observed).map_err(io)?
        != stable_identity_from_handle(held).map_err(io)?
    {
        return Err(io("tree entry binding changed; all entries retained"));
    }
    Ok(())
}

impl StagedTree {
    fn bindings(&self) -> Result<(), PlatformError> {
        for entry in &self.entries {
            if version(&entry.source.handle)? != entry.source.before
                || stable_identity_from_handle(&entry.candidate).map_err(io)?
                    != entry.candidate_identity
            {
                return Err(io("tree source version or candidate identity changed"));
            }
            if let Some(parent) = entry.source.parent {
                let parent = &self.entries[parent];
                binding(
                    &parent.source.handle,
                    &entry.source.name,
                    &entry.source.handle,
                    entry.source.before.directory,
                )?;
                binding(
                    &parent.candidate,
                    &entry.source.name,
                    &entry.candidate,
                    entry.source.before.directory,
                )?;
            }
        }
        Ok(())
    }

    fn namespace(&self) -> Result<(), PlatformError> {
        self.bindings()?;
        for entry in &self.entries {
            if entry.source.before.directory
                && entry.source.before.link.is_none()
                && (children(
                    &entry.source.handle,
                    entry.source.children.len().saturating_add(1),
                )? != entry.source.children
                    || children(
                        &entry.candidate,
                        entry.source.children.len().saturating_add(1),
                    )? != entry.source.children)
            {
                return Err(io("tree child membership changed; all entries retained"));
            }
        }
        Ok(())
    }

    /// Verify the complete staged tree again before publication. Stream rereads
    /// precede the last bottom-up timestamp writes. Root name bindings belong to
    /// the enclosing transaction, which must validate its pinned stage/parents.
    pub(super) fn verify_and_finalize(
        &self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        // Check timestamps before verification's own reads can affect them.
        for entry in &self.entries {
            entry
                .payload
                .validate_candidate_metadata(&entry.candidate)?;
        }
        self.finish(&mut progress)
    }

    fn finish(
        &self,
        progress: &mut impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        self.namespace()?;
        for entry in &self.entries {
            progress(0)?;
            entry.payload.validate_after_children(&entry.candidate)?;
            entry.payload.validate_source(&entry.source.handle)?;
            entry.payload.validate_candidate_streams(&entry.candidate)?;
        }
        // All namespace/content reads finish before restoring parent times.
        self.namespace()?;
        for entry in self.entries.iter().rev() {
            progress(0)?;
            entry.payload.apply_final_metadata(&entry.candidate)?;
        }
        self.bindings()?;
        for entry in &self.entries {
            entry
                .payload
                .validate_candidate_metadata(&entry.candidate)?;
        }
        Ok(())
    }

    pub(super) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Stage ordinary files/directories and known links without following targets. Source handles
/// are read-only and remain pinned until the result is dropped. `max_entries` is
/// the transaction owner's explicit handle/memory budget (including the root).
/// Every failure retains the independently owned candidate; no Drop cleanup.
/// Unknown reparse tags and specialized storage are explicitly refused.
pub(super) fn stage_tree(
    source: &File,
    candidate: &File,
    max_entries: usize,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<StagedTree, PlatformError> {
    if max_entries == 0 {
        return Err(io("tree entry budget is zero"));
    }
    let source_version = version(source)?;
    let target = version(candidate)?;
    if source_version.directory != target.directory
        || source_version.identity.same_object(target.identity)
        || target.length != 0
        || target.link.is_some()
        || (target.directory && !children(candidate, 0)?.is_empty())
    {
        return Err(io(
            "tree candidate aliases the source, has a different type or is occupied",
        ));
    }
    let mut sources = vec![SourceEntry {
        handle: source.try_clone().map_err(io)?,
        before: source_version,
        parent: None,
        name: OsString::new(),
        children: Vec::new(),
    }];
    // Finish source discovery before creating any child. A candidate anywhere
    // in the source tree is rejected by held identity, not path prefix guesses.
    let mut index = 0;
    while index < sources.len() {
        progress(0)?;
        if sources[index].before.directory && sources[index].before.link.is_none() {
            let names = children(
                &sources[index].handle,
                max_entries.saturating_sub(sources.len()),
            )?;
            for name in &names {
                progress(0)?;
                let handle = namespace::open_source(&sources[index].handle, name, None)?;
                let before = version(&handle)?;
                if before.identity.same_object(target.identity) {
                    return Err(io(
                        "Copy destination is inside the source tree; candidate retained",
                    ));
                }
                sources.push(SourceEntry {
                    handle,
                    before,
                    parent: Some(index),
                    name: name.clone(),
                    children: Vec::new(),
                });
            }
            sources[index].children = names;
        }
        index += 1;
    }
    // No recursion or path-based reopen: parents always precede their children.
    let mut entries: Vec<Entry> = Vec::with_capacity(sources.len());
    let mut total = 0u64;
    for source in sources {
        progress(total)?;
        if version(&source.handle)? != source.before {
            return Err(io(
                "source changed after tree preflight; candidate retained",
            ));
        }
        let candidate = if let Some(parent) = source.parent {
            windows_fs::create_private(
                &entries[parent].candidate,
                &source.name,
                source.before.directory,
                FILE_SHARE_READ,
            )
            .map_err(io)?
        } else {
            candidate.try_clone().map_err(io)?
        };
        let payload = metadata::stage_entry_payload(&source.handle, &candidate, |copied| {
            progress(
                total
                    .checked_add(copied)
                    .ok_or_else(|| io("tree progress overflow"))?,
            )
        })?;
        total = payload.streams.iter().try_fold(total, |sum, stream| {
            sum.checked_add(stream.bytes)
                .ok_or_else(|| io("tree stream length overflow"))
        })?;
        let candidate_identity = stable_identity_from_handle(&candidate).map_err(io)?;
        entries.push(Entry {
            source,
            candidate,
            candidate_identity,
            payload,
        });
    }
    let result = StagedTree { entries };
    result.finish(&mut |_| progress(total))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::{
        fs::OpenOptions,
        io::{Seek, SeekFrom, Write},
        os::windows::{ffi::OsStringExt, fs::OpenOptionsExt},
        path::Path,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FileBasicInfo,
        SetFileInformationByHandle,
    };

    fn read(path: &Path) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .unwrap()
    }
    fn populate(path: &Path) {
        std::fs::create_dir(path).unwrap();
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::create_dir(path.join("empty")).unwrap();
        std::fs::write(path.join("nested").join("file"), b"original tree data").unwrap();
        std::fs::write(
            format!("{}:note:$DATA", path.display()),
            b"directory stream",
        )
        .unwrap();
    }
    fn owner(path: &Path) -> File {
        super::super::tests::parent(path)
    }

    #[test]
    fn windows_native_copy_tree_stages_populated_unicode_raw_names_streams_and_metadata() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(src.join("empty")).unwrap();
        std::fs::create_dir(src.join("folder")).unwrap();
        // More than one 64KiB native enumeration buffer.
        for i in 0..160 {
            let name = format!("{i:03}-{}-🦀", "x".repeat(190));
            std::fs::write(src.join("folder").join(name), [i as u8]).unwrap();
        }
        std::fs::write(src.join("first"), b"independent hard-link copy").unwrap();
        std::fs::hard_link(src.join("first"), src.join("second")).unwrap();
        let raw = OsString::from_wide(&[b'u' as u16, 0xd800]);
        std::fs::write(src.join(&raw), b"raw UTF-16 filename").unwrap();
        std::fs::write(
            format!("{}:réservé-🦀:$DATA", src.display()),
            b"root named data",
        )
        .unwrap();
        std::fs::write(format!("{}:empty:$DATA", src.join("first").display()), b"").unwrap();
        let source_write = owner(&src);
        super::super::tests::set_fixture_ea(&source_write);
        let times = FILE_BASIC_INFO {
            CreationTime: 132_000_000_000_000_000,
            LastAccessTime: 132_100_000_000_000_000,
            LastWriteTime: 132_200_000_000_000_000,
            ChangeTime: 0,
            FileAttributes: 0,
        };
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    source_write.as_raw_handle().cast(),
                    FileBasicInfo,
                    ptr::from_ref(&times).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            },
            0
        );
        drop(source_write);
        let source = read(&src);
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let mut last = 0;
        let staged = stage_tree(&source, &candidate, 1000, |bytes| {
            assert!(bytes >= last);
            last = bytes;
            Ok(())
        })
        .unwrap();
        assert_eq!(staged.entry_count(), 166);
        assert!(last > 160);
        stage.validate_candidate(&candidate, true).unwrap();
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        for entry in &staged.entries {
            super::super::tests::assert_owner_only(&entry.candidate);
            entry
                .payload
                .validate_candidate_metadata(&entry.candidate)
                .unwrap();
        }
        let target = windows_fs::location(&candidate).unwrap();
        assert_eq!(
            std::fs::read(target.join(&raw)).unwrap(),
            b"raw UTF-16 filename"
        );
        assert_eq!(
            std::fs::read(format!("{}:réservé-🦀:$DATA", target.display())).unwrap(),
            b"root named data"
        );
        assert_eq!(
            std::fs::read_dir(target.join("folder")).unwrap().count(),
            160
        );
        let first = staged
            .entries
            .iter()
            .find(|entry| entry.source.name == "first")
            .unwrap();
        let second = staged
            .entries
            .iter()
            .find(|entry| entry.source.name == "second")
            .unwrap();
        assert_eq!(first.source.before.identity, second.source.before.identity);
        assert_ne!(first.candidate_identity, second.candidate_identity);
        assert_eq!(
            metadata::basic(&candidate).unwrap().LastWriteTime,
            times.LastWriteTime
        );
    }

    #[test]
    fn windows_native_copy_tree_refuses_destination_inside_source_before_child_creation() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        populate(&src);
        let source = read(&src);
        let stage = namespace::PrivateStage::create(&source).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        assert!(stage_tree(&source, &candidate, 100, |_| Ok(())).is_err());
        assert!(children(&candidate, 0).unwrap().is_empty());
        assert!(
            super::super::transfer(&candidate, None, &mut |_| Ok(()))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(src.join("nested/file")).unwrap(),
            b"original tree data"
        );
    }

    #[test]
    fn windows_native_copy_tree_budget_preflight_and_known_links_never_follow_targets() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        populate(&src);
        let parent = owner(root.path());
        for budget in [0, 1, 3] {
            let stage = namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(true).unwrap();
            assert!(stage_tree(&read(&src), &candidate, budget, |_| Ok(())).is_err());
            assert!(children(&candidate, 0).unwrap().is_empty());
            assert!(
                super::super::transfer(&candidate, None, &mut |_| Ok(()))
                    .unwrap()
                    .is_empty()
            );
        }
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"keep outside bytes").unwrap();
        std::os::windows::fs::symlink_file(&outside, src.join("file-link")).unwrap();
        std::os::windows::fs::symlink_dir(root.path(), src.join("dir-link")).unwrap();
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let staged = stage_tree(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
        assert_eq!(staged.entry_count(), 6);
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        for entry in &staged.entries {
            if let Some(expected) = &entry.source.before.link {
                assert_eq!(
                    super::super::links::capture(&entry.candidate)
                        .unwrap()
                        .as_ref(),
                    Some(expected)
                );
            }
        }
        assert_eq!(std::fs::read(outside).unwrap(), b"keep outside bytes");
    }

    #[test]
    fn windows_native_copy_tree_candidate_and_source_child_changes_are_retained() {
        for change_source in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let src = root.path().join("source");
            populate(&src);
            let parent = owner(root.path());
            let stage = namespace::PrivateStage::create(&parent).unwrap();
            let candidate = stage.create_candidate(true).unwrap();
            let staged = stage_tree(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
            let changed = if change_source {
                src.clone()
            } else {
                windows_fs::location(&candidate).unwrap()
            };
            std::fs::write(changed.join("unrelated"), b"user added data").unwrap();
            assert!(staged.verify_and_finalize(|_| Ok(())).is_err());
            assert_eq!(
                std::fs::read(changed.join("unrelated")).unwrap(),
                b"user added data"
            );
            assert_eq!(
                std::fs::read(src.join("nested/file")).unwrap(),
                b"original tree data"
            );
        }
    }

    #[test]
    fn windows_native_copy_tree_candidate_stream_tampering_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        populate(&src);
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let staged = stage_tree(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
        let entry = staged
            .entries
            .iter()
            .find(|entry| !entry.source.before.directory)
            .unwrap();
        let mut changed = &entry.candidate;
        changed.seek(SeekFrom::Start(0)).unwrap();
        changed.write_all(b"modified tree data").unwrap();
        changed.sync_all().unwrap();
        assert!(staged.verify_and_finalize(|_| Ok(())).is_err());
        let path = windows_fs::location(&entry.candidate).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"modified tree data");
        assert_eq!(
            std::fs::read(src.join("nested/file")).unwrap(),
            b"original tree data"
        );
    }

    #[test]
    fn windows_native_copy_tree_cancellation_retains_partial_candidate_and_original() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        std::fs::create_dir(&src).unwrap();
        let original = vec![31u8; super::super::BUFFER_BYTES * 3];
        std::fs::write(src.join("large"), &original).unwrap();
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let error = stage_tree(&read(&src), &candidate, 100, |bytes| {
            if bytes > 0 {
                Err(io("injected tree cancellation"))
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap();
        assert!(error.to_string().contains("injected tree cancellation"));
        assert_eq!(std::fs::read(src.join("large")).unwrap(), original);
        assert_eq!(
            children(&candidate, 100).unwrap(),
            vec![OsString::from("large")]
        );
        let target = windows_fs::location(&candidate).unwrap().join("large");
        OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(target)
            .unwrap();
        OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(src.join("large"))
            .unwrap();
    }

    #[test]
    fn windows_native_copy_tree_pinned_roots_survive_ancestor_alias_retarget() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        std::fs::create_dir(first.join("destination")).unwrap();
        std::fs::create_dir(second.join("destination")).unwrap();
        populate(&first.join("source"));
        populate(&second.join("source"));
        std::fs::write(second.join("source/nested/file"), b"unrelated alias target").unwrap();
        let alias = root.path().join("alias");
        std::os::windows::fs::symlink_dir(&first, &alias).unwrap();
        let source = read(&alias.join("source"));
        let parent = owner(&alias.join("destination"));
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let mut changed = false;
        let staged = stage_tree(&source, &candidate, 100, |_| {
            if !changed {
                std::fs::remove_dir(&alias).unwrap();
                std::os::windows::fs::symlink_dir(&second, &alias).unwrap();
                changed = true;
            }
            Ok(())
        })
        .unwrap();
        assert!(changed);
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        stage.validate_candidate(&candidate, true).unwrap();
        assert_eq!(
            std::fs::read(
                first
                    .join("destination")
                    .join(&stage.candidate_name)
                    .join("nested/file")
            )
            .unwrap(),
            b"original tree data"
        );
        assert_eq!(
            std::fs::read(second.join("source/nested/file")).unwrap(),
            b"unrelated alias target"
        );
        assert_eq!(
            std::fs::read_dir(second.join("destination"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn windows_native_copy_tree_read_only_source_acl_needs_no_delete_or_write_access() {
        use super::super::tests::{ReadOnlyAcl, set_dacl};
        use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_GENERIC_WRITE};
        let root = tempfile::tempdir().unwrap();
        let source_parent = root.path().join("read-only-parent");
        std::fs::create_dir(&source_parent).unwrap();
        let src = source_parent.join("source");
        populate(&src);
        let guard = ReadOnlyAcl(vec![
            source_parent,
            src.clone(),
            src.join("nested"),
            src.join("nested/file"),
            src.join("empty"),
        ]);
        for path in &guard.0 {
            set_dacl(path, "D:P(A;OICI;FRWD;;;OW)");
        }
        for access in [DELETE, FILE_GENERIC_WRITE] {
            // The parent of the source must deny delete-child as well;
            // otherwise Windows can grant DELETE through that parent's ACL.
            for path in guard.0.iter().skip(1) {
                assert!(
                    OpenOptions::new()
                        .access_mode(access)
                        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
                        .open(path)
                        .is_err()
                );
            }
        }
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let staged = stage_tree(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        let target = windows_fs::location(&candidate).unwrap();
        assert_eq!(
            std::fs::read(target.join("nested/file")).unwrap(),
            b"original tree data"
        );
        assert_eq!(
            std::fs::read(src.join("nested/file")).unwrap(),
            b"original tree data"
        );
        drop(staged);
        drop(guard);
    }

    #[test]
    fn windows_native_copy_tree_child_rebinding_preserves_original_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        populate(&src);
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let staged = stage_tree(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
        let entry = staged
            .entries
            .iter()
            .find(|entry| !entry.source.before.directory)
            .unwrap();
        let target = windows_fs::location(&entry.candidate).unwrap();
        crate::move_transaction::rename_windows_handle_no_replace(
            &entry.candidate,
            &parent,
            std::ffi::OsStr::new("retained-file"),
        )
        .unwrap();
        std::fs::write(&target, b"unrelated replacement").unwrap();
        assert!(
            staged
                .namespace()
                .unwrap_err()
                .to_string()
                .contains("binding changed")
        );
        assert!(staged.verify_and_finalize(|_| Ok(())).is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"unrelated replacement");
        assert_eq!(
            std::fs::read(root.path().join("retained-file")).unwrap(),
            b"original tree data"
        );
    }

    #[test]
    fn windows_native_copy_tree_accepts_regular_root_and_rejects_occupied_directory() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("file");
        std::fs::write(&src, b"single file").unwrap();
        let parent = owner(root.path());
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(false).unwrap();
        let staged = stage_tree(&read(&src), &candidate, 1, |_| Ok(())).unwrap();
        assert_eq!(staged.entry_count(), 1);
        staged.verify_and_finalize(|_| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(windows_fs::location(&candidate).unwrap()).unwrap(),
            b"single file"
        );
        let src = root.path().join("source-directory");
        populate(&src);
        let stage = namespace::PrivateStage::create(&parent).unwrap();
        let candidate = stage.create_candidate(true).unwrap();
        let location = windows_fs::location(&candidate).unwrap();
        std::fs::write(location.join("unrelated"), b"keep").unwrap();
        assert!(stage_tree(&read(&src), &candidate, 100, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(location.join("unrelated")).unwrap(), b"keep");
    }
}
