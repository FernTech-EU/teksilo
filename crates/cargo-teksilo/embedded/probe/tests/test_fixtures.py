# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Scratch space, and the refusal to hand back a path inside the repository."""

from __future__ import annotations

import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from teksilo_probe import fixtures  # noqa: E402


class ScratchTests(unittest.TestCase):
    def test_honours_the_scratch_environment_variable(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / "session-scratch"
            with mock.patch.dict(os.environ, {fixtures.SCRATCH_ENV: str(target)}):
                self.assertEqual(fixtures.scratch_dir(), target)
                self.assertTrue(target.is_dir())

    def test_falls_back_to_the_system_temp_dir(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop(fixtures.SCRATCH_ENV, None)
            self.assertTrue(fixtures.scratch_dir().is_dir())

    def test_paths_carry_the_pid_so_parallel_probes_do_not_collide(self):
        self.assertIn(str(os.getpid()), fixtures.scratch_path("thing").name)


class WorkingCopyTests(unittest.TestCase):
    def test_copies_a_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "doc.skrib"
            source.write_text("contents")
            with mock.patch.dict(os.environ, {fixtures.SCRATCH_ENV: str(Path(tmp) / "s")}):
                copy = fixtures.working_copy(source, guard_root=tmp + "/repo")
            self.assertNotEqual(Path(copy), source)
            self.assertEqual(Path(copy).read_text(), "contents")

    def test_copies_a_directory_whole(self):
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "project"
            (source / "inner").mkdir(parents=True)
            (source / "inner" / "a.txt").write_text("a")
            with mock.patch.dict(os.environ, {fixtures.SCRATCH_ENV: str(Path(tmp) / "s")}):
                copy = fixtures.working_copy(source, guard_root=tmp + "/repo")
            self.assertEqual((Path(copy) / "inner" / "a.txt").read_text(), "a")

    def test_a_read_only_fixture_becomes_writable(self):
        """copy2 preserves the mode, and a fixture is kept read-only precisely
        so a stray write fails loudly — the protection must not follow the copy."""
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "locked.skrib"
            source.write_text("x")
            source.chmod(0o444)
            with mock.patch.dict(os.environ, {fixtures.SCRATCH_ENV: str(Path(tmp) / "s")}):
                copy = fixtures.working_copy(source, guard_root=tmp + "/repo")
            self.assertTrue(os.stat(copy).st_mode & stat.S_IWUSR)

    def test_a_missing_fixture_names_the_path(self):
        with self.assertRaises(FileNotFoundError):
            fixtures.working_copy("/definitely/not/here.skrib")

    def test_a_copy_inside_the_project_is_refused(self):
        """A live-app probe rewrites what it opens; a fixture in the repo would
        be silently corrupted for the next run, and the probe would still pass."""
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / "scratch").mkdir(parents=True)
            source = Path(tmp) / "doc.skrib"
            source.write_text("x")
            with mock.patch.dict(os.environ,
                                 {fixtures.SCRATCH_ENV: str(repo / "scratch")}):
                with self.assertRaises(RuntimeError) as caught:
                    fixtures.working_copy(source, guard_root=repo)
            self.assertIn("inside the project", str(caught.exception))

    def test_a_stale_copy_from_a_previous_run_is_replaced(self):
        with tempfile.TemporaryDirectory() as tmp:
            scratch = Path(tmp) / "s"
            source = Path(tmp) / "doc.skrib"
            source.write_text("new")
            with mock.patch.dict(os.environ, {fixtures.SCRATCH_ENV: str(scratch)}):
                first = fixtures.working_copy(source, guard_root=tmp + "/repo")
                Path(first).write_text("stale")
                second = fixtures.working_copy(source, guard_root=tmp + "/repo")
            self.assertEqual(first, second)
            self.assertEqual(Path(second).read_text(), "new")


if __name__ == "__main__":
    unittest.main()
