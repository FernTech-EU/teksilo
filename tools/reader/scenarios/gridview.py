# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""grid-view: a sectioned, multi-selection, reorderable GridView of 60 photo
tiles (four albums of fifteen, five columns at the example's 900 px width).

What a reader should get from it (`crates/teksilo-widgets/src/grid_view.rs`,
`grid_view/keyboard.rs`, `grid_view/a11y.rs`, `grid_view/body_pane.rs`):

* one Tab stop, the grid (`Role::Grid`, named "Photo library"), with the
  cursor carried by `active_descendant` onto `Role::GridCell` tiles;
* each move says the tile it lands on (arrows, Home/End, PageUp/Down,
  type-ahead), and whether it is selected;
* a change in how many tiles are selected is said once, through the tree's
  announcer (`grid_view/selection_count.rs`, `ctx.announce`: K2 on main);
* Alt+arrow moves a tile and says where it went (`ctx.announce`: K2 too), and
  so does the tile's context menu (`common/ordered_move.rs`);
* the section a tile belongs to (`Role::RowHeader` per album).

The captions follow `examples/grid_view/src/main.rs` `make_photos`. The
example installs no `I18nConfig`, which matters for every framework string
(`tr_widget!`) it shows: `resolve_message_widget` answers the message id when
no manager is installed (`crates/teksilo-i18n/src/resolve.rs:22-24`).
"""

from reader_lib.checks import (announced, custom, focused, no_event, not_announced,
                               not_said, said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

WORDS = ["Sunset", "Harbor", "Trail", "Picnic", "Summit", "Garden", "Market", "Bridge",
         "Cabin", "Meadow", "Canyon", "Festival", "Skyline", "Lantern", "Orchard", "Pier"]
COUNT = 60


def caption(index: int) -> str:
    a, i = divmod(index, 15)
    return f"{WORDS[(a * 4 + i) % len(WORDS)]} {i + 1}"


CAPTIONS = [caption(i) for i in range(COUNT)]


def count_words(n: int) -> str:
    """`grid-view-selection-count` in `crates/teksilo-widgets/locales/en-US.ftl`."""
    if n == 0:
        return "No item selected"
    return "1 item selected" if n == 1 else f"{n} items selected"


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def _cells(tree):
    """(caption, node) for every table cell in `tree`, the caption from its
    label child (the cell itself carries no name)."""
    from reader_lib.checks import _walk
    found = []
    for node in _walk(tree):
        if node.get("role") != "table cell":
            continue
        label = next((c.get("name") for c in node.get("children", [])
                      if c.get("role") == "label"), None)
        found.append((label, node))
    return found


def tile_said(index: int):
    """Orca says the caption of tile `index` in the act, uncut."""
    return said(CAPTIONS[index])


def tile_focus(index: int, extra: list | None = None) -> list:
    """Focus lands on a table cell and Orca says tile `index`'s caption."""
    return [focused(role="table cell"), tile_said(index)] + list(extra or [])


def count_said(n: int) -> list:
    """The new selection count reaches the bus in words and Orca says it."""
    return [announced(count_words(n)), said(count_words(n))]


def no_message_id():
    """No announcement carries a raw Fluent message id."""
    return not_announced("grid-view-")


def cell_state(index: int, state: str, present: bool = True):
    """After the act, the cell holding tile `index` has (or lacks) `state`."""
    def run(act):
        for label, node in _cells(act.tree):
            if label == CAPTIONS[index]:
                has = state in node.get("states", [])
                return has == present, [f"[table cell] of {label!r} states={node.get('states')}"]
        return False, [f"no realized cell for {CAPTIONS[index]!r} after the act"]
    return custom(f"the cell of {CAPTIONS[index]!r} {'has' if present else 'lacks'} "
                  f"state {state!r}", run, needs_tree=True)


def said_any_of(words: list[str], describe: str):
    def run(act):
        spoken = [u.text for u in utterances(act.orca)]
        ok = any(w.lower() in s.lower() for s in spoken for w in words)
        return ok, [f"Orca said {spoken!r}"]
    return custom(describe, run, needs_orca=True)


def said_state(word: str):
    """Orca says a selection state word (`selected` / `not selected`)."""
    return said_any_of([word], f"Orca says the tile is {word!r}")


def no_focus_move():
    """The act made no focus change on the bus (focus stayed where it was)."""
    return no_event("object:state-changed:focused")


def focus_names_tile(index: int):
    """The act's last focus change is a cell whose label is tile `index`,
    checked on the tree after the act (the focused cell)."""
    def run(act):
        for label, node in _cells(act.tree):
            if "focused" in node.get("states", []):
                return label == CAPTIONS[index], [f"focused cell holds {label!r}"]
        return False, ["no focused cell in the tree after the act"]
    return custom(f"the focused cell holds {CAPTIONS[index]!r}", run, needs_tree=True)


def focus_is_grid():
    return focused(role="table", name="Photo library")


# ---------------------------------------------------------------------------
# Scene setting
# ---------------------------------------------------------------------------


def focus_grid(run, label="Tab onto the grid"):
    """Tab onto the grid: its only Tab stop, with no cursor yet."""
    run.wait_for(role="table", name="Photo library")
    with run.act(label, [focus_is_grid(), said("Photo library")],
                 should="focus reaches the grid and the reader hears its name, its role "
                        "and how many tiles it holds"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# 1. The arrows
# ---------------------------------------------------------------------------


def arrows(run):
    focus_grid(run)
    with run.act("Right: the first tile",
                 tile_focus(0, count_said(1) + [no_message_id(), cell_state(0, "selected")]),
                 should="with no cursor yet, Right lands on the first tile, which becomes "
                        "the selection: the reader hears 'Sunset 1' and '1 item selected'"):
        run.key("Right")
    with run.act("Right again", tile_focus(1, [cell_state(1, "selected")]),
                 should="the cursor and the selection move one tile on: 'Harbor 2'; one "
                        "tile is still selected, so no count is said"):
        run.key("Right")
    with run.act("Down a row", tile_focus(6),
                 should="the cursor moves five tiles on (one row down): 'Market 7'"):
        run.key("Down")
    with run.act("Left", tile_focus(5), should="the cursor moves back one tile: 'Garden 6'"):
        run.key("Left")
    with run.act("Up", tile_focus(0), should="the cursor moves up a row to 'Sunset 1'"):
        run.key("Up")
    with run.act("Left at the start of a row", [no_focus_move()],
                 should="no wrap: nothing moves (a silent edge is the usual grid "
                        "behaviour)"):
        run.key("Left")


# ---------------------------------------------------------------------------
# 2. Home / End / PageDown / PageUp: the far jumps, into unrealized tiles
# ---------------------------------------------------------------------------


def far_jumps(run):
    focus_grid(run)
    with run.act("Right: the first tile", tile_focus(0), should="'Sunset 1'"):
        run.key("Right")
    with run.act("End: the last tile", tile_focus(59, [focus_names_tile(59)]),
                 should="End reaches the last tile of the collection, 'Canyon 15', "
                        "which was not realized; the grid scrolls and the reader hears it"):
        run.key("End")
    with run.act("Home: back to the first", tile_focus(0, [focus_names_tile(0)]),
                 should="Home returns to 'Sunset 1'"):
        run.key("Home")
    with run.act("PageDown", [focused(role="table cell"),
                              said_any_of(CAPTIONS, "Orca says the tile PageDown lands on")],
                 should="the cursor moves about a viewport down and the reader hears the "
                        "tile it lands on", tree=True):
        run.key("PageDown")
    with run.act("PageUp", tile_focus(0),
                 should="the cursor comes back a viewport up to 'Sunset 1'"):
        run.key("PageUp")
    with run.act("Ctrl+End", tile_focus(59), should="the last tile again, cursor only"):
        run.key("Ctrl+End")
    with run.act("Ctrl+Home", tile_focus(0), should="the first tile again, cursor only"):
        run.key("Ctrl+Home")


# ---------------------------------------------------------------------------
# 3. Selection: Space, Ctrl+arrow + Ctrl+Space, Shift+arrow, Ctrl+A
# ---------------------------------------------------------------------------


def selection(run):
    focus_grid(run)
    with run.act("Right: the first tile, selected", tile_focus(0, count_said(1)),
                 should="the cursor lands on 'Sunset 1', which becomes the selection; the "
                        "reader hears the tile and '1 item selected'"):
        run.key("Right")
    with run.act("Space: unselect it",
                 count_said(0) + [no_focus_move(), cell_state(0, "selected", present=False)],
                 should="Space toggles the tile off in a multiple selection; focus stays "
                        "put and the reader hears that nothing is selected now"):
        run.key("space")
    with run.act("Space: select it again",
                 count_said(1) + [no_focus_move(), cell_state(0, "selected")],
                 should="Space toggles it back on: '1 item selected'"):
        run.key("space")
    with run.act("Ctrl+Right: move the cursor only",
                 tile_focus(1, [cell_state(1, "selected", present=False),
                                said_state("not selected")]),
                 should="the cursor walks to 'Harbor 2' without selecting it; the reader "
                        "hears the tile and that it is not selected"):
        run.key("Ctrl+Right")
    with run.act("Ctrl+Space: add it", count_said(2) + [no_focus_move()],
                 should="Ctrl+Space adds 'Harbor 2': '2 items selected'"):
        run.key("Ctrl+space")
    with run.act("Shift+Right: extend", tile_focus(2, count_said(3)),
                 should="the selection extends from the anchor to 'Trail 3'; the reader "
                        "hears the tile and the new count"):
        run.key("Shift+Right")
    with run.act("Ctrl+A: select all", count_said(60) + [no_focus_move()],
                 should="every tile is selected: '60 items selected'"):
        run.key("Ctrl+A")
    with run.act("Ctrl+Shift+A: select none", count_said(0) + [no_focus_move()],
                 should="the selection is cleared: 'No item selected'"):
        run.key("Ctrl+Shift+A")


# ---------------------------------------------------------------------------
# 4. Type-ahead
# ---------------------------------------------------------------------------


def type_ahead(run):
    focus_grid(run)
    with run.act("type 'c'", tile_focus(8, count_said(1)),
                 should="the first caption after the start that begins with c, 'Cabin 9', "
                        "which becomes the selection; the reader hears it"):
        run.type("c")
    with run.act("type 'c' again", tile_focus(10),
                 should="the same letter cycles to the next c: 'Canyon 11'"):
        run.type("c")
    with run.act("type 'cab' quickly", [focused(role="table cell"), tile_said(19),
                                        focus_names_tile(19)],
                 should="a new search from 'Canyon 11': the next caption starting 'cab' "
                        "is 'Cabin 5' (index 19), and the reader hears it", tree=True):
        run.type("cab")
    with run.act("Home: back to 'Sunset 1'", tile_focus(0),
                 should="the first tile, to start the next search from a known place"):
        run.key("Home")
    with run.act("type 'can' quickly", [focused(role="table cell"), tile_said(10),
                                        focus_names_tile(10)],
                 should="from 'Sunset 1', the next caption starting 'can' is 'Canyon 11' "
                        "(index 10)", tree=True):
        run.type("can")


# ---------------------------------------------------------------------------
# 5. Reorder with Alt+arrow
# ---------------------------------------------------------------------------


def reorder(run):
    focus_grid(run)
    # Ctrl+Right with no cursor lands on the first tile without selecting it, so
    # nothing is announced before the move: the move's own announcement is
    # then the first message of the session.
    with run.act("Ctrl+Right: the first tile, cursor only", tile_focus(0),
                 should="the cursor lands on 'Sunset 1' without selecting it"):
        run.key("Ctrl+Right")
    with run.act("Alt+Right: move it one on",
                 [announced("Sunset 1 moved to 2 of 60"), said("moved to 2 of 60")],
                 should="the tile moves one place on; the reader hears where it went "
                        "(through the announcer: K2)"):
        run.key("Alt+Right")
    with run.act("Alt+Down: move it a row down",
                 [announced("Sunset 1 moved to 7 of 60"), said("moved to 7 of 60")],
                 should="the tile moves a row down, to position 7 (announcer: K2)"):
        run.key("Alt+Down")
    with run.act("Right: the tile after it", tile_focus(7),
                 should="the neighbour of the moved tile is 'Bridge 8' (unchanged)"):
        run.key("Right")
    # The order is now Harbor 2, Trail 3, Picnic 4, Summit 5, Garden 6, Market 7,
    # Sunset 1, Bridge 8, ...
    with run.act("Home: the first tile, now 'Harbor 2'", [focused(role="table cell"),
                                                         said("Harbor 2")],
                 should="the first tile is now 'Harbor 2'"):
        run.key("Home")
    with run.act("type 's': the next caption starting with s",
                 [focused(role="table cell"), said("Summit 5"), not_said("Garden 6")],
                 should="after the moves, the next tile whose caption starts with s is "
                        "'Summit 5' (now fourth); type-ahead finds it by what the tiles "
                        "say now", tree=True):
        run.type("s")


# ---------------------------------------------------------------------------
# 6. Sections
# ---------------------------------------------------------------------------


def sections(run):
    focus_grid(run)
    with run.act("Right: the first tile", tile_focus(0, [said("Travel")]),
                 should="the first tile of the first album: the reader hears 'Sunset 1' "
                        "and, entering the grid, the album it is in ('Travel')"):
        run.key("Right")
    with run.act("Down: the second row of Travel", tile_focus(5),
                 should="the second row of Travel: 'Garden 6'"):
        run.key("Down")
    with run.act("Down: the last row of Travel", tile_focus(10),
                 should="the third row of Travel: 'Canyon 11'"):
        run.key("Down")
    with run.act("Down: into the Family album", tile_focus(15, [said("Family")]),
                 should="the cursor crosses into the next album: the reader hears "
                        "'Summit 1' and that this is the Family album"):
        run.key("Down")
    with run.act("Up: back into Travel", tile_focus(10, [said("Travel")]),
                 should="the cursor crosses back: 'Canyon 11', in the Travel album"):
        run.key("Up")


# ---------------------------------------------------------------------------
# 7. A screen reader's own actions on a tile; Escape; Enter
# ---------------------------------------------------------------------------


def cell_path(run, index: int) -> str:
    """The bus path of the realized cell holding tile `index`, found through its
    label child (a cell has no name of its own)."""
    from reader_lib.run import RunError
    for label, node in _cells(run.tree_now()):
        if label == CAPTIONS[index]:
            return node["path"]
    raise RunError(f"no realized cell holds {CAPTIONS[index]!r}")


class AppLog:
    """What the example printed during an act ("activate tile N")."""

    def __init__(self, run):
        self.run = run
        self.marks: dict[str, list[str]] = {}
        self._start = 0

    def lines(self) -> list[str]:
        try:
            return self.run.app_log.read_text(errors="replace").splitlines()
        except OSError:
            return []

    def begin(self) -> None:
        self._start = len(self.lines())

    def end(self, label: str) -> None:
        self.marks[label] = self.lines()[self._start:]

    def activated(self, label: str, index: int):
        def check(_act):
            got = self.marks.get(label, [])
            return f"activate tile {index}" in got, [f"the example printed {got!r}"]
        return custom(f"the example activated tile {index} ({CAPTIONS[index]!r})", check)


def at_actions(run):
    focus_grid(run)
    with run.act("Right: the first tile", tile_focus(0), should="'Sunset 1'"):
        run.key("Right")
    market = cell_path(run, 6)
    with run.act("AT-SPI click on 'Market 7'",
                 tile_focus(6, [cell_state(6, "selected")]),
                 should="a screen reader's activation of a tile selects it and puts the "
                        "cursor on it; the reader hears the tile (the count stays at one, "
                        "so no count is said)", tree=True):
        run.action("click", path=market)
    with run.act("Escape: drop the cursor", [focus_is_grid(), said("Photo library")],
                 should="Escape clears the cursor; focus is back on the grid itself and "
                        "the reader hears so"):
        run.key("Escape")
    log = AppLog(run)
    with run.act("Enter on the grid with no cursor",
                 [log.activated("enter", 6),
                  said_any_of(["Market 7"], "the reader hears which tile Enter acted on")],
                 should="Enter opens the tile the keyboard would act on (the selected "
                        "'Market 7'); the reader learns which"):
        log.begin()
        run.key("Return")
        run.wait(0.5)
        log.end("enter")


def at_focus(run):
    """A screen reader putting focus on a tile (UIA SetFocus from NVDA's object
    navigation, VoiceOver's cursor with 'keyboard focus follows the VoiceOver
    cursor' on, AT-SPI grab_focus): the keys should then act on that tile."""
    focus_grid(run)
    with run.act("Right: the first tile", tile_focus(0), should="'Sunset 1'"):
        run.key("Right")
    trail = cell_path(run, 2)
    with run.act("AT-SPI grab_focus on 'Trail 3'", tile_focus(2),
                 should="the tile advertises a focus action; asking for it moves the "
                        "cursor there and the reader hears 'Trail 3'", tree=True):
        run.grab_focus(path=trail)
    log = AppLog(run)
    with run.act("Enter on the focused tile", [log.activated("enter", 2)],
                 should="Enter opens the tile focus is on, 'Trail 3'"):
        log.begin()
        run.key("Return")
        run.wait(0.5)
        log.end("enter")
    with run.act("Right from 'Trail 3'", tile_focus(3),
                 should="the cursor moves on from the tile focus is on: 'Picnic 4'",
                 tree=True):
        run.key("Right")
    with run.act("Right again", tile_focus(4),
                 should="'Summit 5'", tree=True):
        run.key("Right")


# ---------------------------------------------------------------------------
# 8. The tile's context menu: the other non-drag reorder
# ---------------------------------------------------------------------------


def context_menu(run):
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0),
                 should="the cursor lands on 'Sunset 1' without selecting anything"):
        run.key("Ctrl+Right")
    with run.act("Ctrl+Right: 'Harbor 2', cursor only", tile_focus(1),
                 should="the cursor moves on to 'Harbor 2' without selecting anything"):
        run.key("Ctrl+Right")
    with run.act("Menu key: the tile's context menu",
                 [said("Move Left"), menu_current("Move Left")],
                 should="the context menu of the focused tile opens; the reader hears it "
                        "and its current row, 'Move Left'", tree=True):
        run.key("Menu")
    with run.act("Down: the next row", [said("Move Right"), menu_current("Move Right")],
                 should="the highlight moves to the next row and the reader hears it",
                 tree=True):
        run.key("Down")
    with run.act("Enter: activate the highlighted row",
                 [announced("Harbor 2 moved to 3 of 60"), said("moved to 3 of 60")],
                 should="'Move Right' runs: the tile moves one on, the menu closes, focus "
                        "returns to the grid on the moved tile and the reader hears where "
                        "it went"):
        run.key("Return")


