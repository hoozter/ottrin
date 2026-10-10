use crate::Colors;
use egui::{Button, CornerRadius, Response, Stroke, Ui, Widget};

/// Give buttons their own theme role without changing text inputs or checkboxes.
pub(crate) trait ThemeButtonExt<'a> {
    fn themed(self, colors: &Colors) -> ThemeButton<'a>;
}

pub(crate) struct ThemeButton<'a> {
    button: Button<'a>,
    colors: Colors,
}

impl<'a> ThemeButtonExt<'a> for Button<'a> {
    fn themed(self, colors: &Colors) -> ThemeButton<'a> {
        ThemeButton {
            button: self,
            colors: *colors,
        }
    }
}

fn apply_button_style(ui: &mut Ui, colors: Colors) {
    let visuals = ui.visuals_mut();
    visuals.override_text_color = None;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(colors.button_radius);
        widget.fg_stroke = Stroke::new(1.0, colors.button_text);
    }
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = colors.button_bg;
        widget.weak_bg_fill = colors.button_bg;
    }
    // Feedback overlays belong on the button's own surface, not the panel
    // behind it. An unframed button never paints these fills.
    visuals.widgets.hovered.bg_fill = colors.button_bg.blend(colors.hover);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.active.bg_fill = colors.button_bg.blend(colors.accent_dim);
    visuals.widgets.active.weak_bg_fill = visuals.widgets.active.bg_fill;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, colors.accent);
}

impl Widget for ThemeButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.scope(|ui| {
            apply_button_style(ui, self.colors);
            self.button.ui(ui)
        })
        .inner
    }
}

/// Text keeps its own selection ink; the field frame uses the focus accent.
/// Egui's frame is disabled so this is the sole field-frame renderer.
pub(crate) trait ThemeTextEditExt<'a> {
    fn themed(self, colors: &Colors) -> ThemeTextEdit<'a>;
}

pub(crate) struct ThemeTextEdit<'a> {
    edit: egui::TextEdit<'a>,
    colors: Colors,
}

impl<'a> ThemeTextEditExt<'a> for egui::TextEdit<'a> {
    fn themed(self, colors: &Colors) -> ThemeTextEdit<'a> {
        ThemeTextEdit {
            edit: self,
            colors: *colors,
        }
    }
}

impl ThemeTextEdit<'_> {
    pub(crate) fn show(self, ui: &mut Ui) -> egui::text_edit::TextEditOutput {
        let background = ui.painter().add(egui::epaint::Shape::Noop);
        let output = self.edit.frame(false).show(ui);
        let stroke = if output.response.has_focus() && ui.is_enabled() {
            Stroke::new(1.0, self.colors.accent)
        } else {
            Stroke::NONE
        };
        let expansion = ui.style().interact(&output.response).expansion;
        ui.painter().set(
            background,
            egui::epaint::RectShape::new(
                output.response.rect.expand(expansion),
                CornerRadius::same(self.colors.border_radius),
                ui.visuals().text_edit_bg_color(),
                stroke,
                egui::StrokeKind::Inside,
            ),
        );
        output
    }
}

