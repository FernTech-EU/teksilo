# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""dialogs-and-popovers: MessageBox, Dialog, popover and snackbar as a reader
meets them.

The example (`examples/dialogs_and_popovers/src/main.rs`, no CLI arguments)
has one trigger for each overlay presentation Teksilo ships:

* five `MessageBox` triggers: question "Save changes?" (Save/Discard/Cancel,
  default Save, escape Cancel), critical "Delete file?" (Yes/No, default and
  escape No), critical "Could not open file" (Retry/Ignore/Abort, a "Show
  details" disclosure), information "Welcome" (Ok, a don't-show-again check
  box), question "How do I…?" (custom Help + OK). A `MessageBox` is a modal,
  assertive `Role::AlertDialog` named by its title and described by its text
  (`message_box.rs`, `Widget::accessibility`), wrapped in a `ModalContainer`
  that publishes no dialog of its own (`dialog.rs`,
  `ModalContainer::accessibility`), with its content `Live::Off` so only the
  title is announced;
* two `Dialog`s: "Adaptive modal window" (a stock `Button` trigger) and
  "Open dialog" (a custom `Panel` trigger through `OverlayTrigger`), each a
  `ModalContainer` `Role::Dialog` named through `labelled_by` by its
  `DialogContent` title;
* a `PopoverWidget<OverlayTrigger>` ("Show popover") whose surface is a
  non-modal `Role::Dialog` (`popover_surface.rs`), named by `surface_name`
  (empty by default, and the example sets none);
* a `Snackbar` ("Show snackbar"), a `Role::Alert` / `Live::Polite` surface
  that auto-dismisses after 2.5 s (`snackbar.rs`). The example sets no
  `.announcement(..)`, so the alert is named by the fallback "Snackbar".

None of these messages go through the framework announcer (`ctx.announce`):
the MessageBox title and the snackbar name are live-region nodes of their own.

On Linux every modal is presented in-tree: `supports_native_modal_windows()`
is false on every unix but macOS (`teksilo-platform/src/window_system.rs`),
so `ModalPresentation::Auto` resolves to `InTree` (`teksilo-app/src/app.rs`,
`resolve_modal_presentation`). `accesskit_atspi_common` 0.20 maps neither
`expanded` nor `has_popup` to AT-SPI (node.rs `state()`), so no disclosure
state reaches Orca; the checks that read it record that gap.
"""

from __future__ import annotations

from reader_lib.checks import (announced, custom, event, focused, in_tree, no_event,
                               not_announced, not_in_tree, not_said, said, said_once)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario

PACKAGE = "dialogs-and-popovers"

#: AT-SPI roles a reader treats as a dialog.
DIALOG_ROLES = {"dialog", "alert", "file chooser", "color chooser", "font chooser"}


# ---------------------------------------------------------------------------
# Tree helpers
# ---------------------------------------------------------------------------


def _walk(tree):
    if not tree:
        return
    stack = [tree]
    while stack:
        node = stack.pop()
        yield node
        stack.extend(reversed(node.get("children", [])))


def _path_to(tree, pred):
    """The nodes from the root down to the first node `pred` accepts."""
    if not tree:
        return None
    if pred(tree):
        return [tree]
    for child in tree.get("children", []):
        found = _path_to(child, pred)
        if found:
            return [tree] + found
    return None


def _outline(node) -> str:
    return f"[{node.get('role')}] {node.get('name')!r}"


def one_dialog_around(role: str, name: str, *, dialog_role: str | None = None,
                      dialog_name: str | None = None):
    """The control sits under exactly one dialog node (no dialog-in-dialog),
    and that dialog has the given role and name."""
    def run(act):
        path = _path_to(act.tree, lambda n: n.get("role") == role
                        and (n.get("name") or "") == name)
        if not path:
            return False, [f"no [{role}] {name!r} in the tree after the act"]
        dialogs = [n for n in path if n.get("role") in DIALOG_ROLES]
        evidence = ["path: " + " > ".join(_outline(n) for n in path)]
        ok = len(dialogs) == 1
        if ok and dialog_role is not None:
            ok = dialogs[0].get("role") == dialog_role
        if ok and dialog_name is not None:
            ok = (dialogs[0].get("name") or "") == dialog_name
        return ok, evidence
    want = f"[{dialog_role or 'dialog'}] {dialog_name!r}" if dialog_name else "one dialog"
    return custom(f"[{role}] {name!r} sits under exactly one dialog node, {want}", run,
                  needs_tree=True)


def dialog_holds(dialog_role: str, dialog_name: str, *texts: str):
    """Walking the dialog, a reader finds each text (as a node's name, text or
    description)."""
    def run(act):
        dialog = next((n for n in _walk(act.tree) if n.get("role") == dialog_role
                       and (n.get("name") or "") == dialog_name), None)
        if dialog is None:
            return False, [f"no [{dialog_role}] {dialog_name!r} in the tree"]
        found, missing = [], []
        for text in texts:
            hit = next((n for n in _walk(dialog)
                        if text in (n.get("name") or "")
                        or text in ((n.get("text") or {}).get("text") or "")), None)
            (found if hit else missing).append(
                f"{text!r}: {_outline(hit)}" if hit else f"{text!r}: not reachable")
        return not missing, found + missing
    return custom(f"walking [{dialog_role}] {dialog_name!r} reaches {list(texts)}", run,
                  needs_tree=True)


def described(dialog_role: str, dialog_name: str, *parts: str):
    def run(act):
        dialog = next((n for n in _walk(act.tree) if n.get("role") == dialog_role
                       and (n.get("name") or "") == dialog_name), None)
        if dialog is None:
            return False, [f"no [{dialog_role}] {dialog_name!r} in the tree"]
        desc = dialog.get("description") or ""
        return all(p in desc for p in parts), [f"description={desc!r}"]
    return custom(f"[{dialog_role}] {dialog_name!r} is described by {list(parts)}", run,
                  needs_tree=True)


def only_announcements(*texts: str):
    """The act's `object:announcement`s are exactly these texts, in any order."""
    def run(act):
        got = [e.get("text") or "" for e in act.events if e["type"] == "object:announcement"]
        return sorted(got) == sorted(texts), [f"announcements: {got!r}"]
    return custom(f"the only announcements are {list(texts)}", run)


