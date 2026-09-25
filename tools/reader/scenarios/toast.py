# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""toast-demo: what a reader gets from toasts and the notification centre.

The example (`examples/toast_demo/src/main.rs`) spawns toasts from a toolbar
(Info, Success, Warning, Error, Persistent error), updates one in place by id
(Start / Update / Complete upload), drives a loading toast from a worker
thread (Start background job: 20 progress steps 160 ms apart, then a success
toast), and has a `NotificationCenterButton` (the bell) and an "Open log
dialog" button in its status bar. None of its messages go through the
framework announcer (`ctx.announce`): every toast is heard because its node is
a live region that enters the tree with a name.

Where the reader's share is produced:

* `crates/teksilo-widgets/src/toast/surface.rs`, `ToastSurface::accessibility`:
  one node per toast, `Role::Status` (polite; AT-SPI "status bar") or
  `Role::Alert` (assertive, for an error; AT-SPI "notification"), named by
  the title, the body as its description only. Focusable (Escape dismisses).
  The close button is `IconButton::clear()`.
* `crates/teksilo-widgets/src/toast/host.rs`, `ToastHost::build`: binds the
  registry's version at `Rebuild` and builds a fresh `ToastSurface` for every
  live entry on every queue change (show, update, dismiss, expiry). A fresh
  widget is a fresh AccessKit node, and every adapter announces a live node
  that enters the tree with a name.
* `crates/teksilo-core/src/widget_tree/layout_impl.rs`: when a rebuild
  destroys the focused node, focus goes to the first focusable descendant of
  the rebuilt root, which for the host is the oldest toast.
* `crates/teksilo-widgets/src/notification/center_button.rs`: the bell, an
  `IconButton::bell()` named by its tooltip, rebuilt on every archive change,
  with the unread count in a separate `Badge`.
* `crates/teksilo-widgets/src/notification/log.rs`: the log, `Role::List`
  named "Notifications", rows are `StandardListItem`s.
