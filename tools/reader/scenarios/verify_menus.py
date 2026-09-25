# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for menus-and-dropdowns (the sweep's own acts are in
`menus.py`; these only add what its scenarios did not do).

* `verify-menus-submenu-held`: does the keyboard-opened Recent submenu close
  on its own, or because the harness's key client (`fake_key`) connects and
  disconnects a KWin fake-input device around every key press? The Right key
  is pressed by one `fake_key` process that stays connected for 3 s after the
  press; an ordinary `run.key("Right")` follows as the control.
* `verify-menus-rich-keys`: inside the View options menu, do keys reach any
  focused embedded control (Enter on an icon button, Space on a toggle, the
  arrows / Home / PageUp on the slider, Down on the embedded combo box), or
  only the slider's?
"""

from __future__ import annotations

import subprocess
import time

from reader_lib import keys as keymap
from reader_lib.checks import _focus_node, _is_focus, custom, event, focused
from reader_lib.scenario import Scenario
from scenarios.menus import COMBO, focus_combo, menus_open, printed, spoke_anything

PKG = "menus-and-dropdowns"


def held_keys(run, steps: list[str], label: str) -> float:
    """Run one `fake_key` process with `steps` (which may hold `wait:ms`),
    and return when it exited, in ms from the act's start."""
    run._step(f"fake_key held: {label} ({' '.join(steps)})")
    done = subprocess.run([str(run.fake_key), *steps], capture_output=True, text=True,
                          timeout=60)
    exit_ms = (time.monotonic() - run.current.start_mono) * 1000.0
    if done.returncode != 0:
        raise RuntimeError(f"fake_key failed: {done.stderr.strip()}")
    run.note(f"{label}: the key client exited {exit_ms:.0f} ms after the act started")
    return exit_ms


def menu_lifetimes(exit_box: list) -> object:
    """Record when a [menu] was added under the frame and removed from it, in
    ms from the act's start, against when the key client exited. Always
    passes: it is the record the verdict is read from."""
    def run(act):
        rows = []
        for e in act.events:
            if e["type"].startswith("object:children-changed") and \
                    (e.get("target") or {}).get("role") == "menu":
                rows.append(f"{act.rel_ms(e):+8.1f} ms {e['type']} -> [menu]")
        rows.append(f"key client exited at {exit_box[0]:+.0f} ms" if exit_box
                    else "key client exit time not recorded")
        return True, rows
    return custom("when a menu was added / removed, against the key client's exit (a record)",
                  run)


def submenu_held(run):
    run.wait_for(role="menu item", name="File")
    focus_combo(run, "fruit")
    with run.act("Alt+F, Down x4 (Recent highlighted)", [focused(role="menu")],
                 should="File opens and the highlight reaches Recent"):
        run.key("Alt+F")
        run.wait(0.4)
        run.key("Down", "Down", "Down", "Down")
    exit_a: list = []
    with run.act("Right, pressed by a key client that stays connected 3 s",
                 [menu_lifetimes(exit_a), menus_open(2)],
                 should="the submenu opens and stays open; if it closes, it closes "
                 "about 150 ms after the key client disconnects, not after the press",
                 record=2.5, tree=True):
        exit_a.append(held_keys(run, keymap.chord_steps("Right") + ["wait:3000"],
                                "Right held 3 s"))
    exit_b: list = []
    with run.act("Right again, ordinary key press (control)",
                 [menu_lifetimes(exit_b), menus_open(2)],
                 should="the submenu opens again; with an ordinary press the key client "
                 "disconnects at once", record=2.5, tree=True):
        t0 = time.monotonic()
        run.key("Right")
        exit_b.append((time.monotonic() - run.current.start_mono) * 1000.0)
        run.note(f"ordinary Right: run.key took {(time.monotonic() - t0) * 1000:.0f} ms")
    exit_c: list = []
    log = printed(run)
    with run.act("Right, then Enter 1.5 s later, one key client held throughout",
                 [menu_lifetimes(exit_c), log.check("Recent: project-alpha"),
                  spoke_anything()],
                 should="the submenu opens, stays open while no pointer event arrives, and "
                 "Enter on its highlighted first item opens project-alpha.toml",
                 record=2.5, tree=True):
        exit_c.append(held_keys(
            run, keymap.chord_steps("Right") + ["wait:1500"]
            + keymap.chord_steps("Return") + ["wait:1500"], "Right, Enter, held"))
    log.done()


def focus_record():
    def run(act):
        moves = [_focus_node(e) for e in act.events if _is_focus(e)]
        return True, [" -> ".join(f"[{n.get('role')}] {n.get('name')!r}" for n in moves)
                      or "no focus change"]
    return custom("the focus landings of the act (a record)", run)


