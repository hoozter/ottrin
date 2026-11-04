//! Read-only standalone metadata window. No index, preview renderer or settings writer.
use crate::{Colors, file_details, path_label, widget_style::ThemeButtonExt, window_chrome};
use egui::{Context, RichText};
use ottrin_core::{
    AppConfig, EntryKind, GraphicsRuntimeStatus, format_file_timestamp, format_size,
};
use ottrin_platform::FileProperties;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

struct Item {
    path: PathBuf,
    result: Option<Result<FileProperties, String>>,
}

pub struct PropertiesApp {
    items: Vec<Item>,
    selected: usize,
    title: String,
    results: Option<mpsc::Receiver<(usize, Result<FileProperties, String>)>>,
    stopped: Arc<AtomicBool>,
    config: AppConfig,
    colors: Colors,
    appearance: crate::system_appearance::SystemAppearance,
    is_windows: bool,
}
impl PropertiesApp {
    pub fn title(paths: &[PathBuf]) -> String {
        if let [path] = paths {
            format!("Properties — {}", ottrin_preview::preview_title(path))
        } else {
            format!("Properties — {} items", paths.len())
        }
    }
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<PathBuf>,
        runtime: GraphicsRuntimeStatus,
    ) -> Result<Self, String> {
        if paths.iter().any(|path| !path.is_absolute()) {
            return Err("Properties requires absolute native paths".into());
        }
        if paths.is_empty() {
            return Err("Properties needs at least one item".into());
        }
        crate::install_application_fonts(&cc.egui_ctx);
        let config = crate::load_config()?;
        let colors = Colors::for_config(&config, cc.egui_ctx.system_theme());
        crate::theme_colors::apply_to_context(&cc.egui_ctx, &colors);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let (completed, results) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let inputs = paths.clone();
        std::thread::Builder::new()
            .name("file-properties".into())
            .spawn(move || {
                for (index, path) in inputs.into_iter().enumerate() {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let result =
                        ottrin_platform::read_file_properties(&path).map_err(|e| e.to_string());
                    if completed.send((index, result)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .map_err(|e| format!("Cannot start the properties reader: {e}"))?;
        let title = Self::title(&paths);
        Ok(Self {
            items: paths
                .into_iter()
                .map(|path| Item { path, result: None })
                .collect(),
            selected: 0,
            title,
            results: Some(results),
            stopped,
            config,
            colors,
            appearance: crate::system_appearance::SystemAppearance::start(cc.egui_ctx.clone()),
            is_windows: runtime.is_windows,
        })
    }
    fn poll(&mut self) {
        loop {
            let Some(results) = self.results.as_ref() else {
                break;
            };
            match results.try_recv() {
                Ok((index, result)) => self.items[index].result = Some(result),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.results = None;
                    for item in &mut self.items {
                        if item.result.is_none() {
                            item.result = Some(Err(
                                "Properties reader ended before this item was read".into(),
                            ));
                        }
                    }
                    break;
                }
            }
        }
    }
    fn contents(&mut self, ui: &mut egui::Ui) {
        let c = self.colors;
        if self.items.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Item").strong());
                egui::ComboBox::from_id_salt("properties-item")
                    .selected_text(path_label(&self.items[self.selected].path))
                    .width((ui.available_width() - 20.0).max(120.0))
                    .show_ui(ui, |ui| {
                        for (index, item) in self.items.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, index, path_label(&item.path));
                        }
                    });
            });
            ui.add_space(12.0);
        }
        let item = &self.items[self.selected];
        match &item.result {
            Some(Ok(properties)) => render_properties(ui, c, &self.config, properties),
            Some(Err(error)) => {
                ui.label(RichText::new(path_label(&item.path)).strong());
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Could not read properties: {error}")).color(c.error),
                );
            }
            None => {
                ui.spinner();
                ui.label("Reading properties…");
            }
        }
    }
}
impl eframe::App for PropertiesApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.appearance.poll();
        let colors = Colors::for_config(
            &self.config,
            ctx.system_theme().or(self.appearance.preferred()),
        );
        if colors != self.colors {
            self.colors = colors;
            crate::theme_colors::apply_to_context(ctx, &colors);
        }
        let mut close =
            ctx.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.panel).inner_margin(0))
            .show(ctx, |ui| {
                let title = crate::administrator_surface_label(
                    &self.title,
                    crate::is_administrator_session(),
                );
                window_chrome::render_custom_titlebar(
                    ui,
                    ctx,
                    colors,
                    colors.titlebar_bg,
                    &title,
                    true,
                    &mut close,
                );
                egui::TopBottomPanel::bottom("properties-actions")
                    .frame(egui::Frame::new().fill(colors.panel).inner_margin(12))
                    .show_inside(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new("Close").themed(&colors)).clicked() {
                                close = true;
                            }
                        });
                    });
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .inner_margin(16)
                            .show(ui, |ui| self.contents(ui));
                    });
            });
        window_chrome::paint_viewport_frame(ctx, colors, self.is_windows);
        window_chrome::handle_viewport_resize(ctx, colors.app_radius);
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
impl Drop for PropertiesApp {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}
fn row(ui: &mut egui::Ui, c: Colors, title: &str, value: &str) {
    ui.label(RichText::new(title).color(c.text_muted));
    ui.add(
        egui::Label::new(RichText::new(value).color(c.text))
            .selectable(true)
            .wrap(),
    );
    ui.end_row();
}
fn render_properties(
    ui: &mut egui::Ui,
    c: Colors,
    config: &AppConfig,
    properties: &FileProperties,
) {
    let entry = ottrin_core::FileEntry {
        name: ottrin_preview::preview_title(&properties.path),
        path: properties.path.clone(),
        kind: properties.kind,
        #[cfg(unix)]
        is_executable: properties.kind == EntryKind::File && properties.unix_mode & 0o111 != 0,
        #[cfg(not(unix))]
        is_executable: false,
        symlink_target_is_dir: None,
        size_bytes: Some(properties.size_bytes),
        modified_unix_secs: None,
    };
    let style = crate::style_for_entry(&entry, crate::EntryVisualState::default(), &c, config);
    let icon = crate::material_icon(style.icon);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(icon)
                .size(30.0 * c.font_scale)
                .color(style.icon_color),
        );
        ui.vertical(|ui| {
            ui.label(
                RichText::new(ottrin_preview::preview_title(&properties.path))
                    .strong()
                    .color(c.heading),
            );
            ui.label(
                RichText::new(file_details::file_type_description(
                    &properties.path,
                    properties.kind,
                ))
                .color(c.text_muted),
            );
        });
    });
    ui.add_space(16.0);
    crate::appearance_editor::settings_surface(c)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Details").strong().color(c.heading));
            ui.add_space(8.0);
            egui::Grid::new("properties-details")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .max_col_width((ui.available_width() * 0.7).max(100.0))
                .show(ui, |ui| {
                    row(ui, c, "Location", &path_label(&properties.path));
                    if properties.kind == EntryKind::File {
                        row(
                            ui,
                            c,
                            "Size",
                            &format!(
                                "{} ({} bytes)",
                                format_size(properties.size_bytes),
                                properties.size_bytes
                            ),
                        );
                    }
                    if let Some(target) = &properties.symlink_target {
                        row(ui, c, "Link target", &path_label(target));
                    }
                    row(
                        ui,
                        c,
                        "Modified",
                        &format_file_timestamp(properties.modified),
                    );
                    row(ui, c, "Created", &format_file_timestamp(properties.created));
                });
        });
    ui.add_space(12.0);
    crate::appearance_editor::settings_surface(c)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Permissions").strong().color(c.heading));
            ui.add_space(8.0);
            egui::Grid::new("properties-permissions")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    #[cfg(unix)]
                    row(
                        ui,
                        c,
                        "Mode",
                        &file_details::format_unix_permissions(properties.unix_mode),
                    );
                    #[cfg(windows)]
                    row(
                        ui,
                        c,
                        "Read-only attribute",
                        if properties.readonly { "Yes" } else { "No" },
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn property_rows_use_real_metadata_and_remain_within_the_content_width() {
        let root = tempfile::tempdir().unwrap();
        let path = root
            .path()
            .join(format!("{}report.txt", "long metadata name ".repeat(8)));
        std::fs::write(&path, b"metadata must not change this file").unwrap();
        let properties = ottrin_platform::read_file_properties(&path).unwrap();
        for theme in [ottrin_core::ThemeMode::Dark, ottrin_core::ThemeMode::Light] {
            for scale in [1.0, 1.75] {
                let config = AppConfig {
                    theme,
                    ..AppConfig::default()
                };
                let mut colors = Colors::for_config(&config, None);
                colors.font_scale = scale;
                let ctx = Context::default();
                crate::install_application_fonts(&ctx);
                crate::theme_colors::apply_to_context(&ctx, &colors);
                let mut texts = Vec::new();
                // Long native paths can push Permissions below the viewport.
                // Inspect both scroll positions rather than requiring every row
                // to fit on screen at the largest text scale.
                let mut bottom = 0.0;
                for at_bottom in [false, true] {
                    for _ in 0..2 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(720.0, 540.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().frame(egui::Frame::NONE).show(
                                    ctx,
                                    |ui| {
                                        let mut close = false;
                                        window_chrome::render_custom_titlebar(
                                            ui,
                                            ctx,
                                            colors,
                                            colors.titlebar_bg,
                                            &format!("Properties — {}", path_label(&path)),
                                            false,
                                            &mut close,
                                        );
                                        let scroll = egui::ScrollArea::vertical()
                                            .auto_shrink([false, false])
                                            .vertical_scroll_offset(if at_bottom {
                                                bottom
                                            } else {
                                                0.0
                                            })
                                            .show(ui, |ui| {
                                                egui::Frame::new().inner_margin(16).show(
                                                    ui,
                                                    |ui| {
                                                        render_properties(
                                                            ui,
                                                            colors,
                                                            &config,
                                                            &properties,
                                                        )
                                                    },
                                                );
                                            });
                                        assert!(
                                            ui.min_rect().right() <= 720.5,
                                            "Properties expanded beyond viewport: {:?}",
                                            ui.min_rect()
                                        );
                                        bottom = (scroll.content_size.y
                                            - scroll.inner_rect.height())
                                        .max(0.0);
                                    },
                                );
                            },
                        );
                        texts.extend(output.shapes.into_iter().filter_map(
                            |shape| match shape.shape {
                                egui::Shape::Text(text) => Some(text),
                                _ => None,
                            },
                        ));
                    }
                }
                for label in [
                    "Details",
                    "Location",
                    "Size",
                    "Modified",
                    "Created",
                    "Permissions",
                    #[cfg(unix)]
                    "Mode",
                    #[cfg(windows)]
                    "Read-only attribute",
                ] {
                    assert!(
                        texts.iter().any(|text| text.galley.job.text == label),
                        "Missing metadata label: {label}; scale {scale}; rendered {:?}",
                        texts.iter().map(|t| &t.galley.job.text).collect::<Vec<_>>()
                    );
                }
                for text in texts {
                    assert!(
                        text.pos.x + text.galley.size().x <= 720.5,
                        "Metadata overflowed at scale {scale}: {}",
                        text.galley.job.text
                    );
                }
            }
        }
        assert_eq!(
            std::fs::read(path).unwrap(),
            b"metadata must not change this file"
        );
    }
    #[cfg(unix)]
    #[test]
    fn permission_labels_include_set_id_and_sticky_bits() {
        for (mode, expected) in [
            (0o755, "rwxr-xr-x (0755)"),
            (0o4755, "rwsr-xr-x (4755)"),
            (0o2640, "rw-r-S--- (2640)"),
            (0o1777, "rwxrwxrwt (1777)"),
        ] {
            assert_eq!(file_details::format_unix_permissions(mode), expected);
        }
    }
}
