# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for widget-catalog part A (the sweep's own acts are
in `catalog_a.py`; these only add what its scenarios did not do).

* `verify-catalog-a-combo-reopen`: the Inputs page's fruit combo box opened,
  a fruit chosen, then opened again. The list's rows leave the tree when the
  popup closes and come back with the same ids: are they heard the second
  time, and is the current fruit said on the reopen?
* `verify-catalog-a-at-click`: a screen reader's own activation (the AT-SPI
  `click` action) on a check box and on the Enable feature toggle, then a
  long quiet wait, then a key that changes nothing: does the state change
  reach the bus in answer to the action, or only with a later focus move?
* `verify-catalog-a-bounce-then-tab`: after the Theme tooltip's fade has
  thrown focus back to Theme, where the next two Tabs go.
"""

from __future__ import annotations

import subprocess
import time
from contextlib import contextmanager
from typing import Any, Iterator

from reader_lib.checks import (_focus_node, _is_focus, custom, event, focused, no_event,
                               said)
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

PKG = "widget-catalog"


@contextmanager
def held_seat(run: Any) -> Iterator[None]:
    """Keep one fake-input client connected for the whole scenario, so the
    seat does not gain and lose a pointer with every key press (winit's
    `failed to get pointer data` panic in the private KWin)."""
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


def tab_to(run: Any, name: str, role: str | None = None, limit: int = 40,
           chord: str = "Tab") -> dict:
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


def scene(run: Any, label: str, fn, *, record: float = 1.0) -> None:
    with run.act(f"(scene) {label}", should="scene setting, not judged",
                 settle=0.3, record=record):
        fn()


def _focus_trail():
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        return True, [f"{act.rel_ms(e):+8.1f} ms {e['type']} -> "
                      f"[{_focus_node(e).get('role')}] {_focus_node(e).get('name')!r}"
                      for e in moves] or ["no focus change"]
    return custom("(record) where focus went in the act", run)


def _no_defunct_drop():
    def run(act):
        drops = [f"{line.stamp} {line.text}" for line in act.orca if line.is_defunct_drop]
        return not drops, drops
    return custom("Orca ignores nothing as defunct", run, needs_orca=True)


# ---------------------------------------------------------------------------


def combo_reopen(run):
    run.wait_for(role="page tab", name="Inputs")
    with held_seat(run):
        scene(run, "Tab to the last radio tile", lambda: (
            run.grab_focus(role="page tab", name="Inputs"),
            tab_to(run, "Notebook", role="radio button", limit=30)))
        scene(run, "Tab onto the fruit combo box", lambda: run.key("Tab"))
        with run.act("Space: open the combo box (1)", [said("Apple"), _focus_trail()],
                     should="the list opens and the reader hears where they are",
                     tree=True):
            run.key("space")
        with run.act("Down (1)", [said("Banana"), _no_defunct_drop()],
                     should="the reader hears Banana"):
            run.key("Down")
        with run.act("Enter: choose Banana", [_focus_trail()],
                     should="the list closes, the combo box holds Banana"):
            run.key("Return")
        with run.act("Space: open the combo box again (2)",
                     [said("Banana"), _no_defunct_drop(), _focus_trail()],
                     should="the list opens on the current fruit and the reader hears it",
                     tree=True):
            run.key("space")
        with run.act("Down (2)", [said("Cherry"), _no_defunct_drop(),
                                  event("object:state-changed:selected", role="list item")],
                     should="the reader hears Cherry"):
            run.key("Down")
        with run.act("Up (2)", [said("Banana"), _no_defunct_drop()],
                     should="the reader hears Banana"):
            run.key("Up")
        with run.act("Escape", [focused(role="combo box")],
                     should="the list closes and focus is on the combo box"):
            run.key("Escape")


def at_click(run):
    run.wait_for(role="check box", name="Two-state checkbox")
    with held_seat(run):
        scene(run, "focus the two-state check box",
              lambda: run.grab_focus(role="check box", name="Two-state checkbox"))
        with run.act("AT-SPI click on the two-state check box",
                     [event("object:state-changed:checked", role="check box"), said("checked")],
                     should="a screen reader's activation checks it and the reader hears it",
                     record=3.0):
            run.action("click", role="check box", name="Two-state checkbox")
        with run.act("wait 5 s", [no_event("object:state-changed:checked")],
                     should="nothing (the change should already have been sent)",
                     settle=0.1, record=5.0):
            pass
        with run.act("Shift (a key that changes nothing)",
                     [no_event("object:state-changed:checked")],
                     should="nothing", record=2.0):
            run.key("Shift")
        with run.act("Tab to the tristate check box",
                     [focused(role="check box", name="Tristate checkbox"), _focus_trail()],
                     should="focus moves on (the late state change, if any, shows here)"):
            run.key("Tab")
        scene(run, "focus the Enable feature toggle",
              lambda: run.grab_focus(role="toggle button", name="Enable feature"))
        with run.act("AT-SPI click on the Enable feature toggle",
                     [event("object:state-changed:pressed", role="toggle button"),
                      said("pressed")],
                     should="a screen reader's activation turns it on and the reader hears it",
                     record=3.0):
            run.action("click", role="toggle button", name="Enable feature")
        with run.act("Tab onward", [_focus_trail()],
                     should="focus moves on (the late state change, if any, shows here)"):
            run.key("Tab")


def bounce_then_tab(run):
    run.wait_for(role="combo box", name="Theme")
    with held_seat(run):
        scene(run, "Tab to Text scale",
              lambda: tab_to(run, "Text scale", role="spin button", limit=10))
        with run.act("Tab onto Theme", [focused(role="combo box", name="Theme")],
                     should="focus lands on Theme; its tooltip shows 0.7 s later",
                     record=1.0):
            run.key("Tab")
        with run.act("Tab away (bounces back to Theme)", [_focus_trail()],
                     should="focus moves on to the Palette tab and stays", settle=0.1,
                     record=1.5):
            run.key("Tab")
        with run.act("Tab again", [_focus_trail(), focused(role="page tab", name="Palette")],
                     should="focus moves on to the Palette tab", settle=0.1, record=1.5,
                     tree=True):
            run.key("Tab")
        with run.act("Tab once more", [_focus_trail()],
                     should="focus moves on", settle=0.1, record=1.5):
            run.key("Tab")


def wizard_trigger(run):
    """The Chrome page's Wizard is given a `Button` as its trigger
    (`examples/widget_catalog/src/tabs/chrome.rs` `.trigger(Button::new(..))`),
    which `Wizard::build` wraps in a focusable, named `OverlayTrigger`
    (`stepper/wizard.rs`). Where does Tab land, and does the key a reader
    presses there open the wizard?"""
    run.wait_for(role="page tab", name="Chrome")
    with held_seat(run):
        scene(run, "Tab to Open wizard", lambda: (
            run.grab_focus(role="page tab", name="Chrome"),
            tab_to(run, "Open wizard", role="push button", limit=12)))
        with run.act("Space on Open wizard", [said("Onboarding"), said("Welcome"),
                                              _focus_trail()],
                     should="the wizard opens; the reader hears its name and the first step",
                     record=3.0, tree=True):
            run.key("space")
        with run.act("Escape", [_focus_trail(),
                                focused(role="push button", name="Open wizard")],
                     should="the wizard closes and focus is back on its trigger", record=2.0):
            run.key("Escape")
        scene(run, "Tab to Next", lambda: tab_to(run, "Next", role="push button", limit=3))
        with run.act("Enter on Next", [said("Configure"), _focus_trail()],
                     should="the wizard moves to step 2 and the reader hears which step",
                     record=3.0, tree=True):
            run.key("Return")
        scene(run, "Tab to Cancel", lambda: tab_to(run, "Cancel", role="push button", limit=4))
        with run.act("Space on Cancel", [focused(role="push button", name="Open wizard"),
                                         _focus_trail()],
                     should="the wizard closes and focus is back on its trigger", record=2.5):
            run.key("space")
        with run.act("AT-SPI click on Open wizard", [said("Onboarding"), _focus_trail()],
                     should="a screen reader's activation of the trigger opens the wizard",
                     record=3.0):
            run.action("click", role="push button", name="Open wizard")


def scroll_back(run):
    """The Inputs page walked down to its last control (the page scrolls, and
    what scrolls out of the ScrollArea leaves the filtered tree:
    `accesskit_consumer` `filters.rs:64-86`), then walked back up with
    Shift+Tab: are the controls the reader met on the way down heard on the
    way back?"""
    from reader_lib.scenario import tab_walk

    run.wait_for(role="page tab", name="Inputs")
    with held_seat(run):
        scene(run, "Tab down the page to the fruit combo box (every stop heard)", lambda: (
            run.grab_focus(role="page tab", name="Inputs"),
            tab_to(run, "Notebook", role="radio button", limit=30),
            run.key("Tab")), record=1.5)
        tab_walk(run, stops=16, chord="Shift+Tab", label="Shift+Tab")


SCENARIOS = [
    Scenario("verify-catalog-a-scroll-back", PKG, scroll_back,
             "Inputs page walked down, then back up with Shift+Tab",
             args=["--tab", "inputs"]),
    Scenario("verify-catalog-a-wizard-trigger", PKG, wizard_trigger,
             "the Chrome page's Wizard trigger (a Button inside an OverlayTrigger) by keyboard",
             args=["--tab", "chrome"]),
    Scenario("verify-catalog-a-combo-reopen", PKG, combo_reopen,
             "fruit combo box opened, a fruit chosen, opened again", args=["--tab", "inputs"]),
    Scenario("verify-catalog-a-at-click", PKG, at_click,
             "AT-SPI click on a check box and a toggle: when the state change is sent",
             args=["--tab", "inputs"]),
    Scenario("verify-catalog-a-bounce-then-tab", PKG, bounce_then_tab,
             "where Tab goes after the Theme tooltip bounced focus back",
             args=["--tab", "palette"]),
]
