# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The three-way exit code.

Collapsing "could not run" into "failed" is what makes a probe suite
untrustworthy, so the distinction is pinned here rather than left to a reader's
discipline.
"""

from __future__ import annotations

import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe.report import EXIT_ABSENT, EXIT_ERROR, EXIT_PASS, Report  # noqa: E402


def report(title=None) -> Report:
    return Report(title, stream=io.StringIO())


class ExitCodeTests(unittest.TestCase):
    def test_no_checks_at_all_is_a_pass(self):
        self.assertEqual(report().finish(), EXIT_PASS)

    def test_all_checks_pass(self):
        r = report()
        r.check(True, "one")
        r.check(1 == 1, "two")
        self.assertEqual(r.finish(), EXIT_PASS)

    def test_a_failed_check_is_absent_not_error(self):
        r = report()
        r.check(True, "one")
        r.check(False, "the toolbar collapsed")
        self.assertEqual(r.finish(), EXIT_ABSENT)

    def test_an_error_outranks_a_failed_check(self):
        r = report()
        r.check(False, "behaviour missing")
        r.error("the app never launched")
        self.assertEqual(r.finish(), EXIT_ERROR)

    def test_an_error_alone_is_an_error(self):
        r = report()
        r.error("no bridge")
        self.assertEqual(r.finish(), EXIT_ERROR)

    def test_notes_never_change_the_outcome(self):
        r = report()
        r.check(True, "fine")
        r.note("two other instances were running")
        self.assertEqual(r.finish(), EXIT_PASS)

    def test_check_returns_its_own_verdict(self):
        r = report()
        self.assertTrue(r.check(True, "yes"))
        self.assertFalse(r.check(False, "no"))

    def test_check_coerces_truthiness(self):
        r = report()
        r.check([], "an empty list is false")
        r.check([1], "a non-empty list is true")
        self.assertEqual(r.failed, ["an empty list is false"])
        self.assertEqual(r.passed, ["a non-empty list is true"])


class SummaryTests(unittest.TestCase):
    def test_pass_summary_counts_checks(self):
        r = report()
        r.check(True, "a")
        r.check(True, "b")
        self.assertIn("PASS (2 checks)", r.summary())

    def test_absent_summary_lists_what_was_missing(self):
        r = report()
        r.check(False, "the Save button is enabled")
        summary = r.summary()
        self.assertIn("ABSENT (1)", summary)
        self.assertIn("the Save button is enabled", summary)

    def test_error_summary_says_the_probe_could_not_run(self):
        r = report()
        r.error("no automation bridge within 60s")
        self.assertIn("could not run", r.summary())

    def test_output_is_written_as_it_goes(self):
        stream = io.StringIO()
        r = Report("my probe", stream=stream)
        r.check(True, "ok thing")
        r.check(False, "bad thing")
        text = stream.getvalue()
        self.assertIn("== my probe ==", text)
        self.assertIn("ok    ok thing", text)
        self.assertIn("FAIL  bad thing", text)


if __name__ == "__main__":
    unittest.main()
