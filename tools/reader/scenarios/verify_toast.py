# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for toast-demo (the sweep's own acts are in
`toast.py`; these only add what its scenarios did not do).

* a toast shown while the bell's popover is open (every toast pushes to the
  archive, which rebuilds the bell and the log inside the popover);
* the log dialog open while the background job runs (every progress step
  pushes to the archive, which rebuilds the log), and "Clear all" in it;
* one of two toasts dismissed by its close button (focus, and whether the
  remaining toast is read again);
* an error toast's own action pressed from the keyboard;
* the toast lifetime with the second toast raised by real keys (Tab, Space)
  rather than an AT-SPI click, so a frame runs before it is shown.
"""

from __future__ import annotations

import time

from reader_lib.checks import (_focus_node, _is_focus, announced, custom, focused, in_tree,
                               not_in_tree, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario
from scenarios.toast import (_anns, _ann_line, focus_kept, focus_never_left_after,
                             focus_not_frame, only_announced, press, spawn)

PACKAGE = "toast-demo"


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        moves = [(act.rel_ms(e), _focus_node(e)) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"{ms:+.0f}ms [{n.get('role')}] {n.get('name')!r}"
                                  for ms, n in moves) or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def announcements_record():
    """Every announcement of the act (always passes)."""
    def run(act):
        a = _anns(act)
        return True, [f"{len(a)} announcement(s)"] + [_ann_line(act, e) for e in a[:40]]
    return custom("the announcements of the act (a record)", run)


def speech_record():
    def run(act):
        us = utterances(act.orca)
        return True, [f"{u.stamp} said{' (cut)' if u.cut else ''}: {u.text!r}"
                      for u in us] or ["Orca said nothing"]
    return custom("what Orca said in the act (a record)", run, needs_orca=True)


def dialog_count():
    def run(act):
        from reader_lib.checks import _walk
        found = [n for n in _walk(act.tree) if n.get("role") == "dialog"]
        return bool(found), [f"[dialog] {n.get('name')!r} states={n.get('states')}"
                             for n in found] or ["no dialog in the tree after the act"]
    return custom("a dialog is in the tree after the act", run, needs_tree=True)


# ---------------------------------------------------------------------------
# A. A toast arrives while the bell's popover is open
# ---------------------------------------------------------------------------


def popover_toast_arrives(run):
    spawn(run, "Info", "Info notice #1")
    press(run, "Notifications")
    with run.act("Space on the bell", [dialog_count(), focus_sequence(), speech_record()],
                 tree=True, should="the popover opens and focus moves into it"):
        run.key("space")
    with run.act("AT-SPI click on Warning while the popover is open",
                 [dialog_count(), focus_sequence(), announcements_record(), speech_record(),
                  focus_kept("push button", "Mark all read")],
                 tree=True, record=2.0,
                 should="the new toast is heard; the popover the reader is in stays open, "
                        "with focus where it was"):
        run.action("click", role="push button", name="Warning")
    with run.act("Tab in the popover after the toast",
                 [dialog_count(), focus_sequence(), speech_record()], tree=True,
                 should="the reader goes on inside the popover"):
        run.key("Tab")


def popover_during_job(run):
    """The bell's popover opened while the background job runs."""
    press(run, "Start background job")
    with run.act("Space on Start background job, Tab x2 to the bell, Space, listen 2 s",
                 [focus_sequence(), speech_record(), dialog_count(),
                  focus_never_left_after("push button", "Mark all read")],
                 record=2.5, tree=True,
                 should="the notification popover opens and stays open while the job "
                        "reports progress"):
        run.key("space")
        run.wait(0.3)
        run.key("Tab", "Tab", gap=0.3)
        run.key("space")
        run.wait(2.0)


# ---------------------------------------------------------------------------
# B. The log dialog open while the background job runs; then Clear all
# ---------------------------------------------------------------------------


def log_during_job(run):
    press(run, "Start background job")
    with run.act("Space on Start background job, Tab to Open log dialog, Space, Tab to "
                 "Clear all, listen 2.5 s",
                 [focus_sequence(), announcements_record(), speech_record(),
                  focus_never_left_after("push button", "Clear all"), dialog_count()],
                 record=3.0, tree=True,
                 should="the log opens; the reader moves to Clear all and stays there "
                        "while the job's progress is archived"):
        run.key("space")
        run.wait(0.3)
        run.key("Tab")
        run.wait(0.3)
        run.key("space")
        run.wait(0.6)
        run.key("Tab")
        run.wait(2.5)


def log_clear_all(run):
    spawn(run, "Info", "Info notice #1")
    spawn(run, "Warning", "Warning #2")
    press(run, "Open log dialog")
    with run.act("Space on Open log dialog", [dialog_count(), focus_sequence(),
                                              speech_record()],
                 tree=True, should="the log dialog opens"):
        run.key("space")
    with run.act("Tab to Clear all", [focused(role="push button", name="Clear all"),
                                      speech_record()],
                 should="focus on Clear all"):
        run.key("Tab")
    with run.act("Space on Clear all",
                 [focus_sequence(), speech_record(), announcements_record(),
                  not_in_tree(name="Warning #2", role="unknown"), dialog_count()],
                 tree=True,
                 should="the log empties; the reader hears that it did, and focus stays "
                        "somewhere sensible in the dialog"):
        run.key("space")


# ---------------------------------------------------------------------------
# C. One of two toasts dismissed by its close button
# ---------------------------------------------------------------------------


