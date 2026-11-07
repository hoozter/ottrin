//! The file-manager footer, shared with the appearance editor's sample.
use super::*;

pub(crate) struct Indicator<'a> {
    pub icon: &'static str,
    pub color: Color32,
    pub size: f32,
    pub tooltip: &'a str,
}

pub(crate) struct State<'a> {
    pub helper: Indicator<'a>,
    pub index_color: Color32,
    pub index_tooltip: &'a str,
    pub show_hidden_files: bool,
    pub command_visible: bool,
    pub status: &'a str,
    pub feedback: Option<(&'a str, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Services,
    ToggleHidden,
    ToggleCommand,
}

pub(crate) fn render(ui: &mut egui::Ui, c: Colors, state: State<'_>) -> Option<Action> {
    let bottom_text = c.text_muted;
    let mut action = None;
    let mut text_rect = Rect::NOTHING;
    let mut controls_left = ui.max_rect().right();
    // Reserve persistent controls before allocating flexible footer text.
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(4.0);
        let helper = state.helper;
        let response = ui.add(
            egui::Button::new(
                RichText::new(helper.icon)
                    .size(helper.size * c.font_scale)
                    .color(helper.color),
            )
            .frame_when_inactive(false)
            .min_size(Vec2::splat(22.0))
            .themed(&c)
            .accessible_name("Manage administrator helper"),
        );
        if response
            .on_hover_text(format!("{} — click to manage services", helper.tooltip))
            .clicked()
        {
            action = Some(Action::Services);
        }
        ui.add_space(2.0);
        let s_resp = ui.add(
            egui::Button::new(
                RichText::new(MI_SEARCH)
                    .size(13.0 * c.font_scale)
                    .color(state.index_color),
            )
            .frame_when_inactive(false)
            .min_size(Vec2::splat(22.0))
            .themed(&c)
            .accessible_name("Manage index service"),
        );
        if s_resp
            .on_hover_text(format!(
                "{}\nClick to manage the index service",
                state.index_tooltip
            ))
            .clicked()
        {
            action = Some(Action::Services);
        }
        ui.add_space(2.0);
        // Hidden files toggle — right side
        let (hidden_icon, hidden_color) = if state.show_hidden_files {
            (MI_VISIBILITY, c.accent)
        } else {
            (MI_VISIBILITY_OFF, bottom_text)
        };
        let hid_resp = ui.add(
            egui::Button::new(
                RichText::new(hidden_icon)
                    .size(16.0 * c.font_scale)
                    .color(hidden_color),
            )
            .frame_when_inactive(false)
            .min_size(Vec2::splat(24.0))
            .themed(&c)
            .accessible_name(if state.show_hidden_files {
                "Hide hidden files"
            } else {
                "Show hidden files"
            }),
        );
        if hid_resp.clicked() {
            action = Some(Action::ToggleHidden);
        }
        controls_left = hid_resp.rect.left();
        hid_resp.on_hover_text(if state.show_hidden_files {
            "Hide hidden files"
        } else {
            "Show hidden files"
        });
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            // Command frame toggle — LEFT side (bar opens at the bottom-left)
            let cf_color = if state.command_visible {
                c.accent_dim
            } else {
                bottom_text
            };
            let cf_resp = ui.add(
                egui::Button::new(
                    RichText::new(MI_TERMINAL)
                        .size(16.0 * c.font_scale)
                        .color(cf_color),
                )
                .frame_when_inactive(false)
                .min_size(Vec2::splat(24.0))
                .themed(&c)
                .accessible_name(if state.command_visible {
                    "Hide command bar"
                } else {
                    "Show command bar"
                }),
            );
            if cf_resp.clicked() {
                action = Some(Action::ToggleCommand);
            }
            cf_resp.on_hover_text("Toggle command bar  (or type any key)");
            ui.add_space(4.0);

            // Item count + selection name
            let status = state.status;
            let neutral = egui::TextFormat {
                font_id: FontId::proportional(11.5 * c.font_scale),
                color: bottom_text,
                ..Default::default()
            };
            let mut job = egui::text::LayoutJob::default();
            let tooltip = if let Some((message, ok)) = state.feedback {
                // Feedback has priority over a potentially long selection.
                job.append(
                    &message.replace(['\r', '\n'], " "),
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(11.0 * c.font_scale),
                        color: if ok { c.accent } else { c.error },
                        ..Default::default()
                    },
                );
                job.append("  ·  ", 0.0, neutral.clone());
                format!("{}\n{}", message, status)
            } else {
                status.to_owned()
            };
            job.append(&status.replace(['\r', '\n'], " "), 0.0, neutral);
            text_rect = ui
                .add(
                    egui::Label::new(job)
                        .truncate()
                        .show_tooltip_when_elided(false),
                )
                .on_hover_text(tooltip)
                .rect;
        });
    });

    // Subtle centered wordmark in the status bar.
    let logo_size = Vec2::new(82.0, 18.0);
    let logo_rect = Rect::from_center_size(ui.max_rect().center(), logo_size);
    if text_rect.right() + 8.0 <= logo_rect.left() && logo_rect.right() + 8.0 <= controls_left {
        ui.put(
            logo_rect,
            egui::Image::new(egui::include_image!("../../../assets/logo.svg"))
                .fit_to_exact_size(logo_size)
                .tint(c.text_muted),
        );
    }
    action
}
