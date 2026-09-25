# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""password-field: what a reader gets from a `PasswordField`.

The example (`examples/password_field/src/main.rs`) is a sign-in card
(Username `TextInput`, Password and Confirm password `PasswordField`s with
validators, a Sign in button enabled only when both match and are 8+
characters) beside an echo-mode showcase: five `PasswordField`s pre-filled
with "hunter2" (Masked, RevealWhileTyping, NoEcho, RevealMode::Hold,
AtRevealPolicy::AlwaysProtected).

Where each piece of a reader's experience is produced:

* the field node: `TextInputField::accessibility`
  (`crates/teksilo-widgets/src/primitives/text_input_field/widget_impl.rs`).
  Protected, it is `Role::PasswordInput` with a bullet string as its *value*
  and no text runs; revealed under `SwapRole` it becomes `Role::TextInput`
  with the plaintext as value and runs.
* the reveal toggle: `IconButton::visibility_toggle` with the label
  "Toggle password visibility" (`password_field.rs`, `RevealMode::Toggle`);
  `RevealMode::Hold` builds a `MinSize` with a pointer handler and
  `access_role(Role::Button)` instead.
* the Caps Lock warning: a `TextWidget` "⇪" with `Role::Status`,
  `Live::Polite`, label "Caps Lock is on", shown while Caps Lock is on AND
  focus is within the field's row (`visible_when`). Caps Lock itself is
  tracked by counting key presses (`teksilo-app/src/app.rs`, the
  `KeyboardInput` arm), not read from the OS.
* validation: a `ValidationStrip` (`Role::Status`, assertive while invalid)
  the field is `described_by`.

Focus is moved with real keys wherever the Tab order reaches (it starts on
Username at launch); `grab_focus` is used only for the showcase fields, which
is what a reader's own focus request does.

The listener's tree comes from one long-lived libatspi client, which caches a
node's interface list for good (`atspi_accessible_get_interfaces`), and
AccessKit sends no cache signal when a node's interfaces change on an update
(`accesskit_atspi_common` `adapter.rs` `node_updated` re-registers them on
D-Bus only). So where an act swaps a field's role, `fresh_probe` asks a new
libatspi process, which has cached nothing.
"""

from __future__ import annotations

import json
import subprocess
import sys
import time

from reader_lib.checks import (_focus_node, _is_focus, announced, custom, event, focused,
                               no_event, not_announced, not_in_tree, not_said, said)
from reader_lib.orca import utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

TOGGLE = "Toggle password visibility"
CAPS = "Caps Lock is on"
SECRET = "q7zx9wkp"  # 8 characters: valid, so leaving the field raises nothing


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _walk(tree):
    if not tree:
        return
    stack = [tree]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def _node(tree, role=None, name=None, nth=0):
    for n in _walk(tree):
        if (role is None or n.get("role") == role) and (name is None or n.get("name") == name):
            if nth == 0:
                return n
            nth -= 1
    return None


def scene_focus(run, role: str, name: str) -> None:
    """Scene setting inside an act of its own: a reader's own focus request.
    Outside an act, Orca's speech for it would be matched (by event type) to
    the next act's first focus event and credited to that act."""
    run.wait_for(role=role, name=name)
    with run.act(f"scene: focus [{role}] {name!r} through AT-SPI",
                 should="scene setting, not judged"):
        run.grab_focus(role=role, name=name)


def tab_to(run, role: str, name: str, limit: int = 24, chord: str = "Tab") -> dict:
    """Scene setting: press `chord` until the bus reports focus on [role] name."""
    for _ in range(limit):
        node = run.last_focus()
        if node.get("role") == role and node.get("name") == name:
            return node
        run.key(chord)
        time.sleep(0.4)
    node = run.last_focus()
    if node.get("role") == role and node.get("name") == name:
        return node
    raise RunError(f"{limit} x {chord} never focused [{role}] {name!r}; last focus {node}")


