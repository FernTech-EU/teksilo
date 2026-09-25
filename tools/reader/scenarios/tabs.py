# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""tab-widget and tab-migration: a tab strip, its tabs, its panel, closing,
reordering and overflow, and moving tabs between two groups, as a screen
reader user meets them.

Where each piece of accessibility is made:

* `TabBar` (`crates/teksilo-widgets/src/tab_widget/bar.rs`, `accessibility`)
  is the `Role::TabList`, orientation and `size_of_set` on the container. The
  non-pinned headers sit inside a `ScrollArea` (`Role::ScrollView`, an AT-SPI
  `panel`); the bar's leading and trailing slots are its children too.
* `TabHeader` (`tab_widget/header.rs`, `accessibility`) is each `Role::Tab`:
  the name is `at_name` (the title), `selected`, `position_in_set`,
  `controls` the panel, a `Click` action and custom actions "Close" and the
  moves. Arrows / Home / End move selection and focus together (automatic
  activation), Enter / Space dive into the panel, Delete closes, Alt+arrows
  and Alt+Home/End move the tab and speak through `ctx.announce`
  (`common/ordered_move.rs`, `move_announcement`).
* `TabPane` (`tab_widget.rs`, `accessibility`) is the `Role::TabPanel` named
  by its tab's title, which the AT-SPI adapter maps to `scroll pane`.
* The close button is hover-revealed (`visible_when`), so it is not in the
  tree at the default density; the scroll arrows and the "Show all tabs"
  dropdown (`bar.rs`, `build_scroll_arrow`, `build_overflow_dropdown`) appear
  only when the headers overflow.

Every step that sets a scene runs inside an act of its own ("setup: ..."),
so the harness moves its cursor in Orca's log past what that step made Orca
say: a key pressed outside any act is received by Orca after the cursor, and
its receipts would be credited to the next act.
"""

from __future__ import annotations

from reader_lib.checks import (Check, _event_line, _focus_node, _is_focus, _walk, announced,
                               custom, event, focused, in_tree, not_announced, not_in_tree,
                               said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

TW = "tab-widget"
TM = "tab-migration"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def nodes(tree, role=None, name=None, name_startswith=None):
    for node in _walk(tree):
        if role is not None and node.get("role") != role:
            continue
        if name is not None and (node.get("name") or "") != name:
            continue
        if name_startswith is not None and \
                not (node.get("name") or "").startswith(name_startswith):
            continue
        yield node


def summary(node) -> str:
    return (f"[{node.get('role')}] {node.get('name')!r} desc={node.get('description')!r} "
            f"states={node.get('states')} attrs={node.get('attributes')} "
            f"actions={[a['name'] for a in node.get('actions', [])]} "
            f"rel={node.get('relations')} children={node.get('child_count')}")


def outline(node, depth=0, out=None, limit=80):
    out = [] if out is None else out
    if len(out) >= limit:
        return out
    out.append("  " * depth + f"[{node.get('role')}] {node.get('name')!r}"
               + (f" desc={node.get('description')!r}" if node.get("description") else "")
               + (f" {sorted(set(node.get('states', [])) - {'enabled', 'sensitive', 'showing', 'visible'})}"
                  if node.get("states") else "")
               + (f" attrs={node.get('attributes')}" if node.get("attributes") else ""))
    for child in node.get("children", []):
        outline(child, depth + 1, out, limit)
    return out


def dump(role: str, describe: str | None = None, name=None) -> Check:
    """List every node of `role` after the act; fails only when there is none."""
    def run(act):
        found = [summary(n) for n in nodes(act.tree, role=role, name=name)]
        return bool(found), found[:40] or [f"no [{role}] in the tree"]
    return custom(describe or f"the tree holds [{role}] nodes (listed)", run, needs_tree=True)


def subtree(role: str, name=None, describe: str | None = None, limit=60) -> Check:
    """The outline under the first [role] name, as evidence."""
    def run(act):
        for n in nodes(act.tree, role=role, name=name):
            return True, outline(n, limit=limit)
        return False, [f"no [{role}] {name!r} in the tree"]
    return custom(describe or f"outline of [{role}] {name!r}", run, needs_tree=True)


def selected_tab(name: str, tree_role: str = "page tab") -> Check:
    """After the act exactly one page tab is selected, and it is `name`."""
    def run(act):
        sel = [n for n in nodes(act.tree, role=tree_role) if "selected" in n.get("states", [])]
        names = [n.get("name") for n in sel]
        return names == [name], [f"selected page tabs: {names}"]
    return custom(f"the one selected page tab is {name!r}", run, needs_tree=True)


def panel_named(name: str) -> Check:
    """The tab panel (a `scroll pane` on AT-SPI) in the tree is named `name`."""
    def run(act):
        panes = [n.get("name") for n in nodes(act.tree, role="scroll pane")]
        return name in panes, [f"scroll panes: {panes}"]
    return custom(f"the tab panel is named {name!r}", run, needs_tree=True)


def spoke() -> Check:
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                             for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca says something", run, needs_orca=True)


def silent() -> Check:
    def run(act):
        heard = utterances(act.orca)
        return not heard, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                           for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca says nothing", run, needs_orca=True)


def tab_order(*names: str, role: str = "page tab") -> Check:
    """The page tabs, in tree order, are `names`."""
    def run(act):
        got = [n.get("name") for n in nodes(act.tree, role=role)]
        return got == list(names), [f"page tabs in tree order: {got}"]
    return custom(f"the page tabs read in the order {list(names)}", run, needs_tree=True)


def announcements() -> Check:
    """Every announcement of the act, listed (passes whatever they are)."""
    def run(act):
        found = [_event_line(act, e) for e in act.events if e["type"] == "object:announcement"]
        return True, found or ["no object:announcement in the act"]
    return custom("the act's announcements (listed)", run)


def orca_log(*needles: str) -> Check:
    """Orca log lines of the act containing any of `needles` (passes whatever)."""
    def run(act):
        found = [f"{line.stamp} {line.text}" for line in act.orca
                 if any(n in line.text for n in needles)]
        return True, found or [f"no Orca line with {needles}"]
    return custom(f"Orca's log lines with {needles} (listed)", run, needs_orca=True)


def focus_target_was_defunct() -> Check:
    """The node focus lands on in this act had been announced defunct earlier
    in the run, and nothing un-did that: libatspi keeps it defunct, and Orca
    ignores events from it."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return True, ["no focus change in the act"]
        target = _focus_node(moves[-1])
        path = target.get("path")
        earlier = [d for d in act.history
                   if d["type"] == "object:state-changed:defunct" and d.get("detail1") == 1
                   and d["source"].get("path") == path and d["seq"] < moves[-1]["seq"]]
        lines = [_event_line(act, moves[-1]), f"path {path}"]
        lines += [f"earlier: {d['wall']} object:state-changed:defunct 1 "
                  f"[{d['source'].get('role')}] {d['source'].get('name')!r}" for d in earlier]
        return not earlier, lines
    return custom("the node focus lands on was not announced defunct earlier in the run",
                  run)


