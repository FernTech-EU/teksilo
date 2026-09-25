# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""docking: `DockingLayout` as a screen reader user meets it.

The example (`examples/docking/src/main.rs`) opens a VS Code-style shell:

* a toolbar of plain `Button`s (Toggle Sidebar / Toggle Panel / Toggle
  Inspector / Flip Corner / Lock Layout / Export / Restore) and a theme combo;
* a **leading** side in Rail presentation: an activity rail
  (`DockActivityBar`, a `Role::TabList` "Leading activity bar" holding one
  `Role::Tab` "Source", `activity_bar.rs` `DockRailItem::accessibility`) whose
  one activity holds two docks, Explorer and Search, each an `Accordion`
  (`accordion.rs`, `Role::Button` + `expanded`) in a `Splitter`;
* a **trailing** side in Strip presentation: a `TabWidget` with one tab,
  Properties, a sole-pane dock with an opted-in header bar;
* a **bottom** side in Strip presentation: one tab, Terminal, holding
  Terminal + Problems as two accordion panes.

Each side's content is a `DockSidePanel`, a `Role::Complementary` landmark
named "Leading panel" / "Trailing panel" / "Bottom panel" (`panel.rs`,
`a11y::side_label`); each side has a `DockResizeHandle`, a `Role::Splitter`
with a percentage value and `expanded` (`resize_handle.rs`). Each dock header
carries a `⋮` options menu (`PopoverIconButton`, `.access_label("More actions:
<dock>")`, `panel.rs` `dock_header_trailing`) and the rail items and strip tabs
carry a context menu (`context_menu.rs`) with Hide / Move to / the activity
check list / a size submenu: the keyboard alternative to dragging a dock.

Walk order of repeated nodes (the listener's `nth` counts matches in tree
order): the focusable unnamed separators are the dock resize handles, Leading,
Trailing, Top (zero-sized: the top side is empty) and Bottom; the "More
actions" buttons are Explorer, Search, Properties, Terminal, Problems.

No message in this example goes through the framework announcer
(`ctx.announce`): nothing in `crates/teksilo-widgets/src/docking*` calls it,
and the example calls it nowhere, so K2's fix covers nothing here. The same
mechanism does reach docking another way: hiding a side (or switching its
activity) parks its content dormant, the adapter marks every node defunct,
and showing it re-adds the same ids (docking-reshown-events,
docking-reshown-visited).

Several checks state what a reader should get and fail on main; those
failures are the findings, not scenario errors.
"""

from __future__ import annotations

from reader_lib.checks import (Check, _focus_node, _is_focus, _walk, custom, event,
                               focused, in_tree, not_in_tree, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

PKG = "docking"

RAIL_TAB = {"role": "page tab", "name": "Source"}
EXPLORER = {"role": "push button", "name": "Explorer"}
SETTINGS = {"role": "push button", "name": "Settings"}
PROPERTIES_TAB = {"role": "page tab", "name": "Properties"}


def handle(which: str) -> dict:
    """A dock resize handle, by side."""
    return {"role": "separator", "name": "", "state": "focusable",
            "nth": {"leading": 0, "trailing": 1, "top": 2, "bottom": 3}[which]}


def more_actions(dock: str) -> dict:
    """A dock header's `⋮` options button, by dock."""
    return {"role": "push button", "name": "More actions",
            "nth": {"explorer": 0, "search": 1, "properties": 2, "terminal": 3,
                    "problems": 4}[dock]}


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def said_any(*texts: str) -> Check:
    """Orca said at least one of `texts`, uncut."""
    def run(act):
        heard = utterances(act.orca)
        hit = [u for u in heard for t in texts
               if normalized(t) in normalized(u.text) and not u.cut]
        return bool(hit), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                           for u in heard] or ["Orca said nothing in this act"]
    return custom(f"Orca says one of {texts!r}", run, needs_orca=True)


def spoke_anything() -> Check:
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                             for u in heard] or ["Orca said nothing in this act"]
    return custom("Orca says something", run, needs_orca=True)


def _focus_before(act) -> dict:
    moves = [e for e in act.history if _is_focus(e) and e["mono"] < act.start_mono]
    return _focus_node(moves[-1]) if moves else {}


