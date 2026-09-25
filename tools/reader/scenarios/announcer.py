# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The framework's announcer, heard through a calendar's header arrows.

A header arrow says where the calendar moved to, the new title, through the
tree's announcer (`crates/teksilo-widgets/src/calendar/header.rs`, the
`announce` in `step_single`), while focus stays on the arrow. So pressing it
twice is two messages from the announcer in a row, with no focus change to
cut either: the plainest case of the announcer speaking, and the one the
announcer's reuse of a node breaks. Every message is spoken through one of two
reserved nodes (`NodeId(1)` polite, `NodeId(2)` assertive), hidden between
messages. `accesskit_atspi_common` announces a node that leaves the filtered
tree defunct (`adapter.rs`, `remove_node`) and nothing unsays it when the
same id comes back, and Orca 46.1 drops an announcement from a defunct object
("Ignoring defunct object", `event_manager.py`). So the first message of a
session is heard and every later one is not.
"""

import datetime as dt

from reader_lib.checks import announced, said
from reader_lib.scenario import Scenario


def month_after(title: str, months: int) -> str:
    shown = dt.datetime.strptime(title, "%B %Y").date()
    index = shown.year * 12 + (shown.month - 1) + months
    return dt.date(index // 12, index % 12 + 1, 1).strftime("%B %Y")


def body(run):
    arrow = {"role": "push button", "name": "Next month"}
    run.wait_for(**arrow)
    grid = run.wait_for(role="table", name_startswith="Calendar, ")
    shown = grid["name"].split(", ", 1)[1]
    run.note(f"the first calendar shows {shown} at launch")
    run.grab_focus(**arrow)
    for step in (1, 2, 3):
        title = month_after(shown, step)
        with run.act(f"Space on Next month, press {step}",
                     [announced(title), said(title)],
                     should=f"the calendar moves on a month and the reader hears {title!r}"):
            run.key("space")


SCENARIOS = [
    Scenario("announcer-calendar-arrows", "datetime-pickers", body,
             "three presses of a calendar's Next month arrow, each announced"),
]
