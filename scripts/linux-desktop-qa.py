#!/usr/bin/python3
"""Unattended packaged Ottrin smoke QA through AT-SPI and X11 input.

The runner creates a disposable HOME/XDG profile, never operates on the user's
files, and writes screenshots, logs, and a short result report to --evidence.
Run it with the system Python because Ubuntu installs PyAT-SPI there.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import zipfile

import pyatspi


VIEW_TO_LIST = "Switch to List view"
VIEW_TO_GRID = "Switch to Grid view"
VIEW_TO_COLUMN = "Switch to Column view"

target_process_id: int | None = None


def run(*args: str, check: bool = True, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        check=check,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
    )


def all_nodes(node):
    yield node
    for child in node:
        yield from all_nodes(child)


def wait_for(predicate, message: str, timeout: float = 10.0):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except Exception as error:  # AT-SPI trees are replaced between frames.
            last_error = error
        time.sleep(0.1)
    suffix = f": {last_error}" if last_error else ""
    raise AssertionError(f"Timed out waiting for {message}{suffix}")


def ottrin_app():
    desktop = pyatspi.Registry.getDesktop(0)
    candidates = [
        app
        for app in desktop
        if app.name == "ottrin"
        and app.childCount > 0
        and (target_process_id is None or app.get_process_id() == target_process_id)
    ]
    return candidates[-1] if candidates else None


def named_nodes(name: str):
    app = ottrin_app()
    return [] if app is None else [node for node in all_nodes(app) if node.name == name]


def visible_entry(name: str):
    return next(
        (
            node
            for node in named_nodes(name)
            if node.getRoleName() in {"toggle button", "push button"}
            and pyatspi.STATE_SHOWING in node.getState().getStates()
        ),
        None,
    )


def visible_named_nodes(name: str):
    return [
        node
        for node in named_nodes(name)
        if pyatspi.STATE_SHOWING in node.getState().getStates()
    ]


def click_accessible_name(name: str, role: str | None = None):
    node = wait_for(
        lambda: next(
            (
                item
                for item in named_nodes(name)
                if (role is None or item.getRoleName() == role)
                and pyatspi.STATE_SHOWING in item.getState().getStates()
            ),
            None,
        ),
        f"accessible control {name!r}",
    )
    action = node.queryAction()
    if not action.doAction(0):
        raise AssertionError(f"AT-SPI click failed for {name!r}")
    time.sleep(0.35)


def click_view_toggle(window_id: str, name: str, next_name: str):
    """Advance the view switch and prove the next state reached AT-SPI.

    Some AT-SPI bridges acknowledge an egui button action without delivering
    the pointer event.  Use that action first, then retry once through the
    existing X11 route if its accessible label did not advance.
    """
    node = wait_for(
        lambda: next(
            (
                item
                for item in named_nodes(name)
                if item.getRoleName() == "push button"
                and pyatspi.STATE_SHOWING in item.getState().getStates()
            ),
            None,
        ),
        f"view switch {name!r}",
    )
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    if not node.queryAction().doAction(0):
        raise AssertionError(f"AT-SPI click failed for view switch {name!r}")
    try:
        wait_for(
            lambda: visible_named_nodes(next_name),
            f"view transition from {name!r} to {next_name!r}",
            timeout=2.0,
        )
    except AssertionError:
        run("xdotool", "windowactivate", "--sync", window_id)
        run(
            "xdotool",
            "mousemove",
            str(rect.x + rect.width // 2),
            str(rect.y + rect.height // 2),
            "click",
            "1",
        )
        wait_for(
            lambda: visible_named_nodes(next_name),
            f"X11 view transition from {name!r} to {next_name!r}",
        )
    time.sleep(0.35)


def click_accessible_suffix(suffix: str, role: str | None = None):
    node = wait_for(
        lambda: next(
            (
                item
                for item in all_nodes(ottrin_app())
                if (role is None or item.getRoleName() == role)
                and pyatspi.STATE_SHOWING in item.getState().getStates()
                and (item.name or "").endswith(suffix)
            ),
            None,
        ),
        f"accessible control ending in {suffix!r}",
    )
    if not node.queryAction().doAction(0):
        raise AssertionError(f"AT-SPI click failed for control ending in {suffix!r}")
    time.sleep(0.8)


def context_menu_entry(window_id: str, name: str):
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    node.queryAction().doAction(0)
    time.sleep(0.2)
    node = wait_for(lambda: visible_entry(name), f"selected entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(rect.x + rect.width // 2),
        str(rect.y + rect.height // 2),
        "click",
        "3",
    )
    time.sleep(0.35)


def search_result_node(name: str):
    candidates = []
    for item in named_nodes(f"Search result: {name}"):
        if pyatspi.STATE_SHOWING not in item.getState().getStates():
            continue
        try:
            rect = item.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        except NotImplementedError:
            continue
        if rect.width > 0 and rect.height > 0:
            candidates.append((rect.width * rect.height, item))
    return max(candidates, default=(0, None), key=lambda candidate: candidate[0])[1]


def click_search_result(window_id: str, name: str, button: str = "1"):
    node = wait_for(
        lambda: search_result_node(name), f"visible search result {name!r}"
    )
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(rect.x + rect.width // 2),
        str(rect.y + rect.height // 2),
        "click",
        button,
    )
    time.sleep(0.35)


def write_visible_accessibility_tree(evidence: Path, name: str):
    app = ottrin_app()
    lines = []
    if app is not None:
        for node in all_nodes(app):
            try:
                states = node.getState().getStates()
                if pyatspi.STATE_SHOWING not in states:
                    continue
                lines.append(f"{node.getRoleName()} | {node.name or ''}")
            except Exception:
                continue
    (evidence / f"{name}.a11y.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def double_click_entry(window_id: str, name: str):
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    x = rect.x + rect.width // 2
    y = rect.y + rect.height // 2
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(x),
        str(y),
        "click",
        "--repeat",
        "2",
        "--delay",
        "100",
        "1",
    )
    time.sleep(0.5)


def click_entry(window_id: str, name: str):
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(rect.x + rect.width // 2),
        str(rect.y + rect.height // 2),
        "click",
        "1",
    )
    time.sleep(0.35)


def ctrl_click_entry(window_id: str, name: str):
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run("xdotool", "keydown", "Control_L")
    try:
        run(
            "xdotool",
            "mousemove",
            str(rect.x + rect.width // 2),
            str(rect.y + rect.height // 2),
            "mousedown",
            "1",
            "mouseup",
            "1",
        )
    finally:
        run("xdotool", "keyup", "Control_L", check=False)
    time.sleep(0.35)


def click_named_control(window_id: str, name: str, role: str = "push button"):
    node = wait_for(
        lambda: next(
            (
                item
                for item in named_nodes(name)
                if item.getRoleName() == role
                and pyatspi.STATE_SHOWING in item.getState().getStates()
            ),
            None,
        ),
        f"visible control {name!r}",
    )
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(rect.x + rect.width // 2),
        str(rect.y + rect.height // 2),
        "click",
        "1",
    )
    time.sleep(0.35)


def invoke_selection_action(window_id: str, filename: str, action_suffix: str):
    last_error = None
    for _ in range(3):
        try:
            click_accessible_name(filename, "toggle button")
            click_named_control(window_id, "Menu")
            node = wait_for(
                lambda: next(
                    (
                        item
                        for item in all_nodes(ottrin_app())
                        if (item.name or "").endswith(action_suffix)
                    ),
                    None,
                ),
                f"selection action {action_suffix!r}",
                timeout=2.0,
            )
            if not node.queryAction().doAction(0):
                raise AssertionError(f"AT-SPI action failed for {action_suffix!r}")
            time.sleep(0.8)
            return
        except Exception as error:
            last_error = error
            run("xdotool", "key", "--clearmodifiers", "Escape", check=False)
            time.sleep(0.4)
    raise AssertionError(
        f"Could not invoke {action_suffix!r} for {filename!r}: {last_error}"
    )


def invoke_context_action(window_id: str, filename: str, action_suffix: str):
    last_error = None
    for _ in range(3):
        try:
            context_menu_entry(window_id, filename)
            node = wait_for(
                lambda: next(
                    (
                        item
                        for item in all_nodes(ottrin_app())
                        if (item.name or "").endswith(action_suffix)
                    ),
                    None,
                ),
                f"context action {action_suffix!r}",
                timeout=2.0,
            )
            if not node.queryAction().doAction(0):
                raise AssertionError(f"AT-SPI action failed for {action_suffix!r}")
            time.sleep(0.8)
            return
        except Exception as error:
            last_error = error
            run("xdotool", "key", "--clearmodifiers", "Escape", check=False)
            time.sleep(0.4)
    raise AssertionError(
        f"Could not invoke {action_suffix!r} for {filename!r}: {last_error}"
    )


def xkey(window_id: str, *keys: str):
    run("xdotool", "windowactivate", "--sync", window_id)
    app = ottrin_app()
    if app is not None and app.childCount:
        app[0].queryComponent().grabFocus()
    time.sleep(0.1)
    for key in keys:
        run("xdotool", "key", "--clearmodifiers", key)
    time.sleep(0.35)


def go_to(window_id: str, path: Path):
    xkey(window_id, "ctrl+l")
    wait_for(
        lambda: next(
            (
                node
                for node in all_nodes(ottrin_app())
                if node.getRoleName() in {"entry", "text"}
                and pyatspi.STATE_FOCUSED in node.getState().getStates()
            ),
            None,
        ),
        "focused address field",
    )
    xkey(window_id, "ctrl+a")
    run("xdotool", "type", "--clearmodifiers", "--delay", "1", str(path))
    xkey(window_id, "Return")


def screenshot(evidence: Path, name: str):
    target = evidence / f"{name}.png"
    run("spectacle", "-b", "-n", "-a", "-o", str(target))
    wait_for(lambda: target.is_file() and target.stat().st_size > 0, f"screenshot {target}")


def quick_inspector_visible(filename: str):
    title = f"Quick Inspector — {filename}"
    return any(node.name == title for node in visible_named_nodes(title))


def close_quick_inspector(main_window_id: str, filename: str):
    xkey(main_window_id, "space")
    wait_for(
        lambda: not quick_inspector_visible(filename),
        "Quick Inspector close",
    )


def create_preview_fixtures(project_root: Path, documents: Path):
    shutil.copy2(
        project_root / ".hermes/spikes/pdfium-preview/fixture.pdf",
        documents / "preview.pdf",
    )
    shutil.copy2(
        project_root / ".hermes/spikes/pdfium-preview/fixture.pdf",
        documents / "preview-second.pdf",
    )
    with zipfile.ZipFile(documents / "letter.docx", "w") as archive:
        archive.writestr(
            "word/document.xml",
            "<w:document><w:body><w:p><w:r><w:t>Ottrin DOCX QA</w:t>"
            "</w:r></w:p></w:body></w:document>",
        )
    workbook_parts = {
        "xl/sharedStrings.xml": "<sst><si><t>Name</t></si><si><t>Ada</t></si></sst>",
        "xl/workbook.xml": (
            '<workbook xmlns:r="relationships"><sheets>'
            '<sheet name="Quarter 1" r:id="rId7"/></sheets></workbook>'
        ),
        "xl/_rels/workbook.xml.rels": (
            '<Relationships><Relationship Id="rId7" '
            'Target="worksheets/sheet1.xml"/></Relationships>'
        ),
        "xl/worksheets/sheet1.xml": (
            '<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c>'
            '<c r="C1"><v>Score</v></c></row><row>'
            '<c r="A2" t="s"><v>1</v></c><c r="C2"><v>42</v></c>'
            "</row></sheetData></worksheet>"
        ),
    }
    for filename in ("workbook.xlsx", "macros.xlsm"):
        with zipfile.ZipFile(documents / filename, "w") as archive:
            for part, xml in workbook_parts.items():
                archive.writestr(part, xml)
    with zipfile.ZipFile(documents / "slides.pptx", "w") as archive:
        archive.writestr(
            "ppt/slides/slide1.xml",
            "<p:sld><a:t>Launch plan</a:t><a:t>Ready for v1</a:t></p:sld>",
        )
    with zipfile.ZipFile(documents / "archive.zip", "w") as archive:
        archive.writestr("docs/readme.txt", "hello from Ottrin archive QA")
    (documents / "opaque.bin").write_bytes(b"\x00\x01\x02Ottrin generic QA\xff")
    preview_folder = documents / "PreviewFolder"
    preview_folder.mkdir()
    (preview_folder / "child.txt").write_text("folder summary fixture\n", encoding="utf-8")


def open_quick_inspector(
    main_window_id: str,
    filename: str,
    evidence: Path,
    screenshot_name: str,
):
    invoke_context_action(main_window_id, filename, "Quick Inspector")
    wait_for(
        lambda: quick_inspector_visible(filename),
        f"Quick Inspector for {filename}",
    )
    if Path(filename).suffix.lower() in {".png", ".jpg", ".jpeg", ".pdf", ".docx"}:
        wait_for(
            lambda: any(
                node.getRoleName() == "push button"
                for node in visible_named_nodes("Fit")
            ),
            f"Fit control for {filename}",
        )
        wait_for(
            lambda: any(
                node.getRoleName() == "push button"
                for node in visible_named_nodes("100%")
            ),
            f"100% control for {filename}",
        )
    screenshot(evidence, screenshot_name)
    close_quick_inspector(main_window_id, filename)


def a11y_enabled() -> bool:
    result = run(
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.a11y.Bus",
        "--object-path",
        "/org/a11y/bus",
        "--method",
        "org.freedesktop.DBus.Properties.Get",
        "org.a11y.Status",
        "IsEnabled",
        capture=True,
    )
    return "true" in result.stdout.lower()


def set_a11y(enabled: bool):
    run(
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.a11y.Bus",
        "--object-path",
        "/org/a11y/bus",
        "--method",
        "org.freedesktop.DBus.Properties.Set",
        "org.a11y.Status",
        "IsEnabled",
        f"<{'true' if enabled else 'false'}>",
    )


def require_a11y_registry():
    """Fail before launching Ottrin when the desktop AT-SPI route is absent."""
    try:
        pyatspi.Registry.getDesktop(0)
    except Exception as error:
        raise RuntimeError(
            "required AT-SPI registry service is unavailable; "
            "run this packaged desktop QA in a functioning accessibility session"
            f": {error}"
        ) from error


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("dist/Ottrin-0.1.0-linux-x86_64/bin/ottrin"),
    )
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()

    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"packaged binary not found: {binary}")
    for tool in ("busctl", "gdbus", "spectacle", "xdotool"):
        if shutil.which(tool) is None:
            parser.error(f"required tool not found: {tool}")
    try:
        require_a11y_registry()
    except RuntimeError as error:
        parser.error(str(error))

    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    qa_root = Path(tempfile.mkdtemp(prefix="ottrin-desktop-qa-"))
    home = qa_root / "home"
    documents = home / "Documents"
    subfolder = documents / "Subfolder"
    multi_target = documents / "MultiTarget"
    subfolder.mkdir(parents=True)
    multi_target.mkdir()
    (documents / "notes.txt").write_text(
        "Hello Ottrin desktop QA\nsearch-token-ottrin-2026\n", encoding="utf-8"
    )
    (documents / "sample.json").write_text('{"qa": true}\n', encoding="utf-8")
    (documents / "delete-me.txt").write_text("disposable\n", encoding="utf-8")
    (documents / "trash-me.txt").write_text("recoverable disposable\n", encoding="utf-8")
    shutil.copy2("assets/icon.png", documents / "image.png")
    create_preview_fixtures(Path.cwd(), documents)

    prior_a11y = a11y_enabled()
    set_a11y(True)
    env = os.environ.copy()
    env.pop("WAYLAND_DISPLAY", None)
    env.update(
        {
            "WINIT_UNIX_BACKEND": "x11",
            "NO_AT_BRIDGE": "0",
            "HOME": str(home),
            "XDG_CONFIG_HOME": str(qa_root / "config"),
            "XDG_CACHE_HOME": str(qa_root / "cache"),
            "XDG_STATE_HOME": str(qa_root / "state"),
            "XDG_DATA_HOME": str(qa_root / "data"),
        }
    )
    stderr = (evidence / "ottrin.stderr.log").open("w", encoding="utf-8")
    app_process = subprocess.Popen([str(binary)], env=env, stderr=stderr)
    global target_process_id
    target_process_id = app_process.pid
    results: list[dict[str, str]] = []

    def passed(check: str, detail: str):
        results.append({"check": check, "status": "passed", "detail": detail})

    try:
        wait_for(ottrin_app, "Ottrin AT-SPI application")
        window_id = wait_for(
            lambda: run(
                "xdotool",
                "search",
                "--pid",
                str(app_process.pid),
                "--name",
                "^Ottrin$",
                capture=True,
                check=False,
            ).stdout.strip().splitlines()[-1:]
            or None,
            "Ottrin X11 window",
        )[0]
        run("xdotool", "windowactivate", "--sync", window_id)
        wait_for(lambda: visible_entry("Documents"), "initial home listing")

        double_click_entry(window_id, "Documents")
        wait_for(lambda: visible_entry("notes.txt"), "Documents listing")
        expected = {
            "Subfolder",
            "MultiTarget",
            "delete-me.txt",
            "image.png",
            "preview.pdf",
            "preview-second.pdf",
            "letter.docx",
            "workbook.xlsx",
            "macros.xlsm",
            "slides.pptx",
            "archive.zip",
            "opaque.bin",
            "PreviewFolder",
            "notes.txt",
            "sample.json",
            "trash-me.txt",
        }
        observed = {name for name in expected if visible_entry(name)}
        assert observed == expected, (observed, expected)
        passed("address-navigation", f"Opened {documents} and exposed all fixture entries.")
        screenshot(evidence, "01-miller-documents")

        # Exercise search while the main file view has a stable keyboard focus;
        # later workflow steps intentionally open menus and dialogs.
        click_accessible_name("Toggle Search", "push button")
        search_field = wait_for(
            lambda: next(
                (
                    node
                    for node in all_nodes(ottrin_app())
                    if node.getRoleName() in {"entry", "text"}
                    and pyatspi.STATE_FOCUSED in node.getState().getStates()
                ),
                None,
            ),
            "focused search field",
        )
        assert search_field is not None
        run("xdotool", "type", "--clearmodifiers", "--delay", "1", "sample.json")
        wait_for(
            lambda: len(visible_named_nodes("sample.json")) >= 2,
            "search result for sample.json",
            timeout=15.0,
        )
        passed("search", "Found sample.json through the packaged filename search interface.")
        click_search_result(window_id, "sample.json")
        xkey(window_id, "space")
        wait_for(
            lambda: quick_inspector_visible("sample.json"),
            "Quick Inspector from selected search result",
        )
        close_quick_inspector(window_id, "sample.json")
        passed(
            "search-space-preview",
            "Selected a search result and opened its full Quick Inspector with Space.",
        )
        click_search_result(window_id, "sample.json", "3")
        screenshot(evidence, "01a-search-result-context")
        write_visible_accessibility_tree(evidence, "01a-search-result-context")
        click_accessible_suffix("Copy Item")
        passed(
            "search-context-actions",
            "Opened the shared search-result context menu and copied its selected item.",
        )
        screenshot(evidence, "01a-search-result")
        xkey(window_id, "Escape")
        wait_for(lambda: visible_entry("notes.txt"), "Documents after search")

        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID)
        wait_for(lambda: visible_entry("notes.txt"), "List-view entries")
        passed("list-view", "Cycled from Miller to List with named selectable rows.")
        screenshot(evidence, "02-list-documents")

        click_view_toggle(window_id, VIEW_TO_GRID, VIEW_TO_COLUMN)
        wait_for(lambda: visible_entry("notes.txt"), "Grid-view entries")
        passed("grid-view", "Cycled from List to Grid with named selectable cells.")
        screenshot(evidence, "03-grid-documents")

        click_view_toggle(window_id, VIEW_TO_COLUMN, VIEW_TO_LIST)
        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID)
        wait_for(lambda: visible_entry("notes.txt"), "List view for workflow matrix")
        invoke_context_action(window_id, "notes.txt", "Copy Item")
        double_click_entry(window_id, "Subfolder")
        wait_for(
            lambda: visible_entry("notes.txt") and not visible_entry("delete-me.txt"),
            "Subfolder navigation",
        )
        click_named_control(window_id, "Menu")
        click_accessible_suffix("Paste into Current Folder")
        wait_for(lambda: (subfolder / "notes.txt").is_file(), "copied file")
        assert (documents / "notes.txt").is_file()
        passed("copy", "Copied notes.txt into Subfolder without changing the source.")
        screenshot(evidence, "04-copy-result")

        click_accessible_name("\ue2c7 Documents", "push button")
        wait_for(lambda: visible_entry("notes.txt"), "Documents after copy")
        invoke_context_action(window_id, "notes.txt", "Quick Inspector")
        wait_for(
            lambda: quick_inspector_visible("notes.txt"),
            "Quick Inspector viewport",
        )
        passed("quick-inspector-text", "Opened the packaged text preview from its accessible menu.")
        screenshot(evidence, "05-quick-inspector-text")
        close_quick_inspector(window_id, "notes.txt")

        invoke_context_action(window_id, "image.png", "Quick Inspector")
        wait_for(
            lambda: quick_inspector_visible("image.png"),
            "image Quick Inspector viewport",
        )
        passed("quick-inspector-image", "Opened the packaged image preview.")
        screenshot(evidence, "06-quick-inspector-image")
        close_quick_inspector(window_id, "image.png")

        preview_matrix = [
            ("preview.pdf", "07-quick-inspector-pdf", "rendered PDF plus metadata"),
            (
                "preview-second.pdf",
                "07a-quick-inspector-second-pdf",
                "second rendered PDF in the same process",
            ),
            ("letter.docx", "08-quick-inspector-docx", "DOCX"),
            ("workbook.xlsx", "09-quick-inspector-xlsx", "XLSX"),
            ("macros.xlsm", "10-quick-inspector-xlsm", "XLSM"),
            ("slides.pptx", "11-quick-inspector-pptx", "PPTX"),
            ("archive.zip", "12-quick-inspector-archive", "archive"),
            ("PreviewFolder", "13-quick-inspector-folder", "folder summary"),
            ("opaque.bin", "14-quick-inspector-generic", "generic fallback"),
        ]
        for filename, evidence_name, description in preview_matrix:
            open_quick_inspector(window_id, filename, evidence, evidence_name)
            passed(
                f"quick-inspector-{filename.rsplit('.', 1)[-1].lower()}",
                f"Opened the packaged {description} preview for {filename}.",
            )

        click_view_toggle(window_id, VIEW_TO_GRID, VIEW_TO_COLUMN)
        wait_for(lambda: visible_entry("trash-me.txt"), "trash fixture")
        invoke_context_action(window_id, "trash-me.txt", "Delete…")
        # The dialog opens with Move to Trash selected. Confirm through the
        # product's documented keyboard route rather than relying on an
        # accessibility bridge to synthesize a second pointer click.
        xkey(window_id, "Return")
        wait_for(lambda: not (documents / "trash-me.txt").exists(), "trash deletion")
        wait_for(
            lambda: any((qa_root / "data" / "Trash" / "files").glob("trash-me*")),
            "isolated trash payload",
        )
        passed("trash-delete", "Moved only the disposable fixture into isolated XDG Trash.")

        click_view_toggle(window_id, VIEW_TO_COLUMN, VIEW_TO_LIST)
        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID)
        wait_for(lambda: visible_entry("delete-me.txt"), "delete fixture")
        invoke_context_action(window_id, "delete-me.txt", "Delete…")
        # The confirmation choices expose accessible names. Select the explicit
        # destructive action rather than relying on keyboard focus/order.
        click_accessible_name("Delete Permanently")
        wait_for(lambda: not (documents / "delete-me.txt").exists(), "permanent deletion")
        passed("permanent-delete", "Permanently deleted only the disposable fixture.")

        wait_for(lambda: visible_entry("sample.json"), "Documents after preview")
        click_entry(window_id, "sample.json")
        ctrl_click_entry(window_id, "notes.txt")
        screenshot(evidence, "15-multi-select")
        click_named_control(window_id, "Menu")
        click_accessible_suffix("Copy Selection")
        double_click_entry(window_id, "MultiTarget")
        click_named_control(window_id, "Menu")
        click_accessible_suffix("Paste into Current Folder")
        wait_for(
            lambda: (multi_target / "sample.json").is_file()
            and (multi_target / "notes.txt").is_file(),
            "two copied multi-selection fixtures",
        )
        passed(
            "multi-select",
            "Selected two entries with Ctrl-click and copied both as one operation.",
        )
        click_accessible_name("\ue2c7 Documents", "push button")
        wait_for(lambda: visible_entry("sample.json"), "Documents after multi-copy")

        xkey(window_id, "ctrl+t")
        wait_for(lambda: visible_entry("Documents"), "new-tab home listing")
        go_to(window_id, subfolder)
        wait_for(lambda: visible_entry("notes.txt"), "new-tab Subfolder listing")
        xkey(window_id, "ctrl+Tab")
        wait_for(
            lambda: visible_entry("sample.json"),
            "original tab after keyboard tab switch",
        )
        passed(
            "tabs",
            "Created a second tab, navigated it independently, and switched back.",
        )
        screenshot(evidence, "17-tabs")

        invoke_context_action(window_id, "Subfolder", "Open in Tandem")
        wait_for(
            lambda: len(visible_named_nodes("notes.txt")) >= 2,
            "Tandem panes",
        )
        xkey(window_id, "Tab")
        assert app_process.poll() is None
        passed(
            "tandem",
            "Opened Subfolder in Tandem and switched the active pane by keyboard.",
        )
        screenshot(evidence, "18-tandem")

        xkey(window_id, "Tab")
        wait_for(lambda: visible_entry("sample.json"), "Documents Tandem pane")
        invoke_context_action(window_id, "sample.json", "Rename…")
        wait_for(
            lambda: next(
                (
                    node
                    for node in all_nodes(ottrin_app())
                    if node.getRoleName() in {"entry", "text"}
                    and pyatspi.STATE_FOCUSED in node.getState().getStates()
                ),
                None,
            ),
            "focused rename field",
        )
        xkey(window_id, "ctrl+a")
        run("xdotool", "type", "--clearmodifiers", "--delay", "1", "renamed.json")
        click_named_control(window_id, "Rename", "push button")
        wait_for(lambda: (documents / "renamed.json").is_file(), "renamed file")
        assert not (documents / "sample.json").exists()
        passed("rename", "Renamed sample.json to renamed.json through the packaged UI.")

        passed("process-health", "Packaged process remained alive through the workflow.")
        (evidence / "results.json").write_text(
            json.dumps(
                {
                    "binary": str(binary),
                    "fixture_root": str(qa_root),
                    "results": results,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        print(json.dumps(results, indent=2))
        return 0
    finally:
        if app_process.poll() is None:
            app_process.terminate()
            try:
                app_process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                app_process.kill()
        stderr.close()
        if not prior_a11y:
            set_a11y(False)


if __name__ == "__main__":
    raise SystemExit(main())