_PROBE = r"""
import json, sys, gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
pid, role, name, nth = int(sys.argv[1]), sys.argv[2], sys.argv[3], int(sys.argv[4])
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
        if n.get_role_name() == role and (n.get_name() or "") == name:
            if nth == 0:
                rec = {"found": True, "role": n.get_role_name(), "name": n.get_name(),
                       "interfaces": sorted(n.get_interfaces()),
                       "states": sorted(s.value_nick for s in n.get_state_set().get_states()),
                       "description": n.get_description()}
                try:
                    cnt = Atspi.Text.get_character_count(n)
                    rec["text"] = Atspi.Text.get_text(n, 0, cnt)
                except Exception as exc:
                    rec["text_error"] = type(exc).__name__ + ": " + str(exc)[:120]
                out = rec
                break
            nth -= 1
        for i in reversed(range(n.get_child_count())):
            ch = n.get_child_at_index(i)
            if ch is not None:
                stack.append(ch)
    except Exception:
        continue
print(json.dumps(out, ensure_ascii=False))
"""


def fresh_probe(run, role: str, name: str, nth: int = 0) -> dict:
    """Ask a new libatspi process, with nothing cached, what [role] name is now."""
    assert run.app is not None
    done = subprocess.run([sys.executable, "-W", "ignore", "-c", _PROBE, str(run.app.pid),
                           role, name, str(nth)], capture_output=True, text=True, timeout=60)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1])
    except (ValueError, IndexError):
        return {"found": False, "error": done.stderr[-400:]}


def probe_into(run, store: dict, key: str, role: str, name: str, nth: int = 0) -> None:
    store[key] = fresh_probe(run, role, name, nth)
    run.note(f"fresh probe {key}: {store[key]}")


def probe_has(store: dict, key: str, *, interface: str | None = None,
              no_interface: str | None = None, text: str | None = None,
              role: str | None = None):
    want = ", ".join(x for x in (role and f"role {role}",
                                 interface and f"interface {interface}",
                                 no_interface and f"no interface {no_interface}",
                                 text is not None and f"text {text!r}") if x)

    def check(_act):
        rec = store.get(key) or {}
        if not rec.get("found"):
            return False, [f"fresh probe found nothing: {rec}"]
        ok = True
        if role and rec.get("role") != role:
            ok = False
        if interface and interface not in rec.get("interfaces", []):
            ok = False
        if no_interface and no_interface in rec.get("interfaces", []):
            ok = False
        if text is not None and rec.get("text") != text:
            ok = False
        return ok, [f"fresh libatspi client: {rec}"]
    return custom(f"a fresh AT-SPI client sees {want}", check)


def nothing_leaks(secret: str, tree: bool = False):
    """No event the act emitted, and (with `tree`) no node after it, carries
    `secret` in its text, name, description or value."""
    def run(act):
        found = []
        for e in act.events:
            if secret in (e.get("text") or ""):
                found.append(f"event {e['type']} [{e['source'].get('role')}] "
                             f"{e['source'].get('name')!r} text={e.get('text')!r}")
            if secret in (e.get("source", {}).get("name") or ""):
                found.append(f"event {e['type']} source name {e['source'].get('name')!r}")
        if tree:
            for n in _walk(act.tree):
                blobs = [n.get("name"), n.get("description"),
                         (n.get("text") or {}).get("text"), str(n.get("value") or "")]
                if any(secret in (b or "") for b in blobs):
                    found.append(f"tree [{n.get('role')}] {n.get('name')!r} "
                                 f"text={n.get('text')} desc={n.get('description')!r}")
        return not found, found or [f"{secret!r} appears in no event"
                                    + (" and no node" if tree else "")]
    return custom(f"no event{' or node' if tree else ''} carries {secret!r}", run,
                  needs_tree=tree)


