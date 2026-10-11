use egui::Color32;
use ottrin_core::{AppConfig, ThemeMode, ThemePreset};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Colors {
    pub(crate) bg: Color32,
    pub(crate) panel: Color32,
    pub(crate) panel_raised: Color32,
    pub(crate) statusbar_bg: Color32,
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
    pub(crate) folder_text: Color32,
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
    pub(crate) button_bg: Color32,
    pub(crate) button_text: Color32,
    pub(crate) font_scale: f32,
    pub(crate) file_label_blend: f32,
    pub(crate) palette: [Color32; 10],
}

impl Colors {
    fn dark() -> Self {
        Self {
            statusbar_bg: Color32::from_rgb(27, 29, 34),
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
            folder_text: Color32::from_rgb(188, 191, 196),
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
            button_bg: Color32::from_rgb(40, 43, 50),
            button_text: Color32::from_rgb(188, 191, 196),
            font_scale: 1.0,
            file_label_blend: 0.14,
            palette: [Color32::TRANSPARENT; 10],
        }
    }

    fn light() -> Self {
        Self {
            statusbar_bg: Color32::from_rgb(244, 244, 244),
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
            folder_text: Color32::from_rgb(80, 80, 80),
            text: Color32::from_rgb(20, 20, 20),
            text_dim: Color32::from_rgb(80, 80, 80),
            text_muted: Color32::from_rgb(150, 150, 150),
            accent: Color32::from_rgb(35, 94, 223),
            accent_dim: Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
            selected_bg: Color32::from_rgba_unmultiplied(37, 99, 235, 50),
            row_alt: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            error: Color32::from_rgb(196, 48, 48),
            app_radius: 5,
            border_radius: 5,
            button_radius: 5,
            button_bg: Color32::from_rgb(235, 235, 235),
            button_text: Color32::from_rgb(80, 80, 80),
            font_scale: 1.0,
            file_label_blend: 0.14,
            palette: [Color32::TRANSPARENT; 10],
        }
    }

    pub(crate) fn from_rgba(v: [u8; 4]) -> Color32 {
        Color32::from_rgba_unmultiplied(v[0], v[1], v[2], v[3])
    }

    pub(crate) fn to_rgba(c: Color32) -> [u8; 4] {
        c.to_srgba_unmultiplied()
    }