impl Widget for ThemeTextEdit<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Style the menu trigger as a button, then restore ordinary roles inside it.
pub(crate) fn themed_menu_button<'a, R>(
    ui: &mut Ui,
    label: impl egui::IntoAtoms<'a>,
    colors: Colors,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let content_style = ui.style().clone();
    ui.scope(|ui| {
        apply_button_style(ui, colors);
        ui.menu_button(label, |ui| {
            ui.set_style(content_style);
            contents(ui)
        })
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OttrinApp, ThemePreset};
    use egui::{Color32, Context, Rect, Vec2, epaint::Shape};

    #[test]
    fn button_theme_changes_do_not_recolor_or_round_inputs_and_body_labels() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        let mut custom = OttrinApp::custom_from_colors(Colors::preset(ThemePreset::Ottrin, true));
        custom.button_bg = [180, 24, 100, 255];
        custom.button_text = [100, 245, 160, 255];
        custom.button_radius = 14.0;
        custom.border_radius = 3.0;
        app.state.config.active_custom_theme = Some("Fixture".into());
        app.state
            .config
            .custom_themes
            .push(ottrin_core::SavedTheme {
                name: "Fixture".into(),
                theme_mode: ottrin_core::ThemeMode::Dark,
                base_preset: ThemePreset::Ottrin,
                customization: custom,
            });
        app.reapply_theme(&ctx);
        let c = app.colors;
        let mut input_rect = Rect::NOTHING;
        let mut button_rect = Rect::NOTHING;
        let mut text = "Typed input".to_string();
        let mut checked = true;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(500.0, 320.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Body label");
                    ui.checkbox(&mut checked, "Checkbox label");
                    input_rect = ui.text_edit_singleline(&mut text).rect;
                    button_rect = ui.add(Button::new("Button label").themed(&c)).rect;
                });
            },
        );
        let mut saw_button = false;
        let mut saw_input = false;
        let mut labels = std::collections::BTreeMap::new();
        for shape in output.shapes {
            match shape.shape {
                Shape::Rect(rect)
                    if rect.rect.contains(button_rect.center()) && rect.fill == c.button_bg =>
                {
                    assert_eq!(rect.corner_radius.nw, 14);
                    saw_button = true;
                }
                Shape::Rect(rect)
                    if rect.rect.contains(input_rect.center()) && rect.fill == c.panel_raised =>
                {
                    assert_eq!(rect.corner_radius.nw, 3);
                    saw_input = true;
                }
                Shape::Text(text) => {
                    labels.insert(text.galley.job.text.clone(), text.fallback_color);
                }
                _ => {}
            }
        }
        assert!(saw_button, "the changed button background must be painted");
        assert!(
            saw_input,
            "the input must retain its raised surface and surface corners"
        );
        assert_eq!(
            labels.get("Button label"),
            Some(&Color32::from_rgb(100, 245, 160))
        );
        assert_eq!(labels.get("Body label"), Some(&c.text));
        assert_eq!(labels.get("Checkbox label"), Some(&c.text));
        assert_eq!(labels.get("Typed input"), Some(&c.text));
    }
    #[test]
    fn rendered_button_feedback_keeps_its_surface_and_menu_labels_use_button_text() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.colors.button_bg = Color32::from_rgb(180, 24, 100);
        app.colors.button_text = Color32::from_rgb(100, 245, 160);
        app.colors.hover = Color32::from_rgba_unmultiplied(0, 0, 255, 128);
        app.colors.accent_dim = Color32::from_rgba_unmultiplied(0, 255, 255, 64);
        app.apply_theme_to_ctx(&ctx);
        ctx.style_mut(|style| style.animation_time = 0.0);
        let c = app.colors;
        let mut button = Rect::NOTHING;
        let mut menu = Rect::NOTHING;
        for state in 0..5 {
            let events = match state {
                0 => vec![],
                1 => vec![egui::Event::PointerMoved(button.center())],
                2 | 4 => vec![egui::Event::PointerButton {
                    pos: if state == 2 {
                        button.center()
                    } else {
                        menu.center()
                    },
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                _ => vec![
                    egui::Event::PointerButton {
                        pos: button.center(),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerMoved(menu.center()),
                ],
            };
            let mut output = None;
            for frame in 0..2 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(500.0, 320.0),
                        )),
                        events: if frame == 0 { events.clone() } else { vec![] },
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            button = ui.add(Button::new("Action").themed(&c)).rect;
                            menu = crate::menu_item(ui, "", "Menu action", &c).rect;
                        });
                    },
                ));
            }
            let output = output.expect("rendered button frame");
            let expected = match state {
                0 => c.button_bg,
                1 | 3 => c.button_bg.blend(c.hover),
                _ => c.button_bg.blend(c.accent_dim),
            };
            let mut painted = false;
            let mut menu_label = false;
            for shape in output.shapes {
                match shape.shape {
                    Shape::Rect(rect)
                        if rect.rect.contains(if state < 3 {
                            button.center()
                        } else {
                            menu.center()
                        }) && rect.fill == expected =>
                    {
                        painted = true
                    }
                    Shape::Text(text) if text.galley.job.text.trim() == "Menu action" => {
                        assert_eq!(text.fallback_color, c.button_text);
                        menu_label = true;
                    }
                    _ => {}
                }
            }
            assert!(
                painted,
                "state {state}: button feedback discarded its chosen surface"
            );
            assert!(menu_label, "menu action was not rendered");
        }
    }
    #[test]
    fn input_focus_uses_accent_without_recoloring_selected_text() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.colors.text = Color32::from_rgb(231, 236, 240);
        app.colors.accent = Color32::from_rgb(100, 245, 160);
        app.colors.button_radius = 14;
        app.colors.border_radius = 3;
        app.apply_theme_to_ctx(&ctx);
        let c = app.colors;
        let mut value = "Editable input".to_string();
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(500.0, 320.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut value).themed(&c))
                            .request_focus();
                    });
                },
            ));
        }
        let output = output.expect("rendered input");
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            Shape::Rect(rect) if rect.fill == c.panel_raised && rect.corner_radius.nw == 3 && rect.stroke == Stroke::new(1.0, c.accent)
        )), "focus frame must use the accent and input surface/corners");
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape,
                Shape::Text(text) if text.galley.job.text == value && text.fallback_color == c.text
            )),
            "focused input must retain primary text ink"
        );
        assert_eq!(
            ctx.style().visuals.selection.stroke.color,
            c.text,
            "selected text ink must not become the focus accent"
        );
    }
}
