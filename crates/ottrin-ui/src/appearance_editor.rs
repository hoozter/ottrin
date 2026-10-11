use super::*;

const REGIONS: [ThemePreviewRegion; 11] = [
    ThemePreviewRegion::Defaults,
    ThemePreviewRegion::Titlebar,
    ThemePreviewRegion::Navigation,
    ThemePreviewRegion::Bookmarks,
    ThemePreviewRegion::ColumnBackground,
    ThemePreviewRegion::SelectedRow,
    ThemePreviewRegion::ColumnText,
    ThemePreviewRegion::Sidebar,
    ThemePreviewRegion::StatusBar,
    ThemePreviewRegion::Accent,
    ThemePreviewRegion::Borders,
];

impl OttrinApp {
    pub(super) fn render_appearance_settings(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        c: Colors,
    ) {
        let mut preferences_changed = false;
        ui.heading(RichText::new("Appearance").color(c.heading));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Appearance mode").color(c.text_dim));
            let mut mode = self.current_theme_mode();
            ui.add_enabled_ui(!self.theme_editor.open, |ui| {
                appearance_mode_control(ui, &mut mode, true, c);
            });
            if mode != self.current_theme_mode() {
                self.apply_theme_mode(mode, ctx);
                preferences_changed = true;
            }
        });
        ui.horizontal(|ui| {
            let active_name = self
                .state
                .config
                .active_custom_theme
                .as_deref()
                .unwrap_or(Self::theme_preset_label(self.state.config.theme_preset));
            ui.label(
                RichText::new(format!("Active · {active_name}"))
                    .small()
                    .color(c.text_dim),
            );
            if self.current_theme_mode() == ThemeMode::System {
                ui.label(
                    RichText::new(format!(
                        "Effective · {}",
                        Self::theme_mode_label(Colors::resolved_mode(
                            ThemeMode::System,
                            self.system_theme(ctx)
                        ))
                    ))
                    .small()
                    .color(c.text_dim),
                );
            }
        });
        if self.theme_editor.open {
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Theme preview in progress").color(c.accent));
                if ui
                    .add(egui::Button::new("Continue editing").themed(&c))
                    .clicked()
                {
                    self.resume_theme_editor(ctx);
                }
                if ui
                    .add(egui::Button::new("Discard preview").themed(&c))
                    .clicked()
                {
                    self.cancel_theme_editor(ctx);
                }
            });
        }
        let presets = [
            ThemePreset::Ottrin,
            ThemePreset::Breeze,
            ThemePreset::Adwaita,
            ThemePreset::Windows11,
            ThemePreset::Solarized,
            ThemePreset::Nord,
            ThemePreset::G33k,
        ];
        let columns = ((ui.available_width() / 190.0).floor() as usize).clamp(1, 4);
        let gap = 12.0;
        let width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
        let dark = Colors::resolved_mode(self.current_theme_mode(), self.system_theme(ctx))
            != ThemeMode::Light;
        ui.add_space(20.0);
        ui.label(RichText::new("Built-in themes").strong().color(c.heading));
        ui.add_space(8.0);
        let mut apply_preset = None;
        let mut edit_preset = None;
        ui.add_enabled_ui(!self.theme_editor.open, |ui| {
            for row in presets.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for &preset in row {
                        let selected = self.state.config.theme_preset == preset
                            && self.state.config.active_custom_theme.is_none();
                        let action = appearance_theme_card(
                            ui,
                            c,
                            Colors::preset(preset, dark),
                            Self::theme_preset_label(preset),
                            selected,
                            width,
                            false,
                        );
                        if action.edit {
                            edit_preset = Some(preset);
                        } else if action.apply {
                            apply_preset = Some(preset);
                        }
                    }
                });
                ui.add_space(gap);
            }
        });
        if let Some(preset) = apply_preset {
            self.apply_theme_preset(preset, ctx);
            preferences_changed = true;
        }
        if let Some(preset) = edit_preset {
            self.open_theme_editor_for_preset(preset, ctx);
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Your themes").strong().color(c.heading));
        ui.add_space(8.0);
        if self.state.config.custom_themes.is_empty() {
            ui.label(
                RichText::new("Use a theme’s pencil to create your own.")
                    .small()
                    .color(c.text_dim),
            );
        }
        let mut apply = None;
        let mut edit = None;
        let mut remove = None;
        ui.add_enabled_ui(!self.theme_editor.open, |ui| {
            for (row_index, row) in self.state.config.custom_themes.chunks(columns).enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (column, saved) in row.iter().enumerate() {
                        let index = row_index * columns + column;
                        ui.push_id(("saved_theme", index), |ui| {
                            let selected = self.state.config.active_custom_theme.as_deref()
                                == Some(saved.name.as_str());
                            let action = appearance_theme_card(
                                ui,
                                c,
                                Colors::for_saved(
                                    saved,
                                    self.current_theme_mode(),
                                    self.system_theme(ctx),
                                ),
                                &saved.name,
                                selected,
                                width,
                                true,
                            );
                            if action.edit {
                                edit = Some(index);
                            } else if action.remove {
                                remove = Some(index);
                            } else if action.apply {
                                apply = Some(index);
                            }
                        });
                    }
                });
                ui.add_space(gap);
            }
        });
        if let Some(index) = apply {
            self.apply_saved_custom_theme(&self.state.config.custom_themes[index].clone(), ctx);
            preferences_changed = true;
        }
        if let Some(index) = edit {
            self.open_theme_editor_for_saved(index, ctx);
        }
        if let Some(index) = remove {
            self.delete_saved_custom_theme(index, ctx);
            preferences_changed = true;
        }
        ui.add_space(16.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(
            RichText::new("File and folder colors")
                .strong()
                .color(c.heading),
        );
        ui.label(RichText::new("Use category colors to distinguish images, documents, archives and other file types.").small().color(c.text_dim));
        let file_colors_changed = ui
            .checkbox(
                &mut self.state.config.colorize_file_types,
                "Color files by type",
            )
            .changed();
        let folder_colors_changed = ui
            .add_enabled(
                self.state.config.colorize_file_types,
                egui::Checkbox::new(
                    &mut self.state.config.colorize_folder_labels,
                    "Also color folder labels",
                ),
            )
            .changed();
        if file_colors_changed || folder_colors_changed {
            self.reapply_theme(ctx);
            preferences_changed = true;
        }
        if preferences_changed {
            self.persist_config();
        }
        if let Some((_, _, until)) = &self.theme_status
            && Instant::now() > *until
        {
            self.theme_status = None;
        }
        if let Some((message, ok, _)) = &self.theme_status {
            ui.add_space(8.0);
            ui.label(RichText::new(message).color(if *ok { c.accent } else { c.error }));
        }
    }

    pub(super) fn render_theme_editor_embedded(&mut self, ctx: &Context) -> bool {
        let c = self.colors;
        let mut close_requested = false;
        let mut changed = false;
        let seed = self.theme_editor.initial_custom.clone();
        let creating = !matches!(self.theme_editor.source, ThemeEditorSource::Saved(_));
        let title = match self.theme_editor.source {
            ThemeEditorSource::Preset => format!(
                "Customize {}",
                Self::theme_preset_label(self.theme_editor.preset)
            ),
            ThemeEditorSource::Saved(_) => format!("Edit {}", self.theme_editor.initial_name),
            ThemeEditorSource::Imported => format!("Import {}", self.theme_editor.initial_name),
        };
        egui::CentralPanel::default().frame(Frame::new().fill(c.panel).inner_margin(16)).show(ctx, |ui| {
            render_custom_titlebar(ui, ctx, c, c.titlebar_bg, &title, true, &mut close_requested, |_| {});
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(if creating { "Theme name" } else { "Name" });
                ui.add(egui::TextEdit::singleline(&mut self.theme_editor.save_as_name).desired_width(220.0).themed(&c));
                if self.theme_editor_is_dirty() {
                    ui.label(RichText::new("Unsaved changes").small().color(c.accent));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut mode = self.theme_editor.mode;
                    appearance_mode_control(ui, &mut mode, false, c);
                    ui.label(RichText::new(if ctx.content_rect().width() < 950.0 { "Variant" } else { "Editing variant" }).small().color(c.text_dim)).on_hover_text("Edit this theme’s Light or Dark palette. The application-wide appearance preference stays unchanged.");
                    if mode != self.theme_editor.mode {
                        self.switch_theme_editor_variant(mode, ctx);
                    }
                });
            });
            ui.add_space(12.0);
            let available = ui.available_rect_before_wrap();
            let footer = Rect::from_min_max(egui::pos2(available.min.x, available.max.y - 56.0), available.max);
            let content = Rect::from_min_max(available.min, egui::pos2(available.max.x, footer.min.y - 12.0));
            let split = content.left() + content.width() * 0.58;
            let preview_rect = Rect::from_min_max(content.min, egui::pos2(split - 10.0, content.bottom()));
            let inspector_rect = Rect::from_min_max(egui::pos2(split + 10.0, content.top()), content.max);
            ui.scope_builder(UiBuilder::new().max_rect(preview_rect), |ui| {
                ui.set_min_size(preview_rect.size());
                let preview = self.theme_editor_preview_colors();
                if let Some(region) = theme_editor_large_preview(ui, preview,
                    self.theme_editor.selected_region).1 {
                    self.theme_editor.selected_region = region;
                }
            });
            ui.scope_builder(UiBuilder::new().max_rect(inspector_rect), |ui| {
                ui.set_width(inspector_rect.width());
                let mut region = self.theme_editor.selected_region;
                egui::ComboBox::from_id_salt("appearance_component").width(inspector_rect.width())
                    .selected_text(Self::theme_preview_region_label(region)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut region, ThemePreviewRegion::Defaults, "Theme defaults");
                        ui.separator();
                        for component in REGIONS.into_iter().filter(|r| *r != ThemePreviewRegion::Defaults) {
                            ui.selectable_value(&mut region, component, Self::theme_preview_region_label(component));
                        }
                    }).response.on_hover_text(Self::theme_preview_region_hint(region));
                self.theme_editor.selected_region = region;
                ui.add_space(8.0);
                ScrollArea::vertical().id_salt("appearance_properties").max_height((inspector_rect.height()-44.0).max(100.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 10.0;
                    ui.label(RichText::new(Self::theme_preview_region_hint(region)).small().color(c.text_dim));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Colors").strong().color(c.heading));
                    let fields = Self::theme_preview_region_fields(region);
                    for &field in fields { changed |= self.appearance_color_row(ui, field, &seed); }
                    if region == ThemePreviewRegion::Defaults {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Typography").strong().color(c.heading));
                        changed |= number_row(ui, "Text size", &mut self.theme_editor.custom.font_scale, 0.85..=1.35, seed.font_scale, 0.01, c);
                        ui.add_space(10.0);
                        ui.label(RichText::new("Geometry").strong().color(c.heading));
                        changed |= number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                        changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                    }
                    if region == ThemePreviewRegion::Accent {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Geometry").strong().color(c.heading));
                        changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                    }
                    if region == ThemePreviewRegion::Borders {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Geometry").strong().color(c.heading));
                        changed |= number_row(ui, "Window border thickness", &mut self.theme_editor.custom.window_border_width, 0.0..=4.0, seed.window_border_width, 0.5, c);
                    }
                    if region == ThemePreviewRegion::ColumnText {
                        ui.add_space(10.0);
                        ui.label(RichText::new("Category colors").strong().color(c.heading));
                        changed |= number_row(ui, "Neutral text blend", &mut self.theme_editor.custom.file_label_blend, 0.0..=1.0, seed.file_label_blend, 0.01, c);
                        for field in [ThemeColorField::PaletteSoftWhite, ThemeColorField::PaletteFolderBlue, ThemeColorField::PaletteLinkBlue, ThemeColorField::PaletteSteel,
                            ThemeColorField::PaletteGreen, ThemeColorField::PaletteOrange, ThemeColorField::PalettePurple, ThemeColorField::PalettePink, ThemeColorField::PaletteRed, ThemeColorField::PaletteYellow] {
                            changed |= self.appearance_color_row(ui, field, &seed);
                        }
                    }
                    ui.add_space(10.0);
                    if ui.add(egui::Button::new(if region == ThemePreviewRegion::Defaults { "Reset theme defaults" } else { "Reset component" }).themed(&c))
                        .on_hover_text(if region == ThemePreviewRegion::Defaults { "Restore the shared defaults shown above. Components using them update together; component-specific colors and the other variant are unchanged." } else { "Restore the component-specific values above. Shared styling below and the other variant are unchanged." }).clicked() {
                        for &field in fields { *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field) = Self::theme_color_field_value(&seed, field); }
                        match region {
                            ThemePreviewRegion::Defaults => { self.theme_editor.custom.font_scale=seed.font_scale; self.theme_editor.custom.app_radius=seed.app_radius; self.theme_editor.custom.border_radius=seed.border_radius; }
                            ThemePreviewRegion::Accent => self.theme_editor.custom.button_radius=seed.button_radius,
                            ThemePreviewRegion::Borders => self.theme_editor.custom.window_border_width=seed.window_border_width,
                            ThemePreviewRegion::ColumnText => { self.theme_editor.custom.file_label_blend=seed.file_label_blend; for field in Self::theme_color_field_order().into_iter().filter(|f| matches!(f, ThemeColorField::PaletteSoftWhite|ThemeColorField::PaletteFolderBlue|ThemeColorField::PaletteLinkBlue|ThemeColorField::PaletteSteel|ThemeColorField::PaletteGreen|ThemeColorField::PaletteOrange|ThemeColorField::PalettePurple|ThemeColorField::PalettePink|ThemeColorField::PaletteRed|ThemeColorField::PaletteYellow)) { *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field)=Self::theme_color_field_value(&seed, field); } },
                            _ => {}
                        }
                        self.sync_theme_editor_hex_inputs(); changed=true;
                    }
                    if region != ThemePreviewRegion::Defaults {
                        ui.add_space(14.0);
                        ui.separator();
                        ui.label(RichText::new("Shared styling").strong().color(c.heading));
                        ui.label(RichText::new("Also affects other components. Reset component leaves these values unchanged.").small().color(c.text_dim));
                        for &field in Self::theme_preview_region_shared_fields(region) {
                            changed |= self.appearance_color_row(ui, field, &seed);
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("Typography").strong().color(c.heading));
                        changed |= number_row(ui, "Text size", &mut self.theme_editor.custom.font_scale, 0.85..=1.35, seed.font_scale, 0.01, c);
                        if matches!(region, ThemePreviewRegion::Navigation | ThemePreviewRegion::Titlebar | ThemePreviewRegion::Borders | ThemePreviewRegion::Bookmarks) {
                            ui.add_space(6.0);
                            ui.label(RichText::new("Geometry").strong().color(c.heading));
                            match region {
                                ThemePreviewRegion::Navigation => {
                                    ui.label(RichText::new("Surface corners: tabs, text inputs and cards.").small().color(c.text_dim));
                                    changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                }
                                ThemePreviewRegion::Bookmarks => {
                                    ui.label(RichText::new("Surface corners: expanded location cards, tabs and inputs.").small().color(c.text_dim));
                                    changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                    ui.label(RichText::new("Button corners: collapsed location icons and other buttons.").small().color(c.text_dim));
                                    changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                                }
                                _ => {
                                    ui.label(RichText::new("Window corners: main window, Settings, editor and Inspector.").small().color(c.text_dim));
                                    changed |= number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                                }
                            }
                        }
                    }
                });
            });
            ui.scope_builder(UiBuilder::new().max_rect(footer), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    themed_menu_button(ui, "More", c, |ui| {
                        if ui.add_enabled(!self.theme_editor_is_dirty(), egui::Button::new("Import theme…").themed(&c)).on_hover_text("Import into an unchanged draft. Save or discard your changes first.").clicked() { self.import_theme_editor_json(ctx); ui.close(); }
                        if ui.add(egui::Button::new("Export theme…").themed(&c)).clicked() { self.export_theme_editor_json(); ui.close(); }
                        if ui.add(egui::Button::new("Reset theme to starting values").themed(&c)).on_hover_text("Restore all properties in both Light and Dark variants to their values when this editing session opened.").clicked() {
                            self.theme_editor.custom=seed.clone(); self.theme_editor.other_variant=self.theme_editor.initial_other_variant.clone(); self.sync_theme_editor_hex_inputs(); changed=true; ui.close();
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new(if creating { "Create theme" } else { "Save changes" }).strong()).stroke(Stroke::new(1.0, c.accent)).themed(&c)) .clicked() {
                            self.save_theme_editor(ctx); self.persist_config();
                        }
                        if !creating && ui.add(egui::Button::new("Save as new").themed(&c)).clicked() {
                            self.save_theme_editor_as_inner(ctx); self.persist_config();
                        }
                        if ui.add(egui::Button::new("Cancel").themed(&c)).on_hover_text("Discard this session and restore the previously active theme.").clicked() { self.cancel_theme_editor(ctx); }
                    });
                });
                if let Some((message, ok, _)) = &self.theme_status {
                    ui.label(RichText::new(message).small().color(if *ok { c.accent } else { c.error }));
                }
            });
        });
        if changed {
            self.apply_theme_editor_live_preview(ctx);
        }
        close_requested
    }

    pub(super) fn render_theme_exit_confirmation(&mut self, ctx: &Context) {
        if self.theme_editor.pending_exit {
            egui::Window::new("Unsaved theme changes")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Save the theme before closing Ottrin?");
                    if let Some((message, false, expiry)) = &self.theme_status
                        && Instant::now() < *expiry
                    {
                        ui.colored_label(self.colors.error, message);
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new("Save and close").themed(&self.colors))
                            .clicked()
                        {
                            self.save_theme_editor(ctx);
                            if !self.theme_editor.open {
                                self.persist_config();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if ui
                            .add(egui::Button::new("Discard and close").themed(&self.colors))
                            .clicked()
                        {
                            self.cancel_theme_editor(ctx);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui
                            .add(egui::Button::new("Keep editing").themed(&self.colors))
                            .clicked()
                        {
                            self.resume_theme_editor(ctx);
                        }
                    });
                });
        }
    }

    fn appearance_color_row(
        &mut self,
        ui: &mut egui::Ui,
        field: ThemeColorField,
        seed: &ThemeCustomization,
    ) -> bool {
        let mut changed = false;
        ui.push_id(field, |ui| {
            let rgba = Self::theme_color_field_value(&self.theme_editor.custom, field);
            let mut hex = self
                .theme_editor
                .color_hex_inputs
                .get(&field)
                .cloned()
                .unwrap_or_else(|| format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2]));
            ui.horizontal(|ui| {
                let label_width = (ui.available_width() - (28.0 * self.colors.font_scale + 88.0 * self.colors.font_scale + 28.0 + ui.spacing().item_spacing.x * 3.0)).clamp(60.0, 200.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(label_width, 28.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_width(label_width);
                        ui.add(egui::Label::new(Self::theme_color_field_label(field)).wrap())
                            .on_hover_text(Self::theme_color_field_hint(field));
                    },
                );
                let mut color = rgba;
                if compact_color_picker(ui, &mut color, self.colors).changed() {
                    *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field) =
                        color;
                    hex = format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
                    self.theme_editor
                        .color_hex_inputs
                        .insert(field, hex.clone());
                    changed = true;
                }
                let valid_hex = parse_hex_rgb(&hex).is_some();
                if ui
                    .add_sized([88.0 * self.colors.font_scale, 28.0],
                        egui::TextEdit::singleline(&mut hex)
                            .id(egui::Id::new(("appearance_hex", field)))
                            .desired_width(88.0 * self.colors.font_scale)
                            .text_color(if valid_hex { self.colors.text } else { self.colors.error }).themed(&self.colors),
                    )
                    .on_hover_text(if valid_hex {
                        "RGB color: three or six hexadecimal digits. Use the color picker to change opacity."
                    } else {
                        "Enter three or six hexadecimal digits, such as #369 or #336699. The previous color stays applied until this entry is valid."
                    })
                    .changed()
                {
                    self.theme_editor
                        .color_hex_inputs
                        .insert(field, hex.clone());
                    if let Some(rgb) = parse_hex_rgb(&hex) {
                        *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field) =
                            [rgb[0], rgb[1], rgb[2], rgba[3]];
                        changed = true;
                    }
                }
                if appearance_icon_button(ui, MI_SETTINGS_BACKUP_RESTORE, self.colors)
                    .on_hover_text("Restore this color to its value when the editor opened")
                    .clicked()
                {
                    *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field) =
                        Self::theme_color_field_value(seed, field);
                    let color = Self::theme_color_field_value(seed, field);
                    self.theme_editor.color_hex_inputs.insert(
                        field,
                        format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]),
                    );
                    changed = true;
                }

            });
        });
        changed
    }
}

