# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Seven small examples, as a screen reader meets them.

`color-picker-demo`, `font-picker`, `recent-projects`, `file-dialogs`,
`file-drop`, `drag-and-drop`, `text-and-layout`. Each scenario states, act by
act, what a reader should get; the report records what they got.

Where each piece is produced:

* **ColorPicker** (`crates/teksilo-widgets/src/color_picker.rs`): the root is a
  `Role::Group` named "Color picker" with `Live::Polite`, carrying "Color
  changed to #…" as its *value* (`accessibility`, ~l.813). The RGB / HSV
  spinners are bare `SpinBox`es beside a painted "R" / "G" / … `TextWidget`
  (`spinner_cell`, ~l.980) with no label. The preview is a `ColorSwatch`
  labelled "Selected color" whose hex is its value (`color_picker/swatch.rs`
  `accessibility`). The saturation × brightness field is a `Role::Group`
  whose pair is its value, and whose arrow keys `ctx.announce` the new pair
  (`color_picker/hsv_canvas.rs`). The hex field is `HexColorInput`, a
  `Role::TextInput` wrapper around a `TextInput` whose inner field is a
  second `Role::TextInput` (`hex_color_input.rs` `accessibility`).
* **FontPicker** (`font_picker.rs`) is a searchable `ComboBox` preset; the
  per-row sample is `a11y_hidden`.
* **recent-projects** renders its rows through a `Repeater`, each row a
  `Panel` with a title label and three buttons named "Open", "Pin"/"Unpin",
  "Remove" (`examples/recent_projects/src/main.rs`).
* **file-dialogs**' status line is a plain `TextWidget` bound to a signal. The
  native dialog is the XDG portal's, which the private session runs; the
  scenarios cancel it with Escape or type a path into it.
* **DropZone** (`drop_zone.rs`) is a `Role::Group` labelled by its prompt,
  with a `Live::Polite` status `TextWidget` and a "Browse…" button.
