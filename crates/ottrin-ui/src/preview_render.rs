use crate::{
    Colors, MI_ARROW_BACK, MI_ARROW_FORWARD, MI_CONTENT_COPY, MI_DELETE, MI_EDIT, MI_FOLDER_OPEN,
    MI_FOLDER_ZIP, MI_OPEN_IN_NEW, file_icon_for_path, mix_color,
};
use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, ScrollArea, Sense, Stroke, Vec2,
};
use ottrin_preview::{PreviewData, PreviewKind};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericPreviewAction {
    Rename,
    Delete,
}

pub(crate) fn inspector_action_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    c: Colors,
) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(format!("{icon}  {label}"))
                .size(11.5)
                .color(c.text),
        )
        .fill(c.panel)
        .stroke(Stroke::new(1.0, mix_color(c.border, c.panel_raised, 0.35)))
        .corner_radius(6.0)
        .min_size(Vec2::new(108.0, 30.0)),
    )
    .on_hover_text(tooltip)
}

pub(crate) fn inspector_icon_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    c: Colors,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(16.0).color(c.text))
            .fill(c.panel)
            .stroke(Stroke::new(1.0, mix_color(c.border, c.panel_raised, 0.35)))
            .corner_radius(6.0)
            .min_size(Vec2::new(30.0, 28.0)),
    )
    .on_hover_text(format!("{label}\n{tooltip}"))
}

pub(crate) fn compact_preview_metadata(name: &str, body: &str) -> String {
    let mut parts = vec![name.to_string()];
    parts.extend(
        body.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("Image preview available.")
                    || trimmed.eq_ignore_ascii_case("Open externally for full editing.")
                {
                    None
                } else if trimmed.ends_with('.') && !trimmed.contains(':') {
                    Some(trimmed.trim_end_matches('.').to_string())
                } else {
                    Some(trimmed.to_string())
                }
            })
            .take(3),
    );
    parts.join(" · ")
}

pub(crate) fn split_preview_extracted_text(body: &str) -> (&str, Option<&str>) {
    const MARKER: &str = "\n\nExtracted text:\n";
    if let Some((metadata, text)) = body.split_once(MARKER) {
        (metadata, Some(text.trim()))
    } else {
        (body, None)
    }
}

fn split_archive_listing(body: &str) -> (&str, Option<&str>) {
    const MARKER: &str = "\nContents:\n";
    if let Some((metadata, listing)) = body.split_once(MARKER) {
        (metadata, Some(listing.trim()))
    } else {
        (body, None)
    }
}

pub(crate) fn preview_payload_for_copy(
    preview: &PreviewData,
) -> Option<(&'static str, &'static str, &str)> {
    match preview.kind {
        PreviewKind::Text | PreviewKind::Pdf | PreviewKind::OfficeMetadata => {
            let (_, extracted) = split_preview_extracted_text(&preview.body);
            let payload = extracted.unwrap_or(preview.body.trim());
            (!payload.is_empty()).then_some((MI_CONTENT_COPY, "Copy Text", payload))
        }
        PreviewKind::ArchiveMetadata => {
            let (_, listing) = split_archive_listing(&preview.body);
            listing
                .filter(|payload| !payload.is_empty())
                .map(|payload| (MI_CONTENT_COPY, "Copy File List", payload))
        }
        _ => None,
    }
}

pub(crate) fn render_preview_footer(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(
            RichText::new(compact_preview_metadata(name, body))
                .color(c.text_dim)
                .size(11.0),
        );
    });
}

pub(crate) fn scrollable_preview_height(available_height: f32, minimum_height: f32) -> f32 {
    (available_height - 30.0).max(minimum_height)
}

