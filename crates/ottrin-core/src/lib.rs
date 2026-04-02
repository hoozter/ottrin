use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePreset {
    #[default]
    Ottrin,
    Breeze,
    Adwaita,
    Windows11,
    Solarized,
    Nord,
    G33k,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeCustomization {
    pub enabled: bool,
    pub background: [u8; 4],
    pub panel: [u8; 4],
    pub toolbar: [u8; 4],
    pub bookmarks_bar: [u8; 4],
    pub smart_panel: [u8; 4],
    pub titlebar: [u8; 4],
    pub border: [u8; 4],
    pub window_border: [u8; 4],
    pub window_border_width: f32,
    pub accent: [u8; 4],
    pub folder: [u8; 4],
    pub button_bg: [u8; 4],
    pub button_text: [u8; 4],
    pub text_heading: [u8; 4],
    pub text_folder: [u8; 4],
    pub text_file: [u8; 4],
    // Semantic palette tokens (VS Code/Seti inspired) used by file styling.
    pub palette_folder_blue: [u8; 4],
    pub palette_link_blue: [u8; 4],
    pub palette_steel: [u8; 4],
    pub palette_soft_white: [u8; 4],
    pub palette_green: [u8; 4],
    pub palette_orange: [u8; 4],
    pub palette_purple: [u8; 4],
    pub palette_pink: [u8; 4],
    pub palette_red: [u8; 4],
    pub palette_yellow: [u8; 4],
    pub app_radius: f32,
    pub border_radius: f32,
    pub button_radius: f32,
    pub font_scale: f32,
}

impl Default for ThemeCustomization {
    fn default() -> Self {
        Self {
            enabled: false,
            background: [17, 18, 20, 255],
            panel: [23, 24, 27, 255],
            toolbar: [23, 24, 27, 255],
            bookmarks_bar: [17, 18, 20, 255],
            smart_panel: [23, 24, 27, 255],
            titlebar: [30, 32, 35, 255],
            border: [52, 55, 60, 255],
            window_border: [52, 55, 60, 255],
            window_border_width: 1.0,
            accent: [92, 132, 196, 255],
            folder: [92, 132, 196, 255],
            button_bg: [30, 32, 35, 255],
            button_text: [188, 191, 196, 255],
            text_heading: [232, 232, 232, 255],
            text_folder: [224, 226, 230, 255],
            text_file: [170, 172, 176, 255],
            palette_folder_blue: [81, 154, 186, 255],
            palette_link_blue: [111, 168, 195, 255],
            palette_steel: [109, 128, 134, 255],
            palette_soft_white: [232, 236, 235, 255],
            palette_green: [141, 193, 73, 255],
            palette_orange: [227, 121, 51, 255],
            palette_purple: [160, 116, 196, 255],
            palette_pink: [245, 83, 133, 255],
            palette_red: [204, 62, 68, 255],
            palette_yellow: [203, 203, 65, 255],
            app_radius: 5.0,
            border_radius: 5.0,
            button_radius: 5.0,
            font_scale: 1.0,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTheme {
    pub name: String,
    pub theme_mode: ThemeMode,
    pub base_preset: ThemePreset,
    pub customization: ThemeCustomization,
}

// ── View mode (per-tab) ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Miller, // Three-column Finder-style (default)
    List,   // Single-pane sortable list
    Grid,   // Icon grid with thumbnails
}

// ── Sort ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortBy {
    #[default]
    Name,
    Size,
    Modified,
    Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortConfig {
    pub by: SortBy,
    pub ascending: bool,
}

impl Default for SortConfig {
    fn default() -> Self {
        Self { by: SortBy::Name, ascending: true }
    }
}

// ── List columns ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListColumn {
    Name,
    Kind,
    Size,
    Modified,
}

// ── Search ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SearchScope {
    #[default]
    Global,
    CurrentFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SearchSort {
    #[default]
    Relevance,
    Name,
    Path,
    Modified,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub default_scope: SearchScope,
    pub include_hidden_system: bool,
    pub include_roots: Vec<PathBuf>,
    pub exclude_roots: Vec<PathBuf>,
    /// Optional: only index file content for these roots when enabled.
    pub content_indexing_enabled: bool,
    pub content_include_roots: Vec<PathBuf>,
    pub content_exclude_roots: Vec<PathBuf>,
    pub include_globs: Vec<String>,
    pub exclude_globs: Vec<String>,
    pub default_sort: SearchSort,
    pub result_limit: usize,
    /// How often (in hours) to automatically rebuild the index in the background.
    /// 0 = only rebuild on startup / file-change events / manual trigger.
    pub refresh_interval_hours: u32,
    /// Supplement results with the system locate/plocate database when available.
    pub use_system_locate: bool,
    /// Let Ottrin manage its own mlocate database (~/.cache/ottrin/locate.db).
    /// updatedb is run at startup and on the schedule below. No root required
    /// for the user's own directories. Opt-in.
    pub manage_locate_db: bool,
    /// How often (hours) to re-run updatedb for the managed locate database.
    /// 0 = first launch only.
    pub locate_update_hours: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_scope: SearchScope::Global,
            include_hidden_system: false,
            include_roots: Vec::new(),
            exclude_roots: default_exclude_roots(),
            content_indexing_enabled: false,
            content_include_roots: Vec::new(),
            content_exclude_roots: Vec::new(),
            include_globs: Vec::new(),
            exclude_globs: vec![
                // Editor junk — transient files nobody intentionally searches for
                "*.swp".to_string(),
                "*.swo".to_string(),
                "*.tmp".to_string(),
                "*~".to_string(),
                // VCS internals — binary objects, not user content
                ".git/**".to_string(),
                ".svn/**".to_string(),
                ".hg/**".to_string(),
                // Package manager dependencies — huge, not user files
                "node_modules/**".to_string(),
                // Python bytecode — compiled artifacts, not source
                "__pycache__/**".to_string(),
                "*.pyc".to_string(),
                "*.pyo".to_string(),
                // Trash directories
                ".Trash/**".to_string(),
                ".Trash-*/**".to_string(),
            ],
            default_sort: SearchSort::Relevance,
            result_limit: 500,
            refresh_interval_hours: 4,
            use_system_locate: true,
            manage_locate_db: false,
            locate_update_hours: 6,
        }
    }
}

