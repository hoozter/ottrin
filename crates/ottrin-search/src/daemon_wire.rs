//! Native-unit daemon payloads. Persistence keeps its own existing representation.
//! Remote derives construct the original types, so added fields fail compilation
//! until their wire representation is explicitly accounted for here.
use crate::SearchQueryResult;
use ottrin_core::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(remote = "SearchQuery")]
pub(super) struct SearchQueryWire {
    text: String,
    scope: SearchScope,
    #[serde(with = "ottrin_core::native_path::optional")]
    root_path: Option<PathBuf>,
    include_hidden_system: bool,
    sort: SearchSort,
    limit: usize,
    offset: usize,
    /// When true, search file contents via ripgrep in addition to filenames.
    #[serde(default)]
    content_search: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "SearchConfig")]
pub(super) struct SearchConfigWire {
    default_scope: SearchScope,
    include_hidden_system: bool,
    #[serde(with = "ottrin_core::native_path::list")]
    include_roots: Vec<PathBuf>,
    /// Explicit opt-ins for discovered volumes. These retain user intent while a
    /// removable or mapped location is unavailable; only currently discovered
    /// paths are supplied to the running indexer.
    #[serde(with = "volume_list")]
    indexed_volumes: Vec<IndexedVolumeLocation>,
    /// Runtime-only paths for currently available opted-in volumes.
    #[serde(with = "ottrin_core::native_path::list")]
    active_indexed_volume_roots: Vec<PathBuf>,
    #[serde(with = "ottrin_core::native_path::list")]
    exclude_roots: Vec<PathBuf>,
    /// Optional: only index file content for these roots when enabled.
    content_indexing_enabled: bool,
    #[serde(with = "ottrin_core::native_path::list")]
    content_include_roots: Vec<PathBuf>,
    #[serde(with = "ottrin_core::native_path::list")]
    content_exclude_roots: Vec<PathBuf>,
    /// Supported file groups whose bodies may be extracted.
    content_file_groups: Vec<ContentFileGroup>,
    /// Maximum admitted size for one file body during content extraction.
    content_max_file_size_bytes: u64,
    include_globs: Vec<String>,
    exclude_globs: Vec<String>,
    default_sort: SearchSort,
    result_limit: usize,
    /// How often (in hours) to automatically rebuild the index in the background.
    /// 0 = only rebuild on file-change events / manual trigger.
    /// Startup loads the saved SQLite index when available.
    refresh_interval_hours: u32,
    /// Supplement results with the system locate/plocate database when available.
    use_system_locate: bool,
    /// Let Ottrin manage its own mlocate database (~/.cache/ottrin/locate.db).
    /// updatedb is run at startup and on the schedule below. No root required
    /// for the user's own directories. Opt-in.
    manage_locate_db: bool,
    /// How often (hours) to re-run updatedb for the managed locate database.
    /// 0 = first launch only.
    locate_update_hours: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "IndexedVolumeLocation")]
struct IndexedVolumeLocationWire {
    /// Platform-provided identity. It must not depend on the display label or
    /// current mount point.
    identity: String,
    /// Last observed root, retained for inspection and updated on rediscovery.
    #[serde(with = "native_single")]
    last_known_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "SearchResultItem")]
struct SearchResultItemWire {
    #[serde(with = "native_single")]
    path: PathBuf,
    #[serde(with = "native_single")]
    parent_path: PathBuf,
    name: String,
    kind: EntryKind,
    #[serde(default)]
    is_executable: bool,
    #[serde(default)]
    symlink_target_is_dir: Option<bool>,
    size_bytes: Option<u64>,
    modified_unix_secs: Option<u64>,
    /// For content search results: a short snippet of the matching line.
    #[serde(default)]
    content_snippet: Option<String>,
    /// Pre-computed lowercase name — populated at index-build time to avoid
    /// per-query allocations in the hot query path.
    #[serde(skip, default)]
    name_lower: String,
    /// Pre-computed lossy path string — populated at index-build time.
    #[serde(skip, default)]
    path_str: String,
    /// Pre-computed lowercase lossy path string — avoids per-query allocation.
    #[serde(skip, default)]
    path_lower: String,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "SearchResponse")]
struct SearchResponseWire {
    #[serde(with = "item_list")]
    items: Vec<SearchResultItem>,
    total: usize,
    status: SearchIndexStatus,
    error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "SearchQueryResult")]
pub(super) struct SearchQueryResultWire {
    #[serde(with = "SearchResponseWire")]
    response: SearchResponse,
    /// Non-fatal information about intentionally excluded results.
    #[serde(default)]
    warning: Option<String>,
}

mod native_single {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        path: &std::path::Path,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        native_path::NativePath::encode(path).serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        native_path::NativePath::deserialize(deserializer)?
            .decode()
            .map_err(serde::de::Error::custom)
    }
}
mod volume_list {
    use super::*;
    use serde::{Deserializer, Serializer};
    #[derive(Serialize)]
    struct Ref<'a>(#[serde(with = "IndexedVolumeLocationWire")] &'a IndexedVolumeLocation);
    #[derive(Deserialize)]
    struct Owned(#[serde(with = "IndexedVolumeLocationWire")] IndexedVolumeLocation);
    pub fn serialize<S: Serializer>(
        items: &[IndexedVolumeLocation],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        items
            .iter()
            .map(Ref)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<IndexedVolumeLocation>, D::Error> {
        Ok(Vec::<Owned>::deserialize(deserializer)?
            .into_iter()
            .map(|item| item.0)
            .collect())
    }
}
mod item_list {
    use super::*;
    use serde::{Deserializer, Serializer};
    #[derive(Serialize)]
    struct Ref<'a>(#[serde(with = "SearchResultItemWire")] &'a SearchResultItem);
    #[derive(Deserialize)]
    struct Owned(#[serde(with = "SearchResultItemWire")] SearchResultItem);
    pub fn serialize<S: Serializer>(
        items: &[SearchResultItem],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        items
            .iter()
            .map(Ref)
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<SearchResultItem>, D::Error> {
        Ok(Vec::<Owned>::deserialize(deserializer)?
            .into_iter()
            .map(|item| item.0)
            .collect())
    }
}

/// Machine-readable search output shares the daemon's native-path contract.
pub fn encode_search_response(response: &SearchResponse) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct Output<'a>(#[serde(with = "SearchResponseWire")] &'a SearchResponse);
    serde_json::to_string_pretty(&Output(response))
}
