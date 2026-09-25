# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's own acts for widget-catalog part C (overlays tab).

Each scenario isolates one claim of the catalog-c sweep that its own acts
left mixed with other state:

* `verify-catalog-c-bounce-cold`: the focus bounce after Tab away from a
  control whose tooltip is up, from a cold start (the first tooltip of the
  run is shown with a fade; a warm reshow is not), for a composite tooltip
  and the title bar's Theme combo box.
* `verify-catalog-c-tooltip-reshow`: Tab into a sticky composite tooltip on
  its first show, then again on its second show (same content node).
* `verify-catalog-c-notices`: the snackbar through its outer trigger node, a
  toast arriving while focus rests on the bell with the log closed, and
  Escape on a toast that holds focus (focus given through AT-SPI, not by
  opening the log, which opens under the harness's stationary pointer).
* `verify-catalog-c-popover`: Escape straight after opening the popover, and
  Shift+Tab inside it.

Every scene-setting step runs inside an act of its own, so Orca's speech is
credited where it belongs.
"""

from __future__ import annotations

from reader_lib.checks import (_focus_node, _is_focus, announced, custom, focused, said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

PKG = "widget-catalog"


def setup(run, label, step, record: float = 1.0) -> None:
    with run.act(f"set the scene: {label}", should="scene setting, not judged", record=record):
        step()


def focus_lines(act, last: int = 8) -> list[str]:
    moves = [e for e in act.events if _is_focus(e)]
    return [f"{act.rel_ms(e):+.1f} ms focus -> [{_focus_node(e).get('role')}] "
            f"{_focus_node(e).get('name')!r} {_focus_node(e).get('path', '')[-32:]}"
            for e in moves[-last:]]


def focus_stays(role: str, name: str):
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        lines = focus_lines(act)
        if not moves:
            return False, ["no focus change in the act"]
        landed = [i for i, e in enumerate(moves)
                  if _focus_node(e).get("role") == role and _focus_node(e).get("name") == name]
        if not landed:
            return False, lines
        after = moves[landed[0]:]
        return all(_focus_node(e).get("name") == name for e in after), lines
    return custom(f"focus lands on [{role}] {name!r} and stays there", run)


def orca_heard_focus():
    """Orca did not drop the act's focus event as defunct."""
    def run(act):
        drops = [f"{line.stamp} {line.text}" for line in act.orca if line.is_defunct_drop]
        said_ = [u.text for u in utterances(act.orca)]
        return not drops, drops + [f"Orca said {t!r}" for t in said_] + focus_lines(act)
    return custom("Orca handles the focus event (no defunct drop)", run, needs_orca=True)


def not_defunct_bell(act):
    lines = []
    dead = False
    for e in act.events:
        src = e.get("source", {})
        if e["type"] == "object:state-changed:defunct" and src.get("name") == "Notifications":
            dead = True
            lines.append(f"{act.rel_ms(e):+.1f} ms defunct [push button] 'Notifications' "
                         f"{src.get('path', '')[-32:]}")
        if _is_focus(e) or e["type"] == "object:state-changed:focused":
            lines.append(f"{act.rel_ms(e):+.1f} ms {e['type']} {e.get('detail1')} "
                         f"[{src.get('role')}] {src.get('name')!r} {src.get('path', '')[-32:]}")
    return not dead, lines or ["nothing happened to the bell"]


def last_focus_not_frame(act):
    moves = [e for e in act.events if _is_focus(e)]
    if not moves:
        return False, ["no focus change in the act"]
    return _focus_node(moves[-1]).get("role") != "frame", focus_lines(act)


# ---------------------------------------------------------------------------


def bounce_cold(run):
    run.wait_for(role="push button", name="Province info")
    with run.act("cold: focus 'Province info', wait 1.5 s (tooltip up), Tab",
                 [focus_stays("push button", "Tabbed details")],
                 should="focus moves to 'Tabbed details' and stays", settle=0.5, record=2.0):
        run.grab_focus(role="push button", name="Province info")
        run.wait(1.5)
        run.key("Tab")
    setup(run, "Escape, focus Save, wait 4 s for the tooltip session to cool",
          lambda: (run.key("Escape"), run.grab_focus(role="push button", name="Save"),
                   run.wait(4.0)))
    with run.act("cold: focus the Theme combo box, wait 1.5 s (tooltip up), Tab",
                 [custom("focus leaves the Theme combo box and stays gone",
                         lambda act: (
                             bool([e for e in act.events if _is_focus(e)]) and
                             _focus_node([e for e in act.events if _is_focus(e)][-1])
                             .get("name") != "Theme", focus_lines(act)))],
                 should="focus moves on from Theme and stays there", settle=0.5, record=2.0):
        run.grab_focus(role="combo box", name="Theme")
        run.wait(1.5)
        run.key("Tab")
    setup(run, "Escape, focus Save, wait 4 s for the tooltip session to cool",
          lambda: (run.key("Escape"), run.grab_focus(role="push button", name="Save"),
                   run.wait(4.0)))
    with run.act("cold: focus 'Hover or hold — level 3', wait 1 s (rich tooltip up), Tab",
                 [focus_stays("push button", "Plain among rich")],
                 should="focus moves to 'Plain among rich' and stays", settle=0.5, record=2.0):
        run.grab_focus(role="push button", name="Hover or hold — level 3")
        run.wait(1.0)
        run.key("Tab")


def tooltip_reshow(run):
    run.wait_for(role="push button", name="With internal Button")
    with run.act("first show: focus 'With internal Button', wait 3.2 s, Tab into the tooltip",
                 [focused(role="dialog"), said("Treasury report"), orca_heard_focus()],
                 should="focus enters the sticky tooltip and the reader hears it",
                 settle=0.5, record=2.0):
        run.grab_focus(role="push button", name="With internal Button")
        run.wait(3.2)
        run.key("Tab")
    with run.act("Escape back to the button",
                 [focused(role="push button", name="With internal Button")],
                 should="the tooltip closes and focus returns to its button", record=1.5):
        run.key("Escape")
    setup(run, "focus 'Plain among rich', wait 3 s",
          lambda: (run.grab_focus(role="push button", name="Plain among rich"),
                   run.wait(3.0)))
    with run.act("second show: focus 'With internal Button', wait 3.2 s, Tab into the tooltip",
                 [focused(role="dialog"), said("Treasury report"), orca_heard_focus()],
                 should="the second showing is heard as the first was",
                 settle=0.5, record=2.0):
        run.grab_focus(role="push button", name="With internal Button")
        run.wait(3.2)
        run.key("Tab")


def notices(run):
    run.wait_for(role="push button", name="Show snackbar")
    setup(run, "focus 'Show snackbar'",
          lambda: run.grab_focus(role="push button", name="Show snackbar"))
    with run.act("AT-SPI click on the focused inner 'Show snackbar' button",
                 [custom("some announcement reaches the bus",
                         lambda act: (any(e["type"] == "object:announcement"
                                          for e in act.events),
                                      [e.get("text", "") for e in act.events
                                       if e["type"] == "object:announcement"]
                                      or ["no announcement"]))],
                 should="a reader's activation opens the snackbar", record=4.0):
        run.action("click", role="push button", name="Show snackbar")
    with run.act("AT-SPI click on the outer trigger node 'File saved successfully'",
                 [announced("Snackbar")],
                 should="control: the outer node's Click is the one that works", record=4.0):
        run.action("click", role="push button", name="File saved successfully")
    setup(run, "focus the Notifications bell (log closed)",
          lambda: run.grab_focus(role="push button", name_contains="Notifications"))
    with run.act("a toast arrives (AT-SPI click on Info) while focus rests on the bell",
                 [custom("the focused bell is not destroyed", not_defunct_bell),
                  said("Info notice")],
                 should="the toast is heard; the bell keeps focus as the same node",
                 record=3.0):
        run.action("click", role="push button", name="Info")
    setup(run, "show a Loading toast (AT-SPI click)",
          lambda: run.action("click", role="push button", name="Loading"), record=2.0)
    setup(run, "focus the toast's status node through AT-SPI",
          lambda: run.grab_focus(role="status bar", name_contains="Working"), record=1.5)
    with run.act("Escape on the focused toast",
                 [custom("focus lands on a control, not the window", last_focus_not_frame)],
                 should="the toast goes and focus returns to where the reader was",
                 record=2.0):
        run.key("Escape")


def popover(run):
    run.wait_for(role="push button", name="Anchor")
    setup(run, "focus Anchor", lambda: run.grab_focus(role="push button", name="Anchor"))
    with run.act("Space on Anchor (popover)", should="the popover opens", tree=True):
        run.key("space")
    with run.act("Escape at once", [focused(role="push button", name="Anchor"), said("Anchor")],
                 should="the popover closes and focus returns to Anchor"):
        run.key("Escape")
    with run.act("Space on Anchor again", should="the popover opens a second time"):
        run.key("space")
    with run.act("Shift+Tab inside the open popover",
                 [custom("focus goes back to Anchor or stays in the popover",
                         lambda act: (bool([e for e in act.events if _is_focus(e)]) and
                                      _focus_node([e for e in act.events if _is_focus(e)][-1])
                                      .get("name") in ("Anchor", ""), focus_lines(act)))],
                 should="the reader stays near the popover"):
        run.key("Shift+Tab")


SCENARIOS = [
    Scenario("verify-catalog-c-bounce-cold", PKG, bounce_cold,
             "overlays: Tab away from a cold-shown tooltip (composite, Theme combo, rich)",
             args=["--tab", "overlays"]),
    Scenario("verify-catalog-c-tooltip-reshow", PKG, tooltip_reshow,
             "overlays: Tab into a sticky composite tooltip on its first and second show",
             args=["--tab", "overlays"]),
    Scenario("verify-catalog-c-notices", PKG, notices,
             "overlays: snackbar trigger nodes, toast vs focused bell, Escape on a toast",
             args=["--tab", "overlays"]),
    Scenario("verify-catalog-c-popover", PKG, popover,
             "overlays: popover Escape and Shift+Tab", args=["--tab", "overlays"]),
]
