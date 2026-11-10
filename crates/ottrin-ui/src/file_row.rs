//! File rows shared by the real file manager and its chooser.
use crate::{Colors, EntryVisualState, material_icon, style_for_entry};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Vec2, WidgetInfo, WidgetType};
use ottrin_core::{AppConfig, EntryKind, FileEntry};

pub(crate) const ICON_TEXT_GAP: f32 = 6.0;

#[derive(Clone, Copy)]
pub(crate) struct FileRowLayout {
    pub index: usize,
    pub width: f32,
    pub scale: f32,
}

pub(crate) fn file_row_height(scale: f32, colors: &Colors) -> f32 {
    (25.0 * scale.clamp(0.9, 1.35) * colors.font_scale).round()
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
    let row_h = file_row_height(scale, c);
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
    let painter = ui.painter().with_clip_rect(row_rect);
    let icon_rect = painter.text(
        egui::Pos2::new(row_rect.min.x + 10.0, row_rect.center().y),
        Align2::LEFT_CENTER,
        icon_str,
        FontId::proportional(icon_size),
        style.icon_color,
    );

    let gap = ICON_TEXT_GAP * c.font_scale;
    let text_left = icon_rect.right() + gap;
    let text_right = if is_dir {
        painter
            .text(
                egui::Pos2::new(row_rect.right() - 10.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                "\u{203A}",
                FontId::proportional(text_size),
                c.text_muted,
            )
            .left()
            - gap
    } else {
        row_rect.right() - 10.0
    }
    .max(row_rect.left());
    let name_rect = Rect::from_min_max(
        egui::pos2(text_left.min(text_right), row_rect.top()),
        egui::pos2(text_right, row_rect.bottom()),
    );
    // Measured leading/trailing visuals own their space even when density and
    // theme text size change independently. Clip names to the remaining cell.
    let label_job = crate::widget_style::single_line_text_job(
        entry.name.as_str(),
        FontId::proportional(text_size),
        style.text_color,
        name_rect.width().max(1.0),
    );
    let galley = ui.painter().layout_job(label_job);
    let elided = galley.elided;
    painter.with_clip_rect(name_rect).galley(
        egui::pos2(
            name_rect.left(),
            row_rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        style.text_color,
    );
    // NOTE: do NOT add_space here — allocate_rect already advanced the cursor.
    // Adding space here was the cause of double-row-height spacing.
    if elided {
        response.on_hover_text(entry.name.clone())
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_filename_words_use_row_width_before_eliding() {
        let ctx = egui::Context::default();
        crate::install_application_fonts(&ctx);
        let colors = Colors::preset(ottrin_core::ThemePreset::Ottrin, true);
        for prefix in ["00-", "invoice "] {
            let name = format!("{prefix}{}.txt", "W".repeat(40));
            let entry = FileEntry {
                path: std::path::PathBuf::from(&name),
                name: name.clone(),
                kind: EntryKind::File,
                is_executable: false,
                symlink_target_is_dir: None,
                size_bytes: None,
                modified_unix_secs: None,
            };
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(400.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_file_row(
                            ui,
                            &entry,
                            EntryVisualState {
                                selected: false,
                                focused: true,
                                hovered: false,
                                opened: false,
                                symlink_dir: false,
                            },
                            FileRowLayout {
                                index: 0,
                                width: 240.0,
                                scale: 1.0,
                            },
                            &colors,
                            &AppConfig::default(),
                        );
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
            assert_eq!(label.galley.rows.len(), 1);
            assert!(label.galley.elided);
            let painted = label.galley.rows[0].text();
            assert!(
                painted.chars().filter(|c| *c == 'W').count() >= 8,
                "Do not collapse a long filename to its prefix: {painted:?}"
            );
            assert!(painted.ends_with('…'));
            assert!(label.galley.size().x <= 200.5);
        }
    }
}
