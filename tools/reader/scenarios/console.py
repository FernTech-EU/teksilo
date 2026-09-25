# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The console family: `terminal-demo`, `log_view`, `code_editor`.

What each surface hands a reader, and where it is made:

* **Terminal** (`crates/teksilo-terminal/src/a11y.rs`,
  `build_terminal_a11y`): a `Role::Terminal` node named by `.label(..)`, one
  `Role::TextRun` per visible row hung straight off it, the VT cursor as the
  AT caret, and a `Ctrl+Tab` keyboard shortcut. New output goes through a
  zero-size `Role::Status` / `Live::Polite` child (`LiveAnnouncer`) whose
  *name* is set to "the row the cursor just left" whenever the cursor's row
  changes (`terminal.rs`, `apply_drain`). The drain runs in `paint()`.
* **LogView** (`crates/teksilo-widgets/src/code_editor/log_view.rs`, the walk in
  `code_editor/a11y.rs::build_log_a11y`): a read-only `Role::Document` with
  runs for the visible window only. No live region unless the app opts in
  with `announce_appends(true)`, which the demo does not.
* **CodeEditor** (`crates/teksilo-widgets/src/code_editor.rs`,
  `accessibility`): `Role::MultilineTextInput` with runs for every block, the
  gutter `set_hidden()` (`code_editor/gutter.rs`), and the completion popup
  as the combobox-with-listbox pattern: the editor keeps focus and points
  `active_descendant` at the highlighted `Role::ListBoxOption`
  (`code_editor/completion.rs`).

None of the three examples routes a message through the framework
announcer (`ctx.announce`), so K2 does not reach them; K1 (the unnamed
window) does, at every launch.

What the runs found (September 2026, main 261a218f): the CodeEditor and the
LogView give focus to a wrapper the adapter reports as [unknown] '' while the
text lives in a child, so Orca says nothing and no caret event is ever sent
(`console-editor-focus`, `console-log-focus`); Tab, Ctrl+Tab and Escape+Tab
all indent inside the editor (`console-editor-trap`); the terminal's rows reach
the bus glued together with no separator, a full screen is re-read whole for
every new line, and its live region announces the line the cursor left, which
is usually the command the user typed (`console-terminal-*`).

Focus is moved with real keys wherever Tab reaches; scene setting runs in an
act of its own so that Orca's speech for it is not credited to the next act.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import time

from reader_lib.checks import (_focus_node, _is_focus, _walk, custom, event, focused,
                               no_event, not_said, said)
from reader_lib.orca import utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def tab_to(run, role: str | tuple[str, ...], name: str | None = None, limit: int = 12,
           chord: str = "Tab") -> dict:
    """Scene setting: press `chord` until the bus reports focus on [role], or
    on any of the roles given."""
    roles = (role,) if isinstance(role, str) else role
    for _ in range(limit):
        node = run.last_focus()
        if node.get("role") in roles and (name is None or node.get("name") == name):
            return node
        run.key(chord)
        time.sleep(0.4)
    node = run.last_focus()
    if node.get("role") in roles and (name is None or node.get("name") == name):
        return node
    raise RunError(f"{limit} x {chord} never focused [{role}] {name!r}; last focus {node}")


def announcements(act) -> list[str]:
    return [e.get("text") or "" for e in act.events if e["type"] == "object:announcement"]


def said_any(act) -> list[str]:
    return [u.text for u in utterances(act.orca)]


def announced_line(line: str):
    """An `object:announcement` whose text is exactly `line`: the live region's
    contract is one newly completed output line (`a11y.rs`, `LiveAnnouncer`)."""
    def run(act):
        every = announcements(act)
        return any(t.strip() == line for t in every), \
            [f"announcement {t!r}" for t in every] or ["no object:announcement in the act"]
    return custom(f"the bus announces the line {line!r} on its own", run)