def setup(run, label: str):
    """An act that only sets the scene; its speech is recorded, not judged."""
    return run.act(f"setup: {label}", should="(scene setting)", settle=0.6, record=1.5)


def to_tab(run, arrows: int) -> None:
    """Tab to the selected tab (Welcome at launch), then Right `arrows` times,
    as one setup act."""
    with setup(run, f"Tab to the strip, Right x{arrows}"):
        run.key("Tab")
        for _ in range(arrows):
            run.wait(0.3)
            run.key("Right")


# ---------------------------------------------------------------------------
# tab-widget: the tree at launch
# ---------------------------------------------------------------------------


def launch_tree(run):
    """The strip and its panel as a reader walks them."""
    run.wait_for(role="page tab list")
    with run.act("read the tab strip at launch",
                 [subtree("page tab list", describe="the tab list's outline"),
                  dump("page tab", "every page tab (listed)"),
                  dump("scroll pane", "every tab panel (listed)"),
                  custom("the tab list holds only page tabs (ARIA tablist owns tabs)",
                         lambda act: _only_tabs_in_tablist(act), needs_tree=True),
                  custom("every page tab is a direct child of the tab list",
                         lambda act: _tabs_are_children(act), needs_tree=True),
                  custom("only page tabs carry setsize",
                         lambda act: _setsize_only_on_tabs(act), needs_tree=True),
                  custom("a disabled tab (Locked) is not reported enabled/sensitive",
                         lambda act: _locked_is_disabled(act), needs_tree=True),
                  custom("the pinned tab keeps its declared tooltip "
                         "'Welcome — start here' as its description",
                         lambda act: _welcome_desc(act), needs_tree=True),
                  custom("a closable tab offers a close route on AT-SPI "
                         "(an action beyond 'click', or a close button in the tree)",
                         lambda act: _close_route(act), needs_tree=True),
                  custom("the tab panel is labelled by its tab (a labelled-by relation)",
                         lambda act: _panel_labelled(act), needs_tree=True)],
                 should="the reader finds one tab list holding six tabs, Welcome selected, "
                 "Locked unavailable, and the Welcome panel", tree=True):
        pass


def _tablist(act):
    return next(nodes(act.tree, role="page tab list"), None)


def _only_tabs_in_tablist(act):
    tl = _tablist(act)
    if tl is None:
        return False, ["no page tab list"]
    others = [f"[{c.get('role')}] {c.get('name')!r} attrs={c.get('attributes')}"
              for c in tl.get("children", []) if c.get("role") != "page tab"]
    return not others, ["children of the page tab list that are not page tabs:"] + others


