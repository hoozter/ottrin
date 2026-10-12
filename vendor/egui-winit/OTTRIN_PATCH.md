# Ottrin input adapter patch

Source: egui-winit 0.33.3 from crates.io (MIT OR Apache-2.0). The source and
normalized Cargo manifest are vendored; dependencies remain registry packages.

Windows Shift+Delete emits its original pressed Key event immediately after
Cut. This preserves event-time modifiers and lets file views distinguish
permanent deletion from Ctrl+X. Text widgets retain native Cut; egui 0.33
TextEdit ignores Windows Shift+Delete Key events. Other clipboard shortcuts
and platforms are unchanged. Keep the pair ordering when updating the adapter.

Ottrin's file shortcut regression verifies the pair, rapid release, separate
Ctrl+X, and text/Inspector ownership. Native Windows package acceptance must
exercise actual key input, both confirmations, cancellation and destinations.

Original crates.io archive SHA-256: `ec6687e5bb551702f4ad10ac428bab12acf9d53047ebb1082d4a0ed8c6251a29`.
