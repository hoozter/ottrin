# Current delivered Linux browser integration

The AppImage built from clean `928b7937bb5bf7d5fa0fbb5dafd70ae4d4ef41ee`,
SHA-256 `af45a0acf9d1baa04f8105c95a00ef6540782243e4bbe65579bffb1f0ebdfeb5`,
passes actual installed Firefox and installed Flatpak Falkon Open/Save.
`INPUTS.json` binds the final two cases to frozen shared producer inputs.

Both native dialogs belong to the delivered Ottrin executable. The browser
visibly opens the chosen local document and saves the exact original bytes.
Source and saved SHA-256 are both
`d33cc86d0fcf836c80ee17a691eecb202f4f85240b50754202aba67e8a8af6c6`.
The actual backend OpenFile and SaveFile calls carry `firefox` or
`org.kde.falkon`, respectively. The latter is observed through the real installed
Flatpak's metadata, rather than an invented sandbox identity.

The Flatpak shell first refuses startup if it can see the outside fixture.
Only its private browser-data directory is explicitly granted. To accommodate
the private session's nonstandard temporary runtime URL prefix, a sandbox-local
symlink points that prefix to Flatpak's existing `/run/flatpak/doc` per-app
projection. It adds no source-folder grant. This is an isolated X11/private
bus/profile check; normal native setup and distribution lifecycle were tested
separately. Physical-desktop behavior and other browsers remain separate scopes.
The private first-run wallet wizard is declined using its normal Cancel button;
no wallet, persistent override or personal browser configuration is changed.

Fourteen preceding local harness attempts are rejected. They exposed duplicate
session-bus/document mounts, first-run wallet setup, input focus, filter selection,
nonstandard document paths and an inappropriate unsandboxed-only log assertion.
Their terminal results/teardown remain in `REJECTED_ATTEMPTS.json`; redundant
screenshots/logs are removed. The successful final flow needs no loopback server.
Earlier root Firefox acceptance retains its own frozen FIREFOX inputs.

`FOLLOWUP.json` records a real remaining product defect: chooser keyboard input
can reach the folder view beneath the File type popup. Pointer filter selection
passes here; keyboard accessibility is not accepted by these browser results.

Every completed private profile/process/display/bus/mount and transient scope is
removed. No index starts in the accepted sessions. `CLEANUP.json` records exact
remaining evidence storage; this checkpoint adds no packages or compiler output.
