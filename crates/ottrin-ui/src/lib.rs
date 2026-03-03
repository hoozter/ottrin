use egui::{
    Align, Align2, Color32, Context, FontId, Frame, Key, Layout, Rect, RichText,
    ScrollArea, Sense, Stroke, UiBuilder, Vec2,
};
use egui::text::{CCursor, CCursorRange};
use ottrin_core::{
    AppConfig, AppState, ConflictAction, DeleteMode, EntryKind, FileCommand, FileEntry,
    ThemeMode, ViewMode, default_home_dir, format_modified, format_size,
};
use ottrin_platform::{DefaultPlatform, PlatformOps};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

// ── Material Icons codepoints ──────────────────────────────────────────────
// Font: MaterialIcons-Regular.ttf (Apache 2.0, bundled)
#[allow(dead_code)]
const MI_ARROW_BACK:    &str = "\u{E5C4}";
const MI_ARROW_FORWARD: &str = "\u{E5C8}";
const MI_ARROW_UP:      &str = "\u{E5D8}";
const MI_CLOSE:         &str = "\u{E5CD}";
const MI_MAXIMIZE:      &str = "\u{E930}";
const MI_MINIMIZE:      &str = "\u{E931}";
const MI_VIEW_COLUMN:   &str = "\u{E8EC}"; // Miller columns
const MI_VIEW_LIST:     &str = "\u{E8EF}"; // List
const MI_APPS:          &str = "\u{E5C3}"; // Grid
const MI_DARK_MODE:     &str = "\u{E51C}";
const MI_LIGHT_MODE:    &str = "\u{E518}";
const MI_FOLDER:        &str = "\u{E2C7}";
const MI_FOLDER_OPEN:   &str = "\u{E2C8}";
const MI_FILE:          &str = "\u{E24D}";
const MI_LINK:          &str = "\u{E157}";
const MI_COPY:          &str = "\u{E14D}";
const MI_CUT:           &str = "\u{E14E}";
const MI_DELETE:        &str = "\u{E872}";
const MI_EDIT:          &str = "\u{E3C9}";
const MI_OPEN:          &str = "\u{E89E}";
const MI_HOME:          &str = "\u{E88A}";
const MI_BOOKMARK:      &str = "\u{E866}";
const MI_SEARCH:        &str = "\u{E8B6}";
const MI_TERMINAL:      &str = "\u{EB8E}";
const MI_CLEAR:         &str = "\u{E14C}";
const MI_MOVE:          &str = "\u{E675}";
const MI_SETTINGS:      &str = "\u{E8B8}";
const MI_INFO:          &str = "\u{E88E}";
const MI_CHECK:         &str = "\u{E5CA}";
const MI_CREATE_FOLDER: &str = "\u{E2CC}";

// File-type icons
const MI_PICTURE_PDF:   &str = "\u{E415}"; // picture_as_pdf
const MI_IMAGE:         &str = "\u{E3F4}"; // image
const MI_MOVIE:         &str = "\u{E02C}"; // movie
const MI_MUSIC_NOTE:    &str = "\u{E405}"; // music_note / audio
const MI_CODE:          &str = "\u{E86F}"; // code
const MI_FOLDER_ZIP:    &str = "\u{EB2C}"; // folder_zip / archive
const MI_SLIDESHOW:     &str = "\u{E41B}"; // slideshow / presentation
const MI_DESCRIPTION:   &str = "\u{E873}"; // description / document
const MI_TABLE_CHART:   &str = "\u{E265}"; // table_chart / spreadsheet
const MI_VISIBILITY:         &str = "\u{E8F4}"; // visibility (show hidden)
const MI_VISIBILITY_OFF:     &str = "\u{E8F5}"; // visibility_off (hide hidden)
const MI_DRIVE_FOLDER_UPLOAD:&str = "\u{E9A3}"; // drive_folder_upload (target destination)
const MI_MENU:               &str = "\u{E5D2}"; // menu (hamburger)
const MI_TUNE:               &str = "\u{E429}"; // tune (display settings / scale)

// ── Theme colours ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Colors {
    bg: Color32,
    panel: Color32,
    panel_raised: Color32,
    border: Color32,
    text: Color32,
    text_dim: Color32,
    text_muted: Color32,
    accent: Color32,
    accent_dim: Color32,
    hover: Color32,
    selected_bg: Color32,
    row_alt: Color32,
    error: Color32,
}

impl Colors {
    fn dark() -> Self {
        Self {
            bg:           Color32::from_rgb(18,  18,  18),
            panel:        Color32::from_rgb(28,  28,  30),
            panel_raised: Color32::from_rgb(40,  40,  44),
            border:       Color32::from_rgb(58,  58,  64),
            text:         Color32::from_rgb(235, 235, 235),
            text_dim:     Color32::from_rgb(160, 160, 160),
            text_muted:   Color32::from_rgb(95,  95,  95),
            accent:       Color32::from_rgb(77,  142, 240),
            accent_dim:   Color32::from_rgba_unmultiplied(77, 142, 240, 60),
            hover:        Color32::from_rgba_unmultiplied(255, 255, 255, 22),
            selected_bg:  Color32::from_rgba_unmultiplied(77, 142, 240, 65),
            row_alt:      Color32::from_rgba_unmultiplied(255, 255, 255, 10),
            error:        Color32::from_rgb(224, 85,  85),
        }
    }

    fn light() -> Self {
        Self {
            bg:           Color32::from_rgb(255, 255, 255),
            panel:        Color32::from_rgb(244, 244, 244),
            panel_raised: Color32::from_rgb(235, 235, 235),
            border:       Color32::from_rgb(208, 208, 208),
            text:         Color32::from_rgb(20,  20,  20),
            text_dim:     Color32::from_rgb(80,  80,  80),
            text_muted:   Color32::from_rgb(150, 150, 150),
            accent:       Color32::from_rgb(37,  99,  235),
            accent_dim:   Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            hover:        Color32::from_rgba_unmultiplied(0, 0, 0, 18),
            selected_bg:  Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            row_alt:      Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            error:        Color32::from_rgb(196, 48,  48),
        }
    }

    fn for_theme(theme: ThemeMode) -> Self {
        match theme {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
            ThemeMode::System => {
                // Use dark as the system default; in a future release we'd
                // query the OS preference via platform-specific APIs.
                Self::dark()
            }
        }
    }
}

// ── Async worker messages ─────────────────────────────────────────────────────

#[derive(Debug)]
struct ListingRequest {
    key: ListingKey,
    path: PathBuf,
    request_id: u64,
    show_hidden: bool,
}

/// Key that identifies which UI slot a listing result belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ListingKey {
    TabMain(u64),   // tab_id — main column (Miller mid / list view)
    TabLeft(u64),   // tab_id — Miller left column (parent dir)
    TabRight(u64),  // tab_id — Miller right column (selected subfolder)
}

#[derive(Debug)]
struct ListingResponse {
    key: ListingKey,
    request_id: u64,
    result: Result<Vec<FileEntry>, String>,
}

/// Async file operation result.
#[derive(Debug)]
struct OpResult {
    tab_id: Option<u64>, // which tab should refresh after this op
}

// ── Per-tab listing state ─────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ColumnState {
    request_id: u64,
    loading: bool,
    entries: Vec<FileEntry>,
    error: Option<String>,
    dir: PathBuf,
}

#[derive(Debug, Default)]
struct TabUiState {
    // Three columns for Miller view; main is also used for List/Grid
    main: ColumnState,
    left: ColumnState,  // parent dir in Miller
    right: ColumnState, // selected subfolder preview in Miller
    right_is_file: bool,

    // Selection: indices into each column's entries
    main_sel: Option<usize>,
    left_sel: Option<usize>, // always points to current dir entry within parent

    // Column scroll ids (must be stable)
    scroll_epoch: u64, // bump to reset scroll on navigation
}

// ── Command frame ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct CommandFrame {
    visible: bool,
    input: String,
    history: Vec<String>,
    history_idx: Option<usize>,
    completions: Vec<String>,
    error: Option<String>,
    message: Option<String>, // informational output (e.g., from "help")
    /// Needs input focus handed to the TextEdit next frame.
    request_focus: bool,
}

// ── Address bar ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct AddressBar {
    editing: bool,
    text: String,
    request_focus: bool,
}

// ── Preview overlay ───────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct PreviewOverlay {
    visible: bool,
    path: Option<PathBuf>,
    content: Option<String>, // text preview lines
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Clipboard {
    sources: Vec<PathBuf>,
    cut: bool,
}

// ── Recent-targets dropdown ───────────────────────────────────────────────────

#[derive(Debug, Default)]
struct TargetDropdown {
    open: bool,
}

// ── Right sidebar section ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SidebarSection {
    #[default]
    None,   // collapsed
    Target,
    Info,
}

// ── Settings tab ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    General,
    Appearance,
    Files,
    Search,
    Cache,
}

// ── Properties popup ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct PropertiesPopup {
    visible: bool,
    path: Option<PathBuf>,
}

// ── Main app struct ───────────────────────────────────────────────────────────

pub struct OttrinApp {
    state: AppState,
    colors: Colors,

    // Per-tab UI state keyed by tab_id
    tab_ui: HashMap<u64, TabUiState>,

    // Async listing
    listing_tx: Sender<ListingRequest>,
    listing_rx: Receiver<ListingResponse>,

    // File operation results
    op_tx: Sender<OpResult>,
    op_rx: Receiver<OpResult>,

    platform: Arc<DefaultPlatform>,
    clipboard: Option<Clipboard>,

    // UI overlays / transient state
    command_frame: CommandFrame,
    address_bar: AddressBar,
    preview: PreviewOverlay,
    target_dropdown: TargetDropdown,
    properties: PropertiesPopup,

    next_request_id: u64,
    sidebar: SidebarSection,
    // Last measured grid column count — set each frame by render_grid,
    // used in handle_keyboard for 2D arrow navigation.
    last_grid_cols: usize,
    // Icon scale factor (1.0 = default). Controlled by bottom resize handle.
    icon_scale: f32,
    // About dialog visible
    show_about: bool,
    // Settings modal visible
    show_settings: bool,
    // Active settings category tab
    settings_tab: SettingsTab,
}

impl Default for OttrinApp {
    fn default() -> Self {
        Self::new_with_config(AppConfig::default())
    }
}

