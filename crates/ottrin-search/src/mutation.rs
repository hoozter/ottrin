use ottrin_core::SearchResultItem;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Default)]
pub(crate) struct ActivationFence {
    epoch: Mutex<u64>,
}

impl ActivationFence {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn capture(&self) -> u64 {
        *self.epoch.lock().expect("activation fence")
    }

    pub(crate) fn invalidate(&self) -> u64 {
        let mut epoch = self.epoch.lock().expect("activation fence");
        *epoch = epoch.wrapping_add(1);
        *epoch
    }

    pub(crate) fn lock(&self) -> ActivationFenceGuard<'_> {
        ActivationFenceGuard(self.epoch.lock().expect("activation fence"))
    }
}

pub(crate) struct ActivationFenceGuard<'a>(MutexGuard<'a, u64>);

impl ActivationFenceGuard<'_> {
    pub(crate) fn matches(&self, expected_epoch: u64) -> bool {
        *self.0 == expected_epoch
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedItem {
    root: PathBuf,
    item: SearchResultItem,
}

impl OwnedItem {
    pub(crate) fn new(root: PathBuf, item: SearchResultItem) -> Self {
        Self { root, item }
    }

    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub(crate) fn item(&self) -> &SearchResultItem {
        &self.item
    }
}

#[derive(Debug)]
pub(crate) enum IndexMutation {
    ReplaceAll(Vec<OwnedItem>),
    ReplaceRoot {
        root: PathBuf,
        items: Vec<OwnedItem>,
    },
    Delta {
        upserts: Vec<OwnedItem>,
        remove_trees: Vec<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenerationSnapshot {
    generation: i64,
    plan_identity: String,
    items: Vec<OwnedItem>,
}

impl GenerationSnapshot {
    pub(crate) fn new(generation: i64, plan_identity: String, items: Vec<OwnedItem>) -> Self {
        Self {
            generation,
            plan_identity,
            items: normalized(items),
        }
    }

    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }

    pub(crate) fn plan_identity(&self) -> &str {
        &self.plan_identity
    }

    pub(crate) fn items(&self) -> &[OwnedItem] {
        &self.items
    }

    pub(crate) fn apply(self, mutation: IndexMutation) -> Self {
        let items = match mutation {
            IndexMutation::ReplaceAll(items) => items,
            IndexMutation::ReplaceRoot { root, items } => self
                .items
                .into_iter()
                .filter(|owned| owned.root != root)
                .chain(items)
                .collect(),
            IndexMutation::Delta {
                upserts,
                remove_trees,
            } => self
                .items
                .into_iter()
                .filter(|owned| {
                    !remove_trees
                        .iter()
                        .any(|removed| owned.item.path.starts_with(removed))
                })
                .chain(upserts)
                .collect(),
        };
        Self::new(self.generation, self.plan_identity, items)
    }
}

fn normalized(items: Vec<OwnedItem>) -> Vec<OwnedItem> {
    items
        .into_iter()
        .map(|owned| (owned.item.path.clone(), owned))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{EntryKind, SearchResultItem};
    use std::path::{Path, PathBuf};

    fn item(path: PathBuf, size: u64) -> SearchResultItem {
        let mut item = SearchResultItem {
            parent_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
            kind: EntryKind::File,
            size_bytes: Some(size),
            modified_unix_secs: Some(1),
            is_executable: false,
            symlink_target_is_dir: None,
            content_snippet: None,
            name_lower: String::new(),
            path_str: String::new(),
            path_lower: String::new(),
        };
        item.prepare();
        item
    }

    #[test]
    fn replace_root_only_changes_items_owned_by_named_root() {
        let parent = PathBuf::from("root");
        let nested = parent.join("nested");
        let parent_item = OwnedItem::new(parent.clone(), item(parent.join("parent.txt"), 1));
        let nested_item = OwnedItem::new(nested.clone(), item(nested.join("old.txt"), 2));
        let snapshot = GenerationSnapshot::new(
            7,
            "plan".to_string(),
            vec![parent_item.clone(), nested_item],
        );
        let replacement = OwnedItem::new(nested.clone(), item(nested.join("new.txt"), 3));

        let changed = snapshot.apply(IndexMutation::ReplaceRoot {
            root: nested,
            items: vec![replacement.clone()],
        });

        assert_eq!(changed.items(), &[replacement, parent_item]);
    }

    #[test]
    fn delta_is_deterministic_for_upserts_and_recursive_removals() {
        let root = PathBuf::from("root");
        let tree = root.join("tree");
        let retained = OwnedItem::new(root.clone(), item(root.join("retained.txt"), 1));
        let removed = OwnedItem::new(root.clone(), item(tree.join("child.txt"), 2));
        let snapshot =
            GenerationSnapshot::new(1, "plan".to_string(), vec![removed, retained.clone()]);
        let updated = OwnedItem::new(root.clone(), item(root.join("retained.txt"), 4));
        let added = OwnedItem::new(root, item(PathBuf::from("root").join("added.txt"), 3));

        let changed = snapshot.apply(IndexMutation::Delta {
            upserts: vec![updated.clone(), added.clone()],
            remove_trees: vec![tree],
        });

        assert_eq!(changed.items(), &[added, updated]);
        assert_ne!(changed.items(), &[retained]);
    }
}