def rich_keys(run):
    run.wait_for(role="combo box", nth=COMBO["huge"])
    focus_combo(run, "huge")
    with run.act("Tab x3 to View options", [focused(role="push button", name="View options")],
                 should="focus on View options"):
        run.key("Tab", "Tab", "Tab", gap=0.4)
    with run.act("Enter opens View options", [focused(role="menu")], should="the menu opens",
                 tree=True):
        run.key("Return")
    with run.act("Tab to Previous", [focused(name="Previous")], should="focus on Previous"):
        run.key("Tab")
    log = printed(run)
    with run.act("Enter on the embedded Previous button", [log.check("Prev"), focus_record()],
                 should="Previous runs (the example prints 'Prev')"):
        run.key("Return")
    log.done()
    # Enter may have closed the menu; reopen if so.
    if not run.find(role="menu"):
        run.note("the menu closed after Enter on Previous; reopening it")
        with run.act("reopen View options (Enter on it)", [focus_record()],
                     should="focus back in the menu"):
            if (run.last_focus() or {}).get("name") != "View options":
                run.grab_focus(role="push button", name="View options")
                run.wait(0.6)
            run.key("Return")
        with run.act("Tab to Previous again", [focused(name="Previous")], should="Previous"):
            run.key("Tab")
    with run.act("Tab x4 to the Pin toggle", [focused(name="Pin (bistate)")],
                 should="focus on the Pin toggle"):
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.4)
    log = printed(run)
    with run.act("Space on the Pin toggle",
                 [log.check("TogglePin"), event("object:state-changed:pressed"), focus_record()],
                 should="the toggle flips; the example prints 'TogglePin'"):
        run.key("space")
    log.done()
    if not run.find(role="menu"):
        run.note("the menu closed after Space on Pin; the slider acts below will say so")
    for i in range(8):
        with run.act(f"Tab {i + 1} toward the slider", [focus_record()], settle=0.6,
                     record=1.2) as act:
            run.key("Tab")
        run.collect_events(act)
        moves = [e for e in act.events if _is_focus(e)]
        node = _focus_node(moves[-1]) if moves else {}
        if node.get("role") == "slider":
            break
    for chord in ("Right", "Up", "Page_Up", "Home", "Left"):
        with run.act(f"{chord} on the slider",
                     [event("object:property-change:accessible-value", role="slider"),
                      focus_record()],
                     should="the slider's value changes"):
            run.key(chord)
    with run.act("Tab to the embedded Theme combo box",
                 [focused(role="combo box"), focus_record()], should="focus on the combo box"):
        run.key("Tab")
    with run.act("Down on the embedded combo box",
                 [event("object:selection-changed"), spoke_anything(), focus_record()],
                 should="the combo box's list opens / selection moves, and is spoken",
                 tree=True):
        run.key("Down")
    try:
        bridge = run.bridge()
        snap = bridge.call("snapshot_tree")
        stack = [snap]
        found = []
        while stack:
            cur = stack.pop()
            if isinstance(cur, dict):
                if cur.get("label") in ("Opacity", "Pin (bistate)") or \
                        cur.get("role") in ("Slider", "ComboBox"):
                    found.append({k: v for k, v in cur.items() if k != "children"})
                stack.extend(cur.values())
            elif isinstance(cur, list):
                stack.extend(cur)
        run.note(f"after every act, through the bridge: {str(found)[:900]}")
    except Exception as exc:
        run.note(f"the bridge could not be read: {exc}")


def scroll_visited(run):
    """The Country combo box is visited first (so Orca has met it), then the
    page is scrolled away from it by Tab and back by Shift+Tab."""
    from reader_lib.checks import said
    run.wait_for(role="combo box", nth=COMBO["country"])
    focus_combo(run, "country")
    with run.act("Tab to Huge", [focused(role="combo box")], should="Huge"):
        run.key("Tab")
    with run.act("Tab to Add (the page scrolls)", [focused(name="Add")], should="Add"):
        run.key("Tab")
    with run.act("Tab to Search (the combo boxes leave the view)",
                 [focused(name="Search"), focus_record()], should="Search", tree=True):
        run.key("Tab")
    with run.act("Shift+Tab back to Add", [focused(name="Add")], should="Add"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab back to Huge", [focused(role="combo box"), said("combo box")],
                 should="Huge, spoken"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab back to Country (visited before the scroll)",
                 [focused(role="combo box"), said("combo box")],
                 should="Country, spoken as it was the first time"):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to Color (never visited)",
                 [focused(role="combo box"), said("combo box")],
                 should="Color, spoken"):
        run.key("Shift+Tab")


def disabled_bridge(run):
    """What the application itself says of the two items built with
    enabled(false), read through the bridge (not a measurement of what a
    reader gets: the AT-SPI side is in the tree snapshot beside it)."""
    run.wait_for(role="menu item", name="Disabled item")
    run.snapshot("launch, AT-SPI side")
    try:
        bridge = run.bridge()
        snap = bridge.call("snapshot_tree")
        stack = [snap]
        found = []
        while stack:
            cur = stack.pop()
            if isinstance(cur, dict):
                if cur.get("label") in ("Disabled item", "Copy item", "Cut item") or \
                        (cur.get("role") == "MenuItem" and "disabled" in str(cur)):
                    found.append({k: v for k, v in cur.items() if k != "children"})
                stack.extend(cur.values())
            elif isinstance(cur, list):
                stack.extend(cur)
        run.note(f"bridge: {str(found)[:1500]}")
    except Exception as exc:
        run.note(f"the bridge could not be read: {exc}")


SCENARIOS = [
    Scenario("verify-menus-submenu-held", PKG, submenu_held,
             "Right opens File > Recent with the key client held connected: does the "
             "submenu close by itself or when the client leaves?"),
    Scenario("verify-menus-rich-keys", PKG, rich_keys,
             "View options: do keys reach embedded controls (buttons, toggle, slider, combo)?"),
    Scenario("verify-menus-scroll-visited", PKG, scroll_visited,
             "a combo box Orca has met, scrolled out of view and back"),
    Scenario("verify-menus-disabled-bridge", PKG, disabled_bridge,
             "the application's own record of the disabled inline menu item"),
]
