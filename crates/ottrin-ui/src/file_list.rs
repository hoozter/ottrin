//! List rows shared by the main file view and the appearance editor.
use super::*;

pub(crate) struct ListRowLayout<'a> {
    pub index: usize,
    pub columns: &'a [ListColumn],
    pub widths: &'a [f32],
    pub scale: f32,
    pub thumbnails_enabled: bool,
}

pub(crate) fn row_height(scale: f32, c: Colors) -> f32 {
    (28.0 * scale.clamp(0.92, 1.28) * c.font_scale).round()
}
pub(crate) fn header_height(scale: f32, c: Colors) -> f32 {
    (26.0 * scale.clamp(0.92, 1.22) * c.font_scale).round()
}
pub(crate) fn column_label(column: ListColumn) -> &'static str {
    match column {
        ListColumn::Name => "Name",
        ListColumn::Kind => "Type",
        ListColumn::Size => "Size",
        ListColumn::Modified => "Modified",
    }
}
pub(crate) fn column_widths(available: f32, initial: &[f32]) -> Vec<f32> {
    let mut widths = initial.to_vec();
    let max_total = (available - 16.0).max(120.0);
    let mut used: f32 = widths.iter().sum();
    if used > max_total {
        let scale = max_total / used;
        for width in &mut widths {
            *width = (*width * scale).max(80.0);
        }
        used = widths.iter().sum();
    }
    if used < max_total
        && let Some(first) = widths.first_mut()
    {
        *first += max_total - used;
    }
    widths
}
pub(crate) fn paint_heading(
    ui: &egui::Ui,
    rect: Rect,
    column: ListColumn,
    direction: Option<bool>,
    c: Colors,
    scale: f32,
) {
    paint_sort_header(
        ui,
        rect.shrink2(Vec2::new(4.0, 0.0)),
        column_label(column),
        direction,
        FontId::proportional((11.5 * scale.sqrt()).max(10.2) * c.font_scale),
        if direction.is_some() {
            c.text_dim
        } else {
            c.text_muted
        },
        false,
    );
}

pub(crate) fn render_file_list_row(
    ui: &mut egui::Ui,
    entry: &FileEntry,
    selected: bool,
    layout: ListRowLayout<'_>,
    c: Colors,
    config: &AppConfig,
    mut thumbnail: Option<egui::Image<'static>>,
) -> egui::Response {
    let ListRowLayout {
        index: row_index,
        columns: visible_cols,
        widths: col_widths,
        scale,
        thumbnails_enabled,
    } = layout;
    let row_h = row_height(scale, c);
    let icon_size = (16.0 * scale.sqrt()).max(14.0);
    let name_size = (13.0 * scale.sqrt()).max(11.0) * c.font_scale;
    let meta_size = (12.0 * scale.sqrt()).max(10.0) * c.font_scale;
    let requested = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), row_h));
    let response = ui.allocate_rect(requested, Sense::click());
    let row_rect = response.rect;
    let row_fill = if selected {
        c.selected_bg
    } else if row_index % 2 == 0 {
        Color32::TRANSPARENT
    } else {
        c.row_alt
    };
    if row_fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(row_rect, 0.0, row_fill);
    }
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, true, selected, entry.name.as_str())
    });
    if response.hovered() && !selected {
        ui.painter().rect_filled(row_rect, 0.0, c.hover);
    }
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
        config,
    );

    for (idx, col) in visible_cols.iter().enumerate() {
        let w = col_widths[idx];
        match col {
            ListColumn::Name => {
                let icon_str = material_icon(style.icon);
                let thumb_size = (row_h - 7.0).clamp(18.0, 32.0);
                let thumb_rect = Rect::from_min_size(
                    egui::pos2(x + 6.0, y - thumb_size / 2.0),
                    Vec2::splat(thumb_size),
                );
                let mut painted_thumbnail = false;
                if thumbnails_enabled && image_thumbnail_candidate(entry) {
                    paint_thumbnail_slot(ui, thumb_rect, c);
                    if let Some(img) = thumbnail.take() {
                        paint_centered_image(ui, thumb_rect, img);
                        painted_thumbnail = true;
                    }
                }
                let mut leading_width = icon_size.ceil();
                if thumbnails_enabled {
                    // One leading slot keeps image and ordinary-file
                    // names aligned while thumbnails load or fail.
                    leading_width = leading_width.max(thumb_size);
                }
                if !painted_thumbnail {
                    let icon_rect = ui.painter().text(
                        egui::Pos2::new(x + 6.0, y),
                        Align2::LEFT_CENTER,
                        icon_str,
                        FontId::proportional(icon_size),
                        style.icon_color,
                    );
                    leading_width = leading_width.max(icon_rect.width());
                }
                let name_right = x + w - 6.0;
                let name_left = x + 6.0 + leading_width + file_row::ICON_TEXT_GAP * c.font_scale;
                let name_rect = Rect::from_min_max(
                    egui::pos2(name_left.min(name_right), row_rect.top()),
                    egui::pos2(name_right, row_rect.bottom()),
                );
                let font = FontId::proportional(name_size);
                let visible_name =
                    OttrinApp::middle_ellipsis(ui, &entry.name, name_rect.width(), &font);
                // Column widths are pixels, not character counts. Use
                // real glyph advances and keep painting inside the cell.
                ui.painter_at(name_rect).text(
                    egui::Pos2::new(name_rect.left(), y),
                    Align2::LEFT_CENTER,
                    &visible_name,
                    font,
                    style.text_color,
                );
                if visible_name != entry.name {
                    ui.interact(
                        name_rect,
                        ui.id().with(("list_name_tip", row_index)),
                        Sense::hover(),
                    )
                    .on_hover_text(entry.name.clone());
                }
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
    response
}
