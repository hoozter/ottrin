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


if __name__ == "__main__":
    unittest.main()