/// A compact, labelled color control. RGBA bytes remain authoritative; only
/// the hue of a gray color is remembered as transient picker interaction state.
fn compact_color_picker(ui: &mut egui::Ui, rgba: &mut [u8; 4], c: Colors) -> egui::Response {
    use egui::ecolor::{Hsva, HsvaGamma};
    let popup_id = ui.make_persistent_id("appearance_color_picker");
    let hue_id = popup_id.with("gray_hue");
    let (swatch, response) =
        ui.allocate_exact_size(Vec2::new(28.0 * c.font_scale, 22.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::ColorButton));
    egui::widgets::color_picker::show_color_at(
        ui.painter(),
        Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
        swatch.shrink(1.0),
    );
    ui.painter().rect_stroke(
        swatch,
        2.0,
        Stroke::new(1.0, c.border),
        egui::StrokeKind::Inside,
    );
    let mut response = response.on_hover_text(format!(
        "Choose color · opacity {:.0}%",
        f32::from(rgba[3]) * 100.0 / 255.0
    ));
    egui::Popup::menu(&response)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 130.0 * c.font_scale;
            ui.label(RichText::new("Color").strong().color(c.heading));
            // Derive HSV from opaque RGB so reducing opacity to zero never
            // erases the color. Opacity is edited independently below.
            let mut hsv = HsvaGamma::from(Hsva::from_srgba_unmultiplied([
                rgba[0], rgba[1], rgba[2], 255,
            ]));
            if hsv.s == 0.0 || hsv.v == 0.0 {
                hsv.h = ui
                    .ctx()
                    .data_mut(|data| data.get_temp(hue_id))
                    .unwrap_or(0.0);
            }
            let side = 180.0 * c.font_scale;
            let (plane, plane_response) =
                ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
            let mut hsv_changed = false;
            if let Some(pos) = plane_response.interact_pointer_pos() {
                hsv.s = ((pos.x - plane.left()) / plane.width()).clamp(0.0, 1.0);
                hsv.v = ((plane.bottom() - pos.y) / plane.height()).clamp(0.0, 1.0);
                hsv_changed = true;
            }
            let mut mesh = egui::epaint::Mesh::default();
            const STEPS: u32 = 24;
            for y in 0..=STEPS {
                for x in 0..=STEPS {
                    let s = x as f32 / STEPS as f32;
                    let v = 1.0 - y as f32 / STEPS as f32;
                    mesh.colored_vertex(
                        egui::pos2(plane.left() + s * side, plane.top() + (1.0 - v) * side),
                        Color32::from(HsvaGamma {
                            s,
                            v,
                            a: 1.0,
                            ..hsv
                        }),
                    );
                    if x < STEPS && y < STEPS {
                        let top = y * (STEPS + 1) + x;
                        mesh.add_triangle(top, top + 1, top + STEPS + 1);
                        mesh.add_triangle(top + 1, top + STEPS + 2, top + STEPS + 1);
                    }
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
            let point = egui::pos2(plane.left() + hsv.s * side, plane.bottom() - hsv.v * side);
            ui.painter()
                .circle_stroke(point, 5.0, Stroke::new(2.0, Color32::BLACK));
            ui.painter()
                .circle_stroke(point, 4.0, Stroke::new(1.5, Color32::WHITE));
            plane_response.on_hover_text("Saturation and brightness");
            ui.label(RichText::new("Hue").small().color(c.text_dim));
            let (bar, bar_response) = ui.allocate_exact_size(
                Vec2::new(side, 14.0 * c.font_scale),
                Sense::click_and_drag(),
            );
            if let Some(pos) = bar_response.interact_pointer_pos() {
                hsv.h = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                hsv_changed = true;
            }
            let mut mesh = egui::epaint::Mesh::default();
            for i in 0..=STEPS {
                let h = i as f32 / STEPS as f32;
                let color = Color32::from(HsvaGamma {
                    h,
                    s: 1.0,
                    v: 1.0,
                    a: 1.0,
                });
                let x = bar.left() + h * side;
                mesh.colored_vertex(egui::pos2(x, bar.top()), color);
                mesh.colored_vertex(egui::pos2(x, bar.bottom()), color);
                if i < STEPS {
                    mesh.add_triangle(i * 2, i * 2 + 1, i * 2 + 2);
                    mesh.add_triangle(i * 2 + 1, i * 2 + 2, i * 2 + 3);
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
            let marker = Rect::from_center_size(
                egui::pos2(bar.left() + hsv.h * side, bar.center().y),
                Vec2::new(4.0, bar.height() + 4.0),
            );
            ui.painter().rect_stroke(
                marker,
                1.0,
                Stroke::new(2.5, Color32::BLACK),
                egui::StrokeKind::Outside,
            );
            ui.painter().rect_stroke(
                marker,
                1.0,
                Stroke::new(1.5, Color32::WHITE),
                egui::StrokeKind::Outside,
            );
            if hsv_changed {
                let rgb = Hsva::from(hsv).to_srgba_unmultiplied();
                rgba[..3].copy_from_slice(&rgb[..3]);
                response.mark_changed();
            }
            ui.ctx().data_mut(|data| data.insert_temp(hue_id, hsv.h));
            ui.add_space(6.0);
            for (index, label) in [(0, "Red"), (1, "Green"), (2, "Blue")] {
                ui.horizontal(|ui| {
                    ui.add_sized([60.0 * c.font_scale, 22.0], egui::Label::new(label));
                    if ui
                        .add(egui::Slider::new(&mut rgba[index], 0..=255))
                        .changed()
                    {
                        response.mark_changed();
                    }
                });
            }
            let mut opacity = rgba[3] as f32 * 100.0 / 255.0;
            ui.horizontal(|ui| {
                ui.add_sized([60.0 * c.font_scale, 22.0], egui::Label::new("Opacity"));
                if ui
                    .add(
                        egui::Slider::new(&mut opacity, 0.0..=100.0)
                            .step_by(1.0)
                            .suffix("%"),
                    )
                    .changed()
                {
                    rgba[3] = (opacity * 255.0 / 100.0).round() as u8;
                    response.mark_changed();
                }
            });
        });
    response
}

fn number_row(
    ui: &mut egui::Ui,
    title: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    step: f64,
    c: Colors,
) -> bool {
    let mut changed = false;
    ui.push_id(title, |ui| {
        ui.horizontal(|ui| {
            let value_width = 28.0 * c.font_scale + ui.spacing().item_spacing.x + 88.0;
            let label_width = (ui.available_width()
                - (value_width + 28.0 + ui.spacing().item_spacing.x * 2.0))
                .clamp(60.0, 200.0);
            ui.allocate_ui_with_layout(
                Vec2::new(label_width, 28.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(label_width);
                    ui.add(egui::Label::new(title).wrap());
                },
            );
            changed |= ui
                .add_sized(
                    [value_width, 28.0],
                    egui::DragValue::new(value)
                        .range(range)
                        .speed(step)
                        .max_decimals(if step < 0.1 {
                            2
                        } else if step < 1.0 {
                            1
                        } else {
                            0
                        })
                        .suffix(match title {
                            "Text size" => "×",
                            "Neutral text blend" => "",
                            _ => " px",
                        }),
                )
                .on_hover_text(if title == "Neutral text blend" { "Blend category-colored labels toward neutral text: 0 is the category color, 1 is neutral file/folder text. Neutral labels are unchanged." } else { "Drag or type a value" })
                .changed();
            if appearance_icon_button(ui, MI_SETTINGS_BACKUP_RESTORE, c)
                .on_hover_text("Restore this value to its value when the editor opened")
                .clicked()
            {
                *value = default;
                changed = true;
            }
        });
    });
    changed
}

fn appearance_icon_button(ui: &mut egui::Ui, icon: &str, c: Colors) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::splat(4.0);
        ui.add_sized(
            [28.0, 28.0],
            egui::Button::new(icon)
                .frame_when_inactive(false)
                .themed(&c),
        )
    })
    .inner
}

