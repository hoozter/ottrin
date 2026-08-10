use crate::{
    Colors, MI_ARROW_BACK, MI_ARROW_FORWARD, MI_CONTENT_COPY, MI_DELETE, MI_EDIT, MI_FOLDER_OPEN,
    MI_FOLDER_ZIP, MI_OPEN_IN_NEW, file_icon_for_path, mix_color,
};
use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, ScrollArea, Sense, Stroke, Vec2,
};
use ottrin_preview::{
    PreviewData, PreviewKind, SpreadsheetOmission, SpreadsheetOmissionReason, SpreadsheetPreview,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericPreviewAction {
    Rename,
    Delete,
}

pub(crate) fn inspector_action_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    c: Colors,
) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(format!("{icon}  {label}"))
                .size(11.5)
                .color(c.text),
        )
        .fill(c.panel)
        .stroke(Stroke::new(1.0, mix_color(c.border, c.panel_raised, 0.35)))
        .corner_radius(6.0)
        .min_size(Vec2::new(108.0, 30.0)),
    )
    .on_hover_text(tooltip)
}

pub(crate) fn inspector_icon_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    c: Colors,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(16.0).color(c.text))
            .fill(c.panel)
            .stroke(Stroke::new(1.0, mix_color(c.border, c.panel_raised, 0.35)))
            .corner_radius(6.0)
            .min_size(Vec2::new(30.0, 28.0)),
    )
    .on_hover_text(format!("{label}\n{tooltip}"))
}

pub(crate) fn compact_preview_metadata(name: &str, body: &str) -> String {
    let mut parts = vec![name.to_string()];
    parts.extend(
        body.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty()
                    || trimmed.eq_ignore_ascii_case("Image preview available.")
                    || trimmed.eq_ignore_ascii_case("Open externally for full editing.")
                {
                    None
                } else if trimmed.ends_with('.') && !trimmed.contains(':') {
                    Some(trimmed.trim_end_matches('.').to_string())
                } else {
                    Some(trimmed.to_string())
                }
            })
            .take(3),
    );
    parts.join(" · ")
}

pub(crate) fn split_preview_extracted_text(body: &str) -> (&str, Option<&str>) {
    const MARKER: &str = "\n\nExtracted text:\n";
    if let Some((metadata, text)) = body.split_once(MARKER) {
        (metadata, Some(text.trim()))
    } else {
        (body, None)
    }
}

fn split_archive_listing(body: &str) -> (&str, Option<&str>) {
    const MARKER: &str = "\nContents:\n";
    if let Some((metadata, listing)) = body.split_once(MARKER) {
        (metadata, Some(listing.trim_end()))
    } else {
        (body, None)
    }
}

const MAX_ARCHIVE_TREE_NODES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveTreeNode {
    name: String,
    directory: bool,
    size: Option<String>,
    children: Vec<ArchiveTreeNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveTreeModel {
    roots: Vec<ArchiveTreeNode>,
    truncated: bool,
}

fn archive_display_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_control() {
                '�'
            } else {
                character
            }
        })
        .collect()
}

fn insert_archive_path(
    siblings: &mut Vec<ArchiveTreeNode>,
    components: &[String],
    explicit_directory: bool,
    size: Option<&str>,
    remaining_nodes: &mut usize,
) -> bool {
    let Some((name, tail)) = components.split_first() else {
        return true;
    };
    let directory = explicit_directory || !tail.is_empty();
    let position = siblings.iter().position(|node| node.name == *name);
    let index = if let Some(position) = position {
        position
    } else {
        if *remaining_nodes == 0 {
            return false;
        }
        *remaining_nodes -= 1;
        siblings.push(ArchiveTreeNode {
            name: name.clone(),
            directory,
            size: if tail.is_empty() {
                size.map(str::to_string)
            } else {
                None
            },
            children: Vec::new(),
        });
        siblings.len() - 1
    };

    let node = &mut siblings[index];
    node.directory |= directory;
    if tail.is_empty() && node.size.is_none() {
        node.size = size.map(str::to_string);
    }
    if tail.is_empty() {
        true
    } else {
        insert_archive_path(
            &mut node.children,
            tail,
            explicit_directory,
            size,
            remaining_nodes,
        )
    }
}