def node_has(role: str, name: str, *, interface: str | None = None,
             state: str | None = None, no_state: str | None = None,
             text: str | None = None, not_desc: str | None = None,
             desc: str | None = None, nth: int = 0):
    """After the act, [role] name has the interface / state / text asked for."""
    want = ", ".join(x for x in (interface and f"interface {interface}",
                                 state and f"state {state}",
                                 no_state and f"no state {no_state}",
                                 text is not None and f"text {text!r}",
                                 desc and f"description containing {desc!r}",
                                 not_desc and f"no description containing {not_desc!r}") if x)

    def run(act):
        n = _node(act.tree, role, name, nth)
        if n is None:
            return False, [f"no [{role}] {name!r} in the tree after the act"]
        ok = True
        if interface and interface not in n.get("interfaces", []):
            ok = False
        if state and state not in n.get("states", []):
            ok = False
        if no_state and no_state in n.get("states", []):
            ok = False
        if text is not None and (n.get("text") or {}).get("text") != text:
            ok = False
        if desc and desc not in (n.get("description") or ""):
            ok = False
        if not_desc and not_desc in (n.get("description") or ""):
            ok = False
        return ok, [f"[{n.get('role')}] {n.get('name')!r} interfaces={n.get('interfaces')} "
                    f"states={n.get('states')} text={n.get('text')} "
                    f"description={n.get('description')!r} actions={n.get('actions')}"]
    return custom(f"[{role}] {name!r} has {want}", run, needs_tree=True)


def orca_spoke():
    """Orca said anything at all in the act."""
    def run(act):
        us = utterances(act.orca)
        return bool(us), [f"Orca said: {u.text!r}" for u in us] or ["Orca said nothing"]
    return custom("Orca says something", run, needs_orca=True)


def orca_says_content():
    """Orca's speech in the act mentions the field's masked content."""
    marks = ("•", "●", "bullet", "star", "circle", "dot", "*")

    def run(act):
        us = utterances(act.orca)
        ok = any(any(m in u.text.lower() for m in marks) for u in us)
        return ok, [f"Orca said: {u.text!r}" for u in us] or ["Orca said nothing"]
    return custom("Orca's speech says the field holds masked characters", run,
                  needs_orca=True)


