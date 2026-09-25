# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""datetime-pickers: the date fields' calendars and names, after the fix.

What the sweep found (`datetime-01`, `datetime-03`, `datetime-06`,
`datetime-07`, `datetime-13`, `catalog-b-11`), act by act, with the checks a
reader's experience should now pass:

- a date field's calendar opens on the date the field holds, however the
  last opening was left, so Enter keeps that date, and a range calendar
  opens with no range begun;
- the calendar's months view is worked from the keyboard, each month heard
  with its year, and never commits the day it hides;
- the field focus lands on is named, and a date or range part is an entry
  read with its date.

Launch puts the keyboard focus in the `DateEdit`'s field. The Tab order is
the one `datetime.py` lists.

Speech is judged only where focus lands on a node the reader has not met.
A node that leaves the tree and comes back (a calendar opened a second time
on the same month, the months view shown again) returns under an id AT-SPI
already declared defunct, and Orca drops its focus event: that is
`datetime-02`, the node-ids topic, not this one. There the checks hold the
focus event, which says where the calendar is, and not Orca.
"""

from __future__ import annotations

from reader_lib.checks import focused, in_tree, no_event, said
from reader_lib.scenario import Scenario
from scenarios.datetime import PACKAGE, label, offers_action, position, tabs


def cell(name: str):
    return focused(role="table cell", name=name)


# ---------------------------------------------------------------------------
# DateEdit: its field's name, and its calendar on the field's date
# ---------------------------------------------------------------------------


def dateedit(run):
    position(run, "Shift+Tab away from the DateEdit field", "Shift+Tab")
    with run.act("Tab back to the DateEdit field",
                 [focused(role="entry", name="Date"), said("Date"), said("05/02/2026")],
                 should="the reader hears the field's name and its date"):
        run.key("Tab")
    with run.act("Up in the field (caret on the year)",
                 [label("Edit: 2027-05-02")],
                 should="the year goes up one", tree=True):
        run.key("Up")
    with run.act("Alt+Down opens the calendar",
                 [cell("Sunday, May 2, 2027"), said("Sunday, May 2, 2027")],
                 should="the calendar opens on the date the field holds, 2 May 2027"):
        run.key("Alt+Down")
    with run.act("Enter in the calendar",
                 [label("Edit: 2027-05-02"), focused(role="entry", name="Date")],
                 should="the field's own date is committed: nothing changes", tree=True):
        run.key("Return")
    position(run, "Alt+Down, Right, Escape", "Alt+Down", "Right", "Escape")
    with run.act("Alt+Down reopens it",
                 [cell("Sunday, May 2, 2027")],
                 should="the calendar opens on the field's date again, not on 3 May "
                        "where the last opening's cursor was left (the same month's "
                        "days come back: what Orca says is datetime-02's)"):
        run.key("Alt+Down")
    with run.act("Enter in the reopened calendar",
                 [label("Edit: 2027-05-02")],
                 should="the field keeps its date", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# DateTimeEdit: its parts read with their text, its calendar on the date part
# ---------------------------------------------------------------------------


def datetimeedit(run):
    position(run, "Tab three times, to the 12 h field", *tabs(3))
    with run.act("Tab to the DateTimeEdit date part",
                 [focused(role="entry", name="Date"), said("Date"), said("05/02/2026")],
                 should="the reader hears the part's name and its date"):
        run.key("Tab")
    with run.act("Up in the date part",
                 [label("DateTime: 2027-05-02 14:35")],
                 should="the year goes up one", tree=True):
        run.key("Up")
    with run.act("Tab to the time part",
                 [focused(role="entry", name="Time"), said("Time"), said("02:35 PM")],
                 should="the reader hears the part's name and its time"):
        run.key("Tab")
    position(run, "Tab to Open calendar", "Tab")
    with run.act("Space on Open calendar",
                 [cell("Sunday, May 2, 2027"), said("Sunday, May 2, 2027")],
                 should="the calendar opens on the date the date part holds"):
        run.key("space")
    with run.act("Enter in the calendar",
                 [label("DateTime: 2027-05-02 14:35"), focused(role="entry", name="Date")],
                 should="the date part's own date is committed and the time kept",
                 tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# DateRangeEdit: its halves read with their dates, its calendar on the start
# ---------------------------------------------------------------------------


def rangeedit(run):
    position(run, "Tab six times, to the DateTimeEdit's Open calendar", *tabs(6))
    with run.act("Tab to the DateRangeEdit start date",
                 [focused(role="entry", name="Start date"), said("Start date"),
                  said("05/02/2026")],
                 should="the reader hears the half's name and its date"):
        run.key("Tab")
    with run.act("Tab to the end date",
                 [focused(role="entry", name="End date"), said("End date"),
                  said("05/16/2026")],
                 should="the reader hears the half's name and its date"):
        run.key("Tab")
    position(run, "Shift+Tab back to the start date", "Shift+Tab")
    with run.act("Up in the start date (caret on the year)",
                 [label("Range edit: 2026-05-16 – 2027-05-02")],
                 should="the start's year goes up one, past the end, and the two swap",
                 tree=True):
        run.key("Up")
    position(run, "Tab twice, to Open range calendar", "Tab", "Tab")
    with run.act("Space on Open range calendar",
                 [cell("Saturday, May 16, 2026"), said("Saturday, May 16, 2026")],
                 should="the range calendar opens on the range's start"):
        run.key("space")


def range_anchor(run):
    position(run, "Tab nine times, to Open range calendar", *tabs(9))
    with run.act("Space on Open range calendar",
                 [cell("Saturday, May 2, 2026")],
                 should="the range calendar opens on the range's start"):
        run.key("space")
    with run.act("Right, Enter: one end of a new range",
                 [label("Range edit: 2026-05-02 – 2026-05-16")],
                 should="one end is picked; the field keeps its range", tree=True):
        run.key("Right", "Return", gap=0.6)
    with run.act("Escape closes the calendar",
                 [label("Range edit: 2026-05-02 – 2026-05-16")],
                 should="the popup takes Escape and closes, the range half picked; "
                        "the field keeps its range", tree=True):
        run.key("Escape")
    run.grab_focus(role="push button", name="Open range calendar")
    with run.act("Space on Open range calendar again",
                 [cell("Saturday, May 2, 2026")],
                 should="the calendar opens on the range's start (the same month's "
                        "days come back: what Orca says is datetime-02's)"):
        run.key("space")
    with run.act("Enter in the reopened calendar",
                 [label("Range edit: 2026-05-02 – 2026-05-16"),
                  no_event("object:state-changed:focused", role="push button",
                           name_contains="Open range calendar")],
                 should="a new opening begins a new range: one Enter picks one end, "
                        "the field keeps its range and the calendar stays open for the "
                        "other end, where the half picked before the close made it a "
                        "range and closed the calendar",
                 tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# The single calendar's months view, from the keyboard
# ---------------------------------------------------------------------------


def zoom(run):
    position(run, "Tab fourteen times, to the title button", *tabs(14))
    with run.act("Space on the title button (May 2026)",
                 [cell("May 2026"), said("May 2026")],
                 should="the months of 2026 show and focus goes to the month shown, "
                        "heard with its year", tree=True):
        run.key("space")
    with run.act("Down in the months view",
                 [cell("August 2026"), said("August 2026"), label("Selected: 2026-05-02")],
                 should="the cursor moves down a row of months and the reader hears the "
                        "month; the selected date does not change", tree=True):
        run.key("Down")
    with run.act("Enter on August",
                 [cell("Sunday, August 2, 2026"), said("Sunday, August 2, 2026"),
                  label("Selected: 2026-05-02")],
                 should="August opens on its days, the cursor on the 2nd; nothing is "
                        "selected", tree=True):
        run.key("Return")
    position(run, "Tab three times, to the title (August 2026)", *tabs(3))
    with run.act("Space on the title again",
                 [cell("August 2026")],
                 should="back in the months, on August (the months come back: what "
                        "Orca says is datetime-02's)"):
        run.key("space")
    with run.act("Right in the months view",
                 [cell("September 2026")],
                 should="the cursor moves to September"):
        run.key("Right")
    with run.act("Escape in the months view",
                 [cell("Wednesday, September 2, 2026"),
                  said("Wednesday, September 2, 2026"), label("Selected: 2026-05-02")],
                 should="Escape goes back to the days, of the month under the cursor; "
                        "nothing is selected", tree=True):
        run.key("Escape")
    position(run, "Tab three times, to the title (September 2026)", *tabs(3))
    with run.act("Space on the title once more",
                 [cell("September 2026"), offers_action("table cell", "March 2026")],
                 should="every month offers a screen reader's activation", tree=True):
        run.key("space")
    with run.act("Tab from the months",
                 [focused(role="push button", name="Previous year")],
                 should="the months are one Tab stop, the grid's: Tab leaves them"):
        run.key("Tab")
    with run.act("AT-SPI click on the month March",
                 [label("Selected: 2026-05-02"),
                  in_tree(role="push button", name="March 2026")],
                 should="a screen reader's own activation opens March on its days; "
                        "the title reads March 2026", tree=True):
        run.action("click", role="table cell", name="March 2026")


# ---------------------------------------------------------------------------
# DateEdit's calendar: the months view, then a reopening
# ---------------------------------------------------------------------------


def popover_months(run):
    with run.act("Alt+Down opens the DateEdit calendar",
                 [cell("Saturday, May 2, 2026"), said("Saturday, May 2, 2026")],
                 should="the calendar opens on the field's date"):
        run.key("Alt+Down")
    position(run, "Tab three times, to the popover's title button", *tabs(3))
    with run.act("Space on the title: the months view",
                 [cell("May 2026"), said("May 2026")],
                 should="the months show and the reader hears the month shown"):
        run.key("space")
    with run.act("Down in the months view",
                 [cell("August 2026"), said("August 2026")],
                 should="the reader hears the month the cursor moved to"):
        run.key("Down")
    with run.act("Escape closes the calendar from the months view",
                 [focused(role="entry", name="Date"), said("Date")],
                 should="the popup takes Escape first and closes; focus goes back to "
                        "the field"):
        run.key("Escape")
    with run.act("Alt+Down reopens the calendar",
                 [cell("Saturday, May 2, 2026"), said("Saturday, May 2, 2026")],
                 should="the calendar opens on the field's date, on its days, not on "
                        "the months where it was closed nor on August where the "
                        "cursor was left"):
        run.key("Alt+Down")
    with run.act("Enter in the reopened calendar",
                 [label("Edit: 2026-05-02")],
                 should="the field keeps its date", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# TimeEdit's field names
# ---------------------------------------------------------------------------


def timeedit(run):
    with run.act("Tab twice, to the 24 h field",
                 [focused(role="entry", name="Time"), said("Time"), said("14:35")],
                 should="the reader hears the field's name and its time"):
        run.key("Tab", "Tab", gap=0.6)
    with run.act("Tab to the 12 h field",
                 [focused(role="entry", name="Time"), said("Time"), said("02:35:00 PM")],
                 should="the reader hears the field's name and its time"):
        run.key("Tab")


SCENARIOS = [
    Scenario("fix-dateedit-stale", PACKAGE, dateedit,
             "DateEdit: its field's name; its calendar opens on the field's date"),
    Scenario("fix-dateedit-datetimeedit", PACKAGE, datetimeedit,
             "DateTimeEdit: its parts as entries; its calendar on the date part"),
    Scenario("fix-dateedit-rangeedit", PACKAGE, rangeedit,
             "DateRangeEdit: its halves as entries; its calendar on the start"),
    Scenario("fix-dateedit-range-anchor", PACKAGE, range_anchor,
             "DateRangeEdit: a range half picked, the calendar closed, then one Enter "
             "in the next opening"),
    Scenario("fix-dateedit-zoom", PACKAGE, zoom,
             "the single calendar's months view: arrows, Enter, Escape, one Tab stop, "
             "a screen reader's click"),
    Scenario("fix-dateedit-popover-months", PACKAGE, popover_months,
             "DateEdit's calendar: closed from the months view, and a reopening"),
    Scenario("fix-dateedit-timeedit", PACKAGE, timeedit,
             "TimeEdit: the field focus lands on is named"),
]
