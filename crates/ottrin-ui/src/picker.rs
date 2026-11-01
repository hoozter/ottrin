//! The actual chooser surface, shared by internal dialogs and standalone requests.
//! Directory browsing never starts or waits for an index daemon.
use crate::icons::*;
use crate::widget_style::{ThemeButtonExt, ThemeTextEditExt};
use crate::{Colors, EntryVisualState, file_row, listing, path_crumbs, path_label};
use egui::{Align, Context, Layout, RichText, ScrollArea, Vec2};
use ottrin_core::picker::{
    PickerMode, PickerPattern, PickerRequest, PickerResponse, is_picker_filename,
};
use ottrin_core::{AppConfig, EntryKind, FileEntry, SortConfig, default_home_dir};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

struct VisibleEntries {
    entries: Arc<Vec<FileEntry>>,
    indices: Arc<Vec<usize>>,
}

impl VisibleEntries {
    fn len(&self) -> usize {
        self.indices.len()
    }
    fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
    fn get(&self, index: usize) -> &FileEntry {
        &self.entries[self.indices[index]]
    }
    fn iter(&self) -> impl Iterator<Item = &FileEntry> {
        self.indices.iter().map(|&index| &self.entries[index])
    }
}

#[derive(Clone)]
struct Filter {
    globs: Vec<globset::GlobMatcher>,
    mimes: Vec<String>,
}
impl Filter {
    fn compile(request: &ottrin_core::picker::PickerFilter) -> Result<Self, String> {
        let mut filter = Self {
            globs: Vec::new(),
            mimes: Vec::new(),
        };
        for pattern in &request.patterns {
            match pattern {
                PickerPattern::Glob(pattern) => filter.globs.push(
                    globset::GlobBuilder::new(pattern)
                        .backslash_escape(true)
                        .case_insensitive(cfg!(windows))
                        .build()
                        .map_err(|error| error.to_string())?
                        .compile_matcher(),
                ),
                PickerPattern::Mime(mime) => {
                    let parsed = mime
                        .parse::<mime_guess2::Mime>()
                        .map_err(|error| error.to_string())?;
                    filter.mimes.push(parsed.essence_str().to_ascii_lowercase());
                }
            }
        }
        Ok(filter)
    }
    fn matches(&self, path: &Path) -> bool {
        if self.mimes.iter().any(|mime| mime == "*/*") {
            return true;
        }
        let name = path.file_name().map(Path::new).unwrap_or(path);
        self.globs.iter().any(|glob| glob.is_match(name))
            || mime_guess2::from_path(path).iter().any(|mime| {
                let actual = mime.essence_str();
                self.mimes.iter().any(|pattern| {
                    pattern == actual
                        || pattern
                            .strip_suffix("/*")
                            .is_some_and(|prefix| actual.starts_with(&format!("{prefix}/")))
                })
            })
            || (self
                .mimes
                .iter()
                .any(|mime| mime == "application/octet-stream")
                && mime_guess2::from_path(path).is_empty())
    }
}

enum Work {
    Scan {
        generation: u64,
        directory: PathBuf,
        hidden: bool,
    },
    Validate {
        generation: u64,
        paths: Vec<PathBuf>,
        mode: PickerMode,
        overwrite_confirmed: bool,
        filter: Option<Filter>,
    },
}
enum ResultMessage {
    Listing {
        generation: u64,
        entries: Result<Vec<FileEntry>, String>,
    },
    Selection {
        generation: u64,
        result: Result<Selection, String>,
    },
}
enum Selection {
    Ready(Vec<PathBuf>),
    Overwrite(PathBuf),
    Navigate(PathBuf),
}

struct Worker {
    sender: Option<mpsc::Sender<Work>>,
    results: mpsc::Receiver<ResultMessage>,
    stopped: Arc<AtomicBool>,
}
impl Worker {
    fn start(ctx: Context) -> Self {
        let (sender, requests) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                while let Ok(latest) = requests.try_recv() {
                    request = latest;
                }
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let result = match request {
                    Work::Scan {
                        generation,
                        directory,
                        hidden,
                    } => {
                        let entries =
                            listing::list_directory(&directory, hidden).map(|mut entries| {
                                listing::sort_entries(&mut entries, &SortConfig::default());
                                entries
                            });
                        ResultMessage::Listing {
                            generation,
                            entries,
                        }
                    }
                    Work::Validate {
                        generation,
                        paths,
                        mode,
                        overwrite_confirmed,
                        filter,
                    } => ResultMessage::Selection {
                        generation,
                        result: validate_selection(
                            paths,
                            mode,
                            overwrite_confirmed,
                            filter.as_ref(),
                        ),
                    },
                };
                if stop.load(Ordering::Acquire) || completed.send(result).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            sender: Some(sender),
            results,
            stopped,
        }
    }
    fn send(&self, work: Work) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("The file chooser has closed")?
            .send(work)
            .map_err(|_| "The file chooser worker stopped unexpectedly".into())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.sender.take();
    }
}

fn is_directory(entry: &FileEntry) -> bool {
    entry.kind == EntryKind::Directory
        || (entry.kind == EntryKind::Symlink && entry.symlink_target_is_dir == Some(true))
}
fn validate_selection(
    paths: Vec<PathBuf>,
    mode: PickerMode,
    overwrite_confirmed: bool,
    filter: Option<&Filter>,
) -> Result<Selection, String> {
    if paths.is_empty() {
        return Err("Choose a file or folder first".into());
    }
    for path in &paths {
        match std::fs::metadata(path) {
            Ok(metadata) => match mode {
                PickerMode::Open if metadata.is_dir() && paths.len() == 1 => {
                    return Ok(Selection::Navigate(path.clone()));
                }
                PickerMode::Folder if !metadata.is_dir() => return Err("Choose a folder".into()),
                PickerMode::Open | PickerMode::Save if !metadata.is_file() => {
                    return Err("Choose a regular file".into());
                }
                PickerMode::Open if filter.is_some_and(|filter| !filter.matches(path)) => {
                    return Err("The file does not match the selected file type.".into());
                }
                PickerMode::Save if !overwrite_confirmed => {
                    return Ok(Selection::Overwrite(path.clone()));
                }
                _ => (),
            },
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && mode == PickerMode::Save =>
            {
                match std::fs::symlink_metadata(path) {
                    Ok(_) => {
                        return Err(format!(
                            "{} exists but its target is unavailable",
                            path_label(path)
                        ));
                    }
                    Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error.to_string()),
                }
                let parent = path.parent().ok_or("The save location has no parent")?;
                if !std::fs::metadata(parent)
                    .map_err(|error| error.to_string())?
                    .is_dir()
                {
                    return Err("The save location is not a folder".into());
                }
            }
            Err(error) => return Err(format!("Cannot use {}: {error}", path_label(path))),
        }
    }
    Ok(Selection::Ready(paths))
}