def heard_line(line: str, command: str):
    """Orca said `line` as a word of its own, in an utterance that is not the
    echo of the command the user typed (`command`)."""
    def run(act):
        said = said_any(act)
        ok = [u for u in said if line in re.split(r"\s+", u.strip()) and command not in u]
        return bool(ok), [f"Orca said {u!r}" for u in said] or ["Orca said nothing"]
    return custom(f"Orca says {line!r} as a word of its own", run, needs_orca=True)


_LINE_PROBE = r"""
import json, sys, gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
pid, role = int(sys.argv[1]), sys.argv[2]
desktop = Atspi.get_desktop(0)
app = None
for i in range(desktop.get_child_count()):
    c = desktop.get_child_at_index(i)
    try:
        if c is not None and c.get_process_id() == pid:
            app = c
    except Exception:
        pass
out = {"found": False}
stack = [app] if app else []
while stack:
    n = stack.pop()
    try:
        if n.get_role_name() == role:
            cnt = Atspi.Text.get_character_count(n)
            text = Atspi.Text.get_text(n, 0, cnt)
            lines, off = [], 0
            while off < cnt and len(lines) < 80:
                r = Atspi.Text.get_string_at_offset(n, off, Atspi.TextGranularity.LINE)
                lines.append([r.start_offset, r.end_offset, r.content])
                off = max(r.end_offset, off + 1)
            out = {"found": True, "text": text, "caret": Atspi.Text.get_caret_offset(n),
                   "lines": lines}
            break
        for i in reversed(range(n.get_child_count())):
            ch = n.get_child_at_index(i)
            if ch is not None:
                stack.append(ch)
    except Exception as exc:
        out = {"found": False, "error": repr(exc)}
        continue
print(json.dumps(out, ensure_ascii=False))
"""


def line_probe(run, role: str) -> dict:
    """Ask a fresh libatspi client for the text of the first [role] node, line
    by line (`get_string_at_offset(LINE)`), as a reader's line review does."""
    assert run.app is not None
    done = subprocess.run([sys.executable, "-W", "ignore", "-c", _LINE_PROBE,
                           str(run.app.pid), role], capture_output=True, text=True,
                          timeout=60)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {"found": False, "error": done.stderr[-400:]}


def node_in(tree, role=None, name=None):
    for n in _walk(tree):
        if (role is None or n.get("role") == role) and (name is None or n.get("name") == name):
            return n
    return None


# ---------------------------------------------------------------------------
# terminal-demo
# ---------------------------------------------------------------------------

TERM = {"role": "terminal", "name": "Demo shell"}


def terminal_focus(run) -> None:
    run.wait_for(**TERM)
    with run.act("scene: Tab to the terminal", should="scene setting, not judged"):
        tab_to(run, "terminal", "Demo shell")


def terminal_echo(run):
    terminal_focus(run)
    with run.act("type 'echo hello'",
                 should="the typed characters appear at the caret; the reader can review "
                        "them (Orca does not hear the keys here, so no key echo)"):
        run.type("echo hello")
    with run.act("Enter",
                 [announced_line("hello"), heard_line("hello", "echo hello")],
                 should="the command runs and the reader hears its output, 'hello'",
                 record=3.0, tree=True):
        run.key("Enter")
    with run.act("type 'echo hello' again and Enter",
                 [announced_line("hello"), heard_line("hello", "echo hello")],
                 should="the same output again is heard again",
                 record=3.0):
        run.type("echo hello")
        run.key("Enter")
    probe = {}

    def lines_check(act):
        rec = probe.get("rec") or {}
        text = rec.get("text", "")
        return ("hello\n" in text or "hello\r" in text), [
            f"text {text!r}", f"caret {rec.get('caret')}",
            *[f"line {a}-{b} {c!r}" for a, b, c in rec.get("lines", [])[:8]]]
    with run.act("read the terminal's text and lines (fresh AT-SPI client)",
                 [custom("the terminal's text separates its rows ('hello' ends a line)",
                         lines_check)],
                 should="a reader reviewing the screen gets one line per row"):
        probe["rec"] = line_probe(run, "terminal")
        run.note(f"terminal line probe: {probe['rec']}")
    with run.act("type 'echo one; echo two' and Enter",
                 [announced_line("one"), announced_line("two"),
                  heard_line("one", "echo one"), heard_line("two", "echo two")],
                 should="both lines of output are heard", record=3.0):
        run.type("echo one; echo two")
        run.key("Enter")