impl OttrinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // Register Material Icons as a fallback font.
        // PUA codepoints (U+E000–U+F8FF) used by Material Icons are not in
        // NotoSans, so egui automatically falls back to this font for them.
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "material-icons".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(
                include_bytes!("../assets/MaterialIcons-Regular.ttf")
            )),
        );
        // Push as last fallback so NotoSans still handles normal text
        fonts.families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("material-icons".to_owned());
        cc.egui_ctx.set_fonts(fonts);

        let config = load_config();
        let app = Self::new_with_config(config);
        app.apply_theme_to_ctx(&cc.egui_ctx);
        app
    }

    fn new_with_config(config: AppConfig) -> Self {
        let (listing_tx, listing_worker_rx) = mpsc::channel::<ListingRequest>();
        let (listing_worker_tx, listing_rx) = mpsc::channel::<ListingResponse>();
        let (op_tx, op_rx) = mpsc::channel::<OpResult>();

        // Single long-lived listing worker thread — serialises requests.
        // For a future improvement, use a thread pool.
        std::thread::spawn(move || {
            while let Ok(req) = listing_worker_rx.recv() {
                let result = list_directory(&req.path, req.show_hidden);
                let _ = listing_worker_tx.send(ListingResponse {
                    key: req.key,
                    request_id: req.request_id,
                    result,
                });
            }
        });

        let colors = Colors::for_theme(config.theme);

        let mut state = AppState::default();
        state.config = config;

        let mut app = Self {
            state,
            colors,
            tab_ui: HashMap::new(),
            listing_tx,
            listing_rx,
            op_tx,
            op_rx,
            platform: Arc::new(DefaultPlatform),
            clipboard: None,
            command_frame: CommandFrame::default(),
            address_bar: AddressBar::default(),
            preview: PreviewOverlay::default(),
            target_dropdown: TargetDropdown::default(),
            properties: PropertiesPopup::default(),
            next_request_id: 1,
            sidebar: SidebarSection::None,
            last_grid_cols: 4,
            icon_scale: 1.0,
            show_about: false,
            show_settings: false,
            settings_tab: SettingsTab::General,
        };

        // Kick off initial listings for the default tab
        app.refresh_active_tab();
        app
    }

    fn apply_theme_to_ctx(&self, ctx: &Context) {
        let mut visuals = match self.state.config.theme {
            ThemeMode::Light => egui::Visuals::light(),
            ThemeMode::Dark | ThemeMode::System => egui::Visuals::dark(),
        };
        let c = &self.colors;
        visuals.panel_fill = c.panel;
        visuals.window_fill = c.bg;
        visuals.extreme_bg_color = c.bg;
        visuals.faint_bg_color = c.row_alt;
        visuals.widgets.noninteractive.bg_fill = c.panel;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text_dim);
        visuals.widgets.inactive.bg_fill = c.panel_raised;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, c.text);
        visuals.widgets.hovered.bg_fill = c.hover;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, c.text);
        visuals.widgets.active.bg_fill = c.accent_dim;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, c.accent);
        visuals.selection.bg_fill = c.selected_bg;
        visuals.selection.stroke = Stroke::new(1.0, c.accent);
        ctx.set_visuals(visuals);
    }

    // ── Listing requests ──────────────────────────────────────────────────────

    fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    fn request_listing(&mut self, key: ListingKey, dir: PathBuf) {
        let id = self.next_id();
        let hidden = self.state.config.show_hidden_files;
        let tab_ui = self.tab_ui_for_key(key);
        match key {
            ListingKey::TabMain(_) => {
                tab_ui.main.request_id = id;
                tab_ui.main.loading = true;
                tab_ui.main.dir = dir.clone();
            }
            ListingKey::TabLeft(_) => {
                tab_ui.left.request_id = id;
                tab_ui.left.loading = true;
                tab_ui.left.dir = dir.clone();
            }
            ListingKey::TabRight(_) => {
                tab_ui.right.request_id = id;
                tab_ui.right.loading = true;
                tab_ui.right.dir = dir.clone();
                tab_ui.right_is_file = false;
            }
        }
        let _ = self.listing_tx.send(ListingRequest { key, path: dir, request_id: id, show_hidden: hidden });
    }

    fn tab_ui_for_key(&mut self, key: ListingKey) -> &mut TabUiState {
        let tab_id = match key {
            ListingKey::TabMain(id) | ListingKey::TabLeft(id) | ListingKey::TabRight(id) => id,
        };
        self.tab_ui.entry(tab_id).or_default()
    }

    fn refresh_active_tab(&mut self) {
        let tab_id = self.state.active_tab().id;
        let dir = self.state.active_tab().current_dir.clone();
        self.refresh_tab(tab_id, dir);
    }

    fn refresh_tab(&mut self, tab_id: u64, dir: PathBuf) {
        // Reset selection and scroll on navigation
        if let Some(ui_state) = self.tab_ui.get_mut(&tab_id) {
            ui_state.main_sel = None;
            ui_state.left_sel = None;
            ui_state.right = ColumnState::default();
            ui_state.right_is_file = false;
            ui_state.scroll_epoch = ui_state.scroll_epoch.wrapping_add(1);
        }
        // Main column = current dir
        self.request_listing(ListingKey::TabMain(tab_id), dir.clone());
        // Left column = parent dir
        if let Some(parent) = dir.parent().map(|p| p.to_path_buf()) {
            let left_dir = parent;
            // Pre-select the current dir entry in the left column
            let cur = dir.clone();
            {
                let tab_ui = self.tab_ui.entry(tab_id).or_default();
                tab_ui.left.dir = left_dir.clone();
            }
            self.request_listing(ListingKey::TabLeft(tab_id), left_dir);
            // After listing loads, find_left_selection will match
            {
                let tab_ui = self.tab_ui.entry(tab_id).or_default();
                tab_ui.left.dir = cur.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            }
        } else {
            // At root — left column is empty
            let tab_ui = self.tab_ui.entry(tab_id).or_default();
            tab_ui.left = ColumnState::default();
        }
    }

    fn poll_listing_results(&mut self) {
        // Collect tab ids that need a right-column load after the loop
        // (can't call load_miller_right mid-loop due to borrows)
        let mut refresh_right_for: Vec<u64> = Vec::new();

        while let Ok(resp) = self.listing_rx.try_recv() {
            let tab_id = match resp.key {
                ListingKey::TabMain(id) | ListingKey::TabLeft(id) | ListingKey::TabRight(id) => id,
            };
            let ui_state = self.tab_ui.entry(tab_id).or_default();
            let col = match resp.key {
                ListingKey::TabMain(_) => &mut ui_state.main,
                ListingKey::TabLeft(_) => &mut ui_state.left,
                ListingKey::TabRight(_) => &mut ui_state.right,
            };
            // Discard stale responses
            if col.request_id != resp.request_id {
                continue;
            }
            col.loading = false;
            match resp.result {
                Ok(mut entries) => {
                    sort_entries(&mut entries, &ottrin_core::SortConfig::default());
                    col.entries = entries;
                    col.error = None;
                }
                Err(e) => {
                    col.entries = Vec::new();
                    col.error = Some(e);
                }
            }
            // After left column loads, find which entry is the current dir
            if matches!(resp.key, ListingKey::TabLeft(_)) {
                let tab = self.state.tab_by_id(tab_id);
                if let Some(tab) = tab {
                    let cur = tab.current_dir.clone();
                    let ui_state = self.tab_ui.entry(tab_id).or_default();
                    ui_state.left_sel = ui_state.left.entries
                        .iter()
                        .position(|e| e.path == cur);
                }
            }
            // After main column loads with no selection, auto-select first
            // entry. This ensures the right (preview) column always shows
            // content immediately after navigation — proper Miller behaviour.
            if matches!(resp.key, ListingKey::TabMain(_)) {
                let ui_state = self.tab_ui.entry(tab_id).or_default();
                if ui_state.main_sel.is_none() && !ui_state.main.entries.is_empty() {
                    ui_state.main_sel = Some(0);
                    refresh_right_for.push(tab_id);
                }
            }
        }

        for tid in refresh_right_for {
            self.load_miller_right(tid);
        }
    }

    fn poll_op_results(&mut self) {
        while let Ok(res) = self.op_rx.try_recv() {
            if let Some(tab_id) = res.tab_id {
                if let Some(tab) = self.state.tab_by_id(tab_id) {
                    let dir = tab.current_dir.clone();
                    self.refresh_tab(tab_id, dir);
                }
            }
        }
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn navigate_tab_to(&mut self, tab_id: u64, dir: PathBuf) {
        if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
            tab.navigate_to(dir.clone());
        }
        self.refresh_tab(tab_id, dir);
    }

    fn navigate_active_back(&mut self) {
        let tab_id = self.state.active_tab().id;
        if self.state.active_tab_mut().navigate_back() {
            let dir = self.state.active_tab().current_dir.clone();
            self.refresh_tab(tab_id, dir);
        }
    }

    fn navigate_active_forward(&mut self) {
        let tab_id = self.state.active_tab().id;
        if self.state.active_tab_mut().navigate_forward() {
            let dir = self.state.active_tab().current_dir.clone();
            self.refresh_tab(tab_id, dir);
        }
    }

    fn navigate_active_up(&mut self) {
        let tab_id = self.state.active_tab().id;
        if self.state.active_tab_mut().navigate_up() {
            let dir = self.state.active_tab().current_dir.clone();
            self.refresh_tab(tab_id, dir);
        }
    }

    fn open_entry(&mut self, path: PathBuf, kind: EntryKind) {
        match kind {
            EntryKind::Directory | EntryKind::Symlink => {
                let tab_id = self.state.active_tab().id;
                self.navigate_tab_to(tab_id, path);
            }
            EntryKind::File => {
                let _ = self.platform.reveal_in_system(&path);
            }
            EntryKind::Other => {}
        }
    }

    fn select_next_entry(&mut self) {
        let tab_id = self.state.active_tab().id;
        let ui_state = self.tab_ui.entry(tab_id).or_default();
        let len = ui_state.main.entries.len();
        if len == 0 { return; }
        ui_state.main_sel = Some(match ui_state.main_sel {
            None => 0,
            Some(i) => (i + 1).min(len - 1),
        });
        self.load_miller_right(tab_id);
    }

    fn select_prev_entry(&mut self) {
        let tab_id = self.state.active_tab().id;
        let ui_state = self.tab_ui.entry(tab_id).or_default();
        let len = ui_state.main.entries.len();
        if len == 0 { return; }
        ui_state.main_sel = Some(match ui_state.main_sel {
            None => 0,
            Some(0) => 0,
            Some(i) => i - 1,
        });
        self.load_miller_right(tab_id);
    }

    fn grid_move(&mut self, delta: isize) {
        let tab_id = self.state.active_tab().id;
        let ui_state = self.tab_ui.entry(tab_id).or_default();
        let len = ui_state.main.entries.len();
        if len == 0 { return; }
        let cur = ui_state.main_sel.unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        ui_state.main_sel = Some(next);
        self.load_miller_right(tab_id);
    }

    fn enter_selected(&mut self) {
        let tab_id = self.state.active_tab().id;
        let entry = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel.and_then(|i| ui.main.entries.get(i)).cloned()
        };
        if let Some(e) = entry {
            self.open_entry(e.path, e.kind);
        }
    }

    fn enter_selected_folder_miller(&mut self) {
        let tab_id = self.state.active_tab().id;
        let entry = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel.and_then(|i| ui.main.entries.get(i)).cloned()
        };
        if let Some(e) = entry {
            if matches!(e.kind, EntryKind::Directory | EntryKind::Symlink) {
                let tab_id = self.state.active_tab().id;
                self.navigate_tab_to(tab_id, e.path);
            }
        }
    }

    fn load_miller_right(&mut self, tab_id: u64) {
        let sel = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel.and_then(|i| ui.main.entries.get(i)).cloned()
        };
        if let Some(entry) = sel {
            match entry.kind {
                EntryKind::Directory | EntryKind::Symlink => {
                    let is_file = { self.tab_ui.entry(tab_id).or_default().right_is_file };
                    let cur_right_dir = {
                        self.tab_ui.entry(tab_id).or_default().right.dir.clone()
                    };
                    if cur_right_dir != entry.path || is_file {
                        self.request_listing(ListingKey::TabRight(tab_id), entry.path);
                    }
                }
                _ => {
                    let ui = self.tab_ui.entry(tab_id).or_default();
                    ui.right_is_file = true;
                    ui.right.entries = Vec::new();
                    ui.right.dir = entry.path;
                }
            }
        } else {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.right = ColumnState::default();
            ui.right_is_file = false;
        }
    }

    // ── Keyboard handling ─────────────────────────────────────────────────────

    fn handle_keyboard(&mut self, ctx: &Context) {
        // Don't intercept keys when typing in address bar
        if self.address_bar.editing {
            return;
        }

        ctx.input(|input| {
            let view = self.state.active_tab().view_mode;
            let is_grid = matches!(view, ViewMode::Grid);

            // ── Navigation ──────────────────────────────────────────────────
            if input.key_pressed(Key::ArrowDown) && !self.command_frame.visible {
                if is_grid {
                    self.grid_move(self.last_grid_cols as isize);
                } else {
                    self.select_next_entry();
                }
            }
            if input.key_pressed(Key::ArrowUp) && !self.command_frame.visible {
                if is_grid {
                    self.grid_move(-(self.last_grid_cols as isize));
                } else {
                    self.select_prev_entry();
                }
            }
            if input.key_pressed(Key::ArrowRight) && !self.command_frame.visible {
                if is_grid {
                    self.grid_move(1);
                } else {
                    self.enter_selected_folder_miller();
                }
            }
            if input.key_pressed(Key::ArrowLeft) && !self.command_frame.visible {
                if is_grid {
                    self.grid_move(-1);
                } else {
                    self.navigate_active_up();
                }
            }
            if input.key_pressed(Key::Enter) && !self.command_frame.visible {
                self.enter_selected();
            }
            if input.key_pressed(Key::Backspace) && !self.command_frame.visible {
                self.navigate_active_up();
            }

            // ── Back / Forward ───────────────────────────────────────────────
            let alt = input.modifiers.alt;
            if alt && input.key_pressed(Key::ArrowLeft) {
                self.navigate_active_back();
            }
            if alt && input.key_pressed(Key::ArrowRight) {
                self.navigate_active_forward();
            }
            if alt && input.key_pressed(Key::ArrowUp) {
                self.navigate_active_up();
            }

            // ── Tabs ─────────────────────────────────────────────────────────
            let ctrl = input.modifiers.ctrl || input.modifiers.command;
            if ctrl && input.key_pressed(Key::T) {
                let home = default_home_dir();
                let idx = self.state.new_tab(home.clone());
                self.state.active_tab_idx = idx;
                self.refresh_tab(self.state.tabs.last().unwrap().id, home);
            }
            if ctrl && input.key_pressed(Key::W) {
                let idx = self.state.active_tab_idx;
                self.state.close_tab(idx);
            }
            if ctrl && input.key_pressed(Key::Tab) {
                let n = self.state.tabs.len();
                self.state.active_tab_idx = (self.state.active_tab_idx + 1) % n;
                self.refresh_active_tab();
            }

            // ── Address bar ───────────────────────────────────────────────────
            if ctrl && input.key_pressed(Key::L) {
                let path = self.state.active_tab().current_dir.display().to_string();
                self.address_bar.text = path;
                self.address_bar.editing = true;
                self.address_bar.request_focus = true;
            }

            // ── Preview ───────────────────────────────────────────────────────
            if input.key_pressed(Key::Escape) {
                self.preview.visible = false;
                self.command_frame.visible = false;
                self.command_frame.error = None;
                self.address_bar.editing = false;
                self.target_dropdown.open = false;
            }

            // ── File operations ───────────────────────────────────────────────
            if ctrl && input.key_pressed(Key::C) && !self.command_frame.visible {
                self.copy_selection(false);
            }
            if ctrl && input.key_pressed(Key::X) && !self.command_frame.visible {
                self.copy_selection(true);
            }
            if ctrl && input.key_pressed(Key::V) && !self.command_frame.visible {
                self.paste_clipboard();
            }
            if ctrl && input.key_pressed(Key::A) && !self.command_frame.visible {
                self.select_all();
            }
            if input.key_pressed(Key::F2) && !self.command_frame.visible {
                self.begin_rename();
            }
            if input.key_pressed(Key::Delete) && !self.command_frame.visible {
                if input.modifiers.shift {
                    self.delete_selection(DeleteMode::Permanent);
                } else {
                    self.delete_selection(DeleteMode::Trash);
                }
            }

            // ── Refresh ───────────────────────────────────────────────────────
            if input.key_pressed(Key::F5) || (ctrl && input.key_pressed(Key::R)) {
                self.refresh_active_tab();
            }

            // ── Command frame activation ──────────────────────────────────────
            // Any printable non-whitespace character while file list is focused
            // opens the command frame. Space is excluded — it controls the preview.
            if !self.command_frame.visible && !self.address_bar.editing {
                for event in &input.events {
                    if let egui::Event::Text(text) = event {
                        // Skip whitespace-only text (space, newline, etc.)
                        if !text.is_empty() && !ctrl && !text.chars().all(|c| c.is_whitespace()) {
                            self.command_frame.input = text.clone();
                            self.command_frame.visible = true;
                            self.command_frame.request_focus = true;
                            self.command_frame.error = None;
                            self.command_frame.message = None;
                            break;
                        }
                    }
                }
            }
        });
    }

    fn open_preview(&mut self) {
        let tab_id = self.state.active_tab().id;
        let selected = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel
                .and_then(|idx| ui.main.entries.get(idx))
                .map(|entry| (entry.path.clone(), entry.kind))
        };
        if let Some((path, kind)) = selected {
            self.preview.path = Some(path.clone());
            self.preview.visible = true;
            if kind == EntryKind::File {
                self.preview.content = load_text_preview(&path);
            } else {
                self.preview.content = None;
            }
        }
    }

    fn copy_selection(&mut self, cut: bool) {
        let tab_id = self.state.active_tab().id;
        let ui = self.tab_ui.entry(tab_id).or_default();
        if let Some(idx) = ui.main_sel {
            if let Some(entry) = ui.main.entries.get(idx) {
                self.clipboard = Some(Clipboard {
                    sources: vec![entry.path.clone()],
                    cut,
                });
            }
        }
    }

    fn paste_clipboard(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            let dest = self.state.active_tab().current_dir.clone();
            let tab_id = self.state.active_tab().id;
            let cmd = if clip.cut {
                FileCommand::Move { sources: clip.sources, destination: dest, conflict: ConflictAction::Rename }
            } else {
                FileCommand::Copy { sources: clip.sources, destination: dest, conflict: ConflictAction::Rename }
            };
            self.run_file_op(cmd, Some(tab_id));
            if clip.cut { self.clipboard = None; }
        }
    }

    fn delete_selection(&mut self, mode: DeleteMode) {
        let tab_id = self.state.active_tab().id;
        let targets: Vec<PathBuf> = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel
                .and_then(|i| ui.main.entries.get(i))
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default()
        };
        if targets.is_empty() { return; }
        let cmd = FileCommand::Delete { targets, mode };
        self.run_file_op(cmd, Some(tab_id));
    }

    fn select_all(&mut self) {
        // For now just select last item (multi-select is Phase 2)
        let tab_id = self.state.active_tab().id;
        let ui = self.tab_ui.entry(tab_id).or_default();
        if !ui.main.entries.is_empty() {
            ui.main_sel = Some(ui.main.entries.len() - 1);
        }
    }

    fn begin_rename(&mut self) {
        let tab_id = self.state.active_tab().id;
        let ui = self.tab_ui.entry(tab_id).or_default();
        if let Some(idx) = ui.main_sel {
            if let Some(entry) = ui.main.entries.get(idx) {
                self.command_frame.input = format!("mv {} ", entry.name);
                self.command_frame.visible = true;
                self.command_frame.request_focus = true;
            }
        }
    }

    fn run_file_op(&mut self, cmd: FileCommand, refresh_tab: Option<u64>) {
        let platform = Arc::clone(&self.platform);
        let op_tx = self.op_tx.clone();
        std::thread::spawn(move || {
            let _ = platform.execute_command(&cmd);
            let _ = op_tx.send(OpResult { tab_id: refresh_tab });
        });
    }

    // ── Command frame execution ───────────────────────────────────────────────

    fn execute_command_input(&mut self) {
        let raw = self.command_frame.input.trim().to_string();
        if raw.is_empty() {
            self.command_frame.visible = false;
            return;
        }
        // Push to history
        self.command_frame.history.push(raw.clone());
        self.command_frame.history_idx = None;

        let tab_id = self.state.active_tab().id;
        let cwd = self.state.active_tab().current_dir.clone();
        // dispatch returns (error, message, keep_open)
        let (error, message, keep_open) = self.dispatch_command(&raw, tab_id, &cwd);
        if let Some(err) = error {
            self.command_frame.error = Some(err);
            self.command_frame.message = None;
            self.command_frame.request_focus = true; // keep user typing after an error
        } else if keep_open {
            self.command_frame.error = None;
            self.command_frame.message = message;
            self.command_frame.input.clear();
            self.command_frame.request_focus = true;
        } else {
            self.command_frame.input.clear();
            self.command_frame.visible = false;
            self.command_frame.error = None;
            self.command_frame.message = None;
        }
    }

    /// Returns (error, message, keep_open).
    /// error = Some → show error and keep frame open
    /// message + keep_open = show informational text and keep frame open
    /// all None + !keep_open → close frame (success)
    fn dispatch_command(&mut self, raw: &str, tab_id: u64, cwd: &Path) -> (Option<String>, Option<String>, bool) {
        let parts: Vec<&str> = raw.splitn(3, ' ').collect();
        let cmd_word = parts[0].to_lowercase();

        // helper closures
        macro_rules! err { ($e:expr) => { (Some($e.into()), None, false) }; }
        macro_rules! ok   { ()       => { (None, None, false) }; }
        macro_rules! msg  { ($m:expr) => { (None, Some($m.into()), true) }; }

        match cmd_word.as_str() {
            "help" | "?" => {
                msg!("cd  mkdir  touch  cp  mv  rm [-f]  chmod  ln -s  terminal  <path>")
            }
            "cd" | "/" => {
                let target_str = parts.get(1).copied().unwrap_or("~");
                let target = resolve_path(target_str, cwd);
                if target.is_dir() {
                    self.navigate_tab_to(tab_id, target);
                    ok!()
                } else {
                    err!(format!("Not a directory: {}", target.display()))
                }
            }
            "mkdir" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() { return err!("Usage: mkdir <name>"); }
                let parent = cwd.to_path_buf();
                let cmd = FileCommand::CreateFolder { parent, name: name.to_string() };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "touch" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() { return err!("Usage: touch <name>"); }
                let parent = cwd.to_path_buf();
                let cmd = FileCommand::CreateFile { parent, name: name.to_string() };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "cp" => {
                let src_str = parts.get(1).copied().unwrap_or("");
                let dst_str = parts.get(2).copied().unwrap_or_else(|| {
                    self.state.config.target.current.as_ref().map(|p| p.to_str().unwrap_or("")).unwrap_or("")
                });
                let src = resolve_path(src_str, cwd);
                let dst = resolve_path(dst_str, cwd);
                if !src.exists() { return err!(format!("No such file: {}", src.display())); }
                if !dst.is_dir() { return err!(format!("Destination not a directory: {}", dst.display())); }
                let cmd = FileCommand::Copy { sources: vec![src], destination: dst, conflict: ConflictAction::Rename };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "mv" => {
                let src_str = parts.get(1).copied().unwrap_or("");
                let dst_str = parts.get(2).copied().unwrap_or("");
                let dst_resolved = if dst_str.is_empty() {
                    self.state.config.target.current.clone()
                } else {
                    Some(resolve_path(dst_str, cwd))
                };
                let src = resolve_path(src_str, cwd);
                if !src.exists() { return err!(format!("No such file: {}", src.display())); }
                match dst_resolved {
                    None => err!("No destination specified and no target set."),
                    Some(dst) => {
                        if !dst.is_dir() { return err!(format!("Destination not a directory: {}", dst.display())); }
                        let cmd = FileCommand::Move { sources: vec![src], destination: dst, conflict: ConflictAction::Rename };
                        self.run_file_op(cmd, Some(tab_id));
                        ok!()
                    }
                }
            }
            "rm" => {
                let force = parts.get(1).copied() == Some("-f");
                let src_str = parts.get(if force { 2 } else { 1 }).copied().unwrap_or("");
                if src_str.is_empty() { return err!("Usage: rm [-f] <file>"); }
                let src = resolve_path(src_str, cwd);
                if !src.exists() { return err!(format!("No such file: {}", src.display())); }
                let mode = if force { DeleteMode::Permanent } else { DeleteMode::Trash };
                let cmd = FileCommand::Delete { targets: vec![src], mode };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "chmod" => {
                let mode_str = parts.get(1).copied().unwrap_or("").to_string();
                let file_str = parts.get(2).copied().unwrap_or(parts.get(1).copied().unwrap_or(""));
                let (m, f) = if mode_str.starts_with('+') || mode_str.starts_with('-') {
                    (mode_str.clone(), parts.get(2).copied().unwrap_or(parts.get(1).copied().unwrap_or("")))
                } else {
                    (mode_str.clone(), file_str)
                };
                let target = resolve_path(f, cwd);
                if !target.exists() { return err!(format!("No such file: {}", target.display())); }
                let cmd = FileCommand::Chmod { target, mode_str: m };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "ln" => {
                let tokens: Vec<&str> = raw.split_whitespace().collect();
                if tokens.get(1).copied() != Some("-s") {
                    return err!("Usage: ln -s <target> <link_path>");
                }
                let target_str = tokens.get(2).copied().unwrap_or("");
                let link_str = tokens.get(3).copied().unwrap_or("");
                if target_str.is_empty() || link_str.is_empty() {
                    return err!("Usage: ln -s <target> <link_path>");
                }
                let target_path = resolve_path(target_str, cwd);
                let link_path = resolve_path(link_str, cwd);
                let cmd = FileCommand::Symlink { link_path, target: target_path };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "terminal" | "cmd" | "term" => {
                open_terminal_here(cwd);
                ok!()
            }
            _ => {
                let target = resolve_path(&raw, cwd);
                if target.is_dir() {
                    self.navigate_tab_to(tab_id, target);
                    ok!()
                } else {
                    err!(format!("Unknown command: {}  (type 'help' for a list)", cmd_word))
                }
            }
        }
    }

    // ── Rendering: top-level ─────────────────────────────────────────────────

    /// Row 1 (topmost): tabs on the left, window controls on the right.
    /// This panel is also draggable to move the window.
    fn render_tab_row(&mut self, ctx: &Context) {
        let c = self.colors;

        let panel_resp = egui::TopBottomPanel::top("tab_row")
            .exact_height(34.0)
            .frame(Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(4, 0)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let n_tabs = self.state.tabs.len();
                    let active = self.state.active_tab_idx;
                    let mut navigate_to: Option<(u64, PathBuf)> = None;
                    let mut close_idx: Option<usize> = None;
                    let mut new_tab_requested = false;

                    for i in 0..n_tabs {
                        let tab = &self.state.tabs[i];
                        let is_active = i == active;
                        let label = tab.display_name().to_string();
                        let tab_id = tab.id;

                        let tab_fill = if is_active { c.bg } else { Color32::TRANSPARENT };
                        let text_color = if is_active { c.text } else { c.text_dim };

                        let frame = Frame::new()
                            .fill(tab_fill)
                            .stroke(Stroke::new(0.5, c.border))
                            .corner_radius(egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 })
                            .inner_margin(egui::Margin { left: 10, right: 6, top: 6, bottom: 0 });

                        let resp = frame.show(ui, |ui| {
                            ui.set_max_width(160.0);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.label(RichText::new(&label).color(text_color).size(12.5));
                                ui.add_space(4.0);
                                if n_tabs > 1 {
                                    if ui.add(egui::Label::new(
                                        RichText::new("\u{00D7}").color(c.text_muted).size(11.0)
                                    ).sense(Sense::click())).clicked() {
                                        close_idx = Some(i);
                                    }
                                }
                            });
                        });

                        let tab_rect = resp.response.rect;
                        // Frame::show response has Sense::hover only — interact explicitly for clicks
                        let tab_click = ui.interact(tab_rect, ui.id().with(("tab_click", tab_id)), Sense::click());
                        if tab_click.clicked() && close_idx.is_none() && !is_active {
                            self.state.active_tab_idx = i;
                            if !self.tab_ui.contains_key(&tab_id) {
                                let dir = self.state.tabs[i].current_dir.clone();
                                navigate_to = Some((tab_id, dir));
                            }
                        }
                        resp.response.on_hover_text(self.state.tabs[i].current_dir.display().to_string());
                        if is_active {
                            ui.painter().hline(tab_rect.x_range(), tab_rect.bottom() - 1.0, Stroke::new(2.0, c.accent));
                        }
                    }

                    // New tab +
                    ui.add_space(6.0);
                    if ui.add(
                        egui::Button::new(RichText::new("+").color(c.text_muted).size(16.0)).frame(false)
                    ).clicked() {
                        new_tab_requested = true;
                    }

                    // Apply tab actions
                    if let Some(idx) = close_idx {
                        self.state.close_tab(idx);
                    } else if let Some((tid, dir)) = navigate_to {
                        self.refresh_tab(tid, dir);
                    }
                    if new_tab_requested {
                        let home = default_home_dir();
                        let idx = self.state.new_tab(home.clone());
                        self.state.active_tab_idx = idx;
                        let new_id = self.state.tabs.last().unwrap().id;
                        self.refresh_tab(new_id, home);
                    }

                    // Window controls — far right, Material Icons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        if ui.add(egui::Button::new(RichText::new(MI_CLOSE).size(18.0).color(c.text_muted)).frame(false).min_size(Vec2::splat(28.0))).on_hover_text("Close").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        if ui.add(egui::Button::new(RichText::new(MI_MAXIMIZE).size(18.0).color(c.text_muted)).frame(false).min_size(Vec2::splat(28.0))).on_hover_text(if is_max {"Restore"} else {"Maximize"}).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                        }
                        if ui.add(egui::Button::new(RichText::new(MI_MINIMIZE).size(18.0).color(c.text_muted)).frame(false).min_size(Vec2::splat(28.0))).on_hover_text("Minimize").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });

        // Drag the window: start a system drag when the primary button is pressed
        // inside the panel's empty areas (interactive widgets consume their own events).
        let panel_rect = panel_resp.response.rect;
        let pointer_in_bar = ctx.input(|i| {
            i.pointer.hover_pos().map(|p| panel_rect.contains(p)).unwrap_or(false)
        });
        let is_pressed = ctx.input(|i| i.pointer.primary_pressed());
        if pointer_in_bar && is_pressed {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    /// Row 2: back/forward/up navigation, address bar, view toggle, theme toggle.
    fn render_nav_row(&mut self, ctx: &Context) {
        let c = self.colors;

        egui::TopBottomPanel::top("nav_row")
            .exact_height(36.0)
            .frame(Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let can_back = self.state.active_tab().can_go_back();
                    let can_fwd  = self.state.active_tab().can_go_forward();
                    let can_up   = self.state.active_tab().can_go_up();

                    let back_col = if can_back { c.text } else { c.text_muted };
                    let fwd_col  = if can_fwd  { c.text } else { c.text_muted };
                    let up_col   = if can_up   { c.text } else { c.text_muted };
                    if ui.add_enabled(can_back, egui::Button::new(RichText::new(MI_ARROW_BACK).size(20.0).color(back_col)).frame(false).min_size(Vec2::splat(30.0))).on_hover_text("Back").clicked() { self.navigate_active_back(); }
                    if ui.add_enabled(can_fwd,  egui::Button::new(RichText::new(MI_ARROW_FORWARD).size(20.0).color(fwd_col)).frame(false).min_size(Vec2::splat(30.0))).on_hover_text("Forward").clicked() { self.navigate_active_forward(); }
                    if ui.add_enabled(can_up,   egui::Button::new(RichText::new(MI_ARROW_UP).size(20.0).color(up_col)).frame(false).min_size(Vec2::splat(30.0))).on_hover_text("Parent folder").clicked() { self.navigate_active_up(); }
                    ui.add_space(6.0);

                    // Right side: view toggle, settings menu, then address bar fills the rest
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);

                        // ── Hamburger menu ────────────────────────────────────
                        let menu_popup_id = ui.make_persistent_id("hamburger_menu");
                        let menu_btn = ui.add(
                            egui::Button::new(RichText::new(MI_MENU).size(20.0).color(c.text_dim))
                                .frame(false).min_size(Vec2::splat(30.0))
                        ).on_hover_text("Menu");
                        if menu_btn.clicked() {
                            egui::Popup::toggle_id(ui.ctx(), menu_popup_id);
                        }
                        egui::Popup::menu(&menu_btn)
                            .id(menu_popup_id)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(190.0);
                                ui.style_mut().spacing.item_spacing.y = 0.0;

                                // ── Create ─────────────────────────────────────
                                if ui.add(egui::Button::new(
                                    RichText::new(format!("{} New Folder", MI_CREATE_FOLDER)).size(13.0)
                                ).frame(false).min_size(Vec2::new(190.0, 28.0))).clicked() {
                                    self.command_frame.input = "mkdir ".to_string();
                                    self.command_frame.visible = true;
                                    self.command_frame.request_focus = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }
                                if ui.add(egui::Button::new(
                                    RichText::new(format!("{} New File", MI_FILE)).size(13.0)
                                ).frame(false).min_size(Vec2::new(190.0, 28.0))).clicked() {
                                    self.command_frame.input = "touch ".to_string();
                                    self.command_frame.visible = true;
                                    self.command_frame.request_focus = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }

                                ui.add(egui::Separator::default().horizontal().spacing(4.0));

                                // ── Settings ───────────────────────────────────
                                if ui.add(egui::Button::new(
                                    RichText::new(format!("{} Settings…", MI_SETTINGS)).size(13.0)
                                ).frame(false).min_size(Vec2::new(190.0, 28.0))).clicked() {
                                    self.show_settings = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }

                                ui.add(egui::Separator::default().horizontal().spacing(4.0));

                                // ── About ──────────────────────────────────────
                                if ui.add(egui::Button::new(
                                    RichText::new(format!("{} About Ottrin", MI_INFO)).size(13.0)
                                ).frame(false).min_size(Vec2::new(190.0, 28.0))).clicked() {
                                    self.show_about = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }
                            });

                        // View mode toggle — Material Icon
                        let view = self.state.active_tab().view_mode;
                        let (view_icon, view_tip) = match view {
                            ViewMode::Miller => (MI_VIEW_COLUMN, "Switch to List view"),
                            ViewMode::List   => (MI_VIEW_LIST,   "Switch to Grid view"),
                            ViewMode::Grid   => (MI_APPS,        "Switch to Miller view"),
                        };
                        if ui.add(egui::Button::new(RichText::new(view_icon).size(20.0).color(c.text_dim)).frame(false).min_size(Vec2::splat(30.0))).on_hover_text(view_tip).clicked() {
                            let next = match view {
                                ViewMode::Miller => ViewMode::List,
                                ViewMode::List   => ViewMode::Grid,
                                ViewMode::Grid   => ViewMode::Miller,
                            };
                            self.state.active_tab_mut().view_mode = next;
                        }
                        ui.add_space(6.0);

                        // Address bar fills the remaining width (leave 20px gap on left)
                        let bw = ui.available_width();
                        ui.allocate_ui(Vec2::new((bw - 20.0).max(80.0), 26.0), |ui| {
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                self.render_address_bar(ui);
                            });
                        });
                    });
                });
            });
    }

    /// Row 3: bookmark shortcuts to common directories (user-editable).
    fn render_bookmarks_row(&mut self, ctx: &Context) {
        let c = self.colors;
        let home = default_home_dir();

        // Ensure default bookmarks exist on first run
        if self.state.config.bookmarks.is_empty() {
            let defaults = [
                (MI_HOME.to_string(),     "Home".to_string(),      home.clone()),
                ("/".to_string(),         "Root".to_string(),      PathBuf::from("/")),
                (MI_FOLDER.to_string(),   "Desktop".to_string(),   home.join("Desktop")),
                (MI_FOLDER.to_string(),   "Downloads".to_string(), home.join("Downloads")),
                (MI_FOLDER.to_string(),   "Documents".to_string(), home.join("Documents")),
            ];
            self.state.config.bookmarks = defaults
                .into_iter()
                .filter(|(_, _, p)| p.exists())
                .map(|(icon, name, path)| (icon, name, path))
                .collect();
        }

        let mut navigate_to: Option<PathBuf> = None;
        let mut remove_idx: Option<usize> = None;
        let current_dir = self.state.active_tab().current_dir.clone();

        egui::TopBottomPanel::top("bookmarks_row")
            .exact_height(30.0)
            .frame(
                Frame::new()
                    .fill(c.bg)
                    .stroke(Stroke::new(1.0, c.border))
                    .inner_margin(egui::Margin::symmetric(8, 0))
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let bookmarks = self.state.config.bookmarks.clone();
                    for (idx, (icon, name, path)) in bookmarks.iter().enumerate() {
                        let is_here = &current_dir == path;
                        // All bookmarks look like buttons; active one uses accent tint
                        let (text_col, bg_col, stroke_col) = if is_here {
                            (c.accent,
                             c.selected_bg,
                             c.accent)
                        } else {
                            (c.text_dim, c.panel_raised, c.border)
                        };

                        let btn = egui::Button::new(
                            RichText::new(format!("{} {}", icon, name)).size(12.0).color(text_col)
                        )
                        .fill(bg_col)
                        .stroke(Stroke::new(1.0, stroke_col))
                        .corner_radius(5.0)
                        .min_size(Vec2::new(0.0, 22.0));

                        let resp = ui.add(btn).on_hover_text(path.display().to_string());
                        if resp.clicked() {
                            navigate_to = Some(path.clone());
                        }
                        resp.context_menu(|ui| {
                            ui.set_min_width(160.0);
                            if ui.add(egui::Button::new(
                                RichText::new(format!("{} Remove \"{}\"", MI_CLEAR, name)).size(13.0)
                            ).frame(false).min_size(Vec2::new(160.0, 28.0))).clicked() {
                                remove_idx = Some(idx);
                                ui.close();
                            }
                        });
                    }
                });
            });

        if let Some(path) = navigate_to {
            let tab_id = self.state.active_tab().id;
            self.navigate_tab_to(tab_id, path);
        }
        if let Some(idx) = remove_idx {
            self.state.config.bookmarks.remove(idx);
        }
    }

    fn render_address_bar(&mut self, ui: &mut egui::Ui) {
        let c = self.colors;

        if self.address_bar.editing {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.address_bar.text)
                    .font(FontId::proportional(14.0))
                    .text_color(c.text)
                    .frame(true)
                    .desired_width(ui.available_width())
            );
            if self.address_bar.request_focus {
                response.request_focus();
                self.address_bar.request_focus = false;
            }
            if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let raw = self.address_bar.text.trim().to_string();
                let cwd = self.state.active_tab().current_dir.clone();
                let target = resolve_path(&raw, &cwd);
                if target.is_dir() {
                    let tab_id = self.state.active_tab().id;
                    self.navigate_tab_to(tab_id, target);
                }
                self.address_bar.editing = false;
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.address_bar.editing = false;
            }
        } else {
            // Breadcrumb display
            let path = self.state.active_tab().current_dir.clone();
            let frame = Frame::new()
                .fill(c.panel_raised)
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 3));
            let addr_resp = frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.set_height(18.0);
                    // Components of the path as clickable breadcrumbs
                    let mut cumulative = PathBuf::new();
                    let components: Vec<_> = path.components().collect();
                    for (i, component) in components.iter().enumerate() {
                        let comp_str = component.as_os_str().to_string_lossy();
                        let display = if comp_str == "/" || comp_str.is_empty() {
                            "/".to_string()
                        } else {
                            comp_str.to_string()
                        };
                        cumulative.push(component);
                        let dest = cumulative.clone();
                        let is_last = i == components.len() - 1;
                        let text_color = if is_last { c.text } else { c.text_dim };
                        let resp = ui.add(
                            egui::Label::new(
                                RichText::new(&display).color(text_color).size(13.0)
                            ).sense(Sense::click())
                        );
                        if resp.clicked() {
                            let tab_id = self.state.active_tab().id;
                            self.navigate_tab_to(tab_id, dest);
                        }
                        if resp.hovered() { ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand); }
                        if !is_last {
                            ui.label(RichText::new("›").color(c.text_muted).size(12.0));
                        }
                    }
                });
            });
            // Click anywhere in the bar to switch to path-edit mode
            let bar_interact = ui.interact(
                addr_resp.response.rect,
                egui::Id::new("addr_bar_click"),
                Sense::click(),
            );
            if addr_resp.response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            if bar_interact.clicked() {
                let path_str = self.state.active_tab().current_dir.display().to_string();
                self.address_bar.text = path_str;
                self.address_bar.editing = true;
                self.address_bar.request_focus = true;
            }
        }
    }

    fn render_target_sidebar(&mut self, ctx: &Context) {
        let c = self.colors;
        let target_set = self.state.config.target.is_set();
        let is_expanded = self.sidebar != SidebarSection::None;
        // Always show the 36px icon strip; when expanded add 220px content panel
        let strip_w = 36.0;
        let content_w = 220.0;
        let panel_width = if is_expanded { strip_w + content_w } else { strip_w };

        egui::SidePanel::right("target_panel")
            .exact_width(panel_width)
            .resizable(false)
            .frame(Frame::new().fill(c.panel).stroke(Stroke::new(1.0, c.border)))
            .show(ctx, |ui| {
                let full = ui.max_rect();

                // ── Icon strip (always visible, rightmost 36px) ─────────────
                let strip_rect = Rect::from_min_size(
                    egui::Pos2::new(full.max.x - strip_w, full.min.y),
                    Vec2::new(strip_w, full.height()),
                );
                // Slightly raised background for strip
                ui.painter().rect_filled(strip_rect, 0.0, c.panel_raised);

                ui.scope_builder(UiBuilder::new().max_rect(strip_rect), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(12.0);

                        // Info icon — highlight when active
                        let info_active = self.sidebar == SidebarSection::Info;
                        if info_active {
                            ui.painter().rect_filled(
                                Rect::from_min_size(egui::Pos2::new(strip_rect.min.x, ui.cursor().min.y - 2.0), Vec2::new(strip_w, 32.0)),
                                4.0, c.selected_bg,
                            );
                            ui.painter().vline(strip_rect.min.x + 1.0,
                                (ui.cursor().min.y - 2.0)..=(ui.cursor().min.y + 30.0),
                                Stroke::new(2.0, c.accent));
                        }
                        let info_col = if info_active { c.accent } else { c.text_dim };
                        if ui.add(egui::Button::new(
                            RichText::new(MI_DESCRIPTION).size(18.0).color(info_col)
                        ).frame(false).min_size(Vec2::splat(28.0)))
                        .on_hover_text("Info").clicked() {
                            self.sidebar = if info_active { SidebarSection::None } else { SidebarSection::Info };
                        }

                        ui.add_space(8.0);

                        // Target icon — highlight when active or set
                        let target_active = self.sidebar == SidebarSection::Target;
                        if target_active {
                            ui.painter().rect_filled(
                                Rect::from_min_size(egui::Pos2::new(strip_rect.min.x, ui.cursor().min.y - 2.0), Vec2::new(strip_w, 32.0)),
                                4.0, c.selected_bg,
                            );
                            ui.painter().vline(strip_rect.min.x + 1.0,
                                (ui.cursor().min.y - 2.0)..=(ui.cursor().min.y + 30.0),
                                Stroke::new(2.0, c.accent));
                        }
                        let target_col = if target_active { c.accent } else if target_set { c.accent_dim } else { c.text_muted };
                        if ui.add(egui::Button::new(
                            RichText::new(MI_DRIVE_FOLDER_UPLOAD).size(22.0).color(target_col)
                        ).frame(false).min_size(Vec2::splat(30.0)))
                        .on_hover_text(if target_set { "Target set" } else { "No target" }).clicked() {
                            self.sidebar = if target_active { SidebarSection::None } else { SidebarSection::Target };
                        }
                    });
                });

                if !is_expanded { return; }

                // Separator between content and strip
                ui.painter().vline(full.max.x - strip_w, full.y_range(), Stroke::new(1.0, c.border));

                // ── Content panel (left 220px when expanded) ────────────────
                let content_rect = Rect::from_min_size(full.min, Vec2::new(content_w, full.height()));
                ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
                    match self.sidebar {
                        SidebarSection::None => {}

                        // ── Info section ───────────────────────────────────────
                        SidebarSection::Info => {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(MI_DESCRIPTION).size(13.0).color(c.text_dim));
                                ui.add_space(3.0);
                                ui.label(RichText::new("INFO").color(c.text_muted).size(10.0).strong());
                            });
                            ui.add_space(8.0);

                            let tab_id = self.state.active_tab().id;
                            let selected = {
                                let ui_state = self.tab_ui.entry(tab_id).or_default();
                                ui_state.main_sel
                                    .and_then(|i| ui_state.main.entries.get(i))
                                    .cloned()
                            };

                            if let Some(entry) = selected {
                                let icon = mi_entry_icon(entry.kind, &entry.path);
                                let icon_color = match entry.kind {
                                    EntryKind::Directory => c.accent,
                                    EntryKind::Symlink   => c.accent_dim,
                                    _                    => c.text_dim,
                                };
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    ui.label(RichText::new(icon).size(34.0).color(icon_color));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(truncate_name(&entry.name, 20))
                                        .color(c.text).size(12.0).strong());
                                });
                                ui.add_space(8.0);
                                let lc = c.text_muted;
                                let vc = c.text_dim;
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Type").color(lc).size(10.5));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(8.0);
                                        ui.label(RichText::new(entry_kind_label(entry.kind)).color(vc).size(10.5));
                                    });
                                });
                                if let (EntryKind::File, Some(size)) = (entry.kind, entry.size_bytes) {
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Size").color(lc).size(10.5));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new(format_size(size)).color(vc).size(10.5));
                                        });
                                    });
                                }
                                if let Some(secs) = entry.modified_unix_secs {
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Modified").color(lc).size(10.5));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new(format_modified(secs)).color(vc).size(10.5));
                                        });
                                    });
                                }
                                ui.add_space(8.0);
                                ui.add(egui::Separator::default().horizontal().spacing(0.0));
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(entry.path.display().to_string())
                                        .color(c.text_muted).size(9.5));
                                });
                            } else {
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    ui.add_space(32.0);
                                    ui.label(RichText::new(MI_DESCRIPTION).size(30.0).color(c.text_muted));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("No selection").color(c.text_muted).size(11.0));
                                });
                            }
                        }

                        // ── Target section ─────────────────────────────────────
                        SidebarSection::Target => {
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(MI_DRIVE_FOLDER_UPLOAD).size(13.0).color(c.text_dim));
                                ui.add_space(3.0);
                                ui.label(RichText::new("TARGET").color(c.text_muted).size(10.0).strong());
                            });
                            ui.add_space(6.0);

                            let card_fill = if target_set {
                                Color32::from_rgba_unmultiplied(77, 142, 240, 18)
                            } else {
                                c.panel_raised
                            };
                            let card_stroke = if target_set {
                                Stroke::new(1.0, Color32::from_rgba_unmultiplied(77, 142, 240, 80))
                            } else {
                                Stroke::new(1.0, c.border)
                            };
                            Frame::new()
                                .fill(card_fill)
                                .stroke(card_stroke)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if target_set {
                                        let path_str = self.state.config.target.current
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_default();
                                        let folder_name = std::path::Path::new(&path_str)
                                            .file_name().and_then(|n| n.to_str()).unwrap_or(&path_str);
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(MI_DRIVE_FOLDER_UPLOAD).size(24.0).color(c.accent));
                                            ui.vertical(|ui| {
                                                ui.add_space(2.0);
                                                ui.label(RichText::new(truncate_name(folder_name, 16))
                                                    .color(c.text).size(12.0).strong());
                                                let parent = std::path::Path::new(&path_str)
                                                    .parent().map(|p| p.display().to_string())
                                                    .unwrap_or_default();
                                                ui.label(RichText::new(truncate_name(&parent, 20))
                                                    .color(c.text_muted).size(10.5))
                                                    .on_hover_text(&path_str);
                                            });
                                        });
                                        ui.add_space(6.0);
                                        let cur_dir = self.state.active_tab().current_dir.clone();
                                        let cur_name = cur_dir.file_name().and_then(|n| n.to_str()).unwrap_or("here");
                                        if ui.add(
                                            egui::Button::new(RichText::new(
                                                format!("{} Set to: {}", MI_CHECK, truncate_name(cur_name, 12))
                                            ).size(11.5).color(c.text))
                                                .min_size(Vec2::new(ui.available_width(), 26.0))
                                                .corner_radius(4.0)
                                        ).on_hover_text(format!("Set target to: {}", cur_dir.display())).clicked() {
                                            self.state.config.target.set(cur_dir);
                                        }
                                    } else {
                                        ui.vertical_centered(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new(MI_DRIVE_FOLDER_UPLOAD).size(28.0).color(c.text_muted));
                                            ui.label(RichText::new("No target set").color(c.text_muted).size(11.0));
                                            ui.add_space(6.0);
                                            let cur_dir = self.state.active_tab().current_dir.clone();
                                            if ui.add(
                                                egui::Button::new(RichText::new(
                                                    format!("{} Set Target", MI_DRIVE_FOLDER_UPLOAD)
                                                ).size(12.0).color(c.text))
                                                    .min_size(Vec2::new(ui.available_width() - 8.0, 28.0))
                                                    .corner_radius(4.0)
                                            ).on_hover_text(format!("Set: {}", cur_dir.display())).clicked() {
                                                self.state.config.target.set(cur_dir);
                                            }
                                            ui.add_space(4.0);
                                            ui.label(RichText::new("Set a destination folder\nfor copy & move ops.")
                                                .color(c.text_muted).size(10.5));
                                        });
                                    }
                                });
                            ui.add_space(8.0);

                            if !self.state.config.target.recent.is_empty() {
                                ui.label(RichText::new("RECENT").color(c.text_muted).size(9.5).strong());
                                ui.add_space(4.0);
                                let recents = self.state.config.target.recent.clone();
                                let mut chosen: Option<PathBuf> = None;
                                for path in &recents {
                                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("…");
                                    let is_current = self.state.config.target.current.as_ref() == Some(path);
                                    let col = if is_current { c.accent } else { c.text_dim };
                                    let icon = if is_current { MI_CHECK } else { MI_FOLDER };
                                    let resp = ui.add(
                                        egui::Button::new(
                                            RichText::new(format!("{} {}", icon, truncate_name(name, 16)))
                                                .size(12.0).color(col)
                                        )
                                        .frame(is_current)
                                        .fill(if is_current { Color32::from_rgba_unmultiplied(77, 142, 240, 25) } else { Color32::TRANSPARENT })
                                        .corner_radius(4.0)
                                        .min_size(Vec2::new(ui.available_width(), 26.0))
                                    );
                                    let clicked = resp.clicked();
                                    resp.on_hover_text(path.display().to_string());
                                    if clicked { chosen = Some(path.clone()); }
                                }
                                if let Some(path) = chosen {
                                    self.state.config.target.set(path);
                                }
                            }
                        }
                    }
                });
            });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let c = self.colors;
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(26.0)
            .frame(Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(12, 0)))
            .show(ctx, |ui| {
                ui.set_height(26.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Command frame toggle — LEFT side (bar opens at the bottom-left)
                    let cf_color = if self.command_frame.visible { c.accent } else { c.text_muted };
                    let cf_resp = ui.add(
                        egui::Button::new(RichText::new(MI_TERMINAL).size(16.0).color(cf_color))
                            .frame(false).min_size(Vec2::splat(24.0))
                    );
                    if cf_resp.clicked() {
                        self.command_frame.visible = !self.command_frame.visible;
                        if self.command_frame.visible {
                            self.command_frame.request_focus = true;
                        }
                    }
                    cf_resp.on_hover_text("Toggle command bar  (or type any key)");
                    ui.add_space(4.0);

                    // Item count + selection name
                    let tab_id = self.state.active_tab().id;
                    let count = self.tab_ui.get(&tab_id)
                        .map(|s| s.main.entries.len())
                        .unwrap_or(0);
                    let sel_name = self.tab_ui.get(&tab_id)
                        .and_then(|s| s.main_sel)
                        .and_then(|i| self.tab_ui.get(&tab_id)?.main.entries.get(i))
                        .map(|e| e.name.clone());
                    let status = if let Some(name) = sel_name {
                        format!("{} items  ·  {}", count, name)
                    } else {
                        format!("{} items", count)
                    };
                    ui.label(RichText::new(status).color(c.text_muted).size(11.5));

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);
                        // Hidden files toggle — right side
                        let (hidden_icon, hidden_color) = if self.state.config.show_hidden_files {
                            (MI_VISIBILITY, c.accent)
                        } else {
                            (MI_VISIBILITY_OFF, c.text_muted)
                        };
                        let hid_resp = ui.add(
                            egui::Button::new(RichText::new(hidden_icon).size(16.0).color(hidden_color))
                                .frame(false).min_size(Vec2::splat(24.0))
                        );
                        if hid_resp.clicked() {
                            self.state.config.show_hidden_files = !self.state.config.show_hidden_files;
                            self.refresh_active_tab();
                        }
                        hid_resp.on_hover_text(if self.state.config.show_hidden_files {
                            "Hide hidden files"
                        } else {
                            "Show hidden files"
                        });
                    });
                });
            });
    }

    fn render_command_frame(&mut self, ctx: &Context) {
        if !self.command_frame.visible && !self.state.config.command_frame_always_visible {
            return;
        }
        let c = self.colors;

        // Tab completion: use a stable widget ID so we can check focus before
        // consuming the key. egui's default Tab handling moves focus; we must
        // consume it first to prevent that when the TextEdit is focused.
        let cmd_input_id = egui::Id::new("cmd_input_field");
        let cmd_focused = ctx.memory(|m| m.has_focus(cmd_input_id));
        let tab_pressed = if self.command_frame.visible && cmd_focused {
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab))
        } else {
            false
        };

        egui::TopBottomPanel::bottom("command_frame")
            .resizable(false)
            .max_height(120.0)
            .frame(
                Frame::new()
                    .fill(c.panel_raised)
                    .stroke(Stroke::new(1.0, c.border))
                    .inner_margin(egui::Margin::symmetric(12, 6))
            )
            .show(ctx, |ui| {
                // Error or informational message
                if let Some(err) = &self.command_frame.error.clone() {
                    ui.label(RichText::new(err).color(c.error).size(11.5));
                } else if let Some(msg) = &self.command_frame.message.clone() {
                    ui.label(RichText::new(msg).color(c.accent).size(11.5));
                }

                // Completions hint (shown after Tab press)
                if !self.command_frame.completions.is_empty() {
                    let hint = self.command_frame.completions
                        .iter().take(6)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("   ");
                    ui.label(
                        RichText::new(format!("Tab>  {}", hint))
                            .color(c.text_muted)
                            .size(11.0)
                    );
                }

                // Input line
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(">").color(c.accent).size(13.0));
                    ui.add_space(4.0);

                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.command_frame.input)
                            .id(cmd_input_id)
                            .font(FontId::monospace(13.0))
                            .text_color(c.text)
                            .frame(false)
                            .desired_width(ui.available_width())
                            .hint_text("type a command — or 'help' for a list")
                    );

                    if self.command_frame.request_focus {
                        resp.request_focus();
                        self.command_frame.request_focus = false;
                    }

                    // Tab: compute and apply first completion.
                    // tab_pressed was already gated on cmd_focused (verified before panel render),
                    // so we don't need resp.has_focus() here.
                    if tab_pressed {
                        // Compute fresh completions on Tab press
                        self.update_completions();
                        if let Some(comp) = self.command_frame.completions.first().cloned() {
                            let parts: Vec<&str> = self.command_frame.input.trim().splitn(2, ' ').collect();
                            let prefix = parts.first().copied().unwrap_or("").to_string();
                            self.command_frame.input = format!("{} {}", prefix, comp);
                            // Refresh list for the updated input
                            self.update_completions();
                        }
                        self.command_frame.request_focus = true;
                        // Move the TextEdit cursor to end of the completed string so the user
                        // can continue typing from the end rather than the old cursor position.
                        let char_count = self.command_frame.input.chars().count();
                        let cursor = CCursor::new(char_count);
                        let range = CCursorRange::one(cursor);
                        let mut te_state = egui::text_edit::TextEditState::load(ui.ctx(), cmd_input_id)
                            .unwrap_or_default();
                        te_state.cursor.set_char_range(Some(range));
                        te_state.store(ui.ctx(), cmd_input_id);
                    }

                    // Enter executes, Escape closes.
                    if resp.lost_focus() {
                        if ui.input(|i| i.key_pressed(Key::Enter)) {
                            self.execute_command_input();
                        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                            self.command_frame.visible = false;
                            self.command_frame.error = None;
                            self.command_frame.completions.clear();
                        } else {
                            // Anything else (unlikely now) — keep focus here
                            self.command_frame.request_focus = true;
                        }
                    }

                    // History navigation with ↑ / ↓
                    if resp.has_focus() {
                        let (up, down) = ctx.input(|i| (
                            i.key_pressed(Key::ArrowUp),
                            i.key_pressed(Key::ArrowDown),
                        ));
                        if up && !self.command_frame.history.is_empty() {
                            let max = self.command_frame.history.len() - 1;
                            let idx = self.command_frame.history_idx
                                .map(|i| i.saturating_sub(1))
                                .unwrap_or(max);
                            self.command_frame.history_idx = Some(idx);
                            self.command_frame.input = self.command_frame.history[idx].clone();
                        }
                        if down {
                            if let Some(idx) = self.command_frame.history_idx {
                                if idx + 1 < self.command_frame.history.len() {
                                    let next = idx + 1;
                                    self.command_frame.history_idx = Some(next);
                                    self.command_frame.input = self.command_frame.history[next].clone();
                                } else {
                                    self.command_frame.history_idx = None;
                                    self.command_frame.input.clear();
                                }
                            }
                        }

                        // Clear error on any edit; do NOT auto-update completions
                        // (they are computed lazily on Tab only)
                        if resp.changed() {
                            self.command_frame.error = None;
                            self.command_frame.message = None;
                            self.command_frame.completions.clear();
                        }
                    }
                });
            });
    }

    fn update_completions(&mut self) {
        let input = self.command_frame.input.trim().to_string();
        let cwd = self.state.active_tab().current_dir.clone();
        self.command_frame.completions = compute_path_completions(&input, &cwd);
    }

    fn render_file_pane(&mut self, ctx: &Context) {
        let c = self.colors;
        let view_mode = self.state.active_tab().view_mode;

        // Scale handle — only shown in Grid view
        if matches!(view_mode, ViewMode::Grid) {
            egui::TopBottomPanel::bottom("scale_bar")
                .exact_height(22.0)
                .frame(Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(12, 0)))
                .show(ctx, |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.label(RichText::new(MI_TUNE).size(13.0).color(c.text_muted));
                        ui.add_space(6.0);
                        ui.add(
                            egui::Slider::new(&mut self.icon_scale, 0.5_f32..=2.5)
                                .show_value(false)
                                .trailing_fill(true)
                        );
                    });
                });
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(c.bg))
            .show(ctx, |ui| {
                let tab_id = self.state.active_tab().id;
                match view_mode {
                    ViewMode::Miller => self.render_miller(ui, tab_id),
                    ViewMode::List   => self.render_list(ui, tab_id),
                    ViewMode::Grid   => self.render_grid(ui, tab_id),
                }
            });
    }

    fn render_miller(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let full = ui.available_rect_before_wrap();
        let col_w = (full.width() - 2.0) / 3.0; // 2px for two 1px separators
        let h = full.height();

        // Pre-collect data to avoid borrow issues
        let (left_entries, left_sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (s.left.entries.clone(), s.left_sel)
        };
        let (main_entries, main_loading, main_err, main_sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (s.main.entries.clone(), s.main.loading, s.main.error.clone(), s.main_sel)
        };
        let (right_entries, right_loading, right_is_file, right_dir) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (s.right.entries.clone(), s.right.loading, s.right_is_file, s.right.dir.clone())
        };
        let epoch = self.tab_ui.entry(tab_id).or_default().scroll_epoch;

        // ── Left column ───────────────────────────────────────────────────────
        let left_rect = Rect::from_min_size(full.min, Vec2::new(col_w, h));
        ui.scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
            ScrollArea::vertical()
                .id_salt(("miller_left", tab_id, epoch))
                .show(ui, |ui| {
                    ui.set_width(col_w);
                    ui.set_min_height(h);
                    for (i, entry) in left_entries.iter().enumerate() {
                        let selected = left_sel == Some(i);
                        let resp = self.render_file_row(ui, entry, selected, false, i, &c);
                        if resp.clicked() {
                            let path = entry.path.clone();
                            let kind = entry.kind;
                            if matches!(kind, EntryKind::Directory) {
                                self.navigate_tab_to(tab_id, path);
                            }
                            return;
                        }
                    }
                });
        });

        // Separator
        let sep_x1 = full.min.x + col_w;
        ui.painter().vline(sep_x1, full.y_range(), Stroke::new(1.0, c.border));

        // ── Middle column (active) ─────────────────────────────────────────────
        let mid_rect = Rect::from_min_size(
            egui::Pos2::new(full.min.x + col_w + 1.0, full.min.y),
            Vec2::new(col_w, h),
        );
        ui.scope_builder(UiBuilder::new().max_rect(mid_rect), |ui| {
            ScrollArea::vertical()
                .id_salt(("miller_mid", tab_id, epoch))
                .show(ui, |ui| {
                    ui.set_width(col_w);
                    ui.set_min_height(h);
                    if main_loading {
                        ui.add_space(16.0);
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            ui.label(RichText::new("Loading…").color(c.text_muted).size(12.0));
                        });
                        return;
                    }
                    if let Some(err) = &main_err {
                        ui.add_space(16.0);
                        ui.label(RichText::new(err).color(c.error).size(12.0));
                        return;
                    }
                    if main_entries.is_empty() {
                        ui.add_space(16.0);
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            ui.label(RichText::new("Empty folder").color(c.text_muted).size(12.0));
                        });
                        return;
                    }
                    let mut add_bookmark: Option<(String, String, PathBuf)> = None;
                    let mut show_properties: Option<PathBuf> = None;
                    let mut create_folder_here = false;
                    let mut create_file_here = false;
                    let mut open_from_menu: Option<(PathBuf, EntryKind, usize)> = None;
                    let mut copy_clipboard_from_menu: Option<(PathBuf, bool)> = None; // (path, cut)
                    let mut transfer_to_target_from_menu: Option<(PathBuf, PathBuf, bool)> = None; // (src, dst_dir, move?)
                    let mut rename_from_menu: Option<String> = None;
                    let mut delete_from_menu: Option<PathBuf> = None;
                    for (i, entry) in main_entries.iter().enumerate() {
                        let selected = main_sel == Some(i);
                        let resp = self.render_file_row(ui, entry, selected, true, i, &c);
                        // Right-click context menu
                        let has_target = self.state.config.target.current.is_some();
                        let target_path = self.state.config.target.current.clone();
                        let entry_path_cm = entry.path.clone();
                        let entry_kind_cm = entry.kind;
                        let entry_name_cm = entry.name.clone();
                        let is_dir_cm = matches!(entry_kind_cm, EntryKind::Directory | EntryKind::Symlink);
                        let already_bm = self.state.config.bookmarks.iter().any(|(_, _, p)| p == &entry_path_cm);

                        resp.context_menu(|ui| {
                            ui.set_min_width(200.0);
                            ui.style_mut().spacing.item_spacing.y = 0.0;

                            // ── Open ──────────────────────────────────────────
                            let open_label = if is_dir_cm {
                                format!("{} Open folder", MI_FOLDER_OPEN)
                            } else {
                                format!("{} Open", MI_OPEN)
                            };
                            if ui.add(egui::Button::new(RichText::new(&open_label).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                open_from_menu = Some((entry_path_cm.clone(), entry_kind_cm, i));
                                ui.close();
                            }

                            // ── Bookmark (folders only) ────────────────────────
                            if is_dir_cm {
                                let bm_label = if already_bm {
                                    format!("{} Remove from bookmarks", MI_BOOKMARK)
                                } else {
                                    format!("{} Add to bookmarks", MI_BOOKMARK)
                                };
                                if ui.add(egui::Button::new(RichText::new(&bm_label).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                    if already_bm {
                                        self.state.config.bookmarks.retain(|(_, _, p)| p != &entry_path_cm);
                                    } else {
                                        add_bookmark = Some((
                                            MI_FOLDER.to_string(),
                                            entry_name_cm.clone(),
                                            entry_path_cm.clone(),
                                        ));
                                    }
                                    ui.close();
                                }
                            }

                            ui.add(egui::Separator::default().horizontal().spacing(4.0));

                            // ── Clipboard ─────────────────────────────────────
                            if ui.add(egui::Button::new(RichText::new(format!("{} Copy", MI_COPY)).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                copy_clipboard_from_menu = Some((entry_path_cm.clone(), false));
                                ui.close();
                            }
                            if ui.add(egui::Button::new(RichText::new(format!("{} Cut", MI_CUT)).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                copy_clipboard_from_menu = Some((entry_path_cm.clone(), true));
                                ui.close();
                            }

                            // ── Copy/Move to target ───────────────────────────
                            if has_target {
                                if let Some(ref tp) = target_path {
                                    let tname = truncate_name(tp.file_name().and_then(|n| n.to_str()).unwrap_or("…"), 18);
                                    let full = tp.display().to_string();
                                    ui.add(egui::Separator::default().horizontal().spacing(4.0));
                                    ui.label(RichText::new(format!("Target: {}", tname)).size(10.5).color(Color32::from_rgb(160, 160, 160)));
                                    if ui.add(egui::Button::new(RichText::new(format!("{} Copy to target", MI_COPY)).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).on_hover_text(&full).clicked() {
                                        transfer_to_target_from_menu = Some((entry_path_cm.clone(), tp.clone(), false));
                                        ui.close();
                                    }
                                    if ui.add(egui::Button::new(RichText::new(format!("{} Move to target", MI_MOVE)).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).on_hover_text(&full).clicked() {
                                        transfer_to_target_from_menu = Some((entry_path_cm.clone(), tp.clone(), true));
                                        ui.close();
                                    }
                                }
                            }

                            ui.add(egui::Separator::default().horizontal().spacing(4.0));

                            // ── File operations ───────────────────────────────
                            if ui.add(egui::Button::new(RichText::new(format!("{} Rename", MI_EDIT)).size(13.0)).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                rename_from_menu = Some(entry_name_cm.clone());
                                ui.close();
                            }
                            if ui.add(
                                egui::Button::new(RichText::new(format!("{} Delete", MI_DELETE)).color(Color32::from_rgb(220, 80, 70)).size(13.0))
                                    .frame(false)
                                    .min_size(Vec2::new(200.0, 28.0))
                            ).clicked() {
                                delete_from_menu = Some(entry_path_cm.clone());
                                ui.close();
                            }

                            ui.add(egui::Separator::default().horizontal().spacing(4.0));

                            // ── Create in current folder ──────────────────────
                            if ui.add(egui::Button::new(
                                RichText::new(format!("{} New folder here", MI_CREATE_FOLDER)).size(13.0)
                            ).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                create_folder_here = true;
                                ui.close();
                            }
                            if ui.add(egui::Button::new(
                                RichText::new(format!("{} New file here", MI_FILE)).size(13.0)
                            ).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                create_file_here = true;
                                ui.close();
                            }

                            // ── Properties ────────────────────────────────────
                            if ui.add(egui::Button::new(
                                RichText::new(format!("{} Properties", MI_INFO)).size(13.0)
                            ).frame(false).min_size(Vec2::new(200.0, 28.0))).clicked() {
                                show_properties = Some(entry_path_cm.clone());
                                ui.close();
                            }
                        });
                        if resp.double_clicked() {
                            let path = entry.path.clone();
                            let kind = entry.kind;
                            self.tab_ui.entry(tab_id).or_default().main_sel = Some(i);
                            if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                                self.navigate_tab_to(tab_id, path);
                            } else {
                                let _ = self.platform.reveal_in_system(&path);
                            }
                            return;
                        } else if resp.clicked() {
                            self.tab_ui.entry(tab_id).or_default().main_sel = Some(i);
                            self.load_miller_right(tab_id);
                        }
                    }
                    // Apply deferred context-menu actions
                    if let Some((icon, name, path)) = add_bookmark {
                        self.state.config.bookmarks.push((icon, name, path));
                    }
                    if let Some(path) = show_properties {
                        self.properties.visible = true;
                        self.properties.path = Some(path);
                    }
                    if let Some((path, cut)) = copy_clipboard_from_menu {
                        self.clipboard = Some(Clipboard { sources: vec![path], cut });
                    }
                    if let Some((src, dst_dir, is_move)) = transfer_to_target_from_menu {
                        let cmd = if is_move {
                            FileCommand::Move {
                                sources: vec![src],
                                destination: dst_dir,
                                conflict: ConflictAction::Rename,
                            }
                        } else {
                            FileCommand::Copy {
                                sources: vec![src],
                                destination: dst_dir,
                                conflict: ConflictAction::Rename,
                            }
                        };
                        self.run_file_op(cmd, Some(tab_id));
                    }
                    if let Some(name) = rename_from_menu {
                        self.command_frame.input = format!("mv {} ", name);
                        self.command_frame.visible = true;
                        self.command_frame.request_focus = true;
                    }
                    if let Some(path) = delete_from_menu {
                        let cmd = FileCommand::Delete {
                            targets: vec![path],
                            mode: DeleteMode::Trash,
                        };
                        self.run_file_op(cmd, Some(tab_id));
                    }
                    if let Some((path, kind, idx)) = open_from_menu {
                        self.tab_ui.entry(tab_id).or_default().main_sel = Some(idx);
                        if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                            self.navigate_tab_to(tab_id, path);
                        } else {
                            let _ = self.platform.reveal_in_system(&path);
                        }
                    }
                    if create_folder_here {
                        self.command_frame.input = "mkdir ".to_string();
                        self.command_frame.visible = true;
                        self.command_frame.request_focus = true;
                    }
                    if create_file_here {
                        self.command_frame.input = "touch ".to_string();
                        self.command_frame.visible = true;
                        self.command_frame.request_focus = true;
                    }
                });
        });

        // Separator
        let sep_x2 = full.min.x + 2.0 * col_w + 1.0;
        ui.painter().vline(sep_x2, full.y_range(), Stroke::new(1.0, c.border));

        // ── Right column (preview/child) ──────────────────────────────────────
        let right_rect = Rect::from_min_size(
            egui::Pos2::new(full.min.x + 2.0 * (col_w + 1.0), full.min.y),
            Vec2::new(full.max.x - full.min.x - 2.0 * (col_w + 1.0), h),
        );
        ui.scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
            if right_is_file {
                // File detail panel — icon, name, size, modified date
                ui.add_space(24.0);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    let icon = file_type_mi_icon(&right_dir);
                    ui.label(RichText::new(icon).size(52.0).color(c.text_dim));
                    ui.add_space(10.0);
                    let name = right_dir.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("—");
                    ui.label(RichText::new(name).color(c.text).size(13.5).strong());
                    ui.add_space(6.0);
                    // File metadata
                    if let Ok(meta) = std::fs::metadata(&right_dir) {
                        if meta.is_file() {
                            let size_str = format_size(meta.len());
                            ui.label(RichText::new(size_str).color(c.text_dim).size(12.0));
                        }
                        if let Ok(modified) = meta.modified() {
                            use std::time::UNIX_EPOCH;
                            if let Ok(dur) = modified.duration_since(UNIX_EPOCH) {
                                let mod_str = format_modified(dur.as_secs());
                                ui.add_space(2.0);
                                ui.label(RichText::new(mod_str).color(c.text_muted).size(11.5));
                            }
                        }
                    }
                    ui.add_space(12.0);
                    // File type label
                    let ext = right_dir.extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    if !ext.is_empty() {
                        ui.label(RichText::new(format!("{} file", ext)).color(c.text_muted).size(11.0));
                    }
                    ui.add_space(16.0);
                    ui.label(RichText::new("Space to preview  ·  Enter to open").color(c.text_muted).size(10.5));
                });
                return;
            }
            ScrollArea::vertical()
                .id_salt(("miller_right", tab_id, epoch))
                .show(ui, |ui| {
                    ui.set_min_height(h);
                    if right_loading {
                        ui.add_space(16.0);
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            ui.label(RichText::new("Loading…").color(c.text_muted).size(12.0));
                        });
                        return;
                    }
                    if right_entries.is_empty() {
                        ui.add_space(16.0);
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            let msg = if right_dir == PathBuf::default() {
                                "Select a folder to preview"
                            } else {
                                "Empty folder"
                            };
                            ui.label(RichText::new(msg).color(c.text_muted).size(12.0));
                        });
                        return;
                    }
                    for (i, entry) in right_entries.iter().enumerate() {
                        let resp = self.render_file_row(ui, entry, false, false, i, &c);
                        if resp.clicked() {
                            if matches!(entry.kind, EntryKind::Directory | EntryKind::Symlink) {
                                let path = entry.path.clone();
                                self.navigate_tab_to(tab_id, path);
                            }
                            return;
                        }
                        if i >= 200 { // guard against enormous dirs in preview col
                            ui.label(RichText::new("…").color(c.text_muted).size(11.0));
                            break;
                        }
                    }
                });
        });
    }

    fn render_list(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let (entries, loading, error, sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (s.main.entries.clone(), s.main.loading, s.main.error.clone(), s.main_sel)
        };

        if loading {
            ui.add_space(32.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.label(RichText::new("Loading…").color(c.text_muted));
            });
            return;
        }
        if let Some(err) = error {
            ui.label(RichText::new(err).color(c.error));
            return;
        }

        // Column headers
        let header_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 26.0));
        ui.allocate_rect(header_rect, Sense::hover());
        ui.painter().rect_filled(header_rect, 0.0, c.panel_raised);
        let col_widths = [0.45, 0.12, 0.14, 0.18]; // Name, Kind, Size, Modified fractions
        let total_w = header_rect.width() - 16.0;
        let mut x = header_rect.min.x + 8.0;
        for (label, frac) in ["Name", "Kind", "Size", "Modified"].iter().zip(&col_widths) {
            let w = total_w * frac;
            ui.painter().text(
                egui::Pos2::new(x + 4.0, header_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.5),
                c.text_muted,
            );
            x += w;
        }
        ui.add_space(26.0);

        // Rows
        ScrollArea::vertical()
            .id_salt(("list_view", tab_id))
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.add_space(24.0);
                    ui.with_layout(Layout::top_down(Align::Center), |ui| {
                        ui.label(RichText::new("Empty folder").color(c.text_muted));
                    });
                    return;
                }
                for (i, entry) in entries.iter().enumerate() {
                    let selected = sel == Some(i);
                    let row_h = 28.0;
                    let row_rect = Rect::from_min_size(
                        ui.cursor().min,
                        Vec2::new(ui.available_width(), row_h),
                    );
                    let row_fill = if selected {
                        c.selected_bg
                    } else if i % 2 == 0 {
                        Color32::TRANSPARENT
                    } else {
                        c.row_alt
                    };
                    if row_fill != Color32::TRANSPARENT {
                        ui.painter().rect_filled(row_rect, 0.0, row_fill);
                    }

                    let response = ui.allocate_rect(row_rect, Sense::click());
                    if response.hovered() && !selected {
                        ui.painter().rect_filled(row_rect, 0.0, c.hover);
                    }
                    if response.clicked() {
                        let ui_s = self.tab_ui.entry(tab_id).or_default();
                        ui_s.main_sel = Some(i);
                    }
                    if response.double_clicked() {
                        let path = entry.path.clone();
                        let kind = entry.kind;
                        if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                            self.navigate_tab_to(tab_id, path);
                        } else {
                            let _ = self.platform.reveal_in_system(&path);
                        }
                        return;
                    }

                    // Row content
                    let total_w = row_rect.width() - 16.0;
                    let mut x = row_rect.min.x + 8.0;
                    let y = row_rect.center().y;

                    // Material Icon for entry type
                    let icon_str = mi_entry_icon(entry.kind, &entry.path);
                    let icon_color = match entry.kind {
                        EntryKind::Directory => c.accent,
                        EntryKind::Symlink   => c.accent_dim,
                        _                    => c.text_muted,
                    };
                    ui.painter().text(
                        egui::Pos2::new(x + 6.0, y),
                        Align2::LEFT_CENTER,
                        icon_str,
                        FontId::proportional(16.0),
                        icon_color,
                    );

                    // Name
                    let name_w = total_w * col_widths[0];
                    ui.painter().text(
                        egui::Pos2::new(x + 28.0, y),
                        Align2::LEFT_CENTER,
                        &entry.name,
                        FontId::proportional(13.0),
                        c.text,
                    );
                    x += name_w;

                    // Kind
                    let kind_str = entry_kind_label(entry.kind);
                    ui.painter().text(
                        egui::Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        kind_str,
                        FontId::proportional(12.0),
                        c.text_dim,
                    );
                    x += total_w * col_widths[1];

                    // Size
                    let size_str = match entry.kind {
                        EntryKind::Directory => "—".to_string(),
                        _ => entry.size_bytes.map(format_size).unwrap_or_default(),
                    };
                    ui.painter().text(
                        egui::Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        size_str,
                        FontId::proportional(12.0),
                        c.text_dim,
                    );
                    x += total_w * col_widths[2];

                    // Modified
                    let mod_str = entry.modified_unix_secs
                        .map(format_modified)
                        .unwrap_or_default();
                    ui.painter().text(
                        egui::Pos2::new(x, y),
                        Align2::LEFT_CENTER,
                        mod_str,
                        FontId::proportional(12.0),
                        c.text_dim,
                    );

                }
            });
    }

    fn render_grid(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let (entries, loading, sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (s.main.entries.clone(), s.main.loading, s.main_sel)
        };
        if loading {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("Loading…").color(c.text_muted));
            });
            return;
        }
        if entries.is_empty() {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("Empty folder").color(c.text_muted));
            });
            return;
        }

        // Icon size scales with icon_scale slider (1.0 = default 48px icon)
        let icon_px  = (48.0 * self.icon_scale).round();
        let label_sz = (11.0 * self.icon_scale.sqrt()).max(9.0);
        let label_chars = ((item_chars_for_scale(self.icon_scale)) as usize).max(6);
        // Cell width = icon + padding; ensure wide enough for truncated label
        let cell_inner = (icon_px * 1.15).max(76.0);
        let padding = (10.0 * self.icon_scale).round().max(8.0);
        let available_w = ui.available_width();
        let cols = ((available_w / (cell_inner + padding)) as usize).max(1);
        self.last_grid_cols = cols;

        ScrollArea::vertical().id_salt(("grid", tab_id)).show(ui, |ui| {
            ui.add_space(padding * 0.5);
            let mut i = 0;
            while i < entries.len() {
                ui.horizontal(|ui| {
                    ui.add_space(padding * 0.5);
                    for col in 0..cols {
                        if i + col >= entries.len() { break; }
                        let entry = &entries[i + col];
                        let idx = i + col;
                        let selected = sel == Some(idx);
                        let icon = mi_entry_icon(entry.kind, &entry.path);
                        let icon_color = match entry.kind {
                            EntryKind::Directory => c.accent,
                            EntryKind::Symlink   => c.accent_dim,
                            _                    => c.text_dim,
                        };
                        let name = truncate_name(&entry.name, label_chars);

                        // Allocate the entire cell rect for hover/click sensing
                        let cell_h = icon_px + label_sz * 2.0 + 16.0;
                        let (cell_rect, cell_resp) = ui.allocate_exact_size(
                            Vec2::new(cell_inner, cell_h), Sense::click()
                        );
                        let is_hovered = cell_resp.hovered();

                        // Background
                        let fill = if selected {
                            c.selected_bg
                        } else if is_hovered {
                            c.hover
                        } else {
                            Color32::TRANSPARENT
                        };
                        if fill != Color32::TRANSPARENT {
                            ui.painter().rect_filled(cell_rect, 8.0, fill);
                        }
                        if selected {
                            ui.painter().rect_stroke(
                                cell_rect, 8.0,
                                Stroke::new(1.0, c.accent),
                                egui::StrokeKind::Middle
                            );
                        }

                        // Icon centered horizontally in cell
                        let icon_y = cell_rect.min.y + 8.0;
                        let icon_cx = cell_rect.center().x;
                        ui.painter().text(
                            egui::Pos2::new(icon_cx, icon_y + icon_px * 0.5),
                            Align2::CENTER_CENTER,
                            icon,
                            FontId::proportional(icon_px),
                            icon_color,
                        );

                        // Label below icon
                        ui.painter().text(
                            egui::Pos2::new(icon_cx, icon_y + icon_px + 6.0),
                            Align2::CENTER_TOP,
                            &name,
                            FontId::proportional(label_sz),
                            if selected { c.text } else { c.text_dim },
                        );

                        // Handle click/double-click
                        if cell_resp.clicked() {
                            let ui_s = self.tab_ui.entry(tab_id).or_default();
                            ui_s.main_sel = Some(idx);
                        }
                        if cell_resp.double_clicked() {
                            let path = entry.path.clone();
                            let kind = entry.kind;
                            if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                                self.navigate_tab_to(tab_id, path);
                            } else {
                                let _ = self.platform.reveal_in_system(&path);
                            }
                        }

                        ui.add_space(padding);
                    }
                });
                i += cols;
                ui.add_space(padding * 0.25);
            }
        });
    }

    fn render_preview_overlay(&mut self, ctx: &Context) {
        if !self.preview.visible {
            return;
        }
        let c = self.colors;
        let path = match &self.preview.path {
            Some(p) => p.clone(),
            None => return,
        };
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("—");

        egui::Window::new("preview_overlay")
            .title_bar(false)
            .resizable(true)
            .collapsible(false)
            .default_width(600.0)
            .default_height(500.0)
            .frame(
                Frame::window(&ctx.style())
                    .fill(c.panel_raised)
                    .stroke(Stroke::new(1.0, c.border))
                    .corner_radius(10.0)
            )
            .show(ctx, |ui| {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // Header
                    ui.horizontal(|ui| {
                        let icon = file_icon_for_path(&path);
                        ui.label(RichText::new(icon).size(18.0));
                        ui.label(RichText::new(name).color(c.text).size(14.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(
                                egui::Label::new(
                                    RichText::new("X").color(c.text_muted).size(16.0)
                                ).sense(Sense::click())
                            ).clicked() {
                                self.preview.visible = false;
                            }
                        });
                    });
                    ui.separator();

                    ScrollArea::vertical().show(ui, |ui| {
                        match &self.preview.content {
                            Some(text) => {
                                let text = text.clone();
                                ui.add(
                                    egui::TextEdit::multiline(&mut text.as_str())
                                        .font(FontId::monospace(12.0))
                                        .text_color(c.text)
                                        .frame(false)
                                        .desired_rows(30)
                                );
                            }
                            None => {
                                ui.add_space(20.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    let icon = file_icon_for_path(&path);
                                    ui.label(RichText::new(icon).size(64.0));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(name).color(c.text).size(14.0));
                                    if let Some(size) = get_file_size(&path) {
                                        ui.label(RichText::new(format_size(size)).color(c.text_dim).size(12.0));
                                    }
                                    ui.add_space(16.0);
                                    if ui.button("Open with default app").clicked() {
                                        let _ = self.platform.reveal_in_system(&path);
                                    }
                                });
                            }
                        }
                    });
                });
            });
    }

    fn render_properties_popup(&mut self, ctx: &Context) {
        if !self.properties.visible { return; }
        let path = match self.properties.path.clone() {
            Some(p) => p,
            None => return,
        };
        let c = self.colors;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("—").to_string();
        let is_dir = path.is_dir();

        egui::Window::new("properties_popup")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(320.0)
            .frame(Frame::window(&ctx.style())
                .fill(c.panel_raised)
                .stroke(Stroke::new(1.0, c.border))
                .corner_radius(10.0))
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    let icon = if is_dir { MI_FOLDER } else { file_type_mi_icon(&path) };
                    let icon_col = if is_dir { c.accent } else { c.text_dim };
                    ui.label(RichText::new(icon).size(18.0).color(icon_col));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Properties").color(c.text).size(13.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Label::new(
                            RichText::new(MI_CLOSE).color(c.text_muted).size(16.0)
                        ).sense(Sense::click())).clicked() {
                            self.properties.visible = false;
                        }
                    });
                });
                ui.add(egui::Separator::default().horizontal().spacing(4.0));
                ui.add_space(4.0);

                // Metadata table
                let lc = c.text_muted;
                let vc = c.text_dim;

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Name").color(lc).size(11.5));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&name).color(vc).size(11.5));
                    });
                });
                if let Ok(meta) = std::fs::metadata(&path) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Type").color(lc).size(11.5));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(if meta.is_dir() { "Folder" } else { "File" }).color(vc).size(11.5));
                        });
                    });
                    if meta.is_file() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Size").color(lc).size(11.5));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(format_size(meta.len())).color(vc).size(11.5));
                            });
                        });
                    }
                    if let Ok(modified) = meta.modified() {
                        use std::time::UNIX_EPOCH;
                        if let Ok(dur) = modified.duration_since(UNIX_EPOCH) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Modified").color(lc).size(11.5));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(RichText::new(format_modified(dur.as_secs())).color(vc).size(11.5));
                                });
                            });
                        }
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = meta.permissions().mode() & 0o777;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Permissions").color(lc).size(11.5));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(format!("{:o}", mode)).color(vc).size(11.5));
                            });
                        });
                    }
                }
                ui.add_space(4.0);
                ui.add(egui::Separator::default().horizontal().spacing(4.0));
                ui.add_space(2.0);
                // Full path (word-wrapped)
                ui.label(RichText::new("Path").color(lc).size(10.5));
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(path.display().to_string()).color(vc).size(10.5));
                });
                ui.add_space(4.0);
            });
    }

    fn render_about_popup(&mut self, ctx: &Context) {
        if !self.show_about { return; }
        let c = self.colors;
        egui::Window::new("about_ottrin")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(280.0)
            .frame(Frame::window(&ctx.style())
                .fill(c.panel_raised)
                .stroke(Stroke::new(1.0, c.border))
                .corner_radius(12.0))
            .show(ctx, |ui| {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new(MI_FOLDER).size(44.0).color(c.accent));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Ottrin").color(c.text).size(22.0).strong());
                    ui.label(RichText::new("File Manager").color(c.text_muted).size(12.0));
                    ui.add_space(10.0);
                    ui.label(RichText::new("Version 0.1.0  ·  Pre-release").color(c.text_dim).size(11.5));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Built with Rust + egui 0.33").color(c.text_muted).size(10.5));
                    ui.add_space(12.0);
                    ui.add(egui::Separator::default().horizontal().spacing(0.0));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Author").color(c.text_muted).size(11.5));
                        ui.add_space(4.0);
                        ui.hyperlink_to(
                            RichText::new("hoozter").size(11.5),
                            "https://hoozter.com"
                        );
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(MI_CODE).size(13.0).color(c.text_muted));
                        ui.add_space(2.0);
                        ui.hyperlink_to(
                            RichText::new("github.com/hoozter/ottrin").size(11.0),
                            "https://github.com/hoozter/ottrin"
                        );
                    });
                    ui.add_space(16.0);
                    if ui.button("  Close  ").clicked() {
                        self.show_about = false;
                    }
                    ui.add_space(12.0);
                });
            });
    }

    fn render_settings_modal(&mut self, ctx: &Context) {
        if !self.show_settings { return; }
        let c = self.colors;

        egui::Window::new("settings_modal")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(520.0)
            .frame(Frame::window(&ctx.style())
                .fill(c.panel)
                .stroke(Stroke::new(1.0, c.border))
                .corner_radius(10.0)
                .inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                ui.set_min_width(520.0);
                ui.set_min_height(340.0);

                // ── Title bar ────────────────────────────────────────────────
                Frame::new()
                    .fill(c.panel_raised)
                    .inner_margin(egui::Margin { left: 16, right: 8, top: 10, bottom: 10 })
                    .show(ui, |ui| {
                        ui.set_min_width(520.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} Settings", MI_SETTINGS))
                                .color(c.text).size(14.0).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(egui::Button::new(
                                    RichText::new(MI_CLOSE).size(16.0).color(c.text_muted)
                                ).frame(false).min_size(Vec2::splat(26.0))).clicked() {
                                    self.show_settings = false;
                                }
                            });
                        });
                    });
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.cursor().min.y,
                    Stroke::new(1.0, c.border),
                );

                // ── Two-panel body ───────────────────────────────────────────
                let body_rect = ui.available_rect_before_wrap();
                let sidebar_w = 130.0;

                // Left sidebar
                let sidebar_rect = Rect::from_min_size(body_rect.min, Vec2::new(sidebar_w, body_rect.height().max(300.0)));
                ui.scope_builder(UiBuilder::new().max_rect(sidebar_rect), |ui| {
                    Frame::new()
                        .fill(c.panel_raised)
                        .inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            ui.set_min_height(300.0);
                            let categories = [
                                (SettingsTab::General,    MI_TUNE,        "General"),
                                (SettingsTab::Appearance, MI_DARK_MODE,   "Appearance"),
                                (SettingsTab::Files,      MI_FOLDER,      "Files"),
                                (SettingsTab::Search,     MI_SEARCH,      "Search"),
                                (SettingsTab::Cache,      MI_DESCRIPTION, "Cache"),
                            ];
                            for (tab, icon, label) in &categories {
                                let active = self.settings_tab == *tab;
                                let bg = if active { c.selected_bg } else { Color32::TRANSPARENT };
                                let text_col = if active { c.text } else { c.text_dim };
                                let resp = Frame::new()
                                    .fill(bg)
                                    .inner_margin(egui::Margin { left: 12, right: 8, top: 0, bottom: 0 })
                                    .show(ui, |ui| {
                                        ui.set_min_width(sidebar_w);
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(*icon).size(14.0).color(text_col));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(*label).size(13.0).color(text_col));
                                        });
                                        ui.add_space(2.0);
                                    });
                                let r = ui.interact(resp.response.rect, ui.id().with(label), Sense::click());
                                if r.clicked() { self.settings_tab = *tab; }
                                if active {
                                    ui.painter().vline(sidebar_rect.min.x + 2.0, resp.response.rect.y_range(), Stroke::new(2.0, c.accent));
                                }
                            }
                        });
                });

                // Separator line between sidebar and content
                let sep_x = body_rect.min.x + sidebar_w;
                ui.painter().vline(sep_x, body_rect.y_range(), Stroke::new(1.0, c.border));

                // Right content panel
                let content_rect = Rect::from_min_max(
                    egui::Pos2::new(sep_x + 1.0, body_rect.min.y),
                    body_rect.max,
                );
                ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
                    Frame::new()
                        .inner_margin(egui::Margin::symmetric(20, 16))
                        .show(ui, |ui| {
                            ui.set_min_width(content_rect.width() - 40.0);
                            match self.settings_tab {
                                SettingsTab::General => {
                                    ui.label(RichText::new("General").color(c.text).size(14.0).strong());
                                    ui.add_space(12.0);
                                    ui.label(RichText::new("Default view").color(c.text_dim).size(12.0));
                                    ui.add_space(6.0);
                                    let view_opts = [
                                        (ViewMode::Miller, "Miller columns"),
                                        (ViewMode::List,   "List"),
                                        (ViewMode::Grid,   "Grid"),
                                    ];
                                    for (mode, label) in &view_opts {
                                        let sel = self.state.config.default_view_mode == *mode;
                                        let icon = if sel { MI_CHECK } else { "  " };
                                        if ui.add(egui::Button::new(
                                            RichText::new(format!("{} {}", icon, label)).size(13.0)
                                                .color(if sel { c.text } else { c.text_dim })
                                        ).frame(false).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
                                            self.state.config.default_view_mode = *mode;
                                        }
                                    }
                                }
                                SettingsTab::Appearance => {
                                    ui.label(RichText::new("Appearance").color(c.text).size(14.0).strong());
                                    ui.add_space(12.0);
                                    ui.label(RichText::new("Theme").color(c.text_dim).size(12.0));
                                    ui.add_space(6.0);
                                    let themes = [
                                        (ThemeMode::Dark,   MI_DARK_MODE,  "Dark"),
                                        (ThemeMode::Light,  MI_LIGHT_MODE, "Light"),
                                        (ThemeMode::System, MI_TUNE,       "System"),
                                    ];
                                    for (mode, icon, label) in &themes {
                                        let sel = self.state.config.theme == *mode;
                                        let check = if sel { MI_CHECK } else { "  " };
                                        if ui.add(egui::Button::new(
                                            RichText::new(format!("{} {} {}", check, icon, label)).size(13.0)
                                                .color(if sel { c.text } else { c.text_dim })
                                        ).frame(false).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
                                            self.state.config.theme = *mode;
                                            self.colors = Colors::for_theme(*mode);
                                            self.apply_theme_to_ctx(ctx);
                                        }
                                    }
                                }
                                SettingsTab::Files => {
                                    ui.label(RichText::new("Files").color(c.text).size(14.0).strong());
                                    ui.add_space(12.0);
                                    let show_hidden = self.state.config.show_hidden_files;
                                    let icon = if show_hidden { MI_CHECK } else { "  " };
                                    if ui.add(egui::Button::new(
                                        RichText::new(format!("{} Show hidden files", icon)).size(13.0)
                                            .color(if show_hidden { c.text } else { c.text_dim })
                                    ).frame(false).min_size(Vec2::new(ui.available_width(), 26.0))).clicked() {
                                        self.state.config.show_hidden_files = !show_hidden;
                                        self.refresh_active_tab();
                                    }
                                }
                                SettingsTab::Search => {
                                    ui.label(RichText::new("Search").color(c.text).size(14.0).strong());
                                    ui.add_space(12.0);
                                    ui.label(RichText::new("Advanced search settings will appear here once search is implemented.")
                                        .color(c.text_muted).size(12.5).italics());
                                }
                                SettingsTab::Cache => {
                                    ui.label(RichText::new("Cache & Thumbnails").color(c.text).size(14.0).strong());
                                    ui.add_space(12.0);
                                    ui.label(RichText::new("Thumbnail generation and cache management will appear here.")
                                        .color(c.text_muted).size(12.5).italics());
                                }
                            }
                        });
                });
            });
    }

    /// Render a single file row. Returns the egui Response so callers can
    /// check `.clicked()`, `.double_clicked()`, etc.
    ///
    /// `row_index` is used to paint alternating row backgrounds.
    fn render_file_row(
        &self,
        ui: &mut egui::Ui,
        entry: &FileEntry,
        selected: bool,
        is_active_col: bool,
        row_index: usize,
        c: &Colors,
    ) -> egui::Response {
        let row_h = 25.0;
        let row_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), row_h));

        // Alternating stripe (painted first, underneath everything else)
        if row_index % 2 == 1 && !selected {
            ui.painter().rect_filled(row_rect, 0.0, c.row_alt);
        }

        let fill = if selected && is_active_col {
            c.selected_bg
        } else if selected {
            c.accent_dim
        } else {
            Color32::TRANSPARENT
        };

        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(row_rect, 0.0, fill);
        }

        let response = ui.allocate_rect(row_rect, Sense::click());

        if response.hovered() && !selected {
            ui.painter().rect_filled(row_rect, 0.0, c.hover);
        }

        let is_dir = matches!(entry.kind, EntryKind::Directory);
        let text_col = if selected && is_active_col {
            c.text
        } else if is_dir {
            c.text
        } else {
            c.text_dim
        };

        // Draw MI icon — same font as Grid and List views for visual consistency
        let icon_str = mi_entry_icon(entry.kind, &entry.path);
        let icon_color = match entry.kind {
            EntryKind::Directory => c.accent,
            EntryKind::Symlink   => c.accent_dim,
            _                    => c.text_muted,
        };
        ui.painter().text(
            egui::Pos2::new(row_rect.min.x + 10.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            icon_str,
            FontId::proportional(16.0),
            icon_color,
        );

        ui.painter().text(
            egui::Pos2::new(row_rect.min.x + 30.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            &entry.name,
            FontId::proportional(13.0),
            text_col,
        );
        // Arrow for directories — shows there are children
        if is_dir {
            ui.painter().text(
                egui::Pos2::new(row_rect.max.x - 10.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                "\u{203A}", // › single right angle quotation mark — in NotoSans
                FontId::proportional(13.0),
                c.text_muted,
            );
        }

        // NOTE: do NOT add_space here — allocate_rect already advanced the cursor.
        // Adding space here was the cause of double-row-height spacing.
        response
    }
}

// ── eframe::App implementation ────────────────────────────────────────────────

impl eframe::App for OttrinApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Poll for async results at ~30fps. egui will also repaint immediately
        // on any user input, so this only affects the background-update rate.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        // Process async results
        self.poll_listing_results();
        self.poll_op_results();

        // Handle keyboard before rendering
        self.handle_keyboard(ctx);

        // Space to toggle preview — must consume before widgets see it
        if !self.address_bar.editing && !self.command_frame.visible {
            let space = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space));
            if space {
                if self.preview.visible {
                    self.preview.visible = false;
                } else {
                    self.open_preview();
                }
            }
        }

        // Render bottom panels first (egui registers bottom-to-top)
        self.render_status_bar(ctx);
        self.render_command_frame(ctx);

        // Three stacked top panels:
        //   1. Tab row:       tabs + window controls
        //   2. Nav row:       back/forward/up + address bar + toggles
        //   3. Bookmarks row: quick-access directory shortcuts
        self.render_tab_row(ctx);
        self.render_nav_row(ctx);
        self.render_bookmarks_row(ctx);

        // Render right sidebar (target panel, collapsible)
        self.render_target_sidebar(ctx);

        // Central file pane (fills remaining space)
        self.render_file_pane(ctx);

        // Floating overlays
        self.render_preview_overlay(ctx);
        self.render_properties_popup(ctx);
        self.render_about_popup(ctx);
        self.render_settings_modal(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_config(&self.state.config);
    }
}

