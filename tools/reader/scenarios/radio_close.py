# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""radio-tile and close-confirmation, as a screen reader user meets them.

radio-tile (`examples/radio_tile`): three `RadioTileGroup`s. The group is the
one Tab stop and holds focus; it publishes the selected tile as its
`active_descendant`, which `accesskit_consumer` resolves as the focus, so a
reader's focus lands on the tile itself. Each tile sets `position_in_set`
(1-based through `AccessNodeBuilder`, zero-based in AccessKit, +1 again on
AT-SPI) and the group sets `size_of_set` (`radio_tile.rs` `accessibility`,
`radio_tile_group.rs` `accessibility`). Each tile also declares its siblings
with `push_to_radio_group`, which `accesskit_atspi_common` 0.20 does not
export (its `relation_set` carries `ControllerFor` only), so Orca finds no
MEMBER_OF relation: its `newRadioButtonGroup` (speech_generator.py) repeats
the group's name on every arrow, and its where-am-I `positionInGroup` has no
members to count.

close-confirmation (`examples/close_confirmation`): the main window's close
guard vetoes a close while the document is dirty and presents a Save /
Discard / Cancel `MessageBox` (`Role::AlertDialog`, assertive live, named by
its title, the text as its description, Save the default and initial focus,
Cancel the Escape answer). On Linux a message box is presented in the
window's own tree. The second window uses the `can_close` +
`on_close_blocked` sugar with a Yes/No box whose default and Escape answer are
both No.

A close the compositor asks for (the close button, the window menu, Alt+F4)
reaches `WindowManager::request_close`; `post_event` then drains the modal
queue *before* `process_pending` runs the guard (`teksilo-app/src/app.rs`,
`post_event`), so the message box the guard presents waits in the queue for
the next event of any kind. `ask_to_close` below states that it should come
at once, and when it does not, presses Shift (a key that does nothing) so the
rest of the scenario still has its dialog. Alt+F4 cannot be driven here: the
private KWin runs no global-shortcut service, so the keys reach the example
and nothing asks for a close; `run.close_window()` is the compositor's route.

