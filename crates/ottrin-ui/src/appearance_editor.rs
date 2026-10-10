use super::*;

const REGIONS: [ThemePreviewRegion; 10] = [
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
        ui.label(RichText::new("Choose a theme, then make it yours.").color(c.text_dim));
        ui.add_space(14.0);
        Frame::new()
            .fill(c.panel_raised)
            .corner_radius(c.border_radius)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let name = if self.theme_editor.open { self.theme_editor.save_as_name.clone() } else { self
                    .state
                    .config
                    .active_custom_theme
                    .clone()
                    .unwrap_or_else(|| {
                        Self::theme_preset_label(self.state.config.theme_preset).into()
                    }) };
                ui.horizontal(|ui| {
                    theme_preview_strip(ui, c, 90.0, 30.0, c);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&name).strong().color(c.heading));
                        ui.label(RichText::new(if self.theme_editor.open { "Unsaved preview" } else { "Active theme" }).small().color(c.text_dim));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(if self.theme_editor.open { "Return to theme editor" } else { "Edit active theme" }).themed(&c))
                            .clicked()
                        {
                            if self.theme_editor.open {
                                ctx.send_viewport_cmd_to(Self::theme_editor_viewport_id(), egui::ViewportCommand::Focus);
                            } else {
                            let saved_index =
                                self.state.config.custom_themes.iter().position(|theme| {
                                    Some(theme.name.as_str())
                                        == self.state.config.active_custom_theme.as_deref()
                                });
                            if let Some(index) = saved_index {
                                self.open_theme_editor_for_saved(index, ctx);
                            } else {
                                self.open_theme_editor_for_preset(
                                    self.state.config.theme_preset,
                                    ctx,
                                );
                            }
                            }
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Color mode");
                    let mut mode = if self.theme_editor.open { self.theme_editor.mode } else { self.current_theme_mode() };
                    ui.add_enabled_ui(!self.theme_editor.open && self.active_saved_theme().is_none(), |ui| {
                    for (value, label) in [
                        (ThemeMode::Light, "Light"),
                        (ThemeMode::Dark, "Dark"),
                        (ThemeMode::System, "System"),
                    ] {
                        ui.selectable_value(&mut mode, value, label);
                    }
                    });
                    if !self.theme_editor.open && mode != self.current_theme_mode() { self.apply_theme_mode(mode, ctx); preferences_changed = true; }
                });
                if self.active_saved_theme().is_some() {
                    ui.label(RichText::new("Saved palettes keep their colors. Choose a built-in theme to change color mode.").small().color(c.text_dim));
                } else if self.state.config.theme == ThemeMode::System {
                    let preference = self.system_theme(ctx);
                    ui.label(RichText::new(if preference.is_some() {
                        format!("Follows the desktop · {}", Self::theme_mode_label(Colors::resolved_mode(ThemeMode::System, preference)))
                    } else { "Desktop preference unavailable · using Dark".into() }).small().color(c.text_dim));
                }
            });
        if self.theme_editor.open {
            ui.label(
                RichText::new("Finish the preview in the editor before switching themes.")
                    .small()
                    .color(c.text_dim),
            );
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Built-in themes").strong().color(c.heading));
        ui.label(
            RichText::new("Select a theme to apply its defaults. Your saved themes stay below.")
                .small()
                .color(c.text_dim),
        );
        ui.add_space(8.0);
        ui.add_enabled_ui(!self.theme_editor.open, |ui| {
            let presets = [
                ThemePreset::Ottrin,
                ThemePreset::Breeze,
                ThemePreset::Adwaita,
                ThemePreset::Windows11,
                ThemePreset::Solarized,
                ThemePreset::Nord,
                ThemePreset::G33k,
            ];
            let columns = ((ui.available_width() / 180.0).floor() as usize).clamp(1, 4);
            let gap = ui.spacing().item_spacing.x;
            let width = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
            for row in presets.chunks(columns) {
                ui.horizontal(|ui| {
                    for &preset in row {
                        let selected = self.state.config.theme_preset == preset
                            && self.state.config.active_custom_theme.is_none();
                        let preview = Colors::preset(
                            preset,
                            Colors::resolved_mode(
                                self.current_theme_mode(),
                                self.system_theme(ctx),
                            ) != ThemeMode::Light,
                        );
                        if appearance_theme_card(
                            ui,
                            c,
                            preview,
                            Self::theme_preset_label(preset),
                            selected,
                            width,
                        )
                        .clicked()
                        {
                            self.apply_theme_preset(preset, ctx);
                            preferences_changed = true;
                        }
                    }
                });
                ui.add_space(6.0);
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new("Your themes").strong().color(c.heading));
        if self.state.config.custom_themes.is_empty() {
            ui.label(
                RichText::new("Save a theme in the editor to keep your changes here.")
                    .small()
                    .color(c.text_dim),
            );
        }
        let mut apply = None;
        let mut edit = None;
        let mut remove = None;
        ui.add_enabled_ui(!self.theme_editor.open, |ui| {
            for (index, saved) in self.state.config.custom_themes.iter().enumerate() {
                ui.push_id(("saved_theme", index), |ui| {
                    let preview = Colors::for_saved(saved, self.system_theme(ctx));
                    let selected = self.state.config.active_custom_theme.as_deref()
                        == Some(saved.name.as_str());
                    Frame::new()
                        .fill(c.panel_raised)
                        .corner_radius(c.border_radius)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                theme_preview_strip(ui, preview, 64.0, 24.0, c);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&saved.name).strong().color(c.heading));
                                    ui.label(
                                        RichText::new(format!(
                                            "{} · {}",
                                            Self::theme_preset_label(saved.base_preset),
                                            Self::theme_mode_label(saved.theme_mode)
                                        ))
                                        .small()
                                        .color(c.text_dim),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.add(egui::Button::new("Delete").themed(&c)).clicked() {
                                        remove = Some(index);
                                    }
                                    if ui.add(egui::Button::new("Edit").themed(&c)).clicked() {
                                        edit = Some(index);
                                    }
                                    if ui
                                        .add_enabled(
                                            !selected,
                                            egui::Button::new(if selected {
                                                "Active"
                                            } else {
                                                "Apply"
                                            })
                                            .themed(&c),
                                        )
                                        .clicked()
                                    {
                                        apply = Some(index);
                                    }
                                });
                            });
                        });
                });
                ui.add_space(4.0);
            }
        });
        if let Some(index) = apply {
            let saved = self.state.config.custom_themes[index].clone();
            self.apply_saved_custom_theme(&saved, ctx);
            preferences_changed = true;
        }
        if let Some(index) = edit {
            self.open_theme_editor_for_saved(index, ctx);
        }
        if let Some(index) = remove {
            preferences_changed = true;
            self.delete_saved_custom_theme(index, ctx);
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
        let seed = Self::custom_from_colors(Colors::preset(
            self.theme_editor.preset,
            self.theme_editor.mode != ThemeMode::Light,
        ));
        let action_model = ThemeEditorActionModel::for_source(self.theme_editor.source_saved_idx);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(c.panel).inner_margin(12))
            .show(ctx, |ui| {
                render_custom_titlebar(ui, ctx, c, c.titlebar_bg, "Appearance · Theme editor", true,
                    &mut close_requested, |_| {});
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Editing").strong().color(c.heading));
                    let identity = if self.theme_editor.source_saved_idx.is_some() {
                        format!("{} · {}", self.theme_editor.save_as_name, Self::theme_mode_label(self.theme_editor.mode))
                    } else {
                        format!("{} · {}", Self::theme_preset_label(self.theme_editor.preset), Self::theme_mode_label(self.theme_editor.mode))
                    };
                    ui.label(identity);
                    if action_model.allow_scope_changes {
                        let mut preset = self.theme_editor.preset;
                        egui::ComboBox::from_id_salt("editor_base")
                            .selected_text(Self::theme_preset_label(preset))
                            .show_ui(ui, |ui| {
                                for p in [ThemePreset::Ottrin, ThemePreset::Breeze, ThemePreset::Adwaita,
                                    ThemePreset::Windows11, ThemePreset::Solarized, ThemePreset::Nord, ThemePreset::G33k] {
                                    ui.selectable_value(&mut preset, p, Self::theme_preset_label(p));
                                }
                            });
                        let mut mode = self.theme_editor.mode;
                        egui::ComboBox::from_id_salt("editor_mode")
                            .selected_text(Self::theme_mode_label(mode))
                            .show_ui(ui, |ui| {
                                for m in [ThemeMode::Dark, ThemeMode::Light] {
                                    ui.selectable_value(&mut mode, m, Self::theme_mode_label(m));
                                }
                            });
                        if preset != self.theme_editor.preset || mode != self.theme_editor.mode {
                            self.switch_theme_editor_scope(preset, mode, ctx);
                        }
                    }
                });
                ui.label(RichText::new("Changes preview live. Save to keep them; Cancel restores the saved appearance.")
                    .small().color(c.text_dim));
                ui.add_space(10.0);

                // Keep the preview and navigation visible while properties scroll independently.
                let available = ui.available_rect_before_wrap();
                let footer = Rect::from_min_max(egui::pos2(available.min.x, available.max.y - 76.0), available.max);
                let content = Rect::from_min_max(available.min, egui::pos2(available.max.x, footer.min.y - 12.0));
                ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
                    ui.columns(2, |columns| {
                        columns[0].set_height(content.height());
                        columns[0].label(RichText::new("Live preview").strong().color(c.heading));
                        columns[0].label(RichText::new(if self.theme_editor.tab == ThemeEditorTab::FileColors {
                            "Real file icons and labels with file colors enabled."
                        } else if self.theme_editor.tab == ThemeEditorTab::Typography { "Preview of shared text roles." }
                        else if self.theme_editor.tab == ThemeEditorTab::Shape { "Preview of shared corners and borders." }
                        else if self.theme_editor.tab == ThemeEditorTab::Interaction { "Examples of selection, hover, focus, and errors." }
                        else { "Choose an element below or click it in the preview." }).small().color(c.text_dim));
                        let preview = self.theme_editor_preview_colors();
                        let clicked = if self.theme_editor.tab == ThemeEditorTab::FileColors {
                            appearance_file_color_preview(&mut columns[0], preview);
                            None
                        } else {
                            theme_editor_large_preview(&mut columns[0], preview, self.theme_editor.selected_region, self.theme_editor.tab).1
                        };
                        if let Some(region) = clicked {
                            self.theme_editor.selected_region = region;
                            self.theme_editor.tab = ThemeEditorTab::Colors;
                        }
                        if self.theme_editor.tab == ThemeEditorTab::FileColors {
                            columns[0].add_space(8.0);
                            columns[0].label(RichText::new(format!("Current file views: colors {}, colored folder labels {}.",
                                if self.state.config.colorize_file_types { "on" } else { "off" },
                                if self.state.config.colorize_file_types && self.state.config.colorize_folder_labels { "on" } else { "off" }))
                                .small().color(c.text_dim));
                        } else if self.theme_editor.tab == ThemeEditorTab::Colors {
                        let navigation_height = (content.max.y - columns[0].cursor().min.y).max(60.0);
                        ScrollArea::vertical().id_salt("appearance_elements").max_height(navigation_height)
                            .show(&mut columns[0], |ui| {
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            for region in REGIONS {
                                if ui.selectable_label(self.theme_editor.selected_region == region,
                                    Self::theme_preview_region_label(region)).clicked() {
                                    self.theme_editor.selected_region = region;
                                    self.theme_editor.tab = ThemeEditorTab::Colors;
                                }
                            }
                        });
                        ui.add_space(12.0);
                        ui.label(RichText::new("Scope").strong().color(c.heading));
                        ui.label(RichText::new(Self::theme_preview_region_hint(self.theme_editor.selected_region))
                            .small().color(c.text_dim));

                            });
                        } else {
                            columns[0].add_space(12.0);
                            columns[0].label(RichText::new("Scope · Across Ottrin").strong().color(c.heading));
                            columns[0].label(RichText::new(match self.theme_editor.tab {
                                ThemeEditorTab::Typography => "Shared interface text. Folder and file labels have their own roles in Colors.",
                                ThemeEditorTab::Shape => "Window frames, surfaces, and buttons across the interface.",
                                ThemeEditorTab::Interaction => "Shared hover, selection, focus, and error feedback in file views and controls.",
                                _ => unreachable!(),
                            }).small().color(c.text_dim));
                        }

                        columns[1].set_height(content.height());
                        columns[1].horizontal_wrapped(|ui| {
                            for (tab, title) in [(ThemeEditorTab::Colors, "Colors"),
                                (ThemeEditorTab::Typography, "Typography"),
                                (ThemeEditorTab::Shape, "Shape"), (ThemeEditorTab::FileColors, "File colors"), (ThemeEditorTab::Interaction, "States")] {
                                ui.selectable_value(&mut self.theme_editor.tab, tab, title);
                            }
                        });
                        columns[1].separator();
                        let scroll_height = (content.height() - 44.0).max(100.0);
                        ScrollArea::vertical().id_salt("appearance_properties")
                            .max_height(scroll_height).show(&mut columns[1], |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 10.0;
                                match self.theme_editor.tab {
                                    ThemeEditorTab::Colors => {
                                        let region = self.theme_editor.selected_region;
                                        ui.label(RichText::new(Self::theme_preview_region_label(region)).size(18.0 * c.font_scale).strong().color(c.heading));
                                        let fields = Self::theme_preview_region_fields(region);
                                        for &field in fields {
                                            changed |= self.appearance_color_row(ui, field, &seed);
                                        }
                                        if ui.horizontal(|ui| ui.add(egui::Button::new("Reset element colors").themed(&c))).inner.clicked() {
                                            for &field in fields {
                                                *Self::theme_color_field_value_mut(&mut self.theme_editor.custom, field) =
                                                    Self::theme_color_field_value(&seed, field);
                                                let color = Self::theme_color_field_value(&seed, field);
                                                self.theme_editor.color_hex_inputs.insert(field, format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]));
                                            }
                                            changed = true;
                                        }
                                        ui.add_space(8.0);
                                        ui.separator();
                                        ui.label(RichText::new("Shared defaults").strong().color(c.heading));
                                        ui.label(RichText::new("These colors are shared across the application.").small().color(c.text_dim));
                                        for field in [ThemeColorField::Panel, ThemeColorField::RaisedPanel, ThemeColorField::Accent,
                                            ThemeColorField::Border, ThemeColorField::WindowBorder] {
                                            if fields.contains(&field) { continue; }
                                            changed |= self.appearance_color_row(ui, field, &seed);
                                        }
                                    }
                                    ThemeEditorTab::Typography => {
                                        ui.label(RichText::new("Application typography").size(18.0 * c.font_scale).strong().color(c.heading));
                                        ui.label(RichText::new("Shared text roles across windows and file views.").small().color(c.text_dim));
                                        changed |= number_row(ui, "Text size", &mut self.theme_editor.custom.font_scale,
                                            0.85..=1.35, seed.font_scale, 0.01, c);
                                        for field in [ThemeColorField::Text, ThemeColorField::TextDim, ThemeColorField::TextMuted, ThemeColorField::TextHeading] {
                                            changed |= self.appearance_color_row(ui, field, &seed);
                                        }
                                    }
                                    ThemeEditorTab::Shape => {
                                        ui.label(RichText::new("Application shape").size(18.0 * c.font_scale).strong().color(c.heading));
                                        ui.label(RichText::new("Shared corner and border dimensions, in pixels.").small().color(c.text_dim));
                                        changed |= number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius,
                                            0.0..=24.0, seed.app_radius, 1.0, c);
                                        changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius,
                                            0.0..=14.0, seed.border_radius, 1.0, c);
                                        changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius,
                                            0.0..=14.0, seed.button_radius, 1.0, c);
                                        changed |= number_row(ui, "Window border thickness", &mut self.theme_editor.custom.window_border_width,
                                            0.0..=4.0, seed.window_border_width, 0.5, c);
                                    }
                                    ThemeEditorTab::Interaction => {
                                        ui.label(RichText::new("Interaction states").size(18.0 * c.font_scale).strong().color(c.heading));
                                        ui.label(RichText::new("Shared feedback colors for selections, hover, focus, errors and destructive actions.").small().color(c.text_dim));
                                        for field in [ThemeColorField::Selection, ThemeColorField::Hover, ThemeColorField::Accent, ThemeColorField::Error] {
                                            changed |= self.appearance_color_row(ui, field, &seed);
                                        }
                                    }
                                    ThemeEditorTab::FileColors => {
                                        ui.label(RichText::new("File type palette").size(18.0 * c.font_scale).strong().color(c.heading));
                                        ui.label(RichText::new("Used when “Color files by type” is on in Appearance. The preview shows that mode, including colored folder labels.").small().color(c.text_dim));
                                        ui.label(RichText::new("Shared colors serve the listed types. Fonts, packages and code use blends; colored labels blend with neutral text.").small().color(c.text_dim));
                                        for field in [ThemeColorField::PaletteSoftWhite, ThemeColorField::PaletteFolderBlue,
                                            ThemeColorField::PaletteLinkBlue, ThemeColorField::PaletteSteel,
                                            ThemeColorField::PaletteGreen, ThemeColorField::PaletteOrange,
                                            ThemeColorField::PalettePurple, ThemeColorField::PalettePink,
                                            ThemeColorField::PaletteRed, ThemeColorField::PaletteYellow] {
                                            changed |= self.appearance_color_row(ui, field, &seed);
                                        }
                                    }
                                }
                            });
                    });
                });
                ui.scope_builder(UiBuilder::new().max_rect(footer), |ui| {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Theme name");
                        ui.add(egui::TextEdit::singleline(&mut self.theme_editor.save_as_name).desired_width(220.0).themed(&self.colors));
                        if ui.add(egui::Button::new("Save and apply").themed(&c)).clicked() {
                            self.save_theme_editor(ctx);
                            self.persist_config();
                        }
                        if ui.add(egui::Button::new("Cancel").themed(&c)).clicked() { close_requested = true; }
                        themed_menu_button(ui, "More", c, |ui| {
                            if ui.add(egui::Button::new("Import theme…").themed(&c)).clicked() { self.import_theme_editor_json(ctx); ui.close(); }
                            if ui.add(egui::Button::new("Export theme…").themed(&c)).clicked() { self.export_theme_editor_json(); ui.close(); }
                            if action_model.show_save_as && ui.add(egui::Button::new("Save a copy").themed(&c)).clicked() { self.save_theme_editor_as(); self.persist_config(); ui.close(); }
                            if ui.add(egui::Button::new("Reset entire theme to base").themed(&c)).clicked() {
                                self.theme_editor.custom = seed.clone(); self.sync_theme_editor_hex_inputs(); changed = true; ui.close();
                            }
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
                ui.allocate_ui_with_layout(
                    Vec2::new(180.0 * self.colors.font_scale, 26.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(180.0 * self.colors.font_scale);
                        ui.label(Self::theme_color_field_label(field))
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
                    .add(
                        egui::TextEdit::singleline(&mut hex)
                            .id(egui::Id::new(("appearance_hex", field)))
                            .desired_width(80.0 * self.colors.font_scale)
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
                if ui
                    .add(egui::Button::new("↺").small().themed(&self.colors))
                    .on_hover_text("Reset this color to the base theme")
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
                let alpha = Self::theme_color_field_value(&self.theme_editor.custom, field)[3];
                if alpha < 255 {
                    ui.label(RichText::new(format!("{:.0}%", alpha as f32 * 100.0 / 255.0))
                        .small().color(self.colors.text_dim))
                        .on_hover_text("Opacity. This color blends with the surface behind it; change opacity in the color picker.");
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
    let (swatch, response) = ui.allocate_exact_size(ui.spacing().interact_size, Sense::click());
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
    let mut response = response.on_hover_text("Choose color and opacity");
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
        ui.label(title);
        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 150.0;
            changed |= ui
                .add(egui::Slider::new(value, range).step_by(step))
                .changed();
            if ui
                .add(egui::Button::new("↺").small().themed(&c))
                .on_hover_text("Reset this dimension to the base theme")
                .clicked()
            {
                *value = default;
                changed = true;
            }
        });
    });
    changed
}

fn appearance_file_color_preview(ui: &mut egui::Ui, c: Colors) {
    let width = ui.available_width().max(320.0);
    let height = (width * 0.56).clamp(320.0, 460.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect.shrink(8.0), c.border_radius, c.bg);
    let palette = semantic_style::semantic_palette(&c);
    let samples = [
        ("Documents", EntryKind::Directory, false),
        ("shortcut", EntryKind::Symlink, false),
        ("program", EntryKind::File, true),
        ("photo.png", EntryKind::File, false),
        ("movie.mp4", EntryKind::File, false),
        ("music.mp3", EntryKind::File, false),
        ("archive.zip", EntryKind::File, false),
        ("report.pdf", EntryKind::File, false),
        ("notes.txt", EntryKind::File, false),
        ("budget.xlsx", EntryKind::File, false),
        ("slides.pptx", EntryKind::File, false),
        ("typeface.ttf", EntryKind::File, false),
        ("disk.iso", EntryKind::File, false),
        ("package.deb", EntryKind::File, false),
        ("settings.ini", EntryKind::File, false),
        ("source.rs", EntryKind::File, false),
    ];
    let row_height = (height - 40.0) / 8.0;
    for (index, (name, kind, executable)) in samples.into_iter().enumerate() {
        let semantic = ottrin_core::classify_file(
            Path::new(name),
            kind,
            executable,
            None,
            ottrin_core::platform_case_mode_current(),
        );
        let style = semantic_style::style_for_semantic(
            &semantic,
            semantic_style::EntryVisualState::default(),
            &c,
            &palette,
            c.preset,
            true,
            true,
        );
        let origin = rect.min
            + Vec2::new(
                24.0 + (index / 8) as f32 * (width / 2.0),
                26.0 + (index % 8) as f32 * row_height,
            );
        painter.text(
            origin,
            Align2::LEFT_CENTER,
            semantic_style::material_icon(style.icon),
            FontId::proportional(16.0 * c.font_scale),
            style.icon_color,
        );
        painter.text(
            origin + Vec2::new(25.0, 0.0),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(12.0 * c.font_scale),
            style.text_color,
        );
    }
}

fn appearance_theme_card(
    ui: &mut egui::Ui,
    c: Colors,
    preview: Colors,
    name: &str,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 104.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, c.border_radius, c.panel_raised);
    p.rect_stroke(
        rect,
        c.border_radius,
        Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { c.accent } else { c.border },
        ),
        egui::StrokeKind::Inside,
    );
    let thumb = Rect::from_min_max(rect.min + Vec2::splat(8.0), rect.max - Vec2::new(8.0, 30.0));
    p.rect_filled(thumb, preview.app_radius, preview.bg);
    let title = Rect::from_min_size(thumb.min, Vec2::new(thumb.width(), 10.0));
    p.rect_filled(title, preview.app_radius, preview.titlebar_bg);
    let nav = Rect::from_min_size(
        egui::pos2(thumb.left(), title.bottom()),
        Vec2::new(thumb.width(), 9.0),
    );
    p.rect_filled(nav, 0.0, preview.toolbar_bg);
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
            1.0,
            preview.folder,
        );
        p.line_segment(
            [
                row_rect.min + Vec2::new(12.0, 4.0),
                row_rect.min + Vec2::new(body.width() * 0.65, 4.0),
            ],
            Stroke::new(1.0, preview.folder_text),
        );
    }
    p.rect_filled(
        Rect::from_min_max(egui::pos2(thumb.left(), content.bottom()), thumb.max),
        0.0,
        preview.statusbar_bg,
    );
    p.text(
        egui::pos2(rect.left() + 10.0, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.0 * c.font_scale),
        c.text,
    );
    if selected {
        p.circle_filled(
            egui::pos2(rect.right() - 14.0, rect.bottom() - 15.0),
            3.0,
            c.accent,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            app.state.config.custom_themes[index]
                .customization
                .button_radius,
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
        app.save_theme_editor_as();
        assert_eq!(app.state.config.custom_themes.len(), 2);
        assert_eq!(
            app.state.config.custom_themes[0]
                .customization
                .button_radius,
            original.customization.button_radius
        );
        assert_eq!(app.state.config.custom_themes[1].name, "My theme (2)");
        assert_eq!(
            app.state.config.custom_themes[1]
                .customization
                .button_radius,
            13.0
        );
        assert_eq!(app.theme_editor.source_saved_idx, Some(0));
        app.persist_config();
        let reopened = load_config();
        assert_eq!(reopened.custom_themes.len(), 2);
        assert_eq!(
            reopened.custom_themes[0].customization.button_radius,
            original.customization.button_radius
        );
        assert_eq!(reopened.custom_themes[1].customization.button_radius, 13.0);
        assert_eq!(reopened.active_custom_theme.as_deref(), Some("My theme"));
        assert_eq!(
            Colors::for_config(&reopened, None).button_radius,
            original.customization.button_radius.round() as u8
        );
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
                theme_mode: saved_mode,
                base_preset: ThemePreset::Solarized,
                customization: OttrinApp::custom_from_colors(Colors::preset(
                    ThemePreset::Solarized,
                    saved_mode == ThemeMode::Dark,
                )),
            }];
            app.delete_saved_custom_theme(0, &ctx);
            assert!(app.state.config.custom_themes.is_empty());
            assert!(app.state.config.active_custom_theme.is_none());
            assert_eq!(app.state.config.theme, saved_mode);
            assert_eq!(
                app.colors,
                Colors::preset(ThemePreset::Solarized, saved_mode == ThemeMode::Dark)
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
            theme_mode: ThemeMode::Dark,
            base_preset: ThemePreset::Ottrin,
            customization: custom.clone(),
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
                        theme_mode: if dark {
                            ThemeMode::Dark
                        } else {
                            ThemeMode::Light
                        },
                        base_preset: preset,
                        customization: custom,
                    }],
                    ..Default::default()
                };
                let seeded = Colors::for_config(&config, None);
                for (expected, actual) in [
                    (original.bg, seeded.bg),
                    (original.panel, seeded.panel),
                    (original.panel_raised, seeded.panel_raised),
                    (original.bookmarks_bg, seeded.bookmarks_bg),
                    (original.titlebar_bg, seeded.titlebar_bg),
                    (original.toolbar_bg, seeded.toolbar_bg),
                    (original.statusbar_bg, seeded.statusbar_bg),
                    (original.selected_bg, seeded.selected_bg),
                    (original.heading, seeded.heading),
                    (original.hover, seeded.hover),
                    (original.error, seeded.error),
                ] {
                    assert_eq!(expected, actual, "{preset:?}, dark={dark}");
                }
            }
        }
    }
}