fn split_archive_listing_entry(line: &str) -> (&str, Option<&str>) {
    let line = line.strip_prefix("  ").unwrap_or(line);
    if let Some((path, size)) = line.rsplit_once("  (")
        && let Some(size) = size.strip_suffix(')')
    {
        (path, Some(size))
    } else {
        (line, None)
    }
}

fn archive_tree_from_listing(listing: &str) -> ArchiveTreeModel {
    let mut roots = Vec::new();
    let mut remaining_nodes = MAX_ARCHIVE_TREE_NODES;
    let mut truncated = false;

    for line in listing.lines() {
        let (path, size) = split_archive_listing_entry(line);
        if path.is_empty() {
            continue;
        }
        let explicit_directory = path.ends_with('/');
        let components = path
            .split('/')
            .filter(|component| !component.is_empty())
            .map(archive_display_name)
            .collect::<Vec<_>>();
        if components.is_empty() {
            continue;
        }
        if !insert_archive_path(
            &mut roots,
            &components,
            explicit_directory,
            size,
            &mut remaining_nodes,
        ) {
            truncated = true;
            break;
        }
    }

    ArchiveTreeModel { roots, truncated }
}

#[cfg(test)]
fn archive_tree_node_count(nodes: &[ArchiveTreeNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + archive_tree_node_count(&node.children))
        .sum()
}

fn archive_partial_notice(metadata: &str) -> Option<&str> {
    metadata
        .lines()
        .find(|line| line.starts_with("Partial listing:"))
}

fn render_archive_tree_nodes(
    ui: &mut egui::Ui,
    nodes: &[ArchiveTreeNode],
    c: Colors,
    parent_id: &str,
    depth: usize,
) {
    for (index, node) in nodes.iter().enumerate() {
        let node_id = format!("{parent_id}/{index}:{}", node.name);
        if node.directory {
            egui::CollapsingHeader::new(
                RichText::new(format!("{MI_FOLDER_OPEN}  {}", node.name))
                    .color(c.text)
                    .size(12.0),
            )
            .id_salt(&node_id)
            .default_open(depth < 2)
            .show(ui, |ui| {
                render_archive_tree_nodes(ui, &node.children, c, &node_id, depth + 1);
            });
        } else {
            ui.horizontal(|ui| {
                let icon = file_icon_for_path(Path::new(&node.name));
                ui.label(RichText::new(icon).color(c.text_dim).size(13.0));
                ui.add(
                    egui::Label::new(RichText::new(&node.name).color(c.text).size(12.0))
                        .selectable(true),
                );
                if let Some(size) = &node.size {
                    ui.label(RichText::new(size).color(c.text_muted).size(10.5));
                }
            });
        }
    }
}

pub(crate) fn preview_payload_for_copy(
    preview: &PreviewData,
) -> Option<(&'static str, &'static str, &str)> {
    match preview.kind {
        PreviewKind::Text | PreviewKind::Pdf | PreviewKind::OfficeMetadata => {
            let (_, extracted) = split_preview_extracted_text(&preview.body);
            let payload = extracted.unwrap_or(preview.body.trim());
            (!payload.is_empty()).then_some((MI_CONTENT_COPY, "Copy Text", payload))
        }
        PreviewKind::ArchiveMetadata => {
            let (_, listing) = split_archive_listing(&preview.body);
            listing
                .filter(|payload| !payload.is_empty())
                .map(|payload| (MI_CONTENT_COPY, "Copy File List", payload))
        }
        _ => None,
    }
}

pub(crate) fn render_preview_footer(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    if body.trim().is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(
            RichText::new(compact_preview_metadata(name, body))
                .color(c.text_dim)
                .size(11.0),
        );
    });
}

pub(crate) fn scrollable_preview_height(available_height: f32, minimum_height: f32) -> f32 {
    (available_height - 30.0).max(minimum_height)
}

