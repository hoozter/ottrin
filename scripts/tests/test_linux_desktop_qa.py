#!/usr/bin/python3
"""Focused regressions for the Linux packaged-desktop QA prerequisites."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch


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


class X11CoordinateTranslationTests(unittest.TestCase):
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

    def test_double_click_targets_the_named_entry_inside_its_x11_window(self):
        commands = []
        with (
            patch.object(qa, "wait_for", return_value=object()),
            patch.object(qa, "x11_point_for_node", return_value=(280, 330)),
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