"""

from __future__ import annotations

import time

from reader_lib.checks import (announced, custom, focused, in_tree, not_announced,
                               not_in_tree, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.run import RunError
from reader_lib.scenario import Scenario

PACKAGE = "toast-demo"


# ---------------------------------------------------------------------------
# Checks of our own
# ---------------------------------------------------------------------------


def _n(text: str | None) -> str:
    return normalized(text or "")


def _anns(act):
    return [e for e in act.events if e["type"] == "object:announcement"]


def _ann_line(act, e):
    src = e.get("source", {})
    return (f"{act.rel_ms(e):+8.1f} ms object:announcement [{src.get('role')}] "
            f"{src.get('name')!r} text={e.get('text')!r}")


def only_announced(*titles: str):
    """The act's announcements are exactly `titles`, each once: nothing that
    was already on screen is announced again."""
    def run(act):
        texts = [e.get("text") for e in _anns(act)]
        ok = sorted(_n(t) for t in texts) == sorted(_n(t) for t in titles)
        return ok, [f"{len(texts)} announcement(s)"] + [_ann_line(act, e) for e in _anns(act)[:30]]
    return custom(f"the act announces exactly {list(titles)!r}, each once", run)


def announced_exactly_at_most(text: str, most: int):
    """No more than `most` announcements whose text is exactly `text`."""
    def run(act):
        found = [e for e in _anns(act) if _n(e.get("text")) == _n(text)]
        return len(found) <= most, [f"{len(found)} announcement(s) of exactly {text!r}"] + \
            [_ann_line(act, e) for e in found[:25]]
    return custom(f"at most {most} announcement(s) of exactly {text!r}", run)


def said_exactly_at_most(text: str, most: int):
    def run(act):
        heard = [u for u in utterances(act.orca) if _n(u.text) == _n(text)]
        return len(heard) <= most, [f"Orca said exactly {text!r} {len(heard)} time(s)"] + \
            [f"{u.stamp} said{' (cut)' if u.cut else ''}: {u.text!r}" for u in heard[:25]]
    return custom(f"Orca says exactly {text!r} at most {most} time(s)", run, needs_orca=True)


def said_any(*texts: str):
    """Orca said at least one of `texts` (in any utterance, cut or not)."""
    def run(act):
        every = [f"{u.stamp} said{' (cut)' if u.cut else ''}: {u.text!r}"
                 for u in utterances(act.orca)]
        ok = any(_n(t) in _n(u.text) for u in utterances(act.orca) for t in texts)
        return ok, every or ["Orca said nothing"]
    return custom(f"Orca says any of {texts!r}", run, needs_orca=True)


def first_said_uncut(text: str):
    """The first utterance carrying `text` was not cut: a later repeat (a
    re-announcement when another toast expires) does not count."""
    def run(act):
        heard = [u for u in utterances(act.orca) if _n(text) in _n(u.text)]
        if not heard:
            return False, [f"Orca never said {text!r}"]
        return not heard[0].cut, [f"{u.stamp} said{' (cut)' if u.cut else ''}: {u.text!r}"
                                  for u in heard]
    return custom(f"Orca's first {text!r} is not cut", run, needs_orca=True)


def _focus_gains(act):
    return [e for e in act.events
            if e["type"] == "object:state-changed:focused" and e.get("detail1") == 1]


def _focus_line(act, e):
    src = e.get("source", {})
    return (f"{act.rel_ms(e):+8.1f} ms focused {e.get('detail1')} [{src.get('role')}] "
            f"{src.get('name')!r} path={src.get('path')}")


def focus_kept(role: str, name: str):
    """No focus change in the act moved focus off a node with this role and
    name, and none moved it to a fresh node of the same name either (Orca
    reads a new focus as a new arrival)."""
    def run(act):
        lines, lost = [], False
        for e in act.events:
            if e["type"] != "object:state-changed:focused":
                continue
            src = e.get("source", {})
            lines.append(_focus_line(act, e))
            if e.get("detail1") == 1:
                lost = True  # any gain is a move, even onto a same-named node
            if e.get("detail1") == 0 and src.get("role") == role and src.get("name") == name:
                lost = True
        return not lost, lines or ["no focus change in the act"]
    return custom(f"focus stays, untouched, on [{role}] {name!r}", run)


def focus_reached(role: str, name: str):
    """Some focus gain in the act landed on [role] name."""
    def run(act):
        gains = _focus_gains(act)
        ok = any(e["source"].get("role") == role and e["source"].get("name") == name
                 for e in gains)
        return ok, [_focus_line(act, e) for e in gains][:40] or ["no focus gain in the act"]
    return custom(f"focus reaches [{role}] {name!r} at some point", run)


def focus_not_frame():
    def run(act):
        gains = _focus_gains(act)
        lines = [_focus_line(act, e) for e in gains]
        if not gains:
            return False, ["no focus gain in the act"]
        return gains[-1]["source"].get("role") != "frame", lines
    return custom("focus lands on a real control, not the bare frame", run)


def focus_last_is(role: str, name: str | None):
    def run(act):
        gains = _focus_gains(act)
        lines = [_focus_line(act, e) for e in gains]
        if not gains:
            return False, ["no focus gain in the act"]
        last = gains[-1]["source"]
        ok = last.get("role") == role and (name is None or last.get("name") == name)
        return ok, lines
    return custom(f"the act's last focus gain is [{role}] {name or '*'!r}", run)


def focus_never_left_after(role: str, name: str):
    """Once focus reaches [role] name in the act, no later gain moves it
    elsewhere."""
    def run(act):
        gains = _focus_gains(act)
        lines = [_focus_line(act, e) for e in gains]
        reached = [i for i, e in enumerate(gains)
                   if e["source"].get("role") == role and e["source"].get("name") == name]
        if not reached:
            return False, lines or ["no focus gain in the act"]
        later = gains[reached[0] + 1:]
        moved = [e for e in later
                 if not (e["source"].get("role") == role and e["source"].get("name") == name)]
        return not moved, lines
    return custom(f"once on [{role}] {name!r}, focus stays there", run)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def press(run, button: str) -> None:
    """Put focus on a button through AT-SPI, outside any act, and wait until
    the bus says it is there (a grab made before the window's own activation
    is undone by it)."""
    run.wait_for(role="frame", state="active", timeout=20)
    run.wait_for(role="push button", name=button)
    for _ in range(3):
        run.grab_focus(role="push button", name=button)
        deadline = time.monotonic() + 3.0
        while time.monotonic() < deadline:
            if run.find(role="push button", name=button, state="focused"):
                return
            time.sleep(0.1)
    raise RunError(f"focus never reached the {button!r} button")


def spawn(run, button: str, title: str) -> None:
    """Raise a toast from `button` in an act of its own, and let it be read."""
    press(run, button)
    with run.act(f"Space on {button}", [said(title)], record=1.0,
                 should=f"the {title!r} toast appears and is read"):
        run.key("space")


# ---------------------------------------------------------------------------
# 1. Every severity, one after another, then their expiry
# ---------------------------------------------------------------------------

SEVERITIES = [
    # button, title, body, AT-SPI role of the toast node
    ("Info", "Info notice #1", None, "status bar"),
    ("Success", "Saved #2", None, "status bar"),
    ("Warning", "Warning #3", "Take a look when you have a moment.", "status bar"),
    ("Error", "Build #4 failed", "Three errors in src/main.rs, two warnings.", "notification"),
]


def severities(run):
    for button, title, body, role in SEVERITIES:
        press(run, button)
        checks = [announced(title), said(title), only_announced(title),
                  in_tree(role=role, name=title)]
        if body:
            checks.append(in_tree(role=role, name=title, description_contains=body))
            checks.append(said(body))
        with run.act(f"Space on {button}", checks, settle=0.8, record=1.5,
                     should=f"a {button.lower()} toast appears and the reader hears "
                            f"{title!r}{' and its body' if body else ''}, and nothing "
                            "already on screen again"):
            run.key("space")
    # Each toast expires 10 s after it appeared; each expiry rebuilds the host.
    titles = [t for _, t, _, _ in SEVERITIES]
    with run.act("the toasts time out one by one",
                 [only_announced()] + [not_said(t) for t in titles]
                 + [not_in_tree(name=t) for t in titles],
                 settle=0.2, record=1.0,
                 should="each toast leaves quietly; nothing still on screen is re-read"):
        run.wait(11.0)


# ---------------------------------------------------------------------------
# 2. A persistent error, and what happens to it when others come and go
# ---------------------------------------------------------------------------


def persistent(run):
    press(run, "Persistent error")
    with run.act("Space on Persistent error",
                 [announced("Sticky error #1"), said("Sticky error #1"),
                  said("This one persists until you dismiss it."),
                  in_tree(role="notification", name="Sticky error #1")],
                 should="an assertive error toast appears and the reader hears it"):
        run.key("space")
    press(run, "Info")
    with run.act("Space on Info while the sticky error is up",
                 [said("Info notice #2"), only_announced("Info notice #2"),
                  not_said("Sticky error #1")],
                 should="only the new toast is read"):
        run.key("space")
    with run.act("the info toast times out",
                 [only_announced(), not_said("Sticky error #1"),
                  not_in_tree(name="Info notice #2"),
                  in_tree(role="notification", name="Sticky error #1")],
                 settle=0.2, record=1.0,
                 should="the info toast leaves; the sticky error, still there, is not "
                        "announced again as if it were new"):
        run.wait(10.0)


# ---------------------------------------------------------------------------
# 3. Update in place by id
# ---------------------------------------------------------------------------


def upload(run):
    press(run, "Start upload")
    with run.act("Space on Start upload",
                 [announced("Uploading 1 of 7"), said("Uploading 1 of 7"),
                  in_tree(role="status bar", name_contains="Uploading 1 of 7")],
                 should="a loading toast appears and is read"):
        run.key("space")
    press(run, "Update upload")
    with run.act("Space on Update upload",
                 [announced("Uploading 4 of 7"), said("Uploading 4 of 7"),
                  only_announced("Uploading 4 of 7…"),
                  in_tree(role="status bar", name_contains="Uploading 4 of 7"),
                  not_in_tree(name_contains="Uploading 1 of 7")],
                 should="the same toast now says 4 of 7, and the reader hears the update"):
        run.key("space")
    press(run, "Complete upload")
    with run.act("Space on Complete upload",
                 [announced("Upload complete"), said("Upload complete"),
                  only_announced("Upload complete"),
                  not_in_tree(name_contains="Uploading"),
                  in_tree(role="status bar", name="Upload complete")],
                 should="the toast becomes a success and the reader hears it"):
        run.key("space")
    with run.act("the success toast times out",
                 [only_announced(), not_said("Upload complete"),
                  not_in_tree(name="Upload complete")],
                 settle=0.2, record=1.0,
                 should="it leaves quietly after 5 s"):
        run.wait(5.0)


# ---------------------------------------------------------------------------
# 4. A background job driving a loading toast
# ---------------------------------------------------------------------------


def background_job(run):
    press(run, "Start background job")
    with run.act("Space on Start background job, and the job runs to the end",
                 [announced("Background job"), said("Background job"),
                  announced_exactly_at_most("Background job", 1),
                  said_exactly_at_most("Background job", 1),
                  said_any("%", "percent", "Fetching item"),
                  said("Background job complete"),
                  said("All 20 items fetched"),
                  not_in_tree(role="push button", name="Cancel"),
                  not_in_tree(role="progress bar")],
                 record=2.0, tree=True,
                 should="the reader hears the job start, some sense of its progress, "
                        "and its completion, without the title repeated at every step"):
        run.key("space")
        run.wait(4.0)


def background_job_with_info(run):
    """An info toast already on screen while the job progresses."""
    spawn(run, "Info", "Info notice #1")
    press(run, "Start background job")
    with run.act("Space on Start background job while the info toast is up",
                 [said("Background job"), not_announced("Info notice #1"),
                  not_said("Info notice #1")],
                 record=2.0,
                 should="progress steps do not re-read the unrelated info toast"):
        run.key("space")
        run.wait(4.0)


# ---------------------------------------------------------------------------
# 5. Cancelling the job from the keyboard, and through AT-SPI
# ---------------------------------------------------------------------------


def job_cancel_keyboard(run):
    """All in one act: the job lasts 3.2 s, shorter than Orca's catch-up
    between two acts can be."""
    press(run, "Start background job")
    with run.act("Space on Start background job, Tab x4 to the toast's Cancel, "
                 "listen 0.7 s, Space",
                 [focus_reached("push button", "Cancel"),
                  focus_never_left_after("push button", "Cancel"),
                  said("Background job cancelled"),
                  not_said("Background job complete")],
                 record=3.0,
                 should="the reader reaches the job's Cancel by Tab, hears it, presses "
                        "it, and the job is cancelled"):
        run.key("space")
        run.wait(0.3)
        # Start background job -> Open log dialog -> Notifications -> the job
        # toast -> its Cancel.
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.25)
        run.wait(0.7)
        run.key("space")


def job_cancel_atspi(run):
    press(run, "Start background job")
    with run.act("Space on Start background job, then an AT-SPI click on the toast's "
                 "Cancel (retried if the node is gone)",
                 [said("Background job cancelled"), not_said("Background job complete")],
                 record=3.0,
                 should="a screen reader's own activation of Cancel cancels the job"):
        run.key("space")
        run.wait(0.6)
        _click_cancel_retrying(run)


def _click_cancel_retrying(run, attempts: int = 30):
    """Find the Cancel button and do its action; the node is replaced every
    160 ms, so a lookup can find a node that is gone by the time it is acted
    on. Each attempt is written into the act's steps."""
    for i in range(attempts):
        try:
            reply = run.action("click", role="push button", name="Cancel")
        except RunError as exc:
            run._step(f"attempt {i + 1}: {exc}")
            time.sleep(0.05)
            continue
        run._step(f"attempt {i + 1}: do_action returned {reply.get('done')}")
        if reply.get("done"):
            return
        time.sleep(0.05)


