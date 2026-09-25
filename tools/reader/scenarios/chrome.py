# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Window chrome and its helpers, as a screen reader user meets them:
tooltips (`tooltips-showcase`), the ToolBox (`tool-box`), the Splitter
(`splitter`), the ShortcutSettings rebind panel (`shortcuts-demo`), the custom
title bar (`title-bar-demo`), the collapsible menu bar (`collapsible-menu-bar`)
and the model-driven menu bar (`native-menu`, which on Linux is the in-window
bar).

Where each piece of accessibility comes from:

* Tooltips (`crates/teksilo-widgets/src/tooltip/`). A plain tip's text is
  copied onto the described control as its `description` by the AccessKit
  pass, and the tip is never shown on focus. A rich or composite tip with
  dwell promotion is armed by focus (`WidgetTree::tooltip_focus_enter`,
  `crates/teksilo-core/src/widget_tree/overlay_impl.rs`) and shows after the
  tooltip delay; it is `Role::Tooltip`, then `Role::Dialog` once sticky
  (`rich.rs` / `composite.rs`, `accessibility`). A rich tip's name is its
  `TooltipContent` text (`rich.rs`, `accessibility`), and a composite tip's is
  the generic "Tooltip" unless the app gives it a label (`composite.rs`).
* `ToolBoxHeader` (`tool_box.rs`) is a `Role::Button` with `expanded` and a
  `controls` relation to its `Role::Region` panel, which is hidden while
  collapsed.
* A `Splitter` divider (`splitter/handle.rs`) is `Role::Splitter`, named
  "Splitter divider", with a percent value, orientation and `expanded` for a
  collapsible neighbour. AT-SPI maps `Splitter` to `separator`.
* `ShortcutSettings` (`shortcut_settings.rs`): per shortcut a name label, the
  bound chord as a label, and three buttons ("Rebind", "Rebind 2nd",
  "Reset"); the capture hint is a `Role::Status` live node while capturing.
* `TitleBar` (`title_bar.rs`) is a `Role::Banner`; its window controls
  (`title_bar/controls.rs`) are unfocusable `Role::Button`s.
* `MenuBar` (`menu_bar.rs`, `menu_bar/trigger.rs`): triggers are
  `Role::MenuItem` with `has_popup` and `expanded`; a collapsible bar is a
  hamburger `IconButton` that reveals the bar in an overlay.

