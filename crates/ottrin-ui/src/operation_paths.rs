//! Queue-time lexical claims. This is scheduling, not native path authority.
use std::{
    collections::BTreeSet,
    ops::Bound,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OperationPath {
    pub(super) path: PathBuf,
    pub(super) writes: bool,
}
impl OperationPath {
    pub(super) fn write(path: PathBuf) -> Self {
        Self { path, writes: true }
    }
    pub(super) fn read(path: PathBuf) -> Self {
        Self {
            path,
            writes: false,
        }
    }
}

/// Borrowed only for one admission check; no cached ownership or filesystem state.
pub(super) struct OperationPathIndex<'a> {
    all: BTreeSet<&'a Path>,
    writes: BTreeSet<&'a Path>,
}
impl<'a> OperationPathIndex<'a> {
    pub(super) fn new(paths: &'a [OperationPath]) -> Self {
        Self {
            all: paths.iter().map(|claim| claim.path.as_path()).collect(),
            writes: paths
                .iter()
                .filter(|claim| claim.writes)
                .map(|claim| claim.path.as_path())
                .collect(),
        }
    }
    pub(super) fn conflicts(&self, held: &[OperationPath]) -> bool {
        held.iter().any(|claim| {
            let candidates = if claim.writes {
                &self.all
            } else {
                &self.writes
            };
            has_overlap(candidates, &claim.path)
        })
    }
}

fn has_overlap(paths: &BTreeSet<&Path>, path: &Path) -> bool {
    // Empty paths have no components and prefix every path, including absolutes.
    if paths.contains(Path::new("")) {
        return true;
    }
    if path.ancestors().any(|ancestor| paths.contains(ancestor)) {
        return true;
    }
    // Component ordering places a prefix before its contiguous descendants.
    paths
        .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
        .next()
        .is_some_and(|candidate| candidate.starts_with(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(held: &[OperationPath], requested: &[OperationPath]) -> bool {
        held.iter().any(|a| {
            requested.iter().any(|b| {
                (a.writes || b.writes)
                    && (a.path.starts_with(&b.path) || b.path.starts_with(&a.path))
            })
        })
    }
    fn samples() -> Vec<PathBuf> {
        [
            "",
            ".",
            "..",
            "a",
            "a/",
            "a/b",
            "a/b/c",
            "a/./b",
            "a//b",
            "a/..",
            "a-b",
            "../a",
            "/",
            "/a",
            "/a/b",
            "/a-b",
            "/b",
            r"C:\",
            r"C:\a",
            r"c:\a",
            r"C:\a\b",
            r"\\server\share",
            r"\\server\share\a",
            r"\\?\C:\a",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
    #[test]
    fn indexed_claims_match_component_prefix_reference() {
        let paths = samples();
        for a in &paths {
            for b in &paths {
                for aw in [false, true] {
                    for bw in [false, true] {
                        let held = [OperationPath {
                            path: a.clone(),
                            writes: aw,
                        }];
                        let requested = [OperationPath {
                            path: b.clone(),
                            writes: bw,
                        }];
                        assert_eq!(
                            OperationPathIndex::new(&requested).conflicts(&held),
                            reference(&held, &requested),
                            "{held:?} vs {requested:?}"
                        );
                    }
                }
            }
        }
        for offset in 0..paths.len() {
            let requested: Vec<_> = paths
                .iter()
                .enumerate()
                .filter(|(i, _)| (i + offset) % 4 != 0)
                .map(|(i, path)| OperationPath {
                    path: path.clone(),
                    writes: (i + offset) % 3 == 0,
                })
                .collect();
            for path in &paths {
                for writes in [false, true] {
                    let held = [OperationPath {
                        path: path.clone(),
                        writes,
                    }];
                    assert_eq!(
                        OperationPathIndex::new(&requested).conflicts(&held),
                        reference(&held, &requested)
                    );
                }
            }
        }
    }
    #[test]
    fn large_independent_batches_preserve_read_and_write_exclusion() {
        for writes in [false, true] {
            let held: Vec<_> = (0..20_000)
                .map(|i| {
                    if writes {
                        OperationPath::write(PathBuf::from(format!("/first/item-{i}")))
                    } else {
                        OperationPath::read(PathBuf::from(format!("/first/item-{i}")))
                    }
                })
                .collect();
            let mut requested: Vec<_> = (0..20_000)
                .map(|i| {
                    if writes {
                        OperationPath::write(PathBuf::from(format!("/second/item-{i}")))
                    } else {
                        OperationPath::read(PathBuf::from(format!("/second/item-{i}")))
                    }
                })
                .collect();
            assert!(!OperationPathIndex::new(&requested).conflicts(&held));
            requested[19_999] = OperationPath::write(held[19_999].path.clone());
            assert!(OperationPathIndex::new(&requested).conflicts(&held));
            requested[19_999] = OperationPath::write(PathBuf::from("/first"));
            assert!(OperationPathIndex::new(&requested).conflicts(&held));
        }
    }
}
