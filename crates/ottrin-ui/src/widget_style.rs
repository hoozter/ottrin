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
        // Keep allocation in the caller's layout. A nested scope paints into
        // the remainder of a wrapping row before the parent can move it.
        let previous_style = ui.style().clone();
        apply_button_style(ui, self.colors);
        let response = self.button.ui(ui);
        ui.set_style(previous_style);
        response
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

/// Menu actions and repeated choice rows share separate indicator/icon slots.
/// Native Button still owns hit testing, focus, enabled state and label semantics.
pub(crate) struct AlignedMenuButton {
    indicator: String,
    indicator_column: bool,
    icon: String,
    label: String,
    colors: Colors,
    framed: bool,
    min_size: egui::Vec2,
    submenu: bool,
    width: Option<f32>,
}

pub(crate) fn aligned_menu_button(
    indicator: &str,
    icon: &str,
    label: &str,
    colors: &Colors,
) -> AlignedMenuButton {
    AlignedMenuButton {
        indicator: indicator.trim().to_owned(),
        indicator_column: !indicator.is_empty(),
        icon: icon.to_owned(),
        label: label.to_owned(),
        colors: *colors,
        framed: false,
        min_size: egui::Vec2::new(188.0 * colors.font_scale, 28.0 * colors.font_scale),
        submenu: false,
        width: None,
    }
}

impl AlignedMenuButton {
    /// Choice groups reserve this column on every row; action-only menus omit it.
    pub(crate) fn indicator_column(mut self) -> Self {
        self.indicator_column = true;
        self
    }
    pub(crate) fn frame_when_inactive(mut self, framed: bool) -> Self {
        self.framed = framed;
        self
    }
    pub(crate) fn min_size(mut self, size: egui::Vec2) -> Self {
        self.min_size = size;
        self
    }
    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self.min_size.x = width;
        self
    }
    pub(crate) fn themed(mut self, colors: &Colors) -> Self {
        self.colors = *colors;
        self
    }
}

impl Widget for AlignedMenuButton {
    fn ui(self, ui: &mut Ui) -> Response {
        use egui::{Align2, Atom, AtomExt, FontId, RichText, Vec2};
        ui.scope(|ui| {
            apply_button_style(ui, self.colors);
            if let Some(width) = self.width {
                ui.set_width(width);
            }
            let scale = self.colors.font_scale;
            ui.spacing_mut().icon_spacing = 4.0 * scale;
            ui.spacing_mut().button_padding = egui::Vec2::new(6.0 * scale, 4.0 * scale);
            let indicator_id = ui.next_auto_id().with("indicator");
            let icon_id = ui.next_auto_id().with("icon");
            let mut atoms = Vec::new();
            if self.indicator_column {
                atoms.push(Atom::custom(
                    indicator_id,
                    Vec2::new(14.0 * scale, 20.0 * scale),
                ));
            }
            atoms.push(Atom::custom(icon_id, Vec2::new(18.0 * scale, 20.0 * scale)));
            atoms.push(
                RichText::new(self.label)
                    .size(12.5 * scale)
                    .atom_shrink(true),
            );
            atoms.push(Atom::grow());
            let mut button = Button::new(egui::Atoms::from(atoms))
                .frame(true)
                .frame_when_inactive(self.framed)
                .min_size(self.min_size);
            if self.width.is_some() {
                button = button.wrap_mode(egui::TextWrapMode::Truncate);
            }
            if self.submenu {
                button = button.right_text(egui::containers::menu::SubMenuButton::RIGHT_ARROW);
            }
            let output = button.atom_ui(ui);
            let color = ui.style().interact(&output.response).text_color();
            for (id, glyph) in [(indicator_id, self.indicator), (icon_id, self.icon)] {
                if !glyph.is_empty()
                    && let Some(rect) = output.rect(id)
                {
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        glyph,
                        FontId::proportional(14.0 * scale),
                        color,
                    );
                }
            }
            output.response
        })
        .inner
    }
}