def focus_kept_or_moved_to_named() -> Check:
    """After the act a reader still has a live, named focus: either the act
    moved focus to a named control, or the control focused before it is still
    in the tree and still focused."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if moves:
            last = _focus_node(moves[-1])
            ok = bool(last.get("name")) and last.get("role") not in ("frame", "application")
            return ok, [f"focus moved to [{last.get('role')}] {last.get('name')!r}"]
        before = _focus_before(act)
        path = before.get("path")
        for node in _walk(act.tree):
            if node.get("path") == path:
                ok = "focused" in node.get("states", [])
                return ok, [f"no focus change; [{node.get('role')}] {node.get('name')!r} "
                            f"is still in the tree, states={node.get('states')}"]
        return False, [f"no focus change, and the node focused before the act, "
                       f"[{before.get('role')}] {before.get('name')!r}, is no longer in the "
                       "tree a reader walks: focus is on nothing"]
    return custom("the reader's focus is still on a live, named control", run,
                  needs_tree=True)


def no_focus_change() -> Check:
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        return not moves, [f"focus moved to [{_focus_node(e).get('role')}] "
                           f"{_focus_node(e).get('name')!r}" for e in moves]
    return custom("focus stays where it was", run)


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def focus_act(run, label: str, spec: dict, expect: list | None = None,
              should: str = "") -> None:
    """Put focus somewhere through AT-SPI, as its own act, so what Orca says
    about it is not credited to the act after it."""
    with run.act(f"focus {label} (AT-SPI grab_focus)", expect or [],
                 should=should or f"focus lands on {label} and the reader says what it is"):
        run.grab_focus(**spec)


def rail_toggle(run):
    """The active rail item hides and shows its side (`DockRailItem`'s
    `activate`: re-activating the selected item hides the side)."""
    run.wait_for(**RAIL_TAB)
    focus_act(run, "the Source rail tab", RAIL_TAB,
              [focused(role="page tab", name="Source"), said("Source")])
    with run.act("Enter on the active rail tab",
                 [not_in_tree(role="landmark", name="Leading panel"),
                  no_focus_change(), said_any("collapsed", "hidden")],
                 should="the leading side hides, focus stays on the rail tab, and the "
                        "reader hears that its panel collapsed"):
        run.key("Return")
    with run.act("Enter again",
                 [in_tree(role="landmark", name="Leading panel"),
                  no_focus_change(), said_any("expanded", "shown")],
                 should="the leading side comes back and the reader hears it expanded"):
        run.key("Return")
    with run.act("Space on the active rail tab",
                 [not_in_tree(role="landmark", name="Leading panel"),
                  said_any("collapsed", "hidden")],
                 should="Space does what Enter does"):
        run.key("space")


def handle_keys(run):
    """The leading side's resize handle from the keyboard: arrows resize, Home
    hides the side, End shows it, Enter toggles it (`resize_handle.rs`)."""
    run.wait_for(**handle("leading"))
    focus_act(run, "the leading resize handle", handle("leading"),
              [focused(role="separator"), said_any("Leading", "Explorer", "sidebar")],
              should="the reader hears which divider this is (it resizes the leading "
                     "side) and its value")
    with run.act("Right arrow on the handle",
                 [event("object:property-change:accessible-value", role="separator"),
                  said_any("%", "21", "22", "23")],
                 should="the leading side grows by a step and the reader hears the new "
                        "size"):
        run.key("Right")
    with run.act("Left arrow on the handle",
                 [event("object:property-change:accessible-value", role="separator"),
                  spoke_anything()],
                 should="the side shrinks back and the reader hears the new size"):
        run.key("Left")
    with run.act("Home on the handle",
                 [not_in_tree(role="landmark", name="Leading panel"),
                  focus_kept_or_moved_to_named(), said_any("collapsed", "hidden")],
                 should="Home hides the leading side; the reader hears it collapsed and "
                        "focus stays on a control they can use to bring it back"):
        run.key("Home")
    with run.act("End on the handle",
                 [in_tree(role="landmark", name="Leading panel"),
                  said_any("expanded", "shown")],
                 should="End shows the side again (the handle's own documented key)"):
        run.key("End")
    with run.act("Enter on the handle",
                 [said_any("collapsed", "hidden")],
                 should="Enter toggles the side"):
        run.key("Return")


def accordion_header(run):
    """A dock's accordion header (Explorer): Space collapses its pane, Space
    again expands it (`accordion.rs`: `Role::Button` + `set_expanded`)."""
    run.wait_for(**EXPLORER)
    focus_act(run, "the Explorer dock header", EXPLORER,
              [focused(role="push button", name="Explorer"),
               said_any("expanded")],
              should="the reader hears 'Explorer', that it is a disclosure, and that it "
                     "is expanded")
    with run.act("Space on the Explorer header",
                 [no_focus_change(), said_any("collapsed")],
                 should="the Explorer pane folds to its header and the reader hears "
                        "'collapsed'"):
        run.key("space")
    with run.act("Space again",
                 [no_focus_change(), said_any("expanded")],
                 should="the pane comes back and the reader hears 'expanded'"):
        run.key("space")


def reshown_content(run):
    """Hide the leading side from its rail tab, show it again, then walk back
    into its content. A hidden side's `DockSidePanel` is parked dormant
    (`docking.rs`, `visible_when(panel, progress > eps)`), which prunes its
    whole subtree from the AT tree; showing it re-adds the same widget ids."""
    run.wait_for(**RAIL_TAB)
    focus_act(run, "the Source rail tab", RAIL_TAB, [focused(role="page tab", name="Source")])
    with run.act("Enter: hide the leading side",
                 [not_in_tree(role="landmark", name="Leading panel")], should="the side hides"):
        run.key("Return")
    with run.act("Enter: show it again", [in_tree(role="landmark", name="Leading panel")],
                 should="the side comes back"):
        run.key("Return")
    with run.act("Shift+Tab into the shown side",
                 [focused(role="push button", name="More actions"), said("More actions")],
                 should="focus moves back into the side's content (Search's options button) "
                        "and the reader hears it"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab again",
                 [focused(role="push button", name="Search"), said("Search")],
                 should="focus moves to the Search dock header and the reader hears it"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to the splitter divider",
                 [focused(role="separator"), said("splitter")],
                 should="focus moves to the divider between Explorer and Search"):
        run.key("Shift+Tab")


def rail_menu(run):
    """The rail tab's context menu from the keyboard: Shift+F10, arrows,
    Escape, then the Menu key (`activity_context_menu`)."""
    run.wait_for(**RAIL_TAB)
    focus_act(run, "the Source rail tab", RAIL_TAB, [focused(role="page tab", name="Source")])
    with run.act("Shift+F10 on the rail tab",
                 [in_tree(role="menu"), in_tree(role="menu item", name_contains="Hide"),
                  said_any("menu"), said_any("Hide")],
                 should="the activity's context menu opens; the reader hears a menu and its "
                        "first item, Hide \"Source\""):
        run.key("Shift+F10")
    with run.act("Down in the menu", [said_any("Hide")],
                 should="the highlight moves to the first item and the reader hears it"):
        run.key("Down")
    with run.act("Down again", [said_any("Move to")],
                 should="the reader hears the next item, Move to"):
        run.key("Down")
    with run.act("Down again (the activity check list)", [said_any("Source")],
                 should="the reader hears the check item Source and that it is checked"):
        run.key("Down")
    with run.act("Escape", [focused(role="page tab", name="Source"), said("Source")],
                 should="the menu closes and focus returns to the rail tab"):
        run.key("Escape")
    with run.act("the Menu key on the rail tab", [in_tree(role="menu"), spoke_anything()],
                 should="the Menu key opens the same menu"):
        run.key("Menu")
    with run.act("Escape again", [focused(role="page tab", name="Source")],
                 should="the menu closes and focus returns to the rail tab"):
        run.key("Escape")


def hide_activity(run):
    """Hide the only activity of the leading rail from its context menu, then
    try to bring it back from the keyboard."""
    run.wait_for(**RAIL_TAB)
    focus_act(run, "the Source rail tab", RAIL_TAB, [focused(role="page tab", name="Source")])
    with run.act("Shift+F10 on the rail tab", [in_tree(role="menu item", name_contains="Hide")],
                 should="the activity's context menu opens"):
        run.key("Shift+F10")
    with run.act("Down, Enter: Hide \"Source\"",
                 [not_in_tree(role="page tab", name="Source"),
                  focus_kept_or_moved_to_named(), spoke_anything()],
                 should="the activity is hidden; focus lands on a named control nearby "
                        "and the reader hears where it is"):
        run.key("Down", "Return")
    with run.act("Tab", [spoke_anything()],
                 should="Tab from wherever focus is now reaches a control the reader hears"):
        run.key("Tab")
    focus_act(run, "the rail's Settings button", SETTINGS,
              [focused(role="push button", name="Settings")])
    with run.act("Shift+F10 on the rail's Settings button",
                 [in_tree(role="check menu item", name="Source"), spoke_anything()],
                 should="the rail's background menu (the only way back for a Rail side) "
                        "opens from the keyboard, with the hidden activity Source unchecked"):
        run.key("Shift+F10")
    with run.act("Down, Enter on Source",
                 [in_tree(role="page tab", name="Source")],
                 should="the activity comes back"):
        run.key("Down", "Return")


def options_menu(run):
    """Explorer's `⋮` options menu: open, walk, the Move to side submenu,
    close (`dock_options_menu`, multi-pane branch)."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"),
              [focused(role="push button", name="More actions"), said_any("Explorer")],
              should="the reader hears which dock these options are for (the button's "
                     "access_label is 'More actions: Explorer')")
    with run.act("Enter on the options button",
                 [in_tree(role="menu item", name="Move to new activity"),
                  said_any("Move to new activity", "menu")],
                 should="the options menu opens and the reader hears it"):
        run.key("Return")
    with run.act("Down", [said_any("Move to new activity")],
                 should="the reader hears the first item"):
        run.key("Down")
    with run.act("Down again", [said_any("Move to side")],
                 should="the reader hears the Move to side submenu item"):
        run.key("Down")
    with run.act("Right: open the submenu",
                 [in_tree(role="menu item", name="Trailing"), said_any("Trailing", "Top",
                                                                       "Bottom")],
                 should="the submenu opens and the reader hears its first side"):
        run.key("Right")
    with run.act("Escape", [spoke_anything()],
                 should="the submenu (or, if it already closed itself, the menu) closes and "
                        "the reader hears where they are"):
        run.key("Escape")


