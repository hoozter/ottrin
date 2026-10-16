//! Whole-tree final DACL projection. Capture/validation never apply permissions.
use super::super::{EntryVersion, version};
use super::*;
use std::{collections::BTreeSet, ffi::OsString, os::windows::ffi::OsStrExt};

pub(in super::super) struct SecurityEntry<'a> {
    pub(in super::super) parent: Option<usize>,
    pub(in super::super) name: OsString,
    pub(in super::super) candidate: &'a File,
    pub(in super::super) directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in super::super) struct PlannedEntry {
    parent: Option<usize>,
    name: Vec<u16>,
    identity: WindowsStableIdentity,
    directory: bool,
    private: PrivateSecurity,
    final_security: Parts,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in super::super) struct TreeSecurityEvidence {
    creator: Vec<u8>,
    parent: WindowsStableIdentity,
    parent_security: Parts,
    entries: Vec<PlannedEntry>,
}
impl TreeSecurityEvidence {
    pub(in super::super) fn binds_staged<'a>(
        &self,
        creator: &[u8],
        parent: WindowsStableIdentity,
        rows: impl ExactSizeIterator<
            Item = (
                Option<usize>,
                &'a [u16],
                WindowsStableIdentity,
                bool,
                &'a PrivateSecurity,
            ),
        >,
    ) -> bool {
        self.creator == creator
            && self.parent == parent
            && self.parent_security.valid_record()
            && self.entries.len() == rows.len()
            && self.entries.iter().zip(rows).all(
                |(entry, (parent, name, identity, directory, private))| {
                    entry.parent == parent
                        && entry.name == name
                        && entry.identity == identity
                        && entry.directory == directory
                        && entry.private == *private
                        && entry.final_security.valid_record()
                        && entry.final_security.present
                        && entry.final_security.dacl.is_some()
                        && entry.final_security.owner == private.0.owner
                        && entry.final_security.group == private.0.group
                },
            )
    }
}

