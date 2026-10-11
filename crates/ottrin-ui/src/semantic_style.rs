use crate::icons::*;
use crate::listing::detect_executable;
use crate::theme_colors::{Colors, mix_color};
use egui::Color32;
use ottrin_core::{
    AppConfig, CodeSubtype, EntryKind, FileCategory, FileEntry, FileSemantic, FolderKind,
    SearchResultItem, classify_file, platform_case_mode_current,
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

pub(crate) fn semantic_palette(c: &Colors) -> SemanticPalette {
    let [
        blue,
        link,
        steel,
        soft_white,
        green,
        orange,
        purple,
        pink,
        red,
        yellow,
    ] = c.palette;
    SemanticPalette {
        directory: blue,
        symlink: link,
        executable: green,
        image: purple,
        video: red,
        audio: link,
        archive: yellow,
        pdf: red,
        document: soft_white,
        spreadsheet: green,
        presentation: orange,
        font: mix_color(purple, pink, 0.45),
        disk_image: steel,
        package: mix_color(yellow, orange, 0.50),
        config: steel,
        code: yellow,
        code_rust: mix_color(orange, yellow, 0.45),
        code_js: mix_color(yellow, green, 0.25),
        code_ts: mix_color(blue, link, 0.40),
        code_markup: mix_color(purple, blue, 0.38),
        code_python: mix_color(green, blue, 0.40),
        code_shell: mix_color(green, link, 0.35),
        code_toml: mix_color(yellow, steel, 0.30),
        code_yaml: mix_color(link, steel, 0.28),
        unknown: soft_white,
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
    colorize: bool,
    colorize_folder_labels: bool,
) -> EntryVisualStyle {
    let text_mix = c.file_label_blend;
    let cat_color = semantic_color(semantic, p);
    let is_neutral_category = matches!(
        semantic.category,
        FileCategory::Document | FileCategory::Unknown
    );
    let folder_text_base = c.folder_text;
    let file_text_base = c.file;
    let category_text_base = if matches!(semantic.category, FileCategory::Directory) {
        folder_text_base
    } else {
        file_text_base
    };
    let icon_color = if colorize {
        cat_color
    } else if matches!(semantic.category, FileCategory::Directory) {
        c.folder
    } else {
        c.file
    };
    let neutral_label = is_neutral_category
        || (matches!(semantic.category, FileCategory::Directory) && !colorize_folder_labels);
    let mut text_color = if colorize && !neutral_label {
        // Selection emphasizes the file's own text role, never unrelated UI headings or muted text.
        let blend = if state.selected {
            text_mix.max(if state.focused { 0.62 } else { 0.60 })
        } else {
            text_mix
        };
        mix_color(cat_color, category_text_base, blend)
    } else {
        category_text_base
    };
    if state.hovered && !state.selected {
        text_color = mix_color(text_color, category_text_base, 0.22);
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
        &semantic_palette(c),
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
        &semantic_palette(c),
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
    use ottrin_core::ThemePreset;
    use ottrin_core::{AppConfig, CodeSubtype, EntryKind, FileCategory, FileEntry, FileSemantic};
    use std::path::PathBuf;

    #[test]
    fn file_row_states_do_not_inherit_unrelated_ui_text_or_focus_colors() {
        let original = Colors::preset(ThemePreset::Ottrin, true);
        let mut changed = original;
        changed.heading = Color32::from_rgb(220, 40, 90);
        changed.text_muted = Color32::from_rgb(40, 220, 90);
        changed.accent_dim = Color32::from_rgb(90, 40, 220);
        for category in [
            FileCategory::Directory,
            FileCategory::Symlink,
            FileCategory::Document,
            FileCategory::Image,
        ] {
            let semantic = FileSemantic {
                category,
                code_subtype: None,
                folder_kind: None,
                rule_hint: None,
            };
            for state in [
                EntryVisualState::default(),
                EntryVisualState {
                    selected: true,
                    focused: true,
                    ..Default::default()
                },
                EntryVisualState {
                    hovered: true,
                    ..Default::default()
                },
            ] {
                for colorize in [false, true] {
                    let before = style_for_semantic(
                        &semantic,
                        state,
                        &original,
                        &semantic_palette(&original),
                        colorize,
                        true,
                    );
                    let after = style_for_semantic(
                        &semantic,
                        state,
                        &changed,
                        &semantic_palette(&changed),
                        colorize,
                        true,
                    );
                    assert_eq!(
                        (before.icon_color, before.text_color),
                        (after.icon_color, after.text_color),
                        "file role leaked unrelated UI colors: {category:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn bundled_palettes_keep_interface_and_file_roles_readable() {
        fn luminance(color: Color32) -> f32 {
            let rgb = color.to_srgba_unmultiplied();
            let linear = |channel: u8| {
                let value = channel as f32 / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (a, b) = (luminance(a), luminance(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        }
        fn composite(foreground: Color32, background: Color32) -> Color32 {
            let fg = foreground.to_srgba_unmultiplied();
            let bg = background.to_srgba_unmultiplied();
            let alpha = fg[3] as f32 / 255.0;
            let channel =
                |i: usize| (fg[i] as f32 * alpha + bg[i] as f32 * (1.0 - alpha)).round() as u8;
            Color32::from_rgb(channel(0), channel(1), channel(2))
        }
        let mut failures = Vec::new();
        for preset in [
            ThemePreset::Ottrin,
            ThemePreset::Breeze,
            ThemePreset::Adwaita,
            ThemePreset::Windows11,
            ThemePreset::Solarized,
            ThemePreset::Nord,
            ThemePreset::G33k,
        ] {
            for dark in [false, true] {
                let c = Colors::preset(preset, dark);
                for (surface_name, surface) in [
                    ("canvas", c.bg),
                    ("panel", c.panel),
                    ("raised", c.panel_raised),
                    ("title", c.titlebar_bg),
                    ("toolbar", c.toolbar_bg),
                    ("sidebar", c.smart_panel_bg),
                    ("status", c.statusbar_bg),
                ] {
                    for (role, ink) in [
                        ("primary", c.text),
                        ("heading", c.heading),
                        ("secondary", c.text_dim),
                        ("muted", c.text_muted),
                        ("error", c.error),
                        ("link / accent text", c.accent),
                    ] {
                        let ratio = contrast(composite(ink, surface), surface);
                        if ratio < 4.5 {
                            failures.push(format!(
                                "{preset:?} dark={dark} {surface_name} {role}: {ratio:.2}"
                            ));
                        }
                    }
                }
                let mut semantics: Vec<_> = [
                    FileCategory::Directory,
                    FileCategory::Symlink,
                    FileCategory::Executable,
                    FileCategory::Image,
                    FileCategory::Video,
                    FileCategory::Audio,
                    FileCategory::Archive,
                    FileCategory::Pdf,
                    FileCategory::Document,
                    FileCategory::Spreadsheet,
                    FileCategory::Presentation,
                    FileCategory::Font,
                    FileCategory::DiskImage,
                    FileCategory::Package,
                    FileCategory::Config,
                    FileCategory::Code,
                    FileCategory::Unknown,
                ]
                .into_iter()
                .map(|category| FileSemantic {
                    category,
                    code_subtype: None,
                    folder_kind: None,
                    rule_hint: None,
                })
                .collect();
                semantics.extend(
                    [
                        CodeSubtype::Rust,
                        CodeSubtype::JavaScript,
                        CodeSubtype::TypeScript,
                        CodeSubtype::HTML,
                        CodeSubtype::CSS,
                        CodeSubtype::JSON,
                        CodeSubtype::Markdown,
                        CodeSubtype::Python,
                        CodeSubtype::Shell,
                        CodeSubtype::TOML,
                        CodeSubtype::YAML,
                        CodeSubtype::Other,
                    ]
                    .into_iter()
                    .map(|code_subtype| FileSemantic {
                        category: FileCategory::Code,
                        code_subtype: Some(code_subtype),
                        folder_kind: None,
                        rule_hint: None,
                    }),
                );
                for semantic in semantics {
                    for (name, state, surface) in [
                        ("normal", EntryVisualState::default(), c.bg),
                        (
                            "striped",
                            EntryVisualState::default(),
                            composite(c.row_alt, c.bg),
                        ),
                        (
                            "hover",
                            EntryVisualState {
                                hovered: true,
                                ..Default::default()
                            },
                            composite(c.hover, c.bg),
                        ),
                        (
                            "selected",
                            EntryVisualState {
                                selected: true,
                                ..Default::default()
                            },
                            composite(c.selected_bg, c.bg),
                        ),
                        (
                            "focused selection",
                            EntryVisualState {
                                selected: true,
                                focused: true,
                                ..Default::default()
                            },
                            composite(c.selected_bg, c.bg),
                        ),
                    ] {
                        for colorize in [false, true] {
                            let style = style_for_semantic(
                                &semantic,
                                state,
                                &c,
                                &semantic_palette(&c),
                                colorize,
                                true,
                            );
                            for (role, color, minimum) in [
                                ("icon", style.icon_color, 3.0),
                                ("label", style.text_color, 4.5),
                            ] {
                                let ratio = contrast(composite(color, surface), surface);
                                if ratio < minimum {
                                    failures.push(format!("{preset:?} dark={dark} {semantic:?} {name} colorize={colorize} {role}: {ratio:.2}"));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "Unreadable bundled interface or file roles:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn opened_folder_uses_open_icon_variant() {
        let cfg = AppConfig::default();
        let c = Colors::for_config(&cfg, None);
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
        let c = Colors::for_config(&cfg, None);
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
        let c = Colors::for_config(&cfg, None);
        let p = semantic_palette(&c);
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
        let rust_style =
            style_for_semantic(&rust, EntryVisualState::default(), &c, &p, true, false);
        let ts_style = style_for_semantic(&ts, EntryVisualState::default(), &c, &p, true, false);
        assert_ne!(rust_style.icon_color, ts_style.icon_color);
        assert!(matches!(rust_style.icon, IconKey::Code));
    }
}
