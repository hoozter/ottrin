//! File rows shared by the real file manager and its chooser.
use crate::{Colors, EntryVisualState, material_icon, style_for_entry, truncate_middle};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType};
use ottrin_core::{AppConfig, EntryKind, FileEntry, MillerColumnWidthMode};

#[derive(Clone, Copy)]
pub(crate) struct FileRowLayout {
    pub index: usize,
    pub width: f32,
    pub scale: f32,
}

pub(crate) fn render_file_row(
    ui: &mut egui::Ui,
    entry: &FileEntry,
    state: EntryVisualState,
    layout: FileRowLayout,
    c: &Colors,
    config: &AppConfig,
) -> egui::Response {
    let FileRowLayout {
        index: row_index,
        width: row_width,
        scale,
    } = layout;
    let selected = state.selected;
    let is_active_col = state.focused;
    let opened = state.opened;
    let row_h = (25.0 * scale.clamp(0.9, 1.35) * c.font_scale).round();
    let icon_size = (16.0 * scale.sqrt()).max(14.0);
    let text_size = (13.0 * scale.sqrt()).max(11.0) * c.font_scale;
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
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Button, true, selected, entry.name.as_str())
    });

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
        config,
    );
    let mut label = entry.name.clone();
    if config.miller_column_width_mode == MillerColumnWidthMode::Fixed {
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
    if label != entry.name {
        response.on_hover_text(entry.name.clone())
    } else {
        response
    }
}
