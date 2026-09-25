# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Verification of the chrome sweep (`chrome.py`): acts the sweep did not do,
to settle a finding's cause or to look for what it missed.

* A collapsible menu bar revealed a second time (F10, Escape, F10): the bar
  is shown again with the node ids it had, which the Linux adapter announced
  defunct when it left.
* Alt+V on a collapsible bar that is already revealed: if the menu opens
  then, the Alt+letter failure from a collapsed bar is the synthetic click
  landing on a bar that is still rolled up.
* A menu-bar dropdown opened a second time (F10, Down, Escape, Down): the
  dropdown factory builds a fresh `MenuList`, so it should not be defunct.
* native-menu: a closure-only entry (File > Open Recent > document-1.txt)
  against an intent entry (File > New), and the arena parent chain of the
  model-built `MenuItem`s, read through the bridge after the acts.
* A global shortcut with nothing focused, and the arena's first root, which
  `pointer_router.rs` anchors such a shortcut at.
* A rich tip summoned by a real Tab arrival (not AT-SPI focus), then Tab.

Every scenario is named `verify-chrome-*`.
"""

from __future__ import annotations

from reader_lib.checks import custom, focused, in_tree, said
from reader_lib.orca import normalized, utterances
from reader_lib.scenario import Scenario


def said_any(*texts: str):
    def run(act):
        heard = utterances(act.orca)
        hit = [u for u in heard for t in texts if normalized(t) in normalized(u.text)]
        return bool(hit), [f"Orca said{' (cut)' if u.cut else ''}: {u.text!r}"
                           for u in heard] or ["Orca said nothing in this act"]
    return custom(f"Orca says one of {texts!r}", run, needs_orca=True)


def log_mark(run) -> list:
    try:
        size = len(run.app_log.read_text(errors="replace"))
    except OSError:
        size = 0
    return [size, run.app_log]


def printed(text: str, mark: list):
    def run(act):
        log = mark[1].read_text(errors="replace") if mark[1].exists() else ""
        tail = log[mark[0]:]
        return text in tail, [f"app.log after the act started: {tail.strip()[-300:]!r}"]
    return custom(f"the example printed {text!r}", run)


# ---------------------------------------------------------------------------
# The bridge, read after the acts, for causes
# ---------------------------------------------------------------------------


def _layout(run, include_debug: bool = True, max_depth=None) -> dict | None:
    try:
        bridge = run.bridge()
        args = {"include_debug": include_debug}
        if max_depth is not None:
            args["max_depth"] = max_depth
        return bridge.call("layout_tree", **args)
    except Exception as exc:  # noqa: BLE001 - diagnostic only
        run.note(f"the bridge could not be read: {type(exc).__name__}: {exc}")
        return None


def _short(t: str | None) -> str:
    return (t or "?").rsplit("::", 1)[-1]


def note_roots(run, label: str) -> None:
    lt = _layout(run, include_debug=False, max_depth=0)
    if not lt:
        return
    nodes = {n["id"]: n for n in lt.get("nodes", [])}
    roots = lt.get("roots", [])
    listing = [f"#{i} {_short(nodes.get(r, {}).get('type'))}"
               f"{'' if nodes.get(r, {}).get('active') else ' (dormant)'}"
               for i, r in enumerate(roots)]
    run.note(f"{label}: the arena has {len(roots)} parentless roots, in the order "
             f"arena.roots() returns them (the first is where a Global shortcut with no "
             f"focus is anchored): {', '.join(listing[:25])}")


def note_chain(run, label: str, type_suffix: str, debug_contains: str) -> None:
    lt = _layout(run, include_debug=True)
    if not lt:
        return
    nodes = {n["id"]: n for n in lt.get("nodes", [])}
    roots = lt.get("roots", [])
    hits = [n for n in lt.get("nodes", [])
            if (n.get("type") or "").endswith(type_suffix)
            and debug_contains in (n.get("debug") or "")]
    if not hits:
        run.note(f"{label}: no {type_suffix} whose Debug holds {debug_contains!r}")
        return
    for hit in hits[:2]:
        chain = []
        cur = hit
        seen = set()
        while cur is not None and cur["id"] not in seen:
            seen.add(cur["id"])
            chain.append(f"{_short(cur.get('type'))}"
                         f"{'' if cur.get('active') else '(dormant)'}")
            parent = cur.get("parent")
            cur = nodes.get(parent) if parent is not None else None
        top = chain[-1] if chain else "?"
        run.note(f"{label}: arena parent chain of the {type_suffix} holding "
                 f"{debug_contains!r}, up to its parentless top ({len(chain)} nodes): "
                 f"{' -> '.join(chain)}; the top is {top!r}")


# ---------------------------------------------------------------------------
# collapsible-menu-bar
# ---------------------------------------------------------------------------

CMB = "collapsible-menu-bar"


def cmb_rereveal(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("F10 (first reveal)", [focused(role="menu item", name="File"), said("File")],
                 should="the bar is revealed with focus on File", record=2.5):
        run.key("F10")
    with run.act("Escape (hide)", should="the bar hides", record=2.5):
        run.key("Escape")
    with run.act("F10 (second reveal)", [focused(role="menu item", name="File"), said("File")],
                 should="the second reveal is heard like the first", record=2.5, tree=True):
        run.key("F10")
    with run.act("Right (the next trigger, or its menu)",
                 [said_any("Edit", "menu")],
                 should="the reader hears where Right went", record=2.5):
        run.key("Right")
    with run.act("Escape, Escape", record=2.5):
        run.key("Escape")
        run.wait(0.6)
        run.key("Escape")
    with run.act("Tab to the hamburger, Space (third reveal)",
                 [focused(role="menu item", name="File"), said("File")],
                 should="the third reveal is heard", record=3.0):
        run.key("Tab")
        run.wait(1.0)
        run.key("space")


def cmb_altv_revealed(run):
    run.wait_for(role="push button", name="Menu")
    with run.act("F10 (reveal; the bar unrolls fully)",
                 [focused(role="menu item", name="File")], should="revealed", record=2.5):
        run.key("F10")
    with run.act("Alt+V on the revealed bar",
                 [focused(role="menu"), said("menu")],
                 should="with the bar already unrolled, the synthetic click lands on View and "
                        "its menu opens", record=3.0, tree=True):
        run.key("Alt+v")


# ---------------------------------------------------------------------------
# shortcuts-demo
# ---------------------------------------------------------------------------

SC = "shortcuts-demo"


def menu_reopen(run):
    run.wait_for(role="menu item", name="File")
    with run.act("F10", [focused(role="menu item", name="File")], record=2.0):
        run.key("F10")
    with run.act("Down (open File, first time)", [focused(role="menu"), said("menu")],
                 record=2.0):
        run.key("Down")
    with run.act("Escape (close it)", [focused(role="menu item", name="File")], record=2.0):
        run.key("Escape")
    with run.act("Down (open File, second time)", [focused(role="menu"), said("menu")],
                 should="a dropdown opened a second time is heard as the first",
                 record=2.5):
        run.key("Down")
    with run.act("Escape", record=1.5):
        run.key("Escape")


def shortcuts_nofocus_diag(run):
    run.wait_for(role="menu item", name="File")
    mark = log_mark(run)
    with run.act("Ctrl+S with nothing focused", [printed("[action] Save", mark)],
                 should="a global shortcut reaches its action", record=2.0):
        run.key("Ctrl+s")
    mark2 = log_mark(run)
    with run.act("Ctrl+S again, with nothing focused",
                 [printed("[action] Save", mark2)], record=2.0):
        run.key("Ctrl+s")
    note_roots(run, "shortcuts-demo after launch, nothing focused")
    run.wait_for(role="push button", name="Go to line 7")
    mark3 = log_mark(run)
    with run.act("focus 'Go to line 7' (AT-SPI), then Ctrl+S",
                 [printed("[action] Save", mark3)], record=2.0):
        run.grab_focus(role="push button", name="Go to line 7")
        run.wait(0.8)
        run.key("Ctrl+s")
    run.key("F10")
    run.wait(0.6)
    run.key("Down")
    run.wait(1.0)
    note_chain(run, "shortcuts-demo, File open", "MenuItem", "Save")


# ---------------------------------------------------------------------------
# native-menu
# ---------------------------------------------------------------------------

NM = "native-menu"


def native_diag(run):
    run.wait_for(role="push button", name="Add recent file")
    with run.act("Ctrl+N with nothing focused",
                 [in_tree(role="label", name="New chosen")], record=2.0, tree=True):
        run.key("Ctrl+n")
    note_roots(run, "native-menu after launch, nothing focused")
    with run.act("click 'Add recent file' (AT-SPI)",
                 [in_tree(role="label", name="Choose a menu item…")], record=1.5, tree=True):
        run.action("click", role="push button", name="Add recent file")
    with run.act("Alt+F, click 'Open Recent' (AT-SPI), click 'document-1.txt' (AT-SPI)",
                 [in_tree(role="label", name="Opened document-1.txt")],
                 should="a closure-only entry (no intent) runs from the in-window menu",
                 record=3.0, tree=True):
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="Open Recent")
        run.wait(1.0)
        run.action("click", role="menu item", name="document-1.txt")
    with run.act("Escape, Escape", record=1.5):
        run.key("Escape")
        run.wait(0.4)
        run.key("Escape")
    with run.act("Alt+F, click 'New' (AT-SPI)",
                 [in_tree(role="label", name="New chosen")],
                 should="an intent entry runs from the in-window menu", record=3.0, tree=True):
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="New")
    with run.act("Alt+V, click 'Show Grid' (AT-SPI)",
                 [in_tree(role="label", name_contains="Grid: hidden")],
                 should="Show Grid toggles", record=3.0, tree=True):
        run.key("Alt+v")
        run.wait(1.0)
        run.action("click", role="check menu item", name="Show Grid")
    run.key("Alt+f")
    run.wait(1.0)
    note_chain(run, "native-menu, File open", "MenuItem", "New")


# ---------------------------------------------------------------------------
# tooltips-showcase
# ---------------------------------------------------------------------------

TIPS = "tooltips-showcase"
LEVEL1 = "Hover or hold — level 1"
LEVEL2 = "Hover or hold — level 2"


def tips_tab_arrival(run):
    """Arrive on a rich-tip anchor by a real Tab, wait for the tip, Tab on."""
    run.wait_for(role="push button", name=LEVEL1)

    def stays(anchor, nxt):
        def check(act):
            moves = [e for e in act.events
                     if e["type"] == "object:state-changed:focused" and e.get("detail1") == 1]
            lines = [f"{act.rel_ms(e):+.1f} ms focused 1 [{e['source'].get('role')}] "
                     f"{e['source'].get('name')!r}" for e in moves]
            landed = next((i for i, e in enumerate(moves)
                           if e["source"].get("name") == nxt), None)
            if landed is None:
                return False, [f"focus never reached {nxt!r}"] + lines
            back = [e for e in moves[landed + 1:] if e["source"].get("name") == anchor]
            return not back, lines
        return custom(f"after landing on {nxt!r}, focus does not go back to {anchor!r}", check)

    for i in range(3):
        run.grab_focus(role="push button", name="Close")
        with run.act(f"try {i + 1}: from Close, Tab to level 1, wait 1.2 s, Tab",
                     [stays(LEVEL1, LEVEL2), focused(role="push button", name=LEVEL2)],
                     should="focus lands on level 2 and stays", record=2.5, settle=3.0):
            run.key("Tab")
            run.wait(1.2)
            run.key("Tab")
        with run.act(f"try {i + 1}: Escape", record=1.5):
            run.key("Escape")



# ---------------------------------------------------------------------------
# Per-act output: the sweep's `printed` read the log to the END of the run, so
# a later act's print passed an earlier act. These read only what the act
# itself printed, cut at the act's own end.
# ---------------------------------------------------------------------------


def _log_len(run) -> int:
    try:
        return len(run.app_log.read_text(errors="replace"))
    except OSError:
        return 0


def printed_between(text: str, window: list):
    """`window` is [start, end, path], filled by the act body."""
    def run(act):
        log = window[2].read_text(errors="replace") if window[2].exists() else ""
        part = log[window[0]:window[1]]
        return text in part, [f"the example printed during the act: {part.strip()[-300:]!r}"]
    return custom(f"the example printed {text!r} during this act (not later)", run)


def menu_intents_sc(run):
    run.wait_for(role="menu item", name="File")
    w1 = [0, 0, run.app_log]
    with run.act("F10, Down (open File), AT-SPI click 'Save' (send_intent from a MenuItem)",
                 [printed_between("[action] Save", w1)],
                 should="the Save action runs", record=1.0):
        w1[0] = _log_len(run)
        run.key("F10")
        run.wait(0.6)
        run.key("Down")
        run.wait(1.0)
        run.action("click", role="menu item", name="Save")
        run.wait(1.5)
        w1[1] = _log_len(run)
    w2 = [0, 0, run.app_log]
    with run.act("focus 'Go to line 7' (AT-SPI), Ctrl+S",
                 [printed_between("[action] Save", w2)], record=1.0):
        w2[0] = _log_len(run)
        run.grab_focus(role="push button", name="Go to line 7")
        run.wait(0.8)
        run.key("Ctrl+s")
        run.wait(1.5)
        w2[1] = _log_len(run)
    w3 = [0, 0, run.app_log]
    with run.act("activate 'Save (button)' (AT-SPI; its handler sends the same intent)",
                 [printed_between("[action] Save", w3)], record=1.0):
        w3[0] = _log_len(run)
        run.action("click", role="push button", name="Save (button)")
        run.wait(1.5)
        w3[1] = _log_len(run)
    run.key("F10")
    run.wait(0.6)
    run.key("Down")
    run.wait(1.0)
    note_chain(run, "shortcuts-demo, File open", "MenuItem", "Save")
    note_roots(run, "shortcuts-demo, File open")


def menu_intents_cmb(run):
    run.wait_for(role="push button", name="Menu")
    w1 = [0, 0, run.app_log]
    with run.act("Tab, Space (reveal), Down (File), AT-SPI click 'New' (send_intent)",
                 [printed_between("New", w1)], record=1.0):
        w1[0] = _log_len(run)
        run.key("Tab")
        run.wait(0.5)
        run.key("space")
        run.wait(0.8)
        run.key("Down")
        run.wait(1.0)
        run.action("click", role="menu item", name="New")
        run.wait(1.5)
        w1[1] = _log_len(run)
    with run.act("Escape, Escape", record=1.0):
        run.key("Escape")
        run.wait(0.5)
        run.key("Escape")
    w2 = [0, 0, run.app_log]
    with run.act("Tab to the hamburger, Space, Right (Edit), AT-SPI click 'Undo' (a println closure)",
                 [printed_between("Undo", w2)], record=1.0):
        w2[0] = _log_len(run)
        run.grab_focus(role="push button", name="Menu")
        run.wait(0.5)
        run.key("space")
        run.wait(0.8)
        run.key("Right")
        run.wait(1.0)
        run.action("click", role="menu item", name="Undo")
        run.wait(1.5)
        w2[1] = _log_len(run)
    w3 = [0, 0, run.app_log]
    with run.act("focus the slider (AT-SPI), Ctrl+N", [printed_between("New", w3)], record=1.0):
        w3[0] = _log_len(run)
        run.key("Escape")
        run.wait(0.5)
        run.grab_focus(role="slider", name="Bar width")
        run.wait(0.6)
        run.key("Ctrl+n")
        run.wait(1.5)
        w3[1] = _log_len(run)


def native_submenu_reopen(run):
    run.wait_for(role="push button", name="Add recent file")
    run.action("click", role="push button", name="Add recent file")
    run.wait(1.0)
    with run.act("Alt+F, click 'Open' (AT-SPI; an intent entry, closes the menu)",
                 record=2.0, tree=True):
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="Open")
    with run.act("Alt+F again", [focused(role="menu")],
                 should="File opens again", record=2.0):
        run.key("Alt+f")
    with run.act("Escape, Escape", record=1.5):
        run.key("Escape")
        run.wait(0.5)
        run.key("Escape")
    with run.act("Alt+F, click 'Open Recent', click 'document-1.txt' (AT-SPI)",
                 [in_tree(role="label", name="Opened document-1.txt")], record=2.0, tree=True):
        run.key("Alt+f")
        run.wait(1.0)
        run.action("click", role="menu item", name="Open Recent")
        run.wait(1.0)
        run.action("click", role="menu item", name="document-1.txt")
    with run.act("Alt+F after the submenu item", [focused(role="menu")],
                 should="File opens again", record=2.0):
        run.key("Alt+f")
    with run.act("Alt+F once more", [focused(role="menu")],
                 should="File opens (if the first Alt+F only cleared a stale open state)",
                 record=2.0):
        run.key("Alt+f")
    with run.act("Escape", record=1.5):
        run.key("Escape")
    with run.act("Alt+E", [focused(role="menu")], should="Edit opens", record=2.0):
        run.key("Alt+e")
    run.key("Escape")
    run.wait(0.5)
    run.key("Alt+f")
    run.wait(1.0)
    note_chain(run, "native-menu, File open", "MenuItem", "New")
    note_roots(run, "native-menu, File open")



def _defunct(role, name):
    def run(act):
        from reader_lib.checks import _walk
        hits = [n for n in _walk(act.tree) if n.get("role") == role
                and (n.get("name") or "") == name]
        if not hits:
            return False, [f"no [{role}] {name!r} in the tree"]
        return "defunct" not in hits[0].get("states", []), [
            f"[{role}] {name!r} states={hits[0].get('states')} "
            f"actions={[a.get('name') for a in hits[0].get('actions', [])]}"]
    return custom(f"[{role}] {name!r} is live (not defunct)", run, needs_tree=True)


def titlebar_toggle(run):
    run.wait_for(role="push button", name="Maximize")
    with run.act("click Maximize (AT-SPI)", [in_tree(role="push button", name="Restore")],
                 record=2.0, tree=True):
        run.action("click", role="push button", name="Maximize")
    with run.act("click Restore (AT-SPI)",
                 [in_tree(role="push button", name="Maximize"),
                  _defunct("push button", "Maximize")],
                 should="the Maximize button is back, as a live object", record=2.0, tree=True):
        run.action("click", role="push button", name="Restore")
    with run.act("click Maximize again (AT-SPI)",
                 [in_tree(role="push button", name="Restore")],
                 should="a screen reader's click on the returned button still works",
                 record=2.0, tree=True):
        run.action("click", role="push button", name="Maximize")



def _orca_locus(target: str):
    def run(act):
        moves = [l.text for l in act.orca if "Changing locus of focus" in l.text]
        bad = [m for m in moves if target in m.split(" to ", 1)[-1]]
        return not bad, moves or ["Orca did not move its locus of focus in this act"]
    return custom(f"Orca does not move its locus of focus to {target!r}", run, needs_orca=True)


def tips_tabbed(run):
    """A composite tip whose body holds a TabWidget, summoned by focus."""
    from reader_lib.checks import not_said
    run.wait_for(role="push button", name="Tabbed details")
    for i in range(2):
        run.grab_focus(role="push button", name="Plain among rich")
        with run.act(f"try {i + 1}: Tab to 'Province info', Tab to 'Tabbed details' "
                     "(each after 0.3 s), wait for its tip",
                     [not_said("Stats page tab"), _orca_locus("Stats")],
                     should="the reader hears 'Tabbed details'; a tab inside the tooltip is "
                            "not presented as if it had focus", record=3.0, settle=3.0):
            run.key("Tab")
            run.wait(0.3)
            run.key("Tab")
        with run.act(f"try {i + 1}: Escape", record=1.5):
            run.key("Escape")


SCENARIOS = [
    Scenario("verify-chrome-cmb-rereveal", CMB, cmb_rereveal,
             "the collapsible bar revealed a second and third time"),
    Scenario("verify-chrome-cmb-altv-revealed", CMB, cmb_altv_revealed,
             "Alt+V on an already revealed collapsible bar"),
    Scenario("verify-chrome-menu-reopen", SC, menu_reopen,
             "a menu-bar dropdown opened a second time"),
    Scenario("verify-chrome-shortcuts-nofocus-diag", SC, shortcuts_nofocus_diag,
             "a global shortcut with nothing focused, and the arena roots"),
    Scenario("verify-chrome-native-diag", NM, native_diag,
             "which in-window menu entries run in native-menu, and why"),
    Scenario("verify-chrome-tips-tab-arrival", TIPS, tips_tab_arrival,
             "snap-back after a real Tab arrival"),
    Scenario("verify-chrome-menu-intents-sc", SC, menu_intents_sc,
             "per-act: which paths reach shortcuts-demo's register_action"),
    Scenario("verify-chrome-menu-intents-cmb", CMB, menu_intents_cmb,
             "per-act: which paths reach collapsible-menu-bar's register_action"),
    Scenario("verify-chrome-tips-tabbed", TIPS, tips_tabbed,
             "a composite tip holding a TabWidget, summoned by focus"),
    Scenario("verify-chrome-titlebar-toggle", "title-bar-demo", titlebar_toggle,
             "Maximize, Restore, Maximize through AT-SPI"),
    Scenario("verify-chrome-native-submenu-reopen", NM, native_submenu_reopen,
             "Alt+F after an item chosen from File and from File > Open Recent"),
]