def promote_dock(run):
    """Explorer's options → Move to new activity: the keyboard alternative to
    dragging a pane out into its own activity. The model change rebuilds the
    whole layout (`DockingLayout::build`, `version()` bound at Rebuild)."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"),
              [focused(role="push button", name="More actions")])
    with run.act("Enter on the options button",
                 [in_tree(role="menu item", name="Move to new activity")],
                 should="the options menu opens"):
        run.key("Return")
    with run.act("Down, Enter: Move to new activity",
                 [focus_kept_or_moved_to_named(), spoke_anything()],
                 should="Explorer becomes its own activity; focus lands on it (its rail "
                        "tab or its header) and the reader hears where it went"):
        run.key("Down", "Return")
    run.snapshot("after Move to new activity")
    with run.act("Tab", [spoke_anything()],
                 should="Tab from wherever focus is reaches a control the reader hears"):
        run.key("Tab")


def move_tab_to_side(run):
    """The Properties strip tab's context menu → Move to ▸ Leading: the
    keyboard alternative to dragging a tab to another side."""
    run.wait_for(**PROPERTIES_TAB)
    focus_act(run, "the Properties tab", PROPERTIES_TAB,
              [focused(role="page tab", name="Properties")])
    with run.act("Shift+F10 on the Properties tab",
                 [in_tree(role="menu item", name="Move to"), spoke_anything()],
                 should="the tab's context menu opens"):
        run.key("Shift+F10")
    with run.act("Down, Down: Move to", [said_any("Move to")],
                 should="the reader hears Move to"):
        run.key("Down", "Down")
    with run.act("Right: the side submenu",
                 [in_tree(role="menu item", name="Leading"), said_any("Leading")],
                 should="the submenu opens, stays open, and the reader hears Leading"):
        run.key("Right")
    with run.act("Escape", should="close whatever is open"):
        run.key("Escape")
    with run.act("Escape", should="close whatever is open"):
        run.key("Escape")
    # The submenu closes itself (see the act above), so a reader cannot pick a
    # side at a human pace. Reopen and go through it at 40 ms a key, faster
    # than any person, only to learn where focus goes after the move.
    focus_act(run, "the Properties tab again", PROPERTIES_TAB)
    with run.act("Shift+F10, Down, Down, Right, Down, Enter at 40 ms a key",
                 [in_tree(role="page tab", name="Properties"), focus_kept_or_moved_to_named(),
                  said_any("Properties")],
                 should="Properties moves to the leading side; focus follows it and the "
                        "reader hears it"):
        run.key("Shift+F10", "Down", "Down", "Right", "Down", "Return", gap=0.04)
    run.snapshot("after Move to Leading")
    with run.act("Tab", [spoke_anything()],
                 should="Tab reaches a control the reader hears"):
        run.key("Tab")


def hide_side_with_focus_inside(run):
    """Focus is on the Explorer header when the leading side is hidden from
    outside (a screen reader's activation of Toggle Sidebar, which leaves
    focus where it is)."""
    run.wait_for(**EXPLORER)
    focus_act(run, "the Explorer dock header", EXPLORER,
              [focused(role="push button", name="Explorer")])
    with run.act("AT-SPI click on Toggle Sidebar while Explorer has focus",
                 [not_in_tree(role="landmark", name="Leading panel"),
                  focus_kept_or_moved_to_named(), spoke_anything()],
                 should="the side hides; focus, which was inside it, moves to a live named "
                        "control and the reader hears where it now is"):
        run.action("click", role="push button", name="Toggle Sidebar")
    with run.act("Tab", [spoke_anything()],
                 should="Tab from wherever focus is reaches a control the reader hears"):
        run.key("Tab")


def hide_dock_from_its_options(run):
    """Properties' own `⋮` → Hide "Properties" (sole-pane branch of
    `dock_options_menu`): focus is on the button that disappears with the
    dock. Then the strip's "Hidden activities" button is the way back."""
    run.wait_for(**more_actions("properties"))
    focus_act(run, "Properties' options button", more_actions("properties"),
              [focused(role="push button", name="More actions")])
    with run.act("Enter on the options button",
                 [in_tree(role="menu item", name_contains="Hide")],
                 should="the options menu opens"):
        run.key("Return")
    with run.act("Down, Enter: Hide \"Properties\"",
                 [not_in_tree(role="page tab", name="Properties"),
                  focus_kept_or_moved_to_named(), spoke_anything()],
                 should="the dock hides; focus lands on a named control (the Hidden "
                        "activities button that brings it back) and the reader hears it"):
        run.key("Down", "Return")
    run.snapshot("after Hide Properties")
    with run.act("Tab", [spoke_anything()],
                 should="Tab reaches a control the reader hears"):
        run.key("Tab")


