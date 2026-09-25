# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for new-widgets-kit (the sweep's own acts are in
`newkit.py`; these only add what its scenarios did not do).

* the empty-field text events on two more fields than the sweep's search
  field: the FilePickerField's entry (empty at launch) and the InputDialog's
  field emptied with Ctrl+A, BackSpace, with a non-empty replacement beside it
  as the control (Orca says "Selection deleted." from the delete event);
* a dismissed banner's invisible controls operated from the keyboard (the
  example prints "Save now clicked" when Save now runs);
* Space to pick a highlighted suggestion.
"""

from __future__ import annotations

from reader_lib.checks import (_focus_node, _is_focus, custom, event, focused, in_tree,
                               not_in_tree, said)
from reader_lib.scenario import Scenario

PACKAGE = "new-widgets-kit"


def focus_sequence():
    """Record every focus landing of the act, in order (always passes)."""
    def run(act):
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r} {n.get('path')}"
                                  for n in moves) or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def text_events():
    """Record every text-changed event of the act with its source path."""
    def run(act):
        rows = [f"{e['type']} [{e['source'].get('role')}] {e['source'].get('path')} "
                f"text={e.get('text')!r}" for e in act.events
                if e["type"].startswith("object:text-changed")]
        return True, rows or ["no text-changed event"]
    return custom("the act's text-changed events (a record)", run)


# ---------------------------------------------------------------------------
# Empty-field text events on the file entry and the dialog field
# ---------------------------------------------------------------------------


def empty_edits(run):
    run.wait_for(role="entry", state="focused")
    with run.act("Tab to the file entry (empty)", [focused(role="entry"), focus_sequence()],
                 should="focus reaches the FilePickerField's entry"):
        run.key("Tab")
    with run.act("type 'a' into the empty file entry",
                 [event("object:text-changed:insert", role="entry", text_contains="a"),
                  text_events()],
                 should="the first character typed into an empty field is reported"):
        run.type("a")
    with run.act("type 'b' after it (control)",
                 [event("object:text-changed:insert", role="entry", text_contains="b"),
                  text_events()],
                 should="the second character is reported"):
        run.type("b")
    with run.act("BackSpace deletes 'b' (control: the field keeps 'a')",
                 [event("object:text-changed:delete", role="entry", text_contains="b"),
                  text_events()],
                 should="the deletion is reported"):
        run.key("BackSpace")
    with run.act("BackSpace deletes 'a' and empties the field",
                 [event("object:text-changed:delete", role="entry", text_contains="a"),
                  text_events()],
                 should="the deletion of the last character is reported like any other"):
        run.key("BackSpace")
    with run.act("Tab twice to Rename…", [focused(role="push button", name="Rename…")]):
        run.key("Tab", "Tab")
    with run.act("Space opens the InputDialog", [in_tree(role="dialog", name="Rename document"),
                                                 focused(role="entry")],
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Ctrl+A, BackSpace empties the dialog field",
                 [event("object:text-changed:delete", role="entry", text_contains="untitled"),
                  said("Selection deleted"), text_events()],
                 should="the deletion is reported and Orca says the selection was deleted"):
        run.key("Ctrl+A", "BackSpace")
    with run.act("type 'x' into the emptied dialog field",
                 [event("object:text-changed:insert", role="entry", text_contains="x"),
                  text_events()],
                 should="the first character typed into the emptied field is reported"):
        run.type("x")
    with run.act("Ctrl+A, type 'y' (control: non-empty replaced by non-empty)",
                 [event("object:text-changed:delete", role="entry", text_contains="x"),
                  said("Selection deleted"), text_events()],
                 should="the replacement is reported and Orca says the selection was deleted"):
        run.key("Ctrl+A")
        run.type("y")
    with run.act("Escape closes the dialog",
                 [not_in_tree(role="dialog"), focused(role="push button", name="Rename…")],
                 tree=True):
        run.key("Escape")


# ---------------------------------------------------------------------------
# A dismissed banner's controls, operated from the keyboard
# ---------------------------------------------------------------------------


def banner_invisible(run):
    run.wait_for(role="entry", state="focused")
    with run.act("Shift+Tab three times to the Unsaved changes dismiss button",
                 [focused(role="push button", name="Clear"), focus_sequence()]):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Space dismisses the banner", [focus_sequence()], tree=True, record=3.0):
        run.key("space")
    with run.act("Tab forward from the dismissed banner's Clear",
                 [focus_sequence()],
                 should="focus moves on to a visible control", tree=True):
        run.key("Tab")
    with run.act("Shift+Tab twice, back into the dismissed banner",
                 [focus_sequence(), focused(role="push button", name="Save now")],
                 should="(records whether the invisible Save now is still a Tab stop)"):
        run.key("Shift+Tab", "Shift+Tab")
    with run.act("Space on the invisible Save now", [focus_sequence()],
                 should="(records whether an invisible control still acts: the example "
                        "prints 'Save now clicked' to app.log)"):
        run.key("space")


# ---------------------------------------------------------------------------
# Space picks a suggestion
# ---------------------------------------------------------------------------


def search_space(run):
    run.wait_for(role="entry", state="focused")
    with run.act("type 'ap'", [in_tree(role="list item", name="Apple")], tree=True,
                 record=3.0):
        run.type("ap")
    with run.act("Down", [event("object:active-descendant-changed"), said("Apple")]):
        run.key("Down")
    with run.act("Space picks Apple",
                 [not_in_tree(role="list box"), in_tree(role="label", name='Filtering: "Apple"'),
                  said("Apple"), text_events()],
                 should="the field takes 'Apple', the list closes and the reader hears "
                        "what was picked", tree=True, record=3.0):
        run.key("space")


# ---------------------------------------------------------------------------
# Caret and selection moves with no text change
# ---------------------------------------------------------------------------


def caret_moves(run):
    run.wait_for(role="entry", state="focused")
    with run.act("Tab to the file entry", [focused(role="entry")]):
        run.key("Tab")
    with run.act("type 'abc'", [event("object:text-changed:insert", role="entry",
                                      text_contains="c")]):
        run.type("abc")
    with run.act("Left",
                 [event("object:text-caret-moved", role="entry"), said("c")],
                 should="the caret moves back one character and the reader hears 'c'"):
        run.key("Left")
    with run.act("Left again",
                 [event("object:text-caret-moved", role="entry"), said("b")],
                 should="the reader hears 'b'"):
        run.key("Left")
    with run.act("Home",
                 [event("object:text-caret-moved", role="entry")],
                 should="the caret moves to the start of the field"):
        run.key("Home")
    with run.act("Shift+End selects the whole text",
                 [event("object:text-selection-changed", role="entry"), said("abc")],
                 should="the selection is reported and the reader hears 'abc selected'"):
        run.key("Shift+End")
    with run.act("Right collapses the selection to its end",
                 [event("object:text-selection-changed", role="entry")],
                 should="the selection's removal is reported"):
        run.key("Right")
    with run.act("Ctrl+A selects all",
                 [event("object:text-selection-changed", role="entry"), said("selected")],
                 should="the selection is reported and the reader hears it"):
        run.key("Ctrl+A")


def caret_moves_dialog_search(run):
    run.wait_for(role="entry", state="focused")
    with run.act("type 'xy' in the search field (no suggestion matches)",
                 [event("object:text-changed:insert", role="entry", text_contains="y")]):
        run.type("xy")
    with run.act("Left in the search field",
                 [event("object:text-caret-moved", role="entry")],
                 should="the caret move is reported"):
        run.key("Left")
    with run.act("Shift+Home in the search field",
                 [event("object:text-selection-changed", role="entry")],
                 should="the selection is reported"):
        run.key("Shift+Home")
    with run.act("Tab three times to Rename…", [focused(role="push button", name="Rename…")]):
        run.key("Tab", "Tab", "Tab")
    with run.act("Space opens the InputDialog (field all selected)",
                 [in_tree(role="dialog", name="Rename document"), focused(role="entry")],
                 record=3.0):
        run.key("space")
    with run.act("End collapses the selection to the end",
                 [event("object:text-selection-changed", role="entry"),
                  said("unselected")],
                 should="the selection's removal is reported ('Text unselected')"):
        run.key("End")
    with run.act("Left in the dialog field",
                 [event("object:text-caret-moved", role="entry")],
                 should="the caret move is reported"):
        run.key("Left")
    with run.act("Shift+Home in the dialog field",
                 [event("object:text-selection-changed", role="entry"), said("selected")],
                 should="the selection is reported and the reader hears it"):
        run.key("Shift+Home")
    with run.act("Escape", [not_in_tree(role="dialog")], tree=True):
        run.key("Escape")


SCENARIOS = [
    Scenario("verify-newkit-caret-dialog", PACKAGE, caret_moves_dialog_search,
             "caret and selection moves in the search field and the InputDialog field"),
    Scenario("verify-newkit-caret", PACKAGE, caret_moves,
             "caret and selection moves in the file entry, with no text change"),
    Scenario("verify-newkit-empty-edits", PACKAGE, empty_edits,
             "first character into / last character out of the file entry and the "
             "InputDialog field"),
    Scenario("verify-newkit-banner-invisible", PACKAGE, banner_invisible,
             "a dismissed banner's controls, still Tab stops, operated by keys"),
    Scenario("verify-newkit-search-space", PACKAGE, search_space,
             "Down then Space picks a suggestion"),
]
