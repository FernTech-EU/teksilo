# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's scenarios for terminal-demo, log_view and code_editor, beside the
sweep's `console.py`.

* verify-console-editor-atfocus / verify-console-log-atfocus: a screen
  reader's own focus request (AT-SPI `grab_focus`) on the node that holds the
  text. Where does focus land, and does the reader then get caret events?
  Tells whether the mis-aimed focus of `console.py`'s focus scenarios is
  confined to Tab, or is where the framework puts the editor's focus whatever
  asks for it.
* verify-console-terminal-scrollback: Shift+PageUp / Shift+PageDown on a
  terminal with scrollback, and `clear`, which change every visible row at
  once.
* verify-console-terminal-ls: several short output lines in one drain.
"""

from __future__ import annotations

import time

from reader_lib.checks import custom, event, focused
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario
from scenarios.console import TERM, announcements, said_any, tab_to


def _speech_summary(act):
    said = [u.text for u in utterances(act.orca)]
    chars = sum(len(u) for u in said)
    return said, chars


def said_at_most(limit: int):
    def run(act):
        said, chars = _speech_summary(act)
        return chars <= limit, [f"{len(said)} utterances, {chars} characters",
                                *[f"Orca said {u!r}"[:300] for u in said[:8]],
                                f"announcements {announcements(act)!r}"[:300]]
    return custom(f"Orca says at most {limit} characters", run, needs_orca=True)


def said_something():
    def run(act):
        said = said_any(act)
        return bool(said), [f"Orca said {u!r}"[:300] for u in said] or ["Orca said nothing"]
    return custom("Orca says something", run, needs_orca=True)


# ---------------------------------------------------------------------------
# A reader's own focus request on the text node
# ---------------------------------------------------------------------------


def editor_atfocus(run):
    run.wait_for(role="entry")
    with run.act("AT-SPI grab_focus on the [entry]",
                 [focused(role="entry"), said_something()],
                 should="focus lands on the entry; the reader hears it", tree=True):
        run.grab_focus(role="entry")
    with run.act("Down after the AT focus request",
                 [event("object:text-caret-moved", role="entry")],
                 should="the caret moves and the bus says so"):
        run.key("Down")


def log_atfocus(run):
    run.wait_for(role="document frame")
    with run.act("AT-SPI grab_focus on the [document frame]",
                 [focused(role="document frame"), said_something()],
                 should="focus lands on the log's document; the reader hears it", tree=True):
        run.grab_focus(role="document frame")


# ---------------------------------------------------------------------------
# Terminal
# ---------------------------------------------------------------------------


def terminal_scrollback(run):
    run.wait_for(**TERM)
    with run.act("scene: Tab to the terminal", should="scene setting, not judged"):
        tab_to(run, "terminal", "Demo shell")
    with run.act("scene: 'seq 1 100' fills the screen and the scrollback",
                 should="scene setting", record=4.0):
        run.type("seq 1 100")
        run.key("Enter")
    with run.act("Shift+PageUp (scroll back one page)", [said_at_most(200)],
                 should="the view scrolls back; a reader is not read the whole page as "
                        "if it were new output", record=4.0, tree=True):
        run.key("Shift+Page_Up")
    with run.act("Shift+PageDown (back to the bottom)", [said_at_most(200)],
                 should="the view returns; again not read as new output", record=4.0):
        run.key("Shift+Page_Down")
    with run.act("'clear' and Enter", [said_at_most(120)],
                 should="the screen clears; the reader hears the new prompt, not the "
                        "cleared screen", record=4.0, tree=True):
        run.type("clear")
        run.key("Enter")


def terminal_ls(run):
    run.wait_for(**TERM)
    with run.act("scene: Tab to the terminal", should="scene setting, not judged"):
        tab_to(run, "terminal", "Demo shell")
    with run.act("printf three lines",
                 [custom("each of 'alpha', 'beta', 'gamma' reaches the bus as its own "
                         "announcement or is spoken apart from its neighbours",
                         lambda act: (
                             all(any(w == u.strip() or f" {w} " in f" {u} "
                                     for u in said_any(act) + announcements(act))
                                 for w in ("alpha", "beta", "gamma")),
                             [f"Orca said {u!r}" for u in said_any(act)]
                             + [f"announcement {a!r}" for a in announcements(act)]),
                         needs_orca=True)],
                 should="three output lines, heard as three lines", record=3.0):
        run.type("printf 'alpha\\nbeta\\ngamma\\n'")
        run.key("Enter")
    time.sleep(0.2)


def terminal_scrolled_output(run):
    """Shift+PageUp emits nothing (verify-console-terminal-scrollback). Is that
    because the view did not scroll, or because the scroll never reaches the
    accessibility tree? Scroll back while a command sleeps, then let its output
    arrive: the drain re-walks the tree, and the text it publishes is whatever
    page the view shows."""
    run.wait_for(**TERM)
    with run.act("scene: Tab to the terminal", should="scene setting, not judged"):
        tab_to(run, "terminal", "Demo shell")
    with run.act("scene: 'seq 1 100' fills the screen and the scrollback",
                 should="scene setting", record=4.0):
        run.type("seq 1 100")
        run.key("Enter")
    probe: dict = {}
    with run.act("start 'sleep 8; echo LATE', then Shift+PageUp at once",
                 should="the view scrolls back; the reader's text follows the page shown",
                 record=0.8, tree=True):
        run.type("sleep 8; echo LATE")
        run.key("Enter")
        time.sleep(0.3)
        run.key("Shift+Page_Up")
    with run.act("the tree 1 s after Shift+PageUp (fresh AT-SPI client)",
                 should="the terminal's text is the page the view shows", record=0.2):
        from scenarios.console import line_probe
        probe["before"] = line_probe(run, "terminal")
        run.note(f"after Shift+PageUp: {probe['before'].get('text', '')[:160]!r}")
    with run.act("LATE arrives while scrolled back",
                 should="the reader hears 'LATE' (or nothing, the view being scrolled "
                        "away), not a re-read of the page scrolled to",
                 record=4.5, tree=True):
        run.wait(4.0)
    with run.act("the tree after LATE (fresh AT-SPI client)",
                 should="the terminal's text is the page the view shows", record=0.2):
        from scenarios.console import line_probe
        probe["after"] = line_probe(run, "terminal")
        run.note(f"after LATE: {probe['after'].get('text', '')[:160]!r}")


def log_stream_focused(run):
    """The log's document really focused (the AT focus request reaches it), then
    streamed into: what does a reader sitting on the log hear?"""
    run.wait_for(role="document frame")
    with run.act("scene: AT-SPI grab_focus on the [document frame]",
                 should="scene setting, not judged"):
        run.grab_focus(role="document frame")
    with run.act("Start / Stop through AT-SPI, focus left on the document",
                 [said_at_most(300),
                  custom("focus stays on the document",
                         lambda act: (not any(e["type"] == "object:state-changed:focused"
                                              and e.get("detail1") == 1
                                              for e in act.events),
                                      [f"{e['type']} {e.get('detail1')} "
                                       f"[{e['source'].get('role')}] {e['source'].get('name')!r}"
                                       for e in act.events
                                       if e["type"] == "object:state-changed:focused"]
                                      or ["no focus change"]))],
                 should="streaming starts; a reader on the log is not flooded",
                 record=4.0):
        run.action("click", role="push button", name="Start / Stop")
    with run.act("Start / Stop again", should="streaming stops", record=2.0):
        run.action("click", role="push button", name="Start / Stop")


SCENARIOS = [
    Scenario("verify-console-log-stream-focused", "log_view", log_stream_focused,
             "stream into the log with the reader really on its document"),
    Scenario("verify-console-terminal-scrolled-output", "terminal-demo",
             terminal_scrolled_output,
             "Shift+PageUp while a command sleeps; what reaches the reader, and when"),
    Scenario("verify-console-editor-atfocus", "code_editor", editor_atfocus,
             "AT-SPI grab_focus on the code editor's entry"),
    Scenario("verify-console-log-atfocus", "log_view", log_atfocus,
             "AT-SPI grab_focus on the log's document"),
    Scenario("verify-console-terminal-scrollback", "terminal-demo", terminal_scrollback,
             "Shift+PageUp/Down and clear in the terminal"),
    Scenario("verify-console-terminal-ls", "terminal-demo", terminal_ls,
             "three output lines in one drain"),
]