/// Pre-populated excluded folder roots. On Linux, virtual kernel filesystems
/// are included so they don't get walked if someone indexes `/`. All of these
/// are visible and removable in Settings → Search.
pub fn default_exclude_roots() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "linux")]
    {
        v.push(PathBuf::from("/proc"));
        v.push(PathBuf::from("/sys"));
        v.push(PathBuf::from("/dev"));
        v.push(PathBuf::from("/run"));
    }
    v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub text: String,
    pub scope: SearchScope,
    pub root_path: Option<PathBuf>,
    pub include_hidden_system: bool,
    pub sort: SearchSort,
    pub limit: usize,
    pub offset: usize,
    /// When true, search file contents via ripgrep in addition to filenames.
    #[serde(default)]
    pub content_search: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub path: PathBuf,
    pub parent_path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    #[serde(default)]
    pub is_executable: bool,
    #[serde(default)]
    pub symlink_target_is_dir: Option<bool>,
    pub size_bytes: Option<u64>,
    pub modified_unix_secs: Option<u64>,
    /// For content search results: a short snippet of the matching line.
    #[serde(default)]
    pub content_snippet: Option<String>,
    /// Pre-computed lowercase name — populated at index-build time to avoid
    /// per-query allocations in the hot query path.
    #[serde(skip, default)]
    pub name_lower: String,
    /// Pre-computed lossy path string — populated at index-build time.
    #[serde(skip, default)]
    pub path_str: String,
    /// Pre-computed lowercase lossy path string — avoids per-query allocation.
    #[serde(skip, default)]
    pub path_lower: String,
}

