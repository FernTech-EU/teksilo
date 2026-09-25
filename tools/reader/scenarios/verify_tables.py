# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for data-grid and tree-table-view (the sweep's own
acts are in `tables.py`; these only add what its scenarios did not do).

* the first and the second entry into the grid, and plain Tab from a table
  with no cursor yet (does Tab skip the first cell?);
* a column filter applied while the grid already has a cursor (is the
  silence after Enter only the no-cursor case?), and the popover's Clear;
* a data change the reader did not make (a live append through AT-SPI)
  while the cursor sits on a cell: is the cell re-announced?
* PageDown / End / Home;
* the tree table: a cursor-only move, Space, the Selection interface, and
  what a row and the tree cell publish.
"""

from __future__ import annotations

from reader_lib.checks import _event_line, _focus_node, _is_focus, custom, focused, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

from scenarios.tables import (GRID_TABS_BEFORE, TREE_TABS_BEFORE, _find, _tab_to, _walk,
                              focused_node_facts, orca_spoke, replaced, said_cell)


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        return True, [_event_line(act, e) for e in moves] or ["no focus change on the bus"]
    return custom("record: the act's focus changes", run)


def speech_record():
    """Record what Orca said in the act (always passes)."""
    def run(act):
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                      for u in utterances(act.orca)] or ["Orca said nothing"]
    return custom("record: what Orca said", run, needs_orca=True)


def no_focus_change():
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        return not moves, [_event_line(act, e) for e in moves] or ["no focus change"]
    return custom("no focus change on the bus", run)


def container_record(role: str):
    """Record the view's name, states, interfaces, selected children."""
    def run(act):
        node = _find(act.tree, role)
        if node is None:
            return False, [f"no [{role}] in the tree"]
        return True, [f"[{role}] name={node.get('name')!r} states={node.get('states')} "
                      f"interfaces={node.get('interfaces')} "
                      f"selected_children={node.get('selected_children')} "
                      f"attributes={node.get('attributes')}"]
    return custom(f"record: the [{role}] as a reader can query it", run, needs_tree=True)


def rows_record(role: str, limit: int = 8):
    """Record the first body rows: states, attributes, and their first cell."""
    def run(act):
        table = _find(act.tree, role)
        out = []
        for n in _walk(table or {}):
            if n.get("role") == "table row" and "selectable" in n.get("states", []):
                cells = [c for c in n.get("children", []) if c.get("role") == "table cell"]
                texts = []
                if cells:
                    texts = [d.get("name") for d in _walk(cells[0]) if d.get("role") == "label"]
                out.append(f"row states={n.get('states')} attrs={n.get('attributes')} | "
                           f"cell0 states={cells[0].get('states') if cells else None} "
                           f"attrs={cells[0].get('attributes') if cells else None} "
                           f"labels={texts}")
                if len(out) >= limit:
                    break
        return True, out or ["no body rows"]
    return custom(f"record: the first body rows of the [{role}]", run, needs_tree=True)


# ---------------------------------------------------------------------------
# data-grid: first and second entry, Tab from a cursor-less table
# ---------------------------------------------------------------------------


