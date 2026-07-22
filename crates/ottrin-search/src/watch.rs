use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RootState {
    Uncovered { retry_at: Duration },
    Healthy,
    RecoveryNeeded { retry_at: Duration },
    Recovering,
}

#[derive(Debug)]
struct RootWatch {
    state: RootState,
    failures: u32,
}

/// Exact per-root watcher state. Time is supplied by the worker so retries are deterministic.
#[derive(Debug)]
pub(crate) struct WatchRegistry {
    roots: BTreeMap<PathBuf, RootWatch>,
    initial_backoff: Duration,
    maximum_backoff: Duration,
    last_error: Option<String>,
}

impl WatchRegistry {
    pub(crate) fn new(
        roots: Vec<PathBuf>,
        initial_backoff: Duration,
        maximum_backoff: Duration,
    ) -> Self {
        Self {
            roots: roots
                .into_iter()
                .map(|root| {
                    (
                        root,
                        RootWatch {
                            state: RootState::Uncovered {
                                retry_at: Duration::ZERO,
                            },
                            failures: 0,
                        },
                    )
                })
                .collect(),
            initial_backoff,
            maximum_backoff: maximum_backoff.max(initial_backoff),
            last_error: None,
        }
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = &Path> {
        self.roots.keys().map(PathBuf::as_path)
    }

    pub(crate) fn owning_root(&self, path: &Path) -> Option<&Path> {
        self.roots
            .keys()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .map(PathBuf::as_path)
    }

    pub(crate) fn record_watch_success(&mut self, root: &Path) {
        if let Some(entry) = self.roots.get_mut(root) {
            entry.state = RootState::Healthy;
            entry.failures = 0;
        }
    }

    pub(crate) fn record_retry_watch_success(&mut self, root: &Path) {
        if let Some(entry) = self.roots.get_mut(root) {
            entry.state = RootState::RecoveryNeeded {
                retry_at: Duration::ZERO,
            };
            entry.failures = 0;
        }
    }

    pub(crate) fn record_watch_failure(&mut self, root: &Path, now: Duration, error: &str) {
        let initial = self.initial_backoff;
        let maximum = self.maximum_backoff;
        if let Some(entry) = self.roots.get_mut(root) {
            let shift = entry.failures.min(31);
            let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
            let delay = initial.saturating_mul(multiplier).min(maximum);
            entry.failures = entry.failures.saturating_add(1);
            entry.state = RootState::Uncovered {
                retry_at: now.saturating_add(delay),
            };
            self.last_error = Some(error.to_string());
        }
    }

    pub(crate) fn due_retries(&self, now: Duration) -> Vec<PathBuf> {
        self.roots
            .iter()
            .filter_map(|(root, entry)| match entry.state {
                RootState::Uncovered { retry_at } if retry_at <= now => Some(root.clone()),
                _ => None,
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn retry_deadline(&self, root: &Path) -> Option<Duration> {
        match self.roots.get(root)?.state {
            RootState::Uncovered { retry_at } => Some(retry_at),
            _ => None,
        }
    }

    pub(crate) fn mark_paths_uncertain(&mut self, paths: &[PathBuf], error: &str) {
        let affected = if paths.is_empty() {
            self.roots.keys().cloned().collect()
        } else {
            paths
                .iter()
                .filter_map(|path| self.owning_root(path).map(Path::to_path_buf))
                .collect::<BTreeSet<_>>()
        };
        for root in affected {
            if let Some(entry) = self.roots.get_mut(&root)
                && !matches!(entry.state, RootState::Uncovered { .. })
            {
                entry.state = RootState::RecoveryNeeded {
                    retry_at: Duration::ZERO,
                };
            }
        }
        self.last_error = Some(error.to_string());
    }

    pub(crate) fn begin_recovery(&mut self, root: &Path) {
        if let Some(entry) = self.roots.get_mut(root)
            && matches!(entry.state, RootState::RecoveryNeeded { .. })
        {
            entry.state = RootState::Recovering;
        }
    }

    pub(crate) fn complete_recovery(&mut self, root: &Path) {
        if let Some(entry) = self.roots.get_mut(root)
            && matches!(entry.state, RootState::Recovering)
        {
            entry.state = RootState::Healthy;
            entry.failures = 0;
        }
    }

    pub(crate) fn record_recovery_failure(&mut self, root: &Path, now: Duration, error: &str) {
        let initial = self.initial_backoff;
        let maximum = self.maximum_backoff;
        if let Some(entry) = self.roots.get_mut(root) {
            let shift = entry.failures.min(31);
            let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
            let delay = initial.saturating_mul(multiplier).min(maximum);
            entry.failures = entry.failures.saturating_add(1);
            entry.state = RootState::RecoveryNeeded {
                retry_at: now.saturating_add(delay),
            };
            self.last_error = Some(error.to_string());
        }
    }

    #[cfg(test)]
    pub(crate) fn needs_recovery(&self, root: &Path) -> bool {
        self.roots.get(root).is_some_and(|entry| {
            matches!(
                entry.state,
                RootState::RecoveryNeeded { .. } | RootState::Recovering
            )
        })
    }

    pub(crate) fn recovery_roots(&self, now: Duration) -> Vec<PathBuf> {
        self.roots
            .iter()
            .filter(|(_, entry)| {
                matches!(entry.state, RootState::RecoveryNeeded { retry_at } if retry_at <= now)
            })
            .map(|(root, _)| root.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn is_healthy(&self, root: &Path) -> bool {
        self.roots
            .get(root)
            .is_some_and(|entry| matches!(entry.state, RootState::Healthy))
    }

    pub(crate) fn is_watched(&self, root: &Path) -> bool {
        self.roots.get(root).is_some_and(|entry| {
            matches!(
                entry.state,
                RootState::Healthy | RootState::RecoveryNeeded { .. } | RootState::Recovering
            )
        })
    }

    pub(crate) fn watched_roots(&self) -> BTreeSet<PathBuf> {
        self.roots
            .keys()
            .filter(|root| self.is_watched(root))
            .cloned()
            .collect()
    }

    pub(crate) fn ready_with(&self, _exists: impl Fn(&Path) -> bool) -> bool {
        self.roots
            .values()
            .all(|entry| matches!(entry.state, RootState::Healthy))
    }

    pub(crate) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("root-a"), PathBuf::from("root-b")]
    }

    #[test]
    fn failed_root_retries_without_disturbing_healthy_roots() {
        let mut registry =
            WatchRegistry::new(roots(), Duration::from_secs(1), Duration::from_secs(8));
        registry.record_watch_success(Path::new("root-a"));
        registry.record_watch_failure(Path::new("root-b"), Duration::ZERO, "backend refused");

        assert_eq!(
            registry.watched_roots(),
            BTreeSet::from([PathBuf::from("root-a")])
        );
        assert_eq!(
            registry.due_retries(Duration::from_millis(999)),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            registry.due_retries(Duration::from_secs(1)),
            vec![PathBuf::from("root-b")]
        );
        assert!(registry.is_watched(Path::new("root-a")));
    }

    #[test]
    fn ready_is_false_while_any_configured_root_is_uncovered() {
        let mut registry =
            WatchRegistry::new(roots(), Duration::from_secs(1), Duration::from_secs(8));
        registry.record_watch_success(Path::new("root-a"));
        assert!(!registry.ready_with(|_| true));
        assert!(!registry.ready_with(|root| root == Path::new("root-a")));
    }

    #[test]
    fn successful_retry_requires_rescan_before_root_becomes_healthy() {
        let mut registry =
            WatchRegistry::new(roots(), Duration::from_secs(1), Duration::from_secs(8));
        registry.record_watch_failure(Path::new("root-b"), Duration::ZERO, "failed");
        registry.record_retry_watch_success(Path::new("root-b"));

        assert!(registry.needs_recovery(Path::new("root-b")));
        assert!(!registry.is_healthy(Path::new("root-b")));
        registry.begin_recovery(Path::new("root-b"));
        registry.complete_recovery(Path::new("root-b"));
        assert!(registry.is_healthy(Path::new("root-b")));
    }

    #[test]
    fn backend_error_marks_only_affected_roots_uncertain() {
        let mut registry =
            WatchRegistry::new(roots(), Duration::from_secs(1), Duration::from_secs(8));
        registry.record_watch_success(Path::new("root-a"));
        registry.record_watch_success(Path::new("root-b"));
        registry.mark_paths_uncertain(
            &[PathBuf::from("root-b/child/file.txt")],
            "event stream error",
        );

        assert!(registry.is_healthy(Path::new("root-a")));
        assert!(registry.needs_recovery(Path::new("root-b")));
        assert_eq!(registry.last_error(), Some("event stream error"));
    }

    #[test]
    fn retry_backoff_is_bounded() {
        let mut registry = WatchRegistry::new(
            vec![PathBuf::from("root")],
            Duration::from_secs(1),
            Duration::from_secs(4),
        );
        let expected = [1, 2, 4, 4, 4];
        let mut now = Duration::ZERO;
        for delay in expected {
            registry.record_watch_failure(Path::new("root"), now, "failed");
            let deadline = registry
                .retry_deadline(Path::new("root"))
                .expect("retry deadline");
            assert_eq!(deadline, now + Duration::from_secs(delay));
            now = deadline;
        }
    }

    #[test]
    fn failed_recovery_returns_to_bounded_retry_instead_of_sticking_recovering() {
        let root = PathBuf::from("root");
        let mut registry = WatchRegistry::new(
            vec![root.clone()],
            Duration::from_secs(1),
            Duration::from_secs(4),
        );
        registry.record_watch_success(&root);
        registry.mark_paths_uncertain(&[root.join("child")], "stream uncertain");
        registry.begin_recovery(&root);
        registry.record_recovery_failure(&root, Duration::ZERO, "scan failed");

        assert!(registry.needs_recovery(&root));
        assert!(
            registry
                .recovery_roots(Duration::from_millis(999))
                .is_empty()
        );
        assert_eq!(registry.recovery_roots(Duration::from_secs(1)), vec![root]);
    }

    #[test]
    fn owning_root_uses_component_aware_most_specific_match() {
        let registry = WatchRegistry::new(
            vec![PathBuf::from("root"), PathBuf::from("root/nested")],
            Duration::from_secs(1),
            Duration::from_secs(8),
        );
        assert_eq!(
            registry.owning_root(Path::new("root/nested/file")),
            Some(Path::new("root/nested"))
        );
        assert_eq!(registry.owning_root(Path::new("rootish/file")), None);
    }
}