def toolbar_toggles(run):
    """The example's own Toggle Panel button: a reader who presses it should
    learn whether the panel is now shown."""
    run.wait_for(role="push button", name="Toggle Panel")
    focus_act(run, "Toggle Panel", {"role": "push button", "name": "Toggle Panel"},
              [focused(role="push button", name="Toggle Panel")])
    with run.act("Space on Toggle Panel",
                 [not_in_tree(role="landmark", name="Bottom panel"),
                  said_any("not pressed", "collapsed", "hidden")],
                 should="the bottom panel hides and the reader hears the new state"):
        run.key("space")
    with run.act("Space again",
                 [in_tree(role="landmark", name="Bottom panel"),
                  said_any("pressed", "expanded", "shown")],
                 should="the bottom panel comes back and the reader hears the new state"):
        run.key("space")


def lock_layout(run):
    """The toolbar's Lock Layout (`DockingModel::set_policy`) and Restore
    (`import_state`) are structural model changes: `DockingLayout` rebuilds
    every side (`docking.rs` `build`, `destroy_subtree` on each side), while
    focus stays on the toolbar button, outside the layout."""
    lock = {"role": "push button", "name": "Lock Layout"}
    run.wait_for(**lock)
    focus_act(run, "Lock Layout", lock, [focused(role="push button", name="Lock Layout")])
    with run.act("Space on Lock Layout",
                 [no_focus_change(), not_said("page tab"),
                  custom("Orca's last word is not about another control",
                         lambda act: ((not utterances(act.orca))
                                      or "page tab" not in utterances(act.orca)[-1].text,
                                      [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                                       for u in utterances(act.orca)] or ["nothing"]),
                         needs_orca=True)],
                 should="the layout locks; focus stays on the button and the reader hears "
                        "nothing about tabs they did not touch"):
        run.key("space")
    with run.act("Space on Lock Layout again (unlock)",
                 [no_focus_change(), not_said("page tab")],
                 should="the layout unlocks; the reader hears nothing about other tabs"):
        run.key("space")
    export = {"role": "push button", "name": "Export"}
    focus_act(run, "Export", export)
    with run.act("Space on Export", should="the layout is exported (status text changes)"):
        run.key("space")
    focus_act(run, "Restore", {"role": "push button", "name": "Restore"})
    with run.act("Space on Restore",
                 [no_focus_change(), not_said("page tab")],
                 should="the layout is restored; the reader hears nothing about other tabs"):
        run.key("space")


