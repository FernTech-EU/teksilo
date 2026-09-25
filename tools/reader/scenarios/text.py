# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""text: what a reader gets from Teksilo's rich text surfaces.

Three examples:

* `rich-text-editor` (`examples/rich_text_editor`): a formatting toolbar
  (`format_toolbar.rs`, every button `.focusable(false)` and named by its
  tooltip), a highlighter row (`highlight_controls.rs`, a `SearchField` with a
  placeholder and no label), and a `Splitter` holding the editable
  `RichTextEditor::editor` beside a read-only `RichTextEditor::read_only`
  preview of the same `TextDocument`, loaded from a markdown sample with
  headings, lists, a table, nested blockquotes, code blocks and links.
* `rich-text-viewer` (`examples/rich_text_viewer`): one read-only
  `RichTextEditor::read_only` over a markdown sample (headings and lists, no
  links, no table).
* `ime-playground` (`examples/ime_playground`): a `TextInput`, a
  `PasswordField` and a `RichTextEditor::editor`, each under a `TextWidget`
  caption, and three global shortcuts (F1 / F2 / F3) that replay canned IME
  composition scripts through `SyntheticImeInject`, the same dispatch path a
  real input method's `WindowEvent::Ime` takes. That is how composition is
  tested here without an input method.

Where a reader's experience of a rich text surface is produced:

* `RichTextEditorBody::accessibility`
  (`crates/teksilo-widgets/src/rich_text/body.rs`): `Role::MultilineTextInput`
  for the editor, `Role::Document` (read only) for the viewer, no name, the
  text selection, the actions. The wrapper `RichTextEditor` is a
  `GenericContainer` (`rich_text.rs`, `impl Widget for RichTextEditor`).
* `FlowWalk` (`rich_text/body/flow_walk.rs`): one `TextRunSource` per block,
  built with default attributes; a heading gets `Role::Heading` with a
  `Role::Label` text container, a table `Table/Row/Cell` with a `Label` per
  cell, a blockquote `Role::Blockquote`. Lists, links and code blocks get no
  node, and nothing puts a character between two blocks.
* the runs themselves: `teksilo_core::accessibility::text_runs`.

## Orca and the keys

Orca 46.1 decides what to say about a caret move from the last key it saw:
`_presentTextAtNewCaretPosition` (`orca/scripts/default.py`) says the line
after Up/Down, the word after Ctrl+Left/Right, the character after
Left/Right/Home/End, and nothing at all when it saw no key. In the private
session (a Wayland session, `WAYLAND_DISPLAY` set) libatspi gives Orca the
*legacy* keyboard device, which learns of keys only from applications that
report them to the registry through
`org.a11y.atspi.DeviceEventController.NotifyListenersSync`, as the GTK and Qt
bridges do. `accesskit_unix` 0.23 never calls it; Teksilo itself does since
`teksilo_platform::key_report`, for every key that reaches a window while a
reader is attached. Before that, Orca heard none of the keys typed into a
Teksilo window, and said nothing about any caret move.

So `press` presses the real key through KWin and leaves the reporting to the
application. Acts whose label says "Orca told of the key" date from before
the application reported keys, when `press` reported each key itself first
(`tell_orca`); they now mean the application's own report. `tell_orca` stays
for measuring a build from before that, with `press(..., tell=True)`; on a
current build it would report every key twice.

The registry's demarshaller reads the event as `(uiiiisb)` (Qt's
`QSpiDeviceEvent`) although its introspection XML says `(uiuuisb)`, so the
report goes through Gio with the exact signature; `gdbus call` types it from
the XML and is refused.

Two measurement rules follow from the harness:

* Every scene-setting step (a focus request, a caret placement, keys that only
  set up the next act) runs inside an act of its own labelled "scene: ...".
  The harness credits Orca's speech to an act from Orca's receipt of the first
  event of the act's types that follows the previous act, so a step taken
  between two acts would have its speech credited to the next one.
* Speech checks read Orca's log from the act's own start (`heard`), because a
  reported key is echoed before Orca receives the act's first event. Orca's key
  echo itself (logged by the null speech server as "key event") is left out.

Text queries (`probe`) run in a fresh libatspi process each time, so no cache
of a long-lived client stands between the scenario and the application.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from typing import Any, Callable

from reader_lib import keys as keymap
from reader_lib.checks import _focus_node, _is_focus, custom, event, focused, no_event
from reader_lib.orca import normalized, utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

EDITOR = {"role": "entry", "state": "multi-line"}
PREVIEW = {"role": "document frame"}

# ---------------------------------------------------------------------------
# Telling Orca of a key, as a toolkit bridge does
# ---------------------------------------------------------------------------

#: X keysyms of the keys the scenarios press.
KEYSYMS = {
    "up": 0xff52, "down": 0xff54, "left": 0xff51, "right": 0xff53,
    "home": 0xff50, "end": 0xff57, "pageup": 0xff55, "pagedown": 0xff56,
    "backspace": 0xff08, "delete": 0xffff, "return": 0xff0d, "enter": 0xff0d,
    "tab": 0xff09, "escape": 0xff1b, "esc": 0xff1b, "space": 0x20,
    "f1": 0xffbe, "f2": 0xffbf, "f3": 0xffc0, "f10": 0xffc7,
}
#: AT-SPI modifier bits (`Atspi.ModifierType`): Shift 0, Control 2, Alt 3.
MODIFIER_BITS = {"shift": 1 << 0, "ctrl": 1 << 2, "control": 1 << 2, "alt": 1 << 3}


def _a11y_address() -> str:
    """The private session's accessibility bus address."""
    done = subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path",
         "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress"],
        capture_output=True, text=True, timeout=10)
    if done.returncode != 0:
        raise RunError(f"no accessibility bus address: {done.stderr.strip()}")
    return done.stdout.strip().strip("(),").strip("'")