def no_new_window():
    """Nothing in the act added a window (a frame/dialog/alert under the
    application): on Linux a modal is presented in the same window."""
    def run(act):
        added = [e for e in act.events if e["type"] == "object:children-changed:add"
                 and e.get("source", {}).get("role") == "application"]
        return not added, [f"{e['type']} -> {e.get('target')}" for e in added] or \
            ["no window added under the application"]
    return custom("the modal is presented inside the same window, no new window", run)


def states_of(role: str, name: str, *, want: str | None = None, absent: str | None = None):
    """Record a node's AT-SPI states; pass when `want` is among them and
    `absent` is not."""
    def run(act):
        node = next((n for n in _walk(act.tree) if n.get("role") == role
                     and (n.get("name") or "") == name), None)
        if node is None:
            return False, [f"no [{role}] {name!r}"]
        states = node.get("states", [])
        ok = (want is None or want in states) and (absent is None or absent not in states)
        return ok, [f"states={states}", f"attributes={node.get('attributes')}"]
    what = ", ".join(x for x in (f"has {want}" if want else "",
                                  f"lacks {absent}" if absent else "") if x)
    return custom(f"[{role}] {name!r} {what}", run, needs_tree=True)


def label_now(text: str):
    """The 'Last result' label reads `text` after the act."""
    return in_tree(role="label", name_contains=text)


def focus_record():
    """Record where the act's last focus change went (always passes)."""
    def run(act):
        from reader_lib.checks import _focus_node, _is_focus
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return True, ["no focus change in the act"]
        node = _focus_node(moves[-1])
        return True, [f"focus ended on [{node.get('role')}] {node.get('name')!r} "
                      f"path={node.get('path')}"]
    return custom("where focus went (a record, not a judgement)", run)


def said_nothing_but(*allowed: str):
    """Orca said nothing in the act beyond utterances containing one of
    `allowed` (to catch unexpected speech)."""
    def run(act):
        extra = [u.text for u in utterances(act.orca)
                 if not any(a.lower() in u.text.lower() for a in allowed)]
        return not extra, [f"Orca also said: {t!r}" for t in extra] or ["nothing else"]
    return custom(f"Orca says nothing beyond {list(allowed)}", run, needs_orca=True)


# ---------------------------------------------------------------------------
# Acts shared by the scenarios
# ---------------------------------------------------------------------------


def focus_opener(run, name: str) -> None:
    """Put focus on a trigger through AT-SPI, outside any act."""
    run.wait_for(role="push button", name=name)
    run.grab_focus(role="push button", name=name)
    run.wait(1.0)