def no_defunct_drop() -> Check:
    """Orca did not ignore any event of the act as coming from a defunct
    object."""
    def run(act):
        drops = [f"{l.stamp} {l.text}" for l in act.orca if l.is_defunct_drop]
        return not drops, drops or ["Orca ignored nothing as defunct"]
    return custom("Orca ignores no event of the act as defunct", run, needs_orca=True)


def reshown_events(run):
    """Events from a side's content after the side was hidden and shown
    again. Hiding parks the `DockSidePanel` dormant, so the adapter removes
    its nodes and marks each defunct (`accesskit_atspi_common` adapter.rs
    `remove_node`); showing re-adds the same ids. First a reference: the
    bottom side's pane divider answers an arrow key before any hide."""
    divider = {"role": "separator", "name": "Splitter divider", "nth": 1}
    toggle = {"role": "push button", "name": "Toggle Panel"}
    run.wait_for(**divider)
    focus_act(run, "the Terminal|Problems divider", divider,
              [focused(role="separator", name="Splitter divider")])
    with run.act("Right on the divider (before any hide)",
                 [event("object:property-change:accessible-value", role="separator"),
                  spoke_anything(), no_defunct_drop()],
                 should="the divider moves and Orca answers the value change"):
        run.key("Right")
    focus_act(run, "Toggle Panel", toggle)
    with run.act("Space on Toggle Panel (hide the bottom side)",
                 [not_in_tree(role="landmark", name="Bottom panel")],
                 should="the bottom side hides"):
        run.key("space")
    with run.act("Space on Toggle Panel (show it again)",
                 [in_tree(role="landmark", name="Bottom panel"), no_defunct_drop()],
                 should="the bottom side comes back, alive to the reader"):
        run.key("space")
    focus_act(run, "the Terminal|Problems divider again", divider,
              [focused(role="separator", name="Splitter divider"), said("splitter")])
    with run.act("Right on the divider (after hide and show)",
                 [event("object:property-change:accessible-value", role="separator"),
                  spoke_anything(), no_defunct_drop()],
                 should="the divider moves and Orca answers the value change, as before the "
                        "hide"):
        run.key("Right")
    with run.act("Tab to the Problems header, Space (collapse it)",
                 [no_defunct_drop()],
                 should="the Problems pane collapses; nothing the reader could be told is "
                        "dropped as defunct"):
        run.key("Tab")
        run.wait(0.8)
        run.key("space")


