# Ottrin 0.1.0 Release Notes

Ottrin 0.1.0 is the first daily-driver release candidate of the keyboard-first file manager.

## Highlights

- Miller, List, and Grid browsing with tabs, bookmarks, history, and Tandem View.
- Transactional copy publication with race-resistant overwrite, retained recovery data, and explicit partial-batch warnings. Move mutation is disabled pending a durable recovery design.
- Indexed search with durable SQLite generations, watcher updates, scoped fallback, and content snippets.
- Quick Inspector for text, images, bounded first-page PDF visuals with text/metadata fallback, DOCX content, XLSX/XLSM worksheet tables, PPTX slide text, archives, folders, audio metadata, and generic files.
- Linux Trash and Windows Recycle Bin support, plus scoped privileged-helper flows.
- Linux portable/AppImage, Windows portable, and a per-user Windows installer candidate with cleanup-aware uninstall.

## Important Limitations

- Damaged, encrypted, unsupported, or unrenderable PDFs fall back to extracted text and metadata without a visual page.
- Legacy binary Office files (`.doc`, `.xls`, `.ppt`) receive metadata previews rather than document content previews.
- Real Windows desktop, Windows UAC/helper, packaged Linux polkit, and full packaged-workflow smoke tests are release blockers until manually signed off.
- Full undo, tab drag-and-drop, media playback, PDF page navigation, and faithful Office layout rendering are not included. Linux `.deb`/`.rpm`, Windows MSI, and macOS packages are also deferred.

See [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md) for the required sign-off matrix.