# ---------------------------------------------------------------------------
# 6. Reaching a toast by keyboard, and dismissing it
# ---------------------------------------------------------------------------


def dismiss(run):
    spawn(run, "Persistent error", "Sticky error #1")
    with run.act("Tab x7 into the toast",
                 [focused(role="notification", name="Sticky error #1"),
                  said("Sticky error #1"),
                  said("This one persists until you dismiss it."),
                  custom("Orca says the body once",
                         lambda act: _said_count(act, "This one persists until you dismiss it.",
                                                 1), needs_orca=True)],
                 should="focus lands on the toast; the reader hears its title and body once"):
        # Persistent error -> Start upload -> Update upload -> Complete upload ->
        # Start background job -> Open log dialog -> Notifications -> toast.
        run.key(*["Tab"] * 7)
    with run.act("Tab to the toast's close button",
                 [focused(role="push button"), said_any("Close", "Dismiss")],
                 should="the close button says what it does"):
        run.key("Tab")
    with run.act("Space on the close button",
                 [not_in_tree(name="Sticky error #1"), focus_not_frame()],
                 tree=True,
                 should="the toast goes, and focus moves somewhere sensible (the bell, "
                        "or the control that raised it)"):
        run.key("space")
    with run.act("Tab after the dismissal",
                 [focused(role="push button")],
                 should="Tab continues from where the reader was"):
        run.key("Tab")
    spawn(run, "Persistent error", "Sticky error #2")
    with run.act("Tab x7 into the second toast",
                 [focused(role="notification", name="Sticky error #2")],
                 should="focus lands on the toast"):
        run.key(*["Tab"] * 7)
    with run.act("Escape on the focused toast",
                 [not_in_tree(name="Sticky error #2"), focus_not_frame()],
                 tree=True,
                 should="the toast goes, and focus moves somewhere sensible"):
        run.key("Escape")