def _tabs_are_children(act):
    tl = _tablist(act)
    if tl is None:
        return False, ["no page tab list"]
    direct = [c.get("name") for c in tl.get("children", []) if c.get("role") == "page tab"]
    every = [n.get("name") for n in nodes(tl, role="page tab")]
    nested = [n for n in every if n not in direct]
    return not nested, [f"direct page-tab children: {direct}",
                        f"page tabs nested under another node: {nested}"]


def _setsize_only_on_tabs(act):
    wrong = [f"[{n.get('role')}] {n.get('name')!r} attrs={n.get('attributes')}"
             for n in _walk(act.tree)
             if (n.get("attributes") or {}).get("setsize") and n.get("role") != "page tab"]
    return not wrong, ["nodes that are not page tabs but carry setsize:"] + wrong


def _locked_is_disabled(act):
    locked = next(nodes(act.tree, role="page tab", name="Locked"), None)
    if locked is None:
        return False, ["no Locked tab"]
    states = locked.get("states", [])
    return "enabled" not in states and "sensitive" not in states, [summary(locked)]


def _welcome_desc(act):
    w = next(nodes(act.tree, role="page tab", name="Welcome"), None)
    if w is None:
        return False, ["no Welcome tab"]
    return (w.get("description") or "") == "Welcome — start here", [summary(w)]


def _close_route(act):
    doc = next(nodes(act.tree, role="page tab", name="Doc 1"), None)
    if doc is None:
        return False, ["no Doc 1 tab"]
    actions = [a["name"] for a in doc.get("actions", [])]
    close_buttons = [summary(n) for n in _walk(act.tree)
                     if "close" in (n.get("name") or "").lower()
                     and n.get("role") == "push button"]
    return (len(actions) > 1 or bool(close_buttons)), \
        [summary(doc), f"close buttons in the tree: {close_buttons or 'none'}"]


def _panel_labelled(act):
    panes = list(nodes(act.tree, role="scroll pane"))
    if not panes:
        return False, ["no scroll pane"]
    rel = panes[0].get("relations") or {}
    return "labelled-by" in rel, [summary(panes[0])]


# ---------------------------------------------------------------------------
# tab-widget: arrows, Home, End through the strip
# ---------------------------------------------------------------------------


def arrows(run):
    run.wait_for(role="page tab", name="Welcome")
    with run.act("Tab to the strip",
                 [focused(role="page tab", name="Welcome"), said("Welcome")],
                 should="focus lands on the selected tab, Welcome, and the reader says it"):
        run.key("Tab")
    with run.act("Right: to Settings",
                 [focused(role="page tab", name="Settings"), said("Settings"),
                  selected_tab("Settings"), panel_named("Settings")],
                 should="Settings becomes the selected, focused tab, its panel shows, "
                 "and the reader says 'Settings page tab'", tree=True):
        run.key("Right")
    with run.act("Right: past the disabled Locked to Doc 1",
                 [focused(role="page tab", name="Doc 1"), said("Doc 1"),
                  selected_tab("Doc 1"), panel_named("Doc 1")],
                 should="arrow navigation skips the disabled tab and lands on Doc 1",
                 tree=True):
        run.key("Right")
    with run.act("End: to the last tab",
                 [focused(role="page tab", name="Doc 3"), said("Doc 3"),
                  selected_tab("Doc 3")],
                 should="focus and selection move to Doc 3, and the reader says it",
                 tree=True):
        run.key("End")
    with run.act("Right on the last tab wraps to Welcome",
                 [focused(role="page tab", name="Welcome"), said("Welcome"),
                  selected_tab("Welcome")],
                 should="the strip wraps to the first tab", tree=True):
        run.key("Right")
    with run.act("Left on the first tab wraps to Doc 3",
                 [focused(role="page tab", name="Doc 3"), said("Doc 3")],
                 should="the strip wraps backwards to the last tab"):
        run.key("Left")
    with run.act("Home: to the first tab",
                 [focused(role="page tab", name="Welcome"), said("Welcome"),
                  selected_tab("Welcome")],
                 should="Home moves focus and selection to Welcome", tree=True):
        run.key("Home")


# ---------------------------------------------------------------------------
# tab-widget: into the panel and back
# ---------------------------------------------------------------------------


def panel(run):
    run.wait_for(role="page tab", name="Welcome")
    to_tab(run, 1)
    with run.act("Enter on Settings dives into its panel",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation")],
                 should="focus moves to the panel's first control and the reader says it",
                 tree=True):
        run.key("Return")
    with run.act("Shift+Tab from the panel's first control",
                 [spoke()],
                 should="(where Shift+Tab goes: the bar's trailing controls sit between "
                 "the tabs and the panel in the Tab order)"):
        run.key("Shift+Tab")
    with setup(run, "back into the panel"):
        run.grab_focus(role="push button", name="Toggle orientation")
    with run.act("Ctrl+Tab inside the panel",
                 [focused(role="page tab", name="Doc 1"), selected_tab("Doc 1")],
                 should="the desktop tab-control chord selects the next tab from inside "
                 "the panel (Windows tab control: Ctrl+Tab / Ctrl+PageDown; GTK notebook: "
                 "Ctrl+PageDown)", tree=True):
        run.key("Ctrl+Tab")
    with setup(run, "back into the panel"):
        run.grab_focus(role="push button", name="Toggle orientation")
    with run.act("Ctrl+PageDown inside the panel",
                 [selected_tab("Doc 1"), spoke()],
                 should="the other desktop chord for the next tab", tree=True):
        run.key("Ctrl+PageDown")


