# Show HN: Ottrin – a keyboard-first file manager built in Rust (egui)

> **Draft** — post to https://news.ycombinator.com/submit when ready.

---

**Title:** Show HN: Ottrin – a keyboard-first file manager built in Rust (egui)

**URL:** https://github.com/hoozter/ottrin

---

**Post body:**

I got tired of file managers that are either too bloated (Nautilus on a slow machine) or too minimal (no previews, no tabs). So I built Ottrin.

It's a native file manager written in Rust using egui/eframe. Miller columns are the default navigation model — same idea as macOS Finder's column view, but keyboard-first. Left/right arrows move between columns, Enter opens, Backspace goes up. No mouse needed.

**What it does:**
- Miller columns with smooth horizontal scroll (parent → current → preview side-by-side)
- List and grid views switchable per-tab
- Integrated command frame — type any printable key to get a shell-like command bar; supports mkdir, mv, cp, rm, chmod, ln -s
- Indexed search with scope switching (home, current folder, everywhere)
- File previews for text, images, PDF, audio/video metadata, archives — hit Space
- Tabs with per-tab navigation history
- Bookmarks bar (right-click to add/remove)
- Drop folder for quick copy/move targets
- Theme system with 7 built-in presets and live editing
- Privilege escalation helper for root-owned paths (polkit on Linux, UAC on Windows)

**What it doesn't do yet (v0.1.0):**
- Full-text content search (filenames only right now)
- Multi-select batch rename
- VCS status integration
- Plugin system

Pre-built binaries for Linux (AppImage), macOS (Universal DMG), and Windows (MSI) are in the releases. Source is MIT.

Feedback welcome — especially on the Miller columns UX and whether the command frame is intuitive or confusing.

---

## HN submission notes

- Post on a weekday morning (9–11am US Eastern) for best visibility.
- "Show HN" prefix is required for project launches — keep it in the title.
- The title should stay factual; avoid adjectives like "blazing-fast" or "amazing".
- Link directly to the GitHub repo, not a landing page, for Show HN posts.
- Be present to answer questions in the first hour — response time matters for ranking.

## Anticipated questions

**Q: Why not use a TUI (like ranger or lf)?**
A: Previews, drag-and-drop targets, and theme rendering are cleaner in a GUI. egui gives native performance without a browser runtime.

**Q: How does it compare to Thunar / Dolphin / Nautilus?**
A: Lighter than Dolphin, more keyboard-driven than Nautilus, richer preview system than Thunar. Main differentiator is the Miller columns model and the integrated command frame.

**Q: Why egui and not GTK/Qt?**
A: Single binary, cross-platform with minimal system dependencies, no GLib/Qt runtime to deal with. The immediate-mode model is also very natural for this kind of interactive UI.

**Q: Is it production-ready?**
A: v0.1.0 is functional for daily use on Linux. macOS and Windows builds exist but have had less testing. Active development.