impl SearchResultItem {
    /// Populate the pre-computed lowercase / path-string fields from the primary fields.
    /// Call this once after construction (indexing or loading from cache/DB).
    #[inline]
    pub fn prepare(&mut self) {
        self.name_lower = self.name.to_ascii_lowercase();
        self.path_str = self.path.to_string_lossy().into_owned();
        self.path_lower = self.path_str.to_ascii_lowercase();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchIndexStatus {
    Unavailable,
    Indexing,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub items: Vec<SearchResultItem>,
    pub total: usize,
    pub status: SearchIndexStatus,
    pub error: Option<String>,
}

// ── File entry ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    #[serde(default)]
    pub is_executable: bool,
    #[serde(default)]
    pub symlink_target_is_dir: Option<bool>,
    pub size_bytes: Option<u64>,
    pub modified_unix_secs: Option<u64>,
}

// ── Semantic file classification ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileCategory {
    Directory,
    Symlink,
    Executable,
    Image,
    Video,
    Audio,
    Archive,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Font,
    DiskImage,
    Package,
    Config,
    Code,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeSubtype {
    Rust,
    JavaScript,
    TypeScript,
    HTML,
    CSS,
    JSON,
    Markdown,
    Python,
    Shell,
    TOML,
    YAML,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FolderKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Public,
    Templates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformCaseMode {
    CaseSensitive,
    CaseInsensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSemantic {
    pub category: FileCategory,
    pub code_subtype: Option<CodeSubtype>,
    #[serde(default)]
    pub folder_kind: Option<FolderKind>,
    #[serde(default)]
    pub rule_hint: Option<String>,
}

impl FileSemantic {
    fn new(category: FileCategory, code_subtype: Option<CodeSubtype>, rule_hint: Option<&str>) -> Self {
        Self {
            category,
            code_subtype,
            folder_kind: None,
            rule_hint: rule_hint.map(ToOwned::to_owned),
        }
    }
}

// ── Tab state ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabState {
    pub id: u64,
    pub current_dir: PathBuf,
    pub history_back: Vec<PathBuf>,
    pub history_forward: Vec<PathBuf>,
    pub view_mode: ViewMode,
    pub sort: SortConfig,
    pub label: Option<String>, // custom tab label; None = auto (folder name)
}

impl TabState {
    pub fn new(id: u64, dir: PathBuf) -> Self {
        Self {
            id,
            current_dir: dir,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            view_mode: ViewMode::Miller,
            sort: SortConfig::default(),
            label: None,
        }
    }

    /// Navigate to a new directory, pushing current onto the back-stack.
    pub fn navigate_to(&mut self, dir: PathBuf) {
        if dir == self.current_dir {
            return;
        }
        let old = std::mem::replace(&mut self.current_dir, dir);
        self.history_back.push(old);
        self.history_forward.clear();
    }

    /// Navigate up one real filesystem level. Always works regardless of how
    /// the current dir was reached.
    pub fn navigate_up(&mut self) -> bool {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(parent);
            true
        } else {
            false
        }
    }

    pub fn navigate_back(&mut self) -> bool {
        if let Some(prev) = self.history_back.pop() {
            let current = std::mem::replace(&mut self.current_dir, prev);
            self.history_forward.push(current);
            true
        } else {
            false
        }
    }

    pub fn navigate_forward(&mut self) -> bool {
        if let Some(next) = self.history_forward.pop() {
            let current = std::mem::replace(&mut self.current_dir, next);
            self.history_back.push(current);
            true
        } else {
            false
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.history_back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.history_forward.is_empty()
    }

    pub fn can_go_up(&self) -> bool {
        self.current_dir.parent().is_some()
    }

    /// Returns the display name for this tab's label.
    pub fn display_name(&self) -> &str {
        if let Some(ref label) = self.label {
            return label;
        }
        self.current_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_else(|| self.current_dir.to_str().unwrap_or("—"))
    }
}

// ── Target system ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TargetState {
    pub current: Option<PathBuf>,
    #[serde(default)]
    pub pinned: Vec<PathBuf>, // user-curated targets
    #[serde(default)]
    pub has_ever_set: bool,
    #[serde(default)]
    pub recent: Vec<PathBuf>, // max 10, most recent first
}

impl TargetState {
    pub fn set(&mut self, path: PathBuf) {
        self.has_ever_set = true;
        // Remove duplicate from recent list
        self.recent.retain(|p| p != &path);
        // Push old current into recent
        if let Some(old) = self.current.take()
            && old != path
            && !self.pinned.iter().any(|p| p == &old)
        {
            self.recent.insert(0, old);
            self.recent.truncate(10);
        }
        self.current = Some(path);
    }

    pub fn clear(&mut self) {
        if let Some(current) = self.current.take()
            && !self.pinned.iter().any(|p| p == &current)
        {
            self.recent.insert(0, current);
            self.recent.truncate(10);
        }
    }

    pub fn is_set(&self) -> bool {
        self.current.is_some()
    }

    pub fn pin(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        if !self.pinned.iter().any(|p| p == &path) {
            self.pinned.insert(0, path);
        }
    }

    pub fn unpin(&mut self, path: &PathBuf) {
        self.pinned.retain(|p| p != path);
    }

    pub fn move_pin_up(&mut self, index: usize) {
        if index == 0 || index >= self.pinned.len() {
            return;
        }
        self.pinned.swap(index, index - 1);
    }

    pub fn move_pin_down(&mut self, index: usize) {
        if index + 1 >= self.pinned.len() {
            return;
        }
        self.pinned.swap(index, index + 1);
    }
}

// ── Link view (split tabs) ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkView {
    pub left_tab_id: u64,
    pub right_tab_id: u64,
    /// When pinned, the left panel stays fixed; clicking other tabs updates only
    /// the right panel (Commander-style layout on demand).
    pub left_pinned: bool,
    /// Fraction of total width given to the left panel (0.0–1.0).
    pub split_ratio: f32,
}

impl LinkView {
    pub fn new(left_id: u64, right_id: u64) -> Self {
        Self {
            left_tab_id: left_id,
            right_tab_id: right_id,
            left_pinned: false,
            split_ratio: 0.5,
        }
    }
}

