# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""data-collections: Repeater, a virtualized ListView with keyboard reorder, an
auto-measured feed and a TreeView, as a screen reader meets them.

What each piece should give a reader, and where it is made:

* `ListView` (`crates/teksilo-widgets/src/list_view/widget_impl.rs`,
  `accessibility`): a `Role::ListBox` holding focus, the cursor's row named as
  its active descendant, so every arrow press is a focus change a reader
  speaks; `size_of_set` = the model length on the container. Between the list
  box and its rows sits the body pane's `Role::Group`
  (`list_view/body_pane.rs`, `accessibility`). Each row is a
  `Role::ListBoxOption` (`list_item_a11y.rs`) with `selected` and
  `position_in_set`; its name is hoisted from the delegate's
  `StandardListItem` (`teksilo-core widget_tree/accessibility_emit_impl.rs`,
  name-from-content), which also carries the subtitle as a description and
  embeds the row's `Checkbox` as a child.
* `TreeView` (`tree_view/widget_impl.rs`, `list_item_a11y.rs`
  `TreeItemWrapper`): `Role::Tree`, rows `Role::TreeItem` with level,
  position, expanded and selected.
* Alt+Arrow reorder and Alt+Right/Left reparent speak through the announcer
  (`common/ordered_move.rs`, `move_announcement` / `reparent_announcement`).

Three things about Orca 46.1 decide what a check can expect:

