# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's probes for the scene / animations / previewer sweep (`scene_etc.py`).

Acts the sweep does not do, each asking one question:

* ``verify-sceneetc-toggle-tree`` (theme-styles): when Space turns a `Toggle`
  on, is the new state in the tree the adapter holds (so only the event is
  late), or is the AccessKit tree itself stale until something else re-walks?
* ``verify-sceneetc-item-focus`` (scene-showcase): an AT-SPI grab_focus on a
  lightweight item while focus is elsewhere: where does focus land?
* ``verify-sceneetc-transform`` (scene-corkboard): select a card the only way
  the keyboard can (Enter), then reach the main pane by AT-SPI and try the
  selection frame's keyboard route (`t`, Tab, arrows, Enter, Esc).
* ``verify-sceneetc-hidden-content`` (animations-kit): the Slide banner that
  starts slid out, and the Blur section's "sensitive" numbers that start
  obscured: are they in the tree a reader walks while a sighted user cannot
  see them?
"""

from __future__ import annotations

from reader_lib.checks import _focus_node, _is_focus, _walk, custom, event, focused, in_tree
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario


def _heard(act) -> list[str]:
    return [f"{u.stamp} Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
            for u in utterances(act.orca)] or ["Orca said nothing in the act"]


def said_something():
    def run(act):
        return bool(utterances(act.orca)), _heard(act)
    return custom("Orca says something", run, needs_orca=True)


def node_where(describe: str, pred, *, want: bool = True):
    def run(act):
        hits = [n for n in _walk(act.tree) if pred(n)]
        lines = [f"[{n.get('role')}] {n.get('name')!r} states={n.get('states')} "
                 f"extents={n.get('extents')} interfaces={n.get('interfaces')}"
                 for n in hits[:12]]
        return (bool(hits) == want), lines or ["no node matched"]
    return custom(describe, run, needs_tree=True)


def last_focus_is(describe: str, pred):
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        node = _focus_node(moves[-1])
        return pred(node), [f"last focus: [{node.get('role')}] {node.get('name')!r} "
                            f"path ...{(node.get('path') or '')[-16:]}"]
    return custom(describe, run)


def scene(run, label: str, **kw):
    return run.act(f"(scene) {label}", should="setting the scene, not judged", **kw)


def _n(text) -> str:
    return normalized(text or "")


# ---------------------------------------------------------------------------
# theme-styles: is the Toggle's state stale in the adapter's tree, or only
# its event?
# ---------------------------------------------------------------------------


def _pressed(name: str):
    return lambda n: n.get("role") == "toggle button" and n.get("name") == name \
        and "pressed" in (n.get("states") or [])


def toggle_tree_body(run):
    run.wait_for(role="toggle button", name="Notifications")
    with scene(run, "focus the Notifications toggle"):
        run.grab_focus(role="toggle button", name="Notifications")
    with run.act("Space, then read the adapter's tree",
                 [event("object:state-changed:pressed", role="toggle button",
                        name_contains="Notifications"),
                  node_where("the adapter's tree shows Notifications pressed",
                             _pressed("Notifications"))],
                 should="the toggle is on in the tree at once", record=4.0, tree=True):
        run.key("space")
    with run.act("do nothing for 6 s, read the tree again",
                 [event("object:state-changed:pressed", role="toggle button"),
                  node_where("the adapter's tree shows Notifications pressed",
                             _pressed("Notifications"))],
                 should="(does the change arrive on its own, late?)", record=6.0, tree=True):
        pass
    with run.act("Space again (turn it off), with no other update",
                 [event("object:state-changed:pressed", role="toggle button",
                        name_contains="Notifications")],
                 should="the toggle turns off and the bus says so", record=4.0, tree=True):
        run.key("space")
    with run.act("Tab to the next toggle",
                 [focused(role="toggle button", name="Dark mode")],
                 should="where the pending state change is flushed"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# scene-showcase: AT-SPI grab_focus on a lightweight item, focus elsewhere
# ---------------------------------------------------------------------------


def item_focus_body(run):
    run.wait_for(role="panel", name="draggable 1")
    with scene(run, "focus the toolbar's Theme combo box"):
        run.grab_focus(role="combo box", name="Theme")
    with run.act("AT-SPI grab_focus on the lightweight item 'draggable 1'",
                 [last_focus_is("focus lands on the item itself",
                                lambda n: n.get("name") == "draggable 1"),
                  said_something()],
                 should="focus lands on the item the reader asked for"):
        run.grab_focus(role="panel", name="draggable 1")
    with run.act("AT-SPI grab_focus on 'draggable 2' (focus now on the pane)",
                 [last_focus_is("focus lands on the item itself",
                                lambda n: n.get("name") == "draggable 2")],
                 should="focus lands on the item the reader asked for"):
        run.grab_focus(role="panel", name="draggable 2")


# ---------------------------------------------------------------------------
# scene-corkboard: the selection frame from the keyboard, the long way round
# ---------------------------------------------------------------------------


def _selected_card(n) -> bool:
    return n.get("role") == "panel" and n.get("name") == "Act I — Opening" \
        and "selected" in (n.get("states") or [])


def transform_body(run):
    run.wait_for(role="push button", name="Add Act")
    with scene(run, "focus the main pane's first card"):
        run.grab_focus(role="panel", name="Act I — Opening")
    with run.act("Enter on the card (selects it, enters editing)",
                 [node_where("the card is selected", _selected_card),
                  node_where("a selection frame exists",
                             lambda n: n.get("name") == "Selection")],
                 should="the card is selected and its prose takes focus", tree=True):
        run.key("Enter")
    with run.act("Escape", [said_something()],
                 should="Esc leaves editing for the card"):
        run.key("Escape")
    with run.act("AT-SPI grab_focus on the main pane",
                 [focused(role="panel", name="")],
                 should="focus is on the main pane (the pane the transform keys belong to)"):
        run.grab_focus(role="panel", name="")
    with run.act("t enters transform mode",
                 [last_focus_is("focus (or the active descendant) lands on a named handle",
                                lambda n: bool((n.get("name") or "").strip())
                                and n.get("role") in ("slider", "push button")),
                  said_something()],
                 should="the reader lands on a named handle of the selection frame",
                 tree=True):
        run.key("t")
    landed = run.last_focus()
    run.note(f"t landed on [{landed.get('role')}] {landed.get('name')!r}")
    for i in (1, 2):
        with run.act(f"Tab {i} roves the handles", [said_something()],
                     should="the reader hears the next handle"):
            run.key("Tab")
        landed = run.last_focus()
        run.note(f"Tab {i} in transform mode landed on [{landed.get('role')}] "
                 f"{landed.get('name')!r}")
    with run.act("Right arrow steps the handle", [said_something()],
                 should="the reader hears what changed"):
        run.key("Right")
    with run.act("Enter commits", [said_something()],
                 should="the reader hears the change was applied"):
        run.key("Enter")
    with run.act("Escape leaves transform mode", [said_something()],
                 should="focus returns to the pane and the reader hears it"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# animations-kit: content a sighted user cannot see, in the tree
# ---------------------------------------------------------------------------


def hidden_content_body(run):
    run.wait_for(role="toggle button", name="Animate me")
    with scene(run, "focus Animate me"):
        run.grab_focus(role="toggle button", name="Animate me")
    with scene(run, "Tab six times, to Toggle banner"):
        run.key(*(["Tab"] * 6), gap=0.6)
    run.note(f"six Tabs landed on {run.last_focus().get('name')!r}")
    with run.act("the Slide banner, slid out and faded at launch",
                 [node_where("the slid-out banner is NOT in the tree a reader walks",
                             lambda n: "Banner" in (n.get("name") or ""), want=False)],
                 should="content slid off its slot and faded to nothing is not read",
                 tree=True):
        pass
    with run.act("Tab to Submit (the Shake button)", [said_something()],
                 should="reached"):
        run.key("Tab")
    with run.act("Space on Submit (shakes the 'incorrect password' field)",
                 [said_something()],
                 should="the invalid-input feedback reaches the reader somehow"):
        run.key("space")
    with scene(run, "Tab four times, to Reveal / Hide"):
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.6)
    run.note(f"then four Tabs landed on {run.last_focus().get('name')!r}")
    with run.act("the Blur section's numbers, obscured at launch",
                 [node_where("the obscured card number is NOT in the tree a reader walks",
                             lambda n: "Account #" in (n.get("name") or ""), want=False),
                  node_where("the obscured CVV is NOT in the tree a reader walks",
                             lambda n: "CVV" in (n.get("name") or ""), want=False)],
                 should="numbers the page hides until 'Reveal' are not read to a screen "
                        "reader either",
                 tree=True):
        pass


SCENARIOS = [
    Scenario("verify-sceneetc-toggle-tree", "theme-styles", toggle_tree_body,
             "Space on a Toggle: stale tree or only a late event?"),
    Scenario("verify-sceneetc-item-focus", "scene-showcase", item_focus_body,
             "AT-SPI grab_focus on a lightweight scene item from outside the scene"),
    Scenario("verify-sceneetc-transform", "scene-corkboard", transform_body,
             "select a card with Enter, then the selection frame's keyboard route"),
    Scenario("verify-sceneetc-hidden-content", "animations-kit", hidden_content_body,
             "slid-out banner and blurred numbers in the tree a reader walks"),
]
