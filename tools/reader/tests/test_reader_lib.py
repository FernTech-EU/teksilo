# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The harness's own logic, which decides what counts as a finding.

    python3 -m unittest discover -s tools/reader/tests

No session, no application, no Orca: the Orca log lines here are copied from
real runs of Orca 46.1.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from reader_lib import audit, checks, keys, orca  # noqa: E402

LOG = """\
11:53:32.811178 - EVENT MANAGER: registering listener for: object:announcement
11:53:32.811289 - EVENT MANAGER: registering listener for: object:state-changed:focused
11:53:32.811300 - EVENT MANAGER: registering listener for: window:activate
11:53:34.763309 - NULL SPEECH: stop
11:53:34.763422 - SPEECH OUTPUT: 'frame.' {'established': False}
11:53:34.845749 - NULL SPEECH: stop
11:53:34.845916 - SPEECH OUTPUT: 'entry 05/02/2026 selected.' {'established': False}
11:53:41.253888 - EVENT MANAGER: object:announcement for [DEAD] in [application: 'datetime-pickers'] (1, 0, June 2026)
11:53:41.285503 - EVENT MANAGER: object:announcement for [DEAD] in [application: 'datetime-pickers'] (1, 0, June 2026) is not obsoleted
11:53:41.285889 - EVENT MANAGER: Ignoring defunct object: [DEAD]
11:53:45.154734 - EVENT MANAGER: object:announcement for [status bar: 'July 2026'] in [application: 'datetime-pickers'] (1, 0, July 2026)
11:53:45.268606 - SPEECH OUTPUT: 'July 2026' {'established': False}
11:53:49.041050 - SCRIPT MANAGER: Active script is: x (module=orca.scripts.default) orca.start > event_manager._onNoFocus > script_manager.getActiveScript
"""


class Keys(unittest.TestCase):
    def test_a_chord_holds_its_modifiers_around_the_key(self):
        self.assertEqual(keys.chord_steps("Ctrl+Shift+Z"), ["+29", "+42", "=44", "-42", "-29"])

    def test_names_are_case_blind_and_letters_are_us_keys(self):
        self.assertEqual(keys.chord_steps("tab"), keys.chord_steps("Tab"))
        self.assertEqual(keys.chord_steps("a"), ["=30"])
        self.assertEqual(keys.chord_steps("F10"), ["=68"])

    def test_a_non_modifier_before_the_last_key_is_refused(self):
        with self.assertRaises(keys.KeyError_):
            keys.chord_steps("A+B")

    def test_text_is_typed_with_shift_where_a_us_layout_needs_it(self):
        self.assertEqual(keys.text_steps("A!"), ["+42", "=30", "-42", "+42", "=2", "-42"])

    def test_text_a_us_layout_cannot_type_is_refused(self):
        with self.assertRaises(keys.KeyError_):
            keys.text_steps("é")


class OrcaLog(unittest.TestCase):
    def lines(self):
        return orca.lines_between(LOG, "00:00:00.000000", "23:59:59.999999")

    def test_what_orca_listens_to_is_read_from_its_log(self):
        self.assertEqual(orca.listened(LOG), {"object:announcement",
                                              "object:state-changed:focused",
                                              "window:activate"})

    def test_a_listened_type_covers_its_details_and_nothing_else(self):
        types = {"object:state-changed:focused", "window:activate"}
        self.assertTrue(orca.hears(types, "object:state-changed:focused"))
        self.assertTrue(orca.hears(types, "window:activate"))
        self.assertFalse(orca.hears(types, "object:state-changed:focusable"))
        self.assertFalse(orca.hears(types, "object:bounds-changed"))

    def test_a_stop_cuts_only_what_was_still_being_said(self):
        said = orca.utterances(self.lines())
        self.assertEqual([u.text for u in said],
                         ["frame.", "entry 05/02/2026 selected.", "July 2026"])
        # "frame." was 82 ms into an estimated 0.63 s when the next stop came.
        self.assertTrue(said[0].cut)
        self.assertFalse(said[1].cut)
        self.assertFalse(said[2].cut)

    def test_an_utterance_long_finished_is_not_cut_by_a_later_stop(self):
        log = ("10:00:00.000000 - SPEECH OUTPUT: 'OK' {}\n"
               "10:00:05.000000 - NULL SPEECH: stop\n")
        said = orca.utterances(orca.lines_between(log, "00", "24"))
        self.assertFalse(said[0].cut)

    def test_queued_utterances_are_all_cut_by_one_stop(self):
        log = ("10:00:00.000000 - SPEECH OUTPUT: 'a long first sentence to say' {}\n"
               "10:00:00.010000 - SPEECH OUTPUT: 'a second one queued behind it' {}\n"
               "10:00:00.500000 - NULL SPEECH: stop\n")
        said = orca.utterances(orca.lines_between(log, "00", "24"))
        self.assertTrue(all(u.cut for u in said))

    def test_the_fate_of_a_sentence(self):
        lines = self.lines()
        self.assertEqual(orca.fate(lines, "July 2026"), "spoken")
        self.assertEqual(orca.fate(lines, "frame"), "cut")
        self.assertEqual(orca.fate(lines, "June 2026"), "dropped")
        self.assertEqual(orca.fate(lines, "August 2026"), "unheard")

    def test_receipts_are_the_applications_events_from_a_line_on(self):
        lines = LOG.splitlines()
        got = orca.receipts(lines, 0, "datetime-pickers")
        # The "is not obsoleted" line is the same event taken from the queue,
        # not a second receipt.
        self.assertEqual([(r.type, r.stamp) for r in got],
                         [("object:announcement", "11:53:41.253888"),
                          ("object:announcement", "11:53:45.154734")])
        self.assertEqual(orca.receipts(lines, 9, "datetime-pickers")[0].stamp,
                         "11:53:45.154734")
        self.assertEqual(orca.receipts(lines, 0, "another-app"), [])

    def test_the_idle_heartbeat_is_not_work(self):
        lines = LOG.splitlines()
        busy = orca.last_busy(lines, 0)
        self.assertEqual(lines[busy][:15], "11:53:45.268606")