def _a11y_connection(run: Any) -> Any:
    """A connection to the private session's accessibility bus, kept on `run`."""
    conn = getattr(run, "_text_a11y_conn", None)
    if conn is not None:
        return conn
    from gi.repository import Gio  # noqa: PLC0415 - only a run with Orca needs it

    address = _a11y_address()
    conn = Gio.DBusConnection.new_for_address_sync(
        address, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
        | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
    run._text_a11y_conn = conn
    return conn


def _chord_parts(chord: str) -> tuple[int, str, int, str]:
    """(modifier mask, key name, X keycode, text) of a chord like `Ctrl+Right`."""
    parts = chord.split("+") if chord != "+" else ["+"]
    *held, last = [p for p in parts if p] or ["+"]
    mask = 0
    for name in held:
        mask |= MODIFIER_BITS[name.lower()]
    keycode = keymap.code(last) + 8
    lower = last.lower()
    if lower in KEYSYMS:
        return mask, lower, keycode, ""
    if len(last) == 1:
        char = last.upper() if mask & MODIFIER_BITS["shift"] and last.isalpha() else last
        return mask, char, keycode, char
    raise RunError(f"no keysym for {last!r}")


def tell_orca(run: Any, chord: str) -> None:
    """Report `chord` to the registry's DeviceEventController, press then
    release, the way a GTK or Qt accessibility bridge reports each key it
    handles. Orca's legacy device receives it synchronously: when this
    returns, Orca has recorded the key as its last input event.

    Only for a build from before the application reported keys itself
    (`teksilo_platform::key_report`): a current build reports every key it
    receives, and telling Orca too would report each key twice.

    The registry's own demarshaller reads `(uiiiisb)` (Qt's `QSpiDeviceEvent`),
    whatever its introspection XML says, so the call is made with that
    signature through Gio rather than through `gdbus call`, which would type
    the argument from the XML."""
    if run.orca is None:
        return
    from gi.repository import Gio, GLib  # noqa: PLC0415

    mask, name, keycode, text = _chord_parts(chord)
    keysym = KEYSYMS.get(name, ord(name) if len(name) == 1 else 0)
    conn = _a11y_connection(run)
    for kind in (0, 1):  # KEY_PRESSED_EVENT, KEY_RELEASED_EVENT
        stamp = int(time.time() * 1000) & 0x7fffffff
        try:
            conn.call_sync(
                "org.a11y.atspi.Registry", "/org/a11y/atspi/registry/deviceeventcontroller",
                "org.a11y.atspi.DeviceEventController", "NotifyListenersSync",
                GLib.Variant("((uiiiisb))",
                             ((kind, keysym, keycode, mask, stamp, text, bool(text)),)),
                GLib.VariantType("(b)"), Gio.DBusCallFlags.NONE, 40000, None)
        except GLib.Error as exc:
            raise RunError(f"the registry refused the key report {chord!r}: {exc.message}")
    if run.current is not None:
        run.current.steps.append(f"told Orca of {chord}")


def press(run: Any, *chords: str, tell: bool = False) -> None:
    """Press each chord for real. The application reports it to Orca; `tell`
    reports it from here first, for a build that does not (`tell_orca`)."""
    for chord in chords:
        if tell:
            tell_orca(run, chord)
        run.key(chord)


def type_keys(run: Any, text: str, tell: bool = False) -> None:
    """Type lowercase ASCII text key by key (see `press` for `tell`)."""
    for ch in text:
        press(run, "space" if ch == " " else ch, tell=tell)


# ---------------------------------------------------------------------------
# Asking the application's Text interface, from a fresh libatspi client
# ---------------------------------------------------------------------------

PROBE = r'''
import json, sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

pid, spec, ops = int(sys.argv[1]), json.loads(sys.argv[2]), json.loads(sys.argv[3])
desktop = Atspi.get_desktop(0)
app = None
for i in range(desktop.get_child_count()):
    child = desktop.get_child_at_index(i)
    try:
        if child is not None and child.get_process_id() == pid:
            app = child
            break
    except Exception:
        pass
if app is None:
    print(json.dumps({"error": "the application is not on the bus"}))
    sys.exit(0)

def walk(root):
    stack = [root]
    while stack:
        node = stack.pop()
        yield node
        try:
            count = node.get_child_count()
        except Exception:
            continue
        for i in reversed(range(count)):
            try:
                child = node.get_child_at_index(i)
            except Exception:
                continue
            if child is not None:
                stack.append(child)

def matches(node):
    try:
        if "role" in spec and node.get_role_name() != spec["role"]:
            return False
        if "name" in spec and (node.get_name() or "") != spec["name"]:
            return False
        if "state" in spec:
            states = {s.value_nick for s in node.get_state_set().get_states()}
            if spec["state"] not in states:
                return False
    except Exception:
        return False
    return True

nth = int(spec.get("nth", 0))
node = None
for candidate in walk(app):
    if matches(candidate):
        if nth == 0:
            node = candidate
            break
        nth -= 1
if node is None:
    print(json.dumps({"error": f"no node matches {spec}"}))
    sys.exit(0)

T = Atspi.Text
G = {"char": Atspi.TextGranularity.CHAR, "word": Atspi.TextGranularity.WORD,
     "line": Atspi.TextGranularity.LINE, "sentence": Atspi.TextGranularity.SENTENCE,
     "paragraph": Atspi.TextGranularity.PARAGRAPH}
out = {"name": node.get_name(), "role": node.get_role_name()}
try:
    out["states"] = sorted(s.value_nick for s in node.get_state_set().get_states())
    out["interfaces"] = sorted(node.get_interfaces())
    if "Text" not in out["interfaces"]:
        print(json.dumps(out))
        sys.exit(0)
    out["count"] = T.get_character_count(node)
    out["caret"] = T.get_caret_offset(node)
except Exception as exc:
    out["error"] = f"{type(exc).__name__}: {exc}"
    print(json.dumps(out))
    sys.exit(0)
if "set_caret" in ops:
    out["set_caret_done"] = bool(T.set_caret_offset(node, int(ops["set_caret"])))
if "find" in ops:
    whole = T.get_text(node, 0, out["count"])
    out["find"] = whole.find(ops["find"])
if "text" in ops:
    start, end = ops["text"]
    out["text"] = T.get_text(node, start, min(end, out["count"]))
at = ops.get("at", out["caret"])
out["at"] = at
for gran in ops.get("grans", []):
    try:
        rng = T.get_string_at_offset(node, at, G[gran])
        out[gran] = {"text": rng.content, "start": rng.start_offset, "end": rng.end_offset}
    except Exception as exc:
        out[gran] = {"error": f"{type(exc).__name__}: {exc}"}
if ops.get("attrs"):
    try:
        attrs, start, end = T.get_attribute_run(node, at, False)
        out["attrs"] = {"attrs": dict(attrs or {}), "start": start, "end": end}
    except Exception as exc:
        out["attrs"] = {"error": f"{type(exc).__name__}: {exc}"}
if ops.get("selection"):
    try:
        out["selections"] = [[T.get_selection(node, i).start_offset,
                              T.get_selection(node, i).end_offset]
                             for i in range(T.get_n_selections(node))]
    except Exception as exc:
        out["selections"] = f"{type(exc).__name__}: {exc}"
if ops.get("hypertext"):
    try:
        out["links"] = Atspi.Hypertext.get_n_links(node) if node.get_hypertext_iface() else None
    except Exception as exc:
        out["links"] = f"{type(exc).__name__}: {exc}"
print(json.dumps(out, ensure_ascii=False))
'''


def probe(run: Any, spec: dict, **ops: Any) -> dict:
    """Ask a fresh libatspi client for the Text interface of the node `spec`
    names: `grans=("char", "word", "line")` at the caret (or `at=`), `attrs`,
    `selection`, `text=(start, end)`, `find="..."`, `set_caret=offset`."""
    assert run.app is not None
    done = subprocess.run([sys.executable, "-W", "ignore", "-c", PROBE, str(run.app.pid),
                           json.dumps(spec), json.dumps(ops)],
                          capture_output=True, text=True, timeout=60)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {"error": f"probe failed: {done.stderr.strip()[-500:]}"}


# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------


def heard(run: Any, act: Any, echo: bool = False) -> list:
    """What Orca said from the act's start to where it went quiet after it.

    The harness credits Orca's speech to an act from Orca's receipt of the
    act's first event. Orca hears of a key, and echoes it, as the application
    reports it, before the application acts on it and so before that
    receipt, so this reads from the act's own start instead. Key echo (Orca's
    own `speakKeyEvent`, which the null speech server logs as "key event") is
    left out unless `echo`: it is Orca repeating the key, not the application
    telling the reader anything."""
    from reader_lib.orca import ORCA_LINE, lines_between  # noqa: PLC0415

    if run.orca is None:
        return []
    text = run.orca.text()
    lines = lines_between(text, act.start_wall, act.orca_end or act.end_wall)
    said = utterances(lines)
    echoes: set[str] = set()
    pending = None
    for raw in text.splitlines():
        match = ORCA_LINE.match(raw.strip())
        if not match:
            continue
        stamp, rest = match.groups()
        if rest.startswith("SPEECH OUTPUT:"):
            pending = stamp
        elif rest.startswith("NULL SPEECH: key event") and pending:
            echoes.add(pending)
            pending = None
        elif rest.startswith("NULL SPEECH:"):
            pending = None
    return [u for u in said if echo or u.stamp not in echoes]


def _lines(said: list) -> list[str]:
    return [f"{u.stamp} Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in said] \
        or ["Orca said nothing in this act"]


def said_expected(run: Any, describe: str, expected: Callable[[], str | None]) -> Any:
    """Orca said, uncut, something containing the string `expected()` gives at
    evaluation time (recorded by the scenario after the act)."""
    def check(act: Any) -> tuple[bool, list[str]]:
        want = expected()
        said = heard(run, act)
        evidence = [f"expected {want!r}"] + _lines(said)
        if not want:
            return False, evidence + ["(no expected text was recorded)"]
        return any(normalized(want) in normalized(u.text) and not u.cut for u in said), evidence
    return custom(describe, check, needs_orca=True)


def said_something(run: Any, describe: str = "Orca says something about the act") -> Any:
    def check(act: Any) -> tuple[bool, list[str]]:
        said = heard(run, act)
        return bool(said), _lines(said)
    return custom(describe, check, needs_orca=True)


def said_exactly(run: Any, text: str, want: bool, echo: bool = False) -> Any:
    """Orca did (or did not) say an utterance that is exactly `text`."""
    def check(act: Any) -> tuple[bool, list[str]]:
        said = heard(run, act, echo=echo)
        found = [u for u in said if u.text.strip() == text]
        return bool(found) == want, _lines(said)
    return custom(f"Orca {'says' if want else 'does not say'} exactly {text!r}", check,
                  needs_orca=True)


def said_any(run: Any, describe: str, words: list[str]) -> Any:
    """Some utterance contains one of `words`."""
    def check(act: Any) -> tuple[bool, list[str]]:
        said = heard(run, act)
        ok = any(normalized(w) in normalized(u.text) for u in said for w in words)
        return ok, _lines(said)
    return custom(describe, check, needs_orca=True)


def value_is(describe: str, got: Callable[[], Any], want: Any) -> Any:
    """A fact the scenario recorded after the act equals `want`."""
    def check(act: Any) -> tuple[bool, list[str]]:
        value = got()
        return value == want, [f"got {value!r}, want {want!r}"]
    return custom(describe, check)


def fact(describe: str, test: Callable[[], tuple[bool, list[str]]]) -> Any:
    """A check over facts the scenario recorded after the act."""
    return custom(describe, lambda act: test())


def caret_event(role: str = "entry") -> Any:
    return event("object:text-caret-moved", role=role)


def focused_editor_named() -> Any:
    def check(act: Any) -> tuple[bool, list[str]]:
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change in this act"]
        node = _focus_node(moves[-1])
        return bool(node.get("name")), [f"focus on [{node.get('role')}] {node.get('name')!r}"]
    return custom("the control focus lands on has a name", check)


def tree_has(describe: str, test: Callable[[dict], bool]) -> Any:
    from reader_lib.checks import _walk  # noqa: PLC0415

    def check(act: Any) -> tuple[bool, list[str]]:
        found = [n for n in _walk(act.tree) if test(n)]
        return bool(found), [f"found [{n.get('role')}] {n.get('name')!r} "
                             f"attrs={n.get('attributes')}" for n in found[:4]] \
            or ["no such node in the tree after the act"]
    return custom(describe, check, needs_tree=True)


def tree_all(describe: str, select: Callable[[dict], bool],
             test: Callable[[dict], bool]) -> Any:
    from reader_lib.checks import _walk  # noqa: PLC0415

    def check(act: Any) -> tuple[bool, list[str]]:
        chosen = [n for n in _walk(act.tree) if select(n)]
        bad = [n for n in chosen if not test(n)]
        if not chosen:
            return False, ["no node of that kind in the tree"]
        return not bad, [f"{len(bad)} of {len(chosen)} fail, e.g. [{n.get('role')}] "
                         f"{n.get('name')!r} attrs={n.get('attributes')} children="
                         f"{[(c.get('role'), c.get('name')) for c in n.get('children', [])][:2]}"
                         for n in bad[:3]] or [f"all {len(chosen)} pass"]
    return custom(describe, check, needs_tree=True)


# ---------------------------------------------------------------------------
# Scenes
# ---------------------------------------------------------------------------


def scene(run: Any, label: str, fn: Callable[[], Any]) -> Any:
    """Set the scene inside an act of its own, so the events it causes, and
    Orca's handling of them, are not credited to the next act."""
    result = None
    with run.act(f"scene: {label}", should="setting the scene; not judged",
                 settle=0.3, record=0.8):
        result = fn()
    return result


def focus_by_request(run: Any, spec: dict = EDITOR) -> None:
    """Focus a text surface the way a screen reader asks for it: AT-SPI
    `grab_focus` on the text node itself."""
    run.wait_for(**spec)
    scene(run, f"grab_focus on {spec}", lambda: run.grab_focus(**spec))


def set_caret(run: Any, spec: dict, offset: int) -> dict:
    """Place the caret through AT-SPI `set_caret_offset`, as a screen reader's
    own caret placement does."""
    return scene(run, f"set the caret to {offset}",
                 lambda: probe(run, spec, set_caret=offset))


def keys_scene(run: Any, *chords: str) -> None:
    scene(run, " ".join(chords), lambda: press(run, *chords))


def offset_of(run: Any, spec: dict, needle: str) -> int:
    found = probe(run, spec, find=needle)
    where = found.get("find", -1)
    if where is None or where < 0:
        raise RunError(f"{needle!r} is not in the text of {spec}: {found.get('error')}")
    return where


class Facts(dict):
    """What the scenario read from the bus after each act, for checks that
    run when the run is collected."""

    def get_path(self, key: str, *path: str) -> Any:
        value: Any = self.get(key)
        for part in path:
            if not isinstance(value, dict):
                return None
            value = value.get(part)
        return value


def nav_act(run: Any, facts: Facts, label: str, chord: str, unit: str, should: str,
            extra: list | None = None, spec: dict = EDITOR, role: str = "entry") -> None:
    """One caret move, Orca told of the key: the caret event, and Orca saying
    the `unit` ("char", "word" or "line") the Text interface reports at the
    new caret."""
    def expected() -> str | None:
        text = facts.get_path(label, unit, "text")
        return text.strip() if isinstance(text, str) and unit != "char" else text
    checks = [caret_event(role),
              said_expected(run, f"Orca reads the {unit} at the new caret", expected)]
    with run.act(label, checks + list(extra or []), should=should):
        press(run, chord)
    facts[label] = probe(run, spec, grans=["char", "word", "line"])
    run.note(f"{label}: caret {facts.get_path(label, 'caret')}, "
             f"char {facts.get_path(label, 'char', 'text')!r}, "
             f"word {facts.get_path(label, 'word', 'text')!r}, "
             f"line {facts.get_path(label, 'line', 'text')!r}")


# ---------------------------------------------------------------------------
# rich-text-editor
# ---------------------------------------------------------------------------


def body_editor_tab_in(run: Any) -> None:
    """The keyboard's way into the editor: Tab, then a line, a letter, out and
    back in again."""
    run.wait_for(**EDITOR)
    facts = Facts()
    with run.act("Tab from the search field into the editor",
                 [focused(role="entry"), focused_editor_named(),
                  said_any(run, "Orca says it is an editable text", ["entry", "text"])],
                 should="focus lands on the editor's text, and the reader hears a name "
                        "and that it is an editable text"):
        press(run, "Tab")
    facts["focus"] = run.last_focus()
    run.note(f"after Tab, focus: {facts['focus']}")
    with run.act("Down in the editor (Orca told of the key)",
                 [caret_event(), said_something(run, "Orca reads the next line")],
                 should="the caret moves to the next line and the reader hears it"):
        press(run, "Down")
    facts["after down"] = probe(run, EDITOR, grans=["line"], selection=True)
    run.note(f"after Down, the entry's caret and line: {facts['after down']}")
    with run.act("type x in the editor (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry", text_contains="x")],
                 should="the letter goes in and the text change reaches the reader"):
        press(run, "x")
    with run.act("Backspace in the editor (Orca told of the key)",
                 [event("object:text-changed:delete", role="entry", text_contains="x"),
                  said_exactly(run, "x", True)],
                 should="the letter goes and the reader hears which"):
        press(run, "BackSpace")
    with run.act("Ctrl+Tab out of the editor",
                 [said_something(run, "Orca says where focus went")],
                 should="focus leaves the editor for the next control, which the reader hears"):
        press(run, "Ctrl+Tab")
    run.note(f"after Ctrl+Tab, focus: {run.last_focus()}")
    with run.act("Ctrl+Shift+Tab back into the editor",
                 [focused(role="entry"), said_something(run, "Orca says where focus went")],
                 should="focus comes back to the editor and the reader hears it"):
        press(run, "Ctrl+Shift+Tab")
    run.note(f"after Ctrl+Shift+Tab, focus: {run.last_focus()}")
    with run.act("Down after coming back (Orca told of the key)",
                 [caret_event(), said_something(run, "Orca reads the next line")],
                 should="the caret moves and the reader hears the line"):
        press(run, "Down")