def rail_arrows(run):
    """After Explorer is promoted to its own activity the leading rail has two
    tabs, Source and Explorer: arrows move between them (roving focus,
    `DockRailItem` on_key), Enter activates one."""
    run.wait_for(**more_actions("explorer"))
    # Set the scene inside an act of its own: speech from steps outside any act
    # is credited by the harness to the next act.
    with run.act("setup: Explorer's options, Move to new activity",
                 [in_tree(role="page tab", name="Explorer")],
                 should="Explorer becomes a second rail activity, with focus on its tab"):
        run.grab_focus(**more_actions("explorer"))
        run.wait(0.8)
        run.key("Return")
        run.wait(0.8)
        run.key("Down", "Return")
    # Move to new activity leaves focus on the new Explorer rail tab (see
    # docking-promote), so the walk starts there.
    run.wait_for(role="page tab", name="Explorer", state="focused")
    with run.act("Up arrow", [focused(role="page tab", name="Source"), said("Source")],
                 should="focus moves to the Source rail tab and the reader hears it (and, "
                        "ideally, 1 of 2)"):
        run.key("Up")
    with run.act("Enter on Source",
                 [event("object:state-changed:selected", role="page tab", detail1=1),
                  said_any("selected", "Search")],
                 should="Source becomes the shown activity; the reader hears it selected or "
                        "hears the content that replaced Explorer"):
        run.key("Return")
    with run.act("Down arrow", [focused(role="page tab", name="Explorer"), said("Explorer")],
                 should="focus moves back to the Explorer rail tab"):
        run.key("Down")