def box_opens(run, label: str, *, title: str, default: str, desc: list[str],
              holds: list[str], role: str = "alert", key: str = "space", extra=()):
    with run.act(label,
                 [announced(title), focused(role="push button", name=default),
                  said(default), said(title), said_once(title),
                  in_tree(role=role, name=title, state="modal"),
                  one_dialog_around("push button", default, dialog_role=role,
                                    dialog_name=title),
                  described(role, title, *desc),
                  dialog_holds(role, title, *desc, *holds),
                  only_announcements(title), no_new_window(), *extra],
                 should="the reader hears the title once, the text, then the focused "
                        "default button; the text is the box's description and reachable "
                        "by walking; nothing else is announced"):
        run.key(key)


# ---------------------------------------------------------------------------
# MessageBox: "Save changes?" (question, Save/Discard/Cancel, default Save,
# escape Cancel)
# ---------------------------------------------------------------------------


SAVE_TITLE = "Save changes?"
SAVE_TEXT = "You have unsaved changes in report.skrib."
SAVE_INFO = "Your changes will be lost if you don't save them."


def save_changes(run):
    opener = "Save changes?"
    focus_opener(run, opener)
    box_opens(run, "Space on 'Save changes?' opens the box", title=SAVE_TITLE,
              default="Save", desc=[SAVE_TEXT, SAVE_INFO], holds=["Save", "Discard", "Cancel"])
    with run.act("Enter answers with the default, Save",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="alert"),
                  label_now("Save changes? → Save (dismissal=Button)")],
                 should="the box closes, the answer is Save, and focus goes back to the "
                        "opener, which the reader says"):
        run.key("Return")
    box_opens(run, "Space opens it a second time", title=SAVE_TITLE, default="Save",
              desc=[SAVE_TEXT, SAVE_INFO], holds=["Save", "Discard", "Cancel"])
    with run.act("Escape answers with the escape button, Cancel",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="alert"),
                  label_now("Save changes? → Cancel (dismissal=Escape)")],
                 should="Escape closes the box as Cancel and focus returns to the opener"):
        run.key("Escape")
    with run.act("Space opens it a third time", [focused(role="push button", name="Save")]):
        run.key("space")
    with run.act("Tab from Save", [focused(role="push button", name="Discard"),
                                   said("Discard")],
                 should="Tab moves to the next button of the box"):
        run.key("Tab")
    with run.act("Tab again", [focused(role="push button", name="Cancel"), said("Cancel")]):
        run.key("Tab")
    with run.act("Tab from the last button wraps inside the box",
                 [focused(role="push button", name="Save"), said("Save")],
                 should="Tab stays inside the modal box"):
        run.key("Tab")
    with run.act("Shift+Tab wraps back to Cancel",
                 [focused(role="push button", name="Cancel")]):
        run.key("Shift+Tab")
    with run.act("Shift+Tab to Discard", [focused(role="push button", name="Discard")]):
        run.key("Shift+Tab")
    with run.act("Enter on the focused Discard answers Discard",
                 [focused(role="push button", name=opener), said(opener),
                  label_now("Save changes? → Discard (dismissal=Button)")],
                 should="Enter on a focused non-default button answers with that button"):
        run.key("Return")


# ---------------------------------------------------------------------------
# MessageBox: "Delete file?" (critical, No/Yes, default No, escape No)
# ---------------------------------------------------------------------------


