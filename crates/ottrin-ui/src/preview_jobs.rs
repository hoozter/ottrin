//! The single production visual-preview worker, shared by file-manager and picker consumers.
use super::*;

pub(super) fn spawn(
    repaint: ResultRepaint,
) -> (Sender<PreviewJobRequest>, Receiver<PreviewJobResponse>) {
    let (preview_req_tx, preview_req_rx) = mpsc::channel::<PreviewJobRequest>();
    let (preview_res_tx, preview_res_rx) = mpsc::channel::<PreviewJobResponse>();
    std::thread::spawn(move || {
        while let Ok(mut req) = preview_req_rx.recv() {
            if req.cancellation.load(Ordering::Acquire) {
                continue;
            }
            let sqlite_candidate = ottrin_preview::is_sqlite_preview_candidate(&req.path);
            let obj_candidate = ottrin_preview::is_obj_path(&req.path);
            let ply_candidate = ottrin_preview::is_ply_path(&req.path);
            let office_candidate = ottrin_preview::is_office_visual_path(&req.path);
            let native_legacy_doc_candidate =
                ottrin_preview::is_native_legacy_doc_visual_path(&req.path);
            let office_provider_candidate = req.office_provider.as_ref().is_some_and(|detected| {
                ottrin_preview::office_format_for_path(&req.path)
                    .is_some_and(|format| detected.capability.formats.contains(&format))
            });
            let video_candidate = ottrin_preview::preview_profile(&req.path).visual_renderer()
                == ottrin_preview::PreviewVisualRenderer::Video;
            let html_candidate = ottrin_preview::preview_profile(&req.path).visual_renderer()
                == ottrin_preview::PreviewVisualRenderer::Html;
            let eps_candidate = ottrin_preview::preview_profile(&req.path).visual_renderer()
                == ottrin_preview::PreviewVisualRenderer::Eps;
            let mut video_source_generation = req
                .video_source
                .as_ref()
                .map(|source| source.generation().clone());
            let generation_is_current = if office_provider_candidate {
                let detected = req.office_provider.as_ref().expect("checked above");
                let identity = ottrin_preview::OfficeRenderCacheIdentity {
                    provider: detected.capability.id,
                    provider_version: detected.capability.version.clone(),
                    policy_version: 1,
                };
                req.cache_key
                    == office_provider_preview_cache_key(
                        req.target_dim,
                        req.page_index,
                        req.office_source_generation.as_ref(),
                        &identity,
                    )
            } else if video_candidate {
                req.video_source.as_ref().is_none_or(|source| {
                    req.cache_key
                        == ottrin_preview::video_thumbnail_cache_key(
                            source.generation(),
                            req.target_dim,
                        )
                })
            } else {
                req.cache_key
                    == preview_cache_key_for_page_with_generations(
                        &req.path,
                        req.target_dim,
                        req.page_index,
                        req.sqlite_source_generation.as_ref(),
                        req.obj_source_generation.as_ref(),
                        req.ply_source_generation.as_ref(),
                        req.office_source_generation.as_ref(),
                    )
            };
            let selected_pdf_page = req.page_index > 0
                && matches!(ottrin_preview::classify_path(&req.path), PreviewKind::Pdf);
            let mut video_error = None;
            #[cfg(target_os = "linux")]
            let mut html_error = None::<String>;
            #[cfg(not(target_os = "linux"))]
            let html_error = None::<String>;
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let mut eps_error = None::<String>;
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            let eps_error = None::<String>;
            let mut office_error = None;
            let mut office_render = None;
            let bytes = if !generation_is_current
                || (sqlite_candidate
                    && (req.sqlite_source_generation.is_none()
                        || req.sqlite_source_generation
                            != ottrin_preview::sqlite_preview_source_generation(&req.path)))
                || (obj_candidate
                    && (req.obj_source_generation.is_none()
                        || req.obj_source_generation
                            != ottrin_preview::obj_preview_source_generation(&req.path)))
                || (ply_candidate
                    && (req.ply_source_generation.is_none()
                        || req.ply_source_generation
                            != ottrin_preview::ply_preview_source_generation(&req.path)))
                || (office_candidate
                    && (req.office_source_generation.is_none()
                        || req.office_source_generation
                            != ottrin_preview::office_preview_source_generation(&req.path)))
            {
                None
            } else if office_provider_candidate {
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                {
                    let detected = req.office_provider.as_ref().expect("checked above");
                    let format =
                        ottrin_preview::office_format_for_path(&req.path).expect("checked above");
                    let render_dimension = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                        PREVIEW_CACHE_BUCKET_XL
                    } else {
                        req.target_dim
                    };
                    let unique = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos();
                    let output_dir = std::env::temp_dir().join(format!(
                        "ottrin-office-preview-{}-{unique}",
                        std::process::id()
                    ));
                    let rendered = std::fs::create_dir(&output_dir)
                        .map_err(|error| error.to_string())
                        .and_then(|()| {
                            let isolation = NativeOfficeIsolation;
                            let converter = ottrin_preview::InstalledLibreOfficeConverter::new(
                                detected.executable.clone(),
                                &isolation,
                            );
                            let request = ottrin_preview::OfficeRenderRequest {
                                source_path: req.path.clone(),
                                format,
                                private_output_dir: output_dir.clone(),
                                policy: ottrin_preview::OfficeRenderPolicy {
                                    version: 1,
                                    max_input_bytes: 256 * 1024 * 1024,
                                    max_output_bytes: 512 * 1024 * 1024,
                                    max_pages: 1000,
                                    timeout: Duration::from_secs(60),
                                },
                            };
                            ottrin_preview::render_office_preview_page(
                                std::slice::from_ref(&detected.capability),
                                &request,
                                &converter,
                                req.page_index,
                                render_dimension,
                                req.cancellation.as_ref(),
                                |pdf, page, dimension| {
                                    ottrin_preview::render_pdf_page_png(pdf, page, dimension)
                                        .map_err(|error| error.to_string())
                                },
                            )
                            .map_err(|error| error.to_string())
                        });
                    let _ = std::fs::remove_dir_all(&output_dir);
                    match rendered {
                        Ok(rendered) => {
                            office_render = Some(OfficeRenderedPage {
                                source_path: req.path.clone(),
                                source_generation: req
                                    .office_source_generation
                                    .clone()
                                    .expect("provider requests require a source generation"),
                                label: rendered.provider_label,
                                page_count: rendered.page_count,
                                cache_identity: Some(rendered.cache_identity),
                            });
                            Some(rendered.png)
                        }
                        Err(error) => {
                            office_error = Some(error);
                            None
                        }
                    }
                }
                #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                {
                    None
                }
            } else if video_candidate {
                if req.target_dim != PREVIEW_CACHE_ORIGINAL
                    && let Some(cached) = load_cached_video_thumbnail(&req.cache_key)
                {
                    Some(cached)
                } else if let Some(source) = req.video_source.take() {
                    let decode_dimension = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                        PREVIEW_CACHE_BUCKET_XL
                    } else {
                        req.target_dim
                    };
                    match ottrin_preview::render_video_thumbnail_png(
                        source,
                        decode_dimension,
                        Arc::clone(&req.cancellation),
                    ) {
                        Ok(rendered) => {
                            video_source_generation = Some(rendered.source_generation);
                            if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                                persist_cached_video_thumbnail(
                                    &req.cache_key,
                                    &rendered.png,
                                    req.cache_max_bytes,
                                );
                            }
                            Some(rendered.png)
                        }
                        Err(error) => {
                            video_error = Some(error.fallback_message().to_string());
                            None
                        }
                    }
                } else {
                    video_error = Some(
                        ottrin_preview::VideoThumbnailError::AdmissionFailed
                            .fallback_message()
                            .to_string(),
                    );
                    None
                }
            } else if sqlite_candidate {
                if req.target_dim != PREVIEW_CACHE_ORIGINAL
                    && let Some(cached) = load_cached_thumbnail(
                        &req.path,
                        req.target_dim,
                        req.page_index,
                        &req.cache_key,
                    )
                {
                    repaint.send(
                        &preview_res_tx,
                        PreviewJobResponse {
                            slot: req.slot,
                            path: req.path,
                            target_dim: req.target_dim,
                            page_index: req.page_index,
                            cache_key: req.cache_key,
                            sqlite_source_generation: req.sqlite_source_generation,
                            obj_source_generation: req.obj_source_generation,
                            ply_source_generation: req.ply_source_generation,
                            office_source_generation: req.office_source_generation,
                            video_source_generation: None,
                            office_render: None,
                            request_id: req.request_id,
                            bytes: Some(cached),
                            error: None,
                        },
                    );
                    continue;
                }
                let rendered = ottrin_preview::render_sqlite_metadata_preview_png_with_generation(
                    &req.path,
                    if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                        PREVIEW_CACHE_BUCKET_XL
                    } else {
                        req.target_dim
                    },
                );
                match rendered {
                    Ok(rendered)
                        if Some(&rendered.source_generation)
                            == req.sqlite_source_generation.as_ref() =>
                    {
                        if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                            persist_cached_thumbnail(
                                &req.path,
                                req.target_dim,
                                req.page_index,
                                &req.cache_key,
                                &rendered.png,
                                req.cache_max_bytes,
                            );
                        }
                        Some(rendered.png)
                    }
                    _ => None,
                }
            } else if obj_candidate {
                if req.target_dim != PREVIEW_CACHE_ORIGINAL
                    && let Some(cached) = load_cached_thumbnail(
                        &req.path,
                        req.target_dim,
                        req.page_index,
                        &req.cache_key,
                    )
                {
                    repaint.send(
                        &preview_res_tx,
                        PreviewJobResponse {
                            slot: req.slot,
                            path: req.path,
                            target_dim: req.target_dim,
                            page_index: req.page_index,
                            cache_key: req.cache_key,
                            sqlite_source_generation: req.sqlite_source_generation,
                            obj_source_generation: req.obj_source_generation,
                            ply_source_generation: req.ply_source_generation,
                            office_source_generation: req.office_source_generation,
                            video_source_generation: None,
                            office_render: None,
                            request_id: req.request_id,
                            bytes: Some(cached),
                            error: None,
                        },
                    );
                    continue;
                }
                match ottrin_preview::render_obj_preview_png_with_generation(
                    &req.path,
                    if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                        PREVIEW_CACHE_BUCKET_XL
                    } else {
                        req.target_dim
                    },
                ) {
                    Ok(rendered)
                        if Some(&rendered.source_generation)
                            == req.obj_source_generation.as_ref() =>
                    {
                        if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                            persist_cached_thumbnail(
                                &req.path,
                                req.target_dim,
                                req.page_index,
                                &req.cache_key,
                                &rendered.png,
                                req.cache_max_bytes,
                            );
                        }
                        Some(rendered.png)
                    }
                    _ => None,
                }
            } else if ply_candidate {
                if req.target_dim != PREVIEW_CACHE_ORIGINAL
                    && let Some(cached) = load_cached_thumbnail(
                        &req.path,
                        req.target_dim,
                        req.page_index,
                        &req.cache_key,
                    )
                {
                    repaint.send(
                        &preview_res_tx,
                        PreviewJobResponse {
                            slot: req.slot,
                            path: req.path,
                            target_dim: req.target_dim,
                            page_index: req.page_index,
                            cache_key: req.cache_key,
                            sqlite_source_generation: req.sqlite_source_generation,
                            obj_source_generation: req.obj_source_generation,
                            ply_source_generation: req.ply_source_generation,
                            office_source_generation: req.office_source_generation,
                            video_source_generation: None,
                            office_render: None,
                            request_id: req.request_id,
                            bytes: Some(cached),
                            error: None,
                        },
                    );
                    continue;
                }
                match ottrin_preview::render_ply_preview_png_with_generation(
                    &req.path,
                    if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                        PREVIEW_CACHE_BUCKET_XL
                    } else {
                        req.target_dim
                    },
                ) {
                    Ok(rendered)
                        if Some(&rendered.source_generation)
                            == req.ply_source_generation.as_ref() =>
                    {
                        if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                            persist_cached_thumbnail(
                                &req.path,
                                req.target_dim,
                                req.page_index,
                                &req.cache_key,
                                &rendered.png,
                                req.cache_max_bytes,
                            );
                        }
                        Some(rendered.png)
                    }
                    _ => None,
                }
            } else if office_candidate {
                if !native_legacy_doc_candidate
                    && req.target_dim != PREVIEW_CACHE_ORIGINAL
                    && let Some(cached) = load_cached_thumbnail(
                        &req.path,
                        req.target_dim,
                        req.page_index,
                        &req.cache_key,
                    )
                {
                    repaint.send(
                        &preview_res_tx,
                        PreviewJobResponse {
                            slot: req.slot,
                            path: req.path,
                            target_dim: req.target_dim,
                            page_index: req.page_index,
                            cache_key: req.cache_key,
                            sqlite_source_generation: req.sqlite_source_generation,
                            obj_source_generation: req.obj_source_generation,
                            ply_source_generation: req.ply_source_generation,
                            office_source_generation: req.office_source_generation,
                            video_source_generation: None,
                            office_render: None,
                            request_id: req.request_id,
                            bytes: Some(cached),
                            error: None,
                        },
                    );
                    continue;
                }
                let render_dimension = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                    PREVIEW_CACHE_BUCKET_XL
                } else {
                    req.target_dim
                };
                if native_legacy_doc_candidate {
                    match ottrin_preview::render_legacy_doc_pdf_with_generation(&req.path) {
                        Ok(rendered)
                            if Some(&rendered.source_generation)
                                == req.office_source_generation.as_ref() =>
                        {
                            match ottrin_preview::render_pdf_bytes_page_png(
                                &rendered.pdf,
                                req.page_index,
                                render_dimension,
                            ) {
                                Ok(png) => {
                                    office_render = Some(OfficeRenderedPage {
                                        source_path: req.path.clone(),
                                        source_generation: rendered.source_generation,
                                        label: "Rendered by Ottrin — native legacy Word preview"
                                            .to_string(),
                                        page_count: rendered.page_count,
                                        cache_identity: None,
                                    });
                                    Some(png)
                                }
                                Err(error) => {
                                    office_error = Some(error.to_string());
                                    None
                                }
                            }
                        }
                        Ok(_) => None,
                        Err(error) => {
                            office_error = Some(error.to_string());
                            None
                        }
                    }
                } else {
                    match ottrin_preview::render_office_content_preview_png_with_generation(
                        &req.path,
                        render_dimension,
                    ) {
                        Ok(rendered)
                            if Some(&rendered.source_generation)
                                == req.office_source_generation.as_ref() =>
                        {
                            if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                                persist_cached_thumbnail(
                                    &req.path,
                                    req.target_dim,
                                    req.page_index,
                                    &req.cache_key,
                                    &rendered.png,
                                    req.cache_max_bytes,
                                );
                            }
                            Some(rendered.png)
                        }
                        _ => None,
                    }
                }
            } else if html_candidate {
                #[cfg(target_os = "linux")]
                {
                    let persistent_cache_allowed = visual_renderer_uses_persistent_thumbnail_cache(
                        ottrin_preview::PreviewVisualRenderer::Html,
                    );
                    if persistent_cache_allowed
                        && req.target_dim != PREVIEW_CACHE_ORIGINAL
                        && let Some(cached) = load_cached_thumbnail(
                            &req.path,
                            req.target_dim,
                            req.page_index,
                            &req.cache_key,
                        )
                    {
                        Some(cached)
                    } else {
                        let render_dimension = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                            PREVIEW_CACHE_BUCKET_XL
                        } else {
                            req.target_dim
                        };
                        match ottrin_preview::render_html_content_preview_png(
                            &req.path,
                            render_dimension,
                        ) {
                            Ok(bytes) => {
                                if persistent_cache_allowed
                                    && req.target_dim != PREVIEW_CACHE_ORIGINAL
                                {
                                    persist_cached_thumbnail(
                                        &req.path,
                                        req.target_dim,
                                        req.page_index,
                                        &req.cache_key,
                                        &bytes,
                                        req.cache_max_bytes,
                                    );
                                }
                                Some(bytes)
                            }
                            Err(error) => {
                                html_error = Some(error.to_string());
                                None
                            }
                        }
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    None
                }
            } else if eps_candidate {
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                {
                    if req.target_dim != PREVIEW_CACHE_ORIGINAL
                        && let Some(cached) = load_cached_thumbnail(
                            &req.path,
                            req.target_dim,
                            req.page_index,
                            &req.cache_key,
                        )
                    {
                        Some(cached)
                    } else {
                        let render_dimension = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                            PREVIEW_CACHE_BUCKET_XL
                        } else {
                            req.target_dim
                        };
                        match ottrin_preview::render_eps_content_preview_png(
                            &req.path,
                            render_dimension,
                        ) {
                            Ok(bytes) => {
                                if req.target_dim != PREVIEW_CACHE_ORIGINAL {
                                    persist_cached_thumbnail(
                                        &req.path,
                                        req.target_dim,
                                        req.page_index,
                                        &req.cache_key,
                                        &bytes,
                                        req.cache_max_bytes,
                                    );
                                }
                                Some(bytes)
                            }
                            Err(error) => {
                                eps_error = Some(format!(
                                    "EPS preview unavailable ({error:?}). Open externally to view this file."
                                ));
                                None
                            }
                        }
                    }
                }
                #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                {
                    None
                }
            } else if req.target_dim == PREVIEW_CACHE_ORIGINAL || selected_pdf_page {
                full_preview_bytes_at_page(&req.path, PREVIEW_CACHE_BUCKET_XL, req.page_index)
            } else {
                let cached = load_cached_thumbnail(
                    &req.path,
                    req.target_dim,
                    req.page_index,
                    &req.cache_key,
                );
                thumbnail_bytes_from_cache_or_build(&req.path, req.target_dim, cached).map(
                    |(bytes, should_persist)| {
                        if should_persist {
                            persist_cached_thumbnail(
                                &req.path,
                                req.target_dim,
                                req.page_index,
                                &req.cache_key,
                                &bytes,
                                req.cache_max_bytes,
                            );
                        }
                        bytes
                    },
                )
            };
            let error = if office_error.is_some() {
                office_error
            } else if video_error.is_some() {
                video_error
            } else if html_error.is_some() {
                html_error
            } else if eps_error.is_some() {
                eps_error
            } else if bytes.is_none() && ottrin_preview::is_svg_path(&req.path) {
                let target_dim = if req.target_dim == PREVIEW_CACHE_ORIGINAL {
                    PREVIEW_CACHE_BUCKET_XL
                } else {
                    req.target_dim
                };
                ottrin_preview::render_svg_content_preview_png(&req.path, target_dim)
                    .err()
                    .map(|error| error.to_string())
            } else {
                None
            };
            repaint.send(
                &preview_res_tx,
                PreviewJobResponse {
                    slot: req.slot,
                    path: req.path,
                    target_dim: req.target_dim,
                    page_index: req.page_index,
                    cache_key: req.cache_key,
                    sqlite_source_generation: req.sqlite_source_generation,
                    obj_source_generation: req.obj_source_generation,
                    ply_source_generation: req.ply_source_generation,
                    office_source_generation: req.office_source_generation,
                    video_source_generation,
                    office_render,
                    request_id: req.request_id,
                    bytes,
                    error,
                },
            );
        }
    });
    (preview_req_tx, preview_res_rx)
}