def body_editor_caret(run: Any) -> None:
    """Caret moves by character, word, line and document, with focus put on
    the editor's text by a screen reader's own request."""
    focus_by_request(run)
    keys_scene(run, "Ctrl+Home")
    facts = Facts()
    nav_act(run, facts, "Right: next character", "Right", "char",
            "the caret moves one character and the reader hears the character",
            extra=[said_exactly(run, "i", True)])
    nav_act(run, facts, "Ctrl+Right: next word", "Ctrl+Right", "word",
            "the caret moves one word and the reader hears the word")
    heading = "RichTextEditor — Capability Showcase"
    heading_end = len(heading)
    # The H1 wraps: "RichTextEditor — Capability " then "Showcase". From
    # offset 14, Down lands on the second line, whose 8 characters end at the
    # end of the paragraph.
    with run.act("Down onto the heading's short last line (Orca told of the key)",
                 [caret_event(),
                  said_any(run, "Orca reads the line the caret is on: 'Showcase'",
                           ["Showcase"]),
                  said_exactly(run, "This window hosts two RichTextEditor widgets bound "
                               "to the same TextDocument. The", False)],
                 should="the caret moves to the heading's second line, 'Showcase', and "
                        "the reader hears that line"):
        press(run, "Down")
    facts["down"] = probe(run, EDITOR, grans=["char", "line"])
    run.note(f"after Down: {facts['down']}")
    nav_act(run, facts, "Up: back to the first line", "Up", "line",
            "the caret moves back up and the reader hears the first line",
            extra=[said_any(run, "Orca reads the heading's first line", ["RichTextEditor"])])
    nav_act(run, facts, "Home: start of the line", "Home", "char",
            "the caret goes to the start of the line and the reader hears its first "
            "character", extra=[said_exactly(run, "R", True)])
    set_caret(run, EDITOR, heading_end - 4)
    with run.act("End: end of the first paragraph (Orca told of the key)",
                 [caret_event(),
                  value_is("the caret offset is the end of the heading",
                           lambda: facts.get_path("end of paragraph", "caret"), heading_end),
                  fact("the character at the caret is a line break, not the next "
                       "paragraph's first letter",
                       lambda: (facts.get_path("end of paragraph", "char", "text")
                                in ("\n", "\r\n", ""),
                                [f"char at caret: {facts.get_path('end of paragraph', 'char')!r}",
                                 f"text around: {facts.get_path('end of paragraph', 'text')!r}"])),
                  said_exactly(run, "T", False)],
                 should="the caret stops after 'Showcase', and the reader hears nothing "
                        "of the next paragraph"):
        press(run, "End")
    facts["end of paragraph"] = probe(run, EDITOR, grans=["char", "word", "line"],
                                      text=[heading_end - 8, heading_end + 8])
    run.note(f"End at the heading: {facts['end of paragraph']}")
    with run.act("Right across the paragraph break (Orca told of the key)",
                 [caret_event(),
                  fact("the caret offset moves on",
                       lambda: ((facts.get_path("after break", "caret") or 0) > heading_end,
                                [f"caret before {heading_end}, after "
                                 f"{facts.get_path('after break', 'caret')}"])),
                  said_exactly(run, "T", True)],
                 should="the caret moves to the start of the next paragraph and the reader "
                        "hears its first letter"):
        press(run, "Right")
    facts["after break"] = probe(run, EDITOR, grans=["char", "word", "line"])
    run.note(f"Right across the break: {facts['after break']}")
    with run.act("the word at the paragraph break, as the Text interface answers",
                 [fact("the word ending the heading is 'Showcase', not run into the next "
                       "paragraph's first word",
                       lambda: ("Showcase" == (facts.get_path("word at break", "word", "text")
                                               or "").strip(),
                                [f"{facts.get('word at break')}"])),
                  fact("the paragraph at the heading is the heading alone",
                       lambda: ((facts.get_path("word at break", "paragraph", "text") or "")
                                .strip() == heading,
                                [f"paragraph: {facts.get_path('word at break', 'paragraph')!r}"])),
                  fact("the text holds a separator between the two paragraphs",
                       lambda: ((facts.get_path("word at break", "text") or "")
                                != "ShowcaseThis",
                                [f"text {heading_end - 8}..{heading_end + 4}: "
                                 f"{facts.get_path('word at break', 'text')!r}"]))],
                 should="word boundaries and the text stop at a paragraph break", record=0.2):
        facts["word at break"] = probe(run, EDITOR, grans=["word", "paragraph"],
                                       at=heading_end - 1,
                                       text=[heading_end - 8, heading_end + 4])
    run.note(f"at the break: {facts['word at break']}")
    nav_act(run, facts, "Ctrl+End: end of the document", "Ctrl+End", "line",
            "the caret goes to the end and the reader hears the last line",
            extra=[said_any(run, "Orca reads the last line", ["watch the preview pane"])])
    nav_act(run, facts, "Ctrl+Home: start of the document", "Ctrl+Home", "line",
            "the caret goes to the start and the reader hears the first line",
            extra=[said_any(run, "Orca reads the first line", ["RichTextEditor"])])
    nav_act(run, facts, "Page Down", "PageDown", "line",
            "the view pages down and the reader hears the line the caret landed on")


