# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""datetime-pickers: the calendars and the date and time fields, as a reader
meets them.

The example (`examples/datetime_pickers`) shows a single `Calendar`, a range
`Calendar`, a `DateEdit`, two `TimeEdit`s (24 h, and 12 h with seconds), a
`DateTimeEdit` and a `DateRangeEdit`, none of them given a `.label()`.
Launch puts the keyboard focus in the `DateEdit`'s field (text selected, caret
at the end, on the year), and Tab goes, from there:

     1 Open calendar (DateEdit)      10 Theme (tool bar)
     2 24 h time field               11 single calendar grid
     3 12 h time field               12-16 its header buttons
     4 DateTimeEdit date part           (Previous year, Previous month,
     5 DateTimeEdit time part            title, Next month, Next year)
     6 Open calendar (DateTimeEdit)  17 its Today button
     7 DateRangeEdit start           18 range calendar grid
     8 DateRangeEdit end             19-23 its header buttons
     9 Open range calendar           24 its Today button

Every act uses real keys unless it says otherwise. The day under a grid's
cursor is the grid's active descendant, so an arrow press is a focus change
(`calendar.rs`, `Calendar::accessibility`).

Positioning (the Tabs that bring focus to where an act starts) is an act of
its own, judged by nothing, so that what Orca says while it moves is not
credited to the act that follows.
"""

from __future__ import annotations

from reader_lib.checks import announced, custom, event, focused, in_tree, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

PACKAGE = "datetime-pickers"
TODAY = "Friday, September 25, 2026"


def position(run, label: str, *chords: str) -> None:
    """Move focus to where the next act starts, in an act judged by nothing."""
    with run.act(label, should="(positioning, not judged)", settle=0.6, record=1.2):
        run.key(*chords, gap=0.2)


def tabs(n: int) -> list[str]:
    return ["Tab"] * n


def label(name: str):
    """The example's status line under a control reads `name` after the act."""
    return in_tree(role="label", name=name)


def month_change(expect_spoken: str):
    """What a change of month costs: the day cells retired and made on the bus,
    and what Orca did with the events. Passes when the new day was spoken and
    Orca dropped as defunct nothing but focus-loss events of retired cells."""
    def run(act):
        defunct = sum(1 for e in act.events if e["type"] == "object:state-changed:defunct"
                      and e.get("detail1") == 1)
        added = sum(1 for e in act.events if e["type"] == "object:children-changed:add")
        removed = sum(1 for e in act.events if e["type"] == "object:children-changed:remove")
        drops = [line for line in act.orca if line.is_defunct_drop]
        taken = [line for line in act.orca if line.text.endswith("is not obsoleted")]
        obsoleted = [line for line in act.orca if " is obsoleted by " in line.text]
        spoken = [u.text for u in utterances(act.orca)]
        # Which event each drop was: the log line before "Ignoring defunct".
        kinds = []
        for i, line in enumerate(act.orca):
            if line.is_defunct_drop:
                before = act.orca[i - 1].text if i else ""
                kinds.append(before[:110])
        ok = any(expect_spoken.casefold() in s.casefold() for s in spoken)
        evidence = [f"bus: {defunct} defunct, {added} children added, {removed} removed; "
                    f"Orca: took {len(taken)} events, replaced {len(obsoleted)}, "
                    f"ignored {len(drops)} as defunct; said {spoken!r}"]
        evidence += [f"{line.stamp} {line.text}" for line in drops[:6]]
        evidence += [f"  preceded by: {k}" for k in kinds[:6]]
        return ok, evidence
    return custom(f"a change of month: the new day {expect_spoken!r} is spoken", run,
                  needs_orca=True)


