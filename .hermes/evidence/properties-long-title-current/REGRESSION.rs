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
                    let title = format!("Properties — {}report.txt", "long metadata name ".repeat(8));
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
                        let output = ctx.run(egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 300.0))),
                            events,
                            ..Default::default()
                        }, |ctx| {
                            egui::CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| {
                                let bar = render_custom_titlebar(ui, ctx, colors, colors.titlebar_bg, &title, false, &mut close, |_| {});
                                assert!(bar.rect.right() <= width + 0.5, "Long title expanded chrome: {:?}", bar.rect);
                                close_position = egui::pos2(bar.rect.right() - 22.0, bar.rect.center().y);
                                ui.label("Window contents");
                                assert!(ui.min_rect().right() <= width + 0.5, "Long title expanded body: {:?}", ui.min_rect());
                            });
                        });
                        for shape in output.shapes {
                            if let egui::Shape::Text(text) = shape.shape {
                                assert!(text.pos.x + text.galley.size().x <= width + 0.5, "Title escaped viewport at width {width}, scale {scale}");
                            }
                        }
                    }
                    assert!(close, "Close was not clickable at width {width}, scale {scale}");
                }
            }
        }
    }