def body_editor_keys_unreported(run: Any) -> None:
    """Caret moves with no key reported from the harness: what Orca makes of
    them rests on the application's own report (`teksilo_platform::key_report`).
    Before that, no key was ever reported to Orca, as none is by
    `accesskit_unix`, and it said nothing (finding text-15). Focus is put on
    the editor's text by a screen reader's own request, so the caret events
    do reach the bus."""
    focus_by_request(run)
    scene(run, "Ctrl+Home, not reported", lambda: press(run, "Ctrl+Home", tell=False))
    for label, chord in (("Down", "Down"), ("Right", "Right"), ("Ctrl+Right", "Ctrl+Right"),
                         ("End", "End")):
        with run.act(f"{label}, no key reported but by the application",
                     [caret_event(), said_something(run, "Orca reads what the caret moved over")],
                     should="the caret moves and the reader hears what it moved to"):
            press(run, chord, tell=False)


def body_editor_select(run: Any) -> None:
    """Selecting with Shift+arrows, Shift+End, Shift+Down and Ctrl+A."""
    focus_by_request(run)
    keys_scene(run, "Ctrl+Home")
    facts = Facts()

    def sel_act(label: str, chord: str, want: str | None, should: str) -> None:
        checks = [event("object:text-selection-changed", role="entry"),
                  said_any(run, "Orca says what is selected", ["selected"])]
        if want:
            checks.append(said_any(run, f"Orca names the selection {want!r}", [want]))
        with run.act(label, checks, should=should):
            press(run, chord)
        facts[label] = probe(run, EDITOR, selection=True)
        run.note(f"{label}: {facts[label].get('selections')} caret {facts[label].get('caret')}")

    sel_act("Shift+Right: select one character", "Shift+Right", "R",
            "one character is selected and the reader hears it, selected")
    sel_act("Shift+Ctrl+Right: extend by a word", "Shift+Ctrl+Right", "ichTextEditor",
            "the selection grows by a word and the reader hears the added text")
    sel_act("Shift+End: extend to the end of the line", "Shift+End", "Capability",
            "the selection reaches the end of the line and the reader hears the added text")
    sel_act("Shift+Down: extend by a line", "Shift+Down", "Showcase",
            "the selection grows by the heading's second line and the reader hears it")
    with run.act("Right: collapse the selection",
                 [event("object:text-selection-changed", role="entry"),
                  said_any(run, "Orca says the text is unselected", ["unselected"])],
                 should="the selection goes away and the reader hears that it did"):
        press(run, "Right")
    with run.act("Ctrl+A: select all",
                 [event("object:text-selection-changed", role="entry"),
                  said_any(run, "Orca says everything is selected", ["selected"])],
                 should="the whole document is selected and the reader is told so"):
        press(run, "Ctrl+A")
    facts["all"] = probe(run, EDITOR, selection=True)
    run.note(f"Ctrl+A: {facts['all'].get('selections')} of {facts['all'].get('count')}")