class KeyEcho(unittest.TestCase):
    """Orca echoing a key the application reported is not the application
    speaking: the next key cuts it, as it does for a fast typist."""

    LOG = ("10:00:00.000000 - SPEECH OUTPUT: 'tab' {}\n"
           "10:00:00.000100 - NULL SPEECH: key event\n"
           "10:00:00.050000 - NULL SPEECH: stop\n"
           "10:00:00.051000 - SPEECH OUTPUT: 'Save push button.' {}\n"
           "10:00:00.090000 - NULL SPEECH: stop\n")

    def test_an_utterance_followed_by_a_key_event_is_echo(self):
        said = orca.utterances(orca.lines_between(self.LOG, "00", "24"))
        self.assertEqual([(u.text, u.echo, u.cut) for u in said],
                         [("tab", True, True), ("Save push button.", False, True)])

    def test_an_echo_cut_is_not_observed(self):
        act = Checks.Act([], orca_lines=orca.lines_between(self.LOG, "00", "24"))
        act.app_name = "app"
        found = [o["evidence"][0] for o in checks.observations(act) if o["kind"] == "orca-cut"]
        self.assertEqual(found, ["10:00:00.051000 said 'Save push button.'"])


class OrcaLogMore(unittest.TestCase):
    def test_a_receipt_whose_source_name_spans_lines_is_found(self):
        lines = ["12:00:00.000001 - EVENT MANAGER: object:state-changed:focused for [tool tip: 'Warning",
                 "Disk almost full'] in [application: 'toast-demo'] (1, 0, 0)",
                 "12:00:00.000002 - EVENT MANAGER: window:activate for [frame: 'x'] "
                 "in [application: 'toast-demo'] (0, 0, x)"]
        got = orca.receipts(lines, 0, "toast-demo")
        self.assertEqual([r.type for r in got], ["object:state-changed:focused", "window:activate"])

    def test_orcas_locus_changes_and_silent_results_are_kept(self):
        log = ("10:00:00.000000 - FOCUS MANAGER: Changing locus of focus from [panel] to [unknown]. Notify: True\n"
               "10:00:00.100000 - SPEECH GENERATOR: Results for [unknown] are pauses only\n"
               "10:00:00.200000 - SCRIPT MANAGER: something else\n")
        kinds = [line.to_json()["kind"] for line in orca.lines_between(log, "00", "24")]
        self.assertEqual(kinds, ["locus", "silent"])


