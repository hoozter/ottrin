//! Pinned native source-tree preflight and private child staging. No publication.
use super::{
    EntryVersion, PlatformError, WindowsStableIdentity, io, metadata, namespace, security,
    stable_identity_from_handle, version,
};
use crate::windows_fs;
#[cfg(test)]
use std::ptr;
use std::{ffi::OsString, fs::File};
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_COMPRESSED, FILE_SHARE_READ};

struct SourceEntry {
    handle: File,
    before: EntryVersion,
    parent: Option<usize>,
    name: OsString,
    children: Vec<OsString>,
}

struct Entry {
    source: SourceEntry,
    candidate: windows_fs::PerObjectFile,
    candidate_identity: WindowsStableIdentity,
    payload: metadata::StagedEntry,
}

pub(super) struct StagedTree {
    entries: Vec<Entry>,
}

fn children(handle: &File, limit: usize) -> Result<Vec<OsString>, PlatformError> {
    windows_fs::directory_children_limited(handle, limit).map_err(io)
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
                namespace::validate_binding(
                    &parent.source.handle,
                    &entry.source.name,
                    entry.source.before.identity,
                )?;
                namespace::validate_binding(
                    &parent.candidate,
                    &entry.source.name,
                    entry.candidate_identity,
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
    #[cfg(test)]
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
            entry
                .payload
                .validate_source(&entry.source.handle, || progress(0))?;
            entry
                .payload
                .validate_candidate_streams(&entry.candidate, || progress(0))?;
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

    /// Derive the complete final DACL graph while every candidate is private.
    /// This never applies permissions; the journal owner must persist the plan
    /// before any future top-down security/publication transition.
    pub(super) fn destination_security_plan(
        &self,
        parent: &File,
        root_name: &std::ffi::OsStr,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<security::TreeSecurityPlan, PlatformError> {
        self.namespace()?;
        let plan = security::TreeSecurityPlan::capture(
            parent,
            self.entries
                .iter()
                .enumerate()
                .map(|(index, entry)| security::SecurityEntry {
                    parent: entry.source.parent,
                    name: if index == 0 {
                        root_name.to_owned()
                    } else {
                        entry.source.name.clone()
                    },
                    candidate: &entry.candidate,
                    directory: entry.source.before.directory,
                }),
            &mut progress,
        )?;
        self.namespace()?;
        plan.validate()?;
        Ok(plan)
    }

    pub(super) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// Mutation observer: returning an error prevents the next payload operation.
/// The journal records intent before child creation and creation/type-transition
/// ownership before any payload write. A verified tree is acknowledged last.
pub(super) trait StagingJournal {
    fn intent(
        &mut self,
        index: usize,
        parent: Option<usize>,
        name: &std::ffi::OsStr,
        source: &EntryVersion,
    ) -> Result<(), PlatformError>;
    fn created(
        &mut self,
        index: usize,
        parent: &File,
        candidate: &File,
    ) -> Result<(), PlatformError>;
    fn staged(
        &mut self,
        index: usize,
        evidence: &metadata::StagedEntry,
    ) -> Result<(), PlatformError>;
    fn verified(&mut self, count: usize) -> Result<(), PlatformError>;
}

pub(super) struct PinnedTree {
    sources: Vec<SourceEntry>,
}

/// Frozen observations of an existing destination, separate from newly staged
/// Copy data. They grant no rename, permission-write, Undo or cleanup authority.
/// Root-name binding belongs to the enclosing held-parent transaction.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExistingTreeEvidence {
    pub(super) entries: Vec<ExistingEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExistingEntry {
    pub(super) parent: Option<usize>,
    pub(super) name: Vec<u16>,
    pub(super) version: EntryVersion,
    pub(super) streams: Vec<super::StreamEvidence>,
    pub(super) compression: u16,
    pub(super) links: u32,
    pub(super) owner_group_dacl: [u8; 32],
}

impl PinnedTree {
    fn validate_sources(
        &self,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        for source in &self.sources {
            progress()?;
            if version(&source.handle)? != source.before {
                return Err(io("existing tree version changed during observation"));
            }
            if let Some(parent) = source.parent {
                namespace::validate_binding(
                    &self.sources[parent].handle,
                    &source.name,
                    source.before.identity,
                )?;
            }
            if source.before.directory
                && source.before.link.is_none()
                && children(&source.handle, source.children.len().saturating_add(1))?
                    != source.children
            {
                return Err(io("existing tree membership changed during observation"));
            }
        }
        Ok(())
    }

    /// Read through the existing no-follow source traversal and backup codec.
    /// Original hard links are observed, not broken or rejected like candidates.
    /// Normal read-induced access-time changes are allowed; persistent metadata,
    /// namespace, stream contents and the observed security scope must stay exact.
    pub(super) fn freeze_existing(
        &self,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<ExistingTreeEvidence, PlatformError> {
        use std::os::windows::ffi::OsStrExt;
        self.validate_sources(|| progress(0))?;
        let mut entries = Vec::with_capacity(self.sources.len());
        let mut total = 0u64;
        for source in &self.sources {
            progress(total)?;
            if source.before.attributes & !metadata::ADMITTED_ATTRIBUTES != 0 {
                return Err(io("unsupported existing-tree file attributes"));
            }
            let compression = metadata::compression(&source.handle)?;
            let links = namespace::link_count(&source.handle)?;
            if links == 0
                || (source.before.attributes & FILE_ATTRIBUTE_COMPRESSED != 0) != (compression != 0)
            {
                return Err(io(
                    "existing-tree compression or link count is inconsistent",
                ));
            }
            let owner_group_dacl = security::owner_group_dacl_digest(&source.handle)?;
            let streams = super::transfer(&source.handle, None, &mut |bytes| {
                progress(
                    total
                        .checked_add(bytes)
                        .ok_or_else(|| io("existing-tree progress overflow"))?,
                )
            })?;
            total = streams.iter().try_fold(total, |sum, stream| {
                sum.checked_add(stream.bytes)
                    .ok_or_else(|| io("existing-tree stream length overflow"))
            })?;
            entries.push(ExistingEntry {
                parent: source.parent,
                name: source.name.encode_wide().collect(),
                version: source.before.clone(),
                streams,
                compression,
                links,
                owner_group_dacl,
            });
        }
        self.validate_sources(|| progress(total))?;
        for (source, evidence) in self.sources.iter().zip(&entries) {
            progress(total)?;
            if namespace::link_count(&source.handle)? != evidence.links
                || metadata::compression(&source.handle)? != evidence.compression
                || security::owner_group_dacl_digest(&source.handle)? != evidence.owner_group_dacl
            {
                return Err(io("existing tree security, compression or aliases changed"));
            }
        }
        self.validate_sources(|| progress(total))?;
        Ok(ExistingTreeEvidence { entries })
    }

    pub(super) fn validate_existing(
        &self,
        expected: &ExistingTreeEvidence,
        progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<(), PlatformError> {
        if self.freeze_existing(progress)? != *expected {
            return Err(io(
                "existing tree differs from frozen evidence; no mutation authorized",
            ));
        }
        Ok(())
    }

    /// Complete source discovery before creating destination entries. No links
    /// are traversed and no source DELETE/data-write access is requested.
    pub(super) fn open(
        source: &File,
        max_entries: usize,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        if max_entries == 0 {
            return Err(io("tree entry budget is zero"));
        }
        let mut sources = vec![SourceEntry {
            handle: source.try_clone().map_err(io)?,
            before: version(source)?,
            parent: None,
            name: OsString::new(),
            children: Vec::new(),
        }];
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
        Ok(Self { sources })
    }
    pub(super) fn root(&self) -> &EntryVersion {
        &self.sources[0].before
    }
    pub(super) fn reject_destination_parent(&self, parent: &File) -> Result<(), PlatformError> {
        let id = stable_identity_from_handle(parent).map_err(io)?;
        if self
            .sources
            .iter()
            .any(|source| source.before.identity.same_object(id))
        {
            return Err(io("Copy destination is inside the source tree"));
        }
        Ok(())
    }
    pub(super) fn stage(
        self,
        candidate: &windows_fs::PerObjectFile,
        root_parent: &File,
        journal: &mut impl StagingJournal,
        mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
    ) -> Result<StagedTree, PlatformError> {
        let target = version(candidate)?;
        if self.root().directory != target.directory
            || target.length != 0
            || target.link.is_some()
            || (target.directory && !children(candidate, 0)?.is_empty())
            || self
                .sources
                .iter()
                .any(|source| source.before.identity.same_object(target.identity))
        {
            return Err(io(
                "tree candidate aliases the source, has a different type, is inside the source or is occupied",
            ));
        }
        let mut entries: Vec<Entry> = Vec::with_capacity(self.sources.len());
        let mut total = 0u64;
        for (index, source) in self.sources.into_iter().enumerate() {
            progress(total)?;
            if version(&source.handle)? != source.before {
                return Err(io(
                    "source changed after tree preflight; candidate retained",
                ));
            }
            let candidate = if let Some(parent) = source.parent {
                journal.intent(index, Some(parent), &source.name, &source.before)?;
                let candidate = windows_fs::PerObjectFile::create_private(
                    &entries[parent].candidate,
                    &source.name,
                    source.before.directory,
                    FILE_SHARE_READ,
                )
                .map_err(io)?;
                journal.created(index, &entries[parent].candidate, &candidate)?;
                candidate
            } else {
                let candidate = candidate.try_clone().map_err(io)?;
                journal.created(0, root_parent, &candidate)?;
                candidate
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
            journal.staged(index, &payload)?;
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
        journal.verified(result.entry_count())?;
        Ok(result)
    }
}

#[cfg(test)]
pub(super) fn stage_tree_unjournaled(
    source: &File,
    candidate: &windows_fs::PerObjectFile,
    max_entries: usize,
    mut progress: impl FnMut(u64) -> Result<(), PlatformError>,
) -> Result<StagedTree, PlatformError> {
    struct ComponentOnly;
    impl StagingJournal for ComponentOnly {
        fn intent(
            &mut self,
            _: usize,
            _: Option<usize>,
            _: &std::ffi::OsStr,
            _: &EntryVersion,
        ) -> Result<(), PlatformError> {
            Ok(())
        }
        fn created(&mut self, _: usize, _: &File, _: &File) -> Result<(), PlatformError> {
            Ok(())
        }
        fn staged(&mut self, _: usize, _: &metadata::StagedEntry) -> Result<(), PlatformError> {
            Ok(())
        }
        fn verified(&mut self, _: usize) -> Result<(), PlatformError> {
            Ok(())
        }
    }
    // The no-journal adapter exists only for the earlier codec/component cases.
    let tree = PinnedTree::open(source, max_entries, &mut progress)?;
    tree.stage(candidate, candidate, &mut ComponentOnly, progress)
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

    fn freeze(path: &Path) -> ExistingTreeEvidence {
        PinnedTree::open(&read(path), 64, |_| Ok(()))
            .unwrap()
            .freeze_existing(|_| Ok(()))
            .unwrap()
    }

    #[test]
    fn windows_native_existing_tree_freezes_streams_security_and_original_aliases_readonly() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        populate(&path);
        std::fs::hard_link(path.join("nested/file"), path.join("alias")).unwrap();
        std::fs::hard_link(path.join("alias"), root.path().join("outside-alias")).unwrap();
        super::super::tests::set_fixture_ea(&owner(&path));
        let raw = OsString::from_wide(&[b'u' as u16, 0xd800]);
        std::fs::write(path.join(&raw), b"raw name").unwrap();
        let before = freeze(&path);
        assert_eq!(before.entries.len(), 6);
        assert!(before.entries.iter().any(|entry| entry.links == 3));
        assert!(
            before.entries[0].streams.len() >= 2,
            "Directory named data and EA must be observed"
        );
        let bytes = super::super::evidence_digest(&before).unwrap();
        let tree = PinnedTree::open(&read(&path), 64, |_| Ok(())).unwrap();
        tree.validate_existing(&before, |_| Ok(())).unwrap();
        assert_eq!(
            super::super::evidence_digest(&tree.freeze_existing(|_| Ok(())).unwrap()).unwrap(),
            bytes
        );
        assert_eq!(
            std::fs::read(path.join("nested/file")).unwrap(),
            b"original tree data"
        );
        assert_eq!(
            std::fs::read(root.path().join("outside-alias")).unwrap(),
            b"original tree data"
        );
    }

    #[test]
    fn windows_native_existing_tree_refuses_changed_original_evidence_without_normalizing() {
        for case in [
            "payload",
            "named",
            "security",
            "alias",
            "replacement",
            "membership",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("original");
            populate(&path);
            let mut before = freeze(&path);
            match case {
                "payload" => {
                    std::fs::write(path.join("nested/file"), b"Original tree data").unwrap()
                }
                "named" => std::fs::write(
                    format!("{}:note:$DATA", path.display()),
                    b"Directory stream",
                )
                .unwrap(),
                "security" => super::super::tests::set_dacl(&path, "D:P(A;OICI;FA;;;OW)"),
                "alias" => {
                    std::fs::hard_link(path.join("nested/file"), root.path().join("outside-alias"))
                        .unwrap()
                }
                "replacement" => {
                    std::fs::rename(path.join("nested/file"), root.path().join("displaced"))
                        .unwrap();
                    std::fs::write(path.join("nested/file"), b"original tree data").unwrap();
                }
                "membership" => std::fs::write(path.join("added"), b"external addition").unwrap(),
                _ => unreachable!(),
            }
            let tree = PinnedTree::open(&read(&path), 64, |_| Ok(())).unwrap();
            if before.entries.len() == tree.sources.len() {
                // Prove the streams, security, alias and identity evidence is
                // checked even when cached timestamps match the new metadata.
                for (entry, source) in before.entries.iter_mut().zip(&tree.sources) {
                    let original_id = entry.version.identity;
                    entry.version = source.before.clone();
                    entry.version.identity = original_id;
                }
            }
            let changed = tree.freeze_existing(|_| Ok(())).unwrap();
            assert!(
                tree.validate_existing(&before, |_| Ok(())).is_err(),
                "{case}"
            );
            assert_eq!(
                tree.freeze_existing(|_| Ok(())).unwrap(),
                changed,
                "Refusal must not normalize {case}"
            );
        }
    }

    #[test]
    fn windows_native_existing_tree_cancellation_and_budget_leave_original_intact() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("original");
        populate(&path);
        let before = freeze(&path);
        assert!(PinnedTree::open(&read(&path), 2, |_| Ok(())).is_err());
        let tree = PinnedTree::open(&read(&path), 64, |_| Ok(())).unwrap();
        for stop in [0, 7, 16] {
            let mut calls = 0;
            let result = tree.freeze_existing(|_| {
                calls += 1;
                if calls > stop {
                    Err(io("injected existing-tree cancellation"))
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err(), "{stop}");
            tree.validate_existing(&before, |_| Ok(())).unwrap();
        }
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
        let staged = stage_tree_unjournaled(&source, &candidate, 1000, |bytes| {
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
        assert!(stage_tree_unjournaled(&source, &candidate, 100, |_| Ok(())).is_err());
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
            assert!(stage_tree_unjournaled(&read(&src), &candidate, budget, |_| Ok(())).is_err());
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
        let staged = stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
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
            let staged = stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
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
        let staged = stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
        let entry = staged
            .entries
            .iter()
            .find(|entry| !entry.source.before.directory)
            .unwrap();
        let mut changed: &File = &entry.candidate;
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
        let error = stage_tree_unjournaled(&read(&src), &candidate, 100, |bytes| {
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
        let staged = stage_tree_unjournaled(&source, &candidate, 100, |_| {
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
        let staged = stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
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
        let staged = stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).unwrap();
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
        let staged = stage_tree_unjournaled(&read(&src), &candidate, 1, |_| Ok(())).unwrap();
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
        assert!(stage_tree_unjournaled(&read(&src), &candidate, 100, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(location.join("unrelated")).unwrap(), b"keep");
    }
}