def last_focus_is(role: str, name: str):
    """The act ENDS with focus on [role] name."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change in the act"]
        last = _focus_node(moves[-1])
        return (last.get("role") == role and last.get("name") == name,
                [f"last focus [{last.get('role')}] {last.get('name')!r}"])
    return custom(f"the act ends with focus on [{role}] {name!r}", run)


def inserted(role: str, text: str):
    """A text-changed:insert from a [role] node carrying exactly `text`
    (concatenated over the act)."""
    def run(act):
        got = "".join(e.get("text") or "" for e in act.events
                      if e["type"].startswith("object:text-changed:insert")
                      and e["source"].get("role") == role)
        return got == text, [f"inserted text from [{role}]: {got!r}"]
    return custom(f"[{role}] reports {text!r} inserted", run)


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------


def typing(run):
    """Typing into a masked field: what a reader hears per keystroke, and what
    the field says about its content afterwards. Username is the control: the
    same keys into a plain entry."""
    probes: dict = {}
    with run.act("the empty Username entry, from a fresh AT-SPI client",
                 [probe_has(probes, "empty-username", interface="Text")],
                 should="an empty entry already supports Text (character count 0), so its "
                        "first insertion can be reported"):
        probe_into(run, probes, "empty-username", "entry", "")
    with run.act("type 'bob' into Username (the control: a plain entry)",
                 [event("object:text-changed:insert", role="entry"),
                  inserted("entry", "bob")],
                 should="a plain entry reports each keystroke on the bus"):
        run.type("bob")
    with run.act("Tab from Username to Password",
                 [focused(role="password text", name="Password"), said("Password"),
                  said("password text")],
                 should="focus lands on the password field; the reader hears its name and role"):
        run.key("Tab")
    with run.act(f"type '{SECRET}' into Password",
                 [event("object:text-changed:insert", role="password text"),
                  orca_spoke(), nothing_leaks(SECRET)],
                 should="each keystroke is echoed as a mask character (a GTK password entry "
                        "reports '●' inserted and Orca speaks it; Orca suppresses its own "
                        "key echo in password text, default.py:2702), never the letter"):
        run.type(SECRET)
    with run.act("the Password field after typing",
                 [node_has("password text", "Password", interface="Text"),
                  probe_has(probes, "after-typing", interface="Text"),
                  nothing_leaks(SECRET, tree=True)],
                 should="the field's content is readable as eight mask characters, so a "
                        "reader can tell it is filled and how long it is", tree=True):
        probe_into(run, probes, "after-typing", "password text", "Password")
    with run.act("Shift+Tab to Username and Tab back to Password",
                 [last_focus_is("password text", "Password"), orca_says_content(),
                  not_said(SECRET)],
                 should="arriving in a filled password field, the reader hears that it "
                        "holds masked characters (Orca's password-text format reads "
                        "currentLineText, formatting.py:405-407)"):
        run.key("Shift+Tab")
        time.sleep(0.8)
        run.key("Tab")
    with run.act("Backspace in Password",
                 [event("object:text-changed:delete", role="password text"), orca_spoke()],
                 should="a deletion is echoed"):
        run.key("BackSpace")


def reveal(run):
    """The reveal toggle under the default AtRevealPolicy::SwapRole."""
    tab_to(run, "password text", "Password")
    with run.act(f"type '{SECRET}' into Password", [nothing_leaks(SECRET)],
                 should="nothing of the secret reaches the bus while masked"):
        run.type(SECRET)
    with run.act("Tab to the reveal toggle",
                 [focused(role="toggle button", name=TOGGLE), said(TOGGLE),
                  said("not pressed"), not_announced("characters")],
                 should="the reader hears the toggle, its role and that it is off; an "
                        "8-character password raises no validation message"):
        run.key("Tab")
    probes: dict = {}
    with run.act("Space on the toggle: reveal",
                 [event("object:state-changed:pressed", role="toggle button", detail1=1),
                  said("pressed"),
                  event("object:property-change:accessible-role", role="entry"),
                  # Not the listener's tree: its libatspi cache still holds the
                  # masked node's interfaces (no Text) after the role swap.
                  probe_has(probes, "revealed", role="entry", interface="Text", text=SECRET)],
                 should="the toggle reads as pressed; under SwapRole the field becomes a "
                        "plain entry exposing the password, like the web's type=text swap",
                 tree=True):
        run.key("space")
        time.sleep(0.8)
        probe_into(run, probes, "revealed", "entry", "Password")
    with run.act("Shift+Tab back to the revealed field",
                 [focused(role="entry", name="Password"), said(SECRET)],
                 should="the reader hears the revealed password"):
        run.key("Shift+Tab")
    with run.act("type 'Z' at the end of the revealed field",
                 [inserted("entry", "Z")],
                 should="revealed, the field reports the typed character like any entry"):
        run.key("End")
        run.type("Z")
    with run.act("Tab to the toggle, Space: hide again",
                 [event("object:state-changed:pressed", role="toggle button", detail1=0),
                  said("not pressed"),
                  node_has("password text", "Password"), not_in_tree("entry", "Password"),
                  probe_has(probes, "hidden", role="password text", no_interface="Text"),
                  nothing_leaks(SECRET + "Z", tree=True)],
                 should="the toggle reads as not pressed and the field is a password field "
                        "again", tree=True):
        run.key("Tab")
        time.sleep(0.5)
        run.key("space")
        time.sleep(0.8)
        probe_into(run, probes, "hidden", "password text", "Password")
    with run.act("Shift+Tab back to the masked field",
                 [focused(role="password text", name="Password"), not_said(SECRET)],
                 should="the reader hears a password field and not the password"):
        run.key("Shift+Tab")


def protected(run):
    """AtRevealPolicy::AlwaysProtected: revealing is visual only."""
    scene_focus(run, "password text", "Always protected")
    with run.act("Tab to the Always protected field's toggle",
                 [focused(role="toggle button", name=TOGGLE), said("not pressed")],
                 should="the reader hears the toggle and that it is off"):
        run.key("Tab")
    with run.act("Space: reveal (visually)",
                 [event("object:state-changed:pressed", role="toggle button", detail1=1),
                  said("pressed"), node_has("password text", "Always protected"),
                  not_in_tree("entry", "Always protected"), nothing_leaks("hunter2", tree=True)],
                 should="the toggle reads as pressed; the field stays a password field and "
                        "the password never reaches the bus", tree=True):
        run.key("space")
    with run.act("Shift+Tab back to the field",
                 [focused(role="password text", name="Always protected"),
                  not_said("hunter2"), nothing_leaks("hunter2")],
                 should="the reader hears a password field, not the password; nothing tells "
                        "them the field is shown in clear on screen"):
        run.key("Shift+Tab")
    with run.act("Tab, Space: hide", [event("object:state-changed:pressed", detail1=0)],
                 should="the toggle reads as not pressed"):
        run.key("Tab")
        time.sleep(0.5)
        run.key("space")


def echo_modes(run):
    """RevealWhileTyping and NoEcho: visual modes that must not reach AT."""
    run.wait_for(role="password text", name="Reveal while typing")
    with run.act("focus Reveal while typing (a reader's own focus request)",
                 [focused(role="password text", name="Reveal while typing"),
                  not_said("hunter2"), nothing_leaks("hunter2", tree=True)],
                 should="the field is shown in clear to the eye while focused, but a "
                        "reader gets a password field and not the password", tree=True):
        run.grab_focus(role="password text", name="Reveal while typing")
    with run.act("type 'x' into Reveal while typing",
                 [nothing_leaks("hunter2", tree=True), nothing_leaks("x"),
                  event("object:text-changed:insert", role="password text")],
                 should="the keystroke is echoed masked", tree=True):
        run.key("End")
        run.type("x")
    with run.act("focus No echo (a reader's own focus request)",
                 [focused(role="password text", name="No echo"), not_said("hunter2"),
                  nothing_leaks("hunter2", tree=True)],
                 should="no echo hides even the length", tree=True):
        run.grab_focus(role="password text", name="No echo")
    with run.act("type 'y' into No echo", [nothing_leaks("y"), nothing_leaks("hunter2")],
                 should="nothing about the secret reaches the bus"):
        run.type("y")


def caps_lock(run):
    """The Caps Lock warning: a polite live `Role::Status` shown while Caps
    Lock is on and focus is in the field's row."""
    tab_to(run, "password text", "Password")
    try:
        with run.act("Caps Lock on in the Password field",
                     [announced(CAPS), said(CAPS)],
                     should="the reader hears that Caps Lock is on"):
            run.key("CapsLock")
        with run.act("Caps Lock off",
                     should="the warning goes (nothing is required to be said)"):
            run.key("CapsLock")
        with run.act("Caps Lock on again", [announced(CAPS), said(CAPS)],
                     should="the reader hears it a second time"):
            run.key("CapsLock")
        with run.act("Tab to the reveal toggle (Caps Lock still on)",
                     [focused(role="toggle button", name=TOGGLE), said(TOGGLE),
                      no_event("object:state-changed:defunct", role="status bar")],
                     should="focus stays within the field's row, the warning stays; the "
                            "reader hears the toggle"):
            run.key("Tab")
        with run.act("Tab to Confirm password with Caps Lock on",
                     [focused(role="password text", name="Confirm password"),
                      announced(CAPS), said(CAPS)],
                     should="arriving in a password field with Caps Lock on, the reader is "
                            "told Caps Lock is on"):
            run.key("Tab")
        with run.act("Shift+Tab twice back to Password with Caps Lock on",
                     [last_focus_is("password text", "Password"), said(CAPS)],
                     should="the reader is told again on arriving in Password"):
            run.key("Shift+Tab")
            time.sleep(0.6)
            run.key("Shift+Tab")
        with run.act("the tree with Caps Lock on in Password",
                     [node_has("status bar", CAPS)],
                     should="the warning is in the tree while it shows", tree=True):
            pass
    finally:
        # The private session is shared by every scenario of one invocation:
        # never leave Caps Lock on behind.
        with run.act("Caps Lock off (restore)", should="the warning goes"):
            run.key("CapsLock")


