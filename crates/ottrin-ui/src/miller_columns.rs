//! Column view headers and row cells share one layout.
use super::*;

pub(super) const METADATA: [(SortBy, &str, &str, f32); 4] = [
    (SortBy::Modified, "Modified", MI_HISTORY, 132.0),
    (SortBy::Kind, "Type", MI_DESCRIPTION, 84.0),
    (SortBy::Size, "Size", MI_STORAGE, 76.0),
    (SortBy::Created, "Created", MI_HISTORY, 132.0),
];

pub(super) fn header_height(c: &Colors) -> f32 {
    19.0 * c.font_scale
}

/// Live columns and Appearance use the same compact, file-view surface.
pub(super) fn allocate_header(ui: &mut egui::Ui, c: &Colors) -> Rect {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), header_height(c)),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, c.bg);
    rect
}

pub(super) fn paint_heading(
    ui: &egui::Ui,
    cell: Rect,
    label: &str,
    direction: Option<bool>,
    c: &Colors,
) {
    paint_sort_header(
        ui,
        cell.shrink2(Vec2::new(6.0, 0.0)),
        label,
        direction,
        FontId::proportional(10.0 * c.font_scale),
        if direction.is_some() {
            c.text_dim
        } else {
            c.text_muted
        },
        false,
    );
}

pub(super) fn metadata_width(config: &AppConfig, scale: f32, c: &Colors) -> f32 {
    METADATA
        .iter()
        .filter(|(by, ..)| config.miller_columns.contains(by))
        .map(|(.., width)| width * scale.sqrt() * c.font_scale)
        .sum()
}

pub(super) fn cells(
    rect: Rect,
    config: &AppConfig,
    scale: f32,
    c: &Colors,
) -> Vec<(SortBy, &'static str, Rect)> {
    let name_width = (rect.width() - metadata_width(config, scale, c)).max(1.0);
    let mut cells = vec![(
        SortBy::Name,
        "Name",
        Rect::from_min_size(rect.min, Vec2::new(name_width, rect.height())),
    )];
    let mut x = rect.left() + name_width;
    for (by, label, _, width) in METADATA {
        if config.miller_columns.contains(&by) {
            let width = width * scale.sqrt() * c.font_scale;
            cells.push((
                by,
                label,
                Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, rect.height())),
            ));
            x += width;
        }
    }
    cells
}