// ── App config (persisted to disk) ───────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: ThemeMode,
    #[serde(default)]
    pub theme_preset: ThemePreset,
    #[serde(default)]
    pub theme_custom: ThemeCustomization,
    /// Per-preset custom overrides (key format: "<preset>::<mode>").
    #[serde(default)]
    pub theme_custom_by_preset: BTreeMap<String, ThemeCustomization>,
    /// User-saved custom themes shown in Appearance settings.
    #[serde(default)]
    pub custom_themes: Vec<SavedTheme>,
    /// Name of the currently active saved custom theme (None = using a built-in preset).
    #[serde(default)]
    pub active_custom_theme: Option<String>,
    pub default_view_mode: ViewMode,
    #[serde(default = "default_true")]
    pub colorize_file_types: bool,
    /// When semantic file colors are on, also colorize folder name labels.
    #[serde(default)]
    pub colorize_folder_labels: bool,
    pub list_columns: Vec<ListColumn>,
    #[serde(default = "default_view_scale")]
    pub miller_view_scale: f32,
    #[serde(default = "default_view_scale")]
    pub list_view_scale: f32,
    #[serde(default = "default_view_scale")]
    pub grid_view_scale: f32,
    pub command_frame_always_visible: bool,
    pub restore_session: bool,
    pub show_hidden_files: bool,
    pub target: TargetState,
    /// Tab directories to restore on next launch.
    pub last_session_dirs: Vec<PathBuf>,
    /// User-editable bookmarks: (icon_char, display_name, path).
    /// The icon_char is a Material Icons codepoint string (e.g. "\u{E88A}").
    #[serde(default)]
    pub bookmarks: Vec<(String, String, PathBuf)>,
    #[serde(default)]
    pub search: SearchConfig,
    /// Per-folder persisted Column view widths (key = absolute folder path).
    #[serde(default)]
    pub folder_column_widths: BTreeMap<String, f32>,
    #[serde(default)]
    pub miller_column_width_mode: MillerColumnWidthMode,
    /// Remembered main window size from last session.
    #[serde(default)]
    pub window_size: Option<[f32; 2]>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            theme_preset: ThemePreset::Ottrin,
            theme_custom: ThemeCustomization::default(),
            theme_custom_by_preset: BTreeMap::new(),
            custom_themes: Vec::new(),
            active_custom_theme: None,
            default_view_mode: ViewMode::Miller,
            colorize_file_types: true,
            colorize_folder_labels: false,
            list_columns: vec![
                ListColumn::Name,
                ListColumn::Kind,
                ListColumn::Size,
                ListColumn::Modified,
            ],
            miller_view_scale: default_view_scale(),
            list_view_scale: default_view_scale(),
            grid_view_scale: default_view_scale(),
            command_frame_always_visible: false,
            restore_session: true,
            show_hidden_files: false,
            target: TargetState::default(),
            last_session_dirs: Vec::new(),
            bookmarks: Vec::new(),
            search: SearchConfig::default(),
            folder_column_widths: BTreeMap::new(),
            miller_column_width_mode: MillerColumnWidthMode::Fixed,
            window_size: None,
        }
    }
}

fn default_view_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MillerColumnWidthMode {
    Fixed,
    Auto,
}

impl Default for MillerColumnWidthMode {
    fn default() -> Self {
        Self::Fixed
    }
}

// ── App state (runtime) ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub config: AppConfig,
    pub tabs: Vec<TabState>,
    pub active_tab_idx: usize,
    pub link_view: Option<LinkView>,
    pub next_tab_id: u64,
}

impl Default for AppState {
    fn default() -> Self {
        let home = default_home_dir();
        let tab = TabState::new(1, home);
        Self {
            config: AppConfig::default(),
            tabs: vec![tab],
            active_tab_idx: 0,
            link_view: None,
            next_tab_id: 2,
        }
    }
}

impl AppState {
    pub fn active_tab(&self) -> &TabState {
        let idx = self.active_tab_idx.min(self.tabs.len().saturating_sub(1));
        &self.tabs[idx]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        let idx = self.active_tab_idx.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[idx]
    }

    pub fn new_tab(&mut self, dir: PathBuf) -> usize {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let mut tab = TabState::new(id, dir);
        tab.view_mode = self.config.default_view_mode;
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    pub fn close_tab(&mut self, idx: usize) {
        if self.tabs.len() <= 1 {
            return; // never close the last tab
        }
        // If this tab is part of link view, unlink first
        let closing_id = self.tabs[idx].id;
        if let Some(lv) = &self.link_view
            && (lv.left_tab_id == closing_id || lv.right_tab_id == closing_id)
        {
            self.link_view = None;
        }
        self.tabs.remove(idx);
        if self.active_tab_idx >= self.tabs.len() {
            self.active_tab_idx = self.tabs.len() - 1;
        }
    }

    pub fn tab_by_id(&self, id: u64) -> Option<&TabState> {
        self.tabs.iter().find(|t| t.id == id)
    }

    pub fn tab_by_id_mut(&mut self, id: u64) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    pub fn tab_idx_by_id(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }
}

// ── File operations ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictAction {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileCommand {
    CreateFile { parent: PathBuf, name: String },
    CreateFolder { parent: PathBuf, name: String },
    Rename { source: PathBuf, new_name: String },
    Delete { targets: Vec<PathBuf>, mode: DeleteMode },
    Copy { sources: Vec<PathBuf>, destination: PathBuf, conflict: ConflictAction },
    Move { sources: Vec<PathBuf>, destination: PathBuf, conflict: ConflictAction },
    ShowProperties { target: PathBuf },
    Chmod { target: PathBuf, mode_str: String },
    Symlink { link_path: PathBuf, target: PathBuf },
}

// ── Privileged operations (planned integrated elevation) ────────────────────