#[derive(Default)]
struct ThemeCardAction {
    apply: bool,
    edit: bool,
    remove: bool,
}

fn theme_miniature(ui: &egui::Ui, thumb: Rect, preview: Colors) {
    let p = ui.painter();
    p.rect_filled(thumb, preview.app_radius, preview.bg);
    let title = Rect::from_min_size(thumb.min, Vec2::new(thumb.width(), 10.0));
    p.rect_filled(title, preview.app_radius, preview.titlebar_bg);
    let nav = Rect::from_min_size(
        egui::pos2(thumb.left(), title.bottom()),
        Vec2::new(thumb.width(), 9.0),
    );
    p.rect_filled(nav, 0.0, preview.toolbar_bg);
    let action = Rect::from_min_max(
        nav.right_top() + Vec2::new(-22.0, 1.0),
        nav.right_bottom() + Vec2::new(-3.0, -1.0),
    );
    let action_radius = (preview.button_radius as f32 * action.height() / 28.0).round() as u8;
    p.rect_filled(action, action_radius, preview.button_bg);
    p.rect_stroke(
        action,
        action_radius,
        Stroke::new(1.0, preview.accent),
        egui::StrokeKind::Inside,
    );
    p.line_segment(
        [
            action.center() - Vec2::new(4.0, 0.0),
            action.center() + Vec2::new(4.0, 0.0),
        ],
        Stroke::new(preview.font_scale, preview.button_text),
    );
    let content = Rect::from_min_max(
        egui::pos2(thumb.left(), nav.bottom()),
        thumb.max - Vec2::new(0.0, 7.0),
    );
    let rail = Rect::from_min_size(content.min, Vec2::new(13.0, content.height()));
    p.rect_filled(rail, 0.0, preview.bookmarks_bg);
    let sidebar = Rect::from_min_max(content.max - Vec2::new(11.0, content.height()), content.max);
    p.rect_filled(sidebar, 0.0, preview.smart_panel_bg);
    let body = Rect::from_min_max(
        egui::pos2(rail.right() + 4.0, content.top() + 4.0),
        egui::pos2(sidebar.left() - 4.0, content.bottom() - 3.0),
    );
    for row in 0..3 {
        let row_rect = Rect::from_min_size(
            body.min + Vec2::new(0.0, row as f32 * 10.0),
            Vec2::new(body.width(), 8.0),
        );
        if row == 1 {
            p.rect_filled(row_rect, preview.border_radius, preview.selected_bg);
        }
        p.rect_filled(
            Rect::from_min_size(row_rect.min + Vec2::new(3.0, 2.0), Vec2::new(5.0, 4.0)),
            preview.border_radius.min(2),
            preview.folder,
        );
        p.line_segment(
            [
                row_rect.min + Vec2::new(12.0, 4.0),
                row_rect.min + Vec2::new(body.width() * 0.65, 4.0),
            ],
            Stroke::new(preview.font_scale, preview.folder_text),
        );
    }
    p.rect_filled(
        Rect::from_min_max(egui::pos2(thumb.left(), content.bottom()), thumb.max),
        0.0,
        preview.statusbar_bg,
    );
    p.rect_stroke(
        thumb,
        preview.app_radius,
        Stroke::new(preview.window_border_width, preview.window_border),
        egui::StrokeKind::Inside,
    );
}