* **drag-and-drop**'s three `ListView`s and one `TreeView` carry no label; the
  in-view reorder has a keyboard route (`common/ordered_move.rs`), the
  cross-view export has none (`docs/a11y/non-drag-alternatives.md`, "What is
  still drag-only").
* **text-and-layout** is `TextWidget`s only, two of them visual section
  titles.

Every act, the scene setting included, is recorded: the harness credits
Orca's speech to an act by matching its receipts to the act's events, and an
event between two acts would be credited to the next one.
"""

from __future__ import annotations

from reader_lib.checks import (_walk, custom, event, focused, in_tree, no_event, said)
from reader_lib.orca import normalized, utterances
from reader_lib.run import REPO
from reader_lib.scenario import Scenario


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def _heard(act) -> list[str]:
    return [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in utterances(act.orca)] \
        or ["Orca said nothing in the act"]


def said_any(*texts: str):
    """Orca said, uncut, something containing one of `texts`."""
    def run(act):
        ok = any(not u.cut and any(normalized(t) in normalized(u.text) for t in texts)
                 for u in utterances(act.orca))
        return ok, _heard(act)
    return custom(f"Orca says one of {list(texts)!r}", run, needs_orca=True)


def said_checked():
    """Orca said 'checked' as a state, not as part of 'not checked'."""
    def run(act):
        ok = any(not u.cut and "checked" in normalized(u.text)
                 and "not checked" not in normalized(u.text) for u in utterances(act.orca))
        return ok, _heard(act)
    return custom("Orca says 'checked' (not 'not checked')", run, needs_orca=True)


def said_something():
    def run(act):
        return bool(utterances(act.orca)), _heard(act)
    return custom("Orca says something", run, needs_orca=True)


def speech_count_at_most(limit: int):
    def run(act):
        heard = utterances(act.orca)
        return len(heard) <= limit, [f"{len(heard)} utterances"] + _heard(act)[:12]
    return custom(f"Orca says at most {limit} things", run, needs_orca=True)


def spoken_at_most(text: str, limit: int):
    """At most `limit` uncut utterances of the act contain `text`."""
    def run(act):
        heard = [u for u in utterances(act.orca)
                 if not u.cut and normalized(text) in normalized(u.text)]
        return len(heard) <= limit, [f"{len(heard)} uncut utterances contain {text!r}"] \
            + _heard(act)
    return custom(f"Orca says {text!r} at most {limit} time(s)", run, needs_orca=True)


def announcements_at_most(limit: int):
    def run(act):
        found = [e for e in act.events if e["type"] == "object:announcement"]
        return len(found) <= limit, [f"{len(found)} object:announcement events"] + [
            f"{act.rel_ms(e):+.1f} ms [{e['source'].get('role')}] {e.get('text')!r}"
            for e in found[:15]]
    return custom(f"at most {limit} object:announcement events", run)


def node_where(describe: str, pred, *, want: bool = True):
    """After the act, some node of the tree satisfies `pred` (or none does)."""
    def run(act):
        hits = [n for n in _walk(act.tree) if pred(n)]
        lines = [f"[{n.get('role')}] {n.get('name')!r} states={n.get('states')} "
                 f"attrs={n.get('attributes')} value={n.get('value')}" for n in hits[:12]]
        return (bool(hits) == want), lines or ["no node matched"]
    return custom(describe, run, needs_tree=True)


def focus_named(describe: str, pred):
    """The act's last focus change landed on a node for which `pred(node)`."""
    from reader_lib.checks import _focus_node, _is_focus

    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        node = _focus_node(moves[-1])
        return bool(pred(node)), [f"last focus: [{node.get('role')}] {node.get('name')!r}"]
    return custom(describe, run)


def scene(run, label: str, should: str = "", **kw):
    """An act that only sets the scene: recorded, so its events are its own."""
    return run.act(f"(scene) {label}", should=should or "setting the scene", **kw)


# ---------------------------------------------------------------------------
# text-and-layout
# ---------------------------------------------------------------------------


def text_layout_body(run):
    with run.act("read the window as a reader browses it", [
            in_tree(role="label", name="Body text (14px) — the default reading style for content."),
            node_where("every label carries its words on the Text interface",
                       lambda n: n.get("role") == "label"
                       and (n.get("text") or {}).get("text") != n.get("name"), want=False),
            in_tree(role="heading", name="Typography Styles"),
            in_tree(role="heading", name="Layout Primitives"),
            in_tree(role="heading", name="Text & Layout"),
    ], should="every line of text is a label a reader can review, and the three titles are "
              "headings a reader can jump between", tree=True):
        run.wait(0.2)
    with run.act("Tab to the theme switcher", [
            focused(role="combo box", name="Theme"),
            said("Theme"),
            said_any("Light", "Dark", "System"),
    ], should="the reader hears the switcher's name and which theme is on"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# file-dialogs
# ---------------------------------------------------------------------------


#: A file that exists on every checkout, typed into the portal's file dialog.
PICK_FILE = str(REPO / "examples" / "file_dialogs" / "Cargo.toml")
#: A PNG the image-only DropZone accepts.
PICK_PNG = str(REPO / "docs" / "widgets" / "img" / "accordion.png")


def file_dialogs_body(run):
    # The native dialog is the XDG portal's (xdg-desktop-portal-kde), which
    # the private session starts; it is out of scope. What is in scope is what
    # the reader hears when it closes: the example writes the result into a
    # plain status `TextWidget` (`examples/file_dialogs/src/main.rs`).
    with scene(run, "Tab to Open file…"):
        run.key("Tab", "Tab")
    with run.act("Space on Open file… opens the portal's dialog", [said("dialog")],
                 should="the native dialog opens (out of scope beyond that)", record=3.0):
        run.key("space")
    with run.act("Escape cancels the dialog", [
            focused(role="push button", name="Open file…"),
            event("object:property-change:accessible-name", role="label",
                  name_contains="cancelled"),
            said("Open cancelled"),
    ], should="focus is back on Open file…, and the reader hears what came of the dialog, "
              "as a sighted user reads it in the status line", record=3.5, tree=True):
        run.key("Escape")
    with run.act("Space on Open file…, type a path in the dialog, Enter", [
            focused(role="push button", name="Open file…"),
            event("object:property-change:accessible-name", role="label",
                  name_contains="Opened:"),
            said("Opened"),
    ], should="the file is opened, focus is back on the button, and the reader hears "
              "'Opened: …'", record=4.0):
        run.key("space")
        run.wait(2.5)
        run.type(PICK_FILE)
        run.wait(0.5)
        run.key("Return")


# ---------------------------------------------------------------------------
# recent-projects
# ---------------------------------------------------------------------------


def _row_button_context(node: dict) -> bool:
    name = node.get("name") or ""
    return name not in ("Open", "Pin", "Unpin", "Remove")


def recent_projects_body(run):
    # After seeding, the rows are (top to bottom) playground, journal-2026.md,
    # Teksilo, ★ Skribisto; their Pin buttons are, in order, playground's,
    # journal's, Teksilo's.
    with scene(run, "focus Seed demo entries"):
        run.grab_focus(role="push button", name="Seed demo entries")
    with run.act("Space on Seed demo entries", [
            in_tree(role="label", name_contains="Skribisto"),
            said_something(),
    ], should="the four entries appear, and the reader is told something happened",
            tree=True):
        run.key("space")
    with run.act("Tab into the first row", [
            focus_named("the focused row button says which project it acts on",
                        _row_button_context),
    ], should="the reader knows which project the button belongs to"):
        run.key("Tab", "Tab")
    with run.act("Tab through that row", [
            focus_named("the focused row button says which project it acts on",
                        _row_button_context),
    ], should="the reader knows which project each button acts on"):
        run.key("Tab")
    with scene(run, "focus Teksilo's Pin (the third row's)"):
        run.grab_focus(role="push button", name="Pin", nth=2)
    with run.act("Space on Teksilo's Pin", [
            focused(role="push button", name="Unpin"),
            said_any("pinned", "Unpin"),
    ], should="Teksilo is pinned, focus stays in its row on the (now) Unpin button, and the "
              "reader hears the new state", tree=True, record=3.0):
        run.key("space")
    with run.act("Tab after pinning", [said_something()],
                 should="Tab continues from where the reader was"):
        run.key("Tab")
    with scene(run, "focus the third row's Remove"):
        run.grab_focus(role="push button", name="Remove", nth=2)
    with run.act("Space on the third row's Remove", [
            focus_named("focus lands on a neighbouring row's control",
                        lambda n: n.get("role") == "push button"),
            said_something(),
    ], should="the row goes, the reader is told, and focus moves to the next or previous "
              "row", tree=True, record=3.0):
        run.key("space")
    with scene(run, "focus Hide paths"):
        run.grab_focus(role="push button", name="Hide paths")
    with run.act("Space on Hide paths", [
            focused(role="push button", name="Show paths"),
            said("Show paths"),
    ], should="the paths go, and focus lands on the Show paths button that replaced the one "
              "pressed", tree=True):
        run.key("space")
    with run.act("Tab after Hide paths", [said_something()],
                 should="Tab continues from the toolbar, where the reader was"):
        run.key("Tab")
    with scene(run, "focus Bigger font"):
        run.grab_focus(role="push button", name="Bigger font")
    with run.act("Space on Bigger font", [
            event("object:property-change:accessible-name", role="label",
                  name_contains="Font size: 17"),
            said("17"),
    ], should="the reader hears the new size, as the header shows it"):
        run.key("space")


# ---------------------------------------------------------------------------
# font-picker
# ---------------------------------------------------------------------------


def font_picker_body(run):
    with scene(run, "Tab to Font family"):
        run.key("Tab", "Tab")
    with run.act("open the font list with Alt+Down", [
            said_something(),
    ], should="the list opens and the reader hears where focus went (the search field or "
              "the first font), and how many fonts there are", tree=True, record=3.0):
        run.key("Alt+Down")
    with run.act("Down arrow", [
            said_something(),
            focus_named("focus or the active item is a named font row",
                        lambda n: bool(n.get("name"))),
    ], should="the reader hears the next font's family name"):
        run.key("Down")
    with run.act("Down arrow again", [said_something()],
                 should="the reader hears the next font's family name"):
        run.key("Down")
    with run.act("type 'mono' to search", [
            said_something(),
    ], should="the list narrows, and the reader hears what they typed and what is now "
              "selectable", tree=True, record=3.0):
        run.type("mono")
    with run.act("Down arrow in the filtered list", [said_something()],
                 should="the reader hears the first matching font"):
        run.key("Down")
    with run.act("Enter to pick it", [
            focused(role="combo box", name="Font family"),
            said_any("Mono", "mono"),
    ], should="the list closes, focus returns to the picker, and the reader hears the "
              "font now chosen", tree=True, record=3.0):
        run.key("Return")
    with scene(run, "Tab to Monospace only"):
        run.key("Tab")
    with run.act("Space checks Monospace only", [
            event("object:state-changed:checked", role="check box", detail1=1),
            said_checked(),
    ], should="the reader hears the box checked", record=3.0):
        run.key("space")
    with run.act("Shift+Tab back to Font family", [
            focused(role="combo box", name="Font family"),
            said_any("Mono", "mono"),
    ], should="the reader hears the picker's name and the font it holds"):
        run.key("Shift+Tab")
    with scene(run, "Tab twice to Writing system"):
        run.key("Tab", "Tab")
    with run.act("Alt+Down opens the writing-system list", [said_something()],
                 should="the list opens and the reader hears the current script",
                 tree=True, record=3.0):
        run.key("Alt+Down")
    with run.act("Down arrow in the writing-system list", [said("Latin")],
                 should="the reader hears the next script, Latin"):
        run.key("Down")
    with run.act("Enter picks it", [
            focused(role="combo box", name="Writing system"),
            said("Latin"),
    ], should="the list closes and the reader hears the script now chosen", record=3.0):
        run.key("Return")


def font_mono_toggle_body(run):
    """The Monospace only box: when does its state reach the bus?

    In the first run its checked state arrived only with the next key, ~4 s
    late. So: Space, then a key that does nothing here (Shift), then the same
    through AT-SPI's click, then again by Space."""
    with scene(run, "Tab three times to Monospace only"):
        run.key("Tab", "Tab", "Tab")
    for round_ in (1, 2):
        state = 1 if round_ == 1 else 0
        with run.act(f"Space {round_}: {'check' if state else 'uncheck'} Monospace only", [
                event("object:state-changed:checked", role="check box", detail1=state),
                said_checked() if state else said("not checked"),
        ], should="the box changes at once and the reader hears it", record=4.0):
            run.key("space")
        with run.act(f"Shift {round_}: a key that does nothing here", [
                no_event("object:state-changed:checked", role="check box"),
        ], should="nothing: the box's change belongs to the Space before", record=3.0):
            run.key("shift")
    with run.act("AT-SPI click on Monospace only", [
            event("object:state-changed:checked", role="check box", detail1=1),
            said_checked(),
    ], should="a screen reader's own activation checks the box at once", record=4.0):
        run.action("click", role="check box", name="Monospace only")
    with run.act("Shift 3: a key that does nothing here", [
            no_event("object:state-changed:checked", role="check box"),
    ], should="nothing", record=3.0):
        run.key("shift")


# ---------------------------------------------------------------------------
# file-drop
# ---------------------------------------------------------------------------


def file_drop_body(run):
    with run.act("Tab to the first Browse…", [
            focused(role="push button", name="Browse…"),
            said("Drop images here"),
            said("Browse"),
    ], should="the reader hears which zone the Browse button belongs to"):
        run.key("Tab")
    with run.act("Space on Browse…, type a PNG's path in the portal's dialog, Enter", [
            focused(role="push button", name="Browse…"),
            event("object:property-change:accessible-name", role="label",
                  name_contains="Added 1 file"),
            said("Added 1 file"),
    ], should="the file is taken, focus is back on Browse…, and the zone's live status "
              "line says 'Added 1 file', as it does for a drop", record=4.0, tree=True):
        run.key("space")
        run.wait(2.5)
        run.type(PICK_PNG)
        run.wait(0.5)
        run.key("Return")
    with run.act("the tree: a keyboard route to the drag-out rows and the internal target", [
            node_where("the drag-out rows are focusable, or offer an action",
                       lambda n: "Drag this" in (n.get("name") or "")
                       and ("focusable" in n.get("states", []) or n.get("actions"))),
            node_where("the internal drop target is named",
                       lambda n: n.get("role") == "panel"
                       and "drop" in (n.get("name") or "").lower()
                       and "Internal" in (n.get("name") or "")),
    ], should="a keyboard user can start the outbound drags and reach the internal target",
            tree=True):
        run.wait(0.2)


# ---------------------------------------------------------------------------
# drag-and-drop
# ---------------------------------------------------------------------------


def drag_and_drop_body(run):
    with run.act("Tab into the Songs list", [
            focused(role="list box"),
            said("Songs"),
    ], should="the reader hears the list's name (Songs), and where in it they are",
            tree=True):
        run.key("Tab", "Tab")
    with run.act("Down arrow in Songs", [
            said("Hyperballad"),
    ], should="the first song is current and the reader hears it"):
        run.key("Down")
    with run.act("Down arrow to Unravel", [said("Unravel")],
                 should="the reader hears the next song"):
        run.key("Down")
    with run.act("Alt+Down moves Unravel one place later", [
            said("Unravel"),
            said_any("3", "three"),
    ], should="the song moves, and the reader hears where it went", tree=True, record=3.0):
        run.key("Alt+Down")
    with run.act("Alt+Down again", [said_any("4", "four")],
                 should="the song moves on, and the reader hears its new place", record=3.0):
        run.key("Alt+Down")
    with run.act("Alt+Up moves it back", [said_any("3", "three")],
                 should="the song moves back, and the reader hears its new place", record=3.0):
        run.key("Alt+Up")
    with run.act("open the row's context menu with the Menu key", [
            said_something(),
            node_where("the menu offers a way to copy the song to the playlist",
                       lambda n: n.get("role", "").endswith("menu item")
                       and "playlist" in (n.get("name") or "").lower()),
    ], should="the reorder rows are there, and a row to add the song to the playlist, "
              "which is what the drag does", tree=True, record=3.0):
        run.key("Menu")
    with run.act("Down arrow in the menu", [said("Move")],
                 should="the first command row is current and the reader hears it"):
        run.key("Down")
    with run.act("Enter on the current menu row", [
            focused(role="list item", name="Unravel"),
            said_any("of 10"),
    ], should="the menu row does what the chord does, and the reader hears the new place",
            record=3.0):
        run.key("Return")
    with run.act("Tab to the Playlist", [
            focused(role="list box"),
            said("Playlist"),
    ], should="the reader hears the list's name and that it is empty"):
        run.key("Tab")
    with run.act("Tab to Up next", [focused(role="list box"), said_any("Up next", "queue")],
                 should="the reader hears the queue's name"):
        run.key("Tab")
    with run.act("Tab to Folders", [focused(role="tree"), said("Folders")],
                 should="the reader hears the tree's name"):
        run.key("Tab")
    with run.act("Down arrow in Folders", [
            said("Documents"),
            said_any("collapsed", "expandable"),
    ], should="the reader hears the folder and that it can be expanded", tree=True):
        run.key("Down")
    with run.act("Right arrow expands Documents", [
            said("expanded"),
    ], should="the reader hears the folder open", tree=True):
        run.key("Right")


# ---------------------------------------------------------------------------
# color-picker-demo
# ---------------------------------------------------------------------------
#
# At launch the framework focuses the first `TextInput` it finds
# (`TextInput::initial_focus_hint`, `text_input/widget_impl.rs`, used for a
# plain window by `initial_window_focus` in `teksilo-app/src/window_manager.rs`),
# which here is the first picker's Hex field. The first picker is bound to
# #3584E4: R 53, G 132, B 228; H 213, S 77 %, V 89 %.


def color_channels_body(run):
    with run.act("Tab from the Hex field to the first RGB spinner", [
            focused(role="spin button"),
            said_any("Red", "R spin"),
            said("53"),
    ], should="the reader hears which channel this is (Red) and its value, 53"):
        run.key("Tab")
    with run.act("Up arrow on it", [
            said("54"),
            said_any("Color changed to #3684E4", "3684E4"),
    ], should="the value steps to 54, and the picker says the colour it now holds, as its "
              "live region promises", tree=True, record=3.0):
        run.key("Up")
    with run.act("Tab to the next spinner", [
            said_any("Green", "G spin"),
            said("132"),
    ], should="the reader hears Green, 132"):
        run.key("Tab")
    with run.act("Shift+Tab three times to the Selected color preview", [
            focused(role="push button", name_contains="Selected color"),
            said_any("#3684E4", "3684E4"),
    ], should="the reader hears the colour the preview shows"):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Shift+Tab to the Hue slider", [
            focused(role="slider", name="Hue"),
            said("Hue"),
    ], should="the reader hears Hue and its value"):
        run.key("Shift+Tab")
    with run.act("Up arrow on Hue", [said_something()],
                 should="the reader hears the new hue", tree=True):
        run.key("Up")
    with run.act("Shift+Tab to Saturation and brightness", [
            focused(role="panel", name="Saturation and brightness"),
            said_any("Saturation 77", "77%"),
    ], should="the reader hears the field's name and the pair it holds"):
        run.key("Shift+Tab")
    with run.act("Right arrow on Saturation and brightness", [
            said("Saturation 78%"),
    ], should="saturation steps up one and the reader hears the new pair (the field "
              "announces it through the tree's announcer)", record=3.0):
        run.key("Right")
    with run.act("Up arrow on Saturation and brightness", [
            said("brightness 90%"),
    ], should="brightness steps up one and the reader hears the new pair (a second message "
              "through the announcer, the case K2 broke)", record=3.0):
        run.key("Up")


