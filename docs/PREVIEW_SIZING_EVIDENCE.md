# Preview sizing landing evidence

2026-09-10 — task `t_e930fff1`. This is a source landing, not release acceptance or a new package.

## Scope and candidate

The landing contains the reviewed sizing changes in `crates/ottrin-ui/src/lib.rs`, `crates/ottrin-ui/src/preview_render.rs`, `scripts/linux-desktop-qa.py` and `scripts/tests/test_linux_desktop_qa.py`, plus this evidence and the roadmap entry. No new product behavior was implemented during landing.

Fit and manual modes share the original raster; the actual submitted image rectangle supplies effective zoom. Main preview fitting uses the padded viewport and measured wrapping toolbar. Inspector sizing controls have a separate wrapping row. Fit-width PDFs use continuous following-page rendering. Text and structured DOCX support font scaling; spreadsheets use measured equal-height rows, readable ordinary widths, explicit Overview and trailing-empty-extent trimming. Superseded thumbnail/manual branching, SVG sizing branches and uneven spreadsheet Grid allocation were removed by the implementation.

The stable implementation handoff is `.hermes/preview-sizing-astra-report.md`; its detailed evidence is under `.hermes/preview-sizing/astra/`. These local evidence files are not release payloads. Before staging, the complete tracked working diff matched the handoff byte-for-byte:

- Base: `45fe5ca00f973d0f75076e8dcb9dc1f83cab0583`.
- Full dirty candidate SHA-256: `f4f7f41a6bc9491bf67cbfb226ac5b72c402f1a93aea40529e66e9b53190ff7d`.
- Preview-only source patch SHA-256: `c8d330654dfdd85dee835168409c8c88c055e229a23895b2ea9c871c789403db`.

The full candidate includes unrelated inherited work and is not the commit scope. Seven UI hunks for Settings startup and legacy DOC provider selection were excluded from staging and left unchanged in the working tree. Dirty packaging/lifecycle and `USER_GUIDE.md` edits were also left untouched. The previous implementation owner had exited before landing began; no competing build owner was found.

## Fresh landing verification

Run under private HOME/XDG directories, D-Bus and Xvfb with the shared primary-checkout Cargo output. Exact commands and results are in `.hermes/preview-sizing/astra/landing-focused-gates.log`:

| Command | Result |
| --- | --- |
| `cargo test --offline -p ottrin-ui --lib sizing_` | 9 passed |
| `cargo test --offline -p ottrin-ui --lib preview_render::tests` | 22 passed |
| `cargo test --offline -p ottrin-ui --lib pdf_fit_` | 1 passed |
| `/usr/bin/python3 -B -m unittest discover -s scripts/tests -p test_linux_desktop_qa.py` | 29 passed |
| `cargo clippy --offline -p ottrin-ui --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` | Passed |

The Rust filters overlap; these counts are not a sum of unique tests. Tests and Clippy exercised the preserved working candidate, including the two unrelated UI corrections, not a clean checkout of the narrower staged commit. No all-workspace or all-platform claim follows from these focused checks. Original RED evidence remains in the implementation handoff: original-raster fit, spreadsheet geometry/extents, continuous-page painting and toolbar hit-rectangle overlap failed before their corresponding corrections.

## Existing package: identity only rechecked during landing

The existing `dist/packages/Ottrin-0.1.0-x86_64.AppImage` still hashes to `9edabfc314691ca1a00044a6a4172a8b07a51e953a007ad33a3702a73922e6d6`. Its provenance identifies the base revision plus the complete dirty candidate above. It was not rebuilt, relabelled as this landing commit, repackaged or published.

The implementation handoff records native select → Space Inspector captures for seven fixture types at two verified sizes, plus manual zoom and spreadsheet Overview interactions; it also records matching executable hashes and successful AppImage `--help`. This landing does not claim a fresh desktop run or independently renewed visual acceptance.

## Remaining acceptance boundaries

- The earlier full UI run was **287 passed, 1 failed**, not all green. `thumbnail_cache::tests::eps_rejects_stale_raster_cache_and_uses_only_the_isolated_renderer` failed with the private session's unavailable systemd user manager. This test was not rerun against the live user's service manager or silently skipped to claim a full-suite pass. Resume that gate in a disposable session with a working user manager.
- Native editing integration remains unverified: `TextEditSession::open` returned `UnsupportedMetadata` for disposable fixtures. Rendering tests prove text scale/wrapping, not native save acceptance. Resume on a disposable filesystem satisfying the existing metadata contract; do not weaken the guard.
- AccessKit/AT-SPI remains disabled by the unchanged manifest because of the documented Unix D-Bus lifecycle failure. Native screenshots are not accessibility acceptance.
- Main preview has egui clip/control/resize evidence, not exhaustive packaged main-pane acceptance for every document type. Standalone `preview PATH` and Windows native acceptance are not established by the Inspector captures.

These boundaries remain in final QA; they do not require reopening unrelated implementation during this bounded source landing.

## Miller resize correction — 2026-10-12

Native Windows QA of the clean `cf40e58` portable package exposed an adjacent preview disappearing when a running window shrank from 1100 to 800 pixels; launching at the saved 800-pixel width worked. The production renderer now reveals the actual selected-column/preview bounds when shrinking a viewport whose preview was visible. Deliberate horizontal scrolling remains authoritative. Preview width uses the real viewport edge rather than the painter clip, which includes shadow bleed.

The rendered regression covers startup, shrinking, complete preview visibility, vertical keyboard navigation without horizontal movement, actual horizontal wheel input and a later resize preserving that manual position. Linux UI checks pass: 344 tests, one ignored, strict all-target Clippy, formatting and diff validation. This is source verification; rebuilt package and native resize acceptance are pending. The earlier package screenshots and clean native Windows exit are retained in `.hermes/evidence/windows-refresh-20261012/`.

The exact AppImage from clean `903c55395691828cbde81f889a8a5b846c0eebb4` (SHA-256 `4a6a0bdadeca8d37bbe9cb079dec735e20dd7510ea303188feabf3aa12a08748`) passes private native Linux Dark/Light resizing from 1100 × 690 to 800 × 600, visible adjacent text preview, vertical selection and clean exit. All five packaged executables match workspace release output. Owned apps, daemon, bus, display, profile and extracts were removed. `dist/packages/MILLER_RESIZE_ACCEPTANCE.json` records this focused scope. Native Windows resize and other preview types remain pending.
