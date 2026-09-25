# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's scenarios for rich-text-editor, beside the sweep's `text.py`.

* verify-text-ctxmenu: the editor's context menu from the keyboard
  (Shift+F10), once with focus arrived by Tab (on the wrapper `section`) and
  once after a screen reader's own focus request on the text node, and what
  focus returns to when the menu closes.
* verify-text-traversal: where Ctrl+Tab and Ctrl+Shift+Tab go after a screen
  reader's focus request put focus on the text node (the sweep's text-16 went
  one way only), and whether a return by keyboard after that is heard.
* verify-text-search: typing a query into the Highlighter's search field:
  whether a reader learns anything of the matches the editor paints.
* verify-text-list-indent: Tab on a list item (the editor's indent), and what
  a reader learns of the new depth.

Helpers come from `scenarios.text` (read, not changed).
"""

from __future__ import annotations

from reader_lib.checks import custom, event, focused
from reader_lib.scenario import Scenario
from scenarios.text import (EDITOR, Facts, focus_by_request, heard, keys_scene, offset_of,
                            press, probe, said_something, scene, set_caret)


def _speech(run):
    def check(act):
        said = heard(run, act)
        return bool(said), [f"{u.stamp} Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                            for u in said] or ["Orca said nothing in this act"]
    return custom("Orca says something", check, needs_orca=True)


def _focus_role(*roles: str):
    def check(act):
        from reader_lib.checks import _focus_node, _is_focus
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change in this act"]
        node = _focus_node(moves[-1])
        return node.get("role") in roles, [f"focus on [{node.get('role')}] {node.get('name')!r}"]
    return custom(f"focus lands on one of {roles}", check)


def ctxmenu(run):
    run.wait_for(**EDITOR)
    with run.act("Tab from the search field into the editor",
                 [_focus_role("entry", "section")], should="focus arrives in the editor"):
        press(run, "Tab")
    with run.act("Shift+F10 in the editor (focus arrived by Tab)",
                 [_focus_role("menu item", "menu"), _speech(run)],
                 should="the editor's context menu opens and the reader hears its first item"):
        press(run, "Shift+F10")
    with run.act("Down in the context menu", [_speech(run)],
                 should="the next menu item is read"):
        press(run, "Down")
    with run.act("Escape closes the menu", [_speech(run)],
                 should="focus returns to the editor and the reader hears where it is"):
        press(run, "Escape")
    run.note(f"after Escape (Tab arrival), focus: {run.last_focus()}")
    focus_by_request(run)
    with run.act("Shift+F10 after a focus request on the text",
                 [_focus_role("menu item", "menu"), _speech(run)],
                 should="the editor's context menu opens and the reader hears its first item"):
        press(run, "Shift+F10")
    with run.act("Escape closes the menu again", [_speech(run)],
                 should="focus returns to the editor's text and the reader hears it"):
        press(run, "Escape")
    run.note(f"after Escape (request arrival), focus: {run.last_focus()}")


def traversal(run):
    run.wait_for(**EDITOR)
    focus_by_request(run)
    with run.act("Ctrl+Tab after a focus request on the text",
                 [focused(role="separator", name="Splitter divider"), _speech(run)],
                 should="focus moves on to the Splitter divider, as it does after Tab arrival"):
        press(run, "Ctrl+Tab")
    run.note(f"after Ctrl+Tab, focus: {run.last_focus()}")
    focus_by_request(run)
    with run.act("Ctrl+Shift+Tab after a focus request on the text",
                 [focused(role="entry"), _speech(run)],
                 should="focus moves back to the search field (the control before the editor)"):
        press(run, "Ctrl+Shift+Tab")
    run.note(f"after Ctrl+Shift+Tab, focus: {run.last_focus()}")
    with run.act("Tab from the search field into the editor, after a focus request earlier",
                 [_speech(run)], should="the reader hears the editor"):
        press(run, "Tab")
    run.note(f"after Tab, focus: {run.last_focus()}")
    facts = Facts()
    facts["entry"] = probe(run, EDITOR, selection=True)
    run.note(f"entry states after Tab: {facts['entry'].get('states')}")
    with run.act("Down after Tab arrival (Orca told of the key)",
                 [event("object:text-caret-moved", role="entry"), _speech(run)],
                 should="the caret moves and the reader hears the line"):
        press(run, "Down")


def search(run):
    run.wait_for(**EDITOR)
    run.note(f"focus at launch: {run.last_focus()}")
    with run.act("select the query and type 'teh' (Orca told of each key)",
                 [custom("an announcement or a status tells how many matches",
                         lambda act: (any(e["type"] == "object:announcement"
                                          for e in act.events),
                                      [f"{e['type']} {e.get('text')!r}" for e in act.events
                                       if e["type"] in ("object:announcement",
                                                        "object:property-change:accessible-name")]
                                      or ["no announcement and no name change"])),
                  _speech(run)],
                 should="the reader learns how many matches the editor now shows"):
        press(run, "Ctrl+A", "t", "e", "h")
    at = offset_of(run, EDITOR, "recieve teh ") + len("recieve ")
    facts = Facts()
    facts["match"] = probe(run, EDITOR, attrs=True, at=at + 1)
    run.note(f"attributes at the 'teh' match: {facts['match'].get('attrs')}")
    with run.act("the match, as the Text interface answers",
                 [custom("the attribute run at the match says it is highlighted",
                         lambda act: (bool((facts.get_path("match", "attrs", "attrs") or {})),
                                      [f"{facts.get('match', {}).get('attrs')}"]))],
                 should="a reader asking for the text's attributes finds the match", record=0.2):
        pass


def list_indent(run):
    run.wait_for(**EDITOR)
    focus_by_request(run)
    start = offset_of(run, EDITOR, "Second item, mixing bold")
    set_caret(run, EDITOR, start)
    facts = Facts()
    facts["before"] = probe(run, EDITOR, grans=["line"], text=[start, start + 20])
    with run.act("Tab on a list item: indent it (Orca told of the key)",
                 [_speech(run),
                  custom("something on the bus changes for the new depth",
                         lambda act: (bool([e for e in act.events
                                            if not e["type"].startswith("object:bounds")
                                            and e["type"] != "object:text-caret-moved"]),
                                      [f"{e['type']} [{e['source'].get('role')}]"
                                       for e in act.events
                                       if not e["type"].startswith("object:bounds")]
                                      or ["no event"]))],
                 should="the item moves one level deeper and the reader hears the new level"):
        press(run, "Tab")
    facts["after"] = probe(run, EDITOR, grans=["line"], text=[start, start + 20])
    run.note(f"list item before: {facts['before'].get('line')} / after: "
             f"{facts['after'].get('line')} count {facts['before'].get('count')} -> "
             f"{facts['after'].get('count')}")
    keys_scene(run, "Shift+Tab")


def ctxmenu_caret(run):
    """Where Shift+F10 leaves the caret, and where Paste from that menu writes.

    `open_context_menu_from_keyboard` (teksilo-core `pointer_router.rs`)
    anchors a keyboard-opened menu at the centre of the focused widget, and
    the editor's default factory (`rich_text/context_menu.rs`,
    `default_factory`) moves the caret to the anchor as a right-click would."""
    run.wait_for(**EDITOR)
    focus_by_request(run)
    keys_scene(run, "Ctrl+Home", "Shift+Ctrl+Right", "Ctrl+C", "Ctrl+Home")
    facts = Facts()
    facts["before"] = probe(run, EDITOR, grans=["line"])
    run.note(f"caret before Shift+F10: {facts['before'].get('caret')} line "
             f"{facts.get_path('before', 'line', 'text')!r}")
    with run.act("Shift+F10 with the caret at the document's start",
                 [_focus_role("menu item", "menu")],
                 should="the context menu opens; the caret stays at 0"):
        press(run, "Shift+F10")
    with run.act("Escape without choosing anything",
                 [custom("the caret is where it was before the menu (0)",
                         lambda act: (facts.get_path("after escape", "caret") == 0,
                                      [f"caret {facts.get_path('after escape', 'caret')}, line "
                                       f"{facts.get_path('after escape', 'line', 'text')!r}"])),
                  _speech(run)],
                 should="the menu closes and the caret is still at the start"):
        press(run, "Escape")
    facts["after escape"] = probe(run, EDITOR, grans=["line"])
    run.note(f"caret after Shift+F10, Escape: {facts['after escape'].get('caret')}")
    keys_scene(run, "Ctrl+Home")
    facts["before paste"] = probe(run, EDITOR)
    run.note(f"caret before the menu's Paste: {facts['before paste'].get('caret')}")
    with run.act("Shift+F10 again, caret at 0", [], should="the context menu opens"):
        press(run, "Shift+F10")
    with run.act("p: the menu's Paste (mnemonic or type-ahead)",
                 [custom("the paste goes in at the caret (offset 0)",
                         lambda act: (any(e["type"] == "object:text-changed:insert"
                                          and e.get("detail1") == 0 for e in act.events),
                                      [f"{e['type']} at {e.get('detail1')} "
                                       f"[{e['source'].get('role')}] {e.get('text')!r}"
                                       for e in act.events
                                       if e["type"].startswith("object:text-changed")]
                                      or ["no text change"]))],
                 should="Paste inserts the copied word where the caret was"):
        press(run, "p")
        run.wait(0.8)
        if run.find(role="menu"):
            press(run, "Return")
    facts["pasted"] = probe(run, EDITOR, grans=["line"], text=[0, 30])
    run.note(f"after the menu's Paste: caret {facts['pasted'].get('caret')}, text 0..30 "
             f"{facts['pasted'].get('text')!r}, line {facts.get_path('pasted', 'line', 'text')!r}")
    keys_scene(run, "Ctrl+Home", "Shift+Ctrl+Right")
    facts["selected"] = probe(run, EDITOR, selection=True)
    run.note(f"selection before Shift+F10: {facts['selected'].get('selections')}")

    def copy_enabled(act):
        from reader_lib.checks import _walk
        rows = [n for n in _walk(act.tree) if n.get("role") == "menu item"
                and "Copy" in (n.get("name") or "")]
        return (bool(rows) and all("enabled" in n.get("states", []) for n in rows),
                [f"[menu item] {n.get('name')!r} states={n.get('states')}" for n in rows]
                or ["no Copy row in the tree"])

    with run.act("Shift+F10 with the first word selected",
                 [custom("the menu's Copy is enabled (the selection survives)", copy_enabled,
                         needs_tree=True)],
                 should="the menu opens over the selection; Copy and Cut act on it", tree=True):
        press(run, "Shift+F10")
    with run.act("Escape from the menu over a selection",
                 [custom("the selection is still the first word",
                         lambda act: (facts.get_path("after sel menu", "selections")
                                      == facts.get_path("selected", "selections"),
                                      [f"before {facts.get_path('selected', 'selections')}, after "
                                       f"{facts.get_path('after sel menu', 'selections')}, caret "
                                       f"{facts.get_path('after sel menu', 'caret')}"]))],
                 should="the menu closes and the selection is untouched"):
        press(run, "Escape")
    facts["after sel menu"] = probe(run, EDITOR, selection=True)
    run.note(f"selection after Shift+F10, Escape: {facts['after sel menu'].get('selections')} "
             f"caret {facts['after sel menu'].get('caret')}")


def ctxmenu_field(run):
    """The same for a single-line `TextInput` (ime-playground), whose field
    repositions its caret the same way (`text_input_field/widget_impl.rs`,
    `.context_menu`). Its caret moves reach no reader (the sweep's text-09), so
    where the caret went is read from where the next typed letter lands."""
    field = {"role": "entry", "state": "focused"}
    run.wait_for(**field)
    scene(run, "type 60 letters", lambda: run.type("abcdefghij" * 6))
    keys_scene(run, "Home")
    with run.act("Shift+F10 in the TextInput, caret at its start", [_focus_role("menu", "menu item")],
                 should="the field's context menu opens"):
        press(run, "Shift+F10")
    keys_scene(run, "Escape")
    with run.act("type x after closing the menu",
                 [custom("x goes in at the start of the field (offset 0), where the caret was",
                         lambda act: (any(e["type"] == "object:text-changed:insert"
                                          and e.get("detail1") == 0 for e in act.events),
                                      [f"{e['type']} at {e.get('detail1')} {e.get('text')!r}"
                                       for e in act.events
                                       if e["type"].startswith("object:text-changed")]
                                      or ["no text change"]))],
                 should="the caret did not move, so x is inserted at 0"):
        press(run, "x")
    facts = Facts()
    facts["x"] = probe(run, field, text=[0, 70])
    run.note(f"field text after x: {facts['x'].get('text')!r}")


SCENARIOS = [
    Scenario("verify-text-ctxmenu-field", "ime-playground", ctxmenu_field,
             "Shift+F10 in a single-line TextInput: where the caret goes"),
    Scenario("verify-text-ctxmenu-caret", "rich-text-editor", ctxmenu_caret,
             "where Shift+F10 leaves the caret, and where the menu's Paste writes"),
    Scenario("verify-text-ctxmenu", "rich-text-editor", ctxmenu,
             "Shift+F10 in the editor after Tab arrival and after a focus request"),
    Scenario("verify-text-traversal", "rich-text-editor", traversal,
             "Ctrl+Tab / Ctrl+Shift+Tab after a focus request on the editor's text"),
    Scenario("verify-text-search", "rich-text-editor", search,
             "typing a query into the Highlighter's search field"),
    Scenario("verify-text-list-indent", "rich-text-editor", list_indent,
             "Tab (indent) on a list item"),
]
