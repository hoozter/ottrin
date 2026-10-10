use crate::widget_style::ThemeButtonExt;
use crate::{
    Colors, MI_ARROW_BACK, MI_ARROW_FORWARD, MI_CONTENT_COPY, MI_DELETE, MI_DESCRIPTION, MI_EDIT,
    MI_FOLDER_OPEN, MI_FOLDER_ZIP, MI_INFO, MI_MUSIC_NOTE, MI_OPEN_IN_NEW, file_icon_for_path,
    mix_color,
};
use egui::{
    Align, Align2, Color32, FontId, Layout, Rect, RichText, ScrollArea, Sense, Stroke, Vec2,
};
use egui_extras::syntax_highlighting::{CodeTheme, highlight};
use ottrin_core::ContentFileGroup;
use ottrin_preview::{
    DocxAlignment, DocxBlock, DocxImageFallback, DocxList, DocxParagraph, DocxParagraphContent,
    PreviewData, PreviewKind, SpreadsheetOmission, SpreadsheetOmissionReason, SpreadsheetPreview,
};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) const PREVIEW_CONTENT_PADDING: f32 = 16.0;
pub(crate) const FLOW_TEXT_SIZE: f32 = 16.0;
pub(crate) const SPREADSHEET_MIN_COLUMN_WIDTH: f32 = 96.0;
const SPREADSHEET_ROW_HEADER_WIDTH: f32 = 46.0;
const SPREADSHEET_CELL_HORIZONTAL_PADDING: f32 = 16.0;

/// The same sizing vocabulary is used by the Miller preview and Quick Inspector.
/// Fit modes respond to available space; manual modes intentionally do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewSizingMode {
    FitPage,
    FitWidth,
}

/// Per-body controls shared by the compact preview and Quick Inspector.
/// These are presentation choices only: they never alter the preview payload.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewBodySizing {
    pub flow_text_scale: f32,
    pub spreadsheet_overview: bool,
}

impl Default for PreviewBodySizing {
    fn default() -> Self {
        Self {
            flow_text_scale: 1.0,
            spreadsheet_overview: false,
        }
    }
}

pub(crate) fn preview_sizing_for(kind: PreviewKind) -> PreviewSizingMode {
    match kind {
        PreviewKind::Pdf | PreviewKind::OfficeMetadata => PreviewSizingMode::FitWidth,
        _ => PreviewSizingMode::FitPage,
    }
}

pub(crate) fn fit_size(source: Vec2, bounds: Vec2, mode: PreviewSizingMode) -> Vec2 {
    if source.x <= 0.0 || source.y <= 0.0 {
        return source;
    }
    let scale = match mode {
        PreviewSizingMode::FitWidth => bounds.x.max(0.0) / source.x,
        PreviewSizingMode::FitPage => {
            (bounds.x.max(0.0) / source.x).min(bounds.y.max(0.0) / source.y)
        }
    };
    let scale = if matches!(mode, PreviewSizingMode::FitPage) {
        scale.min(1.0)
    } else {
        scale
    };
    source * scale
}

pub(crate) fn spreadsheet_column_width(available_per_column: f32, overview: bool) -> f32 {
    if overview {
        available_per_column.max(1.0)
    } else {
        available_per_column.max(SPREADSHEET_MIN_COLUMN_WIDTH)
    }
}

/// Width available to cell *content*, accounting for row labels and cell chrome.
/// This is deliberately separate from `spreadsheet_column_width`: Grid allocates
/// the content width plus each cell's inner margin.
pub(crate) fn spreadsheet_content_width(
    viewport_width: f32,
    column_count: usize,
    overview: bool,
) -> f32 {
    let columns = column_count.max(1) as f32;
    let available = ((viewport_width
        - SPREADSHEET_ROW_HEADER_WIDTH
        - SPREADSHEET_CELL_HORIZONTAL_PADDING
        - 2.0)
        / columns
        - SPREADSHEET_CELL_HORIZONTAL_PADDING)
        .max(1.0);
    spreadsheet_column_width(available, overview)
}