    pub(crate) fn preset(preset: ThemePreset, dark: bool) -> Self {
        let mut colors = match preset {
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
                        accent: Color32::from_rgb(92, 169, 243),
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
                        accent: Color32::from_rgb(61, 174, 233),
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
                        accent: Color32::from_rgb(40, 113, 151),
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
                        accent: Color32::from_rgb(91, 155, 233),
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
                        accent: Color32::from_rgb(42, 104, 180),
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
                        accent: Color32::from_rgb(96, 178, 255),
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
                        accent: Color32::from_rgb(0, 110, 195),
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
                        panel_raised: Color32::from_rgb(10, 58, 70),
                        border: Color32::from_rgb(101, 123, 131),
                        window_border: Color32::from_rgb(101, 123, 131),
                        heading: Color32::from_rgb(187, 199, 199),
                        folder: Color32::from_rgb(38, 139, 210),
                        file: Color32::from_rgb(159, 174, 174),
                        accent: Color32::from_rgb(76, 176, 168),
                        error: Color32::from_rgb(220, 50, 47),
                        border_radius: 4,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                } else {
                    Self {
                        bg: Color32::from_rgb(253, 246, 227),
                        panel: Color32::from_rgb(238, 232, 213),
                        panel_raised: Color32::from_rgb(253, 246, 227),
                        border: Color32::from_rgb(147, 161, 161),
                        window_border: Color32::from_rgb(147, 161, 161),
                        heading: Color32::from_rgb(67, 85, 92),
                        folder: Color32::from_rgb(35, 128, 194),
                        file: Color32::from_rgb(94, 113, 120),
                        accent: Color32::from_rgb(29, 113, 106),
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
                        accent: Color32::from_rgb(173, 194, 215),
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
                        accent: Color32::from_rgb(77, 97, 116),
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
                        accent: Color32::from_rgb(96, 214, 255),
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
                        accent: Color32::from_rgb(35, 93, 221),
                        error: Color32::from_rgb(190, 68, 68),
                        border_radius: 3,
                        font_scale: 1.0,
                        ..Self::dark()
                    }
                }
            }
        };
        // Each preset owns all its chrome surfaces; do not inherit unrelated dark defaults.
        if preset != ThemePreset::Ottrin && !(preset == ThemePreset::Breeze && dark) {
            colors.toolbar_bg = colors.panel;
            colors.bookmarks_bg = colors.panel;
            colors.smart_panel_bg = colors.panel;
            colors.titlebar_bg = colors.bg;
        }
        if !dark {
            colors.file = mix_color(colors.file, colors.heading, 0.5);
        }
        colors.text = colors.heading;
        colors.error = mix_color(
            colors.error,
            if dark { Color32::WHITE } else { Color32::BLACK },
            if dark { 0.55 } else { 0.24 },
        );
        // Restrained overlays keep semantic ink readable in every row state.
        colors.selected_bg = Color32::from_rgba_unmultiplied(
            colors.accent.r(),
            colors.accent.g(),
            colors.accent.b(),
            if dark { 36 } else { 28 },
        );
        colors.hover = if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 8)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 12)
        };
        colors.file_label_blend = match preset {
            ThemePreset::Solarized => 0.36,
            ThemePreset::Nord => 0.22,
            ThemePreset::Adwaita | ThemePreset::Windows11 => 0.19,
            ThemePreset::Breeze => 0.18,
            ThemePreset::G33k => 0.20,
            ThemePreset::Ottrin => 0.14,
        };
        colors.palette = Self::semantic_palette_for_preset(preset, dark, colors.file);
        colors.folder_text = colors.file;
        colors.text_dim = colors.file;
        colors.text_muted = colors.file;
        colors.button_bg = colors.panel_raised;
        colors.button_text = colors.text;
        colors.statusbar_bg = colors.smart_panel_bg;
        colors.derive_overlays();
        colors
    }

    fn derive_overlays(&mut self) {
        // Overlays follow the stored palette, so an unedited custom copy renders
        // identically to its preset without consulting that preset at runtime.
        let bg = Self::to_rgba(self.bg);
        let dark = (0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32) < 128.0;
        let accent = Self::to_rgba(self.accent);
        self.accent_dim = Self::from_rgba([
            accent[0],
            accent[1],
            accent[2],
            ((u16::from(accent[3]) * if dark { 36 } else { 28 }) / 255) as u8,
        ]);
        self.row_alt = if dark {
            Color32::from_rgba_unmultiplied(255, 255, 255, 5)
        } else {
            Color32::from_rgba_unmultiplied(0, 0, 0, 5)
        };
    }

    pub(crate) fn for_config(config: &AppConfig, system_theme: Option<egui::Theme>) -> Self {
        if let Some(saved) = config.active_custom_theme.as_ref().and_then(|name| {
            config
                .custom_themes
                .iter()
                .find(|theme| theme.name == *name)
        }) {
            Self::for_saved(saved, config.theme, system_theme)
        } else {
            Self::base_for_config(config, system_theme)
        }
    }

    pub(crate) fn for_saved(
        saved: &ottrin_core::SavedTheme,
        mode: ThemeMode,
        system_theme: Option<egui::Theme>,
    ) -> Self {
        let dark = Self::resolved_mode(mode, system_theme) != ThemeMode::Light;
        // Complete stored variants own their visuals. The preset is provenance only.
        Self::from_custom(if dark { &saved.dark } else { &saved.light })
    }

    pub(crate) fn from_custom(custom: &ottrin_core::ThemeCustomization) -> Self {
        Self::dark().customized(custom)
    }

    pub(crate) fn customized(mut self, custom: &ottrin_core::ThemeCustomization) -> Self {
        let base = &mut self;
        base.palette = [
            custom.palette_folder_blue,
            custom.palette_link_blue,
            custom.palette_steel,
            custom.palette_soft_white,
            custom.palette_green,
            custom.palette_orange,
            custom.palette_purple,
            custom.palette_pink,
            custom.palette_red,
            custom.palette_yellow,
        ]
        .map(Self::from_rgba);

        base.bg = Self::from_rgba(custom.background);
        base.panel = Self::from_rgba(custom.panel);
        // Keep custom colors exact: no implicit gamma/tint transforms.
        // `panel_raised` is a generic elevated surface and must not alias titlebar.
        base.panel_raised = Self::from_rgba(custom.panel_raised);
        base.statusbar_bg = Self::from_rgba(custom.statusbar);
        base.toolbar_bg = Self::from_rgba(custom.toolbar);
        // Bookmarks have their own surface token.
        base.bookmarks_bg = Self::from_rgba(custom.bookmarks_bar);
        base.smart_panel_bg = Self::from_rgba(custom.smart_panel);
        base.titlebar_bg = Self::from_rgba(custom.titlebar);
        base.border = Self::from_rgba(custom.border);
        base.window_border = Self::from_rgba(custom.window_border);
        // Round to nearest 0.5px to avoid sub-pixel rendering artifacts.
        base.window_border_width = (custom.window_border_width.clamp(0.0, 4.0) * 2.0).round() / 2.0;
        base.accent = Self::from_rgba(custom.accent);
        base.selected_bg = Self::from_rgba(custom.selected_bg);
        base.hover = Self::from_rgba(custom.hover);
        base.error = Self::from_rgba(custom.error);
        base.folder = Self::from_rgba(custom.folder);
        base.button_bg = Self::from_rgba(custom.button_bg);
        base.button_text = Self::from_rgba(custom.button_text);
        base.heading = Self::from_rgba(custom.text_heading);
        base.folder_text = Self::from_rgba(custom.text_folder);
        base.text_dim = Self::from_rgba(custom.text_dim);
        base.file = Self::from_rgba(custom.text_file);
        base.text = Self::from_rgba(custom.text);
        base.text_muted = Self::from_rgba(custom.text_muted);
        base.app_radius = custom.app_radius.clamp(0.0, 24.0) as u8;
        base.border_radius = custom.border_radius.clamp(0.0, 14.0) as u8;
        base.button_radius = custom.button_radius.clamp(0.0, 14.0) as u8;
        base.font_scale = custom.font_scale.clamp(0.85, 1.35);
        base.file_label_blend = custom.file_label_blend.clamp(0.0, 1.0);
        base.derive_overlays();
        self
    }

    pub(crate) fn resolved_mode(mode: ThemeMode, system_theme: Option<egui::Theme>) -> ThemeMode {
        match mode {
            ThemeMode::System => {
                if system_theme == Some(egui::Theme::Light) {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                }
            }
            other => other,
        }
    }

    pub(crate) fn base_for_config(config: &AppConfig, system_theme: Option<egui::Theme>) -> Self {
        Self::preset(
            config.theme_preset,
            Self::resolved_mode(config.theme, system_theme) != ThemeMode::Light,
        )
    }

    fn semantic_palette_for_preset(
        preset: ThemePreset,
        dark: bool,
        neutral: Color32,
    ) -> [Color32; 10] {
        // folder blue, link blue, steel, soft white, green, orange, purple, pink, red, yellow
        let tokens = match preset {
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
        };
        // Built-in inks deepen on light surfaces and soften on dark ones.
        // Saved palettes retain their exact colors through `customized`.
        let mut colors = tokens.map(|token| {
            mix_color(
                Self::from_rgba(token),
                if dark { Color32::WHITE } else { Color32::BLACK },
                if dark {
                    if preset == ThemePreset::Solarized {
                        0.34
                    } else {
                        0.25
                    }
                } else {
                    0.46
                },
            )
        });
        colors[3] = neutral;
        colors
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
    if !s.is_ascii() {
        return None;
    }
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

#[cfg(test)]
mod theme_resolution_tests {
    use super::*;

    #[test]
    fn system_mode_uses_desktop_preference_and_explicit_modes_ignore_it() {
        for preset in [
            ThemePreset::Ottrin,
            ThemePreset::Breeze,
            ThemePreset::Adwaita,
            ThemePreset::Windows11,
            ThemePreset::Solarized,
            ThemePreset::Nord,
            ThemePreset::G33k,
        ] {
            let mut config = AppConfig {
                theme: ThemeMode::System,
                theme_preset: preset,
                ..Default::default()
            };
            assert_eq!(
                Colors::for_config(&config, Some(egui::Theme::Light)),
                Colors::preset(preset, false)
            );
            assert_eq!(
                Colors::for_config(&config, Some(egui::Theme::Dark)),
                Colors::preset(preset, true)
            );
            assert_eq!(
                Colors::for_config(&config, None),
                Colors::preset(preset, true)
            );
            config.theme = ThemeMode::Light;
            assert_eq!(
                Colors::for_config(&config, Some(egui::Theme::Dark)),
                Colors::preset(preset, false)
            );
            config.theme = ThemeMode::Dark;
            assert_eq!(
                Colors::for_config(&config, Some(egui::Theme::Light)),
                Colors::preset(preset, true)
            );
        }
    }

    #[test]
    fn saved_theme_is_the_only_palette_source_and_survives_config_roundtrip() {
        let light = crate::OttrinApp::custom_from_colors(Colors::preset(ThemePreset::Nord, false));
        let dark = crate::OttrinApp::custom_from_colors(Colors::preset(ThemePreset::Nord, true));
        let saved = ottrin_core::SavedTheme {
            name: "Fixed palette".into(),
            base_preset: ThemePreset::Nord,
            dark: dark.clone(),
            light: light.clone(),
        };
        let mut config = AppConfig {
            theme: ThemeMode::System,
            theme_preset: ThemePreset::G33k,
            active_custom_theme: Some(saved.name.clone()),
            custom_themes: vec![saved],
            ..Default::default()
        };
        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("theme_custom").is_none());
        assert!(serialized.get("theme_custom_by_preset").is_none());
        config = serde_json::from_value(serialized).unwrap();
        for (mode, expected) in [(egui::Theme::Light, &light), (egui::Theme::Dark, &dark)] {
            assert_eq!(
                crate::OttrinApp::custom_from_colors(Colors::for_config(&config, Some(mode))),
                *expected
            );
        }
        let owned = Colors::for_config(&config, Some(egui::Theme::Dark));
        config.theme_preset = ThemePreset::Solarized;
        config.custom_themes[0].base_preset = ThemePreset::G33k;
        assert_eq!(Colors::for_config(&config, Some(egui::Theme::Dark)), owned);
        config.theme_preset = ThemePreset::G33k;
        config.custom_themes[0].dark.button_radius = 13.0;
        assert_eq!(Colors::for_config(&config, None).button_radius, 13);
        config.active_custom_theme = None;
        assert_eq!(
            Colors::for_config(&config, Some(egui::Theme::Light)),
            Colors::preset(ThemePreset::G33k, false)
        );
    }
}
