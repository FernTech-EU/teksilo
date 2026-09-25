# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""fix-key-report: Orca hears the keys typed into a Teksilo window.

Orca 46.1 in a Wayland session reads no keyboard of its own: libatspi gives it
the *legacy* device, which learns of a key only when the application reports
it to the AT-SPI registry (`DeviceEventController.NotifyListenersSync`), as
GTK 3 and Qt do and as `teksilo_platform::key_report` now does. Before that,
Orca's log had no `KEYBOARD_EVENT` line in a whole run and it said nothing
about any caret move (`text-editor-keys-unreported` in `text.py`).

Every key here is a real key, pressed through the private KWin
(`run.key`), and nothing tells Orca of it but the application. `text.py`'s
`tell_orca` is not used anywhere in this module.

Each act checks three things:

* Orca's log holds a `KEYBOARD_EVENT` press and release for the key, with its
  X keysym and keycode, stamped inside the act (`orca_got`).
* What Orca then says about the caret move or the typing, which it can only
  say knowing the key.
* For Orca+T (Orca's "present time"), that Orca says the time and the
  application never sees the T: a key Orca takes is not delivered.

`fix-key-report-text-input` also reads the reports themselves off the
accessibility bus (`dbus-monitor`, in the private session only): each is one
`(uiiiisb)` struct with the key's keysym, keycode and text; keypad + with Num
Lock on carries Mod2, so Orca's keypad "say all" does not take it; and a key
typed into the password field goes without its character (keysym
`VoidSymbol`, no text) but with its keycode.
"""

from __future__ import annotations

import datetime as dt
import re
import subprocess
import threading
from typing import Any

from reader_lib.checks import custom, event, no_event
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

from scenarios.text import (
    EDITOR,
    Facts,
    _a11y_address,
    caret_event,
    fact,
    focus_by_request,
    heard,
    probe,
    said_any,
    said_exactly,
    said_expected,
    scene,
)

#: X keysym and X keycode (evdev + 8, US layout) of the keys pressed here.
KEYS = {
    "Right": (0xff53, 114),
    "Left": (0xff51, 113),
    "Down": (0xff54, 116),
    "Up": (0xff52, 111),
    "Home": (0xff50, 110),
    "BackSpace": (0xff08, 22),
    "Insert": (0xff63, 118),
    "a": (0x61, 38),
    "b": (0x62, 56),
    "t": (0x74, 28),
    "x": (0x78, 53),
    "KP_Add": (0xffab, 86),
}

#: AT-SPI's Num Lock modifier bit, which the registry adds when a report
#: carries X's Mod2, and which Orca logs (`input_event.py`).
ATSPI_NUMLOCK = 1 << 14
VOID_SYMBOL = 0x00ff_ffff

#: One key event as Orca 46.1 logs it (`KeyboardEvent.__str__`,
#: `orca/input_event.py`). `time=` is Orca's wall clock when it built the event.
KEYBOARD_EVENT = re.compile(
    r"KEYBOARD_EVENT:\s+type=ATSPI_KEY_(PRESSED|RELEASED)_EVENT\s+"
    r"id=(\d+)\s+hw_code=(\d+)\s+modifiers=(\d+)\s+"
    r"event_string=\((.*?)\)\s+keyval_name=\((.*?)\)\s+"
    r"timestamp=\d+\s+time=([\d.]+)", re.S)


def keys_orca_got(run: Any, act: Any) -> list[dict]:
    """The key events Orca logged during the act."""
    if run.orca is None:
        return []
    end = act.orca_end or act.end_wall
    found = []
    for match in KEYBOARD_EVENT.finditer(run.orca.text()):
        stamp = dt.datetime.fromtimestamp(float(match.group(7))).strftime("%H:%M:%S.%f")
        if act.start_wall <= stamp <= end:
            found.append({
                "pressed": match.group(1) == "PRESSED",
                "id": int(match.group(2)),
                "hw_code": int(match.group(3)),
                "modifiers": int(match.group(4)),
                "event_string": match.group(5),
                "keyval_name": match.group(6),
                "stamp": stamp,
            })
    return found


def _key_lines(keys: list[dict]) -> list[str]:
    return [f"{k['stamp']} Orca KEYBOARD_EVENT {'press' if k['pressed'] else 'release'} "
            f"id={k['id']:#x} hw_code={k['hw_code']} modifiers={k['modifiers']} "
            f"event_string=({k['event_string']}) keyval_name=({k['keyval_name']})"
            for k in keys] or ["no KEYBOARD_EVENT in Orca's log during this act"]


def orca_got(run: Any, name: str, control: bool = False) -> Any:
    """Orca's log holds a press and a release of the key `name` (Control held
    or not) during the act."""
    keysym, hw_code = KEYS[name]
    want_mods = 1 << 2 if control else 0

    def check(act: Any) -> tuple[bool, list[str]]:
        keys = keys_orca_got(run, act)
        mine = [k for k in keys if k["id"] == keysym and k["hw_code"] == hw_code
                and (k["modifiers"] & 0xff) == want_mods]
        ok = any(k["pressed"] for k in mine) and any(not k["pressed"] for k in mine)
        return ok, _key_lines(keys)
    label = f"{'Ctrl+' if control else ''}{name}"
    return custom(f"Orca receives the key {label} (keysym {keysym:#x}, keycode {hw_code}), "
                  f"press and release", check, needs_orca=True)


def echoed(run: Any, text: str) -> Any:
    """Orca echoed the key: an utterance `text` its null speech server logged
    as a key event."""
    def check(act: Any) -> tuple[bool, list[str]]:
        with_echo = heard(run, act, echo=True)
        without = {u.stamp for u in heard(run, act)}
        echoes = [u for u in with_echo if u.stamp not in without]
        ok = any(u.text.strip() == text for u in echoes)
        return ok, [f"{u.stamp} key echo {u.text!r}" for u in echoes] or ["no key echo"]
    return custom(f"Orca echoes the key {text!r}", check, needs_orca=True)


def said_a_time(run: Any) -> Any:
    def check(act: Any) -> tuple[bool, list[str]]:
        said = heard(run, act)
        ok = any(re.search(r"\b\d{1,2}[:.h]\d{2}", u.text) for u in said)
        return ok, [f"{u.stamp} Orca said {u.text!r}" for u in said] or ["Orca said nothing"]
    return custom("Orca says the time", check, needs_orca=True)


def orca_command(run: Any, *codes: int) -> None:
    """Hold the Orca modifier (Insert) and tap the key of evdev `codes`, as a
    user does for an Orca command. `run.key` holds only the usual modifiers."""
    assert run.fake_key is not None
    steps = ["+110"] + [f"={code}" for code in codes] + ["-110"]
    if run.current is not None:
        run.current.steps.append(f"key Insert+{'+'.join(str(c) for c in codes)} (Orca modifier)")
    done = subprocess.run([str(run.fake_key), *steps], capture_output=True, text=True,
                          timeout=30)
    if done.returncode != 0:
        raise RunError(f"fake_key failed for the Orca command: {done.stderr.strip()}")


# ---------------------------------------------------------------------------
# The reports on the accessibility bus
# ---------------------------------------------------------------------------

#: One `NotifyListenersSync` call as `dbus-monitor` prints it.
CALL = re.compile(r"^method call time=([\d.]+) .*member=NotifyListenersSync$")
FIELD_LINE = re.compile(r"^\s+(uint32|int32|string|boolean) (.*)$")


class BusReports:
    """Every key report the application sends the registry, read by
    `dbus-monitor` on the private session's accessibility bus."""

    def __init__(self, run: Any) -> None:
        address = _a11y_address()
        self.lines: list[str] = []
        self.proc = subprocess.Popen(
            ["stdbuf", "-oL", "dbus-monitor", "--address", address,
             "type='method_call',interface='org.a11y.atspi.DeviceEventController',"
             "member='NotifyListenersSync'"],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        run.note(f"reading the key reports on the accessibility bus ({address})")

    def _read(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            self.lines.append(line.rstrip("\n"))

    def close(self) -> None:
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        self.reader.join(timeout=5)

    def calls(self) -> list[dict]:
        """Each call: its wall-clock stamp, whether its one argument is a
        struct, and the struct's seven fields."""
        found: list[dict] = []
        current: dict | None = None
        for line in list(self.lines):
            match = CALL.match(line)
            if match:
                stamp = dt.datetime.fromtimestamp(float(match.group(1)))
                current = {"stamp": stamp.strftime("%H:%M:%S.%f"), "struct": False,
                           "fields": []}
                found.append(current)
                continue
            if current is None:
                continue
            if line.strip() == "struct {":
                current["struct"] = True
            field = FIELD_LINE.match(line)
            if field:
                kind, value = field.groups()
                current["fields"].append(
                    value.strip('"') if kind == "string" else
                    value == "true" if kind == "boolean" else int(value))
        return found

    def during(self, act: Any) -> list[dict]:
        end = act.orca_end or act.end_wall
        return [c for c in self.calls() if act.start_wall <= c["stamp"] <= end]


def _call_line(call: dict) -> str:
    fields = call["fields"]
    if call["struct"] and len(fields) == 7:
        kind, keysym, hw_code, mods, _stamp, text, is_text = fields
        return (f"{call['stamp']} NotifyListenersSync, one (uiiiisb) struct: kind={kind} "
                f"keysym={keysym:#x} hw_code={hw_code} modifiers={mods:#x} "
                f"text={text!r} is_text={is_text}")
    return (f"{call['stamp']} NotifyListenersSync, {'a struct' if call['struct'] else 'no struct'} "
            f"of {len(fields)} fields: {fields}")


def reported_on_bus(bus: BusReports, describe: str, hw_code: int,
                    want: Any) -> Any:
    """The press of the key `hw_code` went to the registry as one `(uiiiisb)`
    struct, and `want(fields)` holds of it."""
    def check(act: Any) -> tuple[bool, list[str]]:
        calls = bus.during(act)
        mine = [c for c in calls if c["struct"] and len(c["fields"]) == 7
                and c["fields"][0] == 0 and c["fields"][2] == hw_code]
        ok = bool(mine) and all(want(c["fields"]) for c in mine)
        return ok, [_call_line(c) for c in calls] or ["no key report on the bus in this act"]
    return custom(describe, check)


def nav(run: Any, facts: Facts, label: str, chord: str, unit: str, should: str,
        extra: list | None = None, spec: dict = EDITOR) -> None:
    """One caret move by a real key: Orca receives the key, the caret moves,
    and Orca says the `unit` the Text interface reports at the new caret."""
    control = chord.startswith("Ctrl+")
    name = chord.removeprefix("Ctrl+")

    def expected() -> str | None:
        text = facts.get_path(label, unit, "text")
        return text.strip() if isinstance(text, str) and unit != "char" else text
    checks = [orca_got(run, name, control), caret_event(),
              said_expected(run, f"Orca reads the {unit} at the new caret", expected)]
    with run.act(label, checks + list(extra or []), should=should):
        run.key(chord)
    facts[label] = probe(run, spec, grans=["char", "word", "line"])
    run.note(f"{label}: caret {facts.get_path(label, 'caret')}, "
             f"char {facts.get_path(label, 'char', 'text')!r}, "
             f"word {facts.get_path(label, 'word', 'text')!r}, "
             f"line {facts.get_path(label, 'line', 'text')!r}")


# ---------------------------------------------------------------------------
# rich-text-editor
# ---------------------------------------------------------------------------


def body_editor(run: Any) -> None:
    """Caret moves, typing and an Orca command in the rich text editor, every
    key real and reported by the application alone."""
    focus_by_request(run)
    scene(run, "Ctrl+Home", lambda: run.key("Ctrl+Home"))
    facts = Facts()
    nav(run, facts, "Right: next character", "Right", "char",
        "Orca hears Right and says the character the caret moved to",
        extra=[said_exactly(run, "i", True)])
    nav(run, facts, "Ctrl+Right: next word", "Ctrl+Right", "word",
        "Orca hears Ctrl+Right and says the word the caret moved to")
    # Where Down lands depends on whether the heading wraps at the window's
    # width, so the line is the one the Text interface reports after it.
    nav(run, facts, "Down: the next line", "Down", "line",
        "Orca hears Down and reads the line the caret moved to")
    nav(run, facts, "Up: back to the first line", "Up", "line",
        "Orca hears Up and reads the first line again",
        extra=[said_any(run, "Orca reads the heading's first line", ["RichTextEditor"])])
    nav(run, facts, "Home: start of the line", "Home", "char",
        "Orca hears Home and says the first character",
        extra=[said_exactly(run, "R", True)])
    with run.act("type x",
                 [orca_got(run, "x"),
                  event("object:text-changed:insert", role="entry", text_contains="x"),
                  echoed(run, "x")],
                 should="the x goes in, and Orca, hearing the key, echoes it"):
        run.key("x")
    with run.act("BackSpace",
                 [orca_got(run, "BackSpace"),
                  event("object:text-changed:delete", role="entry", text_contains="x"),
                  said_exactly(run, "x", True)],
                 should="Orca hears BackSpace and says the character it deleted"):
        run.key("BackSpace")
    facts["before command"] = probe(run, EDITOR, text=[0, 40])
    with run.act("Orca+T (Insert+T): Orca's own command",
                 [orca_got(run, "t"), said_a_time(run),
                  no_event("object:text-changed:insert", role="entry"),
                  fact("the editor's text is as it was: the T never reached it",
                       lambda: (facts.get_path("after command", "text")
                                == facts.get_path("before command", "text"),
                                [f"before {facts.get_path('before command', 'text')!r}",
                                 f"after {facts.get_path('after command', 'text')!r}"]))],
                 should="Orca takes the key and says the time; the application never "
                        "sees the T"):
        orca_command(run, 20)
    facts["after command"] = probe(run, EDITOR, text=[0, 40])


# ---------------------------------------------------------------------------
# ime-playground: a TextInput
# ---------------------------------------------------------------------------

FIELD = {"role": "entry", "state": "focused"}


def body_text_input(run: Any) -> None:
    """Typing into a single-line TextInput, moving back over it, and an Orca
    command there.

    Only what the reader gets from the key itself is judged here. What a
    TextInput tells the reader about its own caret and its first character is
    separate, and wrong for its own reasons (the sweep's text-09 and text-10):
    it puts no caret move on the bus, and an empty field reports no insertion.
    """
    run.wait_for(role="entry", state="single-line")
    bus = BusReports(run)
    try:
        text_input_acts(run, bus)
    finally:
        bus.close()


def keypad(run: Any, *codes: int) -> None:
    """Tap keypad (or other) keys by evdev code: `run.key` knows no keypad
    digits."""
    assert run.fake_key is not None
    if run.current is not None:
        run.current.steps.append(f"key evdev {' '.join(str(c) for c in codes)}")
    done = subprocess.run([str(run.fake_key), *[f"={c}" for c in codes]],
                          capture_output=True, text=True, timeout=30)
    if done.returncode != 0:
        raise RunError(f"fake_key failed for {codes}: {done.stderr.strip()}")


KEY_NUMLOCK, KEY_KP1, KEY_KPPLUS = 69, 79, 78


def text_input_acts(run: Any, bus: BusReports) -> None:
    scene(run, "empty the field", lambda: run.key("Ctrl+A", "BackSpace"))
    with run.act("type a",
                 [orca_got(run, "a"), echoed(run, "a"),
                  reported_on_bus(bus, "the report is one (uiiiisb) struct: a, 0x61, "
                                  "keycode 38, text 'a', is_text", 38,
                                  lambda f: f[1] == 0x61 and f[5] == "a" and f[6] is True)],
                 should="Orca hears the key and echoes it"):
        run.key("a")
    with run.act("type b",
                 [orca_got(run, "b"),
                  event("object:text-changed:insert", role="entry", text_contains="b"),
                  echoed(run, "b")],
                 should="the b goes in, and Orca hears the key and echoes it"):
        run.key("b")
    with run.act("Left", [orca_got(run, "Left")],
                 should="Orca hears Left"):
        run.key("Left")
    with run.act("Home", [orca_got(run, "Home")],
                 should="Orca hears Home"):
        run.key("Home")
    facts = Facts()
    facts["before command"] = probe(run, FIELD, text=[0, 20])
    with run.act("Orca+T (Insert+T) in the field",
                 [orca_got(run, "t"), said_a_time(run),
                  no_event("object:text-changed:insert", role="entry"),
                  fact("the field's text is as it was: the T never reached it",
                       lambda: (facts.get_path("after command", "text")
                                == facts.get_path("before command", "text"),
                                [f"before {facts.get_path('before command', 'text')!r}",
                                 f"after {facts.get_path('after command', 'text')!r}"]))],
                 should="Orca takes the key and says the time; the field never sees the T"):
        orca_command(run, 20)
    facts["after command"] = probe(run, FIELD, text=[0, 20])

    def num_lock_on() -> None:
        """Leave Num Lock on, whatever it was: keypad 1 types 1 only when it
        is on. The field is given a letter first, as an empty TextInput has
        no Text interface to read back (the sweep's text-10)."""
        run.key("Ctrl+A", "BackSpace", "z")
        keypad(run, KEY_KP1)
        if "1" not in (probe(run, FIELD, text=[0, 20]).get("text") or ""):
            keypad(run, KEY_NUMLOCK, KEY_KP1)
        facts["num lock"] = probe(run, FIELD, text=[0, 20])
    scene(run, "Num Lock on, keypad 1 typed", num_lock_on)
    run.note(f"the field with Num Lock on: {facts.get_path('num lock', 'text')!r}")
    with run.act("keypad + with Num Lock on",
                 [orca_got_keypad_with_num_lock(run),
                  reported_on_bus(bus, "the report carries Num Lock (Mod2)", 86,
                                  lambda f: f[1] == 0xffab and f[3] & (1 << 4) != 0),
                  event("object:text-changed:insert", role="entry", text_contains="+")],
                 should="the + goes into the field: with Num Lock on it is a character, "
                        "not Orca's keypad 'say all'"):
        keypad(run, KEY_KPPLUS)
    with run.act("Tab to the password field", should="setting the scene; not judged",
                 settle=0.3, record=0.8):
        run.key("Tab")
    run.note(f"focus in the password field: {run.last_focus()}")
    with run.act("type x in the password field",
                 [reported_on_bus(bus, "the report withholds the character: keysym "
                                  "VoidSymbol, no text, the keycode (53) kept", 53,
                                  lambda f: f[1] == VOID_SYMBOL and f[5] == ""
                                  and f[6] is False)],
                 should="the key reaches Orca without its character"):
        run.key("x")


def orca_got_keypad_with_num_lock(run: Any) -> Any:
    keysym, hw_code = KEYS["KP_Add"]

    def check(act: Any) -> tuple[bool, list[str]]:
        keys = keys_orca_got(run, act)
        mine = [k for k in keys if k["id"] == keysym and k["hw_code"] == hw_code
                and k["modifiers"] & ATSPI_NUMLOCK]
        return any(k["pressed"] for k in mine), _key_lines(keys)
    return custom(f"Orca receives keypad + (keysym {keysym:#x}, keycode {hw_code}) with "
                  f"Num Lock on", check, needs_orca=True)


SCENARIOS = [
    Scenario("fix-key-report-editor", "rich-text-editor", body_editor,
             "real keys in the rich text editor: Orca hears each, reads caret moves, "
             "echoes typing, and takes its own command key"),
    Scenario("fix-key-report-text-input", "ime-playground", body_text_input,
             "real keys in a TextInput: Orca hears each, echoes typing, takes its "
             "own command key, leaves keypad + to the field with Num Lock on; a "
             "password field's key reaches the bus without its character"),
]