def delete_file(run):
    opener = "Delete file?"
    title = "Delete file?"
    text = "Permanently delete report.skrib? This action cannot be undone."
    focus_opener(run, opener)
    box_opens(run, "Space on 'Delete file?' opens the critical box", title=title,
              default="No", desc=[text], holds=["No", "Yes"])
    with run.act("Enter answers No",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="alert"),
                  label_now("Delete file? → No (dismissal=Button)")],
                 should="Enter takes the safe default and focus returns to the opener"):
        run.key("Return")
    with run.act("Space opens it again", [focused(role="push button", name="No")]):
        run.key("space")
    with run.act("Tab to Yes", [focused(role="push button", name="Yes"), said("Yes")],
                 should="Tab moves inside the box"):
        run.key("Tab")
    with run.act("Enter on Yes answers Yes",
                 [focused(role="push button", name=opener),
                  label_now("Delete file? → Yes (dismissal=Button)")],
                 should="the focused button answers, not the default"):
        run.key("Return")
    with run.act("Space opens it a third time", [focused(role="push button", name="No")]):
        run.key("space")
    with run.act("Escape answers No",
                 [focused(role="push button", name=opener), said(opener),
                  label_now("Delete file? → No (dismissal=Escape)")],
                 should="Escape still closes a critical box, as No"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# MessageBox: "Could not open file" (critical, Abort/Ignore/Retry, details)
# ---------------------------------------------------------------------------


def could_not_open(run):
    opener = "Could not open file"
    title = "Could not open file"
    text = "report.skrib could not be opened."
    info = "You may not have permission."
    detail = "EACCES"
    focus_opener(run, opener)
    box_opens(run, "Space opens the error box", title=title, default="Retry",
              desc=[text, info], holds=["Show details", "Abort", "Ignore", "Retry"],
              extra=[not_in_tree(name_contains=detail)])
    with run.act("Tab from Retry wraps to the Show details toggle",
                 [focused(name="Show details"), said("Show details"), said("collapsed"),
                  states_of("push button", "Show details", want="expandable")],
                 should="the details disclosure is reachable and says it is collapsed",
                 tree=True):
        run.key("Tab")
    with run.act("Space expands the details",
                 [said("expanded"),
                  states_of("push button", "Show details", want="expanded"),
                  dialog_holds("alert", title, detail),
                  no_event("object:state-changed:focused")],
                 should="the reader hears it expand and the details become reachable",
                 tree=True):
        run.key("space")
    with run.act("Tab from the expanded toggle", [focus_record()],
                 should="where the next Tab goes once the details are shown", tree=True):
        run.key("Tab")
    with run.act("Escape answers Abort",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="alert"),
                  label_now("Could not open → Abort (dismissal=Escape)")],
                 should="Escape closes the box as Abort and focus returns to the opener"):
        run.key("Escape")


# ---------------------------------------------------------------------------
# MessageBox: "Welcome to Teksilo" (information, Ok, don't-show-again box)
# ---------------------------------------------------------------------------


def welcome(run):
    opener = "Welcome"
    title = "Welcome to Teksilo"
    text = "This demo showcases the MessageBox pipeline across severities."
    focus_opener(run, opener)
    box_opens(run, "Space opens the information box", title=title, default="OK",
              desc=[text], holds=["Don't show this again", "OK"])
    with run.act("Tab to the check box", [focused(role="check box"),
                                          said("Don't show this again"),
                                          said("not checked")],
                 should="the don't-show-again box is a Tab stop with its label and state"):
        run.key("Tab")
    with run.act("Space checks it",
                 [event("object:state-changed:checked", role="check box"),
                  in_tree(role="check box", state="checked"), said("checked")],
                 should="the reader hears the new state and the box reads checked",
                 record=4.0, tree=True):
        run.key("space")
    with run.act("Tab to OK", [focused(role="push button", name="OK"),
                               no_event("object:state-changed:checked"),
                               not_said("checked")],
                 should="only the focus moves; the box's state was published when it changed",
                 tree=True):
        run.key("Tab")
    with run.act("Shift+Tab back to the check box",
                 [focused(role="check box"), said("checked")],
                 should="the reader hears the box as checked"):
        run.key("Shift+Tab")
    with run.act("AT-SPI click unchecks it",
                 [event("object:state-changed:checked", role="check box"),
                  said("not checked")],
                 should="a screen reader's own activation toggles it and the state follows",
                 record=4.0, tree=True):
        run.action("click", role="check box")
    with run.act("Enter on the check box answers OK",
                 [focused(role="push button", name=opener), said(opener),
                  label_now("Welcome → Ok (checkbox=false, dismissal=Button)")],
                 should="Enter, with focus on a non-button, takes the default OK"):
        run.key("Return")


# ---------------------------------------------------------------------------
# MessageBox: "How do I…?" (question, custom Help + OK)
# ---------------------------------------------------------------------------


def custom_buttons(run):
    opener = "Custom buttons"
    title = "How do I…?"
    text = "Open help or dismiss with OK."
    focus_opener(run, opener)
    box_opens(run, "Space opens the custom-button box", title=title, default="OK",
              desc=[text], holds=["Help", "OK"])
    with run.act("Tab to Help", [focused(role="push button", name="Help"), said("Help")]):
        run.key("Tab")
    with run.act("Space on Help answers Help",
                 [focused(role="push button", name=opener),
                  label_now("Custom buttons → Help (dismissal=Button)")]):
        run.key("space")
    with run.act("Space opens it again", [focused(role="push button", name="OK")]):
        run.key("space")
    with run.act("Escape answers OK",
                 [focused(role="push button", name=opener),
                  label_now("Custom buttons → Ok (dismissal=Escape)")]):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Dialog with a stock Button trigger: "Adaptive modal window"
