# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""TableView (`data-grid`) and TreeTableView (`tree-table-view`), as a
screen reader meets them.

`data-grid` is a 1000-row, 7-column `TableView` in `MultiRow` selection mode
(`examples/data_grid/src/main.rs`), sorted by ID ascending at launch, with an
editable Name column (F2 / type-to-edit) and three filterable columns (Name,
Email, Role). `tree-table-view` is a 3-column `TreeTableView` over a mock
filesystem (`examples/tree_table_view/src/main.rs`), sorted by Name
ascending, `MultiRow` as well.

Where the accessibility comes from:

* the view is `Role::Grid` / `Role::TreeGrid` with `row_count` /
  `column_count`, and names itself only from `.a11y_label(..)`, which neither
  example calls (`table_view/widget_impl.rs` `accessibility`,
  `tree_table_view/widget_impl.rs` `accessibility`);
* the keyboard cursor is the container's `active_descendant`, pointed at the
  focused cell (`CellA11y`, `table_view/a11y.rs`), which AT-SPI carries as a
  `state-changed:focused` on the cell;
* headers are `Role::ColumnHeader` with the sort direction and, for a
  resizable column, Increment / Decrement (`table_view/header.rs`
  `HeaderCell::accessibility`); the header cell is not focusable and sorts,
  reorders and resizes only from the pointer;
* the tree table's rows carry level / expanded / position in set
  (`TreeRowA11y`), mirrored onto the tree column's cell.

What the Linux adapter does with it (accesskit_atspi_common-0.20.0): no
AT-SPI Table / TableCell interface, no `rowindex` / `colindex` / `level` /
`sort` attribute, no expandable / expanded state (`node.rs` `state()` and
`attributes()`), and only a `click` action. Orca 46.1 treats a `table`
without the Table interface as a layout table (`ax_table.py`
`is_layout_table`). The checks below state what a reader should get anyway;
their evidence says what they get.

None of the widgets here speak through `ctx.announce`, except the
TreeTableView's keyboard row move / reparent (`tree_table_view/widget_impl.rs`,
the two `ctx.announce` calls), which neither example enables. So nothing in
these scenarios depends on the announcer (K2).

Orca does not hear the harness's keys (it reads its own X display), so its
key-dependent speech (Space announcing a selection change, `onSelectedChanged`
in `scripts/default.py`; typed-character echo) is not exercised: a finding on
selection speech rests on the events the bus carried, not on Orca's silence.

