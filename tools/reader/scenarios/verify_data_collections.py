# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for data-collections (the sweep's own acts are in
`collections.py`; these only add what its scenarios did not do).

* a cursor-only move that needs no scroll (Ctrl+Home from row 3, Ctrl+Down),
  to tell whether the sweep's "Ctrl+End works" is the scroll's doing, and
  what a reader gets when focus leaves the list and comes back after one;
* the row menu closed with Escape, and a command activated through AT-SPI
  (a screen reader's own activation), which scrolls the moved row out;
* the TreeView's level on navigation (Orca says "tree level N" when it
  changes, for a tree that exports NODE_CHILD_OF), Home/End, an AT-SPI click
  on a row, and Space on a branch's tristate box.
"""

from __future__ import annotations

from reader_lib.checks import _event_line, _focus_node, _is_focus, announced, custom, event, \
    focused, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

from scenarios.data_collections import checkbox_state, enter_list, focused_row_on_bus, row_has

PACKAGE = "data-collections"


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        lines = [_event_line(act, e) for e in moves] or ["no focus change on the bus"]
        return True, lines
    return custom("record: the act's focus changes", run)


def speech_record():
    """Record what Orca said in the act (always passes)."""
    def run(act):
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                      for u in utterances(act.orca)] or ["Orca said nothing"]
    return custom("record: what Orca said", run, needs_orca=True)


def event_counts():
    """Record how many events of each type the act sent (always passes)."""
    def run(act):
        counts: dict[str, int] = {}
        for e in act.events:
            counts[e["type"]] = counts.get(e["type"], 0) + 1
        return True, [f"{len(act.events)} events: {counts}"]
    return custom("record: event counts", run)


# ---------------------------------------------------------------------------
# ListView: cursor-only moves that need no scroll
# ---------------------------------------------------------------------------


def cursor_noscroll(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down x3: row 3", [focused(role="list item", name="Item 3")],
                 should="the cursor on row 3, selected"):
        run.key("Down", "Down", "Down")
    with run.act("Ctrl+Home: cursor only, row 1, no scroll needed",
                 [focused(role="list item", name="Item 1"), said("Item 1"),
                  focus_sequence(), speech_record()],
                 should="the cursor jumps to row 1 (already on screen) and the reader "
                        "hears it", tree=True):
        run.key("Ctrl+Home")
    with run.act("Ctrl+Down: cursor only, row 2",
                 [focused(role="list item", name="Item 2"), said("Item 2"),
                  focus_sequence(), speech_record()],
                 should="the cursor moves to row 2 and the reader hears it", tree=True):
        run.key("Ctrl+Down")
    with run.act("nothing pressed, 3 s later",
                 [focused_row_on_bus("list item", "Item 2")],
                 should="the bus has caught up with the cursor", record=3.0, tree=True):
        pass
    with run.act("Tab out of the list", [focus_sequence(), speech_record()],
                 should="focus leaves the list"):
        run.key("Tab")
    with run.act("Shift+Tab back into the list",
                 [focused(role="list item", name="Item 2"), said("Item 2"),
                  focus_sequence(), speech_record()],
                 should="focus comes back to the row the cursor is on (row 2)", tree=True):
        run.key("Shift+Tab")


# ---------------------------------------------------------------------------
# ListView: the row menu, Escape, and an AT-SPI activation of a command
# ---------------------------------------------------------------------------


def menu_more(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: row 1", [focused(role="list item", name="Item 1")],
                 should="the cursor on row 1"):
        run.key("Down")
    with run.act("Shift+F10: the row's menu", [focus_sequence(), speech_record()],
                 should="the menu opens and the reader hears where focus is", tree=True):
        run.key("Shift+F10")
    with run.act("Down, Down, Up in the menu",
                 [focus_sequence(), speech_record(), event_counts(),
                  event("object:", role="menu item")],
                 should="each arrow reads the highlighted command", tree=True):
        run.key("Down", "Down", "Up")
    with run.act("Escape closes the menu",
                 [focused(role="list item", name="Item 1"), said("Item 1"),
                  focus_sequence(), speech_record()],
                 should="focus returns to row 1 and the reader hears it", tree=True):
        run.key("Escape")
    with run.act("Shift+F10 again", [focus_sequence(), speech_record()],
                 should="the menu opens again", tree=True):
        run.key("Shift+F10")
    with run.act("AT-SPI click on 'Move to Bottom'",
                 [announced("Moved to 200 of 200"), said("Moved to 200 of 200"),
                  focused(role="list item", name="Item 1"),
                  focus_sequence(), speech_record(), event_counts()],
                 should="a screen reader's own activation runs the command: Item 1 goes to "
                        "the bottom, focus follows it, the reader hears where it went",
                 tree=True):
        run.action("click", role="menu item", name="Move to Bottom")


# ---------------------------------------------------------------------------
# TreeView: level on navigation, Home/End, AT-SPI click, a branch's box
# ---------------------------------------------------------------------------


def tree_levels(run):
    enter_list(run, "TreeView", "- Remove Last Root")
    with run.act("Tab into the tree", [focused(role="tree")], should="focus on the tree"):
        run.key("Tab")
    with run.act("Down: Documents", [focused(role="tree item", name="Documents"),
                                     speech_record(), event_counts()],
                 should="the cursor on Documents"):
        run.key("Down")
    with run.act("Right: expand Documents",
                 [said("expanded"), focus_sequence(), speech_record(), event_counts()],
                 should="Documents opens and the reader hears 'expanded'", tree=True):
        run.key("Right")
    with run.act("Down: Projects, one level down",
                 [focused(role="tree item", name="Projects"), said("level 2"),
                  row_has("tree item", "Projects", attribute="level"),
                  focus_sequence(), speech_record(), event_counts()],
                 should="the cursor goes one level down; Orca says 'tree level 2' as it "
                        "does in a GTK tree when the level changes", tree=True):
        run.key("Down")
    with run.act("Up: back to Documents, level 1",
                 [focused(role="tree item", name="Documents"), said("level 1"),
                  speech_record()],
                 should="the level changes back and the reader hears it", tree=True):
        run.key("Up")
    with run.act("End: last row", [focused(role="tree item", name="Downloads"),
                                   said("Downloads"), speech_record()],
                 should="the cursor jumps to the last visible row", tree=True):
        run.key("End")
    with run.act("Home: first row", [focused(role="tree item", name="Documents"),
                                     said("Documents"), speech_record()],
                 should="the cursor jumps back to the first row", tree=True):
        run.key("Home")
    with run.act("AT-SPI click on Pictures",
                 [focused(role="tree item", name="Pictures"), said("Pictures"),
                  row_has("tree item", "Pictures", state="selected"),
                  focus_sequence(), speech_record()],
                 should="a screen reader's activation of a row selects it and moves the "
                        "cursor there", tree=True):
        run.action("click", role="tree item", name="Pictures")
    with run.act("Space on Pictures (a branch with a tristate box)",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Pictures", detail1=1),
                  checkbox_state("Pictures", "checked"),
                  speech_record(), event_counts()],
                 should="Space checks the branch; the bus says so", tree=True):
        run.key("space")
    with run.act("nothing pressed, 3 s later",
                 [checkbox_state("Pictures", "checked")],
                 should="the bus has caught up", record=3.0, tree=True):
        pass


SCENARIOS = [
    Scenario("verify-collections-cursor-noscroll", PACKAGE, cursor_noscroll,
             "ListView: Ctrl+Home / Ctrl+Down cursor-only moves that need no scroll, "
             "then Tab out and back"),
    Scenario("verify-collections-menu-more", PACKAGE, menu_more,
             "ListView: row menu arrows, Escape, and an AT-SPI click on Move to Bottom"),
    Scenario("verify-collections-tree-levels", PACKAGE, tree_levels,
             "TreeView: level on navigation, Home/End, AT-SPI click, Space on a branch"),
]