def caps_leave_on(run):
    """First half of the Caps Lock state test: turn Caps Lock on and leave it
    on when the application exits. Must be followed, in the same invocation,
    by password-caps-lock-start-on, which turns it off again.

    The first character typed into an empty entry is not reported on the bus
    (see password-typing), so the entry's text is read with a fresh client."""
    probes: dict = {}
    with run.act("Caps Lock on in Username, then type 'q'",
                 [probe_has(probes, "username", text="Q")],
                 should="the lock is on in the session: 'q' types 'Q'"):
        run.key("CapsLock")
        time.sleep(0.4)
        run.type("q")
        time.sleep(0.5)
        probe_into(run, probes, "username", "entry", "")


def caps_start_on(run):
    """Second half: the application starts while Caps Lock is already on (as
    when a user turned it on before opening the app, or in another window).
    Teksilo counts Caps Lock presses from `false` (`teksilo-app/src/app.rs`,
    `caps_lock_active = !caps_lock_active`), so it believes it is off."""
    probes: dict = {}
    with run.act("type 'q' into Username (Caps Lock left on by the last run)",
                 [probe_has(probes, "username-on", text="Q")],
                 should="the session's lock is on at launch: 'q' types 'Q'"):
        run.type("q")
        time.sleep(0.5)
        probe_into(run, probes, "username-on", "entry", "")
    with run.act("Tab to Password with Caps Lock on",
                 [focused(role="password text", name="Password"), announced(CAPS),
                  said(CAPS)],
                 should="arriving in a password field with Caps Lock on, the reader "
                        "is told it is on"):
        run.key("Tab")
    with run.act("press Caps Lock (it is now really off)",
                 [not_announced(CAPS), not_said(CAPS)],
                 should="Caps Lock goes off; nothing claims it is on"):
        run.key("CapsLock")
    with run.act("Shift+Tab to Username and type 'q' (proves the lock is off)",
                 [probe_has(probes, "username-off", text="Qq")],
                 should="the lock is off: 'q' types 'q'"):
        run.key("Shift+Tab")
        time.sleep(0.6)
        run.key("End")
        run.type("q")
        time.sleep(0.5)
        probe_into(run, probes, "username-off", "entry", "")