/// Action that may require elevation, routed through a privileged helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivilegedCommand {
    File(FileCommand),
    ListDirectory { path: PathBuf, show_hidden: bool },
}

/// Context passed with privileged requests for auditing / UX messaging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PrivilegedContext {
    pub reason: Option<String>,
    pub cwd: Option<PathBuf>,
}

/// Request payload sent from unprivileged app process to elevated helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedRequest {
    pub command: PrivilegedCommand,
    pub context: PrivilegedContext,
}

/// High-level result category returned by privileged execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivilegedStatus {
    Success,
    Denied,
    Unsupported,
    Failed,
}

/// Typed response payload from elevated helper to UI/platform layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivilegedResponse {
    pub status: PrivilegedStatus,
    pub message: Option<String>,
    pub payload: Option<PrivilegedPayload>,
}

/// Optional typed data returned from privileged operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrivilegedPayload {
    Entries(Vec<FileEntry>),
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn default_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Format a byte count as a human-readable string (best-fit unit).
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1_024;
    const MB: u64 = KB * 1_024;
    const GB: u64 = MB * 1_024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Returns the available size display units for the given byte count,
/// cycling through them with `index % len`.
/// Each entry is `(label, formatted_value)`.
pub fn format_size_units(bytes: u64) -> Vec<String> {
    const KB: f64 = 1_024.0;
    const MB: f64 = KB * 1_024.0;
    const GB: f64 = MB * 1_024.0;
    const TB: f64 = GB * 1_024.0;
    let b = bytes as f64;

    if b >= TB {
        vec![
            format!("{:.2} TB", b / TB),
            format!("{:.1} GB", b / GB),
            format!("{:.0} MB", b / MB),
        ]
    } else if b >= GB {
        vec![
            format!("{:.2} GB", b / GB),
            format!("{:.0} MB", b / MB),
            format!("{:.0} KB", b / KB),
        ]
    } else if b >= MB {
        vec![
            format!("{:.1} MB", b / MB),
            format!("{:.0} KB", b / KB),
            format!("{} B", bytes),
        ]
    } else if b >= KB {
        vec![
            format!("{:.0} KB", b / KB),
            format!("{} B", bytes),
        ]
    } else {
        vec![format!("{} B", bytes)]
    }
}

/// Format a Unix timestamp as a short relative or absolute string.
pub fn format_modified(unix_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ago = now.saturating_sub(unix_secs);
    if ago < 60 {
        "just now".to_string()
    } else if ago < 3_600 {
        format!("{} min ago", ago / 60)
    } else if ago < 86_400 {
        format!("{} hr ago", ago / 3_600)
    } else if ago < 86_400 * 30 {
        format!("{} days ago", ago / 86_400)
    } else {
        // Approximate year/month
        let days = unix_secs / 86_400;
        let year = 1970u64 + days / 365;
        let month = ((days % 365) / 30) + 1;
        let months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
        format!("{} {}", months[(month as usize).min(11)], year)
    }
}

pub fn platform_case_mode_current() -> PlatformCaseMode {
    #[cfg(target_os = "windows")]
    {
        PlatformCaseMode::CaseInsensitive
    }
    #[cfg(not(target_os = "windows"))]
    {
        PlatformCaseMode::CaseSensitive
    }
}

pub fn classify_file(
    path: &Path,
    entry_kind: EntryKind,
    is_executable: bool,
    symlink_target_is_dir: Option<bool>,
    platform_case_mode: PlatformCaseMode,
) -> FileSemantic {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if matches!(entry_kind, EntryKind::Directory) || symlink_target_is_dir == Some(true) {
        let mut sem = FileSemantic::new(FileCategory::Directory, None, Some("fs:directory"));
        sem.folder_kind = classify_folder_kind(file_name);
        return sem;
    }

    if let Some(semantic) = classify_exact_name(file_name, platform_case_mode) {
        return semantic;
    }
    if let Some(semantic) = classify_special_extension(file_name) {
        return semantic;
    }
    if let Some(semantic) = classify_extension(path) {
        return semantic;
    }

    match entry_kind {
        EntryKind::Symlink => FileSemantic::new(FileCategory::Symlink, None, Some("fs:symlink")),
        EntryKind::File | EntryKind::Other if is_executable => {
            FileSemantic::new(FileCategory::Executable, None, Some("fs:executable"))
        }
        _ => FileSemantic::new(FileCategory::Unknown, None, Some("fallback:unknown")),
    }
}

