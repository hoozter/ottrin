#!/usr/bin/python3
"""Focused regressions for the Linux packaged-desktop QA prerequisites."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch


SCRIPT = Path(__file__).parents[1] / "linux-desktop-qa.py"
SPEC = importlib.util.spec_from_file_location("linux_desktop_qa", SCRIPT)
assert SPEC and SPEC.loader
qa = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qa)


class A11yRegistryPreflightTests(unittest.TestCase):
    def test_rejects_missing_registry_cache_before_launch(self):
        with patch.object(
            qa.pyatspi.Registry,
            "getDesktop",
            side_effect=RuntimeError("accessibility bus is unavailable"),
        ):
            with self.assertRaisesRegex(RuntimeError, "registry"):
                qa.require_a11y_registry()

    def test_accepts_reachable_registry_cache(self):
        with patch.object(qa.pyatspi.Registry, "getDesktop", return_value=object()):
            qa.require_a11y_registry()

    def test_ignores_a_transient_empty_desktop_child(self):
        with patch.object(qa.pyatspi.Registry, "getDesktop", return_value=[None]):
            self.assertIsNone(qa.ottrin_app())

    def test_records_workflow_failure_with_fixture_state(self):
        evidence = Path(self.id().replace(".", "-"))
        qa_root = Path("/tmp/ottrin-desktop-qa-fixture")
        writes = {}

        with (
            patch.object(qa, "visible_a11y_nodes", return_value=["push button | Delete Permanently"]),
            patch.object(Path, "exists", return_value=True),
            patch.object(Path, "write_text", side_effect=lambda text, **_: writes.setdefault("text", text)),
        ):
            qa.write_workflow_failure(evidence, qa_root, AssertionError("timed out"))

        self.assertIn('"error": "timed out"', writes["text"])
        self.assertIn('"delete_me_exists": true', writes["text"])
        self.assertIn("push button | Delete Permanently", writes["text"])


class X11CoordinateTranslationTests(unittest.TestCase):
    def test_open_search_retries_with_checked_x11_target_when_a11y_does_not_focus(self):
        commands = []
        toggle = MagicMock()
        toggle.queryAction.return_value.doAction.return_value = True
        with (
            patch.object(
                qa,
                "wait_for",
                side_effect=[toggle, AssertionError("no focus"), object()],
            ),
            patch.object(qa, "x11_point_for_node", return_value=(280, 330)),
            patch.object(
                qa,
                "run",
                side_effect=lambda *args, **kwargs: commands.append(args),
            ),
        ):
            self.assertIsNotNone(qa.open_search("0x123"))

        self.assertEqual(
            commands,
            [
                ("xdotool", "windowactivate", "--sync", "0x123"),
                (
                    "xdotool",
                    "mousemove",
                    "--window",
                    "0x123",
                    "280",
                    "330",
                    "click",
                    "1",
                ),
            ],
        )

    def test_maps_entry_center_through_the_accessible_window(self):
        self.assertEqual(
            qa.x11_point_for_entry(
                component=(200, 350, 80, 30),
                app=(100, 200, 400, 300),
                window={"X": 500, "Y": 300, "WIDTH": 800, "HEIGHT": 600},
            ),
            (280, 330),
        )

    def test_ctrl_click_keeps_control_held_for_the_second_entry_click(self):
        commands = []
        with (
            patch.object(qa, "wait_for", return_value=object()),
            patch.object(qa, "x11_point_for_node", return_value=(280, 330)),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.ctrl_click_entry("0x123", "notes.txt")

        self.assertEqual(
            commands,
            [
                ("xdotool", "windowactivate", "--sync", "0x123"),
                (
                    "xdotool",
                    "keydown",
                    "Control_L",
                    "mousemove",
                    "--window",
                    "0x123",
                    "280",
                    "330",
                    "click",
                    "1",
                    "keyup",
                    "Control_L",
                ),
            ],
        )

    def test_click_entry_retries_with_checked_x11_target_when_a11y_does_not_select(self):
        commands = []
        with (
            patch.object(qa, "click_accessible_name"),
            patch.object(qa, "entry_selected", side_effect=[False, True]),
            patch.object(qa, "wait_for", return_value=object()),
            patch.object(qa, "x11_point_for_node", return_value=(280, 330)),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.click_entry("0x123", "sample.json")

        self.assertEqual(
            commands,
            [
                ("xdotool", "windowactivate", "--sync", "0x123"),
                (
                    "xdotool",
                    "mousemove",
                    "--window",
                    "0x123",
                    "280",
                    "330",
                    "click",
                    "1",
                ),
            ],
        )

    def test_double_click_targets_the_named_entry_inside_its_x11_window(self):
        commands = []
        with (
            patch.object(qa, "wait_for", return_value=object()),
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.double_click_entry("0x123", "Subfolder")

        self.assertEqual(
            commands,
            [
                ("xdotool", "windowactivate", "--sync", "0x123"),
                (
                    "xdotool",
                    "mousemove",
                    "--window",
                    "0x123",
                    "280",
                    "330",
                    "click",
                    "--repeat",
                    "2",
                    "--delay",
                    "100",
                    "1",
                ),
            ],
        )

    def test_records_entry_activation_diagnostics_only_after_navigation_fails(self):
        evidence = Path(self.id().replace(".", "-"))
        diagnostics = {
            "accessible_entry": {"x": 200, "y": 350, "width": 80, "height": 30},
            "accessible_window": {"x": 100, "y": 200, "width": 400, "height": 300},
            "x11_window": {"X": 500, "Y": 300, "WIDTH": 800, "HEIGHT": 600},
            "x11_window_relative_target": {"x": 280, "y": 330},
        }
        visible = ["toggle button | Subfolder", "toggle button | notes.txt"]
        writes = {}

        with (
            patch.object(qa, "visible_a11y_nodes", return_value=visible),
            patch.object(Path, "write_text", side_effect=lambda text, **_: writes.setdefault("text", text)),
        ):
            qa.write_entry_activation_failure(evidence, "Subfolder", diagnostics)

        self.assertIn('"accessible_entry"', writes["text"])
        self.assertIn('"post_click_visible_nodes"', writes["text"])
        self.assertIn("toggle button | Subfolder", writes["text"])

    def test_confirms_empty_destination_from_its_visible_breadcrumb(self):
        with patch.object(
            qa,
            "visible_a11y_nodes",
            return_value=[
                "label | This folder is empty",
                "push button | \\ue2c7 Documents",
                "push button | \\ue2c7 Subfolder",
            ],
        ):
            self.assertTrue(qa.destination_breadcrumb_visible("Subfolder"))

    def test_docx_page_preview_does_not_claim_image_pdf_zoom_toolbar(self):
        self.assertFalse(qa.inspector_requires_zoom_controls("letter.docx"))
        self.assertTrue(qa.inspector_requires_zoom_controls("preview.pdf"))

    def test_maps_each_toggle_target_to_the_explicit_view_menu_item(self):
        self.assertEqual(qa.view_menu_label(qa.VIEW_TO_GRID), "List view")
        self.assertEqual(qa.view_menu_label(qa.VIEW_TO_COLUMN), "Grid view")
        self.assertEqual(qa.view_menu_label(qa.VIEW_TO_LIST), "Column view")

    def test_scales_a11y_component_into_the_x11_window(self):
        point = qa.x11_point_for_component(
            component=(110, 220, 20, 10),
            app=(100, 200, 400, 300),
            window={"X": 500, "Y": 300, "WIDTH": 800, "HEIGHT": 600},
        )

        self.assertEqual(point, (40, 50))

    def test_rejects_a_component_outside_the_accessible_window(self):
        with self.assertRaisesRegex(AssertionError, "outside"):
            qa.x11_point_for_component(
                component=(99, 220, 20, 10),
                app=(100, 200, 400, 300),
                window={"X": 500, "Y": 300, "WIDTH": 800, "HEIGHT": 600},
            )

if __name__ == "__main__":
    unittest.main()