// ── Directory listing worker ───────────────────────────────────────────────────

fn list_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    use std::time::UNIX_EPOCH;

    let read_dir = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    for item in read_dir {
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let name = item.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if meta.is_dir() {
            EntryKind::Directory
        } else if meta.is_symlink() {
            EntryKind::Symlink
        } else if meta.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let size_bytes = if meta.is_file() { Some(meta.len()) } else { None };
        let modified_unix_secs = meta.modified().ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        entries.push(FileEntry {
            name,
            path: item.path(),
            kind,
            size_bytes,
            modified_unix_secs,
        });
    }

    Ok(entries)
}

fn sort_entries(entries: &mut Vec<FileEntry>, config: &ottrin_core::SortConfig) {
    use ottrin_core::SortBy;
    // Directories always come first
    entries.sort_by(|a, b| {
        let a_dir = matches!(a.kind, EntryKind::Directory);
        let b_dir = matches!(b.kind, EntryKind::Directory);
        if a_dir != b_dir {
            return b_dir.cmp(&a_dir); // dirs first
        }
        let ord = match config.by {
            SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Size => a.size_bytes.unwrap_or(0).cmp(&b.size_bytes.unwrap_or(0)),
            SortBy::Modified => a.modified_unix_secs.unwrap_or(0).cmp(&b.modified_unix_secs.unwrap_or(0)),
            SortBy::Kind => entry_kind_label(a.kind).cmp(entry_kind_label(b.kind)),
        };
        if config.ascending { ord } else { ord.reverse() }
    });
}