/// Shared allocation boundary for main Preview and Quick Inspector content.
pub(crate) fn preview_content_frame<R>(
    ui: &mut egui::Ui,
    size: Vec2,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let (outer, response) = ui.allocate_exact_size(size, Sense::hover());
    let inner_rect = outer.shrink2(Vec2::splat(PREVIEW_CONTENT_PADDING).min(size * 0.5));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(inner_rect));
    child.visuals_mut().clip_rect_margin = 0.0;
    child.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    let inner = ScrollArea::both()
        .auto_shrink([false, false])
        .show(&mut child, contents)
        .inner;
    egui::InnerResponse { inner, response }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericPreviewAction {
    Rename,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewCardAction {
    OpenExternally,
}

pub(crate) fn preview_card_action(preview: &PreviewData) -> Option<PreviewCardAction> {
    preview
        .open_external
        .then_some(PreviewCardAction::OpenExternally)
}

pub(crate) fn specialized_preview_body_action(preview: &PreviewData) -> Option<PreviewCardAction> {
    matches!(
        preview.kind,
        PreviewKind::VideoMetadata | PreviewKind::AudioMetadata
    )
    .then(|| preview_card_action(preview))
    .flatten()
}

pub(crate) fn shared_preview_body_kind(kind: PreviewKind) -> bool {
    matches!(
        kind,
        PreviewKind::Text
            | PreviewKind::OfficeMetadata
            | PreviewKind::ArchiveMetadata
            | PreviewKind::AudioMetadata
            | PreviewKind::EpsMetadata
    )
}

pub(crate) fn render_shared_preview_body(
    ui: &mut egui::Ui,
    c: Colors,
    path: &Path,
    name: &str,
    preview: &PreviewData,
    selected_sheet: &mut Option<String>,
    sizing: PreviewBodySizing,
) -> bool {
    match preview.kind {
        PreviewKind::Text => {
            render_text_body_preview_scaled(ui, c, name, &preview.body, sizing.flow_text_scale)
        }
        PreviewKind::OfficeMetadata => {
            if !render_docx_structured_body_preview(ui, c, name, preview, sizing.flow_text_scale)
                && !render_spreadsheet_body_preview(ui, c, name, preview, selected_sheet, sizing)
            {
                render_document_body_preview(ui, c, MI_DESCRIPTION, name, &preview.body);
            }
        }
        PreviewKind::ArchiveMetadata => render_archive_body_preview(ui, c, name, &preview.body),
        PreviewKind::AudioMetadata => {
            render_metadata_card_preview(ui, c, MI_MUSIC_NOTE, name, &preview.body);
            render_preview_card_action(ui, c, path, preview);
        }
        PreviewKind::EpsMetadata => {
            render_document_body_preview(ui, c, MI_OPEN_IN_NEW, name, &preview.body);
        }
        _ => return false,
    }
    true
}

pub(crate) fn render_preview_card_action(
    ui: &mut egui::Ui,
    c: Colors,
    path: &Path,
    preview: &PreviewData,
) {
    if preview_card_action(preview) == Some(PreviewCardAction::OpenExternally)
        && inspector_action_button(
            ui,
            MI_OPEN_IN_NEW,
            "Open externally",
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
}

pub(crate) fn inspector_action_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    tooltip: &str,
    c: Colors,
) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(format!("{icon}  {label}")).size(11.5 * c.font_scale))
            .min_size(Vec2::new(108.0, 30.0))
            .themed(&c),
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
        egui::Button::new(RichText::new(icon).size(16.0 * c.font_scale))
            .min_size(Vec2::new(30.0, 28.0))
            .themed(&c),
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
                    .size(12.0 * c.font_scale),
            )
            .id_salt(&node_id)
            .default_open(depth < 2)
            .show(ui, |ui| {
                render_archive_tree_nodes(ui, &node.children, c, &node_id, depth + 1);
            });
        } else {
            ui.horizontal(|ui| {
                let icon = file_icon_for_path(Path::new(&node.name));
                ui.label(
                    RichText::new(icon)
                        .color(c.text_dim)
                        .size(13.0 * c.font_scale),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&node.name)
                            .color(c.text)
                            .size(12.0 * c.font_scale),
                    )
                    .selectable(true),
                );
                if let Some(size) = &node.size {
                    ui.label(
                        RichText::new(size)
                            .color(c.text_muted)
                            .size(10.5 * c.font_scale),
                    );
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
                .size(11.0 * c.font_scale),
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
    highlighted: Option<egui::text::LayoutJob>,
    minimum_height: f32,
    id: impl std::hash::Hash,
    scale: f32,
) {
    let height = scrollable_preview_height(ui.available_height(), minimum_height);
    egui::Frame::new()
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(28)))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt(id)
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let highlighted = highlighted.map(|mut job| {
                        for section in &mut job.sections {
                            section.format.font_id.size = FLOW_TEXT_SIZE * scale.clamp(0.7, 2.0);
                        }
                        job
                    });
                    let label = highlighted.map_or_else(
                        || {
                            egui::Label::new(
                                RichText::new(text)
                                    .font(FontId::monospace(
                                        (FLOW_TEXT_SIZE * scale).clamp(10.0, 32.0),
                                    ))
                                    .color(color),
                            )
                        },
                        egui::Label::new,
                    );
                    ui.add(label.wrap().selectable(true));
                });
        });
}

fn source_preview_layout_job(
    context: &egui::Context,
    style: &egui::Style,
    name: &str,
    source: &str,
) -> Option<egui::text::LayoutJob> {
    let path = Path::new(name);
    if !ContentFileGroup::SourceCode.supports_path(path) {
        return None;
    }
    let language = path.extension()?.to_str()?.to_ascii_lowercase();
    let theme = CodeTheme::from_style(style);
    Some(highlight(context, style, &theme, source, &language))
}

pub(crate) fn render_text_body_preview_scaled(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    body: &str,
    scale: f32,
) {
    let (metadata, extracted) = split_preview_extracted_text(body);
    let source = extracted.unwrap_or(body).trim();
    let highlighted = source_preview_layout_job(ui.ctx(), ui.style(), name, source);
    render_scrollable_preview_text(
        ui,
        source,
        c.text,
        highlighted,
        160.0,
        ("inspector_text", name),
        scale,
    );
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
                            .size(12.5 * c.font_scale)
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
                                    .size(11.0 * c.font_scale),
                            );
                        }
                        if tree.truncated {
                            ui.label(
                                RichText::new("Tree rendering truncated at the safe node limit.")
                                    .color(c.text_muted)
                                    .italics()
                                    .size(11.0 * c.font_scale),
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
                .size(12.5 * c.font_scale)
                .strong(),
        );
        ui.label(
            RichText::new(
                "Source layout is unavailable. This fallback preserves extracted line breaks and whitespace only.",
            )
            .color(c.text_muted)
            .size(10.5 * c.font_scale),
        );
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(FontId::monospace(12.0 * c.font_scale))
                    .color(c.text),
            )
            .selectable(true),
        );
    });
}

