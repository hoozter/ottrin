use crate::CUSTOM_TITLEBAR_HEIGHT;
use crate::theme_colors::Colors;
use egui::{
    Align, Color32, Context, Frame, Layout, Rect, ResizeDirection, RichText, Sense, Stroke, Vec2,
};

pub(crate) fn render_custom_titlebar(
    ui: &mut egui::Ui,
    ctx: &Context,
    colors: Colors,
    title_fill: Color32,
    title: &str,
    allow_drag: bool,
    close_requested: &mut bool,
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
            // Allocate the window control first so a long filename cannot
            // expand the viewport layout or push Close outside the window.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let close_btn = wm_btn(ui, WmBtn::Close, &colors);
                close_btn_rect = Some(close_btn.rect);
                if close_btn.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if close_btn.clicked() {
                    *close_requested = true;
                }
                ui.add_sized(
                    [ui.available_width(), CUSTOM_TITLEBAR_HEIGHT],
                    egui::Label::new(
                        RichText::new(title)
                            .color(colors.heading)
                            .size(12.5 * colors.font_scale)
                            .strong(),
                    )
                    .truncate()
                    .halign(Align::Min),
                )
                .on_hover_text(title);
            });
        });
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

/// Use identical native corner masking and borders for file-manager and picker windows.
pub(crate) fn paint_viewport_frame(ctx: &Context, colors: Colors, is_windows: bool) {
    use crate::rounded_alpha_callback_enabled;

    // Paint the same rounded border and alpha mask for every Ottrin viewport.
    // The GL callback runs first to fix the alpha channel, then the
    // border stroke paints on top so it is never occluded.
    let vp_rect = ctx.viewport_rect();
    let border_painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("viewport_border"),
    ));

    let radius = colors.app_radius as f32;
    if rounded_alpha_callback_enabled(is_windows) {
        border_painter.add(egui::Shape::Callback(egui::PaintCallback {
            rect: vp_rect,
            callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(
                move |info, painter| {
                    use eframe::glow::HasContext as _;
                    let gl = painter.gl();
                    let [scr_w, scr_h] = info.screen_size_px;
                    let scr_w = scr_w as i32;
                    let scr_h = scr_h as i32;
                    let ppp = info.pixels_per_point;

                    unsafe {
                        let r_f = radius * ppp;
                        let r_px = r_f.ceil() as i32;

                        // --- alpha pass: only write the A channel --------
                        gl.color_mask(false, false, false, true);

                        // 1. Clear alpha to 0 everywhere (fully transparent).
                        gl.clear_color(0.0, 0.0, 0.0, 0.0);
                        gl.disable(eframe::glow::SCISSOR_TEST);
                        gl.clear(eframe::glow::COLOR_BUFFER_BIT);

                        // 2. Fill alpha=1.0 only inside the rounded rect.
                        gl.clear_color(0.0, 0.0, 0.0, 1.0);
                        gl.enable(eframe::glow::SCISSOR_TEST);

                        if r_px > 0 && scr_h > 2 * r_px {
                            // Middle band: full width, fully opaque.
                            gl.scissor(0, r_px, scr_w, scr_h - 2 * r_px);
                            gl.clear(eframe::glow::COLOR_BUFFER_BIT);

                            // Corner scanlines: arc-shaped inset.
                            for row in 0..r_px {
                                let dy = r_f - row as f32 - 0.5;
                                let inset = if dy <= 0.0 {
                                    0
                                } else if dy >= r_f {
                                    scr_w // fully outside
                                } else {
                                    let x_inside = (r_f * r_f - dy * dy).sqrt();
                                    let edge = r_f - x_inside;
                                    (edge.ceil() as i32).clamp(0, r_px)
                                };
                                let strip_w = scr_w - 2 * inset;
                                if strip_w <= 0 {
                                    continue;
                                }
                                // Top row (GL origin is bottom-left).
                                gl.scissor(inset, scr_h - 1 - row, strip_w, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                // Bottom row.
                                gl.scissor(inset, row, strip_w, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                            }
                        } else {
                            // No rounding or window too small — full opaque.
                            gl.scissor(0, 0, scr_w, scr_h);
                            gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                        }

                        // --- RGB pass: zero out RGB in corner boxes so
                        //     stale content colour doesn't bleed through
                        //     the border's anti-aliased fringe. ----------
                        if r_px > 0 {
                            gl.color_mask(true, true, true, false);
                            gl.clear_color(0.0, 0.0, 0.0, 0.0);
                            // Only clear the thin L-shaped strips outside
                            // the arc, not the whole corner box (to avoid
                            // destroying content pixels inside the arc).
                            for row in 0..r_px {
                                let dy = r_f - row as f32 - 0.5;
                                let inset = if dy <= 0.0 {
                                    0
                                } else if dy >= r_f {
                                    r_px
                                } else {
                                    let x_inside = (r_f * r_f - dy * dy).sqrt();
                                    let edge = r_f - x_inside;
                                    (edge.ceil() as i32).clamp(0, r_px)
                                };
                                if inset <= 0 {
                                    continue;
                                }
                                // Top-left / top-right
                                gl.scissor(0, scr_h - 1 - row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                gl.scissor(scr_w - inset, scr_h - 1 - row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                // Bottom-left / bottom-right
                                gl.scissor(0, row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                                gl.scissor(scr_w - inset, row, inset, 1);
                                gl.clear(eframe::glow::COLOR_BUFFER_BIT);
                            }
                        }

                        gl.color_mask(true, true, true, true);
                        gl.disable(eframe::glow::SCISSOR_TEST);
                    }
                },
            )),
        }));
    }

    // Border stroke renders AFTER the callback so it sits on top.
    border_painter.rect_stroke(
        vp_rect,
        colors.app_radius,
        Stroke::new(colors.window_border_width, colors.window_border),
        egui::StrokeKind::Inside,
    );
}

/// Shared edge resize behavior for undecorated application windows.
pub(crate) fn handle_viewport_resize(ctx: &Context) {
    let (maximized, fullscreen) = ctx.input(|i| {
        let viewport = i.viewport();
        (
            viewport.maximized.unwrap_or(false),
            viewport.fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen {
        return;
    }

    let rect = ctx.viewport_rect();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };

    if let Some(direction) = resize_direction_for_pos(rect, pos, 6.0) {
        ctx.set_cursor_icon(cursor_icon_for_resize_direction(direction));
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        if primary_pressed {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_titles_keep_close_visible_and_clickable_without_expanding_the_window() {
        for dark in [true, false] {
            for scale in [1.0, 1.75] {
                for width in [320.0, 720.0] {
                    let ctx = Context::default();
                    crate::install_application_fonts(&ctx);
                    let mut colors = Colors::preset(ottrin_core::ThemePreset::Ottrin, dark);
                    colors.font_scale = scale;
                    crate::theme_colors::apply_to_context(&ctx, &colors);
                    let title =
                        format!("Properties — {}report.txt", "long metadata name ".repeat(8));
                    let mut close = false;
                    let mut close_position = egui::Pos2::ZERO;
                    for frame in 0..4 {
                        let mut events = Vec::new();
                        if frame >= 2 {
                            events.push(egui::Event::PointerMoved(close_position));
                            events.push(egui::Event::PointerButton {
                                pos: close_position,
                                button: egui::PointerButton::Primary,
                                pressed: frame == 2,
                                modifiers: egui::Modifiers::NONE,
                            });
                        }
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 300.0),
                                )),
                                events,
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default()
                                    .frame(Frame::NONE)
                                    .show(ctx, |ui| {
                                        let bar = render_custom_titlebar(
                                            ui,
                                            ctx,
                                            colors,
                                            colors.titlebar_bg,
                                            &title,
                                            false,
                                            &mut close,
                                        );
                                        assert!(
                                            bar.rect.right() <= width + 0.5,
                                            "Long title expanded chrome: {:?}",
                                            bar.rect
                                        );
                                        close_position = egui::pos2(
                                            bar.rect.right() - 22.0,
                                            bar.rect.center().y,
                                        );
                                        ui.label("Window contents");
                                        assert!(
                                            ui.min_rect().right() <= width + 0.5,
                                            "Long title expanded body: {:?}",
                                            ui.min_rect()
                                        );
                                    });
                            },
                        );
                        for shape in output.shapes {
                            if let egui::Shape::Text(text) = shape.shape {
                                assert!(
                                    text.pos.x + text.galley.size().x <= width + 0.5,
                                    "Title escaped viewport at width {width}, scale {scale}"
                                );
                            }
                        }
                    }
                    assert!(
                        close,
                        "Close was not clickable at width {width}, scale {scale}"
                    );
                }
            }
        }
    }

    #[test]
    fn custom_titlebar_does_not_draw_a_full_width_separator_below_the_chrome() {
        let ctx = Context::default();
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(640.0, 480.0),
            )),
            ..Default::default()
        });
        let colors = Colors::preset(ottrin_core::ThemePreset::Ottrin, true);
        egui::CentralPanel::default().show(&ctx, |ui| {
            let mut close_requested = false;
            render_custom_titlebar(
                ui,
                &ctx,
                colors,
                colors.titlebar_bg,
                "Test title",
                false,
                &mut close_requested,
            );
        });
        let output = ctx.end_pass();
        assert!(
            !output.shapes.iter().any(|shape| matches!(
                shape.shape,
                egui::Shape::LineSegment { points, .. }
                    if (points[0].x - points[1].x).abs() > 600.0
            )),
            "the titlebar must not add an unconditional full-width divider"
        );
    }

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
