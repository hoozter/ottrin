use super::*;

#[test]
fn miller_parent_and_child_sort_independently_and_survive_navigation() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("Downloads");
    std::fs::create_dir(&child).unwrap();
    for directory in [root.path(), child.as_path()] {
        std::fs::write(directory.join("a-small"), "a").unwrap();
        std::fs::write(directory.join("z-large"), "zzzz").unwrap();
    }
    let config = AppConfig {
        startup_folder: Some(child.clone()),
        ..Default::default()
    };
    let mut app =
        OttrinApp::new_with_config_and_graphics_status(config, GraphicsRuntimeStatus::default());
    let id = app.state.active_tab().id;
    let size = SortConfig {
        by: SortBy::Size,
        ascending: false,
        folders_first: false,
    };
    app.apply_folder_sort(&child, Some(size));
    let (columns, _) = app.build_miller_model(id);
    let parent = columns.iter().find(|c| c.dir == root.path()).unwrap();
    let downloads = columns.iter().find(|c| c.dir == child).unwrap();
    assert_eq!(downloads.entries[0].name, "z-large");
    assert_eq!(parent.entries[0].name, "Downloads");
    app.navigate_tab_to(id, root.path().to_path_buf());
    app.navigate_tab_to(id, child.clone());
    assert_eq!(app.state.config.sort_for_folder(&child), size);
    let config: AppConfig =
        serde_json::from_str(&serde_json::to_string(&app.state.config).unwrap()).unwrap();
    assert_eq!(config.sort_for_folder(&child), size);
    assert_eq!(config.sort_for_folder(root.path()), SortConfig::default());
    app.apply_folder_sort(&child, None);
    assert!(!app.state.config.has_folder_sort(&child));
    assert_eq!(
        app.state.config.sort_for_folder(&child),
        app.state.config.default_sort
    );
}

#[test]
fn folder_header_click_changes_only_that_folder_and_preserves_selection() {
    let (root, mut app, _) = super::super::tests::clipboard_keyboard_fixture();
    app.state.config.miller_columns = vec![SortBy::Modified];
    let path = root.path();
    let ctx = Context::default();
    let draw = |app: &mut OttrinApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(600.0, 80.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| app.render_folder_sort_header(ui, path));
            },
        )
    };
    let _ = draw(&mut app, Vec::new());
    let output = draw(&mut app, Vec::new());
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Modified" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()).center())
            }
            _ => None,
        })
        .expect("Modified heading");
    for pressed in [true, false] {
        let _ = draw(
            &mut app,
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
    assert_eq!(app.state.config.sort_for_folder(path).by, SortBy::Modified);
    assert!(!app.state.config.sort_for_folder(path).ascending);
    assert_eq!(
        app.state.config.sort_for_folder(&path.join("untouched")),
        SortConfig::default()
    );
    app.change_folder_sort_heading(path, SortBy::Modified);
    assert!(app.state.config.sort_for_folder(path).ascending);
}

#[test]
fn explicit_startup_and_new_tab_action_use_requested_folder() {
    let root = tempfile::tempdir().unwrap();
    let alternate = root.path().join("work");
    std::fs::create_dir(&alternate).unwrap();
    let config = AppConfig {
        startup_folder: Some(root.path().to_path_buf()),
        restore_tabs: true,
        saved_tabs: vec![ottrin_core::TabState::new(8, root.path().to_path_buf())],
        ..Default::default()
    };
    let mut app =
        OttrinApp::new_with_config_and_graphics_status(config, GraphicsRuntimeStatus::default());
    app.open_directory_at_startup(alternate.clone());
    assert_eq!(app.state.active_tab().current_dir, alternate);
    app.open_folder_in_new(root.path());
    assert_eq!(app.state.tabs.len(), 2);
    assert_eq!(app.state.active_tab().current_dir, root.path());
    assert_eq!(app.state.config.default_view_mode, ViewMode::Miller);
    app.set_tab_view_mode(app.state.active_tab().id, ViewMode::Grid);
    assert_eq!(app.state.config.default_view_mode, ViewMode::Miller);
}

#[test]
fn search_order_is_saved_independently_of_folder_order() {
    let (_root, mut app, _) = super::super::tests::clipboard_keyboard_fixture();
    let original = app.state.config.default_sort;
    app.set_search_sort(SearchSort::NameDescending);
    assert_eq!(
        app.state.config.search.default_sort,
        SearchSort::NameDescending
    );
    assert_eq!(app.state.config.default_sort, original);
    let config: AppConfig =
        serde_json::from_str(&serde_json::to_string(&app.state.config).unwrap()).unwrap();
    let reopened =
        OttrinApp::new_with_config_and_graphics_status(config, GraphicsRuntimeStatus::default());
    assert_eq!(reopened.search_ui.sort, SearchSort::NameDescending);
}

#[test]
fn name_only_header_toggles_name_sort_even_when_hidden_metadata_is_sorted() {
    let (root, mut app, _) = super::super::tests::clipboard_keyboard_fixture();
    let path = root.path();
    app.apply_folder_sort(
        path,
        Some(SortConfig {
            by: SortBy::Created,
            ascending: false,
            folders_first: true,
        }),
    );
    let ctx = Context::default();
    install_application_fonts(&ctx);
    let draw = |app: &mut OttrinApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(200.0, 80.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.render_folder_sort_header(ui, path);
                });
            },
        )
    };
    let _ = draw(&mut app, Vec::new());
    let output = draw(&mut app, Vec::new());
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&"Name"));
    assert!(!labels.contains(&"Created"));
    assert!(!labels.contains(&"Modified"));
    let point = egui::pos2(40.0, 18.0);
    for ascending in [true, false] {
        for pressed in [true, false] {
            let _ = draw(
                &mut app,
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
        assert_eq!(app.state.config.sort_for_folder(path).by, SortBy::Name);
        assert_eq!(app.state.config.sort_for_folder(path).ascending, ascending);
    }
}

#[test]
fn restore_column_defaults_leaves_sorting_and_other_views_alone() {
    let (_root, mut app, _) = super::super::tests::clipboard_keyboard_fixture();
    app.state.config.miller_columns = vec![SortBy::Size, SortBy::Created];
    let original_sort = app.state.config.default_sort;
    let original_list = app.state.config.list_columns.clone();
    let ctx = Context::default();
    install_application_fonts(&ctx);
    let draw = |app: &mut OttrinApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(400.0, 400.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.render_miller_columns_menu(ui));
            },
        )
    };
    let _ = draw(&mut app, Vec::new());
    let output = draw(&mut app, Vec::new());
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Restore defaults" => {
                Some(Rect::from_min_size(text.pos, text.galley.size()).center())
            }
            _ => None,
        })
        .expect("reset action");
    for pressed in [true, false] {
        let _ = draw(
            &mut app,
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
    assert!(app.state.config.miller_columns.is_empty());
    assert_eq!(app.state.config.default_sort, original_sort);
    assert_eq!(app.state.config.list_columns, original_list);
}
