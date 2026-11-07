use super::*;

const REGIONS: [ThemePreviewRegion; 12] = [
    ThemePreviewRegion::Defaults,
    ThemePreviewRegion::Inspector,
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
        let previous_region = self.theme_editor.selected_region;
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
        egui::CentralPanel::default().frame(Frame::new().fill(c.panel).inner_margin(egui::Margin::same(c.window_border_width.ceil() as i8))).show(ctx, |ui| {
            render_custom_titlebar(ui, ctx, c, c.titlebar_bg, &title, true, &mut close_requested);
            Frame::new().inner_margin(16).show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label("Theme name");
                ui.add(egui::TextEdit::singleline(&mut self.theme_editor.save_as_name).desired_width(220.0).themed(&c).accessible_name("Theme name"));
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
                ui.horizontal(|ui| {
                    if ui.selectable_label(!self.theme_editor.preview_inspector, "File manager").clicked() {
                        self.theme_editor.preview_inspector = false;
                        self.theme_editor.selected_region = ThemePreviewRegion::Defaults;
                    }
                    if ui.selectable_label(self.theme_editor.preview_inspector, "Quick Inspector").clicked() {
                        self.theme_editor.preview_inspector = true;
                        self.theme_editor.selected_region = ThemePreviewRegion::Inspector;
                    }
                });
                ui.add_space(6.0);
                let preview = self.theme_editor_preview_colors();
                let picked = if self.theme_editor.preview_inspector {
                    inspector_controls::theme_preview(ui, preview, self.theme_editor.selected_region,
                        &mut self.theme_editor.preview_inspector_state)
                } else {
                    theme_editor_large_preview(ui, preview, self.theme_editor.selected_region,
                        &mut self.theme_editor.preview_view).1
                };
                if let Some(region) = picked { self.theme_editor.selected_region = region; }
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
                if region != self.theme_editor.selected_region {
                    match region {
                        ThemePreviewRegion::Inspector => self.theme_editor.preview_inspector = true,
                        ThemePreviewRegion::Navigation | ThemePreviewRegion::Bookmarks | ThemePreviewRegion::ColumnBackground |
                        ThemePreviewRegion::SelectedRow | ThemePreviewRegion::ColumnText | ThemePreviewRegion::Sidebar |
                        ThemePreviewRegion::StatusBar => self.theme_editor.preview_inspector = false,
                        _ => {},
                    }
                }
                self.theme_editor.selected_region = region;
                ui.add_space(8.0);
                let properties = ScrollArea::vertical().id_salt("appearance_properties").max_height((inspector_rect.height()-44.0).max(100.0));
                let properties = if region != previous_region { properties.vertical_scroll_offset(0.0) } else { properties };
                properties.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 10.0;
                    ui.label(RichText::new(Self::theme_preview_region_hint(region)).small().color(c.text_dim));
                    ui.add_space(4.0);
                    let fields = Self::theme_preview_region_fields(region);
                    let color_group = match region {
                        ThemePreviewRegion::Defaults => "Shared palette",
                        ThemePreviewRegion::Inspector => "Shared panel background",
                        _ => "Component colors",
                    };
                    changed |= property_group(ui, color_group, c, |ui| {
                        let mut changed = false;
                        for &field in fields { changed |= self.appearance_color_row(ui, field, &seed); }
                        changed
                    });
                    if region == ThemePreviewRegion::Defaults {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Typography", c, |ui| {
                            number_row(ui, "Text size", &mut self.theme_editor.custom.font_scale, 0.85..=1.35, seed.font_scale, 0.01, c)
                        });
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Geometry", c, |ui| {
                            let mut changed = number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                            changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                            changed
                        });
                    }
                    if region == ThemePreviewRegion::Accent {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Geometry", c, |ui| {
                            let mut group_changed = false;
                        group_changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                            group_changed
                        });
                    }
                    if region == ThemePreviewRegion::Borders {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Geometry", c, |ui| {
                            let mut group_changed = false;
                        group_changed |= window_border_width_row(ui, &mut self.theme_editor.custom.window_border_width, seed.window_border_width, c);
                            group_changed
                        });
                    }
                    if region == ThemePreviewRegion::ColumnText {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Category colors", c, |ui| {
                            let mut group_changed = false;
                        group_changed |= number_row(ui, "Neutral text blend", &mut self.theme_editor.custom.file_label_blend, 0.0..=1.0, seed.file_label_blend, 0.01, c);
                        for field in [ThemeColorField::PaletteSoftWhite, ThemeColorField::PaletteFolderBlue, ThemeColorField::PaletteLinkBlue, ThemeColorField::PaletteSteel,
                            ThemeColorField::PaletteGreen, ThemeColorField::PaletteOrange, ThemeColorField::PalettePurple, ThemeColorField::PalettePink, ThemeColorField::PaletteRed, ThemeColorField::PaletteYellow] {
                            group_changed |= self.appearance_color_row(ui, field, &seed);
                        }
                            group_changed
                        });
                    }
                    ui.add_space(10.0);
                    let reset_label = match region {
                        ThemePreviewRegion::Defaults => "Reset theme defaults",
                        ThemePreviewRegion::Inspector => "Reset shared panel background",
                        _ => "Reset component",
                    };
                    if ui.add(egui::Button::new(reset_label).themed(&c))
                        .on_hover_text(match region {
                            ThemePreviewRegion::Defaults => "Restore the shared palette, text size and corners shown above for this variant.",
                            ThemePreviewRegion::Inspector => "Restore the background shared by Quick Inspector, Settings and other panels for this variant.",
                            _ => "Restore the component values shown above for this variant.",
                        }).clicked() {
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
                    if region == ThemePreviewRegion::Inspector {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Shared window geometry", c, |ui| {
                            ui.label(RichText::new("Shared by Ottrin windows. Reset shared panel background only restores the panel color.").small().color(c.text_dim));
                            let mut changed = number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                            changed |= window_border_width_row(ui, &mut self.theme_editor.custom.window_border_width, seed.window_border_width, c);
                            changed
                        });
                    }
                    if region == ThemePreviewRegion::Accent {
                        ui.add_space(10.0);
                        changed |= property_group(ui, "Shared menu geometry", c, |ui| {
                            ui.label(RichText::new("Surface corners round the menu frame. Border thickness is shared with windows. Reset component leaves both values unchanged.").small().color(c.text_dim));
                            let mut changed = number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                            changed |= window_border_width_row(ui, &mut self.theme_editor.custom.window_border_width, seed.window_border_width, c);
                            changed
                        });
                    }
                    if region != ThemePreviewRegion::Defaults {
                        ui.add_space(14.0);
                        changed |= property_group(ui, "Shared colors", c, |ui| {
                            let hint = if region == ThemePreviewRegion::Inspector {
                                "These colors are shared. Reset shared panel background only restores the panel color."
                            } else {
                                "These values also affect other components. Reset component leaves them unchanged."
                            };
                            ui.label(RichText::new(hint).small().color(c.text_dim));
                            let mut changed = false;
                            for &field in Self::theme_preview_region_shared_fields(region) {
                                changed |= self.appearance_color_row(ui, field, &seed);
                            }
                            changed
                        });
                        ui.add_space(6.0);
                        changed |= property_group(ui, "Shared typography", c, |ui| {
                            let mut group_changed = false;
                        group_changed |= number_row(ui, "Text size", &mut self.theme_editor.custom.font_scale, 0.85..=1.35, seed.font_scale, 0.01, c);
                            group_changed
                        });
                        if matches!(region, ThemePreviewRegion::Inspector | ThemePreviewRegion::Navigation | ThemePreviewRegion::Titlebar | ThemePreviewRegion::Borders | ThemePreviewRegion::Bookmarks | ThemePreviewRegion::Sidebar | ThemePreviewRegion::StatusBar)
                            || (region == ThemePreviewRegion::SelectedRow && self.theme_editor.preview_view == ViewMode::Grid) {
                            ui.add_space(6.0);
                            changed |= property_group(ui, "Shared geometry", c, |ui| {
                            let mut group_changed = false;
                            match region {
                                ThemePreviewRegion::Inspector => {
                                    ui.label(RichText::new("Shared surface corners: worksheet controls, inputs, cards and Grid tiles. Document pages keep their own colors.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                }
                                ThemePreviewRegion::Sidebar => {
                                    group_changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                                }
                                ThemePreviewRegion::StatusBar => {
                                    ui.label(RichText::new("Window corners shape the lower window edge. Button corners shape hover and focus backgrounds. Reset component leaves both shared settings unchanged.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                                    group_changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                                }
                                ThemePreviewRegion::SelectedRow => {
                                    ui.label(RichText::new("Shared surface corners affect Grid tiles, tabs, inputs and cards. List and Column rows stay square.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                }
                                ThemePreviewRegion::Navigation => {
                                    ui.label(RichText::new("Shared surface corners: tabs, inputs, cards and Grid tiles.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                }
                                ThemePreviewRegion::Bookmarks => {
                                    ui.label(RichText::new("Shared surface corners: location cards, tabs, inputs and Grid tiles.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Surface corners", &mut self.theme_editor.custom.border_radius, 0.0..=14.0, seed.border_radius, 1.0, c);
                                    ui.label(RichText::new("Button corners: collapsed location icons and other buttons.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Button corners", &mut self.theme_editor.custom.button_radius, 0.0..=14.0, seed.button_radius, 1.0, c);
                                }
                                _ => {
                                    ui.label(RichText::new("Window corners: main window, Settings, editor and Inspector.").small().color(c.text_dim));
                                    group_changed |= number_row(ui, "Window corners", &mut self.theme_editor.custom.app_radius, 0.0..=24.0, seed.app_radius, 1.0, c);
                                }
                            }
                            group_changed
                        });
                        }
                    }
                });
            });
            ui.scope_builder(UiBuilder::new().max_rect(footer), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    themed_menu_button(ui, "More", c, |ui| {
                        if ui.add_enabled(!self.theme_editor_is_dirty(), aligned_menu_button("", "", "Import theme…", &c).themed(&c)).on_hover_text("Import into an unchanged draft. Save or discard your changes first.").clicked() { self.import_theme_editor_json(ctx); ui.close(); }
                        if menu_item(ui, "", "Export theme…", &c).clicked() { self.export_theme_editor_json(ctx); ui.close(); }
                        if menu_item(ui, "", "Reset theme to starting values", &c).on_hover_text("Restore all properties in both Light and Dark variants to their values when this editing session opened.").clicked() {
                            self.theme_editor.custom=seed.clone(); self.theme_editor.other_variant=self.theme_editor.initial_other_variant.clone(); self.sync_theme_editor_hex_inputs(); changed=true; ui.close();
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new(if creating { "Create theme" } else { "Save changes" }).strong()).stroke(Stroke::new(1.0, c.accent)).themed(&c)) .clicked() {
                            self.save_theme_editor(ctx);
                        }
                        if !creating && ui.add(egui::Button::new("Save as new").themed(&c)).clicked() {
                            self.save_theme_editor_as_inner(ctx);
                        }
                        if ui.add(egui::Button::new("Cancel").themed(&c)).on_hover_text("Discard this session and restore the previously active theme.").clicked() { self.cancel_theme_editor(ctx); }
                    });
                });
                if let Some((message, ok, _)) = &self.theme_status {
                    ui.label(RichText::new(message).small().color(if *ok { c.accent } else { c.error }));
                }
            });
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
            let label = Self::theme_color_field_label(field);
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
                        ui.add(egui::Label::new(label).wrap())
                            .on_hover_text(Self::theme_color_field_hint(field));
                    },
                );
                let mut color = rgba;
                if compact_color_picker(ui, label, &mut color, self.colors).changed() {
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
                            .text_color(if valid_hex { self.colors.text } else { self.colors.error }).themed(&self.colors).accessible_name(format!("{label} (hex)")),
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
                let restore = appearance_icon_button(ui, MI_SETTINGS_BACKUP_RESTORE, self.colors)
                    .on_hover_text("Restore this color to its value when the editor opened");
                name_control(&restore, format!("Restore {label}"));
                if restore.clicked() {
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
fn compact_color_picker(
    ui: &mut egui::Ui,
    label: &str,
    rgba: &mut [u8; 4],
    c: Colors,
) -> egui::Response {
    use egui::ecolor::{Hsva, HsvaGamma};
    let popup_id = ui.make_persistent_id("appearance_color_picker");
    let hue_id = popup_id.with("gray_hue");
    let (swatch, response) =
        ui.allocate_exact_size(Vec2::new(28.0 * c.font_scale, 22.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::ColorButton));
    name_control(&response, format!("Choose {label}"));
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
                    let channel = ui.add(egui::Slider::new(&mut rgba[index], 0..=255));
                    name_control(&channel, label);
                    if channel.changed() {
                        response.mark_changed();
                    }
                });
            }
            let mut opacity = rgba[3] as f32 * 100.0 / 255.0;
            ui.horizontal(|ui| {
                ui.add_sized([60.0 * c.font_scale, 22.0], egui::Label::new("Opacity"));
                let slider = ui.add(
                    egui::Slider::new(&mut opacity, 0.0..=100.0)
                        .step_by(1.0)
                        .suffix("%"),
                );
                name_control(&slider, "Opacity");
                if slider.changed() {
                    rgba[3] = (opacity * 255.0 / 100.0).round() as u8;
                    response.mark_changed();
                }
            });
        });
    response
}

fn window_border_width_row(ui: &mut egui::Ui, value: &mut f32, default: f32, c: Colors) -> bool {
    number_row(
        ui,
        "Window border thickness",
        value,
        0.0..=4.0,
        default,
        0.25,
        c,
    )
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
            let number = ui
                .add_sized(
                    [value_width, 28.0],
                    egui::DragValue::new(value)
                        .range(range)
                        .speed(step)
                        .fixed_decimals(if step < 1.0 { 2 } else { 0 })
                        .suffix(match title {
                            "Text size" => "×",
                            "Neutral text blend" => "",
                            _ => " px",
                        }),
                )
                .on_hover_text(if title == "Neutral text blend" {
                    "Blend category-colored labels toward neutral text: 0 is the category color, 1 is neutral file/folder text. Neutral labels are unchanged."
                } else {
                    "Drag or type a value"
                });
            name_control(&number, title);
            changed |= number.changed();
            let restore = appearance_icon_button(ui, MI_SETTINGS_BACKUP_RESTORE, c)
                .on_hover_text("Restore this value to its value when the editor opened");
            name_control(&restore, format!("Restore {title}"));
            if restore.clicked() {
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
    // Match the miniature's 7-point action to the actual 28-point button.
    // Window geometry uses the same scale instead of full-size theme values.
    let scale = 7.0 / 28.0;
    let window_radius = (f32::from(preview.app_radius) * scale).round() as u8;
    p.rect_filled(thumb, window_radius, preview.bg);
    let title = Rect::from_min_size(thumb.min, Vec2::new(thumb.width(), 10.0));
    p.rect_filled(
        title,
        egui::CornerRadius {
            nw: window_radius,
            ne: window_radius,
            sw: 0,
            se: 0,
        },
        preview.titlebar_bg,
    );
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
            p.rect_filled(row_rect, 0.0, preview.selected_bg);
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
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: window_radius,
            se: window_radius,
        },
        preview.statusbar_bg,
    );
    p.rect_stroke(
        thumb,
        window_radius,
        Stroke::new(preview.window_border_width * scale, preview.window_border),
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
    let label_job = crate::widget_style::single_line_text_job(
        name,
        FontId::proportional(12.0 * c.font_scale),
        c.text,
        label_width,
    );
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

pub(super) fn settings_surface(c: Colors) -> Frame {
    Frame::new()
        .fill(c.panel)
        .stroke(Stroke::new(1.0, c.border))
        .corner_radius(c.border_radius)
        .inner_margin(12)
}

/// One consistent property surface for component colors, geometry and shared values.
fn property_group(
    ui: &mut egui::Ui,
    title: &str,
    c: Colors,
    body: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    settings_surface(c)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(
                RichText::new(title)
                    .strong()
                    .size(16.0 * c.font_scale)
                    .color(c.heading),
            );
            ui.painter().hline(
                ui.available_rect_before_wrap().x_range(),
                ui.cursor().min.y,
                Stroke::new(1.0, c.border),
            );
            ui.add_space(6.0);
            body(ui)
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_component_reveals_its_properties_without_resetting_ordinary_edits() {
        use egui::accesskit::{Action, ActionRequest, Role};
        fn render(
            app: &mut OttrinApp,
            ctx: &Context,
            events: Vec<egui::Event>,
            time: &mut f64,
        ) -> egui::FullOutput {
            *time += 1.0 / 60.0;
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 780.0),
                    )),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            )
        }
        fn text_rect(output: &egui::FullOutput, label: &str) -> Option<(Rect, Rect)> {
            output.shapes.iter().find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == label => {
                    Some((t.galley.rect.translate(t.pos.to_vec2()), s.clip_rect))
                }
                _ => None,
            })
        }
        fn action(
            output: &egui::FullOutput,
            label: &str,
            role: Role,
            action: Action,
        ) -> egui::Event {
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes;
            let targets: Vec<_> = nodes
                .iter()
                .filter(|(_, n)| n.role() == role && n.label() == Some(label))
                .collect();
            assert_eq!(targets.len(), 1, "One actual named control: {label}");
            egui::Event::AccessKitActionRequest(ActionRequest {
                action,
                target: targets[0].0,
                data: None,
            })
        }
        fn scroll(app: &mut OttrinApp, ctx: &Context, time: &mut f64) -> egui::FullOutput {
            let _ = render(
                app,
                ctx,
                vec![
                    egui::Event::PointerMoved(egui::pos2(1000.0, 500.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, -900.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                time,
            );
            let mut output = render(app, ctx, vec![], time);
            for _ in 0..60 {
                output = render(app, ctx, vec![], time);
            }
            output
        }
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut time = 0.0;
            let mut app = OttrinApp::default();
            app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
            app.switch_theme_editor_variant(mode, &ctx);
            app.theme_editor.custom.font_scale = 1.35;
            app.theme_editor.preview_inspector = true;
            app.theme_editor.selected_region = ThemePreviewRegion::Inspector;
            app.apply_theme_editor_live_preview(&ctx);
            for _ in 0..3 {
                let _ = render(&mut app, &ctx, vec![], &mut time);
            }
            let initial = render(&mut app, &ctx, vec![], &mut time);
            let (hint, clip) = text_rect(
                &initial,
                OttrinApp::theme_preview_region_hint(ThemePreviewRegion::Inspector),
            )
            .expect("Initial description is visible");
            assert!(clip.contains_rect(hint));
            let output = scroll(&mut app, &ctx, &mut time);
            assert!(
                text_rect(
                    &output,
                    OttrinApp::theme_preview_region_hint(ThemePreviewRegion::Inspector)
                )
                .is_none_or(|(hint, clip)| hint.bottom() < clip.top()),
                "Exercise an actually scrolled property pane"
            );
            let event = action(&output, "File manager", Role::Button, Action::Click);
            let _ = render(&mut app, &ctx, vec![event], &mut time);
            let output = render(&mut app, &ctx, vec![], &mut time);
            assert_eq!(
                app.theme_editor.selected_region,
                ThemePreviewRegion::Defaults
            );
            let (hint, clip) = text_rect(
                &output,
                OttrinApp::theme_preview_region_hint(ThemePreviewRegion::Defaults),
            )
            .expect("Selecting a different component must paint its description in view");
            assert!(
                clip.contains_rect(hint),
                "Selecting a different component must reveal its entire description"
            );
            let output = scroll(&mut app, &ctx, &mut time);
            let (before, clip) =
                text_rect(&output, "Window corners").expect("Scrolled numeric control is visible");
            assert!(clip.contains_rect(before));
            let radius = app.theme_editor.custom.app_radius;
            let event = action(
                &output,
                "Window corners",
                Role::SpinButton,
                Action::Increment,
            );
            let _ = render(&mut app, &ctx, vec![event], &mut time);
            let output = render(&mut app, &ctx, vec![], &mut time);
            assert!(
                app.theme_editor.custom.app_radius > radius,
                "Exercise an actual property edit"
            );
            let (after, _) =
                text_rect(&output, "Window corners").expect("Scrolled numeric control is visible");
            assert!(
                (before.top() - after.top()).abs() < 1.0,
                "An ordinary edit must preserve the scrolled position"
            );
            let event = action(&output, "Quick Inspector", Role::Button, Action::Click);
            let _ = render(&mut app, &ctx, vec![event], &mut time);
            let output = scroll(&mut app, &ctx, &mut time);
            let (title, _) = text_rect(&output, "Quick Inspector — notes.txt")
                .expect("Actual preview title remains visible");
            let pos = title.center();
            let _ = render(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                &mut time,
            );
            let output = render(&mut app, &ctx, vec![], &mut time);
            assert_eq!(
                app.theme_editor.selected_region,
                ThemePreviewRegion::Titlebar
            );
            let (hint, clip) = text_rect(
                &output,
                OttrinApp::theme_preview_region_hint(ThemePreviewRegion::Titlebar),
            )
            .expect("Preview element selection reveals the new component description");
            assert!(clip.contains_rect(hint));
        }
    }

    #[test]
    fn shared_geometry_follows_component_reset_and_keeps_its_ownership() {
        use egui::accesskit::{Action, ActionRequest, Role};
        fn render(
            app: &mut OttrinApp,
            ctx: &Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 1800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            )
        }
        fn activate(app: &mut OttrinApp, ctx: &Context, label: &str) {
            let output = render(app, ctx, vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let targets: Vec<_> = nodes
                .iter()
                .filter(|(_, n)| n.role() == Role::Button && n.label() == Some(label))
                .collect();
            assert_eq!(targets.len(), 1, "One actual named action: {label}");
            let _ = render(
                app,
                ctx,
                vec![egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target: targets[0].0,
                    data: None,
                })],
            );
        }
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut app = OttrinApp::default();
            app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
            app.switch_theme_editor_variant(mode, &ctx);
            app.theme_editor.selected_region = ThemePreviewRegion::Inspector;
            app.theme_editor.preview_inspector = true;
            let seed = app.theme_editor.initial_custom.clone();
            app.theme_editor.custom.panel = [23, 61, 97, 255];
            app.theme_editor.custom.app_radius = 21.0;
            app.theme_editor.custom.window_border_width = 3.5;
            app.theme_editor.custom.titlebar = [33, 101, 67, 255];
            app.apply_theme_editor_live_preview(&ctx);
            let output = render(&mut app, &ctx, vec![]);
            let y = |label: &str| {
                output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::epaint::Shape::Text(t) if t.galley.job.text == label => Some(t.pos.y),
                        _ => None,
                    })
                    .next()
                    .unwrap_or_else(|| panic!("Missing actual section/action: {label}"))
            };
            assert!(
                y("Reset shared panel background") < y("Window corners"),
                "Shared geometry must follow the component reset, consistently with other shared settings"
            );
            activate(&mut app, &ctx, "Reset shared panel background");
            assert_eq!(app.theme_editor.custom.panel, seed.panel);
            assert_eq!(app.theme_editor.custom.app_radius, 21.0);
            assert_eq!(app.theme_editor.custom.window_border_width, 3.5);
            assert_eq!(app.theme_editor.custom.titlebar, [33, 101, 67, 255]);
            activate(&mut app, &ctx, "Restore Window corners");
            assert_eq!(app.theme_editor.custom.app_radius, seed.app_radius);
            assert_eq!(app.theme_editor.custom.window_border_width, 3.5);
            activate(&mut app, &ctx, "Restore Window border thickness");
            assert_eq!(
                app.theme_editor.custom.window_border_width,
                seed.window_border_width
            );
            assert_eq!(app.theme_editor.custom.titlebar, [33, 101, 67, 255]);

            app.theme_editor.selected_region = ThemePreviewRegion::StatusBar;
            app.theme_editor.preview_inspector = false;
            app.theme_editor.custom.statusbar = [23, 61, 97, 255];
            app.theme_editor.custom.app_radius = 21.0;
            app.theme_editor.custom.button_radius = 13.0;
            app.apply_theme_editor_live_preview(&ctx);
            activate(&mut app, &ctx, "Reset component");
            assert_eq!(app.theme_editor.custom.statusbar, seed.statusbar);
            assert_eq!(app.theme_editor.custom.app_radius, 21.0);
            assert_eq!(app.theme_editor.custom.button_radius, 13.0);
            activate(&mut app, &ctx, "Restore Window corners");
            assert_eq!(app.theme_editor.custom.app_radius, seed.app_radius);
            activate(&mut app, &ctx, "Restore Button corners");
            assert_eq!(app.theme_editor.custom.button_radius, seed.button_radius);

            app.theme_editor.selected_region = ThemePreviewRegion::Accent;
            app.theme_editor.custom.button_radius = 12.0;
            app.theme_editor.custom.border_radius = 13.0;
            app.theme_editor.custom.window_border_width = 3.5;
            app.theme_editor.custom.panel = [23, 61, 97, 255];
            app.theme_editor.custom.window_border = [117, 42, 76, 255];
            app.apply_theme_editor_live_preview(&ctx);
            activate(&mut app, &ctx, "Reset component");
            assert_eq!(app.theme_editor.custom.button_radius, seed.button_radius);
            assert_eq!(app.theme_editor.custom.border_radius, 13.0);
            assert_eq!(app.theme_editor.custom.window_border_width, 3.5);
            assert_eq!(app.theme_editor.custom.panel, [23, 61, 97, 255]);
            assert_eq!(app.theme_editor.custom.window_border, [117, 42, 76, 255]);
            activate(&mut app, &ctx, "Restore Surface corners");
            assert_eq!(app.theme_editor.custom.border_radius, seed.border_radius);
        }
    }

    #[test]
    fn appearance_numeric_metadata_matches_each_typed_commit() {
        use egui::accesskit::{Action, ActionRequest, Role};

        fn render(
            app: &mut OttrinApp,
            ctx: &Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 1600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            )
        }
        fn numeric(output: egui::FullOutput) -> (egui::accesskit::NodeId, f64, bool, bool) {
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let fields: Vec<_> = nodes
                .into_iter()
                .filter(|(_, n)| {
                    n.role() == Role::SpinButton && n.label() == Some("Window corners")
                })
                .collect();
            assert_eq!(fields.len(), 1);
            (
                fields[0].0,
                fields[0].1.numeric_value().unwrap(),
                fields[0].1.supports_action(Action::Increment),
                fields[0].1.supports_action(Action::Decrement),
            )
        }

        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            ctx.set_pixels_per_point(1.5);
            let mut app = OttrinApp::default();
            app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
            app.switch_theme_editor_variant(mode, &ctx);
            app.theme_editor.selected_region = ThemePreviewRegion::Defaults;
            app.theme_editor.custom.app_radius = 0.0;
            app.theme_editor.initial_custom.app_radius = 0.0;
            app.apply_theme_editor_live_preview(&ctx);
            let (id, value, increment, decrement) = numeric(render(&mut app, &ctx, vec![]));
            assert_eq!(value, 0.0);
            assert!(increment);
            assert!(!decrement);
            let _ = render(
                &mut app,
                &ctx,
                vec![egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Focus,
                    target: id,
                    data: None,
                })],
            );
            for (text, expected) in [("2", 2.0), ("4", 24.0)] {
                let output = render(&mut app, &ctx, vec![egui::Event::Text(text.into())]);
                assert_eq!(
                    app.theme_editor.custom.app_radius, expected,
                    "Actual typed model value"
                );
                assert_eq!(
                    app.colors.app_radius, expected as u8,
                    "Actual live theme value"
                );
                let (_, native_value, increment, decrement) = numeric(output);
                assert_eq!(
                    native_value,
                    f64::from(expected),
                    "Native numeric value must describe the committed input frame"
                );
                assert_eq!(
                    increment,
                    expected < 24.0,
                    "Increment must match the committed range boundary"
                );
                assert_eq!(
                    decrement,
                    expected > 0.0,
                    "Decrement must match the committed range boundary"
                );
            }
        }
    }

    #[test]
    fn appearance_numeric_controls_expose_their_visible_names() {
        use egui::accesskit::Role;

        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for pixels_per_point in [1.0, 1.5] {
                let ctx = Context::default();
                ctx.enable_accesskit();
                ctx.set_pixels_per_point(pixels_per_point);
                let mut app = OttrinApp::default();
                app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
                app.switch_theme_editor_variant(mode, &ctx);
                app.theme_editor.custom.window_border_width = 1.25;
                let border_color = Color32::from_rgb(203, 58, 117);
                app.theme_editor.custom.window_border = border_color.to_array();
                app.apply_theme_editor_live_preview(&ctx);
                assert_eq!(app.colors.window_border_width, 1.25);
                let mut names = std::collections::BTreeSet::new();
                for region in REGIONS {
                    app.theme_editor.selected_region = region;
                    app.theme_editor.preview_inspector = region == ThemePreviewRegion::Inspector;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(1180.0, 1600.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            app.render_theme_editor_embedded(ctx);
                        },
                    );
                    if matches!(
                        region,
                        ThemePreviewRegion::Borders
                            | ThemePreviewRegion::Inspector
                            | ThemePreviewRegion::Accent
                    ) {
                        assert!(output.shapes.iter().any(|shape| matches!(
                            &shape.shape, egui::Shape::Text(text) if text.galley.job.text == "1.25 px"
                        )), "Window border thickness must display the full quarter-pixel value in {region:?}");
                    }
                    assert!(
                        output.shapes.iter().any(|shape| matches!(
                            &shape.shape, egui::Shape::Rect(rect)
                                if rect.stroke == Stroke::new(1.25, border_color)
                        )),
                        "Preview must paint the exact selected border in {region:?}"
                    );
                    if matches!(
                        region,
                        ThemePreviewRegion::Defaults
                            | ThemePreviewRegion::Inspector
                            | ThemePreviewRegion::Borders
                    ) {
                        let frame = output
                            .shapes
                            .iter()
                            .find_map(|shape| match &shape.shape {
                                egui::Shape::Rect(rect)
                                    if rect.stroke == Stroke::new(1.25, border_color) =>
                                {
                                    Some(rect)
                                }
                                _ => None,
                            })
                            .unwrap();
                        let selection_width = if region == ThemePreviewRegion::Defaults {
                            1.0
                        } else {
                            2.0
                        };
                        assert!(output.shapes.iter().any(|shape| matches!(
                            &shape.shape, egui::Shape::Rect(rect)
                                if rect.stroke == Stroke::new(selection_width, app.colors.accent)
                                    && rect.stroke_kind == egui::StrokeKind::Outside
                                    && rect.rect.contains_rect(frame.rect.expand(2.0))
                        )), "Frame selection must leave the actual border visible in {region:?}");
                    }
                    for (_, node) in output.platform_output.accesskit_update.unwrap().nodes {
                        if node.role() == Role::SpinButton {
                            if node.label() == Some("Window border thickness") {
                                assert_eq!(node.numeric_value_step(), Some(0.25), "{region:?}");
                            }
                            names.insert(node.label().unwrap_or_default().to_owned());
                        }
                    }
                }
                for title in [
                    "Text size",
                    "Window corners",
                    "Surface corners",
                    "Button corners",
                    "Window border thickness",
                    "Neutral text blend",
                ] {
                    assert!(
                        names.contains(title),
                        "Missing native numeric name {title}: {names:?}"
                    );
                }
                assert!(!names.contains(""), "Unnamed appearance numeric control");
            }
        }
    }

    #[test]
    fn named_window_corner_actions_edit_and_restore_the_actual_theme() {
        use egui::accesskit::{Action, ActionData, ActionRequest, Role};

        fn render(
            app: &mut OttrinApp,
            ctx: &Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 1600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            )
        }

        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            ctx.set_pixels_per_point(1.5);
            let mut app = OttrinApp::default();
            app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
            app.switch_theme_editor_variant(mode, &ctx);
            app.theme_editor.selected_region = ThemePreviewRegion::Defaults;
            let seed = app.theme_editor.custom.app_radius;
            let output = render(&mut app, &ctx, vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let fields: Vec<_> = nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == Role::SpinButton && node.label() == Some("Window corners")
                })
                .collect();
            assert_eq!(fields.len(), 1, "One named Window corners native control");
            let (id, node) = fields[0];
            assert_eq!(node.numeric_value(), Some(f64::from(seed)));
            assert!(node.supports_action(Action::SetValue));
            let output = render(
                &mut app,
                &ctx,
                vec![egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::SetValue,
                    target: *id,
                    data: Some(ActionData::NumericValue(24.0)),
                })],
            );
            assert_eq!(app.theme_editor.custom.app_radius, 24.0);
            assert_eq!(app.colors.app_radius, 24);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            let resets: Vec<_> = nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == Role::Button && node.label() == Some("Restore Window corners")
                })
                .collect();
            assert_eq!(resets.len(), 1, "One specifically named restore button");
            assert!(resets[0].1.supports_action(Action::Click));
            let _ = render(
                &mut app,
                &ctx,
                vec![egui::Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target: resets[0].0,
                    data: None,
                })],
            );
            assert_eq!(app.theme_editor.custom.app_radius, seed);
            assert_eq!(app.colors.app_radius, seed as u8);
        }
    }

    #[test]
    fn rendered_theme_fields_name_the_theme_and_each_visible_color() {
        use egui::accesskit::{Action, ActionData, ActionRequest, Role};

        let render = |app: &mut OttrinApp, ctx: &Context, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 1600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.render_theme_editor_embedded(ctx);
                },
            )
        };
        let node_id = |output: &egui::FullOutput, role, label: &str, action| {
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == role
                        && node.label() == Some(label)
                        && node.supports_action(action)
                })
                .unwrap_or_else(|| panic!("Missing {label} control for {action:?}"))
                .0
        };
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut app = OttrinApp::default();
            app.open_theme_editor_for_preset(ThemePreset::Ottrin, &ctx);
            app.switch_theme_editor_variant(mode, &ctx);
            app.theme_editor.selected_region = ThemePreviewRegion::Navigation;
            let output = render(&mut app, &ctx, vec![]);
            let fields = super::super::tests::rendered_text_fields(&output);
            assert!(
                fields.contains(&("Theme name".into(), app.theme_editor.save_as_name.clone())),
                "{fields:?}"
            );
            let colors: Vec<_> = fields
                .iter()
                .filter(|(name, _)| name.ends_with(" (hex)"))
                .collect();
            assert!(!colors.is_empty(), "{fields:?}");
            assert!(
                colors
                    .iter()
                    .all(|(_, value)| parse_hex_rgb(value).is_some())
            );
            assert!(
                fields.iter().all(|(name, _)| !name.is_empty()),
                "{fields:?}"
            );

            let field = ThemeColorField::Toolbar;
            let label = OttrinApp::theme_color_field_label(field);
            let seed = OttrinApp::theme_color_field_value(&app.theme_editor.custom, field);
            let choose = node_id(
                &output,
                Role::ColorWell,
                &format!("Choose {label}"),
                Action::Click,
            );
            let restore = node_id(
                &output,
                Role::Button,
                &format!("Restore {label}"),
                Action::Click,
            );
            let action = |target, action, data| {
                egui::Event::AccessKitActionRequest(ActionRequest {
                    target,
                    action,
                    data,
                })
            };
            let output = render(&mut app, &ctx, vec![action(choose, Action::Click, None)]);
            let red = node_id(&output, Role::Slider, "Red", Action::SetValue);
            let _ = node_id(&output, Role::Slider, "Green", Action::SetValue);
            let _ = node_id(&output, Role::Slider, "Blue", Action::SetValue);
            let opacity = node_id(&output, Role::Slider, "Opacity", Action::SetValue);
            let _ = render(
                &mut app,
                &ctx,
                vec![
                    action(red, Action::SetValue, Some(ActionData::NumericValue(147.0))),
                    action(
                        opacity,
                        Action::SetValue,
                        Some(ActionData::NumericValue(50.0)),
                    ),
                ],
            );
            let edited = [147, seed[1], seed[2], 128];
            assert_eq!(
                OttrinApp::theme_color_field_value(&app.theme_editor.custom, field),
                edited
            );
            assert_eq!(
                app.colors.toolbar_bg,
                Color32::from_rgba_unmultiplied(147, seed[1], seed[2], 128)
            );
            let _ = render(&mut app, &ctx, vec![action(restore, Action::Click, None)]);
            assert_eq!(
                OttrinApp::theme_color_field_value(&app.theme_editor.custom, field),
                seed
            );
        }
    }

    #[test]
    fn preview_uses_real_smart_panel_icons_and_toggle_behavior() {
        use egui::accesskit::{Action, ActionRequest};

        let ctx = Context::default();
        ctx.enable_accesskit();
        let c = Colors::preset(ThemePreset::Nord, true);
        let render = |mut view, events| {
            let mut picked = None;
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
                        picked = theme_editor_large_preview(
                            ui,
                            c,
                            ThemePreviewRegion::Sidebar,
                            &mut view,
                        )
                        .1;
                    });
                },
            );
            (output, picked)
        };
        let (output, _) = render(ViewMode::List, vec![]);
        let activate_info = |output: &egui::FullOutput| {
            let info = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some("Show or hide info panel")
                        && node.supports_action(Action::Click)
                })
                .unwrap()
                .0;

            vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target: info,
                data: None,
            })]
        };
        assert_eq!(
            render(ViewMode::List, activate_info(&output)).1,
            Some(ThemePreviewRegion::Sidebar)
        );
        let (info_panel, _) = render(ViewMode::List, vec![]);
        assert!(info_panel.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Text(text) if text.galley.job.text == "Info")));
        assert_eq!(
            render(ViewMode::List, activate_info(&info_panel)).1,
            Some(ThemePreviewRegion::Sidebar)
        );
        let (output, _) = render(ViewMode::Miller, vec![]);
        for icon in [
            MI_DESCRIPTION,
            MI_MOVE_TO_INBOX,
            MI_SEARCH,
            MI_HISTORY,
            MI_DELETE,
        ] {
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == icon)), "Missing real Smart Panel icon {icon}");
        }
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if ["Details", "Target", "Preview"].contains(&t.galley.job.text.as_str()))));
        let click = |icon| {
            let pos = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(t) if t.galley.job.text == icon => {
                        Some(t.pos + t.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .max_by(|left, right| left.x.total_cmp(&right.x))
                .unwrap();
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        assert_eq!(
            render(ViewMode::Miller, click(MI_SEARCH)).1,
            Some(ThemePreviewRegion::Sidebar)
        );
        let (expanded, _) = render(ViewMode::Miller, vec![]);
        assert!(expanded.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == "Search")), "The real header appears when Search opens");
        assert!(expanded.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Rect(r) if r.fill == c.selected_bg && r.rect.width() == SMART_PANEL_STRIP_WIDTH && r.corner_radius.nw == c.button_radius)));
        render(ViewMode::Miller, click(MI_SEARCH));
        let (collapsed, _) = render(ViewMode::Miller, vec![]);
        assert!(!collapsed.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == "Search")), "Clicking the active icon collapses its panel");

        for view in [ViewMode::List, ViewMode::Grid, ViewMode::Miller] {
            render(view, vec![]);
            assert_eq!(
                render(view, click(MI_DESCRIPTION)).1,
                Some(ThemePreviewRegion::Sidebar)
            );
            let (info, _) = render(view, vec![]);
            let title_visible = |title| {
                info.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == title)
            })
            };
            assert_eq!(title_visible("Info"), view != ViewMode::Miller);
            assert!(
                !title_visible("Trash"),
                "Info must never show the Trash header"
            );
            render(view, click(MI_DESCRIPTION));
        }
    }

    #[test]
    fn locations_preview_expands_and_uses_the_live_tile_styles() {
        use egui::accesskit::{Action, ActionRequest};

        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut colors = Colors::preset(ThemePreset::Ottrin, true);
        colors.button_radius = 13;
        colors.border_radius = 4;
        colors.selected_bg = Color32::from_rgb(171, 69, 121);
        let render = |events| {
            let mut picked = None;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        picked = theme_editor_large_preview(
                            ui,
                            colors,
                            ThemePreviewRegion::Bookmarks,
                            &mut ViewMode::Miller,
                        )
                        .1;
                    });
                },
            );
            (output, picked)
        };
        let activate = |output: &egui::FullOutput, name: &str| {
            let (id, _) = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(name) && node.supports_action(Action::Click))
                .unwrap_or_else(|| panic!("Missing {name}"));
            vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target: *id,
                data: None,
            })]
        };
        let selected_tile = |output: &egui::FullOutput, height: f32, corners| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(r)
                        if r.fill == colors.selected_bg.gamma_multiply(0.62)
                            && r.rect.height() == height
                            && r.corner_radius.nw == corners =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .expect("Selected location tile uses the live fill, height and corners")
        };

        let (collapsed, _) = render(vec![]);
        selected_tile(&collapsed, 36.0, colors.button_radius);
        assert_eq!(
            render(activate(&collapsed, "Expand locations rail")).1,
            Some(ThemePreviewRegion::Bookmarks)
        );
        let (expanded, _) = render(vec![]);
        let home = selected_tile(&expanded, 52.0, colors.border_radius);
        assert!(expanded.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::epaint::Shape::Text(text) if text.galley.job.text == "/home/user/Documents")));
        assert_eq!(
            render(activate(&expanded, "Documents · /home/user/Documents")).1,
            Some(ThemePreviewRegion::Bookmarks)
        );
        let (selected, _) = render(vec![]);
        assert!(selected_tile(&selected, 52.0, colors.border_radius).top() > home.bottom());
        assert_eq!(
            render(activate(&selected, "Collapse locations rail")).1,
            Some(ThemePreviewRegion::Bookmarks)
        );
        selected_tile(&render(vec![]).0, 36.0, colors.button_radius);
    }

    #[test]
    fn settings_navigation_exposes_named_choices_and_accepts_native_activation() {
        use egui::accesskit::{Action, ActionRequest, Role, Toggled};

        for dark in [false, true] {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut app = OttrinApp::default();
            app.colors = Colors::preset(ThemePreset::Ottrin, dark);
            app.show_settings = true;
            app.settings_tab = SettingsTab::General;
            let mut render = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(920.0, 640.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        app.render_settings_modal_embedded(ctx);
                    },
                )
            };
            let output = render(vec![]);
            let nodes = &output.platform_output.accesskit_update.unwrap().nodes;
            let mut services = None;
            for label in [
                "General",
                "Appearance",
                "Files",
                "Services",
                "Search",
                "Cache",
            ] {
                let choices: Vec<_> = nodes
                    .iter()
                    .filter(|(_, node)| {
                        node.role() == Role::RadioButton && node.label() == Some(label)
                    })
                    .collect();
                assert_eq!(choices.len(), 1, "One native navigation choice for {label}");
                let (id, node) = choices[0];
                assert!(node.supports_action(Action::Click), "{label}");
                assert_eq!(
                    node.toggled(),
                    Some(if label == "General" {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                );
                if label == "Services" {
                    services = Some(*id);
                }
            }
            let _ = render(vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target: services.unwrap(),
                data: None,
            })]);
            // The same event and response used by native adapters must switch the
            // actual Settings page, not merely a detached preview widget.
            let output = render(vec![]);
            let nodes = output.platform_output.accesskit_update.unwrap().nodes;
            assert!(nodes.iter().any(|(_, node)| {
                node.role() == Role::RadioButton
                    && node.label() == Some("Services")
                    && node.toggled() == Some(Toggled::True)
            }));
            assert_eq!(app.settings_tab, SettingsTab::Services);
        }
    }

    #[test]
    fn settings_navigation_rows_keep_icon_and_text_centred_inside_their_surface() {
        for scale in [1.0, 1.35] {
            let ctx = Context::default();
            let mut c = Colors::preset(ThemePreset::Ottrin, true);
            c.font_scale = scale;
            let mut bounds = None;
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(164.0 * scale - 38.0);
                    bounds = Some(
                        settings_navigation_row(ui, MI_DARK_MODE, "Appearance", true, &c).rect,
                    );
                });
            });
            let bounds = bounds.unwrap();
            for text in output.shapes.iter().filter_map(|shape| match &shape.shape {
                egui::epaint::Shape::Text(t) => Some(t),
                _ => None,
            }) {
                let rect = Rect::from_min_size(text.pos, text.galley.size());
                assert!((rect.center().y - bounds.center().y).abs() < 1.0);
                assert!(
                    bounds.contains_rect(rect),
                    "Navigation text must stay within its row at scale {scale}"
                );
            }
        }
    }

    #[test]
    fn appearance_preview_border_clicks_do_not_activate_underlying_controls() {
        for radius in [0, 24] {
            let ctx = Context::default();
            let mut colors = Colors::preset(ThemePreset::Ottrin, true);
            colors.app_radius = radius;
            colors.window_border_width = 4.0;
            colors.window_border = Color32::from_rgb(113, 13, 149);
            let render = |colors, events| {
                let mut picked = (None, None);
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
                            picked = theme_editor_large_preview(
                                ui,
                                colors,
                                ThemePreviewRegion::ColumnBackground,
                                &mut ViewMode::Miller,
                            );
                        });
                    },
                );
                (output, picked)
            };
            let click = |pos| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            let (output, _) = render(colors, vec![]);
            let frame = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(r)
                        if r.stroke.color == colors.window_border && r.rect.width() > 500.0 =>
                    {
                        Some(r.rect)
                    }
                    _ => None,
                })
                .unwrap();
            let target = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Text(t) if t.galley.job.text == MI_MOVE_TO_INBOX => {
                        Some(t.pos + t.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .unwrap();
            for pos in [
                egui::pos2(frame.left() + 2.0, frame.center().y),
                egui::pos2(frame.center().x, frame.top() + 2.0),
                egui::pos2(frame.center().x, frame.bottom() - 2.0),
                egui::pos2(frame.right() - 2.0, target.y),
            ] {
                assert_eq!(
                    render(colors, click(pos)).1.1,
                    Some(ThemePreviewRegion::Borders)
                );
            }
            let (closed, _) = render(colors, vec![]);
            let has_panel = |output: &egui::FullOutput| {
                output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == "Drop Folder")
            })
            };
            assert!(
                !has_panel(&closed),
                "Border clicks must not open the underlying panel"
            );
            assert_eq!(
                render(colors, click(target)).1.1,
                Some(ThemePreviewRegion::Sidebar)
            );
            assert!(
                has_panel(&render(colors, vec![]).0),
                "The actual panel button must still work"
            );
            let corner = frame.min + Vec2::splat(1.0);
            assert_eq!(
                render(colors, click(corner)).1.1,
                if radius == 0 {
                    Some(ThemePreviewRegion::Borders)
                } else {
                    None
                }
            );
            colors.window_border_width = 0.0;
            assert_eq!(
                render(colors, click(egui::pos2(frame.right() - 2.0, target.y)))
                    .1
                    .0,
                Some(ThemePreviewRegion::Sidebar),
                "An invisible border must not capture content input"
            );
        }
    }

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
                        clicked =
                            theme_editor_large_preview(ui, colors, region, &mut ViewMode::Miller).1;
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
                theme_editor_large_preview(
                    ui,
                    colors,
                    ThemePreviewRegion::Navigation,
                    &mut ViewMode::Miller,
                );
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
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::epaint::Shape::Text(t) if t.galley.job.text == "workspace" && t.galley.job.sections.iter().all(|section| section.format.color == colors.text_muted))));
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
            "Shared colors",
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
                    theme_editor_large_preview(
                        ui,
                        colors,
                        ThemePreviewRegion::ColumnText,
                        &mut ViewMode::Miller,
                    );
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
                        theme_editor_large_preview(ui, c, region, &mut ViewMode::Miller);
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
    fn theme_save_failure_keeps_the_draft_open_and_never_claims_success() {
        let ctx = Context::default();
        let mut app = OttrinApp::default();
        let path = crate::config::config_path().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let malformed = b"{ invalid external settings";
        std::fs::write(&path, malformed).unwrap();
        let before = serde_json::to_value(&app.state.config).unwrap();
        app.open_theme_editor_for_preset(ThemePreset::Breeze, &ctx);
        app.theme_editor.save_as_name = "Retained draft".into();
        app.theme_editor.custom.button_radius = 13.0;
        app.save_theme_editor(&ctx);
        assert!(app.theme_editor.open);
        assert_eq!(app.theme_editor.custom.button_radius, 13.0);
        assert_eq!(serde_json::to_value(&app.state.config).unwrap(), before);
        assert!(!app.theme_status.as_ref().unwrap().1);
        assert_eq!(std::fs::read(&path).unwrap(), malformed);

        // Simulate an explicit user repair, then retry the retained draft.
        std::fs::remove_file(&path).unwrap();
        app.save_theme_editor(&ctx);
        assert!(!app.theme_editor.open);
        assert!(app.theme_status.as_ref().unwrap().1);
        assert_eq!(
            load_config().unwrap().custom_themes[0].name,
            "Retained draft"
        );

        app.open_theme_editor_for_saved(0, &ctx);
        app.theme_editor.custom.button_radius = 9.0;
        let original = std::fs::read(&path).unwrap();
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&path, readonly).unwrap();
        app.save_theme_editor(&ctx);
        assert!(app.theme_editor.open);
        assert_eq!(app.theme_editor.custom.button_radius, 9.0);
        assert!(!app.theme_status.as_ref().unwrap().1);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::set_permissions(&path, permissions).unwrap();
        app.save_theme_editor(&ctx);
        assert!(!app.theme_editor.open);
        assert!(app.theme_status.as_ref().unwrap().1);
        assert_eq!(
            Colors::for_config(&load_config().unwrap(), None).button_radius,
            9
        );
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
        let persisted = load_config().expect("valid private test config");
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
        let reopened = load_config().expect("valid private test config");
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
        assert_eq!(
            Colors::for_config(&load_config().expect("valid private test config"), None)
                .button_radius,
            3
        );
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
        let reopened = load_config().expect("valid private test config");
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
    fn quick_inspector_uses_shared_window_surface_title_and_border_roles() {
        for dark in [false, true] {
            for radius in [0, 24] {
                let ctx = Context::default();
                install_application_fonts(&ctx);
                ctx.enable_accesskit();
                let mut app = OttrinApp::default();
                app.colors = Colors::preset(ThemePreset::Ottrin, dark);
                app.colors.panel = Color32::from_rgb(40, 84, 106);
                app.colors.panel_raised = Color32::from_rgb(153, 41, 75);
                app.colors.titlebar_bg = Color32::from_rgb(23, 109, 63);
                app.colors.heading = Color32::from_rgb(244, 210, 70);
                app.colors.window_border = Color32::from_rgb(235, 113, 135);
                let mut customization = OttrinApp::custom_from_colors(app.colors);
                customization.window_border_width = 1.25;
                app.colors = Colors::from_custom(&customization);
                app.colors.app_radius = radius;
                app.preview.visible = true;
                app.preview.path = Some(PathBuf::from("/__appearance_fixture__/sample.bin"));
                app.preview.data = Some(PreviewData {
                    title: "Theme fixture".into(),
                    kind: PreviewKind::Unsupported,
                    body: "Read-only theme fixture".into(),
                    document: None,
                    spreadsheet: None,
                    open_external: false,
                });
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(720.0, 560.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render_preview_overlay(ctx),
                );
                let title = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.job.text == "Quick Inspector — Theme fixture" =>
                        {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("Actual Inspector title rendered");
                assert!(
                    title
                        .galley
                        .job
                        .sections
                        .iter()
                        .all(|s| s.format.color == app.colors.heading)
                );
                let title_center = title.pos + title.galley.rect.center().to_vec2();
                let rects: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) => Some(rect),
                        _ => None,
                    })
                    .collect();
                assert!(
                    rects
                        .iter()
                        .any(|r| r.fill == app.colors.titlebar_bg && r.rect.contains(title_center)),
                    "Title must use the shared Title bar color"
                );
                assert!(
                    rects
                        .iter()
                        .any(|r| r.fill == app.colors.panel
                            && r.rect.size() == Vec2::new(720.0, 560.0)),
                    "Inspector window background must use Panels and dialogs"
                );
                assert!(
                    !rects.iter().any(|r| r.fill == app.colors.panel_raised
                        && r.rect.size() == Vec2::new(720.0, 560.0)),
                    "Inputs / raised surfaces must not recolor the Inspector window"
                );
                assert!(
                    rects
                        .iter()
                        .any(|r| r.stroke == Stroke::new(1.25, app.colors.window_border)
                            && r.corner_radius == egui::CornerRadius::same(radius)
                            && r.rect.size() == Vec2::new(720.0, 560.0)),
                    "Inspector must use shared window border width/color and radius"
                );
                assert!(
                    output
                        .platform_output
                        .accesskit_update
                        .as_ref()
                        .unwrap()
                        .nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Close window")),
                    "Actual shared window close control must remain available"
                );
                assert!(app.preview.text_editor.is_none());
            }
        }
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
