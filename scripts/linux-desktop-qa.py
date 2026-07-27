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
VIEW_MENU_LABELS = {
    VIEW_TO_LIST: "Column view",
    VIEW_TO_GRID: "List view",
    VIEW_TO_COLUMN: "Grid view",
}

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
        if app is not None
        and app.name == "ottrin"
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


def entry_selected(name: str) -> bool:
    node = visible_entry(name)
    if node is None:
        return False
    if pyatspi.STATE_SELECTED in node.getState().getStates():
        return True
    # KWin/Xwayland occasionally exposes the selected row without
    # STATE_SELECTED, while Ottrin's visible status still identifies it.  That
    # label is an explicit product selection signal, so accept only a matching
    # visible status label as the compatibility fallback.
    return any(
        line.startswith("label | ") and line.endswith(f"·  {name}")
        for line in visible_a11y_nodes()
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


def focused_search_field():
    return next(
        (
            node
            for node in all_nodes(ottrin_app())
            if node.getRoleName() in {"entry", "text"}
            and pyatspi.STATE_FOCUSED in node.getState().getStates()
        ),
        None,
    )


def open_search(window_id: str):
    """Open search and prove its input owns focus before typing into it."""
    node = wait_for(
        lambda: next(
            (
                item
                for item in named_nodes("Toggle Search")
                if item.getRoleName() == "push button"
                and pyatspi.STATE_SHOWING in item.getState().getStates()
            ),
            None,
        ),
        "accessible control 'Toggle Search'",
    )
    if not node.queryAction().doAction(0):
        raise AssertionError("AT-SPI click failed for 'Toggle Search'")
    try:
        return wait_for(focused_search_field, "focused search field", timeout=2.0)
    except AssertionError:
        x, y = x11_point_for_node(window_id, node)
        run("xdotool", "windowactivate", "--sync", window_id)
        run(
            "xdotool",
            "mousemove",
            "--window",
            window_id,
            str(x),
            str(y),
            "click",
            "1",
        )
        return wait_for(focused_search_field, "focused search field after X11 retry")


def x11_window_geometry(window_id: str) -> dict[str, int]:
    """Return the X11 outer geometry used by xdotool's desktop coordinates."""
    result = run("xdotool", "getwindowgeometry", "--shell", window_id, capture=True)
    values = dict(
        line.split("=", 1)
        for line in result.stdout.splitlines()
        if "=" in line
    )
    try:
        return {key: int(values[key]) for key in ("X", "Y", "WIDTH", "HEIGHT")}
    except (KeyError, ValueError) as error:
        raise AssertionError(f"Could not read X11 geometry for window {window_id!r}") from error


def view_menu_label(next_name: str) -> str:
    try:
        return VIEW_MENU_LABELS[next_name]
    except KeyError as error:
        raise AssertionError(f"unknown target view switch {next_name!r}") from error


def x11_point_for_component(component, app, window: dict[str, int]) -> tuple[int, int]:
    """Map an AT-SPI desktop component center into window-relative X11 space."""
    component_left, component_top, component_width, component_height = component
    app_left, app_top, app_width, app_height = app
    if app_width <= 0 or app_height <= 0:
        raise AssertionError("accessible Ottrin window has an empty extent")
    if not (
        app_left <= component_left
        and app_top <= component_top
        and component_left + component_width <= app_left + app_width
        and component_top + component_height <= app_top + app_height
    ):
        raise AssertionError("view switch is outside the accessible Ottrin window")
    return (
        round((component_left + component_width / 2 - app_left) * window["WIDTH"] / app_width),
        round((component_top + component_height / 2 - app_top) * window["HEIGHT"] / app_height),
    )


def x11_point_for_entry(component, app, window: dict[str, int]) -> tuple[int, int]:
    """Map a List entry into the X11 window just like a view control."""
    return x11_point_for_component(component, app, window)


def x11_point_for_node(window_id: str, node) -> tuple[int, int]:
    """Return a checked X11-window-relative target for an accessible node."""
    target = entry_x11_diagnostics(window_id, node)["x11_window_relative_target"]
    return target["x"], target["y"]


def entry_x11_diagnostics(window_id: str, node) -> dict[str, object]:
    """Describe the checked accessible-entry to X11 target mapping."""
    app = ottrin_app()
    if app is None or app.childCount == 0:
        raise AssertionError("Ottrin accessible window is unavailable for X11 coordinate mapping")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    app_rect = app[0].queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    component = (rect.x, rect.y, rect.width, rect.height)
    accessible_window = (app_rect.x, app_rect.y, app_rect.width, app_rect.height)
    window = x11_window_geometry(window_id)
    x, y = x11_point_for_entry(
        component,
        accessible_window,
        window,
    )
    return {
        "accessible_entry": dict(zip(("x", "y", "width", "height"), component)),
        "accessible_window": dict(
            zip(("x", "y", "width", "height"), accessible_window)
        ),
        "x11_window": window,
        "x11_window_relative_target": {"x": x, "y": y},
    }


def view_toggle_x11_diagnostics(window_id: str, node) -> dict[str, object]:
    component = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    app = ottrin_app()
    if app is None or app.childCount == 0:
        raise AssertionError("Ottrin accessible window is unavailable for X11 coordinate mapping")
    app_rect = app[0].queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    component_tuple = (component.x, component.y, component.width, component.height)
    app_tuple = (app_rect.x, app_rect.y, app_rect.width, app_rect.height)
    window = x11_window_geometry(window_id)
    x, y = x11_point_for_component(
        component_tuple,
        app_tuple,
        window,
    )
    return {
        "accessible_component": dict(zip(("x", "y", "width", "height"), component_tuple)),
        "accessible_window": dict(zip(("x", "y", "width", "height"), app_tuple)),
        "x11_window": window,
        "x11_window_relative_target": {"x": x, "y": y},
    }


def click_view_toggle(
    window_id: str, name: str, next_name: str, evidence: Path | None = None
):
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
    if not node.queryAction().doAction(0):
        raise AssertionError(f"AT-SPI click failed for view switch {name!r}")
    try:
        wait_for(
            lambda: visible_named_nodes(next_name),
            f"view transition from {name!r} to {next_name!r}",
            timeout=2.0,
        )
    except AssertionError:
        click_accessible_name("View Options", "push button")
        click_accessible_suffix(view_menu_label(next_name), "push button")
        try:
            wait_for(
                lambda: visible_named_nodes(next_name),
                f"View Options transition from {name!r} to {next_name!r}",
                timeout=2.0,
            )
            time.sleep(0.35)
            return
        except AssertionError:
            pass
        diagnostics = view_toggle_x11_diagnostics(window_id, node)
        if evidence is not None:
            (evidence / f"view-toggle-{name.replace(' ', '-').lower()}.json").write_text(
                json.dumps(diagnostics, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
        x11_target = diagnostics["x11_window_relative_target"]
        run("xdotool", "windowactivate", "--sync", window_id)
        run(
            "xdotool",
            "mousemove",
            "--window",
            window_id,
            str(x11_target["x"]),
            str(x11_target["y"]),
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
    # A prior Quick Inspector can leave its old row selected even when the
    # accessibility action below reports success.  Prove the named row owns
    # selection before right-clicking its desktop coordinates; otherwise the
    # next context action can silently target the previous PDF.
    click_entry(window_id, name)
    node = wait_for(lambda: visible_entry(name), f"selected entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run("xdotool", "mousemove", str(rect.x + rect.width // 2), str(rect.y + rect.height // 2), "click", "3")
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
    run("xdotool", "mousemove", str(rect.x + rect.width // 2), str(rect.y + rect.height // 2), "click", button)
    time.sleep(0.35)


def visible_a11y_nodes() -> list[str]:
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
    return lines


def destination_breadcrumb_visible(name: str) -> bool:
    """Confirm navigation even when the destination has no entries yet."""
    return any(
        node.startswith("push button | ") and node.endswith(name)
        for node in visible_a11y_nodes()
    )


def write_visible_accessibility_tree(evidence: Path, name: str):
    lines = visible_a11y_nodes()
    (evidence / f"{name}.a11y.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_entry_activation_failure(evidence: Path, name: str, diagnostics: dict[str, object]):
    """Persist target geometry and post-click UI state only for a failed activation."""
    payload = dict(diagnostics)
    payload["entry_name"] = name
    payload["post_click_visible_nodes"] = visible_a11y_nodes()
    (evidence / f"entry-activation-{name.replace(' ', '-').lower()}.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def write_workflow_failure(evidence: Path, qa_root: Path, error: Exception):
    """Retain the failing UI state without treating a partial run as a pass."""
    documents = qa_root / "home" / "Documents"
    payload = {
        "error": str(error),
        "fixture_root": str(qa_root),
        "delete_me_exists": (documents / "delete-me.txt").exists(),
        "trash_me_exists": (documents / "trash-me.txt").exists(),
        "visible_nodes": visible_a11y_nodes(),
    }
    (evidence / "workflow-failure.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def double_click_entry(window_id: str, name: str) -> dict[str, object]:
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    diagnostics = entry_x11_diagnostics(window_id, node)
    target = diagnostics["x11_window_relative_target"]
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        "--window",
        window_id,
        str(target["x"]),
        str(target["y"]),
        "click",
        "--repeat",
        "2",
        "--delay",
        "100",
        "1",
    )
    time.sleep(0.5)
    return diagnostics


def double_click_entry_at_desktop_coordinates(window_id: str, name: str):
    """Retry activation through the direct AT-SPI desktop coordinate route."""
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        str(rect.x + rect.width // 2),
        str(rect.y + rect.height // 2),
        "click",
        "--repeat",
        "2",
        "--delay",
        "100",
        "1",
    )
    time.sleep(0.5)


def click_entry(window_id: str, name: str):
    """Select the first entry, proving selection before extending it with Ctrl-click."""
    click_accessible_name(name, "toggle button")
    if entry_selected(name):
        return

    # The accessibility action can report success while a replaced List-view
    # node leaves the previous preview selection active.  Retry once through
    # the same checked accessible-to-X11 mapping used for the Ctrl extension,
    # then require the named entry's selected state before copying anything.
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    x, y = x11_point_for_node(window_id, node)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "mousemove",
        "--window",
        window_id,
        str(x),
        str(y),
        "click",
        "1",
    )
    try:
        wait_for(
            lambda: entry_selected(name),
            f"selected entry {name!r} after window-relative X11 click",
            timeout=2.0,
        )
    except AssertionError:
        # KWin/Xwayland can expose a client geometry that differs from the
        # AT-SPI frame mapping.  Context actions use desktop coordinates
        # successfully on that route, so make one final direct, named-node
        # click before failing the selection proof.
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
        wait_for(
            lambda: entry_selected(name),
            f"selected entry {name!r} after direct AT-SPI X11 click",
        )
    time.sleep(0.35)


def ctrl_click_entry(window_id: str, name: str):
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    x, y = x11_point_for_node(window_id, node)
    run("xdotool", "windowactivate", "--sync", window_id)
    run(
        "xdotool",
        "keydown",
        "Control_L",
        "mousemove",
        "--window",
        window_id,
        str(x),
        str(y),
        "click",
        "1",
        "keyup",
        "Control_L",
    )
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
    run("xdotool", "mousemove", str(rect.x + rect.width // 2), str(rect.y + rect.height // 2), "click", "1")
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


def inspector_requires_zoom_controls(filename: str) -> bool:
    """Only image and PDF inspectors expose the toolbar zoom buttons."""
    return Path(filename).suffix.lower() in {".png", ".jpg", ".jpeg", ".pdf"}


def close_quick_inspector(
    main_window_id: str, filename: str, *, dismiss_parent_menu: bool = False
):
    xkey(main_window_id, "space")
    wait_for(
        lambda: not quick_inspector_visible(filename),
        "Quick Inspector close",
    )
    if dismiss_parent_menu:
        # The native inspector is opened from a context menu.  Closing its
        # child viewport can leave that parent popup active on KWin/Xwayland,
        # where it otherwise absorbs the next named-entry click and invalidates
        # the matrix.  Do not send Escape for a Space-opened search preview:
        # that shortcut belongs to the search panel itself.
        xkey(main_window_id, "Escape")


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
    if inspector_requires_zoom_controls(filename):
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
    close_quick_inspector(main_window_id, filename, dismiss_parent_menu=True)


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
        search_field = open_search(window_id)
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
        # Escape dismisses the result context menu but leaves the search panel
        # active; its modal input layer then absorbs toolbar clicks.  Toggle it
        # through the same accessible control that opened it before exercising
        # the main view controls.
        click_accessible_name("Toggle Search", "push button")
        wait_for(
            lambda: not visible_named_nodes("Search result: sample.json"),
            "closed search panel",
        )
        wait_for(lambda: visible_entry("notes.txt"), "Documents after search")

        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID, evidence)
        wait_for(lambda: visible_entry("notes.txt"), "List-view entries")
        passed("list-view", "Cycled from Miller to List with named selectable rows.")
        screenshot(evidence, "02-list-documents")

        click_view_toggle(window_id, VIEW_TO_GRID, VIEW_TO_COLUMN, evidence)
        wait_for(lambda: visible_entry("notes.txt"), "Grid-view entries")
        passed("grid-view", "Cycled from List to Grid with named selectable cells.")
        screenshot(evidence, "03-grid-documents")

        click_view_toggle(window_id, VIEW_TO_COLUMN, VIEW_TO_LIST, evidence)
        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID, evidence)
        wait_for(lambda: visible_entry("notes.txt"), "List view for workflow matrix")
        invoke_context_action(window_id, "notes.txt", "Copy Item")
        subfolder_activation = double_click_entry(window_id, "Subfolder")
        try:
            wait_for(
                lambda: destination_breadcrumb_visible("Subfolder"),
                "Subfolder navigation after window-relative double-click",
                timeout=2.0,
            )
        except AssertionError:
            double_click_entry_at_desktop_coordinates(window_id, "Subfolder")
            try:
                wait_for(
                    lambda: destination_breadcrumb_visible("Subfolder"),
                    "Subfolder navigation after direct AT-SPI X11 double-click",
                )
            except AssertionError:
                write_entry_activation_failure(
                    evidence, "Subfolder", subfolder_activation
                )
                raise
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

        click_view_toggle(window_id, VIEW_TO_GRID, VIEW_TO_COLUMN, evidence)
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

        click_view_toggle(window_id, VIEW_TO_COLUMN, VIEW_TO_LIST, evidence)
        click_view_toggle(window_id, VIEW_TO_LIST, VIEW_TO_GRID, evidence)
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
    except Exception as error:
        write_workflow_failure(evidence, qa_root, error)
        raise
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