fn appearance_theme_card(
    ui: &mut egui::Ui,
    c: Colors,
    preview: Colors,
    name: &str,
    selected: bool,
    width: f32,
    custom: bool,
) -> ThemeCardAction {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 138.0), Sense::click());
    let response = response
        .on_hover_text(name)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    let p = ui.painter();
    p.rect_filled(
        rect,
        c.border_radius,
        if response.hovered() {
            c.panel_raised
        } else {
            c.panel
        },
    );
    p.rect_stroke(
        rect,
        c.border_radius,
        Stroke::new(
            if selected || response.has_focus() {
                2.0
            } else {
                1.0
            },
            if selected || response.has_focus() {
                c.accent
            } else {
                c.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let thumb = Rect::from_min_max(rect.min + Vec2::splat(8.0), rect.max - Vec2::new(8.0, 42.0));
    theme_miniature(ui, thumb, preview);
    let label_width = (width - if custom { 86.0 } else { 58.0 }).max(50.0);
    let mut label_job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        FontId::proportional(12.0 * c.font_scale),
        c.text,
    );
    label_job.wrap.max_width = label_width;
    label_job.wrap.max_rows = 1;
    let label = p.layout_job(label_job);
    let label_y = rect.bottom() - 21.0 - label.size().y / 2.0;
    p.galley(egui::pos2(rect.left() + 10.0, label_y), label, c.text);
    if selected {
        p.circle_filled(thumb.right_top() + Vec2::new(-9.0, 9.0), 4.0, c.accent);
    }
    let edit_rect = Rect::from_min_size(
        egui::pos2(
            rect.right() - if custom { 66.0 } else { 36.0 },
            rect.bottom() - 35.0,
        ),
        Vec2::splat(28.0),
    );
    let edit_response = ui
        .scope_builder(UiBuilder::new().max_rect(edit_rect), |ui| {
            appearance_icon_button(ui, MI_EDIT, c)
        })
        .inner
        .on_hover_text(if custom {
            "Edit theme"
        } else {
            "Customize theme"
        });
    edit_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("{} {name}", if custom { "Edit" } else { "Customize" }),
        )
    });
    let edit = edit_response.clicked();
    let remove = custom
        && ui
            .put(
                Rect::from_min_size(
                    egui::pos2(rect.right() - 34.0, rect.bottom() - 35.0),
                    Vec2::splat(28.0),
                ),
                egui::Button::new("×").themed(&c),
            )
            .on_hover_text("Delete custom theme")
            .clicked();
    ThemeCardAction {
        apply: response.clicked() && !edit && !remove,
        edit,
        remove,
    }
}

