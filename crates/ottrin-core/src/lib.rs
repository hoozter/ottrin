use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
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
    pub size_bytes: Option<u64>,
    pub modified_unix_secs: Option<u64>,
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
    pub recent: Vec<PathBuf>, // max 10, most recent first
}

impl TargetState {
    pub fn set(&mut self, path: PathBuf) {
        // Remove duplicate from recent list
        self.recent.retain(|p| p != &path);
        // Push old current into recent
        if let Some(old) = self.current.take() {
            if old != path {
                self.recent.insert(0, old);
                self.recent.truncate(10);
            }
        }
        self.current = Some(path);
    }

    pub fn clear(&mut self) {
        if let Some(current) = self.current.take() {
            self.recent.insert(0, current);
            self.recent.truncate(10);
        }
    }

    pub fn is_set(&self) -> bool {
        self.current.is_some()
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
    pub default_view_mode: ViewMode,
    pub list_columns: Vec<ListColumn>,
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            default_view_mode: ViewMode::Miller,
            list_columns: vec![
                ListColumn::Name,
                ListColumn::Kind,
                ListColumn::Size,
                ListColumn::Modified,
            ],
            command_frame_always_visible: false,
            restore_session: true,
            show_hidden_files: false,
            target: TargetState::default(),
            last_session_dirs: Vec::new(),
            bookmarks: Vec::new(),
        }
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
        if let Some(lv) = &self.link_view {
            if lv.left_tab_id == closing_id || lv.right_tab_id == closing_id {
                self.link_view = None;
            }
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

// ── Helpers ───────────────────────────────────────────────────────────────────

pub fn default_home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Format a byte count as a human-readable string.
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
}
