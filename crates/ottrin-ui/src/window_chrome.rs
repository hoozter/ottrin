use crate::CUSTOM_TITLEBAR_HEIGHT;
use crate::icons::MI_CLOSE;
use crate::theme_colors::{Colors, mix_color};
use egui::{
    Align, Color32, Context, Frame, Layout, Rect, ResizeDirection, RichText, Sense, Shadow, Stroke,
    Vec2,
};

pub(crate) fn viewport_window_fill(colors: Colors, focused: bool) -> (Color32, Color32, Shadow) {
    let window_fill = mix_color(colors.bg, colors.panel, 0.12);
    let title_fill = if focused {
        colors.titlebar_bg
    } else {
        mix_color(colors.titlebar_bg, colors.bg, 0.48)
    };
    let shadow = Shadow {
        offset: [0, 8],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(if focused { 80 } else { 56 }),
    };
    (window_fill, title_fill, shadow)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_custom_titlebar(
    ui: &mut egui::Ui,
    ctx: &Context,
    colors: Colors,
    title_fill: Color32,
    title: &str,
    allow_drag: bool,
    close_requested: &mut bool,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let mut close_btn_rect: Option<Rect> = None;
    let title_bar = Frame::new()
        .fill(title_fill)
        .inner_margin(egui::Margin {
            left: 6,
            right: 8,
            top: 1,
            bottom: 1,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(CUSTOM_TITLEBAR_HEIGHT);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(colors.text).size(12.5).strong());
                ui.add_space(18.0);
                body(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let close_btn = ui.add(
                        egui::Button::new(
                            RichText::new(MI_CLOSE).size(16.0).color(colors.text_muted),
                        )
                        .frame(false)
                        .min_size(Vec2::new(24.0, 24.0)),
                    );
                    close_btn_rect = Some(close_btn.rect);
                    if close_btn.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if close_btn.clicked() {
                        *close_requested = true;
                    }
                });
            });
        });
    ui.painter().hline(
        title_bar.response.rect.x_range(),
        ui.cursor().min.y,
        Stroke::new(1.0, mix_color(colors.border, title_fill, 0.6)),
    );
    if allow_drag {
        // Use the full titlebar rect (excluding close button) as the drag area.
        let bar_rect = title_bar.response.rect;
        let can_drag = ctx.input(|i| {
            if !i.pointer.primary_pressed() {
                return false;
            }
            let Some(pos) = i.pointer.hover_pos() else {
                return false;
            };
            if !bar_rect.contains(pos) {
                return false;
            }
            // Don't drag when clicking the close button.
            if let Some(close_r) = close_btn_rect
                && close_r.expand(4.0).contains(pos)
            {
                return false;
            }
            true
        });
        if can_drag {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
    title_bar.response
}
/// Minimal flat window-control button. Draws a line-based icon (×, □, −).
/// Uses muted grey at rest, text colour on hover — no colours.
pub(crate) fn wm_btn(ui: &mut egui::Ui, kind: WmBtn, c: &Colors) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let col = if resp.hovered() { c.text } else { c.text_muted };
    let p = ui.painter();
    let ctr = rect.center();
    let sw = 1.2; // stroke width

    match kind {
        WmBtn::Close => {
            // × — two diagonal lines
            let d = 4.5;
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x - d, ctr.y - d),
                    egui::Pos2::new(ctr.x + d, ctr.y + d),
                ],
                Stroke::new(sw, col),
            );
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x + d, ctr.y - d),
                    egui::Pos2::new(ctr.x - d, ctr.y + d),
                ],
                Stroke::new(sw, col),
            );
        }
        WmBtn::Maximize => {
            // □ — square outline
            let d = 4.5;
            let r = egui::Rect::from_center_size(ctr, Vec2::splat(d * 2.0));
            p.rect_stroke(r, 0.0, Stroke::new(sw, col), egui::StrokeKind::Middle);
        }
        WmBtn::Minimize => {
            // − — horizontal bar
            let d = 4.5;
            p.line_segment(
                [
                    egui::Pos2::new(ctr.x - d, ctr.y),
                    egui::Pos2::new(ctr.x + d, ctr.y),
                ],
                Stroke::new(sw, col),
            );
        }
    }
    resp
}

pub(crate) fn resize_direction_for_pos(
    rect: Rect,
    pos: egui::Pos2,
    margin: f32,
) -> Option<ResizeDirection> {
    let left = pos.x <= rect.left() + margin;
    let right = pos.x >= rect.right() - margin;
    let top = pos.y <= rect.top() + margin;
    let bottom = pos.y >= rect.bottom() - margin;

    match (left, right, top, bottom) {
        (true, false, true, false) => Some(ResizeDirection::NorthWest),
        (false, true, true, false) => Some(ResizeDirection::NorthEast),
        (true, false, false, true) => Some(ResizeDirection::SouthWest),
        (false, true, false, true) => Some(ResizeDirection::SouthEast),
        (true, false, false, false) => Some(ResizeDirection::West),
        (false, true, false, false) => Some(ResizeDirection::East),
        (false, false, true, false) => Some(ResizeDirection::North),
        (false, false, false, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

pub(crate) fn cursor_icon_for_resize_direction(direction: ResizeDirection) -> egui::CursorIcon {
    match direction {
        ResizeDirection::North => egui::CursorIcon::ResizeNorth,
        ResizeDirection::South => egui::CursorIcon::ResizeSouth,
        ResizeDirection::East => egui::CursorIcon::ResizeEast,
        ResizeDirection::West => egui::CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => egui::CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => egui::CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => egui::CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => egui::CursorIcon::ResizeSouthWest,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum WmBtn {
    Close,
    Maximize,
    Minimize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_direction_detects_edges_and_corners() {
        let rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 80.0));
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(2.0, 2.0), 6.0),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(98.0, 2.0), 6.0),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(50.0, 78.0), 6.0),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(1.0, 40.0), 6.0),
            Some(ResizeDirection::West)
        );
        assert_eq!(
            resize_direction_for_pos(rect, egui::pos2(50.0, 40.0), 6.0),
            None
        );
    }
}