/// One independently owned dialog. The caller supplies the live Ottrin theme.
/// Results are paths only: accepting Save never creates or truncates a file.
pub(crate) struct PickerDialog {
    request: PickerRequest,
    filters: Vec<Filter>,
    directory: PathBuf,
    location: String,
    editing_location: bool,
    focus_location: bool,
    entries: Arc<Vec<FileEntry>>,
    visible: Arc<Vec<usize>>,
    visible_key: Option<(u64, Option<usize>, String)>,
    selected: Vec<PathBuf>,
    anchor: Option<usize>,
    focus: Option<PathBuf>,
    preview: Option<crate::picker_preview::PickerPreview>,
    filename: String,
    query: String,
    hidden: bool,
    grid: bool,
    options_expanded: bool,
    generation: u64,
    loading: bool,
    validating: bool,
    listing_valid: bool,
    error: Option<String>,
    overwrite: Option<PathBuf>,
    choices: BTreeMap<String, String>,
    worker: Worker,
    result: Option<PickerResponse>,
}
impl PickerDialog {
    pub fn new(request: PickerRequest, ctx: &Context) -> Result<Self, String> {
        request.validate()?;
        let filters = request
            .filters
            .iter()
            .map(Filter::compile)
            .collect::<Result<Vec<_>, _>>()?;
        let directory = request
            .initial_directory
            .clone()
            .unwrap_or_else(default_home_dir);
        let filename = request.filename.clone();
        let choices = request
            .choices
            .iter()
            .map(|choice| (choice.id.clone(), choice.selected.clone()))
            .collect();
        let mut dialog = Self {
            request,
            filters,
            location: path_label(&directory),
            directory,
            editing_location: false,
            focus_location: false,
            entries: Arc::new(Vec::new()),
            visible: Arc::new(Vec::new()),
            visible_key: None,
            selected: Vec::new(),
            anchor: None,
            focus: None,
            preview: None,
            filename,
            query: String::new(),
            hidden: false,
            grid: false,
            options_expanded: false,
            generation: 0,
            loading: false,
            validating: false,
            listing_valid: false,
            error: None,
            overwrite: None,
            choices,
            worker: Worker::start(ctx.clone()),
            result: None,
        };
        if dialog.request.current_filter.is_none() && !dialog.filters.is_empty() {
            dialog.request.current_filter = Some(0);
        }
        dialog.scan();
        Ok(dialog)
    }
    pub fn request(&self) -> &PickerRequest {
        &self.request
    }
    pub fn take_result(&mut self) -> Option<PickerResponse> {
        self.result.take()
    }
    pub fn cancel(&mut self) {
        let mut response = self.request.cancelled_response();
        response.choices.clone_from(&self.choices);
        self.result = Some(response);
    }
    fn scan(&mut self) {
        self.generation += 1;
        self.loading = true;
        self.validating = false;
        self.listing_valid = false;
        self.clear_selection();
        self.entries = Arc::new(Vec::new());
        self.visible_key = None;
        self.overwrite = None;
        self.error = None;
        if let Err(error) = self.worker.send(Work::Scan {
            generation: self.generation,
            directory: self.directory.clone(),
            hidden: self.hidden,
        }) {
            self.loading = false;
            self.error = Some(error);
        }
    }
    fn navigate(&mut self, directory: PathBuf) {
        self.directory = if directory.is_absolute() {
            directory
        } else {
            self.directory.join(directory)
        };
        self.location = path_label(&self.directory);
        self.editing_location = false;
        if self.request.mode != PickerMode::Save {
            self.filename.clear();
        }
        self.query.clear();
        self.scan();
    }
    fn poll(&mut self) {
        while let Ok(message) = self.worker.results.try_recv() {
            match message {
                ResultMessage::Listing {
                    generation,
                    entries,
                } if generation == self.generation => {
                    self.loading = false;
                    match entries {
                        Ok(entries) => {
                            self.entries = Arc::new(entries);
                            self.visible_key = None;
                            self.listing_valid = true;
                        }
                        Err(error) => {
                            self.entries = Arc::new(Vec::new());
                            self.visible_key = None;
                            self.error = Some(error);
                        }
                    }
                }
                ResultMessage::Selection { generation, result }
                    if generation == self.generation =>
                {
                    self.validating = false;
                    match result {
                        Ok(Selection::Overwrite(path)) => self.overwrite = Some(path),
                        Ok(Selection::Navigate(path)) => self.navigate(path),
                        Ok(Selection::Ready(paths)) => {
                            self.result = Some(PickerResponse {
                                accepted: true,
                                paths,
                                current_filter: self.request.current_filter,
                                choices: self.choices.clone(),
                            })
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                _ => (),
            }
        }
    }
    fn visible_entries(&mut self) -> VisibleEntries {
        let key = (
            self.generation,
            self.request.current_filter,
            self.query.clone(),
        );
        if self.visible_key.as_ref() != Some(&key) {
            let query = self.query.to_lowercase();
            self.visible = Arc::new(
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        let directory = is_directory(entry);
                        ((self.request.mode != PickerMode::Folder || directory)
                            && (query.is_empty() || entry.name.to_lowercase().contains(&query))
                            && (directory
                                || self
                                    .request
                                    .current_filter
                                    .is_none_or(|index| self.filters[index].matches(&entry.path))))
                        .then_some(index)
                    })
                    .collect(),
            );
            self.visible_key = Some(key);
        }
        VisibleEntries {
            entries: self.entries.clone(),
            indices: self.visible.clone(),
        }
    }
    fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.focus = None;
        if self.request.mode != PickerMode::Save {
            self.filename.clear();
        }
        self.cancel_validation();
    }
    fn cancel_validation(&mut self) {
        if self.validating {
            self.generation += 1;
            self.validating = false;
        }
    }
    fn select(
        &mut self,
        entry: &FileEntry,
        index: usize,
        visible: &VisibleEntries,
        modifiers: egui::Modifiers,
    ) {
        self.cancel_validation();
        if self.request.multiple && modifiers.shift {
            let start = self.anchor.unwrap_or(index).min(index);
            let end = self
                .anchor
                .unwrap_or(index)
                .max(index)
                .min(visible.len() - 1);
            self.selected = visible
                .iter()
                .skip(start)
                .take(end - start + 1)
                .map(|entry| entry.path.clone())
                .collect();
        } else if self.request.multiple && (modifiers.ctrl || modifiers.command) {
            if let Some(index) = self.selected.iter().position(|path| path == &entry.path) {
                self.selected.remove(index);
            } else {
                self.selected.push(entry.path.clone());
            }
            self.anchor = Some(index);
        } else {
            self.selected = vec![entry.path.clone()];
            self.anchor = Some(index);
        }
        self.focus = if self.selected.contains(&entry.path) {
            Some(entry.path.clone())
        } else {
            self.selected.last().cloned()
        };
        if self.request.mode != PickerMode::Save {
            self.filename = self
                .focus
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        } else if !is_directory(entry) {
            self.filename.clone_from(&entry.name);
        }
        self.error = None;
    }
    fn accept(&mut self, confirmed: bool) {
        if self.loading || self.validating || !self.listing_valid {
            return;
        }
        let paths = if self.request.mode == PickerMode::Save {
            if !is_picker_filename(&self.filename) {
                self.error =
                    Some("Enter one filename. Use Location to choose another folder.".into());
                return;
            }
            if let Err(error) = ottrin_platform::validate_creation_name(&self.filename) {
                self.error = Some(error.to_string());
                return;
            }
            let path = self.directory.join(&self.filename);
            if self
                .request
                .current_filter
                .is_some_and(|index| !self.filters[index].matches(&path))
            {
                self.error = Some("The filename does not match the selected file type.".into());
                return;
            }
            vec![path]
        } else if self.request.mode == PickerMode::Folder && self.selected.is_empty() {
            vec![self.directory.clone()]
        } else if !self.selected.is_empty() {
            self.selected.clone()
        } else if is_picker_filename(&self.filename) {
            vec![self.directory.join(&self.filename)]
        } else {
            self.error = Some("Choose a file first.".into());
            return;
        };
        if self.request.mode == PickerMode::Open
            && paths.iter().any(|path| {
                self.entries
                    .iter()
                    .any(|entry| &entry.path == path && is_directory(entry))
            })
        {
            if paths.len() == 1 {
                self.navigate(paths[0].clone());
            } else {
                self.error = Some("Choose files, or open a folder to browse it.".into());
            }
            return;
        }
        self.validating = true;
        self.error = None;
        if let Err(error) = self.worker.send(Work::Validate {
            generation: self.generation,
            paths,
            mode: self.request.mode,
            overwrite_confirmed: confirmed,
            filter: self
                .request
                .current_filter
                .map(|index| self.filters[index].clone()),
        }) {
            self.validating = false;
            self.error = Some(error);
        }
    }
    /// Render only the chooser body; native/embedded hosts use the same titlebar and frame.
    fn handle_file_keys(&mut self, ctx: &Context, _config: &AppConfig) {
        let visible = self.visible_entries();
        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A))
            && self.request.multiple
        {
            self.cancel_validation();
            self.selected = visible.iter().map(|entry| entry.path.clone()).collect();
            self.focus = self.selected.last().cloned();
            self.filename.clear();
        }
        let step = ctx.input(|input| {
            if input.key_pressed(egui::Key::ArrowDown) {
                1
            } else if input.key_pressed(egui::Key::ArrowUp) {
                -1
            } else {
                0
            }
        });
        if step != 0 && !visible.is_empty() {
            let current = self
                .focus
                .as_ref()
                .and_then(|path| visible.iter().position(|entry| &entry.path == path));
            let index = current.map_or(if step > 0 { 0 } else { visible.len() - 1 }, |index| {
                (index as isize + step).clamp(0, visible.len() as isize - 1) as usize
            });
            self.select(
                visible.get(index),
                index,
                &visible,
                ctx.input(|input| input.modifiers),
            );
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
            if let Some(entry) = self
                .focus
                .as_ref()
                .and_then(|path| visible.iter().find(|entry| &entry.path == path))
                && is_directory(entry)
            {
                self.navigate(entry.path.clone());
            } else {
                self.accept(false);
            }
        }
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, colors: Colors, config: &AppConfig) {
        self.poll();
        let ctx = ui.ctx().clone();
        if ctx.input(|input| input.key_pressed(egui::Key::Escape))
            && self.overwrite.take().is_none()
        {
            if self.editing_location {
                self.editing_location = false;
                self.location = path_label(&self.directory);
            } else {
                self.cancel();
            }
        }
        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::L)) {
            self.editing_location = true;
            self.focus_location = true;
        }
        let blocked = self.overwrite.is_some();
        if !blocked && !self.editing_location && !ctx.wants_keyboard_input() {
            self.handle_file_keys(&ctx, config);
        }
        ui.add_enabled_ui(!blocked, |ui| {
            egui::Frame::new()
                .fill(colors.toolbar_bg)
                .inner_margin(6)
                .corner_radius(colors.border_radius)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.directory.parent().is_some(),
                                egui::Button::new(MI_ARROW_UP).themed(&colors),
                            )
                            .on_hover_text("Parent folder")
                            .clicked()
                            && let Some(parent) = self.directory.parent()
                        {
                            self.navigate(parent.to_path_buf());
                        }
                        if self.editing_location {
                            let editor_id = ui.make_persistent_id("picker-location");
                            if std::mem::take(&mut self.focus_location) {
                                ui.memory_mut(|memory| memory.request_focus(editor_id));
                                let mut state =
                                    egui::text_edit::TextEditState::load(&ctx, editor_id)
                                        .unwrap_or_default();
                                state
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::two(
                                        egui::text::CCursor::new(0),
                                        egui::text::CCursor::new(self.location.chars().count()),
                                    )));
                                state.store(&ctx, editor_id);
                            }
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.location)
                                    .id(editor_id)
                                    .char_limit(32768)
                                    .desired_width(ui.available_width() * 0.62)
                                    .themed(&colors),
                            );
                            if response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            {
                                let path = PathBuf::from(&self.location);
                                self.navigate(if path.is_absolute() {
                                    path
                                } else {
                                    self.directory.join(path)
                                });
                            }
                        } else {
                            let width = (ui.available_width() * 0.62).max(120.0);
                            ScrollArea::horizontal()
                                .id_salt("picker-breadcrumbs")
                                .max_width(width)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for crumb in path_crumbs(&self.directory) {
                                            if ui
                                                .add(
                                                    egui::Button::new(format!(
                                                        "{} {}",
                                                        if crumb.is_root {
                                                            MI_STORAGE
                                                        } else {
                                                            MI_FOLDER
                                                        },
                                                        crumb.label
                                                    ))
                                                    .frame(false)
                                                    .themed(&colors),
                                                )
                                                .on_hover_text(path_label(&crumb.target))
                                                .clicked()
                                            {
                                                self.navigate(crumb.target);
                                            }
                                        }
                                        if ui
                                            .add(egui::Button::new(MI_EDIT).themed(&colors))
                                            .on_hover_text("Edit location (Ctrl+L)")
                                            .clicked()
                                        {
                                            self.editing_location = true;
                                            self.focus_location = true;
                                        }
                                    });
                                });
                        }
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut self.query)
                                .hint_text("Search this folder")
                                .char_limit(1024)
                                .desired_width(ui.available_width().max(80.0))
                                .themed(&colors),
                        );
                        if search.changed() {
                            self.clear_selection();
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if self.loading {
                                "Reading folder…".into()
                            } else {
                                format!("{} items", self.visible_entries().len())
                            })
                            .color(colors.text_muted)
                            .small(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(MI_VISIBILITY)
                                        .selected(self.preview.is_some())
                                        .themed(&colors),
                                )
                                .on_hover_text("Show read-only preview")
                                .clicked()
                            {
                                self.preview = if self.preview.is_some() {
                                    None
                                } else {
                                    Some(crate::picker_preview::PickerPreview::new(&ctx))
                                };
                            }
                            if ui
                                .add(
                                    egui::Button::new(if self.hidden {
                                        MI_VISIBILITY
                                    } else {
                                        MI_VISIBILITY_OFF
                                    })
                                    .themed(&colors),
                                )
                                .on_hover_text("Show hidden files")
                                .clicked()
                            {
                                self.hidden = !self.hidden;
                                self.scan();
                            }
                            if ui
                                .add(egui::Button::new(MI_APPS).themed(&colors))
                                .on_hover_text("Grid view")
                                .clicked()
                            {
                                self.grid = true;
                            }
                            if ui
                                .add(egui::Button::new(MI_VIEW_LIST).themed(&colors))
                                .on_hover_text("List view")
                                .clicked()
                            {
                                self.grid = false;
                            }
                        });
                    });
                });
            ui.separator();
            let options_open = self.options_expanded;
            let footer_height = (80.0
                + if self.request.filters.is_empty() || self.request.mode == PickerMode::Folder {
                    0.0
                } else {
                    32.0
                }
                + if self.request.choices.is_empty() {
                    0.0
                } else {
                    28.0
                }
                + if options_open { 96.0 } else { 0.0 }
                + if self.error.is_some() { 24.0 } else { 0.0 })
                * colors.font_scale;
            let height = (ui.available_height() - footer_height).max(0.0);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(150.0 * colors.font_scale, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(colors.bookmarks_bg)
                            .corner_radius(colors.border_radius)
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.set_min_height(height - 16.0);
                                ui.set_width(130.0 * colors.font_scale);
                                ui.label(RichText::new("Places").color(colors.heading).strong());
                                let home = default_home_dir();
                                let mut places = config.bookmarks.clone();
                                for bookmark in crate::OttrinApp::default_bookmarks_from_roots(
                                    &home,
                                    Vec::new(),
                                ) {
                                    if !places.iter().any(|(_, _, path)| path == &bookmark.2) {
                                        places.push(bookmark);
                                    }
                                }
                                ScrollArea::vertical()
                                    .id_salt("picker-places")
                                    .show(ui, |ui| {
                                        for (icon, name, path) in places {
                                            let selected = self.directory == path;
                                            if ui
                                                .add(
                                                    crate::widget_style::aligned_menu_button(
                                                        "", &icon, &name, &colors,
                                                    )
                                                    .width(ui.available_width())
                                                    .selected(selected),
                                                )
                                                .on_hover_text(path_label(&path))
                                                .clicked()
                                            {
                                                self.navigate(path);
                                            }
                                        }
                                    });
                            });
                    },
                );
                let width = if self.preview.is_some() {
                    (ui.available_width() * 0.58).max(120.0)
                } else {
                    ui.available_width()
                };
                ui.allocate_ui_with_layout(
                    Vec2::new(width, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        if self.loading {
                            ui.spinner();
                            ui.label("Reading folder…");
                        }
                        let visible = self.visible_entries();
                        let columns = (ui.available_width() / (130.0 * colors.font_scale))
                            .floor()
                            .max(1.0) as usize;
                        let row_count = if self.grid {
                            visible.len().div_ceil(columns)
                        } else {
                            visible.len()
                        };
                        let row_height = if self.grid {
                            80.0 * colors.font_scale
                        } else {
                            (25.0 * colors.font_scale).round()
                        };
                        ScrollArea::vertical()
                            .id_salt("picker-files")
                            .max_height(height)
                            .show_rows(ui, row_height, row_count, |ui, range| {
                                if self.grid {
                                    egui::Grid::new("picker-grid")
                                        .num_columns(columns)
                                        .min_col_width(110.0 * colors.font_scale)
                                        .show(ui, |ui| {
                                            for index in (range.start * columns)
                                                ..(range.end * columns).min(visible.len())
                                            {
                                                let entry = visible.get(index);
                                                let selected = self.selected.contains(&entry.path);
                                                let style = crate::style_for_entry(
                                                    entry,
                                                    EntryVisualState {
                                                        selected,
                                                        focused: true,
                                                        symlink_dir: entry.symlink_target_is_dir
                                                            == Some(true),
                                                        ..Default::default()
                                                    },
                                                    &colors,
                                                    config,
                                                );
                                                let response = ui.add_sized(
                                                    [
                                                        120.0 * colors.font_scale,
                                                        80.0 * colors.font_scale,
                                                    ],
                                                    egui::Button::new(
                                                        RichText::new(format!(
                                                            "{}\n{}",
                                                            crate::material_icon(style.icon),
                                                            entry.name
                                                        ))
                                                        .color(style.text_color),
                                                    )
                                                    .selected(selected)
                                                    .themed(&colors),
                                                );
                                                if response.clicked() {
                                                    self.select(
                                                        entry,
                                                        index,
                                                        &visible,
                                                        ui.input(|input| input.modifiers),
                                                    );
                                                }
                                                if response.double_clicked() {
                                                    if is_directory(entry) {
                                                        self.navigate(entry.path.clone());
                                                    } else if self.request.mode
                                                        != PickerMode::Folder
                                                    {
                                                        self.accept(false);
                                                    }
                                                }
                                                if (index + 1) % columns == 0 {
                                                    ui.end_row();
                                                }
                                            }
                                        });
                                } else {
                                    for index in range {
                                        let entry = visible.get(index);
                                        let response = file_row::render_file_row(
                                            ui,
                                            entry,
                                            EntryVisualState {
                                                selected: self.selected.contains(&entry.path),
                                                focused: true,
                                                ..Default::default()
                                            },
                                            file_row::FileRowLayout {
                                                index,
                                                width: ui.available_width(),
                                                scale: 1.0,
                                            },
                                            &colors,
                                            config,
                                        );
                                        if self.focus.as_ref() == Some(&entry.path)
                                            && ui.input(|input| {
                                                input.key_pressed(egui::Key::ArrowDown)
                                                    || input.key_pressed(egui::Key::ArrowUp)
                                            })
                                        {
                                            response.scroll_to_me(Some(Align::Center));
                                        }
                                        if response.clicked() {
                                            self.select(
                                                entry,
                                                index,
                                                &visible,
                                                ui.input(|input| input.modifiers),
                                            );
                                        }
                                        if response.double_clicked() {
                                            if is_directory(entry) {
                                                self.navigate(entry.path.clone());
                                            } else if self.request.mode != PickerMode::Folder {
                                                self.accept(false);
                                            }
                                        }
                                    }
                                }
                                if visible.is_empty() && !self.loading {
                                    ui.label(if self.query.is_empty() {
                                        "No matching files or folders"
                                    } else {
                                        "No matches in this folder"
                                    });
                                }
                            });
                    },
                );
                if let Some(preview) = &mut self.preview {
                    let path = self
                        .focus
                        .as_ref()
                        .filter(|path| {
                            self.entries
                                .iter()
                                .any(|entry| &entry.path == *path && !is_directory(entry))
                        })
                        .cloned();
                    preview.select(path, config);
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_max_height(height);
                            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                            preview.show(ui, colors, config);
                        },
                    );
                }
            });
            ui.separator();
            if let Some(error) = &self.error {
                ui.add(egui::Label::new(RichText::new(error).color(colors.error)).truncate())
                    .on_hover_text(error);
            }
            ui.horizontal(|ui| {
                ui.label(if self.request.mode == PickerMode::Folder {
                    "Folder"
                } else {
                    "File name"
                });
                if self.request.mode == PickerMode::Folder {
                    ui.label(path_label(self.selected.first().unwrap_or(&self.directory)));
                } else if self.selected.len() > 1 {
                    ui.label(format!("{} files selected", self.selected.len()));
                } else {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.filename)
                            .char_limit(4096)
                            .desired_width(ui.available_width())
                            .themed(&colors),
                    );
                    if response.changed() {
                        self.selected.clear();
                        self.cancel_validation();
                    }
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        self.accept(false);
                    }
                }
            });
            if !self.request.filters.is_empty() && self.request.mode != PickerMode::Folder {
                ui.horizontal(|ui| {
                    ui.label("File type");
                    let old = self.request.current_filter;
                    egui::ComboBox::from_id_salt("picker-filter")
                        .selected_text(
                            self.request.filters[self.request.current_filter.unwrap_or(0)]
                                .name
                                .clone(),
                        )
                        .show_ui(ui, |ui| {
                            for (index, filter) in self.request.filters.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.request.current_filter,
                                    Some(index),
                                    &filter.name,
                                );
                            }
                        });
                    if old != self.request.current_filter {
                        self.clear_selection();
                    }
                });
            }
            if !self.request.choices.is_empty() {
                let label = if options_open {
                    "▾ Options"
                } else {
                    "▸ Options"
                };
                if ui
                    .add(
                        egui::Button::new(RichText::new(label).strong().color(colors.heading))
                            .frame(false)
                            .themed(&colors),
                    )
                    .clicked()
                {
                    self.options_expanded = !self.options_expanded;
                    ui.ctx().request_repaint();
                }
                if options_open {
                    ScrollArea::vertical()
                        .id_salt("picker-options")
                        .max_height(88.0 * colors.font_scale)
                        .show(ui, |ui| {
                            for choice in &self.request.choices {
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [180.0 * colors.font_scale, ui.spacing().interact_size.y],
                                        egui::Label::new(&choice.label).truncate(),
                                    )
                                    .on_hover_text(&choice.label);
                                    if choice.options.is_empty() {
                                        let mut selected = self.choices[&choice.id] == "true";
                                        if ui.checkbox(&mut selected, "").changed() {
                                            self.choices
                                                .insert(choice.id.clone(), selected.to_string());
                                        }
                                    } else {
                                        let selected = self
                                            .choices
                                            .get_mut(&choice.id)
                                            .expect("validated choice");
                                        let label = choice
                                            .options
                                            .iter()
                                            .find(|(id, _)| id == selected)
                                            .map(|(_, label)| label.as_str())
                                            .unwrap_or("");
                                        egui::ComboBox::from_id_salt(("picker-choice", &choice.id))
                                            .width(220.0 * colors.font_scale)
                                            .truncate()
                                            .selected_text(label)
                                            .show_ui(ui, |ui| {
                                                for (id, label) in &choice.options {
                                                    ui.selectable_value(
                                                        selected,
                                                        id.clone(),
                                                        label,
                                                    );
                                                }
                                            });
                                    }
                                });
                            }
                        });
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.listing_valid && !self.loading && !self.validating,
                        egui::Button::new(self.request.action_label()).themed(&colors),
                    )
                    .clicked()
                {
                    self.accept(false);
                }
                if ui
                    .add(egui::Button::new("Cancel").themed(&colors))
                    .clicked()
                {
                    self.cancel();
                }
                if self.validating {
                    ui.spinner();
                }
            });
        });
        if let Some(path) = self.overwrite.clone() {
            egui::Modal::new(egui::Id::new("picker-overwrite")).show(&ctx,|ui| {
                ui.heading("Replace this file?");ui.label(path_label(&path));
                ui.label("The application will replace this file when it saves. Ottrin has not changed it.");
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Cancel").themed(&colors)).clicked() { self.overwrite=None; }
                    if ui.add(egui::Button::new("Replace").themed(&colors)).clicked() { self.overwrite=None;self.accept(true); }
                });
            });
        }
    }
}