fn docx_color(value: Option<&str>, fallback: Color32) -> Color32 {
    let Some(value) = value.filter(|value| value.len() == 6) else {
        return fallback;
    };
    let Ok(rgb) = u32::from_str_radix(value, 16) else {
        return fallback;
    };
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn docx_list_layout(list: &DocxList) -> (&str, f32) {
    (&list.marker, f32::from(list.level) * 18.0)
}

fn render_docx_paragraph(ui: &mut egui::Ui, c: Colors, paragraph: &DocxParagraph, scale: f32) {
    if paragraph.has_page_break {
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            RichText::new("Page break in source")
                .color(c.text_dim)
                .italics()
                .size(10.0),
        );
        ui.add_space(8.0);
    }
    if paragraph.content.is_empty() {
        return;
    }

    let alignment = match paragraph.alignment {
        DocxAlignment::Left | DocxAlignment::Justified => Align::Min,
        DocxAlignment::Center => Align::Center,
        DocxAlignment::Right => Align::Max,
    };
    let heading_style = paragraph.style_name.as_deref().is_some_and(|style| {
        let style = style.to_ascii_lowercase();
        style.contains("title") || style.contains("heading")
    });
    let render_content = |ui: &mut egui::Ui| {
        let render_runs = |ui: &mut egui::Ui, runs: &mut Vec<&ottrin_preview::DocxRun>| {
            if runs.is_empty() {
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for run in runs.drain(..) {
                    let size = run
                        .font_size_half_points
                        .map(|half_points| f32::from(half_points) / 2.0)
                        .unwrap_or(if heading_style { 16.0 } else { 12.0 })
                        .clamp(9.0, 28.0);
                    let mut text = RichText::new(&run.text)
                        .color(docx_color(run.color.as_deref(), c.text))
                        .size(size * (4.0 / 3.0) * scale);
                    if run.bold || heading_style {
                        text = text.strong();
                    }
                    if run.italic {
                        text = text.italics();
                    }
                    if run.underline {
                        text = text.underline();
                    }
                    ui.add(egui::Label::new(text).selectable(true));
                }
            });
        };
        let mut runs = Vec::new();
        for content in &paragraph.content {
            match content {
                DocxParagraphContent::Text(run) => runs.push(run),
                DocxParagraphContent::Image(image) => {
                    render_runs(ui, &mut runs);
                    if let Some(png) = image.png.as_ref() {
                        let hash = format!("{:x}", Sha256::digest(png));
                        let uri = format!("bytes://docx-inline/{hash}.png");
                        ui.ctx().include_bytes(uri.clone(), png.clone());
                        ui.add(
                            egui::Image::new(uri)
                                .max_width(ui.available_width().min(720.0))
                                .max_height(480.0),
                        );
                        if let Some(alt_text) = image.alt_text.as_deref() {
                            ui.label(RichText::new(alt_text).color(c.text_dim).size(10.0));
                        }
                    } else {
                        let reason = match image.fallback {
                            Some(DocxImageFallback::MissingRelationship) => {
                                "Inline image is missing from this document package"
                            }
                            Some(DocxImageFallback::UnsafeRelationship) => {
                                "Inline image uses a blocked or unsafe relationship"
                            }
                            Some(DocxImageFallback::InvalidImage) => {
                                "Inline image is not a supported safe raster image"
                            }
                            Some(DocxImageFallback::ImageBoundsExceeded) => {
                                "Inline image exceeds safe preview bounds"
                            }
                            None => "Inline image is unavailable",
                        };
                        let label = image
                            .alt_text
                            .as_deref()
                            .map(|alt| format!("{reason}: {alt}"))
                            .unwrap_or_else(|| reason.to_string());
                        ui.label(RichText::new(label).color(c.text_dim).italics().size(10.5));
                    }
                    ui.add_space(5.0);
                }
            }
        }
        render_runs(ui, &mut runs);
    };
    ui.with_layout(Layout::top_down(alignment), |ui| {
        if let Some(list) = paragraph.list.as_ref() {
            let (marker, indentation) = docx_list_layout(list);
            ui.horizontal_top(|ui| {
                ui.add_space(indentation);
                ui.add_sized(
                    [28.0, 18.0],
                    egui::Label::new(RichText::new(marker).color(c.text).size(12.0)),
                );
                ui.vertical(|ui| render_content(ui));
            });
        } else {
            render_content(ui);
        }
    });
    ui.add_space(if heading_style { 8.0 } else { 5.0 });
}

