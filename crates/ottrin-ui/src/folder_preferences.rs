//! Folder-scoped ordering and the General defaults that it overrides.
use super::*;
use ottrin_core::SortConfig;

const SORT_FIELDS: [(SortBy, &str); 5] = [
    (SortBy::Name, "Name"),
    (SortBy::Modified, "Date modified"),
    (SortBy::Kind, "Type"),
    (SortBy::Size, "Size"),
    (SortBy::Created, "Date created"),
];

fn choose_heading(mut sort: SortConfig, by: SortBy) -> SortConfig {
    if sort.by == by {
        sort.ascending = !sort.ascending;
    } else {
        sort.by = by;
        sort.ascending = default_sort_direction(by);
    }
    sort
}

fn sort_icon(by: SortBy) -> &'static str {
    match by {
        SortBy::Name => MI_FILE,
        SortBy::Modified | SortBy::Created => MI_HISTORY,
        SortBy::Kind => MI_DESCRIPTION,
        SortBy::Size => MI_STORAGE,
    }
}

fn sort_options(ui: &mut egui::Ui, sort: &mut SortConfig, c: Colors) {
    for (by, label) in SORT_FIELDS {
        if ui
            .add(aligned_menu_button(
                if sort.by == by { MI_CHECK } else { " " },
                sort_icon(by),
                label,
                &c,
            ))
            .clicked()
        {
            if sort.by != by {
                *sort = choose_heading(*sort, by);
            }
            ui.close();
        }
    }
    menu_separator(ui, c);
    for (ascending, label, icon) in [
        (true, "Ascending", MI_ARROW_UP),
        (false, "Descending", MI_ARROW_DOWN),
    ] {
        if ui
            .add(aligned_menu_button(
                if sort.ascending == ascending {
                    MI_CHECK
                } else {
                    " "
                },
                icon,
                label,
                &c,
            ))
            .clicked()
        {
            sort.ascending = ascending;
            ui.close();
        }
    }
    if ui
        .add(aligned_menu_button(
            if sort.folders_first { MI_CHECK } else { " " },
            MI_FOLDER,
            "Folders first",
            &c,
        ))
        .clicked()
    {
        sort.folders_first = !sort.folders_first;
        ui.close();
    }
}

impl OttrinApp {
    pub(super) fn refresh_visibility(&mut self) {
        let tabs: Vec<_> = self
            .state
            .tabs
            .iter()
            .map(|t| (t.id, t.current_dir.clone()))
            .collect();
        for (id, path) in tabs {
            self.refresh_tab(id, path);
        }
        self.persist_config();
    }
    pub(super) fn set_search_sort(&mut self, sort: SearchSort) {
        self.search_ui.sort = sort;
        self.state.config.search.default_sort = sort;
        self.persist_config();
        self.run_search_query();
    }