def dismiss_one_of_two(run):
    spawn(run, "Persistent error", "Sticky error #1")
    spawn(run, "Persistent error", "Sticky error #2")
    # Persistent error -> Start upload -> Update upload -> Complete upload ->
    # Start background job -> Open log dialog -> Notifications -> toast 1 ->
    # its Clear -> toast 2 -> its Clear.
    with run.act("Tab x10 to the second toast's close button",
                 [focused(role="push button", name="Clear"), focus_sequence(),
                  speech_record()], record=1.5,
                 should="focus reaches the second toast's close button"):
        run.key(*["Tab"] * 10, gap=0.2)
    with run.act("Space on the second toast's close button",
                 [focus_sequence(), announcements_record(), speech_record(),
                  only_announced(), not_in_tree(name="Sticky error #2")],
                 tree=True,
                 should="the second toast goes; the first, still there, is not announced "
                        "again as if it were new; focus goes somewhere sensible"):
        run.key("space")


# ---------------------------------------------------------------------------
# D. An error toast's own action pressed from the keyboard
# ---------------------------------------------------------------------------


def show_errors(run):
    spawn(run, "Error", "Build #1 failed")
    with run.act("Tab x9 to Show errors", [focused(role="push button", name="Show errors")],
                 record=1.0, should="focus reaches the toast's action"):
        run.key(*["Tab"] * 9, gap=0.2)
    with run.act("Space on Show errors",
                 [focus_sequence(), speech_record(), focus_not_frame()],
                 tree=True,
                 should="the action runs and the toast closes; focus goes somewhere "
                        "sensible and the reader hears where"):
        run.key("space")


# ---------------------------------------------------------------------------
# E. Lifetime, the second toast raised by real keys
# ---------------------------------------------------------------------------


def lifetime_keys(run):
    press(run, "Info")

    def lives(title, want, slack=1.0):
        def check(act):
            adds = [e for e in act.events if e["type"] == "object:children-changed:add"
                    and e.get("target", {}).get("name") == title]
            removes = [e for e in act.events if e["type"] == "object:children-changed:remove"
                       and e.get("target", {}).get("name") == title]
            if not adds or not removes:
                return False, [f"{title!r}: {len(adds)} add(s), {len(removes)} removal(s)"]
            life = removes[-1]["mono"] - adds[0]["mono"]
            return abs(life - want) <= slack, [
                f"{title!r} first added {act.rel_ms(adds[0]):+.1f} ms, last removed "
                f"{act.rel_ms(removes[-1]):+.1f} ms: {life:.2f} s on the bus"]
        return custom(f"{title!r} stays about {want:g} s", check)

    with run.act("Space on Info; 5 s later Tab to Success and Space; watch both",
                 [lives("Info notice #1", 10.0), lives("Saved #2", 10.0)],
                 settle=0.5, record=0.5,
                 should="each toast stays its own 10 s"):
        run.key("space")
        run.wait(5.0)
        run.key("Tab", gap=0.4)
        run.key("space")
        end = time.monotonic() + 14.0
        while time.monotonic() < end:
            if not run.find(name="Saved #2") and not run.find(name="Info notice #1"):
                break
            time.sleep(0.2)


# ---------------------------------------------------------------------------
# F. Reaching a toast's action while listening to each stop
# ---------------------------------------------------------------------------


def reach_listening(run):
    spawn(run, "Error", "Build #1 failed")
    with run.act("Tab x9 to Show errors, 1 s per stop (a reader listening to each)",
                 [focused(role="push button", name="Show errors"), focus_sequence()],
                 record=1.0, tree=True,
                 should="a reader who listens to each stop still reaches the toast's action"):
        run.key(*["Tab"] * 9, gap=1.0)
    spawn(run, "Error", "Build #2 failed")
    press(run, "Info")
    with run.act("Shift+Tab from the first toolbar button",
                 [focus_sequence(), speech_record()], record=1.5,
                 should="the reader finds where Shift+Tab from the top lands (the toasts, "
                        "last in the Tab order?)"):
        run.key("Shift+Tab")


def popover_mark_read(run):
    spawn(run, "Info", "Info notice #1")
    spawn(run, "Warning", "Warning #2")
    press(run, "Notifications")
    with run.act("Space on the bell", [dialog_count(), speech_record()], tree=True,
                 should="the popover opens"):
        run.key("space")
    with run.act("Space on Mark all read",
                 [dialog_count(), focus_sequence(), speech_record(),
                  focus_kept("push button", "Mark all read")],
                 tree=True,
                 should="the rows are marked read; the popover stays open with focus on "
                        "the button the reader pressed"):
        run.key("space")


SCENARIOS = [
    Scenario("verify-toast-popover-mark-read", PACKAGE, popover_mark_read,
             "Mark all read pressed inside the bell's popover"),
    Scenario("verify-toast-reach-listening", PACKAGE, reach_listening,
             "reach an error toast's action at a listening pace; Shift+Tab from the top"),
    Scenario("verify-toast-popover-toast", PACKAGE, popover_toast_arrives,
             "a toast shown while the bell's popover is open"),
    Scenario("verify-toast-popover-during-job", PACKAGE, popover_during_job,
             "the bell's popover opened while the background job runs"),
    Scenario("verify-toast-log-during-job", PACKAGE, log_during_job,
             "the log dialog open while the background job archives its progress"),
    Scenario("verify-toast-log-clear-all", PACKAGE, log_clear_all,
             "Clear all in the log dialog"),
    Scenario("verify-toast-dismiss-one-of-two", PACKAGE, dismiss_one_of_two,
             "the second of two persistent toasts closed by its button"),
    Scenario("verify-toast-show-errors", PACKAGE, show_errors,
             "an error toast's action pressed from the keyboard"),
    Scenario("verify-toast-lifetime-keys", PACKAGE, lifetime_keys,
             "toast lifetime with the second toast raised by Tab + Space"),
]
