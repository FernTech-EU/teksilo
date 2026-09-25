# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""grid-view, the verifier's own acts: what the sweep's scenarios
(`gridview.py`) did not press, to settle or extend its findings.

* `verify-gridview-first-count`: the first announcement of a session is the
  count said by Space on the tile the cursor is already on. K2 drops most
  later messages, so the first is the one that can be heard, and with no
  cursor move the only focus change in the act is the pane rebuild's.
* `verify-gridview-type-ahead-start`: type-ahead with no cursor yet, and a
  growing prefix from the first tile.
* `verify-gridview-reorder-multi`: Alt+Right with three tiles selected.
* `verify-gridview-at-focus-keys`: a screen reader's focus on a tile, then
  Space and Tab, the keys a reader presses next.
* `verify-gridview-menu-escape`: the tile context menu, closed with Escape,
  and opened with Shift+F10.
"""

from reader_lib.checks import announced, custom, focused, in_tree, said
from reader_lib.scenario import Scenario
from scenarios.gridview import (CAPTIONS, cell_path, cell_state, count_said, focus_grid,
                                focus_is_grid, focus_names_tile, no_focus_move, tile_focus)


def status_is(text: str):
    """After the act, the example's status line reads `text`."""
    return in_tree(role="label", name=text)


# ---------------------------------------------------------------------------


def first_count(run):
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0),
                 should="the cursor lands on the first tile, nothing selected, nothing said "
                        "but the tile"):
        run.key("Ctrl+Right")
    with run.act("Space: select it (the session's first message)",
                 count_said(1) + [no_focus_move(), cell_state(0, "selected")],
                 should="the tile becomes selected; focus stays; the reader hears the count "
                        "in full", tree=True):
        run.key("space")
    with run.act("Space: unselect it", count_said(0) + [no_focus_move()],
                 should="the tile is unselected; focus stays; 'No item selected'"):
        run.key("space")


def type_ahead_start(run):
    focus_grid(run)
    with run.act("type 's' with no cursor yet", tile_focus(0, [focus_names_tile(0)]),
                 should="the first caption that starts with s is the first tile, 'Sunset 1'; "
                        "with no cursor yet, a search starts at the top", tree=True):
        run.type("s")
    with run.act("Home: 'Sunset 1'", tile_focus(0), should="the first tile"):
        run.key("Home")
    with run.act("type 'su' quickly from 'Sunset 1'",
                 [focused(role="table cell"), said(CAPTIONS[4]), focus_names_tile(4)],
                 should="'s' goes on to the next s, 'Summit 5'; 'su' still matches 'Summit 5', "
                        "so the cursor stays there", tree=True):
        run.type("su")


def reorder_multi(run):
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1'", tile_focus(0), should="cursor only"):
        run.key("Ctrl+Right")
    for n in range(3):
        with run.act(f"Ctrl+Space: add {CAPTIONS[n]!r}", count_said(n + 1),
                     should=f"{n + 1} selected"):
            run.key("Ctrl+space")
        if n < 2:
            with run.act(f"Ctrl+Right: {CAPTIONS[n + 1]!r}", tile_focus(n + 1),
                         should="cursor only"):
                run.key("Ctrl+Right")
    with run.act("check: three selected", [status_is("3 selected")], tree=True,
                 should="the status line says three are selected", record=0.5):
        run.wait(0.1)
    with run.act("Alt+Right: move 'Trail 3' one on",
                 [announced("Trail 3 moved to 4 of 60"), status_is("3 selected")],
                 should="the focused tile moves one place on and the three tiles stay "
                        "selected (or the reader is told the selection changed)", tree=True):
        run.key("Alt+Right")


def at_focus_keys(run):
    focus_grid(run)
    with run.act("Right: 'Sunset 1', selected", tile_focus(0), should="'Sunset 1'"):
        run.key("Right")
    trail = cell_path(run, 2)
    with run.act("AT-SPI grab_focus on 'Trail 3'", tile_focus(2),
                 should="the cursor moves to 'Trail 3'", tree=True):
        run.grab_focus(path=trail)
    with run.act("Space on the focused tile", [status_is("2 selected"),
                                               cell_state(2, "selected")],
                 should="Space toggles the tile focus is on, 'Trail 3': two tiles selected",
                 tree=True):
        run.key("space")
    with run.act("Tab", [focus_is_grid()],
                 should="Tab leaves the tile for the next stop; the grid is the only one, so "
                        "focus comes back to the grid", tree=True):
        run.key("Tab")
    with run.act("Right after Tab", [focused(role="table cell")],
                 should="the arrows work again", tree=True):
        run.key("Right")


def menu_escape(run):
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0), should="cursor only"):
        run.key("Ctrl+Right")
    with run.act("Menu key", [focused(role="menu")], should="the tile's menu opens",
                 tree=True):
        run.key("Menu")
    with run.act("Escape: close the menu", tile_focus(0),
                 should="the menu closes and focus returns to the tile, which the reader hears"):
        run.key("Escape")
    with run.act("Right after the menu", tile_focus(1),
                 should="the arrows work again: 'Harbor 2'"):
        run.key("Right")
    with run.act("Shift+F10", [focused(role="menu")],
                 should="Shift+F10 opens the tile's context menu too"):
        run.key("Shift+F10")
    with run.act("Escape again", tile_focus(1), should="back on 'Harbor 2'"):
        run.key("Escape")


def alt_no_cursor(run):
    focus_grid(run)
    with run.act("Alt+Right with no cursor yet",
                 [custom("nothing moves before the reader has a tile",
                         lambda act: (not any(e["type"] == "object:announcement"
                                              and "moved" in (e.get("text") or "")
                                              for e in act.events),
                                      [e.get("text") for e in act.events
                                       if e["type"] == "object:announcement"]))],
                 should="with no cursor, there is no tile the reader chose to move"):
        run.key("Alt+Right")


SCENARIOS = [
    Scenario("verify-gridview-first-count", "grid-view", first_count,
             "Space on the tile under the cursor: the session's first message"),
    Scenario("verify-gridview-type-ahead-start", "grid-view", type_ahead_start,
             "type-ahead with no cursor, and a growing prefix from the first tile"),
    Scenario("verify-gridview-reorder-multi", "grid-view", reorder_multi,
             "Alt+Right with three tiles selected"),
    Scenario("verify-gridview-at-focus-keys", "grid-view", at_focus_keys,
             "AT focus on a tile, then Space, Tab, Right"),
    Scenario("verify-gridview-menu-escape", "grid-view", menu_escape,
             "the tile menu closed with Escape; Shift+F10"),
    Scenario("verify-gridview-alt-no-cursor", "grid-view", alt_no_cursor,
             "Alt+Right before the cursor is anywhere"),
]