def grid_entry(run):
    _tab_to(run, GRID_TABS_BEFORE)
    with run.act("Tab into the table (first entry, no cursor)",
                 [focused(role="table"), focus_sequence(), speech_record(),
                  container_record("table")], tree=True,
                 should="focus lands on the grid; the reader hears something"):
        run.key("Tab")
    with run.act("plain Tab from the cursor-less table",
                 [focused(role="table cell"), said_cell("1"), focus_sequence(),
                  speech_record()],
                 should="Tab puts the cursor on the first cell (row 1, ID: '1'), "
                        "as the arrows do"):
        run.key("Tab")
    with run.act("Shift+Tab", [focus_sequence(), speech_record()],
                 should="(record) where Shift+Tab goes from there"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab again at the first cell",
                 [focus_sequence(), speech_record(),
                  focused(role="push button", name="+ Append row")],
                 should="Shift+Tab at the first cell leaves the grid backwards"):
        run.key("Shift+Tab")
    with run.act("Ctrl+Shift+Tab out of the grid",
                 [focused(role="push button", name="+ Append row"), speech_record()],
                 should="focus goes back to '+ Append row'"):
        run.key("Ctrl+Shift+Tab")
    with run.act("Tab back into the table (second entry, cursor on row 1)",
                 [focused(role="table cell"), said_cell("1"), focus_sequence(),
                  speech_record()],
                 should="focus comes back to the cell the cursor is on and the reader "
                        "hears it"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# data-grid: filter with a cursor, Clear
# ---------------------------------------------------------------------------


def grid_filter_cursor(run):
    _tab_to(run, GRID_TABS_BEFORE + 1)
    with run.act("ArrowDown onto row 1", [said_cell("1")], should="the cursor is on row 1"):
        run.key("Down")
    with run.act("ArrowRight to the Name cell", [said_cell("Avery 0")],
                 should="the cursor is on 'Avery 0'"):
        run.key("Right")
    with run.act("activate the Name column's Filter (AT-SPI)",
                 [focused(role="entry"), focus_sequence(), speech_record()],
                 should="focus moves into the filter field", tree=True):
        run.action("click", role="push button", name="Filter", nth=0)
    focused_node_facts(run, "in the filter field")
    with run.act("type 'Blake'", [focus_sequence()], should="(scene)"):
        run.type("Blake")
    with run.act("Enter applies the filter (with a cursor set before)",
                 [focus_sequence(), speech_record(), orca_spoke(),
                  container_record("table")],
                 should="the table is filtered; focus lands somewhere the reader hears",
                 tree=True, record=3.0):
        run.key("Return")
    focused_node_facts(run, "after Enter")
    with run.act("ArrowDown after the filter", [focus_sequence(), speech_record()],
                 should="(record) where the cursor is now"):
        run.key("Down")
    with run.act("open the Name filter again (AT-SPI) and Clear it (AT-SPI)",
                 [focus_sequence(), speech_record(), container_record("table")],
                 should="(record) the filter is cleared; where focus lands",
                 tree=True, record=3.0):
        run.action("click", role="push button", name="Filter", nth=0)
        run.wait(1.5)
        run.action("click", role="push button", name="Clear")


# ---------------------------------------------------------------------------
# data-grid: a live data change under the cursor; paging keys
# ---------------------------------------------------------------------------


def grid_live(run):
    _tab_to(run, GRID_TABS_BEFORE + 1)
    with run.act("ArrowDown onto row 1", [said_cell("1")], should="the cursor is on row 1"):
        run.key("Down")
    with run.act("a live append the reader did not make (AT-SPI click on '+ Append row')",
                 [no_focus_change(), focus_sequence(), speech_record(),
                  replaced(10)],
                 should="a row is added at the end, off screen; the cell the reader is "
                        "on is not re-announced and focus stays put"):
        run.action("click", role="push button", name="+ Append row")
    with run.act("a second live append",
                 [no_focus_change(), focus_sequence(), speech_record(), replaced(10)],
                 should="the same, again"):
        run.action("click", role="push button", name="+ Append row")
    with run.act("PageDown", [focus_sequence(), speech_record(), replaced(10)],
                 should="(record) the cursor pages down and the reader hears the cell",
                 record=3.0):
        run.key("PageDown")
    with run.act("End", [focus_sequence(), speech_record()],
                 should="(record) End: last cell of the row, or last row"):
        run.key("End")
    with run.act("Home", [focus_sequence(), speech_record()],
                 should="(record) Home"):
        run.key("Home")
    with run.act("Ctrl+Down (cursor only)", [focus_sequence(), speech_record(),
                                               replaced(10)],
                 should="(record) the cursor moves, nothing is rebuilt"):
        run.key("Ctrl+Down")


# ---------------------------------------------------------------------------
# tree-table-view: cursor-only move, Space, what rows publish
# ---------------------------------------------------------------------------


def ttv_more(run):
    _tab_to(run, TREE_TABS_BEFORE, role="tree table")
    with run.act("Tab into the tree table", [focused(role="tree table"), speech_record(),
                                              container_record("tree table")],
                 should="(record) first entry", tree=True):
        run.key("Tab")
    with run.act("ArrowDown onto the first row",
                 [focused(role="table cell"), speech_record(), replaced(10),
                  rows_record("tree table"), container_record("tree table")],
                 should="(record) the first row, and what rows publish", tree=True):
        run.key("Down")
    focused_node_facts(run, "on the first row")
    with run.act("Ctrl+ArrowDown: cursor only",
                 [focused(role="table cell"), speech_record(), replaced(10),
                  container_record("tree table")],
                 should="the cursor moves without selecting; nothing needs rebuilding",
                 tree=True):
        run.key("Ctrl+Down")
    with run.act("Space selects the cursor row",
                 [no_focus_change(), speech_record(), replaced(10),
                  container_record("tree table"), rows_record("tree table")],
                 should="the row becomes selected as a state change on the cell the "
                        "reader is on", tree=True):
        run.key("space")
    with run.act("Tab from a tree cell", [focus_sequence(), speech_record()],
                 should="(record) Tab in the tree table"):
        run.key("Tab")
    with run.act("Ctrl+Tab leaves the tree table", [focus_sequence(), speech_record()],
                 should="focus leaves"):
        run.key("Ctrl+Tab")


def ttv_empty_cell(run):
    _tab_to(run, TREE_TABS_BEFORE + 1, role="tree table")
    with run.act("ArrowDown x3 to 'docs'", [speech_record(), focus_sequence()],
                 should="(scene) the cursor on the folder 'docs'"):
        run.key("Down", "Down", "Down", gap=0.8)
    with run.act("Tab to the folder's empty Size cell",
                 [focused(role="table cell"), orca_spoke(), speech_record()],
                 should="the reader hears that the cell is empty ('blank') rather "
                        "than nothing", tree=True):
        run.key("Tab")
    focused_node_facts(run, "on the docs Size cell")
    with run.act("Tab to the Kind cell", [speech_record()],
                 should="(record) 'folder'"):
        run.key("Tab")


SCENARIOS = [
    Scenario("verify-tables-ttv-empty", "tree-table-view", ttv_empty_cell,
             "tree table: what an empty cell (a folder's Size) says"),
    Scenario("verify-tables-entry", "data-grid", grid_entry,
             "first/second entry into the grid; Tab and Shift+Tab from a cursor-less table"),
    Scenario("verify-tables-filter-cursor", "data-grid", grid_filter_cursor,
             "a column filter applied while the grid has a cursor, then Clear"),
    Scenario("verify-tables-live", "data-grid", grid_live,
             "a live append under the cursor; PageDown/End/Home/Ctrl+Down"),
    Scenario("verify-tables-ttv", "tree-table-view", ttv_more,
             "tree table: cursor-only move, Space, what rows publish"),
]