* A list item's speech on focus is `labelOrName + checkedStateIfCheckable +
  unselectedStateIfSelectable + expandableState + positionInList +
  listBoxItemWidgets` (`formatting.py`). "not selected" needs the item's
  parent to have the Selection interface, and the embedded widgets are read
  only when the item's parent is a list box (`speech_generator.py`
  `_generateUnselectedStateIfSelectable`, `_generateListBoxItemWidgets`).
* A tree item's is `labelOrName + expandableState + positionInList`: no
  level on navigation even for a native tree.
* It reads a position only when `enablePositionSpeaking` is on (off by
  default) or for Where Am I, and then it counts the siblings in the exported
  tree (`script_utilities.py` `getPositionAndSetSize`). The harness cannot
  press Orca's own keys, so `orca_position` computes that answer from the tree
  the same way.
* It speaks a selection change only after it saw Space itself
  (`default.py` `onSelectedChanged`), which it never does here, since the
  harness's keys do not reach Orca's own keyboard. So "selected" is judged on
  the bus, not in speech.
"""

from __future__ import annotations

from reader_lib.checks import (_event_line, _walk, announced, custom, event, focused,
                               no_event, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

PACKAGE = "data-collections"


def said_checked():
    """Orca said "checked" on its own: not "not checked", not "partially
    checked" (`said` matches by containment, and both contain it)."""
    def run(act):
        heard = []
        for u in utterances(act.orca):
            rest = normalized(u.text).replace("notchecked", "").replace("partiallychecked", "")
            if "checked" in rest:
                heard.append(u)
        evidence = [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                    for u in utterances(act.orca)] or ["Orca said nothing in the act"]
        return any(not u.cut for u in heard), evidence
    return custom("Orca says 'checked' (not 'not checked')", run, needs_orca=True)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def nodes(tree, role=None, name=None, name_startswith=None):
    for node in _walk(tree):
        if role is not None and node.get("role") != role:
            continue
        if name is not None and (node.get("name") or "") != name:
            continue
        if name_startswith is not None and not (node.get("name") or "").startswith(name_startswith):
            continue
        yield node


def parents(tree):
    """id(node) -> parent record, over the whole tree."""
    found = {}
    stack = [tree] if tree else []
    while stack:
        node = stack.pop()
        for child in node.get("children", []):
            found[id(child)] = node
            stack.append(child)
    return found


def row_summary(node) -> str:
    return (f"[{node.get('role')}] {node.get('name')!r} desc={node.get('description')!r} "
            f"states={node.get('states')} attrs={node.get('attributes')} "
            f"actions={[a['name'] for a in node.get('actions', [])]}")


def with_children(node) -> list[str]:
    lines = [row_summary(node)]
    for child in node.get("children", []):
        lines.append("  child " + row_summary(child))
        for grandchild in child.get("children", []):
            lines.append("    grandchild " + row_summary(grandchild))
    return lines


def open_tab(run, title: str) -> None:
    """Select a page tab the way a screen reader's activation does."""
    run.action("click", role="page tab", name=title)


def tree_dump(label: str, role: str, limit: int = 40):
    """A check that lists every node of `role` after the act, so the report
    carries the tree's own words. Fails only when there is none."""
    def run(act):
        found = [row_summary(n) for n in nodes(act.tree, role=role)]
        return bool(found), found[:limit] or [f"no [{role}] in the tree"]
    return custom(f"{label}: the [{role}] nodes in the tree", run, needs_tree=True)


def orca_position(role: str, name: str, want_pos: int, want_total: int):
    """What Orca's `getPositionAndSetSize` answers for the row: its index among
    its parent's children, and their count (no NODE_* relations are exported,
    so the functional parent is the plain parent)."""
    def run(act):
        up = parents(act.tree)
        for node in nodes(act.tree, role=role, name=name):
            parent = up.get(id(node))
            if parent is None:
                return False, ["the row has no parent in the tree"]
            siblings = parent.get("children", [])
            count = parent.get("child_count", len(siblings))
            if count > 100:
                pos = node.get("index_in_parent", -1)
                total = count
            else:
                pos = siblings.index(node)
                total = len(siblings)
            ok = (pos + 1, total) == (want_pos, want_total)
            return ok, [f"Orca would count {pos + 1} of {total} "
                        f"(parent [{parent.get('role')}] {parent.get('name')!r} "
                        f"holds {count} children); the row publishes "
                        f"{node.get('attributes')}"]
        return False, [f"no [{role}] {name!r} in the tree"]
    return custom(f"Orca's position for [{role}] {name!r} would be {want_pos} of {want_total}",
                  run, needs_tree=True)


def row_has(role: str, name: str, *, state: str | None = None,
            description_contains: str | None = None, missing_state: str | None = None,
            attribute: str | None = None):
    """The row node itself (the node focus lands on) carries a state, a
    description or an attribute."""
    def run(act):
        for node in nodes(act.tree, role=role, name=name):
            states = node.get("states", [])
            ok = True
            if state is not None and state not in states:
                ok = False
            if missing_state is not None and missing_state in states:
                ok = False
            if description_contains is not None and \
                    description_contains not in (node.get("description") or ""):
                ok = False
            if attribute is not None and attribute not in (node.get("attributes") or {}):
                ok = False
            return ok, with_children(node)
        return False, [f"no [{role}] {name!r} in the tree"]
    want = []
    if state:
        want.append(f"state {state!r}")
    if missing_state:
        want.append(f"no state {missing_state!r}")
    if description_contains:
        want.append(f"a description with {description_contains!r}")
    if attribute:
        want.append(f"an attribute {attribute!r}")
    return custom(f"[{role}] {name!r} carries {', '.join(want)}", run, needs_tree=True)


def parent_of(role: str, name: str, want_role: str):
    """The exported parent of the row has the role Orca looks for."""
    def run(act):
        up = parents(act.tree)
        for node in nodes(act.tree, role=role, name=name):
            parent = up.get(id(node)) or {}
            grand = up.get(id(parent)) or {}
            return parent.get("role") == want_role, [
                f"[{role}] {name!r}'s parent is [{parent.get('role')}] "
                f"{parent.get('name')!r} (its parent [{grand.get('role')}] "
                f"{grand.get('name')!r})"]
        return False, [f"no [{role}] {name!r} in the tree"]
    return custom(f"[{role}] {name!r}'s parent is a [{want_role}]", run, needs_tree=True)


def checkbox_state(name: str, want: str | None, *, absent: str | None = None):
    """The row's embedded check box, as the bus has it now."""
    def run(act):
        for node in nodes(act.tree, role="check box", name=name):
            states = node.get("states", [])
            ok = (want is None or want in states) and (absent is None or absent not in states)
            return ok, [row_summary(node)]
        return False, [f"no [check box] {name!r} in the tree"]
    return custom(f"the [check box] {name!r} in the tree is "
                  f"{want or ''}{' not ' + absent if absent else ''}".strip(),
                  run, needs_tree=True)


def focused_row_on_bus(role: str, want: str):
    """After the act the one node the bus holds as focused is the row `want`."""
    def run(act):
        focused_nodes = [n for n in _walk(act.tree) if "focused" in n.get("states", [])]
        names = [f"[{n.get('role')}] {n.get('name')!r}" for n in focused_nodes]
        ok = any(n.get("role") == role and n.get("name") == want for n in focused_nodes)
        return ok, [f"focused on the bus: {names}"]
    return custom(f"the bus holds [{role}] {want!r} as focused", run, needs_tree=True)


def events_count(label: str, type_prefix: str, at_most: int):
    """How many events of a type the act sent: many defunct/children-changed
    events for one key press are a burst Orca has to work through."""
    def run(act):
        found = [e for e in act.events if e["type"].startswith(type_prefix)]
        return len(found) <= at_most, [f"{len(found)} {type_prefix} events in the act"] + \
            [_event_line(act, e) for e in found[:6]]
    return custom(f"{label}: at most {at_most} {type_prefix} events", run)


def enter_list(run, tab: str, last_button: str) -> None:
    """Open `tab` and put focus on the button just before its list, so the next
    Tab lands on the list.

    Done inside an act of its own: Orca's speech for these steps (the tab, the
    button) would otherwise be credited to the next act, whose first focus
    event the harness matches to Orca's receipt of this one's by type."""
    with run.act(f"set the scene: open {tab}, focus {last_button}",
                 should="scene setting, not judged"):
        open_tab(run, tab)
        run.wait_for(role="push button", name=last_button)
        run.grab_focus(role="push button", name=last_button)


# ---------------------------------------------------------------------------
# ListView: arrows, positions, virtualization
# ---------------------------------------------------------------------------


def listview_arrows(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list",
                 [focused(role="list box"),
                  custom("the list box has a name",
                         lambda a: (bool(next(nodes(a.tree, role="list box"), {}).get("name")),
                                    [row_summary(n) for n in nodes(a.tree, role="list box")]),
                         needs_tree=True),
                  said("list box")],
                 should="focus reaches the list and the reader hears what list it is",
                 tree=True):
        run.key("Tab")
    with run.act("Down: first row",
                 [focused(role="list item", name="Item 1"),
                  said("Item 1"),
                  said("Item #1"),
                  row_has("list item", "Item 1", state="selected",
                          description_contains="Item #1"),
                  parent_of("list item", "Item 1", "list box"),
                  orca_position("list item", "Item 1", 1, 200)],
                 should="the cursor lands on row 1; the reader hears its name and its "
                        "subtitle ('Item #1 · category'), and row 1 of 200",
                 tree=True):
        run.key("Down")
    with run.act("Down: second row (a checkbox row)",
                 [focused(role="list item", name="Item 2"), said("Item 2"),
                  said("not checked"),
                  row_has("list item", "Item 2", state="checkable"),
                  orca_position("list item", "Item 2", 2, 200)],
                 should="the cursor moves to row 2, which carries an unchecked checkbox; "
                        "the reader hears the name and that it is not checked",
                 tree=True):
        run.key("Down")
    with run.act("Shift+Down: extend", [focused(role="list item", name="Item 3"),
                                        said("Item 3"),
                                        row_has("list item", "Item 3", state="selected"),
                                        row_has("list item", "Item 2", state="selected")],
                 should="the selection extends to row 3 and the reader hears it", tree=True):
        run.key("Shift+Down")
    with run.act("End: last row", [focused(role="list item", name="Item 200"),
                                   said("Item 200"),
                                   orca_position("list item", "Item 200", 200, 200),
                                   tree_dump("after End", "list item", 30)],
                 should="the cursor jumps to row 200 and the reader hears it, as 200 of 200",
                 tree=True):
        run.key("End")
    with run.act("Up from the last row", [focused(role="list item", name="Item 199"),
                                          said("Item 199"),
                                          orca_position("list item", "Item 199", 199, 200)],
                 should="the cursor moves to row 199", tree=True):
        run.key("Up")
    with run.act("Home: first row", [focused(role="list item", name="Item 1"),
                                     said("Item 1")],
                 should="the cursor jumps back to row 1", tree=True):
        run.key("Home")
    with run.act("Page Down", [focused(role="list item", name_contains="Item"),
                               tree_dump("after Page Down", "list item", 30)],
                 should="the cursor moves a page down and the reader hears the row", tree=True):
        run.key("Page_Down")


# ---------------------------------------------------------------------------
# ListView: the cursor apart from the selection
# ---------------------------------------------------------------------------


def listview_cursor(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: row 1", [focused(role="list item", name="Item 1")],
                 should="the cursor on row 1, selected"):
        run.key("Down")
    with run.act("Ctrl+Down: cursor only, row 2",
                 [focused(role="list item", name="Item 2"), said("Item 2"),
                  said("not selected")],
                 should="the cursor moves to row 2 (the selection stays on row 1); the "
                        "reader hears row 2 and that it is not selected", tree=True):
        run.key("Ctrl+Down")
    with run.act("nothing pressed, 3 s later",
                 [focused_row_on_bus("list item", "Item 2")],
                 should="the bus has caught up with the cursor", record=3.0, tree=True):
        pass
    with run.act("Ctrl+Down again: row 3",
                 [focused(role="list item", name="Item 3"), said("Item 3")],
                 should="the cursor moves on to row 3; the reader hears it", tree=True):
        run.key("Ctrl+Down")
    with run.act("Ctrl+Space: add the cursor's row to the selection",
                 [event("object:state-changed:selected", role="list item",
                        name_contains="Item 3", detail1=1),
                  row_has("list item", "Item 3", state="selected"),
                  no_event("object:state-changed:focused", role="list item",
                           name_contains="Item 3")],
                 should="row 3 joins the selection; the bus says so from the row focus "
                        "is already on (Orca speaks 'selected' only after a Space it saw "
                        "itself, which the harness cannot give it)", tree=True):
        run.key("Ctrl+space")
    with run.act("Ctrl+End: cursor only, to the last row",
                 [focused(role="list item", name="Item 200"), said("Item 200"),
                  said("not selected"),
                  row_has("list item", "Item 200", missing_state="selected")],
                 should="the cursor jumps to row 200 without selecting it; the reader hears "
                        "the row and that it is not selected", tree=True):
        run.key("Ctrl+End")
    with run.act("Tab out of the list", [custom(
            "focus leaves the list for the next control, not into a row's check box",
            lambda a: (any(e["type"] == "object:state-changed:focused"
                           and e.get("detail1") == 1
                           and e.get("source", {}).get("role") not in ("check box", "list item")
                           for e in a.events)
                       and not any(e["type"] == "object:state-changed:focused"
                                   and e.get("detail1") == 1
                                   and e.get("source", {}).get("role") == "check box"
                                   for e in a.events),
                       [_event_line(a, e) for e in a.events
                        if e["type"] == "object:state-changed:focused"]))],
                 should="Tab leaves the list for the next control", tree=True):
        run.key("Tab")
    with run.act("Shift+Tab back into the list",
                 [focused(role="list item", name="Item 200"), said("Item 200")],
                 should="focus comes back to the list, and the reader hears the row the "
                        "cursor is on"):
        run.key("Shift+Tab")
    with run.act("AT-SPI click on a row", [focused(role="list item", name="Item 199"),
                                           said("Item 199"),
                                           row_has("list item", "Item 199",
                                                   state="selected")],
                 should="a screen reader's activation of a row selects it and moves the "
                        "cursor there", tree=True):
        run.action("click", role="list item", name="Item 199")


# ---------------------------------------------------------------------------
# ListView: a row's checkbox
# ---------------------------------------------------------------------------


def listview_checkbox(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down twice: row 2 (a checkbox row)",
                 [focused(role="list item", name="Item 2"),
                  checkbox_state("Item 2", None, absent="checked"),
                  said("not checked")],
                 should="the cursor on row 2, whose checkbox is unchecked; the reader hears "
                        "that", tree=True):
        run.key("Down", "Down")
    with run.act("Space: check row 2",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Item 2", detail1=1),
                  said_checked(),
                  checkbox_state("Item 2", "checked")],
                 should="the row's checkbox is checked; the bus says so and the reader "
                        "hears it", tree=True):
        run.key("space")
    with run.act("nothing pressed, 3 s later",
                 [checkbox_state("Item 2", "checked"),
                  no_event("object:state-changed:checked")],
                 should="nothing new: the check was already on the bus",
                 record=3.0, tree=True):
        pass
    with run.act("Down: the next update",
                 [focused(role="list item", name="Item 3"),
                  no_event("object:state-changed:checked", role="check box",
                           name_contains="Item 2")],
                 should="the cursor moves on; no stale check-state change for row 2 "
                        "arrives with it", tree=True):
        run.key("Down")
    with run.act("Up and Space again: uncheck row 2",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Item 2", detail1=0),
                  said("not checked")],
                 should="the checkbox is cleared and the reader hears it", tree=True):
        run.key("Up", "space")
    with run.act("AT-SPI click on row 4's check box",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Item 4", detail1=1),
                  checkbox_state("Item 4", "checked")],
                 should="activating the embedded check box through AT-SPI checks it and "
                        "the bus says so", tree=True):
        run.action("click", role="check box", name="Item 4")
    with run.act("Down: the next update after the AT-SPI click",
                 [focused(role="list item", name="Item 3")],
                 should="the cursor moves on; whatever the bus had not been told arrives "
                        "now", tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------
# ListView: keyboard reorder through the announcer
# ---------------------------------------------------------------------------


def listview_reorder(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: first row", [focused(role="list item", name="Item 1"), said("Item 1")],
                 should="the cursor on row 1"):
        run.key("Down")
    with run.act("Alt+Down: move row 1 down",
                 [announced("Moved to 2 of 200"), said("Moved to 2 of 200"),
                  focused(role="list item", name="Item 1"),
                  row_has("list item", "Item 1", state="selected"),
                  events_count("row churn", "object:state-changed:defunct", 20)],
                 should="Item 1 moves to position 2 and the reader hears where it went",
                 tree=True):
        run.key("Alt+Down")
    with run.act("Alt+Down again",
                 [announced("Moved to 3 of 200"), said("Moved to 3 of 200")],
                 should="Item 1 moves to position 3 and the reader hears it", tree=True):
        run.key("Alt+Down")
    with run.act("Alt+Home: move to top",
                 [announced("Moved to 1 of 200"), said("Moved to 1 of 200")],
                 should="Item 1 moves back to the top and the reader hears it", tree=True):
        run.key("Alt+Home")
    with run.act("Alt+Up at the top", [not_said("Moved")],
                 should="nothing moves, and nothing claims a move"):
        run.key("Alt+Up")
    with run.act("Down after the moves", [focused(role="list item", name="Item 2"),
                                          said("Item 2")],
                 should="the cursor moves on normally after the moves", tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------
# ListView: the row's move menu, the keyboard's other route to a reorder
# ---------------------------------------------------------------------------


def listview_menu(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: first row", [focused(role="list item", name="Item 1")],
                 should="the cursor on row 1"):
        run.key("Down")
    with run.act("Shift+F10: the row's menu",
                 [focused(role="menu item"), said("Move Down")],
                 should="the row's context menu opens on its first command and the reader "
                        "hears it", tree=True):
        run.key("Shift+F10")
    with run.act("Down: to the first command",
                 [focused(role="menu item", name="Move Down"), said("Move Down")],
                 should="the first command of the row's menu is focused and heard",
                 tree=True):
        run.key("Down")
    with run.act("Enter on Move Down",
                 [announced("Moved to 2 of 200"), said("Moved to 2 of 200"),
                  focused(role="list item", name="Item 1")],
                 should="Item 1 moves to position 2; the menu closes, focus is back on the "
                        "row, and the reader hears where it went", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# ListView: add / remove rows while the list is not focused
# ---------------------------------------------------------------------------


def listview_add_remove(run):
    enter_list(run, "ListView", "+ Add Item")
    with run.act("Space on + Add Item",
                 [custom("the list's set size follows the model (201)",
                         lambda a: (any(n.get("attributes", {}).get("setsize") == "201"
                                        for n in nodes(a.tree, role="list item")),
                                    [row_summary(n) for n in
                                     list(nodes(a.tree, role="list item"))[:2]]),
                         needs_tree=True)],
                 should="an item is appended; the reader, still on the button, is not told "
                        "(the app has no status message) but the list says 201", tree=True):
        run.key("space")
    with run.act("focus - Remove First", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="- Remove First")
    with run.act("Space on - Remove First",
                 [custom("row 1 is now 'Item 2' at position 1",
                         lambda a: (any(n.get("name") == "Item 2" and
                                        n.get("attributes", {}).get("posinset") == "1"
                                        for n in nodes(a.tree, role="list item")),
                                    [row_summary(n) for n in
                                     list(nodes(a.tree, role="list item"))[:3]]),
                         needs_tree=True)],
                 should="the first item is removed; positions renumber", tree=True):
        run.key("space")
    with run.act("Tab into the list after the edits",
                 [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down after the edits", [focused(role="list item", name="Item 2"),
                                          said("Item 2"),
                                          row_has("list item", "Item 2", attribute="posinset")],
                 should="the first row is Item 2, 1 of 200", tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------
# Auto feed: auto-measured rows
# ---------------------------------------------------------------------------


def feed(run):
    enter_list(run, "Auto Feed", "+ Append Message")
    with run.act("Tab into the feed", [focused(role="list box")],
                 should="focus reaches the feed list", tree=True):
        run.key("Tab")
    with run.act("Down: first message", [focused(role="list item", name_contains="1"),
                                         said("Message 1"),
                                         tree_dump("feed", "list item", 8),
                                         row_has("list item", "#1", state="selected")],
                 should="the reader hears the message the row shows, not only its number",
                 tree=True):
        run.key("Down")
    with run.act("Down: second message", [said("Message 2"),
                                          said("detail line 2 of message 2")],
                 should="the reader hears the second message and its detail lines", tree=True):
        run.key("Down")
    with run.act("End: last message", [focused(role="list item", name_contains="300"),
                                       said("Message 300"),
                                       orca_position("list item", "#300", 300, 300)],
                 should="the cursor jumps to message 300 and the reader hears it", tree=True):
        run.key("End")
    with run.act("focus + Append Message", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="+ Append Message")
    with run.act("Append a message while the feed is not focused",
                 [custom("the feed's set size follows the model (301)",
                         lambda a: (any(n.get("attributes", {}).get("setsize") == "301"
                                        for n in nodes(a.tree, role="list item")),
                                    [row_summary(n) for n in
                                     list(nodes(a.tree, role="list item"))[-2:]]),
                         needs_tree=True)],
                 should="one message is appended", tree=True):
        run.key("space")


# ---------------------------------------------------------------------------
# TreeView: level, expanded state, selection
# ---------------------------------------------------------------------------


def tree_nav(run):
    enter_list(run, "TreeView", "- Remove Last Root")
    with run.act("Tab into the tree", [focused(role="tree"), said("tree")],
                 should="focus reaches the tree and the reader hears what it is", tree=True):
        run.key("Tab")
    with run.act("Down: Documents", [focused(role="tree item", name="Documents"),
                                     said("Documents"), said("collapsed"),
                                     said("folder"),
                                     row_has("tree item", "Documents", state="expandable"),
                                     row_has("tree item", "Documents", attribute="level"),
                                     tree_dump("tree", "tree item")],
                 should="the cursor on the first root; the reader hears its name, "
                        "collapsed, and the subtitle 'folder · depth 0'; the row "
                        "publishes its level", tree=True):
        run.key("Down")
    with run.act("Right: expand Documents", [said("expanded"),
                                             row_has("tree item", "Documents",
                                                     state="expanded"),
                                             tree_dump("tree after expand", "tree item")],
                 should="Documents opens and the reader hears 'expanded'", tree=True):
        run.key("Right")
    with run.act("Right again: into the first child",
                 [focused(role="tree item", name="Projects"), said("Projects"),
                  row_has("tree item", "Projects", attribute="level")],
                 should="the cursor moves to Projects, one level down; the row says so",
                 tree=True):
        run.key("Right")
    with run.act("Right: expand Projects", [said("expanded")],
                 should="Projects opens and the reader hears 'expanded'", tree=True):
        run.key("Right")
    with run.act("Down: Teksilo (a leaf, level 3)",
                 [focused(role="tree item", name="Teksilo"), said("Teksilo"),
                  said("not checked"),
                  orca_position("tree item", "Teksilo", 1, 2)],
                 should="the cursor on the leaf Teksilo, 1 of 2 under Projects; the reader "
                        "hears whether its checkbox is checked",
                 tree=True):
        run.key("Down")
    with run.act("Space: check the leaf",
                 [event("object:state-changed:checked", role="check box",
                        name_contains="Teksilo", detail1=1),
                  said_checked(),
                  checkbox_state("Teksilo", "checked")],
                 should="Space checks Teksilo and the reader hears it", tree=True):
        run.key("space")
    with run.act("Left: up to Projects", [focused(role="tree item", name="Projects"),
                                          said("Projects"), said("partially checked"),
                                          checkbox_state("Projects", "indeterminate")],
                 should="the cursor goes back to the parent Projects, whose checkbox is "
                        "now mixed; the reader hears it",
                 tree=True):
        run.key("Left")
    with run.act("Left: collapse Projects", [said("collapsed"),
                                             focused(role="tree item", name="Projects"),
                                             row_has("tree item", "Projects",
                                                     missing_state="expanded")],
                 should="Projects closes and the reader hears 'collapsed'", tree=True):
        run.key("Left")
    with run.act("tree after the collapse",
                 [custom("Documents offers an expand/collapse action on AT-SPI",
                         lambda a: (any(any(x["name"].lower() in ("expand", "collapse",
                                                                  "expand or collapse",
                                                                  "toggle")
                                            for x in n.get("actions", []))
                                        for n in nodes(a.tree, role="tree item",
                                                       name="Documents")),
                                    [row_summary(n) for n in nodes(a.tree, role="tree item",
                                                                   name="Documents")]),
                         needs_tree=True)],
                 should="a reader can open and close a branch with its own command",
                 tree=True):
        pass


# ---------------------------------------------------------------------------
# TreeView: keyboard reorder and reparent through the announcer
# ---------------------------------------------------------------------------


def tree_moves(run):
    enter_list(run, "TreeView", "- Remove Last Root")
    with run.act("Tab into the tree", [focused(role="tree")], should="focus on the tree"):
        run.key("Tab")
    with run.act("Down: Documents", [focused(role="tree item", name="Documents")],
                 should="the cursor on Documents"):
        run.key("Down")
    with run.act("Alt+Down: move Documents after Pictures",
                 [announced("Moved to 2 of 3"), said("Moved to 2 of 3"),
                  focused(role="tree item", name="Documents")],
                 should="Documents moves to second place among the roots and the reader "
                        "hears it", tree=True):
        run.key("Alt+Down")
    with run.act("Alt+Right: indent Documents into Pictures",
                 [announced("level 2"), said("level 2")],
                 should="Documents becomes a child of Pictures and the reader hears the "
                        "new level", tree=True):
        run.key("Alt+Right")
    with run.act("Alt+Left: outdent it back",
                 [announced("level 1"), said("level 1")],
                 should="Documents comes back to the root level and the reader hears it",
                 tree=True):
        run.key("Alt+Left")
    with run.act("Down after the moves", [focused(role="tree item"),
                                          said("Downloads")],
                 should="the cursor moves on to the next row normally", tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------
# Repeater
# ---------------------------------------------------------------------------


def repeater(run):
    run.wait_for(role="push button", name="+ Add Tag")
    with run.act("tree of the Repeater tab",
                 [custom("the tags read as a list", lambda a: (
                     any(True for _ in nodes(a.tree, role="list")),
                     [row_summary(n) for n in nodes(a.tree, role="panel")][:8]),
                     needs_tree=True)],
                 should="the tags are a list a reader can count", tree=True):
        pass
    with run.act("focus + Add Tag", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="+ Add Tag")
    with run.act("Space on + Add Tag",
                 [custom("the new tag reached the bus",
                         lambda a: (any(True for _ in nodes(a.tree, role="label",
                                                           name="Tag 5")),
                                    ["labels: " + ", ".join(repr(n.get("name")) for n in
                                                            nodes(a.tree, role="label"))]),
                         needs_tree=True),
                  said("Tag 5")],
                 should="a tag is added; a reader on the button is told what was added",
                 tree=True):
        run.key("space")
    with run.act("focus - Remove Last", should="scene setting, not judged"):
        run.grab_focus(role="push button", name="- Remove Last")
    with run.act("Space on - Remove Last", [said("Tag 5")],
                 should="the last tag is removed; the reader is told", tree=True):
        run.key("space")


SCENARIOS = [
    Scenario("collections-listview-arrows", PACKAGE, listview_arrows,
             "ListView: Tab in, arrows, Shift+Down, End/Home/Page Down"),
    Scenario("collections-listview-cursor", PACKAGE, listview_cursor,
             "ListView: Ctrl+Down / Ctrl+End cursor-only moves, Ctrl+Space, Tab out and "
             "back, AT-SPI click"),
    Scenario("collections-listview-checkbox", PACKAGE, listview_checkbox,
             "ListView: Space on a checkbox row, and the embedded check box's own action"),
    Scenario("collections-listview-reorder", PACKAGE, listview_reorder,
             "ListView: Alt+Down / Alt+Home keyboard reorder, said through the announcer"),
    Scenario("collections-listview-menu", PACKAGE, listview_menu,
             "ListView: Shift+F10 on a row, then its Move Down command"),
    Scenario("collections-listview-edit", PACKAGE, listview_add_remove,
             "ListView: + Add Item / - Remove First, then the list"),
    Scenario("collections-feed", PACKAGE, feed,
             "Auto Feed: arrows through auto-measured message rows"),
    Scenario("collections-tree-nav", PACKAGE, tree_nav,
             "TreeView: expand, descend, level, checkbox, collapse"),
    Scenario("collections-tree-moves", PACKAGE, tree_moves,
             "TreeView: Alt+Down reorder and Alt+Right/Left reparent through the announcer"),
    Scenario("collections-repeater", PACKAGE, repeater,
             "Repeater: the tag list and its Add / Remove buttons"),
]
