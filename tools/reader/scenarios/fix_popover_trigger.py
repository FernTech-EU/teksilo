# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""dialogs-and-popovers: the custom-trigger popover, by keyboard alone.

The sweep's `dialogs-popover` opens the popover with an AT-SPI click, the only
way in while its trigger was no Tab stop, and `verify-dialogs-popover-more`
records where Tab out of it goes without saying where it should. These walk
it with real keys: Tab to 'Show popover', Enter, then out of the popover by
Tab, by Shift+Tab, or by Escape; Tab on to the Dialog's custom trigger; and a
screen reader's own focus request on either trigger.

What each act expects follows from the example's order: the page's first
controls are the Theme box in the toolbar, then 'Show popover' (a
`PopoverWidget<OverlayTrigger>` around a `Panel`), then 'Open dialog' (a
`Dialog` with a custom `Panel` trigger). The popover holds labels and badges
and nothing that takes focus, so opening it puts focus on its dialog, named by
its trigger.

Each scenario opens the popover once. A second opening brings the popover's
nodes back under the ids the adapter already declared defunct, and Orca
ignores them; that is the node-ids topic, not this one.
"""

from __future__ import annotations

from reader_lib.checks import focused, in_tree, not_in_tree, said
from reader_lib.scenario import Scenario

PACKAGE = "dialogs-and-popovers"
OPENER = "Show popover"
NEXT = "Open dialog"
TEXT = "Use popovers"


def open_by_keyboard(run):
    run.wait_for(role="push button", name=OPENER)
    with run.act("Tab to the Theme box", [focused(role="combo box", name="Theme")]):
        run.key("Tab")
    with run.act("Tab to the popover's trigger",
                 [focused(role="push button", name=OPENER), said(OPENER)],
                 should="the popover's trigger is the Tab stop after the Theme box, "
                        "and the reader hears its name"):
        run.key("Tab")
    with run.act("Enter opens the popover",
                 [in_tree(role="dialog", name=OPENER),
                  focused(role="dialog", name=OPENER), said(TEXT)],
                 should="focus moves onto the popover, a dialog named by its trigger, "
                        "and the reader hears what it holds", tree=True):
        run.key("Return")


def tab_out(run):
    open_by_keyboard(run)
    with run.act("Tab from inside the popover",
                 [focused(role="push button", name=NEXT), said(NEXT),
                  not_in_tree(role="dialog", name=OPENER)],
                 should="Tab closes the popover and goes on to the control after its "
                        "trigger", tree=True):
        run.key("Tab")


def shift_tab_out(run):
    open_by_keyboard(run)
    with run.act("Shift+Tab from inside the popover",
                 [focused(role="push button", name=OPENER), said(OPENER),
                  not_in_tree(role="dialog", name=OPENER)],
                 should="Shift+Tab closes the popover and goes back to its trigger",
                 tree=True):
        run.key("Shift+Tab")


def escape_out(run):
    open_by_keyboard(run)
    with run.act("Escape closes it",
                 [focused(role="push button", name=OPENER), said(OPENER),
                  not_in_tree(role="dialog", name=OPENER)],
                 should="focus returns to the trigger and the reader names it", tree=True):
        run.key("Escape")


def dialog_trigger(run):
    """dialogs-02: the Dialog's custom trigger is focused as its named button."""
    title = "Review Changes"
    run.wait_for(role="push button", name=NEXT)
    with run.act("Tab to the Theme box", [focused(role="combo box", name="Theme")]):
        run.key("Tab")
    with run.act("Tab to the popover's trigger", [focused(role="push button", name=OPENER)]):
        run.key("Tab")
    with run.act("Tab to the dialog's trigger",
                 [focused(role="push button", name=NEXT), said(NEXT)],
                 should="focus lands on the trigger's named button, not on the panel it wraps"):
        run.key("Tab")
    with run.act("Enter opens the dialog",
                 [in_tree(role="dialog", name=title), focused(role="push button", name="Cancel"),
                  said(title)]):
        run.key("Return")
    with run.act("Escape closes it",
                 [focused(role="push button", name=NEXT), said(NEXT),
                  not_in_tree(role="dialog", name=title)],
                 should="focus returns to the trigger and the reader names it"):
        run.key("Escape")


def grab_focus(run):
    """What a screen reader's own focus request does on each custom trigger."""
    run.wait_for(role="push button", name="Show snackbar")
    run.grab_focus(role="push button", name="Show snackbar")
    # Long enough for Orca to finish reading it, so the act starts quiet.
    run.wait(2.5)
    with run.act("AT-SPI grab_focus on the popover's trigger",
                 [focused(role="push button", name=OPENER), said(OPENER)],
                 should="a screen reader can put focus on the trigger"):
        run.grab_focus(role="push button", name=OPENER)
    with run.act("AT-SPI grab_focus on the dialog's trigger",
                 [focused(role="push button", name=NEXT), said(NEXT)]):
        run.grab_focus(role="push button", name=NEXT)


SCENARIOS = [
    Scenario("fix-popover-trigger-tab-out", PACKAGE, tab_out,
             "the custom-trigger popover by keyboard: Tab in, Enter, Tab out"),
    Scenario("fix-popover-trigger-shift-tab-out", PACKAGE, shift_tab_out,
             "the custom-trigger popover by keyboard: Tab in, Enter, Shift+Tab out"),
    Scenario("fix-popover-trigger-escape", PACKAGE, escape_out,
             "the custom-trigger popover by keyboard: Tab in, Enter, Escape"),
    Scenario("fix-popover-trigger-dialog", PACKAGE, dialog_trigger,
             "the Dialog's custom trigger by keyboard: Tab to it, Enter, Escape"),
    Scenario("fix-popover-trigger-grab-focus", PACKAGE, grab_focus,
             "AT-SPI grab_focus on the popover's and the dialog's custom triggers"),
]