Every scenario is named `chrome-*`.
"""

from __future__ import annotations

from reader_lib.checks import (Check, _walk, custom, event, focused, in_tree, no_event,
                               not_in_tree, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario, tab_walk


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def said_any(*texts: str) -> Check:
    """Orca said at least one of `texts`."""
    def run(act):
        heard = utterances(act.orca)
        hit = [u for u in heard for t in texts if normalized(t) in normalized(u.text)]
        return bool(hit), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                           for u in heard] or ["Orca said nothing in this act"]
    return custom(f"Orca says one of {texts!r}", run, needs_orca=True)


def spoke_anything() -> Check:
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                             for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca says something", run, needs_orca=True)


def _nodes(act, role=None, name=None, name_contains=None):
    return [n for n in _walk(act.tree)
            if (role is None or n.get("role") == role)
            and (name is None or (n.get("name") or "") == name)
            and (name_contains is None
                 or normalized(name_contains) in normalized(n.get("name") or ""))]


def node_facts(describe: str, pred, *, role=None, name=None, name_contains=None) -> Check:
    """After the act, the first matching node satisfies `pred(node)`."""
    def run(act):
        nodes = _nodes(act, role, name, name_contains)
        if not nodes:
            return False, [f"no [{role}] {name or name_contains!r} in the tree after the act"]
        n = nodes[0]
        return bool(pred(n)), [f"[{n.get('role')}] {n.get('name')!r} "
                               f"desc={n.get('description')!r} states={n.get('states')} "
                               f"attributes={n.get('attributes', {})} "
                               f"relations={n.get('relations', {})} value={n.get('value')}"]
    return custom(describe, run, needs_tree=True)


def description_is(role, name, want) -> Check:
    return node_facts(f"[{role}] {name!r} has the description {want!r}",
                      lambda n: normalized(n.get("description") or "") == normalized(want),
                      role=role, name=name)


def has_state(role, name, state) -> Check:
    return node_facts(f"[{role}] {name!r} is {state}",
                      lambda n: state in n.get("states", []), role=role, name=name)


def lacks_state(role, name, state) -> Check:
    return node_facts(f"[{role}] {name!r} is not {state}",
                      lambda n: state not in n.get("states", []), role=role, name=name)


def any_event(*types: str) -> Check:
    def run(act):
        found = [e for e in act.events if any(e["type"].startswith(t) for t in types)]
        return bool(found), [f"{e['type']} [{e['source'].get('role')}] "
                             f"{e['source'].get('name')!r} {e.get('detail1')}"
                             for e in found[:12]] or [f"no {types} event in the act"]
    return custom(f"an event of {types}", run)


def printed(text: str, act_marker: list) -> Check:
    """The example printed `text` to its stdout (app.log) during the act.

    `act_marker` is a one-element list the body fills with the log's length
    when the act starts."""
    def run(act):
        log = act_marker[1].read_text(errors="replace") if act_marker[1].exists() else ""
        tail = log[act_marker[0]:]
        return text in tail, [f"app.log after the act started: {tail.strip()[-300:]!r}"]
    return custom(f"the example printed {text!r}", run)


def log_mark(run) -> list:
    try:
        size = len(run.app_log.read_text(errors="replace"))
    except OSError:
        size = 0
    return [size, run.app_log]


# ---------------------------------------------------------------------------
# Tab walks (limited)
# ---------------------------------------------------------------------------


def walk(stops: int):
    def body(run):
        tab_walk(run, stops=stops)
    return body


# ---------------------------------------------------------------------------
# tooltips-showcase
# ---------------------------------------------------------------------------

TIPS = "tooltips-showcase"
LEVEL1 = "Hover or hold — level 1"
LEVEL3 = "Hover or hold — level 3"


def tips_plain(run):
    run.wait_for(role="push button", name="Save")
    with run.act("Tab to the Save button",
                 [focused(role="push button", name="Save"), said("Save"),
                  said("Save the current document")],
                 should="the reader hears the button, then its tooltip as its description",
                 record=2.5):
        run.key("Tab")
    with run.act("stay on Save past the tooltip delay",
                 [not_in_tree(role="tool tip")],
                 should="a plain tip is not shown on focus; its text was already the "
                        "description, so nothing more is said", record=1.5, tree=True):
        run.wait(1.0)
    with run.act("Tab to Open", [focused(role="push button", name="Open"),
                                 said("Open a file")],
                 should="the next button and its tooltip text"):
        run.key("Tab")


def tips_rich(run):
    run.wait_for(role="push button", name=LEVEL1)
    before = run.find(role="push button", name=LEVEL1) or {}
    run.note(f"before any focus, [{LEVEL1}] carries description "
             f"{before.get('description')!r}")
    run.grab_focus(role="push button", name="Close")
    with run.act("Tab to the rich-tooltip button 'level 1' and wait",
                 [focused(role="push button", name=LEVEL1), said(LEVEL1),
                  said("Level 1 of the cascade"),
                  not_said("(:tip-b)"), not_said("[next link]")],
                 should="the reader hears the button and, as its description or as the "
                        "tooltip that focus summons, the tip's text as written for a reader, "
                        "without its markup", record=4.0, tree=True):
        run.key("Tab")
    with run.act("Tab again, into the tip that focus promoted",
                 [spoke_anything()],
                 should="the doc says a promoted tip is Tab-reachable: focus enters the "
                        "tooltip panel (its link, its More disclosure) and the reader "
                        "hears where it is", record=3.0, tree=True):
        run.key("Tab")
    with run.act("Escape",
                 should="the tip closes and focus is back on a control the reader "
                        "hears", record=2.5, tree=True):
        run.key("Escape")


def tips_rich_after(run):
    """Whether the anchor's description exists before, and after, the tip was
    built once."""
    run.wait_for(role="push button", name=LEVEL3)
    run.grab_focus(role="push button", name="Plain among rich")
    with run.act("launch state of the level-3 anchor",
                 [description_is("push button", LEVEL3,
                                 "Level 3 — end of the cascade. Press Esc or click "
                                 "outside to dismiss.")],
                 should="the anchor carries its tip's text as its description from the "
                        "start, as a plain tip's anchor does", record=0.5, tree=True):
        pass
    with run.act("Shift+Tab to level 3 and wait for the tip",
                 [focused(role="push button", name=LEVEL3), said("end of the cascade")],
                 should="the reader hears the button and its tip", record=4.0, tree=True):
        run.key("Shift+Tab")
    with run.act("Escape, then Shift+Tab away and Tab back",
                 [focused(role="push button", name=LEVEL3), said("end of the cascade")],
                 should="the second visit says the same", record=3.0, tree=True):
        run.key("Escape")
        run.wait(0.8)
        run.key("Shift+Tab")
        run.wait(1.0)
        run.key("Tab")


def tips_composite(run):
    run.wait_for(role="push button", name="Province info")
    before = run.find(role="push button", name="Province info") or {}
    run.note(f"[Province info] description at launch: {before.get('description')!r}")
    run.grab_focus(role="push button", name="Plain among rich")
    with run.act("Tab to 'Province info' and wait for its composite tooltip",
                 [focused(role="push button", name="Province info"), said("Province info"),
                  not_said("Tooltip")],
                 should="the reader hears the button; its description is not the generic "
                        "word 'Tooltip'", record=4.5, tree=True):
        run.key("Tab")
    with run.act("Tab into the composite tooltip",
                 [said_any("Iberia", "Food: 42", "Province overview")],
                 should="the promoted surface is reachable and its content (Iberia, the "
                        "stats) is read", record=3.0, tree=True):
        run.key("Tab")
    with run.act("Escape", should="the surface closes", record=2.0, tree=True):
        run.key("Escape")


def tips_interactive(run):
    run.wait_for(role="push button", name="With internal Button")
    with run.act("focus 'With internal Button' through AT-SPI and wait for its tip to "
                 "promote",
                 [in_tree(role="dialog", name="Tooltip")],
                 should="the composite tip shows after the delay and promotes to a dialog",
                 record=4.5, tree=True):
        run.grab_focus(role="push button", name="With internal Button")
    with run.act("Tab, as the tip says, into the surface",
                 [spoke_anything()],
                 should="focus enters the surface and the reader hears where it is",
                 record=2.5, tree=True):
        run.key("Tab")
    with run.act("Tab again, to the inner button",
                 [focused(role="push button", name="Open ledger"), said("Open ledger")],
                 should="the inner 'Open ledger' button takes focus and is read",
                 record=2.5, tree=True):
        run.key("Tab")
    mark = log_mark(run)
    with run.act("Space on it",
                 [printed("Open ledger pressed", mark)],
                 should="the inner button's action runs", record=2.0):
        run.key("space")


def tips_snapback(run):
    """Tab away from an anchor whose tip focus summoned, three times."""
    cases = [(LEVEL1, "Hover or hold — level 2"), ("Province info", "Tabbed details"),
             (LEVEL3, "Plain among rich")]
    run.wait_for(role="push button", name=LEVEL1)

    def stays(anchor, nxt):
        def check(act):
            moves = [e for e in act.events
                     if e["type"] == "object:state-changed:focused" and e.get("detail1") == 1]
            landed = next((i for i, e in enumerate(moves)
                           if e["source"].get("name") == nxt), None)
            lines = [f"{act.rel_ms(e):+.1f} ms focused 1 [{e['source'].get('role')}] "
                     f"{e['source'].get('name')!r}" for e in moves]
            if landed is None:
                return False, [f"focus never reached {nxt!r}"] + lines
            back = [e for e in moves[landed + 1:] if e["source"].get("name") == anchor]
            return not back, lines
        return custom(f"after landing on {nxt!r}, focus does not go back to {anchor!r}", check)

    for anchor, nxt in cases:
        with run.act(f"Escape, wait 3 s, focus {anchor!r} (AT-SPI), wait 1.2 s for its tip "
                     "(shown, not yet promoted), then Tab",
                     [stays(anchor, nxt), focused(role="push button", name=nxt)],
                     should=f"focus moves to {nxt!r} and stays there; the reader hears "
                            f"{nxt!r} once", record=2.5):
            run.key("Escape")
            run.wait(3.0)
            run.grab_focus(role="push button", name=anchor)
            run.wait(1.2)
            run.key("Tab")


def tips_reshow(run):
    """A composite tip shown a second time, entered by Tab."""
    run.wait_for(role="push button", name="Province info")
    with run.act("focus 'Province info' through AT-SPI and wait for promotion",
                 [in_tree(role="dialog", name="Tooltip")],
                 should="the tip shows and promotes", record=4.0, tree=True):
        run.grab_focus(role="push button", name="Province info")
    with run.act("Tab into it (first showing)", [said("Iberia")],
                 should="the reader hears the surface and its content", record=2.5):
        run.key("Tab")
    with run.act("Escape", should="the tip closes, focus back on the button", record=2.5):
        run.key("Escape")
    with run.act("Shift+Tab away, wait, Tab back and wait for promotion",
                 [focused(role="push button", name="Province info")],
                 should="the tip shows again", record=4.0, tree=True):
        run.key("Shift+Tab")
        run.wait(3.0)
        run.key("Tab")
    with run.act("Tab into it (second showing)", [said("Iberia")],
                 should="the reader hears the surface again, as the first time",
                 record=2.5, tree=True):
        run.key("Tab")


def _context_panel_reachable(act):
    for node in _walk(act.tree):
        kids = node.get("children", [])
        if any((k.get("name") or "") == "Right-click here for a menu" for k in kids):
            reach = ["focusable" in n.get("states", []) for n in _walk(node)]
            return any(reach), [f"[{node.get('role')}] {node.get('name')!r} "
                                f"states={node.get('states')}; focusable nodes inside: "
                                f"{sum(reach)}"]
    return False, ["the 'Right-click here for a menu' label is not in the tree"]


def tips_menu(run):
    """The right-click menu of rich-tooltip items has no keyboard route."""
    run.wait_for(role="label", name="Right-click here for a menu")
    with run.act("launch state of the right-click panel",
                 [custom("the 'Right-click here' panel (or a node in it) is focusable, so a "
                         "keyboard user can reach its context menu",
                         _context_panel_reachable, needs_tree=True)],
                 should="the context-menu target is reachable by keyboard", record=0.3,
                 tree=True):
        pass
    run.grab_focus(role="push button", name="With internal Button")
    with run.act("Tab from the last composite button",
                 [focused(role="page tab", name="Food")],
                 should="Tab order goes from the composite column to the fourth column; "
                        "if the panel is not a stop, its menu has no keyboard route",
                 record=2.0):
        run.key("Escape")
        run.wait(0.3)
        run.key("Tab")


# ---------------------------------------------------------------------------
# tool-box
# ---------------------------------------------------------------------------

TB = "tool-box"


def toolbox(run):
    run.wait_for(role="push button", name="Outline")
    run.grab_focus(role="combo box", name="Theme")
    with run.act("Tab to the first section header",
                 [focused(role="push button", name="Outline"), said("Outline"),
                  said_any("expanded", "collapse")],
                 should="the reader hears 'Outline', a button, and that it is expanded",
                 tree=True):
        run.key("Tab")
    with run.act("Down to Properties",
                 [focused(role="push button", name="Properties"), said("Properties"),
                  said_any("collapsed", "expand")],
                 should="focus moves to the next header and the reader hears it is "
                        "collapsed", tree=True):
        run.key("Down")
    with run.act("Space to open Properties",
                 [said_any("expanded"), in_tree(role="landmark", name="Properties"),
                  not_in_tree(role="landmark", name="Outline")],
                 should="the section opens, Outline closes, and the reader hears the "
                        "new state", tree=True, record=3.0):
        run.key("space")
    with run.act("End",
                 [focused(role="push button", name="References")],
                 should="End goes to the last enabled header, skipping the disabled "
                        "Build tasks", tree=True):
        run.key("End")
    with run.act("launch state of Build tasks",
                 [node_facts("the disabled 'Build tasks' header is exposed as disabled "
                             "(not enabled/sensitive) and not focusable",
                             lambda n: "enabled" not in n.get("states", [])
                             and "focusable" not in n.get("states", []),
                             role="push button", name="Build tasks")],
                 should="a reader browsing the palette hears Build tasks is unavailable",
                 record=0.3, tree=True):
        pass
    with run.act("activate References through AT-SPI (a screen reader's click)",
                 [in_tree(role="landmark", name="References")],
                 should="the reader's own activation opens the section", tree=True):
        run.action("click", role="push button", name="References")
    with run.act("Home, Space (re-open Outline, which was open at launch)",
                 [in_tree(role="landmark", name="Outline"), region_live("Outline")],
                 should="the section's content comes back as live objects a reader can read",
                 tree=True, record=3.0):
        run.key("Home")
        run.wait(0.4)
        run.key("space")


def region_live(name: str) -> Check:
    """The [landmark] `name` and everything under it are not defunct."""
    def run(act):
        for node in _walk(act.tree):
            if node.get("role") == "landmark" and node.get("name") == name:
                dead = [f"[{n.get('role')}] {n.get('name')!r} states={n.get('states')}"
                        for n in _walk(node) if "defunct" in n.get("states", [])]
                return not dead, dead or [f"[landmark] {name!r} and its "
                                          f"{sum(1 for _ in _walk(node))} nodes are live"]
        return False, [f"no [landmark] {name!r} in the tree"]
    return custom(f"the [landmark] {name!r} subtree is not defunct", run, needs_tree=True)


# ---------------------------------------------------------------------------
# splitter
# ---------------------------------------------------------------------------

SP = "splitter"


def splitter(run):
    run.wait_for(role="separator", name="Splitter divider")
    run.grab_focus(role="combo box", name="Theme")
    with run.act("Tab to the first divider",
                 [focused(role="separator", name="Splitter divider"),
                  said("Splitter divider"), said_any("28", "27")],
                 should="the reader hears which divider this is, its role, and its "
                        "position", tree=True):
        run.key("Tab")
    with run.act("Right arrow (grow the sidebar)",
                 [event("object:property-change:accessible-value"), said_any("30", "31")],
                 should="the divider moves and the reader hears the new position",
                 tree=True):
        run.key("Right")
    with run.act("Right arrow again",
                 [said_any("33", "34", "32")],
                 should="another step, heard", tree=True):
        run.key("Right")
    with run.act("Enter (collapse the sidebar)",
                 [said_any("collapsed"), not_in_tree(role="panel", name="Sidebar")],
                 should="the sidebar folds and the reader hears it collapsed", tree=True,
                 record=3.0):
        run.key("Return")
    with run.act("Enter again (restore)",
                 [said_any("expanded"), in_tree(role="panel", name="Sidebar")],
                 should="the sidebar comes back and the reader hears it", tree=True,
                 record=3.0):
        run.key("Return")
    with run.act("Tab to the second divider",
                 [focused(role="separator", name="Splitter divider"),
                  custom("the second divider is told apart from the first by what a "
                         "reader hears (name or description)",
                         lambda act: (False, ["both dividers are named "
                                              "'Splitter divider'; see the tree"])
                         if len({n.get("name") for n in _nodes(act, "separator")}) < 2
                         else (True, []), needs_tree=True)],
                 should="the reader can tell which panes this divider separates", tree=True):
        run.key("Tab")


def splitter_buttons(run):
    run.wait_for(role="push button", name="Export layout")
    with run.act("activate Export layout through AT-SPI",
                 [said("Exported")],
                 should="the status line's new text is announced, since it is the only "
                        "feedback", tree=True, record=3.0):
        run.action("click", role="push button", name="Export layout")
    with run.act("activate Add / Remove Inspector through AT-SPI",
                 [not_in_tree(role="panel", name="Inspector")],
                 should="the inspector pane and its divider leave the tree", tree=True,
                 record=3.0):
        run.action("click", role="push button", name="Add / Remove Inspector")
    with run.act("activate Collapse Sidebar through AT-SPI",
                 [not_in_tree(role="panel", name="Sidebar")],
                 should="the sidebar folds", tree=True, record=3.0):
        run.action("click", role="push button", name="Collapse Sidebar")


# ---------------------------------------------------------------------------
# shortcuts-demo
# ---------------------------------------------------------------------------

SC = "shortcuts-demo"


def shortcuts_rebind(run):
    run.wait_for(role="push button", name="Go to line 7")
    run.grab_focus(role="push button", name="Go to line 7")
    with run.act("Tab into the settings panel",
                 [focused(role="push button", name_contains="Rebind"),
                  node_facts("the first Rebind button itself names its shortcut or chord "
                             "(in its name or description)",
                             lambda n: any(t in (n.get("name") or "") + " "
                                           + (n.get("description") or "")
                                           for t in ("Cycle Bounds Overlay", "Ctrl+B")),
                             role="push button", name_contains="Rebind")],
                 should="the reader hears which shortcut this Rebind button rebinds and "
                        "its current chord", tree=True):
        run.key("Tab")
    with run.act("Tab to the next button",
                 [said_any("Cycle Bounds Overlay", "second", "secondary")],
                 should="the reader hears which shortcut and which slot", record=2.0):
        run.key("Tab")
    with run.act("Tab past Reset",
                 [focused(role="push button", name="Rebind")],
                 should="Reset is disabled while no override exists; Tab skips it to the "
                        "next row's Rebind"):
        run.key("Tab")


def shortcuts_capture(run):
    run.wait_for(role="push button", name="Go to line 7")
    run.grab_focus(role="push button", name="Go to line 7")
    with run.act("Tab to the first Rebind",
                 [focused(role="push button", name="Rebind")], should="on Rebind"):
        run.key("Tab")
    with run.act("Space on Rebind (start capturing)",
                 [announced_any("Press"), said("Press")],
                 should="the capture hint 'Press any key…' is announced, and focus stays "
                        "somewhere the reader knows", tree=True, record=3.0):
        run.key("space")
    with run.act("press Ctrl+K (the new chord)",
                 [said("Ctrl+K")],
                 should="the new binding is confirmed to the reader", tree=True, record=3.0):
        run.key("Ctrl+K")
    with run.act("Tab onward",
                 [spoke_anything()],
                 should="focus continues from where the reader was", record=2.0):
        run.key("Tab")


def shortcuts_conflict(run):
    run.wait_for(role="push button", name="Go to line 7")
    run.grab_focus(role="push button", name="Go to line 7")
    with run.act("Tab to the first Rebind (Cycle Bounds Overlay, Ctrl+B)",
                 [focused(role="push button", name="Rebind")], should="on Rebind"):
        run.key("Tab")
    with run.act("Space on Rebind", [said("Press")], should="capture starts",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("press Ctrl+I (Italic's chord)",
                 [said_any("Italic", "conflict", "assigned")],
                 should="the reader is told Ctrl+I was taken from Italic (the demo says "
                        "conflicts auto-resolve, so Italic loses its binding)", tree=True,
                 record=3.0):
        run.key("Ctrl+I")


def announced_any(text: str) -> Check:
    def run(act):
        found = [e for e in act.events if e["type"] == "object:announcement"
                 and normalized(text) in normalized(e.get("text") or "")]
        every = [f"{e['type']} [{e['source'].get('role')}] {e['source'].get('name')!r} "
                 f"text={e.get('text')!r}" for e in act.events
                 if e["type"] == "object:announcement"]
        return bool(found), every or ["no object:announcement in the act"]
    return custom(f"the bus carries an announcement containing {text!r}", run)


def shortcuts_menu(run):
    run.wait_for(role="menu item", name="File")
    with run.act("F10",
                 [focused(role="menu item", name="File"), said("File")],
                 should="F10 focuses the first menu and the reader hears it", tree=True):
        run.key("F10")
    with run.act("Down (open File)",
                 [said_any("Open notes.txt")],
                 should="the File menu opens and the reader hears its first item with "
                        "its shortcut", tree=True, record=3.0):
        run.key("Down")
    with run.act("Down to Save",
                 [said("Save")],
                 should="the reader hears the next item", tree=True):
        run.key("Down")
    with run.act("Escape twice", should="the menu closes", record=2.0):
        run.key("Escape")
        run.wait(0.4)
        run.key("Escape")


# ---------------------------------------------------------------------------
# title-bar-demo
# ---------------------------------------------------------------------------

TBAR = "title-bar-demo"


def titlebar(run):
    run.wait_for(role="push button", name="Maximize")
    with run.act("launch state of the window controls",
                 [in_tree(role="push button", name="Minimize"),
                  in_tree(role="push button", name="Maximize"),
                  in_tree(role="push button", name="Close")],
                 should="the three controls are named", record=0.3, tree=True):
        pass
    with run.act("Tab through the window",
                 [spoke_anything()],
                 should="focus reaches every control there is", record=1.5):
        run.key("Tab")
    with run.act("Tab again", should="focus goes on or wraps", record=1.5):
        run.key("Tab")
    with run.act("Alt+Space (the window menu chord on Windows / most Linux desktops)",
                 [spoke_anything()],
                 should="a keyboard route to the window menu, if the chrome offers one",
                 record=2.5, tree=True):
        run.key("Alt+space")
    with run.act("Escape", record=1.5):
        run.key("Escape")
    with run.act("activate Maximize through AT-SPI",
                 [in_tree(role="push button", name="Restore")],
                 should="the window maximizes and the button now reads Restore",
                 record=3.0, tree=True):
        run.action("click", role="push button", name="Maximize")


# ---------------------------------------------------------------------------
# collapsible-menu-bar
# ---------------------------------------------------------------------------

CMB = "collapsible-menu-bar"


def cmb_f10(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("F10 (reveal the collapsed bar)",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the hidden bar is revealed and the reader hears the first menu, "
                        "in a menu bar", tree=True, record=3.0):
        run.key("F10")
    with run.act("Right to Edit",
                 [focused(role="menu item", name="Edit"), said("Edit")],
                 should="the next menu", tree=True):
        run.key("Right")
    with run.act("Down (open Edit)",
                 [said_any("Undo")],
                 should="the Edit menu opens and the reader hears its first item",
                 tree=True, record=3.0):
        run.key("Down")
    with run.act("Escape",
                 should="the menu closes; focus back on Edit", tree=True, record=2.5):
        run.key("Escape")
    with run.act("Escape again",
                 [spoke_anything()],
                 should="the revealed bar hides and focus lands somewhere the reader is "
                        "told of (the hamburger)", tree=True, record=2.5):
        run.key("Escape")


def cmb_return(run):
    """Where focus goes when the revealed bar hides, from a focused control."""
    run.wait_for(role="slider", name="Bar width")
    with run.act("focus the width slider (AT-SPI)",
                 [focused(role="slider", name="Bar width")], should="on the slider"):
        run.grab_focus(role="slider", name="Bar width")
    with run.act("F10 (reveal the collapsed bar)",
                 [focused(role="menu item", name="File")],
                 should="the bar is revealed with focus on File", record=2.5):
        run.key("F10")
    with run.act("Escape (hide the bar)",
                 [focused(role="slider", name="Bar width"), said("Bar width")],
                 should="the bar hides and focus returns to the slider the user left",
                 record=2.5):
        run.key("Escape")
    with run.act("tap Alt (reveal again)",
                 [focused(role="menu item", name="File")],
                 should="the bar is revealed with focus on File", record=2.5):
        run.key("Alt")
    with run.act("tap Alt again (hide it, as the Alt-tap toggle does on Windows)",
                 [focused(role="slider", name="Bar width")],
                 should="a second Alt tap leaves the menu bar and returns focus to the "
                        "slider", record=2.5):
        run.key("Alt")


def cmb_alt_letter(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("Alt+V (reveal and open View)",
                 [said_any("Zoom In")],
                 should="the bar is revealed with View open, and the reader hears the "
                        "menu's first item", tree=True, record=3.5):
        run.key("Alt+v")
    with run.act("Escape twice", record=2.5, tree=True):
        run.key("Escape")
        run.wait(0.5)
        run.key("Escape")


def cmb_alt_tap(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("tap Alt (reveal, focus the first menu)",
                 [focused(role="menu item", name="File"), said("File")],
                 should="a bare Alt tap reveals the bar and the reader hears File",
                 tree=True, record=3.0):
        run.key("Alt")
    with run.act("Escape", record=2.5, tree=True):
        run.key("Escape")


def cmb_hamburger(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("Tab to the hamburger",
                 [focused(role="push button", name="Menu"), said("Menu"),
                  said_any("collapsed", "menu", "popup")],
                 should="the reader hears the hamburger and that it opens something",
                 tree=True):
        run.key("Tab")
    with run.act("Space on the hamburger",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the bar floats in with focus on File, and the reader hears it",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Escape", [spoke_anything()], should="the bar hides; focus back on the "
                 "hamburger", tree=True, record=2.5):
        run.key("Escape")


def cmb_slider(run):
    run.wait_for(role="slider", name="Bar width")
    run.grab_focus(role="slider", name="Bar width")
    with run.act("Home on the width slider (narrow the second bar)",
                 [said_any("collapsed")],
                 should="the second bar folds into a hamburger; the status label says so",
                 tree=True, record=3.0):
        run.key("Home")


# ---------------------------------------------------------------------------
# native-menu
# ---------------------------------------------------------------------------

NM = "native-menu"


def native_menu(run):
    run.wait_for(role="menu item", name="File")
    with run.act("F10",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the reader hears the first menu", tree=True):
        run.key("F10")
    with run.act("Down (open File)",
                 [said_any("New")],
                 should="File opens; the reader hears New and its shortcut", tree=True,
                 record=3.0):
        run.key("Down")
    with run.act("Down, Down (to the disabled Save)",
                 [said("Save")],
                 should="the reader hears Save and that it is unavailable", tree=True,
                 record=2.5):
        run.key("Down")
        run.wait(0.4)
        run.key("Down")
    with run.act("Escape, Escape", record=2.0):
        run.key("Escape")
        run.wait(0.4)
        run.key("Escape")


def native_menu_mnemonic(run):
    run.wait_for(role="menu item", name="File")
    with run.act("Alt+F (open File by its mnemonic)",
                 [said_any("New")],
                 should="File opens and the reader hears its first item", tree=True,
                 record=3.0):
        run.key("Alt+f")
    with run.act("N (activate New by its in-menu mnemonic)",
                 [said("New chosen")],
                 should="New runs; the status line 'New chosen' is the only feedback and "
                        "a reader should hear it", tree=True, record=3.0):
        run.key("n")


def native_menu_check(run):
    run.wait_for(role="menu item", name="View")
    with run.act("Alt+V",
                 [said_any("Show Grid")],
                 should="View opens; the reader hears 'Show Grid', checked", tree=True,
                 record=3.0):
        run.key("Alt+v")
    with run.act("Enter on Show Grid",
                 [said_any("Grid: hidden", "not checked", "unchecked")],
                 should="the item toggles and the reader hears the new state", tree=True,
                 record=3.0):
        run.key("Return")


def native_activate(run):
    """Which activation paths reach the example's actions."""
    run.wait_for(role="menu item", name="File")
    with run.act("Ctrl+O (the Open shortcut)",
                 [in_tree(role="label", name="Open chosen")],
                 should="the shortcut runs Open; the status line says 'Open chosen'",
                 tree=True):
        run.key("Ctrl+o")
    with run.act("focus 'Add recent file' (AT-SPI), then Ctrl+O",
                 [in_tree(role="label", name="Open chosen")],
                 should="from a focused control inside the example's root widget, the "
                        "shortcut reaches the action", tree=True):
        run.grab_focus(role="push button", name="Add recent file")
        run.wait(0.8)
        run.key("Ctrl+o")
    with run.act("Alt+F, then a screen reader's click on 'New'",
                 [in_tree(role="label", name="New chosen")],
                 should="New runs from the in-window menu; the status says 'New chosen'",
                 tree=True, record=3.0):
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="New")
    with run.act("Escape", record=1.5):
        run.key("Escape")
    with run.act("Alt+V, then a screen reader's click on 'Show Grid'",
                 [in_tree(role="label", name_contains="Grid: hidden")],
                 should="Show Grid toggles off; the body label says 'Grid: hidden'",
                 tree=True, record=3.0):
        run.key("Alt+v")
        run.wait(1.0)
        run.action("click", role="check menu item", name="Show Grid")


