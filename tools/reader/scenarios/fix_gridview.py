# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""grid-view, after the fix of the sweep's gridview-01, -02 and -03: what a
reader gets now, act by act, with nothing else in the way.

* `fix-gridview-count`: a change of how many tiles are selected, with the
  cursor staying put. The count is said in words (it was the message id,
  gridview-01), in full, and the tile under the cursor keeps its node, so no
  focus event follows the count to cut it, and the tile's selected state
  changes where the reader is (gridview-02).
* `fix-gridview-at-focus`: a screen reader's focus request on a tile. A tile
  is reached through the grid's active descendant and offers no focus of its
  own, so the request moves nothing: the reader's focus and the grid's cursor
  stay together, and Enter and the arrows act from the tile the reader was
  told about (gridview-03). A click on a tile, which a reader's activation
  sends, chooses it and moves the cursor there.
"""

from reader_lib.checks import event, no_event
from reader_lib.scenario import Scenario
from scenarios.gridview import (AppLog, cell_path, cell_state, count_said, focus_grid,
                                focus_names_tile, no_focus_move, no_message_id, tile_focus)


def selected_event(present: bool):
    """The bus carries the change of a table cell's selected state."""
    return event("object:state-changed:selected", role="table cell",
                 detail1=1 if present else 0)


def count(run):
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0),
                 should="the cursor lands on the first tile, nothing selected"):
        run.key("Ctrl+Right")
    with run.act("Space: select it",
                 count_said(1) + [no_message_id(), no_focus_move(), selected_event(True),
                                  cell_state(0, "selected")],
                 should="the tile becomes selected where the reader is; focus stays, and "
                        "the reader hears '1 item selected' in full", tree=True):
        run.key("space")
    with run.act("Space: unselect it",
                 count_said(0) + [no_focus_move(), selected_event(False),
                                  cell_state(0, "selected", present=False)],
                 should="the tile is unselected; focus stays; 'No item selected'", tree=True):
        run.key("space")
    with run.act("Ctrl+A: select all",
                 count_said(60) + [no_focus_move(), focus_names_tile(0)],
                 should="every tile is selected; focus stays on 'Sunset 1'; "
                        "'60 items selected'", tree=True):
        run.key("Ctrl+A")
    with run.act("Ctrl+Shift+A: select none", count_said(0) + [no_focus_move()],
                 should="the selection is cleared; focus stays; 'No item selected'"):
        run.key("Ctrl+Shift+A")


def at_focus(run):
    # The cursor moves with Ctrl+arrows, which leave the selection alone, so
    # no count is said ahead of a focus change in any act: what is judged is
    # the focus alone.
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0),
                 should="the cursor lands on the first tile"):
        run.key("Ctrl+Right")
    with run.act("Ctrl+Space: select it", count_said(1) + [no_focus_move()],
                 should="'Sunset 1' is selected; focus stays; '1 item selected'"):
        run.key("Ctrl+space")
    trail = cell_path(run, 2)
    with run.act("AT-SPI grab_focus on 'Trail 3'",
                 [no_focus_move(), focus_names_tile(0),
                  cell_state(2, "focusable", present=False)],
                 should="a tile offers no focus of its own, so the request moves nothing: "
                        "the reader's focus and the grid's cursor stay on 'Sunset 1'",
                 tree=True):
        run.grab_focus(path=trail)
    log = AppLog(run)
    with run.act("Enter", [log.activated("enter", 0),
                           no_event("object:state-changed:focused", role="frame")],
                 should="Enter opens the tile the reader is on, 'Sunset 1'"):
        log.begin()
        run.key("Return")
        run.wait(0.5)
        log.end("enter")
    with run.act("Ctrl+Right from 'Sunset 1'", tile_focus(1, [focus_names_tile(1)]),
                 should="the cursor moves on from the tile the reader is on: 'Harbor 2'",
                 tree=True):
        run.key("Ctrl+Right")
    with run.act("Ctrl+Right again", tile_focus(2, [focus_names_tile(2)]),
                 should="'Trail 3'; the keys still reach the grid", tree=True):
        run.key("Ctrl+Right")
    picnic = cell_path(run, 3)
    with run.act("AT-SPI click on 'Picnic 4'", tile_focus(3, [cell_state(3, "selected")]),
                 should="a reader's activation of a tile chooses it: the cursor and the "
                        "selection move there (still one tile, so no count) and the reader "
                        "hears it", tree=True):
        run.action("click", path=picnic)
    with run.act("Enter on 'Picnic 4'", [log.activated("enter-picnic", 3)],
                 should="Enter opens the tile the reader chose"):
        log.begin()
        run.key("Return")
        run.wait(0.5)
        log.end("enter-picnic")


SCENARIOS = [
    Scenario("fix-gridview-count", "grid-view", count,
             "a selection count with the cursor staying put: said in words, in full, "
             "with the tile's selected state changing in place"),
    Scenario("fix-gridview-at-focus", "grid-view", at_focus,
             "a screen reader's focus request on a tile moves nothing; Enter and the "
             "arrows act from the tile the reader is on; a click chooses a tile"),
]