def reshown_visited(run):
    """Orca meets the Explorer header, the leading side is hidden and shown
    from its rail tab, then Shift+Tab walks back through the side: the nodes
    Orca never met before the hide against the one it did."""
    run.wait_for(**EXPLORER)
    focus_act(run, "the Explorer dock header", EXPLORER,
              [focused(role="push button", name="Explorer"), said("Explorer")])
    focus_act(run, "the Source rail tab", RAIL_TAB, [focused(role="page tab", name="Source")])
    with run.act("Enter: hide the leading side",
                 [not_in_tree(role="landmark", name="Leading panel")], should="the side hides"):
        run.key("Return")
    with run.act("Enter: show it again", [in_tree(role="landmark", name="Leading panel")],
                 should="the side comes back"):
        run.key("Return")
    walk = [("More actions", "Search's options button, which Orca never met"),
            ("Search", "the Search header, never met"),
            ("Splitter divider", "the pane divider, never met"),
            ("More actions", "Explorer's options button, never met"),
            ("New File", "Explorer's first header action, never met"),
            ("Explorer", "the Explorer header, which Orca met before the hide")]
    for name, what in walk:
        with run.act(f"Shift+Tab to {name}", [focused(name=name), said(name), no_defunct_drop()],
                     should=f"focus moves to {what}, and the reader hears it"):
            run.key("Shift+Tab")


SCENARIOS = [
    Scenario("docking-rail-toggle", PKG, rail_toggle,
             "the active rail tab hides and shows its side, by Enter and Space"),
    Scenario("docking-handle-keys", PKG, handle_keys,
             "the leading resize handle: arrows, Home, End, Enter"),
    Scenario("docking-accordion", PKG, accordion_header,
             "a dock's accordion header collapses and expands its pane"),
    Scenario("docking-reshown-content", PKG, reshown_content,
             "hide and show the leading side, then walk back into its content"),
    Scenario("docking-rail-menu", PKG, rail_menu,
             "the rail tab's context menu from Shift+F10 and the Menu key"),
    Scenario("docking-hide-activity", PKG, hide_activity,
             "hide the rail's only activity from its menu, then restore it by keyboard"),
    Scenario("docking-options-menu", PKG, options_menu,
             "Explorer's options menu: open, walk, submenu, close"),
    Scenario("docking-promote", PKG, promote_dock,
             "Explorer's options -> Move to new activity, and where focus goes"),
    Scenario("docking-move-tab", PKG, move_tab_to_side,
             "the Properties tab's menu -> Move to -> Leading, and where focus goes"),
    Scenario("docking-hide-focused-side", PKG, hide_side_with_focus_inside,
             "hide the leading side while focus is inside it"),
    Scenario("docking-hide-dock", PKG, hide_dock_from_its_options,
             "hide Properties from its own options menu, and where focus goes"),
    Scenario("docking-toolbar-toggles", PKG, toolbar_toggles,
             "the example's Toggle Panel button and what it tells a reader"),
    Scenario("docking-lock-layout", PKG, lock_layout,
             "Lock Layout / Restore rebuild every side while focus is on the toolbar"),
    Scenario("docking-reshown-events", PKG, reshown_events,
             "a pane divider's value change before and after its side was hidden and shown"),
    Scenario("docking-rail-arrows", PKG, rail_arrows,
             "arrow keys and Enter across two rail tabs"),
    Scenario("docking-reshown-visited", PKG, reshown_visited,
             "after hide and show, controls Orca met before the hide against ones it did not"),
]