pub(crate) fn render_docx_structured_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    preview: &PreviewData,
    scale: f32,
) -> bool {
    let Some(document) = preview.document.as_ref() else {
        return false;
    };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.label(
            RichText::new("Ottrin preview")
                .color(c.text)
                .size(12.5 * c.font_scale)
                .strong(),
        );
        let info = ui
            .add(
                egui::Button::new(RichText::new(MI_INFO).color(c.text_muted).size(13.0 * c.font_scale))
                    .frame(false)
                    .min_size(Vec2::splat(18.0)).themed(&c),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if info.hovered() {
            egui::Tooltip::for_widget(&info).show(|ui| {
                ui.label(
                    "Ottrin renders this document directly from the DOCX file. The preview should closely match the original, but fonts, page breaks, images and complex layouts may sometimes appear differently.",
                );
            });
        }
    });
    ui.add_space(6.0);

    let height = scrollable_preview_height(ui.available_height(), 180.0);
    egui::Frame::new()
        .fill(Color32::from_rgb(250, 250, 248))
        .stroke(Stroke::new(1.0, Color32::from_gray(190)))
        .inner_margin(egui::Margin::same(18))
        .show(ui, |ui| {
            ScrollArea::vertical()
                .id_salt(("inspector_docx_structure", name))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let document_colors = Colors {
                        text: Color32::from_rgb(32, 34, 38),
                        text_muted: Color32::from_rgb(92, 96, 104),
                        text_dim: Color32::from_rgb(112, 116, 124),
                        ..c
                    };
                    for (block_index, block) in document.blocks.iter().enumerate() {
                        match block {
                            DocxBlock::Paragraph(paragraph) => {
                                render_docx_paragraph(ui, document_colors, paragraph, scale);
                            }
                            DocxBlock::Table(table) => {
                                let stroke = if table.bordered {
                                    Stroke::new(1.0, Color32::from_gray(120))
                                } else {
                                    Stroke::NONE
                                };
                                egui::Grid::new(("docx_table", name, block_index))
                                    .spacing(Vec2::ZERO)
                                    .show(ui, |ui| {
                                        for row in &table.rows {
                                            for cell in &row.cells {
                                                egui::Frame::new()
                                                    .stroke(stroke)
                                                    .inner_margin(egui::Margin::same(7))
                                                    .show(ui, |ui| {
                                                        ui.set_min_width(150.0);
                                                        for paragraph in &cell.paragraphs {
                                                            render_docx_paragraph(
                                                                ui,
                                                                document_colors,
                                                                paragraph,
                                                                scale,
                                                            );
                                                        }
                                                    });
                                            }
                                            ui.end_row();
                                        }
                                    });
                                ui.add_space(8.0);
                            }
                        }
                    }
                });
        });

    let (metadata, extracted) = split_preview_extracted_text(&preview.body);
    if let Some(text) = extracted.filter(|text| !text.is_empty()) {
        egui::CollapsingHeader::new(
            RichText::new(EXTRACTED_TEXT_FALLBACK_LABEL)
                .color(c.text_muted)
                .size(11.0 * c.font_scale),
        )
        .id_salt(("document_extracted_fallback", name))
        .default_open(false)
        .show(ui, |ui| render_extracted_text_fallback_content(ui, c, text));
    }
    render_preview_footer(ui, c, name, metadata);
    true
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
            let last_row = sheet
                .cells
                .iter()
                .filter(|cell| !cell.text.is_empty())
                .map(|cell| cell.row)
                .max()
                .unwrap_or(0);
            let last_column = sheet
                .cells
                .iter()
                .filter(|cell| !cell.text.is_empty())
                .map(|cell| cell.column)
                .max()
                .unwrap_or(0);
            let relevant_cells = sheet
                .cells
                .iter()
                .filter(|cell| cell.row <= last_row && cell.column <= last_column);
            let mut row_coordinates = relevant_cells
                .clone()
                .map(|cell| cell.row)
                .collect::<Vec<_>>();
            let mut column_coordinates = relevant_cells.map(|cell| cell.column).collect::<Vec<_>>();
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
                    .filter(|omission| {
                        !spreadsheet_omission_is_rendered(
                            omission,
                            &row_coordinates,
                            &column_coordinates,
                        )
                    })
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

fn spreadsheet_omission_is_rendered(
    omission: &SpreadsheetOmission,
    row_coordinates: &[usize],
    column_coordinates: &[usize],
) -> bool {
    match omission.reason {
        SpreadsheetOmissionReason::SparseRowGap { from, to } => {
            sparse_gap_is_rendered(from, to, row_coordinates)
        }
        SpreadsheetOmissionReason::SparseColumnGap { from, to } => {
            sparse_gap_is_rendered(from, to, column_coordinates)
        }
        _ => false,
    }
}

