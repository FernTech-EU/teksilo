# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's extra acts for spin-box (the sweep's own acts are in
`spinbox.py`; these only add what its scenarios did not do).

* After a spin box scrolls out and back, what does a *fresh* AT-SPI client
  (one that never saw the `defunct` event) read as its states? That tells a
  stale client cache from a state AccessKit itself reports.
* A commit that clamps or reverts to the value the box already held: is the
  reader told anything?
* After a live switch to French, leave a box that was edited while focused:
  does its unfocused text go back to the French form?
"""

from __future__ import annotations

import json
import subprocess
import sys
import textwrap

from reader_lib.checks import custom, focused, said
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario
from scenarios.spinbox import setup_focus, spin_state, spin_value, tab_until

PACKAGE = "spin-box"

_FRESH_CLIENT = textwrap.dedent("""
    import json, sys
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
    want = sys.argv[1]
    desktop = Atspi.get_desktop(0)
    stack = []
    for i in range(desktop.get_child_count()):
        app = desktop.get_child_at_index(i)
        if app is not None and app.get_name() == "spin-box":
            stack.append(app)
    found = []
    while stack:
        node = stack.pop()
        try:
            if node.get_role_name() == "spin button" and node.get_name() == want:
                states = sorted(s.value_nick for s in node.get_state_set().get_states())
                found.append({"path": node.get_path() if hasattr(node, "get_path") else None,
                              "states": states})
            for i in range(node.get_child_count()):
                child = node.get_child_at_index(i)
                if child is not None:
                    stack.append(child)
        except Exception as exc:
            found.append({"error": repr(exc)})
    print(json.dumps(found))
""")


def fresh_client_states(name: str) -> list[dict]:
    """The states of spin button `name`, read by a brand-new libatspi client
    (a process that has never received an event, so nothing is cached): what
    the application itself answers to GetState."""
    done = subprocess.run([sys.executable, "-W", "ignore", "-c", _FRESH_CLIENT, name],
                          capture_output=True, text=True, timeout=60)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return [{"error": done.stderr.strip()[-500:] or "no output"}]


def said_anything():
    def run(act):
        heard = utterances(act.orca)
        return bool(heard), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                             for u in heard] or ["Orca said nothing in the act"]
    return custom("Orca says something", run, needs_orca=True)


# ---------------------------------------------------------------------------
# Scroll out and back: whose state is `defunct`?
# ---------------------------------------------------------------------------


def return_state(run):
    setup_focus(run, "Font size")
    tab_until(run, "spin button", "Opacity (fill)")
    tab_until(run, "spin button", "Font size", chord="Shift+Tab")
    result: dict = {}
    with run.act("a fresh AT-SPI client reads the returned Font size's states",
                 [custom("a fresh libatspi client (no cache) does not read 'defunct'",
                         lambda act: (bool(result.get("fresh"))
                                      and all("defunct" not in n.get("states", ["defunct"])
                                              for n in result["fresh"]),
                                      [f"fresh client: {result.get('fresh')}"]),),
                  spin_state("Font size", "defunct", present=True)],
                 should="record: AccessKit's own answer vs the listener's cached state",
                 tree=True, record=0.5):
        result["fresh"] = fresh_client_states("Font size")
        run._step(f"fresh client read {result['fresh']}")
        run.note(f"fresh libatspi client reads Font size as {result['fresh']}")
    with run.act("Up on the returned Font size", [said("13"), spin_value("Font size", 13, "13"),
                                                  said_anything()],
                 should="the reader hears 13, from Font size"):
        run.key("Up")
    with run.act("Down on the returned Font size", [said("12"), said_anything()],
                 should="the reader hears 12"):
        run.key("Down")


# ---------------------------------------------------------------------------
# Commits that end on the value already held
# ---------------------------------------------------------------------------


def silent_clamp(run):
    setup_focus(run, "Font size")
    with run.act("setup: PageUp x10 to the maximum 96", [spin_value("Font size", 96, "96")]):
        run.key(*["PageUp"] * 10, gap=0.25)
    with run.act("at 96: select all, type 200, Enter (clamps to the 96 already held)",
                 [spin_value("Font size", 96, "96"), said_anything()],
                 should="the reader learns the entry was clamped (or at least hears 96)"):
        run.key("Ctrl+A")
        run.type("200")
        run.key("Enter")
    with run.act("setup: PageDown x10 to the minimum 4", [spin_value("Font size", 4, "4")]):
        run.key(*["PageDown"] * 10, gap=0.25)
    with run.act("at 4: select all, type 1, Enter (clamps to the 4 already held)",
                 [spin_value("Font size", 4, "4"), said_anything()],
                 should="the reader learns the entry was clamped (or at least hears 4)"):
        run.key("Ctrl+A")
        run.type("1")
        run.key("Enter")
    setup_focus(run, "Gain")
    with run.act("Gain: select all, type abc, Enter (reverts to 0.0)",
                 [spin_value("Gain", 0.0, "0.0"), said("0.0")],
                 should="the reader learns the entry was refused"):
        run.key("Ctrl+A")
        run.type("abc")
        run.key("Enter")


# ---------------------------------------------------------------------------
# French: a box edited while focused, then left
# ---------------------------------------------------------------------------


def locale_blur(run):
    setup_focus(run, "Language", role="combo box")
    with run.act("Language: Down, Enter picks français",
                 [spin_value("Gain", 0.0, "0,0")], tree=True):
        run.key("Down")
        run.wait(1.0)
        run.key("Enter")
    setup_focus(run, "Gain")
    with run.act("Gain: Down in French, then Tab away",
                 [spin_value("Gain", -0.5, "-0,5"), focused(role="spin button", name="Opacity")],
                 should="left unfocused, Gain shows the French form again, as every "
                        "untouched box does", tree=True):
        run.key("Down")
        run.wait(0.8)
        run.key("Tab")
    setup_focus(run, "Frequency")
    with run.act("Frequency: select all, type 12.5 (keypad-style point), Enter",
                 [spin_value("Frequency", 12.5, "12,50")],
                 should="the point still reads as the decimal separator; the box shows "
                        "12,50 in French", tree=True):
        run.key("Ctrl+A")
        run.type("12.5")
        run.key("Enter")


def blur_clamp(run):
    """Commit on blur that clamps: the value change lands in the same tree
    update as the focus move, so is the clamped value heard?"""
    setup_focus(run, "Font size")
    for i in (1, 2, 3):
        with run.act(f"select all, type 500, Tab away (clamps to 96 on blur) ({i})",
                     [spin_value("Font size", 96, "96"), focused(role="spin button", name="Gain"),
                      said("96")],
                     should="the reader learns the entry became 96, then hears Gain"):
            run.key("Ctrl+A")
            run.type("500")
            run.key("Tab")
        with run.act(f"setup: Shift+Tab back, PageDown to 86 ({i})",
                     [focused(role="spin button", name="Font size")]):
            run.key("Shift+Tab")
            run.wait(0.8)
            run.key("PageDown")


SCENARIOS = [
    Scenario("verify-spinbox-blur-clamp", PACKAGE, blur_clamp,
             "type 500 and Tab away: is the clamp to 96 heard before the next box"),
    Scenario("verify-spinbox-return-state", PACKAGE, return_state,
             "a box that scrolled out and back: fresh client's states vs cached defunct"),
    Scenario("verify-spinbox-silent-clamp", PACKAGE, silent_clamp,
             "commits that clamp or revert to the value already held"),
    Scenario("verify-spinbox-locale-blur", PACKAGE, locale_blur,
             "French: leave a box edited while focused; type a point in Frequency"),
]