# ---------------------------------------------------------------------------


def adaptive_dialog(run):
    opener = "Adaptive modal window"
    title = "Adaptive modal dialog"
    supporting = "The framework chooses the best modal presentation"
    body = "The app code does not branch on Wayland"
    focus_opener(run, opener)
    with run.act("Space opens the dialog",
                 [in_tree(role="dialog", name=title, state="modal"),
                  one_dialog_around("push button", "Close", dialog_role="dialog",
                                    dialog_name=title),
                  dialog_holds("dialog", title, supporting, body, "Close"),
                  focused(role="push button", name="Close"),
                  said(title), said_once(title), said("Close"), no_new_window(),
                  states_of("push button", opener, want="expanded")],
                 should="focus moves into the dialog; the reader hears its title and "
                        "text once, then the focused button; the opener says it is expanded"):
        run.key("space")
    with run.act("Tab stays in the dialog",
                 [in_tree(role="push button", name="Close", state="focused"),
                  no_event("object:state-changed:focused", name_contains=opener)],
                 should="the only control is Close, so Tab stays on it", tree=True):
        run.key("Tab")
    with run.act("Escape closes it",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="dialog", name=title)],
                 should="focus returns to the opener"):
        run.key("Escape")
    with run.act("Enter on the opener opens it",
                 [focused(role="push button", name="Close"), said(title)],
                 should="Enter opens the dialog as Space does"):
        run.key("Return")
    with run.act("Enter on Close closes it, and it stays closed",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="dialog", name=title)],
                 should="the dialog closes and focus is back on the opener; the key's "
                        "release on the opener does not open it again", record=3.0,
                 tree=True):
        run.key("Return")


# ---------------------------------------------------------------------------
# Dialog with a custom Panel trigger: "Open dialog" / "Review Changes"
# ---------------------------------------------------------------------------


def review_dialog(run):
    opener = "Open dialog"
    title = "Review Changes"
    supporting = "Dialogs open centered"
    body = "This helper gives dialogs a consistent header"
    run.wait_for(role="push button", name="Save changes?")
    with run.act("Tab to the Theme combo box", [focused(role="combo box", name="Theme")]):
        run.key("Tab")
    with run.act("Tab reaches the 'Open dialog' trigger",
                 [focused(role="push button", name=opener), said(opener)],
                 should="Tab lands on the trigger and the reader says 'Open dialog, button'"):
        run.key("Tab")
    with run.act("Enter opens the dialog",
                 [in_tree(role="dialog", name=title, state="modal"),
                  one_dialog_around("push button", "Cancel", dialog_role="dialog",
                                    dialog_name=title),
                  dialog_holds("dialog", title, supporting, body, "Cancel", "Apply"),
                  focused(role="push button", name="Cancel"),
                  said(title), said_once(title), said("Cancel")],
                 should="focus moves into the dialog and the reader hears its title"):
        run.key("Return")
    with run.act("Tab to Apply", [focused(role="push button", name="Apply"), said("Apply")]):
        run.key("Tab")
    with run.act("Tab wraps to Cancel", [focused(role="push button", name="Cancel")]):
        run.key("Tab")
    with run.act("Escape closes it",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="dialog", name=title)],
                 should="focus returns to the trigger and the reader names it"):
        run.key("Escape")
    with run.act("AT-SPI click on the trigger opens it",
                 [in_tree(role="dialog", name=title),
                  focused(role="push button", name="Cancel"), said(title)],
                 should="a screen reader's own activation opens the dialog too"):
        run.action("click", role="push button", name=opener)
    with run.act("Enter on Cancel closes it",
                 [focused(role="push button", name=opener), said(opener),
                  not_in_tree(role="dialog", name=title)]):
        run.key("Return")


# ---------------------------------------------------------------------------
# Popover: "Show popover"
# ---------------------------------------------------------------------------


