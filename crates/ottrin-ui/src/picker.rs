//! The actual chooser surface, shared by internal dialogs and standalone requests.
//! Directory browsing never starts or waits for an index daemon.
use crate::icons::*;
use crate::picker_preferences::PickerPreferences;
use crate::widget_style::{ThemeButtonExt, ThemeTextEditExt};
use crate::{Colors, EntryVisualState, file_row, listing, path_crumbs, path_label};
use egui::{Align, Context, Layout, RichText, ScrollArea, Vec2};
use ottrin_core::picker::{
    PickerMode, PickerPattern, PickerRequest, PickerResponse, is_picker_filename,
};
use ottrin_core::{
    AppConfig, EntryKind, FileEntry, SearchIndexStatus, SearchQuery, SearchScope, SearchSort,
    SortConfig, default_home_dir,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

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
    Search {
        generation: u64,
        query: SearchQuery,
        cancelled: Arc<AtomicBool>,
    },
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
    Search {
        generation: u64,
        result: Result<ottrin_search::SearchQueryResult, String>,
    },
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
                    Work::Search {
                        generation,
                        query,
                        cancelled,
                    } => {
                        if cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        ResultMessage::Search {
                            generation,
                            result: ottrin_search::SearchDaemonClient::connect_default()
                                .and_then(|client| client.query_cancellable(query, &cancelled)),
                        }
                    }
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
    location: OsString,
    editing_location: bool,
    focus_location: bool,
    entries: Arc<Vec<FileEntry>>,
    visible: Arc<Vec<usize>>,
    visible_key: Option<(u64, Option<usize>, String)>,
    selected: Vec<PathBuf>,
    anchor: Option<usize>,
    focus: Option<PathBuf>,
    preview: Option<crate::picker_preview::PickerPreview>,
    preview_enabled: bool,
    filename: OsString,
    query: String,
    indexed: bool,
    search_cancelled: Option<Arc<AtomicBool>>,
    search_due: Option<Instant>,
    search_note: Option<String>,
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
    pub fn new(
        request: PickerRequest,
        ctx: &Context,
        preferences: PickerPreferences,
    ) -> Result<Self, String> {
        request.validate()?;
        let filters = request
            .filters
            .iter()
            .map(Filter::compile)
            .collect::<Result<Vec<_>, _>>()?;
        let directory = match request.initial_directory.clone() {
            Some(directory) => directory,
            None => preferences.directory()?.unwrap_or_else(default_home_dir),
        };
        let filename = OsString::from(&request.filename);
        let choices = request
            .choices
            .iter()
            .map(|choice| (choice.id.clone(), choice.selected.clone()))
            .collect();
        let mut dialog = Self {
            request,
            filters,
            location: directory.as_os_str().to_os_string(),
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
            preview_enabled: false,
            filename,
            query: String::new(),
            indexed: false,
            search_cancelled: None,
            search_due: None,
            search_note: None,
            hidden: preferences.hidden,
            grid: preferences.grid,
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
        dialog.set_preview_enabled(preferences.preview, ctx);
        dialog.scan();
        Ok(dialog)
    }
    pub fn request(&self) -> &PickerRequest {
        &self.request
    }
    fn set_preview_enabled(&mut self, enabled: bool, ctx: &Context) {
        self.preview_enabled = enabled;
        self.preview = (enabled && self.request.mode != PickerMode::Folder)
            .then(|| crate::picker_preview::PickerPreview::new(ctx));
    }
    pub(crate) fn preferences(&self) -> PickerPreferences {
        PickerPreferences {
            grid: self.grid,
            hidden: self.hidden,
            preview: self.preview_enabled,
            last_directory: Some(ottrin_core::native_path::NativePath::encode(
                &self.directory,
            )),
        }
    }
    pub fn take_result(&mut self) -> Option<PickerResponse> {
        self.result.take()
    }
    pub fn cancel(&mut self) {
        let mut response = self.request.cancelled_response();
        response.choices.clone_from(&self.choices);
        self.result = Some(response);
    }
    fn cancel_search(&mut self) {
        if let Some(cancelled) = self.search_cancelled.take() {
            cancelled.store(true, Ordering::Release);
        }
        self.search_due = None;
    }
    fn schedule_search(&mut self, ctx: &Context) {
        self.cancel_search();
        self.generation += 1;
        self.clear_selection();
        self.entries = Arc::new(Vec::new());
        self.visible_key = None;
        self.error = None;
        self.overwrite = None;
        self.search_note = None;
        self.listing_valid = false;
        self.loading = !self.query.trim().is_empty();
        if self.loading {
            self.search_due = Some(Instant::now() + Duration::from_millis(150));
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
    fn dispatch_search(&mut self) {
        let cancelled = Arc::new(AtomicBool::new(false));
        self.search_cancelled = Some(cancelled.clone());
        let text = if self.request.mode == PickerMode::Folder {
            format!("({}) (type:folder | type:link)", self.query)
        } else {
            self.query.clone()
        };
        if let Err(error) = self.worker.send(Work::Search {
            generation: self.generation,
            query: SearchQuery {
                text,
                scope: SearchScope::Global,
                root_path: None,
                include_hidden_system: self.hidden,
                sort: SearchSort::Relevance,
                limit: 200,
                offset: 0,
                content_search: false,
            },
            cancelled,
        }) {
            self.loading = false;
            self.error = Some(error);
        }
    }
    fn scan(&mut self) {
        self.cancel_search();
        self.search_note = None;
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
        self.indexed = false;
        self.directory = if directory.is_absolute() {
            directory
        } else {
            self.directory.join(directory)
        };
        self.location = self.directory.as_os_str().to_os_string();
        self.editing_location = false;
        if self.request.mode != PickerMode::Save {
            self.filename.clear();
        }
        self.query.clear();
        self.scan();
    }
    fn poll(&mut self) {
        if self.search_due.is_some_and(|due| Instant::now() >= due) {
            self.search_due = None;
            self.dispatch_search();
        }
        while let Ok(message) = self.worker.results.try_recv() {
            match message {
                ResultMessage::Search { generation, result } if generation == self.generation => {
                    self.loading = false;
                    self.search_cancelled = None;
                    match result {
                        Ok(result) => {
                            let response = result.response;
                            if let Some(error) = response.error {
                                self.error = Some(error);
                            } else if response.status == SearchIndexStatus::Unavailable {
                                self.error = Some(
                                    "Index unavailable. Choose This folder to browse directly."
                                        .into(),
                                );
                            } else {
                                let count = response.items.len();
                                let mut notes = Vec::new();
                                if response.total > count {
                                    notes.push(format!("First {count} of {} index matches; refine your query. File type filters apply to these results.", response.total));
                                }
                                if response.status != SearchIndexStatus::Ready {
                                    notes.push("The index is still loading or updating; results may be incomplete.".into());
                                }
                                if let Some(warning) = result.warning {
                                    notes.push(warning);
                                }
                                self.search_note = (!notes.is_empty()).then(|| notes.join(" "));
                                self.entries = Arc::new(
                                    response
                                        .items
                                        .into_iter()
                                        .map(|item| FileEntry {
                                            name: item.name,
                                            path: item.path,
                                            kind: item.kind,
                                            is_executable: item.is_executable,
                                            symlink_target_is_dir: item.symlink_target_is_dir,
                                            size_bytes: item.size_bytes,
                                            modified_unix_secs: item.modified_unix_secs,
                                        })
                                        .collect(),
                                );
                                self.visible_key = None;
                                self.listing_valid = true;
                            }
                        }
                        Err(error) => {
                            self.error = Some(format!(
                                "Index unavailable. Choose This folder to browse directly. Details: {error}"
                            ))
                        }
                    }
                }
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
                            && (self.indexed
                                || query.is_empty()
                                || entry.name.to_lowercase().contains(&query))
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
                .map(|name| name.to_os_string())
                .unwrap_or_default();
        } else if !is_directory(entry) {
            self.filename = entry.path.file_name().unwrap_or_default().to_os_string();
        }
        if self.indexed
            && let Some(parent) = self.focus.as_ref().and_then(|path| path.parent())
        {
            self.directory = parent.to_path_buf();
            self.location = self.directory.as_os_str().to_os_string();
        }
        self.error = None;
    }
    fn accept(&mut self, confirmed: bool) {
        if self.loading || self.validating || !self.listing_valid {
            return;
        }
        let paths = if self.request.mode == PickerMode::Save {
            // The native basename survives selection resets and folder changes.
            // Its lossy presentation is never used to construct a filesystem path.
            let mut components = Path::new(&self.filename).components();
            if !matches!(components.next(), Some(std::path::Component::Normal(name)) if name == self.filename.as_os_str())
                || components.next().is_some()
                || self.filename.as_encoded_bytes().contains(&0)
            {
                self.error =
                    Some("Enter one filename. Use Location to choose another folder.".into());
                return;
            }
            let path = self.directory.join(&self.filename);
            if !self.entries.iter().any(|entry| entry.path == path) {
                let displayed = self.filename.to_string_lossy();
                if !is_picker_filename(&displayed) {
                    self.error =
                        Some("Enter one filename. Use Location to choose another folder.".into());
                    return;
                }
                if let Err(error) = ottrin_platform::validate_creation_name(&displayed) {
                    self.error = Some(error.to_string());
                    return;
                }
            }
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
        } else if is_picker_filename(&self.filename.to_string_lossy()) {
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
        if ctx.input(|input| crate::command_key_pressed(input, egui::Key::A))
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
        if let Some(due) = self.search_due {
            ctx.request_repaint_after(due.saturating_duration_since(Instant::now()));
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape))
            && self.overwrite.take().is_none()
        {
            if self.editing_location {
                self.editing_location = false;
                self.location = self.directory.as_os_str().to_os_string();
            } else {
                self.cancel();
            }
        }
        if ctx.input(|input| crate::command_key_pressed(input, egui::Key::L)) {
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
                            let mut location = self.location.to_string_lossy().into_owned();
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
                                        egui::text::CCursor::new(location.chars().count()),
                                    )));
                                state.store(&ctx, editor_id);
                            }
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut location)
                                    .id(editor_id)
                                    .char_limit(32768)
                                    .desired_width(ui.available_width() * 0.62)
                                    .themed(&colors),
                            );
                            if response.changed() {
                                self.location = location.into();
                            }
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
                                .hint_text(if self.indexed { "Search indexed folders" } else { "Filter this folder" })
                                .char_limit(1024)
                                .desired_width(ui.available_width().max(80.0))
                                .themed(&colors),
                        );
                        search.clone().on_hover_text(if self.indexed {
                            "Uses Ottrin search syntax, like the main Search panel. Searches configured indexed folders without starting the index service."
                        } else { "Filter filenames in this folder." });
                        if search.changed() {
                            if self.indexed { self.schedule_search(&ctx); } else { self.clear_selection(); }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let previous = self.indexed;
                        egui::ComboBox::from_id_salt("picker-search-scope")
                            .selected_text(if self.indexed { "Indexed folders" } else { "This folder" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.indexed, false, "This folder");
                                ui.selectable_value(&mut self.indexed, true, "Indexed folders");
                            });
                        if previous != self.indexed {
                            if self.indexed { self.schedule_search(&ctx); } else { self.query.clear(); self.scan(); }
                        }
                        ui.label(
                            RichText::new(if self.loading && self.indexed {
                                "Searching…".into()
                            } else if self.loading {
                                "Reading folder…".into()
                            } else {
                                let count = self.visible_entries().len();
                                format!("{count} {}", if count == 1 { "item" } else { "items" })
                            })
                            .color(colors.text_muted)
                            .small(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.request.mode != PickerMode::Folder
                                && ui
                                    .add(
                                        egui::Button::new(MI_VISIBILITY)
                                            .selected(self.preview_enabled)
                                            .themed(&colors),
                                    )
                                    .on_hover_text("Show read-only preview")
                                    .clicked()
                            {
                                self.set_preview_enabled(!self.preview_enabled, &ctx);
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
                                if self.indexed { self.schedule_search(&ctx); } else { self.scan(); }
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
            if let Some(note) = &self.search_note {
                ui.add(egui::Label::new(RichText::new(note).small().color(colors.text_muted)).truncate()).on_hover_text(note);
            }
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
                        // A sparse grid must still own its whole file column;
                        // otherwise horizontal layout gives that space to Preview.
                        ui.set_min_width(width);
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(ui.cursor().min, Vec2::new(width, height)),
                            0.0,
                            colors.bg,
                        );
                        if self.loading {
                            ui.spinner();
                            ui.label(if self.indexed { "Searching indexed folders…" } else { "Reading folder…" });
                        }
                        let visible = self.visible_entries();
                        let grid_metrics =
                            crate::file_grid::GridMetrics::new(config.grid_view_scale, colors);
                        let columns = ((ui.available_width() + grid_metrics.gap)
                            / (grid_metrics.size.x + grid_metrics.gap))
                            .floor()
                            .max(1.0) as usize;
                        let row_count = if self.grid {
                            visible.len().div_ceil(columns)
                        } else {
                            visible.len()
                        };
                        let row_height = if self.grid {
                            grid_metrics.size.y
                        } else {
                            (25.0 * colors.font_scale).round() + if self.indexed { ui.text_style_height(&egui::TextStyle::Small) + ui.spacing().item_spacing.y } else { 0.0 }
                        };
                        ScrollArea::vertical()
                            .id_salt("picker-files")
                            .max_height(height)
                            .show_rows(ui, row_height, row_count, |ui, range| {
                                if self.grid {
                                    egui::Grid::new("picker-grid")
                                        .num_columns(columns)
                                        .spacing(Vec2::new(
                                            grid_metrics.gap,
                                            ui.spacing().item_spacing.y,
                                        ))
                                        .min_col_width(grid_metrics.size.x)
                                        .show(ui, |ui| {
                                            for index in (range.start * columns)
                                                ..(range.end * columns).min(visible.len())
                                            {
                                                let entry = visible.get(index);
                                                let selected = self.selected.contains(&entry.path);
                                                let response =
                                                    crate::file_grid::render_file_grid_cell(
                                                        ui,
                                                        entry,
                                                        selected,
                                                        config.grid_view_scale,
                                                        colors,
                                                        config,
                                                        crate::file_grid::GridThumbnail::Icon,
                                                    );
                                                let response = response.on_hover_text(path_label(&entry.path));
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
                                        let response = if self.indexed {
                                            let location = ui.horizontal(|ui| {
                                                ui.add_space(30.0);
                                                ui.add(egui::Label::new(RichText::new(entry.path.parent().map(path_label).unwrap_or_default()).small().color(colors.text_muted)).truncate().sense(egui::Sense::click()))
                                                    .on_hover_text(path_label(&entry.path))
                                            }).inner;
                                            response.union(location)
                                        } else { response };
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
                                    ui.label(if self.indexed && self.query.trim().is_empty() {
                                        "Enter a query to search indexed folders"
                                    } else if self.indexed && self.error.is_some() {
                                        "Choose This folder to continue browsing without the index."
                                    } else if self.indexed {
                                        "No matching indexed files or folders"
                                    } else if self.query.is_empty() {
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
                    let mut filename = self.filename.to_string_lossy().into_owned();
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut filename)
                            .char_limit(4096)
                            .desired_width(ui.available_width())
                            .themed(&colors),
                    );
                    if response.changed() {
                        self.filename = filename.into();
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

impl Drop for PickerDialog {
    fn drop(&mut self) {
        self.cancel_search();
    }
}

/// Standalone host for portal and cooperating-application requests. It loads no
/// file-manager operation workers. Only explicit indexed search connects to an existing index.
pub struct PickerApp {
    parent: Option<crate::picker_parent::PickerParent>,
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
        let parent = crate::picker_parent::PickerParent::attach(cc, &request.parent_window)?;
        Ok(Self {
            parent,
            dialog: PickerDialog::new(
                request,
                &cc.egui_ctx,
                PickerPreferences::load().unwrap_or_else(|error| {
                    eprintln!("ottrin: {error}; using default picker preferences");
                    PickerPreferences::default()
                }),
            )?,
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
        if self.parent.as_ref().is_some_and(|parent| parent.gone()) {
            self.dialog.cancel();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.dialog.cancel();
        }
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
            .frame(egui::Frame::new().fill(colors.panel).inner_margin(0))
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
        if self.finish_response() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn finish_response(&mut self) -> bool {
        if let Some(result) = self.dialog.take_result() {
            if let Err(error) = self.dialog.preferences().save() {
                eprintln!("ottrin: Could not remember picker preferences: {error}");
            }
            *self.outcome.lock().expect("picker outcome") = Some(result);
            true
        } else {
            false
        }
    }
}

impl eframe::App for PickerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Native parents borrow winit's display; release them before window teardown.
        self.parent.take();
        if self.outcome.lock().expect("picker outcome").is_none() {
            self.dialog.cancel();
            self.finish_response();
        }
    }
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
        let mut dialog = PickerDialog::new(request, ctx, PickerPreferences::default()).unwrap();
        wait_listing(&mut dialog);
        PickerApp {
            parent: None,
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
        let mut dialog = PickerDialog::new(request, &ctx, PickerPreferences::default()).unwrap();
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
        let mut dialog = PickerDialog::new(request, &ctx, PickerPreferences::default()).unwrap();
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
        let mut dialog = PickerDialog::new(request, &ctx, PickerPreferences::default()).unwrap();
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
            PickerPreferences::default(),
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
    fn preview_keeps_its_column_when_grid_contents_are_sparse_or_empty() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("one.txt"), "preview content").unwrap();
        for dark in [true, false] {
            for scale in [1.0, 1.5] {
                let ctx = Context::default();
                let mut app = host(
                    &ctx,
                    PickerRequest {
                        initial_directory: Some(root.path().to_path_buf()),
                        ..Default::default()
                    },
                    dark,
                    scale,
                );
                app.dialog.set_preview_enabled(true, &ctx);
                let size = Vec2::new(940.0, 640.0);
                frame(&mut app, &ctx, size, 0.0, vec![]);
                let list = frame(&mut app, &ctx, size, 0.1, vec![]);
                let column = text_center(&list, "Preview").unwrap().x;
                app.dialog.grid = true;
                let grid = frame(&mut app, &ctx, size, 0.2, vec![]);
                let sparse_column = text_center(&grid, "Preview").unwrap().x;
                assert!(
                    (column - sparse_column).abs() < 1.0,
                    "Preview moved from {column} to {sparse_column} for one grid item"
                );
                app.dialog.query = "no matching file".into();
                let empty = frame(&mut app, &ctx, size, 0.3, vec![]);
                let empty_column = text_center(&empty, "Preview").unwrap().x;
                assert!((column - empty_column).abs() < 1.0);
                assert!(text_center(&empty, "Open").is_some());
                assert!(text_center(&empty, "Cancel").is_some());
            }
        }
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
                app.dialog.set_preview_enabled(true, &ctx);
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
        app.dialog.set_preview_enabled(true, &ctx);
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
            assert_eq!(app.dialog.location, target.as_os_str());
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
    #[test]
    fn caller_location_overrides_remembered_folder_without_resetting_picker_options() {
        let remembered = tempfile::tempdir().unwrap();
        let requested = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let preferences = PickerPreferences {
            grid: true,
            hidden: true,
            preview: true,
            last_directory: Some(ottrin_core::native_path::NativePath::encode(
                remembered.path(),
            )),
        };
        let dialog = PickerDialog::new(
            PickerRequest {
                initial_directory: Some(requested.path().to_path_buf()),
                ..Default::default()
            },
            &ctx,
            preferences,
        )
        .unwrap();
        assert_eq!(dialog.directory, requested.path());
        assert!(dialog.grid && dialog.hidden && dialog.preview.is_some());
    }
    #[test]
    fn location_shortcut_uses_event_modifiers_when_the_final_snapshot_has_released_ctrl() {
        let root = tempfile::tempdir().unwrap();
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
        let event_modifiers = egui::Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(940.0, 640.0),
                )),
                modifiers: Default::default(),
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::L,
                        physical_key: Some(egui::Key::L),
                        pressed: true,
                        repeat: false,
                        modifiers: event_modifiers,
                    },
                    egui::Event::Key {
                        key: egui::Key::L,
                        physical_key: Some(egui::Key::L),
                        pressed: false,
                        repeat: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| app.render_window(ctx),
        );
        assert!(
            app.dialog.editing_location,
            "Ctrl+L must survive a released final modifier snapshot"
        );
    }
    #[test]
    fn folder_only_picker_preserves_file_preview_preference_without_starting_its_workers() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let dialog = PickerDialog::new(
            PickerRequest {
                mode: PickerMode::Folder,
                initial_directory: Some(root.path().to_path_buf()),
                ..Default::default()
            },
            &ctx,
            PickerPreferences {
                preview: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(dialog.preview.is_none());
        assert!(dialog.preferences().preview);
    }
    fn capture_work(
        dialog: &mut PickerDialog,
    ) -> (mpsc::Receiver<Work>, mpsc::Sender<ResultMessage>) {
        let (sender, requests) = mpsc::channel();
        let (completed, results) = mpsc::channel();
        dialog.worker = Worker {
            sender: Some(sender),
            results,
            stopped: Arc::new(AtomicBool::new(false)),
        };
        (requests, completed)
    }
    fn search_result(path: &Path) -> ottrin_search::SearchQueryResult {
        ottrin_search::SearchQueryResult {
            response: ottrin_core::SearchResponse {
                items: vec![ottrin_core::SearchResultItem {
                    path: path.to_path_buf(),
                    parent_path: path.parent().unwrap().to_path_buf(),
                    name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    kind: EntryKind::File,
                    is_executable: false,
                    symlink_target_is_dir: None,
                    size_bytes: Some(8),
                    modified_unix_secs: None,
                    content_snippet: None,
                    name_lower: String::new(),
                    path_str: String::new(),
                    path_lower: String::new(),
                }],
                total: 1,
                status: SearchIndexStatus::Ready,
                error: None,
            },
            warning: None,
        }
    }
    #[test]
    fn indexed_search_cancels_abandoned_queries_and_discards_their_results() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let mut app = host(
            &ctx,
            PickerRequest {
                initial_directory: Some(root.path().into()),
                ..Default::default()
            },
            true,
            1.0,
        );
        wait_listing(&mut app.dialog);
        let (requests, completed) = capture_work(&mut app.dialog);
        app.dialog.indexed = true;
        app.dialog.query = "ext:txt".into();
        app.dialog.schedule_search(&ctx);
        assert!(
            requests.try_recv().is_err(),
            "typing must debounce, not dispatch immediately"
        );
        app.dialog.search_due = Some(Instant::now());
        app.dialog.poll();
        let Work::Search {
            generation,
            query,
            cancelled,
        } = requests.recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(query.text, "ext:txt");
        assert!(!query.content_search);
        app.dialog.query = "ext:pdf".into();
        app.dialog.schedule_search(&ctx);
        assert!(cancelled.load(Ordering::Acquire));
        completed
            .send(ResultMessage::Search {
                generation,
                result: Ok(search_result(&root.path().join("stale.txt"))),
            })
            .unwrap();
        app.dialog.poll();
        assert!(app.dialog.entries.is_empty());
        assert!(app.dialog.loading);
        app.dialog.navigate(root.path().into());
        assert!(!app.dialog.indexed);
        assert!(app.dialog.search_due.is_none());
        assert!(matches!(requests.recv().unwrap(), Work::Scan { .. }));
    }
    #[test]
    fn indexed_save_uses_the_selected_results_parent_and_rechecks_overwrite() {
        let browsing = tempfile::tempdir().unwrap();
        let results = tempfile::tempdir().unwrap();
        let path = results.path().join("report.txt");
        std::fs::write(&path, "original").unwrap();
        let ctx = Context::default();
        let mut app = host(
            &ctx,
            PickerRequest {
                mode: PickerMode::Save,
                initial_directory: Some(browsing.path().into()),
                ..Default::default()
            },
            true,
            1.0,
        );
        wait_listing(&mut app.dialog);
        let (requests, completed) = capture_work(&mut app.dialog);
        app.dialog.indexed = true;
        app.dialog.query = "ext:txt".into();
        app.dialog.schedule_search(&ctx);
        app.dialog.search_due = None;
        completed
            .send(ResultMessage::Search {
                generation: app.dialog.generation,
                result: Ok(search_result(&path)),
            })
            .unwrap();
        app.dialog.poll();
        let visible = app.dialog.visible_entries();
        assert_eq!(
            visible.len(),
            1,
            "advanced queries must not be filtered as literal filenames"
        );
        app.dialog
            .select(visible.get(0), 0, &visible, Default::default());
        assert_eq!(app.dialog.directory, results.path());
        app.dialog.accept(false);
        let Work::Validate {
            paths,
            mode,
            overwrite_confirmed,
            ..
        } = requests.recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(paths, vec![path.clone()]);
        assert!(
            matches!(validate_selection(paths, mode, overwrite_confirmed, None), Ok(Selection::Overwrite(p)) if p == path)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert!(!browsing.path().join("report.txt").exists());
    }
    #[test]
    fn indexed_unavailability_keeps_actions_visible_and_direct_browsing_recovers() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("local.txt"), "original").unwrap();
        for dark in [false, true] {
            let ctx = Context::default();
            let mut app = host(
                &ctx,
                PickerRequest {
                    initial_directory: Some(root.path().into()),
                    ..Default::default()
                },
                dark,
                1.35,
            );
            wait_listing(&mut app.dialog);
            let (_requests, completed) = capture_work(&mut app.dialog);
            app.dialog.indexed = true;
            app.dialog.query = "query".into();
            app.dialog.schedule_search(&ctx);
            app.dialog.search_due = None;
            completed
                .send(ResultMessage::Search {
                    generation: app.dialog.generation,
                    result: Err("connection refused".repeat(20)),
                })
                .unwrap();
            let size = Vec2::new(680.0, 480.0);
            frame(&mut app, &ctx, size, 0.0, vec![]);
            let output = frame(&mut app, &ctx, size, 0.1, vec![]);
            assert!(text_center(&output, "Open").is_some());
            assert!(text_center(&output, "Cancel").is_some());
            assert!(!app.dialog.listing_valid);
            assert!(app.dialog.error.as_ref().unwrap().contains("This folder"));
            app.dialog.worker = Worker::start(ctx.clone());
            app.dialog.navigate(root.path().into());
            wait_listing(&mut app.dialog);
            assert_eq!(app.dialog.visible_entries().len(), 1);
        }
    }
    #[test]
    fn empty_indexed_search_does_not_connect_and_closing_cancels_inflight_work() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let mut app = host(
            &ctx,
            PickerRequest {
                mode: PickerMode::Folder,
                initial_directory: Some(root.path().into()),
                ..Default::default()
            },
            true,
            1.0,
        );
        wait_listing(&mut app.dialog);
        let (requests, _completed) = capture_work(&mut app.dialog);
        app.dialog.indexed = true;
        app.dialog.query = "  ".into();
        app.dialog.schedule_search(&ctx);
        app.dialog.poll();
        assert!(requests.try_recv().is_err());
        assert!(!app.dialog.loading);
        app.dialog.query = "photos".into();
        app.dialog.schedule_search(&ctx);
        app.dialog.search_due = Some(Instant::now());
        app.dialog.poll();
        let Work::Search {
            query, cancelled, ..
        } = requests.recv().unwrap()
        else {
            panic!()
        };
        assert_eq!(query.text, "(photos) (type:folder | type:link)");
        drop(app);
        assert!(cancelled.load(Ordering::Acquire));
    }
    #[cfg(any(unix, windows))]
    #[test]
    fn confirming_an_unedited_location_preserves_native_directory_units() {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(b"folder\x81".to_vec())
        };
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[102, 0xd800])
        };
        let directory = root.path().join(name);
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("keep.txt"), b"preserve original").unwrap();
        let ctx = Context::default();
        let mut app = host(
            &ctx,
            PickerRequest {
                initial_directory: Some(directory.clone()),
                ..Default::default()
            },
            true,
            1.0,
        );
        let size = Vec2::new(940.0, 640.0);
        frame(&mut app, &ctx, size, 0.0, vec![]);
        frame(
            &mut app,
            &ctx,
            size,
            0.1,
            vec![egui::Event::Key {
                key: egui::Key::L,
                physical_key: Some(egui::Key::L),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
        );
        assert!(app.dialog.editing_location);
        frame(
            &mut app,
            &ctx,
            size,
            0.2,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        wait_listing(&mut app.dialog);
        assert_eq!(app.dialog.directory, directory);
        assert_eq!(app.dialog.visible_entries().len(), 1);
        assert_eq!(
            std::fs::read(directory.join("keep.txt")).unwrap(),
            b"preserve original"
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn native_save_name_survives_refresh_and_navigation_without_a_selection() {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let filename = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"original\x80.txt".to_vec())
        };
        #[cfg(windows)]
        let filename = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[111, 0xd800, 46, 116, 120, 116])
        };
        let original = root.path().join(&filename);
        std::fs::write(&original, b"preserve original").unwrap();
        let other = root.path().join("other");
        std::fs::create_dir(&other).unwrap();
        let ctx = Context::default();
        let mut dialog = PickerDialog::new(
            PickerRequest {
                mode: PickerMode::Save,
                initial_directory: Some(root.path().into()),
                ..Default::default()
            },
            &ctx,
            Default::default(),
        )
        .unwrap();
        wait_listing(&mut dialog);
        let visible = dialog.visible_entries();
        let index = visible
            .iter()
            .position(|entry| entry.path == original)
            .unwrap();
        dialog.select(visible.get(index), index, &visible, Default::default());
        dialog.hidden = true;
        dialog.scan();
        wait_listing(&mut dialog);
        assert!(dialog.selected.is_empty());
        dialog.accept(false);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            dialog.overwrite,
            Some(original.clone()),
            "Refreshing must not replace native filename units with display glyphs"
        );
        dialog.navigate(other.clone());
        wait_listing(&mut dialog);
        dialog.accept(false);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            dialog.take_result().unwrap().paths,
            vec![other.join(filename)]
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"preserve original");
        assert_eq!(std::fs::read_dir(other).unwrap().count(), 0);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn save_selection_and_response_preserve_native_filename_without_mutating_the_file() {
        let root = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let filename = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"original\x80.txt".to_vec())
        };
        #[cfg(windows)]
        let filename = {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[
                111, 114, 105, 103, 105, 110, 97, 108, 0xd800, 46, 116, 120, 116,
            ])
        };
        let path = root.path().join(filename);
        std::fs::write(&path, "preserve original").unwrap();
        let ctx = Context::default();
        let mut dialog = PickerDialog::new(
            PickerRequest {
                mode: PickerMode::Save,
                initial_directory: Some(root.path().into()),
                ..Default::default()
            },
            &ctx,
            Default::default(),
        )
        .unwrap();
        wait_listing(&mut dialog);
        let visible = dialog.visible_entries();
        assert_eq!(visible.len(), 1);
        dialog.select(visible.get(0), 0, &visible, Default::default());
        dialog.accept(false);
        let until = Instant::now() + Duration::from_secs(3);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            dialog.overwrite,
            Some(path.clone()),
            "Existing selected native path must not turn into a different lossy filename"
        );
        dialog.accept(true);
        while dialog.validating {
            dialog.poll();
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
        let response = dialog.take_result().unwrap();
        assert_eq!(response.paths, vec![path.clone()]);
        assert_eq!(
            serde_json::from_slice::<PickerResponse>(&serde_json::to_vec(&response).unwrap())
                .unwrap(),
            response
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve original");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
