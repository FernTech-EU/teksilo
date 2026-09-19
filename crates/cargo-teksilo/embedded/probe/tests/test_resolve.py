# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Version comparison, and reading teksilo's version out of the resolved graph."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import resolve  # noqa: E402


class VersionTupleTests(unittest.TestCase):
    def test_plain_versions(self):
        self.assertEqual(resolve.version_tuple("0.9.4")[:3], (0, 9, 4))
        self.assertEqual(resolve.version_tuple("1.0.0")[:3], (1, 0, 0))

    def test_a_version_line_is_accepted_whole(self):
        self.assertEqual(
            resolve.version_tuple("teksilo-automation-mcp 0.12.1")[:3], (0, 12, 1))

    def test_a_missing_patch_component_is_zero(self):
        self.assertEqual(resolve.version_tuple("0.12")[:3], (0, 12, 0))

    def test_ordering(self):
        self.assertLess(resolve.version_tuple("0.9.3"), resolve.version_tuple("0.9.4"))
        self.assertLess(resolve.version_tuple("0.9.9"), resolve.version_tuple("0.10.0"))
        self.assertLess(resolve.version_tuple("0.12.1"), resolve.version_tuple("1.0.0"))

    def test_a_prerelease_sorts_below_its_release(self):
        """The obvious per-chunk parse reads `0.10.0-rc.1` as *newer*."""
        self.assertLess(resolve.version_tuple("0.10.0-rc.1"),
                        resolve.version_tuple("0.10.0"))
        self.assertLess(resolve.version_tuple("0.9.9"),
                        resolve.version_tuple("0.10.0-rc.1"))

    def test_junk_is_not_a_crash(self):
        self.assertEqual(resolve.version_tuple(None), ())
        self.assertEqual(resolve.version_tuple(""), ())
        self.assertEqual(resolve.version_tuple("not-a-version"), ())


class TeksiloVersionTests(unittest.TestCase):
    """The bug this replaces: `teksilo = { workspace = true }` read as `None`.

    The prototype parsed `[dependencies].teksilo` and understood a bare string
    and `{ version = "…" }`. Every other shape — a workspace dependency, which
    is the modern default, a path dependency, a `[patch]` — answered `None`, so
    the stale-client check became a silent no-op. Reading the resolved graph
    answers all of them.
    """

    def metadata(self, *versions: str, target="/tmp/target") -> dict:
        return {
            "target_directory": target,
            "workspace_members": ["app 0.1.0 (path+file:///app)"],
            "packages": [
                {"id": "app 0.1.0 (path+file:///app)", "name": "app", "version": "0.1.0",
                 "targets": [{"kind": ["bin"], "name": "app"}]},
                *[{"id": f"teksilo {v}", "name": "teksilo", "version": v,
                   "targets": [{"kind": ["lib"], "name": "teksilo"}]}
                  for v in versions],
            ],
        }

    def test_reads_the_resolved_version(self):
        with mock.patch.object(resolve, "cargo_metadata",
                               return_value=self.metadata("0.12.1")):
            self.assertEqual(resolve.teksilo_version(), "0.12.1")

    def test_a_workspace_dependency_is_not_special(self):
        """The shape the manifest states it in never reaches this code at all."""
        with mock.patch.object(resolve, "cargo_metadata",
                               return_value=self.metadata("0.12.1")):
            self.assertEqual(resolve.teksilo_version(), "0.12.1")

    def test_two_majors_in_one_graph_report_the_newer(self):
        with mock.patch.object(resolve, "cargo_metadata",
                               return_value=self.metadata("0.12.1", "1.0.0")):
            self.assertEqual(resolve.teksilo_version(), "1.0.0")

    def test_no_teksilo_is_none_not_an_error(self):
        with mock.patch.object(resolve, "cargo_metadata", return_value=self.metadata()):
            self.assertIsNone(resolve.teksilo_version())

    def test_cargo_failing_is_none_not_an_error(self):
        with mock.patch.object(resolve, "cargo_metadata",
                               side_effect=resolve.ResolveError("no cargo")):
            self.assertIsNone(resolve.teksilo_version())

    def test_binary_targets_come_from_the_workspace_members(self):
        with mock.patch.object(resolve, "cargo_metadata",
                               return_value=self.metadata("0.12.1")):
            self.assertEqual(resolve.app_bin_names(), ["app"])


class CheckVersionTests(unittest.TestCase):
    def test_an_older_client_is_refused(self):
        with mock.patch.object(resolve, "mcp_version", return_value="0.9.2"), \
                mock.patch.object(resolve, "_wants_install", return_value=False):
            with self.assertRaises(resolve.ResolveError) as caught:
                resolve.check_mcp_version("/bin/mcp", "0.12.1", "cargo install ...")
        message = str(caught.exception)
        self.assertIn("0.9.2", message)
        self.assertIn("0.12.1", message)

    def test_a_newer_client_is_fine(self):
        with mock.patch.object(resolve, "mcp_version", return_value="0.13.0"):
            resolve.check_mcp_version("/bin/mcp", "0.12.1", "cargo install ...")

    def test_an_equal_client_is_fine(self):
        with mock.patch.object(resolve, "mcp_version", return_value="0.12.1"):
            resolve.check_mcp_version("/bin/mcp", "0.12.1", "cargo install ...")

    def test_an_unknown_app_version_skips_the_check(self):
        """An unanswerable question is not a failure."""
        with mock.patch.object(resolve, "mcp_version",
                               side_effect=AssertionError("must not be asked")):
            resolve.check_mcp_version("/bin/mcp", None, "cargo install ...")

    def test_an_unaskable_client_skips_the_check(self):
        with mock.patch.object(resolve, "mcp_version", return_value=None):
            resolve.check_mcp_version("/bin/mcp", "0.12.1", "cargo install ...")


class InstallCommandTests(unittest.TestCase):
    def test_pins_the_version_and_locks(self):
        cmd = resolve.mcp_install_command("0.12.1")
        self.assertEqual(
            cmd,
            ["cargo", "install", "teksilo-automation-mcp", "--version", "0.12.1",
             "--locked"])

    def test_no_version_still_locks(self):
        self.assertEqual(resolve.mcp_install_command(None),
                         ["cargo", "install", "teksilo-automation-mcp", "--locked"])


class ProjectDirTests(unittest.TestCase):
    def test_walks_up_to_a_cargo_toml(self):
        found = resolve.project_dir()
        self.assertTrue((found / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
