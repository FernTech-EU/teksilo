# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""A keyboard move's announcement, heard after the focus move it came with.

Moving a row or a tab from the keyboard says where it went through the
framework's announcer (`common/ordered_move.rs`, `tab_widget/header.rs`) in
the handler that also puts focus on the moved item, which the move rebuilt.
Both used to reach the adapter in one update, and `accesskit_consumer` hands
an update's added nodes to the adapter before its focus move
(`tree.rs:640-673`), so the announcement reached the bus first and Orca 46.1
stopped it to read the new focus (`scripts/default.py:698-705`). The sweep
heard every such message cut or dropped (collections-04, misc-09,
catalog-c-27, tabs-03).

These scenarios judge only that: the move's announcement reaches the bus
after the act's focus change, and Orca says it whole. The rows a move rebuilds
still go defunct, which the harness observes on every act; that is another
finding.
"""

from __future__ import annotations

from reader_lib.checks import _event_line, _is_focus, _norm, announced, custom, focused, said
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario
from scenarios.data_collections import enter_list
from scenarios.tabs import to_tab


def after_focus(text: str):
    """The announcement carrying `text` reached the bus after the act's first
    focus change, so no reader is told of it before the focus it came with."""
    def run(act):
        focus = next((e for e in act.events if _is_focus(e)), None)
        spoken = [e for e in act.events if e["type"] == "object:announcement"
                  and _norm(text) in _norm(e.get("text"))]
        evidence = [_event_line(act, e) for e in act.events
                    if _is_focus(e) or e["type"] == "object:announcement"]
        if focus is None or not spoken:
            return False, evidence or ["no focus change or no announcement in the act"]
        return all(e["seq"] > focus["seq"] for e in spoken), evidence
    return custom(f"{text!r} reaches the bus after the act's focus change", run)


def never_cut(text: str):
    """Orca never cut a saying of `text` (`said` passes when any one is whole)."""
    def run(act):
        wanted = normalized(text)
        heard = [u for u in utterances(act.orca) if wanted in normalized(u.text)]
        evidence = [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                    for u in utterances(act.orca)] or ["Orca said nothing in the act"]
        return bool(heard) and not any(u.cut for u in heard), evidence
    return custom(f"Orca says {text!r} and never cuts it", run, needs_orca=True)


def moved(text: str) -> list:
    return [announced(text), after_focus(text), said(text), never_cut(text)]


def listview(run):
    enter_list(run, "ListView", "- Remove First")
    with run.act("Tab into the list", [focused(role="list box")], should="focus on the list"):
        run.key("Tab")
    with run.act("Down: first row", [focused(role="list item", name="Item 1")],
                 should="the cursor on row 1"):
        run.key("Down")
    for label, keys, message in (("Alt+Down", "Alt+Down", "Moved to 2 of 200"),
                                 ("Alt+Down again", "Alt+Down", "Moved to 3 of 200"),
                                 ("Alt+Home", "Alt+Home", "Moved to 1 of 200")):
        with run.act(label, [focused(role="list item", name="Item 1"), *moved(message)],
                     should=f"Item 1 moves, keeps focus, and the reader hears {message!r} "
                     "after the row"):
            run.key(keys)


def tree(run):
    enter_list(run, "TreeView", "- Remove Last Root")
    with run.act("Tab into the tree", [focused(role="tree")], should="focus on the tree"):
        run.key("Tab")
    with run.act("Down: Documents", [focused(role="tree item", name="Documents")],
                 should="the cursor on Documents"):
        run.key("Down")
    for label, keys, message in (("Alt+Down", "Alt+Down", "Moved to 2 of 3"),
                                 ("Alt+Right", "Alt+Right", "Moved to level 2"),
                                 ("Alt+Left", "Alt+Left", "Moved to level 1")):
        with run.act(label, [focused(role="tree item", name="Documents"), *moved(message)],
                     should=f"Documents moves, keeps focus, and the reader hears {message!r} "
                     "after the row"):
            run.key(keys)


def tabs(run):
    run.wait_for(role="page tab", name="Doc 1")
    to_tab(run, 2)
    for label, message in (("Alt+Right", "Doc 1 moved to 5 of 6"),
                           ("Alt+Right again", "Doc 1 moved to 6 of 6")):
        with run.act(label, [focused(role="page tab", name="Doc 1"), *moved(message)],
                     should=f"Doc 1 moves right, keeps focus, and the reader hears "
                     f"{message!r} after the tab"):
            run.key("Alt+Right")


SCENARIOS = [
    Scenario("fix-announce-focus-listview", "data-collections", listview,
             "three ListView keyboard moves: each move heard after focus lands on the row"),
    Scenario("fix-announce-focus-tree", "data-collections", tree,
             "a TreeView move, indent and outdent: each heard after focus lands on the row"),
    Scenario("fix-announce-focus-tabs", "tab-widget", tabs,
             "two tab moves from the keyboard: each heard after focus lands on the tab"),
]