def menu_current(name: str):
    """After the act, the reader can tell which menu row is current: focus or
    the menu's active descendant is on the row named `name`."""
    from reader_lib.checks import _walk

    def run(act):
        rows = [n for n in _walk(act.tree) if n.get("role") == "menu item"]
        focused_rows = [n.get("name") for n in rows if "focused" in n.get("states", [])]
        menus = [n for n in _walk(act.tree) if n.get("role") == "menu"]
        evidence = [f"menu rows: {[(n.get('name'), n.get('states')) for n in rows]}",
                    f"menus: {[(n.get('name'), n.get('states')) for n in menus]}"]
        return name in focused_rows, evidence
    return custom(f"the menu's current row is {name!r} on the bus", run, needs_tree=True)


# ---------------------------------------------------------------------------
# 9. Two announcements in a row: the count, then again
# ---------------------------------------------------------------------------


def repeat_count(run):
    """Ctrl+Space three times on three tiles: three counts in a row, with no
    focus move in the act, so nothing but K2 stands between the reader and each
    message."""
    focus_grid(run)
    with run.act("Ctrl+Right: 'Sunset 1', cursor only", tile_focus(0),
                 should="the cursor on the first tile, nothing selected"):
        run.key("Ctrl+Right")
    for n in (1, 2, 3):
        with run.act(f"Ctrl+Space: {n} selected", count_said(n) + [no_message_id()],
                     should=f"the tile is added to the selection: '{count_words(n)}'"):
            run.key("Ctrl+space")
        if n < 3:
            with run.act(f"Ctrl+Right: {CAPTIONS[n]!r}, cursor only", tile_focus(n),
                         should="the cursor moves on without touching the selection"):
                run.key("Ctrl+Right")


