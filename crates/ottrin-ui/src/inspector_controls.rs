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
pub(crate) fn more_menu(
    ui: &mut egui::Ui,
    c: Colors,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    name_more(widget_style::themed_menu_from_button(ui, more_button(c), c, contents).response)
}
pub(crate) fn more_target(ui: &mut egui::Ui, c: Colors) -> egui::Response {
    name_more(ui.add(more_button(c).themed(&c)))
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

pub(crate) struct TextSize {
    pub changed: bool,
    pub targets: [Rect; 2],
}

pub(crate) fn text_size(ui: &mut egui::Ui, c: Colors, scale: &mut f32) -> TextSize {
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
    TextSize {
        changed,
        targets: [smaller.rect, larger.rect],
    }
}

fn theme_sample() -> PreviewData {
    PreviewData {
        title: "notes.txt".into(), kind: PreviewKind::Text,
        body: "Filetype: plain text file\nEncoding: UTF-8\nLines: 3\nPreview: Complete\n\nExtracted text:\nWelcome to Ottrin\nPreview files without changing them.\nYour theme applies to every window.".into(),
        document: None, spreadsheet: None, open_external: false,
    }
}

pub(crate) fn theme_preview(
    ui: &mut egui::Ui,
    c: Colors,
    selected: ThemePreviewRegion,
    text_scale: &mut f32,
) -> Option<ThemePreviewRegion> {
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
        "Quick Inspector — notes.txt",
        false,
        &mut close,
    )
    .rect;
    let mut buttons_clicked = false;
    let mut targets = Vec::new();
    Frame::new().inner_margin(12).show(&mut child, |ui| {
        let actions = action_row(
            ui,
            c,
            Availability {
                can_edit: true,
                ..Default::default()
            },
            |ui| more_target(ui, c),
        );
        buttons_clicked |= actions.action.is_some() || actions.more.clicked();
        targets.extend(actions.targets);
        ui.horizontal_wrapped(|ui| {
            let sizing = text_size(ui, c, text_scale);
            buttons_clicked |= sizing.changed;
            targets.extend(sizing.targets);
        });
        ui.separator();
        preview_render::preview_content_frame(
            ui,
            ui.available_size(),
            preview_render::PreviewScroll::Body,
            |ui| {
                let sample = theme_sample();
                preview_render::render_text_body_preview_scaled(
                    ui,
                    c,
                    &sample.title,
                    &sample.body,
                    *text_scale,
                );
            },
        );
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
                    app.preview.data = Some(theme_sample());
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
                    let mut scale = 1.0;
                    let preview = preview_ctx.run(input(Vec2::new(748.0, 588.0), vec![]), |ctx| {
                        egui::CentralPanel::default()
                            .frame(Frame::NONE)
                            .show(ctx, |ui| {
                                theme_preview(
                                    ui,
                                    app.colors,
                                    ThemePreviewRegion::Inspector,
                                    &mut scale,
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
                        let expected = button(&actual, name).1.translate(Vec2::splat(14.0));
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
    fn inspector_preview_selects_real_controls_without_editing_or_closing() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        ctx.enable_accesskit();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        theme_colors::apply_to_context(&ctx, &colors);
        let mut scale = 1.0;
        let mut render = |events| {
            let mut picked = None;
            let out = ctx.run(input(Vec2::new(748.0, 588.0), events), |ctx| {
                egui::CentralPanel::default()
                    .frame(Frame::NONE)
                    .show(ctx, |ui| {
                        picked =
                            theme_preview(ui, colors, ThemePreviewRegion::Inspector, &mut scale);
                    });
            });
            (out, picked)
        };
        for (name, expected) in [
            ("More actions", ThemePreviewRegion::Accent),
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
        assert!(scale > 1.0, "Sizing the sample uses the real text controls");
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
                        action = action_row(ui, c, available, |ui| more_target(ui, c)).action;
                    });
                });
                (out, action)
            };
            let id = button(&render(vec![]).0, name).0;
            assert_eq!(render(activate(id)).1, expected, "{name}");
        }
    }
}
