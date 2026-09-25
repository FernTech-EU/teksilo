# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The page behind an in-tree modal, as a screen reader's own requests meet it
(topic modal-at: dialogs-08, radioclose-M1).

The sweep's `dialogs-modal-inert` and `verify-radioclose-behind-modal*` found
an AT-SPI click or `grab_focus` on a control behind a modal box reaching it: a
second and a third box stacked up, a checkbox behind "Close window?" toggled,
and focus left the box for a control it covers. These acts ask the same things
of the fixed build, then check that the key the reader presses next still goes
to the box, and that the page is as alive to Orca once the box has closed as
before it opened. The page stays in the tree while the box is up (taking it
out leaves every node on it defunct to Orca when it comes back), so the
requests find their target; what is judged is that nothing comes of them.

* `fix-modal-at-dialogs` (`dialogs-and-popovers`): "Save changes?" up, then a
  click on "Welcome" and on "Delete file?", and `grab_focus` on "Delete
  file?", all behind it; Space, which must press the box's own Save; Tab on
  along the page.
* `fix-modal-at-close` (`close-confirmation`): "Close window?" up, then a click
  on the document's checkbox and on the Close button, and `grab_focus` on the
  button that opens a second window, all behind it; Escape; Shift+Tab to the
  checkbox, which must still be checked.
"""

from __future__ import annotations

from reader_lib.checks import (event, focused, no_event, not_in_tree, not_said, said)
from reader_lib.scenario import Scenario

from scenarios.dialogs import focus_opener
from scenarios.radio_close import BOX, CLOSE_BTN, SUGAR_OPEN, box_state, count_in_tree, tab_to_box

NOTHING = "nothing happens: the page behind a modal box takes no request"


def nothing_came_of_it(*extra):
    """No box opened, no focus moved, nothing was announced."""
    return [count_in_tree("alert", 1), no_event("object:state-changed:focused"),
            no_event("object:announcement"), *extra]


def dialogs(run):
    focus_opener(run, "Save changes?")
    with run.act("Space opens 'Save changes?'", [
            count_in_tree("alert", 1), focused(role="push button", name="Save")],
            should="(scene) the box, focus on its default button", tree=True):
        run.key("space")
    with run.act("AT-SPI click on 'Welcome' behind the box",
                 nothing_came_of_it(not_in_tree(role="alert", name="Welcome to Teksilo")),
                 should=NOTHING, tree=True, record=2.0):
        run.action("click", role="push button", name="Welcome")
    with run.act("AT-SPI click on 'Delete file?' behind the box",
                 nothing_came_of_it(not_in_tree(role="alert", name="Delete file?")),
                 should=NOTHING, tree=True, record=2.0):
        run.action("click", role="push button", name="Delete file?")
    with run.act("AT-SPI grab_focus on 'Delete file?' behind the box",
                 nothing_came_of_it(), should="focus stays on the box's Save", tree=True):
        run.grab_focus(role="push button", name="Delete file?")
    with run.act("Space, where focus is", [
            not_in_tree(role="alert"),
            event("object:text-changed:insert", text_contains="Save changes? → Save"),
            focused(role="push button", name="Save changes?"),
            said("Save changes? push button"), not_said("Delete file?")],
            should="the key presses the box's own Save: the box closes with that answer, "
                   "focus goes back to its opener and the reader hears it", tree=True):
        run.key("space")
    with run.act("Tab on along the page", [
            focused(role="push button", name="Delete file?"),
            said("Delete file? push button")],
            should="the page is as alive to the reader as before the box opened",
            tree=True):
        run.key("Tab")


def close(run):
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="(scene) the reader reaches the button"):
        run.key("Tab")
    with run.act("Space: the dialog opens", [count_in_tree("alert", 1),
                                             focused(role="push button", name="Save")],
                 should="(scene) one dialog, focus on Save", tree=True, record=3.0):
        run.key("space")
    with run.act("AT-SPI click on the checkbox behind the dialog",
                 nothing_came_of_it(box_state(True)),
                 should="the document stays dirty: the checkbox is not toggled", tree=True):
        run.action("click", **BOX)
    with run.act("AT-SPI click on 'Close window (ctx.close_window)' behind the dialog",
                 nothing_came_of_it(no_event("object:children-changed:add", role="frame")),
                 should="the guard does not run again: no second dialog", tree=True,
                 record=3.0):
        run.action("click", **CLOSE_BTN)
    with run.act("AT-SPI grab_focus on 'Open can_close-sugar window…' behind the dialog",
                 nothing_came_of_it(), should="focus stays on the dialog's Save", tree=True):
        run.grab_focus(**SUGAR_OPEN)
    with run.act("Escape: Cancel", [
            not_in_tree(role="alert"), count_in_tree("frame", 1), focused(**CLOSE_BTN),
            said("Close window (ctx.close_window) push button")],
            should="the dialog goes, no second window was opened behind it, and the "
                   "reader hears focus come back to the button", tree=True, record=3.0):
        run.key("Escape")
    with run.act("Shift+Tab to the checkbox", [
            focused(**BOX), said("check box checked"), box_state(True)],
            should="the checkbox is as it was before the dialog: checked", tree=True):
        run.key("Shift+Tab")


SCENARIOS = [
    Scenario("fix-modal-at-dialogs", "dialogs-and-popovers", dialogs,
             "AT-SPI click and grab_focus behind a modal MessageBox, then Space and Tab"),
    Scenario("fix-modal-at-close", "close-confirmation", close,
             "AT-SPI click and grab_focus behind the close-confirmation box, then Escape"),
]