struct Entry {
    name: OsString,
    parent: Option<usize>,
    handle: File,
    before: EntryVersion,
    private: PrivateSecurity,
    children: Vec<OsString>,
    policy: DestinationSecurity,
}
/// Native descriptors and pinned objects for a complete final permission plan.
/// Evidence can be journaled before mutation; this type has no apply/cleanup API.
pub(in super::super) struct TreeSecurityPlan {
    parent: File,
    creator: Vec<u8>,
    entries: Vec<Entry>,
}
impl TreeSecurityPlan {
    pub(in super::super) fn capture<'a>(
        parent: &File,
        inputs: impl IntoIterator<Item = SecurityEntry<'a>>,
        mut progress: impl FnMut() -> Result<(), PlatformError>,
    ) -> Result<Self, PlatformError> {
        let token = current_token()?;
        let creator = owner_for_token(&token)?;
        let mut entries: Vec<Entry> = Vec::new();
        let mut names = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for (index, input) in inputs.into_iter().enumerate() {
            progress()?;
            windows_fs::valid_component(&input.name).map_err(io)?;
            let before = version(input.candidate)?;
            if before.directory != input.directory
                || !identities.insert(before.identity.object_key())
                || !names.insert((input.parent, input.name.encode_wide().collect::<Vec<_>>()))
            {
                return Err(io(
                    "Copy security plan has wrong entry kinds or aliased/duplicate entries",
                ));
            }
            let private = PrivateSecurity::capture(input.candidate, &creator)?;
            let policy = if index == 0 {
                if input.parent.is_some() {
                    return Err(io("Copy security plan root has a parent"));
                }
                namespace::validate_binding(parent, &input.name, before.identity)?;
                DestinationSecurity::capture_with_token(parent, input.directory, &token)?
            } else {
                let p = input.parent.and_then(|p| entries.get(p)).ok_or_else(|| {
                    io("Copy security plan has a missing/forward parent or multiple roots")
                })?;
                if !p.before.directory || p.before.link.is_some() {
                    return Err(io(
                        "Copy security plan cannot create children under a file or link",
                    ));
                }
                namespace::validate_binding(&p.handle, &input.name, before.identity)?;
                let mut actual = capture(&p.handle)?;
                let original_parent = parts(actual.as_mut_ptr().cast())?;
                let (derived, expected) = derive(p.policy.derived.0, &token, input.directory)?;
                let policy = DestinationSecurity {
                    parent: p.before.identity,
                    // The future apply must see the parent's FINAL permissions.
                    parent_security: p.policy.expected.clone(),
                    directory: input.directory,
                    derived,
                    expected,
                };
                let mut after = capture(&p.handle)?;
                if parts(after.as_mut_ptr().cast())? != original_parent {
                    return Err(io("private Copy parent changed while projecting security"));
                }
                policy
            };
            if private.0.owner != policy.expected.owner || private.0.group != policy.expected.group
            {
                return Err(io(
                    "Copy candidate owner/group differ from the final creator policy",
                ));
            }
            let handle = input.candidate.try_clone().map_err(io)?;
            entries.push(Entry {
                name: input.name,
                parent: input.parent,
                handle,
                before,
                private,
                children: Vec::new(),
                policy,
            });
        }
        if entries.is_empty() {
            return Err(io("Copy security plan is empty"));
        }
        for index in 1..entries.len() {
            let p = entries[index].parent.unwrap();
            let name = entries[index].name.clone();
            entries[p].children.push(name);
        }
        for entry in &mut entries {
            entry
                .children
                .sort_unstable_by(|a, b| a.encode_wide().cmp(b.encode_wide()));
        }
        let result = Self {
            parent: parent.try_clone().map_err(io)?,
            creator,
            entries,
        };
        result.validate()?;
        Ok(result)
    }
    pub(in super::super) fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub(in super::super) fn evidence(&self) -> TreeSecurityEvidence {
        TreeSecurityEvidence {
            creator: self.creator.clone(),
            parent: self.entries[0].policy.parent,
            parent_security: self.entries[0].policy.parent_security.clone(),
            entries: self
                .entries
                .iter()
                .map(|e| PlannedEntry {
                    parent: e.parent,
                    name: e.name.encode_wide().collect(),
                    identity: e.before.identity,
                    directory: e.before.directory,
                    private: e.private.clone(),
                    final_security: e.policy.expected.clone(),
                })
                .collect(),
        }
    }
    pub(in super::super) fn validate(&self) -> Result<(), PlatformError> {
        if current_owner()? != self.creator {
            return Err(io("Copy security plan creator context changed"));
        }
        self.entries[0].policy.validate_parent(&self.parent)?;
        for entry in &self.entries {
            let parent = entry
                .parent
                .map(|p| &self.entries[p].handle)
                .unwrap_or(&self.parent);
            namespace::validate_binding(parent, &entry.name, entry.before.identity)?;
            entry.private.validate(&entry.handle, &self.creator)?;
            if version(&entry.handle)? != entry.before {
                return Err(io(
                    "Copy candidate metadata changed after security planning",
                ));
            }
            if entry.before.directory
                && entry.before.link.is_none()
                && windows_fs::directory_children_limited(
                    &entry.handle,
                    entry.children.len().saturating_add(1),
                )
                .map_err(io)?
                    != entry.children
            {
                return Err(io("Copy security plan directory membership changed"));
            }
        }
        self.entries[0].policy.validate_parent(&self.parent)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parent(path: &std::path::Path) -> File {
        super::super::super::tests::parent(path)
    }
    fn observed(file: &File) -> Parts {
        let mut raw = capture(file).unwrap();
        parts(raw.as_mut_ptr().cast()).unwrap()
    }
    fn ordinary(parent: &File, name: &str, directory: bool) -> File {
        unsafe {
            windows_fs::open_relative(
                parent,
                OsStr::new(name),
                RelativeOptions {
                    directory: Some(directory),
                    create_new: true,
                    access: FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | WRITE_DAC,
                    sharing: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    security: ptr::null(),
                },
            )
        }
        .unwrap()
    }
    fn private(parent: &File, name: &str, directory: bool) -> File {
        windows_fs::create_private(
            parent,
            OsStr::new(name),
            directory,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
        .unwrap()
    }
    fn graph(parent: &File, private_tree: bool) -> Vec<File> {
        let make = |parent, name, directory| {
            if private_tree {
                private(parent, name, directory)
            } else {
                ordinary(parent, name, directory)
            }
        };
        let root = make(
            parent,
            if private_tree {
                "candidate"
            } else {
                "reference"
            },
            true,
        );
        let folder = make(&root, "folder", true);
        let file = make(&root, "file", false);
        let empty = make(&root, "empty", true);
        let leaf = make(&folder, "leaf", false);
        vec![root, folder, file, empty, leaf]
    }
    fn inputs(files: &[File]) -> Vec<SecurityEntry<'_>> {
        let parents = [None, Some(0), Some(0), Some(0), Some(1)];
        let names = ["candidate", "folder", "file", "empty", "leaf"];
        files
            .iter()
            .enumerate()
            .map(|(i, f)| SecurityEntry {
                parent: parents[i],
                name: names[i].into(),
                candidate: f,
                directory: matches!(i, 0 | 1 | 3),
            })
            .collect()
    }
    #[test]
    fn windows_native_copy_tree_security_projects_final_inheritance_against_kernel_trees() {
        for sddl in [
            "D:P(A;OICI;FA;;;OW)(A;OICI;FR;;;WD)",
            "D:P(A;;FA;;;OW)",
            "D:P(A;OICI;FA;;;CO)(A;;FA;;;OW)",
            "D:P(D;OICI;GW;;;BG)(A;OICI;FA;;;OW)",
            "D:P(A;OICINP;FA;;;OW)(A;;FA;;;OW)",
            "D:P(A;OI;FA;;;OW)(A;CI;FA;;;CO)(A;;FA;;;OW)",
        ] {
            let root = tempfile::tempdir().unwrap();
            let p = parent(root.path());
            super::super::super::tests::set_dacl(root.path(), sddl);
            let reference = graph(&p, false);
            let candidates = graph(&p, true);
            let before: Vec<_> = candidates.iter().map(observed).collect();
            let plan = TreeSecurityPlan::capture(&p, inputs(&candidates), || Ok(())).unwrap();
            assert_eq!(plan.entry_count(), 5);
            for (i, actual) in reference.iter().enumerate() {
                assert_eq!(
                    plan.entries[i].policy.expected,
                    observed(actual),
                    "index {i}, parent {sddl}"
                );
                assert_eq!(
                    observed(&candidates[i]),
                    before[i],
                    "planning applied private permissions"
                );
            }
            // This intentionally wrong policy derives from the still-private
            // root. The complete projection must avoid that source of inheritance.
            let wrong = DestinationSecurity::capture(&candidates[0], false).unwrap();
            if sddl.contains("NP") || sddl == "D:P(A;;FA;;;OW)" {
                assert_ne!(wrong.expected, plan.entries[2].policy.expected);
            }
            plan.validate().unwrap();
            let evidence = plan.evidence();
            assert_eq!(evidence.entries.len(), 5);
            assert_eq!(
                serde_json::to_value(&evidence).unwrap()["entries"][4]["parent"],
                1
            );
            assert_eq!(evidence.entries[4].final_security, observed(&reference[4]));
        }
    }
    #[test]
    fn windows_native_copy_tree_security_refuses_bad_graphs_and_cancels_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let p = parent(root.path());
        let files = graph(&p, true);
        let before: Vec<_> = files.iter().map(observed).collect();
        assert!(TreeSecurityPlan::capture(&p, Vec::<SecurityEntry<'_>>::new(), || Ok(())).is_err());
        for mode in 0..6 {
            let mut entries = inputs(&files);
            match mode {
                0 => entries[1].parent = None,
                1 => entries[1].parent = Some(99),
                2 => entries[1].directory = false,
                3 => entries[1].name = "..".into(),
                4 => {
                    entries[4].candidate = &files[2];
                }
                _ => {
                    entries.pop();
                }
            }
            assert!(
                TreeSecurityPlan::capture(&p, entries, || Ok(())).is_err(),
                "mode {mode}"
            );
        }
        let mut calls = 0;
        assert!(
            TreeSecurityPlan::capture(&p, inputs(&files), || {
                calls += 1;
                if calls == 3 {
                    Err(io("cancelled permission planning"))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(calls, 3);
        assert_eq!(files.iter().map(observed).collect::<Vec<_>>(), before);
        assert!(TreeSecurityPlan::capture(&p, inputs(&files), || Ok(())).is_ok());
    }
    #[test]
    fn windows_native_copy_tree_security_revalidates_parent_private_permissions_and_membership() {
        for mode in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let p = parent(root.path());
            let files = graph(&p, true);
            let plan = TreeSecurityPlan::capture(&p, inputs(&files), || Ok(())).unwrap();
            match mode {
                0 => super::super::super::tests::set_dacl(
                    root.path(),
                    "D:P(A;OICI;FA;;;OW)(A;OICI;FRFX;;;WD)",
                ),
                1 => super::super::super::tests::set_dacl(
                    &root.path().join("candidate/folder"),
                    "D:P(A;;FA;;;OW)",
                ),
                _ => std::fs::write(
                    root.path().join("candidate/folder/extra"),
                    b"unexpected data",
                )
                .unwrap(),
            }
            assert!(plan.validate().is_err(), "mode {mode}");
            if mode == 2 {
                assert_eq!(
                    std::fs::read(root.path().join("candidate/folder/extra")).unwrap(),
                    b"unexpected data"
                );
            }
        }
    }
    #[test]
    fn windows_native_copy_tree_security_link_policies_match_native_links_without_targets() {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        for directory in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let p = parent(root.path());
            super::super::super::tests::set_dacl(
                root.path(),
                "D:P(A;OI;FA;;;OW)(A;CI;FA;;;CO)(A;;FA;;;OW)(A;OICI;FR;;;WD)",
            );
            let missing = root.path().join("missing");
            let reference_path = root.path().join("reference");
            if directory {
                std::os::windows::fs::symlink_dir(&missing, &reference_path).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&missing, &reference_path).unwrap();
            }
            let reference = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&reference_path)
                .unwrap();
            let candidate = private(&p, "candidate", directory);
            super::super::super::links::capture(&reference)
                .unwrap()
                .unwrap()
                .install(&candidate)
                .unwrap();
            let before = observed(&candidate);
            let plan = TreeSecurityPlan::capture(
                &p,
                [SecurityEntry {
                    parent: None,
                    name: "candidate".into(),
                    candidate: &candidate,
                    directory,
                }],
                || Ok(()),
            )
            .unwrap();
            assert_eq!(plan.entries[0].policy.expected, observed(&reference));
            assert_eq!(observed(&candidate), before);
            assert!(!missing.exists());
            plan.validate().unwrap();
        }
    }
}