Every key is pressed inside an act, scene setting included (`_tab_to`): the
harness matches Orca's receipts from the previous act's end, so the speech of
keys pressed between two acts would be credited to the second.
"""

from __future__ import annotations

import re

from reader_lib.checks import _focus_node, _is_focus, custom, event, focused, in_tree, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def _words(text: str) -> str:
    return re.sub(r"\s+", " ", text or "").strip().casefold()


def said_cell(text: str):
    """Orca said `text` as a whole word (a cell's content), heard whole.
    `said()` matches substrings, and a cell reading "1" is in almost
    everything."""
    want = _words(text)
    pattern = re.compile(r"(^|[\s,.;:])" + re.escape(want) + r"($|[\s,.;:])")

    def run(act):
        heard = [u for u in utterances(act.orca) if pattern.search(_words(u.text))]
        whole = [u for u in heard if not u.cut]
        evidence = [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                    for u in utterances(act.orca)] or ["Orca said nothing in the act"]
        return bool(whole), evidence
    return custom(f"Orca says {text!r} as a word, heard whole", run, needs_orca=True)


def orca_spoke():
    """Orca produced some speech for the act's focus, and not 'pauses only'."""
    def run(act):
        pauses = [f"{line.stamp} {line.text}" for line in act.orca
                  if "pauses only" in line.text]
        spoken = [u.text for u in utterances(act.orca)]
        return bool(spoken) and not pauses, (pauses + [f"Orca said {spoken!r}"])
    return custom("Orca has something to say for the new focus", run, needs_orca=True)


def orca_log(run, *needles: str, describe: str):
    """Evidence only (always passes): the lines of Orca's whole debug log,
    inside the act's Orca window, that contain any of `needles`. The act
    itself keeps only speech, stops and event-manager lines, so this reads
    the log file, which exists until the run stops (after the checks)."""
    def run_check(act):
        start = act.orca_start or act.start_wall
        end = act.orca_end or act.end_wall
        text = run.orca.text() if run.orca is not None else ""
        found = []
        for line in text.splitlines():
            stamp = line[:15]
            if len(stamp) == 15 and stamp[2] == ":" and start <= stamp <= end \
                    and any(n in line for n in needles):
                found.append(line[:220])
        return True, found or ["no such line"]
    return custom(describe, run_check, needs_orca=True)


def replaced(max_allowed: int):
    """The act replaced at most `max_allowed` nodes (one `defunct` each)."""
    def run(act):
        gone = [e for e in act.events if e["type"] == "object:state-changed:defunct"
                and e.get("detail1") == 1]
        roles: dict[str, int] = {}
        for e in gone:
            role = e.get("source", {}).get("role") or "?"
            roles[role] = roles.get(role, 0) + 1
        total = len([e for e in act.events if e["type"] != "object:bounds-changed"
                     and not e["type"].startswith("harness:")])
        return len(gone) <= max_allowed, [
            f"{len(gone)} nodes went defunct in the act: {roles}",
            f"{total} events on the bus in the act (bounds changes excluded)"]
    return custom(f"at most {max_allowed} nodes are replaced", run)


def new_focus_not_dropped():
    """Orca did not drop the act's *arriving* focus as defunct. (Dropping the
    old cell's focus-lost event after its row was rebuilt is harmless; losing
    the new cell's focus-gained event is what silences the move.)"""
    def run(act):
        lines = act.orca
        dropped = []
        for i, line in enumerate(lines):
            if not line.is_defunct_drop:
                continue
            # The event Orca was processing: the nearest preceding
            # "... is not obsoleted" line.
            for back in range(i - 1, -1, -1):
                if lines[back].text.endswith("is not obsoleted"):
                    dropped.append(f"{lines[back].stamp} {lines[back].text} -> "
                                   f"{line.stamp} {line.text}")
                    break
        lost = [d for d in dropped if "state-changed:focused" in d and "(1, 0" in d]
        return not lost, dropped or ["Orca dropped no event as defunct"]
    return custom("Orca does not drop the arriving focus as defunct", run, needs_orca=True)


def _walk(node):
    stack = [node]
    while stack:
        n = stack.pop()
        yield n
        stack.extend(reversed(n.get("children", [])))


def _find(tree, role, name=None):
    for n in _walk(tree or {}):
        if n.get("role") == role and (name is None or n.get("name") == name):
            return n
    return None


def container_facts(role: str):
    """The view as a reader's tools see it: its name, interfaces, states and
    attributes, and how many body rows the bus holds. Fails when the view
    has no name or offers no Table interface, which is what a reader needs
    to hear its size and a cell's position."""
    def run(act):
        table = _find(act.tree, role)
        if table is None:
            return False, [f"no [{role}] in the tree after the act"]
        rows = [n for n in _walk(table) if n.get("role") == "table row"
                and "selectable" in n.get("states", [])]
        ok = bool(table.get("name")) and "Table" in (table.get("interfaces") or [])
        return ok, [
            f"[{role}] name={table.get('name')!r} interfaces={table.get('interfaces')} "
            f"attributes={table.get('attributes', {})} states={table.get('states')}",
            f"{len(rows)} body rows are on the bus",
        ]
    return custom(f"the [{role}] is named and offers the Table interface", run,
                  needs_tree=True)


def rows_on_bus(role: str, expect_first: str | None = None):
    """Evidence: which body rows the bus holds (their first cell's text)."""
    def run(act):
        table = _find(act.tree, role)
        if table is None:
            return False, [f"no [{role}] in the tree after the act"]
        firsts = []
        for n in _walk(table):
            if n.get("role") == "table row" and "selectable" in n.get("states", []):
                cells = [c for c in n.get("children", []) if c.get("role") == "table cell"]
                label = None
                if cells:
                    for d in _walk(cells[0]):
                        if d.get("role") == "label":
                            label = d.get("name")
                            break
                firsts.append(label)
        ok = expect_first is None or expect_first in firsts
        return ok, [f"{len(firsts)} body rows on the bus; first cells: {firsts}"]
    return custom(f"the rows on the bus{' include ' + repr(expect_first) if expect_first else ''}",
                  run, needs_tree=True)


def focused_node_facts(run, label: str) -> None:
    """Record what a reader can ask the focused node (outside any check)."""
    node = run.find(focused=True) or {}
    run.note(f"{label}: focused [{node.get('role')}] name={node.get('name')!r} "
             f"states={node.get('states')} attributes={node.get('attributes')} "
             f"interfaces={node.get('interfaces')} actions={node.get('actions')}")


def header_publishes(name: str, what: str):
    """The column header `name` offers `what`: 'sort' (a sort attribute or
    state a reader can read) or 'action' (any AT-SPI action, e.g. to sort)."""
    def run(act):
        node = _find(act.tree, "column header", name)
        if node is None:
            return False, [f"no [column header] {name!r} in the tree"]
        attrs = node.get("attributes", {}) or {}
        record = (f"[column header] {name!r} interfaces={node.get('interfaces')} "
                  f"attributes={attrs} actions={node.get('actions', [])} "
                  f"states={node.get('states')} description={node.get('description')!r}")
        if what == "sort":
            text = _words(" ".join([str(attrs), node.get("description") or ""]))
            return "sort" in text, [record]
        return bool(node.get("actions")), [record]
    return custom(f"the {name!r} header publishes its {what}", run, needs_tree=True)


def row_state_on_bus(role: str, row_first_cell: str, state: str, present: bool = True):
    """The body row whose first cell reads `row_first_cell` has (or lacks)
    `state`, on the row and on its first cell."""
    def run(act):
        table = _find(act.tree, role)
        for n in _walk(table or {}):
            if n.get("role") != "table row" or "selectable" not in n.get("states", []):
                continue
            cells = [c for c in n.get("children", []) if c.get("role") == "table cell"]
            if not cells:
                continue
            text = next((d.get("name") for d in _walk(cells[0])
                         if d.get("role") == "label"), None)
            if text != row_first_cell:
                continue
            row_has = state in n.get("states", [])
            cell_has = state in cells[0].get("states", [])
            ok = (row_has and cell_has) if present else (not row_has and not cell_has)
            return ok, [f"row {text!r} states={n.get('states')} attrs={n.get('attributes')}",
                        f"its first cell states={cells[0].get('states')} "
                        f"attrs={cells[0].get('attributes')}"]
        return False, [f"no body row whose first cell reads {row_first_cell!r}"]
    verb = "has" if present else "lacks"
    return custom(f"the row {row_first_cell!r} {verb} {state!r}", run, needs_tree=True)


def focus_events(max_moves: int | None = None):
    """Evidence: every focus gain in the act, in order."""
    def run(act):
        gains = [e for e in act.events if _is_focus(e)]
        lines = [f"{act.rel_ms(e):+.1f} ms {e['type']} [{_focus_node(e).get('role')}] "
                 f"{_focus_node(e).get('name')!r}" for e in gains]
        ok = max_moves is None or len(gains) <= max_moves
        return ok, lines or ["no focus gain in the act"]
    return custom("evidence: the focus gains of the act"
                  + (f" (at most {max_moves})" if max_moves is not None else ""), run)


# ---------------------------------------------------------------------------
# data-grid
# ---------------------------------------------------------------------------

#: Tab stops before the table: Theme, Reset filters, Reset sort, + Append row.
GRID_TABS_BEFORE = 4


def _tab_to(run, count: int, role: str = "table") -> None:
    """Press Tab `count` times, a reading pace apart, inside an act of its own
    with no checks. (Keys pressed between acts are not an act's: the harness
    matches Orca's receipts from the last act's end, so their speech would be
    credited to the next act.)"""
    run.wait_for(role=role)
    with run.act(f"set the scene: Tab x{count}", should="scene setting, not judged",
                 settle=0.5, record=1.5):
        for _ in range(count):
            run.key("Tab", gap=2.0)


def grid_cells(run):
    """Enter the grid and walk its cells."""
    _tab_to(run, GRID_TABS_BEFORE)
    with run.act("Tab into the table",
                 [focused(role="table"), orca_spoke(), said("1000"),
                  orca_log(run, "is layout only", "pauses only",
                           describe="evidence: why Orca says what it says"),
                  container_facts("table")],
                 should="focus lands on the grid and the reader hears what it is: "
                        "a named table and its size (1000 rows, 7 columns), or a cell",
                 tree=True):
        run.key("Tab")
    with run.act("ArrowDown onto the first row",
                 [focused(role="table cell"), said_cell("1"), said_cell("ID"),
                  said("row 1"), new_focus_not_dropped(), replaced(10),
                  orca_log(run, "table-implementing", "index attributes", "newColumnHeader",
                           describe="evidence: what Orca could find of the table")],
                 should="the cursor lands on row 1, ID column; the reader hears "
                        "'ID', '1' and where it is ('row 1' of 1000)"):
        run.key("Down")
    focused_node_facts(run, "after ArrowDown onto the first row")
    with run.act("ArrowDown to row 2",
                 [focused(role="table cell"), said_cell("2"), said("row 2"),
                  new_focus_not_dropped(), replaced(10)],
                 should="the reader hears '2' and that this is row 2"):
        run.key("Down")
    with run.act("ArrowRight to the Name column",
                 [focused(role="table cell"), said_cell("Blake 1"), said_cell("Name"),
                  new_focus_not_dropped(), replaced(10)],
                 should="the reader hears the new column's header 'Name' and 'Blake 1'"):
        run.key("Right")
    with run.act("ArrowRight to the Email column",
                 [focused(role="table cell"), said_cell("user1@example.com"),
                  said_cell("Email"), new_focus_not_dropped()],
                 should="the reader hears 'Email' and 'user1@example.com'"):
        run.key("Right")
    with run.act("ArrowRight x4 to the Notes column (reading pace)",
                 [said_cell("Editor"), said("$35137"),
                  said_cell("Onboarded in batch 1."), said("Pending equipment request")],
                 should="the cursor passes Role, Salary and Active to Notes; the reader "
                        "hears each cell, and the whole of the two-line note of row 2",
                 record=3.0):
        run.key("Right", "Right", "Right", "Right", gap=1.5)
    with run.act("Ctrl+End to the last row",
                 [focused(role="table cell"), orca_spoke(),
                  said("1000"), rows_on_bus("table", "1000")],
                 should="the cursor jumps to row 1000 (same column: Notes, which reads "
                        "'—' for ID 1000) and the reader hears it and where it is; the rows "
                        "around it are on the bus",
                 tree=True):
        run.key("Ctrl+End")
    with run.act("Ctrl+Home back to the first row",
                 [focused(role="table cell"), said("Onboarded in batch 1"),
                  rows_on_bus("table", "1")],
                 should="the cursor comes back to row 1 and the reader hears it",
                 tree=True):
        run.key("Ctrl+Home")


def grid_pace(run):
    """ArrowDown at a reading pace and at a key-repeat pace."""
    _tab_to(run, GRID_TABS_BEFORE + 1)
    with run.act("ArrowDown onto row 1", [said_cell("1")], should="the cursor is on row 1"):
        run.key("Down")
    with run.act("four ArrowDowns at a reading pace (0.8 s apart)",
                 [said_cell("2"), said_cell("3"), said_cell("4"), said_cell("5"),
                  new_focus_not_dropped(), replaced(1000), focus_events()],
                 should="each row is read as the cursor passes it, ending on ID 5",
                 record=3.0):
        run.key("Down", "Down", "Down", "Down", gap=0.8)
    with run.act("six ArrowDowns at key-repeat pace (0.1 s apart)",
                 [said_cell("11"), new_focus_not_dropped(), replaced(2000), focus_events()],
                 should="the reader may skip rows in passing, but hears the row the "
                        "cursor stops on (ID 11)",
                 record=3.0):
        run.key("Down", "Down", "Down", "Down", "Down", "Down", gap=0.1)
    with run.act("one ArrowDown after the burst",
                 [said_cell("12"), new_focus_not_dropped()],
                 should="the reader hears '12'"):
        run.key("Down")
    with run.act("Ctrl+ArrowDown: move the cursor without selecting",
                 [said_cell("13"), new_focus_not_dropped(), replaced(10)],
                 should="the cursor moves to row 13 without changing the selection; "
                        "nothing needs rebuilding"):
        run.key("Ctrl+Down")


def grid_selection(run):
    """Row selection in a MultiRow table."""
    _tab_to(run, GRID_TABS_BEFORE + 1)
    with run.act("ArrowDown selects row 1",
                 [focused(role="table cell"), said_cell("1"),
                  event("object:state-changed:selected", detail1=1),
                  row_state_on_bus("table", "1", "selected"),
                  event("object:selection-changed", role="table")],
                 should="the cursor lands on row 1 and selects it (selection follows "
                        "the cursor); the bus says the row became selected",
                 tree=True):
        run.key("Down")
    with run.act("Space unselects row 1",
                 [event("object:state-changed:selected", detail1=0),
                  row_state_on_bus("table", "1", "selected", present=False),
                  focus_events(max_moves=0), said("not selected")],
                 should="Space toggles the focused row off; the reader hears that it is "
                        "no longer selected (a state change on the cell the reader is on, "
                        "not a new focus)",
                 tree=True):
        run.key("space")
    with run.act("Space selects row 1 again",
                 [event("object:state-changed:selected", detail1=1),
                  row_state_on_bus("table", "1", "selected"), focus_events(max_moves=0)],
                 should="Space toggles it back on, as a state change",
                 tree=True):
        run.key("space")
    with run.act("Shift+ArrowDown extends to row 2",
                 [focused(role="table cell"), said_cell("2"),
                  row_state_on_bus("table", "1", "selected"),
                  row_state_on_bus("table", "2", "selected")],
                 should="the selection now holds rows 1 and 2; the reader hears row 2",
                 tree=True):
        run.key("Shift+Down")
    with run.act("Ctrl+A selects all",
                 [row_state_on_bus("table", "5", "selected"), said("selected")],
                 should="every row is selected and the reader is told so",
                 tree=True):
        run.key("Ctrl+A")
    with run.act("check the status line",
                 [in_tree(role="label", name_contains="selection: 1000")],
                 should="the example's status line says how many rows are selected",
                 settle=0.5, record=0.5, tree=True):
        pass


def grid_tab_trap(run):
    """Getting out of the table with the keyboard."""
    _tab_to(run, GRID_TABS_BEFORE)
    with run.act("Tab into the table", [focused(role="table")],
                 should="focus reaches the table"):
        run.key("Tab")
    with run.act("Shift+Tab from the table",
                 [focused(role="push button", name="+ Append row")],
                 should="Shift+Tab goes back to the control before the table"):
        run.key("Shift+Tab")
    with run.act("Tab from the first cell",
                 [focused(role="table cell"), said_cell("Avery 0")],
                 should="(for the record) plain Tab walks the cells"):
        run.key("Tab")
    with run.act("Ctrl+Tab leaves the table",
                 [custom("focus leaves the table", _left_table)],
                 should="Ctrl+Tab takes focus to whatever follows the table"):
        run.key("Ctrl+Tab")
    with run.act("Shift+Tab back into the table", [focused()],
                 should="(for the record) where Shift+Tab lands coming back"):
        run.key("Shift+Tab")
    with run.act("Ctrl+Shift+Tab leaves the table backwards",
                 [focused(role="push button", name="+ Append row")],
                 should="Ctrl+Shift+Tab takes focus back to '+ Append row'"):
        run.key("Ctrl+Shift+Tab")


def _left_table(act):
    moves = [e for e in act.events if _is_focus(e)]
    if not moves:
        return False, ["no focus change in the act"]
    node = _focus_node(moves[-1])
    ok = node.get("role") not in ("table", "table cell")
    return ok, [f"last focus [{node.get('role')}] {node.get('name')!r}"]


def grid_headers(run):
    """What a column header publishes, and what can be done to one."""
    run.wait_for(role="table")
    with run.act("read the headers",
                 [header_publishes("ID", "sort"), header_publishes("ID", "action"),
                  header_publishes("Name", "action"),
                  in_tree(role="table", state="multiselectable")],
                 should="the ID header says it is sorted ascending (the example's "
                        "default sort); a header offers a way to sort (an action); a "
                        "multi-row grid says it is multi-selectable",
                 settle=1.0, record=0.5, tree=True):
        pass
    # The sort, the reorder and the resize are pointer gestures on the header
    # (`HeaderCell::build`'s `on_pointer_event`); the header is `focusable(false)`
    # and `keyboard.rs` has no chord for any of them. The screen reader's own
    # activation is the other door: try it.
    outcome: dict = {}

    def header_clicked(act):
        return outcome.get("ok", False), [outcome.get("why", "not tried")]
    with run.act("AT-SPI click on the Name header",
                 [custom("the header can be activated (clicked) through AT-SPI",
                         header_clicked)],
                 should="a screen reader's activation sorts by Name"):
        try:
            run.action("click", role="column header", name="Name")
            outcome.update(ok=True, why="the click was accepted")
        except Exception as exc:  # noqa: BLE001 - the refusal is the evidence
            outcome.update(ok=False, why=f"refused: {exc}")
    with run.act("activate 'Reset sort'",
                 [header_publishes("ID", "sort")],
                 should="the example's 'Reset sort' button resets the sort (to ID "
                        "ascending) and a reader can tell",
                 tree=True):
        run.action("click", role="push button", name="Reset sort")


def grid_filter(run):
    """The Name column's filter, reached through the screen reader's activation
    (the funnel is not a Tab stop)."""
    run.wait_for(role="table")
    filters = run.find_all(role="push button", name="Filter")
    run.note(f"{len(filters)} [push button] 'Filter' nodes on the bus, states: "
             f"{[f.get('states') for f in filters]}")
    with run.act("activate the Name column's Filter",
                 [focused(role="entry"), said("Filter"), said("Name"),
                  in_tree(role="dialog", name_contains="Name"),
                  in_tree(role="entry", name_contains="Name")],
                 should="a popover opens with a filter field, focus moves into it and "
                        "the reader hears which column it filters; the popover and the "
                        "field are named for the column",
                 tree=True):
        run.action("click", role="push button", name="Filter", nth=0)
    focused_node_facts(run, "in the filter popover")
    with run.act("type 'Blake'", [said("Blake")],
                 should="the reader hears what they type"):
        run.type("Blake")
    with run.act("Enter applies the filter",
                 [custom("the table holds only Blake rows", _fewer_rows, needs_tree=True),
                  focus_events(), orca_spoke()],
                 should="the table is filtered to the Blake rows (the field applies on "
                        "Enter, `filter.rs`); the reader is told something happened and, "
                        "ideally, how many rows are left; focus stays somewhere sensible",
                 tree=True, record=3.0):
        run.key("Return")
    focused_node_facts(run, "after Enter in the filter")
    with run.act("Escape closes the filter",
                 [focused(), orca_spoke()],
                 should="the popover closes and focus returns to where it was (the "
                        "table, or the header), and the reader is told where they are"):
        run.key("Escape")


def _fewer_rows(act):
    table = _find(act.tree, "table")
    rows = []
    for n in _walk(table or {}):
        if n.get("role") == "table row" and "selectable" in n.get("states", []):
            label = next((d.get("name") for d in _walk(n) if d.get("role") == "label"
                          and d.get("name", "").startswith("Blake")), None)
            rows.append(label)
    blake = [r for r in rows if r]
    return bool(rows) and len(blake) == len(rows), [
        f"{len(rows)} body rows on the bus, {len(blake)} of them Blake rows: {blake[:6]}"]


def grid_edit(run):
    """F2 on a Name cell swaps in a TextInput."""
    _tab_to(run, GRID_TABS_BEFORE + 1)
    with run.act("ArrowDown onto row 1", [said_cell("1")], should="the cursor is on row 1"):
        run.key("Down")
    with run.act("ArrowRight to the Name cell",
                 [said_cell("Avery 0")], should="the reader hears 'Avery 0'"):
        run.key("Right")
    with run.act("F2 starts editing",
                 [focused(role="entry"), said("Avery 0"), orca_spoke(),
                  in_tree(role="entry", name_contains="Name")],
                 should="the Name cell becomes an edit field holding 'Avery 0', focus "
                        "is in it and the reader hears an editable 'Avery 0' labelled by "
                        "its column, 'Name'",
                 tree=True):
        run.key("F2")
    focused_node_facts(run, "while editing")
    with run.act("type 'X'", [said("X")],
                 should="the reader hears the typed character"):
        run.type("X")
    with run.act("Escape cancels the edit",
                 [focused(role="table cell"), said_cell("Avery 0")],
                 should="the edit is cancelled and the reader is back on the cell "
                        "'Avery 0'"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# tree-table-view
# ---------------------------------------------------------------------------

#: Tab stops before the tree table: Theme.
TREE_TABS_BEFORE = 1


def ttv_tree(run):
    """Walk the tree column: level, expand, collapse."""
    _tab_to(run, TREE_TABS_BEFORE, role="tree table")
    with run.act("Tab into the tree table",
                 [focused(role="tree table"), orca_spoke(), said("4"),
                  container_facts("tree table")],
                 should="focus lands on the tree table and the reader hears its name "
                        "and size",
                 tree=True):
        run.key("Tab")
    with run.act("ArrowDown onto the first row",
                 [focused(role="table cell"), said_cell("Cargo.toml"), said_cell("Name"),
                  said("level 1")],
                 should="the cursor lands on 'Cargo.toml' (a top-level file) and the "
                        "reader hears its name, its column and its level"):
        run.key("Down")
    focused_node_facts(run, "on Cargo.toml")
    with run.act("ArrowDown twice to 'docs'",
                 [said_cell("docs"), said("collapsed")],
                 should="the cursor lands on the folder 'docs' and the reader hears "
                        "that it is collapsed"):
        run.key("Down", "Down", gap=0.8)
    focused_node_facts(run, "on docs")
    with run.act("ArrowRight expands 'docs'",
                 [said("expanded"), rows_on_bus("tree table", "README.md"),
                  replaced(10), focus_events(max_moves=0)],
                 should="docs opens, its three children appear below it, and the "
                        "reader hears 'expanded' (a state change on the cell the reader "
                        "is on, not a new focus)",
                 tree=True):
        run.key("Right")
    with run.act("ArrowDown onto 'README.md'",
                 [said_cell("README.md"), said("level 2")],
                 should="the cursor lands on the first child and the reader hears it "
                        "and that it is one level deeper"):
        run.key("Down")
    focused_node_facts(run, "on README.md")
    with run.act("ArrowRight on a leaf moves to the Size column",
                 [said("4321"), said_cell("Size")],
                 should="a leaf has nothing to expand, so the cursor moves to its size; "
                        "the reader hears 'Size' and the value"):
        run.key("Right")
    with run.act("ArrowLeft back to the Name column",
                 [said_cell("README.md")], should="the reader hears 'README.md'"):
        run.key("Left")
    with run.act("ArrowLeft on a child goes to its parent",
                 [said_cell("docs")],
                 should="the cursor goes up to the parent folder 'docs'"):
        run.key("Left")
    with run.act("ArrowLeft collapses 'docs'",
                 [said("collapsed"), custom("the children left the bus", _children_gone,
                                            needs_tree=True),
                  focus_events(max_moves=0)],
                 should="docs closes, its children go, and the reader hears 'collapsed'",
                 tree=True):
        run.key("Left")


def _children_gone(act):
    table = _find(act.tree, "tree table")
    names = [n.get("name") for n in _walk(table or {}) if n.get("role") == "label"]
    return "README.md" not in names, [f"labels on the bus: {names[:20]}"]


SCENARIOS = [
    Scenario("tables-grid-cells", "data-grid", grid_cells,
             "enter the 1000-row grid and move between cells: names, headers, position"),
    Scenario("tables-grid-pace", "data-grid", grid_pace,
             "ArrowDown at a reading pace and at key-repeat pace: is each row heard?"),
    Scenario("tables-grid-selection", "data-grid", grid_selection,
             "row selection in a MultiRow table: Down, Space, Shift+Down, Ctrl+A"),
    Scenario("tables-grid-tab-trap", "data-grid", grid_tab_trap,
             "Tab, Shift+Tab, Ctrl+Tab and Ctrl+Shift+Tab in and out of the grid"),
    Scenario("tables-grid-headers", "data-grid", grid_headers,
             "what the column headers publish, and a screen reader's click on one"),
    Scenario("tables-grid-filter", "data-grid", grid_filter,
             "the Name column's filter through a screen reader's activation"),
    Scenario("tables-grid-edit", "data-grid", grid_edit,
             "F2 on a Name cell: the edit field a reader meets"),
    Scenario("tables-ttv-tree", "tree-table-view", ttv_tree,
             "the tree column: level, expand and collapse with the arrows"),
]