def _said_count(act, text, want):
    heard = [u for u in utterances(act.orca) if _n(text) in _n(u.text)]
    return len(heard) == want, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                                for u in heard] or [f"Orca never said {text!r}"]


# ---------------------------------------------------------------------------
# 7. A toast arriving while the reader is inside another toast, or on the bell
# ---------------------------------------------------------------------------


def focus_survives(run):
    spawn(run, "Persistent error", "Sticky error #1")
    with run.act("Tab x7 into the sticky toast",
                 [focused(role="notification", name="Sticky error #1")],
                 should="focus lands on the toast"):
        run.key(*["Tab"] * 7)
    with run.act("AT-SPI click on Info (a new toast arrives, focus should not move)",
                 [first_said_uncut("Info notice #2"), only_announced("Info notice #2"),
                  focus_kept("notification", "Sticky error #1")],
                 should="a new toast appears and is heard; the reader stays where they "
                        "were, with nothing re-read"):
        run.action("click", role="push button", name="Info")
    with run.act("Tab after the new toast arrived",
                 [focus_last_is("push button", "Clear")],
                 should="Tab continues from the sticky toast, to its close button"):
        run.key("Tab")
    # The bell: every toast changes the archive, which rebuilds the bell.
    press(run, "Notifications")
    with run.act("AT-SPI click on Success while focus is on the bell",
                 [first_said_uncut("Saved #3"), focus_kept("push button", "Notifications")],
                 should="a new toast appears and is heard; focus stays on the bell"):
        run.action("click", role="push button", name="Success")


