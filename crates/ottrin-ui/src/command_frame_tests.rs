//! Frame-driven tests for the command palette's Commands/Files sections.

use super::*;

fn command_frame_fixture() -> (tempfile::TempDir, OttrinApp, PathBuf) {
    let (root, mut app, source) = super::tests::clipboard_keyboard_fixture();
    app.state.active_tab_mut().view_mode = ViewMode::List;
    let tab_id = app.state.active_tab().id;
    // The fixture only fills `main.entries`; the matcher requires the loaded
    // listing to identify the active folder.
    app.tab_ui.entry(tab_id).or_default().main.dir = root.path().to_path_buf();
    (root, app, source)
}

fn add_file(app: &mut OttrinApp, root: &Path, name: &str) -> FileEntry {
    let tab_id = app.state.active_tab().id;
    let ui = app.tab_ui.entry(tab_id).or_default();
    let mut entry = ui
        .main
        .entries
        .first()
        .cloned()
        .expect("fixture-provided entry");
    entry.name = name.to_string();
    entry.path = root.join(name);
    entry.kind = EntryKind::File;
    entry.is_executable = false;
    entry.symlink_target_is_dir = None;
    entry.size_bytes = Some(13);
    entry.modified_unix_secs = None;
    entry.created_unix_secs = None;
    std::fs::write(&entry.path, b"command frame fixture").unwrap();
    ui.main.entries.push(entry.clone());
    entry
}

fn add_directory(app: &mut OttrinApp, root: &Path, name: &str) -> FileEntry {
    let tab_id = app.state.active_tab().id;
    let ui = app.tab_ui.entry(tab_id).or_default();
    let mut entry = ui
        .main
        .entries
        .first()
        .cloned()
        .expect("fixture-provided entry");
    entry.name = name.to_string();
    entry.path = root.join(name);
    entry.kind = EntryKind::Directory;
    entry.is_executable = false;
    entry.symlink_target_is_dir = None;
    entry.size_bytes = None;
    entry.modified_unix_secs = None;
    entry.created_unix_secs = None;
    std::fs::create_dir_all(&entry.path).unwrap();
    ui.main.entries.push(entry.clone());
    entry
}

fn open_command_frame(app: &mut OttrinApp, input: &str) {
    app.command_frame.visible = true;
    app.command_frame.input = input.to_string();
    app.command_frame.history_idx = None;
    app.command_frame.request_focus = true;
    app.command_frame.error = None;
    app.command_frame.message = None;
    app.command_frame.completions.clear();
    app.command_frame.file_matches.clear();
    app.command_frame.file_focus = None;
    app.command_frame.file_query.clear();
    app.command_frame.file_directory = None;
    app.command_frame.reveal_file = false;
}

fn run_command_frame(app: &mut OttrinApp, ctx: &Context, events: Vec<egui::Event>) {
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(960.0, 640.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.render_command_frame(ctx),
    );
}

fn key_event(key: Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn prime_command_input(app: &mut OttrinApp, ctx: &Context) {
    run_command_frame(app, ctx, Vec::new());
    assert!(
        ctx.memory(|memory| memory.has_focus(egui::Id::new("cmd_input_field"))),
        "command input should hold keyboard focus"
    );
}

fn match_names(app: &OttrinApp) -> Vec<String> {
    app.command_frame
        .file_matches
        .iter()
        .map(|entry| entry.name.clone())
        .collect()
}

#[test]
fn tab_focuses_first_matching_file_and_uses_real_file_selection() {
    let (root, mut app, source) = command_frame_fixture();
    add_file(&mut app, root.path(), "alpha-report.xls");
    add_file(&mut app, root.path(), "beta-report.xls");
    let tab_id = app.state.active_tab().id;
    {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        ui.main_sel = Some(1);
        ui.selected_paths = [source.clone()].into_iter().collect();
    }

    open_command_frame(&mut app, "report");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert_eq!(
        match_names(&app),
        vec!["alpha-report.xls", "beta-report.xls"]
    );
    assert_eq!(app.command_frame.file_focus, None);

    let expected = app.command_frame.file_matches[0].path.clone();
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&expected));
    assert_eq!(app.state.active_tab().current_dir, root.path());
    assert_eq!(app.tab_ui[&tab_id].main_sel, Some(1));
    assert!(app.tab_ui[&tab_id].selected_paths.is_empty());
    assert_eq!(app.active_selected_entry().unwrap().path, expected);
    assert_eq!(std::fs::read(source).unwrap(), b"clipboard fixture");
}