def body_editor_typing(run: Any) -> None:
    """Typing, deleting, a new paragraph, undo, paste, at the document's end."""
    focus_by_request(run)
    keys_scene(run, "Ctrl+End")
    facts = Facts()
    facts["start"] = probe(run, EDITOR, selection=True)
    count0 = facts["start"].get("count") or 0
    with run.act("Enter: a new paragraph (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry"),
                  fact("the text grows by a line break",
                       lambda: ((facts.get_path("enter", "count") or 0) == count0 + 1,
                                [f"characters before {count0}, after "
                                 f"{facts.get_path('enter', 'count')}"]))],
                 should="a new, empty paragraph starts; the text gains a line break"):
        press(run, "Return")
    facts["enter"] = probe(run, EDITOR, grans=["line", "char"],
                           text=[max(0, count0 - 10), count0 + 5])
    run.note(f"after Enter: {facts['enter']}")
    with run.act("type 'hello' (Orca told of each key)",
                 [event("object:text-changed:insert", role="entry", text_contains="h"),
                  event("object:text-changed:insert", role="entry", text_contains="o"),
                  fact("the new word is on a line of its own",
                       lambda: ((facts.get_path("typed", "line", "text") or "").strip()
                                == "hello",
                                [f"line at caret: {facts.get_path('typed', 'line')!r}"]))],
                 should="each letter goes in, on the new line"):
        type_keys(run, "hello")
    facts["typed"] = probe(run, EDITOR, grans=["line", "word"])
    run.note(f"after typing: {facts['typed']}")
    with run.act("Backspace (Orca told of the key)",
                 [event("object:text-changed:delete", role="entry", text_contains="o"),
                  said_exactly(run, "o", True)],
                 should="the last letter goes and the reader hears which"):
        press(run, "BackSpace")
    with run.act("Ctrl+Backspace: delete the word (Orca told of the key)",
                 [event("object:text-changed:delete", role="entry", text_contains="hell"),
                  said_any(run, "Orca says the deleted word", ["hell"])],
                 should="the word goes and the reader hears which"):
        press(run, "Ctrl+BackSpace")
    with run.act("Ctrl+Z: undo (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry", text_contains="hell"),
                  said_something(run, "Orca says what undo did")],
                 should="the word comes back and the reader hears what changed"):
        press(run, "Ctrl+Z")
    facts["undone"] = probe(run, EDITOR, grans=["line"])
    run.note(f"after undo: {facts['undone']}")
    keys_scene(run, "Ctrl+Home", "Shift+Ctrl+Right", "Ctrl+C", "Ctrl+End")
    with run.act("Ctrl+V: paste the copied word (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry",
                        text_contains="RichTextEditor"),
                  said_any(run, "Orca says the text was pasted, or reads it",
                           ["pasted", "RichTextEditor"])],
                 should="the copied word goes in and the reader hears it was pasted"):
        press(run, "Ctrl+V")
    facts["pasted"] = probe(run, EDITOR, grans=["line"])
    run.note(f"after paste: {facts['pasted']}")