def shortcuts_menu_activate(run):
    run.wait_for(role="menu item", name="File")
    mark = log_mark(run)
    with run.act("F10, Down (open File), then a screen reader's click on 'Save'",
                 [printed("[action] Save", mark)],
                 should="the Save action runs (the demo prints '[action] Save')", record=2.5):
        run.key("F10")
        run.wait(0.6)
        run.key("Down")
        run.wait(1.0)
        run.action("click", role="menu item", name="Save")
    mark2 = log_mark(run)
    with run.act("focus 'Go to line 7' (AT-SPI), then Ctrl+S",
                 [printed("[action] Save", mark2)],
                 should="the same action from its shortcut", record=2.0):
        run.grab_focus(role="push button", name="Go to line 7")
        run.wait(0.8)
        run.key("Ctrl+s")


def cmb_menu_activate(run):
    run.wait_for(role="push button", name="Menu")
    mark = log_mark(run)
    with run.act("Tab, Space (reveal), Down (open File), a screen reader's click on 'New'",
                 [printed("New", mark)],
                 should="New runs (the demo prints 'New')", record=2.5):
        run.key("Tab")
        run.wait(0.5)
        run.key("space")
        run.wait(0.8)
        run.key("Down")
        run.wait(1.0)
        run.action("click", role="menu item", name="New")
    mark2 = log_mark(run)
    with run.act("Escape, Tab (to the slider), then Ctrl+N",
                 [printed("New", mark2)], should="the same action from its shortcut",
                 record=2.0):
        run.key("Escape")
        run.wait(0.6)
        run.grab_focus(role="slider", name="Bar width")
        run.wait(0.6)
        run.key("Ctrl+n")


