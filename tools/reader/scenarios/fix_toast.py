# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""toast-demo: the background job's toast, judged act by act on what the
toast fix promises (the sweep's own acts are in `toast.py`).

`toast-job-cancel-keys` runs start, Tab, listen and Space as one act, because
the job lasts 3.2 s, and its "focus stays on Cancel" check reads the whole
act: after Space the job is cancelled, the toast drops its Cancel and focus
moves, correctly, to the toast itself. These checks split the act at the
moment Space is pressed:

* until then, focus stays on the Cancel the reader reached and that node is
  never replaced;
* after it, the job is cancelled and focus is in the job's own toast, not on
  another toast and not on the bare frame;
* the job's title is announced once, not at each of its 20 steps.
"""

from __future__ import annotations

import time

from reader_lib.checks import custom, not_said, said
from reader_lib.scenario import Scenario
from scenarios.toast import (_focus_gains, _focus_line, announced_exactly_at_most, press,
                             said_exactly_at_most)

PACKAGE = "toast-demo"


def _cancel_held_until(marks):
    """From focus reaching Cancel to the Space press, no focus gain moved it
    and the Cancel node was never made defunct."""
    def run(act):
        space = marks.get("space")
        gains = _focus_gains(act)
        lines = [_focus_line(act, e) for e in gains]
        reached = [e for e in gains if e["source"].get("role") == "push button"
                   and e["source"].get("name") == "Cancel"]
        if not reached or space is None:
            return False, lines or ["focus never reached Cancel"]
        start = reached[0]["mono"]
        lines.append(f"Cancel reached {act.rel_ms(reached[0]):+.1f} ms, Space pressed "
                     f"{(space - act.start_mono) * 1000:+.1f} ms")
        moved = [e for e in gains if start < e["mono"] < space]
        defunct = [e for e in act.events if start < e["mono"] < space
                   and e["type"] == "object:state-changed:defunct"
                   and e.get("detail1") == 1
                   and e.get("source", {}).get("name") == "Cancel"]
        lines += [f"moved: {_focus_line(act, e)}" for e in moved]
        lines += [f"defunct: {act.rel_ms(e):+.1f} ms [push button] 'Cancel'" for e in defunct]
        return not moved and not defunct, lines
    return custom("from reaching Cancel to pressing Space, focus stays on that same Cancel",
                  run)


def _focus_after(marks, role, name):
    """Every focus gain after the Space press lands on [role] name."""
    def run(act):
        space = marks.get("space")
        after = [e for e in _focus_gains(act) if space is not None and e["mono"] > space]
        lines = [_focus_line(act, e) for e in after] or ["no focus gain after Space"]
        ok = bool(after) and all(e["source"].get("role") == role
                                 and e["source"].get("name") == name for e in after)
        return ok, lines
    return custom(f"after Space, focus is on [{role}] {name!r} and nowhere else", run)


def job_cancel_keys(run):
    marks = {}
    press(run, "Start background job")
    with run.act("Space on Start background job, Tab x4 to the toast's Cancel, "
                 "listen 0.7 s, Space",
                 [_cancel_held_until(marks),
                  said("Background job cancelled"),
                  not_said("Background job complete"),
                  _focus_after(marks, "status bar", "Background job cancelled"),
                  announced_exactly_at_most("Background job", 1)],
                 record=3.0,
                 should="the reader reaches the job's Cancel by Tab, stays on it while the "
                        "job reports progress, presses it, and stays in the job's toast"):
        run.key("space")
        run.wait(0.3)
        # Start background job -> Open log dialog -> Notifications -> the job
        # toast -> its Cancel.
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.25)
        run.wait(0.7)
        marks["space"] = time.monotonic()
        run.key("space")


def job_title_once(run):
    press(run, "Start background job")
    with run.act("Space on Start background job, and the job runs to the end",
                 [announced_exactly_at_most("Background job", 1),
                  said_exactly_at_most("Background job", 1),
                  said("Background job complete")],
                 record=2.0,
                 should="the reader hears the job start once and its completion, without "
                        "the title repeated at every step"):
        run.key("space")
        run.wait(4.0)


SCENARIOS = [
    Scenario("fix-toast-job-cancel-keys", PACKAGE, job_cancel_keys,
             "the job toast's Cancel reached by Tab keeps focus until pressed"),
    Scenario("fix-toast-job-title-once", PACKAGE, job_title_once,
             "the job toast's title is announced once, its completion once"),
]