def terminal_late(run):
    """Output that arrives after the command line was left: the one shape the
    live region is built for."""
    terminal_focus(run)
    with run.act("type 'sleep 1; echo late' and Enter",
                 [announced_line("late"), heard_line("late", "echo late")],
                 should="a second later the reader hears 'late'", record=4.0):
        run.type("sleep 1; echo late")
        run.key("Enter")


def terminal_scroll(run):
    """Once the screen is full the cursor stays on the last row, and the grid
    scrolls under it."""
    terminal_focus(run)
    with run.act("scene: fill the screen with 'seq 1 60'", should="scene setting",
                 record=4.0):
        run.type("seq 1 60")
        run.key("Enter")
    with run.act("type 'echo hello' and Enter on a full screen",
                 [announced_line("hello"), heard_line("hello", "echo hello")],
                 should="the reader hears 'hello', as on an empty screen", record=4.0,
                 tree=True):
        run.type("echo hello")
        run.key("Enter")
    with run.act("type 'sleep 1; echo late' and Enter on a full screen",
                 [announced_line("late"), heard_line("late", "echo late")],
                 should="a second later the reader hears 'late'", record=4.0):
        run.type("sleep 1; echo late")
        run.key("Enter")


PACED = "for i in 1 2 3 4 5 6; do echo line$i; sleep 0.4; done"


def paced_check(limit_chars: int):
    """Each 'lineN' heard, and the output part of the speech (what Orca said
    after the command line was echoed) within `limit_chars` characters."""
    def run(act):
        said = said_any(act)
        # Orca's echo of the typed command is one utterance a key; count only
        # what it said after the last typed character.
        try:
            start = max(i for i, u in enumerate(said) if len(u.strip()) <= 2) + 1
        except ValueError:
            start = 0
        out = said[start:]
        chars = sum(len(u) for u in out)
        heard = [f"line{i}" for i in range(1, 7)
                 if any(f"line{i}" in re.split(r"\s+|(?<=\d)(?=line)|(?<=\d)(?=cyril)", u)
                        for u in out)]
        ok = chars <= limit_chars and len(heard) == 6
        return ok, [f"after the command: {len(out)} utterances, {chars} characters",
                    f"heard as words: {heard}",
                    f"announcements: {announcements(act)!r}"[:400],
                    *[f"Orca said {u!r}"[:300] for u in out[:10]]]
    return custom(f"each line heard, output speech at most {limit_chars} characters", run,
                  needs_orca=True)


def terminal_flood(run):
    """A command that prints a line every 0.4 s, on an empty then a full screen."""
    terminal_focus(run)
    with run.act("paced output on an empty screen",
                 [paced_check(200)],
                 should="each of six lines is heard once, as it arrives", record=4.0):
        run.type(PACED)
        run.key("Enter")
    with run.act("scene: fill the screen with 'seq 1 60'", should="scene setting",
                 record=3.0):
        run.type("seq 1 60")
        run.key("Enter")
    with run.act("paced output on a full screen",
                 [paced_check(400)],
                 should="each of six lines is heard once, as it arrives, as on an empty "
                        "screen", record=4.0):
        run.type(PACED)
        run.key("Enter")


def terminal_escape(run):
    """Tab goes to the child; Ctrl+Tab is the way out (WCAG 2.1.2)."""
    terminal_focus(run)
    with run.act("Tab inside the terminal", [no_event("object:state-changed:focused")],
                 should="Tab is the shell's (completion); focus stays in the terminal"):
        run.key("Tab")
    with run.act("Ctrl+Tab", [focused(role="push button", name="Clear"), said("Clear")],
                 should="focus leaves the terminal for the next control, wrapping to Clear"):
        run.key("Ctrl+Tab")
    with run.act("scene: back to the terminal", should="scene setting, not judged"):
        tab_to(run, "terminal", "Demo shell")
    with run.act("Ctrl+Shift+Tab",
                 [focused(role="push button", name="Scroll to bottom"),
                  said("Scroll to bottom")],
                 should="focus leaves the terminal backwards"):
        run.key("Ctrl+Shift+Tab")


