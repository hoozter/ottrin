//! Read-only chooser consumer of the production preview pipeline.
use super::*;

struct Prepare {
    path: PathBuf,
    generation: u64,
    page: usize,
    cancelled: Arc<AtomicBool>,
    office_enabled: bool,
    cache_max_bytes: u64,
}
struct Prepared {
    generation: u64,
    data: PreviewData,
    visual: Option<PreviewJobRequest>,
}

pub(super) struct PickerPreview {
    prepare: Sender<Prepare>,
    prepared: Receiver<Prepared>,
    visual: Sender<PreviewJobRequest>,
    rendered: Receiver<PreviewJobResponse>,
    path: Option<PathBuf>,
    generation: u64,
    page: usize,
    cancelled: Arc<AtomicBool>,
    data: Option<PreviewData>,
    image: Option<ImagePreviewCacheEntry>,
    cache_key: String,
    office_render: Option<OfficeRenderedPage>,
    sheet: Option<String>,
}
impl PickerPreview {
    pub(super) fn new(ctx: &Context) -> Self {
        let repaint = ResultRepaint::default();
        repaint.bind(ctx);
        let (visual, rendered) = preview_jobs::spawn(repaint.clone());
        let (prepare, requests) = mpsc::channel::<Prepare>();
        let (completed, prepared) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(mut request) = requests.recv() {
                while let Ok(latest) = requests.try_recv() {
                    request = latest;
                }
                if request.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                let data = load_preview_cancellable(
                    &PreviewRequest {
                        path: request.path.clone(),
                        kind_hint: EntryKind::File,
                    },
                    &request.cancelled,
                );
                if request.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                let path = &request.path;
                let sqlite = ottrin_preview::is_sqlite_preview_candidate(path)
                    .then(|| ottrin_preview::sqlite_preview_source_generation(path))
                    .flatten();
                let obj = ottrin_preview::is_obj_path(path)
                    .then(|| ottrin_preview::obj_preview_source_generation(path))
                    .flatten();
                let ply = ottrin_preview::is_ply_path(path)
                    .then(|| ottrin_preview::ply_preview_source_generation(path))
                    .flatten();
                let office = ottrin_preview::office_format_for_path(path)
                    .and_then(|_| ottrin_preview::office_preview_source_generation(path));
                let mut provider = None;
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                if office.is_some()
                    && request.office_enabled
                    && !prefers_native_legacy_doc_renderer(path)
                {
                    provider = ottrin_preview::detect_installed_libreoffice(
                        &NativeOfficeIsolation,
                        &request.cancelled,
                    )
                    .ok()
                    .filter(|detected| {
                        ottrin_preview::office_format_for_path(path)
                            .is_some_and(|format| detected.capability.formats.contains(&format))
                    });
                }
                let video = (ottrin_preview::preview_profile(path).visual_renderer()
                    == ottrin_preview::PreviewVisualRenderer::Video)
                    .then(|| ottrin_preview::admit_video_thumbnail_source(path).ok())
                    .flatten();
                let target_dim = PREVIEW_CACHE_BUCKET_LARGE;
                let mut cache_key = preview_cache_key_for_page_with_generations(
                    path,
                    target_dim,
                    request.page,
                    sqlite.as_ref(),
                    obj.as_ref(),
                    ply.as_ref(),
                    office.as_ref(),
                );
                if let Some(detected) = &provider {
                    cache_key = office_provider_preview_cache_key(
                        target_dim,
                        request.page,
                        office.as_ref(),
                        &ottrin_preview::OfficeRenderCacheIdentity {
                            provider: detected.capability.id,
                            provider_version: detected.capability.version.clone(),
                            policy_version: 1,
                        },
                    );
                } else if let Some(source) = &video {
                    cache_key =
                        ottrin_preview::video_thumbnail_cache_key(source.generation(), target_dim);
                }
                if request.cancelled.load(Ordering::Acquire) {
                    continue;
                }
                let use_visual = preview_uses_visual_preview(path, data.kind)
                    || provider.is_some()
                    || quick_inspector_uses_rendered_office_page(path);
                let visual = PreviewJobRequest {
                    slot: PreviewRequestSlot::new(
                        path.clone(),
                        PreviewRequestTarget::Thumbnail,
                        target_dim,
                        request.page,
                    ),
                    path: path.clone(),
                    target_dim,
                    page_index: request.page,
                    cache_key,
                    sqlite_source_generation: sqlite,
                    obj_source_generation: obj,
                    ply_source_generation: ply,
                    office_source_generation: office,
                    office_provider: provider,
                    video_source: video,
                    cancellation: request.cancelled,
                    request_id: request.generation,
                    cache_max_bytes: request.cache_max_bytes,
                };
                repaint.send(
                    &completed,
                    Prepared {
                        generation: request.generation,
                        data,
                        visual: use_visual.then_some(visual),
                    },
                );
            }
        });
        Self {
            prepare,
            prepared,
            visual,
            rendered,
            path: None,
            generation: 0,
            page: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            data: None,
            image: None,
            cache_key: String::new(),
            office_render: None,
            sheet: None,
        }
    }
    #[cfg(test)]
    pub(super) fn has_image(&self) -> bool {
        self.image
            .as_ref()
            .and_then(ImagePreviewCacheEntry::bytes)
            .is_some()
    }
    pub(super) fn select(&mut self, path: Option<PathBuf>, config: &AppConfig) {
        if self.path == path {
            return;
        }
        self.path = path;
        self.page = 0;
        self.sheet = None;
        self.request(config);
    }
    fn request(&mut self, config: &AppConfig) {
        self.cancelled.store(true, Ordering::Release);
        self.cancelled = Arc::new(AtomicBool::new(false));
        self.generation = self.generation.wrapping_add(1);
        self.data = None;
        self.image = None;
        self.office_render = None;
        if let Some(path) = &self.path {
            let _ = self.prepare.send(Prepare {
                path: path.clone(),
                generation: self.generation,
                page: self.page,
                cancelled: self.cancelled.clone(),
                office_enabled: config.installed_office_previews_enabled,
                cache_max_bytes: u64::from(config.thumbnail_cache_max_mb) * 1024 * 1024,
            });
        }
    }
    pub(super) fn show(&mut self, ui: &mut egui::Ui, colors: Colors, config: &AppConfig) {
        while let Ok(result) = self.prepared.try_recv() {
            if result.generation == self.generation {
                self.data = Some(result.data);
                if let Some(visual) = result.visual {
                    self.cache_key.clone_from(&visual.cache_key);
                    if self.visual.send(visual).is_err() {
                        self.image = Some(ImagePreviewCacheEntry::from_response_with_reason(
                            None,
                            Some("The preview worker stopped.".into()),
                        ));
                    }
                }
            }
        }
        while let Ok(result) = self.rendered.try_recv() {
            if result.request_id == self.generation
                && self.path.as_ref() == Some(&result.path)
                && result.cache_key == self.cache_key
                && preview_response_matches_current_generation(&result)
            {
                self.office_render = result.office_render;
                self.image = Some(ImagePreviewCacheEntry::from_response_with_reason(
                    result.bytes,
                    result.error,
                ));
            }
        }
        ui.heading("Preview");
        let Some(path) = self.path.clone() else {
            ui.label("Select a file to preview.");
            return;
        };
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        ui.add(egui::Label::new(name.as_ref()).truncate())
            .on_hover_text(name.as_ref());
        let count = self
            .data
            .as_ref()
            .and_then(|data| preview_page_count(data, self.office_render.as_ref()));
        if let Some(count) = count.filter(|count| *count > 1) {
            let mut next = self.page;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.page > 0, egui::Button::new(MI_ARROW_BACK))
                    .clicked()
                {
                    next -= 1;
                }
                ui.label(format!("{} / {count}", self.page + 1));
                if ui
                    .add_enabled(self.page + 1 < count, egui::Button::new(MI_ARROW_FORWARD))
                    .clicked()
                {
                    next += 1;
                }
            });
            if next != self.page {
                self.page = next;
                self.request(config);
            }
        }
        let Some(data) = &self.data else {
            ui.spinner();
            return;
        };
        let size = ui.available_size().max(Vec2::ZERO);
        if let Some(bytes) = self.image.as_ref().and_then(ImagePreviewCacheEntry::bytes) {
            preview_render::preview_content_frame(
                ui,
                size,
                preview_render::PreviewScroll::WidthFittedPage,
                |ui| {
                    ui.add(preview_image_with_fit_bounds(
                        format!("bytes://preview/{}", self.cache_key),
                        bytes.to_vec(),
                        ui.available_size(),
                        colors.border_radius,
                    ));
                },
            );
        } else {
            preview_render::preview_content_frame(
                ui,
                size,
                preview_render::PreviewScroll::Body,
                |ui| {
                    if data.kind != PreviewKind::AudioMetadata
                        && preview_render::render_shared_preview_body(
                            ui,
                            colors,
                            &path,
                            &name,
                            data,
                            &mut self.sheet,
                            Default::default(),
                        )
                    {
                        return;
                    }
                    ScrollArea::both().show(ui, |ui| {
                        preview_render::render_metadata_card_preview(
                            ui,
                            colors,
                            MI_DESCRIPTION,
                            &name,
                            &data.body,
                        );
                        if let Some(reason) = self
                            .image
                            .as_ref()
                            .and_then(ImagePreviewCacheEntry::unavailable_reason)
                        {
                            ui.label(RichText::new(reason).small().color(colors.text_muted));
                        }
                    });
                },
            );
        }
    }
}
impl Drop for PickerPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
