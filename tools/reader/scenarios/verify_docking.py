# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verification scenarios for the docking sweep (`docking.py`).

Each scenario here exists to settle one question the sweep left open:

* `verify-docking-submenu-routes`: why a "Move to side" submenu closes itself.
  The same submenu is opened three ways: Right on the highlighted row (the
  MenuList's own key path, which activates the row through `synthetic_click`),
  an AT-SPI click on the row (the item's `activate_item`, which opens it as
  `SubmenuOpenRoute::KeyboardOrAt`), and Enter on the highlighted row.
* `verify-docking-activity-switch`: whether switching a rail side between two
  activities and back leaves the controls Orca met before the switch as
  defunct as hiding the side does (the sweep claimed so without measuring it).
* `verify-docking-reshown-persist`: whether a control Orca met before its side
  was hidden stays silent on every later visit, or only the first.
* `verify-docking-hide-focused-bottom`: the focus loss of docking-02 through a
  second side (the bottom panel, Toggle Panel).
"""

from __future__ import annotations

from reader_lib.checks import custom, event, focused, in_tree, not_in_tree, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

from scenarios.docking import (EXPLORER, PROPERTIES_TAB, RAIL_TAB, focus_act,
                               focus_kept_or_moved_to_named, more_actions, no_defunct_drop,
                               said_any, spoke_anything)

PKG = "docking"


def no_menu_removed() -> "custom":
    """No `[menu]` left the tree during the act (a submenu that closes itself
    is removed from the frame with no input)."""
    def run(act):
        removed = [e for e in act.events if e["type"] == "object:children-changed:remove"
                   and (e.get("target") or {}).get("role") == "menu"]
        return not removed, [f"{(e['mono'] - act.start_mono) * 1000:+.1f} ms "
                             f"children-changed:remove -> [menu]" for e in removed] \
            or ["no menu removed in the act"]
    return custom("no menu leaves the tree during the act", run)


def orca_heard() -> "custom":
    def run(act):
        heard = utterances(act.orca)
        return True, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in heard] \
            or ["Orca said nothing in this act"]
    return custom("(record) what Orca said", run, needs_orca=True)


def submenu_routes(run):
    """Explorer's options menu, the Move to side submenu opened three ways."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"))
    with run.act("Enter: open the options menu",
                 [in_tree(role="menu item", name="Move to side")], should="the menu opens"):
        run.key("Return")
    with run.act("Down, Down: highlight Move to side", [orca_heard()], tree=True,
                 should="the highlight is on Move to side (the tree shows whether any state "
                        "tells a reader which item is highlighted)"):
        run.key("Down", "Down")
    with run.act("Right on the highlighted row (MenuList synthetic_click path)",
                 [in_tree(role="menu item", name="Trailing"), no_menu_removed(), orca_heard()],
                 record=3.0, should="the submenu opens and stays open"):
        run.key("Right")
    # If it closed itself, focus is back on the options menu with the
    # highlight still on Move to side; if it did not, close it first.
    if run.find(role="menu item", name="Trailing"):
        with run.act("Left: close the submenu", should="back to the options menu"):
            run.key("Left")
    with run.act("AT-SPI click on the Move to side row (activate_item, KeyboardOrAt route)",
                 [in_tree(role="menu item", name="Trailing"), no_menu_removed(), orca_heard()],
                 record=3.0, should="the submenu opens and stays open"):
        run.action("click", role="menu item", name="Move to side")
    with run.act("Escape: close the submenu", [orca_heard()], tree=True,
                 should="the submenu closes, the options menu stays"):
        run.key("Escape")
    if not run.find(role="menu item", name="Move to side"):
        with run.act("reopen the options menu", should="the menu opens again"):
            run.grab_focus(**more_actions("explorer"))
            run.wait(0.8)
            run.key("Return")
            run.wait(0.8)
            run.key("Down", "Down")
    with run.act("Enter on the highlighted Move to side row (synthetic_click path)",
                 [in_tree(role="menu item", name="Trailing"), no_menu_removed(), orca_heard()],
                 record=3.0, should="the submenu opens and stays open"):
        run.key("Return")
    with run.act("Escape", should="close whatever is open"):
        run.key("Escape")
    with run.act("Escape", should="close whatever is open"):
        run.key("Escape")


def activity_switch(run):
    """Move Properties to the leading rail (the 40 ms key path the sweep used),
    then switch the rail between Source and Properties and back. The rail
    keeps each activity's content across a switch (relayout, not rebuild), so
    the inactive activity's content is parked dormant, which the AT-SPI
    adapter reports as defunct nodes."""
    run.wait_for(**PROPERTIES_TAB)
    focus_act(run, "the Properties tab", PROPERTIES_TAB)
    with run.act("setup: Shift+F10, Down, Down, Right, Down, Enter at 40 ms a key",
                 [in_tree(role="page tab", name="Properties")],
                 should="Properties moves to the leading rail"):
        run.key("Shift+F10", "Down", "Down", "Right", "Down", "Return", gap=0.04)
    run.snapshot("after Move to Leading")
    source = run.find(role="page tab", name="Source")
    run.note(f"after the move the Source rail tab's states are {source and source.get('states')}")
    if source and "selected" not in source.get("states", []):
        focus_act(run, "the Source rail tab", RAIL_TAB)
        with run.act("Enter on Source: show the Source activity",
                     [in_tree(role="push button", name="Explorer")],
                     should="Source's content (Explorer, Search) is shown"):
            run.key("Return")
    focus_act(run, "the Explorer header, before any switch", EXPLORER,
              [focused(role="push button", name="Explorer"), said("Explorer")])
    focus_act(run, "the Properties rail tab", {"role": "page tab", "name": "Properties"},
              [said("Properties")])
    with run.act("Enter on Properties: switch the rail to Properties",
                 [not_in_tree(role="push button", name="Explorer"), orca_heard()],
                 should="Properties' content replaces Source's"):
        run.key("Return")
    focus_act(run, "the Source rail tab again", RAIL_TAB, [said("Source")])
    with run.act("Enter on Source: switch back",
                 [in_tree(role="push button", name="Explorer"), orca_heard()],
                 should="Source's content comes back"):
        run.key("Return")
    focus_act(run, "the Search header (never met before the switch)",
              {"role": "push button", "name": "Search"},
              [said("Search"), no_defunct_drop()])
    focus_act(run, "the Explorer header (met before the switch)", EXPLORER,
              [focused(role="push button", name="Explorer"), said("Explorer"),
               no_defunct_drop()],
              should="the reader hears Explorer, as before the switch")
    with run.act("Space on Explorer (collapse its pane)", [no_defunct_drop(), orca_heard()],
                 should="the pane collapses; nothing is dropped as defunct"):
        run.key("space")


def reshown_persist(run):
    """Does a control Orca met before its side was hidden stay silent on every
    later visit, or recover?"""
    run.wait_for(**EXPLORER)
    focus_act(run, "the Explorer header", EXPLORER, [said("Explorer")])
    focus_act(run, "the Source rail tab", RAIL_TAB)
    with run.act("Enter: hide the leading side",
                 [not_in_tree(role="landmark", name="Leading panel")], should="the side hides"):
        run.key("Return")
    with run.act("Enter: show it again", [in_tree(role="landmark", name="Leading panel")],
                 should="the side comes back"):
        run.key("Return")
    for visit in (1, 2, 3):
        focus_act(run, f"the Explorer header, visit {visit} after the re-show", EXPLORER,
                  [focused(role="push button", name="Explorer"), said("Explorer"),
                   no_defunct_drop()])
        focus_act(run, f"the Search header, visit {visit}", {"role": "push button",
                                                             "name": "Search"},
                  [said("Search"), no_defunct_drop()])
    with run.act("Space on Explorer while focused (after a grab)", [no_defunct_drop(),
                                                                   orca_heard()],
                 should="the pane collapses; nothing dropped"):
        run.grab_focus(**EXPLORER)
        run.wait(1.0)
        run.key("space")


def hide_focused_bottom(run):
    """docking-02 through the bottom side: focus on the Terminal header, then
    an AT-SPI click on Toggle Panel."""
    terminal = {"role": "push button", "name": "Terminal"}
    run.wait_for(**terminal)
    focus_act(run, "the Terminal dock header", terminal, [focused(role="push button",
                                                                  name="Terminal")])
    with run.act("AT-SPI click on Toggle Panel while Terminal has focus",
                 [not_in_tree(role="landmark", name="Bottom panel"),
                  focus_kept_or_moved_to_named(), spoke_anything()],
                 should="the bottom side hides; focus moves to a live named control"):
        run.action("click", role="push button", name="Toggle Panel")
    with run.act("Tab", [spoke_anything()], should="Tab reaches a control"):
        run.key("Tab")


def status_line(run):
    """The example's status line reports what the header actions and Export /
    Restore did. Is any of it announced?"""
    new_file = {"role": "push button", "name": "New File"}
    run.wait_for(**new_file)
    focus_act(run, "New File", new_file)
    with run.act("Space on New File",
                 [event("object:announcement"), orca_heard()],
                 should="the status line changes to 'New File (demo action)'; a reader "
                        "hears it"):
        run.key("space")
    focus_act(run, "Export", {"role": "push button", "name": "Export"})
    with run.act("Space on Export", [event("object:announcement"), orca_heard()],
                 tree=True, should="the status line says 'Exported layout.'"):
        run.key("space")


def menu_reopen(run):
    """Open Explorer's options menu, close it, open it again. The popover's
    content is parked dormant on dismiss and shown again with the same ids,
    which is the defunct-reuse mechanism of docking-01 applied to a menu. The
    rail tab's context menu is the control: it is built fresh each time."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"))
    for n in (1, 2, 3):
        with run.act(f"Enter: open the options menu, time {n}",
                     [focused(role="menu"), said("menu"), no_defunct_drop()],
                     should="the menu opens and the reader hears 'menu'"):
            run.key("Return")
        with run.act(f"Escape: close it, time {n}",
                     [focused(role="push button", name="More actions"), said("More actions")],
                     should="focus returns to the options button and the reader hears it"):
            run.key("Escape")
    more = {"role": "push button", "name": "More", "nth": 0}
    focus_act(run, "Explorer's toolbar More dropdown", more)
    for n in (1, 2):
        with run.act(f"Enter: open the More dropdown, time {n}",
                     [focused(role="menu"), said("menu"), no_defunct_drop()],
                     should="the dropdown opens and the reader hears 'menu'"):
            run.key("Return")
        with run.act(f"Escape: close the dropdown, time {n}", should="focus returns"):
            run.key("Escape")
    focus_act(run, "the Source rail tab", RAIL_TAB)
    for n in (1, 2):
        with run.act(f"Shift+F10: rail context menu, time {n}",
                     [focused(role="menu"), said("menu"), no_defunct_drop()],
                     should="the context menu opens and the reader hears 'menu'"):
            run.key("Shift+F10")
        with run.act(f"Escape: close the context menu, time {n}", should="focus returns"):
            run.key("Escape")


def submenu_trace(run):
    """The Right-key submenu, with the app's input trace on
    (`TEKSILO_TRACE_INPUT=all`, `crates/teksilo-core/src/pointer/trace.rs`),
    to see in app.log whether any real pointer sample reaches the app."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"))
    with run.act("Enter, Down, Down", should="the menu is open, Move to side highlighted"):
        run.key("Return")
        run.wait(0.8)
        run.key("Down", "Down")
    with run.act("Right on the highlighted row",
                 [in_tree(role="menu item", name="Trailing"), no_menu_removed()],
                 record=3.0, should="the submenu opens and stays open"):
        run.key("Right")