def color_hex_body(run):
    # Launch focus is the first picker's Hex field (see the note above).
    with run.act("select the Hex text and type an invalid short value, then Enter", [
            said_any("Not a valid hex color", "not a valid"),
    ], should="the value is refused and the reader hears why", tree=True, record=3.0):
        run.key("Ctrl+a")
        run.type("12")
        run.key("Return")
    with run.act("select all and type #FF8800, then Enter", [
            said_any("FF8800", "ff8800"),
            said_any("Color changed to #FF8800"),
    ], should="the colour is taken, and the reader hears it (the picker's live region "
              "promises 'Color changed to #FF8800')", tree=True, record=3.0):
        run.key("Ctrl+a")
        run.type("#FF8800")
        run.key("Return")
    with run.act("Tab away and back to the Hex field", [
            said("FF8800"),
            spoken_at_most("Hex entry", 1),
    ], should="the reader hears the field once, with its value"):
        run.key("Tab")
        run.wait(1.0)
        run.key("Shift+Tab")


def color_swatches_body(run):
    with scene(run, "focus the first picker's Color presets grid"):
        run.grab_focus(role="table", name="Color presets")
    with run.act("Tab once from the grid", [
            focus_named("Tab leaves the grid (one tab stop for the whole grid)",
                        lambda n: not (n.get("name") or "").startswith("Swatch")),
    ], should="the module doc's promise: Tab enters the grid once, arrows move inside it, "
              "Tab again leaves it"):
        run.key("Tab")
    with scene(run, "focus the grid again"):
        run.grab_focus(role="table", name="Color presets")
    with run.act("Right arrow inside the grid", [
            said("Swatch"),
    ], should="the next swatch becomes current and the reader hears it"):
        run.key("Right")
    with scene(run, "focus Swatch #3685E3 of the first picker"):
        run.grab_focus(role="push button", name="Swatch #3685E3")
    with run.act("Space on Swatch #3685E3", [
            said("selected"),
            announcements_at_most(2),
    ], should="the swatch becomes the colour, the reader hears it selected, once",
            tree=True, record=3.0):
        run.key("space")