/// Standalone host for portal and cooperating-application requests. It loads no
/// file-manager operation workers and never connects to or starts the index.
pub struct PickerApp {
    dialog: PickerDialog,
    config: AppConfig,
    colors: Colors,
    appearance: crate::system_appearance::SystemAppearance,
    is_windows: bool,
    outcome: Arc<std::sync::Mutex<Option<PickerResponse>>>,
}
impl PickerApp {
    pub fn validate_request(request: &PickerRequest) -> Result<(), String> {
        request.validate()?;
        for filter in &request.filters {
            Filter::compile(filter)?;
        }
        Ok(())
    }
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        request: PickerRequest,
        runtime: ottrin_core::GraphicsRuntimeStatus,
        outcome: Arc<std::sync::Mutex<Option<PickerResponse>>>,
    ) -> Result<Self, String> {
        crate::install_application_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let config = crate::load_config();
        let colors = Colors::for_config(&config, cc.egui_ctx.system_theme());
        crate::theme_colors::apply_to_context(&cc.egui_ctx, &colors);
        Ok(Self {
            dialog: PickerDialog::new(request, &cc.egui_ctx)?,
            config,
            colors,
            appearance: crate::system_appearance::SystemAppearance::start(cc.egui_ctx.clone()),
            is_windows: runtime.is_windows,
            outcome,
        })
    }
}
impl PickerApp {
    fn render_window(&mut self, ctx: &Context) {
        self.appearance.poll();
        let colors = Colors::for_config(
            &self.config,
            ctx.system_theme().or(self.appearance.preferred()),
        );
        if colors != self.colors {
            self.colors = colors;
            crate::theme_colors::apply_to_context(ctx, &colors);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(colors.bg).inner_margin(0))
            .show(ctx, |ui| {
                let mut close = false;
                crate::window_chrome::render_custom_titlebar(
                    ui,
                    ctx,
                    colors,
                    colors.titlebar_bg,
                    self.dialog.request().window_title(),
                    true,
                    &mut close,
                    |_| {},
                );
                if close {
                    self.dialog.cancel();
                }
                egui::Frame::new()
                    .inner_margin(12)
                    .show(ui, |ui| self.dialog.show(ui, colors, &self.config));
            });
        crate::window_chrome::paint_viewport_frame(ctx, colors, self.is_windows);
        if self.dialog.overwrite.is_none() {
            crate::window_chrome::handle_viewport_resize(ctx);
        }
        if let Some(result) = self.dialog.take_result() {
            *self.outcome.lock().expect("picker outcome") = Some(result);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for PickerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.render_window(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ottrin_core::{SavedTheme, ThemeMode, ThemePreset};
    use std::time::{Duration, Instant};

    fn wait_listing(dialog: &mut PickerDialog) {
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.loading {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(dialog.listing_valid, "{:?}", dialog.error);
    }
    fn host(ctx: &Context, request: PickerRequest, dark: bool, scale: f32) -> PickerApp {
        crate::install_application_fonts(ctx);
        let mut colors = Colors::preset(ThemePreset::Ottrin, dark);
        colors.font_scale = scale;
        let custom = crate::OttrinApp::custom_from_colors(colors);
        let config = AppConfig {
            theme: if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            active_custom_theme: Some("Picker QA".into()),
            custom_themes: vec![SavedTheme {
                name: "Picker QA".into(),
                base_preset: ThemePreset::Ottrin,
                light: custom.clone(),
                dark: custom,
            }],
            ..Default::default()
        };
        crate::theme_colors::apply_to_context(ctx, &colors);
        let mut dialog = PickerDialog::new(request, ctx).unwrap();
        wait_listing(&mut dialog);
        PickerApp {
            dialog,
            config,
            colors,
            appearance: Default::default(),
            is_windows: cfg!(windows),
            outcome: Default::default(),
        }
    }
    fn frame(
        app: &mut PickerApp,
        ctx: &Context,
        size: Vec2,
        time: f64,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                modifiers: events
                    .iter()
                    .rev()
                    .find_map(|event| match event {
                        egui::Event::Key { modifiers, .. }
                        | egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                        _ => None,
                    })
                    .unwrap_or_default(),
                events,
                ..Default::default()
            },
            |ctx| app.render_window(ctx),
        )
    }
    fn text_center(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(label) if label.galley.job.text == text => {
                let rect = egui::Rect::from_min_size(label.pos, label.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect.center())
            }
            _ => None,
        })
    }
    fn click(app: &mut PickerApp, ctx: &Context, size: Vec2, time: f64, point: egui::Pos2) {
        for (offset, pressed) in [(0.0, true), (0.02, false)] {
            frame(
                app,
                ctx,
                size,
                time + offset,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn rendered_picker_keeps_files_and_actions_visible_and_opens_the_clicked_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("alpha.txt"), "original").unwrap();
        std::fs::create_dir(root.path().join("subfolder")).unwrap();
        for dark in [false, true] {
            for scale in [1.0, 1.35] {
                for size in [Vec2::new(680.0, 480.0), Vec2::new(940.0, 640.0)] {
                    let ctx = Context::default();
                    let request = PickerRequest {
                        initial_directory: Some(root.path().to_path_buf()),
                        ..Default::default()
                    };
                    let mut app = host(&ctx, request, dark, scale);
                    frame(&mut app, &ctx, size, 0.0, vec![]);
                    let output = frame(&mut app, &ctx, size, 0.1, vec![]);
                    let file = text_center(&output, "alpha.txt")
                        .expect("file row must be wholly inside the picker");
                    let open = text_center(&output, "Open")
                        .expect("Open action must be wholly inside the picker");
                    assert!(text_center(&output, "Cancel").is_some());
                    assert!(
                        file.x > 150.0 * scale,
                        "Places must not cover the file pane"
                    );
                    click(&mut app, &ctx, size, 0.2, file);
                    click(&mut app, &ctx, size, 0.4, open);
                    let until = Instant::now() + Duration::from_secs(3);
                    while app.outcome.lock().unwrap().is_none() {
                        frame(&mut app, &ctx, size, 0.5, vec![]);
                        assert!(Instant::now() < until, "{:?}", app.dialog.error);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    let result = app.outcome.lock().unwrap().take().unwrap();
                    assert!(result.accepted);
                    assert_eq!(result.paths, vec![root.path().join("alpha.txt")]);
                }
            }
        }
        assert_eq!(
            std::fs::read(root.path().join("alpha.txt")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn caller_options_remain_bounded_and_cannot_push_cancel_or_open_offscreen() {
        let root = tempfile::tempdir().unwrap();
        for scale in [1.0, 1.35] {
            for expanded in [false, true] {
                let ctx = Context::default();
                let request = PickerRequest {
                    initial_directory: Some(root.path().to_path_buf()),
                    choices: (0..64)
                        .map(|id| ottrin_core::picker::PickerChoice {
                            id: format!("choice-{id}"),
                            label: format!("Choice {id}: {}", "a long caller label ".repeat(20)),
                            options: vec![],
                            selected: "false".into(),
                        })
                        .collect(),
                    ..Default::default()
                };
                let mut app = host(&ctx, request, true, scale);
                app.dialog.options_expanded = expanded;
                frame(&mut app, &ctx, Vec2::new(680.0, 480.0), 0.0, vec![]);
                let output = frame(&mut app, &ctx, Vec2::new(680.0, 480.0), 0.1, vec![]);
                assert!(
                    text_center(&output, "Cancel").is_some(),
                    "Cancel must remain visible with{scale} scale,expanded={expanded}"
                );
                assert!(
                    text_center(&output, "Open").is_some(),
                    "Open must remain visible with{scale} scale,expanded={expanded}"
                );
            }
        }
    }

    #[test]
    fn filtered_out_typed_names_are_rejected_and_save_confirmation_never_changes_a_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("existing.txt");
        std::fs::write(&path, "original").unwrap();
        let ctx = Context::default();
        let request = PickerRequest {
            initial_directory: Some(root.path().to_path_buf()),
            filters: vec![ottrin_core::picker::PickerFilter {
                name: "PNG images".into(),
                patterns: vec![PickerPattern::Mime("image/png".into())],
            }],
            ..Default::default()
        };
        let mut dialog = PickerDialog::new(request, &ctx).unwrap();
        wait_listing(&mut dialog);
        dialog.filename = "existing.txt".into();
        dialog.accept(false);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(dialog.error.as_ref().unwrap().contains("file type"));
        assert!(dialog.take_result().is_none());
        drop(dialog);
        let request = PickerRequest {
            mode: PickerMode::Save,
            initial_directory: Some(root.path().to_path_buf()),
            filename: "existing.txt".into(),
            ..Default::default()
        };
        let mut dialog = PickerDialog::new(request, &ctx).unwrap();
        wait_listing(&mut dialog);
        dialog.accept(false);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(dialog.overwrite.as_ref(), Some(&path));
        assert!(dialog.take_result().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        dialog.overwrite = None;
        dialog.accept(true);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(dialog.take_result().unwrap().paths, vec![path.clone()]);
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }
    #[test]
    fn typed_hidden_folders_remain_navigable_with_an_active_file_filter() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".hidden");
        std::fs::create_dir(&directory).unwrap();
        let ctx = Context::default();
        let request = PickerRequest {
            initial_directory: Some(root.path().to_path_buf()),
            filters: vec![ottrin_core::picker::PickerFilter {
                name: "PNG images".into(),
                patterns: vec![PickerPattern::Mime("image/png".into())],
            }],
            ..Default::default()
        };
        let mut dialog = PickerDialog::new(request, &ctx).unwrap();
        wait_listing(&mut dialog);
        assert!(dialog.entries.is_empty());
        dialog.filename = ".hidden".into();
        dialog.accept(false);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(dialog.directory, directory);
        wait_listing(&mut dialog);
        assert!(dialog.take_result().is_none());
    }
    #[test]
    fn multiselect_deselection_keeps_filename_and_preview_on_a_selected_file() {
        let root = tempfile::tempdir().unwrap();
        for name in ["alpha.txt", "beta.txt"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let ctx = Context::default();
        let mut dialog = PickerDialog::new(
            PickerRequest {
                multiple: true,
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            &ctx,
        )
        .unwrap();
        wait_listing(&mut dialog);
        let visible = dialog.visible_entries();
        let ctrl = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        dialog.select(visible.get(0), 0, &visible, Default::default());
        dialog.select(visible.get(1), 1, &visible, ctrl);
        dialog.select(visible.get(1), 1, &visible, ctrl);
        assert_eq!(dialog.selected, vec![root.path().join("alpha.txt")]);
        assert_eq!(dialog.focus, Some(root.path().join("alpha.txt")));
        assert_eq!(dialog.filename, "alpha.txt");
        dialog.select(visible.get(0), 0, &visible, ctrl);
        assert!(dialog.selected.is_empty());
        assert!(dialog.focus.is_none());
        assert!(dialog.filename.is_empty());
    }

    #[test]
    fn rendered_read_only_preview_does_not_cover_actions_or_change_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("image.png");
        image::RgbaImage::from_pixel(40, 60, image::Rgba([20, 50, 80, 255]))
            .save(&path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        for dark in [true, false] {
            for scale in [1.0, 1.35] {
                let ctx = Context::default();
                egui_extras::install_image_loaders(&ctx);
                let mut app = host(
                    &ctx,
                    PickerRequest {
                        initial_directory: Some(root.path().to_path_buf()),
                        ..Default::default()
                    },
                    dark,
                    scale,
                );
                app.dialog.preview = Some(crate::picker_preview::PickerPreview::new(&ctx));
                let visible = app.dialog.visible_entries();
                app.dialog
                    .select(visible.get(0), 0, &visible, Default::default());
                let size = Vec2::new(680.0, 480.0);
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut time = 0.0;
                loop {
                    let output = frame(&mut app, &ctx, size, time, vec![]);
                    assert!(text_center(&output, "Open").is_some());
                    assert!(text_center(&output, "Cancel").is_some());
                    assert!(text_center(&output, "Preview").is_some());
                    if app.dialog.preview.as_ref().unwrap().has_image() {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Production image preview did not arrive"
                    );
                    time += 0.02;
                    std::thread::sleep(Duration::from_millis(2));
                }
                frame(
                    &mut app,
                    &ctx,
                    size,
                    time + 0.1,
                    vec![egui::Event::Text(" ".into())],
                );
                assert_eq!(std::fs::read(&path).unwrap(), original);
                app.dialog.cancel();
                assert!(!app.dialog.take_result().unwrap().accepted);
            }
        }
    }
    #[test]
    fn keyboard_navigation_after_a_pointer_click_opens_the_next_file() {
        let root = tempfile::tempdir().unwrap();
        for name in ["alpha.txt", "beta.txt"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let ctx = Context::default();
        let size = Vec2::new(940.0, 640.0);
        let mut app = host(
            &ctx,
            PickerRequest {
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            true,
            1.0,
        );
        frame(&mut app, &ctx, size, 0.0, vec![]);
        let output = frame(&mut app, &ctx, size, 0.1, vec![]);
        click(
            &mut app,
            &ctx,
            size,
            0.2,
            text_center(&output, "alpha.txt").unwrap(),
        );
        for (time, key) in [(0.4, egui::Key::ArrowDown), (0.6, egui::Key::Enter)] {
            for (offset, pressed) in [(0.0, true), (0.02, false)] {
                frame(
                    &mut app,
                    &ctx,
                    size,
                    time + offset,
                    vec![egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                );
            }
        }
        let until = Instant::now() + Duration::from_secs(3);
        while app.dialog.validating {
            frame(&mut app, &ctx, size, 1.0, vec![]);
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            app.outcome
                .lock()
                .unwrap()
                .as_ref()
                .expect("Enter must accept")
                .paths,
            vec![root.path().join("beta.txt")]
        );
    }
    #[test]
    fn text_preview_uses_the_readable_shared_body_instead_of_a_thumbnail() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("alpha.txt");
        std::fs::write(&path, "Readable chooser text").unwrap();
        let ctx = Context::default();
        let mut app = host(
            &ctx,
            PickerRequest {
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            true,
            1.0,
        );
        app.dialog.preview = Some(crate::picker_preview::PickerPreview::new(&ctx));
        let visible = app.dialog.visible_entries();
        app.dialog
            .select(visible.get(0), 0, &visible, Default::default());
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut time = 0.0;
        loop {
            let output = frame(&mut app, &ctx, Vec2::new(940.0, 640.0), time, vec![]);
            if output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(label)
                if label.galley.job.text.contains("Readable chooser text"))
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Readable production text body did not arrive"
            );
            time += 0.02;
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!app.dialog.preview.as_ref().unwrap().has_image());
        assert_eq!(std::fs::read(path).unwrap(), b"Readable chooser text");
    }
    #[test]
    fn location_shortcut_replaces_the_path_on_repeated_navigation() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        std::fs::create_dir(&child).unwrap();
        let ctx = Context::default();
        let size = Vec2::new(940.0, 640.0);
        let mut app = host(
            &ctx,
            PickerRequest {
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            false,
            1.0,
        );
        let mut time = 0.0;
        frame(&mut app, &ctx, size, time, vec![]);
        for target in [&child, &root.path().to_path_buf()] {
            time += 0.1;
            let modifiers = egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            };
            frame(
                &mut app,
                &ctx,
                size,
                time,
                vec![egui::Event::Key {
                    key: egui::Key::L,
                    physical_key: Some(egui::Key::L),
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
            );
            assert!(
                app.dialog.editing_location,
                "Ctrl+L must open the location editor"
            );
            time += 0.1;
            frame(
                &mut app,
                &ctx,
                size,
                time,
                vec![egui::Event::Text(target.to_string_lossy().into_owned())],
            );
            assert_eq!(app.dialog.location, target.to_string_lossy());
            time += 0.1;
            frame(
                &mut app,
                &ctx,
                size,
                time,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: Some(egui::Key::Enter),
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
            assert_eq!(&app.dialog.directory, target);
            wait_listing(&mut app.dialog);
            time += 0.1;
            frame(
                &mut app,
                &ctx,
                size,
                time,
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: Some(egui::Key::Enter),
                    pressed: false,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
        }
    }
}
