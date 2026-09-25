# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's scenarios for password-field, beside the sweep's `password.py`.

* verify-password-caps-arrive: Caps Lock turned on in Username (no warning
  there), then Tab into Password. The Password row's warning is added in the
  same update as the focus move, for the first time in the session, so the
  defunct drop cannot be the cause of anything that goes wrong.
* verify-password-empty-edits: what the bus reports for the first character
  typed into an empty entry and for the deletion that empties it again.
* verify-password-stale-confirm: a cross-field validation (Confirm checks
  Password) whose message outlives the mismatch once Password is changed, and
  whether anything tells a reader that Sign in became available.
"""

from __future__ import annotations

import time

from reader_lib.checks import announced, custom, focused, not_said, said
from reader_lib.scenario import Scenario
from scenarios.password import inserted, node_has, probe_has, probe_into

CAPS = "Caps Lock is on"


def _deleted(role: str, text: str):
    def run(act):
        got = "".join(e.get("text") or "" for e in act.events
                      if e["type"].startswith("object:text-changed:delete")
                      and e["source"].get("role") == role)
        return got == text, [f"deleted text from [{role}]: {got!r}"]
    return custom(f"[{role}] reports {text!r} deleted", run)


def _no_event_from(name: str):
    def run(act):
        found = [f"{e['type']} {e.get('detail1')} [{e['source'].get('role')}] "
                 f"{e['source'].get('name')!r}" for e in act.events
                 if (e.get("source") or {}).get("name") == name
                 and not e["type"].startswith("object:bounds")]
        return not found, found or [f"no event from {name!r}"]
    return custom(f"no event from {name!r} (nothing tells a reader it changed)", run)


def caps_arrive(run):
    try:
        with run.act("Caps Lock on in Username (a plain entry: no warning expected)",
                     [custom("no announcement in the act",
                             lambda act: (not [e for e in act.events
                                               if e["type"] == "object:announcement"],
                                          [e.get("text") for e in act.events
                                           if e["type"] == "object:announcement"]
                                          or ["none"]))],
                     should="nothing is said; the entry is not a password field"):
            run.key("CapsLock")
        with run.act("Tab to Password with Caps Lock on (the warning's first showing)",
                     [focused(role="password text", name="Password"), announced(CAPS),
                      said(CAPS)],
                     should="arriving in a password field with Caps Lock on, the reader is "
                            "told it is on"):
            run.key("Tab")
        with run.act("the tree with the warning showing", [node_has("status bar", CAPS)],
                     should="the warning is in the tree", tree=True):
            pass
    finally:
        with run.act("Caps Lock off (restore)", should="scene restore, not judged"):
            run.key("CapsLock")


def empty_edits(run):
    probes: dict = {}
    with run.act("type 'a' into the empty Username", [inserted("entry", "a")],
                 should="the first character is reported inserted"):
        run.type("a")
    with run.act("type 'b'", [inserted("entry", "b")],
                 should="the second character is reported inserted"):
        run.type("b")
    with run.act("BackSpace (Username 'ab' to 'a')", [_deleted("entry", "b")],
                 should="the deletion is reported"):
        run.key("BackSpace")
    with run.act("BackSpace (Username 'a' to empty)",
                 [_deleted("entry", "a"), probe_has(probes, "emptied", interface="Text")],
                 should="the deletion that empties the field is reported, and the empty "
                        "entry still supports Text"):
        run.key("BackSpace")
        time.sleep(0.5)
        probe_into(run, probes, "emptied", "entry", "")
    with run.act("type 'c' into the emptied Username", [inserted("entry", "c")],
                 should="the first character is reported inserted"):
        run.type("c")


def stale_confirm(run):
    with run.act("Tab to Password, type 'abcdefgh'", [focused(role="password text",
                                                              name="Password")],
                 should="scene setting"):
        run.key("Tab")
        time.sleep(0.5)
        run.type("abcdefgh")
    with run.act("Tab twice to Confirm, type 'abcdefgX'",
                 [focused(role="password text", name="Confirm password")],
                 should="scene setting"):
        run.key("Tab")
        time.sleep(0.5)
        run.key("Tab")
        time.sleep(0.5)
        run.type("abcdefgX")
    with run.act("Tab away from Confirm (mismatch)",
                 [announced("Passwords don't match")],
                 should="the mismatch is flagged (its speech is judged by the sweep)"):
        run.key("Tab")
    with run.act("Shift+Tab three times to Password",
                 [focused(role="password text", name="Password")],
                 should="scene setting"):
        run.key("Shift+Tab")
        time.sleep(0.5)
        run.key("Shift+Tab")
        time.sleep(0.5)
        run.key("Shift+Tab")
    with run.act("make Password 'abcdefgX' (now equal to Confirm)",
                 [_no_event_from("Sign in")],
                 should="Sign in becomes available; a reader should be told (a state "
                        "change on the button)"):
        run.key("End")
        run.key("BackSpace")
        run.type("X")
        time.sleep(0.4)
    with run.act("Tab twice to Confirm",
                 [focused(role="password text", name="Confirm password"),
                  not_said("Passwords don't match"),
                  node_has("password text", "Confirm password", not_desc="don't match")],
                 should="the passwords match now, so the reader does not hear a stale "
                        "'Passwords don't match'", tree=True):
        run.key("Tab")
        time.sleep(0.5)
        run.key("Tab")
    with run.act("Tab twice to Sign in", [focused(role="push button", name="Sign in")],
                 should="Sign in is reachable: the form is submittable"):
        run.key("Tab")
        time.sleep(0.5)
        run.key("Tab")


SCENARIOS = [
    Scenario("verify-password-caps-arrive", "password-field", caps_arrive,
             "Caps Lock on in Username, then Tab into Password: the first warning"),
    Scenario("verify-password-empty-edits", "password-field", empty_edits,
             "first insertion into, and last deletion from, an empty entry"),
    Scenario("verify-password-stale-confirm", "password-field", stale_confirm,
             "Confirm's mismatch message after Password is changed to match"),
]