fn render_scrollable_preview_text(
    ui: &mut egui::Ui,
    text: &str,
    color: Color32,
    minimum_height: f32,
    id: impl std::hash::Hash,
) {
    let height = scrollable_preview_height(ui.available_height(), minimum_height);
    let width = ui.available_width();
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
        .show(ui, |ui| {
            ui.set_width(width);
            ScrollArea::vertical()
                .id_salt(id)
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.add(
                        egui::Label::new(
                            RichText::new(text)
                                .font(FontId::monospace(12.0))
                                .color(color),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
        });
}

pub(crate) fn render_text_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    let source = extracted.unwrap_or(body).trim();
    render_scrollable_preview_text(ui, source, c.text, 160.0, ("inspector_text", name));
    render_preview_footer(ui, c, name, metadata);
}

pub(crate) fn render_archive_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    let (metadata, listing) = split_archive_listing(body);
    let listing = listing.unwrap_or("").trim();
    if listing.is_empty() {
        render_metadata_card_preview(ui, c, MI_FOLDER_ZIP, name, metadata);
        return;
    }
    render_scrollable_preview_text(ui, listing, c.text, 180.0, ("inspector_archive", name));
    render_preview_footer(ui, c, name, metadata);
}

pub(crate) fn render_document_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    name: &str,
    body: &str,
) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    if let Some(text) = extracted.filter(|text| !text.is_empty()) {
        render_scrollable_preview_text(ui, text, c.text, 180.0, ("inspector_document", name));
        render_preview_footer(ui, c, name, metadata);
    } else {
        render_metadata_card_preview(ui, c, icon, name, metadata);
    }
}

#[allow(deprecated)]
pub(crate) fn render_docx_page_preview(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    body: &str,
    fit: bool,
    zoom: f32,
) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    let Some(text) = extracted.filter(|text| !text.is_empty()) else {
        render_metadata_card_preview(ui, c, MI_OPEN_IN_NEW, name, metadata);
        return;
    };

    const PAGE_WIDTH: f32 = 816.0;
    const PAGE_HEIGHT: f32 = 1056.0;
    let fit_scale = (ui.available_width() / PAGE_WIDTH)
        .min((ui.available_height().max(240.0) - 8.0) / PAGE_HEIGHT)
        .clamp(0.2, 1.0);
    let scale = if fit { fit_scale } else { zoom.clamp(0.1, 8.0) };
    let page_size = Vec2::new(PAGE_WIDTH * scale, PAGE_HEIGHT * scale);
    let margin = 72.0 * scale;
    let font_size = (15.0 * scale).max(4.0);

    ui.vertical_centered(|ui| {
        let page = egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, Color32::from_gray(190)))
            .inner_margin(egui::Margin::same(0))
            .show(ui, |ui| {
                ui.set_min_size(page_size);
                ui.set_max_size(page_size);
                ui.allocate_ui_at_rect(
                    Rect::from_min_max(
                        ui.min_rect().min + Vec2::splat(margin),
                        ui.min_rect().min + page_size - Vec2::splat(margin),
                    ),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .font(FontId::proportional(font_size))
                                    .color(Color32::from_rgb(28, 28, 30)),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    },
                );
            });
        page.response
            .on_hover_text("Ctrl+wheel or pinch to zoom. Drag or scroll to pan.");
        render_preview_footer(ui, c, name, metadata);
    });
}

pub(crate) fn render_folder_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    ui.add_space(16.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(RichText::new(MI_FOLDER_OPEN).size(58.0).color(c.folder));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
    });
    ui.add_space(10.0);
    let (metadata, sample) = if let Some((metadata, sample)) = body.split_once("\n\nSample:\n") {
        (metadata, Some(sample))
    } else {
        (body, None)
    };
    for line in metadata.lines() {
        if let Some((key, val)) = line.split_once(':') {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{key}:"))
                        .color(c.text_muted)
                        .size(11.5),
                );
                ui.label(RichText::new(val.trim()).color(c.text).size(11.5));
            });
        } else if !line.trim().is_empty() {
            ui.label(RichText::new(line).color(c.text_dim).size(11.5));
        }
    }
    if let Some(sample) = sample {
        ui.add_space(10.0);
        ui.label(
            RichText::new("Sample contents")
                .color(c.text)
                .size(12.0)
                .strong(),
        );
        let mut text = sample.to_string();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .font(FontId::monospace(11.0))
                .text_color(c.text_dim)
                .frame(true)
                .desired_rows(10)
                .interactive(true),
        );
    }
}

