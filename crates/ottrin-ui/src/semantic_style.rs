use crate::icons::*;
use crate::listing::detect_executable;
use crate::theme_colors::{Colors, mix_color};
use egui::Color32;
use ottrin_core::{
    AppConfig, CodeSubtype, EntryKind, FileCategory, FileEntry, FileSemantic, FolderKind,
    SearchResultItem, ThemePreset, classify_file, platform_case_mode_current,
};
use std::path::Path;

#[derive(Clone, Copy)]
pub(crate) enum IconKey {
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
pub(crate) struct EntryVisualState {
    pub(crate) selected: bool,
    pub(crate) focused: bool,
    pub(crate) hovered: bool,
    pub(crate) opened: bool,
    pub(crate) symlink_dir: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct EntryVisualStyle {
    pub(crate) icon: IconKey,
    pub(crate) icon_color: Color32,
    pub(crate) text_color: Color32,
}

#[derive(Clone, Copy)]
pub(crate) struct SemanticPalette {
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

pub(crate) fn semantic_palette(config: &AppConfig, c: &Colors) -> SemanticPalette {
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

pub(crate) fn semantic_for_path(path: &Path) -> FileSemantic {
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

pub(crate) fn material_icon(icon: IconKey) -> &'static str {
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

pub(crate) fn style_for_semantic(
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

pub(crate) fn style_for_entry(
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

pub(crate) fn style_for_search_item(
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

pub(crate) fn file_icon_for_path(path: &Path) -> &'static str {
    let sem = semantic_for_path(path);
    material_icon(icon_for_semantic(&sem, EntryVisualState::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_colors::Colors;
    use ottrin_core::{AppConfig, CodeSubtype, EntryKind, FileCategory, FileEntry, FileSemantic};
    use std::path::PathBuf;

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
}