// ── Config persistence ────────────────────────────────────────────────────────

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut h = default_home_dir();
            h.push(".config");
            h
        });
    let mut p = base;
    p.push("ottrin");
    p.push("config.json");
    Some(p)
}

fn load_config() -> AppConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return AppConfig::default(),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return AppConfig::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_config(config: &AppConfig) {
    let path = match config_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(path, json);
    }
}

// ── Helper utilities ──────────────────────────────────────────────────────────

fn resolve_path(input: &str, cwd: &Path) -> PathBuf {
    let expanded = if input.starts_with('~') {
        let home = default_home_dir();
        let rest = input.trim_start_matches('~').trim_start_matches('/');
        if rest.is_empty() { home } else { home.join(rest) }
    } else {
        PathBuf::from(input)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

fn compute_path_completions(input: &str, cwd: &Path) -> Vec<String> {
    // Extract the last word which may be a partial path
    let last_word = input.split_whitespace().last().unwrap_or("");
    if last_word.is_empty() || last_word.starts_with('-') {
        return Vec::new();
    }
    let partial = resolve_path(last_word, cwd);
    let (search_dir, prefix) = if partial.is_dir() {
        (partial.clone(), String::new())
    } else {
        let parent = partial.parent().unwrap_or(cwd).to_path_buf();
        let name = partial.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        (parent, name)
    };

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };

    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with(&prefix)
        })
        .take(8)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

