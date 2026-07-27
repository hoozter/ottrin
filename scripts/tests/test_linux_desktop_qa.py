#!/usr/bin/python3
"""Focused regressions for the Linux packaged-desktop QA prerequisites."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).parents[1] / "linux-desktop-qa.py"
SPEC = importlib.util.spec_from_file_location("linux_desktop_qa", SCRIPT)
assert SPEC and SPEC.loader
qa = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(qa)


class A11yRegistryPreflightTests(unittest.TestCase):
    def test_rejects_missing_registry_cache_before_launch(self):
        missing_registry = subprocess.CompletedProcess(
            ["busctl"], 1, stdout="", stderr="Failed to introspect object"
        )
        with patch.object(qa, "run", return_value=missing_registry):
            with self.assertRaisesRegex(RuntimeError, "registry/cache"):
                qa.require_a11y_registry()

    def test_accepts_reachable_registry_cache(self):
        reachable_registry = subprocess.CompletedProcess(
            ["busctl"], 0, stdout="NAME TYPE SIGNATURE RESULT/FLAGS\n", stderr=""
        )
        with patch.object(qa, "run", return_value=reachable_registry):
            qa.require_a11y_registry()


if __name__ == "__main__":
    unittest.main()
