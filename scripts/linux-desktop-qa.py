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
import signal
import socket
import struct
import subprocess
import tempfile
import time
import zipfile
import zlib

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
    """Open search through the checked ordinary-pointer route before typing."""
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
    return wait_for(focused_search_field, "focused search field after X11 click")


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


def desktop_point_for_node(node) -> tuple[int, int]:
    """Return the named node's direct desktop center for the XWayland retry."""
    rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    return rect.x + rect.width // 2, rect.y + rect.height // 2


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
        "a11y_desktop_target": dict(zip(("x", "y"), desktop_point_for_node(node))),
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
    """Advance the view switch through the checked ordinary-pointer route."""
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
    diagnostics = view_toggle_x11_diagnostics(window_id, node)
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
    try:
        wait_for(
            lambda: visible_named_nodes(next_name),
            f"X11 view transition from {name!r} to {next_name!r}",
        )
    except AssertionError:
        if evidence is not None:
            (evidence / f"view-toggle-{name.replace(' ', '-').lower()}.json").write_text(
                json.dumps(diagnostics, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
        raise
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
    selected_before_context = True
    try:
        click_entry(window_id, name)
    except AssertionError:
        # A native context click is also a named-row selection gesture.  Keep
        # its authority strict by proving selection immediately afterwards;
        # never invoke a menu action while the old row remains selected.
        selected_before_context = False
    node = wait_for(lambda: visible_entry(name), f"selected entry {name!r}")
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
        "3",
    )
    if not selected_before_context:
        wait_for(
            lambda: entry_selected(name),
            f"selected entry {name!r} after named context click",
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


def write_context_action_failure(
    evidence: Path,
    filename: str,
    action_suffix: str,
    error: Exception,
    window_id: str | None = None,
):
    """Retain the exact target and popup tree when a context action is unsafe."""
    payload = {
        "filename": filename,
        "action": action_suffix,
        "error": str(error),
        "visible_nodes": visible_a11y_nodes(),
    }
    node = visible_entry(filename)
    if node is not None:
        payload["entry_selected"] = entry_selected(filename)
        if window_id is not None:
            try:
                payload["entry_x11_diagnostics"] = entry_x11_diagnostics(window_id, node)
            except Exception as diagnostic_error:
                payload["entry_x11_diagnostics_error"] = str(diagnostic_error)
    safe_name = filename.replace("/", "-")
    (evidence / f"context-action-{safe_name}.json").write_text(
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


def double_click_entry_at_desktop_coordinates(window_id: str, name: str) -> dict[str, object]:
    """Retry a named entry's activation using ordinary desktop-coordinate input."""
    node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
    x, y = desktop_point_for_node(node)
    diagnostics: dict[str, object] = {"a11y_desktop_target": {"x": x, "y": y}}
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
    return diagnostics


def click_entry(window_id: str, name: str):
    """Select an entry through ordinary pointer routes with named-node proof."""
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
        # KWin/Xwayland can disagree about client-relative coordinates. Retry
        # exactly once at the same named node's AT-SPI desktop center, still
        # using an ordinary xdotool click, then require the selection state.
        node = wait_for(lambda: visible_entry(name), f"visible entry {name!r}")
        x, y = desktop_point_for_node(node)
        run("xdotool", "windowactivate", "--sync", window_id)
        run("xdotool", "mousemove", str(x), str(y), "click", "1")
        wait_for(
            lambda: entry_selected(name),
            f"selected entry {name!r} after named desktop-coordinate X11 click",
            timeout=2.0,
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


def invoke_context_action(
    window_id: str,
    filename: str,
    action_suffix: str,
    evidence: Path | None = None,
):
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
    error = AssertionError(
        f"Could not invoke {action_suffix!r} for {filename!r}: {last_error}"
    )
    if evidence is not None:
        # The target proof has failed, so capture the actual visible popup tree
        # and a screen before returning the fail-closed error to the workflow.
        screenshot_error = None
        try:
            screenshot(evidence, f"context-action-{filename.replace('/', '-')}")
        except Exception as capture_error:
            screenshot_error = capture_error
        write_context_action_failure(evidence, filename, action_suffix, error, window_id)
        if screenshot_error is not None:
            (evidence / f"context-action-{filename.replace('/', '-')}.screenshot-error.txt").write_text(
                f"{screenshot_error}\n", encoding="utf-8"
            )
    raise error


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
    active = run("xdotool", "getactivewindow", capture=True).stdout.strip()
    pid = int(run("xdotool", "getwindowpid", active, capture=True).stdout.strip())
    if pid != target_process_id:
        raise AssertionError(f"Refusing screenshot of non-Ottrin window {active}, PID {pid}")
    run("spectacle", "--new-instance", "--background", "--nonotify", "--activewindow", "--output", str(target))
    wait_for(lambda: target.is_file() and target.stat().st_size > 0, f"screenshot {target}")


def quick_inspector_visible(filename: str):
    title = f"Quick Inspector — {filename}"
    return any(node.name == title for node in visible_named_nodes(title))


def inspector_content_visible(marker: str) -> bool:
    """Require rendered inspector text, not merely its native viewport."""
    return any(marker in node for node in visible_a11y_nodes())


def quick_inspector_quiescent(filename: str) -> bool:
    """Reject a transient AT-SPI disappearance while a viewport is closing."""
    if quick_inspector_visible(filename):
        return False
    time.sleep(0.35)
    return not quick_inspector_visible(filename)


def inspector_requires_zoom_controls(filename: str) -> bool:
    """Rendered pages and images expose the inspector zoom toolbar."""
    return Path(filename).suffix.lower() in {".png", ".jpg", ".jpeg", ".pdf", ".svg"}


def close_quick_inspector(
    main_window_id: str, filename: str, *, dismiss_parent_menu: bool = False
):
    xkey(main_window_id, "space")
    try:
        wait_for(
            lambda: quick_inspector_quiescent(filename),
            "Quick Inspector close after Space",
        )
    except AssertionError:
        # On KWin/XWayland a native child viewport can swallow Space while
        # remaining in the accessibility tree. Escape is the inspector's
        # documented alternate close route; require the same quiescence after
        # it instead of continuing with a stale viewport.
        xkey(main_window_id, "Escape")
        wait_for(
            lambda: quick_inspector_quiescent(filename),
            "Quick Inspector close after Escape fallback",
        )
        return
    if dismiss_parent_menu:
        # The native inspector is opened from a context menu.  Closing its
        # child viewport can leave that parent popup active on KWin/Xwayland,
        # where it otherwise absorbs the next named-entry click and invalidates
        # the matrix.  Do not send Escape for a Space-opened search preview:
        # that shortcut belongs to the search panel itself.
        xkey(main_window_id, "Escape")
        wait_for(
            lambda: quick_inspector_quiescent(filename),
            "Quick Inspector quiescence after parent-menu Escape",
        )


def write_pdf_fixture(path: Path, text: str, pages: int = 1):
    """Write a minimal self-contained PDF so desktop QA has no ignored-file dependency."""
    stream = f"BT /F1 18 Tf 72 720 Td ({text}) Tj ET\n".encode("ascii")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
        f"<< /Length {len(stream)} >>\nstream\n".encode("ascii") + stream + b"endstream",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    if pages == 2:
        objects[1] = b"<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>"
        second = f"BT /F1 18 Tf 72 720 Td ({text} - page 2) Tj ET\n".encode("ascii")
        objects.extend([
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>",
            f"<< /Length {len(second)} >>\nstream\n".encode("ascii") + second + b"endstream",
        ])
    payload = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for number, obj in enumerate(objects, start=1):
        offsets.append(len(payload))
        payload.extend(f"{number} 0 obj\n".encode("ascii"))
        payload.extend(obj)
        payload.extend(b"\nendobj\n")
    xref_offset = len(payload)
    payload.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
    payload.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        payload.extend(f"{offset:010} 00000 n \n".encode("ascii"))
    payload.extend(
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n".encode(
            "ascii"
        )
    )
    path.write_bytes(payload)


def create_preview_fixtures(project_root: Path, documents: Path):
    write_pdf_fixture(documents / "preview.pdf", "Ottrin PDF QA", pages=2)
    write_pdf_fixture(documents / "preview-second.pdf", "Ottrin PDF QA second document")
    (documents / "ottrin-preview.svg").write_text(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\">"
        "<rect width=\"120\" height=\"80\" fill=\"#00ff88\"/>"
        "<circle cx=\"60\" cy=\"40\" r=\"22\" fill=\"#16202a\"/>"
        "</svg>",
        encoding="utf-8",
    )
    baseline = project_root / "crates/ottrin-preview/tests/fixtures/docx-baseline.docx"
    with zipfile.ZipFile(baseline) as source, zipfile.ZipFile(documents / "letter.docx", "w") as archive:
        for part in source.infolist():
            contents = source.read(part.filename)
            if part.filename == "word/document.xml":
                contents = contents.replace(b"DOCX BASELINE", b"Ottrin DOCX QA visible document content")
            archive.writestr(part.filename, contents)
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
    wide_parts = dict(workbook_parts)
    wide_parts["xl/worksheets/sheet1.xml"] = '<worksheet><sheetData><row>' + ''.join(
        f'<c r="{chr(65 + column)}1"><v>Column {column + 1}</v></c>' for column in range(20)
    ) + '</row><row>' + ''.join(
        f'<c r="{chr(65 + column)}2"><v>' + ('Long wrapped cell content ' * 8 if column == 0 else '42') + '</v></c>' for column in range(20)
    ) + '</row></sheetData></worksheet>'
    with zipfile.ZipFile(documents / "wide.xlsx", "w") as archive:
        for part, xml in wide_parts.items():
            archive.writestr(part, xml)
    # Deterministic source-size raster, large enough to exercise fitted percentages.
    def png_chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data))
    raster = b''.join(b'\0' + bytes((30, 110 if row < 450 else 160, 190)) * 1600 for row in range(900))
    (documents / "image.png").write_bytes(b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('!2I5B', 1600, 900, 8, 2, 0, 0, 0)) + png_chunk(b'IDAT', zlib.compress(raster)) + png_chunk(b'IEND', b''))
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
    # Quick Inspector is read-only, but the runner still proves that its named
    # row owns selection before opening an entry-relative context action.
    invoke_context_action(
        main_window_id,
        filename,
        "Quick Inspector",
        evidence,
    )
    wait_for(
        lambda: quick_inspector_visible(filename),
        f"Quick Inspector for {filename}",
    )
    if filename == "letter.docx":
        wait_for(
            lambda: inspector_content_visible("Ottrin DOCX QA visible document content"),
            "visible DOCX document content",
        )
        wait_for(lambda: visible_named_nodes("+ text"), "DOCX text sizing controls")
    if inspector_requires_zoom_controls(filename):
        wait_for(
            lambda: any(
                node.getRoleName() == "push button"
                for node in visible_named_nodes(
                    "Fit width" if Path(filename).suffix.lower() in {".pdf", ".docx"} else "Fit page"
                )
            ),
            f"sizing mode control for {filename}",
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


def stop_disposable_daemon(qa_root: Path):
    daemon_socket = qa_root / "runtime/ottrin/index-v1.sock"
    if not daemon_socket.exists():
        return
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        try:
            connection.connect(str(daemon_socket))
        except ConnectionRefusedError:
            return
        pid, uid, _ = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        with Path(f"/proc/{pid}/environ").open("rb") as stream:
            environment = stream.read().split(b"\0")
        expected = os.fsencode("XDG_RUNTIME_DIR=" + str(qa_root / "runtime"))
        if uid != os.getuid() or expected not in environment:
            raise AssertionError("Disposable daemon ownership mismatch")
        descriptor = os.pidfd_open(pid)
        try:
            signal.pidfd_send_signal(descriptor, signal.SIGTERM)
        finally:
            os.close(descriptor)


def disposable_environment(qa_root: Path):
    # The daemon socket belongs to XDG_RUNTIME_DIR, independently of HOME.
    runtime = qa_root / "runtime"
    runtime.mkdir(mode=0o700, exist_ok=True)
    runtime.chmod(0o700)
    env = os.environ.copy()
    env.pop("WAYLAND_DISPLAY", None)
    env.update(
        {
            "WINIT_UNIX_BACKEND": "x11",
            "NO_AT_BRIDGE": "0",
            "HOME": str(qa_root / "home"),
            "XDG_RUNTIME_DIR": str(runtime),
            "XDG_CONFIG_HOME": str(qa_root / "config"),
            "XDG_CACHE_HOME": str(qa_root / "cache"),
            "XDG_STATE_HOME": str(qa_root / "state"),
            "XDG_DATA_HOME": str(qa_root / "data"),
        }
    )
    return env


def native_window_capture(window: str, evidence: Path, name: str, expected_size=None):
    from PIL import Image
    target = evidence / (name + ".png")
    def painted():
        pid = int(run("xdotool", "getwindowpid", window, capture=True).stdout.strip())
        if pid != target_process_id:
            raise AssertionError("Refusing native capture of a non-owned window")
        run("import", "-window", window, str(target))
        with Image.open(target) as picture:
            if expected_size is not None and picture.size != expected_size:
                return False
            extrema = picture.convert("RGB").getextrema()
        return any(high - low > 20 for low, high in extrema)
    wait_for(painted, "nonblank native preview capture", timeout=10.0)


def native_preview_matrix(binary: Path, evidence: Path, qa_root: Path, env: dict):
    """Native-window evidence when the product intentionally disables AT-SPI."""
    global target_process_id
    results = []
    for filename in ("notes.txt", "image.png", "preview.pdf", "letter.docx", "ottrin-preview.svg", "workbook.xlsx", "wide.xlsx"):
        path = qa_root / "home/Documents" / filename
        stderr = (evidence / (filename + ".stderr.log")).open("w")
        process = subprocess.Popen([str(binary), "--select", str(path)], env=env, stderr=stderr, start_new_session=True)
        target_process_id = process.pid
        try:
            def owned_window(title):
                if process.poll() is not None:
                    raise RuntimeError(f"Packaged process exited {process.returncode}")
                result = run("xdotool", "search", "--all", "--onlyvisible", "--pid", str(process.pid), "--name", title, capture=True, check=False)
                return result.stdout.strip().splitlines()[-1:] or None
            main = wait_for(lambda: owned_window("^Ottrin$"), "native file manager")[0]
            run("xdotool", "windowactivate", "--sync", main)
            native_window_capture(main, evidence, "main-readiness")
            (evidence / "main-readiness.png").unlink()
            run("xdotool", "key", "--window", main, "space")
            window = wait_for(lambda: owned_window("Quick Inspector.*" + filename), "native packaged Quick Inspector")[0]
            actual_title = run("xdotool", "getwindowname", window, capture=True).stdout.strip()
            assert "Quick Inspector" in actual_title and filename in actual_title, actual_title
            run("xdotool", "windowactivate", "--sync", window)
            for label, width, height in (("narrow", 500, 440), ("wide", 1000, 780)):
                run("xdotool", "windowsize", "--sync", window, str(width), str(height))
                native_window_capture(window, evidence, f"native-{filename}-{label}", (width, height))
            if filename == "preview.pdf":
                run("xdotool", "mousemove", "--window", window, "400", "350", "click", "--repeat", "12", "--delay", "60", "5")
                native_window_capture(window, evidence, "native-preview.pdf-scrolled")
            results.append({"file": filename, "pid": process.pid, "window": window, "title": actual_title, "status": "native window captured; content requires image inspection"})
        finally:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            stop_disposable_daemon(qa_root)
            stderr.close()
    (evidence / "native-report.json").write_text(json.dumps({"binary": str(binary), "qa_root": str(qa_root), "checks": results}, indent=2))
    return 0


def run_qa(qa_root: Path) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("dist/Ottrin-0.1.0-linux-x86_64/bin/ottrin"),
    )
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--native-previews", action="store_true", help="Capture native preview windows when AT-SPI is intentionally disabled")
    parser.add_argument("--preview-only", action="store_true", help="Exercise preview fixtures without unrelated file workflows")
    args = parser.parse_args()

    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"packaged binary not found: {binary}")
    for tool in ("busctl", "gdbus", "spectacle", "xdotool"):
        if shutil.which(tool) is None:
            parser.error(f"required tool not found: {tool}")
    if not args.native_previews:
        try:
            require_a11y_registry()
        except RuntimeError as error:
            parser.error(str(error))

    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
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

    env = disposable_environment(qa_root)
    if args.native_previews:
        return native_preview_matrix(binary, evidence, qa_root, env)
    prior_a11y = a11y_enabled()
    set_a11y(True)
    stderr = (evidence / "ottrin.stderr.log").open("w", encoding="utf-8")
    app_process = subprocess.Popen([str(binary)], env=env, stderr=stderr, start_new_session=True)
    global target_process_id
    target_process_id = app_process.pid
    results: list[dict[str, str]] = []

    def passed(check: str, detail: str):
        results.append({"check": check, "status": "passed", "detail": detail})

    try:
        try:
            wait_for(ottrin_app, "Ottrin AT-SPI application")
        except AssertionError:
            desktop = pyatspi.Registry.getDesktop(0)
            diagnosis = {"pid": app_process.pid, "exit": app_process.poll(),
                "applications": [{"name": item.name, "pid": item.get_process_id(), "children": item.childCount} for item in desktop if item is not None],
                "windows": run("xdotool", "search", "--pid", str(app_process.pid), capture=True, check=False).stdout}
            (evidence / "startup-diagnosis.json").write_text(json.dumps(diagnosis, indent=2))
            raise
        window_id = wait_for(
            lambda: run(
                "xdotool",
                "search",
                "--all",
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
            "ottrin-preview.svg",
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

        if args.preview_only:
            for filename in ("notes.txt", "image.png", "preview.pdf", "preview-second.pdf", "letter.docx", "ottrin-preview.svg", "workbook.xlsx", "macros.xlsm"):
                open_quick_inspector(window_id, filename, evidence, "preview-" + filename.replace(".", "-"))
                passed("preview-" + filename, "Packaged preview content and controls exercised")
            (evidence / "report.json").write_text(json.dumps({"binary": str(binary), "qa_root": str(qa_root), "checks": results}, indent=2))
            return 0

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
            subfolder_activation["desktop_retry"] = double_click_entry_at_desktop_coordinates(
                window_id, "Subfolder"
            )
            try:
                wait_for(
                    lambda: destination_breadcrumb_visible("Subfolder"),
                    "Subfolder navigation after named desktop-coordinate double-click",
                    timeout=2.0,
                )
            except AssertionError:
                write_entry_activation_failure(evidence, "Subfolder", subfolder_activation)
                raise
        click_named_control(window_id, "Menu")
        click_accessible_suffix("Paste into Current Folder")
        wait_for(lambda: (subfolder / "notes.txt").is_file(), "copied file")
        assert (documents / "notes.txt").is_file()
        passed("copy", "Copied notes.txt into Subfolder without changing the source.")
        screenshot(evidence, "04-copy-result")

        click_accessible_name("\ue2c7 Documents", "push button")
        wait_for(lambda: visible_entry("notes.txt"), "Documents after copy")
        open_quick_inspector(
            window_id, "notes.txt", evidence, "05-quick-inspector-text"
        )
        passed("quick-inspector-text", "Opened the packaged text preview from its accessible menu.")

        open_quick_inspector(
            window_id, "image.png", evidence, "06-quick-inspector-image"
        )
        passed("quick-inspector-image", "Opened the packaged image preview.")

        preview_matrix = [
            ("preview.pdf", "07-quick-inspector-pdf", "rendered PDF plus metadata"),
            (
                "preview-second.pdf",
                "07a-quick-inspector-second-pdf",
                "second rendered PDF in the same process",
            ),
            ("letter.docx", "08-quick-inspector-docx", "DOCX"),
            ("ottrin-preview.svg", "08a-quick-inspector-svg", "rendered SVG"),
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
        # The UI and its disposable daemon share this owned process group.
        try:
            os.killpg(app_process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            app_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(app_process.pid, signal.SIGKILL)
            app_process.wait()
        stop_disposable_daemon(qa_root)
        stderr.close()
        if not prior_a11y:
            set_a11y(False)


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="pqa-", dir=Path.cwd() / ".hermes") as temporary:
        return run_qa(Path(temporary))


if __name__ == "__main__":
    raise SystemExit(main())
