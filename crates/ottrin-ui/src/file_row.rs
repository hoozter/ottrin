//! File rows shared by the real file manager and its chooser.
use crate::{Colors, EntryVisualState, material_icon, style_for_entry};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType};
use ottrin_core::{AppConfig, EntryKind, FileEntry};

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

    // egui rounds allocation bounds to its pixel grid. Paint those same bounds
    // so selection backgrounds, hit testing and theme targets cannot drift.
    let response = ui.allocate_rect(row_rect, Sense::click());
    let row_rect = response.rect;

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
    let icon_str = material_icon(style.icon);
    ui.painter().text(
        egui::Pos2::new(row_rect.min.x + 10.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        icon_str,
        FontId::proportional(icon_size),
        style.icon_color,
    );

    // Use measured glyph layout instead of character-count estimates. Reserve
    // the directory chevron so enlarged text never paints over that control.
    let mut label_job = egui::text::LayoutJob::simple_singleline(
        entry.name.clone(),
        FontId::proportional(text_size),
        style.text_color,
    );
    label_job.wrap.max_width = (row_rect.width() - if is_dir { 56.0 } else { 40.0 }).max(1.0);
    label_job.wrap.max_rows = 1;
    let galley = ui.painter().layout_job(label_job);
    let elided = galley.elided;
    ui.painter().galley(
        egui::pos2(
            row_rect.left() + 30.0,
            row_rect.center().y - galley.size().y / 2.0,
        ),
        galley,
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
    if elided {
        response.on_hover_text(entry.name.clone())
    } else {
        response
    }
}