fn appearance_mode_control(
    ui: &mut egui::Ui,
    mode: &mut ThemeMode,
    include_system: bool,
    c: Colors,
) {
    let options = [
        (ThemeMode::Light, "Light"),
        (ThemeMode::Dark, "Dark"),
        (ThemeMode::System, "System"),
    ];
    Frame::new()
        .fill(c.panel_raised)
        .corner_radius(c.button_radius)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (value, label) in options {
                    if value == ThemeMode::System && !include_system {
                        continue;
                    }
                    let selected = *mode == value;
                    let response = ui.add_sized(
                        [66.0 * c.font_scale, 28.0],
                        egui::Button::new(RichText::new(label).color(if selected {
                            c.button_text
                        } else {
                            c.text_dim
                        }))
                        .fill(if selected {
                            c.button_bg
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(c.button_radius),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::RadioButton,
                            ui.is_enabled(),
                            selected,
                            label,
                        )
                    });
                    if selected || response.has_focus() {
                        ui.painter().line_segment(
                            [
                                response.rect.left_bottom() + Vec2::new(8.0, -2.0),
                                response.rect.right_bottom() + Vec2::new(-8.0, -2.0),
                            ],
                            Stroke::new(2.0, c.accent),
                        );
                    }
                    if response.clicked() {
                        *mode = value;
                    }
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_preview_selection_matches_painted_row_and_square_canvas() {
        let ctx = Context::default();
        let colors = Colors::preset(ThemePreset::Ottrin, true);
        let render = |region, events: Vec<egui::Event>| {
            let mut clicked = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        clicked = theme_editor_large_preview(ui, colors, region).1;
                    });
                },
            );
            (output, clicked)
        };
        let (output, _) = render(ThemePreviewRegion::SelectedRow, vec![]);
        let painted = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(r)
                    if r.fill == colors.selected_bg && r.rect.width() > 100.0 =>
                {
                    Some(r)
                }
                _ => None,
            })
            .expect("A selected file row is painted");
        let outline = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(r)
                    if r.stroke.color == colors.accent && r.stroke.width == 2.0 =>
                {
                    Some(r)
                }
                _ => None,
            })
            .expect("Selected-row editor outline");
        assert_eq!(
            outline.rect, painted.rect,
            "Editor target must exactly follow the painted row"
        );
        assert_eq!(painted.corner_radius, egui::CornerRadius::ZERO);
        assert_eq!(outline.corner_radius, painted.corner_radius);
        let click = |pos| {
            render(
                ThemePreviewRegion::SelectedRow,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            render(
                ThemePreviewRegion::SelectedRow,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            )
            .1
        };
        assert_eq!(
            click(painted.rect.center()),
            Some(ThemePreviewRegion::SelectedRow)
        );
        assert_ne!(
            click(painted.rect.center_bottom() + Vec2::new(0.0, 3.0)),
            Some(ThemePreviewRegion::SelectedRow),
            "Gap below the row must not pick Selected row"
        );
        let (output, _) = render(ThemePreviewRegion::ColumnBackground, vec![]);
        let outline = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(r)
                    if r.stroke.color == colors.accent && r.stroke.width == 2.0 =>
                {
                    Some(r)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            outline.corner_radius,
            egui::CornerRadius::ZERO,
            "The file canvas has no radius setting"
        );
    }

    #[test]
    fn appearance_tabs_share_actual_fill_geometry_and_inspector_values() {
        let ctx = Context::default();
        let mut colors = Colors::preset(ThemePreset::Ottrin, true);
        colors.toolbar_bg = Color32::from_rgb(191, 28, 73);
        colors.titlebar_bg = Color32::from_rgb(13, 33, 51);
        colors.border_radius = 14;
        colors.text_muted = Color32::from_rgb(111, 232, 149);
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                theme_editor_large_preview(ui, colors, ThemePreviewRegion::Navigation);
            });
        });
        let tab = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Rect(r) if r.fill == theme_tab_fill(colors) => Some(r),
                _ => None,
            })
            .expect("Preview represents actual tab fill");
        assert_eq!(tab.corner_radius, theme_tab_corners(colors));
        assert_eq!(tab.corner_radius.sw, 0);
        assert_eq!(tab.corner_radius.se, 0);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == "workspace" && t.fallback_color == colors.text_muted)));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Rect(r) if r.rect == tab.rect && r.stroke.width == 2.0 && r.stroke.color == colors.accent)), "Choosing Navigation & tabs also outlines the tab");
        let fields = OttrinApp::theme_preview_region_shared_fields(ThemePreviewRegion::Navigation);
        for field in [
            ThemeColorField::Titlebar,
            ThemeColorField::RaisedPanel,
            ThemeColorField::TextMuted,
            ThemeColorField::TextDim,
            ThemeColorField::Accent,
        ] {
            assert!(fields.contains(&field));
        }
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
        app.theme_editor.selected_region = ThemePreviewRegion::Navigation;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 1300.0),
                )),
                ..Default::default()
            },
            |ctx| {
                app.render_theme_editor_embedded(ctx);
            },
        );
        for label in [
            "Shared styling",
            "Surface corners",
            "Text size",
            "Muted text",
            "Title bar",
            "Active & focus accent",
        ] {
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == label)), "Missing contextual shared control {label}");
        }
    }

    #[test]
    fn appearance_miniature_reflects_button_and_accent_edits() {
        let ctx = Context::default();
        for radius in [0, 14] {
            let mut colors = Colors::preset(ThemePreset::Nord, true);
            colors.button_bg = Color32::from_rgb(173, 23, 89);
            colors.accent = Color32::from_rgb(11, 211, 137);
            colors.button_radius = radius;
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    theme_miniature(
                        ui,
                        Rect::from_min_size(egui::pos2(20.0, 20.0), Vec2::new(180.0, 88.0)),
                        colors,
                    );
                });
            });
            let action = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect) if rect.fill == colors.button_bg => Some(rect),
                    _ => None,
                })
                .expect("The library thumbnail must represent button styling");
            assert_eq!(action.corner_radius.nw, if radius == 0 { 0 } else { 4 });
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Rect(rect) if rect.stroke.color == colors.accent)));
        }
    }

    #[test]
    fn appearance_preview_elides_long_names_at_enlarged_text_size() {
        let ctx = Context::default();
        let mut colors = Colors::preset(ThemePreset::Nord, true);
        colors.font_scale = 1.35;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(440.0, 520.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    theme_editor_large_preview(ui, colors, ThemePreviewRegion::ColumnText);
                });
            },
        );
        let label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(text) if text.galley.job.text == "Documents" => {
                    Some(text)
                }
                _ => None,
            })
            .expect("Preview contains a representative long folder name");
        assert!(
            label.galley.elided,
            "Long names must show an ellipsis instead of being clipped"
        );
        assert_eq!(label.galley.rows.len(), 1, "File rows remain single-line");
        assert!(label.galley.size().x <= label.galley.job.wrap.max_width + 1.0);
    }

    #[test]
    fn appearance_component_selection_does_not_change_preview_ink() {
        let c = Colors::preset(ThemePreset::Nord, true);
        let mut previous = None;
        for region in [
            ThemePreviewRegion::Defaults,
            ThemePreviewRegion::ColumnText,
            ThemePreviewRegion::ColumnBackground,
        ] {
            let ctx = Context::default();
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(700.0, 650.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        theme_editor_large_preview(ui, c, region);
                    });
                },
            );
            let ink: Vec<_> = output
                .shapes
                .into_iter()
                .filter_map(|s| {
                    if let egui::epaint::Shape::Text(t) = s.shape {
                        Some((t.galley.job.text.clone(), t.fallback_color))
                    } else {
                        None
                    }
                })
                .collect();
            if let Some(expected) = previous {
                assert_eq!(
                    ink, expected,
                    "Selecting a component must only change its highlight and inspector"
                );
            }
            previous = Some(ink);
        }
    }

    #[test]
    fn appearance_imported_draft_remains_unsaved_until_create_or_cancel() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
        let saved_count = app.state.config.custom_themes.len();
        app.theme_editor.source = ThemeEditorSource::Imported;
        app.close_theme_editor_window(&ctx);
        assert!(app.theme_editor.open && !app.theme_editor.visible);
        assert!(app.theme_editor_is_dirty());
        assert_eq!(app.state.config.custom_themes.len(), saved_count);
        app.cancel_theme_editor(&ctx);
        assert!(!app.theme_editor.open);
        assert_eq!(app.state.config.custom_themes.len(), saved_count);
    }

    #[test]
    fn appearance_inspector_exposes_all_properties_and_keeps_rows_aligned() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Nord, &ctx);
        let mut labels = std::collections::BTreeSet::new();
        for region in REGIONS {
            app.theme_editor.selected_region = region;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 1100.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            );
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(text) = shape.shape {
                    labels.insert(text.galley.job.text.clone());
                }
            }
        }
        for field in OttrinApp::theme_color_field_order() {
            assert!(
                labels.contains(OttrinApp::theme_color_field_label(field)),
                "Missing color control {field:?}"
            );
        }
        for title in [
            "Text size",
            "Window corners",
            "Surface corners",
            "Button corners",
            "Window border thickness",
            "Neutral text blend",
        ] {
            assert!(labels.contains(title), "Missing numeric control {title}");
        }
        for region in REGIONS {
            app.theme_editor.selected_region = region;
            for scale in [1.0, 1.35] {
                app.theme_editor.custom.font_scale = scale;
                app.apply_theme_editor_live_preview(&ctx);
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(800.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        app.render_theme_editor_embedded(ctx);
                    },
                );
                let resets: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| {
                        if let egui::epaint::Shape::Text(t) = &s.shape {
                            (t.galley.job.text == MI_SETTINGS_BACKUP_RESTORE).then_some((
                                t.pos.x,
                                t.pos.y,
                                t.pos.x + t.galley.size().x,
                            ))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(
                    !resets.is_empty(),
                    "Inspector must show usable rows at the minimum size: {region:?}"
                );
                let x = resets[0].0;
                for (left, _, right) in resets {
                    assert!(
                        (left - x).abs() < 1.0,
                        "Reset column drifts at font scale {scale}"
                    );
                    assert!(right < 784.0, "Reset target clips at font scale {scale}");
                }
            }
        }
    }

    #[test]
    fn appearance_variants_are_independent_and_global_preference_survives() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.state.config.theme = ThemeMode::Light;
        app.open_theme_editor_for_preset(ThemePreset::Nord, &ctx);
        let original_dark = app.theme_editor.other_variant.clone();
        app.theme_editor.custom.button_radius = 11.0;
        app.switch_theme_editor_variant(ThemeMode::Dark, &ctx);
        assert_eq!(app.theme_editor.custom, original_dark);
        app.theme_editor.custom.background = [13, 17, 23, 255];
        app.switch_theme_editor_variant(ThemeMode::Light, &ctx);
        assert_eq!(app.theme_editor.custom.button_radius, 11.0);
        assert_eq!(app.state.config.theme, ThemeMode::Light);
        app.save_theme_editor(&ctx);
        let saved = app.state.config.custom_themes.last().unwrap();
        assert_eq!(saved.light.button_radius, 11.0);
        assert_eq!(saved.dark.background, [13, 17, 23, 255]);
        let owned = saved.clone();
        app.open_theme_editor_for_saved(app.state.config.custom_themes.len() - 1, &ctx);
        app.theme_editor.custom.background = [255, 0, 100, 255];
        app.apply_theme_editor_live_preview(&ctx);
        app.close_theme_editor_window(&ctx);
        assert!(app.theme_editor.open && !app.theme_editor.visible);
        assert_eq!(app.state.config.custom_themes.last().unwrap(), &owned);
        assert_eq!(app.colors.bg, Colors::from_rgba([255, 0, 100, 255]));
        app.theme_editor.pending_exit = true;
        app.resume_theme_editor(&ctx);
        assert!(app.theme_editor.visible && !app.theme_editor.pending_exit);
        assert_eq!(app.theme_editor.custom.background, [255, 0, 100, 255]);
        app.cancel_theme_editor(&ctx);
        assert_eq!(
            app.colors,
            Colors::for_saved(&owned, ThemeMode::Light, None)
        );
        assert!(!app.theme_editor.open);
    }

    #[test]
    fn appearance_card_pencil_and_body_are_independent_click_targets() {
        for custom in [false, true] {
            for edit in [false, true] {
                let ctx = Context::default();
                let c = Colors::preset(ThemePreset::Ottrin, true);
                let pos = if edit {
                    egui::pos2(if custom { 188.0 } else { 218.0 }, 117.0)
                } else {
                    egui::pos2(40.0, 40.0)
                };
                let mut action = ThemeCardAction::default();
                let mut frame = |events| {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(400.0, 250.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .frame(Frame::new())
                                .show(ctx, |ui| {
                                    action = appearance_theme_card(
                                        ui, c, c, "Fixture", false, 240.0, custom,
                                    );
                                });
                        },
                    );
                };
                frame(vec![]);
                frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
                frame(vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
                assert_eq!(action.edit, edit);
                assert_eq!(action.apply, !edit);
                assert!(!action.remove);
            }
        }
    }

    #[test]
    fn invalid_hex_entry_is_safe_visible_and_keeps_the_applied_color() {
        assert_eq!(parse_hex_rgb("aédef"), None);
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
        let field = ThemeColorField::Background;
        let original = app.theme_editor.custom.background;
        app.theme_editor
            .color_hex_inputs
            .insert(field, "aédef".into());
        let seed = app.theme_editor.custom.clone();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(600.0, 200.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(!app.appearance_color_row(ui, field, &seed));
                });
            },
        );
        assert_eq!(app.theme_editor.custom.background, original);
        assert_eq!(app.theme_editor.color_hex_inputs[&field], "aédef");
        assert!(
            output.shapes.iter().any(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape {
                    text.galley.job.text == "aédef" && text.fallback_color == app.colors.error
                } else {
                    false
                }
            }),
            "invalid text must use the error role"
        );
    }

    #[test]
    fn appearance_hex_typing_does_not_rewrite_input_midway() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
        let field = ThemeColorField::Background;
        app.theme_editor.selected_region = ThemePreviewRegion::ColumnBackground;
        app.theme_editor
            .color_hex_inputs
            .insert(field, String::new());
        let original_toolbar = app.theme_editor.custom.toolbar;
        let original_alpha = app.theme_editor.custom.background[3];
        let frame = |app: &mut OttrinApp, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 780.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            );
        };
        frame(&mut app, vec![]);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("appearance_hex", field))));
        for character in "#B02060".chars() {
            frame(&mut app, vec![egui::Event::Text(character.to_string())]);
        }
        assert_eq!(app.theme_editor.color_hex_inputs[&field], "#B02060");
        assert_eq!(
            app.theme_editor.custom.background,
            [0xB0, 0x20, 0x60, original_alpha]
        );
        assert_eq!(app.theme_editor.custom.toolbar, original_toolbar);
    }

    #[test]
    fn appearance_cancel_restores_full_theme_and_does_not_save_preview() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.state.config.theme = ThemeMode::Light;
        app.state.config.theme_preset = ThemePreset::Nord;
        let original = app.state.config.clone();
        app.persist_config();
        app.open_theme_editor_for_preset(ThemePreset::G33k, &ctx);
        app.theme_editor.custom.button_radius = 13.0;
        app.apply_theme_editor_live_preview(&ctx);
        assert_eq!(app.colors.button_radius, 13);
        app.persist_config();
        let persisted = load_config();
        assert_eq!(persisted.theme_preset, original.theme_preset);
        assert_eq!(persisted.custom_themes, original.custom_themes);
        app.cancel_theme_editor(&ctx);
        assert_eq!(app.state.config.theme_preset, original.theme_preset);
        assert_eq!(app.state.config.theme, original.theme);
        assert_eq!(app.state.config.custom_themes, original.custom_themes);
        assert_eq!(
            app.state.config.active_custom_theme,
            original.active_custom_theme
        );
    }

    #[test]
    fn appearance_save_applies_named_theme_and_survives_reopening() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.open_theme_editor_for_preset(ThemePreset::Breeze, &ctx);
        app.theme_editor.save_as_name = "Appearance persistence fixture".into();
        app.theme_editor.custom.button_radius = 12.0;
        app.theme_editor.custom.bookmarks_bar = [12, 180, 90, 255];
        app.apply_theme_editor_live_preview(&ctx);
        app.save_theme_editor(&ctx);
        app.persist_config();
        let reopened = load_config();
        assert_eq!(
            reopened.active_custom_theme.as_deref(),
            Some("Appearance persistence fixture")
        );
        assert_eq!(Colors::for_config(&reopened, None).button_radius, 12);
        assert_eq!(
            Colors::for_config(&reopened, None).bookmarks_bg,
            Colors::from_rgba([12, 180, 90, 255])
        );
        let index = app
            .state
            .config
            .custom_themes
            .iter()
            .position(|t| t.name == "Appearance persistence fixture")
            .unwrap();
        app.open_theme_editor_for_saved(index, &ctx);
        app.theme_editor.custom.button_radius = 3.0;
        app.apply_theme_editor_live_preview(&ctx);
        app.save_theme_editor(&ctx);
        app.persist_config();
        assert_eq!(Colors::for_config(&load_config(), None).button_radius, 3);
        assert_eq!(
            app.state.config.custom_themes[index].dark.button_radius,
            3.0
        );
    }

    #[test]
    fn appearance_save_copy_preserves_original_and_rename_is_explicit() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.state.config.custom_themes.clear();
        app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
        app.theme_editor.save_as_name = "My theme".into();
        app.save_theme_editor(&ctx);
        let original = app.state.config.custom_themes[0].clone();
        app.open_theme_editor_for_saved(0, &ctx);
        app.theme_editor.custom.button_radius = 13.0;
        app.save_theme_editor_as_inner(&ctx);
        assert_eq!(app.state.config.custom_themes.len(), 2);
        assert_eq!(
            app.state.config.custom_themes[0].dark.button_radius,
            original.dark.button_radius
        );
        assert_eq!(app.state.config.custom_themes[1].name, "My theme (2)");
        assert_eq!(app.state.config.custom_themes[1].dark.button_radius, 13.0);
        assert_eq!(app.theme_editor.source, ThemeEditorSource::Saved(0));
        app.persist_config();
        let reopened = load_config();
        assert_eq!(reopened.custom_themes.len(), 2);
        assert_eq!(
            reopened.custom_themes[0].dark.button_radius,
            original.dark.button_radius
        );
        assert_eq!(reopened.custom_themes[1].dark.button_radius, 13.0);
        assert_eq!(
            reopened.active_custom_theme.as_deref(),
            Some("My theme (2)")
        );
        assert_eq!(Colors::for_config(&reopened, None).button_radius, 13);
        app.open_theme_editor_for_saved(0, &ctx);
        app.theme_editor.save_as_name = "My theme (2)".into();
        app.save_theme_editor(&ctx);
        assert!(
            app.theme_editor.open,
            "duplicate names must not overwrite a different theme"
        );
        app.theme_editor.save_as_name = "Renamed theme".into();
        app.save_theme_editor(&ctx);
        assert!(!app.theme_editor.open);
        assert_eq!(app.state.config.custom_themes[0].name, "Renamed theme");
        assert_eq!(
            app.state.config.active_custom_theme.as_deref(),
            Some("Renamed theme")
        );
    }

    #[test]
    fn dialogs_use_panel_and_window_shape_roles_independently_of_inputs() {
        fn rectangles(shape: &egui::epaint::Shape, result: &mut Vec<egui::epaint::RectShape>) {
            match shape {
                egui::epaint::Shape::Rect(rect) => result.push(rect.clone()),
                egui::epaint::Shape::Vec(children) => {
                    for child in children {
                        rectangles(child, result);
                    }
                }
                _ => {}
            }
        }
        for kind in ["rename", "creation", "about", "open_with", "graphics"] {
            let ctx = Context::default();
            let mut app = OttrinApp::default();
            app.colors.panel = Color32::from_rgb(40, 84, 106);
            app.colors.panel_raised = Color32::from_rgb(41, 55, 75);
            app.colors.window_border = Color32::from_rgb(235, 113, 135);
            app.colors.window_border_width = 4.0;
            app.colors.app_radius = 20;
            let tab_id = app.state.active_tab().id;
            app.pending_rename = Some(PendingRename {
                tab_id,
                source: PathBuf::from("/__appearance_fixture__/notes.txt"),
                name_input: "notes.txt".into(),
                request_focus: false,
            });
            app.pending_creation = Some(PendingCreation {
                tab_id,
                kind: CreationKind::Folder,
                parent: PathBuf::from("/__appearance_fixture__"),
                name_input: String::new(),
                error: None,
                request_focus: false,
            });
            app.show_about = true;
            app.open_with_dialog = Some(OpenWithDialog {
                target: PathBuf::from("/__appearance_fixture__/notes.txt"),
                choices: Vec::new(),
            });
            app.show_graphics_compatibility_notice = true;
            app.apply_theme_to_ctx(&ctx);
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut shapes = Vec::new();
            for _ in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(1024.0, 680.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| match kind {
                        "rename" => app.render_rename_modal(ctx),
                        "creation" => app.render_creation_modal(ctx),
                        "about" => app.render_about_popup(ctx),
                        "open_with" => app.render_open_with_dialog(ctx),
                        "graphics" => app.render_graphics_compatibility_notice(ctx),
                        _ => unreachable!(),
                    },
                );
                shapes = output.shapes;
            }
            let mut rects = Vec::new();
            fn heading_positions(shape: &egui::epaint::Shape, result: &mut Vec<egui::Pos2>) {
                match shape {
                    egui::epaint::Shape::Text(text)
                        if ["Open With", "Graphics compatibility enabled"]
                            .contains(&text.galley.job.text.as_str()) =>
                    {
                        result.push(text.pos + text.galley.rect.center().to_vec2());
                    }
                    egui::epaint::Shape::Vec(children) => {
                        for child in children {
                            heading_positions(child, result);
                        }
                    }
                    _ => {}
                }
            }
            let mut headings = Vec::new();
            for shape in shapes {
                rectangles(&shape.shape, &mut rects);
                heading_positions(&shape.shape, &mut headings);
            }
            if ["open_with", "graphics"].contains(&kind) {
                assert_eq!(headings.len(), 1, "{kind}: missing dialog heading");
                let heading_surface = rects
                    .iter()
                    .rev()
                    .find(|rect| rect.rect.contains(headings[0]));
                assert_eq!(
                    heading_surface.map(|rect| rect.fill),
                    Some(app.colors.panel),
                    "{kind}: heading must belong to the dialog surface, not an input-colored title band"
                );
            }
            let windows: Vec<_> = rects
                .iter()
                .filter(|rect| {
                    rect.corner_radius.nw == 20
                        && rect.stroke == Stroke::new(4.0, app.colors.window_border)
                })
                .collect();
            assert!(
                !windows.is_empty(),
                "{kind}: no themed dialog frame rendered"
            );
            assert!(
                windows.iter().all(|rect| rect.fill == app.colors.panel),
                "{kind}: dialog fill must use Panels and dialogs, not Inputs / raised surfaces"
            );
        }
    }

    #[test]
    fn deleting_active_saved_theme_keeps_its_visible_mode_for_the_base_palette() {
        for (saved_mode, prior_mode) in [
            (ThemeMode::Light, ThemeMode::Dark),
            (ThemeMode::Dark, ThemeMode::Light),
        ] {
            let ctx = Context::default();
            let mut app = OttrinApp::default();
            app.state.config.theme = prior_mode;
            app.state.config.active_custom_theme = Some("Palette to delete".into());
            app.state.config.custom_themes = vec![SavedTheme {
                name: "Palette to delete".into(),
                base_preset: ThemePreset::Solarized,
                dark: (OttrinApp::custom_from_colors(Colors::preset(
                    ThemePreset::Solarized,
                    saved_mode == ThemeMode::Dark,
                )))
                .clone(),
                light: OttrinApp::custom_from_colors(Colors::preset(
                    ThemePreset::Solarized,
                    saved_mode == ThemeMode::Dark,
                )),
            }];
            app.delete_saved_custom_theme(0, &ctx);
            assert!(app.state.config.custom_themes.is_empty());
            assert!(app.state.config.active_custom_theme.is_none());
            assert_eq!(app.state.config.theme, prior_mode);
            assert_eq!(
                app.colors,
                Colors::preset(ThemePreset::Solarized, prior_mode == ThemeMode::Dark)
            );
        }
    }

    #[test]
    fn settings_surface_and_title_do_not_inherit_content_background() {
        fn front_fill(shape: &egui::epaint::Shape, point: egui::Pos2) -> Option<Color32> {
            match shape {
                egui::epaint::Shape::Rect(rect)
                    if rect.rect.contains(point) && rect.fill.a() != 0 =>
                {
                    Some(rect.fill)
                }
                egui::epaint::Shape::Vec(shapes) => shapes
                    .iter()
                    .rev()
                    .find_map(|shape| front_fill(shape, point)),
                _ => None,
            }
        }
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.show_settings = true;
        app.settings_tab = SettingsTab::Appearance;
        app.colors.panel = Color32::from_rgb(90, 30, 170);
        app.colors.titlebar_bg = Color32::from_rgb(20, 140, 100);
        for background in [Color32::from_rgb(176, 32, 96), Color32::WHITE] {
            app.colors.bg = background;
            app.apply_theme_to_ctx(&ctx);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(920.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.render_settings_modal_embedded(ctx);
                },
            );
            for (point, color) in [
                (egui::pos2(500.0, 700.0), app.colors.panel),
                (egui::pos2(500.0, 25.0), app.colors.titlebar_bg),
            ] {
                assert_eq!(
                    output
                        .shapes
                        .iter()
                        .rev()
                        .find_map(|shape| front_fill(&shape.shape, point)),
                    Some(color),
                    "Settings must paint the selected surface role directly, without an unrelated overlay"
                );
            }
        }
    }

    #[test]
    fn file_canvas_paints_content_color_without_sidebar_color_leaking_into_it() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        app.state.active_tab_mut().view_mode = ViewMode::List;
        app.colors.bg = Color32::from_rgb(176, 32, 96);
        app.colors.smart_panel_bg = Color32::from_rgb(20, 140, 100);
        app.colors.panel = Color32::from_rgb(90, 30, 170);
        app.apply_theme_to_ctx(&ctx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1024.0, 680.0),
                )),
                ..Default::default()
            },
            |ctx| app.render_single_file_pane(ctx),
        );
        let center = app.file_pane_rect.expect("file canvas").center();
        let fills: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::epaint::Shape::Rect(rect) = &shape.shape
                    && rect.rect.contains(center)
                {
                    return Some(rect.fill);
                }
                None
            })
            .collect();
        assert!(
            fills.contains(&app.colors.bg),
            "canvas must paint its own background"
        );
        assert!(
            !fills.contains(&app.colors.smart_panel_bg),
            "sidebar background must not fill the canvas"
        );
    }

    #[test]
    fn appearance_surface_and_selection_colors_do_not_alias_other_elements() {
        let mut config = AppConfig::default();
        let mut custom = OttrinApp::custom_from_colors(Colors::preset(ThemePreset::Ottrin, true));
        let original = Colors::for_config(&config, None);
        custom.bookmarks_bar = [170, 22, 80, 255];
        custom.statusbar = [25, 170, 80, 255];
        custom.selected_bg = [140, 20, 220, 60];
        custom.panel_raised = [42, 50, 70, 255];
        config.active_custom_theme = Some("Fixture".into());
        config.custom_themes.push(SavedTheme {
            name: "Fixture".into(),
            base_preset: ThemePreset::Ottrin,
            dark: custom.clone(),
            light: custom.clone(),
        });
        let changed = Colors::for_config(&config, None);
        assert_eq!(
            changed.bookmarks_bg,
            Colors::from_rgba(custom.bookmarks_bar)
        );
        assert_eq!(changed.statusbar_bg, Colors::from_rgba(custom.statusbar));
        assert_eq!(changed.selected_bg, Colors::from_rgba(custom.selected_bg));
        assert_eq!(changed.panel_raised, Colors::from_rgba(custom.panel_raised));
        assert_eq!(changed.toolbar_bg, original.toolbar_bg);
        assert_eq!(changed.smart_panel_bg, original.smart_panel_bg);
        assert_eq!(changed.panel, original.panel);
        assert_eq!(changed.accent, original.accent);
        assert_eq!(changed.accent_dim, original.accent_dim);
        assert_eq!(changed.row_alt, original.row_alt);
    }

    #[test]
    fn appearance_every_preset_seeds_without_changing_its_colors() {
        for preset in [
            ThemePreset::Ottrin,
            ThemePreset::Breeze,
            ThemePreset::Adwaita,
            ThemePreset::Windows11,
            ThemePreset::Solarized,
            ThemePreset::Nord,
            ThemePreset::G33k,
        ] {
            for dark in [false, true] {
                let original = Colors::preset(preset, dark);
                let custom = OttrinApp::custom_from_colors(original);
                let config = AppConfig {
                    theme: if dark {
                        ThemeMode::Dark
                    } else {
                        ThemeMode::Light
                    },
                    theme_preset: preset,
                    active_custom_theme: Some("Fixture".into()),
                    custom_themes: vec![SavedTheme {
                        name: "Fixture".into(),
                        base_preset: preset,
                        dark: (custom).clone(),
                        light: custom,
                    }],
                    ..Default::default()
                };
                let seeded = Colors::for_config(&config, None);
                assert_eq!(original, seeded, "{preset:?}, dark={dark}");
            }
        }
    }
}