fn classify_exact_name(name: &str, platform_case_mode: PlatformCaseMode) -> Option<FileSemantic> {
    use CodeSubtype as S;
    use FileCategory as C;

    let by_name: [(&str, C, Option<S>, &str); 25] = [
        ("README", C::Document, None, "exact:README"),
        ("README.md", C::Code, Some(S::Markdown), "exact:README.md"),
        ("LICENSE", C::Document, None, "exact:LICENSE"),
        ("COPYING", C::Document, None, "exact:COPYING"),
        ("Dockerfile", C::Config, None, "exact:Dockerfile"),
        ("Makefile", C::Config, None, "exact:Makefile"),
        ("Cargo.toml", C::Config, Some(S::TOML), "exact:Cargo.toml"),
        ("Cargo.lock", C::Config, Some(S::TOML), "exact:Cargo.lock"),
        ("package.json", C::Config, Some(S::JSON), "exact:package.json"),
        ("package-lock.json", C::Config, Some(S::JSON), "exact:package-lock.json"),
        ("pnpm-lock.yaml", C::Config, Some(S::YAML), "exact:pnpm-lock.yaml"),
        ("yarn.lock", C::Config, None, "exact:yarn.lock"),
        ("tsconfig.json", C::Config, Some(S::JSON), "exact:tsconfig.json"),
        ("vite.config.js", C::Config, Some(S::JavaScript), "exact:vite.config.js"),
        ("webpack.config.js", C::Config, Some(S::JavaScript), "exact:webpack.config.js"),
        (".env", C::Config, None, "exact:.env"),
        (".gitignore", C::Config, None, "exact:.gitignore"),
        (".gitattributes", C::Config, None, "exact:.gitattributes"),
        (".editorconfig", C::Config, None, "exact:.editorconfig"),
        ("CMakeLists.txt", C::Config, None, "exact:CMakeLists.txt"),
        ("meson.build", C::Config, None, "exact:meson.build"),
        ("compose.yaml", C::Config, Some(S::YAML), "exact:compose.yaml"),
        ("compose.yml", C::Config, Some(S::YAML), "exact:compose.yml"),
        ("docker-compose.yaml", C::Config, Some(S::YAML), "exact:docker-compose.yaml"),
        ("docker-compose.yml", C::Config, Some(S::YAML), "exact:docker-compose.yml"),
    ];

    by_name
        .into_iter()
        .find(|(candidate, _, _, _)| name_match(name, candidate, platform_case_mode))
        .map(|(_, category, subtype, rule)| FileSemantic::new(category, subtype, Some(rule)))
}

fn classify_special_extension(name: &str) -> Option<FileSemantic> {
    use CodeSubtype as S;
    use FileCategory as C;
    let lower = name.to_ascii_lowercase();
    let by_suffix: [(&str, C, Option<S>, &str); 6] = [
        (".tar.gz", C::Archive, None, "special-ext:.tar.gz"),
        (".tar.bz2", C::Archive, None, "special-ext:.tar.bz2"),
        (".tar.xz", C::Archive, None, "special-ext:.tar.xz"),
        (".tar.zst", C::Archive, None, "special-ext:.tar.zst"),
        (".d.ts", C::Code, Some(S::TypeScript), "special-ext:.d.ts"),
        (".user.js", C::Code, Some(S::JavaScript), "special-ext:.user.js"),
    ];
    by_suffix
        .into_iter()
        .find(|(suffix, _, _, _)| lower.ends_with(suffix))
        .map(|(_, category, subtype, rule)| FileSemantic::new(category, subtype, Some(rule)))
}

