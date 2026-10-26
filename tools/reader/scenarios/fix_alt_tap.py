# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A bare Alt tap puts the menu bar in keyboard mode; the release of an Alt
held while a window takes or loses the focus does not.

A window read the release of Alt as a tap whenever its modifier state went
from Alt to nothing with no other key between. So the window behind one
Alt+F4 closed, or the one Alt+Tab brought forward, gave its first menu title
the keyboard, and the window Alt+Tab left could too. These acts hold Alt with
real keys while the private KWin closes or activates a window, as Alt+F4 and
Alt+Tab would (the run's KWin takes no global shortcuts), then release it.

`fix-alt-tap` runs native-menu on Wayland. `fix-alt-tap-x11` runs it on the
run's own Xwayland, which needs the session started with
`TEKSILO_READER_XWAYLAND=1`:

    TEKSILO_READER_XWAYLAND=1 tools/reader/reader.py run fix-alt-tap-x11
"""

from __future__ import annotations

import os
import subprocess

from reader_lib.checks import Check, _focus_node, _is_focus, custom, focused
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

MAIN = "Teksilo: Native Menu"
INSPECTOR = "Teksilo: Inspector"


def menu_bar_left_alone() -> Check:
    """No focus change in the act lands on a menu bar title."""
    def run_(act) -> tuple[bool, list[str]]:
        hits = [_focus_node(e) for e in act.events if _is_focus(e)]
        bad = [n for n in hits if n.get("role") == "menu item"]
        lines = [f"focus on [{n.get('role')}] {n.get('name')!r}" for n in hits]
        return not bad, lines or ["no focus change on the bus in this act"]
    return custom("the keyboard does not go to a menu bar title", run_)


def open_inspector(run) -> None:
    run.key("Alt+W", "I")
    run.wait_for(role="frame", name=INSPECTOR)
    run.wait(1.0)


def on_x11(run, display: str) -> None:
    """The run's X server holds the main window: the example is on X11, not
    on Wayland with the X11 variables ignored."""
    try:
        tree = subprocess.run(["xwininfo", "-root", "-tree"], env={**os.environ, "DISPLAY": display},
                              capture_output=True, text=True, timeout=10).stdout
    except FileNotFoundError:
        run.note("xwininfo is not installed: whether the example ran on X11 is not checked")
        return
    lines = [line.strip() for line in tree.splitlines() if MAIN in line]
    if not lines:
        raise RunError(f"no window titled {MAIN!r} on the run's X display {display}")
    run.note(f"on the run's X display {display}: " + "; ".join(lines))


def body(run) -> None:
    display = run.scenario.env.get("DISPLAY")
    if display == "":
        raise RunError("the X11 run needs the session started with TEKSILO_READER_XWAYLAND=1")
    run.wait_for(role="menu item", name="File")
    if display:
        on_x11(run, display)
    with run.act("a bare Alt tap", [focused(role="menu item", name="File")],
                 should="focus on the File menu bar item"):
        run.key("Alt")
    run.key("Escape")
    run.wait(0.5)

    open_inspector(run)
    with run.act("Alt held while KWin closes the Inspector, as Alt+F4 would",
                 [menu_bar_left_alone()],
                 should="the main window comes back with its menu bar left alone"):
        run.hold("Alt")
        run.wait(0.3)
        run.close_window(INSPECTOR)
        run.wait(1.0)
        run.release("Alt")
        run.wait(0.8)

    open_inspector(run)
    run.activate_window(MAIN)
    run.wait(1.0)
    with run.act("Alt held while KWin moves the focus to the Inspector, as Alt+Tab would",
                 [menu_bar_left_alone()],
                 should="the Inspector takes the focus with its menu bar left alone"):
        run.hold("Alt")
        run.wait(0.3)
        run.activate_window(INSPECTOR)
        run.wait(1.0)
        run.release("Alt")
        run.wait(0.8)
    with run.act("back to the main window", [menu_bar_left_alone()],
                 should="the window Alt left kept its menu bar out of it too"):
        run.activate_window(MAIN)
        run.wait(1.0)

    with run.act("a bare Alt tap after all that", [focused(role="menu item", name="File")],
                 should="focus on the File menu bar item"):
        run.key("Alt")


SCENARIOS = [
    Scenario("fix-alt-tap", "native-menu", body,
             "Alt held across a window closing or activating, on Wayland"),
    Scenario("fix-alt-tap-x11", "native-menu", body,
             "Alt held across a window closing or activating, on X11",
             env={"WAYLAND_DISPLAY": "",
                  "DISPLAY": os.environ.get("TEKSILO_READER_X11_DISPLAY", "")}),
]