def focus_second(run):
    spawn(run, "Persistent error", "Sticky error #1")
    spawn(run, "Warning", "Warning #2")
    # Warning -> Error -> Persistent error -> Start upload -> Update upload ->
    # Complete upload -> Start background job -> Open log dialog ->
    # Notifications -> sticky toast -> its Clear -> Warning toast.
    with run.act("Tab x11 to the Warning toast",
                 [focused(role="status bar", name="Warning #2")], record=1.0,
                 should="focus reaches the second toast"):
        run.key(*["Tab"] * 11, gap=0.2)
    with run.act("AT-SPI click on Success (a third toast arrives)",
                 [first_said_uncut("Saved #3"), focus_kept("status bar", "Warning #2")],
                 should="the new toast is heard; the reader stays on the Warning toast"):
        run.action("click", role="push button", name="Success")


# ---------------------------------------------------------------------------
# 8. An error toast's action, reached by keyboard and held there
# ---------------------------------------------------------------------------


def error_action(run):
    spawn(run, "Error", "Build #1 failed")
    # Error -> Persistent error -> Start upload -> Update upload -> Complete
    # upload -> Start background job -> Open log dialog -> Notifications ->
    # toast -> Show errors.
    with run.act("Tab x9 to the toast's Show errors",
                 [focused(role="push button", name="Show errors"), said("Show errors")],
                 record=1.0,
                 should="focus reaches the action; the reader hears it"):
        run.key(*["Tab"] * 9, gap=0.2)
    with run.act("stay on Show errors, deciding, for 10 s",
                 [focus_kept("push button", "Show errors"),
                  in_tree(role="push button", name="Show errors")],
                 settle=0.0, record=0.5,
                 should="the toast does not vanish from under keyboard focus"):
        run.wait(10.0)


