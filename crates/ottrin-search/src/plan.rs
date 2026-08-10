use globset::{Glob, GlobSet, GlobSetBuilder};
use ottrin_core::SearchConfig;
use std::fmt::Write;
use std::path::{Path, PathBuf};

const FILTER_POLICY_VERSION: u32 = 3;
const CONTENT_POLICY_VERSION: u32 = 1;
const FOLLOW_LINKS: bool = false;
const SAME_FILE_SYSTEM: bool = true;

/// An immutable, fully resolved description of one exact index scan.
///
/// Roots are resolved and globs are compiled at construction time. Every path
/// membership decision for that scan is made through this value.
#[derive(Debug)]
pub(crate) struct ScanPlan {
    roots: Vec<PathBuf>,
    excluded_roots: Vec<PathBuf>,
    include_hidden_system: bool,
    content_indexing_enabled: bool,
    content_include_roots: Vec<PathBuf>,
    content_exclude_roots: Vec<PathBuf>,

    include: GlobSet,
    exclude: GlobSet,
    identity: String,
}

impl ScanPlan {
    pub(crate) fn new(config: &SearchConfig) -> Result<Self, String> {
        Self::new_with_root_resolver(config, || crate::effective_roots(config))
    }

    fn new_with_root_resolver(
        config: &SearchConfig,
        resolve_roots: impl FnOnce() -> Vec<PathBuf>,
    ) -> Result<Self, String> {
        let mut roots = resolve_roots();
        dedup_paths(&mut roots);
        let mut excluded_roots = config.exclude_roots.clone();
        dedup_paths(&mut excluded_roots);
        let mut content_include_roots = config.content_include_roots.clone();
        dedup_paths(&mut content_include_roots);
        let mut content_exclude_roots = config.content_exclude_roots.clone();
        dedup_paths(&mut content_exclude_roots);
        let mut include_globs = normalized_patterns(&config.include_globs);
        let mut exclude_globs = normalized_patterns(&config.exclude_globs);
        let include = build_globset(&include_globs)?;
        let exclude = build_globset(&exclude_globs)?;
        include_globs.sort();
        include_globs.dedup();
        exclude_globs.sort();
        exclude_globs.dedup();

        let identity = canonical_identity(
            &roots,
            &excluded_roots,
            config.include_hidden_system,
            ContentPolicy {
                enabled: config.content_indexing_enabled,
                include_roots: &content_include_roots,
                exclude_roots: &content_exclude_roots,
            },
            &include_globs,
            &exclude_globs,
        );
        Ok(Self {
            roots,
            excluded_roots,
            include_hidden_system: config.include_hidden_system,
            content_indexing_enabled: config.content_indexing_enabled,
            content_include_roots,
            content_exclude_roots,
            include,
            exclude,
            identity,
        })
    }

    pub(crate) fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub(crate) fn identity(&self) -> &str {
        &self.identity
    }

    pub(crate) fn follow_links(&self) -> bool {
        FOLLOW_LINKS
    }

    pub(crate) fn same_file_system(&self) -> bool {
        SAME_FILE_SYSTEM
    }

    pub(crate) fn root_for(&self, path: &Path) -> Option<&Path> {
        self.roots
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .map(PathBuf::as_path)
    }

    #[cfg(test)]
    pub(crate) fn includes_path(&self, path: &Path) -> bool {
        self.root_for(path)
            .is_some_and(|root| self.includes_path_for_root(path, root))
    }

    pub(crate) fn includes_path_for_root(&self, path: &Path, root: &Path) -> bool {
        if self.root_for(path) != Some(root) {
            return false;
        }
        if path == root {
            return true;
        }
        if self
            .excluded_roots
            .iter()
            .any(|excluded| path.starts_with(excluded))
        {
            return false;
        }
        if !self.include_hidden_system && crate::path_has_hidden_component(path, root) {
            return false;
        }
        if crate::path_has_builtin_excluded_component(path, root) {
            return false;
        }
        let relative = crate::normalized_rel_path(path, root);
        !self.exclude.is_match(&relative) || self.include.is_match(&relative)
    }

    pub(crate) fn includes_item_name(&self, name: &str) -> bool {
        self.include_hidden_system || !name.starts_with('.')
    }

    pub(crate) fn indexes_content_for(&self, path: &Path) -> bool {
        self.content_indexing_enabled
            && self
                .content_include_roots
                .iter()
                .any(|root| path.starts_with(root))
            && !self
                .content_exclude_roots
                .iter()
                .any(|root| path.starts_with(root))
    }
}