def hold(run):
    """RevealMode::Hold: press-and-hold on a pointer-only 'button'."""
    run.wait_for(role="password text", name="Hold to reveal")
    with run.act("the Hold to reveal button as the tree gives it",
                 [node_has("push button", TOGGLE, interface="Action"),
                  node_has("push button", TOGGLE, state="focusable")],
                 should="a control announced as a button can be operated: it has an "
                        "action and can take focus", tree=True):
        pass
    scene_focus(run, "password text", "Hold to reveal")
    with run.act("Tab from the Hold to reveal field",
                 [focused(role="push button", name=TOGGLE)],
                 should="the reveal button is the next Tab stop, as the other fields' "
                        "toggles are"):
        run.key("Tab")
    outcome: dict = {}
    with run.act("activate the Hold button through AT-SPI",
                 [custom("the button offers an action a reader can invoke",
                         lambda act: (outcome.get("ok", False),
                                      [outcome.get("detail", "not tried")]))],
                 should="a screen reader's own activation reveals the field"):
        try:
            reply = run.action(0, role="push button", name=TOGGLE)
            outcome.update(ok=bool(reply.get("done")), detail=f"{reply}")
        except RunError as exc:
            outcome.update(ok=False, detail=f"AT-SPI action failed: {exc}")


def validation(run):
    """The validators: the message a reader hears on leaving, and on coming back."""
    tab_to(run, "password text", "Password")
    with run.act("type 'abc' into Password", [nothing_leaks("abc")],
                 should="nothing leaks"):
        run.type("abc")
    with run.act("Tab away from the short password",
                 [said("Use at least 8 characters"),
                  focused(role="toggle button", name=TOGGLE)],
                 should="the reader hears the validation message, whole"):
        run.key("Tab")
    with run.act("Shift+Tab back to Password",
                 [focused(role="password text", name="Password"),
                  said("Use at least 8 characters"),
                  node_has("password text", "Password", desc="Use at least 8 characters"),
                  node_has("password text", "Password", state="invalid-entry")],
                 should="arriving back, the reader hears the message and that the field is "
                        "invalid", tree=True):
        run.key("Shift+Tab")
    with run.act("type 'defgh' (8 characters now), Tab away",
                 [not_said("Use at least 8 characters")],
                 should="the error is gone and is not repeated"):
        run.key("End")
        run.type("defgh")
        time.sleep(0.3)
        run.key("Tab")
    with run.act("Shift+Tab back to the now valid Password",
                 [focused(role="password text", name="Password"),
                  not_said("Use at least 8 characters"),
                  node_has("password text", "Password", not_desc="Use at least",
                           no_state="invalid-entry")],
                 should="the stale message is not read any more", tree=True):
        run.key("Shift+Tab")
    tab_to(run, "password text", "Confirm password")
    with run.act("type 'abcdefgX' into Confirm, Tab away",
                 [said("Passwords don't match")],
                 should="the reader hears that the passwords differ, whole"):
        run.type("abcdefgX")
        time.sleep(0.3)
        run.key("Tab")
    with run.act("Shift+Tab back to Confirm",
                 [focused(role="password text", name="Confirm password"),
                  said("Passwords don't match")],
                 should="arriving back, the reader hears the message"):
        run.key("Shift+Tab")
    with run.act("fix Confirm (Backspace, 'h'), Tab to its toggle",
                 [focused(role="toggle button", name=TOGGLE),
                  not_said("Passwords don't match")],
                 should="the error clears"):
        run.key("End")
        run.key("BackSpace")
        run.type("h")
        time.sleep(0.3)
        run.key("Tab")
    with run.act("Tab to Sign in (now enabled)",
                 [focused(role="push button", name="Sign in"), said("Sign in")],
                 should="Sign in is reachable once both passwords match"):
        run.key("Tab")