def terminal_exit(run):
    """The shell exits: the demo's status line turns to '○ exited'."""
    terminal_focus(run)
    with run.act("type 'exit' and Enter",
                 [said("exited")],
                 should="the shell ends and the reader hears that it did", record=4.0,
                 tree=True):
        run.type("exit")
        run.key("Enter")
    with run.act("type 'ls' into the dead terminal",
                 should="the reader can tell the terminal no longer runs anything",
                 record=2.5, tree=True):
        run.type("ls")


def terminal_shortcut_hint(run):
    """The terminal says Ctrl+Tab is its way out
    (`set_keyboard_shortcut("Ctrl+Tab")`): is that anywhere a reader can get it?"""
    node = run.wait_for(**TERM)

    def check(act):
        n = node_in(act.tree, "terminal", "Demo shell") or {}
        evidence = [f"terminal node: description={n.get('description')!r} "
                    f"attributes={n.get('attributes')!r} actions={n.get('actions')!r} "
                    f"interfaces={n.get('interfaces')!r}"]
        blob = repr(n)
        return ("Ctrl+Tab" in blob or "Control+Tab" in blob), evidence
    with run.act("read the terminal node", [custom("the terminal node carries 'Ctrl+Tab' "
                                                   "somewhere a reader can read it", check,
                                                   needs_tree=True)],
                 should="a reader can find out how to leave the terminal", tree=True):
        pass


def focus_reaches_text(role: str):
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change on the bus in this act"]
        last = _focus_node(moves[-1])
        return last.get("role") == role, [
            f"{e['type']} -> [{_focus_node(e).get('role')}] {_focus_node(e).get('name')!r} "
            f"{_focus_node(e).get('path')}" for e in moves]
    return custom(f"focus lands on the [{role}] that holds the text", run)


def said_something():
    def run(act):
        said = said_any(act)
        return bool(said), [f"Orca said {u!r}" for u in said] or ["Orca said nothing"]
    return custom("Orca says something when focus arrives", run, needs_orca=True)


def log_focus_judged(run):
    with run.act("scene: Tab to the Theme combo box", should="scene setting, not judged"):
        tab_to(run, "combo box", "Theme", limit=10)
    with run.act("Tab to the log",
                 [focus_reaches_text("document frame"), said_something(),
                  said("[00:00:00")],
                 should="focus lands on the log; the reader hears what it is and the line "
                        "at its caret", tree=True):
        run.key("Tab")
    with run.act("Down in the log", [event("object:text-caret-moved")],
                 should="the log scrolls or the caret moves; the reader's caret tracking "
                        "follows"):
        run.key("Down")


def editor_focus_judged(run):
    with run.act("scene: Tab to the Theme combo box", should="scene setting, not judged"):
        tab_to(run, "combo box", "Theme", limit=10)
    with run.act("Tab to the editor",
                 [focus_reaches_text("entry"), said_something(),
                  said("A little Teksilo widget")],
                 should="focus lands on the editor; the reader hears a name, 'edit' / "
                        "'multi-line' and the line at the caret", tree=True):
        run.key("Tab")
    with run.act("Down in the editor",
                 [event("object:text-caret-moved", role="entry")],
                 should="the caret moves to line 2 and the bus says so, so a reader's caret "
                        "tracking (speech, braille) follows"):
        run.key("Down")
    with run.act("type 'Z'",
                 [event("object:text-changed:insert", role="entry"),
                  event("object:text-caret-moved", role="entry")],
                 should="the character is inserted at the caret and the caret moves on"):
        run.type("Z")


# ---------------------------------------------------------------------------
# log_view
# ---------------------------------------------------------------------------

LOG_DOC = {"role": "document frame"}