#[test]
fn shift_tab_keeps_commands_and_returns_from_files() {
    let (root, mut app, _) = command_frame_fixture();
    add_file(&mut app, root.path(), "alpha-report.xls");
    open_command_frame(&mut app, "report");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert_eq!(app.command_frame.file_focus, None);

    // Shift+Tab must explicitly select the Commands section, not toggle Files.
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::SHIFT)],
    );
    assert_eq!(app.command_frame.file_focus, None);

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert!(app.command_frame.file_focus.is_some());

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::SHIFT)],
    );
    assert_eq!(app.command_frame.file_focus, None);
}

#[test]
fn typing_a_query_filters_files_and_returns_enter_to_commands() {
    let (root, mut app, source) = command_frame_fixture();
    let report = add_file(&mut app, root.path(), "report.xls");
    add_file(&mut app, root.path(), "notes.txt");
    let tab_id = app.state.active_tab().id;
    {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        ui.main_sel = Some(0);
        ui.selected_paths = [source.clone()].into_iter().collect();
    }

    open_command_frame(&mut app, "");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert!(app.command_frame.file_matches.is_empty());

    run_command_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("report".to_string())],
    );
    assert_eq!(app.command_frame.input, "report");
    assert_eq!(app.command_frame.file_query, "report");
    assert_eq!(match_names(&app), vec!["report.xls"]);
    assert_eq!(app.command_frame.file_focus, None);

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&report.path));

    run_command_frame(&mut app, &ctx, vec![egui::Event::Text(".old".to_string())]);
    assert_eq!(app.command_frame.input, "report.old");
    assert_eq!(app.command_frame.file_query, "report.old");
    assert!(app.command_frame.file_matches.is_empty());
    assert_eq!(app.command_frame.file_focus, None);
    assert_eq!(app.state.active_tab().current_dir, root.path());
    assert_eq!(app.active_selected_entry().unwrap().path, report.path);
    assert!(app.tab_ui[&tab_id].selected_paths.is_empty());
}

#[test]
fn enter_in_commands_runs_search_even_when_a_file_name_matches() {
    let (root, mut app, source) = command_frame_fixture();
    let search_file = add_file(&mut app, root.path(), "search-results.xls");
    let tab_id = app.state.active_tab().id;
    app.tab_ui.get_mut(&tab_id).unwrap().main_sel = Some(0);

    open_command_frame(&mut app, "search");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert_eq!(match_names(&app), vec!["search-results.xls"]);
    assert_eq!(app.command_frame.file_focus, None);

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(!app.command_frame.visible);
    assert_eq!(app.sidebar, SidebarSection::Search);
    assert_eq!(
        app.command_frame.history.last().map(String::as_str),
        Some("search")
    );
    assert_eq!(app.state.active_tab().current_dir, root.path());
    assert_eq!(app.active_selected_entry().unwrap().path, search_file.path);
    assert_eq!(app.preview.path, None);
    assert_eq!(
        std::fs::read(search_file.path).unwrap(),
        b"command frame fixture"
    );
    assert_eq!(std::fs::read(source).unwrap(), b"clipboard fixture");
}

#[test]
fn enter_on_focused_directory_navigates_into_it() {
    let (root, mut app, _) = command_frame_fixture();
    let nested = add_directory(&mut app, root.path(), "nested");
    add_file(&mut app, root.path(), "notes.txt");

    open_command_frame(&mut app, "nested");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert_eq!(match_names(&app), vec!["nested"]);

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&nested.path));

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(app.state.active_tab().current_dir, nested.path);
    assert!(!app.command_frame.visible);
    assert!(nested.path.is_dir());
}