    pub(super) fn render_search_sort_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.search_ui.sort;
        let mut chosen = before.column();
        let mut reverse = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Sort results:");
            egui::ComboBox::from_id_salt("search-sort")
                .selected_text(match chosen {
                    SearchSort::Name => "Name",
                    SearchSort::Path => "Location",
                    SearchSort::Modified => "Date modified",
                    SearchSort::Size => "Size",
                    _ => "Relevance / Best match",
                })
                .show_ui(ui, |ui| {
                    for (sort, label) in [
                        (SearchSort::Relevance, "Relevance / Best match"),
                        (SearchSort::Name, "Name"),
                        (SearchSort::Path, "Location"),
                        (SearchSort::Modified, "Date modified"),
                        (SearchSort::Size, "Size"),
                    ] {
                        ui.selectable_value(&mut chosen, sort, label);
                    }
                });
            let label = match before {
                SearchSort::Name | SearchSort::Path => "A–Z",
                SearchSort::NameDescending | SearchSort::PathDescending => "Z–A",
                SearchSort::Modified => "Newest first",
                SearchSort::Oldest => "Oldest first",
                SearchSort::Size => "Largest first",
                SearchSort::Smallest => "Smallest first",
                SearchSort::Relevance => "Best match",
            };
            if ui
                .add_enabled(before != SearchSort::Relevance, egui::Button::new(label))
                .on_hover_text(
                    "Reverse search ordering. Remembered independently of folder sorting.",
                )
                .clicked()
            {
                reverse = true;
            }
        });
        if reverse {
            self.set_search_sort(before.reversed());
        } else if chosen != before.column() {
            self.set_search_sort(chosen);
        }
    }

    pub(super) fn item_pointer_opens(&self, response: &egui::Response) -> bool {
        let unmodified = response.ctx.input(|i| !i.modifiers.any());
        if self.state.config.single_click_open {
            response.clicked() && unmodified
        } else {
            response.double_clicked()
        }
    }

    pub(super) fn open_folder_in_new(&mut self, path: &Path) {
        if !path.is_dir() {
            self.set_status_message("This folder is unavailable.", false, 5);
            return;
        }
        if self.state.config.open_folders_in_new_window {
            let launched = ottrin_platform::application_launch_path().and_then(|executable| {
                std::process::Command::new(executable)
                    .arg("--")
                    .arg(path)
                    .spawn()
                    .map(|mut child| {
                        std::thread::spawn(move || {
                            let _ = child.wait();
                        });
                    })
                    .map_err(|e| format!("Could not open a new window: {e}"))
            });
            if let Err(error) = launched {
                self.set_status_message(error, false, 8);
            }
        } else {
            let index = self.state.new_tab(path.to_path_buf());
            self.state.select_tab_idx(index);
            let id = self.state.tabs[index].id;
            self.refresh_tab(id, path.to_path_buf());
            self.request_miller_reveal(id);
        }
    }

    /// Resort every presentation of this directory, retaining selection by path.
    /// Cached raw listings are ordered again when a Miller column is constructed.
    pub(super) fn apply_folder_sort(&mut self, path: &Path, sort: Option<SortConfig>) {
        self.state.config.set_folder_sort(path, sort);
        self.refresh_folder_ordering();
        self.persist_config();
    }

    fn refresh_folder_ordering(&mut self) {
        for ui in self.tab_ui.values_mut() {
            let selected = ui
                .main_sel
                .and_then(|i| ui.main.entries.get(i))
                .map(|e| e.path.clone());
            let left_selected = ui
                .left_sel
                .and_then(|i| ui.left.entries.get(i))
                .map(|e| e.path.clone());
            let right_selected = ui
                .right_sel
                .and_then(|i| ui.right.entries.get(i))
                .map(|e| e.path.clone());
            for col in [&mut ui.main, &mut ui.left, &mut ui.right] {
                sort_entries(
                    &mut col.entries,
                    &self.state.config.sort_for_folder(&col.dir),
                );
            }
            ui.main_sel =
                selected.and_then(|path| ui.main.entries.iter().position(|e| e.path == path));
            ui.left_sel =
                left_selected.and_then(|path| ui.left.entries.iter().position(|e| e.path == path));
            ui.right_sel = right_selected
                .and_then(|path| ui.right.entries.iter().position(|e| e.path == path));
        }
    }

    pub(super) fn change_folder_sort_heading(&mut self, path: &Path, by: SortBy) {
        let sort = choose_heading(self.state.config.sort_for_folder(path), by);
        self.apply_folder_sort(path, Some(sort));
    }

    pub(super) fn render_folder_sort_menu(&mut self, ui: &mut egui::Ui, path: &Path) {
        let c = self.colors;
        let original = self.state.config.sort_for_folder(path);
        let mut sort = original;
        sort_options(ui, &mut sort, c);
        if sort != original {
            self.apply_folder_sort(path, Some(sort));
        }
        menu_separator(ui, c);
        let overridden = self.state.config.has_folder_sort(path);
        if ui
            .add_enabled(
                overridden,
                aligned_menu_button(" ", MI_SETTINGS_BACKUP_RESTORE, "Use default sorting", &c),
            )
            .clicked()
        {
            self.apply_folder_sort(path, None);
            ui.close();
        }
    }

    pub(super) fn render_miller_columns_menu(&mut self, ui: &mut egui::Ui) {
        let c = self.colors;
        let before = self.state.config.miller_columns.clone();
        ui.add_enabled(false, aligned_menu_button(MI_CHECK, MI_FILE, "Name", &c));
        for (by, label, icon, _) in miller_columns::METADATA {
            let on = self.state.config.miller_columns.contains(&by);
            if ui
                .add(aligned_menu_button(
                    if on { MI_CHECK } else { " " },
                    icon,
                    label,
                    &c,
                ))
                .clicked()
            {
                if on {
                    self.state
                        .config
                        .miller_columns
                        .retain(|column| *column != by);
                } else {
                    self.state.config.miller_columns.push(by);
                }
            }
        }
        menu_separator(ui, c);
        if ui
            .add_enabled(
                !self.state.config.miller_columns.is_empty(),
                aligned_menu_button(" ", MI_SETTINGS_BACKUP_RESTORE, "Restore defaults", &c),
            )
            .clicked()
        {
            self.state.config.miller_columns.clear();
        }
        if self.state.config.miller_columns != before {
            let tabs: Vec<_> = self.state.tabs.iter().map(|tab| tab.id).collect();
            for id in tabs {
                self.tab_ui.entry(id).or_default().miller_total_w_max = 0.0;
                self.request_miller_reveal(id);
            }
            self.persist_config();
        }
    }

    fn render_miller_header_menu(&mut self, ui: &mut egui::Ui, path: &Path) {
        let c = self.colors;
        widget_style::aligned_choice_submenu(ui, MI_VIEW_LIST, "Sort by", c, |ui| {
            self.render_folder_sort_menu(ui, path);
        });
        widget_style::aligned_choice_submenu(ui, MI_VIEW_COLUMN, "Visible columns", c, |ui| {
            self.render_miller_columns_menu(ui);
        });
    }

    pub(super) fn render_folder_sort_header(&mut self, ui: &mut egui::Ui, path: &Path) {
        let sort = self.state.config.sort_for_folder(path);
        let c = self.colors;
        let rect = miller_columns::allocate_header(ui, &c);
        let scale = self.view_scale_for(ViewMode::Miller);
        for (by, label, cell) in miller_columns::cells(rect, &self.state.config, scale, &c) {
            let response = ui.interact(
                cell,
                ui.id().with(("folder-sort", path, label)),
                Sense::click(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    format!("Sort {} by {label}", path_label(path)),
                )
            });
            if response.hovered() {
                ui.painter().rect_filled(cell, 0.0, c.hover);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            miller_columns::paint_heading(
                ui,
                cell,
                label,
                (sort.by == by).then_some(sort.ascending),
                &c,
            );
            if response.clicked() {
                self.change_folder_sort_heading(path, by);
                ui.ctx().request_repaint();
            }
            response.context_menu(|ui| self.render_miller_header_menu(ui, path));
        }
    }

    pub(super) fn render_general_folder_preferences(&mut self, ui: &mut egui::Ui) {
        let c = self.colors;
        ui.add_space(12.0);
        settings_section_title(ui, c, "Startup");
        settings_description(
            ui,
            c,
            "Choose where Ottrin opens when no folder was supplied. Places and explicit folder launches always take precedence.",
        );
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut self.state.config.restore_tabs,
                "Restore previous tabs on startup",
            )
            .changed();
        settings_description(
            ui,
            c,
            "Restoring previous tabs overrides the startup folder. Unavailable folders are skipped.",
        );
        ui.add_enabled_ui(!self.state.config.restore_tabs, |ui| {
            ui.label(format!(
                "Startup folder: {}",
                self.state
                    .config
                    .startup_folder
                    .as_deref()
                    .map(path_label)
                    .unwrap_or_else(|| "Home".into())
            ));
            ui.horizontal(|ui| {
                if ui.button("Choose folder…").clicked() {
                    self.pick_search_folder(
                        ui.ctx(),
                        internal_picker::Purpose::StartupFolder,
                        "Choose startup folder",
                    );
                }
                if ui
                    .add_enabled(
                        self.state.config.startup_folder.is_some(),
                        egui::Button::new("Use Home"),
                    )
                    .clicked()
                {
                    self.state.config.startup_folder = None;
                    changed = true;
                }
            });
        });
        ui.add_space(12.0);
        settings_section_title(ui, c, "Default sorting");
        settings_description(
            ui,
            c,
            "Applies to folders without saved sorting preferences. Per-folder settings override this default. Right-click a folder heading and choose Use default sorting to clear its override.",
        );
        let before = self.state.config.default_sort;
        egui::ComboBox::from_id_salt("default-folder-sort")
            .selected_text(
                SORT_FIELDS
                    .iter()
                    .find(|(by, _)| *by == before.by)
                    .unwrap()
                    .1,
            )
            .show_ui(ui, |ui| {
                for (by, label) in SORT_FIELDS {
                    if ui
                        .selectable_value(&mut self.state.config.default_sort.by, by, label)
                        .changed()
                    {
                        self.state.config.default_sort.ascending = default_sort_direction(by);
                    }
                }
            });
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut self.state.config.default_sort.ascending,
                true,
                "Ascending",
            );
            ui.radio_value(
                &mut self.state.config.default_sort.ascending,
                false,
                "Descending",
            );
            ui.checkbox(
                &mut self.state.config.default_sort.folders_first,
                "Folders first",
            );
        });
        if before != self.state.config.default_sort {
            self.refresh_folder_ordering();
            changed = true;
        }
        ui.add_space(12.0);
        settings_section_title(ui, c, "Opening and safety");
        let hidden_before = self.state.config.show_hidden_files;
        changed |= ui
            .checkbox(
                &mut self.state.config.show_hidden_files,
                "Show hidden files",
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label("Open items with:");
            changed |= ui
                .radio_value(
                    &mut self.state.config.single_click_open,
                    false,
                    "Double click",
                )
                .changed();
            changed |= ui
                .radio_value(
                    &mut self.state.config.single_click_open,
                    true,
                    "Single click",
                )
                .changed();
        });
        settings_description(
            ui,
            c,
            "Column view keeps single-click folder navigation. Ctrl/Shift clicks select without opening.",
        );
        ui.horizontal(|ui| {
            ui.label("Open folder in new:");
            changed |= ui
                .radio_value(
                    &mut self.state.config.open_folders_in_new_window,
                    false,
                    "Tab",
                )
                .changed();
            changed |= ui
                .radio_value(
                    &mut self.state.config.open_folders_in_new_window,
                    true,
                    "Window",
                )
                .changed();
        });
        settings_description(
            ui,
            c,
            "Controls the folder menu's Open in new action. Ordinary navigation stays in the current tab.",
        );
        changed |= ui
            .checkbox(
                &mut self.state.config.confirm_permanent_delete,
                "Confirm permanent deletion",
            )
            .changed();
        settings_description(
            ui,
            c,
            "Normal deletion moves items to Trash. Turning this off allows explicit permanent deletion without confirmation.",
        );
        if hidden_before != self.state.config.show_hidden_files {
            self.refresh_visibility();
        }
        if changed {
            self.persist_config();
        }
        ui.add_space(14.0);
        ui.separator();
    }
}

#[cfg(test)]
#[path = "folder_preferences_tests.rs"]
mod tests;
