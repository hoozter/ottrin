# Ottrin 0.1.0 Release Notes

Ottrin 0.1.0 is the first daily-driver release candidate of the keyboard-first file manager.

## Highlights

- Miller, List, and Grid browsing with tabs, bookmarks, history, and Tandem View.
- Transactional copy publication with race-resistant overwrite, retained recovery data, and explicit partial-batch warnings. Linux same-filesystem Move is enabled for test-covered Skip/Rename/Overwrite/Cancel with explicit dispositions and conflict handling. Cross-filesystem and Windows Move remain disabled without mutation pending a separately accepted protocol.
- Indexed search with durable SQLite generations, watcher updates, scoped fallback, and content snippets.
- Quick Inspector for text, images, bounded multi-page PDF visuals with Previous/Next/page selection, bounded DOCX/XLSX/XLSM content-preview visuals, PPTX slide text, archives, folders, audio metadata, and generic files. PDF and Office previews retain extracted-text and metadata fallbacks.
- Best-effort embedded Affinity preview for `.afphoto` and `.afdesign`: it uses only a bounded validated embedded PNG and is labeled as potentially different from the saved canvas; `.afpub` receives the same bounded classification and safe metadata fallback, while public visual coverage remains unclaimed pending a legal fixture.
- Linux Trash and Windows Recycle Bin support, plus scoped privileged-helper flows.
- Packaging workflows target Linux portable/AppImage, Windows portable, and a per-user Windows installer with cleanup-aware uninstall; exact-current-commit artifacts are not yet accepted.

## Important Limitations

- Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without a visual page.
- Damaged, oversized, unsupported, or unrenderable DOCX/XLSX/XLSM packages fall back to extracted text and metadata. Their visual output is a bounded content preview, not faithful Office layout.
- Legacy binary Office files (`.doc`, `.xls`, `.ppt`) receive metadata previews rather than document content previews.
- Legacy retained Move records have source-level startup/batch discovery and
  manual-review surfaces. They do not back accepted Linux same-filesystem Move,
  and no packaged or runtime recovery capability is claimed.
- Indexed FTS5 negated content search is not an accepted/supported behavior; positive indexed content search remains the supported path.
- Windows packaging is designed to use WGPU when Automatic finds no compatibility signal and to carry the checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime, including app-local `opengl32.dll` and `libgallium_wgl.dll`. Automatic selection, persisted preference, explicit overrides, notices, and loaded-DLL provenance remain release gates for the exact rebuilt portable ZIP and installer.
- Linux and Windows CI, real Windows desktop and UAC/helper, packaged Linux polkit, and full packaged-workflow smoke tests are release blockers until manually signed off against the exact release commit and artifacts. Windows sign-off must cover the portable ZIP and per-user installer on the native WGPU default and Automatic compatibility paths, the persisted preference, and the explicit `--renderer opengl` / `OTTRIN_RENDERER=opengl` recovery path where applicable, including UAC/helper flows.
- Full undo, media playback, and faithful Office layout rendering are not included. Tab drag-and-drop and grid rubber-band selection are implemented and source-tested, but accepted release behavior is pending manual and packaged validation. Linux `.deb`/`.rpm`, Windows MSI, and macOS packages are deferred. Code signing is not included in this release.

See [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md) for the required sign-off matrix.
