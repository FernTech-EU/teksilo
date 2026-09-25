# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""widget-catalog, first half of the tabs: what a reader meets on each page.

One census scenario per tab (`catalog-a-<tab>`), launched with `--tab <tab>`:

1. the launch tree (the harness audits it);
2. the tab is focused through AT-SPI (a reader's own focus request), then
   Up opens the previous tab and Down opens this one again: what fires, and
   what Orca says, when a page opens by the keyboard;
3. a Tab walk from the selected tab into the page, one act a stop. Because of
   step 2 the walk meets the page as a reader who came back to it does.

The catalog's tab bar is vertical, so Up/Down move between tabs (automatic
activation: `tab_widget/header.rs`, the ArrowUp/ArrowDown arms select and
focus). Every page sits in a `TabContent` (`examples/widget_catalog/src/main.rs`)
= Switcher(classic, teksu) in Padding in ScrollArea, under the TabWidget's
`Role::TabPanel` (an AT-SPI scroll pane: `accesskit_atspi_common` `node.rs:256`).

The focused scenarios after the censuses each take one thing a census met and
state what a reader should get from it.
"""

from __future__ import annotations

import time

from reader_lib.checks import (_focus_node, _is_focus, announced, custom, event, focused,
                               in_tree, no_event, not_said, said)
from reader_lib.orca import utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario, tab_walk

#: The catalog's tab order (`examples/widget_catalog/src/tabs.rs`, `TABS`)
#: and each tab's English title (`locales/en-US/widget_catalog.ftl`).
TABS = [
    ("palette", "Palette"), ("layout", "Layout"), ("visuals", "Visuals"),
    ("containers", "Containers"), ("chrome", "Chrome"), ("buttons", "Buttons"),
    ("styling", "Styling"), ("inputs", "Inputs"), ("indicators", "Indicators"),
]


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def _walk(tree):
    stack = [tree or {}]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def _tab_panel_named(title: str):
    def run(act):
        found = [n.get("name") for n in _walk(act.tree) if n.get("role") == "scroll pane"]
        return title in found, [f"scroll panes in the tree: {found}"]
    return custom(f"the tab panel for {title!r} is in the tree", run, needs_tree=True)


def _last_focus_is(role: str, name: str):
    """The act ENDS with focus on the node, which `focused` cannot say when a
    later focus change inside the act took it somewhere else."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        lines = [f"{act.rel_ms(e):+8.1f} ms {e['type']} [{_focus_node(e).get('role')}] "
                 f"{_focus_node(e).get('name')!r}" for e in moves]
        if not moves:
            return False, ["no focus change in the act"]
        last = _focus_node(moves[-1])
        return (last.get("role") == role and last.get("name") == name), lines
    return custom(f"focus is still on [{role}] {name!r} when the act ends", run)


def _no_focus_on(role: str, name: str):
    def run(act):
        back = [e for e in act.events if _is_focus(e)
                and _focus_node(e).get("role") == role and _focus_node(e).get("name") == name]
        return not back, [f"{act.rel_ms(e):+8.1f} ms {e['type']} -> [{role}] {name!r}"
                          for e in back]
    return custom(f"focus never goes to [{role}] {name!r}", run)


def _spoken_without(word: str, about: str):
    """Orca's speech in the act does not contain `word` (e.g. 'not selected')."""
    def run(act):
        said_all = [u.text for u in utterances(act.orca)]
        bad = [t for t in said_all if word.lower() in t.lower()]
        return not bad, [f"Orca said: {t!r}" for t in said_all] or ["Orca said nothing"]
    return custom(f"Orca does not say {word!r} about {about}", run, needs_orca=True)


def _last_page_tab_said(name: str):
    """The last page tab Orca spoke in the act is `name`: Orca moves its locus
    of focus to whatever page tab it presents, so the last one it spoke is
    where it believes focus is."""
    def run(act):
        spoken = [u.text for u in utterances(act.orca) if "page tab" in u.text]
        if not spoken:
            return False, ["Orca spoke no page tab"]
        return spoken[-1].startswith(name), [f"Orca said: {t!r}" for t in spoken]
    return custom(f"the last page tab Orca speaks is {name!r} (its locus of focus)", run,
                  needs_orca=True)


def _node(tree, role: str, name: str | None = None, nth: int = 0):
    found = [n for n in _walk(tree) if n.get("role") == role
             and (name is None or n.get("name") == name)]
    return found[nth] if len(found) > nth else None