fn normalized_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .filter(|pattern| !pattern.trim().is_empty())
        .cloned()
        .collect()
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = Vec::with_capacity(paths.len());
    paths.retain(|path| {
        if seen.contains(path) {
            false
        } else {
            seen.push(path.clone());
            true
        }
    });
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            Glob::new(pattern).map_err(|error| format!("Invalid glob '{}': {}", pattern, error))?,
        );
    }
    builder.build().map_err(|error| error.to_string())
}

struct ContentPolicy<'a> {
    enabled: bool,
    include_roots: &'a [PathBuf],
    exclude_roots: &'a [PathBuf],
}

fn canonical_identity(
    roots: &[PathBuf],
    excluded_roots: &[PathBuf],
    include_hidden_system: bool,
    content_policy: ContentPolicy<'_>,
    include_globs: &[String],
    exclude_globs: &[String],
) -> String {
    let mut canonical_roots = roots.to_vec();
    canonical_roots.sort();
    let mut canonical_excluded_roots = excluded_roots.to_vec();
    canonical_excluded_roots.sort();
    let mut canonical_content_include_roots = content_policy.include_roots.to_vec();
    canonical_content_include_roots.sort();
    let mut canonical_content_exclude_roots = content_policy.exclude_roots.to_vec();
    canonical_content_exclude_roots.sort();
    let mut identity = String::new();
    write!(
        identity,
        "descriptor=2;filter={FILTER_POLICY_VERSION};content-policy={CONTENT_POLICY_VERSION};encoding={PATH_ENCODING};hidden={include_hidden_system};content={};follow={FOLLOW_LINKS};samefs={SAME_FILE_SYSTEM};",
        content_policy.enabled,
    )
    .expect("writing to String cannot fail");
    append_identity_list(
        &mut identity,
        "roots",
        canonical_roots.iter().map(|path| encode_path(path)),
    );
    append_identity_list(
        &mut identity,
        "excluded",
        canonical_excluded_roots
            .iter()
            .map(|path| encode_path(path)),
    );
    append_identity_list(&mut identity, "include", include_globs.iter().cloned());
    append_identity_list(&mut identity, "exclude", exclude_globs.iter().cloned());
    append_identity_list(
        &mut identity,
        "content-include",
        canonical_content_include_roots
            .iter()
            .map(|path| encode_path(path)),
    );
    append_identity_list(
        &mut identity,
        "content-exclude",
        canonical_content_exclude_roots
            .iter()
            .map(|path| encode_path(path)),
    );
    identity
}

fn append_identity_list(
    identity: &mut String,
    label: &str,
    values: impl IntoIterator<Item = String>,
) {
    write!(identity, "{label}=").expect("writing to String cannot fail");
    for value in values {
        write!(identity, "{}:{value}", value.len()).expect("writing to String cannot fail");
    }
    identity.push(';');
}

#[cfg(unix)]
const PATH_ENCODING: &str = "unix-bytes-hex";

#[cfg(windows)]
const PATH_ENCODING: &str = "windows-utf16-hex";

#[cfg(not(any(unix, windows)))]
const PATH_ENCODING: &str = "lossy-unicode";

#[cfg(unix)]
fn encode_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    hex(path.as_os_str().as_bytes())
}

