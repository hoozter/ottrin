//! Shared file-manager and chooser grid cells: geometry, semantics and theme.
use super::*;

pub(crate) struct GridMetrics {
    pub size: Vec2,
    pub icon_size: f32,
    pub gap: f32,
    pub side_padding: f32,
    label_size: f32,
    icon_gap: f32,
    scale: f32,
}
impl GridMetrics {
    pub(crate) fn new(scale: f32, colors: Colors) -> Self {
        let scale = scale.clamp(0.75, 1.85);
        let icon_size = (48.0 * scale).round();
        let label_size = (11.0 * scale.sqrt()).max(9.0) * colors.font_scale;
        let gap = (10.0 * scale).round().max(8.0);
        let icon_gap = (8.0 * scale.clamp(0.9, 1.35)).round();
        Self {
            size: Vec2::new(
                (icon_size * 1.18).max(84.0),
                icon_size + icon_gap + label_size * 2.2 + 18.0,
            ),
            icon_size,
            gap,
            side_padding: (gap * 1.2).max(18.0),
            label_size,
            icon_gap,
            scale,
        }
    }
}
pub(crate) enum GridThumbnail {
    Icon,
    Loading,
    Ready(egui::Image<'static>),
}

pub(crate) fn render_file_grid_cell(
    ui: &mut egui::Ui,
    entry: &FileEntry,
    selected: bool,
    scale: f32,
    colors: Colors,
    config: &AppConfig,
    thumbnail: GridThumbnail,
) -> egui::Response {
    let metrics = GridMetrics::new(scale, colors);
    let (rect, response) = ui.allocate_exact_size(metrics.size, Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &entry.name));
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
        &colors,
        config,
    );
    let fill = if selected {
        colors.selected_bg
    } else if response.hovered() {
        colors.hover
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, colors.border_radius as f32, fill);
    }
    if selected {
        ui.painter().rect_stroke(
            rect,
            colors.border_radius as f32,
            Stroke::new(1.0, colors.accent),
            egui::StrokeKind::Middle,
        );
    }
    let icon_y = rect.min.y + 8.0 * metrics.scale.clamp(0.9, 1.3);
    let icon_center = egui::pos2(rect.center().x, icon_y + metrics.icon_size * 0.5);
    let icon_rect = Rect::from_center_size(icon_center, Vec2::splat(metrics.icon_size));
    match thumbnail {
        GridThumbnail::Icon => {
            ui.painter().text(
                icon_center,
                Align2::CENTER_CENTER,
                material_icon(style.icon),
                FontId::proportional(metrics.icon_size),
                style.icon_color,
            );
        }
        GridThumbnail::Loading => paint_thumbnail_slot(ui, icon_rect, colors),
        GridThumbnail::Ready(image) => {
            paint_thumbnail_slot(ui, icon_rect, colors);
            paint_centered_image(ui, icon_rect, image);
        }
    }
    // Measure the actual font against the tile, using both reserved label lines.
    // Character-count truncation hides distinguishing filename suffixes even when
    // the complete name would fit, particularly with narrow glyphs.
    let mut job = egui::text::LayoutJob::simple(
        entry.name.clone(),
        FontId::proportional(metrics.label_size),
        style.text_color,
        rect.width() - 12.0,
    );
    job.wrap.max_rows = 2;
    // Prefer filename separators over splitting a short extension mid-word.
    job.wrap.break_anywhere = false;
    job.halign = egui::Align::Center;
    job.break_on_newline = false;
    let label = ui.painter().layout_job(job);
    let elided = label.elided;
    ui.painter().galley(
        egui::pos2(
            rect.center().x,
            icon_y + metrics.icon_size + metrics.icon_gap,
        ),
        label,
        style.text_color,
    );
    if elided {
        response.on_hover_text(&entry.name)
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filenames_use_two_measured_lines_without_escaping_the_tile() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        for scale in [0.75, 1.0, 1.85] {
            for font_scale in [1.0, 1.5] {
                for name in [
                    "owned-0025.txt",
                    "iiiiiiiiiiiiiiiiiiii.txt",
                    "WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt",
                    "a filename with spaces and a very long description.txt",
                ] {
                    let mut colors = Colors::preset(ThemePreset::Ottrin, true);
                    colors.font_scale = font_scale;
                    let entry = FileEntry {
                        name: name.into(),
                        path: PathBuf::from(name),
                        kind: EntryKind::File,
                        is_executable: false,
                        symlink_target_is_dir: None,
                        size_bytes: None,
                        modified_unix_secs: None,
                    };
                    let mut tile = Rect::NOTHING;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::splat(500.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                tile = render_file_grid_cell(
                                    ui,
                                    &entry,
                                    false,
                                    scale,
                                    colors,
                                    &AppConfig::default(),
                                    GridThumbnail::Icon,
                                )
                                .rect;
                            });
                        },
                    );
                    let label = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == name => Some(text),
                            _ => None,
                        })
                        .unwrap();
                    assert!(label.galley.rows.len() <= 2);
                    let bounds = label.galley.rect.translate(label.pos.to_vec2());
                    assert!(
                        tile.expand(0.5).contains_rect(bounds),
                        "{name} {scale} {font_scale}: {bounds:?} outside {tile:?}"
                    );
                    assert!((bounds.center().x - tile.center().x).abs() < 0.5);
                    if name == "owned-0025.txt" && font_scale == 1.0 {
                        assert!(
                            !label.galley.elided,
                            "Distinct fixture names must remain readable: {scale}"
                        );
                    }
                    if name == "owned-0025.txt" && !label.galley.elided {
                        assert!(
                            label.galley.rows.last().unwrap().text().ends_with(".txt"),
                            "Keep the short extension together: {scale} {font_scale}"
                        );
                    }
                    if name.starts_with("WWWW") {
                        assert!(label.galley.elided);
                        assert_eq!(label.galley.rows.len(), 2);
                    }
                }
            }
        }
    }

    #[test]
    fn grid_cells_render_separate_semantic_icon_and_label_colors_and_themed_corners() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        let entry = FileEntry {
            name: "folder".into(),
            path: PathBuf::from("/folder"),
            kind: EntryKind::Directory,
            is_executable: false,
            symlink_target_is_dir: None,
            size_bytes: None,
            modified_unix_secs: None,
        };
        let config = AppConfig {
            colorize_file_types: false,
            ..Default::default()
        };
        for dark in [false, true] {
            let mut colors = Colors::preset(ThemePreset::Ottrin, dark);
            colors.folder = Color32::from_rgb(220, 60, 70);
            colors.folder_text = Color32::from_rgb(20, 170, 180);
            colors.border_radius = 13;
            theme_colors::apply_to_context(&ctx, &colors);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(400.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_file_grid_cell(
                            ui,
                            &entry,
                            true,
                            1.0,
                            colors,
                            &config,
                            GridThumbnail::Icon,
                        );
                    });
                },
            );
            let text: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect();
            assert!(text.iter().any(|text| {
                text.galley.job.text == entry.name
                    && text
                        .galley
                        .job
                        .sections
                        .iter()
                        .all(|section| section.format.color == colors.folder_text)
            }));
            assert!(text.iter().any(|text| {
                text.galley.job.text != entry.name
                    && text.galley.job.sections.iter().all(|section| {
                        section.format.color == colors.folder && section.format.font_id.size == 48.0
                    })
            }));
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect)
                if rect.fill == colors.selected_bg && rect.corner_radius.nw == 13))
            );
        }
    }
}