fn load_text_preview(path: &Path) -> Option<String> {
    const LIMIT: usize = 8192;
    let bytes = std::fs::read(path).ok()?;
    // Check it looks like text (no null bytes in first 512)
    if bytes[..bytes.len().min(512)].contains(&0u8) {
        return None;
    }
    let s = String::from_utf8_lossy(&bytes[..bytes.len().min(LIMIT)]);
    Some(s.into_owned())
}

fn get_file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn open_terminal_here(dir: &Path) {
    // macOS — use the OS open mechanism
    #[cfg(target_os = "macos")]
    {
        // `open -a Terminal .` opens macOS Terminal in the given dir
        let _ = std::process::Command::new("open")
            .args(["-a", "Terminal", "."])
            .current_dir(dir)
            .spawn();
        return;
    }

    // Windows — try Windows Terminal, fall back to cmd
    #[cfg(target_os = "windows")]
    {
        for cmd in &[
            vec!["wt.exe", "-d", "."],
            vec!["cmd.exe", "/c", "start", "cmd.exe"],
        ] {
            if std::process::Command::new(cmd[0]).args(&cmd[1..]).current_dir(dir).spawn().is_ok() {
                return;
            }
        }
        return;
    }

    // Linux / BSD — probe in order of reliability:
    //   1. $TERMINAL set by the user (highest priority)
    //   2. x-terminal-emulator (Debian/Ubuntu alternatives — resolves to user's default)
    //   3. Desktop-environment defaults by DE name
    //   4. Known emulators in rough popularity order
    //   5. xterm as last resort (almost always installed)
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let dir_str = dir.to_str().unwrap_or(".");

        // 1. User override via $TERMINAL — try with --working-directory first, then bare
        if let Ok(t) = std::env::var("TERMINAL") {
            if !t.is_empty() {
                if std::process::Command::new(&t).arg("--working-directory").arg(dir_str).spawn().is_ok() { return; }
                if std::process::Command::new(&t).current_dir(dir).spawn().is_ok() { return; }
            }
        }

        // Each entry: (binary, extra args that set working dir)
        // Most modern terminals need an explicit flag; a few respect current_dir.
        let candidates: &[(&str, &[&str])] = &[
            ("x-terminal-emulator",  &["--working-directory", dir_str]),
            ("gnome-terminal",       &["--working-directory", dir_str]),
            ("cosmic-term",          &["--working-directory", dir_str]),
            ("tilix",                &["--working-directory", dir_str]),
            ("mate-terminal",        &["--working-directory", dir_str]),
            ("xfce4-terminal",       &["--working-directory", dir_str]),
            ("lxterminal",           &["--working-directory", dir_str]),
            ("konsole",              &["--workdir", dir_str]),
            ("alacritty",            &["--working-directory", dir_str]),
            ("kitty",                &["-d", dir_str]),
            ("wezterm",              &["start", "--cwd", dir_str]),
            ("xterm",                &[]),   // xterm respects current_dir
        ];

        for (t, args) in candidates {
            if std::process::Command::new(t).args(*args).current_dir(dir).spawn().is_ok() {
                return;
            }
        }
    }
}