`checkbox_toggle` covers the one other control a reader meets in
close-confirmation: its checked state reaches the bus only when something else
re-walks the tree (`Checkbox` binds nothing at `AccessibilityOnly`).
"""

from __future__ import annotations

from reader_lib.checks import (Check, custom, event, focused, in_tree, no_event,
                               not_in_tree, not_said, said)
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario

RADIO = "radio button"


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


def positions(expected: dict[str, tuple[str, str]]) -> Check:
    """Each named radio button carries AT-SPI `posinset` / `setsize` as given."""
    def run(act):
        evidence, ok = [], True
        for name, (pos, size) in expected.items():
            node = next((n for n in _walk(act.tree)
                         if n.get("role") == RADIO and n.get("name") == name), None)
            if node is None:
                ok = False
                evidence.append(f"[radio button] {name!r} is not in the tree")
                continue
            attrs = node.get("attributes") or {}
            got = (attrs.get("posinset"), attrs.get("setsize"))
            if got != (pos, size):
                ok = False
            evidence.append(f"[radio button] {name!r} posinset={got[0]} setsize={got[1]} "
                            f"(want {pos} of {size})")
        return ok, evidence
    return custom("each tile says its position: " + ", ".join(
        f"{n}={p}/{s}" for n, (p, s) in expected.items()), run, needs_tree=True)


def member_of(name: str) -> Check:
    """The tile exports its radio group as an AT-SPI MEMBER_OF relation, which
    is what Orca 46.1 reads for group membership and "N of M"."""
    def run(act):
        node = next((n for n in _walk(act.tree)
                     if n.get("role") == RADIO and n.get("name") == name), None)
        if node is None:
            return False, [f"[radio button] {name!r} is not in the tree"]
        rels = node.get("relations") or {}
        return "member-of" in rels, [f"[radio button] {name!r} relations={rels!r}"]
    return custom(f"[radio button] {name!r} has a member-of relation", run, needs_tree=True)


def group_holds(group: str, members: list[str]) -> Check:
    """The named radio group (a panel on AT-SPI) holds exactly these tiles."""
    def run(act):
        node = next((n for n in _walk(act.tree)
                     if n.get("role") == "panel" and n.get("name") == group), None)
        if node is None:
            return False, [f"no [panel] {group!r} in the tree"]
        kids = [c.get("name") for c in node.get("children", []) if c.get("role") == RADIO]
        return kids == members, [f"[panel] {group!r} holds {kids}"]
    return custom(f"the group {group!r} holds {members}", run, needs_tree=True)


def disabled(role: str, name: str) -> Check:
    def run(act):
        node = next((n for n in _walk(act.tree)
                     if n.get("role") == role and n.get("name") == name), None)
        if node is None:
            return False, [f"[{role}] {name!r} is not in the tree"]
        states = node.get("states", [])
        return ("enabled" not in states and "sensitive" not in states), [f"states={states}"]
    return custom(f"[{role}] {name!r} is disabled (no enabled / sensitive)", run,
                  needs_tree=True)


def count_in_tree(role: str, count: int) -> Check:
    def run(act):
        found = [n for n in _walk(act.tree) if n.get("role") == role]
        return len(found) == count, [f"[{n.get('role')}] {n.get('name')!r}" for n in found] \
            or [f"no [{role}] in the tree"]
    return custom(f"the tree holds exactly {count} [{role}]", run, needs_tree=True)


def said_in_order(*texts: str) -> Check:
    """Orca said each text, in this order, none of them cut."""
    def run(act):
        spoken = utterances(act.orca)
        evidence = [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in spoken]
        at = 0
        for text in texts:
            want = normalized(text)
            # An uncut utterance holding the text: a cut one was not heard.
            while at < len(spoken) and (spoken[at].cut
                                        or want not in normalized(spoken[at].text)):
                at += 1
            if at == len(spoken):
                return False, [f"missing, out of order or only heard cut: {text!r}"] + evidence
            at += 1
        return True, evidence
    return custom("Orca says, in order: " + " / ".join(repr(t) for t in texts), run,
                  needs_orca=True)


def said_count(text: str, count: int) -> Check:
    def run(act):
        heard = [u for u in utterances(act.orca) if normalized(text) in normalized(u.text)]
        return len(heard) == count, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                                     for u in heard] or [f"never said {text!r}"]
    return custom(f"Orca says {text!r} exactly {count} time(s)", run, needs_orca=True)


def close_only(run, caption: str) -> None:
    """Ask the private KWin to close the one window whose caption holds
    `caption`, as its close button would. `run.close_window()` closes every
    window of the process, which asks two guards at once when the sugar window
    is open."""
    assert run.app is not None
    run._step(f"close the window {caption!r} through KWin")
    script = run.work / f"close-{abs(hash(caption))}-{len(run.acts)}.js"
    script.write_text(
        "const wins = workspace.windowList ? workspace.windowList() : workspace.clientList();\n"
        f"wins.forEach(w => {{ if (w.pid === {run.app.pid} && "
        f"String(w.caption).indexOf({caption!r}) >= 0) w.closeWindow(); }});\n",
        encoding="utf-8")
    run._kwin_script(script)


def note_alive(run, label: str) -> None:
    run.note(f"{label}: the application is "
             + ("still running" if run.alive() else f"gone (exit {run.app.returncode})"))


# ---------------------------------------------------------------------------
# radio-tile
# ---------------------------------------------------------------------------


def tab_into(run, stops: list[tuple[str, str]]) -> None:
    """Tab once per stop, one act a press, each landing on the named tile."""
    for i, (name, group) in enumerate(stops, 1):
        with run.act(f"Tab {i}: {group}", [focused(role=RADIO, name=name)],
                     should=f"focus enters {group!r} on its selected tile {name!r}"):
            run.key("Tab")


def tile_tree(run):
    run.wait_for(role=RADIO, name="Single file")
    with run.act("read the tree at rest", [
            group_holds("Project format", ["Single file", "Bundle"]),
            group_holds("Template", ["None", "Empty Novel", "Light Novel", "Novel", "Notebook"]),
            group_holds("Publication stage", ["Draft", "Review", "Published", "Archived"]),
            positions({"Single file": ("1", "2"), "Bundle": ("2", "2"),
                       "None": ("1", "5"), "Light Novel": ("3", "5"), "Novel": ("4", "5"),
                       "Notebook": ("5", "5"), "Draft": ("1", "4"), "Published": ("3", "4"),
                       "Archived": ("4", "4")}),
            in_tree(role=RADIO, name="Single file", state="checked"),
            in_tree(role=RADIO, name="Novel", state="checked"),
            in_tree(role=RADIO, name="Draft", state="checked"),
            in_tree(role=RADIO, name="Bundle", description_contains="Friendlier to version"),
            in_tree(role=RADIO, name="Light Novel", description_contains="15 chapters"),
            disabled(RADIO, "Archived"),
            member_of("Single file"),
            member_of("Novel")],
            should="every tile is a radio button named by its title, described by its "
                   "description or meta, in a group named by its label, with its position "
                   "and its group membership"):
        pass


def tile_row(run):
    run.wait_for(role=RADIO, name="Single file")
    with run.act("Tab into Project format", [
            focused(role=RADIO, name="Single file"),
            said_in_order("Project format", "Single file", "selected radio button",
                          "One .skrib archive")],
            should="the reader hears the group, the selected tile, its state and its "
                   "description"):
        run.key("Tab")
    with run.act("Right: Bundle", [
            focused(role=RADIO, name="Bundle"),
            event("object:state-changed:checked", role=RADIO, name_contains="Bundle",
                  detail1=1),
            event("object:state-changed:checked", role=RADIO, name_contains="Single file",
                  detail1=0),
            said_in_order("Bundle", "selected radio button", "Friendlier to version control"),
            not_said("Project format")],
            should="selection follows focus; the reader hears the new tile as selected, "
                   "and not the group again"):
        run.key("Right")
    with run.act("Right again: wraps to Single file", [
            focused(role=RADIO, name="Single file"),
            said_in_order("Single file", "selected radio button"),
            not_said("Project format")],
            should="the row wraps round, as a radio group does"):
        run.key("Right")
    with run.act("Left: back to Bundle", [
            focused(role=RADIO, name="Bundle"), said("Bundle"), not_said("Project format")],
            should="Left moves back"):
        run.key("Left")


def tile_list(run):
    run.wait_for(role=RADIO, name="Novel")
    tab_into(run, [("Single file", "Project format")])
    with run.act("Tab 2: Template", [
            focused(role=RADIO, name="Novel"),
            said_in_order("Template", "Novel", "selected radio button", "20 chapters")],
            should="the second group's selected tile, Novel, with its meta"):
        run.key("Tab")
    with run.act("Home: None", [
            focused(role=RADIO, name="None"),
            event("object:state-changed:checked", role=RADIO, name_contains="None", detail1=1),
            said_in_order("None", "selected radio button", "empty binder"),
            not_said("Template")],
            should="Home selects the first tile"):
        run.key("Home")
    with run.act("End: Notebook", [
            focused(role=RADIO, name="Notebook"),
            said_in_order("Notebook", "selected radio button", "free-form notes"),
            not_said("Template")],
            should="End selects the last tile"):
        run.key("End")
    with run.act("Up: Novel", [focused(role=RADIO, name="Novel"), said("Novel"),
                               not_said("Template")],
                 should="Up moves back one"):
        run.key("Up")
    with run.act("Down: Notebook", [focused(role=RADIO, name="Notebook"), said("Notebook"),
                                    not_said("Template")],
                 should="Down moves on one"):
        run.key("Down")


def tile_grid(run):
    run.wait_for(role=RADIO, name="Draft")
    tab_into(run, [("Single file", "Project format"), ("Novel", "Template")])
    with run.act("Tab 3: Publication stage", [
            focused(role=RADIO, name="Draft"),
            said_in_order("Publication stage", "Draft", "selected radio button",
                          "Work in progress")],
            should="the grid group's selected tile, Draft"):
        run.key("Tab")
    with run.act("Right: Review", [focused(role=RADIO, name="Review"), said("Review")],
                 should="Right moves on"):
        run.key("Right")
    with run.act("Right: Published", [focused(role=RADIO, name="Published"),
                                      said("Published")],
                 should="Right moves on"):
        run.key("Right")
    with run.act("Right past the disabled Archived: Draft", [
            focused(role=RADIO, name="Draft"), said("Draft"), not_said("Archived"),
            no_event("object:state-changed:checked", role=RADIO, name_contains="Archived")],
            should="the disabled tile is skipped and selection wraps to Draft"):
        run.key("Right")
    with run.act("Left from Draft: Published (skips Archived)", [
            focused(role=RADIO, name="Published"), not_said("Archived")],
            should="Left wraps back past the disabled tile"):
        run.key("Left")


def tile_at_click(run):
    run.wait_for(role=RADIO, name="Bundle")
    with run.act("AT-SPI click on Bundle, focus elsewhere", [
            event("object:state-changed:checked", role=RADIO, name_contains="Bundle",
                  detail1=1),
            event("object:state-changed:checked", role=RADIO, name_contains="Single file",
                  detail1=0)],
            should="a screen reader's own activation selects the tile"):
        run.action("click", role=RADIO, name="Bundle")
    with run.act("AT-SPI click on the disabled Archived", [
            no_event("object:state-changed:checked", role=RADIO, name_contains="Archived"),
            no_event("object:state-changed:checked", role=RADIO, name_contains="Draft")],
            should="a disabled tile cannot be selected, by any route", tree=True):
        run.action("click", role=RADIO, name="Archived")
    tab_into(run, [("Bundle", "Project format"), ("Novel", "Template")])
    with run.act("Tab 3: Publication stage still has Draft", [
            focused(role=RADIO, name="Draft"), not_said("Archived")],
            should="the grid still has Draft selected"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# close-confirmation
# ---------------------------------------------------------------------------

MAIN = dict(focus_name="Save", title="Close window?",
            text="The document has unsaved changes",
            more="Your changes will be lost")
SUGAR = dict(focus_name="No", title="Close this window?",
             text="locked against accidental closing", more=None)


def dialog_checks(*, focus_name: str, title: str, text: str, more: str | None) -> list:
    checks = [
        in_tree(role="alert", name=title),
        in_tree(role="alert", name=title, description_contains=text),
        count_in_tree("alert", 1),
        focused(role="push button", name=focus_name),
        said_in_order(title, text, focus_name),
    ]
    if more:
        checks.append(in_tree(role="alert", name=title, description_contains=more))
    return checks


def ask_to_close(run, label: str, do, *, record: float = 6.0, **dialog) -> bool:
    """Ask for a close with `do`, and expect the confirmation to come up by
    itself. When it does not, press Shift, a key that does nothing, and expect
    it then, so the rest of the scenario still has its dialog. Returns whether
    it came by itself."""
    checks = dialog_checks(**dialog)
    with run.act(label, checks, record=record,
                 should="the close is held and one dialog asks at once, named by its "
                        "question; the reader hears the question and the text, and lands on "
                        f"{dialog['focus_name']}") as act:
        do()
    run.collect_events(act)
    came = any(e["type"] == "object:children-changed:add"
               and (e.get("target") or {}).get("role") == "alert" for e in act.events)
    if came:
        return True
    run.note(f"{label!r}: no dialog reached the bus in {record:g} s; pressed Shift to wake "
             "the app")
    with run.act("press Shift, which does nothing, to wake the app", checks,
                 should="(scene setting) the held dialog comes up on the next input event"):
        run.key("Shift")
    return False


def title_once(title: str) -> list:
    """What a reader should hear of the title as the dialog opens: once."""
    return [said_count(title, 1)]


BOX = {"role": "check box", "name": "Document has unsaved changes"}
CLOSE_BTN = {"role": "push button", "name": "Close window (ctx.close_window)"}


def tab_to_box(run):
    run.wait_for(**BOX)
    with run.act("Tab to the unsaved-changes checkbox", [focused(**BOX),
                                                         said("Document has unsaved changes")],
                 should="the reader reaches the first control"):
        run.key("Tab")


def close_kwin_escape(run):
    tab_to_box(run)
    ask_to_close(run, "close the window as its close button does", run.close_window, **MAIN)
    with run.act("Escape", [
            not_in_tree(role="alert"),
            focused(**BOX), said("Document has unsaved changes"),
            no_event("object:state-changed:defunct", role="frame")],
            should="Cancel: the dialog goes, the window stays, focus returns to the "
                   "checkbox", tree=True):
        run.key("Escape")
    note_alive(run, "after Escape")


def close_kwin_twice(run):
    tab_to_box(run)
    with run.act("close through KWin", should="(scene setting) the first close request",
                 record=3.0):
        run.close_window()
    with run.act("close through KWin again, as a user who heard nothing would", [
            count_in_tree("alert", 1), focused(role="push button", name="Save")],
            should="one dialog, however many times the close was asked for", record=4.0,
            tree=True):
        run.close_window()
    with run.act("press Shift, which does nothing", [count_in_tree("alert", 1)],
                 should="still one dialog", tree=True):
        run.key("Shift")
    with run.act("Escape: Cancel", [not_in_tree(role="alert"), focused(**BOX)],
                 should="Cancel closes the dialog and focus returns to the checkbox; no "
                        "second dialog is left", tree=True):
        run.key("Escape")
    note_alive(run, "after Escape")


def close_again_while_open(run):
    """The dialog is up (opened by the button, which does not wait); the user
    asks the window to close again, as a second click on the close button."""
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="the reader reaches the button"):
        run.key("Tab")
    with run.act("Space on Close window (ctx.close_window)", [count_in_tree("alert", 1)],
                 should="(scene setting) the dialog opens", tree=True, record=3.0):
        run.key("space")
    with run.act("close through KWin while the dialog is up", [count_in_tree("alert", 1)],
                 should="a close asked for while the dialog is already asking adds nothing",
                 tree=True, record=3.0):
        run.close_window()
    with run.act("press Shift, which does nothing", [count_in_tree("alert", 1)],
                 should="still one dialog", tree=True):
        run.key("Shift")


def close_kwin_space(run):
    """A reader who asked for a close and heard nothing presses Space on the
    control they are on."""
    tab_to_box(run)
    with run.act("close through KWin", should="(scene setting) the close request; nothing "
                 "is heard", record=4.0):
        run.close_window()
    with run.act("Space on the checkbox", [
            no_event("object:state-changed:checked", role="check box"),
            in_tree(role="alert", name="Close window?"),
            focused(role="push button", name="Save")],
            should="the close request was answered with a dialog before this key, so the "
                   "key does not reach the checkbox beneath it", tree=True):
        run.key("space")
    note_alive(run, "after Space")


def close_button_cancel(run):
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="the reader reaches the button"):
        run.key("Tab")
    ask_to_close(run, "Space on Close window (ctx.close_window)", lambda: run.key("space"),
                 **MAIN)
    with run.act("Tab: Discard", [focused(role="push button", name="Discard"),
                                   said("Discard")],
                 should="focus moves along the buttons"):
        run.key("Tab")
    with run.act("Tab: Cancel", [focused(role="push button", name="Cancel"), said("Cancel")],
                 should="focus moves along the buttons"):
        run.key("Tab")
    with run.act("Tab: wraps inside the dialog", [focused(role="push button", name="Save")],
                 should="focus stays inside the modal dialog"):
        run.key("Tab")
    with run.act("Shift+Tab: Cancel", [focused(role="push button", name="Cancel")],
                 should="back to Cancel"):
        run.key("Shift+Tab")
    with run.act("Space on Cancel", [
            not_in_tree(role="alert"), focused(**CLOSE_BTN), said("Close window")],
            should="Cancel closes the dialog, keeps the window, and focus goes back to "
                   "the button that opened it", tree=True):
        run.key("space")
    note_alive(run, "after Cancel")


def close_button_title(run):
    """The opening of the dialog, on the route that does not wait: the title
    once."""
    tab_to_box(run)
    with run.act("Tab to the Close window button", [focused(**CLOSE_BTN)],
                 should="the reader reaches the button"):
        run.key("Tab")
    with run.act("Space on Close window (ctx.close_window)",
                 dialog_checks(**MAIN) + title_once("Close window?"),
                 should="one dialog; the reader hears its title once, its text, and Save",
                 record=4.0):
        run.key("space")


def close_discard(run):
    tab_to_box(run)
    ask_to_close(run, "close the window as its close button does", run.close_window, **MAIN)
    with run.act("Tab: Discard", [focused(role="push button", name="Discard"),
                                   said("Discard")],
                 should="the reader lands on Discard"):
        run.key("Tab")
    with run.act("Enter on Discard: the window closes and the app quits",
                 should="the focused button answers, not the default: Discard closes",
                 record=3.0):
        run.key("Return")
    note_alive(run, "after Enter on Discard")


def close_save_enter(run):
    tab_to_box(run)
    ask_to_close(run, "close the window as its close button does", run.close_window, **MAIN)
    with run.act("Enter: Save, then close",
                 should="Enter answers Save; the window closes", record=3.0):
        run.key("Return")
    note_alive(run, "after Enter on Save")


def box_state(want_checked: bool) -> Check:
    """After the act, the checkbox's `checked` state on the bus is as given."""
    def run(act):
        node = next((n for n in _walk(act.tree)
                     if n.get("role") == "check box" and n.get("name") == BOX["name"]), None)
        if node is None:
            return False, ["the checkbox is not in the tree"]
        states = node.get("states", [])
        return ("checked" in states) == want_checked, [f"states={states}"]
    return custom(f"the checkbox reads {'checked' if want_checked else 'not checked'} "
                  "on the bus", run, needs_tree=True)