# ---------------------------------------------------------------------------
# 9. The bell and its popover
# ---------------------------------------------------------------------------


def bell(run):
    spawn(run, "Info", "Info notice #1")
    spawn(run, "Warning", "Warning #2")
    press(run, "Open log dialog")
    with run.act("Tab to the bell with two unread notifications",
                 [focused(role="push button", name_contains="Notifications"),
                  said_any("2 unread", "2 new", "2 notifications", "Notifications 2",
                           "Notifications, 2", "2")],
                 tree=True,
                 should="the reader hears the bell and how many notifications are unread"):
        run.key("Tab")
    with run.act("Space on the bell",
                 [said("Notifications"),
                  in_tree(role="dialog", name="Notifications"),
                  in_tree(role="list", name="Notifications"),
                  in_tree(role="list item", name="Warning #2"),
                  in_tree(role="list item", name="Info notice #1"),
                  not_said("Today")],
                 tree=True,
                 should="the popover opens as a dialog named Notifications, focus moves "
                        "into it, and the rows are list items"):
        run.key("space")
    with run.act("Tab, Tab, Tab in the popover",
                 [said_any("Warning #2"), said_any("Info notice #1"),
                  in_tree(role="list", name="Notifications")],
                 tree=True,
                 should="the reader reaches the notifications themselves, and stays in "
                        "the popover"):
        run.key("Tab", "Tab", "Tab", gap=0.8)