fn classify_extension(path: &Path) -> Option<FileSemantic> {
    use CodeSubtype as S;
    use FileCategory as C;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }

    let sem = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif"
        | "avif" | "svg" | "ico" | "icns" | "psd" | "psb" | "ai" | "eps" | "xcf" | "kra"
        | "ora" | "raw" | "dng" | "cr2" | "nef" | "orf" | "arw" | "rw2" | "pef" | "sr2"
        | "svgz" | "jxl" | "fig" | "sketch" => FileSemantic::new(C::Image, None, Some("ext:image")),

        "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" | "mpg" | "mpeg" | "m4v"
        | "3gp" | "mts" | "m2ts" | "ogv" | "rm" | "rmvb" | "f4v" => {
            FileSemantic::new(C::Video, None, Some("ext:video"))
        }

        "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aac" | "m4a" | "wma" | "aiff"
        | "mid" | "midi" | "amr" | "ac3" | "ape" => {
            FileSemantic::new(C::Audio, None, Some("ext:audio"))
        }

        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "lz" | "lzma" | "lz4" | "zst"
        | "tgz" | "tbz" | "txz" | "cab" | "ar" | "cpio" | "jar" | "war" | "ear" => {
            FileSemantic::new(C::Archive, None, Some("ext:archive"))
        }

        "iso" | "img" | "bin" | "cue" | "dmg" | "qcow" | "qcow2" | "vmdk" | "vdi" | "vhd"
        | "vhdx" | "ova" | "ovf" => FileSemantic::new(C::DiskImage, None, Some("ext:disk-image")),

        "ttf" | "otf" | "woff" | "woff2" | "eot" | "pfb" | "pfm" => {
            FileSemantic::new(C::Font, None, Some("ext:font"))
        }

        "pdf" => FileSemantic::new(C::Pdf, None, Some("ext:pdf")),
        "doc" | "docx" | "odt" | "pages" | "txt" | "rtf" | "tex" | "latex" | "epub" | "mobi"
        | "azw" | "azw3" | "fb2" => FileSemantic::new(C::Document, None, Some("ext:document")),

        "xls" | "xlsx" | "ods" | "numbers" | "csv" | "tsv" => {
            FileSemantic::new(C::Spreadsheet, None, Some("ext:spreadsheet"))
        }

        "ppt" | "pptx" | "odp" | "key" => {
            FileSemantic::new(C::Presentation, None, Some("ext:presentation"))
        }

        "deb" | "rpm" | "pkg" | "apk" | "flatpak" | "flatpakref" | "appimage" | "snap"
        | "msi" | "exe" => FileSemantic::new(C::Package, None, Some("ext:package")),

        "ini" | "cfg" | "conf" | "env" | "properties" | "plist" | "lock" | "editorconfig"
        | "gitignore" | "gitattributes" => FileSemantic::new(C::Config, None, Some("ext:config")),

        "rs" => FileSemantic::new(C::Code, Some(S::Rust), Some("ext:code-rust")),
        "js" | "mjs" | "cjs" | "jsx" => {
            FileSemantic::new(C::Code, Some(S::JavaScript), Some("ext:code-js"))
        }
        "ts" | "tsx" => FileSemantic::new(C::Code, Some(S::TypeScript), Some("ext:code-ts")),
        "html" | "htm" => FileSemantic::new(C::Code, Some(S::HTML), Some("ext:code-html")),
        "css" | "scss" | "sass" | "less" => {
            FileSemantic::new(C::Code, Some(S::CSS), Some("ext:code-css"))
        }
        "json" | "jsonc" => FileSemantic::new(C::Code, Some(S::JSON), Some("ext:code-json")),
        "md" | "markdown" | "rst" | "adoc" | "org" => {
            FileSemantic::new(C::Code, Some(S::Markdown), Some("ext:code-markdown"))
        }
        "py" | "pyc" | "pyo" | "ipynb" => {
            FileSemantic::new(C::Code, Some(S::Python), Some("ext:code-python"))
        }
        "sh" | "bash" | "zsh" | "fish" => {
            FileSemantic::new(C::Code, Some(S::Shell), Some("ext:code-shell"))
        }
        "toml" => FileSemantic::new(C::Code, Some(S::TOML), Some("ext:code-toml")),
        "yaml" | "yml" => FileSemantic::new(C::Code, Some(S::YAML), Some("ext:code-yaml")),
        "xml" | "c" | "h" | "cpp" | "hpp" | "cc" | "hh" | "go" | "mod" | "sum" | "java"
        | "class" | "kt" | "kts" | "gradle" | "cs" | "csproj" | "sln" | "vb" | "php"
        | "phtml" | "rb" | "gemspec" | "lua" | "swift" | "sql" => {
            FileSemantic::new(C::Code, Some(S::Other), Some("ext:code-other"))
        }

        _ => return None,
    };
    Some(sem)
}

fn classify_folder_kind(name: &str) -> Option<FolderKind> {
    // Known user-folder names should match regardless of platform case mode.
    // Real-world home folders are commonly titled (e.g. "Pictures", "Downloads").
    let normalized = normalize_folder_name(name);
    let by_name: [(&str, FolderKind); 32] = [
        ("home", FolderKind::Home),
        ("my home", FolderKind::Home),
        ("desktop", FolderKind::Desktop),
        ("my desktop", FolderKind::Desktop),
        ("documents", FolderKind::Documents),
        ("document", FolderKind::Documents),
        ("my documents", FolderKind::Documents),
        ("my document", FolderKind::Documents),
        ("docs", FolderKind::Documents),
        ("downloads", FolderKind::Downloads),
        ("download", FolderKind::Downloads),
        ("my downloads", FolderKind::Downloads),
        ("my download", FolderKind::Downloads),
        ("pictures", FolderKind::Pictures),
        ("picture", FolderKind::Pictures),
        ("photos", FolderKind::Pictures),
        ("photo", FolderKind::Pictures),
        ("images", FolderKind::Pictures),
        ("image", FolderKind::Pictures),
        ("my pictures", FolderKind::Pictures),
        ("my photos", FolderKind::Pictures),
        ("music", FolderKind::Music),
        ("audio", FolderKind::Music),
        ("my music", FolderKind::Music),
        ("videos", FolderKind::Videos),
        ("video", FolderKind::Videos),
        ("movies", FolderKind::Videos),
        ("movie", FolderKind::Videos),
        ("my videos", FolderKind::Videos),
        ("my movies", FolderKind::Videos),
        ("public", FolderKind::Public),
        ("templates", FolderKind::Templates),
    ];

    by_name
        .into_iter()
        .find(|(candidate, _)| normalized == *candidate)
        .map(|(_, kind)| kind)
}

fn name_match(name: &str, candidate: &str, mode: PlatformCaseMode) -> bool {
    match mode {
        PlatformCaseMode::CaseSensitive => name == candidate,
        PlatformCaseMode::CaseInsensitive => name.eq_ignore_ascii_case(candidate),
    }
}

