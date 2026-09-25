# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The CodeEditor's way out, for the `code-editor-trap` fix (console-02).

Tab indents in a code editor, so it cannot be how a keyboard user leaves one.
Ctrl+Tab and Ctrl+Shift+Tab are, as they are out of the terminal and a
rich-text table: `code_editor/keyboard.rs` leaves both to the tree's focus
traversal, and the editor's text node names them in its description
(`code_editor/a11y.rs`, `build_editor_a11y`).

The example has two tab stops, the Theme combo box in the status bar and the
editor, so a move out of the editor in either direction lands on the combo
box. Where keyboard focus lands inside the editor is another finding
(console-01: the wrapper, reported as [unknown] ''), so these scenarios accept
either the wrapper or the [entry] as "in the editor", and the hint is read
with the reader's own focus request on the [entry], which reaches it today.
"""

from __future__ import annotations

import time

from reader_lib.checks import _is_focus, _focus_node, custom, event, focused, in_tree, no_event, said
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

#: The roles focus can have inside the editor: its wrapper, or its text node.
IN_EDITOR = ("unknown", "entry")

#: The status bar's combo box, the only other tab stop.
THEME = {"role": "combo box", "name": "Theme"}


def tab_into_editor(run, limit: int = 8) -> None:
    """Scene setting: press Tab until the bus reports focus in the editor."""
    for _ in range(limit):
        if run.last_focus().get("role") in IN_EDITOR:
            return
        run.key("Tab")
        time.sleep(0.4)
    if run.last_focus().get("role") not in IN_EDITOR:
        raise RunError(f"{limit} x Tab never reached the editor; last focus {run.last_focus()}")


def stays_in_editor():
    """No focus change in the act lands outside the editor."""
    def check(act):
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        out = [m for m in moves if m.get("role") not in IN_EDITOR]
        return not out, [f"focus moved to [{m.get('role')}] {m.get('name')!r}" for m in out] \
            or ["no focus change outside the editor"]
    return custom("focus stays in the editor", check)


def keys(run):
    with run.act("scene: Tab into the editor", should="scene setting, not judged"):
        tab_into_editor(run)
    with run.act("Ctrl+Tab in the editor",
                 [focused(**THEME), no_event("object:text-changed")],
                 should="focus leaves the editor for the next control, the Theme combo box, "
                        "and nothing is written into the document"):
        run.key("Ctrl+Tab")
    with run.act("scene: Tab back into the editor", should="scene setting, not judged"):
        tab_into_editor(run)
    with run.act("Ctrl+Shift+Tab in the editor",
                 [focused(**THEME), no_event("object:text-changed")],
                 should="focus leaves the editor backwards, for the Theme combo box, and "
                        "no line is dedented"):
        run.key("Ctrl+Shift+Tab")
    with run.act("scene: Tab back into the editor", should="scene setting, not judged"):
        tab_into_editor(run)
    with run.act("Tab in the editor",
                 [event("object:text-changed:insert", role="entry"), stays_in_editor()],
                 should="Tab still indents: four spaces go in at the caret, focus stays"):
        run.key("Tab")
    with run.act("Shift+Tab in the editor",
                 [event("object:text-changed:delete", role="entry"), stays_in_editor()],
                 should="Shift+Tab still dedents: the four spaces come out, focus stays"):
        run.key("Shift+Tab")


def hint(run):
    with run.act("read the editor's text node",
                 [in_tree(role="entry", description_contains="Ctrl+Tab"),
                  in_tree(role="entry", description_contains="Ctrl+Shift+Tab")],
                 should="the [entry] carries a description naming Ctrl+Tab and Ctrl+Shift+Tab",
                 tree=True):
        pass
    with run.act("AT-SPI grab_focus on the [entry]",
                 [focused(role="entry"), said("Ctrl+Tab moves to the next control")],
                 should="the reader lands on the editor's text and hears how to leave it",
                 record=3.0):
        run.grab_focus(role="entry")
    with run.act("Ctrl+Tab from the [entry]",
                 [focused(**THEME), no_event("object:text-changed")],
                 should="the chord the reader was told moves focus out, and writes nothing"):
        run.key("Ctrl+Tab")


SCENARIOS = [
    Scenario("fix-code-editor-trap-keys", "code_editor", keys,
             "Ctrl+Tab and Ctrl+Shift+Tab leave the editor; Tab and Shift+Tab still indent"),
    Scenario("fix-code-editor-trap-hint", "code_editor", hint,
             "the editor's text node tells a reader the way out, and it works"),
]
