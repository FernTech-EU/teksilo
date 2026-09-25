# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""fix-editor-focus: the keyboard's way into a multi-line text surface.

The sweep found `RichTextEditor`, `CodeEditor` and `LogView` putting focus on
the wrapper widget that takes the keys, an unnamed [section] or [unknown] with
no text, while the text lived in a child (catalog-b-02, text-01, console-01).
Orca said "section." or nothing, and no caret move ever reached the bus,
because `accesskit_atspi_common` reports one only for the focused node
(`adapter.rs:215-218`). Nothing could name the text node either (text-07,
console-09).

Each scenario Tabs into a surface and checks that focus lands on the node that
holds the text, under the name the example gives it with `.label(..)`, that
Orca reads that name and the text, and that an arrow key moves a caret the bus
reports and Orca reads. The examples name their surfaces "Editor" and
"Preview" (rich-text-editor), "Sample document" (rich-text-viewer), "Code"
(code_editor) and "Log" (log_view).

Keys go through `scenarios.text.press`, which reports each key to the registry
as a toolkit bridge does before pressing it for real, so Orca knows which key
moved the caret and says the line (see `scenarios/text.py`, "Orca and the
keys").
"""

from __future__ import annotations

import time

from reader_lib.checks import event, focused, said
from reader_lib.run import RunError
from reader_lib.scenario import Scenario
from scenarios.text import Facts, nav_act, press

EDITOR = {"role": "entry", "name": "Editor"}
PREVIEW = {"role": "document frame", "name": "Preview"}
VIEWER = {"role": "document frame", "name": "Sample document"}
CODE = {"role": "entry", "name": "Code"}
LOG = {"role": "document frame", "name": "Log"}


def tab_to(run, role: str, name: str, limit: int = 10) -> None:
    """Scene setting: Tab until the bus reports focus on [role] `name`,
    leaving Orca time to finish each control's name (and the toolbar's, on the
    way in) before the next focus stops it, so the scene records no cut
    speech."""
    for _ in range(limit):
        node = run.last_focus()
        if node.get("role") == role and node.get("name") == name:
            return
        run.key("Tab")
        time.sleep(3.5)
    node = run.last_focus()
    if node.get("role") != role or node.get("name") != name:
        raise RunError(f"{limit} x Tab never focused [{role}] {name!r}; last focus {node}")


def rich_editor(run) -> None:
    """rich-text-editor: Tab from the search field into the editor, Down, over
    to the preview, Down, and back."""
    run.wait_for(role="entry", state="multi-line")
    facts = Facts()
    with run.act("Tab from the search field into the editor",
                 [focused(role="entry", name="Editor"), said("Editor"), said("entry"),
                  said("RichTextEditor")],
                 should="focus lands on the editor's text; the reader hears its name, "
                        "that it is an editable text, and the line at the caret"):
        press(run, "Tab")
    nav_act(run, facts, "Down in the editor (Orca told of the key)", "Down", "line",
            "the caret moves to the next line, the bus reports it, and the reader "
            "hears the line", spec=EDITOR)
    with run.act("scene: Ctrl+Tab out to the splitter", should="scene setting, not judged"):
        press(run, "Ctrl+Tab")
    with run.act("Tab into the preview",
                 [focused(role="document frame", name="Preview"), said("Preview")],
                 should="focus lands on the preview's document, and the reader hears "
                        "its name"):
        press(run, "Tab")
    nav_act(run, facts, "Down in the preview (Orca told of the key)", "Down", "line",
            "the caret moves to the next line and the reader hears it", spec=PREVIEW,
            role="document frame")
    with run.act("scene: Ctrl+Shift+Tab out to the splitter",
                 should="scene setting, not judged"):
        press(run, "Ctrl+Shift+Tab")
    with run.act("Ctrl+Shift+Tab back into the editor",
                 [focused(role="entry", name="Editor"), said("Editor")],
                 should="focus comes back to the editor, and the reader hears it again"):
        press(run, "Ctrl+Shift+Tab")
    nav_act(run, facts, "Down after coming back (Orca told of the key)", "Down", "line",
            "the caret moves and the reader hears the line", spec=EDITOR)


def rich_viewer(run) -> None:
    """rich-text-viewer: Tab past the Theme combo box into the document."""
    run.wait_for(role="document frame")
    facts = Facts()
    with run.act("scene: Tab to the Theme combo box", should="scene setting, not judged"):
        tab_to(run, "combo box", "Theme")
    with run.act("Tab into the viewer",
                 [focused(role="document frame", name="Sample document"),
                  said("Sample document"), said("Teksilo Rich Text Viewer")],
                 should="focus lands on the document; the reader hears its name and "
                        "the line at the caret"):
        press(run, "Tab")
    nav_act(run, facts, "Down in the viewer (Orca told of the key)", "Down", "line",
            "the caret moves to the next line and the reader hears it", spec=VIEWER,
            role="document frame")


def code_editor(run) -> None:
    """code_editor: Tab past the Theme combo box into the editor, then Down."""
    run.wait_for(role="entry", state="multi-line")
    facts = Facts()
    with run.act("scene: Tab to the Theme combo box", should="scene setting, not judged"):
        tab_to(run, "combo box", "Theme")
    with run.act("Tab into the editor",
                 [focused(role="entry", name="Code"), said("Code"), said("entry"),
                  said("A little Teksilo widget")],
                 should="focus lands on the editor's text; the reader hears its name, "
                        "that it is an editable text, and the line at the caret"):
        press(run, "Tab")
    nav_act(run, facts, "Down in the editor (Orca told of the key)", "Down", "line",
            "the caret moves to the next line, the bus reports it, and the reader "
            "hears the line", spec=CODE)
    with run.act("type Z in the editor",
                 [event("object:text-changed:insert", role="entry", text_contains="Z"),
                  event("object:text-caret-moved", role="entry")],
                 should="the letter goes in at the caret and the caret moves on"):
        press(run, "Z")


def log_view(run) -> None:
    """log_view: Tab past the Theme combo box into the log."""
    run.wait_for(role="document frame")
    with run.act("scene: Tab to the Theme combo box", should="scene setting, not judged"):
        tab_to(run, "combo box", "Theme")
    with run.act("Tab into the log",
                 [focused(role="document frame", name="Log"), said("Log"),
                  said("[00:00:00")],
                 should="focus lands on the log's document; the reader hears its name "
                        "and the line at the caret"):
        press(run, "Tab")


SCENARIOS = [
    Scenario("fix-editor-focus-rich", "rich-text-editor", rich_editor,
             "Tab into the rich text editor and its preview: name, text, caret"),
    Scenario("fix-editor-focus-viewer", "rich-text-viewer", rich_viewer,
             "Tab into the read-only viewer: name, text, caret"),
    Scenario("fix-editor-focus-code", "code_editor", code_editor,
             "Tab into the code editor: name, text, caret, typing"),
    Scenario("fix-editor-focus-log", "log_view", log_view,
             "Tab into the log: name and text"),
]