#[allow(dead_code)]
fn entry_icon(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "\u{25B8}", // ▸ small right-pointing triangle
        EntryKind::File      => "\u{00B7}", // · middle dot
        EntryKind::Symlink   => "\u{2192}", // → rightwards arrow
        EntryKind::Other     => "\u{00B7}", // ·
    }
}

/// Colour to paint the entry_icon for a given kind.
#[allow(dead_code)]
fn icon_color(kind: EntryKind, c: &Colors) -> Color32 {
    match kind {
        EntryKind::Directory => c.accent,
        EntryKind::Symlink   => c.accent_dim,
        _                    => c.text_muted,
    }
}

fn entry_kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "Folder",
        EntryKind::File      => "File",
        EntryKind::Symlink   => "Symlink",
        EntryKind::Other     => "Other",
    }
}

/// Returns a Material Icon codepoint for a file based on its extension.
/// Falls back to the generic file icon for unknown types.
fn file_type_mi_icon(path: &Path) -> &'static str {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        // Documents
        "pdf"                                   => MI_PICTURE_PDF,
        "doc" | "docx" | "odt" | "rtf"         => MI_DESCRIPTION,
        "txt" | "md" | "rst" | "log"            => MI_DESCRIPTION,
        // Spreadsheets
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => MI_TABLE_CHART,
        // Presentations
        "ppt" | "pptx" | "odp" | "key"         => MI_SLIDESHOW,
        // Images
        "jpg" | "jpeg" | "png" | "gif" | "webp"
        | "svg" | "bmp" | "tiff" | "ico" | "heic" | "avif" => MI_IMAGE,
        // Video
        "mp4" | "mkv" | "avi" | "mov" | "wmv"
        | "flv" | "webm" | "m4v" | "mpg"       => MI_MOVIE,
        // Audio
        "mp3" | "flac" | "wav" | "ogg" | "aac"
        | "m4a" | "opus" | "wma"               => MI_MUSIC_NOTE,
        // Code
        "rs" | "py" | "js" | "ts" | "go"
        | "c" | "cpp" | "h" | "java" | "kt"
        | "swift" | "rb" | "php" | "cs"
        | "html" | "css" | "json" | "toml"
        | "yaml" | "yml" | "xml" | "sh"
        | "bash" | "zsh" | "fish" | "lua"
        | "sql" | "wasm"                        => MI_CODE,
        // Archives
        "zip" | "tar" | "gz" | "bz2" | "xz"
        | "7z" | "rar" | "zst" | "lz4"         => MI_FOLDER_ZIP,
        // Executables / binaries
        "exe" | "elf" | "so" | "dll" | "bin"
        | "deb" | "rpm" | "appimage"            => MI_TERMINAL,
        // Default
        _                                       => MI_FILE,
    }
}

