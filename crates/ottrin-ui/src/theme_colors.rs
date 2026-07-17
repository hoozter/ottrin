use egui::Color32;
use ottrin_core::{AppConfig, ThemeMode, ThemePreset};

#[derive(Clone, Copy)]
pub(crate) struct Colors {
    pub(crate) bg: Color32,
    pub(crate) panel: Color32,
    pub(crate) panel_raised: Color32,
    pub(crate) toolbar_bg: Color32,
    pub(crate) bookmarks_bg: Color32,
    pub(crate) smart_panel_bg: Color32,
    pub(crate) titlebar_bg: Color32,
    pub(crate) border: Color32,
    pub(crate) window_border: Color32,
    pub(crate) window_border_width: f32,
    pub(crate) heading: Color32,
    pub(crate) folder: Color32,
    pub(crate) file: Color32,
    pub(crate) text: Color32,
    pub(crate) text_dim: Color32,
    pub(crate) text_muted: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_dim: Color32,
    pub(crate) hover: Color32,
    pub(crate) selected_bg: Color32,
    pub(crate) row_alt: Color32,
    pub(crate) error: Color32,
    pub(crate) app_radius: u8,
    pub(crate) border_radius: u8,
    pub(crate) button_radius: u8,
    pub(crate) font_scale: f32,
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

    pub(crate) fn from_rgba(v: [u8; 4]) -> Color32 {
        Color32::from_rgba_unmultiplied(v[0], v[1], v[2], v[3])
    }

    pub(crate) fn to_rgba(c: Color32) -> [u8; 4] {
        [c.r(), c.g(), c.b(), c.a()]
    }

    pub(crate) fn preset(preset: ThemePreset, dark: bool) -> Self {
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

    pub(crate) fn for_config(config: &AppConfig) -> Self {
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

    pub(crate) fn base_for_config(config: &AppConfig) -> Self {
        let dark = match config.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark | ThemeMode::System => true,
        };
        Self::preset(config.theme_preset, dark)
    }
}

pub(crate) fn mix_color(a: Color32, b: Color32, t: f32) -> Color32 {
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

pub(crate) fn parse_hex_rgb(input: &str) -> Option<[u8; 3]> {
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

pub(crate) fn shift_hsva(
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

pub(crate) fn apply_contrast(color: Color32, contrast: f32) -> Color32 {
    let contrast = contrast.clamp(0.75, 1.35);
    let map = |v: u8| -> u8 {
        let n = (v as f32 / 255.0 - 0.5) * contrast + 0.5;
        (n.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    Color32::from_rgba_unmultiplied(map(color.r()), map(color.g()), map(color.b()), color.a())
}