def color_scroll_body(run):
    with scene(run, "focus the second picker's last swatch"):
        run.grab_focus(role="push button", name="Swatch #F5F5F5", nth=1)
    with run.act("Tab into the Compact picker, which scrolls into view", [
            focus_named("focus lands in the Compact picker",
                        lambda n: n.get("name") == "Saturation and brightness"),
            announcements_at_most(1),
            speech_count_at_most(3),
    ], should="the page scrolls and the reader hears the one control focus reached, not "
              "every control that scrolled into view", tree=True, record=3.0):
        run.key("Tab")
    with scene(run, "focus the Theme switcher"):
        run.grab_focus(role="combo box", name="Theme")
    with run.act("Shift+Tab wraps to the last control of the page", [
            said_something(),
            announcements_at_most(1),
    ], should="focus lands on the last ColorEdit, and the reader hears only that",
            tree=True, record=3.0):
        run.key("Shift+Tab")
    with run.act("Shift+Tab twice to the first ColorEdit", [
            focused(role="push button", name_contains="Color #E91E63"),
            said("E91E63"),
    ], should="the reader hears the trigger's colour and that it opens a picker"):
        run.key("Shift+Tab", "Shift+Tab")
    with run.act("Space opens its picker popover", [
            said_something(),
            announcements_at_most(1),
            speech_count_at_most(4),
    ], should="the popover opens, focus goes into it, and the reader hears where they are, "
              "not a roll-call of every control in it", tree=True, record=3.5):
        run.key("space")
    with run.act("Escape closes the popover", [
            focused(role="push button", name_contains="Color #"),
    ], should="focus returns to the trigger"):
        run.key("Escape")