class Checks(unittest.TestCase):
    class Act:
        def __init__(self, events, orca_lines=(), tree=None):
            self.events = events
            self.history = events
            self.orca = list(orca_lines)
            self.tree = tree
            self.start_mono = 0.0
            self.start_wall = "10:00:00.000000"
            self.error = None

        def rel_ms(self, event):
            return event["mono"] * 1000

    def event(self, seq, kind, role, name, detail1=0, **extra):
        record = {"seq": seq, "mono": seq / 100, "type": kind, "detail1": detail1,
                  "source": {"role": role, "name": name, "path": f"/p/{name}"}}
        record.update(extra)
        return record

    def test_an_announcement_before_the_focus_move_is_observed(self):
        act = self.Act([
            self.event(1, "object:announcement", "status bar", "Saved", text="Saved"),
            self.event(2, "object:state-changed:focused", "push button", "OK", 1),
        ])
        kinds = [o["kind"] for o in checks.observations(act)]
        self.assertEqual(kinds, ["announced-before-focus"])

    def test_an_announcement_from_a_node_the_bus_called_defunct_is_observed(self):
        act = self.Act([
            self.event(1, "object:state-changed:defunct", "status bar", "Saved", 1),
            self.event(2, "object:announcement", "status bar", "Saved", text="Again"),
        ])
        kinds = [o["kind"] for o in checks.observations(act)]
        self.assertEqual(kinds, ["announced-from-defunct"])

    def test_focused_follows_the_last_focus_move_and_an_active_descendant(self):
        act = self.Act([
            self.event(1, "object:state-changed:focused", "table", "Calendar", 1),
            self.event(2, "object:active-descendant-changed", "table", "Calendar",
                       target={"role": "table cell", "name": "Friday, May 1", "path": "/c"}),
        ])
        self.assertTrue(checks.focused(role="table cell").run(act)[0])
        self.assertFalse(checks.focused(role="table").run(act)[0])

    def test_an_announcement_long_before_the_focus_move_is_not_the_race(self):
        act = self.Act([
            self.event(1, "object:announcement", "status bar", "Saved", text="Saved"),
            self.event(20, "object:state-changed:focused", "push button", "OK", 1),
        ])
        self.assertEqual(checks.observations(act), [])

    def test_the_focus_loss_of_a_removed_node_is_not_a_finding(self):
        lines = orca.lines_between(
            "10:00:00.000000 - EVENT MANAGER: object:state-changed:focused for [push button: 'Save'] "
            "in [application: 'app'] (0, 0, 0) is not obsoleted\n"
            "10:00:00.000100 - EVENT MANAGER: Ignoring defunct object: [push button: 'Save']\n"
            "10:00:01.000000 - EVENT MANAGER: object:announcement for [status bar: 'x'] "
            "in [application: 'app'] (1, 0, Saved) is not obsoleted\n"
            "10:00:01.000100 - EVENT MANAGER: Ignoring defunct object: [status bar: 'x']\n", "00", "24")
        act = self.Act([], orca_lines=lines)
        act.app_name = "app"
        found = checks.observations(act)
        self.assertEqual([o["kind"] for o in found], ["orca-dropped-defunct"])
        self.assertIn("object:announcement", found[0]["evidence"][0])

    def test_focus_stays_needs_no_focus_move_and_the_same_holder(self):
        before = self.event(1, "object:state-changed:focused", "entry", "Name", 1)
        before["mono"] = -1.0
        act = self.Act([])
        act.history = [before]
        self.assertTrue(checks.focus_stays(role="entry", name="Name").run(act)[0])
        self.assertFalse(checks.focus_stays(role="push button").run(act)[0])

    def test_in_tree_says_what_it_found_when_only_a_state_is_missing(self):
        tree = {"role": "frame", "name": "w", "children": [
            {"role": "check box", "name": "Agree", "states": ["focusable"]}]}
        act = self.Act([], tree=tree)
        ok, evidence = checks.in_tree(role="check box", name="Agree", state="checked").run(act)
        self.assertFalse(ok)
        self.assertIn("but states=", evidence[0])

    def test_a_speech_check_is_skipped_without_orca(self):
        act = self.Act([])
        result = checks.evaluate(checks.said("x"), act, have_orca=False)
        self.assertEqual(result["status"], "skipped")


class FakeKey(unittest.TestCase):
    """The key client is built once per target directory, on the first run."""

    def test_the_first_build_in_a_fresh_target_directory_lands_there(self):
        import os
        import shutil
        import tempfile
        from reader_lib import run
        if not (shutil.which("gcc") and shutil.which("wayland-scanner")
                and Path("/usr/share/plasma-wayland-protocols/fake-input.xml").exists()):
            self.skipTest("gcc, wayland-scanner or plasma-wayland-protocols is missing")
        # Beside the repository's own target, on its file system: the build's
        # temporaries must not go to /tmp, which is often another one.
        repo_target = Path(__file__).resolve().parents[3] / "target"
        repo_target.mkdir(exist_ok=True)
        fresh = Path(tempfile.mkdtemp(prefix="reader-test-target-", dir=repo_target))
        saved = os.environ.get("TEKSILO_READER_TARGET_DIR")
        os.environ["TEKSILO_READER_TARGET_DIR"] = str(fresh)
        try:
            built = run.build_fake_key()
            self.assertTrue(built.is_file())
            self.assertEqual(built.parent, fresh / "reader" / ".bin")
        finally:
            if saved is None:
                os.environ.pop("TEKSILO_READER_TARGET_DIR")
            else:
                os.environ["TEKSILO_READER_TARGET_DIR"] = saved
            shutil.rmtree(fresh, ignore_errors=True)


class Audit(unittest.TestCase):
    def test_an_unnamed_window_and_control_are_named_as_such(self):
        tree = {"role": "application", "name": "app", "children": [
            {"role": "frame", "name": "", "children": [
                {"role": "push button", "name": "", "states": ["focusable", "showing"]},
                {"role": "push button", "name": "OK", "states": ["focusable", "showing"]},
            ]}]}
        kinds = sorted(i["kind"] for i in audit.issues(tree))
        self.assertEqual(kinds, ["unnamed-control", "unnamed-window"])


if __name__ == "__main__":
    unittest.main()