def blur_empty(run):
    """Tab through the empty sign-in form: the validator runs on a field the
    user only passed through, and its message races the focus move."""
    with run.act("Tab from Username to the empty Password",
                 [focused(role="password text", name="Password")],
                 should="focus lands on Password"):
        run.key("Tab")
    with run.act("Tab on from the empty, untouched Password",
                 [focused(role="toggle button", name=TOGGLE),
                  not_announced("Use at least 8 characters")],
                 should="passing through an empty field does not flag it (the user has not "
                        "entered anything yet)"):
        run.key("Tab")
    with run.act("Shift+Tab back to Password",
                 [focused(role="password text", name="Password")],
                 should="what the reader hears on coming back"):
        run.key("Shift+Tab")


def copy(run):
    """Copy suppression: a masked field's text never reaches the clipboard.

    Every paste lands in the empty Username entry, whose first insertion is
    never reported on the bus (see password-typing), so what was pasted is
    read back with a fresh AT-SPI client."""
    probes: dict = {}

    def username_text(key: str, want: str | None):
        def check(_act):
            rec = probes.get(key) or {}
            got = rec.get("text") or ""
            ok = (got == want) if want is not None else ("hunter2" not in got)
            return ok, [f"Username holds {got!r} (fresh client: {rec})"]
        return custom(f"Username holds {want!r}" if want is not None
                      else "Username does not hold 'hunter2'", check)

    scene_focus(run, "password text", "Masked")
    with run.act("Ctrl+A, Ctrl+C in the masked field", [nothing_leaks("hunter2")],
                 should="nothing is copied"):
        run.key("Ctrl+a")
        run.key("Ctrl+c")
    scene_focus(run, "entry", "")
    with run.act("Ctrl+V into Username",
                 [nothing_leaks("hunter2", tree=True), username_text("masked", None)],
                 should="the masked password was not on the clipboard", tree=True):
        run.key("Ctrl+v")
        time.sleep(0.5)
        probe_into(run, probes, "masked", "entry", "")
    # Reveal the Masked field with its own toggle (the third one in tree order).
    with run.act("scene: click the Masked field's toggle through AT-SPI",
                 should="scene setting, not judged"):
        run.action("click", role="toggle button", name=TOGGLE, nth=2)
    scene_focus(run, "entry", "Masked")
    with run.act("Ctrl+A, Ctrl+C in the revealed field", should="copy is allowed once revealed"):
        run.key("Ctrl+a")
        run.key("Ctrl+c")
    scene_focus(run, "entry", "")
    with run.act("Ctrl+V into Username after copying the revealed field",
                 [username_text("revealed", "hunter2")],
                 should="the revealed password was copied (proves the clipboard works in "
                        "this session, so the masked result above means something)"):
        run.key("Ctrl+v")
        time.sleep(0.5)
        probe_into(run, probes, "revealed", "entry", "")