# ---------------------------------------------------------------------------


SCENARIOS = [
    Scenario("misc-color-channels", "color-picker-demo", color_channels_body,
             "color picker: RGB / HSV spinners, preview, hue, saturation × brightness"),
    Scenario("misc-color-hex", "color-picker-demo", color_hex_body,
             "color picker: the hex field, refused and accepted values"),
    Scenario("misc-color-swatches", "color-picker-demo", color_swatches_body,
             "color picker: the preset grid, Tab stops, arrows, picking a swatch"),
    Scenario("misc-color-scroll", "color-picker-demo", color_scroll_body,
             "color picker: pickers scrolling into view, and a ColorEdit popover opening"),
    Scenario("misc-text-layout", "text-and-layout", text_layout_body,
             "text-and-layout: labels as text, section titles as headings, the theme switcher"),
    Scenario("misc-file-dialogs", "file-dialogs", file_dialogs_body,
             "file-dialogs: the buttons, and what the reader hears of a dialog's result"),
    Scenario("misc-recent-projects", "recent-projects", recent_projects_body,
             "recent-projects: seeding, row buttons, pin, remove, show/hide paths, font size"),
    Scenario("misc-font-picker", "font-picker", font_picker_body,
             "font-picker: open, arrow, search, pick, filter"),
    Scenario("misc-font-mono-toggle", "font-picker", font_mono_toggle_body,
             "font-picker: Monospace only by Space and by AT-SPI click; does its state reach the bus?"),
    Scenario("misc-file-drop", "file-drop", file_drop_body,
             "file-drop: DropZone Browse fallback, and the keyboard routes to the drags"),
    Scenario("misc-drag-and-drop", "drag-and-drop", drag_and_drop_body,
             "drag-and-drop: list names, keyboard reorder, the non-drag alternatives, tree"),
]