def body_editor_format(run: Any) -> None:
    """Formatting: what the Text interface says of bold text and links, and
    what Ctrl+B and the toolbar tell a reader."""
    focus_by_request(run)
    facts = Facts()
    two = offset_of(run, EDITOR, "hosts two ") + len("hosts ")
    with run.act("the attributes of the bold word 'two', as the Text interface answers",
                 [fact("the attribute run at 'two' says it is bold",
                       lambda: (any("bold" in str(v).lower() or str(v) in ("700", "800", "900")
                                    for v in (facts.get_path("two", "attrs", "attrs")
                                              or {}).values()),
                                [f"{facts.get('two', {}).get('attrs')}"]))],
                 should="a reader asking for the text's attributes learns 'two' is bold",
                 record=0.2):
        facts["two"] = probe(run, EDITOR, attrs=True, at=two + 1)
    link = offset_of(run, EDITOR, "the text-document repo")
    with run.act("the link 'the text-document repo', as the Text interface answers",
                 [fact("the editor offers its links (Hypertext)",
                       lambda: ((facts.get_path("link", "links") or 0) > 0,
                                [f"hypertext links: {facts.get_path('link', 'links')!r}, "
                                 f"attrs at the link: {facts.get_path('link', 'attrs')}"]))],
                 should="a reader can find and follow the link", record=0.2):
        facts["link"] = probe(run, EDITOR, attrs=True, hypertext=True, at=link + 2)
    window = offset_of(run, EDITOR, "This window hosts") + len("This ")
    set_caret(run, EDITOR, window)
    keys_scene(run, "Shift+Ctrl+Right")
    facts["selected"] = probe(run, EDITOR, selection=True)
    run.note(f"selection before Ctrl+B: {facts['selected'].get('selections')}")
    with run.act("Ctrl+B on the selected word (Orca told of the key)",
                 [event("object:state-changed:pressed", role="toggle button",
                        name_contains="Bold"),
                  said_any(run, "the reader hears that bold is now on", ["bold"]),
                  fact("the Text interface now says the word is bold",
                       lambda: (any("bold" in str(v).lower() or str(v) in ("700", "800", "900")
                                    for v in (facts.get_path("bolded", "attrs", "attrs")
                                              or {}).values()),
                                [f"{facts.get('bolded', {}).get('attrs')}"]))],
                 should="the word turns bold, the Bold button shows pressed, and the reader "
                        "hears that bold is on"):
        press(run, "Ctrl+B")
    facts["bolded"] = probe(run, EDITOR, attrs=True, at=window + 1)
    bold = run.find(role="toggle button", name_contains="Bold")
    run.note(f"Bold button after Ctrl+B: {bold and bold.get('states')}")
    with run.act("activate Italic through AT-SPI",
                 [event("object:state-changed:pressed", role="toggle button",
                        name_contains="Italic")],
                 should="the selected word turns italic and the Italic button shows pressed"):
        run.action("click", role="toggle button", name_contains="Italic")
    italic = run.find(role="toggle button", name_contains="Italic")
    run.note(f"Italic button after its action: {italic and italic.get('states')}")
    facts["italic"] = probe(run, EDITOR, attrs=True, at=window + 1)
    run.note(f"attributes at 'window' after Italic: {facts['italic'].get('attrs')}")
    with run.act("Tab from the editor to the toolbar",
                 [focused(role="toggle button")],
                 should="the formatting buttons are reachable by keyboard", record=1.0):
        press(run, "Ctrl+Tab")
    run.note(f"after Ctrl+Tab from the editor, focus: {run.last_focus()}")