def sign_in(run):
    """The Sign in button while it is disabled (both passwords empty)."""
    with run.act("the disabled Sign in button as the tree gives it",
                 [node_has("push button", "Sign in", no_state="enabled"),
                  node_has("push button", "Sign in", no_state="sensitive")],
                 should="a disabled button reads as unavailable (not enabled / not "
                        "sensitive), so a reader reviewing the form hears 'grayed'",
                 tree=True):
        pass
    with run.act("a reader's own focus request on the disabled Sign in",
                 [custom("focus does not land on a disabled button",
                         lambda act: (not any(_is_focus(e) and _focus_node(e).get("name")
                                              == "Sign in" for e in act.events),
                                      [f"{e['type']} {_focus_node(e)}" for e in act.events
                                       if _is_focus(e)] or ["no focus change"]))],
                 should="the reader's focus request is refused or harmless"):
        try:
            run.grab_focus(role="push button", name="Sign in")
        except RunError as exc:
            run.note(f"grab_focus on Sign in: {exc}")


SCENARIOS = [
    Scenario("password-typing", "password-field", typing,
             "type into a masked field: per-keystroke echo and the field's content"),
    Scenario("password-reveal", "password-field", reveal,
             "reveal toggle under SwapRole: pressed state, role swap, what is read"),
    Scenario("password-protected", "password-field", protected,
             "reveal toggle under AlwaysProtected: stays a password field"),
    Scenario("password-echo-modes", "password-field", echo_modes,
             "RevealWhileTyping and NoEcho: nothing reaches the bus"),
    Scenario("password-caps-lock", "password-field", caps_lock,
             "the Caps Lock warning, on and off, across fields"),
    Scenario("password-caps-lock-leave-on", "password-field", caps_leave_on,
             "turn Caps Lock on and leave it on (run before password-caps-lock-start-on)"),
    Scenario("password-caps-lock-start-on", "password-field", caps_start_on,
             "the app starts with Caps Lock already on (after password-caps-lock-leave-on)"),
    Scenario("password-hold", "password-field", hold,
             "RevealMode::Hold: can a reader find and operate the button"),
    Scenario("password-validation", "password-field", validation,
             "the validators' messages on leaving and on coming back"),
    Scenario("password-blur-empty", "password-field", blur_empty,
             "Tab through the empty form: a validation message on an untouched field"),
    Scenario("password-copy", "password-field", copy,
             "copy suppression while masked, copy allowed when revealed"),
    Scenario("password-sign-in", "password-field", sign_in,
             "the disabled Sign in button as a reader meets it"),
]