def panel_revisit(run):
    """Leave a tab and come back: does its panel come back readable?"""
    run.wait_for(role="page tab", name="Welcome")
    to_tab(run, 1)
    with run.act("Enter into the Settings panel (first visit)",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation")],
                 should="focus lands on the panel's button and the reader says it"):
        run.key("Return")
    with setup(run, "back to the Settings tab"):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Right to Doc 1 (Settings' panel leaves the tree)",
                 [focused(role="page tab", name="Doc 1"), said("Doc 1"),
                  event("object:state-changed:defunct", role="push button",
                        name_contains="Toggle orientation")],
                 should="Doc 1 is selected and spoken; the Settings panel is removed"):
        run.key("Right")
    with run.act("Left back to Settings (its panel returns)",
                 [focused(role="page tab", name="Settings"), said("Settings"),
                  panel_named("Settings")],
                 should="Settings is selected again and its panel is back", tree=True):
        run.key("Left")
    with run.act("Enter into the returned Settings panel",
                 [focused(role="push button", name="Toggle orientation"),
                  said("Toggle orientation"), focus_target_was_defunct(),
                  orca_log("Ignoring defunct", "Unknown object")],
                 should="focus lands on the panel's button and the reader says it, "
                 "as it did the first time"):
        run.key("Return")
    with run.act("Space on Toggle orientation in the returned panel",
                 [spoke()], should="the button works; the strip turns vertical",
                 tree=True):
        run.key("space")