def log_focus(run) -> dict:
    with run.act("scene: Tab to the log", should="scene setting, not judged"):
        # Focus lands on the LogView's wrapper, which the adapter reports as
        # [unknown] '' (see console-log-focus); the document is its child.
        # Since the editor-focus fix it lands on the document itself.
        node = tab_to(run, ("unknown", "document frame"), None, limit=14)
    return node


def log_stream(run):
    """The producer streams 40 lines a frame into a tail-following log."""
    log_focus(run)
    with run.act("Start / Stop through AT-SPI, focus left on the log",
                 [said("streaming")],
                 should="the log starts streaming; a reader focused on it is not flooded, "
                        "and the reader gets some sign that it is streaming",
                 record=4.0, tree=True):
        run.action("click", role="push button", name="Start / Stop")
    with run.act("Start / Stop again", should="streaming stops", record=2.0, tree=True):
        run.action("click", role="push button", name="Start / Stop")


def _cpu_seconds(pid: int) -> float:
    try:
        fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return (int(fields[11]) + int(fields[12])) / 100.0
    except (OSError, IndexError, ValueError):
        return -1.0


def _threads(pid: int) -> str:
    """The busiest threads of `pid`, by CPU seconds, with their names."""
    import os
    rows = []
    try:
        for tid in os.listdir(f"/proc/{pid}/task"):
            try:
                comm = open(f"/proc/{pid}/task/{tid}/comm").read().strip()
                fields = open(f"/proc/{pid}/task/{tid}/stat").read().rsplit(")", 1)[1].split()
                rows.append(((int(fields[11]) + int(fields[12])) / 100.0, tid, comm))
            except (OSError, IndexError, ValueError):
                continue
    except OSError:
        return "?"
    rows.sort(reverse=True)
    return ", ".join(f"{comm}[{tid}]={cpu:.1f}s" for cpu, tid, comm in rows[:4])


def log_burst(run):
    """Burst 10k appends 10 000 lines in one handler. In the debug build the
    application then stops answering for a long time; measure how long, from
    the status label the burst updates and the process's CPU time."""
    log_focus(run)
    with run.act("Burst 10k through AT-SPI, focus left on the log",
                 should="10 000 lines arrive at once; the reader is told, not flooded",
                 record=1.0):
        run.action("click", role="push button", name="Burst 10k")
        start = time.monotonic()
        seen = None
        while time.monotonic() - start < 240:
            label = run.find(role="label", name_contains="generated") or {}
            cpu = _cpu_seconds(run.app.pid)
            if "10030" in (label.get("name") or ""):
                seen = time.monotonic() - start
                run.note(f"burst: the status label read {label.get('name')!r} "
                         f"{seen:.1f}s after the click (app CPU {cpu:.1f}s)")
                break
            time.sleep(5)
            run.note(f"burst +{time.monotonic() - start:.0f}s: label "
                     f"{label.get('name')!r}, app CPU {cpu:.1f}s; threads "
                     f"{_threads(run.app.pid)}")
        if seen is None:
            run.note("burst: the status label never changed in 240 s")
    with run.act("after the burst: Tab", [event("object:state-changed:focused")],
                 should="the application answers keys again", record=3.0, tree=True):
        run.key("Tab")


def log_buttons(run):
    """The same controls pressed with real keys, focus on the button."""
    with run.act("scene: Tab to Start / Stop", should="scene setting, not judged"):
        tab_to(run, "push button", "Start / Stop")
    with run.act("Space on Start / Stop", [said("streaming")],
                 should="streaming starts, and the reader hears "
                 "that it did (the status line changes to 'streaming')", record=3.0):
        run.key("space")
    with run.act("Space on Start / Stop again", [said("paused")],
                 should="streaming stops, and the reader "
                 "hears that it did", record=2.0):
        run.key("space")


# ---------------------------------------------------------------------------
# code_editor
# ---------------------------------------------------------------------------

#: The roles focus lands on when the editor takes it: the CodeEditor's wrapper,
#: which the adapter reports as [unknown] '' (the [entry] is its child), and,
#: since the editor-focus fix, the [entry] itself.
EDITOR_ROLE = ("unknown", "entry")