#[cfg(windows)]
fn encode_path(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .map(|unit| format!("{unit:04x}"))
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn encode_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(unix)]
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            write!(out, "{byte:02x}").expect("writing to String cannot fail");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchService;
    use ottrin_core::{SearchConfig, SearchIndexStatus, SearchScope, SearchSort};
    use std::cell::Cell;
    use std::path::{Path, PathBuf};

    fn config() -> SearchConfig {
        SearchConfig {
            include_roots: vec![PathBuf::from("/scan")],
            exclude_roots: vec![PathBuf::from("/scan/excluded")],
            include_globs: vec!["keep/**".to_string()],
            exclude_globs: vec!["*.tmp".to_string()],
            ..SearchConfig::default()
        }
    }

    #[test]
    fn identity_invalidates_indexes_created_before_content_policy() {
        let identity = ScanPlan::new(&config()).expect("valid plan");
        assert!(identity.identity().contains("filter=3;"));
        assert!(identity.identity().contains("content-policy=1;"));
        assert!(identity.identity().contains("content=false;"));
    }

    #[test]
    fn identity_ignores_query_and_schedule_settings() {
        let cfg = config();
        let expected = ScanPlan::new(&cfg)
            .expect("valid plan")
            .identity()
            .to_string();
        let mut changed = cfg.clone();
        changed.default_scope = SearchScope::CurrentFolder;
        changed.default_sort = SearchSort::Size;
        changed.result_limit += 1;
        changed.refresh_interval_hours += 1;
        changed.use_system_locate = !changed.use_system_locate;
        changed.manage_locate_db = !changed.manage_locate_db;
        changed.locate_update_hours += 1;

        assert_eq!(
            ScanPlan::new(&changed).expect("valid plan").identity(),
            expected
        );
    }

    #[test]
    fn identity_changes_for_every_membership_setting() {
        let cfg = config();
        let expected = ScanPlan::new(&cfg)
            .expect("valid plan")
            .identity()
            .to_string();
        let variants = [
            SearchConfig {
                include_roots: vec![PathBuf::from("/other")],
                ..cfg.clone()
            },
            SearchConfig {
                exclude_roots: vec![PathBuf::from("/scan/other")],
                ..cfg.clone()
            },
            SearchConfig {
                include_hidden_system: !cfg.include_hidden_system,
                ..cfg.clone()
            },
            SearchConfig {
                include_globs: vec!["other/**".to_string()],
                ..cfg.clone()
            },
            SearchConfig {
                exclude_globs: vec!["*.bak".to_string()],
                ..cfg.clone()
            },
        ];

        for variant in variants {
            assert_ne!(
                ScanPlan::new(&variant).expect("valid plan").identity(),
                expected
            );
        }
    }

    #[test]
    fn roots_are_resolved_once_and_frozen() {
        let calls = Cell::new(0);
        let plan = ScanPlan::new_with_root_resolver(&config(), || {
            calls.set(calls.get() + 1);
            vec![
                PathBuf::from("/resolved-z"),
                PathBuf::from("/resolved-a"),
                PathBuf::from("/resolved-z"),
            ]
        })
        .expect("valid plan");

        assert_eq!(
            plan.roots(),
            &[PathBuf::from("/resolved-z"), PathBuf::from("/resolved-a")]
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn invalid_startup_plan_recovers_after_config_is_corrected() {
        let _guard = crate::SEARCH_SERVICE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = std::env::temp_dir().join(format!(
            "ottrin-plan-recovery-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&root).expect("create plan recovery root");
        let mut service = SearchService::new(SearchConfig {
            include_roots: vec![root.clone()],
            include_globs: vec!["[".to_string()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        });

        service.start();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline
            && !service
                .diagnostics()
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("Invalid glob"))
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let invalid = service.diagnostics();
        assert!(matches!(invalid.status, SearchIndexStatus::Unavailable));
        assert!(
            invalid
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("Invalid glob"))
        );
        assert!(service.worker.is_some());
        service.update_config(SearchConfig {
            include_roots: vec![root.clone()],
            use_system_locate: false,
            manage_locate_db: false,
            refresh_interval_hours: 0,
            ..SearchConfig::default()
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline
            && !matches!(service.diagnostics().status, SearchIndexStatus::Ready)
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let recovered = service.diagnostics();
        assert!(
            matches!(recovered.status, SearchIndexStatus::Ready),
            "recovery diagnostics: {recovered:?}"
        );
        service.stop();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overlapping_paths_are_owned_by_the_most_specific_root() {
        let plan = ScanPlan::new(&SearchConfig {
            include_roots: vec![PathBuf::from("/scan"), PathBuf::from("/scan/nested")],
            exclude_roots: Vec::new(),
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            include_hidden_system: true,
            ..SearchConfig::default()
        })
        .expect("valid plan");

        assert_eq!(
            plan.root_for(Path::new("/scan/nested/file.txt")),
            Some(Path::new("/scan/nested"))
        );
        assert!(
            !plan.includes_path_for_root(Path::new("/scan/nested/file.txt"), Path::new("/scan"))
        );
    }

    #[test]
    fn membership_filtering_is_identical_for_owned_and_direct_checks() {
        let plan = ScanPlan::new(&config()).expect("valid plan");
        let paths = [
            "/scan/visible.txt",
            "/scan/file.tmp",
            "/scan/keep/file.tmp",
            "/scan/excluded/file.txt",
            "/outside/file.txt",
        ];

        for path in paths.map(Path::new) {
            let through_owner = plan
                .root_for(path)
                .is_some_and(|root| plan.includes_path_for_root(path, root));
            assert_eq!(
                plan.includes_path(path),
                through_owner,
                "{}",
                path.display()
            );
        }
    }
}
