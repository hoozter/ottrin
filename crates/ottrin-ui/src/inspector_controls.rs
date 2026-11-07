//! Shared Inspector controls. Callers own file actions and preview-only selection.
use super::*;

#[derive(Clone, Copy, Default)]
pub(crate) struct Availability {
    pub editing: bool,
    pub can_save: bool,
    pub can_edit: bool,
    pub image: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Edit,
    Cancel,
    Save,
    RotateRight,
    RotateLeft,
}

pub(crate) struct Actions {
    pub action: Option<Action>,
    pub more: egui::Response,
    pub targets: Vec<Rect>,
}

fn more_button(c: Colors) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(MI_MORE_VERT)
            .color(c.button_text)
            .size(18.0 * c.font_scale),
    )
}
fn name_more(response: egui::Response) -> egui::Response {
    widget_style::name_control(&response, "More actions");
    response.on_hover_text("More actions")
}
pub(crate) enum MoreAction<'a> {
    ShowInOttrin,
    CopyPath,
    CopyPayload { label: &'static str, text: &'a str },
    Copy,
    Cut,
    Rename,
    OpenExternal,
    Reveal,
    Trash,
}

pub(crate) fn more_menu<'a>(
    ui: &mut egui::Ui,
    c: Colors,
    standalone: bool,
    preview: &'a PreviewData,
) -> egui::InnerResponse<Option<MoreAction<'a>>> {
    let menu = widget_style::themed_menu_from_button(ui, more_button(c), c, |ui| {
        let mut action = None;
        let mut item = |ui: &mut egui::Ui, icon, label, hint: &str, selected| {
            let response = menu_item(ui, icon, label, &c);
            let response = if hint.is_empty() {
                response
            } else {
                response.on_hover_text(hint)
            };
            if response.clicked() {
                action = Some(selected);
                ui.close();
            }
        };
        if standalone {
            item(
                ui,
                MI_FOLDER_OPEN,
                "Show in Ottrin",
                "",
                MoreAction::ShowInOttrin,
            );
        }
        item(ui, MI_CONTENT_COPY, "Copy Path", "", MoreAction::CopyPath);
        if let Some((icon, label, text)) = preview_render::preview_payload_for_copy(preview) {
            item(ui, icon, label, "", MoreAction::CopyPayload { label, text });
        }
        item(
            ui,
            MI_CONTENT_COPY,
            "Copy",
            "Copy this file for pasting in Ottrin.",
            MoreAction::Copy,
        );
        item(
            ui,
            MI_CONTENT_CUT,
            "Cut",
            "Cut this file for moving in Ottrin.",
            MoreAction::Cut,
        );
        item(ui, MI_EDIT, "Rename", "", MoreAction::Rename);
        if preview.open_external {
            item(
                ui,
                MI_OPEN_IN_NEW,
                "Open externally",
                "",
                MoreAction::OpenExternal,
            );
        }
        item(ui, MI_OPEN_IN_NEW, "Reveal", "", MoreAction::Reveal);
        item(ui, MI_DELETE, "Move to Trash", "", MoreAction::Trash);
        action
    });
    egui::InnerResponse::new(menu.inner.flatten(), name_more(menu.response))
}

pub(crate) fn action_row(
    ui: &mut egui::Ui,
    c: Colors,
    availability: Availability,
    more: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> Actions {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let more = more(ui);
            let mut targets = vec![more.rect];
            let mut action = None;
            let mut activate = |response: egui::Response, candidate| {
                targets.push(response.rect);
                if response.clicked() {
                    action = Some(candidate);
                }
            };
            if availability.editing {
                activate(
                    inspector_icon_button(
                        ui,
                        MI_EDIT,
                        "Cancel edits",
                        "Discard the unsaved text edits without changing the file.",
                        c,
                    ),
                    Action::Cancel,
                );
                let administrator = administrator_session();
                activate(
                    ui.add_enabled(
                        availability.can_save,
                        egui::Button::new(administrator_surface_label("Save", administrator))
                            .themed(&c),
                    )
                    .on_hover_text(administrator_surface_label(
                        "Atomically save the edited text (Ctrl+S).",
                        administrator,
                    )),
                    Action::Save,
                );
            } else if availability.can_edit {
                activate(
                    inspector_icon_button(
                        ui,
                        MI_EDIT,
                        "Edit text",
                        "Edit this complete UTF-8 text file.",
                        c,
                    ),
                    Action::Edit,
                );
            }
            if availability.image {
                activate(
                    inspector_icon_button(
                        ui,
                        MI_ROTATE_RIGHT,
                        "Rotate Right",
                        "Rotate image right and save the file.",
                        c,
                    ),
                    Action::RotateRight,
                );
                activate(
                    inspector_icon_button(
                        ui,
                        MI_ROTATE_LEFT,
                        "Rotate Left",
                        "Rotate image left and save the file.",
                        c,
                    ),
                    Action::RotateLeft,
                );
            }
            Actions {
                action,
                more,
                targets,
            }
        })
        .inner
    })
    .inner
}

pub(crate) struct SizingControls {
    pub changed: bool,
    pub targets: Vec<Rect>,
}