def editor_focus(run) -> dict:
    with run.act("scene: Tab to the editor", should="scene setting, not judged"):
        node = tab_to(run, EDITOR_ROLE, None, limit=14)
    return node


def editor_trap(run):
    """Tab indents in a code editor. Is there a way out by keyboard?"""
    editor_focus(run)
    with run.act("Ctrl+Tab in the editor",
                 [custom("focus leaves the editor", lambda act: (
                     any(_is_focus(e) and _focus_node(e).get("role") not in EDITOR_ROLE
                         for e in act.events),
                     [f"{e['type']} {e.get('source', {}).get('role')} "
                      f"{e.get('text', '')!r}" for e in act.events
                      if e["type"] != "object:bounds-changed"][:12])),
                  no_event("object:text-changed")],
                 should="Ctrl+Tab leaves the editor (as it leaves the terminal and a "
                        "rich-text table) and writes nothing into the document"):
        run.key("Ctrl+Tab")
    with run.act("Escape then Tab",
                 [custom("focus leaves the editor", lambda act: (
                     any(_is_focus(e) and _focus_node(e).get("role") not in EDITOR_ROLE
                         for e in act.events),
                     [f"{e['type']} {e.get('source', {}).get('role')} "
                      f"{e.get('text', '')!r}" for e in act.events
                      if e["type"] != "object:bounds-changed"][:12]))],
                 should="some keyboard route leaves the editor"):
        run.key("Escape", "Tab")
    with run.act("Ctrl+Shift+Tab",
                 [custom("focus leaves the editor", lambda act: (
                     any(_is_focus(e) and _focus_node(e).get("role") not in EDITOR_ROLE
                         for e in act.events),
                     [f"{e['type']} {e.get('source', {}).get('role')} "
                      f"{e.get('text', '')!r}" for e in act.events
                      if e["type"] != "object:bounds-changed"][:12])),
                  no_event("object:text-changed")],
                 should="Ctrl+Shift+Tab leaves the editor backwards and writes nothing"):
        run.key("Ctrl+Shift+Tab")


def completion_checks(expect_open: bool):
    def run(act):
        exp = [e for e in act.events if e["type"] == "object:state-changed:expanded"]
        ev = [f"{e['type']} {e.get('detail1')} [{e['source'].get('role')}]" for e in exp]
        if expect_open:
            return any(e.get("detail1") == 1 for e in exp), ev or ["no expanded change"]
        return any(e.get("detail1") == 0 for e in exp), ev or ["no expanded change"]
    return custom(f"the editor reports expanded={int(expect_open)}", run)


def editor_completion(run):
    editor_focus(run)
    with run.act("scene: Ctrl+End, Enter (a fresh line at the end)",
                 should="scene setting, not judged"):
        run.key("Ctrl+End", "Enter")
    with run.act("type 'wh'",
                 [completion_checks(True),
                  event("object:active-descendant-changed"),
                  said("while")],
                 should="the completion list opens: the reader hears that suggestions are "
                        "available and which one is highlighted ('while', the first)",
                 record=3.0, tree=True):
        run.type("wh")
    with run.act("Down",
                 [event("object:active-descendant-changed"), said("where")],
                 should="the next suggestion is highlighted and the reader hears it",
                 record=2.5, tree=True):
        run.key("Down")
    with run.act("Enter accepts",
                 [completion_checks(False), said("where")],
                 should="the highlighted word replaces 'wh'; the list closes; the reader "
                        "hears what was inserted", record=2.5, tree=True):
        run.key("Enter")
    with run.act("scene: Enter, a fresh line", should="scene setting, not judged"):
        run.key("Enter")
    with run.act("Ctrl+Space on an empty line",
                 [completion_checks(True), event("object:active-descendant-changed")],
                 should="the whole list opens and the reader hears the first entry and "
                        "how many there are", record=3.0, tree=True):
        run.key("Ctrl+space")
    with run.act("Escape",
                 [completion_checks(False)],
                 should="the list closes and the reader hears it closed", record=2.0):
        run.key("Escape")