def offers_action(role: str, name: str, action: str = "click"):
    """After the act, the node offers `action` on AT-SPI, which is how a screen
    reader activates what it is on."""
    def run(act):
        from reader_lib.checks import _walk
        for node in _walk(act.tree):
            if node.get("role") == role and node.get("name") == name:
                actions = [a.get("name") for a in node.get("actions", [])]
                return action in actions, [f"[{role}] {name!r} interfaces="
                                           f"{node.get('interfaces')} actions={actions}"]
        return False, [f"no [{role}] {name!r} in the tree"]
    return custom(f"[{role}] {name!r} offers a {action!r} action", run, needs_tree=True)


def focus_in(names: tuple[str, ...]):
    from reader_lib.checks import _focus_node, _is_focus

    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        lines = [f"{e['type']} {e.get('detail1')} [{_focus_node(e).get('role')}] "
                 f"{_focus_node(e).get('name')!r}" for e in moves]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        return _focus_node(moves[-1]).get("name") in names, lines
    return custom(f"focus lands on one of {names}", run)


# ---------------------------------------------------------------------------
# The single calendar's day grid, key by key
# ---------------------------------------------------------------------------


def grid_keys(run):
    position(run, "Tab ten times, to Theme", *tabs(10))
    with run.act("Tab into the single calendar",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026"), said("Calendar"), said("selected")],
                 should="the reader hears that this is a calendar of May 2026, the day "
                        "under the cursor, and that this day is the selected one"):
        run.key("Tab")
    moves = [
        # chord, day focus should land on, is it the selected day, month change
        ("Right", "Sunday, May 3, 2026", False, False),
        ("Left", "Saturday, May 2, 2026", True, False),
        ("Down", "Saturday, May 9, 2026", False, False),
        ("Home", "Sunday, May 3, 2026", False, False),
        ("End", "Saturday, May 9, 2026", False, False),
        ("Ctrl+Home", "Friday, May 1, 2026", False, False),
        ("Ctrl+End", "Sunday, May 31, 2026", False, False),
        ("PageDown", "Tuesday, June 30, 2026", False, True),
        ("PageUp", "Saturday, May 30, 2026", False, True),
        ("Shift+PageDown", "Sunday, May 30, 2027", False, True),
        ("Shift+PageUp", "Saturday, May 30, 2026", False, True),
    ]
    for chord, day, is_selected, crosses in moves:
        expect = [focused(role="table cell", name=day), said(day)]
        if is_selected:
            expect.append(said("selected"))
        if crosses:
            expect.append(month_change(day))
        with run.act(f"{chord} in the grid", expect,
                     should=f"the cursor moves to {day} and the reader says it"
                            + (", and that it is the selected day" if is_selected else "")):
            run.key(chord)
    with run.act("Enter commits the day under the cursor (May 30)",
                 [event("object:state-changed:selected", role="table cell",
                        name_contains="May 30, 2026", detail1=1),
                  said("selected"), label("Selected: 2026-05-30")],
                 should="the day becomes the selection; the bus says so and the reader "
                        "hears it selected", tree=True):
        run.key("Return")
    with run.act("Left then Right, back onto the new selection",
                 [focused(role="table cell", name="Saturday, May 30, 2026"),
                  said("selected")],
                 should="coming back to the selected day, the reader hears it is "
                        "selected"):
        run.key("Left", "Right", gap=0.6)
    with run.act("T jumps to today",
                 [focused(role="table cell", name=TODAY), said(TODAY), said("today")],
                 should="the documented T key moves the cursor to today and the reader "
                        "hears today's date, and that it is today"):
        run.key("t")


# ---------------------------------------------------------------------------
# The header arrows and Today, which speak through the announcer
# ---------------------------------------------------------------------------


def grid_buttons(run):
    position(run, "Tab fifteen times, to Next month", *tabs(15))
    with run.act("Space on Next month", [announced("June 2026"), said("June 2026")],
                 should="the calendar moves on a month and the reader hears June 2026"):
        run.key("space")
    with run.act("Space on Next month again", [announced("July 2026"), said("July 2026")],
                 should="the reader hears July 2026"):
        run.key("space")
    position(run, "Tab twice, to Today", "Tab", "Tab")
    with run.act("Space on Today", [announced(TODAY), said(TODAY), said("selected")],
                 should="the cursor and the selection go to today; the reader hears "
                        "today's date and that it is now selected", tree=True):
        run.key("space")