/// Returns a Material Icon for a file entry (folder, file, symlink).
/// For files, uses file_type_mi_icon to pick a type-specific icon.
fn mi_entry_icon(kind: EntryKind, path: &Path) -> &'static str {
    match kind {
        EntryKind::Directory => MI_FOLDER,
        EntryKind::Symlink   => MI_LINK,
        EntryKind::File      => file_type_mi_icon(path),
        EntryKind::Other     => MI_FILE,
    }
}

#[allow(dead_code)]
fn file_icon_for_path(path: &Path) -> &'static str {
    file_type_mi_icon(path)
}

/// How many label chars to show in grid cells at a given scale factor.
fn item_chars_for_scale(scale: f32) -> f32 {
    (10.0 * scale).round().max(6.0)
}

fn truncate_name(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars {
        name.to_string()
    } else {
        let stem: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        format!("{}…", stem)
    }
}

#[allow(dead_code)]
fn nav_button(ui: &mut egui::Ui, kind: NavBtn, enabled: bool, c: &Colors) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::click());
    if !enabled {
        return resp;
    }
    let col = if resp.hovered() { c.text } else { c.text_dim };
    let p = ui.painter();
    let ctr = rect.center();
    let sw = 1.5;
    match kind {
        NavBtn::Back => {
            // Left-pointing chevron <
            p.line_segment([egui::Pos2::new(ctr.x + 3.0, ctr.y - 4.5), egui::Pos2::new(ctr.x - 3.0, ctr.y)], Stroke::new(sw, col));
            p.line_segment([egui::Pos2::new(ctr.x - 3.0, ctr.y), egui::Pos2::new(ctr.x + 3.0, ctr.y + 4.5)], Stroke::new(sw, col));
        }
        NavBtn::Forward => {
            // Right-pointing chevron >
            p.line_segment([egui::Pos2::new(ctr.x - 3.0, ctr.y - 4.5), egui::Pos2::new(ctr.x + 3.0, ctr.y)], Stroke::new(sw, col));
            p.line_segment([egui::Pos2::new(ctr.x + 3.0, ctr.y), egui::Pos2::new(ctr.x - 3.0, ctr.y + 4.5)], Stroke::new(sw, col));
        }
        NavBtn::Up => {
            // Up-pointing chevron ^
            p.line_segment([egui::Pos2::new(ctr.x - 4.5, ctr.y + 3.0), egui::Pos2::new(ctr.x, ctr.y - 3.0)], Stroke::new(sw, col));
            p.line_segment([egui::Pos2::new(ctr.x, ctr.y - 3.0), egui::Pos2::new(ctr.x + 4.5, ctr.y + 3.0)], Stroke::new(sw, col));
        }
    }
    resp
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum NavBtn { Back, Forward, Up }