def popover(run):
    opener = "Show popover"
    node = run.wait_for(role="push button", name=opener)
    run.note(f"the popover trigger at launch: states={node.get('states')} "
             f"actions={node.get('actions')}")
    with run.act("Tab twice from the window",
                 [focused(role="push button", name=opener)],
                 should="the first control of the page, the popover trigger, is a Tab stop "
                        "after the Theme box"):
        run.key("Tab", "Tab")
    with run.act("AT-SPI grab_focus on the popover trigger",
                 [focused(role="push button", name=opener)],
                 should="a screen reader can at least put focus on it"):
        run.grab_focus(role="push button", name=opener)
    with run.act("AT-SPI click on the trigger opens the popover",
                 [in_tree(role="dialog"),
                  custom("the popover's dialog has a name",
                         lambda act: (any(n.get("role") == "dialog" and n.get("name")
                                          for n in _walk(act.tree)),
                                      [_outline(n) for n in _walk(act.tree)
                                       if n.get("role") == "dialog"]), needs_tree=True),
                  dialog_holds("dialog", "", "Use popovers for compact contextual actions",
                               "Quick actions"),
                  focus_record(),
                  custom("focus lands inside the popover's dialog",
                         lambda act: _focus_inside(act, "dialog"), needs_tree=True),
                  said("Use popovers")],
                 should="the popover appears, focus moves into it (or the reader is told), "
                        "and the reader hears what it holds", tree=True):
        run.action("click", role="push button", name=opener)
    with run.act("Escape closes it",
                 [not_in_tree(role="dialog"), focus_record()],
                 should="the popover closes and focus returns to where it was", tree=True):
        run.key("Escape")
    with run.act("AT-SPI click opens it a second time",
                 [in_tree(role="dialog"), focus_record(), said("Use popovers")],
                 should="the same popover, the same reading, again", tree=True):
        run.action("click", role="push button", name=opener)
    with run.act("Escape closes it again", [not_in_tree(role="dialog")], tree=True):
        run.key("Escape")


def _focus_inside(act, role):
    from reader_lib.checks import _focus_node, _is_focus
    moves = [e for e in act.events if _is_focus(e)]
    if not moves:
        return False, ["no focus change in the act"]
    target = _focus_node(moves[-1]).get("path")
    path = _path_to(act.tree, lambda n: n.get("path") == target)
    if not path:
        return False, [f"focused path {target} not in the tree"]
    return any(n.get("role") == role for n in path), \
        ["path: " + " > ".join(_outline(n) for n in path)]


# ---------------------------------------------------------------------------
# Snackbar: "Show snackbar"
# ---------------------------------------------------------------------------


def snackbar(run):
    """Three showings, each left to time out."""
    opener = "Show snackbar"
    focus_opener(run, opener)
    for n in (1, 2, 3):
        with run.act(f"Space shows the snackbar, showing {n}",
                     [announced("Autosave complete"), said("Autosave complete"),
                      said("Snackbar"),
                      not_announced("Dismiss"),
                      in_tree(role="notification"),
                      dialog_holds("notification", "Snackbar", "Autosave complete",
                                   "Dismiss")],
                     should="the reader hears the message, politely, while focus stays "
                            "on the opener",
                     record=1.0, tree=True):
            run.key("space")
        with run.act(f"showing {n} times out",
                     [not_in_tree(role="notification"),
                      no_event("object:state-changed:focused")],
                     should="it goes away; focus does not move", record=2.5, tree=True):
            pass


def snackbar_focus(run):
    """Show it, Tab on to the next control, and let it time out."""
    opener = "Show snackbar"
    focus_opener(run, opener)
    with run.act("Space shows the snackbar", [in_tree(role="notification")],
                 record=0.3, tree=True):
        run.key("space")
    with run.act("Tab moves on while it is up",
                 [focused(role="push button", name="Adaptive modal window")],
                 settle=0.1, record=0.3):
        run.key("Tab")
    with run.act("it times out while focus is on the next control",
                 [no_event("object:state-changed:focused"),
                  not_in_tree(role="notification"),
                  in_tree(role="push button", name="Adaptive modal window",
                          state="focused"),
                  not_said("Show snackbar")],
                 should="the snackbar leaves; the reader's focus stays where they put it",
                 settle=0.1, record=3.0, tree=True):
        pass


