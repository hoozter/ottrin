use egui::emath::Rangef;
use egui::scroll_area::ScrollBarVisibility;
use egui::style::ScrollAnimation;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Align2, Color32, Context, FontId, Frame, Key, Layout, Rect, ResizeDirection, RichText,
    ScrollArea, Sense, Shadow, Stroke, UiBuilder, Vec2,
};
use egui_extras::{Column, TableBuilder};
use ottrin_core::{
    AppConfig, AppState, CodeSubtype, ConflictAction, DeleteMode, EntryKind, FileCategory,
    FileCommand, FileEntry, FileSemantic, FolderKind, ListColumn, MillerColumnWidthMode,
    PrivilegedCommand, PrivilegedContext, PrivilegedPayload, PrivilegedRequest, PrivilegedStatus,
    SavedTheme, SearchIndexStatus, SearchQuery, SearchResultItem, SearchScope, SearchSort, SortBy,
    TandemSide, ThemeCustomization, ThemeMode, ThemePreset, ViewMode, classify_file,
    default_home_dir, format_modified, format_size, format_size_units, platform_case_mode_current,
};
use ottrin_platform::{DefaultPlatform, PlatformOps, PrivilegedAvailability};
use ottrin_preview::{PreviewData, PreviewKind, PreviewRequest, load_preview};
use ottrin_search::{SearchIndexDiagnostics, SearchService, default_roots as default_search_roots};
use sha2::{Digest, Sha256};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

fn format_age_str(secs: u64) -> String {
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

const CUSTOM_TITLEBAR_HEIGHT: f32 = 36.0;
const VIEW_CONTROLS_BAR_HEIGHT: f32 = 28.0;
const STATUS_BAR_HEIGHT: f32 = 26.0;
const SMART_PANEL_STRIP_WIDTH: f32 = 36.0;
const MAX_CACHED_IMAGE_PREVIEWS: usize = 96;
const PREVIEW_CACHE_BUCKET_SMALL: u32 = 256;
const PREVIEW_CACHE_BUCKET_LARGE: u32 = 640;
const PREVIEW_CACHE_BUCKET_XL: u32 = 1280;
const PREVIEW_REQUESTS_PER_FRAME: usize = 2;
const MILLER_FIXED_WIDTH: f32 = 240.0;
const TANDEM_PANE_HEADER_HEIGHT: f32 = 38.0;
const TANDEM_DIVIDER_WIDTH: f32 = 8.0;

// ── Material Icons codepoints ──────────────────────────────────────────────
// Font: MaterialSymbolsFilled.ttf (Apache 2.0, derived from MaterialSymbolsOutlined variable font with FILL=1)
#[allow(dead_code)]
const MI_ARROW_BACK: &str = "\u{E5C4}";
const MI_ARROW_FORWARD: &str = "\u{E5C8}";
const MI_ARROW_UP: &str = "\u{E5D8}";
const MI_CLOSE: &str = "\u{E5CD}";
const MI_VIEW_COLUMN: &str = "\u{E8EC}"; // Miller columns
const MI_VIEW_LIST: &str = "\u{E8EF}"; // List
const MI_APPS: &str = "\u{E5C3}"; // Grid
const MI_WIDTH_NORMAL: &str = "\u{F8F6}"; // width_normal (fixed columns)
const MI_WIDTH_FULL: &str = "\u{F8F5}"; // width_full (auto-fit columns)
const MI_DARK_MODE: &str = "\u{E51C}";
const MI_LIGHT_MODE: &str = "\u{E518}";
const MI_FOLDER: &str = "\u{E2C7}";
const MI_FOLDER_OPEN: &str = "\u{E2C8}";
const MI_FOLDER_SHARED: &str = "\u{E2C9}";
const MI_DESKTOP: &str = "\u{E30C}";
const MI_DOWNLOAD: &str = "\u{E2C4}";
const MI_FILE: &str = "\u{E24D}";
const MI_LINK: &str = "\u{E157}";
const MI_HOME: &str = "\u{E88A}";
const MI_STORAGE: &str = "\u{E1DB}"; // storage / hard disk
const MI_BOOKMARK: &str = "\u{E866}";
const MI_SEARCH: &str = "\u{E8B6}";
const MI_TERMINAL: &str = "\u{EB8E}";
const MI_CLEAR: &str = "\u{E14C}";
const MI_SETTINGS: &str = "\u{E8B8}";
const MI_INFO: &str = "\u{E88E}";
const MI_CHECK: &str = "\u{E5CA}";
const MI_CREATE_FOLDER: &str = "\u{E2CC}";
const MI_LOCK: &str = "\u{E897}";
const MI_SETTINGS_BACKUP_RESTORE: &str = "\u{E8BA}";

// File-type icons
const MI_PICTURE_PDF: &str = "\u{E415}"; // picture_as_pdf
const MI_IMAGE: &str = "\u{E3F4}"; // image
const MI_MOVIE: &str = "\u{E02C}"; // movie
const MI_MUSIC_NOTE: &str = "\u{E405}"; // music_note / audio
const MI_CODE: &str = "\u{E86F}"; // code
const MI_FOLDER_ZIP: &str = "\u{EB2C}"; // folder_zip / archive
const MI_SLIDESHOW: &str = "\u{E41B}"; // slideshow / presentation
const MI_DESCRIPTION: &str = "\u{E873}"; // description / document
const MI_TABLE_CHART: &str = "\u{E265}"; // table_chart / spreadsheet
const MI_VISIBILITY: &str = "\u{E8F4}"; // visibility (show hidden)
const MI_VISIBILITY_OFF: &str = "\u{E8F5}"; // visibility_off (hide hidden)
const MI_MOVE_TO_INBOX: &str = "\u{E168}"; // move_to_inbox (drop folder destination)
const MI_TUNE: &str = "\u{E429}"; // tune (display settings / scale)
const MI_EDIT: &str = "\u{E3C9}"; // edit

// ── Theme colours ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Colors {
    bg: Color32,
    panel: Color32,
    panel_raised: Color32,
    toolbar_bg: Color32,
    bookmarks_bg: Color32,
    smart_panel_bg: Color32,
    titlebar_bg: Color32,
    border: Color32,
    window_border: Color32,
    window_border_width: f32,
    heading: Color32,
    folder: Color32,
    file: Color32,
    text: Color32,
    text_dim: Color32,
    text_muted: Color32,
    accent: Color32,
    accent_dim: Color32,
    hover: Color32,
    selected_bg: Color32,
    row_alt: Color32,
    error: Color32,
    app_radius: u8,
    border_radius: u8,
    button_radius: u8,
    font_scale: f32,
}

impl Colors {
    fn dark() -> Self {
        Self {
            // Lifted neutrals — dark but not pitch-black, cool-tinted.
            // bg = file-list rows / browsing canvas (darkest surface)
            // panel = toolbars / tab bar / nav row (main UI chrome)
            // panel_raised = cards / inputs / elevated elements (lightest dark)
            // smart_panel_bg = sidebar panel (between bg and panel)
            bg: Color32::from_rgb(24, 26, 30),
            panel: Color32::from_rgb(31, 33, 38),
            panel_raised: Color32::from_rgb(40, 43, 50),
            toolbar_bg: Color32::from_rgb(31, 33, 38),
            bookmarks_bg: Color32::from_rgb(31, 33, 38),
            smart_panel_bg: Color32::from_rgb(27, 29, 34),
            titlebar_bg: Color32::from_rgb(24, 26, 30),
            border: Color32::from_rgb(50, 54, 63),
            window_border: Color32::from_rgb(50, 54, 63),
            window_border_width: 1.0,
            heading: Color32::from_rgb(232, 232, 232),
            folder: Color32::from_rgb(81, 154, 186),
            file: Color32::from_rgb(188, 191, 196),
            text: Color32::from_rgb(232, 232, 232),
            text_dim: Color32::from_rgb(188, 191, 196),
            text_muted: Color32::from_rgb(116, 118, 124),
            accent: Color32::from_rgb(81, 154, 186),
            accent_dim: Color32::from_rgba_unmultiplied(81, 154, 186, 56),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            selected_bg: Color32::from_rgba_unmultiplied(81, 154, 186, 58),
            row_alt: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
            error: Color32::from_rgb(224, 85, 85),
            app_radius: 5,
            border_radius: 5,
            button_radius: 5,
            font_scale: 1.0,
        }
    }

    fn light() -> Self {
        Self {
            bg: Color32::from_rgb(255, 255, 255),
            panel: Color32::from_rgb(244, 244, 244),
            panel_raised: Color32::from_rgb(235, 235, 235),
            toolbar_bg: Color32::from_rgb(244, 244, 244),
            bookmarks_bg: Color32::from_rgb(255, 255, 255),
            smart_panel_bg: Color32::from_rgb(244, 244, 244),
            titlebar_bg: Color32::from_rgb(255, 255, 255),
            border: Color32::from_rgb(208, 208, 208),
            window_border: Color32::from_rgb(208, 208, 208),
            window_border_width: 1.0,
            heading: Color32::from_rgb(20, 20, 20),
            folder: Color32::from_rgb(37, 99, 235),
            file: Color32::from_rgb(80, 80, 80),
            text: Color32::from_rgb(20, 20, 20),
            text_dim: Color32::from_rgb(80, 80, 80),
            text_muted: Color32::from_rgb(150, 150, 150),
            accent: Color32::from_rgb(37, 99, 235),
            accent_dim: Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
            selected_bg: Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            row_alt: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            error: Color32::from_rgb(196, 48, 48),
            app_radius: 5,
            border_radius: 5,
            button_radius: 5,
            font_scale: 1.0,
        }
    }

    fn from_rgba(v: [u8; 4]) -> Color32 {
        Color32::from_rgba_unmultiplied(v[0], v[1], v[2], v[3])
    }

    fn to_rgba(c: Color32) -> [u8; 4] {
        [c.r(), c.g(), c.b(), c.a()]
    }

    fn preset(preset: ThemePreset, dark: bool) -> Self {
        match preset {
            ThemePreset::Ottrin => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(30, 34, 39),
                        panel: Color32::from_rgb(38, 43, 49),
                        panel_raised: Color32::from_rgb(49, 56, 66),
                        toolbar_bg: Color32::from_rgb(38, 43, 49),
                        bookmarks_bg: Color32::from_rgb(38, 43, 49),
                        smart_panel_bg: Color32::from_rgb(35, 40, 46),
                        titlebar_bg: Color32::from_rgb(30, 34, 39),
                        border: Color32::from_rgb(59, 68, 80),
                        window_border: Color32::from_rgb(59, 68, 80),
                        heading: Color32::from_rgb(231, 236, 240),
                        folder: Color32::from_rgb(84, 162, 220),
                        file: Color32::from_rgb(196, 204, 214),
                        text: Color32::from_rgb(231, 236, 240),
                        text_dim: Color32::from_rgb(192, 200, 210),
                        text_muted: Color32::from_rgb(154, 163, 174),
                        accent: Color32::from_rgb(78, 161, 242),
                        accent_dim: Color32::from_rgba_unmultiplied(78, 161, 242, 70),
                        hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                        selected_bg: Color32::from_rgba_unmultiplied(78, 161, 242, 70),
                        row_alt: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                        error: Color32::from_rgb(224, 108, 117),
                        app_radius: 6,
                        border_radius: 6,
                        button_radius: 6,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self::light()
                }
            }
            ThemePreset::Breeze => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(26, 28, 31),
                        panel: Color32::from_rgb(31, 35, 39),
                        panel_raised: Color32::from_rgb(36, 40, 45),
                        toolbar_bg: Color32::from_rgb(36, 40, 45),
                        bookmarks_bg: Color32::from_rgb(31, 35, 39),
                        smart_panel_bg: Color32::from_rgb(31, 35, 39),
                        titlebar_bg: Color32::from_rgb(43, 47, 52),
                        border: Color32::from_rgb(64, 69, 75),
                        window_border: Color32::from_rgb(64, 69, 75),
                        heading: Color32::from_rgb(252, 252, 252),
                        folder: Color32::from_rgb(29, 153, 243),
                        file: Color32::from_rgb(161, 169, 177),
                        text: Color32::from_rgb(252, 252, 252),
                        text_dim: Color32::from_rgb(161, 169, 177),
                        text_muted: Color32::from_rgb(112, 111, 110),
                        accent: Color32::from_rgb(61, 174, 233),
                        accent_dim: Color32::from_rgba_unmultiplied(61, 174, 233, 66),
                        hover: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                        selected_bg: Color32::from_rgba_unmultiplied(61, 174, 233, 48),
                        row_alt: Color32::from_rgba_unmultiplied(255, 255, 255, 5),
                        error: Color32::from_rgb(218, 68, 83),
                        border_radius: 5,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(255, 255, 255),
                        panel: Color32::from_rgb(239, 240, 241),
                        panel_raised: Color32::from_rgb(252, 252, 252),
                        border: Color32::from_rgb(227, 229, 231),
                        window_border: Color32::from_rgb(227, 229, 231),
                        heading: Color32::from_rgb(35, 38, 41),
                        folder: Color32::from_rgb(41, 128, 185),
                        file: Color32::from_rgb(112, 125, 138),
                        text: Color32::from_rgb(35, 38, 41),
                        text_dim: Color32::from_rgb(112, 125, 138),
                        text_muted: Color32::from_rgb(112, 111, 110),
                        accent: Color32::from_rgb(61, 174, 233),
                        accent_dim: Color32::from_rgb(163, 212, 250),
                        hover: Color32::from_rgb(247, 247, 247),
                        selected_bg: Color32::from_rgb(163, 212, 250),
                        row_alt: Color32::from_rgb(247, 247, 247),
                        error: Color32::from_rgb(218, 68, 83),
                        border_radius: 5,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
            ThemePreset::Adwaita => {
                if dark {
                    Self {
                        // Canonical libadwaita dark surfaces:
                        // window #222226, sidebar #2e2e32, view #1d1d20
                        bg: Color32::from_rgb(34, 34, 38),
                        panel: Color32::from_rgb(46, 46, 50),
                        panel_raised: Color32::from_rgb(29, 29, 32),
                        border: Color32::from_rgb(61, 56, 70), // dark_3
                        window_border: Color32::from_rgb(61, 56, 70),
                        heading: Color32::from_rgb(255, 255, 255),
                        folder: Color32::from_rgb(53, 132, 228),
                        file: Color32::from_rgb(192, 191, 188), // light_4
                        text: Color32::from_rgb(255, 255, 255),
                        text_dim: Color32::from_rgb(198, 198, 201),
                        text_muted: Color32::from_rgb(119, 118, 123), // dark_1
                        accent: Color32::from_rgb(53, 132, 228),
                        accent_dim: Color32::from_rgba_unmultiplied(53, 132, 228, 72),
                        hover: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                        selected_bg: Color32::from_rgba_unmultiplied(53, 132, 228, 56),
                        row_alt: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                        error: Color32::from_rgb(224, 27, 36), // red_3
                        border_radius: 6,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(250, 250, 251),
                        panel: Color32::from_rgb(235, 235, 237),
                        panel_raised: Color32::from_rgb(243, 243, 245),
                        border: Color32::from_rgba_unmultiplied(0, 0, 6, 18),
                        window_border: Color32::from_rgba_unmultiplied(0, 0, 6, 18),
                        heading: Color32::from_rgba_unmultiplied(0, 0, 6, 204),
                        folder: Color32::from_rgb(53, 132, 228),
                        file: Color32::from_rgba_unmultiplied(0, 0, 6, 204),
                        text: Color32::from_rgba_unmultiplied(0, 0, 6, 204),
                        text_dim: Color32::from_rgba_unmultiplied(0, 0, 6, 140),
                        text_muted: Color32::from_rgba_unmultiplied(0, 0, 6, 128),
                        accent: Color32::from_rgb(53, 132, 228),
                        accent_dim: Color32::from_rgba_unmultiplied(53, 132, 228, 64),
                        hover: Color32::from_rgba_unmultiplied(0, 0, 6, 10),
                        selected_bg: Color32::from_rgba_unmultiplied(53, 132, 228, 64),
                        row_alt: Color32::from_rgba_unmultiplied(0, 0, 6, 18),
                        error: Color32::from_rgb(224, 27, 36),
                        border_radius: 6,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
            ThemePreset::Windows11 => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(18, 18, 18),
                        panel: Color32::from_rgb(28, 28, 28),
                        panel_raised: Color32::from_rgb(43, 43, 43),
                        toolbar_bg: Color32::from_rgb(32, 32, 32),
                        bookmarks_bg: Color32::from_rgb(32, 32, 32),
                        smart_panel_bg: Color32::from_rgb(24, 24, 24),
                        titlebar_bg: Color32::from_rgb(34, 34, 34),
                        border: Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                        window_border: Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                        heading: Color32::from_rgb(243, 243, 243),
                        folder: Color32::from_rgb(102, 183, 255),
                        file: Color32::from_rgb(214, 214, 214),
                        text: Color32::from_rgb(243, 243, 243),
                        text_dim: Color32::from_rgb(214, 214, 214),
                        text_muted: Color32::from_rgb(182, 182, 182),
                        accent: Color32::from_rgb(96, 178, 255),
                        accent_dim: Color32::from_rgba_unmultiplied(96, 178, 255, 58),
                        hover: Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                        selected_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 11),
                        row_alt: Color32::from_rgba_unmultiplied(255, 255, 255, 3),
                        error: Color32::from_rgb(209, 52, 56),
                        border_radius: 8,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(255, 255, 255),
                        panel: Color32::from_rgb(250, 250, 250),
                        panel_raised: Color32::from_rgb(245, 245, 245),
                        border: Color32::from_rgb(209, 209, 209),
                        window_border: Color32::from_rgb(209, 209, 209),
                        heading: Color32::from_rgb(36, 36, 36),
                        folder: Color32::from_rgb(0, 120, 212),
                        file: Color32::from_rgb(66, 66, 66),
                        text: Color32::from_rgb(36, 36, 36),
                        text_dim: Color32::from_rgb(66, 66, 66),
                        text_muted: Color32::from_rgb(97, 97, 97),
                        accent: Color32::from_rgb(0, 120, 212),
                        accent_dim: Color32::from_rgb(239, 246, 252),
                        hover: Color32::from_rgb(245, 245, 245),
                        selected_bg: Color32::from_rgb(235, 235, 235),
                        row_alt: Color32::from_rgb(240, 240, 240),
                        error: Color32::from_rgb(209, 52, 56),
                        border_radius: 7,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
            ThemePreset::Solarized => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(0, 43, 54),
                        panel: Color32::from_rgb(7, 54, 66),
                        panel_raised: Color32::from_rgb(88, 110, 117),
                        border: Color32::from_rgb(101, 123, 131),
                        window_border: Color32::from_rgb(101, 123, 131),
                        heading: Color32::from_rgb(147, 161, 161),
                        folder: Color32::from_rgb(38, 139, 210),
                        file: Color32::from_rgb(131, 148, 150),
                        text: Color32::from_rgb(131, 148, 150),
                        text_dim: Color32::from_rgb(101, 123, 131),
                        text_muted: Color32::from_rgb(88, 110, 117),
                        accent: Color32::from_rgb(42, 161, 152),
                        accent_dim: Color32::from_rgb(42, 161, 152),
                        hover: Color32::from_rgb(7, 54, 66),
                        selected_bg: Color32::from_rgb(38, 139, 210),
                        row_alt: Color32::from_rgb(7, 54, 66),
                        error: Color32::from_rgb(220, 50, 47),
                        border_radius: 4,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(253, 246, 227),
                        panel: Color32::from_rgb(238, 232, 213),
                        panel_raised: Color32::from_rgb(238, 232, 213),
                        border: Color32::from_rgb(147, 161, 161),
                        window_border: Color32::from_rgb(147, 161, 161),
                        heading: Color32::from_rgb(88, 110, 117),
                        folder: Color32::from_rgb(38, 139, 210),
                        file: Color32::from_rgb(101, 123, 131),
                        text: Color32::from_rgb(101, 123, 131),
                        text_dim: Color32::from_rgb(131, 148, 150),
                        text_muted: Color32::from_rgb(147, 161, 161),
                        accent: Color32::from_rgb(42, 161, 152),
                        accent_dim: Color32::from_rgb(42, 161, 152),
                        hover: Color32::from_rgb(238, 232, 213),
                        selected_bg: Color32::from_rgb(38, 139, 210),
                        row_alt: Color32::from_rgb(238, 232, 213),
                        error: Color32::from_rgb(220, 50, 47),
                        border_radius: 4,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
            ThemePreset::Nord => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(46, 52, 64),
                        panel: Color32::from_rgb(59, 66, 82),
                        panel_raised: Color32::from_rgb(67, 76, 94),
                        border: Color32::from_rgb(76, 86, 106),
                        window_border: Color32::from_rgb(76, 86, 106),
                        heading: Color32::from_rgb(236, 239, 244),
                        folder: Color32::from_rgb(136, 192, 208),
                        file: Color32::from_rgb(216, 222, 233),
                        text: Color32::from_rgb(216, 222, 233),
                        text_dim: Color32::from_rgb(76, 86, 106),
                        text_muted: Color32::from_rgb(67, 76, 94),
                        accent: Color32::from_rgb(129, 161, 193),
                        accent_dim: Color32::from_rgb(94, 129, 172),
                        hover: Color32::from_rgb(59, 66, 82),
                        selected_bg: Color32::from_rgb(94, 129, 172),
                        row_alt: Color32::from_rgb(59, 66, 82),
                        error: Color32::from_rgb(191, 97, 106),
                        border_radius: 5,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(236, 239, 244),
                        panel: Color32::from_rgb(229, 233, 240),
                        panel_raised: Color32::from_rgb(216, 222, 233),
                        border: Color32::from_rgb(76, 86, 106),
                        window_border: Color32::from_rgb(76, 86, 106),
                        heading: Color32::from_rgb(46, 52, 64),
                        folder: Color32::from_rgb(94, 129, 172),
                        file: Color32::from_rgb(59, 66, 82),
                        text: Color32::from_rgb(46, 52, 64),
                        text_dim: Color32::from_rgb(67, 76, 94),
                        text_muted: Color32::from_rgb(76, 86, 106),
                        accent: Color32::from_rgb(129, 161, 193),
                        accent_dim: Color32::from_rgb(136, 192, 208),
                        hover: Color32::from_rgb(216, 222, 233),
                        selected_bg: Color32::from_rgb(136, 192, 208),
                        row_alt: Color32::from_rgb(229, 233, 240),
                        error: Color32::from_rgb(191, 97, 106),
                        border_radius: 5,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
            ThemePreset::G33k => {
                if dark {
                    Self {
                        bg: Color32::from_rgb(7, 12, 9),
                        panel: Color32::from_rgb(10, 18, 13),
                        panel_raised: Color32::from_rgb(14, 26, 18),
                        border: Color32::from_rgb(28, 54, 34),
                        window_border: Color32::from_rgb(28, 54, 34),
                        heading: Color32::from_rgb(131, 255, 147),
                        folder: Color32::from_rgb(96, 214, 255),
                        file: Color32::from_rgb(131, 255, 147),
                        text: Color32::from_rgb(131, 255, 147),
                        text_dim: Color32::from_rgb(112, 222, 126),
                        text_muted: Color32::from_rgb(72, 158, 86),
                        accent: Color32::from_rgb(96, 214, 255),
                        accent_dim: Color32::from_rgba_unmultiplied(96, 214, 255, 54),
                        hover: Color32::from_rgba_unmultiplied(120, 255, 160, 20),
                        selected_bg: Color32::from_rgba_unmultiplied(96, 214, 255, 44),
                        row_alt: Color32::from_rgba_unmultiplied(120, 255, 160, 8),
                        error: Color32::from_rgb(255, 124, 124),
                        border_radius: 3,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(232, 255, 236),
                        panel: Color32::from_rgb(222, 248, 227),
                        panel_raised: Color32::from_rgb(210, 242, 216),
                        border: Color32::from_rgb(150, 195, 158),
                        window_border: Color32::from_rgb(150, 195, 158),
                        heading: Color32::from_rgb(26, 76, 35),
                        folder: Color32::from_rgb(37, 99, 235),
                        file: Color32::from_rgb(38, 88, 46),
                        text: Color32::from_rgb(26, 76, 35),
                        text_dim: Color32::from_rgb(38, 88, 46),
                        text_muted: Color32::from_rgb(76, 124, 84),
                        accent: Color32::from_rgb(37, 99, 235),
                        accent_dim: Color32::from_rgba_unmultiplied(37, 99, 235, 40),
                        hover: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
                        selected_bg: Color32::from_rgba_unmultiplied(37, 99, 235, 30),
                        row_alt: Color32::from_rgba_unmultiplied(0, 0, 0, 7),
                        error: Color32::from_rgb(190, 68, 68),
                        border_radius: 3,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
        }
    }

    fn for_config(config: &AppConfig) -> Self {
        let mut base = Self::base_for_config(config);
        let custom = &config.theme_custom;
        if custom.enabled {
            base.bg = Self::from_rgba(custom.background);
            base.panel = Self::from_rgba(custom.panel);
            // Keep custom colors exact: no implicit gamma/tint transforms.
            // `panel_raised` is a generic elevated surface and must not alias titlebar.
            base.panel_raised = Self::from_rgba(custom.panel);
            base.toolbar_bg = Self::from_rgba(custom.toolbar);
            // Main interface controls nav + bookmarks + address surfaces.
            base.bookmarks_bg = base.toolbar_bg;
            base.smart_panel_bg = Self::from_rgba(custom.smart_panel);
            base.titlebar_bg = Self::from_rgba(custom.titlebar);
            base.border = Self::from_rgba(custom.border);
            base.window_border = Self::from_rgba(custom.window_border);
            // Round to nearest 0.5px to avoid sub-pixel rendering artifacts.
            base.window_border_width =
                (custom.window_border_width.clamp(0.0, 4.0) * 2.0).round() / 2.0;
            base.accent = Self::from_rgba(custom.accent);
            base.accent_dim =
                Self::from_rgba([custom.accent[0], custom.accent[1], custom.accent[2], 56]);
            base.selected_bg =
                Self::from_rgba([custom.accent[0], custom.accent[1], custom.accent[2], 46]);
            base.folder = Self::from_rgba(custom.folder);
            base.heading = Self::from_rgba(custom.text_heading);
            base.text_dim = Self::from_rgba(custom.text_folder);
            base.file = Self::from_rgba(custom.text_file);
            base.text = Self::from_rgba(custom.text_heading);
            base.text_muted = Self::from_rgba(custom.text_file);
            base.app_radius = custom.app_radius.clamp(0.0, 24.0) as u8;
            base.border_radius = custom.border_radius.clamp(0.0, 14.0) as u8;
            base.button_radius = custom.button_radius.clamp(0.0, 14.0) as u8;
            base.font_scale = custom.font_scale.clamp(0.85, 1.35);
        }
        // Keep dark presets on one consistent neutral text token unless the user
        // is actively customizing text slots in the theme editor model.
        if matches!(config.theme, ThemeMode::Dark | ThemeMode::System) && !custom.enabled {
            let neutral =
                Self::neutral_text_for_preset(config.theme_preset, base.text_muted, base.text_dim);
            base.text_muted = neutral;
            base.text = neutral;
            base.text_dim = neutral;
            base.heading = neutral;
        }
        base
    }

    fn neutral_text_for_preset(preset: ThemePreset, muted: Color32, dim: Color32) -> Color32 {
        let t = match preset {
            ThemePreset::Ottrin => 0.70,
            ThemePreset::Breeze => 0.26,
            ThemePreset::Adwaita => 0.24,
            ThemePreset::Windows11 => 0.18,
            ThemePreset::Solarized => 0.58,
            ThemePreset::Nord => 0.45,
            ThemePreset::G33k => 0.36,
        };
        mix_color(muted, dim, t)
    }

    fn base_for_config(config: &AppConfig) -> Self {
        let dark = match config.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark | ThemeMode::System => true,
        };
        Self::preset(config.theme_preset, dark)
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
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ListingKey {
    TabMain(u64),  // tab_id — main column (Miller mid / list view)
    TabLeft(u64),  // tab_id — Miller left column (parent dir)
    TabRight(u64), // tab_id — Miller right column (selected subfolder)
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
    error: Option<String>,
    retry_cmd: Option<FileCommand>,
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
    right_sel: Option<usize>,
    // Path-backed focus for Column view; more stable than fixed left/middle/right.
    miller_focus_dir: Option<PathBuf>,
    // Remember last selected child per directory, so navigating back/up can
    // restore the previous path instead of jumping to the first row.
    selection_memory: HashMap<PathBuf, PathBuf>,
    // One-shot explicit path to restore on next TabMain listing load.
    pending_select_path: Option<PathBuf>,
    // One-shot hint: after explicit "enter directory" keyboard action, select
    // first entry in that directory to keep Right-navigation continuous.
    pending_select_first: bool,
    // Per-directory cache used by dynamic 2..6 Column view.
    miller_cache: HashMap<PathBuf, Result<Vec<FileEntry>, String>>,
    // One-shot focus hint after directional navigation across pane edges.
    miller_focus_hint: Option<MillerFocusHint>,
    // One-shot: ensure active row is scrolled into view after keyboard move.
    miller_scroll_selected: bool,
    // How many preview columns past current_dir are allowed.
    // Up/Down → 1, Left → increment, Right → decrement (min 1).
    miller_preview_depth: usize,
    // High-water mark: total scroll content width can only grow, never shrink.
    miller_total_w_max: f32,
    // Saved horizontal scroll offset — used to freeze scroll on Up/Down.
    miller_h_offset: f32,
    // Column scroll ids (must be stable)
    scroll_epoch: u64, // bump to reset scroll on navigation
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MillerFocusHint {
    LeftEdge,
    CurrentDir,
}

#[derive(Debug, Clone)]
struct MillerColumnModel {
    dir: PathBuf,
    entries: Vec<FileEntry>,
    error: Option<String>,
    selected: Option<usize>,
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

#[derive(Debug, Clone)]
struct UiStatusMessage {
    text: String,
    ok: bool,
    until: Instant,
}

// ── Address bar ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct AddressBar {
    editing: bool,
    text: String,
    request_focus: bool,
    tab_id: Option<u64>,
}

// ── Preview overlay ───────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct PreviewOverlay {
    visible: bool,
    path: Option<PathBuf>,
    data: Option<PreviewData>,
}

#[derive(Debug)]
struct PreviewJobRequest {
    path: PathBuf,
    target_dim: u32,
}

#[derive(Debug)]
struct PreviewJobResponse {
    path: PathBuf,
    target_dim: u32,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
struct InfoPanelDetails {
    readonly: bool,
    child_count: Option<usize>,
    extension: Option<String>,
    created: Option<String>,
    accessed: Option<String>,
    permissions: Option<String>,
    owner: Option<String>,
    group: Option<String>,
    symlink_target: Option<String>,
    inode: Option<u64>,
    device: Option<u64>,
    links: Option<u64>,
    image_dimensions: Option<String>,
    image_format: Option<String>,
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
    dir: Option<PathBuf>,
    entries: Vec<FileEntry>,
    error: Option<String>,
}

// ── Right sidebar section ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SidebarSection {
    #[default]
    None, // collapsed
    Target,
    Info,
    Search,
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

type SettingsNavItem = (SettingsTab, &'static str, &'static str);
type SettingsNavGroup = (&'static str, &'static [SettingsNavItem]);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ThemeEditorTab {
    #[default]
    MainInterface,
    TextAndIcons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
enum ThemePreviewRegion {
    Titlebar,
    Navigation,
    Bookmarks,
    #[default]
    ColumnBackground,
    SelectedRow,
    ColumnText,
    Sidebar,
    StatusBar,
    Accent,
    Borders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ThemeColorField {
    Background,
    Panel,
    Toolbar,
    SmartPanel,
    Titlebar,
    Border,
    WindowBorder,
    Accent,
    Folder,
    ButtonBg,
    ButtonText,
    TextHeading,
    TextFolder,
    TextFile,
    PaletteSoftWhite,
    PaletteFolderBlue,
    PaletteLinkBlue,
    PaletteSteel,
    PaletteGreen,
    PaletteOrange,
    PalettePurple,
    PalettePink,
    PaletteRed,
    PaletteYellow,
}

#[derive(Debug, Clone, Copy)]
struct ThemePresetCardSize {
    width: f32,
    card_h: f32,
    thumb_h: f32,
}

#[derive(Debug, Clone)]
struct ThemeEditorState {
    open: bool,
    tab: ThemeEditorTab,
    selected_region: ThemePreviewRegion,
    preset: ThemePreset,
    mode: ThemeMode,
    source_saved_idx: Option<usize>,
    base_custom: ThemeCustomization,
    original_custom: ThemeCustomization,
    custom: ThemeCustomization,
    save_as_name: String,
    main_hue_shift: f32,
    main_saturation: f32,
    main_brightness: f32,
    main_contrast: f32,
    text_hue_shift: f32,
    text_saturation: f32,
    text_brightness: f32,
    text_contrast: f32,
    color_picker_open: bool,
    color_picker_field: Option<ThemeColorField>,
    color_hex_inputs: std::collections::BTreeMap<ThemeColorField, String>,
    color_clipboard_hex: String,
}

impl Default for ThemeEditorState {
    fn default() -> Self {
        let custom = ThemeCustomization::default();
        Self {
            open: false,
            tab: ThemeEditorTab::MainInterface,
            selected_region: ThemePreviewRegion::default(),
            preset: ThemePreset::default(),
            mode: ThemeMode::default(),
            source_saved_idx: None,
            base_custom: custom.clone(),
            original_custom: custom.clone(),
            custom,
            save_as_name: String::new(),
            main_hue_shift: 0.0,
            main_saturation: 1.0,
            main_brightness: 1.0,
            main_contrast: 1.0,
            text_hue_shift: 0.0,
            text_saturation: 1.0,
            text_brightness: 1.0,
            text_contrast: 1.0,
            color_picker_open: false,
            color_picker_field: None,
            color_hex_inputs: std::collections::BTreeMap::new(),
            color_clipboard_hex: String::new(),
        }
    }
}

// ── Properties popup ──────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct PropertiesPopup {
    visible: bool,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivilegedUiKind {
    InProgress,
    Success,
    Error,
}

#[derive(Debug, Clone)]
struct PrivilegedUiStatus {
    kind: PrivilegedUiKind,
    text: String,
    since: Instant,
}

#[derive(Debug, Default)]
struct SearchUiState {
    query: String,
    scope: SearchScope,
    sort: SearchSort,
    include_hidden: bool,
    results: Vec<SearchResultItem>,
    selected: Option<usize>,
    scroll_to_selected: bool,
    total: usize,
    last_error: Option<String>,
    request_focus: bool,
    input_focused: bool,
    last_query_root: Option<PathBuf>,
    include_glob_input: String,
    exclude_glob_input: String,
    show_include_glob_input: bool,
    show_exclude_glob_input: bool,
    panel_width: f32,
    /// Tracks index size seen on last render — when it grows, re-run the query
    /// so results update live as new batches arrive during indexing.
    last_seen_index_count: usize,
    /// When true, run ripgrep content search in addition to filename search.
    content_search: bool,
    /// Cached preview for the currently selected search result.
    result_preview: Option<(PathBuf, PreviewData)>,
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
    address_bars: HashMap<u64, AddressBar>,
    preview: PreviewOverlay,
    info_panel_preview: Option<(PathBuf, PreviewData)>,
    info_panel_details: Option<(PathBuf, InfoPanelDetails)>,
    image_preview_cache: HashMap<String, Vec<u8>>,
    preview_tx: Sender<PreviewJobRequest>,
    preview_rx: Receiver<PreviewJobResponse>,
    preview_inflight: HashSet<PathBuf>,
    preview_budget_remaining: usize,
    info_panel_hashes: HashMap<PathBuf, String>,
    info_panel_hash_errors: HashMap<PathBuf, String>,
    info_panel_hash_inflight: Option<PathBuf>,
    hash_tx: Sender<(PathBuf, Result<String, String>)>,
    hash_rx: Receiver<(PathBuf, Result<String, String>)>,
    target_dropdown: TargetDropdown,
    properties: PropertiesPopup,
    pending_privileged_retry: Option<(FileCommand, Option<u64>)>,
    privileged_availability: PrivilegedAvailability,
    privileged_status: Option<PrivilegedUiStatus>,
    search_service: SearchService,
    search_started: bool,
    search_ui: SearchUiState,

    next_request_id: u64,
    sidebar: SidebarSection,
    // Last measured grid column count — set each frame by render_grid,
    // used in handle_keyboard for 2D arrow navigation.
    last_grid_cols: usize,
    // Legacy setting kept for compatibility; Column view is now auto-scroll.
    miller_col_count: usize,
    // List view column widths in pixels: Name, Kind, Size, Modified
    list_col_px: [f32; 4],
    // About dialog visible
    show_about: bool,
    // Settings modal visible
    show_settings: bool,
    // Active settings category tab
    settings_tab: SettingsTab,
    // Appearance: transient feedback for save/export actions.
    theme_status: Option<(String, bool, Instant)>,
    // General transient status message (copy, context actions, etc).
    status_message: Option<UiStatusMessage>,
    // Dedicated per-theme editor popup state (KDE-style edit flow).
    theme_editor: ThemeEditorState,
    // One-shot in-app coaching after setting a drop folder from the empty-state hint.
    drop_folder_confirm_until: Option<Instant>,
    // Last measured content rect for the file pane, excluding the in-pane bottom toolbar.
    file_pane_rect: Option<Rect>,
    smart_panel_rect: Option<Rect>,
}

impl Default for OttrinApp {
    fn default() -> Self {
        Self::new_with_config(AppConfig::default())
    }
}

impl Drop for OttrinApp {
    fn drop(&mut self) {
        self.search_service.stop();
    }
}

impl OttrinApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // Register Material Icons as a fallback font.
        // PUA codepoints (U+E000–U+F8FF) used by Material icons are not in
        // NotoSans, so egui automatically falls back to this font for them.
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "material-icons".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/MaterialSymbolsFilled.ttf"
            ))),
        );
        // Push as last fallback so NotoSans still handles normal text
        fonts
            .families
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
        let (hash_tx, hash_rx) = mpsc::channel::<(PathBuf, Result<String, String>)>();
        let (preview_req_tx, preview_req_rx) = mpsc::channel::<PreviewJobRequest>();
        let (preview_res_tx, preview_res_rx) = mpsc::channel::<PreviewJobResponse>();

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

        std::thread::spawn(move || {
            while let Ok(req) = preview_req_rx.recv() {
                let cache_key = preview_cache_key(&req.path, req.target_dim);
                let bytes = if let Some(bytes) = load_cached_thumbnail(&cache_key) {
                    Some(bytes)
                } else if let Some(bytes) = build_thumbnail_bytes(&req.path, req.target_dim) {
                    persist_cached_thumbnail(&cache_key, &bytes);
                    Some(bytes)
                } else {
                    std::fs::read(&req.path).ok()
                };
                let _ = preview_res_tx.send(PreviewJobResponse {
                    path: req.path,
                    target_dim: req.target_dim,
                    bytes,
                });
            }
        });

        let colors = Colors::for_config(&config);

        let mut state = AppState {
            config,
            ..AppState::default()
        };
        if state.config.search.include_roots.is_empty() {
            state.config.search.include_roots = default_search_roots();
        }
        state.config.folder_column_widths.clear();
        let search_config = state.config.search.clone();

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
            address_bars: HashMap::new(),
            preview: PreviewOverlay::default(),
            info_panel_preview: None,
            info_panel_details: None,
            image_preview_cache: HashMap::new(),
            preview_tx: preview_req_tx,
            preview_rx: preview_res_rx,
            preview_inflight: HashSet::new(),
            preview_budget_remaining: PREVIEW_REQUESTS_PER_FRAME,
            info_panel_hashes: HashMap::new(),
            info_panel_hash_errors: HashMap::new(),
            info_panel_hash_inflight: None,
            hash_tx,
            hash_rx,
            target_dropdown: TargetDropdown::default(),
            properties: PropertiesPopup::default(),
            pending_privileged_retry: None,
            privileged_availability: PrivilegedAvailability::Unsupported(
                "Status not initialized".to_string(),
            ),
            privileged_status: None,
            search_service: SearchService::new(search_config),
            search_started: false,
            search_ui: SearchUiState::default(),
            next_request_id: 1,
            sidebar: SidebarSection::None,
            last_grid_cols: 4,
            miller_col_count: 3,
            list_col_px: [360.0, 140.0, 120.0, 180.0],
            show_about: false,
            show_settings: false,
            settings_tab: SettingsTab::General,
            theme_status: None,
            status_message: None,
            theme_editor: ThemeEditorState::default(),
            drop_folder_confirm_until: None,
            file_pane_rect: None,
            smart_panel_rect: None,
        };

        // Theme editing is per preset+mode scope; always hydrate the active
        // scope so users can edit immediately without a separate enable step.
        app.load_or_seed_override_for_scope();
        app.colors = Colors::for_config(&app.state.config);

        // Kick off initial listings for the default tab
        app.search_ui.scope = app.state.config.search.default_scope;
        app.search_ui.sort = app.state.config.search.default_sort;
        app.search_ui.include_hidden = app.state.config.search.include_hidden_system;
        app.search_ui.panel_width = 520.0;
        app.search_service
            .update_config(app.state.config.search.clone());
        app.refresh_active_tab();
        app.privileged_availability = app.platform.privileged_availability();
        app
    }

    fn apply_theme_to_ctx(&self, ctx: &Context) {
        let mut visuals = match self.state.config.theme {
            ThemeMode::Light => egui::Visuals::light(),
            ThemeMode::Dark | ThemeMode::System => egui::Visuals::dark(),
        };
        let c = &self.colors;
        let widget_radius = if self.state.config.theme_custom.enabled {
            self.state
                .config
                .theme_custom
                .button_radius
                .clamp(0.0, 14.0) as u8
        } else {
            c.button_radius
        };
        let (button_bg, button_text) = if self.state.config.theme_custom.enabled {
            (
                Colors::from_rgba(self.state.config.theme_custom.button_bg),
                Colors::from_rgba(self.state.config.theme_custom.button_text),
            )
        } else {
            (c.panel_raised, c.text_muted)
        };
        // Force all major visual slots so no inherited hue survives from
        // egui defaults (which can make dark themes feel warm/purple).
        visuals.override_text_color = Some(c.text_muted);
        visuals.weak_text_color = Some(c.text_muted);
        visuals.hyperlink_color = c.accent;
        visuals.panel_fill = c.panel;
        visuals.window_fill = c.panel; // modals/settings use panel color, not bg
        visuals.window_stroke = Stroke::new(1.0, c.border);
        visuals.window_corner_radius = egui::CornerRadius::same(c.app_radius);
        visuals.menu_corner_radius = egui::CornerRadius::same(c.border_radius);
        visuals.window_shadow = Shadow::NONE;
        visuals.popup_shadow = Shadow::NONE;
        visuals.extreme_bg_color = c.panel_raised;
        visuals.faint_bg_color = c.row_alt;
        visuals.text_edit_bg_color = Some(c.panel_raised);
        visuals.code_bg_color = c.panel_raised;
        visuals.warn_fg_color = c.text_muted;
        visuals.error_fg_color = c.error;
        visuals.widgets.noninteractive.bg_fill = c.panel;
        visuals.widgets.noninteractive.weak_bg_fill = c.panel;
        visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text_muted);
        visuals.widgets.inactive.bg_fill = button_bg;
        visuals.widgets.inactive.weak_bg_fill = button_bg;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, button_text);
        visuals.widgets.hovered.bg_fill = c.hover;
        visuals.widgets.hovered.weak_bg_fill = c.hover;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, c.text_muted);
        visuals.widgets.active.bg_fill = c.accent_dim;
        visuals.widgets.active.weak_bg_fill = c.accent_dim;
        visuals.widgets.active.bg_stroke = Stroke::NONE;
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, c.accent);
        visuals.widgets.open.bg_fill = button_bg;
        visuals.widgets.open.weak_bg_fill = button_bg;
        visuals.widgets.open.bg_stroke = Stroke::NONE;
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(widget_radius);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, button_text);
        visuals.selection.bg_fill = c.selected_bg;
        visuals.selection.stroke = Stroke::NONE;
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            let mut scroll = egui::style::ScrollStyle::solid();
            // Keep a thin visible handle, but hide the track/background.
            // Use floating bars so they don't consume layout width and cause
            // columns to "bounce" when a bar appears/disappears.
            scroll.floating = true;
            scroll.floating_allocated_width = 0.0;
            scroll.bar_width = 6.0;
            scroll.handle_min_length = 28.0;
            scroll.bar_outer_margin = 0.0;
            scroll.bar_inner_margin = 0.0;
            scroll.dormant_background_opacity = 0.0;
            scroll.active_background_opacity = 0.0;
            scroll.interact_background_opacity = 0.0;
            scroll.dormant_handle_opacity = 0.9;
            scroll.active_handle_opacity = 0.95;
            scroll.interact_handle_opacity = 1.0;
            style.spacing.scroll = scroll;
            style.text_styles.insert(
                egui::TextStyle::Heading,
                FontId::proportional(21.0 * c.font_scale),
            );
            style.text_styles.insert(
                egui::TextStyle::Body,
                FontId::proportional(15.0 * c.font_scale),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                FontId::proportional(14.0 * c.font_scale),
            );
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                FontId::monospace(13.0 * c.font_scale),
            );
            style.spacing.button_padding = Vec2::new(10.0, 6.0);
        });
    }

    fn set_status_message(&mut self, text: impl Into<String>, ok: bool, ttl_secs: u64) {
        self.status_message = Some(UiStatusMessage {
            text: text.into(),
            ok,
            until: Instant::now() + Duration::from_secs(ttl_secs),
        });
    }

    fn reapply_theme(&mut self, ctx: &Context) {
        self.colors = Colors::for_config(&self.state.config);
        self.apply_theme_to_ctx(ctx);
    }

    fn cached_image_preview_widget(
        &mut self,
        path: &Path,
        max_width: f32,
        max_height: f32,
        corner_radius: u8,
    ) -> Option<egui::Image<'static>> {
        let max_dim = max_width.max(max_height);
        let target_dim = if max_dim >= 900.0 {
            PREVIEW_CACHE_BUCKET_XL
        } else if max_dim >= 420.0 {
            PREVIEW_CACHE_BUCKET_LARGE
        } else {
            PREVIEW_CACHE_BUCKET_SMALL
        };
        let cache_key = preview_cache_key(path, target_dim);
        if !self.image_preview_cache.contains_key(&cache_key) {
            if !self.preview_inflight.contains(path) && self.preview_budget_remaining > 0 {
                let _ = self.preview_tx.send(PreviewJobRequest {
                    path: path.to_path_buf(),
                    target_dim,
                });
                self.preview_inflight.insert(path.to_path_buf());
                self.preview_budget_remaining = self.preview_budget_remaining.saturating_sub(1);
            }
            return None;
        }
        let bytes = self.image_preview_cache.get(&cache_key)?.clone();
        Some(
            egui::Image::from_bytes(format!("bytes://{}", path.display()), bytes)
                .max_width(max_width)
                .max_height(max_height)
                .corner_radius(corner_radius),
        )
    }

    fn theme_mode_key(mode: ThemeMode) -> &'static str {
        match mode {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::System => "system",
        }
    }

    fn theme_mode_label(mode: ThemeMode) -> &'static str {
        match mode {
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
            ThemeMode::System => "System",
        }
    }

    fn theme_preset_key(preset: ThemePreset) -> &'static str {
        match preset {
            ThemePreset::Ottrin => "ottrin",
            ThemePreset::Breeze => "breeze",
            ThemePreset::Adwaita => "adwaita",
            ThemePreset::Windows11 => "windows11",
            ThemePreset::Solarized => "solarized",
            ThemePreset::Nord => "nord",
            ThemePreset::G33k => "g33k",
        }
    }

    fn theme_preset_label(preset: ThemePreset) -> &'static str {
        match preset {
            ThemePreset::Ottrin => "Ottrin",
            ThemePreset::Breeze => "Breeze",
            ThemePreset::Adwaita => "Adwaita",
            ThemePreset::Windows11 => "Windows 11",
            ThemePreset::Solarized => "Solarized",
            ThemePreset::Nord => "Nord",
            ThemePreset::G33k => "G33k",
        }
    }

    fn theme_preset_icon(preset: ThemePreset) -> &'static str {
        match preset {
            ThemePreset::Ottrin => MI_SETTINGS,
            ThemePreset::Breeze => MI_LIGHT_MODE,
            ThemePreset::Adwaita => MI_HOME,
            ThemePreset::Windows11 => MI_APPS,
            ThemePreset::Solarized => MI_DARK_MODE,
            ThemePreset::Nord => MI_DESCRIPTION,
            ThemePreset::G33k => MI_TERMINAL,
        }
    }

    fn theme_scope_key_for(preset: ThemePreset, mode: ThemeMode) -> String {
        format!(
            "{}::{}",
            Self::theme_preset_key(preset),
            Self::theme_mode_key(mode)
        )
    }

    fn current_theme_scope_key(&self) -> String {
        Self::theme_scope_key_for(self.state.config.theme_preset, self.state.config.theme)
    }

    fn semantic_tokens_for_preset(preset: ThemePreset) -> [[u8; 4]; 10] {
        // folder blue, link blue, steel, soft white, green, orange, purple, pink, red, yellow
        match preset {
            ThemePreset::Ottrin => [
                [81, 154, 186, 255],
                [111, 168, 195, 255],
                [109, 128, 134, 255],
                [232, 236, 235, 255],
                [141, 193, 73, 255],
                [227, 121, 51, 255],
                [160, 116, 196, 255],
                [245, 83, 133, 255],
                [204, 62, 68, 255],
                [203, 203, 65, 255],
            ],
            ThemePreset::Breeze => [
                [61, 174, 233, 255],
                [64, 196, 208, 255],
                [130, 142, 155, 255],
                [252, 252, 252, 255],
                [96, 176, 120, 255],
                [218, 144, 86, 255],
                [143, 136, 214, 255],
                [218, 88, 105, 255],
                [218, 88, 105, 255],
                [193, 157, 88, 255],
            ],
            ThemePreset::Adwaita => [
                [53, 132, 228, 255],
                [63, 164, 196, 255],
                [133, 145, 160, 255],
                [246, 245, 244, 255],
                [89, 163, 122, 255],
                [208, 137, 88, 255],
                [151, 131, 212, 255],
                [214, 81, 99, 255],
                [224, 27, 36, 255],
                [191, 154, 81, 255],
            ],
            ThemePreset::Windows11 => [
                [72, 157, 235, 255],
                [74, 182, 204, 255],
                [133, 144, 156, 255],
                [232, 236, 240, 255],
                [106, 172, 118, 255],
                [206, 141, 96, 255],
                [156, 138, 206, 255],
                [206, 98, 104, 255],
                [206, 98, 104, 255],
                [190, 158, 97, 255],
            ],
            ThemePreset::Solarized => [
                [38, 139, 210, 255],
                [42, 161, 152, 255],
                [131, 148, 150, 255],
                [253, 246, 227, 255],
                [133, 153, 0, 255],
                [203, 75, 22, 255],
                [108, 113, 196, 255],
                [211, 54, 130, 255],
                [220, 50, 47, 255],
                [181, 137, 0, 255],
            ],
            ThemePreset::Nord => [
                [129, 161, 193, 255],
                [136, 192, 208, 255],
                [143, 157, 178, 255],
                [236, 239, 244, 255],
                [163, 190, 140, 255],
                [208, 135, 112, 255],
                [180, 142, 173, 255],
                [191, 97, 106, 255],
                [191, 97, 106, 255],
                [235, 203, 139, 255],
            ],
            ThemePreset::G33k => [
                [96, 214, 255, 255],
                [84, 199, 196, 255],
                [118, 154, 132, 255],
                [131, 255, 147, 255],
                [131, 225, 122, 255],
                [212, 150, 93, 255],
                [168, 144, 215, 255],
                [246, 114, 114, 255],
                [246, 114, 114, 255],
                [201, 171, 97, 255],
            ],
        }
    }

    fn custom_from_colors(colors: Colors, preset: ThemePreset) -> ThemeCustomization {
        let tokens = Self::semantic_tokens_for_preset(preset);
        ThemeCustomization {
            enabled: true,
            background: Colors::to_rgba(colors.bg),
            panel: Colors::to_rgba(colors.panel),
            toolbar: Colors::to_rgba(colors.toolbar_bg),
            bookmarks_bar: Colors::to_rgba(colors.bookmarks_bg),
            smart_panel: Colors::to_rgba(colors.smart_panel_bg),
            titlebar: Colors::to_rgba(colors.titlebar_bg),
            border: Colors::to_rgba(colors.border),
            window_border: Colors::to_rgba(colors.window_border),
            window_border_width: colors.window_border_width,
            accent: Colors::to_rgba(colors.accent),
            folder: Colors::to_rgba(colors.folder),
            button_bg: Colors::to_rgba(colors.panel_raised),
            button_text: Colors::to_rgba(colors.text_muted),
            text_heading: Colors::to_rgba(colors.heading),
            text_folder: Colors::to_rgba(colors.text_dim),
            text_file: Colors::to_rgba(colors.file),
            palette_folder_blue: tokens[0],
            palette_link_blue: tokens[1],
            palette_steel: tokens[2],
            palette_soft_white: tokens[3],
            palette_green: tokens[4],
            palette_orange: tokens[5],
            palette_purple: tokens[6],
            palette_pink: tokens[7],
            palette_red: tokens[8],
            palette_yellow: tokens[9],
            app_radius: colors.app_radius as f32,
            border_radius: colors.border_radius as f32,
            button_radius: colors.button_radius as f32,
            font_scale: colors.font_scale,
        }
    }

    fn save_current_override_for_scope(&mut self) {
        let key = self.current_theme_scope_key();
        self.state
            .config
            .theme_custom_by_preset
            .insert(key, self.state.config.theme_custom.clone());
    }

    fn load_or_seed_override_for_scope(&mut self) {
        let key = self.current_theme_scope_key();
        if let Some(saved) = self.state.config.theme_custom_by_preset.get(&key).cloned() {
            self.state.config.theme_custom = saved;
            self.state.config.theme_custom.enabled = true;
            return;
        }
        let base = Colors::base_for_config(&self.state.config);
        let custom = Self::custom_from_colors(base, self.state.config.theme_preset);
        self.state.config.theme_custom = custom.clone();
        self.state.config.theme_custom_by_preset.insert(key, custom);
    }

    fn apply_theme_preset(&mut self, preset: ThemePreset, ctx: &Context) {
        // Switching to a built-in preset should give a clean default, not a
        // stale override that preserved previous font_scale / palette edits.
        self.state.config.theme_preset = preset;
        self.state.config.active_custom_theme = None;
        let key = self.current_theme_scope_key();
        self.state.config.theme_custom_by_preset.remove(&key);
        self.load_or_seed_override_for_scope();
        self.reapply_theme(ctx);
    }

    fn apply_theme_mode(&mut self, mode: ThemeMode, ctx: &Context) {
        self.save_current_override_for_scope();
        self.state.config.theme = mode;
        self.load_or_seed_override_for_scope();
        self.reapply_theme(ctx);
    }

    fn apply_saved_custom_theme(&mut self, saved: &SavedTheme, ctx: &Context) {
        self.state.config.theme = saved.theme_mode;
        self.state.config.theme_preset = saved.base_preset;
        self.state.config.theme_custom = saved.customization.clone();
        self.state.config.theme_custom.enabled = true;
        self.state.config.active_custom_theme = Some(saved.name.clone());
        self.save_current_override_for_scope();
        self.reapply_theme(ctx);
    }

    fn preview_colors_for_scope(&self, preset: ThemePreset, mode: ThemeMode) -> Colors {
        let mut cfg = self.state.config.clone();
        cfg.theme_preset = preset;
        cfg.theme = mode;
        let key = Self::theme_scope_key_for(preset, mode);
        if let Some(saved) = cfg.theme_custom_by_preset.get(&key).cloned() {
            cfg.theme_custom = saved;
            cfg.theme_custom.enabled = true;
        } else {
            cfg.theme_custom.enabled = false;
        }
        Colors::for_config(&cfg)
    }

    fn theme_scope_custom_or_seed(
        &self,
        preset: ThemePreset,
        mode: ThemeMode,
    ) -> ThemeCustomization {
        let key = Self::theme_scope_key_for(preset, mode);
        if let Some(saved) = self.state.config.theme_custom_by_preset.get(&key).cloned() {
            let mut scoped = saved;
            scoped.enabled = true;
            return scoped;
        }
        let dark = matches!(mode, ThemeMode::Dark | ThemeMode::System);
        let base = Colors::preset(preset, dark);
        Self::custom_from_colors(base, preset)
    }

    fn theme_editor_scope_is_active(&self) -> bool {
        self.theme_editor.preset == self.state.config.theme_preset
            && self.theme_editor.mode == self.state.config.theme
    }

    fn theme_preview_region_label(region: ThemePreviewRegion) -> &'static str {
        match region {
            ThemePreviewRegion::Titlebar => "Title bar",
            ThemePreviewRegion::Navigation => "Navigation bar",
            ThemePreviewRegion::Bookmarks => "Bookmarks bar",
            ThemePreviewRegion::ColumnBackground => "Column background",
            ThemePreviewRegion::SelectedRow => "Selected row",
            ThemePreviewRegion::ColumnText => "Column text",
            ThemePreviewRegion::Sidebar => "Sidebar",
            ThemePreviewRegion::StatusBar => "Status bar",
            ThemePreviewRegion::Accent => "Accent details",
            ThemePreviewRegion::Borders => "Borders",
        }
    }

    fn theme_preview_region_hint(region: ThemePreviewRegion) -> &'static str {
        match region {
            ThemePreviewRegion::Titlebar => "Window chrome, tab shelf, and top heading contrast.",
            ThemePreviewRegion::Navigation => "Back controls, address field, and toolbar rhythm.",
            ThemePreviewRegion::Bookmarks => "Quick destination chips sitting under navigation.",
            ThemePreviewRegion::ColumnBackground => "The main browsing canvas and column surfaces.",
            ThemePreviewRegion::SelectedRow => {
                "Focused file row, active state, and emphasis balance."
            }
            ThemePreviewRegion::ColumnText => "Neutral folder/file labels and default icon tint.",
            ThemePreviewRegion::Sidebar => "Info/search/target strip and its supporting contrast.",
            ThemePreviewRegion::StatusBar => "Bottom utility bar and low-emphasis metadata.",
            ThemePreviewRegion::Accent => {
                "Selection highlight, active underline, and call-to-action color."
            }
            ThemePreviewRegion::Borders => "Dividers, outlines, and window-edge definition.",
        }
    }

    fn theme_preview_region_fields(region: ThemePreviewRegion) -> &'static [ThemeColorField] {
        match region {
            ThemePreviewRegion::Titlebar => &[
                ThemeColorField::Titlebar,
                ThemeColorField::TextHeading,
                ThemeColorField::Border,
            ],
            ThemePreviewRegion::Navigation => &[
                ThemeColorField::Toolbar,
                ThemeColorField::Border,
                ThemeColorField::TextFile,
            ],
            ThemePreviewRegion::Bookmarks => &[
                ThemeColorField::Toolbar,
                ThemeColorField::Accent,
                ThemeColorField::TextFolder,
            ],
            ThemePreviewRegion::ColumnBackground => &[
                ThemeColorField::Background,
                ThemeColorField::Panel,
                ThemeColorField::Border,
            ],
            ThemePreviewRegion::SelectedRow => &[
                ThemeColorField::Accent,
                ThemeColorField::TextFolder,
                ThemeColorField::TextFile,
            ],
            ThemePreviewRegion::ColumnText => &[
                ThemeColorField::TextHeading,
                ThemeColorField::TextFolder,
                ThemeColorField::TextFile,
                ThemeColorField::Folder,
            ],
            ThemePreviewRegion::Sidebar => &[
                ThemeColorField::SmartPanel,
                ThemeColorField::Accent,
                ThemeColorField::Border,
            ],
            ThemePreviewRegion::StatusBar => &[
                ThemeColorField::Titlebar,
                ThemeColorField::TextFile,
                ThemeColorField::Border,
            ],
            ThemePreviewRegion::Accent => &[
                ThemeColorField::Accent,
                ThemeColorField::ButtonBg,
                ThemeColorField::ButtonText,
            ],
            ThemePreviewRegion::Borders => &[
                ThemeColorField::Border,
                ThemeColorField::WindowBorder,
                ThemeColorField::Panel,
            ],
        }
    }

    fn theme_color_field_order() -> [ThemeColorField; 24] {
        [
            ThemeColorField::Background,
            ThemeColorField::Panel,
            ThemeColorField::Titlebar,
            ThemeColorField::Toolbar,
            ThemeColorField::SmartPanel,
            ThemeColorField::Border,
            ThemeColorField::WindowBorder,
            ThemeColorField::Accent,
            ThemeColorField::ButtonBg,
            ThemeColorField::ButtonText,
            ThemeColorField::Folder,
            ThemeColorField::TextHeading,
            ThemeColorField::TextFolder,
            ThemeColorField::TextFile,
            ThemeColorField::PaletteSoftWhite,
            ThemeColorField::PaletteFolderBlue,
            ThemeColorField::PaletteLinkBlue,
            ThemeColorField::PaletteSteel,
            ThemeColorField::PaletteGreen,
            ThemeColorField::PaletteOrange,
            ThemeColorField::PalettePurple,
            ThemeColorField::PalettePink,
            ThemeColorField::PaletteRed,
            ThemeColorField::PaletteYellow,
        ]
    }

    fn theme_color_field_label(field: ThemeColorField) -> &'static str {
        match field {
            ThemeColorField::Background => "Content background",
            ThemeColorField::Panel => "Panels and dialogs",
            ThemeColorField::Toolbar => "Toolbar + tabs",
            ThemeColorField::SmartPanel => "Smart panel (info/search/target)",
            ThemeColorField::Titlebar => "Title bar",
            ThemeColorField::Border => "Borders and dividers",
            ThemeColorField::WindowBorder => "Window border",
            ThemeColorField::Accent => "Selection/active accent",
            ThemeColorField::Folder => "Folder icon",
            ThemeColorField::ButtonBg => "Filled button background",
            ThemeColorField::ButtonText => "Filled button text",
            ThemeColorField::TextHeading => "Heading text",
            ThemeColorField::TextFolder => "Folder text (neutral)",
            ThemeColorField::TextFile => "File text (neutral)",
            ThemeColorField::PaletteSoftWhite => "Neutral (semantic base)",
            ThemeColorField::PaletteFolderBlue => "Folder blue",
            ThemeColorField::PaletteLinkBlue => "Link blue",
            ThemeColorField::PaletteSteel => "Steel",
            ThemeColorField::PaletteGreen => "Green",
            ThemeColorField::PaletteOrange => "Orange",
            ThemeColorField::PalettePurple => "Purple",
            ThemeColorField::PalettePink => "Pink",
            ThemeColorField::PaletteRed => "Red",
            ThemeColorField::PaletteYellow => "Yellow",
        }
    }

    fn theme_color_field_short_label(field: ThemeColorField) -> &'static str {
        match field {
            ThemeColorField::Background => "Content",
            ThemeColorField::Panel => "Panels",
            ThemeColorField::Toolbar => "Toolbar/tabs",
            ThemeColorField::SmartPanel => "Smart panel",
            ThemeColorField::Titlebar => "Title bar",
            ThemeColorField::Border => "Borders",
            ThemeColorField::WindowBorder => "Window",
            ThemeColorField::Accent => "Accent",
            ThemeColorField::Folder => "Folder icon",
            ThemeColorField::ButtonBg => "Button bg",
            ThemeColorField::ButtonText => "Button text",
            ThemeColorField::TextHeading => "Heading",
            ThemeColorField::TextFolder => "Folder text",
            ThemeColorField::TextFile => "File text",
            ThemeColorField::PaletteSoftWhite => "Neutral",
            ThemeColorField::PaletteFolderBlue => "Folder blue",
            ThemeColorField::PaletteLinkBlue => "Link blue",
            ThemeColorField::PaletteSteel => "Steel",
            ThemeColorField::PaletteGreen => "Green",
            ThemeColorField::PaletteOrange => "Orange",
            ThemeColorField::PalettePurple => "Purple",
            ThemeColorField::PalettePink => "Pink",
            ThemeColorField::PaletteRed => "Red",
            ThemeColorField::PaletteYellow => "Yellow",
        }
    }

    fn theme_color_field_hint(field: ThemeColorField) -> &'static str {
        match field {
            ThemeColorField::Background => "File area and input surfaces",
            ThemeColorField::Panel => "Cards, dialogs, raised surfaces",
            ThemeColorField::Toolbar => "Tabs, nav row, bookmarks, address",
            ThemeColorField::SmartPanel => "Info/search/target sidebar",
            ThemeColorField::Titlebar => "Top window chrome",
            ThemeColorField::Border => "Thin separators and strokes",
            ThemeColorField::WindowBorder => "Border around application windows",
            ThemeColorField::Accent => "Selection and highlight color",
            ThemeColorField::Folder => "Default folder/icon tint",
            ThemeColorField::ButtonBg => "Filled button surface",
            ThemeColorField::ButtonText => "Filled button label",
            ThemeColorField::TextHeading => "Titles and strong labels",
            ThemeColorField::TextFolder => "Base folder labels (semantic off)",
            ThemeColorField::TextFile => "Base file labels (semantic off)",
            ThemeColorField::PaletteSoftWhite => "Semantic neutral token",
            ThemeColorField::PaletteFolderBlue => "Primary directory token",
            ThemeColorField::PaletteLinkBlue => "Link/symlink token",
            ThemeColorField::PaletteSteel => "Muted utility token",
            ThemeColorField::PaletteGreen => "Positive/script token",
            ThemeColorField::PaletteOrange => "Archive/media token",
            ThemeColorField::PalettePurple => "Image/design token",
            ThemeColorField::PalettePink => "Accent semantic token",
            ThemeColorField::PaletteRed => "Danger/error token",
            ThemeColorField::PaletteYellow => "Warning/config token",
        }
    }

    fn theme_color_field_value(custom: &ThemeCustomization, field: ThemeColorField) -> [u8; 4] {
        match field {
            ThemeColorField::Background => custom.background,
            ThemeColorField::Panel => custom.panel,
            ThemeColorField::Toolbar => custom.toolbar,
            ThemeColorField::SmartPanel => custom.smart_panel,
            ThemeColorField::Titlebar => custom.titlebar,
            ThemeColorField::Border => custom.border,
            ThemeColorField::WindowBorder => custom.window_border,
            ThemeColorField::Accent => custom.accent,
            ThemeColorField::Folder => custom.folder,
            ThemeColorField::ButtonBg => custom.button_bg,
            ThemeColorField::ButtonText => custom.button_text,
            ThemeColorField::TextHeading => custom.text_heading,
            ThemeColorField::TextFolder => custom.text_folder,
            ThemeColorField::TextFile => custom.text_file,
            ThemeColorField::PaletteSoftWhite => custom.palette_soft_white,
            ThemeColorField::PaletteFolderBlue => custom.palette_folder_blue,
            ThemeColorField::PaletteLinkBlue => custom.palette_link_blue,
            ThemeColorField::PaletteSteel => custom.palette_steel,
            ThemeColorField::PaletteGreen => custom.palette_green,
            ThemeColorField::PaletteOrange => custom.palette_orange,
            ThemeColorField::PalettePurple => custom.palette_purple,
            ThemeColorField::PalettePink => custom.palette_pink,
            ThemeColorField::PaletteRed => custom.palette_red,
            ThemeColorField::PaletteYellow => custom.palette_yellow,
        }
    }

    fn theme_color_field_value_mut(
        custom: &mut ThemeCustomization,
        field: ThemeColorField,
    ) -> &mut [u8; 4] {
        match field {
            ThemeColorField::Background => &mut custom.background,
            ThemeColorField::Panel => &mut custom.panel,
            ThemeColorField::Toolbar => &mut custom.toolbar,
            ThemeColorField::SmartPanel => &mut custom.smart_panel,
            ThemeColorField::Titlebar => &mut custom.titlebar,
            ThemeColorField::Border => &mut custom.border,
            ThemeColorField::WindowBorder => &mut custom.window_border,
            ThemeColorField::Accent => &mut custom.accent,
            ThemeColorField::Folder => &mut custom.folder,
            ThemeColorField::ButtonBg => &mut custom.button_bg,
            ThemeColorField::ButtonText => &mut custom.button_text,
            ThemeColorField::TextHeading => &mut custom.text_heading,
            ThemeColorField::TextFolder => &mut custom.text_folder,
            ThemeColorField::TextFile => &mut custom.text_file,
            ThemeColorField::PaletteSoftWhite => &mut custom.palette_soft_white,
            ThemeColorField::PaletteFolderBlue => &mut custom.palette_folder_blue,
            ThemeColorField::PaletteLinkBlue => &mut custom.palette_link_blue,
            ThemeColorField::PaletteSteel => &mut custom.palette_steel,
            ThemeColorField::PaletteGreen => &mut custom.palette_green,
            ThemeColorField::PaletteOrange => &mut custom.palette_orange,
            ThemeColorField::PalettePurple => &mut custom.palette_purple,
            ThemeColorField::PalettePink => &mut custom.palette_pink,
            ThemeColorField::PaletteRed => &mut custom.palette_red,
            ThemeColorField::PaletteYellow => &mut custom.palette_yellow,
        }
    }

    fn sync_theme_editor_hex_inputs(&mut self) {
        self.theme_editor.color_hex_inputs.clear();
        for field in Self::theme_color_field_order() {
            let rgba = Self::theme_color_field_value(&self.theme_editor.custom, field);
            self.theme_editor.color_hex_inputs.insert(
                field,
                format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]),
            );
        }
    }

    fn reset_theme_editor_sliders(&mut self) {
        self.theme_editor.main_hue_shift = 0.0;
        self.theme_editor.main_saturation = 1.0;
        self.theme_editor.main_brightness = 1.0;
        self.theme_editor.main_contrast = 1.0;
        self.theme_editor.text_hue_shift = 0.0;
        self.theme_editor.text_saturation = 1.0;
        self.theme_editor.text_brightness = 1.0;
        self.theme_editor.text_contrast = 1.0;
        self.theme_editor.color_picker_open = false;
        self.theme_editor.color_picker_field = None;
        self.sync_theme_editor_hex_inputs();
    }

    fn reset_theme_editor_quick_sliders_only(&mut self) {
        self.theme_editor.main_hue_shift = 0.0;
        self.theme_editor.main_saturation = 1.0;
        self.theme_editor.main_brightness = 1.0;
        self.theme_editor.main_contrast = 1.0;
        self.theme_editor.text_hue_shift = 0.0;
        self.theme_editor.text_saturation = 1.0;
        self.theme_editor.text_brightness = 1.0;
        self.theme_editor.text_contrast = 1.0;
    }

    fn reanchor_theme_editor_quick_from_current(&mut self) {
        self.theme_editor.base_custom = self.theme_editor.custom.clone();
        self.reset_theme_editor_quick_sliders_only();
        self.sync_theme_editor_hex_inputs();
    }

    fn switch_theme_editor_scope(&mut self, preset: ThemePreset, mode: ThemeMode, ctx: &Context) {
        if self.theme_editor.source_saved_idx.is_some() {
            return;
        }
        if self.theme_editor.preset == preset && self.theme_editor.mode == mode {
            return;
        }
        if self.theme_editor_scope_is_active() {
            self.state.config.theme_custom = self.theme_editor.original_custom.clone();
            self.state.config.theme_custom.enabled = true;
            self.reapply_theme(ctx);
        }

        let custom = self.theme_scope_custom_or_seed(preset, mode);
        self.theme_editor.preset = preset;
        self.theme_editor.mode = mode;
        self.theme_editor.base_custom = custom.clone();
        self.theme_editor.original_custom = custom.clone();
        self.theme_editor.custom = custom;
        self.theme_editor.save_as_name = format!(
            "{} {}",
            Self::theme_preset_label(preset),
            Self::theme_mode_label(mode)
        );
        self.reset_theme_editor_sliders();
        self.apply_theme_editor_live_preview(ctx);
    }

    fn open_theme_editor_for_preset(&mut self, preset: ThemePreset, ctx: &Context) {
        let mode = self.state.config.theme;
        let custom = self.theme_scope_custom_or_seed(preset, mode);
        self.theme_editor.open = true;
        self.theme_editor.tab = ThemeEditorTab::MainInterface;
        self.theme_editor.selected_region = ThemePreviewRegion::ColumnBackground;
        self.theme_editor.preset = preset;
        self.theme_editor.mode = mode;
        self.theme_editor.source_saved_idx = None;
        self.theme_editor.base_custom = custom.clone();
        self.theme_editor.original_custom = custom.clone();
        self.theme_editor.custom = custom;
        self.theme_editor.save_as_name = format!(
            "{} {}",
            Self::theme_preset_label(preset),
            Self::theme_mode_label(mode)
        );
        self.reset_theme_editor_sliders();
        self.apply_theme_editor_live_preview(ctx);
    }

    fn open_theme_editor_for_saved(&mut self, index: usize, ctx: &Context) {
        if let Some(saved) = self.state.config.custom_themes.get(index).cloned() {
            self.theme_editor.open = true;
            self.theme_editor.tab = ThemeEditorTab::MainInterface;
            self.theme_editor.selected_region = ThemePreviewRegion::ColumnBackground;
            self.theme_editor.preset = saved.base_preset;
            self.theme_editor.mode = saved.theme_mode;
            self.theme_editor.source_saved_idx = Some(index);
            self.theme_editor.base_custom = saved.customization.clone();
            self.theme_editor.original_custom = saved.customization.clone();
            self.theme_editor.custom = saved.customization;
            self.theme_editor.save_as_name = saved.name;
            self.reset_theme_editor_sliders();
            self.apply_theme_editor_live_preview(ctx);
        }
    }

    fn apply_theme_editor_live_preview(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }
        if self.theme_editor_scope_is_active() {
            self.state.config.theme_custom = self.theme_editor.custom.clone();
            self.state.config.theme_custom.enabled = true;
            self.reapply_theme(ctx);
        }
        ctx.request_repaint();
    }

    fn cancel_theme_editor(&mut self, ctx: &Context) {
        if self.theme_editor_scope_is_active() {
            self.state.config.theme_custom = self.theme_editor.original_custom.clone();
            self.state.config.theme_custom.enabled = true;
            self.reapply_theme(ctx);
        }
        self.theme_editor = ThemeEditorState::default();
    }

    fn is_builtin_theme_name(name: &str) -> bool {
        let lower = name.trim().to_ascii_lowercase();
        matches!(
            lower.as_str(),
            "ottrin" | "breeze" | "adwaita" | "windows 11" | "solarized" | "nord" | "g33k"
        )
    }

    fn generate_save_as_name(&self, base: &str) -> String {
        let candidate = if Self::is_builtin_theme_name(base) {
            format!("{} (edited)", base.trim())
        } else {
            base.trim().to_string()
        };
        if !self
            .state
            .config
            .custom_themes
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
        for n in 2..=99 {
            let numbered = format!("{} (edited {})", base.trim(), n);
            if !self
                .state
                .config
                .custom_themes
                .iter()
                .any(|t| t.name.eq_ignore_ascii_case(&numbered))
            {
                return numbered;
            }
        }
        format!("{} (edited)", base.trim())
    }

    fn save_theme_editor(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }
        self.theme_editor.custom.enabled = true;
        if let Some(idx) = self.theme_editor.source_saved_idx {
            // Editing a user-created theme: overwrite in place.
            if let Some(saved) = self.state.config.custom_themes.get_mut(idx) {
                saved.theme_mode = self.theme_editor.mode;
                saved.base_preset = self.theme_editor.preset;
                saved.customization = self.theme_editor.custom.clone();
            }
            self.theme_status = Some((
                "Custom theme updated".to_string(),
                true,
                Instant::now() + Duration::from_secs(3),
            ));
            self.theme_editor.open = false;
        } else {
            // Editing a built-in preset: save as a new custom theme instead
            // of overwriting the built-in.
            if self.theme_editor.save_as_name.trim().is_empty() {
                let base_label = Self::theme_preset_label(self.theme_editor.preset);
                self.theme_editor.save_as_name = self.generate_save_as_name(base_label);
            }
            self.save_theme_editor_as_inner(ctx);
        }
    }

    fn save_theme_editor_as(&mut self) {
        if !self.theme_editor.open {
            return;
        }
        self.do_save_theme_as(false);
    }

    fn save_theme_editor_as_inner(&mut self, ctx: &Context) {
        self.do_save_theme_as(true);
        if self.theme_editor.custom.enabled {
            self.reapply_theme(ctx);
        }
    }

    fn do_save_theme_as(&mut self, close_and_apply: bool) {
        let name = self.theme_editor.save_as_name.trim().to_string();
        if name.is_empty() {
            self.theme_status = Some((
                "Enter a theme name to save".to_string(),
                false,
                Instant::now() + Duration::from_secs(4),
            ));
            return;
        }
        if Self::is_builtin_theme_name(&name) {
            self.theme_editor.save_as_name = self.generate_save_as_name(&name);
            self.theme_status = Some((
                format!(
                    "\"{}\" is a built-in theme — renamed to \"{}\"",
                    name, self.theme_editor.save_as_name
                ),
                false,
                Instant::now() + Duration::from_secs(4),
            ));
            return;
        }
        let saved = SavedTheme {
            name: name.clone(),
            theme_mode: self.theme_editor.mode,
            base_preset: self.theme_editor.preset,
            customization: self.theme_editor.custom.clone(),
        };
        if let Some(existing) = self
            .state
            .config
            .custom_themes
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(&name))
        {
            *existing = saved;
        } else {
            self.state.config.custom_themes.push(saved);
        }
        if close_and_apply {
            self.state.config.theme = self.theme_editor.mode;
            self.state.config.theme_preset = self.theme_editor.preset;
            self.state.config.theme_custom = self.theme_editor.custom.clone();
            self.state.config.theme_custom.enabled = true;
            self.theme_editor.open = false;
        }
        self.theme_status = Some((
            format!("Saved \"{}\"", name),
            true,
            Instant::now() + Duration::from_secs(3),
        ));
    }

    fn export_theme_editor_json(&mut self) {
        if !self.theme_editor.open {
            return;
        }
        let mut customization = self.theme_editor.custom.clone();
        customization.enabled = true;
        let save_name = self.theme_editor.save_as_name.trim();
        let fallback_name = format!(
            "{} {}",
            Self::theme_preset_label(self.theme_editor.preset),
            Self::theme_mode_label(self.theme_editor.mode)
        );
        let theme_name = if save_name.is_empty() {
            fallback_name
        } else {
            save_name.to_string()
        };
        let export_theme = SavedTheme {
            name: theme_name.clone(),
            theme_mode: self.theme_editor.mode,
            base_preset: self.theme_editor.preset,
            customization,
        };
        let default_file_name = sanitize_theme_file_name(&format!("{}.json", theme_name));
        let dialog = rfd::FileDialog::new()
            .add_filter("Ottrin Theme JSON", &["json"])
            .set_file_name(default_file_name.as_str());
        let Some(path) = dialog.save_file() else {
            return;
        };

        let payload = serde_json::json!({
            "format": "ottrin-theme",
            "version": 1,
            "theme": export_theme,
        });
        let json = match serde_json::to_string_pretty(&payload) {
            Ok(json) => json,
            Err(err) => {
                self.theme_status = Some((
                    format!("Theme export failed: {}", err),
                    false,
                    Instant::now() + Duration::from_secs(4),
                ));
                return;
            }
        };

        match std::fs::write(&path, json) {
            Ok(()) => {
                self.theme_status = Some((
                    format!("Exported theme to {}", path.display()),
                    true,
                    Instant::now() + Duration::from_secs(3),
                ));
            }
            Err(err) => {
                self.theme_status = Some((
                    format!("Theme export failed: {}", err),
                    false,
                    Instant::now() + Duration::from_secs(4),
                ));
            }
        }
    }

    fn import_theme_editor_json(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }
        let dialog = rfd::FileDialog::new().add_filter("Ottrin Theme JSON", &["json"]);
        let Some(path) = dialog.pick_file() else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.theme_status = Some((
                    format!("Theme import failed: {}", err),
                    false,
                    Instant::now() + Duration::from_secs(4),
                ));
                return;
            }
        };

        let fallback_name = theme_name_from_path(&path);
        let imported = match decode_theme_import_json(
            &bytes,
            fallback_name.as_str(),
            self.theme_editor.preset,
            self.theme_editor.mode,
        ) {
            Ok(saved) => saved,
            Err(err) => {
                self.theme_status = Some((
                    format!("Theme import failed: {}", err),
                    false,
                    Instant::now() + Duration::from_secs(4),
                ));
                return;
            }
        };

        let mut imported_name = imported.name.trim().to_string();
        if imported_name.is_empty() {
            imported_name = fallback_name;
        }
        if Self::is_builtin_theme_name(&imported_name) {
            imported_name = self.generate_save_as_name(&imported_name);
        }

        let mut imported_custom = imported.customization;
        imported_custom.enabled = true;
        self.theme_editor.source_saved_idx = None;
        self.theme_editor.preset = imported.base_preset;
        self.theme_editor.mode = imported.theme_mode;
        self.theme_editor.base_custom = imported_custom.clone();
        self.theme_editor.original_custom = imported_custom.clone();
        self.theme_editor.custom = imported_custom;
        self.theme_editor.save_as_name = imported_name.clone();
        self.theme_editor.selected_region = ThemePreviewRegion::ColumnBackground;
        self.reset_theme_editor_sliders();
        self.apply_theme_editor_live_preview(ctx);
        self.theme_status = Some((
            format!("Imported \"{}\"", imported_name),
            true,
            Instant::now() + Duration::from_secs(3),
        ));
    }

    fn reset_theme_editor_to_default(&mut self, ctx: &Context) {
        if !self.theme_editor.open || self.theme_editor.source_saved_idx.is_some() {
            return;
        }
        let dark = matches!(self.theme_editor.mode, ThemeMode::Dark | ThemeMode::System);
        let base = Colors::preset(self.theme_editor.preset, dark);
        let custom = Self::custom_from_colors(base, self.theme_editor.preset);
        self.theme_editor.base_custom = custom.clone();
        self.theme_editor.custom = custom;
        self.reset_theme_editor_sliders();
        self.apply_theme_editor_live_preview(ctx);
    }

    fn apply_theme_editor_main_quick(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }
        let h = self.theme_editor.main_hue_shift;
        let s = self.theme_editor.main_saturation.clamp(0.6, 1.6);
        let v = self.theme_editor.main_brightness.clamp(0.65, 1.35);
        let contrast = self.theme_editor.main_contrast.clamp(0.75, 1.35);
        let tune = |rgba: [u8; 4]| -> [u8; 4] {
            let shifted = shift_hsva(Colors::from_rgba(rgba), h, s, v);
            Colors::to_rgba(apply_contrast(shifted, contrast))
        };

        let mut custom = self.theme_editor.base_custom.clone();
        custom.background = tune(self.theme_editor.base_custom.background);
        custom.toolbar = tune(self.theme_editor.base_custom.toolbar);
        custom.panel = tune(self.theme_editor.base_custom.panel);
        custom.smart_panel = tune(self.theme_editor.base_custom.smart_panel);
        custom.titlebar = tune(self.theme_editor.base_custom.titlebar);
        custom.border = tune(self.theme_editor.base_custom.border);
        custom.window_border = tune(self.theme_editor.base_custom.window_border);
        custom.accent = tune(self.theme_editor.base_custom.accent);
        custom.folder = tune(self.theme_editor.base_custom.folder);
        custom.button_bg = tune(self.theme_editor.base_custom.button_bg);
        custom.text_heading = tune(self.theme_editor.base_custom.text_heading);
        custom.text_folder = tune(self.theme_editor.base_custom.text_folder);
        custom.text_file = tune(self.theme_editor.base_custom.text_file);

        self.theme_editor.custom = custom;
        self.sync_theme_editor_hex_inputs();
        self.apply_theme_editor_live_preview(ctx);
    }

    fn apply_theme_editor_text_quick(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }
        let h = self.theme_editor.text_hue_shift;
        let s = self.theme_editor.text_saturation.clamp(0.6, 1.6);
        let v = self.theme_editor.text_brightness.clamp(0.65, 1.35);
        let contrast = self.theme_editor.text_contrast.clamp(0.75, 1.35);
        let tune = |rgba: [u8; 4]| -> [u8; 4] {
            let shifted = shift_hsva(Colors::from_rgba(rgba), h, s, v);
            Colors::to_rgba(apply_contrast(shifted, contrast))
        };

        let mut custom = self.theme_editor.base_custom.clone();
        custom.text_heading = tune(self.theme_editor.base_custom.text_heading);
        custom.text_folder = tune(self.theme_editor.base_custom.text_folder);
        custom.text_file = tune(self.theme_editor.base_custom.text_file);
        custom.button_text = tune(self.theme_editor.base_custom.button_text);
        custom.palette_soft_white = tune(self.theme_editor.base_custom.palette_soft_white);
        custom.palette_folder_blue = tune(self.theme_editor.base_custom.palette_folder_blue);
        custom.palette_link_blue = tune(self.theme_editor.base_custom.palette_link_blue);
        custom.palette_steel = tune(self.theme_editor.base_custom.palette_steel);
        custom.palette_green = tune(self.theme_editor.base_custom.palette_green);
        custom.palette_orange = tune(self.theme_editor.base_custom.palette_orange);
        custom.palette_purple = tune(self.theme_editor.base_custom.palette_purple);
        custom.palette_pink = tune(self.theme_editor.base_custom.palette_pink);
        custom.palette_red = tune(self.theme_editor.base_custom.palette_red);
        custom.palette_yellow = tune(self.theme_editor.base_custom.palette_yellow);

        self.theme_editor.custom = custom;
        self.sync_theme_editor_hex_inputs();
        self.apply_theme_editor_live_preview(ctx);
    }

    fn theme_editor_preview_colors(&self) -> Colors {
        let mut cfg = self.state.config.clone();
        cfg.theme = self.theme_editor.mode;
        cfg.theme_preset = self.theme_editor.preset;
        cfg.theme_custom = self.theme_editor.custom.clone();
        cfg.theme_custom.enabled = true;
        Colors::for_config(&cfg)
    }

    fn render_appearance_settings(&mut self, ui: &mut egui::Ui, ctx: &Context, c: Colors) {
        ui.label(
            RichText::new("Appearance")
                .color(c.text)
                .size(14.0)
                .strong(),
        );
        ui.label(
            RichText::new("Pick a preset, then edit that theme in a dedicated popup.")
                .color(c.text_muted)
                .size(11.0),
        );
        ui.add_space(10.0);

        if let Some((_, _, until)) = &self.theme_status
            && Instant::now() > *until
        {
            self.theme_status = None;
        }
        if let Some((msg, ok, _)) = &self.theme_status {
            ui.label(
                RichText::new(msg)
                    .color(if *ok { c.accent } else { c.error })
                    .size(11.0),
            );
            ui.add_space(6.0);
        }

        let compact = ui.available_width() < 640.0;
        if compact {
            ui.vertical(|ui| {
                ui.label(RichText::new("Mode").color(c.text_dim).size(11.5).strong());
                let mut mode = self.state.config.theme;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut mode, ThemeMode::Dark, "Dark");
                    ui.selectable_value(&mut mode, ThemeMode::Light, "Light");
                    ui.selectable_value(&mut mode, ThemeMode::System, "System");
                });
                if mode != self.state.config.theme {
                    self.apply_theme_mode(mode, ctx);
                }
                ui.add_space(6.0);
                let mut colorize = self.state.config.colorize_file_types;
                if ui.checkbox(&mut colorize, "Semantic file colors").changed() {
                    self.state.config.colorize_file_types = colorize;
                }
                if self.state.config.colorize_file_types {
                    let mut folders = self.state.config.colorize_folder_labels;
                    if ui.checkbox(&mut folders, "Include folder labels").changed() {
                        self.state.config.colorize_folder_labels = folders;
                    }
                }
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Mode").color(c.text_dim).size(11.5).strong());
                let mut mode = self.state.config.theme;
                ui.selectable_value(&mut mode, ThemeMode::Dark, "Dark");
                ui.selectable_value(&mut mode, ThemeMode::Light, "Light");
                ui.selectable_value(&mut mode, ThemeMode::System, "System");
                if mode != self.state.config.theme {
                    self.apply_theme_mode(mode, ctx);
                }
                ui.separator();
                let mut colorize = self.state.config.colorize_file_types;
                if ui.checkbox(&mut colorize, "Semantic file colors").changed() {
                    self.state.config.colorize_file_types = colorize;
                }
                if self.state.config.colorize_file_types {
                    let mut folders = self.state.config.colorize_folder_labels;
                    if ui.checkbox(&mut folders, "Include folder labels").changed() {
                        self.state.config.colorize_folder_labels = folders;
                    }
                }
            });
        }

        ui.add_space(10.0);
        Frame::new()
            .fill(c.panel)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(7.0)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                let preview = self.preview_colors_for_scope(
                    self.state.config.theme_preset,
                    self.state.config.theme,
                );
                if compact {
                    theme_preview_strip(ui, preview, 116.0, 34.0, c);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        let active_label = if let Some(name) = &self.state.config.active_custom_theme {
                            format!("Active theme: {} ({})", name, Self::theme_mode_label(self.state.config.theme))
                        } else {
                            format!(
                                "Active theme: {} ({})",
                                Self::theme_preset_label(self.state.config.theme_preset),
                                Self::theme_mode_label(self.state.config.theme)
                            )
                        };
                        ui.label(
                            RichText::new(active_label)
                            .color(c.text)
                            .size(12.0)
                            .strong(),
                        );
                        ui.label(
                            RichText::new(if self.state.config.colorize_file_types {
                                "Semantic file colors are on. Theme edits affect both surfaces and file styling."
                            } else {
                                "Semantic file colors are off. Folder/file text uses the neutral text roles directly."
                            })
                            .color(c.text_muted)
                            .size(10.8),
                        );
                    });
                } else {
                    ui.horizontal(|ui| {
                        theme_preview_strip(ui, preview, 116.0, 34.0, c);
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            let active_label = if let Some(name) = &self.state.config.active_custom_theme {
                                format!("Active theme: {} ({})", name, Self::theme_mode_label(self.state.config.theme))
                            } else {
                                format!(
                                    "Active theme: {} ({})",
                                    Self::theme_preset_label(self.state.config.theme_preset),
                                    Self::theme_mode_label(self.state.config.theme)
                                )
                            };
                            ui.label(
                                RichText::new(active_label)
                                .color(c.text)
                                .size(12.0)
                                .strong(),
                            );
                            ui.label(
                                RichText::new(if self.state.config.colorize_file_types {
                                    "Semantic file colors are on. Theme edits affect both surfaces and file styling."
                                } else {
                                    "Semantic file colors are off. Folder/file text uses the neutral text roles directly."
                                })
                                .color(c.text_muted)
                                .size(10.8),
                            );
                        });
                    });
                }
            });

        ui.add_space(10.0);
        ui.label(
            RichText::new("Theme presets")
                .color(c.text_dim)
                .size(11.0)
                .strong(),
        );
        ui.add_space(6.0);

        let presets = [
            ThemePreset::Ottrin,
            ThemePreset::Breeze,
            ThemePreset::Adwaita,
            ThemePreset::Windows11,
            ThemePreset::Solarized,
            ThemePreset::Nord,
            ThemePreset::G33k,
        ];
        let cols = 2;
        let gap = 8.0;
        let card_w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).max(180.0);
        let card_h = 180.0;
        let thumb_h = 130.0;
        let mut edit_preset: Option<ThemePreset> = None;
        for row in presets.chunks(cols) {
            ui.horizontal(|ui| {
                for preset in row {
                    let preview = self.preview_colors_for_scope(*preset, self.state.config.theme);
                    let selected = self.state.config.theme_preset == *preset
                        && self.state.config.active_custom_theme.is_none();
                    let response = theme_preset_card_with_size(
                        ui,
                        c,
                        preview,
                        Self::theme_preset_icon(*preset),
                        Self::theme_preset_label(*preset),
                        selected,
                        ThemePresetCardSize {
                            width: card_w,
                            card_h,
                            thumb_h,
                        },
                    );
                    // Paint edit icon in the label area (bottom-right of card).
                    let inner = response.rect.shrink2(Vec2::new(10.0, 10.0));
                    let label_y = inner.top() + thumb_h + 28.0;
                    let icon_size = 22.0;
                    let icon_rect = Rect::from_min_size(
                        egui::pos2(inner.right() - icon_size, label_y - icon_size * 0.5),
                        Vec2::splat(icon_size),
                    );
                    let icon_hovered = ui.rect_contains_pointer(icon_rect);
                    let p = ui.painter();
                    p.text(
                        icon_rect.center(),
                        Align2::CENTER_CENTER,
                        MI_EDIT,
                        FontId::proportional(14.0),
                        if icon_hovered { c.text } else { c.text_muted },
                    );
                    if response.clicked() {
                        if icon_hovered {
                            edit_preset = Some(*preset);
                        } else if !selected {
                            self.apply_theme_preset(*preset, ctx);
                        }
                    }
                }
            });
            ui.add_space(6.0);
        }
        if let Some(preset) = edit_preset {
            self.open_theme_editor_for_preset(preset, ctx);
        }

        ui.add_space(10.0);
        Frame::new()
            .fill(c.panel)
            .stroke(Stroke::new(1.0, c.border))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(10, 10))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Saved themes")
                        .color(c.text)
                        .size(12.0)
                        .strong(),
                );
                ui.add_space(6.0);
                if self.state.config.custom_themes.is_empty() {
                    ui.label(
                        RichText::new("No saved custom themes yet.")
                            .color(c.text_muted)
                            .size(10.5),
                    );
                } else {
                    let mut remove_idx: Option<usize> = None;
                    let mut apply_theme: Option<SavedTheme> = None;
                    let mut edit_idx: Option<usize> = None;
                    let active_name = self.state.config.active_custom_theme.clone();
                    for (idx, saved) in self.state.config.custom_themes.iter().enumerate() {
                        let is_active = active_name.as_deref() == Some(saved.name.as_str());
                        let card_fill = if is_active {
                            c.panel_raised.gamma_multiply(1.08)
                        } else {
                            c.panel_raised
                        };
                        let card_stroke = if is_active {
                            Stroke::new(1.5, c.accent)
                        } else {
                            Stroke::new(1.0, c.border)
                        };
                        let resp = Frame::new()
                            .fill(card_fill)
                            .stroke(card_stroke)
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    theme_preview_strip(
                                        ui,
                                        self.preview_colors_for_scope(
                                            saved.base_preset,
                                            saved.theme_mode,
                                        ),
                                        72.0,
                                        24.0,
                                        c,
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{}  {}",
                                                Self::theme_preset_icon(saved.base_preset),
                                                saved.name,
                                            ))
                                            .color(c.text)
                                            .size(11.0)
                                            .strong(),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{} · {}",
                                                Self::theme_preset_label(saved.base_preset),
                                                Self::theme_mode_label(saved.theme_mode)
                                            ))
                                            .color(c.text_muted)
                                            .size(10.0),
                                        );
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(MI_CLEAR)
                                                        .size(14.0)
                                                        .color(c.text_muted),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Delete theme")
                                            .clicked()
                                        {
                                            remove_idx = Some(idx);
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(MI_EDIT)
                                                        .size(14.0)
                                                        .color(c.text_muted),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("Edit theme")
                                            .clicked()
                                        {
                                            edit_idx = Some(idx);
                                        }
                                    });
                                });
                            });
                        // Click the card row to apply.
                        if resp.response.interact(Sense::click()).clicked() && !is_active {
                            apply_theme = Some(saved.clone());
                        }
                        if resp.response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.add_space(2.0);
                    }
                    if let Some(idx) = remove_idx {
                        let removed_name = self.state.config.custom_themes[idx].name.clone();
                        self.state.config.custom_themes.remove(idx);
                        if self.state.config.active_custom_theme.as_deref() == Some(&removed_name) {
                            self.state.config.active_custom_theme = None;
                        }
                    }
                    if let Some(saved) = apply_theme {
                        self.apply_saved_custom_theme(&saved, ctx);
                    }
                    if let Some(idx) = edit_idx {
                        self.open_theme_editor_for_saved(idx, ctx);
                    }
                }
            });
    }

    fn render_theme_editor_popup(&mut self, ctx: &Context) {
        if !self.theme_editor.open {
            return;
        }

        let mut close_requested = false;
        let builder = egui::ViewportBuilder::default()
            .with_title("Ottrin Theme Editor")
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([1040.0, 680.0])
            .with_decorations(false)
            .with_resizable(true);
        let vp_id = Self::theme_editor_viewport_id();

        ctx.show_viewport_immediate(vp_id, builder, |theme_ctx, class| {
            self.apply_theme_to_ctx(theme_ctx);
            if theme_ctx.input(|i| i.viewport().close_requested()) {
                close_requested = true;
                return;
            }
            let _ = class;
            if self.render_theme_editor_embedded(theme_ctx) {
                close_requested = true;
                return;
            }
            self.handle_viewport_resize(theme_ctx);
        });

        if close_requested {
            self.cancel_theme_editor(ctx);
            ctx.send_viewport_cmd_to(
                Self::theme_editor_viewport_id(),
                egui::ViewportCommand::Close,
            );
        } else if !self.theme_editor.open {
            self.theme_editor = ThemeEditorState::default();
            ctx.send_viewport_cmd_to(
                Self::theme_editor_viewport_id(),
                egui::ViewportCommand::Close,
            );
        }
    }

    fn theme_editor_viewport_id() -> egui::ViewportId {
        egui::ViewportId::from_hash_of("ottrin-theme-editor")
    }

    fn render_theme_editor_embedded(&mut self, ctx: &Context) -> bool {
        let c = self.colors;
        let mut close_requested = false;
        let popup_title = format!(
            "Theme - {}",
            Self::theme_preset_label(self.theme_editor.preset)
        );
        let active_tab_bg = mix_color(c.toolbar_bg, c.titlebar_bg, 0.28);
        let viewport_focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let (window_fill, window_title, shadow) = viewport_window_fill(c, viewport_focused);

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(window_fill)
                    .stroke(Stroke::new(c.window_border_width, c.window_border))
                    .shadow(shadow)
                    .inner_margin(egui::Margin::same(c.window_border_width.ceil() as i8)),
            )
            .show(ctx, |ui| {
                render_custom_titlebar(
                    ui,
                    ctx,
                    c,
                    window_title,
                    &format!("{} {}", MI_EDIT, popup_title),
                    true,
                    &mut close_requested,
                    |ui| {
                        let tab_btn = |ui: &mut egui::Ui, selected: bool, label: &str, c: Colors, active_tab_bg: Color32| {
                            let fill = if selected { active_tab_bg } else { Color32::TRANSPARENT };
                            let r = ui.add_sized(
                                [132.0, 24.0],
                                egui::Button::new(
                                    RichText::new(label)
                                        .color(if selected { c.text } else { c.text_dim })
                                        .size(11.2),
                                )
                                .fill(fill)
                                .stroke(Stroke::NONE)
                                .corner_radius(egui::CornerRadius {
                                    nw: 5,
                                    ne: 5,
                                    sw: 0,
                                    se: 0,
                                }),
                            );
                            if selected {
                                let y = r.rect.max.y - 1.0;
                                ui.painter().line_segment(
                                    [egui::Pos2::new(r.rect.min.x + 6.0, y), egui::Pos2::new(r.rect.max.x - 6.0, y)],
                                    Stroke::new(2.0, c.accent),
                                );
                            }
                            r
                        };
                        let main_selected = self.theme_editor.tab == ThemeEditorTab::MainInterface;
                        if tab_btn(ui, main_selected, "Main interface", c, active_tab_bg).clicked() {
                            self.theme_editor.tab = ThemeEditorTab::MainInterface;
                        }
                        let text_selected = self.theme_editor.tab == ThemeEditorTab::TextAndIcons;
                        if tab_btn(ui, text_selected, "Text and icons", c, active_tab_bg).clicked() {
                            self.theme_editor.tab = ThemeEditorTab::TextAndIcons;
                        }
                    },
                );

                ui.add_space(10.0);
                let body_rect = ui.available_rect_before_wrap();
                let footer_h = 44.0;
                let footer_rect = Rect::from_min_max(
                    egui::pos2(body_rect.min.x, body_rect.max.y - footer_h),
                    body_rect.max,
                );
                let content_rect = Rect::from_min_max(
                    body_rect.min,
                    egui::pos2(body_rect.max.x - 8.0, footer_rect.min.y - 8.0),
                );

                ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                        let section_fill = mix_color(c.panel_raised, window_fill, 0.26);
                        let card_fill = mix_color(c.panel, c.panel_raised, 0.34);
                        let card_stroke = Stroke::new(1.0, mix_color(c.border, window_fill, 0.42));
                        let section_radius = self.theme_editor.custom.border_radius.clamp(0.0, 14.0) as u8;
                        let section_card = |ui: &mut egui::Ui, title: &str, detail: &str, add_body: &mut dyn FnMut(&mut egui::Ui)| {
                            Frame::new()
                                .fill(section_fill)
                                .stroke(card_stroke)
                                .corner_radius(section_radius)
                                .inner_margin(egui::Margin::symmetric(12, 10))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(title).color(c.text).size(12.0).strong());
                                    if !detail.is_empty() {
                                        ui.label(RichText::new(detail).color(c.text_muted).size(10.0));
                                        ui.add_space(6.0);
                                    } else {
                                        ui.add_space(4.0);
                                    }
                                    add_body(ui);
                                });
                        };

                        let section_card_fixed = |ui: &mut egui::Ui,
                                                  height: f32,
                                                  title: &str,
                                                  detail: &str,
                                                  add_body: &mut dyn FnMut(&mut egui::Ui)| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
                            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                                Frame::new()
                                    .fill(section_fill)
                                    .stroke(card_stroke)
                                    .corner_radius(section_radius)
                                    .inner_margin(egui::Margin::symmetric(12, 10))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(title).color(c.text).size(12.0).strong());
                                        if !detail.is_empty() {
                                            ui.label(RichText::new(detail).color(c.text_muted).size(10.0));
                                            ui.add_space(6.0);
                                        } else {
                                            ui.add_space(4.0);
                                        }
                                        add_body(ui);
                                        let remaining = ui.available_height();
                                        if remaining > 0.0 {
                                            ui.add_space(remaining);
                                        }
                                    });
                            });
                        };

                    let quick_slider = |ui: &mut egui::Ui,
                                        value: &mut f32,
                                        range: std::ops::RangeInclusive<f32>,
                                        title: &str,
                                        detail: &str,
                                        tint: Color32|
                     -> bool {
                        let mut changed = false;
                        let label = if detail.is_empty() {
                            title.to_string()
                        } else {
                            format!("{title} - {detail}")
                        };
                        ui.label(RichText::new(label).color(c.text).size(11.1).strong());
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            let value_w = 64.0;
                            let bar_h = 24.0;
                            let bar_w = (ui.available_width() - value_w - 12.0).max(120.0);

                            // Paint the gradient track behind the slider.
                            let (track_rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h), Sense::hover());
                            paint_theme_tuning_track(ui, track_rect, title, tint, c);

                            // Overlay a custom drag interaction on the track for full-width dragging.
                            let drag_id = ui.id().with(title).with("track_drag");
                            let drag_resp = ui.interact(track_rect, drag_id, Sense::click_and_drag());
                            if (drag_resp.clicked() || drag_resp.dragged())
                                && let Some(pos) = drag_resp.interact_pointer_pos()
                            {
                                let t = ((pos.x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
                                let new_val = egui::lerp(*range.start()..=*range.end(), t);
                                if (*value - new_val).abs() > f32::EPSILON {
                                    *value = new_val;
                                    changed = true;
                                }
                            }

                            // Draw the handle circle at the current value position.
                            let t = ((*value - *range.start()) / (*range.end() - *range.start())).clamp(0.0, 1.0);
                            let handle_x = egui::lerp(track_rect.left() + 6.0..=track_rect.right() - 6.0, t);
                            let handle_center = egui::pos2(handle_x, track_rect.center().y);
                            let handle_r = 7.0;
                            let handle_color = if drag_resp.dragged() || drag_resp.hovered() {
                                Color32::WHITE
                            } else {
                                Color32::from_rgba_premultiplied(230, 230, 230, 220)
                            };
                            ui.painter().circle(handle_center, handle_r, handle_color, Stroke::new(1.5, Color32::from_gray(60)));

                            // Value badge to the right of the slider.
                            ui.add_space(8.0);
                            let val = format!("{:.2}", *value);
                            let value_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(value_w, bar_h));
                            ui.painter().rect_filled(
                                value_rect,
                                bar_h * 0.5,
                                mix_color(c.panel, c.panel_raised, 0.65),
                            );
                            ui.painter().rect_stroke(
                                value_rect,
                                bar_h * 0.5,
                                Stroke::new(1.0, mix_color(c.border, c.panel, 0.35)),
                                egui::StrokeKind::Middle,
                            );
                            ui.painter().text(
                                value_rect.center(),
                                Align2::CENTER_CENTER,
                                val,
                                FontId::proportional(11.0 * c.font_scale),
                                c.text,
                            );
                            ui.advance_cursor_after_rect(value_rect);
                        });
                        changed
                    };

                    let radius_row = |ui: &mut egui::Ui,
                                      value: &mut f32,
                                      range: std::ops::RangeInclusive<f32>,
                                      title: &str,
                                      detail: &str|
                     -> bool {
                        let mut changed = false;
                        Frame::new()
                            .fill(card_fill)
                            .stroke(Stroke::new(1.0, mix_color(c.border, card_fill, 0.38)))
                            .corner_radius(section_radius)
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(title).color(c.text).size(11.0).strong());
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        changed |= ui.add_sized(
                                            [54.0, 22.0],
                                            egui::DragValue::new(value).speed(0.25).range(*range.start()..=*range.end()),
                                        ).changed();
                                    });
                                });
                                ui.label(RichText::new(detail).color(c.text_dim).size(9.6));
                                changed |= ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        egui::Slider::new(value, range).show_value(false),
                                    )
                                    .changed();
                            });
                        changed
                    };

                    let font_scale_row = |ui: &mut egui::Ui,
                                          value: &mut f32,
                                          range: std::ops::RangeInclusive<f32>|
                     -> bool {
                        let mut changed = false;
                        Frame::new()
                            .fill(card_fill)
                            .stroke(Stroke::new(1.0, mix_color(c.border, card_fill, 0.38)))
                            .corner_radius(section_radius)
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("Font scale").color(c.text).size(11.0).strong());
                                        ui.label(RichText::new("Adjust overall text size.").color(c.text_dim).size(9.6));
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let pct = (*value * 100.0).round() as i32;
                                        ui.label(RichText::new(format!("{pct}%")).color(c.text).size(11.0));
                                        let plus = ui.add_sized([22.0, 22.0], egui::Button::new("+"));
                                        if plus.clicked() {
                                            *value = (*value + 0.02).min(*range.end());
                                            changed = true;
                                        }
                                        let minus = ui.add_sized([22.0, 22.0], egui::Button::new("-"));
                                        if minus.clicked() {
                                            *value = (*value - 0.02).max(*range.start());
                                            changed = true;
                                        }
                                    });
                                });
                            });
                        changed
                    };

                    let color_row = |ui: &mut egui::Ui, app: &mut Self, field: ThemeColorField, c: Colors| -> bool {
                        let mut changed = false;
                        let rgba = Self::theme_color_field_value(&app.theme_editor.custom, field);
                        let mut hex_input = app
                            .theme_editor
                            .color_hex_inputs
                            .get(&field)
                            .cloned()
                            .unwrap_or_else(|| format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]));
                        Frame::new()
                            .fill(card_fill)
                            .stroke(Stroke::new(1.0, mix_color(c.border, card_fill, 0.38)))
                            .corner_radius(section_radius)
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(Self::theme_color_field_short_label(field))
                                                .color(c.text)
                                                .size(10.9)
                                                .strong(),
                                        );
                                        ui.label(
                                            RichText::new(Self::theme_color_field_hint(field))
                                                .color(c.text_dim)
                                                .size(9.4),
                                        );
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let resp = ui.add_sized(
                                            [82.0, 24.0],
                                            egui::TextEdit::singleline(&mut hex_input).hint_text("#RRGGBB"),
                                        );
                                        if resp.changed() {
                                            app.theme_editor.color_hex_inputs.insert(field, hex_input.clone());
                                            if let Some(rgb) = parse_hex_rgb(&hex_input) {
                                                let v = Self::theme_color_field_value_mut(&mut app.theme_editor.custom, field);
                                                *v = [rgb[0], rgb[1], rgb[2], 255];
                                                changed = true;
                                            }
                                        }
                                        ui.add_space(6.0);
                                        let swatch = Colors::from_rgba(rgba);
                                        let swatch_btn = ui.add(
                                            egui::Button::new("")
                                                .min_size(Vec2::new(38.0, 24.0))
                                                .fill(swatch)
                                                .stroke(Stroke::new(1.0, mix_color(c.border, swatch, 0.40)))
                                                .corner_radius(6.0),
                                        );
                                        if swatch_btn.clicked() {
                                            app.theme_editor.color_picker_field = Some(field);
                                            app.theme_editor.color_picker_open = true;
                                        }
                                    });
                                });
                            });
                        if changed {
                            app.reanchor_theme_editor_quick_from_current();
                        }
                        changed
                    };

                    let presets = [
                        ThemePreset::Ottrin,
                        ThemePreset::Breeze,
                        ThemePreset::Adwaita,
                        ThemePreset::Windows11,
                        ThemePreset::Solarized,
                        ThemePreset::Nord,
                        ThemePreset::G33k,
                    ];
                    let is_user_theme = self.theme_editor.source_saved_idx.is_some();
                    let mut requested_scope: Option<(ThemePreset, ThemeMode)> = None;
                    let mut cancel_requested = false;
                    let mut save_requested = false;
                    let mut save_as_requested = false;
                    let mut import_requested = false;
                    let mut export_requested = false;
                    let mut reset_requested = false;
                    let mut mode_choice = self.theme_editor.mode;

                    {
                        let mut body = |ui: &mut egui::Ui| {
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Cancel")).clicked() {
                                    cancel_requested = true;
                                }
                                if ui.add_sized([72.0, 26.0], egui::Button::new("Save")).clicked() {
                                    save_requested = true;
                                }
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Import")).clicked() {
                                    import_requested = true;
                                }
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Export")).clicked() {
                                    export_requested = true;
                                }
                                if is_user_theme
                                    && ui.add_sized([84.0, 26.0], egui::Button::new("Save As")).clicked()
                                {
                                    save_as_requested = true;
                                }
                                if !is_user_theme
                                    && ui
                                        .add_sized([136.0, 26.0], egui::Button::new("Reset to default"))
                                        .clicked()
                                {
                                    reset_requested = true;
                                }
                                ui.add_sized(
                                    [240.0, 26.0],
                                    egui::TextEdit::singleline(&mut self.theme_editor.save_as_name)
                                        .hint_text("Theme name"),
                                );
                            });
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Mode").color(c.text_dim).size(10.6).strong());
                                let dark = ui.add_enabled(
                                    !is_user_theme,
                                    egui::Button::new("Dark").selected(mode_choice == ThemeMode::Dark),
                                );
                                if dark.clicked() {
                                    mode_choice = ThemeMode::Dark;
                                }
                                let light = ui.add_enabled(
                                    !is_user_theme,
                                    egui::Button::new("Light").selected(mode_choice == ThemeMode::Light),
                                );
                                if light.clicked() {
                                    mode_choice = ThemeMode::Light;
                                }
                                let system = ui.add_enabled(
                                    !is_user_theme,
                                    egui::Button::new("System").selected(mode_choice == ThemeMode::System),
                                );
                                if system.clicked() {
                                    mode_choice = ThemeMode::System;
                                }
                            });
                            ui.add_space(6.0);
                            ui.label(RichText::new("Preset").color(c.text_dim).size(10.6).strong());
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                for preset in presets {
                                    let selected = self.theme_editor.preset == preset;
                                    let button = egui::Button::new(format!(
                                        "{} {}",
                                        Self::theme_preset_icon(preset),
                                        Self::theme_preset_label(preset)
                                    ))
                                    .fill(if selected { mix_color(c.accent, c.panel, 0.18) } else { card_fill })
                                    .stroke(Stroke::new(
                                        1.0,
                                        if selected { c.accent } else { mix_color(c.border, card_fill, 0.36) },
                                    ))
                                    .corner_radius(section_radius);
                                    let response = ui.add_enabled(!is_user_theme, button);
                                    if response.clicked() {
                                        requested_scope = Some((preset, mode_choice));
                                    }
                                }
                            });
                            if is_user_theme {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("Saved themes keep their preset and mode. Use Save As if you want to branch into another base scope.")
                                        .color(c.text_muted)
                                        .size(9.8),
                                );
                            }
                        };
                        section_card(
                            ui,
                            "Theme scope",
                            "Keep scope and save controls in view while you tune the live preview.",
                            &mut body,
                        );
                    }

                    if cancel_requested {
                        self.cancel_theme_editor(ctx);
                    } else {
                        if import_requested {
                            self.import_theme_editor_json(ctx);
                            mode_choice = self.theme_editor.mode;
                            requested_scope = None;
                        }
                        if export_requested {
                            self.export_theme_editor_json();
                        }
                        if save_requested {
                            self.save_theme_editor(ctx);
                        }
                        if save_as_requested {
                            self.save_theme_editor_as();
                        }
                        if reset_requested {
                            self.reset_theme_editor_to_default(ctx);
                        }
                        if !is_user_theme && mode_choice != self.theme_editor.mode {
                            requested_scope = Some((self.theme_editor.preset, mode_choice));
                        }
                        if let Some((preset, mode)) = requested_scope {
                            self.switch_theme_editor_scope(preset, mode, ctx);
                        }
                    }

                    if self.theme_editor.open {
                        match self.theme_editor.tab {
                        ThemeEditorTab::MainInterface => {
                            let preview = self.theme_editor_preview_colors();
                            {
                                let mut body = |ui: &mut egui::Ui| {
                                    let mut quick_changed = false;
                                    quick_changed |= quick_slider(
                                        ui,
                                        &mut self.theme_editor.main_hue_shift,
                                        -40.0..=40.0,
                                        "Hue",
                                        "Shift the UI color balance.",
                                        Colors::from_rgba(self.theme_editor.custom.accent),
                                    );
                                    ui.add_space(6.0);
                                    quick_changed |= quick_slider(
                                        ui,
                                        &mut self.theme_editor.main_saturation,
                                        0.65..=1.45,
                                        "Saturation",
                                        "Make chrome calmer or richer.",
                                        Colors::from_rgba(self.theme_editor.custom.folder),
                                    );
                                    ui.add_space(6.0);
                                    quick_changed |= quick_slider(
                                        ui,
                                        &mut self.theme_editor.main_brightness,
                                        0.75..=1.25,
                                        "Brightness",
                                        "Lift or darken main surfaces.",
                                        Colors::from_rgba(self.theme_editor.custom.text_heading),
                                    );
                                    ui.add_space(6.0);
                                    quick_changed |= quick_slider(
                                        ui,
                                        &mut self.theme_editor.main_contrast,
                                        0.80..=1.30,
                                        "Contrast",
                                        "Adjust separation between surfaces and borders.",
                                        Colors::from_rgba(self.theme_editor.custom.border),
                                    );
                                    if quick_changed {
                                        self.apply_theme_editor_main_quick(ctx);
                                    }
                                };
                                section_card(
                                    ui,
                                    "Quick tuning",
                                    "Broad controls stay directly above the preview so you can tune the whole interface before editing single regions.",
                                    &mut body,
                                );
                            }

                            ui.add_space(4.0);
                            {
                                let mut clicked_region = None;
                                let mut body = |ui: &mut egui::Ui| {
                                    let (_, clicked) =
                                        theme_editor_large_preview(ui, preview, self.theme_editor.selected_region);
                                    clicked_region = clicked;
                                };
                                section_card(
                                    ui,
                                    "Live preview",
                                    "Hover the preview to inspect regions. Click a region to focus its color controls below.",
                                    &mut body,
                                );
                                if let Some(region) = clicked_region {
                                    self.theme_editor.selected_region = region;
                                }
                            }

                            ui.add_space(4.0);
                            let mut changed = false;
                            let selected_region = self.theme_editor.selected_region;
                            let selected_region_label = Self::theme_preview_region_label(selected_region);
                            let selected_region_hint = Self::theme_preview_region_hint(selected_region);
                            let region_fields = Self::theme_preview_region_fields(selected_region).to_vec();
                            let preview_regions = [
                                ThemePreviewRegion::Titlebar,
                                ThemePreviewRegion::Navigation,
                                ThemePreviewRegion::Bookmarks,
                                ThemePreviewRegion::ColumnBackground,
                                ThemePreviewRegion::SelectedRow,
                                ThemePreviewRegion::ColumnText,
                                ThemePreviewRegion::Sidebar,
                                ThemePreviewRegion::StatusBar,
                                ThemePreviewRegion::Accent,
                                ThemePreviewRegion::Borders,
                            ];
                            ui.columns(2, |cols| {
                                cols[0].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            for region in preview_regions {
                                                let selected = self.theme_editor.selected_region == region;
                                                let chip = egui::Button::new(Self::theme_preview_region_label(region))
                                                    .fill(if selected {
                                                        mix_color(c.accent, c.panel, 0.18)
                                                    } else {
                                                        card_fill
                                                    })
                                                    .stroke(Stroke::new(
                                                        1.0,
                                                        if selected {
                                                            c.accent
                                                        } else {
                                                            mix_color(c.border, card_fill, 0.32)
                                                        },
                                                    ))
                                                    .corner_radius(section_radius);
                                                if ui.add(chip).clicked() {
                                                    self.theme_editor.selected_region = region;
                                                }
                                            }
                                        });
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(selected_region_hint)
                                                .color(c.text_muted)
                                                .size(9.8),
                                        );
                                        ui.add_space(8.0);
                                        for (idx, field) in region_fields.iter().enumerate() {
                                            changed |= color_row(ui, self, *field, c);
                                            if idx + 1 < region_fields.len() {
                                                ui.add_space(6.0);
                                            }
                                        }
                                    };
                                    section_card(
                                        ui,
                                        selected_region_label,
                                        "Only the selected preview region's theme fields are shown here.",
                                        &mut body,
                                    );
                                });
                                cols[1].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        changed |= radius_row(
                                            ui,
                                            &mut self.theme_editor.custom.app_radius,
                                            0.0..=24.0,
                                            "Window radius",
                                            "Outer corners for windows and major surfaces.",
                                        );
                                        ui.add_space(6.0);
                                        changed |= radius_row(
                                            ui,
                                            &mut self.theme_editor.custom.border_radius,
                                            0.0..=14.0,
                                            "Surface radius",
                                            "Cards, lists, and raised containers.",
                                        );
                                        ui.add_space(6.0);
                                        changed |= radius_row(
                                            ui,
                                            &mut self.theme_editor.custom.button_radius,
                                            0.0..=14.0,
                                            "Button radius",
                                            "Interactive controls and filled actions.",
                                        );
                                        ui.add_space(6.0);
                                        changed |= radius_row(
                                            ui,
                                            &mut self.theme_editor.custom.window_border_width,
                                            0.0..=4.0,
                                            "Border width",
                                            "Thickness of the window border outline.",
                                        );
                                    };
                                    section_card(
                                        ui,
                                        "Shape",
                                        "Shape and border controls stay alongside the region-specific palette.",
                                        &mut body,
                                    );
                                });
                            });
                            if changed {
                                self.reanchor_theme_editor_quick_from_current();
                                self.apply_theme_editor_live_preview(ctx);
                            }
                        }
                        ThemeEditorTab::TextAndIcons => {
                            let preview = self.theme_editor_preview_colors();
                            let top_pair_h = 360.0;
                            ui.columns(2, |cols| {
                                cols[0].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        // Text & icons specific preview card
                                        let fs = self.theme_editor.custom.font_scale;
                                        Frame::new()
                                            .fill(preview.bg)
                                            .stroke(Stroke::new(1.0, preview.border))
                                            .corner_radius(preview.border_radius)
                                            .inner_margin(egui::Margin::symmetric(14, 12))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new("Heading text")
                                                        .color(preview.heading)
                                                        .size(14.0 * fs)
                                                        .strong(),
                                                );
                                                ui.add_space(6.0);
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(MI_FOLDER).color(preview.folder).size(14.0));
                                                    ui.label(RichText::new("Folder label").color(preview.folder).size(12.0 * fs));
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(MI_FILE).color(preview.text_dim).size(14.0));
                                                    ui.label(RichText::new("File label").color(preview.text_dim).size(12.0 * fs));
                                                    ui.label(RichText::new("secondary").color(preview.text_muted).size(10.5 * fs));
                                                });
                                                ui.add_space(8.0);
                                                ui.label(RichText::new("Semantic palette").color(preview.text_muted).size(10.0));
                                                ui.add_space(4.0);
                                                let palette = [
                                                    ("Folder", self.theme_editor.custom.palette_folder_blue),
                                                    ("Link", self.theme_editor.custom.palette_link_blue),
                                                    ("Script", self.theme_editor.custom.palette_green),
                                                    ("Archive", self.theme_editor.custom.palette_orange),
                                                    ("Media", self.theme_editor.custom.palette_purple),
                                                    ("Image", self.theme_editor.custom.palette_pink),
                                                    ("Alert", self.theme_editor.custom.palette_red),
                                                    ("Config", self.theme_editor.custom.palette_yellow),
                                                ];
                                                ui.horizontal_wrapped(|ui| {
                                                    for (name, rgba) in &palette {
                                                        let col = Colors::from_rgba(*rgba);
                                                        let _ = ui.add(
                                                            egui::Button::new(
                                                                RichText::new(*name).color(col).size(10.5 * fs),
                                                            )
                                                            .fill(mix_color(preview.bg, col, 0.12))
                                                            .stroke(Stroke::new(1.0, mix_color(preview.border, col, 0.25)))
                                                            .corner_radius(preview.button_radius),
                                                        );
                                                    }
                                                });
                                                ui.add_space(6.0);
                                                let _ = ui.add(
                                                    egui::Button::new(RichText::new("Accent tag").color(preview.text))
                                                        .fill(preview.selected_bg)
                                                        .stroke(Stroke::NONE)
                                                        .corner_radius(preview.button_radius),
                                                );
                                            });
                                    };
                                    section_card_fixed(
                                        ui,
                                        top_pair_h,
                                        "Live preview",
                                        "Check text, icons, and semantic palette balance.",
                                        &mut body,
                                    );
                                });
                                cols[1].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        let mut quick_changed = false;
                                        quick_changed |= quick_slider(
                                            ui,
                                            &mut self.theme_editor.text_hue_shift,
                                            -40.0..=40.0,
                                            "Hue",
                                            "Shift text and icon balance.",
                                            Colors::from_rgba(self.theme_editor.custom.palette_folder_blue),
                                        );
                                        ui.add_space(6.0);
                                        quick_changed |= quick_slider(
                                            ui,
                                            &mut self.theme_editor.text_saturation,
                                            0.65..=1.45,
                                            "Saturation",
                                            "Calm or enrich semantic colors.",
                                            Colors::from_rgba(self.theme_editor.custom.palette_purple),
                                        );
                                        ui.add_space(6.0);
                                        quick_changed |= quick_slider(
                                            ui,
                                            &mut self.theme_editor.text_brightness,
                                            0.75..=1.25,
                                            "Brightness",
                                            "Raise or lower readability.",
                                            Colors::from_rgba(self.theme_editor.custom.palette_soft_white),
                                        );
                                        ui.add_space(6.0);
                                        quick_changed |= quick_slider(
                                            ui,
                                            &mut self.theme_editor.text_contrast,
                                            0.80..=1.30,
                                            "Contrast",
                                            "Increase or soften text separation.",
                                            Colors::from_rgba(self.theme_editor.custom.palette_green),
                                        );
                                        if quick_changed {
                                            self.apply_theme_editor_text_quick(ctx);
                                        }
                                    };
                                    section_card_fixed(
                                        ui,
                                        top_pair_h,
                                        "Quick tuning",
                                        "Broad controls for text, icons, and semantic palette balance.",
                                        &mut body,
                                    );
                                });
                            });

                            ui.add_space(6.0);
                            let mut changed = false;
                            ui.columns(2, |cols| {
                                cols[0].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        changed |= font_scale_row(
                                            ui,
                                            &mut self.theme_editor.custom.font_scale,
                                            0.85..=1.35,
                                        );
                                        ui.add_space(6.0);
                                        changed |= color_row(ui, self, ThemeColorField::TextHeading, c);
                                        ui.add_space(6.0);
                                        changed |= color_row(ui, self, ThemeColorField::TextFolder, c);
                                        ui.add_space(6.0);
                                        changed |= color_row(ui, self, ThemeColorField::TextFile, c);
                                        ui.add_space(6.0);
                                        changed |= color_row(ui, self, ThemeColorField::Folder, c);
                                    };
                                    section_card(
                                        ui,
                                        "Base text and icons",
                                        "Neutral labels and the default folder/icon tint used throughout the app.",
                                        &mut body,
                                    );
                                });
                                cols[1].vertical(|ui| {
                                    let mut body = |ui: &mut egui::Ui| {
                                        ui.label(
                                            RichText::new("These semantic tokens are used when Semantic file colors is enabled.")
                                                .color(c.text_muted)
                                                .size(9.8),
                                        );
                                        ui.add_space(6.0);
                                        for field in [
                                            ThemeColorField::PaletteSoftWhite,
                                            ThemeColorField::PaletteFolderBlue,
                                            ThemeColorField::PaletteLinkBlue,
                                            ThemeColorField::PaletteSteel,
                                            ThemeColorField::PaletteGreen,
                                            ThemeColorField::PaletteOrange,
                                            ThemeColorField::PalettePurple,
                                            ThemeColorField::PalettePink,
                                            ThemeColorField::PaletteRed,
                                            ThemeColorField::PaletteYellow,
                                        ] {
                                            changed |= color_row(ui, self, field, c);
                                            ui.add_space(6.0);
                                        }
                                    };
                                    section_card(
                                        ui,
                                        "Semantic palette",
                                        "Color tokens used for folders, links, scripts, archives, images, warnings, and status hints.",
                                        &mut body,
                                    );
                                });
                            });
                            if changed {
                                self.reanchor_theme_editor_quick_from_current();
                                self.apply_theme_editor_live_preview(ctx);
                            }
                        }
                        }
                    }
                    });
                });
                ui.scope_builder(UiBuilder::new().max_rect(footer_rect), |ui| {
                    Frame::new()
                        .fill(window_title)
                        .stroke(Stroke::new(1.0, mix_color(c.border, window_fill, 0.45)))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let is_user_theme = self.theme_editor.source_saved_idx.is_some();
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Cancel")).clicked() {
                                    self.cancel_theme_editor(ctx);
                                }
                                if is_user_theme {
                                    // User-created theme: "Save" overwrites in place.
                                    if ui.add_sized([72.0, 26.0], egui::Button::new("Save")).clicked() {
                                        self.save_theme_editor(ctx);
                                    }
                                }
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Import")).clicked() {
                                    self.import_theme_editor_json(ctx);
                                }
                                if ui.add_sized([84.0, 26.0], egui::Button::new("Export")).clicked() {
                                    self.export_theme_editor_json();
                                }
                                ui.add_space(4.0);
                                ui.add_sized(
                                    [220.0, 26.0],
                                    egui::TextEdit::singleline(&mut self.theme_editor.save_as_name)
                                        .hint_text("Theme name"),
                                );
                                if is_user_theme {
                                    // "Save As" creates a new copy.
                                    if ui.add_sized([84.0, 26.0], egui::Button::new("Save As")).clicked() {
                                        self.save_theme_editor_as();
                                    }
                                } else {
                                    // Built-in preset: only "Save" as new custom theme.
                                    if ui.add_sized([84.0, 26.0], egui::Button::new("Save")).clicked() {
                                        self.save_theme_editor(ctx);
                                    }
                                }
                                if self.theme_editor.source_saved_idx.is_none()
                                    && ui
                                        .add_sized([136.0, 26.0], egui::Button::new("Reset to default"))
                                        .clicked()
                                {
                                    self.reset_theme_editor_to_default(ctx);
                                }
                            });
                        });
                });
            });

        if self.theme_editor.color_picker_open {
            let mut picker_open = true;
            let mut close_picker_requested = false;
            if let Some(field) = self.theme_editor.color_picker_field {
                egui::Window::new("theme_editor_color_picker_window")
                    .id(egui::Id::new("theme_editor_color_picker"))
                    .order(egui::Order::Tooltip)
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .movable(false)
                    .default_size(Vec2::new(336.0, 392.0))
                    .frame(
                        Frame::window(&ctx.style())
                            .fill(c.panel)
                            .stroke(Stroke::new(1.0, mix_color(c.border, c.panel, 0.55)))
                            .shadow(Shadow {
                                offset: [0, 8],
                                blur: 18,
                                spread: 0,
                                color: Color32::from_black_alpha(72),
                            })
                            .corner_radius(c.app_radius),
                    )
                    .open(&mut picker_open)
                    .show(ctx, |ui| {
                        render_custom_titlebar(
                            ui,
                            ctx,
                            c,
                            c.titlebar_bg,
                            &format!("{} color", Self::theme_color_field_label(field)),
                            false,
                            &mut close_picker_requested,
                            |_ui| {},
                        );
                        ui.add_space(8.0);
                        let mut rgba =
                            Self::theme_color_field_value(&self.theme_editor.custom, field);
                        let mut color = Colors::from_rgba(rgba);
                        if egui::color_picker::color_picker_color32(
                            ui,
                            &mut color,
                            egui::color_picker::Alpha::Opaque,
                        ) {
                            rgba = Colors::to_rgba(color);
                            *Self::theme_color_field_value_mut(
                                &mut self.theme_editor.custom,
                                field,
                            ) = rgba;
                            self.reanchor_theme_editor_quick_from_current();
                            self.apply_theme_editor_live_preview(ctx);
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Hex").color(c.text_dim).size(11.0));
                            let mut hex = self
                                .theme_editor
                                .color_hex_inputs
                                .get(&field)
                                .cloned()
                                .unwrap_or_else(|| {
                                    format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])
                                });
                            if ui
                                .add_sized([110.0, 24.0], egui::TextEdit::singleline(&mut hex))
                                .changed()
                            {
                                self.theme_editor
                                    .color_hex_inputs
                                    .insert(field, hex.clone());
                                if let Some(rgb) = parse_hex_rgb(&hex) {
                                    let rgba = [rgb[0], rgb[1], rgb[2], 255];
                                    *Self::theme_color_field_value_mut(
                                        &mut self.theme_editor.custom,
                                        field,
                                    ) = rgba;
                                    self.reanchor_theme_editor_quick_from_current();
                                    self.apply_theme_editor_live_preview(ctx);
                                }
                            }
                            if ui.button("Copy").clicked() {
                                let hx = format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]);
                                ui.ctx().copy_text(hx.clone());
                                self.theme_editor.color_clipboard_hex = hx;
                            }
                            if ui
                                .add_enabled(
                                    !self.theme_editor.color_clipboard_hex.is_empty(),
                                    egui::Button::new("Paste"),
                                )
                                .clicked()
                            {
                                let pasted = self.theme_editor.color_clipboard_hex.clone();
                                if let Some(rgb) = parse_hex_rgb(&pasted) {
                                    let rgba = [rgb[0], rgb[1], rgb[2], 255];
                                    *Self::theme_color_field_value_mut(
                                        &mut self.theme_editor.custom,
                                        field,
                                    ) = rgba;
                                    self.theme_editor.color_hex_inputs.insert(field, pasted);
                                    self.reanchor_theme_editor_quick_from_current();
                                    self.apply_theme_editor_live_preview(ctx);
                                }
                            }
                        });
                    });
                ctx.move_to_top(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("theme_editor_color_picker"),
                ));
            }
            if close_picker_requested {
                picker_open = false;
            }
            if !picker_open {
                self.theme_editor.color_picker_open = false;
                self.theme_editor.color_picker_field = None;
            }
        }
        close_requested
    }

    fn is_bookmarked(&self, path: &Path) -> bool {
        self.state
            .config
            .bookmarks
            .iter()
            .any(|(_, _, p)| p == path)
    }

    fn add_bookmark_for_path(&mut self, path: PathBuf) {
        if self.is_bookmarked(&path) {
            return;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| path.display().to_string());
        self.state
            .config
            .bookmarks
            .push((MI_FOLDER.to_string(), name, path));
    }

    fn remove_bookmark_for_path(&mut self, path: &Path) {
        self.state.config.bookmarks.retain(|(_, _, p)| p != path);
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
        let _ = self.listing_tx.send(ListingRequest {
            key,
            path: dir,
            request_id: id,
            show_hidden: hidden,
        });
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
            // Focus starts on the actual current directory column.
            ui_state.miller_focus_dir = Some(dir.clone());
            // Seed ancestry path memory so back-path rows are immediately
            // treated as traversed from first render (including startup).
            let mut child = dir.clone();
            let mut cursor = dir.parent().map(Path::to_path_buf);
            while let Some(parent) = cursor {
                ui_state
                    .selection_memory
                    .insert(parent.clone(), child.clone());
                child = parent.clone();
                cursor = parent.parent().map(Path::to_path_buf);
            }
            ui_state.miller_cache.clear();
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
                    let sort_cfg = if matches!(resp.key, ListingKey::TabMain(_)) {
                        self.state
                            .tab_by_id(tab_id)
                            .map(|t| t.sort)
                            .unwrap_or_default()
                    } else {
                        ottrin_core::SortConfig::default()
                    };
                    sort_entries(&mut entries, &sort_cfg);
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
                    ui_state.left_sel = ui_state.left.entries.iter().position(|e| e.path == cur);
                }
            }
            // Main column selection is path-driven:
            // only restore explicit pending path (e.g. navigate up/back),
            // otherwise keep current dir un-entered (no implicit child select).
            if matches!(resp.key, ListingKey::TabMain(_)) {
                let ui_state = self.tab_ui.entry(tab_id).or_default();
                let is_current_main_dir = self
                    .state
                    .tab_by_id(tab_id)
                    .map(|t| t.current_dir == ui_state.main.dir)
                    .unwrap_or(false);
                if !is_current_main_dir {
                    // Live column-anchor navigation can change current_dir
                    // without an async request. Ignore selection side-effects
                    // from now-stale main responses.
                    continue;
                }
                if !ui_state.main.entries.is_empty() {
                    let dir = ui_state.main.dir.clone();
                    let desired = ui_state.pending_select_path.take();
                    let select_first = std::mem::take(&mut ui_state.pending_select_first);
                    let idx = desired.as_ref().and_then(|path| {
                        ui_state.main.entries.iter().position(|e| &e.path == path)
                    });
                    if let Some(i) = idx {
                        ui_state.main_sel = Some(i);
                        let selected_path = ui_state.main.entries[i].path.clone();
                        ui_state.selection_memory.insert(dir.clone(), selected_path);
                        refresh_right_for.push(tab_id);
                    } else if select_first {
                        ui_state.main_sel = Some(0);
                        let selected_path = ui_state.main.entries[0].path.clone();
                        ui_state.selection_memory.insert(dir.clone(), selected_path);
                        refresh_right_for.push(tab_id);
                    } else {
                        // Keep an already valid selection (possibly set during
                        // render-time model build) instead of wiping focus.
                        let keep = ui_state
                            .main_sel
                            .filter(|&i| i < ui_state.main.entries.len());
                        if let Some(i) = keep {
                            ui_state.main_sel = Some(i);
                            let selected_path = ui_state.main.entries[i].path.clone();
                            ui_state.selection_memory.insert(dir.clone(), selected_path);
                            refresh_right_for.push(tab_id);
                        } else {
                            // Fresh directory view (e.g., startup): no implicit child selection.
                            ui_state.main_sel = None;
                        }
                    }
                } else {
                    ui_state.main_sel = None;
                }
            }
            if matches!(resp.key, ListingKey::TabRight(_)) {
                let ui_state = self.tab_ui.entry(tab_id).or_default();
                if ui_state.right.entries.is_empty() {
                    ui_state.right_sel = None;
                } else if ui_state.right_sel.is_none() {
                    ui_state.right_sel = Some(0);
                }
            }
        }

        for tid in refresh_right_for {
            self.load_miller_right(tid);
        }
    }

    fn poll_op_results(&mut self) {
        while let Ok(res) = self.op_rx.try_recv() {
            if let Some(err) = res.error {
                self.command_frame.error = Some(err);
                self.command_frame.message = None;
                self.command_frame.visible = true;
                if let Some(cmd) = res.retry_cmd {
                    self.pending_privileged_retry = Some((cmd, res.tab_id));
                } else {
                    self.pending_privileged_retry = None;
                }
                continue;
            }
            self.pending_privileged_retry = None;
            if let Some(tab_id) = res.tab_id
                && self.state.tab_by_id(tab_id).is_some()
            {
                let mut refresh_tabs = vec![tab_id];
                if let Some(link) = self.state.link_view.as_ref()
                    && link.contains_tab(tab_id)
                {
                    let other_tab_id = if link.left_tab_id == tab_id {
                        link.right_tab_id
                    } else {
                        link.left_tab_id
                    };
                    refresh_tabs.push(other_tab_id);
                }
                refresh_tabs.sort_unstable();
                refresh_tabs.dedup();
                for refresh_tab in refresh_tabs {
                    if let Some(tab) = self.state.tab_by_id(refresh_tab) {
                        let dir = tab.current_dir.clone();
                        self.refresh_tab(refresh_tab, dir);
                    }
                }
            }
        }
    }

    fn poll_hash_results(&mut self) {
        while let Ok((path, result)) = self.hash_rx.try_recv() {
            if self.info_panel_hash_inflight.as_ref() == Some(&path) {
                self.info_panel_hash_inflight = None;
            }
            match result {
                Ok(hash) => {
                    self.info_panel_hashes.insert(path, hash);
                }
                Err(err) => {
                    self.info_panel_hash_errors.insert(path, err);
                }
            }
        }
    }

    fn poll_preview_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(resp) = self.preview_rx.try_recv() {
            self.preview_inflight.remove(&resp.path);
            if let Some(bytes) = resp.bytes {
                if self.image_preview_cache.len() >= MAX_CACHED_IMAGE_PREVIEWS {
                    self.image_preview_cache.clear();
                }
                let key = preview_cache_key(&resp.path, resp.target_dim);
                self.image_preview_cache.insert(key, bytes);
                changed = true;
            }
        }
        changed
    }

    fn remember_main_selection(&mut self, tab_id: u64) {
        let selected = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel
                .and_then(|i| ui.main.entries.get(i))
                .map(|e| (ui.main.dir.clone(), e.path.clone()))
        };
        if let Some((dir, path)) = selected {
            self.tab_ui
                .entry(tab_id)
                .or_default()
                .selection_memory
                .insert(dir, path);
        }
    }

    fn set_main_selection(&mut self, tab_id: u64, idx: usize) {
        let selected = {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.main_sel = Some(idx);
            ui.main
                .entries
                .get(idx)
                .map(|e| (ui.main.dir.clone(), e.path.clone()))
        };
        if let Some((dir, path)) = selected {
            self.tab_ui
                .entry(tab_id)
                .or_default()
                .selection_memory
                .insert(dir, path);
        }
    }

    // Column-view live anchor: update active dir without pushing history.
    // Used when user navigates an ancestor column so child previews rebase.
    fn set_tab_anchor_dir_live(&mut self, tab_id: u64, dir: PathBuf) {
        let same = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir == dir)
            .unwrap_or(false);
        if same {
            return;
        }
        self.remember_main_selection(tab_id);
        if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
            tab.current_dir = dir;
        }
        let ui = self.tab_ui.entry(tab_id).or_default();
        ui.main.dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        ui.main_sel = None;
        ui.scroll_epoch = ui.scroll_epoch.wrapping_add(1);
    }

    fn miller_width_key(dir: &Path) -> String {
        dir.to_string_lossy().to_string()
    }

    fn infer_miller_column_width(entries: &[FileEntry]) -> f32 {
        let max_chars = entries
            .iter()
            .take(160)
            .map(|e| e.name.chars().count())
            .max()
            .unwrap_or(12);
        // Rough glyph width estimate + icon/arrow paddings.
        (max_chars as f32 * 6.6 + 60.0).clamp(120.0, 360.0)
    }

    fn miller_default_width(&self, entries: &[FileEntry]) -> f32 {
        match self.state.config.miller_column_width_mode {
            MillerColumnWidthMode::Fixed => MILLER_FIXED_WIDTH,
            MillerColumnWidthMode::Auto => Self::infer_miller_column_width(entries),
        }
    }

    fn miller_column_width(&mut self, dir: &Path, entries: &[FileEntry]) -> f32 {
        let key = Self::miller_width_key(dir);
        if let Some(width) = self.state.config.folder_column_widths.get(&key) {
            return width.clamp(96.0, 560.0);
        }
        self.miller_default_width(entries)
    }

    fn set_miller_column_width(&mut self, dir: &Path, width: f32) {
        let key = Self::miller_width_key(dir);
        self.state
            .config
            .folder_column_widths
            .insert(key, width.clamp(96.0, 560.0));
    }

    fn clear_all_miller_column_widths(&mut self) {
        self.state.config.folder_column_widths.clear();
    }

    fn has_custom_miller_width(&self, dir: &Path) -> bool {
        let key = Self::miller_width_key(dir);
        self.state.config.folder_column_widths.contains_key(&key)
    }

    fn miller_list_entries(
        &mut self,
        tab_id: u64,
        dir: &Path,
        sort_cfg: ottrin_core::SortConfig,
    ) -> Result<Vec<FileEntry>, String> {
        let hidden = self.state.config.show_hidden_files;
        let cached = self
            .tab_ui
            .entry(tab_id)
            .or_default()
            .miller_cache
            .get(dir)
            .cloned();
        let mut result = if let Some(res) = cached {
            res
        } else {
            list_directory(dir, hidden)
        };
        if let Ok(ref mut entries) = result {
            sort_entries(entries, &sort_cfg);
        }
        self.tab_ui
            .entry(tab_id)
            .or_default()
            .miller_cache
            .insert(dir.to_path_buf(), result.clone());
        result
    }

    fn resolve_miller_selection(
        &mut self,
        tab_id: u64,
        dir: &Path,
        entries: &[FileEntry],
        is_current_dir: bool,
    ) -> Option<usize> {
        if entries.is_empty() {
            return None;
        }
        if is_current_dir {
            let ui = self.tab_ui.entry(tab_id).or_default();
            // Current dir selection priority:
            // 1) one-shot pending path (from left/up navigation)
            // 2) explicit current main selection
            // 3) one-shot select-first hint (from explicit enter action)
            let preferred_path = ui.pending_select_path.take().or_else(|| {
                if ui.main.dir == dir {
                    ui.main_sel
                        .and_then(|i| ui.main.entries.get(i))
                        .map(|e| e.path.clone())
                } else {
                    None
                }
            });
            let mut idx = preferred_path
                .as_ref()
                .and_then(|p| entries.iter().position(|e| &e.path == p));
            if idx.is_none() {
                // If a deep child path was requested while entering an ancestor
                // (e.g. requested "/home/campbell" while current dir is "/"),
                // select the nearest direct child ("/home").
                if let Some(preferred) = preferred_path.as_ref() {
                    let mut cursor = preferred.as_path();
                    while let Some(parent) = cursor.parent() {
                        if parent == dir {
                            idx = entries.iter().position(|e| e.path == cursor);
                            break;
                        }
                        cursor = parent;
                    }
                }
            }
            if let Some(i) = idx {
                let selected_path = entries[i].path.clone();
                ui.selection_memory.insert(dir.to_path_buf(), selected_path);
                ui.main_sel = Some(i);
                Some(i)
            } else if ui.pending_select_first {
                ui.pending_select_first = false;
                ui.main_sel = Some(0);
                let selected_path = entries[0].path.clone();
                ui.selection_memory.insert(dir.to_path_buf(), selected_path);
                Some(0)
            } else {
                // Fresh folder: present contents without entering a child.
                ui.main_sel = None;
                None
            }
        } else {
            // Preview/ancestor columns should not auto-select row 0; only use
            // remembered selection if present.
            let preferred_path = self
                .tab_ui
                .entry(tab_id)
                .or_default()
                .selection_memory
                .get(dir)
                .cloned();
            let idx = preferred_path
                .as_ref()
                .and_then(|p| entries.iter().position(|e| &e.path == p))?;
            Some(idx)
        }
    }

    fn set_miller_selection(
        &mut self,
        tab_id: u64,
        dir: &Path,
        entries: &[FileEntry],
        idx: usize,
        is_current_dir: bool,
    ) {
        if entries.is_empty() {
            return;
        }
        let clamped = idx.min(entries.len() - 1);
        let path = entries[clamped].path.clone();
        let ui = self.tab_ui.entry(tab_id).or_default();
        ui.selection_memory.insert(dir.to_path_buf(), path);
        if is_current_dir {
            ui.main_sel = Some(clamped);
        }
        // NOTE: horizontal scroll to the preview column is NOT set here.
        // It is set explicitly in the ArrowRight/Enter handler so that
        // up/down navigation within a column never causes horizontal jumping.
    }

    fn build_miller_model(
        &mut self,
        tab_id: u64,
        _pane_count: usize,
    ) -> (Vec<MillerColumnModel>, usize) {
        let current_dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let sort_cfg = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.sort)
            .unwrap_or_default();

        // Build root -> ... -> current path chain first.
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut cursor = Some(current_dir.clone());
        while let Some(dir) = cursor {
            dirs.push(dir.clone());
            cursor = dir.parent().map(Path::to_path_buf);
        }
        dirs.reverse();
        let path_chain = dirs.clone();

        // Then keep extending to the right using selected-child preview chain.
        let max_depth = self
            .tab_ui
            .entry(tab_id)
            .or_default()
            .miller_preview_depth
            .max(1);
        let mut depth = 0usize;
        let mut cols = Vec::new();
        let mut idx = 0usize;
        while idx < dirs.len() && cols.len() < 64 {
            let dir = dirs[idx].clone();
            let is_current_dir = dir == current_dir;
            match self.miller_list_entries(tab_id, &dir, sort_cfg) {
                Ok(entries) => {
                    let selected = if idx + 1 < path_chain.len() {
                        let next_dir = &path_chain[idx + 1];
                        let sel = entries.iter().position(|e| &e.path == next_dir);
                        if let Some(si) = sel
                            && let Some(sel_entry) = entries.get(si)
                        {
                            self.tab_ui
                                .entry(tab_id)
                                .or_default()
                                .selection_memory
                                .insert(dir.clone(), sel_entry.path.clone());
                        }
                        sel
                    } else {
                        self.resolve_miller_selection(tab_id, &dir, &entries, is_current_dir)
                    };

                    if is_current_dir {
                        let ui = self.tab_ui.entry(tab_id).or_default();
                        ui.main.dir = current_dir.clone();
                        ui.main.entries = entries.clone();
                        ui.main.error = None;
                        ui.main.loading = false;
                    }

                    // Extend preview chain: from current_dir onward, follow
                    // selected children to show their contents as preview
                    // columns (Finder-style).  Limited by miller_preview_depth
                    // so Up/Down resets to 1 level, Left grows, Right shrinks.
                    if idx + 1 >= path_chain.len()
                        && depth < max_depth
                        && let Some(sel_idx) = selected
                        && let Some(sel_entry) = entries.get(sel_idx)
                        && matches!(sel_entry.kind, EntryKind::Directory | EntryKind::Symlink)
                        && !dirs.contains(&sel_entry.path)
                    {
                        dirs.push(sel_entry.path.clone());
                        depth += 1;
                    }

                    cols.push(MillerColumnModel {
                        dir,
                        entries,
                        error: None,
                        selected,
                    });
                }
                Err(err) => {
                    if is_current_dir {
                        let ui = self.tab_ui.entry(tab_id).or_default();
                        ui.main.dir = current_dir.clone();
                        ui.main.entries.clear();
                        ui.main.error = Some(err.clone());
                        ui.main.loading = false;
                        ui.main_sel = None;
                    }
                    cols.push(MillerColumnModel {
                        dir,
                        entries: Vec::new(),
                        error: Some(err),
                        selected: None,
                    });
                }
            }
            idx += 1;
        }

        let current_col = cols.iter().position(|c| c.dir == current_dir).unwrap_or(0);
        (cols, current_col)
    }

    fn effective_miller_focus_col(
        &mut self,
        tab_id: u64,
        cols: &[MillerColumnModel],
        current_col: usize,
    ) -> usize {
        if cols.is_empty() {
            return 0;
        }
        let current_col = current_col.min(cols.len() - 1);
        let ui = self.tab_ui.entry(tab_id).or_default();
        let mut focus_col = match ui.miller_focus_hint {
            Some(MillerFocusHint::LeftEdge) => 0,
            Some(MillerFocusHint::CurrentDir) => current_col,
            None => ui
                .miller_focus_dir
                .as_ref()
                .and_then(|d| cols.iter().position(|c| &c.dir == d))
                .unwrap_or(current_col),
        };
        // Startup/no-selection behavior: if current dir has no active row yet,
        // treat the path row in the previous column as keyboard focus.
        if focus_col == current_col && cols[current_col].selected.is_none() && current_col > 0 {
            focus_col = current_col - 1;
        }
        focus_col
    }

    // ── Navigation ────────────────────────────────────────────────────────────

    fn navigate_tab_to(&mut self, tab_id: u64, dir: PathBuf) {
        self.focus_tab(tab_id);
        self.remember_main_selection(tab_id);
        if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
            tab.navigate_to(dir.clone());
        }
        self.refresh_tab(tab_id, dir);
    }

    fn focus_tab(&mut self, tab_id: u64) {
        let _ = self.state.select_tab_id_preserving_tandem(tab_id);
    }

    fn focus_tab_idx(&mut self, idx: usize) {
        let _ = self.state.select_tab_idx_preserving_tandem(idx);
    }

    fn address_bar_state_mut(&mut self, tab_id: u64) -> &mut AddressBar {
        self.address_bars.entry(tab_id).or_default()
    }

    fn address_bar_state(&self, tab_id: u64) -> Option<&AddressBar> {
        self.address_bars.get(&tab_id)
    }

    fn any_address_bar_editing(&self) -> bool {
        self.address_bars.values().any(|bar| bar.editing)
    }

    fn tandem_tab_ids(&self) -> Option<(u64, u64)> {
        let link_view = self.state.link_view.as_ref()?;
        if link_view.left_tab_id == link_view.right_tab_id {
            return None;
        }
        let left_exists = self.state.tab_by_id(link_view.left_tab_id).is_some();
        let right_exists = self.state.tab_by_id(link_view.right_tab_id).is_some();
        if left_exists && right_exists {
            Some((link_view.left_tab_id, link_view.right_tab_id))
        } else {
            None
        }
    }

    fn tandem_active_tab_id(&self) -> Option<u64> {
        let link_view = self.state.link_view.as_ref()?;
        match link_view.active_side {
            TandemSide::Left => Some(link_view.left_tab_id),
            TandemSide::Right => Some(link_view.right_tab_id),
        }
    }

    fn tandem_other_tab_id(&self, tab_id: u64) -> Option<u64> {
        let link_view = self.current_tandem_view()?;
        if link_view.left_tab_id == tab_id {
            Some(link_view.right_tab_id)
        } else if link_view.right_tab_id == tab_id {
            Some(link_view.left_tab_id)
        } else {
            None
        }
    }

    fn tandem_transfer_destination(&self, tab_id: u64) -> Option<(u64, PathBuf)> {
        let other_tab_id = self.tandem_other_tab_id(tab_id)?;
        let destination = self.state.tab_by_id(other_tab_id)?.current_dir.clone();
        Some((other_tab_id, destination))
    }

    fn queue_transfer_to_tandem(
        &mut self,
        source_tab_id: u64,
        source_dir: &Path,
        source_path: PathBuf,
        move_files: bool,
    ) -> bool {
        let Some((_, destination)) = self.tandem_transfer_destination(source_tab_id) else {
            return false;
        };
        if destination == source_dir {
            return false;
        }
        let cmd = if move_files {
            FileCommand::Move {
                sources: vec![source_path],
                destination,
                conflict: ConflictAction::Rename,
            }
        } else {
            FileCommand::Copy {
                sources: vec![source_path],
                destination,
                conflict: ConflictAction::Rename,
            }
        };
        self.run_file_op(cmd, Some(source_tab_id));
        self.set_status_message(
            if move_files {
                "Queued move to other pane"
            } else {
                "Queued copy to other pane"
            },
            true,
            3,
        );
        true
    }

    fn set_tandem_active_side(&mut self, side: TandemSide) {
        let tab_id = {
            let Some(link) = self.state.link_view.as_mut() else {
                return;
            };
            link.normalize_legacy();
            let tab_id = link.tab_id_for_side(side);
            link.active_side = side;
            tab_id
        };
        if let Some(idx) = self.state.tab_idx_by_id(tab_id) {
            self.state.active_tab_idx = idx;
        }
    }

    fn toggle_tandem_pin(&mut self, tab_id: u64) {
        let should_focus = {
            let Some(link) = self.state.link_view.as_mut() else {
                return;
            };
            link.normalize_legacy();
            if !link.contains_tab(tab_id) {
                return;
            }
            if link.pinned_tab_id == Some(tab_id) {
                link.set_pinned_tab(None);
            } else {
                link.set_pinned_tab(Some(tab_id));
            }
            link.set_active_tab(tab_id);
            true
        };
        if should_focus {
            self.focus_tab(tab_id);
        }
    }

    fn navigate_tab_back(&mut self, tab_id: u64) {
        self.focus_tab(tab_id);
        let before = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        self.remember_main_selection(tab_id);
        let moved = self
            .state
            .tab_by_id_mut(tab_id)
            .map(|tab| tab.navigate_back())
            .unwrap_or(false);
        if moved {
            let dir = self
                .state
                .tab_by_id(tab_id)
                .map(|tab| tab.current_dir.clone())
                .unwrap_or_else(default_home_dir);
            if before.parent() == Some(dir.as_path()) {
                self.tab_ui.entry(tab_id).or_default().pending_select_path = Some(before);
            }
            self.refresh_tab(tab_id, dir);
        }
    }

    fn navigate_tab_forward(&mut self, tab_id: u64) {
        self.focus_tab(tab_id);
        let before = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        self.remember_main_selection(tab_id);
        let moved = self
            .state
            .tab_by_id_mut(tab_id)
            .map(|tab| tab.navigate_forward())
            .unwrap_or(false);
        if moved {
            let dir = self
                .state
                .tab_by_id(tab_id)
                .map(|tab| tab.current_dir.clone())
                .unwrap_or_else(default_home_dir);
            if before.parent() == Some(dir.as_path()) {
                self.tab_ui.entry(tab_id).or_default().pending_select_path = Some(before);
            }
            self.refresh_tab(tab_id, dir);
        }
    }

    fn navigate_tab_up(&mut self, tab_id: u64) {
        self.focus_tab(tab_id);
        let before = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        self.remember_main_selection(tab_id);
        let moved = self
            .state
            .tab_by_id_mut(tab_id)
            .map(|tab| tab.navigate_up())
            .unwrap_or(false);
        if moved {
            let dir = self
                .state
                .tab_by_id(tab_id)
                .map(|tab| tab.current_dir.clone())
                .unwrap_or_else(default_home_dir);
            self.tab_ui.entry(tab_id).or_default().pending_select_path = Some(before);
            self.refresh_tab(tab_id, dir);
        }
    }

    fn open_directory_in_tandem(&mut self, dir: PathBuf) {
        if !dir.is_dir() {
            return;
        }
        let current_idx = self.state.active_tab_idx;
        let new_idx = self.state.new_tab(dir.clone());
        let new_tab_id = self.state.tabs[new_idx].id;
        let _ = self.state.activate_tandem(current_idx, new_idx);
        self.refresh_tab(new_tab_id, dir);
    }

    fn navigate_active_back(&mut self) {
        let tab_id = self.state.active_tab().id;
        self.navigate_tab_back(tab_id);
    }

    fn navigate_active_forward(&mut self) {
        let tab_id = self.state.active_tab().id;
        self.navigate_tab_forward(tab_id);
    }

    fn navigate_active_up(&mut self) {
        let tab_id = self.state.active_tab().id;
        self.navigate_tab_up(tab_id);
    }

    fn open_entry(&mut self, path: PathBuf, kind: EntryKind) {
        let tab_id = self.state.active_tab().id;
        self.open_entry_in_tab(tab_id, path, kind);
    }

    fn open_entry_in_tab(&mut self, tab_id: u64, path: PathBuf, kind: EntryKind) {
        match kind {
            EntryKind::Directory | EntryKind::Symlink => {
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
        let (len, cur_sel) = {
            let ui_state = self.tab_ui.entry(tab_id).or_default();
            (ui_state.main.entries.len(), ui_state.main_sel)
        };
        if len == 0 {
            return;
        }
        let next = match cur_sel {
            None => 0,
            Some(i) => (i + 1).min(len - 1),
        };
        self.set_main_selection(tab_id, next);
        self.load_miller_right(tab_id);
    }

    fn select_prev_entry(&mut self) {
        let tab_id = self.state.active_tab().id;
        let (len, cur_sel) = {
            let ui_state = self.tab_ui.entry(tab_id).or_default();
            (ui_state.main.entries.len(), ui_state.main_sel)
        };
        if len == 0 {
            return;
        }
        let prev = match cur_sel {
            None => 0,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.set_main_selection(tab_id, prev);
        self.load_miller_right(tab_id);
    }

    fn grid_move(&mut self, delta: isize) {
        let tab_id = self.state.active_tab().id;
        let (len, cur_sel) = {
            let ui_state = self.tab_ui.entry(tab_id).or_default();
            (ui_state.main.entries.len(), ui_state.main_sel)
        };
        if len == 0 {
            return;
        }
        let cur = cur_sel.unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        self.set_main_selection(tab_id, next);
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
        if let Some(e) = entry
            && matches!(e.kind, EntryKind::Directory | EntryKind::Symlink)
        {
            let tab_id = self.state.active_tab().id;
            self.navigate_tab_to(tab_id, e.path);
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
                    let cur_right_dir =
                        { self.tab_ui.entry(tab_id).or_default().right.dir.clone() };
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
        if self.any_address_bar_editing() {
            return;
        }

        if self.sidebar == SidebarSection::Search
            && !self.command_frame.visible
            && self.search_ui.input_focused
        {
            let mut consumed = false;
            ctx.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                    consumed = true;
                    if !self.search_ui.results.is_empty() {
                        let cur = self.search_ui.selected.unwrap_or(0);
                        self.search_ui.selected =
                            Some((cur + 1).min(self.search_ui.results.len() - 1));
                        self.search_ui.scroll_to_selected = true;
                    }
                }
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                    consumed = true;
                    if !self.search_ui.results.is_empty() {
                        let cur = self.search_ui.selected.unwrap_or(0);
                        self.search_ui.selected = Some(cur.saturating_sub(1));
                        self.search_ui.scroll_to_selected = true;
                    }
                }
                if input.consume_key(egui::Modifiers::ALT, Key::Enter) {
                    consumed = true;
                    self.open_search_selected_parent();
                } else if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                    consumed = true;
                    self.open_search_selected();
                }
            });
            if consumed {
                return;
            }
        }

        if self.sidebar == SidebarSection::Search
            && !self.command_frame.visible
            && !self.search_ui.input_focused
        {
            let mut consumed = false;
            ctx.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                    && !self.search_ui.results.is_empty()
                {
                    consumed = true;
                    let cur = self.search_ui.selected.unwrap_or(0);
                    self.search_ui.selected = Some((cur + 1).min(self.search_ui.results.len() - 1));
                    self.search_ui.scroll_to_selected = true;
                }
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
                    && !self.search_ui.results.is_empty()
                {
                    consumed = true;
                    let cur = self.search_ui.selected.unwrap_or(0);
                    self.search_ui.selected = Some(cur.saturating_sub(1));
                    self.search_ui.scroll_to_selected = true;
                }
                if input.consume_key(egui::Modifiers::ALT, Key::Enter) {
                    consumed = true;
                    self.open_search_selected_parent();
                } else if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                    consumed = true;
                    self.open_search_selected();
                }
            });
            if consumed {
                return;
            }
        }

        let tandem_switch = if !self.command_frame.visible {
            ctx.input(|input| {
                let ctrl = input.modifiers.ctrl || input.modifiers.command;
                let switch_left = input.key_pressed(Key::Tab) && input.modifiers.shift;
                let switch_right = input.key_pressed(Key::Tab) && !input.modifiers.shift;
                let ctrl_left = ctrl && input.key_pressed(Key::ArrowLeft);
                let ctrl_right = ctrl && input.key_pressed(Key::ArrowRight);
                if self.current_tandem_view().is_some()
                    && (switch_left || switch_right || ctrl_left || ctrl_right)
                {
                    Some(if switch_left || ctrl_left {
                        TandemSide::Left
                    } else {
                        TandemSide::Right
                    })
                } else {
                    None
                }
            })
        } else {
            None
        };
        if let Some(side) = tandem_switch {
            self.set_tandem_active_side(side);
            return;
        }

        ctx.input(|input| {
            let view = self.state.active_tab().view_mode;
            let is_grid = matches!(view, ViewMode::Grid);
            let is_miller = matches!(view, ViewMode::Miller);
            let tab_id = self.state.active_tab().id;
            let ctrl = input.modifiers.ctrl || input.modifiers.command;

            if ctrl && input.key_pressed(Key::F) {
                self.sidebar = SidebarSection::Search;
                self.search_ui.request_focus = true;
                self.command_frame.visible = false;
                self.command_frame.error = None;
                self.command_frame.message = None;
                self.search_ui.scroll_to_selected = true;
            }

            // ── Navigation ──────────────────────────────────────────────────
            if input.key_pressed(Key::ArrowDown) && !self.command_frame.visible {
                if is_miller {
                    let (cols, current_col) =
                        self.build_miller_model(tab_id, self.miller_col_count);
                    if !cols.is_empty() {
                        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
                        let col = &cols[focus_col];
                        if !col.entries.is_empty() {
                            let next = match col.selected {
                                None => 0,
                                Some(i) => (i + 1).min(col.entries.len() - 1),
                            };
                            self.set_miller_selection(
                                tab_id,
                                &col.dir,
                                &col.entries,
                                next,
                                focus_col == current_col,
                            );
                            let ui_state = self.tab_ui.entry(tab_id).or_default();
                            ui_state.miller_scroll_selected = true;
                            ui_state.miller_preview_depth = 1;
                            if focus_col < current_col {
                                self.set_tab_anchor_dir_live(tab_id, col.dir.clone());
                            }
                        }
                    }
                } else if is_grid {
                    self.grid_move(self.last_grid_cols as isize);
                } else {
                    self.select_next_entry();
                }
            }
            if input.key_pressed(Key::ArrowUp) && !self.command_frame.visible {
                if is_miller {
                    let (cols, current_col) =
                        self.build_miller_model(tab_id, self.miller_col_count);
                    if !cols.is_empty() {
                        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
                        let col = &cols[focus_col];
                        if !col.entries.is_empty() {
                            let prev = match col.selected {
                                None => 0,
                                Some(0) => 0,
                                Some(i) => i - 1,
                            };
                            self.set_miller_selection(
                                tab_id,
                                &col.dir,
                                &col.entries,
                                prev,
                                focus_col == current_col,
                            );
                            let ui_state = self.tab_ui.entry(tab_id).or_default();
                            ui_state.miller_scroll_selected = true;
                            ui_state.miller_preview_depth = 1;
                            if focus_col < current_col {
                                self.set_tab_anchor_dir_live(tab_id, col.dir.clone());
                            }
                        }
                    }
                } else if is_grid {
                    self.grid_move(-(self.last_grid_cols as isize));
                } else {
                    self.select_prev_entry();
                }
            }
            if input.key_pressed(Key::ArrowRight) && !self.command_frame.visible {
                if is_miller {
                    let (cols, current_col) =
                        self.build_miller_model(tab_id, self.miller_col_count);
                    if !cols.is_empty() {
                        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
                        let col = &cols[focus_col];
                        let selected = col.selected.and_then(|i| col.entries.get(i));
                        if focus_col < current_col && focus_col + 1 < cols.len() {
                            let target_col = &cols[focus_col + 1];
                            let new_dir = target_col.dir.clone();
                            {
                                let ui_state = self.tab_ui.entry(tab_id).or_default();
                                ui_state.miller_focus_dir = Some(new_dir.clone());
                                // Restore the remembered child selection so
                                // the target column keeps its highlight.
                                if let Some(sel_idx) = target_col.selected
                                    && let Some(sel_entry) = target_col.entries.get(sel_idx)
                                {
                                    ui_state.pending_select_path = Some(sel_entry.path.clone());
                                    ui_state
                                        .selection_memory
                                        .insert(new_dir.clone(), sel_entry.path.clone());
                                } else {
                                    // No remembered selection — use selection_memory
                                    // or fall back to first item.
                                    let remembered =
                                        ui_state.selection_memory.get(&new_dir).cloned();
                                    if let Some(sel_path) = remembered {
                                        ui_state.pending_select_path = Some(sel_path);
                                    } else {
                                        ui_state.pending_select_first = true;
                                    }
                                }
                            }
                            self.set_tab_anchor_dir_live(tab_id, new_dir);
                        } else if let Some(entry) = selected {
                            if matches!(entry.kind, EntryKind::Directory | EntryKind::Symlink) {
                                let ui_state = self.tab_ui.entry(tab_id).or_default();
                                let depth = ui_state.miller_preview_depth.max(1);
                                if depth > 1 {
                                    // Re-entering a remembered path (after Left):
                                    // advance current_dir one level without clearing
                                    // miller_cache, so preview columns survive.
                                    // Restore the remembered child selection so the
                                    // new current_dir column keeps its highlight.
                                    let remembered =
                                        ui_state.selection_memory.get(&entry.path).cloned();
                                    ui_state.miller_preview_depth = depth - 1;
                                    ui_state.miller_focus_hint = Some(MillerFocusHint::CurrentDir);
                                    if let Some(sel_path) = remembered {
                                        ui_state.pending_select_path = Some(sel_path);
                                    }
                                    self.set_tab_anchor_dir_live(tab_id, entry.path.clone());
                                } else {
                                    // Fresh entry into folder: select first item.
                                    ui_state.miller_focus_hint = Some(MillerFocusHint::CurrentDir);
                                    ui_state.pending_select_first = true;
                                    ui_state.miller_preview_depth = 1;
                                    self.navigate_tab_to(tab_id, entry.path.clone());
                                }
                            } else if focus_col + 1 < cols.len() {
                                self.tab_ui.entry(tab_id).or_default().miller_focus_dir =
                                    Some(cols[focus_col + 1].dir.clone());
                            } else {
                                self.sidebar = SidebarSection::Info;
                            }
                        }
                    }
                } else if is_grid {
                    self.grid_move(1);
                } else {
                    self.enter_selected_folder_miller();
                }
            }
            if input.key_pressed(Key::ArrowLeft) && !self.command_frame.visible {
                if is_miller {
                    let (cols, current_col) =
                        self.build_miller_model(tab_id, self.miller_col_count);
                    if !cols.is_empty() {
                        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
                        if focus_col > 0 {
                            let target_col = &cols[focus_col - 1];
                            let new_dir = target_col.dir.clone();
                            let moved_from_dir = cols[focus_col].dir.clone();
                            {
                                let ui_state = self.tab_ui.entry(tab_id).or_default();
                                ui_state.miller_focus_dir = Some(new_dir.clone());
                                ui_state.miller_preview_depth =
                                    ui_state.miller_preview_depth.max(1) + 1;
                                // Restore selection based on the column path we just left,
                                // not the active tab dir (which can differ when focus is on
                                // an ancestor/path marker column).
                                if moved_from_dir.parent() == Some(new_dir.as_path()) {
                                    ui_state.pending_select_path = Some(moved_from_dir.clone());
                                }
                                if let Some(sel_idx) = target_col.selected
                                    && let Some(sel_entry) = target_col.entries.get(sel_idx)
                                {
                                    ui_state
                                        .selection_memory
                                        .insert(new_dir.clone(), sel_entry.path.clone());
                                }
                            }
                            self.set_tab_anchor_dir_live(tab_id, new_dir);
                        } else {
                            self.tab_ui.entry(tab_id).or_default().miller_focus_hint =
                                Some(MillerFocusHint::LeftEdge);
                            self.navigate_active_up();
                        }
                    }
                } else if is_grid {
                    self.grid_move(-1);
                } else {
                    self.navigate_active_up();
                }
            }
            if input.key_pressed(Key::Enter) && !self.command_frame.visible {
                if is_miller {
                    let (cols, current_col) =
                        self.build_miller_model(tab_id, self.miller_col_count);
                    if !cols.is_empty() {
                        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
                        let col = &cols[focus_col];
                        if let Some(entry) = col.selected.and_then(|i| col.entries.get(i)) {
                            if matches!(entry.kind, EntryKind::Directory | EntryKind::Symlink) {
                                self.tab_ui.entry(tab_id).or_default().miller_focus_hint =
                                    Some(MillerFocusHint::CurrentDir);
                            }
                            self.open_entry(entry.path.clone(), entry.kind);
                        }
                    }
                } else {
                    self.enter_selected();
                }
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
            if ctrl && input.key_pressed(Key::T) {
                let home = default_home_dir();
                let idx = self.state.new_tab(home.clone());
                self.focus_tab_idx(idx);
                self.refresh_tab(self.state.tabs.last().unwrap().id, home);
            }
            if ctrl && input.key_pressed(Key::W) {
                let idx = self.state.active_tab_idx;
                self.state.close_tab(idx);
            }
            if ctrl && input.key_pressed(Key::Tab) {
                let n = self.state.tabs.len();
                let next = (self.state.active_tab_idx + 1) % n;
                self.focus_tab_idx(next);
                self.refresh_active_tab();
            }

            // ── Address bar ───────────────────────────────────────────────────
            if ctrl && input.key_pressed(Key::L) {
                let path = self.displayed_path().display().to_string();
                let bar = self.address_bar_state_mut(tab_id);
                bar.text = path;
                bar.editing = true;
                bar.request_focus = true;
                bar.tab_id = Some(tab_id);
            }

            // ── Preview ───────────────────────────────────────────────────────
            if input.key_pressed(Key::Escape) {
                if self.sidebar == SidebarSection::Search {
                    self.sidebar = SidebarSection::None;
                    self.search_ui.request_focus = false;
                    self.search_ui.input_focused = false;
                    self.tab_ui.entry(tab_id).or_default().miller_focus_hint =
                        Some(MillerFocusHint::CurrentDir);
                }
                self.preview.visible = false;
                self.command_frame.visible = false;
                self.command_frame.error = None;
                for bar in self.address_bars.values_mut() {
                    bar.editing = false;
                    bar.tab_id = None;
                }
                self.target_dropdown.open = false;
            }

            // ── File operations ───────────────────────────────────────────────
            if !self.command_frame.visible
                && self.current_tandem_view().is_some()
                && ctrl
                && input.modifiers.shift
                && input.key_pressed(Key::C)
            {
                if let Some(entry) = self.active_selected_entry() {
                    let tab_id = self.state.active_tab().id;
                    let source_dir = self
                        .state
                        .tab_by_id(tab_id)
                        .map(|tab| tab.current_dir.clone())
                        .unwrap_or_else(default_home_dir);
                    let _ = self.queue_transfer_to_tandem(tab_id, &source_dir, entry.path, false);
                }
            } else if ctrl && input.key_pressed(Key::C) && !self.command_frame.visible {
                self.copy_selection(false);
            }
            if !self.command_frame.visible
                && self.current_tandem_view().is_some()
                && ctrl
                && input.modifiers.shift
                && input.key_pressed(Key::X)
            {
                if let Some(entry) = self.active_selected_entry() {
                    let tab_id = self.state.active_tab().id;
                    let source_dir = self
                        .state
                        .tab_by_id(tab_id)
                        .map(|tab| tab.current_dir.clone())
                        .unwrap_or_else(default_home_dir);
                    let _ = self.queue_transfer_to_tandem(tab_id, &source_dir, entry.path, true);
                }
            } else if ctrl && input.key_pressed(Key::X) && !self.command_frame.visible {
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
            if !self.command_frame.visible
                && !self.any_address_bar_editing()
                && self.sidebar != SidebarSection::Search
                && !self.show_settings
            {
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

    fn dismiss_command_frame_if_inactive(&mut self, ctx: &Context) {
        if !self.command_frame.visible || self.state.config.command_frame_always_visible {
            return;
        }
        // Newly opened this frame; allow the TextEdit to acquire focus first.
        if self.command_frame.request_focus {
            return;
        }
        let focused = ctx.memory(|m| m.has_focus(egui::Id::new("cmd_input_field")));
        if focused {
            return;
        }
        let interacted_elsewhere = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.key_pressed(Key::ArrowDown)
                || i.key_pressed(Key::ArrowUp)
                || i.key_pressed(Key::ArrowLeft)
                || i.key_pressed(Key::ArrowRight)
                || i.key_pressed(Key::Enter)
                || i.key_pressed(Key::Backspace)
                || i.key_pressed(Key::Tab)
                || i.key_pressed(Key::Delete)
                || i.events
                    .iter()
                    .any(|ev| matches!(ev, egui::Event::Text(t) if !t.trim().is_empty()))
        });
        if interacted_elsewhere {
            self.command_frame.visible = false;
            self.command_frame.error = None;
            self.command_frame.message = None;
            self.command_frame.completions.clear();
        }
    }

    fn open_preview(&mut self) {
        if let Some(entry) = self.active_selected_entry() {
            let path = entry.path.clone();
            self.preview.path = Some(path.clone());
            self.preview.visible = true;
            self.preview.data = Some(load_preview(&PreviewRequest {
                path,
                kind_hint: entry.kind,
            }));
        }
    }

    fn ensure_search_started(&mut self) {
        if self.search_started {
            return;
        }
        self.search_service
            .update_config(self.state.config.search.clone());
        self.search_service.start();
        self.search_started = true;
    }

    fn sync_search_scope_root(&mut self) {
        let active_root = Some(self.state.active_tab().current_dir.clone());
        if self.search_ui.last_query_root == active_root {
            return;
        }

        if matches!(self.search_ui.scope, SearchScope::CurrentFolder)
            && self.sidebar == SidebarSection::Search
            && !self.search_ui.query.trim().is_empty()
        {
            self.run_search_query();
            return;
        }

        self.search_ui.last_query_root = active_root;
    }

    fn cached_info_preview(&mut self, entry: &FileEntry) -> PreviewData {
        if let Some((path, preview)) = &self.info_panel_preview
            && path == &entry.path
        {
            return preview.clone();
        }
        let preview = load_preview(&PreviewRequest {
            path: entry.path.clone(),
            kind_hint: entry.kind,
        });
        self.info_panel_preview = Some((entry.path.clone(), preview.clone()));
        preview
    }

    fn middle_ellipsis(ui: &egui::Ui, text: &str, max_width: f32, font: &FontId) -> String {
        let ellipsis = "…";
        if text.is_empty() {
            return String::new();
        }
        let text_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        });
        if text_width <= max_width {
            return text.to_string();
        }
        let ellipsis_width = ui.fonts_mut(|f| {
            f.layout_no_wrap(ellipsis.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        });
        if ellipsis_width >= max_width {
            return ellipsis.to_string();
        }

        let chars: Vec<char> = text.chars().collect();
        if chars.len() <= 2 {
            return ellipsis.to_string();
        }

        let mut best = format!("{}{}{}", chars[0], ellipsis, chars[chars.len() - 1]);
        let mut lo = 2usize;
        let mut hi = chars.len().saturating_sub(1);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let left_keep = mid.div_ceil(2);
            let right_keep = mid / 2;
            if left_keep + right_keep >= chars.len() {
                break;
            }
            let left: String = chars[..left_keep].iter().collect();
            let right: String = chars[chars.len() - right_keep..].iter().collect();
            let candidate = format!("{left}{ellipsis}{right}");
            let width = ui.fonts_mut(|f| {
                f.layout_no_wrap(candidate.clone(), font.clone(), Color32::WHITE)
                    .size()
                    .x
            });
            if width <= max_width {
                best = candidate;
                lo = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                hi = mid - 1;
            }
        }
        best
    }

    fn cached_info_details(&mut self, entry: &FileEntry) -> InfoPanelDetails {
        if let Some((path, details)) = &self.info_panel_details
            && path == &entry.path
        {
            return details.clone();
        }
        let details = build_info_panel_details(&entry.path);
        self.info_panel_details = Some((entry.path.clone(), details.clone()));
        details
    }

    fn active_selected_entry_for(&mut self, tab_id: u64) -> Option<FileEntry> {
        let view_mode = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.view_mode)
            .unwrap_or(ViewMode::Miller);
        if matches!(view_mode, ViewMode::Miller) {
            let (cols, current_col) = self.build_miller_model(tab_id, self.miller_col_count);
            if cols.is_empty() {
                return None;
            }
            let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
            let col = &cols[focus_col];
            return col.selected.and_then(|i| col.entries.get(i)).cloned();
        }
        let ui = self.tab_ui.entry(tab_id).or_default();
        ui.main_sel
            .and_then(|idx| ui.main.entries.get(idx))
            .cloned()
    }

    fn active_selected_entry(&mut self) -> Option<FileEntry> {
        let tab_id = self.state.active_tab().id;
        self.active_selected_entry_for(tab_id)
    }

    fn displayed_path_for(&mut self, tab_id: u64) -> PathBuf {
        let fallback = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let view_mode = self
            .state
            .tab_by_id(tab_id)
            .map(|tab| tab.view_mode)
            .unwrap_or(ViewMode::Miller);
        if !matches!(view_mode, ViewMode::Miller) {
            return fallback;
        }
        match self.active_selected_entry_for(tab_id) {
            Some(entry) if matches!(entry.kind, EntryKind::Directory | EntryKind::Symlink) => {
                entry.path
            }
            _ => fallback,
        }
    }

    fn displayed_path(&mut self) -> PathBuf {
        let tab_id = self.state.active_tab().id;
        self.displayed_path_for(tab_id)
    }

    fn current_tandem_view(&self) -> Option<ottrin_core::LinkView> {
        let mut link = self.state.link_view.clone()?;
        link.normalize_legacy();
        if link.left_tab_id == link.right_tab_id {
            return None;
        }
        if self.state.tab_by_id(link.left_tab_id).is_none()
            || self.state.tab_by_id(link.right_tab_id).is_none()
        {
            return None;
        }
        let active_id = self.state.active_tab().id;
        if !link.contains_tab(active_id) {
            return None;
        }
        Some(link)
    }

    fn retry_list_with_privileges(&mut self, tab_id: u64, dir: PathBuf) -> Result<(), String> {
        self.privileged_status = Some(PrivilegedUiStatus {
            kind: PrivilegedUiKind::InProgress,
            text: "Requesting administrator access…".to_string(),
            since: Instant::now(),
        });
        let req = PrivilegedRequest {
            command: PrivilegedCommand::ListDirectory {
                path: dir.clone(),
                show_hidden: self.state.config.show_hidden_files,
            },
            context: PrivilegedContext {
                reason: Some(format!("Read restricted folder {}", dir.display())),
                cwd: Some(self.state.active_tab().current_dir.clone()),
            },
        };
        let res = self
            .platform
            .execute_privileged(&req)
            .map_err(|e| e.to_string())?;
        match res.status {
            PrivilegedStatus::Success => {
                let entries = match res.payload {
                    Some(PrivilegedPayload::Entries(entries)) => entries,
                    _ => return Err("Privileged helper returned no directory data".to_string()),
                };
                // Feed the data into both the dynamic Miller cache and the async column states.
                self.tab_ui
                    .entry(tab_id)
                    .or_default()
                    .miller_cache
                    .insert(dir.clone(), Ok(entries.clone()));
                let sort_cfg = self
                    .state
                    .tab_by_id(tab_id)
                    .map(|t| t.sort)
                    .unwrap_or_default();
                let mut sorted = entries;
                sort_entries(&mut sorted, &sort_cfg);

                let ui = self.tab_ui.entry(tab_id).or_default();
                if ui.main.dir == dir {
                    ui.main.entries = sorted.clone();
                    ui.main.error = None;
                    ui.main.loading = false;
                    if ui.main_sel.is_none() && !ui.main.entries.is_empty() {
                        ui.main_sel = Some(0);
                    }
                }
                if ui.left.dir == dir {
                    ui.left.entries = sorted.clone();
                    ui.left.error = None;
                    ui.left.loading = false;
                }
                if ui.right.dir == dir {
                    ui.right.entries = sorted;
                    ui.right.error = None;
                    ui.right.loading = false;
                    if ui.right_sel.is_none() && !ui.right.entries.is_empty() {
                        ui.right_sel = Some(0);
                    }
                }
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Success,
                    text: "Loaded restricted folder with administrator privileges.".to_string(),
                    since: Instant::now(),
                });
                Ok(())
            }
            PrivilegedStatus::Denied => {
                let msg = res
                    .message
                    .unwrap_or_else(|| "Permission denied".to_string());
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
            PrivilegedStatus::Unsupported => {
                let msg = res.message.unwrap_or_else(|| {
                    "Integrated privilege management not available on this platform yet".to_string()
                });
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
            PrivilegedStatus::Failed => {
                let msg = res
                    .message
                    .unwrap_or_else(|| "Privileged operation failed".to_string());
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
        }
    }

    fn copy_selection(&mut self, cut: bool) {
        let tab_id = self.state.active_tab().id;
        let ui = self.tab_ui.entry(tab_id).or_default();
        if let Some(idx) = ui.main_sel
            && let Some(entry) = ui.main.entries.get(idx)
        {
            self.clipboard = Some(Clipboard {
                sources: vec![entry.path.clone()],
                cut,
            });
        }
    }

    fn paste_clipboard(&mut self) {
        if let Some(clip) = self.clipboard.clone() {
            let dest = self.state.active_tab().current_dir.clone();
            let tab_id = self.state.active_tab().id;
            let cmd = if clip.cut {
                FileCommand::Move {
                    sources: clip.sources,
                    destination: dest,
                    conflict: ConflictAction::Rename,
                }
            } else {
                FileCommand::Copy {
                    sources: clip.sources,
                    destination: dest,
                    conflict: ConflictAction::Rename,
                }
            };
            self.run_file_op(cmd, Some(tab_id));
            if clip.cut {
                self.clipboard = None;
            }
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
        if targets.is_empty() {
            return;
        }
        let cmd = FileCommand::Delete { targets, mode };
        self.run_file_op(cmd, Some(tab_id));
    }

    fn select_all(&mut self) {
        // For now just select last item (multi-select is Phase 2)
        let tab_id = self.state.active_tab().id;
        let len = self.tab_ui.entry(tab_id).or_default().main.entries.len();
        if len > 0 {
            self.set_main_selection(tab_id, len - 1);
        }
    }

    fn begin_rename(&mut self) {
        let tab_id = self.state.active_tab().id;
        let ui = self.tab_ui.entry(tab_id).or_default();
        if let Some(idx) = ui.main_sel
            && let Some(entry) = ui.main.entries.get(idx)
        {
            self.command_frame.input = format!("mv {} ", entry.name);
            self.command_frame.visible = true;
            self.command_frame.request_focus = true;
        }
    }

    fn run_file_op(&mut self, cmd: FileCommand, refresh_tab: Option<u64>) {
        let platform = Arc::clone(&self.platform);
        let op_tx = self.op_tx.clone();
        let retry_cmd = cmd.clone();
        std::thread::spawn(move || {
            let res = platform.execute_command(&cmd);
            let (error, retry) = match res {
                Ok(_) => (None, None),
                Err(e) => {
                    let msg = e.to_string();
                    if is_permission_denied_msg(&msg) {
                        (Some(msg), Some(retry_cmd))
                    } else {
                        (Some(msg), None)
                    }
                }
            };
            let _ = op_tx.send(OpResult {
                tab_id: refresh_tab,
                error,
                retry_cmd: retry,
            });
        });
    }

    fn retry_file_op_with_privileges(
        &mut self,
        cmd: FileCommand,
        refresh_tab: Option<u64>,
    ) -> Result<(), String> {
        self.privileged_status = Some(PrivilegedUiStatus {
            kind: PrivilegedUiKind::InProgress,
            text: "Requesting administrator access…".to_string(),
            since: Instant::now(),
        });
        let req = PrivilegedRequest {
            command: PrivilegedCommand::File(cmd),
            context: PrivilegedContext {
                reason: Some("Retry denied filesystem operation".to_string()),
                cwd: Some(self.state.active_tab().current_dir.clone()),
            },
        };
        let res = self
            .platform
            .execute_privileged(&req)
            .map_err(|e| e.to_string())?;
        match res.status {
            PrivilegedStatus::Success => {
                if let Some(tab_id) = refresh_tab {
                    if let Some(tab) = self.state.tab_by_id(tab_id) {
                        self.refresh_tab(tab_id, tab.current_dir.clone());
                    }
                } else {
                    self.refresh_active_tab();
                }
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Success,
                    text: "Operation completed with administrator privileges.".to_string(),
                    since: Instant::now(),
                });
                Ok(())
            }
            PrivilegedStatus::Denied => {
                let msg = res
                    .message
                    .unwrap_or_else(|| "Permission denied".to_string());
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
            PrivilegedStatus::Unsupported => {
                let msg = res.message.unwrap_or_else(|| {
                    "Integrated privilege management not available on this platform yet".to_string()
                });
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
            PrivilegedStatus::Failed => {
                let msg = res
                    .message
                    .unwrap_or_else(|| "Privileged operation failed".to_string());
                self.privileged_status = Some(PrivilegedUiStatus {
                    kind: PrivilegedUiKind::Error,
                    text: msg.clone(),
                    since: Instant::now(),
                });
                Err(msg)
            }
        }
    }

    fn run_search_query(&mut self) {
        self.ensure_search_started();
        let previously_selected_path = self
            .search_ui
            .selected
            .and_then(|i| self.search_ui.results.get(i))
            .map(|it| it.path.clone());
        let root = Some(self.state.active_tab().current_dir.clone());
        if self.search_ui.query.trim().is_empty() {
            self.search_ui.results.clear();
            self.search_ui.total = 0;
            self.search_ui.selected = None;
            self.search_ui.last_error = None;
            self.search_ui.last_query_root = root;
            self.search_ui.scroll_to_selected = false;
            return;
        }
        let q = SearchQuery {
            text: self.search_ui.query.clone(),
            scope: self.search_ui.scope,
            root_path: root.clone(),
            include_hidden_system: self.search_ui.include_hidden,
            sort: self.search_ui.sort,
            limit: self.state.config.search.result_limit.max(1),
            offset: 0,
            content_search: self.search_ui.content_search,
        };
        let resp = self.search_service.query(q);
        self.search_ui.last_query_root = root;
        self.search_ui.last_error = resp.error;
        if self.search_ui.last_error.is_none() {
            self.search_ui.results = resp.items;
            self.search_ui.total = resp.total;
            let new_selected = previously_selected_path
                .and_then(|path| self.search_ui.results.iter().position(|it| it.path == path))
                .or(if self.search_ui.results.is_empty() {
                    None
                } else {
                    Some(0)
                });
            if new_selected != self.search_ui.selected {
                self.search_ui.result_preview = None;
            }
            self.search_ui.selected = new_selected;
            self.search_ui.scroll_to_selected = true;
        }
    }

    fn open_search_selected(&mut self) {
        let entry = self
            .search_ui
            .selected
            .and_then(|i| self.search_ui.results.get(i))
            .cloned();
        if let Some(it) = entry {
            self.open_entry(it.path, it.kind);
        }
    }

    fn open_search_selected_parent(&mut self) {
        let entry = self
            .search_ui
            .selected
            .and_then(|i| self.search_ui.results.get(i))
            .cloned();
        if let Some(it) = entry {
            let tab_id = self.state.active_tab().id;
            self.navigate_tab_to(tab_id, it.parent_path);
        }
    }

    fn request_info_hash(&mut self, path: PathBuf) {
        if self.info_panel_hashes.contains_key(&path) {
            return;
        }
        if self.info_panel_hash_inflight.as_ref() == Some(&path) {
            return;
        }
        self.info_panel_hash_inflight = Some(path.clone());
        let tx = self.hash_tx.clone();
        std::thread::spawn(move || {
            let res = compute_sha256(&path);
            let _ = tx.send((path, res));
        });
    }

    fn open_target_picker(&mut self) {
        if self.target_dropdown.open {
            return;
        }
        let start_dir = self.state.active_tab().current_dir.clone();
        self.target_dropdown.open = true;
        self.target_dropdown.dir = Some(start_dir);
        self.refresh_target_picker_entries();
    }

    fn refresh_target_picker_entries(&mut self) {
        let Some(dir) = self.target_dropdown.dir.clone() else {
            return;
        };
        match list_directory(&dir, self.state.config.show_hidden_files) {
            Ok(mut entries) => {
                entries.retain(|e| matches!(e.kind, EntryKind::Directory | EntryKind::Symlink));
                self.target_dropdown.entries = entries;
                self.target_dropdown.error = None;
            }
            Err(e) => {
                self.target_dropdown.entries.clear();
                self.target_dropdown.error = Some(e);
            }
        }
    }

    fn enter_target_picker_dir(&mut self, dir: PathBuf) {
        self.target_dropdown.dir = Some(dir);
        self.refresh_target_picker_entries();
    }

    fn set_drop_folder(&mut self, dir: PathBuf, open_panel: bool, show_confirmation: bool) {
        self.state.config.target.set(dir);
        self.target_dropdown.open = false;
        if open_panel {
            self.sidebar = SidebarSection::Target;
        }
        if show_confirmation {
            self.drop_folder_confirm_until = Some(Instant::now() + Duration::from_secs(5));
        }
    }

    fn confirm_target_picker_current(&mut self) {
        if let Some(dir) = self.target_dropdown.dir.clone() {
            self.set_drop_folder(dir, true, false);
        }
    }

    fn is_drop_folder_hint_suppressed_system_path(dir: &Path) -> bool {
        #[cfg(target_os = "windows")]
        {
            let _ = dir;
            false
        }
        #[cfg(not(target_os = "windows"))]
        {
            const BLOCKED: [&str; 5] = ["/bin", "/usr", "/etc", "/dev", "/tmp"];
            BLOCKED.iter().any(|p| {
                let root = Path::new(p);
                dir == root || dir.starts_with(root)
            })
        }
    }

    fn should_show_drop_folder_hint_for_dir(&self, dir: &Path) -> bool {
        !self.state.config.target.is_set()
            && !self.state.config.target.has_ever_set
            && !Self::is_drop_folder_hint_suppressed_system_path(dir)
    }

    fn render_empty_folder_state(
        &mut self,
        ui: &mut egui::Ui,
        dir: &Path,
        c: &Colors,
        show_drop_folder_hint: bool,
    ) {
        let show_hint = show_drop_folder_hint && self.should_show_drop_folder_hint_for_dir(dir);
        ui.add_space(16.0);
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.label(
                RichText::new("This folder is empty")
                    .color(c.text_muted)
                    .size(12.0),
            );
            if show_hint {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Set this folder as Drop Folder")
                        .color(c.text_dim)
                        .size(11.5)
                        .strong(),
                );
                ui.label(
                    RichText::new("Move or copy files here from anywhere.")
                        .color(c.text_muted)
                        .size(10.5),
                );
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("{} Set as Drop Folder", MI_MOVE_TO_INBOX))
                                .size(11.5)
                                .color(c.text),
                        )
                        .min_size(Vec2::new(198.0, 28.0))
                        .corner_radius(4.0),
                    )
                    .clicked()
                {
                    self.set_drop_folder(dir.to_path_buf(), true, true);
                }
            }
        });
        if let Some(until) = self.drop_folder_confirm_until {
            if Instant::now() < until {
                ui.add_space(8.0);
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.label(
                        RichText::new("✓ This folder is now your Drop Folder")
                            .color(c.accent)
                            .size(11.0)
                            .strong(),
                    );
                    ui.label(
                        RichText::new("Right-click files and choose “Move to Drop Folder”")
                            .color(c.text_muted)
                            .size(10.5),
                    );
                });
                ui.ctx().request_repaint();
            } else {
                self.drop_folder_confirm_until = None;
            }
        }
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
            self.set_status_message(err.clone(), false, 4);
            self.command_frame.error = Some(err);
            self.command_frame.message = None;
            self.command_frame.request_focus = true; // keep user typing after an error
        } else if keep_open {
            if let Some(msg) = &message {
                self.set_status_message(msg.clone(), true, 3);
            }
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
    fn dispatch_command(
        &mut self,
        raw: &str,
        tab_id: u64,
        cwd: &Path,
    ) -> (Option<String>, Option<String>, bool) {
        let parts: Vec<&str> = raw.splitn(3, ' ').collect();
        let cmd_word = parts[0].to_lowercase();

        // helper closures
        macro_rules! err {
            ($e:expr) => {
                (Some($e.into()), None, false)
            };
        }
        macro_rules! ok {
            () => {
                (None, None, false)
            };
        }
        macro_rules! msg {
            ($m:expr) => {
                (None, Some($m.into()), true)
            };
        }

        match cmd_word.as_str() {
            "help" | "?" => {
                msg!("cd  mkdir  touch  cp  mv  rm [-f]  chmod  ln -s  terminal  <path>")
            }
            "search" => {
                let q = raw.get(parts[0].len()..).map(str::trim).unwrap_or("");
                self.sidebar = SidebarSection::Search;
                self.search_ui.query = q.to_string();
                self.search_ui.request_focus = true;
                self.search_ui.scroll_to_selected = true;
                self.run_search_query();
                ok!()
            }
            "cd" | "/" => {
                let target_str = parts.get(1).copied().unwrap_or("~");
                let target = resolve_path(target_str, cwd);
                if target.is_dir() {
                    // cd should enter the folder: select first item and
                    // set focus to the current-dir column so Up/Down
                    // navigate within the target, not its parent.
                    let ui_state = self.tab_ui.entry(tab_id).or_default();
                    ui_state.pending_select_first = true;
                    ui_state.miller_focus_hint = Some(MillerFocusHint::CurrentDir);
                    ui_state.miller_preview_depth = 1;
                    self.navigate_tab_to(tab_id, target);
                    ok!()
                } else {
                    err!(format!("Not a directory: {}", target.display()))
                }
            }
            "mkdir" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    return err!("Usage: mkdir <name>");
                }
                let parent = cwd.to_path_buf();
                let cmd = FileCommand::CreateFolder {
                    parent,
                    name: name.to_string(),
                };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "touch" => {
                let name = parts.get(1).copied().unwrap_or("");
                if name.is_empty() {
                    return err!("Usage: touch <name>");
                }
                let parent = cwd.to_path_buf();
                let cmd = FileCommand::CreateFile {
                    parent,
                    name: name.to_string(),
                };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "cp" => {
                let src_str = parts.get(1).copied().unwrap_or("");
                let dst_str = parts.get(2).copied().unwrap_or_else(|| {
                    self.state
                        .config
                        .target
                        .current
                        .as_ref()
                        .map(|p| p.to_str().unwrap_or(""))
                        .unwrap_or("")
                });
                let src = resolve_path(src_str, cwd);
                let dst = resolve_path(dst_str, cwd);
                if !src.exists() {
                    return err!(format!("No such file: {}", src.display()));
                }
                if !dst.is_dir() {
                    return err!(format!("Destination not a directory: {}", dst.display()));
                }
                let cmd = FileCommand::Copy {
                    sources: vec![src],
                    destination: dst,
                    conflict: ConflictAction::Rename,
                };
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
                if !src.exists() {
                    return err!(format!("No such file: {}", src.display()));
                }
                match dst_resolved {
                    None => err!("No destination specified and no drop folder set."),
                    Some(dst) => {
                        if !dst.is_dir() {
                            return err!(format!("Destination not a directory: {}", dst.display()));
                        }
                        let cmd = FileCommand::Move {
                            sources: vec![src],
                            destination: dst,
                            conflict: ConflictAction::Rename,
                        };
                        self.run_file_op(cmd, Some(tab_id));
                        ok!()
                    }
                }
            }
            "rm" => {
                let force = parts.get(1).copied() == Some("-f");
                let src_str = parts.get(if force { 2 } else { 1 }).copied().unwrap_or("");
                if src_str.is_empty() {
                    return err!("Usage: rm [-f] <file>");
                }
                let src = resolve_path(src_str, cwd);
                if !src.exists() {
                    return err!(format!("No such file: {}", src.display()));
                }
                let mode = if force {
                    DeleteMode::Permanent
                } else {
                    DeleteMode::Trash
                };
                let cmd = FileCommand::Delete {
                    targets: vec![src],
                    mode,
                };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "chmod" => {
                let mode_str = parts.get(1).copied().unwrap_or("").to_string();
                let file_str = parts
                    .get(2)
                    .copied()
                    .unwrap_or(parts.get(1).copied().unwrap_or(""));
                let (m, f) = if mode_str.starts_with('+') || mode_str.starts_with('-') {
                    (
                        mode_str.clone(),
                        parts
                            .get(2)
                            .copied()
                            .unwrap_or(parts.get(1).copied().unwrap_or("")),
                    )
                } else {
                    (mode_str.clone(), file_str)
                };
                let target = resolve_path(f, cwd);
                if !target.exists() {
                    return err!(format!("No such file: {}", target.display()));
                }
                let cmd = FileCommand::Chmod {
                    target,
                    mode_str: m,
                };
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
                let cmd = FileCommand::Symlink {
                    link_path,
                    target: target_path,
                };
                self.run_file_op(cmd, Some(tab_id));
                ok!()
            }
            "terminal" | "cmd" | "term" => {
                open_terminal_here(cwd);
                ok!()
            }
            _ => {
                let target = resolve_path(raw, cwd);
                if target.is_dir() {
                    self.navigate_tab_to(tab_id, target);
                    ok!()
                } else {
                    err!(format!(
                        "Unknown command: {}  (type 'help' for a list)",
                        cmd_word
                    ))
                }
            }
        }
    }

    // ── Rendering: top-level ─────────────────────────────────────────────────

    /// Row 1 (topmost): tabs on the left, window controls on the right.
    /// This panel is also draggable to move the window.
    fn render_tab_row(&mut self, ctx: &Context) {
        let c = self.colors;
        let mut blocked_drag_rects: Vec<Rect> = Vec::new();
        let titlebar_bg = c.titlebar_bg;
        let active_tab_bg = mix_color(c.toolbar_bg, titlebar_bg, 0.28);
        let inactive_text = mix_color(c.text_muted, titlebar_bg, 0.32);

        let panel_resp = egui::TopBottomPanel::top("tab_row")
            .exact_height(CUSTOM_TITLEBAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(titlebar_bg)
                    .inner_margin(egui::Margin::symmetric(6, 1)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let n_tabs = self.state.tabs.len();
                    let active = self.state.active_tab_idx;
                    let tandem = self.current_tandem_view();
                    let mut navigate_to: Option<(u64, PathBuf)> = None;
                    let mut close_idx: Option<usize> = None;
                    let mut new_tab_requested = false;

                    for i in 0..n_tabs {
                        let tab = &self.state.tabs[i];
                        let is_active = i == active;
                        let label = tab.display_name().to_string();
                        let tab_id = tab.id;

                        // Active tab should visually connect to the nav row below.
                        let tab_fill = if is_active {
                            active_tab_bg
                        } else {
                            Color32::TRANSPARENT
                        };
                        let text_color = if is_active {
                            c.text_muted
                        } else {
                            inactive_text
                        };
                        let mut close_rect: Option<Rect> = None;

                        let frame = Frame::new()
                            .fill(tab_fill)
                            .stroke(Stroke::NONE)
                            .corner_radius(egui::CornerRadius {
                                nw: 5,
                                ne: 5,
                                sw: 0,
                                se: 0,
                            })
                            .inner_margin(egui::Margin {
                                left: 14,
                                right: 6,
                                top: 4,
                                bottom: 4,
                            });

                        let resp = frame.show(ui, |ui| {
                            ui.set_max_width(190.0);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                if let Some(link) = tandem.as_ref()
                                    && link.contains_tab(tab_id)
                                {
                                    let dot_color = if link.active_tab_id() == tab_id {
                                        c.accent
                                    } else {
                                        c.text_muted
                                    };
                                    ui.label(RichText::new("•").color(dot_color).size(14.0));
                                    if link.pinned_tab_id == Some(tab_id) {
                                        ui.add_space(2.0);
                                        ui.label(RichText::new(MI_LOCK).color(c.accent).size(11.5));
                                    }
                                    ui.add_space(4.0);
                                }
                                ui.label(RichText::new(&label).color(text_color).size(12.5));
                                ui.add_space(6.0);
                                if n_tabs > 1 {
                                    let close_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new(MI_CLOSE).color(c.text_dim).size(13.0),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(18.0, 18.0)),
                                    );
                                    close_rect = Some(close_btn.rect);
                                    if close_btn.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if close_btn.clicked() {
                                        close_idx = Some(i);
                                    }
                                }
                            });
                        });

                        let tab_rect = resp.response.rect;
                        let tab_click_rect = if let Some(close_rect) = close_rect {
                            blocked_drag_rects.push(close_rect);
                            Rect::from_min_max(
                                tab_rect.min,
                                egui::pos2(
                                    (close_rect.min.x - 6.0).max(tab_rect.min.x),
                                    tab_rect.max.y,
                                ),
                            )
                        } else {
                            tab_rect
                        };
                        // Frame::show response has Sense::hover only — interact explicitly for clicks
                        let tab_click = ui.interact(
                            tab_click_rect,
                            ui.id().with(("tab_click", tab_id)),
                            Sense::click(),
                        );
                        if tab_click.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        blocked_drag_rects.push(tab_rect);
                        if tab_click.clicked() && close_idx.is_none() && !is_active {
                            let shift = ui.ctx().input(|input| input.modifiers.shift);
                            if shift {
                                let active_idx = self.state.active_tab_idx;
                                if active_idx != i {
                                    let _ = self.state.activate_tandem(active_idx, i);
                                }
                            } else {
                                self.focus_tab_idx(i);
                            }
                            if !self.tab_ui.contains_key(&tab_id) {
                                let dir = self.state.tabs[i].current_dir.clone();
                                navigate_to = Some((tab_id, dir));
                            }
                        }
                        resp.response
                            .on_hover_text(self.state.tabs[i].current_dir.display().to_string());
                        if is_active {
                            ui.painter().hline(
                                tab_rect.x_range(),
                                tab_rect.bottom() - 1.0,
                                Stroke::new(2.0, c.accent),
                            );
                        }
                    }

                    // New tab +
                    ui.add_space(6.0);
                    let new_tab_btn = ui.add(
                        egui::Button::new(RichText::new("+").color(c.text_muted).size(16.0))
                            .frame(false),
                    );
                    blocked_drag_rects.push(new_tab_btn.rect);
                    if new_tab_btn.clicked() {
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
                        self.focus_tab_idx(idx);
                        let new_id = self.state.tabs.last().unwrap().id;
                        self.refresh_tab(new_id, home);
                    }

                    // Window controls — far right, Material Icons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        let close_btn = wm_btn(ui, WmBtn::Close, &c).on_hover_text("Close");
                        blocked_drag_rects.push(close_btn.rect);
                        if close_btn.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        let max_btn = wm_btn(ui, WmBtn::Maximize, &c).on_hover_text(if is_max {
                            "Restore"
                        } else {
                            "Maximize"
                        });
                        blocked_drag_rects.push(max_btn.rect);
                        if max_btn.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                        }
                        let min_btn = wm_btn(ui, WmBtn::Minimize, &c).on_hover_text("Minimize");
                        blocked_drag_rects.push(min_btn.rect);
                        if min_btn.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });

        ctx.layer_painter(egui::LayerId::background()).hline(
            panel_resp.response.rect.x_range(),
            panel_resp.response.rect.bottom(),
            Stroke::new(1.0, mix_color(c.border, titlebar_bg, 0.55)),
        );

        // Drag only from non-interactive tab-row areas so button clicks do not race StartDrag.
        let panel_rect = panel_resp.response.rect;
        let can_drag = ctx.input(|i| {
            if !i.pointer.primary_pressed() {
                return false;
            }
            let Some(pos) = i.pointer.hover_pos() else {
                return false;
            };
            if !panel_rect.contains(pos) {
                return false;
            }
            !blocked_drag_rects.iter().any(|r| r.contains(pos))
        });
        if can_drag {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    /// Row 2: back/forward/up navigation, address bar, view toggle, theme toggle.
    fn render_nav_row(&mut self, ctx: &Context) {
        let c = self.colors;
        let active_tab_id = self.state.active_tab().id;

        egui::TopBottomPanel::top("nav_row")
            .exact_height(CUSTOM_TITLEBAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(c.toolbar_bg)
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let can_back = self.state.active_tab().can_go_back();
                    let can_fwd = self.state.active_tab().can_go_forward();
                    let can_up = self.state.active_tab().can_go_up();

                    let back_col = if can_back {
                        c.text_muted
                    } else {
                        c.text_muted.gamma_multiply(0.62)
                    };
                    let fwd_col = if can_fwd {
                        c.text_muted
                    } else {
                        c.text_muted.gamma_multiply(0.62)
                    };
                    let up_col = if can_up {
                        c.text_muted
                    } else {
                        c.text_muted.gamma_multiply(0.62)
                    };
                    if ui
                        .add_enabled(
                            can_back,
                            egui::Button::new(
                                RichText::new(MI_ARROW_BACK).size(20.0).color(back_col),
                            )
                            .frame(false)
                            .min_size(Vec2::splat(30.0)),
                        )
                        .on_hover_text("Back")
                        .clicked()
                    {
                        self.navigate_active_back();
                    }
                    if ui
                        .add_enabled(
                            can_fwd,
                            egui::Button::new(
                                RichText::new(MI_ARROW_FORWARD).size(20.0).color(fwd_col),
                            )
                            .frame(false)
                            .min_size(Vec2::splat(30.0)),
                        )
                        .on_hover_text("Forward")
                        .clicked()
                    {
                        self.navigate_active_forward();
                    }
                    if ui
                        .add_enabled(
                            can_up,
                            egui::Button::new(RichText::new(MI_ARROW_UP).size(20.0).color(up_col))
                                .frame(false)
                                .min_size(Vec2::splat(30.0)),
                        )
                        .on_hover_text("Parent folder")
                        .clicked()
                    {
                        self.navigate_active_up();
                    }
                    ui.add_space(6.0);

                    // Right side: view toggle, settings menu, then address bar fills the rest
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);

                        // ── Hamburger menu ────────────────────────────────────
                        let menu_popup_id = ui.make_persistent_id("hamburger_menu");
                        let menu_btn = ui
                            .add(
                                egui::Button::image(
                                    egui::Image::new(egui::include_image!(
                                        "../../../assets/icon.svg"
                                    ))
                                    .fit_to_exact_size(Vec2::splat(20.0))
                                    .tint(c.text_muted),
                                )
                                .frame(false)
                                .min_size(Vec2::splat(30.0)),
                            )
                            .on_hover_text("Menu");
                        if menu_btn.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        egui::Popup::menu(&menu_btn)
                            .id(menu_popup_id)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(190.0);
                                ui.style_mut().spacing.item_spacing.y = 0.0;

                                // ── Create ─────────────────────────────────────
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!(
                                                "{} New Folder",
                                                MI_CREATE_FOLDER
                                            ))
                                            .size(13.0),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(190.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.command_frame.input = "mkdir ".to_string();
                                    self.command_frame.visible = true;
                                    self.command_frame.request_focus = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("{} New File", MI_FILE))
                                                .size(13.0),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(190.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.command_frame.input = "touch ".to_string();
                                    self.command_frame.visible = true;
                                    self.command_frame.request_focus = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }

                                ui.add(egui::Separator::default().horizontal().spacing(4.0));

                                // ── Settings ───────────────────────────────────
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("{} Settings…", MI_SETTINGS))
                                                .size(13.0),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(190.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.show_settings = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }

                                ui.add(egui::Separator::default().horizontal().spacing(4.0));

                                // ── About ──────────────────────────────────────
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!("{} About Ottrin", MI_INFO))
                                                .size(13.0),
                                        )
                                        .frame(false)
                                        .min_size(Vec2::new(190.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.show_about = true;
                                    egui::Popup::close_id(ui.ctx(), menu_popup_id);
                                }
                            });

                        // View mode toggle — Material Icon
                        let view = self.state.active_tab().view_mode;
                        let (view_icon, view_tip) = match view {
                            ViewMode::Miller => (MI_VIEW_COLUMN, "Switch to List view"),
                            ViewMode::List => (MI_VIEW_LIST, "Switch to Grid view"),
                            ViewMode::Grid => (MI_APPS, "Switch to Column view"),
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(view_icon).size(20.0).color(c.text_muted),
                                )
                                .frame(false)
                                .min_size(Vec2::splat(30.0)),
                            )
                            .on_hover_text(view_tip)
                            .clicked()
                        {
                            let next = match view {
                                ViewMode::Miller => ViewMode::List,
                                ViewMode::List => ViewMode::Grid,
                                ViewMode::Grid => ViewMode::Miller,
                            };
                            self.state.active_tab_mut().view_mode = next;
                        }
                        ui.add_space(6.0);

                        // Address bar fills the remaining width (leave 20px gap on left)
                        let bw = ui.available_width();
                        ui.allocate_ui(Vec2::new((bw - 20.0).max(80.0), 26.0), |ui| {
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                self.render_address_bar(ui, active_tab_id);
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
                (MI_HOME.to_string(), "Home".to_string(), home.clone()),
                ("/".to_string(), "Root".to_string(), PathBuf::from("/")),
                (
                    MI_FOLDER.to_string(),
                    "Desktop".to_string(),
                    home.join("Desktop"),
                ),
                (
                    MI_FOLDER.to_string(),
                    "Downloads".to_string(),
                    home.join("Downloads"),
                ),
                (
                    MI_FOLDER.to_string(),
                    "Documents".to_string(),
                    home.join("Documents"),
                ),
            ];
            self.state.config.bookmarks = defaults
                .into_iter()
                .filter(|(_, _, p)| p.exists())
                .collect();
        }

        let mut navigate_to: Option<PathBuf> = None;
        let mut remove_idx: Option<usize> = None;
        let displayed_path = self.displayed_path();

        egui::TopBottomPanel::top("bookmarks_row")
            .exact_height(30.0)
            .frame(
                Frame::new()
                    .fill(c.toolbar_bg)
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ctx, |ui| {
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.max_rect().bottom(),
                    Stroke::new(1.0, mix_color(c.border, c.toolbar_bg, 0.5)),
                );
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let bookmarks = self.state.config.bookmarks.clone();
                    for (idx, (icon, name, path)) in bookmarks.iter().enumerate() {
                        let is_here = &displayed_path == path;
                        // All bookmarks look like buttons; active one uses accent tint
                        let (text_col, bg_col) = if is_here {
                            (c.text_muted, c.selected_bg.gamma_multiply(0.62))
                        } else {
                            (c.text_muted, mix_color(c.toolbar_bg, c.panel, 0.28))
                        };

                        let btn = egui::Button::new(
                            RichText::new(format!("{} {}", icon, name))
                                .size(12.0)
                                .color(text_col),
                        )
                        .fill(bg_col)
                        .stroke(Stroke::NONE)
                        .corner_radius(c.border_radius as f32)
                        .min_size(Vec2::new(0.0, 22.0));

                        let resp = ui.add(btn).on_hover_text(path.display().to_string());
                        if resp.clicked() {
                            navigate_to = Some(path.clone());
                        }
                        resp.context_menu(|ui| {
                            ui.visuals_mut().override_text_color = Some(c.text_muted);
                            ui.set_min_width(160.0);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!("{} Remove \"{}\"", MI_CLEAR, name))
                                            .size(13.0),
                                    )
                                    .frame(false)
                                    .min_size(Vec2::new(160.0, 28.0)),
                                )
                                .clicked()
                            {
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

    fn render_address_bar(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let is_editing_here = self
            .address_bar_state(tab_id)
            .map(|bar| bar.editing && bar.tab_id == Some(tab_id))
            .unwrap_or(false);

        if is_editing_here {
            let mut submit_target: Option<PathBuf> = None;
            let mut invalid_target = false;
            let mut close = false;
            {
                let bar = self.address_bar_state_mut(tab_id);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut bar.text)
                        .id_source(("address_bar_text", tab_id))
                        .font(FontId::proportional(14.0))
                        .text_color(c.text_muted)
                        .frame(true)
                        .desired_width(ui.available_width()),
                );
                if bar.request_focus {
                    response.request_focus();
                    bar.request_focus = false;
                }
                if response.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        let raw = bar.text.trim().to_string();
                        let cwd = self
                            .state
                            .tab_by_id(tab_id)
                            .map(|tab| tab.current_dir.clone())
                            .unwrap_or_else(default_home_dir);
                        let target = resolve_path(&raw, &cwd);
                        if target.is_dir() {
                            submit_target = Some(target);
                        } else {
                            invalid_target = true;
                        }
                    }
                    close = true;
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    close = true;
                }
            }
            if let Some(target) = submit_target {
                self.navigate_tab_to(tab_id, target);
            }
            if invalid_target {
                self.set_status_message("Not a directory", false, 2);
            }
            if close {
                let bar = self.address_bar_state_mut(tab_id);
                bar.editing = false;
                bar.tab_id = None;
            }
        } else {
            // Breadcrumb display
            let path = self.displayed_path_for(tab_id);
            let palette = semantic_palette(&self.state.config, &c);
            let frame = Frame::new()
                .fill(c.toolbar_bg)
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
                        let text_color = c.text_muted;
                        let icon = if dest == Path::new("/") {
                            MI_STORAGE
                        } else {
                            let sem = semantic_for_path(&dest);
                            let style = style_for_semantic(
                                &sem,
                                EntryVisualState::default(),
                                &c,
                                &palette,
                                self.state.config.theme_preset,
                                self.state.config.colorize_file_types,
                                self.state.config.colorize_folder_labels,
                            );
                            material_icon(style.icon)
                        };
                        let resp = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} {}", icon, display))
                                    .color(text_color)
                                    .size(13.0),
                            )
                            .frame(false),
                        );
                        if resp.clicked() {
                            self.focus_tab(tab_id);
                            self.navigate_tab_to(tab_id, dest);
                        }
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if !is_last {
                            ui.label(RichText::new("›").color(c.text_muted).size(12.0));
                        }
                    }
                });
            });
            // Click anywhere in the bar to switch to path-edit mode
            let bar_interact = ui.interact(
                addr_resp.response.rect,
                egui::Id::new(("addr_bar_click", tab_id)),
                Sense::click(),
            );
            if addr_resp.response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            if bar_interact.clicked() {
                let path_str = self.displayed_path_for(tab_id).display().to_string();
                self.focus_tab(tab_id);
                let bar = self.address_bar_state_mut(tab_id);
                bar.text = path_str;
                bar.editing = true;
                bar.request_focus = true;
                bar.tab_id = Some(tab_id);
            }
        }
    }

    fn compute_smart_panel_layout(&mut self, avail: Rect) -> (Rect, Rect) {
        let is_expanded = self.sidebar != SidebarSection::None;
        let search_active = self.sidebar == SidebarSection::Search;
        let strip_w = SMART_PANEL_STRIP_WIDTH;
        // In Miller view, Info is rendered inline as the rightmost column
        // (Finder-style preview), so the sidebar only needs the icon strip.
        let is_miller = matches!(self.state.active_tab().view_mode, ViewMode::Miller);
        let sidebar_inline =
            is_miller && matches!(self.sidebar, SidebarSection::Info | SidebarSection::None);
        let max_content_w = (avail.width() - 280.0).max(220.0);
        let content_w = if sidebar_inline || !is_expanded {
            0.0
        } else if search_active {
            let desired = if self.search_ui.panel_width > 1.0 {
                self.search_ui.panel_width
            } else {
                520.0
            };
            desired.clamp(300.0, max_content_w)
        } else {
            220.0
        };
        let panel_width = if content_w > 0.0 {
            strip_w + content_w
        } else {
            strip_w
        };
        let panel_rect = Rect::from_min_size(
            egui::pos2(avail.max.x - panel_width, avail.min.y),
            Vec2::new(panel_width, avail.height()),
        );
        let file_rect = Rect::from_min_max(avail.min, egui::pos2(panel_rect.min.x, avail.max.y));
        (file_rect, panel_rect)
    }

    fn render_target_sidebar(&mut self, ctx: &Context) {
        let c = self.colors;
        let target_set = self.state.config.target.is_set();
        let is_expanded = self.sidebar != SidebarSection::None;
        let search_active = self.sidebar == SidebarSection::Search;
        let Some(panel_rect) = self.smart_panel_rect else {
            return;
        };
        let avail = panel_rect;
        if avail.width() < 32.0 || avail.height() <= 48.0 {
            return;
        }
        // Always show the 36px icon strip; when expanded add content panel.
        let strip_w = SMART_PANEL_STRIP_WIDTH;

        egui::Area::new(egui::Id::new("target_panel_area"))
            .order(egui::Order::Middle)
            .fixed_pos(panel_rect.min)
            .show(ctx, |ui| {
                ui.set_min_size(panel_rect.size());
                let full = Rect::from_min_size(ui.min_rect().min, panel_rect.size());
                ui.painter().rect_filled(full, 0.0, c.smart_panel_bg);
                // Left border — thin line separating panel from file view
                ui.painter().vline(full.min.x, full.y_range(), Stroke::new(1.0, c.border));
                let live_content_w = if is_expanded {
                    (full.width() - strip_w).max(0.0)
                } else {
                    0.0
                };
                if is_expanded && search_active {
                    self.search_ui.panel_width = live_content_w;
                }

                // ── Icon strip (always visible, rightmost 36px) ─────────────
                let strip_rect = Rect::from_min_size(
                    egui::Pos2::new(full.max.x - strip_w, full.min.y),
                    Vec2::new(strip_w, full.height()),
                );
                // Strip background — slightly raised, with left border
                ui.painter().rect_filled(strip_rect, 0.0, mix_color(c.smart_panel_bg, c.panel_raised, 0.3));
                ui.painter().vline(strip_rect.min.x, strip_rect.y_range(), Stroke::new(1.0, c.border));

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
                        let info_col = if info_active { c.accent } else { c.text_muted };
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
                            RichText::new(MI_MOVE_TO_INBOX).size(22.0).color(target_col)
                        ).frame(false).min_size(Vec2::splat(30.0)))
                        .on_hover_text(if target_set { "Drop Folder set" } else { "No drop folder set" }).clicked() {
                            self.sidebar = if target_active { SidebarSection::None } else { SidebarSection::Target };
                        }
                        ui.add_space(8.0);
                        let search_active = self.sidebar == SidebarSection::Search;
                        if search_active {
                            ui.painter().rect_filled(
                                Rect::from_min_size(egui::Pos2::new(strip_rect.min.x, ui.cursor().min.y - 2.0), Vec2::new(strip_w, 32.0)),
                                4.0, c.selected_bg,
                            );
                            ui.painter().vline(strip_rect.min.x + 1.0,
                                (ui.cursor().min.y - 2.0)..=(ui.cursor().min.y + 30.0),
                                Stroke::new(2.0, c.accent));
                        }
                        let search_diag = self.search_service.diagnostics();
                        let search_col = if search_active {
                            c.accent
                        } else {
                            c.text_muted
                        };
                        let mut btn = ui.add(egui::Button::new(
                            RichText::new(MI_SEARCH).size(20.0).color(search_col)
                        ).frame(false).min_size(Vec2::splat(30.0)));
                        btn = btn.on_hover_text(search_diagnostics_tooltip(
                            &search_diag,
                            self.search_ui.scope,
                            &self.state.active_tab().current_dir,
                        ));
                        if btn.clicked() {
                            self.sidebar = if search_active { SidebarSection::None } else { SidebarSection::Search };
                            if self.sidebar == SidebarSection::Search {
                                self.search_ui.request_focus = true;
                                self.command_frame.visible = false;
                                self.command_frame.error = None;
                                self.command_frame.message = None;
                                self.run_search_query();
                            }
                        }
                    });
                });

                if !is_expanded { return; }

                // In Miller view, Info content is rendered inline as the
                // rightmost column (Finder-style preview).
                let is_miller = matches!(
                    self.state.active_tab().view_mode,
                    ViewMode::Miller
                );
                if is_miller && matches!(self.sidebar, SidebarSection::Info) {
                    return;
                }

                // Single separator between content and strip only.
                ui.painter().vline(full.max.x - strip_w, full.y_range(), Stroke::new(1.0, mix_color(c.border, c.smart_panel_bg, 0.45)));

                // ── Content panel (left 220px when expanded) ────────────────
                let content_pad = 8.0;
                let content_rect = Rect::from_min_max(
                    egui::Pos2::new(full.min.x + content_pad, full.min.y + content_pad),
                    egui::Pos2::new(full.min.x + live_content_w - content_pad, full.max.y - content_pad),
                );
                ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
                    ui.set_clip_rect(content_rect);
                    match self.sidebar {
                        SidebarSection::None => {}

                        // ── Info section ───────────────────────────────────────
                        SidebarSection::Info => {
                            // Panel title bar
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(MI_DESCRIPTION).size(13.0).color(c.text_dim));
                                ui.add_space(4.0);
                                ui.label(RichText::new("Info").color(c.text_dim).size(12.0).strong());
                            });
                            ui.add_space(6.0);
                            ui.painter().hline(content_rect.x_range(), ui.cursor().min.y, Stroke::new(1.0, c.border));
                            ui.add_space(8.0);

                            let selected = self.active_selected_entry();

                            if let Some(entry) = selected {
                                let style = style_for_entry(
                                    &entry,
                                    EntryVisualState {
                                        selected: true,
                                        focused: true,
                                        hovered: false,
                                        opened: false,
                                        symlink_dir: matches!(entry.kind, EntryKind::Symlink)
                                            && entry.symlink_target_is_dir == Some(true),
                                    },
                                    &c,
                                    &self.state.config,
                                );
                                let icon = material_icon(style.icon);
                                let icon_color = style.icon_color;
                                let mut preview_widget: Option<egui::Image<'static>> = None;
                                let mut text_snippet: Option<String> = None;
                                if matches!(entry.kind, EntryKind::File) {
                                    let preview = self.cached_info_preview(&entry);
                                    match preview.kind {
                                        PreviewKind::Image => {
                                            let max_w = (ui.available_width() - 28.0).max(96.0);
                                            let max_h = 132.0;
                                            if let Some(img) = self.cached_image_preview_widget(&entry.path, max_w, max_h, 8) {
                                                preview_widget = Some(img);
                                            }
                                        }
                                        PreviewKind::Text => {
                                            let is_svg = entry
                                                .path
                                                .extension()
                                                .and_then(|e| e.to_str())
                                                .map(|e| e.eq_ignore_ascii_case("svg"))
                                                .unwrap_or(false);
                                            if is_svg {
                                                let max_w = (ui.available_width() - 28.0).max(96.0);
                                                let max_h = 132.0;
                                                if let Some(img) = self.cached_image_preview_widget(&entry.path, max_w, max_h, 8) {
                                                    preview_widget = Some(img);
                                                }
                                            }
                                            if preview_widget.is_none() {
                                                const SNIPPET_MAX_CHARS: usize = 480;
                                                let mut snippet = String::new();
                                                let mut chars_left = SNIPPET_MAX_CHARS;
                                                for (i, line) in preview.body.lines().enumerate() {
                                                    if i >= 8 || chars_left == 0 {
                                                        break;
                                                    }
                                                    if i > 0 {
                                                        snippet.push('\n');
                                                    }
                                                    let take = line.len().min(chars_left);
                                                    snippet.push_str(&line[..take]);
                                                    chars_left = chars_left.saturating_sub(take);
                                                }
                                                if !snippet.is_empty() {
                                                    text_snippet = Some(snippet);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                ui.add_space(4.0);
                                Frame::new()
                                    .fill(Color32::TRANSPARENT)
                                    .inner_margin(egui::Margin::symmetric(6, 4))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(ui.available_width(), 118.0),
                                            Layout::top_down(Align::Center),
                                            |ui| {
                                                // Clip to allocated rect so overflowing
                                                // text never pushes metadata rows away.
                                                let clip = ui.clip_rect().intersect(ui.max_rect());
                                                ui.set_clip_rect(clip);
                                                let preview_size = Vec2::new((ui.available_width() - 12.0).max(72.0), 92.0);
                                                if let Some(img) = preview_widget {
                                                    ui.add(img.max_height(preview_size.y).max_width(preview_size.x));
                                                    ui.add_space(6.0);
                                                } else if let Some(mut snippet) = text_snippet {
                                                    ui.add_sized(
                                                        preview_size,
                                                        egui::TextEdit::multiline(&mut snippet)
                                                            .font(FontId::monospace(10.0))
                                                            .desired_rows(6)
                                                            .interactive(false),
                                                    );
                                                    ui.add_space(6.0);
                                                } else {
                                                    ui.label(RichText::new(icon).size(34.0).color(icon_color));
                                                    ui.add_space(4.0);
                                                }
                                                ui.label(
                                                    RichText::new(truncate_name(&entry.name, 20))
                                                        .color(c.text)
                                                        .size(12.0)
                                                        .strong(),
                                                );
                                            },
                                        );
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
                                ui.add_space(6.0);
                                let details = self.cached_info_details(&entry);
                                egui::CollapsingHeader::new(
                                    RichText::new("More Details").color(c.text_dim).size(10.5)
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("Read-only").color(lc).size(10.0));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new(if details.readonly { "Yes" } else { "No" })
                                                .color(vc).size(10.0));
                                        });
                                    });
                                    if let Some(child_count) = details.child_count {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Items").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(child_count.to_string()).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(ext) = details.extension.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Extension").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(ext).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(owner) = details.owner.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Owner").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(owner).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(group) = details.group.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Group").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(group).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(perms) = details.permissions.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Permissions").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(perms).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(created) = details.created.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Created").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(created).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(accessed) = details.accessed.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Accessed").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(accessed).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(target) = details.symlink_target.as_deref() {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Target").color(lc).size(10.0));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(target).color(vc).size(10.0));
                                        });
                                    }
                                });
                                egui::CollapsingHeader::new(
                                    RichText::new("Advanced").color(c.text_dim).size(10.5)
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    if let Some(inode) = details.inode {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Inode").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(inode.to_string()).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(device) = details.device {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Device").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(device.to_string()).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(links) = details.links {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Hard links").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(links.to_string()).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(dimensions) = details.image_dimensions.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Dimensions").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(dimensions).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    if let Some(fmt) = details.image_format.as_deref() {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Format").color(lc).size(10.0));
                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(fmt).color(vc).size(10.0));
                                            });
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        ui.add_space(8.0);
                                        ui.label(RichText::new("SHA-256").color(lc).size(10.0));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.add_space(8.0);
                                            let inflight = self.info_panel_hash_inflight.as_ref() == Some(&entry.path);
                                            if let Some(hash) = self.info_panel_hashes.get(&entry.path) {
                                                if ui.add(egui::Button::new("Copy").min_size(Vec2::new(48.0, 20.0))).clicked() {
                                                    ui.ctx().copy_text(hash.clone());
                                                }
                                            } else if inflight {
                                                ui.label(RichText::new("Computing…").color(vc).size(10.0));
                                            } else {
                                                if ui.add(egui::Button::new("Compute").min_size(Vec2::new(62.0, 20.0))).clicked() {
                                                    self.request_info_hash(entry.path.clone());
                                                }
                                            }
                                        });
                                    });
                                    if let Some(err) = self.info_panel_hash_errors.get(&entry.path) {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new("Hash error").color(c.error).size(10.0));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(err).color(c.text_muted).size(10.0));
                                        });
                                    }
                                    if let Some(hash) = self.info_panel_hashes.get(&entry.path) {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.add_space(8.0);
                                            ui.label(RichText::new(hash).color(vc).size(10.0));
                                        });
                                    }
                                });
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
                            // Panel title bar
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(RichText::new(MI_MOVE_TO_INBOX).size(13.0).color(c.text_dim));
                                ui.add_space(4.0);
                                ui.label(RichText::new("Drop Folder").color(c.text_dim).size(12.0).strong());
                            });
                            ui.add_space(6.0);
                            ui.painter().hline(content_rect.x_range(), ui.cursor().min.y, Stroke::new(1.0, c.border));
                            ui.add_space(8.0);

                            let card_fill = if target_set {
                                Color32::from_rgba_unmultiplied(c.accent.r(), c.accent.g(), c.accent.b(), 16)
                            } else {
                                c.panel_raised
                            };
                            let card_stroke = if target_set {
                                Stroke::new(
                                    1.0,
                                    Color32::from_rgba_unmultiplied(c.accent.r(), c.accent.g(), c.accent.b(), 34),
                                )
                            } else {
                                Stroke::new(1.0, mix_color(c.border, c.smart_panel_bg, 0.65))
                            };
                            Frame::new()
                                .fill(card_fill)
                                .stroke(card_stroke)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    let path_str = self.state.config.target.current
                                        .as_ref()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|| "No drop folder set".to_string());
                                    let folder_name = std::path::Path::new(&path_str)
                                        .file_name().and_then(|n| n.to_str()).unwrap_or(&path_str);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(MI_MOVE_TO_INBOX).size(24.0).color(if target_set { c.accent } else { c.text_muted }));
                                        ui.vertical(|ui| {
                                            ui.add_space(2.0);
                                            ui.label(RichText::new(truncate_name(folder_name, 18))
                                                .color(if target_set { c.text } else { c.text_muted }).size(12.0).strong());
                                            if target_set {
                                                let parent = std::path::Path::new(&path_str)
                                                    .parent().map(|p| p.display().to_string())
                                                    .unwrap_or_default();
                                                ui.label(RichText::new(truncate_name(&parent, 21))
                                                    .color(c.text_muted).size(10.5))
                                                    .on_hover_text(&path_str);
                                            } else {
                                                ui.label(RichText::new("Choose a folder to quickly move or copy files to.")
                                                    .color(c.text_muted).size(10.5));
                                            }
                                        });
                                    });
                                    ui.add_space(8.0);
                                    let btn_label = if target_set {
                                        format!("{} Change Drop Folder…", MI_FOLDER_OPEN)
                                    } else {
                                        format!("{} Set Drop Folder…", MI_MOVE_TO_INBOX)
                                    };
                                    if ui.add(
                                        egui::Button::new(RichText::new(btn_label).size(12.0).color(c.text))
                                            .min_size(Vec2::new(ui.available_width(), 28.0))
                                            .corner_radius(4.0)
                                    ).clicked() {
                                        self.open_target_picker();
                                    }
                                    if target_set {
                                        ui.add_space(4.0);
                                        if ui.add(
                                            egui::Button::new(RichText::new(
                                                format!("{} Clear Drop Folder", MI_CLEAR)
                                            ).size(11.0).color(c.text_dim))
                                            .min_size(Vec2::new(ui.available_width(), 24.0))
                                            .corner_radius(4.0)
                                        ).clicked() {
                                            self.state.config.target.clear();
                                        }
                                    }
                                });
                            ui.add_space(8.0);

                            if self.target_dropdown.open {
                                let picker_fill = Color32::from_rgba_unmultiplied(
                                    c.panel_raised.r(),
                                    c.panel_raised.g(),
                                    c.panel_raised.b(),
                                    232,
                                );
                                Frame::new()
                                    .fill(picker_fill)
                                    .stroke(Stroke::new(1.0, mix_color(c.border, c.smart_panel_bg, 0.7)))
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(8, 8))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(MI_FOLDER_OPEN).size(13.0).color(c.text_dim));
                                            ui.label(RichText::new("Choose Drop Folder").color(c.text).size(11.5).strong());
                                        });
                                        ui.add_space(4.0);
                                        if let Some(dir) = self.target_dropdown.dir.clone() {
                                            ui.label(RichText::new(dir.display().to_string()).color(c.text_dim).size(10.5));
                                        }
                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            if ui.add_sized([66.0, 24.0], egui::Button::new("Up")).clicked()
                                                && let Some(parent) = self.target_dropdown.dir.clone().and_then(|d| d.parent().map(Path::to_path_buf))
                                            {
                                                self.enter_target_picker_dir(parent);
                                            }
                                            if ui.add_sized([84.0, 24.0], egui::Button::new("Cancel")).clicked() {
                                                self.target_dropdown.open = false;
                                            }
                                            if ui.add_sized([84.0, 24.0], egui::Button::new("Set")).clicked() {
                                                self.confirm_target_picker_current();
                                            }
                                        });
                                        ui.add_space(6.0);
                                        if let Some(err) = &self.target_dropdown.error {
                                            ui.label(RichText::new(err).color(c.error).size(10.5));
                                        } else {
                                            ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                                                let dirs = self.target_dropdown.entries.clone();
                                                for ent in dirs {
                                                    let row = ui.add(
                                                        egui::Button::new(
                                                            RichText::new(format!("{} {}", MI_FOLDER, truncate_name(&ent.name, 22)))
                                                                .size(11.5)
                                                                .color(c.text_dim)
                                                        )
                                                        .frame(false)
                                                        .min_size(Vec2::new(ui.available_width(), 24.0))
                                                    );
                                                    if row.hovered() {
                                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                    }
                                                    if row.clicked() {
                                                        self.enter_target_picker_dir(ent.path.clone());
                                                    }
                                                }
                                            });
                                        }
                                    });
                                ui.add_space(8.0);
                            }

                            let pinned_paths = self.state.config.target.pinned.clone();
                            let recents = self.state.config.target.recent.clone();
                            let mut chosen_target: Option<PathBuf> = None;
                            let mut pin_from_recent: Option<PathBuf> = None;
                            let mut unpin_path: Option<PathBuf> = None;
                            let mut move_pin_up: Option<usize> = None;
                            let mut move_pin_down: Option<usize> = None;

                            if !pinned_paths.is_empty() {
                                ui.add_space(8.0);
                                Frame::new()
                                    .fill(c.panel_raised)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(8, 8))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Pinned Drop Folders").color(c.text_muted).size(11.0).strong());
                                        ui.add_space(6.0);
                                        for (idx, path) in pinned_paths.iter().enumerate() {
                                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("…");
                                            let is_current = self.state.config.target.current.as_ref() == Some(path);
                                            let row_fill = if idx % 2 == 0 { c.panel } else { c.panel_raised };
                                            Frame::new()
                                                .fill(row_fill)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(4.0)
                                                .inner_margin(egui::Margin::symmetric(4, 4))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        let label = if is_current {
                                                            format!("{} {}", MI_CHECK, truncate_name(name, 14))
                                                        } else {
                                                            format!("{} {}", MI_BOOKMARK, truncate_name(name, 14))
                                                        };
                                                        let resp = ui.add(
                                                            egui::Button::new(
                                                                RichText::new(label).size(12.0).color(if is_current { c.accent } else { c.text })
                                                            )
                                                            .frame(false)
                                                            .min_size(Vec2::new((ui.available_width() - 86.0).max(90.0), 24.0))
                                                        );
                                                        if resp.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if resp.clicked() {
                                                            chosen_target = Some(path.clone());
                                                        }
                                                        resp.on_hover_text(path.display().to_string());

                                                        let up = ui.add_sized([22.0, 22.0], egui::Button::new("↑"));
                                                        if up.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if up.clicked() {
                                                            move_pin_up = Some(idx);
                                                        }
                                                        let down = ui.add_sized([22.0, 22.0], egui::Button::new("↓"));
                                                        if down.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if down.clicked() {
                                                            move_pin_down = Some(idx);
                                                        }
                                                        let unpin = ui.add_sized([28.0, 22.0], egui::Button::new(MI_CLEAR));
                                                        if unpin.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if unpin.clicked() {
                                                            unpin_path = Some(path.clone());
                                                        }
                                                    });
                                                });
                                            ui.add_space(4.0);
                                        }
                                    });
                            }

                            let recent_paths: Vec<PathBuf> = recents
                                .into_iter()
                                .filter(|p| !pinned_paths.iter().any(|pin| pin == p))
                                .collect();
                            if !recent_paths.is_empty() {
                                ui.add_space(8.0);
                                Frame::new()
                                    .fill(c.panel_raised)
                                    .stroke(Stroke::NONE)
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(8, 8))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("Recent Drop Folders").color(c.text_muted).size(11.0).strong());
                                        ui.add_space(6.0);
                                        for (idx, path) in recent_paths.iter().enumerate() {
                                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("…");
                                            let is_current = self.state.config.target.current.as_ref() == Some(path);
                                            let row_fill = if idx % 2 == 0 { c.panel } else { c.panel_raised };
                                            Frame::new()
                                                .fill(row_fill)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(4.0)
                                                .inner_margin(egui::Margin::symmetric(4, 4))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        let resp = ui.add(
                                                            egui::Button::new(
                                                                RichText::new(format!("{} {}", if is_current { MI_CHECK } else { MI_FOLDER }, truncate_name(name, 16)))
                                                                    .size(12.0).color(if is_current { c.accent } else { c.text })
                                                            )
                                                            .frame(false)
                                                            .min_size(Vec2::new((ui.available_width() - 34.0).max(90.0), 24.0))
                                                        );
                                                        if resp.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if resp.clicked() {
                                                            chosen_target = Some(path.clone());
                                                        }
                                                        resp.on_hover_text(path.display().to_string());

                                                        let pin = ui.add_sized([28.0, 22.0], egui::Button::new(MI_BOOKMARK));
                                                        if pin.hovered() {
                                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                        }
                                                        if pin.clicked() {
                                                            pin_from_recent = Some(path.clone());
                                                        }
                                                    });
                                                });
                                            ui.add_space(4.0);
                                        }
                                    });
                            }

                            if let Some(idx) = move_pin_up {
                                self.state.config.target.move_pin_up(idx);
                            }
                            if let Some(idx) = move_pin_down {
                                self.state.config.target.move_pin_down(idx);
                            }
                            if let Some(path) = unpin_path {
                                self.state.config.target.unpin(&path);
                            }
                            if let Some(path) = pin_from_recent {
                                self.state.config.target.pin(path);
                            }
                            if let Some(path) = chosen_target {
                                self.set_drop_folder(path, true, false);
                            }
                        }
                        SidebarSection::Search => {
                            self.render_search_sidebar(ui);
                        }
                    }
                });
            });
    }

    fn render_status_bar(&mut self, ctx: &Context) {
        let c = self.colors;
        let bottom_bar_bg = mix_color(c.toolbar_bg, c.bg, 0.42);
        let bottom_text = mix_color(c.text_muted, bottom_bar_bg, 0.38);
        let bottom_dim = mix_color(c.text_dim, bottom_bar_bg, 0.48);
        if let Some(st) = &self.privileged_status {
            let ttl = match st.kind {
                PrivilegedUiKind::InProgress => Duration::from_secs(30),
                PrivilegedUiKind::Success => Duration::from_secs(8),
                PrivilegedUiKind::Error => Duration::from_secs(12),
            };
            if st.since.elapsed() > ttl {
                self.privileged_status = None;
            }
        }
        if let Some(msg) = &self.status_message
            && Instant::now() > msg.until
        {
            self.status_message = None;
        }
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(STATUS_BAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(bottom_bar_bg)
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| {
                ui.set_height(STATUS_BAR_HEIGHT);
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.max_rect().top(),
                    Stroke::new(1.0, mix_color(c.border, bottom_bar_bg, 0.5)),
                );
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    // Command frame toggle — LEFT side (bar opens at the bottom-left)
                    let cf_color = if self.command_frame.visible {
                        c.accent_dim
                    } else {
                        bottom_text
                    };
                    let cf_resp = ui.add(
                        egui::Button::new(RichText::new(MI_TERMINAL).size(16.0).color(cf_color))
                            .frame(false)
                            .min_size(Vec2::splat(24.0)),
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
                    let count = self
                        .tab_ui
                        .get(&tab_id)
                        .map(|s| s.main.entries.len())
                        .unwrap_or(0);
                    let sel_name = self
                        .tab_ui
                        .get(&tab_id)
                        .and_then(|s| s.main_sel)
                        .and_then(|i| self.tab_ui.get(&tab_id)?.main.entries.get(i))
                        .map(|e| e.name.clone());
                    let status = if let Some(name) = sel_name {
                        format!("{} items  ·  {}", count, name)
                    } else {
                        format!("{} items", count)
                    };
                    ui.label(RichText::new(status).color(bottom_text).size(11.5));
                    if let Some(msg) = &self.status_message {
                        let col = if msg.ok { c.accent } else { c.error };
                        ui.label(
                            RichText::new(format!("· {}", msg.text))
                                .color(col)
                                .size(11.0),
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);
                        // Privileged capability + latest operation status
                        match &self.privileged_status {
                            Some(st) => {
                                let (icon, col) = match st.kind {
                                    PrivilegedUiKind::InProgress => (MI_LOCK, bottom_dim),
                                    PrivilegedUiKind::Success => (MI_CHECK, c.accent),
                                    PrivilegedUiKind::Error => (MI_LOCK, c.error),
                                };
                                let resp = ui.add(
                                    egui::Button::new(RichText::new(icon).size(14.0).color(col))
                                        .frame(false)
                                        .min_size(Vec2::splat(22.0)),
                                );
                                resp.on_hover_text(&st.text);
                            }
                            None => {
                                let (icon, col, tip) = match &self.privileged_availability {
                                    PrivilegedAvailability::Ready => (
                                        MI_LOCK,
                                        bottom_text,
                                        "Integrated privilege management ready",
                                    ),
                                    PrivilegedAvailability::Misconfigured(msg) => {
                                        (MI_LOCK, c.error, msg.as_str())
                                    }
                                    PrivilegedAvailability::Unsupported(msg) => {
                                        (MI_LOCK, bottom_text, msg.as_str())
                                    }
                                };
                                let resp = ui.add(
                                    egui::Button::new(RichText::new(icon).size(13.0).color(col))
                                        .frame(false)
                                        .min_size(Vec2::splat(22.0)),
                                );
                                resp.on_hover_text(tip);
                            }
                        }
                        ui.add_space(2.0);
                        let search_diag = self.search_service.diagnostics();
                        let s_col = match search_diag.status {
                            SearchIndexStatus::Unavailable => c.error,
                            _ => bottom_text,
                        };
                        let s_resp = ui.add(
                            egui::Button::new(RichText::new(MI_SEARCH).size(13.0).color(s_col))
                                .frame(false)
                                .min_size(Vec2::splat(22.0)),
                        );
                        s_resp.on_hover_text(search_diagnostics_tooltip(
                            &search_diag,
                            self.search_ui.scope,
                            &self.state.active_tab().current_dir,
                        ));
                        ui.add_space(2.0);
                        // Hidden files toggle — right side
                        let (hidden_icon, hidden_color) = if self.state.config.show_hidden_files {
                            (MI_VISIBILITY, c.accent)
                        } else {
                            (MI_VISIBILITY_OFF, bottom_text)
                        };
                        let hid_resp = ui.add(
                            egui::Button::new(
                                RichText::new(hidden_icon).size(16.0).color(hidden_color),
                            )
                            .frame(false)
                            .min_size(Vec2::splat(24.0)),
                        );
                        if hid_resp.clicked() {
                            self.state.config.show_hidden_files =
                                !self.state.config.show_hidden_files;
                            self.refresh_active_tab();
                        }
                        hid_resp.on_hover_text(if self.state.config.show_hidden_files {
                            "Hide hidden files"
                        } else {
                            "Show hidden files"
                        });
                    });
                });

                // Subtle centered wordmark in the status bar.
                let logo_size = Vec2::new(82.0, 18.0);
                let logo_rect = Rect::from_center_size(ui.max_rect().center(), logo_size);
                ui.put(
                    logo_rect,
                    egui::Image::new(egui::include_image!("../../../assets/logo.svg"))
                        .fit_to_exact_size(logo_size)
                        .tint(c.border),
                );
            });
    }

    fn render_search_sidebar(&mut self, ui: &mut egui::Ui) {
        let c = self.colors;

        // ── Title heading ─────────────────────────────────────────────────
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(RichText::new("Search").color(c.text).size(18.0).strong());
        });
        ui.add_space(12.0);

        // ── Search input — visually bordered, clearly an input ──────────
        let border_col = if self.search_ui.input_focused {
            c.accent
        } else {
            c.border
        };
        let border_w = if self.search_ui.input_focused {
            1.5
        } else {
            1.0
        };
        Frame::new()
            .fill(c.panel_raised)
            .stroke(Stroke::new(border_w, border_col))
            .corner_radius(8.0)
            .inner_margin(egui::Margin {
                left: 10,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let icon_col = if self.search_ui.input_focused {
                        c.accent
                    } else {
                        c.text_muted
                    };
                    ui.label(RichText::new(MI_SEARCH).size(15.0).color(icon_col));
                    ui.add_space(6.0);
                    let resp = ui.add_sized(
                        [ui.available_width(), 20.0],
                        egui::TextEdit::singleline(&mut self.search_ui.query)
                            .id_source("search_input_field")
                            .frame(false)
                            .hint_text("Search files and folders…")
                            .font(FontId::proportional(13.0)),
                    );
                    if self.search_ui.request_focus {
                        resp.request_focus();
                        self.search_ui.request_focus = false;
                    }
                    self.search_ui.input_focused = resp.has_focus();
                    if resp.changed() {
                        self.run_search_query();
                    }
                });
            });

        ui.add_space(8.0);

        // ── Scope + mode pills ────────────────────────────────────────────
        ui.horizontal(|ui| {
            let global = matches!(self.search_ui.scope, SearchScope::Global);
            for (label, is_active, scope) in [
                ("Global", global, SearchScope::Global),
                ("This folder", !global, SearchScope::CurrentFolder),
            ] {
                let fill = if is_active {
                    c.selected_bg
                } else {
                    Color32::TRANSPARENT
                };
                let txt = if is_active { c.accent } else { c.text_muted };
                let btn = ui.add(
                    egui::Button::new(RichText::new(label).size(12.0).color(txt))
                        .fill(fill)
                        .stroke(Stroke::NONE)
                        .corner_radius(16.0),
                );
                if btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if btn.clicked() {
                    self.search_ui.scope = scope;
                    self.run_search_query();
                }
            }

            // Content search toggle pill
            ui.add_space(4.0);
            {
                let is_content = self.search_ui.content_search;
                let fill = if is_content {
                    c.selected_bg
                } else {
                    Color32::TRANSPARENT
                };
                let txt = if is_content { c.accent } else { c.text_muted };
                let btn = ui
                    .add(
                        egui::Button::new(RichText::new("Content").size(12.0).color(txt))
                            .fill(fill)
                            .stroke(Stroke::new(
                                1.0,
                                if is_content { c.accent } else { c.border },
                            ))
                            .corner_radius(16.0),
                    )
                    .on_hover_text("Search inside file contents using ripgrep");
                if btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if btn.clicked() {
                    self.search_ui.content_search = !is_content;
                    self.run_search_query();
                }
            }

            // Settings gear — far right, subtle
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                let gear = ui.add(
                    egui::Label::new(RichText::new(MI_SETTINGS).size(13.0).color(c.text_muted))
                        .sense(Sense::click()),
                );
                gear.clone().on_hover_text("Search settings");
                if gear.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if gear.clicked() {
                    self.show_settings = true;
                    self.settings_tab = SettingsTab::Search;
                }
            });
        });

        // Separator below scope pills
        ui.add_space(6.0);
        ui.painter().hline(
            ui.cursor().min.x..=(ui.cursor().min.x + ui.available_width()),
            ui.cursor().min.y,
            Stroke::new(1.0, c.border),
        );
        ui.add_space(8.0);

        // ── Body: empty state or results ──────────────────────────────────
        let results_height = ui.available_height().max(120.0);
        let diag = self.search_service.diagnostics();

        // Live result refresh: while indexing, re-run query whenever a new batch
        // arrives so results grow in real-time without needing user input.
        if matches!(diag.status, SearchIndexStatus::Indexing) && !self.search_ui.query.is_empty() {
            if diag.indexed_items != self.search_ui.last_seen_index_count {
                self.search_ui.last_seen_index_count = diag.indexed_items;
                self.run_search_query();
            }
            // Keep repainting so we poll for the next batch.
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        } else {
            self.search_ui.last_seen_index_count = diag.indexed_items;
        }

        if self.search_ui.query.is_empty() {
            // Empty state — centered, welcoming, informative
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), results_height),
                Layout::top_down(Align::Center),
                |ui| {
                    ui.add_space(results_height * 0.18);
                    ui.label(
                        RichText::new("Start typing to search")
                            .color(c.text)
                            .size(14.0)
                            .strong(),
                    );
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("Search across all indexed locations")
                            .color(c.text_muted)
                            .size(11.5),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Use multiple words to narrow results")
                            .color(c.text_muted)
                            .size(11.5),
                    );
                    // Indexing status near the bottom
                    ui.add_space(results_height * 0.16);
                    match diag.status {
                        SearchIndexStatus::Indexing => {
                            ui.ctx().request_repaint();
                            // Estimate progress: multi-root uses root index, single-root
                            // uses a log scale (home folders are typically 100k–500k files).
                            let pct = if diag.total_roots > 1 {
                                let done = (diag.active_root_index.saturating_sub(1)) as f32;
                                ((done / diag.total_roots as f32) * 100.0) as u32
                            } else {
                                // log2(count+1) / log2(300_001) maps 0→0%, 300k→100%
                                let n = diag.indexed_items as f32 + 1.0;
                                let ratio = n.log2() / 300_001_f32.log2();
                                (ratio.clamp(0.0, 0.99) * 100.0) as u32
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Indexing files… ({}%)", pct))
                                        .color(c.text_muted)
                                        .size(11.0),
                                );
                                ui.add(egui::Spinner::new().size(12.0).color(c.accent));
                            });
                        }
                        SearchIndexStatus::Ready => {
                            ui.label(
                                RichText::new(format!("{} files indexed", diag.indexed_items))
                                    .color(c.text_muted)
                                    .size(10.5),
                            );
                        }
                        SearchIndexStatus::Unavailable => {
                            ui.label(
                                RichText::new("Search unavailable")
                                    .color(c.error)
                                    .size(11.0),
                            );
                        }
                    }
                },
            );
            return;
        }

        // ── Results header (count + error) ───────────────────────────────
        {
            let n = self.search_ui.total;
            ui.horizontal(|ui| {
                let count_label = if n == 1 {
                    "1 result".to_string()
                } else {
                    format!("{n} results")
                };
                ui.label(RichText::new(count_label).color(c.text_muted).size(11.0));
                if matches!(diag.status, SearchIndexStatus::Indexing) {
                    ui.ctx().request_repaint();
                    let pct = if diag.total_roots > 1 {
                        let done = (diag.active_root_index.saturating_sub(1)) as f32;
                        ((done / diag.total_roots as f32) * 100.0) as u32
                    } else {
                        let n = diag.indexed_items as f32 + 1.0;
                        ((n.log2() / 300_001_f32.log2()).clamp(0.0, 0.99) * 100.0) as u32
                    };
                    ui.label(
                        RichText::new(format!("· indexing {}%", pct))
                            .color(c.accent)
                            .size(10.5),
                    );
                    ui.add(egui::Spinner::new().size(10.0).color(c.accent));
                }
            });
            if let Some(err) = &self.search_ui.last_error {
                ui.label(RichText::new(err).color(c.error).size(10.5));
            }
            ui.add_space(2.0);
        }

        {
            // Reserve some height for the preview pane below the results.
            let has_preview = self.search_ui.selected.is_some()
                && self
                    .search_ui
                    .results
                    .get(self.search_ui.selected.unwrap_or(0))
                    .map(|it| matches!(it.kind, EntryKind::File))
                    .unwrap_or(false);
            let preview_panel_h = if has_preview {
                (results_height * 0.35).clamp(90.0, 180.0)
            } else {
                0.0
            };
            let table_h =
                (results_height - preview_panel_h - if has_preview { 10.0 } else { 0.0 }).max(80.0);

            let table_w = ui.available_width().max(320.0);
            let modified_w = 96.0;
            let size_w = 78.0;
            let fixed_w = modified_w + size_w + 18.0;
            let flex_total = (table_w - fixed_w).max(180.0);
            let mut name_w = (flex_total * 0.37).clamp(90.0, 300.0);
            let mut folder_w = (flex_total - name_w).clamp(90.0, 460.0);
            let used = name_w + folder_w;
            if used > flex_total {
                let scale = (flex_total / used).clamp(0.5, 1.0);
                name_w *= scale;
                folder_w *= scale;
            }

            let mut pending_op: Option<FileCommand> = None;
            let mut open_parent: Option<PathBuf> = None;
            let mut open_entry_now: Option<(PathBuf, EntryKind)> = None;
            let mut open_tandem_now: Option<PathBuf> = None;
            let mut queued_status: Option<&'static str> = None;
            let cur_dir = self.state.active_tab().current_dir.clone();
            let target_dir = self.state.config.target.current.clone();
            let tandem_target_dir = self
                .tandem_transfer_destination(self.state.active_tab().id)
                .map(|(_, dir)| dir);
            let mut clicked_sort: Option<SearchSort> = None;

            let mut table = TableBuilder::new(ui)
                .id_salt("search_results_table")
                .striped(true)
                .resizable(false)
                .vscroll(true)
                .scroll_bar_visibility(ScrollBarVisibility::VisibleWhenNeeded)
                .max_scroll_height(table_h - 18.0)
                .min_scrolled_height((table_h - 18.0).max(80.0))
                .column(Column::exact(name_w).clip(true))
                .column(Column::exact(folder_w).clip(true))
                .column(Column::exact(modified_w).clip(true))
                .column(Column::exact(size_w).clip(true));
            if self.search_ui.scroll_to_selected
                && let Some(row) = self.search_ui.selected
            {
                table = table.scroll_to_row(row, Some(egui::Align::Center));
            }

            table
                .header(22.0, |mut header| {
                    let mut header_col =
                        |header: &mut egui_extras::TableRow<'_, '_>,
                         label: &str,
                         sort: SearchSort,
                         right_align: bool| {
                            header.col(|ui| {
                                let active = self.search_ui.sort == sort;
                                let text = if active {
                                    format!("{label} ▲")
                                } else {
                                    label.to_string()
                                };
                                let rich = RichText::new(text).size(10.5).color(if active {
                                    c.text
                                } else {
                                    c.text_muted
                                });
                                let resp = if right_align {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add(egui::Label::new(rich).sense(Sense::click()))
                                    })
                                    .inner
                                } else {
                                    ui.add(egui::Label::new(rich).sense(Sense::click()))
                                };
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() && self.search_ui.sort != sort {
                                    clicked_sort = Some(sort);
                                }
                            });
                        };
                    header_col(&mut header, "Name", SearchSort::Name, false);
                    header_col(&mut header, "Folder", SearchSort::Path, false);
                    header_col(&mut header, "Modified", SearchSort::Modified, false);
                    header_col(&mut header, "Size", SearchSort::Size, true);
                })
                .body(|mut body| {
                    for (i, item) in self.search_ui.results.iter().enumerate() {
                        let selected = self.search_ui.selected == Some(i);
                        body.row(24.0, |mut row| {
                            row.set_selected(selected);
                            let style = style_for_search_item(
                                item,
                                EntryVisualState {
                                    selected,
                                    focused: true,
                                    hovered: false,
                                    opened: false,
                                    symlink_dir: matches!(item.kind, EntryKind::Symlink)
                                        && item.symlink_target_is_dir == Some(true),
                                },
                                &c,
                                &self.state.config,
                            );
                            let icon = material_icon(style.icon);
                            let name_cap = ((name_w - 28.0) / 7.0).max(8.0) as usize;
                            let folder_cap = ((folder_w - 12.0) / 6.4).max(10.0) as usize;
                            let modified_text = item
                                .modified_unix_secs
                                .map(format_modified)
                                .unwrap_or_else(|| "—".to_string());
                            let size_text = item
                                .size_bytes
                                .map(format_size)
                                .unwrap_or_else(|| "—".to_string());

                            row.col(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(icon).size(11.5).color(style.icon_color),
                                    );
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(truncate_name(&item.name, name_cap))
                                                .size(11.5)
                                                .color(style.text_color),
                                        );
                                        // Show content match snippet inline when available
                                        if let Some(snip) = &item.content_snippet {
                                            let snip_cap =
                                                ((name_w - 16.0) / 5.5).max(10.0) as usize;
                                            ui.label(
                                                RichText::new(truncate_name(snip, snip_cap))
                                                    .size(9.5)
                                                    .color(c.text_muted)
                                                    .italics(),
                                            );
                                        }
                                    });
                                });
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(truncate_name(
                                        &item.parent_path.display().to_string(),
                                        folder_cap,
                                    ))
                                    .size(10.5)
                                    .color(c.text_muted),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(modified_text).size(10.5).color(c.text_muted),
                                );
                            });
                            row.col(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(size_text).size(10.5).color(c.text_muted),
                                    );
                                });
                            });

                            let row_resp = row.response();
                            if row_resp.clicked() {
                                self.search_ui.selected = Some(i);
                                self.search_ui.scroll_to_selected = true;
                            }
                            if row_resp.double_clicked() {
                                open_entry_now = Some((item.path.clone(), item.kind));
                            }

                            row_resp.context_menu(|ui| {
                                ui.visuals_mut().override_text_color = Some(c.text_muted);
                                let path = item.path.clone();
                                let parent = item.parent_path.clone();
                                let same_as_current = parent == cur_dir;
                                let target_missing = target_dir.is_none();
                                let same_as_target = target_dir.as_ref() == Some(&parent);
                                let same_as_tandem =
                                    tandem_target_dir.as_deref() == Some(parent.as_path());

                                if ui
                                    .button(RichText::new("Open").color(c.text_muted))
                                    .clicked()
                                {
                                    open_entry_now = Some((path.clone(), item.kind));
                                    ui.close();
                                }
                                if ui
                                    .button(
                                        RichText::new("Open Folder Location").color(c.text_muted),
                                    )
                                    .clicked()
                                {
                                    open_parent = Some(parent.clone());
                                    ui.close();
                                }
                                if matches!(item.kind, EntryKind::Directory | EntryKind::Symlink)
                                    && ui
                                        .button(RichText::new("Open in Tandem").color(c.text_muted))
                                        .clicked()
                                {
                                    open_tandem_now = Some(path.clone());
                                    ui.close();
                                }
                                if ui
                                    .button(RichText::new("Copy File Path").color(c.text_muted))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(path.display().to_string());
                                    ui.close();
                                }
                                ui.separator();

                                let copy_target_btn = ui
                                    .add_enabled(
                                        !target_missing && !same_as_target,
                                        egui::Button::new(
                                            RichText::new("Copy to Drop Folder")
                                                .color(c.text_muted),
                                        ),
                                    )
                                    .on_hover_text(if target_missing {
                                        "No drop folder set"
                                    } else if same_as_target {
                                        "Already in destination"
                                    } else {
                                        ""
                                    });
                                if copy_target_btn.clicked() {
                                    if let Some(dst) = target_dir.clone() {
                                        pending_op = Some(FileCommand::Copy {
                                            sources: vec![path.clone()],
                                            destination: dst,
                                            conflict: ConflictAction::Rename,
                                        });
                                    }
                                    ui.close();
                                }
                                let move_target_btn = ui
                                    .add_enabled(
                                        !target_missing && !same_as_target,
                                        egui::Button::new(
                                            RichText::new("Move to Drop Folder")
                                                .color(c.text_muted),
                                        ),
                                    )
                                    .on_hover_text(if target_missing {
                                        "No drop folder set"
                                    } else if same_as_target {
                                        "Already in destination"
                                    } else {
                                        ""
                                    });
                                if move_target_btn.clicked() {
                                    if let Some(dst) = target_dir.clone() {
                                        pending_op = Some(FileCommand::Move {
                                            sources: vec![path.clone()],
                                            destination: dst,
                                            conflict: ConflictAction::Rename,
                                        });
                                    }
                                    ui.close();
                                }

                                let copy_cur_btn = ui
                                    .add_enabled(
                                        !same_as_current,
                                        egui::Button::new(
                                            RichText::new("Copy to Current Folder")
                                                .color(c.text_muted),
                                        ),
                                    )
                                    .on_hover_text(if same_as_current {
                                        "Already in destination"
                                    } else {
                                        ""
                                    });
                                if copy_cur_btn.clicked() {
                                    pending_op = Some(FileCommand::Copy {
                                        sources: vec![path.clone()],
                                        destination: cur_dir.clone(),
                                        conflict: ConflictAction::Rename,
                                    });
                                    ui.close();
                                }
                                let move_cur_btn = ui
                                    .add_enabled(
                                        !same_as_current,
                                        egui::Button::new(
                                            RichText::new("Move to Current Folder")
                                                .color(c.text_muted),
                                        ),
                                    )
                                    .on_hover_text(if same_as_current {
                                        "Already in destination"
                                    } else {
                                        ""
                                    });
                                if move_cur_btn.clicked() {
                                    pending_op = Some(FileCommand::Move {
                                        sources: vec![path.clone()],
                                        destination: cur_dir.clone(),
                                        conflict: ConflictAction::Rename,
                                    });
                                    ui.close();
                                }

                                if tandem_target_dir.is_some() {
                                    ui.separator();
                                    let copy_tandem_btn = ui
                                        .add_enabled(
                                            !same_as_tandem,
                                            egui::Button::new(
                                                RichText::new("Copy to Other Pane")
                                                    .color(c.text_muted),
                                            ),
                                        )
                                        .on_hover_text(if same_as_tandem {
                                            "Already in destination"
                                        } else {
                                            ""
                                        });
                                    if copy_tandem_btn.clicked() {
                                        if let Some(dst) = tandem_target_dir.clone() {
                                            pending_op = Some(FileCommand::Copy {
                                                sources: vec![path.clone()],
                                                destination: dst,
                                                conflict: ConflictAction::Rename,
                                            });
                                        }
                                        queued_status = Some("Queued copy to other pane");
                                        ui.close();
                                    }
                                    let move_tandem_btn = ui
                                        .add_enabled(
                                            !same_as_tandem,
                                            egui::Button::new(
                                                RichText::new("Move to Other Pane")
                                                    .color(c.text_muted),
                                            ),
                                        )
                                        .on_hover_text(if same_as_tandem {
                                            "Already in destination"
                                        } else {
                                            ""
                                        });
                                    if move_tandem_btn.clicked() {
                                        if let Some(dst) = tandem_target_dir.clone() {
                                            pending_op = Some(FileCommand::Move {
                                                sources: vec![path.clone()],
                                                destination: dst,
                                                conflict: ConflictAction::Rename,
                                            });
                                        }
                                        queued_status = Some("Queued move to other pane");
                                        ui.close();
                                    }
                                }
                            });
                        });
                    }
                });
            if let Some(sort) = clicked_sort {
                self.search_ui.sort = sort;
                self.run_search_query();
            }
            self.search_ui.scroll_to_selected = false;
            if let Some(msg) = queued_status {
                self.set_status_message(msg, true, 3);
            }
            if let Some(cmd) = pending_op {
                let tab_id = self.state.active_tab().id;
                self.run_file_op(cmd, Some(tab_id));
            }
            if let Some(p) = open_parent {
                let tab_id = self.state.active_tab().id;
                self.navigate_tab_to(tab_id, p);
            }
            if let Some((p, k)) = open_entry_now {
                self.open_entry(p, k);
            }
            if let Some(path) = open_tandem_now {
                self.open_directory_in_tandem(path);
            }

            // ── Preview pane for selected result ─────────────────────────────
            if has_preview
                && let Some(sel) = self.search_ui.selected
                && let Some(item) = self.search_ui.results.get(sel).cloned()
            {
                // Load/cache preview
                let preview = if self
                    .search_ui
                    .result_preview
                    .as_ref()
                    .map(|(p, _)| p == &item.path)
                    .unwrap_or(false)
                {
                    self.search_ui
                        .result_preview
                        .as_ref()
                        .map(|(_, p)| p.clone())
                } else {
                    let loaded = load_preview(&PreviewRequest {
                        path: item.path.clone(),
                        kind_hint: item.kind,
                    });
                    self.search_ui.result_preview = Some((item.path.clone(), loaded.clone()));
                    Some(loaded)
                };

                ui.add_space(4.0);
                ui.painter().hline(
                    ui.cursor().min.x..=(ui.cursor().min.x + ui.available_width()),
                    ui.cursor().min.y,
                    Stroke::new(1.0, c.border),
                );
                ui.add_space(6.0);

                Frame::new()
                    .fill(c.panel_raised)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_min_height(preview_panel_h - 18.0);
                        ui.set_max_height(preview_panel_h - 18.0);
                        ui.horizontal(|ui| {
                            let style = style_for_search_item(
                                &item,
                                EntryVisualState {
                                    selected: true,
                                    focused: false,
                                    hovered: false,
                                    opened: false,
                                    symlink_dir: matches!(item.kind, EntryKind::Symlink)
                                        && item.symlink_target_is_dir == Some(true),
                                },
                                &c,
                                &self.state.config,
                            );
                            ui.label(
                                RichText::new(material_icon(style.icon))
                                    .size(14.0)
                                    .color(style.icon_color),
                            );
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&item.name).size(12.0).color(c.text).strong(),
                                );
                                if let Some(sz) = item.size_bytes {
                                    ui.label(
                                        RichText::new(format_size(sz))
                                            .size(10.5)
                                            .color(c.text_muted),
                                    );
                                }
                            });
                        });
                        ui.add_space(4.0);
                        // Text preview snippet or image indicator
                        if let Some(pv) = preview {
                            match pv.kind {
                                PreviewKind::Text => {
                                    let snippet: String =
                                        pv.body.lines().take(5).collect::<Vec<_>>().join("\n");
                                    ScrollArea::vertical()
                                        .id_salt("search_result_preview_text")
                                        .max_height(preview_panel_h - 70.0)
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(snippet)
                                                    .size(10.5)
                                                    .color(c.text_muted)
                                                    .font(FontId::monospace(10.5)),
                                            );
                                        });
                                }
                                PreviewKind::Image => {
                                    let avail_w = ui.available_width().max(60.0);
                                    let max_h = preview_panel_h - 70.0;
                                    if let Some(img) = self
                                        .cached_image_preview_widget(&item.path, avail_w, max_h, 4)
                                    {
                                        ui.add(img);
                                    } else {
                                        ui.label(
                                            RichText::new("Image").size(10.5).color(c.text_muted),
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
            }
        }
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
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                // Error or informational message
                if let Some(err) = &self.command_frame.error.clone() {
                    ui.label(RichText::new(err).color(c.error).size(11.5));
                    if let Some((cmd, tab_id)) = self.pending_privileged_retry.clone() {
                        ui.add_space(4.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Retry As Administrator…").size(11.5),
                                )
                                .corner_radius(5.0),
                            )
                            .clicked()
                        {
                            match self.retry_file_op_with_privileges(cmd, tab_id) {
                                Ok(()) => {
                                    self.pending_privileged_retry = None;
                                    self.command_frame.error = None;
                                    self.command_frame.message = Some(
                                        "Operation completed with administrator privileges."
                                            .to_string(),
                                    );
                                }
                                Err(e) => {
                                    self.command_frame.error = Some(e);
                                }
                            }
                        }
                    }
                } else if let Some(msg) = &self.command_frame.message.clone() {
                    ui.label(RichText::new(msg).color(c.accent).size(11.5));
                }

                // Completions hint (shown after Tab press)
                if !self.command_frame.completions.is_empty() {
                    let hint = self
                        .command_frame
                        .completions
                        .iter()
                        .take(6)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("   ");
                    ui.label(
                        RichText::new(format!("Tab>  {}", hint))
                            .color(c.text_muted)
                            .size(11.0),
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
                            .hint_text("type a command — or 'help' for a list"),
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
                            // Completions are full paths — replace the path
                            // argument while keeping the command word.
                            let parts: Vec<&str> =
                                self.command_frame.input.trim().splitn(2, ' ').collect();
                            let cmd_word = parts.first().copied().unwrap_or("").to_string();
                            self.command_frame.input = format!("{} {}", cmd_word, comp);
                            // Refresh list for the updated input
                            self.update_completions();
                        }
                        self.command_frame.request_focus = true;
                        // Move the TextEdit cursor to end of the completed string so the user
                        // can continue typing from the end rather than the old cursor position.
                        let char_count = self.command_frame.input.chars().count();
                        let cursor = CCursor::new(char_count);
                        let range = CCursorRange::one(cursor);
                        let mut te_state =
                            egui::text_edit::TextEditState::load(ui.ctx(), cmd_input_id)
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
                        let (up, down) = ctx.input(|i| {
                            (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown))
                        });
                        if up && !self.command_frame.history.is_empty() {
                            let max = self.command_frame.history.len() - 1;
                            let idx = self
                                .command_frame
                                .history_idx
                                .map(|i| i.saturating_sub(1))
                                .unwrap_or(max);
                            self.command_frame.history_idx = Some(idx);
                            self.command_frame.input = self.command_frame.history[idx].clone();
                        }
                        if down && let Some(idx) = self.command_frame.history_idx {
                            if idx + 1 < self.command_frame.history.len() {
                                let next = idx + 1;
                                self.command_frame.history_idx = Some(next);
                                self.command_frame.input = self.command_frame.history[next].clone();
                            } else {
                                self.command_frame.history_idx = None;
                                self.command_frame.input.clear();
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

    #[allow(clippy::too_many_arguments)]
    fn entry_context_menu(
        &mut self,
        response: &egui::Response,
        entry: &FileEntry,
        current_dir: &Path,
        pending_op: &mut Option<FileCommand>,
        open_parent: &mut Option<PathBuf>,
        open_entry: &mut Option<(PathBuf, EntryKind)>,
        open_tandem: &mut Option<PathBuf>,
        add_bookmark: &mut Option<PathBuf>,
        remove_bookmark: &mut Option<PathBuf>,
    ) {
        let path = entry.path.clone();
        let kind = entry.kind;
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| current_dir.to_path_buf());
        let target_dir = self.state.config.target.current.clone();
        let target_missing = target_dir.is_none();
        let same_as_target = target_dir.as_ref() == Some(&parent);
        let same_as_current = parent == current_dir;
        let is_dir_like = matches!(kind, EntryKind::Directory | EntryKind::Symlink);
        let is_bookmarked = self.is_bookmarked(&path);
        let tandem_target_dir = self
            .tandem_transfer_destination(self.state.active_tab().id)
            .map(|(_, dir)| dir);
        let same_as_tandem = tandem_target_dir.as_deref() == Some(current_dir);

        response.context_menu(|ui| {
            ui.visuals_mut().override_text_color = Some(self.colors.text_muted);
            let menu_col = self.colors.text_muted;
            if ui.button(RichText::new("Open").color(menu_col)).clicked() {
                *open_entry = Some((path.clone(), kind));
                ui.close();
            }
            if ui
                .button(RichText::new("Open Folder Location").color(menu_col))
                .clicked()
            {
                *open_parent = Some(parent.clone());
                ui.close();
            }
            if is_dir_like
                && ui
                    .button(RichText::new("Open in Tandem").color(menu_col))
                    .clicked()
            {
                *open_tandem = Some(path.clone());
                ui.close();
            }
            if ui
                .button(RichText::new("Copy File Path").color(menu_col))
                .clicked()
            {
                ui.ctx().copy_text(path.display().to_string());
                self.set_status_message("Copied file path", true, 3);
                ui.close();
            }
            if is_dir_like {
                if !is_bookmarked {
                    if ui
                        .button(RichText::new("Add to Shortcuts").color(menu_col))
                        .clicked()
                    {
                        *add_bookmark = Some(path.clone());
                        self.set_status_message("Added to shortcuts", true, 3);
                        ui.close();
                    }
                } else if ui
                    .button(RichText::new("Remove from Shortcuts").color(menu_col))
                    .clicked()
                {
                    *remove_bookmark = Some(path.clone());
                    self.set_status_message("Removed from shortcuts", true, 3);
                    ui.close();
                }
            }
            ui.separator();

            let copy_target_btn = ui
                .add_enabled(
                    !target_missing && !same_as_target,
                    egui::Button::new(RichText::new("Copy to Drop Folder").color(menu_col)),
                )
                .on_hover_text(if target_missing {
                    "No drop folder set"
                } else if same_as_target {
                    "Already in destination"
                } else {
                    ""
                });
            if copy_target_btn.clicked() {
                if let Some(dst) = target_dir.clone() {
                    *pending_op = Some(FileCommand::Copy {
                        sources: vec![path.clone()],
                        destination: dst,
                        conflict: ConflictAction::Rename,
                    });
                }
                self.set_status_message("Queued copy to Drop Folder", true, 3);
                ui.close();
            }
            let move_target_btn = ui
                .add_enabled(
                    !target_missing && !same_as_target,
                    egui::Button::new(RichText::new("Move to Drop Folder").color(menu_col)),
                )
                .on_hover_text(if target_missing {
                    "No drop folder set"
                } else if same_as_target {
                    "Already in destination"
                } else {
                    ""
                });
            if move_target_btn.clicked() {
                if let Some(dst) = target_dir.clone() {
                    *pending_op = Some(FileCommand::Move {
                        sources: vec![path.clone()],
                        destination: dst,
                        conflict: ConflictAction::Rename,
                    });
                }
                self.set_status_message("Queued move to Drop Folder", true, 3);
                ui.close();
            }

            let copy_cur_btn = ui
                .add_enabled(
                    !same_as_current,
                    egui::Button::new(RichText::new("Copy to Current Folder").color(menu_col)),
                )
                .on_hover_text(if same_as_current {
                    "Already in destination"
                } else {
                    ""
                });
            if copy_cur_btn.clicked() {
                *pending_op = Some(FileCommand::Copy {
                    sources: vec![path.clone()],
                    destination: current_dir.to_path_buf(),
                    conflict: ConflictAction::Rename,
                });
                ui.close();
            }
            let move_cur_btn = ui
                .add_enabled(
                    !same_as_current,
                    egui::Button::new(RichText::new("Move to Current Folder").color(menu_col)),
                )
                .on_hover_text(if same_as_current {
                    "Already in destination"
                } else {
                    ""
                });
            if move_cur_btn.clicked() {
                *pending_op = Some(FileCommand::Move {
                    sources: vec![path.clone()],
                    destination: current_dir.to_path_buf(),
                    conflict: ConflictAction::Rename,
                });
                ui.close();
            }

            if tandem_target_dir.is_some() {
                ui.separator();
                let tandem_label = "Other Pane";
                let copy_tandem_btn = ui
                    .add_enabled(
                        !same_as_tandem,
                        egui::Button::new(
                            RichText::new(format!("Copy to {tandem_label}")).color(menu_col),
                        ),
                    )
                    .on_hover_text(if same_as_tandem {
                        "Already in destination"
                    } else {
                        ""
                    });
                if copy_tandem_btn.clicked() {
                    if let Some(dst) = tandem_target_dir.clone() {
                        *pending_op = Some(FileCommand::Copy {
                            sources: vec![path.clone()],
                            destination: dst,
                            conflict: ConflictAction::Rename,
                        });
                    }
                    self.set_status_message("Queued copy to other pane", true, 3);
                    ui.close();
                }
                let move_tandem_btn = ui
                    .add_enabled(
                        !same_as_tandem,
                        egui::Button::new(
                            RichText::new(format!("Move to {tandem_label}")).color(menu_col),
                        ),
                    )
                    .on_hover_text(if same_as_tandem {
                        "Already in destination"
                    } else {
                        ""
                    });
                if move_tandem_btn.clicked() {
                    if let Some(dst) = tandem_target_dir.clone() {
                        *pending_op = Some(FileCommand::Move {
                            sources: vec![path.clone()],
                            destination: dst,
                            conflict: ConflictAction::Rename,
                        });
                    }
                    self.set_status_message("Queued move to other pane", true, 3);
                    ui.close();
                }
            }
        });
    }

    fn update_completions(&mut self) {
        let input = self.command_frame.input.trim().to_string();
        let cwd = self.state.active_tab().current_dir.clone();
        self.command_frame.completions = compute_path_completions(&input, &cwd);
    }

    fn render_file_pane(&mut self, ctx: &Context) {
        if let Some((left_tab_id, right_tab_id)) = self.tandem_tab_ids() {
            self.render_tandem_file_pane(ctx, left_tab_id, right_tab_id);
        } else {
            self.render_single_file_pane(ctx);
        }
    }

    fn view_scale_for(&self, view_mode: ViewMode) -> f32 {
        let scale = match view_mode {
            ViewMode::Miller => self.state.config.miller_view_scale,
            ViewMode::List => self.state.config.list_view_scale,
            ViewMode::Grid => self.state.config.grid_view_scale,
        };
        scale.clamp(0.75, 1.85)
    }

    fn set_view_scale_for(&mut self, view_mode: ViewMode, scale: f32) {
        let clamped = scale.clamp(0.75, 1.85);
        match view_mode {
            ViewMode::Miller => self.state.config.miller_view_scale = clamped,
            ViewMode::List => self.state.config.list_view_scale = clamped,
            ViewMode::Grid => self.state.config.grid_view_scale = clamped,
        }
    }

    fn render_view_controls_row(
        &mut self,
        ui: &mut egui::Ui,
        tab_id: u64,
        bottom_bar_bg: Color32,
        bottom_text: Color32,
    ) {
        let c = self.colors;
        let current_sort = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.sort)
            .unwrap_or_default();
        let current_dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let view_mode = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.view_mode)
            .unwrap_or(ViewMode::Miller);

        ui.painter().hline(
            ui.max_rect().x_range(),
            ui.max_rect().top(),
            Stroke::new(1.0, mix_color(c.border, bottom_bar_bg, 0.5)),
        );
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Sort").size(11.5).color(bottom_text));
            ui.add_space(6.0);

            let mut sort_by = current_sort.by;
            egui::ComboBox::from_id_salt(("sort_by", tab_id))
                .selected_text(
                    RichText::new(match sort_by {
                        SortBy::Name => "Name",
                        SortBy::Kind => "Type",
                        SortBy::Size => "Size",
                        SortBy::Modified => "Modified",
                    })
                    .color(bottom_text)
                    .size(11.5),
                )
                .width(90.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut sort_by,
                        SortBy::Name,
                        RichText::new("Name").color(bottom_text),
                    );
                    ui.selectable_value(
                        &mut sort_by,
                        SortBy::Kind,
                        RichText::new("Type").color(bottom_text),
                    );
                    ui.selectable_value(
                        &mut sort_by,
                        SortBy::Size,
                        RichText::new("Size").color(bottom_text),
                    );
                    ui.selectable_value(
                        &mut sort_by,
                        SortBy::Modified,
                        RichText::new("Modified").color(bottom_text),
                    );
                });

            let mut asc = if sort_by == current_sort.by {
                current_sort.ascending
            } else {
                default_sort_direction(sort_by)
            };
            let sort_supports_direction = sort_supports_direction(sort_by);
            let direction_label = match sort_by {
                SortBy::Name | SortBy::Kind => {
                    if asc {
                        "A-Z"
                    } else {
                        "Z-A"
                    }
                }
                SortBy::Size => "Largest",
                SortBy::Modified => "Newest",
            };
            if !sort_supports_direction {
                asc = false;
            }
            if ui
                .add_enabled(
                    sort_supports_direction,
                    egui::Button::new(RichText::new(direction_label).size(11.5).color(bottom_text))
                        .min_size(Vec2::new(72.0, 20.0)),
                )
                .on_hover_text(if sort_supports_direction {
                    "Toggle sort direction"
                } else {
                    "This sort mode uses a fixed direction"
                })
                .clicked()
            {
                asc = !asc;
            }

            if sort_by != current_sort.by || asc != current_sort.ascending {
                self.focus_tab(tab_id);
                let dir = self.state.tab_by_id(tab_id).map(|t| t.current_dir.clone());
                if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
                    tab.sort.by = sort_by;
                    tab.sort.ascending = asc;
                }
                if let Some(dir) = dir {
                    self.refresh_tab(tab_id, dir);
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.render_view_scale_control(ui, view_mode, &current_dir, c);
            });
        });
    }

    fn render_side_nav_row(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let can_back = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.can_go_back())
            .unwrap_or(false);
        let can_fwd = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.can_go_forward())
            .unwrap_or(false);
        let can_up = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.can_go_up())
            .unwrap_or(false);

        let back_col = if can_back {
            c.text_muted
        } else {
            c.text_muted.gamma_multiply(0.62)
        };
        let fwd_col = if can_fwd {
            c.text_muted
        } else {
            c.text_muted.gamma_multiply(0.62)
        };
        let up_col = if can_up {
            c.text_muted
        } else {
            c.text_muted.gamma_multiply(0.62)
        };

        ui.spacing_mut().item_spacing.x = 2.0;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if ui
                .add_enabled(
                    can_back,
                    egui::Button::new(RichText::new(MI_ARROW_BACK).size(20.0).color(back_col))
                        .frame(false)
                        .min_size(Vec2::splat(30.0)),
                )
                .on_hover_text("Back")
                .clicked()
            {
                self.focus_tab(tab_id);
                self.navigate_tab_back(tab_id);
            }
            if ui
                .add_enabled(
                    can_fwd,
                    egui::Button::new(RichText::new(MI_ARROW_FORWARD).size(20.0).color(fwd_col))
                        .frame(false)
                        .min_size(Vec2::splat(30.0)),
                )
                .on_hover_text("Forward")
                .clicked()
            {
                self.focus_tab(tab_id);
                self.navigate_tab_forward(tab_id);
            }
            if ui
                .add_enabled(
                    can_up,
                    egui::Button::new(RichText::new(MI_ARROW_UP).size(20.0).color(up_col))
                        .frame(false)
                        .min_size(Vec2::splat(30.0)),
                )
                .on_hover_text("Parent folder")
                .clicked()
            {
                self.focus_tab(tab_id);
                self.navigate_tab_up(tab_id);
            }
            ui.add_space(6.0);

            let view = self
                .state
                .tab_by_id(tab_id)
                .map(|t| t.view_mode)
                .unwrap_or(ViewMode::Miller);
            let (view_icon, view_tip) = match view {
                ViewMode::Miller => (MI_VIEW_COLUMN, "Switch to List view"),
                ViewMode::List => (MI_VIEW_LIST, "Switch to Grid view"),
                ViewMode::Grid => (MI_APPS, "Switch to Column view"),
            };
            if ui
                .add(
                    egui::Button::new(RichText::new(view_icon).size(20.0).color(c.text_muted))
                        .frame(false)
                        .min_size(Vec2::splat(30.0)),
                )
                .on_hover_text(view_tip)
                .clicked()
            {
                self.focus_tab(tab_id);
                let next = match view {
                    ViewMode::Miller => ViewMode::List,
                    ViewMode::List => ViewMode::Grid,
                    ViewMode::Grid => ViewMode::Miller,
                };
                if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
                    tab.view_mode = next;
                }
            }
            if let Some(link) = self.current_tandem_view()
                && link.contains_tab(tab_id)
            {
                let pinned = link.pinned_tab_id == Some(tab_id);
                ui.add_space(2.0);
                let pin = ui
                    .add(
                        egui::Button::new(RichText::new(MI_LOCK).size(18.0).color(if pinned {
                            c.accent
                        } else {
                            c.text_muted
                        }))
                        .frame(false)
                        .min_size(Vec2::splat(30.0)),
                    )
                    .on_hover_text(if pinned {
                        "Unpin this side"
                    } else {
                        "Pin this side"
                    });
                if pin.clicked() {
                    self.toggle_tandem_pin(tab_id);
                }
            }
            ui.add_space(6.0);
            ui.allocate_ui(Vec2::new(ui.available_width().max(80.0), 26.0), |ui| {
                self.render_address_bar(ui, tab_id);
            });
        });
    }

    fn render_tab_contents(&mut self, ui: &mut egui::Ui, tab_id: u64, allow_smart_panel: bool) {
        let view_mode = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.view_mode)
            .unwrap_or(ViewMode::Miller);
        let avail = ui.available_rect_before_wrap();
        if avail.width() <= 48.0 || avail.height() <= 48.0 {
            match view_mode {
                ViewMode::Miller => self.render_miller(ui, tab_id),
                ViewMode::List => self.render_list(ui, tab_id),
                ViewMode::Grid => self.render_grid(ui, tab_id),
            }
            return;
        }
        if allow_smart_panel {
            let (file_rect_local, panel_rect) = self.compute_smart_panel_layout(avail);
            self.smart_panel_rect = Some(panel_rect);
            ui.scope_builder(
                UiBuilder::new().max_rect(file_rect_local),
                |ui| match view_mode {
                    ViewMode::Miller => self.render_miller(ui, tab_id),
                    ViewMode::List => self.render_list(ui, tab_id),
                    ViewMode::Grid => self.render_grid(ui, tab_id),
                },
            );
        } else {
            match view_mode {
                ViewMode::Miller => self.render_miller(ui, tab_id),
                ViewMode::List => self.render_list(ui, tab_id),
                ViewMode::Grid => self.render_grid(ui, tab_id),
            }
        }
    }

    fn render_single_file_pane(&mut self, ctx: &Context) {
        let c = self.colors;
        let bottom_bar_bg = mix_color(c.toolbar_bg, c.bg, 0.42);
        let bottom_text = mix_color(c.text_muted, bottom_bar_bg, 0.38);
        let tab_id = self.state.active_tab().id;
        self.smart_panel_rect = None;

        egui::TopBottomPanel::bottom("view_controls_bar")
            .exact_height(VIEW_CONTROLS_BAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(bottom_bar_bg)
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ctx, |ui| {
                self.render_view_controls_row(ui, tab_id, bottom_bar_bg, bottom_text);
            });

        let mut file_rect: Option<Rect> = None;
        let pane = egui::CentralPanel::default()
            .frame(Frame::new().fill(c.bg))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                if avail.width() <= 48.0 || avail.height() <= 48.0 {
                    self.render_tab_contents(ui, tab_id, true);
                    return;
                }
                let (file_rect_local, panel_rect) = self.compute_smart_panel_layout(avail);
                self.smart_panel_rect = Some(panel_rect);
                file_rect = Some(file_rect_local);
                ui.scope_builder(UiBuilder::new().max_rect(file_rect_local), |ui| {
                    self.render_tab_contents(ui, tab_id, true);
                });
            });
        self.file_pane_rect = file_rect.or(Some(pane.response.rect));
    }

    fn render_tandem_side(
        &mut self,
        ui: &mut egui::Ui,
        tab_id: u64,
        side_rect: Rect,
        bottom_bar_bg: Color32,
        bottom_text: Color32,
    ) {
        let c = self.colors;
        let header_h = TANDEM_PANE_HEADER_HEIGHT;
        let footer_h = VIEW_CONTROLS_BAR_HEIGHT;
        let header_rect =
            Rect::from_min_size(side_rect.min, Vec2::new(side_rect.width(), header_h));
        let footer_rect = Rect::from_min_size(
            egui::pos2(side_rect.min.x, side_rect.max.y - footer_h),
            Vec2::new(side_rect.width(), footer_h),
        );
        let content_rect = Rect::from_min_max(
            egui::pos2(side_rect.min.x, side_rect.min.y + header_h),
            egui::pos2(side_rect.max.x, side_rect.max.y - footer_h),
        );

        ui.painter().rect_filled(side_rect, 0.0, c.bg);
        ui.painter().rect_stroke(
            side_rect,
            0.0,
            Stroke::new(1.0, mix_color(c.border, c.bg, 0.8)),
            egui::StrokeKind::Inside,
        );

        ui.scope_builder(UiBuilder::new().max_rect(header_rect), |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, c.toolbar_bg);
            self.render_side_nav_row(ui, tab_id);
        });
        ui.scope_builder(UiBuilder::new().max_rect(footer_rect), |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, bottom_bar_bg);
            self.render_view_controls_row(ui, tab_id, bottom_bar_bg, bottom_text);
        });
        ui.scope_builder(UiBuilder::new().max_rect(content_rect), |ui| {
            self.render_tab_contents(ui, tab_id, false);
        });
    }

    fn render_tandem_file_pane(&mut self, ctx: &Context, left_tab_id: u64, right_tab_id: u64) {
        let c = self.colors;
        let bottom_bar_bg = mix_color(c.toolbar_bg, c.bg, 0.42);
        let bottom_text = mix_color(c.text_muted, bottom_bar_bg, 0.38);
        self.smart_panel_rect = None;
        let pane = egui::CentralPanel::default()
            .frame(Frame::new().fill(c.bg))
            .show(ctx, |ui| {
                let avail = ui.available_rect_before_wrap();
                if avail.width() <= 96.0 || avail.height() <= 96.0 {
                    self.render_tab_contents(
                        ui,
                        self.tandem_active_tab_id().unwrap_or(left_tab_id),
                        false,
                    );
                    return;
                }

                let mut split_ratio = self
                    .state
                    .link_view
                    .as_ref()
                    .map(|lv| lv.split_ratio.clamp(0.25, 0.75))
                    .unwrap_or(0.5);
                let divider_w = TANDEM_DIVIDER_WIDTH;
                let split_x = avail.left() + avail.width() * split_ratio;
                let divider_rect = Rect::from_min_max(
                    egui::pos2(
                        (split_x - divider_w * 0.5)
                            .clamp(avail.left() + 64.0, avail.right() - 64.0),
                        avail.top(),
                    ),
                    egui::pos2(
                        (split_x + divider_w * 0.5)
                            .clamp(avail.left() + 64.0, avail.right() - 64.0),
                        avail.bottom(),
                    ),
                );
                let divider_resp = ui.interact(
                    divider_rect,
                    ui.id().with("tandem_split_divider"),
                    Sense::click_and_drag(),
                );
                if (divider_resp.dragged() || divider_resp.clicked())
                    && let Some(pointer) = ui.ctx().pointer_interact_pos()
                {
                    split_ratio = ((pointer.x - avail.left()) / avail.width()).clamp(0.25, 0.75);
                    if let Some(link_view) = self.state.link_view.as_mut() {
                        link_view.split_ratio = split_ratio;
                    }
                }
                if divider_resp.hovered() || divider_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                ui.painter()
                    .rect_filled(divider_rect, 4.0, mix_color(c.panel_raised, c.bg, 0.65));
                ui.painter().line_segment(
                    [
                        egui::pos2(divider_rect.center().x, divider_rect.top()),
                        egui::pos2(divider_rect.center().x, divider_rect.bottom()),
                    ],
                    Stroke::new(1.0, c.border),
                );

                let left_max_x = (divider_rect.left() - 4.0).max(avail.left() + 64.0);
                let right_min_x = (divider_rect.right() + 4.0).min(avail.right() - 64.0);
                let left_rect = Rect::from_min_max(avail.min, egui::pos2(left_max_x, avail.max.y));
                let right_rect =
                    Rect::from_min_max(egui::pos2(right_min_x, avail.min.y), avail.max);

                let active_tab_id = self.tandem_active_tab_id().unwrap_or(left_tab_id);
                let left_border = if active_tab_id == left_tab_id {
                    c.accent
                } else {
                    c.border
                };
                let right_border = if active_tab_id == right_tab_id {
                    c.accent
                } else {
                    c.border
                };
                ui.painter().rect_stroke(
                    left_rect,
                    0.0,
                    Stroke::new(1.0, left_border),
                    egui::StrokeKind::Inside,
                );
                ui.painter().rect_stroke(
                    right_rect,
                    0.0,
                    Stroke::new(1.0, right_border),
                    egui::StrokeKind::Inside,
                );

                ui.scope_builder(UiBuilder::new().max_rect(left_rect), |ui| {
                    self.render_tandem_side(ui, left_tab_id, left_rect, bottom_bar_bg, bottom_text);
                });
                ui.scope_builder(UiBuilder::new().max_rect(right_rect), |ui| {
                    self.render_tandem_side(
                        ui,
                        right_tab_id,
                        right_rect,
                        bottom_bar_bg,
                        bottom_text,
                    );
                });
            });
        self.file_pane_rect = Some(pane.response.rect);
    }

    fn render_view_scale_control(
        &mut self,
        ui: &mut egui::Ui,
        view_mode: ViewMode,
        current_dir: &Path,
        c: Colors,
    ) {
        let scale = self.view_scale_for(view_mode);
        let control_text = mix_color(c.text_dim, mix_color(c.toolbar_bg, c.bg, 0.42), 0.5);
        if matches!(view_mode, ViewMode::Miller) {
            let has_custom = self.has_custom_miller_width(current_dir);
            let mode = self.state.config.miller_column_width_mode;
            let (icon, tip) = match mode {
                MillerColumnWidthMode::Fixed => (MI_WIDTH_NORMAL, "Fixed column width"),
                MillerColumnWidthMode::Auto => (MI_WIDTH_FULL, "Auto-fit column width"),
            };
            let icon_col = if has_custom { c.accent } else { control_text };
            let resp = ui.add(
                egui::Button::new(RichText::new(icon).size(14.0).color(icon_col))
                    .frame(false)
                    .min_size(Vec2::new(20.0, 20.0)),
            );
            if resp.clicked() {
                self.state.config.miller_column_width_mode = match mode {
                    MillerColumnWidthMode::Fixed => MillerColumnWidthMode::Auto,
                    MillerColumnWidthMode::Auto => MillerColumnWidthMode::Fixed,
                };
                self.clear_all_miller_column_widths();
            }
            resp.on_hover_text(if has_custom {
                format!("{} · custom width set", tip)
            } else {
                tip.to_string()
            });
            ui.add_space(6.0);
        }
        ui.label(
            RichText::new(format!("{:.0}%", scale * 100.0))
                .size(11.0)
                .color(control_text),
        );
        ui.add_space(8.0);
        if ui
            .add(
                egui::Button::new(
                    RichText::new(MI_SETTINGS_BACKUP_RESTORE)
                        .size(15.0)
                        .color(control_text),
                )
                .frame(false)
                .min_size(Vec2::new(20.0, 20.0)),
            )
            .on_hover_text("Reset current view size")
            .clicked()
        {
            self.set_view_scale_for(view_mode, 1.0);
        }
        ui.add_space(8.0);

        let track_size = Vec2::new(132.0, 18.0);
        let (track_rect, track_resp) = ui.allocate_exact_size(track_size, Sense::click_and_drag());
        let track_fill = mix_color(c.panel_raised, c.bg, 0.28);
        ui.painter().rect_filled(track_rect, 9.0, track_fill);
        ui.painter().rect_stroke(
            track_rect,
            9.0,
            Stroke::new(1.0, mix_color(c.border, track_fill, 0.55)),
            egui::StrokeKind::Inside,
        );
        let t = ((scale - 0.75) / (1.85 - 0.75)).clamp(0.0, 1.0);
        let knob_x = egui::lerp((track_rect.left() + 9.0)..=(track_rect.right() - 9.0), t);
        let knob_center = egui::pos2(knob_x, track_rect.center().y);
        ui.painter().line_segment(
            [
                egui::pos2(track_rect.left() + 10.0, track_rect.center().y),
                egui::pos2(track_rect.right() - 10.0, track_rect.center().y),
            ],
            Stroke::new(2.0, mix_color(c.text_muted, track_fill, 0.45)),
        );
        ui.painter()
            .circle_filled(knob_center, 6.0, mix_color(c.text_muted, track_fill, 0.6));
        ui.painter()
            .circle_stroke(knob_center, 6.0, Stroke::new(1.0, c.bg));
        for offset in [-2.0_f32, 0.0, 2.0] {
            ui.painter().vline(
                knob_center.x + offset,
                (knob_center.y - 2.2)..=(knob_center.y + 2.2),
                Stroke::new(1.0, c.bg),
            );
        }
        if track_resp.hovered() || track_resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if (track_resp.dragged() || track_resp.clicked())
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
        {
            let t = ((pointer.x - (track_rect.left() + 9.0)) / (track_rect.width() - 18.0))
                .clamp(0.0, 1.0);
            self.set_view_scale_for(view_mode, 0.75 + t * (1.85 - 0.75));
        }
        track_resp.on_hover_text("Drag to scale Miller, List, and Grid density");
    }

    fn render_miller(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let scale = self.view_scale_for(ViewMode::Miller);
        let full = ui.available_rect_before_wrap();
        let h = full.height();
        let current_dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let mut pending_op: Option<FileCommand> = None;
        let mut open_parent: Option<PathBuf> = None;
        let mut open_entry_now: Option<(PathBuf, EntryKind)> = None;
        let mut open_tandem_now: Option<PathBuf> = None;
        let mut add_bookmark: Option<PathBuf> = None;
        let mut remove_bookmark: Option<PathBuf> = None;
        let (cols, current_col) = self.build_miller_model(tab_id, self.miller_col_count);
        if cols.is_empty() {
            return;
        }
        let focus_col = self.effective_miller_focus_col(tab_id, &cols, current_col);
        {
            let ui = self.tab_ui.entry(tab_id).or_default();
            ui.miller_focus_dir = Some(cols[focus_col].dir.clone());
            ui.miller_focus_hint = None;
        }
        // Detect navigation keys for scroll control.
        // Only act when the command frame is closed.
        let cmd_open = self.command_frame.visible;
        let is_up_down =
            ui.input(|i| i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowDown)) && !cmd_open;
        let scroll_left_nav = ui
            .input(|i| i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::Backspace))
            && !cmd_open;
        let scroll_right_nav =
            ui.input(|i| i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::Enter)) && !cmd_open;
        let epoch = self.tab_ui.entry(tab_id).or_default().scroll_epoch;
        let scroll_selected = {
            let ui_state = self.tab_ui.entry(tab_id).or_default();
            std::mem::take(&mut ui_state.miller_scroll_selected)
        };

        // File preview is based on the FOCUS column's selection.
        // Only show file preview when focus has a file selected;
        // folder selections show a preview column + empties.
        let focus_selected = cols
            .get(focus_col)
            .and_then(|c| c.selected.and_then(|i| c.entries.get(i).cloned()));

        // Read saved horizontal offset BEFORE the ScrollArea renders.
        let saved_h_offset = self.tab_ui.entry(tab_id).or_default().miller_h_offset;

        let mut sa = ScrollArea::horizontal()
            .id_salt(("miller_h", tab_id))
            .auto_shrink([false, false])
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible);

        // On Up/Down: force the scroll offset BEFORE rendering so egui
        // never gets a chance to shift the viewport horizontally.
        if is_up_down {
            sa = sa.horizontal_scroll_offset(saved_h_offset);
        }

        let sa_output = sa.show(ui, |ui| {
            ui.set_min_height(h);
            ui.horizontal(|ui| {
                let h_spacing = ui.style().spacing.item_spacing.x;
                // Guarantee the horizontal layout is at least as wide as
                // the high-water mark so egui never clamps the scroll offset.
                let prev_hwm = self.tab_ui.entry(tab_id).or_default().miller_total_w_max;
                if prev_hwm > 0.0 {
                    ui.set_min_width(prev_hwm);
                }
                // Preview columns only show selection when re-entering
                // a remembered path (depth > 1).  Read once, not per-row.
                let preview_depth = self
                    .tab_ui
                    .entry(tab_id)
                    .or_default()
                    .miller_preview_depth
                    .max(1);
                for (col_idx, col) in cols.iter().enumerate() {
                    let mut width = if col_idx > current_col {
                        // Keep per-folder width stable across role changes
                        // (current <-> preview). Only fall back to default
                        // sizing when no width exists yet.
                        let key = Self::miller_width_key(&col.dir);
                        if let Some(saved) =
                            self.state.config.folder_column_widths.get(&key).copied()
                        {
                            saved.clamp(96.0, 560.0)
                        } else {
                            self.miller_default_width(&col.entries).clamp(96.0, 560.0)
                        }
                    } else {
                        self.miller_column_width(&col.dir, &col.entries)
                    };
                    width = (width * scale.clamp(0.92, 1.22)).clamp(96.0, 620.0);
                    let inner = ui.allocate_ui_with_layout(
                        Vec2::new(width, h),
                        Layout::top_down(Align::Min),
                        |ui| {
                            // Enforce column-local clipping to avoid row painting leaking
                            // into neighboring columns when horizontally scrollable.
                            let clip = ui.clip_rect().intersect(ui.max_rect());
                            ui.set_clip_rect(clip);
                            ScrollArea::vertical()
                                .id_salt(("miller_col", tab_id, col.dir.clone(), epoch))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_min_width(width);
                                    ui.set_max_width(width);
                                    ui.set_min_height(h);
                                    if let Some(err) = &col.error {
                                        let retry = render_listing_error(ui, &c, err, true);
                                        if retry {
                                            let _ = self.retry_list_with_privileges(
                                                tab_id,
                                                col.dir.clone(),
                                            );
                                        }
                                        return;
                                    }
                                    if col.entries.is_empty() {
                                        if col_idx == current_col {
                                            // Entered empty folder: keep interaction minimal and non-blocking.
                                            self.render_empty_folder_state(ui, &col.dir, &c, false);
                                        } else if col_idx > current_col {
                                            // Previewed empty folder: show one-time Drop Folder hint.
                                            self.render_empty_folder_state(ui, &col.dir, &c, true);
                                        } else {
                                            ui.add_space(16.0);
                                            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                                ui.label(
                                                    RichText::new("Empty folder")
                                                        .color(c.text_muted)
                                                        .size(12.0),
                                                );
                                            });
                                        }
                                        return;
                                    }
                                    for (i, entry) in col.entries.iter().enumerate() {
                                        let is_path_row = col_idx < current_col
                                            && cols
                                                .get(col_idx + 1)
                                                .map(|next| next.dir == entry.path)
                                                .unwrap_or(false);
                                        let is_column_selection = col.selected == Some(i)
                                            && (col_idx <= focus_col || preview_depth > 1);
                                        let selected = is_path_row || is_column_selection;
                                        let opened_row = is_path_row
                                            || (col_idx < current_col && col.selected == Some(i));
                                        let resp = self.render_file_row(
                                            ui,
                                            entry,
                                            selected,
                                            col_idx == focus_col,
                                            opened_row,
                                            i,
                                            width,
                                            &c,
                                        );
                                        if scroll_selected && col_idx == focus_col && selected {
                                            let row_smooth = ScrollAnimation {
                                                points_per_second: 800.0,
                                                duration: Rangef::new(0.08, 0.25),
                                            };
                                            ui.scroll_to_rect_animation(
                                                resp.rect,
                                                Some(egui::Align::Center),
                                                row_smooth,
                                            );
                                        }
                                        if resp.clicked() {
                                            self.focus_tab(tab_id);
                                            let is_current = col_idx == current_col;
                                            self.set_miller_selection(
                                                tab_id,
                                                &col.dir,
                                                &col.entries,
                                                i,
                                                is_current,
                                            );
                                            {
                                                let ui_state =
                                                    self.tab_ui.entry(tab_id).or_default();
                                                ui_state.miller_focus_dir = Some(col.dir.clone());
                                                ui_state.miller_focus_hint = None;
                                            }
                                            if col_idx < current_col {
                                                self.set_tab_anchor_dir_live(
                                                    tab_id,
                                                    col.dir.clone(),
                                                );
                                            }
                                            ui.ctx().request_repaint();
                                        }
                                        if resp.double_clicked() {
                                            self.focus_tab(tab_id);
                                            let is_current = col_idx == current_col;
                                            self.set_miller_selection(
                                                tab_id,
                                                &col.dir,
                                                &col.entries,
                                                i,
                                                is_current,
                                            );
                                            if matches!(
                                                entry.kind,
                                                EntryKind::Directory | EntryKind::Symlink
                                            ) {
                                                self.tab_ui
                                                    .entry(tab_id)
                                                    .or_default()
                                                    .miller_focus_hint =
                                                    Some(MillerFocusHint::CurrentDir);
                                            }
                                            self.open_entry(entry.path.clone(), entry.kind);
                                        }
                                        self.entry_context_menu(
                                            &resp,
                                            entry,
                                            &current_dir,
                                            &mut pending_op,
                                            &mut open_parent,
                                            &mut open_entry_now,
                                            &mut open_tandem_now,
                                            &mut add_bookmark,
                                            &mut remove_bookmark,
                                        );
                                        if i >= 400 {
                                            ui.label(
                                                RichText::new("…").color(c.text_muted).size(11.0),
                                            );
                                            break;
                                        }
                                    }
                                });
                        },
                    );
                    let rect = inner.response.rect;

                    if col_idx == focus_col {
                        let clip = ui.clip_rect();
                        let smooth = ScrollAnimation {
                            points_per_second: 600.0,
                            duration: Rangef::new(0.15, 0.4),
                        };
                        // Left/Backspace: scroll left if focus is off the left edge.
                        if scroll_left_nav && rect.left() < clip.left() {
                            ui.scroll_to_rect_animation(rect, Some(egui::Align::LEFT), smooth);
                        }
                        // Right/Enter: ALWAYS ensure focus + 1 column is
                        // visible.  The user must never be in the rightmost
                        // visible column.
                        if scroll_right_nav {
                            let col_w =
                                (MILLER_FIXED_WIDTH * scale.clamp(0.92, 1.22)).clamp(96.0, 620.0);
                            let needed_right = rect.max.x + col_w;
                            if needed_right > clip.right() {
                                let padded = Rect::from_min_max(
                                    rect.min,
                                    egui::pos2(needed_right, rect.max.y),
                                );
                                ui.scroll_to_rect_animation(
                                    padded,
                                    Some(egui::Align::RIGHT),
                                    smooth,
                                );
                            }
                        }
                    }
                    // Always show separator after each directory column
                    // (the rightmost fill/preview column always follows).
                    {
                        ui.painter()
                            .vline(rect.max.x, rect.y_range(), Stroke::new(1.0, c.border));
                        let sep_rect = Rect::from_min_size(
                            egui::Pos2::new(rect.max.x - 3.0, rect.min.y),
                            Vec2::new(6.0, h),
                        );
                        let sep_resp = ui.interact(
                            sep_rect,
                            ui.id().with(("miller_sep", tab_id, col.dir.clone())),
                            Sense::drag(),
                        );
                        if sep_resp.hovered() || sep_resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        if sep_resp.dragged() {
                            let dx = ui.ctx().input(|i| i.pointer.delta().x);
                            width = (width + dx).clamp(96.0, 560.0);
                            self.set_miller_column_width(&col.dir, width);
                        }
                    }
                }

                // ── Finder-style rightmost fill columns ─────────────
                // Total content width can only GROW, never shrink.
                let col_w = (MILLER_FIXED_WIDTH * scale.clamp(0.92, 1.22)).clamp(96.0, 620.0);
                let viewport_w = full.width();
                // content_start is the left edge of the scroll area content.
                let content_start = ui.min_rect().min.x;
                // cursor_x is where the next element goes (after all content columns).
                let cursor_x = ui.cursor().min.x;
                // Content width so far (just the directory columns).
                let content_w = cursor_x - content_start;

                // Total content width can only GROW, never shrink.
                let min_total = content_w + viewport_w;
                let prev_hwm = self.tab_ui.entry(tab_id).or_default().miller_total_w_max;
                let target_total = min_total.max(prev_hwm);
                // Absolute X we need to fill to.
                let fill_to = content_start + target_total;

                let is_file_preview = focus_selected
                    .as_ref()
                    .map(|e| matches!(e.kind, EntryKind::File))
                    .unwrap_or(false);

                if is_file_preview {
                    // File preview: fill from cursor to visible right edge,
                    // at least one standard column wide, capped at viewport
                    // width to prevent infinite scroll feedback loops.
                    let visible_right = ui.clip_rect().max.x;
                    let preview_right = visible_right.max(cursor_x + col_w);
                    let fill_w = (preview_right - cursor_x).max(col_w).min(viewport_w);
                    self.render_miller_file_preview(
                        ui,
                        focus_selected.as_ref().unwrap(),
                        fill_w,
                        h,
                    );
                }

                // ALWAYS pad with empty fill columns to reach fill_to.
                // This runs after file preview too, so switching between
                // file/folder preview never shrinks the total width.
                {
                    let cur = ui.cursor().min.x;
                    let remaining = (fill_to - cur).max(0.0);
                    if remaining > 1.0 {
                        let n_fill = ((remaining / (col_w + h_spacing)).ceil() as usize).max(1);
                        for fi in 0..n_fill {
                            let w = if fi == n_fill - 1 {
                                (fill_to - ui.cursor().min.x).max(col_w)
                            } else {
                                col_w
                            };
                            let r = self.render_miller_empty_preview(ui, w, h);
                            if fi < n_fill - 1 {
                                ui.painter().vline(
                                    r.max.x,
                                    r.y_range(),
                                    Stroke::new(1.0, c.border),
                                );
                            }
                        }
                    }
                }

                // High-water mark — total width can only grow.
                let total_w = ui.cursor().min.x - content_start;
                let hwm = &mut self.tab_ui.entry(tab_id).or_default().miller_total_w_max;
                if total_w > *hwm {
                    *hwm = total_w;
                }
            });
        });

        // ── Freeze horizontal scroll on Up/Down ──────────────────────
        // On Up/Down frames: force offset back to the saved value so the
        // viewport never shifts horizontally when merely changing selection.
        // On all other frames: record the current offset for next time.
        if is_up_down {
            let mut state = sa_output.state;
            state.offset.x = saved_h_offset;
            state.store(ui.ctx(), sa_output.id);
        } else {
            self.tab_ui.entry(tab_id).or_default().miller_h_offset = sa_output.state.offset.x;
        }

        if let Some(cmd) = pending_op {
            self.run_file_op(cmd, Some(tab_id));
        }
        if let Some(dir) = open_parent {
            self.navigate_tab_to(tab_id, dir);
        }
        if let Some((p, k)) = open_entry_now {
            self.open_entry(p, k);
        }
        if let Some(path) = open_tandem_now {
            self.open_directory_in_tandem(path);
        }
        if let Some(path) = add_bookmark {
            self.add_bookmark_for_path(path);
        }
        if let Some(path) = remove_bookmark {
            self.remove_bookmark_for_path(&path);
        }
    }

    /// Renders a Finder-style file preview as the rightmost Miller column.
    /// Returns the rect of the rendered preview pane.
    fn render_miller_file_preview(
        &mut self,
        ui: &mut egui::Ui,
        entry: &FileEntry,
        width: f32,
        height: f32,
    ) -> Rect {
        let c = self.colors;
        let inner = ui.allocate_ui_with_layout(
            Vec2::new(width, height),
            Layout::top_down(Align::Min),
            |ui| {
                let clip = ui.clip_rect().intersect(ui.max_rect());
                ui.set_clip_rect(clip);

                let bg = mix_color(c.bg, c.panel, 0.12);
                ui.painter().rect_filled(ui.max_rect(), 0.0, bg);

                let pad = 16.0;
                let lc = c.text_muted;
                let vc = c.text;

                // ── Read info-panel collapsed state ──────────
                let info_id = ui.id().with(("preview_info_open", &entry.path));
                let info_open = ui.ctx().data_mut(|d| {
                    // Default to open
                    let v = d.get_persisted_mut_or_insert_with::<bool>(info_id, || true);
                    *v
                });
                let details_id = ui.id().with(("preview_details", &entry.path));
                let details_open = ui
                    .ctx()
                    .data_mut(|d| *d.get_persisted_mut_or_default::<bool>(details_id));

                // Calculate info panel height to reserve space for it
                let info_header_h = 28.0; // collapse toggle bar
                let info_bottom_pad = 24.0; // keep clear of scrollbar grab zone
                let info_body_h = if info_open {
                    let base = 100.0; // name + type + size + modified + location
                    if details_open { base + 90.0 } else { base }
                } else {
                    0.0
                };
                let info_total_h = info_header_h + info_body_h + info_bottom_pad;

                // ── Prepare preview content ──────────
                let style = style_for_entry(
                    entry,
                    EntryVisualState {
                        selected: true,
                        focused: true,
                        hovered: false,
                        opened: false,
                        symlink_dir: false,
                    },
                    &c,
                    &self.state.config,
                );
                let icon = material_icon(style.icon);
                let icon_color = style.icon_color;

                let mut preview_widget: Option<egui::Image<'static>> = None;
                let mut text_snippet: Option<String> = None;
                let max_preview_w = (width - pad * 2.0).max(96.0);
                // Preview fills the space above the info panel
                let max_preview_h = (height - info_total_h - 32.0).max(80.0);

                let preview = self.cached_info_preview(entry);
                match preview.kind {
                    PreviewKind::Image => {
                        if let Some(img) = self.cached_image_preview_widget(
                            &entry.path,
                            max_preview_w,
                            max_preview_h,
                            8,
                        ) {
                            preview_widget = Some(img);
                        }
                    }
                    PreviewKind::Text => {
                        let is_svg = entry
                            .path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.eq_ignore_ascii_case("svg"))
                            .unwrap_or(false);
                        if is_svg
                            && let Some(img) = self.cached_image_preview_widget(
                                &entry.path,
                                max_preview_w,
                                max_preview_h,
                                8,
                            )
                        {
                            preview_widget = Some(img);
                        }
                        if preview_widget.is_none() {
                            const SNIPPET_MAX: usize = 800;
                            let mut snippet = String::new();
                            let mut left = SNIPPET_MAX;
                            for (i, line) in preview.body.lines().enumerate() {
                                if i >= 16 || left == 0 {
                                    break;
                                }
                                if i > 0 {
                                    snippet.push('\n');
                                }
                                let take = line.len().min(left);
                                snippet.push_str(&line[..take]);
                                left = left.saturating_sub(take);
                            }
                            if !snippet.is_empty() {
                                text_snippet = Some(snippet);
                            }
                        }
                    }
                    _ => {}
                }

                // ── Preview area (top, fills remaining space) ──────────
                let preview_h = (height - info_total_h).max(80.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(width, preview_h),
                    Layout::top_down(Align::Center),
                    |ui| {
                        ui.add_space(16.0);
                        if let Some(img) = preview_widget {
                            ui.add(img.max_height(max_preview_h).max_width(max_preview_w));
                        } else if let Some(mut snippet) = text_snippet {
                            let text_w = (width - pad * 2.0).max(120.0);
                            let text_h = max_preview_h.min(preview_h * 0.7).max(60.0);
                            ui.add_sized(
                                Vec2::new(text_w, text_h),
                                egui::TextEdit::multiline(&mut snippet)
                                    .font(FontId::monospace(11.0))
                                    .desired_rows(12)
                                    .interactive(false),
                            );
                        } else {
                            ui.add_space((preview_h * 0.3).max(16.0));
                            ui.label(RichText::new(icon).size(52.0).color(icon_color));
                        }
                    },
                );

                // ── Info panel (bottom, framed, collapsible) ──────────
                let info_bg = mix_color(c.bg, c.panel, 0.25);
                let info_rect = Rect::from_min_size(
                    egui::pos2(ui.max_rect().min.x, ui.max_rect().max.y - info_total_h),
                    Vec2::new(width, info_total_h),
                );
                ui.painter().rect_filled(info_rect, 0.0, info_bg);
                // Top border line
                ui.painter().hline(
                    info_rect.x_range(),
                    info_rect.min.y,
                    Stroke::new(1.0, mix_color(c.border, c.bg, 0.5)),
                );

                // Place the info panel at the bottom
                ui.scope_builder(UiBuilder::new().max_rect(info_rect), |ui| {
                    ui.set_clip_rect(info_rect);

                    // ── Collapse/expand header bar ──────────
                    let header_rect =
                        Rect::from_min_size(info_rect.min, Vec2::new(width, info_header_h));
                    let header_resp = ui.allocate_rect(header_rect, Sense::click());

                    let type_desc = file_type_description(&entry.path, entry.kind);
                    let arrow = if info_open { "\u{E5CE}" } else { "\u{E5CF}" };
                    let header_text = if info_open {
                        truncate_name(&entry.name, 40).to_string()
                    } else {
                        format!("{} — {type_desc}", truncate_name(&entry.name, 30))
                    };

                    // Arrow icon
                    ui.painter().text(
                        egui::pos2(header_rect.min.x + 10.0, header_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        arrow,
                        FontId::proportional(12.0),
                        c.text_muted,
                    );
                    // Header label
                    ui.painter().text(
                        egui::pos2(header_rect.min.x + 26.0, header_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &header_text,
                        FontId::proportional(11.0),
                        c.text,
                    );

                    if header_resp.clicked() {
                        let new_val = !info_open;
                        ui.ctx().data_mut(|d| d.insert_persisted(info_id, new_val));
                        ui.ctx().request_repaint();
                    }

                    if !info_open {
                        return;
                    }

                    // ── Info body ──────────
                    let body_rect = Rect::from_min_size(
                        egui::pos2(info_rect.min.x, info_rect.min.y + info_header_h),
                        Vec2::new(width, info_body_h),
                    );
                    ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
                        let ipad = 12.0;
                        ui.horizontal(|ui| {
                            ui.add_space(ipad);
                            ui.vertical(|ui| {
                                let info_w = (width - ipad * 2.0).max(100.0);
                                ui.set_max_width(info_w);

                                // File type
                                ui.label(RichText::new(&type_desc).color(lc).size(10.0));
                                ui.add_space(6.0);

                                // Metadata rows helper — inline "Label: value"
                                let meta_row = |ui: &mut egui::Ui, label: &str, value: &str| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("{label}:")).color(lc).size(10.0),
                                        );
                                        ui.label(RichText::new(value).color(vc).size(10.0));
                                    });
                                };

                                // Size — clickable to cycle units
                                if let Some(size) = entry.size_bytes {
                                    let units = format_size_units(size);
                                    if units.len() > 1 {
                                        let size_cycle_id =
                                            ui.id().with(("size_cycle", &entry.path));
                                        let idx: usize = ui.ctx().data_mut(|d| {
                                            *d.get_persisted_mut_or_default::<usize>(size_cycle_id)
                                        });
                                        let display = &units[idx % units.len()];
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Size:").color(lc).size(10.0));
                                            let resp = ui.add(
                                                egui::Label::new(
                                                    RichText::new(display)
                                                        .color(c.accent)
                                                        .size(10.0)
                                                        .underline(),
                                                )
                                                .sense(Sense::click()),
                                            );
                                            if resp.clicked() {
                                                let next = (idx + 1) % units.len();
                                                ui.ctx().data_mut(|d| {
                                                    d.insert_persisted(size_cycle_id, next)
                                                });
                                                ui.ctx().request_repaint();
                                            }
                                            if resp.hovered() {
                                                ui.ctx().set_cursor_icon(
                                                    egui::CursorIcon::PointingHand,
                                                );
                                            }
                                        });
                                    } else {
                                        meta_row(ui, "Size", &format_size(size));
                                    }
                                }
                                if let Some(secs) = entry.modified_unix_secs {
                                    meta_row(ui, "Modified", &format_modified(secs));
                                }
                                if let Some(parent) = entry.path.parent() {
                                    meta_row(ui, "Location", &parent.display().to_string());
                                }

                                // ── Details toggle ──────────
                                ui.add_space(4.0);
                                let dtoggle = if details_open {
                                    "\u{E5CE} Details"
                                } else {
                                    "\u{E5CF} Details"
                                };
                                if ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(dtoggle).color(c.accent).size(10.0),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .clicked()
                                {
                                    let new_val = !details_open;
                                    ui.ctx()
                                        .data_mut(|d| d.insert_persisted(details_id, new_val));
                                    ui.ctx().request_repaint();
                                }

                                if details_open {
                                    ui.add_space(2.0);
                                    if let Some(ext) =
                                        entry.path.extension().and_then(|e| e.to_str())
                                    {
                                        meta_row(ui, "Extension", &format!(".{ext}"));
                                    }
                                    meta_row(ui, "Full path", &entry.path.display().to_string());
                                    let detail = self.cached_info_details(entry);
                                    if let Some(owner) = detail.owner.as_deref() {
                                        meta_row(ui, "Owner", owner);
                                    }
                                    if let Some(group) = detail.group.as_deref() {
                                        meta_row(ui, "Group", group);
                                    }
                                    if let Some(perms) = detail.permissions.as_deref() {
                                        meta_row(ui, "Permissions", perms);
                                    }
                                    if let Some(dims) = detail.image_dimensions.as_deref() {
                                        meta_row(ui, "Dimensions", dims);
                                    }
                                }
                            });
                        });
                    });
                });
            },
        );
        inner.response.rect
    }

    /// Renders the empty/no-selection state for the rightmost fill column.
    fn render_miller_empty_preview(&self, ui: &mut egui::Ui, width: f32, height: f32) -> Rect {
        let c = self.colors;
        let inner = ui.allocate_ui_with_layout(
            Vec2::new(width, height),
            Layout::top_down(Align::Min),
            |ui| {
                let clip = ui.clip_rect().intersect(ui.max_rect());
                ui.set_clip_rect(clip);
                // Same background as regular columns — no special tint
                // so empty fill columns look identical to content columns.
                ui.painter().rect_filled(ui.max_rect(), 0.0, c.bg);
            },
        );
        inner.response.rect
    }

    fn render_list(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let scale = self.view_scale_for(ViewMode::Miller);
        let current_dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let mut pending_op: Option<FileCommand> = None;
        let mut open_parent: Option<PathBuf> = None;
        let mut open_entry_now: Option<(PathBuf, EntryKind)> = None;
        let mut open_tandem_now: Option<PathBuf> = None;
        let mut add_bookmark: Option<PathBuf> = None;
        let mut remove_bookmark: Option<PathBuf> = None;
        let current_sort = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.sort)
            .unwrap_or_default();
        let (entries, loading, error, sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (
                s.main.entries.clone(),
                s.main.loading,
                s.main.error.clone(),
                s.main_sel,
            )
        };
        let mut visible_cols = self.state.config.list_columns.clone();
        if !visible_cols.contains(&ListColumn::Name) {
            visible_cols.insert(0, ListColumn::Name);
        }
        visible_cols.sort_by_key(|c| match c {
            ListColumn::Name => 0,
            ListColumn::Kind => 1,
            ListColumn::Size => 2,
            ListColumn::Modified => 3,
        });

        if loading {
            ui.add_space(32.0);
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.label(RichText::new("Loading…").color(c.text_muted));
            });
            return;
        }
        if let Some(err) = error {
            let dir = self.tab_ui.entry(tab_id).or_default().main.dir.clone();
            let retry = render_listing_error(ui, &c, &err, true);
            if retry {
                let _ = self.retry_list_with_privileges(tab_id, dir);
            }
            return;
        }

        // Column headers (click to sort)
        let header_h = (26.0 * scale.clamp(0.92, 1.22)).round();
        let row_h = (28.0 * scale.clamp(0.92, 1.28)).round();
        let icon_size = (16.0 * scale.sqrt()).max(14.0);
        let name_size = (13.0 * scale.sqrt()).max(11.0);
        let meta_size = (12.0 * scale.sqrt()).max(10.0);
        let header_size = (11.5 * scale.sqrt()).max(10.2);
        let header_rect =
            Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), header_h));
        let header_resp = ui.allocate_rect(header_rect, Sense::click());
        header_resp.context_menu(|ui| {
            ui.visuals_mut().override_text_color = Some(c.text_muted);
            ui.set_min_width(180.0);
            ui.label(
                RichText::new("Visible columns")
                    .size(12.0)
                    .color(c.text_dim),
            );
            ui.add(egui::Separator::default().horizontal().spacing(4.0));

            let mut cols = self.state.config.list_columns.clone();
            let mut name_on = cols.contains(&ListColumn::Name);
            ui.add_enabled(false, egui::Checkbox::new(&mut name_on, "Name"));

            for (col, label) in [
                (ListColumn::Kind, "Type"),
                (ListColumn::Size, "Size"),
                (ListColumn::Modified, "Modified"),
            ] {
                let mut on = cols.contains(&col);
                if ui.checkbox(&mut on, label).changed() {
                    if on {
                        cols.push(col);
                    } else {
                        cols.retain(|c| c != &col);
                    }
                    self.state.config.list_columns = cols.clone();
                }
            }
        });
        ui.painter().rect_filled(header_rect, 0.0, c.panel_raised);
        let col_index = |col: ListColumn| -> usize {
            match col {
                ListColumn::Name => 0,
                ListColumn::Kind => 1,
                ListColumn::Size => 2,
                ListColumn::Modified => 3,
            }
        };
        let col_sort = |col: ListColumn| -> SortBy {
            match col {
                ListColumn::Name => SortBy::Name,
                ListColumn::Kind => SortBy::Kind,
                ListColumn::Size => SortBy::Size,
                ListColumn::Modified => SortBy::Modified,
            }
        };
        let col_label = |col: ListColumn| -> &'static str {
            match col {
                ListColumn::Name => "Name",
                ListColumn::Kind => "Type",
                ListColumn::Size => "Size",
                ListColumn::Modified => "Modified",
            }
        };
        let mut x = header_rect.min.x + 8.0;
        let mut col_widths = visible_cols
            .iter()
            .map(|c| self.list_col_px[col_index(*c)])
            .collect::<Vec<_>>();
        let max_total = (header_rect.width() - 16.0).max(120.0);
        let mut used_total: f32 = col_widths.iter().sum();
        if used_total > max_total {
            let scale = max_total / used_total;
            for w in &mut col_widths {
                *w = (*w * scale).max(80.0);
            }
            used_total = col_widths.iter().sum();
        }
        if used_total < max_total {
            col_widths[0] += max_total - used_total;
        }

        for (idx, col) in visible_cols.iter().enumerate() {
            let sort_by = col_sort(*col);
            let label = col_label(*col);
            let w = col_widths[idx];
            let col_rect = Rect::from_min_size(
                egui::Pos2::new(x, header_rect.min.y),
                Vec2::new(w, header_rect.height()),
            );
            let col_resp = ui.interact(
                col_rect,
                ui.id().with(("list_header_sort", tab_id, label)),
                Sense::click(),
            );
            if col_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                ui.painter().rect_filled(col_rect, 0.0, c.hover);
            }
            if col_resp.clicked() {
                let dir = self.state.tab_by_id(tab_id).map(|t| t.current_dir.clone());
                if let Some(tab) = self.state.tab_by_id_mut(tab_id) {
                    if tab.sort.by == sort_by {
                        if sort_supports_direction(sort_by) {
                            tab.sort.ascending = !tab.sort.ascending;
                        }
                    } else {
                        tab.sort.by = sort_by;
                        tab.sort.ascending = default_sort_direction(sort_by);
                    }
                }
                if let Some(dir) = dir {
                    self.refresh_tab(tab_id, dir);
                }
                return;
            }
            let arrow = if current_sort.by == sort_by {
                if current_sort.ascending {
                    " ▲"
                } else {
                    " ▼"
                }
            } else {
                ""
            };
            ui.painter().text(
                egui::Pos2::new(x + 4.0, header_rect.center().y),
                Align2::LEFT_CENTER,
                format!("{}{}", label, arrow),
                FontId::proportional(header_size),
                if current_sort.by == sort_by {
                    c.text_dim
                } else {
                    c.text_muted
                },
            );

            // Drag handle between columns
            if idx + 1 < visible_cols.len() {
                let sep_rect = Rect::from_min_size(
                    egui::Pos2::new(x + w - 2.0, header_rect.min.y + 2.0),
                    Vec2::new(4.0, header_rect.height() - 4.0),
                );
                let sep_resp = ui.interact(
                    sep_rect,
                    ui.id().with(("list_col_resize", tab_id, idx)),
                    Sense::drag(),
                );
                if sep_resp.hovered() || sep_resp.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    ui.painter().vline(
                        sep_rect.center().x,
                        sep_rect.y_range(),
                        Stroke::new(1.0, c.border),
                    );
                }
                if sep_resp.dragged() {
                    let dx = ui.ctx().input(|i| i.pointer.delta().x);
                    let left_idx = col_index(visible_cols[idx]);
                    let right_idx = col_index(visible_cols[idx + 1]);
                    let new_left = (self.list_col_px[left_idx] + dx).max(80.0);
                    let new_right = (self.list_col_px[right_idx] - dx).max(80.0);
                    self.list_col_px[left_idx] = new_left;
                    self.list_col_px[right_idx] = new_right;
                }
            }
            x += w;
        }

        // Rows
        ScrollArea::vertical()
            .id_salt(("list_view", tab_id))
            .show(ui, |ui| {
                if entries.is_empty() {
                    self.render_empty_folder_state(ui, &current_dir, &c, false);
                    return;
                }
                for (i, entry) in entries.iter().enumerate() {
                    let selected = sel == Some(i);
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
                        self.focus_tab(tab_id);
                        self.set_main_selection(tab_id, i);
                    }
                    if response.double_clicked() {
                        self.focus_tab(tab_id);
                        let path = entry.path.clone();
                        let kind = entry.kind;
                        if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                            self.navigate_tab_to(tab_id, path);
                        } else {
                            let _ = self.platform.reveal_in_system(&path);
                        }
                        return;
                    }
                    self.entry_context_menu(
                        &response,
                        entry,
                        &current_dir,
                        &mut pending_op,
                        &mut open_parent,
                        &mut open_entry_now,
                        &mut open_tandem_now,
                        &mut add_bookmark,
                        &mut remove_bookmark,
                    );

                    // Row content
                    let mut x = row_rect.min.x + 8.0;
                    let y = row_rect.center().y;
                    let style = style_for_entry(
                        entry,
                        EntryVisualState {
                            selected,
                            focused: true,
                            hovered: response.hovered(),
                            opened: false,
                            symlink_dir: matches!(entry.kind, EntryKind::Symlink)
                                && entry.symlink_target_is_dir == Some(true),
                        },
                        &c,
                        &self.state.config,
                    );

                    for (idx, col) in visible_cols.iter().enumerate() {
                        let w = col_widths[idx];
                        match col {
                            ListColumn::Name => {
                                let icon_str = material_icon(style.icon);
                                ui.painter().text(
                                    egui::Pos2::new(x + 6.0, y),
                                    Align2::LEFT_CENTER,
                                    icon_str,
                                    FontId::proportional(icon_size),
                                    style.icon_color,
                                );
                                ui.painter().text(
                                    egui::Pos2::new(x + 28.0, y),
                                    Align2::LEFT_CENTER,
                                    &entry.name,
                                    FontId::proportional(name_size),
                                    style.text_color,
                                );
                            }
                            ListColumn::Kind => {
                                ui.painter().text(
                                    egui::Pos2::new(x + 2.0, y),
                                    Align2::LEFT_CENTER,
                                    entry_kind_label(entry.kind),
                                    FontId::proportional(meta_size),
                                    c.text_dim,
                                );
                            }
                            ListColumn::Size => {
                                let size_str = match entry.kind {
                                    EntryKind::Directory => "—".to_string(),
                                    _ => entry.size_bytes.map(format_size).unwrap_or_default(),
                                };
                                ui.painter().text(
                                    egui::Pos2::new(x + 2.0, y),
                                    Align2::LEFT_CENTER,
                                    size_str,
                                    FontId::proportional(meta_size),
                                    c.text_dim,
                                );
                            }
                            ListColumn::Modified => {
                                let mod_str = entry
                                    .modified_unix_secs
                                    .map(format_modified)
                                    .unwrap_or_default();
                                ui.painter().text(
                                    egui::Pos2::new(x + 2.0, y),
                                    Align2::LEFT_CENTER,
                                    mod_str,
                                    FontId::proportional(meta_size),
                                    c.text_dim,
                                );
                            }
                        }
                        x += w;
                    }
                }
            });
        if let Some(cmd) = pending_op {
            self.run_file_op(cmd, Some(tab_id));
        }
        if let Some(dir) = open_parent {
            self.navigate_tab_to(tab_id, dir);
        }
        if let Some((p, k)) = open_entry_now {
            self.open_entry(p, k);
        }
        if let Some(path) = open_tandem_now {
            self.open_directory_in_tandem(path);
        }
        if let Some(path) = add_bookmark {
            self.add_bookmark_for_path(path);
        }
        if let Some(path) = remove_bookmark {
            self.remove_bookmark_for_path(&path);
        }
    }

    fn render_grid(&mut self, ui: &mut egui::Ui, tab_id: u64) {
        let c = self.colors;
        let current_dir = self
            .state
            .tab_by_id(tab_id)
            .map(|t| t.current_dir.clone())
            .unwrap_or_else(default_home_dir);
        let mut pending_op: Option<FileCommand> = None;
        let mut open_parent: Option<PathBuf> = None;
        let mut open_entry_now: Option<(PathBuf, EntryKind)> = None;
        let mut open_tandem_now: Option<PathBuf> = None;
        let mut add_bookmark: Option<PathBuf> = None;
        let mut remove_bookmark: Option<PathBuf> = None;
        let (entries, loading, error, sel) = {
            let s = self.tab_ui.entry(tab_id).or_default();
            (
                s.main.entries.clone(),
                s.main.loading,
                s.main.error.clone(),
                s.main_sel,
            )
        };
        if loading {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("Loading…").color(c.text_muted));
            });
            return;
        }
        if let Some(err) = error {
            let dir = self.tab_ui.entry(tab_id).or_default().main.dir.clone();
            let retry = render_listing_error(ui, &c, &err, true);
            if retry {
                let _ = self.retry_list_with_privileges(tab_id, dir);
            }
            return;
        }
        if entries.is_empty() {
            self.render_empty_folder_state(ui, &current_dir, &c, false);
            return;
        }

        let scale = self.view_scale_for(ViewMode::Grid);
        // Shared view scale drives grid density too.
        let icon_px = (48.0 * scale).round();
        let label_sz = (11.0 * scale.sqrt()).max(9.0);
        let label_chars = ((item_chars_for_scale(scale)) as usize).max(6);
        // Cell width = icon + padding; ensure wide enough for truncated label
        let cell_inner = (icon_px * 1.18).max(84.0);
        let padding = (10.0 * scale).round().max(8.0);
        let side_padding = (padding * 1.2).max(18.0);
        let available_w = ui.available_width();
        let usable_w = (available_w - side_padding * 2.0).max(cell_inner + padding);
        let cols = ((usable_w / (cell_inner + padding)) as usize).max(1);
        self.last_grid_cols = cols;

        ScrollArea::vertical()
            .id_salt(("grid", tab_id))
            .show(ui, |ui| {
                ui.add_space(padding * 0.5);
                let mut i = 0;
                while i < entries.len() {
                    ui.horizontal(|ui| {
                        ui.add_space(side_padding);
                        for col in 0..cols {
                            if i + col >= entries.len() {
                                break;
                            }
                            let entry = &entries[i + col];
                            let idx = i + col;
                            let selected = sel == Some(idx);
                            let name = truncate_name(&entry.name, label_chars);

                            // Allocate the entire cell rect for hover/click sensing
                            let icon_gap = (8.0 * scale.clamp(0.9, 1.35)).round();
                            let cell_h = icon_px + icon_gap + label_sz * 2.2 + 18.0;
                            let (cell_rect, cell_resp) = ui
                                .allocate_exact_size(Vec2::new(cell_inner, cell_h), Sense::click());
                            let is_hovered = cell_resp.hovered();
                            let style = style_for_entry(
                                entry,
                                EntryVisualState {
                                    selected,
                                    focused: true,
                                    hovered: is_hovered,
                                    opened: false,
                                    symlink_dir: matches!(entry.kind, EntryKind::Symlink)
                                        && entry.symlink_target_is_dir == Some(true),
                                },
                                &c,
                                &self.state.config,
                            );
                            let icon = material_icon(style.icon);
                            let icon_color = style.icon_color;
                            let label_color = style.text_color;

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
                                    cell_rect,
                                    8.0,
                                    Stroke::new(1.0, c.accent),
                                    egui::StrokeKind::Middle,
                                );
                            }

                            // Icon centered horizontally in cell
                            let icon_y = cell_rect.min.y + (8.0 * scale.clamp(0.9, 1.3));
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
                                egui::Pos2::new(icon_cx, icon_y + icon_px + icon_gap),
                                Align2::CENTER_TOP,
                                &name,
                                FontId::proportional(label_sz),
                                label_color,
                            );

                            // Handle click/double-click
                            if cell_resp.clicked() {
                                self.focus_tab(tab_id);
                                self.set_main_selection(tab_id, idx);
                            }
                            if cell_resp.double_clicked() {
                                self.focus_tab(tab_id);
                                let path = entry.path.clone();
                                let kind = entry.kind;
                                if matches!(kind, EntryKind::Directory | EntryKind::Symlink) {
                                    self.navigate_tab_to(tab_id, path);
                                } else {
                                    let _ = self.platform.reveal_in_system(&path);
                                }
                            }
                            self.entry_context_menu(
                                &cell_resp,
                                entry,
                                &current_dir,
                                &mut pending_op,
                                &mut open_parent,
                                &mut open_entry_now,
                                &mut open_tandem_now,
                                &mut add_bookmark,
                                &mut remove_bookmark,
                            );

                            ui.add_space(padding);
                        }
                        ui.add_space(side_padding);
                    });
                    i += cols;
                    ui.add_space(padding * 0.25);
                }
            });
        if let Some(cmd) = pending_op {
            self.run_file_op(cmd, Some(tab_id));
        }
        if let Some(dir) = open_parent {
            self.navigate_tab_to(tab_id, dir);
        }
        if let Some((p, k)) = open_entry_now {
            self.open_entry(p, k);
        }
        if let Some(path) = open_tandem_now {
            self.open_directory_in_tandem(path);
        }
        if let Some(path) = add_bookmark {
            self.add_bookmark_for_path(path);
        }
        if let Some(path) = remove_bookmark {
            self.remove_bookmark_for_path(&path);
        }
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
        if self.preview.data.is_none() {
            let kind_hint = if path.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            self.preview.data = Some(load_preview(&PreviewRequest {
                path: path.clone(),
                kind_hint,
            }));
        }
        let preview = match self.preview.data.clone() {
            Some(p) => p,
            None => return,
        };
        let name = preview.title.as_str();

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
                    .corner_radius(10.0),
            )
            .show(ctx, |ui| {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    // Header
                    ui.horizontal(|ui| {
                        let icon = file_icon_for_path(&path);
                        ui.label(RichText::new(icon).size(18.0));
                        ui.label(RichText::new(name).color(c.text).size(14.0));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Label::new(
                                        RichText::new("X").color(c.text_muted).size(16.0),
                                    )
                                    .sense(Sense::click()),
                                )
                                .clicked()
                            {
                                self.preview.visible = false;
                            }
                        });
                    });
                    ui.separator();

                    ScrollArea::vertical().show(ui, |ui| {
                        match preview.kind {
                            PreviewKind::Text => {
                                let is_svg = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .map(|e| e.eq_ignore_ascii_case("svg"))
                                    .unwrap_or(false);
                                if is_svg {
                                    let max_w = (ui.available_width() - 24.0).max(120.0);
                                    let max_h = (ui.available_height() * 0.7).max(160.0);
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(8.0);
                                        if let Some(img) =
                                            self.cached_image_preview_widget(&path, max_w, max_h, 8)
                                        {
                                            ui.add(img);
                                            if !preview.body.is_empty() {
                                                ui.add_space(8.0);
                                                ui.label(
                                                    RichText::new(&preview.body)
                                                        .color(c.text_dim)
                                                        .size(11.5),
                                                );
                                            }
                                        }
                                    });
                                }
                                let mut text = preview.body.clone();
                                ui.add(
                                    egui::TextEdit::multiline(&mut text)
                                        .font(FontId::monospace(12.0))
                                        .text_color(c.text)
                                        .frame(false)
                                        .desired_rows(30)
                                        .interactive(false),
                                );
                            }
                            PreviewKind::Image => {
                                let max_w = (ui.available_width() - 24.0).max(120.0);
                                let max_h = (ui.available_height() * 0.7).max(160.0);
                                ui.vertical_centered(|ui| {
                                    ui.add_space(8.0);
                                    if let Some(img) =
                                        self.cached_image_preview_widget(&path, max_w, max_h, 8)
                                    {
                                        ui.add(img);
                                    } else {
                                        ui.label(
                                            RichText::new("Image preview unavailable")
                                                .color(c.text_dim)
                                                .size(11.0),
                                        );
                                    }
                                    ui.add_space(10.0);
                                    if !preview.body.is_empty() {
                                        ui.label(
                                            RichText::new(&preview.body)
                                                .color(c.text_dim)
                                                .size(11.5),
                                        );
                                    }
                                });
                            }
                            PreviewKind::ArchiveMetadata => {
                                // Rich archive card: header + scrollable file listing
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(MI_FOLDER_ZIP).size(36.0).color(c.folder),
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(name).color(c.text).size(14.0).strong(),
                                        );
                                        // First lines are summary (before "Contents:")
                                        let body = &preview.body;
                                        let (header, _listing) =
                                            if let Some(pos) = body.find("\nContents:\n") {
                                                (
                                                    &body[..pos],
                                                    Some(&body[pos + "\nContents:\n".len()..]),
                                                )
                                            } else {
                                                (body.as_str(), None)
                                            };
                                        ui.label(
                                            RichText::new(header.trim())
                                                .color(c.text_dim)
                                                .size(11.5),
                                        );
                                    });
                                });
                                // File listing in monospace
                                let body = &preview.body;
                                if let Some(pos) = body.find("\nContents:\n") {
                                    let listing = &body[pos + "\nContents:\n".len()..];
                                    if !listing.is_empty() {
                                        ui.add_space(6.0);
                                        ui.separator();
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new("Contents")
                                                .color(c.text_muted)
                                                .size(11.0),
                                        );
                                        let mut text = listing.to_string();
                                        ui.add(
                                            egui::TextEdit::multiline(&mut text)
                                                .font(FontId::monospace(11.0))
                                                .text_color(c.text_dim)
                                                .frame(false)
                                                .desired_rows(16)
                                                .interactive(false),
                                        );
                                    }
                                }
                                ui.add_space(12.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    if ui.button("Open with default app").clicked() {
                                        let _ = self.platform.reveal_in_system(&path);
                                    }
                                });
                            }
                            PreviewKind::AudioMetadata => {
                                // Rich audio card: icon + structured metadata rows
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(MI_MUSIC_NOTE).size(36.0).color(c.folder),
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(name).color(c.text).size(14.0).strong(),
                                        );
                                    });
                                });
                                ui.add_space(8.0);
                                // Render each metadata line as a key-value row
                                for line in preview.body.lines() {
                                    if let Some((key, val)) = line.split_once(':') {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{key}:"))
                                                    .color(c.text_muted)
                                                    .size(11.5),
                                            );
                                            ui.label(
                                                RichText::new(val.trim()).color(c.text).size(11.5),
                                            );
                                        });
                                    } else if !line.trim().is_empty() {
                                        ui.label(RichText::new(line).color(c.text_dim).size(11.5));
                                    }
                                }
                                ui.add_space(16.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    if ui.button("Open with default app").clicked() {
                                        let _ = self.platform.reveal_in_system(&path);
                                    }
                                });
                            }
                            PreviewKind::Pdf => {
                                // Rich PDF card: icon + structured metadata
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(MI_PICTURE_PDF).size(36.0).color(c.folder),
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(name).color(c.text).size(14.0).strong(),
                                        );
                                    });
                                });
                                ui.add_space(8.0);
                                for line in preview.body.lines() {
                                    if let Some((key, val)) = line.split_once(':') {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{key}:"))
                                                    .color(c.text_muted)
                                                    .size(11.5),
                                            );
                                            ui.label(
                                                RichText::new(val.trim()).color(c.text).size(11.5),
                                            );
                                        });
                                    } else if !line.trim().is_empty() {
                                        ui.label(RichText::new(line).color(c.text).size(11.5));
                                    }
                                }
                                ui.add_space(16.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    if ui.button("Open with default app").clicked() {
                                        let _ = self.platform.reveal_in_system(&path);
                                    }
                                });
                            }
                            PreviewKind::VideoMetadata | PreviewKind::OfficeMetadata => {
                                ui.add_space(20.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    let icon = match preview.kind {
                                        PreviewKind::VideoMetadata => MI_MOVIE,
                                        PreviewKind::OfficeMetadata => MI_DESCRIPTION,
                                        _ => MI_FILE,
                                    };
                                    ui.label(RichText::new(icon).size(58.0).color(c.folder));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(name).color(c.text).size(14.0));
                                    if !preview.body.is_empty() {
                                        ui.add_space(8.0);
                                        for line in preview.body.lines() {
                                            if let Some((key, val)) = line.split_once(':') {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(format!("{key}:"))
                                                            .color(c.text_muted)
                                                            .size(11.5),
                                                    );
                                                    ui.label(
                                                        RichText::new(val.trim())
                                                            .color(c.text)
                                                            .size(11.5),
                                                    );
                                                });
                                            } else if !line.trim().is_empty() {
                                                ui.label(
                                                    RichText::new(line)
                                                        .color(c.text_dim)
                                                        .size(11.5),
                                                );
                                            }
                                        }
                                    }
                                    ui.add_space(16.0);
                                    if ui.button("Open with default app").clicked() {
                                        let _ = self.platform.reveal_in_system(&path);
                                    }
                                });
                            }
                            _ => {
                                ui.add_space(20.0);
                                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                                    let icon = file_icon_for_path(&path);
                                    ui.label(RichText::new(icon).size(64.0));
                                    ui.add_space(8.0);
                                    ui.label(RichText::new(name).color(c.text).size(14.0));
                                    if !preview.body.is_empty() {
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new(&preview.body)
                                                .color(c.text_dim)
                                                .size(11.5),
                                        );
                                    }
                                    if preview.open_external {
                                        ui.add_space(16.0);
                                        if ui.button("Open with default app").clicked() {
                                            let _ = self.platform.reveal_in_system(&path);
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
            });
    }

    fn render_properties_popup(&mut self, ctx: &Context) {
        if !self.properties.visible {
            return;
        }
        let path = match self.properties.path.clone() {
            Some(p) => p,
            None => return,
        };
        let c = self.colors;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("—")
            .to_string();
        let is_dir = path.is_dir();

        egui::Window::new("properties_popup")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(320.0)
            .frame(
                Frame::window(&ctx.style())
                    .fill(c.panel_raised)
                    .stroke(Stroke::new(1.0, c.border))
                    .corner_radius(10.0),
            )
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    let sem = semantic_for_path(&path);
                    let style = style_for_semantic(
                        &sem,
                        EntryVisualState {
                            selected: true,
                            focused: true,
                            hovered: false,
                            opened: is_dir,
                            symlink_dir: matches!(sem.category, FileCategory::Directory)
                                && path.is_symlink(),
                        },
                        &c,
                        &semantic_palette(&self.state.config, &c),
                        self.state.config.theme_preset,
                        self.state.config.colorize_file_types,
                        self.state.config.colorize_folder_labels,
                    );
                    let icon = material_icon(style.icon);
                    let icon_col = style.icon_color;
                    ui.label(RichText::new(icon).size(18.0).color(icon_col));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Properties")
                            .color(c.text)
                            .size(13.0)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Label::new(
                                    RichText::new(MI_CLOSE).color(c.text_muted).size(16.0),
                                )
                                .sense(Sense::click()),
                            )
                            .clicked()
                        {
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
                            ui.label(
                                RichText::new(if meta.is_dir() { "Folder" } else { "File" })
                                    .color(vc)
                                    .size(11.5),
                            );
                        });
                    });
                    if meta.is_file() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Size").color(lc).size(11.5));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format_size(meta.len())).color(vc).size(11.5),
                                );
                            });
                        });
                    }
                    if let Ok(modified) = meta.modified() {
                        use std::time::UNIX_EPOCH;
                        if let Ok(dur) = modified.duration_since(UNIX_EPOCH) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Modified").color(lc).size(11.5));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(format_modified(dur.as_secs()))
                                            .color(vc)
                                            .size(11.5),
                                    );
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
                    ui.label(
                        RichText::new(path.display().to_string())
                            .color(vc)
                            .size(10.5),
                    );
                });
                ui.add_space(4.0);
            });
    }

    fn render_about_popup(&mut self, ctx: &Context) {
        if !self.show_about {
            return;
        }
        let c = self.colors;
        egui::Window::new("about_ottrin")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .default_width(280.0)
            .frame(
                Frame::window(&ctx.style())
                    .fill(c.panel_raised)
                    .stroke(Stroke::new(1.0, c.border))
                    .corner_radius(12.0),
            )
            .show(ctx, |ui| {
                ui.with_layout(Layout::top_down(Align::Center), |ui| {
                    ui.add_space(20.0);
                    ui.label(RichText::new(MI_FOLDER).size(44.0).color(c.accent));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Ottrin").color(c.text).size(22.0).strong());
                    ui.label(RichText::new("File Manager").color(c.text_muted).size(12.0));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Version 0.1.0  ·  Pre-release")
                            .color(c.text_dim)
                            .size(11.5),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Built with Rust + egui 0.33")
                            .color(c.text_muted)
                            .size(10.5),
                    );
                    ui.add_space(12.0);
                    ui.add(egui::Separator::default().horizontal().spacing(0.0));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Author").color(c.text_muted).size(11.5));
                        ui.add_space(4.0);
                        ui.hyperlink_to(RichText::new("hoozter").size(11.5), "https://hoozter.com");
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(MI_CODE).size(13.0).color(c.text_muted));
                        ui.add_space(2.0);
                        ui.hyperlink_to(
                            RichText::new("github.com/hoozter/ottrin").size(11.0),
                            "https://github.com/hoozter/ottrin",
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
        if !self.show_settings {
            return;
        }

        let mut close_requested = false;
        let builder = egui::ViewportBuilder::default()
            .with_title("Ottrin Settings")
            .with_inner_size([920.0, 640.0])
            .with_min_inner_size([760.0, 520.0])
            .with_decorations(false)
            .with_resizable(true);
        let vp_id = Self::settings_viewport_id();

        ctx.show_viewport_immediate(vp_id, builder, |settings_ctx, class| {
            self.apply_theme_to_ctx(settings_ctx);
            if settings_ctx.input(|i| i.viewport().close_requested()) {
                close_requested = true;
                return;
            }
            let _ = class;
            if self.render_settings_modal_embedded(settings_ctx) {
                self.show_settings = false;
                settings_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                close_requested = true;
                return;
            }
            self.handle_viewport_resize(settings_ctx);
        });

        if close_requested {
            self.show_settings = false;
            ctx.send_viewport_cmd_to(Self::settings_viewport_id(), egui::ViewportCommand::Close);
        }
    }

    fn settings_viewport_id() -> egui::ViewportId {
        egui::ViewportId::from_hash_of("ottrin-settings")
    }

    fn render_settings_modal_embedded(&mut self, ctx: &Context) -> bool {
        if !self.show_settings {
            return false;
        }
        let c = self.colors;
        // Apply scroll style to this viewport's own ctx (separate from main ctx)
        ctx.style_mut(|style| {
            let mut scroll = egui::style::ScrollStyle::solid();
            scroll.floating = false;
            scroll.bar_width = 8.0;
            scroll.handle_min_length = 36.0;
            scroll.bar_outer_margin = 2.0;
            scroll.bar_inner_margin = 1.0;
            scroll.dormant_background_opacity = 0.15;
            scroll.active_background_opacity = 0.25;
            scroll.interact_background_opacity = 0.35;
            scroll.dormant_handle_opacity = 0.45;
            scroll.active_handle_opacity = 0.7;
            scroll.interact_handle_opacity = 0.9;
            style.spacing.scroll = scroll;
        });
        let mut close_requested = false;
        let viewport_focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        let (window_fill, window_title, shadow) = viewport_window_fill(c, viewport_focused);
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(window_fill)
                    .stroke(Stroke::new(c.window_border_width, c.window_border))
                    .shadow(shadow)
                    .inner_margin(egui::Margin::same(c.window_border_width.ceil() as i8)),
            )
            .show(ctx, |ui| {
                render_custom_titlebar(
                    ui,
                    ctx,
                    c,
                    window_title,
                    &format!("{} Settings", MI_SETTINGS),
                    true,
                    &mut close_requested,
                    |_ui| {},
                );

                // ── Index summary strip removed (kept only on Search page) ───
                let full_rect = ui.available_rect_before_wrap();
                let summary_h = 0.0;

                // ── Two-panel body ───────────────────────────────────────────
                let body_rect = Rect::from_min_max(
                    egui::pos2(full_rect.min.x, full_rect.min.y + summary_h + 6.0),
                    full_rect.max,
                );
                let sidebar_w = 132.0;

                // Left sidebar
                let sidebar_rect = Rect::from_min_size(body_rect.min, Vec2::new(sidebar_w, body_rect.height()));
                ui.scope_builder(UiBuilder::new().max_rect(sidebar_rect), |ui| {
                    Frame::new()
                        .fill(mix_color(c.panel_raised, c.panel, 0.45))
                        .inner_margin(egui::Margin { left: 0, right: 0, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            let groups: &[SettingsNavGroup] = &[
                                ("Preferences", &[
                                    (SettingsTab::General,    MI_TUNE,        "General"),
                                    (SettingsTab::Appearance, MI_DARK_MODE,   "Appearance"),
                                    (SettingsTab::Files,      MI_FOLDER,      "Files"),
                                ]),
                                ("Search", &[
                                    (SettingsTab::Search,     MI_SEARCH,      "Search"),
                                    (SettingsTab::Cache,      MI_DESCRIPTION, "Cache"),
                                ]),
                            ];
                            let _diag = self.search_service.diagnostics();

                            for (group_label, items) in groups {
                                ui.add_space(2.0);
                                ui.label(RichText::new(*group_label).size(10.0).color(c.text_muted));
                                ui.add_space(4.0);
                                for (tab, icon, label) in *items {
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
                                                ui.add_space(5.0);
                                                ui.label(RichText::new(*label).size(12.0).color(text_col));
                                                if *tab == SettingsTab::Search {
                                                    // No index status indicator here — keep all progress in Search page only.
                                                }
                                            });
                                            ui.add_space(1.0);
                                        });
                                    let r = ui.interact(resp.response.rect, ui.id().with(label), Sense::click());
                                    if r.hovered() {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    if r.clicked() { self.settings_tab = *tab; }
                                    if active {
                                        ui.painter().vline(sidebar_rect.min.x + 2.0, resp.response.rect.y_range(), Stroke::new(2.0, c.accent));
                                    }
                                }
                                ui.add_space(6.0);
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
                        .fill(mix_color(c.panel, c.bg, 0.35))
                        .stroke(Stroke::NONE)
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 14 })
                        .show(ui, |ui| {
                            let settings_title = 15.0;
                            let settings_subtitle = 11.5;
                            let content_w = ui.available_width().max(0.0);
                            ui.set_width(content_w);
                            ui.set_max_width(content_w);
                            ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            let content_w = (ui.available_width() - 12.0).max(0.0);
                            ui.set_max_width(content_w);
                            match self.settings_tab {
                                SettingsTab::General => {
                                    ui.label(RichText::new("General").color(c.text).size(settings_title).strong());
                                    ui.label(RichText::new("Default behavior and integrated privilege management.")
                                        .color(c.text_muted).size(settings_subtitle));
                                    ui.add_space(12.0);
                                    ui.label(RichText::new("Default view").color(c.text_dim).size(12.0));
                                    ui.add_space(6.0);
                                    let view_opts = [
                                        (ViewMode::Miller, "Column view", MI_VIEW_COLUMN),
                                        (ViewMode::List,   "List view",   MI_VIEW_LIST),
                                        (ViewMode::Grid,   "Grid view",   MI_APPS),
                                    ];
                                    for (mode, label, icon_mi) in &view_opts {
                                        let sel = self.state.config.default_view_mode == *mode;
                                        let check = if sel { MI_CHECK } else { " " };
                                        if ui.add(egui::Button::new(
                                            RichText::new(format!("{}  {}  {}", check, icon_mi, label)).size(13.0)
                                                .color(if sel { c.text } else { c.text_dim })
                                        ).frame(true).min_size(Vec2::new(ui.available_width(), 28.0))).clicked() {
                                            self.state.config.default_view_mode = *mode;
                                        }
                                    }
                                    ui.add_space(10.0);
                                    ui.label(RichText::new("Tip: per-folder view presets are planned.")
                                        .color(c.text_muted).size(11.0));

                                    ui.add_space(14.0);
                                    ui.separator();
                                    ui.add_space(10.0);
                                    ui.label(RichText::new("Integrated Privilege Management").color(c.text).size(13.0).strong());
                                    let (status_text, status_color) = match &self.privileged_availability {
                                        PrivilegedAvailability::Ready => ("Ready", c.accent),
                                        PrivilegedAvailability::Misconfigured(_) => ("Needs setup", c.error),
                                        PrivilegedAvailability::Unsupported(_) => ("Unavailable", c.text_muted),
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(status_text).color(status_color).size(11.5).strong());
                                        let detail = match &self.privileged_availability {
                                            PrivilegedAvailability::Ready => "The helper and elevation backend are configured.",
                                            PrivilegedAvailability::Misconfigured(msg) => msg.as_str(),
                                            PrivilegedAvailability::Unsupported(msg) => msg.as_str(),
                                        };
                                        ui.label(RichText::new(detail).color(c.text_muted).size(11.0));
                                    });
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("Normally auto-discovered from bundled install. Developer override: set `OTTRIN_PRIV_HELPER` or launch with `ottrin-app --helper-path=/path/to/ottrin-priv-helper`.")
                                            .color(c.text_muted)
                                            .size(11.0),
                                    );
                                }
                                SettingsTab::Appearance => {
                                    self.render_appearance_settings(ui, ctx, c);
                                }
                                SettingsTab::Files => {
                                    ui.label(RichText::new("Files").color(c.text).size(settings_title).strong());
                                    ui.label(RichText::new("Defaults for file and folder visibility.")
                                        .color(c.text_muted).size(settings_subtitle));
                                    ui.add_space(12.0);
                                    let show_hidden = self.state.config.show_hidden_files;
                                    let icon = if show_hidden { MI_CHECK } else { "  " };
                                    if ui.add(egui::Button::new(
                                        RichText::new(format!("{} Show hidden files", icon)).size(13.0)
                                            .color(if show_hidden { c.text } else { c.text_dim })
                                    ).frame(true).min_size(Vec2::new(ui.available_width(), 28.0))).clicked() {
                                        self.state.config.show_hidden_files = !show_hidden;
                                        self.refresh_active_tab();
                                    }
                                }
                                SettingsTab::Search => {
                                    self.ensure_search_started();
                                    // ── Header ────────────────────────────────
                                    ui.label(RichText::new("Search").color(c.text).size(settings_title).strong());
                                    ui.label(RichText::new(
                                        "Control where Ottrin looks, what to skip, and how the index stays current."
                                    ).color(c.text_muted).size(settings_subtitle));
                                    ui.add_space(12.0);
                                    let ok_col = Color32::from_rgb(80, 185, 110);
                                    let warn_col = Color32::from_rgb(214, 167, 78);
                                    settings_section_icon(ui, c, MI_SEARCH, "Index status", "Live health, progress, and rebuild controls.", |ui| {
                                        let diag = self.search_service.diagnostics();
                                        let is_indexing = matches!(diag.status, SearchIndexStatus::Indexing);
                                        let ready_col = ok_col;
                                        let (mut dot_col, mut status_word) = match diag.status {
                                            SearchIndexStatus::Indexing   => (c.accent, "Indexing"),
                                            SearchIndexStatus::Ready      => (ready_col, "Ready"),
                                            SearchIndexStatus::Unavailable => (c.error, "Unavailable"),
                                        };
                                        let est_total = diag.last_completed_indexed_items
                                            .or(diag.estimated_total_items)
                                            .filter(|t| *t > 0);
                                        let pct = est_total.map(|total| {
                                            let raw = (diag.indexed_items as f32 / total as f32).min(0.99);
                                            (raw * 100.0).round().min(99.0)
                                        });
                                        let progress_caption = if let Some(total) = est_total {
                                            format!("{} / {} files indexed", diag.indexed_items, total)
                                        } else {
                                            format!("{} files indexed", diag.indexed_items)
                                        };
                                        let count_line = if is_indexing {
                                            progress_caption.clone()
                                        } else if matches!(diag.status, SearchIndexStatus::Ready) {
                                            if let Some(total) = est_total {
                                                format!("{} / {} files indexed", diag.indexed_items, total)
                                            } else {
                                                format!("{} files indexed", diag.indexed_items)
                                            }
                                        } else {
                                            diag.last_error.clone().unwrap_or_else(|| "Unavailable".to_string())
                                        };
                                        let now_secs = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_secs()).unwrap_or(0);
                                        let last_progress_age = diag.last_progress_unix_secs.map(|t| now_secs.saturating_sub(t));
                                        let stalled = is_indexing && last_progress_age.map(|age| age > 600).unwrap_or(false);
                                        if stalled {
                                            dot_col = warn_col;
                                            status_word = "Stalled";
                                        }
                                        let second_line: Option<String> = if is_indexing {
                                            diag.last_indexed_path
                                                .as_ref()
                                                .or(diag.active_root.as_ref())
                                                .map(|p| p.display().to_string())
                                        } else {
                                            None
                                        };
                                        let status_pct = if matches!(diag.status, SearchIndexStatus::Ready) {
                                            Some(100)
                                        } else {
                                            pct.map(|p| p as i32)
                                        };
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Status:").color(c.text_dim).size(11.0));
                                            ui.add_space(6.0);
                                            if is_indexing {
                                                ui.add(egui::Spinner::new().color(dot_col).size(12.0));
                                            } else {
                                                let (dot_r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                                                ui.painter().circle_filled(dot_r.center(), 4.0, dot_col);
                                            }
                                            ui.add_space(6.0);
                                            let status_text = if matches!(diag.status, SearchIndexStatus::Ready) {
                                                "Ready"
                                            } else if matches!(diag.status, SearchIndexStatus::Unavailable) {
                                                "Unavailable"
                                            } else {
                                                status_word
                                            };
                                            ui.label(RichText::new(status_text).color(dot_col).size(11.0).strong());
                                            if let Some(pct) = status_pct {
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(format!("{pct}%")).color(dot_col).size(11.0).strong());
                                            }
                                        });
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(&count_line).color(c.text).size(13.0).strong());
                                        ui.add_space(2.0);
                                        let path_full = second_line.unwrap_or_else(|| "—".to_string());
                                        let path_height = 18.0;
                                        let font_id = FontId::proportional(11.0);
                                        let available_w = ui.available_width();
                                        let path_text = Self::middle_ellipsis(ui, &path_full, available_w, &font_id);
                                        let (path_rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(available_w, path_height),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().text(
                                            path_rect.left_top(),
                                            Align2::LEFT_TOP,
                                            path_text,
                                            font_id.clone(),
                                            c.text_muted,
                                        );
                                        if path_full != "—" {
                                            resp.on_hover_text(path_full);
                                        }
                                        // No horizontal progress bar; keep a single status + percent line.
                                        ui.add_space(10.0);
                                        let btn = egui::Button::new(RichText::new("Rebuild index").size(12.0))
                                            .fill(c.panel_raised)
                                            .stroke(Stroke::NONE)
                                            .min_size(Vec2::new(ui.available_width(), 28.0));
                                        if ui.add_enabled(!is_indexing, btn).clicked() {
                                            self.search_service.rebuild_index();
                                        }
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_FOLDER, "Sources", "Folders Ottrin scans and what it skips.", |ui| {
                                        ui.label(RichText::new("Indexed locations").color(c.text).size(12.5).strong());
                                        ui.add_space(2.0);
                                        ui.label(RichText::new(
                                            "Ottrin keeps an in-memory index of these folders. Subfolders are included automatically."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(8.0);

                                        {
                                            let mut remove_idx: Option<usize> = None;
                                            let n = self.state.config.search.include_roots.len();
                                            Frame::new()
                                                .fill(c.panel)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(6.0)
                                                .inner_margin(egui::Margin::ZERO)
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    if n == 0 {
                                                        Frame::new().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                                                            ui.label(RichText::new(
                                                                "No folders added — Ottrin will not find any files until you add at least one."
                                                            ).color(c.text_muted).italics().size(11.0));
                                                        });
                                                    } else {
                                                        for i in 0..n {
                                                            let p = self.state.config.search.include_roots[i].display().to_string();
                                                            if i > 0 { ui.add(egui::Separator::default().horizontal().spacing(0.0)); }
                                                            let row_fill = if i % 2 == 0 {
                                                                mix_color(c.panel, c.panel_raised, 0.15)
                                                            } else {
                                                                c.panel
                                                            };
                                                            Frame::new()
                                                                .fill(row_fill)
                                                                .inner_margin(egui::Margin { left: 12, right: 6, top: 7, bottom: 7 })
                                                                .show(ui, |ui| {
                                                                ui.horizontal(|ui| {
                                                                    ui.label(RichText::new(MI_FOLDER).color(c.accent).size(14.0));
                                                                    ui.add_space(6.0);
                                                                    ui.label(RichText::new(&p).size(11.5).color(c.text));
                                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                        if ui.add_sized([26.0, 26.0], egui::Button::new(
                                                                            RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)
                                                                        ).frame(false)).clicked() { remove_idx = Some(i); }
                                                                    });
                                                                });
                                                            });
                                                        }
                                                    }
                                                });
                                            if let Some(i) = remove_idx {
                                                self.state.config.search.include_roots.remove(i);
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        }
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if ui.button(format!("  {}  Add folder…", MI_FOLDER_OPEN)).clicked()
                                                && let Some(dir) = rfd::FileDialog::new().pick_folder()
                                            {
                                                self.state.config.search.include_roots.push(dir);
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        });

                                        ui.add_space(12.0);
                                        ui.label(RichText::new("Exclusions").color(c.text_dim).size(11.5).strong());
                                        ui.label(RichText::new(
                                            "Folders and patterns never included in results, even inside indexed locations."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(6.0);

                                        {
                                            let excl_n = self.state.config.search.exclude_roots.len();
                                            let excl_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                                                ui.ctx(), ui.id().with("skip_folders"), true,
                                            );
                                            let excl_open = excl_state.is_open();
                                            excl_state.show_header(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new("Excluded folders").color(c.text).size(12.0));
                                                    ui.add_space(6.0);
                                                    if !excl_open {
                                                        let preview: String = self.state.config.search.exclude_roots.iter()
                                                            .take(4)
                                                            .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()))
                                                            .collect::<Vec<_>>()
                                                            .join("   ");
                                                        let suffix = if excl_n > 4 { format!("   +{}", excl_n - 4) } else { String::new() };
                                                        ui.label(RichText::new(format!("{}{}", preview, suffix)).color(c.text_muted).size(10.5).monospace());
                                                    } else {
                                                        ui.label(RichText::new(format!("{} folder{}", excl_n, if excl_n == 1 { "" } else { "s" })).color(c.text_muted).size(10.5));
                                                    }
                                                });
                                            }).body(|ui| {
                                                ui.add_space(4.0);
                                                let mut remove_idx: Option<usize> = None;
                                                let n = self.state.config.search.exclude_roots.len();
                                            Frame::new()
                                                .fill(c.panel).stroke(Stroke::NONE)
                                                .corner_radius(6.0).inner_margin(egui::Margin::ZERO)
                                                .show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        if n == 0 {
                                                            Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                                                                ui.label(RichText::new("No folders excluded.").color(c.text_muted).italics().size(11.0));
                                                            });
                                                        } else {
                                                            for i in 0..n {
                                                            let p = self.state.config.search.exclude_roots[i].display().to_string();
                                                            if i > 0 { ui.add(egui::Separator::default().horizontal().spacing(0.0)); }
                                                            let row_fill = if i % 2 == 0 {
                                                                mix_color(c.panel, c.panel_raised, 0.15)
                                                            } else {
                                                                c.panel
                                                            };
                                                            Frame::new()
                                                                .fill(row_fill)
                                                                .inner_margin(egui::Margin { left: 12, right: 6, top: 6, bottom: 6 })
                                                                .show(ui, |ui| {
                                                                ui.horizontal(|ui| {
                                                                    ui.label(RichText::new(MI_FOLDER).color(c.text_dim).size(14.0));
                                                                    ui.add_space(4.0);
                                                                    ui.label(RichText::new(&p).size(11.0).color(c.text));
                                                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                            if ui.add_sized([26.0, 26.0], egui::Button::new(RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)).frame(false)).clicked() { remove_idx = Some(i); }
                                                                        });
                                                                    });
                                                                });
                                                            }
                                                        }
                                                    });
                                                if let Some(i) = remove_idx {
                                                    self.state.config.search.exclude_roots.remove(i);
                                                    self.search_service.update_config(self.state.config.search.clone());
                                                }
                                                ui.add_space(4.0);
                                                if ui.button(format!("  {}  Add folder…", MI_FOLDER_OPEN)).clicked()
                                                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                                                {
                                                    self.state.config.search.exclude_roots.push(dir);
                                                    self.search_service.update_config(self.state.config.search.clone());
                                                }
                                                ui.add_space(4.0);
                                            });
                                        }
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_TUNE, "Filters", "Patterns that exclude or limit indexed results.", |ui| {
                                        // File patterns — collapsible
                                        let ig = self.state.config.search.include_globs.len();
                                        let eg = self.state.config.search.exclude_globs.len();
                                        let pat_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(), ui.id().with("skip_patterns"), true,
                                        );
                                        let pat_open = pat_state.is_open();
                                        pat_state.show_header(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("File patterns").color(c.text).size(12.0));
                                                ui.add_space(6.0);
                                                if !pat_open {
                                                    let preview: String = self.state.config.search.exclude_globs.iter()
                                                        .take(4).cloned().collect::<Vec<_>>().join("   ");
                                                    let suffix = if eg > 4 { format!("   +{}", eg - 4) } else { String::new() };
                                                    ui.label(RichText::new(format!("{}{}", preview, suffix)).color(c.text_muted).size(10.5).monospace());
                                                } else {
                                                    let mut parts = Vec::new();
                                                    if eg > 0 { parts.push(format!("{} always skip", eg)); }
                                                    if ig > 0 { parts.push(format!("{} only include", ig)); }
                                                    if parts.is_empty() { parts.push("none".to_string()); }
                                                    ui.label(RichText::new(parts.join(" · ")).color(c.text_muted).size(10.5));
                                                }
                                            });
                                        }).body(|ui| {
                                            ui.add_space(6.0);

                                            // Always skip
                                            ui.label(RichText::new("Always skip").color(c.text_dim).size(11.5).strong());
                                            ui.label(RichText::new("Files and folders matching these patterns are never indexed.")
                                                .color(c.text_muted).size(10.5));
                                            ui.add_space(4.0);
                                            {
                                                let n = self.state.config.search.exclude_globs.len();
                                                let mut remove_idx = None;
                                                Frame::new()
                                                    .fill(c.panel_raised).stroke(Stroke::NONE)
                                                    .corner_radius(6.0).inner_margin(egui::Margin::ZERO)
                                                    .show(ui, |ui| {
                                                        ui.set_width(ui.available_width());
                                                        if n == 0 {
                                                            Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                                                                ui.label(RichText::new("Nothing skipped.").color(c.text_muted).italics().size(11.0));
                                                            });
                                                        } else {
                                                            for i in 0..n {
                                                                let g = self.state.config.search.exclude_globs[i].clone();
                                                                if i > 0 { ui.add(egui::Separator::default().horizontal().spacing(0.0)); }
                                                                let row_fill = if i % 2 == 0 {
                                                                    mix_color(c.panel, c.panel_raised, 0.15)
                                                                } else {
                                                                    c.panel
                                                                };
                                                                Frame::new()
                                                                    .fill(row_fill)
                                                                    .inner_margin(egui::Margin { left: 10, right: 4, top: 5, bottom: 5 })
                                                                    .show(ui, |ui| {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(RichText::new(&g).size(11.0).color(c.text).monospace());
                                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                            if ui.add_sized([24.0, 24.0], egui::Button::new(RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)).frame(false)).clicked() { remove_idx = Some(i); }
                                                                        });
                                                                    });
                                                                });
                                                            }
                                                        }
                                                    });
                                                if let Some(i) = remove_idx {
                                                    self.state.config.search.exclude_globs.remove(i);
                                                    self.search_service.update_config(self.state.config.search.clone());
                                                }
                                            }
                                            ui.add_space(4.0);
                                            if self.search_ui.show_exclude_glob_input {
                                                ui.horizontal(|ui| {
                                                    let resp = ui.add_sized([ui.available_width() - 54.0, 24.0],
                                                        egui::TextEdit::singleline(&mut self.search_ui.exclude_glob_input).hint_text("e.g. *.log"));
                                                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                                    if ui.add_sized([44.0, 24.0], egui::Button::new("Add")).clicked() || enter {
                                                        let g = self.search_ui.exclude_glob_input.trim().to_string();
                                                        if !g.is_empty() {
                                                            self.state.config.search.exclude_globs.push(g);
                                                            self.search_ui.exclude_glob_input.clear();
                                                            self.search_service.update_config(self.state.config.search.clone());
                                                        }
                                                        self.search_ui.show_exclude_glob_input = false;
                                                    }
                                                });
                                            } else if ui.button("+ Add pattern").clicked() {
                                                self.search_ui.show_exclude_glob_input = true;
                                            }

                                            ui.add_space(14.0);

                                            // Only include (optional filter)
                                            ui.label(RichText::new("Only include").color(c.text_dim).size(11.5).strong());
                                            ui.label(RichText::new(
                                                "Restrict results to these file types only. Leave empty to include everything."
                                            ).color(c.text_muted).size(10.5));
                                            ui.add_space(4.0);
                                            {
                                                let mut remove_idx = None;
                                                Frame::new()
                                                    .fill(c.panel_raised).stroke(Stroke::NONE)
                                                    .corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 7))
                                                    .show(ui, |ui| {
                                                        ui.set_min_width(ui.available_width());
                                                        if self.state.config.search.include_globs.is_empty() {
                                                            ui.label(RichText::new("All file types").color(c.text_muted).italics().size(11.0));
                                                        } else {
                                                            ui.horizontal_wrapped(|ui| {
                                                                for i in 0..self.state.config.search.include_globs.len() {
                                                                    let g = self.state.config.search.include_globs[i].clone();
                                                                    Frame::new()
                                                                        .fill(c.panel).stroke(Stroke::NONE)
                                                                        .corner_radius(10.0).inner_margin(egui::Margin { left: 8, right: 4, top: 2, bottom: 2 })
                                                                        .show(ui, |ui| {
                                                                            ui.horizontal(|ui| {
                                                                                ui.label(RichText::new(&g).size(11.0).color(c.text).monospace());
                                                                                if ui.add_sized([16.0, 16.0], egui::Button::new(RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)).frame(false)).clicked() { remove_idx = Some(i); }
                                                                            });
                                                                        });
                                                                }
                                                            });
                                                        }
                                                    });
                                                if let Some(i) = remove_idx {
                                                    self.state.config.search.include_globs.remove(i);
                                                    self.search_service.update_config(self.state.config.search.clone());
                                                }
                                            }
                                            ui.add_space(4.0);
                                            if self.search_ui.show_include_glob_input {
                                                ui.horizontal(|ui| {
                                                    let resp = ui.add_sized([ui.available_width() - 54.0, 24.0],
                                                        egui::TextEdit::singleline(&mut self.search_ui.include_glob_input).hint_text("e.g. *.pdf"));
                                                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                                    if ui.add_sized([44.0, 24.0], egui::Button::new("Add")).clicked() || enter {
                                                        let g = self.search_ui.include_glob_input.trim().to_string();
                                                        if !g.is_empty() {
                                                            self.state.config.search.include_globs.push(g);
                                                            self.search_ui.include_glob_input.clear();
                                                            self.search_service.update_config(self.state.config.search.clone());
                                                        }
                                                        self.search_ui.show_include_glob_input = false;
                                                    }
                                                });
                                            } else if ui.button("+ Add pattern").clicked() {
                                                self.search_ui.show_include_glob_input = true;
                                            }
                                            ui.add_space(4.0);
                                        });
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_DESCRIPTION, "Content indexing (optional)", "Choose which folders can be scanned for file contents.", |ui| {
                                        let before = self.state.config.search.content_indexing_enabled;
                                        ui.checkbox(
                                            &mut self.state.config.search.content_indexing_enabled,
                                            "Enable content indexing (slower, larger index)",
                                        );
                                        if before != self.state.config.search.content_indexing_enabled {
                                            self.search_service.update_config(self.state.config.search.clone());
                                        }
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(
                                            "When enabled, Ottrin can extract file contents for full-text search. Keep this scoped to specific folders you care about."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(10.0);

                                        ui.label(RichText::new("Content sources").color(c.text).size(12.0).strong());
                                        ui.label(RichText::new(
                                            "Only these folders are eligible for content indexing."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(6.0);

                                        {
                                            let mut remove_idx: Option<usize> = None;
                                            let n = self.state.config.search.content_include_roots.len();
                                            Frame::new()
                                                .fill(c.panel)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(6.0)
                                                .inner_margin(egui::Margin::ZERO)
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    if n == 0 {
                                                        Frame::new().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                                                            ui.label(RichText::new(
                                                                "No folders selected — content indexing is disabled by default."
                                                            ).color(c.text_muted).italics().size(11.0));
                                                        });
                                                    } else {
                                                        for i in 0..n {
                                                            let p = self.state.config.search.content_include_roots[i].display().to_string();
                                                            if i > 0 { ui.add(egui::Separator::default().horizontal().spacing(0.0)); }
                                                            let row_fill = if i % 2 == 0 {
                                                                mix_color(c.panel, c.panel_raised, 0.15)
                                                            } else {
                                                                c.panel
                                                            };
                                                            Frame::new()
                                                                .fill(row_fill)
                                                                .inner_margin(egui::Margin { left: 12, right: 6, top: 7, bottom: 7 })
                                                                .show(ui, |ui| {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(RichText::new(MI_FOLDER).color(c.accent).size(14.0));
                                                                        ui.add_space(6.0);
                                                                        ui.label(RichText::new(&p).size(11.5).color(c.text));
                                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                            if ui.add_sized([26.0, 26.0], egui::Button::new(
                                                                                RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)
                                                                            ).frame(false)).clicked() { remove_idx = Some(i); }
                                                                        });
                                                                    });
                                                                });
                                                        }
                                                    }
                                                });
                                            if let Some(i) = remove_idx {
                                                self.state.config.search.content_include_roots.remove(i);
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        }
                                        ui.add_space(4.0);
                                        ui.horizontal(|ui| {
                                            if ui.button(format!("  {}  Add folder…", MI_FOLDER_OPEN)).clicked()
                                                && let Some(dir) = rfd::FileDialog::new().pick_folder()
                                            {
                                                self.state.config.search.content_include_roots.push(dir);
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                            if ui.button("Use indexed locations").clicked() {
                                                self.state.config.search.content_include_roots = self.state.config.search.include_roots.clone();
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        });

                                        ui.add_space(10.0);
                                        ui.label(RichText::new("Content exclusions").color(c.text_dim).size(11.5).strong());
                                        ui.label(RichText::new(
                                            "These folders are skipped even if they live inside the content sources."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(6.0);

                                        {
                                            let mut remove_idx: Option<usize> = None;
                                            let n = self.state.config.search.content_exclude_roots.len();
                                            Frame::new()
                                                .fill(c.panel)
                                                .stroke(Stroke::NONE)
                                                .corner_radius(6.0)
                                                .inner_margin(egui::Margin::ZERO)
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    if n == 0 {
                                                        Frame::new().inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                                                            ui.label(RichText::new(
                                                                "No exclusions — all folders inside your content sources are eligible."
                                                            ).color(c.text_muted).italics().size(11.0));
                                                        });
                                                    } else {
                                                        for i in 0..n {
                                                            let p = self.state.config.search.content_exclude_roots[i].display().to_string();
                                                            if i > 0 { ui.add(egui::Separator::default().horizontal().spacing(0.0)); }
                                                            let row_fill = if i % 2 == 0 {
                                                                mix_color(c.panel, c.panel_raised, 0.15)
                                                            } else {
                                                                c.panel
                                                            };
                                                            Frame::new()
                                                                .fill(row_fill)
                                                                .inner_margin(egui::Margin { left: 12, right: 6, top: 7, bottom: 7 })
                                                                .show(ui, |ui| {
                                                                    ui.horizontal(|ui| {
                                                                        ui.label(RichText::new(MI_FOLDER).color(c.text_dim).size(14.0));
                                                                        ui.add_space(6.0);
                                                                        ui.label(RichText::new(&p).size(11.5).color(c.text));
                                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                                            if ui.add_sized([26.0, 26.0], egui::Button::new(
                                                                                RichText::new(MI_CLOSE).size(10.0).color(c.text_muted)
                                                                            ).frame(false)).clicked() { remove_idx = Some(i); }
                                                                        });
                                                                    });
                                                                });
                                                        }
                                                    }
                                                });
                                            if let Some(i) = remove_idx {
                                                self.state.config.search.content_exclude_roots.remove(i);
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        }
                                        ui.add_space(4.0);
                                        if ui.button(format!("  {}  Add exclusion…", MI_FOLDER_OPEN)).clicked()
                                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                                        {
                                            self.state.config.search.content_exclude_roots.push(dir);
                                            self.search_service.update_config(self.state.config.search.clone());
                                        }
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_VISIBILITY, "Search behavior", "Default scope and visibility when you open search.", |ui| {
                                        Frame::new()
                                            .fill(c.panel)
                                            .corner_radius(6.0)
                                            .inner_margin(egui::Margin::symmetric(12, 10))
                                            .show(ui, |ui| {
                                                ui.set_width(ui.available_width());
                                                ui.label(RichText::new("Default scope").color(c.text_dim).size(11.5));
                                                {
                                                    let mut scope = self.state.config.search.default_scope;
                                                    ui.radio_value(&mut scope, SearchScope::Global, "All indexed locations");
                                                    ui.radio_value(&mut scope, SearchScope::CurrentFolder, "Current folder only");
                                                    if scope != self.state.config.search.default_scope {
                                                        self.state.config.search.default_scope = scope;
                                                        self.search_ui.scope = scope;
                                                        self.search_service.update_config(self.state.config.search.clone());
                                                    }
                                                }
                                                ui.add_space(4.0);
                                                let before_h = self.state.config.search.include_hidden_system;
                                                ui.checkbox(
                                                    &mut self.state.config.search.include_hidden_system,
                                                    "Include hidden files (dot-files)",
                                                ).on_hover_text("When on, files and folders whose names start with '.' appear in results.");
                                                if before_h != self.state.config.search.include_hidden_system {
                                                    self.search_service.update_config(self.state.config.search.clone());
                                                }
                                            });
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_STORAGE, "Indexing & performance", "Background refresh and supplemental sources.", |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Auto-refresh:").color(c.text_dim).size(11.5))
                                                .on_hover_text("How often Ottrin rebuilds the full index while the app is running, in addition to inotify change events.");
                                            let mut h = self.state.config.search.refresh_interval_hours;
                                            egui::ComboBox::new("search_refresh_interval", "")
                                                .selected_text(match h {
                                                    0 => "On change only",
                                                    1 => "Every hour",
                                                    2 => "Every 2 hours",
                                                    4 => "Every 4 hours",
                                                    8 => "Every 8 hours",
                                                    24 => "Daily",
                                                    _ => "Custom",
                                                })
                                                .width(140.0)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut h, 0, "On change only");
                                                    ui.selectable_value(&mut h, 1, "Every hour");
                                                    ui.selectable_value(&mut h, 2, "Every 2 hours");
                                                    ui.selectable_value(&mut h, 4, "Every 4 hours");
                                                    ui.selectable_value(&mut h, 8, "Every 8 hours");
                                                    ui.selectable_value(&mut h, 24, "Daily");
                                                });
                                            if h != self.state.config.search.refresh_interval_hours {
                                                self.state.config.search.refresh_interval_hours = h;
                                                self.search_service.update_config(self.state.config.search.clone());
                                            }
                                        });

                                        ui.add_space(12.0);
                                        ui.label(RichText::new("Supplemental results").color(c.text_dim).size(11.5).strong());
                                        ui.label(RichText::new("Optional sources that fill gaps while the index is still warming up.")
                                            .color(c.text_muted).size(10.5));
                                        ui.add_space(6.0);

                                        {
                                            let locate_ok = ottrin_search::SearchService::system_locate_available();
                                            Frame::new()
                                                .fill(c.panel).corner_radius(6.0)
                                                .inner_margin(egui::Margin::symmetric(12, 10))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    let before = self.state.config.search.use_system_locate;
                                                    ui.add_enabled(locate_ok, egui::Checkbox::new(
                                                        &mut self.state.config.search.use_system_locate,
                                                        "Use system locate database (plocate)",
                                                    ));
                                                    if before != self.state.config.search.use_system_locate {
                                                        self.search_service.update_config(self.state.config.search.clone());
                                                    }
                                                    ui.add_space(2.0);
                                                    ui.label(RichText::new(
                                                        "Fills in extra results during first launch while the index warms up. No privileges needed; has no effect once the index is ready."
                                                    ).color(c.text_muted).size(10.5));
                                                    ui.add_space(4.0);
                                                    if locate_ok {
                                                        ui.label(RichText::new("plocate is installed and available").size(10.5).color(ok_col));
                                                        let sys_age = ottrin_search::SearchService::system_locate_db_age_secs();
                                                        let stale = sys_age.map(|s| s > 7 * 86_400).unwrap_or(false);
                                                        let age_label = sys_age
                                                            .map(|s| format!("System locate DB age: {}", format_age_str(s)))
                                                            .unwrap_or_else(|| "System locate DB age: unknown".to_string());
                                                        ui.label(RichText::new(age_label)
                                                            .size(10.5)
                                                            .color(if stale { warn_col } else { c.text_muted }));
                                                        let sys_sched = ottrin_search::SearchService::system_locate_schedule_status();
                                                        if sys_sched.needs_setup {
                                                            ui.label(RichText::new(
                                                                "System updatedb schedule not detected — results may be stale. Recommended: enable a system updatedb schedule in Advanced."
                                                            ).size(10.5).color(warn_col));
                                                            if ottrin_search::SearchService::updatedb_available()
                                                                && ui.add(egui::Button::new(
                                                                    RichText::new("Install daily updatedb cron job via pkexec…").size(11.0)
                                                                ).fill(c.panel_raised).stroke(Stroke::new(1.0, c.border))).clicked() {
                                                                    let script = "#!/bin/sh\n/usr/bin/updatedb\n";
                                                                    let cmd = format!(
                                                                        "printf '{}' > /etc/cron.daily/ottrin-updatedb && chmod 755 /etc/cron.daily/ottrin-updatedb",
                                                                        script
                                                                    );
                                                                    let result = std::process::Command::new("pkexec")
                                                                        .args(["sh", "-c", &cmd])
                                                                        .status();
                                                                    let (text, ok) = match result {
                                                                        Ok(s) if s.success() => (
                                                                            "Daily updatedb cron job installed at /etc/cron.daily/ottrin-updatedb.".to_string(),
                                                                            true,
                                                                        ),
                                                                        Ok(s) => (format!("pkexec exited with status {}", s), false),
                                                                        Err(e) => (format!("Failed to run pkexec: {}", e), false),
                                                                    };
                                                                    self.status_message = Some(UiStatusMessage {
                                                                        text, ok,
                                                                        until: Instant::now() + Duration::from_secs(8),
                                                                    });
                                                            }
                                                        } else {
                                                            let detail = match (sys_sched.source.as_deref(), sys_sched.detail.as_deref()) {
                                                                (Some(src), Some(det)) => format!("System schedule detected: {} ({})", src, det),
                                                                (Some(src), None) => format!("System schedule detected: {}", src),
                                                                _ => "System schedule detected".to_string(),
                                                            };
                                                            ui.label(RichText::new(detail).size(10.5).color(c.text_muted));
                                                        }
                                                    } else {
                                                        ui.label(RichText::new("plocate not found — install plocate to enable this").size(10.5).color(c.text_muted).italics());
                                                        ui.label(RichText::new("Recommended dependencies: plocate (fast estimates) and mlocate/updatedb (for scheduled updates).")
                                                            .size(10.5).color(c.text_muted));
                                                    }
                                                });
                                        }

                                        ui.add_space(6.0);
                                        {
                                            let updatedb_ok = ottrin_search::SearchService::updatedb_available();
                                            Frame::new()
                                                .fill(c.panel).corner_radius(6.0)
                                                .inner_margin(egui::Margin::symmetric(12, 10))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    let before_manage = self.state.config.search.manage_locate_db;
                                                    ui.add_enabled(updatedb_ok, egui::Checkbox::new(
                                                        &mut self.state.config.search.manage_locate_db,
                                                        "Build a private locate database",
                                                    ));
                                                    if before_manage != self.state.config.search.manage_locate_db {
                                                        self.search_service.update_config(self.state.config.search.clone());
                                                    }
                                                    ui.add_space(2.0);
                                                    ui.label(RichText::new(
                                                        "Enables instant results on cold launch before the index finishes building. Stores a private file list at ~/.cache/ottrin/locate.db — no root required. Ottrin refreshes it while the app is running; add a schedule to keep it fresh when Ottrin is closed."
                                                    ).color(c.text_muted).size(10.5));
                                                    ui.add_space(4.0);
                                                    if updatedb_ok {
                                                        ui.label(RichText::new("updatedb is installed and available").size(10.5).color(ok_col));
                                                        let sched = ottrin_search::SearchService::locate_schedule_status();
                                                        if sched.enabled {
                                                            ui.label(RichText::new(format!("Schedule: {} ({})", sched.source, sched.detail))
                                                                .size(10.5).color(c.text_muted));
                                                        } else {
                                                            ui.label(RichText::new("No user schedule detected — recommended for when Ottrin is closed.")
                                                                .size(10.5).color(warn_col));
                                                        }
                                                    } else {
                                                        ui.label(RichText::new("updatedb not found — install mlocate to enable this").size(10.5).color(c.text_muted).italics());
                                                    }

                                                if self.state.config.search.manage_locate_db && updatedb_ok {
                                                    ui.add_space(8.0);
                                                    ui.separator();
                                                    ui.add_space(4.0);
                                                    ui.horizontal(|ui| {
                                                        ui.label(RichText::new("Update frequency:").color(c.text_dim).size(11.5));
                                                        let mut h = self.state.config.search.locate_update_hours;
                                                        egui::ComboBox::new("locate_update_freq", "")
                                                            .selected_text(match h {
                                                                0 => "First launch only",
                                                                2 => "Every 2 hours",
                                                                4 => "Every 4 hours",
                                                                6 => "Every 6 hours",
                                                                12 => "Every 12 hours",
                                                                24 => "Daily",
                                                                _ => "Custom",
                                                            })
                                                            .width(140.0)
                                                            .show_ui(ui, |ui| {
                                                                ui.selectable_value(&mut h, 0, "First launch only");
                                                                ui.selectable_value(&mut h, 2, "Every 2 hours");
                                                                ui.selectable_value(&mut h, 4, "Every 4 hours");
                                                                ui.selectable_value(&mut h, 6, "Every 6 hours");
                                                                ui.selectable_value(&mut h, 12, "Every 12 hours");
                                                                ui.selectable_value(&mut h, 24, "Daily");
                                                            });
                                                        if h != self.state.config.search.locate_update_hours {
                                                            self.state.config.search.locate_update_hours = h;
                                                            self.search_service.update_config(self.state.config.search.clone());
                                                        }
                                                    });
                                                    ui.add_space(4.0);
                                                    ui.horizontal(|ui| {
                                                        let age_label = ottrin_search::SearchService::locate_db_age_secs()
                                                            .map(|s| format!("Last updated {}", format_age_str(s)))
                                                            .unwrap_or_else(|| "Not built yet".to_string());
                                                        ui.label(RichText::new(&age_label).color(c.text_muted).size(11.0));
                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            if ui.button("Update now").clicked() {
                                                                let roots = self.state.config.search.include_roots.clone();
                                                                let root = roots.into_iter().next()
                                                                    .unwrap_or_else(|| ottrin_search::default_roots().into_iter().next().unwrap_or_default());
                                                                ottrin_search::SearchService::run_user_updatedb(root);
                                                            }
                                                        });
                                                    });
                                                    ui.add_space(4.0);
                                                    ui.horizontal(|ui| {
                                                        if ui.button("Set up automatic schedule…").on_hover_text(
                                                            "Installs a systemd user timer or crontab entry that keeps the                                                              locate database fresh on your chosen schedule, even when Ottrin is closed."
                                                        ).clicked() {
                                                            let h = self.state.config.search.locate_update_hours.max(1);
                                                            let roots = self.state.config.search.include_roots.clone();
                                                            let root = roots.into_iter().next()
                                                                .unwrap_or_else(|| ottrin_search::default_roots().into_iter().next().unwrap_or_default());
                                                            let (text, ok) = match ottrin_search::SearchService::install_locate_schedule(h, &root) {
                                                                Ok(msg) => (msg, true),
                                                                Err(e) => (format!("Failed: {}", e), false),
                                                            };
                                                            self.status_message = Some(UiStatusMessage {
                                                                text, ok,
                                                                until: Instant::now() + Duration::from_secs(8),
                                                            });
                                                        }
                                                        ui.label(RichText::new("Works even when Ottrin is closed")
                                                            .color(c.text_muted).size(10.5).italics());
                                                    });
                                                }
                                            });
                                        }
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_SETTINGS, "Advanced", "System-level improvements (optional).", |ui| {
                                        let adv_parent = egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(), ui.id().with("adv_parent"), false,
                                        );
                                        let adv_parent_open = adv_parent.is_open();
                                        adv_parent.show_header(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Advanced (system-level)")
                                                    .color(if adv_parent_open { c.text } else { c.text_dim })
                                                    .size(12.0).strong());
                                                if !adv_parent_open {
                                                    ui.label(RichText::new("updatedb cron · fanotify daemon")
                                                        .color(c.text_muted).size(10.5).italics());
                                                }
                                            });
                                        }).body(|ui| {
                                            ui.add_space(4.0);
                                            ui.label(RichText::new(
                                                "Optional improvements that require elevated privileges. Both are independent — enable either, neither, or both."
                                            ).color(c.text_muted).size(10.5));
                                            ui.add_space(8.0);

                                    // ── Advanced 1: Root updatedb schedule ────
                                    {
                                        let upd_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(), ui.id().with("adv_root_updatedb"), false,
                                        );
                                        let upd_open = upd_state.is_open();
                                        upd_state.show_header(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Keep system locate database current")
                                                    .color(if upd_open { c.text } else { c.text_dim }).size(11.5));
                                                ui.label(RichText::new("recommended")
                                                    .color(c.accent).size(10.0).italics());
                                            });
                                        }).body(|ui| {
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(
                                                "By default, your system's locate database (/var/lib/mlocate/mlocate.db) \
                                                 is only rebuilt occasionally — or not at all on some desktop installs. \
                                                 When it's stale, plocate results are stale too, making the locate \
                                                 supplement much less useful."
                                            ).color(c.text_muted).size(10.5));
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(
                                                "You can schedule a daily root updatedb run to keep it fresh. \
                                                 This is completely independent of Ottrin — it just improves the \
                                                 system-wide locate database that any tool can query."
                                            ).color(c.text_muted).size(10.5));
                                            ui.add_space(8.0);
                                            ui.horizontal(|ui| {
                                                if ui.add(egui::Button::new(
                                                    RichText::new("Install daily updatedb cron job via pkexec…").size(12.0)
                                                ).fill(c.panel_raised).stroke(Stroke::new(1.0, c.border))).on_hover_text(
                                                    "Writes /etc/cron.daily/ottrin-updatedb via pkexec:\n\
                                                     #!/bin/sh\n\
                                                     /usr/bin/updatedb\n\n\
                                                     This rebuilds the system locate database daily as root.\n\
                                                     One-time setup. Remove: sudo rm /etc/cron.daily/ottrin-updatedb"
                                                ).clicked() {
                                                    let script = "#!/bin/sh\n/usr/bin/updatedb\n";
                                                    let cmd = format!(
                                                        "printf '{}' > /etc/cron.daily/ottrin-updatedb && chmod 755 /etc/cron.daily/ottrin-updatedb",
                                                        script
                                                    );
                                                    let result = std::process::Command::new("pkexec")
                                                        .args(["sh", "-c", &cmd])
                                                        .status();
                                                    let (text, ok) = match result {
                                                        Ok(s) if s.success() => (
                                                            "Daily updatedb cron job installed at /etc/cron.daily/ottrin-updatedb.".to_string(),
                                                            true,
                                                        ),
                                                        Ok(s) => (format!("pkexec exited with status {}", s), false),
                                                        Err(e) => (format!("Failed to run pkexec: {}", e), false),
                                                    };
                                                    self.status_message = Some(UiStatusMessage {
                                                        text, ok,
                                                        until: Instant::now() + Duration::from_secs(8),
                                                    });
                                                }
                                                ui.label(RichText::new("Requires admin password")
                                                    .color(c.text_muted).size(10.5).italics());
                                            });
                                            ui.add_space(4.0);
                                            ui.label(RichText::new(
                                                "Alternatively: run  sudo updatedb  manually at any time, or add the \
                                                 line  0 3 * * * root /usr/bin/updatedb  to /etc/crontab yourself."
                                            ).color(c.text_muted).size(10.5).italics());
                                            ui.add_space(6.0);
                                        });
                                    }

                                    ui.add_space(4.0);

                                    // ── Advanced 2: Privileged indexing daemon ─
                                    {
                                        let priv_state = egui::collapsing_header::CollapsingState::load_with_default_open(
                                            ui.ctx(), ui.id().with("adv_priv_indexing"), false,
                                        );
                                        let priv_open = priv_state.is_open();
                                        priv_state.show_header(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Privileged indexing daemon (fanotify)")
                                                    .color(if priv_open { c.text } else { c.text_dim }).size(11.5));
                                                ui.label(RichText::new("opt-in")
                                                    .color(c.text_muted).size(10.0).italics());
                                            });
                                        }).body(|ui| {
                                            ui.add_space(6.0);
                                            ui.label(RichText::new(
                                                "By default, Ottrin watches each indexed folder individually using inotify. \
                                                 This works well for most users. The privileged daemon (ottrin-indexd) \
                                                 uses fanotify instead — a single kernel hook that covers an entire \
                                                 filesystem mount. The difference matters when you have very deep trees, \
                                                 heavy I/O, or hit inotify's per-user watch limit \
                                                 (/proc/sys/fs/inotify/max_user_watches)."
                                            ).color(c.text_muted).size(10.5));
                                            ui.add_space(8.0);
                                            Frame::new()
                                                .fill(Color32::from_rgba_unmultiplied(200, 130, 30, 18))
                                                .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 150, 30, 80)))
                                                .corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 8))
                                                .show(ui, |ui| {
                                                    ui.label(RichText::new("Security note").color(c.text).size(11.0).strong());
                                                    ui.label(RichText::new(
                                                        "CAP_SYS_ADMIN is a powerful Linux capability. Ottrin requests it \
                                                         only for ottrin-indexd — a read-only daemon with no network access \
                                                         and no file-write operations. It lives on the binary (setcap), not \
                                                         your session. Revoke at any time: \
                                                         sudo setcap -r /path/to/ottrin-indexd"
                                                    ).color(c.text_muted).size(10.5));
                                                });
                                            ui.add_space(8.0);
                                            let priv_ok = ottrin_search::SearchService::privileged_indexing_available();
                                            if priv_ok {
                                                ui.horizontal(|ui| {
                                                    ui.label(RichText::new(MI_SEARCH).color(c.accent).size(14.0));
                                                    ui.add_space(4.0);
                                                    ui.label(RichText::new("CAP_SYS_ADMIN detected — fanotify mode active.")
                                                        .color(c.text).size(11.5));
                                                });
                                            } else {
                                                ui.label(RichText::new("Currently running in user-space mode (inotify).")
                                                    .color(c.text_dim).size(11.0));
                                                ui.add_space(8.0);
                                                ui.horizontal(|ui| {
                                                    if ui.add(egui::Button::new(
                                                        RichText::new("Grant via polkit…").size(12.0)
                                                    ).fill(c.panel_raised).stroke(Stroke::new(1.0, c.border))).on_hover_text(
                                                        "Runs:\n  setcap cap_sys_admin,cap_dac_read_search+ep <ottrin-indexd>\n\
                                                         One-time setup. Revoke:\n  sudo setcap -r <ottrin-indexd>"
                                                    ).clicked() {
                                                        let helper = std::env::current_exe().ok()
                                                            .and_then(|p| p.parent().map(|d| d.join("ottrin-indexd")));
                                                        if let Some(path) = helper.filter(|p| p.exists()) {
                                                            let _ = std::process::Command::new("pkexec")
                                                                .args(["setcap", "cap_sys_admin,cap_dac_read_search+ep"])
                                                                .arg(&path).spawn();
                                                        } else {
                                                            self.status_message = Some(UiStatusMessage {
                                                                text: "ottrin-indexd not found next to ottrin. Install the full package first.".to_string(),
                                                                ok: false,
                                                                until: Instant::now() + Duration::from_secs(6),
                                                            });
                                                        }
                                                    }
                                                    ui.label(RichText::new("Requires pkexec + admin password")
                                                        .color(c.text_muted).size(10.5).italics());
                                                });
                                            }
                                            ui.add_space(6.0);
                                        });
                                    }
                                    ui.add_space(4.0);
                                    }); // end adv_parent.body
                                    });

                                    ui.add_space(12.0);
                                    settings_section_icon(ui, c, MI_CLEAR, "Reset", "Restore search settings to defaults.", |ui| {
                                        ui.label(RichText::new(
                                            "Resets all search settings (locations, exclusions, scope, refresh interval) back to defaults. Does not affect the current index."
                                        ).color(c.text_muted).size(10.5));
                                        ui.add_space(8.0);
                                        if ui.add(egui::Button::new(
                                            RichText::new("Reset search settings to defaults").size(12.0).color(c.error)
                                        ).stroke(Stroke::new(1.0, c.error))).clicked() {
                                            let defaults = ottrin_core::SearchConfig::default();
                                            self.state.config.search.include_roots = default_search_roots();
                                            self.state.config.search.exclude_roots = defaults.exclude_roots;
                                            self.state.config.search.exclude_globs = defaults.exclude_globs;
                                            self.state.config.search.include_globs = defaults.include_globs;
                                            self.state.config.search.default_scope = defaults.default_scope;
                                            self.state.config.search.include_hidden_system = defaults.include_hidden_system;
                                            self.state.config.search.refresh_interval_hours = defaults.refresh_interval_hours;
                                            self.state.config.search.use_system_locate = defaults.use_system_locate;
                                            self.state.config.search.manage_locate_db = defaults.manage_locate_db;
                                            self.search_service.update_config(self.state.config.search.clone());
                                        }
                                        ui.add_space(4.0);
                                    });
                                }
                                SettingsTab::Cache => {
                                    ui.label(RichText::new("Cache & Thumbnails").color(c.text).size(settings_title).strong());
                                    ui.label(RichText::new("Current preview state and the next planned thumbnail/cache controls.")
                                        .color(c.text_muted).size(settings_subtitle));
                                    ui.add_space(12.0);
                                    Frame::new()
                                        .fill(c.panel_raised)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::symmetric(12, 10))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("In-memory preview state").color(c.text).size(12.0).strong());
                                            ui.label(RichText::new("Ottrin keeps lightweight preview and metadata state in memory. Image thumbnails are cached on disk for faster reloads.")
                                                .color(c.text_muted).size(10.6));
                                            ui.add_space(8.0);
                                            let preview_cached = self.info_panel_preview.as_ref().map(|(p, _)| p.display().to_string());
                                            let detail_cached = self.info_panel_details.as_ref().map(|(p, _)| p.display().to_string());
                                            let overlay_cached = self.preview.path.as_ref().map(|p| p.display().to_string());
                                            let image_cache_count = self.image_preview_cache.len();

                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Info preview").color(c.text_dim).size(11.0));
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(preview_cached.unwrap_or_else(|| "Empty".to_string())).color(c.text_muted).size(11.0));
                                            });
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Info details").color(c.text_dim).size(11.0));
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(detail_cached.unwrap_or_else(|| "Empty".to_string())).color(c.text_muted).size(11.0));
                                            });
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Preview overlay").color(c.text_dim).size(11.0));
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(overlay_cached.unwrap_or_else(|| "Empty".to_string())).color(c.text_muted).size(11.0));
                                            });
                                            ui.horizontal(|ui| {
                                                ui.label(RichText::new("Image byte cache").color(c.text_dim).size(11.0));
                                                ui.add_space(8.0);
                                                ui.label(RichText::new(format!("{} cached file(s)", image_cache_count)).color(c.text_muted).size(11.0));
                                            });
                                            ui.add_space(10.0);
                                            ui.horizontal(|ui| {
                                                if ui.add_sized([152.0, 28.0], egui::Button::new("Clear preview state")).clicked() {
                                                    self.info_panel_preview = None;
                                                    self.preview.data = None;
                                                    self.preview.path = None;
                                                    self.image_preview_cache.clear();
                                                }
                                                if ui.add_sized([154.0, 28.0], egui::Button::new("Clear metadata state")).clicked() {
                                                    self.info_panel_details = None;
                                                    self.info_panel_hashes.clear();
                                                    self.info_panel_hash_errors.clear();
                                                    self.info_panel_hash_inflight = None;
                                                }
                                            });
                                        });
                                    ui.add_space(10.0);
                                    Frame::new()
                                        .fill(c.panel_raised)
                                        .stroke(Stroke::NONE)
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::symmetric(12, 10))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new("Thumbnail cache").color(c.text).size(12.0).strong());
                                            let stats = preview_cache_stats();
                                            let dir_label = preview_cache_dir()
                                                .map(|p| p.display().to_string())
                                                .unwrap_or_else(|| "Unavailable".to_string());
                                            ui.label(
                                                RichText::new(format!("Cache path: {}", dir_label))
                                                    .color(c.text_muted)
                                                    .size(10.6),
                                            );
                                            if let Some((count, bytes)) = stats {
                                                ui.label(
                                                    RichText::new(format!("Cached thumbnails: {} ({}).", count, format_size(bytes)))
                                                        .color(c.text_muted)
                                                        .size(10.6),
                                                );
                                            } else {
                                                ui.label(
                                                    RichText::new("Cached thumbnails: unavailable.")
                                                        .color(c.text_muted)
                                                        .size(10.6),
                                                );
                                            }
                                            ui.add_space(6.0);
                                            ui.horizontal(|ui| {
                                                if ui.add_sized([156.0, 28.0], egui::Button::new("Clear thumbnail cache")).clicked() {
                                                    clear_preview_cache_dir();
                                                }
                                                ui.label(
                                                    RichText::new("Async budget controls for large folders are still pending.")
                                                        .color(c.text_dim)
                                                        .size(10.2),
                                                );
                                            });
                                        });
                                }
                            }
                                });
                        });
                });
            });
        close_requested
    }

    /// Render a single file row. Returns the egui Response so callers can
    /// check `.clicked()`, `.double_clicked()`, etc.
    ///
    /// `row_index` is used to paint alternating row backgrounds.
    #[allow(clippy::too_many_arguments)]
    fn render_file_row(
        &self,
        ui: &mut egui::Ui,
        entry: &FileEntry,
        selected: bool,
        is_active_col: bool,
        opened: bool,
        row_index: usize,
        row_width: f32,
        c: &Colors,
    ) -> egui::Response {
        let scale = self.view_scale_for(ViewMode::Grid);
        let row_h = (25.0 * scale.clamp(0.9, 1.35)).round();
        let icon_size = (16.0 * scale.sqrt()).max(14.0);
        let text_size = (13.0 * scale.sqrt()).max(11.0);
        let row_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(row_width.max(1.0), row_h));

        // Alternating stripe (painted first, underneath everything else)
        if row_index % 2 == 1 && !selected {
            ui.painter().rect_filled(row_rect, 0.0, c.row_alt);
        }

        let fill = if selected && is_active_col {
            c.selected_bg
        } else if selected {
            c.selected_bg.gamma_multiply(0.72)
        } else {
            Color32::TRANSPARENT
        };

        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(row_rect, 0.0, fill);
            // Keep selected row visually distinct from path marker rows.
            let stroke_col = if is_active_col {
                c.accent
            } else {
                c.accent_dim
            };
            ui.painter().rect_stroke(
                row_rect.shrink(1.0),
                3.0,
                Stroke::new(1.0, stroke_col),
                egui::StrokeKind::Inside,
            );
        }

        let response = ui.allocate_rect(row_rect, Sense::click());

        if response.hovered() && !selected {
            ui.painter().rect_filled(row_rect, 0.0, c.hover);
        }

        let is_dir = matches!(entry.kind, EntryKind::Directory | EntryKind::Symlink);
        let style = style_for_entry(
            entry,
            EntryVisualState {
                selected,
                focused: is_active_col,
                hovered: response.hovered(),
                opened,
                symlink_dir: matches!(entry.kind, EntryKind::Symlink)
                    && entry.symlink_target_is_dir == Some(true),
            },
            c,
            &self.state.config,
        );
        let mut label = entry.name.clone();
        if self.state.config.miller_column_width_mode == MillerColumnWidthMode::Fixed {
            let usable = (row_width - 64.0).max(64.0);
            let max_chars = (usable / (text_size * 0.6)).floor() as usize;
            label = truncate_middle(&label, max_chars.max(8));
        }
        let icon_str = material_icon(style.icon);
        ui.painter().text(
            egui::Pos2::new(row_rect.min.x + 10.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            icon_str,
            FontId::proportional(icon_size),
            style.icon_color,
        );

        ui.painter().text(
            egui::Pos2::new(row_rect.min.x + 30.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            &label,
            FontId::proportional(text_size),
            style.text_color,
        );
        // Arrow for directories — shows there are children
        if is_dir {
            ui.painter().text(
                egui::Pos2::new(row_rect.max.x - 10.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                "\u{203A}", // › single right angle quotation mark — in NotoSans
                FontId::proportional(text_size),
                c.text_muted,
            );
        }

        // NOTE: do NOT add_space here — allocate_rect already advanced the cursor.
        // Adding space here was the cause of double-row-height spacing.
        response
    }

    fn handle_viewport_resize(&self, ctx: &Context) {
        let (maximized, fullscreen) = ctx.input(|i| {
            let viewport = i.viewport();
            (
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
            )
        });
        if maximized || fullscreen {
            return;
        }

        let rect = ctx.viewport_rect();
        let Some(pos) = ctx.pointer_hover_pos() else {
            return;
        };

        if let Some(direction) = resize_direction_for_pos(rect, pos, 6.0) {
            ctx.set_cursor_icon(cursor_icon_for_resize_direction(direction));
            let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
            if primary_pressed {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    }
}

// ── eframe::App implementation ────────────────────────────────────────────────

impl eframe::App for OttrinApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Transparent clear — the GL alpha-fix callback ensures content
        // stays opaque while corners become transparent.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Track window size for persistence on exit.
        let vp = ctx.viewport_rect();
        if vp.width() > 100.0 && vp.height() > 100.0 {
            self.state.config.window_size = Some([vp.width(), vp.height()]);
        }

        self.file_pane_rect = None;
        self.smart_panel_rect = None;
        self.preview_budget_remaining = PREVIEW_REQUESTS_PER_FRAME;
        // Process async results
        self.poll_listing_results();
        self.poll_op_results();
        self.poll_hash_results();
        let preview_ready = self.poll_preview_jobs();
        self.sync_search_scope_root();

        let search_indexing = self.search_started
            && matches!(
                self.search_service.diagnostics().status,
                SearchIndexStatus::Indexing
            );
        let transient_status = self.privileged_status.is_some();
        if search_indexing || transient_status {
            // Background work and expiring status messages still need periodic UI refresh,
            // but not at a game-loop rate.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if preview_ready {
            ctx.request_repaint();
        }

        // Handle keyboard before rendering
        self.handle_keyboard(ctx);
        self.dismiss_command_frame_if_inactive(ctx);

        // Space to toggle preview — must consume before widgets see it
        if !self.any_address_bar_editing() && !self.command_frame.visible {
            let space = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space));
            if space {
                if self.preview.visible {
                    self.preview.visible = false;
                } else {
                    self.open_preview();
                }
            }
        }

        // Block scroll areas from capturing edge-drag events during window resize.
        {
            let vp = ctx.viewport_rect();
            let edge = 6.0;
            egui::Area::new(egui::Id::new("resize_edge_blocker"))
                .order(egui::Order::Foreground)
                .fixed_pos(vp.min)
                .interactable(true)
                .show(ctx, |ui| {
                    ui.set_clip_rect(vp);
                    let edges = [
                        Rect::from_min_max(vp.min, egui::pos2(vp.max.x, vp.min.y + edge)),
                        Rect::from_min_max(egui::pos2(vp.min.x, vp.max.y - edge), vp.max),
                        Rect::from_min_max(vp.min, egui::pos2(vp.min.x + edge, vp.max.y)),
                        Rect::from_min_max(egui::pos2(vp.max.x - edge, vp.min.y), vp.max),
                    ];
                    for (i, rect) in edges.iter().enumerate() {
                        ui.interact(
                            *rect,
                            egui::Id::new("edge").with(i),
                            Sense::click_and_drag(),
                        );
                    }
                });
        }

        // Render bottom panels first (egui registers bottom-to-top)
        self.render_status_bar(ctx);
        self.render_command_frame(ctx);

        // Three stacked top panels:
        //   1. Tab row:       tabs + window controls
        //   2. Nav row:       back/forward/up + address bar + toggles
        //   3. Bookmarks row: quick-access directory shortcuts
        self.render_tab_row(ctx);
        if self.tandem_tab_ids().is_none() {
            self.render_nav_row(ctx);
        }
        self.render_bookmarks_row(ctx);

        // Central file pane (fills remaining space)
        self.render_file_pane(ctx);

        // Render right sidebar (target panel, collapsible) above the final content rect.
        self.render_target_sidebar(ctx);

        // Floating overlays
        self.render_preview_overlay(ctx);
        self.render_properties_popup(ctx);
        self.render_about_popup(ctx);
        self.render_settings_modal(ctx);
        self.render_theme_editor_popup(ctx);
        self.handle_viewport_resize(ctx);

        // Paint rounded border with transparent corners on the main viewport.
        // The GL callback runs first to fix the alpha channel, then the
        // border stroke paints on top so it is never occluded.
        let vp_rect = ctx.viewport_rect();
        let border_painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("main_border"),
        ));

        let radius = self.colors.app_radius as f32;
        border_painter.add(egui::Shape::Callback(egui::PaintCallback {
            rect: vp_rect,
            callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                move |info, painter| {
                    use eframe::glow::HasContext as _;
                    let gl = painter.gl();
                    let [scr_w, scr_h] = info.screen_size_px;
                    let scr_w = scr_w as i32;
                    let scr_h = scr_h as i32;
                    let ppp = info.pixels_per_point;

                    unsafe {
                        let r_f = radius * ppp;
                        let r_px = r_f.ceil() as i32;

                        // --- alpha pass: only write the A channel --------
                        gl.color_mask(false, false, false, true);

                        // 1. Clear alpha to 0 everywhere (fully transparent).
                        gl.clear_color(0.0, 0.0, 0.0, 0.0);
                        gl.disable(eframe::glow::SCISSOR_TEST);
                        gl.clear(eframe::glow::COLOR_BUFFER_BIT);

                        // 2. Fill alpha=1.0 only inside the rounded rect.
                        gl.clear_color(0.0, 0.0, 0.0, 1.0);
                        gl.enable(eframe::glow::SCISSOR_TEST);

                        if r_px > 0 && scr_h > 2 * r_px {
                            // Middle band: full width, fully opaque.
                            gl.scissor(0, r_px, scr_w, scr_h - 2 * r_px);
                            gl.clear(eframe::glow::COLOR_BUFFER_BIT);

                            // Corner scanlines: arc-shaped inset.
                            for row in 0..r_px {
                                let dy = r_f - row as f32 - 0.5;
                                let inset = if dy <= 0.0 {
                                    0
                                } else if dy >= r_f {
                                    scr_w // fully outside
                                } else {
                                    let x_inside = (r_f * r_f - dy * dy).sqrt();
                                    let edge = r_f - x_inside;
                                    (edge.ceil() as i32).clamp(0, r_px)
                                };
                                let strip_w = scr_w - 2 * inset;
                                if strip_w <= 0 {
                                    continue;
                                }
                                // Top row (GL origin is bottom-left).
                                gl.scissor(inset, scr_h - 1 - row, strip_w, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                // Bottom row.
                                gl.scissor(inset, row, strip_w, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                            }
                        } else {
                            // No rounding or window too small — full opaque.
                            gl.scissor(0, 0, scr_w, scr_h);
                            gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                        }

                        // --- RGB pass: zero out RGB in corner boxes so
                        //     stale content colour doesn't bleed through
                        //     the border's anti-aliased fringe. ----------
                        if r_px > 0 {
                            gl.color_mask(true, true, true, false);
                            gl.clear_color(0.0, 0.0, 0.0, 0.0);
                            // Only clear the thin L-shaped strips outside
                            // the arc, not the whole corner box (to avoid
                            // destroying content pixels inside the arc).
                            for row in 0..r_px {
                                let dy = r_f - row as f32 - 0.5;
                                let inset = if dy <= 0.0 {
                                    0
                                } else if dy >= r_f {
                                    r_px
                                } else {
                                    let x_inside = (r_f * r_f - dy * dy).sqrt();
                                    let edge = r_f - x_inside;
                                    (edge.ceil() as i32).clamp(0, r_px)
                                };
                                if inset <= 0 {
                                    continue;
                                }
                                // Top-left / top-right
                                gl.scissor(0, scr_h - 1 - row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                gl.scissor(scr_w - inset, scr_h - 1 - row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                // Bottom-left / bottom-right
                                gl.scissor(0, row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                gl.scissor(scr_w - inset, row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                            }
                        }

                        gl.color_mask(true, true, true, true);
                        gl.disable(eframe::glow::SCISSOR_TEST);
                    }
                },
            )),
        }));

        // Border stroke renders AFTER the callback so it sits on top.
        border_painter.rect_stroke(
            vp_rect,
            self.colors.app_radius,
            Stroke::new(self.colors.window_border_width, self.colors.window_border),
            egui::StrokeKind::Inside,
        );
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        save_config(&self.state.config);
    }
}

// ── Directory listing worker ───────────────────────────────────────────────────

fn list_directory(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>, String> {
    use std::time::UNIX_EPOCH;

    let read_dir = std::fs::read_dir(path).map_err(|e| format_list_dir_error(path, &e))?;

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
        let file_type = match item.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        let meta = match item.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let size_bytes = if matches!(kind, EntryKind::File) {
            Some(meta.len())
        } else {
            None
        };
        let modified_unix_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let symlink_target_is_dir = if matches!(kind, EntryKind::Symlink) {
            Some(meta.is_dir())
        } else {
            None
        };
        let is_executable = detect_executable(&meta);

        entries.push(FileEntry {
            name,
            path: item.path(),
            kind,
            is_executable,
            symlink_target_is_dir,
            size_bytes,
            modified_unix_secs,
        });
    }

    Ok(entries)
}

fn detect_executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

fn sort_entries(entries: &mut [FileEntry], config: &ottrin_core::SortConfig) {
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
            SortBy::Size => b
                .size_bytes
                .unwrap_or(0)
                .cmp(&a.size_bytes.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortBy::Modified => b
                .modified_unix_secs
                .unwrap_or(0)
                .cmp(&a.modified_unix_secs.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortBy::Kind => kind_sort_rank(a).cmp(&kind_sort_rank(b)).then_with(|| {
                let a_name = a.name.to_lowercase();
                let b_name = b.name.to_lowercase();
                if config.ascending {
                    a_name.cmp(&b_name)
                } else {
                    b_name.cmp(&a_name)
                }
            }),
        };
        match config.by {
            SortBy::Name => {
                if config.ascending {
                    ord
                } else {
                    ord.reverse()
                }
            }
            SortBy::Kind | SortBy::Size | SortBy::Modified => ord,
        }
    });
}

fn sort_supports_direction(sort_by: SortBy) -> bool {
    matches!(sort_by, SortBy::Name | SortBy::Kind)
}

fn default_sort_direction(sort_by: SortBy) -> bool {
    matches!(sort_by, SortBy::Name | SortBy::Kind)
}

fn kind_sort_rank(entry: &FileEntry) -> u8 {
    match entry.kind {
        EntryKind::Directory => 0,
        EntryKind::Symlink => 1,
        EntryKind::File => 2,
        EntryKind::Other => 3,
    }
}

fn sanitize_theme_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        "ottrin-theme.json".to_string()
    } else {
        trimmed.to_string()
    }
}

fn theme_name_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported theme");
    stem.to_string()
}

fn decode_theme_import_json(
    bytes: &[u8],
    fallback_name: &str,
    default_preset: ThemePreset,
    default_mode: ThemeMode,
) -> Result<SavedTheme, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|err| format!("invalid JSON: {}", err))?;

    if let Some(theme_value) = value.get("theme") {
        let mut saved: SavedTheme = serde_json::from_value(theme_value.clone())
            .map_err(|err| format!("invalid theme payload: {}", err))?;
        if saved.name.trim().is_empty() {
            saved.name = fallback_name.to_string();
        }
        saved.customization.enabled = true;
        return Ok(saved);
    }

    if let Ok(mut saved) = serde_json::from_value::<SavedTheme>(value.clone()) {
        if saved.name.trim().is_empty() {
            saved.name = fallback_name.to_string();
        }
        saved.customization.enabled = true;
        return Ok(saved);
    }

    let mut custom: ThemeCustomization = serde_json::from_value(value)
        .map_err(|err| format!("expected Ottrin theme JSON: {}", err))?;
    custom.enabled = true;
    Ok(SavedTheme {
        name: fallback_name.to_string(),
        theme_mode: default_mode,
        base_preset: default_preset,
        customization: custom,
    })
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

pub fn load_config() -> AppConfig {
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
        if rest.is_empty() {
            home
        } else {
            home.join(rest)
        }
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
        let name = partial
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        (parent, name)
    };

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };

    // Return full paths so tab completion preserves the directory prefix.
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with(&prefix)
        })
        .take(8)
        .map(|e| {
            let full = e.path();
            // Append trailing / for directories so the user can keep tabbing deeper.
            if full.is_dir() {
                format!("{}/", full.display())
            } else {
                full.display().to_string()
            }
        })
        .collect()
}

fn format_list_dir_error(path: &Path, e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => {
            format!(
                "Permission denied\nYou don't have permission to read this folder:\n{}",
                path.display()
            )
        }
        ErrorKind::NotFound => {
            format!(
                "Folder not found\nThis folder no longer exists:\n{}",
                path.display()
            )
        }
        _ => format!("Could not open folder:\n{}\n({})", path.display(), e),
    }
}

fn is_permission_denied_msg(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("permission denied") || m.contains("access is denied")
}

fn render_listing_error(ui: &mut egui::Ui, c: &Colors, err: &str, allow_retry: bool) -> bool {
    let denied = is_permission_denied_msg(err);
    let mut retry_clicked = false;
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        if denied {
            ui.label(RichText::new(MI_LOCK).size(24.0).color(c.text_muted));
            ui.add_space(4.0);
            for line in err.lines() {
                ui.label(RichText::new(line).color(c.text_dim).size(11.5));
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new("You can still browse other folders. Restricted actions require admin privileges.")
                    .color(c.text_muted)
                    .size(10.5),
            );
            if allow_retry {
                ui.add_space(8.0);
                if ui.add(
                    egui::Button::new(RichText::new("Retry As Administrator…").size(11.5))
                        .corner_radius(5.0)
                        .min_size(Vec2::new(190.0, 24.0))
                ).clicked() {
                    retry_clicked = true;
                }
            }
        } else {
            for line in err.lines() {
                ui.label(RichText::new(line).color(c.error).size(11.5));
            }
        }
    });
    retry_clicked
}

fn viewport_window_fill(colors: Colors, focused: bool) -> (Color32, Color32, Shadow) {
    let window_fill = mix_color(colors.bg, colors.panel, 0.12);
    let title_fill = if focused {
        colors.titlebar_bg
    } else {
        mix_color(colors.titlebar_bg, colors.bg, 0.48)
    };
    let shadow = Shadow {
        offset: [0, 8],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(if focused { 80 } else { 56 }),
    };
    (window_fill, title_fill, shadow)
}

fn preview_cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut h = default_home_dir();
            h.push(".cache");
            h
        });
    let mut p = base;
    p.push("ottrin");
    p.push("preview_cache");
    Some(p)
}

fn preview_cache_key(path: &Path, target_dim: u32) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        if let Ok(mtime) = meta.modified()
            && let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH)
        {
            dur.as_secs().hash(&mut hasher);
        }
    }
    target_dim.hash(&mut hasher);
    format!("{:016x}_{}.png", hasher.finish(), target_dim)
}

fn preview_cache_path(cache_key: &str) -> Option<PathBuf> {
    let mut dir = preview_cache_dir()?;
    dir.push(cache_key);
    Some(dir)
}

fn load_cached_thumbnail(cache_key: &str) -> Option<Vec<u8>> {
    let path = preview_cache_path(cache_key)?;
    std::fs::read(path).ok()
}

fn persist_cached_thumbnail(cache_key: &str, bytes: &[u8]) {
    let Some(path) = preview_cache_path(cache_key) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
}

fn build_thumbnail_bytes(path: &Path, target_dim: u32) -> Option<Vec<u8>> {
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumb = img.thumbnail(target_dim, target_dim);
    let mut bytes = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

fn compute_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    Ok(out)
}

fn preview_cache_stats() -> Option<(usize, u64)> {
    let dir = preview_cache_dir()?;
    let mut count = 0usize;
    let mut total = 0u64;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            count += 1;
            total += meta.len();
        }
    }
    Some((count, total))
}

fn clear_preview_cache_dir() {
    let Some(dir) = preview_cache_dir() else {
        return;
    };
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_custom_titlebar(
    ui: &mut egui::Ui,
    ctx: &Context,
    colors: Colors,
    title_fill: Color32,
    title: &str,
    allow_drag: bool,
    close_requested: &mut bool,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let mut close_btn_rect: Option<Rect> = None;
    let title_bar = Frame::new()
        .fill(title_fill)
        .inner_margin(egui::Margin {
            left: 6,
            right: 8,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(CUSTOM_TITLEBAR_HEIGHT);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(colors.text).size(12.5).strong());
                ui.add_space(18.0);
                body(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close_btn = ui.add(
                        egui::Button::new(
                            RichText::new(MI_CLOSE).size(16.0).color(colors.text_muted),
                        )
                        .frame(false)
                        .min_size(Vec2::new(24.0, 24.0)),
                    );
                    close_btn_rect = Some(close_btn.rect);
                    if close_btn.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if close_btn.clicked() {
                        *close_requested = true;
                    }
                });
            });
        });
    ui.painter().hline(
        title_bar.response.rect.x_range(),
        ui.cursor().min.y,
        Stroke::new(1.0, mix_color(colors.border, title_fill, 0.6)),
    );
    if allow_drag {
        // Use the full titlebar rect (excluding close button) as the drag area.
        let bar_rect = title_bar.response.rect;
        let can_drag = ctx.input(|i| {
            if !i.pointer.primary_pressed() {
                return false;
            }
            let Some(pos) = i.pointer.hover_pos() else {
                return false;
            };
            if !bar_rect.contains(pos) {
                return false;
            }
            // Don't drag when clicking the close button.
            if let Some(close_r) = close_btn_rect
                && close_r.expand(4.0).contains(pos)
            {
                return false;
            }
            true
        });
        if can_drag {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
    title_bar.response
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
            if std::process::Command::new(cmd[0])
                .args(&cmd[1..])
                .current_dir(dir)
                .spawn()
                .is_ok()
            {
                return;
            }
        }
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
        if let Ok(t) = std::env::var("TERMINAL")
            && !t.is_empty()
        {
            if std::process::Command::new(&t)
                .arg("--working-directory")
                .arg(dir_str)
                .spawn()
                .is_ok()
            {
                return;
            }
            if std::process::Command::new(&t)
                .current_dir(dir)
                .spawn()
                .is_ok()
            {
                return;
            }
        }

        // Each entry: (binary, extra args that set working dir)
        // Most modern terminals need an explicit flag; a few respect current_dir.
        let candidates: &[(&str, &[&str])] = &[
            ("x-terminal-emulator", &["--working-directory", dir_str]),
            ("gnome-terminal", &["--working-directory", dir_str]),
            ("cosmic-term", &["--working-directory", dir_str]),
            ("tilix", &["--working-directory", dir_str]),
            ("mate-terminal", &["--working-directory", dir_str]),
            ("xfce4-terminal", &["--working-directory", dir_str]),
            ("lxterminal", &["--working-directory", dir_str]),
            ("konsole", &["--workdir", dir_str]),
            ("alacritty", &["--working-directory", dir_str]),
            ("kitty", &["-d", dir_str]),
            ("wezterm", &["start", "--cwd", dir_str]),
            ("xterm", &[]), // xterm respects current_dir
        ];

        for (t, args) in candidates {
            if std::process::Command::new(t)
                .args(*args)
                .current_dir(dir)
                .spawn()
                .is_ok()
            {
                return;
            }
        }
    }
}

fn entry_kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Directory => "Folder",
        EntryKind::File => "File",
        EntryKind::Symlink => "Symlink",
        EntryKind::Other => "Other",
    }
}

/// Human-readable file type description based on extension.
/// Returns e.g. "PNG Image", "Rust Source", "PDF Document".
fn file_type_description(path: &std::path::Path, kind: EntryKind) -> String {
    if kind == EntryKind::Directory {
        return "Folder".into();
    }
    if kind == EntryKind::Symlink {
        return "Symbolic Link".into();
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        // Images
        "png" => "PNG Image",
        "jpg" | "jpeg" => "JPEG Image",
        "gif" => "GIF Image",
        "svg" => "SVG Image",
        "webp" => "WebP Image",
        "bmp" => "Bitmap Image",
        "ico" => "Icon File",
        "tiff" | "tif" => "TIFF Image",
        "avif" => "AVIF Image",
        "heic" | "heif" => "HEIC Image",
        // Audio
        "mp3" => "MP3 Audio",
        "flac" => "FLAC Audio",
        "wav" => "WAV Audio",
        "ogg" => "Ogg Audio",
        "m4a" | "aac" => "AAC Audio",
        "opus" => "Opus Audio",
        "wma" => "WMA Audio",
        // Video
        "mp4" | "m4v" => "MP4 Video",
        "mkv" => "Matroska Video",
        "avi" => "AVI Video",
        "mov" => "QuickTime Video",
        "webm" => "WebM Video",
        "wmv" => "WMV Video",
        "flv" => "Flash Video",
        // Documents
        "pdf" => "PDF Document",
        "doc" | "docx" => "Word Document",
        "xls" | "xlsx" => "Excel Spreadsheet",
        "ppt" | "pptx" => "PowerPoint Presentation",
        "odt" => "OpenDocument Text",
        "ods" => "OpenDocument Spreadsheet",
        "odp" => "OpenDocument Presentation",
        "rtf" => "Rich Text Document",
        "txt" => "Plain Text",
        "md" | "markdown" => "Markdown Document",
        "csv" => "CSV Data",
        // Code
        "rs" => "Rust Source",
        "py" => "Python Script",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "jsx" => "React JSX",
        "tsx" => "React TSX",
        "html" | "htm" => "HTML Document",
        "css" => "CSS Stylesheet",
        "scss" | "sass" => "Sass Stylesheet",
        "json" => "JSON Data",
        "xml" => "XML Document",
        "yaml" | "yml" => "YAML Data",
        "toml" => "TOML Config",
        "sh" | "bash" => "Shell Script",
        "zsh" => "Zsh Script",
        "c" => "C Source",
        "cpp" | "cc" | "cxx" => "C++ Source",
        "h" | "hpp" => "C/C++ Header",
        "java" => "Java Source",
        "kt" => "Kotlin Source",
        "go" => "Go Source",
        "rb" => "Ruby Script",
        "php" => "PHP Script",
        "swift" => "Swift Source",
        "lua" => "Lua Script",
        "sql" => "SQL Script",
        "r" => "R Script",
        // Config
        "ini" | "cfg" => "Config File",
        "conf" => "Config File",
        "env" => "Environment File",
        "lock" => "Lock File",
        "log" => "Log File",
        // Archives
        "zip" => "ZIP Archive",
        "tar" => "Tar Archive",
        "gz" | "gzip" => "GZip Archive",
        "bz2" => "BZip2 Archive",
        "xz" => "XZ Archive",
        "7z" => "7-Zip Archive",
        "rar" => "RAR Archive",
        "zst" => "Zstandard Archive",
        // Design
        "afdesign" => "Affinity Designer File",
        "afphoto" => "Affinity Photo File",
        "psd" => "Photoshop Document",
        "ai" => "Illustrator Document",
        "sketch" => "Sketch File",
        "fig" | "figma" => "Figma File",
        "xcf" => "GIMP Image",
        // Packages
        "deb" => "Debian Package",
        "rpm" => "RPM Package",
        "appimage" => "AppImage",
        "dmg" => "Disk Image",
        "iso" => "ISO Disk Image",
        "exe" => "Windows Executable",
        "msi" => "Windows Installer",
        // Fonts
        "ttf" => "TrueType Font",
        "otf" => "OpenType Font",
        "woff" | "woff2" => "Web Font",
        // Web
        "wasm" => "WebAssembly",
        "webmanifest" => "Web Manifest",
        // Other
        _ => {
            if ext.is_empty() {
                return "File".into();
            }
            return format!("{} File", ext.to_ascii_uppercase());
        }
    }
    .into()
}

fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| -> u8 {
        ((x as f32 * (1.0 - t) + y as f32 * t).round()).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

fn parse_hex_rgb(input: &str) -> Option<[u8; 3]> {
    let s = input.trim().trim_start_matches('#');
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some([r, g, b])
        }
        3 => {
            let mut out = [0u8; 3];
            for (i, ch) in s.chars().enumerate() {
                let v = ch.to_digit(16)? as u8;
                out[i] = (v << 4) | v;
            }
            Some(out)
        }
        _ => None,
    }
}

fn shift_hsva(
    color: Color32,
    hue_shift_degrees: f32,
    saturation_scale: f32,
    value_scale: f32,
) -> Color32 {
    let mut hsva = egui::ecolor::Hsva::from(color);
    hsva.h = (hsva.h + (hue_shift_degrees / 360.0)).rem_euclid(1.0);
    hsva.s = (hsva.s * saturation_scale).clamp(0.0, 1.0);
    hsva.v = (hsva.v * value_scale).clamp(0.0, 1.0);
    Color32::from(hsva)
}

fn apply_contrast(color: Color32, contrast: f32) -> Color32 {
    let contrast = contrast.clamp(0.75, 1.35);
    let map = |v: u8| -> u8 {
        let n = (v as f32 / 255.0 - 0.5) * contrast + 0.5;
        (n.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color32::from_rgba_unmultiplied(map(color.r()), map(color.g()), map(color.b()), color.a())
}

#[derive(Clone, Copy)]
enum IconKey {
    FolderClosed,
    FolderOpen,
    FolderHome,
    FolderDesktop,
    FolderDocuments,
    FolderDownloads,
    FolderPictures,
    FolderMusic,
    FolderVideos,
    FolderPublic,
    FolderTemplates,
    FolderSymlinkClosed,
    FolderSymlinkOpen,
    Link,
    File,
    Document,
    Spreadsheet,
    Presentation,
    Image,
    Video,
    Audio,
    Archive,
    Pdf,
    Font,
    DiskImage,
    Package,
    Config,
    Code,
    Executable,
}

#[derive(Clone, Copy, Default)]
struct EntryVisualState {
    selected: bool,
    focused: bool,
    hovered: bool,
    opened: bool,
    symlink_dir: bool,
}

#[derive(Clone, Copy)]
struct EntryVisualStyle {
    icon: IconKey,
    icon_color: Color32,
    text_color: Color32,
}

#[derive(Clone, Copy)]
struct SemanticPalette {
    directory: Color32,
    symlink: Color32,
    executable: Color32,
    image: Color32,
    video: Color32,
    audio: Color32,
    archive: Color32,
    pdf: Color32,
    document: Color32,
    spreadsheet: Color32,
    presentation: Color32,
    font: Color32,
    disk_image: Color32,
    package: Color32,
    config: Color32,
    code: Color32,
    code_rust: Color32,
    code_js: Color32,
    code_ts: Color32,
    code_markup: Color32,
    code_python: Color32,
    code_shell: Color32,
    code_toml: Color32,
    code_yaml: Color32,
    unknown: Color32,
}

fn semantic_palette(config: &AppConfig, c: &Colors) -> SemanticPalette {
    if config.theme_custom.enabled {
        let blue = Colors::from_rgba(config.theme_custom.palette_folder_blue);
        let link = Colors::from_rgba(config.theme_custom.palette_link_blue);
        let steel = Colors::from_rgba(config.theme_custom.palette_steel);
        let soft_white = Colors::from_rgba(config.theme_custom.palette_soft_white);
        let green = Colors::from_rgba(config.theme_custom.palette_green);
        let orange = Colors::from_rgba(config.theme_custom.palette_orange);
        let purple = Colors::from_rgba(config.theme_custom.palette_purple);
        let pink = Colors::from_rgba(config.theme_custom.palette_pink);
        let red = Colors::from_rgba(config.theme_custom.palette_red);
        let yellow = Colors::from_rgba(config.theme_custom.palette_yellow);
        return SemanticPalette {
            directory: blue,
            symlink: link,
            executable: green,
            image: purple,
            video: red,
            audio: link,
            archive: yellow,
            pdf: red,
            document: mix_color(soft_white, c.panel, 0.30),
            spreadsheet: green,
            presentation: orange,
            font: mix_color(purple, pink, 0.45),
            disk_image: steel,
            package: mix_color(yellow, orange, 0.50),
            config: steel,
            code: mix_color(yellow, c.accent, 0.52),
            code_rust: mix_color(orange, yellow, 0.45),
            code_js: mix_color(yellow, green, 0.25),
            code_ts: mix_color(blue, link, 0.40),
            code_markup: mix_color(purple, blue, 0.38),
            code_python: mix_color(green, blue, 0.40),
            code_shell: mix_color(green, link, 0.35),
            code_toml: mix_color(yellow, steel, 0.30),
            code_yaml: mix_color(link, steel, 0.28),
            unknown: mix_color(soft_white, c.panel, 0.42),
        };
    }

    let (blue, cyan, green, amber, red, violet, orange, slate) = match config.theme_preset {
        ThemePreset::Ottrin => (
            // Seti-inspired muted semantic families (user-calibrated):
            // blue #519aba, green #8dc149, orange #e37933, pink #f55385,
            // purple #a074c4, red #cc3e44, steel #6d8086, yellow #cbcb41
            Color32::from_rgb(81, 154, 186),  // blue
            Color32::from_rgb(109, 128, 134), // steel/cyan-ish
            Color32::from_rgb(141, 193, 73),  // green
            Color32::from_rgb(203, 203, 65),  // yellow
            Color32::from_rgb(204, 62, 68),   // red
            Color32::from_rgb(160, 116, 196), // purple
            Color32::from_rgb(227, 121, 51),  // orange
            Color32::from_rgb(109, 128, 134), // steel
        ),
        ThemePreset::Breeze => (
            Color32::from_rgb(61, 174, 233),
            Color32::from_rgb(64, 196, 208),
            Color32::from_rgb(96, 176, 120),
            Color32::from_rgb(193, 157, 88),
            Color32::from_rgb(218, 88, 105),
            Color32::from_rgb(143, 136, 214),
            Color32::from_rgb(218, 144, 86),
            Color32::from_rgb(130, 142, 155),
        ),
        ThemePreset::Adwaita => (
            Color32::from_rgb(53, 132, 228),
            Color32::from_rgb(63, 164, 196),
            Color32::from_rgb(89, 163, 122),
            Color32::from_rgb(191, 154, 81),
            Color32::from_rgb(214, 81, 99),
            Color32::from_rgb(151, 131, 212),
            Color32::from_rgb(208, 137, 88),
            Color32::from_rgb(133, 145, 160),
        ),
        ThemePreset::Windows11 => (
            Color32::from_rgb(72, 157, 235),
            Color32::from_rgb(74, 182, 204),
            Color32::from_rgb(106, 172, 118),
            Color32::from_rgb(190, 158, 97),
            Color32::from_rgb(206, 98, 104),
            Color32::from_rgb(156, 138, 206),
            Color32::from_rgb(206, 141, 96),
            Color32::from_rgb(133, 144, 156),
        ),
        ThemePreset::Solarized => (
            Color32::from_rgb(38, 139, 210),
            Color32::from_rgb(42, 161, 152),
            Color32::from_rgb(133, 153, 0),
            Color32::from_rgb(181, 137, 0),
            Color32::from_rgb(220, 50, 47),
            Color32::from_rgb(108, 113, 196),
            Color32::from_rgb(203, 75, 22),
            Color32::from_rgb(131, 148, 150),
        ),
        ThemePreset::Nord => (
            Color32::from_rgb(129, 161, 193),
            Color32::from_rgb(136, 192, 208),
            Color32::from_rgb(163, 190, 140),
            Color32::from_rgb(235, 203, 139),
            Color32::from_rgb(191, 97, 106),
            Color32::from_rgb(180, 142, 173),
            Color32::from_rgb(208, 135, 112),
            Color32::from_rgb(143, 157, 178),
        ),
        ThemePreset::G33k => (
            Color32::from_rgb(96, 214, 255),
            Color32::from_rgb(84, 199, 196),
            Color32::from_rgb(131, 225, 122),
            Color32::from_rgb(201, 171, 97),
            Color32::from_rgb(246, 114, 114),
            Color32::from_rgb(168, 144, 215),
            Color32::from_rgb(212, 150, 93),
            Color32::from_rgb(118, 154, 132),
        ),
    };

    SemanticPalette {
        directory: blue,
        symlink: cyan,
        executable: green,
        image: violet,
        video: red,
        audio: cyan,
        archive: amber,
        pdf: red,
        // Neutral categories always follow the same muted token so "white files"
        // never jump out brighter than the rest of the UI.
        document: c.text_muted,
        spreadsheet: green,
        presentation: orange,
        font: mix_color(violet, orange, 0.35),
        disk_image: slate,
        package: mix_color(amber, orange, 0.50),
        config: slate,
        code: mix_color(amber, c.accent, 0.52),
        code_rust: mix_color(orange, amber, 0.45),
        code_js: mix_color(amber, green, 0.25),
        code_ts: mix_color(blue, cyan, 0.40),
        code_markup: mix_color(violet, blue, 0.38),
        code_python: mix_color(green, blue, 0.40),
        code_shell: mix_color(green, cyan, 0.35),
        code_toml: mix_color(amber, slate, 0.30),
        code_yaml: mix_color(cyan, slate, 0.28),
        unknown: c.text_muted,
    }
}

fn semantic_intensity(preset: ThemePreset) -> (f32, f32) {
    // (icon_gamma_boost, text_mix_to_neutral)
    // text_mix_to_neutral is used as mix(cat_color, neutral_text, t):
    // smaller t = stronger category presence, larger t = softer/washed.
    match preset {
        ThemePreset::Solarized => (1.00, 0.36),
        ThemePreset::Nord => (1.02, 0.22),
        ThemePreset::Adwaita | ThemePreset::Windows11 => (1.03, 0.19),
        ThemePreset::Breeze => (1.03, 0.18),
        ThemePreset::G33k => (1.02, 0.20),
        ThemePreset::Ottrin => (1.04, 0.14),
    }
}

fn semantic_for_entry(entry: &FileEntry) -> FileSemantic {
    classify_file(
        &entry.path,
        entry.kind,
        entry.is_executable,
        entry.symlink_target_is_dir,
        platform_case_mode_current(),
    )
}

fn semantic_for_search_item(item: &SearchResultItem) -> FileSemantic {
    classify_file(
        &item.path,
        item.kind,
        item.is_executable,
        item.symlink_target_is_dir,
        platform_case_mode_current(),
    )
}

fn semantic_for_path(path: &Path) -> FileSemantic {
    let kind = if path.is_dir() {
        EntryKind::Directory
    } else if path.is_symlink() {
        EntryKind::Symlink
    } else if path.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let meta = std::fs::metadata(path).ok();
    let is_executable = meta.as_ref().map(detect_executable).unwrap_or(false);
    let symlink_target_is_dir = if matches!(kind, EntryKind::Symlink) {
        meta.as_ref().map(|m| m.is_dir())
    } else {
        None
    };
    classify_file(
        path,
        kind,
        is_executable,
        symlink_target_is_dir,
        platform_case_mode_current(),
    )
}

fn semantic_color(semantic: &FileSemantic, p: &SemanticPalette) -> Color32 {
    match semantic.category {
        FileCategory::Directory => p.directory,
        FileCategory::Symlink => p.symlink,
        FileCategory::Executable => p.executable,
        FileCategory::Image => p.image,
        FileCategory::Video => p.video,
        FileCategory::Audio => p.audio,
        FileCategory::Archive => p.archive,
        FileCategory::Pdf => p.pdf,
        FileCategory::Document => p.document,
        FileCategory::Spreadsheet => p.spreadsheet,
        FileCategory::Presentation => p.presentation,
        FileCategory::Font => p.font,
        FileCategory::DiskImage => p.disk_image,
        FileCategory::Package => p.package,
        FileCategory::Config => p.config,
        FileCategory::Code => match semantic.code_subtype.unwrap_or(CodeSubtype::Other) {
            CodeSubtype::Rust => p.code_rust,
            CodeSubtype::JavaScript => p.code_js,
            CodeSubtype::TypeScript => p.code_ts,
            CodeSubtype::HTML | CodeSubtype::CSS | CodeSubtype::Markdown => p.code_markup,
            CodeSubtype::Python => p.code_python,
            CodeSubtype::Shell => p.code_shell,
            CodeSubtype::TOML => p.code_toml,
            CodeSubtype::YAML => p.code_yaml,
            CodeSubtype::JSON | CodeSubtype::Other => p.code,
        },
        FileCategory::Unknown => p.unknown,
    }
}

fn icon_for_semantic(semantic: &FileSemantic, state: EntryVisualState) -> IconKey {
    match semantic.category {
        FileCategory::Directory => {
            if state.symlink_dir {
                if state.opened {
                    IconKey::FolderSymlinkOpen
                } else {
                    IconKey::FolderSymlinkClosed
                }
            } else if state.opened {
                IconKey::FolderOpen
            } else {
                match semantic.folder_kind {
                    Some(FolderKind::Home) => IconKey::FolderHome,
                    Some(FolderKind::Desktop) => IconKey::FolderDesktop,
                    Some(FolderKind::Documents) => IconKey::FolderDocuments,
                    Some(FolderKind::Downloads) => IconKey::FolderDownloads,
                    Some(FolderKind::Pictures) => IconKey::FolderPictures,
                    Some(FolderKind::Music) => IconKey::FolderMusic,
                    Some(FolderKind::Videos) => IconKey::FolderVideos,
                    Some(FolderKind::Public) => IconKey::FolderPublic,
                    Some(FolderKind::Templates) => IconKey::FolderTemplates,
                    None => IconKey::FolderClosed,
                }
            }
        }
        FileCategory::Symlink => IconKey::Link,
        FileCategory::Executable => IconKey::Executable,
        FileCategory::Image => IconKey::Image,
        FileCategory::Video => IconKey::Video,
        FileCategory::Audio => IconKey::Audio,
        FileCategory::Archive => IconKey::Archive,
        FileCategory::Pdf => IconKey::Pdf,
        FileCategory::Document => IconKey::Document,
        FileCategory::Spreadsheet => IconKey::Spreadsheet,
        FileCategory::Presentation => IconKey::Presentation,
        FileCategory::Font => IconKey::Font,
        FileCategory::DiskImage => IconKey::DiskImage,
        FileCategory::Package => IconKey::Package,
        FileCategory::Config => IconKey::Config,
        FileCategory::Code => IconKey::Code,
        FileCategory::Unknown => IconKey::File,
    }
}

fn material_icon(icon: IconKey) -> &'static str {
    match icon {
        IconKey::FolderClosed => MI_FOLDER,
        IconKey::FolderOpen => MI_FOLDER_OPEN,
        IconKey::FolderHome => MI_HOME,
        IconKey::FolderDesktop => MI_DESKTOP,
        IconKey::FolderDocuments => MI_DESCRIPTION,
        IconKey::FolderDownloads => MI_DOWNLOAD,
        IconKey::FolderPictures => MI_IMAGE,
        IconKey::FolderMusic => MI_MUSIC_NOTE,
        IconKey::FolderVideos => MI_MOVIE,
        IconKey::FolderPublic => MI_FOLDER_SHARED,
        IconKey::FolderTemplates => MI_FOLDER_OPEN,
        IconKey::FolderSymlinkClosed | IconKey::FolderSymlinkOpen => MI_LINK,
        IconKey::Link => MI_LINK,
        IconKey::File => MI_FILE,
        IconKey::Document => MI_DESCRIPTION,
        IconKey::Spreadsheet => MI_TABLE_CHART,
        IconKey::Presentation => MI_SLIDESHOW,
        IconKey::Image => MI_IMAGE,
        IconKey::Video => MI_MOVIE,
        IconKey::Audio => MI_MUSIC_NOTE,
        IconKey::Archive => MI_FOLDER_ZIP,
        IconKey::Pdf => MI_PICTURE_PDF,
        IconKey::Font => MI_DESCRIPTION,
        IconKey::DiskImage => MI_APPS,
        IconKey::Package => MI_FOLDER_ZIP,
        IconKey::Config => MI_SETTINGS,
        IconKey::Code => MI_CODE,
        IconKey::Executable => MI_TERMINAL,
    }
}

fn style_for_semantic(
    semantic: &FileSemantic,
    state: EntryVisualState,
    c: &Colors,
    p: &SemanticPalette,
    preset: ThemePreset,
    colorize: bool,
    colorize_folder_labels: bool,
) -> EntryVisualStyle {
    let (icon_boost, text_mix) = semantic_intensity(preset);
    let cat_color = semantic_color(semantic, p);
    let is_neutral_category = matches!(
        semantic.category,
        FileCategory::Document | FileCategory::Unknown
    );
    let folder_text_base = c.text_dim;
    let file_text_base = c.file;
    let category_text_base = if matches!(semantic.category, FileCategory::Directory) {
        folder_text_base
    } else {
        file_text_base
    };
    let icon_color = if colorize {
        if state.selected && state.focused {
            mix_color(cat_color, c.heading, 0.18)
        } else if state.selected {
            mix_color(cat_color, c.heading, 0.12)
        } else {
            cat_color.gamma_multiply(icon_boost)
        }
    } else {
        match semantic.category {
            FileCategory::Directory => c.folder,
            FileCategory::Symlink => c.accent_dim.gamma_multiply(1.28),
            _ => c.file.gamma_multiply(0.92),
        }
    };
    let mut text_color = if state.selected && state.focused {
        if colorize {
            mix_color(cat_color, c.text_muted, 0.24)
        } else {
            c.text_muted
        }
    } else if state.selected {
        if colorize {
            mix_color(cat_color, c.text_muted, 0.22)
        } else {
            c.text_muted
        }
    } else if colorize {
        let is_dir = matches!(semantic.category, FileCategory::Directory);
        if is_neutral_category || (is_dir && !colorize_folder_labels) {
            category_text_base
        } else {
            // Keep semantic relation to icon color with enough signal:
            // category leads, but folder/file base tones keep readability and
            // user text controls meaningful in both semantic and neutral modes.
            mix_color(cat_color, category_text_base, text_mix)
        }
    } else {
        match semantic.category {
            FileCategory::Directory => folder_text_base,
            FileCategory::Symlink => c.accent_dim.gamma_multiply(1.4),
            _ => file_text_base,
        }
    };
    if state.hovered && !state.selected {
        text_color = mix_color(text_color, c.text_muted, 0.22);
    }
    EntryVisualStyle {
        icon: icon_for_semantic(semantic, state),
        icon_color,
        text_color,
    }
}

fn style_for_entry(
    entry: &FileEntry,
    state: EntryVisualState,
    c: &Colors,
    cfg: &AppConfig,
) -> EntryVisualStyle {
    let sem = semantic_for_entry(entry);
    style_for_semantic(
        &sem,
        state,
        c,
        &semantic_palette(cfg, c),
        cfg.theme_preset,
        cfg.colorize_file_types,
        cfg.colorize_folder_labels,
    )
}

fn style_for_search_item(
    item: &SearchResultItem,
    state: EntryVisualState,
    c: &Colors,
    cfg: &AppConfig,
) -> EntryVisualStyle {
    let sem = semantic_for_search_item(item);
    style_for_semantic(
        &sem,
        state,
        c,
        &semantic_palette(cfg, c),
        cfg.theme_preset,
        cfg.colorize_file_types,
        cfg.colorize_folder_labels,
    )
}

fn file_icon_for_path(path: &Path) -> &'static str {
    let sem = semantic_for_path(path);
    material_icon(icon_for_semantic(&sem, EntryVisualState::default()))
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

fn truncate_middle(name: &str, max_chars: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max_chars || max_chars <= 2 {
        return name.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    let left = keep / 2;
    let right = keep - left;
    let head: String = chars.iter().take(left).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(right)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{}…{}", head, tail)
}

/// Minimal flat window-control button. Draws a line-based icon (×, □, −).
/// Uses muted grey at rest, text colour on hover — no colours.
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
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x - d, ctr.y - d),
                    egui::Pos2::new(ctr.x + d, ctr.y + d),
                ],
                Stroke::new(sw, col),
            );
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x + d, ctr.y - d),
                    egui::Pos2::new(ctr.x - d, ctr.y + d),
                ],
                Stroke::new(sw, col),
            );
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
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x - d, ctr.y),
                    egui::Pos2::new(ctr.x + d, ctr.y),
                ],
                Stroke::new(sw, col),
            );
        }
    }
    resp
}

fn resize_direction_for_pos(rect: Rect, pos: egui::Pos2, margin: f32) -> Option<ResizeDirection> {
    let left = pos.x <= rect.left() + margin;
    let right = pos.x >= rect.right() - margin;
    let top = pos.y <= rect.top() + margin;
    let bottom = pos.y >= rect.bottom() - margin;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some(ResizeDirection::NorthWest),
        (false, true, true, false) => Some(ResizeDirection::NorthEast),
        (true, false, false, true) => Some(ResizeDirection::SouthWest),
        (false, true, false, true) => Some(ResizeDirection::SouthEast),
        (true, false, false, false) => Some(ResizeDirection::West),
        (false, true, false, false) => Some(ResizeDirection::East),
        (false, false, true, false) => Some(ResizeDirection::North),
        (false, false, false, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn cursor_icon_for_resize_direction(direction: ResizeDirection) -> egui::CursorIcon {
    match direction {
        ResizeDirection::North => egui::CursorIcon::ResizeNorth,
        ResizeDirection::South => egui::CursorIcon::ResizeSouth,
        ResizeDirection::East => egui::CursorIcon::ResizeEast,
        ResizeDirection::West => egui::CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => egui::CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => egui::CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => egui::CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => egui::CursorIcon::ResizeSouthWest,
    }
}

#[derive(Clone, Copy)]
enum WmBtn {
    Close,
    Maximize,
    Minimize,
}

fn build_info_panel_details(path: &Path) -> InfoPanelDetails {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => return InfoPanelDetails::default(),
    };

    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_modified(d.as_secs()));
    let accessed = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| format_modified(d.as_secs()));
    let child_count = if meta.is_dir() {
        std::fs::read_dir(path)
            .ok()
            .map(|it| it.filter_map(Result::ok).count())
    } else {
        None
    };
    let extension = if meta.is_file() {
        Some(
            path.extension()
                .and_then(|e| e.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("none")
                .to_string(),
        )
    } else {
        None
    };
    let symlink_target = if meta.file_type().is_symlink() {
        std::fs::read_link(path)
            .ok()
            .map(|p| p.display().to_string())
    } else {
        None
    };

    #[cfg(unix)]
    let permissions = Some(format_unix_permissions(&meta));
    #[cfg(not(unix))]
    let permissions = None;

    #[cfg(unix)]
    let (owner, group) = (
        lookup_unix_user(meta.uid()).or_else(|| Some(meta.uid().to_string())),
        lookup_unix_group(meta.gid()).or_else(|| Some(meta.gid().to_string())),
    );
    #[cfg(not(unix))]
    let (owner, group) = (None, None);

    #[cfg(unix)]
    let (inode, device, links) = (Some(meta.ino()), Some(meta.dev()), Some(meta.nlink()));
    #[cfg(not(unix))]
    let (inode, device, links) = (None, None, None);

    let mut image_dimensions = None;
    let mut image_format = None;

    if meta.is_file() && matches!(ottrin_preview::classify_path(path), PreviewKind::Image) {
        if let Ok((w, h)) = image::image_dimensions(path) {
            image_dimensions = Some(format!("{w} × {h}px"));
        }
        if let Ok(fmt) = image::ImageFormat::from_path(path) {
            image_format = Some(format!("{:?}", fmt).to_ascii_uppercase());
        }
    }

    InfoPanelDetails {
        readonly: meta.permissions().readonly(),
        child_count,
        extension,
        created,
        accessed,
        permissions,
        owner,
        group,
        symlink_target,
        inode,
        device,
        links,
        image_dimensions,
        image_format,
    }
}

#[cfg(unix)]
fn format_unix_permissions(meta: &std::fs::Metadata) -> String {
    let mode = meta.mode() & 0o777;
    let mut out = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let part = ((mode >> shift) & 0o7) as u8;
        out.push(if part & 0o4 != 0 { 'r' } else { '-' });
        out.push(if part & 0o2 != 0 { 'w' } else { '-' });
        out.push(if part & 0o1 != 0 { 'x' } else { '-' });
    }
    format!("{out} ({mode:03o})")
}

#[cfg(unix)]
fn lookup_unix_user(uid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        let _passwd = parts.next()?;
        let uid_part = parts.next()?;
        if uid_part.parse::<u32>().ok()? == uid {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(unix)]
fn lookup_unix_group(gid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/group").ok()?;
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        let _passwd = parts.next()?;
        let gid_part = parts.next()?;
        if gid_part.parse::<u32>().ok()? == gid {
            return Some(name.to_string());
        }
    }
    None
}

fn theme_editor_region_at_pos(
    regions: &[(ThemePreviewRegion, Rect)],
    pos: egui::Pos2,
) -> Option<ThemePreviewRegion> {
    for (region, rect) in regions {
        if rect.contains(pos) {
            return Some(*region);
        }
    }
    None
}

fn theme_editor_large_preview(
    ui: &mut egui::Ui,
    preview: Colors,
    selected_region: ThemePreviewRegion,
) -> (Option<ThemePreviewRegion>, Option<ThemePreviewRegion>) {
    let width = ui.available_width().max(320.0);
    let height = (width * 0.56).clamp(320.0, 460.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let p = ui.painter();
    let outer = rect;
    let frame = rect.shrink(14.0);
    let title_h = (frame.height() * 0.13).clamp(32.0, 44.0);
    let nav_h = (frame.height() * 0.11).clamp(28.0, 38.0);
    let bookmarks_h = (frame.height() * 0.09).clamp(24.0, 32.0);
    let status_h = (frame.height() * 0.08).clamp(22.0, 28.0);
    let titlebar = Rect::from_min_size(frame.min, Vec2::new(frame.width(), title_h));
    let nav = Rect::from_min_size(
        egui::pos2(frame.left(), titlebar.bottom()),
        Vec2::new(frame.width(), nav_h),
    );
    let bookmarks = Rect::from_min_size(
        egui::pos2(frame.left(), nav.bottom()),
        Vec2::new(frame.width(), bookmarks_h),
    );
    let status = Rect::from_min_size(
        egui::pos2(frame.left(), frame.bottom() - status_h),
        Vec2::new(frame.width(), status_h),
    );
    let content = Rect::from_min_max(
        egui::pos2(frame.left(), bookmarks.bottom()),
        egui::pos2(frame.right(), status.top()),
    );
    let sidebar_w = (content.width() * 0.14).clamp(44.0, 74.0);
    let column_gap = 6.0;
    let columns_rect = Rect::from_min_max(
        content.min,
        egui::pos2(content.right() - sidebar_w - column_gap, content.bottom()),
    );
    let sidebar = Rect::from_min_max(
        egui::pos2(columns_rect.right() + column_gap, content.top()),
        content.max,
    );
    let col_w = (columns_rect.width() - column_gap * 2.0) / 3.0;
    let col1 = Rect::from_min_max(
        columns_rect.min,
        egui::pos2(columns_rect.left() + col_w, columns_rect.bottom()),
    );
    let col2 = Rect::from_min_max(
        egui::pos2(col1.right() + column_gap, columns_rect.top()),
        egui::pos2(col1.right() + column_gap + col_w, columns_rect.bottom()),
    );
    let col3 = Rect::from_min_max(
        egui::pos2(col2.right() + column_gap, columns_rect.top()),
        columns_rect.max,
    );
    let selected_row = Rect::from_min_size(
        egui::pos2(col2.left() + 8.0, col2.top() + 54.0),
        Vec2::new(col2.width() - 16.0, 22.0),
    );
    let text_band = Rect::from_min_max(
        egui::pos2(col2.left() + 18.0, col1.top() + 18.0),
        egui::pos2(col3.right() - 10.0, col3.bottom() - 18.0),
    );
    let accent_badge = Rect::from_min_size(
        egui::pos2(status.right() - 124.0, status.center().y - 11.0),
        Vec2::new(96.0, 22.0),
    );
    let border_rect = outer.shrink(3.0);

    p.rect_filled(
        outer,
        preview.app_radius as f32 + 4.0,
        mix_color(preview.panel, preview.bg, 0.32),
    );
    p.rect_filled(frame, preview.app_radius as f32, preview.panel);
    p.rect_stroke(
        frame,
        preview.app_radius as f32,
        Stroke::new(preview.window_border_width.max(1.0), preview.window_border),
        egui::StrokeKind::Inside,
    );

    p.rect_filled(titlebar, preview.app_radius as f32, preview.titlebar_bg);
    for (idx, dot) in [
        Color32::from_rgb(255, 95, 87),
        Color32::from_rgb(255, 189, 46),
        Color32::from_rgb(39, 201, 63),
    ]
    .into_iter()
    .enumerate()
    {
        p.circle_filled(
            egui::pos2(
                titlebar.left() + 18.0 + idx as f32 * 14.0,
                titlebar.center().y,
            ),
            4.0,
            dot,
        );
    }
    let title_tab = Rect::from_min_size(
        egui::pos2(titlebar.left() + 72.0, titlebar.top() + 7.0),
        Vec2::new(150.0, titlebar.height() - 12.0),
    );
    p.rect_filled(title_tab, preview.border_radius as f32, preview.toolbar_bg);
    p.text(
        egui::pos2(title_tab.left() + 12.0, title_tab.center().y),
        Align2::LEFT_CENTER,
        "workspace",
        FontId::proportional(11.0 * preview.font_scale),
        preview.text,
    );
    p.line_segment(
        [
            egui::pos2(title_tab.left() + 10.0, title_tab.bottom()),
            egui::pos2(title_tab.right() - 10.0, title_tab.bottom()),
        ],
        Stroke::new(2.4, preview.accent),
    );
    p.text(
        egui::pos2(titlebar.right() - 82.0, titlebar.center().y),
        Align2::LEFT_CENTER,
        "Ottrin Theme Editor",
        FontId::proportional(10.5 * preview.font_scale),
        preview.text_muted,
    );

    p.rect_filled(nav, 0.0, preview.toolbar_bg);
    for (idx, glyph) in ["<-", "->", "^"].into_iter().enumerate() {
        let x = nav.left() + 20.0 + idx as f32 * 28.0;
        p.text(
            egui::pos2(x, nav.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::monospace(10.0 * preview.font_scale),
            preview.text_dim,
        );
    }
    let address = Rect::from_min_max(
        egui::pos2(nav.left() + 104.0, nav.top() + 7.0),
        egui::pos2(nav.right() - 92.0, nav.bottom() - 7.0),
    );
    p.rect_filled(address, preview.border_radius as f32, preview.bg);
    p.rect_stroke(
        address,
        preview.border_radius as f32,
        Stroke::new(1.0, mix_color(preview.border, preview.bg, 0.28)),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(address.left() + 10.0, address.center().y),
        Align2::LEFT_CENTER,
        "/home/user/projects/ottrin",
        FontId::proportional(10.0 * preview.font_scale),
        preview.text_dim,
    );
    let accent_button = Rect::from_min_size(
        egui::pos2(nav.right() - 72.0, nav.top() + 7.0),
        Vec2::new(54.0, nav.height() - 14.0),
    );
    p.rect_filled(accent_button, preview.button_radius as f32, preview.accent);
    p.text(
        accent_button.center(),
        Align2::CENTER_CENTER,
        "Sync",
        FontId::proportional(10.0 * preview.font_scale),
        preview.text,
    );

    p.rect_filled(bookmarks, 0.0, preview.bookmarks_bg);
    for (idx, name) in ["Home", "Repo", "Archive", "Design"]
        .into_iter()
        .enumerate()
    {
        let chip = Rect::from_min_size(
            egui::pos2(
                bookmarks.left() + 12.0 + idx as f32 * 74.0,
                bookmarks.top() + 5.0,
            ),
            Vec2::new(64.0, bookmarks.height() - 10.0),
        );
        let fill = if idx == 1 {
            mix_color(preview.accent, preview.bookmarks_bg, 0.18)
        } else {
            Color32::TRANSPARENT
        };
        p.rect_filled(chip, preview.button_radius as f32, fill);
        p.text(
            chip.center(),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(9.5 * preview.font_scale),
            if idx == 1 {
                preview.text
            } else {
                preview.text_dim
            },
        );
    }

    p.rect_filled(columns_rect, 0.0, preview.bg);
    for (idx, col_rect) in [col1, col2, col3].into_iter().enumerate() {
        let tint = if idx == 1 {
            mix_color(preview.panel, preview.bg, 0.16)
        } else {
            mix_color(preview.panel, preview.bg, 0.08)
        };
        p.rect_filled(col_rect, preview.border_radius as f32, tint);
        if idx > 0 {
            p.line_segment(
                [
                    egui::pos2(col_rect.left() - column_gap * 0.5, col_rect.top() + 8.0),
                    egui::pos2(col_rect.left() - column_gap * 0.5, col_rect.bottom() - 8.0),
                ],
                Stroke::new(1.0, preview.border),
            );
        }
        for row in 0..6 {
            let row_rect = Rect::from_min_size(
                egui::pos2(
                    col_rect.left() + 8.0,
                    col_rect.top() + 16.0 + row as f32 * 28.0,
                ),
                Vec2::new(col_rect.width() - 16.0, 20.0),
            );
            let fill = if idx == 1 && row == 1 {
                preview.selected_bg
            } else if row % 2 == 1 {
                preview.row_alt
            } else {
                Color32::TRANSPARENT
            };
            p.rect_filled(row_rect, preview.border_radius as f32, fill);
            p.circle_filled(
                egui::pos2(row_rect.left() + 8.0, row_rect.center().y),
                3.6,
                if row % 3 == 0 {
                    preview.folder
                } else {
                    preview.text_muted
                },
            );
            let label = if idx == 2 && row == 1 {
                "README.md"
            } else if row % 2 == 0 {
                "folder"
            } else {
                "selection"
            };
            p.text(
                egui::pos2(row_rect.left() + 16.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(9.6 * preview.font_scale),
                if idx == 2 && row == 1 {
                    preview.file
                } else {
                    preview.text_dim
                },
            );
        }
    }

    p.rect_filled(
        sidebar,
        preview.border_radius as f32,
        preview.smart_panel_bg,
    );
    p.text(
        egui::pos2(sidebar.left() + 10.0, sidebar.top() + 14.0),
        Align2::LEFT_CENTER,
        "Smart panel",
        FontId::proportional(9.4 * preview.font_scale),
        preview.text_muted,
    );
    for idx in 0..4 {
        let item = Rect::from_min_size(
            egui::pos2(
                sidebar.left() + 10.0,
                sidebar.top() + 28.0 + idx as f32 * 34.0,
            ),
            Vec2::new(sidebar.width() - 20.0, 24.0),
        );
        p.rect_filled(
            item,
            preview.button_radius as f32,
            if idx == 0 {
                mix_color(preview.accent, preview.smart_panel_bg, 0.18)
            } else {
                preview.hover
            },
        );
        p.text(
            egui::pos2(item.left() + 10.0, item.center().y),
            Align2::LEFT_CENTER,
            match idx {
                0 => "Search",
                1 => "Details",
                2 => "Target",
                _ => "Preview",
            },
            FontId::proportional(9.5 * preview.font_scale),
            if idx == 0 {
                preview.text
            } else {
                preview.text_dim
            },
        );
    }

    p.rect_filled(status, 0.0, preview.titlebar_bg);
    p.text(
        egui::pos2(status.left() + 12.0, status.center().y),
        Align2::LEFT_CENTER,
        "35 items  |  preview ready  |  theme editing",
        FontId::proportional(9.4 * preview.font_scale),
        preview.text_muted,
    );
    p.rect_filled(accent_badge, preview.button_radius as f32, preview.accent);
    p.text(
        accent_badge.center(),
        Align2::CENTER_CENTER,
        "Accent focus",
        FontId::proportional(9.6 * preview.font_scale),
        preview.text,
    );

    let regions = vec![
        (
            ThemePreviewRegion::SelectedRow,
            selected_row.expand2(Vec2::new(2.0, 3.0)),
        ),
        (
            ThemePreviewRegion::Accent,
            accent_badge.expand2(Vec2::new(4.0, 4.0)),
        ),
        (ThemePreviewRegion::ColumnText, text_band),
        (ThemePreviewRegion::Titlebar, titlebar),
        (ThemePreviewRegion::Navigation, nav),
        (ThemePreviewRegion::Bookmarks, bookmarks),
        (ThemePreviewRegion::Sidebar, sidebar),
        (ThemePreviewRegion::StatusBar, status),
        (ThemePreviewRegion::Borders, border_rect),
        (ThemePreviewRegion::ColumnBackground, columns_rect),
    ];

    let hovered_region = response
        .hover_pos()
        .and_then(|pos| theme_editor_region_at_pos(&regions, pos));
    if hovered_region.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clicked_region = if response.clicked() {
        hovered_region
    } else {
        None
    };

    for (region, region_rect) in &regions {
        let is_selected = *region == selected_region;
        let is_hovered = Some(*region) == hovered_region;
        if !is_selected && !is_hovered {
            continue;
        }
        let fill = if is_selected {
            mix_color(preview.accent, Color32::WHITE, 0.10).linear_multiply(0.26)
        } else {
            preview.accent_dim
        };
        let stroke = if is_selected {
            Stroke::new(2.0, preview.accent)
        } else {
            Stroke::new(1.0, mix_color(preview.accent, preview.bg, 0.18))
        };
        p.rect_filled(*region_rect, 8.0, fill);
        p.rect_stroke(*region_rect, 8.0, stroke, egui::StrokeKind::Inside);
    }

    let callout_region = hovered_region.unwrap_or(selected_region);
    let callout = Rect::from_min_size(
        egui::pos2(frame.right() - 246.0, frame.top() + 12.0),
        Vec2::new(224.0, 54.0),
    );
    p.rect_filled(
        callout,
        preview.border_radius as f32,
        Color32::from_rgba_premultiplied(
            preview.panel_raised.r(),
            preview.panel_raised.g(),
            preview.panel_raised.b(),
            236,
        ),
    );
    p.rect_stroke(
        callout,
        preview.border_radius as f32,
        Stroke::new(1.0, mix_color(preview.border, preview.accent, 0.18)),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(callout.left() + 10.0, callout.top() + 16.0),
        Align2::LEFT_CENTER,
        format!(
            "{} region",
            OttrinApp::theme_preview_region_label(callout_region)
        ),
        FontId::proportional(10.6 * preview.font_scale),
        preview.text,
    );
    p.text(
        egui::pos2(callout.left() + 10.0, callout.top() + 34.0),
        Align2::LEFT_CENTER,
        OttrinApp::theme_preview_region_hint(callout_region),
        FontId::proportional(8.8 * preview.font_scale),
        preview.text_muted,
    );

    (hovered_region, clicked_region)
}

fn theme_preset_card_with_size(
    ui: &mut egui::Ui,
    app_colors: Colors,
    preview: Colors,
    icon: &str,
    label: &str,
    selected: bool,
    size: ThemePresetCardSize,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(size.width, size.card_h), Sense::click());
    let p = ui.painter();
    let rounding = preview.border_radius as f32;
    let card_fill = if selected {
        preview.panel_raised.gamma_multiply(1.08)
    } else {
        preview.panel_raised
    };
    let stroke = if selected {
        Stroke::new(1.5, app_colors.accent)
    } else {
        Stroke::new(1.0, app_colors.border)
    };
    p.rect_filled(rect, rounding, card_fill);
    p.rect_stroke(rect, rounding, stroke, egui::StrokeKind::Middle);

    let inner = rect.shrink2(Vec2::new(10.0, 10.0));
    let thumb = Rect::from_min_size(inner.min, Vec2::new(inner.width(), size.thumb_h));
    p.rect_filled(thumb, preview.app_radius as f32, preview.panel);
    p.rect_stroke(
        thumb,
        preview.app_radius as f32,
        Stroke::new(1.0, preview.border),
        egui::StrokeKind::Middle,
    );

    let title_h = 18.0;
    let nav_h = 18.0;
    let chips_h = 14.0;
    let bottom_h = 16.0;
    let titlebar = Rect::from_min_size(thumb.min, Vec2::new(thumb.width(), title_h));
    p.rect_filled(titlebar, preview.app_radius as f32, preview.titlebar_bg);
    let tab = Rect::from_min_size(
        egui::pos2(titlebar.left() + 6.0, titlebar.top() + 2.0),
        Vec2::new(62.0, 14.0),
    );
    p.rect_filled(tab, preview.border_radius as f32, preview.toolbar_bg);
    p.text(
        egui::pos2(tab.left() + 8.0, tab.center().y),
        Align2::LEFT_CENTER,
        "workspace",
        FontId::proportional(7.8 * preview.font_scale),
        preview.text,
    );
    p.line_segment(
        [
            egui::pos2(tab.left(), tab.bottom()),
            egui::pos2(tab.right(), tab.bottom()),
        ],
        Stroke::new(1.6, preview.accent),
    );
    p.text(
        egui::pos2(tab.right() + 8.0, tab.center().y),
        Align2::LEFT_CENTER,
        "+",
        FontId::proportional(9.5),
        preview.text_dim,
    );

    let nav = Rect::from_min_size(
        egui::pos2(thumb.left(), titlebar.bottom()),
        Vec2::new(thumb.width(), nav_h),
    );
    p.rect_filled(nav, 0.0, preview.toolbar_bg);
    p.text(
        egui::pos2(nav.left() + 10.0, nav.center().y),
        Align2::CENTER_CENTER,
        "←",
        FontId::proportional(8.5),
        preview.text_dim,
    );
    p.text(
        egui::pos2(nav.left() + 24.0, nav.center().y),
        Align2::CENTER_CENTER,
        "→",
        FontId::proportional(8.5),
        preview.text_dim,
    );
    p.text(
        egui::pos2(nav.left() + 38.0, nav.center().y),
        Align2::CENTER_CENTER,
        "↑",
        FontId::proportional(8.5),
        preview.text_dim,
    );
    let addr = Rect::from_min_size(
        egui::pos2(nav.left() + 50.0, nav.top() + 3.0),
        Vec2::new(nav.width() * 0.55, 12.0),
    );
    p.rect_filled(addr, preview.border_radius as f32, preview.bg);
    p.text(
        egui::pos2(addr.left() + 6.0, addr.center().y),
        Align2::LEFT_CENTER,
        "/home/user",
        FontId::proportional(7.6 * preview.font_scale),
        preview.text_dim,
    );

    let chips = Rect::from_min_size(
        egui::pos2(thumb.left(), nav.bottom()),
        Vec2::new(thumb.width(), chips_h),
    );
    p.rect_filled(chips, 0.0, preview.bookmarks_bg);
    for (i, name) in ["Home", "Root", "Desktop"].iter().enumerate() {
        let x = chips.left() + 6.0 + i as f32 * 34.0;
        let chip = Rect::from_min_size(egui::pos2(x, chips.top() + 2.0), Vec2::new(30.0, 10.0));
        p.rect_filled(
            chip,
            preview.button_radius as f32,
            if i == 0 {
                preview.hover
            } else {
                Color32::TRANSPARENT
            },
        );
        p.text(
            chip.center(),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(6.8 * preview.font_scale),
            if i == 0 {
                preview.text
            } else {
                preview.text_dim
            },
        );
    }

    let content = Rect::from_min_max(
        egui::pos2(thumb.left(), chips.bottom()),
        egui::pos2(thumb.right(), thumb.bottom() - bottom_h),
    );
    p.rect_filled(content, 0.0, preview.bg);
    let col_gap = 4.0;
    let smart_w = 16.0;
    let col_w = ((content.width() - smart_w - col_gap * 3.0) / 3.0).max(18.0);
    for col in 0..3 {
        let x = content.left() + col as f32 * (col_w + col_gap);
        let col_rect = Rect::from_min_size(
            egui::pos2(x, content.top()),
            Vec2::new(col_w, content.height()),
        );
        if col > 0 {
            p.line_segment(
                [
                    egui::pos2(col_rect.left() - col_gap * 0.5, content.top()),
                    egui::pos2(col_rect.left() - col_gap * 0.5, content.bottom()),
                ],
                Stroke::new(1.0, preview.border),
            );
        }
        for row in 0..4 {
            let row_y = col_rect.top() + 5.0 + row as f32 * 14.0;
            let row_rect = Rect::from_min_size(
                egui::pos2(col_rect.left() + 2.0, row_y),
                Vec2::new(col_rect.width() - 4.0, 10.0),
            );
            let fill = if col == 1 && row == 1 {
                preview.selected_bg
            } else if row % 2 == 1 {
                preview.row_alt
            } else {
                Color32::TRANSPARENT
            };
            p.rect_filled(row_rect, preview.border_radius as f32, fill);
            p.rect_filled(
                Rect::from_min_size(
                    egui::pos2(row_rect.left() + 3.0, row_rect.center().y - 2.0),
                    Vec2::new(6.0, 4.0),
                ),
                2.0,
                preview.folder,
            );
            p.text(
                egui::pos2(row_rect.left() + 12.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                if row == 1 && col == 2 {
                    "google..."
                } else {
                    "folder"
                },
                FontId::proportional(6.8 * preview.font_scale),
                if col == 2 && row == 1 {
                    preview.file
                } else {
                    preview.text_dim
                },
            );
        }
    }
    let smart = Rect::from_min_size(
        egui::pos2(content.right() - smart_w, content.top()),
        Vec2::new(smart_w, content.height()),
    );
    p.rect_filled(smart, 0.0, preview.smart_panel_bg);
    for i in 0..3 {
        let icon = Rect::from_min_size(
            egui::pos2(smart.left() + 3.0, smart.top() + 6.0 + i as f32 * 15.0),
            Vec2::new(10.0, 10.0),
        );
        p.rect_filled(
            icon,
            preview.button_radius as f32,
            if i == 0 {
                preview.selected_bg
            } else {
                preview.hover
            },
        );
    }

    let bottom = Rect::from_min_size(
        egui::pos2(thumb.left(), thumb.bottom() - bottom_h),
        Vec2::new(thumb.width(), bottom_h),
    );
    p.rect_filled(bottom, 0.0, preview.titlebar_bg);
    p.text(
        egui::pos2(bottom.left() + 8.0, bottom.center().y),
        Align2::LEFT_CENTER,
        "35 items",
        FontId::proportional(6.6 * preview.font_scale),
        preview.text_muted,
    );
    let mini_slider = Rect::from_min_size(
        egui::pos2(bottom.right() - 42.0, bottom.center().y - 2.0),
        Vec2::new(26.0, 4.0),
    );
    p.rect_filled(mini_slider, 2.0, preview.bg);
    p.rect_filled(
        Rect::from_min_size(mini_slider.min, Vec2::new(14.0, 4.0)),
        2.0,
        preview.accent,
    );

    let title = format!("{}  {}", icon, label);
    p.text(
        egui::pos2(inner.center().x, thumb.max.y + 28.0),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(12.0),
        if selected {
            app_colors.text
        } else {
            app_colors.text_dim
        },
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn paint_theme_tuning_track(ui: &mut egui::Ui, rect: Rect, title: &str, tint: Color32, c: Colors) {
    let painter = ui.painter();
    let track = rect.shrink2(Vec2::new(1.0, 1.0));
    let steps = 56;
    if title == "Contrast" {
        let mid = track.center().y;
        let top = Rect::from_min_max(track.min, egui::pos2(track.max.x, mid));
        let bottom = Rect::from_min_max(egui::pos2(track.min.x, mid), track.max);
        let start = mix_color(c.panel_raised, c.border, 0.65);
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let t1 = (i + 1) as f32 / steps as f32;
            let x0 = egui::lerp(track.left()..=track.right(), t0);
            let x1 = egui::lerp(track.left()..=track.right(), t1);
            let t = (t0 + t1) * 0.5;
            let top_col = mix_color(start, Color32::BLACK, t);
            let bottom_col = mix_color(start, Color32::WHITE, t);
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x0, top.top() + 1.0),
                    egui::pos2(x1, top.bottom() - 1.0),
                ),
                0.0,
                top_col,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(x0, bottom.top() + 1.0),
                    egui::pos2(x1, bottom.bottom() - 1.0),
                ),
                0.0,
                bottom_col,
            );
        }
        return;
    }

    let stops: Vec<Color32> = match title {
        "Hue" => vec![
            Color32::from_rgb(255, 90, 90),
            Color32::from_rgb(255, 196, 76),
            Color32::from_rgb(118, 220, 98),
            Color32::from_rgb(86, 205, 220),
            Color32::from_rgb(92, 146, 255),
            Color32::from_rgb(190, 112, 255),
            Color32::from_rgb(255, 90, 160),
        ],
        "Saturation" => vec![mix_color(c.panel_raised, c.text_muted, 0.35), tint],
        "Brightness" => vec![
            Color32::from_rgb(10, 10, 12),
            tint,
            mix_color(Color32::WHITE, tint, 0.18),
        ],
        _ => vec![
            mix_color(c.panel_raised, c.border, 0.25),
            mix_color(c.panel_raised, c.border, 0.85),
        ],
    };
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let color = gradient_stops_color(&stops, (t0 + t1) * 0.5);
        let x0 = egui::lerp(track.left()..=track.right(), t0);
        let x1 = egui::lerp(track.left()..=track.right(), t1);
        painter.rect_filled(
            Rect::from_min_max(egui::pos2(x0, track.top()), egui::pos2(x1, track.bottom())),
            0.0,
            color,
        );
    }
    let _ = stops;
}

fn gradient_stops_color(stops: &[Color32], t: f32) -> Color32 {
    if stops.is_empty() {
        return Color32::WHITE;
    }
    if stops.len() == 1 {
        return stops[0];
    }
    let clamped = t.clamp(0.0, 1.0);
    let scaled = clamped * (stops.len() - 1) as f32;
    let idx = scaled.floor() as usize;
    let next = (idx + 1).min(stops.len() - 1);
    let local_t = scaled - idx as f32;
    mix_color(stops[idx], stops[next], local_t)
}

fn settings_section_icon(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    title: &str,
    subtitle: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    Frame::new()
        .fill(c.panel_raised)
        .stroke(Stroke::NONE)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(c.accent).size(14.0));
                ui.add_space(4.0);
                ui.label(RichText::new(title).color(c.text).size(13.0).strong());
            });
            if !subtitle.is_empty() {
                ui.add_space(2.0);
                ui.label(RichText::new(subtitle).color(c.text_muted).size(11.0));
            }
            ui.add_space(8.0);
            body(ui);
        });
}

fn search_scope_summary(scope: SearchScope, current_dir: &Path) -> String {
    match scope {
        SearchScope::Global => "Scope: Global index".to_string(),
        SearchScope::CurrentFolder => format!(
            "Scope: Current folder ({})",
            compact_search_path(current_dir)
        ),
    }
}

fn search_root_summary(diagnostics: &SearchIndexDiagnostics) -> String {
    format!(
        "Roots: {} configured, {} watched",
        diagnostics.configured_root_count, diagnostics.watched_root_count
    )
}

fn search_scan_summary(diagnostics: &SearchIndexDiagnostics) -> String {
    match diagnostics.status {
        SearchIndexStatus::Indexing => {
            let started = diagnostics
                .last_scan_started_unix_secs
                .map(format_modified)
                .unwrap_or_else(|| "just now".to_string());
            if diagnostics.total_roots > 0 {
                format!(
                    "Scan: running {} · root {}/{} · {} indexed",
                    started,
                    diagnostics.active_root_index.max(1),
                    diagnostics.total_roots,
                    diagnostics.indexed_items
                )
            } else {
                format!("Scan: running {} · preparing roots", started)
            }
        }
        SearchIndexStatus::Ready => {
            let completed = diagnostics
                .last_scan_completed_unix_secs
                .map(format_modified)
                .unwrap_or_else(|| "unknown".to_string());
            let duration = diagnostics
                .last_scan_duration_ms
                .map(format_search_duration)
                .unwrap_or_else(|| "unknown duration".to_string());
            format!(
                "Scan: completed {} in {} · {} indexed",
                completed, duration, diagnostics.indexed_items
            )
        }
        SearchIndexStatus::Unavailable => {
            let completed = diagnostics
                .last_scan_completed_unix_secs
                .map(format_modified)
                .unwrap_or_else(|| "recently".to_string());
            let duration = diagnostics
                .last_scan_duration_ms
                .map(format_search_duration)
                .unwrap_or_else(|| "unknown duration".to_string());
            format!("Scan: failed {} after {}", completed, duration)
        }
    }
}

fn search_diagnostics_tooltip(
    diagnostics: &SearchIndexDiagnostics,
    scope: SearchScope,
    current_dir: &Path,
) -> String {
    let mut lines = vec![
        search_scope_summary(scope, current_dir),
        search_root_summary(diagnostics),
        search_scan_summary(diagnostics),
    ];
    if let Some(active_root) = diagnostics.active_root.as_ref() {
        lines.push(format!(
            "Current root: {}",
            compact_search_path(active_root)
        ));
    }
    if let Some(detail) = diagnostics.detail.as_ref().filter(|d| !d.trim().is_empty()) {
        lines.push(detail.trim().to_string());
    }
    if let Some(err) = diagnostics
        .last_error
        .as_ref()
        .filter(|e| !e.trim().is_empty())
    {
        lines.push(format!("Error: {}", err.trim()));
    }
    lines.join("\n")
}

fn compact_search_path(path: &Path) -> String {
    let text = path.display().to_string();
    let max_chars = 56usize;
    if text.chars().count() <= max_chars {
        return text;
    }
    let tail: String = text
        .chars()
        .rev()
        .take(max_chars.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{}", tail)
}

fn format_search_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{} ms", duration_ms)
    } else if duration_ms < 60_000 {
        format!("{:.1} s", duration_ms as f64 / 1_000.0)
    } else {
        let secs = duration_ms / 1_000;
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

fn theme_preview_strip(ui: &mut egui::Ui, preview: Colors, width: f32, height: f32, c: Colors) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let swatches = [
        preview.bg,
        preview.panel,
        preview.panel_raised,
        preview.accent,
        preview.folder,
    ];
    let swatch_w = rect.width() / swatches.len() as f32;
    for (idx, swatch) in swatches.into_iter().enumerate() {
        let x0 = rect.left() + swatch_w * idx as f32;
        let x1 = if idx + 1 == swatches.len() {
            rect.right()
        } else {
            rect.left() + swatch_w * (idx as f32 + 1.0)
        };
        let swatch_rect =
            Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom()));
        ui.painter().rect_filled(swatch_rect, 0.0, swatch);
    }
    ui.painter().rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, c.border),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_folder_uses_open_icon_variant() {
        let cfg = AppConfig::default();
        let c = Colors::for_config(&cfg);
        let entry = FileEntry {
            name: "folder".to_string(),
            path: PathBuf::from("/tmp/folder"),
            kind: EntryKind::Directory,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: None,
            modified_unix_secs: None,
        };
        let closed = style_for_entry(
            &entry,
            EntryVisualState {
                opened: false,
                ..Default::default()
            },
            &c,
            &cfg,
        );
        let opened = style_for_entry(
            &entry,
            EntryVisualState {
                opened: true,
                ..Default::default()
            },
            &c,
            &cfg,
        );
        assert!(matches!(closed.icon, IconKey::FolderClosed));
        assert!(matches!(opened.icon, IconKey::FolderOpen));
    }

    #[test]
    fn colorize_disabled_keeps_neutral_text_color() {
        let cfg = AppConfig {
            colorize_file_types: false,
            ..AppConfig::default()
        };
        let c = Colors::for_config(&cfg);
        let entry = FileEntry {
            name: "main.rs".to_string(),
            path: PathBuf::from("/tmp/main.rs"),
            kind: EntryKind::File,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: None,
            modified_unix_secs: None,
        };
        let style = style_for_entry(&entry, EntryVisualState::default(), &c, &cfg);
        assert_eq!(style.text_color, c.file);
    }

    #[test]
    fn semantic_style_is_deterministic_by_subtype() {
        let cfg = AppConfig::default();
        let c = Colors::for_config(&cfg);
        let p = semantic_palette(&cfg, &c);
        let rust = FileSemantic {
            category: FileCategory::Code,
            code_subtype: Some(CodeSubtype::Rust),
            folder_kind: None,
            rule_hint: None,
        };
        let ts = FileSemantic {
            category: FileCategory::Code,
            code_subtype: Some(CodeSubtype::TypeScript),
            folder_kind: None,
            rule_hint: None,
        };
        let rust_style = style_for_semantic(
            &rust,
            EntryVisualState::default(),
            &c,
            &p,
            ThemePreset::Ottrin,
            true,
            false,
        );
        let ts_style = style_for_semantic(
            &ts,
            EntryVisualState::default(),
            &c,
            &p,
            ThemePreset::Ottrin,
            true,
            false,
        );
        assert_ne!(rust_style.icon_color, ts_style.icon_color);
        assert!(matches!(rust_style.icon, IconKey::Code));
    }

    #[test]
    fn size_sort_is_fixed_descending() {
        let mut entries = vec![
            FileEntry {
                name: "small.txt".to_string(),
                path: PathBuf::from("/tmp/small.txt"),
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: Some(10),
                modified_unix_secs: None,
            },
            FileEntry {
                name: "large.txt".to_string(),
                path: PathBuf::from("/tmp/large.txt"),
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: Some(100),
                modified_unix_secs: None,
            },
        ];
        sort_entries(
            &mut entries,
            &ottrin_core::SortConfig {
                by: SortBy::Size,
                ascending: true,
            },
        );
        assert_eq!(entries[0].name, "large.txt");
        sort_entries(
            &mut entries,
            &ottrin_core::SortConfig {
                by: SortBy::Size,
                ascending: false,
            },
        );
        assert_eq!(entries[0].name, "large.txt");
    }

    #[test]
    fn kind_sort_keeps_group_order_and_flips_names_within_group() {
        let mut entries = vec![
            FileEntry {
                name: "beta.txt".to_string(),
                path: PathBuf::from("/tmp/beta.txt"),
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: None,
                modified_unix_secs: None,
            },
            FileEntry {
                name: "alpha.txt".to_string(),
                path: PathBuf::from("/tmp/alpha.txt"),
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: None,
                modified_unix_secs: None,
            },
            FileEntry {
                name: "docs".to_string(),
                path: PathBuf::from("/tmp/docs"),
                kind: EntryKind::Directory,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: None,
                modified_unix_secs: None,
            },
        ];
        sort_entries(
            &mut entries,
            &ottrin_core::SortConfig {
                by: SortBy::Kind,
                ascending: true,
            },
        );
        assert_eq!(entries[0].name, "docs");
        assert_eq!(entries[1].name, "alpha.txt");
        assert_eq!(entries[2].name, "beta.txt");

        sort_entries(
            &mut entries,
            &ottrin_core::SortConfig {
                by: SortBy::Kind,
                ascending: false,
            },
        );
        assert_eq!(entries[0].name, "docs");
        assert_eq!(entries[1].name, "beta.txt");
        assert_eq!(entries[2].name, "alpha.txt");
    }

    #[test]
    fn resize_direction_detects_edges_and_corners() {
        let rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 80.0));
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(2.0, 2.0), 6.0),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(98.0, 2.0), 6.0),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(50.0, 78.0), 6.0),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(1.0, 40.0), 6.0),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(50.0, 40.0), 6.0),
            None
        );
    }

    #[test]
    fn decode_theme_import_accepts_wrapped_payload() {
        let wrapped = serde_json::json!({
            "format": "ottrin-theme",
            "version": 1,
            "theme": {
                "name": "Wrapped",
                "theme_mode": "Dark",
                "base_preset": "Nord",
                "customization": ThemeCustomization::default(),
            }
        });
        let bytes = serde_json::to_vec(&wrapped).unwrap();
        let imported =
            decode_theme_import_json(&bytes, "Fallback", ThemePreset::Ottrin, ThemeMode::System)
                .unwrap();
        assert_eq!(imported.name, "Wrapped");
        assert_eq!(imported.base_preset, ThemePreset::Nord);
        assert_eq!(imported.theme_mode, ThemeMode::Dark);
        assert!(imported.customization.enabled);
    }

    #[test]
    fn decode_theme_import_accepts_saved_theme_payload() {
        let saved = SavedTheme {
            name: "Saved".to_string(),
            theme_mode: ThemeMode::Light,
            base_preset: ThemePreset::Solarized,
            customization: ThemeCustomization::default(),
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        let imported =
            decode_theme_import_json(&bytes, "Fallback", ThemePreset::Ottrin, ThemeMode::System)
                .unwrap();
        assert_eq!(imported.name, "Saved");
        assert_eq!(imported.base_preset, ThemePreset::Solarized);
        assert_eq!(imported.theme_mode, ThemeMode::Light);
        assert!(imported.customization.enabled);
    }

    #[test]
    fn decode_theme_import_accepts_customization_payload() {
        let custom = ThemeCustomization {
            accent: [1, 2, 3, 255],
            ..ThemeCustomization::default()
        };
        let bytes = serde_json::to_vec(&custom).unwrap();
        let imported = decode_theme_import_json(
            &bytes,
            "Fallback Name",
            ThemePreset::Breeze,
            ThemeMode::Dark,
        )
        .unwrap();
        assert_eq!(imported.name, "Fallback Name");
        assert_eq!(imported.base_preset, ThemePreset::Breeze);
        assert_eq!(imported.theme_mode, ThemeMode::Dark);
        assert_eq!(imported.customization.accent, [1, 2, 3, 255]);
        assert!(imported.customization.enabled);
    }
}
