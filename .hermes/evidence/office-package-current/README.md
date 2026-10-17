# Exact Office package checkpoint

Start with ACCEPTANCE.json. Product packages use clean a6dc090; CANCEL_BINARY.json
identifies the later test-only readiness correction. NATIVE_BOUNDARY_RESULTS.json
retains the initial five-pass/one-failed-probe outcome; CANCEL_RECHECK.json and
COLD_CANCEL_RESULTS.json preserve the subsequent warm and corrected cold runs.
No initial failure is overwritten.

PACKAGED_UI_RESULTS.json and the four main/inspector PNGs cover the full Windows
portable bundle under explicit OpenGL, including injected provider failure.
LAYOUT_PIXEL_COMPARISON.json binds the five page images to prior reviewed pixels.
MSI_PAYLOAD.json is administrative extraction, not installed lifecycle acceptance.

linux-light100 was an incomplete QA attempt: the operator requested a cleared
state immediately after Undo, before explicitly clearing retained Copy history.
The harness cleaned its resources. linux-light100-final completes the correct
Cancel, Undo, explicit Clear and normal close workflow. Its RESULTS.json is the
accepted AppImage GUI result. Physical Wayland remains outside this scope.

PACKAGE_TEARDOWN.json records removal of the temporary Windows Office task and
owned compiler/payload extractions, original-state checks and VM shutdown.
RAW_RECEIPT_IDENTITIES.json preserves byte hashes before ANSI/BOM/CRLF receipt
normalization to UTF-8 LF. Disposable uploaded executable/zip copies and the
now-published Linux candidate copies were removed after their consumers exited.

Text diagnostics and extracted text are normalized for repository whitespace;
raw byte identities are retained before normalization. Pixel/PDF bytes are intact.
