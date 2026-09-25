# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""widget-catalog, part C: the overlays, data, drag-and-drop, animations,
touch and settings tabs, as a screen reader meets them.

One launch per scenario, each on its own tab (`--tab NAME`), so a scenario
starts from the tab's own first reading. The overlays tab is split by kind of
overlay (tooltips, popover and message boxes, snackbar and toasts), because
each overlay leaves state behind it (a sticky tooltip, an archive entry) that
would colour the next one's reading.

Every step that sets a scene is done inside an act of its own ("set the
scene: ..."), never between acts: the harness credits Orca's speech to an act
by Orca's receipt of the act's events, matched by type from where the last
act left off, so a focus change made between two acts would be matched to the
next act's first focus change and its speech credited there.

What a reader should get, per tab:

* **overlays**: every trigger named and reachable; a plain tooltip's text as
  its control's description; a rich or composite tooltip shown and spoken
  when its control takes keyboard focus (`docs/tooltips.md`, "Keyboard / a11y
  promotion"), and focus staying where Tab put it when the reader moves on; a
  popover or a message box presented as one named dialog that takes focus and
  gives it back; a snackbar or a toast spoken as it appears, with its message
  rather than a generic word.
* **data**: every view named, every row with a role and a position; arrow
  keys move and are spoken; expand and collapse spoken; Tab leaves a table.
* **dragdrop**: a keyboard route to every drop, and it spoken.
* **animations**: a control per demo, each named for what it drives; content
  a demo hides leaves the tree a reader walks.
* **touch**: every control named; the reorderable list reorderable from the
  keyboard, and the move spoken.
* **settings**: every setting named and its state spoken.
"""

from __future__ import annotations

from reader_lib.checks import (_focus_node, _is_focus, _walk, announced, custom, focused,
                               in_tree, no_event, not_in_tree, not_said, said)
from reader_lib.orca import utterances
from reader_lib.scenario import Scenario, tab_walk

PKG = "widget-catalog"

LEVEL_1 = "Hover or hold — level 1"
LEVEL_2 = "Hover or hold — level 2"
LEVEL_3 = "Hover or hold — level 3"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def setup(run, label: str, step, record: float = 1.0) -> None:
    """Set a scene inside an act of its own, with nothing expected of it, so
    Orca's catch-up covers it and its speech is not credited to the next act."""
    with run.act(f"set the scene: {label}", should="scene setting, not judged",
                 record=record):
        step()


def alive():
    """The application is still running after the act."""
    def run(act):
        bad = [e for e in act.events if e["type"] == "object:state-changed:defunct"
               and e.get("source", {}).get("role") == "frame"]
        return not bad and "exited" not in (act.error or ""), \
            [f"{e['wall']} {e['type']} [{e['source'].get('role')}]" for e in bad] or \
            [act.error or "the window is still on the bus"]
    return custom("the application is still running", run)


def announcements(act) -> list[str]:
    return [e.get("text", "") for e in act.events if e["type"] == "object:announcement"]


def any_announcement():
    def run(act):
        found = announcements(act)
        return bool(found), [f"announced {t!r}" for t in found] or \
            ["no object:announcement in the act"]
    return custom("some announcement reaches the bus", run)


def focus_lines(act, last: int = 6) -> list[str]:
    moves = [e for e in act.events if _is_focus(e)]
    return [f"{act.rel_ms(e):+.1f} ms focus -> [{_focus_node(e).get('role')}] "
            f"{_focus_node(e).get('name')!r}" for e in moves[-last:]]


def focus_stays(role: str, name: str):
    """Every focus change of the act after the first landing on the node
    stays on it: nothing sends focus elsewhere once the key put it there."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        lines = focus_lines(act, 8)
        if not moves:
            return False, ["no focus change in the act"]
        landed = [i for i, e in enumerate(moves)
                  if _focus_node(e).get("role") == role and _focus_node(e).get("name") == name]
        if not landed:
            return False, lines
        after = moves[landed[0]:]
        ok = all(_focus_node(e).get("name") == name for e in after)
        return ok, lines
    return custom(f"focus lands on [{role}] {name!r} and stays there", run)


def named_focus():
    """The act's last focus change lands on a node with a name."""
    def run(act):
        moves = [e for e in act.events if _is_focus(e)]
        if not moves:
            return False, ["no focus change in the act"]
        node = _focus_node(moves[-1])
        return bool((node.get("name") or "").strip()), focus_lines(act)
    return custom("focus lands on a node with a name", run)


def no_role_in_tree(role: str, under: str | None = None, under_role: str | None = None):
    """No node of `role` in the tree after the act (optionally only under the
    first node named `under`, of the role `under_role` when given)."""
    def run(act):
        root = act.tree
        if under is not None:
            root = next((n for n in _walk(act.tree) if n.get("name") == under
                         and (under_role is None or n.get("role") == under_role)), None)
        bad = [f"[{n.get('role')}] {n.get('name')!r} desc={n.get('description')!r}"
               for n in _walk(root) if n.get("role") == role]
        return not bad, bad[:14] + ([f"... {len(bad)} in all"] if len(bad) > 14 else [])
    where = f" under {under!r}" if under else ""
    return custom(f"no node with the role {role!r}{where} in the tree", run, needs_tree=True)


def orca_quiet():
    return custom("Orca stays quiet", lambda act: (not utterances(act.orca),
                                                    [u.text for u in utterances(act.orca)]),
                  needs_orca=True)


# ---------------------------------------------------------------------------
# Overlays: tooltips
# ---------------------------------------------------------------------------


def description_while_focused(name: str, text: str):
    """The focused control's description gained `text` while it still had
    focus: the only way a focus-promoted tooltip's text reaches a reader who
    stays put (Orca speaks a description change on the locus of focus)."""
    def run(act):
        focus_at = None
        left_at = None
        for e in act.events:
            src = e.get("source", {})
            if e["type"] == "object:state-changed:focused" and src.get("name") == name:
                if e.get("detail1") == 1 and focus_at is None:
                    focus_at = e["seq"]
                elif e.get("detail1") == 0 and focus_at is not None and left_at is None:
                    left_at = e["seq"]
        changes = [e for e in act.events
                   if e["type"] == "object:property-change:accessible-description"
                   and e.get("source", {}).get("name") == name]
        inside = [e for e in changes if focus_at is not None and e["seq"] > focus_at
                  and (left_at is None or e["seq"] < left_at) and text in (e.get("text") or "")]
        adds = [e for e in act.events if e["type"] == "object:children-changed:add"
                and text in (e.get("target", {}).get("name") or "")]
        evidence = [f"{act.rel_ms(e):+.1f} ms tooltip node added: "
                    f"[{e['target'].get('role')}] {e['target'].get('name')!r}" for e in adds]
        evidence += [f"{act.rel_ms(e):+.1f} ms description of {name!r} -> {e.get('text')!r}"
                     for e in changes] or [f"no description change on {name!r} in the act"]
        return bool(inside), evidence
    return custom(f"{name!r} gains the tooltip text as its description while focused", run)


def no_markup_in_names():
    """No node reaches the bus named with raw `[label](:key)` link markup."""
    def run(act):
        bad = []
        for e in act.events:
            for node in (e.get("source", {}), e.get("target", {})):
                name = node.get("name") or ""
                if "](:" in name:
                    bad.append(f"{act.rel_ms(e):+.1f} ms {e['type']} [{node.get('role')}] "
                               f"{name!r}")
            if "](:" in (e.get("text") or ""):
                bad.append(f"{act.rel_ms(e):+.1f} ms {e['type']} text={e.get('text')!r}")
        return not bad, sorted(set(bad))[:6]
    return custom("no node or text on the bus carries raw link markup", run)


def overlays_tooltips(run):
    run.wait_for(role="push button", name="Save")
    with run.act("focus Save (plain tooltip)",
                 [focused(role="push button", name="Save"), said("Save"),
                  said("Save the current document")],
                 should="the button and its plain tooltip's text, as its description"):
        run.grab_focus(role="push button", name="Save")
    with run.act("Tab to Open (plain tooltip)",
                 [focused(role="push button", name="Open"),
                  in_tree(role="push button", name="Open", description_contains="Open a file")],
                 should="the next button, its tooltip's text as its description",
                 tree=True):
        run.key("Tab")
    with run.act(f"focus {LEVEL_1!r} and stay (rich tooltip)",
                 [focused(role="push button", name=LEVEL_1),
                  description_while_focused(LEVEL_1, "Level 1 of the cascade"),
                  said("Level 1 of the cascade"), no_markup_in_names()],
                 should="focus shows the rich tooltip after its delay; a reader who stays "
                        "on the button hears the tooltip's text, without link markup",
                 record=4.0):
        run.grab_focus(role="push button", name=LEVEL_1)
    with run.act("Escape closes the rich tooltip",
                 [not_in_tree(name_contains="Level 1 of the cascade"),
                  not_said("[next link]"), no_markup_in_names()],
                 should="the tooltip goes, focus stays on the button; if anything is "
                        "said it is the tooltip's text, not its markup", tree=True):
        run.key("Escape")
    setup(run, "focus 'Plain among rich'",
          lambda: run.grab_focus(role="push button", name="Plain among rich"))
    with run.act("focus 'Province info' and stay (composite tooltip)",
                 [focused(role="push button", name="Province info"),
                  said("Province info"), said("Iberia"), not_said("Tooltip")],
                 should="the composite tooltip opens on focus and its content reaches the "
                        "reader, not the generic word 'Tooltip'", record=3.5, tree=True):
        run.grab_focus(role="push button", name="Province info")
    setup(run, "Escape, then focus 'Plain among rich'",
          lambda: (run.key("Escape"),
                   run.grab_focus(role="push button", name="Plain among rich")))
    with run.act("focus 'Tabbed details' and stay (composite tooltip with tabs)",
                 [focused(role="push button", name="Tabbed details"),
                  not_said("Stats page tab")],
                 should="the tooltip opening does not speak a page tab the reader is not "
                        "on", record=3.5):
        run.grab_focus(role="push button", name="Tabbed details")
    setup(run, "Escape", lambda: run.key("Escape"))
    with run.act("focus 'With internal Button' and stay past the dwell",
                 [focused(role="push button", name="With internal Button"),
                  said("Treasury report")],
                 should="the composite tooltip's content is spoken", record=3.5, tree=True):
        run.grab_focus(role="push button", name="With internal Button")
    with run.act("Tab into the sticky composite tooltip",
                 [focused(role="dialog"), said("Treasury report"), not_said("Tooltip dialog")],
                 should="the docs: Tab into the surface; the reader hears what it is",
                 record=1.5):
        run.key("Tab")
    with run.act("Tab to the inner button",
                 [focused(role="push button", name="Open ledger"), said("Open ledger")],
                 should="the inner button takes focus and is read", tree=True):
        run.key("Tab")


def tooltip_tab_away(run):
    """Tab away from a control while its focus-summoned tooltip is on screen.

    A focus-summoned tooltip opens after its delay (about 0.5 s) and is
    promoted to a sticky dialog after the dwell (2 s). Between the two, a Tab
    moves focus on; the tooltip should close behind it and focus stay put.
    """
    run.wait_for(role="push button", name=LEVEL_1)
    setup(run, f"focus {LEVEL_1!r}", lambda: run.grab_focus(role="push button", name=LEVEL_1),
          record=0.3)
    for here, there in ((LEVEL_1, LEVEL_2), (LEVEL_2, LEVEL_3), (LEVEL_3, "Plain among rich")):
        with run.act(f"wait 1 s on {here!r} (tooltip up, not sticky), Tab",
                     [focus_stays("push button", there)],
                     should=f"focus moves to {there!r} and stays; the tooltip of {here!r} "
                            "closes behind it", settle=0.1, record=2.0):
            run.wait(1.0)
            run.key("Tab")
    setup(run, f"Escape, focus {LEVEL_1!r}",
          lambda: (run.key("Escape"), run.grab_focus(role="push button", name=LEVEL_1)),
          record=0.3)
    with run.act(f"wait 3 s on {LEVEL_1!r} (tooltip sticky), Tab",
                 [focus_stays("push button", LEVEL_2)],
                 should="control case: a sticky tooltip is dismissed by the focus move",
                 settle=0.1, record=2.0):
        run.wait(3.0)
        run.key("Tab")
    setup(run, "Escape, focus 'Province info'",
          lambda: (run.key("Escape"),
                   run.grab_focus(role="push button", name="Province info")),
          record=0.3)
    with run.act("wait 1.2 s on 'Province info' (composite tooltip up), Tab",
                 [focus_stays("push button", "Tabbed details")],
                 should="focus moves to 'Tabbed details' and stays", settle=0.1, record=2.0):
        run.wait(1.2)
        run.key("Tab")


# ---------------------------------------------------------------------------
# Overlays: popover and message boxes
# ---------------------------------------------------------------------------


def overlays_dialogs(run):
    run.wait_for(role="push button", name="Anchor")
    setup(run, "focus Anchor", lambda: run.grab_focus(role="push button", name="Anchor"))
    with run.act("Space on Anchor (popover)",
                 [named_focus(), said("Popover content")],
                 should="the popover opens, focus moves into it, and the reader hears "
                        "what it is and what it says", tree=True):
        run.key("space")
    with run.act("Tab inside the open popover",
                 [custom("focus stays inside the popover or the reader learns where it went",
                         lambda act: (bool([e for e in act.events if _is_focus(e)]),
                                      focus_lines(act)))],
                 should="the reader can tell where focus is", record=1.5):
        run.key("Tab")
    with run.act("Escape closes the popover",
                 [focused(role="push button", name="Anchor"), said("Anchor"),
                  not_in_tree(name="Popover content")],
                 should="the popover goes and focus comes back to Anchor", tree=True):
        run.key("Escape")
    boxes = [
        ("Open Dialog", "Dialog example", "This is a Dialog"),
        ("Information", "Information", "Informational dialog."),
        ("Warning", "Warning", "Disk is almost full."),
        ("Error", "Error", "Something went wrong."),
        ("Confirm", "Are you sure?", "This action cannot be undone."),
    ]
    for trigger, title, body in boxes:
        setup(run, f"focus {trigger!r}",
              lambda t=trigger: run.grab_focus(role="push button", name=t, nth=0))
        with run.act(f"Space on {trigger!r} (message box)",
                     [said(title), said(body), in_tree(role="alert", name=title), alive()],
                     should=f"an alert dialog named {title!r} opens, takes focus, and the "
                            f"reader hears its title and its message", tree=True):
            run.key("space")
        if trigger == "Open Dialog":
            with run.act("Shift+Tab to 'Show details'",
                         [focused(role="push button", name="Show details"),
                          said("Show details"), said("collapsed")],
                         should="the disclosure button is reached and says it is collapsed"):
                run.key("Shift+Tab")
            with run.act("Space on 'Show details'",
                         [said("expanded"), said(body),
                          custom("the details are not inside the button",
                                 details_outside_button, needs_tree=True)],
                         should="the details open, the reader learns they did, and the "
                                "text is somewhere a reader can go", tree=True):
                run.key("space")
        with run.act(f"Escape closes the {trigger!r} message box",
                     [focused(role="push button", name=trigger), said(trigger),
                      not_in_tree(role="alert")],
                     should="the dialog goes and focus returns to its trigger", tree=True):
            run.key("Escape")


def details_outside_button(act):
    """The message box's detail text sits under no push button."""
    lines = []
    bad = False

    def visit(node, parents):
        nonlocal bad
        if node.get("role") == "label" and "presented via MessageBox" in (node.get("name") or ""):
            chain = " > ".join(f"[{p.get('role')}] {p.get('name')!r}" for p in parents[-4:])
            lines.append(f"{chain} > [label] {node.get('name')!r}")
            if any(p.get("role") == "push button" for p in parents):
                bad = True
        for c in node.get("children", []):
            visit(c, parents + [node])
    if act.tree:
        visit(act.tree, [])
    return (not bad and bool(lines)), lines or ["the detail text is not in the tree"]


# ---------------------------------------------------------------------------
# Overlays: snackbar, toasts, notification centre
# ---------------------------------------------------------------------------


def not_announced_word(word: str):
    def run(act):
        bad = [t for t in announcements(act) if t.strip() == word]
        return not bad, [f"announced {t!r}" for t in bad]
    return custom(f"no announcement that is only the word {word!r}", run)


def said_count(text: str, most: int):
    def run(act):
        from reader_lib.orca import normalized
        heard = [u for u in utterances(act.orca) if normalized(text) in normalized(u.text)]
        return len(heard) <= most, [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                                    for u in utterances(act.orca)]
    return custom(f"Orca says {text!r} at most {most} time(s)", run, needs_orca=True)


def bell_survives(act):
    """The focused bell is not replaced by a new node (which leaves the reader
    on a defunct object and makes Orca read the bell again)."""
    lines = []
    dead = False
    for e in act.events:
        src = e.get("source", {})
        if e["type"] == "object:state-changed:defunct" and src.get("name") == "Notifications" \
                and src.get("role") == "push button":
            dead = True
            lines.append(f"{act.rel_ms(e):+.1f} ms {e['type']} [push button] 'Notifications' "
                         f"{src.get('path')}")
        if e["type"].startswith("object:state-changed:focused"):
            lines.append(f"{act.rel_ms(e):+.1f} ms {e['type']} {e.get('detail1')} "
                         f"[{src.get('role')}] {src.get('name')!r}")
        if e["type"].startswith("object:children-changed") and \
                (e.get("target", {}).get("name") == "Notifications"):
            lines.append(f"{act.rel_ms(e):+.1f} ms {e['type']} -> "
                         f"[{e['target'].get('role')}] 'Notifications' {e['target'].get('path')}")
    return not dead, lines or ["nothing happened to the bell"]


def overlays_notices(run):
    run.wait_for(role="push button", name="Show snackbar")
    setup(run, "focus 'Show snackbar'",
          lambda: run.grab_focus(role="push button", name="Show snackbar"))
    with run.act("Space on 'Show snackbar' (first time)",
                 [announced("File saved successfully"), said("File saved successfully"),
                  not_announced_word("Snackbar")],
                 should="the snackbar appears and the reader hears its message",
                 record=4.5):
        run.key("space")
    with run.act("Space on 'Show snackbar' (second time)",
                 [any_announcement(), said("File saved successfully")],
                 should="a second snackbar is heard as the first was", record=4.5):
        run.key("space")
    with run.act("AT-SPI click on the focused 'Show snackbar'",
                 [any_announcement()],
                 should="a screen reader's own activation of the focused button opens "
                        "the snackbar too", record=4.5):
        run.action("click", role="push button", name="Show snackbar")
    toasts = [
        ("Info", "Info notice", None),
        ("Success", "Saved", None),
        ("Warning", "Warning", "Take a look when you have a moment."),
        ("Error", "Build failed", "Three errors, two warnings."),
        ("Loading", "Working", None),
    ]
    for trigger, title, body in toasts:
        # The message-box row has its own "Warning" and "Error" buttons first.
        nth = 1 if trigger in ("Warning", "Error") else 0
        setup(run, f"focus the toast trigger {trigger!r}",
              lambda t=trigger, n=nth: run.grab_focus(role="push button", name=t, nth=n),
              record=0.6)
        expect = [announced(title), said(title)]
        if body:
            expect.append(said(body))
        expect.append(custom("no toast already on screen is announced again",
                             lambda act, t=title: (
                                 all(t.startswith(a) or a.startswith(t) or a == t
                                     for a in announcements(act)),
                                 [f"announced {a!r}" for a in announcements(act)])))
        with run.act(f"Space on toast trigger {trigger!r}", expect,
                     should=f"the toast appears and the reader hears {title!r}"
                            + (f" and {body!r}" if body else "") + ", once",
                     record=3.0, tree=True):
            run.key("space")
    with run.act("focus the Notifications bell",
                 [focused(role="push button", name_contains="Notifications"),
                  said("Notifications"), said("5")],
                 should="the bell is named and says how many are unread"):
        run.grab_focus(role="push button", name_contains="Notifications")
    with run.act("Space on the Notifications bell",
                 [said("Build failed"), in_tree(name_contains="Build failed"),
                  in_tree(role="dialog", name_contains="Notification"),
                  no_role_in_tree("unknown", under="Notifications", under_role="list")],
                 should="the notification log opens as a named dialog and the reader "
                        "hears it; its entries are list items", tree=True):
        run.key("space")
    with run.act("Tab inside the notification log",
                 [named_focus()],
                 should="Tab reaches the log's next control", record=1.3):
        run.key("Tab")
    with run.act("Escape closes the notification log",
                 [focused(role="push button", name_contains="Notifications"),
                  custom("the focused bell is not destroyed under the reader", bell_survives)],
                 should="focus returns to the bell", tree=True):
        run.key("Escape")
    with run.act("a toast arrives while focus rests on the bell",
                 [custom("the focused bell is not destroyed under the reader", bell_survives),
                  said("Info notice")],
                 should="the unread count changes; focus and the reader's place stay on "
                        "the bell, and the toast is heard", record=3.0, tree=True):
        run.action("click", role="push button", name="Info")
    setup(run, "open the notification log again, Tab to its last control",
          lambda: (run.grab_focus(role="push button", name_contains="Notifications"),
                   run.key("space"), run.wait(0.8), run.key("Tab")), record=1.5)
    with run.act("Tab from the log's last control",
                 [custom("focus stays inside the log (an entry or a control of it)",
                         lambda act: (bool([e for e in act.events if _is_focus(e)]) and
                                      not any(_focus_node(e).get("role") == "status bar"
                                              or _focus_node(e).get("name") == "Clear"
                                              for e in act.events if _is_focus(e)),
                                      focus_lines(act)))],
                 should="the entries are reachable, or focus stays in the open log",
                 record=1.5, tree=True):
        run.key("Tab")
    with run.act("Escape on the toast that took focus",
                 [custom("focus lands on a named control, not the window",
                         lambda act: (bool([e for e in act.events if _is_focus(e)]) and
                                      _focus_node([e for e in act.events if _is_focus(e)][-1])
                                      .get("role") != "frame", focus_lines(act)))],
                 should="the toast goes and focus returns to where the reader was",
                 record=1.5):
        run.key("Escape")


# ---------------------------------------------------------------------------
# Data
# ---------------------------------------------------------------------------


def left_table(act):
    """The act's last focus change is not a cell of the People table."""
    moves = [e for e in act.events if _is_focus(e)]
    if not moves:
        return False, ["no focus change: focus stayed where it was"]
    last = _focus_node(moves[-1])
    lines = focus_lines(act, 4)
    utter = [u.text for u in utterances(act.orca)]
    lines += [f"Orca said {t!r}" for t in utter]
    in_table = last.get("role") in ("table cell", "table") and last.get("name") != "Tiles"
    return not in_table, lines


def data_views(run):
    run.wait_for(role="list box")
    with run.act("focus the ListView",
                 [named_focus(), no_role_in_tree("unknown")],
                 should="the list is named and read; no row has an unmapped role",
                 tree=True):
        run.grab_focus(role="list box")
    with run.act("Down in the ListView",
                 [focused(role="list item", name="Row 1"), said("Row 1")],
                 should="the first row is read, with its selection"):
        run.key("Down")
    with run.act("End in the ListView",
                 [focused(role="list item", name="Row 10"), said("Row 10")],
                 should="the last row is read"):
        run.key("End")
    with run.act("Tab to the TreeView",
                 [focused(role="tree"), named_focus()],
                 should="focus reaches the tree, which is named"):
        run.key("Tab")
    with run.act("Down in the TreeView",
                 [focused(role="tree item", name="Documents"), said("Documents"),
                  said("collapsed")],
                 should="the first item is read, with the state that it can be expanded"):
        run.key("Down")
    with run.act("Right (expand Documents)",
                 [in_tree(role="tree item", name="Projects"), said("expanded"),
                  no_event("object:state-changed:defunct", role="tree item")],
                 should="Documents opens; the reader hears that it expanded, on the same "
                        "row it was on", tree=True):
        run.key("Right")
    with run.act("Down onto the first child",
                 [said("Projects")], should="the first child is read, with its level"):
        run.key("Down")
    with run.act("Left then Left (to the parent, then collapse)",
                 [not_in_tree(role="tree item", name="Projects"), said("collapsed")],
                 should="the reader hears the item collapsed", tree=True):
        run.key("Left")
        run.key("Left")
    with run.act("Tab to the TableView",
                 [focused(role="table"), named_focus()],
                 should="focus reaches the table, which is named", tree=True):
        run.key("Tab")
    with run.act("Down in the TableView", [said("Avery")],
                 should="the first row's focused cell is read"):
        run.key("Down")
    with run.act("Tab from a cell",
                 [custom("focus leaves the table", left_table)],
                 should="Tab moves on to the next control (or the reader is told how to "
                        "leave)", record=1.5):
        run.key("Tab")
    with run.act("Tab five more times",
                 [custom("focus leaves the table", left_table)],
                 should="Tab never stays trapped in the table", record=1.5):
        run.key("Tab", "Tab", "Tab", "Tab", "Tab")
    with run.act("Ctrl+Tab leaves the table",
                 [custom("focus leaves the table", left_table)],
                 should="the documented escape works", record=2.0, tree=True):
        run.key("Ctrl+Tab")
    with run.act("Down, Down, Right in the TreeTableView (expand 'docs')",
                 [in_tree(name="README.md"), said("expanded")],
                 should="the folder opens and the reader hears it", tree=True, record=2.0):
        run.key("Down")
        run.key("Down")
        run.key("Right")
    with run.act("Ctrl+Tab to the GridView", [said("Tiles")],
                 should="focus reaches the tile grid, named 'Tiles'", tree=True, record=2.0):
        run.key("Ctrl+Tab")
    with run.act("Right in the GridView", [said("Sunset")],
                 should="the first tile is read"):
        run.key("Right")


# ---------------------------------------------------------------------------
# Drag and drop
# ---------------------------------------------------------------------------


def drop_target_route(act):
    """Somewhere in the tree, the DropTarget's content sits in a node a
    keyboard can reach or an AT can act on."""
    lines = []
    ok = False

    def visit(node, parents):
        nonlocal ok
        if "wraps a Panel" in (node.get("name") or ""):
            lines.append(f"found [{node.get('role')}] {node.get('name')!r}")
            for p in reversed(parents[-3:]):
                acts = [a.get("name") for a in p.get("actions", [])]
                lines.append(f"  ancestor [{p.get('role')}] {p.get('name')!r} "
                             f"states={p.get('states')} actions={acts}")
                if "focusable" in p.get("states", []) or acts:
                    ok = True
        for c in node.get("children", []):
            visit(c, parents + [node])
    if act.tree:
        visit(act.tree, [])
    return ok, lines or ["the DropTarget's body is not in the tree"]


def empty_announcements(act):
    """No announcement with no text anywhere in the run so far."""
    empty = [e for e in act.history if e["type"] == "object:announcement"
             and not (e.get("text") or "").strip()]
    return not empty, [f"{e['wall']} object:announcement [{e['source'].get('role')}] "
                       f"{e['source'].get('name')!r} text={e.get('text')!r} "
                       f"path {e['source'].get('path')}" for e in empty] or ["none"]


def dragdrop(run):
    run.wait_for(role="push button", name="Browse…")
    with run.act("the launch's announcements",
                 [custom("no empty announcement reached the bus", empty_announcements)],
                 should="nothing is announced that has nothing to say", record=0.3):
        pass
    with run.act("focus the first Browse… button",
                 [focused(role="push button", name="Browse…"), said("Drop anything here")],
                 should="the button, and which zone it browses for"):
        run.grab_focus(role="push button", name="Browse…")
    with run.act("Tab to the second Browse… button",
                 [focused(role="push button", name="Browse…"), said("Drop images here")],
                 should="the second button, with its zone, so the two are told apart"):
        run.key("Tab")
    with run.act("Tab on past the zones",
                 [said("DropTarget")],
                 should="the DropTarget offers a keyboard stop (or anything a reader can "
                        "act on) to drop a file without dragging", tree=True):
        run.key("Tab")
    with run.act("look for a non-drag route to the DropTarget",
                 [custom("the DropTarget is focusable or offers an action", drop_target_route,
                         needs_tree=True)],
                 should="WCAG 2.5.7: a drop target has a route that needs no drag",
                 tree=True, record=0.5):
        pass


# ---------------------------------------------------------------------------
# Animations
# ---------------------------------------------------------------------------


def distinct_toggle_names(act):
    names = [n.get("name") for n in _walk(act.tree) if n.get("role") == "toggle button"]
    dupes = sorted({n for n in names if names.count(n) > 1})
    return not dupes, [f"toggle buttons: {names}"] + [f"{d!r} names {names.count(d)} toggles"
                                                    for d in dupes]


def churn(act):
    """Nothing is added to or removed from the tree while the reader does
    nothing."""
    moves = [e for e in act.events if e["type"].startswith("object:children-changed")
             or e["type"] == "object:state-changed:defunct"]
    return not moves, [f"{act.rel_ms(e):+.1f} ms {e['type']} -> "
                       f"[{(e.get('target') or e.get('source', {})).get('role')}] "
                       f"{(e.get('target') or e.get('source', {})).get('name')!r}"
                       for e in moves[:8]] + ([f"... {len(moves)} in all"] if moves else [])


def animations(run):
    run.wait_for(role="toggle button", name="Visible")
    with run.act("focus the Fade toggle",
                 [focused(role="toggle button", name="Visible"), said("Visible"),
                  custom("each toggle is named for what it drives", distinct_toggle_names,
                         needs_tree=True)],
                 should="the toggle says what it drives (the Fade demo)", tree=True):
        run.grab_focus(role="toggle button", name="Visible")
    with run.act("Space: fade the cell out",
                 [said("not pressed"), not_in_tree(name="fading")],
                 should="the toggle's new state is spoken; the faded-out cell leaves the "
                        "tree a reader walks", tree=True, record=2.0):
        run.key("space")
    with run.act("idle 6 s on the tab (Cycle and Pulse running)",
                 [orca_quiet(), custom("the tree holds still", churn)],
                 should="decorative motion is silent and does not churn the tree",
                 record=6.0):
        pass
    setup(run, "focus the Collapse toggle",
          lambda: run.grab_focus(role="toggle button", name="Expanded"))
    with run.act("Space on Expanded: collapse the Collapse demo",
                 [said("not pressed"), not_in_tree(role="label", name="Collapsing content")],
                 should="the collapsed content leaves the tree", tree=True, record=2.0):
        run.key("space")
    with run.act("Tab from Expanded (the Slide demo's toggle)",
                 [focused(role="toggle button"), said("Slide")],
                 should="the Slide demo's toggle, told apart from the Fade one", record=1.5):
        run.key("Tab")
    with run.act("Space: slide the cell out",
                 [said("not pressed"), not_in_tree(name="snackbar")],
                 should="the slid-out cell leaves the tree", tree=True, record=2.0):
        run.key("space")


# ---------------------------------------------------------------------------
# Touch
# ---------------------------------------------------------------------------


def value_change(role: str):
    """A value change on a `role` node reached the bus inside the act."""
    def run(act):
        found = [e for e in act.events if e["type"] == "object:property-change:accessible-value"
                 and e.get("source", {}).get("role") == role]
        return bool(found), [f"{act.rel_ms(e):+.1f} ms {e['type']} [{role}] "
                             f"{e['source'].get('name')!r}" for e in found] or \
            [f"no accessible-value change on a {role} in the act"]
    return custom(f"the {role}'s new value reaches the bus", run)


def touch(run):
    run.wait_for(role="list box")
    with run.act("focus the reorderable list",
                 [named_focus()], should="the list is named", tree=True):
        run.grab_focus(role="list box")
    with run.act("Down in the reorderable list",
                 [focused(role="list item", name="row 1"), said("row 1")],
                 should="the keyboard's current row is read", record=2.0):
        run.key("Down")
    with run.act("Down again",
                 [focused(role="list item", name="row 2"), said("row 2")],
                 should="the next row is read", record=2.0):
        run.key("Down")
    with run.act("Alt+Down: move the current row down (1st move)",
                 [any_announcement(), said("row 2")],
                 should="the keyboard route to the reorder: the row moves and the reader "
                        "hears where it went", record=2.5):
        run.key("Alt+Down")
    with run.act("Alt+Down: move it again (2nd move)",
                 [any_announcement(), said("row 2")],
                 should="a second move is heard too", record=2.5):
        run.key("Alt+Down")
    with run.act("Tab to the slider",
                 [focused(role="slider"), said("40")],
                 should="the slider is named for what it sets and its value read"):
        run.key("Tab")
    with run.act("Right on the slider", [value_change("slider"), said("41")],
                 should="the new value is read at once", record=2.5):
        run.key("Right")
    with run.act("Tab to the splitter handle",
                 [focused(role="separator")],
                 should="the gutter is a named, operable separator", tree=True):
        run.key("Tab")
    with run.act("Right on the splitter handle",
                 [value_change("separator")],
                 should="the pane size change is reported", record=2.5):
        run.key("Right")
    with run.act("Tab to the text field",
                 [focused(role="entry"), named_focus(), said("Tap, hold, drag a handle")],
                 should="the text field is named, and its placeholder read"):
        run.key("Tab")


# ---------------------------------------------------------------------------
# Tab walks
# ---------------------------------------------------------------------------


def walker(stops: int):
    def body(run):
        tab_walk(run, stops=stops)
    return body


# ---------------------------------------------------------------------------
# Settings
# ---------------------------------------------------------------------------


def settings_launch(run):
    with run.act("after launch on the Settings tab",
                 [alive(), in_tree(role="spin button", name_contains="Text size")],
                 should="the Settings tab is on screen and its controls are in the tree",
                 tree=True, record=0.5):
        pass


def settings_open(run):
    """Launched on the Touch tab (so the Settings page tab is in the tree),
    then Settings chosen as a reader would."""
    run.wait_for(role="page tab", name="Touch")
    setup(run, "focus the Touch page tab",
          lambda: run.grab_focus(role="page tab", name="Touch"))
    with run.act("Down arrow from Touch to Settings",
                 [focused(role="page tab", name="Settings"), said("Settings"), alive()],
                 should="the Settings tab is selected and read, its settings shown",
                 tree=True, record=4.0):
        run.key("Down")


SCENARIOS = [
    Scenario("catalog-c-overlays-tooltips", PKG, overlays_tooltips,
             "overlays tab: plain, rich and composite tooltips from the keyboard",
             args=["--tab", "overlays"]),
    Scenario("catalog-c-tooltip-tab-away", PKG, tooltip_tab_away,
             "overlays tab: Tab away from a control while its tooltip is shown",
             args=["--tab", "overlays"]),
    Scenario("catalog-c-overlays-dialogs", PKG, overlays_dialogs,
             "overlays tab: popover and the five message boxes",
             args=["--tab", "overlays"]),
    Scenario("catalog-c-overlays-notices", PKG, overlays_notices,
             "overlays tab: snackbar, five toasts, the notification bell and log",
             args=["--tab", "overlays"]),
    Scenario("catalog-c-overlays-tabwalk", PKG, walker(45),
             "overlays tab: Tab walk", args=["--tab", "overlays"]),
    Scenario("catalog-c-data-tabwalk", PKG, walker(30),
             "data tab: Tab walk", args=["--tab", "data"]),
    Scenario("catalog-c-data-views", PKG, data_views,
             "data tab: list, tree, table, tree table and grid from the keyboard",
             args=["--tab", "data"]),
    Scenario("catalog-c-dragdrop-tabwalk", PKG, walker(20),
             "dragdrop tab: Tab walk", args=["--tab", "dragdrop"]),
    Scenario("catalog-c-dragdrop", PKG, dragdrop,
             "dragdrop tab: the keyboard routes to each drop", args=["--tab", "dragdrop"]),
    Scenario("catalog-c-animations-tabwalk", PKG, walker(30),
             "animations tab: Tab walk", args=["--tab", "animations"]),
    Scenario("catalog-c-animations", PKG, animations,
             "animations tab: toggles, hidden content, idle motion",
             args=["--tab", "animations"]),
    Scenario("catalog-c-touch-tabwalk", PKG, walker(30),
             "touch tab: Tab walk", args=["--tab", "touch"]),
    Scenario("catalog-c-touch", PKG, touch,
             "touch tab: keyboard reorder, slider, splitter, text field",
             args=["--tab", "touch"]),
    Scenario("catalog-c-settings-open", PKG, settings_open,
             "settings tab: reached from the Touch tab with the arrow key",
             args=["--tab", "touch"]),
    Scenario("catalog-c-settings-launch", PKG, settings_launch,
             "settings tab: launched straight onto it", args=["--tab", "settings"]),
]
