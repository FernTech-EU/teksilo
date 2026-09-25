# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for datetime-pickers (the sweep's own acts are in
`datetime.py`; these only add what its scenarios did not do).

* reopening the DateTimeEdit and DateRangeEdit calendars (the sweep ran the
  reopen on DateEdit only): is the retained calendar silent the second time?
* DateEdit's calendar left in the months view with a moved hidden cursor,
  closed, reopened and Enter pressed: what the field ends up holding, and what
  the reader heard of it;
* Tab through DateEdit's open calendar: does focus stay in the popup?
* DateRangeEdit: the start stepped by Up, then its calendar opened;
* the range calendar: Escape after a start was set.
"""

from __future__ import annotations

from reader_lib.checks import custom, focused, in_tree, said
from reader_lib.scenario import Scenario

PACKAGE = "datetime-pickers"


def _label(name: str):
    return in_tree(role="label", name=name)


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        from reader_lib.checks import _focus_node, _is_focus
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                      or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def spoken_record():
    """Record everything Orca said in the act (always passes)."""
    def run(act):
        from reader_lib.orca import utterances
        said_ = [f"{u.stamp} {u.text!r}{' (cut)' if u.cut else ''}" for u in utterances(act.orca)]
        drops = [f"{line.stamp} {line.text}" for line in act.orca if line.is_defunct_drop]
        return True, (said_ or ["Orca said nothing"]) + drops
    return custom("what Orca said in the act (a record)", run, needs_orca=True)


def position(run, label: str, *chords: str) -> None:
    with run.act(label, [focus_sequence()], should="(positioning, not judged)",
                 settle=0.6, record=1.2):
        run.key(*chords, gap=0.25)


# ---------------------------------------------------------------------------
# Reopening the DateTimeEdit and DateRangeEdit calendars
# ---------------------------------------------------------------------------


def reopen_others(run):
    # Launch focus: the DateEdit field. Tab 6 = DateTimeEdit's Open calendar.
    position(run, "Tab six times, to the DateTimeEdit's Open calendar", *(["Tab"] * 6))
    with run.act("Space opens the DateTimeEdit calendar",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026"), spoken_record()],
                 should="the day under the cursor is heard", tree=True):
        run.key("space")
    with run.act("Right, to 3 May", [said("Sunday, May 3, 2026"), spoken_record()]):
        run.key("Right")
    with run.act("Escape closes it", [focus_sequence(), spoken_record()],
                 should="focus goes back to the DateTimeEdit and the reader hears it"):
        run.key("Escape")
    with run.act("Put focus on the DateTimeEdit's Open calendar (AT-SPI grab_focus)",
                 [focus_sequence(), spoken_record()], should="(positioning, not judged)"):
        run.grab_focus(role="push button", name="Open calendar", nth=1)
    with run.act("Space reopens the DateTimeEdit calendar",
                 [focus_sequence(), said("May"), spoken_record()],
                 should="the reopened calendar is heard as the first time"):
        run.key("space")
    with run.act("Left, back onto 2 May (met in the first opening)",
                 [said("Saturday, May 2, 2026"), spoken_record()],
                 should="the reader hears the day"):
        run.key("Left")
    with run.act("Left again, to 1 May (never met)",
                 [said("Friday, May 1, 2026"), spoken_record()],
                 should="the reader hears the day"):
        run.key("Left")
    with run.act("Escape closes it again", [focus_sequence(), spoken_record()]):
        run.key("Escape")
    with run.act("Put focus on Open range calendar (AT-SPI grab_focus)",
                 [focus_sequence(), spoken_record()], should="(positioning, not judged)"):
        run.grab_focus(role="push button", name="Open range calendar")
    with run.act("Space opens the DateRangeEdit calendar",
                 [focus_sequence(), said("Saturday, May 2, 2026"), spoken_record()],
                 should="the day under the cursor is heard", tree=True):
        run.key("space")
    with run.act("Right, to 3 May, in the range calendar",
                 [said("Sunday, May 3, 2026"), spoken_record()]):
        run.key("Right")
    with run.act("Escape closes the range calendar", [focus_sequence(), spoken_record()]):
        run.key("Escape")
    with run.act("Put focus on Open range calendar again (AT-SPI grab_focus)",
                 [focus_sequence(), spoken_record()], should="(positioning, not judged)"):
        run.grab_focus(role="push button", name="Open range calendar")
    with run.act("Space reopens the DateRangeEdit calendar",
                 [focus_sequence(), said("May"), spoken_record()],
                 should="the reopened calendar is heard as the first time"):
        run.key("space")
    with run.act("Left, back onto 2 May, in the reopened range calendar",
                 [said("Saturday, May 2, 2026"), spoken_record()]):
        run.key("Left")


# ---------------------------------------------------------------------------
# DateEdit: months view, a hidden cursor move, close, reopen, Enter
# ---------------------------------------------------------------------------


def months_dataloss(run):
    with run.act("Alt+Down opens the DateEdit calendar",
                 [said("Saturday, May 2, 2026"), spoken_record()]):
        run.key("Alt+Down")
    position(run, "Tab three times, to the popover's title button", "Tab", "Tab", "Tab")
    with run.act("Space on the title: the months view", [spoken_record()], tree=True):
        run.key("space")
    with run.act("Down in the months view (the title button has focus)",
                 [focus_sequence(), spoken_record(), _label("Edit: 2026-05-02")],
                 should="nothing a reader cannot see changes; the field keeps its date",
                 tree=True):
        run.key("Down")
    with run.act("Escape closes the popover", [focus_sequence(), spoken_record()]):
        run.key("Escape")
    with run.act("Alt+Down reopens it (in the months view)",
                 [focus_sequence(), spoken_record()],
                 should="the reader hears that the calendar opened and where it is",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Enter in the reopened calendar",
                 [_label("Edit: 2026-05-02"), focus_sequence(), spoken_record()],
                 should="the field keeps 2 May 2026, which is all the reader ever chose "
                        "or heard; if the value changes, the reader hears it", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# Tab through DateEdit's open calendar
# ---------------------------------------------------------------------------


def popover_tab(run):
    with run.act("Alt+Down opens the DateEdit calendar",
                 [said("Saturday, May 2, 2026"), spoken_record()]):
        run.key("Alt+Down")
    for n in range(1, 8):
        with run.act(f"Tab {n} inside the open calendar", [focus_sequence(), spoken_record()],
                     should="focus stays in the popup (or the popup closes as focus "
                            "leaves it)", settle=0.5, record=1.3, tree=(n >= 5)):
            run.key("Tab")


# ---------------------------------------------------------------------------
# DateRangeEdit: step the start, open the calendar
# ---------------------------------------------------------------------------


def rangeedit_stale(run):
    position(run, "Tab seven times, to the DateRangeEdit start", *(["Tab"] * 7))
    with run.act("Up in the start date (year)",
                 [_label("Range edit: 2027-05-02 – 2026-05-16"), spoken_record()],
                 should="the start's year goes up one", tree=True):
        run.key("Up")
    with run.act("Tab twice, to Open range calendar", [focus_sequence(), spoken_record()],
                 should="(positioning)"):
        run.key("Tab", "Tab", gap=0.4)
    with run.act("Space opens the range calendar",
                 [focus_sequence(), said("2027"), spoken_record()],
                 should="the calendar opens on the range's start, 2 May 2027", tree=True):
        run.key("space")


# ---------------------------------------------------------------------------
# The range calendar: Escape after a start was set
# ---------------------------------------------------------------------------


def range_escape(run):
    position(run, "Tab eighteen times, into the range calendar", *(["Tab"] * 18))
    with run.act("Enter sets a start", [spoken_record()], tree=True):
        run.key("Return")
    with run.act("Right twice", [focus_sequence(), spoken_record()]):
        run.key("Right", "Right", gap=0.6)
    with run.act("Escape cancels the pending start",
                 [spoken_record(), focus_sequence()],
                 should="the reader hears that the pending range was cancelled", tree=True):
        run.key("Escape")
    with run.act("Enter again (a start again, or an end?)",
                 [spoken_record(), _label("No range selected")], tree=True):
        run.key("Return")


def popover_shift_tab(run):
    with run.act("Alt+Down opens the DateEdit calendar",
                 [said("Saturday, May 2, 2026"), spoken_record()]):
        run.key("Alt+Down")
    for n in range(1, 3):
        with run.act(f"Shift+Tab {n} from the open calendar's grid",
                     [focus_sequence(), spoken_record()],
                     should="Shift+Tab leaves the popup back to the field it belongs to "
                            "(its Open calendar button or the date entry)",
                     settle=0.5, record=1.3, tree=True):
            run.key("Shift+Tab")


SCENARIOS = [
    Scenario("verify-datetime-popover-shifttab", PACKAGE, popover_shift_tab,
             "Shift+Tab out of DateEdit's open calendar"),
    Scenario("verify-datetime-reopen-others", PACKAGE, reopen_others,
             "reopen the DateTimeEdit and DateRangeEdit calendars: silent the second time?"),
    Scenario("verify-datetime-months-dataloss", PACKAGE, months_dataloss,
             "DateEdit calendar: months view, Down, close, reopen, Enter"),
    Scenario("verify-datetime-popover-tab", PACKAGE, popover_tab,
             "Tab seven times through DateEdit's open calendar"),
    Scenario("verify-datetime-rangeedit-stale", PACKAGE, rangeedit_stale,
             "DateRangeEdit: step the start's year, then open the calendar"),
    Scenario("verify-datetime-range-escape", PACKAGE, range_escape,
             "range calendar: Enter, Right, Escape, Enter"),
]
