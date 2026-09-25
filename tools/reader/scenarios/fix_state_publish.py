# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A control's own state, told to the reader as it changes.

Checkbox, Toggle, RadioButton and Slider changed their state and told no
platform: the new state went out only with the next unrelated update of the
tree, most often the focus move that followed, where Orca's "checked" or "52"
was cut by the new focus. These acts state what a reader gets instead, on the
widget-catalog's Inputs page:

* `fix-state-publish-toggles`: Space and a screen reader's own click (the
  AT-SPI `click` action) on the two check boxes and the Enable feature toggle,
  Space on Option B; each change is on the bus within the act, the tree holds
  it, Orca says it, and a Tab afterwards carries nothing stale.
* `fix-state-publish-slider`: the vertical slider read as the 0.3 the app
  holds, and arrows on Volume and on the vertical slider, each new value on the
  bus within the act and spoken uncut, the third step up from 0.3 as 0.33 and
  not as the 0.32999998 its `f32` sums come to.

Orca speaks a radio button's checked change only after a Space it saw itself
(`orca/scripts/default.py`, `onCheckedChanged`), and the harness's keys do not
reach Orca's own keyboard, so Option B is judged on the bus alone.
"""

from __future__ import annotations

from typing import Any

from reader_lib.checks import custom, event, focused, in_tree, no_event, not_said, said
from reader_lib.scenario import Scenario

from scenarios.verify_catalog_a import held_seat

PKG = "widget-catalog"


def _scene(run: Any, label: str, fn) -> None:
    """Scene setting inside an act of its own, not judged (see
    `catalog_a.scene`)."""
    with run.act(f"(scene) {label}", should="scene setting, not judged",
                 settle=0.3, record=1.0):
        fn()


def _without_state(role: str, name: str, state: str):
    def run(act: Any) -> tuple[bool, list[str]]:
        stack = [act.tree] if act.tree else []
        while stack:
            node = stack.pop()
            if node.get("role") == role and node.get("name") == name:
                states = node.get("states", [])
                return state not in states, [f"[{role}] {name!r} states={states}"]
            stack.extend(node.get("children", []))
        return False, [f"no [{role}] {name!r} in the tree after the act"]
    return custom(f"the tree holds [{role}] {name!r} without {state!r}", run,
                  needs_tree=True)


def toggles(run: Any) -> None:
    run.wait_for(role="check box", name="Two-state checkbox")
    with held_seat(run):
        _scene(run, "focus the two-state check box",
               lambda: run.grab_focus(role="check box", name="Two-state checkbox"))
        with run.act("Space on the two-state check box",
                     [event("object:state-changed:checked", role="check box",
                            name_contains="Two-state", detail1=1),
                      said("checked"), not_said("not checked"),
                      in_tree(role="check box", name="Two-state checkbox", state="checked")],
                     should="the box is checked and the reader hears it at once",
                     record=2.0, tree=True):
            run.key("space")
        with run.act("AT-SPI click on the two-state check box",
                     [event("object:state-changed:checked", role="check box",
                            name_contains="Two-state", detail1=0),
                      said("not checked"),
                      _without_state("check box", "Two-state checkbox", "checked")],
                     should="a screen reader's own activation clears it, and the reader "
                            "hears it at once", record=2.0, tree=True):
            run.action("click", role="check box", name="Two-state checkbox")

        _scene(run, "focus the tristate check box",
               lambda: run.grab_focus(role="check box", name="Tristate checkbox"))
        with run.act("Space on the tristate check box",
                     [event("object:state-changed:checked", role="check box",
                            name_contains="Tristate", detail1=1),
                      said("checked"), not_said("not checked")],
                     should="the box is checked and the reader hears it at once",
                     record=2.0):
            run.key("space")

        _scene(run, "focus the Enable feature toggle",
               lambda: run.grab_focus(role="toggle button", name="Enable feature"))
        with run.act("Space on the Enable feature toggle",
                     [event("object:state-changed:pressed", role="toggle button",
                            name_contains="Enable feature", detail1=1),
                      said("pressed"), not_said("not pressed"),
                      in_tree(role="toggle button", name="Enable feature", state="pressed")],
                     should="the switch turns on and the reader hears it at once",
                     record=2.0, tree=True):
            run.key("space")
        with run.act("AT-SPI click on the Enable feature toggle",
                     [event("object:state-changed:pressed", role="toggle button",
                            name_contains="Enable feature", detail1=0),
                      said("not pressed")],
                     should="a screen reader's own activation turns it off, and the "
                            "reader hears it at once", record=2.0):
            run.action("click", role="toggle button", name="Enable feature")

        _scene(run, "focus Option B",
               lambda: run.grab_focus(role="radio button", name="Option B"))
        with run.act("Space on Option B",
                     [event("object:state-changed:checked", role="radio button",
                            name_contains="Option B", detail1=1),
                      event("object:state-changed:checked", role="radio button",
                            name_contains="Option A", detail1=0),
                      in_tree(role="radio button", name="Option B", state="checked")],
                     should="Option B becomes the selected one and Option A is cleared, "
                            "both on the bus at once", record=2.0, tree=True):
            run.key("space")

        with run.act("Tab onward",
                     [focused(),
                      no_event("object:state-changed:checked"),
                      no_event("object:state-changed:pressed")],
                     should="focus moves on, and no state change comes late with it"):
            run.key("Tab")


def slider(run: Any) -> None:
    run.wait_for(role="slider", name="Volume")
    with held_seat(run):
        with run.act("focus the vertical slider through AT-SPI",
                     [focused(role="slider", name="Vertical slider"), said("0.3"),
                      not_said("0.3000")],
                     should="the reader hears the 0.3 the app holds, not its binary noise"):
            run.grab_focus(role="slider", name="Vertical slider")
        with run.act("Up on the vertical slider",
                     [event("object:property-change:accessible-value", role="slider",
                            name_contains="Vertical"),
                      said("0.31"), not_said("0.3100")],
                     should="the value moves and the reader hears 0.31 at once", record=2.0):
            run.key("Up")
        # Each arrow adds an f32 step to an f32 value, and the third sum from
        # 0.3 is 0.32999998: the reader is to hear the step it reached.
        for figure, drift in (("0.32", "0.3199"), ("0.33", "0.3299")):
            with run.act(f"Up on the vertical slider to {figure}",
                         [event("object:property-change:accessible-value", role="slider",
                                name_contains="Vertical"),
                          said(figure), not_said(drift)],
                         should=f"the value moves and the reader hears {figure}, not the "
                                "drift of f32 sums", record=2.0):
                run.key("Up")

        _scene(run, "focus the Volume slider",
               lambda: run.grab_focus(role="slider", name="Volume"))
        for n in (1, 2):
            with run.act(f"Right on Volume ({n})",
                         [event("object:property-change:accessible-value", role="slider",
                                name_contains="Volume"),
                          said(str(50 + n))],
                         should=f"the value moves to {50 + n} and the reader hears it at once",
                         record=2.0):
                run.key("Right")

        with run.act("Tab onward",
                     [focused(), no_event("object:property-change:accessible-value")],
                     should="focus moves on, and no value change comes late with it"):
            run.key("Tab")


SCENARIOS = [
    Scenario("fix-state-publish-toggles", PKG, toggles,
             "check boxes, a toggle and a radio button: each change told at once",
             args=["--tab", "inputs"]),
    Scenario("fix-state-publish-slider", PKG, slider,
             "slider values: each change told at once, and read as the app holds it",
             args=["--tab", "inputs"]),
]