def doc_revisit(run):
    """The same for a dynamic document tab: Doc 1, away, back, into its panel."""
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    with run.act("Enter into Doc 1's panel (first visit)",
                 [focused(role="push button", name="Make an edit"), said("Make an edit")],
                 should="focus lands on Make an edit and the reader says it"):
        run.key("Return")
    with setup(run, "back to the Doc 1 tab"):
        run.grab_focus(role="page tab", name="Doc 1")
    with run.act("Right to Doc 2",
                 [focused(role="page tab", name="Doc 2"), said("Doc 2")],
                 should="Doc 2 is selected and spoken"):
        run.key("Right")
    with run.act("Left back to Doc 1",
                 [focused(role="page tab", name="Doc 1"), said("Doc 1")],
                 should="Doc 1 is selected again and spoken"):
        run.key("Left")
    with run.act("Enter into Doc 1's returned panel",
                 [focused(role="push button", name="Make an edit"), said("Make an edit"),
                  focus_target_was_defunct(), orca_log("Ignoring defunct", "Unknown object")],
                 should="focus lands on Make an edit and the reader says it, as the first "
                 "time", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# tab-widget: close with Delete and the confirmation
# ---------------------------------------------------------------------------


def close(run):
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    with run.act("Delete on Doc 1 asks to confirm",
                 [said("Close tab?"), subtree("dialog", describe="the dialog's outline"),
                  spoke()],
                 should="a confirmation dialog opens and the reader hears its question",
                 tree=True):
        run.key("Delete")
    with run.act("answer Yes (AT-SPI click)",
                 [focused(role="page tab"), spoke(), not_in_tree("page tab", "Doc 1"),
                  dump("page tab", "the page tabs after the close")],
                 should="the dialog closes, Doc 1 is gone, focus lands on a tab and the "
                 "reader hears where it is", tree=True):
        run.action("click", role="push button", name="Yes")
    with run.act("Right from there", [spoke()],
                 should="arrows still move through the strip", tree=True):
        run.key("Right")


def close_keys(run):
    """The same, answered with real keys only."""
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    with run.act("Delete on Doc 1", [spoke()], should="the confirmation opens",
                 tree=True):
        run.key("Delete")
    with run.act("Shift+Tab to Yes",
                 [focused(role="push button", name="Yes"), said("Yes")],
                 should="focus moves from the default No to Yes"):
        run.key("Shift+Tab")
    with run.act("Space on Yes closes Doc 1",
                 [focused(role="page tab"), spoke(), not_in_tree("page tab", "Doc 1"),
                  selected_tab("Doc 2")],
                 should="Doc 1 closes and focus lands on the tab that took its place, "
                 "which the reader hears",
                 tree=True):
        run.key("space")
    with run.act("Right from there", [spoke()], should="arrows still work in the strip",
                 tree=True):
        run.key("Right")


# ---------------------------------------------------------------------------
# tab-widget: keyboard reorder
# ---------------------------------------------------------------------------


def reorder(run):
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    with run.act("Alt+Right moves Doc 1 after Doc 2",
                 [focused(role="page tab", name="Doc 1"), announced("Doc 1 moved to 5 of 6"),
                  said("Doc 1 moved to 5 of 6"),
                  tab_order("Welcome", "Settings", "Locked", "Doc 2", "Doc 1", "Doc 3")],
                 should="Doc 1 moves one place right, keeps focus, and the reader hears "
                 "'Doc 1 moved to 5 of 6'", tree=True):
        run.key("Alt+Right")
    with run.act("Alt+Right again",
                 [focused(role="page tab", name="Doc 1"), announced("Doc 1 moved to 6 of 6"),
                  said("Doc 1 moved to 6 of 6"),
                  tab_order("Welcome", "Settings", "Locked", "Doc 2", "Doc 3", "Doc 1")],
                 should="Doc 1 moves to the end and the reader hears it", tree=True):
        run.key("Alt+Right")
    with run.act("Right after the moves",
                 [focused(role="page tab", name="Welcome"), spoke()],
                 should="arrow navigation follows the new order: from the last tab, "
                 "Right wraps to Welcome", tree=True):
        run.key("Right")


def reorder_rejected(run):
    """A move the default handler refuses: the dynamic Doc 1 into the static
    region (Alt+Home, then Alt+Left). The first announcement of the session is
    this one, so the announcer's reuse of its node (K2) cannot hide it."""
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    with run.act("Alt+Home on Doc 1",
                 [announcements(), not_announced("moved to 1 of 6"),
                  tab_order("Welcome", "Settings", "Locked", "Doc 1", "Doc 2", "Doc 3"),
                  spoke()],
                 should="either Doc 1 moves to the first place and the reader hears where "
                 "it went, or it cannot move and the reader is not told it did",
                 tree=True):
        run.key("Alt+Home")
    with run.act("Alt+Left on Doc 1",
                 [announcements(), not_announced("moved to 3 of 6"),
                  tab_order("Welcome", "Settings", "Locked", "Doc 1", "Doc 2", "Doc 3"),
                  spoke()],
                 should="Doc 1 cannot pass the static Locked tab; the reader is not told it "
                 "moved", tree=True):
        run.key("Alt+Left")


def reorder_static(run):
    """A static, unpinned tab (Settings) offered the moves."""
    run.wait_for(role="page tab", name="Settings")
    to_tab(run, 1)
    with run.act("Alt+Right on Settings",
                 [announcements(), not_announced("Settings moved"),
                  tab_order("Welcome", "Settings", "Locked", "Doc 1", "Doc 2", "Doc 3"),
                  spoke()],
                 should="a static tab does not move; the reader is not told it did",
                 tree=True):
        run.key("Alt+Right")
    with run.act("the context-menu key on Settings",
                 [dump("menu item", "the menu's items"), spoke()],
                 should="(what the static tab's menu offers)", tree=True):
        run.key("Menu")
    with setup(run, "Escape"):
        run.key("Escape")


def context_menu(run):
    run.wait_for(role="page tab", name="Doc 2")
    to_tab(run, 3)
    with run.act("the context-menu key on Doc 2",
                 [focused(role="menu item"), spoke(), dump("menu item", "the menu's items"),
                  custom("the menu offers closing the tab",
                         lambda act: (any("close" in (n.get("name") or "").lower()
                                          for n in nodes(act.tree, role="menu item")),
                                      [n.get("name") for n in
                                       nodes(act.tree, role="menu item")]),
                         needs_tree=True)],
                 should="the tab's context menu opens with its moves (and a way to close "
                 "the tab), and the reader hears the first item", tree=True):
        run.key("Menu")
    with run.act("Escape", [focused(role="page tab", name="Doc 2"), said("Doc 2")],
                 should="the menu closes and focus returns to Doc 2"):
        run.key("Escape")
    with run.act("Shift+F10 on Doc 2",
                 [focused(role="menu item"), spoke()],
                 should="the other context-menu chord opens it too", tree=True):
        run.key("Shift+F10")
    with run.act("Down in the menu",
                 [focused(role="menu item"), spoke()],
                 should="focus moves to a menu item and the reader hears it", tree=True):
        run.key("Down")
    with run.act("Enter on the focused item",
                 [spoke(), announcements(),
                  focused(role="page tab", name="Doc 2")],
                 should="Doc 2 moves, the menu closes, focus returns to Doc 2 and "
                 "the reader hears where it went", tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# tab-widget: new tabs, overflow, scroll arrows, the dropdown
# ---------------------------------------------------------------------------


def _open_new_tabs(run, count: int):
    run.wait_for(role="push button", name="+ New tab")
    with setup(run, "focus '+ New tab'"):
        run.grab_focus(role="push button", name="+ New tab")


def overflow(run):
    _open_new_tabs(run, 0)
    with run.act("Space on '+ New tab'",
                 [in_tree("page tab", "Doc 4"), spoke(), announcements(),
                  custom("Orca does not present a tab while focus stays on the button",
                         lambda act: (not any("page tab" in u.text for u in utterances(act.orca)),
                                      [u.text for u in utterances(act.orca)]
                                      or ["Orca said nothing"]), needs_orca=True),
                  orca_log("locus of focus", "Setting locusOfFocus")],
                 should="a new tab Doc 4 opens and the reader is told", tree=True):
        run.key("space")
    with run.act("three more new tabs: the strip overflows",
                 [in_tree("page tab", "Doc 7"), subtree("page tab list"),
                  custom("the scroll arrows and the dropdown are named",
                         lambda act: _overflow_named(act), needs_tree=True)],
                 should="the strip overflows; scroll arrows and a 'Show all tabs' button "
                 "appear, each named", tree=True):
        run.key("space")
        run.wait(0.4)
        run.key("space")
        run.wait(0.4)
        run.key("space")


def _overflow_named(act):
    tl = _tablist(act)
    if tl is None:
        return False, ["no page tab list"]
    buttons = [n for n in _walk(tl) if n.get("role") in ("push button", "toggle button")]
    unnamed = [summary(b) for b in buttons if not (b.get("name") or "").strip()]
    names = [b.get("name") for b in buttons]
    return not unnamed and any("all tabs" in (n or "").lower() for n in names), \
        [f"buttons in the tab list: {names}"] + [f"unnamed: {u}" for u in unnamed]


def overflow_walk(run):
    """Overflow the strip, then Tab through it to meet the arrows and dropdown."""
    _open_new_tabs(run, 4)
    with setup(run, "four new tabs"):
        run.key("space", "space", "space", "space", gap=0.5)
    run.snapshot("after four new tabs")
    with setup(run, "focus Welcome"):
        run.grab_focus(role="page tab", name="Welcome")
    for i in range(9):
        with run.act(f"Tab {i + 1} through the overflowing strip", [spoke()],
                     should="each stop is named", settle=0.4, record=1.3):
            run.key("Tab")


def dropdown(run):
    _open_new_tabs(run, 4)
    with setup(run, "four new tabs"):
        run.key("space", "space", "space", "space", gap=0.5)
    trigger = run.wait_for(role="push button", name="Show all tabs", timeout=5)
    run.note(f"the dropdown trigger: {summary(trigger)}")
    with setup(run, "focus 'Show all tabs'"):
        run.grab_focus(role="push button", name="Show all tabs")
    with run.act("Space on 'Show all tabs'",
                 [spoke(), dump("list item", "list items"),
                  custom("the popup's content is a menu, as the trigger's has-popup says",
                         lambda act: (bool(list(nodes(act.tree, role="menu")))
                                      or bool(list(nodes(act.tree, role="menu item"))),
                                      [f"menus: {[n.get('name') for n in nodes(act.tree, role='menu')]}",
                                       f"lists: {[n.get('name') for n in nodes(act.tree, role='list')]}"]),
                         needs_tree=True),
                  subtree("list box", describe="the popup list's outline")],
                 should="the dropdown opens, focus moves into it, and the reader hears "
                 "a list of the tabs, the selected one marked", tree=True):
        run.key("space")
    with run.act("Down in the dropdown", [spoke(), focused(role="list item")],
                 should="the reader hears the next tab in the list", tree=True):
        run.key("Down")
    with run.act("Enter in the dropdown",
                 [spoke(), focused(role="page tab")],
                 should="the chosen tab is selected, the dropdown closes, and focus "
                 "returns to the strip on that tab", tree=True):
        run.key("Return")
    with run.act("Tab in the dropdown", [spoke()],
                 should="(where Tab goes inside the open dropdown)", tree=True):
        run.key("Tab")
    with run.act("Space on what Tab reached",
                 [spoke(), selected_tab("Welcome")],
                 should="(what Space on the reached entry does)", tree=True):
        run.key("space")


def dropdown_at(run):
    """The dropdown driven as a screen reader drives it: AT-SPI click on an entry."""
    _open_new_tabs(run, 4)
    with setup(run, "four new tabs"):
        run.key("space", "space", "space", "space", gap=0.5)
    run.wait_for(role="push button", name="Show all tabs", timeout=5)
    with setup(run, "open 'Show all tabs' (AT-SPI click)"):
        run.action("click", role="push button", name="Show all tabs")
    run.snapshot("the open dropdown")
    with run.act("AT-SPI click on the 'Doc 6' entry",
                 [selected_tab("Doc 6"), spoke(), focused(role="page tab", name="Doc 6")],
                 should="Doc 6 (scrolled out of the strip) is selected, the dropdown closes, "
                 "and the reader hears the new tab", tree=True):
        run.action("click", role="push button", name="Doc 6")
    with run.act("Tab from where focus was left", [spoke()],
                 should="(where the reader is after the dropdown)", tree=True):
        run.key("Tab")


def overflow_arrows(run):
    """Arrow through an overflowing strip: tabs scroll out of view and back."""
    _open_new_tabs(run, 4)
    with setup(run, "four new tabs"):
        run.key("space", "space", "space", "space", gap=0.5)
    with setup(run, "focus Welcome"):
        run.grab_focus(role="page tab", name="Welcome")
    with run.act("Right, Right: visit Settings and Doc 1 while they are in view",
                 [focused(role="page tab", name="Doc 1"), said("Settings"), said("Doc 1")],
                 should="the reader hears Settings, then Doc 1"):
        run.key("Right")
        run.wait(2.0)
        run.key("Right")
    with run.act("End: to Doc 7, scrolled out of view",
                 [focused(role="page tab", name="Doc 7"), said("Doc 7"),
                  subtree("page tab list", describe="the strip after End")],
                 should="the strip scrolls, Doc 7 is focused and spoken", tree=True):
        run.key("End")
    with run.act("Home: back to Welcome",
                 [focused(role="page tab", name="Welcome"), said("Welcome")],
                 should="Welcome is focused and spoken", tree=True):
        run.key("Home")
    with run.act("Right: to Settings (scrolled back into view)",
                 [focused(role="page tab", name="Settings"), said("Settings"),
                  focus_target_was_defunct(), orca_log("Ignoring defunct")],
                 should="Settings is focused and spoken, as before the strip scrolled",
                 tree=True):
        run.key("Right")
    with run.act("Right: to Doc 1",
                 [focused(role="page tab", name="Doc 1"), said("Doc 1"),
                  focus_target_was_defunct(), orca_log("Ignoring defunct")],
                 should="Doc 1 is focused and spoken", tree=True):
        run.key("Right")


# ---------------------------------------------------------------------------
# tab-widget: Orient and Sizing rebuild the widget under focus
# ---------------------------------------------------------------------------


def orient(run):
    run.wait_for(role="push button", name="Orient")
    with setup(run, "focus Orient"):
        run.grab_focus(role="push button", name="Orient")
    with run.act("Space on Orient",
                 [custom("Orca does not present a tab while focus stays on Orient",
                         lambda act: (not any("page tab" in u.text for u in utterances(act.orca)),
                                      [u.text for u in utterances(act.orca)]
                                      or ["Orca said nothing"]), needs_orca=True),
                  orca_log("locus of focus", "Setting locusOfFocus"),
                  custom("the tab list reads vertical",
                         lambda act: (_tablist(act) is not None
                                      and "vertical" in _tablist(act).get("states", []),
                                      [summary(_tablist(act))] if _tablist(act) else []),
                         needs_tree=True)],
                 should="the strip turns vertical; focus stays on Orient", tree=True):
        run.key("space")
    with run.act("Tab after the rebuild", [spoke()],
                 should="Tab moves on from Orient to the next control"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# tab-widget: what a screen reader's own activation does
# ---------------------------------------------------------------------------


def at_activation(run):
    run.wait_for(role="page tab", name="Doc 2")
    with run.act("AT-SPI click on Doc 2",
                 [selected_tab("Doc 2"), panel_named("Doc 2"),
                  event("object:selection-changed", role="page tab list")],
                 should="Doc 2 becomes the selected tab and its panel shows", tree=True):
        run.action("click", role="page tab", name="Doc 2")
    with run.act("AT-SPI grab_focus on Doc 3",
                 [focused(role="page tab", name="Doc 3"), said("Doc 3")],
                 should="focus lands on Doc 3 and the reader says it", tree=True):
        run.grab_focus(role="page tab", name="Doc 3")
    with run.act("AT-SPI grab_focus on the disabled Locked tab",
                 [spoke(), custom("the reader is told Locked is unavailable",
                                  lambda act: (any(w in u.text.lower()
                                                   for u in utterances(act.orca)
                                                   for w in ("grayed", "unavailable",
                                                             "dimmed")),
                                               [u.text for u in utterances(act.orca)]
                                               or ["Orca said nothing"]),
                                  needs_orca=True)],
                 should="a disabled tab is either refused focus or, if it takes "
                 "it, read as unavailable by Orca's own state word ('grayed'), not only "
                 "by the example's tooltip", tree=True):
        run.grab_focus(role="page tab", name="Locked")


# ---------------------------------------------------------------------------
# tab-migration
# ---------------------------------------------------------------------------


def migration_tree(run):
    run.wait_for(role="page tab list")
    with run.act("read the two groups at launch",
                 [dump("page tab list", "every tab list (listed)"),
                  dump("page tab", "every tab (listed)"),
                  dump("scroll pane", "every tab panel (listed)"),
                  custom("each tab list has a name that tells the two groups apart",
                         lambda act: _named_tablists(act), needs_tree=True)],
                 should="two tab lists, 'Group A' and 'Group B', each told apart by name",
                 tree=True):
        pass


def _named_tablists(act):
    lists = list(nodes(act.tree, role="page tab list"))
    names = [n.get("name") for n in lists]
    ok = len(lists) == 2 and all(names) and len(set(names)) == 2
    return ok, [summary(n) for n in lists]


def migration_walk(run):
    run.wait_for(role="page tab", name="Alpha")
    with run.act("Tab to group A's strip",
                 [focused(role="page tab", name="Alpha"), said("Group A")],
                 should="focus lands on Alpha; the reader learns it is in Group A"):
        run.key("Tab")
    with run.act("Tab into Alpha's panel",
                 [focused(role="push button", name="Make an edit"), said("Make an edit")],
                 should="focus reaches the panel's button"):
        run.key("Tab")
    with run.act("Tab to group B's strip",
                 [focused(role="page tab", name="Xeno"), said("Group B")],
                 should="focus lands on Xeno; the reader learns it is in Group B"):
        run.key("Tab")


def migration_move(run):
    """Move a tab to the other group by keyboard: is there any way?"""
    run.wait_for(role="page tab", name="Alpha")
    with setup(run, "Tab to Alpha"):
        run.key("Tab")
    with run.act("the context-menu key on Alpha",
                 [dump("menu item", "the menu's items"),
                  custom("the menu offers moving the tab to the other group",
                         lambda act: _menu_offers_group(act), needs_tree=True)],
                 should="a keyboard route to move Alpha into Group B (drag's alternative)",
                 tree=True):
        run.key("Menu")
    with run.act("Escape", [focused(role="page tab", name="Alpha")],
                 should="the menu closes"):
        run.key("Escape")
    with run.act("Alt+End on Alpha",
                 [announced("Alpha moved to 3 of 3"), said("Alpha moved to 3 of 3"),
                  tab_order("Bravo", "Charlie", "Alpha", "Xeno", "Yotta")],
                 should="Alpha moves to the end of group A and the reader hears it",
                 tree=True):
        run.key("Alt+End")
    with run.act("Alt+Right on Alpha at the end of group A",
                 [spoke(), tab_order("Bravo", "Charlie", "Alpha", "Xeno", "Yotta")],
                 should="at the end of group A a further move would be the one into Group B; "
                 "the reader should be told it cannot move, or it should move",
                 tree=True):
        run.key("Alt+Right")


def _menu_offers_group(act):
    items = [n.get("name") for n in nodes(act.tree, role="menu item")]
    ok = any("group" in (n or "").lower() or "other" in (n or "").lower() for n in items)
    return ok, [f"menu items: {items}"]


SCENARIOS = [
    Scenario("tabs-launch-tree", TW, launch_tree,
             "the tab strip, its tabs and panel at launch, audited"),
    Scenario("tabs-arrows", TW, arrows,
             "Right / End / Home / Left through the strip, selection following focus"),
    Scenario("tabs-panel", TW, panel,
             "Enter into the panel, Shift+Tab back, Ctrl+Tab / Ctrl+PageDown"),
    Scenario("tabs-panel-revisit", TW, panel_revisit,
             "leave Settings and come back, then Enter into its returned panel"),
    Scenario("tabs-doc-revisit", TW, doc_revisit,
             "leave Doc 1 and come back, then Enter into its returned panel"),
    Scenario("tabs-close", TW, close,
             "Delete on a closable tab, the confirmation answered through AT-SPI"),
    Scenario("tabs-close-keys", TW, close_keys,
             "Delete on a closable tab, the confirmation answered with real keys"),
    Scenario("tabs-reorder", TW, reorder,
             "Alt+Right moves a tab; the announcement and focus"),
    Scenario("tabs-reorder-rejected", TW, reorder_rejected,
             "Alt+Home / Alt+Left on a dynamic tab next to the static ones"),
    Scenario("tabs-reorder-static", TW, reorder_static,
             "Alt+Right and the context menu on a static tab"),
    Scenario("tabs-context-menu", TW, context_menu,
             "the tab's context menu from the keyboard"),
    Scenario("tabs-overflow", TW, overflow,
             "new tabs until the strip overflows: scroll arrows and the dropdown"),
    Scenario("tabs-overflow-walk", TW, overflow_walk,
             "Tab through an overflowing strip"),
    Scenario("tabs-dropdown", TW, dropdown,
             "the 'Show all tabs' dropdown from the keyboard"),
    Scenario("tabs-dropdown-at", TW, dropdown_at,
             "the 'Show all tabs' dropdown driven through AT-SPI"),
    Scenario("tabs-overflow-arrows", TW, overflow_arrows,
             "arrows through an overflowing strip: tabs scroll out and back"),
    Scenario("tabs-orient", TW, orient,
             "Orient rebuilds the strip under focus"),
    Scenario("tabs-at-activation", TW, at_activation,
             "a screen reader's own click and focus on tabs"),
    Scenario("tabs-migration-tree", TM, migration_tree,
             "two tab groups at launch: are they told apart"),
    Scenario("tabs-migration-walk", TM, migration_walk,
             "Tab through both groups"),
    Scenario("tabs-migration-move", TM, migration_move,
             "moving a tab to the other group by keyboard"),
]