def bell_escape(run):
    spawn(run, "Info", "Info notice #1")
    press(run, "Notifications")
    with run.act("Space on the bell", [said("Mark all read")], tree=True,
                 should="the popover opens"):
        run.key("space")
    with run.act("Escape closes the popover",
                 [focused(role="push button", name="Notifications"),
                  not_in_tree(role="dialog")],
                 tree=True,
                 should="the popover closes, focus returns to the bell, and the unread "
                        "count is gone"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# 10. The log dialog
# ---------------------------------------------------------------------------


def log_dialog(run):
    spawn(run, "Info", "Info notice #1")
    spawn(run, "Error", "Build #2 failed")
    press(run, "Open log dialog")
    with run.act("Space on Open log dialog",
                 [said("Notifications"),
                  in_tree(role="dialog", name="Notifications"),
                  in_tree(role="list", name="Notifications"),
                  in_tree(role="list item", name="Build #2 failed"),
                  in_tree(role="list item", name="Info notice #1")],
                 tree=True,
                 should="a dialog titled Notifications opens and the reader hears it"):
        run.key("space")
    with run.act("Tab through the dialog (4 presses)",
                 [said_any("Build #2 failed"), said_any("Info notice #1")],
                 tree=True,
                 should="the reader reaches the notifications themselves"):
        run.key("Tab", "Tab", "Tab", "Tab", gap=0.8)
    # The private KWin's pointer rests at the centre of the screen, which is
    # where the dialog's first row lands: its rich tooltip opens on hover and
    # takes the first Escape. Two presses, as a mouse-free reader would not
    # meet that tooltip at all.
    with run.act("Escape twice (a hover tooltip under the resting pointer takes the "
                 "first) closes the dialog",
                 [focused(role="push button", name="Open log dialog"),
                  not_in_tree(role="dialog")],
                 tree=True,
                 should="the dialog closes and focus returns to Open log dialog"):
        run.key("Escape")
        run.wait(0.6)
        run.key("Escape")


# ---------------------------------------------------------------------------
# 11. How long a toast stays: a second toast shown 5 s after the first
# ---------------------------------------------------------------------------


def _watch(run, titles, until: float, every: float = 0.2):
    """Poll the bus for each title until none is left or `until` seconds
    pass; return when each was first and last seen, as monotonic times."""
    seen = {t: [None, None] for t in titles}
    end = time.monotonic() + until
    while time.monotonic() < end:
        now = time.monotonic()
        for t in titles:
            if run.find(name=t):
                if seen[t][0] is None:
                    seen[t][0] = now
                seen[t][1] = now
        if all(v[0] is not None for v in seen.values()) and \
                not any(run.find(name=t) for t in titles):
            break
        time.sleep(every)
    return seen


def lifetime(run):
    """Info at t0, Success about 5 s later (through AT-SPI, so focus does
    not move and nothing else repaints), then watch both on the bus."""
    press(run, "Info")
    result = {}

    def lives(title, want, slack=1.0):
        """From the first `children-changed:add` of a node with this name to
        the last `children-changed:remove` of one: the host swaps the node on
        every rebuild, so the toast is gone at the last removal."""
        def check(act):
            adds = [e for e in act.events if e["type"] == "object:children-changed:add"
                    and e.get("target", {}).get("name") == title]
            removes = [e for e in act.events if e["type"] == "object:children-changed:remove"
                       and e.get("target", {}).get("name") == title]
            if not adds or not removes:
                return False, [f"{title!r}: {len(adds)} add(s), {len(removes)} removal(s)"]
            life = removes[-1]["mono"] - adds[0]["mono"]
            return abs(life - want) <= slack, [
                f"{title!r} first added {act.rel_ms(adds[0]):+.1f} ms, last removed "
                f"{act.rel_ms(removes[-1]):+.1f} ms: {life:.2f} s on the bus"]
        return custom(f"{title!r} stays about {want:g} s", check)

    with run.act("Space on Info, AT-SPI click on Success 5 s later, watch both",
                 [lives("Info notice #1", 10.0), lives("Saved #2", 10.0)],
                 settle=0.5, record=0.5,
                 should="each toast stays its own 10 s, so a reader has the same time "
                        "to reach the second as the first"):
        result["t0"] = time.monotonic()
        run.key("space")
        run.wait(5.0)
        run.action("click", role="push button", name="Success")
        seen = _watch(run, ["Info notice #1", "Saved #2"], until=14.0)
        result.update({t: tuple(v) for t, v in seen.items()})
        for t, (a, b) in seen.items():
            if a is not None:
                run._step(f"{t!r} seen from +{a - result['t0']:.2f} s to "
                          f"+{b - result['t0']:.2f} s")


SCENARIOS = [
    Scenario("toast-lifetime", PACKAGE, lifetime,
             "a second toast shown 5 s after the first: does it get its own 10 s?"),
    Scenario("toast-severities", PACKAGE, severities,
             "Info, Success, Warning, Error one after another, then their timeouts"),
    Scenario("toast-persistent", PACKAGE, persistent,
             "a persistent error, an info toast beside it, the info's timeout"),
    Scenario("toast-upload", PACKAGE, upload,
             "a loading toast updated in place by id, then completed"),
    Scenario("toast-job", PACKAGE, background_job,
             "the background job's progress toast from start to completion"),
    Scenario("toast-job-with-info", PACKAGE, background_job_with_info,
             "background job progress while an info toast is on screen"),
    Scenario("toast-job-cancel-keys", PACKAGE, job_cancel_keyboard,
             "reach the job toast's Cancel by Tab and press it"),
    Scenario("toast-job-cancel-atspi", PACKAGE, job_cancel_atspi,
             "cancel the job through an AT-SPI click on Cancel"),
    Scenario("toast-dismiss", PACKAGE, dismiss,
             "reach a sticky toast by Tab, close it by its button and by Escape"),
    Scenario("toast-focus-survives", PACKAGE, focus_survives,
             "a new toast arrives while focus is inside a toast, or on the bell"),
    Scenario("toast-focus-second", PACKAGE, focus_second,
             "focus on the second of two toasts when a third arrives"),
    Scenario("toast-error-action", PACKAGE, error_action,
             "an error toast's Show errors reached by Tab and held under focus"),
    Scenario("toast-bell", PACKAGE, bell,
             "the bell with unread notifications, its popover and its rows"),
    Scenario("toast-bell-escape", PACKAGE, bell_escape,
             "the bell's popover closed by Escape"),
    Scenario("toast-log-dialog", PACKAGE, log_dialog,
             "the notification log dialog"),
]
