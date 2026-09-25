# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's acts for radio-tile and close-confirmation, beside the sweep's
`radio_close.py` (read, never edited, from here).

* `verify-radioclose-hold`: a close through KWin, then nothing at all for 15 s,
  then a key that does nothing. Settles whether the held dialog of the
  compositor route waits on input, not on a slow first frame.
* `verify-radioclose-space-leak`: the key that wakes the held dialog is Space on
  the checkbox beneath. The sweep judged "the Space does not reach the
  checkbox" by the absence of a `checked` event, which a Checkbox never emits
  (radioclose-02); here the state is read after a focus move re-walks the tree,
  and a second close asks the guard itself whether the document is still dirty.
* `verify-radioclose-sugar-toggle`: the same Checkbox defect on the sugar
  window's "Locked against closing", and what it costs: the reader unlocks,
  hears nothing, and the next close goes through with no question.
* `verify-radioclose-twice-both`: two stacked dialogs, both answered, to see
  where focus lands after each.
"""

from __future__ import annotations

from reader_lib.checks import custom, event, focused, in_tree, no_event, not_in_tree, said
from reader_lib.scenario import Scenario

from scenarios.radio_close import (BOX, MAIN, SUGAR, SUGAR_BOX, box_state, close_only,
                                   count_in_tree, dialog_checks, note_alive, open_sugar,
                                   tab_to_box)


def _alert_added(act) -> bool:
    return any(e["type"] == "object:children-changed:add"
               and (e.get("target") or {}).get("role") == "alert" for e in act.events)


def hold(run):
    tab_to_box(run)
    with run.act("close through KWin, then nothing for 15 s", [
            no_event("object:children-changed:add", role="frame"),
            no_event("object:announcement"),
            not_in_tree(role="alert")],
            should="the dialog should come at once; this records whether it comes by itself "
                   "at all", record=15.0, tree=True) as act:
        run.close_window()
    run.collect_events(act)
    run.note(f"hold: an [alert] was added during the 15 s wait: {_alert_added(act)}")
    with run.act("press Shift, which does nothing", dialog_checks(**MAIN),
                 should="the held dialog comes on the next input event", tree=True):
        run.key("Shift")
    with run.act("Escape", [not_in_tree(role="alert"), focused(**BOX)],
                 should="Cancel", tree=True):
        run.key("Escape")
    note_alive(run, "after Escape")


def space_leak(run):
    tab_to_box(run)
    with run.act("close through KWin", should="(scene) the close request; nothing is heard",
                 record=3.0) as act:
        run.close_window()
    run.collect_events(act)
    run.note(f"space-leak: an [alert] came by itself in 3 s: {_alert_added(act)}")
    with run.act("Space on the checkbox (the waking key)", [
            in_tree(role="alert", name="Close window?"),
            focused(role="push button", name="Save")],
            should="the key wakes the dialog; it should not toggle the checkbox",
            tree=True):
        run.key("space")
    with run.act("Escape: Cancel, focus back on the checkbox (a re-walk)", [
            not_in_tree(role="alert"), focused(**BOX), said("checked"), box_state(True)],
            should="focus returns to the checkbox, still checked: the waking Space did "
                   "not reach it", tree=True):
        run.key("Escape")
    with run.act("close through KWin again", record=3.0,
                 should="(scene) a second close: the guard asks only if dirty is still "
                        "true") as act2:
        run.close_window()
    run.collect_events(act2)
    if not _alert_added(act2):
        with run.act("press Shift, which does nothing", [
                in_tree(role="alert", name="Close window?")],
                should="the dialog comes: the document is still dirty", tree=True):
            run.key("Shift")
    note_alive(run, "after the second close")


def sugar_toggle(run):
    open_sugar(run)
    with run.act("Space: unlock (uncheck 'Locked against closing')", [
            event("object:state-changed:checked", role="check box", detail1=0),
            said("not checked")],
            should="the checkbox unchecks and the reader hears it", tree=True):
        run.key("space")
    with run.act("close the sugar window through KWin", [
            no_event("object:announcement")],
            should="unlocked: the window closes with no question", record=4.0):
        close_only(run, "can_close")
    note_alive(run, "after closing the unlocked sugar window")
    left = run.find_all(role="frame")
    run.note(f"sugar-toggle: frames on the bus after the close: {len(left)}")


def twice_both(run):
    tab_to_box(run)
    with run.act("close through KWin", record=3.0, should="(scene) first close"):
        run.close_window()
    with run.act("close through KWin again", record=3.0, should="(scene) second close"):
        run.close_window()
    with run.act("press Shift, which does nothing", [count_in_tree("alert", 1)],
                 should="one dialog", tree=True):
        run.key("Shift")
    with run.act("Escape: first Cancel", [not_in_tree(role="alert"), focused(**BOX)],
                 should="Cancel ends it", tree=True):
        run.key("Escape")
    with run.act("Escape: second Cancel", [not_in_tree(role="alert"), focused(**BOX)],
                 should="(if a second dialog was left) it ends too", tree=True):
        run.key("Escape")
    note_alive(run, "after two Escapes")


CLOSE_BTN = {"role": "push button", "name": "Close window (ctx.close_window)"}


def behind_modal(run):
    """While the message box is up, the window's own controls stay in the
    tree, focusable and enabled. Does a screen reader's own activation reach
    them through the modal?"""
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="(scene) the reader reaches the button"):
        run.key("Tab")
    with run.act("Space: the dialog opens", [count_in_tree("alert", 1),
                                             focused(role="push button", name="Save")],
                 should="(scene) one dialog, focus on Save", tree=True, record=3.0):
        run.key("space")
    with run.act("AT-SPI click on 'Close window (ctx.close_window)' behind the dialog", [
            count_in_tree("alert", 1), no_event("object:children-changed:add", role="frame")],
            should="the modal dialog blocks the window beneath it: nothing happens",
            tree=True, record=3.0):
        run.action("click", **CLOSE_BTN)
    with run.act("AT-SPI click on the checkbox behind the dialog",
                 [count_in_tree("alert", 1)],
                 should="the modal dialog blocks the window beneath it: nothing happens",
                 tree=True):
        run.action("click", **BOX)
    with run.act("AT-SPI grab_focus on the checkbox behind the dialog", [
            no_event("object:state-changed:focused", role="check box")],
            should="focus stays inside the modal dialog", tree=True):
        run.grab_focus(**BOX)
    n = len(run.find_all(role="alert"))
    run.note(f"behind-modal: alerts on the bus before answering: {n}")
    for i in range(max(n, 1)):
        with run.act(f"AT-SPI click on Cancel ({i + 1})", tree=True,
                     should="a screen reader's own activation answers the dialog"):
            run.action("click", role="push button", name="Cancel")
    with run.act("Shift+Tab / Tab round to the checkbox, which re-walks it", tree=True,
                 should="the reader hears the checkbox's real state; still checked if the "
                        "click behind the dialog did nothing"):
        focus = run.last_focus()
        run.note(f"behind-modal: focus after the dialogs: [{focus.get('role')}] "
                 f"{focus.get('name')!r}")
        if focus.get("name") == BOX["name"]:
            run.key("Tab")
            run.wait(0.8)
            run.key("Shift+Tab")
        else:
            run.key("Shift+Tab")
    with run.act("close through KWin", record=3.0,
                 should="(scene) the guard answers from the real dirty flag") as act:
        run.close_window()
    run.collect_events(act)
    if not _alert_added(act) and run.alive():
        with run.act("press Shift, which does nothing", tree=True,
                     should="a dialog comes if the document is still dirty"):
            run.key("Shift")
    note_alive(run, "at the end")


def behind_modal_box(run):
    """Only the checkbox behind the dialog, by AT-SPI click; then Cancel and a
    close through KWin, which the guard answers from the real dirty flag."""
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="(scene) the reader reaches the button"):
        run.key("Tab")
    with run.act("Space: the dialog opens", [count_in_tree("alert", 1),
                                             focused(role="push button", name="Save")],
                 should="(scene) one dialog, focus on Save", tree=True, record=3.0):
        run.key("space")
    with run.act("AT-SPI click on the checkbox behind the dialog", [
            count_in_tree("alert", 1), focused(role="push button", name="Save"),
            box_state(True)],
            should="the modal dialog blocks the window beneath it: nothing happens",
            tree=True):
        run.action("click", **BOX)
    with run.act("Escape: Cancel", [not_in_tree(role="alert"), focused(**CLOSE_BTN)],
                 should="the dialog goes; focus back on the button", tree=True):
        run.key("Escape")
    with run.act("Shift+Tab to the checkbox (a focus move re-walks it)", [
            focused(**BOX), said("check box checked"), box_state(True)],
            should="the checkbox is as it was before the dialog: checked", tree=True):
        run.key("Shift+Tab")
    with run.act("close through KWin", [
            custom("the application is still running", lambda act: (run.alive(), []))],
            record=3.0, should="the document is dirty, so the guard holds the close "
                               "(its dialog is held until the next input, radioclose-01)"):
        run.close_window()
    note_alive(run, "after the close through KWin")


def behind_modal_focus(run):
    """Only AT-SPI grab_focus on a control behind the dialog."""
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="(scene) the reader reaches the button"):
        run.key("Tab")
    with run.act("Space: the dialog opens", [count_in_tree("alert", 1),
                                             focused(role="push button", name="Save")],
                 should="(scene) one dialog, focus on Save", tree=True, record=3.0):
        run.key("space")
    with run.act("AT-SPI grab_focus on 'Open can_close-sugar window…' behind the dialog", [
            no_event("object:state-changed:focused", role="push button",
                     name_contains="Open can_close")],
            should="focus stays inside the modal dialog", tree=True):
        run.grab_focus(role="push button", name="Open can_close-sugar window…")
    with run.act("Space, where focus now is", [count_in_tree("alert", 1)],
                 should="the key goes to the dialog, not to the window beneath it",
                 tree=True, record=3.0):
        run.key("space")
    run.note(f"behind-modal-focus: frames on the bus: {len(run.find_all(role='frame'))}")


SCENARIOS = [
    Scenario("verify-radioclose-behind-modal-box", "close-confirmation", behind_modal_box,
             "AT-SPI click on the checkbox behind the message box, then close"),
    Scenario("verify-radioclose-behind-modal-focus", "close-confirmation",
             behind_modal_focus,
             "AT-SPI grab_focus on a button behind the message box, then Space"),
    Scenario("verify-radioclose-behind-modal", "close-confirmation", behind_modal,
             "AT-SPI click / focus on the window's controls behind the message box"),
    Scenario("verify-radioclose-hold", "close-confirmation", hold,
             "close through KWin and wait 15 s with no input, then Shift"),
    Scenario("verify-radioclose-space-leak", "close-confirmation", space_leak,
             "the Space that wakes the held dialog: does it toggle the checkbox?"),
    Scenario("verify-radioclose-sugar-toggle", "close-confirmation", sugar_toggle,
             "unlock the sugar window by Space; the state change and the close"),
    Scenario("verify-radioclose-twice-both", "close-confirmation", twice_both,
             "two stacked dialogs, answer both"),
]
