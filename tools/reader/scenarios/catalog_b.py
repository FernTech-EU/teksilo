# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""widget-catalog, part B: the indicators, charts, scene, text, richtext,
datetime, color and menus tabs, as a screen reader meets them.

Census (`catalog-b-<tab>`), one launch per tab with `--tab <tab>`:

1. the launch tree, audited by the harness;
2. a **first** Tab walk: the selected page tab is focused through AT-SPI (a
   reader's own focus request) and Tab is pressed, one act a stop, each stop
   checked for a name and for Orca saying it. The walk ends when focus leaves
   the page (the status bar or the title bar) or comes back to a node it
   already visited. A field that takes Tab for itself (the rich-text editor
   inserts a tab character, `rich_text/keyboard.rs`) is left with Ctrl+Tab;
3. the page opened by the keyboard: Up to the tab above, Down back (the
   catalog's tab bar is vertical and activates on arrows): what fires, and
   what Orca says, when the page opens;
4. a **second** Tab walk over the page just reopened: what a reader hears
   when they come back to controls they have already met.

The two walks are the same keys over the same controls, so a difference
between them is the page's reopening, not the controls.

Focused scenarios (`catalog-b-<tab>-<what>`) then do what a user does with
the one control a tab is about: arrow through a chart's marks, pan a scene,
step a spin box, move in a rich-text document, drive the colour picker, open
a menu, switch the language.
"""

from __future__ import annotations

import subprocess
from contextlib import contextmanager
from typing import Any, Callable, Iterator

from reader_lib.checks import (_focus_node, _is_focus, _walk, custom, event, focused,
                               in_tree, not_in_tree, not_said, said, said_once)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

PKG = "widget-catalog"

#: --tab name -> (English title, the title of the tab above it).
TABS = {
    "indicators": ("Indicators", "Inputs"),
    "charts": ("Charts", "Indicators"),
    "scene": ("Scene", "Charts"),
    "text": ("Text", "Scene"),
    "richtext": ("Rich Text", "Text"),
    "datetime": ("Date & Time", "Rich Text"),
    "color": ("Color", "Date & Time"),
    "menus": ("Menus", "Color"),
}

#: Where the page ends, going forward: the title bar's controls.
CHROME_NAMES = {"Menu", "English", "Français", "العربية", "Text scale", "Theme",
                "Minimize", "Maximize", "Close"}
#: The tab strip's own stops, before the page.
STRIP_NAMES = {"Scroll tabs down", "Scroll tabs up", "Show all tabs", "teksu! DSL"}


# ---------------------------------------------------------------------------
# Working around a crash (see `held_seat`)
# ---------------------------------------------------------------------------


@contextmanager
def held_seat(run: Any) -> Iterator[None]:
    """Keep one fake-input client connected for the whole scenario.

    Every `run.key` starts a `fake_key` process, and KWin makes a fake input
    device for each client that authenticates and removes it when the client
    disconnects. With no other input device in `kwin_wayland --virtual`, the
    seat's pointer capability comes and goes with each key press, and winit
    0.30.13 panics on a pointer frame for a pointer it has just released
    (`failed to get pointer data`, `wayland/seat/pointer/mod.rs:409`), which
    killed the application in the middle of a walk. A client that stays
    connected keeps a device on the seat, so the capability never drops.
    """
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


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def _last_focus(act: Any) -> dict:
    moves = [e for e in act.events if _is_focus(e)]
    return _focus_node(moves[-1]) if moves else {}


def _spoken(act: Any) -> list[str]:
    return [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in utterances(act.orca)]


def stop_named() -> Any:
    """The node the act moved focus to has a name."""
    def run(act: Any) -> tuple[bool, list[str]]:
        node = _last_focus(act)
        if not node:
            return False, ["no focus change on the bus in this act"]
        return bool((node.get("name") or "").strip()), [
            f"focus on [{node.get('role')}] {node.get('name')!r}"]
    return custom("the stop has a name", run)


def stop_spoken() -> Any:
    """Orca said the focused node's name, whole."""
    def run(act: Any) -> tuple[bool, list[str]]:
        node = _last_focus(act)
        name = (node.get("name") or "").strip()
        said_ = utterances(act.orca)
        evidence = _spoken(act)
        if not name:
            return False, [f"focus on an unnamed [{node.get('role')}]"] + evidence
        heard = [u for u in said_ if normalized(name) in normalized(u.text) and not u.cut]
        return bool(heard), evidence or ["Orca said nothing"]
    return custom("Orca says the stop's name", run, needs_orca=True)


def said_any(*texts: str) -> Any:
    """Orca said, whole, something containing one of `texts`."""
    def run(act: Any) -> tuple[bool, list[str]]:
        said_ = utterances(act.orca)
        heard = [u for u in said_ if not u.cut
                 and any(normalized(t) in normalized(u.text) for t in texts)]
        return bool(heard), _spoken(act) or ["Orca said nothing"]
    return custom(f"Orca says one of {list(texts)!r}", run, needs_orca=True)


def said_something() -> Any:
    """Orca said anything at all, whole."""
    def run(act: Any) -> tuple[bool, list[str]]:
        said_ = [u for u in utterances(act.orca) if not u.cut]
        return bool(said_), _spoken(act) or ["Orca said nothing"]
    return custom("Orca says something", run, needs_orca=True)


def value_event(role: str | None = None) -> Any:
    return event("object:property-change:accessible-value", role=role)


def focus_name(describe: str, predicate: Callable[[str], bool]) -> Any:
    """The name the act's focus landed on satisfies `predicate`."""
    def run(act: Any) -> tuple[bool, list[str]]:
        node = _last_focus(act)
        name = node.get("name") or ""
        return bool(node) and predicate(name), [
            f"focus on [{node.get('role')}] {name!r}" if node else "no focus change"]
    return custom(describe, run)


def tree_node(describe: str, match: Callable[[dict], bool],
              judge: Callable[[dict], tuple[bool, str]]) -> Any:
    """After the act, the first node matching `match` passes `judge`."""
    def run(act: Any) -> tuple[bool, list[str]]:
        for node in _walk(act.tree):
            if match(node):
                ok, why = judge(node)
                return ok, [f"[{node.get('role')}] {node.get('name')!r}: {why}"]
        return False, ["no such node in the tree after the act"]
    return custom(describe, run, needs_tree=True)


def text_of(node: dict) -> str:
    text = node.get("text")
    return text.get("text", "") if isinstance(text, dict) else ""


# ---------------------------------------------------------------------------
# Acts shared by every tab
# ---------------------------------------------------------------------------


def start_on_page(run: Any, tab: str) -> None:
    """Focus the selected page tab, where a keyboard user starts from."""
    title, _ = TABS[tab]
    run.wait_for(role="page tab", name=title)
    run.grab_focus(role="page tab", name=title)
    run.wait(0.6)


def open_tab_by_keyboard(run: Any, title: str, previous: str) -> None:
    """From the page tab, Up to the tab above and Down back: the page opens
    the way a keyboard user opens it."""
    run.grab_focus(role="page tab", name=title)
    run.wait(0.6)
    with run.act(f"Up to the {previous} tab", [focused(role="page tab", name=previous)],
                 should=f"the {previous} page opens"):
        run.key("Up")
    with run.act(f"Down back to the {title} tab",
                 [focused(role="page tab", name=title), said(title)],
                 should=f"the {title} page opens and the reader hears its tab, "
                        "nothing on the page talks over it", tree=True):
        run.key("Down")


def walk_page(run: Any, stops: int, *, label: str = "Tab", chord: str = "Tab") -> list[dict]:
    """Tab from wherever focus is, one act a stop, until focus leaves the page.

    When a press typed into a field instead of moving focus, the next press is
    Ctrl+Tab, the rich-text editor's documented way out."""
    seen: set[str] = set()
    landed: list[dict] = []
    press = chord
    for i in range(stops):
        with run.act(f"{label} {i + 1} ({press})", [stop_named(), stop_spoken()],
                     should="focus moves to the next control and the reader says "
                            "what it is", settle=0.4, record=1.4) as act:
            run.key(press)
        run.collect_events(act)
        node = _last_focus(act)
        typed = any(e["type"].startswith("object:text-changed:insert")
                    and e.get("text") == "\t" for e in act.events)
        press = "Ctrl+Tab" if (typed or not node) else chord
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


def census(tab: str, stops: int, revisit: int = 14):
    title, previous = TABS[tab]

    def body(run: Any) -> None:
        start_on_page(run, tab)
        walk_page(run, stops, label="first walk, Tab")
        open_tab_by_keyboard(run, title, previous)
        walk_page(run, revisit, label="after reopening, Tab")
    return body


# ---------------------------------------------------------------------------
# Focused scenarios
# ---------------------------------------------------------------------------


def tab_until(run: Any, limit: int = 40, **want: Any) -> dict:
    """Press Tab (outside any act, or as one act's steps) until focus lands on
    a node matching `want` (`role=`, `name=`, `name_startswith=`)."""
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


def indicators_open(run: Any) -> None:
    """Launched on Inputs: open Indicators by the keyboard, leave it and come
    back, four visits. The page holds two named live progress indicators
    (`ProgressBar` and `Spinner` set `Live::Polite`, `progress_bar.rs` /
    `spinner.rs`), which every adapter announces as they enter the tree."""
    run.wait_for(role="page tab", name="Inputs")
    run.grab_focus(role="page tab", name="Inputs")
    run.wait(0.5)
    for n in (1, 2, 3, 4):
        with run.act(f"Down to Indicators, visit {n}",
                     [focused(role="page tab", name="Indicators"), said("Indicators")],
                     should="the Indicators page opens and the reader hears its tab, "
                            "whole", tree=(n == 1)):
            run.key("Down")
        if n < 4:
            with run.act(f"Up to Inputs, after visit {n}",
                         [focused(role="page tab", name="Inputs")],
                         should="the Inputs page opens"):
                run.key("Up")
    with run.act("Tab to the first link after four visits",
                 [focused(role="link", name="Open the Teksilo docs"),
                  said("Open the Teksilo docs")],
                 should="the reader hears the link"):
        tab_until(run, role="link")


def text_return(run: Any) -> None:
    """Tab into Username, leave the page and come back, and Tab into it again,
    three rounds: a field focused once must be heard again when it is
    focused again after its page was hidden and shown."""
    start_on_page(run, "text")
    for n in (1, 2, 3):
        with run.act(f"Tab to Username, round {n}",
                     [focused(role="entry", name="Username"), said("Username")],
                     should="the reader hears the field's name and its text"):
            tab_until(run, role="entry", name="Username")
        run.grab_focus(role="page tab", name="Text")
        run.wait(0.5)
        with run.act(f"Up and Down, round {n}", [focused(role="page tab", name="Text")],
                     should="the page is hidden and shown again"):
            run.key("Up")
            run.wait(0.8)
            run.key("Down")


def charts_marks(run: Any) -> None:
    """Arrow through a bar chart's marks, then Tab down the page to the donut
    and Shift+Tab back up to the first chart, which scrolled out of view on
    the way down (`hit.rs`: Right/Down = next, Left/Up = previous, Home/End)."""
    start_on_page(run, "charts")
    tab_until(run, role="document frame", name_startswith="Bar chart")
    with run.act("Right to the first mark",
                 [focused(name="Revenue, Q1: 41"), said("Revenue, Q1: 41"),
                  said_once("Revenue, Q1: 41")],
                 should="the reader hears the first bar once: its series, category and value"):
        run.key("Right")
    with run.act("Right to the next mark",
                 [focused(name="Cost, Q1: 34"), said("Cost, Q1: 34")],
                 should="the reader hears the next bar"):
        run.key("Right")
    with run.act("Down to the next mark",
                 [focused(name="Profit, Q1: 27"), said("Profit, Q1: 27")],
                 should="the reader hears the next bar"):
        run.key("Down")
    with run.act("End to the last mark",
                 [focused(name="Profit, Q4: 18"), said("Profit, Q4: 18")],
                 should="the reader hears the last bar"):
        run.key("End")
    with run.act("Home to the first mark",
                 [focused(name="Revenue, Q1: 41"), said("Revenue, Q1: 41")],
                 should="the reader hears the first bar"):
        run.key("Home")
    with run.act("Tab to the second bar chart",
                 [focused(role="document frame", name_contains="Bar chart"),
                  focus_name("the second bar chart's name tells it from the first",
                             lambda n: n != "Bar chart: 3 series, 4 categories")],
                 should="the reader hears the next chart, and can tell it from the first"):
        run.key("Tab")
    with run.act("Tab to the line chart",
                 [focused(role="document frame", name_contains="Line chart"),
                  said("Line chart")],
                 should="the reader hears the line chart"):
        run.key("Tab")
    with run.act("Tab to the pie chart",
                 [focused(role="document frame", name_contains="Pie chart"),
                  said("Pie chart")],
                 should="the donut is reachable, scrolled into view and named",
                 tree=True):
        run.key("Tab")
    with run.act("Right on the pie chart",
                 [focused(name_contains="Storage"), said("Storage"),
                  focus_name("the slice's name starts with its category, not a comma",
                             lambda n: n.startswith("Storage")),
                  focus_name("the slice's name carries its share, as the chart draws it",
                             lambda n: "%" in n)],
                 should="the reader hears the first slice with its value and share"):
        run.key("Right")
    for n, want in ((1, "Line chart"), (2, "Bar chart"), (3, "Bar chart")):
        with run.act(f"Shift+Tab {n} back up the page",
                     [focused(role="document frame", name_contains=want), said(want)],
                     should=f"focus goes back to the {want.lower()} and the reader hears it, "
                            "though it scrolled out of view and back"):
            run.key("Shift+Tab")
    with run.act("Right on the first bar chart again",
                 [focused(name_contains="Q"), said_any("Revenue", "Cost", "Profit")],
                 should="the reader hears the mark the arrow reached"):
        run.key("Right")


def switch_to_french(run: Any) -> None:
    """Press the title bar's Français button through AT-SPI, and wait for the
    tab strip to show a French title."""
    with run.act("activate Français", [in_tree(role="page tab", name="Graphiques")],
                 should="the whole UI switches to French", tree=True, record=3.0):
        run.action("click", role="push button", name="Français")


def charts_french(run: Any) -> None:
    """The charts' accessible names in a French UI: every visible string on
    the page is French (`tab-charts-*` in the catalog's fr-FR bundle), and a
    reader should hear the charts in the same language."""
    run.wait_for(role="document frame", name_startswith="Bar chart")
    switch_to_french(run)
    with run.act("look at the charts' names after the switch",
                 [not_in_tree(name_contains="Bar chart"),
                  not_in_tree(name_contains="Line chart"),
                  not_in_tree(name="Chart legend")],
                 should="no chart is named in English any more", tree=True):
        run.wait(0.5)
    run.grab_focus(role="page tab", name="Graphiques")
    run.wait(0.5)
    with run.act("Tab to the first chart, in French",
                 [focused(role="document frame"), not_said("Bar chart")],
                 should="the reader hears the chart named in French"):
        tab_until(run, role="document frame")


def color_french(run: Any) -> None:
    """The colour picker in a French UI: the hue and opacity strips follow
    (`color-picker-hue-label` = Teinte), the saturation/brightness area and
    its value should too."""
    run.wait_for(role="panel", name="Saturation and brightness")
    switch_to_french(run)
    with run.act("look at the picker's names after the switch",
                 [in_tree(role="slider", name="Teinte"),
                  not_in_tree(name="Saturation and brightness")],
                 should="every control of the picker is named in French", tree=True):
        run.wait(0.5)
    run.grab_focus(role="page tab", name="Couleur")
    run.wait(0.5)
    tab_until(run, role="panel")
    with run.act("Right on the saturation/brightness area, in French",
                 [not_said("Saturation"), not_said("brightness")],
                 should="the reader hears the new saturation in French"):
        run.key("Right")


def scene_items(run: Any) -> None:
    """Keyboard into the SceneView: what it is called, what the arrows say,
    and whether a lightweight item (the tiles, the two draggable rects) can
    be reached or operated without a pointer (`view/gestures_impl.rs`:
    arrows pan, +/- zoom, Alt+Arrow nudges the *selected* items)."""
    start_on_page(run, "scene")
    with run.act("Tab to the scene view", [stop_named(), stop_spoken()],
                 should="the reader hears the scene view by name", tree=True):
        tab_until(run, role="panel")
    for key in ("Right", "Down"):
        with run.act(f"{key} in the scene view", [said_something()],
                     should="the view pans and the reader hears that it moved, "
                            "or what is now in view"):
            run.key(key)
    with run.act("Space in the scene view",
                 [event("object:state-changed:selected")],
                 should="an item is selected, and the reader hears which"):
        run.key("space")
    with run.act("look for a way to operate 'draggable 1'",
                 [tree_node("'draggable 1' offers an action or can take focus",
                            lambda n: n.get("name") == "draggable 1",
                            lambda n: (bool(n.get("actions"))
                                       or "focusable" in (n.get("states") or []),
                                       f"actions={n.get('actions')} "
                                       f"states={n.get('states')}")),
                  tree_node("'draggable 1' says it can be selected",
                            lambda n: n.get("name") == "draggable 1",
                            lambda n: ("selectable" in (n.get("states") or []),
                                       f"states={n.get('states')}"))],
                 should="the item a pointer user drags can be reached and moved by a "
                        "keyboard or screen reader user too", tree=True):
        run.wait(0.3)
    with run.act("AT-SPI grab_focus on 'draggable 1'",
                 [focused(role="panel", name="draggable 1")],
                 should="a screen reader's focus request reaches the item"):
        run.grab_focus(role="panel", name="draggable 1")
    with run.act("Tab to the card's button", [focused(role="push button", name="Click me"),
                                              said("Click me")],
                 should="the heavyweight card's button is reachable and named"):
        tab_until(run, role="push button", name="Click me")
    with run.act("Shift+Tab back to the scene view", [stop_named(), stop_spoken()],
                 should="the reader hears the scene view again"):
        run.key("Shift+Tab")


def text_fields(run: Any) -> None:
    """The Text tab's SpinBox and SearchField, as a user steps and types."""
    start_on_page(run, "text")
    with run.act("Tab to the SpinBox", [focused(role="spin button"), stop_named()],
                 should="the reader hears which number the spin box holds"):
        tab_until(run, role="spin button")
    with run.act("Up on the SpinBox", [value_event(), said("1")],
                 should="the value goes to 1 and the reader hears it"):
        run.key("Up")
    with run.act("Page Up on the SpinBox", [value_event(), said_something()],
                 should="the value moves by a page and the reader hears it"):
        run.key("Page_Up")
    with run.act("Tab to the SearchField", [focused(role="entry"), stop_named()],
                 should="the reader hears the search field by name"):
        run.key("Tab")
    with run.act("type 'ap' into the SearchField",
                 [event("object:text-changed:insert"), said_any("Apple", "Apricot",
                                                                "suggestion", "result")],
                 should="the reader hears that suggestions appeared"):
        run.type("ap")
    with run.act("Down into the suggestions", [said_any("Apple", "Apricot")],
                 should="the reader hears the first suggestion"):
        run.key("Down")
    with run.act("Escape the suggestions", [event("object:children-changed:remove")],
                 should="the list closes and the reader is still in the field"):
        run.key("Escape")


def richtext_documents(run: Any) -> None:
    """Into the rich-text editor and the read-only viewer: what gets focus,
    what Orca reads as the caret moves, and what text the document offers."""
    start_on_page(run, "richtext")
    blocks_apart = tree_node(
        "the editor's text keeps its paragraphs apart",
        lambda n: n.get("role") == "entry" and "Type here" in text_of(n),
        lambda n: ("bulletsecond" not in text_of(n) and "work.editable" not in text_of(n),
                   f"text={text_of(n)[:140]!r}"))
    viewer_apart = tree_node(
        "the viewer's text keeps its headings and paragraphs apart",
        lambda n: n.get("role") == "document frame" and "Read-only viewer" in text_of(n),
        lambda n: ("viewerRichTextEditor" not in text_of(n),
                   f"text={text_of(n)[:140]!r}"))
    with run.act("Tab into the editor",
                 [focused(role="entry"), stop_named(), said("Type here"), blocks_apart,
                  viewer_apart],
                 should="focus lands on the editor itself, and the reader hears its name "
                        "and the line under the caret", tree=True):
        for _ in range(8):
            run.key("Tab")
            run.wait(0.35)
            node = run.last_focus()
            if node.get("role") in ("entry", "section", "document frame"):
                break
    for key, want in (("Down", "editable bullet"), ("Down", "second bullet"),
                      ("Down", "blockquote"), ("ctrl+End", "10")):
        with run.act(f"{key} in the editor",
                     [event("object:text-caret-moved"), said_any(want)],
                     should=f"the reader hears the line the caret moved to ({want!r})"):
            run.key(key)
    with run.act("type 'x' in the editor",
                 [event("object:text-changed:insert", text_contains="x")],
                 should="the character reaches the document (Orca's own key echo is not "
                        "exercised by the harness)"):
        run.type("x")
    with run.act("Ctrl+Tab to the read-only viewer",
                 [focused(role="document frame"), stop_named(), said("Read-only viewer")],
                 should="the reader hears the viewer's name and what it holds"):
        run.key("Ctrl+Tab")
    for key, want in (("Down", "RichTextEditor"), ("Down", "It supports"),
                      ("ctrl+End", "teksilo-scene")):
        with run.act(f"{key} in the viewer",
                     [event("object:text-caret-moved"), said_any(want)],
                     should=f"the reader hears the line reached ({want!r})"):
            run.key(key)


def datetime_fields(run: Any) -> None:
    """The date and time fields: what each Tab stop is called, and what typing
    a date says."""
    start_on_page(run, "datetime")
    with run.act("Tab to the DateEdit", [focused(role="entry"), stop_named(), stop_spoken()],
                 should="the reader hears the date field by name", tree=True):
        tab_until(run, role="entry")
    with run.act("type 09252026 into the DateEdit",
                 [event("object:text-changed:insert")],
                 should="each digit lands in the field and the mask advances (Orca's own "
                        "key echo is not exercised by the harness)"):
        run.type("09252026")
    with run.act("Tab to Open calendar", [focused(role="push button", name="Open calendar")],
                 should="the reader hears the calendar button"):
        run.key("Tab")
    with run.act("Tab to the TimeEdit", [focused(role="entry"), stop_named(), stop_spoken()],
                 should="the reader hears the time field by name"):
        run.key("Tab")


def color_channels(run: Any) -> None:
    """Drive the colour picker by the keyboard, fresh (no page switch first):
    the saturation/brightness area, the hue strip, the channel spin boxes and
    the swatch grid."""
    start_on_page(run, "color")
    with run.act("Tab to the ColorEdit",
                 [focused(role="push button", name_contains="Theme accent"),
                  said_any("55AADD", "#55")],
                 should="the reader hears the colour the button holds"):
        tab_until(run, role="push button", name_startswith="Theme accent")
    with run.act("Tab to the saturation/brightness area",
                 [focused(name="Saturation and brightness"), said("Saturation and brightness"),
                  said_any("64%", "64 %", "64 percent")],
                 should="the reader hears the area and where the marker is"):
        run.key("Tab")
    with run.act("Right on the saturation/brightness area",
                 [said_any("Saturation 65", "65%", "65 %")],
                 should="the reader hears the new saturation"):
        run.key("Right")
    with run.act("Tab to Hue", [focused(role="slider", name="Hue"), said("Hue")],
                 should="the reader hears the hue slider and its value"):
        run.key("Tab")
    with run.act("Up on Hue", [value_event(), said_something()],
                 should="the reader hears the new hue"):
        run.key("Up")
    with run.act("Tab on to the R channel",
                 [focused(role="spin button"),
                  focus_name("the spin box is named for its channel",
                             lambda n: "red" in n.lower()),
                  said_any("Red")],
                 should="the reader hears which channel the spin box edits"):
        tab_until(run, role="spin button")
    with run.act("Up on the R channel", [value_event(), said_something()],
                 should="the reader hears the red channel's new value"):
        run.key("Up")
    with run.act("Tab to the next channel",
                 [focused(role="spin button"),
                  focus_name("the spin box is named for its channel",
                             lambda n: "green" in n.lower()),
                  said_any("Green")],
                 should="the reader hears which channel the spin box edits"):
        run.key("Tab")
    with run.act("Tab to the swatch grid",
                 [focused(role="table", name="Color presets"), said("Color presets")],
                 should="the reader hears the swatch grid"):
        tab_until(run, role="table")
    for n in (1, 2):
        with run.act(f"Right in the swatch grid, {n}",
                     [event("object:active-descendant-changed"), said("Swatch")],
                     should="the arrow moves to a swatch and the reader hears which one"):
            run.key("Right")
    with run.act("Enter in the swatch grid",
                 [said_any("Color changed", "#")],
                 should="the swatch the reader moved to is chosen, and the reader hears it"):
        run.key("Return")


def menus_open(run: Any) -> None:
    start_on_page(run, "menus")
    with run.act("Tab to the demo menu bar",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the reader hears the menu bar's first menu, and that it opens one"):
        tab_until(run, role="menu item", name="File")
    with run.act("Down opens File",
                 [focused(role="menu item", name="New"), said("New")],
                 should="the menu opens and the reader hears its first item"):
        run.key("Down")
    with run.act("Down to Open", [focused(role="menu item", name="Open"), said("Open")],
                 should="the reader hears the next item"):
        run.key("Down")
    with run.act("Escape closes the menu",
                 [focused(role="menu item", name="File"), said("File")],
                 should="focus comes back to File and the reader hears it"):
        run.key("Escape")
    with run.act("Right to Edit", [focused(role="menu item", name="Edit"), said("Edit")],
                 should="the reader hears the next menu"):
        run.key("Right")
    with run.act("Down opens Edit",
                 [focused(role="menu item", name="Undo"), said("Undo")],
                 should="the Edit menu opens on Undo"):
        run.key("Down")
    with run.act("End to Alignment",
                 [focused(role="menu item", name="Alignment"), said("Alignment")],
                 should="the reader hears the submenu item and that it opens a menu"):
        run.key("End")
    with run.act("Right opens Alignment",
                 [focused(role="menu item", name="Align left"), said("Align left")],
                 should="the submenu opens on its first item"):
        run.key("Right")
    with run.act("Escape twice", should="the menus close and focus is back on the bar",
                 tree=True):
        run.key("Escape", "Escape")
    with run.act("Tab on to the standalone MenuList",
                 [stop_named(), stop_spoken()],
                 should="the reader hears the list and its item"):
        tab_until(run, role="menu")
    with run.act("Down in the MenuList", [focused(role="menu item"), said("Cut")],
                 should="the reader hears the item the arrow reached"):
        run.key("Down")
    with run.act("Down again in the MenuList", [focused(role="menu item"), said("Copy")],
                 should="the reader hears the next item"):
        run.key("Down")
    with run.act("look at the standalone items", [
            tree_node("'Disabled item' is exposed as disabled",
                      lambda n: n.get("name") == "Disabled item",
                      lambda n: ("enabled" not in (n.get("states") or [])
                                 and "sensitive" not in (n.get("states") or []),
                                 f"states={n.get('states')}")),
            tree_node("'With shortcut' carries its Ctrl+S",
                      lambda n: n.get("name") == "With shortcut",
                      lambda n: ("Ctrl" in str(n.get("actions")) or "Ctrl" in str(
                          n.get("description")) or "Ctrl" in str(n.get("attributes")),
                                 f"actions={n.get('actions')} desc={n.get('description')!r} "
                                 f"attrs={n.get('attributes')}"))],
            should="a disabled item says so, and an item's shortcut is reachable",
            tree=True):
        run.wait(0.3)


SCENARIOS = [
    Scenario("catalog-b-indicators", PKG, held(census("indicators", 30)),
             "Indicators tab: first walk, open by keyboard, walk again",
             args=["--tab", "indicators"]),
    Scenario("catalog-b-charts", PKG, held(census("charts", 30)),
             "Charts tab: first walk, open by keyboard, walk again", args=["--tab", "charts"]),
    Scenario("catalog-b-scene", PKG, held(census("scene", 30)),
             "Scene tab: first walk, open by keyboard, walk again", args=["--tab", "scene"]),
    Scenario("catalog-b-text", PKG, held(census("text", 30)),
             "Text tab: first walk, open by keyboard, walk again", args=["--tab", "text"]),
    Scenario("catalog-b-richtext", PKG, held(census("richtext", 30)),
             "Rich Text tab: first walk, open by keyboard, walk again",
             args=["--tab", "richtext"]),
    Scenario("catalog-b-datetime", PKG, held(census("datetime", 30)),
             "Date & Time tab: first walk, open by keyboard, walk again",
             args=["--tab", "datetime"]),
    Scenario("catalog-b-color", PKG, held(census("color", 30)),
             "Color tab: first walk, open by keyboard, walk again", args=["--tab", "color"]),
    Scenario("catalog-b-menus", PKG, held(census("menus", 30)),
             "Menus tab: first walk, open by keyboard, walk again", args=["--tab", "menus"]),
    Scenario("catalog-b-indicators-open", PKG, held(indicators_open),
             "Indicators opened by keyboard four times from Inputs: what its live nodes say",
             args=["--tab", "inputs"]),
    Scenario("catalog-b-text-return", PKG, held(text_return),
             "Username focused, its page hidden and shown, focused again: three rounds",
             args=["--tab", "text"]),
    Scenario("catalog-b-charts-marks", PKG, held(charts_marks),
             "arrow through a bar chart's marks, reach the donut, come back up",
             args=["--tab", "charts"]),
    Scenario("catalog-b-charts-fr", PKG, held(charts_french),
             "the charts' accessible names after switching the UI to French",
             args=["--tab", "charts"]),
    Scenario("catalog-b-color-fr", PKG, held(color_french),
             "the colour picker's accessible names after switching the UI to French",
             args=["--tab", "color"]),
    Scenario("catalog-b-scene-items", PKG, held(scene_items),
             "keyboard into a SceneView and its items", args=["--tab", "scene"]),
    Scenario("catalog-b-text-fields", PKG, held(text_fields),
             "step the SpinBox and type in the SearchField", args=["--tab", "text"]),
    Scenario("catalog-b-richtext-documents", PKG, held(richtext_documents),
             "move through the rich-text editor and viewer", args=["--tab", "richtext"]),
    Scenario("catalog-b-datetime-fields", PKG, held(datetime_fields),
             "the Date & Time tab's edit fields: names, typing",
             args=["--tab", "datetime"]),
    Scenario("catalog-b-color-channels", PKG, held(color_channels),
             "step the colour picker's area, hue, channels and swatches",
             args=["--tab", "color"]),
    Scenario("catalog-b-menus-open", PKG, held(menus_open),
             "open the Menus tab's menu bar and walk its menus", args=["--tab", "menus"]),
]