fn sparse_gap_is_rendered(from: usize, to: usize, coordinates: &[usize]) -> bool {
    let Some(gap_size) = to.checked_sub(from).and_then(|width| width.checked_add(1)) else {
        return false;
    };
    gap_size <= MAX_VISIBLE_BLANK_GAP
        && (from == 0 || coordinates.binary_search(&(from - 1)).is_ok())
        && to
            .checked_add(1)
            .is_some_and(|after| coordinates.binary_search(&after).is_ok())
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

#[derive(Clone, Copy)]
enum SpreadsheetCellKind {
    Header,
    Body,
}

#[derive(Clone, Copy)]
struct SpreadsheetCellVisual {
    fill: Color32,
    text: Color32,
    border: Color32,
}

fn spreadsheet_cell_visual(
    kind: SpreadsheetCellKind,
    alternate_row: bool,
) -> SpreadsheetCellVisual {
    let fill = match kind {
        SpreadsheetCellKind::Header => Color32::from_rgb(242, 244, 247),
        SpreadsheetCellKind::Body if alternate_row => Color32::from_rgb(250, 251, 252),
        SpreadsheetCellKind::Body => Color32::WHITE,
    };
    SpreadsheetCellVisual {
        fill,
        text: match kind {
            SpreadsheetCellKind::Header => Color32::from_rgb(62, 68, 76),
            SpreadsheetCellKind::Body => Color32::from_rgb(30, 34, 40),
        },
        border: Color32::from_rgb(205, 210, 218),
    }
}

fn render_spreadsheet_row(
    ui: &mut egui::Ui,
    cells: &[(&str, SpreadsheetCellKind)],
    widths: &[f32],
    alternate: bool,
    scale: f32,
) {
    let font = FontId::proportional(14.0 * scale);
    let padding = Vec2::new(8.0, 4.0) * scale;
    let height = cells
        .iter()
        .zip(widths)
        .map(|((text, _), width)| {
            ui.fonts_mut(|fonts| {
                fonts.layout(
                    (*text).to_owned(),
                    font.clone(),
                    Color32::BLACK,
                    (*width - 2.0 * padding.x).max(1.0),
                )
            })
            .size()
            .y
        })
        .fold(22.0 * scale, f32::max)
        + 2.0 * padding.y;
    let (row, _) = ui.allocate_exact_size(Vec2::new(widths.iter().sum(), height), Sense::hover());
    let mut left = row.left();
    for ((text, kind), width) in cells.iter().zip(widths) {
        let rect = Rect::from_min_size(egui::pos2(left, row.top()), Vec2::new(*width, height));
        let visual = spreadsheet_cell_visual(*kind, alternate);
        ui.painter().rect(
            rect,
            0.0,
            visual.fill,
            Stroke::new(0.5, visual.border),
            egui::StrokeKind::Inside,
        );
        ui.put(
            rect.shrink2(padding),
            egui::Label::new(RichText::new(*text).font(font.clone()).color(visual.text))
                .wrap()
                .selectable(matches!(kind, SpreadsheetCellKind::Body)),
        );
        left += width;
    }
}

pub(crate) fn render_spreadsheet_body_preview(
    ui: &mut egui::Ui,
    c: Colors,
    name: &str,
    preview: &PreviewData,
    selected_sheet: &mut Option<String>,
    sizing: PreviewBodySizing,
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
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0, Color32::from_rgb(205, 210, 218)))
        .show(ui, |ui| {
            // egui normally expands scroll clips to accommodate widget shadows.
            // Cell backgrounds must stop inside this frame, including at the
            // trailing edge while a solid scrollbar reserves its own space.
            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
            ui.visuals_mut().clip_rect_margin = 0.0;
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ScrollArea::both()
                .id_salt(("inspector_spreadsheet", name, &sheet.id))
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let count = sheet.columns.len();
                    let available = (ui.available_width() - 2.0).max(1.0);
                    let column = spreadsheet_content_width(available, count, false);
                    let natural = SPREADSHEET_ROW_HEADER_WIDTH
                        + SPREADSHEET_CELL_HORIZONTAL_PADDING
                        + count as f32 * (column + SPREADSHEET_CELL_HORIZONTAL_PADDING);
                    let scale = if sizing.spreadsheet_overview {
                        (available / natural).min(1.0)
                    } else {
                        sizing.flow_text_scale.clamp(0.7, 2.0)
                    };
                    let mut widths = vec![
                        (SPREADSHEET_ROW_HEADER_WIDTH + SPREADSHEET_CELL_HORIZONTAL_PADDING)
                            * scale,
                    ];
                    widths.extend(std::iter::repeat_n(
                        (column + SPREADSHEET_CELL_HORIZONTAL_PADDING) * scale,
                        count,
                    ));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut header = vec![("#", SpreadsheetCellKind::Header)];
                    header.extend(
                        sheet
                            .columns
                            .iter()
                            .map(|column| (column.label.as_str(), SpreadsheetCellKind::Header)),
                    );
                    render_spreadsheet_row(ui, &header, &widths, false, scale);
                    for (index, row) in sheet.rows.iter().enumerate() {
                        let mut cells = vec![(row.label.as_str(), SpreadsheetCellKind::Header)];
                        cells.extend(
                            row.cells
                                .iter()
                                .map(|value| (value.as_str(), SpreadsheetCellKind::Body)),
                        );
                        render_spreadsheet_row(ui, &cells, &widths, index % 2 == 1, scale);
                    }
                });
        });
    for omission in sheet.omissions.iter().chain(grid.omissions.iter()) {
        ui.label(
            RichText::new(omission)
                .color(c.text_dim)
                .size(11.0 * c.font_scale),
        );
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
                .size(11.0 * c.font_scale),
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
        ui.label(
            RichText::new(MI_FOLDER_OPEN)
                .size(58.0 * c.font_scale)
                .color(c.folder),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0 * c.font_scale));
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
                        .size(11.5 * c.font_scale),
                );
                ui.label(
                    RichText::new(val.trim())
                        .color(c.text)
                        .size(11.5 * c.font_scale),
                );
            });
        } else if !line.trim().is_empty() {
            ui.label(
                RichText::new(line)
                    .color(c.text_dim)
                    .size(11.5 * c.font_scale),
            );
        }
    }
    if let Some(sample) = sample {
        ui.add_space(10.0);
        ui.label(
            RichText::new("Sample contents")
                .color(c.text)
                .size(12.0 * c.font_scale)
                .strong(),
        );
        let mut text = sample.to_string();
        ui.add(
            egui::TextEdit::multiline(&mut text)
                .font(FontId::monospace(11.0 * c.font_scale))
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
        ui.label(RichText::new(icon).size(64.0 * c.font_scale));
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0 * c.font_scale));
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
            render_preview_card_action(ui, c, path, preview);
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
        ui.label(
            RichText::new(icon)
                .size(58.0 * c.font_scale)
                .color(c.folder),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(name).color(c.text).size(14.0 * c.font_scale));
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
                        .size(11.5 * c.font_scale),
                );
                ui.label(
                    RichText::new(val.trim())
                        .color(c.text)
                        .size(11.5 * c.font_scale),
                );
            });
        } else {
            ui.label(
                RichText::new(line)
                    .color(c.text_dim)
                    .size(11.5 * c.font_scale),
            );
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
            FontId::proportional(24.0 * c.font_scale),
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
    fn rust_source_preview_uses_distinct_syntax_colors() {
        let context = egui::Context::default();
        let source = "fn main() { println!(\"hello\"); }\n";
        let job = source_preview_layout_job(&context, context.style().as_ref(), "main.rs", source)
            .expect("Rust source should be highlighted");

        assert_eq!(job.text, source);
        assert!(job.sections.len() > 1);
        assert!(
            job.sections
                .windows(2)
                .any(|sections| sections[0].format.color != sections[1].format.color),
            "source tokens should not all use the same color"
        );
    }

    #[test]
    fn ordinary_text_preview_does_not_claim_source_highlighting() {
        let context = egui::Context::default();

        assert!(
            source_preview_layout_job(
                &context,
                context.style().as_ref(),
                "notes.txt",
                "fn is prose here",
            )
            .is_none()
        );
    }

    #[test]
    fn wma_metadata_card_exposes_permitted_external_open_action() {
        let path = std::env::temp_dir().join(format!(
            "ottrin-actionable-audio-{}.wma",
            std::process::id()
        ));
        std::fs::write(&path, []).expect("write WMA fixture");
        let preview = ottrin_preview::load_preview(&ottrin_preview::PreviewRequest {
            path: path.clone(),
            kind_hint: ottrin_core::EntryKind::File,
        });
        let _ = std::fs::remove_file(path);

        assert_eq!(preview.kind, PreviewKind::AudioMetadata);
        assert_eq!(
            preview_card_action(&preview),
            Some(PreviewCardAction::OpenExternally)
        );
    }

    #[test]
    fn specialized_media_bodies_expose_permitted_external_open_action() {
        for kind in [PreviewKind::VideoMetadata, PreviewKind::AudioMetadata] {
            let preview = PreviewData {
                title: "sample.media".to_string(),
                kind,
                body: "System media".to_string(),
                document: None,
                spreadsheet: None,
                open_external: true,
            };

            assert_eq!(
                specialized_preview_body_action(&preview),
                Some(PreviewCardAction::OpenExternally)
            );
        }
    }

    #[test]
    fn preview_consistency_common_frame_reserves_padding_and_natural_size() {
        // The frame is independent of format: fit pages, wide tables and small
        // images all receive the same bounded viewport.
        for size in [
            Vec2::new(24.0, 18.0),
            Vec2::new(600.0, 900.0),
            Vec2::new(2000.0, 80.0),
        ] {
            let ctx = egui::Context::default();
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let origin = ui.cursor().min;
                    let outer = Rect::from_min_size(origin, Vec2::new(320.0, 240.0));
                    let result = preview_content_frame(ui, outer.size(), |ui| {
                        let clip = ui.clip_rect();
                        let content = if size.x < 100.0 {
                            let texture = ui.ctx().load_texture(
                                "small image fixture",
                                egui::ColorImage::filled([24, 18], Color32::WHITE),
                                Default::default(),
                            );
                            ui.add(egui::Image::new(&texture)).rect
                        } else {
                            ui.allocate_exact_size(size, Sense::hover()).0
                        };
                        assert!(
                            outer.shrink(16.0).contains_rect(clip),
                            "missing common minimum padding: {clip:?} in {outer:?}"
                        );
                        if size.x < 100.0 {
                            assert_eq!(
                                content.size(),
                                size,
                                "small images keep natural dimensions"
                            );
                            assert!(outer.shrink(16.0).contains_rect(content));
                        }
                    });
                    assert_eq!(
                        result.response.rect, outer,
                        "overflow must not expand the frame"
                    );
                });
            });
        }
    }

    #[test]
    fn sizing_spreadsheet_trims_only_trailing_empty_extents() {
        let payload = SpreadsheetPreview {
            sheets: vec![ottrin_preview::SpreadsheetSheet {
                id: "s".into(),
                name: "Sheet".into(),
                cells: [(0, 0, "first"), (3, 4, "last"), (10000, 10000, "")]
                    .into_iter()
                    .map(|(row, column, text)| ottrin_preview::SpreadsheetCell {
                        row,
                        column,
                        text: text.into(),
                    })
                    .collect(),
                omissions: vec![],
            }],
            omissions: vec![],
        };
        let grid = spreadsheet_grid_from_payload(&payload);
        assert_eq!(grid.sheets[0].columns.len(), 5);
        assert_eq!(grid.sheets[0].rows.len(), 4);
        assert_eq!(grid.sheets[0].rows[3].cells[4], "last");
        assert!(grid.sheets[0].rows[1].cells.iter().all(String::is_empty));
    }

    #[test]
    fn sizing_spreadsheet_actual_cells_fit_and_wrapped_rows_align() {
        for (columns, overview) in [(3, false), (20, true)] {
            let ctx = egui::Context::default();
            let preview = PreviewData {
                title: "sizing.xlsx".into(),
                kind: PreviewKind::OfficeMetadata,
                body: String::new(),
                document: None,
                open_external: false,
                spreadsheet: Some(SpreadsheetPreview {
                    sheets: vec![ottrin_preview::SpreadsheetSheet {
                        id: "sheet".into(),
                        name: "Sheet".into(),
                        cells: (0..columns)
                            .map(|column| ottrin_preview::SpreadsheetCell {
                                row: 0,
                                column,
                                text: if column == 0 {
                                    "long wrapped cell content ".repeat(8)
                                } else {
                                    "short".into()
                                },
                            })
                            .collect(),
                        omissions: vec![],
                    }],
                    omissions: vec![],
                }),
            };
            for _ in 0..3 {
                let mut viewport = Rect::NOTHING;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(440.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            viewport = ui.max_rect();
                            render_spreadsheet_body_preview(
                                ui,
                                Colors::for_config(&Default::default(), None),
                                "sizing.xlsx",
                                &preview,
                                &mut None,
                                PreviewBodySizing {
                                    spreadsheet_overview: overview,
                                    ..Default::default()
                                },
                            );
                        });
                    },
                );
                let cells: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect) if rect.stroke.width == 0.5 => Some(rect.rect),
                        _ => None,
                    })
                    .collect();
                assert_eq!(cells.len(), (columns + 1) * 2);
                assert!(
                    cells
                        .iter()
                        .all(|rect| rect.right() <= viewport.right() + 1.0),
                    "actual cell edges overflow {viewport:?}: {cells:?}"
                );
                let row = &cells[columns + 1..];
                assert!(
                    row.iter()
                        .all(|rect| (rect.height() - row[0].height()).abs() < 1.0),
                    "wrapped row backgrounds must align: {row:?}"
                );
            }
        }
    }

    #[test]
    fn preview_consistency_spreadsheet_paint_stays_inside_frame() {
        let ctx = egui::Context::default();
        let preview = PreviewData {
            title: "wide.xlsx".into(),
            kind: PreviewKind::OfficeMetadata,
            body: String::new(),
            document: None,
            open_external: true,
            spreadsheet: Some(SpreadsheetPreview {
                sheets: vec![ottrin_preview::SpreadsheetSheet {
                    id: "sheet1".into(),
                    name: "Wide sheet".into(),
                    cells: (0..20)
                        .map(|column| ottrin_preview::SpreadsheetCell {
                            row: 0,
                            column,
                            text: format!("column {column}"),
                        })
                        .collect(),
                    omissions: vec![],
                }],
                omissions: vec![],
            }),
        };
        for width in [180.0, 360.0, 720.0] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_spreadsheet_body_preview(
                            ui,
                            Colors::for_config(&Default::default(), None),
                            "wide.xlsx",
                            &preview,
                            &mut None,
                            PreviewBodySizing::default(),
                        );
                    });
                },
            );
            let frame = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::epaint::Shape::Rect(rect)
                        if rect.stroke.width == 1.0 && rect.fill == Color32::WHITE =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("spreadsheet frame");
            let mut cells = 0;
            for shape in &output.shapes {
                if let egui::epaint::Shape::Rect(rect) = &shape.shape
                    && rect.stroke.width == 0.5
                {
                    let painted = rect.rect.intersect(shape.clip_rect);
                    if painted.is_positive() {
                        cells += 1;
                        assert!(
                            frame.contains_rect(painted),
                            "cell {painted:?} escaped frame {frame:?}"
                        );
                    }
                }
            }
            assert!(cells > 0);
        }
    }

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
    fn spreadsheet_grid_does_not_report_a_visible_blank_column_as_omitted() {
        let preview = ottrin_preview::SpreadsheetPreview {
            sheets: vec![ottrin_preview::SpreadsheetSheet {
                id: "first".to_string(),
                name: "Quarter 1".to_string(),
                cells: vec![
                    ottrin_preview::SpreadsheetCell {
                        row: 0,
                        column: 0,
                        text: "Name".to_string(),
                    },
                    ottrin_preview::SpreadsheetCell {
                        row: 0,
                        column: 2,
                        text: "Score".to_string(),
                    },
                ],
                omissions: vec![ottrin_preview::SpreadsheetOmission {
                    reason: ottrin_preview::SpreadsheetOmissionReason::SparseColumnGap {
                        from: 1,
                        to: 1,
                    },
                }],
            }],
            omissions: vec![],
        };

        let grid = spreadsheet_grid_from_payload(&preview);

        assert_eq!(
            grid.sheets[0]
                .columns
                .iter()
                .map(|column| column.label.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        assert!(grid.sheets[0].omissions.is_empty());
    }

    #[test]
    fn spreadsheet_visuals_are_sheet_light_even_in_the_dark_application_theme() {
        let header = spreadsheet_cell_visual(SpreadsheetCellKind::Header, false);
        let body = spreadsheet_cell_visual(SpreadsheetCellKind::Body, false);
        let alternate_body = spreadsheet_cell_visual(SpreadsheetCellKind::Body, true);

        assert_eq!(header.fill, Color32::from_rgb(242, 244, 247));
        assert_eq!(body.fill, Color32::WHITE);
        assert_eq!(alternate_body.fill, Color32::from_rgb(250, 251, 252));
        assert_eq!(header.text, Color32::from_rgb(62, 68, 76));
        assert_eq!(body.text, Color32::from_rgb(30, 34, 40));
        assert_eq!(header.border, Color32::from_rgb(205, 210, 218));
        assert_eq!(body.border, header.border);
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
    fn docx_list_layout_uses_resolved_marker_and_level_indentation() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ottrin-preview/tests/fixtures/docx-lists.docx");
        let preview = ottrin_preview::load_preview(&ottrin_preview::PreviewRequest {
            path,
            kind_hint: ottrin_core::EntryKind::File,
        });
        let document = preview.document.expect("typed DOCX list structure");
        let DocxBlock::Paragraph(paragraph) = &document.blocks[1] else {
            panic!("second block must be a nested list paragraph");
        };

        assert_eq!(
            docx_list_layout(paragraph.list.as_ref().expect("list")),
            ("◦", 18.0)
        );
    }

    #[test]
    fn docx_baseline_renders_the_native_structured_body() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ottrin-preview/tests/fixtures/docx-baseline.docx");
        let preview = ottrin_preview::load_preview(&ottrin_preview::PreviewRequest {
            path,
            kind_hint: ottrin_core::EntryKind::File,
        });
        let context = egui::Context::default();
        let mut rendered = false;

        let output = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                rendered = render_docx_structured_body_preview(
                    ui,
                    Colors::for_config(&ottrin_core::AppConfig::default(), None),
                    "docx-baseline.docx",
                    &preview,
                    1.0,
                );
            });
        });

        assert!(rendered);
        assert!(
            output.shapes.len() >= 12,
            "the title, formatted runs, table cells, and alignment cues must produce real UI content"
        );
    }

    #[test]
    fn docx_inline_image_is_admitted_to_the_real_egui_texture_route() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ottrin-preview/tests/fixtures/docx-inline-images.docx");
        let preview = ottrin_preview::load_preview(&ottrin_preview::PreviewRequest {
            path,
            kind_hint: ottrin_core::EntryKind::File,
        });
        let png = preview
            .document
            .as_ref()
            .and_then(|document| document.blocks.first())
            .and_then(|block| match block {
                DocxBlock::Paragraph(paragraph) => {
                    paragraph.content.iter().find_map(|content| match content {
                        DocxParagraphContent::Image(image) => image.png.as_ref(),
                        DocxParagraphContent::Text(_) => None,
                    })
                }
                DocxBlock::Table(_) => None,
            })
            .expect("fixture image");
        let uri = format!("bytes://docx-inline/{:x}.png", Sha256::digest(png));
        let context = egui::Context::default();
        egui_extras::install_image_loaders(&context);

        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                assert!(render_docx_structured_body_preview(
                    ui,
                    Colors::for_config(&ottrin_core::AppConfig::default(), None),
                    "docx-inline-images.docx",
                    &preview,
                    1.0,
                ));
            });
        });

        context
            .try_load_texture(
                &uri,
                egui::TextureOptions::default(),
                egui::load::SizeHint::default(),
            )
            .expect("renderer must admit the inline image to egui's texture loaders");
    }

    #[test]
    fn document_extraction_is_truthfully_labeled_and_preserves_structure() {
        let body =
            "Filetype: DOCX document\n\nExtracted text:\nHeading\n\n  indented item\nline  two";
        let (_, extracted) = split_preview_extracted_text(body);

        assert_eq!(EXTRACTED_TEXT_FALLBACK_LABEL, "Extracted text fallback");
        assert_eq!(extracted, Some("Heading\n\n  indented item\nline  two"));
    }

    #[test]
    fn visual_sizing_policy_keeps_images_whole_and_documents_width_fitted() {
        assert_eq!(
            preview_sizing_for(PreviewKind::Image),
            PreviewSizingMode::FitPage
        );
        assert_eq!(
            preview_sizing_for(PreviewKind::Pdf),
            PreviewSizingMode::FitWidth
        );
        assert_eq!(
            preview_sizing_for(PreviewKind::OfficeMetadata),
            PreviewSizingMode::FitWidth
        );
        assert_eq!(
            fit_size(
                Vec2::new(1600.0, 400.0),
                Vec2::new(500.0, 300.0),
                PreviewSizingMode::FitPage
            ),
            Vec2::new(500.0, 125.0)
        );
        assert_eq!(
            fit_size(
                Vec2::new(160.0, 40.0),
                Vec2::new(500.0, 300.0),
                PreviewSizingMode::FitPage
            ),
            Vec2::new(160.0, 40.0)
        );
    }

    #[test]
    fn spreadsheet_columns_have_a_readable_floor_but_can_use_an_explicit_overview() {
        assert_eq!(
            spreadsheet_column_width(20.0, false),
            SPREADSHEET_MIN_COLUMN_WIDTH
        );
        assert_eq!(spreadsheet_column_width(20.0, true), 20.0);
        assert!(spreadsheet_column_width(240.0, false) >= SPREADSHEET_MIN_COLUMN_WIDTH);
    }
}