def snackbar_reach(run):
    """Where its Dismiss button sits in the Tab order while it is up, and what
    the time-out does to a reader standing on it.

    The 2.5 s time-out keeps running while the harness waits for Orca to go
    idle after the first act, so on a loaded machine it can fire during the
    Tab act rather than the one after it; either way it fires with focus on
    Dismiss, which is what this scenario is about."""
    run.wait_for(role="push button", name="Custom buttons")
    run.grab_focus(role="push button", name="Custom buttons")
    run.wait(1.0)
    with run.act("AT-SPI click on 'Show snackbar' while focus is on 'Custom buttons'",
                 [in_tree(role="notification")], record=0.3, tree=True):
        run.action("click", role="push button", name="Show snackbar")
    with run.act("Tab from the last page control",
                 [focused(role="push button", name="Dismiss"), said("Dismiss")],
                 should="the snackbar's action is a Tab stop",
                 settle=0.1, record=0.4):
        run.key("Tab")
    with run.act("focus stays on Dismiss while the time-out passes",
                 [no_event("object:state-changed:focused"),
                  in_tree(role="push button", name="Dismiss", state="focused")],
                 should="the snackbar waits while the reader is on its action (or, if it "
                        "goes, focus goes somewhere the reader expects)",
                 settle=0.1, record=3.0, tree=True):
        pass


# ---------------------------------------------------------------------------
# A modal box and the page behind it
# ---------------------------------------------------------------------------


def modal_inert(run):
    """While a MessageBox is modal, a reader's own activation of a control
    behind it should do nothing: the box is modal."""
    focus_opener(run, "Save changes?")
    with run.act("Space opens 'Save changes?'", [focused(role="push button", name="Save")],
                 tree=True):
        run.key("space")
    with run.act("AT-SPI click on 'Welcome' behind the modal box",
                 [not_in_tree(role="alert", name="Welcome to Teksilo"),
                  in_tree(role="alert", name="Save changes?"),
                  no_event("object:state-changed:focused")],
                 should="nothing happens: the page behind a modal box is inert",
                 tree=True):
        run.action("click", role="push button", name="Welcome")
    with run.act("AT-SPI grab_focus on 'Delete file?' behind the boxes",
                 [no_event("object:state-changed:focused")],
                 should="focus stays in the modal box", tree=True):
        run.grab_focus(role="push button", name="Delete file?")
    with run.act("Real Space on the control behind the boxes",
                 [only_announcements(), focus_record()],
                 should="the key reaches whatever holds focus; if that is behind the "
                        "modal, a third box must not open", tree=True):
        run.key("space")
    with run.act("Escape", [focus_record()], should="whatever is on top closes", tree=True):
        run.key("Escape")
    with run.act("Escape again", [focus_record()], should="and the next", tree=True):
        run.key("Escape")
    with run.act("Escape a third time", [focus_record()], tree=True):
        run.key("Escape")


SCENARIOS = [
    Scenario("dialogs-save-changes", PACKAGE, save_changes,
             "the 'Save changes?' MessageBox: open, Enter, Escape, Tab cycle, Enter on Discard"),
    Scenario("dialogs-delete-file", PACKAGE, delete_file,
             "the critical 'Delete file?' MessageBox: default No, Yes by Tab, Escape"),
    Scenario("dialogs-could-not-open", PACKAGE, could_not_open,
             "the critical error MessageBox with a Show details disclosure"),
    Scenario("dialogs-welcome", PACKAGE, welcome,
             "the information MessageBox with a don't-show-again check box"),
    Scenario("dialogs-custom-buttons", PACKAGE, custom_buttons,
             "the question MessageBox with custom Help and OK buttons"),
    Scenario("dialogs-adaptive-dialog", PACKAGE, adaptive_dialog,
             "the Dialog opened from a stock Button trigger"),
    Scenario("dialogs-review-dialog", PACKAGE, review_dialog,
             "the Dialog opened from a custom Panel trigger"),
    Scenario("dialogs-popover", PACKAGE, popover,
             "the custom-trigger popover: reachability, opening, closing"),
    Scenario("dialogs-snackbar", PACKAGE, snackbar,
             "the snackbar shown three times, each left to time out"),
    Scenario("dialogs-snackbar-focus", PACKAGE, snackbar_focus,
             "the snackbar times out after focus has moved on"),
    Scenario("dialogs-snackbar-reach", PACKAGE, snackbar_reach,
             "the snackbar's Dismiss button by Tab, and the time-out while on it"),
    Scenario("dialogs-modal-inert", PACKAGE, modal_inert,
             "activating and focusing controls behind a modal MessageBox through AT-SPI"),
]
