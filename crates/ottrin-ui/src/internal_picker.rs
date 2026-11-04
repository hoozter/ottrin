//! Parent-owned modal use of the same chooser component as the external host.
use super::*;
use ottrin_core::picker::{PickerFilter, PickerMode, PickerPattern, PickerRequest};
use picker::PickerDialog;
use picker_preferences::PickerPreferences;

pub(super) enum Purpose {
    ThemeExport(String),
    ThemeImport,
    OpenWith(PathBuf),
    SearchInclude,
    SearchExclude,
    ContentInclude,
    ContentExclude,
}
pub(super) struct PendingPicker {
    parent: egui::ViewportId,
    dialog: PickerDialog,
    purpose: Purpose,
}
impl OttrinApp {
    pub(super) fn start_internal_picker(
        &mut self,
        ctx: &Context,
        request: PickerRequest,
        purpose: Purpose,
    ) {
        if let Some(picker) = &self.pending_picker {
            ctx.send_viewport_cmd_to(picker.parent, egui::ViewportCommand::Focus);
            return;
        }
        let preferences = PickerPreferences::load().unwrap_or_else(|error| {
            eprintln!("ottrin: {error}; using default picker preferences");
            PickerPreferences::default()
        });
        match PickerDialog::new(request, ctx, preferences) {
            Ok(dialog) => {
                self.pending_picker = Some(PendingPicker {
                    parent: ctx.viewport_id(),
                    dialog,
                    purpose,
                })
            }
            Err(error) => {
                self.set_status_message(format!("Could not open file picker: {error}"), false, 5)
            }
        }
        ctx.request_repaint();
    }
    pub(super) fn pick_search_folder(&mut self, ctx: &Context, purpose: Purpose, title: &str) {
        self.start_internal_picker(
            ctx,
            PickerRequest {
                mode: PickerMode::Folder,
                title: title.into(),
                ..Default::default()
            },
            purpose,
        );
    }
    pub(super) fn theme_picker_request(
        mode: PickerMode,
        title: &str,
        filename: String,
    ) -> PickerRequest {
        PickerRequest {
            mode,
            title: title.into(),
            filename: filename.into(),
            filters: vec![PickerFilter {
                name: "Ottrin theme JSON".into(),
                patterns: vec![PickerPattern::Glob("*.json".into())],
            }],
            ..Default::default()
        }
    }
    pub(super) fn cancel_internal_picker(&mut self, parent: egui::ViewportId) {
        if self
            .pending_picker
            .as_ref()
            .is_some_and(|picker| picker.parent == parent)
        {
            let pending = self.pending_picker.take().expect("matching picker");
            remember(&pending.dialog);
        }
    }
    pub(super) fn render_internal_picker(&mut self, ctx: &Context) {
        if self
            .pending_picker
            .as_ref()
            .is_none_or(|picker| picker.parent != ctx.viewport_id())
        {
            return;
        }
        let mut pending = self.pending_picker.take().expect("matching picker");
        let c = self.colors;
        let size = (ctx.content_rect().size() - Vec2::splat(32.0))
            .min(Vec2::new(940.0, 640.0))
            .max(Vec2::splat(1.0));
        egui::Modal::new(egui::Id::new("ottrin-owned-file-picker"))
            .frame(
                Frame::new()
                    .fill(c.panel)
                    .corner_radius(c.app_radius)
                    .stroke(Stroke::new(c.window_border_width, c.window_border)),
            )
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                let mut close = false;
                render_custom_titlebar(
                    ui,
                    ctx,
                    c,
                    c.titlebar_bg,
                    pending.dialog.request().window_title(),
                    false,
                    &mut close,
                );
                if close {
                    pending.dialog.cancel();
                }
                Frame::new()
                    .inner_margin(10)
                    .show(ui, |ui| pending.dialog.show(ui, c, &self.state.config));
            });
        if let Some(response) = pending.dialog.take_result() {
            remember(&pending.dialog);
            if response.accepted
                && let Some(path) = response.paths.into_iter().next()
            {
                self.complete_internal_picker(ctx, pending.purpose, path);
            }
            ctx.request_repaint();
        } else {
            self.pending_picker = Some(pending);
        }
    }
    fn complete_internal_picker(&mut self, ctx: &Context, purpose: Purpose, path: PathBuf) {
        match purpose {
            Purpose::ThemeExport(json) => {
                let (message, success) = match std::fs::write(&path, json) {
                    Ok(()) => (format!("Exported theme to {}", path_label(&path)), true),
                    Err(error) => (format!("Theme export failed: {error}"), false),
                };
                self.theme_status =
                    Some((message, success, Instant::now() + Duration::from_secs(4)));
            }
            Purpose::ThemeImport => self.import_theme_from_path(ctx, &path),
            Purpose::OpenWith(target) => {
                match self.platform.open_with_application(&target, &path) {
                    Ok(()) => self.set_status_message(
                        format!("Opened with {}", display_app_name(&path)),
                        true,
                        3,
                    ),
                    Err(error) => {
                        self.set_status_message(format!("Open with failed: {error}"), false, 5)
                    }
                }
            }
            purpose => {
                let roots = match purpose {
                    Purpose::SearchInclude => &mut self.state.config.search.include_roots,
                    Purpose::SearchExclude => &mut self.state.config.search.exclude_roots,
                    Purpose::ContentInclude => &mut self.state.config.search.content_include_roots,
                    Purpose::ContentExclude => &mut self.state.config.search.content_exclude_roots,
                    _ => unreachable!("non-folder purpose handled above"),
                };
                if !roots.contains(&path) {
                    roots.push(path);
                }
                self.search_service
                    .update_config(self.state.config.search.clone());
                self.persist_config();
            }
        }
    }
}
fn remember(dialog: &PickerDialog) {
    if let Err(error) = dialog.preferences().save() {
        eprintln!("ottrin: Could not remember picker preferences: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn frame(
        app: &mut OttrinApp,
        ctx: &Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                app.handle_keyboard(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Parent window");
                });
                app.render_internal_picker(ctx);
            },
        )
    }
    fn text_center(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
        // The title and footer action can share a label. Prefer the final action.
        output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == wanted => {
                    let rect = Rect::from_min_size(text.pos, text.galley.size());
                    shape.clip_rect.contains_rect(rect).then_some(rect.center())
                }
                _ => None,
            })
    }
    fn click(app: &mut OttrinApp, ctx: &Context, size: Vec2, point: egui::Pos2) {
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                size,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    #[test]
    fn parent_owned_folder_picker_has_visible_actions_and_updates_only_the_requested_roots() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("chosen");
        std::fs::create_dir(&selected).unwrap();
        let ctx = Context::default();
        install_application_fonts(&ctx);
        let mut app = OttrinApp::default();
        app.state.config.search.include_roots.clear();
        let exclusions = app.state.config.search.exclude_roots.clone();
        app.colors.font_scale = 1.35;
        theme_colors::apply_to_context(&ctx, &app.colors);
        let size = Vec2::new(760.0, 520.0);
        app.start_internal_picker(
            &ctx,
            PickerRequest {
                mode: PickerMode::Folder,
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            Purpose::SearchInclude,
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let output = loop {
            let output = frame(&mut app, &ctx, size, vec![]);
            if text_center(&output, "chosen").is_some() {
                break output;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        let choose =
            text_center(&output, "Choose folder").expect("folder action must fit its parent");
        assert!(text_center(&output, "Cancel").is_some());
        click(
            &mut app,
            &ctx,
            size,
            text_center(&output, "chosen").unwrap(),
        );
        click(&mut app, &ctx, size, choose);
        while app.pending_picker.is_some() {
            let output = frame(&mut app, &ctx, size, vec![]);
            assert!(
                Instant::now() < deadline,
                "{:?}",
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some((&text.galley.job.text, text.pos)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.state.config.search.include_roots, vec![selected]);
        assert_eq!(app.state.config.search.exclude_roots, exclusions);
    }
    #[test]
    fn picker_escape_cancels_without_forwarding_keys_to_the_parent_or_writing_a_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("theme.json");
        std::fs::write(&path, b"preserve original").unwrap();
        let ctx = Context::default();
        install_application_fonts(&ctx);
        let mut app = OttrinApp::default();
        app.theme_editor.open = true;
        app.start_internal_picker(
            &ctx,
            PickerRequest {
                mode: PickerMode::Save,
                initial_directory: Some(root.path().to_path_buf()),
                filename: "theme.json".into(),
                ..Default::default()
            },
            Purpose::ThemeExport("replacement".into()),
        );
        frame(&mut app, &ctx, Vec2::new(920.0, 640.0), vec![]);
        frame(
            &mut app,
            &ctx,
            Vec2::new(920.0, 640.0),
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(app.pending_picker.is_none());
        assert!(app.theme_editor.open);
        assert_eq!(std::fs::read(path).unwrap(), b"preserve original");
    }
    #[test]
    fn secondary_window_picker_keeps_the_main_window_from_consuming_its_location_shortcut() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.start_internal_picker(
            &ctx,
            PickerRequest {
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            Purpose::SearchInclude,
        );
        app.pending_picker.as_mut().unwrap().parent = OttrinApp::settings_viewport_id();
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key: Key::L,
                    physical_key: Some(Key::L),
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |ctx| app.handle_keyboard(ctx),
        );
        assert!(
            !app.any_address_bar_editing(),
            "The parent shortcut must remain available to the secondary-window picker"
        );
    }
}
