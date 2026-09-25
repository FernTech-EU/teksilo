# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""What a reader gets from a menu worked by keyboard, once `MenuList` names
its highlighted row as its active descendant, a menu is named after what
opened it, each row carries its place in the menu, and a submenu the keyboard
opened is dismissed the way a keyboard user ends things.

`menus.py` and `data_collections.py` hold the sweep's scenarios, which found the
defects; these state what the fix should give, act by act. The menu opens
with nothing highlighted, as it always has: the first Down reaches the first
row.
"""

from __future__ import annotations

from reader_lib.checks import focused, in_tree, said, announced
from reader_lib.scenario import Scenario

from scenarios.data_collections import enter_list
from scenarios.docking import focus_act, more_actions
from scenarios.menus import (attr_is, focus_combo, has_state, lacks_state,
                             menus_open, printed, spoke_anything)

MENUS = "menus-and-dropdowns"
COLLECTIONS = "data-collections"
DOCKING = "docking"


def menubar(run):
    """F10, Down into File, the arrows through it, and back out."""
    run.wait_for(role="combo box", nth=1)
    focus_combo(run, "fruit")
    with run.act("F10", [focused(role="menu item", name="File"), said("File")],
                 should="focus on the File menu bar item"):
        run.key("F10")
    with run.act("Down opens File",
                 [focused(role="menu", name="File"), said("File menu"),
                  in_tree(role="menu", name="File"),
                  attr_is("menu item", "New", "posinset", "1"),
                  attr_is("menu item", "Recent", "posinset", "4"),
                  attr_is("menu item", "Quit", "posinset", "5"),
                  attr_is("menu item", "Quit", "setsize", "5")],
                 should="the File menu opens and the reader hears 'File menu'; each "
                 "row says where it stands among the menu's five", tree=True):
        run.key("Down")
    for key, row in [("Down", "New"), ("Down", "Open"), ("Down", "Save"),
                     ("End", "Quit"), ("Home", "New"), ("Up", "Quit")]:
        with run.act(f"{key}: {row}", [focused(role="menu item", name=row), said(row)],
                     should=f"the highlight moves to {row} and the reader hears it"):
            run.key(key)
    with run.act("Escape closes File", [focused(role="menu item", name="File"),
                                        said("File")],
                 should="the menu closes and the reader is back on File"):
        run.key("Escape")


def view_items(run):
    """The View menu's check, tri-state and radio rows, heard as the highlight
    reaches each."""
    run.wait_for(role="combo box", nth=1)
    focus_combo(run, "fruit")
    radio = "radio menu item"
    with run.act("Alt+V opens View",
                 [focused(role="menu", name="View"), said("View menu"),
                  has_state("check menu item", "Word Wrap", "checked"),
                  has_state("check menu item", "Show Inspector", "indeterminate"),
                  has_state(radio, "Light Theme", "checked"),
                  lacks_state(radio, "Dark Theme", "checked"),
                  attr_is(radio, "Dark Theme", "posinset", "4"),
                  attr_is(radio, "Dark Theme", "setsize", "6")],
                 should="the View menu opens, named; its rows carry their states and "
                 "their places", tree=True):
        run.key("Alt+V")
    for row, extra in [("Word Wrap", "checked"), ("Show Inspector", "partially checked"),
                       ("Light Theme", "selected"), ("Dark Theme", "not selected")]:
        with run.act(f"Down: {row}",
                     [focused(name=row), said(row), said(extra)],
                     should=f"the reader hears {row!r}, {extra!r}"):
            run.key("Down")
    with run.act("Escape", [focused(role="menu item", name="View")],
                 should="the menu closes"):
        run.key("Escape")


def _to_recent(run):
    run.wait_for(role="combo box", nth=1)
    focus_combo(run, "fruit")
    with run.act("Alt+F", [focused(role="menu", name="File")], should="File opens",
                 tree=True):
        run.key("Alt+F")
    with run.act("Down x4: Recent", [focused(role="menu item", name="Recent"),
                                     said("Recent")],
                 should="the highlight is on Recent and the reader hears it"):
        run.key("Down", "Down", "Down", "Down")


def submenu(run):
    """File > Recent opened by Right: it stays open, and Enter on a file opens
    the file. Every menu here is opened once: a menu opened a second time
    comes back under node ids AT-SPI already declared defunct, which is a
    defect of its own."""
    _to_recent(run)
    with run.act("Right opens Recent",
                 [menus_open(2), focused(role="menu", name="Recent"), said("Recent menu")],
                 should="the submenu opens with focus in it and the reader hears "
                 "'Recent menu'", tree=True, record=2.5):
        run.key("Right")
    with run.act("2.5 s later", [menus_open(2)],
                 should="the submenu is still open", tree=True):
        run.wait(0.1)
    with run.act("Down: project-alpha.toml",
                 [focused(role="menu item", name="project-alpha.toml"),
                  said("project-alpha.toml")],
                 should="the reader hears the first file"):
        run.key("Down")
    with run.act("Down: notes.md",
                 [focused(role="menu item", name="notes.md"), said("notes.md")],
                 should="the reader hears notes.md"):
        run.key("Down")
    log = printed(run)
    with run.act("Enter on notes.md", [log.check("Recent: notes"), menus_open(0)],
                 should="the file opens and every menu closes", tree=True):
        run.key("Return")
    log.done()


def submenu_enter(run):
    """File > Recent opened by Enter: it stays open, and Left closes it."""
    _to_recent(run)
    with run.act("Enter opens Recent",
                 [menus_open(2), focused(role="menu", name="Recent"), said("Recent menu")],
                 should="the submenu opens with focus in it and the reader hears "
                 "'Recent menu'", tree=True, record=2.5):
        run.key("Return")
    with run.act("2.5 s later", [menus_open(2)],
                 should="the submenu is still open", tree=True):
        run.wait(0.1)
    with run.act("Down: project-alpha.toml",
                 [focused(role="menu item", name="project-alpha.toml"),
                  said("project-alpha.toml")],
                 should="the reader hears the first file"):
        run.key("Down")
    with run.act("Left closes Recent",
                 [menus_open(1), focused(role="menu item", name="Recent"), said("Recent")],
                 should="the submenu closes and the reader is back on Recent", tree=True):
        run.key("Left")


def context_menu(run):
    """A ListView row's context menu, from the keyboard."""
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: first row", [focused(role="list item", name="Item 1")],
                 should="the cursor on row 1"):
        run.key("Down")
    with run.act("Shift+F10: the row's menu", [focused(role="menu"), said("menu")],
                 should="the row's context menu opens with focus in it", tree=True):
        run.key("Shift+F10")
    for key, row in [("Down", "Move Down"), ("Down", "Move to Bottom"), ("Up", "Move Down")]:
        with run.act(f"{key}: {row}", [focused(role="menu item", name=row), said(row)],
                     should=f"the highlight moves to {row} and the reader hears it"):
            run.key(key)
    with run.act("Enter on Move Down",
                 [announced("Moved to 2 of 200"), focused(role="list item", name="Item 1")],
                 should="the command the reader heard runs, and focus is back on the row",
                 tree=True):
        run.key("Return")


