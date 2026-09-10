#!/usr/bin/python3
"""Focused regressions for the Linux packaged-desktop QA prerequisites."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import zipfile


SCRIPT = Path(__file__).parents[1] / "linux-desktop-qa.py"
SPEC = importlib.util.spec_from_file_location("linux_desktop_qa", SCRIPT)
assert SPEC and SPEC.loader
qa = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qa)


class A11yRegistryPreflightTests(unittest.TestCase):
    def test_disposable_environment_never_inherits_daemon_runtime(self):
        with tempfile.TemporaryDirectory() as root:
            with patch.dict(qa.os.environ, {"XDG_RUNTIME_DIR": "/run/user/1000", "WAYLAND_DISPLAY": "host"}):
                env = qa.disposable_environment(Path(root))
            runtime = Path(env["XDG_RUNTIME_DIR"])
            self.assertEqual(runtime, Path(root) / "runtime")
            self.assertEqual(runtime.stat().st_mode & 0o777, 0o700)
            self.assertNotIn("WAYLAND_DISPLAY", env)
            self.assertEqual(env["HOME"], str(Path(root) / "home"))

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

    def test_records_context_action_failure_with_target_and_visible_popup_tree(self):
        evidence = Path(self.id().replace(".", "-"))
        writes = {}
        node = MagicMock()
        with (
            patch.object(
                qa,
                "visible_a11y_nodes",
                return_value=[
                    "toggle button | preview-second.pdf",
                    "menu item | Quick Inspector",
                ],
            ),
            patch.object(qa, "visible_entry", return_value=node),
            patch.object(qa, "entry_selected", return_value=False),
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ),
            patch.object(Path, "write_text", side_effect=lambda text, **_: writes.setdefault("text", text)),
        ):
            qa.write_context_action_failure(
                evidence,
                "preview-second.pdf",
                "Quick Inspector",
                AssertionError("selected entry remained preview.pdf"),
                "0x123",
            )

        self.assertIn('"filename": "preview-second.pdf"', writes["text"])
        self.assertIn('"action": "Quick Inspector"', writes["text"])
        self.assertIn("menu item | Quick Inspector", writes["text"])
        self.assertIn('"entry_selected": false', writes["text"])
        self.assertIn('"x11_window_relative_target"', writes["text"])

    def test_preview_fixtures_include_a_real_safe_svg_and_docx_content_marker(self):
        with tempfile.TemporaryDirectory() as temp:
            documents = Path(temp) / "Documents"
            documents.mkdir()
            qa.create_preview_fixtures(Path.cwd(), documents)

            svg = (documents / "ottrin-preview.svg").read_text(encoding="utf-8")
            self.assertIn("<svg", svg)
            self.assertIn("#00ff88", svg)
            with zipfile.ZipFile(documents / "letter.docx") as docx:
                self.assertIn(
                    b"Ottrin DOCX QA visible document content",
                    docx.read("word/document.xml"),
                )


class X11CoordinateTranslationTests(unittest.TestCase):
    def test_entry_selection_accepts_the_visible_status_label_when_a11y_omits_selected_state(self):
        node = MagicMock()
        node.getState.return_value.getStates.return_value = []
        with (
            patch.object(qa, "visible_entry", return_value=node),
            patch.object(
                qa,
                "visible_a11y_nodes",
                return_value=["label | 16 items  ·  notes.txt"],
            ),
        ):
            self.assertTrue(qa.entry_selected("notes.txt"))

    def test_pointer_selection_never_uses_accessibility_activation_or_keyboard_fallback(self):
        commands = []
        node = MagicMock()
        with (
            patch.object(qa, "visible_entry", return_value=node),
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ),
            patch.object(qa, "entry_selected", side_effect=[False, True]),
            patch.object(qa, "click_accessible_name") as accessibility_activation,
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.click_entry("0x123", "notes.txt")

        accessibility_activation.assert_not_called()
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

    def test_pointer_selection_retries_the_named_node_at_desktop_coordinates(self):
        commands = []
        node = MagicMock()
        node.queryComponent.return_value.getExtents.return_value = MagicMock(
            x=600, y=700, width=40, height=60
        )
        with (
            patch.object(
                qa,
                "wait_for",
                side_effect=[
                    node,
                    AssertionError("window-relative click missed"),
                    node,
                    object(),
                ],
            ),
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.click_entry("0x123", "notes.txt")

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
                ("xdotool", "windowactivate", "--sync", "0x123"),
                ("xdotool", "mousemove", "620", "730", "click", "1"),
            ],
        )

    def test_view_toggle_uses_the_checked_x11_pointer_without_accessibility_activation(self):
        commands = []
        node = MagicMock()
        with (
            patch.object(qa, "wait_for", side_effect=[node, object()]) as wait_for,
            patch.object(
                qa,
                "view_toggle_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ) as diagnostics,
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.click_view_toggle("0x123", qa.VIEW_TO_LIST, qa.VIEW_TO_GRID)

        node.queryAction.assert_not_called()
        diagnostics.assert_called_once_with("0x123", node)
        self.assertEqual(wait_for.call_count, 2)
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

    def test_context_menu_uses_the_checked_x11_target_only_after_named_selection(self):
        commands = []
        node = MagicMock()
        with (
            patch.object(qa, "click_entry") as click_entry,
            patch.object(qa, "wait_for", return_value=node),
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ) as diagnostics,
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.context_menu_entry("0x123", "preview-second.pdf")

        click_entry.assert_called_once_with("0x123", "preview-second.pdf")
        diagnostics.assert_called_once()
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
                    "3",
                ),
            ],
        )

    def test_context_menu_proves_selection_after_the_named_context_click_when_primary_selection_fails(self):
        commands = []
        node = MagicMock()
        with (
            patch.object(qa, "click_entry", side_effect=AssertionError("primary click missed")),
            patch.object(
                qa,
                "wait_for",
                side_effect=[node, object()],
            ) as wait_for,
            patch.object(
                qa,
                "entry_x11_diagnostics",
                return_value={"x11_window_relative_target": {"x": 280, "y": 330}},
            ),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            qa.context_menu_entry("0x123", "image.png")

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
                    "3",
                ),
            ],
        )
        self.assertEqual(wait_for.call_count, 2)
        self.assertIn("after named context click", wait_for.call_args_list[-1].args[1])

    def test_open_search_uses_checked_x11_pointer_without_accessibility_activation(self):
        commands = []
        toggle = MagicMock()
        with (
            patch.object(qa, "wait_for", side_effect=[toggle, object()]) as wait_for,
            patch.object(qa, "x11_point_for_node", return_value=(280, 330)),
            patch.object(
                qa,
                "run",
                side_effect=lambda *args, **kwargs: commands.append(args),
            ),
        ):
            self.assertIsNotNone(qa.open_search("0x123"))

        toggle.queryAction.assert_not_called()
        self.assertEqual(wait_for.call_count, 2)
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


    def test_double_click_retry_targets_the_named_entry_at_desktop_coordinates(self):
        commands = []
        node = MagicMock()
        node.queryComponent.return_value.getExtents.return_value = MagicMock(
            x=600, y=700, width=40, height=60
        )
        with (
            patch.object(qa, "wait_for", return_value=node),
            patch.object(qa, "run", side_effect=lambda *args, **kwargs: commands.append(args)),
            patch.object(qa.time, "sleep"),
        ):
            diagnostics = qa.double_click_entry_at_desktop_coordinates("0x123", "Subfolder")

        self.assertEqual(diagnostics, {"a11y_desktop_target": {"x": 620, "y": 730}})
        self.assertEqual(
            commands,
            [
                ("xdotool", "windowactivate", "--sync", "0x123"),
                (
                    "xdotool",
                    "mousemove",
                    "620",
                    "730",
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

    def test_native_docx_requires_flow_controls_instead_of_raster_controls(self):
        self.assertFalse(qa.inspector_requires_zoom_controls("letter.docx"))
        self.assertTrue(qa.inspector_requires_zoom_controls("preview.pdf"))
        self.assertTrue(qa.inspector_requires_zoom_controls("ottrin-preview.svg"))

    def test_docx_screenshot_waits_for_visible_native_content(self):
        events = []
        with (
            patch.object(qa, "invoke_context_action"),
            patch.object(qa, "quick_inspector_visible", return_value=True),
            patch.object(qa, "inspector_content_visible", return_value=True),
            patch.object(qa, "visible_named_nodes", return_value=[]),
            patch.object(qa, "wait_for", side_effect=lambda predicate, description: events.append(("wait", description))),
            patch.object(qa, "screenshot", side_effect=lambda *args: events.append(("screenshot",))),
            patch.object(qa, "close_quick_inspector"),
        ):
            qa.open_quick_inspector("0x123", "letter.docx", Path("/tmp"), "letter")
        self.assertLess(events.index(("wait", "visible DOCX document content")), events.index(("screenshot",)))
        self.assertNotIn(("wait", "rendered DOCX page"), events)

    def test_inspector_content_requires_the_visible_document_marker(self):
        with patch.object(
            qa,
            "visible_a11y_nodes",
            return_value=["label | Ottrin DOCX QA visible document content"],
        ):
            self.assertTrue(qa.inspector_content_visible("Ottrin DOCX QA visible document content"))
        with patch.object(qa, "visible_a11y_nodes", return_value=["label | letter.docx"]):
            self.assertFalse(qa.inspector_content_visible("Ottrin DOCX QA visible document content"))

    def test_closing_quick_inspector_dismisses_its_parent_context_menu(self):
        with (
            patch.object(qa, "xkey") as xkey,
            patch.object(qa, "wait_for") as wait_for,
        ):
            qa.close_quick_inspector("0x123", "preview.pdf", dismiss_parent_menu=True)

        self.assertEqual(
            xkey.call_args_list,
            [
                (("0x123", "space"), {}),
                (("0x123", "Escape"), {}),
            ],
        )
        self.assertEqual(wait_for.call_count, 2)

    def test_closing_quick_inspector_recovers_when_space_is_swallowed(self):
        with (
            patch.object(qa, "xkey") as xkey,
            patch.object(
                qa,
                "wait_for",
                side_effect=[AssertionError("Quick Inspector close timed out"), object()],
            ) as wait_for,
        ):
            qa.close_quick_inspector("0x123", "image.png")

        self.assertEqual(
            xkey.call_args_list,
            [
                (("0x123", "space"), {}),
                (("0x123", "Escape"), {}),
            ],
        )
        self.assertEqual(wait_for.call_count, 2)

    def test_quick_inspector_close_requires_stable_absence(self):
        with (
            patch.object(qa, "quick_inspector_visible", side_effect=[False, True]),
            patch.object(qa.time, "sleep"),
        ):
            self.assertFalse(qa.quick_inspector_quiescent("image.png"))

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
