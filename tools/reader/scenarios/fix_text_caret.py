# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The masked password field once a reader can read it (the text-caret fix).

A masked `TextInputField` now publishes its mask, one echo character per
character, as its text runs: AT-SPI gives it a Text interface, a keystroke or a
deletion reaches the bus as a mask character, and a caret move as a caret
move. The sweep's `password-reveal` still states the old model for the moment
a revealed field is hidden again (a fresh client sees no Text interface); this
states the new one: nothing of the password crosses the bus as it is revealed
or hidden, and the mask is the field's text again.

* fix-text-caret-password: type into Password, move the caret, delete, reveal,
  hide again, come back and type.
"""

from __future__ import annotations

import time

from reader_lib.checks import custom, event, focused, not_said
from reader_lib.scenario import Scenario
from scenarios.password import (SECRET, TOGGLE, inserted, nothing_leaks, orca_says_content,
                                orca_spoke, probe_has, probe_into, tab_to)

MASK = "•"


def _deleted(role: str, text: str):
    """A text-changed:delete from a [role] node carrying exactly `text`."""
    def run(act):
        got = "".join(e.get("text") or "" for e in act.events
                      if e["type"].startswith("object:text-changed:delete")
                      and e["source"].get("role") == role)
        return got == text, [f"deleted text from [{role}]: {got!r}"]
    return custom(f"[{role}] reports {text!r} deleted", run)


def password(run):
    kept = SECRET[:-1]
    probes: dict = {}
    tab_to(run, "password text", "Password")
    with run.act(f"type '{SECRET}' into Password",
                 [inserted("password text", MASK * len(SECRET)), orca_spoke(),
                  nothing_leaks(SECRET)],
                 should="each keystroke reaches the bus as one mask character, and Orca, "
                        "which does no key echo in password text, echoes it"):
        run.type(SECRET)
    with run.act("Left in Password",
                 [event("object:text-caret-moved", role="password text"),
                  nothing_leaks(SECRET)],
                 should="the caret move is published"):
        run.key("Left")
    with run.act("End, BackSpace in Password",
                 [_deleted("password text", MASK), nothing_leaks(SECRET)],
                 should="the deletion reaches the bus as one mask character"):
        run.key("End")
        run.key("BackSpace")
    with run.act("Tab to the toggle, Space: reveal",
                 [event("object:state-changed:pressed", role="toggle button", detail1=1),
                  nothing_leaks(kept),
                  probe_has(probes, "revealed", role="entry", interface="Text", text=kept)],
                 should="under SwapRole the field becomes a plain entry holding the password, "
                        "for a reader who asks; nothing of it crosses the bus as it is revealed"):
        run.key("Tab")
        time.sleep(0.5)
        run.key("space")
        time.sleep(0.8)
        probe_into(run, probes, "revealed", "entry", "Password")
    with run.act("Space: hide again",
                 [event("object:state-changed:pressed", role="toggle button", detail1=0),
                  nothing_leaks(kept, tree=True),
                  probe_has(probes, "hidden", role="password text", interface="Text",
                            text=MASK * len(kept))],
                 should="nothing of the password crosses the bus as it is hidden, and the "
                        "field's text is its mask again", tree=True):
        run.key("space")
        time.sleep(0.8)
        probe_into(run, probes, "hidden", "password text", "Password")
    with run.act("Shift+Tab back to the masked field",
                 [focused(role="password text", name="Password"), orca_says_content(),
                  not_said(kept)],
                 should="the reader hears a password field holding masked characters"):
        run.key("Shift+Tab")
    with run.act("End, type 'Z'",
                 [inserted("password text", MASK), nothing_leaks("Z")],
                 should="the next keystroke is echoed as a mask character"):
        run.key("End")
        run.type("Z")


SCENARIOS = [
    Scenario("fix-text-caret-password", "password-field", password,
             "masked typing, caret, deletion, and a reveal hidden again"),
]