#[test]
fn escape_closes_via_render_and_keyboard_without_changing_file_selection() {
    for via_handle_keyboard in [false, true] {
        let (root, mut app, source) = command_frame_fixture();
        let matched = add_file(&mut app, root.path(), "matched.txt");
        let tab_id = app.state.active_tab().id;
        {
            let ui = app.tab_ui.get_mut(&tab_id).unwrap();
            ui.main_sel = Some(0);
            ui.selected_paths = [source.clone()].into_iter().collect();
        }
        let directory_before = app.state.active_tab().current_dir.clone();
        let preview_before = app.preview.path.clone();

        open_command_frame(&mut app, "matched");
        let ctx = Context::default();
        prime_command_input(&mut app, &ctx);
        run_command_frame(
            &mut app,
            &ctx,
            vec![key_event(Key::Tab, egui::Modifiers::NONE)],
        );
        assert!(app.command_frame.file_focus.is_some());

        if via_handle_keyboard {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![key_event(Key::Escape, egui::Modifiers::NONE)],
                    ..Default::default()
                },
                |ctx| app.handle_keyboard(ctx),
            );
        } else {
            run_command_frame(
                &mut app,
                &ctx,
                vec![key_event(Key::Escape, egui::Modifiers::NONE)],
            );
        }

        assert!(!app.command_frame.visible);
        assert_eq!(app.command_frame.file_focus, None);
        assert!(app.command_frame.input.is_empty());
        assert_eq!(app.state.active_tab().current_dir, directory_before);
        assert_eq!(app.active_selected_entry().unwrap().path, matched.path);
        assert!(app.tab_ui[&tab_id].selected_paths.is_empty());
        assert_eq!(app.preview.path, preview_before);
        assert!(!app.preview.visible);
        assert_eq!(std::fs::read(source).unwrap(), b"clipboard fixture");
    }
}

#[test]
fn removing_the_focused_file_clears_focus_without_selecting_another_match() {
    let (root, mut app, _) = command_frame_fixture();
    let first = add_file(&mut app, root.path(), "report-alpha.xls");
    let removed = add_file(&mut app, root.path(), "report-beta.xls");

    open_command_frame(&mut app, "report");
    app.update_command_file_matches();
    assert_eq!(
        match_names(&app),
        vec!["report-alpha.xls", "report-beta.xls"]
    );
    app.command_frame.file_focus = Some(removed.path.clone());

    let tab_id = app.state.active_tab().id;
    app.tab_ui
        .get_mut(&tab_id)
        .unwrap()
        .main
        .entries
        .retain(|entry| entry.path != removed.path);
    app.update_command_file_matches();

    assert_eq!(match_names(&app), vec!["report-alpha.xls"]);
    assert_eq!(app.command_frame.file_focus, None);
    assert_ne!(app.command_frame.file_focus.as_ref(), Some(&first.path));
}

#[test]
fn arrows_preserve_command_history_and_move_files_when_focused() {
    let (root, mut app, _) = command_frame_fixture();
    add_file(&mut app, root.path(), "report-a.txt");
    add_file(&mut app, root.path(), "report-b.txt");
    let history = vec!["first".to_string(), "second".to_string()];
    app.command_frame.history = history.clone();

    open_command_frame(&mut app, "");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowUp, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.input, "second");
    assert_eq!(app.command_frame.history_idx, Some(1));

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowUp, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.input, "first");
    assert_eq!(app.command_frame.history_idx, Some(0));

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.input, "second");
    assert_eq!(app.command_frame.history_idx, Some(1));

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert!(app.command_frame.input.is_empty());
    assert_eq!(app.command_frame.history_idx, None);
    assert_eq!(app.command_frame.history, history);

    app.command_frame.input = "report".to_string();
    run_command_frame(&mut app, &ctx, Vec::new());
    assert_eq!(match_names(&app), vec!["report-a.txt", "report-b.txt"]);
    let first_path = app.command_frame.file_matches[0].path.clone();
    let second_path = app.command_frame.file_matches[1].path.clone();

    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&first_path));
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&second_path));
    assert_eq!(app.command_frame.input, "report");
    assert_eq!(app.command_frame.history_idx, None);
    assert_eq!(app.command_frame.history, history);
}

#[test]
fn matching_and_moving_file_focus_selects_without_navigating_or_opening_inspector() {
    let (root, mut app, source) = command_frame_fixture();
    add_file(&mut app, root.path(), "report-a.txt");
    add_file(&mut app, root.path(), "report-b.txt");
    let tab_id = app.state.active_tab().id;
    let child_path = root.path().join("child-folder").join("child.txt");
    {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        ui.main_sel = Some(1);
        ui.selected_paths = [source.clone()].into_iter().collect();
        ui.right.dir = child_path.parent().unwrap().to_path_buf();
        let mut child = ui.main.entries[0].clone();
        child.name = "child.txt".to_string();
        child.path = child_path;
        child.kind = EntryKind::File;
        ui.right.entries = vec![child];
    }
    let directory_before = app.state.active_tab().current_dir.clone();
    let children_before = app.tab_ui[&tab_id].right.entries.clone();

    open_command_frame(&mut app, "report");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );

    assert_eq!(
        app.command_frame.file_focus.as_ref(),
        Some(&app.command_frame.file_matches[1].path)
    );
    assert_eq!(app.state.active_tab().current_dir, directory_before);
    assert_eq!(
        app.active_selected_entry().unwrap().path,
        app.command_frame.file_matches[1].path
    );
    assert!(app.tab_ui[&tab_id].selected_paths.is_empty());
    assert_eq!(app.tab_ui[&tab_id].right.entries, children_before);
    assert_eq!(app.preview.path, None);
    assert!(!app.preview.visible);
}