pub(crate) fn aligned_submenu<R>(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    colors: Colors,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let mut button = aligned_menu_button("", icon, label, &colors);
    button.submenu = true;
    let response = ui.add(button);
    let inner = egui::containers::menu::SubMenu::default().show(ui, &response, contents);
    egui::InnerResponse {
        response,
        inner: inner.map(|inner| inner.inner),
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
    fn page_navigation_keeps_reading_order_and_wraps_without_clipping() {
        for dark in [true, false] {
            for scale in [1.0, 1.35] {
                for width in [180.0, 320.0, 600.0] {
                    let ctx = Context::default();
                    let mut app = OttrinApp::default();
                    app.colors = Colors::preset(ThemePreset::Ottrin, dark);
                    app.colors.font_scale = scale;
                    app.apply_theme_to_ctx(&ctx);
                    let mut bounds = Rect::NOTHING;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(800.0, 500.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.scope_builder(
                                    egui::UiBuilder::new()
                                        .max_rect(Rect::from_min_size(
                                            ui.cursor().min,
                                            Vec2::new(width, 350.0),
                                        ))
                                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                                    |ui| {
                                        bounds = ui.max_rect();
                                        assert_eq!(
                                            crate::render_page_navigation(ui, 1, 12, app.colors),
                                            None
                                        );
                                    },
                                );
                            });
                        },
                    );
                    let texts: Vec<_> = ["Previous", "Page 2 of 12", "Next"]
                        .into_iter()
                        .map(|label| {
                            let text = output
                                .shapes
                                .iter()
                                .find_map(|shape| match &shape.shape {
                                    Shape::Text(text) if text.galley.job.text == label => {
                                        Some(text)
                                    }
                                    _ => None,
                                })
                                .unwrap_or_else(|| {
                                    panic!(
                                        "missing {label}; rendered: {:?}",
                                        output
                                            .shapes
                                            .iter()
                                            .filter_map(|shape| match &shape.shape {
                                                Shape::Text(text) => Some(&text.galley.job.text),
                                                _ => None,
                                            })
                                            .collect::<Vec<_>>()
                                    )
                                });
                            let rect = Rect::from_min_size(text.pos, text.galley.size());
                            assert!(
                                bounds.contains_rect(rect),
                                "navigation text clips at width {width}, scale {scale}"
                            );
                            rect
                        })
                        .collect();
                    for pair in texts.windows(2) {
                        assert!(
                            pair[1].top() > pair[0].top() + 1.0 || pair[1].left() > pair[0].right(),
                            "page controls appear in reverse order"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn preview_toolbar_controls_wrap_inside_narrow_panes() {
        for dark in [true, false] {
            for scale in [1.0, 1.35] {
                for width in [180.0, 238.0, 320.0] {
                    let ctx = Context::default();
                    let mut app = OttrinApp::default();
                    app.colors = Colors::preset(ThemePreset::Ottrin, dark);
                    app.colors.font_scale = scale;
                    app.apply_theme_to_ctx(&ctx);
                    let mut bounds = Rect::NOTHING;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(600.0, 400.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.scope_builder(
                                    egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                        ui.cursor().min,
                                        Vec2::new(width, 350.0),
                                    )),
                                    |ui| {
                                        bounds = ui.max_rect();
                                        ui.horizontal_wrapped(|ui| {
                                            crate::render_visual_sizing_controls(
                                                ui,
                                                crate::PreviewKind::Pdf,
                                                &mut true,
                                                &mut false,
                                                &mut 1.0,
                                                app.colors,
                                            );
                                            ui.add(
                                                Button::new("Expand preview")
                                                    .small()
                                                    .themed(&app.colors),
                                            );
                                        });
                                    },
                                );
                            });
                        },
                    );
                    let mut labels = Vec::new();
                    for shape in &output.shapes {
                        if let Shape::Text(text) = &shape.shape {
                            let rect = Rect::from_min_size(text.pos, text.galley.size());
                            assert_eq!(
                                text.galley.rows.len(),
                                1,
                                "toolbar label split across rows: {}",
                                text.galley.job.text
                            );
                            assert!(
                                bounds.expand(0.5).contains_rect(rect),
                                "{} at {rect:?} outside {bounds:?}, scale {scale}",
                                text.galley.job.text
                            );
                            labels.push(text.galley.job.text.as_str());
                        }
                    }
                    for label in ["Fit page", "Fit width", "100%", "−", "+", "Expand preview"] {
                        assert!(labels.contains(&label), "missing {label}");
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_destination_row_keeps_trailing_actions_inside_the_row() {
        use egui::RichText;
        for scale in [1.0, 1.35] {
            let ctx = Context::default();
            let mut app = OttrinApp::default();
            app.colors.font_scale = scale;
            app.apply_theme_to_ctx(&ctx);
            let c = app.colors;
            let mut bounds = Rect::NOTHING;
            let mut controls = Vec::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(600.0, 150.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.scope_builder(
                            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                                ui.cursor().min,
                                Vec2::new(260.0 * scale, 90.0),
                            )),
                            |ui| {
                                bounds = ui.available_rect_before_wrap();
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().button_padding =
                                        Vec2::new(3.0 * scale, 4.0 * scale);
                                    controls.push(
                                        ui.add(
                                            aligned_menu_button(
                                                crate::icons::MI_CHECK,
                                                crate::icons::MI_BOOKMARK,
                                                "A long destination folder name",
                                                &c,
                                            )
                                            .width(
                                                (ui.available_width()
                                                    - 3.0
                                                        * (24.0 * scale
                                                            + ui.spacing().item_spacing.x))
                                                    .max(40.0),
                                            ),
                                        )
                                        .rect,
                                    );
                                    for icon in [
                                        crate::icons::MI_ARROW_UP,
                                        crate::icons::MI_ARROW_DOWN,
                                        crate::icons::MI_CLEAR,
                                    ] {
                                        controls.push(
                                            ui.add_sized(
                                                [24.0 * scale, 28.0 * scale],
                                                Button::new(RichText::new(icon).size(14.0 * scale))
                                                    .themed(&c),
                                            )
                                            .rect,
                                        );
                                    }
                                });
                            },
                        );
                    });
                },
            );
            for rect in &controls {
                assert!(
                    bounds.expand(0.5).contains_rect(*rect),
                    "control {rect:?} outside {bounds:?} at {scale}"
                );
            }
            for pair in controls.windows(2) {
                assert!(pair[0].right() <= pair[1].left());
            }
        }
    }

    #[test]
    fn action_menu_omits_unused_check_column_and_keeps_icons_near_edge() {
        let ctx = Context::default();
        let app = OttrinApp::default();
        app.apply_theme_to_ctx(&ctx);
        let c = app.colors;
        let mut rows = Vec::new();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for (marker, label) in [("", "Action"), (" ", "Choice")] {
                    rows.push(
                        ui.add(aligned_menu_button(
                            marker,
                            crate::icons::MI_SETTINGS,
                            label,
                            &c,
                        ))
                        .rect,
                    );
                }
            });
        });
        let text_left = |label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    Shape::Text(text) if text.galley.job.text == label => Some(text.pos.x),
                    _ => None,
                })
                .unwrap()
        };
        let action_indent = text_left("Action") - rows[0].left();
        let choice_indent = text_left("Choice") - rows[1].left();
        assert!(
            action_indent <= 32.0 * c.font_scale,
            "Action label has excessive leading space: {action_indent}"
        );
        assert!((choice_indent - action_indent - 18.0 * c.font_scale).abs() < 1.0);
    }

    #[test]
    fn themed_controls_keep_the_resolved_palette_when_system_appearance_changes() {
        for dark in [true, false] {
            let ctx = Context::default();
            let mut app = OttrinApp::default();
            app.colors = Colors::preset(ThemePreset::Ottrin, dark);
            app.apply_theme_to_ctx(&ctx);
            let c = app.colors;
            for system_theme in [egui::Theme::Light, egui::Theme::Dark, egui::Theme::Light] {
                let mut input_rect = Rect::NOTHING;
                let mut text = String::from("Search");
                let output = ctx.run(
                    egui::RawInput {
                        system_theme: Some(system_theme),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            assert_eq!(ui.visuals().text_edit_bg_color(), c.panel_raised);
                            assert_eq!(ui.visuals().widgets.inactive.bg_fill, c.panel_raised);
                            input_rect = ui
                                .add(egui::TextEdit::singleline(&mut text).themed(&c))
                                .rect;
                            egui::ComboBox::from_id_salt("theme-regression-sort")
                                .selected_text("Name")
                                .show_ui(ui, |ui| {
                                    ui.label("Name");
                                });
                        });
                    },
                );
                assert!(
                    output.shapes.iter().any(|shape| matches!(
                        &shape.shape,
                        Shape::Rect(rect) if rect.rect.contains_rect(input_rect)
                            && rect.fill == c.panel_raised
                    )),
                    "the actual input frame must paint the resolved theme surface"
                );
                assert_eq!(ctx.system_theme(), Some(system_theme));
            }
        }
    }

    #[test]
    fn menu_rows_align_labels_and_icons_across_states_themes_and_text_scales() {
        for dark in [true, false] {
            for scale in [1.0, 1.35] {
                let ctx = Context::default();
                let mut app = OttrinApp::default();
                app.colors = Colors::preset(ThemePreset::Ottrin, dark);
                app.colors.font_scale = scale;
                app.apply_theme_to_ctx(&ctx);
                let c = app.colors;
                let mut rows = Vec::new();
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(700.0, 450.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(420.0 * scale);
                            for (indicator, icon, label, enabled) in [
                                (
                                    crate::icons::MI_CHECK,
                                    crate::icons::MI_VIEW_COLUMN,
                                    "Column view",
                                    true,
                                ),
                                ("", crate::icons::MI_VIEW_LIST, "List view", true),
                                ("", "", "Show hidden files", true),
                                ("", crate::icons::MI_IMAGE, "Preview unavailable", false),
                                (crate::icons::MI_CHECK, "", "Name", true),
                            ] {
                                rows.push(
                                    ui.add_enabled(
                                        enabled,
                                        aligned_menu_button(indicator, icon, label, &c)
                                            .indicator_column()
                                            .themed(&c),
                                    )
                                    .rect,
                                );
                            }
                        });
                    },
                );
                let labels = [
                    "Column view",
                    "List view",
                    "Show hidden files",
                    "Preview unavailable",
                    "Name",
                ];
                let mut left = None;
                for (idx, label) in labels.into_iter().enumerate() {
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Text(text) if text.galley.job.text == label => Some(text),
                            _ => None,
                        })
                        .expect("each label must render independently from its icon");
                    let rect = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(
                        rows[idx].contains_rect(rect),
                        "label {label} outside row at scale {scale}"
                    );
                    assert!((rect.center().y - rows[idx].center().y).abs() < 1.0);
                    assert!(
                        (text.pos.x - *left.get_or_insert(text.pos.x)).abs() < 0.1,
                        "checked, unchecked, iconless and disabled labels must start together"
                    );
                }
                for (idx, icon) in [
                    (0, crate::icons::MI_VIEW_COLUMN),
                    (1, crate::icons::MI_VIEW_LIST),
                    (3, crate::icons::MI_IMAGE),
                ] {
                    let text = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Text(text) if text.galley.job.text == icon => Some(text),
                            _ => None,
                        })
                        .expect("icon must render in its own slot");
                    let rect = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(rows[idx].contains_rect(rect));
                    assert!((rect.center().y - rows[idx].center().y).abs() < 1.0);
                }
            }
        }
    }

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
                base_preset: ThemePreset::Ottrin,
                dark: (custom).clone(),
                light: custom,
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