def _states(role: str, name: str, nth: int = 0, absent=(), present=()):
    def run(act):
        node = _node(act.tree, role, name, nth)
        if node is None:
            return False, [f"no [{role}] {name!r} #{nth} in the tree"]
        states = node.get("states", [])
        ok = not any(s in states for s in absent) and all(s in states for s in present)
        return ok, [f"[{role}] {name!r} #{nth} states={states}"]
    what = f"[{role}] {name!r} #{nth}" + (f" has no {list(absent)}" if absent else "") + \
        (f" has {list(present)}" if present else "")
    return custom(what, run, needs_tree=True)


def _description(role: str, name: str, contains: str):
    def run(act):
        node = _node(act.tree, role, name)
        if node is None:
            return False, [f"no [{role}] {name!r} in the tree"]
        desc = node.get("description") or ""
        return contains.lower() in desc.lower(), [f"[{role}] {name!r} description={desc!r}"]
    return custom(f"[{role}] {name!r}'s description holds {contains!r}", run, needs_tree=True)


def _value(role: str, name: str, want: str):
    def run(act):
        node = _node(act.tree, role, name)
        if node is None:
            return False, [f"no [{role}] {name!r} in the tree"]
        value = node.get("value")
        return want in repr(value), [f"[{role}] {name!r} value={value!r}"]
    return custom(f"[{role}] {name!r}'s value reads {want!r}", run, needs_tree=True)


def _every_announcement_holds(pairs: dict):
    """Every announcement whose text is a key also carries the value (a
    banner's title and the message it exists to give)."""
    def run(act):
        lines, ok = [], True
        for e in act.events:
            if e["type"] != "object:announcement":
                continue
            text = e.get("text") or ""
            lines.append(f"{act.rel_ms(e):+8.1f} ms announcement {text!r}")
            for title, body in pairs.items():
                if text.strip().startswith(title) and body.lower() not in text.lower():
                    ok = False
        return ok and bool(lines), lines or ["no announcement in the act"]
    return custom("each banner's announcement carries its message: "
                  + ", ".join(f"{k!r}+{v!r}" for k, v in pairs.items()), run)


# ---------------------------------------------------------------------------
# Scene setting
# ---------------------------------------------------------------------------


def tab_to(run, name: str, role: str | None = None, limit: int = 40,
           chord: str = "Tab") -> dict:
    """Press `chord` until the bus reports focus on a node named `name` (and
    of `role`). Content below the fold is not on the bus until it scrolls in
    (`accesskit_consumer` `filters.rs:64-86` drops a clipped child), so it
    cannot be found by a walk; the keyboard reaches it the way a reader does."""
    for _ in range(limit):
        node = run.last_focus()
        if node.get("name") == name and (role is None or node.get("role") == role):
            return node
        run.key(chord)
        time.sleep(0.35)
    node = run.last_focus()
    if node.get("name") == name and (role is None or node.get("role") == role):
        return node
    raise RunError(f"{limit} x {chord} never focused [{role}] {name!r}; last focus {node}")


def scene(run, label: str, fn, *, record: float = 1.0) -> None:
    """Scene setting inside an act of its own, not judged.

    The harness credits Orca's speech to an act from Orca's receipt of the
    first event of a type the act emitted, looking from the end of the last
    act; speech caused by steps taken *between* acts lands in the next act
    (its report shows it at negative times). Wrapping every scene-setting
    step in an act keeps that speech out of the acts that are judged.
    """
    with run.act(f"(scene) {label}", should="scene setting, not judged",
                 settle=0.3, record=record):
        fn()


# ---------------------------------------------------------------------------
# The censuses
# ---------------------------------------------------------------------------