def header_dropdown(run):
    """A dock header's dropdown action (the example's "More" ToolbarAction
    with a `.menu(..)`, hosted by the framework's header `Toolbar`): what does
    focus land on when it opens, and what is in the tree?"""
    more = {"role": "push button", "name": "More", "nth": 0}
    run.wait_for(**more)
    focus_act(run, "Explorer's header More dropdown", more)
    with run.act("Enter: open the dropdown", [focused(role="menu"), said("menu"), orca_heard()],
                 tree=True, should="a menu opens, focus is on it, the reader hears 'menu'"):
        run.key("Return")
    with run.act("Down", [orca_heard()], tree=True,
                 should="the first item is highlighted and the reader hears 'Sort by name'"):
        run.key("Down")
    with run.act("Escape", [focused(role="push button", name="More"), orca_heard()],
                 should="the dropdown closes, focus returns to More"):
        run.key("Escape")


SCENARIOS = [
    Scenario("verify-docking-header-dropdown", PKG, header_dropdown,
             "a dock header's dropdown action: focus target and tree"),
    Scenario("verify-docking-submenu-trace", PKG, submenu_trace,
             "the Right-key submenu with the app's input trace on",
             env={"TEKSILO_TRACE_INPUT": "all"}),
    Scenario("verify-docking-menu-reopen", PKG, menu_reopen,
             "options menu / dropdown / context menu opened, closed and opened again"),
    Scenario("verify-docking-submenu-routes", PKG, submenu_routes,
             "the Move to side submenu opened by Right, by AT-SPI click and by Enter"),
    Scenario("verify-docking-activity-switch", PKG, activity_switch,
             "switch the leading rail between two activities and back, then revisit"),
    Scenario("verify-docking-reshown-persist", PKG, reshown_persist,
             "after hide/show, revisit a control Orca met before, three times"),
    Scenario("verify-docking-hide-focused-bottom", PKG, hide_focused_bottom,
             "hide the bottom side while focus is on its Terminal header"),
    Scenario("verify-docking-status-line", PKG, status_line,
             "the example's status line after a header action and Export"),
]