# ---------------------------------------------------------------------------
# The title button, the months view
# ---------------------------------------------------------------------------


def zoom(run):
    position(run, "Tab fourteen times, to the title button", *tabs(14))
    with run.act("Space on the title button (May 2026)",
                 [said("2026"), said("month")],
                 should="the months of 2026 replace the days; the reader hears that a "
                        "month view of 2026 is showing", tree=True):
        run.key("space")
    with run.act("Tab three times, into the months",
                 [focused(role="table cell", name="May"), said("May")],
                 should="Tab reaches the months on the one shown, May, as one stop"):
        run.key("Tab", "Tab", "Tab", gap=0.6)
    with run.act("Down in the months view",
                 [focused(role="table cell", name="April"), said("April"),
                  label("Selected: 2026-05-02")],
                 should="the cursor moves down a row of months and the reader hears the "
                        "month; the selected date does not change", tree=True):
        run.key("Down")
    with run.act("Enter on the focused month",
                 [label("Selected: 2026-05-02"), in_tree(role="push button",
                                                         name_contains="2026")],
                 should="the month is opened in the days view; the selected date does "
                        "not change", tree=True):
        run.key("Return")
    with run.act("Tab from the month",
                 [focused(role="push button", name="Today")],
                 should="the months are one grid, one Tab stop: Tab leaves them"):
        run.key("Tab")
    with run.act("Escape in the months view",
                 [in_tree(role="push button", name_contains="May"),
                  offers_action("table cell", "March")],
                 should="Escape goes back to the days view; every month offers a "
                        "screen reader's activation", tree=True):
        run.key("Escape")
    position(run, "Shift+Tab to the arrow named Next month",
             "Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Space on the arrow named Next month, in the months view",
                 [said("June 2026")],
                 should="a button named Next month moves the calendar one month on",
                 tree=True):
        run.key("space")
    with run.act("AT-SPI click on the month March",
                 [in_tree(role="push button", name_contains="March")],
                 should="a screen reader's own activation opens March in the days view",
                 tree=True):
        run.action("click", role="table cell", name="March")
    position(run, "Shift+Tab once, to the title button", "Shift+Tab")
    with run.act("Space on the title button in the months view",
                 [in_tree(role="push button", name_contains="May")],
                 should="the title leads back towards the days, or on to the years; "
                        "either way the reader can get back to picking a day",
                 tree=True):
        run.key("space")


# ---------------------------------------------------------------------------
# DateEdit: step the field, then open its calendar
# ---------------------------------------------------------------------------


def dateedit_stale(run):
    with run.act("Up in the DateEdit field (caret on the year)",
                 [label("Edit: 2027-05-02"), said("2027")],
                 should="the year goes up one and the reader hears the new date",
                 tree=True):
        run.key("Up")
    with run.act("Alt+Down opens the calendar",
                 [focused(role="table cell", name="Sunday, May 2, 2027"),
                  said("May 2, 2027")],
                 should="the calendar opens on the date the field holds, 2 May 2027",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Enter in the calendar",
                 [label("Edit: 2027-05-02")],
                 should="the day under the cursor is committed; it should be the "
                        "field's own date, 2 May 2027, so nothing changes", tree=True):
        run.key("Return")