fn normalize_folder_name(name: &str) -> String {
    let mut s = name.trim().replace(['_', '-'], " ");
    // collapse repeated whitespace
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_navigation_history() {
        let mut tab = TabState::new(1, PathBuf::from("/home/user"));
        tab.navigate_to(PathBuf::from("/home/user/docs"));
        tab.navigate_to(PathBuf::from("/home/user/docs/work"));
        assert_eq!(tab.current_dir, PathBuf::from("/home/user/docs/work"));
        assert!(tab.can_go_back());
        assert!(!tab.can_go_forward());
        tab.navigate_back();
        assert_eq!(tab.current_dir, PathBuf::from("/home/user/docs"));
        assert!(tab.can_go_forward());
        tab.navigate_back();
        assert_eq!(tab.current_dir, PathBuf::from("/home/user"));
        assert!(!tab.can_go_back());
    }

    #[test]
    fn navigate_to_clears_forward_history() {
        let mut tab = TabState::new(1, PathBuf::from("/a"));
        tab.navigate_to(PathBuf::from("/b"));
        tab.navigate_back();
        tab.navigate_to(PathBuf::from("/c")); // goes to /c, forward stack cleared
        assert!(!tab.can_go_forward());
        assert_eq!(tab.current_dir, PathBuf::from("/c"));
    }

    #[test]
    fn target_state_recent_list() {
        let mut t = TargetState::default();
        t.set(PathBuf::from("/a"));
        t.set(PathBuf::from("/b"));
        t.set(PathBuf::from("/c"));
        assert_eq!(t.current, Some(PathBuf::from("/c")));
        assert_eq!(t.recent[0], PathBuf::from("/b"));
        assert_eq!(t.recent[1], PathBuf::from("/a"));
        t.clear();
        assert!(t.current.is_none());
        assert_eq!(t.recent[0], PathBuf::from("/c"));
    }

    #[test]
    fn target_state_pinned_behavior() {
        let mut t = TargetState::default();
        t.set(PathBuf::from("/a"));
        t.set(PathBuf::from("/b"));
        t.pin(PathBuf::from("/a"));
        assert_eq!(t.pinned, vec![PathBuf::from("/a")]);
        assert!(!t.recent.iter().any(|p| p == &PathBuf::from("/a")));
        t.unpin(&PathBuf::from("/a"));
        assert!(t.pinned.is_empty());
    }

    #[test]
    fn app_state_new_tab_and_close() {
        let mut state = AppState::default();
        let idx = state.new_tab(PathBuf::from("/tmp"));
        assert_eq!(state.tabs.len(), 2);
        state.close_tab(idx);
        assert_eq!(state.tabs.len(), 1);
        // Should not close the last tab
        state.close_tab(0);
        assert_eq!(state.tabs.len(), 1);
    }

    #[test]
    fn semantic_priority_exact_over_extension() {
        let s = classify_file(
            Path::new("/tmp/Dockerfile"),
            EntryKind::File,
            false,
            None,
            PlatformCaseMode::CaseSensitive,
        );
        assert_eq!(s.category, FileCategory::Config);
        assert_eq!(s.rule_hint.as_deref(), Some("exact:Dockerfile"));
    }

    #[test]
    fn semantic_platform_case_mode() {
        let cs = classify_file(
            Path::new("/tmp/readme.md"),
            EntryKind::File,
            false,
            None,
            PlatformCaseMode::CaseSensitive,
        );
        assert_ne!(cs.rule_hint.as_deref(), Some("exact:README.md"));

        let ci = classify_file(
            Path::new("/tmp/readme.md"),
            EntryKind::File,
            false,
            None,
            PlatformCaseMode::CaseInsensitive,
        );
        assert_eq!(ci.rule_hint.as_deref(), Some("exact:README.md"));
    }

    #[test]
    fn semantic_code_subtype_mapping() {
        let rs = classify_file(
            Path::new("/tmp/main.rs"),
            EntryKind::File,
            false,
            None,
            PlatformCaseMode::CaseSensitive,
        );
        assert_eq!(rs.category, FileCategory::Code);
        assert_eq!(rs.code_subtype, Some(CodeSubtype::Rust));

        let yml = classify_file(
            Path::new("/tmp/compose.yml"),
            EntryKind::File,
            false,
            None,
            PlatformCaseMode::CaseSensitive,
        );
        assert_eq!(yml.category, FileCategory::Config);
        assert_eq!(yml.code_subtype, Some(CodeSubtype::YAML));
    }

    #[test]
    fn semantic_filesystem_fallbacks() {
        let exec = classify_file(
            Path::new("/tmp/run"),
            EntryKind::File,
            true,
            None,
            PlatformCaseMode::CaseSensitive,
        );
        assert_eq!(exec.category, FileCategory::Executable);

        let link_dir = classify_file(
            Path::new("/tmp/link-to-dir"),
            EntryKind::Symlink,
            false,
            Some(true),
            PlatformCaseMode::CaseSensitive,
        );
        assert_eq!(link_dir.category, FileCategory::Directory);
    }
}