def checkbox_toggle(run):
    tab_to_box(run)
    with run.act("Space: uncheck", [
            event("object:state-changed:checked", role="check box", detail1=0),
            said("not checked"), box_state(False)],
            should="the checkbox unchecks and the reader hears it"):
        run.key("space")
    with run.act("Space: check again", [
            event("object:state-changed:checked", role="check box", detail1=1),
            said("checked"), box_state(True)],
            should="the checkbox checks and the reader hears it"):
        run.key("space")
    with run.act("AT-SPI click: uncheck", [
            event("object:state-changed:checked", role="check box", detail1=0),
            said("not checked"), box_state(False)],
            should="a screen reader's own activation unchecks it and the reader hears it"):
        run.action("click", **BOX)
    with run.act("Tab away and Shift+Tab back", [
            focused(**BOX), said("not checked"), box_state(False)],
            should="coming back, the reader hears the state the checkbox is in"):
        run.key("Tab")
        run.wait(0.8)
        run.key("Shift+Tab")


def close_clean(run):
    tab_to_box(run)
    with run.act("Space: clear the checkbox", [
            event("object:state-changed:checked", role="check box", detail1=0),
            said("not checked")],
            should="the document is marked clean and the reader hears it"):
        run.key("space")
    with run.act("close the window as its close button does", [
            no_event("object:children-changed:add", role="frame")],
            should="with nothing unsaved the window closes at once, no dialog", record=4.0):
        run.close_window()
    note_alive(run, "after closing a clean document")


