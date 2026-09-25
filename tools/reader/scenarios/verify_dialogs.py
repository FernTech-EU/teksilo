# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for dialogs-and-popovers (the sweep's own acts are in
`dialogs.py`; these only add what its scenarios did not do).

* where the snackbar's Dismiss sits in the Tab order when it is shown from its
  own opener, and whether a keyboard user can reach it inside the 2.5 s
  time-out (forwards and backwards);
* Tab out of an open popover, and a third opening of it;
* a MessageBox opened with Enter on its opener (the sweep opened every box with
  Space), and one opened by a screen reader's own activation (AT-SPI click)
  while focus is elsewhere;
* the popover opened while focus is on a real button, so the restore target
  and the reading on close are unambiguous.
"""

from __future__ import annotations

from reader_lib.checks import (announced, custom, focused, in_tree, no_event, not_in_tree,
                               said)
from reader_lib.scenario import Scenario

PACKAGE = "dialogs-and-popovers"


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        from reader_lib.checks import _focus_node, _is_focus
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                      or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def put_focus(run, name: str) -> None:
    run.wait_for(role="push button", name=name)
    run.grab_focus(role="push button", name=name)
    run.wait(1.0)


# ---------------------------------------------------------------------------
# Snackbar: can a keyboard user reach Dismiss before it goes?
# ---------------------------------------------------------------------------


def snackbar_distance(run):
    put_focus(run, "Show snackbar")
    with run.act("Space shows it, then Tab seven times at once",
                 [focus_sequence(), focused(role="push button", name="Dismiss")],
                 should="Dismiss is reachable from the opener by keyboard inside the "
                        "time-out", record=4.0, tree=True):
        run.key("space")
        run.key(*(["Tab"] * 7), gap=0.12)
    put_focus(run, "Show snackbar")
    with run.act("Space shows it, then Shift+Tab until Dismiss",
                 [focus_sequence(), focused(role="push button", name="Dismiss")],
                 should="backwards, Dismiss is the stop before the first page control",
                 record=4.0, tree=True):
        run.key("space")
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab", gap=0.12)


# ---------------------------------------------------------------------------
# Popover: open from a real button, Tab out, reopen three times
# ---------------------------------------------------------------------------


def popover_more(run):
    opener = "Show popover"
    put_focus(run, "Show snackbar")
    with run.act("AT-SPI click opens the popover while focus is on 'Show snackbar'",
                 [in_tree(role="dialog"), focus_sequence(), said("Use popovers")],
                 should="focus moves into the popover and the reader hears it", tree=True):
        run.action("click", role="push button", name=opener)
    with run.act("Tab from inside the popover",
                 [focus_sequence(), not_in_tree(role="dialog")],
                 should="Tab leaves a disclosure popover and closes it; the reader hears "
                        "where focus went", tree=True):
        run.key("Tab")
    put_focus(run, "Show snackbar")
    for n in (2, 3):
        with run.act(f"AT-SPI click opens it, opening {n}",
                     [in_tree(role="dialog"), focus_sequence(), said("Use popovers")],
                     tree=True):
            run.action("click", role="push button", name=opener)
        with run.act(f"Escape closes opening {n}",
                     [focused(role="push button", name="Show snackbar"),
                      said("Show snackbar"), not_in_tree(role="dialog")],
                     should="focus returns to 'Show snackbar', and the reader is told",
                     tree=True):
            run.key("Escape")


# ---------------------------------------------------------------------------
# MessageBox: opened with Enter, and by an AT-SPI click from elsewhere
# ---------------------------------------------------------------------------


def messagebox_more(run):
    put_focus(run, "Save changes?")
    with run.act("Enter on 'Save changes?' opens the box, and it stays open",
                 [announced("Save changes?"), focused(role="push button", name="Save"),
                  said("Save push button"), in_tree(role="alert", name="Save changes?")],
                 should="the Enter that opened the box does not also answer it",
                 record=3.0, tree=True):
        run.key("Return")
    with run.act("Escape closes it", [focused(role="push button", name="Save changes?")]):
        run.key("Escape")
    put_focus(run, "Save changes?")
    with run.act("AT-SPI click on 'Delete file?' while focus is on 'Save changes?'",
                 [announced("Delete file?"), focused(role="push button", name="No"),
                  said("No push button"), in_tree(role="alert", name="Delete file?")],
                 should="a screen reader's own activation opens the box like a key does",
                 tree=True):
        run.action("click", role="push button", name="Delete file?")
    with run.act("Escape answers No and focus goes back to the page",
                 [focus_sequence()],
                 should="focus returns to where it was when the box opened ('Save changes?') "
                        "or to the opener", tree=True):
        run.key("Escape")
    put_focus(run, "Custom buttons")
    for n in (1, 2, 3):
        with run.act(f"Space opens 'How do I…?', opening {n}",
                     [announced("How do I"), focused(role="push button", name="OK")]):
            run.key("space")
        with run.act(f"Escape closes opening {n}",
                     [focused(role="push button", name="Custom buttons")]):
            run.key("Escape")


SCENARIOS = [
    Scenario("verify-dialogs-snackbar-distance", PACKAGE, snackbar_distance,
             "how far the snackbar's Dismiss is from its opener in the Tab order"),
    Scenario("verify-dialogs-popover-more", PACKAGE, popover_more,
             "popover opened from a real button, Tab out, two more openings"),
    Scenario("verify-dialogs-messagebox-more", PACKAGE, messagebox_more,
             "MessageBox opened with Enter, by AT-SPI click from elsewhere, and 3 times"),
]