pub(crate) fn docx_width_fit_scale(page_width: f32, available_width: f32) -> f32 {
    if page_width <= 0.0 {
        return 1.0;
    }
    (available_width.max(0.0) / page_width).min(1.0)
}

fn render_scrollable_preview_text(
    ui: &mut egui::Ui,
    text: &str,
    color: Color32,
    minimum_height: f32,
    id: impl std::hash::Hash,
) {
    let height = scrollable_preview_height(ui.available_height(), minimum_height);
    let width = ui.available_width();
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
        .show(ui, |ui| {
            ui.set_width(width);
            ScrollArea::vertical()
                .id_salt(id)
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    ui.add(
                        egui::Label::new(
                            RichText::new(text)
                                .font(FontId::monospace(12.0))
                                .color(color),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
        });
}

pub(crate) fn render_text_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    let source = extracted.unwrap_or(body).trim();
    render_scrollable_preview_text(ui, source, c.text, 160.0, ("inspector_text", name));
    render_preview_footer(ui, c, name, metadata);
}

pub(crate) fn render_archive_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    let (metadata, listing) = split_archive_listing(body);
    let listing = listing.unwrap_or("").trim_end();
    if listing.is_empty() {
        render_metadata_card_preview(ui, c, MI_FOLDER_ZIP, name, metadata);
        return;
    }

    let tree = archive_tree_from_listing(listing);
    let height = scrollable_preview_height(ui.available_height(), 180.0);
    let width = ui.available_width();
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
        .show(ui, |ui| {
            ui.set_width(width);
            ScrollArea::vertical()
                .id_salt(("inspector_archive_tree", name))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(width);
                    egui::CollapsingHeader::new(
                        RichText::new(format!("{MI_FOLDER_ZIP}  {}", archive_display_name(name)))
                            .color(c.text)
                            .size(12.5)
                            .strong(),
                    )
                    .id_salt(("archive_root", name))
                    .default_open(true)
                    .show(ui, |ui| {
                        render_archive_tree_nodes(ui, &tree.roots, c, name, 0);
                        if let Some(notice) = archive_partial_notice(metadata) {
                            ui.label(
                                RichText::new(notice)
                                    .color(c.text_muted)
                                    .italics()
                                    .size(11.0),
                            );
                        }
                        if tree.truncated {
                            ui.label(
                                RichText::new("Tree rendering truncated at the safe node limit.")
                                    .color(c.text_muted)
                                    .italics()
                                    .size(11.0),
                            );
                        }
                    });
                });
        });
    render_preview_footer(ui, c, name, metadata);
}

const EXTRACTED_TEXT_FALLBACK_LABEL: &str = "Extracted text fallback";

fn render_extracted_text_fallback_content(ui: &mut egui::Ui, c: Colors, text: &str) {
    ui.with_layout(Layout::top_down(Align::Min), |ui| {
        ui.label(
            RichText::new(EXTRACTED_TEXT_FALLBACK_LABEL)
                .color(c.text)
                .size(12.5)
                .strong(),
        );
        ui.label(
            RichText::new(
                "Source layout is unavailable. This fallback preserves extracted line breaks and whitespace only.",
            )
            .color(c.text_muted)
            .size(10.5),
        );
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(FontId::monospace(12.0))
                    .color(c.text),
            )
            .selectable(true),
        );
    });
}