# -- The can_close sugar window ------------------------------------------------

SUGAR_OPEN = {"role": "push button", "name": "Open can_close-sugar window…"}
SUGAR_BOX = {"role": "check box", "name": "Locked against closing"}


def open_sugar(run):
    run.wait_for(**SUGAR_OPEN)
    for i, spec in enumerate([BOX, CLOSE_BTN, SUGAR_OPEN], 1):
        with run.act(f"Tab {i}", [focused(**spec)], should="the reader walks to the button"):
            run.key("Tab")
    with run.act("Space: the second window opens", [event("window:activate")],
                 should="a second window opens and becomes active", record=3.0):
        run.key("space")
    with run.act("Tab in the new window", [focused(**SUGAR_BOX),
                                           said("Locked against closing")],
                 should="Tab reaches the new window's checkbox"):
        run.key("Tab")


def sugar_blocked(run):
    open_sugar(run)
    ask_to_close(run, "close the sugar window as its close button does",
                 lambda: close_only(run, "can_close"), **SUGAR)
    with run.act("Escape: No", [not_in_tree(role="alert"), focused(**SUGAR_BOX)],
                 should="the dialog goes, the window stays, focus back on the checkbox",
                 tree=True):
        run.key("Escape")
    ask_to_close(run, "close it again", lambda: close_only(run, "can_close"), **SUGAR)
    with run.act("Shift+Tab to Yes", [focused(role="push button", name="Yes"), said("Yes")],
                 should="the reader lands on Yes"):
        run.key("Shift+Tab")
    with run.act("Space on Yes: the sugar window closes", [
            event("window:activate"), focused(**SUGAR_OPEN)],
            should="the second window closes; the main window is active again and "
                   "focus is back on the button that opened it", record=4.0):
        run.key("space")
    note_alive(run, "after Yes")