#[test]
fn miller_focus_folder_matches_only_the_active_tabs_focused_listing() {
    let (root, mut app, _) = command_frame_fixture();
    app.state.active_tab_mut().view_mode = ViewMode::Miller;
    let tab_id = app.state.active_tab().id;
    add_file(&mut app, root.path(), "main-target.xls");

    let focus_dir = add_directory(&mut app, root.path(), "focused").path;
    let mut focused = app.tab_ui[&tab_id].main.entries[0].clone();
    focused.name = "focused-target.xls".to_string();
    focused.path = focus_dir.join("focused-target.xls");
    std::fs::write(&focused.path, b"focused").unwrap();
    {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        ui.selection_memory
            .insert(root.path().to_path_buf(), focus_dir.clone());
        ui.main_sel = ui
            .main
            .entries
            .iter()
            .position(|entry| entry.path == focus_dir);
        ui.miller_focus_dir = Some(focus_dir.clone());
        ui.miller_cache
            .insert(focus_dir.clone(), Ok(vec![focused.clone()]));
    }

    let other_dir = root.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let other_idx = app.state.new_tab(other_dir.clone());
    let other_id = app.state.tabs[other_idx].id;
    let mut other = focused.clone();
    other.name = "other-target.xls".to_string();
    other.path = other_dir.join("other-target.xls");
    std::fs::write(&other.path, b"other").unwrap();
    {
        let ui = app.tab_ui.entry(other_id).or_default();
        ui.main.dir = other_dir;
        ui.main.entries = vec![other];
    }
    assert_eq!(app.state.active_tab().id, tab_id);

    open_command_frame(&mut app, "target");
    app.update_command_file_matches();
    assert_eq!(
        app.command_frame.file_directory.as_deref(),
        Some(focus_dir.as_path())
    );
    assert_eq!(match_names(&app), vec!["focused-target.xls"]);
    assert_eq!(app.command_frame.file_matches[0].path, focused.path);
    app.sync_command_file_selection();
    assert_eq!(app.active_selected_entry().unwrap().path, focused.path);
}

#[test]
fn whitespace_edit_returns_enter_to_commands_even_if_matches_are_unchanged() {
    let (root, mut app, _) = command_frame_fixture();
    add_directory(&mut app, root.path(), "nested");
    open_command_frame(&mut app, "nested");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert!(app.command_frame.file_focus.is_some());
    run_command_frame(&mut app, &ctx, vec![egui::Event::Text(" ".into())]);
    assert_eq!(app.command_frame.input, "nested ");
    assert_eq!(app.command_frame.file_focus, None);
    assert_eq!(match_names(&app), ["nested"]);
}

#[test]
fn always_visible_command_field_releases_focus_after_success_and_escape() {
    for key in [Key::Enter, Key::Escape] {
        let (_root, mut app, _) = command_frame_fixture();
        app.state.config.command_frame_always_visible = true;
        open_command_frame(&mut app, "search");
        let ctx = Context::default();
        prime_command_input(&mut app, &ctx);
        run_command_frame(&mut app, &ctx, vec![key_event(key, egui::Modifiers::NONE)]);
        assert!(!app.command_frame.visible);
        assert!(!ctx.memory(|m| m.has_focus(egui::Id::new("cmd_input_field"))));
        run_command_frame(&mut app, &ctx, vec![]);
        assert!(!app.command_frame.visible);
    }
}

#[test]
fn command_file_results_are_painted_inside_the_panel_not_below_its_clip() {
    let (root, mut app, _) = command_frame_fixture();
    add_file(&mut app, root.path(), "search-results.xls");
    open_command_frame(&mut app, "search");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(960.0, 640.0),
            )),
            ..Default::default()
        },
        |ctx| app.render_command_frame(ctx),
    );
    for label in ["Files in this folder", "search-results.xls"] {
        assert!(
            output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().contains(label) => {
                    let rect = Rect::from_min_size(text.pos, text.galley.size());
                    clipped.clip_rect.contains_rect(rect) && rect.bottom() <= 640.0
                }
                _ => false,
            }),
            "{label} must be visible inside the panel"
        );
    }
}