pub(crate) fn render_document_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    name: &str,
    body: &str,
) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    if let Some(text) = extracted.filter(|text| !text.is_empty()) {
        let height = scrollable_preview_height(ui.available_height(), 180.0);
        let width = ui.available_width();
        egui::Frame::new()
            .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
            .show(ui, |ui| {
                ui.set_width(width);
                ScrollArea::both()
                    .id_salt(("inspector_document_fallback", name))
                    .max_height(height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(width);
                        render_extracted_text_fallback_content(ui, c, text);
                    });
            });
        render_preview_footer(ui, c, name, metadata);
    } else {
        render_metadata_card_preview(ui, c, icon, name, metadata);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpreadsheetRow {
    label: String,
    cells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpreadsheetColumn {
    label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpreadsheetSheet {
    id: String,
    name: String,
    columns: Vec<SpreadsheetColumn>,
    rows: Vec<SpreadsheetRow>,
    omissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpreadsheetGrid {
    sheets: Vec<SpreadsheetSheet>,
    omissions: Vec<String>,
}

const MAX_VISIBLE_BLANK_GAP: usize = 16;
const MAX_RENDERED_SPREADSHEET_AXIS: usize = 64;

fn spreadsheet_grid_from_payload(payload: &SpreadsheetPreview) -> SpreadsheetGrid {
    let sheets = payload
        .sheets
        .iter()
        .map(|sheet| {
            let mut row_coordinates = sheet.cells.iter().map(|cell| cell.row).collect::<Vec<_>>();
            let mut column_coordinates = sheet
                .cells
                .iter()
                .map(|cell| cell.column)
                .collect::<Vec<_>>();
            row_coordinates.sort_unstable();
            row_coordinates.dedup();
            column_coordinates.sort_unstable();
            column_coordinates.dedup();
            let (row_coordinates, omitted_rows) = bounded_spreadsheet_axis(row_coordinates);
            let (column_coordinates, omitted_columns) =
                bounded_spreadsheet_axis(column_coordinates);
            let rows = spreadsheet_axis_labels(&row_coordinates, false);
            let columns = spreadsheet_axis_labels(&column_coordinates, true)
                .into_iter()
                .map(|label| SpreadsheetColumn { label })
                .collect::<Vec<_>>();
            let row_labels = rows.clone();
            let mut rendered_rows = rows
                .into_iter()
                .map(|label| SpreadsheetRow {
                    label,
                    cells: vec![String::new(); columns.len()],
                })
                .collect::<Vec<_>>();
            for cell in &sheet.cells {
                let row_label = (cell.row + 1).to_string();
                let column_label = spreadsheet_column_label(cell.column);
                if let (Some(row), Some(column)) = (
                    row_labels.iter().position(|label| label == &row_label),
                    columns
                        .iter()
                        .position(|column| column.label == column_label),
                ) {
                    rendered_rows[row].cells[column] = cell.text.clone();
                }
            }
            SpreadsheetSheet {
                id: sheet.id.clone(),
                name: sheet.name.clone(),
                columns,
                rows: rendered_rows,
                omissions: sheet
                    .omissions
                    .iter()
                    .map(spreadsheet_omission_label)
                    .chain(omitted_rows.map(|omitted| {
                        format!("{omitted} populated row(s) omitted at safe render limit")
                    }))
                    .chain(omitted_columns.map(|omitted| {
                        format!("{omitted} populated column(s) omitted at safe render limit")
                    }))
                    .collect(),
            }
        })
        .collect();
    SpreadsheetGrid {
        sheets,
        omissions: payload
            .omissions
            .iter()
            .map(spreadsheet_omission_label)
            .collect(),
    }
}

fn bounded_spreadsheet_axis(mut coordinates: Vec<usize>) -> (Vec<usize>, Option<usize>) {
    if coordinates.len() <= MAX_RENDERED_SPREADSHEET_AXIS {
        return (coordinates, None);
    }
    let omitted = coordinates.len() - MAX_RENDERED_SPREADSHEET_AXIS;
    let tail = coordinates.split_off(MAX_RENDERED_SPREADSHEET_AXIS / 2);
    coordinates.truncate(MAX_RENDERED_SPREADSHEET_AXIS / 2);
    coordinates.extend(tail.into_iter().skip(omitted));
    (coordinates, Some(omitted))
}

fn spreadsheet_axis_labels(coordinates: &[usize], columns: bool) -> Vec<String> {
    let mut labels = Vec::new();
    let mut previous = None;
    for &coordinate in coordinates {
        let start = previous.map_or(0, |value| value + 1);
        if coordinate > start {
            let gap = coordinate - start;
            if gap <= MAX_VISIBLE_BLANK_GAP {
                for blank in start..coordinate {
                    labels.push(if columns {
                        spreadsheet_column_label(blank)
                    } else {
                        (blank + 1).to_string()
                    });
                }
            } else {
                labels.push(if columns {
                    format!(
                        "… {}–{} omitted",
                        spreadsheet_column_label(start),
                        spreadsheet_column_label(coordinate - 1)
                    )
                } else {
                    format!("… {}–{} omitted", start + 1, coordinate)
                });
            }
        }
        labels.push(if columns {
            spreadsheet_column_label(coordinate)
        } else {
            (coordinate + 1).to_string()
        });
        previous = Some(coordinate);
    }
    labels
}

fn spreadsheet_omission_label(omission: &SpreadsheetOmission) -> String {
    match omission.reason {
        SpreadsheetOmissionReason::SparseRowGap { from, to } => {
            format!(
                "Rows {}–{} omitted to bound sparse preview",
                from + 1,
                to + 1
            )
        }
        SpreadsheetOmissionReason::SparseColumnGap { from, to } => format!(
            "Columns {}–{} omitted to bound sparse preview",
            spreadsheet_column_label(from),
            spreadsheet_column_label(to)
        ),
        SpreadsheetOmissionReason::SheetLimit { omitted } => {
            format!("{omitted} worksheet(s) omitted at safe limit")
        }
        SpreadsheetOmissionReason::RowLimit { from, to } => {
            format!("Rows {}–{} omitted at safe limit", from + 1, to + 1)
        }
        SpreadsheetOmissionReason::ColumnLimit { from, to } => {
            format!(
                "Columns {}–{} omitted at safe limit",
                spreadsheet_column_label(from),
                spreadsheet_column_label(to)
            )
        }
        SpreadsheetOmissionReason::CellLimit { omitted } => {
            format!("{omitted} cell(s) omitted at safe limit")
        }
        SpreadsheetOmissionReason::TextLimit { omitted } => {
            format!("{omitted} character(s) omitted at safe limit")
        }
        SpreadsheetOmissionReason::SharedStringUnavailable { index } => {
            format!("Shared string {index} unavailable at safe scan limit")
        }
    }
}

pub(crate) fn render_spreadsheet_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    preview: &PreviewData,
    selected_sheet: &mut Option<String>,
) -> bool {
    let Some(payload) = preview.spreadsheet.as_ref() else {
        return false;
    };
    let grid = spreadsheet_grid_from_payload(payload);
    let Some(selected_index) =
        spreadsheet_selected_sheet_index(&grid.sheets, selected_sheet.as_deref())
    else {
        return false;
    };
    if selected_sheet.as_deref() != Some(grid.sheets[selected_index].id.as_str()) {
        *selected_sheet = Some(grid.sheets[selected_index].id.clone());
    }
    ui.horizontal_wrapped(|ui| {
        for sheet in &grid.sheets {
            if ui
                .selectable_label(
                    selected_sheet.as_deref() == Some(sheet.id.as_str()),
                    &sheet.name,
                )
                .on_hover_text("Show this worksheet")
                .clicked()
            {
                *selected_sheet = Some(sheet.id.clone());
            }
        }
    });
    ui.add_space(6.0);
    let sheet = &grid.sheets[selected_index];
    let height = scrollable_preview_height(ui.available_height(), 180.0);
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
        .show(ui, |ui| {
            ScrollArea::both()
                .id_salt(("inspector_spreadsheet", name, &sheet.id))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new(("spreadsheet_grid", name, &sheet.id))
                        .striped(true)
                        .min_col_width(112.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new("#").color(c.text_muted).strong());
                            for column in &sheet.columns {
                                ui.label(RichText::new(&column.label).color(c.text_muted).strong());
                            }
                            ui.end_row();
                            for row in &sheet.rows {
                                ui.label(RichText::new(&row.label).color(c.text_muted));
                                for value in &row.cells {
                                    ui.add(
                                        egui::Label::new(RichText::new(value).color(c.text))
                                            .selectable(true),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    for omission in sheet.omissions.iter().chain(grid.omissions.iter()) {
        ui.label(RichText::new(omission).color(c.text_dim).size(11.0));
    }
    render_preview_footer(ui, c, name, &preview.body);
    true
}

fn spreadsheet_selected_sheet_index(
    sheets: &[SpreadsheetSheet],
    selected_id: Option<&str>,
) -> Option<usize> {
    selected_id
        .and_then(|id| sheets.iter().position(|sheet| sheet.id == id))
        .or_else(|| (!sheets.is_empty()).then_some(0))
}

fn spreadsheet_column_label(mut column: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, char::from(b'A' + (column % 26) as u8));
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    label
}

/// Renders bounded document text as content of an existing scroll region.
/// The caller owns scrolling so a paired visual raster and its accessible text
/// share one geometry and scrollbar reservation.
pub(crate) fn render_document_body_preview_in_scroll_content(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    name: &str,
    body: &str,
) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    if let Some(text) = extracted.filter(|text| !text.is_empty()) {
        egui::CollapsingHeader::new(
            RichText::new(EXTRACTED_TEXT_FALLBACK_LABEL)
                .color(c.text_muted)
                .size(11.0),
        )
        .id_salt(("document_extracted_fallback", name))
        .default_open(false)
        .show(ui, |ui| render_extracted_text_fallback_content(ui, c, text));
        render_preview_footer(ui, c, name, metadata);
    } else {
        render_metadata_card_preview(ui, c, icon, name, metadata);
    }
}

#[allow(deprecated)]
pub(crate) fn render_docx_page_preview(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    body: &str,
    _fit: bool,
    _zoom: f32,
) {
    render_document_body_preview(ui, c, MI_OPEN_IN_NEW, name, body);
}

pub(crate) fn render_folder_body_preview(ui: &mut egui::Ui, c: Colors, name: &str, body: &str) {
    ui.add_space(16.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(RichText::new(MI_FOLDER_OPEN).size(58.0).color(c.folder));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
    });
    ui.add_space(10.0);
    let (metadata, sample) = if let Some((metadata, sample)) = body.split_once("\n\nSample:\n") {
        (metadata, Some(sample))
    } else {
        (body, None)
    };
    for line in metadata.lines() {
        if let Some((key, val)) = line.split_once(':') {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{key}:"))
                        .color(c.text_muted)
                        .size(11.5),
                );
                ui.label(RichText::new(val.trim()).color(c.text).size(11.5));
            });
        } else if !line.trim().is_empty() {
            ui.label(RichText::new(line).color(c.text_dim).size(11.5));
        }
    }
    if let Some(sample) = sample {
        ui.add_space(10.0);
        ui.label(
            RichText::new("Sample contents")
                .color(c.text)
                .size(12.0)
                .strong(),
        );
        let mut text = sample.to_string();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .font(FontId::monospace(11.0))
                .text_color(c.text_dim)
                .frame(true)
                .desired_rows(10)
                .interactive(true),
        );
    }
}

pub(crate) fn render_generic_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    path: &Path,
    name: &str,
    preview: &PreviewData,
) -> Option<GenericPreviewAction> {
    let mut action = None;
    ui.add_space(20.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let icon = file_icon_for_path(path);
        ui.label(RichText::new(icon).size(64.0));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
        if !preview.body.is_empty() {
            ui.add_space(8.0);
            render_preview_metadata_rows(ui, c, &preview.body);
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if inspector_action_button(
                ui,
                MI_CONTENT_COPY,
                "Copy Path",
                "Copy this file path to the clipboard.",
                c,
            )
            .clicked()
            {
                ui.ctx().copy_text(path.display().to_string());
            }
            if preview.open_external
                && inspector_action_button(
                    ui,
                    MI_OPEN_IN_NEW,
                    "Open",
                    "Open this file with the system default app.",
                    c,
                )
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl {
                    url: format!("file://{}", path.display()),
                    new_tab: false,
                });
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if inspector_action_button(ui, MI_EDIT, "Rename", "Rename this file.", c).clicked() {
                action = Some(GenericPreviewAction::Rename);
            }
            if inspector_action_button(
                ui,
                MI_DELETE,
                "Move to Trash",
                "Move this file to Trash with confirmation.",
                c,
            )
            .clicked()
            {
                action = Some(GenericPreviewAction::Delete);
            }
        });
    });
    action
}

