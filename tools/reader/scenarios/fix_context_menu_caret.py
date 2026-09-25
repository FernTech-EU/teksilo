# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""fix-context-menu-caret: the context menu opened from the keyboard acts
where the reader left the caret.

Shift+F10 opens the menu a right-click opens, with no click behind it. The
framework anchors that menu in the middle of the focused widget, and the text
surfaces used to move their caret to the anchor as a right-click moves it to
the click: in the editor the caret jumped from 0 to the middle of the pane and
the menu's Paste wrote there, a selection collapsed ("Text unselected."), and
in a single-line field the caret moved too. Closing the field's menu then
selected the whole field, so the next key replaced everything in it.

These acts state what the reader should get, in words the sweep's own
`verify-text-ctxmenu-*` scenarios could not: the copied word here differs from
the text at the caret, so an insertion at 0 cannot be mistaken for one after a
shared prefix, and the field's return from its menu is judged by the
selection event, not by where the next letter lands.
"""

from __future__ import annotations

from typing import Any

from reader_lib.checks import custom, event, no_event, not_said
from reader_lib.scenario import Scenario
from scenarios.text import EDITOR, Facts, focus_by_request, keys_scene, offset_of, probe, press, \
    scene, set_caret, value_is

FIELD = {"role": "entry", "state": "single-line"}


def _caret_only_at(offset: int, role: str = "entry") -> Any:
    """Every caret event the act raised on `role` reports `offset` (the
    adapter re-announces the caret when focus comes back, which is fine)."""
    def check(act: Any) -> tuple[bool, list[str]]:
        moves = [e for e in act.events if e["type"] == "object:text-caret-moved"
                 and e.get("source", {}).get("role") == role]
        bad = [e for e in moves if e.get("detail1") != offset]
        return not bad, [f"caret moved to {e.get('detail1')}" for e in moves] \
            or ["no caret event"]
    return custom(f"no caret event on [{role}] leaves offset {offset}", check)


def editor(run: Any) -> None:
    run.wait_for(**EDITOR)
    focus_by_request(run)
    facts = Facts()
    word_at = offset_of(run, EDITOR, "squiggle")
    set_caret(run, EDITOR, word_at)
    keys_scene(run, "Shift+Ctrl+Right")
    facts["copied"] = probe(run, EDITOR, selection=True)
    start, end = facts["copied"]["selections"][0]
    copied = probe(run, EDITOR, text=[start, end])["text"]
    run.note(f"copied {copied!r} from {start}..{end}")
    keys_scene(run, "Ctrl+C", "Ctrl+Home")
    facts["start"] = probe(run, EDITOR, text=[0, 40])
    run.note(f"caret {facts['start'].get('caret')}, text {facts['start'].get('text')!r}")

    with run.act("Shift+F10 with the caret at 0",
                 [no_event("object:text-caret-moved", role="entry"),
                  no_event("object:text-selection-changed", role="entry"),
                  value_is("the caret is still at 0 with the menu open",
                           lambda: facts.get_path("menu open", "caret"), 0)],
                 should="the menu opens and the caret stays where the reader put it"):
        press(run, "Shift+F10")
    facts["menu open"] = probe(run, EDITOR)

    with run.act("p, Return: the menu's Paste",
                 [event("object:text-changed:insert", role="entry", detail1=0,
                        text_contains=copied),
                  value_is("the document now begins with the pasted word",
                           lambda: (facts.get_path("pasted", "text") or "")
                           .startswith(copied + (facts.get_path("start", "text") or "")[:10]),
                           True),
                  value_is("the caret follows the paste",
                           lambda: facts.get_path("pasted", "caret"), len(copied))],
                 should="Paste inserts the copied word at 0, where the caret was"):
        press(run, "p")
        run.wait(0.8)
        if run.find(role="menu"):
            press(run, "Return")
    facts["pasted"] = probe(run, EDITOR, text=[0, 60])
    run.note(f"after Paste: caret {facts['pasted'].get('caret')}, "
             f"text {facts['pasted'].get('text')!r}")

    keys_scene(run, "Ctrl+Home", "Shift+Ctrl+Right")
    facts["selected"] = probe(run, EDITOR, selection=True)
    run.note(f"selection before Shift+F10: {facts['selected'].get('selections')}")
    with run.act("Shift+F10 over a selection",
                 [no_event("object:text-selection-changed", role="entry"),
                  no_event("object:text-caret-moved", role="entry"),
                  not_said("unselected")],
                 should="the menu opens over the selection, which Cut and Copy act on"):
        press(run, "Shift+F10")
    with run.act("Escape from the menu over a selection",
                 [not_said("unselected"),
                  value_is("the selection is the one the reader made",
                           lambda: facts.get_path("after", "selections"),
                           facts.get_path("selected", "selections"))],
                 should="focus comes back to the editor with the selection untouched"):
        press(run, "Escape")
    facts["after"] = probe(run, EDITOR, selection=True)
    run.note(f"selection after Shift+F10, Escape: {facts['after'].get('selections')}")


def field(run: Any) -> None:
    run.wait_for(role="entry", state="focused")
    scene(run, "type 60 letters", lambda: run.type("abcdefghij" * 6))
    keys_scene(run, "Home")
    facts = Facts()

    with run.act("Shift+F10 in the TextInput, caret at 0",
                 [_caret_only_at(0),
                  value_is("the caret is still at 0 with the menu open",
                           lambda: facts.get_path("menu open", "caret"), 0)],
                 should="the field's menu opens and its caret stays at 0"):
        press(run, "Shift+F10")
    facts["menu open"] = probe(run, FIELD)

    with run.act("Escape from the field's menu",
                 [no_event("object:text-selection-changed", role="entry"),
                  _caret_only_at(0),
                  not_said("selected"),
                  value_is("nothing is selected when focus comes back",
                           lambda: facts.get_path("back", "selections"), [])],
                 should="focus comes back to the field as the reader left it"):
        press(run, "Escape")
    facts["back"] = probe(run, FIELD, selection=True)

    with run.act("type x",
                 [event("object:text-changed:insert", role="entry", detail1=0, text_contains="x"),
                  no_event("object:text-changed:delete", role="entry"),
                  value_is("the field holds x and the sixty letters",
                           lambda: facts.get_path("x", "text"), "x" + "abcdefghij" * 6)],
                 should="x goes in at 0 and nothing is replaced"):
        press(run, "x")
    facts["x"] = probe(run, FIELD, text=[0, 70])


SCENARIOS = [
    Scenario("fix-context-menu-caret-editor", "rich-text-editor", editor,
             "Shift+F10 in the editor: the caret and the selection stay, Paste writes at the caret"),
    Scenario("fix-context-menu-caret-field", "ime-playground", field,
             "Shift+F10 then Escape in a TextInput: the caret stays, nothing is selected"),
]