def body_editor_structure(run: Any) -> None:
    """Headings, lists, a table, blockquotes and links: what the tree holds,
    and what Orca reads line by line through them."""
    run.wait_for(**EDITOR)
    with run.act("the document's structure in the tree",
                 [tree_all("every heading has a name",
                           lambda n: n.get("role") == "heading", lambda n: bool(n.get("name"))),
                  tree_all("every heading has a level",
                           lambda n: n.get("role") == "heading",
                           lambda n: "level" in (n.get("attributes") or {})),
                  tree_has("the lists are lists (a list or list item node)",
                           lambda n: n.get("role") in ("list", "list item")),
                  tree_has("the links are links", lambda n: n.get("role") == "link"),
                  tree_all("the editor and the preview are named",
                           lambda n: (n.get("role") == "entry"
                                      and "multi-line" in n.get("states", []))
                           or n.get("role") == "document frame",
                           lambda n: bool(n.get("name")))],
                 should="headings, lists, links and both panes carry what a reader "
                        "navigates by", record=0.3):
        pass
    focus_by_request(run)
    facts = Facts()

    def line_through(label: str, needle: str, want: list[str], should: str) -> None:
        start = offset_of(run, EDITOR, needle)
        set_caret(run, EDITOR, start)
        keys_scene(run, "Up")
        nav_act(run, facts, label, "Down", "line", should,
                extra=[said_any(run, f"Orca says {' or '.join(map(repr, want))}", want)])

    line_through("Down onto the first bullet item", "First item at indent 0",
                 ["•", "bullet", "list item"],
                 "the reader hears the item's text and that it is a bulleted item")
    line_through("Down onto the first numbered item", "First numbered item",
                 ["1.", "1 ", "one", "list item"],
                 "the reader hears the item's number with its text")
    line_through("Down onto a nested numbered item", "Nested decimal at indent 1",
                 ["1.", "nesting level", "level 2"],
                 "the reader hears the item's number and depth")
    line_through("Down onto a line with a link", "carry an anchor_href",
                 ["link"], "the reader hears that the line holds a link")
    line_through("Down into the table's second row", "Bold**text**",
                 ["Bold"], "the reader hears the first body row's first cell")
    line_through("Down onto the blockquote", "A single-level blockquote",
                 ["quote", "block quote"], "the reader hears that the text is quoted")
    line_through("Down onto a code block line", "fn fibonacci",
                 ["fn fibonacci"], "the reader hears the code line")


# ---------------------------------------------------------------------------
# rich-text-viewer
# ---------------------------------------------------------------------------

VIEWER = {"role": "document frame"}


def body_viewer(run: Any) -> None:
    """The read-only viewer: the keyboard's way in, and reading it by line."""
    run.wait_for(**VIEWER)
    with run.act("the viewer in the tree",
                 [tree_all("the document is named", lambda n: n.get("role") == "document frame",
                           lambda n: bool(n.get("name"))),
                  tree_has("the lists are lists (a list or list item node)",
                           lambda n: n.get("role") in ("list", "list item"))],
                 should="the document carries a name, says it is read-only, and exposes "
                        "its lists", record=0.3):
        pass
    facts = Facts()
    with run.act("Tab to the Theme combo box", [focused(role="combo box")],
                 should="focus moves to the toolbar's Theme picker"):
        press(run, "Tab")
    with run.act("Tab into the viewer",
                 [focused(role="document frame"), focused_editor_named(),
                  said_any(run, "Orca says it is a document", ["document"])],
                 should="focus lands on the document, and the reader hears its name"):
        press(run, "Tab")
    facts["tab"] = run.last_focus()
    with run.act("Down in the viewer (Orca told of the key)",
                 [caret_event("document frame"), said_something(run, "Orca reads the next line")],
                 should="the reader hears the next line"):
        press(run, "Down")
    with run.act("Tab out to the Theme combo box", [focused(role="combo box")],
                 should="focus moves back to the toolbar"):
        press(run, "Tab")
    with run.act("Tab into the viewer a second time",
                 [said_something(run, "Orca says where focus went"),
                  focused(role="document frame")],
                 should="focus lands on the document again, and the reader hears it again"):
        press(run, "Tab")
    focus_by_request(run, VIEWER)
    keys_scene(run, "Ctrl+Home")
    nav_act(run, facts, "Down after a focus request (Orca told of the key)", "Down", "line",
            "the reader hears the next line", spec=VIEWER, role="document frame")
    nav_act(run, facts, "Down again", "Down", "line",
            "the reader hears the next line", spec=VIEWER, role="document frame")
    nav_act(run, facts, "Ctrl+Right: next word", "Ctrl+Right", "word",
            "the reader hears the next word", spec=VIEWER, role="document frame")
    item = offset_of(run, VIEWER, "Mouse wheel scrolling")
    set_caret(run, VIEWER, item)
    keys_scene(run, "Up")
    nav_act(run, facts, "Down onto a bullet item", "Down", "line",
            "the reader hears the item and that it is a bulleted item", spec=VIEWER,
            role="document frame",
            extra=[said_any(run, "Orca says the bullet", ["•", "bullet", "list item"])])
    with run.act("Shift+Down: select a line (Orca told of the key)",
                 [event("object:text-selection-changed", role="document frame"),
                  said_any(run, "Orca says what is selected", ["selected"])],
                 should="the line is selected and the reader hears it"):
        press(run, "Shift+Down")
    with run.act("type x in the viewer (Orca told of the key)",
                 [no_event("object:text-changed", role="document frame")],
                 should="nothing changes: the document is read-only"):
        press(run, "x")


# ---------------------------------------------------------------------------
# ime-playground
# ---------------------------------------------------------------------------

def ime_acts(run: Any, spec: dict, role: str, secure: bool = False) -> None:
    facts = Facts()
    facts["before"] = probe(run, spec, text=[0, 200])
    with run.act(f"F1: Pinyin ni → nihao → commit 你好 in the {role}",
                 ([no_event("object:text-changed", role=role)] if secure else
                  [event("object:text-changed:insert", role=role, text_contains="你好"),
                   fact("the committed text is in the field and no composition is left",
                        lambda: ("你好" in (facts.get_path("f1", "text") or "")
                                 and "nihao" not in (facts.get_path("f1", "text") or ""),
                                 [f"text after: {facts.get_path('f1', 'text')!r}"]))]),
                 should="the composition shows while typing and the commit leaves 你好; "
                        + ("nothing of it reaches the bus in clear" if secure else
                           "the reader can hear what was committed")):
        press(run, "F1", tell=False)
    facts["f1"] = probe(run, spec, text=[0, 200])
    run.note(f"{role} after F1: {facts['f1'].get('text')!r} caret {facts['f1'].get('caret')}")
    with run.act(f"F2: dead-key ^ then commit ê in the {role}",
                 ([no_event("object:text-changed", role=role)] if secure else
                  [event("object:text-changed:insert", role=role, text_contains="ê")]),
                 should="ê is committed"):
        press(run, "F2", tell=False)
    facts["f2"] = probe(run, spec, text=[0, 200])
    run.note(f"{role} after F2: {facts['f2'].get('text')!r}")
    with run.act(f"F3: compose ni then cancel in the {role}",
                 [fact("cancelling leaves the text as it was",
                       lambda: (facts.get_path("f3", "text") == facts.get_path("f2", "text"),
                                [f"before {facts.get_path('f2', 'text')!r}, "
                                 f"after {facts.get_path('f3', 'text')!r}"]))],
                 should="nothing is committed, and the field reads as before"):
        press(run, "F3", tell=False)
    facts["f3"] = probe(run, spec, text=[0, 200])