#[test]
fn down_enters_matches_and_space_previews_the_real_selection_without_editing_query() {
    let (root, mut app, _) = command_frame_fixture();
    let first = add_file(&mut app, root.path(), "sprout notes.txt");
    let second = add_file(&mut app, root.path(), "sprout other.txt");
    open_command_frame(&mut app, "sprout");
    let ctx = Context::default();
    prime_command_input(&mut app, &ctx);
    assert_eq!(app.active_selected_entry().unwrap().path, first.path);
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus, Some(first.path));
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus, Some(second.path.clone()));
    assert_eq!(app.active_selected_entry().unwrap().path, second.path);
    for visible in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    key_event(Key::Space, egui::Modifiers::NONE),
                    egui::Event::Text(" ".into()),
                ],
                ..Default::default()
            },
            |ctx| {
                app.handle_preview_toggle(ctx);
                app.render_command_frame(ctx);
            },
        );
        assert_eq!(app.preview.visible, visible);
        assert_eq!(app.command_frame.input, "sprout");
        if visible {
            assert_eq!(app.preview.path.as_ref(), Some(&second.path));
        }
    }
    assert_eq!(app.active_selected_entry().unwrap().path, second.path);
}

fn search_item(entry: &FileEntry) -> SearchResultItem {
    SearchResultItem {
        path: entry.path.clone(),
        parent_path: entry.path.parent().unwrap().to_path_buf(),
        name: entry.name.clone(),
        kind: entry.kind,
        is_executable: false,
        symlink_target_is_dir: None,
        size_bytes: entry.size_bytes,
        modified_unix_secs: None,
        content_snippet: None,
        name_lower: entry.name.to_lowercase(),
        path_str: entry.path.display().to_string(),
        path_lower: entry.path.display().to_string().to_lowercase(),
    }
}

fn search_frame(app: &mut OttrinApp, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(960.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            app.handle_preview_toggle(ctx);
            app.handle_keyboard(ctx);
            egui::CentralPanel::default().show(ctx, |ui| app.render_search_sidebar(ui));
        },
    )
}

#[test]
fn search_down_releases_query_and_space_toggle_cannot_append_native_text() {
    let (root, mut app, source) = command_frame_fixture();
    let entry = add_file(&mut app, root.path(), "sprout.txt");
    app.sidebar = SidebarSection::Search;
    app.search_ui.query = "sprout".into();
    app.search_ui.last_executed_query = "sprout".into();
    app.search_ui.results = vec![search_item(&entry)];
    app.search_ui.selected = Some(0);
    app.search_ui.request_focus = true;
    let tab_id = app.state.active_tab().id;
    app.tab_ui.get_mut(&tab_id).unwrap().main_sel = Some(0);
    let ctx = Context::default();
    search_frame(&mut app, &ctx, vec![]);
    search_frame(&mut app, &ctx, vec![]);
    assert!(app.search_ui.input_focused);
    search_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::ArrowDown, egui::Modifiers::NONE)],
    );
    assert!(!app.search_ui.input_focused);
    assert_eq!(app.search_ui.selected, Some(0));
    for visible in [true, false] {
        search_frame(
            &mut app,
            &ctx,
            vec![
                key_event(Key::Space, egui::Modifiers::NONE),
                egui::Event::Text(" ".into()),
            ],
        );
        assert_eq!(app.preview.visible, visible);
        assert_eq!(app.search_ui.query, "sprout");
        assert!(!app.search_ui.input_focused);
        assert_eq!(app.state.active_tab().current_dir, root.path());
        assert_eq!(app.active_selected_entry().unwrap().path, source);
    }
    // Native clipboard shortcuts target the result, not the main list.
    search_frame(&mut app, &ctx, vec![egui::Event::Copy]);
    assert_eq!(app.clipboard.as_ref().unwrap().sources, vec![entry.path]);
    for key in [Key::ArrowLeft, Key::ArrowRight, Key::Backspace] {
        search_frame(&mut app, &ctx, vec![key_event(key, egui::Modifiers::NONE)]);
        assert_eq!(app.state.active_tab().current_dir, root.path());
        assert_eq!(app.active_selected_entry().unwrap().path, source);
    }
    app.tab_ui
        .get_mut(&tab_id)
        .unwrap()
        .selected_paths
        .insert(source.clone());
    search_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Escape, egui::Modifiers::NONE)],
    );
    assert_eq!(app.sidebar, SidebarSection::None);
    assert!(app.tab_ui[&tab_id].selected_paths.contains(&source));
}