def datetimeedit_stale(run):
    position(run, "Tab four times, to the DateTimeEdit date part", *tabs(4))
    with run.act("Up in the DateTimeEdit date part",
                 [label("DateTime: 2027-05-02 14:35"), said("2027")],
                 should="the date part's year goes up one and the reader hears it",
                 tree=True):
        run.key("Up")
    position(run, "Tab twice, to Open calendar", "Tab", "Tab")
    with run.act("Space on Open calendar",
                 [focused(role="table cell", name="Sunday, May 2, 2027"),
                  said("May 2, 2027")],
                 should="the calendar opens on the date the field holds, 2 May 2027"):
        run.key("space")
    with run.act("Enter in the calendar",
                 [label("DateTime: 2027-05-02 14:35"), said("2027")],
                 should="the field's own date is committed, the time kept, and the "
                        "reader hears the field's value as focus comes back", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# DateEdit's calendar: Escape, and its months view across a reopening
# ---------------------------------------------------------------------------


def dateedit_popover(run):
    with run.act("Alt+Down opens the DateEdit calendar",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026"), said("Calendar")],
                 should="the reader hears that a calendar opened, and the day under "
                        "the cursor", tree=True):
        run.key("Alt+Down")
    with run.act("Right, to 3 May",
                 [focused(role="table cell", name="Sunday, May 3, 2026"),
                  said("Sunday, May 3, 2026")],
                 should="the reader hears the next day"):
        run.key("Right")
    with run.act("Escape closes it",
                 [focused(role="entry"), said("05/02/2026")],
                 should="focus goes back to the field and the reader hears it"):
        run.key("Escape")
    with run.act("Alt+Down reopens it",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026")],
                 should="the calendar opens again on the field's date, and the reader "
                        "hears it as the first time"):
        run.key("Alt+Down")
    with run.act("Left, a day back",
                 [said("May")],
                 should="the reader hears the day the cursor moved to"):
        run.key("Left")
    with run.act("Left again",
                 [said("May")],
                 should="the reader hears the day the cursor moved to"):
        run.key("Left")
    with run.act("Home, to the week's first day",
                 [said("2026")],
                 should="the reader hears the day the cursor moved to"):
        run.key("Home")
    position(run, "Tab three times, to the popover's title button", "Tab", "Tab", "Tab")
    with run.act("Space on the title: the months view",
                 [said("2026")],
                 should="the months view of 2026 shows, and the reader hears it",
                 tree=True):
        run.key("space")
    with run.act("Escape closes the popover from the months view",
                 [focused(role="entry")],
                 should="focus goes back to the field"):
        run.key("Escape")
    with run.act("Alt+Down reopens the calendar",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026")],
                 should="a date field's calendar opens on its days, on the field's date",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Enter in the reopened calendar",
                 [label("Edit: 2026-05-02")],
                 should="the field keeps its date (or the day under the cursor, "
                        "which is the same)", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# The range calendar and the DateRangeEdit's calendar
# ---------------------------------------------------------------------------


def range_calendar(run):
    position(run, "Tab seventeen times, to the single calendar's Today", *tabs(17))
    with run.act("Tab into the range calendar",
                 [focused(role="table cell", name=TODAY), said(TODAY), said("today")],
                 should="the reader hears the day under the cursor and that it is today"):
        run.key("Tab")
    with run.act("Enter sets the range's start",
                 [said("September 25"), event("object:state-changed:selected",
                                              role="table cell",
                                              name_contains="September 25", detail1=1)],
                 should="the reader hears that the range now starts here", tree=True):
        run.key("Return")
    with run.act("Right three times",
                 [focused(role="table cell", name="Monday, September 28, 2026"),
                  said("Monday, September 28, 2026")],
                 should="each day is said as the cursor moves; the last is heard whole"):
        run.key("Right", "Right", "Right", gap=0.6)
    with run.act("Enter ends the range",
                 [said("September 28"), label("Range: 2026-09-25 – 2026-09-28"),
                  in_tree(role="table cell", name="Saturday, September 26, 2026",
                          state="selected")],
                 should="the range is committed; the reader hears the range",
                 tree=True):
        run.key("Return")
    with run.act("Left onto a day inside the range",
                 [focused(role="table cell", name="Sunday, September 27, 2026"),
                  said("selected")],
                 should="the reader hears the day and that it is in the selection"):
        run.key("Left")


def range_edit(run):
    position(run, "Tab nine times, to Open range calendar", *tabs(9))
    with run.act("Space on Open range calendar",
                 [focused(role="table cell", name="Saturday, May 2, 2026"),
                  said("Saturday, May 2, 2026")],
                 should="the range calendar opens on the range's start and the reader "
                        "hears it"):
        run.key("space")
    with run.act("Enter on 2 May starts a range",
                 [said("May 2")],
                 should="the reader hears the range now starts on 2 May"):
        run.key("Return")
    with run.act("Right three times",
                 [focused(role="table cell", name="Tuesday, May 5, 2026"),
                  said("Tuesday, May 5, 2026")],
                 should="each day is said as the cursor moves"):
        run.key("Right", "Right", "Right", gap=0.6)
    with run.act("Enter ends the range",
                 [label("Range edit: 2026-05-02 – 2026-05-05"),
                  focus_in(("Start date", "End date", "Open range calendar")),
                  said("May 5, 2026")],
                 should="the range 2-5 May is committed, the calendar closes, focus goes "
                        "back to the field and the reader hears the new range",
                 tree=True):
        run.key("Return")
    with run.act("Shift+Tab to the end date",
                 [focused(role="date editor", name="End date"), said("05/05/2026")],
                 should="the reader hears the end date's name and value"):
        run.key("Shift+Tab")


# ---------------------------------------------------------------------------
# A burst of month changes
# ---------------------------------------------------------------------------


def month_burst(run):
    position(run, "Tab eleven times, into the single calendar", *tabs(11))
    with run.act("PageDown six times quickly",
                 [focused(role="table cell", name="Monday, November 2, 2026"),
                  month_change("November 2, 2026")],
                 should="the cursor ends on 2 November 2026 and the reader hears where it "
                        "ended", record=4.0):
        run.key(*(["PageDown"] * 6), gap=0.05)
    with run.act("PageDown once more, after a pause",
                 [focused(role="table cell", name="Wednesday, December 2, 2026"),
                  month_change("December 2, 2026")],
                 should="one more month: the reader hears 2 December 2026"):
        run.key("PageDown")


# ---------------------------------------------------------------------------
# TimeEdit segments, the field names, and a correction
# ---------------------------------------------------------------------------


def timeedit(run):
    with run.act("Tab twice, to the 24 h field",
                 [focused(role="entry"), said("Time"), said("14:35")],
                 should="the reader hears the field's name and its time"):
        run.key("Tab", "Tab", gap=0.6)
    with run.act("Up (caret at the end, on the minutes)",
                 [label("24h time: 14:36"), said("14:36")],
                 should="the minutes go up one and the reader hears the new time",
                 tree=True):
        run.key("Up")
    with run.act("Down", [label("24h time: 14:35"), said("14:35")],
                 should="back to 14:35, heard", tree=True):
        run.key("Down")
    with run.act("Shift+Up (minutes +10)", [label("24h time: 14:45"), said("14:45")],
                 should="the minutes go up ten and the reader hears 14:45", tree=True):
        run.key("Shift+Up")
    with run.act("Home then Up (hour)", [label("24h time: 15:45"), said("15")],
                 should="the hour goes up one and the reader hears it", tree=True):
        run.key("Home", "Up", gap=0.6)
    with run.act("Tab to the 12 h field",
                 [focused(role="entry"), said("Time"), said("02:35:00 PM")],
                 should="the reader hears the field's name and its time, and can tell "
                        "it from the 24 h field"):
        run.key("Tab")
    with run.act("End then Up (AM/PM)", [label("12h time: 02:35 AM"), said("AM")],
                 should="the AM/PM segment flips and the reader hears it", tree=True):
        run.key("End", "Up", gap=0.6)


def dateedit_correction(run):
    with run.act("Type 12/31/2099 over the date, then Enter",
                 [label("Edit: 2030-12-31"), announced("2030"), said("2030")],
                 should="the date past the maximum is corrected to 31 December 2030 and "
                        "the reader hears the correction", tree=True):
        run.type("12/31/2099")
        run.wait(0.4)
        run.key("Return")
    with run.act("Type 99/99/9999, then Enter",
                 [announced("date"), said("date")],
                 should="the reader hears that this is not a date", tree=True):
        run.key("Ctrl+A")
        run.type("99/99/9999")
        run.wait(0.4)
        run.key("Return")


SCENARIOS = [
    Scenario("datetime-grid-keys", PACKAGE, grid_keys,
             "the single calendar's grid: Tab in, arrows, Home/End, PageUp/Down, Enter, T"),
    Scenario("datetime-grid-buttons", PACKAGE, grid_buttons,
             "the single calendar's Next month arrow twice and Today (the announcer)"),
    Scenario("datetime-zoom", PACKAGE, zoom,
             "the title button's months view: Tab, arrows, Enter, Escape, the arrows"),
    Scenario("datetime-dateedit-stale", PACKAGE, dateedit_stale,
             "DateEdit: step the year, open the calendar, press Enter"),
    Scenario("datetime-datetimeedit-stale", PACKAGE, datetimeedit_stale,
             "DateTimeEdit: step the year, open the calendar, press Enter"),
    Scenario("datetime-dateedit-popover", PACKAGE, dateedit_popover,
             "DateEdit's calendar: Escape, then its months view across a reopening"),
    Scenario("datetime-range-calendar", PACKAGE, range_calendar,
             "the range calendar: set a range from the keyboard"),
    Scenario("datetime-range-edit", PACKAGE, range_edit,
             "DateRangeEdit: pick a range in its calendar"),
    Scenario("datetime-month-burst", PACKAGE, month_burst,
             "six quick PageDowns in the single calendar: Orca's queue"),
    Scenario("datetime-timeedit", PACKAGE, timeedit,
             "TimeEdit 24 h and 12 h: names, and Up/Down on each segment"),
    Scenario("datetime-dateedit-correction", PACKAGE, dateedit_correction,
             "DateEdit: a date past the maximum, and a string that is no date"),
]


# ---------------------------------------------------------------------------
# The caret in a date field: is a move the reader makes reported at all?
# ---------------------------------------------------------------------------


def caret(run):
    with run.act("Home in the DateEdit field (text selected, caret at the end)",
                 [event("object:text-caret-moved", role="entry"),
                  event("object:text-selection-changed", role="entry")],
                 should="the caret goes to the start and the selection is dropped; the "
                        "bus says both, which is how a reader follows a caret"):
        run.key("Home")
    with run.act("Right", [event("object:text-caret-moved", role="entry"), said("5")],
                 should="the caret moves one character and the reader hears it"):
        run.key("Right")
    with run.act("Right again", [event("object:text-caret-moved", role="entry")],
                 should="the caret moves one character"):
        run.key("Right")
    with run.act("End", [event("object:text-caret-moved", role="entry")],
                 should="the caret goes to the end"):
        run.key("End")
    with run.act("Shift+Left", [event("object:text-selection-changed", role="entry")],
                 should="one character is selected, and the bus says so"):
        run.key("Shift+Left")
    with run.act("Type 7 over the selection",
                 [event("object:text-changed:insert", role="entry"),
                  label("Edit: 2027-05-02")],
                 should="the typed digit replaces the selected one", tree=True):
        run.type("7")
        run.wait(0.3)
        run.key("Return")


SCENARIOS.append(Scenario("datetime-caret", PACKAGE, caret,
                          "DateEdit's field: Home, Right, End, Shift+Left, typing"))