def body_ime_fields(run: Any) -> None:
    """The three fields: their names, and composition replayed into each."""
    single = {"role": "entry", "state": "single-line", "nth": 0}
    run.wait_for(**single)
    with run.act("the fields in the tree",
                 [tree_all("every text field is named",
                           lambda n: n.get("role") in ("entry", "password text")
                           and "focusable" in n.get("states", []),
                           lambda n: bool(n.get("name")))],
                 should="each field carries the caption shown above it", record=0.3):
        pass
    run.note(f"focus at launch: {run.last_focus()}")
    field = {"role": "entry", "state": "focused"}
    facts = Facts()
    facts["launch"] = probe(run, field)
    run.note(f"the empty TextInput at launch: interfaces {facts['launch'].get('interfaces')}")
    ime_acts(run, field, "entry")
    keys_scene(run, "Ctrl+A", "BackSpace")
    facts["emptied"] = probe(run, field)
    run.note(f"the TextInput emptied: {facts['emptied']}")
    with run.act("type a into the empty TextInput (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry", text_contains="a")],
                 should="the first character typed into an empty field reaches the reader "
                        "as a text change, as every later one does"):
        press(run, "a")
    with run.act("type b after it (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry", text_contains="b")],
                 should="the second character reaches the reader as a text change"):
        press(run, "b")
    facts["typed"] = probe(run, field, grans=["char"])
    run.note(f"the TextInput after typing ab: {facts['typed']}")
    with run.act("Left in the TextInput (Orca told of the key)",
                 [caret_event(), said_exactly(run, "b", True)],
                 should="the caret moves back over b and the reader hears b"):
        press(run, "Left")
    facts["left"] = probe(run, field, grans=["char"])
    run.note(f"the TextInput after Left: caret {facts['left'].get('caret')}")
    with run.act("Home in the TextInput (Orca told of the key)",
                 [caret_event(), said_exactly(run, "a", True),
                  fact("the Text interface's caret is at 0",
                       lambda: (facts.get_path("home", "caret") == 0,
                                [f"caret {facts.get_path('home', 'caret')}"]))],
                 should="the caret goes to the start and the reader hears a"):
        press(run, "Home")
    facts["home"] = probe(run, field, grans=["char"])
    run.note(f"the TextInput after Home: caret {facts['home'].get('caret')}")
    with run.act("type c at the start (Orca told of the key)",
                 [event("object:text-changed:insert", role="entry", text_contains="c"),
                  fact("the Text interface's caret follows the insertion (1)",
                       lambda: (facts.get_path("c", "caret") == 1,
                                [f"caret {facts.get_path('c', 'caret')}, "
                                 f"text {facts.get_path('c', 'text')!r}"]))],
                 should="c goes in before a; the reader's caret follows"):
        press(run, "c")
    facts["c"] = probe(run, field, text=[0, 10])
    run.note(f"the TextInput after typing c: {facts['c']}")
    with run.act("Tab to the password field",
                 [focused(role="password text"), focused_editor_named()],
                 should="focus lands on the password field and the reader hears its name"):
        press(run, "Tab")
    ime_acts(run, {"role": "password text"}, "password text", secure=True)
    keys_scene(run, "Tab")  # the reveal toggle
    with run.act("Tab to the rich editor",
                 [focused(role="entry"), focused_editor_named()],
                 should="focus lands on the editor's text, and the reader hears its caption"):
        press(run, "Tab")
    ime_acts(run, EDITOR, "entry")


def body_editor_shift_tab(run: Any) -> None:
    """Shift+Tab backwards from the search field: which of the formatting
    toolbar's controls the keyboard reaches."""
    from reader_lib.scenario import tab_walk  # noqa: PLC0415

    run.wait_for(**EDITOR)
    landed = tab_walk(run, stops=10, chord="Shift+Tab", label="Shift+Tab")
    run.note("Shift+Tab stops: " + "; ".join(f"[{n.get('role')}] {n.get('name')!r}"
                                             for n in landed))
    with run.act("the formatting buttons in the Tab order",
                 [fact("a formatting toggle (Bold) is among the Shift+Tab stops",
                       lambda: (any("Bold" in (n.get("name") or "") for n in landed),
                                ["stops: " + "; ".join(f"[{n.get('role')}] {n.get('name')!r}"
                                                       for n in landed)]))],
                 should="the keyboard reaches the formatting buttons", record=0.2):
        pass


SCENARIOS = [
    Scenario("text-editor-shift-tab", "rich-text-editor", body_editor_shift_tab,
             "Shift+Tab backwards through the formatting toolbar"),
    Scenario("text-editor-tab-in", "rich-text-editor", body_editor_tab_in,
             "Tab into the rich text editor, read, type, Ctrl+Tab out and back"),
    Scenario("text-editor-caret", "rich-text-editor", body_editor_caret,
             "caret moves by character, word, line, paragraph break and document"),
    Scenario("text-editor-keys-unreported", "rich-text-editor", body_editor_keys_unreported,
             "caret moves with no key ever reported to Orca, as AccessKit ships"),
    Scenario("text-editor-select", "rich-text-editor", body_editor_select,
             "Shift+arrows, Shift+End, Shift+Down, Ctrl+A in the editor"),
    Scenario("text-editor-typing", "rich-text-editor", body_editor_typing,
             "Enter, typing, Backspace, Ctrl+Backspace, undo and paste in the editor"),
    Scenario("text-editor-format", "rich-text-editor", body_editor_format,
             "text attributes, links, Ctrl+B and the toolbar's toggles"),
    Scenario("text-editor-structure", "rich-text-editor", body_editor_structure,
             "headings, lists, table, blockquote, links and code: tree and line reading"),
    Scenario("text-viewer", "rich-text-viewer", body_viewer,
             "the read-only viewer: tree, Tab in, reading by line and word, selecting"),
    Scenario("text-ime", "ime-playground", body_ime_fields,
             "IME composition replayed into a TextInput, a PasswordField and a rich editor"),
]