pub(super) fn render_row(
    ui: &mut egui::Ui,
    entry: &FileEntry,
    state: EntryVisualState,
    layout: file_row::FileRowLayout,
    c: &Colors,
    config: &AppConfig,
) -> egui::Response {
    let name_width = (layout.width - metadata_width(config, layout.scale, c)).max(1.0);
    let response =
        file_row::render_file_row_with_name_width(ui, entry, state, layout, name_width, c, config);
    for (by, _, cell) in cells(response.rect, config, layout.scale, c)
        .into_iter()
        .skip(1)
    {
        let text = match by {
            SortBy::Modified => entry
                .modified_unix_secs
                .map(format_modified)
                .unwrap_or_default(),
            SortBy::Created => entry
                .created_unix_secs
                .map(format_modified)
                .unwrap_or_default(),
            SortBy::Kind => entry_kind_label(entry.kind).to_owned(),
            SortBy::Size if entry.kind == EntryKind::Directory => "—".to_owned(),
            SortBy::Size => entry.size_bytes.map(format_size).unwrap_or_default(),
            SortBy::Name => unreachable!(),
        };
        let cell = cell.shrink2(Vec2::new(6.0, 0.0));
        let galley = ui.painter().layout_job(widget_style::single_line_text_job(
            &text,
            FontId::proportional((12.0 * layout.scale.sqrt()).max(10.0) * c.font_scale),
            c.text_dim,
            cell.width().max(1.0),
        ));
        ui.painter_at(cell.intersect(ui.clip_rect())).galley(
            egui::pos2(cell.left(), cell.center().y - galley.size().y / 2.0),
            galley,
            c.text_dim,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_view_defaults_and_metadata_round_trip_independently_of_list_view() {
        let mut config = AppConfig::default();
        let list_columns = config.list_columns.clone();
        let c = Colors::preset(ottrin_core::ThemePreset::Ottrin, true);
        let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(240.0, 25.0));
        assert_eq!(cells(rect, &config, 1.0, &c).len(), 1);
        assert_eq!(
            cells(rect, &config, 1.0, &c)[0],
            (SortBy::Name, "Name", rect)
        );
        let mut json = serde_json::to_value(&config).unwrap();
        json.as_object_mut().unwrap().remove("miller_columns");
        assert!(
            serde_json::from_value::<AppConfig>(json)
                .unwrap()
                .miller_columns
                .is_empty()
        );
        config.miller_columns = vec![SortBy::Created, SortBy::Size];
        let restored: AppConfig =
            serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
        assert_eq!(restored.miller_columns, config.miller_columns);
        assert_eq!(restored.list_columns, list_columns);
    }

    #[test]
    fn metadata_layout_preserves_name_width_and_aligns_cells_at_all_scales() {
        for dark in [true, false] {
            for scale in [0.9_f32, 1.0, 1.35] {
                let mut c = Colors::preset(ottrin_core::ThemePreset::Ottrin, dark);
                c.font_scale = scale;
                let config = AppConfig {
                    miller_columns: vec![
                        SortBy::Size,
                        SortBy::Modified,
                        SortBy::Created,
                        SortBy::Kind,
                    ],
                    ..Default::default()
                };
                let width = 240.0 + metadata_width(&config, scale, &c);
                let rect = Rect::from_min_size(egui::pos2(12.0, 10.0), Vec2::new(width, 30.0));
                let cells = cells(rect, &config, scale, &c);
                assert_eq!(cells.len(), 5);
                assert!((cells[0].2.width() - 240.0).abs() < 0.001);
                for pair in cells.windows(2) {
                    assert!((pair[0].2.right() - pair[1].2.left()).abs() < 0.001);
                }
                assert!((cells.last().unwrap().2.right() - rect.right()).abs() < 0.001);
            }
        }
    }

    #[test]
    fn metadata_rows_paint_only_enabled_values_inside_matching_cells() {
        let ctx = Context::default();
        install_application_fonts(&ctx);
        let c = Colors::preset(ottrin_core::ThemePreset::Ottrin, true);
        let config = AppConfig {
            miller_columns: vec![SortBy::Modified, SortBy::Size, SortBy::Created],
            ..Default::default()
        };
        let entry = FileEntry {
            path: PathBuf::from("report.txt"),
            name: "report.txt".into(),
            kind: EntryKind::File,
            size_bytes: Some(2048),
            modified_unix_secs: Some(1700000000),
            created_unix_secs: Some(1600000000),
            is_executable: false,
            symlink_target_is_dir: None,
        };
        let width = 240.0 + metadata_width(&config, 1.0, &c);
        let mut row_rect = Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 100.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    row_rect = render_row(
                        ui,
                        &entry,
                        EntryVisualState::default(),
                        file_row::FileRowLayout {
                            index: 0,
                            width,
                            scale: 1.0,
                        },
                        &c,
                        &config,
                    )
                    .rect;
                });
            },
        );
        for (by, _, cell) in cells(row_rect, &config, 1.0, &c).into_iter().skip(1) {
            let expected = match by {
                SortBy::Modified => format_modified(1700000000),
                SortBy::Created => format_modified(1600000000),
                SortBy::Size => format_size(2048),
                _ => unreachable!(),
            };
            let shape = output
                .shapes
                .iter()
                .find(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == expected))
                .expect("enabled metadata value is painted");
            assert!(shape.clip_rect.left() >= cell.left());
            assert!(shape.clip_rect.right() <= cell.right());
        }
    }
}