def docking_submenu(run):
    """A dock's options menu and its Move to side submenu, by keyboard: the
    keyboard alternative to dragging a dock to another side."""
    run.wait_for(**more_actions("explorer"))
    focus_act(run, "Explorer's options button", more_actions("explorer"),
              [focused(role="push button", name="More actions")])
    with run.act("Enter on the options button", [focused(role="menu")],
                 should="the options menu opens with focus in it", tree=True):
        run.key("Return")
    with run.act("Down: Move to new activity",
                 [focused(role="menu item", name="Move to new activity"),
                  said("Move to new activity")],
                 should="the reader hears the first item"):
        run.key("Down")
    with run.act("Down: Move to side",
                 [focused(role="menu item", name="Move to side"), said("Move to side")],
                 should="the reader hears the submenu item"):
        run.key("Down")
    with run.act("Right opens the side submenu",
                 [menus_open(2), focused(role="menu", name="Move to side"),
                  said("Move to side menu")],
                 should="the submenu opens with focus in it and the reader hears it",
                 tree=True, record=2.5):
        run.key("Right")
    with run.act("2.5 s later", [menus_open(2)], should="the submenu is still open",
                 tree=True):
        run.wait(0.1)
    with run.act("Down: the first side", [focused(role="menu item"), spoke_anything()],
                 should="the reader hears the first side the dock can move to"):
        run.key("Down")


SCENARIOS = [
    Scenario("fix-menus-menubar", MENUS, menubar,
             "F10 and the arrows through the File menu"),
    Scenario("fix-menus-view-items", MENUS, view_items,
             "the View menu's check, tri-state and radio rows"),
    Scenario("fix-menus-submenu", MENUS, submenu,
             "File > Recent by Right, and a file opened from it"),
    Scenario("fix-menus-submenu-enter", MENUS, submenu_enter,
             "File > Recent by Enter, and Left back out of it"),
    Scenario("fix-menus-context", COLLECTIONS, context_menu,
             "a ListView row's context menu by Shift+F10"),
    Scenario("fix-menus-docking-submenu", DOCKING, docking_submenu,
             "a dock's options menu and its Move to side submenu"),
]