#[test]
fn background_refresh_preserves_latest_selection_by_path_and_does_not_scroll() {
    let (root, mut app, _) = command_frame_fixture();
    let a = search_item(&add_file(&mut app, root.path(), "a.txt"));
    let b = search_item(&add_file(&mut app, root.path(), "b.txt"));
    app.search_ui.query = "txt".into();
    app.search_ui.results = vec![a.clone(), b.clone()];
    app.search_ui.selected = Some(1);
    for (items, selected) in [(vec![b.clone(), a], Some(0)), (vec![], None)] {
        app.search_ui.search_inflight_background = true;
        app.search_ui.search_inflight = true;
        app.search_ui.latest_request_id += 1;
        app.search_result_tx
            .send(SearchQueryResponse {
                request_id: app.search_ui.latest_request_id,
                query: SearchQuery {
                    text: "txt".into(),
                    scope: app.search_ui.scope,
                    sort: app.search_ui.sort,
                    include_hidden_system: app.search_ui.include_hidden,
                    content_search: false,
                    root_path: None,
                    limit: 10,
                    offset: 0,
                },
                root: None,
                result: SearchQueryResult {
                    response: ottrin_core::SearchResponse {
                        total: items.len(),
                        items,
                        status: SearchIndexStatus::Ready,
                        error: None,
                    },
                    warning: None,
                },
            })
            .unwrap();
        app.poll_search_results();
        assert_eq!(app.search_ui.selected, selected);
        assert!(!app.search_ui.scroll_to_selected);
        assert!(!app.search_ui.search_inflight);
    }
}

#[test]
fn search_native_shift_delete_targets_only_the_result_without_cutting() {
    let (root, mut app, source) = command_frame_fixture();
    let entry = add_file(&mut app, root.path(), "sprout.txt");
    let tab_id = app.state.active_tab().id;
    app.tab_ui.get_mut(&tab_id).unwrap().main_sel = Some(0);
    app.sidebar = SidebarSection::Search;
    app.search_ui.results = vec![search_item(&entry)];
    app.search_ui.selected = Some(0);
    app.state.config.confirm_permanent_delete = true;
    let ctx = Context::default();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![
                egui::Event::Cut,
                key_event(Key::Delete, egui::Modifiers::SHIFT),
            ],
            ..Default::default()
        },
        |ctx| app.handle_keyboard(ctx),
    );
    assert!(app.clipboard.is_none());
    assert_eq!(
        app.pending_delete.as_ref().unwrap().targets,
        vec![entry.path.clone()]
    );
    assert_eq!(app.active_selected_entry().unwrap().path, source);
    assert!(entry.path.exists());
}