def sugar_button(run):
    open_sugar(run)
    with run.act("Tab to Close window", [focused(role="push button", name="Close window")],
                 should="the window's own Close button"):
        run.key("Tab")
    ask_to_close(run, "Space on Close window (can_close)", lambda: run.key("space"), **SUGAR)
    with run.act("Enter: No (the default)", [not_in_tree(role="alert"),
                                            focused(role="push button", name="Close window")],
                 should="the safe default keeps the window; focus back on the button",
                 tree=True):
        run.key("Return")


SCENARIOS = [
    Scenario("radioclose-tile-tree", "radio-tile", tile_tree,
             "the tiles, groups, positions and states a reader finds"),
    Scenario("radioclose-tile-row", "radio-tile", tile_row,
             "arrows through the Project format row of two tiles"),
    Scenario("radioclose-tile-list", "radio-tile", tile_list,
             "Home, End, Up, Down through the Template list"),
    Scenario("radioclose-tile-grid", "radio-tile", tile_grid,
             "arrows through the Publication stage grid, past a disabled tile"),
    Scenario("radioclose-tile-atclick", "radio-tile", tile_at_click,
             "AT-SPI click on a tile, and on a disabled one"),
    Scenario("radioclose-close-escape", "close-confirmation", close_kwin_escape,
             "close through KWin, then Escape"),
    Scenario("radioclose-close-twice", "close-confirmation", close_kwin_twice,
             "close through KWin twice before the dialog shows"),
    Scenario("radioclose-close-again", "close-confirmation", close_again_while_open,
             "the dialog is up; close through KWin once more"),
    Scenario("radioclose-close-space", "close-confirmation", close_kwin_space,
             "close through KWin, then Space on the focused checkbox"),
    Scenario("radioclose-close-cancel", "close-confirmation", close_button_cancel,
             "close through the button, walk the dialog, Cancel"),
    Scenario("radioclose-close-title", "close-confirmation", close_button_title,
             "close through the button: what the reader hears as the dialog opens"),
    Scenario("radioclose-close-discard", "close-confirmation", close_discard,
             "close through KWin, Tab to Discard, Enter"),
    Scenario("radioclose-close-save", "close-confirmation", close_save_enter,
             "close through KWin, Enter on the default Save"),
    Scenario("radioclose-checkbox", "close-confirmation", checkbox_toggle,
             "the unsaved-changes checkbox toggled by Space and by AT-SPI click"),
    Scenario("radioclose-close-clean", "close-confirmation", close_clean,
             "clear the dirty flag, close: no dialog"),
    Scenario("radioclose-sugar-blocked", "close-confirmation", sugar_blocked,
             "open the can_close window, close it through KWin: blocked, Escape, Yes"),
    Scenario("radioclose-sugar-button", "close-confirmation", sugar_button,
             "the can_close window's own Close button, then Enter on the default No"),
]