SCENARIOS = [
    Scenario("gridview-arrows", "grid-view", arrows,
             "Tab onto the grid, then the four arrows"),
    Scenario("gridview-far-jumps", "grid-view", far_jumps,
             "Home / End / PageDown / PageUp / Ctrl+Home / Ctrl+End"),
    Scenario("gridview-selection", "grid-view", selection,
             "Space, Ctrl+arrow + Ctrl+Space, Shift+arrow, Ctrl+A, Ctrl+Shift+A"),
    Scenario("gridview-type-ahead", "grid-view", type_ahead,
             "type-ahead on the tile captions"),
    Scenario("gridview-reorder", "grid-view", reorder,
             "Alt+Right / Alt+Down move the focused tile"),
    Scenario("gridview-sections", "grid-view", sections,
             "crossing from one album (section) into the next"),
    Scenario("gridview-at-actions", "grid-view", at_actions,
             "AT-SPI click on a tile, Escape, Enter"),
    Scenario("gridview-at-focus", "grid-view", at_focus,
             "AT-SPI grab_focus on a tile, then Enter and Right"),
    Scenario("gridview-context-menu", "grid-view", context_menu,
             "the tile's context menu: Move Right"),
    Scenario("gridview-repeat-count", "grid-view", repeat_count,
             "three selection counts in a row (the announcer)"),
]