#[test]
fn search_pointer_double_click_opens_the_directory_result() {
    let (root, mut app, _) = command_frame_fixture();
    let entry = add_directory(&mut app, root.path(), "sprout-directory");
    app.sidebar = SidebarSection::Search;
    app.search_ui.query = "sprout".into();
    app.search_ui.last_executed_query = "sprout".into();
    app.search_ui.results = vec![search_item(&entry)];
    app.search_ui.total = 1;
    app.search_ui.selected = Some(0);
    let ctx = Context::default();
    search_frame(&mut app, &ctx, vec![]);
    let output = search_frame(&mut app, &ctx, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == entry.name => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("rendered result name");
    for _ in 0..2 {
        for pressed in [true, false] {
            search_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    assert_eq!(app.state.active_tab().current_dir, entry.path);
    assert_eq!(app.sidebar, SidebarSection::None);
}

#[test]
fn search_pointer_double_click_on_metadata_cell_opens_the_directory_result() {
    // QA double-clicks the accessibility union center of the result row. At
    // the real narrow search panel width the columns shrink and that center
    // lands on the Folder cell, not the Name cell, so activate it there.
    let (root, mut app, _) = command_frame_fixture();
    let entry = add_directory(&mut app, root.path(), "sprout-directory");
    app.sidebar = SidebarSection::Search;
    app.search_ui.query = "sprout".into();
    app.search_ui.last_executed_query = "sprout".into();
    let mut item = search_item(&entry);
    item.parent_path = PathBuf::from(format!("/{}", "very-long-parent-folder/".repeat(12)));
    app.search_ui.results = vec![item];
    app.search_ui.total = 1;
    app.search_ui.selected = Some(0);
    let ctx = Context::default();
    ctx.enable_accesskit();
    let narrow_frame = |app: &mut OttrinApp, ctx: &Context, events: Vec<egui::Event>| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(960.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                app.handle_preview_toggle(ctx);
                app.handle_keyboard(ctx);
                egui::SidePanel::right("qa_narrow_search_panel")
                    .exact_width(410.0)
                    .resizable(false)
                    .show(ctx, |ui| app.render_search_sidebar(ui));
            },
        )
    };
    narrow_frame(&mut app, &ctx, vec![]);
    let output = narrow_frame(&mut app, &ctx, vec![]);
    let label = format!("Search result: {}", entry.name);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(label.as_str()))
        .and_then(|(_, node)| node.bounds())
        .expect("accessible result row");
    assert!(
        bounds.y1 - bounds.y0 <= 24.5,
        "accessible row must fit the actual 24px hit area: {bounds:?}"
    );
    assert!(
        bounds.x1 <= 960.0,
        "accessible row must stay inside the window: {bounds:?}"
    );
    let pos = egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    for _ in 0..2 {
        for pressed in [true, false] {
            narrow_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    assert_eq!(app.state.active_tab().current_dir, entry.path);
    assert_eq!(app.sidebar, SidebarSection::None);
}

#[test]
fn search_content_snippet_has_room_inside_the_accessible_row() {
    let (root, mut app, _) = command_frame_fixture();
    let entry = add_file(&mut app, root.path(), "sprout.txt");
    let mut item = search_item(&entry);
    item.content_snippet = Some("matched content".into());
    app.sidebar = SidebarSection::Search;
    app.search_ui.query = "sprout".into();
    app.search_ui.last_executed_query = "sprout".into();
    app.search_ui.results = vec![item];
    app.search_ui.total = 1;
    let ctx = Context::default();
    ctx.enable_accesskit();
    search_frame(&mut app, &ctx, vec![]);
    let output = search_frame(&mut app, &ctx, vec![]);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Search result: sprout.txt"))
        .and_then(|(_, node)| node.bounds())
        .unwrap();
    assert!(bounds.y1 - bounds.y0 <= 40.5);
    let snippet = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == "matched content" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .expect("painted content snippet");
    assert!(snippet.top() >= bounds.y0 as f32 && snippet.bottom() <= bounds.y1 as f32);
}

#[test]
fn miller_navigation_keeps_command_scope_on_the_browsed_folder_while_focus_borrows_the_parent_row()
{
    // Regression for the failed command-down QA run: after address navigation
    // into Documents, the Documents column has no active row yet, so keyboard
    // focus borrows the Documents path row in the parent column. That borrow
    // is display-only; it must not persist miller_focus_dir as the ancestor,
    // or "Files in this folder" scopes to the ancestor and sample.json never
    // matches.
    let (root, mut app, _) = command_frame_fixture();
    let home = root.path().join("home");
    let documents = home.join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    std::fs::write(documents.join("sample.json"), b"{}").unwrap();
    let tab_id = app.state.active_tab().id;
    app.state.active_tab_mut().view_mode = ViewMode::Miller;
    app.state.active_tab_mut().current_dir = documents.clone();
    let sample = {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        let mut entry = ui.main.entries.first().cloned().expect("fixture entry");
        entry.name = "sample.json".to_string();
        entry.path = documents.join("sample.json");
        entry.kind = EntryKind::File;
        entry.is_executable = false;
        entry.symlink_target_is_dir = None;
        entry.size_bytes = Some(2);
        entry.modified_unix_secs = None;
        entry.created_unix_secs = None;
        // State left by navigate_tab_to -> refresh_tab: focus is the
        // Documents column and the listing is loaded without an active row.
        ui.main.dir = documents.clone();
        ui.main.entries = vec![entry.clone()];
        ui.main_sel = None;
        ui.miller_focus_dir = Some(documents.clone());
        ui.miller_cache.clear();
        // refresh_tab seeds ancestry memory so the parent's path row exists.
        ui.selection_memory.insert(home.clone(), documents.clone());
        entry
    };

    let ctx = Context::default();
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(960.0, 640.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.render_miller(ui, tab_id));
        },
    );
    // The borrow still displays the Documents path row in the parent column.
    {
        let (cols, current_col) = app.build_miller_model(tab_id);
        let focus_col = app.effective_miller_focus_col(tab_id, &cols, current_col);
        assert_eq!(
            cols[focus_col].dir, home,
            "focus borrows the parent path row"
        );
    }
    // But the borrow must not persist as Miller focus.
    assert_eq!(
        app.tab_ui[&tab_id].miller_focus_dir.as_ref(),
        Some(&documents),
        "command scope stays on the browsed folder"
    );

    open_command_frame(&mut app, "sample");
    prime_command_input(&mut app, &ctx);
    assert_eq!(match_names(&app), vec!["sample.json"]);
    assert_eq!(app.command_frame.file_directory.as_ref(), Some(&documents));
    run_command_frame(
        &mut app,
        &ctx,
        vec![key_event(Key::Tab, egui::Modifiers::NONE)],
    );
    assert_eq!(app.command_frame.file_focus.as_ref(), Some(&sample.path));
    // The reveal selects the result in the real Documents listing.
    assert_eq!(app.tab_ui[&tab_id].main_sel, Some(0));
    assert!(app.tab_ui[&tab_id].selected_paths.is_empty());
}