def shortcuts_nofocus(run):
    run.wait_for(role="menu item", name="File")
    mark = log_mark(run)
    with run.act("Ctrl+S with nothing focused (just after launch)",
                 [printed("[action] Save", mark)],
                 should="a global shortcut reaches its action", record=2.0):
        run.key("Ctrl+s")


def native_focused_menu(run):
    run.wait_for(role="push button", name="Add recent file")
    with run.act("focus 'Add recent file' (AT-SPI), Alt+F, a screen reader's click on 'New'",
                 [in_tree(role="label", name="New chosen")],
                 should="New runs from the in-window menu", tree=True, record=3.0):
        run.grab_focus(role="push button", name="Add recent file")
        run.wait(0.8)
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="New")
    with run.act("Alt+E, Down, Return (keyboard activation of Cut)",
                 [in_tree(role="label", name="Cut chosen")],
                 should="Cut runs from the in-window menu by keyboard", tree=True,
                 record=3.0):
        run.key("Alt+e")
        run.wait(0.8)
        run.key("Down")
        run.wait(0.5)
        run.key("Return")


SCENARIOS = [
    Scenario("chrome-shortcuts-nofocus", SC, shortcuts_nofocus,
             "a global shortcut with nothing focused"),
    Scenario("chrome-native-focused-menu", NM, native_focused_menu,
             "native-menu's in-window menu commands, from a focused control"),
    Scenario("chrome-shortcuts-menu-activate", SC, shortcuts_menu_activate,
             "a menu command and its shortcut reach the same action"),
    Scenario("chrome-cmb-menu-activate", CMB, cmb_menu_activate,
             "a menu command and its shortcut reach the same action"),
    Scenario("chrome-native-activate", NM, native_activate,
             "which activation paths reach native-menu's actions"),
    Scenario("chrome-walk-tooltips", TIPS, walk(20), "Tab through tooltips-showcase"),
    Scenario("chrome-walk-toolbox", TB, walk(8), "Tab through tool-box"),
    Scenario("chrome-walk-splitter", SP, walk(12), "Tab through splitter"),
    Scenario("chrome-walk-shortcuts", SC, walk(14), "Tab through shortcuts-demo"),
    Scenario("chrome-walk-titlebar", TBAR, walk(6), "Tab through title-bar-demo"),
    Scenario("chrome-walk-cmb", CMB, walk(10), "Tab through collapsible-menu-bar"),
    Scenario("chrome-walk-native", NM, walk(10), "Tab through native-menu"),
    Scenario("chrome-tips-plain", TIPS, tips_plain, "plain tooltips on focus"),
    Scenario("chrome-tips-rich", TIPS, tips_rich, "a rich tooltip summoned by focus"),
    Scenario("chrome-tips-rich-desc", TIPS, tips_rich_after,
             "a registry-keyed rich tooltip's description before and after it is built"),
    Scenario("chrome-tips-composite", TIPS, tips_composite,
             "a composite tooltip summoned by focus"),
    Scenario("chrome-tips-interactive", TIPS, tips_interactive,
             "the button inside a composite tooltip"),
    Scenario("chrome-tips-menu", TIPS, tips_menu, "the right-click menu's keyboard route"),
    Scenario("chrome-tips-snapback", TIPS, tips_snapback,
             "Tab away from an anchor whose focus-summoned tip is showing"),
    Scenario("chrome-tips-reshow", TIPS, tips_reshow,
             "a composite tip shown twice, entered by Tab each time"),
    Scenario("chrome-toolbox", TB, toolbox, "ToolBox headers: focus, arrows, expand"),
    Scenario("chrome-splitter", SP, splitter, "Splitter divider: focus, resize, collapse"),
    Scenario("chrome-splitter-buttons", SP, splitter_buttons, "Splitter toolbar buttons"),
    Scenario("chrome-shortcuts-rebind", SC, shortcuts_rebind, "ShortcutSettings buttons"),
    Scenario("chrome-shortcuts-capture", SC, shortcuts_capture, "rebinding a chord"),
    Scenario("chrome-shortcuts-conflict", SC, shortcuts_conflict,
             "rebinding onto a taken chord"),
    Scenario("chrome-shortcuts-menu", SC, shortcuts_menu, "the menu bar's shortcut labels"),
    Scenario("chrome-titlebar", TBAR, titlebar, "custom title bar controls"),
    Scenario("chrome-cmb-f10", CMB, cmb_f10, "F10 reveals the hamburger bar"),
    Scenario("chrome-cmb-alt-letter", CMB, cmb_alt_letter, "Alt+V reveals and opens"),
    Scenario("chrome-cmb-return", CMB, cmb_return,
             "focus after the revealed bar hides, starting from the slider"),
    Scenario("chrome-cmb-alt-tap", CMB, cmb_alt_tap, "a bare Alt tap reveals"),
    Scenario("chrome-cmb-hamburger", CMB, cmb_hamburger, "the hamburger by keyboard"),
    Scenario("chrome-cmb-slider", CMB, cmb_slider, "the responsive bar collapses"),
    Scenario("chrome-native-menu", NM, native_menu, "native-menu in-window bar by F10"),
    Scenario("chrome-native-mnemonic", NM, native_menu_mnemonic, "Alt+F then N"),
    Scenario("chrome-native-check", NM, native_menu_check, "View > Show Grid check item"),
]