pub(crate) fn render_metadata_card_preview(
    ui: &mut egui::Ui,
    c: Colors,
    icon: &str,
    name: &str,
    metadata: &str,
) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() * 0.18).min(48.0));
        ui.label(RichText::new(icon).size(58.0).color(c.folder));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0));
        ui.add_space(10.0);
        render_preview_metadata_rows(ui, c, metadata);
    });
}

fn render_preview_metadata_rows(ui: &mut egui::Ui, c: Colors, metadata: &str) {
    for line in metadata.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{key}:"))
                        .color(c.text_muted)
                        .size(11.5),
                );
                ui.label(RichText::new(val.trim()).color(c.text).size(11.5));
            });
        } else {
            ui.label(RichText::new(line).color(c.text_dim).size(11.5));
        }
    }
}

pub(crate) fn render_image_hover_nav(
    ui: &mut egui::Ui,
    c: Colors,
    image_rect: Rect,
) -> Option<isize> {
    let pointer = ui.input(|input| input.pointer.hover_pos());
    if !pointer
        .map(|pos| image_rect.expand(8.0).contains(pos))
        .unwrap_or(false)
    {
        return None;
    }

    let arrow_size = Vec2::new(42.0, 64.0);
    let left_rect = Rect::from_center_size(
        egui::pos2(image_rect.left() + 30.0, image_rect.center().y),
        arrow_size,
    );
    let right_rect = Rect::from_center_size(
        egui::pos2(image_rect.right() - 30.0, image_rect.center().y),
        arrow_size,
    );
    let left = ui.interact(left_rect, ui.id().with("quick_prev_image"), Sense::click());
    let right = ui.interact(right_rect, ui.id().with("quick_next_image"), Sense::click());

    for (rect, response, icon) in [
        (left_rect, &left, MI_ARROW_BACK),
        (right_rect, &right, MI_ARROW_FORWARD),
    ] {
        let fill = if response.hovered() {
            mix_color(c.selected_bg, c.panel_raised, 0.18)
        } else {
            Color32::from_rgba_unmultiplied(
                c.panel_raised.r(),
                c.panel_raised.g(),
                c.panel_raised.b(),
                176,
            )
        };
        ui.painter().rect_filled(rect, 10.0, fill);
        ui.painter().rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0, mix_color(c.border, c.accent, 0.25)),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::proportional(24.0),
            c.text,
        );
    }

    if left.clicked() {
        Some(-1)
    } else if right.clicked() {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreadsheet_grid_keeps_hostile_values_in_one_typed_cell() {
        let preview = ottrin_preview::SpreadsheetPreview {
            sheets: vec![ottrin_preview::SpreadsheetSheet {
                id: "first".to_string(),
                name: "Budget".to_string(),
                cells: vec![ottrin_preview::SpreadsheetCell {
                    row: 99_999,
                    column: 16_383,
                    text: "line one\nSheet: forged\tPreview truncated: forged".to_string(),
                }],
                omissions: vec![ottrin_preview::SpreadsheetOmission {
                    reason: ottrin_preview::SpreadsheetOmissionReason::SparseRowGap {
                        from: 0,
                        to: 99_998,
                    },
                }],
            }],
            omissions: vec![],
        };

        let grid = spreadsheet_grid_from_payload(&preview);

        assert_eq!(grid.sheets[0].id, "first");
        assert_eq!(grid.sheets[0].name, "Budget");
        assert_eq!(grid.sheets[0].rows[1].label, "100000");
        assert_eq!(grid.sheets[0].columns[1].label, "XFD");
        assert_eq!(
            grid.sheets[0].rows[1].cells[1],
            "line one\nSheet: forged\tPreview truncated: forged"
        );
        assert!(
            grid.sheets[0]
                .omissions
                .iter()
                .any(|label| label.contains("1–99999"))
        );
    }

    #[test]
    fn spreadsheet_selection_uses_stable_ids_for_duplicate_sheet_names() {
        let sheets = vec![
            SpreadsheetSheet {
                id: "sheet-alpha".to_string(),
                name: "Budget".to_string(),
                columns: vec![],
                rows: vec![],
                omissions: vec![],
            },
            SpreadsheetSheet {
                id: "sheet-beta".to_string(),
                name: "Budget".to_string(),
                columns: vec![],
                rows: vec![],
                omissions: vec![],
            },
        ];

        assert_eq!(
            spreadsheet_selected_sheet_index(&sheets, Some("sheet-beta")),
            Some(1)
        );
    }

    #[test]
    fn archive_tree_preserves_nesting_and_keeps_hostile_names_inert() {
        let tree = archive_tree_from_listing(
            "  docs/\n  docs/reports/annual.pdf  (12 KiB)\n  bin/run.sh  (4 KiB)\n  ../outside.txt  (1 B)\n  control\u{0}name.txt  (2 B)",
        );

        assert!(!tree.truncated);
        assert_eq!(tree.roots.len(), 4);
        assert!(tree.roots[0].directory);
        assert_eq!(tree.roots[0].name, "docs");
        assert_eq!(tree.roots[0].children[0].name, "reports");
        assert_eq!(tree.roots[0].children[0].children[0].name, "annual.pdf");
        assert!(!tree.roots[0].children[0].children[0].directory);
        assert_eq!(
            tree.roots[0].children[0].children[0].size.as_deref(),
            Some("12 KiB")
        );
        assert_eq!(tree.roots[2].name, "..");
        assert_eq!(tree.roots[2].children[0].name, "outside.txt");
        assert_eq!(tree.roots[3].name, "control�name.txt");
    }

    #[test]
    fn archive_tree_promotes_implied_folders_without_duplicate_nodes() {
        let tree = archive_tree_from_listing("a/b.txt\na/\na/c/d.txt\na/b.txt");

        assert_eq!(tree.roots.len(), 1);
        assert!(tree.roots[0].directory);
        assert_eq!(tree.roots[0].children.len(), 2);
        assert_eq!(tree.roots[0].children[0].name, "b.txt");
        assert_eq!(tree.roots[0].children[1].name, "c");
        assert_eq!(tree.roots[0].children[1].children[0].name, "d.txt");
    }

    #[test]
    fn archive_tree_reports_when_implied_folders_exhaust_the_render_budget() {
        let listing = (0..MAX_ARCHIVE_TREE_NODES)
            .map(|index| format!("root-{index}/a/b/c/d/e/file.txt"))
            .collect::<Vec<_>>()
            .join("\n");

        let tree = archive_tree_from_listing(&listing);

        assert!(tree.truncated);
        assert!(archive_tree_node_count(&tree.roots) <= MAX_ARCHIVE_TREE_NODES);
    }

    #[test]
    fn archive_tree_does_not_confuse_a_legitimate_filename_with_partial_state() {
        let tree =
            archive_tree_from_listing("docs/readme.txt  (1 KiB)\n  ... and notes.txt  (2 B)");

        assert_eq!(tree.roots.len(), 2);
        assert_eq!(tree.roots[1].name, "... and notes.txt");
        assert_eq!(tree.roots[1].size.as_deref(), Some("2 B"));
        assert_eq!(
            archive_partial_notice(
                "Filetype: ZIP archive\nPartial listing: 201 entries omitted after the first 200."
            ),
            Some("Partial listing: 201 entries omitted after the first 200.")
        );
    }

    #[test]
    fn document_extraction_is_truthfully_labeled_and_preserves_structure() {
        let body =
            "Filetype: DOCX document\n\nExtracted text:\nHeading\n\n  indented item\nline  two";
        let (_, extracted) = split_preview_extracted_text(body);

        assert_eq!(EXTRACTED_TEXT_FALLBACK_LABEL, "Extracted text fallback");
        assert_eq!(extracted, Some("Heading\n\n  indented item\nline  two"));
    }
}
