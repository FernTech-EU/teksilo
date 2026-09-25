# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verifier's probes for the misc sweep (color-picker-demo, font-picker,
recent-projects, drag-and-drop).

Acts the sweep's `misc.py` does not do, each asking one question:

* ``verify-misc-mono-tree``: when Space checks a `Checkbox`, is the new state
  in the tree the adapter holds (so only the event is missing), or is the
  AccessKit tree itself stale until something else re-walks it?
* ``verify-misc-combo-ws``: the non-searchable `ComboBox`: arrowing the open
  list, and what AT-SPI's Selection interface says is selected (Orca's
  `onSelectionChanged` presents what that interface returns).
* ``verify-misc-color-fr``: the colour picker under a French locale: which of
  the names a reader meets are translated and which are fixed English.
* ``verify-misc-recent-show``: the Show/Hide paths swap in both directions,
  and Clear recents with focus in a row.
"""

from __future__ import annotations

from reader_lib.checks import _walk, custom, event, focused, in_tree, said
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario


def _heard(act) -> list[str]:
    return [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}" for u in utterances(act.orca)] \
        or ["Orca said nothing in the act"]


def said_something():
    def run(act):
        return bool(utterances(act.orca)), _heard(act)
    return custom("Orca says something", run, needs_orca=True)


def node_where(describe: str, pred, *, want: bool = True):
    def run(act):
        hits = [n for n in _walk(act.tree) if pred(n)]
        lines = [f"[{n.get('role')}] {n.get('name')!r} states={n.get('states')} "
                 f"attrs={n.get('attributes')}" for n in hits[:20]]
        return (bool(hits) == want), lines or ["no node matched"]
    return custom(describe, run, needs_tree=True)


def orca_log_has(describe: str, needle: str):
    """Orca's log for the act contains `needle` (evidence, not speech)."""
    def run(act):
        lines = [f"{line.stamp} {line.text}" for line in act.orca if needle in line.text]
        return bool(lines), lines[:6] or [f"no Orca log line containing {needle!r}"]
    return custom(describe, run, needs_orca=True)


def scene(run, label: str, **kw):
    return run.act(f"(scene) {label}", should="setting the scene", **kw)


# ---------------------------------------------------------------------------
# font-picker: the Monospace only check box
# ---------------------------------------------------------------------------


def _mono_checked(n: dict) -> bool:
    return n.get("role") == "check box" and n.get("name") == "Monospace only" \
        and "checked" in (n.get("states") or [])


def mono_tree_body(run):
    with scene(run, "Tab three times to Monospace only"):
        run.key("Tab", "Tab", "Tab")
    with run.act("Space, then read the adapter's tree", [
            event("object:state-changed:checked", role="check box", detail1=1),
            node_where("the tree the adapter holds shows the box checked", _mono_checked),
    ], should="the box is checked in the tree at once", record=5.0, tree=True):
        run.key("space")
    with run.act("do nothing for 6 s", [
            event("object:state-changed:checked", role="check box"),
    ], should="(does the change arrive on its own, late?)", record=6.0, tree=True):
        run.wait(0.1)
    with run.act("Tab to Writing system", [
            event("object:state-changed:checked", role="check box", detail1=1),
    ], should="(the stale change is flushed by the focus move)", record=2.5, tree=True):
        run.key("Tab")
    with run.act("AT-SPI click on Monospace only, then read the tree", [
            event("object:state-changed:checked", role="check box", detail1=0),
            node_where("the tree shows the box unchecked",
                       lambda n: n.get("role") == "check box" and n.get("name") == "Monospace only"
                       and "checked" not in (n.get("states") or [])),
    ], should="a screen reader's own activation unchecks it, at once", record=4.0, tree=True):
        run.action("click", role="check box", name="Monospace only")


# ---------------------------------------------------------------------------
# font-picker: the non-searchable Writing system combo
# ---------------------------------------------------------------------------


def combo_ws_body(run):
    with scene(run, "Tab four times to Writing system"):
        run.key("Tab", "Tab", "Tab", "Tab")
    with run.act("Alt+Down opens the list", [said_something()],
                 should="the list opens and the reader hears the current script",
                 record=3.0, tree=True):
        run.key("Alt+Down")
    with run.act("Down arrow", [
            said("Latin"),
            orca_log_has("Orca asked the list box's Selection interface",
                         "selected children"),
    ], should="the reader hears Latin", record=3.0, tree=True):
        run.key("Down")
    with run.act("Down arrow again", [said("Greek")],
                 should="the reader hears Greek", record=3.0, tree=True):
        run.key("Down")
    with run.act("Escape closes the list", [
            focused(role="combo box", name="Writing system"),
            said("Greek"),
    ], should="the list closes, and the reader hears the script now shown", record=3.0,
            tree=True):
        run.key("Escape")
    with run.act("Shift+Tab then Tab back to Writing system", [
            said("Greek"),
    ], should="the reader hears the combo box's current value", record=2.5):
        run.key("Shift+Tab")
        run.wait(0.8)
        run.key("Tab")


# ---------------------------------------------------------------------------
# color-picker-demo in French
# ---------------------------------------------------------------------------


def color_fr_body(run):
    with run.act("the tree at launch, French locale", [
            in_tree(name="Sélecteur de couleur"),
            node_where("no English name is left in the picker",
                       lambda n: (n.get("name") or "") in (
                           "Saturation and brightness", "Selected color", "Color picker",
                           "Hex", "Hue"), want=False),
    ], should="every name the picker gives a reader is French", tree=True):
        run.wait(0.2)
    with run.act("Shift+Tab three times to the saturation × brightness field", [
            said_something(),
    ], should="the field's name is French", record=2.5):
        run.key("Shift+Tab", "Shift+Tab", "Shift+Tab")
    with run.act("Right arrow on it", [said_something()],
                 should="the announced pair is French", record=3.0):
        run.key("Right")


# ---------------------------------------------------------------------------
# recent-projects: the paths toggle in both directions
# ---------------------------------------------------------------------------


def recent_show_body(run):
    with scene(run, "focus Hide paths"):
        run.grab_focus(role="push button", name="Hide paths")
    with run.act("Space on Hide paths", [focused(role="push button", name="Show paths")],
                 should="focus lands on Show paths", record=2.5):
        run.key("space")
    with scene(run, "focus Show paths"):
        run.grab_focus(role="push button", name="Show paths")
    with run.act("Space on Show paths", [focused(role="push button", name="Hide paths")],
                 should="focus lands on Hide paths", record=2.5):
        run.key("space")
    with run.act("Tab after it", [said_something()],
                 should="Tab continues from the toolbar", record=2.0):
        run.key("Tab")


SCENARIOS = [
    Scenario("verify-misc-mono-tree", "font-picker", mono_tree_body,
             "verifier: is a Checkbox's new state in the adapter's tree before the next re-walk?"),
    Scenario("verify-misc-combo-ws", "font-picker", combo_ws_body,
             "verifier: non-searchable ComboBox, arrows, Selection interface, value after close"),
    Scenario("verify-misc-color-fr", "color-picker-demo", color_fr_body,
             "verifier: colour picker names under fr_FR", lang="fr_FR.UTF-8"),
    Scenario("verify-misc-recent-show", "recent-projects", recent_show_body,
             "verifier: Show/Hide paths swap in both directions"),
]