/// Minimal flat window-control button. Draws a line-based icon (×, □, −).
/// Uses muted grey at rest, text colour on hover — no colours.
#[allow(dead_code)]
fn wm_btn(ui: &mut egui::Ui, kind: WmBtn, c: &Colors) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let col = if resp.hovered() { c.text } else { c.text_muted };
    let p = ui.painter();
    let ctr = rect.center();
    let sw = 1.2; // stroke width

    match kind {
        WmBtn::Close => {
            // × — two diagonal lines
            let d = 4.5;
            p.line_segment([egui::Pos2::new(ctr.x - d, ctr.y - d), egui::Pos2::new(ctr.x + d, ctr.y + d)], Stroke::new(sw, col));
            p.line_segment([egui::Pos2::new(ctr.x + d, ctr.y - d), egui::Pos2::new(ctr.x - d, ctr.y + d)], Stroke::new(sw, col));
        }
        WmBtn::Maximize => {
            // □ — square outline
            let d = 4.5;
            let r = egui::Rect::from_center_size(ctr, Vec2::splat(d * 2.0));
            p.rect_stroke(r, 0.0, Stroke::new(sw, col), egui::StrokeKind::Middle);
        }
        WmBtn::Minimize => {
            // − — horizontal bar
            let d = 4.5;
            p.line_segment([egui::Pos2::new(ctr.x - d, ctr.y), egui::Pos2::new(ctr.x + d, ctr.y)], Stroke::new(sw, col));
        }
    }
    resp
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum WmBtn { Close, Maximize, Minimize }

/// Renders a Material Icon button. Pass a MI_* constant as `icon`.
/// Returns the Response so you can check `.clicked()`, chain `.on_hover_text()`, etc.
#[allow(dead_code)]
fn icon_btn(ui: &mut egui::Ui, icon: &str, size: f32, color: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(size).color(color))
            .frame(false)
            .min_size(Vec2::splat(size + 10.0))
    )
}

/// Like icon_btn but with a visible filled background — for primary actions.
#[allow(dead_code)]
fn icon_btn_filled(ui: &mut egui::Ui, icon: &str, label: &str, c: &Colors) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(format!("{} {}", icon, label)).size(12.5).color(c.text)
        )
        .min_size(Vec2::new(0.0, 28.0))
        .corner_radius(5.0)
    )
}