def census(slug: str, title: str, stops: int):
    index = [s for s, _ in TABS].index(slug)
    neighbour = TABS[index - 1][1] if index > 0 else TABS[index + 1][1]
    away_key, back_key = ("Up", "Down") if index > 0 else ("Down", "Up")

    def body(run):
        run.wait_for(role="page tab", name=title)
        run.wait_for(role="scroll pane", name=title)
        with run.act(f"focus the {title} tab through AT-SPI",
                     [focused(role="page tab", name=title), said(title)],
                     should=f"focus lands on the selected {title} tab and the reader says it"):
            run.grab_focus(role="page tab", name=title)
        with run.act(f"{away_key}: open {neighbour}",
                     [focused(role="page tab", name=neighbour), said(neighbour),
                      _last_page_tab_said(neighbour), _tab_panel_named(neighbour)],
                     should=f"the {neighbour} page opens and the reader hears its tab",
                     tree=True):
            run.key(away_key)
        with run.act(f"{back_key}: open {title} again",
                     [focused(role="page tab", name=title), said(title),
                      _last_page_tab_said(title), _tab_panel_named(title)],
                     should=f"the {title} page opens and the reader hears its tab",
                     tree=True):
            run.key(back_key)
        tab_walk(run, stops=stops)

    return Scenario(f"catalog-a-{slug}", "widget-catalog", body,
                    f"census of the {title} tab: tree, opening it, Tab walk",
                    args=["--tab", slug])


# ---------------------------------------------------------------------------
# The title bar's Theme switcher and its composite tooltip
# ---------------------------------------------------------------------------


def tooltip_bounce(run):
    """A focus-shown tooltip (the title bar's Theme switcher carries a
    composite one) is dismissed with a fade when focus leaves its anchor, and
    the fade's end hands focus back to the anchor
    (`overlay_impl.rs:530-532` records the anchor as the overlay's focus
    restore; `widget_tree.rs:1661-1667` restores it when the fade ends,
    wherever focus has gone since).

    The tooltip shows 0.7 s after focus lands (`tooltip_delay_heavy`) and
    turns sticky, a focusable dialog, 2 s after that
    (`tooltip/rich.rs` `DWELL_PROMOTION`). Both are left here: first with a
    dwell between the two, then after it turned sticky.
    """
    def back_to_palette():
        # After a bounce focus sits on Theme with a warm (unfaded) tooltip;
        # one more Tab leaves it for good. Then wait out the 1 s warm-reshow
        # grace so the next tooltip fades again.
        if run.last_focus().get("name") != "Palette":
            run.key("Tab")
            time.sleep(0.6)
        tab_to(run, "Palette", role="page tab", limit=3)

    run.wait_for(role="combo box", name="Theme")
    scene(run, "Tab to Text scale",
          lambda: tab_to(run, "Text scale", role="spin button", limit=10))
    for attempt, chord in ((1, "Tab"), (2, "Shift+Tab")):
        with run.act(f"{chord} onto Theme ({attempt})", [focused(role="combo box", name="Theme")],
                     should="focus lands on Theme; its tooltip shows 0.7 s later",
                     record=0.9):
            run.key(chord)
        with run.act(f"Tab away from Theme after its tooltip has shown ({attempt})",
                     [_last_focus_is("page tab", "Palette"),
                      _no_focus_on("combo box", "Theme"), said("Palette")],
                     should="focus moves on to the Palette tab and stays there",
                     settle=0.1, record=2.0):
            run.key("Tab")
        scene(run, "settle on the Palette tab", back_to_palette, record=2.0)
    with run.act("Shift+Tab onto Theme and rest 3.5 s (the tooltip turns sticky)",
                 [focused(role="combo box", name="Theme")],
                 should="focus lands on Theme", record=3.5):
        run.key("Shift+Tab")
    with run.act("Tab: into the sticky tooltip",
                 [focused(role="dialog"), said("About switching theme at runtime")],
                 should="the reader lands in the tooltip and hears it", settle=0.1,
                 record=1.5):
        run.key("Tab")
    with run.act("Tab: out of the sticky tooltip",
                 [_last_focus_is("page tab", "Palette"),
                  _no_focus_on("combo box", "Theme"), said("Palette")],
                 should="focus moves on to the Palette tab and stays there",
                 settle=0.1, record=2.0):
        run.key("Tab")
    scene(run, "settle on the Palette tab", back_to_palette, record=2.0)
    with run.act("Shift+Tab to Theme and Tab away before its tooltip shows",
                 [_last_focus_is("page tab", "Palette")],
                 should="focus ends on the Palette tab (no tooltip was shown, so "
                        "nothing hands focus back)", settle=0.5, record=2.0):
        run.key("Shift+Tab")
        run.wait(0.25)
        run.key("Tab")


