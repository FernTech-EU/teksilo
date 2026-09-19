# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The generated tool surface, and that it is still in step with the Rust source.

The Rust side asserts the same thing from the other direction
(`crates/teksilo-automation/src/mcp_schema.rs`), so the two cannot drift in
either direction without something going red.
"""

from __future__ import annotations

import inspect
import subprocess
import sys
import unittest
from pathlib import Path

PROBE = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROBE))

from teksilo_probe import tools  # noqa: E402

GENERATOR = PROBE / "generate_tools.py"


class SurfaceTests(unittest.TestCase):
    def test_every_name_has_a_function(self):
        for name in tools.TOOL_NAMES:
            self.assertTrue(callable(getattr(tools, name, None)), name)

    def test_names_are_unique(self):
        self.assertEqual(len(set(tools.TOOL_NAMES)), len(tools.TOOL_NAMES))

    def test_exports_match_the_names(self):
        exported = set(tools.__all__) - {"TOOL_NAMES", "MUTATING"}
        self.assertEqual(exported, set(tools.TOOL_NAMES))

    def test_mutating_is_a_subset(self):
        self.assertTrue(tools.MUTATING.issubset(set(tools.TOOL_NAMES)))
        self.assertIn("inject_key", tools.MUTATING)
        self.assertNotIn("snapshot_tree", tools.MUTATING)

    def test_every_function_takes_the_session_first(self):
        for name in tools.TOOL_NAMES:
            params = list(inspect.signature(getattr(tools, name)).parameters)
            self.assertEqual(params[0], "session", name)


class ArgumentNameTests(unittest.TestCase):
    """The trap a generated surface closes.

    `inject_pointer` has both a `kind` (mouse / touch / pen) and an `action`
    (click / double_click / down / up / move). A harness that passed
    `kind="click"` was asking for an unknown pointer kind and working only
    because `action` defaults to `click`.
    """

    def signature(self, name):
        return inspect.signature(getattr(tools, name)).parameters

    def test_inject_pointer_separates_action_from_kind(self):
        params = self.signature("inject_pointer")
        self.assertIn("action", params)
        self.assertIn("kind", params)
        self.assertIn("x", params)
        self.assertIn("y", params)

    def test_a_wrong_argument_name_is_a_typeerror_here(self):
        with self.assertRaises(TypeError):
            tools.inject_pointer(None, x=1.0, y=2.0, mode="click")

    def test_required_arguments_are_required(self):
        with self.assertRaises(TypeError):
            tools.inject_key(None)

    def test_advance_clock_has_no_settle(self):
        """Mutating, but its parameter struct has no `settle` — and
        `deny_unknown_fields` would refuse one. Derived, not assumed."""
        self.assertIn("advance_clock", tools.MUTATING)
        self.assertNotIn("settle", self.signature("advance_clock"))

    def test_mutating_tools_that_do_take_settle_expose_it(self):
        for name in ("invoke_action", "inject_key", "type_text", "scroll"):
            self.assertIn("settle", self.signature(name), name)

    def test_window_id_is_available_everywhere(self):
        for name in tools.TOOL_NAMES:
            self.assertIn("window_id", self.signature(name), name)


class OmissionTests(unittest.TestCase):
    def test_none_arguments_are_dropped_not_sent_as_null(self):
        """`deny_unknown_fields` plus `Option` means an explicit null and an
        omission are different, and only omission takes the Rust default."""
        sent = {}

        class Recorder:
            def call(self, tool, **args):
                sent.update({"tool": tool, "args": args})
                return {}

        tools.scroll(Recorder(), node=7, dy=-240.0)
        self.assertEqual(sent["tool"], "scroll")
        self.assertEqual(sent["args"], {"node": 7, "dy": -240.0})

    def test_false_is_not_dropped(self):
        sent = {}

        class Recorder:
            def call(self, tool, **args):
                sent.update(args)
                return {}

        tools.layout_tree(Recorder(), include_debug=False)
        self.assertEqual(sent, {"include_debug": False})

    def test_zero_is_not_dropped(self):
        sent = {}

        class Recorder:
            def call(self, tool, **args):
                sent.update(args)
                return {}

        tools.advance_clock(Recorder(), millis=0)
        self.assertEqual(sent, {"millis": 0})


class RegenerationTests(unittest.TestCase):
    def test_the_committed_file_matches_the_rust_sources(self):
        """Skipped outside a teksilo checkout — the sources are not shipped."""
        done = subprocess.run([sys.executable, str(GENERATOR), "--check"],
                              capture_output=True, text=True, cwd=str(PROBE))
        if "cannot find" in (done.stderr or ""):
            self.skipTest("not inside a teksilo checkout")
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    def test_the_generator_is_deterministic(self):
        first = subprocess.run([sys.executable, str(GENERATOR), "--stdout"],
                               capture_output=True, text=True, cwd=str(PROBE))
        if first.returncode != 0:
            self.skipTest("not inside a teksilo checkout")
        second = subprocess.run([sys.executable, str(GENERATOR), "--stdout"],
                                capture_output=True, text=True, cwd=str(PROBE))
        self.assertEqual(first.stdout, second.stdout)


if __name__ == "__main__":
    unittest.main()