pub(crate) fn text_size(ui: &mut egui::Ui, c: Colors, scale: &mut f32) -> SizingControls {
    *scale = scale.clamp(0.7, 2.0);
    let mut changed = false;
    let smaller = ui.add(egui::Button::new("− text").small().themed(&c));
    if smaller.clicked() {
        *scale = (*scale / 1.15).clamp(0.7, 2.0);
        changed = true;
    }
    ui.label(
        RichText::new(format!("Text {}%", (*scale * 100.0).round() as u32))
            .color(c.text_dim)
            .size(10.5 * c.font_scale),
    );
    let larger = ui.add(egui::Button::new("+ text").small().themed(&c));
    if larger.clicked() {
        *scale = (*scale * 1.15).clamp(0.7, 2.0);
        changed = true;
    }
    SizingControls {
        changed,
        targets: vec![smaller.rect, larger.rect],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SampleKind {
    #[default]
    Text,
    Document,
    Spreadsheet,
    Image,
    Pdf,
}
impl SampleKind {
    fn label(self) -> &'static str {
        match self {
            Self::Text => "Text file",
            Self::Document => "Document",
            Self::Spreadsheet => "Spreadsheet",
            Self::Image => "Image",
            Self::Pdf => "PDF",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ThemePreviewState {
    kind: SampleKind,
    sizing: PreviewBodySizing,
    selected_sheet: Option<String>,
    visual: VisualSampleSizing,
    page_index: usize,
}

#[derive(Debug, Clone)]
struct VisualSampleSizing {
    fit: bool,
    fit_width: bool,
    zoom: f32,
}

impl Default for VisualSampleSizing {
    fn default() -> Self {
        Self {
            fit: true,
            fit_width: false,
            zoom: 1.0,
        }
    }
}

const IMAGE_SAMPLE: &[u8] = include_bytes!("../../../assets/inspector-sample.png");

const PDF_SAMPLE: &[u8] = include_bytes!("../../../assets/inspector-sample.pdf");
const PDF_SAMPLE_PAGES: usize = 2;

fn sample_visual_image(
    kind: SampleKind,
    spec: VisualPreviewRenderSpec,
) -> Result<egui::Image<'static>, &'static str> {
    use std::sync::OnceLock;
    static PDF_PAGES: [OnceLock<Result<Vec<u8>, String>>; PDF_SAMPLE_PAGES] =
        [const { OnceLock::new() }; PDF_SAMPLE_PAGES];
    let (uri, bytes) = if kind == SampleKind::Pdf {
        let page = PDF_PAGES[spec.page_index].get_or_init(|| {
            ottrin_preview::render_pdf_bytes_page_png(
                PDF_SAMPLE,
                spec.page_index,
                PREVIEW_CACHE_BUCKET_XL,
            )
            .map_err(|error| error.to_string())
        });
        (
            format!("bytes://theme-sample/document-page-{}.png", spec.page_index),
            page.as_deref().map_err(String::as_str)?,
        )
    } else {
        (
            "bytes://theme-sample/inspector-sample.png".to_owned(),
            IMAGE_SAMPLE,
        )
    };
    if spec.fit {
        Ok(preview_image_with_fit_bounds(
            uri,
            bytes.to_vec(),
            spec.fit_bounds(),
            spec.corner_radius,
        ))
    } else {
        preview_image_with_zoom(uri, bytes.to_vec(), spec.zoom, spec.corner_radius)
            .ok_or("Could not decode the sample image.")
    }
}

/// The sample controls and the real Inspector/sidebar share the same text and
/// worksheet controls. Callers retain their own per-file presentation state.
pub(crate) fn flow_controls(
    ui: &mut egui::Ui,
    c: Colors,
    preview: &PreviewData,
    scale: &mut f32,
    overview: &mut bool,
) -> SizingControls {
    let mut controls = SizingControls {
        changed: false,
        targets: Vec::new(),
    };
    if preview.kind == PreviewKind::Text
        || preview.document.is_some()
        || preview.spreadsheet.is_some()
    {
        controls = text_size(ui, c, scale);
    }
    if preview.spreadsheet.is_some() {
        ui.checkbox(overview, "Overview")
            .on_hover_text("Fit the entire worksheet width; ordinary view retains readable text and horizontal scrolling.");
    }

    controls
}

fn theme_sample(kind: SampleKind) -> PreviewData {
    use ottrin_preview::{
        DocxAlignment, DocxBlock, DocxParagraph, DocxParagraphContent, DocxPreview, DocxRun,
        SpreadsheetCell, SpreadsheetPreview, SpreadsheetSheet,
    };
    let mut sample = PreviewData {
        title: "notes.txt".into(), kind: PreviewKind::Text,
        body: "Filetype: plain text file\nEncoding: UTF-8\nLines: 3\nPreview: Complete\n\nExtracted text:\nWelcome to Ottrin\nPreview files without changing them.\nYour theme applies to every window.".into(),
        document: None, spreadsheet: None, open_external: false,
    };
    match kind {
        SampleKind::Text => {}
        SampleKind::Image => {
            sample.title = "inspector-sample.png".into();
            sample.kind = PreviewKind::Image;
            sample.open_external = true;
            sample.body = format!(
                "Filetype: PNG image\nResolution: 256x256\nSize: {}",
                format_size(IMAGE_SAMPLE.len() as u64)
            );
        }
        SampleKind::Pdf => {
            sample.title = "inspector-sample.pdf".into();
            sample.kind = PreviewKind::Pdf;
            sample.open_external = true;
            sample.body = format!(
                "Filetype: PDF document\nPages: {PDF_SAMPLE_PAGES}\nTitle: Ottrin Inspector sample"
            );
        }
        SampleKind::Document => {
            sample.title = "document.docx".into();
            sample.kind = PreviewKind::OfficeMetadata;
            sample.body = "Filetype: DOCX document\nFormat: DOCX\nInternal parts: 5\n\nExtracted text:\nWelcome to Ottrin\nDocument content keeps its own page and ink. Your theme styles the surrounding controls.".into();
            sample.document = Some(DocxPreview { blocks: [
                ("Welcome to Ottrin", true),
                ("Document content keeps its own page and ink. Your theme styles the surrounding controls.", false),
            ].into_iter().map(|(text, bold)| DocxBlock::Paragraph(DocxParagraph {
                style_name: None, alignment: DocxAlignment::Left, list: None, has_page_break: false,
                content: vec![DocxParagraphContent::Text(DocxRun { text: text.into(), bold, italic: false, underline: false, color: None, font_size_half_points: None })],
            })).collect() });
        }
        SampleKind::Spreadsheet => {
            sample.title = "budget.xlsx".into();
            sample.kind = PreviewKind::OfficeMetadata;
            sample.body = "Filetype: XLSX document\nFormat: XLSX\nInternal parts: 5".into();
            sample.spreadsheet = Some(SpreadsheetPreview {
                omissions: vec![],
                sheets: [
                    (
                        "budget",
                        "Budget",
                        [
                            ["Item", "Planned", "Actual"],
                            ["Travel", "250", "220"],
                            ["Supplies", "80", "65"],
                        ],
                    ),
                    (
                        "summary",
                        "Summary",
                        [
                            ["Period", "Income", "Expenses"],
                            ["January", "1200", "285"],
                            ["February", "1200", "310"],
                        ],
                    ),
                ]
                .into_iter()
                .map(|(id, name, rows)| SpreadsheetSheet {
                    id: id.into(),
                    name: name.into(),
                    omissions: vec![],
                    cells: rows
                        .into_iter()
                        .enumerate()
                        .flat_map(|(row, values)| {
                            values.into_iter().enumerate().map(move |(column, text)| {
                                SpreadsheetCell {
                                    row,
                                    column,
                                    text: text.into(),
                                }
                            })
                        })
                        .collect(),
                })
                .collect(),
            });
        }
    }
    sample
}

pub(crate) fn theme_preview(
    ui: &mut egui::Ui,
    c: Colors,
    selected: ThemePreviewRegion,
    state: &mut ThemePreviewState,
) -> Option<ThemePreviewRegion> {
    ui.horizontal(|ui| {
        ui.label("Sample file");
        let previous = state.kind;
        let sample_type = egui::ComboBox::from_id_salt("theme_inspector_sample")
            .selected_text(state.kind.label())
            .show_ui(ui, |ui| {
                for kind in [
                    SampleKind::Text,
                    SampleKind::Document,
                    SampleKind::Spreadsheet,
                    SampleKind::Image,
                    SampleKind::Pdf,
                ] {
                    ui.selectable_value(&mut state.kind, kind, kind.label());
                }
            });
        widget_style::name_control(&sample_type.response, "Sample file");
        if previous != state.kind {
            state.sizing = PreviewBodySizing::default();
            state.selected_sheet = None;
            state.visual = VisualSampleSizing {
                fit_width: state.kind == SampleKind::Pdf,
                ..Default::default()
            };
            state.page_index = 0;
        }
    });
    ui.add_space(4.0);
    let sample = theme_sample(state.kind);
    let visual_sample = matches!(state.kind, SampleKind::Image | SampleKind::Pdf);
    let (outer, _) =
        ui.allocate_exact_size(ui.available_size().max(Vec2::splat(320.0)), Sense::hover());
    let frame = outer.shrink(14.0);
    let painter = ui.painter().clone();
    painter.rect_filled(outer, c.app_radius as f32 + 4.0, c.panel_raised);
    painter.rect_filled(frame, 0.0, c.panel);
    let inner = frame.shrink(c.window_border_width.ceil());
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("theme_inspector")
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(frame.intersect(ui.clip_rect()));
    let ctx = child.ctx().clone();
    let mut close = false;
    let title = render_custom_titlebar(
        &mut child,
        &ctx,
        c,
        c.titlebar_bg,
        &format!("Quick Inspector — {}", sample.title),
        false,
        &mut close,
    )
    .rect;
    let mut buttons_clicked = false;
    let mut inspector_clicked = false;
    let mut targets = Vec::new();
    Frame::new().inner_margin(12).show(&mut child, |ui| {
        let actions = action_row(
            ui,
            c,
            Availability {
                can_edit: sample.kind == PreviewKind::Text,
                image: sample.kind == PreviewKind::Image,
                ..Default::default()
            },
            |ui| {
                let menu = more_menu(ui, c, false, &sample);
                // The sample selects the styling of a menu row. It has no file-action dispatcher.
                buttons_clicked |= menu.inner.is_some();
                menu.response
            },
        );
        buttons_clicked |= actions.action.is_some() || actions.more.clicked();
        targets.extend(actions.targets);
        if let Some(page_count) = preview_page_count(&sample, None) {
            let navigation = render_page_navigation(ui, state.page_index, page_count, c);
            let button_clicked = navigation.buttons.iter().any(egui::Response::clicked);
            buttons_clicked |= button_clicked;
            targets.extend(navigation.buttons.map(|button| button.rect));
            if let Some(page) = navigation.page {
                state.page_index = page;
                inspector_clicked |= !button_clicked;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if visual_sample {
                for control in render_visual_sizing_controls(
                    ui,
                    &mut state.visual.fit,
                    &mut state.visual.fit_width,
                    &mut state.visual.zoom,
                    c,
                ) {
                    buttons_clicked |= control.clicked();
                    targets.push(control.rect);
                }
                return;
            }
            let overview_before = state.sizing.spreadsheet_overview;
            let controls = flow_controls(
                ui,
                c,
                &sample,
                &mut state.sizing.flow_text_scale,
                &mut state.sizing.spreadsheet_overview,
            );
            buttons_clicked |= controls.changed;
            inspector_clicked |= overview_before != state.sizing.spreadsheet_overview;
            targets.extend(controls.targets);
        });
        ui.separator();
        let scrolling = if !visual_sample {
            preview_render::PreviewScroll::Body
        } else if state.visual.fit && state.visual.fit_width {
            preview_render::PreviewScroll::WidthFittedPage
        } else {
            preview_render::PreviewScroll::Frame
        };
        preview_render::preview_content_frame(ui, ui.available_size(), scrolling, |ui| {
            if visual_sample {
                let spec = VisualPreviewRenderSpec {
                    max_size: inspector_visual_bounds(ui),
                    corner_radius: 8,
                    page_index: state.page_index,
                    fit: state.visual.fit,
                    fit_width: state.visual.fit_width,
                    zoom: state.visual.zoom,
                };
                match sample_visual_image(state.kind, spec) {
                    Ok(image) => {
                        ui.vertical_centered(|ui| {
                            let response =
                                submit_sized_preview(ui, image, spec.fit, &mut state.visual.zoom);
                            if let Some(count) = preview_page_count(&sample, None) {
                                render_following_pdf_pages(
                                    ui,
                                    Path::new(&sample.title),
                                    count,
                                    spec,
                                    response.rect.size(),
                                    c,
                                    |spec| sample_visual_image(state.kind, spec).ok(),
                                );
                            }
                            preview_render::render_preview_footer(
                                ui,
                                c,
                                &sample.title,
                                &sample.body,
                            );
                        });
                    }
                    Err(error) => {
                        render_metadata_card_preview(
                            ui,
                            c,
                            MI_PICTURE_PDF,
                            &sample.title,
                            &format!("{}\n{error}", sample.body),
                        );
                    }
                }
                return;
            }
            let sheet_before = state.selected_sheet.clone();
            let _ = preview_render::render_shared_preview_body(
                ui,
                c,
                Path::new(&sample.title),
                &sample.title,
                &sample,
                &mut state.selected_sheet,
                state.sizing,
            );
            inspector_clicked |= sheet_before.is_some() && sheet_before != state.selected_sheet;
        });
    });
    window_chrome::paint_embedded_viewport_frame(&painter, frame, c, c.panel_raised);
    let regions = [
        (ThemePreviewRegion::Titlebar, title),
        (ThemePreviewRegion::Inspector, frame),
        (ThemePreviewRegion::Borders, frame),
    ];
    let hovered = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|p| outer.contains(*p) && ui.rect_contains_pointer(outer))
        .and_then(|p| theme_editor_region_at_pos(&regions, p, c));
    let selected_rect = match selected {
        ThemePreviewRegion::Titlebar => Some(title),
        ThemePreviewRegion::Inspector
        | ThemePreviewRegion::Defaults
        | ThemePreviewRegion::Borders => Some(frame),
        _ => None,
    };
    if let Some(rect) = selected_rect {
        let corners = if selected == ThemePreviewRegion::Titlebar {
            egui::CornerRadius {
                nw: c.app_radius,
                ne: c.app_radius,
                sw: 0,
                se: 0,
            }
        } else {
            egui::CornerRadius::same(c.app_radius)
        };
        painter.rect_stroke(
            rect,
            corners,
            Stroke::new(2.0, c.accent),
            egui::StrokeKind::Inside,
        );
    }
    if selected == ThemePreviewRegion::Accent {
        for target in targets {
            painter.rect_stroke(
                target,
                c.button_radius,
                Stroke::new(2.0, c.accent),
                egui::StrokeKind::Inside,
            );
        }
    }
    if hovered == Some(ThemePreviewRegion::Borders) && ui.input(|i| i.pointer.primary_clicked()) {
        Some(ThemePreviewRegion::Borders)
    } else if close {
        Some(ThemePreviewRegion::Titlebar)
    } else if buttons_clicked {
        Some(ThemePreviewRegion::Accent)
    } else if inspector_clicked {
        Some(ThemePreviewRegion::Inspector)
    } else if ui.input(|i| i.pointer.primary_clicked()) {
        hovered
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action as AccessibleAction, ActionRequest, Role};

    fn input(size: Vec2, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        }
    }
    fn button(output: &egui::FullOutput, name: &str) -> (egui::accesskit::NodeId, Rect) {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
            .map(|(id, node)| {
                let r = node.bounds().unwrap();
                (
                    *id,
                    Rect::from_min_max(
                        egui::pos2(r.x0 as f32, r.y0 as f32),
                        egui::pos2(r.x1 as f32, r.y1 as f32),
                    ),
                )
            })
            .unwrap_or_else(|| panic!("Missing actual control {name}"))
    }
    fn activate(id: egui::accesskit::NodeId) -> Vec<egui::Event> {
        vec![egui::Event::AccessKitActionRequest(ActionRequest {
            action: AccessibleAction::Click,
            target: id,
            data: None,
        })]
    }

    #[test]
    fn inspector_theme_preview_matches_actual_inspector_controls_and_text() {
        for dark in [false, true] {
            for font_scale in [1.0, 1.35] {
                for radius in [0, 24] {
                    let mut app = OttrinApp::default();
                    app.colors = Colors::preset(ThemePreset::Ottrin, dark);
                    app.colors.font_scale = font_scale;
                    app.colors.button_text = Color32::from_rgb(216, 112, 44);
                    app.colors.app_radius = radius;
                    app.colors.window_border_width = 4.0;
                    app.colors.window_border = Color32::from_rgb(235, 113, 135);
                    app.preview.visible = true;
                    app.preview.path = Some(PathBuf::from("notes.txt"));
                    app.preview.data = Some(theme_sample(SampleKind::Text));
                    app.preview.flow_text_scale = 1.0;
                    let actual_ctx = Context::default();
                    install_application_fonts(&actual_ctx);
                    actual_ctx.enable_accesskit();
                    let actual = actual_ctx.run(input(Vec2::new(720.0, 560.0), vec![]), |ctx| {
                        app.render_preview_overlay(ctx)
                    });
                    let preview_ctx = Context::default();
                    install_application_fonts(&preview_ctx);
                    preview_ctx.enable_accesskit();
                    app.apply_theme_to_ctx(&preview_ctx);
                    let mut sample_state = ThemePreviewState::default();
                    let preview = preview_ctx.run(input(Vec2::new(748.0, 588.0), vec![]), |ctx| {
                        egui::CentralPanel::default()
                            .frame(Frame::NONE)
                            .show(ctx, |ui| {
                                theme_preview(
                                    ui,
                                    app.colors,
                                    ThemePreviewRegion::Inspector,
                                    &mut sample_state,
                                );
                            });
                    });
                    for out in [&actual, &preview] {
                        let more_ink = out.shapes.iter().find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == MI_MORE_VERT => {
                                Some(text.galley.job.sections[0].format.color)
                            }
                            _ => None,
                        });
                        assert_eq!(
                            more_ink,
                            Some(app.colors.button_text),
                            "More actions must honor the button foreground role"
                        );
                    }
                    for name in [
                        "Close window",
                        "More actions",
                        "Edit text",
                        "− text",
                        "+ text",
                    ] {
                        let frame_origin = |output: &egui::FullOutput| {
                            output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    egui::Shape::Rect(rect)
                                        if rect.stroke
                                            == Stroke::new(4.0, app.colors.window_border) =>
                                    {
                                        Some(rect.rect.min)
                                    }
                                    _ => None,
                                })
                                .expect("painted window frame")
                        };
                        let expected = button(&actual, name)
                            .1
                            .translate(frame_origin(&preview) - frame_origin(&actual));
                        let shown = button(&preview, name).1;
                        assert!(
                            (shown.min - expected.min).length() < 0.1
                                && (shown.max - expected.max).length() < 0.1,
                            "{name}: shown{shown:?} actual{expected:?} at {font_scale}"
                        );
                    }
                    let text_style = |out: &egui::FullOutput| {
                        out.shapes
                            .iter()
                            .find_map(|s| match &s.shape {
                                egui::Shape::Text(t)
                                    if t.galley.job.text.contains("Welcome to Ottrin") =>
                                {
                                    Some(t.galley.job.clone())
                                }
                                _ => None,
                            })
                            .expect("Actual read-only text body")
                    };
                    assert_eq!(
                        text_style(&actual),
                        text_style(&preview),
                        "Identical read-only text renderer and sizing"
                    );
                    for out in [&actual, &preview] {
                        assert!(out.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Rect(r) if r.stroke==Stroke::new(4.0,app.colors.window_border) && r.corner_radius==egui::CornerRadius::same(radius))),"Shared border geometry");
                    }
                    assert!(app.preview.text_editor.is_none());
                }
            }
        }
    }

    #[test]
    fn inspector_document_samples_use_actual_body_and_control_presenters() {
        for dark in [false, true] {
            for font_scale in [1.0, 1.35] {
                for kind in [SampleKind::Document, SampleKind::Spreadsheet] {
                    let colors = Colors {
                        font_scale,
                        ..Colors::preset(ThemePreset::Ottrin, dark)
                    };
                    let sample = theme_sample(kind);
                    let mut app = OttrinApp::default();
                    app.colors = colors;
                    app.state.config.installed_office_previews_enabled = false;
                    app.preview.visible = true;
                    app.preview.path = Some(PathBuf::from(&sample.title));
                    app.preview.data = Some(sample);
                    let actual_ctx = Context::default();
                    install_application_fonts(&actual_ctx);
                    actual_ctx.enable_accesskit();
                    let actual = actual_ctx.run(input(Vec2::new(720.0, 560.0), vec![]), |ctx| {
                        app.render_preview_overlay(ctx)
                    });
                    let ctx = Context::default();
                    install_application_fonts(&ctx);
                    ctx.enable_accesskit();
                    theme_colors::apply_to_context(&ctx, &colors);
                    let mut state = ThemePreviewState {
                        kind,
                        ..Default::default()
                    };
                    let preview = ctx.run(input(Vec2::new(748.0, 660.0), vec![]), |ctx| {
                        egui::CentralPanel::default()
                            .frame(Frame::NONE)
                            .show(ctx, |ui| {
                                theme_preview(
                                    ui,
                                    colors,
                                    ThemePreviewRegion::Inspector,
                                    &mut state,
                                );
                            });
                    });
                    for name in ["More actions", "− text", "+ text"] {
                        let actual = button(&actual, name).1;
                        let shown = button(&preview, name).1;
                        assert!(
                            (actual.size() - shown.size()).length() < 0.1,
                            "{kind:?} {name} at {font_scale}"
                        );
                    }
                    for output in [&actual, &preview] {
                        let texts = output
                            .shapes
                            .iter()
                            .filter_map(|shape| match &shape.shape {
                                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        let expected = match kind {
                            SampleKind::Document => "Welcome to Ottrin",
                            _ => "Planned",
                        };
                        assert!(
                            texts.iter().any(|text| text.contains(expected)),
                            "Actual {kind:?} body missing {expected}: {texts:?}"
                        );
                        if kind == SampleKind::Document {
                            assert!(
                                texts
                                    .iter()
                                    .any(|text| text.contains("Extracted text fallback")),
                                "Document preview exposes the same fallback disclosure as real files"
                            );
                        }
                        assert!(
                            !texts.contains(&MI_EDIT),
                            "Document samples must not imply text-file editing"
                        );
                        let checkboxes = output
                            .platform_output
                            .accesskit_update
                            .as_ref()
                            .unwrap()
                            .nodes
                            .iter()
                            .filter(|(_, node)| {
                                node.role() == Role::CheckBox && node.label() == Some("Overview")
                            })
                            .count();
                        assert_eq!(checkboxes, usize::from(kind == SampleKind::Spreadsheet));
                    }
                }
            }
        }
    }

    #[test]
    fn inspector_preview_selects_real_controls_without_editing_or_closing() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        theme_colors::apply_to_context(&ctx, &colors);
        let mut sample_state = ThemePreviewState::default();
        let mut render = |events| {
            let mut picked = None;
            let out = ctx.run(input(Vec2::new(748.0, 588.0), events), |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        picked = theme_preview(
                            ui,
                            colors,
                            ThemePreviewRegion::Inspector,
                            &mut sample_state,
                        );
                    });
            });
            (out, picked)
        };
        for (name, expected) in [
            ("Edit text", ThemePreviewRegion::Accent),
            ("Close window", ThemePreviewRegion::Titlebar),
            ("+ text", ThemePreviewRegion::Accent),
        ] {
            let target = button(&render(vec![]).0, name).0;
            let (_, picked) = render(activate(target));
            assert_eq!(picked, Some(expected));
            assert!(
                !egui::Popup::is_any_open(&ctx),
                "Preview chooses a theme role, never file actions"
            );
        }
        assert!(
            sample_state.sizing.flow_text_scale > 1.0,
            "Sizing the sample uses the real text controls"
        );
    }

    #[test]
    fn inspector_sample_opens_the_live_more_menu_without_dispatching_actions() {
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        let ctx = Context::default();
        install_application_fonts(&ctx);
        ctx.enable_accesskit();
        theme_colors::apply_to_context(&ctx, &colors);
        let mut sample_state = ThemePreviewState::default();
        let mut render_sample = |events| {
            let mut picked = None;
            let output = ctx.run(input(Vec2::new(748.0, 588.0), events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    picked =
                        theme_preview(ui, colors, ThemePreviewRegion::Inspector, &mut sample_state);
                });
            });
            (output, picked)
        };
        let mut app = OttrinApp::default();
        app.colors = colors;
        app.preview.visible = true;
        app.preview.path = Some(PathBuf::from("notes.txt"));
        app.preview.data = Some(theme_sample(SampleKind::Text));
        let live_ctx = Context::default();
        install_application_fonts(&live_ctx);
        live_ctx.enable_accesskit();
        app.apply_theme_to_ctx(&live_ctx);
        let mut render_live = |events| {
            live_ctx.run(input(Vec2::new(720.0, 560.0), events), |ctx| {
                app.render_preview_overlay(ctx);
            })
        };

        let more = button(&render_sample(vec![]).0, "More actions").0;
        let (sample, picked) = render_sample(activate(more));
        assert_eq!(picked, Some(ThemePreviewRegion::Accent));
        assert!(egui::Popup::is_any_open(&ctx));
        let more = button(&render_live(vec![]), "More actions").0;
        let live = render_live(activate(more));
        for name in [
            "Copy Path",
            "Copy Text",
            "Copy",
            "Cut",
            "Rename",
            "Reveal",
            "Move to Trash",
        ] {
            button(&sample, name);
            button(&live, name);
        }
        let (sample, picked) = render_sample(activate(button(&sample, "Copy Path").0));
        assert_eq!(picked, Some(ThemePreviewRegion::Accent));
        assert!(
            !sample
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
        let live = render_live(activate(button(&live, "Copy Path").0));
        assert!(live.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(path) if path == "notes.txt")
        ));
    }

    #[test]
    fn spreadsheet_sample_switches_sheet_and_overview_without_button_geometry() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        theme_colors::apply_to_context(&ctx, &colors);
        let mut state = ThemePreviewState {
            kind: SampleKind::Spreadsheet,
            ..Default::default()
        };
        let picked = std::cell::Cell::new(None);
        let render = |state: &mut ThemePreviewState, events| {
            ctx.run(input(Vec2::new(748.0, 660.0), events), |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        picked.set(theme_preview(
                            ui,
                            colors,
                            ThemePreviewRegion::Inspector,
                            state,
                        ));
                    });
            })
        };
        let output = render(&mut state, vec![]);
        let control = |output: &egui::FullOutput, name: &str| {
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(name))
                .map(|(id, _)| *id)
                .unwrap_or_else(|| panic!("Missing {name}"))
        };
        render(&mut state, activate(control(&output, "Summary")));
        assert_eq!(state.selected_sheet.as_deref(), Some("summary"));
        assert_eq!(picked.get(), Some(ThemePreviewRegion::Inspector));
        let output = render(&mut state, vec![]);
        render(&mut state, activate(control(&output, "Overview")));
        assert!(state.sizing.spreadsheet_overview);
        assert_eq!(
            picked.get(),
            Some(ThemePreviewRegion::Inspector),
            "Worksheet controls select their shared surface styling"
        );
        let output = render(&mut state, vec![]);
        assert!(output.shapes.iter().any(
            |shape| matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Income")
        ));
        let fields = OttrinApp::theme_preview_region_shared_fields(ThemePreviewRegion::Inspector);
        for field in [
            ThemeColorField::Selection,
            ThemeColorField::Accent,
            ThemeColorField::Hover,
        ] {
            assert!(fields.contains(&field));
        }
    }

    #[test]
    fn image_sample_uses_live_inspector_controls_and_sizes_the_visible_raster() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        egui_extras::install_image_loaders(&ctx);
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        theme_colors::apply_to_context(&ctx, &colors);
        let mut state = ThemePreviewState {
            kind: SampleKind::Image,
            ..Default::default()
        };
        let picked = std::cell::Cell::new(None);
        let render = |state: &mut ThemePreviewState, events| {
            ctx.run(input(Vec2::new(748.0, 660.0), events), |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        picked.set(theme_preview(
                            ui,
                            colors,
                            ThemePreviewRegion::Inspector,
                            state,
                        ));
                    });
            })
        };

        let mut app = OttrinApp::default();
        app.colors = colors;
        app.preview.visible = true;
        app.preview.path = Some(PathBuf::from("inspector-sample.png"));
        app.preview.data = Some(theme_sample(SampleKind::Image));
        app.preview.visual_fit = true;
        app.preview.visual_zoom = 1.0;
        let live_ctx = Context::default();
        install_application_fonts(&live_ctx);
        live_ctx.enable_accesskit();
        app.apply_theme_to_ctx(&live_ctx);
        let live = live_ctx.run(input(Vec2::new(720.0, 560.0), vec![]), |ctx| {
            app.render_preview_overlay(ctx)
        });
        let output = render(&mut state, vec![]);
        for name in [
            "Rotate Right",
            "Rotate Left",
            "Fit page",
            "Fit width",
            "100%",
            "Zoom out",
            "Zoom in",
        ] {
            button(&live, name);
            button(&output, name);
        }
        render(&mut state, activate(button(&output, "Fit width").0));
        assert!(state.visual.fit && state.visual.fit_width);
        assert_eq!(picked.get(), Some(ThemePreviewRegion::Accent));

        let output = render(&mut state, vec![]);
        render(&mut state, activate(button(&output, "100%").0));
        assert!(!state.visual.fit);
        assert_eq!(state.visual.zoom, 1.0);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let output = render(&mut state, vec![]);
            if let Some((shape, rect)) = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.brush.is_some() => Some((shape, rect)),
                _ => None,
            }) {
                assert_eq!(rect.rect.size(), Vec2::splat(256.0));
                assert!(shape.clip_rect.contains_rect(rect.rect));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "The sample must paint the bundled image"
            );
            std::thread::yield_now();
        }

        let output = render(&mut state, vec![]);
        render(&mut state, activate(button(&output, "Zoom in").0));
        assert_eq!(state.visual.zoom, 1.2);
        let output = render(&mut state, vec![]);
        render(&mut state, activate(button(&output, "Rotate Right").0));
        assert_eq!(picked.get(), Some(ThemePreviewRegion::Accent));
        assert_eq!(state.visual.zoom, 1.2);
    }

    #[test]
    fn pdf_sample_navigates_pages_with_the_inspector_controls() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        egui_extras::install_image_loaders(&ctx);
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        theme_colors::apply_to_context(&ctx, &colors);
        let mut state = ThemePreviewState {
            kind: SampleKind::Pdf,
            visual: VisualSampleSizing {
                fit_width: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let picked = std::cell::Cell::new(None);
        let render = |state: &mut ThemePreviewState, events| {
            ctx.run(input(Vec2::new(748.0, 660.0), events), |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        picked.set(theme_preview(
                            ui,
                            colors,
                            ThemePreviewRegion::Inspector,
                            state,
                        ));
                    });
            })
        };
        let output = render(&mut state, vec![]);
        assert!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::SpinButton
                    && node.label() == Some("Page number"))
        );
        render(&mut state, activate(button(&output, "Next").0));
        assert_eq!(state.page_index, 1);
        assert_eq!(picked.get(), Some(ThemePreviewRegion::Accent));
        let output = render(&mut state, vec![]);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == "Page 2 of 2")));
        render(&mut state, activate(button(&output, "Previous").0));
        assert_eq!(state.page_index, 0);

        if ottrin_preview::pdfium_runtime_is_available() {
            let spec = VisualPreviewRenderSpec {
                max_size: Vec2::splat(600.0),
                corner_radius: 8,
                page_index: 0,
                fit: true,
                fit_width: false,
                zoom: 1.0,
            };
            let first = sample_visual_image(SampleKind::Pdf, spec).expect("Render first PDF page");
            let second = sample_visual_image(
                SampleKind::Pdf,
                VisualPreviewRenderSpec {
                    page_index: 1,
                    ..spec
                },
            )
            .expect("Render second PDF page");
            assert_ne!(first.source(&ctx).uri(), second.source(&ctx).uri());
        }
    }

    #[test]
    fn sample_file_selector_is_named_and_resets_only_sample_state() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        let mut state = ThemePreviewState {
            kind: SampleKind::Spreadsheet,
            sizing: PreviewBodySizing {
                flow_text_scale: 1.75,
                spreadsheet_overview: true,
            },
            selected_sheet: Some("summary".into()),
            ..Default::default()
        };
        let render = |state: &mut ThemePreviewState, events| {
            ctx.run(input(Vec2::new(748.0, 660.0), events), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    theme_preview(ui, colors, ThemePreviewRegion::Inspector, state);
                });
            })
        };
        let output = render(&mut state, vec![]);
        let selector = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::ComboBox && node.label() == Some("Sample file"))
            .map(|(id, _)| *id)
            .expect("Sample selector has an accessible name");
        let output = render(&mut state, activate(selector));
        let document = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Document"))
            .map(|(id, _)| *id)
            .expect("Actual sample menu exposes Document");
        render(&mut state, activate(document));
        assert_eq!(state.kind, SampleKind::Document);
        assert_eq!(state.sizing.flow_text_scale, 1.0);
        assert!(!state.sizing.spreadsheet_overview);
        assert!(state.selected_sheet.is_none());
    }

    #[test]
    fn inspector_action_presenter_preserves_edit_save_rotate_availability() {
        for (name, available, expected) in [
            (
                "Edit text",
                Availability {
                    can_edit: true,
                    ..Default::default()
                },
                Some(Action::Edit),
            ),
            (
                "Cancel edits",
                Availability {
                    editing: true,
                    ..Default::default()
                },
                Some(Action::Cancel),
            ),
            (
                "Save",
                Availability {
                    editing: true,
                    can_save: true,
                    ..Default::default()
                },
                Some(Action::Save),
            ),
            (
                "Save",
                Availability {
                    editing: true,
                    can_save: false,
                    ..Default::default()
                },
                None,
            ),
            (
                "Rotate Right",
                Availability {
                    image: true,
                    ..Default::default()
                },
                Some(Action::RotateRight),
            ),
            (
                "Rotate Left",
                Availability {
                    image: true,
                    ..Default::default()
                },
                Some(Action::RotateLeft),
            ),
        ] {
            let ctx = Context::default();
            install_application_fonts(&ctx);
            ctx.enable_accesskit();
            let c = Colors::preset(ThemePreset::Ottrin, true);
            let render = |events| {
                let mut action = None;
                let out = ctx.run(input(Vec2::new(720.0, 200.0), events), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let preview = theme_sample(SampleKind::Text);
                        action = action_row(ui, c, available, |ui| {
                            more_menu(ui, c, false, &preview).response
                        })
                        .action;
                    });
                });
                (out, action)
            };
            let id = button(&render(vec![]).0, name).0;
            assert_eq!(render(activate(id)).1, expected, "{name}");
        }
    }
}
