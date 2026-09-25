# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for widget-catalog part B (the sweep's own acts are
in `catalog_b.py`; these only add what its scenarios did not do).

* `verify-catalog-b-reopen-unmet`: the Text page hidden and shown before the
  reader ever Tabs into it, then a Tab walk: are the controls Orca never met
  spoken, and the ones it met (Username, focused at launch) not? Tells apart
  "every control on a reopened page is silent" from "every control Orca had
  already met is silent".
* `verify-catalog-b-datetime-reopen`: the Date & Time page walked, reopened and
  walked again to the end, past the DateTimeEdit and DateRangeEdit, which the
  sweep's second walk (14 stops) never reached.
* `verify-catalog-b-richtext-tab`: what Tab does inside the rich-text editor
  and the read-only viewer, and whether anything tells a reader how to leave.
* `verify-catalog-b-line-marks`: the line chart's marks by keyboard (the
  sweep drove only the first bar chart and the pie).
* `verify-catalog-b-coloredit-popover`: the ColorEdit's popover opened by
  keyboard: what a reader hears on opening, inside, and on closing.
"""

from __future__ import annotations

import subprocess
from contextlib import contextmanager
from typing import Any, Callable, Iterator

from reader_lib.checks import (_focus_node, _is_focus, _walk, custom, event, focused,
                               in_tree, not_in_tree, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

PKG = "widget-catalog"

CHROME_NAMES = {"Menu", "English", "Français", "العربية", "Text scale", "Theme",
                "Minimize", "Maximize", "Close"}


@contextmanager
def held_seat(run: Any) -> Iterator[None]:
    """One fake-input client connected for the whole scenario (the sweep's
    workaround for winit's `failed to get pointer data` panic when the seat's
    pointer capability comes and goes with each key press)."""
    holder = None
    if run.fake_key is not None:
        holder = subprocess.Popen([str(run.fake_key), "wait:1800000"],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        run.wait(0.8)
    try:
        yield
    finally:
        if holder is not None and holder.poll() is None:
            holder.terminate()
            try:
                holder.wait(timeout=5)
            except subprocess.TimeoutExpired:
                holder.kill()


def held(body: Callable[[Any], None]) -> Callable[[Any], None]:
    def wrapped(run: Any) -> None:
        with held_seat(run):
            body(run)
    return wrapped


def _last_focus(act: Any) -> dict:
    moves = [e for e in act.events if _is_focus(e)]
    return _focus_node(moves[-1]) if moves else {}


def _spoken(act: Any) -> list[str]:
    return [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in utterances(act.orca)]


def record_focus() -> Any:
    """Record every focus landing of the act, in order (always passes)."""
    def run(act: Any) -> tuple[bool, list[str]]:
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                      or "no focus change"]
    return custom("the focus landings of the act, in order (a record)", run)


def stop_spoken() -> Any:
    def run(act: Any) -> tuple[bool, list[str]]:
        node = _last_focus(act)
        name = (node.get("name") or "").strip()
        evidence = _spoken(act)
        if not name:
            return False, [f"focus on an unnamed [{node.get('role')}]"] + evidence
        heard = [u for u in utterances(act.orca)
                 if normalized(name) in normalized(u.text) and not u.cut]
        return bool(heard), evidence or ["Orca said nothing"]
    return custom("Orca says the stop's name", run, needs_orca=True)


def said_something() -> Any:
    def run(act: Any) -> tuple[bool, list[str]]:
        said_ = [u for u in utterances(act.orca) if not u.cut]
        return bool(said_), _spoken(act) or ["Orca said nothing"]
    return custom("Orca says something", run, needs_orca=True)


def node_states(describe: str, match: Callable[[dict], bool]) -> Any:
    """Record the states, description and relations of every matching node
    after the act (passes when at least one matched)."""
    def run(act: Any) -> tuple[bool, list[str]]:
        found = [n for n in _walk(act.tree) if match(n)]
        return bool(found), [
            f"[{n.get('role')}] {n.get('name')!r} states={n.get('states')} "
            f"desc={n.get('description')!r} rel={n.get('relations')} "
            f"attrs={n.get('attributes')}" for n in found] or ["no such node"]
    return custom(describe, run, needs_tree=True)


def tab_until(run: Any, limit: int = 40, **want: Any) -> dict:
    for _ in range(limit):
        run.key("Tab")
        run.wait(0.35)
        node = run.last_focus()
        if want.get("role") and node.get("role") != want["role"]:
            continue
        if "name" in want and (node.get("name") or "") != want["name"]:
            continue
        if "name_startswith" in want and not (node.get("name") or "").startswith(
                want["name_startswith"]):
            continue
        return node
    from reader_lib.run import RunError
    raise RunError(f"Tab never reached {want}")


def walk(run: Any, stops: int, label: str) -> list[dict]:
    seen: set[str] = set()
    landed: list[dict] = []
    press = "Tab"
    for i in range(stops):
        with run.act(f"{label} {i + 1} ({press})", [stop_spoken()],
                     should="focus moves to the next control and the reader says what it is",
                     settle=0.4, record=1.4) as act:
            run.key(press)
        run.collect_events(act)
        node = _last_focus(act)
        typed = any(e["type"].startswith("object:text-changed:insert")
                    and e.get("text") == "\t" for e in act.events)
        press = "Ctrl+Tab" if (typed or not node) else "Tab"
        landed.append(node)
        path = node.get("path")
        if path and path in seen:
            break
        if path:
            seen.add(path)
        if node.get("role") == "status bar" or (node.get("name") or "") in CHROME_NAMES:
            break
    run.note(f"{label} stops: " + " | ".join(
        f"[{n.get('role')}] {n.get('name')!r}" for n in landed))
    return landed


def reopen(run: Any, title: str, previous: str) -> None:
    run.grab_focus(role="page tab", name=title)
    run.wait(0.6)
    with run.act(f"Up to the {previous} tab", [focused(role="page tab", name=previous)],
                 should=f"the {previous} page opens"):
        run.key("Up")
    with run.act(f"Down back to the {title} tab", [focused(role="page tab", name=title)],
                 should=f"the {title} page opens", tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------


def reopen_unmet(run: Any) -> None:
    """Username is focused at launch (Orca meets it); nothing else on the page
    is. Hide and show the page, then walk it."""
    run.wait_for(role="page tab", name="Text")
    reopen(run, "Text", "Scene")
    walk(run, 12, "after reopening, Tab")


def datetime_reopen(run: Any) -> None:
    run.wait_for(role="page tab", name="Date & Time")
    run.grab_focus(role="page tab", name="Date & Time")
    run.wait(0.6)
    walk(run, 30, "first walk, Tab")
    reopen(run, "Date & Time", "Rich Text")
    walk(run, 30, "after reopening, Tab")


def richtext_tab(run: Any) -> None:
    run.wait_for(role="page tab", name="Rich Text")
    run.grab_focus(role="page tab", name="Rich Text")
    run.wait(0.6)
    with run.act("Tab into the editor",
                 [record_focus(),
                  node_states("the focused wrapper and the editor's text node",
                              lambda n: n.get("role") in ("section", "entry", "document frame")
                              and n.get("name", "") == "")],
                 should="focus lands on the editor", tree=True):
        for _ in range(8):
            run.key("Tab")
            run.wait(0.35)
            node = run.last_focus()
            if node.get("role") in ("entry", "section", "document frame"):
                break
    with run.act("Tab inside the editor",
                 [record_focus(), event("object:text-changed:insert", text_contains="\t")],
                 should="a reader learns that Tab is kept by the editor, or focus moves on"):
        run.key("Tab")
    with run.act("Shift+Tab inside the editor", [record_focus()],
                 should="focus goes back, or the reader learns why not"):
        run.key("Shift+Tab")
    with run.act("Ctrl+Tab out of the editor", [record_focus(), said_something()],
                 should="focus leaves the editor for the viewer"):
        run.key("Ctrl+Tab")
    with run.act("Ctrl+Shift+Tab back to the editor", [record_focus(), said_something()],
                 should="focus comes back to the editor and the reader hears it again",
                 tree=True):
        run.key("Ctrl+Shift+Tab")
    with run.act("Ctrl+Tab to the viewer again", [record_focus(), said_something()],
                 should="focus goes to the viewer and the reader hears it again"):
        run.key("Ctrl+Tab")
    with run.act("Tab in the read-only viewer", [record_focus(), said_something()],
                 should="Tab leaves a read-only viewer for the next control"):
        run.key("Tab")


def line_marks(run: Any) -> None:
    run.wait_for(role="page tab", name="Charts")
    run.grab_focus(role="page tab", name="Charts")
    run.wait(0.6)
    with run.act("Tab to the line chart", [focused(role="document frame",
                                                   name_contains="Line chart")],
                 should="the line chart is reached", tree=True):
        tab_until(run, role="document frame", name_startswith="Line chart")
    with run.act("Right to the first point", [record_focus(), said("Revenue, Q1")],
                 should="the reader hears the first point"):
        run.key("Right")
    with run.act("Right to the next point", [record_focus(), said_something()],
                 should="the reader hears the next point"):
        run.key("Right")
    with run.act("End to the last point", [record_focus(), said_something()],
                 should="the reader hears the last point"):
        run.key("End")


def coloredit_popover(run: Any) -> None:
    run.wait_for(role="page tab", name="Color")
    run.grab_focus(role="page tab", name="Color")
    run.wait(0.6)
    with run.act("Tab to the ColorEdit", [focused(role="push button",
                                                  name_contains="Theme accent")],
                 should="the ColorEdit is reached"):
        tab_until(run, role="push button", name_startswith="Theme accent")
    with run.act("Space opens its popover", [record_focus(), said_something()],
                 should="the popover opens, focus moves into it and the reader hears where",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Tab inside the popover", [record_focus(), stop_spoken()],
                 should="the reader hears the next control of the popover"):
        run.key("Tab")
    with run.act("Tab again inside the popover", [record_focus(), stop_spoken()],
                 should="the reader hears the next control of the popover"):
        run.key("Tab")
    with run.act("Escape closes it", [record_focus(), said("Theme accent")],
                 should="focus comes back to the ColorEdit and the reader hears it",
                 tree=True):
        run.key("Escape")


def menu_tree(run: Any) -> None:
    """The File menu open, the tree taken while it is open and after each
    arrow: are the items there, named, and does anything mark the one the
    arrows reached?"""
    run.wait_for(role="page tab", name="Menus")
    run.grab_focus(role="page tab", name="Menus")
    run.wait(0.6)
    with run.act("Tab to File", [focused(role="menu item", name="File")],
                 should="the menu bar's first menu is reached"):
        tab_until(run, role="menu item", name="File")
    items = node_states("the open menu and its items",
                        lambda n: n.get("role") in ("menu", "menu item", "check menu item",
                                                    "radio menu item", "separator"))
    with run.act("Down opens File", [record_focus(), items], should="the menu opens",
                 tree=True):
        run.key("Down")
    with run.act("Down to the second item", [record_focus(), items,
                                             event("object:state-changed")],
                 should="something on the bus marks the item the arrow reached", tree=True):
        run.key("Down")
    with run.act("Enter on it", [record_focus(), said_something()],
                 should="the item runs and the reader hears where focus went"):
        run.key("Return")


SCENARIOS = [
    Scenario("verify-catalog-b-menu-tree", PKG, held(menu_tree),
             "the File menu's tree while open", args=["--tab", "menus"]),
    Scenario("verify-catalog-b-reopen-unmet", PKG, held(reopen_unmet),
             "Text page hidden and shown before any Tab, then walked",
             args=["--tab", "text"]),
    Scenario("verify-catalog-b-datetime-reopen", PKG, held(datetime_reopen),
             "Date & Time walked, reopened, walked to the end", args=["--tab", "datetime"]),
    Scenario("verify-catalog-b-richtext-tab", PKG, held(richtext_tab),
             "Tab inside the rich-text editor and the viewer", args=["--tab", "richtext"]),
    Scenario("verify-catalog-b-line-marks", PKG, held(line_marks),
             "the line chart's points by keyboard", args=["--tab", "charts"]),
    Scenario("verify-catalog-b-coloredit-popover", PKG, held(coloredit_popover),
             "the ColorEdit popover by keyboard", args=["--tab", "color"]),
]