def theme_tooltip(run):
    """What a reader gets of the Theme switcher's composite tooltip
    (`examples/widget_catalog/src/main.rs` `theme_switch_caveat`): its text
    should reach the reader as the combo box's description, which the
    framework writes from the anchor's `described_by`
    (`accessibility_description_impl.rs` `relation_text`)."""
    run.wait_for(role="combo box", name="Theme")
    scene(run, "Tab to Text scale",
          lambda: tab_to(run, "Text scale", role="spin button", limit=10))
    with run.act("Tab onto Theme",
                 [focused(role="combo box", name="Theme"), said("Theme"),
                  said("About switching theme at runtime"), not_said("Tooltip"),
                  _description("combo box", "Theme", "About switching theme at runtime")],
                 should="the reader hears the combo box and the tooltip's text as its "
                        "description", tree=True, record=1.2):
        run.key("Tab")
    with run.act("rest on Theme 3 s (the tooltip turns sticky)",
                 [said("About switching theme at runtime")],
                 should="the reader is told the tooltip's text, or that there is one to "
                        "read", settle=0.1, record=3.0, tree=True):
        pass


# ---------------------------------------------------------------------------
# Nodes that leave the tree and come back
# ---------------------------------------------------------------------------


def return_to_page(run):
    """Controls a reader met on a page, after the page was left and opened
    again. The page's nodes leave the filtered tree when the Switcher parks
    the page (`accesskit_atspi_common` `adapter.rs:90-111` `remove_node` sends
    `defunct`), and come back with the same ids, so the same AT-SPI paths."""
    run.wait_for(role="page tab", name="Inputs")
    with run.act("focus the Inputs tab through AT-SPI",
                 [focused(role="page tab", name="Inputs")]):
        run.grab_focus(role="page tab", name="Inputs")
    scene(run, "Tab to the teksu! toggle",
          lambda: tab_to(run, "teksu! DSL", role="toggle button", limit=6))
    with run.act("Tab to the two-state check box (first visit)",
                 [focused(role="check box", name="Two-state checkbox"),
                  said("Two-state checkbox")], should="the reader hears the check box"):
        run.key("Tab")
    with run.act("Tab to the tristate check box (first visit)",
                 [focused(role="check box", name="Tristate checkbox"),
                  said("Tristate checkbox")], should="the reader hears the check box"):
        run.key("Tab")
    with run.act("back to the Inputs tab through AT-SPI",
                 [focused(role="page tab", name="Inputs")]):
        run.grab_focus(role="page tab", name="Inputs")
    with run.act("Down: open Indicators", [focused(role="page tab", name="Indicators"),
                                           said("Indicators")]):
        run.key("Down")
    with run.act("Up: open Inputs again", [focused(role="page tab", name="Inputs"),
                                           said("Inputs")], tree=True):
        run.key("Up")
    scene(run, "Tab to the teksu! toggle",
          lambda: tab_to(run, "teksu! DSL", role="toggle button", limit=6))
    with run.act("Tab to the two-state check box (after returning to the page)",
                 [focused(role="check box", name="Two-state checkbox"),
                  said("Two-state checkbox")],
                 should="the reader hears the check box again"):
        run.key("Tab")
    with run.act("Tab to the tristate check box (after returning to the page)",
                 [focused(role="check box", name="Tristate checkbox"),
                  said("Tristate checkbox")],
                 should="the reader hears the check box again"):
        run.key("Tab")
    with run.act("Tab to Option A (after returning; never focused before)",
                 [focused(role="radio button", name="Option A"), said("Option A")],
                 should="the reader hears the radio button"):
        run.key("Tab")