pub(crate) fn render_generic_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    path: &Path,
    name: &str,
    preview: &PreviewData,
) -> Option<GenericPreviewAction> {
    let mut action = None;
    ui.add_space(20.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let icon = file_icon_for_path(path);
        ui.label(RichText::new(icon).size(64.0));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
        if !preview.body.is_empty() {
            ui.add_space(8.0);
            render_preview_metadata_rows(ui, c, &preview.body);
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if inspector_action_button(
                ui,
                MI_CONTENT_COPY,
                "Copy Path",
                "Copy this file path to the clipboard.",
                c,
            )
            .clicked()
            {
                ui.ctx().copy_text(path.display().to_string());
            }
            if preview.open_external
                && inspector_action_button(
                    ui,
                    MI_OPEN_IN_NEW,
                    "Open",
                    "Open this file with the system default app.",
                    c,
                )
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl {
                    url: format!("file://{}", path.display()),
                    new_tab: false,
                });
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if inspector_action_button(ui, MI_EDIT, "Rename", "Rename this file.", c).clicked() {
                action = Some(GenericPreviewAction::Rename);
            }
            if inspector_action_button(
                ui,
                MI_DELETE,
                "Move to Trash",
                "Move this file to Trash with confirmation.",
                c,
            )
            .clicked()
            {
                action = Some(GenericPreviewAction::Delete);
            }
        });
    });
    action
}

pub(crate) fn render_metadata_card_preview(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    name: &str,
    metadata: &str,
) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() * 0.18).min(48.0));
        ui.label(RichText::new(icon).size(58.0).color(c.folder));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
        ui.add_space(10.0);
        render_preview_metadata_rows(ui, c, metadata);
    });
}

fn render_preview_metadata_rows(ui: &mut egui::Ui, c: Colors, metadata: &str) {
    for line in metadata.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{key}:"))
                        .color(c.text_muted)
                        .size(11.5),
                );
                ui.label(RichText::new(val.trim()).color(c.text).size(11.5));
            });
        } else {
            ui.label(RichText::new(line).color(c.text_dim).size(11.5));
        }
    }
}

pub(crate) fn render_image_hover_nav(
    ui: &mut egui::Ui,
    c: Colors,
    image_rect: Rect,
) -> Option<isize> {
    let pointer = ui.input(|input| input.pointer.hover_pos());
    if !pointer
        .map(|pos| image_rect.expand(8.0).contains(pos))
        .unwrap_or(false)
    {
        return None;
    }

    let arrow_size = Vec2::new(42.0, 64.0);
    let left_rect = Rect::from_center_size(
        egui::pos2(image_rect.left() + 30.0, image_rect.center().y),
        arrow_size,
    );
    let right_rect = Rect::from_center_size(
        egui::pos2(image_rect.right() - 30.0, image_rect.center().y),
        arrow_size,
    );
    let left = ui.interact(left_rect, ui.id().with("quick_prev_image"), Sense::click());
    let right = ui.interact(right_rect, ui.id().with("quick_next_image"), Sense::click());

    for (rect, response, icon) in [
        (left_rect, &left, MI_ARROW_BACK),
        (right_rect, &right, MI_ARROW_FORWARD),
    ] {
        let fill = if response.hovered() {
            mix_color(c.selected_bg, c.panel_raised, 0.18)
        } else {
            Color32::from_rgba_unmultiplied(
                c.panel_raised.r(),
                c.panel_raised.g(),
                c.panel_raised.b(),
                176,
            )
        };
        ui.painter().rect_filled(rect, 10.0, fill);
        ui.painter().rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, mix_color(c.border, c.accent, 0.25)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(24.0),
            c.text,
        );
    }

    if left.clicked() {
        Some(-1)
    } else if right.clicked() {
        Some(1)
    } else {
        None
    }
}