#[test]
fn search_reveal_retains_query_results_and_selection_across_back_and_reopen() {
    // Verified semantics: reveal_search_result navigates to the containing
    // folder, hands the result to the folder's one-shot pending selection, and
    // hides the Search sidebar. It never clears the search session. History
    // Back is directory-only and does not reopen Search; reopening the
    // sidebar shows the retained query, results, and selection without
    // re-executing.
    let (root, mut app, _) = command_frame_fixture();
    let documents = root.path().join("Documents");
    std::fs::create_dir_all(&documents).unwrap();
    let tab_id = app.state.active_tab().id;
    let entry = {
        let ui = app.tab_ui.get_mut(&tab_id).unwrap();
        let mut file = ui.main.entries.first().cloned().expect("fixture entry");
        file.name = "sample.json".to_string();
        file.path = documents.join("sample.json");
        file.size_bytes = Some(13);
        file
    };
    let item = search_item(&entry);
    app.sidebar = SidebarSection::Search;
    app.search_ui.query = "sample".into();
    app.search_ui.last_executed_query = "sample".into();
    app.search_ui.results = vec![item.clone()];
    app.search_ui.total = 1;
    app.search_ui.selected = Some(0);

    // Reveal the result in its containing folder.
    app.reveal_search_result(documents.clone(), entry.path.clone());
    assert_eq!(app.state.active_tab().current_dir, documents);
    assert_eq!(app.sidebar, SidebarSection::None);
    assert!(!app.search_ui.input_focused);
    assert_eq!(
        app.tab_ui[&tab_id].pending_select_path.as_ref(),
        Some(&entry.path),
        "containing-folder reveal selects the revealed result"
    );
    // Revealing hides only the sidebar; the search session survives untouched.
    assert_eq!(app.search_ui.query, "sample");
    assert_eq!(app.search_ui.results, vec![item.clone()]);
    assert_eq!(app.search_ui.selected, Some(0));
    assert_eq!(app.search_ui.last_executed_query, "sample");

    // The pending selection resolves to the sample.json row when the
    // Documents listing applies.
    app.tab_ui.get_mut(&tab_id).unwrap().main.dir = documents.clone();
    let idx = app
        .resolve_miller_selection(tab_id, &documents, std::slice::from_ref(&entry), true)
        .expect("reveal selects the result row");
    assert_eq!(idx, 0);
    assert_eq!(app.tab_ui[&tab_id].main_sel, Some(0));

    // History Back restores the directory, not the search view.
    assert!(app.navigate_tab_back(tab_id));
    assert_eq!(app.state.active_tab().current_dir, root.path());
    assert_eq!(app.sidebar, SidebarSection::None);
    assert_eq!(app.search_ui.query, "sample");
    assert_eq!(app.search_ui.results, vec![item.clone()]);
    assert_eq!(app.search_ui.selected, Some(0));

    // Reopening Search shows the retained session without re-executing.
    app.sidebar = SidebarSection::Search;
    let ctx = Context::default();
    search_frame(&mut app, &ctx, vec![]);
    assert_eq!(app.search_ui.query, "sample");
    assert_eq!(app.search_ui.results, vec![item]);
    assert_eq!(app.search_ui.selected, Some(0));
    assert!(!app.search_ui.search_inflight, "no re-execution on reopen");
}