def editor_multicaret(run):
    editor_focus(run)
    with run.act("scene: Ctrl+Home", should="scene setting, not judged"):
        run.key("Ctrl+Home")
    with run.act("Ctrl+Alt+Down adds a caret below",
                 should="a second caret: the reader is told there are two carets now",
                 record=2.5):
        run.key("Ctrl+Alt+Down")
    with run.act("type 'X' at both carets",
                 [custom("the insertion events carry only what was typed", lambda act: (
                     all((e.get("text") or "") == "X" for e in act.events
                         if e["type"] == "object:text-changed:insert")
                     and any(e["type"] == "object:text-changed:insert" for e in act.events),
                     [f"{e['type']} {e.get('text')!r}" for e in act.events
                      if e["type"].startswith("object:text-changed")]
                     or ["no text change"])),
                  not_said("A little")],
                 should="an X appears at both carets; the reader hears 'X', not the text "
                        "between the carets", record=2.5):
        run.type("X")
    with run.act("Escape back to one caret", should="one caret", record=2.0):
        run.key("Escape")


def editor_brackets(run):
    editor_focus(run)
    with run.act("scene: Ctrl+End, Enter", should="scene setting, not judged"):
        run.key("Ctrl+End", "Enter")
    with run.act("type '('",
                 [said("(")],
                 should="'(' and its auto-closed ')' appear, caret between; the reader "
                        "hears what was inserted", record=2.5, tree=True):
        run.type("(")
    with run.act("Ctrl+/ comments the line",
                 should="the line is commented; the reader hears what changed", record=2.5):
        run.key("Ctrl+slash")
    with run.act("Alt+Up moves the line up",
                 should="the line moves up; the reader hears the moved line", record=2.5):
        run.key("Alt+Up")


SCENARIOS = [
    Scenario("console-terminal-echo", "terminal-demo", terminal_echo,
             "type commands with real keys in the terminal and hear their output"),
    Scenario("console-terminal-late", "terminal-demo", terminal_late,
             "output arriving a second after the command line"),
    Scenario("console-terminal-scroll", "terminal-demo", terminal_scroll,
             "output on a full screen, where the grid scrolls under the cursor"),
    Scenario("console-terminal-flood", "terminal-demo", terminal_flood,
             "a line every 0.4 s, on an empty then a full screen"),
    Scenario("console-terminal-escape", "terminal-demo", terminal_escape,
             "Tab stays in the terminal, Ctrl+Tab and Ctrl+Shift+Tab leave it"),
    Scenario("console-terminal-exit", "terminal-demo", terminal_exit,
             "the shell exits: is the reader told"),
    Scenario("console-terminal-hint", "terminal-demo", terminal_shortcut_hint,
             "is the terminal's Ctrl+Tab way out exposed to a reader"),
    Scenario("console-log-focus", "log_view", log_focus_judged,
             "Tab to the log: where focus lands and what the reader hears"),
    Scenario("console-editor-focus", "code_editor", editor_focus_judged,
             "Tab to the editor, Down, type: where focus lands, what the reader hears"),
    Scenario("console-log-stream", "log_view", log_stream,
             "stream into the log with focus on it, then stop"),
    Scenario("console-log-burst", "log_view", log_burst,
             "10 000 lines at once with focus on the log"),
    Scenario("console-log-buttons", "log_view", log_buttons,
             "Start / Stop with real keys: does the reader hear the state change"),
    Scenario("console-editor-trap", "code_editor", editor_trap,
             "Ctrl+Tab / Escape+Tab / Ctrl+Shift+Tab from inside the editor"),
    Scenario("console-editor-completion", "code_editor", editor_completion,
             "the completion popup: open by typing, arrow, accept, Ctrl+Space, Escape"),
    Scenario("console-editor-multicaret", "code_editor", editor_multicaret,
             "a second caret and typing at both"),
    Scenario("console-editor-brackets", "code_editor", editor_brackets,
             "auto-closed bracket, line comment, move line"),
]