def scroll_reveal(run):
    """Tab from the vertical slider down the Inputs page: the page scrolls,
    and content that was below the fold enters the tree. A segmented control
    entering with a selected segment makes the adapter send
    `selection-changed` (`accesskit_atspi_common` `adapter.rs:78-80`), which
    Orca 46.1 answers by moving its locus of focus to the selected child
    (`orca/scripts/default.py:1579-1602`)."""
    run.wait_for(role="slider", name="Vertical slider")
    scene(run, "focus the vertical slider through AT-SPI",
          lambda: run.grab_focus(role="slider", name="Vertical slider"))
    with run.act("Tab to the first segmented control",
                 [focused(role="radio button", name="First"), said("First")],
                 should="the reader hears the segment"):
        run.key("Tab")
    with run.act("Tab to the width slider below it (the page scrolls)",
                 [focused(role="slider", name="Segmented control width"),
                  said("Segmented control width"), not_said("Document view"),
                  not_said("Overview")],
                 should="the reader hears the slider it is on, and nothing else"):
        run.key("Tab")
    with run.act("Tab to the overflow segmented control",
                 [focused(role="radio button", name="Overview"), said("Overview")],
                 should="the reader hears the segment focus moved to"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# States and values
# ---------------------------------------------------------------------------


def segments(run):
    """The SegmentedControl's selected segment, and the vertical slider's
    value, as Orca reads them on a page opened once (no revived nodes)."""
    run.wait_for(role="slider", name="Vertical slider")
    with run.act("focus the vertical slider through AT-SPI",
                 [focused(role="slider", name="Vertical slider"),
                  said("Vertical slider"), _spoken_without("0000", "the slider's value"),
                  _value("slider", "Vertical slider", "'current': 0.3,")],
                 should="the reader hears the slider and a readable value", tree=True):
        run.grab_focus(role="slider", name="Vertical slider")
    with run.act("Tab into the segmented control",
                 [focused(role="radio button", name="First"), said("First"),
                  _spoken_without("not selected", "the selected segment 'First'"),
                  _states("radio button", "First", present=["checked"])],
                 should="the reader hears the selected segment as selected",
                 tree=True):
        run.key("Tab")
    with run.act("Right: select the second segment",
                 [said("Second"), _spoken_without("not selected", "'Second' once selected")],
                 should="the reader hears 'Second', selected", tree=True):
        run.key("Right")


def slider_keys(run):
    """Arrow keys on a focused Slider: does the value change reach the bus
    (and Orca) in answer to the key, or only with some later event?"""
    run.wait_for(role="slider", name="Volume")
    with run.act("focus the Volume slider through AT-SPI",
                 [focused(role="slider", name="Volume"), said("Volume")]):
        run.grab_focus(role="slider", name="Volume")
    for n in (1, 2):
        with run.act(f"Right on Volume ({n})",
                     [event("object:property-change:accessible-value", role="slider"),
                      said(str(50 + n))],
                     should=f"the value moves to {50 + n} and the reader hears it",
                     record=3.0):
            run.key("Right")
    with run.act("Shift alone (a key that changes nothing)",
                 [no_event("object:property-change:accessible-value")],
                 should="nothing: every change was already delivered", record=2.0):
        run.key("Shift")
    scene(run, "focus the vertical slider through AT-SPI",
          lambda: run.grab_focus(role="slider", name="Vertical slider"))
    with run.act("Up on the vertical slider",
                 [event("object:property-change:accessible-value", role="slider",
                        name_contains="Vertical"), said("0.31")],
                 should="the value moves and the reader hears it", record=3.0):
        run.key("Up")


def toggles(run):
    """Space on the Inputs page's check boxes, toggle and radio buttons: does
    each state change reach the bus and Orca in answer to the key?"""
    run.wait_for(role="check box", name="Two-state checkbox")
    scene(run, "focus the two-state check box",
          lambda: run.grab_focus(role="check box", name="Two-state checkbox"))
    with run.act("Space on the two-state check box",
                 [event("object:state-changed:checked", role="check box"), said("checked")],
                 should="the reader hears it become checked", record=3.0):
        run.key("space")
    scene(run, "focus the tristate check box",
          lambda: run.grab_focus(role="check box", name="Tristate checkbox"))
    with run.act("Space on the tristate check box",
                 [event("object:state-changed", role="check box",
                        name_contains="Tristate"), said("checked")],
                 should="the reader hears its new state", record=3.0):
        run.key("space")
    scene(run, "focus the Enable feature toggle",
          lambda: run.grab_focus(role="toggle button", name="Enable feature"))
    with run.act("Space on the Enable feature toggle",
                 [event("object:state-changed:pressed", role="toggle button"),
                  said("pressed")],
                 should="the reader hears it become on", record=3.0):
        run.key("space")
    scene(run, "focus Option B",
          lambda: run.grab_focus(role="radio button", name="Option B"))
    with run.act("Space on Option B",
                 [event("object:state-changed:checked", role="radio button"),
                  said("selected")],
                 should="the reader hears Option B become the selected one", record=3.0):
        run.key("space")


def disabled_buttons(run):
    """`Button::enabled(false)` on the Buttons page's disabled row, and the
    disabled check box and toggle are elsewhere; this reads the row."""
    run.wait_for(role="push button", name="Flat")
    with run.act("the disabled buttons in the tree",
                 [_states("push button", "Default", 1, absent=["sensitive", "enabled"]),
                  _states("push button", "Regular", 1, absent=["sensitive", "enabled"]),
                  _states("push button", "Flat", 1, absent=["sensitive", "enabled"]),
                  _states("push button", "Default", 0, present=["sensitive", "enabled"])],
                 should="a disabled button reads as unavailable, an enabled one does not",
                 tree=True, settle=0.5, record=0.5):
        pass


# ---------------------------------------------------------------------------
# Pages opened for the first time
# ---------------------------------------------------------------------------


def first_open_containers(run):
    """The Containers page opened for the first time in a session: it holds
    two embedded TabWidgets whose tab lists appear with a selected tab."""
    run.wait_for(role="page tab", name="Chrome")
    run.wait_for(role="scroll pane", name="Chrome")
    with run.act("focus the Chrome tab through AT-SPI",
                 [focused(role="page tab", name="Chrome")]):
        run.grab_focus(role="page tab", name="Chrome")
    with run.act("Up: open Containers for the first time",
                 [focused(role="page tab", name="Containers"), said("Containers"),
                  not_said("Edit page tab"), not_said("Overview page tab"),
                  _last_page_tab_said("Containers")],
                 should="the reader hears the Containers tab, and only that",
                 record=3.5):
        run.key("Up")


def first_open_chrome(run):
    """The Chrome page opened for the first time: four Banners, each a polite
    live `Role::Status` named by its title (`banner.rs:256-265`), and a
    non-linear Stepper whose steps are a tab list."""
    run.wait_for(role="page tab", name="Containers")
    run.wait_for(role="scroll pane", name="Containers")
    with run.act("focus the Containers tab through AT-SPI",
                 [focused(role="page tab", name="Containers")]):
        run.grab_focus(role="page tab", name="Containers")
    with run.act("Down: open Chrome for the first time",
                 [focused(role="page tab", name="Chrome"), said("Chrome"),
                  _every_announcement_holds({"Warning": "Disk is 90 % full",
                                             "Error": "Network connection lost"}),
                  not_said("Account page tab"), _last_page_tab_said("Chrome")],
                 should="the page opens and the reader hears its tab; a banner that "
                        "announces itself says its message", record=3.5, tree=True):
        run.key("Down")


# ---------------------------------------------------------------------------
# Disclosure: Accordion and ToolBox headers
# ---------------------------------------------------------------------------


def accordion(run):
    """Accordion and ToolBox headers: `Role::Button` with `set_expanded`
    (`accordion.rs:668-678`, `tool_box.rs:1017-1037`)."""
    run.wait_for(role="page tab", name="Containers")
    scene(run, "Tab to Show details", lambda: (
        run.grab_focus(role="page tab", name="Containers"),
        tab_to(run, "Show details", role="push button", limit=20)))
    with run.act("the Show details accordion header, focused",
                 [_states("push button", "Show details", present=["expandable"])],
                 should="the header says it can expand and whether it is expanded",
                 tree=True, settle=0.5, record=0.5):
        pass
    with run.act("Space on Show details",
                 [event("object:state-changed:expanded"), said("expanded")],
                 should="the reader hears the new expanded state", tree=True):
        run.key("space")
    with run.act("Space on Show details again",
                 [event("object:state-changed:expanded"), said("collapsed")],
                 should="the reader hears it collapse"):
        run.key("space")
    scene(run, "Tab to the ToolBox's Editor header",
          lambda: tab_to(run, "Editor", role="push button", limit=6))
    with run.act("Space on the ToolBox's Editor header",
                 [event("object:state-changed:expanded"), said("expanded"),
                  _states("push button", "Editor", present=["expanded"])],
                 should="the reader hears the Editor section open", tree=True):
        run.key("space")


def splitter(run):
    """The Containers page's Splitter divider (`splitter/handle.rs:790-830`):
    a `Role::Splitter` named 'Splitter divider' with a percent value."""
    run.wait_for(role="page tab", name="Containers")
    scene(run, "Tab to the ToolBox's Privacy header", lambda: (
        run.grab_focus(role="page tab", name="Containers"),
        tab_to(run, "Privacy", role="push button", limit=20)))
    with run.act("Tab onto the splitter divider",
                 [focused(role="separator", name="Splitter divider"), said("50")],
                 should="the reader hears the divider and where it sits", tree=True):
        run.key("Tab")
    with run.act("Right on the divider",
                 [event("object:property-change:accessible-value"), said("%")],
                 should="the divider moves and the reader hears its new position",
                 record=3.0):
        run.key("Right")


# ---------------------------------------------------------------------------
# Popups
# ---------------------------------------------------------------------------


def combo(run):
    """The Inputs page's ComboBox (`ComboBox::from_items(...).placeholder(...)`,
    no `.label(...)`; `combo_box.rs:1248-1274`)."""
    run.wait_for(role="page tab", name="Inputs")
    scene(run, "Tab to the last radio tile", lambda: (
        run.grab_focus(role="page tab", name="Inputs"),
        tab_to(run, "Notebook", role="radio button", limit=30)))
    with run.act("Tab onto the fruit combo box",
                 [focused(role="combo box"), in_tree(role="combo box",
                                                     name_contains="fruit"),
                  said("fruit")],
                 should="the combo box has a name, and the reader hears it", tree=True):
        run.key("Tab")
    with run.act("Space: open the combo box", [said("Apple")],
                 should="the list opens and the reader hears where they are", tree=True):
        run.key("space")
    with run.act("Down in the open list", [said("Banana")],
                 should="the reader hears the next fruit"):
        run.key("Down")
    with run.act("Enter: choose it", [focused(role="combo box"), said("Banana")],
                 should="the list closes and the reader hears the combo box with the "
                        "chosen fruit", tree=True):
        run.key("Return")


def popover(run):
    """The Buttons page's PopoverButton (`Open popover`)."""
    run.wait_for(role="page tab", name="Buttons")
    scene(run, "Tab to Open popover", lambda: (
        run.grab_focus(role="page tab", name="Buttons"),
        tab_to(run, "Open popover", role="push button", limit=30)))
    with run.act("Space on Open popover", [said("Popover")],
                 should="the popover opens and the reader hears what it says",
                 tree=True, record=3.0):
        run.key("space")
    with run.act("Escape", [focused(role="push button", name="Open popover")],
                 should="the popover closes and focus is back on its button",
                 record=2.0):
        run.key("Escape")


SCENARIOS = [
    census("palette", "Palette", 30),
    census("layout", "Layout", 30),
    census("visuals", "Visuals", 30),
    census("containers", "Containers", 30),
    census("chrome", "Chrome", 30),
    census("buttons", "Buttons", 30),
    census("styling", "Styling", 30),
    census("inputs", "Inputs", 30),
    Scenario("catalog-a-tooltip-bounce", "widget-catalog", tooltip_bounce,
             "Tab away from the Theme switcher after its focus-shown tooltip appeared",
             args=["--tab", "palette"]),
    Scenario("catalog-a-theme-tooltip", "widget-catalog", theme_tooltip,
             "what a reader gets of the Theme switcher's composite tooltip",
             args=["--tab", "palette"]),
    Scenario("catalog-a-return-to-page", "widget-catalog", return_to_page,
             "controls on the Inputs page after leaving the page and coming back",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-scroll-reveal", "widget-catalog", scroll_reveal,
             "Tab down the Inputs page past a segmented control that scrolls into view",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-segments", "widget-catalog", segments,
             "the segmented control's selected segment and the vertical slider's value",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-slider-keys", "widget-catalog", slider_keys,
             "arrow keys on a focused slider: when the value change reaches the bus",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-toggles", "widget-catalog", toggles,
             "Space on check boxes, a toggle and a radio button: the state change",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-first-open-containers", "widget-catalog", first_open_containers,
             "opening the Containers page (two embedded TabWidgets) the first time",
             args=["--tab", "chrome"]),
    Scenario("catalog-a-first-open-chrome", "widget-catalog", first_open_chrome,
             "opening the Chrome page (four Banners, a Stepper) the first time",
             args=["--tab", "containers"]),
    Scenario("catalog-a-accordion", "widget-catalog", accordion,
             "expand and collapse an Accordion header, open a ToolBox section",
             args=["--tab", "containers"]),
    Scenario("catalog-a-splitter", "widget-catalog", splitter,
             "focus and move the Containers page's Splitter divider",
             args=["--tab", "containers"]),
    Scenario("catalog-a-combo", "widget-catalog", combo,
             "focus, open and choose in the Inputs page's combo box",
             args=["--tab", "inputs"]),
    Scenario("catalog-a-popover", "widget-catalog", popover,
             "open and close the Buttons page's PopoverButton", args=["--tab", "buttons"]),
    Scenario("catalog-a-disabled", "widget-catalog", disabled_buttons,
             "the Buttons page's disabled buttons as AT-SPI states",
             args=["--tab", "buttons"]),
]
